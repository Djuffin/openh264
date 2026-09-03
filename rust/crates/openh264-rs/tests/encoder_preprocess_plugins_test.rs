//! **The two preprocessing plugins are accepted *and run*** (Phase 8b session C,
//! T8b.C1/T8b.C2).
//!
//! This file used to be `encoder_unsupported_preprocess_test.rs` and pinned the
//! opposite contract. `METHOD_DENOISE` and `METHOD_DOWNSAMPLE` were untranslated, and
//! — the actual defect — asking for either one *succeeded*:
//! `CWelsPreProcess::BilateralDenoising` was an empty body behind `bEnableDenoise`,
//! and `DownsamplePadding` returned `RET_NOTSUPPORTED` while **both callers dropped
//! the return**, so a lower spatial layer was encoded from whatever the picture pool
//! last held. A consumer got a successful encode and bytes that were not what it
//! asked for. S48 made both refuse at `ParamValidationExt` with `cmInitParaError`,
//! which is what T8b.A5 pinned here, at the cost of 17 gtest rows.
//!
//! Both plugins are ported now, the refusals are gone, and the assertions invert.
//!
//! **They invert into something stronger than "it initializes".** Deleting a guard
//! also makes `InitializeExt` succeed, and that is precisely the bug S48 was written
//! against — so every test below asserts that the plugin *changed the output*.
//! Denoise on must not produce the same bytes as denoise off; a two-layer encode
//! must not produce the same bytes as a one-layer encode of the same source. A port
//! that dropped the guard and left the kernel absent passes an init-only test and
//! fails these.
//!
//! Byte-exactness against the reference is refereed elsewhere — five targeted
//! `cxx_enc`/`rust_enc` pairs and the `dl` sweep preset. What lives here is the
//! property those pairs cannot state on their own: that the feature is reachable
//! through the public API at all.

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

/// A source frame with **structure at every scale**: a coarse gradient the
/// downsampler preserves, plus a fine per-pixel dither the denoiser is built to
/// remove. A flat frame would be a fixed point of both filters and every comparison
/// below would be vacuously equal.
fn textured_frame(w: i32, h: i32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let luma = w * h;
    let mut buf = vec![0u8; luma * 3 / 2];
    for y in 0..h {
        for x in 0..w {
            let coarse = ((x * 200 / w) + (y * 40 / h)) as u32;
            let dither = ((x * 7 + y * 13) % 23) as u32;
            buf[y * w + x] = (16 + coarse + dither).min(235) as u8;
        }
    }
    for y in 0..h / 2 {
        for x in 0..w / 2 {
            let c = (110 + (x * 5 + y * 11) % 31) as u8;
            buf[luma + y * (w / 2) + x] = c;
            buf[luma + luma / 4 + y * (w / 2) + x] = 255 - c;
        }
    }
    buf
}

/// Encode `frames` frames of [`textured_frame`] and return every coded byte, so two
/// configurations can be compared as bitstreams rather than as return codes.
fn encode_bytes(w: i32, h: i32, frames: usize, mutate: impl FnOnce(&mut SEncParamExt)) -> Vec<u8> {
    unsafe {
        let mut enc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);
        let mut p = base_params(enc, w, h);
        mutate(&mut p);
        assert_eq!(
            ISVCEncoder::InitializeExt(enc, &p as *const SEncParamExt),
            CM_RESULT_SUCCESS,
            "InitializeExt"
        );

        let luma = (w * h) as usize;
        let mut buf = textured_frame(w, h);
        let mut pic = SSourcePicture::default();
        pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
        pic.iPicWidth = w;
        pic.iPicHeight = h;
        pic.iStride[0] = w;
        pic.iStride[1] = w / 2;
        pic.iStride[2] = w / 2;
        pic.pData[0] = buf.as_mut_ptr();
        pic.pData[1] = buf.as_mut_ptr().add(luma);
        pic.pData[2] = buf.as_mut_ptr().add(luma + luma / 4);

        let mut out = Vec::new();
        for _ in 0..frames {
            let mut info = SFrameBSInfo::default();
            assert_eq!(
                ISVCEncoder::EncodeFrame(enc, &pic as *const SSourcePicture, &mut info),
                CM_RESULT_SUCCESS,
                "EncodeFrame"
            );
            for li in 0..info.iLayerNum as usize {
                let layer = &info.sLayerInfo[li];
                let n: i32 = (0..layer.iNalCount as usize)
                    .map(|i| *layer.pNalLengthInByte.add(i))
                    .sum();
                out.extend_from_slice(std::slice::from_raw_parts(layer.pBsBuf, n as usize));
            }
        }

        ISVCEncoder::Uninitialize(enc);
        WelsDestroySVCEncoder(enc);
        out
    }
}

/// [`textured_frame`] with every plane rolled up by `rows` luma lines (chroma by
/// half that, so the three planes stay registered). Wrapping rather than clamping,
/// so no new content enters and the only difference between two frames is the
/// displacement itself.
fn scrolled(base: &[u8], w: usize, h: usize, rows: usize) -> Vec<u8> {
    let luma = w * h;
    let (cw, ch) = (w / 2, h / 2);
    let mut out = vec![0u8; luma * 3 / 2];
    for y in 0..h {
        let src = (y + rows) % h;
        out[y * w..(y + 1) * w].copy_from_slice(&base[src * w..(src + 1) * w]);
    }
    for pl in 0..2usize {
        let off = luma + pl * (luma / 4);
        for y in 0..ch {
            let src = (y + rows / 2) % ch;
            out[off + y * cw..off + (y + 1) * cw]
                .copy_from_slice(&base[off + src * cw..off + (src + 1) * cw]);
        }
    }
    out
}

/// [`encode_bytes`] over a **scrolling** source: frame `i` is [`textured_frame`]
/// rolled up by `i * rows_per_frame` lines. Rate control is off and the QP fixed,
/// so the coded size reflects how well the encoder predicted the motion rather
/// than a bitrate target both configurations would hit alike.
fn encode_scrolling(
    w: i32,
    h: i32,
    frames: usize,
    rows_per_frame: usize,
    mutate: impl FnOnce(&mut SEncParamExt),
) -> Vec<u8> {
    unsafe {
        let mut enc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);
        let mut p = base_params(enc, w, h);
        p.iRCMode = RC_MODES::RC_OFF_MODE;
        p.iMinQp = 0;
        p.iMaxQp = 51;
        p.sSpatialLayers[0].iDLayerQp = 26;
        mutate(&mut p);
        assert_eq!(
            ISVCEncoder::InitializeExt(enc, &p as *const SEncParamExt),
            CM_RESULT_SUCCESS,
            "InitializeExt"
        );

        let (uw, uh) = (w as usize, h as usize);
        let luma = uw * uh;
        let base = textured_frame(w, h);
        let mut out = Vec::new();
        for f in 0..frames {
            let mut buf = scrolled(&base, uw, uh, (f * rows_per_frame) % uh);
            let mut pic = SSourcePicture::default();
            pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
            pic.iPicWidth = w;
            pic.iPicHeight = h;
            pic.iStride[0] = w;
            pic.iStride[1] = w / 2;
            pic.iStride[2] = w / 2;
            pic.pData[0] = buf.as_mut_ptr();
            pic.pData[1] = buf.as_mut_ptr().add(luma);
            pic.pData[2] = buf.as_mut_ptr().add(luma + luma / 4);

            let mut info = SFrameBSInfo::default();
            assert_eq!(
                ISVCEncoder::EncodeFrame(enc, &pic as *const SSourcePicture, &mut info),
                CM_RESULT_SUCCESS,
                "EncodeFrame"
            );
            for li in 0..info.iLayerNum as usize {
                let layer = &info.sLayerInfo[li];
                let n: i32 = (0..layer.iNalCount as usize)
                    .map(|i| *layer.pNalLengthInByte.add(i))
                    .sum();
                out.extend_from_slice(std::slice::from_raw_parts(layer.pBsBuf, n as usize));
            }
        }

        ISVCEncoder::Uninitialize(enc);
        WelsDestroySVCEncoder(enc);
        out
    }
}

/// **P10.1: `SCREEN_CONTENT_REAL_TIME` is accepted at init.** Three port-added
/// refusals stood where the C++ allocates — the VAA extension
/// (`RequestMemoryVaaScreen`), the last layer's feature-search preparation
/// (`RequestFeatureSearchPreparation`) and the reference pictures' feature storage
/// (`RequestScreenBlockFeatureStorage`) — and `InitializeExt` answered
/// `cmInitParaError`. P10.1.B3/B4/B5 ported the three allocations.
#[test]
fn screen_content_is_accepted_at_init() {
    assert_eq!(
        init_code(|p| p.iUsageType = EUsageType::SCREEN_CONTENT_REAL_TIME),
        CM_RESULT_SUCCESS
    );
}

/// Long-term reference over a lossless link: the `CWelsReference_LosslessWithLtr`
/// strategy and the scene-LTR marking family behind it.
#[test]
fn screen_content_with_lossless_ltr_is_accepted_at_init() {
    assert_eq!(
        init_code(|p| {
            p.iUsageType = EUsageType::SCREEN_CONTENT_REAL_TIME;
            p.bEnableLongTermReference = true;
            p.bIsLosslessLink = true;
        }),
        CM_RESULT_SUCCESS
    );
}

/// Upstream's `EncodeDecodeTestAPI.ScreenContent_LosslessLink0_EnableLongTermReference`:
/// LTR asked for without a lossless link. `ParamValidationExt` turns LTR off with a
/// warning (`encoder_ext.cpp:415-419`) and the init succeeds.
#[test]
fn screen_content_ltr_without_lossless_link_is_accepted_at_init() {
    assert_eq!(
        init_code(|p| {
            p.iUsageType = EUsageType::SCREEN_CONTENT_REAL_TIME;
            p.bEnableLongTermReference = true;
            p.bIsLosslessLink = false;
        }),
        CM_RESULT_SUCCESS
    );
}

/// A screen-content sequence encodes to completion. It deliberately asserts nothing
/// about the bytes: `screen_content_scrolling_source_codes_smaller_than_camera`
/// below is the row that does, and it took P10.2's three plugins and P10.3's
/// dispatch block to be able to. Byte-exactness against the reference is the `scc`
/// sweep preset's (`SCC_TIER=min`, 28/28 since P10.3.D4).
#[test]
fn screen_content_encodes_a_sequence() {
    let screen = encode_bytes(320, 192, 12, |p| p.iUsageType = EUsageType::SCREEN_CONTENT_REAL_TIME);
    assert!(!screen.is_empty(), "a screen-content encode must produce a stream");
}

/// The control: the configuration every gate in this project uses is still accepted.
/// Without this row the assertions below could be passing because *everything*
/// works by accident.
#[test]
fn the_gates_own_single_layer_configuration_is_still_accepted() {
    assert_eq!(init_code(|_| {}), CM_RESULT_SUCCESS);
}

/// `bEnableDenoise` is accepted, and the filter runs: the coded bytes differ from
/// the same source encoded without it.
#[test]
fn denoise_is_accepted_and_changes_the_output() {
    assert_eq!(init_code(|p| p.bEnableDenoise = true), CM_RESULT_SUCCESS);

    let off = encode_bytes(320, 192, 3, |p| p.bEnableDenoise = false);
    let on = encode_bytes(320, 192, 3, |p| p.bEnableDenoise = true);
    assert!(!off.is_empty() && !on.is_empty(), "both configurations coded something");
    assert_ne!(
        off, on,
        "bEnableDenoise produced identical bytes — the guard was removed but \
         METHOD_DENOISE did not run"
    );
}

/// Two spatial layers — `EncoderOutputTest/5`'s shape — are accepted, and the lower
/// layer is really there: the stream is larger than the one-layer encode and differs
/// from it.
#[test]
fn a_downsampled_spatial_layer_is_accepted_and_encoded() {
    // `BaseEncoderTest`'s own layer rule (`test/api/BaseEncoderTest.cpp:34-64`):
    // layer i is the input halved `n - 1 - i` times, the target bitrate is
    // multiplied by the layer count, and every layer carries that bitrate.
    // Skipping the bitrate scaling makes `ParamValidationExt` refuse the block —
    // which is a real check, not this test's business to route around silently.
    let two_layers = |p: &mut SEncParamExt| {
        let template = p.sSpatialLayers[0];
        p.iSpatialLayerNum = 2;
        for i in 0..2usize {
            p.sSpatialLayers[i] = template;
            p.sSpatialLayers[i].iVideoWidth = 320 >> (1 - i);
            p.sSpatialLayers[i].iVideoHeight = 192 >> (1 - i);
            p.sSpatialLayers[i].iSpatialBitrate = p.iTargetBitrate;
        }
        // *After* the per-layer assignment, exactly as `BaseEncoderTest` does it:
        // each layer carries the base rate and only the overall target scales.
        p.iTargetBitrate *= 2;
    };
    assert_eq!(init_code(two_layers), CM_RESULT_SUCCESS);

    let one = encode_bytes(320, 192, 3, |_| {});
    let two = encode_bytes(320, 192, 3, two_layers);
    assert_ne!(one, two, "a second spatial layer must change the bitstream");
    assert!(
        two.len() > one.len(),
        "the two-layer stream carries an extra layer per frame: {} vs {} bytes",
        two.len(),
        one.len()
    );

    // And the single-layer form of the same thing: one layer, smaller than the
    // source picture. `JudgeNeedOfScaling` downsamples this too — it is the path
    // where `pScaledInputPicture` is allocated and the *top* layer is the
    // downsample target.
    assert_eq!(
        init_code(|p| {
            p.sSpatialLayers[0].iVideoWidth = 160;
            p.sSpatialLayers[0].iVideoHeight = 96;
        }),
        CM_RESULT_SUCCESS,
        "one layer smaller than the source picture is a downsample, and is supported"
    );
    let scaled = encode_bytes(320, 192, 3, |p| {
        p.sSpatialLayers[0].iVideoWidth = 160;
        p.sSpatialLayers[0].iVideoHeight = 96;
    });
    assert!(!scaled.is_empty());
    assert!(
        scaled.len() < one.len(),
        "a 160x96 layer from a 320x192 source codes fewer bytes than the full size: \
         {} vs {}",
        scaled.len(),
        one.len()
    );
}

/// Four spatial layers at 1280x720 — `EncoderOutputTest/7`'s shape, and the case
/// that distinguishes a correct downsampler from an obvious-but-wrong one.
///
/// The reference reaches each layer by **cascaded halving through a scratch buffer**
/// (F98), not by the quarter/one-third kernels that `CDownsampling::Process`'s first
/// arm would suggest — that arm is the out-of-memory fallback, not the normal path.
/// Three halvings happen here.
#[test]
fn four_spatial_layers_at_720p_are_accepted_and_encoded() {
    let four = |p: &mut SEncParamExt| {
        let template = p.sSpatialLayers[0];
        p.iSpatialLayerNum = 4;
        for i in 0..4usize {
            p.sSpatialLayers[i] = template;
            p.sSpatialLayers[i].iVideoWidth = 1280 >> (3 - i);
            p.sSpatialLayers[i].iVideoHeight = 720 >> (3 - i);
            p.sSpatialLayers[i].iSpatialBitrate = p.iTargetBitrate;
        }
        p.iTargetBitrate *= 4;
    };
    let one = encode_bytes(1280, 720, 2, |_| {});
    let all = encode_bytes(1280, 720, 2, four);
    assert!(!all.is_empty(), "the four-layer encode coded something");
    assert!(
        all.len() > one.len(),
        "four layers carry more than one: {} vs {} bytes",
        all.len(),
        one.len()
    );
}

/// A layer **larger** than the input rect is not downsampled, and must not be
/// refused: `ParamTranscode` rounds layer dimensions up to a multiple of 16 while
/// leaving `iPicWidth` alone, so 140x96 legitimately becomes a 144x96 layer. The
/// first version of the T8b.A5 check compared `iPicWidth` against the top layer and
/// refused every non-multiple-of-16 width. The check is gone, but the shape it got
/// wrong is still worth a row.
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

/// **P10.3: the screen-content algorithms ran.** A source that scrolls eight lines
/// per frame codes *materially smaller* under `SCREEN_CONTENT_REAL_TIME` than under
/// `CAMERA_VIDEO_REAL_TIME`, at the same fixed QP with rate control off.
///
/// **Why the comparison is this one and not "the bytes differ".** Camera and screen
/// bytes have differed since long before this phase — the two usage types already
/// disagree about MV range, QP range and reference count — so an `assert_ne!` here
/// would have passed at P10.1, with every screen algorithm dormant, and would not
/// have meant what it said. What cannot happen without the dispatch block is
/// *exploiting the scroll*: `DetectSceneChangeScreen` finds the displacement,
/// `PreprocessSliceCoding`'s screen block installs `SetScrollingMvToMd` and
/// `WelsMotionEstimateSearchScrolled`, and the search starts at the right vector
/// instead of hunting for it. That shows up as size, which is why size is what is
/// asserted. The inequality is checked too, as the weaker half.
///
/// Correctness — that these bytes are the C++'s bytes — is the `scc` sweep's claim,
/// not this file's. What lives here is that the feature is reachable and effective
/// through the public API alone.
#[test]
fn screen_content_scrolling_source_codes_smaller_than_camera() {
    let camera = encode_scrolling(320, 192, 12, 8, |p| {
        p.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME
    });
    let screen = encode_scrolling(320, 192, 12, 8, |p| {
        p.iUsageType = EUsageType::SCREEN_CONTENT_REAL_TIME
    });
    assert!(
        !camera.is_empty() && !screen.is_empty(),
        "both configurations coded something"
    );
    assert_ne!(
        camera, screen,
        "screen content produced the camera path's bytes exactly — the dispatch \
         block did not run"
    );
    assert!(
        screen.len() < camera.len(),
        "a scrolling source must code smaller under screen content than under \
         camera content: {} vs {} bytes",
        screen.len(),
        camera.len()
    );
}
