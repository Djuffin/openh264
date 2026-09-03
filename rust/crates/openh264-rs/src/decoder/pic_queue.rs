// Copyright (c) 2009-2013, Cisco Systems
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

#![deny(unsafe_code)]

//! # Decoded Picture Buffer Pool & Recycled Picture Queue (`pic_queue.rs`)
//!
//! Translated from `codec/decoder/core/inc/pic_queue.h` and `codec/decoder/core/src/pic_queue.cpp`.
//!
//! Provides the pre-allocated recycled picture buffer pool ([`SPicBuff`]) and
//! reconstructed picture object ([`SPicture`]) memory management for the H.264 / AVC
//! video decoder.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]
#![forbid(unsafe_code)]

use std::ffi::{c_char, c_void};
use crate::decoder::decoder_context::SDecodingParam;
use crate::decoder::decoder_core::{ERR_INFO_INVALID_PARAM, ERR_INFO_OUT_OF_MEMORY, ERR_NONE};

// ============================================================================
// Constants & Geometry Macro Definitions
// ============================================================================

/// Pixel boundary alignment applied to frame buffer width and height dimensions.
pub const PICTURE_RESOLUTION_ALIGNMENT: i32 = 32;

/// Perimeter reference extension padding in pixels around all 4 edges.
pub const PADDING_LENGTH: i32 = 32;

/// Chroma reference extension padding in pixels.
pub const CHROMA_PADDING_LENGTH: i32 = 16;

/// Motion vector list indices.
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;

/// Sub-block counts and motion vector component counts per macroblock.
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MV_A: usize = 2;

// ============================================================================
// Data Structures & Enums
// ============================================================================

pub use crate::decoder::picture::{SPicture, PPicture};
pub use crate::safe::plane::PaddedPlane;
use crate::safe::mb_grid::MbDims;
pub use crate::safe::pool::{Pool, PoolRest};

/// A handle to one slot of the decoder's [`PicPool`].
///
/// Identity is slot equality: two pictures are "the same reference" when they occupy
/// the same pool slot, never when they merely share a POC.
pub type PicId = crate::safe::pool::Id;

/// The decoder's recycled picture pool — C++ `SPicBuff` (`pic_queue.h:45-49`).
#[derive(Debug)]
pub struct PicPool {
    /// One slot per pre-allocated picture. Never grows or shrinks: the C++ sizes the
    /// queue once in [`CreatePicBuff`] and recycles thereafter.
    ///
    /// `None` is the C's null `ppPic[i]`, which the partial-failure and teardown
    /// paths both produced and every scan tested for.
    slots: Pool<PicSlot>,
    /// The C's `iCurrentIdx` — the circular cursor both prefetch scans advance.
    cursor: i32,
}

/// What one pool slot holds: the picture, owned, or nothing.
pub type PicSlot = Option<Box<SPicture>>;

/// The C's name for [`PicPool`].
pub type SPicBuff = PicPool;
pub type PPicBuff = *mut PicPool;

/// **A decode bracket's view of the pool**: `PicId` → picture, with the pool reached
/// once at the bracket top and nowhere below it.
#[derive(Clone, Copy, Debug)]
pub struct PicRefs<'a> {
    view: PicView<'a>,
}

/// The three states a bracket's view can be in.
///
/// [`Split`](PicView::Split) is the one the decode brackets take; the other two exist
/// because a bracket can open with no pool (`None` before `CreatePicBuff` and after
/// `DestroyPicBuff`) or with no current picture.
#[derive(Clone, Copy, Debug)]
enum PicView<'a> {
    /// No pool.
    None,
    /// A pool with no slot held mutably.
    Whole(&'a PicPool),
    /// [`PicPool::cur_and_rest_mut`]'s half: one slot held mutably by the caller,
    /// every other readable — [`Pool::mut_and_rest`]'s halves. The current picture
    /// is the caller's `&mut`, so this side keeps its *identity* and no address.
    Split { rest: PoolRest<'a, PicSlot>, cur: PicId },
}

impl<'a> PicRefs<'a> {
    /// The bracket top's derivation, for a scope that holds no picture mutably.
    #[inline]
    pub fn over(pool: Option<&'a PicPool>) -> Self {
        Self {
            view: match pool {
                Some(pool) => PicView::Whole(pool),
                None => PicView::None,
            },
        }
    }

    /// [`PicPool::cur_and_rest_mut`]'s half — the split view, told which slot its
    /// sibling `&mut` came from.
    #[inline]
    fn split(rest: PoolRest<'a, PicSlot>, cur: PicId) -> Self {
        Self { view: PicView::Split { rest, cur } }
    }

    /// **The reader's form of [`classify`](Self::classify)**: the picture a stored
    /// handle names, resolved against the picture the bracket is holding.
    ///
    /// Most of what the decode path does below a bracket top is *read* a reference —
    /// POCs, `bIsComplete`, `bIsLongRef`, the colocated macroblock's motion — and for
    /// those the current picture is just another source.
    ///
    /// The current slot is still never resolved through [`PoolRest::get`], which
    /// panics on it — that is what `cur` is kept for. Motion compensation, which
    /// writes while it reads, cannot use this and asks [`classify`](Self::classify)
    /// directly.
    ///
    /// `cur` is an `Option` because a bracket can open on an empty slot: the pool
    /// hands back no picture and this view still knows the slot's *identity*.
    #[inline]
    pub fn resolve<'s>(
        &self,
        slot: Option<PicId>,
        cur: Option<&'s SPicture>,
    ) -> Option<&'s SPicture>
    where
        'a: 's,
    {
        match self.classify(slot) {
            RefSlot::Empty => None,
            RefSlot::Current => cur,
            RefSlot::Other(pic) => Some(pic),
        }
    }

    /// What a stored handle names, with **the picture this bracket is writing told
    /// apart from the rest**.
    ///
    /// This is the type-level form of the test error concealment spells as
    /// `same_picture(pSrcPic, pDstPic)`: its three copy paths resolve a reference,
    /// compare it against the picture they are about to write, and skip when the two
    /// are one. It is also what **motion compensation** asks: the `Current` arm is
    /// `mc_luma_same`'s (`common/mc.rs`), where source and destination are one
    /// allocation and there is no second cursor to build.
    #[inline]
    pub fn classify(&self, slot: Option<PicId>) -> RefSlot<'a> {
        let Some(id) = slot else {
            return RefSlot::Empty;
        };
        match self.view {
            PicView::None => RefSlot::Empty,
            PicView::Whole(pool) => match pool.slot(id) {
                Some(pic) => RefSlot::Other(pic),
                None => RefSlot::Empty,
            },
            PicView::Split { rest, cur } => {
                if id == cur {
                    RefSlot::Current // never `rest.get(cur)`, which panics.
                } else {
                    match rest.get(id) {
                        Some(pic) => RefSlot::Other(pic),
                        None => RefSlot::Empty,
                    }
                }
            }
        }
    }
}

/// [`PicRefs::classify`]'s answer.
#[derive(Debug)]
pub enum RefSlot<'a> {
    /// No pool, no handle, or an empty slot.
    Empty,
    /// The handle names the picture the bracket holds mutably.
    Current,
    /// A reference picture, disjoint from the bracket's mutable half.
    Other(&'a SPicture),
}

impl RefSlot<'_> {
    /// The resolved picture's `iFramePoc`, or `None` for the two arms that carry no
    /// picture. Error concealment's one read of a reference it may also be *writing*
    /// — the `Current` arm answers `None` there.
    #[inline]
    pub fn poc(&self) -> Option<i32> {
        match self {
            RefSlot::Other(pic) => Some(pic.iFramePoc),
            _ => None,
        }
    }
}

impl PicPool {
    /// Slot count — the C's `iCapacity`.
    #[inline]
    pub fn capacity(&self) -> i32 {
        self.slots.len() as i32
    }

    /// The circular cursor — the C's `iCurrentIdx`.
    #[inline]
    pub fn cursor(&self) -> i32 {
        self.cursor
    }

    /// A handle to slot `index`.
    ///
    /// # Panics
    /// If `index` is outside the pool.
    #[inline]
    pub fn id(&self, index: usize) -> PicId {
        self.slots.id(index)
    }

    /// The picture in slot `id`, read-only, or null when the slot is empty.
    #[inline]
    pub fn slot(&self, id: PicId) -> Option<&SPicture> {
        self.slots.get(id).as_deref()
    }

    /// [`slot`](Self::slot)'s mutable form, for the paths that write through what they
    /// resolve: the DPB's marks and counts, the AU loop's per-picture stamps, error
    /// concealment's destination.
    #[inline]
    pub fn slot_mut(&mut self, id: PicId) -> Option<&mut SPicture> {
        self.slots.get_mut(id).as_deref_mut()
    }

    /// The picture in slot `index`, or `None` if `index` is outside the pool.
    ///
    /// The out-of-range arm is the C's own: `welsDecoderExt.cpp`'s release path
    /// tests the index against `iCapacity` before indexing and means "no picture"
    /// by a failed test.
    #[inline]
    pub fn slot_at_mut(&mut self, index: i32) -> Option<&mut SPicture> {
        if index >= 0 && index < self.capacity() {
            let id = self.id(index as usize);
            self.slot_mut(id)
        } else {
            None
        }
    }

    /// **The bracket top**: the picture being decoded as `&mut`, and a view of every
    /// other slot — [`Pool::mut_and_rest`] in the decoder's terms.
    ///
    /// This is what the three slice brackets, the DPB regions and error concealment's
    /// copy operations open with: below one of these the pool is not reached at all,
    /// so the whole scope runs on a single borrow.
    ///
    /// **The view carries no address for the current slot**, so the current picture
    /// is answered by *identity* (`RefSlot::Current`) and the caller supplies the
    /// picture it is already holding.
    #[inline]
    pub fn cur_and_rest_mut(&mut self, cur: PicId) -> (Option<&mut SPicture>, PicRefs<'_>) {
        let (slot, rest) = self.slots.mut_and_rest(cur);
        (slot.as_deref_mut(), PicRefs::split(rest, cur))
    }

    /// A whole-pool read view, for a bracket that holds no picture mutably.
    #[inline]
    pub fn refs(&self) -> PicRefs<'_> {
        PicRefs::over(Some(self))
    }

    /// **The recycling predicate**, and the whole of what "free" means to this pool:
    /// a slot holds a recyclable picture when it holds a picture at all and that
    /// picture is [`SPicture::is_free`] — `!bUsedAsRef && iRefCount <= 0`.
    #[inline]
    fn is_recyclable(&self, index: usize) -> bool {
        match self.slots.get(self.id(index)) {
            Some(pic) => pic.is_free(),
            None => false,
        }
    }

    /// `PrefetchPic`'s two-pass circular scan for a recyclable slot.
    ///
    /// Pass 1 walks `cursor + 1 .. capacity`; pass 2 wraps and walks `0 ..= cursor`.
    /// The cursor lands on the winning index, or — when pass 2 finds nothing — one
    /// past where it stopped, which is the C's behaviour and the reason its own loop
    /// can run off the end of `ppPic`: each failed prefetch leaves `iCurrentIdx` one
    /// higher, so an exhausted DPB eventually indexes past `iCapacity`. The port
    /// already guarded that with `iPicIdx < iCapacity`; here the bound is the pool's.
    pub fn prefetch_free(&mut self) -> Option<PicId> {
        let capacity = self.capacity();
        if capacity == 0 {
            return None;
        }

        // Pass 1: forward from cursor + 1.
        let mut index = self.cursor + 1;
        while index < capacity {
            if self.is_recyclable(index as usize) {
                self.cursor = index;
                self.stamp_buff_idx(index);
                return Some(self.id(index as usize));
            }
            index += 1;
        }

        // Pass 2: wrap to 0 and walk up to and including the cursor.
        index = 0;
        let mut found = None;
        while index <= self.cursor && index < capacity {
            if self.is_recyclable(index as usize) {
                found = Some(index);
                break;
            }
            index += 1;
        }

        self.cursor = index;
        let found = found?;
        // `index == found` here: the loop above breaks without advancing, and the
        // `found?` above is the only way past a scan that did not.
        self.stamp_buff_idx(found);
        Some(self.id(found as usize))
    }

    /// A pool over pictures that already exist.
    ///
    /// [`CreatePicBuff`]'s tail, named — it is also the only way a fixture can put its
    /// own pictures into a pool.
    pub fn over(slots: Vec<PicSlot>) -> Box<Self> {
        let mut pool = Box::new(PicPool { slots: Pool::new(slots), cursor: 0 });
        pool.stamp_slots();
        pool
    }

    /// Tells every picture which slot it is in.
    ///
    /// Called once, from [`CreatePicBuff`], before the pool is reachable from
    /// anything else. A picture never moves between slots, so this is the only
    /// assignment its [`PicId`] ever gets — which is what makes slot equality a
    /// usable identity where `iPicBuffIdx`, written at prefetch, is not.
    fn stamp_slots(&mut self) {
        let ids: Vec<PicId> = self.slots.ids().collect();
        for id in ids {
            if let Some(pic) = self.slots.get_mut(id) {
                pic.set_pic_id(id);
            }
        }
    }

    /// `PrefetchPicForThread`'s round-robin step: the slot under the cursor, and the
    /// cursor advanced one with a wrap.
    pub fn next_for_thread(&mut self) -> Option<PicId> {
        let capacity = self.capacity();
        if capacity == 0 {
            return None;
        }

        let taken = self.cursor;
        let occupied = self.stamp_buff_idx(taken);

        self.cursor += 1;
        if self.cursor >= capacity {
            self.cursor = 0;
        }
        if occupied {
            Some(self.id(taken as usize))
        } else {
            None
        }
    }

    /// Writes `iPicBuffIdx` into the picture at `index`, and answers whether there
    /// was one — the two scans' shared stamp.
    #[inline]
    fn stamp_buff_idx(&mut self, index: i32) -> bool {
        if index < 0 || index >= self.capacity() {
            return false;
        }
        let id = self.id(index as usize);
        match self.slots.get_mut(id) {
            Some(pic) => {
                pic.iPicBuffIdx = index;
                true
            }
            None => false,
        }
    }
}

pub use crate::decoder::decoder_context::SWelsDecoderContext;

// ============================================================================
// Helper Macros / Inline Functions
// ============================================================================

/// Alignment calculation macro matching `WELS_ALIGN(x, n)`.
#[inline]
pub const fn WELS_ALIGN(x: i32, n: i32) -> i32 {
    (x + (n - 1)) & !(n - 1)
}

pub use crate::decoder::decoder_core::GetThreadCount;

// ============================================================================
// Picture Memory Lifecycle Functions
// ============================================================================

/// `len` bytes of `fill`, or `None` if the allocation fails.
///
/// The C's `WelsMallocz` returned null on failure and `AllocPicture`'s callers all
/// test for it; `vec![fill; len]` would abort the process instead. `try_reserve_exact`
/// keeps the C's contract.
fn try_filled(len: usize, fill: u8) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len).ok()?;
    buf.resize(len, fill);
    Some(buf)
}

/// Allocates and initializes an [`SPicture`] container with its three owned sample
/// planes and its macroblock tracking metadata arrays.
///
/// `None` is the C's null return.
pub fn alloc_picture(
    bParseOnly: bool,
    kiPicWidth: i32,
    kiPicHeight: i32,
) -> Option<Box<SPicture>> {
    let iPicWidth = WELS_ALIGN(kiPicWidth + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);
    let iPicHeight = WELS_ALIGN(kiPicHeight + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);
    let iPicChromaWidth = iPicWidth >> 1;
    let iPicChromaHeight = iPicHeight >> 1;

    let iLumaSize = iPicWidth * iPicHeight;
    let iChromaSize = iPicChromaWidth * iPicChromaHeight;

    let planes: [PaddedPlane; 3] = if bParseOnly {
        // The C set `iLinesize[i]` from the geometry and left `pData[i]`/`pBuffer[i]`
        // null: a parse-only decode reconstructs nothing. Strides, no bytes.
        [
            PaddedPlane::empty(iPicWidth as usize),
            PaddedPlane::empty(iPicChromaWidth as usize),
            PaddedPlane::empty(iPicChromaWidth as usize),
        ]
    } else {
        // One `WelsMallocz` of `iLumaSize + 2*iChromaSize` filled with 128, carved
        // into three by pointer arithmetic, became three allocations each filled with
        // 128. Nothing walks from one plane into the next — `pBuffer[1]` and
        // `pBuffer[2]` were only ever bases for their own plane's `pData` — so the
        // contiguity was incidental.
        let (Some(y), Some(u), Some(v)) = (
            try_filled(iLumaSize as usize, 128),
            try_filled(iChromaSize as usize, 128),
            try_filled(iChromaSize as usize, 128),
        ) else {
            return None;
        };
        // `AllocPicture`'s own origin expressions, kept verbatim. Both are
        // `pad*stride + pad` — luma at pad 32, chroma at pad 16 — and `from_parts`
        // recovers the pad by division, so it *checks* that identity rather than
        // assuming it. It also checks that the C's allocation is tall enough for the
        // padded picture, which the row-count alignment makes true with room over.
        let origin_y = ((1 + iPicWidth) * PADDING_LENGTH) as usize;
        let origin_c = (((1 + iPicChromaWidth) * PADDING_LENGTH) >> 1) as usize;
        [
            PaddedPlane::from_parts(
                y,
                iPicWidth as usize,
                origin_y,
                kiPicWidth as usize,
                kiPicHeight as usize,
            ),
            PaddedPlane::from_parts(
                u,
                iPicChromaWidth as usize,
                origin_c,
                (kiPicWidth >> 1) as usize,
                (kiPicHeight >> 1) as usize,
            ),
            PaddedPlane::from_parts(
                v,
                iPicChromaWidth as usize,
                origin_c,
                (kiPicWidth >> 1) as usize,
                (kiPicHeight >> 1) as usize,
            ),
        ]
    };
    let _ = iPicChromaHeight;

    // `AllocPicture`'s own macroblock geometry, unchanged — the six `WelsMallocz`
    // calls that used to follow were all sized `uiMbCount * elem`, and this is that
    // count, stated once and handed to the containers.
    let dims = MbDims::new(
        ((kiPicWidth + 15) >> 4) as usize,
        ((kiPicHeight + 15) >> 4) as usize,
    );

    let mut pic = Box::new(SPicture::with_planes(planes, dims));

    pic.iWidthInPixel = kiPicWidth;
    pic.iHeightInPixel = kiPicHeight;
    pic.iFrameNum = -1;
    pic.iRefCount = 0;
    pic.pSetUnRef = None;

    Some(pic)
}

// ============================================================================
// Queue Retrieval Interface Routines
// ============================================================================

/// Retrieves the next circular picture node in round-robin FIFO sequence for multi-threaded decoding.
pub fn PrefetchPicForThread(pPicBuf: Option<&mut PicPool>) -> Option<&mut SPicture> {
    let pool = pPicBuf?;
    let id = pool.next_for_thread()?;
    pool.slot_mut(id)
}

/// Retrieves an explicit picture node by its recorded buffer pool index (`iLastPicBuffIdx`).
pub fn PrefetchLastPicForThread(
    pPicBuf: Option<&mut PicPool>,
    iLastPicBuffIdx: i32,
) -> Option<&mut SPicture> {
    let pool = pPicBuf?;
    // `slot_at_mut`'s range test, spelled here.
    if iLastPicBuffIdx < 0 || iLastPicBuffIdx >= pool.capacity() {
        return None;
    }
    let id = pool.id(iLastPicBuffIdx as usize);
    pool.slot_mut(id)
}

// ============================================================================
// Buffer Pool Lifecycle Helpers (CreatePicBuff / DestroyPicBuff)
// ============================================================================

/// Allocates a [`PicPool`] and pre-allocates `kiSize` [`SPicture`] nodes into it.
///
/// The partial-failure arm frees what it has already built, which is what the C++
/// means by `decoder.cpp:91`'s `pPicBuf->iCapacity = iPicIdx;` and its comment
/// "init capacity first for free memory".
///
/// `None` is the C's `1` return.
pub fn CreatePicBuff(
    bParseOnly: bool,
    kiSize: i32,
    kiPicWidth: i32,
    kiPicHeight: i32,
) -> Option<Box<PicPool>> {
    let mut slots: Vec<PicSlot> = Vec::new();
    if slots.try_reserve_exact(kiSize.max(0) as usize).is_err() {
        return None;
    }

    {
        for _ in 0..kiSize {
            let Some(pic) = alloc_picture(bParseOnly, kiPicWidth, kiPicHeight) else {
                return None;
            };
            slots.push(Some(pic));
        }

        Some(PicPool::over(slots))
    }
}

/// `IncreasePicBuff` — `decoder.cpp:107`.
///
/// A stream that raises its reference-frame count without changing resolution takes
/// `WelsRequestMem`'s third arm (`decoder.cpp:493-509`), which resizes the pool in
/// place and keeps decoding.
///
/// **Handles survive this.** The C++ `memcpy`s the old `PPicture` array into the front
/// of the new one, so a picture keeps its position; [`Pool::grow`] appends and leaves
/// every existing index and generation alone. `iCurrentIdx` is carried over for the
/// same reason — it names a slot that has not moved.
///
/// The C++'s partial-failure arm (`iCapacity = iPicIdx`, then `DestroyPicBuff`) is the
/// `Vec` going out of scope on the early return, exactly as in [`CreatePicBuff`].
pub fn IncreasePicBuff(
    pool: &mut PicPool,
    bParseOnly: bool,
    kiOldSize: i32,
    kiPicWidth: i32,
    kiPicHeight: i32,
    kiNewSize: i32,
) -> i32 {
    if kiOldSize <= 0 || kiNewSize <= 0 || kiPicWidth <= 0 || kiPicHeight <= 0 {
        return ERR_INFO_INVALID_PARAM;
    }

    let mut extra: Vec<PicSlot> = Vec::new();
    for _ in kiOldSize..kiNewSize {
        let Some(pic) = alloc_picture(bParseOnly, kiPicWidth, kiPicHeight) else {
            return ERR_INFO_OUT_OF_MEMORY;
        };
        extra.push(Some(pic));
    }
    pool.slots.grow(extra);

    ResetPoolPictureFlags(pool);
    ERR_NONE
}

/// `DecreasePicBuff` — `decoder.cpp:170`. The shrinking half of the same arm.
///
/// Not a truncation: when the DPB's previously-decoded picture sits beyond the new
/// size the C++ moves it to slot 0 and shifts the first `newSize - 1` slots up by one,
/// so `order` below is a permutation and [`Pool::reorder_and_shrink`] takes it as one.
/// The pictures no index names are dropped, which is the C++'s `FreePicture` loop and
/// its `if (iPrevPicIdx != iPicIdx)` guard — the guard is unnecessary here because a
/// value can only be moved out of the old vector once.
///
/// `prev` is the caller's `pPreviousDecodedPictureInDpb`; the returned `Option<PicId>`
/// is where it now lives and the caller **must** store it back. In the C++ that
/// re-derivation is free — the `PPicture` follows its picture — and here it is not,
/// because identity is the slot. Everything else that could name a slot across this
/// call is cleared rather than remapped, and that is the C++'s own list:
///
/// * the three reference lists — `WelsResetRefPic`, which `AllocPicBuffOnNewSeqBegin`
///   runs before `SyncPictureResolutionExt` (`decoder.cpp:489`);
/// * every picture's own `pRefPic` graph — cleared below, oss-fuzz 14423;
/// * the reordering buffers — [`ResetReorderingPictureBuffers`], called below;
/// * `pDec` — the caller nulls it, as `decoder.cpp:537` does for every arm that got
///   this far;
/// * `pECRefPic` — rebuilt from `pRefList` at each use (`error_concealment.rs:691`
///   clears all sixteen before filling them), and `pRefList` is one of the three above.
///
/// A slot that keeps its own value keeps its handles, so the common shrink — no
/// reorder — invalidates nothing at all.
///
/// [`ResetReorderingPictureBuffers`]: crate::decoder::decoder_core::ResetReorderingPictureBuffers
pub fn DecreasePicBuff(
    pCtx: &mut SWelsDecoderContext,
    kiOldSize: i32,
    kiPicWidth: i32,
    kiPicHeight: i32,
    kiNewSize: i32,
) -> i32 {
    if kiOldSize <= 0 || kiNewSize <= 0 || kiPicWidth <= 0 || kiPicHeight <= 0 {
        return ERR_INFO_INVALID_PARAM;
    }

    {
        let SWelsDecoderContext { pPictReoderingStatus, pPictInfoList, .. } = &mut *pCtx;
        crate::decoder::decoder_core::ResetReorderingPictureBuffers(
            pPictReoderingStatus,
            pPictInfoList,
            false,
        );
    }

    let old_size = kiOldSize as usize;
    let new_size = kiNewSize as usize;
    // The C++ searches the old array for the pointer and leaves `iPrevPicIdx ==
    // kiOldSize` when it is not there; here the id *is* the index, and `None` is the
    // "not found" the search reported.
    let prev = pCtx.pLastDecPicInfo.pPreviousDecodedPictureInDpb;
    let iPrevPicIdx = prev.map_or(old_size, |id| id.index());

    let (order, cursor, prev_moved_to_front): (Vec<usize>, i32, bool) =
        if iPrevPicIdx < old_size && iPrevPicIdx >= new_size {
            // found, and beyond the new size: it becomes slot 0 and the rest shift up
            let mut order = Vec::with_capacity(new_size);
            order.push(iPrevPicIdx);
            order.extend(0..new_size - 1);
            (order, 0, true)
        } else {
            // either not found, or already inside the new size and staying put
            let cursor = if iPrevPicIdx < new_size { iPrevPicIdx as i32 } else { 0 };
            ((0..new_size).collect(), cursor, false)
        };

    let Some(pool) = pCtx.pPicBuff.as_deref_mut() else {
        return ERR_INFO_INVALID_PARAM;
    };
    // The dropped pictures are the C++'s `FreePicture` set.
    drop(pool.slots.reorder_and_shrink(&order));
    pool.cursor = cursor;

    // "all references' references have to be reset" — oss-fuzz 14423. The C++ walks
    // each list until the first null; clearing the whole array is the same state and
    // does not depend on the array being null-terminated.
    for id in pool.slots.ids().collect::<Vec<_>>() {
        if let Some(pic) = pool.slots.get_mut(id).as_deref_mut() {
            pic.pRefPic = [[None; 17]; LIST_A];
        }
    }
    ResetPoolPictureFlags(pool);

    if prev_moved_to_front {
        // Re-derived, not carried: the picture moved, so its old id names another
        // slot now (and would fault the generation check in a debug build).
        let front = pool.id(0);
        pCtx.pLastDecPicInfo.pPreviousDecodedPictureInDpb = Some(front);
    }
    ERR_NONE
}

/// The five per-picture fields both resize paths reset over the *whole* new pool —
/// `decoder.cpp:150-157` and `:240-246`, character for character the same loop in
/// both, including that it touches the pictures that were kept and not only the ones
/// that were just allocated.
fn ResetPoolPictureFlags(pool: &mut PicPool) {
    for id in pool.slots.ids().collect::<Vec<_>>() {
        if let Some(pic) = pool.slots.get_mut(id).as_deref_mut() {
            pic.bUsedAsRef = false;
            pic.bIsLongRef = false;
            pic.iRefCount = 0;
            pic.pSetUnRef = None;
            pic.bIsComplete = false;
        }
    }
}

/// Releases every picture the pool addresses, then the pool.
///
/// # The reordering reset
///
/// C++ `decoder.cpp:260` opens this function with
/// `ResetReorderingPictureBuffers (pCtx->pPictReoderingStatus, pCtx->pPictInfoList,
/// false)`; the reset runs before the early returns, exactly where the C++ has it.
/// Without it, `sPictInfoList` keeps POCs and `iPicBuffIdx` values naming slots of a
/// pool that has been freed and rebuilt, and `EmitBufferedPicture` indexes the new
/// pool with the old picture's index. It matters because `DestroyPicBuff` also runs
/// on a pool rebuild inside one session (`WelsFreeDynamicMemory`), where nothing is
/// dropped.
pub fn DestroyPicBuff(pCtx: &mut SWelsDecoderContext, pool: Option<Box<PicPool>>) {
    // The reset, at the head of the function and before the early returns, where
    // `decoder.cpp:260` has it. The pair is destructured for disjointness alone.
    let SWelsDecoderContext { pPictReoderingStatus, pPictInfoList, .. } = &mut *pCtx;
    crate::decoder::decoder_core::ResetReorderingPictureBuffers(
        pPictReoderingStatus,
        pPictInfoList,
        false,
    );

    drop(pool);
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::decoder::decoder_context::parse_only;
    use super::*;
    
    #[test]
    fn test_picture_alignment_geometry() {
        let width = 320;
        let height = 240;
        let aligned_w = WELS_ALIGN(width + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);
        let aligned_h = WELS_ALIGN(height + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);

        assert_eq!(aligned_w, 384);
        assert_eq!(aligned_h, 320);
        assert_eq!(aligned_w % 32, 0);
        assert_eq!(aligned_h % 32, 0);
    }

    #[test]
    fn test_alloc_and_free_picture() {
        let param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        {
            let mut pic = alloc_picture(false, 160, 120)
                .expect("the picture allocates");
            let p_pic: &mut SPicture = &mut *pic;
            assert_eq!((*p_pic).iWidthInPixel, 160);
            assert_eq!((*p_pic).iHeightInPixel, 120);
            assert!(!(*p_pic).data_ptr(0).is_null());

            // stride = WELS_ALIGN(160 + 64, 32) = 224, rows = WELS_ALIGN(120 + 64, 32)
            // = 192, so the luma allocation is 224*192 and the padded picture needs
            // 224*(120+64) — the alignment leaves eight spare rows, and `from_parts`
            // accepts that.
            assert_eq!((*p_pic).linesize(0), 224);
            assert_eq!((*p_pic).linesize(1), 112);
            assert_eq!((*p_pic).plane(0).pad(), 32);
            assert_eq!((*p_pic).plane(1).pad(), 16);
            assert_eq!((*p_pic).plane(0).origin(), (1 + 224) * 32);
            assert_eq!((*p_pic).plane(1).origin(), ((1 + 112) * 32) >> 1);
            assert_eq!((*p_pic).plane(0).as_slice().len(), 224 * 192);
            assert_eq!((*p_pic).plane(1).as_slice().len(), 112 * 96);
            // The 128 fill covers the whole allocation, corners included.
            assert!((*p_pic).plane(0).as_slice().iter().all(|&b| b == 128));
            assert_eq!((*p_pic).plane(0).at(-32, -32), 128);

            drop(pic);
        }
    }

    /// The `bParseOnly` arm: strides from the geometry, no sample memory, and a null
    /// `data_ptr` — the three properties the C's null `pData[i]` beside a non-zero
    /// `iLinesize[i]` encoded, which every caller still tests with `.is_null()`.
    #[test]
    fn test_alloc_picture_parse_only_carries_strides_and_no_bytes() {
        let param = SDecodingParam { bParseOnly: true, ..Default::default() };
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        {
            let pCtx = &mut *ctx;
            assert!(
                crate::decoder::decoder_context::parse_only(&pCtx.pParam),
                "the accessor reads the field the callee used to reach for itself"
            );
            let mut pic = alloc_picture(parse_only(&pCtx.pParam), 160, 120)
                .expect("the picture allocates");
            let p_pic: &mut SPicture = &mut *pic;
            assert_eq!((*p_pic).linesize(0), 224);
            assert_eq!((*p_pic).linesize(1), 112);
            assert_eq!((*p_pic).linesize(2), 112);
            assert!((*p_pic).plane(0).is_empty());
            assert!((*p_pic).data_ptr(0).is_null());
            assert!((*p_pic).data_ptr(1).is_null());
            assert!((*p_pic).data_ptr(2).is_null());
            drop(pic);
        }
    }

    #[test]
    fn test_prefetch_pic_circular_scan() {
        let param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        {
            let pCtx = &mut *ctx;
            let mut pool = CreatePicBuff(false, 4, 64, 64).expect("pool");

            // First prefetch gets index 1 (Pass 1 scan from iCurrentIdx + 1)
            let slot1 = pool.prefetch_free().expect("slot 1 is free");
            assert_eq!(slot1.index(), 1);
            assert_eq!(pool.cursor(), 1);

            // Mark pic1 as used as reference
            pool.slot_mut(slot1).unwrap().bUsedAsRef = true;

            // Second prefetch skips index 1, finds index 2
            let slot2 = pool.prefetch_free().expect("slot 2 is free");
            assert_eq!(slot2.index(), 2);
            assert_eq!(pool.cursor(), 2);

            DestroyPicBuff(pCtx, Some(pool));
        }
    }

    /// **The current slot resolves through the mutable half, and must.**
    ///
    /// A malformed stream can legally put the picture being decoded into a reference
    /// list: `pRefList[i]` is filled from a `ref_idx` the bitstream chooses, and the
    /// C++ resolves it to `pCtx->pDec` and reads on. `PoolRest::get` *panics* on the
    /// slot its split lends out, so answering a reference resolution out of the rest
    /// would turn a decodable-with-garbage stream into an abort.
    #[test]
    fn f42_a_reference_list_entry_naming_the_current_picture_resolves_not_panics() {
        let param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        let mut pool = CreatePicBuff(false, 3, 64, 64).expect("pool");
        let (cur, other) = (pool.id(1), pool.id(2));

        {
            let (pCur, refs) = pool.cur_and_rest_mut(cur);
            let pCur = pCur.expect("the current slot holds a picture");

            // The malformed case: a reference resolution naming the current slot. It
            // resolves — it does not panic through `rest`, and it does not answer
            // `Empty`.
            assert!(
                matches!(refs.classify(Some(cur)), RefSlot::Current),
                "a list entry naming the slot the bracket holds is `Current`"
            );

            // Interleaved write-through-current and read-through-reference, which is
            // what motion compensation off a self-referencing list does. Each read
            // resolves afresh, as the decode path's do.
            pCur.iFramePoc = 77;
            assert_eq!(refs.resolve(Some(cur), Some(pCur)).map(|p| p.iFramePoc), Some(77));
            pCur.iFramePoc = 78;
            assert_eq!(
                refs.resolve(Some(cur), Some(pCur)).map(|p| p.iFramePoc),
                Some(78),
                "the reference answer is the caller's own borrow, so it cannot go stale"
            );

            // The ordinary case still goes through the rest, and is a different
            // picture: the rule widens nothing.
            let pOther = refs.resolve(Some(other), Some(pCur)).expect("slot 2 holds a picture");
            assert_eq!(pOther.pic_id(), Some(other));
            assert_ne!(pOther.pic_id(), pCur.pic_id());
        }

        {
            DestroyPicBuff(&mut *ctx, Some(pool));
        }
    }

    /// Destroying the pool resets the reordering buffers, because they outlive it.
    ///
    /// The pool-rebuild *inside* one session is what the reset guards:
    /// `WelsFreeDynamicMemory` destroys and re-creates the pool without touching the
    /// context, and without the reset the new pool is indexed with the old one's
    /// `iPicBuffIdx`.
    #[test]
    fn destroying_the_pool_resets_the_reordering_buffers() {
        use crate::decoder::decoder_context::{IMinInt32, SPictInfo, SPictReoderingStatus};

        let param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        // The decoder object's own members, and a decode's leavings in them: two
        // buffered pictures naming pool slots 2 and 3.
        ctx.pPictReoderingStatus.iLargestBufferedPicIndex = 1;
        ctx.pPictReoderingStatus.iNumOfPicts = 2;
        ctx.pPictReoderingStatus.bHasBSlice = true;
        ctx.pPictInfoList[0].iPOC = 4;
        ctx.pPictInfoList[0].iPicBuffIdx = 2;
        ctx.pPictInfoList[1].iPOC = 8;
        ctx.pPictInfoList[1].iPicBuffIdx = 3;

        {
            let pool = CreatePicBuff(false, 4, 64, 64);
            assert!(pool.is_some());
            DestroyPicBuff(&mut *ctx, pool);
        }

        // `fullReset = false`, so the loop covers `iLargestBufferedPicIndex + 1` entries
        // — the two that were written — and leaves the untouched tail alone.
        assert_eq!(ctx.pPictReoderingStatus.iNumOfPicts, 0);
        assert_eq!(ctx.pPictReoderingStatus.iLargestBufferedPicIndex, 0);
        assert!(!ctx.pPictReoderingStatus.bHasBSlice);
        assert_eq!(ctx.pPictReoderingStatus.iMinPOC, IMinInt32);
        for i in 0..2 {
            assert_eq!(ctx.pPictInfoList[i].iPicBuffIdx, -1, "slot {i} still names the freed pool");
            assert_eq!(ctx.pPictInfoList[i].iPOC, IMinInt32, "slot {i}");
        }
    }

    /// `same_picture`'s slot arm — two **pooled** pictures carrying one POC are still
    /// two references.
    #[test]
    fn pooled_pictures_are_identified_by_slot_not_by_poc() {
        use crate::decoder::picture::same_picture;

        let param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        {
            let mut pool = CreatePicBuff(false, 2, 64, 64)
                .expect("pool");
            let (id_a, id_b) = (pool.id(0), pool.id(1));

            assert_eq!(pool.slot(id_a).unwrap().pic_id(), Some(id_a), "a picture knows its slot");
            assert_eq!(pool.slot(id_b).unwrap().pic_id(), Some(id_b));

            pool.slot_mut(id_a).unwrap().iFramePoc = 4;
            pool.slot_mut(id_b).unwrap().iFramePoc = 4; // duplicate POC, distinct slots
            let (a, b) = (pool.slot(id_a), pool.slot(id_b));
            assert!(!same_picture(a, b), "two slots are two references");
            assert!(same_picture(a, a));

            // A picture outside the pool has no slot and is its own identity only.
            let mut loose = SPicture::default();
            let mut loose2 = SPicture::default();
            loose.iFramePoc = 4;
            loose2.iFramePoc = 4; // same POC as each other and as the pooled pair
            let l: &SPicture = &loose;
            let l2: &SPicture = &loose2;
            assert_eq!(l.pic_id(), None);
            assert!(same_picture(Some(l), Some(l)));
            assert!(!same_picture(Some(l), Some(l2)), "and POC joins nothing");
            assert!(!same_picture(Some(l), a));

            assert!(same_picture(None, None));
            assert!(!same_picture(None, a));

            DestroyPicBuff(&mut *ctx, Some(pool));
        }
    }

    /// Pass 2 and the cursor's exhausted state — the part of `PrefetchPic` that has
    /// no C++ counterpart to compare against, because the C's loop runs off the end
    /// of `ppPic` where this one stops at the pool's bound.
    ///
    /// With every slot held as a reference the scan finds nothing, and each failed
    /// call leaves the cursor one higher until it reaches `capacity` and stays there.
    /// Releasing a slot then has to be found by the wrap, since the cursor is past it.
    #[test]
    fn prefetch_wraps_and_survives_an_exhausted_pool() {
        let param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        {
            let mut pool = CreatePicBuff(false, 3, 64, 64)
                .expect("pool");
            assert_eq!(pool.capacity(), 3);

            // Every slot in use: the two passes both come up empty, and the cursor
            // climbs one per call and then stops at the capacity rather than past it.
            for i in 0..3 {
                let id = pool.id(i);
                pool.slot_mut(id).unwrap().bUsedAsRef = true;
            }
            assert!(pool.prefetch_free().is_none());
            assert_eq!(pool.cursor(), 1);
            assert!(pool.prefetch_free().is_none());
            assert_eq!(pool.cursor(), 2);
            assert!(pool.prefetch_free().is_none());
            assert_eq!(pool.cursor(), 3);
            assert!(pool.prefetch_free().is_none());
            assert_eq!(pool.cursor(), 3, "an exhausted cursor stays at the bound");

            // Free slot 0 — behind the cursor, so only the wrap can reach it.
            let zero = pool.id(0);
            pool.slot_mut(zero).unwrap().bUsedAsRef = false;
            let got = pool.prefetch_free().expect("the wrap reaches slot 0");
            assert_eq!(got, pool.id(0));
            assert_eq!(pool.cursor(), 0);
            assert_eq!(pool.slot(got).unwrap().iPicBuffIdx, 0, "the winner learns its slot");

            DestroyPicBuff(&mut *ctx, Some(pool));
        }
    }

    #[test]
    fn test_prefetch_pic_for_thread() {
        let param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        {
            let mut pool = CreatePicBuff(false, 3, 64, 64)
                .expect("pool");
            let pic0 = PrefetchPicForThread(Some(&mut pool)).map(|p| p.iPicBuffIdx);
            assert_eq!(pic0, Some(0));
            assert_eq!(pool.cursor(), 1);

            let pic1 = PrefetchPicForThread(Some(&mut pool)).map(|p| p.iPicBuffIdx);
            assert_eq!(pic1, Some(1));
            assert_eq!(pool.cursor(), 2);

            let pic2 = PrefetchPicForThread(Some(&mut pool)).map(|p| p.iPicBuffIdx);
            assert_eq!(pic2, Some(2));
            assert_eq!(pool.cursor(), 0); // Wraps around

            let pic_lookup = PrefetchLastPicForThread(Some(&mut pool), 1).map(|p| p.iPicBuffIdx);
            assert_eq!(pic_lookup, pic1);
            assert!(
                PrefetchPicForThread(None).is_none(),
                "the null test is the Option"
            );

            DestroyPicBuff(&mut *ctx, Some(pool));
        }
    }
}
