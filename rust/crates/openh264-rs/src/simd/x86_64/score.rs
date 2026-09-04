//! x86_64 SSE2 CAVLC scoring kernel.
//!
//! Translated from `codec/encoder/core/x86/score.asm`, of whose four kernels
//! this file holds one. `WelsGetNoneZeroCount_sse2` lives next to the quantizers
//! it is read with, in [`super::quant::get_none_zero_count_sse2`].
//!
//! # Why the two scan kernels are not here
//!
//! `WelsScan4x4DcAc_sse2` and `WelsScan4x4Ac_sse2` were written, checked against
//! the scalar, and then dropped, because **the scalar is already the faster
//! kernel**. `scan_4x4_dc_ac`'s table-driven loop compiles to 13 instructions —
//! two loads, eight shuffles, two `pinsrw`, two stores, no branches — against 21
//! for a transcription of the asm.
//!
//! The asm loses on an instruction it is not allowed to use. It reads its input
//! with `movdqa`, so both halves arrive lane-aligned and the four samples that
//! cross between them have to be lifted out one at a time with `pextrw`/`pinsrw`.
//! Given `movdqu`, LLVM instead issues the second load at a **7-word offset** —
//! `movdqu 14(%rdx)` — which lands those samples in the lanes they belong in, and
//! the extract/insert chain disappears. Handing it `_mm_insert_epi16` calls
//! instead costs two 16-byte blend constants out of the constant pool.
//!
//! Re-check with:
//! `cargo rustc --release --lib -- --emit asm`, then read `scan_4x4_dc_ac` and
//! `scan_4x4_ac` in the emitted `.s`. If they ever stop being shuffle sequences,
//! `score.asm:173` and `:225` are the kernels to transcribe.

#![allow(unsafe_code)]

use core::arch::x86_64::*;

// ============================================================================
// CAVLC bit-cost estimate
// ============================================================================

/// The 16-bit mask whose bit `i` is set when `dct[i]` is non-zero.
///
/// `packs` saturates rather than truncates, so a coefficient survives it
/// non-zero whatever its magnitude: `_mm_packs_epi16` maps `v` to
/// `clamp(v, -128, 127)`, which is `0` only for `v == 0`. A truncating narrow
/// would lose every multiple of 256.
#[target_feature(enable = "sse2")]
unsafe fn nonzero_mask_sse2(dct: &[i16; 16]) -> u32 {
    unsafe {
        let zero = _mm_setzero_si128();
        let v0 = _mm_loadu_si128(dct.as_ptr() as *const __m128i);
        let v1 = _mm_loadu_si128(dct.as_ptr().add(8) as *const __m128i);
        let packed = _mm_packs_epi16(v0, v1);
        let is_zero = _mm_cmpeq_epi8(packed, zero);
        !(_mm_movemask_epi8(is_zero) as u32) & 0xFFFF
    }
}

/// Highest set bit of `m`, or `-1` when `m` is zero — "scan down while the
/// coefficient is zero", which is what both of the scalar's inner loops do.
#[inline(always)]
fn highest_set(m: u32) -> i32 {
    31 - m.leading_zeros() as i32
}

/// JVT-O079 CAVLC bit-cost estimate: for each run of zeros between non-zero
/// coefficients (scanning from the high end), add the run-length penalty.
///
/// C++: `WelsCalculateSingleCtr4x4_sse2`, `codec/encoder/core/x86/score.asm:263`.
///
/// **Not a transcription.** The asm builds the same non-zero mask this does and
/// then reads the answer out of three 256-entry tables, one for the span between
/// the outermost coefficients and one per byte half. Those tables are the
/// scalar's run-length sum precomputed, so transcribing them would state the
/// summation a second time in a layout nothing else here uses. The mask is the
/// part worth vectorising — it is what costs the scalar sixteen data-dependent
/// branches — so it is built with SSE2 and the sum is then a walk over set bits,
/// one iteration per non-zero coefficient rather than one per coefficient.
///
/// The result depends on `dct` **only** through this mask, which is what lets
/// `single_ctr_sse2_matches_the_scalar_for_every_mask` check all 65536 of them.
#[target_feature(enable = "sse2")]
unsafe fn calculate_single_ctr_4x4_sse2_impl(dct: &[i16; 16]) -> i32 {
    use crate::encoder::encode_mb_aux::KI_TRUN_TABLE;

    let nz = unsafe { nonzero_mask_sse2(dct) };

    let mut single_ctr: i32 = 0;
    // The scalar's first loop: skip the trailing zeros above the top coefficient.
    let mut idx = highest_set(nz);

    while idx >= 0 {
        // Step past that coefficient, then measure the run of zeros below it.
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

/// See [`calculate_single_ctr_4x4_sse2_impl`].
#[inline]
pub fn calculate_single_ctr_4x4_sse2(dct: &[i16; 16]) -> i32 {
    unsafe { calculate_single_ctr_4x4_sse2_impl(dct) }
}

// ============================================================================
// Unit Tests
// ============================================================================

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
