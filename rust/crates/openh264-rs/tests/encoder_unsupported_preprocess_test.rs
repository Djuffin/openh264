//! **S48 for the two untranslated preprocessing plugins** (Phase 8b session A, T8b.A5).
//!
//! `METHOD_DENOISE` and `METHOD_DOWNSAMPLE` are not ported
//! (`processing/mod.rs:29-39`). Until T8b.A5 asking for either one *succeeded*:
//! `CWelsPreProcess::BilateralDenoising` is an empty body behind `bEnableDenoise`,
//! and `DownsamplePadding` returns `RET_NOTSUPPORTED` when the sizes differ while
//! **both of its callers drop the return** (`wels_preprocess.rs:1240`, `:1327`). A
//! consumer got a successful encode and bytes that were not what it asked for —
//! `EncoderOutputTest/4` (denoise on), `/5` (2 spatial layers) and `/7` (4 layers)
//! catch it as a *hash* difference, which is the only way it is visible.
//!
//! S48: a feature this phase cannot finish is left **refusing** at the entry point,
//! with a test pinning the error code. `ParamValidationExt` returns
//! `ENC_RETURN_UNSUPPORTED_PARA`; `WelsInitEncoderExt` passes it on, and
//! `InitializeInternal` maps a nonzero to `cmInitParaError` — so `cmInitParaError`
//! (1) is what a consumer sees, and that is what this file pins.
//!
//! The port lands in Phase 8b session C. When it does, these assertions invert and
//! the three `EncoderOutputTest` allowlist rows go away with them.

use openh264_rs::api::codec_api::*;

/// The gate's own single-layer configuration, which must stay accepted.
fn base_params(enc: *mut ISVCEncoder, w: i32, h: i32) -> SEncParamExt {
    let mut p = SEncParamExt::default();
    unsafe {
        assert_eq!(
            ISVCEncoder::GetDefaultParams(enc, &mut p as *mut SEncParamExt),
            CM_RESULT_SUCCESS
        );
    }
    p.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
    p.iPicWidth = w;
    p.iPicHeight = h;
    p.fMaxFrameRate = 30.0;
    p.iTargetBitrate = 2_000_000;
    p.iSpatialLayerNum = 1;
    p.iMultipleThreadIdc = 1;
    p.sSpatialLayers[0].iVideoWidth = w;
    p.sSpatialLayers[0].iVideoHeight = h;
    p.sSpatialLayers[0].fFrameRate = 30.0;
    p.sSpatialLayers[0].iSpatialBitrate = 2_000_000;
    p
}

/// `InitializeExt`'s answer for one parameter block.
fn init_code(mutate: impl FnOnce(&mut SEncParamExt)) -> i32 {
    unsafe {
        let mut enc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);
        let mut p = base_params(enc, 320, 192);
        mutate(&mut p);
        let rv = ISVCEncoder::InitializeExt(enc, &p as *const SEncParamExt);
        if rv == CM_RESULT_SUCCESS {
            ISVCEncoder::Uninitialize(enc);
        }
        WelsDestroySVCEncoder(enc);
        rv
    }
}

/// The control: the configuration every gate in this project uses is still accepted.
/// Without this row the two refusals below could be passing because *everything*
/// fails, which is the failure mode an S48 test is most likely to have.
#[test]
fn the_gates_own_single_layer_configuration_is_still_accepted() {
    assert_eq!(init_code(|_| {}), CM_RESULT_SUCCESS);
}

#[test]
fn denoise_is_refused_rather_than_silently_skipped() {
    assert_eq!(
        init_code(|p| p.bEnableDenoise = true),
        cm_init_para_error(),
        "bEnableDenoise must refuse until METHOD_DENOISE is ported (8b.C)"
    );
}

#[test]
fn a_downsampled_spatial_layer_is_refused_rather_than_silently_skipped() {
    // Two layers, the lower one half size — `EncoderOutputTest/5`'s shape.
    assert_eq!(
        init_code(|p| {
            p.iSpatialLayerNum = 2;
            p.sSpatialLayers[1] = p.sSpatialLayers[0];
            p.sSpatialLayers[0].iVideoWidth = 160;
            p.sSpatialLayers[0].iVideoHeight = 96;
        }),
        cm_init_para_error(),
        "a spatial layer smaller than the input must refuse until METHOD_DOWNSAMPLE is ported (8b.C)"
    );

    // And the single-layer form of the same thing: one layer, smaller than the
    // source picture. `JudgeNeedOfScaling` downsamples this too.
    assert_eq!(
        init_code(|p| {
            p.sSpatialLayers[0].iVideoWidth = 160;
            p.sSpatialLayers[0].iVideoHeight = 96;
        }),
        cm_init_para_error(),
        "one layer smaller than the source picture is still a downsample"
    );
}

/// A layer **larger** than the input rect is not downsampled, and must not be
/// refused: `ParamTranscode` rounds layer dimensions up to a multiple of 16 while
/// leaving `iPicWidth` alone, so 140x96 legitimately becomes a 144x96 layer. The
/// first version of the T8b.A5 check compared `iPicWidth` against the top layer and
/// refused every non-multiple-of-16 width.
#[test]
fn a_layer_rounded_up_to_a_macroblock_multiple_is_not_a_downsample() {
    unsafe {
        let mut enc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);
        let mut p = base_params(enc, 140, 96);
        p.sSpatialLayers[0].iVideoWidth = 144; // what ParamTranscode would produce
        let rv = ISVCEncoder::InitializeExt(enc, &p as *const SEncParamExt);
        assert_eq!(rv, CM_RESULT_SUCCESS, "140x96 must still initialize");
        ISVCEncoder::Uninitialize(enc);
        WelsDestroySVCEncoder(enc);
    }
}

/// `cmInitParaError` — `welsEncoderExt`'s code for a parameter block it will not
/// take. Spelled here rather than imported because the constant lives on the
/// encoder side of the crate.
fn cm_init_para_error() -> i32 {
    1
}
