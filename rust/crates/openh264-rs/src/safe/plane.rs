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
}
