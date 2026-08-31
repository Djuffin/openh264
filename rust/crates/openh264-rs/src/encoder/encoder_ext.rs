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
    ctx_dq_idc_map, ctx_dq_layer, ctx_ltr_at, ctx_mb_index_x,
    ctx_paraset_arrays,
    ctx_mb_index_y, ctx_param_raw,
    ctx_stride_enc_block_offset,
    sWelsEncCtx, SDqIdc, SLogContext, SRefList, SStrideTables, SSubsetSps, SWelsPPS,
    SWelsSPS, BASE_DEPENDENCY_ID,
};
use crate::encoder::md::INTRA_4x4_MODE_NUM;
use crate::encoder::param_svc::{
    SExistingParasetList, SWelsSvcCodingParam, MB_WIDTH_LUMA, UNSPECIFIED_BIT_RATE,
};
use crate::encoder::param_svc::{PpsId, SpsId, SubsetSpsId};
use crate::encoder::svc_encode_slice::current_layer_mut;
use crate::encoder::svc_encode_slice::LayerSps;
use crate::encoder::paraset_strategy::{ParasetStrategy, PARA_SET_TYPE_AVCSPS, PARA_SET_TYPE_PPS};
use crate::api::codec_api::EParameterSetStrategy;
use crate::encoder::picture::SPicture;
use crate::encoder::slice_multi_threading::{
    MAX_DEPENDENCY_LAYER, MAX_SLICES_NUM, MAX_THREADS_NUM,
};
use crate::encoder::svc_enc_slice_segment::{GetInitialSliceNum, InitSlicePEncCtx};
use crate::encoder::svc_encode_slice::{InitSliceInLayer, WelsMbToSliceIdc, current_layer_ref};
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

// **The `tag!` macro stood here and is deleted** (session J, step 3): it minted a
// NUL-terminated allocation label for `CMemoryAlign`, which Phases 3-6 retired
// with the arena. `wels_preprocess.rs:63` records the same deletion for its own
// copy; this one survived because the crate root allowed `dead_code`, which is
// exactly what dropping that allow was for.

/// `WelsGetEncBlockStrideOffset` — `decode_mb_aux.cpp:235`.
///
/// **S11.37: safe — the parameter states the extent the `# Safety` line
/// promised.** "At least 24 writable `i32`s" is `&mut [i32; 24]`; the two
/// callers (the stride-table fillers) make that claim once, where they derive
/// the block from the arena, instead of this body assuming it 24 times.
pub fn WelsGetEncBlockStrideOffset(pBlock: &mut [i32; 24], kiStrideY: i32, kiStrideUV: i32) {
    for j in 0..4i32 {
        let i = (j << 2) as usize;
        let k = ((j & 0x01) << 1) as i32;
        let r = j & 0x02;
        pBlock[i] = (k + r * kiStrideY) << 2;
        pBlock[i + 1] = (1 + k + r * kiStrideY) << 2;
        pBlock[i + 2] = (k + (1 + r) * kiStrideY) << 2;
        pBlock[i + 3] = (1 + k + (1 + r) * kiStrideY) << 2;

        let v = ((j & 0x01) + r * kiStrideUV) << 2;
        pBlock[16 + j as usize] = v;
        pBlock[20 + j as usize] = v;
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
pub fn AcquireLayersNals(
    ctx: &mut sWelsEncCtx,
    pCountLayers: &mut i32,
    pCountNals: &mut i32,
) -> i32 {
    // A7: the `pParam` argument is gone — see `InitFunctionPointers`; the caller
    // held it as a `&mut` across this call, which Miri refused.
    let mut iCountNumLayers: i32 = 0;
    let mut iCountNumNals: i32 = 0;
    let mut iDIndex: i32 = 0;

    let iNumDependencyLayers = ctx.param().iSpatialLayerNum;

    loop {
        // S29: `&mut X as *mut T` is the defect with the cast already written.
        // The callee takes `*mut`, so the reference existed only to be discarded.
        // S11.37: three shared reads — the cursor is gone (F284's test: the
        // result never stayed a pointer).
        let kSliceArgument = &ctx.param().sSpatialLayers[iDIndex as usize].sSliceArgument;
        let iOrgNumNals = iCountNumNals;

        // Note (Sep. 2010, upstream): the memory over-use here counts little towards
        // overall performance and should not be critical even on mobile.
        if SM_SIZELIMITED_SLICE == kSliceArgument.uiSliceMode {
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
            let kiNumOfSlice = GetInitialSliceNum(kSliceArgument);

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
    let Some(pStrategy) = ctx.func_list_mut().pParametersetStrategy.as_mut() else {
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

    // S11.14: the null guards retire with the raws (T9.H).
    *pCountLayers = iCountNumLayers;
    *pCountNals = iCountNumNals;
    0
}

/// `AllocStrideTables` — encoder_ext.cpp:1224.
///
/// # Safety
/// `ppCtx` must point to a live context with `pMemAlign` and `pSvcParam` set.
pub fn AllocStrideTables(ctx: &mut sWelsEncCtx, kiNumSpatialLayers: i32) -> i32 {
    // A7: the binding is gone for the reason `RequestMemorySvc` records — a `&mut`
    // into the parameter block cannot be held across a call that reaches it again.

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
    let iCntTid = if ctx.param().iTemporalLayerNum > 1 { 2 } else { 1 };

    iSpatialIdx = 0;
    while iSpatialIdx < kiNumSpatialLayers {
        let kiTmpWidth = (ctx.param().sSpatialLayers[iSpatialIdx as usize].iVideoWidth + 15) >> 4;
        let kiTmpHeight = (ctx.param().sSpatialLayers[iSpatialIdx as usize].iVideoHeight + 15) >> 4;
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
            let fDlp = &ctx.param().sSpatialLayers[iSpatialIdx as usize];

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

    ctx.pStrideTab = Some(Box::new(SStrideTables::new(iNeedAllocSize)));
    let pPtr: &mut SStrideTables = ctx.pStrideTab.as_mut().unwrap();

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
            // S11.38: the bounded write accessor — the 24-entry claim lives on
            // `SStrideTables`, beside its read twin.
            WelsGetEncBlockStrideOffset(
                pPtr.i32_block24_mut(pBaseDec),
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
    // initialize the scratch row: 0, 1, 2, ... — S11.38, the cursor is an index.
    for (idx, v) in sTmpRow.iter_mut().take(iMaxMbWidth as usize).enumerate() {
        *v = idx as i16;
    }

    iSpatialIdx = kiNumSpatialLayers;
    loop {
        iSpatialIdx -= 1;
        if iSpatialIdx < 0 {
            break;
        }
        let kiMbWidth = sMbSizeMap[iSpatialIdx as usize].iMbWidth;
        let kiMbHeight = sMbSizeMap[iSpatialIdx as usize].iCountMbNum / kiMbWidth;
        // S11.38: the bounded region, row-copied — the raw cursor advanced by
        // the same row width over the same extent.
        if let Some(off) = pPtr.pMbIndexX[iSpatialIdx as usize] {
            let kpRegion =
                pPtr.i16_region_mut(off, (kiMbWidth * kiMbHeight) as usize);
            for row in kpRegion.chunks_exact_mut(kiMbWidth as usize) {
                row.copy_from_slice(&sTmpRow[..kiMbWidth as usize]);
            }
        }
    }

    sTmpRow.fill(0);
    let iMaxMbHeight = sMbSizeMap[(kiNumSpatialLayers - 1) as usize].iCountMbNum
        / sMbSizeMap[(kiNumSpatialLayers - 1) as usize].iMbWidth;
    i = 0;
    loop {
        iSpatialIdx = kiNumSpatialLayers - 1;
        while iSpatialIdx >= 0 {
            let kiMbWidth = sMbSizeMap[iSpatialIdx as usize].iMbWidth;
            let kiMbHeight = sMbSizeMap[iSpatialIdx as usize].iCountMbNum / kiMbWidth;
            // S11.38: the bounded region again; the row copy targets row `i`
            // of the same extent the raw cursor named.
            if i < kiMbHeight {
                if let Some(off) = pPtr.pMbIndexY[iSpatialIdx as usize] {
                    let kpRegion =
                        pPtr.i16_region_mut(off, (kiMbWidth * kiMbHeight) as usize);
                    kpRegion[(i * kiMbWidth) as usize..][..kiMbWidth as usize]
                        .copy_from_slice(&sTmpRow[..kiMbWidth as usize]);
                }
            }
            iSpatialIdx -= 1;
        }
        i += 1;
        if i >= iMaxMbHeight {
            break;
        }

        // The scratch becomes a row of the value `i` — the C++ builds it four
        // halfwords at a time via two 32-bit stores; a fill is the same bytes.
        sTmpRow[..iMaxMbWidth as usize].fill(i as i16);
    }

    drop(sTmpRow);

    0
}

/// `GetMvMvdRange` — encoder_ext.cpp:1508.
///
pub fn GetMvMvdRange(
    pParam: &SWelsSvcCodingParam,
    iMvRange: &mut i32,
    iMvdRange: &mut i32,
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
/// **`&sWelsEncCtx` since T9.H2, and S67's audit is what said so.** This body
/// reaches the context for exactly two things — `ctx_mb_index_x` and
/// `ctx_mb_index_y`, both pure lookups into the stride arena — so the `&mut` it took
/// was asserting an exclusivity it never used, and its call site was one of the
/// twenty-three out-of-family whole-context retags the audit enumerated. It is
/// twenty-two now, and the layer's `&mut` beside it no longer coexists with a
/// context `&mut` at all.
///
/// S11.2d: the context parameter is gone. This body read exactly two things
/// through it — the layer's macroblock X/Y coordinate tables — so it takes
/// those as slices (`SStrideTables::MbIndexXY`) instead of the whole context.
/// That is S10.3c's borrow-*width* rule again, and it is what lets the caller
/// hold the layer `&mut` and the tables `&` at once: they are two disjoint
/// fields of the context, not the context twice.
fn InitMbInfo(
    kpMbIndexX: &[i16],
    kpMbIndexY: &[i16],
    pLayer: &mut SDqLayer,
) {
    let iMbWidth = pLayer.iMbWidth as i32;
    let iMbHeight = pLayer.iMbHeight as i32;
    let iMbNum = iMbWidth * iMbHeight;
    // **S10.3e: no `mb_window` here either.** Same shape as
    // `DynslcUpdateMbNeighbourInfoListForAllSlices` (S10.3c): the layer is `&mut`,
    // so there is no fork and no exclusivity claim to make; what needed the raw
    // was the *width* of `WelsMbToSliceIdc`'s old whole-layer parameter, and that
    // narrowed in S10.3c. The grid and the slice context are two fields.
    //
    // S11.2d: the coordinate tables arrive as slices, so the arena seam
    // (S10.3e's third) no longer reaches this body at all.
    let SDqLayer { sMbDataP, sSliceEncCtx, .. } = pLayer;
    let dims = sMbDataP.dims();
    let mut mbs = crate::safe::mb_grid::MbWindow::new(
        sMbDataP.as_mut_slice(),
        0,
        dims.mb_width(),
        0,
    );

    for iIdx in 0..iMbNum as usize {
        let pMb = mbs.at_mut(iIdx);

        pMb.iMbX = kpMbIndexX[iIdx];
        pMb.iMbY = kpMbIndexY[iIdx];
        pMb.iMbXY = iIdx as i32;

        // [0..65535] > 36864 of LEVEL5.2
        let uiSliceIdc: u16 = WelsMbToSliceIdc(Some(sSliceEncCtx), iIdx as i32);
        let iLeftXY = iIdx as i32 - 1;
        let iTopXY = iIdx as i32 - iMbWidth;
        let iLeftTopXY = iTopXY - 1;
        let iRightTopXY = iTopXY + 1;

        let bLeft = pMb.iMbX > 0 && uiSliceIdc == WelsMbToSliceIdc(Some(sSliceEncCtx), iLeftXY);
        let bTop = pMb.iMbY > 0 && uiSliceIdc == WelsMbToSliceIdc(Some(sSliceEncCtx), iTopXY);
        let bLeftTop =
            pMb.iMbX > 0 && pMb.iMbY > 0 && uiSliceIdc == WelsMbToSliceIdc(Some(sSliceEncCtx), iLeftTopXY);
        let bRightTop = (pMb.iMbX as i32) < (iMbWidth - 1)
            && pMb.iMbY > 0
            && uiSliceIdc == WelsMbToSliceIdc(Some(sSliceEncCtx), iRightTopXY);

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
pub fn InitMbListD(ctx: &mut sWelsEncCtx) -> i32 {
    let iNumDlayer = ctx.param().iSpatialLayerNum;

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
        let iMbWidth = (ctx.param().sSpatialLayers[i].iVideoWidth + 15) >> 4;
        let iMbHeight = (ctx.param().sSpatialLayers[i].iVideoHeight + 15) >> 4;
        // S11.2d: the context splits into its two disjoint owners — the stride
        // tables (shared) and this layer (mutable) — so both borrows are live at
        // once and neither is the whole context. §4.6's destructuring shape.
        let sWelsEncCtx { pStrideTab, ppDqLayerList, .. } = &mut *ctx;
        let Some(pLayer) = ppDqLayerList.get_mut(i).and_then(|l| l.as_deref_mut()) else {
            return 1;
        };
        pLayer.sMbDataP = MbArray::new(
            MbDims::new(iMbWidth as usize, iMbHeight as usize),
            SMB::default(),
        );
        let Some((kpMbIndexX, kpMbIndexY)) = pStrideTab
            .as_ref()
            .and_then(|tab| tab.MbIndexXY(i, (iMbWidth * iMbHeight) as usize))
        else {
            return 1;
        };
        InitMbInfo(kpMbIndexX, kpMbIndexY, pLayer);
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
    ctx: &mut sWelsEncCtx,
    pExistingParasetList: Option<&SExistingParasetList>,
) -> i32 {
    let mut pSps: *mut crate::encoder::param_svc::SWelsSPS = null_mut();
    let mut pSubsetSps: *mut crate::encoder::param_svc::SSubsetSps = null_mut();
    let mut iSpsId: i32 = 0;
    let mut iPpsId: u32 = 0;
    let mut iResult: i32;

    // A7: the binding is gone for the reason `RequestMemorySvc` records — a `&mut`
    // into the parameter block cannot be held across a call that reaches it again.
    let iDlayerCount = ctx.param().iSpatialLayerNum;
    let iNumRef = ctx.param().iMaxNumRefFrame as u32;

    // FME_DEFAULT_FEATURE_INDEX / ME_DIA_CROSS / ME_DIA_CROSS_FME, screen content only
    let kiFeatureStrategyIndex: i32 = FME_DEFAULT_FEATURE_INDEX as i32;
    let kiMe16x16: i32 = ME_DIA_CROSS as i32;
    let kiMe8x8: i32 = ME_DIA_CROSS_FME as i32;
    let kiNeedFeatureStorage = if ctx.param().iUsageType != SCREEN_CONTENT_REAL_TIME {
        0
    } else {
        (kiFeatureStrategyIndex << 16) + ((kiMe16x16 & 0x00FF) << 8) + (kiMe8x8 & 0x00FF)
    };

    let mut iDlayerIndex: i32 = 0;
    while iDlayerIndex < iDlayerCount {
        let mut i: u32 = 0;
        let kiWidth = ctx.param().sSpatialLayers[iDlayerIndex as usize].iVideoWidth;
        let kiHeight = ctx.param().sSpatialLayers[iDlayerIndex as usize].iVideoHeight;
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
            let tab = ctx.pStrideTab.as_mut().expect("pStrideTab allocated");
            match tab.pStrideEncBlockOffset[iDlayerIndex as usize] {
                Some(off) => tab.root().add(off as usize).cast::<i32>(),
                None => std::ptr::null_mut(),
            }
        };
        // S11.37: as at the dec-side fill — the 24-entry claim at the derivation.
        WelsGetEncBlockStrideOffset(&mut *(pEncBlockOffset.cast::<[i32; 24]>()), iPicWidth, iPicChromaWidth);

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
        // A write into the context's own `Vec` header, not into the list's
        // allocation. (S3.B2: the place is behind a `&mut` now, so the explicit
        // autoref is just spelling — borrowck scopes it to this statement.)
        (&mut ctx.ppRefPicListExt)[iDlayerIndex as usize] = Some(pRefListBox);
        iDlayerIndex += 1;
    }

    iDlayerIndex = 0;
    while iDlayerIndex < iDlayerCount {
        // S29's named shape — `&mut X as *mut T` is the defect with the cast already
        // written: the reference retags before the cast discards it, and the tag is
        // what `InitSliceInLayer` used to pop. `addr_of_mut!` derives from the raw
        // parent and creates no reference at all.
        let pDlayer = std::ptr::addr_of_mut!((*ctx_param_raw(&*ctx)).sSpatialLayers[iDlayerIndex as usize]);
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(&*ctx)).sDependencyLayers[iDlayerIndex as usize]);
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
        (&mut ctx.ppDqLayerList)[iDlayerIndex as usize] = Some(pDqLayerBox);
        let pDqLayer = ctx_dq_layer(ctx, iDlayerIndex as usize);

        (*pDqLayer).iMbWidth = kiMbW as i16;
        (*pDqLayer).iMbHeight = kiMbH as i16;

        let mut iMaxSliceNum: i32 = 1;
        let kiSliceNum = GetInitialSliceNum(&(*pDlayer).sSliceArgument);
        if iMaxSliceNum < kiSliceNum {
            iMaxSliceNum = kiSliceNum;
        }
        (*pDqLayer).iMaxSliceNum = iMaxSliceNum;

        // S67 blessed (H2): `pDqLayer` is in the layer's `Box`;
        // `pParam`/`pDlayer`/`pParamInternal` in `pSvcParam`'s.
        iResult = InitSliceInLayer(&mut *ctx, &mut *pDqLayer, iDlayerIndex);
        if iResult != 0 {
            return iResult;
        }

        // deblocking parameters initialization; target-layer deblocking
        (*pDqLayer).iLoopFilterDisableIdc = ctx.param().iLoopFilterDisableIdc as u8;
        (*pDqLayer).iLoopFilterAlphaC0Offset = (ctx.param().iLoopFilterAlphaC0Offset << 1) as i8;
        (*pDqLayer).iLoopFilterBetaOffset = (ctx.param().iLoopFilterBetaOffset << 1) as i8;
        // parallel deblocking
        (*pDqLayer).bDeblockingParallelFlag = ctx.param().bDeblockingParallelFlag;

        // deblocking parameter adjustment
        if SM_SINGLE_SLICE == (*pDlayer).sSliceArgument.uiSliceMode {
            // iLoopFilterDisableIdc will be 0 or 1 under single slice
            if 2 == ctx.param().iLoopFilterDisableIdc {
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
    // `GenerateNewSps`/`InitPps` take `std::ptr::addr_of_mut!(*ctx)`, and reaching the strategy through the
    // context while a `&mut` to it is live would alias. Same reason as
    // `WelsWriteParameterSets`; T4b.2a.
    if ctx.func_list().pParametersetStrategy.is_none() {
        return 1;
    }
    let kiNeededSpsNum = ParasetStrategy(ctx).GetNeededSpsNum() as i32;
    let kiNeededSubsetSpsNum = ParasetStrategy(ctx).GetNeededSubsetSpsNum() as i32;
    // **T6.H2.** Three `WelsMallocz` calls and their three null checks were here.
    // The lengths are the strategy's own numbers, unchanged; the entries are the
    // zeros `WelsMallocz` left, spelled as `ZERO` rather than `Default` because
    // `SWelsSPS::default()` seeds `uiProfileIdc = PRO_BASELINE` and three VUI
    // `*_UNDEF`s, and none of those is what a memset writes (F56: zeros are ruled).
    ctx.pSpsArray = vec![crate::encoder::param_svc::SWelsSPS::ZERO; kiNeededSpsNum as usize];
    // The `else` arm was `pSubsetArray = null_mut()` — no allocation at all when the
    // configuration needs no subset SPS. An empty `Vec` is that, and
    // `subset_array()` answers the same emptiness for it.
    ctx.pSubsetArray = vec![
        crate::encoder::param_svc::SSubsetSps::ZERO;
        kiNeededSubsetSpsNum.max(0) as usize
    ];

    // PPS
    let kiNeededPpsNum = ParasetStrategy(ctx).GetNeededPpsNum() as i32;
    ctx.pPPSArray = vec![crate::encoder::param_svc::SWelsPPS::ZERO; kiNeededPpsNum as usize];

    // **T9.H2 — the ~36's blocker, dissolved.** This supplied the three arrays as
    // three separate raw roots because three `&mut` out of one context, taken through
    // three accessor calls, is what the borrow checker refuses. `ctx_paraset_arrays`
    // answers all three from **one** borrow, which is legal precisely because the
    // compiler can see the three fields are disjoint.
    // **S3.B2.** The receiver is taken *before* the arrays: `ctx_paraset_arrays`
    // holds a `&mut` on the context for as long as the three array borrows live, and
    // deriving the strategy's raw inside that window is a second mutable borrow.
    // **S7.A3**: the hoist and its argument are gone. The note above was right that
    // the strategy lives in the `pFuncList` `Box` and so cannot be popped by the array
    // reborrow — but saying so needed a raw pointer, because borrowck sees two `&mut`
    // claims on `ctx`. One split call says it in the type instead.
    let (pParasetStrategy, pSpsArray, pSubsetArray, pPpsArray) =
        crate::encoder::paraset_strategy::ctx_strategy_and_paraset_arrays(ctx);
    pParasetStrategy.LoadPrevious(
        pExistingParasetList,
        pSpsArray,
        pSubsetArray,
        pPpsArray,
    );

    // **T6.H3.** `SDqIdc` is four bytes of POD and its derived `Default` is the
    // memset image field for field, so `Default` *is* the ruled zero here — unlike
    // `SWelsSPS`'s two lines up.
    ctx.pDqIdcMap = vec![SDqIdc::default(); iDlayerCount as usize];

    iDlayerIndex = 0;
    while iDlayerIndex < iDlayerCount {
        let bUseSubsetSps = !ctx.param().bSimulcastAVC && (iDlayerIndex > BASE_DEPENDENCY_ID as i32);
        // **S11.18: the binding that made S29 a seam is gone.** It was derived
        // here, spanned `GenerateNewSps` — which re-derives the same layer — and
        // was read once, ~85 lines below. That span is the whole of F187's
        // refusal to flip `WelsInitSps`'s layer parameters: a `&mut` retag
        // inside the callee would pop this tag before the read. Derived *at* its
        // one use instead, nothing spans the call and the premise expires.
        let bSvcBaselayer = !ctx.param().bSimulcastAVC
            && (iDlayerCount > BASE_DEPENDENCY_ID as i32)
            && (iDlayerIndex == BASE_DEPENDENCY_ID as i32);

        // S67 blessed (H2): live across it are `pDqIdc`, `pSps`/`pSubsetSps` (Vec buffers) and
        // `pDlayerParam` (`pSvcParam`'s `Box`) — none inside the context's own bytes.
        // **S7.A3**: strategy and the four values the method actually reached for,
        // split off one `&mut` context.
        let (strategy, pParam, pSpsArray, pSubsetArray, pPpsArray) =
            crate::encoder::paraset_strategy::ctx_strategy_and_param_arrays(ctx);
        iSpsId = strategy.GenerateNewSps(
            pParam,
            pSpsArray,
            pSubsetArray,
            pPpsArray,
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
        // **A4 derives these three from the *readers*, deliberately.** They are
        // read-only cursors — `InitPps` takes them as `Option<&_>` and the two
        // `iMb*` reads below are reads — and they must survive the parameter-set
        // calls, which reach the same arrays again. `as_ptr` through a shared
        // borrow is the derivation the raw accessor performed, and it is what
        // makes those coexistences lawful (F71); a `&mut`-derived raw would be
        // popped by the next shared read of the same buffer.
        if !bUseSubsetSps {
            pSps = ctx.sps_array().as_ptr().cast_mut().add(iSpsId as usize);
        } else {
            pSubsetSps = ctx
                .subset_array()
                .as_ptr()
                .cast_mut()
                .add(iSpsId as usize);
            pSps = std::ptr::addr_of_mut!((*pSubsetSps).pSps);
        }

        // S67 blessed (H2): as `GenerateNewSps` above; the two `as_ref()` arguments point into
        // the paraset Vec buffers, which this retag does not cover.
        // **S3.B2.** Receiver and the entropy-mode read both hoisted above the call:
        // argument one is a `&mut` on the context, and the `param()` read that used
        // to sit in argument eleven is a *shared* reborrow of the same context taken
        // while it is live — F208's shape, and the reason it was invisible before is
        // that `ctx` was a raw. The read is a `bool`.
        // **S7.A3**: the flag is read *before* the split, so no borrow of the context
        // is live across it — F208's shape, resolved by ordering rather than by a raw.
        let kbEntropyCodingModeFlag = ctx.param().iEntropyCodingModeFlag != 0;
        let (pParasetStrategy, pps, _) =
            crate::encoder::paraset_strategy::ctx_strategy_and_pps(ctx);
        iPpsId = pParasetStrategy.InitPps(
            pps,
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
            kbEntropyCodingModeFlag,
        );
        let pPps = ctx.pps_array().as_ptr().cast_mut().add(iPpsId as usize);

        // FMO is not used in SVC coding so far; come back if FMO is needed
        iResult = InitSlicePEncCtx(
            &mut *ctx_dq_layer(ctx, iDlayerIndex as usize),
            false,
            (*pSps).iMbWidth as i32,
            (*pSps).iMbHeight as i32,
            // **S6.D1**: `InitSlicePEncCtx` takes `&SSliceArgument` now.
            // S11.18: derived here, not 85 lines up — see the note at the top of
            // this loop body.
            &ctx.param().sSpatialLayers[iDlayerIndex as usize].sSliceArgument,
        );
        if iResult != 0 {
            return iResult;
        }
        // **T9.H2 step 4 — one retag, nothing held, and `uiSpatialId` moved down here
        // with it.** This layer's three `SDqIdc` writes used to straddle the paraset
        // calls: a cursor taken above `GenerateNewSps`, `uiSpatialId` written through
        // it, then `iSpsId`/`iPpsId` written through the same cursor after `InitPps`
        // — the held-across-a-call shape S67's audit exists to find, surviving only
        // because F71's spelling gave the cursor the `Vec` buffer's provenance.
        //
        // Byte-neutral, and measured rather than reasoned: `grep -rn 'pDqIdcMap|
        // ctx_dq_idc_map' src` finds **no** reader between the old and new write
        // positions — `paraset_strategy.rs` never names the map at all, and the only
        // other consumer is `encoder_ext.rs`'s `WelsInitCurrentLayer`, a different
        // function later in the frame. So the three writes commute into one borrow
        // that ends on the closing brace.
        {
            let pDqIdc = &mut ctx_dq_idc_map(&mut *ctx)[iDlayerIndex as usize];
            pDqIdc.uiSpatialId = iDlayerIndex as i8;
            pDqIdc.iSpsId = iSpsId as u8;
            pDqIdc.iPpsId = iPpsId as u16;
        }

        if ctx.param().bSimulcastAVC || bUseSubsetSps {
            iSpsId += 1;
        }
        iPpsId += 1;
        if bUseSubsetSps {
            ctx.iSubsetSpsNum += 1;
        } else {
            ctx.iSpsNum += 1;
        }
        ctx.iPpsNum += 1;

        iDlayerIndex += 1;
    }

    // S67 blessed (H2): the receiver is a `&mut` into the strategy object's own `Box`, reached
    // without a reference to the context (`ctx_func_list`, F71); nothing else is live.
    {
        // **S7.A3**: the strategy and the three counts split off one `&mut` context.
        let (strategy, pSpsNum, pSubsetSpsNum, pPpsNum) =
            crate::encoder::paraset_strategy::ctx_strategy_and_counts(ctx);
        strategy.UpdateParaSetNum(pSpsNum, pSubsetSpsNum, pPpsNum);
    }
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
pub fn RequestMemorySvc(
    ctx: &mut sWelsEncCtx,
    pExistingParasetList: Option<&SExistingParasetList>,
) -> i32 {
    // **A7, and Miri found it**: this binding used to be `ctx_param(std::ptr::addr_of_mut!(*ctx))`, a raw
    // carrying the parameter block's own provenance, so it outlived every later
    // reach into the same block. `param_mut` is a real `&mut`, so the next
    // `param_mut` anywhere below — `AcquireLayersNals`, `RequestMtResource` — pops
    // it, and the read at `:1269` was through a dead tag. The session's Miri lane
    // refused it in one line and nothing else did; F208's rule, one allocation
    // further in. The binding is gone: every use derives its own.
    let mut iCountNals: i32 = 0;
    let mut iCountLayers: i32 = 0;
    let mut iResult: i32;
    let kiNumDependencyLayers = ctx.param().iSpatialLayerNum;
    let mut iVclLayersBsSizeCount: i32 = 0;

    if kiNumDependencyLayers < 1 || kiNumDependencyLayers > MAX_DEPENDENCY_LAYER as i32 {
        return 1;
    }

    if ctx.param().uiGopSize == 0
        || (ctx.param().uiIntraPeriod != 0 && (ctx.param().uiIntraPeriod % ctx.param().uiGopSize) != 0)
    {
        return 1;
    }

    let pFinalSpatial = &ctx.param().sSpatialLayers[(kiNumDependencyLayers - 1) as usize];
    let iMaxPicWidth = pFinalSpatial.iVideoWidth;
    let iMaxPicHeight = pFinalSpatial.iVideoHeight;
    let iCountMaxMbNum = ((15 + iMaxPicWidth) >> 4) * ((15 + iMaxPicHeight) >> 4);

    iResult = AcquireLayersNals(ctx, &mut iCountLayers, &mut iCountNals);
    if iResult != 0 {
        return 1;
    }

    let kiSpsSize = ParasetStrategy(ctx).GetNeededSpsNum() as i32 * SPS_BUFFER_SIZE;
    let kiPpsSize = ParasetStrategy(ctx).GetNeededPpsNum() as i32 * PPS_BUFFER_SIZE;
    let iNonVclLayersBsSizeCount = SSEI_BUFFER_SIZE + kiSpsSize + kiPpsSize;

    let mut bDynamicSlice = false;
    let mut iSliceBufferSize: i32 = 0;
    let mut iMaxSliceBufferSize: i32 = 0;
    let mut iIndex: i32 = 0;
    while iIndex < ctx.param().iSpatialLayerNum {
        // **S3.B2 — §4.6's copy-out, and borrowck is what asked for it.** `fDlp` was
        // a `&` into the parameter block taken through `param()`, i.e. a shared
        // reborrow of the **whole context** (F208), and the loop writes
        // `iMaxSliceCount` and `iSliceBufferSize` on the context while it is live.
        // Under the old `*mut *mut` root neither derivation was visible to borrowck.
        // The five values the loop actually reads are scalars, so they are copied out
        // and the borrow ends on the next line; `sSliceArgument` is *not* copied
        // whole — only the three fields used, which is S54's remedy at A7's shape.
        let (kiVideoWidth, kiVideoHeight, kuiSliceMode, kuiSliceNum, kuiSliceSizeConstraint) = {
            let fDlp = &ctx.param().sSpatialLayers[iIndex as usize];
            (
                fDlp.iVideoWidth,
                fDlp.iVideoHeight,
                fDlp.sSliceArgument.uiSliceMode,
                fDlp.sSliceArgument.uiSliceNum,
                fDlp.sSliceArgument.uiSliceSizeConstraint,
            )
        };

        let fCompressRatioThr = COMPRESS_RATIO_THR;

        let mut iLayerBsSize = WELS_ROUND_f(
            (((3 * kiVideoWidth * kiVideoHeight) >> 1) as f32) * fCompressRatioThr,
        ) + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
        iLayerBsSize = WELS_ALIGN(iLayerBsSize, 4); // 4 bytes aligned
        let mut iMaxLayerBsSize: i32;
        if kuiSliceMode == SM_SIZELIMITED_SLICE {
            bDynamicSlice = true;
            let uiMaxSliceNumEstimation = std::cmp::min(
                crate::encoder::svc_enc_slice_segment::AVERSLICENUM_CONSTRAINT as u32,
                (iLayerBsSize as u32 / kuiSliceSizeConstraint) + 1,
            );
            ctx.iMaxSliceCount =
                std::cmp::max(ctx.iMaxSliceCount, uiMaxSliceNumEstimation as i32);
            iSliceBufferSize = ((std::cmp::max(
                kuiSliceSizeConstraint,
                iLayerBsSize as u32 / uiMaxSliceNumEstimation,
            ) as i32)
                << 1)
                + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
            iMaxLayerBsSize = iSliceBufferSize * uiMaxSliceNumEstimation as i32;
        } else {
            ctx.iMaxSliceCount =
                std::cmp::max(ctx.iMaxSliceCount, kuiSliceNum as i32);
            if ctx.param().bUseLoadBalancing {
                iSliceBufferSize = iLayerBsSize + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
            } else {
                iSliceBufferSize = ((iLayerBsSize / kuiSliceNum as i32) << 1)
                    + MAX_MACROBLOCK_SIZE_IN_BYTE_x2;
            }
            iMaxLayerBsSize = iSliceBufferSize * kuiSliceNum as i32;
        }
        iMaxLayerBsSize = std::cmp::max(iMaxLayerBsSize, iLayerBsSize);
        iVclLayersBsSizeCount += iMaxLayerBsSize;
        iMaxSliceBufferSize = std::cmp::max(iMaxSliceBufferSize, iSliceBufferSize);
        ctx.iSliceBufferSize[iIndex as usize] = iSliceBufferSize;
        iIndex += 1;
    }
    let iTargetSpatialBsSize = iVclLayersBsSizeCount;
    let iCountBsLen = iNonVclLayersBsSizeCount + iVclLayersBsSizeCount;

    iMaxSliceBufferSize = std::cmp::min(iMaxSliceBufferSize, iTargetSpatialBsSize);
    let iTotalLength = iCountBsLen;

    ctx.param_mut().iNumRefFrame = crate::encoder::rc::WELS_CLIP3(
        ctx.param().iNumRefFrame,
        MIN_REF_PIC_COUNT,
        if ctx.param().iUsageType == CAMERA_VIDEO_REAL_TIME {
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
    ctx.pOut = Some(crate::encoder::nal_encap::SWelsEncoderOutput::new_boxed(
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
    ctx.pFrameBs = vec![0u8; iTotalLength.max(0) as usize];
    ctx.iFrameBsSize = iTotalLength;
    ctx.iPosBsBuffer = 0;

    // for dynamic slice mode && CABAC, allocate slice buffers to restore slice data.
    // These are `sDss.pRestoreBuffer` in the two dynamic MB loops: CABAC
    // renormalisation can rewrite bytes already emitted, so stepping back over a
    // slice boundary has to restore the bytes as well as the coder state.
    if bDynamicSlice && ctx.param().iEntropyCodingModeFlag != 0 {
        for iIdx in 0..MAX_THREADS_NUM {
            // **T7.C5 — owned.** The last live allocator call sites in `src/encoder`
            // were this one and its free below. `WelsMalloc` here was *uninitialized*
            // (not `WelsMallocz`), so `vec![0; n]` writes zeros the C++ does not — the
            // same recorded deviation `pFrameBs` above carries, and sound for the same
            // reason: every read of this buffer sits behind a write cursor, since
            // `StashPopMBStatus` only reads back the bytes `StashMBStatus` just wrote.
            ctx.pDynamicBsBuffer[iIdx] = vec![0u8; iMaxSliceBufferSize.max(0) as usize];
        }
    }
    // for pSlice bs buffers
    if ctx.param().iMultipleThreadIdc > 1
        && crate::encoder::slice_multi_threading::RequestMtResource(
            ctx,
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
    ctx.eRefStrategy = crate::encoder::ref_list_mgr_svc::RefStrategyKind::Select(
        ctx.param().iUsageType,
        ctx.param().bEnableLongTermReference,
    );

    // encoder_ext.cpp:1141-1179 allocates five context-wide per-macroblock arrays
    // here -- `pIntra4x4PredModeBlocks`, `pNonZeroCountBlocks`, `pMvUnitBlock4x4`
    // (two banks), `pRefIndexBlock4x4` (two banks) and `pSadCostMb` -- and
    // `InitMbInfo` points each `SMB`'s five pointers into them. **T6.C1** made all
    // five inline arrays of `SMB`, which is allocated (and zeroed) by `InitMbListD`,
    // so there is nothing left to allocate and nothing left to fail.

    ctx.iGlobalQp = 26; // global qp in default

    // **T6.H5.** `SLTRState::default()` is all-zero field for field, which is what
    // `WelsMallocz` left; `ResetLtrState` then writes the four `-1`s and the
    // `LTR_DIRECT_MARK` that make it a *state* rather than a zeroed block, exactly as
    // before — the loop is unchanged.
    ctx.pLtr = vec![
        crate::encoder::ref_list_mgr_svc::SLTRState::default();
        kiNumDependencyLayers as usize
    ];
    for i in 0..kiNumDependencyLayers as usize {
        // S67 blessed (H2): nothing is held across it — the cursor is minted and consumed in
        // the same call.
        crate::encoder::ref_list_mgr_svc::ResetLtrState(ctx_ltr_at(&mut *ctx, i));
    }

    // stride tables
    if AllocStrideTables(ctx, kiNumDependencyLayers) != 0 {
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
    ctx.pWelsSvcRc = (0..kiNumDependencyLayers as usize)
        .map(|_| crate::encoder::rc::SWelsSvcRc::default())
        .collect();

    // pVaa memory allocation
    if ctx.param().iUsageType == SCREEN_CONTENT_REAL_TIME {
        // encoder_ext.cpp:1708, SVAAFrameInfoExt + RequestMemoryVaaScreen. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    // **T6.F3**: one constructor where the C++ cuts seven `CMemoryAlign` blocks.
    // `SVAAFrameInfo` is `Box`-built and owns its per-frame result arrays; the
    // background-detection pair exists exactly when the C++ allocates it.
    // **T6.H10**: `Box::into_raw` stood here; the context holds the `Box`.
    ctx.pVaa = Some(crate::encoder::wels_preprocess::SVAAFrameInfo::new(
        iCountMaxMbNum,
        ctx.param().bEnableBackgroundDetection,
    ));

    if ctx.param().bEnableAdaptiveQuant {
        // encoder_ext.cpp:1720, sAdaptiveQuantParam buffers. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // End of pVaa memory allocation

    // **T6.H7.** A `WelsMallocz`'d block of `kiNumDependencyLayers` null pointers,
    // which `InitDqLayers` then fills with `Box::into_raw`'d lists. `None` is that
    // null, and the `Box` stays where it already was — it just has an owner now.
    ctx.ppRefPicListExt = (0..kiNumDependencyLayers).map(|_| None).collect();

    // **T6.H8.** As `ppRefPicListExt` just above: a block of nulls that
    // `InitDqLayers` fills with `Box`-built layers, so a `Vec` of `None`s that it
    // fills with the `Box`es themselves.
    ctx.ppDqLayerList = (0..kiNumDependencyLayers).map(|_| None).collect();

    // S11.37: the callee's claim — the layer builder's remaining raw walks.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    {
        iResult = unsafe { InitDqLayers(ctx, pExistingParasetList) };
    }
    if iResult != 0 {
        return iResult;
    }

    if InitMbListD(ctx) != 0 {
        return 1;
    }

    let mut iMvdRange: i32 = 0;
    // §4.6, reorder: the two out-parameters borrow the context, so the range is
    // computed into locals and written back.
    let mut iMvRangeOut = ctx.iMvRange;
    GetMvMvdRange(ctx.param(), &mut iMvRangeOut, &mut iMvdRange);
    ctx.iMvRange = iMvRangeOut;
    let kuiMvdInterTableSize = iMvdRange << 2; // intepel*4 = qpel
    let kuiMvdInterTableStride = 1 + (kuiMvdInterTableSize << 1); // qpel_mv_range*2 = (+/-)
    let kuiMvdCacheAlignedSize = kuiMvdInterTableStride * 2; // sizeof(uint16_t)

    ctx.iMvdCostTableSize = kuiMvdInterTableSize;
    ctx.iMvdCostTableStride = kuiMvdInterTableStride;
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
    ctx.pMvdCostTable = vec![
        0u16;
        (52 * kuiMvdCacheAlignedSize + kuiMvdCostTableOvershoot) as usize
            / std::mem::size_of::<u16>()
    ];
    crate::encoder::md::MvdCostInit(
        ctx.mvd_cost_table_mut(),
        kuiMvdInterTableStride,
    );

    let idDec = match ctx.ref_list(0) {
        Some(pRefList0) if !pRefList0.pRef.is_empty() => Some(pRefList0.pRef.at(0)),
        _ => None, // error here
    };
    ctx.pDecPic = idDec;

    // T6.G3: the head of each array, which is what "= pSpsArray" said. Nothing
    // re-aims these, in this port or in the C++ — `encoder_ext.cpp` assigns them here
    // and nowhere else — so the active set is position 0 for the encoder's whole life.
    ctx.iSps = Some(SpsId(0));
    ctx.iPps = Some(PpsId(0));

    0
}

/// `InitSliceSettings` — encoder_ext.cpp:2018.
///
/// Resolves the per-layer slice arguments, then derives `iMultipleThreadIdc` and the
/// maximum slice count from them.
///
/// # Safety
/// `pCodingParam` and `pMaxSliceCount` must be non-null.
pub fn InitSliceSettings(
    pLogCtx: SLogContext,
    // S11.13: the coding parameters arrive by reference — see `InitializeInternal`.
    pCodingParam: &mut SWelsSvcCodingParam,
    kiCpuCores: i32,
    pMaxSliceCount: &mut i16,
) -> i32 {
    let mut iSpatialIdx: i32 = 0;
    let iSpatialNum = pCodingParam.iSpatialLayerNum;
    let mut iMaxSliceCount: u16 = 0;

    loop {
        // **S11.37: NLL does the whole F70 dance.** The Miri-found defect there
        // was a *held* `&mut sSliceArgument` popped by the validator's second
        // `&mut` to the same field; the raw cursor dodged it. With the borrow
        // taken per use — the mode read, the validator's argument, the count
        // reads after it — no borrow spans another derivation, which is the
        // ordering the raw was hand-maintaining. The two geometry scalars are
        // sibling fields, read out before the argument's `&mut`.
        let pDlp = &mut pCodingParam.sSpatialLayers[iSpatialIdx as usize];
        let (kiVideoWidth, kiVideoHeight) = (pDlp.iVideoWidth, pDlp.iVideoHeight);

        match pDlp.sSliceArgument.uiSliceMode {
            SM_SIZELIMITED_SLICE => {
                iMaxSliceCount = crate::encoder::svc_enc_slice_segment::AVERSLICENUM_CONSTRAINT
                    as u16;
            }
            crate::api::codec_api::SliceModeEnum::SM_FIXEDSLCNUM_SLICE => {
                let kiRCMode = pCodingParam.iRCMode;
                let iReturn =
                    crate::encoder::svc_enc_slice_segment::SliceArgumentValidationFixedSliceMode(
                        pLogCtx,
                        &mut pCodingParam.sSpatialLayers[iSpatialIdx as usize].sSliceArgument,
                        kiRCMode,
                        kiVideoWidth,
                        kiVideoHeight,
                    );
                if iReturn != 0 {
                    return ENC_RETURN_UNSUPPORTED_PARA;
                }

                if pCodingParam.sSpatialLayers[iSpatialIdx as usize].sSliceArgument.uiSliceNum as u16 > iMaxSliceCount {
                    iMaxSliceCount = pCodingParam.sSpatialLayers[iSpatialIdx as usize].sSliceArgument.uiSliceNum as u16;
                }
            }
            SM_SINGLE_SLICE | crate::api::codec_api::SliceModeEnum::SM_RASTER_SLICE => {
                if pCodingParam.sSpatialLayers[iSpatialIdx as usize].sSliceArgument.uiSliceNum as u16 > iMaxSliceCount {
                    iMaxSliceCount = pCodingParam.sSpatialLayers[iSpatialIdx as usize].sSliceArgument.uiSliceNum as u16;
                }
            }
            _ => {}
        }

        iSpatialIdx += 1;
        if iSpatialIdx >= iSpatialNum {
            break;
        }
    }

    pCodingParam.iMultipleThreadIdc = std::cmp::min(kiCpuCores as u16, iMaxSliceCount);
    // Loop filter requested to be enabled, with threading enabled: disable it on slice
    // boundaries, since that is not allowed with multithreading.
    if pCodingParam.iLoopFilterDisableIdc == 0 && pCodingParam.iMultipleThreadIdc != 1 {
        pCodingParam.iLoopFilterDisableIdc = 2;
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
pub fn GetMultipleThreadIdc(
    pLogCtx: SLogContext,
    // S11.13: the coding parameters arrive by reference — see `InitializeInternal`.
    pCodingParam: &mut SWelsSvcCodingParam,
    iSliceNum: &mut i16,
    iCacheLineSize: &mut i32,
    uiCpuFeatureFlags: &mut u32,
) -> i32 {
    // number of logical processors on the physical processor package; zero means HTT
    // is not supported
    let mut uiCpuCores: i32 = 0;
    *uiCpuFeatureFlags = crate::decoder::decoder_core::WelsCPUFeatureDetect(&mut uiCpuCores);

    *iCacheLineSize = 16; // 16 bytes aligned in default

    if 0 == pCodingParam.iMultipleThreadIdc && uiCpuCores == 0 {
        // cpuid not supported, or doesn't expose the number of cores: use the
        // high-level system API to detect physical/logical processors
        uiCpuCores = crate::encoder::slice_multi_threading::DynamicDetectCpuCores();
    }

    if 0 == pCodingParam.iMultipleThreadIdc {
        pCodingParam.iMultipleThreadIdc = if uiCpuCores > 0 { uiCpuCores as u16 } else { 1 };
    }

    // So many cpu cores up to MAX_THREADS_NUM means server platforms; for client
    // applications it is constrained to MAX_THREADS_NUM here.
    pCodingParam.iMultipleThreadIdc = crate::encoder::rc::WELS_CLIP3(
        pCodingParam.iMultipleThreadIdc,
        1,
        MAX_THREADS_NUM as u16,
    );
    uiCpuCores = pCodingParam.iMultipleThreadIdc as i32;

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
pub fn WelsInitEncoderExt(
    ppCtx: &mut Option<Box<sWelsEncCtx>>,
    // S11.13: the coding parameters arrive by reference — see `InitializeInternal`.
    pCodingParam: &mut SWelsSvcCodingParam,
    pLogCtx: SLogContext,
    pExistingParasetList: Option<&SExistingParasetList>,
) -> i32 {
    let mut iSliceNum: i16 = 1; // number of slices used
    let mut iCacheLineSize: i32 = 16; // on-chip cache line size in bytes
    let mut uiCpuFeatureFlags: u32 = 0;
    // S11.13: the null guard retires with the raw (T9.H).
    let mut iRet = crate::encoder::wels_encoder_ext::ParamValidationExt(pLogCtx, pCodingParam);
    if iRet != 0 {
        return iRet;
    }
    iRet = pCodingParam.DetermineTemporalSettings();
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
    // this is the same statement. **S3.B2**: the context is held as the `Box`
    // itself for the whole construction — S42's allocation root, with the
    // `into_raw`/`from_raw` round-trip deleted: the error paths hand the box to
    // `WelsUninitEncoderExt` by value, and the success path moves it into the slot.
    *ppCtx = None;

    // C++ mallocs and memsets sWelsEncCtx; Box::new of a Default context is the
    // equivalent, and Default is the all-zero/null state for every member.
    let mut ctxBox = Box::new(sWelsEncCtx::default());

    // **S8.1**: this was guarded on `!pLogCtx.is_null()`, leaving the field at its
    // `Default` when the caller had no trace context. The by-value parameter spells
    // that same absence as `SLogContext::default()` — which is bit-for-bit what
    // `sWelsEncCtx::default()` already put there — so the guard and the assignment
    // it skipped collapse into one unconditional copy.
    ctxBox.sLogCtx = pLogCtx;

    // **T7.C6**: `pCtx->pMemAlign = new CMemoryAlign(iCacheLineSize)` stood here
    // (`encoder_ext.cpp:1631`), the encoder's first allocation. Nothing in
    // `src/encoder` allocates through it any more, so the object, the field and the
    // teardown entry below are gone together. `iCacheLineSize` is still validated and
    // still reaches `InitFunctionPointers`; only the allocator it used to size is
    // gone.

    // **T6.H11**: `AllocCodingParam` and its failure branch were here. The context
    // owns the parameters, so the allocation is a `Box` and failure is panic-on-OOM
    // — the trade `pOut` made at T3.6 and the paraset arrays at T6.H2.
    ctxBox.pSvcParam = Some(crate::encoder::param_svc::NewCodingParam());
    *ctxBox.param_mut() = *pCodingParam;

    // **T6.I1**: a `WelsMallocz` of `sizeof(SWelsFuncPtrList)` and its null branch
    // stood here. The context is born with the table (`Box`, every slot `None` —
    // the same image the memset produced), so there is nothing to allocate and no
    // allocation to fail; `InitFunctionPointers` writes over it exactly as before.
    // The trade is `pSvcParam`'s at T6.H11 and `pOut`'s at T3.6: panic-on-OOM.
    // A7: T9.G6's hoist is gone with the argument — the callee holds the context
    // and derives the parameters itself.
    iRet = crate::encoder::encoder_context::InitFunctionPointers(
        &mut ctxBox,
        uiCpuFeatureFlags,
    );
    if iRet != ENC_RETURN_SUCCESS {
        WelsUninitEncoderExt(Some(ctxBox));
        return iRet;
    }

    ctxBox.iActiveThreadsNum = pCodingParam.iMultipleThreadIdc as i16;
    ctxBox.iMaxSliceCount = iSliceNum as i32;
    // **S3.B2.** `pCtxTmp` existed only to give the `*mut *mut sWelsEncCtx`
    // parameter an lvalue to point at. The parameter is a `&mut sWelsEncCtx` now,
    // so the temporary — and the second level of indirection it stood for — go.
    iRet = RequestMemorySvc(&mut ctxBox, pExistingParasetList);
    if iRet != 0 {
        WelsUninitEncoderExt(Some(ctxBox));
        return iRet;
    }

    if pCodingParam.iEntropyCodingModeFlag != 0 {
        crate::encoder::set_mb_syn_cabac::WelsCabacInit(&mut *ctxBox);
    }
    // T9.G6: hoisted — the call takes the context retag and this argument reads
    // through the same context (shape B).
    let iRCMode = ctxBox.param().iRCMode;
    crate::encoder::rc::WelsRcInitModule(&mut ctxBox, iRCMode);

    // S3.B1: the take dance — the box is held out of the slot for the call that
    // needs `&mut` to both the vpp and the context, then stored. While it is out,
    // `pVpp` is `None`, and `AllocSpatialPictures` provably never reads the slot.
    let Some(mut vpp) = crate::encoder::wels_preprocess::CWelsPreProcess::CreatePreProcess(&mut ctxBox) else {
        WelsUninitEncoderExt(Some(ctxBox));
        return 1;
    };
    iRet = vpp.AllocSpatialPictures(&mut ctxBox);
    ctxBox.pVpp = Some(vpp);
    if iRet != 0 {
        WelsUninitEncoderExt(Some(ctxBox));
        return iRet;
    }

    ctxBox.iStatisticsLogInterval = STATISTICS_LOG_INTERVAL_MS;
    ctxBox.uiLastTimestamp = -1;
    ctxBox.bDeliveryFlag = true;

    // `encoder_ext.cpp:2386` — the doubled `0x` is the reference's own: the
    // format writes `0x%p` and `%p` prints its own prefix.
    crate::common::wels_trace::WelsLog(
        pLogCtx,
        crate::common::wels_trace::WELS_LOG_INFO,
        &format!("WelsInitEncoderExt(), pCtx= 0x{:p}.", std::ptr::addr_of!(*ctxBox)),
    );
    *ppCtx = Some(ctxBox);

    0
}

/// `STATISTICS_LOG_INTERVAL_MS` — `wels_const.h`.
pub const STATISTICS_LOG_INTERVAL_MS: i32 = 5000;

/// `FreeSliceInLayer` — encoder_ext.cpp:942.
///
/// # Safety
/// `pDq` must be non-null.
pub fn FreeSliceInLayer(pDq: &mut SDqLayer) {
    for iIdx in 0..MAX_THREADS_NUM {
        crate::encoder::svc_encode_slice::FreeSliceBuffer(pDq, iIdx);
    }
}

/// `FreeDqLayer` — encoder_ext.cpp:951.
///
/// # Safety
/// `pDq` must have come from `InitDqLayers` and must not be used afterwards.
pub fn FreeDqLayer(p: &mut SDqLayer) {

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
    // unsafe-cat: instrument(test) — S11.5: this is a *test helper*, not port
    // code. It was tagged `port-raw(Phase 9)`, which put it in the convertible
    // queue; the queue is product work, and D-exit-4's enumerated floor is the
    // test instruments. Same allow, honest category.
    fn build_gate_context() -> *mut sWelsEncCtx {
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
            crate::encoder::wels_encoder_ext::ParamValidationExt(crate::common::wels_trace::SLogContext::default(), &mut param),
            ENC_RETURN_SUCCESS
        );
        assert_eq!(param.DetermineTemporalSettings(), ENC_RETURN_SUCCESS);
        assert_eq!(
            GetMultipleThreadIdc(
                crate::common::wels_trace::SLogContext::default(),
                &mut param,
                &mut iSliceNum,
                &mut iCacheLineSize,
                &mut uiCpuFeatureFlags
            ),
            0
        );

        // S3.B2: built through the `Box` like `WelsInitEncoderExt` proper; the raw
        // the callers hold is minted once, at the end, when construction is done.
        let mut ctxBox = Box::new(sWelsEncCtx::default());
        ctxBox.pSvcParam = Some(NewCodingParam());
        *ctxBox.param_mut() = param;
        // T6.I1: the table comes with the context; see `WelsInitEncoderExt`.
        assert_eq!(
            InitFunctionPointers(&mut ctxBox, uiCpuFeatureFlags),
            ENC_RETURN_SUCCESS
        );
        ctxBox.iActiveThreadsNum = param.iMultipleThreadIdc as i16;
        ctxBox.iMaxSliceCount = iSliceNum as i32;

        assert_eq!(RequestMemorySvc(&mut ctxBox, None), 0, "RequestMemorySvc");
        Box::into_raw(ctxBox)
    }

    /// Blocker C: the parameter-set arrays are allocated and populated.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn request_memory_svc_builds_the_parameter_sets() {
        unsafe {
            let pCtx = build_gate_context();

            assert!(!(*pCtx).pSpsArray.is_empty(), "pSpsArray still unallocated");
            assert!(!(*pCtx).pPPSArray.is_empty(), "pPPSArray still unallocated");
            // The configuration needs no subset SPS, and the C++ allocated nothing
            // at all for it — an empty `Vec`, which `subset_array()` reads as the
            // emptiness the raw field's null stood for.
            assert!((*pCtx).pSubsetArray.is_empty(), "pSubsetArray was not needed");
            assert!((*pCtx).subset_array().is_empty());
            assert_eq!((*pCtx).iSpsNum, 1);
            assert_eq!((*pCtx).iPpsNum, 1);
            assert_eq!((*pCtx).iSubsetSpsNum, 0);
            assert_eq!(ctx_sps(&mut *pCtx), (*pCtx).sps_array().as_ptr().cast_mut());
            assert_eq!(ctx_pps(&mut *pCtx), (*pCtx).pps_array().as_ptr().cast_mut());

            // The SPS the strategy generated must be the one Phase 3 proved
            // byte-exact against the C++ reference for this configuration.
            let sps = &(*pCtx).sps_array()[0];
            assert_eq!(sps.iMbWidth, 10);
            assert_eq!(sps.iMbHeight, 6);
            assert_eq!(sps.uiLog2MaxFrameNum, 15);
            assert_eq!(sps.uiPocType, 2);
            assert_eq!(sps.iLevelIdc, 13);

            let pps = &(*pCtx).pps_array()[0];
            assert_eq!(pps.iPicInitQp, 26);
            assert!(pps.bDeblockingFilterControlPresentFlag);

            WelsUninitEncoderExt(Some(Box::from_raw(pCtx)));
        }
    }

    /// Blocker C, second half: the DQ layers, reference lists and macroblock list
    /// exist, which is what `pCurDqLayer` is selected from.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn request_memory_svc_builds_the_dq_layers() {
        unsafe {
            let pCtx = build_gate_context();

            let pDq = ctx_dq_layer(&*pCtx, 0);
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

            assert!((*pCtx).ref_list(0).is_some());
            assert!(!(*pCtx).ref_list(0).expect("just checked").pRef.is_empty());
            assert_eq!(
                (*pCtx).pDecPic,
                Some((*pCtx).ref_list(0).expect("just checked").pRef.at(0))
            );

            assert!((*pCtx).pStrideTab.is_some());
            assert!(!(*pCtx).mvd_cost_table().is_empty());
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
pub fn WelsUninitEncoderExt(pEncContext: Option<Box<sWelsEncCtx>>) {
    // **T8.B5 — the teardown takes the context by value**, which is what
    // `encoder_ext.cpp:1878`'s `*ppCtx = NULL` at the end was expressing: after
    // this call the caller has no context, and now that is a fact about the type
    // rather than a store the function has to remember. **S3.B2**: the free
    // cascade walks the context through the `Box` itself now — the
    // `into_raw`/`from_raw` bracket is gone, and the two raws that remain
    // (`pVpp`, and `ctx_dq_layer`'s slot read for `FreeDqLayer`) are each
    // statement-scoped and name *other* allocations.
    let Some(mut ctxBox) = pEncContext else {
        return;
    };

    // `encoder_ext.cpp:2250-2252` — the teardown announces itself before any
    // free runs, through the context's own log sink. (S3.B2: the `sLogCtx` raw is
    // still `addr_of_mut!` — `WelsLog` wants a pointer — but it is statement-scoped
    // and nothing else of the context is live beside it.)
    {
        let iMultipleThreadIdc = ctxBox.param().iMultipleThreadIdc;
        let kpCtxForLog: *const sWelsEncCtx = std::ptr::addr_of!(*ctxBox);
        crate::common::wels_trace::WelsLog(
            ctxBox.sLogCtx,
            crate::common::wels_trace::WELS_LOG_INFO,
            &format!(
                "WelsUninitEncoderExt(), pCtx= {:p}, iMultipleThreadIdc= {}.",
                kpCtxForLog, iMultipleThreadIdc
            ),
        );
    }

    // S3.B1: the take dance again — and the store back is the drop, because the
    // teardown is the one caller that wants the box gone afterwards.
    if let Some(mut vpp) = ctxBox.pVpp.take() {
        vpp.FreeSpatialPictures(&mut ctxBox);
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
        drop(ctxBox.pOut.take());
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
        for ilayer in 0..ctxBox.ppDqLayerList.len() {
            // S11.37: the safe accessor — `None` where the raw answered null.
            if let Some(pLayer) = crate::encoder::encoder_context::dq_layer_mut(&mut *ctxBox, ilayer) {
                FreeDqLayer(pLayer);
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

    drop(ctxBox);
}

// ============================================================================
// The encoding half of encoder_ext.cpp: WelsEncoderEncodeExt and its helpers.
//
// Translated statement for statement from `codec/encoder/core/src/encoder_ext.cpp`.
// Line references in the doc comments are to that file.
// ============================================================================

/// `encoder_ext.cpp:2393`.
// S4.C: `*mut` -> `&`. The body reads one table entry and writes nothing.
pub fn GetTemporalLevel(
    fDlp: &SSpatialLayerInternal,
    kiFrameNum: i32,
    kiGopSize: i32,
) -> i32 {
    let kiCodingIdx = kiFrameNum & (kiGopSize - 1);
    fDlp.uiCodingIdx2TemporalId[kiCodingIdx as usize] as i32
}

/// `encoder_ext.cpp:3114`.
pub fn GetSubSequenceId(pCtx: &mut sWelsEncCtx, eFrameType: EVideoFrameType) -> i32 {
    if eFrameType == EVideoFrameType::videoFrameTypeIDR {
        0
    } else if eFrameType == EVideoFrameType::videoFrameTypeI {
        1
    } else if eFrameType == EVideoFrameType::videoFrameTypeP {
        if pCtx.bCurFrameMarkedAsSceneLtr {
            2
        } else {
            // T0:3 T1:4 T2:5 T3:6
            3 + pCtx.uiTemporalId as i32
        }
    } else {
        3 + MAX_TEMPORAL_LAYER_NUM as i32
    }
}

/// `encoder_ext.cpp:2797`. Swap the current DQ layer with the next one and make the
/// outgoing layer the reference.
pub fn WelsSwapDqLayers(pCtx: &mut sWelsEncCtx, kiNextDqIdx: i32) {
    // The outgoing layer's *position*, not its address — T6.D3, and since T6.G2 the
    // context holds nothing else: `iCurDqLayer` **is** the index, so the round trip
    // through `pCurDqLayer->iDqIdx` that this site used to need is gone. The
    // `expect` cannot fire on a live path — the frame loop makes a layer current
    // before any swap — and the old spelling dereferenced a null pointer there.
    let kRefIdx = pCtx.iCurDqLayer.expect("WelsSwapDqLayers with no current layer");
    set_current_layer(pCtx, Some(LayerIdx(kiNextDqIdx as u8)));
    // S10.3b: `current_layer_mut` — this body holds `&mut sWelsEncCtx`, which is
    // the borrow that cannot exist while the fork is live, so the fork-shared raw
    // was carrying a tag for a single-threaded write.
    if let Some(pCurLayer) = current_layer_mut(pCtx) {
        pCurLayer.pRefLayer = Some(kRefIdx);
    }
}

// `StampLayerPictureViews` stood here — the once-per-frame stamp of
// `sRefPicView`/`sDecPicView` (T6.F5). Phase 9 E3's harvest deleted both fields:
// the reference readers resolve the picture per call (`layer_ref_pic` +
// `SPicture::data_ptr_shared`/`stride`/`iPictureType`), and the reconstruction
// view had zero readers. One `cursor` tag retires with it.


/// `encoder_ext.cpp:2808`. Prefetch the reference picture after `WelsBuildRefList`.
pub fn PrefetchReferencePicture(pCtx: &mut sWelsEncCtx, keFrameType: EVideoFrameType) {
    let kiSliceCount = current_layer_ref(pCtx).expect("the frame's current layer is stamped").iMaxSliceNum;
    // C++ declares `uint8_t uiRefIdx = -1;`, which wraps to 255.
    let mut uiRefIdx: u8 = 0xff;

    debug_assert!(kiSliceCount > 0);
    if keFrameType != EVideoFrameType::videoFrameTypeIDR {
        debug_assert!(pCtx.iNumRef0 > 0);
        // always get item 0 due to reordering done
        pCtx.pRefPic = pCtx.pRefList0[0];
        current_layer_mut(pCtx).expect("the frame's current layer is stamped").pRefPic = pCtx.pRefPic;
        uiRefIdx = 0; // reordered reference index
    } else {
        // safe for IDR coding
        pCtx.pRefPic = None;
        current_layer_mut(pCtx).expect("the frame's current layer is stamped").pRefPic = None;
    }

    let mut iIdx = 0;
    while iIdx < kiSliceCount {
        // S11.35: the safe twin, per iteration — `None` where the raw answered
        // null, and the borrow ends with the statement.
        if let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(
            current_layer_mut(pCtx).expect("the frame's current layer is stamped"),
            iIdx,
        ) {
            pSlice.sSliceHeaderExt.sSliceHeader.uiRefIndex = uiRefIdx;
        }
        iIdx += 1;
    }
}

/// `encoder_ext.cpp:3376`.
pub fn ClearFrameBsInfo(pCtx: &mut sWelsEncCtx, pFbi: &mut SFrameBSInfo) {
    (*pFbi).sLayerInfo[0].pBsBuf = pCtx.frame_bs();
    (*pFbi).sLayerInfo[0].pNalLengthInByte = pCtx.pOut.as_deref_mut().expect("pOut lives").sNalLen.as_mut_ptr();

    for i in 0..(*pFbi).iLayerNum as usize {
        (*pFbi).sLayerInfo[i].iNalCount = 0;
        (*pFbi).sLayerInfo[i].eFrameType = EVideoFrameType::videoFrameTypeSkip;
    }
    (*pFbi).iLayerNum = 0;
    (*pFbi).iFrameSizeInBytes = 0;
}

/// `encoder_ext.cpp:3341`. Roll the encoder state back one frame after the rate
/// controller decides to drop it.
pub fn StackBackEncoderStatus(pEncCtx: &mut sWelsEncCtx, keFrameType: EVideoFrameType) {
    // S11.37: S11.19's shape — the one context value the writes below interleave
    // with (`ctx_sps`'s POC width) is read out as a scalar first, so the record's
    // `&mut` is taken once, holds nothing across a context reach, and the raw
    // cursor is gone.
    let kiDid = pEncCtx.uiDependencyId as usize;
    let kiLog2MaxPocLsb = crate::encoder::svc_encode_slice::ctx_sps_ref(pEncCtx).map_or(0, |s| s.iLog2MaxPocLsb);

    // for bitstream writing
    pEncCtx.iPosBsBuffer = 0; // reset bs buffer position
    pEncCtx.pOut.as_deref_mut().expect("pOut lives").iNalIndex = 0; // reset NAL index
    pEncCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex = 0; // reset index of Layer Bs

    // Was `InitBits(&pOut->sBsWrite, pOut->pBsBuffer, pOut->uiSize)`. The buffer
    // stays on `pOut` where it already was — owned outright since T3.6, so its
    // length is `sBsBuffer.len()` and not a field; the writer is a position,
    // and resetting it is the whole of what `InitBits` did that still means
    // anything (F13's third site: the `*const`-declared, `*mut`-stored, written-
    // through buffer parameter is gone, not amended).
    pEncCtx.pOut.as_deref_mut().expect("pOut lives").sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();

    if keFrameType == EVideoFrameType::videoFrameTypeP
        || keFrameType == EVideoFrameType::videoFrameTypeI
    {
        {
            let pParamInternal = &mut pEncCtx.param_mut().sDependencyLayers[kiDid];
            pParamInternal.iFrameIndex -= 1;
            if pParamInternal.iPOC != 0 {
                pParamInternal.iPOC -= 2;
            } else {
                pParamInternal.iPOC = (1 << kiLog2MaxPocLsb) - 2;
            }
        }

        let iDid = pEncCtx.uiDependencyId as i32;
        crate::encoder::encoder_context::LoadBackFrameNum(pEncCtx, iDid);

        pEncCtx.eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        pEncCtx.eSliceType = EWelsSliceType::P_SLICE;
        // eNalPriority is not stacked back: it is updated at the start of coding a frame.
    } else if keFrameType == EVideoFrameType::videoFrameTypeIDR {
        pEncCtx.param_mut().sDependencyLayers[kiDid].uiIdrPicId -= 1;
        // set the next frame to be IDR
        let iDid = pEncCtx.uiDependencyId as i32;
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
pub fn WelsInitCurrentLayer(pCtx: &mut sWelsEncCtx, _kiWidth: i32, _kiHeight: i32) {
    // **T6.F1**: the layer is stamped with this frame's picture handles here, once a
    // frame, and the per-macroblock mode-decision family resolves them through it.
    //
    // **S11.39: A3's raw derivation is gone — its premise expired (F285's shape).**
    // The ruling kept `ctx_ref_list_raw` because the value it fed was *stored* —
    // `SDqLayer::pRefList`, read by the fork all frame — so it had to carry the
    // list's own provenance rather than a retag from this body's `&mut`. S10.8
    // deleted that field. What crosses the fork now is `RecPicView`/`RoPicView`,
    // and those capture their plane roots from the plane `Vec` headers themselves
    // (`SharedCells::from_parts`), so nothing a worker reads derives from the
    // borrow chain that reached the picture here. The list is resolved per
    // statement below, at the same slot the raw named.
    // S11.35: the base-slice cursor is gone — see the stamp loop below.
    let kiCurDid = pCtx.uiDependencyId;
    // A7, §4.6 reorder: the flag is a scalar, so it does not have to be a live
    // borrow of the context across the calls below.
    let kbUseSubsetSpsFlag =
        !pCtx.param().bSimulcastAVC && (kiCurDid as i32) > BASE_DEPENDENCY_ID;
    let iSliceCount =
        current_layer_ref(pCtx).expect("the frame's current layer is stamped").iMaxSliceNum;
    // S11.39: the parameter cursor is gone with the body's `unsafe` — both of its
    // reads were scalars (`uiIdrPicId`, `iFrameNum`), and each now goes through
    // `param()` at its use site, A7's route for a body that holds nothing.

    // RHS first, then the place — assignment order is what lets the layer stamp
    // sit beside a context read (`:2079`'s idiom).
    current_layer_mut(pCtx).expect("the frame's current layer is stamped").pDecPic =
        pCtx.pDecPic;

    debug_assert!(iSliceCount > 0);

    // T9.H2 step 4: both reads were through a cursor taken 10 lines up and held
    // across the body's other reaches into the context; they are one bounded borrow
    // that ends on the semicolon.
    let (mut iCurPpsId, iCurSpsId) = {
        let pDqIdc = &ctx_dq_idc_map(pCtx)[kiCurDid as usize];
        (pDqIdc.iPpsId as i32, pDqIdc.iSpsId as i32)
    };

    // The IDR loop index was this call's second argument, read through the deleted
    // cursor; the same field, one statement earlier, and nothing between writes it.
    let kiIdrLoop = (pCtx.param().sDependencyLayers[kiCurDid as usize].uiIdrPicId as i32 - 1)
        .abs()
        % MAX_PPS_COUNT as i32;
    iCurPpsId = ParasetStrategy(pCtx).GetCurrentPpsId(iCurPpsId, kiIdrLoop);

    // T6.G3. The C++ writes the id and then an address derived from it, three times
    // over (`encoder_ext.cpp:2560-2576`); the layer keeps the id and the slice
    // header's own `iPpsId`/`iSpsId` — already here, already the same numbers — are
    // what the header carries. The two pointer copies the header used to take are
    // gone with the fields.
    //
    // **S11.39: every context scalar the stamps below read is hoisted here** —
    // F114's width rule applied to a borrow instead of a signature — so a single
    // `&mut` on the layer can span every stamp from `iPps` to `uiTemporalId`.
    // Nothing between the old read sites and these writes touches the sources.
    let kbSliceHeaderExtFlag = pCtx.eNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;
    let keNalPriority = pCtx.eNalPriority;
    let keNalType = pCtx.eNalType;
    let kbNeedPrefixNalFlag = pCtx.bNeedPrefixNalFlag;
    let keSliceType = pCtx.eSliceType;
    let kuiTemporalId = pCtx.uiTemporalId;
    let kiFrameNum = pCtx.param().sDependencyLayers[kiCurDid as usize].iFrameNum;
    let pCurDq = current_layer_mut(pCtx).expect("the frame's current layer is stamped");

    pCurDq.sLayerInfo.iPps = Some(PpsId(iCurPpsId as u16));

    // The null-versus-not that used to select the arm is the tag now — same two
    // arms, same `iCurSpsId`, indexing the same two different arrays.
    pCurDq.sLayerInfo.eSps = Some(if kbUseSubsetSpsFlag {
        LayerSps::Subset(SubsetSpsId(iCurSpsId as u8))
    } else {
        LayerSps::Avc(SpsId(iCurSpsId as u8))
    });

    // **S11.35: the base slice and its copy loop are one stamp loop.** The
    // "base" carried exactly three scalars (`InitSliceHeadWithBase` copies
    // `iPpsId`, `iSpsId`, `bSliceHeaderExtFlag`) and all three were computed
    // right here — so every slice takes the values directly, slice 0 included.
    // The old raw form skipped missing slots and this keeps that (`None` where
    // null answered).
    let mut iIdx = 0;
    while iIdx < iSliceCount {
        if let Some(pSlice) =
            crate::encoder::svc_encode_slice::slice_in_layer_mut(&mut *pCurDq, iIdx)
        {
            pSlice.sSliceHeaderExt.sSliceHeader.iPpsId = iCurPpsId;
            pSlice.sSliceHeaderExt.sSliceHeader.iSpsId = iCurSpsId;
            pSlice.bSliceHeaderExtFlag = kbSliceHeaderExtFlag;
        }
        iIdx += 1;
    }

    // **S6.A1, and Miri found it where the byte sweep could not**: this binding once
    // stood 50 lines up and a whole-layer shared retag popped it. S11.39 makes the
    // fix structural — there is exactly one layer borrow now, and this is a field
    // path of it, so a hoisted form no longer compiles.
    let pNalHdExt = &mut pCurDq.sLayerInfo.sNalHeaderExt;
    // S11.39: `write_bytes(0)` → `Default`, and they agree field-for-field: every
    // default is its type's zero (`NAL_UNIT_UNSPEC_0` is 0), and the emitted
    // stream reads fields, never padding.
    *pNalHdExt = SNalUnitHeaderExt::default();
    pNalHdExt.sNalUnitHeader.uiNalRefIdc = keNalPriority as u8;
    pNalHdExt.sNalUnitHeader.eNalUnitType = keNalType;

    pNalHdExt.uiDependencyId = kiCurDid;
    pNalHdExt.bDiscardableFlag = if kbNeedPrefixNalFlag {
        pNalHdExt.sNalUnitHeader.uiNalRefIdc == EWelsNalRefIdc::NRI_PRI_LOWEST as u8
    } else {
        false
    };
    pNalHdExt.bIdrFlag = (kiFrameNum == 0)
        && (keNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
            || keSliceType == EWelsSliceType::I_SLICE);
    pNalHdExt.uiTemporalId = kuiTemporalId;

    // pEncPic data. **S37, and the resolution sits here rather than at the top of the
    // function on purpose**: the C++ loads both `SPicture*`s at entry and dereferences
    // them only at this line, so resolving here leaves every statement above it
    // running on a path where a picture is not bound — which is what the C++ does,
    // minus the null dereference it would take three lines later.
    let (Some(idEnc), Some(idDec)) = (pCtx.pEncPic, pCtx.pDecPic) else {
        return;
    };
    if pCtx.pVpp.is_none() {
        return;
    }
    current_layer_mut(pCtx).expect("the frame's current layer is stamped").pEncPic =
        Some(idEnc);
    // **S10.7: `pSrcPool`'s stamp is gone with the field**, and **S11.39: the pool
    // local went with it.** The slot-read the local kept (`ctx_src_pool_raw`,
    // deleted) existed so a *stored* pointer could carry the pool's own provenance;
    // nothing stores one any more, so the pool is borrowed per statement through
    // the `Box` — `ctx_vpp_mut`'s route, proved `Some` by the guard above. What
    // each borrow yields is `PicPlanes` by value and a view whose roots come from
    // the plane headers, so no derivation of these borrows outlives its statement.

    let pEncPic = crate::encoder::encoder_context::ctx_vpp_mut(pCtx)
        .m_pSpatialPicPool
        .get_mut(idEnc)
        .planes();
    let pDecPic = pCtx
        .ref_list_mut(kiCurDid as usize)
        .expect("the layer's reference list is allocated")
        .pic_mut(idDec)
        .planes();

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
    let sRecView = crate::encoder::rec_view::RecPicView::build(
        pCtx.ref_list_mut(kiCurDid as usize)
            .expect("the layer's reference list is allocated")
            .pic_mut(idDec),
    );

    // S9.0: the read half of the seam, built beside the write half above and
    // rebuilt every frame for the same reason. `get` and not `get_mut` — a
    // read-only view makes no exclusive claim, which is the whole difference
    // between this and `RecPicView`.
    let sEncView = crate::encoder::rec_view::RoPicView::build(
        crate::encoder::encoder_context::ctx_vpp_ref(pCtx).m_pSpatialPicPool.get(idEnc),
    );

    let pCurDq = current_layer_mut(pCtx).expect("the frame's current layer is stamped");
    pCurDq.pRecView = Some(sRecView);
    pCurDq.pEncView = Some(sEncView);

    // **S10.5: `pEncData`'s three stamps are gone with the field.** They were
    // written here every frame and read by nobody — step 2 moved the last
    // source-plane reader onto `pEncView`, and the last raw one
    // (`AnalysisVaaInfoIntra_c`, through `mb_cursor`) followed in this checkpoint.
    pCurDq.iEncStride[0] = pEncPic.iLineSize[0];
    pCurDq.iEncStride[1] = pEncPic.iLineSize[1];
    pCurDq.iEncStride[2] = pEncPic.iLineSize[2];
    // cs data
    // **S10.6: `pCsData`'s three stamps are gone with the field**, as `pEncData`'s
    // did — the reconstruction seam (`pRecView`, stamped a few lines above) took
    // every reader long ago, and the three that were left were dead bindings.
    pCurDq.iCsStride[0] = pDecPic.iLineSize[0];
    pCurDq.iCsStride[1] = pDecPic.iLineSize[1];
    pCurDq.iCsStride[2] = pDecPic.iLineSize[2];

    pCurDq.bBaseLayerAvailableFlag = pCurDq.pRefLayer.is_some();

    // **T7.B4.** Was `pTaskManage->InitFrame(kiCurDid)`, whose whole body was "if the
    // layer wants re-slicing, dispatch the pre-encoding task list and wait". The task
    // list is gone; the condition and the barrier position are not. The count is the
    // one `CreateTasks` computed for `WELS_ENC_TASK_UPDATEMBMAP`
    // (`sSliceArgument.uiSliceNum` for every non-`SM_SIZELIMITED_SLICE` mode), and
    // only the fixed modes can reach here: `bNeedAdjustingSlicing` is written by
    // `DynamicAdjustSlicing` alone, which only `AdjustBaseLayer`/`AdjustEnhanceLayer`
    // call, and only on the `SM_FIXEDSLCNUM_SLICE` arm.
    if pCtx.pSliceThreading.is_some()
        && !current_layer_ref(pCtx).is_none()
        && current_layer_ref(pCtx).expect("the frame's current layer is stamped").bNeedAdjustingSlicing
    {
        let kiTaskCount = pCtx.param().sSpatialLayers[kiCurDid as usize]
            .sSliceArgument
            .uiSliceNum as i32;
        crate::encoder::slice_multi_threading::UpdateMbMapForked(pCtx, kiTaskCount);
    }
}

/// `encoder_ext.cpp:2954`. Emit the SVC prefix NAL that precedes each VCL NAL when
/// `bNeedPrefixNalFlag` is set.
pub fn AddPrefixNal(
    pCtx: &mut sWelsEncCtx,
    _pLayerBsInfo: &mut SLayerBSInfo,
    pNalLen: *mut i32,
    pNalIdxInLayer: &mut i32,
    keNalType: EWelsNalUnitType,
    keNalRefIdc: EWelsNalRefIdc,
    iPayloadSize: &mut i32,
) -> i32 {
    let mut iReturn;
    *iPayloadSize = 0;

    // S3.B1: per-statement reborrows — see `WelsWriteOneSPS`.
    if keNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST {
        crate::encoder::nal_encap::WelsLoadNal(
            pCtx.pOut.as_deref_mut().expect("pOut lives"),
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            keNalRefIdc as i32,
        );

        {
            let pOut = pCtx.pOut.as_deref_mut().expect("pOut lives");
            crate::encoder::nal_encap::WelsWriteSVCPrefixNal(
                &mut pOut.sBsBuffer[..],
                &mut pOut.sBsWrite,
                keNalRefIdc as i32,
                keNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR,
            );
        }

        crate::encoder::nal_encap::WelsUnloadNal(pCtx.pOut.as_deref_mut().expect("pOut lives"));
    } else {
        // No prefix NAL unit RBSP syntax here, but the NAL unit header extension is
        // still needed.
        crate::encoder::nal_encap::WelsLoadNal(
            pCtx.pOut.as_deref_mut().expect("pOut lives"),
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            keNalRefIdc as i32,
        );
        crate::encoder::nal_encap::WelsUnloadNal(pCtx.pOut.as_deref_mut().expect("pOut lives"));
    }

    // S11.37: a value, not a cursor — `SNalUnitHeaderExt` is `Copy`, the callee
    // only reads it, and a copy survives the context destructure below without
    // borrowing anything (the S3.B1 raw hoist said the same thing in provenance).
    let kNalHeaderExt =
        current_layer_ref(pCtx).expect("the frame's current layer is stamped").sLayerInfo.sNalHeaderExt;
    // **S11.17**: the context is destructured. The NAL entry and the
    // source bytes live in `pOut`; the destination is the tail of
    // `pFrameBs` — disjoint fields, so both borrows are live at once
    // where the argument list would have been two borrows of the whole
    // context. No copy: the source is borrowed, not cloned.
    let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
    let kpOut = pOut.as_deref().expect("pOut lives");
    let kiPos = *iPosBsBuffer as usize;
    let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
    iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &kpOut.sNalList[(kpOut.iNalIndex - 1) as usize],
        &kpOut.sBsBuffer[..],
        Some(&kNalHeaderExt),
        pDstTail,
        // unsafe-cat: C-ABI — the out-array slot (S11.20's family).
        #[allow(unsafe_code)]
        unsafe {
            &mut *pNalLen.add(*pNalIdxInLayer as usize)
        },
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }
    // unsafe-cat: C-ABI
    #[allow(unsafe_code)]
    unsafe {
        *iPayloadSize = *pNalLen.add(*pNalIdxInLayer as usize);
    }

    pCtx.iPosBsBuffer += *iPayloadSize;
    *pNalIdxInLayer += 1;

    iReturn = ENC_RETURN_SUCCESS;
    iReturn
}

/// `encoder_ext.cpp:3003`. Emit a filler-data NAL of `iLen` bytes.
pub fn WritePadding(pCtx: &mut sWelsEncCtx, iLen: i32, iSize: &mut i32) -> i32 {
    let mut iNalLen = 0i32;

    *iSize = 0;
    // S3.B1: the take dance — this body holds two `&mut` cursors into the output
    // block (`buf`, `pBs`) across further calls into it, which no reborrow scheme
    // spells; the box moves out instead, and every return path below the take
    // stores it back first.
    let mut pOut = pCtx.pOut.take().expect("pOut lives");
    let iNal = pOut.iNalIndex;

    // `pEndBuf - pCurBuf < iLen` in comparison form; `iLen` is non-negative here
    // and a `usize` `len - pos` cannot wrap because `pos <= len` always holds for a
    // writer that has not overrun, which the write below would panic on anyway.
    if (pOut.sBsBuffer.len() - pOut.sBsWrite.pos()) < iLen as usize
        || iNal >= pOut.sNalList.len() as i32
    {
        pCtx.pOut = Some(pOut);
        return ENC_RETURN_MEMOVERFLOWFOUND;
    }

    crate::encoder::nal_encap::WelsLoadNal(
        &mut *pOut,
        EWelsNalUnitType::NAL_UNIT_FILLER_DATA as i32,
        EWelsNalRefIdc::NRI_PRI_LOWEST as i32,
    );

    {
        // The frame-level writer, for non-VCL NALs — two disjoint fields of the
        // owned box, which borrowck splits without ceremony now.
        let buf = &mut pOut.sBsBuffer[..];
        let pBs = &mut pOut.sBsWrite;
        for _ in 0..iLen {
            crate::encoder::vlc_encoder::BsWriteBits(buf, pBs, 8, 0xff);
        }

        crate::encoder::vlc_encoder::BsRbspTrailingBits(buf, pBs);
    }

    crate::encoder::nal_encap::WelsUnloadNal(&mut *pOut);

    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &pOut.sNalList[iNal as usize],
        &pOut.sBsBuffer[..],
        None,
        // S11.17: `pOut` is already moved out of the context here, so the tail
        // borrow has nothing to conflict with.
        pCtx.frame_bs_tail_mut(),
        &mut iNalLen,
    );
    pCtx.pOut = Some(pOut);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    pCtx.iPosBsBuffer += iNalLen;
    *iSize += iNalLen;

    ENC_RETURN_SUCCESS
}

/// `encoder_ext.cpp:2624` (`static inline SetFastCodingFunc`).
fn SetFastCodingFunc(pFuncList: &mut SWelsFuncPtrList) {
    pFuncList.pfIntraFineMd =
        Some(crate::encoder::svc_base_layer_md::WelsMdIntraFinePartitionVaa);
    let sdf = &mut pFuncList.sSampleDealingFuncs;
    sdf.pfMdCost = CostFamily::Sad;
    // The C++ also aims three `pfIntra*Combined3` slots at their `*Sad` twins here;
    // both sides were NULL on every target and the fields are deleted (S18).
}

/// `encoder_ext.cpp:2630` (`static inline SetNormalCodingFunc`).
fn SetNormalCodingFunc(pFuncList: &mut SWelsFuncPtrList) {
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
pub fn PreprocessSliceCoding(pCtx: &mut sWelsEncCtx) {
    let pCurLayer = current_layer_ref(pCtx).expect("the frame's current layer is stamped");
    let bFastMode = pCtx.param().iComplexityMode == LOW_COMPLEXITY;
    // **T6.I2**, as `InitFunctionPointers`: one `&mut` derived from the owner, not
    // one per call. This is the function the whole step-1 checker is about — it is
    // where the table is re-written *per frame*, which is why no reader may hold
    // anything derived from it across a call that reaches it again.
    // **§4.6, reorder, and it is the whole of what A6's flip costs at this body.**
    // Every context read below is lifted above the table's `&mut`: the usage
    // type, the slice type, the two layer ids, the NAL priority, the dependency
    // layer's highest temporal id, and the two layer facts `pfInterMd` is chosen
    // from. Nothing moves relative to anything else — none of these fields is
    // written by this body — so it is behaviour-preserving by construction. And
    // the fact that the compiler *demanded* it is F212's point: the table's
    // re-write can no longer coexist with any reader of the context.
    let kiUsageType = pCtx.param().iUsageType;
    let keSliceType = pCtx.eSliceType;
    let kiCurDid = pCtx.uiDependencyId as usize;
    let kiCurTid = pCtx.uiTemporalId as i32;
    let keNalPriority = pCtx.eNalPriority;
    let kiHighestTemporalId =
        pCtx.param().sDependencyLayers[kiCurDid].iHighestTemporalId as i32;
    let kbBaseAvail = pCurLayer.bBaseLayerAvailableFlag;
    let kbHighestSpatial = if pCtx.param_opt().is_some() {
        pCtx.param().iSpatialLayerNum
            == (pCurLayer.sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    // S11.37: the deblock-gate's two layer flags, read out with the rest — the
    // shared layer borrow ends here, before the table's `&mut` below.
    let kbDeblockingParallelFlag = pCurLayer.bDeblockingParallelFlag;
    let kiLoopFilterDisableIdc = pCurLayer.iLoopFilterDisableIdc;

    let fl: &mut SWelsFuncPtrList = pCtx.func_list_mut();

    // function pointers conditional assignment under sWelsEncCtx
    if (kiUsageType == CAMERA_VIDEO_REAL_TIME && bFastMode)
        || (kiUsageType == SCREEN_CONTENT_REAL_TIME
            && keSliceType == EWelsSliceType::P_SLICE
            && bFastMode)
    {
        SetFastCodingFunc(fl);
    } else {
        SetNormalCodingFunc(fl);
    }

    if keSliceType == EWelsSliceType::P_SLICE {
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
    let kbSatdInMd =
        sdf.pfMeCost == CostFamily::Satd && sdf.pfMdCost == CostFamily::Satd;

    if kbDeblockingParallelFlag
        && kiLoopFilterDisableIdc != 1
        // ENABLE_FRAME_DUMP is not defined, so this clause is compiled in.
        && keNalPriority != EWelsNalRefIdc::NRI_PRI_LOWEST
        && (kiHighestTemporalId == 0 || kiCurTid < kiHighestTemporalId)
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
    fl.pfInterMd = if kbBaseAvail && kbHighestSpatial {
        Some(crate::encoder::svc_mode_decision::WelsMdInterMbEnhancelayer)
    } else {
        Some(crate::encoder::svc_base_layer_md::WelsMdInterMb)
    };

    // S11.37: the one layer write, re-derived after the table's `&mut` ends —
    // the value was computed above from the table's final state.
    current_layer_mut(pCtx)
        .expect("the frame's current layer is stamped")
        .bSatdInMdFlag = kbSatdInMd;
}

/// `encoder_ext.cpp:3131`. Write the parameter sets for (simulcast) SVC.
pub fn WriteSsvcParaset(
    pCtx: &mut sWelsEncCtx,
    kiSpatialNum: i32,
    // **S11.20**: the in/out layer cursor becomes the frame plus an index.
    // `*mut *mut SLayerBSInfo` was C's out-parameter idiom for "advance the
    // caller's cursor"; `&mut usize` says the same thing about a bounds-checked
    // position in `pFbi.sLayerInfo`, and the caller's own `iLayerBsIndex` was
    // already tracking exactly this number alongside it.
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iCountNal = 0i32;

    let iReturn = crate::encoder::wels_encoder_ext::WelsWriteParameterSets(
        pCtx,
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte,
        &mut iCountNal,
        &mut iNonVclSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    // S11.37: the write borrow, taken per record — the cursor never crossed a
    // context reach here, so it was never anything but this.
    for iSpatialId in 0..kiSpatialNum as usize {
        let pParamInternal = &mut pCtx.param_mut().sDependencyLayers[iSpatialId];
        if pParamInternal.uiIdrPicId < 65535 {
            pParamInternal.uiIdrPicId += 1;
        } else {
            pParamInternal.uiIdrPicId = 0;
        }
    }

    pFbi.sLayerInfo[*iLbi].uiSpatialId = 0;
    pFbi.sLayerInfo[*iLbi].uiTemporalId = 0;
    pFbi.sLayerInfo[*iLbi].uiQualityId = 0;
    pFbi.sLayerInfo[*iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
    pFbi.sLayerInfo[*iLbi].iNalCount = iCountNal;
    pFbi.sLayerInfo[*iLbi].eFrameType = EVideoFrameType::videoFrameTypeIDR;
    pFbi.sLayerInfo[*iLbi].iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    // point to next pLayerBsInfo
    let kpPrevNalLen = pFbi.sLayerInfo[*iLbi].pNalLengthInByte;
    *iLbi += 1;
    pCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex += 1;
    pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
    // unsafe-cat: C-ABI — the next layer's out-array is the previous one's
    // tail, in the frozen `SFrameBSInfo` the application walks (S11.20).
    #[allow(unsafe_code)]
    unsafe {
        // unsafe-cat: C-ABI — the next layer's out-array is the previous one's
    // tail, in the frozen `SFrameBSInfo` (S11.20).
    #[allow(unsafe_code)]
    unsafe {
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = kpPrevNalLen.add(iCountNal as usize);
    }
    }

    // update for external countings
    *iLayerNum += 1;
    *iFrameSize += iNonVclSize;
    iReturn
}

/// `encoder_ext.cpp:3163`. Write the parameter sets for simulcast AVC.
pub fn WriteSavcParaset(
    pCtx: &mut sWelsEncCtx,
    iIdx: i32,
    // **S11.20**: the in/out layer cursor becomes the frame plus an index.
    // `*mut *mut SLayerBSInfo` was C's out-parameter idiom for "advance the
    // caller's cursor"; `&mut usize` says the same thing about a bounds-checked
    // position in `pFbi.sLayerInfo`, and the caller's own `iLayerBsIndex` was
    // already tracking exactly this number alongside it.
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iNalSize = 0i32;
    let mut iCountNal;

    // --- SPS ---
    // Re-acquired here and again for the PPS below rather than held across the two
    // writes: `WelsWriteOneSPS`/`WelsWriteOnePPS` reach this same object through
    // `pCtx->pFuncList`. T4b.2a.
    // §4.6, reorder: the id is read out of the array before the strategy's `&mut`
    // — two different fields of one context, and the id is a scalar.
    let iId = pCtx.sps_array()[iIdx as usize].uiSpsId;
    if let Some(pStrategy) = pCtx.func_list_mut().pParametersetStrategy.as_mut() {
        pStrategy.Update(iId, PARA_SET_TYPE_AVCSPS as i32);
    }

    let mut iReturn =
        crate::encoder::wels_encoder_ext::WelsWriteOneSPS(pCtx, iIdx, &mut iNalSize);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    // unsafe-cat: C-ABI — the frozen out-array slot (S11.20's family).
    #[allow(unsafe_code)]
    unsafe {
        *pFbi.sLayerInfo[*iLbi].pNalLengthInByte = iNalSize;
    }
    iNonVclSize += iNalSize;
    iCountNal = 1;

    pFbi.sLayerInfo[*iLbi].uiSpatialId = iIdx as u8;
    pFbi.sLayerInfo[*iLbi].uiTemporalId = 0;
    pFbi.sLayerInfo[*iLbi].uiQualityId = 0;
    pFbi.sLayerInfo[*iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
    pFbi.sLayerInfo[*iLbi].iNalCount = iCountNal;
    pFbi.sLayerInfo[*iLbi].eFrameType = EVideoFrameType::videoFrameTypeIDR;
    pFbi.sLayerInfo[*iLbi].iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    let kpPrevNalLen = pFbi.sLayerInfo[*iLbi].pNalLengthInByte;
    *iLbi += 1;
    pCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex += 1;
    pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
    // unsafe-cat: C-ABI — the next layer's out-array is the previous one's
    // tail, in the frozen `SFrameBSInfo` (S11.20).
    #[allow(unsafe_code)]
    unsafe {
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = kpPrevNalLen.add(iCountNal as usize);
    }
    *iLayerNum += 1;

    // --- PPS ---
    iNalSize = 0;
    // §4.6, reorder: the id is read out of the array before the strategy's `&mut`
    // — two different fields of one context, and the id is a scalar.
    let iId = pCtx.pps_array()[iIdx as usize].iPpsId;
    if let Some(pStrategy) = pCtx.func_list_mut().pParametersetStrategy.as_mut() {
        pStrategy.Update(iId, PARA_SET_TYPE_PPS as i32);
    }
    iReturn = crate::encoder::wels_encoder_ext::WelsWriteOnePPS(pCtx, iIdx, &mut iNalSize);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }
    // unsafe-cat: C-ABI — the frozen out-array slot (S11.20's family).
    #[allow(unsafe_code)]
    unsafe {
        *pFbi.sLayerInfo[*iLbi].pNalLengthInByte = iNalSize;
    }
    iNonVclSize += iNalSize;
    iCountNal = 1;

    pFbi.sLayerInfo[*iLbi].uiSpatialId = iIdx as u8;
    pFbi.sLayerInfo[*iLbi].uiTemporalId = 0;
    pFbi.sLayerInfo[*iLbi].uiQualityId = 0;
    pFbi.sLayerInfo[*iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
    pFbi.sLayerInfo[*iLbi].iNalCount = iCountNal;
    pFbi.sLayerInfo[*iLbi].eFrameType = EVideoFrameType::videoFrameTypeIDR;
    pFbi.sLayerInfo[*iLbi].iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    let kpPrevNalLen = pFbi.sLayerInfo[*iLbi].pNalLengthInByte;
    *iLbi += 1;
    pCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex += 1;
    pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
    // unsafe-cat: C-ABI — the next layer's out-array is the previous one's
    // tail, in the frozen `SFrameBSInfo` (S11.20).
    #[allow(unsafe_code)]
    unsafe {
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = kpPrevNalLen.add(iCountNal as usize);
    }
    *iLayerNum += 1;

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
pub fn WriteSavcParaset_Listing(
    pCtx: &mut sWelsEncCtx,
    kiSpatialNum: i32,
    // **S11.20**: the in/out layer cursor becomes the frame plus an index.
    // `*mut *mut SLayerBSInfo` was C's out-parameter idiom for "advance the
    // caller's cursor"; `&mut usize` says the same thing about a bounds-checked
    // position in `pFbi.sLayerInfo`, and the caller's own `iLayerBsIndex` was
    // already tracking exactly this number alongside it.
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iReturn = ENC_RETURN_SUCCESS;

    // --- SPS list, per spatial layer ---
    for iSpatialId in 0..kiSpatialNum {
        // S11.37: the write borrow, taken per record (as `WriteSsvcParaset`).
        let pParamInternal = &mut pCtx.param_mut().sDependencyLayers[iSpatialId as usize];
        if pParamInternal.uiIdrPicId < 65535 {
            pParamInternal.uiIdrPicId += 1;
        } else {
            pParamInternal.uiIdrPicId = 0;
        }

        let mut iCountNal = 0i32;
        for iIdx in 0..pCtx.iSpsNum {
            let mut iNalSize = 0i32;
            iReturn = crate::encoder::wels_encoder_ext::WelsWriteOneSPS(pCtx, iIdx, &mut iNalSize);
            if iReturn != ENC_RETURN_SUCCESS {
                return iReturn;
            }
            // unsafe-cat: C-ABI — the frozen out-array slot (S11.20's family).
            #[allow(unsafe_code)]
            unsafe {
                *pFbi.sLayerInfo[*iLbi].pNalLengthInByte.add(iCountNal as usize) = iNalSize;
            }
            iNonVclSize += iNalSize;
            iCountNal += 1;
        }

        pFbi.sLayerInfo[*iLbi].uiSpatialId = iSpatialId as u8;
        pFbi.sLayerInfo[*iLbi].uiTemporalId = 0;
        pFbi.sLayerInfo[*iLbi].uiQualityId = 0;
        pFbi.sLayerInfo[*iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
        pFbi.sLayerInfo[*iLbi].iNalCount = iCountNal;
        pFbi.sLayerInfo[*iLbi].eFrameType = EVideoFrameType::videoFrameTypeIDR;
        pFbi.sLayerInfo[*iLbi].iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

        let kpPrevNalLen = pFbi.sLayerInfo[*iLbi].pNalLengthInByte;
        *iLbi += 1;
        pCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex += 1;
        pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
        // unsafe-cat: C-ABI — the next layer's out-array is the previous one's
    // tail, in the frozen `SFrameBSInfo` (S11.20).
    #[allow(unsafe_code)]
    unsafe {
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = kpPrevNalLen.add(iCountNal as usize);
    }
        *iLayerNum += 1;
    }

    // --- PPS list, per spatial layer ---
    //
    // `encoder_ext.cpp:3297` — the one `UpdatePpsList` call site the port did not
    // have, because this function did not exist. It is a no-op for four of the five
    // kinds and the whole point of `SPS_PPS_LISTING`.
    {
        // **S7.A3**: the strategy and the PPS list split off one `&mut` context —
        // the call that used to need a raw pointer because the strategy lives
        // inside the context it was being handed.
        let (strategy, pps, pPpsNum) =
            crate::encoder::paraset_strategy::ctx_strategy_and_pps(pCtx);
        strategy.UpdatePpsList(pps, pPpsNum);
    }

    for iSpatialId in 0..kiSpatialNum {
        let mut iCountNal = 0i32;
        for iIdx in 0..pCtx.iPpsNum {
            let mut iNalSize = 0i32;
            iReturn = crate::encoder::wels_encoder_ext::WelsWriteOnePPS(pCtx, iIdx, &mut iNalSize);
            if iReturn != ENC_RETURN_SUCCESS {
                return iReturn;
            }
            // unsafe-cat: C-ABI — the frozen out-array slot (S11.20's family).
            #[allow(unsafe_code)]
            unsafe {
                *pFbi.sLayerInfo[*iLbi].pNalLengthInByte.add(iCountNal as usize) = iNalSize;
            }
            iNonVclSize += iNalSize;
            iCountNal += 1;
        }

        pFbi.sLayerInfo[*iLbi].uiSpatialId = iSpatialId as u8;
        pFbi.sLayerInfo[*iLbi].uiTemporalId = 0;
        pFbi.sLayerInfo[*iLbi].uiQualityId = 0;
        pFbi.sLayerInfo[*iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
        pFbi.sLayerInfo[*iLbi].iNalCount = iCountNal;
        pFbi.sLayerInfo[*iLbi].eFrameType = EVideoFrameType::videoFrameTypeIDR;
        pFbi.sLayerInfo[*iLbi].iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

        let kpPrevNalLen = pFbi.sLayerInfo[*iLbi].pNalLengthInByte;
        *iLbi += 1;
        pCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex += 1;
        pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
        // unsafe-cat: C-ABI — the next layer's out-array is the previous one's
    // tail, in the frozen `SFrameBSInfo` (S11.20).
    #[allow(unsafe_code)]
    unsafe {
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = kpPrevNalLen.add(iCountNal as usize);
    }
        *iLayerNum += 1;
    }


    // to check number of layers / nals / slices dependencies
    if *iLayerNum > MAX_LAYER_NUM_OF_FRAME {
        crate::common::wels_trace::WelsLog(
            pCtx.sLogCtx,
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
pub fn PrepareEncodeFrame(
    pCtx: &mut sWelsEncCtx,
    // **S11.20**: the in/out layer cursor becomes the frame plus an index.
    // `*mut *mut SLayerBSInfo` was C's out-parameter idiom for "advance the
    // caller's cursor"; `&mut usize` says the same thing about a bounds-checked
    // position in `pFbi.sLayerInfo`, and the caller's own `iLayerBsIndex` was
    // already tracking exactly this number alongside it.
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iSpatialNum: i32,
    // S11.37: `&mut` — the out-parameter idiom, stated (the callers pass a
    // stack local's address).
    iCurDid: &mut i8,
    iCurTid: &mut i32,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
    uiTimeStamp: i64,
) -> EVideoFrameType {
    // A7, §4.6 reorder: `bSimulcastAVC` and `uiGopSize` are scalars, and the
    // per-layer cursor is a raw taken where it is used — this body calls back into
    // the context at almost every statement.
    let kbSimulcastAVC = pCtx.param().bSimulcastAVC;
    let kuiGopSize = pCtx.param().uiGopSize as i32;

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
        let pfRc = pCtx.func_list().pfRc;
        if kbSimulcastAVC {
            pfRc.WelsUpdateBufferWhenSkip(pCtx, *iCurDid as i32);
        } else {
            for i in 0..iSpatialNum as usize {
                // T9.G2, with `WelsEncoderEncodeExt`'s: the cursor is gone and the
                // index is read at the use. Hoisted as well — `WelsUpdateBufferWhenSkip`
                // takes the ctx retag and this argument reads through the same ctx.
                let iDid = pCtx.sSpatialIndexMap[i].iDid;
                pfRc.WelsUpdateBufferWhenSkip(pCtx, iDid);
            }
        }
    } else {
        // S11.37: a shared borrow for the read, ending at the semicolon — the
        // held cursor existed to survive the paraset calls below, and the one
        // write after them re-derives instead.
        *iCurTid = {
            let kParamInternal = &pCtx.param().sDependencyLayers[*iCurDid as usize];
            GetTemporalLevel(kParamInternal, kParamInternal.iCodingIndex, kuiGopSize)
        };
        pCtx.uiTemporalId = *iCurTid as u8;

        if eFrameType == EVideoFrameType::videoFrameTypeIDR {
            // write parameter sets bitstream or SEI/SSEI (if any) here
            if (pCtx.param().eSpsPpsIdStrategy as i32
                & EParameterSetStrategy::SPS_LISTING as i32)
                == 0
            {
                if kbSimulcastAVC {
                    pCtx.iEncoderError = WriteSavcParaset(
                        pCtx,
                        *iCurDid as i32,
                        pFbi,
                        iLbi,
                        iLayerNum,
                        iFrameSize,
                    );
                    pCtx.param_mut().sDependencyLayers[*iCurDid as usize].uiIdrPicId += 1;
                } else {
                    pCtx.iEncoderError =
                        WriteSsvcParaset(pCtx, iSpatialNum, pFbi, iLbi, iLayerNum, iFrameSize);
                }
            } else {
                // The three listing strategies, all of them: the C's test is
                // `! (SPS_LISTING & eSpsPpsIdStrategy)`, a bitmask over 0x02/0x03/0x06.
                // This arm was `ENC_RETURN_UNSUPPORTED_PARA` until T8b.B3 (the S48
                // shape, while the strategies were unported).
                pCtx.iEncoderError = WriteSavcParaset_Listing(
                    pCtx,
                    iSpatialNum,
                    pFbi,
                    iLbi,
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
pub fn PicPartitionNumDecision(pCtx: &mut sWelsEncCtx) -> i32 {
    let mut iPartitionNum = 1;
    if pCtx.param().iMultipleThreadIdc > 1 {
        iPartitionNum = pCtx.param().iMultipleThreadIdc as i32;
    }
    iPartitionNum
}

/// `DynslcUpdateMbNeighbourInfoListForAllSlices` — encoder_ext.cpp:2397.
///
/// # Safety
/// `pCurDq` must be live with `sMbDataP` allocated.
pub fn DynslcUpdateMbNeighbourInfoListForAllSlices(pCurDq: &mut SDqLayer) {
    // **S10.3c: no `mb_window`, and no `unsafe`.** This body held the whole layer
    // `&mut` and still went through `mb_window`'s `from_raw_parts_mut` — a mint
    // that exists so *fork* workers can take `&mut` sub-ranges out of a **shared**
    // layer, which is a claim the compiler cannot check and this caller never
    // needed. It is single-threaded: `WelsInitCurrentQBLayerMltslc` reaches it
    // from a `&mut sWelsEncCtx`.
    //
    // What blocked the safe form was borrow *width*, not aliasing: the two walkers
    // took `Option<&SDqLayer>`, so a whole-layer shared borrow sat across the
    // grid's `&mut`. They read `sSliceEncCtx` and nothing else, so narrowing them
    // (S10.3c) lets the two fields be borrowed at once — which is what they always
    // were.
    let SDqLayer { sMbDataP, sSliceEncCtx, .. } = pCurDq;
    let kiMbWidth = sSliceEncCtx.iMbWidth as i32;
    let kiEndMbInSlice = sSliceEncCtx.iMbNumInFrame - 1;
    let mut iIdx = 0i32;
    let dims = sMbDataP.dims();
    let mut mbs = crate::safe::mb_grid::MbWindow::new(
        sMbDataP.as_mut_slice(),
        0,
        dims.mb_width(),
        0,
    );

    loop {
        let uiSliceIdc = crate::encoder::svc_encode_slice::WelsMbToSliceIdc(
            Some(sSliceEncCtx),
            mbs.at(iIdx as usize).iMbXY as i32,
        );
        crate::encoder::svc_encode_slice::UpdateMbNeighbor(
            Some(sSliceEncCtx),
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
/// **S10.3c: safe.** Its one blocker was
/// `DynslcUpdateMbNeighbourInfoListForAllSlices`'s `mb_window` mint, and that
/// went when the neighbour walkers narrowed to the field they read.
pub fn WelsInitCurrentQBLayerMltslc(pCtx: &mut sWelsEncCtx) {
    // pData init
    let Some(pCurDq) = current_layer_mut(pCtx) else {
        return;
    };
    // mb_neighbor
    // T9.E2h's note explained why the layer root had to be minted before the
    // argument's retag: an accessor-minted raw the detector could not see. With
    // `current_layer_mut` there is one borrow and the compiler holds it, so
    // there is no ordering to preserve.
    DynslcUpdateMbNeighbourInfoListForAllSlices(pCurDq);
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
pub fn UpdateSlicepEncCtxWithPartition(pCurDq: &mut SDqLayer, mut iPartitionNum: i32) {
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
        (*pCurDq).LastCodedMbIdxOfPartition[i].store(0, Ordering::Relaxed);
        (*pCurDq).NumSliceCodedOfPartition[i].store(0, Ordering::Relaxed);

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
        (*pCurDq).LastCodedMbIdxOfPartition[i].store(0, Ordering::Relaxed);
        (*pCurDq).NumSliceCodedOfPartition[i].store(0, Ordering::Relaxed);
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
pub fn WelsInitCurrentDlayerMltslc(pCtx: &mut sWelsEncCtx, iPartitionNum: i32) {
    /// `#define byte_complexIMBat26 (60)`, local to this function in the C++.
    const byte_complexIMBat26: u32 = 60;

    // S11.37: the layer's `&mut` is scoped to the one call that writes it; the
    // two scalar reads below re-derive shared, so the context reads between
    // them are free (the raw cursor was the same ordering, unspoken).
    UpdateSlicepEncCtxWithPartition(
        current_layer_mut(pCtx).expect("the frame's current layer is stamped"),
        iPartitionNum,
    );

    if pCtx.eSliceType == EWelsSliceType::I_SLICE {
        // check if uiSliceSizeConstraint too small
        let iCurDid = pCtx.uiDependencyId as usize;
        let mut uiFrmByte: u32;

        if pCtx.param().iRCMode != crate::RCMode::RC_OFF_MODE {
            // RC case
            uiFrmByte = ((pCtx.param().sSpatialLayers[iCurDid].iSpatialBitrate as u32)
                / (pCtx.param().sDependencyLayers[iCurDid].fInputFrameRate as u32))
                >> 3;
        } else {
            // fixed QP case
            let iTtlMbNumInFrame =
                current_layer_ref(pCtx).expect("the frame's current layer is stamped").sSliceEncCtx.iMbNumInFrame;
            let mut iQDeltaTo26 = 26 - pCtx.param().sSpatialLayers[iCurDid].iDLayerQp;

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
        let _uiMiniPacketSize = uiFrmByte
            / current_layer_ref(pCtx).expect("the frame's current layer is stamped").sSliceEncCtx.iMaxSliceNumConstraint as u32;
        // C++ only WelsLogs a warning here when uiSliceSizeConstraint is smaller.
    }

    WelsInitCurrentQBLayerMltslc(pCtx);
}

/// `DynSliceRealloc` — encoder_ext.cpp:4525.
///
/// # Safety
/// `pCtx` must be a context built by [`WelsInitEncoderExt`].
pub fn DynSliceRealloc(
    pCtx: &mut sWelsEncCtx,
    // S11.20: the frame and an index — see `FrameBsRealloc`.
    pFbi: &mut SFrameBSInfo,
    iLbi: usize,
) -> i32 {
    // T9.G6: hoisted — the call takes the context retag and this argument reads
    // through the same context (shape B).
    let iMaxSliceNum = current_layer_ref(pCtx).expect("the frame's current layer is stamped").iMaxSliceNum;
    let mut iRet = crate::encoder::svc_encode_slice::FrameBsRealloc(
        pCtx,
        pFbi,
        iLbi,
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
pub fn WelsCodeOnePicPartition(
    pCtx: &mut sWelsEncCtx,
    // S11.20: the frame and an index — see `FrameBsRealloc`. This body both
    // stamps its layer's fields and hands the pair to `DynSliceRealloc`, which
    // walks *all* layers up to this one; an index is what that walk needs.
    pFbi: &mut SFrameBSInfo,
    iLbi: usize,
    pNalIdxInLayer: &mut i32,
    pLayerSize: &mut i32,
    iFirstMbIdxInPartition: i32,
    iEndMbIdxInPartition: i32,
    iStartSliceIdx: i32,
) -> i32 {
    let uSlcBuffIdx = 0usize;
    let mut iNalIdxInLayer = *pNalIdxInLayer;
    let mut iSliceIdx = iStartSliceIdx;
    let kiSliceStep = pCtx.iActiveThreadsNum as i32;
    let kiPartitionId = (iStartSliceIdx % kiSliceStep) as usize;
    let mut iPartitionBsSize = 0i32;
    let mut iAnyMbLeftInPartition = iEndMbIdxInPartition - iFirstMbIdxInPartition + 1;
    let keNalType = pCtx.eNalType;
    let keNalRefIdc = pCtx.eNalPriority;
    let kbNeedPrefix = pCtx.bNeedPrefixNalFlag;
    let kiSliceIdxStep = pCtx.iActiveThreadsNum as i32;
    let mut iReturn;

    // S11.35: the start slice's stamp is an index into the layer's own bank —
    // `None` is the old null answer.
    {
        let pCurLayer = current_layer_mut(pCtx).expect("the frame's current layer is stamped");
        let Some(pStartSlice) = pCurLayer.sSliceBufferInfo[uSlcBuffIdx]
            .pSliceBuffer
            .get_mut(iStartSliceIdx as usize)
        else {
            return ENC_RETURN_UNEXPECTED;
        };
        pStartSlice.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = iFirstMbIdxInPartition;
    }

    while iAnyMbLeftInPartition > 0 {
        let mut iPayloadSize = 0i32;

        if iSliceIdx
            >= (current_layer_ref(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[uSlcBuffIdx].iMaxSliceNum - kiSliceIdxStep)
        {
            // insufficient memory in pSliceInLayer[]
            if pCtx.iActiveThreadsNum == 1 {
                // only single thread supports re-alloc now
                if DynSliceRealloc(pCtx, pFbi, iLbi) != 0 {
                    return ENC_RETURN_MEMALLOCERR;
                }
            } else if iSliceIdx >= current_layer_ref(pCtx).expect("the frame's current layer is stamped").iMaxSliceNum {
                return ENC_RETURN_MEMALLOCERR;
            }
        }

        if kbNeedPrefix {
            // S11.20: the C-ABI length pointer is read out before the record's
            // `&mut` — one field of it, taken as a value.
            let kpNalLen = pFbi.sLayerInfo[iLbi].pNalLengthInByte;
            iReturn = AddPrefixNal(
                pCtx,
                &mut pFbi.sLayerInfo[iLbi],
                kpNalLen,
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

        crate::encoder::nal_encap::WelsLoadNal(pCtx.pOut.as_deref_mut().expect("pOut lives"), keNalType as i32, keNalRefIdc as i32);
        // **S11.35: the bank leaves the layer for the call** — the same move as
        // the grid and the scratch below, one storage over: taking it lets the
        // current and forward slots come from one `split_at_mut` while `pCtx`
        // stays free for the `&mut` NAL machinery on either side of the call.
        // The realloc arm above ran with the bank in place, as it must.
        let mut sBank = std::mem::take(
            &mut current_layer_mut(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[uSlcBuffIdx],
        );
        let kiCurSlot = iSliceIdx as usize;
        if kiCurSlot >= sBank.pSliceBuffer.len() {
            current_layer_mut(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[uSlcBuffIdx] = sBank;
            return ENC_RETURN_UNEXPECTED;
        }
        let (kpHead, kpTail) = sBank.pSliceBuffer.split_at_mut(kiCurSlot + 1);
        let pCurSlice = &mut kpHead[kiCurSlot];
        pCurSlice.iSliceIdx = iSliceIdx;
        // The forward slot at the old ST index exactly (`iSliceIdx + step`,
        // i.e. `tail[step - 1]`); `None` is the old past-end null.
        let pNextSlice = kpTail.get_mut((kiSliceIdxStep - 1) as usize);

        // T7.C3: the layer-level half of `WelsCodeOneSlice`'s I_SLICE arm, one line
        // above the call it was lifted out of — this path is single-threaded, so the
        // sequence is unchanged.
        crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);

        // **S11.1b: the pOut pair leaves the context for the call — safe now.**
        // The buffer is taken (a `Vec` move, no bytes) and the writer read out
        // (`BsWriter` is `Copy`); both are restored immediately after the call,
        // before the error check, so every exit path sees them back. Nothing in
        // the chain reaches `pOut` through the context any more (S11.1a threaded
        // the pair), which is what makes the momentary absence invisible — and
        // what S11.1a's raw hoist could only assert, the borrow checker now
        // proves: the chain's `&pCtx` coexists with `&mut` locals, not with
        // `&mut` context fields.
        let pOutRef = pCtx.pOut.as_deref_mut().expect("pOut lives");
        let mut vOutBsBuf = std::mem::take(&mut pOutRef.sBsBuffer);
        let mut sOutBsWrite = pOutRef.sBsWrite;
        let mut pCtxOutBs: Option<&mut crate::encoder::vlc_encoder::BsWriter> = Some(&mut sOutBsWrite);
        // **S11.27: the macroblock grid leaves the layer for the call**, exactly
        // as the `pOut` pair above does and for the same reason: the chain takes
        // `&sWelsEncCtx`, so a `&mut` to a field inside it cannot span the call.
        // `MbArray::empty()` is a `Vec::new()` — the swap moves two pointers and
        // no records — and the restore precedes the error check, so every exit
        // path sees the grid back.
        let mut sMbData = std::mem::replace(
            &mut current_layer_mut(pCtx).expect("the frame's current layer is stamped").sMbDataP,
            crate::safe::mb_grid::MbArray::empty(),
        );
        let mut sMbWindow = crate::safe::mb_grid::MbWindow::whole(&mut sMbData, 0);
        // S11.30: the CABAC restore scratch, taken beside the grid — partition
        // 0 is the only one a single-threaded encode names (`kiSliceIdx %
        // iActiveThreadsNum` with one thread). Empty means never allocated for
        // this configuration — the old null.
        let mut vRestoreBuf = std::mem::take(&mut pCtx.pDynamicBsBuffer[0]);
        let pRestoreBuf =
            if vRestoreBuf.is_empty() { None } else { Some(vRestoreBuf.as_mut_slice()) };
        iReturn = crate::encoder::svc_encode_slice::WelsCodeOneSlice(
            pCtx,
            &mut *pCurSlice,
            keNalType as i32,
            vOutBsBuf.as_mut_slice(),
            &mut pCtxOutBs,
            &mut sMbWindow,
            pRestoreBuf,
            pNextSlice,
        );
        pCtx.pDynamicBsBuffer[0] = vRestoreBuf;
        drop(sMbWindow);
        current_layer_mut(pCtx).expect("the frame's current layer is stamped").sMbDataP = sMbData;
        // S11.35: the bank goes back before the error check, like everything
        // taken — the boundary's forward write (if the limit fired) rides in it.
        current_layer_mut(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[uSlcBuffIdx] = sBank;
        let pOutRef = pCtx.pOut.as_deref_mut().expect("pOut lives");
        pOutRef.sBsBuffer = vOutBsBuf;
        pOutRef.sBsWrite = sOutBsWrite;
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        crate::encoder::nal_encap::WelsUnloadNal(pCtx.pOut.as_deref_mut().expect("pOut lives"));

        // S11.37: a value, not a cursor — `SNalUnitHeaderExt` is `Copy`, the
        // callee only reads it, and a copy survives the context destructure
        // with no borrow (the S3.B1 raw hoist said this in provenance).
        let kNalHeaderExt =
            current_layer_ref(pCtx).expect("the frame's current layer is stamped").sLayerInfo.sNalHeaderExt;
        // **S11.17**: the context is destructured. The NAL entry and the
        // source bytes live in `pOut`; the destination is the tail of
        // `pFrameBs` — disjoint fields, so both borrows are live at once
        // where the argument list would have been two borrows of the whole
        // context. No copy: the source is borrowed, not cloned.
        let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
        let kpOut = pOut.as_deref().expect("pOut lives");
        let kiPos = *iPosBsBuffer as usize;
        let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
        iReturn = crate::encoder::nal_encap::WelsEncodeNal(
            &kpOut.sNalList[(kpOut.iNalIndex - 1) as usize],
            &kpOut.sBsBuffer[..],
            Some(&kNalHeaderExt),
            pDstTail,
            // unsafe-cat: C-ABI — the out-array slot (S11.20's family).
            #[allow(unsafe_code)]
            unsafe {
                &mut *pFbi.sLayerInfo[iLbi].pNalLengthInByte.add(iNalIdxInLayer as usize)
            },
        );
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        // unsafe-cat: C-ABI
        #[allow(unsafe_code)]
        let iSliceSize = unsafe { *pFbi.sLayerInfo[iLbi].pNalLengthInByte.add(iNalIdxInLayer as usize) };

        pCtx.iPosBsBuffer += iSliceSize;
        iPartitionBsSize += iSliceSize;

        iNalIdxInLayer += 1;
        iSliceIdx += kiSliceStep; // iSliceIdx is not contiguous
        iAnyMbLeftInPartition = iEndMbIdxInPartition
            - current_layer_ref(pCtx).expect("the frame's current layer is stamped").LastCodedMbIdxOfPartition[kiPartitionId].load(Ordering::Relaxed);
    }

    *pLayerSize = iPartitionBsSize;
    *pNalIdxInLayer = iNalIdxInLayer;

    // slice based packing???
    pFbi.sLayerInfo[iLbi].uiLayerType = VIDEO_CODING_LAYER;
    pFbi.sLayerInfo[iLbi].uiSpatialId = pCtx.uiDependencyId as u8;
    pFbi.sLayerInfo[iLbi].uiTemporalId = pCtx.uiTemporalId as u8;
    pFbi.sLayerInfo[iLbi].uiQualityId = 0;
    pFbi.sLayerInfo[iLbi].iNalCount = iNalIdxInLayer;
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
///
/// **T9.H3 — the context arrives as `&mut`, and the null guard moved to the
/// caller.** This body opened with `if pCtx.is_null() { return
/// ENC_RETURN_MEMALLOCERR; }`, which a `&mut sWelsEncCtx` cannot express. The
/// condition it tested — `CWelsH264SVCEncoder::m_pEncContext` unset — is still
/// expressible one frame up, and `EncodeFrameInternal` now tests it there and
/// reproduces this function's whole null path (`WelsUninitEncoderExt(take())`
/// then `cmMallocMemeError`) rather than just its return code. The guard was not
/// deleted; it was moved to the last place a null still exists.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsEncoderEncodeExt(
    pCtx: &mut sWelsEncCtx,
    // S11.20: the frame bitstream info by reference — its one caller
    // (`EncodeFrameInternal`) already holds `&mut SFrameBSInfo`.
    pFbi: &mut SFrameBSInfo,
    pSrcPic: *const SSourcePicture,
) -> i32 {
    // **T9.H3's re-stamp of `CWelsPreProcess::m_pEncCtx` stood here — gone at
    // T9.H2, because the field is gone (F192).**
    //
    // H installed it as the *interim* remedy and said so at the time: "the end
    // state is to delete the field and pass the context to its five readers; that
    // is a larger edit than a root stage should carry". That edit has landed. The
    // five sites were four methods, all of them screen-content, and each takes
    // `pCtx: &mut sWelsEncCtx` now as ten of their siblings already did.
    //
    // Re-deriving the copy from the live borrow was the right call for a root
    // stage and it was **not sufficient**: re-stamping makes the stored raw a
    // *child* of the borrow rather than a sibling, which survives the retag — but
    // it does not survive the *protector*. A reference function argument is
    // strongly protected for the duration of the call, so a read through any other
    // tag into that allocation is refused however the tag was derived. Miri says so
    // in one line, and F192 quotes it. Deleting the field is what actually closes
    // it, because it removes the second route rather than blessing it.
    // A7, §4.6 reorder: the frame-rate read is a scalar and every other use of the
    // parameter block in this body is at a statement of its own, so nothing has to
    // be held across the frame loop's context writes.
    let fFrameRateHighest = {
        let p = pCtx.param();
        p.sSpatialLayers[p.iSpatialLayerNum as usize - 1].fFrameRate
    };
    // The reconstruction picture the PSNR block measures, **as a handle** — T9.B3.
    // It was `Option<PicPlanes>`, three raw plane roots copied out of the picture
    // six hundred lines above their only reader; it is now the handle those roots
    // were derived from, and `LayerPlanePsnr` resolves the picture where it reads
    // it. The source picture beside it was the same shape and is gone entirely —
    // `idEncPic`, a local of the layer body, already names it.
    //
    // **The snapshot itself is load-bearing and stays** (F109). `pCtx.pDecPic`
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

    pCtx.iEncoderError = ENC_RETURN_SUCCESS;
    pCtx.bCurFrameMarkedAsSceneLtr = false;
    pFbi.eFrameType = EVideoFrameType::videoFrameTypeSkip;
    pFbi.iLayerNum = 0; // for initialization
    pFbi.uiTimeStamp = crate::encoder::rc::GetTimestampForRc(
        (*pSrcPic).uiTimeStamp,
        pCtx.uiLastTimestamp,
        fFrameRateHighest,
    );
    for iNalIdx in 0..MAX_LAYER_NUM_OF_FRAME as usize {
        pFbi.sLayerInfo[iNalIdx].eFrameType = EVideoFrameType::videoFrameTypeSkip;
        pFbi.sLayerInfo[iNalIdx].iNalCount = 0;
    }

    // Derived after the reset loop above, for the reason `pSpatialIndexMap` used to
    // be derived after `BuildSpatialPicList` (T9.G2 retired that binding): the loop
    // above **writes**
    // `pFbi.sLayerInfo[..]` through `pFbi`, and a write through the parent pops
    // a child taken before it. Every use of this cursor is below.
    // T9.E7: `addr_of_mut!`, not `as_mut_ptr()` — the array method autorefs
    // `&mut pFbi.sLayerInfo` first, so the old mint was a raw ABOVE a Unique,
    // and any sibling raw's write into an entry (the size-limited branch's
    // `pLbi` stamps below) popped it before `SliceLayerInfoUpdate` wrote back
    // through it. A place projection reuses `pFbi`'s provenance; the two mints
    // are then raw siblings, which writes do not pop (T5.O8, F70).
    // **S11.20: an index, not a cursor.** This was a raw walked with `.add(1)`
    // over `pFbi.sLayerInfo`, and the encoder *already* tracked the same
    // position in `pOut.iLayerBsIndex`, incremented beside every advance — two
    // representations of one number, one of them unchecked. The index is the
    // survivor; `sLayerInfo` is a fixed-size array, so every access is bounds-
    // checked where the cursor was checked against nothing.
    let mut iLbi: usize = 0;

    // perform csc/denoise/downsample/padding, generate spatial layers
    // (S3.B1: the take dance — see `WelsInitEncoderExt`; `BuildSpatialPicList`
    // provably makes no cross-file call that could read the empty slot.)
    let iRet = crate::encoder::encoder_context::with_vpp(pCtx, |pVpp, pCtx| {
        pVpp.BuildSpatialPicList(pCtx, pSrcPic, &mut iSpatialNum)
    });
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    pCtx.func_list()
        .pfRc
        .WelsUpdateMaxBrWindowStatus(pCtx, iSpatialNum, pFbi.uiTimeStamp);

    if iSpatialNum < 1 {
        for iDidIdx in 0..pCtx.param().iSpatialLayerNum as usize {
            pCtx.param_mut().sDependencyLayers[iDidIdx].iCodingIndex += 1;
        }
        pFbi.eFrameType = EVideoFrameType::videoFrameTypeSkip;
        pFbi.sLayerInfo[iLbi].eFrameType = EVideoFrameType::videoFrameTypeSkip;
        return ENC_RETURN_SUCCESS;
    }

    // **`pSpatialIndexMap` stood here — T9.G2, the largest single item in the ctx
    // hazard campaign.** It was `pCtx.sSpatialIndexMap.as_ptr()`, held from here
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
    pFbi.sLayerInfo[iLbi].pBsBuf = pCtx.frame_bs();
    pFbi.sLayerInfo[iLbi].pNalLengthInByte = pCtx.pOut.as_deref_mut().expect("pOut lives").sNalLen.as_mut_ptr();
    iCurDid = pCtx.sSpatialIndexMap[0].iDid as i8;
    set_current_layer(pCtx, Some(LayerIdx(iCurDid as u8)));
    current_layer_mut(pCtx).expect("the frame's current layer is stamped").pRefLayer = None;

    if !pCtx.param().bSimulcastAVC {
        eFrameType = PrepareEncodeFrame(pCtx,
            pFbi,
            &mut iLbi,
            iSpatialNum,
            &mut iCurDid,
            &mut iCurTid,
            &mut iLayerNum,
            &mut iFrameSize,
            pFbi.uiTimeStamp,
        );
        if eFrameType == EVideoFrameType::videoFrameTypeSkip {
            pFbi.eFrameType = EVideoFrameType::videoFrameTypeSkip;
            pFbi.sLayerInfo[iLbi].eFrameType = EVideoFrameType::videoFrameTypeSkip;
            return ENC_RETURN_SUCCESS;
        }
        if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
            return pCtx.iEncoderError;
        }
    } else {
        for iDidIdx in 0..pCtx.param().iSpatialLayerNum as usize {
            let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[iDidIdx]);
            let iTemporalId = GetTemporalLevel(
                &*pParamInternal,
                (*pParamInternal).iCodingIndex,
                pCtx.param().uiGopSize as i32,
            );
            if iTemporalId == INVALID_TEMPORAL_ID as i32 {
                (*pParamInternal).iCodingIndex += 1;
            }
        }
    }

    while iSpatialIdx < iSpatialNum {
        iCurDid = pCtx.sSpatialIndexMap[iSpatialIdx as usize].iDid as i8;
        // S29 / F13's family (the encode probe's sixth red, session B): `addr_of_mut!`
        // on the element — `as_mut_ptr().add()` reborrowed the whole array, and the
        // `.iPOC` reads below re-derived it and popped these.
        let pParam: *mut SSpatialLayerConfig =
            std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sSpatialLayers[iCurDid as usize]);
        let pParamInternal =
            std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[iCurDid as usize]);
        let iDecompositionStages = (*pParamInternal).iDecompositionStages as i32;
        set_current_layer(pCtx, Some(LayerIdx(iCurDid as u8)));
        pCtx.uiDependencyId = iCurDid as u8;

        if pCtx.param().bSimulcastAVC {
            eFrameType = PrepareEncodeFrame(pCtx,
                pFbi,
                &mut iLbi,
                iSpatialNum,
                &mut iCurDid,
                &mut iCurTid,
                &mut iLayerNum,
                &mut iFrameSize,
                pFbi.uiTimeStamp,
            );
            if eFrameType == EVideoFrameType::videoFrameTypeSkip {
                pFbi.sLayerInfo[iLbi].eFrameType = EVideoFrameType::videoFrameTypeSkip;
                iSpatialIdx += 1;
                continue;
            }
        }
        crate::encoder::encoder_context::InitFrameCoding(pCtx, eFrameType, iCurDid as i32);
        crate::encoder::encoder_context::with_vpp(pCtx, |pVpp, pCtx| {
            pVpp.AnalyzeSpatialPic(pCtx, iCurDid as i32)
        });

        // **`iPOC` is read at each use below rather than through a held pointer.**
        // Every call in this loop — `InitFrameCoding`, `AnalyzeSpatialPic`,
        // `BuildRefList` — writes this record through its own derivation, and a
        // write through the parent kills a pointer taken before it. No spelling
        // rescues that (S29's boundary clause); only ordering does, and deriving at
        // the use is the ordering that holds however the calls are rearranged. The
        // binding above stays correct for `iDecompositionStages`, read before any
        // of them. Found by the encoder aliasing probe, Phase 6 session A.
        let idEncPic = pCtx.sSpatialIndexMap[iSpatialIdx as usize]
            .pSrc
            .expect("the spatial index map names a live source picture");
        pCtx.pEncPic = Some(idEncPic);
        {
            // S3.B1: the two context reads are copied out first — `p` borrows the
            // context for as long as it lives now that the pool is reached through
            // the owned box, and borrowck orders the block accordingly.
            let kiPictureType = pCtx.eSliceType as i32;
            let kiFramePoc = pCtx.param().sDependencyLayers[iCurDid as usize].iPOC;
            let p = crate::encoder::encoder_context::ctx_vpp_mut(pCtx)
                .m_pSpatialPicPool
                .get_mut(idEncPic);
            p.iPictureType = kiPictureType;
            p.iFramePoc = kiFramePoc;
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
                if pCtx.param().iMultipleThreadIdc > 1
                    && pCtx.param().bUseLoadBalancing
                    && pCtx.param().iMultipleThreadIdc
                        >= pCtx.param().sSpatialLayers[iCurDid as usize]
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
                pCtx.iActiveThreadsNum = iPicIPartitionNum as i16;
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
        let bAvcBased = pCtx.param().bSimulcastAVC || (iCurDid as i32) == BASE_DEPENDENCY_ID;
        pCtx.bNeedPrefixNalFlag = !pCtx.param().bSimulcastAVC
            && bAvcBased
            && (pCtx.param().bPrefixNalAddingCtrl || pCtx.param().iSpatialLayerNum > 1);

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
        if iCurTid == 0 || pCtx.eSliceType == EWelsSliceType::I_SLICE {
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
        pCtx.eNalType = eNalType;
        pCtx.eNalPriority = eNalRefIdc;

        // §4.6, reorder: the next buffer and the slice type come out first, so
        // the context write and the picture write do not want the context at once.
        let eSliceTypeNow = pCtx.eSliceType;
        let idNext = pCtx
            .ref_list(iCurDid as usize)
            .expect("the dependency layer's reference list")
            .pNextBuffer;
        pCtx.pDecPic = idNext;
        fsnr = idNext;
        if let Some(id) = fsnr {
            let iPOC = pCtx.param().sDependencyLayers[iCurDid as usize].iPOC;
            let p = pCtx
                .ref_list_mut(iCurDid as usize)
                .expect("the dependency layer's reference list")
                .pic_mut(id);
            p.iPictureType = eSliceTypeNow as i32;
            p.iFramePoc = iPOC;
        }

        WelsInitCurrentLayer(pCtx, iCurWidth, iCurHeight);

        let eRefStrategy = pCtx.eRefStrategy;
        eRefStrategy.MarkPic(pCtx);
        if !eRefStrategy.BuildRefList(pCtx, pCtx.param().sDependencyLayers[iCurDid as usize].iPOC, 0) {
            eFrameType = EVideoFrameType::videoFrameTypeIDR;
            pCtx.iEncoderError = ENC_RETURN_CORRECTED;
            break;
        }
        if pCtx.eSliceType != EWelsSliceType::I_SLICE {
            eRefStrategy.AfterBuildRefList(pCtx);
        }

        if pCtx.param().iRCMode != RC_OFF_MODE {
            let pRef = if pCtx.eSliceType == EWelsSliceType::P_SLICE && pCtx.iNumRef0 > 0 {
                pCtx.pRefList0[0]
            } else {
                None
            };
            // T9.G6: hoisted — the call takes the context retag and these arguments
            // read through the same context (shape B).
            let idEncPicForVaa = pCtx.pEncPic;
            let bBgd = pCtx.eSliceType == EWelsSliceType::P_SLICE
                && pCtx.param().bEnableBackgroundDetection;
            crate::encoder::encoder_context::with_vpp(pCtx, |pVpp, pCtx| {
                pVpp.AnalyzePictureComplexity(pCtx, idEncPicForVaa, pRef, iCurDid as i32, bBgd)
            });
        }
        // get reordering syntax used for writing the slice header
        crate::encoder::ref_list_mgr_svc::WelsUpdateRefSyntax(pCtx,
            pCtx.param().sDependencyLayers[iCurDid as usize].iPOC,
            eFrameType as i32,
        );
        // update reference picture for the current DQ layer
        PrefetchReferencePicture(pCtx, eFrameType);
        pCtx.func_list()
            .pfRc
            .WelsRcPictureInit(pCtx, pFbi.uiTimeStamp);
        // MUST be called after pfWelsRcPictureInit() and WelsInitCurrentLayer()
        PreprocessSliceCoding(pCtx);

        iLayerSize = 0;
        if (*pParam).sSliceArgument.uiSliceMode == SM_SINGLE_SLICE {
            // only one slice within a quality layer
            let mut iPayloadSize = 0i32;
            // **S11.35: the bank leaves the layer for the slice's span** — the
            // raw resolved slot 0 and held it across the prefix NAL, the
            // boundary stamp and the whole coding call; the take makes those
            // coexistences ownership facts, and `pCtx` stays free throughout.
            let mut sBank = std::mem::take(
                &mut current_layer_mut(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[0],
            );
            let pCurSlice = sBank
                .pSliceBuffer
                .get_mut(0)
                .expect("the single-slice bank holds slot 0");

            if pCtx.bNeedPrefixNalFlag {
                // S11.20: the C-ABI length pointer is read out before the record's
                // `&mut` — one field of it, taken as a value.
                let kpNalLen = pFbi.sLayerInfo[iLbi].pNalLengthInByte;
                pCtx.iEncoderError = AddPrefixNal(pCtx,
                    &mut pFbi.sLayerInfo[iLbi],
                    kpNalLen,
                    &mut iNalIdxInLayer,
                    eNalType,
                    eNalRefIdc,
                    &mut iPayloadSize,
                );
                if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                    return pCtx.iEncoderError;
                }
                iLayerSize += iPayloadSize;
            }

            crate::encoder::nal_encap::WelsLoadNal(
                pCtx.pOut.as_deref_mut().expect("pOut lives"),
                eNalType as i32,
                eNalRefIdc as i32,
            );
            debug_assert_eq!(0, (*pCurSlice).iSliceIdx);
            pCtx.iEncoderError = crate::encoder::svc_encode_slice::SetSliceBoundaryInfo(
                current_layer_ref(pCtx),
                &mut *pCurSlice,
                0,
            );
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }

            // T7.C3, as above.
            crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);
            // **S11.1b: the pOut pair leaves the context for the call — safe now.**
            // The buffer is taken (a `Vec` move, no bytes) and the writer read out
            // (`BsWriter` is `Copy`); both are restored immediately after the call,
            // before the error check, so every exit path sees them back. Nothing in
            // the chain reaches `pOut` through the context any more (S11.1a threaded
            // the pair), which is what makes the momentary absence invisible — and
            // what S11.1a's raw hoist could only assert, the borrow checker now
            // proves: the chain's `&pCtx` coexists with `&mut` locals, not with
            // `&mut` context fields.
            let pOutRef = pCtx.pOut.as_deref_mut().expect("pOut lives");
            let mut vOutBsBuf = std::mem::take(&mut pOutRef.sBsBuffer);
            let mut sOutBsWrite = pOutRef.sBsWrite;
            let mut pCtxOutBs: Option<&mut crate::encoder::vlc_encoder::BsWriter> = Some(&mut sOutBsWrite);
            // **S11.27: the macroblock grid leaves the layer for the call**, exactly
            // as the `pOut` pair above does and for the same reason: the chain takes
            // `&sWelsEncCtx`, so a `&mut` to a field inside it cannot span the call.
            // `MbArray::empty()` is a `Vec::new()` — the swap moves two pointers and
            // no records — and the restore precedes the error check, so every exit
            // path sees the grid back.
            let mut sMbData = std::mem::replace(
                &mut current_layer_mut(pCtx).expect("the frame's current layer is stamped").sMbDataP,
                crate::safe::mb_grid::MbArray::empty(),
            );
            let mut sMbWindow = crate::safe::mb_grid::MbWindow::whole(&mut sMbData, 0);
            // S11.30: the CABAC restore scratch, taken beside the grid — partition
            // 0 is the only one a single-threaded encode names (`kiSliceIdx %
            // iActiveThreadsNum` with one thread). Empty means never allocated for
            // this configuration — the old null.
            let mut vRestoreBuf = std::mem::take(&mut pCtx.pDynamicBsBuffer[0]);
            let pRestoreBuf =
                if vRestoreBuf.is_empty() { None } else { Some(vRestoreBuf.as_mut_slice()) };
            // S11.33: single-slice — the dynamic boundary never fires.
            let iCodeRet =
                crate::encoder::svc_encode_slice::WelsCodeOneSlice(pCtx, &mut *pCurSlice, eNalType as i32, vOutBsBuf.as_mut_slice(), &mut pCtxOutBs, &mut sMbWindow, pRestoreBuf, None);
            pCtx.pDynamicBsBuffer[0] = vRestoreBuf;
            drop(sMbWindow);
            current_layer_mut(pCtx).expect("the frame's current layer is stamped").sMbDataP = sMbData;
            // S11.35: the bank goes back before the error check, like the rest.
            current_layer_mut(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[0] = sBank;
            let pOutRef = pCtx.pOut.as_deref_mut().expect("pOut lives");
            pOutRef.sBsBuffer = vOutBsBuf;
            pOutRef.sBsWrite = sOutBsWrite;
            pCtx.iEncoderError = iCodeRet;
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }

            crate::encoder::nal_encap::WelsUnloadNal(pCtx.pOut.as_deref_mut().expect("pOut lives"));

            // S11.37: a value, not a cursor — `SNalUnitHeaderExt` is `Copy`, the
            // callee only reads it, and a copy survives the context destructure
            // with no borrow (the S3.B1 raw hoist said this in provenance).
            let kNalHeaderExt =
                current_layer_ref(pCtx).expect("the frame's current layer is stamped").sLayerInfo.sNalHeaderExt;
            // **S11.17**: the context is destructured. The NAL entry and the
            // source bytes live in `pOut`; the destination is the tail of
            // `pFrameBs` — disjoint fields, so both borrows are live at once
            // where the argument list would have been two borrows of the whole
            // context. No copy: the source is borrowed, not cloned.
            let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
            let kpOut = pOut.as_deref().expect("pOut lives");
            let kiPos = *iPosBsBuffer as usize;
            let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
            pCtx.iEncoderError = crate::encoder::nal_encap::WelsEncodeNal(
                &kpOut.sNalList[kpOut.iNalIndex as usize - 1],
                &kpOut.sBsBuffer[..],
                Some(&kNalHeaderExt),
                pDstTail,
                &mut *pFbi.sLayerInfo[iLbi].pNalLengthInByte.add(iNalIdxInLayer as usize),
            );
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }
            let iSliceSize = *pFbi.sLayerInfo[iLbi].pNalLengthInByte.add(iNalIdxInLayer as usize);

            iLayerSize += iSliceSize;
            pCtx.iPosBsBuffer += iSliceSize;
            iNalIdxInLayer += 1;
            pFbi.sLayerInfo[iLbi].uiLayerType = VIDEO_CODING_LAYER;
            pFbi.sLayerInfo[iLbi].uiSpatialId = iCurDid as u8;
            pFbi.sLayerInfo[iLbi].uiTemporalId = iCurTid as u8;
            pFbi.sLayerInfo[iLbi].uiQualityId = 0;
            pFbi.sLayerInfo[iLbi].iNalCount = iNalIdxInLayer;
            pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
            pFbi.sLayerInfo[iLbi].iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
        } else if (*pParam).sSliceArgument.uiSliceMode == SM_SIZELIMITED_SLICE
            && pCtx.param().iMultipleThreadIdc <= 1
        {
            // dynamic slicing, single threading
            let kiLastMbInFrame = current_layer_ref(pCtx).expect("the frame's current layer is stamped").sSliceEncCtx.iMbNumInFrame;
            pCtx.iEncoderError = WelsCodeOnePicPartition(pCtx,
                pFbi,
                iLbi,
                &mut iNalIdxInLayer,
                &mut iLayerSize,
                0,
                kiLastMbInFrame - 1,
                0,
            );
            pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
            pFbi.sLayerInfo[iLbi].iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }
        } else if (*pParam).sSliceArgument.uiSliceMode != SM_SIZELIMITED_SLICE
            && pCtx.param().iMultipleThreadIdc > 1
        {
            // THREAD_FULLY_FIRE_MODE/THREAD_PICK_UP_MODE for any mode of
            // non-SM_SIZELIMITED_SLICE
            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(current_layer_mut(pCtx).expect("the frame's current layer is stamped"));
            if iLayerNum + 1 >= MAX_LAYER_NUM_OF_FRAME as i32 {
                // check available layer_bs_info for further writing as followed
                return ENC_RETURN_UNSUPPORTED_PARA;
            }
            if iSliceCount <= 1 {
                return ENC_RETURN_UNEXPECTED;
            }
            //note: the old codes are removed at commit: 3e0ee69
            pFbi.sLayerInfo[iLbi].pBsBuf = pCtx.frame_bs_cur();
            pFbi.sLayerInfo[iLbi].uiLayerType = VIDEO_CODING_LAYER;
            pFbi.sLayerInfo[iLbi].uiSpatialId = pCtx.uiDependencyId;
            pFbi.sLayerInfo[iLbi].uiTemporalId = pCtx.uiTemporalId;
            pFbi.sLayerInfo[iLbi].uiQualityId = 0;
            pFbi.sLayerInfo[iLbi].iNalCount = 0;
            pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
            pFbi.sLayerInfo[iLbi].iSubSeqId = GetSubSequenceId(pCtx, eFrameType);

            // **T7.B1 — the fork/join.** This was
            // `pTaskManage->ExecuteTasks(WELS_ENC_TASK_ENCODING)`: `iSliceCount`
            // heap tasks pushed through the shared pool, each claiming a bs slot
            // under a mutex, joined by a `Mutex<i32>` + `Condvar` barrier. It is now
            // `std::thread::scope` over one job per bs slot; the join is the
            // barrier and the slot claim is the partition. `FinishTask` ORed each
            // task's result into `iEncoderError` under `mutexEncoderError`; the
            // results come back through the join instead and are ORed here, in the
            // same field, one line above the same check.
            pCtx.iEncoderError |=
                crate::encoder::slice_multi_threading::EncodeFixedSlicesForked(pCtx, iSliceCount);
            if pCtx.iEncoderError != 0 {
                return pCtx.iEncoderError;
            }

            iLayerSize = crate::encoder::slice_multi_threading::AppendSliceToFrameBs(pCtx,
                &mut pFbi.sLayerInfo[iLbi],
                iSliceCount,
            );
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }
        } else if (*pParam).sSliceArgument.uiSliceMode == SM_SIZELIMITED_SLICE
            && pCtx.param().iMultipleThreadIdc > 1
        {
            // THREAD_FULLY_FIRE_MODE && SM_SIZELIMITED_SLICE
            let kiPartitionCnt = pCtx.iActiveThreadsNum as i32;

            //TODO: use a function to remove duplicate code here and ln3994
            let iLayerBsIdx = pCtx.pOut.as_deref().expect("pOut lives").iLayerBsIndex;
            // **T9.E6, the mid-row probe's verdict once round 5 stopped aborting
            // first**: this was `&mut pFbi.sLayerInfo[..] as *mut` — a `&mut`
            // element borrow whose Unique retag popped `pLayerBsInfo` (the raw
            // over the whole array, minted at the top of this function) for the
            // element's bytes, and `SliceLayerInfoUpdate` writes through
            // `pLayerBsInfo` right below. S29's spelling reuses the parent's
            // provenance and pops nothing — F70's rule, F114a's shape.
            let pLbi = std::ptr::addr_of_mut!(pFbi.sLayerInfo[iLayerBsIdx as usize]);
            (*pLbi).pBsBuf = pCtx.frame_bs_cur();
            (*pLbi).uiLayerType = VIDEO_CODING_LAYER;
            (*pLbi).uiSpatialId = pCtx.uiDependencyId;
            (*pLbi).uiTemporalId = pCtx.uiTemporalId;
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
            pCtx.iEncoderError |=
                crate::encoder::slice_multi_threading::EncodeSizeLimitedSlicesForked(pCtx,
                    kiPartitionCnt,
                );

            if pCtx.iEncoderError != 0 {
                return pCtx.iEncoderError;
            }

            iRet = crate::encoder::svc_encode_slice::SliceLayerInfoUpdate(
                pCtx,
                pFbi,
                iLbi,
                (*pParam).sSliceArgument.uiSliceMode,
            );
            if iRet != 0 {
                return ENC_RETURN_UNEXPECTED;
            }

            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(current_layer_mut(pCtx).expect("the frame's current layer is stamped"));
            iLayerSize = crate::encoder::slice_multi_threading::AppendSliceToFrameBs(pCtx,
                &mut pFbi.sLayerInfo[iLbi],
                iSliceCount,
            );
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }
        } else {
            // non-dynamic-slicing, single-threaded multi-slice
            let bNeedPrefix = pCtx.bNeedPrefixNalFlag;
            let mut iSliceIdx = 0i32;

            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(current_layer_mut(pCtx).expect("the frame's current layer is stamped"));
            while iSliceIdx < iSliceCount {
                let mut iPayloadSize = 0i32;

                if bNeedPrefix {
                    // S11.20: the C-ABI length pointer is read out before the record's
                    // `&mut` — one field of it, taken as a value.
                    let kpNalLen = pFbi.sLayerInfo[iLbi].pNalLengthInByte;
                    pCtx.iEncoderError = AddPrefixNal(pCtx,
                        &mut pFbi.sLayerInfo[iLbi],
                        kpNalLen,
                        &mut iNalIdxInLayer,
                        eNalType,
                        eNalRefIdc,
                        &mut iPayloadSize,
                    );
                    if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                        return pCtx.iEncoderError;
                    }
                    iLayerSize += iPayloadSize;
                }

                crate::encoder::nal_encap::WelsLoadNal(
                    pCtx.pOut.as_deref_mut().expect("pOut lives"),
                    eNalType as i32,
                    eNalRefIdc as i32,
                );

                // S11.35: the bank leaves the layer for the slice's span, as at
                // the single-slice site; `expect` where the raw deref'd null.
                let mut sBank = std::mem::take(
                    &mut current_layer_mut(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[0],
                );
                let pCurSlice = sBank
                    .pSliceBuffer
                    .get_mut(iSliceIdx as usize)
                    .expect("the fixed-mode bank holds every stamped slice");
                debug_assert_eq!(iSliceIdx, pCurSlice.iSliceIdx);
                pCtx.iEncoderError = crate::encoder::svc_encode_slice::SetSliceBoundaryInfo(
                    current_layer_ref(pCtx),
                    &mut *pCurSlice,
                    iSliceIdx,
                );

                // T7.C3, as above.
                crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);
                // **S11.1b: the pOut pair leaves the context for the call — safe now.**
                // The buffer is taken (a `Vec` move, no bytes) and the writer read out
                // (`BsWriter` is `Copy`); both are restored immediately after the call,
                // before the error check, so every exit path sees them back. Nothing in
                // the chain reaches `pOut` through the context any more (S11.1a threaded
                // the pair), which is what makes the momentary absence invisible — and
                // what S11.1a's raw hoist could only assert, the borrow checker now
                // proves: the chain's `&pCtx` coexists with `&mut` locals, not with
                // `&mut` context fields.
                let pOutRef = pCtx.pOut.as_deref_mut().expect("pOut lives");
                let mut vOutBsBuf = std::mem::take(&mut pOutRef.sBsBuffer);
                let mut sOutBsWrite = pOutRef.sBsWrite;
                let mut pCtxOutBs: Option<&mut crate::encoder::vlc_encoder::BsWriter> = Some(&mut sOutBsWrite);
                // **S11.27: the macroblock grid leaves the layer for the call**, exactly
                // as the `pOut` pair above does and for the same reason: the chain takes
                // `&sWelsEncCtx`, so a `&mut` to a field inside it cannot span the call.
                // `MbArray::empty()` is a `Vec::new()` — the swap moves two pointers and
                // no records — and the restore precedes the error check, so every exit
                // path sees the grid back.
                let mut sMbData = std::mem::replace(
                    &mut current_layer_mut(pCtx).expect("the frame's current layer is stamped").sMbDataP,
                    crate::safe::mb_grid::MbArray::empty(),
                );
                let mut sMbWindow = crate::safe::mb_grid::MbWindow::whole(&mut sMbData, 0);
                // S11.30: the CABAC restore scratch, taken beside the grid — partition
                // 0 is the only one a single-threaded encode names (`kiSliceIdx %
                // iActiveThreadsNum` with one thread). Empty means never allocated for
                // this configuration — the old null.
                let mut vRestoreBuf = std::mem::take(&mut pCtx.pDynamicBsBuffer[0]);
                let pRestoreBuf =
                    if vRestoreBuf.is_empty() { None } else { Some(vRestoreBuf.as_mut_slice()) };
                // S11.33: fixed-mode ST — the dynamic boundary never fires.
                let iCodeRet = crate::encoder::svc_encode_slice::WelsCodeOneSlice(pCtx,
                    &mut *pCurSlice,
                    eNalType as i32,
                    vOutBsBuf.as_mut_slice(),
                    &mut pCtxOutBs,
                    &mut sMbWindow,
                    pRestoreBuf,
                    None,
                );
                pCtx.pDynamicBsBuffer[0] = vRestoreBuf;
                drop(sMbWindow);
                current_layer_mut(pCtx).expect("the frame's current layer is stamped").sMbDataP = sMbData;
                // S11.35: the bank goes back with the rest.
                current_layer_mut(pCtx).expect("the frame's current layer is stamped").sSliceBufferInfo[0] = sBank;
                let pOutRef = pCtx.pOut.as_deref_mut().expect("pOut lives");
                pOutRef.sBsBuffer = vOutBsBuf;
                pOutRef.sBsWrite = sOutBsWrite;
                pCtx.iEncoderError = iCodeRet;
                if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                    return pCtx.iEncoderError;
                }

                crate::encoder::nal_encap::WelsUnloadNal(pCtx.pOut.as_deref_mut().expect("pOut lives"));

                // S11.37: a value, not a cursor — `SNalUnitHeaderExt` is `Copy`, the
                // callee only reads it, and a copy survives the context destructure
                // with no borrow (the S3.B1 raw hoist said this in provenance).
                let kNalHeaderExt =
                    current_layer_ref(pCtx).expect("the frame's current layer is stamped").sLayerInfo.sNalHeaderExt;
                // **S11.17**: the context is destructured. The NAL entry and the
                // source bytes live in `pOut`; the destination is the tail of
                // `pFrameBs` — disjoint fields, so both borrows are live at once
                // where the argument list would have been two borrows of the whole
                // context. No copy: the source is borrowed, not cloned.
                let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
                let kpOut = pOut.as_deref().expect("pOut lives");
                let kiPos = *iPosBsBuffer as usize;
                let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
                pCtx.iEncoderError = crate::encoder::nal_encap::WelsEncodeNal(
                    &kpOut.sNalList[kpOut.iNalIndex as usize - 1],
                    &kpOut.sBsBuffer[..],
                    Some(&kNalHeaderExt),
                    pDstTail,
                    &mut *pFbi.sLayerInfo[iLbi].pNalLengthInByte.add(iNalIdxInLayer as usize),
                );
                if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                    return pCtx.iEncoderError;
                }
                let iSliceSize = *pFbi.sLayerInfo[iLbi].pNalLengthInByte.add(iNalIdxInLayer as usize);

                pCtx.iPosBsBuffer += iSliceSize;
                iLayerSize += iSliceSize;

                iNalIdxInLayer += 1;
                iSliceIdx += 1;
            }

            pFbi.sLayerInfo[iLbi].uiLayerType = VIDEO_CODING_LAYER;
            pFbi.sLayerInfo[iLbi].uiSpatialId = iCurDid as u8;
            pFbi.sLayerInfo[iLbi].uiTemporalId = iCurTid as u8;
            pFbi.sLayerInfo[iLbi].uiQualityId = 0;
            pFbi.sLayerInfo[iLbi].iNalCount = iNalIdxInLayer;
            pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
            pFbi.sLayerInfo[iLbi].iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
        }

        // `None` here meant "never take this path", which is what the method's
        // empty arms return: `false`.
        if pCtx.func_list()
            .pfRc
            .WelsRcPostFrameSkipping(pCtx, iCurDid as i32, pFbi.uiTimeStamp)
        {
            StackBackEncoderStatus(pCtx, eFrameType);
            ClearFrameBsInfo(pCtx, &mut *pFbi);

            iFrameSize = 0;
            iLayerNum = 0;

            pCtx.func_list()
                .pfRc
                .WelsUpdateBufferWhenSkip(pCtx, iSpatialNum);

            crate::encoder::rc::WelsRcPostFrameSkippedUpdate(pCtx, iCurDid as i32);
            pCtx.iEncoderError = ENC_RETURN_SUCCESS;
            let _ = iLayerNum;
            return ENC_RETURN_SUCCESS;
        }

        // deblocking filter. ENABLE_FRAME_DUMP is not defined, so the temporal-id
        // clause is compiled in.
        if !current_layer_ref(pCtx).expect("the frame's current layer is stamped").bDeblockingParallelFlag
            && eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST
            && ((*pParamInternal).iHighestTemporalId == 0
                || iCurTid < (*pParamInternal).iHighestTemporalId as i32)
        {
            crate::encoder::deblocking::PerformDeblockingFilter(pCtx);
        }

        pCtx.func_list()
            .pfRc
            .WelsRcPictureInfoUpdate(pCtx, iLayerSize);
        iFrameSize += iLayerSize;
        crate::encoder::rc::RcTraceFrameBits(pCtx, pFbi.uiTimeStamp, iFrameSize);
        if let Some(id) = pCtx.pDecPic {
            // §4.6, reorder: the read is hoisted so the shared borrow of the
            // context ends before the reference list's cursor is written through.
            let iAverageFrameQp = pCtx.rc_at(iCurDid as usize).iAverageFrameQp;
            if let Some(pRefList) = pCtx.ref_list_mut(iCurDid as usize) {
                pRefList.pic_mut(id).iFrameAverageQp = iAverageFrameQp;
            }
        }

        // update scc related
        if let Some(f) = pCtx.func_list().pfUpdateFMESwitch {
            f(current_layer_ref(pCtx));
        }

        // reference picture list update
        if eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST
            && !eRefStrategy.UpdateRefList(pCtx)
        {
            // set the next frame to be IDR
            pCtx.iEncoderError = ENC_RETURN_CORRECTED;
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
            let pRefListPsnr = pCtx.ref_list(iCurDid as usize);
            if pRefListPsnr.is_some() && pCtx.pVpp.is_some() {
                let recon = pRefListPsnr.expect("checked just above");
                let vpp = crate::encoder::encoder_context::ctx_vpp_ref(pCtx);
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
                if pCtx.param().bPsnrY || (*pSrcPic).bPsnrY {
                    fSnrY = plane_psnr(0, iCurWidth, iCurHeight);
                }
                if pCtx.param().bPsnrU || (*pSrcPic).bPsnrU {
                    fSnrU = plane_psnr(1, iCurWidth >> 1, iCurHeight >> 1);
                }
                if pCtx.param().bPsnrV || (*pSrcPic).bPsnrV {
                    fSnrV = plane_psnr(2, iCurWidth >> 1, iCurHeight >> 1);
                }
            }
        }

        pFbi.sLayerInfo[iLbi].rPsnr[0] = 0.0;
        pFbi.sLayerInfo[iLbi].rPsnr[1] = 0.0;
        pFbi.sLayerInfo[iLbi].rPsnr[2] = 0.0;
        if (*pSrcPic).bPsnrY {
            pFbi.sLayerInfo[iLbi].rPsnr[0] = fSnrY;
        }
        if (*pSrcPic).bPsnrU {
            pFbi.sLayerInfo[iLbi].rPsnr[1] = fSnrU;
        }
        if (*pSrcPic).bPsnrV {
            pFbi.sLayerInfo[iLbi].rPsnr[2] = fSnrV;
        }

        iCountNal = pFbi.sLayerInfo[iLbi].iNalCount;
        iLayerNum += 1;
        // The NAL-length array is the application's `pNalLengthInByte`, a
        // C-ABI field: still a pointer, still advanced by the previous layer's
        // NAL count. Only the *layer* walk became an index.
        let kpPrevNalLen = pFbi.sLayerInfo[iLbi].pNalLengthInByte;
        iLbi += 1;
        pCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex += 1;
        pFbi.sLayerInfo[iLbi].pBsBuf = pCtx.frame_bs_cur();
        pFbi.sLayerInfo[iLbi].pNalLengthInByte = kpPrevNalLen.add(iCountNal as usize);

        if pCtx.param().iPaddingFlag != 0
            && pCtx.rc_at(pCtx.uiDependencyId as usize).iPaddingSize > 0
        {
            let mut iPaddingNalSize = 0i32;
            let iPaddingSize = pCtx.rc_at(pCtx.uiDependencyId as usize).iPaddingSize;
            pCtx.iEncoderError = WritePadding(pCtx, iPaddingSize, &mut iPaddingNalSize);
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }

            if iPaddingNalSize <= 0 {
                return ENC_RETURN_UNEXPECTED;
            }

            // §4.6, reorder: the layer id is read out before the writer's
            // `&mut` claims the context.
            let did = pCtx.uiDependencyId as usize;
            let pRc = pCtx.rc_at_mut(did);
            pRc.iPaddingBitrateStat += pRc.iPaddingSize;
            pRc.iPaddingSize = 0;

            pFbi.sLayerInfo[iLbi].uiSpatialId = 0;
            pFbi.sLayerInfo[iLbi].uiTemporalId = 0;
            pFbi.sLayerInfo[iLbi].uiQualityId = 0;
            pFbi.sLayerInfo[iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
            pFbi.sLayerInfo[iLbi].iNalCount = 1;
            *pFbi.sLayerInfo[iLbi].pNalLengthInByte = iPaddingNalSize;
            pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
            pFbi.sLayerInfo[iLbi].iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            let kpPrevNalLen2 = pFbi.sLayerInfo[iLbi].pNalLengthInByte;
            iLbi += 1;
            pCtx.pOut.as_deref_mut().expect("pOut lives").iLayerBsIndex += 1;
            pFbi.sLayerInfo[iLbi].pBsBuf = pCtx.frame_bs_cur();
            pFbi.sLayerInfo[iLbi].pNalLengthInByte = kpPrevNalLen2.add(1);
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
            && pCtx.param().bUseLoadBalancing
            && pCtx.param().iMultipleThreadIdc > 1
            && pCtx.param().iMultipleThreadIdc >= (*pParam).sSliceArgument.uiSliceNum as u16
        {
            crate::encoder::slice_multi_threading::CalcSliceComplexRatio(current_layer_mut(pCtx).expect("the frame's current layer is stamped"));
        }

        pCtx.eLastNalPriority[iCurDid as usize] = eNalRefIdc;
        iSpatialIdx += 1;

        if (iCurDid as i32) + 1 < pCtx.param().iSpatialLayerNum {
            // iSpatialIdx has already been incremented, so this points at the next layer.
            // Hoisted: `WelsSwapDqLayers` takes the ctx retag and this argument reads
            // through the same ctx (shape B).
            let iNextDid = pCtx.sSpatialIndexMap[iSpatialIdx as usize].iDid;
            WelsSwapDqLayers(pCtx, iNextDid);
        }

        if crate::encoder::encoder_context::with_vpp(pCtx, |pVpp, pCtx| {
            pVpp.UpdateSpatialPictures(pCtx, iCurTid as i8, iCurDid as i32)
        }) != 0
        {
            crate::encoder::wels_encoder_ext::ForceCodingIDR(pCtx, iCurDid as i32);
            // the above sets the next frame to IDR
            pFbi.eFrameType = eFrameType;
            pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
            return ENC_RETURN_CORRECTED;
        }

        let uiDidForLtr = pCtx.uiDependencyId as usize;
        // A7, §4.6 reorder: the flag is read before the LTR state's `&mut`.
        let kbEnableLtr = pCtx.param().bEnableLongTermReference;
        let pLtr = ctx_ltr_at(pCtx, uiDidForLtr);
        if kbEnableLtr
            && ((pLtr.bLTRMarkingFlag
                && pLtr.iLTRMarkMode == crate::encoder::ref_list_mgr_svc::LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32)
                || eFrameType == EVideoFrameType::videoFrameTypeIDR)
        {
            pCtx.bRefOfCurTidIsLtr[iCurDid as usize][iCurTid as usize] = true;
        }
        if pCtx.param().bSimulcastAVC {
            (*pParamInternal).iCodingIndex += 1;
        }
    } // end of (iSpatialIdx/iSpatialNum)

    if !pCtx.param().bSimulcastAVC {
        for i in 0..pCtx.param().iSpatialLayerNum as usize {
            pCtx.param_mut().sDependencyLayers[i].iCodingIndex += 1;
        }
    }

    if ENC_RETURN_CORRECTED == pCtx.iEncoderError {
        // **`iSpatialIdx == iSpatialNum` here** — the loop above ran to completion —
        // so with 4 spatial layers configured this reads **one past the end** of a
        // `[SSpatialPicIndex; 4]`. Upstream does the identical thing at
        // `encoder_ext.cpp:4109-4110` (`(pSpatialIndexMap + iSpatialIdx)->iDid`,
        // twice), so the port reproduces it rather than fixing it: an index would
        // panic where this reads, and a panic is not byte-identical. F162.
        // Spelled through a derivation that lives and dies inside this statement, so
        // nothing is held across the two calls below — which is the whole hazard.
        let iDid = (*pCtx.sSpatialIndexMap.as_ptr().add(iSpatialIdx as usize)).iDid;
        crate::encoder::encoder_context::with_vpp(pCtx, |pVpp, pCtx| {
            pVpp.UpdateSpatialPictures(pCtx, iCurTid as i8, iDid)
        });
        crate::encoder::wels_encoder_ext::ForceCodingIDR(pCtx, iDid);
        // the above sets the next frame to IDR
        pFbi.eFrameType = eFrameType;
        pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
        return ENC_RETURN_CORRECTED;
    }

    // check number of layers / nals / slices dependencies
    if iLayerNum > MAX_LAYER_NUM_OF_FRAME {
        return 1;
    }

    pFbi.iLayerNum = iLayerNum;

    crate::encoder::slice_multi_threading::WelsEmms();

    pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
    pFbi.iFrameSizeInBytes = iFrameSize;
    pFbi.eFrameType = eFrameType;
    for k in 0..pFbi.iLayerNum as usize {
        if pFbi.eFrameType != pFbi.sLayerInfo[k].eFrameType {
            pFbi.eFrameType = EVideoFrameType::videoFrameTypeIPMixed;
        }
    }

    ENC_RETURN_SUCCESS
}
