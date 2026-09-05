#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/scenechangedetection/` — the plugin reached
//! through `METHOD_SCENE_CHANGE_DETECTION_VIDEO`.
//!
//! `CWelsPreProcessVideo::DetectSceneChange` (`wels_preprocess.cpp:600`) calls it
//! for every P frame when `bEnableSceneChangeDetect` is set, which `FillDefault`
//! leaves **on**. `DecideFrameType` turns a `LARGE_CHANGED_SCENE` verdict into an
//! IDR, subject to `iFrameIndex >= (VGOP_SIZE << 1)`.
//!
//! The C++ is a template, `CSceneChangeDetection<T>`, over two detector functors.
//! [`CSceneChangeDetection`] is the video one,
//! `METHOD_SCENE_CHANGE_DETECTION_VIDEO`; [`CSceneChangeDetectionScreen`] is
//! `METHOD_SCENE_CHANGE_DETECTION_SCREEN`, which the screen preprocessor runs once
//! per available reference on every P frame. The two functors differ in what they
//! *write*, not only in how they measure — the screen one fills a block-static map
//! the video one has no parameter for.

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use crate::simd::kernels::sad::sample_sad_8x8;
use crate::encoder::wels_preprocess::{ESceneChangeIdc, EStaticBlockIdc, SPixMap, SSceneChangeResult};
use crate::safe::plane::PlaneCursor;

/// The two luma planes this detector walks, routed from the pool pictures that own
/// them. `DenoisePlanes` is the same shape one plugin over
/// (`processing/denoise.rs:220`); this one is read-only and luma-only, which is all
/// `CSceneChangeDetectorVideo` ever touches.
///
/// Each slice starts at its plane's logical origin and runs to the end of the padded
/// allocation, so a block at `(x, y)` is at byte `y * stride + x`.
pub struct ScdPlanes<'a> {
    pub cur: &'a [u8],
    pub cur_stride: usize,
    pub refp: &'a [u8],
    pub ref_stride: usize,
}

use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};

/// `SceneChangeDetection.h:52-55`.
const HIGH_MOTION_BLOCK_THRESHOLD: i32 = 320;
const SCENE_CHANGE_MOTION_RATIO_LARGE_VIDEO: f32 = 0.85;
const SCENE_CHANGE_MOTION_RATIO_MEDIUM: f32 = 0.50;
const SCENE_CHANGE_MOTION_RATIO_LARGE_SCREEN: f32 = 0.80;

/// `PESN` — `util.h:60`.
const PESN: f32 = 1e-6;

/// `CSceneChangeDetection<CSceneChangeDetectorVideo>` — `SceneChangeDetection.h:204`.
#[derive(Default)]
pub struct CSceneChangeDetection {
    pub m_sSceneChangeParam: SSceneChangeResult,
}

impl CSceneChangeDetection {
    /// `CSceneChangeDetection::Set`.
    pub fn Set(&mut self, param: &SSceneChangeResult) -> i32 {
        self.m_sSceneChangeParam = *param;
        RET_SUCCESS
    }

    /// `CSceneChangeDetection::Get` — copies the whole result struct back.
    pub fn Get(&self, param: &mut SSceneChangeResult) -> i32 {
        *param = self.m_sSceneChangeParam;
        RET_SUCCESS
    }

    /// `CSceneChangeDetection::Process` — `SceneChangeDetection.h:215`, with
    /// `CSceneChangeDetectorVideo::operator()` inlined.
    ///
    /// `pSrcPixMap` carries the geometry (it is the VP's own parameter block); the
    /// pixels arrive as [`ScdPlanes`], and the block walk is slice indexing over two
    /// [`PlaneCursor`]s.
    pub fn Process(&mut self, pSrcPixMap: &SPixMap, planes: &ScdPlanes<'_>) -> i32 {
        if planes.cur.is_empty() || planes.refp.is_empty() {
            return RET_INVALIDPARAM;
        }
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iBlock8x8Width = (iWidth >> 3).max(0) as usize;
        let iBlock8x8Height = (iHeight >> 3).max(0) as usize;

        let iBlock8x8Num = (iBlock8x8Width * iBlock8x8Height) as i32;
        let iSceneChangeThresholdLarge =
            (SCENE_CHANGE_MOTION_RATIO_LARGE_VIDEO * iBlock8x8Num as f32 + 0.5 + PESN) as i32;
        let iSceneChangeThresholdMedium =
            (SCENE_CHANGE_MOTION_RATIO_MEDIUM * iBlock8x8Num as f32 + 0.5 + PESN) as i32;

        self.m_sSceneChangeParam.iMotionBlockNum = 0;
        self.m_sSceneChangeParam.iFrameComplexity = 0;
        self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::SIMILAR_SCENE;

        // CSceneChangeDetectorVideo::operator() — SceneChangeDetection.h:113. The C++
        // walks two row cursors and steps them by `stride << 3`; the offsets below are
        // the same arithmetic with the multiplication written out.
        for j in 0..iBlock8x8Height {
            for i in 0..iBlock8x8Width {
                let cur = PlaneCursor::new(
                    planes.cur,
                    j * 8 * planes.cur_stride + i * 8,
                    planes.cur_stride,
                );
                let refp = PlaneCursor::new(
                    planes.refp,
                    j * 8 * planes.ref_stride + i * 8,
                    planes.ref_stride,
                );
                let iSad = sample_sad_8x8(&cur, &refp);
                self.m_sSceneChangeParam.iMotionBlockNum +=
                    (iSad > HIGH_MOTION_BLOCK_THRESHOLD) as i32;
            }
        }

        if self.m_sSceneChangeParam.iMotionBlockNum >= iSceneChangeThresholdLarge {
            self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::LARGE_CHANGED_SCENE;
        } else if self.m_sSceneChangeParam.iMotionBlockNum >= iSceneChangeThresholdMedium {
            self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::MEDIUM_CHANGED_SCENE;
        }

        RET_SUCCESS
    }
}

/// `CSceneChangeDetection<CSceneChangeDetectorScreen>` — the functor at
/// `SceneChangeDetection.h:141-192` over the shared `Process` at `:204-249`, built by
/// `BuildSceneChangeDetection` for `METHOD_SCENE_CHANGE_DETECTION_SCREEN`
/// (`SceneChangeDetection.cpp:44-46`).
///
/// Two things the video detector does not do. It **classifies** every 8x8 block into
/// the caller's block-static map — collocated-static, scrolled-static, or moving — and
/// the mode decision reads that map to skip macroblocks (`SetBlockStaticIdcToMd`,
/// `JudgeStaticSkip`, `JudgeScrollSkip`). And it accumulates `iFrameComplexity`, which
/// the reference-selection judgement in `DetectSceneChangeScreen` sorts candidate
/// references by. Its "large scene change" ratio is 0.80 rather than the video
/// detector's 0.85; the medium ratio is the same 0.50.
#[derive(Default)]
pub struct CSceneChangeDetectionScreen {
    pub m_sSceneChangeParam: SSceneChangeResult,
}

impl CSceneChangeDetectionScreen {
    /// `CSceneChangeDetection::Set` — `SceneChangeDetection.h:259`.
    pub fn Set(&mut self, param: &SSceneChangeResult) -> i32 {
        self.m_sSceneChangeParam = *param;
        RET_SUCCESS
    }

    /// `CSceneChangeDetection::Get` — `SceneChangeDetection.h:251`. The whole result
    /// struct, as the C++ assignment copies it.
    pub fn Get(&self, param: &mut SSceneChangeResult) -> i32 {
        *param = self.m_sSceneChangeParam;
        RET_SUCCESS
    }

    /// `CSceneChangeDetection::Process` — `SceneChangeDetection.h:215-249` — with
    /// `CSceneChangeDetectorScreen::operator()` (`:158-191`) inlined.
    ///
    /// **The block-static row travels as `&mut [u8]`.** The C++ reads
    /// `m_sSceneChangeParam.pStaticBlockIdc` — a `uint8_t*` copied in through `Set` —
    /// and post-increments it once per block. Here the *selector* stays in
    /// [`SSceneChangeResult::pStaticBlockIdc`] for the bookkeeping the judgement code
    /// does with it, and the caller resolves it to the row with
    /// `SBlockStaticIdcStore::row_mut`. A row shorter than the block grid is
    /// `RET_INVALIDPARAM` and **nothing is written** — where the C++ would run off the
    /// end of the allocation, or write through `NULL` when the selector names no row.
    ///
    /// **The scroll test is `(!iScrollMvX || !iScrollMvY)`** — *at least one component
    /// zero*, not both. This scroll detector never produces a non-zero `iScrollMvX`,
    /// so the disjunct that can be false is unreachable from the encoder. The bounds
    /// check that follows it agrees with the read it guards: both add the vector.
    pub fn Process(
        &mut self,
        pSrcPixMap: &SPixMap,
        planes: &ScdPlanes<'_>,
        pStaticBlockIdc: &mut [u8],
    ) -> i32 {
        if planes.cur.is_empty() || planes.refp.is_empty() {
            return RET_INVALIDPARAM;
        }
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iBlock8x8Width = (iWidth >> 3).max(0) as usize;
        let iBlock8x8Height = (iHeight >> 3).max(0) as usize;

        if pStaticBlockIdc.len() < iBlock8x8Width * iBlock8x8Height {
            return RET_INVALIDPARAM;
        }

        let iBlock8x8Num = (iBlock8x8Width * iBlock8x8Height) as i32;
        let iSceneChangeThresholdLarge =
            (SCENE_CHANGE_MOTION_RATIO_LARGE_SCREEN * iBlock8x8Num as f32 + 0.5 + PESN) as i32;
        let iSceneChangeThresholdMedium =
            (SCENE_CHANGE_MOTION_RATIO_MEDIUM * iBlock8x8Num as f32 + 0.5 + PESN) as i32;

        self.m_sSceneChangeParam.iMotionBlockNum = 0;
        self.m_sSceneChangeParam.iFrameComplexity = 0;
        self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::SIMILAR_SCENE;

        // CSceneChangeDetectorScreen::operator() — SceneChangeDetection.h:152-191.
        // The three scroll fields are read out of the parameter block once, as the
        // C++'s three locals at `:153-155` are.
        let bScrollDetectFlag = self.m_sSceneChangeParam.sScrollResult.bScrollDetectFlag;
        let iScrollMvX = self.m_sSceneChangeParam.sScrollResult.iScrollMvX;
        let iScrollMvY = self.m_sSceneChangeParam.sScrollResult.iScrollMvY;

        for j in 0..iBlock8x8Height {
            for i in 0..iBlock8x8Width {
                let iBlockPointX = (i << 3) as i32;
                let iBlockPointY = (j << 3) as i32;
                let mut uiBlockIdcTmp = EStaticBlockIdc::NO_STATIC;
                let cur = PlaneCursor::new(
                    planes.cur,
                    j * 8 * planes.cur_stride + i * 8,
                    planes.cur_stride,
                );
                let refp = PlaneCursor::new(
                    planes.refp,
                    j * 8 * planes.ref_stride + i * 8,
                    planes.ref_stride,
                );
                let iSad = sample_sad_8x8(&cur, &refp);
                if iSad == 0 {
                    uiBlockIdcTmp = EStaticBlockIdc::COLLOCATED_STATIC;
                } else if bScrollDetectFlag
                    && (iScrollMvX == 0 || iScrollMvY == 0)
                    && (iBlockPointX + iScrollMvX >= 0)
                    && (iBlockPointX + iScrollMvX <= iWidth - 8)
                    && (iBlockPointY + iScrollMvY >= 0)
                    && (iBlockPointY + iScrollMvY <= iHeight - 8)
                {
                    // `pRefTmp + iScrollMvY * iRefStride + iScrollMvX` (`:170`) — plus
                    // on both axes. The complexity plugin's inter kernel subtracts on
                    // Y for the same vector; both are upstream's, and they differ.
                    //
                    // **Signed throughout, because the vector is.** A downward scroll
                    // gives a negative `iScrollMvY`, and folding that into the
                    // `usize` anchor a component at a time would wrap — silently in
                    // release, and as an overflow panic in debug. The four bounds
                    // tests above guarantee the *sum* lands inside the plane, which is
                    // exactly what `try_from` checks here.
                    let iRefScrollOff = (j * 8 * planes.ref_stride + i * 8) as isize
                        + iScrollMvY as isize * planes.ref_stride as isize
                        + iScrollMvX as isize;
                    let refp_scroll = PlaneCursor::new(
                        planes.refp,
                        usize::try_from(iRefScrollOff)
                            .expect("the scroll bounds test admits only in-plane blocks"),
                        planes.ref_stride,
                    );
                    let iSadScroll = sample_sad_8x8(&cur, &refp_scroll);
                    if iSadScroll == 0 {
                        uiBlockIdcTmp = EStaticBlockIdc::SCROLLED_STATIC;
                    } else {
                        self.m_sSceneChangeParam.iFrameComplexity += iSad as i64;
                        self.m_sSceneChangeParam.iMotionBlockNum +=
                            (iSad > HIGH_MOTION_BLOCK_THRESHOLD) as i32;
                    }
                } else {
                    self.m_sSceneChangeParam.iFrameComplexity += iSad as i64;
                    self.m_sSceneChangeParam.iMotionBlockNum +=
                        (iSad > HIGH_MOTION_BLOCK_THRESHOLD) as i32;
                }
                // `*(pStaticBlockIdc)++ = uiBlockIdcTmp` — the C++ walks the row with a
                // post-increment, which is raster order over the block grid.
                pStaticBlockIdc[j * iBlock8x8Width + i] = uiBlockIdcTmp as u8;
            }
        }

        if self.m_sSceneChangeParam.iMotionBlockNum >= iSceneChangeThresholdLarge {
            self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::LARGE_CHANGED_SCENE;
        } else if self.m_sSceneChangeParam.iMotionBlockNum >= iSceneChangeThresholdMedium {
            self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::MEDIUM_CHANGED_SCENE;
        }

        RET_SUCCESS
    }
}

#[cfg(test)]
mod screen_tests {
    use super::*;
    use crate::encoder::wels_preprocess::SScrollDetectionParam;

    fn pixmap(w: i32, h: i32) -> SPixMap {
        let mut m = SPixMap::default();
        m.iStride[0] = w;
        m.sRect.iRectWidth = w;
        m.sRect.iRectHeight = h;
        m
    }

    /// A deterministic textured frame — enough per-block variety that no two 8x8
    /// blocks are accidentally equal.
    fn noise(w: usize, h: usize, seed: u32) -> Vec<u8> {
        let mut state = seed | 1;
        (0..w * h)
            .map(|_| {
                state = state.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff;
                (state >> 16) as u8
            })
            .collect()
    }

    fn run(
        cur: &[u8],
        refp: &[u8],
        w: i32,
        h: i32,
        scroll: SScrollDetectionParam,
        idc: &mut [u8],
    ) -> SSceneChangeResult {
        let mut d = CSceneChangeDetectionScreen::default();
        let mut p = SSceneChangeResult::default();
        p.sScrollResult = scroll;
        d.Set(&p);
        let planes = ScdPlanes {
            cur,
            cur_stride: w as usize,
            refp,
            ref_stride: w as usize,
        };
        assert_eq!(d.Process(&pixmap(w, h), &planes, idc), RET_SUCCESS);
        let mut out = SSceneChangeResult::default();
        d.Get(&mut out);
        out
    }

    /// Identical frames: every block's SAD is 0, so every block is
    /// `COLLOCATED_STATIC`, nothing accumulates, and the verdict is `SIMILAR_SCENE`.
    #[test]
    fn identical_frames_are_all_collocated_static() {
        const W: usize = 64;
        const H: usize = 32;
        let f = noise(W, H, 9);
        let mut idc = vec![0xEEu8; (W / 8) * (H / 8)];
        let r = run(&f, &f, W as i32, H as i32, SScrollDetectionParam::default(), &mut idc);
        assert_eq!(r.iMotionBlockNum, 0);
        assert_eq!(r.iFrameComplexity, 0);
        assert_eq!(r.eSceneChangeIdc, ESceneChangeIdc::SIMILAR_SCENE);
        assert!(
            idc.iter().all(|&v| v == EStaticBlockIdc::COLLOCATED_STATIC as u8),
            "every block collocated-static: {idc:?}"
        );
    }

    /// A frame scrolled up by eight rows, with the vector the scroll detector would
    /// report for it (`+8`, "previous minus current"). Every block whose scrolled
    /// source is inside the picture matches exactly and is `SCROLLED_STATIC`; the
    /// bottom row of blocks, whose scrolled source would start at `iHeight`, fails
    /// the `iBlockPointY + iScrollMvY <= iHeight - 8` test and falls to the
    /// accumulate arm — `NO_STATIC` unless it happens to be collocated-equal.
    ///
    /// This is the whole point of the plugin: the same content one block lower is
    /// *not* a scene change.
    #[test]
    fn a_scrolled_frame_is_scrolled_static_except_at_the_bottom_edge() {
        const W: usize = 64;
        const H: usize = 32;
        const BW: usize = W / 8;
        const BH: usize = H / 8;
        let tall = noise(W, H + 8, 21);
        let refp: Vec<u8> = tall[..W * H].to_vec();
        let cur: Vec<u8> = tall[8 * W..][..W * H].to_vec();

        let mut scroll = SScrollDetectionParam::default();
        scroll.bScrollDetectFlag = true;
        scroll.iScrollMvX = 0;
        scroll.iScrollMvY = 8;
        let mut idc = vec![0xEEu8; BW * BH];
        let r = run(&cur, &refp, W as i32, H as i32, scroll, &mut idc);

        for j in 0..BH - 1 {
            for i in 0..BW {
                assert_eq!(
                    idc[j * BW + i],
                    EStaticBlockIdc::SCROLLED_STATIC as u8,
                    "block ({i},{j}) should be scrolled-static"
                );
            }
        }
        // The bottom block row: `(BH-1)*8 + 8 = H` exceeds `H - 8`, so no scroll SAD
        // is taken and the block is measured collocated.
        for i in 0..BW {
            assert_ne!(
                idc[(BH - 1) * BW + i],
                EStaticBlockIdc::SCROLLED_STATIC as u8,
                "block ({i},{}) is outside the scroll bounds", BH - 1
            );
        }
        assert!(r.iFrameComplexity > 0, "the bottom row still accumulates");
        assert_eq!(r.iMotionBlockNum, BW as i32, "one moving block row of {BW}");
    }

    /// A **downward** scroll — a negative `iScrollMvY`. The anchor arithmetic is
    /// signed for this: folding a negative vector into the `usize` offset a component
    /// at a time wraps, which is an overflow panic in debug and a wrong read in
    /// release. Here the top block row is the one outside the bounds.
    #[test]
    fn a_negative_scroll_vector_reads_the_right_blocks() {
        const W: usize = 64;
        const H: usize = 32;
        const BW: usize = W / 8;
        const BH: usize = H / 8;
        let tall = noise(W, H + 8, 33);
        // cur is the *upper* window, ref the lower: content moved down by 8, so the
        // detector's vector is -8.
        let cur: Vec<u8> = tall[..W * H].to_vec();
        let refp: Vec<u8> = tall[8 * W..][..W * H].to_vec();

        let mut scroll = SScrollDetectionParam::default();
        scroll.bScrollDetectFlag = true;
        scroll.iScrollMvY = -8;
        let mut idc = vec![0xEEu8; BW * BH];
        let r = run(&cur, &refp, W as i32, H as i32, scroll, &mut idc);

        for i in 0..BW {
            assert_ne!(
                idc[i], EStaticBlockIdc::SCROLLED_STATIC as u8,
                "block ({i},0) is above the scroll bounds"
            );
        }
        for j in 1..BH {
            for i in 0..BW {
                assert_eq!(
                    idc[j * BW + i],
                    EStaticBlockIdc::SCROLLED_STATIC as u8,
                    "block ({i},{j}) should be scrolled-static"
                );
            }
        }
        assert_eq!(r.iMotionBlockNum, BW as i32);
    }

    /// Random against flat: every block moves, so `iMotionBlockNum` reaches the block
    /// count and the verdict clears the 0.80 threshold.
    #[test]
    fn an_unrelated_frame_is_a_large_changed_scene() {
        const W: usize = 64;
        const H: usize = 32;
        const N: usize = (W / 8) * (H / 8);
        let cur = noise(W, H, 5);
        let flat = vec![0u8; W * H];
        let mut idc = vec![0xEEu8; N];
        let r = run(&cur, &flat, W as i32, H as i32, SScrollDetectionParam::default(), &mut idc);
        assert_eq!(r.iMotionBlockNum, N as i32);
        assert_eq!(r.eSceneChangeIdc, ESceneChangeIdc::LARGE_CHANGED_SCENE);
        assert!(idc.iter().all(|&v| v == EStaticBlockIdc::NO_STATIC as u8));
    }

    /// A row one byte short of the block grid is `RET_INVALIDPARAM` and *nothing* is
    /// written — where the C++, handed the same short allocation, would walk off the
    /// end of it. The C++'s other undefined case, a `NULL` `pStaticBlockIdc`, is the
    /// caller's `None` and is refused there.
    #[test]
    fn a_short_block_static_row_is_refused_without_a_write() {
        const W: i32 = 64;
        const H: i32 = 32;
        let f = noise(W as usize, H as usize, 2);
        let mut idc = vec![0xEEu8; ((W / 8) * (H / 8)) as usize - 1];
        let mut d = CSceneChangeDetectionScreen::default();
        d.Set(&SSceneChangeResult::default());
        let planes = ScdPlanes {
            cur: &f,
            cur_stride: W as usize,
            refp: &f,
            ref_stride: W as usize,
        };
        assert_eq!(d.Process(&pixmap(W, H), &planes, &mut idc), RET_INVALIDPARAM);
        assert!(idc.iter().all(|&v| v == 0xEE), "nothing was written");
    }

    /// The screen threshold is 0.80 where the video detector's is 0.85, and the
    /// `+ 0.5 + PESN` cast is the same. With 32 blocks: large at
    /// `(int)(0.80 * 32 + 0.5 + 1e-6) = 26`, medium at `(int)(0.50 * 32 + ...) = 16`.
    #[test]
    fn the_screen_thresholds_are_zero_eight_and_zero_five() {
        const W: usize = 64;
        const H: usize = 32;
        const BW: usize = W / 8;
        const N: usize = BW * (H / 8);
        assert_eq!(N, 32);

        // `iMotionBlockNum` counts blocks whose SAD exceeds 320. Make exactly `k` of
        // them differ by a wide margin and the rest identical.
        let make = |k: usize| -> (Vec<u8>, Vec<u8>) {
            let base = noise(W, H, 77);
            let mut cur = base.clone();
            for b in 0..k {
                let (bi, bj) = (b % BW, b / BW);
                for y in 0..8 {
                    for x in 0..8 {
                        cur[(bj * 8 + y) * W + bi * 8 + x] ^= 0xFF;
                    }
                }
            }
            (cur, base)
        };

        for (k, want) in [
            (15, ESceneChangeIdc::SIMILAR_SCENE),
            (16, ESceneChangeIdc::MEDIUM_CHANGED_SCENE),
            (25, ESceneChangeIdc::MEDIUM_CHANGED_SCENE),
            (26, ESceneChangeIdc::LARGE_CHANGED_SCENE),
        ] {
            let (cur, refp) = make(k);
            let mut idc = vec![0u8; N];
            let r = run(&cur, &refp, W as i32, H as i32, SScrollDetectionParam::default(), &mut idc);
            assert_eq!(r.iMotionBlockNum, k as i32, "k={k}");
            assert_eq!(r.eSceneChangeIdc, want, "k={k}");
        }
    }
}
