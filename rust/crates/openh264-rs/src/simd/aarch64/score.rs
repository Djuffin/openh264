//! The CAVLC bit-cost estimate.
//!
//! The sixteen `== 0` tests are one vector compare and a mask, and the run-length sum
//! is a walk over the set bits, one iteration per non-zero coefficient.
//!
//! NEON has no `pmovmskb`. The mask comes out of the compare as a byte per
//! coefficient (`cmeq` on words, narrowed with `uzp1` — the low byte of each), and is
//! turned into sixteen bits by ANDing each byte with its lane's power of two and
//! adding across each eight-lane half, the standard NEON movemask idiom.
//!
//! The result depends on `dct` only through that mask.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld8_i16, ld16};
use crate::encoder::encode_mb_aux::KI_TRUN_TABLE;

/// The 16-bit mask whose bit `i` is set when `dct[i]` is non-zero.
#[inline]
#[target_feature(enable = "neon")]
fn nonzero_mask(dct: &[i16; 16]) -> u32 {
    let z0 = vceqzq_s16(ld8_i16(&dct[..8]));
    let z1 = vceqzq_s16(ld8_i16(&dct[8..]));
    // `uzp1 .16b` keeps the low byte of every word lane: `0xFF` where the coefficient
    // is zero. The compare is on the whole word, so no magnitude is lost on the way.
    let zero = vuzp1q_u8(vreinterpretq_u8_u16(z0), vreinterpretq_u8_u16(z1));
    let nonzero = vmvnq_u8(zero);
    let bits = vandq_u8(nonzero, ld16(&POWERS));
    let lo = vaddv_u8(vget_low_u8(bits)) as u32;
    let hi = vaddv_u8(vget_high_u8(bits)) as u32;
    lo | (hi << 8)
}

/// Bit `i & 7` set in lane `i`: the weights of the movemask reduce.
static POWERS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];

/// Highest set bit of `m`, or `-1` when `m` is zero.
#[inline(always)]
fn highest_set(m: u32) -> i32 {
    31 - m.leading_zeros() as i32
}

/// JVT-O079 CAVLC bit-cost estimate: for each run of zeros between non-zero
/// coefficients (scanning from the high end), add the run-length penalty.
///
/// C++: `WelsCalculateSingleCtr4x4_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
#[inline]
pub fn calculate_single_ctr_4x4(dct: &[i16; 16]) -> i32 {
    // SAFETY: NEON is baseline on aarch64.
    let nz = unsafe { nonzero_mask(dct) };

    let mut single_ctr: i32 = 0;
    // Skip the trailing zeros above the top coefficient.
    let mut idx = highest_set(nz);

    while idx >= 0 {
        // Step past that coefficient, then measure the run of zeros below it.
        idx -= 1;
        let run_start = idx;
        let below = if idx < 0 {
            0
        } else {
            nz & ((1u32 << (idx + 1)) - 1)
        };
        idx = highest_set(below);
        let run = run_start - idx;
        if (run as usize) < KI_TRUN_TABLE.len() {
            single_ctr += KI_TRUN_TABLE[run as usize];
        }
    }

    single_ctr
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_mb_aux::calculate_single_ctr_4x4 as scalar;

    /// The result is a function of the 16-bit non-zero mask alone, so all 65536 masks
    /// are checked against the scalar.
    #[test]
    fn single_ctr_matches_the_scalar_for_every_mask() {
        for mask in 0u32..=0xFFFF {
            let dct: [i16; 16] = core::array::from_fn(|i| ((mask >> i) & 1) as i16);
            assert_eq!(
                calculate_single_ctr_4x4(&dct),
                scalar(&dct),
                "mask {mask:#06x}"
            );
        }
    }

    /// The compare is on whole words, so a coefficient whose low byte is zero still
    /// counts as non-zero; `256` is the smallest such value.
    #[test]
    fn single_ctr_sees_coefficients_a_truncating_narrow_would_lose() {
        for &v in &[256i16, -256, 512, i16::MIN, 0x0100, 0x7F00] {
            for pos in 0..16 {
                let mut dct = [0i16; 16];
                dct[pos] = v;
                assert_eq!(
                    calculate_single_ctr_4x4(&dct),
                    scalar(&dct),
                    "value {v} at {pos}"
                );
            }
        }
    }
}
