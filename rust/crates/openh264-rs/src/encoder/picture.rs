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

#![deny(unsafe_code)]

use crate::encoder::encoder_context::{BLOCK_SIZE_ALL, SMVUnitXY};
pub use crate::safe::plane::PaddedPlane;

/// `PADDING_LENGTH` — `codec/encoder/core/inc/wels_const.h`. The luma border
/// `AllocPicture` puts around every picture; chroma gets half of it.
const PADDING_LENGTH: usize = 32;

/// `WELS_ALIGN(x, n)` for the two alignments this file needs, in `usize`.
#[inline]
const fn align_up(x: usize, n: usize) -> usize {
    (x + n - 1) & !(n - 1)
}

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
    /// The three planes, **owned** since T6.F2 — Y, Cb, Cr. They were
    /// `pBuffer: *mut u8` (one `CMemoryAlign` block), `pData: [*mut u8; 3]` (three
    /// cursors into it, each at the plane's padded origin) and `iLineSize: [i32; 3]`.
    ///
    /// Each [`PaddedPlane`] owns its bytes, so `FreePicture` and the whole
    /// allocator round trip go with them. The per-macroblock world still walks raw
    /// cursors: [`data_ptr`](Self::data_ptr) hands them out from the **allocation
    /// root** (S28), never through a narrowing slice, which is what makes reading
    /// back into the top and left borders defined.
    planes: [PaddedPlane; 3],

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

        // `picture_handle.cpp:60-74`'s geometry, unchanged — the alignment is
        // load-bearing (`pStrideDecBlockOffset` is built from the same strides) and
        // the port has been wrong here once already.
        let kuiAlignedWidth = align_up(kiWidth.max(0) as usize, 16);
        let kuiAlignedHeight = align_up(kiHeight.max(0) as usize, 16);
        let kuiLumaStride = align_up(kuiAlignedWidth + 2 * PADDING_LENGTH, 32);
        let kuiChromaStride = align_up((kuiAlignedWidth + 2 * PADDING_LENGTH) >> 1, 16);

        Box::new(SPicture {
            // Zeroed, and that is **F58** a fourth time: `AnalyzeSpatialPic` hands
            // `VaaCalculation` a reference picture nothing has written on the first
            // frame and `VAACalcSad` reads its visible luma. `PaddedPlane::new`
            // zeroes, which is what `WelsMallocz` gave and what makes the read
            // defined.
            planes: [
                PaddedPlane::new(
                    kuiAlignedWidth,
                    kuiAlignedHeight,
                    PADDING_LENGTH,
                    kuiLumaStride,
                ),
                PaddedPlane::new(
                    kuiAlignedWidth >> 1,
                    kuiAlignedHeight >> 1,
                    PADDING_LENGTH >> 1,
                    kuiChromaStride,
                ),
                PaddedPlane::new(
                    kuiAlignedWidth >> 1,
                    kuiAlignedHeight >> 1,
                    PADDING_LENGTH >> 1,
                    kuiChromaStride,
                ),
            ],

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

    /// Plane `i`'s **root-derived** cursor at its logical origin — the raw `pData[i]`
    /// every per-macroblock consumer still walks.
    ///
    /// **S28 verbatim, and the decoder paid for the lesson.** The obvious spelling
    /// `plane.as_mut_slice()[origin..].as_mut_ptr()` is safe code with the right
    /// address and Undefined Behaviour at the first read into the top or left border,
    /// because the slice index narrows provenance to `[origin..]` and the border is
    /// exactly what this pointer exists to reach — intra prediction reads
    /// `pRef[-iLineSize]` on the top macroblock row, and `ExpandReferencingPicture`
    /// writes the whole frame. Deriving from the allocation root and *offsetting*
    /// keeps the provenance of the whole plane. No byte-level gate can see the
    /// difference; the Miri test at the bottom of this file is the instrument.
    ///
    /// **And the root must be taken without slicing, which cost this session a
    /// second Miri failure.** `plane.as_mut_slice().as_mut_ptr()` has the right
    /// *provenance* — the whole allocation — but `&mut self.buf` is a `Unique` retag,
    /// so the **next** call on the same plane pops the pointer the previous one
    /// handed out. The encoder does exactly that within one frame:
    /// `WelsInitCurrentLayer` stamps `pEncData` from the source picture, and
    /// `AnalyzePictureComplexity` asks the same picture for its planes again a few
    /// hundred lines later, after which `WelsMdI16x16`'s SAD reads through the first
    /// cursor. [`PaddedPlane::root_ptr`] reads the address out of the `Vec` header
    /// instead, so repeated calls are siblings rather than a stack.
    #[inline]
    pub fn data_ptr(&mut self, i: usize) -> *mut u8 {
        let plane = &mut self.planes[i];
        if plane.is_empty() {
            return std::ptr::null_mut();
        }
        let origin = plane.origin();
        plane.root_ptr().wrapping_add(origin)
    }

    /// [`data_ptr`](Self::data_ptr) through `&self` — the **in-fork** form
    /// (Phase 9 E3, the `sRefPicView` harvest).
    ///
    /// The reference picture's plane origins used to be copied onto the layer
    /// once per frame (`SRefPicView`) exactly because resolving them per call
    /// needed `&mut SPicture` — a fresh whole-picture exclusive retag every
    /// worker would perform (F73's shape). With the root read through `&self`
    /// ([`PaddedPlane::root_ptr_shared`], F71) the per-call resolution is a
    /// shared read of a pre-fork-stamped pool picture, which is what every
    /// in-frame consumer of the reference actually does. Same address, same
    /// whole-plane provenance (S28), null when the plane is unallocated.
    #[inline]
    pub fn data_ptr_shared(&self, i: usize) -> *mut u8 {
        let plane = &self.planes[i];
        if plane.is_empty() {
            return std::ptr::null_mut();
        }
        let origin = plane.origin();
        plane.root_ptr_shared().wrapping_add(origin)
    }

    /// Plane `i`'s stride — the C++'s `iLineSize[i]`.
    #[inline]
    pub fn stride(&self, i: usize) -> i32 {
        self.planes[i].stride() as i32
    }

    /// Plane `i`, for a converted caller that wants the safe view.
    #[inline]
    pub fn plane(&self, i: usize) -> &PaddedPlane {
        &self.planes[i]
    }

    /// Mutable form of [`plane`](Self::plane).
    #[inline]
    pub fn plane_mut(&mut self, i: usize) -> &mut PaddedPlane {
        &mut self.planes[i]
    }

    /// All three planes mutably at once.
    ///
    /// [`plane_mut`](Self::plane_mut) borrows the whole picture, so a step that
    /// writes Y, U and V in one pass — `METHOD_DENOISE` and `METHOD_DOWNSAMPLE`,
    /// both of which take the three planes as slices rather than the `SPixMap` of
    /// raw pointers the C++ hands them — cannot hold three of them. Array
    /// destructuring can, and stays safe.
    #[inline]
    pub fn planes_mut3(&mut self) -> [&mut PaddedPlane; 3] {
        let [y, u, v] = &mut self.planes;
        [y, u, v]
    }

    /// `ExpandReferencingPicture` for a picture that owns its planes — **T6.F4**, and
    /// it is the sentence `decoder/picture.rs::expand_as_reference` left for this
    /// phase ("the encoder's two call sites keep the raw entry point: converting them
    /// is Phase 6's").
    ///
    /// `common::expand_pic::ExpandReferencingPicture` takes three raw plane origins
    /// and each of its two kernels then rebuilds the whole allocation out of the
    /// mid-plane pointer it was handed — `expand_shim_span`, the one place in the port
    /// that does that arithmetic, and the exact backward reach the S28 test below
    /// pins. A picture that owns its planes has nothing to rebuild:
    /// `plane_mut(i).as_mut_slice()` **is** the padded allocation, `origin()` is the
    /// `pad * stride + pad` the shim computed, and `expand_picture` — already safe,
    /// already the single copy of the C++ body — takes it directly.
    ///
    /// This runs once per plane per reference picture, so it is per-frame work and
    /// the bounds checks amortise over a whole plane (session E's rule for what may
    /// convert). The per-plane null guards the raw form carried are
    /// `plane(i).is_empty()`, which is what the null answered.
    pub fn expand_as_reference(&mut self) {
        let (kiWidthY, kiHeightY) = (self.iWidthInPixel, self.iHeightInPixel);
        let planes = [
            (0usize, kiWidthY, kiHeightY, PADDING_LENGTH),
            (1, kiWidthY >> 1, kiHeightY >> 1, PADDING_LENGTH >> 1),
            (2, kiWidthY >> 1, kiHeightY >> 1, PADDING_LENGTH >> 1),
        ];
        for (i, pic_w, pic_h, pad) in planes {
            let stride = self.stride(i) as usize;
            let plane = self.plane_mut(i);
            if plane.is_empty() || pic_w <= 0 || pic_h <= 0 {
                continue;
            }
            crate::common::expand_pic::expand_picture(
                plane.as_mut_slice(),
                stride,
                pic_w as usize,
                pic_h as usize,
                pad,
            );
        }
    }

    /// The picture's plane roots, strides and visible geometry, copied out.
    ///
    /// **S37's shape for a per-frame step**: a picture is an arena, so the
    /// preprocessing and analysis stages resolve it once, take this, and then work
    /// through raw cursors — rather than holding an `&SPicture` across the calls that
    /// resolve the *other* picture they need. Six fields by value, no borrow left
    /// behind, and both pictures of a read-one-write-another pair can be in hand at
    /// once without asking the pool for two references.
    #[inline]
    pub fn planes(&mut self) -> PicPlanes {
        PicPlanes {
            pData: [self.data_ptr(0), self.data_ptr(1), self.data_ptr(2)],
            iLineSize: [self.stride(0), self.stride(1), self.stride(2)],
            iWidthInPixel: self.iWidthInPixel,
            iHeightInPixel: self.iHeightInPixel,
        }
    }

    /// Set picture as unreferenced. Matches `SPicture::SetUnref()`, `picture.h:106`.
    ///
    /// # Safety
    /// `pScreenBlockFeatureStorage` must be null or point to a valid storage block.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
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

/// A picture's plane roots and geometry, copied out of it — see [`SPicture::planes`].
#[derive(Clone, Copy, Debug)]
pub struct PicPlanes {
    /// Null on a `Default` — "no picture bound", which is what the C++'s null
    /// `pRefPic` meant at the sites that read this.
    pub pData: [*mut u8; 3],
    pub iLineSize: [i32; 3],
    pub iWidthInPixel: i32,
    pub iHeightInPixel: i32,
}

impl Default for PicPlanes {
    /// "No picture bound" — three null roots and zero geometry, which is the state
    /// `SDqLayer`'s stamped views hold on an I-slice, where the C++ leaves `pRefPic`
    /// null and no reader reaches them.
    fn default() -> Self {
        Self {
            pData: [std::ptr::null_mut(); 3],
            iLineSize: [0; 3],
            iWidthInPixel: 0,
            iHeightInPixel: 0,
        }
    }
}

// `SRefPicView` and `SPicture::view()` stood here — T6.F5's per-frame raw copy
// of the reference/reconstruction picture's plane roots, strides, type and the
// dormant feature-storage pointer, stamped onto the layer because resolving
// them per call used to need `&mut SPicture` (a per-worker exclusive retag,
// F73). Phase 9 E3's harvest re-routed every reader through
// [`SPicture::data_ptr_shared`] / [`SPicture::stride`] / the fields themselves,
// resolved per call via `layer_ref_pic` — shared reads of a pre-fork-stamped
// pool picture — and both layer fields (`sRefPicView`, and `sDecPicView`, which
// had zero readers) died with the stamp. T6.F5's measured +1.45% was the cost
// of *five dependent loads through an Option chain per site*; the per-call
// route here is the same pool resolution every surviving reference read
// (`sMvList`, `uiRefMbType`, `pMbSkipSad`) has performed since T9.C3, so no new
// cost class is introduced. D-gate-1 defers the measurement to the phase close.

// ===========================================================================
// The two pools, and the two handle types that address them — T6.F1
// ===========================================================================

/// The encoder owns pictures in exactly **three** places, confirmed at their
/// allocation and free sites (Phase 6 session B's settlement):
///
/// * the **reconstruction pool**, one per dependency layer, in that layer's
///   `SRefList` — the pictures the encoder decodes into and then references;
/// * the **spatial source pool**, in `CWelsPreProcess` — the downsampled copies of
///   the caller's frames;
/// * **one scaled input picture**, a slot of its own in `Scaled_Picture`.
///
/// Every other place a picture appeared — fourteen fields across the context, the
/// reference lists, the layer and the preprocessor — was an *alias*: a `SPicture*`
/// answering "which picture", spelled as an address. Those are handles now, and the
/// handles are **two distinct types that do not convert to each other**, because
/// `pEncPic` (source) and `pDecPic`/`pRefPic` (reconstruction) meet in one
/// `WelsEncoderEncodeExt` iteration and in `UpdateOriginalPicInfo`: one shared type
/// would let either be passed where the other belongs, and nothing would say so.
///
/// A handle is `Copy`, names a slot rather than an address, and does not own — so
/// the recycling hazard the C++ has is *preserved* (a slot reused under an old
/// handle) rather than fixed, with a debug-build generation counter to catch it in
/// tests. See [`crate::safe::pool`].
///
/// **Scope of a [`RecPicId`]**: it names a slot in **one dependency layer's**
/// `SRefList`, not a global picture. Every consumer resolves it through the layer it
/// came from — `ppRefPicListExt[uiDependencyId]`, or `SDqLayer::pRefList`, which is
/// that same list. Nothing in the port carries a `RecPicId` across a layer switch;
/// `WelsEncoderEncodeExt` sets `pDecPic`/`pRefPic` from the current layer's list at
/// the top of each iteration and consumes them inside it.
/// A reference-picture handle that may name **either** pool.
///
/// One field in the encoder holds both kinds and the C++ cannot see it:
/// `SDqLayer::pRefOri`. `WelsBuildRefList` (camera) stores *reconstruction* pictures
/// there — `ref_list_mgr_svc.cpp:613`/`:626` assign `pRefList->pLongRefList[i]` and
/// `pShortRefList[i]` — while `WelsBuildRefListScreen` stores *spatial source*
/// pictures, taken from `m_pSpatialPic` through `GetRefFrameInfo`
/// (`wels_preprocess.cpp:1267`). Both are `SPicture*` in C++, so the disagreement is
/// invisible there; two handle types make it a type error, and this enum is the
/// answer — the field really does hold either, and which one depends on a usage-type
/// branch taken frames earlier.
///
/// Its readers are `JudgeStaticSkip` and `JudgeScrollSkip`, both screen-content
/// paths, so on the camera path the `Rec` writes are dead stores. They are
/// transcribed rather than dropped: that is a behaviour question, not a spelling one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PicRef {
    /// A slot of a dependency layer's reconstruction pool.
    Rec(RecPicId),
    /// A slot of the spatial source pool.
    Src(SrcPicId),
}

macro_rules! pic_pool {
    ($id:ident, $pool:ident, $what:literal) => {
        #[doc = concat!("A handle to a slot of the ", $what, " picture pool.")]
        ///
        /// See the module note on [`SrcPicId`]/[`RecPicId`]: the two handle types are
        /// deliberately unrelated, and there is no conversion in either direction.
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
        pub struct $id(crate::safe::pool::Id);

        #[doc = concat!("The ", $what, " picture pool: the owner of its slots.")]
        #[derive(Debug)]
        pub struct $pool(crate::safe::pool::Pool<Box<SPicture>>);

        impl $pool {
            /// Takes ownership of `slots`. The pool never grows: both C++ picture
            /// sets are sized once at initialisation and recycled thereafter.
            pub fn new(slots: Vec<Box<SPicture>>) -> Self {
                Self(crate::safe::pool::Pool::new(slots))
            }

            /// A pool with no slots — what a host holds before `RequestMemorySvc`
            /// runs and after the pictures are released.
            pub fn empty() -> Self {
                Self::new(Vec::new())
            }

            /// Number of slots.
            #[inline]
            pub fn len(&self) -> usize {
                self.0.len()
            }

            /// Whether the pool has no slots.
            #[inline]
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }

            /// A handle to slot `index`.
            ///
            /// # Panics
            /// If `index` is out of range.
            #[inline]
            pub fn at(&self, index: usize) -> $id {
                $id(self.0.id(index))
            }

            /// Handles to every slot, in order — the iteration the recycling
            /// predicates do.
            pub fn ids(&self) -> impl Iterator<Item = $id> + '_ {
                self.0.ids().map($id)
            }

            /// The picture `id` names.
            #[inline]
            pub fn get(&self, id: $id) -> &SPicture {
                self.0.get(id.0)
            }

            /// Mutable form of [`get`](Self::get).
            #[inline]
            pub fn get_mut(&mut self, id: $id) -> &mut SPicture {
                self.0.get_mut(id.0)
            }

            /// Two *different* pictures mutably at once — the read-one-write-another
            /// shape (downsampling, motion-compensated analysis).
            ///
            /// # Panics
            /// If `a == b`.
            #[inline]
            pub fn pair_mut(&mut self, a: $id, b: $id) -> (&mut SPicture, &mut SPicture) {
                let (x, y) = self.0.pair_mut(a.0, b.0);
                (&mut **x, &mut **y)
            }

        }
    };
}

pic_pool!(SrcPicId, SrcPicPool, "spatial source");
pic_pool!(RecPicId, RecPicPool, "reconstruction");

#[cfg(test)]
mod tests {
    use super::*;

    /// **S28's mandated test for [`SPicture::data_ptr`]** — Phase 6 session F, step 2.
    ///
    /// The rule's own sentence is "every such accessor gets a Miri test that reads the
    /// pointer's full legal reach in both directions", and the second direction is the
    /// one with teeth: the samples are inside the allocation either way, so a
    /// `data_ptr` that narrowed provenance to `[origin..]` would read the right bytes,
    /// pass all 369 sweep configurations and both benches, and be Undefined Behaviour
    /// at the first border read. **No byte-level gate this project owns can see the
    /// difference. The raw reads below are the instrument.**
    ///
    /// Both backward reaches the *encoder* performs are exercised: one sample
    /// diagonally behind the origin — intra prediction reading `pRef[-iLineSize - 1]`
    /// on the top-left macroblock, and any motion vector past the picture edge — and
    /// the whole `pad * stride + pad` walk back to the allocation base, which is what
    /// `ExpandReferencingPicture` does to every reconstruction picture, every frame.
    #[test]
    // unsafe-cat: cursor
    #[allow(unsafe_code)]
    fn data_ptr_reaches_the_padding_behind_the_logical_origin() {
        // 176x144 QCIF as `SPicture::new` lays it out.
        let mut pic = SPicture::new(176, 144, false);
        let pad = PADDING_LENGTH;
        let stride = pic.plane(0).stride();
        assert_eq!(
            stride,
            align_up(align_up(176, 16) + 2 * PADDING_LENGTH, 32),
            "the C's WELS_ALIGN(WELS_ALIGN(w,16) + 2*PADDING_LENGTH, 32)"
        );
        assert_eq!(
            pic.plane(0).origin(),
            (1 + stride) * pad,
            "the C's (1 + iLineSize[0]) * PADDING_LENGTH"
        );

        pic.plane_mut(0).set(0, 0, 0x5A);
        pic.plane_mut(0).set(-1, -1, 0xC3);
        pic.plane_mut(0).set(-(pad as isize), -(pad as isize), 0x7E);

        let base = pic.plane(0).as_slice().as_ptr();
        let len = pic.plane(0).as_slice().len();
        let origin = pic.plane(0).origin();

        let p = pic.data_ptr(0);
        assert_eq!(unsafe { p.offset_from(base) } as usize, origin);
        assert_eq!(unsafe { *p }, 0x5A);
        assert_eq!(
            unsafe { *p.sub(stride + 1) },
            0xC3,
            "one sample diagonally behind the origin — intra prediction's top-left read"
        );
        // `ExpandReferencingPicture`'s reach: the whole padded plane from `pData[i]`.
        let whole = unsafe { std::slice::from_raw_parts(p.sub(pad * stride + pad), len) };
        assert_eq!(whole[0], 0x7E, "the top-left corner of the padding");

        // And forward, to the last byte of the bottom-right padding.
        let tail = len - (pad * stride + pad) - 1;
        assert_eq!(unsafe { *p.add(tail) }, 0);

        // Chroma keeps half the padding and its own aligned stride.
        assert_eq!(
            pic.stride(1),
            align_up((align_up(176, 16) + 2 * PADDING_LENGTH) >> 1, 16) as i32
        );
        assert_eq!(pic.stride(1), pic.stride(2));
        assert_eq!(pic.plane(1).pad(), PADDING_LENGTH / 2);
    }

    /// **S28/S40's mandated test for [`SPicture::data_ptr_shared`]** (E3's
    /// harvest). Three properties, none visible to a byte gate:
    /// the shared mint reaches the padding behind the origin (S28 — provenance
    /// is the whole plane, not `[origin..]`); repeated mints are siblings, so
    /// an earlier pointer survives a later call (S40's two-calls test); and the
    /// harvest's actual shape — a pre-fork `data_ptr` stamp followed by shared
    /// per-call mints — leaves both usable, with the shared read seeing the
    /// exclusive write. Miri at the session close is the referee.
    #[test]
    // unsafe-cat: cursor
    #[allow(unsafe_code)]
    fn data_ptr_shared_reaches_the_padding_and_survives_sibling_mints() {
        let mut pic = SPicture::new(176, 144, false);
        let pad = PADDING_LENGTH;
        let stride = pic.plane(0).stride();

        pic.plane_mut(0).set(0, 0, 0x5A);
        pic.plane_mut(0).set(-1, -1, 0xC3);

        // The harvest's shape: one exclusive stamp first (WelsInitCurrentLayer's
        // pEncData/pCsData world), then shared per-call mints.
        let p_stamp = pic.data_ptr(0);
        let p1 = pic.data_ptr_shared(0);
        let p2 = pic.data_ptr_shared(0);
        assert_eq!(p1, p2);
        assert_eq!(p1, p_stamp, "the shared mint is the same origin address");

        assert_eq!(unsafe { *p1 }, 0x5A);
        assert_eq!(
            unsafe { *p1.sub(stride + 1) },
            0xC3,
            "one sample diagonally behind the origin — the S28 reach"
        );
        // Forward, to the last byte of the bottom-right padding.
        let len = pic.plane(0).as_slice().len();
        let tail = len - (pad * stride + pad) - 1;
        assert_eq!(unsafe { *p1.add(tail) }, 0);

        // S40: the first mint is used after the second call — and after an
        // exclusive write through the stamp, which the shared read observes.
        unsafe { *p_stamp = 0x11 };
        let _p3 = pic.data_ptr_shared(0);
        assert_eq!(unsafe { *p1 }, 0x11, "a later mint or write popped the first");
    }

    /// The four per-macroblock side arrays exist exactly when `bNeedMbInfo` says so,
    /// and are sized as `picture_handle.cpp:105` sizes them.
    #[test]
    fn side_arrays_follow_need_mb_info() {
        let with = SPicture::new(176, 144, true);
        let n = ((176 + 15) >> 4) * ((144 + 15) >> 4);
        assert_eq!(with.uiRefMbType.len(), n as usize);
        assert_eq!(with.pRefMbQp.len(), n as usize);
        assert_eq!(with.pMbSkipSad.len(), n as usize);
        assert_eq!(with.sMvList.len(), n as usize);

        let without = SPicture::new(176, 144, false);
        assert!(without.uiRefMbType.is_empty());
        assert!(without.sMvList.is_empty());
    }

    /// **F56 at the constructor**: a fresh picture is `WelsMallocz`'s zeroed block
    /// plus `picture_handle.cpp`'s seven writes — *not* the unreferenced state.
    #[test]
    fn a_fresh_picture_is_not_an_unreferenced_one() {
        let pic = SPicture::new(176, 144, false);
        assert_eq!(pic.iFramePoc, 0);
        assert_eq!(pic.uiTemporalId, 0);
        assert_eq!(pic.uiSpatialId, 0);
        assert_eq!(pic.uiRecieveConfirmed, RECIEVE_UNKOWN);
        // and the seven the C++ writes
        assert_eq!(pic.iWidthInPixel, 176);
        assert_eq!(pic.iHeightInPixel, 144);
        assert_eq!(pic.iFrameNum, -1);
        assert!(!pic.bIsLongRef);
        assert_eq!(pic.iLongTermPicNum, -1);
        assert_eq!(pic.iMarkFrameNum, -1);
    }
}
