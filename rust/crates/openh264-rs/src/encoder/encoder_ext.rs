//! Port of the memory-allocation and layer-initialisation half of
//! `codec/encoder/core/src/encoder_ext.cpp`.
//!
//! `wels_encoder_ext.rs` already holds the parameter validation and the
//! parameter-set NAL writers from the same file; this module holds the rest of the
//! core encoder: `AcquireLayersNals`, `AllocStrideTables`, `InitMbListD`,
//! `InitDqLayers`, `RequestMemorySvc`, `GetMultipleThreadIdc`, `WelsInitEncoderExt`
//! and `WelsEncoderEncodeExt`.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]
use std::sync::atomic::{AtomicU16, Ordering};
use crate::encoder::picture::{RecPicId, RecPicPool, SrcPicId, SrcPicPool};
use crate::encoder::md::CostFamily;
use std::ffi::c_char;

use crate::api::codec_api::EUsageType::{CAMERA_VIDEO_REAL_TIME, SCREEN_CONTENT_REAL_TIME};
use crate::api::codec_api::SliceModeEnum;
use crate::api::codec_api::SliceModeEnum::{SM_SINGLE_SLICE, SM_SIZELIMITED_SLICE};
use crate::api::codec_api::RC_MODES::RC_OFF_MODE;
use crate::api::codec_api::ELevelIdc;
use crate::decoder::nalu::g_ksLevelLimits;
use crate::encoder::encoder_context::{
    ctx_dq_idc_map, ctx_ltr_at,
    ctx_paraset_arrays,
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
use crate::encoder::svc_encode_slice::set_current_layer;
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
use crate::encoder::encoder_context::dq_layer_ref;
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
use crate::encoder::svc_encode_slice::{current_layer_expect, current_layer_expect_mut};

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

/// `WelsGetEncBlockStrideOffset` — `decode_mb_aux.cpp:235`.
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
/// Returns 0 on success and 1 if the frame would need more layers, NALs or slices
/// than the limits allow, or if `pFuncList.pParametersetStrategy` is not installed.
pub fn AcquireLayersNals(
    ctx: &mut sWelsEncCtx,
    pCountLayers: &mut i32,
    pCountNals: &mut i32,
) -> i32 {
    let mut iCountNumLayers: i32 = 0;
    let mut iCountNumNals: i32 = 0;
    let mut iDIndex: i32 = 0;

    let iNumDependencyLayers = ctx.param().iSpatialLayerNum;

    loop {
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

    *pCountLayers = iCountNumLayers;
    *pCountNals = iCountNumNals;
    0
}

/// `AllocStrideTables` — encoder_ext.cpp:1224.
///
/// # Panics
/// Panics if the context's coding parameters have not been built yet: every
/// dimension the tables are sized from is read through `ctx.param()`.
pub fn AllocStrideTables(ctx: &mut sWelsEncCtx, kiNumSpatialLayers: i32) -> i32 {
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
    // The dec and enc regions are counted in 24-`i32` blocks (`kiUnit1Size` is
    // exactly one) and the coordinate tables in `i16`s (`iSizeAllMbAlignCache`
    // is `iCountMbNum * sizeof(int16_t)`, so `iUnit2Size` bytes is
    // `iUnit2Size / 2` entries per table).
    let kiBlockCount = (iCountLayersNeedCs[0] + iCountLayersNeedCs[1] + kiNumSpatialLayers)
        .max(0) as usize;
    let kiCoordLen = (iUnit2Size.max(0) as usize / 2) * 2;

    ctx.pStrideTab = Some(Box::new(SStrideTables::new(kiBlockCount, kiCoordLen)));
    let pPtr: &mut SStrideTables = ctx.pStrideTab.as_mut().unwrap();

    // The C++ carves the block with four running `uint8_t*` cursors. They are
    // *indices* into the two typed stores here, advanced by the same regions in
    // the same order — the arithmetic below is the same walk, in the units the
    // storage is made of.
    let mut pBaseDec: u32 = 0; // iCountLayersNeedCs, in blocks
    let mut pBaseEnc: u32 = (iCountLayersNeedCs[0] + iCountLayersNeedCs[1]).max(0) as u32;
    let mut pBaseMbX: u32 = 0; // in i16 entries
    let mut pBaseMbY: u32 = (iUnit2Size.max(0) as u32) / 2;

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
            WelsGetEncBlockStrideOffset(
                pPtr.i32_block24_mut(pBaseDec),
                kiLumaWidth,
                kiChromaWidth,
            );
            pBaseDec += 1;

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

            // not in the spatial map: assign the matching one to it.
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

        pBaseEnc += 1;
        // `iSizeAllMbAlignCache` is bytes; the store counts `i16`s.
        pBaseMbX += (kiAllocMbSize as u32) / 2;
        pBaseMbY += (kiAllocMbSize as u32) / 2;

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

    let mut sTmpRow = vec![0i16; (iRowSize as usize).div_ceil(std::mem::size_of::<i16>())];
    // initialize the scratch row: 0, 1, 2, ...
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
fn InitMbInfo(
    kpMbIndexX: &[i16],
    kpMbIndexY: &[i16],
    pLayer: &mut SDqLayer,
) {
    let iMbWidth = pLayer.iMbWidth as i32;
    let iMbHeight = pLayer.iMbHeight as i32;
    let iMbNum = iMbWidth * iMbHeight;
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
        // discards it — the result is never stored.
    }
}

/// `InitMbListD` — encoder_ext.cpp:907.
pub fn InitMbListD(ctx: &mut sWelsEncCtx) -> i32 {
    let iNumDlayer = ctx.param().iSpatialLayerNum;

    if iNumDlayer > MAX_DEPENDENCY_LAYER as i32 {
        return 1;
    }

    for i in 0..iNumDlayer as usize {
        let iMbWidth = (ctx.param().sSpatialLayers[i].iVideoWidth + 15) >> 4;
        let iMbHeight = (ctx.param().sSpatialLayers[i].iVideoHeight + 15) >> 4;
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
/// It allocates the reference lists and DQ layers,
/// then `pSpsArray`/`pSubsetArray`/`pPPSArray`, and drives the parameter-set strategy
/// to fill them and set `iSpsNum`/`iSubsetSpsNum`/`iPpsNum`.
///
/// Expects a live context with `pSvcParam`, `pStrideTab`, `ppRefPicListExt`,
/// `ppDqLayerList` and `pFuncList->pParametersetStrategy` set — each `expect`
/// below names the initializer that owes it.
pub fn InitDqLayers(
    ctx: &mut sWelsEncCtx,
    pExistingParasetList: Option<&SExistingParasetList>,
) -> i32 {
    let mut iSpsId: i32 = 0;
    let mut iPpsId: u32 = 0;
    let mut iResult: i32;

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

        {
            let tab = ctx.pStrideTab.as_mut().expect("pStrideTab allocated");
            let kuiOff = tab.pStrideEncBlockOffset[iDlayerIndex as usize]
                .expect("AllocStrideTables filled the enc-side offset for every layer");
            WelsGetEncBlockStrideOffset(tab.i32_block24_mut(kuiOff), iPicWidth, iPicChromaWidth);
        }

        // Reference list.
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
        (&mut ctx.ppRefPicListExt)[iDlayerIndex as usize] = Some(pRefListBox);
        iDlayerIndex += 1;
    }

    iDlayerIndex = 0;
    while iDlayerIndex < iDlayerCount {
        let kiMbW = (ctx.param().sSpatialLayers[iDlayerIndex as usize].iVideoWidth + 0x0f) >> 4;
        let kiMbH = (ctx.param().sSpatialLayers[iDlayerIndex as usize].iVideoHeight + 0x0f) >> 4;

        {
            let pParamInternal =
                &mut ctx.param_mut().sDependencyLayers[iDlayerIndex as usize];
            pParamInternal.iCodingIndex = 0;
            pParamInternal.iFrameIndex = 0;
            pParamInternal.iFrameNum = 0;
            pParamInternal.iPOC = 0;
            pParamInternal.uiIdrPicId = 0;
            pParamInternal.bEncCurFrmAsIdrFlag = true; // make sure the first frame is IDR
        }

        let mut pDqLayerBox = Box::new(SDqLayer::new(LayerIdx(iDlayerIndex as u8)));

        pDqLayerBox.iMbWidth = kiMbW as i16;
        pDqLayerBox.iMbHeight = kiMbH as i16;

        let mut iMaxSliceNum: i32 = 1;
        let kiSliceNum = GetInitialSliceNum(
            &ctx.param().sSpatialLayers[iDlayerIndex as usize].sSliceArgument,
        );
        if iMaxSliceNum < kiSliceNum {
            iMaxSliceNum = kiSliceNum;
        }
        pDqLayerBox.iMaxSliceNum = iMaxSliceNum;

        iResult = InitSliceInLayer(ctx, &mut pDqLayerBox, iDlayerIndex);
        if iResult != 0 {
            return iResult;
        }

        // deblocking parameters initialization; target-layer deblocking
        pDqLayerBox.iLoopFilterDisableIdc = ctx.param().iLoopFilterDisableIdc as u8;
        pDqLayerBox.iLoopFilterAlphaC0Offset = (ctx.param().iLoopFilterAlphaC0Offset << 1) as i8;
        pDqLayerBox.iLoopFilterBetaOffset = (ctx.param().iLoopFilterBetaOffset << 1) as i8;
        // parallel deblocking
        pDqLayerBox.bDeblockingParallelFlag = ctx.param().bDeblockingParallelFlag;

        // deblocking parameter adjustment
        if SM_SINGLE_SLICE
            == ctx.param().sSpatialLayers[iDlayerIndex as usize].sSliceArgument.uiSliceMode
        {
            // iLoopFilterDisableIdc will be 0 or 1 under single slice
            if 2 == ctx.param().iLoopFilterDisableIdc {
                pDqLayerBox.iLoopFilterDisableIdc = 0;
            }
            pDqLayerBox.bDeblockingParallelFlag = false;
        } else {
            // multi-slice
            if 0 == pDqLayerBox.iLoopFilterDisableIdc {
                pDqLayerBox.bDeblockingParallelFlag = false;
            }
        }

        // encoder_ext.cpp:1125-1135 — the last layer alone carries the preparation.
        if kiNeedFeatureStorage != 0 && iDlayerIndex == iDlayerCount - 1 {
            let kiVideoWidth = ctx.param().sSpatialLayers[iDlayerIndex as usize].iVideoWidth;
            let kiVideoHeight = ctx.param().sSpatialLayers[iDlayerIndex as usize].iVideoHeight;
            pDqLayerBox.pFeatureSearchPreparation = Some(Box::new(
                crate::encoder::svc_motion_estimate::SFeatureSearchPreparation::new(
                    kiVideoWidth,
                    kiVideoHeight,
                    kiNeedFeatureStorage,
                ),
            ));
        }

        (&mut ctx.ppDqLayerList)[iDlayerIndex as usize] = Some(pDqLayerBox);

        iDlayerIndex += 1;
    }

    // dynamically allocate parameter-set memory instead of the standard's maximum, to
    // reduce size (3/18/2010)
    if ctx.func_list().pParametersetStrategy.is_none() {
        return 1;
    }
    let kiNeededSpsNum = ParasetStrategy(ctx).GetNeededSpsNum() as i32;
    let kiNeededSubsetSpsNum = ParasetStrategy(ctx).GetNeededSubsetSpsNum() as i32;
    ctx.pSpsArray = vec![crate::encoder::param_svc::SWelsSPS::ZERO; kiNeededSpsNum as usize];
    ctx.pSubsetArray = vec![
        crate::encoder::param_svc::SSubsetSps::ZERO;
        kiNeededSubsetSpsNum.max(0) as usize
    ];

    // PPS
    let kiNeededPpsNum = ParasetStrategy(ctx).GetNeededPpsNum() as i32;
    ctx.pPPSArray = vec![crate::encoder::param_svc::SWelsPPS::ZERO; kiNeededPpsNum as usize];

    let (pParasetStrategy, pSpsArray, pSubsetArray, pPpsArray) =
        crate::encoder::paraset_strategy::ctx_strategy_and_paraset_arrays(ctx);
    pParasetStrategy.LoadPrevious(
        pExistingParasetList,
        pSpsArray,
        pSubsetArray,
        pPpsArray,
    );

    ctx.pDqIdcMap = vec![SDqIdc::default(); iDlayerCount as usize];

    iDlayerIndex = 0;
    while iDlayerIndex < iDlayerCount {
        let bUseSubsetSps = !ctx.param().bSimulcastAVC && (iDlayerIndex > BASE_DEPENDENCY_ID as i32);
        let bSvcBaselayer = !ctx.param().bSimulcastAVC
            && (iDlayerCount > BASE_DEPENDENCY_ID as i32)
            && (iDlayerIndex == BASE_DEPENDENCY_ID as i32);

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
        let kbEntropyCodingModeFlag = ctx.param().iEntropyCodingModeFlag != 0;
        let (pParasetStrategy, pSpsArray, pSubsetArray, pPpsArray) =
            crate::encoder::paraset_strategy::ctx_strategy_and_paraset_arrays(ctx);
        let (kpSps, kpSubsetSps): (&SWelsSPS, Option<&SSubsetSps>) = if !bUseSubsetSps {
            (&pSpsArray[iSpsId as usize], None)
        } else {
            let kpSubset = &pSubsetArray[iSpsId as usize];
            (&kpSubset.pSps, Some(kpSubset))
        };
        iPpsId = pParasetStrategy.InitPps(
            pPpsArray,
            iSpsId as u32,
            Some(kpSps),
            kpSubsetSps,
            iPpsId,
            true,
            bUseSubsetSps,
            kbEntropyCodingModeFlag,
        );
        // The C++ takes `pPps = &pPPSArray[iPpsId]` here and hands it to
        // `InitSlicePEncCtx`'s final parameter, which the port's callee never
        // had (nothing reads it).

        let (kiSpsMbWidth, kiSpsMbHeight) = {
            let kpSps = if !bUseSubsetSps {
                &ctx.sps_array()[iSpsId as usize]
            } else {
                &ctx.subset_array()[iSpsId as usize].pSps
            };
            (kpSps.iMbWidth as i32, kpSps.iMbHeight as i32)
        };

        // FMO is not used in SVC coding so far; come back if FMO is needed
        let sWelsEncCtx { ppDqLayerList, pSvcParam, .. } = &mut *ctx;
        iResult = InitSlicePEncCtx(
            ppDqLayerList[iDlayerIndex as usize]
                .as_deref_mut()
                .expect("the layer was stored by the loop above"),
            false,
            kiSpsMbWidth,
            kiSpsMbHeight,
            &pSvcParam
                .as_deref()
                .expect("the coding parameters are built by WelsInitEncoderExt")
                .sSpatialLayers[iDlayerIndex as usize]
                .sSliceArgument,
        );
        if iResult != 0 {
            return iResult;
        }
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

    {
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
/// # Panics
/// Panics if the context's coding parameters have not been built, or if
/// `pFuncList.pParametersetStrategy` has not been installed by
/// `InitFunctionPointers` — the paraset buffer sizes are read through it.
pub fn RequestMemorySvc(
    ctx: &mut sWelsEncCtx,
    pExistingParasetList: Option<&SExistingParasetList>,
) -> i32 {
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
    let mut iSliceBufferSize: i32;
    let mut iMaxSliceBufferSize: i32 = 0;
    let mut iIndex: i32 = 0;
    while iIndex < ctx.param().iSpatialLayerNum {
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
    ctx.pOut = Some(crate::encoder::nal_encap::SWelsEncoderOutput::new_boxed(
        iCountBsLen as usize,
        iCountNals as usize,
    ));

    // The C++ takes this block with `WelsMalloc` — *uninitialized* — and it is the
    // only member of this function's set that does. `vec![0; n]` writes zeros the
    // C++ does not.
    ctx.pFrameBs = vec![0u8; iTotalLength.max(0) as usize];
    ctx.iFrameBsSize = iTotalLength;
    ctx.iPosBsBuffer = 0;

    // for dynamic slice mode && CABAC, allocate slice buffers to restore slice data.
    // These are `sDss.pRestoreBuffer` in the two dynamic MB loops: CABAC
    // renormalisation can rewrite bytes already emitted, so stepping back over a
    // slice boundary has to restore the bytes as well as the coder state.
    if bDynamicSlice && ctx.param().iEntropyCodingModeFlag != 0 {
        for iIdx in 0..MAX_THREADS_NUM {
            // `WelsMalloc` here was *uninitialized* (not `WelsMallocz`), so
            // `vec![0; n]` writes zeros the C++ does not.
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

    ctx.eRefStrategy = crate::encoder::ref_list_mgr_svc::RefStrategyKind::Select(
        ctx.param().iUsageType,
        ctx.param().bEnableLongTermReference,
    );

    ctx.iGlobalQp = 26; // global qp in default

    ctx.pLtr = vec![
        crate::encoder::ref_list_mgr_svc::SLTRState::default();
        kiNumDependencyLayers as usize
    ];
    for i in 0..kiNumDependencyLayers as usize {
        crate::encoder::ref_list_mgr_svc::ResetLtrState(ctx_ltr_at(&mut *ctx, i));
    }

    // stride tables
    if AllocStrideTables(ctx, kiNumDependencyLayers) != 0 {
        return 1;
    }

    // Rate control module memory allocation; only malloc once for RC data (12/14/2009)
    // Built one at a time rather than with `vec![x; n]`, which would need
    // `SWelsSvcRc: Clone`, and the derive is not there.
    ctx.pWelsSvcRc = (0..kiNumDependencyLayers as usize)
        .map(|_| crate::encoder::rc::SWelsSvcRc::default())
        .collect();

    // pVaa memory allocation — encoder_ext.cpp:1707-1718.
    let kbBgd = ctx.param().bEnableBackgroundDetection;
    let kiMaxNumRef = ctx.param().iMaxNumRefFrame;
    if ctx.param().iUsageType == SCREEN_CONTENT_REAL_TIME {
        // `RequestMemoryVaaScreen` (encoder_ext.cpp:1478-1491): one `WelsMallocz` of
        // `iNumRef * (iCountMaxMbNum << 2)` bytes, walked by sixteen row pointers at
        // one stride — which is what `SBlockStaticIdcStore::alloc` is. The C++
        // passes `iMaxNumRefFrame` as the row count and leaves the slots past it
        // null; `select()` answers `None` past `rows`.
        let rows = (kiMaxNumRef.max(0) as usize)
            .min(crate::encoder::wels_preprocess::SBlockStaticIdcStore::MAX_ROWS);
        let stride = (iCountMaxMbNum.max(0) as usize) << 2;
        let mut ext = crate::encoder::wels_preprocess::SVAAFrameInfoExt {
            sVaaFrameInfo: *crate::encoder::wels_preprocess::SVAAFrameInfo::new(
                iCountMaxMbNum,
                kbBgd,
            ),
            ..Default::default()
        };
        ext.pVaaBlockStaticIdc.alloc(rows, stride);
        ctx.pVaa = Some(Box::new(crate::encoder::wels_preprocess::VaaBlock::Screen(ext)));
    } else {
        ctx.pVaa = Some(Box::new(crate::encoder::wels_preprocess::VaaBlock::Base(
            *crate::encoder::wels_preprocess::SVAAFrameInfo::new(iCountMaxMbNum, kbBgd),
        )));
    }

    if ctx.param().bEnableAdaptiveQuant {
        // encoder_ext.cpp:1720, sAdaptiveQuantParam buffers. Not ported.
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // End of pVaa memory allocation

    ctx.ppRefPicListExt = (0..kiNumDependencyLayers).map(|_| None).collect();

    ctx.ppDqLayerList = (0..kiNumDependencyLayers).map(|_| None).collect();

    iResult = InitDqLayers(ctx, pExistingParasetList);
    if iResult != 0 {
        return iResult;
    }

    if InitMbListD(ctx) != 0 {
        return 1;
    }

    let mut iMvdRange: i32 = 0;
    let mut iMvRangeOut = ctx.iMvRange;
    GetMvMvdRange(ctx.param(), &mut iMvRangeOut, &mut iMvdRange);
    ctx.iMvRange = iMvRangeOut;
    let kuiMvdInterTableSize = iMvdRange << 2; // intepel*4 = qpel
    let kuiMvdInterTableStride = 1 + (kuiMvdInterTableSize << 1); // qpel_mv_range*2 = (+/-)
    let kuiMvdCacheAlignedSize = kuiMvdInterTableStride * 2; // sizeof(uint16_t)

    ctx.iMvdCostTableSize = kuiMvdInterTableSize;
    ctx.iMvdCostTableStride = kuiMvdInterTableStride;
    // `MvdCostInit` walks two cursors one stride per row for 52 rows. `pNegMvd`
    // starts at the table's base and ends exactly one past it, which is legal.
    // `pPosMvd` starts `(kiSz + 1)` elements in and advances by the same stride,
    // so after the 52nd row it lands `(kiSz + 1)` elements *beyond* the table —
    // 1042 bytes on this configuration. The pointer is formed and never
    // dereferenced, which is why nothing has ever observed it: it is UB in Rust
    // and in C alike, and the C++ upstream forms the same pointer.
    //
    // The extra bytes are never read, never written and never addressed except
    // by the one bump this exists to keep in bounds, so no encoded byte can move.
    let kuiMvdCostTableOvershoot = 2 * ((kuiMvdInterTableStride >> 1) + 1);
    // The size above is in *bytes* (the C++ `WelsMalloc` takes bytes and casts to
    // `uint16_t*`), so the `Vec`'s length is that over two.
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

    // Nothing re-aims these, in this port or in the C++ — `encoder_ext.cpp` assigns
    // them here and nowhere else — so the active set is position 0 for the
    // encoder's whole life.
    ctx.iSps = Some(SpsId(0));
    ctx.iPps = Some(PpsId(0));

    0
}

/// `InitSliceSettings` — encoder_ext.cpp:2018.
///
/// Resolves the per-layer slice arguments, then derives `iMultipleThreadIdc` and the
/// maximum slice count from them.
pub fn InitSliceSettings(
    pLogCtx: SLogContext,
    pCodingParam: &mut SWelsSvcCodingParam,
    kiCpuCores: i32,
    pMaxSliceCount: &mut i16,
) -> i32 {
    let mut iSpatialIdx: i32 = 0;
    let iSpatialNum = pCodingParam.iSpatialLayerNum;
    let mut iMaxSliceCount: u16 = 0;

    loop {
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
pub fn GetMultipleThreadIdc(
    pLogCtx: SLogContext,
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
/// `MEMORY_MONITOR` and the `WelsLog` calls have no counterpart here.
///
/// The context handed back in `*ppCtx` is owned by the caller and is released with
/// [`WelsUninitEncoderExt`], which unwinds the preprocessor's spatial pictures and
/// the DQ layers before the box is dropped.
pub fn WelsInitEncoderExt(
    ppCtx: &mut Option<Box<sWelsEncCtx>>,
    pCodingParam: &mut SWelsSvcCodingParam,
    pLogCtx: SLogContext,
    pExistingParasetList: Option<&SExistingParasetList>,
) -> i32 {
    let mut iSliceNum: i16 = 1; // number of slices used
    let mut iCacheLineSize: i32 = 16; // on-chip cache line size in bytes
    let mut uiCpuFeatureFlags: u32 = 0;
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

    *ppCtx = None;

    // C++ mallocs and memsets sWelsEncCtx; Box::new of a Default context is the
    // equivalent, and Default is the all-zero/null state for every member.
    let mut ctxBox = Box::new(sWelsEncCtx::default());

    ctxBox.sLogCtx = pLogCtx;

    ctxBox.pSvcParam = Some(crate::encoder::param_svc::NewCodingParam());
    *ctxBox.param_mut() = *pCodingParam;

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
    iRet = RequestMemorySvc(&mut ctxBox, pExistingParasetList);
    if iRet != 0 {
        WelsUninitEncoderExt(Some(ctxBox));
        return iRet;
    }

    if pCodingParam.iEntropyCodingModeFlag != 0 {
        crate::encoder::set_mb_syn_cabac::WelsCabacInit(&mut *ctxBox);
    }
    let iRCMode = ctxBox.param().iRCMode;
    crate::encoder::rc::WelsRcInitModule(&mut ctxBox, iRCMode);

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
pub fn FreeSliceInLayer(pDq: &mut SDqLayer) {
    for iIdx in 0..MAX_THREADS_NUM {
        crate::encoder::svc_encode_slice::FreeSliceBuffer(pDq, iIdx);
    }
}

/// `FreeDqLayer` — encoder_ext.cpp:951.
///
/// Releases every slice bank of the layer, uninitialises its slice segment context
/// and zeroes `iMaxSliceNum`, so the layer has to be rebuilt by `InitDqLayers`
/// before it can be used again.
pub fn FreeDqLayer(p: &mut SDqLayer) {
    FreeSliceInLayer(&mut *p);

    crate::encoder::svc_enc_slice_segment::UninitSlicePEncCtx(&mut *p);
    (*p).iMaxSliceNum = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::codec_api::EProfileIdc;
    use crate::encoder::encoder_context::InitFunctionPointers;
    use crate::encoder::param_svc::NewCodingParam;
    use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

    /// Builds the context up to and including `RequestMemorySvc`, which is everything
    /// `WelsInitEncoderExt` does before the preprocessor.
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

        let mut ctxBox = Box::new(sWelsEncCtx::default());
        ctxBox.pSvcParam = Some(NewCodingParam());
        *ctxBox.param_mut() = param;
        assert_eq!(
            InitFunctionPointers(&mut ctxBox, uiCpuFeatureFlags),
            ENC_RETURN_SUCCESS
        );
        ctxBox.iActiveThreadsNum = param.iMultipleThreadIdc as i16;
        ctxBox.iMaxSliceCount = iSliceNum as i32;

        assert_eq!(RequestMemorySvc(&mut ctxBox, None), 0, "RequestMemorySvc");
        Box::into_raw(ctxBox)
    }

    /// The parameter-set arrays are allocated and populated.
    #[test]
    #[allow(unsafe_code)]
    fn request_memory_svc_builds_the_parameter_sets() {
        unsafe {
            let pCtx = build_gate_context();

            assert!(!(*pCtx).pSpsArray.is_empty(), "pSpsArray still unallocated");
            assert!(!(*pCtx).pPPSArray.is_empty(), "pPPSArray still unallocated");
            // The configuration needs no subset SPS, and the C++ allocated nothing
            // at all for it.
            assert!((*pCtx).pSubsetArray.is_empty(), "pSubsetArray was not needed");
            assert!((*pCtx).subset_array().is_empty());
            assert_eq!((*pCtx).iSpsNum, 1);
            assert_eq!((*pCtx).iPpsNum, 1);
            assert_eq!((*pCtx).iSubsetSpsNum, 0);
            assert_eq!(ctx_sps(&mut *pCtx), (*pCtx).sps_array().as_ptr().cast_mut());
            assert_eq!(ctx_pps(&mut *pCtx), (*pCtx).pps_array().as_ptr().cast_mut());

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

    /// The DQ layers, reference lists and macroblock list
    /// exist, which is what `pCurDqLayer` is selected from.
    #[test]
    #[allow(unsafe_code)]
    fn request_memory_svc_builds_the_dq_layers() {
        unsafe {
            let pCtx = build_gate_context();

            let pDq = dq_layer_ref(&*pCtx, 0).expect("RequestMemorySvc built layer 0");
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
/// `None` is accepted and does nothing.
///
/// # Panics
/// Panics if the context's coding parameters were never built: the teardown log
/// reads `iMultipleThreadIdc` from them before anything is released.
pub fn WelsUninitEncoderExt(pEncContext: Option<Box<sWelsEncCtx>>) {
    let Some(mut ctxBox) = pEncContext else {
        return;
    };

    // `encoder_ext.cpp:2250-2252` — the teardown announces itself before any
    // free runs, through the context's own log sink.
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

    if let Some(mut vpp) = ctxBox.pVpp.take() {
        vpp.FreeSpatialPictures(&mut ctxBox);
    }

    {
        drop(ctxBox.pOut.take());

        // DQ layers list.
        for ilayer in 0..ctxBox.ppDqLayerList.len() {
            if let Some(pLayer) = crate::encoder::encoder_context::dq_layer_mut(&mut *ctxBox, ilayer) {
                FreeDqLayer(pLayer);
            }
        }
    }

    drop(ctxBox);
}

// ============================================================================
// The encoding half of encoder_ext.cpp: WelsEncoderEncodeExt and its helpers.
//
// Line references in the doc comments are to that file.
// ============================================================================

/// `encoder_ext.cpp:2393`.
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
    // The outgoing layer's *position*, not its address: `iCurDqLayer` **is** the
    // index. The `expect` cannot fire on a live path — the frame loop makes a
    // layer current before any swap.
    let kRefIdx = pCtx.iCurDqLayer.expect("WelsSwapDqLayers with no current layer");
    set_current_layer(pCtx, Some(LayerIdx(kiNextDqIdx as u8)));
    if let Some(pCurLayer) = current_layer_mut(pCtx) {
        pCurLayer.pRefLayer = Some(kRefIdx);
    }
}

/// `encoder_ext.cpp:2808`. Prefetch the reference picture after `WelsBuildRefList`.
pub fn PrefetchReferencePicture(pCtx: &mut sWelsEncCtx, keFrameType: EVideoFrameType) {
    let kiSliceCount = current_layer_expect(pCtx).iMaxSliceNum;
    // C++ declares `uint8_t uiRefIdx = -1;`, which wraps to 255.
    let mut uiRefIdx: u8 = 0xff;

    debug_assert!(kiSliceCount > 0);
    if keFrameType != EVideoFrameType::videoFrameTypeIDR {
        debug_assert!(pCtx.iNumRef0 > 0);
        // always get item 0 due to reordering done
        pCtx.pRefPic = pCtx.pRefList0[0];
        current_layer_expect_mut(pCtx).pRefPic = pCtx.pRefPic;
        uiRefIdx = 0; // reordered reference index
    } else {
        // safe for IDR coding
        pCtx.pRefPic = None;
        current_layer_expect_mut(pCtx).pRefPic = None;
    }

    let mut iIdx = 0;
    while iIdx < kiSliceCount {
        if let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(
            current_layer_expect_mut(pCtx),
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
    {
        // The frame's first layer starts at entry 0 of `pOut.sNalLen`.
        let pOut = pCtx.out_mut();
        pOut.iNalLenBase = 0;
        (*pFbi).sLayerInfo[0].pNalLengthInByte = pOut.nal_len_ptr();
    }

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
    let kiDid = pEncCtx.uiDependencyId as usize;
    let kiLog2MaxPocLsb = crate::encoder::svc_encode_slice::ctx_sps_ref(pEncCtx).map_or(0, |s| s.iLog2MaxPocLsb);

    // for bitstream writing
    pEncCtx.iPosBsBuffer = 0; // reset bs buffer position
    pEncCtx.out_mut().iNalIndex = 0; // reset NAL index
    pEncCtx.out_mut().iLayerBsIndex = 0; // reset index of Layer Bs

    // Was `InitBits(&pOut->sBsWrite, pOut->pBsBuffer, pOut->uiSize)`.
    pEncCtx.out_mut().sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();

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
    // The layer is stamped with this frame's picture handles here, once a frame,
    // and the per-macroblock mode-decision family resolves them through it.
    let kiCurDid = pCtx.uiDependencyId;
    let kbUseSubsetSpsFlag =
        !pCtx.param().bSimulcastAVC && (kiCurDid as i32) > BASE_DEPENDENCY_ID;
    let iSliceCount =
        current_layer_expect(pCtx).iMaxSliceNum;

    current_layer_expect_mut(pCtx).pDecPic =
        pCtx.pDecPic;

    debug_assert!(iSliceCount > 0);

    let (mut iCurPpsId, iCurSpsId) = {
        let pDqIdc = &ctx_dq_idc_map(pCtx)[kiCurDid as usize];
        (pDqIdc.iPpsId as i32, pDqIdc.iSpsId as i32)
    };

    let kiIdrLoop = (pCtx.param().sDependencyLayers[kiCurDid as usize].uiIdrPicId as i32 - 1)
        .abs()
        % MAX_PPS_COUNT as i32;
    iCurPpsId = ParasetStrategy(pCtx).GetCurrentPpsId(iCurPpsId, kiIdrLoop);

    let kbSliceHeaderExtFlag = pCtx.eNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;
    let keNalPriority = pCtx.eNalPriority;
    let keNalType = pCtx.eNalType;
    let kbNeedPrefixNalFlag = pCtx.bNeedPrefixNalFlag;
    let keSliceType = pCtx.eSliceType;
    let kuiTemporalId = pCtx.uiTemporalId;
    let kiFrameNum = pCtx.param().sDependencyLayers[kiCurDid as usize].iFrameNum;
    let pCurDq = current_layer_expect_mut(pCtx);

    pCurDq.sLayerInfo.iPps = Some(PpsId(iCurPpsId as u16));

    pCurDq.sLayerInfo.eSps = Some(if kbUseSubsetSpsFlag {
        LayerSps::Subset(SubsetSpsId(iCurSpsId as u8))
    } else {
        LayerSps::Avc(SpsId(iCurSpsId as u8))
    });

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

    let pNalHdExt = &mut pCurDq.sLayerInfo.sNalHeaderExt;
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

    // pEncPic data.
    let (Some(idEnc), Some(idDec)) = (pCtx.pEncPic, pCtx.pDecPic) else {
        return;
    };
    if pCtx.pVpp.is_none() {
        return;
    }
    current_layer_expect_mut(pCtx).pEncPic =
        Some(idEnc);

    let pEncPic = crate::encoder::encoder_context::ctx_vpp_mut(pCtx)
        .m_pSpatialPicPool
        .get_mut(idEnc)
        .planes();
    let pDecPic = pCtx
        .ref_list_mut(kiCurDid as usize)
        .expect("the layer's reference list is allocated")
        .pic_mut(idDec)
        .planes();

    // This is the last point in the frame at which the reconstruction picture is
    // borrowed exclusively on the calling thread — everything after it is the
    // macroblock loop, which forks. From here on *nothing* in the frame may take
    // `&mut` on this picture again.
    //
    // Rebuilt every frame, unconditionally, because the pool may have handed
    // `idDec` a different slot: a view is only ever valid for the frame that
    // built it.
    let sRecView = crate::encoder::rec_view::RecPicView::build(
        pCtx.ref_list_mut(kiCurDid as usize)
            .expect("the layer's reference list is allocated")
            .pic_mut(idDec),
    );

    // The read half of the seam, built beside the write half above and rebuilt
    // every frame for the same reason.
    let sEncView = crate::encoder::rec_view::RoPicView::build(
        crate::encoder::encoder_context::ctx_vpp_ref(pCtx).m_pSpatialPicPool.get(idEnc),
    );

    let pCurDq = current_layer_expect_mut(pCtx);
    pCurDq.pRecView = Some(sRecView);
    pCurDq.pEncView = Some(sEncView);

    pCurDq.iEncStride[0] = pEncPic.iLineSize[0];
    pCurDq.iEncStride[1] = pEncPic.iLineSize[1];
    pCurDq.iEncStride[2] = pEncPic.iLineSize[2];
    // cs data
    pCurDq.iCsStride[0] = pDecPic.iLineSize[0];
    pCurDq.iCsStride[1] = pDecPic.iLineSize[1];
    pCurDq.iCsStride[2] = pDecPic.iLineSize[2];

    pCurDq.bBaseLayerAvailableFlag = pCurDq.pRefLayer.is_some();

    // The count is the one `CreateTasks` computed for `WELS_ENC_TASK_UPDATEMBMAP`
    // (`sSliceArgument.uiSliceNum` for every non-`SM_SIZELIMITED_SLICE` mode), and
    // only the fixed modes can reach here: `bNeedAdjustingSlicing` is written by
    // `DynamicAdjustSlicing` alone, which only `AdjustBaseLayer`/`AdjustEnhanceLayer`
    // call, and only on the `SM_FIXEDSLCNUM_SLICE` arm.
    if pCtx.pSliceThreading.is_some()
        && !current_layer_ref(pCtx).is_none()
        && current_layer_expect(pCtx).bNeedAdjustingSlicing
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
    // The slot is `pOut.sNalLen` at the current layer's base plus this index,
    // which the body reaches directly.
    pNalIdxInLayer: &mut i32,
    keNalType: EWelsNalUnitType,
    keNalRefIdc: EWelsNalRefIdc,
    iPayloadSize: &mut i32,
) -> i32 {
    let mut iReturn;
    *iPayloadSize = 0;

    if keNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST {
        crate::encoder::nal_encap::WelsLoadNal(
            pCtx.out_mut(),
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            keNalRefIdc as i32,
        );

        {
            let pOut = pCtx.out_mut();
            crate::encoder::nal_encap::WelsWriteSVCPrefixNal(
                &mut pOut.sBsBuffer[..],
                &mut pOut.sBsWrite,
                keNalRefIdc as i32,
                keNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR,
            );
        }

        crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());
    } else {
        // No prefix NAL unit RBSP syntax here, but the NAL unit header extension is
        // still needed.
        crate::encoder::nal_encap::WelsLoadNal(
            pCtx.out_mut(),
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            keNalRefIdc as i32,
        );
        crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());
    }

    let kNalHeaderExt =
        current_layer_expect(pCtx).sLayerInfo.sNalHeaderExt;
    let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
    let crate::encoder::nal_encap::SWelsEncoderOutput {
        sNalList, sBsBuffer, sNalLen, iNalIndex, iNalLenBase, ..
    } = &mut **pOut.as_mut().expect("pOut lives");
    let kiPos = *iPosBsBuffer as usize;
    let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
    let kiSlot = *iNalLenBase + (*pNalIdxInLayer).max(0) as usize;
    let mut kiNalLenOut = 0i32;
    iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &sNalList[(*iNalIndex - 1) as usize],
        &sBsBuffer[..],
        Some(&kNalHeaderExt),
        pDstTail,
        &mut kiNalLenOut,
    );
    // Written through `&AtomicI32`, never `&mut i32` — a `&mut` here retags the
    // whole buffer `Unique` and pops the C-ABI pointer the application holds.
    sNalLen[kiSlot].store(kiNalLenOut, std::sync::atomic::Ordering::Relaxed);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }
    *iPayloadSize = kiNalLenOut;

    pCtx.iPosBsBuffer += *iPayloadSize;
    *pNalIdxInLayer += 1;

    iReturn = ENC_RETURN_SUCCESS;
    iReturn
}

/// `encoder_ext.cpp:3003`. Emit a filler-data NAL of `iLen` bytes.
pub fn WritePadding(pCtx: &mut sWelsEncCtx, iLen: i32, iSize: &mut i32) -> i32 {
    let mut iNalLen = 0i32;

    *iSize = 0;
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
        // The frame-level writer, for non-VCL NALs.
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
    // both sides were NULL on every target and the fields are deleted.
}

/// `encoder_ext.cpp:2630` (`static inline SetNormalCodingFunc`).
fn SetNormalCodingFunc(pFuncList: &mut SWelsFuncPtrList) {
    pFuncList.pfIntraFineMd = Some(crate::encoder::svc_base_layer_md::WelsMdIntraFinePartition);
    let sdf = &mut pFuncList.sSampleDealingFuncs;
    sdf.pfMdCost = CostFamily::Satd;
    // As `SetFastCodingFunc`: the three `Combined3` aims are deleted with the fields.
}

// `SetMeMethod` (`encoder_ext.cpp:2639-2662`) lives in
// `svc_motion_estimate::SetMeMethod` — beside the four search families it
// selects between, rather than here beside its caller.

/// `encoder_ext.cpp:2665`. Per-frame function-pointer selection. MUST be called after
/// `pfWelsRcPictureInit()` and `WelsInitCurrentLayer()`.
pub fn PreprocessSliceCoding(pCtx: &mut sWelsEncCtx) {
    let pCurLayer = current_layer_expect(pCtx);
    let bFastMode = pCtx.param().iComplexityMode == LOW_COMPLEXITY;
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
    let kbDeblockingParallelFlag = pCurLayer.bDeblockingParallelFlag;
    let kiLoopFilterDisableIdc = pCurLayer.iLoopFilterDisableIdc;

    // ---- the SCREEN_CONTENT_REAL_TIME block, first half (`encoder_ext.cpp:2708-2771`).
    //
    // Its `SFeatureSearchPreparation` half — which reaches the layer, the
    // reference list, the VAA block and the picture pools — runs **before** the
    // table's `&mut` is taken, where the C++ runs it after. The table half stays
    // at the C++'s position, below.
    let kbScreenP =
        kiUsageType == SCREEN_CONTENT_REAL_TIME && keSliceType == EWelsSliceType::P_SLICE;
    let kbScreenI =
        kiUsageType == SCREEN_CONTENT_REAL_TIME && keSliceType != EWelsSliceType::P_SLICE;
    // `SLogContext` is `Copy`; the two `SetMeMethod` warnings need it while `fl` lives.
    let kLogCtx = pCtx.sLogCtx;
    // `:2714-2716`. The scroll fields are the **extension's**; `iFrameSad` below is
    // the **base block's**.
    let (kbScroll, kiScrollMvX, kiScrollMvY) = match pCtx.vaa_ext_ref() {
        Some(pVaaExt) => (
            pVaaExt.sScrollDetectInfo.bScrollDetectFlag,
            pVaaExt.sScrollDetectInfo.iScrollMvX,
            pVaaExt.sScrollDetectInfo.iScrollMvY,
        ),
        None => (false, 0, 0),
    };
    let kiFrameSad = pCtx.vaa().map_or(0, |pVaa| pVaa.sVaaCalcInfo.iFrameSad); // :2738
    let kbLtr = pCtx.param().bEnableLongTermReference; // :2746
    // The layer's own dependency id, as `layer_ref_pic` resolves it — not
    // `pCtx.uiDependencyId`, which is the same value here and would not be under a
    // multi-layer frame loop that had moved on.
    let kiLayerDid = pCurLayer.sLayerInfo.sNalHeaderExt.uiDependencyId as usize;
    let kiMbSize = pCurLayer.iMbWidth as i32 * pCurLayer.iMbHeight as i32; // :2736
    let kbHasPrep = pCurLayer.pFeatureSearchPreparation.is_some();
    let kpRefPicId = pCurLayer.pRefPic;
    let kpRefOri0 = pCurLayer.pRefOri[0];

    // `:2730-2765`, the preparation half. Its two outputs feed the table writes.
    let (kbFmeSwitch, kbFmeInstalled) = if kbScreenP && kbHasPrep {
        // The preparation box comes out of the layer and the reference's feature
        // storage out of the reconstruction picture, so the picture's *planes* can
        // be borrowed shared (`PerformFMEPreprocess` reads them) while its own
        // storage is written through. Both go back below, inside this block, with
        // no `return` in between: a box left taken out would be a silent behaviour
        // change on the next frame — the features recomputed, or the switch never
        // firing.
        let mut prep = current_layer_expect_mut(pCtx).pFeatureSearchPreparation.take();
        let mut out = (false, false);
        if let Some(p) = prep.as_deref_mut() {
            p.iHighFreMbCount = 0; // :2732
            // `:2737-2739`. Both divisions are `int32_t`, as in the C++ — and the
            // first numerator is the zero just written, so the percentage the
            // reference still has a TODO about is always 0.
            p.bFMESwitchFlag = crate::encoder::svc_motion_estimate::CalcFMESwitchFlag(
                p.uiFMEGoodFrameCount,
                p.iHighFreMbCount * 100 / kiMbSize,
                kiFrameSad / kiMbSize,
                kbScroll,
            );
            let kbSwitch = p.bFMESwitchFlag;

            // `:2742`: the storage is the **reconstruction's** on every path — under
            // LTR only the plane source below moves. `:2743`
            // (`pFeatureSearchPreparation->pRefBlockFeature = ..`) has no counterpart:
            // the port does not carry that field, because the whole reference writes
            // it and never reads it.
            let mut storage = kpRefPicId.and_then(|id| {
                pCtx.ref_list_mut(kiLayerDid)
                    .and_then(|pRefList| pRefList.pic_mut(id).pScreenBlockFeatureStorage.take())
            });
            let mut kbInstalled = false;
            if let Some(st) = storage.as_deref_mut() {
                if kbSwitch && !st.bRefBlockFeatureCalculated {
                    // `:2744-2749`
                    let kernels =
                        crate::encoder::svc_motion_estimate::FmeKernels::of(pCtx.func_list());
                    // `:2746` — the original frame under LTR, the reconstruction
                    // otherwise. Under LTR `pRefOri[0]` is a *source-pool* picture on
                    // the screen path, whose `iFrameAverageQp` `UpdateOriginalPicInfo`
                    // copied off the reconstruction at the previous frame's end;
                    // without LTR it is the picture whose storage was just taken out,
                    // which is why the planes are borrowed shared.
                    let pRef: Option<&SPicture> = if kbLtr {
                        kpRefOri0
                            .and_then(|r| crate::encoder::svc_encode_slice::ctx_pic_ref(pCtx, r))
                    } else {
                        kpRefPicId.and_then(|id| pCtx.ref_list(kiLayerDid).map(|l| l.pic(id)))
                    };
                    if let Some(pRef) = pRef {
                        crate::encoder::svc_motion_estimate::PerformFMEPreprocess(
                            &kernels,
                            pRef,
                            &mut p.pFeatureOfBlock,
                            st,
                        );
                    }
                }
                kbInstalled = kbSwitch && st.bRefBlockFeatureCalculated && st.iIs16x16 == 0; // :2752-2753
            }
            // Put the storage back, resolved exactly as the take resolved it.
            if let (Some(id), Some(st)) = (kpRefPicId, storage) {
                if let Some(pRefList) = pCtx.ref_list_mut(kiLayerDid) {
                    pRefList.pic_mut(id).pScreenBlockFeatureStorage = Some(st);
                }
            }
            out = (kbSwitch, kbInstalled);
        }
        current_layer_expect_mut(pCtx).pFeatureSearchPreparation = prep;
        out
    } else {
        if kbScreenI {
            // `:2766-2769` — reset some status when at I_SLICE. The C++ dereferences
            // the preparation unconditionally here and may: `ParamValidation`
            // refuses screen content above one spatial layer
            // (`encoder_ext.cpp:274-279`), so the only DQ layer is the last one,
            // which is the one that carries a preparation. The port asks anyway.
            if let Some(p) = current_layer_expect_mut(pCtx)
                .pFeatureSearchPreparation
                .as_deref_mut()
            {
                p.bFMESwitchFlag = true;
                p.uiFMEGoodFrameCount =
                    crate::encoder::svc_motion_estimate::FMESWITCH_DEFAULT_GOODFRAME_NUM;
            }
        }
        (false, false)
    };

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

    // ---- the SCREEN_CONTENT_REAL_TIME block, table half (`encoder_ext.cpp:2710-2765`),
    // at the C++'s own position. Its preparation half ran above the table's `&mut`;
    // `kbFmeSwitch` and `kbFmeInstalled` are that half's two outputs, and every
    // value below is a `Copy` scalar lifted with them.
    if kbScreenP {
        //to init at each frame will be needed when dealing with hybrid content (camera+screen)
        //MD related func pointers
        fl.pfInterFineMd =
            Some(crate::encoder::svc_mode_decision::WelsMdInterFinePartitionVaaOnScreen);

        //ME related func pointers
        fl.pfSetScrollingMv = Some(if kbScroll && (kiScrollMvX | kiScrollMvY) != 0 {
            crate::encoder::svc_mode_decision::SetScrollingMvToMd
        } else {
            crate::encoder::svc_mode_decision::SetScrollingMvToMdNull
        });

        // Indexed by `EStaticBlockIdc`, which is **not** `pfSearchMethod`'s index
        // space. The P-slice block above filled all three with
        // `WelsMotionEstimateSearch`; the C++ re-states `NO_STATIC` here and so
        // does this.
        fl.pfMotionSearch[EStaticBlockIdc::NO_STATIC as usize] =
            Some(crate::encoder::svc_motion_estimate::WelsMotionEstimateSearch);
        fl.pfMotionSearch[EStaticBlockIdc::COLLOCATED_STATIC as usize] =
            Some(crate::encoder::svc_motion_estimate::WelsMotionEstimateSearchStatic);
        fl.pfMotionSearch[EStaticBlockIdc::SCROLLED_STATIC as usize] =
            Some(crate::encoder::svc_motion_estimate::WelsMotionEstimateSearchScrolled);

        //ME16x16
        if !crate::encoder::svc_motion_estimate::SetMeMethod(
            ME_DIA_CROSS,
            &mut fl.sMeFuncs.pfSearchMethod[BLOCK_16x16],
        ) {
            // Neither warning can fire — both constants are honoured cases — but
            // both are ported, at WARNING as upstream has them.
            crate::common::wels_trace::WelsLog(
                kLogCtx,
                crate::common::wels_trace::WELS_LOG_WARNING,
                "SetMeMethod(BLOCK_16x16) ME_DIA_CROSS unsuccessful, switched to default search",
            );
        }

        //ME8x8
        if kbHasPrep {
            if kbFmeInstalled
                && !crate::encoder::svc_motion_estimate::SetMeMethod(
                    ME_DIA_CROSS_FME,
                    &mut fl.sMeFuncs.pfSearchMethod[BLOCK_8x8],
                )
            {
                crate::common::wels_trace::WelsLog(
                    kLogCtx,
                    crate::common::wels_trace::WELS_LOG_WARNING,
                    "SetMeMethod(BLOCK_8x8) ME_DIA_CROSS_FME unsuccessful, switched to default search",
                );
            }

            //assign UpdateFMESwitch pointer
            fl.pfUpdateFMESwitch = Some(if kbFmeSwitch {
                crate::encoder::svc_motion_estimate::UpdateFMESwitch
            } else {
                crate::encoder::svc_motion_estimate::UpdateFMESwitchNull
            });
        }
    }

    // update some layer-dependent variables to save judgements at MB level
    let sdf = &fl.sSampleDealingFuncs;
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

    // The loop-invariant write hoists to the frame level, before anything spawns;
    // the per-slice readers see the same value in the same order.
    fl.pfInterMd = if kbBaseAvail && kbHighestSpatial {
        Some(crate::encoder::svc_mode_decision::WelsMdInterMbEnhancelayer)
    } else {
        Some(crate::encoder::svc_base_layer_md::WelsMdInterMb)
    };

    current_layer_expect_mut(pCtx)
        .bSatdInMdFlag = kbSatdInMd;
}

/// `encoder_ext.cpp:3131`. Write the parameter sets for (simulcast) SVC.
pub fn WriteSsvcParaset(
    pCtx: &mut sWelsEncCtx,
    kiSpatialNum: i32,
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iCountNal = 0i32;

    let iReturn = crate::encoder::wels_encoder_ext::WelsWriteParameterSets(
        pCtx,
        &mut iCountNal,
        &mut iNonVclSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

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
    *iLbi += 1;
    pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
    // The next layer's slot is this one's plus its NAL count — the pointer
    // chain's arithmetic, in `sNalLen`'s own units.
    {
        let pOut = pCtx.out_mut();
        pOut.iLayerBsIndex += 1;
        pOut.advance_nal_len_base(iCountNal.max(0) as usize);
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = pOut.nal_len_ptr();
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
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iNalSize = 0i32;
    let mut iCountNal;

    // --- SPS ---
    let iId = pCtx.sps_array()[iIdx as usize].uiSpsId;
    if let Some(pStrategy) = pCtx.func_list_mut().pParametersetStrategy.as_mut() {
        pStrategy.Update(iId, PARA_SET_TYPE_AVCSPS as i32);
    }

    let mut iReturn =
        crate::encoder::wels_encoder_ext::WelsWriteOneSPS(pCtx, iIdx, &mut iNalSize);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    pCtx.out().set_nal_len_at(0, iNalSize);
    iNonVclSize += iNalSize;
    iCountNal = 1;

    pFbi.sLayerInfo[*iLbi].uiSpatialId = iIdx as u8;
    pFbi.sLayerInfo[*iLbi].uiTemporalId = 0;
    pFbi.sLayerInfo[*iLbi].uiQualityId = 0;
    pFbi.sLayerInfo[*iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
    pFbi.sLayerInfo[*iLbi].iNalCount = iCountNal;
    pFbi.sLayerInfo[*iLbi].eFrameType = EVideoFrameType::videoFrameTypeIDR;
    pFbi.sLayerInfo[*iLbi].iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    *iLbi += 1;
    pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
    // The next layer's slot is this one's plus its NAL count — the pointer
    // chain's arithmetic, in `sNalLen`'s own units.
    {
        let pOut = pCtx.out_mut();
        pOut.iLayerBsIndex += 1;
        pOut.advance_nal_len_base(iCountNal.max(0) as usize);
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = pOut.nal_len_ptr();
    }
    *iLayerNum += 1;

    // --- PPS ---
    iNalSize = 0;
    let iId = pCtx.pps_array()[iIdx as usize].iPpsId;
    if let Some(pStrategy) = pCtx.func_list_mut().pParametersetStrategy.as_mut() {
        pStrategy.Update(iId, PARA_SET_TYPE_PPS as i32);
    }
    iReturn = crate::encoder::wels_encoder_ext::WelsWriteOnePPS(pCtx, iIdx, &mut iNalSize);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }
    pCtx.out().set_nal_len_at(0, iNalSize);
    iNonVclSize += iNalSize;
    iCountNal = 1;

    pFbi.sLayerInfo[*iLbi].uiSpatialId = iIdx as u8;
    pFbi.sLayerInfo[*iLbi].uiTemporalId = 0;
    pFbi.sLayerInfo[*iLbi].uiQualityId = 0;
    pFbi.sLayerInfo[*iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
    pFbi.sLayerInfo[*iLbi].iNalCount = iCountNal;
    pFbi.sLayerInfo[*iLbi].eFrameType = EVideoFrameType::videoFrameTypeIDR;
    pFbi.sLayerInfo[*iLbi].iSubSeqId = GetSubSequenceId(pCtx, EVideoFrameType::videoFrameTypeIDR);

    *iLbi += 1;
    pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
    // The next layer's slot is this one's plus its NAL count — the pointer
    // chain's arithmetic, in `sNalLen`'s own units.
    {
        let pOut = pCtx.out_mut();
        pOut.iLayerBsIndex += 1;
        pOut.advance_nal_len_base(iCountNal.max(0) as usize);
        pFbi.sLayerInfo[*iLbi].pNalLengthInByte = pOut.nal_len_ptr();
    }
    *iLayerNum += 1;

    *iFrameSize += iNonVclSize;
    ENC_RETURN_SUCCESS
}

/// `encoder_ext.cpp:3251` — the parameter-set writer for the three **listing**
/// strategies.
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
/// # Panics
/// Panics if `pOut.sNalLen` is shorter than the parameter-set counts
/// `GetNeededSpsNum`/`GetNeededPpsNum` asked `RequestMemorySvc` for, or if
/// `pFbi.sLayerInfo` has no room for `2 * kiSpatialNum` more layers past `*iLbi`.
pub fn WriteSavcParaset_Listing(
    pCtx: &mut sWelsEncCtx,
    kiSpatialNum: i32,
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
) -> i32 {
    let mut iNonVclSize = 0i32;
    let mut iReturn = ENC_RETURN_SUCCESS;

    // --- SPS list, per spatial layer ---
    for iSpatialId in 0..kiSpatialNum {
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
            pCtx.out().set_nal_len_at(iCountNal.max(0) as usize, iNalSize);
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

        *iLbi += 1;
        pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
        // The next layer's slot is this one's plus its NAL count — the pointer
        // chain's arithmetic, in `sNalLen`'s own units.
        {
            let pOut = pCtx.out_mut();
            pOut.iLayerBsIndex += 1;
            pOut.advance_nal_len_base(iCountNal.max(0) as usize);
            pFbi.sLayerInfo[*iLbi].pNalLengthInByte = pOut.nal_len_ptr();
        }
        *iLayerNum += 1;
    }

    // --- PPS list, per spatial layer ---
    //
    // `encoder_ext.cpp:3297`. It is a no-op for four of the five kinds and the
    // whole point of `SPS_PPS_LISTING`.
    {
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
            pCtx.out().set_nal_len_at(iCountNal.max(0) as usize, iNalSize);
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

        *iLbi += 1;
        pFbi.sLayerInfo[*iLbi].pBsBuf = pCtx.frame_bs_cur();
        // The next layer's slot is this one's plus its NAL count — the pointer
        // chain's arithmetic, in `sNalLen`'s own units.
        {
            let pOut = pCtx.out_mut();
            pOut.iLayerBsIndex += 1;
            pOut.advance_nal_len_base(iCountNal.max(0) as usize);
            pFbi.sLayerInfo[*iLbi].pNalLengthInByte = pOut.nal_len_ptr();
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
    pFbi: &mut SFrameBSInfo,
    iLbi: &mut usize,
    iSpatialNum: i32,
    iCurDid: &mut i8,
    iCurTid: &mut i32,
    iLayerNum: &mut i32,
    iFrameSize: &mut i32,
    uiTimeStamp: i64,
) -> EVideoFrameType {
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
        let pfRc = pCtx.func_list().pfRc;
        if kbSimulcastAVC {
            pfRc.WelsUpdateBufferWhenSkip(pCtx, *iCurDid as i32);
        } else {
            for i in 0..iSpatialNum as usize {
                let iDid = pCtx.sSpatialIndexMap[i].iDid;
                pfRc.WelsUpdateBufferWhenSkip(pCtx, iDid);
            }
        }
    } else {
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
/// # Panics
/// Panics if `sMbDataP` holds fewer macroblocks than `sSliceEncCtx.iMbNumInFrame`.
pub fn DynslcUpdateMbNeighbourInfoListForAllSlices(pCurDq: &mut SDqLayer) {
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
pub fn WelsInitCurrentQBLayerMltslc(pCtx: &mut sWelsEncCtx) {
    // pData init
    let Some(pCurDq) = current_layer_mut(pCtx) else {
        return;
    };
    // mb_neighbor
    DynslcUpdateMbNeighbourInfoListForAllSlices(pCurDq);
}

/// `UpdateSlicepEncCtxWithPartition` — encoder_ext.cpp:2430.
///
/// Splits the frame into `iPartitionNum` macroblock ranges and stamps
/// `pOverallMbMap` with the partition index. Note the trailing loop clears the
/// *whole* of the four partition arrays out to `MAX_THREADS_NUM`, not just the
/// entries beyond `iPartitionNum` that this call wrote.
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
/// # Panics
/// Panics if the frame's current DQ layer has not been stamped.
pub fn WelsInitCurrentDlayerMltslc(pCtx: &mut sWelsEncCtx, iPartitionNum: i32) {
    /// `#define byte_complexIMBat26 (60)`, local to this function in the C++.
    const byte_complexIMBat26: u32 = 60;

    UpdateSlicepEncCtxWithPartition(
        current_layer_expect_mut(pCtx),
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
                current_layer_expect(pCtx).sSliceEncCtx.iMbNumInFrame;
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
            / current_layer_expect(pCtx).sSliceEncCtx.iMaxSliceNumConstraint as u32;
        // C++ only WelsLogs a warning here when uiSliceSizeConstraint is smaller.
    }

    WelsInitCurrentQBLayerMltslc(pCtx);
}

/// `DynSliceRealloc` — encoder_ext.cpp:4525.
///
/// # Panics
/// Panics if the frame's current DQ layer has not been stamped.
pub fn DynSliceRealloc(
    pCtx: &mut sWelsEncCtx,
    pFbi: &mut SFrameBSInfo,
    iLbi: usize,
) -> i32 {
    let iMaxSliceNum = current_layer_expect(pCtx).iMaxSliceNum;
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
/// # Panics
/// Panics if the frame's current DQ layer has not been stamped.
pub fn WelsCodeOnePicPartition(
    pCtx: &mut sWelsEncCtx,
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

    {
        let pCurLayer = current_layer_expect_mut(pCtx);
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
            >= (current_layer_expect(pCtx).sSliceBufferInfo[uSlcBuffIdx].iMaxSliceNum - kiSliceIdxStep)
        {
            // insufficient memory in pSliceInLayer[]
            if pCtx.iActiveThreadsNum == 1 {
                // only single thread supports re-alloc now
                if DynSliceRealloc(pCtx, pFbi, iLbi) != 0 {
                    return ENC_RETURN_MEMALLOCERR;
                }
            } else if iSliceIdx >= current_layer_expect(pCtx).iMaxSliceNum {
                return ENC_RETURN_MEMALLOCERR;
            }
        }

        if kbNeedPrefix {
            iReturn = AddPrefixNal(
                pCtx,
                &mut pFbi.sLayerInfo[iLbi],
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

        crate::encoder::nal_encap::WelsLoadNal(pCtx.out_mut(), keNalType as i32, keNalRefIdc as i32);
        let mut sBank = std::mem::take(
            &mut current_layer_expect_mut(pCtx).sSliceBufferInfo[uSlcBuffIdx],
        );
        let kiCurSlot = iSliceIdx as usize;
        if kiCurSlot >= sBank.pSliceBuffer.len() {
            current_layer_expect_mut(pCtx).sSliceBufferInfo[uSlcBuffIdx] = sBank;
            return ENC_RETURN_UNEXPECTED;
        }
        let (kpHead, kpTail) = sBank.pSliceBuffer.split_at_mut(kiCurSlot + 1);
        let pCurSlice = &mut kpHead[kiCurSlot];
        pCurSlice.iSliceIdx = iSliceIdx;
        // The forward slot at the old ST index exactly (`iSliceIdx + step`,
        // i.e. `tail[step - 1]`).
        let pNextSlice = kpTail.get_mut((kiSliceIdxStep - 1) as usize);

        crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);

        let pOutRef = pCtx.out_mut();
        let mut vOutBsBuf = std::mem::take(&mut pOutRef.sBsBuffer);
        let mut sOutBsWrite = pOutRef.sBsWrite;
        let mut pCtxOutBs: Option<&mut crate::encoder::vlc_encoder::BsWriter> = Some(&mut sOutBsWrite);
        let mut sMbData = std::mem::replace(
            &mut current_layer_expect_mut(pCtx).sMbDataP,
            crate::safe::mb_grid::MbArray::empty(),
        );
        let mut sMbWindow = crate::safe::mb_grid::MbWindow::whole(&mut sMbData, 0);
        // The CABAC restore scratch — partition 0 is the only one a
        // single-threaded encode names (`kiSliceIdx % iActiveThreadsNum` with one
        // thread). Empty means never allocated for this configuration.
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
        current_layer_expect_mut(pCtx).sMbDataP = sMbData;
        current_layer_expect_mut(pCtx).sSliceBufferInfo[uSlcBuffIdx] = sBank;
        let pOutRef = pCtx.out_mut();
        pOutRef.sBsBuffer = vOutBsBuf;
        pOutRef.sBsWrite = sOutBsWrite;
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());

        let kNalHeaderExt =
            current_layer_expect(pCtx).sLayerInfo.sNalHeaderExt;
        let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
        let crate::encoder::nal_encap::SWelsEncoderOutput {
            sNalList, sBsBuffer, sNalLen, iNalIndex, iNalLenBase, ..
        } = &mut **pOut.as_mut().expect("pOut lives");
        let kiPos = *iPosBsBuffer as usize;
        let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
        let kiSlot = *iNalLenBase + iNalIdxInLayer.max(0) as usize;
        let mut kiNalLenOut = 0i32;
        iReturn = crate::encoder::nal_encap::WelsEncodeNal(
            &sNalList[(*iNalIndex - 1) as usize],
            &sBsBuffer[..],
            Some(&kNalHeaderExt),
            pDstTail,
            &mut kiNalLenOut,
        );
        // Written through `&AtomicI32`, never `&mut i32` — a `&mut` here retags the
        // whole buffer `Unique` and pops the C-ABI pointer the application holds.
        sNalLen[kiSlot].store(kiNalLenOut, std::sync::atomic::Ordering::Relaxed);
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        let iSliceSize = pCtx
            .out().nal_len_at(iNalIdxInLayer.max(0) as usize);

        pCtx.iPosBsBuffer += iSliceSize;
        iPartitionBsSize += iSliceSize;

        iNalIdxInLayer += 1;
        iSliceIdx += kiSliceStep; // iSliceIdx is not contiguous
        iAnyMbLeftInPartition = iEndMbIdxInPartition
            - current_layer_expect(pCtx).LastCodedMbIdxOfPartition[kiPartitionId].load(Ordering::Relaxed);
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
/// The `ENC_RETURN_UNSUPPORTED_PARA` returns that remain in this function are the
/// layer-count bounds (`iLayerNum >= MAX_LAYER_NUM_OF_FRAME`), not feature refusals.
///
/// # Panics
/// Panics if the context's coding parameters and output block have not been built
/// by [`WelsInitEncoderExt`].
pub fn WelsEncoderEncodeExt(
    pCtx: &mut sWelsEncCtx,
    pFbi: &mut SFrameBSInfo,
    pSrcPic: &SSourcePicture,
) -> i32 {
    let fFrameRateHighest = {
        let p = pCtx.param();
        p.sSpatialLayers[p.iSpatialLayerNum as usize - 1].fFrameRate
    };
    // The reconstruction picture the PSNR block measures, **as a handle**.
    //
    // The snapshot itself is load-bearing: `pCtx.pDecPic` cannot be re-read at
    // the PSNR block, because `UpdateRefList` runs in between and ends in
    // `EndofUpdateRefList` -> `PrefetchNextBuffer`, which reassigns it to the
    // *next* frame's target.
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
    let mut iCurDid: i8;
    let mut iCurTid: i32 = 0;

    pCtx.iEncoderError = ENC_RETURN_SUCCESS;
    pCtx.bCurFrameMarkedAsSceneLtr = false;
    pFbi.eFrameType = EVideoFrameType::videoFrameTypeSkip;
    pFbi.iLayerNum = 0; // for initialization
    pFbi.uiTimeStamp = crate::encoder::rc::GetTimestampForRc(
        pSrcPic.uiTimeStamp,
        pCtx.uiLastTimestamp,
        fFrameRateHighest,
    );
    for iNalIdx in 0..MAX_LAYER_NUM_OF_FRAME as usize {
        pFbi.sLayerInfo[iNalIdx].eFrameType = EVideoFrameType::videoFrameTypeSkip;
        pFbi.sLayerInfo[iNalIdx].iNalCount = 0;
    }

    let mut iLbi: usize = 0;

    // perform csc/denoise/downsample/padding, generate spatial layers
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

    crate::encoder::encoder_context::InitBitStream(pCtx);
    pFbi.sLayerInfo[iLbi].pBsBuf = pCtx.frame_bs();
    {
        // The frame's first layer starts at entry 0 of `pOut.sNalLen`.
        let pOut = pCtx.out_mut();
        pOut.iNalLenBase = 0;
        pFbi.sLayerInfo[iLbi].pNalLengthInByte = pOut.nal_len_ptr();
    }
    iCurDid = pCtx.sSpatialIndexMap[0].iDid as i8;
    set_current_layer(pCtx, Some(LayerIdx(iCurDid as u8)));
    current_layer_expect_mut(pCtx).pRefLayer = None;

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
            let iTemporalId = GetTemporalLevel(
                &pCtx.param().sDependencyLayers[iDidIdx],
                pCtx.param().sDependencyLayers[iDidIdx].iCodingIndex,
                pCtx.param().uiGopSize as i32,
            );
            if iTemporalId == INVALID_TEMPORAL_ID as i32 {
                pCtx.param_mut().sDependencyLayers[iDidIdx].iCodingIndex += 1;
            }
        }
    }

    while iSpatialIdx < iSpatialNum {
        iCurDid = pCtx.sSpatialIndexMap[iSpatialIdx as usize].iDid as i8;
        let iDecompositionStages =
            pCtx.param().sDependencyLayers[iCurDid as usize].iDecompositionStages as i32;
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

        let idEncPic = pCtx.sSpatialIndexMap[iSpatialIdx as usize]
            .pSrc
            .expect("the spatial index map names a live source picture");
        pCtx.pEncPic = Some(idEncPic);
        {
            let kiPictureType = pCtx.eSliceType as i32;
            let kiFramePoc = pCtx.param().sDependencyLayers[iCurDid as usize].iPOC;
            let p = crate::encoder::encoder_context::ctx_vpp_mut(pCtx)
                .m_pSpatialPicPool
                .get_mut(idEncPic);
            p.iPictureType = kiPictureType;
            p.iFramePoc = kiFramePoc;
        }

        iCurWidth = pCtx.param().sSpatialLayers[iCurDid as usize].iVideoWidth;
        iCurHeight = pCtx.param().sSpatialLayers[iCurDid as usize].iVideoHeight;

        match pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceMode {
            // **The consumer half of the load-balancing loop.** The producer,
            // `CalcSliceComplexRatio`, runs at the end of this same layer body under
            // the same four-term guard.
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
        let iSliceCount;
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
        if pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceMode == SM_SINGLE_SLICE {
            // only one slice within a quality layer
            let mut iPayloadSize = 0i32;
            let mut sBank = std::mem::take(
                &mut current_layer_expect_mut(pCtx).sSliceBufferInfo[0],
            );
            let pCurSlice = sBank
                .pSliceBuffer
                .get_mut(0)
                .expect("the single-slice bank holds slot 0");

            if pCtx.bNeedPrefixNalFlag {
                pCtx.iEncoderError = AddPrefixNal(pCtx,
                    &mut pFbi.sLayerInfo[iLbi],
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
                pCtx.out_mut(),
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

            crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);
            let pOutRef = pCtx.out_mut();
            let mut vOutBsBuf = std::mem::take(&mut pOutRef.sBsBuffer);
            let mut sOutBsWrite = pOutRef.sBsWrite;
            let mut pCtxOutBs: Option<&mut crate::encoder::vlc_encoder::BsWriter> = Some(&mut sOutBsWrite);
            let mut sMbData = std::mem::replace(
                &mut current_layer_expect_mut(pCtx).sMbDataP,
                crate::safe::mb_grid::MbArray::empty(),
            );
            let mut sMbWindow = crate::safe::mb_grid::MbWindow::whole(&mut sMbData, 0);
            // The CABAC restore scratch — partition 0 is the only one a
            // single-threaded encode names (`kiSliceIdx % iActiveThreadsNum` with
            // one thread). Empty means never allocated for this configuration.
            let mut vRestoreBuf = std::mem::take(&mut pCtx.pDynamicBsBuffer[0]);
            let pRestoreBuf =
                if vRestoreBuf.is_empty() { None } else { Some(vRestoreBuf.as_mut_slice()) };
            // Single-slice — the dynamic boundary never fires.
            let iCodeRet =
                crate::encoder::svc_encode_slice::WelsCodeOneSlice(pCtx, &mut *pCurSlice, eNalType as i32, vOutBsBuf.as_mut_slice(), &mut pCtxOutBs, &mut sMbWindow, pRestoreBuf, None);
            pCtx.pDynamicBsBuffer[0] = vRestoreBuf;
            drop(sMbWindow);
            current_layer_expect_mut(pCtx).sMbDataP = sMbData;
            current_layer_expect_mut(pCtx).sSliceBufferInfo[0] = sBank;
            let pOutRef = pCtx.out_mut();
            pOutRef.sBsBuffer = vOutBsBuf;
            pOutRef.sBsWrite = sOutBsWrite;
            pCtx.iEncoderError = iCodeRet;
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }

            crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());

            let kNalHeaderExt =
                current_layer_expect(pCtx).sLayerInfo.sNalHeaderExt;
            let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
            let crate::encoder::nal_encap::SWelsEncoderOutput {
                sNalList, sBsBuffer, sNalLen, iNalIndex, iNalLenBase, ..
            } = &mut **pOut.as_mut().expect("pOut lives");
            let kiPos = *iPosBsBuffer as usize;
            let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
            let kiSlot = *iNalLenBase + iNalIdxInLayer.max(0) as usize;
            let mut kiNalLenOut = 0i32;
            let kiEncodeNalRet = crate::encoder::nal_encap::WelsEncodeNal(
                &sNalList[*iNalIndex as usize - 1],
                &sBsBuffer[..],
                Some(&kNalHeaderExt),
                pDstTail,
                &mut kiNalLenOut,
            );
            // Written through `&AtomicI32`, never `&mut i32` — a `&mut` here retags
            // the whole buffer `Unique` and pops the C-ABI pointer the application
            // holds.
            sNalLen[kiSlot].store(kiNalLenOut, std::sync::atomic::Ordering::Relaxed);
            pCtx.iEncoderError = kiEncodeNalRet;
            if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                return pCtx.iEncoderError;
            }
            let iSliceSize = pCtx
                .out().nal_len_at(iNalIdxInLayer.max(0) as usize);

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
        } else if pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceMode
            == SM_SIZELIMITED_SLICE
            && pCtx.param().iMultipleThreadIdc <= 1
        {
            // dynamic slicing, single threading
            let kiLastMbInFrame = current_layer_expect(pCtx).sSliceEncCtx.iMbNumInFrame;
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
        } else if pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceMode != SM_SIZELIMITED_SLICE
            && pCtx.param().iMultipleThreadIdc > 1
        {
            // THREAD_FULLY_FIRE_MODE/THREAD_PICK_UP_MODE for any mode of
            // non-SM_SIZELIMITED_SLICE
            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(current_layer_expect_mut(pCtx));
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
        } else if pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceMode
            == SM_SIZELIMITED_SLICE
            && pCtx.param().iMultipleThreadIdc > 1
        {
            // THREAD_FULLY_FIRE_MODE && SM_SIZELIMITED_SLICE
            let kiPartitionCnt = pCtx.iActiveThreadsNum as i32;

            //TODO: use a function to remove duplicate code here and ln3994
            let iLayerBsIdx = pCtx.out().iLayerBsIndex;
            let pLbi = &mut pFbi.sLayerInfo[iLayerBsIdx as usize];
            pLbi.pBsBuf = pCtx.frame_bs_cur();
            pLbi.uiLayerType = VIDEO_CODING_LAYER;
            pLbi.uiSpatialId = pCtx.uiDependencyId;
            pLbi.uiTemporalId = pCtx.uiTemporalId;
            pLbi.uiQualityId = 0;
            pLbi.eFrameType = eFrameType;
            pLbi.iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            pLbi.iNalCount = 0;

            let mut iRet = crate::encoder::svc_encode_slice::InitAllSlicesInThread(pCtx);
            if iRet != 0 {
                return ENC_RETURN_UNEXPECTED;
            }
            pCtx.iEncoderError |=
                crate::encoder::slice_multi_threading::EncodeSizeLimitedSlicesForked(pCtx,
                    kiPartitionCnt,
                );

            if pCtx.iEncoderError != 0 {
                return pCtx.iEncoderError;
            }

            let kuiSliceMode =
                pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceMode;
            iRet = crate::encoder::svc_encode_slice::SliceLayerInfoUpdate(
                pCtx,
                pFbi,
                iLbi,
                kuiSliceMode,
            );
            if iRet != 0 {
                return ENC_RETURN_UNEXPECTED;
            }

            iSliceCount =
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(current_layer_expect_mut(pCtx));
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
                crate::encoder::svc_encode_slice::GetCurrentSliceNum(current_layer_expect_mut(pCtx));
            while iSliceIdx < iSliceCount {
                let mut iPayloadSize = 0i32;

                if bNeedPrefix {
                    pCtx.iEncoderError = AddPrefixNal(pCtx,
                        &mut pFbi.sLayerInfo[iLbi],
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
                    pCtx.out_mut(),
                    eNalType as i32,
                    eNalRefIdc as i32,
                );

                let mut sBank = std::mem::take(
                    &mut current_layer_expect_mut(pCtx).sSliceBufferInfo[0],
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

                crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);
                let pOutRef = pCtx.out_mut();
                let mut vOutBsBuf = std::mem::take(&mut pOutRef.sBsBuffer);
                let mut sOutBsWrite = pOutRef.sBsWrite;
                let mut pCtxOutBs: Option<&mut crate::encoder::vlc_encoder::BsWriter> = Some(&mut sOutBsWrite);
                let mut sMbData = std::mem::replace(
                    &mut current_layer_expect_mut(pCtx).sMbDataP,
                    crate::safe::mb_grid::MbArray::empty(),
                );
                let mut sMbWindow = crate::safe::mb_grid::MbWindow::whole(&mut sMbData, 0);
                // The CABAC restore scratch — partition 0 is the only one a
                // single-threaded encode names (`kiSliceIdx % iActiveThreadsNum`
                // with one thread). Empty means never allocated for this
                // configuration.
                let mut vRestoreBuf = std::mem::take(&mut pCtx.pDynamicBsBuffer[0]);
                let pRestoreBuf =
                    if vRestoreBuf.is_empty() { None } else { Some(vRestoreBuf.as_mut_slice()) };
                // Fixed-mode ST — the dynamic boundary never fires.
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
                current_layer_expect_mut(pCtx).sMbDataP = sMbData;
                current_layer_expect_mut(pCtx).sSliceBufferInfo[0] = sBank;
                let pOutRef = pCtx.out_mut();
                pOutRef.sBsBuffer = vOutBsBuf;
                pOutRef.sBsWrite = sOutBsWrite;
                pCtx.iEncoderError = iCodeRet;
                if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                    return pCtx.iEncoderError;
                }

                crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());

                let kNalHeaderExt =
                    current_layer_expect(pCtx).sLayerInfo.sNalHeaderExt;
                let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
                let crate::encoder::nal_encap::SWelsEncoderOutput {
                    sNalList, sBsBuffer, sNalLen, iNalIndex, iNalLenBase, ..
                } = &mut **pOut.as_mut().expect("pOut lives");
                let kiPos = *iPosBsBuffer as usize;
                let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
                let kiSlot = *iNalLenBase + iNalIdxInLayer.max(0) as usize;
                let mut kiNalLenOut = 0i32;
                let kiEncodeNalRet = crate::encoder::nal_encap::WelsEncodeNal(
                    &sNalList[*iNalIndex as usize - 1],
                    &sBsBuffer[..],
                    Some(&kNalHeaderExt),
                    pDstTail,
                    &mut kiNalLenOut,
                );
                // Written through `&AtomicI32`, never `&mut i32` — a `&mut` here
                // retags the whole buffer `Unique` and pops the C-ABI pointer the
                // application holds.
                sNalLen[kiSlot].store(kiNalLenOut, std::sync::atomic::Ordering::Relaxed);
                pCtx.iEncoderError = kiEncodeNalRet;
                if pCtx.iEncoderError != ENC_RETURN_SUCCESS {
                    return pCtx.iEncoderError;
                }
                let iSliceSize = pCtx
                    .out().nal_len_at(iNalIdxInLayer.max(0) as usize);

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

        if pCtx.func_list()
            .pfRc
            .WelsRcPostFrameSkipping(pCtx, iCurDid as i32, pFbi.uiTimeStamp)
        {
            StackBackEncoderStatus(pCtx, eFrameType);
            ClearFrameBsInfo(pCtx, &mut *pFbi);

            pCtx.func_list()
                .pfRc
                .WelsUpdateBufferWhenSkip(pCtx, iSpatialNum);

            crate::encoder::rc::WelsRcPostFrameSkippedUpdate(pCtx, iCurDid as i32);
            pCtx.iEncoderError = ENC_RETURN_SUCCESS;
            return ENC_RETURN_SUCCESS;
        }

        // deblocking filter. ENABLE_FRAME_DUMP is not defined, so the temporal-id
        // clause is compiled in.
        if !current_layer_expect(pCtx).bDeblockingParallelFlag
            && eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST
            && (pCtx.param().sDependencyLayers[iCurDid as usize].iHighestTemporalId == 0
                || iCurTid < pCtx.param().sDependencyLayers[iCurDid as usize].iHighestTemporalId as i32)
        {
            crate::encoder::deblocking::PerformDeblockingFilter(pCtx);
        }

        pCtx.func_list()
            .pfRc
            .WelsRcPictureInfoUpdate(pCtx, iLayerSize);
        iFrameSize += iLayerSize;
        crate::encoder::rc::RcTraceFrameBits(pCtx, pFbi.uiTimeStamp, iFrameSize);
        if let Some(id) = pCtx.pDecPic {
            let iAverageFrameQp = pCtx.rc_at(iCurDid as usize).iAverageFrameQp;
            if let Some(pRefList) = pCtx.ref_list_mut(iCurDid as usize) {
                pRefList.pic_mut(id).iFrameAverageQp = iAverageFrameQp;
            }
        }

        // update scc related
        //
        // Position is the C++'s (`encoder_ext.cpp:3891-3897`): after
        // `pDecPic->iFrameAverageQp` is stamped, before the reference list is
        // updated.
        let pfUpdateFMESwitch = pCtx.func_list().pfUpdateFMESwitch;
        if let Some(f) = pfUpdateFMESwitch {
            f(current_layer_expect_mut(pCtx));
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
        // `-1.0` is the null-plane sentinel, answered where the handle
        // is rather than inside the kernel.
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
                if pCtx.param().bPsnrY || pSrcPic.bPsnrY {
                    fSnrY = plane_psnr(0, iCurWidth, iCurHeight);
                }
                if pCtx.param().bPsnrU || pSrcPic.bPsnrU {
                    fSnrU = plane_psnr(1, iCurWidth >> 1, iCurHeight >> 1);
                }
                if pCtx.param().bPsnrV || pSrcPic.bPsnrV {
                    fSnrV = plane_psnr(2, iCurWidth >> 1, iCurHeight >> 1);
                }
            }
        }

        pFbi.sLayerInfo[iLbi].rPsnr[0] = 0.0;
        pFbi.sLayerInfo[iLbi].rPsnr[1] = 0.0;
        pFbi.sLayerInfo[iLbi].rPsnr[2] = 0.0;
        if pSrcPic.bPsnrY {
            pFbi.sLayerInfo[iLbi].rPsnr[0] = fSnrY;
        }
        if pSrcPic.bPsnrU {
            pFbi.sLayerInfo[iLbi].rPsnr[1] = fSnrU;
        }
        if pSrcPic.bPsnrV {
            pFbi.sLayerInfo[iLbi].rPsnr[2] = fSnrV;
        }

        iCountNal = pFbi.sLayerInfo[iLbi].iNalCount;
        iLayerNum += 1;
        // The NAL-length array is the application's `pNalLengthInByte`, a
        // C-ABI field: still a pointer, still advanced by the previous layer's
        // NAL count.
        iLbi += 1;
        pFbi.sLayerInfo[iLbi].pBsBuf = pCtx.frame_bs_cur();
        // The next layer's slot is this one's plus its NAL count — the pointer
        // chain's arithmetic, in `sNalLen`'s own units.
        {
            let pOut = pCtx.out_mut();
            pOut.iLayerBsIndex += 1;
            pOut.advance_nal_len_base(iCountNal.max(0) as usize);
            pFbi.sLayerInfo[iLbi].pNalLengthInByte = pOut.nal_len_ptr();
        }

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

            let did = pCtx.uiDependencyId as usize;
            let pRc = pCtx.rc_at_mut(did);
            pRc.iPaddingBitrateStat += pRc.iPaddingSize;
            pRc.iPaddingSize = 0;

            pFbi.sLayerInfo[iLbi].uiSpatialId = 0;
            pFbi.sLayerInfo[iLbi].uiTemporalId = 0;
            pFbi.sLayerInfo[iLbi].uiQualityId = 0;
            pFbi.sLayerInfo[iLbi].uiLayerType = NON_VIDEO_CODING_LAYER;
            pFbi.sLayerInfo[iLbi].iNalCount = 1;
            pCtx.out().set_nal_len_at(0, iPaddingNalSize);
            pFbi.sLayerInfo[iLbi].eFrameType = eFrameType;
            pFbi.sLayerInfo[iLbi].iSubSeqId = GetSubSequenceId(pCtx, eFrameType);
            iLbi += 1;
            pFbi.sLayerInfo[iLbi].pBsBuf = pCtx.frame_bs_cur();
            // The next layer's slot is this one's plus its NAL count — the
            // pointer chain's arithmetic, in `sNalLen`'s own units.
            {
                let pOut = pCtx.out_mut();
                pOut.iLayerBsIndex += 1;
                pOut.advance_nal_len_base(1);
                pFbi.sLayerInfo[iLbi].pNalLengthInByte = pOut.nal_len_ptr();
            }
            iLayerNum += 1;

            iFrameSize += iPaddingNalSize;
        }

        // The producer half of the load-balancing loop, at the C++'s own site:
        // `encoder_ext.cpp:4064-4073`, end of the per-layer body, after the padding
        // block and immediately above the `eLastNalPriority` stamp — and under the
        // C++'s own four-term guard, which is the same one the consumer arm above
        // already reproduces. The workers stamped `uiSliceConsumeTime` on their way
        // through `EncodeOneSliceInJob` (`bRecordsTime`, which is
        // `bUseLoadBalancing`); this turns those times into the `iSliceComplexRatio`
        // that next frame's `DynamicAdjustSlicing` reads.
        //
        // The `MT_DEBUG`-only `TrackSliceComplexities` that follows it in the C++ has
        // no counterpart here and needs none: `MT_DEBUG` is off in every build either
        // project makes.
        if pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceMode == SliceModeEnum::SM_FIXEDSLCNUM_SLICE
            && pCtx.param().bUseLoadBalancing
            && pCtx.param().iMultipleThreadIdc > 1
            && pCtx.param().iMultipleThreadIdc
                >= pCtx.param().sSpatialLayers[iCurDid as usize].sSliceArgument.uiSliceNum as u16
        {
            crate::encoder::slice_multi_threading::CalcSliceComplexRatio(current_layer_expect_mut(pCtx));
        }

        pCtx.eLastNalPriority[iCurDid as usize] = eNalRefIdc;
        iSpatialIdx += 1;

        if (iCurDid as i32) + 1 < pCtx.param().iSpatialLayerNum {
            // iSpatialIdx has already been incremented, so this points at the next layer.
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
            pCtx.param_mut().sDependencyLayers[iCurDid as usize].iCodingIndex += 1;
        }
    } // end of (iSpatialIdx/iSpatialNum)

    if !pCtx.param().bSimulcastAVC {
        for i in 0..pCtx.param().iSpatialLayerNum as usize {
            pCtx.param_mut().sDependencyLayers[i].iCodingIndex += 1;
        }
    }

    if ENC_RETURN_CORRECTED == pCtx.iEncoderError {
        // `iSpatialIdx == iSpatialNum` here — the loop above ran to completion —
        // so this addresses the slot *after* the last one the frame wrote.
        // Upstream indexes it anyway (`encoder_ext.cpp:4109-4110`).
        //
        // The map is `[SSpatialPicIndex; 4]`; at 1, 2 or 3 the read is in bounds
        // and `get` returns the same byte it always did. The fifth slot answers
        // `0` — what an unwritten `SSpatialPicIndex` holds, which is what the
        // in-bounds cases read anyway.
        let iDid = pCtx.sSpatialIndexMap.get(iSpatialIdx as usize).map_or(0, |e| e.iDid);
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
