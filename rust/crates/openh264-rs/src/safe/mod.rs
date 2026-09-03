#![forbid(unsafe_code)]

//! Safe vocabulary types for the codec.
//!
//! * **Detached cursors.** No type here stores a borrow into a buffer.
//!   Cursors are *positions*; buffers are *parameters*. The only lifetime-carrying
//!   types are the ephemeral views ([`plane::PlaneCursor`], [`plane::PlaneCursorMut`],
//!   [`pool::PoolRest`]) that never outlive the call chain that made them.
//! * **Bounds come from slice indexing.** These types do not add bounds checks of
//!   their own beyond validating their construction invariant; they arrange for the
//!   arithmetic to land in a slice index, so an out-of-range access is a loud panic
//!   instead of silent corruption.
//!
//! # Map
//!
//! | module | replaces |
//! |---|---|
//! | [`plane`] | pixel-plane cursors with stride math and negative offsets |
//! | [`bits`] | detachable bit cursors (`SBitStringAux` reader *and* writer) |
//! | [`pool`] | multi-alias object graphs (DPB, ref lists) |
//! | [`mb_grid`] | per-MB metadata addressing |

pub mod bits;
pub mod err;
pub mod mb_grid;
pub mod mvd_cost;
pub mod plane;
pub mod pool;

/// Deterministic PRNG for the property-style unit tests in this module.
///
/// Kept in a file of its own so the differential integration test can include the
/// *same* generator by path (`tests/common/prng.rs`), which is what makes a failing
/// seed reproducible across both test layers.
#[cfg(test)]
#[path = "prng.rs"]
pub(crate) mod prng;

/// Tests for [`prng`] itself.
///
/// They live here rather than in `prng.rs` because that file is included by path
/// into the integration-test crates, where `cfg(test)` is also on — tests declared
/// there would compile into pre-existing test binaries.
#[cfg(test)]
mod prng_tests {
    use super::prng::Prng;

    #[test]
    fn deterministic_for_a_seed() {
        let a: Vec<u64> = (0..8).map(|_| Prng::new(12345).next_u64()).collect();
        assert!(a.iter().all(|&v| v == a[0]), "same seed, same first draw");

        let mut p = Prng::new(12345);
        let first: Vec<u64> = (0..8).map(|_| p.next_u64()).collect();
        let mut q = Prng::new(12345);
        let second: Vec<u64> = (0..8).map(|_| q.next_u64()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn zero_seed_does_not_stick() {
        let mut p = Prng::new(0);
        let v: Vec<u64> = (0..4).map(|_| p.next_u64()).collect();
        assert!(v.iter().all(|&x| x != 0));
    }

    #[test]
    fn ranges_are_respected() {
        let mut p = Prng::new(7);
        for _ in 0..1000 {
            let v = p.range_i32(-33, 33);
            assert!((-33..=33).contains(&v), "range_i32 out of range: {v}");
            assert!(p.below(5) < 5);
        }
    }
}
