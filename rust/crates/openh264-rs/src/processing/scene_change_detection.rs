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
//! Only `CSceneChangeDetectorVideo` is ported; `CSceneChangeDetectorScreen`
//! belongs to `METHOD_SCENE_CHANGE_DETECTION_SCREEN`, which stays unsupported —
//! it also needs the scroll detector, which is unported.

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use crate::common::sad_common::sample_sad;
use crate::encoder::wels_preprocess::{ESceneChangeIdc, SPixMap, SSceneChangeResult};
use crate::safe::plane::PlaneCursor;

/// The two luma planes this detector walks, routed from the pool pictures that own
/// them — **S37: built per call, never stored.** `DenoisePlanes` is the same shape one
/// plugin over (`processing/denoise.rs:220`); this one is read-only and luma-only,
/// which is all `CSceneChangeDetectorVideo` ever touches.
///
/// Each slice starts at its plane's logical origin and runs to the end of the padded
/// allocation, so a block at `(x, y)` is at byte `y * stride + x`.
pub struct ScdPlanes<'a> {
    pub cur: &'a [u8],
    pub cur_stride: usize,
    pub refp: &'a [u8],
    pub ref_stride: usize,
}

// `sad_8x8_raw` stood here — **F151 CLOSED, T9.X.** Session F relocated it into this
// file because the block walk below read `SPixMap.pPixel` raw and so could not take a
// `PlaneCursor` "until the preprocess family's own session converts the pixmap". This
// is that session. The walk takes [`ScdPlanes`] now and the kernel is
// `common::sad_common::sample_sad::<8, 8>`, which is the same summation over the same
// 64 `|a - b|` terms with the same `i32` accumulator.
//
// F151's ratchet rebaseline for this file (+2 raw_ptr, +3 unsafe_block, +1 unsafe_fn)
// comes back out with it.

use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};

/// `SceneChangeDetection.h:52-55`.
const HIGH_MOTION_BLOCK_THRESHOLD: i32 = 320;
const SCENE_CHANGE_MOTION_RATIO_LARGE_VIDEO: f32 = 0.85;
const SCENE_CHANGE_MOTION_RATIO_MEDIUM: f32 = 0.50;

/// `PESN` — `util.h:60`.
const PESN: f32 = 1e-6;

/// `CSceneChangeDetection<CSceneChangeDetectorVideo>` — `SceneChangeDetection.h:204`.
#[derive(Default)]
pub struct CSceneChangeDetection {
    pub m_sSceneChangeParam: SSceneChangeResult,
}

impl CSceneChangeDetection {
    /// `CSceneChangeDetection::Set`. Typed since Phase 6 session B (the `IWelsVP`
    /// vtable's `void*` is gone).
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
    /// **T9.X — safe.** `pSrcPixMap` still carries the geometry (it is the VP's own
    /// parameter block); the pixels arrive as [`ScdPlanes`], and the block walk is
    /// slice indexing over two [`PlaneCursor`]s. `denoise::Denoise` has taken its
    /// pixels this way since it was ported.
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
                let iSad = sample_sad::<8, 8>(&cur, &refp);
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
