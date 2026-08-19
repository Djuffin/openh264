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

use crate::common::sad_common::WelsSampleSad8x8_c;
use crate::encoder::wels_preprocess::{ESceneChangeIdc, SPixMap, SSceneChangeResult};

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
    /// # Safety
    /// Both pixel maps must describe readable luma planes of the stated size.
    pub unsafe fn Process(&mut self, pSrcPixMap: &SPixMap, pRefPixMap: &SPixMap) -> i32 {
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iBlock8x8Width = iWidth >> 3;
        let iBlock8x8Height = iHeight >> 3;
        let pRefY = pRefPixMap.pPixel[0];
        let pCurY = pSrcPixMap.pPixel[0];
        let iRefStride = (*pRefPixMap).iStride[0];
        let iCurStride = (*pSrcPixMap).iStride[0];

        let iBlock8x8Num = iBlock8x8Width * iBlock8x8Height;
        let iSceneChangeThresholdLarge =
            (SCENE_CHANGE_MOTION_RATIO_LARGE_VIDEO * iBlock8x8Num as f32 + 0.5 + PESN) as i32;
        let iSceneChangeThresholdMedium =
            (SCENE_CHANGE_MOTION_RATIO_MEDIUM * iBlock8x8Num as f32 + 0.5 + PESN) as i32;

        self.m_sSceneChangeParam.iMotionBlockNum = 0;
        self.m_sSceneChangeParam.iFrameComplexity = 0;
        self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::SIMILAR_SCENE;

        // CSceneChangeDetectorVideo::operator() — SceneChangeDetection.h:113.
        let iRefRowStride = (iRefStride << 3) as isize;
        let iCurRowStride = (iCurStride << 3) as isize;
        let mut pRefRow = pRefY;
        let mut pCurRow = pCurY;
        for _j in 0..iBlock8x8Height {
            let mut pRefTmp = pRefRow;
            let mut pCurTmp = pCurRow;
            for _i in 0..iBlock8x8Width {
                let iSad = WelsSampleSad8x8_c(pCurTmp, iCurStride, pRefTmp, iRefStride);
                self.m_sSceneChangeParam.iMotionBlockNum +=
                    (iSad > HIGH_MOTION_BLOCK_THRESHOLD) as i32;
                pRefTmp = pRefTmp.offset(8);
                pCurTmp = pCurTmp.offset(8);
            }
            pRefRow = pRefRow.offset(iRefRowStride);
            pCurRow = pCurRow.offset(iCurRowStride);
        }

        if self.m_sSceneChangeParam.iMotionBlockNum >= iSceneChangeThresholdLarge {
            self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::LARGE_CHANGED_SCENE;
        } else if self.m_sSceneChangeParam.iMotionBlockNum >= iSceneChangeThresholdMedium {
            self.m_sSceneChangeParam.eSceneChangeIdc = ESceneChangeIdc::MEDIUM_CHANGED_SCENE;
        }

        RET_SUCCESS
    }
}
