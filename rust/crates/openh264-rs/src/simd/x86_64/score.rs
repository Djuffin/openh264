// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

//! x86_64 SSE2 CAVLC scoring kernel.
//!
//! `codec/encoder/core/x86/score.asm` holds four kernels; this file holds one.
//! `WelsGetNoneZeroCount_sse2` lives next to the quantizers it is read with, in
//! [`super::quant::get_none_zero_count`].
//!
//! `WelsScan4x4DcAc_sse2` and `WelsScan4x4Ac_sse2` have no counterpart here, because the
//! scalar is the faster kernel: `scan_4x4_dc_ac`'s table-driven loop compiles to 13
//! instructions — two loads, eight shuffles, two `pinsrw`, two stores, no branches —
//! against 21 for the asm. The asm reads its input with `movdqa`, so both halves arrive
//! lane-aligned and the four samples that cross between them have to be lifted out one at
//! a time with `pextrw`/`pinsrw`; given `movdqu`, LLVM instead issues the second load at a
//! 7-word offset, which lands those samples in the lanes they belong in.

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
#[inline(always)]
fn nonzero_mask(dct: &[i16; 16]) -> u32 {
    unsafe {
        let zero = _mm_setzero_si128();
        let ptr = dct.as_ptr() as *const __m128i;
        let v0 = _mm_loadu_si128(ptr);
        let v1 = _mm_loadu_si128(ptr.add(1));
        let packed = _mm_packs_epi16(v0, v1);
        let is_zero = _mm_cmpeq_epi8(packed, zero);
        (!(_mm_movemask_epi8(is_zero) as u32)) & 0xFFFF
    }
}

/// JVT-O079 CAVLC bit-cost estimate: for each run of zeros between non-zero
/// coefficients (scanning from the high end), add the run-length penalty.
///
/// Since `KI_TRUN_TABLE` is `[3, 2, 2, 1, 1, 1, 0, ...]`, each non-zero coefficient at
/// bit `p` of `nz` contributes `[run(p) <= 0] + [run(p) <= 2] + [run(p) <= 5]`, where
/// `run(p)` is the distance to the nearest preceding non-zero bit (or virtual bit `-1`).
///
/// C++: `WelsCalculateSingleCtr4x4_sse2`, `codec/encoder/core/x86/score.asm:263`.
#[inline(always)]
pub fn calculate_single_ctr_4x4(dct: &[i16; 16]) -> i32 {
    let nz = nonzero_mask(dct);
    let p0 = (nz << 1) | 1;
    let p2 = p0 | (p0 << 1) | (p0 << 2);
    let p5 = p2 | (p2 << 3);
    let c0 = (nz & p0) as u64;
    let c2 = (nz & p2) as u64;
    let c5 = (nz & p5) as u64;
    (c0 | (c2 << 16) | (c5 << 32)).count_ones() as i32
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_mb_aux::calculate_single_ctr_4x4 as scalar_calculate_single_ctr_4x4;

    /// `calculate_single_ctr_4x4` reads its input only through `== 0`, so its result is a
    /// function of the 16-bit non-zero mask alone — and all 65536 of them fit in a test.
    #[test]
    fn single_ctr_matches_the_scalar_for_every_mask() {
        for mask in 0u32..=0xFFFF {
            let dct: [i16; 16] = core::array::from_fn(|i| ((mask >> i) & 1) as i16);
            assert_eq!(
                calculate_single_ctr_4x4(&dct),
                scalar_calculate_single_ctr_4x4(&dct),
                "mask {mask:#06x}"
            );
        }
    }

    /// The mask is built through a **saturating** narrow, so a coefficient whose
    /// low byte is zero has to stay non-zero. `256` is the smallest such value
    /// and `-256` its mirror; a `packuswb`/truncating kernel reports 0 for both.
    #[test]
    fn single_ctr_sees_coefficients_a_truncating_narrow_would_lose() {
        for &v in &[256i16, -256, 512, i16::MIN, 0x0100, 0x7F00] {
            for pos in 0..16 {
                let mut dct = [0i16; 16];
                dct[pos] = v;
                assert_eq!(
                    calculate_single_ctr_4x4(&dct),
                    scalar_calculate_single_ctr_4x4(&dct),
                    "value {v} at {pos}"
                );
            }
        }
    }
}
