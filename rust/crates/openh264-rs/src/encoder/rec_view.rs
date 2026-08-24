// Copyright (c) 2009-2013, Cisco Systems
// All rights reserved.
//
// This file has no C++ counterpart: it is the port's answer to a shape the C++
// expresses with a bare pointer.

//! The **reconstruction seam** — decision D-mt-3, option A.
//!
//! Every multi-threaded worker of the encoder writes into *one* reconstruction
//! picture: the three pixel planes, and the four per-macroblock side arrays
//! (`sMvList`, `pRefMbQp`, `pMbSkipSad`, `uiRefMbType`) that the mode-decision
//! half stamps as it goes. The byte sets are disjoint per worker — F107 §2
//! measured that a worker reads back only what it wrote — but **no `&mut` can
//! say so**: a macroblock's pixel span is the full width of its rows (three of
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
//! nothing else may borrow the picture — that exclusive borrow is what retires
//! F73, and it is the whole reason the view is built where the frame's plane
//! roots are stamped rather than inside the loop. From it the view captures, per
//! storage, exactly two numbers: the allocation's base address and its length.
//! Every later access re-derives a `&[Cell<T>]` from that captured pair
//! ([`SharedCells::cells`], the one place raw parts cross into cell land), so no
//! access is a child of any other and none of them can pop a sibling — S40's
//! retag-stable root shape, one level down.
//!
//! Writes then go through [`Cell`] with **no synchronisation at all**, which is
//! sound **iff** no two workers touch the same byte. That "iff" is not asserted
//! here; it is F107 §2's measurement, and it is *checked* by the two Miri
//! data-race probes this type exists to make possible — the fork/join probe in
//! `svc_encode_slice.rs` and the mid-row-boundary probe beside it. A shared
//! `Cell` retag performs no memory access, so Miri reports an **actual
//! overlapping access** rather than a retag conflict: if two macroblocks ever
//! shared a byte, the probes say so.
//!
//! Publication to post-join readers is the scope join, which is a
//! happens-before edge: everything a worker wrote before `thread::scope`
//! returns is visible to whatever reads the picture after it.
//!
//! # What is *not* claimed
//!
//! The view does not make overlapping writes safe, and it does not check
//! disjointness at run time — bounds are checked, ownership is not. It is
//! exactly as sound as the slice partition, which is why the probes are the
//! acceptance and not a nicety.

use std::cell::Cell;

use crate::encoder::encoder_context::SMVUnitXY;
use crate::encoder::picture::SPicture;

/// The one place a captured base/length pair becomes a cell slice.
///
/// Held by value in every view below, so the whole seam has exactly one
/// `UnsafeCell`-crossing accessor ([`SharedCells::cells`]) and exactly one place
/// raw parts are captured ([`SharedCells::from_parts`], which
/// [`SharedCells::capture`] and the plane build both route through).
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
    /// # Safety
    /// The base/length pair was taken at [`from_parts`](Self::from_parts) from
    /// storage borrowed exclusively at that moment — a `&mut Vec<T>` through
    /// [`capture`](Self::capture), or a `PaddedPlane`'s `root_ptr`/`buf_len` pair
    /// — and the module contract keeps that exclusive borrow the last one for the
    /// view's whole lifetime, so the range is allocated and no `&mut` to it
    /// exists. A `Cell` retag is `SharedReadWrite`, which performs no memory
    /// access, so this call cannot itself race.
    // unsafe-cat: recon-seam
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

/// **The seam's one promise, and the reason the two MT Miri probes are this
/// session's acceptance rather than a nicety.**
///
/// A captured base address is `!Sync` by inference, and every view in this
/// module is built on one — so this single `impl` is what lets a worker hold
/// `&RecPicView` across `thread::scope`. What makes the sharing sound is not
/// the `impl` but F107 §2's measurement: each worker touches only its own
/// macroblocks' bytes and entries. The `impl` is where that claim is written
/// down, and it is placed on the type that holds the raw parts rather than on
/// the picture view, because the parts are what is being promised about.
///
/// It is *exercised* — `two_threads_write_disjoint_stripes_of_one_row` and
/// `two_threads_stamp_disjoint_entries_of_one_side_array` below share one view
/// across `thread::scope` in safe code and do not compile without it — and it
/// is *checked* by those two probes and by the encoder's two fork/join probes,
/// all four under Miri's data-race detector.
///
/// Note what does **not** become `Sync`: [`RecCursor`] holds `&[Cell<u8>]` and
/// stays thread-local by inference, so a worker must make its own from the
/// shared plane rather than being handed one.
// unsafe-cat: recon-seam
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

    /// Writes `N` samples into row `dy` starting at `dx0`.
    #[inline]
    pub fn write_row<const N: usize>(&self, dy: isize, dx0: isize, src: &[u8; N]) {
        let start = idx(self.center, dx0, dy, self.stride);
        let row = &self.cells[start..][..N];
        for (c, &v) in row.iter().zip(src.iter()) {
            c.set(v);
        }
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
}

/// The reconstruction plane's flavour of `common::copy_mb`'s `copy_WxH` family:
/// a `W`x`h` block copied out of a contiguous prediction buffer into the shared
/// view.
///
/// **This is the "write-through-`&self` flavour" F107 §3 priced into D-mt-3.**
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
/// plane. Both are geometry bugs in the caller, and the raw form they replace
/// would have written into another picture.
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
    /// Captures a per-macroblock array for the frame.
    ///
    /// Public because the reconstruction picture is not the only owner of one:
    /// the rate controller's `pGomCost` has the same shape and the same fork
    /// (T9.C5). The contract is the module's, unchanged — the exclusive borrow
    /// taken here must be the last one until the view is dropped, and the
    /// workers' index sets must be disjoint.
    #[inline]
    pub fn capture(v: &mut Vec<T>) -> Self {
        Self { cells: SharedCells::capture(v) }
    }

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
    /// module contract rests on, and taking it here — once, before the fork —
    /// is what removes the per-macroblock `layer_dec_pic_mut` retags that F73
    /// named and that Miri reported as a data race on `SRefList` itself.
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

    /// **The probe the seam's one `Sync` impl exists for, and the mid-row case in
    /// miniature.** Two scoped threads write *the same rows* at different
    /// columns — the shape F107 §3 proved no `&mut [u8]` can express — through
    /// one shared view. Under Miri's data-race detector (on by default) this
    /// passes only because the byte sets are disjoint.
    ///
    /// **Calibration recipe, deliberately not a live test — and it was run.**
    /// Widen the second thread's column range from `16..32` to `0..32` so the
    /// two overlap, and re-run under Miri. Measured at this commit:
    ///
    /// ```text
    /// error: Undefined Behavior: Data race detected between (1) non-atomic
    /// write on thread `unnamed-2` and (2) retag write of type `u8` on thread
    /// `unnamed-3` at alloc296466+0x188
    /// ```
    ///
    /// That is the detector confirming it can see this class at all — S55's
    /// planted fault, aimed at the aliasing instrument rather than the byte
    /// one, and reverted. It stays a recipe rather than a `#[should_panic]`
    /// test because Miri aborts the process on UB: there is no unwinding to
    /// catch, so a live negative test would abort the whole battery.
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
    /// whole array however narrow the write — and it is the half of the seam
    /// the design brief did not scope (see the session's findings).
    #[test]
    fn two_threads_stamp_disjoint_entries_of_one_side_array() {
        let mut mvs: Vec<SMVUnitXY> = vec![SMVUnitXY::default(); 64];
        let mut qps: Vec<u8> = vec![0; 64];
        let arr = SharedMbArray { cells: SharedCells::capture(&mut mvs) };
        let qp = SharedMbArray { cells: SharedCells::capture(&mut qps) };
        // Interleaved, not split: consecutive indices land on different
        // threads, so neighbouring entries share a cache line and any
        // whole-array retag would be caught.
        //
        // Calibrated the same way as the plane probe, and also run: replacing
        // `let mut i = t` with `let mut i = 0` makes both threads write every
        // entry, and Miri stops with "Data race detected between (1)
        // non-atomic write on thread `unnamed-2` and (2) retag write of type
        // `SMVUnitXY` on thread `unnamed-3`". Reverted.
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

