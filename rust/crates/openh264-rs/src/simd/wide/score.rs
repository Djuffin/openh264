//! The CAVLC bit-cost estimate on `wide` lane types — the twin of
//! `simd::x86_64::score`, which explains why this is the one kernel of `score.asm`'s
//! four that is worth having.
//!
//! The non-zero mask is a saturating narrow (`i8x16::from_i16x16_saturate`, which
//! is `packsswb`), a byte compare and `to_bitmask` (`pmovmskb`); the run-length sum
//! over set bits is the same scalar walk as the intrinsic kernel's.

#![forbid(unsafe_code)]

use wide::bytemuck::cast;
use wide::{i16x16, i16x8, i8x16, CmpEq};

use crate::encoder::encode_mb_aux::KI_TRUN_TABLE;

/// The 16-bit mask whose bit `i` is set when `dct[i]` is non-zero. The narrow
/// saturates, so a coefficient survives it non-zero whatever its magnitude.
#[inline(always)]
fn nonzero_mask(dct: &[i16; 16]) -> u32 {
    let v0 = i16x8::from_slice_unaligned(&dct[..8]);
    let v1 = i16x8::from_slice_unaligned(&dct[8..]);
    let packed = i8x16::from_i16x16_saturate(cast::<[i16x8; 2], i16x16>([v0, v1]));
    !packed.simd_eq(i8x16::ZERO).to_bitmask() & 0xFFFF
}

/// Highest set bit of `m`, or `-1` when `m` is zero.
#[inline(always)]
fn highest_set(m: u32) -> i32 {
    31 - m.leading_zeros() as i32
}

/// C++: `WelsCalculateSingleCtr4x4_sse2`, `codec/encoder/core/x86/score.asm:263`.
#[inline]
pub fn calculate_single_ctr_4x4_sse2(dct: &[i16; 16]) -> i32 {
    let nz = nonzero_mask(dct);

    let mut single_ctr: i32 = 0;
    let mut idx = highest_set(nz);

    while idx >= 0 {
        idx -= 1;
        let run_start = idx;
        let below = if idx < 0 { 0 } else { nz & ((1u32 << (idx + 1)) - 1) };
        idx = highest_set(below);
        let run = run_start - idx;
        if (run as usize) < KI_TRUN_TABLE.len() {
            single_ctr += KI_TRUN_TABLE[run as usize];
        }
    }

    single_ctr
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_mb_aux::calculate_single_ctr_4x4;

    /// `calculate_single_ctr_4x4` reads its input only through `== 0`, so its
    /// result is a function of the 16-bit non-zero mask alone — all 65536 of
    /// which fit in a test. Nothing weaker would do: the SSE2 kernel replaces
    /// the scalar's per-coefficient walk with a walk over set bits, so the two
    /// share no structure that a sampled test could lean on.
    #[test]
    fn single_ctr_sse2_matches_the_scalar_for_every_mask() {
        for mask in 0u32..=0xFFFF {
            let dct: [i16; 16] = core::array::from_fn(|i| ((mask >> i) & 1) as i16);
            assert_eq!(
                calculate_single_ctr_4x4_sse2(&dct),
                calculate_single_ctr_4x4(&dct),
                "mask {mask:#06x}"
            );
        }
    }

    /// The mask is built through a **saturating** narrow, so a coefficient whose
    /// low byte is zero has to stay non-zero. `256` is the smallest such value
    /// and `-256` its mirror; a `packuswb`/truncating kernel reports 0 for both.
    #[test]
    fn single_ctr_sse2_sees_coefficients_a_truncating_narrow_would_lose() {
        for &v in &[256i16, -256, 512, i16::MIN, 0x0100, 0x7F00] {
            for pos in 0..16 {
                let mut dct = [0i16; 16];
                dct[pos] = v;
                assert_eq!(
                    calculate_single_ctr_4x4_sse2(&dct),
                    calculate_single_ctr_4x4(&dct),
                    "value {v} at {pos}"
                );
            }
        }
    }
}
