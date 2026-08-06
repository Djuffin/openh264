//! Compile-time guards on the layout of every type that crosses the C ABI.
//!
//! These types are shared verbatim with `codec/api/wels/codec_app_def.h` and
//! `codec_def.h`. Callers written against the C headers pass pointers straight
//! into [`crate::api::codec_api`], so a field reordering or a width change here
//! is silent memory corruption, not a compile error.
//!
//! The expected values were extracted from the C headers with a
//! `sizeof`/`offsetof` dump on darwin/arm64; they hold on any LP64 target.
//! Regenerate with `rust/docs/encoder_port_status.md`'s ABI section as the
//! reference if the upstream headers ever change.
//!
//! This module exists because the encoder port previously declared
//! `SFrameBSInfo`, `SLayerBSInfo`, `SSourcePicture` and `SEncParamExt` a second
//! time in `lib.rs`, where a local item shadowed the `pub use` re-export. The
//! duplicates were 3104/24/72/864 bytes against the correct 7192/56/80/924, so
//! every encoder write landed at the wrong offset in the caller's struct.

use crate::api::codec_api::*;
use core::mem::{align_of, size_of};

macro_rules! assert_size {
    ($t:ty, $n:expr) => {
        const _: () = assert!(size_of::<$t>() == $n, concat!(stringify!($t), " size"));
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

// ---- sizes (codec_app_def.h) ----
assert_size!(SEncParamBase, 24);
assert_size!(SEncParamExt, 924);
assert_size!(SSpatialLayerConfig, 200);
assert_size!(SSliceArgument, 152);
assert_size!(SSourcePicture, 80);
assert_size!(SLayerBSInfo, 56);
assert_size!(SFrameBSInfo, 7192);
assert_size!(SEncoderStatistics, 88);
assert_size!(SBitrateInfo, 8);
assert_size!(OpenH264Version, 16);
assert_size!(SDecodingParam, 32);

// ---- SEncParamExt ----
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

// ---- SSpatialLayerConfig / SSliceArgument ----
assert_offset!(SSpatialLayerConfig, sSliceArgument, 32);
assert_offset!(SSliceArgument, uiSliceMode, 0);
assert_offset!(SSliceArgument, uiSliceNum, 4);
assert_offset!(SSliceArgument, uiSliceMbNum, 8);
assert_offset!(SSliceArgument, uiSliceSizeConstraint, 148);

// ---- SSourcePicture ----
assert_offset!(SSourcePicture, iStride, 4);
assert_offset!(SSourcePicture, pData, 24);
assert_offset!(SSourcePicture, iPicWidth, 56);
assert_offset!(SSourcePicture, iPicHeight, 60);
assert_offset!(SSourcePicture, uiTimeStamp, 64);

// ---- SLayerBSInfo ----
assert_offset!(SLayerBSInfo, eFrameType, 4);
assert_offset!(SLayerBSInfo, uiLayerType, 8);
assert_offset!(SLayerBSInfo, iSubSeqId, 12);
assert_offset!(SLayerBSInfo, iNalCount, 16);
assert_offset!(SLayerBSInfo, pNalLengthInByte, 24);
assert_offset!(SLayerBSInfo, pBsBuf, 32);
assert_offset!(SLayerBSInfo, rPsnr, 40);

// ---- SFrameBSInfo ----
assert_offset!(SFrameBSInfo, iLayerNum, 0);
assert_offset!(SFrameBSInfo, sLayerInfo, 8);
assert_offset!(SFrameBSInfo, eFrameType, 7176);
assert_offset!(SFrameBSInfo, iFrameSizeInBytes, 7180);
assert_offset!(SFrameBSInfo, uiTimeStamp, 7184);

// ---- enums must be int-sized, as C enums are here ----
const _: () = assert!(size_of::<EUsageType>() == 4);
const _: () = assert!(size_of::<RC_MODES>() == 4);
const _: () = assert!(size_of::<EVideoFrameType>() == 4);
const _: () = assert!(size_of::<ECOMPLEXITY_MODE>() == 4);
const _: () = assert!(size_of::<EParameterSetStrategy>() == 4);
const _: () = assert!(size_of::<SliceModeEnum>() == 4);
const _: () = assert!(size_of::<EProfileIdc>() == 4);
const _: () = assert!(size_of::<ELevelIdc>() == 4);
const _: () = assert!(size_of::<LAYER_NUM>() == 4);
const _: () = assert!(align_of::<SFrameBSInfo>() == 8);

#[cfg(test)]
mod tests {
    use super::*;

    /// The const asserts above already fail the build on a mismatch; this test
    /// exists so `cargo test` reports the module by name and so the numbers are
    /// visible in test output when they are being re-derived.
    #[test]
    fn abi_layout_matches_c_headers() {
        assert_eq!(size_of::<SFrameBSInfo>(), 7192);
        assert_eq!(size_of::<SLayerBSInfo>(), 56);
        assert_eq!(size_of::<SSourcePicture>(), 80);
        assert_eq!(size_of::<SEncParamExt>(), 924);
        assert_eq!(core::mem::offset_of!(SFrameBSInfo, eFrameType), 7176);
    }
}
