#![forbid(unsafe_code)]

//! A slot arena addressed by copyable handles.
//!
//! One decoder `SPicture` is reachable through up to nine locations at once (the DPB
//! pool, both ref lists, `pDec`, `pECRefPic`, the per-picture `pRefPic` graph — which
//! has *cycles* —, `SDeblockingFilter::pRefPics`, …). None of those aliases owns it;
//! they are all "which picture". One owner, `Copy` handles, and the cyclic `pRefPic`
//! graph becomes plain data, because a handle does not own what it names.
//!
//! **Identity is handle equality**: `picture.rs`'s `same_picture` compares the slot
//! each picture was allocated into. The comparisons it serves are boundary strength's
//! "same reference picture?" (`deblocking.rs`), error concealment's four self-copy
//! guards, and `manage_dec_ref.rs`'s EC prefetch overlap test.
//!
//! This is generalised over `T` rather than written against `Picture`: the encoder
//! needs the same shape for its own picture pool. `PicId` is an alias over [`Id`]
//! (`pic_queue.rs`).

use std::num::NonZeroU32;

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

/// A handle to a slot in a [`Pool`].
///
/// # Staleness
///
/// Recycling can hand out a slot that an old handle still names, exactly as the C++
/// can hand out a `SPicture*` to memory a new picture now occupies. That hazard is
/// *preserved* rather than fixed, because fixing it would change decode behaviour;
/// but in a debug build each slot carries a generation counter that
/// [`Pool::replace`] bumps and every accessor checks, so the tests catch logic rot
/// that release builds would silently tolerate.
///
/// **Equality never consults the generation**, in either profile: a handle names a
/// slot, and two handles to one slot are equal — a debug build that answered
/// differently would be a debug/release semantic split.
///
/// # Representation
///
/// The field holds **`slot + 1`**, so `Id` has a niche and `Option<Id>` is one word
/// with no separate discriminant. The consumers that make this worth spelling
/// out are the reference-id arrays deblocking fills and compares per macroblock
/// (`[[Option<PicId>; 16]; 2]`, `deblocking.rs`): a niche halves them and makes `==`
/// one comparison instead of two.
#[derive(Clone, Copy, Debug)]
pub struct Id {
    /// `slot + 1`. Never read directly — [`Id::index`] subtracts the bias.
    index: NonZeroU32,
    #[cfg(debug_assertions)]
    generation: u32,
}

impl Id {
    /// The slot this handle names.
    #[inline]
    pub fn index(self) -> usize {
        self.index.get() as usize - 1
    }
}

impl PartialEq for Id {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for Id {}

impl std::hash::Hash for Id {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

// ---------------------------------------------------------------------------
// Pool
// ---------------------------------------------------------------------------

/// A fixed set of slots, owned in one place and addressed by [`Id`].
#[derive(Debug)]
pub struct Pool<T> {
    slots: Vec<T>,
    #[cfg(debug_assertions)]
    generations: Vec<u32>,
}

impl<T> Pool<T> {
    /// Takes ownership of `slots`. The pool never grows or shrinks: the C++ picture
    /// queues are sized once at initialisation and recycled thereafter.
    pub fn new(slots: Vec<T>) -> Self {
        #[cfg(debug_assertions)]
        let generations = vec![0u32; slots.len()];
        Self {
            slots,
            #[cfg(debug_assertions)]
            generations,
        }
    }

    /// Number of slots.
    #[inline]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the pool has no slots.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// A handle to slot `index`, stamped with its current generation.
    ///
    /// # Panics
    /// If `index` is out of range.
    #[inline]
    pub fn id(&self, index: usize) -> Id {
        assert!(
            index < self.slots.len(),
            "slot {index} outside a pool of {}",
            self.slots.len()
        );
        self.handle(index)
    }

    /// [`id`](Self::id) without the range assert — for callers that have just read
    /// the index out of the slot vector itself.
    #[inline]
    fn handle(&self, index: usize) -> Id {
        Id {
            index: NonZeroU32::new(index as u32 + 1).expect("slot index + 1 is non-zero"),
            #[cfg(debug_assertions)]
            generation: self.generations[index],
        }
    }

    /// Handles to every slot, in order — the iteration the recycling predicates do
    /// (`find_free`-style searches over `bUsedAsRef`/`iRefCount`).
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        (0..self.slots.len()).map(|i| self.id(i))
    }

    /// Every slot with its handle.
    pub fn iter(&self) -> impl Iterator<Item = (Id, &T)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .map(|(i, slot)| (self.handle(i), slot))
    }

    #[cfg(debug_assertions)]
    #[inline]
    fn check(&self, id: Id) {
        assert!(
            id.index() < self.slots.len(),
            "handle {} outside a pool of {}",
            id.index(),
            self.slots.len()
        );
        assert_eq!(
            self.generations[id.index()],
            id.generation,
            "stale handle to slot {}: it has been recycled {} time(s) since",
            id.index(),
            self.generations[id.index()] - id.generation
        );
    }

    #[cfg(not(debug_assertions))]
    #[inline]
    fn check(&self, _id: Id) {}

    /// The slot `id` names.
    ///
    /// # Panics
    /// If `id` is out of range, or — in a debug build only — stale.
    #[inline]
    pub fn get(&self, id: Id) -> &T {
        self.check(id);
        &self.slots[id.index()]
    }

    /// Mutable form of [`get`](Self::get).
    #[inline]
    pub fn get_mut(&mut self, id: Id) -> &mut T {
        self.check(id);
        &mut self.slots[id.index()]
    }

    /// Two slots mutably at once.
    ///
    /// # Panics
    /// If `a == b`.
    pub fn pair_mut(&mut self, a: Id, b: Id) -> (&mut T, &mut T) {
        self.check(a);
        self.check(b);
        assert_ne!(a, b, "pair_mut on one slot ({})", a.index());
        let [x, y] = self
            .slots
            .get_disjoint_mut([a.index(), b.index()])
            .expect("pool handles must be distinct and in range");
        (x, y)
    }

    /// One slot mutably, plus read access to every *other* slot.
    ///
    /// This is the split the decoder needs constantly: the current picture as `&mut`
    /// while one or more reference pictures are read (B-slice MC reads two at once).
    pub fn mut_and_rest(&mut self, cur: Id) -> (&mut T, PoolRest<'_, T>) {
        self.check(cur);
        let index = cur.index();
        let (lo, rest) = self.slots.split_at_mut(index);
        let (slot, hi) = rest.split_first_mut().expect("index is in range");
        (
            slot,
            PoolRest {
                lo,
                hi,
                cur: index,
                #[cfg(debug_assertions)]
                generations: &self.generations,
            },
        )
    }

    /// Appends slots to the end of the pool.
    ///
    /// **This is the one place the "never grows or shrinks" contract above is
    /// relaxed.** `WelsRequestMem`'s third arm resizes the decoder's picture pool in
    /// place when a stream changes its reference-frame count without changing
    /// resolution (`decoder.cpp:493-509`).
    ///
    /// Existing slots keep their index **and their generation**, so every outstanding
    /// handle stays valid. That is the faithful reading of `IncreasePicBuff`
    /// (`decoder.cpp:143`), which `memcpy`s the old `PPicture` array into the front of
    /// the new one: a picture keeps its position, so a handle keeps its meaning. New
    /// slots start at generation 0 and no handle to them can exist yet.
    pub fn grow(&mut self, extra: Vec<T>) {
        #[cfg(debug_assertions)]
        {
            // Past every generation now live. `grow` after `reorder_and_shrink`
            // **reuses indices the shrink dropped**, and a slot dropped at generation
            // 0 and re-created at generation 0 would accept a handle taken before the
            // shrink — the one confusion the counter exists to prevent. Derived
            // rather than stored: a pool of sixteen slots makes this a sixteen-element
            // max on a path that runs once per sequence.
            let fresh = self
                .generations
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
                .wrapping_add(1);
            self.generations.resize(self.slots.len() + extra.len(), fresh);
        }
        self.slots.extend(extra);
    }

    /// Rebuilds the pool as `order.len()` slots, new slot `i` taking the value that
    /// was at old index `order[i]`. Returns every value no index in `order` named, in
    /// old-index order.
    ///
    /// This is `DecreasePicBuff` (`decoder.cpp:170`), which is not a truncation: when
    /// the DPB's previously-decoded picture sits beyond the new size it is moved to
    /// slot 0 and the rest shift up by one. The returned values are what the C++
    /// `FreePicture`s — by construction rather than by its `if (iPrevPicIdx !=
    /// iPicIdx)` guard, since a value can only be moved out once.
    ///
    /// # Generations
    ///
    /// A slot that receives a *different* value has its generation bumped, so a
    /// handle made before the reorder faults on access in a debug build instead of
    /// silently naming another picture. A slot that keeps its own value
    /// (`order[i] == i`) keeps its generation and its handles.
    ///
    /// **This is stricter than the C++ and deliberately so.** Here identity is the
    /// slot, so a caller that keeps an [`Id`] across this call must re-derive it.
    /// `DecreasePicBuff` re-derives the one id the C++ deliberately preserves and
    /// clears the rest, which is why nothing faults.
    ///
    /// # Panics
    /// If `order` is longer than the pool, names an index out of range, or names one
    /// twice — each would mean a slot had to be duplicated or invented, which is a
    /// caller bug and not a state to recover from.
    pub fn reorder_and_shrink(&mut self, order: &[usize]) -> Vec<T> {
        let old_len = self.slots.len();
        assert!(
            order.len() <= old_len,
            "reorder_and_shrink to {} slots from a pool of {old_len}",
            order.len()
        );
        let mut seen = vec![false; old_len];
        for &i in order {
            assert!(i < old_len, "index {i} outside a pool of {old_len}");
            assert!(!std::mem::replace(&mut seen[i], true), "index {i} named twice");
        }

        #[cfg(debug_assertions)]
        {
            let gens: Vec<u32> = order
                .iter()
                .enumerate()
                .map(|(new_i, &old_i)| {
                    if new_i == old_i {
                        self.generations[old_i]
                    } else {
                        self.generations[old_i].wrapping_add(1)
                    }
                })
                .collect();
            self.generations = gens;
        }

        let mut old: Vec<Option<T>> = self.slots.drain(..).map(Some).collect();
        let mut kept = Vec::with_capacity(order.len());
        for &i in order {
            kept.push(old[i].take().expect("each index is named at most once"));
        }
        self.slots = kept;
        old.into_iter().flatten().collect()
    }

    /// Replaces a slot's contents, invalidating every outstanding handle to it in
    /// debug builds.
    ///
    /// This is the recycling operation (`AllocPicture`/`FreePicture`'s slot reuse):
    /// in release it is a plain assignment with C-identical semantics, including the
    /// hazard that an old handle now names the new occupant.
    pub fn replace(&mut self, id: Id, value: T) -> T {
        self.check(id);
        #[cfg(debug_assertions)]
        {
            self.generations[id.index()] += 1;
        }
        std::mem::replace(&mut self.slots[id.index()], value)
    }
}

/// Read access to every slot of a [`Pool`] except the one held mutably.
///
/// Produced by [`Pool::mut_and_rest`]; lives only for the call chain that made it
/// (this is one of the module's three ephemeral view types — see [`crate::safe`]).
#[derive(Debug)]
pub struct PoolRest<'a, T> {
    lo: &'a [T],
    hi: &'a [T],
    cur: usize,
    #[cfg(debug_assertions)]
    generations: &'a [u32],
}

// `Copy` is hand-written because `#[derive]` would add a `T: Copy` bound, and the
// `T` this view is built over is `Option<Box<SPicture>>` — the slots are not
// copyable and are not being copied: the fields above are two shared slices and an
// index, and copying them is copying *borrows*. A `PoolRest` that is not `Copy`
// forces every signature it is threaded through to take it by reference, which grows
// a second lifetime across the whole macroblock tree.
impl<T> Clone for PoolRest<'_, T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for PoolRest<'_, T> {}

impl<'a, T> PoolRest<'a, T> {
    /// The slot `id` names.
    ///
    /// **The borrow is the view's, not this call's**: the rest is two shared slices
    /// with lifetime `'a`, so a result may outlive the `&self` that asked for it.
    ///
    /// # Panics
    /// If `id` names the slot that is held mutably, or is out of range, or — debug
    /// builds only — is stale.
    #[inline]
    pub fn get(&self, id: Id) -> &'a T {
        let index = id.index();
        assert_ne!(
            index, self.cur,
            "slot {index} is the one held mutably by this split"
        );
        #[cfg(debug_assertions)]
        assert_eq!(
            self.generations[index], id.generation,
            "stale handle to slot {index}"
        );
        if index < self.cur {
            &self.lo[index]
        } else {
            &self.hi[index - self.cur - 1]
        }
    }

    /// Total number of slots in the pool this view came from.
    #[inline]
    pub fn pool_len(&self) -> usize {
        self.lo.len() + self.hi.len() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_of(n: usize) -> Pool<i32> {
        Pool::new((0..n as i32).map(|i| i * 10).collect())
    }

    #[test]
    fn handles_address_their_slots() {
        let p = pool_of(4);
        assert_eq!(p.len(), 4);
        for i in 0..4 {
            assert_eq!(*p.get(p.id(i)), i as i32 * 10);
            assert_eq!(p.id(i).index(), i);
        }
    }

    #[test]
    fn an_optional_handle_costs_no_more_than_a_handle() {
        // The niche, pinned: `Id`'s field is `slot + 1`, so `None` is the zero and
        // `Option<Id>` needs no discriminant. `deblocking.rs` fills
        // `[[Option<PicId>; 16]; 2]` per macroblock and compares six of them per
        // edge; this is what keeps that one word and one comparison.
        assert_eq!(
            std::mem::size_of::<Option<Id>>(),
            std::mem::size_of::<Id>()
        );
        // And the bias is invisible from outside: slot 0 round-trips.
        assert_eq!(pool_of(1).id(0).index(), 0);
    }

    #[test]
    fn identity_is_handle_equality() {
        let p = pool_of(4);
        assert_eq!(p.id(2), p.id(2));
        assert_ne!(p.id(2), p.id(3));
    }

    #[test]
    fn iteration_pairs_handles_with_slots() {
        let p = pool_of(3);
        let seen: Vec<(usize, i32)> = p.iter().map(|(id, v)| (id.index(), *v)).collect();
        assert_eq!(seen, vec![(0, 0), (1, 10), (2, 20)]);
        assert_eq!(p.ids().count(), 3);
        // The `find_free` shape: search by predicate, return the handle.
        let found = p.iter().find(|(_, v)| **v == 10).map(|(id, _)| id);
        assert_eq!(found, Some(p.id(1)));
    }

    #[test]
    fn get_mut_writes_through() {
        let mut p = pool_of(3);
        let id = p.id(1);
        *p.get_mut(id) = 99;
        assert_eq!(*p.get(id), 99);
    }

    #[test]
    fn pair_mut_hands_out_two_disjoint_slots() {
        let mut p = pool_of(4);
        let (a, b) = p.pair_mut(p.id(0), p.id(3));
        *a += 1;
        *b += 2;
        assert_eq!(*a, 1);
        assert_eq!(*b, 32);
    }

    #[test]
    #[should_panic(expected = "pair_mut on one slot")]
    fn pair_mut_rejects_aliasing() {
        let mut p = pool_of(4);
        let id = p.id(2);
        p.pair_mut(id, id);
    }

    #[test]
    fn mut_and_rest_holds_one_write_and_many_reads() {
        let mut p = pool_of(5);
        let (cur, r0, r1) = (p.id(2), p.id(0), p.id(4));
        let (slot, rest) = p.mut_and_rest(cur);
        *slot = *rest.get(r0) + *rest.get(r1); // 0 + 40
        assert_eq!(*slot, 40);
        assert_eq!(rest.pool_len(), 5);
        assert_eq!(*p.get(cur), 40);
    }

    #[test]
    fn mut_and_rest_maps_both_sides_of_the_split() {
        let mut p = pool_of(5);
        let cur = p.id(2);
        let (_slot, rest) = p.mut_and_rest(cur);
        for i in [0usize, 1, 3, 4] {
            let id = Id {
                index: NonZeroU32::new(i as u32 + 1).unwrap(),
                #[cfg(debug_assertions)]
                generation: 0,
            };
            assert_eq!(*rest.get(id), i as i32 * 10, "slot {i}");
        }
    }

    #[test]
    #[should_panic(expected = "held mutably")]
    fn mut_and_rest_rejects_reading_the_slot_it_lends_out() {
        let mut p = pool_of(5);
        let cur = p.id(2);
        let (_slot, rest) = p.mut_and_rest(cur);
        rest.get(cur);
    }

    #[test]
    #[should_panic(expected = "outside a pool")]
    fn id_rejects_an_out_of_range_slot() {
        let p = pool_of(2);
        p.id(2);
    }

    // --- grow / reorder_and_shrink -----------------------------------------

    #[test]
    fn grow_appends_and_keeps_every_old_handle() {
        let mut p = pool_of(3);
        let old: Vec<Id> = p.ids().collect();
        p.grow(vec![30, 40, 50]);
        assert_eq!(p.len(), 6);
        // the old handles still name the same values — `IncreasePicBuff` memcpy's the
        // old array into the front, so a picture keeps its position
        for (i, id) in old.iter().enumerate() {
            assert_eq!(*p.get(*id), i as i32 * 10);
        }
        assert_eq!(*p.get(p.id(5)), 50);
    }

    #[test]
    fn shrink_keeps_the_named_slots_and_returns_the_rest() {
        let mut p = pool_of(5); // 0 10 20 30 40
        let dropped = p.reorder_and_shrink(&[0, 1, 2]);
        assert_eq!(p.len(), 3);
        assert_eq!(dropped, vec![30, 40], "returned in old-index order");
        assert_eq!(*p.get(p.id(0)), 0);
        assert_eq!(*p.get(p.id(2)), 20);
    }

    #[test]
    fn shrink_can_reorder_the_slots_it_keeps() {
        // `DecreasePicBuff`'s first arm: the previously-decoded picture sits beyond
        // the new size, so it moves to slot 0 and the rest shift up by one.
        let mut p = pool_of(5); // 0 10 20 30 40
        let dropped = p.reorder_and_shrink(&[4, 0, 1]);
        assert_eq!(p.len(), 3);
        assert_eq!(*p.get(p.id(0)), 40);
        assert_eq!(*p.get(p.id(1)), 0);
        assert_eq!(*p.get(p.id(2)), 10);
        assert_eq!(dropped, vec![20, 30]);
    }

    /// A slot that keeps its own value keeps its handles; a slot that receives a
    /// different value does not. Without this, a `PicId` taken before the resize
    /// would silently name another picture — the failure `DecreasePicBuff` has to
    /// re-derive around, and the reason it clears every `pRefPic` entry.
    // `#[cfg]` rather than `#[ignore]`, matching
    // `debug_builds_catch_a_handle_to_a_recycled_slot` below: generations do not
    // exist in a release build.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "stale handle")]
    fn a_handle_to_a_reordered_slot_is_stale() {
        let mut p = pool_of(5);
        let slot0 = p.id(0);
        p.reorder_and_shrink(&[4, 0, 1]); // slot 0 now holds what was at 4
        let _ = p.get(slot0);
    }

    #[test]
    fn a_handle_to_a_slot_that_did_not_move_survives_the_shrink() {
        let mut p = pool_of(5);
        let slot1 = p.id(1);
        p.reorder_and_shrink(&[0, 1, 2]);
        assert_eq!(*p.get(slot1), 10, "order[1] == 1, so the handle is still good");
    }

    /// Growing after a shrink must not resurrect a handle to a slot the shrink
    /// dropped: the new occupant of that index is a different value, and the old
    /// handle carries the pre-shrink generation.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "stale handle")]
    fn a_dropped_slot_reused_by_a_later_grow_rejects_the_old_handle() {
        let mut p = pool_of(4); // 0 10 20 30
        let slot3 = p.id(3);
        p.reorder_and_shrink(&[3, 0]); // slot 0 <- 30, slot 1 <- 0; index 3 is gone
        p.grow(vec![70, 80]); // indices 2 and 3 exist again, holding 70 and 80
        assert_eq!(p.len(), 4);
        // `slot3` names index 3 at generation 0; `grow` stamps fresh slots past every
        // generation the shrink left live, so slot 3 is not at 0 any more.
        let _ = p.get(slot3);
    }

    #[test]
    #[should_panic(expected = "named twice")]
    fn shrink_rejects_a_duplicated_index() {
        pool_of(4).reorder_and_shrink(&[1, 1]);
    }

    #[test]
    #[should_panic(expected = "outside a pool")]
    fn shrink_rejects_an_out_of_range_index() {
        pool_of(4).reorder_and_shrink(&[0, 9]);
    }

    #[test]
    fn replace_returns_the_old_occupant() {
        let mut p = pool_of(3);
        let id = p.id(1);
        assert_eq!(p.replace(id, 77), 10);
        assert_eq!(*p.get(p.id(1)), 77);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "stale handle")]
    fn debug_builds_catch_a_handle_to_a_recycled_slot() {
        let mut p = pool_of(3);
        let old = p.id(1);
        p.replace(old, 77);
        p.get(old);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_stale_handle_still_compares_equal_to_a_fresh_one() {
        // Release builds cannot tell these apart at all; debug builds must not either,
        // or identity semantics would differ between profiles.
        let mut p = pool_of(3);
        let old = p.id(1);
        p.replace(old, 77);
        assert_eq!(old, p.id(1));
    }
}
