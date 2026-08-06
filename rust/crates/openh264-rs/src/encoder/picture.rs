#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! Encoder picture buffers and reference-picture state.
//!
//! Translated from `codec/encoder/core/inc/picture.h`. This is the single definition
//! of `SPicture` and `SScreenBlockFeatureStorage`; before this module the port had six
//! copies of `SPicture` and two of `SScreenBlockFeatureStorage`, most of them truncated.

use crate::encoder::encoder_context::{BLOCK_SIZE_ALL, SMVUnitXY};

/// `LTR_MARKING_RECEIVE_STATE` — `codec/encoder/core/inc/wels_const.h:150`.
pub const RECIEVE_UNKOWN: u8 = 0;
pub const RECIEVE_SUCCESS: u8 = 1;
pub const RECIEVE_FAILED: u8 = 2;

/// `LIST_SIZE` — `picture.h:42`, `(256*256)`.
pub const LIST_SIZE: usize = 0x10000;

/// `SScreenBlockFeatureStorage` — `codec/encoder/core/inc/picture.h:43`.
/// Stored with a reference picture, one per frame.
#[repr(C)]
#[derive(Debug)]
pub struct SScreenBlockFeatureStorage {
    pub pFeatureOfBlockPointer: *mut u16,
    pub iIs16x16: i32,
    pub uiFeatureStrategyIndex: u8,
    pub pTimesOfFeatureValue: *mut u32,
    pub pLocationOfFeature: *mut *mut u16,
    pub pLocationPointer: *mut u16,
    pub iActualListSize: i32,
    pub uiSadCostThreshold: [u32; BLOCK_SIZE_ALL],
    pub bRefBlockFeatureCalculated: bool,
    pub pFeatureValuePointerList: *mut *mut u16,
}

impl Default for SScreenBlockFeatureStorage {
    fn default() -> Self {
        Self {
            pFeatureOfBlockPointer: std::ptr::null_mut(),
            iIs16x16: 0,
            uiFeatureStrategyIndex: 0,
            pTimesOfFeatureValue: std::ptr::null_mut(),
            pLocationOfFeature: std::ptr::null_mut(),
            pLocationPointer: std::ptr::null_mut(),
            iActualListSize: 0,
            uiSadCostThreshold: [u32::MAX; BLOCK_SIZE_ALL],
            bRefBlockFeatureCalculated: false,
            pFeatureValuePointerList: std::ptr::null_mut(),
        }
    }
}

/// `SPicture` — `codec/encoder/core/inc/picture.h:64`.
///
/// Field order and widths are load-bearing (`#[repr(C)]`). Note `pData`/`iLineSize`
/// are **3** elements, not 4 — the copy that used to live in `encoder_context.rs` had
/// them as 4 and also carried an invented `iPOC` field with no C++ counterpart.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPicture {
    // payload data
    pub pBuffer: *mut u8,
    pub pData: [*mut u8; 3],
    pub iLineSize: [i32; 3],

    // picture information, from pSps
    pub iWidthInPixel: i32,
    pub iHeightInPixel: i32,
    pub iPictureType: i32,
    pub iFramePoc: i32,

    pub fFrameRate: f32,
    pub iFrameNum: i32,

    pub uiRefMbType: *mut u32,
    pub pRefMbQp: *mut u8,
    pub pMbSkipSad: *mut i32,

    pub sMvList: *mut SMVUnitXY,

    // self-definition for misc use
    pub iMarkFrameNum: i32,
    pub iLongTermPicNum: i32,

    pub bUsedAsRef: bool,
    pub bIsLongRef: bool,
    pub bIsSceneLTR: bool,
    pub uiRecieveConfirmed: u8,
    pub uiTemporalId: u8,
    pub uiSpatialId: u8,
    pub iFrameAverageQp: i32,

    // for screen reference frames
    pub pScreenBlockFeatureStorage: *mut SScreenBlockFeatureStorage,
}

impl SPicture {
    /// Set picture as unreferenced. Matches `SPicture::SetUnref()`, `picture.h:106`.
    ///
    /// # Safety
    /// `pScreenBlockFeatureStorage` must be null or point to a valid storage block.
    pub unsafe fn SetUnref(&mut self) {
        self.iFramePoc = -1;
        self.iFrameNum = -1;
        self.uiTemporalId = 255;
        self.uiSpatialId = 255;
        self.iLongTermPicNum = -1;
        self.bIsLongRef = false;
        self.uiRecieveConfirmed = RECIEVE_FAILED;
        self.iMarkFrameNum = -1;
        self.bUsedAsRef = false;

        if !self.pScreenBlockFeatureStorage.is_null() {
            (*self.pScreenBlockFeatureStorage).bRefBlockFeatureCalculated = false;
        }
    }
}

impl Default for SPicture {
    fn default() -> Self {
        Self {
            pBuffer: std::ptr::null_mut(),
            pData: [std::ptr::null_mut(); 3],
            iLineSize: [0; 3],
            iWidthInPixel: 0,
            iHeightInPixel: 0,
            iPictureType: 0,
            iFramePoc: -1,
            fFrameRate: 0.0,
            iFrameNum: -1,
            uiRefMbType: std::ptr::null_mut(),
            pRefMbQp: std::ptr::null_mut(),
            pMbSkipSad: std::ptr::null_mut(),
            sMvList: std::ptr::null_mut(),
            iMarkFrameNum: -1,
            iLongTermPicNum: -1,
            bUsedAsRef: false,
            bIsLongRef: false,
            bIsSceneLTR: false,
            uiRecieveConfirmed: RECIEVE_FAILED,
            uiTemporalId: 255,
            uiSpatialId: 255,
            iFrameAverageQp: 0,
            pScreenBlockFeatureStorage: std::ptr::null_mut(),
        }
    }
}
