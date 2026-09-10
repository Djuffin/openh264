#![forbid(unsafe_code)]

//! Padded pixel planes and the cursors that walk them.
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
//! `y ∈ [-pad, height+pad)`, `x ∈ [-pad, width+pad)` are therefore in-allocation by
//! construction, which is why motion compensation may run off the edge of the picture
//! after clamping the vector, and why `ExpandPicture` may write above row 0.
//!
//! `pad` and `stride` are constructor parameters, never constants — the C computes both
//! with its own alignment rules. Negative logical coordinates are not an error: they are
//! the padding, and they are addressable.

/// Biased index arithmetic: logical `(dx, dy)` around `center` → byte offset.
///
/// The only place in the module where a coordinate becomes an index, and the only place
/// a cast is performed:
///
/// * No overflow. `stride`, `dx` and `dy` all derive from picture geometry, so
///   `|dy * stride| < 2^62` on any allocation an H.264 level permits and the `isize`
///   arithmetic cannot wrap; a wrap would need `|dy| ≥ 2^32` to reach a valid index.
/// * No silent out-of-range. A negative sum casts to a huge `usize`, which every caller
///   feeds straight into a slice index — a panic, never an adjacent read.
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
    /// (`WELS_ALIGN(.., PICTURE_RESOLUTION_ALIGNMENT)`).
    ///
    /// # Panics
    /// If `stride < width + 2*pad`, i.e. if a row of the padded picture would not fit
    /// in a row of the allocation. That is a geometry bug in the caller.
    ///
    /// Freshly allocated picture buffers are filled with `128`, not `0`
    /// (`pic_queue.rs:236`); `Picture::new` does that explicitly through
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
    /// The padding is taken to be square, as every C allocation site builds it:
    /// `pad` is recovered as `origin % stride` and checked against `origin / stride`.
    ///
    /// # Panics
    /// If the layout is not self-consistent: `stride == 0`, `origin` not of the form
    /// `pad * stride + pad`, `stride < width + 2*pad`, or a buffer too small to hold
    /// `(height + 2*pad)` rows — each leaves some legal logical coordinate
    /// unaddressable.
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
    /// `AllocPicture`'s `bParseOnly` arm builds this: `iLinesize[i]` from the picture
    /// geometry, `pData[i]` null, because a parse-only decode never reconstructs a
    /// sample. Every coordinate accessor panics on an empty plane — there is no
    /// addressable byte.
    ///
    /// `stride` may be zero here and nowhere else: with no bytes to index it is
    /// metadata, not geometry, and [`from_parts`](Self::from_parts) would have to divide
    /// by it to recover the padding. `SPicture::default()` uses `empty(0)`.
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
    /// A caller pairing a root address with a length must not create a `&[u8]` to learn
    /// the length: that retag is a child of the buffer and the next `&mut` pops it.
    /// `Vec::len` reads the header, like `Vec::as_mut_ptr`.
    #[inline]
    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    /// The whole allocation, padding included — the C++ `pBuffer[i]`.
    ///
    /// For kernels that walk rows with `chunks_exact` rather than through the cursor,
    /// and for whole-plane memset-style operations. With [`origin`](Self::origin) and
    /// [`stride`](Self::stride) it reproduces any access the raw form could make, still
    /// bounds-checked.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// Mutable form of [`as_slice`](Self::as_slice).
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buf
    }

    /// The buffer's root address, without taking a slice of it.
    ///
    /// Not a convenience over `as_mut_slice().as_mut_ptr()` but a different aliasing
    /// statement: `&mut self.buf` deref-coerces to `&mut [u8]`, a `Unique` retag over
    /// the whole allocation, which pops every pointer previously derived from this
    /// plane — so a caller that keeps one raw cursor and then asks the same plane for
    /// another would invalidate the first (`WelsInitCurrentLayer` stamps `pEncData`
    /// from the source picture's planes, and `AnalyzePictureComplexity` asks the same
    /// picture for its planes again later in the frame).
    ///
    /// `Vec::as_mut_ptr` reads the pointer out of the `Vec`'s own header instead, so
    /// repeated calls are sibling `SharedReadWrite` derivations that coexist, which is
    /// what every raw cursor here assumes.
    ///
    /// Still `&mut self`, because the pointer is writable and the borrow checker is what
    /// keeps a `&[u8]` from being live at the same time.
    #[inline]
    pub fn root_ptr(&mut self) -> *mut u8 {
        self.buf.as_mut_ptr()
    }

    /// The same root, reached through `&self`.
    ///
    /// [`root_ptr`](Self::root_ptr) is sound for one thread and wrong under the fork:
    /// `&mut self` is a `Unique` retag over the plane's own header words, and every
    /// worker resolves the same reference picture per call (`layer_ref_pic`), so two of
    /// them retagging it at once is a data race even though neither writes the header.
    /// This reads the buffer pointer out through a shared borrow instead — identical
    /// address, the buffer's own whole-allocation provenance, no exclusive claim on the
    /// container. See `MbArray::root_ptr`.
    #[inline]
    pub fn root_ptr_shared(&self) -> *mut u8 {
        self.buf.as_ptr() as *mut u8
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

/// A read cursor over pixel samples, in whichever storage the plane lives in.
///
/// `RefSamples` cannot carry `advance` because `PlaneCursorMut` implements it and holds
/// a `&mut`, so it is not `Copy`. This trait is the `Copy` half, for kernels that walk
/// sub-blocks and must rebase over both a plain slice plane (`PlaneCursor`) and a shared
/// interior-mutable one (`RecCursor`).
///
/// A kernel has to accept both because the encoder's source picture is not read-only:
/// `VaaBackgroundMbDataUpdate` copies previous-source into current-source in-fork, per
/// macroblock, so a source plane is reached through the shared seam exactly as the
/// reconstruction planes are, while a prediction scratch on `SMbCache` is an owned array
/// and stays a plain slice.
pub trait SampleCursor: Copy {
    /// Sample at `(dx, dy)` from the anchor.
    fn at(&self, dx: isize, dy: isize) -> u8;

    /// `N` samples of row `dy` starting at `dx0`, by value — a shared view cannot
    /// lend a slice into its cells.
    fn row_n<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N];

    /// The same anchor moved by `(dx, dy)`.
    fn advance(self, dx: isize, dy: isize) -> Self;
}

impl SampleCursor for PlaneCursor<'_> {
    #[inline]
    fn at(&self, dx: isize, dy: isize) -> u8 {
        PlaneCursor::at(self, dx, dy)
    }
    #[inline]
    fn row_n<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N] {
        let r = PlaneCursor::row(self, dy, dx0, N);
        let mut out = [0u8; N];
        out.copy_from_slice(r);
        out
    }
    #[inline]
    fn advance(self, dx: isize, dy: isize) -> Self {
        PlaneCursor::advance(self, dx, dy)
    }
}

/// The rows of a block whose bounds have already been checked once, as a whole.
///
/// Handed out by [`RefSamples::span`], which is where the checking happens and where the
/// argument for why the rows inside are free is written down.
pub trait BlockRows {
    /// `W` samples of row `y` starting at column `x` of the span, by value.
    ///
    /// Coordinates are span-relative and unsigned: whatever `(dx0, dy0)` the span was cut
    /// at is its `(0, 0)`. By value for the reason [`RefSamples::row_n`] gives — a shared
    /// view cannot lend a slice into its cells.
    ///
    /// # Panics
    /// If `y * stride + x + W` leaves the span, which a caller reading the block the span
    /// was cut for cannot reach.
    fn row<const W: usize>(&self, y: usize, x: usize) -> [u8; W];

    /// The `h`-row, `W`-wide window starting at row `y` of this span, as a span of
    /// its own: `(h - 1) * stride + W` samples, row 0 of which is row `y` of this one.
    ///
    /// [`row`](Self::row) is free only where the row index is a constant: a symbolic
    /// `y * stride` is something the compiler cannot place inside the span, so a row loop
    /// it does not unroll keeps its per-row checks. Cutting a window per group of rows
    /// costs one check and the constant offsets inside it fold; the four-point SAD
    /// kernels are the loops too large to unroll.
    ///
    /// It cuts from the span rather than the cursor because [`RefSamples::span`] has to
    /// validate the stride, and that check would then sit in the row loop; a window
    /// inherits the validated stride.
    ///
    /// # Panics
    /// If the window leaves the span.
    fn window<const W: usize>(&self, y: usize, h: usize) -> Self
    where
        Self: Sized;
}

pub trait RefSamples {
    /// Sample at `(dx, dy)` from the anchor.
    fn at(&self, dx: isize, dy: isize) -> u8;

    /// `N` samples of row `dy` starting at `dx0`, by value.
    ///
    /// By value rather than by reference because a shared view cannot lend a slice into
    /// its cells; every reference row an intra predictor reads is 3, 4, 8 or 16 samples,
    /// so the copy is a register-file move.
    fn row_n<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N];

    /// `h` consecutive rows of `N` samples each from `(dx0, dy0)` — the folded block
    /// walk, which lets the SAD family be generic without paying for it.
    ///
    /// Not named `row_windows`: [`PlaneCursor::row_windows`] and `RecCursor`'s are
    /// inherent methods yielding `&[u8; N]` and `&[Cell<u8>; N]`, and an inherent method
    /// wins over a same-named trait method wherever the receiver's type is known, so the
    /// spelling would mean different things in generic and concrete code.
    ///
    /// Yields [`Row`](Self::Row) — a borrow for the plane cursors, an owned `RowBuf` only
    /// for the cell view; `N` sizes the slice, not the returned type. This walk slices
    /// once per block per side, where a per-row `row_n` walk makes a 16x8 SAD emit 32
    /// bounds branches before reading a sample.
    ///
    /// # Panics
    /// If the block leaves the buffer, at the first slicing.
    fn row_blocks<const N: usize>(
        &self,
        dy0: isize,
        dx0: isize,
        h: usize,
    ) -> impl Iterator<Item = Self::Row<'_>>;

    /// Rows of a block that has been bounds-checked once, one type per implementor:
    /// `&[u8]` plus a stride for the plane cursors, `&[Cell<u8>]` plus a stride for the
    /// shared view. Both hand out rows by value through [`BlockRows::row`].
    type Span<'a>: BlockRows
    where
        Self: 'a;

    /// The `W`x`H` block at `(dx0, dy0)` as one bounds-checked span:
    /// `(H - 1) * stride + W` samples in which row `y`, column `x` is at
    /// `y * stride + x`.
    ///
    /// [`row_n`](Self::row_n) pays two slice checks per row — one for `buf[start..]`, one
    /// for `[..N]` — and inside a kernel reached through a shim LLVM can fold neither:
    /// the stride arrives as a run-time value and the buffer was just materialised from a
    /// pointer. A 16x16 SAD reading both operands that way emits 64 compare-and-branch
    /// pairs before the 32 `uabal`s that are its actual work.
    ///
    /// This pays those two checks once per operand, and the rows inside are then free:
    /// the span's length is `(H - 1) * stride + W` by construction, so
    /// `y * stride + x + W <= len` holds for every row the block contains and LLVM drops
    /// the branches. `processing/vaacalc.rs::half_mb_stats` turns on the same argument.
    ///
    /// That holds for a constant `y`, so a row loop LLVM unrolls — the single-block SAD
    /// and SATD shapes — comes out with no per-row branch at all. A loop too large to
    /// unroll leaves `y * stride` symbolic, which no span length places inside the span;
    /// those loops take the block a group of rows at a time through
    /// [`BlockRows::window`], which restores the constant offsets. The four-point SADs
    /// are that case.
    ///
    /// [`row_blocks`](Self::row_blocks) also checks once per block, but buys it with two
    /// integer divisions — `chunks(stride)` divides to count the chunks and again to
    /// bound the last — and hands rows over borrowed rather than by value. On a vector
    /// kernel, whose row is a register either way, the span is cheaper; on the scalar
    /// reference in `common/sad_common.rs`, whose row is what LLVM vectorises, the borrow
    /// is worth more than the divisions and it keeps `row_blocks`.
    ///
    /// The span is a window, not a block: `W` and `H` size the reach, and a caller whose
    /// probes leave the block asks for the reach it needs. The four-point SAD reads `x`
    /// in `-1 .. W + 1` and `y` in `-1 .. H + 1`, so it cuts `span::<W + 2, H + 2>(-1,
    /// -1)` — with `H + 2` passed as its own const parameter, stable Rust having no
    /// arithmetic in a const-argument position — and indexes its probes at span columns
    /// 0, 1 and 2.
    ///
    /// # Panics
    /// If the block leaves the buffer, at the slicing — same contract as
    /// [`row_n`](Self::row_n). Negative `dx0`/`dy0` are ordinary (they address the
    /// padding); one that is genuinely out of range casts to a huge `usize` and
    /// panics at the slice rather than reading anything.
    fn span<const W: usize, const H: usize>(&self, dy0: isize, dx0: isize) -> Self::Span<'_>;

    /// The same anchor moved by `(dx, dy)` — `pSrc.add(dy * stride + dx)`.
    ///
    /// Both plane cursors and the shared cell cursor have an inherent `advance` with this
    /// signature; this is the one generic code can call, and `common/mc.rs`'s quarter-pel
    /// arms are what need it. The inherent method wins wherever the receiver's type is
    /// known, so no concrete call site changes meaning by this existing.
    #[must_use]
    fn advance(self, dx: isize, dy: isize) -> Self
    where
        Self: Sized;

    /// A run-time-length row, borrowed where the cursor can lend one and owned only where
    /// it cannot.
    ///
    /// [`row_blocks`](Self::row_blocks) covers the fixed-size block walks and
    /// [`span`](Self::span) the shaped ones; this is for a caller whose row length is a
    /// run-time value and has neither — the two 4-sample prediction rows the `wide` and
    /// SSE2 DCT kernels read. An associated type because a copy here is measurable:
    /// `Row<'a> = &'a [u8]` for the plane cursors, and only
    /// [`RecCursor`](crate::encoder::rec_view::RecCursor) pays for a copy.
    ///
    /// The bound is `Deref<Target = [u8]>`, so a consumer writes `row[j]` and
    /// `row.iter()` without knowing which it got.
    type Row<'a>: std::ops::Deref<Target = [u8]>
    where
        Self: 'a;

    /// `len` samples of row `dy` starting at `dx0`.
    ///
    /// # Panics
    /// If the row leaves the buffer, at the slicing — same contract as
    /// [`row_n`](Self::row_n).
    fn row_view(&self, dy: isize, dx0: isize, len: usize) -> Self::Row<'_>;
}

/// Longest run-time row [`RefSamples::row_view`] will carry by value.
///
/// The remaining callers read four samples; 32 is headroom.
pub const ROW_BUF_MAX: usize = 32;

/// An owned row — [`RefSamples::Row`] for the cursors that cannot lend one.
///
/// For one implementor only: a shared cell view has no `&[u8]` to hand out, so its row is
/// copied into this. Every other implementor's `Row` is a borrow.
#[derive(Clone, Copy, Debug)]
pub struct RowBuf {
    buf: [u8; ROW_BUF_MAX],
    len: usize,
}

impl RowBuf {
    /// An empty row of `len` samples, to be filled by the caller.
    ///
    /// # Panics
    /// If `len` exceeds [`ROW_BUF_MAX`].
    #[inline]
    pub fn new(len: usize) -> Self {
        assert!(
            len <= ROW_BUF_MAX,
            "row of {len} samples exceeds ROW_BUF_MAX"
        );
        Self {
            buf: [0; ROW_BUF_MAX],
            len,
        }
    }

    /// The writable prefix.
    #[inline]
    pub fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..self.len]
    }
}

impl std::ops::Deref for RowBuf {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

/// A read view of a plane anchored at some sample — the safe form of a `const uint8_t*`
/// walking a picture with a stride.
///
/// `Copy`, so rebasing is a value operation: `cur.advance(16, 0)` replaces
/// `pSrc = pSrc.add(16)`.
#[derive(Clone, Copy, Debug)]
pub struct PlaneCursor<'a> {
    buf: &'a [u8],
    center: usize,
    stride: usize,
}

impl RefSamples for PlaneCursor<'_> {
    #[inline]
    fn at(&self, dx: isize, dy: isize) -> u8 {
        PlaneCursor::at(self, dx, dy)
    }

    #[inline]
    fn row_n<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N] {
        let r: &[u8; N] = PlaneCursor::row(self, dy, dx0, N).try_into().unwrap();
        *r
    }

    #[inline]
    fn row_blocks<const N: usize>(
        &self,
        dy0: isize,
        dx0: isize,
        h: usize,
    ) -> impl Iterator<Item = &[u8]> {
        PlaneCursor::row_windows::<N>(self, dy0, dx0, h).map(|r| &r[..])
    }

    type Span<'a>
        = PlaneSpan<'a>
    where
        Self: 'a;

    #[inline]
    fn span<const W: usize, const H: usize>(&self, dy0: isize, dx0: isize) -> PlaneSpan<'_> {
        PlaneSpan::cut(
            self.buf,
            idx(self.center, dx0, dy0, self.stride),
            self.stride,
            W,
            H,
        )
    }

    type Row<'a>
        = &'a [u8]
    where
        Self: 'a;

    #[inline]
    fn row_view(&self, dy: isize, dx0: isize, len: usize) -> &[u8] {
        PlaneCursor::row(self, dy, dx0, len)
    }

    #[inline]
    fn advance(self, dx: isize, dy: isize) -> Self {
        PlaneCursor::advance(self, dx, dy)
    }
}

/// A plane cursor that can be read *and* written — [`RefSamples`] plus `set`.
///
/// The deblocking filters are the one kernel family that reads and writes the same
/// samples, over two storages: the decoder's ordinary picture as [`PlaneCursorMut`], and
/// the encoder's reconstruction picture through the seam's `RecCursor`.
///
/// `set` takes `&mut self` even though `RecCursor` can write through `&self` — the
/// stricter signature is the one that fits both. No method here hands out `&mut [u8]`
/// into the plane.
pub trait PlaneSamples: RefSamples {
    /// Writes the sample at `(dx, dy)` from the anchor.
    fn set(&mut self, dx: isize, dy: isize, v: u8);

    /// Bytes per row of the plane this view is anchored in.
    ///
    /// The scalar deblocking kernels never need this — they address in flat byte offsets
    /// and are stride-agnostic (`deblocking_common.rs:52`). Their SSE2 twins address in 2D
    /// through the cursor, which requires the caller's cross-line step to be this stride;
    /// exposing it lets them check that rather than assume it.
    fn stride(&self) -> usize;

    /// Writes `N` contiguous samples starting at `(dx0, dy)`.
    #[inline]
    fn set_row_n<const N: usize>(&mut self, dy: isize, dx0: isize, val: &[u8; N]) {
        for (i, &v) in val.iter().enumerate() {
            self.set(dx0 + i as isize, dy, v);
        }
    }

    /// Writes a `W`-wide, `H`-tall block of rows at `(dx0, dy0)` — the write side's twin
    /// of [`RefSamples::span`].
    ///
    /// The default below is `H` calls to [`set_row_n`](Self::set_row_n), i.e. `H` bounds
    /// checks over `H` separately-derived addresses. Both cursor implementations override
    /// it to cut one span and walk it, so a block costs one check however tall it is; see
    /// [`RefSamples::span`] for why the rows inside a cut span fold, and `PlaneSpan::cut`
    /// for the narrowed stride that makes them. The deblocking filters are the caller: a
    /// vertical edge writes sixteen lines four or six samples wide.
    ///
    /// # Panics
    /// If the block leaves the buffer.
    #[inline]
    fn set_block<const W: usize, const H: usize>(
        &mut self,
        dy0: isize,
        dx0: isize,
        rows: &[[u8; W]; H],
    ) {
        for (y, r) in rows.iter().enumerate() {
            self.set_row_n::<W>(dy0 + y as isize, dx0, r);
        }
    }
}

/// A read-write view of a plane anchored at some sample — the safe form of the
/// `pDstY`/`pEncMb`/`pDecMb` cursors (`decode_slice.rs:1944`,
/// `svc_base_layer_md.rs:327-358`).
///
/// Same-plane read-while-write — intra prediction reading `(-1, dy)` and `(dx, -1)`
/// while writing `(0..16, 0..16)`, deblocking straddling an MB edge — is a serial
/// read/write through one `&mut`, which safe Rust permits.
#[derive(Debug)]
pub struct PlaneCursorMut<'a> {
    buf: &'a mut [u8],
    center: usize,
    stride: usize,
}

/// [`RefSamples::Span`] for the two plane cursors: a block's bytes and the stride to
/// walk them by.
///
/// `buf` is exactly `(h - 1) * stride + w` bytes, and that exactness is what lets the
/// per-row slicing below fold away. See [`RefSamples::span`], and `PlaneSpan::cut` for
/// why the stride is narrowed.
#[derive(Clone, Copy, Debug)]
pub struct PlaneSpan<'a> {
    buf: &'a [u8],
    /// A `u32`, and that is load-bearing: `row` needs LLVM to see that
    /// `y * stride + x + W` cannot exceed the span's `(H - 1) * stride + SW` bytes, and
    /// with a `usize` stride `y * stride` may wrap, so nothing follows from `y <= H - 1`
    /// and the per-row check stays. Narrowed to 32 bits and widened back at each use, the
    /// product of a row index under 16 and a stride under `2^32` provably fits and the
    /// branch folds away. Every picture line size in the codec is an `int32_t`, and the
    /// cursors assert as much when made, so nothing real is ruled out.
    stride: u32,
}

impl<'a> PlaneSpan<'a> {
    /// Cuts the `w`x`h` block at byte `start` of `buf` out as a span.
    ///
    /// The length is computed from the narrowed stride, the same value
    /// [`BlockRows::row`] multiplies by, so the two are one SSA value and the row bound
    /// follows from the span bound. Building the slice and the row offsets out of
    /// different spellings of the stride loses that, and with it every per-row check this
    /// exists to remove.
    ///
    /// The narrowing is unchecked here: every cursor asserts the bound when it is made
    /// (see [`PlaneCursor::new`]), and a check per cut — a panicking branch inside a row
    /// loop — measured 1.6x slower on the four-point 16x16 SAD.
    ///
    /// # Panics
    /// If the block leaves `buf`.
    #[inline]
    fn cut(buf: &'a [u8], start: usize, stride: usize, w: usize, h: usize) -> Self {
        debug_assert!(stride <= u32::MAX as usize, "cursor stride bound violated");
        let stride = stride as u32;
        let len = if h == 0 {
            0
        } else {
            (h - 1) * stride as usize + w
        };
        Self {
            buf: &buf[start..][..len],
            stride,
        }
    }
}

impl BlockRows for PlaneSpan<'_> {
    #[inline]
    fn row<const W: usize>(&self, y: usize, x: usize) -> [u8; W] {
        let r: &[u8; W] = self.buf[y * self.stride as usize + x..][..W]
            .try_into()
            .unwrap();
        *r
    }

    #[inline]
    fn window<const W: usize>(&self, y: usize, h: usize) -> Self {
        let stride = self.stride as usize;
        let len = if h == 0 { 0 } else { (h - 1) * stride + W };
        Self {
            buf: &self.buf[y * stride..][..len],
            stride: self.stride,
        }
    }
}

/// The write side of [`PlaneSpan`]: a block's bytes and the stride to walk them by, lent
/// out one row at a time as `&mut [u8; W]`.
///
/// [`PlaneCursorMut::row_mut`] pays the same two unfoldable per-row slice checks
/// [`PlaneSpan`] exists to remove, and a block copy is a row loop over both sides. The
/// invariant is the same: `buf` is exactly `(H - 1) * stride + W` bytes, so
/// `y * stride + x + W <= len` holds for every row the block contains and LLVM drops the
/// branch.
///
/// Not `BlockRows`: that trait hands rows out by value because a shared cell view has no
/// `&[u8]` to lend, and a destination has to be written through. The reconstruction seam's
/// own write path is
/// [`RecCursor::write_row`](crate::encoder::rec_view::RecCursor::write_row).
#[derive(Debug)]
pub struct PlaneSpanMut<'a> {
    buf: &'a mut [u8],
    /// A `u32` for the reason [`PlaneSpan`]'s is — it is what makes `y * stride`
    /// provably unable to wrap, and so what lets the per-row check fold away.
    stride: u32,
}

impl<'a> PlaneSpanMut<'a> {
    /// Cuts the `w`x`h` block at byte `start` of `buf` out as a writable span.
    ///
    /// Length and row offsets come from one narrowed stride, as `PlaneSpan::cut`
    /// explains; the narrowing is unchecked here because the bound is a cursor invariant,
    /// asserted in [`PlaneCursorMut::new`].
    ///
    /// # Panics
    /// If the block leaves `buf`.
    #[inline]
    fn cut(buf: &'a mut [u8], start: usize, stride: usize, w: usize, h: usize) -> Self {
        debug_assert!(stride <= u32::MAX as usize, "cursor stride bound violated");
        let stride = stride as u32;
        let len = if h == 0 {
            0
        } else {
            (h - 1) * stride as usize + w
        };
        Self {
            buf: &mut buf[start..][..len],
            stride,
        }
    }

    /// `W` writable samples of row `y` starting at column `x` of the span.
    ///
    /// # Panics
    /// If `y * stride + x + W` leaves the span — which a caller writing the block the
    /// span was cut for cannot reach.
    #[inline]
    pub fn row_mut<const W: usize>(&mut self, y: usize, x: usize) -> &mut [u8; W] {
        (&mut self.buf[y * self.stride as usize + x..][..W])
            .try_into()
            .unwrap()
    }

    /// The `h`-row, `W`-wide window starting at row `y` — [`BlockRows::window`]'s write
    /// side.
    ///
    /// [`row_mut`](Self::row_mut) is free only where `y` is a constant: a row loop the
    /// compiler declines to unroll leaves `y * stride` symbolic, which no span length can
    /// be shown to contain, so the two per-row checks stay. The motion compensation
    /// kernels are those loops — a six-tap filter body is far past the unroller's
    /// threshold at sixteen rows — so they walk the block a group of rows at a time: one
    /// window cut per group, constant row offsets inside it. See [`RefSamples::span`].
    ///
    /// # Panics
    /// If the window leaves the span.
    #[inline]
    pub fn window_mut<const W: usize>(&mut self, y: usize, h: usize) -> PlaneSpanMut<'_> {
        let stride = self.stride as usize;
        let len = if h == 0 { 0 } else { (h - 1) * stride + W };
        PlaneSpanMut {
            buf: &mut self.buf[y * stride..][..len],
            stride: self.stride,
        }
    }
}

impl<'a> PlaneCursor<'a> {
    /// Anchors a cursor at byte `center` of `buf`.
    ///
    /// `stride` must fit in a `u32`. Every picture line size in the codec is an `int32_t`,
    /// so this rules out nothing real, and it is what lets [`RefSamples::span`] narrow the
    /// stride without a check of its own — the narrowing is what makes `y * stride`
    /// provably unable to wrap, and so what lets the compiler drop a block's per-row
    /// bounds checks. Checked once here rather than at every span a row loop cuts.
    ///
    /// # Panics
    /// If `stride == 0`, `stride > u32::MAX`, or `center >= buf.len()`. Deeper bounds
    /// enforcement is the slice indexing in the accessors — this only rejects an
    /// anchor that could not address its own sample.
    #[inline]
    pub fn new(buf: &'a [u8], center: usize, stride: usize) -> Self {
        assert!(stride > 0, "stride must be non-zero");
        assert!(stride <= u32::MAX as usize, "stride {stride} exceeds u32");
        assert!(
            center < buf.len(),
            "cursor anchor {center} outside a buffer of {} bytes",
            buf.len()
        );
        Self {
            buf,
            center,
            stride,
        }
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
    /// [`row`](Self::row) costs two bounds branches per call — one for `buf[start..]`, one
    /// for `[..len]` — which LLVM folds away only where it can see the stride and the
    /// buffer length. Inside a kernel reached through a shim it can see neither, and a
    /// per-row `row()` walk of a 16x8 block then emits 32 compare-and-branch pairs before
    /// the first sample is read. This walker pays one bounds check for the whole block and
    /// one `[..W]` per row, so use `row` where the compiler can fold the checks and this
    /// where it cannot.
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
    /// Only the anchor is re-checked: this cursor's stride already satisfied
    /// [`new`](Self::new)'s two stride assertions when it was made, and rebasing does not
    /// change it.
    ///
    /// # Panics
    /// If the new anchor is outside the buffer, per [`new`](Self::new).
    #[inline]
    pub fn advance(self, dx: isize, dy: isize) -> Self {
        let center = idx(self.center, dx, dy, self.stride);
        assert!(
            center < self.buf.len(),
            "cursor anchor {center} outside a buffer of {} bytes",
            self.buf.len()
        );
        Self {
            buf: self.buf,
            center,
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

impl<'a> PlaneCursorMut<'a> {
    /// Anchors a write cursor at byte `center` of `buf`.
    ///
    /// # Panics
    /// As [`PlaneCursor::new`], including the `u32` stride bound.
    #[inline]
    pub fn new(buf: &'a mut [u8], center: usize, stride: usize) -> Self {
        assert!(stride > 0, "stride must be non-zero");
        assert!(stride <= u32::MAX as usize, "stride {stride} exceeds u32");
        assert!(
            center < buf.len(),
            "cursor anchor {center} outside a buffer of {} bytes",
            buf.len()
        );
        Self {
            buf,
            center,
            stride,
        }
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
    /// than calling [`set`](Self::set) per sample.
    #[inline]
    pub fn row_mut(&mut self, dy: isize, dx0: isize, len: usize) -> &mut [u8] {
        let start = idx(self.center, dx0, dy, self.stride);
        &mut self.buf[start..][..len]
    }

    /// The `W`x`H` block at `(dx0, dy0)` as one bounds-checked writable span — the
    /// destination twin of [`RefSamples::span`], and the reason a block copy pays no
    /// per-row check on either side.
    ///
    /// Not a trait method: [`RefSamples`] is read-only by construction, and the one write
    /// path that is not a `&mut [u8]` — the reconstruction seam — writes rows by value.
    ///
    /// # Panics
    /// If the block leaves the buffer, at the slicing — same contract as
    /// [`row_mut`](Self::row_mut).
    #[inline]
    pub fn span_mut<const W: usize, const H: usize>(
        &mut self,
        dy0: isize,
        dx0: isize,
    ) -> PlaneSpanMut<'_> {
        let start = idx(self.center, dx0, dy0, self.stride);
        PlaneSpanMut::cut(self.buf, start, self.stride, W, H)
    }

    /// `len` samples of relative row `sy` starting at relative column `sx0`, copied
    /// onto relative row `dy` starting at column `0`, within this one plane.
    ///
    /// A reference list entry naming the picture being decoded makes motion compensation
    /// read and write one allocation, so there is no second cursor to hand
    /// [`row`](Self::row) and [`row_mut`](Self::row_mut) at once. Both windows are indices
    /// into the same slice, and `copy_within` has memmove semantics, so an overlapping
    /// window is defined.
    ///
    /// # Panics
    /// If either window leaves the buffer, at the slice index — same contract as
    /// [`row`](Self::row).
    #[inline]
    pub fn copy_row_within(&mut self, sx0: isize, sy: isize, dy: isize, len: usize) {
        let src = idx(self.center, sx0, sy, self.stride);
        let dst = idx(self.center, 0, dy, self.stride);
        // Both ends are checked before anything moves: `copy_within` panics on an
        // out-of-range source range or destination start, and the explicit index of the
        // destination end makes the message name this plane rather than the slice
        // primitive.
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
    /// `IdctFourResAddPred_c` calling `IdctResAddPred_c` four times. The returned cursor
    /// borrows `self`, so the two can never be live at once.
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

impl RefSamples for PlaneCursorMut<'_> {
    #[inline]
    fn at(&self, dx: isize, dy: isize) -> u8 {
        PlaneCursorMut::at(self, dx, dy)
    }

    #[inline]
    fn row_n<const N: usize>(&self, dy: isize, dx0: isize) -> [u8; N] {
        let r: &[u8; N] = PlaneCursorMut::row(self, dy, dx0, N).try_into().unwrap();
        *r
    }

    #[inline]
    fn row_blocks<const N: usize>(
        &self,
        dy0: isize,
        dx0: isize,
        h: usize,
    ) -> impl Iterator<Item = &[u8]> {
        let start = idx(self.center, dx0, dy0, self.stride);
        let span = if h == 0 { 0 } else { (h - 1) * self.stride + N };
        self.buf[start..][..span]
            .chunks(self.stride)
            .map(|r| &r[..N])
    }

    type Span<'a>
        = PlaneSpan<'a>
    where
        Self: 'a;

    #[inline]
    fn span<const W: usize, const H: usize>(&self, dy0: isize, dx0: isize) -> PlaneSpan<'_> {
        PlaneSpan::cut(
            self.buf,
            idx(self.center, dx0, dy0, self.stride),
            self.stride,
            W,
            H,
        )
    }

    type Row<'a>
        = &'a [u8]
    where
        Self: 'a;

    #[inline]
    fn row_view(&self, dy: isize, dx0: isize, len: usize) -> &[u8] {
        PlaneCursorMut::row(self, dy, dx0, len)
    }

    /// The write cursor's `advance` consumes it, unlike the two read cursors' — a
    /// `&mut [u8]` cannot be copied.
    #[inline]
    fn advance(self, dx: isize, dy: isize) -> Self {
        let center = idx(self.center, dx, dy, self.stride);
        Self {
            buf: self.buf,
            center,
            stride: self.stride,
        }
    }
}

impl PlaneSamples for PlaneCursorMut<'_> {
    #[inline]
    fn stride(&self) -> usize {
        PlaneCursorMut::stride(self)
    }

    #[inline]
    fn set(&mut self, dx: isize, dy: isize, v: u8) {
        PlaneCursorMut::set(self, dx, dy, v)
    }

    #[inline]
    fn set_row_n<const N: usize>(&mut self, dy: isize, dx0: isize, val: &[u8; N]) {
        let r = self.row_mut(dy, dx0, N);
        r.copy_from_slice(val);
    }

    /// One [`span_mut`](PlaneCursorMut::span_mut) and `H` folded row writes; see
    /// [`PlaneSamples::set_block`].
    #[inline]
    fn set_block<const W: usize, const H: usize>(
        &mut self,
        dy0: isize,
        dx0: isize,
        rows: &[[u8; W]; H],
    ) {
        let mut span = self.span_mut::<W, H>(dy0, dx0);
        for (y, r) in rows.iter().enumerate() {
            *span.row_mut::<W>(y, 0) = *r;
        }
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
        // null.
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
        // Not even (0, 0): there is no byte to read, and the slice index says so.
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
        // 16 columns of left padding but 32 rows above: not a layout any picture has.
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
        // One row below the last padded row.
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

    /// `row_windows` yields exactly what the same block of `row` calls yields: it moves
    /// where the bounds checks land, never what is read.
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

    /// The last row of the block is `W` samples, not a whole stride, so the walker's final
    /// chunk is short and must still yield a full `W`-wide window. The block ends the
    /// allocation, where an over-long span would be caught.
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
