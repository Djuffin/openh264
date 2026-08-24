#![forbid(unsafe_code)]

//! Padded pixel planes and the cursors that walk them — the safe replacement for
//! taxonomy class **T2** (plan §1.2, contract §2.2.1).
//!
//! # The invariant being encoded
//!
//! A decoder picture plane is one allocation of `(pad + height + pad) * stride`
//! bytes whose logical `(0, 0)` sits *inside* it, at byte `pad * stride + pad`.
//! `AllocPicture` (`decoder/pic_queue.rs:177-330`) builds exactly that:
//!
//! ```text
//! stride  = WELS_ALIGN(width  + 2*PADDING_LENGTH, PICTURE_RESOLUTION_ALIGNMENT)
//! rows    = WELS_ALIGN(height + 2*PADDING_LENGTH, PICTURE_RESOLUTION_ALIGNMENT)
//! pData[0] = pBuffer[0] + (1 + stride) * PADDING_LENGTH   // == pad*stride + pad
//! pData[1] = pBuffer[1] + ((1 + stride_c) * PADDING_LENGTH) >> 1
//! ```
//!
//! so luma is padded by 32 px and chroma by 16 px on every side. Reads at
//! `y ∈ [-pad, height+pad)`, `x ∈ [-pad, width+pad)` are therefore in-allocation *by
//! construction*, which is why the C++ can motion-compensate off the edge of the
//! picture after clamping the vector, and why `ExpandPicture` may write above row 0.
//!
//! **`pad` and `stride` are constructor parameters, never constants.** The C computes
//! both with its own alignment rules and Phase 5 has to match them byte for byte; a
//! type that baked in 32/16 would force the port to lie about chroma or about aligned
//! strides.
//!
//! # What this buys
//!
//! The hazard in the raw form is not the arithmetic, it is that the size relationship
//! between the buffer and the accesses lives *only* in pointer reinterpretations —
//! precisely the shape of finding F1, where a 16-byte array was written 32 bytes deep
//! through `from_raw_parts_mut` and nothing in the type system disagreed. Here the
//! buffer and its geometry are one value: two call sites cannot disagree about a size,
//! because there is only one size, and every access ends in a slice index.
//!
//! A panic from this module is a **port bug** (plan P13): the same call in the C++
//! would have read or written out of bounds silently. Negative logical coordinates
//! are *not* an error — they are the padding, and they are addressable.

/// Biased index arithmetic: logical `(dx, dy)` around `center` → byte offset.
///
/// This is the only place in the module where a coordinate becomes an index, and the
/// only place a cast is performed. Both parts of the safety argument live here:
///
/// * **No overflow in practice.** `stride`, `dx` and `dy` all derive from picture
///   geometry, so `|dy * stride| < 2^62` on any allocation an H.264 level permits;
///   the `isize` arithmetic cannot wrap. (In a debug build it would panic if it ever
///   did; in a release build it would wrap to a value the slice index then rejects,
///   because reaching a *valid* index would need `|dy| ≥ 2^32`.)
/// * **No silent out-of-range.** A negative sum casts to a huge `usize`, which every
///   caller feeds straight into a slice index — a panic, never a read of whatever
///   happens to be adjacent.
#[inline]
fn idx(center: usize, dx: isize, dy: isize, stride: usize) -> usize {
    (center as isize + dy * stride as isize + dx) as usize
}

/// An owned, padded pixel plane: the buffer, its geometry, and nothing else.
///
/// Mirrors the `pBuffer[i]` / `pData[i]` / `iLinesize[i]` triple of `SPicture`
/// (`decoder/picture.rs`), collapsed into one value that owns its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaddedPlane {
    /// `(height + 2*pad) * stride` bytes, or more — an allocation whose row count was
    /// rounded up (as `AllocPicture` does) is accepted as-is.
    buf: Vec<u8>,
    stride: usize,
    /// Byte offset of logical `(0, 0)`: `pad * stride + pad`.
    origin: usize,
    width: usize,
    height: usize,
    pad: usize,
}

impl PaddedPlane {
    /// Allocates a zeroed plane with `pad` pixels of padding on every side.
    ///
    /// `stride` is a parameter rather than `width + 2*pad` because the C aligns it
    /// (`WELS_ALIGN(.., PICTURE_RESOLUTION_ALIGNMENT)`) and Phase 5 must reproduce the
    /// alignment exactly.
    ///
    /// # Panics
    /// If `stride < width + 2*pad`, i.e. if a row of the padded picture would not fit
    /// in a row of the allocation. That is a geometry bug in the caller, and the C++
    /// equivalent would silently overlap rows.
    ///
    /// Note the C++ fills freshly allocated picture buffers with `128`, not `0`
    /// (`pic_queue.rs:236`, `write_bytes(pBuf0, 128u8, ..)`); a Phase 5 `Picture::new`
    /// that replaces `AllocPicture` has to do that explicitly through
    /// [`as_mut_slice`](Self::as_mut_slice).
    pub fn new(width: usize, height: usize, pad: usize, stride: usize) -> Self {
        assert!(
            stride >= width + 2 * pad,
            "stride {stride} cannot hold a padded row of {width} + 2*{pad}"
        );
        let rows = height + 2 * pad;
        Self {
            buf: vec![0u8; rows * stride],
            stride,
            origin: pad * stride + pad,
            width,
            height,
            pad,
        }
    }

    /// Adopts an existing buffer whose logical origin sits at byte `origin`.
    ///
    /// This is the constructor the Phase 2 shims feed: they own a `Vec` that the C
    /// code allocated the layout of, and hand it here rather than re-deriving the
    /// geometry at each call site.
    ///
    /// The padding is taken to be square, as every C allocation site builds it:
    /// `pad` is recovered as `origin % stride` and checked against `origin / stride`.
    ///
    /// # Panics
    /// If the layout is not self-consistent: `stride == 0`, `origin` not of the form
    /// `pad * stride + pad`, `stride < width + 2*pad`, or a buffer too small to hold
    /// `(height + 2*pad)` rows. Each of those makes some legal logical coordinate
    /// unaddressable, so accepting it would only move the failure later.
    pub fn from_parts(
        buf: Vec<u8>,
        stride: usize,
        origin: usize,
        width: usize,
        height: usize,
    ) -> Self {
        assert!(stride > 0, "stride must be non-zero");
        let pad = origin % stride;
        assert!(
            origin / stride == pad,
            "origin {origin} is not pad*stride+pad for stride {stride} (pad would be \
             {pad} horizontally, {} vertically)",
            origin / stride
        );
        assert!(
            stride >= width + 2 * pad,
            "stride {stride} cannot hold a padded row of {width} + 2*{pad}"
        );
        let need = (height + 2 * pad) * stride;
        assert!(
            buf.len() >= need,
            "buffer of {} bytes cannot hold {} rows of {stride}",
            buf.len(),
            height + 2 * pad
        );
        Self {
            buf,
            stride,
            origin,
            width,
            height,
            pad,
        }
    }

    /// A plane with a stride and no bytes.
    ///
    /// `AllocPicture`'s `bParseOnly` arm builds exactly this: it sets `iLinesize[i]`
    /// from the picture geometry and leaves `pData[i]` null, because a parse-only
    /// decode never reconstructs a sample. Every coordinate accessor panics on an
    /// empty plane — there is no addressable byte — which is the same "nothing here"
    /// the null pointer meant, reported at the access rather than at the crash.
    ///
    /// `stride` may be zero **here and nowhere else**: with no bytes to index it is
    /// metadata, not geometry, and [`from_parts`](Self::from_parts) would have to
    /// divide by it to recover the padding. `SPicture::default()` uses `empty(0)` to
    /// go on reporting the `iLinesize` of zero that its all-null pointer form had.
    pub fn empty(stride: usize) -> Self {
        Self {
            buf: Vec::new(),
            stride,
            origin: 0,
            width: 0,
            height: 0,
            pad: 0,
        }
    }

    /// Whether this plane owns no bytes — true exactly for [`empty`](Self::empty).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Picture width in pixels, excluding padding.
    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Picture height in pixels, excluding padding.
    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Padding in pixels on each of the four sides.
    #[inline]
    pub fn pad(&self) -> usize {
        self.pad
    }

    /// Bytes per row — the C++ `iLinesize[i]`.
    #[inline]
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Byte offset of logical `(0, 0)` — the C++ `pData[i] - pBuffer[i]`.
    #[inline]
    pub fn origin(&self) -> usize {
        self.origin
    }

    /// The allocation's length in bytes, padding included — `as_slice().len()`
    /// without taking the slice.
    ///
    /// The distinction matters exactly where [`root_ptr`](Self::root_ptr)'s does:
    /// a caller pairing a root address with a length must not create a `&[u8]`
    /// to learn the length, because that retag is a child of the buffer and the
    /// next `&mut` pops it. `Vec::len` reads the header, like `Vec::as_mut_ptr`.
    #[inline]
    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    /// The whole allocation, padding included — the C++ `pBuffer[i]`.
    ///
    /// The escape hatch for kernels that want to walk rows with `chunks_exact`
    /// rather than through the cursor, and for the memset-style whole-plane
    /// operations. Together with [`origin`](Self::origin) and
    /// [`stride`](Self::stride) it reproduces any access the raw form could make,
    /// still bounds-checked.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// Mutable form of [`as_slice`](Self::as_slice).
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buf
    }

    /// The buffer's **root address**, without taking a slice of it.
    ///
    /// **This is not a convenience over `as_mut_slice().as_mut_ptr()`; it is a
    /// different aliasing statement, and Phase 6 session F's exit battery is what
    /// made the difference visible.** `&mut self.buf` deref-coerces to `&mut [u8]`
    /// and that is a **`Unique` retag over the whole allocation**, so it pops every
    /// pointer previously derived from this plane. A caller that hands out a raw
    /// cursor, keeps it, and later asks the same plane for another one therefore
    /// invalidates its own first cursor — which is exactly what the encoder does
    /// (`WelsInitCurrentLayer` stamps `pEncData` from the source picture's planes,
    /// and `AnalyzePictureComplexity` asks the same picture for its planes again
    /// later in the same frame).
    ///
    /// `Vec::as_mut_ptr` reads the pointer out of the `Vec`'s own header instead, so
    /// repeated calls are sibling `SharedReadWrite` derivations that coexist — which
    /// is the C's behaviour and the behaviour every raw cursor here assumes.
    ///
    /// Still `&mut self`, because the pointer is writable and the borrow checker is
    /// the thing keeping a `&[u8]` from being live at the same time.
    #[inline]
    pub fn root_ptr(&mut self) -> *mut u8 {
        self.buf.as_mut_ptr()
    }

    /// Sample at logical `(x, y)`; negative coordinates read the padding.
    #[inline]
    pub fn at(&self, x: isize, y: isize) -> u8 {
        self.buf[idx(self.origin, x, y, self.stride)]
    }

    /// Writes the sample at logical `(x, y)`.
    #[inline]
    pub fn set(&mut self, x: isize, y: isize, v: u8) {
        let i = idx(self.origin, x, y, self.stride);
        self.buf[i] = v;
    }

    /// `len` samples of row `y` starting at logical `x0`.
    #[inline]
    pub fn row(&self, y: isize, x0: isize, len: usize) -> &[u8] {
        let start = idx(self.origin, x0, y, self.stride);
        &self.buf[start..][..len]
    }

    /// Mutable form of [`row`](Self::row).
    #[inline]
    pub fn row_mut(&mut self, y: isize, x0: isize, len: usize) -> &mut [u8] {
        let start = idx(self.origin, x0, y, self.stride);
        &mut self.buf[start..][..len]
    }

    /// A read cursor anchored at logical `(x, y)`.
    #[inline]
    pub fn cursor(&self, x: isize, y: isize) -> PlaneCursor<'_> {
        PlaneCursor::new(&self.buf, idx(self.origin, x, y, self.stride), self.stride)
    }

    /// A write cursor anchored at logical `(x, y)` — the safe form of the roving
    /// `pDstY` pointer in `decode_slice.rs:1944`.
    #[inline]
    pub fn cursor_mut(&mut self, x: isize, y: isize) -> PlaneCursorMut<'_> {
        let center = idx(self.origin, x, y, self.stride);
        PlaneCursorMut::new(&mut self.buf, center, self.stride)
    }
}

/// A read view of a plane anchored at some sample — the safe form of a `const uint8_t*`
/// walking a picture with a stride.
///
/// `Copy`, so rebasing is a value operation:
/// `let next = cur.advance(16, 0);` replaces `pSrc = pSrc.add(16)`.
#[derive(Clone, Copy, Debug)]
pub struct PlaneCursor<'a> {
    buf: &'a [u8],
    center: usize,
    stride: usize,
}

/// A read-write view of a plane anchored at some sample — the safe form of the
/// `pDstY`/`pEncMb`/`pDecMb` cursors (`decode_slice.rs:1944`,
/// `svc_base_layer_md.rs:327-358`).
///
/// Same-plane read-while-write — intra prediction reading `(-1, dy)` and `(dx, -1)`
/// while writing `(0..16, 0..16)`, deblocking straddling an MB edge — is a serial
/// read/write through one `&mut`, which safe Rust permits. What it forbids is the
/// thing that is genuinely illegal today: two live pointers into one allocation.
#[derive(Debug)]
pub struct PlaneCursorMut<'a> {
    buf: &'a mut [u8],
    center: usize,
    stride: usize,
}

impl<'a> PlaneCursor<'a> {
    /// Anchors a cursor at byte `center` of `buf`.
    ///
    /// # Panics
    /// If `stride == 0` or `center >= buf.len()`. Deeper bounds enforcement is the
    /// slice indexing in the accessors — this only rejects an anchor that could not
    /// address its own sample.
    #[inline]
    pub fn new(buf: &'a [u8], center: usize, stride: usize) -> Self {
        assert!(stride > 0, "stride must be non-zero");
        assert!(
            center < buf.len(),
            "cursor anchor {center} outside a buffer of {} bytes",
            buf.len()
        );
        Self { buf, center, stride }
    }

    /// Sample at `(dx, dy)` relative to the anchor.
    #[inline]
    pub fn at(&self, dx: isize, dy: isize) -> u8 {
        self.buf[idx(self.center, dx, dy, self.stride)]
    }

    /// `len` samples of relative row `dy`, starting at relative column `dx0`.
    #[inline]
    pub fn row(&self, dy: isize, dx0: isize, len: usize) -> &[u8] {
        let start = idx(self.center, dx0, dy, self.stride);
        &self.buf[start..][..len]
    }

    /// `h` consecutive rows of `W` samples each, as fixed-size windows, starting at
    /// relative `(dx0, dy0)`.
    ///
    /// **When to use this instead of calling [`row`](Self::row) per row.** `row` costs
    /// two bounds branches per call — one for `buf[start..]`, one for `[..len]` — and
    /// LLVM can only fold them away when it can see the stride and the buffer length.
    /// Inside a kernel reached through a shim, it can see neither: the stride arrives
    /// as a runtime `i32` from the caller and the buffer was just materialised from a
    /// pointer. A per-row `row()` walk of a 16x8 block then emits **32** compare-and-
    /// branch pairs before the first sample is read, and on a kernel as cheap per
    /// sample as SAD that is most of the run time. This walker pays one bounds check
    /// for the whole block and one `[..W]` per row, and measured 1.32-1.69x -> 0.83-
    /// 1.14x across the seven SAD shapes (T5; table in `perf_baseline.md`).
    ///
    /// **This does not repeal T4's negative result**, which is about a different
    /// thing. T4 built a `rows()` walker yielding *runtime-length* slices that each
    /// needed `[..WIDTH]` and a `try_into`, and measured it worse than `row()` in
    /// `mc.rs` — where the widths are const-generic and the checks genuinely do fold,
    /// so `row()`'s two branches cost nothing and `Chunks::next`'s `min` and
    /// `split_at` cost something. Both results hold: **use `row` where the window is
    /// statically sized and the compiler can fold the checks, and this where it
    /// cannot.** `mc.rs` was deliberately not refitted onto this.
    ///
    /// # Panics
    /// If the block leaves the buffer, at the first slicing — same contract as `row`.
    #[inline]
    pub fn row_windows<const W: usize>(
        &self,
        dy0: isize,
        dx0: isize,
        h: usize,
    ) -> impl Iterator<Item = &[u8; W]> {
        let start = idx(self.center, dx0, dy0, self.stride);
        let span = if h == 0 { 0 } else { (h - 1) * self.stride + W };
        self.buf[start..][..span]
            .chunks(self.stride)
            .map(|r| r[..W].try_into().unwrap())
    }

    /// The same view rebased by `(dx, dy)` — `pSrc.add(dy * stride + dx)`.
    ///
    /// # Panics
    /// If the new anchor is outside the buffer, per [`new`](Self::new).
    #[inline]
    pub fn advance(self, dx: isize, dy: isize) -> Self {
        Self::new(self.buf, idx(self.center, dx, dy, self.stride), self.stride)
    }

    /// Byte offset of the anchor within the underlying buffer.
    #[inline]
    pub fn center(&self) -> usize {
        self.center
    }

    /// Bytes per row.
    #[inline]
    pub fn stride(&self) -> usize {
        self.stride
    }
}

impl<'a> PlaneCursorMut<'a> {
    /// Anchors a write cursor at byte `center` of `buf`.
    ///
    /// # Panics
    /// As [`PlaneCursor::new`].
    #[inline]
    pub fn new(buf: &'a mut [u8], center: usize, stride: usize) -> Self {
        assert!(stride > 0, "stride must be non-zero");
        assert!(
            center < buf.len(),
            "cursor anchor {center} outside a buffer of {} bytes",
            buf.len()
        );
        Self { buf, center, stride }
    }

    /// Sample at `(dx, dy)` relative to the anchor.
    #[inline]
    pub fn at(&self, dx: isize, dy: isize) -> u8 {
        self.buf[idx(self.center, dx, dy, self.stride)]
    }

    /// Writes the sample at `(dx, dy)` relative to the anchor.
    #[inline]
    pub fn set(&mut self, dx: isize, dy: isize, v: u8) {
        let i = idx(self.center, dx, dy, self.stride);
        self.buf[i] = v;
    }

    /// `len` samples of relative row `dy`, starting at relative column `dx0`.
    #[inline]
    pub fn row(&self, dy: isize, dx0: isize, len: usize) -> &[u8] {
        let start = idx(self.center, dx0, dy, self.stride);
        &self.buf[start..][..len]
    }

    /// Mutable form of [`row`](Self::row) — hoist this out of inner loops rather
    /// than calling [`set`](Self::set) per sample (plan §7.4).
    #[inline]
    pub fn row_mut(&mut self, dy: isize, dx0: isize, len: usize) -> &mut [u8] {
        let start = idx(self.center, dx0, dy, self.stride);
        &mut self.buf[start..][..len]
    }

    /// `len` samples of relative row `sy` starting at relative column `sx0`, copied
    /// onto relative row `dy` starting at column `0` — **within this one plane**.
    ///
    /// This is the F42 copy: a reference list entry naming the picture being decoded
    /// makes motion compensation read and write one allocation, so there is no second
    /// cursor to hand [`row`](Self::row) and [`row_mut`](Self::row_mut) at once. Both
    /// windows are indices into the same slice and `copy_within` is what a single
    /// `&mut` can express — memmove semantics, so an overlapping window is *defined*
    /// rather than the `memcpy` the two-cursor form would have been.
    ///
    /// # Panics
    /// If either window leaves the buffer, at the slice index — same contract as
    /// [`row`](Self::row).
    #[inline]
    pub fn copy_row_within(&mut self, sx0: isize, sy: isize, dy: isize, len: usize) {
        let src = idx(self.center, sx0, sy, self.stride);
        let dst = idx(self.center, 0, dy, self.stride);
        // Both ends are checked before anything moves: `copy_within` panics on an
        // out-of-range source range or destination start, and the explicit index of
        // the destination end is what makes the message name this plane rather than
        // the slice primitive.
        let _ = &self.buf[dst..][..len];
        self.buf.copy_within(src..src + len, dst);
    }

    /// The same view rebased by `(dx, dy)`, consuming it — the `pDstY.add(16)` of
    /// the MB walk.
    ///
    /// # Panics
    /// If the new anchor is outside the buffer, per [`new`](Self::new).
    #[inline]
    pub fn advance(self, dx: isize, dy: isize) -> Self {
        let center = idx(self.center, dx, dy, self.stride);
        Self::new(self.buf, center, self.stride)
    }

    /// A *borrowed* write cursor rebased by `(dx, dy)` — the safe form of passing
    /// `pDst + dy*stride + dx` to a sub-kernel while keeping the outer pointer.
    ///
    /// [`advance`](Self::advance) consumes the cursor, which is right for a walk
    /// (`pDstY = pDstY.add(16)`) and wrong for the composite kernels, where an outer
    /// kernel hands each of its sub-blocks to an inner one and then carries on —
    /// `IdctFourResAddPred_c` calling `IdctResAddPred_c` four times is the shape.
    /// Added in Phase 2's pilot for exactly that; the returned cursor borrows `self`,
    /// so the two can never be live at once.
    ///
    /// # Panics
    /// If the new anchor is outside the buffer, per [`new`](Self::new).
    #[inline]
    pub fn reborrow(&mut self, dx: isize, dy: isize) -> PlaneCursorMut<'_> {
        let center = idx(self.center, dx, dy, self.stride);
        PlaneCursorMut::new(self.buf, center, self.stride)
    }

    /// A read-only cursor at the same anchor, borrowing this one.
    #[inline]
    pub fn as_ref(&self) -> PlaneCursor<'_> {
        PlaneCursor {
            buf: self.buf,
            center: self.center,
            stride: self.stride,
        }
    }

    /// Byte offset of the anchor within the underlying buffer.
    #[inline]
    pub fn center(&self) -> usize {
        self.center
    }

    /// Bytes per row.
    #[inline]
    pub fn stride(&self) -> usize {
        self.stride
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::prng::Prng;

    /// Luma geometry of a 176x144 QCIF picture as `AllocPicture` computes it:
    /// PADDING_LENGTH = 32, PICTURE_RESOLUTION_ALIGNMENT = 32.
    fn qcif_luma() -> PaddedPlane {
        PaddedPlane::new(176, 144, 32, 240)
    }

    #[test]
    fn geometry_matches_alloc_picture() {
        let p = qcif_luma();
        assert_eq!(p.stride(), 240);
        assert_eq!(p.pad(), 32);
        // pData[0] - pBuffer[0] == (1 + iLinesize[0]) * PADDING_LENGTH
        assert_eq!(p.origin(), (1 + 240) * 32);
        assert_eq!(p.origin(), 32 * 240 + 32);
        assert_eq!(p.as_slice().len(), (144 + 64) * 240);
    }

    #[test]
    fn all_four_padding_corners_are_addressable() {
        let mut p = qcif_luma();
        let (w, h, pad) = (176isize, 144isize, 32isize);
        let corners = [
            (-pad, -pad),
            (w + pad - 1, -pad),
            (-pad, h + pad - 1),
            (w + pad - 1, h + pad - 1),
        ];
        for (i, &(x, y)) in corners.iter().enumerate() {
            p.set(x, y, 0xA0 + i as u8);
        }
        for (i, &(x, y)) in corners.iter().enumerate() {
            assert_eq!(p.at(x, y), 0xA0 + i as u8, "corner ({x}, {y})");
        }
        // The far corner is the last addressable byte of the last padded row.
        assert_eq!(
            idx(p.origin(), w + pad - 1, h + pad - 1, p.stride()),
            (144 + 64 - 1) * 240 + (176 + 64 - 1)
        );
    }

    #[test]
    fn row_may_span_negative_x_into_the_padding() {
        let mut p = PaddedPlane::new(16, 16, 4, 24);
        for x in -4..20 {
            p.set(x, 3, (x + 4) as u8);
        }
        let r = p.row(3, -4, 24);
        assert_eq!(r.len(), 24);
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v, i as u8);
        }
        // A row read entirely inside the left padding is legal too.
        assert_eq!(p.row(3, -4, 4), &[0, 1, 2, 3]);
    }

    #[test]
    fn row_mut_writes_through() {
        let mut p = PaddedPlane::new(16, 16, 4, 24);
        p.row_mut(0, 0, 16).copy_from_slice(&[7u8; 16]);
        assert!((0..16).all(|x| p.at(x, 0) == 7));
        assert_eq!(p.at(-1, 0), 0, "the padding must not have been touched");
        assert_eq!(p.at(16, 0), 0);
    }

    #[test]
    fn from_parts_accepts_an_alloc_picture_layout() {
        let (stride, pad, w, h) = (240usize, 32usize, 176usize, 144usize);
        let buf = vec![9u8; (h + 2 * pad) * stride];
        let p = PaddedPlane::from_parts(buf, stride, pad * stride + pad, w, h);
        assert_eq!(p.pad(), pad);
        assert_eq!(p.at(0, 0), 9);
        assert_eq!(p.at(-32, -32), 9);
    }

    #[test]
    fn empty_carries_a_stride_and_owns_nothing() {
        // The `bParseOnly` picture: `iLinesize[0]` set from the geometry, `pData[0]`
        // null. Both halves of that state have to survive the conversion.
        let p = PaddedPlane::empty(224);
        assert!(p.is_empty());
        assert_eq!(p.stride(), 224);
        assert_eq!(p.as_slice().len(), 0);
        assert_eq!(p.origin(), 0);
        // And the zero-stride form, which only `SPicture::default()` builds.
        assert_eq!(PaddedPlane::empty(0).stride(), 0);
    }

    #[test]
    #[should_panic]
    fn empty_addresses_no_coordinate_at_all() {
        // Not even (0, 0): there is no byte to read, and the slice index says so
        // rather than handing back whatever the null pointer used to point at.
        PaddedPlane::empty(224).at(0, 0);
    }

    #[test]
    fn from_parts_accepts_an_over_tall_allocation() {
        // AllocPicture rounds the row count up to PICTURE_RESOLUTION_ALIGNMENT, so the
        // buffer is routinely larger than (height + 2*pad) rows.
        let (stride, pad, w, h) = (240usize, 32usize, 176usize, 140usize);
        let buf = vec![0u8; 208 * stride];
        let p = PaddedPlane::from_parts(buf, stride, pad * stride + pad, w, h);
        assert_eq!(p.height(), 140);
    }

    #[test]
    #[should_panic(expected = "cannot hold a padded row")]
    fn new_rejects_a_stride_that_cannot_hold_the_padding() {
        PaddedPlane::new(176, 144, 32, 239);
    }

    #[test]
    #[should_panic(expected = "is not pad*stride+pad")]
    fn from_parts_rejects_a_non_square_origin() {
        let stride = 240usize;
        let buf = vec![0u8; 208 * stride];
        // 16 columns of left padding but 32 rows above: not a layout this port builds.
        PaddedPlane::from_parts(buf, stride, 32 * stride + 16, 176, 144);
    }

    #[test]
    #[should_panic(expected = "cannot hold")]
    fn from_parts_rejects_a_short_buffer() {
        let stride = 240usize;
        let buf = vec![0u8; 100 * stride];
        PaddedPlane::from_parts(buf, stride, 32 * stride + 32, 176, 144);
    }

    #[test]
    #[should_panic]
    fn reading_beyond_the_padding_panics_rather_than_reading_a_neighbour() {
        let p = qcif_luma();
        // One row below the last padded row: in the C++ this is somebody else's heap.
        p.at(0, 144 + 32);
    }

    #[test]
    #[should_panic]
    fn reading_before_the_allocation_panics() {
        let p = qcif_luma();
        p.at(-33, -32);
    }

    #[test]
    fn cursor_advance_equals_a_fresh_cursor() {
        let mut p = qcif_luma();
        let mut seed = Prng::new(0xC0FFEE);
        for _ in 0..64 {
            let (mb_x, mb_y) = (seed.below(11) as isize, seed.below(9) as isize);
            p.set(mb_x * 16 + 3, mb_y * 16 + 5, 0x5A);

            let fresh = p.cursor(mb_x * 16, mb_y * 16);
            let advanced = p.cursor(0, mb_y * 16).advance(mb_x * 16, 0);
            assert_eq!(fresh.center(), advanced.center());
            assert_eq!(fresh.at(3, 5), 0x5A);
            assert_eq!(advanced.at(3, 5), 0x5A);
        }
    }

    #[test]
    fn cursor_mut_writes_land_where_the_plane_sees_them() {
        let mut p = PaddedPlane::new(64, 64, 32, 128);
        {
            let mut c = p.cursor_mut(16, 16);
            for dy in 0..16 {
                for dx in 0..16 {
                    c.set(dx, dy, (dx + dy) as u8);
                }
            }
            // Intra prediction's neighbour reads: the row above and column left of
            // the block, through the same cursor that just wrote the block.
            assert_eq!(c.at(0, 0), 0);
            assert_eq!(c.at(15, 15), 30);
            let _above = c.row(-1, -1, 18);
        }
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(p.at(16 + x, 16 + y), (x + y) as u8);
            }
        }
    }

    #[test]
    fn reborrow_addresses_a_sub_block_and_gives_the_cursor_back() {
        // IdctFourResAddPred_c's shape: four 4x4 sub-blocks of one 8x8 area, each
        // handed to an inner kernel, with the outer cursor still usable afterwards.
        let mut p = PaddedPlane::new(32, 32, 16, 64);
        let mut c = p.cursor_mut(0, 0);
        for (k, (dx, dy)) in [(0, 0), (4, 0), (0, 4), (4, 4)].into_iter().enumerate() {
            let mut sub = c.reborrow(dx, dy);
            for y in 0..4 {
                sub.row_mut(y, 0, 4).fill(0x10 + k as u8);
            }
        }
        assert_eq!(c.at(0, 0), 0x10);
        assert_eq!(c.at(7, 0), 0x11);
        assert_eq!(c.at(0, 7), 0x12);
        assert_eq!(c.at(7, 7), 0x13);
        assert_eq!(c.at(-1, -1), 0, "outside the 8x8 area, untouched");
        assert_eq!(c.at(8, 8), 0);
    }

    #[test]
    fn cursor_mut_as_ref_sees_the_same_samples() {
        let mut p = PaddedPlane::new(32, 32, 16, 64);
        let mut c = p.cursor_mut(0, 0);
        c.set(1, 1, 42);
        assert_eq!(c.as_ref().at(1, 1), 42);
        assert_eq!(c.as_ref().center(), c.center());
    }

    #[test]
    #[should_panic(expected = "cursor anchor")]
    fn cursor_rejects_an_anchor_outside_the_buffer() {
        let buf = [0u8; 64];
        PlaneCursor::new(&buf, 64, 8);
    }

    /// `row_windows` must yield exactly what the same block of `row` calls yields —
    /// it exists only to move where the bounds checks land, never what is read.
    #[test]
    fn row_windows_yields_the_same_samples_as_a_row_walk() {
        let mut rng = Prng::new(0x9114_0570);
        for &stride in &[9usize, 16, 64, 240] {
            let buf = rng.bytes(stride * 24);
            for &(dx0, dy0) in &[(0isize, 0isize), (-1, -1), (1, 2), (-1, 3)] {
                let c = PlaneCursor::new(&buf, 6 * stride + 4, stride);
                let want: Vec<&[u8]> = (0..8).map(|y| c.row(dy0 + y, dx0, 8)).collect();
                let got: Vec<&[u8; 8]> = c.row_windows::<8>(dy0, dx0, 8).collect();
                assert_eq!(got.len(), 8, "stride {stride}, offset {dx0},{dy0}");
                for (y, (w, g)) in want.iter().zip(got.iter()).enumerate() {
                    assert_eq!(*w, g.as_slice(), "stride {stride}, row {y}");
                }
            }
        }
    }

    /// The last row of the block is `W` samples, not a whole stride, so the walker's
    /// final chunk is short — it must still yield a full `W`-wide window rather than
    /// panicking or dropping the row. A block at the very end of its allocation is
    /// where an over-long span would be caught, and this is that block.
    #[test]
    fn row_windows_reaches_the_last_row_of_a_block_that_ends_the_buffer() {
        let stride = 20usize;
        let buf: Vec<u8> = (0..(3 * stride + 8) as u8).collect();
        let c = PlaneCursor::new(&buf, 0, stride);
        let rows: Vec<&[u8; 8]> = c.row_windows::<8>(0, 0, 4).collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3][0], (3 * stride) as u8);
        assert_eq!(rows[3][7], (3 * stride + 7) as u8);
    }

    #[test]
    fn row_windows_of_zero_rows_is_empty() {
        let buf = [7u8; 64];
        let c = PlaneCursor::new(&buf, 0, 8);
        assert_eq!(c.row_windows::<4>(0, 0, 0).count(), 0);
    }
}
