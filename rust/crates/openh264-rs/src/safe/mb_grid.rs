#![forbid(unsafe_code)]

//! Macroblock addressing — the geometry half of taxonomy class **T5**
//! (plan §1.2, contract §2.2.4).
//!
//! Today the decoder reaches the same per-MB arrays through two paths
//! (`pCtx->sMb.pXXX[0]` and `pCurDqLayer->pXXX` are one allocation:
//! `decoder_core.rs:3843-3869`, plan P2) and the encoder caches five pointers into
//! ctx-level flat arrays inside every `SMB`. Both die the same way: one owner, plain
//! indexing, `mb_idx` recomputed rather than cached.
//!
//! # Scope of this phase
//!
//! **Geometry only.** The field set of the real grid — `mb_type`, `mv`, `ref_index`,
//! `nzc`, `slice_idc`, `scaled_tcoeff`, … — belongs to Phases 5.2 and 6.3, which
//! know which of the `sMb`/`SDqLayer`/`SMB` fields survive. What can be built and
//! proven now is the addressing those phases will share: [`MbDims`] for the index
//! arithmetic and [`MbArray`] for one array over it.
//!
//! Neighbour *availability* is deliberately absent. `left()` here answers "is there a
//! macroblock to the left of this one **in the grid**", nothing more; the decoder's
//! real predicate also compares `pSliceIdc` values (`mv_pred.rs:485-510`), and that
//! is slice logic layered on top in Phase 5, not geometry.

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
/// The point is not the container — it is that `MbArray` has exactly one owner, so
/// reading `[xy - 1]` while writing `[xy]` is an ordinary borrow of one value rather
/// than two live pointers into one allocation.
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

    /// The entry at raster address `mb_xy`.
    #[inline]
    pub fn get(&self, mb_xy: usize) -> &T {
        &self.data[mb_xy]
    }

    /// Mutable form of [`get`](Self::get).
    #[inline]
    pub fn get_mut(&mut self, mb_xy: usize) -> &mut T {
        &mut self.data[mb_xy]
    }

    /// The entry at grid coordinates `(x, y)`.
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> &T {
        &self.data[self.dims.mb_xy(x, y)]
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
            // The pattern that is UB today: read left/top, write current.
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
}
