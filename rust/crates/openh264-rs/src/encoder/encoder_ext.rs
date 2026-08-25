//! Port of the memory-allocation and layer-initialisation half of
//! `codec/encoder/core/src/encoder_ext.cpp`.
//!
//! `wels_encoder_ext.rs` already holds the parameter validation and the
//! parameter-set NAL writers from the same file; this module holds the rest of the
//! core encoder: `AcquireLayersNals`, `AllocStrideTables`, `InitMbListD`,
//! `InitDqLayers`, `RequestMemorySvc`, `GetMultipleThreadIdc`, `WelsInitEncoderExt`
//! and `WelsEncoderEncodeExt`.
//!
//! This is what baseline blockers **C** and **D** describe: before this module the
//! context was never built (`pSpsArray`/`pPPSArray` unallocated, `pCurDqLayer` null)
//! and `WelsEncoderEncodeExtRust` was a sketch that emitted no bytes.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

// Phase 4a: `pfMdCost`/`pfMeCost` are enum selectors, not interior pointers (F13).

#![deny(unsafe_code)]
use std::sync::atomic::{AtomicU16, Ordering};
use crate::encoder::picture::{RecPicId, RecPicPool, SrcPicId, SrcPicPool};
use crate::encoder::md::CostFamily;
use std::ffi::c_char;
use std::ptr::{null, null_mut};

use crate::api::codec_api::EUsageType::{CAMERA_VIDEO_REAL_TIME, SCREEN_CONTENT_REAL_TIME};
use crate::api::codec_api::SliceModeEnum;
use crate::api::codec_api::SliceModeEnum::{SM_SINGLE_SLICE, SM_SIZELIMITED_SLICE};
use crate::api::codec_api::RC_MODES::RC_OFF_MODE;
use crate::api::codec_api::{ELevelIdc, SSpatialLayerConfig};
use crate::decoder::nalu::g_ksLevelLimits;
use crate::encoder::encoder_context::{
    ctx_dq_idc_map, ctx_dq_layer, ctx_frame_bs, ctx_frame_bs_cur, ctx_ltr_at, ctx_mb_index_x,
    ctx_mb_index_y, ctx_mvd_cost_table, ctx_param, ctx_ref_list, ctx_vaa,
    ctx_rc_at,
    ctx_pps_array, ctx_sps_array,
    ctx_stride_enc_block_offset,
    ctx_subset_array,
    sWelsEncCtx, SDqIdc, SLogContext, SRefList, SStrideTables, BASE_DEPENDENCY_ID,
    ctx_func_list,
};
use crate::encoder::md::INTRA_4x4_MODE_NUM;
use crate::encoder::param_svc::{
    SExistingParasetList, SWelsSvcCodingParam, MB_WIDTH_LUMA, UNSPECIFIED_BIT_RATE,
};
use crate::encoder::param_svc::{PpsId, SpsId, SubsetSpsId};
use crate::encoder::svc_encode_slice::LayerSps;
use crate::encoder::paraset_strategy::{ParasetStrategy, PARA_SET_TYPE_AVCSPS, PARA_SET_TYPE_PPS};
use crate::api::codec_api::EParameterSetStrategy;
use crate::encoder::picture::SPicture;
use crate::encoder::slice_multi_threading::{
    MAX_DEPENDENCY_LAYER, MAX_SLICES_NUM, MAX_THREADS_NUM,
};
use crate::encoder::svc_enc_slice_segment::{GetInitialSliceNum, InitSlicePEncCtx};
use crate::encoder::svc_encode_slice::{InitSliceInLayer, WelsMbToSliceIdc};
use crate::encoder::svc_encode_slice::{ctx_sps, ctx_pps};
use crate::encoder::svc_encode_slice::{current_layer, set_current_layer};
use crate::encoder::svc_mode_decision::{
    LEFT_MB_POS, TOPLEFT_MB_POS, TOPRIGHT_MB_POS, TOP_MB_POS,
};
use crate::encoder::svc_motion_estimate::{FME_DEFAULT_FEATURE_INDEX, ME_DIA_CROSS, ME_DIA_CROSS_FME};
use crate::encoder::wels_preprocess::AllocPicture;
use crate::encoder::svc_encode_slice::{
    LayerIdx, SDqLayer, SMB, MB_BLOCK4x4_NUM, MB_LUMA_CHROMA_BLOCK4x4_NUM,
};
use crate::safe::mb_grid::{MbArray, MbDims};
use crate::encoder::svc_motion_estimate::{
    CAMERA_HIGHLAYER_MVD_RANGE, CAMERA_MVD_RANGE, CAMERA_STARTMV_RANGE, EXPANDED_MVD_RANGE,
    EXPANDED_MV_RANGE,
};
use crate::encoder::wels_encoder_ext::{
    ENC_RETURN_CORRECTED, ENC_RETURN_MEMALLOCERR, ENC_RETURN_SUCCESS, ENC_RETURN_UNEXPECTED,
    ENC_RETURN_UNSUPPORTED_PARA, LEVEL_NUMBER,
    MAX_MACROBLOCK_SIZE_IN_BYTE, MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA,
    MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN, MIN_REF_PIC_COUNT,
};

// --- used by the encoding half (WelsEncoderEncodeExt and its helpers) ---
use crate::api::codec_api::{EVideoFrameType, SFrameBSInfo, SLayerBSInfo, SSourcePicture};
use crate::encoder::wels_encoder_ext::{NON_VIDEO_CODING_LAYER, VIDEO_CODING_LAYER};
use crate::common::wels_common_defs::{EWelsNalRefIdc, EWelsNalUnitType, EWelsSliceType};
use crate::encoder::param_svc::{SSpatialLayerInternal, INVALID_TEMPORAL_ID};
use crate::encoder::encoder_context::MAX_PPS_COUNT;
use crate::common::wels_common_defs::SNalUnitHeaderExt;
use crate::encoder::wels_encoder_ext::ENC_RETURN_MEMOVERFLOWFOUND;
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
use crate::encoder::svc_motion_estimate::{
    PSearchMethodFunc, BLOCK_16x16, BLOCK_16x8, BLOCK_8x16, BLOCK_8x8, BLOCK_4x4, BLOCK_8x4,
    BLOCK_4x8, ME_DIA, ME_CROSS, ME_FULL,
};
use crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;
use crate::encoder::wels_preprocess::EStaticBlockIdc;
use crate::encoder::ref_list_mgr_svc::MAX_TEMPORAL_LAYER_NUM;

/// `SPS_BUFFER_SIZE` — `wels_const.h:82`.
pub const SPS_BUFFER_SIZE: i32 = 32;
/// `PPS_BUFFER_SIZE` — `wels_const.h:86`.
pub const PPS_BUFFER_SIZE: i32 = 16;
/// `SSEI_BUFFER_SIZE` — `wels_const.h:78`.
pub const SSEI_BUFFER_SIZE: i32 = 128;
/// `COMPRESS_RATIO_THR` — `wels_const.h:75`. Set to the size of the original data,
/// which is large enough considering MinCR.
pub const COMPRESS_RATIO_THR: f32 = 1.0;
/// `MAX_MACROBLOCK_SIZE_IN_BYTE_x2` — `wels_const.h:93`.
pub const MAX_MACROBLOCK_SIZE_IN_BYTE_x2: i32 = (MAX_MACROBLOCK_SIZE_IN_BYTE as i32) << 1;
/// `MAX_NAL_UNITS_IN_LAYER` — `wels_const.h`.
pub const MAX_NAL_UNITS_IN_LAYER: i32 = 128;
/// `MAX_LAYER_NUM_OF_FRAME` — `codec_app_def.h`.
pub const MAX_LAYER_NUM_OF_FRAME: i32 = 128;
/// `PADDING_LENGTH` — `expand_pic.h:49`, reference extension.
pub const PADDING_LENGTH: i32 = 32;
/// `MB_BLOCK8x8_NUM` — `wels_const_common.h:58`.
pub const MB_BLOCK8x8_NUM: usize = 4;

/// `WELS_ALIGN` — `macros.h`.
#[inline]
pub fn WELS_ALIGN(x: i32, n: i32) -> i32 {
    (x + n - 1) & !(n - 1)
}

/// `WELS_ROUND` — `macros.h`, for the float form used by `RequestMemorySvc`.
#[inline]
fn WELS_ROUND_f(x: f32) -> i32 {
    (x + 0.5) as i32
}

/// Allocation tag for `CMemoryAlign`; the C++ tags are diagnostic strings only.
macro_rules! tag {
    ($s:literal) => {
        concat!($s, "\0").as_ptr() as *const c_char
    };
}

/// `WelsGetEncBlockStrideOffset` — `decode_mb_aux.cpp:235`.
///
/// # Safety
/// `pBlock` must point to at least 24 writable `i32`s.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsGetEncBlockStrideOffset(pBlock: *mut i32, kiStrideY: i32, kiStrideUV: i32) {
    for j in 0..4i32 {
        let i = (j << 2) as usize;
        let k = (j & 0x01) << 1;
        let r = j & 0x02;
        *pBlock.add(i) = (k + r * kiStrideY) << 2;
        *pBlock.add(i + 1) = (1 + k + r * kiStrideY) << 2;
        *pBlock.add(i + 2) = (k + (1 + r) * kiStrideY) << 2;
        *pBlock.add(i + 3) = (1 + k + (1 + r) * kiStrideY) << 2;

        let v = ((j & 0x01) + r * kiStrideUV) << 2;
        *pBlock.add(16 + j as usize) = v;
        *pBlock.add(20 + j as usize) = v;
    }
}

/// `AcquireLayersNals` — encoder_ext.cpp:749.
///
/// Counts the layers and the worst-case NAL units a frame can need, which sizes
/// `pOut->sNalList` and `pOut->sNalLen`.
///
/// # Safety
/// `ppCtx` must point to a live context whose `pFuncList->pParametersetStrategy` is
/// already set.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn AcquireLayersNals(
    ppCtx: *mut *mut sWelsEncCtx,
    pParam: *mut SWelsSvcCodingParam,
    pCountLayers: *mut i32,
    pCountNals: *mut i32,
) -> i32 {
    let mut iCountNumLayers: i32 = 0;
    let mut iCountNumNals: i32 = 0;
    let mut iDIndex: i32 = 0;

    if pParam.is_null() || ppCtx.is_null() || (*ppCtx).is_null() {
        return 1;
    }

    let iNumDependencyLayers = (*pParam).iSpatialLayerNum;

    loop {
        // S29: `&mut X as *mut T` is the defect with the cast already written.
        // The callee takes `*mut`, so the reference existed only to be discarded.
        let pDLayer = std::ptr::addr_of_mut!((*pParam).sSpatialLayers[iDIndex as usize]);
        let iOrgNumNals = iCountNumNals;

        // Note (Sep. 2010, upstream): the memory over-use here counts little towards
        // overall performance and should not be critical even on mobile.
        if SM_SIZELIMITED_SLICE == (*pDLayer).sSliceArgument.uiSliceMode {
            iCountNumNals += MAX_SLICES_NUM as i32;
            // plus prefix NALs
            if iDIndex == 0 {
                iCountNumNals += MAX_SLICES_NUM as i32;
            }
            // MAX_SLICES_NUM < MAX_LAYER_NUM_OF_FRAME ensured at svc_enc_slice_segment.h
            if iCountNumNals - iOrgNumNals > MAX_NAL_UNITS_IN_LAYER {
                return 1;
            }
        } else {
            let kiNumOfSlice = GetInitialSliceNum(&(*pDLayer).sSliceArgument);

            // NEED check iCountNals value in case multiple slices are used
            iCountNumNals += kiNumOfSlice; // for slice VCL NALs
            // plus prefix NALs
            if iDIndex == 0 {
                iCountNumNals += kiNumOfSlice;
            }
            debug_assert!(iCountNumNals - iOrgNumNals <= MAX_NAL_UNITS_IN_LAYER);
            if kiNumOfSlice > MAX_SLICES_NUM as i32 {
                return 1;
            }
        }

        if iCountNumNals - iOrgNumNals > MAX_NAL_UNITS_IN_LAYER {
            return 1;
        }

        iCountNumLayers += 1;

        iDIndex += 1;
        if iDIndex >= iNumDependencyLayers {
            break;
        }
    }

    // T6.I1: a `pFuncList.is_null()` guard was here; the table is owned now.
    // count parasets
    let Some(pStrategy) = (*ctx_func_list(*ppCtx)).pParametersetStrategy.as_mut() else {
        return 1;
    };
    iCountNumNals += 1
        + iNumDependencyLayers
        + (iCountNumLayers << 1)
        + iCountNumLayers // plus iCountNumLayers for reserved application
        + pStrategy.GetAllNeededParasetNum() as i32;

    // to check number of layers / nals / slices dependencies, 12/8/2010
    if iCountNumLayers > MAX_LAYER_NUM_OF_FRAME {
        return 1;
    }

    if !pCountLayers.is_null() {
        *pCountLayers = iCountNumLayers;
    }
    if !pCountNals.is_null() {
        *pCountNals = iCountNumNals;
    }
    0
}

/// `AllocStrideTables` — encoder_ext.cpp:1224.
///
/// # Safety
/// `ppCtx` must point to a live context with `pMemAlign` and `pSvcParam` set.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn AllocStrideTables(ppCtx: *mut *mut sWelsEncCtx, kiNumSpatialLayers: i32) -> i32 {
    let pParam = ctx_param(*ppCtx);

    // The C++ local `sMbSizeMap` is an array of a small anonymous struct.
    #[derive(Clone, Copy, Default)]
    struct SMbSizeMap {
        iMbWidth: i32,
        iCountMbNum: i32,
        iSizeAllMbAlignCache: i32,
    }
    let mut sMbSizeMap = [SMbSizeMap::default(); MAX_DEPENDENCY_LAYER];
    let mut iLineSizeY = [[0i32; 2]; MAX_DEPENDENCY_LAYER];
    let mut iLineSizeUV = [[0i32; 2]; MAX_DEPENDENCY_LAYER];
    let mut iMapSpatialIdx = [[0i32; 2]; MAX_DEPENDENCY_LAYER];
    let mut iCountLayersNeedCs = [0i32; 2];
    let kiUnit1Size: i32 = 24 * 4; // 24 * sizeof(int32_t)
    let mut iUnit2Size: i32 = 0;
    let mut i: i32;
    let mut iSpatialIdx: i32;
    let mut iTemporalIdx: i32;

    if kiNumSpatialLayers <= 0 || kiNumSpatialLayers > MAX_DEPENDENCY_LAYER as i32 {
        return 1;
    }

    // **T6.H1.** Two `WelsMallocz` calls stood here and at the `pBase` line below —
    // the table struct and the one block its sixteen pointers carved up. The struct
    // owns the block now, so both are one `Box::new(SStrideTables::new(..))` built
    // where the block's size is known, and the two `WelsFree`s they paired with are
    // the context's own drop. Nothing is installed into the context until the size
    // is computed, which is the only ordering change: the C++ installs the struct
    // first so an early `return 1` still frees it, and an owned table has no such
    // failure to protect against.
    let iCntTid = if (*pParam).iTemporalLayerNum > 1 { 2 } else { 1 };

    iSpatialIdx = 0;
    while iSpatialIdx < kiNumSpatialLayers {
        let kiTmpWidth = ((*pParam).sSpatialLayers[iSpatialIdx as usize].iVideoWidth + 15) >> 4;
        let kiTmpHeight = ((*pParam).sSpatialLayers[iSpatialIdx as usize].iVideoHeight + 15) >> 4;
        let mut iNumMb = kiTmpWidth * kiTmpHeight;

        sMbSizeMap[iSpatialIdx as usize].iMbWidth = kiTmpWidth;
        sMbSizeMap[iSpatialIdx as usize].iCountMbNum = iNumMb;

        iNumMb *= 2; // sizeof(int16_t)
        sMbSizeMap[iSpatialIdx as usize].iSizeAllMbAlignCache = iNumMb;
        iUnit2Size += iNumMb;

        iSpatialIdx += 1;
    }

    // Adaptive size_cs, size_fdec by implementation dependency
    iTemporalIdx = 0;
    while iTemporalIdx < iCntTid {
        let kbBaseTemporalFlag = usize::from(iTemporalIdx == 0);

        iSpatialIdx = 0;
        while iSpatialIdx < kiNumSpatialLayers {
            let fDlp = &(*pParam).sSpatialLayers[iSpatialIdx as usize];

            let kiWidthPad = WELS_ALIGN(fDlp.iVideoWidth, 16) + (PADDING_LENGTH << 1);
            iLineSizeY[iSpatialIdx as usize][kbBaseTemporalFlag] = WELS_ALIGN(kiWidthPad, 32);
            iLineSizeUV[iSpatialIdx as usize][kbBaseTemporalFlag] =
                WELS_ALIGN(kiWidthPad >> 1, 16);

            iMapSpatialIdx[iCountLayersNeedCs[kbBaseTemporalFlag] as usize][kbBaseTemporalFlag] =
                iSpatialIdx;
            iCountLayersNeedCs[kbBaseTemporalFlag] += 1;
            iSpatialIdx += 1;
        }
        iTemporalIdx += 1;
    }
    let iSizeDec = kiUnit1Size * (iCountLayersNeedCs[0] + iCountLayersNeedCs[1]);
    let iSizeEnc = kiUnit1Size * kiNumSpatialLayers;

    let iNeedAllocSize = iSizeDec + iSizeEnc + (iUnit2Size << 1);

    (**ppCtx).pStrideTab = Some(Box::new(SStrideTables::new(iNeedAllocSize)));
    let pPtr: &mut SStrideTables = (**ppCtx).pStrideTab.as_mut().unwrap();

    // The C++ carves the block with four running `uint8_t*` cursors. They are byte
    // *offsets* into the same block here, advanced by the same amounts in the same
    // order — the arithmetic below is unchanged, only its unit is.
    let mut pBaseDec: u32 = 0; // iCountLayersNeedCs
    let mut pBaseEnc: u32 = iSizeDec as u32; // iNumSpatialLayers
    let mut pBaseMbX: u32 = pBaseEnc + iSizeEnc as u32; // iNumSpatialLayers
    let mut pBaseMbY: u32 = pBaseMbX + iUnit2Size as u32; // iNumSpatialLayers

    iTemporalIdx = 0;
    while iTemporalIdx < iCntTid {
        let kbBaseTemporalFlag = usize::from(iTemporalIdx == 0);

        iSpatialIdx = 0;
        while iSpatialIdx < iCountLayersNeedCs[kbBaseTemporalFlag] {
            let kiActualSpatialIdx =
                iMapSpatialIdx[iSpatialIdx as usize][kbBaseTemporalFlag] as usize;
            let kiLumaWidth = iLineSizeY[kiActualSpatialIdx][kbBaseTemporalFlag];
            let kiChromaWidth = iLineSizeUV[kiActualSpatialIdx][kbBaseTemporalFlag];

            pPtr.pStrideDecBlockOffset[kiActualSpatialIdx][kbBaseTemporalFlag] = Some(pBaseDec);
            // T9.C4: the writable derivation, spelled here — `StrideDecBlockOffset`
            // is `&self`/`*const` now, and this is one of the four sites that fill
            // the block. Same arithmetic the accessor did: root + the offset just
            // stored.
            WelsGetEncBlockStrideOffset(
                pPtr.root().add(pBaseDec as usize).cast::<i32>(),
                kiLumaWidth,
                kiChromaWidth,
            );
            pBaseDec += kiUnit1Size as u32;

            iSpatialIdx += 1;
        }
        iTemporalIdx += 1;
    }
    iTemporalIdx = 0;
    while iTemporalIdx < iCntTid {
        let kbBaseTemporalFlag = usize::from(iTemporalIdx == 0);

        iSpatialIdx = 0;
        while iSpatialIdx < kiNumSpatialLayers {
            let mut iMatchIndex: i32 = 0;
            let mut bInMap = false;
            let mut bMatchFlag = false;

            i = 0;
            while i < iCountLayersNeedCs[kbBaseTemporalFlag] {
                let kiActualIdx = iMapSpatialIdx[i as usize][kbBaseTemporalFlag];
                if kiActualIdx == iSpatialIdx {
                    bInMap = true;
                    break;
                }
                if !bMatchFlag {
                    iMatchIndex = kiActualIdx;
                    bMatchFlag = true;
                }
                i += 1;
            }

            if bInMap {
                iSpatialIdx += 1;
                continue;
            }

            // not in the spatial map: assign the matching one to it. **This is the
            // aliasing the arena has to preserve** — two layers naming one region.
            pPtr.pStrideDecBlockOffset[iSpatialIdx as usize][kbBaseTemporalFlag] =
                pPtr.pStrideDecBlockOffset[iMatchIndex as usize][kbBaseTemporalFlag];

            iSpatialIdx += 1;
        }
        iTemporalIdx += 1;
    }

    iSpatialIdx = 0;
    while iSpatialIdx < kiNumSpatialLayers {
        let kiAllocMbSize = sMbSizeMap[iSpatialIdx as usize].iSizeAllMbAlignCache;

        pPtr.pStrideEncBlockOffset[iSpatialIdx as usize] = Some(pBaseEnc);

        pPtr.pMbIndexX[iSpatialIdx as usize] = Some(pBaseMbX);
        pPtr.pMbIndexY[iSpatialIdx as usize] = Some(pBaseMbY);

        pBaseEnc += kiUnit1Size as u32;
        pBaseMbX += kiAllocMbSize as u32;
        pBaseMbY += kiAllocMbSize as u32;

        iSpatialIdx += 1;
    }

    while iSpatialIdx < MAX_DEPENDENCY_LAYER as i32 {
        pPtr.pStrideDecBlockOffset[iSpatialIdx as usize][0] = None;
        pPtr.pStrideDecBlockOffset[iSpatialIdx as usize][1] = None;
        pPtr.pStrideEncBlockOffset[iSpatialIdx as usize] = None;
        pPtr.pMbIndexX[iSpatialIdx as usize] = None;
        pPtr.pMbIndexY[iSpatialIdx as usize] = None;

        iSpatialIdx += 1;
    }

    // initialize pMbIndexX and pMbIndexY tables as below

    // 4 loops for int16_t required, as introduced below
    let iMaxMbWidth = WELS_ALIGN(sMbSizeMap[(kiNumSpatialLayers - 1) as usize].iMbWidth, 4);
    let iRowSize = iMaxMbWidth * 2;

    // `pTmpRow` was a `WelsMallocz`/`WelsFree` pair scoped to this function — the
    // C++ scratch row both coordinate tables are stamped out of. A local `Vec` is
    // the same block with the same zeros and no free to forget.
    let mut sTmpRow = vec![0i16; (iRowSize as usize).div_ceil(std::mem::size_of::<i16>())];
    let pRowX = sTmpRow.as_mut_ptr();
    let pRowY = pRowX;
    // initialize pRowX & pRowY
    i = 0;
    let mut p = pRowX;
    while i < iMaxMbWidth {
        *p = i as i16;
        *p.add(1) = (1 + i) as i16;
        *p.add(2) = (2 + i) as i16;
        *p.add(3) = (3 + i) as i16;

        p = p.add(4);
        i += 4;
    }

    iSpatialIdx = kiNumSpatialLayers;
    loop {
        iSpatialIdx -= 1;
        if iSpatialIdx < 0 {
            break;
        }
        // T9.C4, as above.
        let mut pMbIndexX = match pPtr.pMbIndexX[iSpatialIdx as usize] {
            Some(off) => pPtr.root().add(off as usize).cast::<i16>(),
            None => std::ptr::null_mut(),
        };
        let kiMbWidth = sMbSizeMap[iSpatialIdx as usize].iMbWidth;
        let kiMbHeight = sMbSizeMap[iSpatialIdx as usize].iCountMbNum / kiMbWidth;

        i = 0;
        while i < kiMbHeight {
            std::ptr::copy_nonoverlapping(pRowX, pMbIndexX, kiMbWidth as usize);

            pMbIndexX = pMbIndexX.add(kiMbWidth as usize);
            i += 1;
        }
    }

    std::ptr::write_bytes(pRowY as *mut u8, 0, iRowSize as usize);
    let iMaxMbHeight = sMbSizeMap[(kiNumSpatialLayers - 1) as usize].iCountMbNum
        / sMbSizeMap[(kiNumSpatialLayers - 1) as usize].iMbWidth;
    i = 0;
    loop {
        let mut t = [0i16; 4];

        let mut j: i16 = 0;

        iSpatialIdx = kiNumSpatialLayers - 1;
        while iSpatialIdx >= 0 {
            let kiMbWidth = sMbSizeMap[iSpatialIdx as usize].iMbWidth;
            let kiMbHeight = sMbSizeMap[iSpatialIdx as usize].iCountMbNum / kiMbWidth;
            // T9.C4, as above.
            let pMbIndexY = match pPtr.pMbIndexY[iSpatialIdx as usize] {
                Some(off) => pPtr.root().add(off as usize).cast::<i16>(),
                None => std::ptr::null_mut(),
            }
            .add((i * kiMbWidth) as usize);

            if i < kiMbHeight {
                std::ptr::copy_nonoverlapping(pRowY, pMbIndexY, kiMbWidth as usize);
            }
            iSpatialIdx -= 1;
        }
        i += 1;
        if i >= iMaxMbHeight {
            break;
        }

        // C++ builds a 4-element int16 row of the value `i` via two 32-bit stores.
        t[0] = i as i16;
        t[1] = i as i16;
        t[2] = i as i16;
        t[3] = i as i16;

        p = pRowY;
        while j < iMaxMbWidth as i16 {
            std::ptr::copy_nonoverlapping(t.as_ptr(), p, 4);

            p = p.add(4);
            j += 4;
        }
    }

    drop(sTmpRow);

    0
}

/// `GetMvMvdRange` — encoder_ext.cpp:1508.
///
/// # Safety
/// `pParam` must be initialised.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetMvMvdRange(
    pParam: *mut SWelsSvcCodingParam,
    iMvRange: *mut i32,
    iMvdRange: *mut i32,
) {
    let mut iMinLevelIdc = ELevelIdc::LEVEL_5_2;
    let iFixMvRange = if (*pParam).iUsageType as i32 != 0 {
        EXPANDED_MV_RANGE
    } else {
        CAMERA_STARTMV_RANGE
    };
    let iFixMvdRange = if (*pParam).iUsageType as i32 != 0 {
        EXPANDED_MVD_RANGE
    } else if (*pParam).iSpatialLayerNum == 1 {
        CAMERA_MVD_RANGE
    } else {
        CAMERA_HIGHLAYER_MVD_RANGE
    };
    for iLayer in 0..(*pParam).iSpatialLayerNum as usize {
        if ((*pParam).sSpatialLayers[iLayer].uiLevelIdc as i32) < iMinLevelIdc as i32 {
            iMinLevelIdc = (*pParam).sSpatialLayers[iLayer].uiLevelIdc;
        }
    }
    let mut idx = 0usize;
    while g_ksLevelLimits[idx].uiLevelIdc != ELevelIdc::LEVEL_5_2 as u8
        && g_ksLevelLimits[idx].uiLevelIdc != iMinLevelIdc as u8
        && idx + 1 < LEVEL_NUMBER
    {
        idx += 1;
    }
    let iMinMv = (g_ksLevelLimits[idx].iMinVmv as i32) >> 2;
    let iMaxMv = (g_ksLevelLimits[idx].iMaxVmv as i32) >> 2;

    *iMvRange = std::cmp::min(iMinMv.abs(), iMaxMv);
    *iMvRange = std::cmp::min(*iMvRange, iFixMvRange);

    *iMvdRange = (*iMvRange + 1) << 1;
    *iMvdRange = std::cmp::min(*iMvdRange, iFixMvdRange);
}

/// `InitMbInfo` — encoder_ext.cpp:835 (file-static).
///
/// Computes every `SMB`'s position and neighbour-availability mask.
///
/// **T6.C1 deleted the wiring half.** The C++ also points each macroblock's five
/// scratch pointers at slots of five context-wide arrays, `pMvUnitBlock4x4` and
/// `pRefIndexBlock4x4` in two banks selected by layer parity
/// (`kiOffset = (kiDlayerId & 1) * kiMaxMbNum`) so that a layer and the layer it
/// predicts from never share a slot. The five arrays are inline in `SMB` now, so
/// every macroblock of every layer owns its own row — the banks' guarantee and
/// more — and `kiDlayerId`/`kiMaxMbNum` no longer select anything. See the session
/// C log entry for the ruling and the configuration it rests on.
///
/// # Safety
/// `pEnc` must have `pStrideTab` allocated; `pList` must hold at least
/// `iMbWidth * iMbHeight` entries.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
unsafe fn InitMbInfo(
    pEnc: *mut sWelsEncCtx,
    pLayer: &mut SDqLayer,
    kiDlayerId: i32,
) {
    let iMbWidth = (*pLayer).iMbWidth as i32;
    let iMbHeight = (*pLayer).iMbHeight as i32;
    let iMbNum = iMbWidth * iMbHeight;
    let mut mbs = crate::encoder::svc_encode_slice::mb_window(pLayer, 0, iMbNum, 0);

    for iIdx in 0..iMbNum as usize {
        let pMb = mbs.at_mut(iIdx);

        pMb.iMbX = *ctx_mb_index_x(pEnc, kiDlayerId as usize).add(iIdx);
        pMb.iMbY = *ctx_mb_index_y(pEnc, kiDlayerId as usize).add(iIdx);
        pMb.iMbXY = iIdx as i32;

        // [0..65535] > 36864 of LEVEL5.2
        let uiSliceIdc: u16 = WelsMbToSliceIdc(pLayer, iIdx as i32);
        let iLeftXY = iIdx as i32 - 1;
        let iTopXY = iIdx as i32 - iMbWidth;
        let iLeftTopXY = iTopXY - 1;
        let iRightTopXY = iTopXY + 1;

        let bLeft = pMb.iMbX > 0 && uiSliceIdc == WelsMbToSliceIdc(pLayer, iLeftXY);
        let bTop = pMb.iMbY > 0 && uiSliceIdc == WelsMbToSliceIdc(pLayer, iTopXY);
        let bLeftTop =
            pMb.iMbX > 0 && pMb.iMbY > 0 && uiSliceIdc == WelsMbToSliceIdc(pLayer, iLeftTopXY);
        let bRightTop = (pMb.iMbX as i32) < (iMbWidth - 1)
            && pMb.iMbY > 0
            && uiSliceIdc == WelsMbToSliceIdc(pLayer, iRightTopXY);

        let mut uiNeighborAvail: u8 = 0;
        if bLeft {
            uiNeighborAvail |= LEFT_MB_POS;
        }
        if bTop {
            uiNeighborAvail |= TOP_MB_POS;
        }
        if bLeftTop {
            uiNeighborAvail |= TOPLEFT_MB_POS;
        }
        if bRightTop {
            uiNeighborAvail |= TOPRIGHT_MB_POS;
        }
        // merged from svc_hd_opt_b for multiple slices coding
        pMb.uiSliceIdc = uiSliceIdc;
        pMb.uiNeighborAvail = uiNeighborAvail;

        // C++ recomputes uiNeighborAvail here for the base-MV neighbourhood, then
        // discards it — the result is never stored. Reproduced as a no-op comment
        // rather than dead code.
    }
}

/// `InitMbListD` — encoder_ext.cpp:907.
///
/// # Safety
/// `ppCtx` must point to a live context with `ppDqLayerList` populated.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitMbListD(ppCtx: *mut *mut sWelsEncCtx) -> i32 {
    let iNumDlayer = (*ctx_param(*ppCtx)).iSpatialLayerNum;

    if iNumDlayer > MAX_DEPENDENCY_LAYER as i32 {
        return 1;
    }

    // **One `MbArray` per layer, and that is not a change of ownership — T6.D5.**
    // The C++ allocated *one* flat block of `sum(iMbWidth * iMbHeight)` records and
    // cut it by cumulative size, handing each layer its cut as `sMbDataP` and
    // storing the same pointers a second time in `ppMbListD`. The cuts are disjoint,
    // contiguous, and exactly the layer's own macroblock count, so neither field was
    // a carrier: each layer already had sole use of its cut. Each layer now owns it
    // (`MbArray<SMB>`, `safe/mb_grid.rs`, legal because the layer is `Box`-built
    // since T6.D3 and the dimensions are the allocation's own — T5.E2's rule), and
    // `ppMbListD`, its two allocations and its free are gone.
    for i in 0..iNumDlayer as usize {
        let iMbWidth = ((*ctx_param(*ppCtx)).sSpatialLayers[i].iVideoWidth + 15) >> 4;
        let iMbHeight = ((*ctx_param(*ppCtx)).sSpatialLayers[i].iVideoHeight + 15) >> 4;
        let pLayer = ctx_dq_layer(*ppCtx, i);
        if pLayer.is_null() {
            return 1;
        }
        (*pLayer).sMbDataP = MbArray::new(
            MbDims::new(iMbWidth as usize, iMbHeight as usize),
            SMB::default(),
        );
        InitMbInfo(*ppCtx, &mut *pLayer, i as i32);
    }

    0
}

/// `InitDqLayers` — encoder_ext.cpp:1008 (file-static inline).
///
/// **This is baseline blocker C.** It allocates the reference lists and DQ layers,
/// then `pSpsArray`/`pSubsetArray`/`pPPSArray`, and drives the parameter-set strategy
/// to fill them and set `iSpsNum`/`iSubsetSpsNum`/`iPpsNum`.
///
/// # Safety
/// `ppCtx` must point to a live context with `pMemAlign`, `pSvcParam`, `pStrideTab`,
/// `ppRefPicListExt`, `ppDqLayerList` and `pFuncList->pParametersetStrategy` set.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitDqLayers(
    ppCtx: *mut *mut sWelsEncCtx,
    pExistingParasetList: *mut SExistingParasetList,
) -> i32 {
    let mut pSps: *mut crate::encoder::param_svc::SWelsSPS = null_mut();
    let mut pSubsetSps: *mut crate::encoder::param_svc::SSubsetSps = null_mut();
    let mut iSpsId: i32 = 0;
    let mut iPpsId: u32 = 0;
    let mut iResult: i32;

    if ppCtx.is_null() || (*ppCtx).is_null() {
        return 1;
    }

    let pParam = ctx_param(*ppCtx);
    let iDlayerCount = (*pParam).iSpatialLayerNum;
    let iNumRef = (*pParam).iMaxNumRefFrame as u32;

    // FME_DEFAULT_FEATURE_INDEX / ME_DIA_CROSS / ME_DIA_CROSS_FME, screen content only
    let kiFeatureStrategyIndex: i32 = FME_DEFAULT_FEATURE_INDEX as i32;
    let kiMe16x16: i32 = ME_DIA_CROSS as i32;
    let kiMe8x8: i32 = ME_DIA_CROSS_FME as i32;
    let kiNeedFeatureStorage = if (*pParam).iUsageType != SCREEN_CONTENT_REAL_TIME {
        0
    } else {
        (kiFeatureStrategyIndex << 16) + ((kiMe16x16 & 0x00FF) << 8) + (kiMe8x8 & 0x00FF)
    };

    let mut iDlayerIndex: i32 = 0;
    while iDlayerIndex < iDlayerCount {
        let mut i: u32 = 0;
        let kiWidth = (*pParam).sSpatialLayers[iDlayerIndex as usize].iVideoWidth;
        let kiHeight = (*pParam).sSpatialLayers[iDlayerIndex as usize].iVideoHeight;
        // with iWidth of horizon
        let mut iPicWidth = WELS_ALIGN(kiWidth, MB_WIDTH_LUMA) + (PADDING_LENGTH << 1);
        let mut iPicChromaWidth = iPicWidth >> 1;

        // 32 (or 16 for chroma below) to match the original implementation here rather
        // than iCacheLineSize
        iPicWidth = WELS_ALIGN(iPicWidth, 32);
        iPicChromaWidth = WELS_ALIGN(iPicChromaWidth, 16);

        // T9.C4, as above — the fourth and last writer, and the one that reaches
        // the tables through the context rather than through `AllocStrideTables`'
        // own `&mut`.
        let pEncBlockOffset = {
            let tab = (**ppCtx).pStrideTab.as_mut().expect("pStrideTab allocated");
            match tab.pStrideEncBlockOffset[iDlayerIndex as usize] {
                Some(off) => tab.root().add(off as usize).cast::<i32>(),
                None => std::ptr::null_mut(),
            }
        };
        WelsGetEncBlockStrideOffset(pEncBlockOffset, iPicWidth, iPicChromaWidth);

        // Reference list. **`Box`-built with a real constructor since T6.F1** — it
        // owns this layer's reconstruction pool now, and a `WelsMallocz`'d shell is UB
        // at the pool's first drop (S21). It is still reached through a raw pointer in
        // the zeroed context, exactly as `SDqLayer` is.
        let mut pending: Vec<Box<SPicture>> = Vec::new();
        loop {
            // use the actual size of the current layer
            let Some(pPic) = AllocPicture(
                kiWidth,
                kiHeight,
                true,
                if iDlayerIndex == iDlayerCount - 1 {
                    kiNeedFeatureStorage
                } else {
                    0
                },
            ) else {
                return 1;
            };
            pending.push(pPic);
            i += 1;
            if i >= 1 + iNumRef {
                break;
            }
        }

        let mut pRefListBox = SRefList::new();
        pRefListBox.pRef = RecPicPool::new(pending);
        pRefListBox.pNextBuffer = Some(pRefListBox.pRef.at(0));
        // The autoref is explicit because the place is behind a raw pointer: this is a
        // write into the context's own `Vec` header, not into the list's allocation.
        (&mut (**ppCtx).ppRefPicListExt)[iDlayerIndex as usize] = Some(pRefListBox);
        iDlayerIndex += 1;
    }

    iDlayerIndex = 0;
    while iDlayerIndex < iDlayerCount {
        // S29's named shape — `&mut X as *mut T` is the defect with the cast already
        // written: the reference retags before the cast discards it, and the tag is
        // what `InitSliceInLayer` used to pop. `addr_of_mut!` derives from the raw
        // parent and creates no reference at all.
        let pDlayer = std::ptr::addr_of_mut!((*pParam).sSpatialLayers[iDlayerIndex as usize]);
        let pParamInternal = std::ptr::addr_of_mut!((*pParam).sDependencyLayers[iDlayerIndex as usize]);
        let kiMbW = ((*pDlayer).iVideoWidth + 0x0f) >> 4;
        let kiMbH = ((*pDlayer).iVideoHeight + 0x0f) >> 4;

        (*pParamInternal).iCodingIndex = 0;
        (*pParamInternal).iFrameIndex = 0;
        (*pParamInternal).iFrameNum = 0;
        (*pParamInternal).iPOC = 0;
        (*pParamInternal).uiIdrPicId = 0;
        (*pParamInternal).bEncCurFrmAsIdrFlag = true; // make sure the first frame is IDR

        // **`Box`, not `WelsMallocz` — T6.D3, and it is the enabler for everything
        // this session's later faces do.** `SDqLayer` is reached only through a
        // pointer in the zeroed context (`ppDqLayerList[i]`), which is T3.6's `pOut`
        // precedent: the context's zeroing never reaches these fields, so a
        // `Box`-built layer may own `Vec`/`MbArray` where an inline-in-a-zeroed-block
        // struct may not (S21, read the other way). `SDqLayer::new` writes every
        // field the zero block used to stand in for.
        // **T6.H8**: `Box::into_raw` stood here and the slot took the raw pointer.
        // The slot owns the `Box` now; every consumer still gets the same cursor out
        // of `ctx_dq_layer`, and this function keeps one for the rest of the loop.
        let pDqLayerBox = Box::new(SDqLayer::new(LayerIdx(iDlayerIndex as u8)));
        (&mut (**ppCtx).ppDqLayerList)[iDlayerIndex as usize] = Some(pDqLayerBox);
        let pDqLayer = ctx_dq_layer(*ppCtx, iDlayerIndex as usize);

        (*pDqLayer).iMbWidth = kiMbW as i16;
        (*pDqLayer).iMbHeight = kiMbH as i16;

        let mut iMaxSliceNum: i32 = 1;
        let kiSliceNum = GetInitialSliceNum(&(*pDlayer).sSliceArgument);
        if iMaxSliceNum < kiSliceNum {
            iMaxSliceNum = kiSliceNum;
        }
        (*pDqLayer).iMaxSliceNum = iMaxSliceNum;

        iResult = InitSliceInLayer(*ppCtx, &mut *pDqLayer, iDlayerIndex);
        if iResult != 0 {
            return iResult;
        }

        // deblocking parameters initialization; target-layer deblocking
        (*pDqLayer).iLoopFilterDisableIdc = (*pParam).iLoopFilterDisableIdc as u8;
        (*pDqLayer).iLoopFilterAlphaC0Offset = ((*pParam).iLoopFilterAlphaC0Offset << 1) as i8;
        (*pDqLayer).iLoopFilterBetaOffset = ((*pParam).iLoopFilterBetaOffset << 1) as i8;
        // parallel deblocking
        (*pDqLayer).bDeblockingParallelFlag = (*pParam).bDeblockingParallelFlag;

        // deblocking parameter adjustment
        if SM_SINGLE_SLICE == (*pDlayer).sSliceArgument.uiSliceMode {
            // iLoopFilterDisableIdc will be 0 or 1 under single slice
            if 2 == (*pParam).iLoopFilterDisableIdc {
                (*pDqLayer).iLoopFilterDisableIdc = 0;
            }
            (*pDqLayer).bDeblockingParallelFlag = false;
        } else {
            // multi-slice
            if 0 == (*pDqLayer).iLoopFilterDisableIdc {
                (*pDqLayer).bDeblockingParallelFlag = false;
            }
        }

        // Screen-content feature search storage is not ported; C++ allocates
        // pFeatureSearchPreparation here when kiNeedFeatureStorage is set, which only
        // happens for SCREEN_CONTENT_REAL_TIME, and this returns before that. The
        // field it used to null out is gone with the machinery behind it (T6.D2).
        if kiNeedFeatureStorage != 0 && iDlayerIndex == iDlayerCount - 1 {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }

        iDlayerIndex += 1;
    }

    // dynamically allocate parameter-set memory instead of the standard's maximum, to
    // reduce size (3/18/2010)
    // T6.I1: a `pFuncList.is_null()` guard was here; the table is owned now.
    // The borrow is re-acquired at each use rather than held across the loop below:
    // `GenerateNewSps`/`InitPps` take `*ppCtx`, and reaching the strategy through the
    // context while a `&mut` to it is live would alias. Same reason as
    // `WelsWriteParameterSets`; T4b.2a.
    if (*ctx_func_list(*ppCtx)).pParametersetStrategy.is_none() {
        return 1;
    }
    let kiNeededSpsNum = ParasetStrategy(*ppCtx).GetNeededSpsNum() as i32;
    let kiNeededSubsetSpsNum = ParasetStrategy(*ppCtx).GetNeededSubsetSpsNum() as i32;
    // **T6.H2.** Three `WelsMallocz` calls and their three null checks were here.
    // The lengths are the strategy's own numbers, unchanged; the entries are the
    // zeros `WelsMallocz` left, spelled as `ZERO` rather than `Default` because
    // `SWelsSPS::default()` seeds `uiProfileIdc = PRO_BASELINE` and three VUI
    // `*_UNDEF`s, and none of those is what a memset writes (F56: zeros are ruled).
    (**ppCtx).pSpsArray = vec![crate::encoder::param_svc::SWelsSPS::ZERO; kiNeededSpsNum as usize];
    // The `else` arm was `pSubsetArray = null_mut()` — no allocation at all when the
    // configuration needs no subset SPS. An empty `Vec` is that, and `ctx_subset_array`
    // answers the same null for it.
    (**ppCtx).pSubsetArray = vec![
        crate::encoder::param_svc::SSubsetSps::ZERO;
        kiNeededSubsetSpsNum.max(0) as usize
    ];

    // PPS
    let kiNeededPpsNum = ParasetStrategy(*ppCtx).GetNeededPpsNum() as i32;
    (**ppCtx).pPPSArray = vec![crate::encoder::param_svc::SWelsPPS::ZERO; kiNeededPpsNum as usize];

    ParasetStrategy(*ppCtx).LoadPrevious(
        pExistingParasetList,
        ctx_sps_array(*ppCtx),
        ctx_subset_array(*ppCtx),
        ctx_pps_array(*ppCtx),
    );

    // **T6.H3.** `SDqIdc` is four bytes of POD and its derived `Default` is the
    // memset image field for field, so `Default` *is* the ruled zero here — unlike
    // `SWelsSPS`'s two lines up.
    (**ppCtx).pDqIdcMap = vec![SDqIdc::default(); iDlayerCount as usize];

    iDlayerIndex = 0;
    while iDlayerIndex < iDlayerCount {
        let pDqIdc = ctx_dq_idc_map(*ppCtx).add(iDlayerIndex as usize);
        let bUseSubsetSps = !(*pParam).bSimulcastAVC && (iDlayerIndex > BASE_DEPENDENCY_ID as i32);
        // S29, and the second site the encoder probe reached: `paraset_strategy.rs`
        // re-derives this same layer inside `GenerateNewSps` below, which popped
        // this binding's Unique tag before `InitSlicePEncCtx` read through it.
        let pDlayerParam = std::ptr::addr_of_mut!((*pParam).sSpatialLayers[iDlayerIndex as usize]);
        let bSvcBaselayer = !(*pParam).bSimulcastAVC
            && (iDlayerCount > BASE_DEPENDENCY_ID as i32)
            && (iDlayerIndex == BASE_DEPENDENCY_ID as i32);
        (*pDqIdc).uiSpatialId = iDlayerIndex as i8;

        iSpsId = ParasetStrategy(*ppCtx).GenerateNewSps(
            *ppCtx,
            bUseSubsetSps,
            iDlayerIndex,
            iDlayerCount,
            iSpsId as u32,
            bSvcBaselayer,
        ) as i32;
        if 0 > iSpsId {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        // T6.G3: `GenerateNewSps` used to hand these back through two
        // pointer-to-pointer out-parameters, and this block then recomputed the
        // selected one from `iSpsId` anyway — the id was already the carrier and the
        // pointers were a second copy of it. Both arms are derived here now, in the
        // spelling the callee used, including the subset arm's inner SPS, which
        // lines 945-946 below read and which this block did *not* previously
        // reassign.
        if !bUseSubsetSps {
            pSps = ctx_sps_array(*ppCtx).add(iSpsId as usize);
        } else {
            pSubsetSps = ctx_subset_array(*ppCtx).add(iSpsId as usize);
            pSps = std::ptr::addr_of_mut!((*pSubsetSps).pSps);
        }

        iPpsId = ParasetStrategy(*ppCtx).InitPps(
            *ppCtx,
            iSpsId as u32,
            // T6.G3: `InitPps` takes the arm it will actually use, not both plus a
            // flag. The two locals are still raw here — they are cursors into the
            // context's arrays, which are session H's — so the reference is formed
            // at this one boundary, where the branch above has just proved which of
            // them is live.
            pSps.as_ref(),
            pSubsetSps.as_ref(),
            iPpsId,
            true,
            bUseSubsetSps,
            (*pParam).iEntropyCodingModeFlag != 0,
        );
        let pPps = ctx_pps_array(*ppCtx).add(iPpsId as usize);

        // FMO is not used in SVC coding so far; come back if FMO is needed
        iResult = InitSlicePEncCtx(
            &mut *ctx_dq_layer(*ppCtx, iDlayerIndex as usize),
            false,
            (*pSps).iMbWidth as i32,
            (*pSps).iMbHeight as i32,
            std::ptr::addr_of_mut!((*pDlayerParam).sSliceArgument),
        );
        if iResult != 0 {
            return iResult;
        }
        (*pDqIdc).iSpsId = iSpsId as u8;
        (*pDqIdc).iPpsId = iPpsId as u16;

        if (*pParam).bSimulcastAVC || bUseSubsetSps {
            iSpsId += 1;
        }
        iPpsId += 1;
        if bUseSubsetSps {
            (**ppCtx).iSubsetSpsNum += 1;
        } else {
            (**ppCtx).iSpsNum += 1;
        }
        (**ppCtx).iPpsNum += 1;

        iDlayerIndex += 1;
    }

    ParasetStrategy(*ppCtx).UpdateParaSetNum(*ppCtx);
    ENC_RETURN_SUCCESS
}

/// `RequestMemorySvc` — encoder_ext.cpp:1533.
///
/// Sizes and allocates everything the encoder needs for a frame, then calls
/// [`InitDqLayers`] and [`InitMbListD`].
///
/// **Deviations, all explicit** — and the list is shorter than it was (T8b.B4):
/// * the screen-content VAA extension (`RequestMemoryVaaScreen`) returns
///   `ENC_RETURN_UNSUPPORTED_PARA` (`:1199`). Phase 10.
/// * the adaptive-quantisation buffers return it too (`:1210`). The *plugin* is
///   ported (`processing/adaptive_quantization.rs`); these are the encoder-side
///   `sAdaptiveQuantParam` blocks, which are not.
/// * **the background-detection buffers are ported** — the sentence that said they
///   were not has been wrong since T6.F3, and the statement ten lines below it says
///   so: `SVAAFrameInfo::new` takes `bEnableBackgroundDetection` and builds the pair
///   exactly when the C++ allocates it.
/// * `RequestMtResource` is reached with `iMultipleThreadIdc > 1` and is ported
///   (Phase 7); it is a branch, not a refusal.
///
/// # Safety
/// `ppCtx` must point to a live context with `pMemAlign`, `pSvcParam` and
/// `pFuncList->pParametersetStrategy` set.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn RequestMemorySvc(
    ppCtx: *mut *mut sWelsEncCtx,
    pExistingParasetList: *mut SExistingParasetList,
) -> i32 {
    let pParam = ctx_param(*ppCtx);
    let mut iCountNals: i32 = 0;
    let mut iCountLayers: i32 = 0;
    let mut iResult: i32;
    let kiNumDependencyLayers = (*pParam).iSpatialLayerNum;
    let mut iVclLayersBsSizeCount: i32 = 0;

    if kiNumDependencyLayers < 1 || kiNumDependencyLayers > MAX_DEPENDENCY_LAYER as i32 {
        return 1;
    }

    if (*pParam).uiGopSize == 0
        || ((*pParam).uiIntraPeriod != 0 && ((*pParam).uiIntraPeriod % (*pParam).uiGopSize) != 0)
    {
        return 1;
    }

    let pFinalSpatial = &(*pParam).sSpatialLayers[(kiNumDependencyLayers - 1) as usize];
    let iMaxPicWidth = pFinalSpatial.iVideoWidth;
    let iMaxPicHeight = pFinalSpatial.iVideoHeight;
    let iCountMaxMbNum = ((15 + iMaxPicWidth) >> 4) * ((15 + iMaxPicHeight) >> 4);

    iResult = AcquireLayersNals(ppCtx, pParam, &mut iCountLayers, &mut iCountNals);
    if iResult != 0 {
        return 1;
    }

    let kiSpsSize = ParasetStrategy(*ppCtx).GetNeededSpsNum() as i32 * SPS_BUFFER_SIZE;
    let kiPpsSize = ParasetStrategy(*ppCtx).GetNeededPpsNum() as i32 * PPS_BUFFER_SIZE;
    let iNonVclLayersBsSizeCount = SSEI_BUFFER_SIZE + kiSpsSize + kiPpsSize;

    let mut bDynamicSlice = false;
    let mut iSliceBufferSize: i32 = 0;
    let mut iMaxSliceBufferSize: i32 = 0;
    let mut iIndex: i32 = 0;
    while iIndex < (*pParam).iSpatialLayerNum {
        let fDlp = &(*pParam).sSpatialLayers[iIndex as usize];

        let fCompressRatioThr = COMPRESS_RATIO_THR;

        let mut iLayerBsSize = WELS_ROUND_f(
            (((3 * fDlp.iVideoWidth * fDlp.iVideoHeight) >> 1) as f32) * fCompressRatioThr,
        ) + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
        iLayerBsSize = WELS_ALIGN(iLayerBsSize, 4); // 4 bytes aligned
        let mut iMaxLayerBsSize: i32;
        let pSliceArgument = &fDlp.sSliceArgument;
        if pSliceArgument.uiSliceMode == SM_SIZELIMITED_SLICE {
            bDynamicSlice = true;
            let uiMaxSliceNumEstimation = std::cmp::min(
                crate::encoder::svc_enc_slice_segment::AVERSLICENUM_CONSTRAINT as u32,
                (iLayerBsSize as u32 / pSliceArgument.uiSliceSizeConstraint) + 1,
            );
            (**ppCtx).iMaxSliceCount =
                std::cmp::max((**ppCtx).iMaxSliceCount, uiMaxSliceNumEstimation as i32);
            iSliceBufferSize = ((std::cmp::max(
                pSliceArgument.uiSliceSizeConstraint,
                iLayerBsSize as u32 / uiMaxSliceNumEstimation,
            ) as i32)
                << 1)
                + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
            iMaxLayerBsSize = iSliceBufferSize * uiMaxSliceNumEstimation as i32;
        } else {
            (**ppCtx).iMaxSliceCount =
                std::cmp::max((**ppCtx).iMaxSliceCount, pSliceArgument.uiSliceNum as i32);
            if (*pParam).bUseLoadBalancing {
                iSliceBufferSize = iLayerBsSize + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
            } else {
                iSliceBufferSize = ((iLayerBsSize / pSliceArgument.uiSliceNum as i32) << 1)
                    + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
            }
            iMaxLayerBsSize = iSliceBufferSize * pSliceArgument.uiSliceNum as i32;
        }
        iMaxLayerBsSize = std::cmp::max(iMaxLayerBsSize, iLayerBsSize);
        iVclLayersBsSizeCount += iMaxLayerBsSize;
        iMaxSliceBufferSize = std::cmp::max(iMaxSliceBufferSize, iSliceBufferSize);
        (**ppCtx).iSliceBufferSize[iIndex as usize] = iSliceBufferSize;
        iIndex += 1;
    }
    let iTargetSpatialBsSize = iVclLayersBsSizeCount;
    let iCountBsLen = iNonVclLayersBsSizeCount + iVclLayersBsSizeCount;

    iMaxSliceBufferSize = std::cmp::min(iMaxSliceBufferSize, iTargetSpatialBsSize);
    let iTotalLength = iCountBsLen;

    (*pParam).iNumRefFrame = crate::encoder::rc::WELS_CLIP3(
        (*pParam).iNumRefFrame,
        MIN_REF_PIC_COUNT,
        if (*pParam).iUsageType == CAMERA_VIDEO_REAL_TIME {
            MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA
        } else {
            MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN
        },
    );

    // Output.
    //
    // Four `WelsMallocz` calls and four null checks became one constructor —
    // **S21's construction audit is why**. The old shape wrote `Vec`-typed
    // fields into memory `WelsMallocz` had zeroed, and a zeroed `Vec` is not a
    // valid `Vec`: the assignment would drop it. `new_boxed` builds the struct
    // whole, so no zeroed intermediate exists to be dropped. The null checks go
    // because allocation failure is now a panic-on-OOM, the same trade the
    // decoder's owned buffers made.
    (**ppCtx).pOut = Box::into_raw(crate::encoder::nal_encap::SWelsEncoderOutput::new_boxed(
        iCountBsLen as usize,
        iCountNals as usize,
    ));

    // **T6.H4, and the session's one recorded zero-fill deviation.** The C++ takes
    // this block with `WelsMalloc` — *uninitialized* — and it is the only member of
    // this function's set that does. `vec![0; n]` writes zeros the C++ does not.
    // Sound because every read of this buffer sits behind a write cursor: bytes are
    // read back only up to `iPosBsBuffer` / `pOut->iNalLen`, which only ever name
    // bytes a NAL writer has just written. A safe container has no uninitialized
    // alternative, so the cost is one memset per encoder, at init.
    (**ppCtx).pFrameBs = vec![0u8; iTotalLength.max(0) as usize];
    (**ppCtx).iFrameBsSize = iTotalLength;
    (**ppCtx).iPosBsBuffer = 0;

    // for dynamic slice mode && CABAC, allocate slice buffers to restore slice data.
    // These are `sDss.pRestoreBuffer` in the two dynamic MB loops: CABAC
    // renormalisation can rewrite bytes already emitted, so stepping back over a
    // slice boundary has to restore the bytes as well as the coder state.
    if bDynamicSlice && (*pParam).iEntropyCodingModeFlag != 0 {
        for iIdx in 0..MAX_THREADS_NUM {
            // **T7.C5 — owned.** The last live allocator call sites in `src/encoder`
            // were this one and its free below. `WelsMalloc` here was *uninitialized*
            // (not `WelsMallocz`), so `vec![0; n]` writes zeros the C++ does not — the
            // same recorded deviation `pFrameBs` above carries, and sound for the same
            // reason: every read of this buffer sits behind a write cursor, since
            // `StashPopMBStatus` only reads back the bytes `StashMBStatus` just wrote.
            (**ppCtx).pDynamicBsBuffer[iIdx] = vec![0u8; iMaxSliceBufferSize.max(0) as usize];
        }
    }
    // for pSlice bs buffers
    if (*pParam).iMultipleThreadIdc > 1
        && crate::encoder::slice_multi_threading::RequestMtResource(
            ppCtx,
            pParam,
            iCountBsLen,
            iMaxSliceBufferSize,
            bDynamicSlice,
        ) != 0
    {
        return 1;
    }

    // T4b.2b: the factory allocated an object whose only member was a back-pointer to
    // this context, so there is nothing left to allocate and nothing left to fail --
    // the `is_null()` check went with the allocation. **S23**: neither selector can
    // change behind this choice; see `RefStrategyKind::Select`.
    (**ppCtx).eRefStrategy = crate::encoder::ref_list_mgr_svc::RefStrategyKind::Select(
        (*pParam).iUsageType,
        (*pParam).bEnableLongTermReference,
    );

    // encoder_ext.cpp:1141-1179 allocates five context-wide per-macroblock arrays
    // here -- `pIntra4x4PredModeBlocks`, `pNonZeroCountBlocks`, `pMvUnitBlock4x4`
    // (two banks), `pRefIndexBlock4x4` (two banks) and `pSadCostMb` -- and
    // `InitMbInfo` points each `SMB`'s five pointers into them. **T6.C1** made all
    // five inline arrays of `SMB`, which is allocated (and zeroed) by `InitMbListD`,
    // so there is nothing left to allocate and nothing left to fail.

    (**ppCtx).iGlobalQp = 26; // global qp in default

    // **T6.H5.** `SLTRState::default()` is all-zero field for field, which is what
    // `WelsMallocz` left; `ResetLtrState` then writes the four `-1`s and the
    // `LTR_DIRECT_MARK` that make it a *state* rather than a zeroed block, exactly as
    // before — the loop is unchanged.
    (**ppCtx).pLtr = vec![
        crate::encoder::ref_list_mgr_svc::SLTRState::default();
        kiNumDependencyLayers as usize
    ];
    for i in 0..kiNumDependencyLayers as usize {
        crate::encoder::ref_list_mgr_svc::ResetLtrState(ctx_ltr_at(*ppCtx, i));
    }

    // stride tables
    if AllocStrideTables(ppCtx, kiNumDependencyLayers) != 0 {
        return 1;
    }

    // Rate control module memory allocation; only malloc once for RC data (12/14/2009)
    // **T6.H6**: and one `Vec` for the states, which now own the five arrays
    // `RcInitLayerMemory` used to cut out of a second block each.
    // Built one at a time rather than with `vec![x; n]`, which would need
    // `SWelsSvcRc: Clone`, and the derive is not there: T9.C5 dropped it over
    // `pGomCost`, and D-dead-3 has since deleted that field. Nothing in the tree
    // clones a rate controller, so re-deriving it would buy this one line and
    // re-open the invitation; see the struct's own note in `rc.rs`.
    (**ppCtx).pWelsSvcRc = (0..kiNumDependencyLayers as usize)
        .map(|_| crate::encoder::rc::SWelsSvcRc::default())
        .collect();

    // pVaa memory allocation
    if (*pParam).iUsageType == SCREEN_CONTENT_REAL_TIME {
        // encoder_ext.cpp:1708, SVAAFrameInfoExt + RequestMemoryVaaScreen. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    // **T6.F3**: one constructor where the C++ cuts seven `CMemoryAlign` blocks.
    // `SVAAFrameInfo` is `Box`-built and owns its per-frame result arrays; the
    // background-detection pair exists exactly when the C++ allocates it.
    // **T6.H10**: `Box::into_raw` stood here; the context holds the `Box`.
    (**ppCtx).pVaa = Some(crate::encoder::wels_preprocess::SVAAFrameInfo::new(
        iCountMaxMbNum,
        (*ctx_param(*ppCtx)).bEnableBackgroundDetection,
    ));

    if (*ctx_param(*ppCtx)).bEnableAdaptiveQuant {
        // encoder_ext.cpp:1720, sAdaptiveQuantParam buffers. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // End of pVaa memory allocation

    // **T6.H7.** A `WelsMallocz`'d block of `kiNumDependencyLayers` null pointers,
    // which `InitDqLayers` then fills with `Box::into_raw`'d lists. `None` is that
    // null, and the `Box` stays where it already was — it just has an owner now.
    (**ppCtx).ppRefPicListExt = (0..kiNumDependencyLayers).map(|_| None).collect();

    // **T6.H8.** As `ppRefPicListExt` just above: a block of nulls that
    // `InitDqLayers` fills with `Box`-built layers, so a `Vec` of `None`s that it
    // fills with the `Box`es themselves.
    (**ppCtx).ppDqLayerList = (0..kiNumDependencyLayers).map(|_| None).collect();

    iResult = InitDqLayers(ppCtx, pExistingParasetList);
    if iResult != 0 {
        return iResult;
    }

    if InitMbListD(ppCtx) != 0 {
        return 1;
    }

    let mut iMvdRange: i32 = 0;
    GetMvMvdRange(pParam, &mut (**ppCtx).iMvRange, &mut iMvdRange);
    let kuiMvdInterTableSize = iMvdRange << 2; // intepel*4 = qpel
    let kuiMvdInterTableStride = 1 + (kuiMvdInterTableSize << 1); // qpel_mv_range*2 = (+/-)
    let kuiMvdCacheAlignedSize = kuiMvdInterTableStride * 2; // sizeof(uint16_t)

    (**ppCtx).iMvdCostTableSize = kuiMvdInterTableSize;
    (**ppCtx).iMvdCostTableStride = kuiMvdInterTableStride;
    // **F57, and it is F14's accommodation a second time (S12, S6-parity).**
    // `MvdCostInit` walks two cursors one stride per row for 52 rows. `pNegMvd`
    // starts at the table's base and ends exactly one past it, which is legal.
    // `pPosMvd` starts `(kiSz + 1)` elements in and advances by the same stride,
    // so after the 52nd row it lands `(kiSz + 1)` elements *beyond* the table —
    // 1042 bytes on this configuration. The pointer is formed and never
    // dereferenced, which is why nothing has ever observed it: it is UB in Rust
    // and in C alike, and the C++ upstream forms the same pointer.
    //
    // Sizing the buffer is the move that does not touch the kernel — F14's
    // reasoning exactly. The extra bytes are never read, never written and never
    // addressed except by the one bump this exists to keep in bounds, so no
    // encoded byte can move. Deleting the term restores the UB and the encoder
    // aliasing probe catches it, which is how it was found.
    let kuiMvdCostTableOvershoot = 2 * ((kuiMvdInterTableStride >> 1) + 1);
    // **T6.H9 — plan item P11.** The size above is in *bytes* (the C++ `WelsMalloc`
    // takes bytes and casts to `uint16_t*`), so the `Vec`'s length is that over two.
    (**ppCtx).pMvdCostTable = vec![
        0u16;
        (52 * kuiMvdCacheAlignedSize + kuiMvdCostTableOvershoot) as usize
            / std::mem::size_of::<u16>()
    ];
    crate::encoder::md::MvdCostInit(ctx_mvd_cost_table(*ppCtx), kuiMvdInterTableStride);

    let pRefList0 = ctx_ref_list(*ppCtx, 0);
    if !pRefList0.is_null() && !(*pRefList0).pRef.is_empty() {
        (**ppCtx).pDecPic = Some((*pRefList0).pRef.at(0));
    } else {
        (**ppCtx).pDecPic = None; // error here
    }

    // T6.G3: the head of each array, which is what "= pSpsArray" said. Nothing
    // re-aims these, in this port or in the C++ — `encoder_ext.cpp` assigns them here
    // and nowhere else — so the active set is position 0 for the encoder's whole life.
    (**ppCtx).iSps = Some(SpsId(0));
    (**ppCtx).iPps = Some(PpsId(0));

    0
}

/// `InitSliceSettings` — encoder_ext.cpp:2018.
///
/// Resolves the per-layer slice arguments, then derives `iMultipleThreadIdc` and the
/// maximum slice count from them.
///
/// # Safety
/// `pCodingParam` and `pMaxSliceCount` must be non-null.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceSettings(
    pLogCtx: *mut SLogContext,
    pCodingParam: *mut SWelsSvcCodingParam,
    kiCpuCores: i32,
    pMaxSliceCount: *mut i16,
) -> i32 {
    let mut iSpatialIdx: i32 = 0;
    let iSpatialNum = (*pCodingParam).iSpatialLayerNum;
    let mut iMaxSliceCount: u16 = 0;

    loop {
        let pDlp = &mut (*pCodingParam).sSpatialLayers[iSpatialIdx as usize]
            as *mut SSpatialLayerConfig;
        // **F70, and S29 again.** This was `&mut (*pDlp).sSliceArgument` — a `Unique`
        // retag over the whole slice-argument struct, held across the
        // `SM_FIXEDSLCNUM_SLICE` arm below, which takes a **second** `&mut` to the
        // same field for the validator and pops the first. The read at the bottom of
        // that arm then goes through a dead tag. `addr_of_mut!` creates no reference,
        // so there is no tag to pop and every read below is through the parameter
        // struct's own provenance.
        //
        // Found by the T7.B4 fork/join probe on its **first** Miri run — not because
        // the probe threads, but because it is the first test in this crate ever to
        // ask for `SM_FIXEDSLCNUM_SLICE`. This arm is what the diffharness drives on
        // 369 rows a sweep and what any multi-slice caller reaches; it has been UB
        // since the parameter validation was written, and no byte gate could see it.
        let pSliceArgument = std::ptr::addr_of_mut!((*pDlp).sSliceArgument);

        match (*pSliceArgument).uiSliceMode {
            SM_SIZELIMITED_SLICE => {
                iMaxSliceCount = crate::encoder::svc_enc_slice_segment::AVERSLICENUM_CONSTRAINT
                    as u16;
            }
            crate::api::codec_api::SliceModeEnum::SM_FIXEDSLCNUM_SLICE => {
                let iReturn =
                    crate::encoder::svc_enc_slice_segment::SliceArgumentValidationFixedSliceMode(
                        pLogCtx,
                        &mut (*pDlp).sSliceArgument,
                        (*pCodingParam).iRCMode,
                        (*pDlp).iVideoWidth,
                        (*pDlp).iVideoHeight,
                    );
                if iReturn != 0 {
                    return ENC_RETURN_UNSUPPORTED_PARA;
                }

                if (*pSliceArgument).uiSliceNum as u16 > iMaxSliceCount {
                    iMaxSliceCount = (*pSliceArgument).uiSliceNum as u16;
                }
            }
            SM_SINGLE_SLICE | crate::api::codec_api::SliceModeEnum::SM_RASTER_SLICE => {
                if (*pSliceArgument).uiSliceNum as u16 > iMaxSliceCount {
                    iMaxSliceCount = (*pSliceArgument).uiSliceNum as u16;
                }
            }
            _ => {}
        }

        iSpatialIdx += 1;
        if iSpatialIdx >= iSpatialNum {
            break;
        }
    }

    (*pCodingParam).iMultipleThreadIdc = std::cmp::min(kiCpuCores as u16, iMaxSliceCount);
    // Loop filter requested to be enabled, with threading enabled: disable it on slice
    // boundaries, since that is not allowed with multithreading.
    if (*pCodingParam).iLoopFilterDisableIdc == 0 && (*pCodingParam).iMultipleThreadIdc != 1 {
        (*pCodingParam).iLoopFilterDisableIdc = 2;
    }
    *pMaxSliceCount = iMaxSliceCount as i16;

    ENC_RETURN_SUCCESS
}

/// `GetMultipleThreadIdc` — encoder_ext.cpp:2199.
///
/// The `X86_ASM` cache-line detection is not compiled on this target, so
/// `iCacheLineSize` is 16 as in the `#else` branch.
///
/// # Safety
/// All three out-pointers must be writable and `pCodingParam` initialised.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetMultipleThreadIdc(
    pLogCtx: *mut SLogContext,
    pCodingParam: *mut SWelsSvcCodingParam,
    iSliceNum: *mut i16,
    iCacheLineSize: *mut i32,
    uiCpuFeatureFlags: *mut u32,
) -> i32 {
    // number of logical processors on the physical processor package; zero means HTT
    // is not supported
    let mut uiCpuCores: i32 = 0;
    *uiCpuFeatureFlags = crate::decoder::decoder_core::WelsCPUFeatureDetect(&mut uiCpuCores);

    *iCacheLineSize = 16; // 16 bytes aligned in default

    if 0 == (*pCodingParam).iMultipleThreadIdc && uiCpuCores == 0 {
        // cpuid not supported, or doesn't expose the number of cores: use the
        // high-level system API to detect physical/logical processors
        uiCpuCores = crate::encoder::slice_multi_threading::DynamicDetectCpuCores();
    }

    if 0 == (*pCodingParam).iMultipleThreadIdc {
        (*pCodingParam).iMultipleThreadIdc = if uiCpuCores > 0 { uiCpuCores as u16 } else { 1 };
    }

    // So many cpu cores up to MAX_THREADS_NUM means server platforms; for client
    // applications it is constrained to MAX_THREADS_NUM here.
    (*pCodingParam).iMultipleThreadIdc = crate::encoder::rc::WELS_CLIP3(
        (*pCodingParam).iMultipleThreadIdc,
        1,
        MAX_THREADS_NUM as u16,
    );
    uiCpuCores = (*pCodingParam).iMultipleThreadIdc as i32;

    if InitSliceSettings(pLogCtx, pCodingParam, uiCpuCores, iSliceNum) != 0 {
        return 1;
    }
    0
}

/// `WelsInitEncoderExt` — encoder_ext.cpp:2290.
///
/// Replaces the `WelsInitEncoderExtRust` sketch, which allocated a fixed 4 MB
/// bitstream buffer, a 64-entry NAL list and nothing else — no `CMemoryAlign`, no
/// `RequestMemorySvc`, no DQ layers, no parameter-set arrays.
///
/// `MEMORY_MONITOR` and the `WelsLog` calls have no counterpart here.
///
/// # Safety
/// `ppCtx` and `pCodingParam` must be non-null; the context returned in `*ppCtx` is
/// owned by the caller and must be released with [`WelsUninitEncoderExt`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitEncoderExt(
    ppCtx: &mut Option<Box<sWelsEncCtx>>,
    pCodingParam: *mut SWelsSvcCodingParam,
    pLogCtx: *mut SLogContext,
    pExistingParasetList: *mut SExistingParasetList,
) -> i32 {
    let mut iSliceNum: i16 = 1; // number of slices used
    let mut iCacheLineSize: i32 = 16; // on-chip cache line size in bytes
    let mut uiCpuFeatureFlags: u32 = 0;
    if pCodingParam.is_null() {
        return 1;
    }

    let mut iRet = crate::encoder::wels_encoder_ext::ParamValidationExt(pLogCtx, pCodingParam);
    if iRet != 0 {
        return iRet;
    }
    iRet = (*pCodingParam).DetermineTemporalSettings();
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
    iRet = GetMultipleThreadIdc(
        pLogCtx,
        pCodingParam,
        &mut iSliceNum,
        &mut iCacheLineSize,
        &mut uiCpuFeatureFlags,
    );
    if iRet != 0 {
        return iRet;
    }

    // **T8.B5 — the out-parameter is the owner's slot.** `encoder_ext.cpp:1615`
    // nulls `*ppCtx` before it allocates, so a failed init leaves the caller
    // holding nothing; here the caller holds an `Option<Box<sWelsEncCtx>>` and
    // this is the same statement. The context below is raw for the whole of the
    // construction and becomes a `Box` again at the one place it is handed back —
    // S42's allocation root, and the only place in `src/encoder` where a
    // `&mut sWelsEncCtx` is born.
    *ppCtx = None;

    // C++ mallocs and memsets sWelsEncCtx; Box::new of a Default context is the
    // equivalent, and Default is the all-zero/null state for every member.
    let pCtx = Box::into_raw(Box::new(sWelsEncCtx::default()));

    if !pLogCtx.is_null() {
        (*pCtx).sLogCtx = *pLogCtx;
    }

    // **T7.C6**: `pCtx->pMemAlign = new CMemoryAlign(iCacheLineSize)` stood here
    // (`encoder_ext.cpp:1631`), the encoder's first allocation. Nothing in
    // `src/encoder` allocates through it any more, so the object, the field and the
    // teardown entry below are gone together. `iCacheLineSize` is still validated and
    // still reaches `InitFunctionPointers`; only the allocator it used to size is
    // gone.

    // **T6.H11**: `AllocCodingParam` and its failure branch were here. The context
    // owns the parameters, so the allocation is a `Box` and failure is panic-on-OOM
    // — the trade `pOut` made at T3.6 and the paraset arrays at T6.H2.
    (*pCtx).pSvcParam = Some(crate::encoder::param_svc::NewCodingParam());
    *ctx_param(pCtx) = *pCodingParam;

    // **T6.I1**: a `WelsMallocz` of `sizeof(SWelsFuncPtrList)` and its null branch
    // stood here. The context is born with the table (`Box`, every slot `None` —
    // the same image the memset produced), so there is nothing to allocate and no
    // allocation to fail; `InitFunctionPointers` writes over it exactly as before.
    // The trade is `pSvcParam`'s at T6.H11 and `pOut`'s at T3.6: panic-on-OOM.
    // T9.G6: hoisted — the call takes the context retag and this argument reads
    // through the same context (shape B).
    let pParamForFuncs = ctx_param(pCtx);
    iRet = crate::encoder::encoder_context::InitFunctionPointers(
        pCtx,
        pParamForFuncs,
        uiCpuFeatureFlags,
    );
    if iRet != ENC_RETURN_SUCCESS {
        WelsUninitEncoderExt(Some(Box::from_raw(pCtx)));
        return iRet;
    }

    (*pCtx).iActiveThreadsNum = (*pCodingParam).iMultipleThreadIdc as i16;
    (*pCtx).iMaxSliceCount = iSliceNum as i32;
    let mut pCtxTmp = pCtx;
    iRet = RequestMemorySvc(&mut pCtxTmp, pExistingParasetList);
    if iRet != 0 {
        WelsUninitEncoderExt(Some(Box::from_raw(pCtx)));
        return iRet;
    }

    if (*pCodingParam).iEntropyCodingModeFlag != 0 {
        crate::encoder::set_mb_syn_cabac::WelsCabacInit(pCtx);
    }
    // T9.G6: hoisted — the call takes the context retag and this argument reads
    // through the same context (shape B).
    let iRCMode = (*ctx_param(pCtx)).iRCMode;
    crate::encoder::rc::WelsRcInitModule(pCtx, iRCMode);

    (*pCtx).pVpp = crate::encoder::wels_preprocess::CWelsPreProcess::CreatePreProcess(pCtx);
    if (*pCtx).pVpp.is_null() {
        WelsUninitEncoderExt(Some(Box::from_raw(pCtx)));
        return 1;
    }
    // T9.G6: hoisted — the call takes the context retag and this argument reads
    // through the same context (shape B).
    let pParamForAlloc = ctx_param(pCtx);
    iRet = (*(*pCtx).pVpp).AllocSpatialPictures(pCtx, pParamForAlloc);
    if iRet != 0 {
        WelsUninitEncoderExt(Some(Box::from_raw(pCtx)));
        return iRet;
    }

    (*pCtx).iStatisticsLogInterval = STATISTICS_LOG_INTERVAL_MS;
    (*pCtx).uiLastTimestamp = -1;
    (*pCtx).bDeliveryFlag = true;
    *ppCtx = Some(Box::from_raw(pCtx));

    0
}

/// `STATISTICS_LOG_INTERVAL_MS` — `wels_const.h`.
pub const STATISTICS_LOG_INTERVAL_MS: i32 = 5000;

/// `FreeSliceInLayer` — encoder_ext.cpp:942.
///
/// # Safety
/// `pDq` must be non-null.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FreeSliceInLayer(pDq: &mut SDqLayer) {
    for iIdx in 0..MAX_THREADS_NUM {
        crate::encoder::svc_encode_slice::FreeSliceBuffer(pDq, iIdx);
    }
}

/// `FreeDqLayer` — encoder_ext.cpp:951.
///
/// # Safety
/// `pDq` must have come from `InitDqLayers` and must not be used afterwards.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FreeDqLayer(p: &mut SDqLayer) {

    // **T7.C4 finished this function.** `FreeSliceInLayer` used to release one
    // `CMemoryAlign` block per slice (`sSliceBs.pBs`) — the last allocation the layer
    // held by raw pointer. The slice owns its bitstream now, so this call empties the
    // banks and their buffers go with them; everything else the C++ frees here has
    // been owned since Phase 6.
    FreeSliceInLayer(&mut *p);

    // `ppSliceInLayer` is a `Vec<SliceIdx>` since T6.D4, `pFirstMbIdxOfSlice` and
    // `pCountMbNumInSlice` are `Vec<i32>` since T6.D6, and `pOverallMbMap` is a
    // `Vec<u16>` since T6.D7 — this call releases that last one early and restamps
    // the segment fields, all of which the layer's `Drop` would cover anyway.
    crate::encoder::svc_enc_slice_segment::UninitSlicePEncCtx(&mut *p);
    (*p).iMaxSliceNum = 0;

    // **T6.H8**: `drop(Box::from_raw(p))` stood here, with the slot's null-out under
    // it. The list owns the `Box`, so the layer's storage goes with the context —
    // which is also why this takes the layer rather than the slot.
}

/// `FreeRefList` — encoder_ext.cpp:986.
///
/// # Safety
/// `pRefList` must have come from `InitDqLayers` and must not be used afterwards.
// **T6.H7**: `FreeRefList` stood here. T6.F1 had already reduced it to
// `drop(Box::from_raw(*pRefList))`; the slot owns that `Box` now, so the last line
// of it is drop glue and the function is deleted rather than converted.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::codec_api::EProfileIdc;
    use crate::encoder::encoder_context::InitFunctionPointers;
    use crate::encoder::param_svc::NewCodingParam;
    use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

    /// Builds the context up to and including `RequestMemorySvc`, which is everything
    /// `WelsInitEncoderExt` does before the preprocessor. This is the direct test of
    /// baseline blocker C: before this phase `pSpsArray`/`pPPSArray` were never
    /// allocated, `iSpsNum`/`iPpsNum` never assigned and `ppDqLayerList` never filled.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn build_gate_context() -> *mut sWelsEncCtx {
        // Drive the same path the public API does: build an SEncParamExt and let
        // ParamTranscode fill sDependencyLayers, which ParamValidationExt then checks.
        // Setting SWelsSvcCodingParam's fields directly leaves the internal
        // dependency-layer frame rates at their FillDefault values and is rejected.
        let mut ext = crate::api::codec_api::SEncParamExt::default();
        ext.iUsageType = CAMERA_VIDEO_REAL_TIME;
        ext.iPicWidth = 160;
        ext.iPicHeight = 96;
        ext.fMaxFrameRate = 6.0;
        ext.iTargetBitrate = 500_000;
        ext.iRCMode = RC_OFF_MODE;
        ext.iTemporalLayerNum = 1;
        ext.iSpatialLayerNum = 1;
        ext.uiIntraPeriod = 0;
        ext.iMultipleThreadIdc = 1;
        ext.iEntropyCodingModeFlag = 0;
        ext.iLoopFilterDisableIdc = 0;
        ext.bEnableDenoise = false;
        ext.bEnableLongTermReference = false;
        ext.eSpsPpsIdStrategy = crate::api::codec_api::EParameterSetStrategy::CONSTANT_ID;
        ext.sSpatialLayers[0].iVideoWidth = 160;
        ext.sSpatialLayers[0].iVideoHeight = 96;
        ext.sSpatialLayers[0].fFrameRate = 6.0;
        ext.sSpatialLayers[0].iSpatialBitrate = 500_000;
        ext.sSpatialLayers[0].uiProfileIdc = EProfileIdc::PRO_BASELINE;
        ext.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_SINGLE_SLICE;
        ext.sSpatialLayers[0].sSliceArgument.uiSliceNum = 1;

        let mut param = SWelsSvcCodingParam::default();
        param.FillDefault();
        assert_eq!(param.ParamTranscode(&ext), ENC_RETURN_SUCCESS);

        let mut iSliceNum: i16 = 1;
        let mut iCacheLineSize: i32 = 16;
        let mut uiCpuFeatureFlags: u32 = 0;
        assert_eq!(
            crate::encoder::wels_encoder_ext::ParamValidationExt(null_mut(), &mut param),
            ENC_RETURN_SUCCESS
        );
        assert_eq!(param.DetermineTemporalSettings(), ENC_RETURN_SUCCESS);
        assert_eq!(
            GetMultipleThreadIdc(
                null_mut(),
                &mut param,
                &mut iSliceNum,
                &mut iCacheLineSize,
                &mut uiCpuFeatureFlags
            ),
            0
        );

        let pCtx = Box::into_raw(Box::new(sWelsEncCtx::default()));
        (*pCtx).pSvcParam = Some(NewCodingParam());
        *ctx_param(pCtx) = param;
        // T6.I1: the table comes with the context; see `WelsInitEncoderExt`.
        assert_eq!(
            { let pParam = ctx_param(pCtx); InitFunctionPointers(pCtx, pParam, uiCpuFeatureFlags) },
            ENC_RETURN_SUCCESS
        );
        (*pCtx).iActiveThreadsNum = param.iMultipleThreadIdc as i16;
        (*pCtx).iMaxSliceCount = iSliceNum as i32;

        let mut p = pCtx;
        assert_eq!(RequestMemorySvc(&mut p, null_mut()), 0, "RequestMemorySvc");
        pCtx
    }

    /// Blocker C: the parameter-set arrays are allocated and populated.
    #[test]
    // unsafe-cat: cursor
    #[allow(unsafe_code)]
    fn request_memory_svc_builds_the_parameter_sets() {
        unsafe {
            let pCtx = build_gate_context();

            assert!(!(*pCtx).pSpsArray.is_empty(), "pSpsArray still unallocated");
            assert!(!(*pCtx).pPPSArray.is_empty(), "pPPSArray still unallocated");
            // The configuration needs no subset SPS, and the C++ allocated nothing
            // at all for it — an empty `Vec`, which `ctx_subset_array` reads as the
            // null the raw field held.
            assert!((*pCtx).pSubsetArray.is_empty(), "pSubsetArray was not needed");
            assert!(ctx_subset_array(pCtx).is_null());
            assert_eq!((*pCtx).iSpsNum, 1);
            assert_eq!((*pCtx).iPpsNum, 1);
            assert_eq!((*pCtx).iSubsetSpsNum, 0);
            assert_eq!(ctx_sps(pCtx), ctx_sps_array(pCtx));
            assert_eq!(ctx_pps(pCtx), ctx_pps_array(pCtx));

            // The SPS the strategy generated must be the one Phase 3 proved
            // byte-exact against the C++ reference for this configuration.
            let sps = &*ctx_sps_array(pCtx);
            assert_eq!(sps.iMbWidth, 10);
            assert_eq!(sps.iMbHeight, 6);
            assert_eq!(sps.uiLog2MaxFrameNum, 15);
            assert_eq!(sps.uiPocType, 2);
            assert_eq!(sps.iLevelIdc, 13);

            let pps = &*ctx_pps_array(pCtx);
            assert_eq!(pps.iPicInitQp, 26);
            assert!(pps.bDeblockingFilterControlPresentFlag);

            WelsUninitEncoderExt(Some(Box::from_raw(pCtx)));
        }
    }

    /// Blocker C, second half: the DQ layers, reference lists and macroblock list
    /// exist, which is what `pCurDqLayer` is selected from.
    #[test]
    // unsafe-cat: cursor
    #[allow(unsafe_code)]
    fn request_memory_svc_builds_the_dq_layers() {
        unsafe {
            let pCtx = build_gate_context();

            let pDq = ctx_dq_layer(pCtx, 0);
            assert!(!pDq.is_null());
            assert_eq!((*pDq).iMbWidth, 10);
            assert_eq!((*pDq).iMbHeight, 6);
            assert_eq!((*pDq).sSliceEncCtx.iMbNumInFrame, 60);
            assert_eq!((*pDq).sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed), 1);
            assert_eq!((*pDq).sSliceEncCtx.pOverallMbMap.len(), 60);
            assert_eq!((*pDq).sMbDataP.dims().count(), 60);

            // InitMbInfo wired every macroblock to its slot in the context arrays.
            let pMb = (*pDq).sMbDataP.get(0);
            assert_eq!(pMb.iMbXY, 0);
            assert_eq!(pMb.iMbX, 0);
            assert_eq!(pMb.iMbY, 0);
            // MB 0 has no left/top neighbour.
            assert_eq!(pMb.uiNeighborAvail, 0);
            let pMb11 = (*pDq).sMbDataP.get(11); // row 1, column 1: all four neighbours present
            assert_eq!(pMb11.iMbX, 1);
            assert_eq!(pMb11.iMbY, 1);
            assert_eq!(
                (*pMb11).uiNeighborAvail,
                LEFT_MB_POS | TOP_MB_POS | TOPLEFT_MB_POS | TOPRIGHT_MB_POS
            );

            assert!(!ctx_ref_list(pCtx, 0).is_null());
            assert!(!(*ctx_ref_list(pCtx, 0)).pRef.is_empty());
            assert_eq!(
                (*pCtx).pDecPic,
                Some((*ctx_ref_list(pCtx, 0)).pRef.at(0))
            );

            assert!((*pCtx).pStrideTab.is_some());
            assert!(!ctx_mvd_cost_table(pCtx).is_null());
            assert_eq!(
                (*pCtx).eRefStrategy,
                crate::encoder::ref_list_mgr_svc::RefStrategyKind::TemporalLayer,
                "the gate configuration is camera content without LTR"
            );

            WelsUninitEncoderExt(Some(Box::from_raw(pCtx)));
        }
    }
}

/// `WelsUninitEncoderExt` — encoder_ext.cpp:2246, with `FreeMemorySvc`
/// (encoder_ext.cpp:1804) folded in.
///
/// # Safety
/// `ppCtx` must point to a context from [`WelsInitEncoderExt`], or be null/point to
/// null.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsUninitEncoderExt(pEncContext: Option<Box<sWelsEncCtx>>) {
    // **T8.B5 — the teardown takes the context by value**, which is what
    // `encoder_ext.cpp:1878`'s `*ppCtx = NULL` at the end was expressing: after
    // this call the caller has no context, and now that is a fact about the type
    // rather than a store the function has to remember. The body below is
    // unchanged and still raw — the free cascade walks the whole context — so the
    // `Box` is opened here and closed at the end, in one function.
    let Some(pEncContext) = pEncContext else {
        return;
    };
    let pCtx = Box::into_raw(pEncContext);

    if !(*pCtx).pVpp.is_null() {
        (*(*pCtx).pVpp).FreeSpatialPictures(pCtx);
        drop(Box::from_raw((*pCtx).pVpp));
        (*pCtx).pVpp = null_mut();
    }

    {
        // **T6.H1**: two `WelsFree`s stood here — the one block, reached as
        // `pStrideDecBlockOffset[0][1]` because that was the only field still
        // holding its head, and then the table struct. The table owns the block and
        // the context owns the table, so both are the `drop(Box::from_raw(pCtx))` at
        // the end of this function, and the entry is deleted rather than converted.
        // **T6.H3**: `pDqIdcMap`'s `WelsFree` stood here; the `Vec` is the context's.
        // R4 in miniature, and the first encoder cascade entries to fall: four
        // `WelsFree` calls — `pOut->pBsBuffer`, `pOut->sNalList`,
        // `pOut->pNalLen` and the struct itself — are one `drop`. The three
        // buffers are `Vec`s that free themselves, and the struct came from
        // `Box::into_raw`, so it goes back through `Box::from_raw`.
        if !(*pCtx).pOut.is_null() {
            drop(Box::from_raw((*pCtx).pOut));
            (*pCtx).pOut = null_mut();
        }
        // T4b.2b: `DestroyReferenceStrategy` freed a box holding one back-pointer.
        // With the strategy an enum there is no allocation, so this free-cascade entry
        // is deleted rather than converted.
        // **T6.H4**: `pFrameBs`'s `WelsFree` stood here; the `Vec` is the context's.
        // **T7.C5**: the `DynamicSliceBs` free walk stood here — the last `WelsFree` in
        // `src/encoder`. The buffers are the context's own and go with its drop.

        // **T6.H2**: three `WelsFree`s stood here, one per parameter-set array. All
        // three are `Vec`s the context owns, so all three are its own drop.
        // The five per-macroblock arrays freed here in encoder_ext.cpp:1932-1961 are
        // inline in `SMB` since T6.C1 and go with the `SMB` list below.
        // `ppMbListD` is gone: each layer owns its own `MbArray<SMB>` (T6.D5).
        // **T6.H9**: `pMvdCostTable`'s `WelsFree` stood here; the `Vec` is the
        // context's.
        // **T6.H6**: this entry was the cascade's one *ordered* pair —
        // `WelsRcFreeMemory(pCtx)` had to run before the `WelsFree` below it, because
        // the per-layer blocks hung off the array being freed. Ownership is what that
        // ordering was expressing, and now the types say it: each `SWelsSvcRc` owns
        // its five arrays, the context owns the layers, and the order is the drop
        // glue's. Both calls are deleted, and so is `WelsRcFreeMemory`.
        // **T6.H5**: `pLtr`'s `WelsFree` stood here; the `Vec` is the context's.
        // DQ layers list. **T6.H8**: the block's `WelsFree` and each layer's
        // `Box::from_raw` are the context's drop now, and what is left of this entry
        // is the residue `FreeDqLayer` still has to release by hand — `sSliceBs.pBs`,
        // one `CMemoryAlign` block per slice, which is **Phase 7's** (the boundary
        // list names it). It has to run *before* the layers drop, which is why the
        // loop stays rather than disappearing with the rest. Its bound is the list's
        // own length now, not `iSpatialLayerNum` read back out of the parameters at
        // teardown — the silent-leak shape T6.H7 found next door.
        for ilayer in 0..(*pCtx).ppDqLayerList.len() {
            let pLayer = ctx_dq_layer(pCtx, ilayer);
            if !pLayer.is_null() {
                FreeDqLayer(&mut *pLayer);
            }
        }
        // **T6.H7**: the reference-list entry stood here — a loop calling `FreeRefList`
        // over `iSpatialLayerNum` slots, then one `WelsFree` for the block. The `Vec`
        // drops every slot it holds, which is strictly more than the loop did: the
        // loop read `iSpatialLayerNum` back out of the *parameters* at teardown, so a
        // configuration change between init and teardown would have leaked the slots
        // past the new count. `FreeRefList` is deleted with it.
        // **T6.F3** put seven `WelsFree`s and their null tests here into one
        // `drop(Box::from_raw(..))`; **T6.H10** deletes that too — the context holds
        // the `Box`, so releasing the VAA block is the context's own drop.
        // **T6.H11**: `FreeCodingParam` stood here; the `Box` is the context's.
        // **T6.I1**: the `WelsFree` of the table, its null guard, and the explicit
        // `pParametersetStrategy.take()` stood here. F19 was that missing `take()`:
        // `encoder_ext.cpp:1995` deletes the strategy object, the port did not, and
        // because the table was raw-allocated `SWelsFuncPtrList`'s own drop glue
        // never ran — so the strategy leaked on every teardown. With the table a
        // `Box` the glue *does* run, the strategy's `Option<Box<_>>` drops with it,
        // and F19 stops being a thing a hand-written line has to remember. The
        // whole block is deleted rather than converted.
        // **T7.C6**: `WELS_DELETE_OP (pCtx->pMemAlign)` stood here — the cascade's
        // last entry, and the reason this whole block was wrapped in a null test on
        // the allocator. Every free above it had already become a drop; this is the
        // last one, and the block below it is now just the context's own.
    }

    drop(Box::from_raw(pCtx));
}

// ============================================================================
// The encoding half of encoder_ext.cpp: WelsEncoderEncodeExt and its helpers.
//
// Translated statement for statement from `codec/encoder/core/src/encoder_ext.cpp`.
// Line references in the doc comments are to that file.
// ============================================================================

/// `encoder_ext.cpp:2393`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetTemporalLevel(
    fDlp: *mut SSpatialLayerInternal,
    kiFrameNum: i32,
    kiGopSize: i32,
) -> i32 {
    let kiCodingIdx = kiFrameNum & (kiGopSize - 1);
    (*fDlp).uiCodingIdx2TemporalId[kiCodingIdx as usize] as i32
}

/// `encoder_ext.cpp:3114`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetSubSequenceId(pCtx: *mut sWelsEncCtx, eFrameType: EVideoFrameType) -> i32 {
    if eFrameType == EVideoFrameType::videoFrameTypeIDR {
        0
    } else if eFrameType == EVideoFrameType::videoFrameTypeI {
        1
    } else if eFrameType == EVideoFrameType::videoFrameTypeP {
        if (*pCtx).bCurFrameMarkedAsSceneLtr {
            2
        } else {
            // T0:3 T1:4 T2:5 T3:6
            3 + (*pCtx).uiTemporalId as i32
        }
    } else {
        3 + MAX_TEMPORAL_LAYER_NUM as i32
    }
}

/// `encoder_ext.cpp:2797`. Swap the current DQ layer with the next one and make the
/// outgoing layer the reference.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsSwapDqLayers(pCtx: *mut sWelsEncCtx, kiNextDqIdx: i32) {
    // The outgoing layer's *position*, not its address — T6.D3, and since T6.G2 the
    // context holds nothing else: `iCurDqLayer` **is** the index, so the round trip
    // through `pCurDqLayer->iDqIdx` that this site used to need is gone. The
    // `expect` cannot fire on a live path — the frame loop makes a layer current
    // before any swap — and the old spelling dereferenced a null pointer there.
    let kRefIdx = (*pCtx).iCurDqLayer.expect("WelsSwapDqLayers with no current layer");
    set_current_layer(pCtx, Some(LayerIdx(kiNextDqIdx as u8)));
    (*current_layer(pCtx)).pRefLayer = Some(kRefIdx);
}

// `StampLayerPictureViews` stood here — the once-per-frame stamp of
// `sRefPicView`/`sDecPicView` (T6.F5). Phase 9 E3's harvest deleted both fields:
// the reference readers resolve the picture per call (`layer_ref_pic` +
// `SPicture::data_ptr_shared`/`stride`/`iPictureType`), and the reconstruction
// view had zero readers. One `cursor` tag retires with it.


/// `encoder_ext.cpp:2808`. Prefetch the reference picture after `WelsBuildRefList`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn PrefetchReferencePicture(pCtx: *mut sWelsEncCtx, keFrameType: EVideoFrameType) {
    let kiSliceCount = (*current_layer(pCtx)).iMaxSliceNum;
    // C++ declares `uint8_t uiRefIdx = -1;`, which wraps to 255.
    let mut uiRefIdx: u8 = 0xff;

    debug_assert!(kiSliceCount > 0);
    if keFrameType != EVideoFrameType::videoFrameTypeIDR {
        debug_assert!((*pCtx).iNumRef0 > 0);
        // always get item 0 due to reordering done
        (*pCtx).pRefPic = (*pCtx).pRefList0[0];
        (*current_layer(pCtx)).pRefPic = (*pCtx).pRefPic;
        uiRefIdx = 0; // reordered reference index
    } else {
        // safe for IDR coding
        (*pCtx).pRefPic = None;
        (*current_layer(pCtx)).pRefPic = None;
    }

    let mut iIdx = 0;
    while iIdx < kiSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(current_layer(pCtx), iIdx);
        if !pSlice.is_null() {
            (*pSlice).sSliceHeaderExt.sSliceHeader.uiRefIndex = uiRefIdx;
        }
        iIdx += 1;
    }
}

/// `encoder_ext.cpp:3376`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ClearFrameBsInfo(pCtx: *mut sWelsEncCtx, pFbi: *mut SFrameBSInfo) {
    (*pFbi).sLayerInfo[0].pBsBuf = ctx_frame_bs(pCtx);
    (*pFbi).sLayerInfo[0].pNalLengthInByte = (*(*pCtx).pOut).sNalLen.as_mut_ptr();

    for i in 0..(*pFbi).iLayerNum as usize {
        (*pFbi).sLayerInfo[i].iNalCount = 0;
        (*pFbi).sLayerInfo[i].eFrameType = EVideoFrameType::videoFrameTypeSkip;
    }
    (*pFbi).iLayerNum = 0;
    (*pFbi).iFrameSizeInBytes = 0;
}

/// `encoder_ext.cpp:3341`. Roll the encoder state back one frame after the rate
/// controller decides to drop it.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn StackBackEncoderStatus(pEncCtx: *mut sWelsEncCtx, keFrameType: EVideoFrameType) {
    let pParamInternal = (*ctx_param(pEncCtx))
        .sDependencyLayers
        .as_mut_ptr()
        .add((*pEncCtx).uiDependencyId as usize);

    // for bitstream writing
    (*pEncCtx).iPosBsBuffer = 0; // reset bs buffer position
    (*(*pEncCtx).pOut).iNalIndex = 0; // reset NAL index
    (*(*pEncCtx).pOut).iLayerBsIndex = 0; // reset index of Layer Bs

    // Was `InitBits(&pOut->sBsWrite, pOut->pBsBuffer, pOut->uiSize)`. The buffer
    // stays on `pOut` where it already was — owned outright since T3.6, so its
    // length is `sBsBuffer.len()` and not a field; the writer is a position,
    // and resetting it is the whole of what `InitBits` did that still means
    // anything (F13's third site: the `*const`-declared, `*mut`-stored, written-
    // through buffer parameter is gone, not amended).
    (*(*pEncCtx).pOut).sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();

    if keFrameType == EVideoFrameType::videoFrameTypeP
        || keFrameType == EVideoFrameType::videoFrameTypeI
    {
        (*pParamInternal).iFrameIndex -= 1;
        if (*pParamInternal).iPOC != 0 {
            (*pParamInternal).iPOC -= 2;
        } else {
            (*pParamInternal).iPOC = (1 << (*ctx_sps(pEncCtx)).iLog2MaxPocLsb) - 2;
        }

        let iDid = (*pEncCtx).uiDependencyId as i32;
        crate::encoder::encoder_context::LoadBackFrameNum(pEncCtx, iDid);

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        (*pEncCtx).eSliceType = EWelsSliceType::P_SLICE;
        // eNalPriority is not stacked back: it is updated at the start of coding a frame.
    } else if keFrameType == EVideoFrameType::videoFrameTypeIDR {
        (*pParamInternal).uiIdrPicId -= 1;
        // set the next frame to be IDR
        let iDid = (*pEncCtx).uiDependencyId as i32;
        crate::encoder::wels_encoder_ext::ForceCodingIDR(pEncCtx, iDid);
    } else {
        // B pictures are not supported now
        debug_assert!(false, "StackBackEncoderStatus: unsupported frame type");
    }

    // No need to stack back RC info -- it is still useful for later RQ model
    // calculation -- nor MB slicing info for dynamic balancing.
}

/// `encoder_ext.cpp:2534`. Bind the current DQ layer to this frame's parameter sets,
/// NAL header and picture buffers.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitCurrentLayer(pCtx: *mut sWelsEncCtx, _kiWidth: i32, _kiHeight: i32) {
    let pParam = ctx_param(pCtx);
    let pCurDq = current_layer(pCtx);
    if pCurDq.is_null() {
        return;
    }
    // **T6.F1**: the layer is stamped with the list its handles belong to, here, once
    // a frame — `pRefPic`/`pDecPic` on `SDqLayer` are slots of *this* list, and the
    // per-macroblock mode-decision family resolves them through it.
    let pRefList = ctx_ref_list(pCtx, (*pCtx).uiDependencyId as usize);
    (*pCurDq).pRefList = pRefList;
    let pBaseSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, 0);
    if pBaseSlice.is_null() {
        return;
    }
    let kiCurDid = (*pCtx).uiDependencyId;
    let kbUseSubsetSpsFlag = !(*pParam).bSimulcastAVC && (kiCurDid as i32) > BASE_DEPENDENCY_ID;
    let pNalHdExt = &mut (*pCurDq).sLayerInfo.sNalHeaderExt;
    let pDqIdc = ctx_dq_idc_map(pCtx).add(kiCurDid as usize);
    let iSliceCount = (*pCurDq).iMaxSliceNum;
    // S29 / F13's family: `addr_of_mut!` on the element, not `as_mut_ptr().add()` —
    // the latter reborrows the whole array and a second such derivation pops the first.
    let pParamInternal = std::ptr::addr_of_mut!((*pParam).sDependencyLayers[kiCurDid as usize]);

    (*pCurDq).pDecPic = (*pCtx).pDecPic;

    debug_assert!(iSliceCount > 0);

    let mut iCurPpsId = (*pDqIdc).iPpsId as i32;
    let iCurSpsId = (*pDqIdc).iSpsId as i32;

    iCurPpsId = ParasetStrategy(pCtx).GetCurrentPpsId(
        iCurPpsId,
        ((*pParamInternal).uiIdrPicId as i32 - 1).abs() % MAX_PPS_COUNT as i32,
    );

    // T6.G3. The C++ writes the id and then an address derived from it, three times
    // over (`encoder_ext.cpp:2560-2576`); the layer keeps the id and the slice
    // header's own `iPpsId`/`iSpsId` — already here, already the same numbers — are
    // what the header carries. The two pointer copies the header used to take are
    // gone with the fields.
    (*pBaseSlice).sSliceHeaderExt.sSliceHeader.iPpsId = iCurPpsId;
    (*pCurDq).sLayerInfo.iPps = Some(PpsId(iCurPpsId as u16));

    (*pBaseSlice).sSliceHeaderExt.sSliceHeader.iSpsId = iCurSpsId;
    // The null-versus-not that used to select the arm is the tag now — same two
    // arms, same `iCurSpsId`, indexing the same two different arrays.
    (*pCurDq).sLayerInfo.eSps = Some(if kbUseSubsetSpsFlag {
        LayerSps::Subset(SubsetSpsId(iCurSpsId as u8))
    } else {
        LayerSps::Avc(SpsId(iCurSpsId as u8))
    });

    (*pBaseSlice).bSliceHeaderExtFlag =
        (*pCtx).eNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;

    let mut iIdx = 1;
    while iIdx < iSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iIdx);
        if !pSlice.is_null() {
            crate::encoder::svc_encode_slice::InitSliceHeadWithBase(&mut *pSlice, &*pBaseSlice);
        }
        iIdx += 1;
    }

    std::ptr::write_bytes(pNalHdExt as *mut _ as *mut u8, 0, std::mem::size_of::<SNalUnitHeaderExt>());
    let pNalHd = &mut pNalHdExt.sNalUnitHeader;
    pNalHd.uiNalRefIdc = (*pCtx).eNalPriority as u8;
    pNalHd.eNalUnitType = (*pCtx).eNalType;

    pNalHdExt.uiDependencyId = kiCurDid;
    pNalHdExt.bDiscardableFlag = if (*pCtx).bNeedPrefixNalFlag {
        pNalHd.uiNalRefIdc == EWelsNalRefIdc::NRI_PRI_LOWEST as u8
    } else {
        false
    };
    pNalHdExt.bIdrFlag = ((*pParamInternal).iFrameNum == 0)
        && ((*pCtx).eNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
            || (*pCtx).eSliceType == EWelsSliceType::I_SLICE);
    pNalHdExt.uiTemporalId = (*pCtx).uiTemporalId;

    // pEncPic data. **S37, and the resolution sits here rather than at the top of the
    // function on purpose**: the C++ loads both `SPicture*`s at entry and dereferences
    // them only at this line, so resolving here leaves every statement above it
    // running on a path where a picture is not bound — which is what the C++ does,
    // minus the null dereference it would take three lines later.
    let (Some(idEnc), Some(idDec)) = ((*pCtx).pEncPic, (*pCtx).pDecPic) else {
        return;
    };
    if pRefList.is_null() || (*pCtx).pVpp.is_null() {
        return;
    }
    // The handle and its pool, beside the roots they stand for (T9.B21). `idEnc` is
    // already resolved above and the pool is the one it indexes, so this is the same
    // fact written twice — the point of a strangler step: both spellings live until
    // the last reader of the raw one is gone.
    //
    // **The pool pointer is taken first and every access below goes through it**,
    // which is `pRefList`'s shape one picture over (`:2074`, then `(*pRefList)
    // .pic_mut(..)` on the next line) and is the ordering that keeps it valid. Taken
    // the other way round — `get_mut` on the field, then `addr_of_mut!` of the same
    // field — the two are competing paths into one place rather than parent and
    // child, and whichever is written second pops the first. S29's boundary clause,
    // and F114a is what it costs to get wrong.
    let pSrcPool = std::ptr::addr_of_mut!((*(*pCtx).pVpp).m_pSpatialPicPool);
    (*pCurDq).pEncPic = Some(idEnc);
    (*pCurDq).pSrcPool = pSrcPool;

    let pEncPic = (*pSrcPool).get_mut(idEnc).planes();
    let pDecPic = (*pRefList).pic_mut(idDec).planes();

    // **The reconstruction seam is built here, and here is not an accident**
    // (T9.C2, D-mt-3 option A). This is the last point in the frame at which the
    // reconstruction picture is borrowed exclusively on the calling thread —
    // everything after it is the macroblock loop, which forks. The view captures
    // the same three plane roots `pCsData` is about to be stamped with, plus the
    // four per-macroblock side arrays no plane cursor can carry, and from here on
    // *nothing* in the frame may take `&mut` on this picture again.
    //
    // Rebuilt every frame, unconditionally, because the pool may have handed
    // `idDec` a different slot: a view is only ever valid for the frame that
    // built it.
    (*pCurDq).pRecView =
        Some(crate::encoder::rec_view::RecPicView::build((*pRefList).pic_mut(idDec)));

    (*pCurDq).pEncData[0] = pEncPic.pData[0];
    (*pCurDq).pEncData[1] = pEncPic.pData[1];
    (*pCurDq).pEncData[2] = pEncPic.pData[2];
    (*pCurDq).iEncStride[0] = pEncPic.iLineSize[0];
    (*pCurDq).iEncStride[1] = pEncPic.iLineSize[1];
    (*pCurDq).iEncStride[2] = pEncPic.iLineSize[2];
    // cs data
    (*pCurDq).pCsData[0] = pDecPic.pData[0];
    (*pCurDq).pCsData[1] = pDecPic.pData[1];
    (*pCurDq).pCsData[2] = pDecPic.pData[2];
    (*pCurDq).iCsStride[0] = pDecPic.iLineSize[0];
    (*pCurDq).iCsStride[1] = pDecPic.iLineSize[1];
    (*pCurDq).iCsStride[2] = pDecPic.iLineSize[2];

    (*pCurDq).bBaseLayerAvailableFlag = (*pCurDq).pRefLayer.is_some();

    // **T7.B4.** Was `pTaskManage->InitFrame(kiCurDid)`, whose whole body was "if the
    // layer wants re-slicing, dispatch the pre-encoding task list and wait". The task
    // list is gone; the condition and the barrier position are not. The count is the
    // one `CreateTasks` computed for `WELS_ENC_TASK_UPDATEMBMAP`
    // (`sSliceArgument.uiSliceNum` for every non-`SM_SIZELIMITED_SLICE` mode), and
    // only the fixed modes can reach here: `bNeedAdjustingSlicing` is written by
    // `DynamicAdjustSlicing` alone, which only `AdjustBaseLayer`/`AdjustEnhanceLayer`
    // call, and only on the `SM_FIXEDSLCNUM_SLICE` arm.
    if !(*pCtx).pSliceThreading.is_null()
        && !current_layer(pCtx).is_null()
        && (*current_layer(pCtx)).bNeedAdjustingSlicing
    {
        let kiTaskCount = (*ctx_param(pCtx)).sSpatialLayers[kiCurDid as usize]
            .sSliceArgument
            .uiSliceNum as i32;
        crate::encoder::slice_multi_threading::UpdateMbMapForked(pCtx, kiTaskCount);
    }
}

/// `encoder_ext.cpp:2954`. Emit the SVC prefix NAL that precedes each VCL NAL when
/// `bNeedPrefixNalFlag` is set.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn AddPrefixNal(
    pCtx: *mut sWelsEncCtx,
    _pLayerBsInfo: *mut SLayerBSInfo,
    pNalLen: *mut i32,
    pNalIdxInLayer: *mut i32,
    keNalType: EWelsNalUnitType,
    keNalRefIdc: EWelsNalRefIdc,
    iPayloadSize: *mut i32,
) -> i32 {
    let mut iReturn;
    *iPayloadSize = 0;

    let pOut = (*pCtx).pOut;

    if keNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST {
        crate::encoder::nal_encap::WelsLoadNal(
            pOut,
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            keNalRefIdc as i32,
        );

        crate::encoder::nal_encap::WelsWriteSVCPrefixNal(
            &mut (&mut *pOut).sBsBuffer[..],
            &mut (*pOut).sBsWrite,
            keNalRefIdc as i32,
            keNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR,
        );

        crate::encoder::nal_encap::WelsUnloadNal(pOut);
    } else {
        // No prefix NAL unit RBSP syntax here, but the NAL unit header extension is
        // still needed.
        crate::encoder::nal_encap::WelsLoadNal(
            pOut,
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            keNalRefIdc as i32,
        );
        crate::encoder::nal_encap::WelsUnloadNal(pOut);
    }

    iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &(&*pOut).sNalList[(*pOut).iNalIndex as usize - 1],
        &(&*pOut).sBsBuffer[..],
        Some(&(*current_layer(pCtx)).sLayerInfo.sNalHeaderExt),
        ctx_frame_bs_cur(pCtx),
        (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
        &mut *pNalLen.add(*pNalIdxInLayer as usize),
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }
    *iPayloadSize = *pNalLen.add(*pNalIdxInLayer as usize);

    (*pCtx).iPosBsBuffer += *iPayloadSize;
    *pNalIdxInLayer += 1;

    iReturn = ENC_RETURN_SUCCESS;
    iReturn
}

/// `encoder_ext.cpp:3003`. Emit a filler-data NAL of `iLen` bytes.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WritePadding(pCtx: *mut sWelsEncCtx, iLen: i32, iSize: *mut i32) -> i32 {
    let mut iNalLen = 0i32;

    *iSize = 0;
    let pOut = (*pCtx).pOut;
    let iNal = (*pOut).iNalIndex;
    // The frame-level writer, for non-VCL NALs.
    let buf = &mut (&mut *pOut).sBsBuffer[..];
    let pBs = &mut (*pOut).sBsWrite;

    // `pEndBuf - pCurBuf < iLen` in comparison form; `iLen` is non-negative here
    // and a `usize` `len - pos` cannot wrap because `pos <= len` always holds for a
    // writer that has not overrun, which the write below would panic on anyway.
    if (buf.len() - pBs.pos()) < iLen as usize || iNal >= (*pOut).sNalList.len() as i32 {
        return ENC_RETURN_MEMOVERFLOWFOUND;
    }

    crate::encoder::nal_encap::WelsLoadNal(
        pOut,
        EWelsNalUnitType::NAL_UNIT_FILLER_DATA as i32,
        EWelsNalRefIdc::NRI_PRI_LOWEST as i32,
    );

    for _ in 0..iLen {
        crate::encoder::vlc_encoder::BsWriteBits(buf, pBs, 8, 0xff);
    }

    crate::encoder::vlc_encoder::BsRbspTrailingBits(buf, pBs);

    crate::encoder::nal_encap::WelsUnloadNal(pOut);

    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &(&*pOut).sNalList[iNal as usize],
        &(&*pOut).sBsBuffer[..],
        None,
        ctx_frame_bs_cur(pCtx),
        (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
        &mut iNalLen,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    (*pCtx).iPosBsBuffer += iNalLen;
    *iSize += iNalLen;

    ENC_RETURN_SUCCESS
}

/// `encoder_ext.cpp:2624` (`static inline SetFastCodingFunc`).
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
unsafe fn SetFastCodingFunc(pFuncList: &mut SWelsFuncPtrList) {
    pFuncList.pfIntraFineMd =
        Some(crate::encoder::svc_base_layer_md::WelsMdIntraFinePartitionVaa);
    let sdf = &mut pFuncList.sSampleDealingFuncs;
    sdf.pfMdCost = CostFamily::Sad;
    // The C++ also aims three `pfIntra*Combined3` slots at their `*Sad` twins here;
    // both sides were NULL on every target and the fields are deleted (S18).
}

/// `encoder_ext.cpp:2630` (`static inline SetNormalCodingFunc`).
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
unsafe fn SetNormalCodingFunc(pFuncList: &mut SWelsFuncPtrList) {
    pFuncList.pfIntraFineMd = Some(crate::encoder::svc_base_layer_md::WelsMdIntraFinePartition);
    let sdf = &mut pFuncList.sSampleDealingFuncs;
    sdf.pfMdCost = CostFamily::Satd;
    // As `SetFastCodingFunc`: the three `Combined3` aims are deleted with the fields.
}

// `SetMeMethod` (`encoder_ext.cpp:2643`) stood here — the ME-method selector
// that aims a `pfSearchMethod` slot at diamond/cross/feature search. **Zero
// callers anywhere in src/ or tests/** (the C++ calls it from the
// SCREEN_CONTENT block `PreprocessSliceCoding` did not translate; the camera
// path installs `WelsDiamondSearch` for every block size directly). S18,
// session F — Phase 10 re-ports it from the reference when the screen-content
// dispatch arrives.

/// `encoder_ext.cpp:2665`. Per-frame function-pointer selection. MUST be called after
/// `pfWelsRcPictureInit()` and `WelsInitCurrentLayer()`.
///
/// The `SCREEN_CONTENT_REAL_TIME` block (`encoder_ext.cpp:2708-2771`) is the only part
/// not translated; see the comment at its position below.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn PreprocessSliceCoding(pCtx: *mut sWelsEncCtx) {
    let pCurLayer = current_layer(pCtx);
    let bFastMode = (*ctx_param(pCtx)).iComplexityMode == LOW_COMPLEXITY;
    // **T6.I2**, as `InitFunctionPointers`: one `&mut` derived from the owner, not
    // one per call. This is the function the whole step-1 checker is about — it is
    // where the table is re-written *per frame*, which is why no reader may hold
    // anything derived from it across a call that reaches it again.
    let fl: &mut SWelsFuncPtrList = &mut *ctx_func_list(pCtx);

    // function pointers conditional assignment under sWelsEncCtx
    if ((*ctx_param(pCtx)).iUsageType == CAMERA_VIDEO_REAL_TIME && bFastMode)
        || ((*ctx_param(pCtx)).iUsageType == SCREEN_CONTENT_REAL_TIME
            && (*pCtx).eSliceType == EWelsSliceType::P_SLICE
            && bFastMode)
    {
        SetFastCodingFunc(fl);
    } else {
        SetNormalCodingFunc(fl);
    }

    if (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
        for i in 0..EStaticBlockIdc::BLOCK_STATIC_IDC_ALL as usize {
            fl.pfMotionSearch[i] =
                Some(crate::encoder::svc_motion_estimate::WelsMotionEstimateSearch);
        }
        for b in [
            BLOCK_16x16, BLOCK_16x8, BLOCK_8x16, BLOCK_8x8, BLOCK_4x4, BLOCK_8x4, BLOCK_4x8,
        ] {
            fl.sMeFuncs.pfSearchMethod[b] =
                Some(crate::encoder::svc_motion_estimate::WelsDiamondSearch);
        }
        fl.pfFirstIntraMode =
            Some(crate::encoder::svc_base_layer_md::WelsMdFirstIntraMode);
        let sdf = &mut fl.sSampleDealingFuncs;
        sdf.pfMeCost = CostFamily::Satd;
        fl.pfSetScrollingMv =
            Some(crate::encoder::svc_mode_decision::SetScrollingMvToMdNull);

        if bFastMode {
            fl.sMeFuncs.pfCalculateSatd =
                Some(crate::encoder::svc_motion_estimate::NotCalculateSatdCost);
            fl.pfInterFineMd =
                Some(crate::encoder::svc_base_layer_md::WelsMdInterFinePartitionVaa);
        } else {
            fl.sMeFuncs.pfCalculateSatd =
                Some(crate::encoder::svc_motion_estimate::CalculateSatdCost);
            fl.pfInterFineMd =
                Some(crate::encoder::svc_base_layer_md::WelsMdInterFinePartition);
        }
    } else {
        fl.sSampleDealingFuncs.pfMeCost = CostFamily::Unset;
    }

    // The SCREEN_CONTENT_REAL_TIME block of the C++ (encoder_ext.cpp:2708-2771) sets up
    // feature-based motion search. It is outside the Phase-5 gate configuration
    // (CAMERA_VIDEO_REAL_TIME) and depends on the unported mode-decision layer for
    // pfInterFineMd, so it is not translated here.

    // update some layer-dependent variables to save judgements at MB level
    let sdf = &fl.sSampleDealingFuncs;
    // Was two `ptr::eq` comparisons against `pfSampleSatd.as_ptr()` — i.e. "does
    // this interior pointer still point at the SATD array". With the pointers
    // gone the question is asked directly, and asking it directly is also what
    // makes it correct: `as_ptr()` here derived a *third* pointer from the
    // struct purely to compare, which under Stacked Borrows invalidated the
    // very pointers it was testing.
    (*pCurLayer).bSatdInMdFlag =
        sdf.pfMeCost == CostFamily::Satd && sdf.pfMdCost == CostFamily::Satd;

    let kiCurDid = (*pCtx).uiDependencyId as usize;
    let kiCurTid = (*pCtx).uiTemporalId as i32;
    let pDep = &(*ctx_param(pCtx)).sDependencyLayers[kiCurDid];
    if (*pCurLayer).bDeblockingParallelFlag
        && (*pCurLayer).iLoopFilterDisableIdc != 1
        // ENABLE_FRAME_DUMP is not defined, so this clause is compiled in.
        && (*pCtx).eNalPriority != EWelsNalRefIdc::NRI_PRI_LOWEST
        && (pDep.iHighestTemporalId == 0 || kiCurTid < pDep.iHighestTemporalId as i32)
    {
        fl.pfDeblocking.pfDeblockingFilterSlice =
            Some(crate::encoder::deblocking::DeblockingFilterSliceAvcbase);
    } else {
        fl.pfDeblocking.pfDeblockingFilterSlice =
            Some(crate::encoder::deblocking::DeblockingFilterSliceAvcbaseNull);
    }

    // **F132 round 7 (T9.E6)**: `pfInterMd` used to be stamped by
    // `WelsCodePSlice`/`WelsCodePOverDynamicSlice` — per slice, from inside the
    // fork, into this shared function list, exactly as the C++ does
    // (`svc_encode_slice.cpp:733/750`). Every slice of a frame computes the
    // same value from the same two layer-level facts, so under MT that was N
    // workers writing the same bytes with no ordering — the write/write race
    // the fixed-slice fork probe stopped on the moment round 5's deblocking
    // race no longer aborted the run first. F71's pattern (T7.C3): the
    // loop-invariant write hoists to the frame level, before anything spawns;
    // the per-slice readers see the same value in the same order.
    let kbBaseAvail = (*pCurLayer).bBaseLayerAvailableFlag;
    let kbHighestSpatial = if !ctx_param(pCtx).is_null() {
        (*ctx_param(pCtx)).iSpatialLayerNum
            == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    fl.pfInterMd = if kbBaseAvail && kbHighestSpatial {
        Some(crate::encoder::svc_mode_decision::WelsMdInterMbEnhancelayer)
    } else {
        Some(crate::encoder::svc_base_layer_md::WelsMdInterMb)
    };
}

/// `encoder_ext.cpp:3131`. Write the parameter sets for (simulcast) SVC.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WriteSsvcParaset(
    pCtx: *mut sWelsEncCtx,
    kiSpatialNum: i32,
    ppLayerBsInfo: *mut *mut SLayerBSInfo,
    iLayerNum: *mut i32,
    iFrameSize: *mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iCountNal = 0i32;
    let pLayerBsInfo = *ppLayerBsInfo;

    let iReturn = crate::encoder::wels_encoder_ext::WelsWriteParameterSets(
        pCtx,
        (*pLayerBsInfo).pNalLengthInByte,
        &mut iCountNal,
        &mut iNonVclSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    for iSpatialId in 0..kiSpatialNum as usize {
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[iSpatialId]);
        if (*pParamInternal).uiIdrPicId < 65535 {
            (*pParamInternal).uiIdrPicId += 1;
        } else {
            (*pParamInternal).uiIdrPicId = 0;
        }
    }

    (*pLayerBsInfo).uiSpatialId = 0;
    (*pLayerBsInfo).uiTemporalId = 0;
    (*pLayerBsInfo).uiQualityId = 0;
    (*pLayerBsInfo).uiLayerType = NON_VIDEO_CODING_LAYER;
    (*pLayerBsInfo).iNalCount = iCountNal;
    (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeIDR;
    (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    // point to next pLayerBsInfo
    let pNext = pLayerBsInfo.add(1);
    *ppLayerBsInfo = pNext;
    (*(*pCtx).pOut).iLayerBsIndex += 1;
    (*pNext).pBsBuf = ctx_frame_bs_cur(pCtx);
    (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);

    // update for external countings
    *iLayerNum += 1;
    *iFrameSize += iNonVclSize;
    iReturn
}

/// `encoder_ext.cpp:3163`. Write the parameter sets for simulcast AVC.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WriteSavcParaset(
    pCtx: *mut sWelsEncCtx,
    iIdx: i32,
    ppLayerBsInfo: *mut *mut SLayerBSInfo,
    iLayerNum: *mut i32,
    iFrameSize: *mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iNalSize = 0i32;
    let mut iCountNal;
    let mut pLayerBsInfo = *ppLayerBsInfo;

    // --- SPS ---
    // Re-acquired here and again for the PPS below rather than held across the two
    // writes: `WelsWriteOneSPS`/`WelsWriteOnePPS` reach this same object through
    // `pCtx->pFuncList`. T4b.2a.
    if let Some(pStrategy) = (*ctx_func_list(pCtx)).pParametersetStrategy.as_mut() {
        pStrategy.Update(
            (*ctx_sps_array(pCtx).add(iIdx as usize)).uiSpsId,
            PARA_SET_TYPE_AVCSPS as i32,
        );
    }

    let mut iReturn =
        crate::encoder::wels_encoder_ext::WelsWriteOneSPS(pCtx, iIdx, &mut iNalSize);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    *(*pLayerBsInfo).pNalLengthInByte = iNalSize;
    iNonVclSize += iNalSize;
    iCountNal = 1;

    (*pLayerBsInfo).uiSpatialId = iIdx as u8;
    (*pLayerBsInfo).uiTemporalId = 0;
    (*pLayerBsInfo).uiQualityId = 0;
    (*pLayerBsInfo).uiLayerType = NON_VIDEO_CODING_LAYER;
    (*pLayerBsInfo).iNalCount = iCountNal;
    (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeIDR;
    (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    let mut pNext = pLayerBsInfo.add(1);
    (*(*pCtx).pOut).iLayerBsIndex += 1;
    (*pNext).pBsBuf = ctx_frame_bs_cur(pCtx);
    (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);
    *iLayerNum += 1;
    pLayerBsInfo = pNext;

    // --- PPS ---
    iNalSize = 0;
    if let Some(pStrategy) = (*ctx_func_list(pCtx)).pParametersetStrategy.as_mut() {
        pStrategy.Update(
            (*ctx_pps_array(pCtx).add(iIdx as usize)).iPpsId,
            PARA_SET_TYPE_PPS as i32,
        );
    }
    iReturn = crate::encoder::wels_encoder_ext::WelsWriteOnePPS(pCtx, iIdx, &mut iNalSize);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }
    *(*pLayerBsInfo).pNalLengthInByte = iNalSize;
    iNonVclSize += iNalSize;
    iCountNal = 1;

    (*pLayerBsInfo).uiSpatialId = iIdx as u8;
    (*pLayerBsInfo).uiTemporalId = 0;
    (*pLayerBsInfo).uiQualityId = 0;
    (*pLayerBsInfo).uiLayerType = NON_VIDEO_CODING_LAYER;
    (*pLayerBsInfo).iNalCount = iCountNal;
    (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeIDR;
    (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    pNext = pLayerBsInfo.add(1);
    (*(*pCtx).pOut).iLayerBsIndex += 1;
    (*pNext).pBsBuf = ctx_frame_bs_cur(pCtx);
    (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);
    *iLayerNum += 1;

    *ppLayerBsInfo = pNext;
    *iFrameSize += iNonVclSize;
    ENC_RETURN_SUCCESS
}

/// `encoder_ext.cpp:3251` — the parameter-set writer for the three **listing**
/// strategies (T8b.B3).
///
/// Its comment upstream says "cover the logic of simulcast avc + sps_pps_listing",
/// which understates it: the caller's test is `! (SPS_LISTING & eSpsPpsIdStrategy)`
/// (`:3424`), a **bitmask** over `codec_app_def.h`'s 0x02 / 0x03 / 0x06, so all three
/// listing strategies route here regardless of `bSimulcastAVC`.
///
/// What makes it different from [`WriteSavcParaset`] is that it writes *lists*: every
/// one of `iSpsNum` SPSs and, after `UpdatePpsList` has expanded the array, every one
/// of `iPpsNum` PPSs — per spatial layer, each list one `SLayerBSInfo`. That is the
/// point of a listing strategy: the decoder is given the whole set up front, so a
/// mid-stream re-initialisation can go back to an id it has already seen without
/// re-sending anything.
///
/// It does **not** call `Update`: under a listing strategy the ids are the list's, not
/// a rotation, and `Update` on those kinds is the inherited `CWelsParametersetIdConstant`
/// body that memsets the whole offset block (see `ParasetIdKind`'s note on `Update`).
///
/// # Safety
/// `pCtx` must be a live encoder context with its parameter-set arrays allocated to
/// the counts `GetNeededSpsNum`/`GetNeededPpsNum` asked `RequestMemorySvc` for, and
/// `ppLayerBsInfo` must name a slot with room for `2 * kiSpatialNum` more layers.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WriteSavcParaset_Listing(
    pCtx: *mut sWelsEncCtx,
    kiSpatialNum: i32,
    ppLayerBsInfo: *mut *mut SLayerBSInfo,
    iLayerNum: *mut i32,
    iFrameSize: *mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iReturn = ENC_RETURN_SUCCESS;
    let mut pLayerBsInfo = *ppLayerBsInfo;

    // --- SPS list, per spatial layer ---
    for iSpatialId in 0..kiSpatialNum {
        let pParamInternal =
            std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[iSpatialId as usize]);
        if (*pParamInternal).uiIdrPicId < 65535 {
            (*pParamInternal).uiIdrPicId += 1;
        } else {
            (*pParamInternal).uiIdrPicId = 0;
        }

        let mut iCountNal = 0i32;
        for iIdx in 0..(*pCtx).iSpsNum {
            let mut iNalSize = 0i32;
            iReturn = crate::encoder::wels_encoder_ext::WelsWriteOneSPS(pCtx, iIdx, &mut iNalSize);
            if iReturn != ENC_RETURN_SUCCESS {
                return iReturn;
            }
            *(*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize) = iNalSize;
            iNonVclSize += iNalSize;
            iCountNal += 1;
        }

        (*pLayerBsInfo).uiSpatialId = iSpatialId as u8;
        (*pLayerBsInfo).uiTemporalId = 0;
        (*pLayerBsInfo).uiQualityId = 0;
        (*pLayerBsInfo).uiLayerType = NON_VIDEO_CODING_LAYER;
        (*pLayerBsInfo).iNalCount = iCountNal;
        (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeIDR;
        (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

        let pNext = pLayerBsInfo.add(1);
        (*(*pCtx).pOut).iLayerBsIndex += 1;
        (*pNext).pBsBuf = ctx_frame_bs_cur(pCtx);
        (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);
        *iLayerNum += 1;
        pLayerBsInfo = pNext;
    }

    // --- PPS list, per spatial layer ---
    //
    // `encoder_ext.cpp:3297` — the one `UpdatePpsList` call site the port did not
    // have, because this function did not exist. It is a no-op for four of the five
    // kinds and the whole point of `SPS_PPS_LISTING`.
    ParasetStrategy(pCtx).UpdatePpsList(pCtx);

    for iSpatialId in 0..kiSpatialNum {
        let mut iCountNal = 0i32;
        for iIdx in 0..(*pCtx).iPpsNum {
            let mut iNalSize = 0i32;
            iReturn = crate::encoder::wels_encoder_ext::WelsWriteOnePPS(pCtx, iIdx, &mut iNalSize);
            if iReturn != ENC_RETURN_SUCCESS {
                return iReturn;
            }
            *(*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize) = iNalSize;
            iNonVclSize += iNalSize;
            iCountNal += 1;
        }

        (*pLayerBsInfo).uiSpatialId = iSpatialId as u8;
        (*pLayerBsInfo).uiTemporalId = 0;
        (*pLayerBsInfo).uiQualityId = 0;
        (*pLayerBsInfo).uiLayerType = NON_VIDEO_CODING_LAYER;
        (*pLayerBsInfo).iNalCount = iCountNal;
        (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeIDR;
        (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

        let pNext = pLayerBsInfo.add(1);
        (*(*pCtx).pOut).iLayerBsIndex += 1;
        (*pNext).pBsBuf = ctx_frame_bs_cur(pCtx);
        (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);
        *iLayerNum += 1;
        pLayerBsInfo = pNext;
    }

    *ppLayerBsInfo = pLayerBsInfo;

    // to check number of layers / nals / slices dependencies
    if *iLayerNum > MAX_LAYER_NUM_OF_FRAME {
        crate::common::wels_trace::WelsLog(
            std::ptr::addr_of_mut!((*pCtx).sLogCtx),
            crate::common::wels_trace::WELS_LOG_ERROR,
            &format!(
                "WriteSavcParaset(), iLayerNum({}) > MAX_LAYER_NUM_OF_FRAME({})!",
                *iLayerNum, MAX_LAYER_NUM_OF_FRAME
            ),
        );
        return ENC_RETURN_UNEXPECTED;
    }

    *iFrameSize += iNonVclSize;
    iReturn
}

/// `encoder_ext.cpp:3387`. Decide this frame's type, and for an IDR write the
/// parameter sets ahead of the slice data.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn PrepareEncodeFrame(
    pCtx: *mut sWelsEncCtx,
    ppLayerBsInfo: *mut *mut SLayerBSInfo,
    iSpatialNum: i32,
    iCurDid: *mut i8,
    iCurTid: *mut i32,
    iLayerNum: *mut i32,
    iFrameSize: *mut i32,
    uiTimeStamp: i64,
) -> EVideoFrameType {
    let pSvcParam = ctx_param(pCtx);

    let bSkipFrameFlag = crate::encoder::rc::WelsRcCheckFrameStatus(
        pCtx,
        uiTimeStamp,
        iSpatialNum,
        *iCurDid as i32,
    );
    let eFrameType = crate::encoder::encoder_context::DecideFrameType(
        pCtx,
        iSpatialNum as i8,
        *iCurDid as i32,
        bSkipFrameFlag,
    );

    if eFrameType == EVideoFrameType::videoFrameTypeSkip {
        // The `else if let Some(f)` this replaces read as a second condition but
        // was the same slot as the `if` branch's: the discriminator is
        // `bSimulcastAVC` alone, and an absent callback made both arms no-ops.
        let pfRc = (*ctx_func_list(pCtx)).pfRc;
        if (*pSvcParam).bSimulcastAVC {
            pfRc.WelsUpdateBufferWhenSkip(pCtx, *iCurDid as i32);
        } else {
            for i in 0..iSpatialNum as usize {
                // T9.G2, with `WelsEncoderEncodeExt`'s: the cursor is gone and the
                // index is read at the use. Hoisted as well — `WelsUpdateBufferWhenSkip`
                // takes the ctx retag and this argument reads through the same ctx.
                let iDid = (*pCtx).sSpatialIndexMap[i].iDid;
                pfRc.WelsUpdateBufferWhenSkip(pCtx, iDid);
            }
        }
    } else {
        let pParamInternal = (*pSvcParam)
            .sDependencyLayers
            .as_mut_ptr()
            .add(*iCurDid as usize);

        *iCurTid = GetTemporalLevel(
            pParamInternal,
            (*pParamInternal).iCodingIndex,
            (*pSvcParam).uiGopSize as i32,
        );
        (*pCtx).uiTemporalId = *iCurTid as u8;

        if eFrameType == EVideoFrameType::videoFrameTypeIDR {
            // write parameter sets bitstream or SEI/SSEI (if any) here
            if ((*ctx_param(pCtx)).eSpsPpsIdStrategy as i32
                & EParameterSetStrategy::SPS_LISTING as i32)
                == 0
            {
                if (*pSvcParam).bSimulcastAVC {
                    (*pCtx).iEncoderError = WriteSavcParaset(
                        pCtx,
                        *iCurDid as i32,
                        ppLayerBsInfo,
                        iLayerNum,
                        iFrameSize,
                    );
                    (*pParamInternal).uiIdrPicId += 1;
                } else {
                    (*pCtx).iEncoderError =
                        WriteSsvcParaset(pCtx, iSpatialNum, ppLayerBsInfo, iLayerNum, iFrameSize);
                }
            } else {
                // The three listing strategies, all of them: the C's test is
                // `! (SPS_LISTING & eSpsPpsIdStrategy)`, a bitmask over 0x02/0x03/0x06.
                // This arm was `ENC_RETURN_UNSUPPORTED_PARA` until T8b.B3 (the S48
                // shape, while the strategies were unported).
                (*pCtx).iEncoderError = WriteSavcParaset_Listing(
                    pCtx,
                    iSpatialNum,
                    ppLayerBsInfo,
                    iLayerNum,
                    iFrameSize,
                );
            }
        }
    }
    eFrameType
}

/// `encoder_ext.cpp:2415`. TUNE back if a picture-partition decision algorithm based
/// on past behaviour becomes available.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn PicPartitionNumDecision(pCtx: *mut sWelsEncCtx) -> i32 {
    let mut iPartitionNum = 1;
    if (*ctx_param(pCtx)).iMultipleThreadIdc > 1 {
        iPartitionNum = (*ctx_param(pCtx)).iMultipleThreadIdc as i32;
    }
    iPartitionNum
}

/// `DynslcUpdateMbNeighbourInfoListForAllSlices` — encoder_ext.cpp:2397.
///
/// # Safety
/// `pCurDq` must be live with `sMbDataP` allocated.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DynslcUpdateMbNeighbourInfoListForAllSlices(pCurDq: &mut SDqLayer) {
    let kiMbWidth = (*pCurDq).sSliceEncCtx.iMbWidth as i32;
    let kiEndMbInSlice = (*pCurDq).sSliceEncCtx.iMbNumInFrame - 1;
    let mut iIdx = 0i32;
    let mut mbs =
        crate::encoder::svc_encode_slice::mb_window(pCurDq, 0, kiEndMbInSlice + 1, 0);

    loop {
        let uiSliceIdc = crate::encoder::svc_encode_slice::WelsMbToSliceIdc(
            pCurDq,
            mbs.at(iIdx as usize).iMbXY as i32,
        );
        crate::encoder::svc_encode_slice::UpdateMbNeighbor(
            pCurDq,
            mbs.at_mut(iIdx as usize),
            kiMbWidth,
            uiSliceIdc,
        );
        iIdx += 1;
        if iIdx > kiEndMbInSlice {
            break;
        }
    }
}

/// `WelsInitCurrentQBLayerMltslc` — encoder_ext.cpp:2423.
///
/// # Safety
/// `pCtx` must be a context built by [`WelsInitEncoderExt`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitCurrentQBLayerMltslc(pCtx: *mut sWelsEncCtx) {
    // pData init
    let pCurDq = current_layer(pCtx);
    // mb_neighbor
    // T9.E2h, F66's shape B with an accessor-minted root the detector cannot
    // see: the MB-list root is minted BEFORE the layer argument's retag (its
    // buffer is a separate allocation, so the retag cannot reach it).
    DynslcUpdateMbNeighbourInfoListForAllSlices(&mut *pCurDq);
}

/// `UpdateSlicepEncCtxWithPartition` — encoder_ext.cpp:2430.
///
/// Splits the frame into `iPartitionNum` macroblock ranges and stamps
/// `pOverallMbMap` with the partition index. Note the trailing loop clears the
/// *whole* of the four partition arrays out to `MAX_THREADS_NUM`, not just the
/// entries beyond `iPartitionNum` that this call wrote.
///
/// # Safety
/// `pCurDq` must be live with `sSliceEncCtx.pOverallMbMap` allocated.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateSlicepEncCtxWithPartition(pCurDq: &mut SDqLayer, mut iPartitionNum: i32) {
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let kiMbNumInFrame = pSliceCtx.iMbNumInFrame;
    let mut iCountMbNumPerPartition = kiMbNumInFrame;
    let mut iAssignableMbLeft = kiMbNumInFrame;
    let mut iCountMbNumInPartition;
    let mut iFirstMbIdx = 0i32;
    let mut i: usize;

    if iPartitionNum <= 0 {
        iPartitionNum = 1;
    } else if iPartitionNum
        > crate::encoder::svc_enc_slice_segment::AVERSLICENUM_CONSTRAINT as i32
    {
        iPartitionNum = crate::encoder::svc_enc_slice_segment::AVERSLICENUM_CONSTRAINT as i32;
    }
    iCountMbNumPerPartition /= iPartitionNum;
    if iCountMbNumPerPartition == 0 || iCountMbNumPerPartition == 1 {
        iCountMbNumPerPartition = kiMbNumInFrame;
        iPartitionNum = 1;
    }

    pSliceCtx.iSliceNumInFrame.store(iPartitionNum, Ordering::Relaxed);

    i = 0;
    while i < iPartitionNum as usize {
        if i + 1 == iPartitionNum as usize {
            iCountMbNumInPartition = iAssignableMbLeft;
        } else {
            iCountMbNumInPartition = iCountMbNumPerPartition;
        }

        (*pCurDq).FirstMbIdxOfPartition[i] = iFirstMbIdx;
        (*pCurDq).EndMbIdxOfPartition[i] = iFirstMbIdx + iCountMbNumInPartition - 1;
        (*pCurDq).LastCodedMbIdxOfPartition[i] = 0;
        (*pCurDq).NumSliceCodedOfPartition[i] = 0;

        {
            let map: &[AtomicU16] = &(*pCurDq).sSliceEncCtx.pOverallMbMap;
            crate::encoder::slice_multi_threading::fill_mb_map(
                map,
                iFirstMbIdx,
                iCountMbNumInPartition,
                i as u16,
            );
        }

        // for next partition (or pSlice)
        iFirstMbIdx += iCountMbNumInPartition;
        iAssignableMbLeft -= iCountMbNumInPartition;
        i += 1;
    }

    while i < MAX_THREADS_NUM {
        (*pCurDq).FirstMbIdxOfPartition[i] = 0;
        (*pCurDq).EndMbIdxOfPartition[i] = 0;
        (*pCurDq).LastCodedMbIdxOfPartition[i] = 0;
        (*pCurDq).NumSliceCodedOfPartition[i] = 0;
        i += 1;
    }
}

/// `WelsInitCurrentDlayerMltslc` — encoder_ext.cpp:2482.
///
/// The I-slice block only logs a warning when `uiSliceSizeConstraint` is too
/// small for the resolution; it does not clamp or fail, so nothing in the
/// bitstream depends on it. It is transcribed anyway because `uiFrmByte`'s
/// arithmetic is unsigned and the shift is data-dependent.
///
/// # Safety
/// `pCtx` must be a context built by [`WelsInitEncoderExt`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitCurrentDlayerMltslc(pCtx: *mut sWelsEncCtx, iPartitionNum: i32) {
    /// `#define byte_complexIMBat26 (60)`, local to this function in the C++.
    const byte_complexIMBat26: u32 = 60;

    let pCurDq = current_layer(pCtx);

    UpdateSlicepEncCtxWithPartition(&mut *pCurDq, iPartitionNum);

    if (*pCtx).eSliceType == EWelsSliceType::I_SLICE {
        // check if uiSliceSizeConstraint too small
        let iCurDid = (*pCtx).uiDependencyId as usize;
        let mut uiFrmByte: u32;

        if (*ctx_param(pCtx)).iRCMode != crate::RCMode::RC_OFF_MODE {
            // RC case
            uiFrmByte = (((*ctx_param(pCtx)).sSpatialLayers[iCurDid].iSpatialBitrate as u32)
                / ((*ctx_param(pCtx)).sDependencyLayers[iCurDid].fInputFrameRate as u32))
                >> 3;
        } else {
            // fixed QP case
            let iTtlMbNumInFrame = (*pCurDq).sSliceEncCtx.iMbNumInFrame;
            let mut iQDeltaTo26 = 26 - (*ctx_param(pCtx)).sSpatialLayers[iCurDid].iDLayerQp;

            uiFrmByte = (iTtlMbNumInFrame as u32).wrapping_mul(byte_complexIMBat26);
            if iQDeltaTo26 > 0 {
                // smaller QP than 26
                uiFrmByte = (uiFrmByte as f32 * (iQDeltaTo26 as f32 / 4.0)) as u32;
            } else if iQDeltaTo26 < 0 {
                // larger QP than 26
                iQDeltaTo26 = (-iQDeltaTo26) >> 2; // delta mod 4
                uiFrmByte >>= iQDeltaTo26; // if delta 4, byte /2
            }
        }

        // MINPACKETSIZE_CONSTRAINT: suppose 16 byte per mb at average
        let _uiMiniPacketSize = uiFrmByte / (*pCurDq).sSliceEncCtx.iMaxSliceNumConstraint as u32;
        // C++ only WelsLogs a warning here when uiSliceSizeConstraint is smaller.
    }

    WelsInitCurrentQBLayerMltslc(pCtx);
}

/// `DynSliceRealloc` — encoder_ext.cpp:4525.
///
/// # Safety
/// `pCtx` must be a context built by [`WelsInitEncoderExt`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DynSliceRealloc(
    pCtx: *mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
) -> i32 {
    // T9.G6: hoisted — the call takes the context retag and this argument reads
    // through the same context (shape B).
    let iMaxSliceNum = (*current_layer(pCtx)).iMaxSliceNum;
    let mut iRet = crate::encoder::svc_encode_slice::FrameBsRealloc(
        pCtx,
        pFrameBsInfo,
        pLayerBsInfo,
        iMaxSliceNum,
    );
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    iRet = crate::encoder::svc_encode_slice::ReallocSliceBuffer(pCtx);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    iRet
}

/// `WelsCodeOnePicPartition` — encoder_ext.cpp:4543.
///
/// The dynamic-slicing coding loop: keeps emitting slices until the partition's
/// macroblocks are exhausted, where "exhausted" is measured by
/// `LastCodedMbIdxOfPartition`, which `AddSliceBoundary` advances — not by a
/// slice counter. `iSliceIdx` steps by `iActiveThreadsNum`, so slice indices are
/// **not** contiguous when more than one partition is in play.
///
/// # Safety
/// `pCtx` must be a context built by [`WelsInitEncoderExt`]; `pLayerBsInfo` must
/// have `pNalLengthInByte` installed.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsCodeOnePicPartition(
    pCtx: *mut sWelsEncCtx,
    pFrameBSInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
    pNalIdxInLayer: *mut i32,
    pLayerSize: *mut i32,
    iFirstMbIdxInPartition: i32,
    iEndMbIdxInPartition: i32,
    iStartSliceIdx: i32,
) -> i32 {
    let pCurLayer = current_layer(pCtx);
    let uSlcBuffIdx = 0usize;
    let pStartSlice = crate::encoder::svc_encode_slice::slice_in_bank(pCurLayer, uSlcBuffIdx, iStartSliceIdx);
    if pStartSlice.is_null() {
        return ENC_RETURN_UNEXPECTED;
    }
    let mut iNalIdxInLayer = *pNalIdxInLayer;
    let mut iSliceIdx = iStartSliceIdx;
    let kiSliceStep = (*pCtx).iActiveThreadsNum as i32;
    let kiPartitionId = (iStartSliceIdx % kiSliceStep) as usize;
    let mut iPartitionBsSize = 0i32;
    let mut iAnyMbLeftInPartition = iEndMbIdxInPartition - iFirstMbIdxInPartition + 1;
    let keNalType = (*pCtx).eNalType;
    let keNalRefIdc = (*pCtx).eNalPriority;
    let kbNeedPrefix = (*pCtx).bNeedPrefixNalFlag;
    let kiSliceIdxStep = (*pCtx).iActiveThreadsNum as i32;
    let mut iReturn;

    (*pStartSlice)
        .sSliceHeaderExt
        .sSliceHeader
        .iFirstMbInSlice = iFirstMbIdxInPartition;

    while iAnyMbLeftInPartition > 0 {
        let mut iPayloadSize = 0i32;

        if iSliceIdx
            >= ((*pCurLayer).sSliceBufferInfo[uSlcBuffIdx].iMaxSliceNum - kiSliceIdxStep)
        {
            // insufficient memory in pSliceInLayer[]
            if (*pCtx).iActiveThreadsNum == 1 {
                // only single thread supports re-alloc now
                if DynSliceRealloc(pCtx, pFrameBSInfo, pLayerBsInfo) != 0 {
                    return ENC_RETURN_MEMALLOCERR;
                }
            } else if iSliceIdx >= (*pCurLayer).iMaxSliceNum {
                return ENC_RETURN_MEMALLOCERR;
            }
        }

        if kbNeedPrefix {
            iReturn = AddPrefixNal(
                pCtx,
                pLayerBsInfo,
                (*pLayerBsInfo).pNalLengthInByte,
                &mut iNalIdxInLayer,
                keNalType,
                keNalRefIdc,
                &mut iPayloadSize,
            );
            if iReturn != ENC_RETURN_SUCCESS {
                return iReturn;
            }
            iPartitionBsSize += iPayloadSize;
        }

        crate::encoder::nal_encap::WelsLoadNal((*pCtx).pOut, keNalType as i32, keNalRefIdc as i32);
        let pCurSlice = crate::encoder::svc_encode_slice::slice_in_bank(current_layer(pCtx), uSlcBuffIdx, iSliceIdx);
        (*pCurSlice).iSliceIdx = iSliceIdx;

        // T7.C3: the layer-level half of `WelsCodeOneSlice`'s I_SLICE arm, one line
        // above the call it was lifted out of — this path is single-threaded, so the
        // sequence is unchanged.
        crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);

        iReturn = crate::encoder::svc_encode_slice::WelsCodeOneSlice(
            pCtx,
            &mut *pCurSlice,
            keNalType as i32,
        );
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

        iReturn = crate::encoder::nal_encap::WelsEncodeNal(
            &(&*(*pCtx).pOut).sNalList[((*(*pCtx).pOut).iNalIndex - 1) as usize],
            &(&*(*pCtx).pOut).sBsBuffer[..],
            Some(&(*current_layer(pCtx)).sLayerInfo.sNalHeaderExt),
            ctx_frame_bs_cur(pCtx),
            (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
            &mut *(*pLayerBsInfo).pNalLengthInByte.add(iNalIdxInLayer as usize),
        );
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        let iSliceSize = *(*pLayerBsInfo).pNalLengthInByte.add(iNalIdxInLayer as usize);

        (*pCtx).iPosBsBuffer += iSliceSize;
        iPartitionBsSize += iSliceSize;

        iNalIdxInLayer += 1;
        iSliceIdx += kiSliceStep; // iSliceIdx is not contiguous
        iAnyMbLeftInPartition =
            iEndMbIdxInPartition - (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId];
    }

    *pLayerSize = iPartitionBsSize;
    *pNalIdxInLayer = iNalIdxInLayer;

    // slice based packing???
    (*pLayerBsInfo).uiLayerType = VIDEO_CODING_LAYER;
    (*pLayerBsInfo).uiSpatialId = (*pCtx).uiDependencyId as u8;
    (*pLayerBsInfo).uiTemporalId = (*pCtx).uiTemporalId as u8;
    (*pLayerBsInfo).uiQualityId = 0;
    (*pLayerBsInfo).iNalCount = iNalIdxInLayer;
    ENC_RETURN_SUCCESS
}

/// `encoder_ext.cpp:3448` — the core SVC encoding process.
///
/// Replaces the `WelsEncoderEncodeExtRust` sketch, which hardcoded an IDR, wrote one
/// slice, and skipped the frame-type/GOP decision, rate control, reference lists,
/// preprocessing and padding entirely.
///
/// # Unported branches — **none left; this list was stale** (T8b.B4)
///
/// It named two, and Phase 7 ported both. `iMultipleThreadIdc > 1` runs through
/// `RequestMtResource` (`:1141`), `InitAllSlicesInThread`
/// (`svc_encode_slice.rs:3315`) and `SliceLayerInfoUpdate` (`:3926`);
/// `SM_SIZELIMITED_SLICE` runs through `WelsCodeOnePicPartition` (`:3032`) and
/// `WelsInitCurrentDlayerMltslc` (`:2948`). Every function the paragraph called
/// unported has been a live definition since then, and both configurations are swept
/// (`sweep.sh`'s `mt` and `sl` presets).
///
/// The `ENC_RETURN_UNSUPPORTED_PARA` returns that remain in this function are the
/// layer-count bounds (`iLayerNum >= MAX_LAYER_NUM_OF_FRAME`), not feature refusals.
///
/// # Safety
/// `pCtx` must be a context built by [`WelsInitEncoderExt`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsEncoderEncodeExt(
    pCtx: *mut sWelsEncCtx,
    pFbi: *mut SFrameBSInfo,
    pSrcPic: *const SSourcePicture,
) -> i32 {
    if pCtx.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    let pSvcParam = ctx_param(pCtx);
    // The reconstruction picture the PSNR block measures, **as a handle** — T9.B3.
    // It was `Option<PicPlanes>`, three raw plane roots copied out of the picture
    // six hundred lines above their only reader; it is now the handle those roots
    // were derived from, and `LayerPlanePsnr` resolves the picture where it reads
    // it. The source picture beside it was the same shape and is gone entirely —
    // `idEncPic`, a local of the layer body, already names it.
    //
    // **The snapshot itself is load-bearing and stays** (F109). `(*pCtx).pDecPic`
    // cannot be re-read at the PSNR block, because `UpdateRefList` runs in between
    // and ends in `EndofUpdateRefList` -> `PrefetchNextBuffer`, which reassigns it
    // to the *next* frame's target. What S37 made this a copy for was the borrow,
    // not the value; the value has to be captured here either way.
    let mut fsnr: Option<crate::encoder::picture::RecPicId>;
    let mut iLayerNum = 0i32;
    let mut iLayerSize;
    let mut iSpatialNum = 0i32;
    let mut iSpatialIdx = 0i32;
    let mut iFrameSize = 0i32;
    let mut iNalIdxInLayer;
    let mut iCountNal;
    let mut eFrameType = EVideoFrameType::videoFrameTypeInvalid;
    let mut iCurWidth;
    let mut iCurHeight;
    let mut eNalType = EWelsNalUnitType::NAL_UNIT_UNSPEC_0;
    let mut eNalRefIdc;
    let mut iCurDid: i8 = 0;
    let mut iCurTid: i32 = 0;

    (*pCtx).iEncoderError = ENC_RETURN_SUCCESS;
    (*pCtx).bCurFrameMarkedAsSceneLtr = false;
    (*pFbi).eFrameType = EVideoFrameType::videoFrameTypeSkip;
    (*pFbi).iLayerNum = 0; // for initialization
    (*pFbi).uiTimeStamp = crate::encoder::rc::GetTimestampForRc(
        (*pSrcPic).uiTimeStamp,
        (*pCtx).uiLastTimestamp,
        (*pSvcParam).sSpatialLayers[(*pSvcParam).iSpatialLayerNum as usize - 1].fFrameRate,
    );
    for iNalIdx in 0..MAX_LAYER_NUM_OF_FRAME as usize {
        (*pFbi).sLayerInfo[iNalIdx].eFrameType = EVideoFrameType::videoFrameTypeSkip;
        (*pFbi).sLayerInfo[iNalIdx].iNalCount = 0;
    }

    // Derived after the reset loop above, for the reason `pSpatialIndexMap` used to
    // be derived after `BuildSpatialPicList` (T9.G2 retired that binding): the loop
    // above **writes**
    // `(*pFbi).sLayerInfo[..]` through `pFbi`, and a write through the parent pops
    // a child taken before it. Every use of this cursor is below.
    // T9.E7: `addr_of_mut!`, not `as_mut_ptr()` — the array method autorefs
    // `&mut (*pFbi).sLayerInfo` first, so the old mint was a raw ABOVE a Unique,
    // and any sibling raw's write into an entry (the size-limited branch's
    // `pLbi` stamps below) popped it before `SliceLayerInfoUpdate` wrote back
    // through it. A place projection reuses `pFbi`'s provenance; the two mints
    // are then raw siblings, which writes do not pop (T5.O8, F70).
    let mut pLayerBsInfo: *mut SLayerBSInfo = std::ptr::addr_of_mut!((*pFbi).sLayerInfo).cast::<SLayerBSInfo>();

    // perform csc/denoise/downsample/padding, generate spatial layers
    let iRet = (*(*pCtx).pVpp).BuildSpatialPicList(pCtx, pSrcPic, &mut iSpatialNum);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    (*ctx_func_list(pCtx))
        .pfRc
        .WelsUpdateMaxBrWindowStatus(pCtx, iSpatialNum, (*pFbi).uiTimeStamp);

    if iSpatialNum < 1 {
        for iDidIdx in 0..(*pSvcParam).iSpatialLayerNum as usize {
            (*pSvcParam).sDependencyLayers[iDidIdx].iCodingIndex += 1;
        }
        (*pFbi).eFrameType = EVideoFrameType::videoFrameTypeSkip;
        (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeSkip;
        return ENC_RETURN_SUCCESS;
    }

    // **`pSpatialIndexMap` stood here — T9.G2, the largest single item in the ctx
    // hazard campaign.** It was `(*pCtx).sSpatialIndexMap.as_ptr()`, held from here
    // to the end of the function across every context call in between: **58 of the
    // 131 live shape-A hazards in the whole encoder** (`phase9_ctx_join.py`), one
    // binding.
    //
    // Phase 6 session A had already been forced to move its *derivation* down here,
    // because `BuildSpatialPicList` above writes the array through the parent and a
    // write through the parent pops a `SharedReadOnly` child taken before it (S29's
    // boundary clause). That fix bought the derivation order and left the holding.
    // Deriving at each use — which is what `rc.rs:1881` and its four siblings have
    // always done for this same field — buys both, and needs no ordering argument at
    // all: `sSpatialIndexMap` is an inline `[SSpatialPicIndex; 4]` in the context,
    // so an index is a field read, not a cursor.
    crate::encoder::encoder_context::InitBitStream(pCtx);
    (*pLayerBsInfo).pBsBuf = ctx_frame_bs(pCtx);
    (*pLayerBsInfo).pNalLengthInByte = (*(*pCtx).pOut).sNalLen.as_mut_ptr();
    iCurDid = (*pCtx).sSpatialIndexMap[0].iDid as i8;
    set_current_layer(pCtx, Some(LayerIdx(iCurDid as u8)));
    (*current_layer(pCtx)).pRefLayer = None;

    if !(*pSvcParam).bSimulcastAVC {
        eFrameType = PrepareEncodeFrame(
            pCtx,
            &mut pLayerBsInfo,
            iSpatialNum,
            &mut iCurDid,
            &mut iCurTid,
            &mut iLayerNum,
            &mut iFrameSize,
            (*pFbi).uiTimeStamp,
        );
        if eFrameType == EVideoFrameType::videoFrameTypeSkip {
            (*pFbi).eFrameType = EVideoFrameType::videoFrameTypeSkip;
            (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeSkip;
            return ENC_RETURN_SUCCESS;
        }
        if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
            return (*pCtx).iEncoderError;
        }
    } else {
        for iDidIdx in 0..(*pSvcParam).iSpatialLayerNum as usize {
            let pParamInternal = std::ptr::addr_of_mut!((*pSvcParam).sDependencyLayers[iDidIdx]);
            let iTemporalId = GetTemporalLevel(
                pParamInternal,
                (*pParamInternal).iCodingIndex,
                (*pSvcParam).uiGopSize as i32,
            );
            if iTemporalId == INVALID_TEMPORAL_ID as i32 {
                (*pParamInternal).iCodingIndex += 1;
            }
        }
    }

    while iSpatialIdx < iSpatialNum {
        iCurDid = (*pCtx).sSpatialIndexMap[iSpatialIdx as usize].iDid as i8;
        // S29 / F13's family (the encode probe's sixth red, session B): `addr_of_mut!`
        // on the element — `as_mut_ptr().add()` reborrowed the whole array, and the
        // `.iPOC` reads below re-derived it and popped these.
        let pParam: *mut SSpatialLayerConfig =
            std::ptr::addr_of_mut!((*pSvcParam).sSpatialLayers[iCurDid as usize]);
        let pParamInternal =
            std::ptr::addr_of_mut!((*pSvcParam).sDependencyLayers[iCurDid as usize]);
        let iDecompositionStages = (*pParamInternal).iDecompositionStages as i32;
        set_current_layer(pCtx, Some(LayerIdx(iCurDid as u8)));
        (*pCtx).uiDependencyId = iCurDid as u8;

        if (*pSvcParam).bSimulcastAVC {
            eFrameType = PrepareEncodeFrame(
                pCtx,
                &mut pLayerBsInfo,
                iSpatialNum,
                &mut iCurDid,
                &mut iCurTid,
                &mut iLayerNum,
                &mut iFrameSize,
                (*pFbi).uiTimeStamp,
            );
            if eFrameType == EVideoFrameType::videoFrameTypeSkip {
                (*pLayerBsInfo).eFrameType = EVideoFrameType::videoFrameTypeSkip;
                iSpatialIdx += 1;
                continue;
            }
        }
        crate::encoder::encoder_context::InitFrameCoding(pCtx, eFrameType, iCurDid as i32);
        (*(*pCtx).pVpp).AnalyzeSpatialPic(pCtx, iCurDid as i32);

        // **`iPOC` is read at each use below rather than through a held pointer.**
        // Every call in this loop — `InitFrameCoding`, `AnalyzeSpatialPic`,
        // `BuildRefList` — writes this record through its own derivation, and a
        // write through the parent kills a pointer taken before it. No spelling
        // rescues that (S29's boundary clause); only ordering does, and deriving at
        // the use is the ordering that holds however the calls are rearranged. The
        // binding above stays correct for `iDecompositionStages`, read before any
        // of them. Found by the encoder aliasing probe, Phase 6 session A.
        let idEncPic = (*pCtx).sSpatialIndexMap[iSpatialIdx as usize]
            .pSrc
            .expect("the spatial index map names a live source picture");
        (*pCtx).pEncPic = Some(idEncPic);
        {
            let p = (*(*pCtx).pVpp).m_pSpatialPicPool.get_mut(idEncPic);
            p.iPictureType = (*pCtx).eSliceType as i32;
            p.iFramePoc = (*pSvcParam).sDependencyLayers[iCurDid as usize].iPOC;
        }

        iCurWidth = (*pParam).iVideoWidth;
        iCurHeight = (*pParam).iVideoHeight;

        match (*pParam).sSliceArgument.uiSliceMode {
            // **The consumer half of the load-balancing loop.** The producer,
            // `CalcSliceComplexRatio`, runs at the end of this same layer body under
            // the same four-term guard — added at T7.C1, which is what closed F72;
            // before that the ratios these adjusters read were permanently zero and
            // the balance was degenerate. Reachable by default, since
            // `GetDefaultParams` sets `bUseLoadBalancing = true` on both sides, and
            // **un-gateable by construction**: the boundaries are a function of
            // measured per-slice times, so two C++ runs differ. The diffharness pins
            // the flag off (`cxx_enc.cpp:119`) and so does the encoder probe; the
            // path's coverage is structural (F72's expected-divergent class).
            SliceModeEnum::SM_FIXEDSLCNUM_SLICE => {
                if (*pSvcParam).iMultipleThreadIdc > 1
                    && (*pSvcParam).bUseLoadBalancing
                    && (*pSvcParam).iMultipleThreadIdc
                        >= (*pSvcParam).sSpatialLayers[iCurDid as usize]
                            .sSliceArgument
                            .uiSliceNum as u16
                {
                    if iCurDid > 0 {
                        crate::encoder::slice_multi_threading::AdjustEnhanceLayer(pCtx, iCurDid as i32);
                    } else {
                        crate::encoder::slice_multi_threading::AdjustBaseLayer(pCtx);
                    }
                }
            }
            SliceModeEnum::SM_SIZELIMITED_SLICE => {
                let iPicIPartitionNum = PicPartitionNumDecision(pCtx);
                // MT compatibility: try to activate a number of threads equal to
                // the number of picture partitions.
                (*pCtx).iActiveThreadsNum = iPicIPartitionNum as i16;
                WelsInitCurrentDlayerMltslc(pCtx, iPicIPartitionNum);
            }
            _ => {}
        }

        // coding each spatial layer, only one quality layer within spatial support
        let mut iSliceCount;
        if iLayerNum >= MAX_LAYER_NUM_OF_FRAME {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }

        iNalIdxInLayer = 0;
        let bAvcBased = (*pSvcParam).bSimulcastAVC || (iCurDid as i32) == BASE_DEPENDENCY_ID;
        (*pCtx).bNeedPrefixNalFlag = !(*pSvcParam).bSimulcastAVC
            && bAvcBased
            && ((*pSvcParam).bPrefixNalAddingCtrl || (*pSvcParam).iSpatialLayerNum > 1);

        if eFrameType == EVideoFrameType::videoFrameTypeP {
            eNalType = if bAvcBased {
                EWelsNalUnitType::NAL_UNIT_CODED_SLICE
            } else {
                EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT
            };
        } else if eFrameType == EVideoFrameType::videoFrameTypeIDR {
            eNalType = if bAvcBased {
                EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
            } else {
                EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT
            };
        }
        if iCurTid == 0 || (*pCtx).eSliceType == EWelsSliceType::I_SLICE {
            eNalRefIdc = EWelsNalRefIdc::NRI_PRI_HIGHEST;
        } else if iCurTid == iDecompositionStages {
            eNalRefIdc = EWelsNalRefIdc::NRI_PRI_LOWEST;
        } else if 1 + iCurTid == iDecompositionStages {
            eNalRefIdc = EWelsNalRefIdc::NRI_PRI_LOW;
        } else if 2 + iCurTid == iDecompositionStages {
            eNalRefIdc = EWelsNalRefIdc::NRI_PRI_HIGH;
        } else {
            eNalRefIdc = EWelsNalRefIdc::NRI_PRI_HIGHEST;
        }
        (*pCtx).eNalType = eNalType;
        (*pCtx).eNalPriority = eNalRefIdc;

        let pRefListCur = ctx_ref_list(pCtx, iCurDid as usize);
        (*pCtx).pDecPic = (*pRefListCur).pNextBuffer;
        fsnr = (*pCtx).pDecPic;
        if let Some(id) = fsnr {
            let p = (*pRefListCur).pic_mut(id);
            p.iPictureType = (*pCtx).eSliceType as i32;
            p.iFramePoc = (*pSvcParam).sDependencyLayers[iCurDid as usize].iPOC;
        }

        WelsInitCurrentLayer(pCtx, iCurWidth, iCurHeight);

        let eRefStrategy = (*pCtx).eRefStrategy;
        eRefStrategy.MarkPic(pCtx);
        if !eRefStrategy.BuildRefList(pCtx, (*pSvcParam).sDependencyLayers[iCurDid as usize].iPOC, 0) {
            eFrameType = EVideoFrameType::videoFrameTypeIDR;
            (*pCtx).iEncoderError = ENC_RETURN_CORRECTED;
            break;
        }
        if (*pCtx).eSliceType != EWelsSliceType::I_SLICE {
            eRefStrategy.AfterBuildRefList(pCtx);
        }

        if (*pSvcParam).iRCMode != RC_OFF_MODE {
            let pRef = if (*pCtx).eSliceType == EWelsSliceType::P_SLICE && (*pCtx).iNumRef0 > 0 {
                (*pCtx).pRefList0[0]
            } else {
                None
            };
            // T9.G6: hoisted — the call takes the context retag and these arguments
            // read through the same context (shape B).
            let idEncPicForVaa = (*pCtx).pEncPic;
            let bBgd = (*pCtx).eSliceType == EWelsSliceType::P_SLICE
                && (*pSvcParam).bEnableBackgroundDetection;
            (*(*pCtx).pVpp).AnalyzePictureComplexity(
                pCtx,
                idEncPicForVaa,
                pRef,
                iCurDid as i32,
                bBgd,
            );
        }
        // get reordering syntax used for writing the slice header
        crate::encoder::ref_list_mgr_svc::WelsUpdateRefSyntax(
            pCtx,
            (*pSvcParam).sDependencyLayers[iCurDid as usize].iPOC,
            eFrameType as i32,
        );
        // update reference picture for the current DQ layer
        PrefetchReferencePicture(pCtx, eFrameType);
        (*ctx_func_list(pCtx))
            .pfRc
            .WelsRcPictureInit(pCtx, (*pFbi).uiTimeStamp);
        // MUST be called after pfWelsRcPictureInit() and WelsInitCurrentLayer()
        PreprocessSliceCoding(pCtx);

        iLayerSize = 0;
        if (*pParam).sSliceArgument.uiSliceMode == SM_SINGLE_SLICE {
            // only one slice within a quality layer
            let mut iPayloadSize = 0i32;
            let pCurSlice = crate::encoder::svc_encode_slice::slice_in_bank(current_layer(pCtx), 0, 0);

            if (*pCtx).bNeedPrefixNalFlag {
                (*pCtx).iEncoderError = AddPrefixNal(
                    pCtx,
                    pLayerBsInfo,
                    (*pLayerBsInfo).pNalLengthInByte,
                    &mut iNalIdxInLayer,
                    eNalType,
                    eNalRefIdc,
                    &mut iPayloadSize,
                );
                if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                    return (*pCtx).iEncoderError;
                }
                iLayerSize += iPayloadSize;
            }

            crate::encoder::nal_encap::WelsLoadNal(
                (*pCtx).pOut,
                eNalType as i32,
                eNalRefIdc as i32,
            );
            debug_assert_eq!(0, (*pCurSlice).iSliceIdx);
            (*pCtx).iEncoderError = crate::encoder::svc_encode_slice::SetSliceBoundaryInfo(
                current_layer(pCtx),
                &mut *pCurSlice,
                0,
            );
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }

            // T7.C3, as above.
            crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);
            (*pCtx).iEncoderError =
                crate::encoder::svc_encode_slice::WelsCodeOneSlice(pCtx, &mut *pCurSlice, eNalType as i32);
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }

            crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

            (*pCtx).iEncoderError = crate::encoder::nal_encap::WelsEncodeNal(
                &(&*(*pCtx).pOut).sNalList[(*(*pCtx).pOut).iNalIndex as usize - 1],
                &(&*(*pCtx).pOut).sBsBuffer[..],
                Some(&(*current_layer(pCtx)).sLayerInfo.sNalHeaderExt),
                ctx_frame_bs_cur(pCtx),
                (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
                &mut *(*pLayerBsInfo).pNalLengthInByte.add(iNalIdxInLayer as usize),
            );
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }
            let iSliceSize = *(*pLayerBsInfo).pNalLengthInByte.add(iNalIdxInLayer as usize);

            iLayerSize += iSliceSize;
            (*pCtx).iPosBsBuffer += iSliceSize;
            iNalIdxInLayer += 1;
            (*pLayerBsInfo).uiLayerType = VIDEO_CODING_LAYER;
            (*pLayerBsInfo).uiSpatialId = iCurDid as u8;
            (*pLayerBsInfo).uiTemporalId = iCurTid as u8;
            (*pLayerBsInfo).uiQualityId = 0;
            (*pLayerBsInfo).iNalCount = iNalIdxInLayer;
            (*pLayerBsInfo).eFrameType = eFrameType;
            (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
        } else if (*pParam).sSliceArgument.uiSliceMode == SM_SIZELIMITED_SLICE
            && (*pSvcParam).iMultipleThreadIdc <= 1
        {
            // dynamic slicing, single threading
            let kiLastMbInFrame = (*current_layer(pCtx)).sSliceEncCtx.iMbNumInFrame;
            (*pCtx).iEncoderError = WelsCodeOnePicPartition(
                pCtx,
                pFbi,
                pLayerBsInfo,
                &mut iNalIdxInLayer,
                &mut iLayerSize,
                0,
                kiLastMbInFrame - 1,
                0,
            );
            (*pLayerBsInfo).eFrameType = eFrameType;
            (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }
        } else if (*pParam).sSliceArgument.uiSliceMode != SM_SIZELIMITED_SLICE
            && (*pSvcParam).iMultipleThreadIdc > 1
        {
            // THREAD_FULLY_FIRE_MODE/THREAD_PICK_UP_MODE for any mode of
            // non-SM_SIZELIMITED_SLICE
            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(&mut *current_layer(pCtx));
            if iLayerNum + 1 >= MAX_LAYER_NUM_OF_FRAME as i32 {
                // check available layer_bs_info for further writing as followed
                return ENC_RETURN_UNSUPPORTED_PARA;
            }
            if iSliceCount <= 1 {
                return ENC_RETURN_UNEXPECTED;
            }
            //note: the old codes are removed at commit: 3e0ee69
            (*pLayerBsInfo).pBsBuf = ctx_frame_bs_cur(pCtx);
            (*pLayerBsInfo).uiLayerType = VIDEO_CODING_LAYER;
            (*pLayerBsInfo).uiSpatialId = (*pCtx).uiDependencyId;
            (*pLayerBsInfo).uiTemporalId = (*pCtx).uiTemporalId;
            (*pLayerBsInfo).uiQualityId = 0;
            (*pLayerBsInfo).iNalCount = 0;
            (*pLayerBsInfo).eFrameType = eFrameType;
            (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);

            // **T7.B1 — the fork/join.** This was
            // `pTaskManage->ExecuteTasks(WELS_ENC_TASK_ENCODING)`: `iSliceCount`
            // heap tasks pushed through the shared pool, each claiming a bs slot
            // under a mutex, joined by a `Mutex<i32>` + `Condvar` barrier. It is now
            // `std::thread::scope` over one job per bs slot; the join is the
            // barrier and the slot claim is the partition. `FinishTask` ORed each
            // task's result into `iEncoderError` under `mutexEncoderError`; the
            // results come back through the join instead and are ORed here, in the
            // same field, one line above the same check.
            (*pCtx).iEncoderError |=
                crate::encoder::slice_multi_threading::EncodeFixedSlicesForked(pCtx, iSliceCount);
            if (*pCtx).iEncoderError != 0 {
                return (*pCtx).iEncoderError;
            }

            iLayerSize = crate::encoder::slice_multi_threading::AppendSliceToFrameBs(
                pCtx,
                pLayerBsInfo,
                iSliceCount,
            );
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }
        } else if (*pParam).sSliceArgument.uiSliceMode == SM_SIZELIMITED_SLICE
            && (*pSvcParam).iMultipleThreadIdc > 1
        {
            // THREAD_FULLY_FIRE_MODE && SM_SIZELIMITED_SLICE
            let kiPartitionCnt = (*pCtx).iActiveThreadsNum as i32;

            //TODO: use a function to remove duplicate code here and ln3994
            let iLayerBsIdx = (*(*pCtx).pOut).iLayerBsIndex;
            // **T9.E6, the mid-row probe's verdict once round 5 stopped aborting
            // first**: this was `&mut (*pFbi).sLayerInfo[..] as *mut` — a `&mut`
            // element borrow whose Unique retag popped `pLayerBsInfo` (the raw
            // over the whole array, minted at the top of this function) for the
            // element's bytes, and `SliceLayerInfoUpdate` writes through
            // `pLayerBsInfo` right below. S29's spelling reuses the parent's
            // provenance and pops nothing — F70's rule, F114a's shape.
            let pLbi = std::ptr::addr_of_mut!((*pFbi).sLayerInfo[iLayerBsIdx as usize]);
            (*pLbi).pBsBuf = ctx_frame_bs_cur(pCtx);
            (*pLbi).uiLayerType = VIDEO_CODING_LAYER;
            (*pLbi).uiSpatialId = (*pCtx).uiDependencyId;
            (*pLbi).uiTemporalId = (*pCtx).uiTemporalId;
            (*pLbi).uiQualityId = 0;
            (*pLbi).eFrameType = eFrameType;
            (*pLbi).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            (*pLbi).iNalCount = 0;

            // A loop stamping `pFrameBsInfo` and `iSliceIndex` into
            // `pSliceThreading->pThreadPEncCtx[iIdx]` stood here. Nothing has ever
            // read either field — see the note where the struct was (T7.B4).

            let mut iRet = crate::encoder::svc_encode_slice::InitAllSlicesInThread(pCtx);
            if iRet != 0 {
                return ENC_RETURN_UNEXPECTED;
            }
            // **T7.B2 — the dynamic path onto the same seam.** Was
            // `pTaskManage->ExecuteTasks(WELS_ENC_TASK_ENCODING)` over
            // `iActiveThreadsNum` `CWelsConstrainedSizeSlicingEncodingTask`s. The
            // claiming here was never a queue — partition is a static modulo of the
            // task index and the slice indices are an arithmetic progression, with
            // `ReOrderSliceInLayer` recovering the layer position from the stamped
            // index alone — so a static partition reproduces the order exactly and
            // removes the one thing that was schedule-dependent (which bank a
            // partition borrowed). See `EncodeSizeLimitedSlicesForked`.
            (*pCtx).iEncoderError |=
                crate::encoder::slice_multi_threading::EncodeSizeLimitedSlicesForked(
                    pCtx,
                    kiPartitionCnt,
                );

            if (*pCtx).iEncoderError != 0 {
                return (*pCtx).iEncoderError;
            }

            iRet = crate::encoder::svc_encode_slice::SliceLayerInfoUpdate(
                pCtx,
                pFbi,
                pLayerBsInfo,
                (*pParam).sSliceArgument.uiSliceMode,
            );
            if iRet != 0 {
                return ENC_RETURN_UNEXPECTED;
            }

            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(&mut *current_layer(pCtx));
            iLayerSize = crate::encoder::slice_multi_threading::AppendSliceToFrameBs(
                pCtx,
                pLayerBsInfo,
                iSliceCount,
            );
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }
        } else {
            // non-dynamic-slicing, single-threaded multi-slice
            let bNeedPrefix = (*pCtx).bNeedPrefixNalFlag;
            let mut iSliceIdx = 0i32;

            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(&mut *current_layer(pCtx));
            while iSliceIdx < iSliceCount {
                let mut iPayloadSize = 0i32;

                if bNeedPrefix {
                    (*pCtx).iEncoderError = AddPrefixNal(
                        pCtx,
                        pLayerBsInfo,
                        (*pLayerBsInfo).pNalLengthInByte,
                        &mut iNalIdxInLayer,
                        eNalType,
                        eNalRefIdc,
                        &mut iPayloadSize,
                    );
                    if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                        return (*pCtx).iEncoderError;
                    }
                    iLayerSize += iPayloadSize;
                }

                crate::encoder::nal_encap::WelsLoadNal(
                    (*pCtx).pOut,
                    eNalType as i32,
                    eNalRefIdc as i32,
                );

                let pCurSlice = crate::encoder::svc_encode_slice::slice_in_bank(current_layer(pCtx), 0, iSliceIdx);
                debug_assert_eq!(iSliceIdx, (*pCurSlice).iSliceIdx);
                (*pCtx).iEncoderError = crate::encoder::svc_encode_slice::SetSliceBoundaryInfo(
                    current_layer(pCtx),
                    &mut *pCurSlice,
                    iSliceIdx,
                );

                // T7.C3, as above.
                crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);
                (*pCtx).iEncoderError = crate::encoder::svc_encode_slice::WelsCodeOneSlice(
                    pCtx,
                    &mut *pCurSlice,
                    eNalType as i32,
                );
                if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                    return (*pCtx).iEncoderError;
                }

                crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

                (*pCtx).iEncoderError = crate::encoder::nal_encap::WelsEncodeNal(
                    &(&*(*pCtx).pOut).sNalList[(*(*pCtx).pOut).iNalIndex as usize - 1],
                    &(&*(*pCtx).pOut).sBsBuffer[..],
                    Some(&(*current_layer(pCtx)).sLayerInfo.sNalHeaderExt),
                    ctx_frame_bs_cur(pCtx),
                    (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
                    &mut *(*pLayerBsInfo).pNalLengthInByte.add(iNalIdxInLayer as usize),
                );
                if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                    return (*pCtx).iEncoderError;
                }
                let iSliceSize = *(*pLayerBsInfo).pNalLengthInByte.add(iNalIdxInLayer as usize);

                (*pCtx).iPosBsBuffer += iSliceSize;
                iLayerSize += iSliceSize;

                iNalIdxInLayer += 1;
                iSliceIdx += 1;
            }

            (*pLayerBsInfo).uiLayerType = VIDEO_CODING_LAYER;
            (*pLayerBsInfo).uiSpatialId = iCurDid as u8;
            (*pLayerBsInfo).uiTemporalId = iCurTid as u8;
            (*pLayerBsInfo).uiQualityId = 0;
            (*pLayerBsInfo).iNalCount = iNalIdxInLayer;
            (*pLayerBsInfo).eFrameType = eFrameType;
            (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
        }

        // `None` here meant "never take this path", which is what the method's
        // empty arms return: `false`.
        if (*ctx_func_list(pCtx))
            .pfRc
            .WelsRcPostFrameSkipping(pCtx, iCurDid as i32, (*pFbi).uiTimeStamp)
        {
            StackBackEncoderStatus(pCtx, eFrameType);
            ClearFrameBsInfo(pCtx, pFbi);

            iFrameSize = 0;
            iLayerNum = 0;

            (*ctx_func_list(pCtx))
                .pfRc
                .WelsUpdateBufferWhenSkip(pCtx, iSpatialNum);

            crate::encoder::rc::WelsRcPostFrameSkippedUpdate(pCtx, iCurDid as i32);
            (*pCtx).iEncoderError = ENC_RETURN_SUCCESS;
            let _ = iLayerNum;
            return ENC_RETURN_SUCCESS;
        }

        // deblocking filter. ENABLE_FRAME_DUMP is not defined, so the temporal-id
        // clause is compiled in.
        if !(*current_layer(pCtx)).bDeblockingParallelFlag
            && eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST
            && ((*pParamInternal).iHighestTemporalId == 0
                || iCurTid < (*pParamInternal).iHighestTemporalId as i32)
        {
            crate::encoder::deblocking::PerformDeblockingFilter(pCtx);
        }

        (*ctx_func_list(pCtx))
            .pfRc
            .WelsRcPictureInfoUpdate(pCtx, iLayerSize);
        iFrameSize += iLayerSize;
        crate::encoder::rc::RcTraceFrameBits(pCtx, (*pFbi).uiTimeStamp, iFrameSize);
        if let Some(id) = (*pCtx).pDecPic {
            (*ctx_ref_list(pCtx, iCurDid as usize))
                .pic_mut(id)
                .iFrameAverageQp = (*ctx_rc_at(pCtx, iCurDid as usize)).iAverageFrameQp;
        }

        // update scc related
        if let Some(f) = (*ctx_func_list(pCtx)).pfUpdateFMESwitch {
            f(current_layer(pCtx));
        }

        // reference picture list update
        if eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST
            && !eRefStrategy.UpdateRefList(pCtx)
        {
            // set the next frame to be IDR
            (*pCtx).iEncoderError = ENC_RETURN_CORRECTED;
            break;
        }

        // MinCr check is a diagnostic log in C++ with no state change; omitted.

        // encoder_ext.cpp:3927-3980. Note the asymmetry, which is the reference's
        // and not a transcription slip: each plane is *computed* when either
        // `pSvcParam->bPsnrX` or `pSrcPic->bPsnrX` is set, but only *reported*
        // when `pSrcPic->bPsnrX` is set. Asking through SEncParamExt alone
        // therefore costs the full-frame scan and reports nothing.
        let mut fSnrY: f32 = 0.0;
        let mut fSnrU: f32 = 0.0;
        let mut fSnrV: f32 = 0.0;
        // `sFsnr = fsnr.unwrap_or(pEncPic)` stood here and never fired: every use of
        // it is under `fsnr.is_some()`, so the fallback was the source picture
        // measured against itself and unreachable. The `is_some()` is the `if let`.
        //
        // **T9.B3, the plane family's first conversion on this side.** The C++
        // (`encoder_ext.cpp:3927-3980`) hands `WelsCalcPsnr` two `uint8_t*` plane
        // origins and two strides, and the port carried them as two `PicPlanes`
        // copied out of the pictures six hundred lines above. Both pictures are
        // named by a handle, so each plane is resolved here, read, and dropped —
        // nothing derived from a picture crosses the encode. The two live in
        // **different owners** (this layer's reference list and the preprocessor's
        // spatial pool), so the two shared borrows never name one allocation, and
        // both are read-only: this runs after the fork has joined and after
        // `UpdateRefList`. `-1.0` is the raw form's null-plane sentinel, answered
        // where the handle is rather than inside the kernel.
        if let Some(idDecPic) = fsnr {
            let pRefListPsnr = crate::encoder::encoder_context::ctx_ref_list(pCtx, iCurDid as usize);
            if !pRefListPsnr.is_null() && !(*pCtx).pVpp.is_null() {
                let recon = &*pRefListPsnr;
                let vpp = &*(*pCtx).pVpp;
                let plane_psnr = |i: usize, w: i32, h: i32| -> f32 {
                    let tar = recon.pic(idDecPic).plane(i);
                    let src = vpp.src_id(idEncPic).plane(i);
                    if tar.is_empty() || src.is_empty() {
                        -1.0
                    } else {
                        crate::common::wels_common_defs::calc_psnr(
                            &tar.cursor(0, 0),
                            &src.cursor(0, 0),
                            w,
                            h,
                        )
                    }
                };
                if (*pSvcParam).bPsnrY || (*pSrcPic).bPsnrY {
                    fSnrY = plane_psnr(0, iCurWidth, iCurHeight);
                }
                if (*pSvcParam).bPsnrU || (*pSrcPic).bPsnrU {
                    fSnrU = plane_psnr(1, iCurWidth >> 1, iCurHeight >> 1);
                }
                if (*pSvcParam).bPsnrV || (*pSrcPic).bPsnrV {
                    fSnrV = plane_psnr(2, iCurWidth >> 1, iCurHeight >> 1);
                }
            }
        }

        (*pLayerBsInfo).rPsnr[0] = 0.0;
        (*pLayerBsInfo).rPsnr[1] = 0.0;
        (*pLayerBsInfo).rPsnr[2] = 0.0;
        if (*pSrcPic).bPsnrY {
            (*pLayerBsInfo).rPsnr[0] = fSnrY;
        }
        if (*pSrcPic).bPsnrU {
            (*pLayerBsInfo).rPsnr[1] = fSnrU;
        }
        if (*pSrcPic).bPsnrV {
            (*pLayerBsInfo).rPsnr[2] = fSnrV;
        }

        iCountNal = (*pLayerBsInfo).iNalCount;
        iLayerNum += 1;
        let pPrev = pLayerBsInfo;
        pLayerBsInfo = pLayerBsInfo.add(1);
        (*(*pCtx).pOut).iLayerBsIndex += 1;
        (*pLayerBsInfo).pBsBuf = ctx_frame_bs_cur(pCtx);
        (*pLayerBsInfo).pNalLengthInByte =
            (*pPrev).pNalLengthInByte.add(iCountNal as usize);

        if (*pSvcParam).iPaddingFlag != 0
            && (*ctx_rc_at(pCtx, (*pCtx).uiDependencyId as usize)).iPaddingSize > 0
        {
            let mut iPaddingNalSize = 0i32;
            let iPaddingSize =
                (*ctx_rc_at(pCtx, (*pCtx).uiDependencyId as usize)).iPaddingSize;
            (*pCtx).iEncoderError = WritePadding(pCtx, iPaddingSize, &mut iPaddingNalSize);
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }

            if iPaddingNalSize <= 0 {
                return ENC_RETURN_UNEXPECTED;
            }

            let pRc = ctx_rc_at(pCtx, (*pCtx).uiDependencyId as usize);
            (*pRc).iPaddingBitrateStat += (*pRc).iPaddingSize;
            (*pRc).iPaddingSize = 0;

            (*pLayerBsInfo).uiSpatialId = 0;
            (*pLayerBsInfo).uiTemporalId = 0;
            (*pLayerBsInfo).uiQualityId = 0;
            (*pLayerBsInfo).uiLayerType = NON_VIDEO_CODING_LAYER;
            (*pLayerBsInfo).iNalCount = 1;
            *(*pLayerBsInfo).pNalLengthInByte = iPaddingNalSize;
            (*pLayerBsInfo).eFrameType = eFrameType;
            (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            let pPrev2 = pLayerBsInfo;
            pLayerBsInfo = pLayerBsInfo.add(1);
            (*(*pCtx).pOut).iLayerBsIndex += 1;
            (*pLayerBsInfo).pBsBuf = ctx_frame_bs_cur(pCtx);
            (*pLayerBsInfo).pNalLengthInByte = (*pPrev2).pNalLengthInByte.add(1);
            iLayerNum += 1;

            iFrameSize += iPaddingNalSize;
        }

        // **F72 completed — T7.C1, decision D-mt-2 (plan §7.4).** The producer half
        // of the load-balancing loop, at the C++'s own site: `encoder_ext.cpp:4064-4073`,
        // end of the per-layer body, after the padding block and immediately above the
        // `eLastNalPriority` stamp — and under the C++'s own four-term guard, which is
        // the same one the consumer arm above already reproduces. The workers stamped
        // `uiSliceConsumeTime` on their way through `EncodeOneSliceInJob`
        // (`bRecordsTime`, which is `bUseLoadBalancing`); this turns those times into
        // the `iSliceComplexRatio` that next frame's `DynamicAdjustSlicing` reads. It
        // was never called in this port, so every ratio was permanently zero and the
        // balance was degenerate rather than absent — nothing crashed and nothing
        // warned, which is what made it worth a finding rather than a bug report.
        //
        // The `MT_DEBUG`-only `TrackSliceComplexities` that follows it in the C++ has
        // no counterpart here and needs none: `MT_DEBUG` is off in every build either
        // project makes.
        if (*pParam).sSliceArgument.uiSliceMode == SliceModeEnum::SM_FIXEDSLCNUM_SLICE
            && (*pSvcParam).bUseLoadBalancing
            && (*pSvcParam).iMultipleThreadIdc > 1
            && (*pSvcParam).iMultipleThreadIdc >= (*pParam).sSliceArgument.uiSliceNum as u16
        {
            crate::encoder::slice_multi_threading::CalcSliceComplexRatio(&mut *current_layer(pCtx));
        }

        (*pCtx).eLastNalPriority[iCurDid as usize] = eNalRefIdc;
        iSpatialIdx += 1;

        if (iCurDid as i32) + 1 < (*pSvcParam).iSpatialLayerNum {
            // iSpatialIdx has already been incremented, so this points at the next layer.
            // Hoisted: `WelsSwapDqLayers` takes the ctx retag and this argument reads
            // through the same ctx (shape B).
            let iNextDid = (*pCtx).sSpatialIndexMap[iSpatialIdx as usize].iDid;
            WelsSwapDqLayers(pCtx, iNextDid);
        }

        if (*(*pCtx).pVpp).UpdateSpatialPictures(pCtx, pSvcParam, iCurTid as i8, iCurDid as i32) != 0 {
            crate::encoder::wels_encoder_ext::ForceCodingIDR(pCtx, iCurDid as i32);
            // the above sets the next frame to IDR
            (*pFbi).eFrameType = eFrameType;
            (*pLayerBsInfo).eFrameType = eFrameType;
            return ENC_RETURN_CORRECTED;
        }

        let uiDidForLtr = (*pCtx).uiDependencyId as usize;
        let pLtr = ctx_ltr_at(pCtx, uiDidForLtr);
        if (*pSvcParam).bEnableLongTermReference
            && (((*pLtr).bLTRMarkingFlag
                && (*pLtr).iLTRMarkMode == crate::encoder::ref_list_mgr_svc::LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32)
                || eFrameType == EVideoFrameType::videoFrameTypeIDR)
        {
            (*pCtx).bRefOfCurTidIsLtr[iCurDid as usize][iCurTid as usize] = true;
        }
        if (*pSvcParam).bSimulcastAVC {
            (*pParamInternal).iCodingIndex += 1;
        }
    } // end of (iSpatialIdx/iSpatialNum)

    if !(*pSvcParam).bSimulcastAVC {
        for i in 0..(*pSvcParam).iSpatialLayerNum as usize {
            (*pSvcParam).sDependencyLayers[i].iCodingIndex += 1;
        }
    }

    if ENC_RETURN_CORRECTED == (*pCtx).iEncoderError {
        // **`iSpatialIdx == iSpatialNum` here** — the loop above ran to completion —
        // so with 4 spatial layers configured this reads **one past the end** of a
        // `[SSpatialPicIndex; 4]`. Upstream does the identical thing at
        // `encoder_ext.cpp:4109-4110` (`(pSpatialIndexMap + iSpatialIdx)->iDid`,
        // twice), so the port reproduces it rather than fixing it: an index would
        // panic where this reads, and a panic is not byte-identical. F162.
        // Spelled through a derivation that lives and dies inside this statement, so
        // nothing is held across the two calls below — which is the whole hazard.
        let iDid = (*(*pCtx).sSpatialIndexMap.as_ptr().add(iSpatialIdx as usize)).iDid;
        (*(*pCtx).pVpp).UpdateSpatialPictures(pCtx, pSvcParam, iCurTid as i8, iDid);
        crate::encoder::wels_encoder_ext::ForceCodingIDR(pCtx, iDid);
        // the above sets the next frame to IDR
        (*pFbi).eFrameType = eFrameType;
        (*pLayerBsInfo).eFrameType = eFrameType;
        return ENC_RETURN_CORRECTED;
    }

    // check number of layers / nals / slices dependencies
    if iLayerNum > MAX_LAYER_NUM_OF_FRAME {
        return 1;
    }

    (*pFbi).iLayerNum = iLayerNum;

    crate::encoder::slice_multi_threading::WelsEmms();

    (*pLayerBsInfo).eFrameType = eFrameType;
    (*pFbi).iFrameSizeInBytes = iFrameSize;
    (*pFbi).eFrameType = eFrameType;
    for k in 0..(*pFbi).iLayerNum as usize {
        if (*pFbi).eFrameType != (*pFbi).sLayerInfo[k].eFrameType {
            (*pFbi).eFrameType = EVideoFrameType::videoFrameTypeIPMixed;
        }
    }

    ENC_RETURN_SUCCESS
}
