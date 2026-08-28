#![forbid(unsafe_code)]

//! The MVD-cost cursor — [`MvdCostCursor`], the safe stand-in for the encoder's
//! `uint16_t*` into `pCtx->pMvdCostTable`.
//!
//! # Why this is not a plain slice
//!
//! `COST_MVD` indexes the table with a **signed** motion-vector difference:
//! `p[iMvdX] + p[iMvdY]`, either operand of either sign. The C++ therefore parks
//! the pointer in the *middle* of a table row and lets the reads run in both
//! directions from it, and the port carried that as a raw `*mut u16` through
//! `SWelsMD`, `SWelsME` and the whole motion-search tree. A `&[u16]` cannot stand
//! in for such a pointer on its own — a slice's index 0 is its first element, and
//! there is no negative side of it.
//!
//! What can stand in is the pair: the table, plus the index of the element the
//! pointer pointed at. That is this type, and `at(mvd)` is `p[mvd]`.
//!
//! # Why the whole table and not the row
//!
//! `table` is the **entire** `pMvdCostTable` allocation, never the single QP row
//! the cursor sits in, and that is deliberate rather than lazy. The raw pointer
//! could stray outside its own row and still land inside the allocation, reading
//! whatever the neighbouring row holds; the encoder is byte-exact against the C++,
//! so a read like that has to keep reading the same value it read before. Bounding
//! the slice to the row would turn such a read into a panic — a behavioural change
//! that no differential sweep could have predicted, because it would fire on the
//! first stream that produced the straying MVD rather than on the ones already
//! measured. With the whole table, an index that leaves its row lands exactly where
//! the pointer landed, and only an index that leaves the *allocation* panics — which
//! is the case that was undefined behaviour before this type existed.
//!
//! # On `safe/`'s detached-cursor policy
//!
//! [`mod@crate::safe`]'s header says no type here stores a borrow into a buffer,
//! excepting the ephemeral views that never outlive the call chain that made them
//! ([`plane::PlaneCursor`](crate::safe::plane::PlaneCursor) and friends). This is
//! one of those: it is derived inside the slice-encode loop from the context's
//! table and dies with it. It is not stored in the context, and the table it
//! borrows is written once at `WelsInitEncoderExt` time and never again — which is
//! also what makes it lawful for the per-slice worker threads to hold one each.

/// A position in the encoder's MVD-cost table, indexed by a **signed** motion-vector
/// difference.
///
/// `Copy`, because the raw pointer it replaces was copied freely — into `SWelsME`
/// per search block, out of `SWelsMD` per macroblock, down the search tree by value.
#[derive(Copy, Clone)]
pub struct MvdCostCursor<'a> {
    /// The whole table, not one QP row — see the module header.
    table: &'a [u16],
    /// Index of the entry the raw cursor pointed at: `at(0)`.
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

    /// The table's **origin** — the entry a zero MVD indexes, `iMvdCostTableSize`
    /// in. Successor to `sWelsEncCtx::mvd_cost_origin`, which returned the same
    /// address as a `*mut u16` (**S5.C4b**).
    ///
    /// Callers derive `table` *field-precisely* — `&(*pEncCtx).pMvdCostTable[..]`,
    /// never a `&self` accessor — and that is not a stylistic preference. A
    /// whole-context shared borrow retags the whole context, and inside the fork
    /// that races any worker's concurrent write to an inline context field; a
    /// two-thread Miri probe shows each half of this directly (F228). The accessor
    /// this replaces got away with the whole-context borrow because it died on the
    /// next line — it handed back a pointer. The cursor is a borrow, held across
    /// the entire macroblock loop, so it takes the field and nothing else.
    ///
    /// Holding it that long is lawful because the table is written exactly once, by
    /// `MvdCostInit` inside `WelsInitEncoderExt`, before any slice worker exists —
    /// and because the `&[u16]` lands in the `Vec`'s *heap buffer*, a different
    /// allocation from the context, which no retag through `pEncCtx` can reach.
    ///
    /// An unsized table answers [`none`](Self::none), which is the null the raw
    /// accessor answered with and the same question `WelsInitInterMDStruc` asks.
    pub fn origin(table: &'a [u16], iMvdCostTableSize: i32) -> Self {
        if table.is_empty() {
            return Self::none();
        }
        let at = iMvdCostTableSize as usize;
        debug_assert!(at < table.len(), "the MVD table's origin is outside the table");
        Self { table, at }
    }

    /// True for [`none`](Self::none) — the null test, spelled safely.
    #[inline(always)]
    pub const fn is_none(self) -> bool {
        self.table.is_empty()
    }

    /// `p[mvd]` — the read every `COST_MVD` is made of.
    ///
    /// Panics if the index leaves the table. That is the case the raw pointer
    /// dereferenced out of bounds, so a panic here is strictly louder than what
    /// stood before, never quieter.
    #[inline(always)]
    pub fn at(self, mvd: i32) -> u16 {
        self.table[self.at.wrapping_add_signed(mvd as isize)]
    }

    /// `p.offset(d)` — a cursor re-parked `d` entries along, for the two consumers
    /// that walk away from the zero-MVD entry (`LineFullSearch_c`'s per-step bump
    /// and `SetFeatureSearchIn`'s per-axis rebase).
    #[inline(always)]
    pub const fn offset(self, d: i32) -> Self {
        Self { table: self.table, at: self.at.wrapping_add_signed(d as isize) }
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
        // `offset` composes, as the pointer arithmetic it replaces did.
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
