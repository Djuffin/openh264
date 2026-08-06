//! Port of `codec/encoder/core/src/au_set.cpp` — access-unit / parameter-set
//! construction and the reference-frame limitation checks.
//!
//! **Incomplete.** Ported so far: the bitrate/level verification and
//! reference-frame limitation group, which is what `ParamValidation` needs.
//! Still missing, tracked as Phase 3.7: `WelsInitSps`, `WelsInitPps`,
//! `WelsInitSubsetSps`, `WelsWriteSpsSyntax`, `WelsWriteSubsetSpsSyntax`,
//! `WelsWritePpsSyntax`, `WelsWriteVUI`, `WelsGetLevelIdc`,
//! `WelsCheckLevelLimitation`.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

use crate::api::codec_api::ELevelIdc::LEVEL_UNKNOWN;
use crate::api::codec_api::{ELevelIdc, SSpatialLayerConfig};
use crate::api::codec_api::EUsageType::*;
use crate::decoder::nalu::g_ksLevelLimits;
use crate::encoder::param_svc::{SWelsSvcCodingParam, WELS_LOG2};
use crate::encoder::rc::{WELS_CLIP3, WELS_MAX};
use crate::encoder::wels_encoder_ext::{
    SLogContext, AUTO_REF_PIC_COUNT, ENC_RETURN_SUCCESS, ENC_RETURN_UNSUPPORTED_PARA,
    LONG_TERM_REF_NUM, LONG_TERM_REF_NUM_SCREEN, MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA,
    MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN, MIN_REF_PIC_COUNT, LEVEL_NUMBER,
};
use crate::encoder::param_svc::UNSPECIFIED_BIT_RATE;

/// `CpbBrNalFactor` — codec/common/inc/wels_common_defs.h:61.
/// Baseline, main and extended profiles.
pub const CpbBrNalFactor: i32 = 1200;

/// `WelsAdjustLevel` — au_set.cpp:76.
///
/// Walks up the level table from `pCurLevelIdx` until one whose max bitrate can
/// carry `iMaxSpatialBitrate`, and adopts it. Returns 0 on success, 1 if even
/// LEVEL_5_2 is too small.
pub fn WelsAdjustLevel(pSpatialLayer: &mut SSpatialLayerConfig, iCurLevelIdx: usize) -> i32 {
    let iMaxBitrate = pSpatialLayer.iMaxSpatialBitrate;
    let mut idx = iCurLevelIdx;
    loop {
        if iMaxBitrate <= g_ksLevelLimits[idx].uiMaxBR as i32 * CpbBrNalFactor {
            pSpatialLayer.uiLevelIdc = level_idc_from_raw(g_ksLevelLimits[idx].uiLevelIdc);
            return 0;
        }
        idx += 1;
        // C++ walks a pointer and stops once it has stepped past LEVEL_5_2
        if idx >= LEVEL_NUMBER
            || level_idc_from_raw(g_ksLevelLimits[idx].uiLevelIdc) == ELevelIdc::LEVEL_5_2
        {
            break;
        }
    }
    1
}

/// Maps the raw `uiLevelIdc` byte stored in the shared level table onto the
/// `ELevelIdc` enum used by the public parameter structs.
fn level_idc_from_raw(uiLevelIdc: u8) -> ELevelIdc {
    match uiLevelIdc {
        10 => ELevelIdc::LEVEL_1_0,
        9 => ELevelIdc::LEVEL_1_B,
        11 => ELevelIdc::LEVEL_1_1,
        12 => ELevelIdc::LEVEL_1_2,
        13 => ELevelIdc::LEVEL_1_3,
        20 => ELevelIdc::LEVEL_2_0,
        21 => ELevelIdc::LEVEL_2_1,
        22 => ELevelIdc::LEVEL_2_2,
        30 => ELevelIdc::LEVEL_3_0,
        31 => ELevelIdc::LEVEL_3_1,
        32 => ELevelIdc::LEVEL_3_2,
        40 => ELevelIdc::LEVEL_4_0,
        41 => ELevelIdc::LEVEL_4_1,
        42 => ELevelIdc::LEVEL_4_2,
        50 => ELevelIdc::LEVEL_5_0,
        51 => ELevelIdc::LEVEL_5_1,
        52 => ELevelIdc::LEVEL_5_2,
        _ => LEVEL_UNKNOWN,
    }
}

/// `WelsBitRateVerification` — codec/encoder/core/src/encoder_ext.cpp:74.
///
/// Declared in au_set.h, defined in encoder_ext.cpp; kept here with the rest of
/// the parameter-set helpers.
pub unsafe fn WelsBitRateVerification(
    _pLogCtx: *mut SLogContext,
    pLayerParam: *mut SSpatialLayerConfig,
    _iLayerId: i32,
) -> i32 {
    if (*pLayerParam).iSpatialBitrate <= 0
        || ((*pLayerParam).iSpatialBitrate as f32) < (*pLayerParam).fFrameRate
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // deal with LEVEL_MAX_BR and MAX_BR setting
    let mut iCurLevelIdx = 0usize;
    while level_idc_from_raw(g_ksLevelLimits[iCurLevelIdx].uiLevelIdc) != ELevelIdc::LEVEL_5_2
        && level_idc_from_raw(g_ksLevelLimits[iCurLevelIdx].uiLevelIdc)
            != (*pLayerParam).uiLevelIdc
    {
        iCurLevelIdx += 1;
    }
    let iLevelMaxBitrate = g_ksLevelLimits[iCurLevelIdx].uiMaxBR as i32 * CpbBrNalFactor;
    let iLevel52MaxBitrate = g_ksLevelLimits[LEVEL_NUMBER - 1].uiMaxBR as i32 * CpbBrNalFactor;

    if UNSPECIFIED_BIT_RATE != iLevelMaxBitrate {
        if (*pLayerParam).iMaxSpatialBitrate == UNSPECIFIED_BIT_RATE
            || (*pLayerParam).iMaxSpatialBitrate > iLevel52MaxBitrate
        {
            (*pLayerParam).iMaxSpatialBitrate = iLevelMaxBitrate;
        } else if (*pLayerParam).iMaxSpatialBitrate > iLevelMaxBitrate {
            WelsAdjustLevel(&mut *pLayerParam, iCurLevelIdx);
        }
    } else if (*pLayerParam).iMaxSpatialBitrate != UNSPECIFIED_BIT_RATE
        && (*pLayerParam).iMaxSpatialBitrate > iLevel52MaxBitrate
    {
        // no level limitation, only guard against an unreasonably large max
        (*pLayerParam).iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
    }

    // deal with iSpatialBitrate and iMaxSpatialBitrate setting
    if (*pLayerParam).iMaxSpatialBitrate != UNSPECIFIED_BIT_RATE
        && (*pLayerParam).iMaxSpatialBitrate < (*pLayerParam).iSpatialBitrate
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    ENC_RETURN_SUCCESS
}

/// `WelsCheckNumRefSetting` — au_set.cpp:88 (file-static in C++).
///
/// Reconciles `iLTRRefNum` / `iNumRefFrame` / `iMaxNumRefFrame` against the GOP
/// size and LTR settings. With `bStrictCheck` an under-sized `iNumRefFrame` is an
/// error; otherwise it is corrected in place.
pub unsafe fn WelsCheckNumRefSetting(
    _pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
    bStrictCheck: bool,
) -> i32 {
    // validate LTR num
    let iCurrentSupportedLtrNum = if (*pParam).iUsageType == CAMERA_VIDEO_REAL_TIME {
        LONG_TERM_REF_NUM
    } else {
        LONG_TERM_REF_NUM_SCREEN
    };
    if (*pParam).bEnableLongTermReference && iCurrentSupportedLtrNum != (*pParam).iLTRRefNum {
        (*pParam).iLTRRefNum = iCurrentSupportedLtrNum;
    } else if !(*pParam).bEnableLongTermReference {
        (*pParam).iLTRRefNum = 0;
    }

    // NB: the C++ carries a TODO saying the reasonable value is
    // WELS_MAX(1, WELS_LOG2(uiGopSize)) unconditionally, but changing it needs
    // reference-list updating changed too. Kept as-is.
    let iCurrentStrNum = if (*pParam).iUsageType == SCREEN_CONTENT_REAL_TIME
        && (*pParam).bEnableLongTermReference
    {
        WELS_MAX(1, WELS_LOG2((*pParam).uiGopSize))
    } else {
        WELS_MAX(1, ((*pParam).uiGopSize >> 1) as i32)
    };
    let mut iNeededRefNum = if (*pParam).uiIntraPeriod != 1 {
        iCurrentStrNum + (*pParam).iLTRRefNum
    } else {
        0
    };

    iNeededRefNum = WELS_CLIP3(
        iNeededRefNum,
        MIN_REF_PIC_COUNT,
        if (*pParam).iUsageType == CAMERA_VIDEO_REAL_TIME {
            MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA
        } else {
            MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN
        },
    );

    // adjust default or invalid input so iNumRefFrame is valid for the next step
    if (*pParam).iNumRefFrame == AUTO_REF_PIC_COUNT {
        (*pParam).iNumRefFrame = iNeededRefNum;
    } else if (*pParam).iNumRefFrame < iNeededRefNum {
        if bStrictCheck {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        (*pParam).iNumRefFrame = iNeededRefNum;
    }

    // if the setting is larger than needed, use the needed one and write the max
    // into the SPS, leaving memory sized for later expansion
    if (*pParam).iMaxNumRefFrame < (*pParam).iNumRefFrame {
        (*pParam).iMaxNumRefFrame = (*pParam).iNumRefFrame;
    }
    (*pParam).iNumRefFrame = iNeededRefNum;

    ENC_RETURN_SUCCESS
}

/// `WelsCheckRefFrameLimitationNumRefFirst` — au_set.cpp:135.
pub unsafe fn WelsCheckRefFrameLimitationNumRefFirst(
    pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
) -> i32 {
    if WelsCheckNumRefSetting(pLogCtx, pParam, false) != 0 {
        // num-ref is the honored setting but it conflicts with temporal and LTR
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    ENC_RETURN_SUCCESS
}

/// `WelsCheckRefFrameLimitationLevelIdcFirst` — au_set.cpp:144.
pub unsafe fn WelsCheckRefFrameLimitationLevelIdcFirst(
    pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
) -> i32 {
    if (*pParam).iNumRefFrame == AUTO_REF_PIC_COUNT
        || (*pParam).iMaxNumRefFrame == AUTO_REF_PIC_COUNT
    {
        // no need to do the checking
        return ENC_RETURN_SUCCESS;
    }

    WelsCheckNumRefSetting(pLogCtx, pParam, false);

    // number of reference frames according to level limitation
    for i in 0..(*pParam).iSpatialLayerNum as usize {
        let pSpatialLayer = (*pParam).sSpatialLayers[i];
        if pSpatialLayer.uiLevelIdc == LEVEL_UNKNOWN {
            continue;
        }

        let uiPicInMBs = (((pSpatialLayer.iVideoHeight + 15) >> 4)
            * ((pSpatialLayer.iVideoWidth + 15) >> 4)) as u32;
        let iRefFrame = (g_ksLevelLimits[pSpatialLayer.uiLevelIdc as usize - 1].uiMaxDPBMbs
            / uiPicInMBs) as i32;

        if iRefFrame < (*pParam).iMaxNumRefFrame {
            (*pParam).iMaxNumRefFrame = iRefFrame;
            if iRefFrame < (*pParam).iNumRefFrame {
                (*pParam).iNumRefFrame = iRefFrame;
            }
        } else {
            // level-idc first strategy: adjust max-ref up to what the level allows
            (*pParam).iMaxNumRefFrame = iRefFrame;
        }
    }

    ENC_RETURN_SUCCESS
}
