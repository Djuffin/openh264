#![forbid(unsafe_code)]
//! Port of `codec/encoder/core/src/au_set.cpp` — access-unit / parameter-set
//! construction, the parameter-set writers, and the reference-frame limitation
//! checks.
//!
//! **Sealed at S11.25**, and the last thing holding it open was documentation.
//! S11.18 gave `WelsInitSps`/`WelsInitSubsetSps` reference parameters (F187's
//! refusal had expired), which left the file's four remaining allows covering
//! test blocks whose comment still read *"the callee is still `unsafe fn`"*.
//! `unused_unsafe` had been reporting all four ever since; the warning was
//! invisible under `lib.rs`'s crate-wide `allow` (E2's target). `forbid` here
//! is what makes that class of drift a compile error rather than a warning
//! nobody reads.
//!
//! Complete, with one documented deviation: `WelsWriteSpsSyntax` returns an error for
//! `uiPocType == 1` where C++ has `assert(0)` behind a `// TODO: implement`. The
//! encoder only ever sets POC type 2 (`WelsInitSps`), so the branch is unreachable in
//! practice.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

#![deny(unsafe_code)]

use crate::api::codec_api::ELevelIdc::LEVEL_UNKNOWN;
use crate::api::codec_api::ESampleAspectRatio::ASP_EXT_SAR;
use crate::api::codec_api::EProfileIdc::*;
use crate::api::codec_api::{ELevelIdc, SSpatialLayerConfig};
use crate::api::codec_api::EUsageType::*;
use crate::safe::bits::BsWriter;
use crate::decoder::nalu::g_ksLevelLimits;
use crate::decoder::parameter_sets::SLevelLimits;
use crate::encoder::encoder_context::SCropOffset;
use crate::encoder::param_svc::{
    SSpatialLayerInternal, SSubsetSps, SWelsPPS, SWelsSPS, SWelsSvcCodingParam, WELS_LOG2,
};
use crate::encoder::paraset_strategy::CWelsParametersetIdStrategyObj;
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
// T9.X2 (F181): the two reference-limitation helpers below log through the context
// they are handed, which is what makes that parameter live rather than dead.
use crate::common::wels_trace::{WELS_LOG_ERROR, WELS_LOG_INFO, WELS_LOG_WARNING, WelsLog};

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
/// **Safe since T6.G3**: both parameters were `*const` to single objects with one
/// caller each, which is R1's shape exactly.
pub fn WelsCheckLevelLimitation(
    kpSps: &SWelsSPS,
    kpLevelLimit: &SLevelLimits,
    fFrameRate: f32,
    iTargetBitRate: i32,
) -> i32 {
    let uiPicWidthInMBs = kpSps.iMbWidth as u32;
    let uiPicHeightInMBs = kpSps.iMbHeight as u32;
    let uiPicInMBs = uiPicWidthInMBs.wrapping_mul(uiPicHeightInMBs);
    let uiNumRefFrames = kpSps.iNumRefFrames as u32;

    if kpLevelLimit.uiMaxMBPS < (uiPicInMBs as f32 * fFrameRate) as u32 {
        return 0;
    }
    if kpLevelLimit.uiMaxFS < uiPicInMBs {
        return 0;
    }
    if (kpLevelLimit.uiMaxFS << 3) < uiPicWidthInMBs.wrapping_mul(uiPicWidthInMBs) {
        return 0;
    }
    if (kpLevelLimit.uiMaxFS << 3) < uiPicHeightInMBs.wrapping_mul(uiPicHeightInMBs) {
        return 0;
    }
    if kpLevelLimit.uiMaxDPBMbs < uiNumRefFrames.wrapping_mul(uiPicInMBs) {
        return 0;
    }
    if iTargetBitRate != UNSPECIFIED_BIT_RATE
        && (kpLevelLimit.uiMaxBR as i32).wrapping_mul(1200) < iTargetBitRate
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
pub fn WelsGetLevelIdc(kpSps: &SWelsSPS, fFrameRate: f32, iTargetBitRate: i32) -> ELevelIdc {
    for iOrder in 0..LEVEL_NUMBER {
        if WelsCheckLevelLimitation(kpSps, &g_ksLevelLimits[iOrder], fFrameRate, iTargetBitRate)
            != 0
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
///
/// **T9.X2 — the log context was not dead, the six `WelsLog` calls were missing.**
/// The parameter stood here as `_pLogCtx` and X2's brief read that underscore as
/// evidence of a dead parameter, to be deleted under S54. The reference disagrees:
/// `encoder_ext.cpp:74-127` logs **six** times through it — one ERROR on an invalid
/// bitrate, two INFO and one WARNING while it rewrites `iMaxSpatialBitrate` against
/// the level table, and an INFO/ERROR pair on the max-vs-target comparison. Every
/// one of those describes a parameter this function silently *changes*, so dropping
/// them cost the caller the only account of why its settings moved. They are
/// restored below and the parameter is live.
///
/// This is F177's rule reaching a parameter rather than a field: an unused name is
/// evidence about *this* tree only, and the reference is where you find out whether
/// it was ever supposed to be used. See F181.
pub fn WelsBitRateVerification(
    pLogCtx: SLogContext,
    pLayerParam: &mut SSpatialLayerConfig,
    iLayerId: i32,
) -> i32 {
    if pLayerParam.iSpatialBitrate <= 0
        || (pLayerParam.iSpatialBitrate as f32) < pLayerParam.fFrameRate
    {
        WelsLog(
            pLogCtx,
            WELS_LOG_ERROR,
            &format!(
                "Invalid bitrate settings in layer {}, bitrate= {} at FrameRate({:.6})",
                iLayerId,
                pLayerParam.iSpatialBitrate,
                pLayerParam.fFrameRate
            ),
        );
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // deal with LEVEL_MAX_BR and MAX_BR setting
    let mut iCurLevelIdx = 0usize;
    while level_idc_from_raw(g_ksLevelLimits[iCurLevelIdx].uiLevelIdc) != ELevelIdc::LEVEL_5_2
        && level_idc_from_raw(g_ksLevelLimits[iCurLevelIdx].uiLevelIdc)
            != pLayerParam.uiLevelIdc
    {
        iCurLevelIdx += 1;
    }
    let iLevelMaxBitrate = g_ksLevelLimits[iCurLevelIdx].uiMaxBR as i32 * CpbBrNalFactor;
    let iLevel52MaxBitrate = g_ksLevelLimits[LEVEL_NUMBER - 1].uiMaxBR as i32 * CpbBrNalFactor;

    if UNSPECIFIED_BIT_RATE != iLevelMaxBitrate {
        if pLayerParam.iMaxSpatialBitrate == UNSPECIFIED_BIT_RATE
            || pLayerParam.iMaxSpatialBitrate > iLevel52MaxBitrate
        {
            pLayerParam.iMaxSpatialBitrate = iLevelMaxBitrate;
            WelsLog(
                pLogCtx,
                WELS_LOG_INFO,
                &format!(
                    "Current MaxSpatialBitrate is invalid (UNSPECIFIED_BIT_RATE or larger than LEVEL5_2) but level setting is valid, set iMaxSpatialBitrate to {} from level ({})",
                    pLayerParam.iMaxSpatialBitrate,
                    pLayerParam.uiLevelIdc as i32
                ),
            );
        } else if pLayerParam.iMaxSpatialBitrate > iLevelMaxBitrate {
            // The reference reads the level id into `iCurLevel` *before* the adjust
            // and prints both; `WelsAdjustLevel` is what moves it.
            let iCurLevel = pLayerParam.uiLevelIdc;
            WelsAdjustLevel(&mut *pLayerParam, iCurLevelIdx);
            WelsLog(
                pLogCtx,
                WELS_LOG_INFO,
                &format!(
                    "LevelIdc is changed from ({}) to ({}) according to the iMaxSpatialBitrate({})",
                    iCurLevel as i32,
                    pLayerParam.uiLevelIdc as i32,
                    pLayerParam.iMaxSpatialBitrate
                ),
            );
        }
    } else if pLayerParam.iMaxSpatialBitrate != UNSPECIFIED_BIT_RATE
        && pLayerParam.iMaxSpatialBitrate > iLevel52MaxBitrate
    {
        // no level limitation, only guard against an unreasonably large max
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "No LevelIdc setting and iMaxSpatialBitrate ({}) is considered too big to be valid, changed to UNSPECIFIED_BIT_RATE",
                pLayerParam.iMaxSpatialBitrate
            ),
        );
        pLayerParam.iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
    }

    // deal with iSpatialBitrate and iMaxSpatialBitrate setting
    //
    // The reference splits this into an `==` arm that only logs and a `<` arm that
    // logs and fails; the port had collapsed them to the failing comparison alone,
    // which is the same program and one message short of the same encoder.
    if pLayerParam.iMaxSpatialBitrate != UNSPECIFIED_BIT_RATE {
        if pLayerParam.iMaxSpatialBitrate == pLayerParam.iSpatialBitrate {
            WelsLog(
                pLogCtx,
                WELS_LOG_INFO,
                &format!(
                    "Setting MaxSpatialBitrate ({}) the same at SpatialBitrate ({}) will make the actual bit rate lower than SpatialBitrate",
                    pLayerParam.iMaxSpatialBitrate,
                    pLayerParam.iSpatialBitrate
                ),
            );
        } else if pLayerParam.iMaxSpatialBitrate < pLayerParam.iSpatialBitrate {
            WelsLog(
                pLogCtx,
                WELS_LOG_ERROR,
                &format!(
                    "MaxSpatialBitrate ({}) should be larger than SpatialBitrate ({}), considering it as error setting",
                    pLayerParam.iMaxSpatialBitrate,
                    pLayerParam.iSpatialBitrate
                ),
            );
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
    }
    ENC_RETURN_SUCCESS
}

/// `WelsCheckNumRefSetting` — au_set.cpp:88 (file-static in C++).
///
/// Reconciles `iLTRRefNum` / `iNumRefFrame` / `iMaxNumRefFrame` against the GOP
/// size and LTR settings. With `bStrictCheck` an under-sized `iNumRefFrame` is an
/// error; otherwise it is corrected in place.
///
/// **T9.X2 — two `WelsLog` calls restored, and the parameter with them** (see
/// [`WelsBitRateVerification`] for the general point). `au_set.cpp:88-133` warns
/// when it resets `iLTRRefNum` and again when it resets `iNumRefFrame`; both
/// describe a silent rewrite of a caller's setting, and the second fires on the
/// strict path too — the reference logs *before* it decides whether to fail.
pub fn WelsCheckNumRefSetting(
    pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
    bStrictCheck: bool,
) -> i32 {
    // validate LTR num
    let iCurrentSupportedLtrNum = if pParam.iUsageType == CAMERA_VIDEO_REAL_TIME {
        LONG_TERM_REF_NUM
    } else {
        LONG_TERM_REF_NUM_SCREEN
    };
    if pParam.bEnableLongTermReference && iCurrentSupportedLtrNum != pParam.iLTRRefNum {
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "iLTRRefNum({}) does not equal to currently supported {}, will be reset",
                pParam.iLTRRefNum,
                iCurrentSupportedLtrNum
            ),
        );
        pParam.iLTRRefNum = iCurrentSupportedLtrNum;
    } else if !pParam.bEnableLongTermReference {
        pParam.iLTRRefNum = 0;
    }

    // NB: the C++ carries a TODO saying the reasonable value is
    // WELS_MAX(1, WELS_LOG2(uiGopSize)) unconditionally, but changing it needs
    // reference-list updating changed too. Kept as-is.
    let iCurrentStrNum = if pParam.iUsageType == SCREEN_CONTENT_REAL_TIME
        && pParam.bEnableLongTermReference
    {
        WELS_MAX(1, WELS_LOG2(pParam.uiGopSize))
    } else {
        WELS_MAX(1, (pParam.uiGopSize >> 1) as i32)
    };
    let mut iNeededRefNum = if pParam.uiIntraPeriod != 1 {
        iCurrentStrNum + pParam.iLTRRefNum
    } else {
        0
    };

    iNeededRefNum = WELS_CLIP3(
        iNeededRefNum,
        MIN_REF_PIC_COUNT,
        if pParam.iUsageType == CAMERA_VIDEO_REAL_TIME {
            MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA
        } else {
            MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN
        },
    );

    // adjust default or invalid input so iNumRefFrame is valid for the next step
    if pParam.iNumRefFrame == AUTO_REF_PIC_COUNT {
        pParam.iNumRefFrame = iNeededRefNum;
    } else if pParam.iNumRefFrame < iNeededRefNum {
        // Logged before the strict-check return, as in the reference: a caller that
        // gets ENC_RETURN_UNSUPPORTED_PARA out of this still learns why.
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "iNumRefFrame({}) setting does not support the temporal and LTR setting, will be reset to {}",
                pParam.iNumRefFrame,
                iNeededRefNum
            ),
        );
        if bStrictCheck {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        pParam.iNumRefFrame = iNeededRefNum;
    }

    // if the setting is larger than needed, use the needed one and write the max
    // into the SPS, leaving memory sized for later expansion
    if pParam.iMaxNumRefFrame < pParam.iNumRefFrame {
        pParam.iMaxNumRefFrame = pParam.iNumRefFrame;
    }
    pParam.iNumRefFrame = iNeededRefNum;

    ENC_RETURN_SUCCESS
}

/// `WelsCheckRefFrameLimitationNumRefFirst` — au_set.cpp:135.
pub fn WelsCheckRefFrameLimitationNumRefFirst(
    pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
) -> i32 {
    if WelsCheckNumRefSetting(pLogCtx, pParam, false) != 0 {
        // num-ref is the honored setting but it conflicts with temporal and LTR
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    ENC_RETURN_SUCCESS
}

/// `WelsCheckRefFrameLimitationLevelIdcFirst` — au_set.cpp:144.
pub fn WelsCheckRefFrameLimitationLevelIdcFirst(
    pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
) -> i32 {
    if pParam.iNumRefFrame == AUTO_REF_PIC_COUNT
        || pParam.iMaxNumRefFrame == AUTO_REF_PIC_COUNT
    {
        // no need to do the checking
        return ENC_RETURN_SUCCESS;
    }

    WelsCheckNumRefSetting(pLogCtx, pParam, false);

    // number of reference frames according to level limitation
    for i in 0..pParam.iSpatialLayerNum as usize {
        let pSpatialLayer = pParam.sSpatialLayers[i];
        if pSpatialLayer.uiLevelIdc == LEVEL_UNKNOWN {
            continue;
        }

        let uiPicInMBs = (((pSpatialLayer.iVideoHeight + 15) >> 4)
            * ((pSpatialLayer.iVideoWidth + 15) >> 4)) as u32;
        let iRefFrame = (g_ksLevelLimits[pSpatialLayer.uiLevelIdc as usize - 1].uiMaxDPBMbs
            / uiPicInMBs) as i32;

        if iRefFrame < pParam.iMaxNumRefFrame {
            pParam.iMaxNumRefFrame = iRefFrame;
            if iRefFrame < pParam.iNumRefFrame {
                pParam.iNumRefFrame = iRefFrame;
            }
        } else {
            // level-idc first strategy: adjust max-ref up to what the level allows
            pParam.iMaxNumRefFrame = iRefFrame;
        }
    }

    ENC_RETURN_SUCCESS
}

/// `WelsWriteVUI` — au_set.cpp:197.
///
/// **T9.X2 — `pBsWriter` is a `&mut BsWriter` in all five writers here.** It was
/// raw with a null check and an immediate `&mut *` at the top of every body, and
/// every production call site already formed the reference and let it coerce:
/// `&mut (*pOut).sBsWrite`, beside `&mut (*pOut).sBsBuffer[..]` as a separate
/// argument. Two disjoint fields of one `SWelsEncoderOutput`, which is not an
/// aliasing question at all — the raw was a vestige of the translation, and the
/// null check was unreachable.
///
/// `WelsWriteSVCPrefixNal` (`nal_encap.rs`) keeps its raw and is NOT part of this:
/// its multi-threaded caller passes `addr_of_mut!((*pSliceBs).sBsWrite)` over
/// fork-shared slice state, deliberately, and a `&mut` there is the seam's
/// question rather than this one's.
///
/// # Safety
/// `pBsWriter` must have room for the VUI.
pub fn WelsWriteVUI(
    buf: &mut [u8],
    pSps: &SWelsSPS,
    pBsWriter: &mut BsWriter,
) -> i32 {

    BsWriteOneBit(buf, pBsWriter, pSps.bAspectRatioPresent as u32); // aspect_ratio_info_present_flag
    if pSps.bAspectRatioPresent {
        BsWriteBits(buf, pBsWriter, 8, pSps.eAspectRatio as u32); // aspect_ratio_idc
        if pSps.eAspectRatio == ASP_EXT_SAR as i32 {
            BsWriteBits(buf, pBsWriter, 16, pSps.sAspectRatioExtWidth as u32); // sar_width
            BsWriteBits(buf, pBsWriter, 16, pSps.sAspectRatioExtHeight as u32); // sar_height
        }
    }
    BsWriteOneBit(buf, pBsWriter, 0); // overscan_info_present_flag

    // See codec_app_def.h and parameter_sets.h for more info about members
    // bVideoSignalTypePresent through uiColorMatrix.
    BsWriteOneBit(buf, pBsWriter, pSps.bVideoSignalTypePresent as u32); // video_signal_type_present_flag
    if pSps.bVideoSignalTypePresent {
        // write video signal type info to header
        BsWriteBits(buf, pBsWriter, 3, pSps.uiVideoFormat as u32);
        BsWriteOneBit(buf, pBsWriter, pSps.bFullRange as u32);
        BsWriteOneBit(buf, pBsWriter, pSps.bColorDescriptionPresent as u32);

        if pSps.bColorDescriptionPresent {
            // write color description info to header
            BsWriteBits(buf, pBsWriter, 8, pSps.uiColorPrimaries as u32);
            BsWriteBits(buf, pBsWriter, 8, pSps.uiTransferCharacteristics as u32);
            BsWriteBits(buf, pBsWriter, 8, pSps.uiColorMatrix as u32);
        }
    }

    BsWriteOneBit(buf, pBsWriter, 0); // chroma_loc_info_present_flag
    BsWriteOneBit(buf, pBsWriter, 0); // timing_info_present_flag
    BsWriteOneBit(buf, pBsWriter, 0); // nal_hrd_parameters_present_flag
    BsWriteOneBit(buf, pBsWriter, 0); // vcl_hrd_parameters_present_flag
    BsWriteOneBit(buf, pBsWriter, 0); // pic_struct_present_flag
    BsWriteOneBit(buf, pBsWriter, 1); // bitstream_restriction_flag

    BsWriteOneBit(buf, pBsWriter, 1); // motion_vectors_over_pic_boundaries_flag
    BsWriteUE(buf, pBsWriter, 0); // max_bytes_per_pic_denom
    BsWriteUE(buf, pBsWriter, 0); // max_bits_per_mb_denom
    BsWriteUE(buf, pBsWriter, 16); // log2_max_mv_length_horizontal
    BsWriteUE(buf, pBsWriter, 16); // log2_max_mv_length_vertical

    BsWriteUE(buf, pBsWriter, 0); // max_num_reorder_frames
    BsWriteUE(buf, pBsWriter, pSps.iNumRefFrames as u32); // max_dec_frame_buffering

    0
}

/// `WelsWriteSpsSyntax` — au_set.cpp:264.
///
/// Writes the SPS RBSP body — no trailing bits; see [`WelsWriteSpsNal`].
///
/// **Deviation.** C++ has `assert (0)` under `uiPocType == 1` behind a
/// `// TODO: implement`. Here that returns 1 instead of aborting; `WelsInitSps` only
/// ever sets POC type 2, so the branch is unreachable.
pub fn WelsWriteSpsSyntax(
    buf: &mut [u8],
    pSps: &SWelsSPS,
    pBsWriter: &mut BsWriter,
    pSpsIdDelta: &[i32],
    bBaseLayer: bool,
) -> i32 {


    BsWriteBits(buf, pBsWriter, 8, pSps.uiProfileIdc as u32);

    BsWriteOneBit(buf, pBsWriter, pSps.bConstraintSet0Flag as u32);
    BsWriteOneBit(buf, pBsWriter, pSps.bConstraintSet1Flag as u32);
    BsWriteOneBit(buf, pBsWriter, pSps.bConstraintSet2Flag as u32);
    BsWriteOneBit(buf, pBsWriter, pSps.bConstraintSet3Flag as u32);
    if PRO_HIGH as u8 == pSps.uiProfileIdc
        || PRO_EXTENDED as u8 == pSps.uiProfileIdc
        || PRO_MAIN as u8 == pSps.uiProfileIdc
    {
        // constraint_set4_flag: with profile_idc 77/88/100, 1 means frame_mbs_only_flag is 1
        BsWriteOneBit(buf, pBsWriter, 1);
        // constraint_set5_flag: with profile_idc 77/88/100, 1 means no B slices
        BsWriteOneBit(buf, pBsWriter, 1);
        BsWriteBits(buf, pBsWriter, 2, 0); // reserved_zero_2bits, equal to 0
    } else {
        BsWriteBits(buf, pBsWriter, 4, 0); // reserved_zero_4bits, equal to 0
    }
    BsWriteBits(buf, pBsWriter, 8, pSps.iLevelIdc as u32); // iLevelIdc
    // seq_parameter_set_id
    BsWriteUE(buf, pBsWriter,
        (*pSps)
            .uiSpsId
            .wrapping_add(pSpsIdDelta[pSps.uiSpsId as usize] as u32),
    );

    if PRO_SCALABLE_BASELINE as u8 == pSps.uiProfileIdc
        || PRO_SCALABLE_HIGH as u8 == pSps.uiProfileIdc
        || PRO_HIGH as u8 == pSps.uiProfileIdc
        || PRO_HIGH10 as u8 == pSps.uiProfileIdc
        || PRO_HIGH422 as u8 == pSps.uiProfileIdc
        || PRO_HIGH444 as u8 == pSps.uiProfileIdc
        || PRO_CAVLC444 as u8 == pSps.uiProfileIdc
        || 44 == pSps.uiProfileIdc
    {
        BsWriteUE(buf, pBsWriter, 1); // uiChromaFormatIdc, now should be 1
        BsWriteUE(buf, pBsWriter, 0); // uiBitDepthLuma
        BsWriteUE(buf, pBsWriter, 0); // uiBitDepthChroma
        BsWriteOneBit(buf, pBsWriter, 0); // qpprime_y_zero_transform_bypass_flag
        BsWriteOneBit(buf, pBsWriter, 0); // seq_scaling_matrix_present_flag
    }

    BsWriteUE(buf, pBsWriter, pSps.uiLog2MaxFrameNum.wrapping_sub(4)); // log2_max_frame_num_minus4
    BsWriteUE(buf, pBsWriter, pSps.uiPocType); // pic_order_cnt_type
    if pSps.uiPocType == 0 {
        BsWriteUE(buf, pBsWriter, (pSps.iLog2MaxPocLsb - 4) as u32); // log2_max_pic_order_cnt_lsb_minus4
    } else if pSps.uiPocType == 1 {
        // C++: `assert (0)` under a "TODO: implement".
        return 1;
    } else {
        // no-op for uiPocType 2.
    }

    BsWriteUE(buf, pBsWriter, pSps.iNumRefFrames as u32); // max_num_ref_frames
    BsWriteOneBit(buf, pBsWriter, pSps.bGapsInFrameNumValueAllowedFlag as u32); // gaps_in_frame_num_value_allowed_flag
    BsWriteUE(buf, pBsWriter, (pSps.iMbWidth as i32 - 1) as u32); // pic_width_in_mbs_minus1
    BsWriteUE(buf, pBsWriter, (pSps.iMbHeight as i32 - 1) as u32); // pic_height_in_map_units_minus1
    BsWriteOneBit(buf, pBsWriter, 1); // bFrameMbsOnlyFlag, hardcoded true in C++

    let d8x8: u8 = if pSps.iLevelIdc >= 30 { 1 } else { 0 };
    BsWriteOneBit(buf, pBsWriter, d8x8 as u32); // direct_8x8_inference_flag

    BsWriteOneBit(buf, pBsWriter, pSps.bFrameCroppingFlag as u32); // bFrameCroppingFlag
    if pSps.bFrameCroppingFlag {
        BsWriteUE(buf, pBsWriter, pSps.sFrameCrop.iCropLeft as u32); // frame_crop_left_offset
        BsWriteUE(buf, pBsWriter, pSps.sFrameCrop.iCropRight as u32); // frame_crop_right_offset
        BsWriteUE(buf, pBsWriter, pSps.sFrameCrop.iCropTop as u32); // frame_crop_top_offset
        BsWriteUE(buf, pBsWriter, pSps.sFrameCrop.iCropBottom as u32); // frame_crop_bottom_offset
    }
    if bBaseLayer {
        BsWriteOneBit(buf, pBsWriter, 1); // vui_parameters_present_flag
        WelsWriteVUI(buf, pSps, pBsWriter);
    } else {
        BsWriteOneBit(buf, pBsWriter, 0);
    }
    0
}

/// `WelsWriteSpsNal` — au_set.cpp:336.
pub fn WelsWriteSpsNal(
    buf: &mut [u8],
    pSps: &SWelsSPS,
    pBsWriter: &mut BsWriter,
    pSpsIdDelta: &[i32],
) -> i32 {
    WelsWriteSpsSyntax(buf, pSps, pBsWriter, pSpsIdDelta, true);

    BsRbspTrailingBits(buf, &mut *pBsWriter);

    0
}

/// `WelsWriteSubsetSpsSyntax` — au_set.cpp:358.
pub fn WelsWriteSubsetSpsSyntax(
    buf: &mut [u8],
    pSubsetSps: &SSubsetSps,
    pBsWriter: &mut BsWriter,
    pSpsIdDelta: &[i32],
) -> i32 {
    let pSps = &pSubsetSps.pSps;

    WelsWriteSpsSyntax(buf, pSps, pBsWriter, pSpsIdDelta, false);

    if pSps.uiProfileIdc == PRO_SCALABLE_BASELINE as u8
        || pSps.uiProfileIdc == PRO_SCALABLE_HIGH as u8
    {
        let pSubsetSpsExt = &pSubsetSps.sSpsSvcExt;

        BsWriteOneBit(buf, &mut *pBsWriter, 1); // bInterLayerDeblockingFilterCtrlPresentFlag
        BsWriteBits(buf, &mut *pBsWriter, 2, pSubsetSpsExt.iExtendedSpatialScalability as u32);
        BsWriteOneBit(buf, &mut *pBsWriter, 0); // uiChromaPhaseXPlus1Flag
        BsWriteBits(buf, &mut *pBsWriter, 2, 1); // uiChromaPhaseYPlus1
        if pSubsetSpsExt.iExtendedSpatialScalability == 1 {
            BsWriteOneBit(buf, &mut *pBsWriter, 0); // uiSeqRefLayerChromaPhaseXPlus1Flag
            BsWriteBits(buf, &mut *pBsWriter, 2, 1); // uiSeqRefLayerChromaPhaseYPlus1
            BsWriteSE(buf, &mut *pBsWriter, 0); // sSeqScaledRefLayer.left_offset
            BsWriteSE(buf, &mut *pBsWriter, 0); // sSeqScaledRefLayer.top_offset
            BsWriteSE(buf, &mut *pBsWriter, 0); // sSeqScaledRefLayer.right_offset
            BsWriteSE(buf, &mut *pBsWriter, 0); // sSeqScaledRefLayer.bottom_offset
        }
        BsWriteOneBit(buf, &mut *pBsWriter, pSubsetSpsExt.bSeqTcoeffLevelPredFlag as u32);
        if pSubsetSpsExt.bSeqTcoeffLevelPredFlag {
            BsWriteOneBit(buf, &mut *pBsWriter, pSubsetSpsExt.bAdaptiveTcoeffLevelPredFlag as u32);
        }
        BsWriteOneBit(buf, &mut *pBsWriter, pSubsetSpsExt.bSliceHeaderRestrictionFlag as u32);

        BsWriteOneBit(buf, &mut *pBsWriter, 0); // bSvcVuiParamPresentFlag
    }
    BsWriteOneBit(buf, &mut *pBsWriter, 0); // bAdditionalExtension2Flag

    BsRbspTrailingBits(buf, &mut *pBsWriter);

    0
}

/// `WelsWritePpsSyntax` — au_set.cpp:406.
///
/// `DISABLE_FMO_FEATURE` is defined unconditionally at `as264_common.h:53`, so the
/// slice-group branch at au_set.cpp:418-454 is not compiled and
/// `num_slice_groups_minus1` is the literal 0 at au_set.cpp:417.
///
/// # Safety
/// `pPps` and `pBsWriter` must be non-null. The strategy is borrowed by reference,
/// so unlike C++ there is no null case to consider here.
pub fn WelsWritePpsSyntax(
    buf: &mut [u8],
    pPps: &SWelsPPS,
    pBsWriter: &mut BsWriter,
    pParametersetStrategy: &CWelsParametersetIdStrategyObj,
) -> i32 {

    BsWriteUE(buf, pBsWriter,
        (*pPps)
            .iPpsId
            .wrapping_add(pParametersetStrategy.GetPpsIdOffset(pPps.iPpsId as i32) as u32),
    );
    BsWriteUE(buf, pBsWriter,
        pPps.iSpsId.wrapping_add(
            pParametersetStrategy.GetSpsIdOffset(pPps.iPpsId as i32, pPps.iSpsId as i32)
                as u32,
        ),
    );

    BsWriteOneBit(buf, pBsWriter, pPps.bEntropyCodingModeFlag as u32);
    BsWriteOneBit(buf, pBsWriter, 0); // bPicOrderPresentFlag

    // DISABLE_FMO_FEATURE branch, au_set.cpp:417.
    BsWriteUE(buf, pBsWriter, 0); // uiNumSliceGroups - 1

    BsWriteUE(buf, pBsWriter, 0); // uiNumRefIdxL0Active - 1
    BsWriteUE(buf, pBsWriter, 0); // uiNumRefIdxL1Active - 1

    BsWriteOneBit(buf, pBsWriter, 0); // bWeightedPredFlag
    BsWriteBits(buf, pBsWriter, 2, 0); // uiWeightedBiPredIdc

    BsWriteSE(buf, pBsWriter, pPps.iPicInitQp as i32 - 26);
    BsWriteSE(buf, pBsWriter, pPps.iPicInitQs as i32 - 26);

    BsWriteSE(buf, pBsWriter, pPps.uiChromaQpIndexOffset as i32);
    BsWriteOneBit(buf, pBsWriter,
        pPps.bDeblockingFilterControlPresentFlag as u32,
    );
    BsWriteOneBit(buf, pBsWriter, 0); // bConstainedIntraPredFlag
    BsWriteOneBit(buf, pBsWriter, 0); // bRedundantPicCntPresentFlag

    BsRbspTrailingBits(buf, pBsWriter);

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
/// **S11.18: the layer parameters are references, and F187's refusal expired.**
///
/// F187 (S29) refused this flip for a measured reason: `InitDqLayers` bound a
/// raw into the same spatial layer, that binding *spanned* `GenerateNewSps`,
/// and a `&mut` retag in here would pop it before the later read. The binding
/// was read exactly once, ~85 lines below its derivation; S11.18 derives it at
/// that use instead, so nothing spans the call and the retag has nothing to
/// invalidate. The deferral's premise, re-verified rather than inherited — and
/// this time it is gone, where S10.5b checked the same premise and found it
/// still live.
pub fn WelsInitSps(
    pSps: &mut SWelsSPS,
    pLayerParam: &mut SSpatialLayerConfig,
    pLayerParamInternal: &SSpatialLayerInternal,
    _kuiIntraPeriod: u32,
    kiNumRefFrame: i32,
    kuiSpsId: u32,
    kbEnableFrameCropping: bool,
    _bEnableRc: bool,
    kiDlayerCount: i32,
    bSVCBaselayer: bool,
) -> i32 {
    // C++ `memset (pSps, 0, sizeof (SWelsSPS))`. Deliberately not `SWelsSPS::default()`,
    // which seeds uiProfileIdc = PRO_BASELINE and the VUI *_UNDEF values rather than 0
    // — `SWelsSPS::ZERO` is that memset as a value (T6.G3).
    *pSps = SWelsSPS::ZERO;
    pSps.uiSpsId = kuiSpsId;
    pSps.iMbWidth = ((pLayerParam.iVideoWidth + 15) >> 4) as i16;
    pSps.iMbHeight = ((pLayerParam.iVideoHeight + 15) >> 4) as i16;

    // max value of both iFrameNum and POC are 2^16-1; in this encoder iPOC = 2*iFrameNum,
    // so max of iFrameNum should be 2^15-1.
    pSps.uiLog2MaxFrameNum = 15; // 16;
    pSps.uiPocType = 2;
    pSps.iLog2MaxPocLsb = 1 + pSps.uiLog2MaxFrameNum as i32;

    pSps.iNumRefFrames = kiNumRefFrame as i16; /* min pRef size when fifo pRef operation */

    if kbEnableFrameCropping {
        pSps.bFrameCroppingFlag = WelsGetPaddingOffset(
            pLayerParamInternal.iActualWidth,
            pLayerParamInternal.iActualHeight,
            pLayerParam.iVideoWidth,
            pLayerParam.iVideoHeight,
            &mut pSps.sFrameCrop,
        );
    } else {
        pSps.bFrameCroppingFlag = false;
    }
    pSps.uiProfileIdc = if pLayerParam.uiProfileIdc as u8 != 0 {
        pLayerParam.uiProfileIdc as u8
    } else {
        PRO_BASELINE as u8
    };
    if pLayerParam.uiProfileIdc == PRO_BASELINE {
        pSps.bConstraintSet0Flag = true;
    }
    if (pLayerParam.uiProfileIdc as i32) <= PRO_MAIN as i32 {
        pSps.bConstraintSet1Flag = true;
    }
    if (kiDlayerCount > 1) && bSVCBaselayer {
        pSps.bConstraintSet2Flag = true;
    }

    let mut uiLevel = WelsGetLevelIdc(
        pSps,
        pLayerParamInternal.fOutputFrameRate,
        pLayerParam.iSpatialBitrate,
    );
    // update level
    // For Scalable Baseline/High/High Intra, level_idc 9 means level 1b.
    // For Baseline/Constrained Baseline/Main/Extended, level_idc 11 with
    // constraint_set3_flag 1 means level 1b.
    if uiLevel == ELevelIdc::LEVEL_1_B
        && (pSps.uiProfileIdc == PRO_BASELINE as u8
            || pSps.uiProfileIdc == PRO_MAIN as u8
            || pSps.uiProfileIdc == PRO_EXTENDED as u8)
    {
        uiLevel = ELevelIdc::LEVEL_1_1;
        pSps.bConstraintSet3Flag = true;
    }
    if (pLayerParam.uiLevelIdc == LEVEL_UNKNOWN)
        || ((pLayerParam.uiLevelIdc as i32) < uiLevel as i32)
    {
        pLayerParam.uiLevelIdc = uiLevel;
    }
    pSps.iLevelIdc = pLayerParam.uiLevelIdc as u8;

    // bGapsInFrameNumValueAllowedFlag is false when spatial and temporal layer counts
    // are both 1 and ltr is 0.
    if (kiDlayerCount == 1) && (pSps.iNumRefFrames == 1) {
        pSps.bGapsInFrameNumValueAllowedFlag = false;
    } else {
        pSps.bGapsInFrameNumValueAllowedFlag = true;
    }

    pSps.bVuiParamPresentFlag = true;

    pSps.bAspectRatioPresent = pLayerParam.bAspectRatioPresent;
    pSps.eAspectRatio = pLayerParam.eAspectRatio as i32;
    pSps.sAspectRatioExtWidth = pLayerParam.sAspectRatioExtWidth;
    pSps.sAspectRatioExtHeight = pLayerParam.sAspectRatioExtHeight;

    // See codec_app_def.h and parameter_sets.h for more info about members
    // bVideoSignalTypePresent through uiColorMatrix.
    pSps.bVideoSignalTypePresent = pLayerParam.bVideoSignalTypePresent;
    pSps.uiVideoFormat = pLayerParam.uiVideoFormat;
    pSps.bFullRange = pLayerParam.bFullRange;
    pSps.bColorDescriptionPresent = pLayerParam.bColorDescriptionPresent;
    pSps.uiColorPrimaries = pLayerParam.uiColorPrimaries;
    pSps.uiTransferCharacteristics = pLayerParam.uiTransferCharacteristics;
    pSps.uiColorMatrix = pLayerParam.uiColorMatrix;

    0
}

/// `WelsInitSubsetSps` — au_set.cpp:566.
///
/// # Safety
/// All three pointers must be non-null and point to writable values.
#[allow(clippy::too_many_arguments)]
// S11.18: the layer parameters are references — see `WelsInitSps`.
pub fn WelsInitSubsetSps(
    pSubsetSps: &mut SSubsetSps,
    pLayerParam: &mut SSpatialLayerConfig,
    pLayerParamInternal: &SSpatialLayerInternal,
    kuiIntraPeriod: u32,
    kiNumRefFrame: i32,
    kuiSpsId: u32,
    kbEnableFrameCropping: bool,
    bEnableRc: bool,
    kiDlayerCount: i32,
) -> i32 {
    // The memset comes first now and the borrow after it, which is the same order
    // of effects: the C++ takes `pSps` before the memset and the memset then zeroes
    // the very bytes it points at.
    *pSubsetSps = SSubsetSps::ZERO;

    WelsInitSps(
        &mut pSubsetSps.pSps,
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
    pSubsetSps.pSps.uiProfileIdc = pLayerParam.uiProfileIdc as u8;

    pSubsetSps.sSpsSvcExt.iExtendedSpatialScalability = 0; /* ESS is 0 in default */
    pSubsetSps.sSpsSvcExt.bAdaptiveTcoeffLevelPredFlag = false;
    pSubsetSps.sSpsSvcExt.bSeqTcoeffLevelPredFlag = false;
    pSubsetSps.sSpsSvcExt.bSliceHeaderRestrictionFlag = true;

    0
}

/// `WelsInitPps` — au_set.cpp:588.
///
/// The `#if !defined(DISABLE_FMO_FEATURE)` slice-group block at au_set.cpp:614-636 is
/// not compiled — see `as264_common.h:53`.
///
/// **T6.G3: the two "at least one of these is null" parameters are `Option`s.** The
/// C++ takes both as pointers and picks between them on `kbUsingSubsetSps`, checking
/// the chosen one for null; that is three runtime states standing in for two, and the
/// port's own `debug_assert!`s were the record of which. The signature says it now,
/// and the selection is one expression that cannot pick something absent.
pub fn WelsInitPps(
    pPps: &mut SWelsPPS,
    pSps: Option<&SWelsSPS>,
    pSubsetSps: Option<&SSubsetSps>,
    kuiPpsId: u32,
    kbDeblockingFilterPresentFlag: bool,
    kbUsingSubsetSps: bool,
    kbEntropyCodingModeFlag: bool,
) -> i32 {
    let pUsedSps = if kbUsingSubsetSps {
        pSubsetSps.map(|s| &s.pSps)
    } else {
        pSps
    };
    // The C++'s two guards collapse to one: it rejected "neither pointer given"
    // up front and `debug_assert!`ed the selected arm separately. `None` on the
    // selected arm is the same rejection either way, and the caller cannot now
    // supply the wrong arm's set and have it silently used.
    let Some(pUsedSps) = pUsedSps else {
        return 1;
    };

    /* fill picture parameter set syntax */
    pPps.iPpsId = kuiPpsId;
    pPps.iSpsId = pUsedSps.uiSpsId;
    pPps.bEntropyCodingModeFlag = kbEntropyCodingModeFlag;

    pPps.iPicInitQp = 26;
    pPps.iPicInitQs = 26;

    pPps.uiChromaQpIndexOffset = 0;
    pPps.bDeblockingFilterControlPresentFlag = kbDeblockingFilterPresentFlag;

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::codec_api::EProfileIdc;

    /// The 160x96 / 6fps / baseline case the differential harness drives.
    fn gate_layer() -> (SSpatialLayerConfig, SSpatialLayerInternal) {
        let mut lp = SSpatialLayerConfig::default();
        lp.iVideoWidth = 160;
        lp.iVideoHeight = 96;
        lp.iSpatialBitrate = 500_000;
        lp.uiProfileIdc = EProfileIdc::PRO_BASELINE;
        lp.uiLevelIdc = LEVEL_UNKNOWN;

        let mut li = SSpatialLayerInternal::default();
        li.iActualWidth = 160;
        li.iActualHeight = 96;
        li.fOutputFrameRate = 6.0;
        (lp, li)
    }

    /// Field-for-field against the C++ `WelsInitSps` linked from `libopenh264.a` for
    /// the same input:
    ///
    /// ```text
    /// mbW=10 mbH=6 log2mfn=15 poc=2 log2poc=16 nref=1 prof=66 level=13
    /// gaps=0 crop=0 cs0=1 cs1=1 cs2=0 cs3=0
    /// ```
    #[test]
    fn init_sps_matches_cxx_for_the_gate_configuration() {
        let (mut lp, mut li) = gate_layer();
        let mut sps = SWelsSPS::default();
        assert_eq!(WelsInitSps(&mut sps, &mut lp, &mut li, 0, 1, 0, true, false, 1, false), 0);

        assert_eq!(sps.iMbWidth, 10);
        assert_eq!(sps.iMbHeight, 6);
        assert_eq!(sps.uiLog2MaxFrameNum, 15);
        assert_eq!(sps.uiPocType, 2);
        assert_eq!(sps.iLog2MaxPocLsb, 16);
        assert_eq!(sps.iNumRefFrames, 1);
        assert_eq!(sps.uiProfileIdc, EProfileIdc::PRO_BASELINE as u8);
        // WelsGetLevelIdc picks LEVEL_1_3 for 60 MBs at 6fps and 500 kbit/s, and
        // writes it back into the layer config.
        assert_eq!(sps.iLevelIdc, 13);
        assert_eq!(lp.uiLevelIdc, ELevelIdc::LEVEL_1_3);
        // one dependency layer with one reference frame
        assert!(!sps.bGapsInFrameNumValueAllowedFlag);
        // coded size equals actual size, so nothing to crop
        assert!(!sps.bFrameCroppingFlag);
        assert!(sps.bConstraintSet0Flag);
        assert!(sps.bConstraintSet1Flag);
        assert!(!sps.bConstraintSet2Flag);
        assert!(!sps.bConstraintSet3Flag);
    }

    /// Byte-exact against the C++ `WelsWriteSpsNal` for the same SPS. This is the
    /// check that would have caught the missing VUI: the ad-hoc writer this replaced
    /// stopped after `vui_parameters_present_flag = 0` and emitted 8 bytes.
    #[test]
    fn write_sps_nal_is_byte_exact_with_cxx() {
        let (mut lp, mut li) = gate_layer();
        let mut sps = SWelsSPS::default();
        let mut buf = [0u8; 512];
        let mut bs = BsWriter::new();
        let delta = [0i32; 32];

        // The `as_mut_ptr() as *const u8` accommodation that used to stand here is
        // gone with `InitBits` (F13's third site): the buffer is a `&mut [u8]` the
        // test already owns, so there is no provenance to launder and nothing to
        // explain. Deleting it is a named deliverable of Phase 3 — see the module
        // header of `tests/safe_bits_differential.rs`.
        WelsInitSps(&mut sps, &mut lp, &mut li, 0, 1, 0, true, false, 1, false);
        WelsWriteSpsNal(&mut buf, &mut sps, &mut bs, &delta);
        let written = bs.pos();

        assert_eq!(
            &buf[..written],
            &[0x42, 0xc0, 0x0d, 0x8c, 0x68, 0x28, 0xd2, 0x01, 0xe1, 0x10, 0x8d, 0x40],
            "SPS RBSP diverged from the C++ reference"
        );
    }

    /// Against the C++ `WelsInitPps`: `ppsid=0 spsid=0 qp=26 qs=26 cqpo=0 ecm=0 dfcp=1`.
    #[test]
    fn init_pps_matches_cxx() {
        let (mut lp, mut li) = gate_layer();
        let mut sps = SWelsSPS::default();
        let mut pps = SWelsPPS::default();
        WelsInitSps(&mut sps, &mut lp, &mut li, 0, 1, 0, true, false, 1, false);
        assert_eq!(
            WelsInitPps(&mut pps, Some(&sps), None, 0, true, false, false),
            0
        );
        assert_eq!(pps.iPpsId, 0);
        assert_eq!(pps.iSpsId, 0);
        assert_eq!(pps.iPicInitQp, 26);
        assert_eq!(pps.iPicInitQs, 26);
        assert_eq!(pps.uiChromaQpIndexOffset, 0);
        assert!(!pps.bEntropyCodingModeFlag);
        assert!(pps.bDeblockingFilterControlPresentFlag);
    }

    /// Byte-exact against the C++ `WelsWritePpsSyntax` driven with a real
    /// `CWelsParametersetIdConstant`, which is what makes this a test of the id
    /// offsets too rather than only of the fixed syntax elements.
    #[test]
    fn write_pps_syntax_is_byte_exact_with_cxx() {
        use crate::api::codec_api::EParameterSetStrategy;
        use crate::encoder::paraset_strategy::CreateParametersetStrategy;

        let (mut lp, mut li) = gate_layer();
        let mut sps = SWelsSPS::default();
        let mut pps = SWelsPPS::default();
        let mut buf = [0u8; 256];
        let mut bs = BsWriter::new();

        // Second of the two F13 accommodations deleted at T3.4 — see the sibling
        // test above.
        WelsInitSps(&mut sps, &mut lp, &mut li, 0, 1, 0, true, false, 1, false);
        WelsInitPps(&mut pps, Some(&sps), None, 0, true, false, false);

        let st = CreateParametersetStrategy(EParameterSetStrategy::CONSTANT_ID, false, 1)
            .expect("CONSTANT_ID is ported");
        WelsWritePpsSyntax(&mut buf, &mut pps, &mut bs, &st);
        let written = bs.pos();

        assert_eq!(
            &buf[..written],
            &[0xce, 0x3c, 0x80],
            "PPS RBSP diverged from the C++ reference"
        );
    }

    /// `WelsInitPps` rejects the combination C++ rejects: no SPS of either kind.
    #[test]
    fn init_pps_rejects_missing_sps() {
        // S5.E2b: `WelsInitPps` is a safe `fn` now, so the wrapper goes with it.
        let mut pps = SWelsPPS::default();
        assert_eq!(
            WelsInitPps(&mut pps, None, None, 0, true, false, false),
            1
        );
    }

    /// `WelsGetPaddingOffset` — au_set.cpp:476. A 1920x1080 coded frame carrying
    /// 1920x1080 actual content needs 1088 coded height, i.e. 4 cropped chroma rows.
    #[test]
    fn padding_offset_crops_the_coded_height() {
        let mut off = SCropOffset::default();
        assert!(WelsGetPaddingOffset(1920, 1080, 1920, 1088, &mut off));
        assert_eq!(off.iCropLeft, 0);
        assert_eq!(off.iCropRight, 0);
        assert_eq!(off.iCropTop, 0);
        assert_eq!(off.iCropBottom, 4);
    }

    /// Equal sizes need no cropping, and the actual size is made even first.
    #[test]
    fn padding_offset_reports_no_crop_when_sizes_match() {
        let mut off = SCropOffset::default();
        assert!(!WelsGetPaddingOffset(160, 96, 160, 96, &mut off));
        // An odd actual size rounds down, which does then require cropping.
        assert!(WelsGetPaddingOffset(161, 96, 161, 96, &mut off));
        assert_eq!(off.iCropRight, 0);
    }

    /// A coded size smaller than the actual size is rejected outright.
    #[test]
    fn padding_offset_rejects_undersized_coded_frame() {
        let mut off = SCropOffset::default();
        assert!(!WelsGetPaddingOffset(1920, 1080, 1280, 720, &mut off));
    }
}
