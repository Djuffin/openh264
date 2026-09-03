// Copyright (c) 2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in
//      the documentation and/or other materials provided with the
//      distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

//! # Reconstructed Picture & Reference Frame Management (`picture.h`)
//!
//! Translated from `codec/decoder/core/inc/picture.h`.
//!
//! Defines the [`SPicture`] structure and [`PPicture`] pointer typedef representing
//! decoded video frame buffers in OpenH264. An [`SPicture`] serves multiple core roles:
//!
//! 1. **Reconstruction Target Canvas**: Memory container for uncompressed YUV 4:2:0
//!    planar pixel samples assembled during macroblock reconstruction and in-loop deblocking.
//! 2. **Reference Frame Storage (DPB)**: Stored in the Decoded Picture Buffer (`SPicBuff`)
//!    and referenced during inter-prediction motion compensation.
//! 3. **Direct Mode & Temporal MV Cache**: Retains macroblock types (`pMbType`),
//!    motion vectors (`pMv`), and reference picture indices (`pRefIndex`) for B-slice
//!    temporal direct mode derivation.
//! 4. **Multi-Threaded Row Synchronization**: Embeds row-level event barriers (`pReadyEvent`)
//!    used by worker threads during parallel macroblock row reconstruction.
//! 5. **Error Concealment Metadata**: Tracks macroblock decoding integrity flags and error
//!    propagation counters (`iMbEcedNum`, `iMbEcedPropNum`).

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

#![deny(unsafe_code)]

// Constants matching OpenH264 common definitions (`wels_const_common.h` and `wels_common_defs.h`)

/// Number of 4x4 sub-blocks in a 16x16 macroblock.
pub const MB_BLOCK4x4_NUM: usize = 16;

/// Motion vector coordinate dimension (X and Y components).
pub const MV_A: usize = 2;

/// Reference picture list index 0.
pub const LIST_0: usize = 0;

/// Reference picture list index 1.
pub const LIST_1: usize = 1;

/// Total number of reference picture lists (LIST_0 and LIST_1).
pub const LIST_A: usize = 2;

/// Standard border padding width/height in pixels for SIMD memory alignment and FIR filtering.
pub const PADDING_LENGTH: i32 = 32;

/// Memory resolution alignment boundary in bytes.
pub const PICTURE_RESOLUTION_ALIGNMENT: i32 = 32;

/// Base H.264 slice types matching `EWelsSliceType` in `wels_common_defs.h`.
pub use crate::decoder::slice::EWelsSliceType;

pub use crate::safe::plane::PaddedPlane;
pub use crate::safe::mb_grid::{MbArray, MbDims};

/// A handle to one slot of the decoder's picture pool. Declared in `safe/pool.rs`
/// and re-exported by `pic_queue.rs` as `PicId`; named here because [`SPicture`]
/// carries one.
pub use crate::safe::pool::Id as PicId;


/// Reconstructed Picture definition.
///
/// Expresses reference pictures stored in the DPB, reconstructed pictures for display output,
/// and temporal motion vector caches for direct-mode B-slice decoding.
///
/// Matches C++ `struct SPicture` from `codec/decoder/core/inc/picture.h`.
#[derive(Debug, Clone)]
pub struct SPicture {
    // =========================================================================
    // Payload Pixel Buffers & Geometries
    // =========================================================================

    /// The three owned sample planes: 0 Y (luma), 1 Cb, 2 Cr.
    ///
    /// Reached through [`plane`](Self::plane) / [`plane_mut`](Self::plane_mut), and
    /// through [`data_ptr`](Self::data_ptr) / [`linesize`](Self::linesize).
    ///
    /// **Three, not four.** C++ declares the arrays `[4]`; `AllocPicture` writes
    /// 0-2 and nothing in either decoder reads index 3. The four `pData[3]`
    /// writes elsewhere in this crate are on `SSourcePicture`, the public API type,
    /// which keeps its fourth slot.
    ///
    /// A picture with **no** sample memory — `AllocPicture`'s `bParseOnly` arm, and
    /// [`Default`] — carries three [`PaddedPlane::empty`] planes: strides, no bytes.
    planes: [PaddedPlane; 3],

    // =========================================================================
    // Error Concealment & Syntax Flags
    // =========================================================================

    /// Flag indicating whether the picture is an IDR (Instantaneous Decoder Refresh) keyframe.
    pub bIdrFlag: bool,

    /// Active picture width in luma pixels (from Sequence Parameter Set).
    pub iWidthInPixel: i32,

    /// Active picture height in luma pixels (from Sequence Parameter Set).
    pub iHeightInPixel: i32,

    /// Picture Order Count (POC) parsed from the slice header.
    pub iFramePoc: i32,

    // =========================================================================
    // Reference Picture Management
    // =========================================================================

    /// `true` if this picture is currently marked as a reference frame in the DPB.
    pub bUsedAsRef: bool,

    /// `true` if this picture is marked as a Long-Term Reference (LTR) frame.
    pub bIsLongRef: bool,

    /// Reference usage counter. Prevents buffer recycling while held by threads or DPB lists.
    pub iRefCount: i8,

    /// Callback function pointer invoked to clear reference marking and unreference the picture.
    ///
    /// The C++'s `void (*pSetUnRef) (PPicture pPic)` (`picture.h:73`) is a raw
    /// pointer because C has nothing else; here it is `&mut SPicture` and the
    /// `extern "C"` ABI is unchanged, because a `&mut T` parameter has the same
    /// ABI as a `*mut T` one.
    pub pSetUnRef: Option<extern "C" fn(&mut SPicture)>,

    /// `true` if all macroblocks in this picture were completely and cleanly decoded from the bitstream.
    pub bIsComplete: bool,

    // =========================================================================
    // Scalable Video Coding (SVC) & Identification Tags
    // =========================================================================

    /// SVC Temporal Layer Identifier (T_id in [0, 7]).
    pub uiTemporalId: u8,

    /// SVC Spatial Dependency Layer Identifier (D_id in [0, 7]).
    pub uiSpatialId: u8,

    /// SVC Quality Layer Identifier (Q_id in [0, 15]).
    pub uiQualityId: u8,

    /// `frame_num` syntax element parsed from slice header.
    pub iFrameNum: i32,

    /// Normalized frame wrap number used during reference picture list construction.
    pub iFrameWrapNum: i32,

    /// Long-term frame index assigned via MMCO commands (`long_term_frame_idx`).
    pub iLongTermFrameIdx: i32,

    /// Derived long-term picture number (`long_term_pic_num`).
    pub uiLongTermPicNum: u32,

    /// Bound Sequence Parameter Set ID (`seq_parameter_set_id`).
    pub iSpsId: i32,

    /// Bound Picture Parameter Set ID (`pic_parameter_set_id`).
    pub iPpsId: i32,

    /// Presentation timestamp (PTS) in microseconds / clock ticks.
    pub uiTimeStamp: u64,

    /// Monotonic relative decoding timestamp counter.
    pub uiDecodingTimeStamp: u32,

    /// Index of this picture node inside the parent `SPicBuff` buffer pool.
    pub iPicBuffIdx: i32,

    /// The pool slot this picture occupies, or `None` for a picture that is not in
    /// the pool at all (`pCtx->pTempDec`, and every test fixture).
    ///
    /// **Not `iPicBuffIdx`, and the difference is when it is written.** The C sets
    /// `iPicBuffIdx` in `PrefetchPic`, so a picture that has never been prefetched
    /// reports slot 0 — which is fine for what the C reads it for (the reordering
    /// path only ever looks at a picture that has been decoded into), and useless as
    /// an identity, because it makes every fresh picture indistinguishable from slot
    /// 0's. This is written once, by [`PicPool`](crate::decoder::pic_queue::PicPool)
    /// at construction, and a picture never moves between slots. `iPicBuffIdx` keeps
    /// its own timing exactly: it is read at `codec_api.rs:1569` off
    /// `pPreviousDecodedPictureInDpb`, so moving its write would be a behaviour
    /// change on the reordering output path.
    slot: Option<PicId>,

    /// Primary slice type of the picture (`I_SLICE`, `P_SLICE`, `B_SLICE`, etc.).
    pub eSliceType: EWelsSliceType,

    /// Multi-slice picture flag where each slice group contains exactly one slice.
    pub bIsUngroupedMultiSlice: bool,

    /// Flag indicating that this picture begins a new video sequence.
    pub bNewSeqBegin: bool,

    /// Total number of macroblocks in this picture that underwent error concealment.
    pub iMbEcedNum: i32,

    /// Number of macroblocks affected by error-concealment propagation.
    pub iMbEcedPropNum: i32,

    /// Total macroblock count in this picture (iMbWidth * iMbHeight).
    pub iMbNum: i32,

    // =========================================================================
    // Macroblock Level Metadata & Direct Mode Caches
    // =========================================================================

    /// Clean-decode flag per macroblock — `[iMbNum]` in the C.
    pub pMbCorrectlyDecodedFlag: MbArray<bool>,

    /// Macroblock coding types (`MB_TYPE_*`), read by direct-mode derivation.
    pub pMbType: MbArray<u32>,

    /// Motion vectors per 4x4 block for `LIST_0` and `LIST_1` (B-slice direct mode).
    pub pMv: [MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>; LIST_A],

    /// Reference indices per 4x4 block for `LIST_0` and `LIST_1` (direct mode).
    pub pRefIndex: [MbArray<[i8; MB_BLOCK4x4_NUM]>; LIST_A],

    /// This picture's own reference lists, as **slot handles** — snapshotted when the
    /// picture is marked as a reference, and read back by `MapColToList0` when a
    /// later B slice uses temporal direct mode.
    pub pRefPic: [[Option<PicId>; 17]; LIST_A],

}

/// Pointer typedef for reconstructed pictures matching `typedef struct SPicture* PPicture;`.
pub type PPicture = *mut SPicture;

impl Default for SPicture {
    fn default() -> Self {
        Self {
            // Every `SPicture::default()` in the crate is a test fixture; the live
            // allocation path is `with_planes`.
            planes: [
                PaddedPlane::empty(0),
                PaddedPlane::empty(0),
                PaddedPlane::empty(0),
            ],
            bIdrFlag: false,
            iWidthInPixel: 0,
            iHeightInPixel: 0,
            iFramePoc: 0,
            bUsedAsRef: false,
            bIsLongRef: false,
            iRefCount: 0,
            pSetUnRef: None,
            bIsComplete: false,
            uiTemporalId: 0,
            uiSpatialId: 0,
            uiQualityId: 0,
            iFrameNum: 0,
            iFrameWrapNum: 0,
            iLongTermFrameIdx: 0,
            uiLongTermPicNum: 0,
            iSpsId: 0,
            iPpsId: 0,
            uiTimeStamp: 0,
            uiDecodingTimeStamp: 0,
            iPicBuffIdx: 0,
            slot: None,
            eSliceType: EWelsSliceType::UNKNOWN_SLICE,
            bIsUngroupedMultiSlice: false,
            bNewSeqBegin: false,
            iMbEcedNum: 0,
            iMbEcedPropNum: 0,
            iMbNum: 0,
            // A picture that has not been through `AllocPicture` covers no
            // macroblocks.
            pMbCorrectlyDecodedFlag: MbArray::empty(),
            pMbType: MbArray::empty(),
            pMv: [MbArray::empty(), MbArray::empty()],
            pRefIndex: [MbArray::empty(), MbArray::empty()],
            pRefPic: [[None; 17]; LIST_A],
        }
    }
}

impl SPicture {
    /// Constructs a new zeroed [`SPicture`] instance.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// The picture `AllocPicture` used to hand back from a zeroing `WelsMallocz`,
    /// carrying the three planes it then built and filled.
    ///
    /// **`eSliceType` is `P_SLICE`, not [`Default`]'s `UNKNOWN_SLICE`.** The live
    /// path was a *zeroing* allocation and `P_SLICE == 0`.
    pub fn with_planes(planes: [PaddedPlane; 3], dims: MbDims) -> Self {
        Self {
            planes,
            eSliceType: EWelsSliceType::P_SLICE,
            // The four per-macroblock families are sized here, from the same
            // `uiMbWidth * uiMbHeight` `AllocPicture` used for its six
            // `WelsMallocz` calls.
            pMbCorrectlyDecodedFlag: MbArray::new(dims, false),
            pMbType: MbArray::new(dims, 0),
            pMv: [
                MbArray::new(dims, [[0; MV_A]; MB_BLOCK4x4_NUM]),
                MbArray::new(dims, [[0; MV_A]; MB_BLOCK4x4_NUM]),
            ],
            pRefIndex: [
                MbArray::new(dims, [0; MB_BLOCK4x4_NUM]),
                MbArray::new(dims, [0; MB_BLOCK4x4_NUM]),
            ],
            ..Default::default()
        }
    }

    /// Plane `i` — 0 Y, 1 Cb, 2 Cr.
    ///
    /// # Panics
    /// If `i > 2`.
    #[inline]
    pub fn plane(&self, i: usize) -> &PaddedPlane {
        &self.planes[i]
    }

    /// Mutable form of [`plane`](Self::plane).
    #[inline]
    pub fn plane_mut(&mut self, i: usize) -> &mut PaddedPlane {
        &mut self.planes[i]
    }

    /// All three planes at once, as **disjoint** mutable borrows.
    ///
    /// [`plane_mut`](Self::plane_mut) takes `&mut self` per call, so two planes
    /// cannot be held together through it — and the chroma deblocking kernels take
    /// Cb and Cr in one call. Destructuring the array is how safe Rust says the
    /// same thing: `let [y, cb, cr] = pic.planes_mut();`.
    #[inline]
    pub fn planes_mut(&mut self) -> &mut [PaddedPlane; 3] {
        &mut self.planes
    }

    /// Checks if the picture buffer is free and available for recycling in the DPB pool.
    ///
    /// A picture node in `SPicBuff` is eligible for reuse if and only if:
    /// `!bUsedAsRef && iRefCount <= 0`
    #[inline]
    pub fn is_free(&self) -> bool {
        !self.bUsedAsRef && self.iRefCount <= 0
    }

    /// The pool slot this picture occupies, or `None` if it is not in the pool.
    #[inline]
    pub fn pic_id(&self) -> Option<PicId> {
        self.slot
    }

    /// Records the slot this picture was allocated into.
    ///
    /// The pool is the only caller and it calls once, from `CreatePicBuff`, before
    /// the pool is reachable from anything else.
    #[inline]
    pub(crate) fn set_pic_id(&mut self, id: PicId) {
        self.slot = Some(id);
    }
}

/// The identity predicate: **are these two the same picture?**
///
/// Slot equality, and where a slot exists that is the whole answer — the alternative
/// reading ("a picture with the same POC") differs exactly when the DPB holds two
/// pictures with a duplicate POC, and a stream can produce that.
///
/// The address fallback is not a hedge, it is the definition of the population that
/// has no slot: `pCtx->pTempDec` (`decode_slice.rs:2043`, allocated outside the pool
/// and never compared with anything) and test fixtures. A picture with no slot is the
/// same picture as nothing but itself.
///
/// The slot a picture names, or `None` for an absent picture or one outside the
/// pool.
///
/// It is deliberately total: an absent picture and a pool-less one are both
/// "no slot".
#[inline]
pub fn pic_slot(p: Option<&SPicture>) -> Option<PicId> {
    match p {
        None => None,
        Some(p) => p.pic_id(),
    }
}

/// Are these two the same picture? See the note above for the address fallback.
#[inline]
pub fn same_picture(a: Option<&SPicture>, b: Option<&SPicture>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return a.is_none() && b.is_none();
    };
    match (a.pic_id(), b.pic_id()) {
        (Some(x), Some(y)) => x == y,
        _ => std::ptr::eq(a, b),
    }
}

impl SPicture {

    // =========================================================================
    // Plane accessors — the one way in
    //
    // Deliberately *not* a stored mirror: nothing caches a `pData` beside the plane
    // that owns it.
    // =========================================================================

    /// Bytes per row of plane `i` — the C++ `iLinesize[i]`, derived from the plane
    /// that owns those bytes rather than stored beside it.
    ///
    /// # Panics
    /// If `i > 2`. There are three planes.
    #[inline]
    pub fn linesize(&self, i: usize) -> i32 {
        self.planes[i].stride() as i32
    }

    /// Logical `(0, 0)` of plane `i` as a raw pointer, for the kernels that take a
    /// pointer and a stride.
    ///
    /// This is `pBuffer[i] + origin` computed on demand: the pointer the C stored is
    /// a *function* of the plane, so it is derived at each use and cannot go stale.
    ///
    /// Null when the picture has no sample memory: `AllocPicture`'s `bParseOnly` arm
    /// builds a picture that carries strides and no bytes, and every caller here
    /// tests for that with `.is_null()` exactly as the C does. An empty `Vec`'s
    /// `as_mut_ptr()` is dangling-but-non-null, so the emptiness is checked rather
    /// than leaned on.
    ///
    /// The public output path (`decoder_core.rs:1087`) hands these pointers to the
    /// API consumer, where they outlive the call by contract.
    ///
    /// # The provenance, which is the whole subtlety
    ///
    /// The returned pointer must be able to reach the **padding behind it**:
    /// `ExpandPictureLuma_c` does `pDst.sub(pad * stride + pad)` to recover the
    /// allocation, motion compensation reads at negative coordinates after clamping,
    /// and `pData[i]` was `pBuffer[i].add(origin)` — a pointer into the middle of an
    /// allocation it is entitled to all of. So this derives from the **whole**
    /// buffer and then moves the address, with `wrapping_add`, which does not narrow
    /// provenance and needs no `unsafe`.
    ///
    /// The obvious spelling, `plane.as_mut_slice()[origin..].as_mut_ptr()`, is
    /// **wrong**: it produces the same address, and it is UB at the first read into
    /// the top or left border, because the slicing hands out provenance over
    /// `[origin..]` only.
    ///
    /// # And the aliasing
    ///
    /// `plane.as_mut_slice().as_mut_ptr()` fixes the provenance and is **still**
    /// wrong: `as_mut_slice` is `&mut self.buf`, a `Unique` retag over the whole
    /// allocation, so every call pops the cursor the previous call returned.
    /// `root_ptr` reads the address out of the `Vec`'s own header with no reference
    /// formed, so repeated calls are sibling `SharedReadWrite` derivations that
    /// coexist, which is what every raw cursor here assumes.
    #[inline]
    pub fn data_ptr(&mut self, i: usize) -> *mut u8 {
        let plane = &mut self.planes[i];
        if plane.is_empty() {
            return std::ptr::null_mut();
        }
        let origin = plane.origin();
        plane.root_ptr().wrapping_add(origin)
    }

    /// `ExpandReferencingPicture` for a picture the caller holds as a borrow.
    ///
    /// `common::expand_pic::ExpandReferencingPicture` takes a slice of raw plane pointers because
    /// it is shared with the encoder, whose pictures are not `SPicture`; each of
    /// its two kernels then rebuilds the whole allocation out of the mid-plane
    /// pointer it was handed (`expand_shim_span`, which is the only place in the
    /// port that does that arithmetic). A picture *owns* its planes, so there is
    /// nothing to rebuild: `plane_mut(i).as_mut_slice()` **is** the padded
    /// allocation the kernel wants, `origin()` is the `pad * stride + pad` the
    /// shim computed, and `expand_picture` — already safe, already the single
    /// copy of the C++ body — takes it directly.
    ///
    /// The per-plane null guards the raw form carries are `plane(i).is_empty()`,
    /// which is what the null answered.
    pub fn expand_as_reference(&mut self) {
        let (kiWidthY, kiHeightY) = (self.iWidthInPixel, self.iHeightInPixel);
        let planes = [
            (0usize, kiWidthY, kiHeightY, PADDING_LENGTH as usize),
            (1, kiWidthY >> 1, kiHeightY >> 1, (PADDING_LENGTH >> 1) as usize),
            (2, kiWidthY >> 1, kiHeightY >> 1, (PADDING_LENGTH >> 1) as usize),
        ];
        for (i, pic_w, pic_h, pad) in planes {
            let stride = self.linesize(i) as usize;
            let plane = self.plane_mut(i);
            if plane.is_empty() {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_picture_initialization() {
        let mut pic = SPicture::new();
        assert!(pic.is_free());
        assert_eq!(pic.bUsedAsRef, false);
        assert_eq!(pic.iRefCount, 0);
        assert_eq!(pic.eSliceType, EWelsSliceType::UNKNOWN_SLICE);
        // The all-null, all-zero plane state.
        assert!(pic.plane(0).is_empty());
        assert_eq!(pic.linesize(0), 0);
        assert_eq!(pic.data_ptr(0), std::ptr::null_mut());
    }

    /// `with_planes` reproduces the *zeroed* allocation, and `Default` does not: the
    /// two disagree on `eSliceType`. `AllocPicture` used a zeroing malloc, so the
    /// live value is `P_SLICE`.
    #[test]
    fn with_planes_reproduces_the_zeroed_allocation_not_default() {
        let pic = SPicture::with_planes([
            PaddedPlane::empty(0),
            PaddedPlane::empty(0),
            PaddedPlane::empty(0),
        ], MbDims::none());
        assert_eq!(pic.eSliceType, EWelsSliceType::P_SLICE);
        assert_eq!(SPicture::default().eSliceType, EWelsSliceType::UNKNOWN_SLICE);
        assert_eq!(EWelsSliceType::P_SLICE as i32, 0, "the zero is what makes it the live value");
    }

    /// `data_ptr` is `pBuffer[i] + origin` computed on demand — the offset
    /// `AllocPicture` used to bake into a stored `pData[i]` — **and it can reach
    /// backwards.**
    ///
    /// The second half must be run under Miri to mean anything: the samples are in
    /// the allocation either way, so a `data_ptr` that narrowed provenance to
    /// `[origin..]` would read the right bytes while being UB at the first border
    /// read.
    ///
    /// Both backward reaches the decoder actually performs are exercised: one sample
    /// diagonally behind the origin (motion compensation past the picture edge) and
    /// the full `pDst.sub(pad * stride + pad)` that `expand_shim_span`
    /// (`decoder_core.rs`) uses to recover the whole allocation from `pData[i]`.
    #[test]
    #[allow(unsafe_code)]
    fn data_ptr_reaches_the_padding_behind_the_logical_origin() {
        let (w, h, pad, stride) = (176usize, 144usize, 32usize, 240usize);
        let mut pic = SPicture::with_planes([
            PaddedPlane::new(w, h, pad, stride),
            PaddedPlane::new(w / 2, h / 2, pad / 2, stride / 2),
            PaddedPlane::new(w / 2, h / 2, pad / 2, stride / 2),
        ], MbDims::none());
        pic.plane_mut(0).set(0, 0, 0x5A);
        pic.plane_mut(0).set(-1, -1, 0xC3);
        pic.plane_mut(0).set(-(pad as isize), -(pad as isize), 0x7E);

        let base = pic.plane(0).as_slice().as_ptr();
        let origin = pic.plane(0).origin();
        let len = pic.plane(0).as_slice().len();
        assert_eq!(origin, (1 + stride) * pad, "the C's (1 + iLinesize[0]) * PADDING_LENGTH");

        let p = pic.data_ptr(0);
        assert_eq!(unsafe { p.offset_from(base) } as usize, origin);
        assert_eq!(unsafe { *p }, 0x5A);
        assert_eq!(
            unsafe { *p.sub(stride + 1) },
            0xC3,
            "one sample diagonally behind the origin — an MV past the picture edge"
        );
        // `expand_shim_span`'s reconstruction, byte for byte.
        let whole = {
            unsafe { std::slice::from_raw_parts(p.sub(pad * stride + pad), (h + 2 * pad) * stride) }
        };
        assert_eq!(whole[0], 0x7E, "the top-left corner of the padding");
        assert_eq!(whole.len(), len, "the padded picture is the whole allocation here");

        assert_eq!(pic.linesize(0), stride as i32);
        assert_eq!(pic.linesize(1), (stride / 2) as i32);
    }

    /// The accessor is asked twice and the first cursor is used after the second
    /// call. `data_ptr` hands out a raw cursor the caller keeps, so its spelling has
    /// to be retag-stable, and that is a *different* property from the provenance the
    /// test above pins: `plane.as_mut_slice().as_mut_ptr()` derives from the
    /// allocation root, while `&mut self.buf` is a `Unique` retag over that same
    /// allocation, so each call pops the pointer the previous call returned.
    #[test]
    #[allow(unsafe_code)]
    fn data_ptr_twice_leaves_the_first_cursor_usable() {
        let (w, h, pad, stride) = (176usize, 144usize, 32usize, 240usize);
        let mut pic = SPicture::with_planes([
            PaddedPlane::new(w, h, pad, stride),
            PaddedPlane::new(w / 2, h / 2, pad / 2, stride / 2),
            PaddedPlane::new(w / 2, h / 2, pad / 2, stride / 2),
        ], MbDims::none());

        let first = pic.data_ptr(0);
        let second = pic.data_ptr(0);
        assert_eq!(first, second, "the same plane resolves to the same address");

        // The use that matters: the FIRST cursor, after the second derivation.
        unsafe { *first = 0x5A };
        assert_eq!(unsafe { *second }, 0x5A, "sibling cursors read each other's writes");
        // And the reverse order, so neither derivation is merely tolerated as dead.
        unsafe { *second = 0xC3 };
        assert_eq!(unsafe { *first }, 0xC3);

        // A cursor into a *different* plane is live across a re-derivation of this one.
        let chroma = pic.data_ptr(1);
        let luma_again = pic.data_ptr(0);
        unsafe { *chroma = 0x7E };
        assert_eq!(unsafe { *chroma }, 0x7E);
        assert_eq!(unsafe { *luma_again }, 0xC3, "re-deriving plane 0 did not disturb it");
    }

    /// The recycling predicate `PrefetchPic` scans on.
    #[test]
    fn test_picture_is_free_predicate() {
        let mut pic = SPicture::new();
        assert!(pic.is_free());

        pic.bUsedAsRef = true;
        assert!(!pic.is_free());

        pic.bUsedAsRef = false;
        pic.iRefCount = 1;
        assert!(!pic.is_free(), "a held picture is not recyclable even when unmarked");

        pic.iRefCount = 0;
        assert!(pic.is_free());
    }
}
