#![forbid(unsafe_code)]

//! The MVD-cost cursor — [`MvdCostCursor`], the safe stand-in for the encoder's
//! `uint16_t*` into `pCtx->pMvdCostTable`.
//!
//! # Not a plain slice
//!
//! `COST_MVD` indexes the table with a *signed* motion-vector difference:
//! `p[iMvdX] + p[iMvdY]`, either operand of either sign, so the pointer is parked in the
//! middle of a table row and the reads run in both directions from it. A `&[u16]` cannot
//! stand in for that — its index 0 is its first element — but the pair can: the table,
//! plus the index of the element the pointer pointed at. `at(mvd)` is `p[mvd]`.
//!
//! # The whole table, not the row
//!
//! `table` is the entire `pMvdCostTable` allocation, never the single QP row the cursor
//! sits in, so an index that strays out of its row reads the neighbouring row exactly as
//! the raw pointer did, and only an index that leaves the *allocation* panics.
//!
//! # Lifetime
//!
//! [`mod@crate::safe`] stores no borrow into a buffer except views that die with the call
//! chain that made them; this is one of those, derived inside the slice-encode loop and
//! never stored in the context. The table it borrows is written once at
//! `WelsInitEncoderExt` time and never again, which is what makes it lawful for each
//! per-slice worker thread to hold one.

/// A position in the encoder's MVD-cost table, indexed by a **signed** motion-vector
/// difference.
#[derive(Copy, Clone)]
pub struct MvdCostCursor<'a> {
    /// The whole table, not one QP row — see the module header.
    table: &'a [u16],
    /// Index of the entry the cursor points at: `at(0)`.
    at: usize,
}

impl<'a> MvdCostCursor<'a> {
    /// The unpositioned cursor — what the null `*mut u16` stood for.
    ///
    /// `SWelsMD::default()` produces one, and `WelsInitInterMDStruc` overwrites it
    /// per macroblock exactly where the C++ tests its pointer against null.
    pub const fn none() -> Self {
        Self { table: &[], at: 0 }
    }

    /// `table.as_ptr().add(at)`, as a cursor.
    pub const fn new(table: &'a [u16], at: usize) -> Self {
        Self { table, at }
    }

    /// The table's origin — the entry a zero MVD indexes, `iMvdCostTableSize` in.
    ///
    /// Callers derive `table` field-precisely (`&(*pEncCtx).pMvdCostTable[..]`, never a
    /// `&self` accessor): a whole-context shared borrow retags the whole context, which
    /// inside the fork races a worker's concurrent write to an inline context field.
    /// Holding the borrow across the whole macroblock loop is lawful because the table is
    /// written exactly once, by `MvdCostInit` inside `WelsInitEncoderExt`, before any
    /// slice worker exists.
    ///
    /// An empty table answers [`none`](Self::none).
    pub fn origin(table: &'a [u16], iMvdCostTableSize: i32) -> Self {
        if table.is_empty() {
            return Self::none();
        }
        let at = iMvdCostTableSize as usize;
        debug_assert!(
            at < table.len(),
            "the MVD table's origin is outside the table"
        );
        Self { table, at }
    }

    /// True for [`none`](Self::none) — the null test.
    #[inline(always)]
    pub const fn is_none(self) -> bool {
        self.table.is_empty()
    }

    /// `p[mvd]` — the read every `COST_MVD` is made of.
    ///
    /// Panics if the index leaves the table.
    #[inline(always)]
    pub fn at(self, mvd: i32) -> u16 {
        self.table[self.at.wrapping_add_signed(mvd as isize)]
    }

    /// `p.offset(d)` — a cursor re-parked `d` entries along, for the two consumers
    /// that walk away from the zero-MVD entry (`LineFullSearch_c`'s per-step bump
    /// and `SetFeatureSearchIn`'s per-axis rebase).
    #[inline(always)]
    pub const fn offset(self, d: i32) -> Self {
        Self {
            table: self.table,
            at: self.at.wrapping_add_signed(d as isize),
        }
    }
}

impl Default for MvdCostCursor<'_> {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the type exists for: index 0 is the middle, and both signs read.
    #[test]
    fn signed_indexing_reads_both_ways_from_the_parked_entry() {
        let table: Vec<u16> = (0..64u16).collect();
        let c = MvdCostCursor::new(&table, 32);
        assert_eq!(c.at(0), 32);
        assert_eq!(c.at(-4), 28);
        assert_eq!(c.at(4), 36);
        // `offset` composes.
        assert_eq!(c.offset(-3).at(1), 30);
        assert_eq!(c.offset(8).offset(-8).at(0), 32);
    }

    #[test]
    fn the_unpositioned_cursor_is_the_null_test() {
        assert!(MvdCostCursor::none().is_none());
        assert!(MvdCostCursor::default().is_none());
        let table = [0u16; 4];
        assert!(!MvdCostCursor::new(&table, 0).is_none());
    }

    #[test]
    #[should_panic]
    fn leaving_the_allocation_panics_rather_than_reading_off_the_end() {
        let table = [0u16; 8];
        let _ = MvdCostCursor::new(&table, 0).at(-1);
    }
}
