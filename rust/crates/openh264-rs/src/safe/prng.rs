#![forbid(unsafe_code)]
// Compiled twice: as `crate::safe::prng` under `#[cfg(test)]`, and as `common::prng`
// in `tests/` via `#[path = "../../src/safe/prng.rs"]`. It must therefore stay
// free-standing — no `crate::` or `super::` paths — and must contain no
// `#[cfg(test)] mod tests`, since `cfg(test)` is on in integration-test crates too and
// such tests would compile into every `tests/` binary. Its own tests live in
// `safe::prng_tests` (`src/safe/mod.rs`).

//! A deterministic PRNG for property-style tests.
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
    /// Creates a generator from `seed`. Zero is remapped; xorshift64 has zero as a
    /// fixed point.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
            seed,
        }
    }

    /// The seed this generator was created with — print it to replay a failing case.
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

    /// Uniform-ish value in `0..n`. Modulo bias is at most 2^-32 for small `n`.
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
