#![forbid(unsafe_code)]

//! A slot arena addressed by copyable handles — the safe replacement for taxonomy
//! class **T4**, multi-alias object graphs (plan §1.2, contract §2.2.3).
//!
//! One decoder `SPicture` is reachable through up to nine locations at once (the DPB
//! pool, both ref lists, `pDec`, `pECRefPic`, the per-picture `pRefPic` graph — which
//! has *cycles* —, `SDeblockingFilter::pRefPics`, …). None of those aliases owns it;
//! they are all "which picture", spelled as an address. Spelled as a handle instead:
//! one owner, `Copy` handles, and the cyclic `pRefPic` graph becomes plain data,
//! because a handle does not own what it names.
//!
//! **Identity is handle equality.** The three decoder pointer comparisons that carry
//! real semantics — boundary strength "same reference picture?" (`deblocking.rs:258`),
//! the self-copy guard (`manage_dec_ref.rs:739`), and `error_concealment.rs:599` —
//! become `Id == Id`, which is the same predicate over the same slots (plan P3).
//!
//! This is generalised over `T` rather than written against `Picture`: the encoder
//! needs the same shape for its own picture pool (Phase 6.1/6.2), and `PicId` becomes
//! a newtype or alias over [`Id`] in Phase 5.1.

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

/// A handle to a slot in a [`Pool`].
///
/// # Staleness (plan §10 **D1**, decided: debug-only generations)
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
/// differently would be a debug/release semantic split, which is the class of
/// divergence finding F1 was made of.
#[derive(Clone, Copy, Debug)]
pub struct Id {
    index: u32,
    #[cfg(debug_assertions)]
    generation: u32,
}

impl Id {
    /// The slot this handle names.
    #[inline]
    pub fn index(self) -> usize {
        self.index as usize
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
        Id {
            index: index as u32,
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
        self.slots.iter().enumerate().map(|(i, slot)| {
            (
                Id {
                    index: i as u32,
                    #[cfg(debug_assertions)]
                    generation: self.generations[i],
                },
                slot,
            )
        })
    }

    #[cfg(debug_assertions)]
    #[inline]
    fn check(&self, id: Id) {
        assert!(
            (id.index as usize) < self.slots.len(),
            "handle {} outside a pool of {}",
            id.index,
            self.slots.len()
        );
        assert_eq!(
            self.generations[id.index as usize], id.generation,
            "stale handle to slot {}: it has been recycled {} time(s) since",
            id.index,
            self.generations[id.index as usize] - id.generation
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
        &self.slots[id.index as usize]
    }

    /// Mutable form of [`get`](Self::get).
    #[inline]
    pub fn get_mut(&mut self, id: Id) -> &mut T {
        self.check(id);
        &mut self.slots[id.index as usize]
    }

    /// Two slots mutably at once.
    ///
    /// # Panics
    /// If `a == b`. Two `&mut` to one picture is not a case the C++ has either — it
    /// is a port bug (plan P13), not a situation to recover from.
    pub fn pair_mut(&mut self, a: Id, b: Id) -> (&mut T, &mut T) {
        self.check(a);
        self.check(b);
        assert_ne!(a, b, "pair_mut on one slot ({})", a.index);
        let [x, y] = self
            .slots
            .get_disjoint_mut([a.index as usize, b.index as usize])
            .expect("pool handles must be distinct and in range");
        (x, y)
    }

    /// One slot mutably, plus read access to every *other* slot.
    ///
    /// This is the split the decoder needs constantly: the current picture as `&mut`
    /// while one or more reference pictures are read (B-slice MC reads two at once,
    /// plan P1). The C++ expresses it with several live `SPicture*`; here the borrow
    /// checker proves the disjointness that the C only assumes.
    ///
    /// **Deviation from plan §2.2.3**, which sketched
    /// `cur_and_refs(cur, refs: &[PicId]) -> (&mut T, RefViews)`. The reference *list*
    /// turned out to carry no weight: its only job was to reject `cur ∈ refs`, which
    /// [`PoolRest::get`] does anyway at the moment of access, and taking it would
    /// force either an allocation or an arbitrary fixed capacity in a per-macroblock
    /// path. Splitting the slot span instead is allocation-free and handles any
    /// access pattern. Recorded in the plan.
    pub fn mut_and_rest(&mut self, cur: Id) -> (&mut T, PoolRest<'_, T>) {
        self.check(cur);
        let index = cur.index as usize;
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
            self.generations[id.index as usize] += 1;
        }
        std::mem::replace(&mut self.slots[id.index as usize], value)
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

impl<T> PoolRest<'_, T> {
    /// The slot `id` names.
    ///
    /// # Panics
    /// If `id` names the slot that is held mutably, or is out of range, or — debug
    /// builds only — is stale.
    #[inline]
    pub fn get(&self, id: Id) -> &T {
        let index = id.index as usize;
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
                index: i as u32,
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
