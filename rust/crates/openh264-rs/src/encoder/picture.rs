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
// SCREEN_CONTENT(dormant: Phase 10)
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
/// **Not `#[repr(C)]` since T6.F0, and not `Copy`.** The four per-macroblock side
/// arrays are owned `Vec`s now, so there is no C++ layout left to assert against and
/// the size pin below is the port's own number. Note `pData`/`iLineSize` are **3**
/// elements, not 4 — the copy that used to live in `encoder_context.rs` had them as 4
/// and also carried an invented `iPOC` field with no C++ counterpart.
///
/// `Copy` was dropped with the `Vec`s and nothing named a by-value copy: the two
/// pools swap *slots*, never values (S34, measured in session B).
#[derive(Debug)]
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

    /// The four per-macroblock side arrays, `kuiCountMbNum` entries each, or **empty**
    /// where `AllocPicture`'s `bNeedMbInfo` was false — the spatial-source and scaled
    /// pictures never carry them, and `is_empty()` is the port's spelling of the null
    /// the C++ leaves there (`picture_handle.cpp:104`).
    pub uiRefMbType: Vec<u32>,
    pub pRefMbQp: Vec<u8>,
    pub pMbSkipSad: Vec<i32>,

    pub sMvList: Vec<SMVUnitXY>,

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
    // SCREEN_CONTENT(dormant: Phase 10)
    pub pScreenBlockFeatureStorage: *mut SScreenBlockFeatureStorage,
}

impl SPicture {
    /// `picture_handle.cpp:51`, everything that is not the plane allocator.
    ///
    /// Builds the picture whole — every field written, none inherited from a zeroed
    /// block (**S21**: a `Vec` field in a `WelsMallocz`'d shell is UB at its first
    /// drop, so this type stops being allocatable that way the moment it owns). The
    /// planes are still the caller's to fill: [`AllocPicture`] takes `pBuffer` from
    /// `CMemoryAlign` and sets `pData`/`iLineSize` immediately after this returns.
    ///
    /// `bNeedMbInfo` decides whether the four side arrays exist at all. The C++
    /// leaves them null when it is false (`picture_handle.cpp:104`); here they are
    /// empty `Vec`s, and every consumer that tested for null tests `is_empty()`.
    ///
    /// **F56, and it bit on the first draft.** The C++ takes this struct from
    /// `WelsMallocz` (`picture_handle.cpp:57`) and then writes *seven* fields; every
    /// other field's value is the zeroed block's. So a fresh picture has
    /// `iFramePoc == 0` and `uiTemporalId == uiSpatialId == 0` — **not** the `-1`/`255`
    /// that [`SetUnref`](Self::SetUnref) leaves behind, which is what a `Default`
    /// spelled from the unref state would have handed back. The two states are
    /// different and only one of them is what `AllocPicture` produces.
    pub fn new(kiWidth: i32, kiHeight: i32, bNeedMbInfo: bool) -> Box<SPicture> {
        let kuiCountMbNum = if bNeedMbInfo {
            (((15 + kiWidth) >> 4) * ((15 + kiHeight) >> 4)).max(0) as usize
        } else {
            0
        };

        Box::new(SPicture {
            pBuffer: std::ptr::null_mut(),
            pData: [std::ptr::null_mut(); 3],
            iLineSize: [0; 3],

            iWidthInPixel: kiWidth,
            iHeightInPixel: kiHeight,
            iPictureType: 0,
            // zeroed, not -1: `AllocPicture` never writes `iFramePoc`.
            iFramePoc: 0,

            fFrameRate: 0.0,
            // `picture_handle.cpp:99`.
            iFrameNum: -1,

            uiRefMbType: vec![0u32; kuiCountMbNum],
            pRefMbQp: vec![0u8; kuiCountMbNum],
            pMbSkipSad: vec![0i32; kuiCountMbNum],
            sMvList: vec![SMVUnitXY { iMvX: 0, iMvY: 0 }; kuiCountMbNum],

            iMarkFrameNum: -1,
            iLongTermPicNum: -1,

            bUsedAsRef: false,
            bIsLongRef: false,
            bIsSceneLTR: false,
            uiRecieveConfirmed: RECIEVE_UNKOWN,
            // zeroed, not 255: `AllocPicture` never writes either id.
            uiTemporalId: 0,
            uiSpatialId: 0,
            iFrameAverageQp: 0,

            // SCREEN_CONTENT(dormant: Phase 10)
            pScreenBlockFeatureStorage: std::ptr::null_mut(),
        })
    }

    /// `uiRefMbType` as a raw pointer to its **root**, or null where the picture
    /// carries no macroblock info.
    ///
    /// **S28 verbatim**: the address is the `Vec`'s own root, never an index into it,
    /// so the pointer's provenance covers the whole array. It exists for one consumer
    /// — `SComplexityAnalysisParam.uiRefMbType`, a `processing/` field that is still
    /// C-shaped (step 3's) and whose reader tests it for null (`AnalyzePictureComplexity`
    /// may run with no usable reference). `is_empty()` is that null.
    #[inline]
    pub fn ref_mb_type_root(&mut self) -> *mut u32 {
        if self.uiRefMbType.is_empty() {
            std::ptr::null_mut()
        } else {
            self.uiRefMbType.as_mut_ptr()
        }
    }

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

        // SCREEN_CONTENT(dormant: Phase 10)
        if !self.pScreenBlockFeatureStorage.is_null() {
            (*self.pScreenBlockFeatureStorage).bRefBlockFeatureCalculated = false;
        }
    }
}

// **`impl Default for SPicture` stood here, and it was a trap (T6.F0).** It spelled
// the *unreferenced* state — `iFramePoc: -1`, `uiTemporalId: 255`, `uiSpatialId: 255`,
// `uiRecieveConfirmed: RECIEVE_FAILED` — which is what [`SPicture::SetUnref`] leaves
// behind and **not** what `AllocPicture` produces (a `WelsMallocz`'d block plus seven
// writes: those four read `0`, `0`, `0`, `RECIEVE_UNKOWN`). No encoder site ever built
// it, so the disagreement was never observable; with the pool about to want a fresh
// picture it would have become observable at exactly the wrong moment. F56's rule is
// that a zero is ruled rather than defaulted, and the ruling here is that `SPicture`
// has one constructor, [`SPicture::new`], and no default.
