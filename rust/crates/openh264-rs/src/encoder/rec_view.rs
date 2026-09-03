#![deny(unsafe_code)]
// Copyright (c) 2009-2013, Cisco Systems
// All rights reserved.
//
// This file has no C++ counterpart.

//! The **reconstruction seam**.
//!
//! Every multi-threaded worker of the encoder writes into *one* reconstruction
//! picture: the three pixel planes, and the four per-macroblock side arrays
//! (`sMvList`, `pRefMbQp`, `pMbSkipSad`, `uiRefMbType`) that the mode-decision
//! half stamps as it goes. The byte sets are disjoint per worker — a worker
//! reads back only what it wrote — but **no `&mut` can say so**: a
//! macroblock's pixel span is the full width of its rows (three of
//! the four MT slice modes put a slice boundary mid-row), and a `&mut Vec<T>`
//! side-array borrow is a `Unique` retag over the whole array whichever single
//! index it goes on to write.
//!
//! So the picture is reached, for the fork's whole scope, through **one shared
//! interior-mutable view** built before the fork from the picture's exclusive
//! borrow. Consumers hold `&RecPicView` and write through `&self`.
//!
//! # The soundness argument, stated once
//!
//! [`RecPicView::build`] takes `&mut SPicture`, so for the view's lifetime
//! nothing else may borrow the picture. From it the view captures, per
//! storage, exactly two numbers: the allocation's base address and its length.
//! Every later access re-derives a `&[Cell<T>]` from that captured pair
//! (`SharedCells::cells`, the one place raw parts cross into cell land), so no
//! access is a child of any other and none of them can pop a sibling.
//!
//! Writes then go through [`Cell`] with **no synchronisation at all**, which is
//! sound **iff** no two workers touch the same byte.
//!
//! Publication to post-join readers is the scope join, which is a
//! happens-before edge: everything a worker wrote before `thread::scope`
//! returns is visible to whatever reads the picture after it.
//!
//! # What is *not* claimed
//!
//! The view does not make overlapping writes safe, and it does not check
//! disjointness at run time — bounds are checked, ownership is not. It is
//! exactly as sound as the slice partition.

use std::cell::Cell;

use crate::encoder::encoder_context::SMVUnitXY;
use crate::encoder::picture::SPicture;

/// The one place a captured base/length pair becomes a cell slice.
///
/// Held by value in every view below, so the whole seam has exactly one
/// `UnsafeCell`-crossing accessor (`SharedCells::cells`) and exactly one place
/// raw parts are captured (`SharedCells::from_parts`, which
/// `SharedCells::capture` and the plane build both route through).
#[derive(Debug)]
pub struct SharedCells<T: Copy> {
    /// The allocation's base, read out of the `Vec` header at capture time
    /// (never through a slice — a `&mut [T]` would be a `Unique` retag over the
    /// whole buffer, which is the bug this type exists to avoid).
    base: *mut T,
    len: usize,
}

impl<T: Copy> SharedCells<T> {
    /// **The one place raw parts are captured.** Everything else in the module
    /// routes through here.
    ///
    /// `base` must be the allocation's own root address, read out of a header
    /// (`Vec::as_mut_ptr`, `PaddedPlane::root_ptr`) and **never** through a
    /// slice: `&mut [T]` is a `Unique` retag over the whole buffer, so a base
    /// taken that way is a child that the next such borrow pops. That is the
    /// same trap `PaddedPlane::root_ptr` documents, one level up.
    #[inline]
    fn from_parts(base: *mut T, len: usize) -> Self {
        Self { base, len }
    }

    /// Captures a `Vec`'s elements for the view's lifetime.
    #[inline]
    fn capture(v: &mut Vec<T>) -> Self {
        Self::from_parts(v.as_mut_ptr(), v.len())
    }

    /// An empty capture — the port's spelling of the null the C++ leaves where
    /// a picture was allocated without `bNeedMbInfo` (`picture_handle.cpp:104`).
    #[inline]
    fn empty() -> Self {
        Self::from_parts(std::ptr::NonNull::dangling().as_ptr(), 0)
    }

    /// The captured storage, as cells.
    ///
    /// **Why the `unsafe` inside is sound.** The base/length pair was taken at
    /// [`from_parts`](Self::from_parts) from
    /// storage borrowed exclusively at that moment — a `&mut Vec<T>` through
    /// [`capture`](Self::capture), or a `PaddedPlane`'s `root_ptr`/`buf_len` pair
    /// — and the module contract keeps that exclusive borrow the last one for the
    /// view's whole lifetime, so the range is allocated and no `&mut` to it
    /// exists. A `Cell` retag is `SharedReadWrite`, which performs no memory
    /// access, so this call cannot itself race.
    #[allow(unsafe_code)]
    #[inline]
    fn cells(&self) -> &[Cell<T>] {
        unsafe { std::slice::from_raw_parts(self.base.cast::<Cell<T>>(), self.len) }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// **The seam's one promise.**
///
/// A captured base address is `!Sync` by inference, and every view in this
/// module is built on one — so this single `impl` is what lets a worker hold
/// `&RecPicView` across `thread::scope`. What makes the sharing sound is not
/// the `impl`: each worker touches only its own macroblocks' bytes and entries.
/// The `impl` is where that claim is written down, and it is placed on the type
/// that holds the raw parts rather than on the picture view, because the parts
/// are what is being promised about.
///
/// Note what does **not** become `Sync`: [`RecCursor`] holds `&[Cell<u8>]` and
/// stays thread-local by inference, so a worker must make its own from the
/// shared plane rather than being handed one.
#[allow(unsafe_code)]
unsafe impl<T: Copy> Sync for SharedCells<T> {}

/// One pixel plane of the reconstruction picture, shared and writable.
///
/// The safe mirror of `PaddedPlane`'s geometry: logical `(0, 0)` sits at
/// `origin`, negative coordinates read the padding, and every access is
/// bounds-checked against the whole allocation.
#[derive(Debug)]
pub struct SharedPlane {
    cells: SharedCells<u8>,
    stride: usize,
    origin: usize,
}

#[inline]
fn idx(center: usize, dx: isize, dy: isize, stride: usize) -> usize {
    (center as isize + dy * stride as isize + dx) as usize
}

impl SharedPlane {
    /// Bytes per row — the C++ `iLineSize[i]`.
    #[inline]
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// True where the picture was built without this plane at all.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Sample at logical `(x, y)`.
    #[inline]
    pub fn at(&self, x: isize, y: isize) -> u8 {
        self.cells.cells()[idx(self.origin, x, y, self.stride)].get()
    }

    /// Writes the sample at logical `(x, y)`.
    #[inline]
    pub fn set(&self, x: isize, y: isize, v: u8) {
        self.cells.cells()[idx(self.origin, x, y, self.stride)].set(v);
    }

    /// A cursor anchored at logical `(x, y)` — the shared analogue of
    /// `PaddedPlane::cursor_mut`, and the type the reconstruction kernels take.
    #[inline]
    pub fn cursor(&self, x: isize, y: isize) -> RecCursor<'_> {
        RecCursor {
            cells: self.cells.cells(),
            center: idx(self.origin, x, y, self.stride),
            stride: self.stride,
        }
    }
}

/// A roving anchor into a [`SharedPlane`], addressed in offsets from its centre.
///
/// Shaped after `PlaneCursorMut` so a converted kernel reads the same, with one
/// difference that is the point of the type: **`set`/`write_row` take `&self`**.
/// The cursor value itself is owned by whoever made it, so a kernel may still
/// take `&mut RecCursor` and advance it; what it may never do is hand out a
/// `&mut [u8]` into the plane.
#[derive(Debug, Clone, Copy)]
pub struct RecCursor<'a> {
    cells: &'a [Cell<u8>],
    center: usize,
    stride: usize,
}

impl<'a> RecCursor<'a> {
    /// Sample at `(dx, dy)` from the anchor.
    #[inline]
    pub fn at(&self, dx: isize, dy: isize) -> u8 {
        self.cells[idx(self.center, dx, dy, self.stride)].get()
    }

    /// Writes the sample at `(dx, dy)` from the anchor.
    #[inline]
    pub fn set(&self, dx: isize, dy: isize, v: u8) {
        self.cells[idx(self.center, dx, dy, self.stride)].set(v);
    }

    /// `N` samples of row `dy` starting at `dx0`, by value.
    ///
    /// By value rather than by reference because a shared view cannot lend
    /// `&[u8]` into cells; every reconstruction kernel's row is 4, 8 or 16
    /// samples, so the copy is a register-file move.
    #[inline]
    pub fn row<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N] {
        let start = idx(self.center, dx0, dy, self.stride);
        let row = &self.cells[start..][..N];
        std::array::from_fn(|i| row[i].get())
    }

    /// `h` consecutive rows of `N` samples each, starting at `(dx0, dy0)` — the
    /// cell mirror of [`PlaneCursor::row_windows`](crate::safe::plane::PlaneCursor::row_windows).
    ///
    /// **Why this can lend where [`row`](Self::row) cannot.** `row` returns by
    /// value because a shared view cannot hand out `&[u8]` into its cells. It can
    /// hand out `&[Cell<u8>]`, though — that is the point of a cell — so the block
    /// walk keeps the shape `PlaneCursor::row_windows` was built for: **one bounds
    /// check per block per side**, not two per row.
    ///
    /// # Panics
    /// If the block leaves the buffer, at the first slicing.
    #[inline]
    pub fn row_windows<const N: usize>(
        &self,
        dy0: isize,
        dx0: isize,
        h: usize,
    ) -> impl Iterator<Item = &[Cell<u8>; N]> {
        let start = idx(self.center, dx0, dy0, self.stride);
        let span = if h == 0 { 0 } else { (h - 1) * self.stride + N };
        self.cells[start..][..span]
            .chunks(self.stride)
            .map(|r| r[..N].try_into().unwrap())
    }

    /// Writes `N` samples into row `dy` starting at `dx0`.
    #[inline]
    pub fn write_row<const N: usize>(&self, dy: isize, dx0: isize, src: &[u8; N]) {
        let start = idx(self.center, dx0, dy, self.stride);
        let row = &self.cells[start..][..N];
        for (c, &v) in row.iter().zip(src.iter()) {
            c.set(v);
        }
    }

    /// A cursor over **caller-owned** bytes — safe, with no raw pointer anywhere.
    ///
    /// `Cell::from_mut(..).as_slice_of_cells()` is the standard library's own door
    /// from an exclusive borrow to shared-mutable cells. It is what lets a per-worker
    /// scratch array on `SMbCache` feed the very kernel a shared picture plane feeds,
    /// so a dispatch slot needs **one** operand type rather than two — which a
    /// function-pointer table cannot express any other way, being unable to be
    /// generic.
    #[inline]
    pub fn over_owned(buf: &'a mut [u8], center: usize, stride: usize) -> Self {
        Self { cells: Cell::from_mut(buf).as_slice_of_cells(), center, stride }
    }

    /// The same anchor moved by `(dx, dy)`.
    #[inline]
    #[must_use]
    pub fn advance(self, dx: isize, dy: isize) -> Self {
        Self { center: idx(self.center, dx, dy, self.stride), ..self }
    }

    /// Bytes per row.
    #[inline]
    pub fn stride(&self) -> usize {
        self.stride
    }
}

impl crate::safe::plane::PlaneSamples for RecCursor<'_> {
    /// `&mut self` to fit the trait, though the write itself needs only `&self`
    /// — the cursor value is the caller's, and what the seam withholds is a
    /// `&mut [u8]` into the plane, which this cannot produce.
    #[inline]
    fn set(&mut self, dx: isize, dy: isize, v: u8) {
        RecCursor::set(self, dx, dy, v)
    }
}

impl crate::safe::plane::RefSamples for RecCursor<'_> {
    #[inline]
    fn at(&self, dx: isize, dy: isize) -> u8 {
        RecCursor::at(self, dx, dy)
    }

    #[inline]
    fn row_n<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N] {
        RecCursor::row::<N>(self, dy, dx0)
    }

    #[inline]
    fn row_blocks<const N: usize>(
        &self,
        dy0: isize,
        dx0: isize,
        h: usize,
    ) -> impl Iterator<Item = crate::safe::plane::RowBuf> {
        RecCursor::row_windows::<N>(self, dy0, dx0, h).map(|r| {
            let mut out = crate::safe::plane::RowBuf::new(N);
            for (o, c) in out.as_mut().iter_mut().zip(r.iter()) {
                *o = c.get();
            }
            out
        })
    }

    /// The one implementor whose row is **owned** — cells cannot lend `&[u8]`.
    type Row<'a>
        = crate::safe::plane::RowBuf
    where
        Self: 'a;

    #[inline]
    fn advance(self, dx: isize, dy: isize) -> Self {
        RecCursor::advance(self, dx, dy)
    }

    #[inline]
    fn row_view(&self, dy: isize, dx0: isize, len: usize) -> crate::safe::plane::RowBuf {
        let mut out = crate::safe::plane::RowBuf::new(len);
        let start = idx(self.center, dx0, dy, self.stride);
        let row = &self.cells[start..][..len];
        for (o, c) in out.as_mut().iter_mut().zip(row.iter()) {
            *o = c.get();
        }
        out
    }
}

/// The reconstruction plane's flavour of `common::copy_mb`'s `copy_WxH` family:
/// a `W`x`h` block copied out of a contiguous prediction buffer into the shared
/// view.
///
/// `PlaneCursorMut::row_mut` hands out `&mut [u8]`, which a shared view cannot
/// do and must not do — a macroblock's contiguous plane span is the full width
/// of its rows, so `&mut [u8]` over it would claim the neighbouring slice's
/// columns as well. Rows go in by value instead.
///
/// Every one of the reconstruction copy sites has an *arena* source — the
/// macroblock cache's `sSkipMb` or `sMemPredMb`, both plain owned arrays — so the
/// source is a slice and a stride rather than a second cursor.
///
/// # Panics
/// If `src` is shorter than `(h - 1) * src_stride + W`, or the block runs off the
/// plane. Both are geometry bugs in the caller.
#[inline]
pub fn copy_block_to_view<const W: usize>(
    src: &[u8],
    src_stride: usize,
    dst: &RecCursor<'_>,
    h: usize,
) {
    for y in 0..h {
        let row: &[u8; W] = src[y * src_stride..][..W].try_into().unwrap();
        dst.write_row::<W>(y as isize, 0, row);
    }
}

/// One per-macroblock side array of the reconstruction picture, shared and
/// writable: `sMvList`, `pRefMbQp`, `pMbSkipSad`, `uiRefMbType`.
///
/// Each worker writes only its own macroblocks' entries, so the index sets are
/// disjoint for the same reason the pixel sets are — but a `&mut Vec<T>` says
/// nothing of the kind, which is why these are here beside the planes rather
/// than left on the `&mut SPicture` route.
#[derive(Debug)]
pub struct SharedMbArray<T: Copy> {
    cells: SharedCells<T>,
}

impl<T: Copy> SharedMbArray<T> {
    /// Entry `i`.
    #[inline]
    pub fn get(&self, i: usize) -> T {
        self.cells.cells()[i].get()
    }

    /// Writes entry `i`.
    #[inline]
    pub fn set(&self, i: usize, v: T) {
        self.cells.cells()[i].set(v);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// True where the picture carries no such array (`bNeedMbInfo` false).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// The reconstruction picture, for the frame loop: three planes and four
/// per-macroblock arrays, all shared and all writable through `&self`.
///
/// Built once per frame on the calling thread, before anything forks; read by
/// every worker through the layer. See the module docs for the soundness
/// argument this type is the subject of.
#[derive(Debug)]
pub struct RecPicView {
    planes: [SharedPlane; 3],
    sMvList: SharedMbArray<SMVUnitXY>,
    pRefMbQp: SharedMbArray<u8>,
    pMbSkipSad: SharedMbArray<i32>,
    uiRefMbType: SharedMbArray<u32>,
}

impl RecPicView {
    /// Captures the picture for the frame.
    ///
    /// `&mut SPicture` is load-bearing: it is the exclusive borrow the whole
    /// module contract rests on, and it is taken here once, before the fork.
    pub fn build(pic: &mut SPicture) -> Self {
        let [y, u, v] = pic.planes_mut3();
        let planes = [y, u, v].map(|p| {
            if p.is_empty() {
                SharedPlane { cells: SharedCells::empty(), stride: p.stride(), origin: 0 }
            } else {
                let (origin, stride, len) = (p.origin(), p.stride(), p.buf_len());
                SharedPlane { cells: SharedCells::from_parts(p.root_ptr(), len), stride, origin }
            }
        });
        Self {
            planes,
            sMvList: SharedMbArray { cells: SharedCells::capture(&mut pic.sMvList) },
            pRefMbQp: SharedMbArray { cells: SharedCells::capture(&mut pic.pRefMbQp) },
            pMbSkipSad: SharedMbArray { cells: SharedCells::capture(&mut pic.pMbSkipSad) },
            uiRefMbType: SharedMbArray { cells: SharedCells::capture(&mut pic.uiRefMbType) },
        }
    }

    /// Plane `i` — 0 luma, 1 Cb, 2 Cr.
    #[inline]
    pub fn plane(&self, i: usize) -> &SharedPlane {
        &self.planes[i]
    }

    /// `SPicture::sMvList` — one motion vector per macroblock.
    #[inline]
    pub fn mv_list(&self) -> &SharedMbArray<SMVUnitXY> {
        &self.sMvList
    }

    /// `SPicture::pRefMbQp` — one QP per macroblock.
    #[inline]
    pub fn ref_mb_qp(&self) -> &SharedMbArray<u8> {
        &self.pRefMbQp
    }

    /// `SPicture::pMbSkipSad` — one skip SAD per macroblock.
    #[inline]
    pub fn mb_skip_sad(&self) -> &SharedMbArray<i32> {
        &self.pMbSkipSad
    }

    /// `SPicture::uiRefMbType` — one macroblock type per macroblock.
    #[inline]
    pub fn ref_mb_type(&self) -> &SharedMbArray<u32> {
        &self.uiRefMbType
    }
}

impl crate::safe::plane::SampleCursor for RecCursor<'_> {
    #[inline]
    fn at(&self, dx: isize, dy: isize) -> u8 {
        RecCursor::at(self, dx, dy)
    }
    #[inline]
    fn row_n<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N] {
        RecCursor::row::<N>(self, dy, dx0)
    }
    #[inline]
    fn advance(self, dx: isize, dy: isize) -> Self {
        RecCursor::advance(self, dx, dy)
    }
}

/// `W` bytes of each of `height` rows, from one shared cursor to another.
///
/// The shared-seam twin of `mc::copy_rows`, and the kernel behind every `pfCopyNxM`
/// slot. Both operands are [`RecCursor`] because the slot's two callers
/// disagree about storage — the background path copies picture-to-picture, the
/// mode-decision path copies an owned prediction scratch into a picture plane — and
/// `RecCursor::over_owned` brings the scratch to the same type without a raw.
#[inline(always)]
pub fn copy_rows_shared<const W: usize>(dst: &RecCursor<'_>, src: &RecCursor<'_>, height: usize) {
    for dy in 0..height as isize {
        let row = src.row::<W>(dy, 0);
        dst.write_row::<W>(dy, 0, &row);
    }
}

/// A three-plane view of a picture the encode **reads** — the source surface, and
/// the analysis surface for the preprocess.
///
/// # Why this is `SharedPlane` and not a plain slice
///
/// `VaaBackgroundMbDataUpdate` copies previous-source into current-source through
/// raw roots, in-fork, per macroblock, and the destination is the very picture
/// `pEncData` reads. `bEnableBackgroundDetection` is `true` by default
/// (`param_svc.rs:293`), so that is the ordinary configuration, not a corner.
///
/// A whole-plane `&[u8]` claims **every byte** of the plane, so it races a concurrent
/// write to any of them: a shared `&T` claims the whole struct — it races any
/// concurrent write to any byte inside.
///
/// So the source planes are reached exactly as the reconstruction planes are: through
/// [`SharedPlane`], whose cells make a concurrent write lawful by construction, and
/// whose [`RecCursor`] never lends a slice.
///
/// Built through `&SPicture` — unlike [`RecPicView`] it makes no exclusive claim, so
/// its constructor is public and unrestricted: several readers of one picture need no
/// rule, and the writer this guards against reaches the plane through its own raw
/// roots rather than through a second view.
#[derive(Debug)]
pub struct RoPicView {
    planes: [SharedPlane; 3],
}

impl RoPicView {
    /// Captures a picture's three planes for reading.
    pub fn build(pic: &crate::encoder::picture::SPicture) -> Self {
        let planes = [0usize, 1, 2].map(|i| {
            let p = pic.plane(i);
            if p.is_empty() {
                SharedPlane { cells: SharedCells::empty(), stride: p.stride(), origin: 0 }
            } else {
                // `root_ptr_shared`, not `root_ptr`: `&mut self` would be a `Unique`
                // retag over the plane header and every worker resolves the same
                // picture.
                SharedPlane {
                    cells: SharedCells::from_parts(p.root_ptr_shared(), p.buf_len()),
                    stride: p.stride(),
                    origin: p.origin(),
                }
            }
        });
        Self { planes }
    }

    /// Plane `i` — 0 luma, 1 Cb, 2 Cr.
    #[inline]
    pub fn plane(&self, i: usize) -> &SharedPlane {
        &self.planes[i]
    }
}

/// A standalone plane view over one `PaddedPlane`, with the same shape
/// [`RecPicView::build`] gives each of its three.
///
/// **Test-only, and deliberately so**: it is the differential tests' way to put a
/// seam cursor and a `PlaneCursorMut` over the same storage, which is how every
/// `RecCursor` kernel in `decode_mb_aux` is checked against its `PlaneCursorMut`
/// twin. Production code reaches planes through `RecPicView::build`'s
/// `&mut SPicture` and nothing else — that exclusive borrow is the module
/// contract, and a public constructor from a single plane would let a caller take
/// two views of one picture without the compiler objecting.
#[cfg(test)]
pub(crate) fn shared_plane_for_test(p: &mut crate::safe::plane::PaddedPlane) -> SharedPlane {
    let (origin, stride, len) = (p.origin(), p.stride(), p.buf_len());
    SharedPlane { cells: SharedCells::from_parts(p.root_ptr(), len), stride, origin }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::plane::PaddedPlane;

    use super::shared_plane_for_test as view_of;

    #[test]
    fn a_cursor_reads_back_what_it_wrote_through_a_shared_view() {
        let mut plane = PaddedPlane::new(32, 16, 8, 48);
        let view = view_of(&mut plane);
        let c = view.cursor(4, 3);
        c.write_row::<4>(0, 0, &[1, 2, 3, 4]);
        c.set(0, 1, 9);
        assert_eq!(c.row::<4>(0, 0), [1, 2, 3, 4]);
        assert_eq!(view.at(4, 3), 1);
        assert_eq!(view.at(7, 3), 4);
        assert_eq!(view.at(4, 4), 9);
        // Negative offsets reach the padding, which is what intra prediction
        // needs of the top and left borders.
        view.set(-1, -1, 77);
        assert_eq!(c.at(-5, -4), 77);
    }

    /// **The new row accessors must agree with the old ones, sample for sample.**
    ///
    /// `RefSamples` has two run-time row readers so the SAD family and
    /// `common/mc.rs` can serve a shared cell view and a plain plane from one
    /// body: `row_blocks` (the folded const-size block walk) and `row_view` (the
    /// run-time-length row, borrowed for a plane and owned for a cell view). Each
    /// exists only to move *where* the read comes from, never *what* is read — so
    /// this asserts the three routes over one buffer give identical bytes:
    /// `PlaneCursor`'s inherent `row`, the trait's `row_blocks`, and the trait's
    /// `row_view`, on both cursor types.
    #[test]
    fn the_row_accessors_agree_across_both_cursor_types() {
        use crate::safe::plane::{PlaneCursor, RefSamples};

        let mut rng_state = 0x51ED_270Fu32;
        let mut next = move || {
            rng_state = rng_state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (rng_state >> 24) as u8
        };
        for &stride in &[16usize, 48, 96] {
            let mut plane = PaddedPlane::new(stride - 16, 16, 8, stride);
            for y in -8..24isize {
                for x in -8..(stride as isize - 8) {
                    let v = next();
                    plane.set(x, y, v);
                }
            }
            // One shared view and one plain cursor over the *same* storage.
            let view = view_of(&mut plane);
            let cells = view.cursor(3, 2);
            let plain = plane.cursor(3, 2);

            for &(dy, dx) in &[(0isize, 0isize), (1, -1), (-2, 2), (3, 0)] {
                // `row_view`: the run-time-length read, both types.
                for len in [1usize, 4, 8] {
                    let want = plain.row(dy, dx, len).to_vec();
                    assert_eq!(&*RefSamples::row_view(&plain, dy, dx, len), &want[..],
                        "plane row_view, stride {stride}, ({dx},{dy}), len {len}");
                    assert_eq!(&*RefSamples::row_view(&cells, dy, dx, len), &want[..],
                        "cell row_view, stride {stride}, ({dx},{dy}), len {len}");
                }
                // `row_blocks`: the folded block walk, both types, against a
                // straight `row` walk of the same block.
                let want: Vec<Vec<u8>> =
                    (0..3).map(|k| plain.row(dy + k, dx, 4).to_vec()).collect();
                let got_plain: Vec<Vec<u8>> =
                    plain.row_blocks::<4>(dy, dx, 3).map(|r| r.to_vec()).collect();
                let got_cells: Vec<Vec<u8>> =
                    cells.row_blocks::<4>(dy, dx, 3).map(|r| r.to_vec()).collect();
                assert_eq!(got_plain, want, "plane row_blocks, stride {stride}, ({dx},{dy})");
                assert_eq!(got_cells, want, "cell row_blocks, stride {stride}, ({dx},{dy})");
            }
        }
    }

    /// **The probe the seam's one `Sync` impl exists for, and the mid-row case in
    /// miniature.** Two scoped threads write *the same rows* at different
    /// columns — the shape no `&mut [u8]` can express — through one shared view.
    /// Under Miri's data-race detector (on by default) this passes only because
    /// the byte sets are disjoint.
    #[test]
    fn two_threads_write_disjoint_stripes_of_one_row() {
        let mut plane = PaddedPlane::new(32, 4, 8, 48);
        let view = view_of(&mut plane);

        std::thread::scope(|s| {
            s.spawn(|| {
                for y in 0..4isize {
                    for x in 0..16isize {
                        view.set(x, y, 0xA0 | (x as u8 & 0x0F));
                    }
                }
            });
            s.spawn(|| {
                for y in 0..4isize {
                    for x in 16..32isize {
                        view.set(x, y, 0x50 | (x as u8 & 0x0F));
                    }
                }
            });
        });

        for y in 0..4isize {
            for x in 0..32isize {
                let want = if x < 16 { 0xA0 } else { 0x50 } | (x as u8 & 0x0F);
                assert_eq!(view.at(x, y), want, "at ({x}, {y})");
            }
        }
    }

    /// The side arrays' half of the same promise: two workers stamping
    /// per-macroblock entries at disjoint indices of one `Vec`. This is the
    /// case a `&mut Vec<T>` cannot express *at all* — the retag covers the
    /// whole array however narrow the write.
    #[test]
    fn two_threads_stamp_disjoint_entries_of_one_side_array() {
        let mut mvs: Vec<SMVUnitXY> = vec![SMVUnitXY::default(); 64];
        let mut qps: Vec<u8> = vec![0; 64];
        let arr = SharedMbArray { cells: SharedCells::capture(&mut mvs) };
        let qp = SharedMbArray { cells: SharedCells::capture(&mut qps) };
        // Interleaved, not split: consecutive indices land on different
        // threads, so neighbouring entries share a cache line and any
        // whole-array retag would be caught.
        std::thread::scope(|s| {
            for t in 0..2usize {
                let (arr, qp) = (&arr, &qp);
                s.spawn(move || {
                    let mut i = t;
                    while i < 64 {
                        arr.set(i, SMVUnitXY { iMvX: i as i16, iMvY: -(i as i16) });
                        qp.set(i, i as u8);
                        i += 2;
                    }
                });
            }
        });

        for i in 0..64usize {
            assert_eq!(arr.get(i), SMVUnitXY { iMvX: i as i16, iMvY: -(i as i16) });
            assert_eq!(qp.get(i), i as u8);
        }
    }

    /// `SharedCells::empty` is the null the C++ leaves where a picture was
    /// built without `bNeedMbInfo`, and the guard every consumer of those four
    /// arrays already spells as `is_empty()`.
    #[test]
    fn an_absent_side_array_reports_empty_rather_than_dangling() {
        let mut none: Vec<i32> = Vec::new();
        let a = SharedMbArray { cells: SharedCells::capture(&mut none) };
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);

        let b: SharedMbArray<u32> = SharedMbArray { cells: SharedCells::empty() };
        assert!(b.is_empty());
    }
}

