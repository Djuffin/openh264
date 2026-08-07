// NOTE: this file is compiled twice, on purpose.
//   * as `crate::safe::prng` under `#[cfg(test)]`, for the in-module unit tests;
//   * as `common::prng` in `tests/`, via `#[path = "../../src/safe/prng.rs"]`,
//     for the differential integration tests.
// It must therefore stay free-standing: no `crate::` paths, no `super::`.

//! A deterministic PRNG for property-style tests.
//!
//! The safety refactor takes **no** new dependencies, dev-dependencies included
//! (plan §2.1.5), so the property-style tests roll their own generator instead of
//! reaching for `proptest`/`quickcheck`. That is a feature here rather than a
//! concession: the tests run under Miri, where a shrinking framework would be
//! unaffordably slow, and a fixed seed printed in the assertion message reproduces a
//! failure exactly — in either test layer, since both include this same file.
//!
//! xorshift64\* (Vigna 2016). Not cryptographic; it only has to be reproducible and
//! to spread bits well enough that the differential tests hit boundary cases.

/// xorshift64\* state. Seeded explicitly; never from the clock.
#[derive(Clone, Debug)]
pub struct Prng {
    state: u64,
    seed: u64,
}

impl Prng {
    /// Creates a generator from `seed`. Zero is remapped, since xorshift64 has zero
    /// as a fixed point.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
            seed,
        }
    }

    /// The seed this generator was created with — print it in assertion messages so
    /// a failing case can be replayed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    pub fn next_u8(&mut self) -> u8 {
        (self.next_u64() >> 56) as u8
    }

    /// Uniform-ish value in `0..n`. Biased by at most 2^-32 for the small `n` these
    /// tests use, which is irrelevant to their purpose.
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "below(0)");
        self.next_u32() % n
    }

    /// Uniform-ish value in `lo..=hi`.
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        assert!(lo <= hi, "range_i32({lo}, {hi})");
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    /// `len` pseudo-random bytes.
    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next_u8()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Prng;

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
