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
use crate::encoder::md::CostFamily;
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};

use crate::api::codec_api::EUsageType::{CAMERA_VIDEO_REAL_TIME, SCREEN_CONTENT_REAL_TIME};
use crate::api::codec_api::SliceModeEnum;
use crate::api::codec_api::SliceModeEnum::{SM_SINGLE_SLICE, SM_SIZELIMITED_SLICE};
use crate::api::codec_api::RC_MODES::RC_OFF_MODE;
use crate::api::codec_api::{ELevelIdc, SSpatialLayerConfig};
use crate::common::memory_align::CMemoryAlign;
use crate::decoder::nalu::g_ksLevelLimits;
use crate::encoder::encoder_context::{
    sWelsEncCtx, SDqIdc, SLogContext, SRefList, SStrideTables, BASE_DEPENDENCY_ID,
};
use crate::encoder::md::INTRA_4x4_MODE_NUM;
use crate::encoder::param_svc::{
    SExistingParasetList, SWelsSvcCodingParam, MB_WIDTH_LUMA, UNSPECIFIED_BIT_RATE,
};
use crate::encoder::paraset_strategy::{ParasetStrategy, PARA_SET_TYPE_AVCSPS, PARA_SET_TYPE_PPS};
use crate::api::codec_api::EParameterSetStrategy;
use crate::encoder::picture::SPicture;
use crate::encoder::slice_multi_threading::{
    MAX_DEPENDENCY_LAYER, MAX_SLICES_NUM, MAX_THREADS_NUM,
};
use crate::encoder::svc_enc_slice_segment::{GetInitialSliceNum, InitSlicePEncCtx};
use crate::encoder::svc_encode_slice::{InitSliceInLayer, WelsMbToSliceIdc};
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

    if (**ppCtx).pFuncList.is_null() {
        return 1;
    }
    // count parasets
    let Some(pStrategy) = (*(**ppCtx).pFuncList).pParametersetStrategy.as_mut() else {
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
pub unsafe fn AllocStrideTables(ppCtx: *mut *mut sWelsEncCtx, kiNumSpatialLayers: i32) -> i32 {
    let pMa = (**ppCtx).pMemAlign;
    let pParam = (**ppCtx).pSvcParam;

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

    let pPtr = (*pMa).WelsMallocz(
        std::mem::size_of::<SStrideTables>() as u32,
        tag!("SStrideTables"),
    ) as *mut SStrideTables;
    if pPtr.is_null() {
        return 1;
    }
    (**ppCtx).pStrideTab = pPtr;

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

    let pBase = (*pMa).WelsMallocz(iNeedAllocSize as u32, tag!("pBase")) as *mut u8;
    if pBase.is_null() {
        return 1;
    }

    let mut pBaseDec = pBase; // iCountLayersNeedCs
    let mut pBaseEnc = pBaseDec.add(iSizeDec as usize); // iNumSpatialLayers
    let mut pBaseMbX = pBaseEnc.add(iSizeEnc as usize); // iNumSpatialLayers
    let mut pBaseMbY = pBaseMbX.add(iUnit2Size as usize); // iNumSpatialLayers

    iTemporalIdx = 0;
    while iTemporalIdx < iCntTid {
        let kbBaseTemporalFlag = usize::from(iTemporalIdx == 0);

        iSpatialIdx = 0;
        while iSpatialIdx < iCountLayersNeedCs[kbBaseTemporalFlag] {
            let kiActualSpatialIdx =
                iMapSpatialIdx[iSpatialIdx as usize][kbBaseTemporalFlag] as usize;
            let kiLumaWidth = iLineSizeY[kiActualSpatialIdx][kbBaseTemporalFlag];
            let kiChromaWidth = iLineSizeUV[kiActualSpatialIdx][kbBaseTemporalFlag];

            WelsGetEncBlockStrideOffset(pBaseDec as *mut i32, kiLumaWidth, kiChromaWidth);

            (*pPtr).pStrideDecBlockOffset[kiActualSpatialIdx][kbBaseTemporalFlag] =
                pBaseDec as *mut i32;
            pBaseDec = pBaseDec.add(kiUnit1Size as usize);

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

            // not in the spatial map: assign the matching one to it
            (*pPtr).pStrideDecBlockOffset[iSpatialIdx as usize][kbBaseTemporalFlag] =
                (*pPtr).pStrideDecBlockOffset[iMatchIndex as usize][kbBaseTemporalFlag];

            iSpatialIdx += 1;
        }
        iTemporalIdx += 1;
    }

    iSpatialIdx = 0;
    while iSpatialIdx < kiNumSpatialLayers {
        let kiAllocMbSize = sMbSizeMap[iSpatialIdx as usize].iSizeAllMbAlignCache;

        (*pPtr).pStrideEncBlockOffset[iSpatialIdx as usize] = pBaseEnc as *mut i32;

        (*pPtr).pMbIndexX[iSpatialIdx as usize] = pBaseMbX as *mut i16;
        (*pPtr).pMbIndexY[iSpatialIdx as usize] = pBaseMbY as *mut i16;

        pBaseEnc = pBaseEnc.add(kiUnit1Size as usize);
        pBaseMbX = pBaseMbX.add(kiAllocMbSize as usize);
        pBaseMbY = pBaseMbY.add(kiAllocMbSize as usize);

        iSpatialIdx += 1;
    }

    while iSpatialIdx < MAX_DEPENDENCY_LAYER as i32 {
        (*pPtr).pStrideDecBlockOffset[iSpatialIdx as usize][0] = null_mut();
        (*pPtr).pStrideDecBlockOffset[iSpatialIdx as usize][1] = null_mut();
        (*pPtr).pStrideEncBlockOffset[iSpatialIdx as usize] = null_mut();
        (*pPtr).pMbIndexX[iSpatialIdx as usize] = null_mut();
        (*pPtr).pMbIndexY[iSpatialIdx as usize] = null_mut();

        iSpatialIdx += 1;
    }

    // initialize pMbIndexX and pMbIndexY tables as below

    // 4 loops for int16_t required, as introduced below
    let iMaxMbWidth = WELS_ALIGN(sMbSizeMap[(kiNumSpatialLayers - 1) as usize].iMbWidth, 4);
    let iRowSize = iMaxMbWidth * 2;

    let pTmpRow = (*pMa).WelsMallocz(iRowSize as u32, tag!("pTmpRow")) as *mut i16;
    if pTmpRow.is_null() {
        return 1;
    }
    let pRowX = pTmpRow;
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
        let mut pMbIndexX = (*pPtr).pMbIndexX[iSpatialIdx as usize];
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
            let pMbIndexY = (*pPtr).pMbIndexY[iSpatialIdx as usize].add((i * kiMbWidth) as usize);

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

    (*pMa).WelsFree(pTmpRow as *mut c_void, tag!("pTmpRow"));

    0
}

/// `GetMvMvdRange` — encoder_ext.cpp:1508.
///
/// # Safety
/// `pParam` must be initialised.
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
unsafe fn InitMbInfo(
    pEnc: *mut sWelsEncCtx,
    pList: *mut SMB,
    pLayer: *mut SDqLayer,
    kiDlayerId: i32,
) {
    let iMbWidth = (*pLayer).iMbWidth as i32;
    let iMbHeight = (*pLayer).iMbHeight as i32;
    let iMbNum = iMbWidth * iMbHeight;

    for iIdx in 0..iMbNum as usize {
        let pMb = pList.add(iIdx);

        (*pMb).iMbX = *(*(*pEnc).pStrideTab).pMbIndexX[kiDlayerId as usize].add(iIdx);
        (*pMb).iMbY = *(*(*pEnc).pStrideTab).pMbIndexY[kiDlayerId as usize].add(iIdx);
        (*pMb).iMbXY = iIdx as i32;

        // [0..65535] > 36864 of LEVEL5.2
        let uiSliceIdc: u16 = WelsMbToSliceIdc(pLayer, iIdx as i32);
        let iLeftXY = iIdx as i32 - 1;
        let iTopXY = iIdx as i32 - iMbWidth;
        let iLeftTopXY = iTopXY - 1;
        let iRightTopXY = iTopXY + 1;

        let bLeft = (*pMb).iMbX > 0 && uiSliceIdc == WelsMbToSliceIdc(pLayer, iLeftXY);
        let bTop = (*pMb).iMbY > 0 && uiSliceIdc == WelsMbToSliceIdc(pLayer, iTopXY);
        let bLeftTop =
            (*pMb).iMbX > 0 && (*pMb).iMbY > 0 && uiSliceIdc == WelsMbToSliceIdc(pLayer, iLeftTopXY);
        let bRightTop = ((*pMb).iMbX as i32) < (iMbWidth - 1)
            && (*pMb).iMbY > 0
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
        (*pMb).uiSliceIdc = uiSliceIdc;
        (*pMb).uiNeighborAvail = uiNeighborAvail;

        // C++ recomputes uiNeighborAvail here for the base-MV neighbourhood, then
        // discards it — the result is never stored. Reproduced as a no-op comment
        // rather than dead code.
    }
}

/// `InitMbListD` — encoder_ext.cpp:907.
///
/// # Safety
/// `ppCtx` must point to a live context with `ppDqLayerList` populated.
pub unsafe fn InitMbListD(ppCtx: *mut *mut sWelsEncCtx) -> i32 {
    let iNumDlayer = (*(**ppCtx).pSvcParam).iSpatialLayerNum;

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
        let iMbWidth = ((*(**ppCtx).pSvcParam).sSpatialLayers[i].iVideoWidth + 15) >> 4;
        let iMbHeight = ((*(**ppCtx).pSvcParam).sSpatialLayers[i].iVideoHeight + 15) >> 4;
        let pLayer = *(**ppCtx).ppDqLayerList.add(i);
        if pLayer.is_null() {
            return 1;
        }
        (*pLayer).sMbDataP = MbArray::new(
            MbDims::new(iMbWidth as usize, iMbHeight as usize),
            SMB::default(),
        );
        InitMbInfo(
            *ppCtx,
            crate::encoder::svc_encode_slice::mb_list_root(pLayer),
            pLayer,
            i as i32,
        );
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

    let pMa = (**ppCtx).pMemAlign;
    let pParam = (**ppCtx).pSvcParam;
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

        WelsGetEncBlockStrideOffset(
            (*(**ppCtx).pStrideTab).pStrideEncBlockOffset[iDlayerIndex as usize],
            iPicWidth,
            iPicChromaWidth,
        );

        // reference list
        let pRefList = (*pMa).WelsMallocz(
            std::mem::size_of::<SRefList>() as u32,
            tag!("pRefList"),
        ) as *mut SRefList;
        if pRefList.is_null() {
            return 1;
        }
        loop {
            // use the actual size of the current layer
            (*pRefList).pRef[i as usize] = AllocPicture(
                pMa,
                kiWidth,
                kiHeight,
                true,
                if iDlayerIndex == iDlayerCount - 1 {
                    kiNeedFeatureStorage
                } else {
                    0
                },
            );
            if (*pRefList).pRef[i as usize].is_null() {
                return 1;
            }
            i += 1;
            if i >= 1 + iNumRef {
                break;
            }
        }

        (*pRefList).pNextBuffer = (*pRefList).pRef[0];
        *(**ppCtx).ppRefPicListExt.add(iDlayerIndex as usize) = pRefList;
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
        let pDqLayer = Box::into_raw(Box::new(SDqLayer::new(LayerIdx(iDlayerIndex as u8))));

        (*pDqLayer).iMbWidth = kiMbW as i16;
        (*pDqLayer).iMbHeight = kiMbH as i16;

        let mut iMaxSliceNum: i32 = 1;
        let kiSliceNum = GetInitialSliceNum(&(*pDlayer).sSliceArgument);
        if iMaxSliceNum < kiSliceNum {
            iMaxSliceNum = kiSliceNum;
        }
        (*pDqLayer).iMaxSliceNum = iMaxSliceNum;

        iResult = InitSliceInLayer(*ppCtx, pDqLayer, iDlayerIndex, pMa);
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

        *(**ppCtx).ppDqLayerList.add(iDlayerIndex as usize) = pDqLayer;

        iDlayerIndex += 1;
    }

    // dynamically allocate parameter-set memory instead of the standard's maximum, to
    // reduce size (3/18/2010)
    if (**ppCtx).pFuncList.is_null() {
        return 1;
    }
    // The borrow is re-acquired at each use rather than held across the loop below:
    // `GenerateNewSps`/`InitPps` take `*ppCtx`, and reaching the strategy through the
    // context while a `&mut` to it is live would alias. Same reason as
    // `WelsWriteParameterSets`; T4b.2a.
    if (*(**ppCtx).pFuncList).pParametersetStrategy.is_none() {
        return 1;
    }
    let kiNeededSpsNum = ParasetStrategy(*ppCtx).GetNeededSpsNum() as i32;
    let kiNeededSubsetSpsNum = ParasetStrategy(*ppCtx).GetNeededSubsetSpsNum() as i32;
    (**ppCtx).pSpsArray = (*pMa).WelsMallocz(
        (kiNeededSpsNum as usize * std::mem::size_of::<crate::encoder::param_svc::SWelsSPS>())
            as u32,
        tag!("pSpsArray"),
    ) as *mut crate::encoder::param_svc::SWelsSPS;
    if (**ppCtx).pSpsArray.is_null() {
        return 1;
    }
    if kiNeededSubsetSpsNum > 0 {
        (**ppCtx).pSubsetArray = (*pMa).WelsMallocz(
            (kiNeededSubsetSpsNum as usize
                * std::mem::size_of::<crate::encoder::param_svc::SSubsetSps>()) as u32,
            tag!("pSubsetArray"),
        ) as *mut crate::encoder::param_svc::SSubsetSps;
        if (**ppCtx).pSubsetArray.is_null() {
            return 1;
        }
    } else {
        (**ppCtx).pSubsetArray = null_mut();
    }

    // PPS
    let kiNeededPpsNum = ParasetStrategy(*ppCtx).GetNeededPpsNum() as i32;
    (**ppCtx).pPPSArray = (*pMa).WelsMallocz(
        (kiNeededPpsNum as usize * std::mem::size_of::<crate::encoder::param_svc::SWelsPPS>())
            as u32,
        tag!("pPPSArray"),
    ) as *mut crate::encoder::param_svc::SWelsPPS;
    if (**ppCtx).pPPSArray.is_null() {
        return 1;
    }

    ParasetStrategy(*ppCtx).LoadPrevious(
        pExistingParasetList,
        (**ppCtx).pSpsArray,
        (**ppCtx).pSubsetArray,
        (**ppCtx).pPPSArray,
    );

    (**ppCtx).pDqIdcMap = (*pMa).WelsMallocz(
        (iDlayerCount as usize * std::mem::size_of::<SDqIdc>()) as u32,
        tag!("pDqIdcMap"),
    ) as *mut SDqIdc;
    if (**ppCtx).pDqIdcMap.is_null() {
        return 1;
    }

    iDlayerIndex = 0;
    while iDlayerIndex < iDlayerCount {
        let pDqIdc = (**ppCtx).pDqIdcMap.add(iDlayerIndex as usize);
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
            &mut pSps,
            &mut pSubsetSps,
            bSvcBaselayer,
        ) as i32;
        if 0 > iSpsId {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        if !bUseSubsetSps {
            pSps = (**ppCtx).pSpsArray.add(iSpsId as usize);
        } else {
            pSubsetSps = (**ppCtx).pSubsetArray.add(iSpsId as usize);
        }

        iPpsId = ParasetStrategy(*ppCtx).InitPps(
            *ppCtx,
            iSpsId as u32,
            pSps,
            pSubsetSps,
            iPpsId,
            true,
            bUseSubsetSps,
            (*pParam).iEntropyCodingModeFlag != 0,
        );
        let pPps = (**ppCtx).pPPSArray.add(iPpsId as usize);

        // FMO is not used in SVC coding so far; come back if FMO is needed
        iResult = InitSlicePEncCtx(
            *(**ppCtx).ppDqLayerList.add(iDlayerIndex as usize),
            (**ppCtx).pMemAlign,
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
/// **Deviations, all explicit:** the screen-content VAA extension
/// (`RequestMemoryVaaScreen`), the adaptive-quantisation buffers, the
/// background-detection buffers and the dynamic-slice CABAC buffers are guarded by
/// parameters the Phase-5 gate configuration does not use, and are not ported; each
/// returns `ENC_RETURN_UNSUPPORTED_PARA` rather than allocating nothing and carrying
/// on. `RequestMtResource` is likewise only reached with `iMultipleThreadIdc > 1`.
///
/// # Safety
/// `ppCtx` must point to a live context with `pMemAlign`, `pSvcParam` and
/// `pFuncList->pParametersetStrategy` set.
pub unsafe fn RequestMemorySvc(
    ppCtx: *mut *mut sWelsEncCtx,
    pExistingParasetList: *mut SExistingParasetList,
) -> i32 {
    let pParam = (**ppCtx).pSvcParam;
    let pMa = (**ppCtx).pMemAlign;
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

    (**ppCtx).pFrameBs = (*pMa).WelsMalloc(iTotalLength as u32, tag!("pFrameBs")) as *mut u8;
    if (**ppCtx).pFrameBs.is_null() {
        return 1;
    }
    (**ppCtx).iFrameBsSize = iTotalLength;
    (**ppCtx).iPosBsBuffer = 0;

    // for dynamic slice mode && CABAC, allocate slice buffers to restore slice data.
    // These are `sDss.pRestoreBuffer` in the two dynamic MB loops: CABAC
    // renormalisation can rewrite bytes already emitted, so stepping back over a
    // slice boundary has to restore the bytes as well as the coder state.
    if bDynamicSlice && (*pParam).iEntropyCodingModeFlag != 0 {
        for iIdx in 0..MAX_THREADS_NUM {
            (**ppCtx).pDynamicBsBuffer[iIdx] =
                (*pMa).WelsMalloc(iMaxSliceBufferSize as u32, tag!("DynamicSliceBs")) as *mut u8;
            if (**ppCtx).pDynamicBsBuffer[iIdx].is_null() {
                return 1;
            }
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

    (**ppCtx).pLtr = (*pMa).WelsMallocz(
        (kiNumDependencyLayers as usize
            * std::mem::size_of::<crate::encoder::ref_list_mgr_svc::SLTRState>())
            as u32,
        tag!("SLTRState"),
    ) as *mut crate::encoder::ref_list_mgr_svc::SLTRState;
    if (**ppCtx).pLtr.is_null() {
        return 1;
    }
    for i in 0..kiNumDependencyLayers as usize {
        crate::encoder::ref_list_mgr_svc::ResetLtrState((**ppCtx).pLtr.add(i));
    }

    // stride tables
    if AllocStrideTables(ppCtx, kiNumDependencyLayers) != 0 {
        return 1;
    }

    // Rate control module memory allocation; only malloc once for RC data (12/14/2009)
    (**ppCtx).pWelsSvcRc = (*pMa).WelsMallocz(
        (kiNumDependencyLayers as usize * std::mem::size_of::<crate::encoder::rc::SWelsSvcRc>())
            as u32,
        tag!("pWelsSvcRc"),
    ) as *mut crate::encoder::rc::SWelsSvcRc;
    if (**ppCtx).pWelsSvcRc.is_null() {
        return 1;
    }

    // pVaa memory allocation
    if (*pParam).iUsageType == SCREEN_CONTENT_REAL_TIME {
        // encoder_ext.cpp:1708, SVAAFrameInfoExt + RequestMemoryVaaScreen. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    (**ppCtx).pVaa = (*pMa).WelsMallocz(
        std::mem::size_of::<crate::encoder::wels_preprocess::SVAAFrameInfo>() as u32,
        tag!("pVaa"),
    ) as *mut crate::encoder::wels_preprocess::SVAAFrameInfo;
    if (**ppCtx).pVaa.is_null() {
        return 1;
    }

    if (*(**ppCtx).pSvcParam).bEnableAdaptiveQuant {
        // encoder_ext.cpp:1720, sAdaptiveQuantParam buffers. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    (*(**ppCtx).pVaa).pVaaBackgroundMbFlag = (*pMa).WelsMallocz(
        iCountMaxMbNum as u32,
        tag!("pVaa->pVaaBackgroundMbFlag"),
    ) as *mut i8;
    if (*(**ppCtx).pVaa).pVaaBackgroundMbFlag.is_null() {
        return 1;
    }

    (*(**ppCtx).pVaa).sVaaCalcInfo.pSad8x8 = (*pMa).WelsMallocz(
        (iCountMaxMbNum as u32) * 4 * 4,
        tag!("pVaa->sVaaCalcInfo.sad8x8"),
    ) as *mut [i32; 4];
    if (*(**ppCtx).pVaa).sVaaCalcInfo.pSad8x8.is_null() {
        return 1;
    }
    (*(**ppCtx).pVaa).sVaaCalcInfo.pSsd16x16 = (*pMa).WelsMallocz(
        (iCountMaxMbNum as u32) * 4,
        tag!("pVaa->sVaaCalcInfo.pSsd16x16"),
    ) as *mut i32;
    if (*(**ppCtx).pVaa).sVaaCalcInfo.pSsd16x16.is_null() {
        return 1;
    }
    (*(**ppCtx).pVaa).sVaaCalcInfo.pSum16x16 = (*pMa).WelsMallocz(
        (iCountMaxMbNum as u32) * 4,
        tag!("pVaa->sVaaCalcInfo.pSum16x16"),
    ) as *mut i32;
    if (*(**ppCtx).pVaa).sVaaCalcInfo.pSum16x16.is_null() {
        return 1;
    }
    (*(**ppCtx).pVaa).sVaaCalcInfo.pSumOfSquare16x16 = (*pMa).WelsMallocz(
        (iCountMaxMbNum as u32) * 4,
        tag!("pVaa->sVaaCalcInfo.pSumOfSquare16x16"),
    ) as *mut i32;
    if (*(**ppCtx).pVaa).sVaaCalcInfo.pSumOfSquare16x16.is_null() {
        return 1;
    }

    if (*(**ppCtx).pSvcParam).bEnableBackgroundDetection {
        (*(**ppCtx).pVaa).sVaaCalcInfo.pSumOfDiff8x8 = (*pMa).WelsMallocz(
            (iCountMaxMbNum as u32) * 4 * 4,
            tag!("pVaa->sVaaCalcInfo.pSumOfDiff8x8"),
        ) as *mut [i32; 4];
        if (*(**ppCtx).pVaa).sVaaCalcInfo.pSumOfDiff8x8.is_null() {
            return 1;
        }
        (*(**ppCtx).pVaa).sVaaCalcInfo.pMad8x8 = (*pMa).WelsMallocz(
            (iCountMaxMbNum as u32) * 4,
            tag!("pVaa->sVaaCalcInfo.pMad8x8"),
        ) as *mut [u8; 4];
        if (*(**ppCtx).pVaa).sVaaCalcInfo.pMad8x8.is_null() {
            return 1;
        }
    }
    // End of pVaa memory allocation

    (**ppCtx).ppRefPicListExt = (*pMa).WelsMallocz(
        (kiNumDependencyLayers as usize * std::mem::size_of::<*mut SRefList>()) as u32,
        tag!("ppRefPicListExt"),
    ) as *mut *mut SRefList;
    if (**ppCtx).ppRefPicListExt.is_null() {
        return 1;
    }

    (**ppCtx).ppDqLayerList = (*pMa).WelsMallocz(
        (kiNumDependencyLayers as usize * std::mem::size_of::<*mut SDqLayer>()) as u32,
        tag!("ppDqLayerList"),
    ) as *mut *mut SDqLayer;
    if (**ppCtx).ppDqLayerList.is_null() {
        return 1;
    }

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
    (**ppCtx).pMvdCostTable = (*pMa).WelsMallocz(
        (52 * kuiMvdCacheAlignedSize + kuiMvdCostTableOvershoot) as u32,
        tag!("pMvdCostTable"),
    ) as *mut u16;
    if (**ppCtx).pMvdCostTable.is_null() {
        return 1;
    }
    crate::encoder::md::MvdCostInit((**ppCtx).pMvdCostTable, kuiMvdInterTableStride);

    if !(*(**ppCtx).ppRefPicListExt).is_null() && !(**(**ppCtx).ppRefPicListExt).pRef[0].is_null() {
        (**ppCtx).pDecPic = (**(**ppCtx).ppRefPicListExt).pRef[0];
    } else {
        (**ppCtx).pDecPic = null_mut(); // error here
    }

    (**ppCtx).pSps = (**ppCtx).pSpsArray;
    (**ppCtx).pPps = (**ppCtx).pPPSArray;

    0
}

/// `InitSliceSettings` — encoder_ext.cpp:2018.
///
/// Resolves the per-layer slice arguments, then derives `iMultipleThreadIdc` and the
/// maximum slice count from them.
///
/// # Safety
/// `pCodingParam` and `pMaxSliceCount` must be non-null.
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
        let pSliceArgument = &mut (*pDlp).sSliceArgument;

        match pSliceArgument.uiSliceMode {
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

                if pSliceArgument.uiSliceNum as u16 > iMaxSliceCount {
                    iMaxSliceCount = pSliceArgument.uiSliceNum as u16;
                }
            }
            SM_SINGLE_SLICE | crate::api::codec_api::SliceModeEnum::SM_RASTER_SLICE => {
                if pSliceArgument.uiSliceNum as u16 > iMaxSliceCount {
                    iMaxSliceCount = pSliceArgument.uiSliceNum as u16;
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
pub unsafe fn WelsInitEncoderExt(
    ppCtx: *mut *mut sWelsEncCtx,
    pCodingParam: *mut SWelsSvcCodingParam,
    pLogCtx: *mut SLogContext,
    pExistingParasetList: *mut SExistingParasetList,
) -> i32 {
    let mut iSliceNum: i16 = 1; // number of slices used
    let mut iCacheLineSize: i32 = 16; // on-chip cache line size in bytes
    let mut uiCpuFeatureFlags: u32 = 0;
    if ppCtx.is_null() || pCodingParam.is_null() {
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

    *ppCtx = null_mut();

    // C++ mallocs and memsets sWelsEncCtx; Box::new of a Default context is the
    // equivalent, and Default is the all-zero/null state for every member.
    let pCtx = Box::into_raw(Box::new(sWelsEncCtx::default()));

    if !pLogCtx.is_null() {
        (*pCtx).sLogCtx = *pLogCtx;
    }

    (*pCtx).pMemAlign = Box::into_raw(Box::new(CMemoryAlign::new(iCacheLineSize as u32)));

    iRet = crate::encoder::param_svc::AllocCodingParam(&mut (*pCtx).pSvcParam, (*pCtx).pMemAlign);
    if iRet != 0 {
        let mut p = pCtx;
        WelsUninitEncoderExt(&mut p);
        return iRet;
    }
    *(*pCtx).pSvcParam = *pCodingParam;

    (*pCtx).pFuncList = (*(*pCtx).pMemAlign).WelsMallocz(
        std::mem::size_of::<crate::encoder::wels_func_ptr_def::SWelsFuncPtrList>() as u32,
        tag!("SWelsFuncPtrList"),
    ) as *mut crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
    if (*pCtx).pFuncList.is_null() {
        let mut p = pCtx;
        WelsUninitEncoderExt(&mut p);
        return 1;
    }
    iRet = crate::encoder::encoder_context::InitFunctionPointers(
        pCtx,
        (*pCtx).pSvcParam,
        uiCpuFeatureFlags,
    );
    if iRet != ENC_RETURN_SUCCESS {
        let mut p = pCtx;
        WelsUninitEncoderExt(&mut p);
        return iRet;
    }

    (*pCtx).iActiveThreadsNum = (*pCodingParam).iMultipleThreadIdc as i16;
    (*pCtx).iMaxSliceCount = iSliceNum as i32;
    let mut pCtxTmp = pCtx;
    iRet = RequestMemorySvc(&mut pCtxTmp, pExistingParasetList);
    if iRet != 0 {
        let mut p = pCtx;
        WelsUninitEncoderExt(&mut p);
        return iRet;
    }

    if (*pCodingParam).iEntropyCodingModeFlag != 0 {
        crate::encoder::set_mb_syn_cabac::WelsCabacInit(pCtx);
    }
    crate::encoder::rc::WelsRcInitModule(pCtx, (*(*pCtx).pSvcParam).iRCMode);

    (*pCtx).pVpp = crate::encoder::wels_preprocess::CWelsPreProcess::CreatePreProcess(pCtx);
    if (*pCtx).pVpp.is_null() {
        let mut p = pCtx;
        WelsUninitEncoderExt(&mut p);
        return 1;
    }
    iRet = (*(*pCtx).pVpp).AllocSpatialPictures(pCtx, (*pCtx).pSvcParam);
    if iRet != 0 {
        let mut p = pCtx;
        WelsUninitEncoderExt(&mut p);
        return iRet;
    }

    (*pCtx).iStatisticsLogInterval = STATISTICS_LOG_INTERVAL_MS;
    (*pCtx).uiLastTimestamp = -1;
    (*pCtx).bDeliveryFlag = true;
    *ppCtx = pCtx;

    0
}

/// `STATISTICS_LOG_INTERVAL_MS` — `wels_const.h`.
pub const STATISTICS_LOG_INTERVAL_MS: i32 = 5000;

/// `FreeSliceInLayer` — encoder_ext.cpp:942.
///
/// # Safety
/// `pDq` and `pMa` must be non-null.
pub unsafe fn FreeSliceInLayer(pDq: *mut SDqLayer, pMa: *mut CMemoryAlign) {
    for iIdx in 0..MAX_THREADS_NUM {
        crate::encoder::svc_encode_slice::FreeSliceBuffer(
            &mut (*pDq).sSliceBufferInfo[iIdx].pSliceBuffer,
            (*pDq).sSliceBufferInfo[iIdx].iMaxSliceNum,
            pMa,
            tag!("pSliceBuffer"),
        );
    }
}

/// `FreeDqLayer` — encoder_ext.cpp:951.
///
/// # Safety
/// `pDq` must have come from `InitDqLayers` and must not be used afterwards.
pub unsafe fn FreeDqLayer(pDq: *mut *mut SDqLayer, pMa: *mut CMemoryAlign) {
    if (*pDq).is_null() {
        return;
    }
    let p = *pDq;

    FreeSliceInLayer(p, pMa);

    // `ppSliceInLayer` is a `Vec<SliceIdx>` since T6.D4 — the layer's own `Drop`
    // releases it when the `Box` below goes.
    // `pFirstMbIdxOfSlice` and `pCountMbNumInSlice` are `Vec<i32>` since T6.D6 — the
    // layer's own `Drop` releases them with the `Box` below.
    crate::encoder::svc_enc_slice_segment::UninitSlicePEncCtx(p, pMa);
    (*p).iMaxSliceNum = 0;

    // The layer is `Box`-built (T6.D3), so its own storage is Rust's to release;
    // the member frees above go one at a time as each member becomes owned.
    drop(Box::from_raw(p));
    *pDq = null_mut();
}

/// `FreeRefList` — encoder_ext.cpp:986.
///
/// # Safety
/// `pRefList` must have come from `InitDqLayers` and must not be used afterwards.
pub unsafe fn FreeRefList(
    pRefList: *mut *mut SRefList,
    pMa: *mut CMemoryAlign,
    iMaxNumRefFrame: i32,
) {
    if (*pRefList).is_null() {
        return;
    }
    let p = *pRefList;

    let mut iRef: i32 = 0;
    loop {
        if !(*p).pRef[iRef as usize].is_null() {
            crate::encoder::wels_preprocess::FreePicture(pMa, &mut (*p).pRef[iRef as usize]);
        }
        iRef += 1;
        if iRef >= 1 + iMaxNumRefFrame {
            break;
        }
    }

    (*pMa).WelsFree(p as *mut c_void, tag!("pRefList"));
    *pRefList = null_mut();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::codec_api::EProfileIdc;
    use crate::encoder::encoder_context::InitFunctionPointers;
    use crate::encoder::param_svc::AllocCodingParam;
    use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

    /// Builds the context up to and including `RequestMemorySvc`, which is everything
    /// `WelsInitEncoderExt` does before the preprocessor. This is the direct test of
    /// baseline blocker C: before this phase `pSpsArray`/`pPPSArray` were never
    /// allocated, `iSpsNum`/`iPpsNum` never assigned and `ppDqLayerList` never filled.
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
        (*pCtx).pMemAlign = Box::into_raw(Box::new(CMemoryAlign::new(iCacheLineSize as u32)));
        assert_eq!(
            AllocCodingParam(&mut (*pCtx).pSvcParam, (*pCtx).pMemAlign),
            0
        );
        *(*pCtx).pSvcParam = param;
        (*pCtx).pFuncList = (*(*pCtx).pMemAlign).WelsMallocz(
            std::mem::size_of::<SWelsFuncPtrList>() as u32,
            tag!("SWelsFuncPtrList"),
        ) as *mut SWelsFuncPtrList;
        assert!(!(*pCtx).pFuncList.is_null());
        assert_eq!(
            InitFunctionPointers(pCtx, (*pCtx).pSvcParam, uiCpuFeatureFlags),
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
    fn request_memory_svc_builds_the_parameter_sets() {
        unsafe {
            let pCtx = build_gate_context();

            assert!(!(*pCtx).pSpsArray.is_null(), "pSpsArray still null");
            assert!(!(*pCtx).pPPSArray.is_null(), "pPPSArray still null");
            assert_eq!((*pCtx).iSpsNum, 1);
            assert_eq!((*pCtx).iPpsNum, 1);
            assert_eq!((*pCtx).iSubsetSpsNum, 0);
            assert_eq!((*pCtx).pSps, (*pCtx).pSpsArray);
            assert_eq!((*pCtx).pPps, (*pCtx).pPPSArray);

            // The SPS the strategy generated must be the one Phase 3 proved
            // byte-exact against the C++ reference for this configuration.
            let sps = &*(*pCtx).pSpsArray;
            assert_eq!(sps.iMbWidth, 10);
            assert_eq!(sps.iMbHeight, 6);
            assert_eq!(sps.uiLog2MaxFrameNum, 15);
            assert_eq!(sps.uiPocType, 2);
            assert_eq!(sps.iLevelIdc, 13);

            let pps = &*(*pCtx).pPPSArray;
            assert_eq!(pps.iPicInitQp, 26);
            assert!(pps.bDeblockingFilterControlPresentFlag);

            let mut p = pCtx;
            WelsUninitEncoderExt(&mut p);
        }
    }

    /// Blocker C, second half: the DQ layers, reference lists and macroblock list
    /// exist, which is what `pCurDqLayer` is selected from.
    #[test]
    fn request_memory_svc_builds_the_dq_layers() {
        unsafe {
            let pCtx = build_gate_context();

            assert!(!(*pCtx).ppDqLayerList.is_null());
            let pDq = *(*pCtx).ppDqLayerList;
            assert!(!pDq.is_null());
            assert_eq!((*pDq).iMbWidth, 10);
            assert_eq!((*pDq).iMbHeight, 6);
            assert_eq!((*pDq).sSliceEncCtx.iMbNumInFrame, 60);
            assert_eq!((*pDq).sSliceEncCtx.iSliceNumInFrame, 1);
            assert_eq!((*pDq).sSliceEncCtx.pOverallMbMap.len(), 60);
            assert_eq!((*pDq).sMbDataP.dims().count(), 60);

            // InitMbInfo wired every macroblock to its slot in the context arrays.
            let pMb = crate::encoder::svc_encode_slice::mb_list_root(pDq);
            assert_eq!((*pMb).iMbXY, 0);
            assert_eq!((*pMb).iMbX, 0);
            assert_eq!((*pMb).iMbY, 0);
            // MB 0 has no left/top neighbour.
            assert_eq!((*pMb).uiNeighborAvail, 0);
            let pMb11 = pMb.add(11); // row 1, column 1: all four neighbours present
            assert_eq!((*pMb11).iMbX, 1);
            assert_eq!((*pMb11).iMbY, 1);
            assert_eq!(
                (*pMb11).uiNeighborAvail,
                LEFT_MB_POS | TOP_MB_POS | TOPLEFT_MB_POS | TOPRIGHT_MB_POS
            );

            assert!(!(*pCtx).ppRefPicListExt.is_null());
            assert!(!(**(*pCtx).ppRefPicListExt).pRef[0].is_null());
            assert_eq!((*pCtx).pDecPic, (**(*pCtx).ppRefPicListExt).pRef[0]);

            assert!(!(*pCtx).pStrideTab.is_null());
            assert!(!(*pCtx).pMvdCostTable.is_null());
            assert_eq!(
                (*pCtx).eRefStrategy,
                crate::encoder::ref_list_mgr_svc::RefStrategyKind::TemporalLayer,
                "the gate configuration is camera content without LTR"
            );

            let mut p = pCtx;
            WelsUninitEncoderExt(&mut p);
        }
    }
}

/// `WelsUninitEncoderExt` — encoder_ext.cpp:2246, with `FreeMemorySvc`
/// (encoder_ext.cpp:1804) folded in.
///
/// # Safety
/// `ppCtx` must point to a context from [`WelsInitEncoderExt`], or be null/point to
/// null.
pub unsafe fn WelsUninitEncoderExt(ppCtx: *mut *mut sWelsEncCtx) {
    if ppCtx.is_null() || (*ppCtx).is_null() {
        return;
    }
    let pCtx = *ppCtx;

    if !(*pCtx).pVpp.is_null() {
        (*(*pCtx).pVpp).FreeSpatialPictures(pCtx);
        drop(Box::from_raw((*pCtx).pVpp));
        (*pCtx).pVpp = null_mut();
    }

    let pMa = (*pCtx).pMemAlign;
    if !pMa.is_null() {
        if !(*pCtx).pStrideTab.is_null() {
            if !(*(*pCtx).pStrideTab).pStrideDecBlockOffset[0][1].is_null() {
                (*pMa).WelsFree(
                    (*(*pCtx).pStrideTab).pStrideDecBlockOffset[0][1] as *mut c_void,
                    tag!("pBase"),
                );
            }
            (*pMa).WelsFree((*pCtx).pStrideTab as *mut c_void, tag!("SStrideTables"));
            (*pCtx).pStrideTab = null_mut();
        }
        if !(*pCtx).pDqIdcMap.is_null() {
            (*pMa).WelsFree((*pCtx).pDqIdcMap as *mut c_void, tag!("pDqIdcMap"));
            (*pCtx).pDqIdcMap = null_mut();
        }
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
        if !(*pCtx).pFrameBs.is_null() {
            (*pMa).WelsFree((*pCtx).pFrameBs as *mut c_void, tag!("pFrameBs"));
            (*pCtx).pFrameBs = null_mut();
        }
        for pBuf in (*pCtx).pDynamicBsBuffer.iter_mut() {
            if !pBuf.is_null() {
                (*pMa).WelsFree(*pBuf as *mut c_void, tag!("DynamicSliceBs"));
                *pBuf = null_mut();
            }
        }
        if !(*pCtx).pSpsArray.is_null() {
            (*pMa).WelsFree((*pCtx).pSpsArray as *mut c_void, tag!("pSpsArray"));
            (*pCtx).pSpsArray = null_mut();
        }
        if !(*pCtx).pPPSArray.is_null() {
            (*pMa).WelsFree((*pCtx).pPPSArray as *mut c_void, tag!("pPPSArray"));
            (*pCtx).pPPSArray = null_mut();
        }
        if !(*pCtx).pSubsetArray.is_null() {
            (*pMa).WelsFree((*pCtx).pSubsetArray as *mut c_void, tag!("pSubsetArray"));
            (*pCtx).pSubsetArray = null_mut();
        }
        // The five per-macroblock arrays freed here in encoder_ext.cpp:1932-1961 are
        // inline in `SMB` since T6.C1 and go with the `SMB` list below.
        // `ppMbListD` is gone: each layer owns its own `MbArray<SMB>` (T6.D5).
        if !(*pCtx).pMvdCostTable.is_null() {
            (*pMa).WelsFree((*pCtx).pMvdCostTable as *mut c_void, tag!("pMvdCostTable"));
            (*pCtx).pMvdCostTable = null_mut();
        }
        // rate control module memory free. encoder_ext.cpp:1982 calls WelsRcFreeMemory
        // *before* releasing pWelsSvcRc itself: the per-layer pTemporalOverRc blocks hang
        // off the array being freed here, so dropping the array first leaks them.
        if !(*pCtx).pWelsSvcRc.is_null() {
            crate::encoder::rc::WelsRcFreeMemory(pCtx);
            (*pMa).WelsFree((*pCtx).pWelsSvcRc as *mut c_void, tag!("pWelsSvcRc"));
            (*pCtx).pWelsSvcRc = null_mut();
        }
        if !(*pCtx).pLtr.is_null() {
            (*pMa).WelsFree((*pCtx).pLtr as *mut c_void, tag!("SLTRState"));
            (*pCtx).pLtr = null_mut();
        }
        // DQ layers list
        if !(*pCtx).ppDqLayerList.is_null() && !(*pCtx).pSvcParam.is_null() {
            for ilayer in 0..(*(*pCtx).pSvcParam).iSpatialLayerNum as usize {
                if !(*(*pCtx).ppDqLayerList.add(ilayer)).is_null() {
                    FreeDqLayer((*pCtx).ppDqLayerList.add(ilayer), pMa);
                }
            }
            (*pMa).WelsFree((*pCtx).ppDqLayerList as *mut c_void, tag!("ppDqLayerList"));
            (*pCtx).ppDqLayerList = null_mut();
        }
        // reference picture list extension
        if !(*pCtx).ppRefPicListExt.is_null() && !(*pCtx).pSvcParam.is_null() {
            for ilayer in 0..(*(*pCtx).pSvcParam).iSpatialLayerNum as usize {
                FreeRefList(
                    (*pCtx).ppRefPicListExt.add(ilayer),
                    pMa,
                    (*(*pCtx).pSvcParam).iMaxNumRefFrame,
                );
            }
            (*pMa).WelsFree((*pCtx).ppRefPicListExt as *mut c_void, tag!("ppRefPicListExt"));
            (*pCtx).ppRefPicListExt = null_mut();
        }
        if !(*pCtx).pVaa.is_null() {
            let pVaa = (*pCtx).pVaa;
            if !(*pVaa).pVaaBackgroundMbFlag.is_null() {
                (*pMa).WelsFree(
                    (*pVaa).pVaaBackgroundMbFlag as *mut c_void,
                    tag!("pVaa->pVaaBackgroundMbFlag"),
                );
            }
            if !(*pVaa).sVaaCalcInfo.pSad8x8.is_null() {
                (*pMa).WelsFree(
                    (*pVaa).sVaaCalcInfo.pSad8x8 as *mut c_void,
                    tag!("pVaa->sVaaCalcInfo.sad8x8"),
                );
            }
            if !(*pVaa).sVaaCalcInfo.pSsd16x16.is_null() {
                (*pMa).WelsFree(
                    (*pVaa).sVaaCalcInfo.pSsd16x16 as *mut c_void,
                    tag!("pVaa->sVaaCalcInfo.pSsd16x16"),
                );
            }
            if !(*pVaa).sVaaCalcInfo.pSum16x16.is_null() {
                (*pMa).WelsFree(
                    (*pVaa).sVaaCalcInfo.pSum16x16 as *mut c_void,
                    tag!("pVaa->sVaaCalcInfo.pSum16x16"),
                );
            }
            if !(*pVaa).sVaaCalcInfo.pSumOfSquare16x16.is_null() {
                (*pMa).WelsFree(
                    (*pVaa).sVaaCalcInfo.pSumOfSquare16x16 as *mut c_void,
                    tag!("pVaa->sVaaCalcInfo.pSumOfSquare16x16"),
                );
            }
            if !(*pCtx).pSvcParam.is_null() && (*(*pCtx).pSvcParam).bEnableBackgroundDetection {
                if !(*pVaa).sVaaCalcInfo.pSumOfDiff8x8.is_null() {
                    (*pMa).WelsFree(
                        (*pVaa).sVaaCalcInfo.pSumOfDiff8x8 as *mut c_void,
                        tag!("pVaa->sVaaCalcInfo.pSumOfDiff8x8"),
                    );
                }
                if !(*pVaa).sVaaCalcInfo.pMad8x8.is_null() {
                    (*pMa).WelsFree(
                        (*pVaa).sVaaCalcInfo.pMad8x8 as *mut c_void,
                        tag!("pVaa->sVaaCalcInfo.pMad8x8"),
                    );
                }
            }
            (*pMa).WelsFree(pVaa as *mut c_void, tag!("pVaa"));
            (*pCtx).pVaa = null_mut();
        }
        if !(*pCtx).pSvcParam.is_null() {
            let _ = crate::encoder::param_svc::FreeCodingParam(&mut (*pCtx).pSvcParam, pMa);
        }
        if !(*pCtx).pFuncList.is_null() {
            // F19: this `take()` is `encoder_ext.cpp:1995`'s
            // `WELS_DELETE_OP (pCtx->pFuncList->pParametersetStrategy)`, which the
            // port had no counterpart for — the strategy object was `Box::into_raw`'d
            // at init and the table `WelsFree`'d out from under it, leaking it on
            // every teardown. The table is raw-allocated, so `SWelsFuncPtrList`'s own
            // drop glue never runs and the owned field has to be taken by hand.
            drop((*(*pCtx).pFuncList).pParametersetStrategy.take());
            (*pMa).WelsFree((*pCtx).pFuncList as *mut c_void, tag!("SWelsFuncPtrList"));
            (*pCtx).pFuncList = null_mut();
        }
        drop(Box::from_raw(pMa));
        (*pCtx).pMemAlign = null_mut();
    }

    drop(Box::from_raw(pCtx));
    *ppCtx = null_mut();
}

// ============================================================================
// The encoding half of encoder_ext.cpp: WelsEncoderEncodeExt and its helpers.
//
// Translated statement for statement from `codec/encoder/core/src/encoder_ext.cpp`.
// Line references in the doc comments are to that file.
// ============================================================================

/// `encoder_ext.cpp:2393`.
pub unsafe fn GetTemporalLevel(
    fDlp: *mut SSpatialLayerInternal,
    kiFrameNum: i32,
    kiGopSize: i32,
) -> i32 {
    let kiCodingIdx = kiFrameNum & (kiGopSize - 1);
    (*fDlp).uiCodingIdx2TemporalId[kiCodingIdx as usize] as i32
}

/// `encoder_ext.cpp:3114`.
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
pub unsafe fn WelsSwapDqLayers(pCtx: *mut sWelsEncCtx, kiNextDqIdx: i32) {
    let pTmpLayer = *(*pCtx).ppDqLayerList.add(kiNextDqIdx as usize);
    // The outgoing layer's *position*, not its address — T6.D3. It carries its own
    // index (`iDqIdx`, stamped at construction) precisely because this is the one
    // site that has to name a layer it holds only a pointer to.
    let kRefIdx = (*(*pCtx).pCurDqLayer).iDqIdx;
    (*pCtx).pCurDqLayer = pTmpLayer;
    (*(*pCtx).pCurDqLayer).pRefLayer = Some(kRefIdx);
}

/// `encoder_ext.cpp:2808`. Prefetch the reference picture after `WelsBuildRefList`.
pub unsafe fn PrefetchReferencePicture(pCtx: *mut sWelsEncCtx, keFrameType: EVideoFrameType) {
    let kiSliceCount = (*(*pCtx).pCurDqLayer).iMaxSliceNum;
    // C++ declares `uint8_t uiRefIdx = -1;`, which wraps to 255.
    let mut uiRefIdx: u8 = 0xff;

    debug_assert!(kiSliceCount > 0);
    if keFrameType != EVideoFrameType::videoFrameTypeIDR {
        debug_assert!((*pCtx).iNumRef0 > 0);
        // always get item 0 due to reordering done
        (*pCtx).pRefPic = (*pCtx).pRefList0[0];
        (*(*pCtx).pCurDqLayer).pRefPic = (*pCtx).pRefPic;
        uiRefIdx = 0; // reordered reference index
    } else {
        // safe for IDR coding
        (*pCtx).pRefPic = null_mut();
        (*(*pCtx).pCurDqLayer).pRefPic = null_mut();
    }

    let mut iIdx = 0;
    while iIdx < kiSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer((*pCtx).pCurDqLayer, iIdx);
        if !pSlice.is_null() {
            (*pSlice).sSliceHeaderExt.sSliceHeader.uiRefIndex = uiRefIdx;
        }
        iIdx += 1;
    }
}

/// `encoder_ext.cpp:3376`.
pub unsafe fn ClearFrameBsInfo(pCtx: *mut sWelsEncCtx, pFbi: *mut SFrameBSInfo) {
    (*pFbi).sLayerInfo[0].pBsBuf = (*pCtx).pFrameBs;
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
pub unsafe fn StackBackEncoderStatus(pEncCtx: *mut sWelsEncCtx, keFrameType: EVideoFrameType) {
    let pParamInternal = (*(*pEncCtx).pSvcParam)
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
            (*pParamInternal).iPOC = (1 << (*(*pEncCtx).pSps).iLog2MaxPocLsb) - 2;
        }

        crate::encoder::encoder_context::LoadBackFrameNum(pEncCtx, (*pEncCtx).uiDependencyId as i32);

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        (*pEncCtx).eSliceType = EWelsSliceType::P_SLICE;
        // eNalPriority is not stacked back: it is updated at the start of coding a frame.
    } else if keFrameType == EVideoFrameType::videoFrameTypeIDR {
        (*pParamInternal).uiIdrPicId -= 1;
        // set the next frame to be IDR
        crate::encoder::wels_encoder_ext::ForceCodingIDR(pEncCtx, (*pEncCtx).uiDependencyId as i32);
    } else {
        // B pictures are not supported now
        debug_assert!(false, "StackBackEncoderStatus: unsupported frame type");
    }

    // No need to stack back RC info -- it is still useful for later RQ model
    // calculation -- nor MB slicing info for dynamic balancing.
}

/// `encoder_ext.cpp:2534`. Bind the current DQ layer to this frame's parameter sets,
/// NAL header and picture buffers.
pub unsafe fn WelsInitCurrentLayer(pCtx: *mut sWelsEncCtx, _kiWidth: i32, _kiHeight: i32) {
    let pParam = (*pCtx).pSvcParam;
    let pEncPic = (*pCtx).pEncPic;
    let pDecPic = (*pCtx).pDecPic;
    let pCurDq = (*pCtx).pCurDqLayer;
    if pCurDq.is_null() {
        return;
    }
    let pBaseSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, 0);
    if pBaseSlice.is_null() {
        return;
    }
    let kiCurDid = (*pCtx).uiDependencyId;
    let kbUseSubsetSpsFlag = !(*pParam).bSimulcastAVC && (kiCurDid as i32) > BASE_DEPENDENCY_ID;
    let pNalHdExt = &mut (*pCurDq).sLayerInfo.sNalHeaderExt;
    let pDqIdc = (*pCtx).pDqIdcMap.add(kiCurDid as usize);
    let iSliceCount = (*pCurDq).iMaxSliceNum;
    // S29 / F13's family: `addr_of_mut!` on the element, not `as_mut_ptr().add()` —
    // the latter reborrows the whole array and a second such derivation pops the first.
    let pParamInternal = std::ptr::addr_of_mut!((*pParam).sDependencyLayers[kiCurDid as usize]);

    (*pCurDq).pDecPic = pDecPic;

    debug_assert!(iSliceCount > 0);

    let mut iCurPpsId = (*pDqIdc).iPpsId as i32;
    let iCurSpsId = (*pDqIdc).iSpsId as i32;

    iCurPpsId = ParasetStrategy(pCtx).GetCurrentPpsId(
        iCurPpsId,
        ((*pParamInternal).uiIdrPicId as i32 - 1).abs() % MAX_PPS_COUNT as i32,
    );

    (*pBaseSlice).sSliceHeaderExt.sSliceHeader.iPpsId = iCurPpsId;
    (*pCurDq).sLayerInfo.pPpsP = (*pCtx).pPPSArray.add(iCurPpsId as usize);
    (*pBaseSlice).sSliceHeaderExt.sSliceHeader.pPps = (*pCurDq).sLayerInfo.pPpsP;

    (*pBaseSlice).sSliceHeaderExt.sSliceHeader.iSpsId = iCurSpsId;
    if kbUseSubsetSpsFlag {
        (*pCurDq).sLayerInfo.pSubsetSpsP = (*pCtx).pSubsetArray.add(iCurSpsId as usize);
        (*pCurDq).sLayerInfo.pSpsP = std::ptr::addr_of_mut!((*(*pCurDq).sLayerInfo.pSubsetSpsP).pSps);
        (*pBaseSlice).sSliceHeaderExt.sSliceHeader.pSps = (*pCurDq).sLayerInfo.pSpsP;
    } else {
        (*pCurDq).sLayerInfo.pSubsetSpsP = null_mut();
        (*pCurDq).sLayerInfo.pSpsP = (*pCtx).pSpsArray.add(iCurSpsId as usize);
        (*pBaseSlice).sSliceHeaderExt.sSliceHeader.pSps = (*pCurDq).sLayerInfo.pSpsP;
    }

    (*pBaseSlice).bSliceHeaderExtFlag =
        (*pCtx).eNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;

    let mut iIdx = 1;
    while iIdx < iSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iIdx);
        if !pSlice.is_null() {
            crate::encoder::svc_encode_slice::InitSliceHeadWithBase(pSlice, pBaseSlice);
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

    // pEncPic data
    (*pCurDq).pEncData[0] = (*pEncPic).pData[0];
    (*pCurDq).pEncData[1] = (*pEncPic).pData[1];
    (*pCurDq).pEncData[2] = (*pEncPic).pData[2];
    (*pCurDq).iEncStride[0] = (*pEncPic).iLineSize[0];
    (*pCurDq).iEncStride[1] = (*pEncPic).iLineSize[1];
    (*pCurDq).iEncStride[2] = (*pEncPic).iLineSize[2];
    // cs data
    (*pCurDq).pCsData[0] = (*pDecPic).pData[0];
    (*pCurDq).pCsData[1] = (*pDecPic).pData[1];
    (*pCurDq).pCsData[2] = (*pDecPic).pData[2];
    (*pCurDq).iCsStride[0] = (*pDecPic).iLineSize[0];
    (*pCurDq).iCsStride[1] = (*pDecPic).iLineSize[1];
    (*pCurDq).iCsStride[2] = (*pDecPic).iLineSize[2];

    (*pCurDq).bBaseLayerAvailableFlag = (*pCurDq).pRefLayer.is_some();

    if !(*pCtx).pTaskManage.is_null() {
        let pTaskManage =
            (*pCtx).pTaskManage as *mut crate::encoder::wels_task_management::CWelsTaskManageBase;
        (*pTaskManage).InitFrame(kiCurDid as i32);
    }
}

/// `encoder_ext.cpp:2954`. Emit the SVC prefix NAL that precedes each VCL NAL when
/// `bNeedPrefixNalFlag` is set.
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
        Some(&(*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt),
        (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize),
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
        (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize),
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
unsafe fn SetFastCodingFunc(pFuncList: *mut SWelsFuncPtrList) {
    (*pFuncList).pfIntraFineMd =
        Some(crate::encoder::svc_base_layer_md::WelsMdIntraFinePartitionVaa);
    let sdf = &mut (*pFuncList).sSampleDealingFuncs;
    sdf.pfMdCost = CostFamily::Sad;
    // The C++ also aims three `pfIntra*Combined3` slots at their `*Sad` twins here;
    // both sides were NULL on every target and the fields are deleted (S18).
}

/// `encoder_ext.cpp:2630` (`static inline SetNormalCodingFunc`).
unsafe fn SetNormalCodingFunc(pFuncList: *mut SWelsFuncPtrList) {
    (*pFuncList).pfIntraFineMd = Some(crate::encoder::svc_base_layer_md::WelsMdIntraFinePartition);
    let sdf = &mut (*pFuncList).sSampleDealingFuncs;
    sdf.pfMdCost = CostFamily::Satd;
    // As `SetFastCodingFunc`: the three `Combined3` aims are deleted with the fields.
}

/// `encoder_ext.cpp:2643`. Returns false when the requested method has no dedicated
/// search and the caller falls back to the diamond search.
pub unsafe fn SetMeMethod(uiMethod: u32, pSearchMethodFunc: *mut Option<PSearchMethodFunc>) -> bool {
    match uiMethod {
        ME_DIA => {
            *pSearchMethodFunc = Some(crate::encoder::svc_motion_estimate::WelsDiamondSearch);
            true
        }
        ME_CROSS => {
            *pSearchMethodFunc = Some(crate::encoder::svc_motion_estimate::WelsMotionCrossSearch);
            true
        }
        ME_DIA_CROSS => {
            *pSearchMethodFunc = Some(crate::encoder::svc_motion_estimate::WelsDiamondCrossSearch);
            true
        }
        ME_DIA_CROSS_FME => {
            *pSearchMethodFunc =
                Some(crate::encoder::svc_motion_estimate::WelsDiamondCrossFeatureSearch);
            true
        }
        ME_FULL => {
            *pSearchMethodFunc = Some(crate::encoder::svc_motion_estimate::WelsDiamondSearch);
            false
        }
        _ => {
            *pSearchMethodFunc = Some(crate::encoder::svc_motion_estimate::WelsDiamondSearch);
            false
        }
    }
}

/// `encoder_ext.cpp:2665`. Per-frame function-pointer selection. MUST be called after
/// `pfWelsRcPictureInit()` and `WelsInitCurrentLayer()`.
///
/// The `SCREEN_CONTENT_REAL_TIME` block (`encoder_ext.cpp:2708-2771`) is the only part
/// not translated; see the comment at its position below.
pub unsafe fn PreprocessSliceCoding(pCtx: *mut sWelsEncCtx) {
    let pCurLayer = (*pCtx).pCurDqLayer;
    let bFastMode = (*(*pCtx).pSvcParam).iComplexityMode == LOW_COMPLEXITY;
    let pFuncList = (*pCtx).pFuncList;

    // function pointers conditional assignment under sWelsEncCtx
    if ((*(*pCtx).pSvcParam).iUsageType == CAMERA_VIDEO_REAL_TIME && bFastMode)
        || ((*(*pCtx).pSvcParam).iUsageType == SCREEN_CONTENT_REAL_TIME
            && (*pCtx).eSliceType == EWelsSliceType::P_SLICE
            && bFastMode)
    {
        SetFastCodingFunc(pFuncList);
    } else {
        SetNormalCodingFunc(pFuncList);
    }

    if (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
        for i in 0..EStaticBlockIdc::BLOCK_STATIC_IDC_ALL as usize {
            (*pFuncList).pfMotionSearch[i] =
                Some(crate::encoder::svc_motion_estimate::WelsMotionEstimateSearch);
        }
        for b in [
            BLOCK_16x16, BLOCK_16x8, BLOCK_8x16, BLOCK_8x8, BLOCK_4x4, BLOCK_8x4, BLOCK_4x8,
        ] {
            (*pFuncList).pfSearchMethod[b] =
                Some(crate::encoder::svc_motion_estimate::WelsDiamondSearch);
        }
        (*pFuncList).pfFirstIntraMode =
            Some(crate::encoder::svc_base_layer_md::WelsMdFirstIntraMode);
        let sdf = &mut (*pFuncList).sSampleDealingFuncs;
        sdf.pfMeCost = CostFamily::Satd;
        (*pFuncList).pfSetScrollingMv =
            Some(crate::encoder::svc_mode_decision::SetScrollingMvToMdNull);

        if bFastMode {
            (*pFuncList).pfCalculateSatd =
                Some(crate::encoder::svc_motion_estimate::NotCalculateSatdCost);
            (*pFuncList).pfInterFineMd =
                Some(crate::encoder::svc_base_layer_md::WelsMdInterFinePartitionVaa);
        } else {
            (*pFuncList).pfCalculateSatd =
                Some(crate::encoder::svc_motion_estimate::CalculateSatdCost);
            (*pFuncList).pfInterFineMd =
                Some(crate::encoder::svc_base_layer_md::WelsMdInterFinePartition);
        }
    } else {
        (*pFuncList).sSampleDealingFuncs.pfMeCost = CostFamily::Unset;
    }

    // The SCREEN_CONTENT_REAL_TIME block of the C++ (encoder_ext.cpp:2708-2771) sets up
    // feature-based motion search. It is outside the Phase-5 gate configuration
    // (CAMERA_VIDEO_REAL_TIME) and depends on the unported mode-decision layer for
    // pfInterFineMd, so it is not translated here.

    // update some layer-dependent variables to save judgements at MB level
    let sdf = &(*pFuncList).sSampleDealingFuncs;
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
    let pDep = &(*(*pCtx).pSvcParam).sDependencyLayers[kiCurDid];
    if (*pCurLayer).bDeblockingParallelFlag
        && (*pCurLayer).iLoopFilterDisableIdc != 1
        // ENABLE_FRAME_DUMP is not defined, so this clause is compiled in.
        && (*pCtx).eNalPriority != EWelsNalRefIdc::NRI_PRI_LOWEST
        && (pDep.iHighestTemporalId == 0 || kiCurTid < pDep.iHighestTemporalId as i32)
    {
        (*pFuncList).pfDeblocking.pfDeblockingFilterSlice =
            Some(crate::encoder::deblocking::DeblockingFilterSliceAvcbase);
    } else {
        (*pFuncList).pfDeblocking.pfDeblockingFilterSlice =
            Some(crate::encoder::deblocking::DeblockingFilterSliceAvcbaseNull);
    }
}

/// `encoder_ext.cpp:3131`. Write the parameter sets for (simulcast) SVC.
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
        let pParamInternal = std::ptr::addr_of_mut!((*(*pCtx).pSvcParam).sDependencyLayers[iSpatialId]);
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
    (*pNext).pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
    (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);

    // update for external countings
    *iLayerNum += 1;
    *iFrameSize += iNonVclSize;
    iReturn
}

/// `encoder_ext.cpp:3163`. Write the parameter sets for simulcast AVC.
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
    if let Some(pStrategy) = (*(*pCtx).pFuncList).pParametersetStrategy.as_mut() {
        pStrategy.Update(
            (*(*pCtx).pSpsArray.add(iIdx as usize)).uiSpsId,
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
    (*pNext).pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
    (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);
    *iLayerNum += 1;
    pLayerBsInfo = pNext;

    // --- PPS ---
    iNalSize = 0;
    if let Some(pStrategy) = (*(*pCtx).pFuncList).pParametersetStrategy.as_mut() {
        pStrategy.Update(
            (*(*pCtx).pPPSArray.add(iIdx as usize)).iPpsId,
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
    (*pNext).pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
    (*pNext).pNalLengthInByte = (*pLayerBsInfo).pNalLengthInByte.add(iCountNal as usize);
    *iLayerNum += 1;

    *ppLayerBsInfo = pNext;
    *iFrameSize += iNonVclSize;
    ENC_RETURN_SUCCESS
}

/// `encoder_ext.cpp:3387`. Decide this frame's type, and for an IDR write the
/// parameter sets ahead of the slice data.
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
    let pSvcParam = (*pCtx).pSvcParam;
    let pSpatialIndexMap = (*pCtx).sSpatialIndexMap.as_ptr();

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
        let pfRc = (*(*pCtx).pFuncList).pfRc;
        if (*pSvcParam).bSimulcastAVC {
            pfRc.WelsUpdateBufferWhenSkip(pCtx, *iCurDid as i32);
        } else {
            for i in 0..iSpatialNum as usize {
                pfRc.WelsUpdateBufferWhenSkip(pCtx, (*pSpatialIndexMap.add(i)).iDid);
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
            if ((*(*pCtx).pSvcParam).eSpsPpsIdStrategy as i32
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
                // WriteSavcParaset_Listing covers the three SPS_LISTING strategies, which
                // CreateParametersetStrategy deliberately does not construct (it returns
                // null rather than falling through to CONSTANT_ID). Reaching here means a
                // listing strategy was configured, which this port does not support.
                (*pCtx).iEncoderError = ENC_RETURN_UNSUPPORTED_PARA;
            }
        }
    }
    eFrameType
}

/// `encoder_ext.cpp:2415`. TUNE back if a picture-partition decision algorithm based
/// on past behaviour becomes available.
pub unsafe fn PicPartitionNumDecision(pCtx: *mut sWelsEncCtx) -> i32 {
    let mut iPartitionNum = 1;
    if (*(*pCtx).pSvcParam).iMultipleThreadIdc > 1 {
        iPartitionNum = (*(*pCtx).pSvcParam).iMultipleThreadIdc as i32;
    }
    iPartitionNum
}

/// `DynslcUpdateMbNeighbourInfoListForAllSlices` — encoder_ext.cpp:2397.
///
/// # Safety
/// `pCurDq` must be live with `sMbDataP` allocated.
pub unsafe fn DynslcUpdateMbNeighbourInfoListForAllSlices(pCurDq: *mut SDqLayer, pMbList: *mut SMB) {
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let kiMbWidth = pSliceCtx.iMbWidth as i32;
    let kiEndMbInSlice = pSliceCtx.iMbNumInFrame - 1;
    let mut iIdx = 0i32;

    loop {
        let pMb = pMbList.add(iIdx as usize);
        let uiSliceIdc =
            crate::encoder::svc_encode_slice::WelsMbToSliceIdc(pCurDq, (*pMb).iMbXY as i32);
        crate::encoder::svc_encode_slice::UpdateMbNeighbor(pCurDq, pMb, kiMbWidth, uiSliceIdc);
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
pub unsafe fn WelsInitCurrentQBLayerMltslc(pCtx: *mut sWelsEncCtx) {
    // pData init
    let pCurDq = (*pCtx).pCurDqLayer;
    // mb_neighbor
    DynslcUpdateMbNeighbourInfoListForAllSlices(pCurDq, crate::encoder::svc_encode_slice::mb_list_root(pCurDq));
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
pub unsafe fn UpdateSlicepEncCtxWithPartition(pCurDq: *mut SDqLayer, mut iPartitionNum: i32) {
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

    pSliceCtx.iSliceNumInFrame = iPartitionNum;

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
            let map: &mut Vec<u16> = &mut (*pCurDq).sSliceEncCtx.pOverallMbMap;
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
pub unsafe fn WelsInitCurrentDlayerMltslc(pCtx: *mut sWelsEncCtx, iPartitionNum: i32) {
    /// `#define byte_complexIMBat26 (60)`, local to this function in the C++.
    const byte_complexIMBat26: u32 = 60;

    let pCurDq = (*pCtx).pCurDqLayer;

    UpdateSlicepEncCtxWithPartition(pCurDq, iPartitionNum);

    if (*pCtx).eSliceType == EWelsSliceType::I_SLICE {
        // check if uiSliceSizeConstraint too small
        let iCurDid = (*pCtx).uiDependencyId as usize;
        let mut uiFrmByte: u32;

        if (*(*pCtx).pSvcParam).iRCMode != crate::RCMode::RC_OFF_MODE {
            // RC case
            uiFrmByte = (((*(*pCtx).pSvcParam).sSpatialLayers[iCurDid].iSpatialBitrate as u32)
                / ((*(*pCtx).pSvcParam).sDependencyLayers[iCurDid].fInputFrameRate as u32))
                >> 3;
        } else {
            // fixed QP case
            let iTtlMbNumInFrame = (*pCurDq).sSliceEncCtx.iMbNumInFrame;
            let mut iQDeltaTo26 = 26 - (*(*pCtx).pSvcParam).sSpatialLayers[iCurDid].iDLayerQp;

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
pub unsafe fn DynSliceRealloc(
    pCtx: *mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
) -> i32 {
    let mut iRet = crate::encoder::svc_encode_slice::FrameBsRealloc(
        pCtx,
        pFrameBsInfo,
        pLayerBsInfo,
        (*(*pCtx).pCurDqLayer).iMaxSliceNum,
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
    let pCurLayer = (*pCtx).pCurDqLayer;
    let uSlcBuffIdx = 0usize;
    let pStartSlice = (*pCurLayer).sSliceBufferInfo[uSlcBuffIdx]
        .pSliceBuffer
        .add(iStartSliceIdx as usize);
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
        let pCurSlice = (*(*pCtx).pCurDqLayer).sSliceBufferInfo[uSlcBuffIdx]
            .pSliceBuffer
            .add(iSliceIdx as usize);
        (*pCurSlice).iSliceIdx = iSliceIdx;

        iReturn = crate::encoder::svc_encode_slice::WelsCodeOneSlice(
            pCtx,
            pCurSlice,
            keNalType as i32,
        );
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

        iReturn = crate::encoder::nal_encap::WelsEncodeNal(
            &(&*(*pCtx).pOut).sNalList[((*(*pCtx).pOut).iNalIndex - 1) as usize],
            &(&*(*pCtx).pOut).sBsBuffer[..],
            Some(&(*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt),
            (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize),
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
/// # Unported branches
///
/// Each of these returns an explicit error rather than falling through:
/// * `iMultipleThreadIdc > 1` — every multi-threaded slice path needs `pTaskManage`,
///   `InitAllSlicesInThread` and `SliceLayerInfoUpdate`, none of which are ported.
/// * `SM_SIZELIMITED_SLICE` — needs `WelsCodeOnePicPartition` and
///   `WelsInitCurrentDlayerMltslc`.
///
/// # Safety
/// `pCtx` must be a context built by [`WelsInitEncoderExt`].
pub unsafe fn WelsEncoderEncodeExt(
    pCtx: *mut sWelsEncCtx,
    pFbi: *mut SFrameBSInfo,
    pSrcPic: *const SSourcePicture,
) -> i32 {
    if pCtx.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    let pSvcParam = (*pCtx).pSvcParam;
    let mut fsnr: *mut SPicture;
    let mut pEncPic: *mut SPicture;
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

    // Derived after the reset loop above, for the same reason `pSpatialIndexMap`
    // is derived after `BuildSpatialPicList`: that loop **writes**
    // `(*pFbi).sLayerInfo[..]` through `pFbi`, and a write through the parent pops
    // a child taken before it. Every use of this cursor is below.
    let mut pLayerBsInfo: *mut SLayerBSInfo = (*pFbi).sLayerInfo.as_mut_ptr();

    // perform csc/denoise/downsample/padding, generate spatial layers
    let iRet = (*(*pCtx).pVpp).BuildSpatialPicList(pCtx, pSrcPic, &mut iSpatialNum);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    (*(*pCtx).pFuncList)
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

    // Derived here rather than at the top of the function, and the order is the
    // fix: `BuildSpatialPicList` above **writes** this array (through
    // `wels_preprocess.rs`'s `(*pEncCtx).sSpatialIndexMap[idx].iDid = ...`), and a
    // write through the parent pops a `SharedReadOnly` child taken before it. S29's
    // own boundary clause — the spelling rescues derivations through a raw parent,
    // but only ordering rescues one the parent then writes through. Every use of
    // this pointer is below. Found by the encoder aliasing probe, Phase 6 session A.
    let pSpatialIndexMap = (*pCtx).sSpatialIndexMap.as_ptr();
    crate::encoder::encoder_context::InitBitStream(pCtx);
    (*pLayerBsInfo).pBsBuf = (*pCtx).pFrameBs;
    (*pLayerBsInfo).pNalLengthInByte = (*(*pCtx).pOut).sNalLen.as_mut_ptr();
    iCurDid = (*pSpatialIndexMap).iDid as i8;
    (*pCtx).pCurDqLayer = *(*pCtx).ppDqLayerList.add(iCurDid as usize);
    (*(*pCtx).pCurDqLayer).pRefLayer = None;

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
        iCurDid = (*pSpatialIndexMap.add(iSpatialIdx as usize)).iDid as i8;
        // S29 / F13's family (the encode probe's sixth red, session B): `addr_of_mut!`
        // on the element — `as_mut_ptr().add()` reborrowed the whole array, and the
        // `.iPOC` reads below re-derived it and popped these.
        let pParam: *mut SSpatialLayerConfig =
            std::ptr::addr_of_mut!((*pSvcParam).sSpatialLayers[iCurDid as usize]);
        let pParamInternal =
            std::ptr::addr_of_mut!((*pSvcParam).sDependencyLayers[iCurDid as usize]);
        let iDecompositionStages = (*pParamInternal).iDecompositionStages as i32;
        (*pCtx).pCurDqLayer = *(*pCtx).ppDqLayerList.add(iCurDid as usize);
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
        pEncPic = (*pSpatialIndexMap.add(iSpatialIdx as usize)).pSrc;
        (*pCtx).pEncPic = pEncPic;
        (*pEncPic).iPictureType = (*pCtx).eSliceType as i32;
        (*pEncPic).iFramePoc = (*pSvcParam).sDependencyLayers[iCurDid as usize].iPOC;

        iCurWidth = (*pParam).iVideoWidth;
        iCurHeight = (*pParam).iVideoHeight;

        match (*pParam).sSliceArgument.uiSliceMode {
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

        (*pCtx).pDecPic = (**(*pCtx).ppRefPicListExt.add(iCurDid as usize)).pNextBuffer;
        fsnr = (*pCtx).pDecPic;
        (*(*pCtx).pDecPic).iPictureType = (*pCtx).eSliceType as i32;
        (*(*pCtx).pDecPic).iFramePoc = (*pSvcParam).sDependencyLayers[iCurDid as usize].iPOC;

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
                null_mut()
            };
            (*(*pCtx).pVpp).AnalyzePictureComplexity(
                pCtx,
                (*pCtx).pEncPic,
                pRef,
                iCurDid as i32,
                (*pCtx).eSliceType == EWelsSliceType::P_SLICE
                    && (*pSvcParam).bEnableBackgroundDetection,
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
        (*(*pCtx).pFuncList)
            .pfRc
            .WelsRcPictureInit(pCtx, (*pFbi).uiTimeStamp);
        // MUST be called after pfWelsRcPictureInit() and WelsInitCurrentLayer()
        PreprocessSliceCoding(pCtx);

        iLayerSize = 0;
        if (*pParam).sSliceArgument.uiSliceMode == SM_SINGLE_SLICE {
            // only one slice within a quality layer
            let mut iPayloadSize = 0i32;
            let pCurSlice = (*(*pCtx).pCurDqLayer).sSliceBufferInfo[0].pSliceBuffer;

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
                (*pCtx).pCurDqLayer,
                pCurSlice,
                0,
            );
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }

            (*pCtx).iEncoderError =
                crate::encoder::svc_encode_slice::WelsCodeOneSlice(pCtx, pCurSlice, eNalType as i32);
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }

            crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

            (*pCtx).iEncoderError = crate::encoder::nal_encap::WelsEncodeNal(
                &(&*(*pCtx).pOut).sNalList[(*(*pCtx).pOut).iNalIndex as usize - 1],
                &(&*(*pCtx).pOut).sBsBuffer[..],
                Some(&(*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt),
                (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize),
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
            let kiLastMbInFrame = (*(*pCtx).pCurDqLayer).sSliceEncCtx.iMbNumInFrame;
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
                crate::encoder::svc_encode_slice::GetCurrentSliceNum((*pCtx).pCurDqLayer);
            if iLayerNum + 1 >= MAX_LAYER_NUM_OF_FRAME as i32 {
                // check available layer_bs_info for further writing as followed
                return ENC_RETURN_UNSUPPORTED_PARA;
            }
            if iSliceCount <= 1 {
                return ENC_RETURN_UNEXPECTED;
            }
            //note: the old codes are removed at commit: 3e0ee69
            (*pLayerBsInfo).pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
            (*pLayerBsInfo).uiLayerType = VIDEO_CODING_LAYER;
            (*pLayerBsInfo).uiSpatialId = (*pCtx).uiDependencyId;
            (*pLayerBsInfo).uiTemporalId = (*pCtx).uiTemporalId;
            (*pLayerBsInfo).uiQualityId = 0;
            (*pLayerBsInfo).iNalCount = 0;
            (*pLayerBsInfo).eFrameType = eFrameType;
            (*pLayerBsInfo).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);

            let pTaskManage = (*pCtx).pTaskManage
                as *mut crate::encoder::wels_task_management::CWelsTaskManageBase;
            (*pTaskManage)
                .ExecuteTasks(crate::encoder::wels_task_management::WELS_ENC_TASK_ENCODING);
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
            let pLbi = &mut (*pFbi).sLayerInfo[iLayerBsIdx as usize] as *mut SLayerBSInfo;
            (*pLbi).pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
            (*pLbi).uiLayerType = VIDEO_CODING_LAYER;
            (*pLbi).uiSpatialId = (*pCtx).uiDependencyId;
            (*pLbi).uiTemporalId = (*pCtx).uiTemporalId;
            (*pLbi).uiQualityId = 0;
            (*pLbi).eFrameType = eFrameType;
            (*pLbi).iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            (*pLbi).iNalCount = 0;

            let mut iIdx = 0i32;
            while iIdx < kiPartitionCnt {
                let pPriv = (*(*pCtx).pSliceThreading).pThreadPEncCtx.add(iIdx as usize);
                (*pPriv).pFrameBsInfo = pFbi;
                (*pPriv).iSliceIndex = iIdx;
                iIdx += 1;
            }

            let mut iRet = crate::encoder::svc_encode_slice::InitAllSlicesInThread(pCtx);
            if iRet != 0 {
                return ENC_RETURN_UNEXPECTED;
            }
            let pTaskManage = (*pCtx).pTaskManage
                as *mut crate::encoder::wels_task_management::CWelsTaskManageBase;
            (*pTaskManage)
                .ExecuteTasks(crate::encoder::wels_task_management::WELS_ENC_TASK_ENCODING);

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
                crate::encoder::svc_encode_slice::GetCurrentSliceNum((*pCtx).pCurDqLayer);
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
                crate::encoder::svc_encode_slice::GetCurrentSliceNum((*pCtx).pCurDqLayer);
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

                let pCurSlice = (*(*pCtx).pCurDqLayer).sSliceBufferInfo[0]
                    .pSliceBuffer
                    .add(iSliceIdx as usize);
                debug_assert_eq!(iSliceIdx, (*pCurSlice).iSliceIdx);
                (*pCtx).iEncoderError = crate::encoder::svc_encode_slice::SetSliceBoundaryInfo(
                    (*pCtx).pCurDqLayer,
                    pCurSlice,
                    iSliceIdx,
                );

                (*pCtx).iEncoderError = crate::encoder::svc_encode_slice::WelsCodeOneSlice(
                    pCtx,
                    pCurSlice,
                    eNalType as i32,
                );
                if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                    return (*pCtx).iEncoderError;
                }

                crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

                (*pCtx).iEncoderError = crate::encoder::nal_encap::WelsEncodeNal(
                    &(&*(*pCtx).pOut).sNalList[(*(*pCtx).pOut).iNalIndex as usize - 1],
                    &(&*(*pCtx).pOut).sBsBuffer[..],
                    Some(&(*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt),
                    (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize),
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
        if (*(*pCtx).pFuncList)
            .pfRc
            .WelsRcPostFrameSkipping(pCtx, iCurDid as i32, (*pFbi).uiTimeStamp)
        {
            StackBackEncoderStatus(pCtx, eFrameType);
            ClearFrameBsInfo(pCtx, pFbi);

            iFrameSize = 0;
            iLayerNum = 0;

            (*(*pCtx).pFuncList)
                .pfRc
                .WelsUpdateBufferWhenSkip(pCtx, iSpatialNum);

            crate::encoder::rc::WelsRcPostFrameSkippedUpdate(pCtx, iCurDid as i32);
            (*pCtx).iEncoderError = ENC_RETURN_SUCCESS;
            let _ = iLayerNum;
            return ENC_RETURN_SUCCESS;
        }

        // deblocking filter. ENABLE_FRAME_DUMP is not defined, so the temporal-id
        // clause is compiled in.
        if !(*(*pCtx).pCurDqLayer).bDeblockingParallelFlag
            && eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST
            && ((*pParamInternal).iHighestTemporalId == 0
                || iCurTid < (*pParamInternal).iHighestTemporalId as i32)
        {
            crate::encoder::deblocking::PerformDeblockingFilter(pCtx);
        }

        (*(*pCtx).pFuncList)
            .pfRc
            .WelsRcPictureInfoUpdate(pCtx, iLayerSize);
        iFrameSize += iLayerSize;
        crate::encoder::rc::RcTraceFrameBits(pCtx, (*pFbi).uiTimeStamp, iFrameSize);
        (*(*pCtx).pDecPic).iFrameAverageQp =
            (*(*pCtx).pWelsSvcRc.add(iCurDid as usize)).iAverageFrameQp;

        // update scc related
        if let Some(f) = (*(*pCtx).pFuncList).pfUpdateFMESwitch {
            f((*pCtx).pCurDqLayer);
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
        if !fsnr.is_null() && ((*pSvcParam).bPsnrY || (*pSrcPic).bPsnrY) {
            fSnrY = crate::common::wels_common_defs::WelsCalcPsnr(
                (*fsnr).pData[0],
                (*fsnr).iLineSize[0],
                (*pEncPic).pData[0],
                (*pEncPic).iLineSize[0],
                iCurWidth,
                iCurHeight,
            );
        }
        if !fsnr.is_null() && ((*pSvcParam).bPsnrU || (*pSrcPic).bPsnrU) {
            fSnrU = crate::common::wels_common_defs::WelsCalcPsnr(
                (*fsnr).pData[1],
                (*fsnr).iLineSize[1],
                (*pEncPic).pData[1],
                (*pEncPic).iLineSize[1],
                iCurWidth >> 1,
                iCurHeight >> 1,
            );
        }
        if !fsnr.is_null() && ((*pSvcParam).bPsnrV || (*pSrcPic).bPsnrV) {
            fSnrV = crate::common::wels_common_defs::WelsCalcPsnr(
                (*fsnr).pData[2],
                (*fsnr).iLineSize[2],
                (*pEncPic).pData[2],
                (*pEncPic).iLineSize[2],
                iCurWidth >> 1,
                iCurHeight >> 1,
            );
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
        (*pLayerBsInfo).pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
        (*pLayerBsInfo).pNalLengthInByte =
            (*pPrev).pNalLengthInByte.add(iCountNal as usize);

        if (*pSvcParam).iPaddingFlag != 0
            && (*(*pCtx).pWelsSvcRc.add((*pCtx).uiDependencyId as usize)).iPaddingSize > 0
        {
            let mut iPaddingNalSize = 0i32;
            let iPaddingSize =
                (*(*pCtx).pWelsSvcRc.add((*pCtx).uiDependencyId as usize)).iPaddingSize;
            (*pCtx).iEncoderError = WritePadding(pCtx, iPaddingSize, &mut iPaddingNalSize);
            if (*pCtx).iEncoderError != ENC_RETURN_SUCCESS {
                return (*pCtx).iEncoderError;
            }

            if iPaddingNalSize <= 0 {
                return ENC_RETURN_UNEXPECTED;
            }

            let pRc = (*pCtx).pWelsSvcRc.add((*pCtx).uiDependencyId as usize);
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
            (*pLayerBsInfo).pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
            (*pLayerBsInfo).pNalLengthInByte = (*pPrev2).pNalLengthInByte.add(1);
            iLayerNum += 1;

            iFrameSize += iPaddingNalSize;
        }

        (*pCtx).eLastNalPriority[iCurDid as usize] = eNalRefIdc;
        iSpatialIdx += 1;

        if (iCurDid as i32) + 1 < (*pSvcParam).iSpatialLayerNum {
            // iSpatialIdx has already been incremented, so this points at the next layer
            WelsSwapDqLayers(pCtx, (*pSpatialIndexMap.add(iSpatialIdx as usize)).iDid);
        }

        if (*(*pCtx).pVpp).UpdateSpatialPictures(pCtx, pSvcParam, iCurTid as i8, iCurDid as i32) != 0 {
            crate::encoder::wels_encoder_ext::ForceCodingIDR(pCtx, iCurDid as i32);
            // the above sets the next frame to IDR
            (*pFbi).eFrameType = eFrameType;
            (*pLayerBsInfo).eFrameType = eFrameType;
            return ENC_RETURN_CORRECTED;
        }

        let pLtr = (*pCtx).pLtr.add((*pCtx).uiDependencyId as usize);
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
        let iDid = (*pSpatialIndexMap.add(iSpatialIdx as usize)).iDid;
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
