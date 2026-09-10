#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

//! Encoder picture buffers and reference-picture state —
//! `codec/encoder/core/inc/picture.h`.

#![deny(unsafe_code)]

use crate::encoder::encoder_context::{BLOCK_SIZE_ALL, SMVUnitXY};
use crate::encoder::svc_motion_estimate::{
    LIST_SIZE_MSE_16x16, LIST_SIZE_SUM_8x8, LIST_SIZE_SUM_16x16,
};
pub use crate::safe::plane::PaddedPlane;
use crate::safe::pool::{Id, Pool};

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
///
/// `iActualListSize` bounds the first two tables; the cursor table is
/// `WELS_MAX (LIST_SIZE_SUM_16x16, LIST_SIZE_MSE_16x16)` long, **not**
/// `iActualListSize`.
#[derive(Debug)]
pub struct SScreenBlockFeatureStorage {
    pub iIs16x16: i32,
    pub uiFeatureStrategyIndex: u8,
    /// Histogram: how many block positions carry each feature value. `iActualListSize`
    /// entries.
    pub pTimesOfFeatureValue: Vec<u32>,
    /// Per feature value, the **start offset** of its group in `pLocationPointer`.
    /// `iActualListSize` entries.
    pub pLocationOfFeature: Vec<usize>,
    /// The arena of (x, y) qpel position pairs, grouped by feature value.
    pub pLocationPointer: Vec<u16>,
    pub iActualListSize: i32,
    pub uiSadCostThreshold: [u32; BLOCK_SIZE_ALL],
    pub bRefBlockFeatureCalculated: bool,
    /// Per feature value, the **roving write cursor** into `pLocationPointer` — starts
    /// at that value's base and advances by 2 per position written.
    pub pFeatureValuePointerList: Vec<usize>,
}

impl Default for SScreenBlockFeatureStorage {
    /// Every buffer empty and `uiSadCostThreshold` `UINT_MAX`-filled — not the zero a
    /// derived `Default` would give.
    fn default() -> Self {
        Self {
            iIs16x16: 0,
            uiFeatureStrategyIndex: 0,
            pTimesOfFeatureValue: Vec::new(),
            pLocationOfFeature: Vec::new(),
            pLocationPointer: Vec::new(),
            iActualListSize: 0,
            uiSadCostThreshold: [u32::MAX; BLOCK_SIZE_ALL],
            bRefBlockFeatureCalculated: false,
            pFeatureValuePointerList: Vec::new(),
        }
    }
}

impl SScreenBlockFeatureStorage {
    /// `svc_motion_estimate.cpp:690-721` as a constructor.
    ///
    /// The histogram and base table are `kiListSize` long, the arena is
    /// `2 * kiFrameSize`, and the cursor table is
    /// `WELS_MAX (LIST_SIZE_SUM_16x16, LIST_SIZE_MSE_16x16)` — not `kiListSize`.
    ///
    /// Called from `AllocPicture` for the last layer's reference pictures under
    /// `SCREEN_CONTENT_REAL_TIME`; `bIsBlock8x8` is `(kiMe8x8FME == ME_FME)`.
    pub fn for_frame(
        kiFrameWidth: i32,
        kiFrameHeight: i32,
        bIsBlock8x8: bool,
        kiFeatureStrategyIndex: u8,
    ) -> Self {
        let kiMarginSize = if bIsBlock8x8 { 8 } else { 16 };
        let kiFrameSize =
            ((kiFrameWidth - kiMarginSize).max(0) * (kiFrameHeight - kiMarginSize).max(0)) as usize;
        let kiListSize = if kiFeatureStrategyIndex == 0 {
            if bIsBlock8x8 {
                LIST_SIZE_SUM_8x8
            } else {
                LIST_SIZE_SUM_16x16
            }
        } else {
            256
        };
        Self {
            iIs16x16: i32::from(!bIsBlock8x8),
            uiFeatureStrategyIndex: kiFeatureStrategyIndex,
            pTimesOfFeatureValue: vec![0; kiListSize],
            pLocationOfFeature: vec![0; kiListSize],
            pLocationPointer: vec![0; 2 * kiFrameSize],
            iActualListSize: kiListSize as i32,
            uiSadCostThreshold: [u32::MAX; BLOCK_SIZE_ALL],
            bRefBlockFeatureCalculated: false,
            pFeatureValuePointerList: vec![0; LIST_SIZE_SUM_16x16.max(LIST_SIZE_MSE_16x16)],
        }
    }
}

/// `SPicture` — `codec/encoder/core/inc/picture.h:64`.
#[derive(Debug)]
pub struct SPicture {
    /// The three planes — Y, Cb, Cr. Each [`PaddedPlane`] owns its bytes.
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
    /// pictures never carry them.
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
    /// `Some` for the last layer's reference pictures under
    /// `SCREEN_CONTENT_REAL_TIME`, `None` for every other picture.
    pub pScreenBlockFeatureStorage: Option<Box<SScreenBlockFeatureStorage>>,
}

impl SPicture {
    /// `picture_handle.cpp:51`, everything that is not the plane allocator.
    ///
    /// `bNeedMbInfo` decides whether the four side arrays exist at all; when it is
    /// false they are empty `Vec`s, which every consumer tests with `is_empty()`.
    ///
    /// A fresh picture has `iFramePoc == 0` and `uiTemporalId == uiSpatialId == 0` —
    /// **not** the `-1`/`255` that [`SetUnref`](Self::SetUnref) leaves behind.
    pub fn new(kiWidth: i32, kiHeight: i32, bNeedMbInfo: bool) -> Box<SPicture> {
        let kuiCountMbNum = if bNeedMbInfo {
            (((15 + kiWidth) >> 4) * ((15 + kiHeight) >> 4)).max(0) as usize
        } else {
            0
        };

        // `picture_handle.cpp:60-74`'s geometry — the alignment is load-bearing
        // (`pStrideDecBlockOffset` is built from the same strides).
        let kuiAlignedWidth = align_up(kiWidth.max(0) as usize, 16);
        let kuiAlignedHeight = align_up(kiHeight.max(0) as usize, 16);
        let kuiLumaStride = align_up(kuiAlignedWidth + 2 * PADDING_LENGTH, 32);
        let kuiChromaStride = align_up((kuiAlignedWidth + 2 * PADDING_LENGTH) >> 1, 16);

        Box::new(SPicture {
            // `PaddedPlane::new` zeroes: on the first frame `VAACalcSad` reads the
            // visible luma of a reference picture nothing has written yet.
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

            // `AllocPicture` attaches the storage after this.
            pScreenBlockFeatureStorage: None,
        })
    }

    /// `uiRefMbType` as a raw pointer to its **root**, or null where the picture
    /// carries no macroblock info.
    ///
    /// The address is the `Vec`'s own root, never an index into it, so the pointer's
    /// provenance covers the whole array. Its one consumer,
    /// `SComplexityAnalysisParam.uiRefMbType`, tests it for null.
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
    /// The pointer is derived from the allocation root and then offset, so its
    /// provenance covers the whole plane including the top and left border — intra
    /// prediction reads `pRef[-iLineSize]` on the top macroblock row, and
    /// `ExpandReferencingPicture` writes the whole frame. Slicing `[origin..]` first
    /// would narrow provenance to that tail and make those reads Undefined Behaviour.
    ///
    /// The root itself is read out of the `Vec` header ([`PaddedPlane::root_ptr`])
    /// rather than through `&mut self.buf`, whose `Unique` retag would pop the pointer
    /// a previous call handed out; the encoder holds two such cursors into one plane
    /// within a frame, so repeated calls must be siblings rather than a stack.
    #[inline]
    pub fn data_ptr(&mut self, i: usize) -> *mut u8 {
        let plane = &mut self.planes[i];
        if plane.is_empty() {
            return std::ptr::null_mut();
        }
        let origin = plane.origin();
        plane.root_ptr().wrapping_add(origin)
    }

    /// [`data_ptr`](Self::data_ptr) through `&self` ([`PaddedPlane::root_ptr_shared`]):
    /// same address, same whole-plane provenance, null when the plane is unallocated.
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

    /// Plane `i`'s samples from the logical origin, as the borrow they are —
    /// [`data_ptr_shared`](Self::data_ptr_shared)'s reach as a slice. Empty where
    /// that answered null.
    #[inline]
    pub fn plane_tail(&self, i: usize) -> &[u8] {
        let plane = &self.planes[i];
        if plane.is_empty() {
            return &[];
        }
        let origin = plane.origin();
        &plane.as_slice()[origin..]
    }

    /// [`plane_tail`](Self::plane_tail)'s write half — [`data_ptr`](Self::data_ptr)'s
    /// reach as a slice.
    #[inline]
    pub fn plane_tail_mut(&mut self, i: usize) -> &mut [u8] {
        let plane = &mut self.planes[i];
        if plane.is_empty() {
            return &mut [];
        }
        let origin = plane.origin();
        &mut plane.as_mut_slice()[origin..]
    }

    /// Copy the top-left `kiWidth x kiHeight` samples of `kpSrc` into `self`,
    /// each plane at its own stride — the pool-to-pool half of `WelsMoveMemory_c`.
    ///
    /// Chroma takes half the geometry in each dimension (`iWidth >> 1`,
    /// `iHeight >> 1`); both pictures keep their own strides, so a copy between
    /// differently-padded allocations is the same walk.
    ///
    /// # Panics
    /// If either picture has a plane shorter than the geometry asks for.
    pub fn copy_planes_from(&mut self, kpSrc: &SPicture, kiWidth: i32, kiHeight: i32) {
        let (kuiW, kuiH) = (kiWidth.max(0) as usize, kiHeight.max(0) as usize);
        for i in 0..3 {
            let (kuiRow, kuiRows) = if i == 0 {
                (kuiW, kuiH)
            } else {
                (kuiW >> 1, kuiH >> 1)
            };
            if kuiRow == 0 || kuiRows == 0 {
                continue;
            }
            let kuiSrcStride = kpSrc.planes[i].stride();
            let kuiDstStride = self.planes[i].stride();
            let kpFrom = kpSrc.plane_tail(i);
            let pInto = self.plane_tail_mut(i);
            for y in 0..kuiRows {
                pInto[y * kuiDstStride..][..kuiRow]
                    .copy_from_slice(&kpFrom[y * kuiSrcStride..][..kuiRow]);
            }
        }
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

    /// All three planes mutably at once, for a step that writes Y, U and V in one
    /// pass (`METHOD_DENOISE`, `METHOD_DOWNSAMPLE`).
    #[inline]
    pub fn planes_mut3(&mut self) -> [&mut PaddedPlane; 3] {
        let [y, u, v] = &mut self.planes;
        [y, u, v]
    }

    /// `ExpandReferencingPicture` for a picture that owns its planes.
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
    /// Preprocessing and analysis resolve a picture once, take this, and then work
    /// through raw cursors instead of holding an `&SPicture` across the calls that
    /// resolve the other picture they need.
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
    pub fn SetUnref(&mut self) {
        self.iFramePoc = -1;
        self.iFrameNum = -1;
        self.uiTemporalId = 255;
        self.uiSpatialId = 255;
        self.iLongTermPicNum = -1;
        self.bIsLongRef = false;
        self.uiRecieveConfirmed = RECIEVE_FAILED;
        self.iMarkFrameNum = -1;
        self.bUsedAsRef = false;

        // picture_handle.cpp:245.
        if let Some(storage) = self.pScreenBlockFeatureStorage.as_deref_mut() {
            storage.bRefBlockFeatureCalculated = false;
        }
    }
}

/// A picture's plane roots and geometry, copied out of it — see [`SPicture::planes`].
#[derive(Clone, Copy, Debug)]
pub struct PicPlanes {
    /// Null on a `Default` — no picture bound.
    pub pData: [*mut u8; 3],
    pub iLineSize: [i32; 3],
    pub iWidthInPixel: i32,
    pub iHeightInPixel: i32,
}

impl Default for PicPlanes {
    /// No picture bound: three null roots and zero geometry — the state `SDqLayer`'s
    /// stamped views hold on an I-slice, where no reader reaches them.
    fn default() -> Self {
        Self {
            pData: [std::ptr::null_mut(); 3],
            iLineSize: [0; 3],
            iWidthInPixel: 0,
            iHeightInPixel: 0,
        }
    }
}

// ===========================================================================
// The two pools, and the two handle types that address them
// ===========================================================================

/// A reference-picture handle that may name **either** pool.
///
/// The encoder owns pictures in exactly **three** places:
///
/// * the **reconstruction pool**, one per dependency layer, in that layer's
///   `SRefList` — the pictures the encoder decodes into and then references;
/// * the **spatial source pool**, in `CWelsPreProcess` — the downsampled copies of
///   the caller's frames;
/// * **one scaled input picture**, a slot of its own in `Scaled_Picture`.
///
/// [`SrcPicId`] and [`RecPicId`] are distinct types with no conversion in either
/// direction, because `pEncPic` (source) and `pDecPic`/`pRefPic` (reconstruction)
/// meet in one `WelsEncoderEncodeExt` iteration and in `UpdateOriginalPicInfo`.
///
/// A handle is `Copy`, names a slot rather than an address, and does not own, so a
/// slot recycled under an old handle is still a hazard; a debug-build generation
/// counter catches it. See [`crate::safe::pool`].
///
/// A [`RecPicId`] names a slot in **one dependency layer's** `SRefList`, not a global
/// picture, and every consumer resolves it through the layer it came from —
/// `ppRefPicListExt[uiDependencyId]`, or `SDqLayer::pRefList`, which is that same
/// list. No `RecPicId` crosses a layer switch.
///
/// `SDqLayer::pRefOri` is the one field that holds both kinds: `WelsBuildRefList`
/// (camera) stores *reconstruction* pictures there, `WelsBuildRefListScreen` stores
/// *spatial source* pictures, and which it is depends on a usage-type branch taken
/// frames earlier. Its readers are `JudgeStaticSkip` and `JudgeScrollSkip`, both
/// screen-content paths, so on the camera path the `Rec` writes are dead stores.
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
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
        pub struct $id(Id);

        #[doc = concat!("The ", $what, " picture pool: the owner of its slots.")]
        #[derive(Debug)]
        pub struct $pool(Pool<Box<SPicture>>);

        impl $pool {
            /// Takes ownership of `slots`. The pool never grows: both picture sets
            /// are sized once at initialisation and recycled thereafter.
            pub fn new(slots: Vec<Box<SPicture>>) -> Self {
                Self(Pool::new(slots))
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

            /// Handles to every slot, in order.
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

    /// [`SPicture::data_ptr`]'s provenance covers the whole plane, not `[origin..]`:
    /// both backward reaches the encoder performs stay in bounds — one sample
    /// diagonally behind the origin (intra prediction's `pRef[-iLineSize - 1]`) and
    /// the whole `pad * stride + pad` walk back to the allocation base
    /// (`ExpandReferencingPicture`).
    #[test]
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

    /// [`SPicture::data_ptr_shared`] reaches the padding behind the origin, repeated
    /// mints are siblings so an earlier pointer survives a later call, and an earlier
    /// `data_ptr` stamp stays usable alongside them — the shared read sees the write
    /// made through it.
    #[test]
    #[allow(unsafe_code)]
    fn data_ptr_shared_reaches_the_padding_and_survives_sibling_mints() {
        let mut pic = SPicture::new(176, 144, false);
        let pad = PADDING_LENGTH;
        let stride = pic.plane(0).stride();

        pic.plane_mut(0).set(0, 0, 0x5A);
        pic.plane_mut(0).set(-1, -1, 0xC3);

        // One exclusive stamp first, then shared per-call mints.
        let p_stamp = pic.data_ptr(0);
        let p1 = pic.data_ptr_shared(0);
        let p2 = pic.data_ptr_shared(0);
        assert_eq!(p1, p2);
        assert_eq!(p1, p_stamp, "the shared mint is the same origin address");

        assert_eq!(unsafe { *p1 }, 0x5A);
        assert_eq!(
            unsafe { *p1.sub(stride + 1) },
            0xC3,
            "one sample diagonally behind the origin, in the top-left padding"
        );
        // Forward, to the last byte of the bottom-right padding.
        let len = pic.plane(0).as_slice().len();
        let tail = len - (pad * stride + pad) - 1;
        assert_eq!(unsafe { *p1.add(tail) }, 0);

        // The first mint is used after the second call — and after an
        // exclusive write through the stamp, which the shared read observes.
        unsafe { *p_stamp = 0x11 };
        let _p3 = pic.data_ptr_shared(0);
        assert_eq!(
            unsafe { *p1 },
            0x11,
            "a later mint or write popped the first"
        );
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

    /// A fresh picture holds zeros where [`SPicture::SetUnref`] leaves `-1`/`255`.
    #[test]
    fn a_fresh_picture_is_not_an_unreferenced_one() {
        let pic = SPicture::new(176, 144, false);
        assert_eq!(pic.iFramePoc, 0);
        assert_eq!(pic.uiTemporalId, 0);
        assert_eq!(pic.uiSpatialId, 0);
        assert_eq!(pic.uiRecieveConfirmed, RECIEVE_UNKOWN);
        // and the fields the constructor writes
        assert_eq!(pic.iWidthInPixel, 176);
        assert_eq!(pic.iHeightInPixel, 144);
        assert_eq!(pic.iFrameNum, -1);
        assert!(!pic.bIsLongRef);
        assert_eq!(pic.iLongTermPicNum, -1);
        assert_eq!(pic.iMarkFrameNum, -1);
    }
    /// [`SPicture::copy_planes_from`] against `WelsMoveMemory_c` on identical picture
    /// pairs, comparing the **whole allocation** of each destination — so a copy into
    /// the wrong rows, or over a padding byte, fails here. Most cases give source and
    /// destination different luma strides.
    #[test]
    #[allow(unsafe_code)]
    fn copy_planes_from_matches_the_raw_primitive_it_replaced() {
        use crate::encoder::wels_preprocess::WelsMoveMemory_c;

        for &(sw, sh, dw, dh, w, h) in &[
            (176, 144, 176, 144, 176, 144), // same geometry, the arm's own case
            (176, 144, 160, 128, 160, 128), // destination narrower: strides differ
            (320, 240, 176, 144, 176, 144),
            (176, 144, 176, 144, 32, 16), // a sub-rectangle of both
        ] {
            let mut src = SPicture::new(sw, sh, false);
            // A pattern that is different in every plane and every row, so a
            // stride slip or a plane swap cannot survive it.
            for i in 0..3 {
                let stride = src.planes[i].stride();
                let origin = src.planes[i].origin();
                let buf = src.planes[i].as_mut_slice();
                for (k, b) in buf.iter_mut().enumerate() {
                    *b = (k.wrapping_mul(31).wrapping_add(i * 7).wrapping_add(origin)
                        ^ (k / stride.max(1))) as u8;
                }
            }

            let mut dst_raw = SPicture::new(dw, dh, false);
            let mut dst_safe = SPicture::new(dw, dh, false);
            for i in 0..3 {
                dst_raw.planes[i].as_mut_slice().fill(0xA5);
                dst_safe.planes[i].as_mut_slice().fill(0xA5);
            }

            // The raw primitive, exactly as `DownsamplePadding` called it.
            let ksrc = src.planes();
            let kdst = dst_raw.planes();
            unsafe {
                WelsMoveMemory_c(
                    kdst.pData[0],
                    kdst.pData[1],
                    kdst.pData[2],
                    kdst.iLineSize[0],
                    kdst.iLineSize[1],
                    kdst.iLineSize[2],
                    ksrc.pData[0],
                    ksrc.pData[1],
                    ksrc.pData[2],
                    ksrc.iLineSize[0],
                    ksrc.iLineSize[1],
                    ksrc.iLineSize[2],
                    w,
                    h,
                );
            }

            dst_safe.copy_planes_from(&src, w, h);

            for i in 0..3 {
                assert_eq!(
                    dst_safe.planes[i].as_slice(),
                    dst_raw.planes[i].as_slice(),
                    "plane {i} differs for src {sw}x{sh} -> dst {dw}x{dh}, copying {w}x{h}"
                );
            }
            // And the copy actually happened.
            assert!(
                dst_safe.planes[0].as_slice().iter().any(|&b| b != 0xA5),
                "nothing was written for {w}x{h}"
            );
        }
    }
}
