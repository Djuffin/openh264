//! Port of `codec/encoder/core/src/au_set.cpp` — access-unit / parameter-set
//! construction, the parameter-set writers, and the reference-frame limitation
//! checks.
//!
//! Complete, with one documented deviation: `WelsWriteSpsSyntax` returns an error for
//! `uiPocType == 1` where C++ has `assert(0)` behind a `// TODO: implement`. The
//! encoder only ever sets POC type 2 (`WelsInitSps`), so the branch is unreachable in
//! practice.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

use crate::api::codec_api::ELevelIdc::LEVEL_UNKNOWN;
use crate::api::codec_api::ESampleAspectRatio::ASP_EXT_SAR;
use crate::api::codec_api::EProfileIdc::*;
use crate::api::codec_api::{ELevelIdc, SSpatialLayerConfig};
use crate::api::codec_api::EUsageType::*;
use crate::common::wels_common_defs::SBitStringAux;
use crate::decoder::nalu::g_ksLevelLimits;
use crate::decoder::parameter_sets::SLevelLimits;
use crate::encoder::encoder_context::SCropOffset;
use crate::encoder::param_svc::{
    SSpatialLayerInternal, SSubsetSps, SWelsPPS, SWelsSPS, SWelsSvcCodingParam, WELS_LOG2,
};
use crate::encoder::paraset_strategy::IWelsParametersetStrategy;
use crate::encoder::rc::{WELS_CLIP3, WELS_MAX};
use crate::encoder::vlc_encoder::{
    BsRbspTrailingBits, BsWriteBits, BsWriteOneBit, BsWriteSE, BsWriteUE,
};
use crate::encoder::wels_encoder_ext::{
    SLogContext, AUTO_REF_PIC_COUNT, ENC_RETURN_SUCCESS, ENC_RETURN_UNSUPPORTED_PARA,
    LONG_TERM_REF_NUM, LONG_TERM_REF_NUM_SCREEN, MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA,
    MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN, MIN_REF_PIC_COUNT, LEVEL_NUMBER,
};
use crate::encoder::param_svc::UNSPECIFIED_BIT_RATE;

/// `CpbBrNalFactor` — codec/common/inc/wels_common_defs.h:61.
/// Baseline, main and extended profiles.
pub const CpbBrNalFactor: i32 = 1200;

/// `WelsCheckLevelLimitation` — au_set.cpp:51 (file-static inline).
///
/// Returns 1 if `kpLevelLimit` can carry the picture described by `kpSps` at
/// `fFrameRate` and `iTargetBitRate`, 0 otherwise.
///
/// The arithmetic is `uint32_t` throughout, as in C++: `iMbWidth`/`iMbHeight` are
/// `int16_t` widened to `uint32_t` before multiplying, and the products are allowed to
/// wrap. `uiPicInMBs * fFrameRate` promotes to `float` and truncates back.
///
/// # Safety
/// `kpSps` and `kpLevelLimit` must reference initialised values.
pub unsafe fn WelsCheckLevelLimitation(
    kpSps: *const SWelsSPS,
    kpLevelLimit: *const SLevelLimits,
    fFrameRate: f32,
    iTargetBitRate: i32,
) -> i32 {
    let uiPicWidthInMBs = (*kpSps).iMbWidth as u32;
    let uiPicHeightInMBs = (*kpSps).iMbHeight as u32;
    let uiPicInMBs = uiPicWidthInMBs.wrapping_mul(uiPicHeightInMBs);
    let uiNumRefFrames = (*kpSps).iNumRefFrames as u32;

    if (*kpLevelLimit).uiMaxMBPS < (uiPicInMBs as f32 * fFrameRate) as u32 {
        return 0;
    }
    if (*kpLevelLimit).uiMaxFS < uiPicInMBs {
        return 0;
    }
    if ((*kpLevelLimit).uiMaxFS << 3) < uiPicWidthInMBs.wrapping_mul(uiPicWidthInMBs) {
        return 0;
    }
    if ((*kpLevelLimit).uiMaxFS << 3) < uiPicHeightInMBs.wrapping_mul(uiPicHeightInMBs) {
        return 0;
    }
    if (*kpLevelLimit).uiMaxDPBMbs < uiNumRefFrames.wrapping_mul(uiPicInMBs) {
        return 0;
    }
    if iTargetBitRate != UNSPECIFIED_BIT_RATE
        && ((*kpLevelLimit).uiMaxBR as i32).wrapping_mul(1200) < iTargetBitRate
    {
        // RC enabled, considering bitrate constraint
        return 0;
    }
    // add more checks here if needed in future

    1
}

/// `WelsGetLevelIdc` — au_set.cpp:187 (file-static inline).
///
/// Returns the first level in `g_ksLevelLimits` that can carry the picture, or
/// `LEVEL_5_1` if none can. Note the fallback is 5_1, not the table's last entry 5_2.
///
/// # Safety
/// `kpSps` must reference an initialised value.
pub unsafe fn WelsGetLevelIdc(kpSps: *const SWelsSPS, fFrameRate: f32, iTargetBitRate: i32) -> ELevelIdc {
    for iOrder in 0..LEVEL_NUMBER {
        if WelsCheckLevelLimitation(kpSps, &g_ksLevelLimits[iOrder], fFrameRate, iTargetBitRate) != 0
        {
            return level_idc_from_raw(g_ksLevelLimits[iOrder].uiLevelIdc);
        }
    }
    ELevelIdc::LEVEL_5_1 // final decision: select the biggest level
}

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

/// `WelsWriteVUI` — au_set.cpp:197.
///
/// # Safety
/// Both pointers must be non-null and `pBitStringAux` must have room for the VUI.
pub unsafe fn WelsWriteVUI(pSps: *mut SWelsSPS, pBitStringAux: *mut SBitStringAux) -> i32 {
    let pLocalBitStringAux = pBitStringAux;
    debug_assert!(!pSps.is_null() && !pBitStringAux.is_null());

    BsWriteOneBit(pLocalBitStringAux, (*pSps).bAspectRatioPresent as u32); // aspect_ratio_info_present_flag
    if (*pSps).bAspectRatioPresent {
        BsWriteBits(pLocalBitStringAux, 8, (*pSps).eAspectRatio as u32); // aspect_ratio_idc
        if (*pSps).eAspectRatio == ASP_EXT_SAR as i32 {
            BsWriteBits(pLocalBitStringAux, 16, (*pSps).sAspectRatioExtWidth as u32); // sar_width
            BsWriteBits(pLocalBitStringAux, 16, (*pSps).sAspectRatioExtHeight as u32); // sar_height
        }
    }
    BsWriteOneBit(pLocalBitStringAux, 0); // overscan_info_present_flag

    // See codec_app_def.h and parameter_sets.h for more info about members
    // bVideoSignalTypePresent through uiColorMatrix.
    BsWriteOneBit(pLocalBitStringAux, (*pSps).bVideoSignalTypePresent as u32); // video_signal_type_present_flag
    if (*pSps).bVideoSignalTypePresent {
        // write video signal type info to header
        BsWriteBits(pLocalBitStringAux, 3, (*pSps).uiVideoFormat as u32);
        BsWriteOneBit(pLocalBitStringAux, (*pSps).bFullRange as u32);
        BsWriteOneBit(pLocalBitStringAux, (*pSps).bColorDescriptionPresent as u32);

        if (*pSps).bColorDescriptionPresent {
            // write color description info to header
            BsWriteBits(pLocalBitStringAux, 8, (*pSps).uiColorPrimaries as u32);
            BsWriteBits(pLocalBitStringAux, 8, (*pSps).uiTransferCharacteristics as u32);
            BsWriteBits(pLocalBitStringAux, 8, (*pSps).uiColorMatrix as u32);
        }
    }

    BsWriteOneBit(pLocalBitStringAux, 0); // chroma_loc_info_present_flag
    BsWriteOneBit(pLocalBitStringAux, 0); // timing_info_present_flag
    BsWriteOneBit(pLocalBitStringAux, 0); // nal_hrd_parameters_present_flag
    BsWriteOneBit(pLocalBitStringAux, 0); // vcl_hrd_parameters_present_flag
    BsWriteOneBit(pLocalBitStringAux, 0); // pic_struct_present_flag
    BsWriteOneBit(pLocalBitStringAux, 1); // bitstream_restriction_flag

    BsWriteOneBit(pLocalBitStringAux, 1); // motion_vectors_over_pic_boundaries_flag
    BsWriteUE(pLocalBitStringAux, 0); // max_bytes_per_pic_denom
    BsWriteUE(pLocalBitStringAux, 0); // max_bits_per_mb_denom
    BsWriteUE(pLocalBitStringAux, 16); // log2_max_mv_length_horizontal
    BsWriteUE(pLocalBitStringAux, 16); // log2_max_mv_length_vertical

    BsWriteUE(pLocalBitStringAux, 0); // max_num_reorder_frames
    BsWriteUE(pLocalBitStringAux, (*pSps).iNumRefFrames as u32); // max_dec_frame_buffering

    0
}

/// `WelsWriteSpsSyntax` — au_set.cpp:264.
///
/// Writes the SPS RBSP body — no trailing bits; see [`WelsWriteSpsNal`].
///
/// **Deviation.** C++ has `assert (0)` under `uiPocType == 1` behind a
/// `// TODO: implement`. Here that returns 1 instead of aborting; `WelsInitSps` only
/// ever sets POC type 2, so the branch is unreachable.
///
/// # Safety
/// `pSps` and `pBitStringAux` must be non-null; `pSpsIdDelta` must point to an array
/// indexable by `pSps->uiSpsId`.
pub unsafe fn WelsWriteSpsSyntax(
    pSps: *mut SWelsSPS,
    pBitStringAux: *mut SBitStringAux,
    pSpsIdDelta: *mut i32,
    bBaseLayer: bool,
) -> i32 {
    let pLocalBitStringAux = pBitStringAux;

    debug_assert!(!pSps.is_null() && !pBitStringAux.is_null());

    BsWriteBits(pLocalBitStringAux, 8, (*pSps).uiProfileIdc as u32);

    BsWriteOneBit(pLocalBitStringAux, (*pSps).bConstraintSet0Flag as u32);
    BsWriteOneBit(pLocalBitStringAux, (*pSps).bConstraintSet1Flag as u32);
    BsWriteOneBit(pLocalBitStringAux, (*pSps).bConstraintSet2Flag as u32);
    BsWriteOneBit(pLocalBitStringAux, (*pSps).bConstraintSet3Flag as u32);
    if PRO_HIGH as u8 == (*pSps).uiProfileIdc
        || PRO_EXTENDED as u8 == (*pSps).uiProfileIdc
        || PRO_MAIN as u8 == (*pSps).uiProfileIdc
    {
        // constraint_set4_flag: with profile_idc 77/88/100, 1 means frame_mbs_only_flag is 1
        BsWriteOneBit(pLocalBitStringAux, 1);
        // constraint_set5_flag: with profile_idc 77/88/100, 1 means no B slices
        BsWriteOneBit(pLocalBitStringAux, 1);
        BsWriteBits(pLocalBitStringAux, 2, 0); // reserved_zero_2bits, equal to 0
    } else {
        BsWriteBits(pLocalBitStringAux, 4, 0); // reserved_zero_4bits, equal to 0
    }
    BsWriteBits(pLocalBitStringAux, 8, (*pSps).iLevelIdc as u32); // iLevelIdc
    // seq_parameter_set_id
    BsWriteUE(
        pLocalBitStringAux,
        (*pSps)
            .uiSpsId
            .wrapping_add(*pSpsIdDelta.add((*pSps).uiSpsId as usize) as u32),
    );

    if PRO_SCALABLE_BASELINE as u8 == (*pSps).uiProfileIdc
        || PRO_SCALABLE_HIGH as u8 == (*pSps).uiProfileIdc
        || PRO_HIGH as u8 == (*pSps).uiProfileIdc
        || PRO_HIGH10 as u8 == (*pSps).uiProfileIdc
        || PRO_HIGH422 as u8 == (*pSps).uiProfileIdc
        || PRO_HIGH444 as u8 == (*pSps).uiProfileIdc
        || PRO_CAVLC444 as u8 == (*pSps).uiProfileIdc
        || 44 == (*pSps).uiProfileIdc
    {
        BsWriteUE(pLocalBitStringAux, 1); // uiChromaFormatIdc, now should be 1
        BsWriteUE(pLocalBitStringAux, 0); // uiBitDepthLuma
        BsWriteUE(pLocalBitStringAux, 0); // uiBitDepthChroma
        BsWriteOneBit(pLocalBitStringAux, 0); // qpprime_y_zero_transform_bypass_flag
        BsWriteOneBit(pLocalBitStringAux, 0); // seq_scaling_matrix_present_flag
    }

    BsWriteUE(pLocalBitStringAux, (*pSps).uiLog2MaxFrameNum.wrapping_sub(4)); // log2_max_frame_num_minus4
    BsWriteUE(pLocalBitStringAux, (*pSps).uiPocType); // pic_order_cnt_type
    if (*pSps).uiPocType == 0 {
        BsWriteUE(pLocalBitStringAux, ((*pSps).iLog2MaxPocLsb - 4) as u32); // log2_max_pic_order_cnt_lsb_minus4
    } else if (*pSps).uiPocType == 1 {
        // C++: `assert (0)` under a "TODO: implement".
        return 1;
    } else {
        // no-op for uiPocType 2.
    }

    BsWriteUE(pLocalBitStringAux, (*pSps).iNumRefFrames as u32); // max_num_ref_frames
    BsWriteOneBit(pLocalBitStringAux, (*pSps).bGapsInFrameNumValueAllowedFlag as u32); // gaps_in_frame_num_value_allowed_flag
    BsWriteUE(pLocalBitStringAux, ((*pSps).iMbWidth as i32 - 1) as u32); // pic_width_in_mbs_minus1
    BsWriteUE(pLocalBitStringAux, ((*pSps).iMbHeight as i32 - 1) as u32); // pic_height_in_map_units_minus1
    BsWriteOneBit(pLocalBitStringAux, 1); // bFrameMbsOnlyFlag, hardcoded true in C++

    let d8x8: u8 = if (*pSps).iLevelIdc >= 30 { 1 } else { 0 };
    BsWriteOneBit(pLocalBitStringAux, d8x8 as u32); // direct_8x8_inference_flag

    BsWriteOneBit(pLocalBitStringAux, (*pSps).bFrameCroppingFlag as u32); // bFrameCroppingFlag
    if (*pSps).bFrameCroppingFlag {
        BsWriteUE(pLocalBitStringAux, (*pSps).sFrameCrop.iCropLeft as u32); // frame_crop_left_offset
        BsWriteUE(pLocalBitStringAux, (*pSps).sFrameCrop.iCropRight as u32); // frame_crop_right_offset
        BsWriteUE(pLocalBitStringAux, (*pSps).sFrameCrop.iCropTop as u32); // frame_crop_top_offset
        BsWriteUE(pLocalBitStringAux, (*pSps).sFrameCrop.iCropBottom as u32); // frame_crop_bottom_offset
    }
    if bBaseLayer {
        BsWriteOneBit(pLocalBitStringAux, 1); // vui_parameters_present_flag
        WelsWriteVUI(pSps, pBitStringAux);
    } else {
        BsWriteOneBit(pLocalBitStringAux, 0);
    }
    0
}

/// `WelsWriteSpsNal` — au_set.cpp:336.
///
/// # Safety
/// See [`WelsWriteSpsSyntax`].
pub unsafe fn WelsWriteSpsNal(
    pSps: *mut SWelsSPS,
    pBitStringAux: *mut SBitStringAux,
    pSpsIdDelta: *mut i32,
) -> i32 {
    WelsWriteSpsSyntax(pSps, pBitStringAux, pSpsIdDelta, true);

    BsRbspTrailingBits(pBitStringAux);

    0
}

/// `WelsWriteSubsetSpsSyntax` — au_set.cpp:358.
///
/// # Safety
/// See [`WelsWriteSpsSyntax`].
pub unsafe fn WelsWriteSubsetSpsSyntax(
    pSubsetSps: *mut SSubsetSps,
    pBitStringAux: *mut SBitStringAux,
    pSpsIdDelta: *mut i32,
) -> i32 {
    let pSps = &mut (*pSubsetSps).pSps as *mut SWelsSPS;

    WelsWriteSpsSyntax(pSps, pBitStringAux, pSpsIdDelta, false);

    if (*pSps).uiProfileIdc == PRO_SCALABLE_BASELINE as u8
        || (*pSps).uiProfileIdc == PRO_SCALABLE_HIGH as u8
    {
        let pSubsetSpsExt = &mut (*pSubsetSps).sSpsSvcExt;

        BsWriteOneBit(pBitStringAux, 1); // bInterLayerDeblockingFilterCtrlPresentFlag
        BsWriteBits(pBitStringAux, 2, pSubsetSpsExt.iExtendedSpatialScalability as u32);
        BsWriteOneBit(pBitStringAux, 0); // uiChromaPhaseXPlus1Flag
        BsWriteBits(pBitStringAux, 2, 1); // uiChromaPhaseYPlus1
        if pSubsetSpsExt.iExtendedSpatialScalability == 1 {
            BsWriteOneBit(pBitStringAux, 0); // uiSeqRefLayerChromaPhaseXPlus1Flag
            BsWriteBits(pBitStringAux, 2, 1); // uiSeqRefLayerChromaPhaseYPlus1
            BsWriteSE(pBitStringAux, 0); // sSeqScaledRefLayer.left_offset
            BsWriteSE(pBitStringAux, 0); // sSeqScaledRefLayer.top_offset
            BsWriteSE(pBitStringAux, 0); // sSeqScaledRefLayer.right_offset
            BsWriteSE(pBitStringAux, 0); // sSeqScaledRefLayer.bottom_offset
        }
        BsWriteOneBit(pBitStringAux, pSubsetSpsExt.bSeqTcoeffLevelPredFlag as u32);
        if pSubsetSpsExt.bSeqTcoeffLevelPredFlag {
            BsWriteOneBit(pBitStringAux, pSubsetSpsExt.bAdaptiveTcoeffLevelPredFlag as u32);
        }
        BsWriteOneBit(pBitStringAux, pSubsetSpsExt.bSliceHeaderRestrictionFlag as u32);

        BsWriteOneBit(pBitStringAux, 0); // bSvcVuiParamPresentFlag
    }
    BsWriteOneBit(pBitStringAux, 0); // bAdditionalExtension2Flag

    BsRbspTrailingBits(pBitStringAux);

    0
}

/// `WelsWritePpsSyntax` — au_set.cpp:406.
///
/// `DISABLE_FMO_FEATURE` is defined unconditionally at `as264_common.h:53`, so the
/// slice-group branch at au_set.cpp:418-454 is not compiled and
/// `num_slice_groups_minus1` is the literal 0 at au_set.cpp:417.
///
/// # Safety
/// `pPps` and `pBitStringAux` must be non-null; `pParametersetStrategy` must be a live
/// strategy from `paraset_strategy::CreateParametersetStrategy`.
pub unsafe fn WelsWritePpsSyntax(
    pPps: *mut SWelsPPS,
    pBitStringAux: *mut SBitStringAux,
    pParametersetStrategy: *mut IWelsParametersetStrategy,
) -> i32 {
    let pLocalBitStringAux = pBitStringAux;

    BsWriteUE(
        pLocalBitStringAux,
        (*pPps).iPpsId.wrapping_add(IWelsParametersetStrategy::GetPpsIdOffset(
            pParametersetStrategy,
            (*pPps).iPpsId as i32,
        ) as u32),
    );
    BsWriteUE(
        pLocalBitStringAux,
        (*pPps).iSpsId.wrapping_add(IWelsParametersetStrategy::GetSpsIdOffset(
            pParametersetStrategy,
            (*pPps).iPpsId as i32,
            (*pPps).iSpsId as i32,
        ) as u32),
    );

    BsWriteOneBit(pLocalBitStringAux, (*pPps).bEntropyCodingModeFlag as u32);
    BsWriteOneBit(pLocalBitStringAux, 0); // bPicOrderPresentFlag

    // DISABLE_FMO_FEATURE branch, au_set.cpp:417.
    BsWriteUE(pLocalBitStringAux, 0); // uiNumSliceGroups - 1

    BsWriteUE(pLocalBitStringAux, 0); // uiNumRefIdxL0Active - 1
    BsWriteUE(pLocalBitStringAux, 0); // uiNumRefIdxL1Active - 1

    BsWriteOneBit(pLocalBitStringAux, 0); // bWeightedPredFlag
    BsWriteBits(pLocalBitStringAux, 2, 0); // uiWeightedBiPredIdc

    BsWriteSE(pLocalBitStringAux, (*pPps).iPicInitQp as i32 - 26);
    BsWriteSE(pLocalBitStringAux, (*pPps).iPicInitQs as i32 - 26);

    BsWriteSE(pLocalBitStringAux, (*pPps).uiChromaQpIndexOffset as i32);
    BsWriteOneBit(
        pLocalBitStringAux,
        (*pPps).bDeblockingFilterControlPresentFlag as u32,
    );
    BsWriteOneBit(pLocalBitStringAux, 0); // bConstainedIntraPredFlag
    BsWriteOneBit(pLocalBitStringAux, 0); // bRedundantPicCntPresentFlag

    BsRbspTrailingBits(pLocalBitStringAux);

    0
}

/// `WelsGetPaddingOffset` — au_set.cpp:476 (file-static inline).
///
/// Returns true when the coded size exceeds the actual size, i.e. when the SPS needs
/// `frame_cropping_flag`. Note that C++ makes the *actual* size even in place before
/// computing both the offsets and the return value.
pub fn WelsGetPaddingOffset(
    mut iActualWidth: i32,
    mut iActualHeight: i32,
    iWidth: i32,
    iHeight: i32,
    pOffset: &mut SCropOffset,
) -> bool {
    if (iWidth < iActualWidth) || (iHeight < iActualHeight) {
        return false;
    }

    // make actual size even
    iActualWidth -= iActualWidth & 1;
    iActualHeight -= iActualHeight & 1;

    pOffset.iCropLeft = 0;
    pOffset.iCropRight = ((iWidth - iActualWidth) / 2) as i16;
    pOffset.iCropTop = 0;
    pOffset.iCropBottom = ((iHeight - iActualHeight) / 2) as i16;

    (iWidth > iActualWidth) || (iHeight > iActualHeight)
}

/// `WelsInitSps` — au_set.cpp:492.
///
/// `kuiIntraPeriod` and `bEnableRc` are accepted and unused, exactly as in C++.
///
/// # Safety
/// All three pointers must be non-null and point to writable values.
pub unsafe fn WelsInitSps(
    pSps: *mut SWelsSPS,
    pLayerParam: *mut SSpatialLayerConfig,
    pLayerParamInternal: *mut SSpatialLayerInternal,
    _kuiIntraPeriod: u32,
    kiNumRefFrame: i32,
    kuiSpsId: u32,
    kbEnableFrameCropping: bool,
    _bEnableRc: bool,
    kiDlayerCount: i32,
    bSVCBaselayer: bool,
) -> i32 {
    // C++ `memset (pSps, 0, sizeof (SWelsSPS))`. Deliberately not `SWelsSPS::default()`,
    // which seeds uiProfileIdc = PRO_BASELINE and the VUI *_UNDEF values rather than 0.
    std::ptr::write_bytes(pSps, 0, 1);
    (*pSps).uiSpsId = kuiSpsId;
    (*pSps).iMbWidth = (((*pLayerParam).iVideoWidth + 15) >> 4) as i16;
    (*pSps).iMbHeight = (((*pLayerParam).iVideoHeight + 15) >> 4) as i16;

    // max value of both iFrameNum and POC are 2^16-1; in this encoder iPOC = 2*iFrameNum,
    // so max of iFrameNum should be 2^15-1.
    (*pSps).uiLog2MaxFrameNum = 15; // 16;
    (*pSps).uiPocType = 2;
    (*pSps).iLog2MaxPocLsb = 1 + (*pSps).uiLog2MaxFrameNum as i32;

    (*pSps).iNumRefFrames = kiNumRefFrame as i16; /* min pRef size when fifo pRef operation */

    if kbEnableFrameCropping {
        (*pSps).bFrameCroppingFlag = WelsGetPaddingOffset(
            (*pLayerParamInternal).iActualWidth,
            (*pLayerParamInternal).iActualHeight,
            (*pLayerParam).iVideoWidth,
            (*pLayerParam).iVideoHeight,
            &mut (*pSps).sFrameCrop,
        );
    } else {
        (*pSps).bFrameCroppingFlag = false;
    }
    (*pSps).uiProfileIdc = if (*pLayerParam).uiProfileIdc as u8 != 0 {
        (*pLayerParam).uiProfileIdc as u8
    } else {
        PRO_BASELINE as u8
    };
    if (*pLayerParam).uiProfileIdc == PRO_BASELINE {
        (*pSps).bConstraintSet0Flag = true;
    }
    if ((*pLayerParam).uiProfileIdc as i32) <= PRO_MAIN as i32 {
        (*pSps).bConstraintSet1Flag = true;
    }
    if (kiDlayerCount > 1) && bSVCBaselayer {
        (*pSps).bConstraintSet2Flag = true;
    }

    let mut uiLevel = WelsGetLevelIdc(
        pSps,
        (*pLayerParamInternal).fOutputFrameRate,
        (*pLayerParam).iSpatialBitrate,
    );
    // update level
    // For Scalable Baseline/High/High Intra, level_idc 9 means level 1b.
    // For Baseline/Constrained Baseline/Main/Extended, level_idc 11 with
    // constraint_set3_flag 1 means level 1b.
    if uiLevel == ELevelIdc::LEVEL_1_B
        && ((*pSps).uiProfileIdc == PRO_BASELINE as u8
            || (*pSps).uiProfileIdc == PRO_MAIN as u8
            || (*pSps).uiProfileIdc == PRO_EXTENDED as u8)
    {
        uiLevel = ELevelIdc::LEVEL_1_1;
        (*pSps).bConstraintSet3Flag = true;
    }
    if ((*pLayerParam).uiLevelIdc == LEVEL_UNKNOWN)
        || (((*pLayerParam).uiLevelIdc as i32) < uiLevel as i32)
    {
        (*pLayerParam).uiLevelIdc = uiLevel;
    }
    (*pSps).iLevelIdc = (*pLayerParam).uiLevelIdc as u8;

    // bGapsInFrameNumValueAllowedFlag is false when spatial and temporal layer counts
    // are both 1 and ltr is 0.
    if (kiDlayerCount == 1) && ((*pSps).iNumRefFrames == 1) {
        (*pSps).bGapsInFrameNumValueAllowedFlag = false;
    } else {
        (*pSps).bGapsInFrameNumValueAllowedFlag = true;
    }

    (*pSps).bVuiParamPresentFlag = true;

    (*pSps).bAspectRatioPresent = (*pLayerParam).bAspectRatioPresent;
    (*pSps).eAspectRatio = (*pLayerParam).eAspectRatio as i32;
    (*pSps).sAspectRatioExtWidth = (*pLayerParam).sAspectRatioExtWidth;
    (*pSps).sAspectRatioExtHeight = (*pLayerParam).sAspectRatioExtHeight;

    // See codec_app_def.h and parameter_sets.h for more info about members
    // bVideoSignalTypePresent through uiColorMatrix.
    (*pSps).bVideoSignalTypePresent = (*pLayerParam).bVideoSignalTypePresent;
    (*pSps).uiVideoFormat = (*pLayerParam).uiVideoFormat;
    (*pSps).bFullRange = (*pLayerParam).bFullRange;
    (*pSps).bColorDescriptionPresent = (*pLayerParam).bColorDescriptionPresent;
    (*pSps).uiColorPrimaries = (*pLayerParam).uiColorPrimaries;
    (*pSps).uiTransferCharacteristics = (*pLayerParam).uiTransferCharacteristics;
    (*pSps).uiColorMatrix = (*pLayerParam).uiColorMatrix;

    0
}

/// `WelsInitSubsetSps` — au_set.cpp:566.
///
/// # Safety
/// All three pointers must be non-null and point to writable values.
#[allow(clippy::too_many_arguments)]
pub unsafe fn WelsInitSubsetSps(
    pSubsetSps: *mut SSubsetSps,
    pLayerParam: *mut SSpatialLayerConfig,
    pLayerParamInternal: *mut SSpatialLayerInternal,
    kuiIntraPeriod: u32,
    kiNumRefFrame: i32,
    kuiSpsId: u32,
    kbEnableFrameCropping: bool,
    bEnableRc: bool,
    kiDlayerCount: i32,
) -> i32 {
    let pSps = &mut (*pSubsetSps).pSps as *mut SWelsSPS;

    std::ptr::write_bytes(pSubsetSps, 0, 1);

    WelsInitSps(
        pSps,
        pLayerParam,
        pLayerParamInternal,
        kuiIntraPeriod,
        kiNumRefFrame,
        kuiSpsId,
        kbEnableFrameCropping,
        bEnableRc,
        kiDlayerCount,
        false,
    );

    // Note: unlike WelsInitSps this takes uiProfileIdc verbatim, with no PRO_BASELINE
    // fallback for 0.
    (*pSps).uiProfileIdc = (*pLayerParam).uiProfileIdc as u8;

    (*pSubsetSps).sSpsSvcExt.iExtendedSpatialScalability = 0; /* ESS is 0 in default */
    (*pSubsetSps).sSpsSvcExt.bAdaptiveTcoeffLevelPredFlag = false;
    (*pSubsetSps).sSpsSvcExt.bSeqTcoeffLevelPredFlag = false;
    (*pSubsetSps).sSpsSvcExt.bSliceHeaderRestrictionFlag = true;

    0
}

/// `WelsInitPps` — au_set.cpp:588.
///
/// The `#if !defined(DISABLE_FMO_FEATURE)` slice-group block at au_set.cpp:614-636 is
/// not compiled — see `as264_common.h:53`.
///
/// # Safety
/// `pPps` must be writable; at least one of `pSps`/`pSubsetSps` must be non-null, and
/// the one selected by `kbUsingSubsetSps` must be.
pub unsafe fn WelsInitPps(
    pPps: *mut SWelsPPS,
    pSps: *mut SWelsSPS,
    pSubsetSps: *mut SSubsetSps,
    kuiPpsId: u32,
    kbDeblockingFilterPresentFlag: bool,
    kbUsingSubsetSps: bool,
    kbEntropyCodingModeFlag: bool,
) -> i32 {
    let pUsedSps: *mut SWelsSPS;
    if pPps.is_null() || (pSps.is_null() && pSubsetSps.is_null()) {
        return 1;
    }
    if !kbUsingSubsetSps {
        debug_assert!(!pSps.is_null());
        if pSps.is_null() {
            return 1;
        }
        pUsedSps = pSps;
    } else {
        debug_assert!(!pSubsetSps.is_null());
        if pSubsetSps.is_null() {
            return 1;
        }
        pUsedSps = &mut (*pSubsetSps).pSps;
    }

    /* fill picture parameter set syntax */
    (*pPps).iPpsId = kuiPpsId;
    (*pPps).iSpsId = (*pUsedSps).uiSpsId;
    (*pPps).bEntropyCodingModeFlag = kbEntropyCodingModeFlag;

    (*pPps).iPicInitQp = 26;
    (*pPps).iPicInitQs = 26;

    (*pPps).uiChromaQpIndexOffset = 0;
    (*pPps).bDeblockingFilterControlPresentFlag = kbDeblockingFilterPresentFlag;

    0
}
