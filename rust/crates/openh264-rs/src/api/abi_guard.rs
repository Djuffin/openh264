//! **Compile-time pins on the layout of every type that crosses the C ABI.**
//!
//! These types are shared verbatim with `codec/api/wels/codec_api.h`,
//! `codec_app_def.h` and `codec_def.h`. Callers written against those headers pass
//! pointers straight into [`crate::api::codec_api`], so a field reordering or a width
//! change here is silent memory corruption, not a compile error.
//!
//! This module exists because the encoder port once declared `SFrameBSInfo`,
//! `SLayerBSInfo`, `SSourcePicture` and `SEncParamExt` a *second* time in `lib.rs`,
//! where a local item shadowed the `pub use` re-export. The duplicates were
//! 3104/24/72/864 bytes against the correct 7192/56/80/924, so every encoder write
//! landed at the wrong offset in the caller's struct.
//!
//! # Where the numbers come from (T8.C4)
//!
//! **Every number below is copied from `rust/tools/abi_sizes.txt`**, which is the
//! output of `rust/tools/abi_sizes.c` — a C program compiled against the upstream
//! headers that prints `sizeof`/`_Alignof` for each type and `offsetof` for each
//! pinned field. Before T8.C4 this file said the numbers "were extracted from the C
//! headers with a sizeof/offsetof dump on darwin/arm64", and that dump was not in the
//! tree; a pin and a struct could only be compared by reading both. Now they can be
//! compared by running `rust/tools/abi_sizes.sh --check`, which also rebuilds the
//! dumper as C++ and diffs the two front ends' answers.
//!
//! **A pin that disagrees with the port is a finding, not a number to adjust.** The
//! header is the contract; if this file stops compiling, the question is which side is
//! wrong, and the answer is in the header.
//!
//! # Coverage: 51 types
//!
//! Every named typedef in the three public headers that a C caller can pass or
//! receive — as a parameter, a return, a field of such a type, or a documented
//! `SetOption`/`GetOption` payload. That is 51 of the 53 the headers declare.
//!
//! The two that are not here are `SliceInfo` and `SRateThresholds`
//! (`codec_def.h:168`, `:182`). They are declared in a public header and **named by
//! nothing at all** — `grep -rn` over `codec/` finds only their own declarations, in
//! no signature, no field and no option payload — and the port does not declare them
//! either. There is nothing to pin and nothing to diverge.
//!
//! Before T8.C4 this file pinned **11** structs by size and **9** enums inline, and
//! carried no alignment pin except `SFrameBSInfo`'s. The seven boundary structs
//! session B named unpinned — `SBufferInfo`, `SDecoderCapability`,
//! `SDecoderStatistics`, `SParserBsInfo`, `SSysMEMBuffer`, `SVideoProperty`,
//! `SVuiSarInfo` — are pinned first below, with every field's offset, because they
//! are the ones a decoder consumer touches on every call.

#![allow(non_snake_case, non_camel_case_types)]

// T8.C7. This module is pure `const` assertion and has never contained an `unsafe`
// token; the deny is here so that it cannot acquire one silently, which is the only
// way a layout pin could ever start lying.
#![forbid(unsafe_code)]

use crate::api::codec_api::*;
use core::mem::{align_of, size_of};

macro_rules! assert_size {
    ($t:ty, $n:expr) => {
        const _: () = assert!(size_of::<$t>() == $n, concat!(stringify!($t), " size"));
    };
}

/// Alignment is half of a layout and it was unpinned for every type but one. A
/// struct can keep its size and change its alignment (a `#[repr(C, packed)]` slip,
/// or a field whose Rust type is more aligned than the C one) and every size pin
/// still passes while a caller's array of them walks off its stride.
macro_rules! assert_align {
    ($t:ty, $n:expr) => {
        const _: () = assert!(align_of::<$t>() == $n, concat!(stringify!($t), " align"));
    };
}

/// `offset_of!` is stable since 1.77; keep the check in a const block so a
/// mismatch is a compile error rather than a test failure.
macro_rules! assert_offset {
    ($t:ty, $f:ident, $n:expr) => {
        const _: () = assert!(
            core::mem::offset_of!($t, $f) == $n,
            concat!(stringify!($t), "::", stringify!($f), " offset")
        );
    };
}


// ===========================================================================
// The seven session B named unpinned — every one of them a struct a decoder
// consumer passes or reads on the call path, and none of them pinned at all.
// ===========================================================================

assert_size!(SBufferInfo, 72);
assert_align!(SBufferInfo, 8);
assert_offset!(SBufferInfo, iBufferStatus, 0);
assert_offset!(SBufferInfo, uiInBsTimeStamp, 8);
assert_offset!(SBufferInfo, uiOutYuvTimeStamp, 16);
assert_offset!(SBufferInfo, UsrData, 24);
assert_offset!(SBufferInfo, pDst, 48);

assert_size!(SSysMEMBuffer, 20);
assert_align!(SSysMEMBuffer, 4);
assert_offset!(SSysMEMBuffer, iWidth, 0);
assert_offset!(SSysMEMBuffer, iHeight, 4);
assert_offset!(SSysMEMBuffer, iFormat, 8);
assert_offset!(SSysMEMBuffer, iStride, 12);

assert_size!(SDecoderCapability, 36);
assert_align!(SDecoderCapability, 4);
assert_offset!(SDecoderCapability, iProfileIdc, 0);
assert_offset!(SDecoderCapability, iProfileIop, 4);
assert_offset!(SDecoderCapability, iLevelIdc, 8);
assert_offset!(SDecoderCapability, iMaxMbps, 12);
assert_offset!(SDecoderCapability, iMaxFs, 16);
assert_offset!(SDecoderCapability, iMaxCpb, 20);
assert_offset!(SDecoderCapability, iMaxDpb, 24);
assert_offset!(SDecoderCapability, iMaxBr, 28);
assert_offset!(SDecoderCapability, bRedPicCap, 32);

assert_size!(SDecoderStatistics, 104);
assert_align!(SDecoderStatistics, 4);
assert_offset!(SDecoderStatistics, uiWidth, 0);
assert_offset!(SDecoderStatistics, uiHeight, 4);
assert_offset!(SDecoderStatistics, fAverageFrameSpeedInMs, 8);
assert_offset!(SDecoderStatistics, fActualAverageFrameSpeedInMs, 12);
assert_offset!(SDecoderStatistics, uiDecodedFrameCount, 16);
assert_offset!(SDecoderStatistics, uiResolutionChangeTimes, 20);
assert_offset!(SDecoderStatistics, uiIDRCorrectNum, 24);
assert_offset!(SDecoderStatistics, uiAvgEcRatio, 28);
assert_offset!(SDecoderStatistics, uiAvgEcPropRatio, 32);
assert_offset!(SDecoderStatistics, uiEcIDRNum, 36);
assert_offset!(SDecoderStatistics, uiEcFrameNum, 40);
assert_offset!(SDecoderStatistics, uiIDRLostNum, 44);
assert_offset!(SDecoderStatistics, uiFreezingIDRNum, 48);
assert_offset!(SDecoderStatistics, uiFreezingNonIDRNum, 52);
assert_offset!(SDecoderStatistics, iAvgLumaQp, 56);
assert_offset!(SDecoderStatistics, iSpsReportErrorNum, 60);
assert_offset!(SDecoderStatistics, iSubSpsReportErrorNum, 64);
assert_offset!(SDecoderStatistics, iPpsReportErrorNum, 68);
assert_offset!(SDecoderStatistics, iSpsNoExistNalNum, 72);
assert_offset!(SDecoderStatistics, iSubSpsNoExistNalNum, 76);
assert_offset!(SDecoderStatistics, iPpsNoExistNalNum, 80);
assert_offset!(SDecoderStatistics, uiProfile, 84);
assert_offset!(SDecoderStatistics, uiLevel, 88);
assert_offset!(SDecoderStatistics, iCurrentActiveSpsId, 92);
assert_offset!(SDecoderStatistics, iCurrentActivePpsId, 96);
assert_offset!(SDecoderStatistics, iStatisticsLogInterval, 100);

assert_size!(SParserBsInfo, 48);
assert_align!(SParserBsInfo, 8);
assert_offset!(SParserBsInfo, iNalNum, 0);
assert_offset!(SParserBsInfo, pNalLenInByte, 8);
assert_offset!(SParserBsInfo, pDstBuff, 16);
assert_offset!(SParserBsInfo, iSpsWidthInPixel, 24);
assert_offset!(SParserBsInfo, iSpsHeightInPixel, 28);
assert_offset!(SParserBsInfo, uiInBsTimeStamp, 32);
assert_offset!(SParserBsInfo, uiOutBsTimeStamp, 40);

assert_size!(SVideoProperty, 8);
assert_align!(SVideoProperty, 4);
assert_offset!(SVideoProperty, size, 0);
assert_offset!(SVideoProperty, eVideoBsType, 4);

assert_size!(SVuiSarInfo, 12);
assert_align!(SVuiSarInfo, 4);
assert_offset!(SVuiSarInfo, uiSarWidth, 0);
assert_offset!(SVuiSarInfo, uiSarHeight, 4);
assert_offset!(SVuiSarInfo, bOverscanAppropriateFlag, 8);

// ===========================================================================
// Every other struct and union the ABI carries.
// ===========================================================================

assert_size!(OpenH264Version, 16);
assert_align!(OpenH264Version, 4);
assert_offset!(OpenH264Version, uMajor, 0);
assert_offset!(OpenH264Version, uMinor, 4);
assert_offset!(OpenH264Version, uRevision, 8);
assert_offset!(OpenH264Version, uReserved, 12);

assert_size!(crate::encoder::ref_list_mgr_svc::SLTRRecoverRequest, 20);
assert_align!(crate::encoder::ref_list_mgr_svc::SLTRRecoverRequest, 4);

assert_size!(crate::encoder::ref_list_mgr_svc::SLTRMarkingFeedback, 16);
assert_align!(crate::encoder::ref_list_mgr_svc::SLTRMarkingFeedback, 4);

assert_size!(crate::encoder::wels_encoder_ext::SLTRConfig, 8);
assert_align!(crate::encoder::wels_encoder_ext::SLTRConfig, 4);

assert_size!(SliceModeEnum, 4);
assert_align!(SliceModeEnum, 4);

assert_size!(SSliceArgument, 152);
assert_align!(SSliceArgument, 4);
assert_offset!(SSliceArgument, uiSliceMode, 0);
assert_offset!(SSliceArgument, uiSliceNum, 4);
assert_offset!(SSliceArgument, uiSliceMbNum, 8);
assert_offset!(SSliceArgument, uiSliceSizeConstraint, 148);

assert_size!(SSpatialLayerConfig, 200);
assert_align!(SSpatialLayerConfig, 4);
assert_offset!(SSpatialLayerConfig, sSliceArgument, 32);

assert_size!(SEncParamBase, 24);
assert_align!(SEncParamBase, 4);
assert_offset!(SEncParamBase, iUsageType, 0);
assert_offset!(SEncParamBase, iPicWidth, 4);
assert_offset!(SEncParamBase, iPicHeight, 8);
assert_offset!(SEncParamBase, iTargetBitrate, 12);
assert_offset!(SEncParamBase, iRCMode, 16);
assert_offset!(SEncParamBase, fMaxFrameRate, 20);

assert_size!(SEncParamExt, 924);
assert_align!(SEncParamExt, 4);
assert_offset!(SEncParamExt, iTemporalLayerNum, 24);
assert_offset!(SEncParamExt, sSpatialLayers, 32);
assert_offset!(SEncParamExt, iComplexityMode, 832);
assert_offset!(SEncParamExt, bEnableFrameSkip, 860);
assert_offset!(SEncParamExt, bEnableLongTermReference, 880);
assert_offset!(SEncParamExt, iMultipleThreadIdc, 892);
assert_offset!(SEncParamExt, iLoopFilterDisableIdc, 896);
assert_offset!(SEncParamExt, bEnableDenoise, 908);
assert_offset!(SEncParamExt, bIsLosslessLink, 913);
assert_offset!(SEncParamExt, bPsnrY, 920);

assert_size!(SDecodingParam, 32);
assert_align!(SDecodingParam, 8);
assert_offset!(SDecodingParam, pFileNameRestructed, 0);
assert_offset!(SDecodingParam, uiCpuLoad, 8);
assert_offset!(SDecodingParam, uiTargetDqLayer, 12);
assert_offset!(SDecodingParam, eEcActiveIdc, 16);
assert_offset!(SDecodingParam, bParseOnly, 20);
assert_offset!(SDecodingParam, sVideoProperty, 24);

assert_size!(SLayerBSInfo, 56);
assert_align!(SLayerBSInfo, 8);
assert_offset!(SLayerBSInfo, eFrameType, 4);
assert_offset!(SLayerBSInfo, uiLayerType, 8);
assert_offset!(SLayerBSInfo, iSubSeqId, 12);
assert_offset!(SLayerBSInfo, iNalCount, 16);
assert_offset!(SLayerBSInfo, pNalLengthInByte, 24);
assert_offset!(SLayerBSInfo, pBsBuf, 32);
assert_offset!(SLayerBSInfo, rPsnr, 40);

assert_size!(SFrameBSInfo, 7192);
assert_align!(SFrameBSInfo, 8);
assert_offset!(SFrameBSInfo, iLayerNum, 0);
assert_offset!(SFrameBSInfo, sLayerInfo, 8);
assert_offset!(SFrameBSInfo, eFrameType, 7176);
assert_offset!(SFrameBSInfo, iFrameSizeInBytes, 7180);
assert_offset!(SFrameBSInfo, uiTimeStamp, 7184);

assert_size!(SSourcePicture, 80);
assert_align!(SSourcePicture, 8);
assert_offset!(SSourcePicture, iStride, 4);
assert_offset!(SSourcePicture, pData, 24);
assert_offset!(SSourcePicture, iPicWidth, 56);
assert_offset!(SSourcePicture, iPicHeight, 60);
assert_offset!(SSourcePicture, uiTimeStamp, 64);

assert_size!(SBitrateInfo, 8);
assert_align!(SBitrateInfo, 4);
assert_offset!(SBitrateInfo, iLayer, 0);
assert_offset!(SBitrateInfo, iBitrate, 4);

assert_size!(crate::encoder::wels_encoder_ext::SDumpLayer, 16);
assert_align!(crate::encoder::wels_encoder_ext::SDumpLayer, 8);

assert_size!(crate::encoder::wels_encoder_ext::SProfileInfo, 8);
assert_align!(crate::encoder::wels_encoder_ext::SProfileInfo, 4);

assert_size!(crate::encoder::wels_encoder_ext::SLevelInfo, 8);
assert_align!(crate::encoder::wels_encoder_ext::SLevelInfo, 4);

assert_size!(crate::encoder::wels_encoder_ext::SDeliveryStatus, 12);
assert_align!(crate::encoder::wels_encoder_ext::SDeliveryStatus, 4);

assert_size!(SEncoderStatistics, 88);
assert_align!(SEncoderStatistics, 8);
assert_offset!(SEncoderStatistics, uiWidth, 0);
assert_offset!(SEncoderStatistics, uiHeight, 4);
assert_offset!(SEncoderStatistics, fAverageFrameSpeedInMs, 8);
assert_offset!(SEncoderStatistics, fAverageFrameRate, 12);
assert_offset!(SEncoderStatistics, fLatestFrameRate, 16);
assert_offset!(SEncoderStatistics, uiBitRate, 20);
assert_offset!(SEncoderStatistics, uiAverageFrameQP, 24);
assert_offset!(SEncoderStatistics, uiInputFrameCount, 28);
assert_offset!(SEncoderStatistics, uiSkippedFrameCount, 32);
assert_offset!(SEncoderStatistics, uiResolutionChangeTimes, 36);
assert_offset!(SEncoderStatistics, uiIDRReqNum, 40);
assert_offset!(SEncoderStatistics, uiIDRSentNum, 44);
assert_offset!(SEncoderStatistics, uiLTRSentNum, 48);
assert_offset!(SEncoderStatistics, iStatisticsTs, 56);
assert_offset!(SEncoderStatistics, iTotalEncodedBytes, 64);

assert_size!(ISVCEncoderVtbl, 72);
assert_align!(ISVCEncoderVtbl, 8);
assert_offset!(ISVCEncoderVtbl, Initialize, 0);
assert_offset!(ISVCEncoderVtbl, InitializeExt, 8);
assert_offset!(ISVCEncoderVtbl, GetDefaultParams, 16);
assert_offset!(ISVCEncoderVtbl, Uninitialize, 24);
assert_offset!(ISVCEncoderVtbl, EncodeFrame, 32);
assert_offset!(ISVCEncoderVtbl, EncodeParameterSets, 40);
assert_offset!(ISVCEncoderVtbl, ForceIntraFrame, 48);
assert_offset!(ISVCEncoderVtbl, SetOption, 56);
assert_offset!(ISVCEncoderVtbl, GetOption, 64);

assert_size!(ISVCDecoderVtbl, 80);
assert_align!(ISVCDecoderVtbl, 8);
assert_offset!(ISVCDecoderVtbl, Initialize, 0);
assert_offset!(ISVCDecoderVtbl, Uninitialize, 8);
assert_offset!(ISVCDecoderVtbl, DecodeFrame, 16);
assert_offset!(ISVCDecoderVtbl, DecodeFrameNoDelay, 24);
assert_offset!(ISVCDecoderVtbl, DecodeFrame2, 32);
assert_offset!(ISVCDecoderVtbl, FlushFrame, 40);
assert_offset!(ISVCDecoderVtbl, DecodeParser, 48);
assert_offset!(ISVCDecoderVtbl, DecodeFrameEx, 56);
assert_offset!(ISVCDecoderVtbl, SetOption, 64);
assert_offset!(ISVCDecoderVtbl, GetOption, 72);

// ===========================================================================
// The enums. C enums are `int` here on every target this project builds for, and
// they cross as struct fields (`SDecodingParam::eEcActiveIdc`,
// `SVideoProperty::eVideoBsType`, `SLayerBSInfo::eFrameType`, ...) and as
// `SetOption`/`GetOption` selectors. A `#[repr(C)]` Rust enum matches; a
// `#[repr(u8)]` one silently would not, and the caller's next field would land in
// the wrong place.
// ===========================================================================

assert_size!(EVideoFormatType, 4);
assert_align!(EVideoFormatType, 4);
assert_size!(EVideoFrameType, 4);
assert_align!(EVideoFrameType, 4);
assert_size!(CM_RETURN, 4);
assert_align!(CM_RETURN, 4);
assert_size!(DECODING_STATE, 4);
assert_align!(DECODING_STATE, 4);
assert_size!(ENCODER_OPTION, 4);
assert_align!(ENCODER_OPTION, 4);
assert_size!(DECODER_OPTION, 4);
assert_align!(DECODER_OPTION, 4);
assert_size!(ERROR_CON_IDC, 4);
assert_align!(ERROR_CON_IDC, 4);
assert_size!(FEEDBACK_VCL_NAL_IN_AU, 4);
assert_align!(FEEDBACK_VCL_NAL_IN_AU, 4);
assert_size!(LAYER_TYPE, 4);
assert_align!(LAYER_TYPE, 4);
assert_size!(LAYER_NUM, 4);
assert_align!(LAYER_NUM, 4);
assert_size!(VIDEO_BITSTREAM_TYPE, 4);
assert_align!(VIDEO_BITSTREAM_TYPE, 4);
assert_size!(KEY_FRAME_REQUEST_TYPE, 4);
assert_align!(KEY_FRAME_REQUEST_TYPE, 4);
assert_size!(RC_MODES, 4);
assert_align!(RC_MODES, 4);
assert_size!(EProfileIdc, 4);
assert_align!(EProfileIdc, 4);
assert_size!(ELevelIdc, 4);
assert_align!(ELevelIdc, 4);
assert_size!(EVideoFormatSPS, 4);
assert_align!(EVideoFormatSPS, 4);
assert_size!(EColorPrimaries, 4);
assert_align!(EColorPrimaries, 4);
assert_size!(ETransferCharacteristics, 4);
assert_align!(ETransferCharacteristics, 4);
assert_size!(EColorMatrix, 4);
assert_align!(EColorMatrix, 4);
assert_size!(ESampleAspectRatio, 4);
assert_align!(ESampleAspectRatio, 4);
assert_size!(EUsageType, 4);
assert_align!(EUsageType, 4);
assert_size!(ECOMPLEXITY_MODE, 4);
assert_align!(ECOMPLEXITY_MODE, 4);
assert_size!(EParameterSetStrategy, 4);
assert_align!(EParameterSetStrategy, 4);

#[cfg(test)]
mod tests {
    use super::*;

    /// The const asserts above already fail the build on a mismatch; this test
    /// exists so `cargo test` reports the module by name, and so the count is a
    /// number a session can quote.
    #[test]
    fn abi_layout_matches_c_headers() {
        assert_eq!(size_of::<SFrameBSInfo>(), 7192);
        assert_eq!(size_of::<SLayerBSInfo>(), 56);
        assert_eq!(size_of::<SSourcePicture>(), 80);
        assert_eq!(size_of::<SEncParamExt>(), 924);
        assert_eq!(core::mem::offset_of!(SFrameBSInfo, eFrameType), 7176);
        // The seven T8.C4 added, by the numbers in `rust/tools/abi_sizes.txt`.
        assert_eq!(size_of::<SBufferInfo>(), 72);
        assert_eq!(size_of::<SSysMEMBuffer>(), 20);
        assert_eq!(size_of::<SDecoderCapability>(), 36);
        assert_eq!(size_of::<SDecoderStatistics>(), 104);
        assert_eq!(size_of::<SParserBsInfo>(), 48);
        assert_eq!(size_of::<SVideoProperty>(), 8);
        assert_eq!(size_of::<SVuiSarInfo>(), 12);
    }

    /// The two vtables *are* the ABI's slot order, and their sizes say how many
    /// slots there are: nine encoder, ten decoder, one pointer each.
    #[test]
    fn the_vtables_have_the_slot_counts_the_header_declares() {
        assert_eq!(size_of::<ISVCEncoderVtbl>(), 9 * size_of::<usize>());
        assert_eq!(size_of::<ISVCDecoderVtbl>(), 10 * size_of::<usize>());
    }
}
