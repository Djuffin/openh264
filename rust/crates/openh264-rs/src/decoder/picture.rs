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
// Phase 5 W6 (family 2). The module's two `unsafe fn` were the identity pair,
// `pic_slot` and `same_picture`, each taking `*const SPicture` with a null test at
// the top — `fmo.rs`'s shape one module over, and it takes `fmo.rs`'s conversion:
// `Option<&SPicture>`, with the null test kept exactly where it was and spelled as
// the `Option`. The callers do the `as_ref()` at their own raw boundary, which is
// where the rawness actually lives; each of those modules is its own family below.
//
// **The enumerated exception is in one test, and it is mandated rather than
// tolerated**: `data_ptr` is the phase-5 shim accessor below — its marker is on the
// accessor itself, and naming the marker again here would raise this file's `shim`
// count by a comment, which the ratchet caught on the first run (S16's prose floor).
// It must reach the padding *behind* the pointer it returns, and S28 requires a Miri
// test that reads the pointer's full legal reach in both directions. Reading through
// a raw pointer is what that instrument *is*, so the test carries
// `#[allow(unsafe_code)]` on itself — not the test module — and it retires with the
// accessor, whose one production use is `DecodeFrameConstruction`'s three `ppDst[i]`
// writes across the C ABI (Phase 8's, with the api-owned fields).

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
///
/// **Not `#[repr(C)]` since T5.C3**, and not `Copy`. The planes are owned Rust
/// values, so a C layout would be a claim the struct can no longer honour; the
/// decoder's `SPicture` crosses no FFI boundary and carries no `assert_size!` or
/// offset pin (`assert_size!(SPicture, 136)` in `encoder/abi_guard.rs` is the
/// encoder's same-named struct, allowlist class (a)). Dropping `Copy` cost nothing:
/// the compiler was asked first, and nothing in the crate copied one by value.
/// `RawDataBuffer` (`decoder/bit_stream.rs`, T3.4) set both precedents.
#[derive(Debug, Clone)]
pub struct SPicture {
    // =========================================================================
    // Payload Pixel Buffers & Geometries
    // =========================================================================

    /// The three owned sample planes: 0 Y (luma), 1 Cb, 2 Cr.
    ///
    /// **T5.C3 replaced the `pBuffer[i]` / `pData[i]` / `iLinesize[i]` triple with
    /// this one field.** C++ `picture.h:53-55` declares three parallel arrays whose
    /// agreement nothing enforces: `pBuffer[i]` the allocation, `pData[i]` a pointer
    /// `pad` rows and `pad` bytes into it, `iLinesize[i]` the stride that relates
    /// them. [`PaddedPlane`] is those three as one value that owns its bytes, so the
    /// two call sites that could disagree about a size no longer exist — F1's shape,
    /// and the reason `safe/plane.rs` was written in Phase 2 for this moment.
    ///
    /// Reached through [`plane`](Self::plane) / [`plane_mut`](Self::plane_mut) by
    /// converted callers, and through [`data_ptr`](Self::data_ptr) /
    /// [`linesize`](Self::linesize) by the ones 5.2-5.6 have yet to convert.
    ///
    /// **Three, not four.** C++ declares the arrays `[4]`; `AllocPicture` writes
    /// 0-2 and nothing in either decoder reads index 3 (T5.C1). The four `pData[3]`
    /// writes elsewhere in this crate are on `SSourcePicture`, the public API type,
    /// which keeps its fourth slot.
    ///
    /// A picture with **no** sample memory — `AllocPicture`'s `bParseOnly` arm, and
    /// [`Default`] — carries three [`PaddedPlane::empty`] planes: strides, no bytes.
    planes: [PaddedPlane; 3],

    // T5.C1: `pub iPlanes: i32` sat here, written `3` once by `AllocPicture` and read
    // nowhere — in this port *and* in the C++ decoder, where `picture.h:56` declares it
    // and `pic_queue.cpp:105` writes it with no source reading it back. Fixing the plane
    // count at three is therefore subtraction on both sides, not a decision.

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
    /// **T5.AC1: the callback takes a borrow, so it is a safe function pointer.**
    /// The C++'s `void (*pSetUnRef) (PPicture pPic)` (`picture.h:73`) is a raw
    /// pointer because C has nothing else; here it is `&mut SPicture` and the
    /// `extern "C"` ABI is unchanged, because a `&mut T` parameter has the same
    /// ABI as a `*mut T` one. The null test the C would need before the call is
    /// discharged by the parameter type at both of the two call sites — the pool
    /// release in `api/codec_api.rs` and `manage_dec_ref`'s reinstall — each of
    /// which already holds the picture as something it has proved non-null.
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

    /// The pool slot this picture occupies — plan §2.2.3's `PicId` — or `None` for a
    /// picture that is not in the pool at all (`pCtx->pTempDec`, and every test
    /// fixture).
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

    // =========================================================================
    // **T5.P′3 (W3): the picture finishes owning itself.**
    //
    // These four families were the last raw allocations on `SPicture` — six
    // `WelsMallocz` calls in `AllocPicture` against four `WelsFree` loops in
    // `FreePicture`, sized from `uiMbCount` at one end and re-derived at the other.
    // They are [`MbArray`]s now, the same containers the layer's 22 per-macroblock
    // families have been on since T5.M1, addressed by the same `MbDims`. The recipe
    // is T5.C3's, applied to the picture's *metadata* where that commit applied it
    // to the picture's *samples*: an owned container whose drop glue is the free
    // path, so `FreePicture` is drop glue and nothing else.
    //
    // What this buys the rest of W3: `AllocPicture`'s `Box` is now a **complete**
    // owner, so a `Box<SPicture>` can be handed to the pool (and to `pTempDec`)
    // without leaking six allocations that no drop glue reaches.
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
    ///
    /// **T5.P′2 turned it from raw picture pointers into `Option<PicId>`**, with
    /// `sRefPic`'s three lists and for their reason: a raw alias into the pool, held
    /// on a *pooled picture*, for as long as that picture is a reference. The
    /// snapshot in `DecodeCurrentAccessUnit` is now a handle-to-handle copy that
    /// never touches the pool at all.
    pub pRefPic: [[Option<PicId>; 17]; LIST_A],

}

/// Pointer typedef for reconstructed pictures matching `typedef struct SPicture* PPicture;`.
///
/// **Phase 8's, and the last thing that names it is the boundary** (T5b.9). Every
/// decoder re-export of this name was dead after T5b.1 turned the reference arm into
/// an identity; what is left is [`PicPool::slot_at_mut`](crate::decoder::pic_queue::PicPool::slot_at_mut)'s
/// return, whose one consumer is `api/codec_api.rs`'s `iPicBuffIdx` release path
/// across the C ABI. The typedef *is* the boundary's spelling, so it retires with
/// the boundary and not before.
pub type PPicture = *mut SPicture;

impl Default for SPicture {
    fn default() -> Self {
        Self {
            // `empty(0)` rather than a plane with a nominal stride: the pointer form's
            // zeroed state reported `iLinesize == 0` and a null `pData`, and both are
            // reproduced exactly. Every `SPicture::default()` in the crate is a test
            // fixture; the live allocation path is `with_planes`.
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
            // macroblocks: `MbArray::empty()` is the state the six null pointers
            // were in, and `as_slice().is_empty()` is the test that replaced
            // `.is_null()` (T5.P′3).
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
    /// **`eSliceType` is `P_SLICE`, not [`Default`]'s `UNKNOWN_SLICE`.** The two have
    /// always disagreed, and this is the arm that matters: the live path was a
    /// *zeroing* allocation and `P_SLICE == 0`, so reproducing the zero is what makes
    /// this constructor a substitution rather than a change. Nothing observes either
    /// value — a picture's `eSliceType` is written at `decoder_core.rs:3661` and
    /// `manage_dec_ref.rs:685` and read nowhere in the decoder, `iPlanes`'s situation
    /// one field over — but a constructor replacing a memset does not get to pick.
    pub fn with_planes(planes: [PaddedPlane; 3], dims: MbDims) -> Self {
        Self {
            planes,
            eSliceType: EWelsSliceType::P_SLICE,
            // **T5.P′3**: the four per-macroblock families are sized here, from the
            // same `uiMbWidth * uiMbHeight` `AllocPicture` used for its six
            // `WelsMallocz` calls. One geometry, stated once, and the drop glue is
            // the free path.
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

    /// Plane `i` — 0 Y, 1 Cb, 2 Cr. The destination the phase-5 shim accessors below
    /// are strangling towards: a converted caller takes the plane and walks it with
    /// cursors, and never sees a pointer.
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

    /// All three planes at once, as **disjoint** mutable borrows (T5.AA2).
    ///
    /// [`plane_mut`](Self::plane_mut) takes `&mut self` per call, so two planes
    /// cannot be held together through it — and the chroma deblocking kernels take
    /// Cb and Cr in one call, which is what the three raw `data_ptr` derivations
    /// were expressing. Destructuring the array is how safe Rust says the same
    /// thing: `let [y, cb, cr] = pic.planes_mut();`.
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

/// Plan P3's identity predicate: **are these two the same picture?**
///
/// Slot equality, and where a slot exists that is the whole answer — which is the
/// property the five P3 identity tests were written to pin, because the alternative
/// reading ("a picture with the same POC") differs exactly when the DPB holds two
/// pictures with a duplicate POC, and a stream can produce that.
///
/// The address fallback is not a hedge, it is the definition of the population that
/// has no slot: `pCtx->pTempDec` (`decode_slice.rs:2043`, allocated outside the pool
/// and never compared with anything) and test fixtures. A picture with no slot is the
/// same picture as nothing but itself. The arm disappears as the holders convert —
/// where a caller already has `PicId`s, as `SDeblockingFilter` does after T5.N4, it
/// compares them directly and never reaches here.
///
/// The slot a picture names, or `None` for an absent picture or one outside the
/// pool.
///
/// The one-way door from the pointer world into the id world, and the shape every
/// raw picture field takes as it converts (T5.P2's `pDec` and `pECRefPic`, and the
/// reference lists after them). It is deliberately total: an absent picture and a
/// pool-less one are both "no slot", which is what the arms it replaces did.
///
/// **T5.W1 moved the absence from a null pointer into the `Option`** and the
/// function stopped being `unsafe`. The `None` arm is the null test, unmoved: a
/// caller holding a raw pointer writes `pic_slot(p.as_ref())`, and `as_ref()`
/// performs exactly the null test this function used to perform for it — one
/// `unsafe` at the caller's own raw boundary instead of one hidden behind a safe-
/// looking argument.
///
/// (S16's prose floor, collected for the eleventh time: the first draft of the
/// sentence above named the pointer type this function exists to delete, and the
/// ratchet counted it.)
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
        // The raw form returned `a == b` whenever either side was null: two nulls
        // compared equal, and a null never equalled a live picture. Reproduced.
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
    // T5.C2 moved every read of `pData[i]` / `iLinesize[i]` in `src/decoder` onto
    // this pair, so that T5.C3's change of representation (three owned
    // `PaddedPlane`s in place of the pointer/stride arrays) touches this file and
    // `pic_queue.rs` and nothing else. The signatures here are already the ones the
    // owned form needs — `&mut self` to hand out a writable raw pointer, `&self` for
    // a stride — so this commit is where the borrow shapes land and where the S25
    // enumerations for the files it touches are written.
    //
    // Deliberately *not* a stored mirror: nothing caches a `pData` beside the plane
    // that owns it. That is the F16/T5 class — two fields that can disagree about
    // one buffer — and the whole point of the conversion is to have one.
    // =========================================================================

    /// Bytes per row of plane `i` — the C++ `iLinesize[i]`, derived from the plane
    /// that owns those bytes rather than stored beside it.
    ///
    /// # Panics
    /// If `i > 2`. There are three planes; the C's fourth slot was deleted at T5.C1.
    #[inline]
    pub fn linesize(&self, i: usize) -> i32 {
        self.planes[i].stride() as i32
    }

    /// SHIM(phase5) — logical `(0, 0)` of plane `i` as a raw pointer, for the kernels
    /// that still take a pointer and a stride.
    ///
    /// This is `pBuffer[i] + origin` computed on demand. It is the whole reason the
    /// conversion needs no mirror field: the pointer the C stored is a *function* of
    /// the plane, so it can be derived at each use and cannot go stale.
    ///
    /// Null when the picture has no sample memory: `AllocPicture`'s `bParseOnly` arm
    /// builds a picture that carries strides and no bytes, and every caller here
    /// tests for that with `.is_null()` exactly as the C does. An empty `Vec`'s
    /// `as_mut_ptr()` is dangling-but-non-null, so the emptiness is checked rather
    /// than leaned on.
    ///
    /// This is the shim the decoder's still-raw callers stand on, and it dies as
    /// 5.2-5.6 convert them onto [`plane_mut`](Self::plane_mut) — except at one
    /// caller, which is not a kernel: the public output path
    /// (`decoder_core.rs:1087`) hands these pointers to the API consumer, where they
    /// outlive the call by contract. That one outlives Phase 5.
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
    /// **wrong**, and wrong in the way that costs a phase: it is safe code, it
    /// produces the same address, every golden row and both benches stay
    /// bit-identical — and it is UB at the first read into the top or left border,
    /// because the slicing hands out provenance over `[origin..]` only. Miri named
    /// it here on the first run against the test below. S15's sentence, collected in
    /// the commit that quotes it: byte-exactness does not imply soundness.
    ///
    /// # And the aliasing, which is a second subtlety S28 does not cover
    ///
    /// `plane.as_mut_slice().as_mut_ptr()` fixes the provenance and is **still**
    /// wrong: `as_mut_slice` is `&mut self.buf`, a `Unique` retag over the whole
    /// allocation, so every call pops the cursor the previous call returned. This
    /// accessor carried that spelling until Phase 6 session G and passed only
    /// because no decoder caller re-derives while a cursor is live — an accident of
    /// the call graph, not a property of the accessor. `root_ptr` reads the address
    /// out of the `Vec`'s own header with no reference formed, so repeated calls are
    /// sibling `SharedReadWrite` derivations that coexist, which is what every raw
    /// cursor here assumes. That is **S40**, earned by F63 on the encoder's side of
    /// the same `PaddedPlane`, and `data_ptr_twice_leaves_the_first_cursor_usable`
    /// is the test that stops it being re-assumed.
    #[inline]
    pub fn data_ptr(&mut self, i: usize) -> *mut u8 {
        let plane = &mut self.planes[i];
        if plane.is_empty() {
            return std::ptr::null_mut();
        }
        let origin = plane.origin();
        plane.root_ptr().wrapping_add(origin)
    }

    // **T5b.7, S18: `data_ptr_ref` stood here.** It was `data_ptr`'s shared form,
    // and it existed for exactly one consumer: `GetRefPic` filling an `sMCRefMember`
    // from a reference picture. `sMCRefMember` died at T5b.2 and took the consumer
    // with it, so the accessor has had no caller outside its own test for two
    // sessions. Its S28 provenance test goes with it — the instrument retires with
    // the subject it instruments, which is what that rule says. `data_ptr` stays:
    // its one production use is `DecodeFrameConstruction`'s three `ppDst[i]` writes.

    // T5.B2: `unsafe fn unref(&mut self)` sat here. It handed `self as *mut SPicture`
    // to the `pSetUnRef` callback, which immediately re-borrowed it `&mut` — the S25
    // shape, and the one Phase 5's brief named as 5.1's first hazard. Two facts
    // settled it: the C++ `SPicture` has no such member (`picture.h:73` declares the
    // callback and nothing else), and the port's only caller was this file's own unit
    // test. Its `else` arm — decrement and clear — was a port invention with no C++
    // counterpart, and it disagreed with `SetUnRef`, which clears the marks *without*
    // touching `iRefCount` and reinstalls itself when the count is still positive.
    // The live unreferencing path is the callback, invoked directly at
    // `api/codec_api.rs:1607` and by `manage_dec_ref.rs`'s seven `SetUnRef` calls.

    /// `ExpandReferencingPicture` for a picture the caller holds as a borrow —
    /// **the decoder's three call sites, in safe code** (T5.AC5).
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
    /// which is what the null answered (T5.C3). The encoder's two call sites keep
    /// the raw entry point: converting them is **Phase 6's** (F12/P10).
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

    // T5.C3: `unsafe fn calculate_data_pointers(&mut self, padding_length: i32)` sat
    // here, recomputing `pData[i]` from `pBuffer[i]` and `iLinesize[i]` with the
    // border formula. It had **no callers** — `AllocPicture` writes the same
    // arithmetic inline — and the plane now holds that offset as its `origin`, so it
    // is both dead and subsumed. `data_ptr` above is what is left of it: the same
    // expression, evaluated at each use instead of cached into a field.
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
        // The all-null, all-zero plane state, in the terms that replaced it.
        assert!(pic.plane(0).is_empty());
        assert_eq!(pic.linesize(0), 0);
        assert_eq!(pic.data_ptr(0), std::ptr::null_mut());
    }

    /// `with_planes` reproduces the *zeroed* allocation, and `Default` does not: the
    /// two disagree on `eSliceType` and always have. `AllocPicture` used a zeroing
    /// malloc, so the live value is `P_SLICE`, and a constructor standing in for a
    /// memset has to say so out loud or the substitution is a change.
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
    /// The second half is the part with teeth, and it must be run under Miri to mean
    /// anything: the samples are in the allocation either way, so a `data_ptr` that
    /// narrowed provenance to `[origin..]` would read the right bytes and pass every
    /// golden row while being UB at the first border read. The first draft of
    /// `data_ptr` did exactly that and Miri failed here immediately.
    ///
    /// Both backward reaches the decoder actually performs are exercised: one sample
    /// diagonally behind the origin (motion compensation past the picture edge) and
    /// the full `pDst.sub(pad * stride + pad)` that `expand_shim_span`
    /// (`decoder_core.rs`) uses to recover the whole allocation from `pData[i]`.
    ///
    /// **The module's enumerated exception to `#![deny(unsafe_code)]`, and S28 is
    /// what mandates it**: the rule's own sentence is "every such accessor gets a
    /// Miri test that reads the pointer's full legal reach in both directions", so
    /// the raw reads below *are* the instrument. It retires with `data_ptr` itself,
    /// when family 13's callers stop taking plane pointers (W6 step 3).
    #[test]
    // unsafe-cat: instrument(test)
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

    /// **S40: the accessor is asked twice and the first cursor is used after the
    /// second call.** `data_ptr` hands out a raw cursor the caller keeps, so its
    /// spelling has to be retag-stable, and that is a *different* property from the
    /// provenance the test above pins — F63 (session F) is the whole reason it has
    /// its own test: `plane.as_mut_slice().as_mut_ptr()` derives from the allocation
    /// root and answers S28 perfectly, while `&mut self.buf` is a `Unique` retag over
    /// that same allocation, so each call pops the pointer the previous call returned.
    ///
    /// The encoder's `PaddedPlane` hit that for real (twice a frame, `WelsInitCurrentLayer`
    /// then `AnalyzePictureComplexity`). Nothing in the decoder re-derives while a
    /// cursor is live *today* — `DecodeFrameConstruction`'s three `ppDst[i]` writes are
    /// the one production caller and it takes all three, uses them, and drops them.
    /// That is an accident of the current call graph, not a property of the accessor,
    /// and S40's sentence is that an accessor surviving on "no caller re-derives yet"
    /// inherits the test rather than the assumption. So this is written against
    /// `data_ptr`'s contract, and it is why the body reads the address out of the
    /// `Vec`'s header (`PaddedPlane::root_ptr`) instead of through a slice.
    ///
    /// Red-proofed at the commit that added it: restoring
    /// `plane.as_mut_slice().as_mut_ptr()` fails this test under Miri with
    /// "attempting a write access using <tag> but that tag does not exist in the
    /// borrow stack", and passes every other test in the file.
    #[test]
    // unsafe-cat: instrument(test)
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

    /// The recycling predicate `PrefetchPic` scans on, in its own right — the test
    /// that used to stand here drove it through `SPicture::unref`, which is gone
    /// (see the note at the deletion), and so tested a port invention rather than
    /// the predicate. `SetUnRef`'s own effects are pinned in `manage_dec_ref.rs`.
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
