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
// **Phase 5b, T5b.6: this file's `unsafe` is gone and no exception is enumerated.**
// `src/decoder/` carries **three** `#[allow(unsafe_code)]` items in total, and they
// are all in `decoder_context.rs` (`api_alias`/`api_alias_mut`) and `picture.rs` (the
// one Miri provenance test S28 mandates for `data_ptr` — T5b.7 retired the second
// with `data_ptr_ref`). Nothing here is one of them.

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

use std::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;
use crate::decoder::decoder_context::SDecodingParam;
use crate::decoder::decoder_context::api_alias_mut;

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

/// A handle to one slot of the decoder's [`PicPool`] — plan §2.2.3's `PicId`.
///
/// Identity is slot equality, which is the predicate the P3 tests pin: two pictures
/// are "the same reference" when they occupy the same pool slot, never when they
/// merely share a POC.
pub type PicId = crate::safe::pool::Id;

/// The decoder's recycled picture pool — C++ `SPicBuff` (`pic_queue.h:45-49`).
///
/// **T5.N1: the pool addresses, it does not own.** The C's `ppPic`/`iCapacity` pair
/// — a `WelsMallocz`'d array of `SPicture*` plus a length nothing related to it — is
/// one [`Pool`] of slots, so a slot index is bounds-checked once by the container
/// instead of by each of the four scans that walked it. What has **not** moved is
/// ownership: [`AllocPicture`]'s `Box::into_raw` is still the constructor and
/// [`FreePicture`]'s `Box::from_raw` still the dropper (T5.C3's pair), so F19's
/// check — *which line frees this?* — has the same answer it had before, and the
/// pool is not a second owner.
///
/// **T5.Q2 — the slots own.** The paragraph that stood here explained why they could
/// not: `pCtx->pDec` was a raw pointer *into* a slot, so a pool-issued `&mut` and a
/// live alias to the same picture were the S25 overlap with nothing to discharge it.
/// W2a and W2b removed every such alias — `pDec`, `pECRefPic`, the layer's copy and
/// both reference lists carry [`PicId`]s — and W3's three earlier seams moved the
/// decode path's resolutions to three slice bracket tops. What is left is this type,
/// and with it the pool is the pictures' one owner: [`Pool::mut_and_rest`] proves the
/// current-vs-reference split in safe code, and drop glue reaches every picture.
///
/// F19, decoder-side, closes here. `AllocPicture`'s `Box::into_raw`/`FreePicture`'s
/// `Box::from_raw` pair is gone from the pool's lifecycle: [`CreatePicBuff`] pushes
/// owners into a `Vec` and [`DestroyPicBuff`] drops them. R4 — "the port frees
/// exactly what the C++ frees" — is discharged by construction rather than by
/// inspection, because there is no longer a spelling in which a slot can be dropped
/// without its picture.
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

/// The C's name for [`PicPool`], kept at the raw-pointer alias for the same reason
/// `PDqLayer` keeps its own (T5.M1): it is a pointer *to* the pool, and Phase 5's
/// remaining steps delete it rather than convert it.
pub type SPicBuff = PicPool;
/// **Phase 8's** (T5b.9). Nothing in `src/decoder/` names this type; its two uses
/// are `api/codec_api.rs`'s `pCtx ? pCtx->pPicBuff : m_pPicBuff` pair, which is the
/// C++'s own expression at the boundary F23 owns. It retires with the boundary.
pub type PPicBuff = *mut PicPool;

/// **A decode bracket's view of the pool** (T5.P″2): `PicId` → picture, with the
/// pool reached once at the bracket top and nowhere below it.
///
/// This is the type W3's settlement is built on. With owned slots a resolution stops
/// being a copy and becomes a derivation through the slot's `Box`, so two live
/// results conflict and per-use resolution cannot survive; the answer is a scope —
/// **the slice** for the decode path, one operation for EC, DPB and output — that
/// borrows the pool at its top and threads this view down. Everything below reads
/// `PicId`s out of the context and resolves them *here*, never through
/// `(*pCtx).pPicBuff`.
///
/// Under `PPicture` slots (the hoist, T5.P″2) it wraps a shared borrow of the pool
/// and [`get`](Self::get) is a slot copy, so threading it changes nothing a byte
/// gate can see. At the flip it becomes the `PoolRest` half of
/// [`Pool::mut_and_rest`] and the current picture becomes the `&mut` half — which is
/// why the two travel together through every signature this face touches.
#[derive(Clone, Copy, Debug)]
pub struct PicRefs<'a> {
    view: PicView<'a>,
}

/// The three states a bracket's view can be in.
///
/// [`Split`](PicView::Split) is the one the decode brackets take and the one the
/// settlement is about; the other two exist because a bracket can open with no pool
/// (`None` before `CreatePicBuff` and after `DestroyPicBuff` — the state `pool_pic`'s
/// null arm was testing for) or with no current picture, and both of those were
/// reachable before the flip and must stay reachable after it.
#[derive(Clone, Copy, Debug)]
enum PicView<'a> {
    /// No pool.
    None,
    /// A pool with no slot held mutably.
    Whole(&'a PicPool),
    /// [`PicPool::cur_and_rest_mut`]'s half: one slot held mutably by the caller,
    /// every other readable — [`Pool::mut_and_rest`]'s halves. The current picture
    /// is the caller's `&mut`, so this side keeps its *identity* and no address:
    /// there is nothing here for a retag on that borrow to invalidate, which is what
    /// [`PicRefs::classify`] and [`PicRefs::resolve`] are built on (**F42**).
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
    /// those the current picture is just another source: a shared reborrow of the
    /// caller's `&mut` coexists with the `&SPicture` the rest hands out, because both
    /// are shared. So **F42's arm costs a parameter here and nothing else**, where
    /// the pointer form it replaces cost every caller its `unsafe`.
    ///
    /// The current slot is still never resolved through [`PoolRest::get`], which
    /// panics on it — that is what `cur` is kept for. Motion compensation, which
    /// writes while it reads, cannot use this and asks [`classify`](Self::classify)
    /// directly.
    ///
    /// `cur` is an `Option` because a bracket can open on an empty slot: the pool
    /// hands back no picture and this view still knows the slot's *identity*, which
    /// is the state the pointer form answered with a null. `None` here is that null,
    /// arm for arm.
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
    ///
    /// [`RefSlot::Current`] carries no reference **by construction**, so the
    /// `&SPicture` in [`RefSlot::Other`] is provably disjoint from the `&mut` the
    /// bracket holds and the two travel together in safe code. No behaviour moves:
    /// the arms are the same three the pointer form's callers were writing by hand,
    /// and the current slot is still never resolved through `PoolRest::get`, which
    /// panics on it.
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
                    RefSlot::Current // F42 — never `rest.get(cur)`, which panics.
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

/// [`PicRefs::classify`]'s answer — the three states its callers were testing for
/// with a null check and a `same_picture` comparison.
#[derive(Debug)]
pub enum RefSlot<'a> {
    /// No pool, no handle, or an empty slot — the pointer form's null.
    Empty,
    /// The handle names the picture the bracket holds mutably (**F42**).
    Current,
    /// A reference picture, disjoint from the bracket's mutable half.
    Other(&'a SPicture),
}

impl RefSlot<'_> {
    /// The resolved picture's `iFramePoc`, or `None` for the two arms that carry no
    /// picture. Error concealment's one read of a reference it may also be *writing*
    /// — the `Current` arm answers `None` there, which is what the pointer form's
    /// `map(|pic| pic.iFramePoc)` over a null answered.
    #[inline]
    pub fn poc(&self) -> Option<i32> {
        match self {
            RefSlot::Other(pic) => Some(pic.iFramePoc),
            _ => None,
        }
    }
}

// **T5b.9: `slot_ptr` deleted dead with `slot_at` (S18).** Its doc named three
// consumers needing an address — `PicRefs::get`'s F42 arm, `slot_at`, and the
// thread prefetch F36 owns. The first became an identity at T5b.1 and the third
// went with F36's probe, so the shared form was left serving only `slot_at`, which
// had no callers of its own. The mutable form below still has one, across the ABI.

/// The pointer form of a resolved slot, spelled **once** (S7) — so that
/// [`PicPool::slot_at_mut`], the boundary's accessor, is the only place in the
/// decoder that writes the cast.
#[inline]
fn slot_ptr_mut(p: Option<&mut SPicture>) -> *mut SPicture {
    match p {
        Some(p) => p,
        None => std::ptr::null_mut(),
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
    ///
    /// **One derivation, ended by the caller's expression.** Under owned slots this
    /// is no longer a pointer copy: it borrows the slot's `Box` and hands out its
    /// pointee's address, so two *live* results for the same slot are two derivations
    /// of one allocation and the later one invalidates the earlier. Coexisting shared
    /// results are fine — that is the discriminator the write paths are read against
    /// — but a result that outlives the expression it was taken in wants a bracket
    /// ([`cur_and_rest`](Self::cur_and_rest)), not this.
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

    // **T5b.9: the shared `slot_at` is deleted dead (S18).** Of the two C++ index
    // paths its doc named, `PrefetchLastPicForThread` is D3's threaded decoder and
    // `welsDecoderExt.cpp`'s release path reaches the *mutable* form below; the
    // shared one has had no caller since T5b.1.

    /// The picture in slot `index`, or null if `index` is outside the pool.
    ///
    /// The out-of-range arm is the C's own: `welsDecoderExt.cpp`'s release path
    /// tests the index against `iCapacity` before indexing and means "no picture"
    /// by a failed test. **Phase 8's**, with [`PPicture`]: the one consumer is
    /// `api/codec_api.rs`, across the C ABI.
    #[inline]
    pub fn slot_at_mut(&mut self, index: i32) -> PPicture {
        if index >= 0 && index < self.capacity() {
            let id = self.id(index as usize);
            slot_ptr_mut(self.slot_mut(id))
        } else {
            std::ptr::null_mut()
        }
    }

    /// **The bracket top**: the picture being decoded as `&mut`, and a view of every
    /// other slot — [`Pool::mut_and_rest`] in the decoder's terms.
    ///
    /// This is what the three slice brackets, the DPB regions and error concealment's
    /// copy operations open with, and the reason the hoist came first: below one of
    /// these the pool is not reached at all, so the whole scope runs on a single
    /// borrow.
    ///
    /// **The view carries no address for the current slot**, and that is structural
    /// rather than documentary: handing back the `&mut` *and* a pointer to the same
    /// picture would be two live derivations of one allocation, and the caller's
    /// first retag would pop the pointer. There is none to pop —
    /// [`PicView::Split`] does not have the field — so F42's arm is answered by
    /// *identity* (`RefSlot::Current`) and the caller supplies the picture it is
    /// already holding. **T5b.2 retired the pointer form of this bracket**: with
    /// `PicRefs::resolve` for the readers and `mc_luma_same` for the one writer, no
    /// caller in the decoder needs an address for the current picture, and the
    /// twenty-three `#[allow(unsafe_code)]` signatures that carried one are gone.
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
    ///
    /// Both scans below used to spell this inline, which is why the null test and the
    /// two flags could drift apart between them.
    ///
    /// **Safe now**, and that is the flip arriving at the smallest place it changes:
    /// the predicate used to deref a slot pointer the pool did not own, so it carried
    /// the whole "every slot is null or a live `AllocPicture`" contract. The slot is
    /// the owner, so the contract is the type's.
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
    ///
    /// **The slot, not the picture** (W3's settled fact 4). Both callers want the
    /// slot — one stores it straight into `pCtx->pDec`, the other writes several
    /// fields through it — and with owned slots a `&mut SPicture` return would borrow
    /// the pool for the whole of the caller's expression. A `PicId` borrows nothing.
    ///
    /// It is also the identical value the callers were computing: `pic_slot(pPic)`
    /// read back the stamp [`stamp_slots`](Self::stamp_slots) wrote from this same
    /// iteration, and a picture never moves between slots (T5.N2).
    ///
    /// **Safe since T5.W3**, and the `# Safety` clause it used to carry was already
    /// stale: it read "as `is_recyclable`", and `is_recyclable` became safe at the
    /// flip (T5.Q2 — the slot owns the picture, so the contract is the type's). What
    /// kept the `unsafe fn` was the stamp below going out through `slot_at_mut` and
    /// back in through a deref; the pool owns the slot, so it writes to it directly.
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
    /// own pictures into a pool, which is what a `PicId` field asks of a test now that
    /// `pCtx->pDec` is one (T5.P2).
    ///
    /// **It takes the owners.** The `Vec<PPicture>` this used to accept was the last
    /// place a picture could enter the pool without the pool becoming responsible for
    /// it; a `Vec<PicSlot>` is the same list with that hole closed, and it is why the
    /// function stops being `unsafe`.
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
    ///
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
    ///
    /// Safe since T5.W3, for [`prefetch_free`](Self::prefetch_free)'s reason.
    pub fn next_for_thread(&mut self) -> Option<PicId> {
        let capacity = self.capacity();
        if capacity == 0 {
            return None;
        }

        let taken = self.cursor;
        // `pPic.is_null()` was "the slot at `taken` holds no picture", which the slot
        // now answers directly; the stamp happens on exactly the same condition.
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
    /// was one — the two scans' shared stamp, which each used to spell as a deref of
    /// a [`slot_at_mut`](Self::slot_at_mut) result (T5.W3).
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
//
// S25 for this file (T5.C3, enumerated with the conversion as plan §7.6 asks):
// *who else reaches this `SPicture` while a borrow of it is held?*
//
// The pool is where the question is sharpest, because `SPicBuff.ppPic` is
// a pointer to an array of picture pointers, and `pCtx->pDec` points **into that
// array** — the picture the
// decoder is writing is one of the slots the recycling scan walks. Four answers:
//
// 1. **`PrefetchPic` holds no borrow of a picture.** It reads `bUsedAsRef` and
//    `iRefCount` through the slot pointer, one field per expression, and writes
//    `iPicBuffIdx` on the winner after the scan has stopped. The two other prefetch
//    functions are shorter still. Nothing in this file takes a `&mut SPicture` that
//    spans a call, so the conversion introduces no borrow here at all: an owned
//    plane changes `AllocPicture`/`FreePicture` and leaves the scan untouched.
//    **T5.N1 re-checked this and the answer is unchanged**, because the borrow the
//    pool now takes is of the *slot array*, not of a picture: `is_recyclable` reads
//    one slot and derefs it inside one expression, and `prefetch_free`'s `&mut self`
//    covers `cursor` and the slots — never the pictures those slots point at, which
//    is exactly why the slots are still pointers (see [`PicPool`]).
// 2. **The scan cannot see a half-built picture.** `CreatePicBuff` fills its slot
//    `Vec` before the pool exists at all, so a picture is either absent from the pool
//    or fully constructed — the C's "fill `ppPic`, then set `iCapacity`" ordering,
//    now enforced by construction rather than by statement order. That is what lets
//    `AllocPicture` hand back a `Box::into_raw`.
// 3. **The re-entrancy that does exist is one level up**, in `manage_dec_ref.rs`,
//    where `WelsInitRefList`'s concealment prefetch takes a slot from this pool and
//    copies into it from `pPreviousDecodedPictureInDpb` — another slot of the same
//    array. That pair is enumerated at its own site (T5.C2), guarded by
//    `pRef == prev_pic`, and pinned by the `narrow_16x16_idr_lost` golden row.
// 4. **`FreePicture` is the one place ownership actually moves**, and it is
//    reachable only from `DestroyPicBuff` (which nulls the slot it just freed) and
//    from `decoder_core.rs:1899` for `pTempDec` (which nulls `pCtx->pTempDec`). No
//    other pointer to a freed picture survives either path — which is the same
//    property `Box::from_raw` needs and the reason it can be used here.



/// `len` bytes of `fill`, or `None` if the allocation fails.
///
/// The C's `WelsMallocz` returned null on failure and `AllocPicture`'s callers all
/// test for it; `vec![fill; len]` would abort the process instead. `try_reserve_exact`
/// keeps the C's contract, which is `RawDataBuffer::try_new_zeroed`'s answer to the
/// same question at T3.4 — and it matters more here, because a plane is megabytes.
fn try_filled(len: usize, fill: u8) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len).ok()?;
    buf.resize(len, fill);
    Some(buf)
}

/// Allocates and initializes an [`SPicture`] container with its three owned sample
/// planes and its macroblock tracking metadata arrays.
///
/// **T5.C3: the picture is heap-constructed, not `WelsMallocz`'d.** A struct with
/// owned fields cannot come out of a zeroing malloc (S21/F19), so the header is a
/// `Box` and the planes are [`PaddedPlane`]s. What has *not* moved is the geometry:
/// every expression below is `AllocPicture`'s own arithmetic, because the kernels'
/// output depends on it byte for byte and the goldens are the referee.
///
/// **T5.P″1: this is the constructor, and it returns the owner.** T5.P′3 made the
/// `Box` a *complete* owner — the four per-macroblock families are containers, so
/// drop glue reaches every byte the picture holds — which is what lets a caller keep
/// the `Box` instead of a pointer to it. [`AllocPicture`] below is the raw spelling
/// for the callers that still hand pointers around (the pool, until W3's flip).
///
/// `None` is the C's null return, and it carries the same three failures: no context,
/// no allocator, and a plane allocation that could not be reserved.
///
/// **T5.W3: the context is gone and `bParseOnly` arrives as the `bool` it always
/// was.** The two guards the signature used to carry — a null context and a null
/// `pMemAlign` — gated on things this function stopped using at T5.P′3, when the
/// planes became owned Rust allocations and `pMa` went unread; they were never
/// dropped, only left. They are **kept and moved to the callers**, which are the two
/// places that hold a context to test: `CreatePicBuff` already tested both before its
/// loop, and `decode_slice.rs`'s lazy `pTempDec` arm now tests `pMemAlign` where the
/// callee used to. Neither widens nor narrows — the C++ `AllocPicture` derefs `pMa`
/// unconditionally, so the port's guard was always the more defensive of the two, and
/// it stays exactly as defensive.
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
        // contiguity was incidental, and every plane's own bytes are unchanged.
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

    // **T5.P′3**: `AllocPicture`'s own macroblock geometry, unchanged — the six
    // `WelsMallocz` calls that used to follow were all sized `uiMbCount * elem`, and
    // this is that count, stated once and handed to the containers.
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

// **T5.Q3: `AllocPicture` and `FreePicture` stood here, and the flip deleted their
// callers.** They were the C's raw pair — `Box::into_raw` into a `PPicture` slot,
// `Box::from_raw` out of one — and the whole reason the picture's constructor was
// split in two at T5.P″1. With owned slots `CreatePicBuff` pushes `alloc_picture`'s
// `Box` straight into the `Vec` and `DestroyPicBuff` drops it, so **the decoder holds
// no raw picture allocation at all**: F19's question has no site left to ask at, and
// S18's straggler rule says a definition whose last caller went with a conversion goes
// with it rather than waiting for the sweep.
//
// `FreePicture`'s `pMa` parameter is the fossil worth naming: T5.P′3 emptied its body
// to a `drop` and kept the parameter because the C's lifecycle still spelled the pair.
// The pair is gone, and with it the last place the decoder passed a `CMemoryAlign` to
// free a picture.

// ============================================================================
// Queue Retrieval Interface Routines
// ============================================================================

// T5.P″1: `PrefetchPic(PPicBuff)` stood here — the C's free-function spelling of
// [`PicPool::prefetch_free`], kept "until its two call sites hold a pool rather than
// a pointer to one". `pCtx->pPicBuff` owns its pool now, so both of them do
// (`decoder_core.rs`'s prefetch and `manage_dec_ref.rs`'s concealment prefetch), and
// they call the scan through `pic_pool_mut`.

/// Retrieves the next circular picture node in round-robin FIFO sequence for multi-threaded decoding.
///
/// T5.W3: the pool arrives as `Option<&mut PicPool>` and the null test is the
/// `Option`, unmoved. T5b.6 takes the return the same way — the two have no
/// production caller (F36's list), so nothing outside this module's own test ever
/// wanted the pointer.
pub fn PrefetchPicForThread(pPicBuf: Option<&mut PicPool>) -> Option<&mut SPicture> {
    let pool = pPicBuf?;
    let id = pool.next_for_thread()?;
    pool.slot_mut(id)
}

/// Retrieves an explicit picture node by its recorded buffer pool index (`iLastPicBuffIdx`).
///
/// T5.W3, as [`PrefetchPicForThread`].
pub fn PrefetchLastPicForThread(
    pPicBuf: Option<&mut PicPool>,
    iLastPicBuffIdx: i32,
) -> Option<&mut SPicture> {
    let pool = pPicBuf?;
    // `slot_at_mut`'s range test, spelled here because that accessor's raw return is
    // the api boundary's (`codec_api.rs`'s release path) and not this one's.
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
/// **T5.N1: the pool and its slot array are one heap value, not two `WelsMallocz`
/// blocks.** S21 asks what happens to a struct gaining an owned field: this one is
/// `WelsMallocz`'d nowhere and comes out of `Box::new` fully built, so no zeroed
/// shell exists to be valid or invalid.
///
/// F19, per allocation: the `Box<PicPool>` here by the `Box::from_raw` in
/// [`DestroyPicBuff`]; the slot `Vec` by that same drop; each picture by the
/// [`FreePicture`] call that same function makes for its slot. **The pool adds no
/// owner** — every picture in it is still exactly one [`AllocPicture`] `Box`.
///
/// The partial-failure arm frees what it has already built, which is what the C++
/// means by `decoder.cpp:91`'s `pPicBuf->iCapacity = iPicIdx;` and its comment
/// "init capacity first for free memory". The port set no capacity before calling
/// `DestroyPicBuff` there, so its loop ran zero times and every picture allocated
/// before the failure leaked; with a `Vec` the count and the contents are the same
/// fact and the arm cannot disagree with itself.
///
/// **T5.P″1: it returns the pool instead of writing it through an out-parameter.**
/// The C's `ppPicBuf` existed to carry `pCtx->pPicBuff`'s address into this function;
/// with the field owning its pool, that address is a `&mut` into the context held
/// across the `AllocPicture` calls that read the context — S25's overlap, written for
/// no reason. `None` is the C's `1` return, and the C's `*ppPicBuf = NULL` is the
/// caller leaving the field as it found it.
///
/// T5.W3: the context is gone for [`alloc_picture`]'s reason, and this function's
/// two guards went with it — its callers hold the context and test there.
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
            // **T5.Q2: the partial-failure arm is the `Vec` going out of scope.** The
            // loop that stood here called `FreePicture` over what it had built, which
            // was the C's `iCapacity = iPicIdx` dance rewritten; an owning `Vec`
            // drops exactly those pictures on the early return, so the arm cannot
            // disagree with itself about how many there were.
            let Some(pic) = alloc_picture(bParseOnly, kiPicWidth, kiPicHeight) else {
                return None;
            };
            slots.push(Some(pic));
        }

        Some(PicPool::over(slots))
    }
}

/// Releases every picture the pool addresses, then the pool.
///
/// # The reordering reset (F37)
///
/// C++ `decoder.cpp:260` opens this function with
/// `ResetReorderingPictureBuffers (pCtx->pPictReoderingStatus, pCtx->pPictInfoList,
/// false)` and the port did not, calling it in exactly one place — decoder *creation*.
/// The two buffers are `CWelsDecoderImpl`'s members, wired into the context by
/// `decoder_init_c`, so they **outlive the context**: across an
/// Initialize/Uninitialize/Initialize cycle `sPictInfoList` kept POCs and
/// `iPicBuffIdx` values naming slots of a pool that had been freed and rebuilt, and
/// `EmitBufferedPicture` indexed the new pool with the old picture's index. Restored
/// here as parity, not as invention — the reset runs before the early returns, exactly
/// where the C++ has it, and the `pCtx` null-guard is the port's own (the C++
/// dereferences unconditionally).
///
/// **T5.P″1: it takes the pool by value.** `ppPicBuf` was the address of
/// `pCtx->pPicBuff` and its two jobs were to read the pool and to null the field;
/// `(*pCtx).pPicBuff.take()` at the call site does both, in one expression, and the
/// "null it afterwards" step cannot be forgotten because there is nothing left to
/// null. A null `pMa` used to abandon the pool; by T5.Q2 it abandoned nothing at
/// all, which is the same C behaviour with the pool's own leak removed.
///
/// **T5b.9: `pMa` is gone too.** T5.Q2 left the parameter in place with `let _ =
/// pMa;` under it, because the teardown had stopped going through the C's
/// allocator; what stayed was a raw pointer no line read and four
/// `&mut ma` casts at the call sites keeping it fed. The signature says what
/// the body does now.
pub fn DestroyPicBuff(pCtx: &mut SWelsDecoderContext, pool: Option<Box<PicPool>>) {
    // Both reordering buffers are `CWelsDecoderImpl`'s; the C++'s two null tests are
    // the two `Option`s, and the pair is disjoint so one `if let` chain serves.
    let SWelsDecoderContext { pPictReoderingStatus, pPictInfoList, .. } = &mut *pCtx;
    if let (Some(status), Some(list)) = (
        api_alias_mut(pPictReoderingStatus),
        api_alias_mut(pPictInfoList),
    ) {
        crate::decoder::decoder_core::ResetReorderingPictureBuffers(status, list, false);
    }

    // **T5.Q2: the release is the `Box` going out of scope.** The `FreePicture` loop
    // that stood here — and with it the `pMa.is_null()` early return that used to
    // abandon every picture in the pool — is drop glue: `pool` owns its slots, each
    // slot owns its picture, and each picture has owned its planes and its four
    // per-macroblock families since T5.P′3. R4's equivalence (the port frees what the
    // C++ frees) is discharged by construction, and the C's allocator is not reached
    // from this teardown at all — which is why the `pMa` parameter went with the
    // loop rather than outliving it.
    drop(pool);
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // **T5b.9: the file head's exception 2 is vestigial and the allow is deleted.**
    // It covered the resolver family's raw answers (W3 fact 1); those became
    // identities at T5b.1 and the module's `unsafe` went with them. With this gone,
    // `#[allow(unsafe_code)]` in `src/decoder/` reads **three by grep** — the two
    // api aliases and `picture.rs`'s Miri test — which is what the module heads say.
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
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        {
            // T5.Q3: `AllocPicture`'s raw pair is gone, so the fixture holds the
            // owner the pool holds and the `Box` is what releases it.
            let mut pic = alloc_picture(false, 160, 120)
                .expect("the picture allocates");
            // T5b.6: a borrow — the fixture never needed the alias, and `(*p_pic)`
            // reads the same either way.
            let p_pic: &mut SPicture = &mut *pic;
            assert_eq!((*p_pic).iWidthInPixel, 160);
            assert_eq!((*p_pic).iHeightInPixel, 120);
            assert!(!(*p_pic).data_ptr(0).is_null());

            // T5.C3: the geometry the C computed is now the plane's, and pinning it
            // here is what makes "same arithmetic" a check rather than a claim.
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
            // The 128 fill covers the whole allocation, corners included — the EC
            // prefetch and `narrow_16x16_idr_lost` both depend on it.
            assert!((*p_pic).plane(0).as_slice().iter().all(|&b| b == 128));
            assert_eq!((*p_pic).plane(0).at(-32, -32), 128);

            drop(pic);
        }
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    /// The `bParseOnly` arm: strides from the geometry, no sample memory, and a null
    /// `data_ptr` — the three properties the C's null `pData[i]` beside a non-zero
    /// `iLinesize[i]` encoded, which every caller still tests with `.is_null()`.
    #[test]
    fn test_alloc_picture_parse_only_carries_strides_and_no_bytes() {
        let mut ma = CMemoryAlign::new(32);
        // T5.W3: the fixture context stays, because `parse_only(&pCtx.pParam)` is what a
        // production caller passes and this asserts the two agree — the mechanical
        // pass through this file set the argument to `false` here and the test caught
        // it, which is the whole reason the flag is read back rather than written in.
        let mut param = SDecodingParam { bParseOnly: true, ..Default::default() };
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        {
            let pCtx = &mut *ctx;
            assert!(
                crate::decoder::decoder_context::parse_only(&pCtx.pParam),
                "the accessor reads the field the callee used to reach for itself"
            );
            let mut pic = alloc_picture(parse_only(&pCtx.pParam), 160, 120)
                .expect("the picture allocates");
            // T5b.6: a borrow — the fixture never needed the alias, and `(*p_pic)`
            // reads the same either way.
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
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    #[test]
    fn test_prefetch_pic_circular_scan() {
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

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
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    /// **F42 — the current slot resolves through the mutable half, and must.**
    ///
    /// A malformed stream can legally put the picture being decoded into a reference
    /// list: `pRefList[i]` is filled from a `ref_idx` the bitstream chooses, and the
    /// C++ resolves it to `pCtx->pDec` and reads on. `PoolRest::get` *panics* on the
    /// slot its split lends out, so answering a reference resolution out of the rest
    /// would turn a decodable-with-garbage stream into an abort — a behaviour change
    /// on exactly the input class the goldens, the sweeps and the conformance corpus
    /// never reach (S6's never-widen default).
    ///
    /// **Red under revert**, per F21: make [`PicRefs::classify`] answer the
    /// `id == cur` case through `rest` and this test panics with "held mutably" —
    /// which is what the decoder would do on that stream.
    ///
    /// **T5b.2 rewrote the test's second half, and the rewrite is the face.** It used
    /// to assert that the reference answer and the current picture are *one address
    /// with one tag*, because that was the only way a read through the reference could
    /// coexist with a write through `pDec`. There is no address now: `classify`
    /// answers by identity and [`PicRefs::resolve`] hands back the caller's own
    /// borrow, so the coexistence is two shared borrows — the thing the compiler
    /// checks rather than the thing Miri had to be asked about.
    #[test]
    fn f42_a_reference_list_entry_naming_the_current_picture_resolves_not_panics() {
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma;
        ctx.pParam = &mut param;

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
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    /// F37: destroying the pool resets the reordering buffers, because they outlive it.
    ///
    /// The cycle this pins is the public one — Initialize, decode, Uninitialize,
    /// Initialize — where the context is rebuilt but `CWelsDecoderImpl`'s
    /// `sPictInfoList` and `sReoderingStatus` are not. Without the reset, the second
    /// life starts with `iPicBuffIdx` values naming slots of the first life's pool.
    #[test]
    fn destroying_the_pool_resets_the_reordering_buffers() {
        use crate::decoder::decoder_context::{IMinInt32, SPictInfo, SPictReoderingStatus};

        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        // A mutable reference coerces to a raw pointer at an assignment or an
        // argument, so this fixture spells no pointer type at all (S16: the metric
        // counts types written, and a test that writes casts it does not need
        // inflates it — including in a comment).
        ctx.pMemAlign = &mut ma;
        ctx.pParam = &mut param;

        // The decoder object's own members, and a decode's leavings in them: two
        // buffered pictures naming pool slots 2 and 3.
        let mut pict_info: [SPictInfo; 16] = [SPictInfo::default(); 16];
        let mut status = SPictReoderingStatus::default();
        status.iLargestBufferedPicIndex = 1;
        status.iNumOfPicts = 2;
        status.bHasBSlice = true;
        pict_info[0].iPOC = 4;
        pict_info[0].iPicBuffIdx = 2;
        pict_info[1].iPOC = 8;
        pict_info[1].iPicBuffIdx = 3;

        // Wired **after** the fixture is dirtied, and that ordering is the test's own
        // brush with F38. Written the other way round, the stores retag `status` and
        // `pict_info`, the writes above go through the *locals* and pop those retags,
        // and `DestroyPicBuff`'s reset reads a dead tag. Miri convicted exactly that
        // on the closing battery — in the test written to prove F37, by the session
        // that had just found and fixed F38 in production. `addr_of_mut!` is **not**
        // the fix at this site: it is what saves the production stores, where the
        // invalidating write goes through the raw `dec_impl` rather than through a
        // local, and a raw sibling does not pop a raw derivation. Here the write is
        // through the local itself, so nothing but ordering helps. S13's law reaches
        // the code you write while applying it.
        ctx.pPictInfoList = &mut pict_info;
        ctx.pPictReoderingStatus = &mut status;

        {
            let pool = CreatePicBuff(false, 4, 64, 64);
            assert!(pool.is_some());
            DestroyPicBuff(&mut *ctx, pool);
        }
        assert_eq!(ma.WelsGetMemoryUsage(), 0);

        // `fullReset = false`, so the loop covers `iLargestBufferedPicIndex + 1` entries
        // — the two that were written — and leaves the untouched tail alone.
        assert_eq!(status.iNumOfPicts, 0);
        assert_eq!(status.iLargestBufferedPicIndex, 0);
        assert!(!status.bHasBSlice);
        assert_eq!(status.iMinPOC, IMinInt32);
        for i in 0..2 {
            assert_eq!(pict_info[i].iPicBuffIdx, -1, "slot {i} still names the freed pool");
            assert_eq!(pict_info[i].iPOC, IMinInt32, "slot {i}");
        }
    }

    /// T5.N2's half of the P3 identity property, and the half the five P3 tests
    /// cannot reach: they build fixtures, which have no slot, so they exercise
    /// `same_picture`'s address arm. This exercises the slot arm — two **pooled**
    /// pictures carrying one POC are still two references.
    #[test]
    fn pooled_pictures_are_identified_by_slot_not_by_poc() {
        use crate::decoder::picture::same_picture;

        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        {
            let mut pool = CreatePicBuff(false, 2, 64, 64)
                .expect("pool");
            let (id_a, id_b) = (pool.id(0), pool.id(1));

            assert_eq!(pool.slot(id_a).unwrap().pic_id(), Some(id_a), "a picture knows its slot");
            assert_eq!(pool.slot(id_b).unwrap().pic_id(), Some(id_b));

            // Owned slots: each write derives its own borrow and ends it, rather than
            // holding two `&mut`s into one pool across both statements.
            pool.slot_mut(id_a).unwrap().iFramePoc = 4;
            pool.slot_mut(id_b).unwrap().iFramePoc = 4; // duplicate POC, distinct slots
            let (a, b) = (pool.slot(id_a), pool.slot(id_b));
            assert!(!same_picture(a, b), "two slots are two references");
            assert!(same_picture(a, a));

            // A picture outside the pool has no slot and is its own identity only.
            // Both writes happen before either address is taken, and the addresses
            // come from `addr_of!` rather than from `&loose` — S29's spelling for
            // S29's reason. The first draft of this test took `&loose`, then wrote
            // `iFramePoc`, then read through the raw pointer: the write invalidated
            // the shared tag the reads were using. Miri convicted it and nothing
            // else in the battery could have.
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

            // T5.W1: the two null pointers are two absent pictures now, and the
            // `as_ref()` above is the null test that used to live inside the callee.
            assert!(same_picture(None, None));
            assert!(!same_picture(None, a));

            DestroyPicBuff(&mut *ctx, Some(pool));
        }
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
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
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

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
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    #[test]
    fn test_prefetch_pic_for_thread() {
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        {
            let mut pool = CreatePicBuff(false, 3, 64, 64)
                .expect("pool");
            // T5.W3: the two thread prefetches take `Option<&mut PicPool>` now, so
            // the fixture hands over a borrow per call instead of deriving one raw
            // pointer and holding it across all four. They still have no production
            // caller (F36's list) and W7's straggler sweep still decides them; what
            // changed is only that the null test is the `Option`.
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
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }
}
