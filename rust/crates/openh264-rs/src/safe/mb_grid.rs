#![forbid(unsafe_code)]

//! Macroblock addressing.
//!
//! Neighbour *availability* is deliberately absent. `left()` here answers "is there a
//! macroblock to the left of this one **in the grid**", nothing more; the decoder's
//! real predicate also compares `pSliceIdc` values (`mv_pred.rs:485-510`), and that
//! is slice logic layered on top, not geometry.
//!
//! The grid carries the **allocation's** dimensions, not the current slice's:
//! it is sized once, at `InitialDqLayersContext`, from the negotiated
//! maximum, and a stream decoding below that maximum leaves `iMbWidth`/`iMbHeight`
//! smaller than [`MbGrid::dims`].

/// The macroblock grid's dimensions, and all index arithmetic over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MbDims {
    mb_width: usize,
    mb_height: usize,
}

impl MbDims {
    /// # Panics
    /// If either dimension is zero: there is no such picture.
    pub fn new(mb_width: usize, mb_height: usize) -> Self {
        assert!(mb_width > 0 && mb_height > 0, "empty macroblock grid");
        Self {
            mb_width,
            mb_height,
        }
    }

    /// The dimensions of a grid covering **no** macroblocks.
    ///
    /// It is deliberately *not* reachable through [`new`](Self::new), whose panic
    /// says there is no such picture — because for a grid that anything indexes,
    /// there is not.
    pub const fn none() -> Self {
        Self {
            mb_width: 0,
            mb_height: 0,
        }
    }

    /// The grid covering a picture of `width` × `height` pixels, rounding partial
    /// macroblocks up — `(kiPicWidth + 15) >> 4`, as `AllocPicture` does
    /// (`pic_queue.rs:267-269`).
    pub fn from_pixels(width: usize, height: usize) -> Self {
        Self::new((width + 15) >> 4, (height + 15) >> 4)
    }

    /// Macroblocks per row.
    #[inline]
    pub fn mb_width(&self) -> usize {
        self.mb_width
    }

    /// Macroblock rows.
    #[inline]
    pub fn mb_height(&self) -> usize {
        self.mb_height
    }

    /// Macroblocks in the picture — the C++ `uiMbCount`.
    #[inline]
    pub fn count(&self) -> usize {
        self.mb_width * self.mb_height
    }

    /// Raster address of the macroblock at `(x, y)` — the C++ `iMbXy`.
    ///
    /// # Panics
    /// If `(x, y)` is outside the grid.
    #[inline]
    pub fn mb_xy(&self, x: usize, y: usize) -> usize {
        assert!(
            x < self.mb_width && y < self.mb_height,
            "macroblock ({x}, {y}) outside a {}x{} grid",
            self.mb_width,
            self.mb_height
        );
        y * self.mb_width + x
    }

    /// Grid coordinates of raster address `mb_xy` — the inverse of
    /// [`mb_xy`](Self::mb_xy).
    ///
    /// # Panics
    /// If `mb_xy` is outside the grid.
    #[inline]
    pub fn xy_of(&self, mb_xy: usize) -> (usize, usize) {
        assert!(
            mb_xy < self.count(),
            "macroblock {mb_xy} outside a grid of {}",
            self.count()
        );
        (mb_xy % self.mb_width, mb_xy / self.mb_width)
    }

    /// The macroblock to the left, if the grid has one.
    ///
    /// Mirrors `if (iCurX != 0) iLeftXy = iCurXy - 1;` (`mv_pred.rs:486-487`).
    #[inline]
    pub fn left(&self, mb_xy: usize) -> Option<usize> {
        let (x, _) = self.xy_of(mb_xy);
        (x != 0).then(|| mb_xy - 1)
    }

    /// The macroblock above, if the grid has one.
    ///
    /// Mirrors `if (iCurY != 0) iTopXy = iCurXy - iMbWidth;` (`mv_pred.rs:495-496`).
    #[inline]
    pub fn top(&self, mb_xy: usize) -> Option<usize> {
        let (_, y) = self.xy_of(mb_xy);
        (y != 0).then(|| mb_xy - self.mb_width)
    }

    /// The macroblock above-left, if the grid has one.
    ///
    /// Mirrors the nested guard at `mv_pred.rs:499-500`: the C++ only computes
    /// `iLeftTopXy = iTopXy - 1` inside `if (iCurY != 0)`, so both edges matter.
    #[inline]
    pub fn top_left(&self, mb_xy: usize) -> Option<usize> {
        let (x, y) = self.xy_of(mb_xy);
        (x != 0 && y != 0).then(|| mb_xy - self.mb_width - 1)
    }

    /// The macroblock above-right, if the grid has one.
    ///
    /// Mirrors `if (iCurX != iMbWidth - 1) iRightTopXy = iTopXy + 1;`
    /// (`mv_pred.rs:506-507`), likewise nested inside the `iCurY != 0` guard.
    #[inline]
    pub fn top_right(&self, mb_xy: usize) -> Option<usize> {
        let (x, y) = self.xy_of(mb_xy);
        (y != 0 && x + 1 != self.mb_width).then(|| mb_xy - self.mb_width + 1)
    }
}

/// One per-macroblock array, owned by the grid it belongs to.
///
/// `MbArray` has exactly one owner, so reading `[xy - 1]` while writing `[xy]`
/// is an ordinary borrow of one value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MbArray<T> {
    data: Vec<T>,
    dims: MbDims,
}

impl<T: Clone> MbArray<T> {
    /// An array of `dims.count()` copies of `value`.
    pub fn new(dims: MbDims, value: T) -> Self {
        Self {
            data: vec![value; dims.count()],
            dims,
        }
    }
}

impl<T> MbArray<T> {
    /// The array of a picture that covers no macroblocks — see [`MbDims::none`].
    pub const fn empty() -> Self {
        Self {
            data: Vec::new(),
            dims: MbDims::none(),
        }
    }

    /// The array's root as a raw pointer.
    ///
    /// Encoder callers take `*mut SMB` and walk *backwards* out of the
    /// macroblock they are handed (`pCurMb.offset(-1)` for the left
    /// neighbour, `.offset(-iMbStride)` for the one above), so the pointer they get
    /// must carry the whole array's provenance. This returns the `Vec`'s own stored
    /// pointer, which does exactly that; a pointer taken through
    /// `as_mut_slice()[xy..]` would have the right address and provenance for the
    /// tail alone.
    ///
    /// Repeated calls are safe to interleave: `Vec::as_mut_ptr` reads the pointer the
    /// allocation already holds rather than reborrowing the buffer, so a second call
    /// does not invalidate the first one's result.
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.data.as_mut_ptr()
    }

    /// The same root, reached through `&self`.
    ///
    /// `as_mut_ptr` above is sound for one thread and unsound for two: `&mut self`
    /// is a `Unique` retag over the array's own three words, and every encoder
    /// worker asks the same layer for the same macroblock array, so two of them
    /// retagging it at once is a data race even though neither writes *it*. This
    /// reads the buffer pointer out instead. The pointer is identical and carries
    /// the buffer's own provenance, so the macroblocks behind it stay writable —
    /// only the access to the array struct narrows, from exclusive to shared.
    pub fn root_ptr(&self) -> *mut T {
        self.data.as_ptr() as *mut T
    }

    /// Adopts `data` as the per-macroblock array of `dims`.
    ///
    /// # Panics
    /// If `data.len() != dims.count()`.
    pub fn from_vec(data: Vec<T>, dims: MbDims) -> Self {
        assert_eq!(
            data.len(),
            dims.count(),
            "per-macroblock array of {} for a grid of {}",
            data.len(),
            dims.count()
        );
        Self { data, dims }
    }

    /// The grid this array is addressed by.
    #[inline]
    pub fn dims(&self) -> MbDims {
        self.dims
    }

    /// The whole array in raster order.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.data
    }

    /// Mutable form of [`as_slice`](Self::as_slice).
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// `#[track_caller]` moves the location to the reader, and the message carries
    /// both grid dimensions, which is what turns "off by one" into "this grid is
    /// 22x18 and the caller thinks it is bigger".
    #[inline]
    #[track_caller]
    fn check(&self, mb_xy: usize) {
        assert!(
            mb_xy < self.data.len(),
            "mb_xy {} >= {} (grid {}x{})",
            mb_xy,
            self.data.len(),
            self.dims.mb_width(),
            self.dims.mb_height()
        );
    }

    /// The entry at raster address `mb_xy`.
    #[inline]
    #[track_caller]
    pub fn get(&self, mb_xy: usize) -> &T {
        self.check(mb_xy);
        &self.data[mb_xy]
    }

    /// Mutable form of [`get`](Self::get).
    #[inline]
    #[track_caller]
    pub fn get_mut(&mut self, mb_xy: usize) -> &mut T {
        self.check(mb_xy);
        &mut self.data[mb_xy]
    }

    /// The entry at grid coordinates `(x, y)`.
    #[inline]
    #[track_caller]
    pub fn at(&self, x: usize, y: usize) -> &T {
        let mb_xy = self.dims.mb_xy(x, y);
        self.check(mb_xy);
        &self.data[mb_xy]
    }

    /// The left neighbour's entry, if the grid has one.
    #[inline]
    pub fn left(&self, mb_xy: usize) -> Option<&T> {
        self.dims.left(mb_xy).map(|xy| &self.data[xy])
    }

    /// The above neighbour's entry, if the grid has one.
    #[inline]
    pub fn top(&self, mb_xy: usize) -> Option<&T> {
        self.dims.top(mb_xy).map(|xy| &self.data[xy])
    }

    /// The above-left neighbour's entry, if the grid has one.
    #[inline]
    pub fn top_left(&self, mb_xy: usize) -> Option<&T> {
        self.dims.top_left(mb_xy).map(|xy| &self.data[xy])
    }

    /// The above-right neighbour's entry, if the grid has one.
    #[inline]
    pub fn top_right(&self, mb_xy: usize) -> Option<&T> {
        self.dims.top_right(mb_xy).map(|xy| &self.data[xy])
    }
}

/// A borrowed window of per-macroblock records — sibling to [`MbArray`] the way
/// a slice is sibling to a `Vec`.
///
/// It is generic so this module keeps depending on nothing; the encoder
/// instantiates it at its `SMB`.
///
/// Under the multi-threaded fork each worker's record accesses are confined to
/// the records it owns — its slice's range (fixed slicing, and the mode-decision
/// and entropy paths generally, whose neighbour reads are guarded by the
/// slice-scoped `uiNeighborAvail` flags) or its partition's (dynamic slicing);
/// deblocking's in-fork guards ask the atomic `pOverallMbMap` the same-slice
/// question. A window is minted per call over exactly the owned
/// range, so disjointness is enforced mechanically: an access that would leave
/// the window — a cross-slice record read under the fork — **panics with
/// coordinates instead of racing**. Single-threaded callers mint the whole grid
/// and the same accessors cross slice boundaries freely (the frame deblocking
/// walk under filter idc 0).
///
/// Every accessor is `#[track_caller]` and its message names the question, the
/// current index, the window and the stride, so a wrong availability flag or a
/// wrong window range says *which* neighbour question failed and where — not
/// "index out of bounds" three frames down.
pub struct MbWindow<'a, T> {
    /// Records `[base .. base + mbs.len())` of one grid, in raster order.
    mbs: &'a mut [T],
    /// Raster address of `mbs[0]`.
    base: usize,
    /// Macroblocks per row — the grid's `iMbWidth`, shared by every window of it.
    stride: usize,
    /// Raster address of the current macroblock.
    cur: usize,
}

impl<'a, T> MbWindow<'a, T> {
    /// A window over `mbs`, which are records `base..` of a grid `stride` wide,
    /// with the current macroblock at raster address `cur`.
    ///
    /// # Panics
    /// If `stride` is zero, or `cur` is outside the window.
    #[track_caller]
    pub fn new(mbs: &'a mut [T], base: usize, stride: usize, cur: usize) -> Self {
        assert!(stride > 0, "macroblock window over a zero-width grid");
        assert!(
            cur >= base && cur - base < mbs.len(),
            "current mb {cur} outside window [{base}..{}) (stride {stride})",
            base + mbs.len()
        );
        Self { mbs, base, stride, cur }
    }

    /// The whole of `arr` as one window, current macroblock at `cur` — the
    /// single-threaded mint, where one owner holds the grid and every neighbour
    /// on it is legal.
    #[track_caller]
    pub fn whole(arr: &'a mut MbArray<T>, cur: usize) -> Self {
        let stride = arr.dims().mb_width();
        Self::new(arr.as_mut_slice(), 0, stride, cur)
    }

    /// Raster address of the current macroblock.
    #[inline]
    pub fn cur_xy(&self) -> usize {
        self.cur
    }

    /// Macroblocks per row.
    #[inline]
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Move the current macroblock to raster address `cur`.
    ///
    /// # Panics
    /// If `cur` is outside the window.
    #[inline]
    #[track_caller]
    pub fn set_cur(&mut self, cur: usize) {
        assert!(
            cur >= self.base && cur - self.base < self.mbs.len(),
            "current mb {cur} outside window [{}..{}) (stride {})",
            self.base,
            self.base + self.mbs.len(),
            self.stride
        );
        self.cur = cur;
    }

    /// `xy` relative to the window.
    #[inline]
    #[track_caller]
    fn rel(&self, xy: usize, what: &'static str) -> usize {
        assert!(
            xy >= self.base && xy - self.base < self.mbs.len(),
            "{what} of mb {} is mb {xy}, outside window [{}..{}) (stride {})",
            self.cur,
            self.base,
            self.base + self.mbs.len(),
            self.stride
        );
        xy - self.base
    }

    /// The current macroblock's record.
    #[inline]
    pub fn cur(&self) -> &T {
        &self.mbs[self.cur - self.base]
    }

    /// Mutable form of [`cur`](Self::cur).
    #[inline]
    pub fn cur_mut(&mut self) -> &mut T {
        &mut self.mbs[self.cur - self.base]
    }

    /// The record at raster address `xy` — for the list walkers; neighbour
    /// questions read better through the named accessors below.
    #[inline]
    #[track_caller]
    pub fn at(&self, xy: usize) -> &T {
        let i = self.rel(xy, "record");
        &self.mbs[i]
    }

    /// Mutable form of [`at`](Self::at).
    #[inline]
    #[track_caller]
    pub fn at_mut(&mut self, xy: usize) -> &mut T {
        let i = self.rel(xy, "record");
        &mut self.mbs[i]
    }

    /// The left neighbour's record. The caller has already checked its
    /// availability flag — a panic here means the flag and the geometry
    /// disagree, which is exactly what it should say.
    ///
    /// # Panics
    /// At the left grid edge, or if `cur - 1` is outside the window.
    #[inline]
    #[track_caller]
    pub fn left(&self) -> &T {
        assert!(
            self.cur % self.stride != 0,
            "left of mb {} at the left grid edge (stride {})",
            self.cur,
            self.stride
        );
        let i = self.rel(self.cur - 1, "left");
        &self.mbs[i]
    }

    /// The record above. See [`left`](Self::left) for the contract.
    ///
    /// # Panics
    /// In the top grid row, or if `cur - stride` is outside the window.
    #[inline]
    #[track_caller]
    pub fn top(&self) -> &T {
        assert!(
            self.cur >= self.stride,
            "top of mb {} in the top grid row (stride {})",
            self.cur,
            self.stride
        );
        let i = self.rel(self.cur - self.stride, "top");
        &self.mbs[i]
    }

    /// The record above-left. See [`left`](Self::left) for the contract.
    #[inline]
    #[track_caller]
    pub fn top_left(&self) -> &T {
        assert!(
            self.cur >= self.stride && self.cur % self.stride != 0,
            "top-left of mb {} at a grid edge (stride {})",
            self.cur,
            self.stride
        );
        let i = self.rel(self.cur - self.stride - 1, "top-left");
        &self.mbs[i]
    }

    /// The record above-right. See [`left`](Self::left) for the contract.
    #[inline]
    #[track_caller]
    pub fn top_right(&self) -> &T {
        assert!(
            self.cur >= self.stride && (self.cur + 1) % self.stride != 0,
            "top-right of mb {} at a grid edge (stride {})",
            self.cur,
            self.stride
        );
        let i = self.rel(self.cur - self.stride + 1, "top-right");
        &self.mbs[i]
    }

    /// The record before the current one in **coding order** — the CABAC
    /// delta-QP question ("the previously coded macroblock"), which is raster
    /// order within a slice and *not* a geometric neighbour: at a row start it
    /// names the previous row's last record, where [`left`](Self::left) would
    /// rightly panic.
    ///
    /// # Panics
    /// If `cur - 1` is outside the window (the caller has already checked
    /// "not the first macroblock of the slice").
    #[inline]
    #[track_caller]
    pub fn prev(&self) -> &T {
        let i = self.rel(self.cur.wrapping_sub(1), "prev-in-coding-order");
        &self.mbs[i]
    }

    /// The window split at its cursor: exclusive access to the current record,
    /// shared access to the two raster predecessors the filter's boundary-strength
    /// and QP guards may read.
    ///
    /// The filter writes exactly one record field (`DeblockingBSCalc_c`
    /// normalises the current macroblock's `iNonZeroCount`) and reads exactly
    /// two neighbours (`cur - 1` and `cur - stride`, both strictly earlier in
    /// raster order).
    ///
    /// `left`/`top` are absent (`None` behind the accessors) at a grid edge or
    /// where the neighbour's record lies outside this window. Calling the
    /// accessor anyway panics — a validated edge whose record the window cannot
    /// name is a bug that should say so, not an index error three frames down.
    #[inline]
    #[track_caller]
    pub fn split_cur(&mut self) -> MbSplit<'_, T> {
        let local = self.cur - self.base;
        let (done, rest) = self.mbs.split_at_mut(local);
        MbSplit {
            cur: &mut rest[0],
            left: (self.cur % self.stride != 0 && self.cur > self.base)
                .then(|| &done[local - 1]),
            top: (self.cur >= self.stride && self.cur >= self.base + self.stride)
                .then(|| &done[local - self.stride]),
        }
    }
}

/// One record held exclusively beside shared references to its raster
/// predecessors — what [`MbWindow::split_cur`] answers.
///
/// The fields are private so the split can only be built by a window that
/// actually contains the records; the accessors mirror [`MbWindow`]'s so the
/// per-macroblock bodies read identically through either.
pub struct MbSplit<'a, T> {
    cur: &'a mut T,
    left: Option<&'a T>,
    top: Option<&'a T>,
}

impl<'a, T> MbSplit<'a, T> {
    /// The current macroblock's record.
    #[inline]
    pub fn cur(&self) -> &T {
        self.cur
    }

    /// Mutable form of [`cur`](Self::cur) — the one record deblocking writes.
    #[inline]
    pub fn cur_mut(&mut self) -> &mut T {
        self.cur
    }

    /// The left neighbour's record. The caller has already checked its
    /// availability flag — a panic here means the flag and the window's
    /// geometry disagree, which is exactly what it should say.
    #[inline]
    #[track_caller]
    pub fn left(&self) -> &T {
        self.left
            .expect("left neighbour read with no record in the window — the availability flag and the geometry disagree (F77)")
    }

    /// The record above. See [`left`](Self::left) for the contract.
    #[inline]
    #[track_caller]
    pub fn top(&self) -> &T {
        self.top
            .expect("top neighbour read with no record in the window — the availability flag and the geometry disagree (F77)")
    }
}

/// Reference lists per macroblock — the decoder's `LIST_A`.
///
/// Declared here rather than imported so this module keeps depending on nothing.
/// `decoder_core.rs`'s `mb_grid_list_count_matches_list_a` is where the identity
/// against `decoder_context::LIST_A` is actually checked, because that is the one
/// place both names are in scope.
pub const LIST_COUNT: usize = 2;

/// Every per-macroblock array the decoder's DQ layer owns, over one grid.
///
/// One owner, one set of dimensions, and indexing that panics instead of running
/// off the end.
///
/// # Field order
///
/// The order below is `InitialDqLayersContext`'s allocation order, not
/// `DqLayerState`'s declaration order.
#[derive(Clone, Debug)]
pub struct MbGrid {
    dims: MbDims,
    /// `pMbType` — `numMb * sizeof(u32)`.
    pub mb_type: MbArray<u32>,
    /// `pMv[LIST_A]` — `numMb * 16 * 2 * sizeof(i16)`, one 4x4-block motion vector
    /// field per list.
    pub mv: [MbArray<[[i16; 2]; 16]>; LIST_COUNT],
    /// `pRefIndex[LIST_A]` — `numMb * 16 * sizeof(i8)`.
    pub ref_index: [MbArray<[i8; 16]>; LIST_COUNT],
    /// `pDirect` — `numMb * 16 * sizeof(i8)`.
    pub direct: MbArray<[i8; 16]>,
    /// `pNoSubMbPartSizeLessThan8x8Flag` — `numMb * sizeof(bool)`.
    pub no_sub_mb_part_size_less_than8x8_flag: MbArray<bool>,
    /// `pTransformSize8x8Flag` — `numMb * sizeof(bool)`.
    pub transform_size8x8_flag: MbArray<bool>,
    /// `pLumaQp` — `numMb * sizeof(i8)`.
    pub luma_qp: MbArray<i8>,
    /// `pChromaQp` — `numMb * 2 * sizeof(i8)`, Cb then Cr.
    pub chroma_qp: MbArray<[i8; 2]>,
    /// `pMvd[LIST_A]` — `numMb * 16 * 2 * sizeof(i16)`.
    pub mvd: [MbArray<[[i16; 2]; 16]>; LIST_COUNT],
    /// `pCbfDc` — `numMb * sizeof(u16)`.
    pub cbf_dc: MbArray<u16>,
    /// `pNzc` — `numMb * 24 * sizeof(i8)`: 16 luma 4x4 blocks then 8 chroma.
    pub nzc: MbArray<[i8; 24]>,
    /// `pScaledTCoeff` — `numMb * MB_COEFF_LIST_SIZE * sizeof(i16)`, 384 = 256 luma
    /// + 2 x 64 chroma.
    pub scaled_tcoeff: MbArray<[i16; 384]>,
    /// `pIntraPredMode` — `numMb * sizeof([i8; 8])`. `dec_frame.h:85`:
    /// `0~3 top4x4; 4~6 left 4x4; 7 intra16x16`.
    pub intra_pred_mode: MbArray<[i8; 8]>,
    /// `pIntra4x4FinalMode` — `numMb * sizeof([i8; 16])`, indexed in scan order.
    pub intra4x4_final_mode: MbArray<[i8; 16]>,
    /// `pIntraNxNAvailFlag` — `numMb * sizeof(u8)`.
    pub intra_nxn_avail_flag: MbArray<u8>,
    /// `pChromaPredMode` — `numMb * sizeof(i8)`.
    pub chroma_pred_mode: MbArray<i8>,
    /// `pCbp` — `numMb * sizeof(i8)`.
    pub cbp: MbArray<i8>,
    /// `pSubMbType` — `numMb * MB_PARTITION_SIZE * sizeof(u32)`.
    pub sub_mb_type: MbArray<[u32; 4]>,
    /// `pSliceIdc` — `numMb * sizeof(i32)`. The slice a macroblock belongs to;
    /// `0xff`-memset at slice start, which is why it is `i32` and not an index.
    pub slice_idc: MbArray<i32>,
    /// `pResidualPredFlag` — `numMb * sizeof(i8)`.
    pub residual_pred_flag: MbArray<i8>,
    /// `pMbCorrectlyDecodedFlag` — `numMb * sizeof(bool)`.
    pub mb_correctly_decoded_flag: MbArray<bool>,
    /// `pMbRefConcealedFlag` — `numMb * sizeof(bool)`.
    pub mb_ref_concealed_flag: MbArray<bool>,
}

impl MbGrid {
    /// A grid over `dims` with every array zero-filled.
    ///
    /// Zero-filled because `WelsMallocz` is what allocated all 25 of these blocks
    /// and it zeroes.
    pub fn new(dims: MbDims) -> Self {
        Self {
            dims,
            mb_type: MbArray::new(dims, 0),
            mv: [
                MbArray::new(dims, [[0; 2]; 16]),
                MbArray::new(dims, [[0; 2]; 16]),
            ],
            ref_index: [MbArray::new(dims, [0; 16]), MbArray::new(dims, [0; 16])],
            direct: MbArray::new(dims, [0; 16]),
            no_sub_mb_part_size_less_than8x8_flag: MbArray::new(dims, false),
            transform_size8x8_flag: MbArray::new(dims, false),
            luma_qp: MbArray::new(dims, 0),
            chroma_qp: MbArray::new(dims, [0; 2]),
            mvd: [
                MbArray::new(dims, [[0; 2]; 16]),
                MbArray::new(dims, [[0; 2]; 16]),
            ],
            cbf_dc: MbArray::new(dims, 0),
            nzc: MbArray::new(dims, [0; 24]),
            scaled_tcoeff: MbArray::new(dims, [0; 384]),
            intra_pred_mode: MbArray::new(dims, [0; 8]),
            intra4x4_final_mode: MbArray::new(dims, [0; 16]),
            intra_nxn_avail_flag: MbArray::new(dims, 0),
            chroma_pred_mode: MbArray::new(dims, 0),
            cbp: MbArray::new(dims, 0),
            sub_mb_type: MbArray::new(dims, [0; 4]),
            slice_idc: MbArray::new(dims, 0),
            residual_pred_flag: MbArray::new(dims, 0),
            mb_correctly_decoded_flag: MbArray::new(dims, false),
            mb_ref_concealed_flag: MbArray::new(dims, false),
        }
    }

    /// The dimensions every array in this grid is addressed by — the **allocation's**,
    /// fixed at construction.
    #[inline]
    pub fn dims(&self) -> MbDims {
        self.dims
    }
}

/// The message names the index, the length **and** both
/// grid dimensions, so a stale-allocation read says which grid it was reading.
#[cfg(test)]
mod f77_instrument_tests {
    use super::*;

    #[test]
    #[should_panic(expected = "grid ")]
    fn an_out_of_range_read_names_the_grid() {
        let grid: MbArray<u8> = MbArray::new(MbDims::new(22, 18), 0u8);
        let _ = grid.get(22 * 18);
    }

    #[test]
    fn the_message_carries_the_index_the_length_and_both_dimensions() {
        let grid: MbArray<u8> = MbArray::new(MbDims::new(22, 18), 0u8);
        let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = grid.get(400);
        }))
        .unwrap_err();
        let text = msg
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| String::from("<not a String>"));
        assert_eq!(text, "mb_xy 400 >= 396 (grid 22x18)");
    }
}

#[cfg(test)]
mod mb_window_tests {
    use super::*;

    /// A 4x3 grid whose records are their own raster addresses.
    fn grid() -> MbArray<usize> {
        let dims = MbDims::new(4, 3);
        MbArray::from_vec((0..dims.count()).collect(), dims)
    }

    #[test]
    fn a_whole_window_answers_every_neighbour_the_grid_has() {
        let mut a = grid();
        let mut w = MbWindow::whole(&mut a, 6); // row 1, col 2: all four present
        assert_eq!((*w.cur(), *w.left(), *w.top(), *w.top_left(), *w.top_right()), (6, 5, 2, 1, 3));
        w.set_cur(9);
        assert_eq!((*w.cur(), *w.top_right()), (9, 6));
        *w.cur_mut() = 99;
        assert_eq!(*w.at(9), 99);
        *w.at_mut(9) = 9;
        assert_eq!(*w.cur(), 9);
    }

    /// The split hands out the current record exclusively and its two
    /// raster predecessors shared, from one borrow — deblocking's footprint.
    #[test]
    fn split_cur_pairs_the_exclusive_record_with_its_shared_predecessors() {
        let mut a = grid();
        let mut w = MbWindow::whole(&mut a, 6); // row 1, col 2
        {
            let mut s = w.split_cur();
            assert_eq!((*s.cur(), *s.left(), *s.top()), (6, 5, 2));
            *s.cur_mut() = 66; // the one write deblocking makes
        }
        assert_eq!(*w.at(6), 66, "the split's write landed in the window");

        w.set_cur(0); // top-left corner: no neighbour exists
        let s = w.split_cur();
        assert_eq!(*s.cur(), 0);

        w.set_cur(4); // row start: left is off the grid row, top present
        let s = w.split_cur();
        assert_eq!(*s.top(), 0);
    }

    /// A sub-window's split refuses a neighbour whose record it cannot name —
    /// from the accessor rather than an index error.
    #[test]
    #[should_panic(expected = "availability flag and the geometry disagree")]
    fn a_split_neighbour_outside_the_window_panics_with_f77s_message() {
        let mut a = grid();
        // Records [5..9) of the 4-wide grid; cur = 6 — its top (2) is outside.
        let mut w = MbWindow::new(&mut a.as_mut_slice()[5..9], 5, 4, 6);
        let s = w.split_cur();
        assert_eq!(*s.left(), 5, "the in-window neighbour still answers");
        let _ = s.top();
    }

    /// The encoder's sub-range mint: a slice's records only, base > 0.
    #[test]
    fn a_sub_window_is_addressed_by_raster_indices_not_window_offsets() {
        let mut a = grid();
        let base = 5;
        let w = MbWindow::new(&mut a.as_mut_slice()[base..9], base, 4, 6);
        assert_eq!((*w.cur(), *w.left()), (6, 5));
        assert_eq!(*w.at(8), 8);
    }

    /// Delta-QP's "previously coded" record at a row start is the previous
    /// row's *last* record — `prev` answers it and `left` refuses it.
    #[test]
    fn prev_is_coding_order_not_geometry() {
        let mut a = grid();
        let w = MbWindow::whole(&mut a, 4); // row 1, col 0
        assert_eq!(*w.prev(), 3);
    }

    #[test]
    #[should_panic(expected = "left of mb 4 at the left grid edge (stride 4)")]
    fn left_at_the_grid_edge_names_the_question() {
        let mut a = grid();
        let _ = MbWindow::whole(&mut a, 4).left();
    }

    #[test]
    #[should_panic(expected = "top of mb 2 in the top grid row (stride 4)")]
    fn top_in_the_top_row_names_the_question() {
        let mut a = grid();
        let _ = MbWindow::whole(&mut a, 2).top();
    }

    /// The fork-safety tooth: a same-slice-guarded walker that steps outside
    /// its own window aborts with the window's coordinates, instead of reading
    /// another worker's record.
    #[test]
    #[should_panic(expected = "top of mb 6 is mb 2, outside window [5..9) (stride 4)")]
    fn an_out_of_window_neighbour_read_names_window_and_question() {
        let mut a = grid();
        let w = MbWindow::new(&mut a.as_mut_slice()[5..9], 5, 4, 6);
        let _ = w.top();
    }

    #[test]
    #[should_panic(expected = "current mb 9 outside window [5..9) (stride 4)")]
    fn set_cur_refuses_an_index_outside_the_window() {
        let mut a = grid();
        MbWindow::new(&mut a.as_mut_slice()[5..9], 5, 4, 6).set_cur(9);
    }

    #[test]
    #[should_panic(expected = "top-right of mb 7 at a grid edge (stride 4)")]
    fn top_right_at_the_right_edge_names_the_question() {
        let mut a = grid();
        let _ = MbWindow::whole(&mut a, 7).top_right();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::prng::Prng;

    #[test]
    fn from_pixels_rounds_partial_macroblocks_up() {
        assert_eq!(MbDims::from_pixels(176, 144), MbDims::new(11, 9));
        assert_eq!(MbDims::from_pixels(1920, 1080), MbDims::new(120, 68));
        assert_eq!(MbDims::from_pixels(1, 1), MbDims::new(1, 1));
        assert_eq!(MbDims::from_pixels(1920, 1080).count(), 8160);
    }

    #[test]
    fn mb_xy_and_xy_of_are_inverses() {
        let d = MbDims::new(11, 9);
        for y in 0..9 {
            for x in 0..11 {
                let xy = d.mb_xy(x, y);
                assert_eq!(d.xy_of(xy), (x, y));
            }
        }
        assert_eq!(d.mb_xy(0, 0), 0);
        assert_eq!(d.mb_xy(10, 8), 98);
        assert_eq!(d.count(), 99);
    }

    #[test]
    fn corner_macroblocks_have_the_neighbours_the_c_computes() {
        let d = MbDims::new(4, 3);
        // top-left corner: nothing above or left
        assert_eq!(d.left(0), None);
        assert_eq!(d.top(0), None);
        assert_eq!(d.top_left(0), None);
        assert_eq!(d.top_right(0), None, "top row has no row above at all");
        // top-right corner
        assert_eq!(d.left(3), Some(2));
        assert_eq!(d.top(3), None);
        // bottom-left corner
        assert_eq!(d.left(8), None);
        assert_eq!(d.top(8), Some(4));
        assert_eq!(d.top_left(8), None);
        assert_eq!(d.top_right(8), Some(5));
        // bottom-right corner: no macroblock above-right
        assert_eq!(d.left(11), Some(10));
        assert_eq!(d.top(11), Some(7));
        assert_eq!(d.top_left(11), Some(6));
        assert_eq!(d.top_right(11), None);
        // interior
        assert_eq!(d.left(5), Some(4));
        assert_eq!(d.top(5), Some(1));
        assert_eq!(d.top_left(5), Some(0));
        assert_eq!(d.top_right(5), Some(2));
    }

    #[test]
    fn degenerate_grids_behave() {
        let row = MbDims::new(5, 1);
        for xy in 0..5 {
            assert_eq!(row.top(xy), None);
            assert_eq!(row.top_left(xy), None);
            assert_eq!(row.top_right(xy), None);
        }
        assert_eq!(row.left(0), None);
        assert_eq!(row.left(4), Some(3));

        let col = MbDims::new(1, 5);
        for xy in 0..5 {
            assert_eq!(col.left(xy), None);
            assert_eq!(col.top_left(xy), None);
            assert_eq!(col.top_right(xy), None, "single column has no right");
        }
        assert_eq!(col.top(0), None);
        assert_eq!(col.top(4), Some(3));

        let one = MbDims::new(1, 1);
        assert_eq!((one.left(0), one.top(0), one.top_left(0), one.top_right(0)), (None, None, None, None));
    }

    #[test]
    fn neighbours_match_hand_computed_coordinates_over_random_grids() {
        let mut rng = Prng::new(0xB1A5_1234);
        for _ in 0..64 {
            let (w, h) = (rng.below(9) as usize + 1, rng.below(9) as usize + 1);
            let d = MbDims::new(w, h);
            for xy in 0..d.count() {
                let (x, y) = (xy % w, xy / w);
                let want = |cond: bool, v: isize| cond.then(|| v as usize);
                assert_eq!(d.left(xy), want(x > 0, xy as isize - 1), "left of {xy} in {w}x{h}");
                assert_eq!(
                    d.top(xy),
                    want(y > 0, xy as isize - w as isize),
                    "top of {xy} in {w}x{h}"
                );
                assert_eq!(
                    d.top_left(xy),
                    want(x > 0 && y > 0, xy as isize - w as isize - 1),
                    "top_left of {xy} in {w}x{h}"
                );
                assert_eq!(
                    d.top_right(xy),
                    want(y > 0 && x + 1 < w, xy as isize - w as isize + 1),
                    "top_right of {xy} in {w}x{h}"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "outside a 4x3 grid")]
    fn mb_xy_rejects_a_coordinate_off_the_grid() {
        MbDims::new(4, 3).mb_xy(4, 0);
    }

    #[test]
    #[should_panic(expected = "empty macroblock grid")]
    fn a_zero_dimension_grid_is_rejected() {
        MbDims::new(0, 3);
    }

    #[test]
    fn array_reads_a_neighbour_while_writing_the_current_macroblock() {
        let dims = MbDims::new(4, 3);
        let mut a = MbArray::new(dims, 0u32);
        for xy in 0..dims.count() {
            // Read left/top, write current.
            let left = a.left(xy).copied().unwrap_or(0);
            let top = a.top(xy).copied().unwrap_or(0);
            *a.get_mut(xy) = left + top + 1;
        }
        assert_eq!(a.as_slice()[0], 1);
        assert_eq!(*a.at(1, 0), 2);
        assert_eq!(*a.at(0, 1), 2);
        assert_eq!(*a.at(1, 1), 2 + 2 + 1);
    }

    #[test]
    fn from_vec_checks_the_length() {
        let dims = MbDims::new(2, 2);
        let a = MbArray::from_vec(vec![1u8, 2, 3, 4], dims);
        assert_eq!(a.as_slice(), &[1, 2, 3, 4]);
        assert_eq!(a.dims(), dims);
    }

    #[test]
    #[should_panic(expected = "for a grid of 4")]
    fn from_vec_rejects_a_mismatched_length() {
        MbArray::from_vec(vec![1u8, 2, 3], MbDims::new(2, 2));
    }

    // -----------------------------------------------------------------------
    // MbGrid
    // -----------------------------------------------------------------------

    /// Every array is sized by the grid's dimensions, and every array agrees.
    #[test]
    fn every_array_is_sized_by_the_grids_dimensions() {
        let dims = MbDims::new(11, 9);
        let g = MbGrid::new(dims);
        assert_eq!(g.dims(), dims);
        let n = dims.count();
        assert_eq!(g.mb_type.as_slice().len(), n);
        assert_eq!(g.slice_idc.as_slice().len(), n);
        assert_eq!(g.direct.as_slice().len(), n);
        assert_eq!(g.no_sub_mb_part_size_less_than8x8_flag.as_slice().len(), n);
        assert_eq!(g.transform_size8x8_flag.as_slice().len(), n);
        assert_eq!(g.luma_qp.as_slice().len(), n);
        assert_eq!(g.chroma_qp.as_slice().len(), n);
        assert_eq!(g.cbf_dc.as_slice().len(), n);
        assert_eq!(g.nzc.as_slice().len(), n);
        assert_eq!(g.scaled_tcoeff.as_slice().len(), n);
        assert_eq!(g.intra_pred_mode.as_slice().len(), n);
        assert_eq!(g.intra4x4_final_mode.as_slice().len(), n);
        assert_eq!(g.intra_nxn_avail_flag.as_slice().len(), n);
        assert_eq!(g.chroma_pred_mode.as_slice().len(), n);
        assert_eq!(g.cbp.as_slice().len(), n);
        assert_eq!(g.sub_mb_type.as_slice().len(), n);
        assert_eq!(g.residual_pred_flag.as_slice().len(), n);
        assert_eq!(g.mb_correctly_decoded_flag.as_slice().len(), n);
        assert_eq!(g.mb_ref_concealed_flag.as_slice().len(), n);
        for l in 0..LIST_COUNT {
            assert_eq!(g.mv[l].as_slice().len(), n);
            assert_eq!(g.mvd[l].as_slice().len(), n);
            assert_eq!(g.ref_index[l].as_slice().len(), n);
            assert_eq!(g.mv[l].dims(), dims);
        }
    }

    /// `WelsMallocz` zeroes, so a fresh grid is what a fresh sequence has always
    /// started from. Checked on the widest element and the narrowest.
    #[test]
    fn a_fresh_grid_is_zero_everywhere() {
        let g = MbGrid::new(MbDims::new(3, 2));
        assert!(g.scaled_tcoeff.as_slice().iter().all(|mb| mb.iter().all(|&c| c == 0)));
        assert!(g.mv[0].as_slice().iter().all(|mb| mb.iter().all(|v| v == &[0, 0])));
        assert!(g.mb_correctly_decoded_flag.as_slice().iter().all(|&f| !f));
        assert!(g.luma_qp.as_slice().iter().all(|&q| q == 0));
        assert_eq!(g.intra_pred_mode.get(5), &[0i8; 8]);
    }

    /// `[i8; 8]` and `[i8; 16]` are the sizes `dec_frame.h:85-86` states, and the
    /// grid's storage is `count() * 8` and `count() * 16` bytes rather than
    /// `count()`.
    #[test]
    fn the_two_arrays_f32_corrected_carry_their_element_counts() {
        let dims = MbDims::new(4, 4);
        let mut g = MbGrid::new(dims);
        assert_eq!(std::mem::size_of_val(g.intra_pred_mode.as_slice()), dims.count() * 8);
        assert_eq!(std::mem::size_of_val(g.intra4x4_final_mode.as_slice()), dims.count() * 16);
        // slot 7 is the I16x16 mode, slots 0..4 the next macroblock's left cache
        g.intra_pred_mode.get_mut(dims.count() - 1)[7] = 3;
        assert_eq!(g.intra_pred_mode.get(dims.count() - 1)[7], 3);
    }

    /// Two arrays of one grid are two values, so writing one while reading another
    /// is an ordinary pair of borrows.
    #[test]
    fn two_arrays_of_one_grid_borrow_independently() {
        let dims = MbDims::new(4, 3);
        let mut g = MbGrid::new(dims);
        for xy in 0..dims.count() {
            g.cbp.as_mut_slice()[xy] = (xy % 7) as i8;
        }
        for xy in 0..dims.count() {
            let cbp = *g.cbp.get(xy);
            let left = g.luma_qp.left(xy).copied().unwrap_or(26);
            *g.luma_qp.get_mut(xy) = left + cbp;
        }
        assert_eq!(*g.luma_qp.get(0), 26);
        assert_eq!(*g.luma_qp.get(1), 26 + 1);
    }

    #[test]
    #[should_panic(expected = "outside a 4x3 grid")]
    fn a_grid_array_rejects_a_coordinate_off_the_grid() {
        MbGrid::new(MbDims::new(4, 3)).nzc.at(4, 0);
    }
}
