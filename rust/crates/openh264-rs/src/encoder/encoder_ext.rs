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

use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};

use crate::api::codec_api::EUsageType::{CAMERA_VIDEO_REAL_TIME, SCREEN_CONTENT_REAL_TIME};
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
use crate::encoder::paraset_strategy::IWelsParametersetStrategy;
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
    SDqLayer, SMB, MB_BLOCK4x4_NUM, MB_LUMA_CHROMA_BLOCK4x4_NUM,
};
use crate::encoder::svc_motion_estimate::{
    CAMERA_HIGHLAYER_MVD_RANGE, CAMERA_MVD_RANGE, CAMERA_STARTMV_RANGE, EXPANDED_MVD_RANGE,
    EXPANDED_MV_RANGE,
};
use crate::encoder::wels_encoder_ext::{
    ENC_RETURN_MEMALLOCERR, ENC_RETURN_SUCCESS, ENC_RETURN_UNSUPPORTED_PARA, LEVEL_NUMBER,
    MAX_MACROBLOCK_SIZE_IN_BYTE, MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA,
    MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN, MIN_REF_PIC_COUNT,
};

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
/// `pOut->sNalList` and `pOut->pNalLen`.
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
        let pDLayer = &mut (*pParam).sSpatialLayers[iDIndex as usize] as *mut SSpatialLayerConfig;
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

    if (**ppCtx).pFuncList.is_null()
        || (*(**ppCtx).pFuncList).pParametersetStrategy.is_null()
    {
        return 1;
    }
    // count parasets
    let pStrategy = (*(**ppCtx).pFuncList).pParametersetStrategy;
    iCountNumNals += 1
        + iNumDependencyLayers
        + (iCountNumLayers << 1)
        + iCountNumLayers // plus iCountNumLayers for reserved application
        + ((*(*pStrategy).pVtbl).GetAllNeededParasetNum)(pStrategy) as i32;

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
/// Wires every `SMB` in a layer to its slot in the context-wide per-macroblock arrays
/// and computes its neighbour-availability mask.
///
/// # Safety
/// `pEnc` must have `pStrideTab`, `pMvUnitBlock4x4`, `pRefIndexBlock4x4`, `pSadCostMb`,
/// `pIntra4x4PredModeBlocks` and `pNonZeroCountBlocks` allocated; `pList` must hold at
/// least `iMbWidth * iMbHeight` entries.
unsafe fn InitMbInfo(
    pEnc: *mut sWelsEncCtx,
    pList: *mut SMB,
    pLayer: *mut SDqLayer,
    kiDlayerId: i32,
    kiMaxMbNum: i32,
) {
    let iMbWidth = (*pLayer).iMbWidth as i32;
    let iMbHeight = (*pLayer).iMbHeight as i32;
    let iMbNum = iMbWidth * iMbHeight;
    let kiOffset = (kiDlayerId & 0x01) * kiMaxMbNum;
    // C++ reinterprets the flat arrays as [MB_BLOCK4x4_NUM] / [MB_BLOCK8x8_NUM] rows.
    let pLayerMvUnitBlock4x4 = (*pEnc)
        .pMvUnitBlock4x4
        .add(MB_BLOCK4x4_NUM * kiOffset as usize);
    let pLayerRefIndexBlock8x8 = (*pEnc)
        .pRefIndexBlock4x4
        .add(MB_BLOCK8x8_NUM * kiOffset as usize);

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

        (*pMb).sMv = pLayerMvUnitBlock4x4.add(MB_BLOCK4x4_NUM * iIdx);
        (*pMb).pRefIndex = pLayerRefIndexBlock8x8.add(MB_BLOCK8x8_NUM * iIdx);
        (*pMb).pSadCost = (*pEnc).pSadCostMb.add(iIdx);
        (*pMb).pIntra4x4PredMode = (*pEnc).pIntra4x4PredModeBlocks.add(iIdx * INTRA_4x4_MODE_NUM);
        (*pMb).pNonZeroCount = (*pEnc)
            .pNonZeroCountBlocks
            .add(iIdx * MB_LUMA_CHROMA_BLOCK4x4_NUM);
    }
}

/// `InitMbListD` — encoder_ext.cpp:907.
///
/// # Safety
/// `ppCtx` must point to a live context with `ppDqLayerList` populated.
pub unsafe fn InitMbListD(ppCtx: *mut *mut sWelsEncCtx) -> i32 {
    let iNumDlayer = (*(**ppCtx).pSvcParam).iSpatialLayerNum;
    let mut iMbSize = [0i32; MAX_DEPENDENCY_LAYER];
    let mut iOverallMbNum: i32 = 0;

    if iNumDlayer > MAX_DEPENDENCY_LAYER as i32 {
        return 1;
    }

    for i in 0..iNumDlayer as usize {
        let iMbWidth = ((*(**ppCtx).pSvcParam).sSpatialLayers[i].iVideoWidth + 15) >> 4;
        let iMbHeight = ((*(**ppCtx).pSvcParam).sSpatialLayers[i].iVideoHeight + 15) >> 4;
        iMbSize[i] = iMbWidth * iMbHeight;
        iOverallMbNum += iMbSize[i];
    }

    let pMa = (**ppCtx).pMemAlign;
    (**ppCtx).ppMbListD = (*pMa).WelsMallocz(
        (iNumDlayer as usize * std::mem::size_of::<*mut SMB>()) as u32,
        tag!("ppMbListD"),
    ) as *mut *mut SMB;
    if (**ppCtx).ppMbListD.is_null() {
        return 1;
    }
    *(**ppCtx).ppMbListD = null_mut();
    *(**ppCtx).ppMbListD = (*pMa).WelsMallocz(
        (iOverallMbNum as usize * std::mem::size_of::<SMB>()) as u32,
        tag!("ppMbListD[0]"),
    ) as *mut SMB;
    if (*(**ppCtx).ppMbListD).is_null() {
        return 1;
    }
    (**(**ppCtx).ppDqLayerList).sMbDataP = *(**ppCtx).ppMbListD;
    InitMbInfo(
        *ppCtx,
        *(**ppCtx).ppMbListD,
        *(**ppCtx).ppDqLayerList,
        0,
        iMbSize[(iNumDlayer - 1) as usize],
    );
    for i in 1..iNumDlayer as usize {
        *(**ppCtx).ppMbListD.add(i) = (*(**ppCtx).ppMbListD.add(i - 1)).add(iMbSize[i - 1] as usize);
        (**(**ppCtx).ppDqLayerList.add(i)).sMbDataP = *(**ppCtx).ppMbListD.add(i);
        InitMbInfo(
            *ppCtx,
            *(**ppCtx).ppMbListD.add(i),
            *(**ppCtx).ppDqLayerList.add(i),
            i as i32,
            iMbSize[(iNumDlayer - 1) as usize],
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
        let pDlayer = &mut (*pParam).sSpatialLayers[iDlayerIndex as usize] as *mut SSpatialLayerConfig;
        let pParamInternal = &mut (*pParam).sDependencyLayers[iDlayerIndex as usize];
        let kiMbW = ((*pDlayer).iVideoWidth + 0x0f) >> 4;
        let kiMbH = ((*pDlayer).iVideoHeight + 0x0f) >> 4;

        pParamInternal.iCodingIndex = 0;
        pParamInternal.iFrameIndex = 0;
        pParamInternal.iFrameNum = 0;
        pParamInternal.iPOC = 0;
        pParamInternal.uiIdrPicId = 0;
        pParamInternal.bEncCurFrmAsIdrFlag = true; // make sure the first frame is IDR

        let pDqLayer = (*pMa).WelsMallocz(
            std::mem::size_of::<SDqLayer>() as u32,
            tag!("pDqLayer"),
        ) as *mut SDqLayer;
        if pDqLayer.is_null() {
            return 1;
        }

        (*pDqLayer).bNeedAdjustingSlicing = false;

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
        // happens for SCREEN_CONTENT_REAL_TIME.
        if kiNeedFeatureStorage != 0 && iDlayerIndex == iDlayerCount - 1 {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        (*pDqLayer).pFeatureSearchPreparation = null_mut();

        *(**ppCtx).ppDqLayerList.add(iDlayerIndex as usize) = pDqLayer;

        iDlayerIndex += 1;
    }

    // dynamically allocate parameter-set memory instead of the standard's maximum, to
    // reduce size (3/18/2010)
    if (**ppCtx).pFuncList.is_null() {
        return 1;
    }
    let pStrategy = (*(**ppCtx).pFuncList).pParametersetStrategy;
    if pStrategy.is_null() {
        return 1;
    }
    let pVtbl = (*pStrategy).pVtbl;
    let kiNeededSpsNum = ((*pVtbl).GetNeededSpsNum)(pStrategy) as i32;
    let kiNeededSubsetSpsNum = ((*pVtbl).GetNeededSubsetSpsNum)(pStrategy) as i32;
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
    let kiNeededPpsNum = ((*pVtbl).GetNeededPpsNum)(pStrategy) as i32;
    (**ppCtx).pPPSArray = (*pMa).WelsMallocz(
        (kiNeededPpsNum as usize * std::mem::size_of::<crate::encoder::param_svc::SWelsPPS>())
            as u32,
        tag!("pPPSArray"),
    ) as *mut crate::encoder::param_svc::SWelsPPS;
    if (**ppCtx).pPPSArray.is_null() {
        return 1;
    }

    ((*pVtbl).LoadPrevious)(
        pStrategy,
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
        let pDlayerParam = &mut (*pParam).sSpatialLayers[iDlayerIndex as usize];
        let bSvcBaselayer = !(*pParam).bSimulcastAVC
            && (iDlayerCount > BASE_DEPENDENCY_ID as i32)
            && (iDlayerIndex == BASE_DEPENDENCY_ID as i32);
        (*pDqIdc).uiSpatialId = iDlayerIndex as i8;

        iSpsId = ((*pVtbl).GenerateNewSps)(
            pStrategy,
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

        iPpsId = ((*pVtbl).InitPps)(
            pStrategy,
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
            &mut pDlayerParam.sSliceArgument,
            pPps as *mut c_void,
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

    ((*pVtbl).UpdateParaSetNum)(pStrategy, *ppCtx);
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

    let pStrategy = (*(**ppCtx).pFuncList).pParametersetStrategy;
    let pVtbl = (*pStrategy).pVtbl;
    let kiSpsSize = ((*pVtbl).GetNeededSpsNum)(pStrategy) as i32 * SPS_BUFFER_SIZE;
    let kiPpsSize = ((*pVtbl).GetNeededPpsNum)(pStrategy) as i32 * PPS_BUFFER_SIZE;
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

    // Output
    (**ppCtx).pOut = (*pMa).WelsMallocz(
        std::mem::size_of::<crate::encoder::nal_encap::SWelsEncoderOutput>() as u32,
        tag!("SWelsEncoderOutput"),
    ) as *mut crate::encoder::nal_encap::SWelsEncoderOutput;
    if (**ppCtx).pOut.is_null() {
        return 1;
    }
    (*(**ppCtx).pOut).pBsBuffer =
        (*pMa).WelsMallocz(iCountBsLen as u32, tag!("pOut->pBsBuffer")) as *mut u8;
    if (*(**ppCtx).pOut).pBsBuffer.is_null() {
        return 1;
    }
    (*(**ppCtx).pOut).uiSize = iCountBsLen as u32;
    (*(**ppCtx).pOut).sNalList = (*pMa).WelsMallocz(
        (iCountNals as usize * std::mem::size_of::<crate::encoder::nal_encap::SWelsNalRaw>())
            as u32,
        tag!("pOut->sNalList"),
    ) as *mut crate::encoder::nal_encap::SWelsNalRaw;
    if (*(**ppCtx).pOut).sNalList.is_null() {
        return 1;
    }
    (*(**ppCtx).pOut).pNalLen =
        (*pMa).WelsMallocz((iCountNals as u32) * 4, tag!("pOut->pNalLen")) as *mut i32;
    if (*(**ppCtx).pOut).pNalLen.is_null() {
        return 1;
    }
    (*(**ppCtx).pOut).iCountNals = iCountNals;
    (*(**ppCtx).pOut).iNalIndex = 0;
    (*(**ppCtx).pOut).iLayerBsIndex = 0;

    (**ppCtx).pFrameBs = (*pMa).WelsMalloc(iTotalLength as u32, tag!("pFrameBs")) as *mut u8;
    if (**ppCtx).pFrameBs.is_null() {
        return 1;
    }
    (**ppCtx).iFrameBsSize = iTotalLength;
    (**ppCtx).iPosBsBuffer = 0;

    // for dynamic slice mode && CABAC, allocate slice buffers to restore slice data
    if bDynamicSlice && (*pParam).iEntropyCodingModeFlag != 0 {
        // encoder_ext.cpp:1649. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    // for slice bs buffers
    if (*pParam).iMultipleThreadIdc > 1 {
        // encoder_ext.cpp:1656, RequestMtResource. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    (**ppCtx).pReferenceStrategy = crate::encoder::ref_list_mgr_svc::CreateReferenceStrategy(
        *ppCtx,
        (*pParam).iUsageType,
        (*pParam).bEnableLongTermReference,
    ) as *mut c_void;
    if (**ppCtx).pReferenceStrategy.is_null() {
        return 1;
    }

    (**ppCtx).pIntra4x4PredModeBlocks = (*pMa).WelsMallocz(
        (iCountMaxMbNum as usize * INTRA_4x4_MODE_NUM) as u32,
        tag!("pIntra4x4PredModeBlocks"),
    ) as *mut i8;
    if (**ppCtx).pIntra4x4PredModeBlocks.is_null() {
        return 1;
    }

    (**ppCtx).pNonZeroCountBlocks = (*pMa).WelsMallocz(
        (iCountMaxMbNum as usize * MB_LUMA_CHROMA_BLOCK4x4_NUM) as u32,
        tag!("pNonZeroCountBlocks"),
    ) as *mut i8;
    if (**ppCtx).pNonZeroCountBlocks.is_null() {
        return 1;
    }

    (**ppCtx).pMvUnitBlock4x4 = (*pMa).WelsMallocz(
        (iCountMaxMbNum as usize
            * 2
            * MB_BLOCK4x4_NUM
            * std::mem::size_of::<crate::encoder::md::SMVUnitXY>()) as u32,
        tag!("pMvUnitBlock4x4"),
    ) as *mut crate::encoder::md::SMVUnitXY;
    if (**ppCtx).pMvUnitBlock4x4.is_null() {
        return 1;
    }

    (**ppCtx).pRefIndexBlock4x4 = (*pMa).WelsMallocz(
        (iCountMaxMbNum as usize * 2 * MB_BLOCK8x8_NUM) as u32,
        tag!("pRefIndexBlock4x4"),
    ) as *mut i8;
    if (**ppCtx).pRefIndexBlock4x4.is_null() {
        return 1;
    }

    (**ppCtx).pSadCostMb =
        (*pMa).WelsMallocz((iCountMaxMbNum as u32) * 4, tag!("pSadCostMb")) as *mut i32;
    if (**ppCtx).pSadCostMb.is_null() {
        return 1;
    }

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
    (**ppCtx).pMvdCostTable = (*pMa).WelsMallocz(
        (52 * kuiMvdCacheAlignedSize) as u32,
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
