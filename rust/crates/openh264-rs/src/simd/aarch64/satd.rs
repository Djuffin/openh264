//! SATD — `WelsSampleSatd*_AArch64_neon`, `codec/encoder/core/arm64/pixel_aarch64_neon.S`.
//!
//! # Two kernels, not one
//!
//! `WelsSampleSatd4x4_AArch64_neon` holds the block as `[row0 | row1]` and
//! `[row2 | row3]`, butterflies vertically with a half swap, and finishes the
//! horizontal pass with `trn` pairs, `abs` and `saba` — the full `Σ|H·X·H|`, then
//! `(sum + 1) >> 1`.
//!
//! The wider shapes use the `SATD_8x4` / `SATD_16x4` macros, which never form the
//! horizontal transform's last stage at all. With `p = c0 + c1`, `q = c2 + c3`,
//! `r = c0 - c1`, `s = c2 - c3` the four outputs of a row are `p ± q` and `r ± s`, and
//! `|a + b| + |a - b| = 2 max(|a|, |b|)`, so the macro takes `smax` of the pairwise
//! sums and differences and the lane total is exactly half of `Σ|coeff|`. That half is
//! an integer — the identity makes every block's `Σ|coeff|` even — so it is also the
//! scalar's `(Σ|coeff| + 1) >> 1`, and the wider kernels need no rounding step.
//!
//! Lane bounds, for the `.8h` accumulation: a vertical output is at most `4 * 255`,
//! a pairwise sum at most 2040, and a four-row group contributes two `smax` vectors,
//! so a 16x16 block's eight groups peak at `8 * 2 * 2040 = 32640` per lane.
//!
//! The rows are read with `row_n`, as the x86_64 and `wide` kernels read them.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld16, ld8};
use crate::safe::plane::RefSamples;

/// Rows `0..4` of a 4-wide block as `([row0 | row1], [row2 | row3])`.
#[inline]
#[target_feature(enable = "neon")]
fn rows4x4<S: RefSamples>(c: &S) -> (uint8x8_t, uint8x8_t) {
    let mut a = [0u8; 16];
    for i in 0..4 {
        a[i * 4..][..4].copy_from_slice(&c.row_n::<4>(i as isize, 0));
    }
    (ld8(&a[..8]), ld8(&a[8..]))
}

/// `WelsSampleSatd4x4_AArch64_neon`.
#[inline]
#[target_feature(enable = "neon")]
fn satd_4x4_neon<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let (a01, a23) = rows4x4(c1);
    let (b01, b23) = rows4x4(c2);
    // usubl: {0,1,2,3,4,5,6,7} and {8,...,15} of the difference.
    let d01 = vreinterpretq_s16_u16(vsubl_u8(a01, b01));
    let d23 = vreinterpretq_s16_u16(vsubl_u8(a23, b23));

    // The vertical transform: `[r0+r2 | r1+r3]`, `[r0-r2 | r1-r3]`, then the halves
    // swapped so the second stage is lane-wise.
    let s = vaddq_s16(d01, d23);
    let d = vsubq_s16(d01, d23);
    let (s64, d64) = (vreinterpretq_s64_s16(s), vreinterpretq_s64_s16(d));
    let x = vreinterpretq_s16_s64(vzip1q_s64(s64, d64));
    let y = vreinterpretq_s16_s64(vzip2q_s64(s64, d64));
    let v4 = vaddq_s16(x, y);
    let v5 = vsubq_s16(x, y);

    // The horizontal transform.
    let (v4s, v5s) = (vreinterpretq_s32_s16(v4), vreinterpretq_s32_s16(v5));
    let t1 = vreinterpretq_s16_s32(vtrn1q_s32(v4s, v5s));
    let t2 = vreinterpretq_s16_s32(vtrn2q_s32(v4s, v5s));
    let v4 = vaddq_s16(t1, t2);
    let v5 = vsubq_s16(t1, t2);
    let u1 = vtrn1q_s16(v4, v5);
    let u2 = vtrn2q_s16(v4, v5);
    let acc = vabsq_s16(vaddq_s16(u1, u2));
    let acc = vabaq_s16(acc, u1, u2);
    let sum = vaddlvq_u16(vreinterpretq_u16_s16(acc)) as i32;
    (sum + 1) >> 1
}

/// The `SATD_8x4` macro's arithmetic on four rows of eight differences: the vertical
/// butterflies, then the `trn`/`abs`/`sabd`/`smax` stage described in the header.
/// Each lane of the result is `max(|p|, |q|)` or `max(|r|, |s|)` of one row and one
/// four-column half, summed over the macro's two `smax` vectors.
#[inline]
#[target_feature(enable = "neon")]
fn group8(d0: int16x8_t, d1: int16x8_t, d2: int16x8_t, d3: int16x8_t) -> int16x8_t {
    let v25 = vaddq_s16(d0, d1);
    let v26 = vsubq_s16(d0, d1);
    let v27 = vaddq_s16(d2, d3);
    let v28 = vsubq_s16(d2, d3);

    let v0 = vaddq_s16(v25, v27);
    let v1 = vsubq_s16(v25, v27);
    let v2 = vaddq_s16(v26, v28);
    let v3 = vsubq_s16(v26, v28);

    let v4 = vtrn1q_s16(v0, v1);
    let v5 = vtrn2q_s16(v0, v1);
    let v6 = vtrn1q_s16(v2, v3);
    let v7 = vtrn2q_s16(v2, v3);

    let v16 = vreinterpretq_s32_s16(vabsq_s16(vaddq_s16(v4, v5)));
    let v17 = vreinterpretq_s32_s16(vabdq_s16(v4, v5));
    let v18 = vreinterpretq_s32_s16(vabsq_s16(vaddq_s16(v6, v7)));
    let v19 = vreinterpretq_s32_s16(vabdq_s16(v6, v7));

    let v4 = vreinterpretq_s16_s32(vtrn1q_s32(v16, v17));
    let v5 = vreinterpretq_s16_s32(vtrn2q_s32(v16, v17));
    let v6 = vreinterpretq_s16_s32(vtrn1q_s32(v18, v19));
    let v7 = vreinterpretq_s16_s32(vtrn2q_s32(v18, v19));

    vaddq_s16(vmaxq_s16(v4, v5), vmaxq_s16(v6, v7))
}

/// `WelsSampleSatd8x8_AArch64_neon` and `8x16`, and the 8x4 shape upstream does
/// not have: `H / 4` groups of `SATD_8x4`, accumulated and reduced once.
#[inline]
#[target_feature(enable = "neon")]
fn satd_8w<A: RefSamples + Copy, B: RefSamples + Copy, const H: usize>(c1: &A, c2: &B) -> i32 {
    const { assert!(H % 4 == 0, "SATD groups are four rows tall") };
    let mut acc = vdupq_n_s16(0);
    for g in 0..H / 4 {
        let mut d = [vdupq_n_s16(0); 4];
        for (i, row) in d.iter_mut().enumerate() {
            let y = (4 * g + i) as isize;
            *row = vreinterpretq_s16_u16(vsubl_u8(ld8(&c1.row_n::<8>(y, 0)), ld8(&c2.row_n::<8>(y, 0))));
        }
        acc = vaddq_s16(acc, group8(d[0], d[1], d[2], d[3]));
    }
    vaddlvq_u16(vreinterpretq_u16_s16(acc)) as i32
}

/// `WelsSampleSatd16x16_AArch64_neon` and `16x8`: `SATD_16x4` is `SATD_8x4` on the
/// low and high halves of each row, and this is spelled that way.
#[inline]
#[target_feature(enable = "neon")]
fn satd_16w<A: RefSamples + Copy, B: RefSamples + Copy, const H: usize>(c1: &A, c2: &B) -> i32 {
    const { assert!(H % 4 == 0, "SATD groups are four rows tall") };
    let mut acc = vdupq_n_s16(0);
    for g in 0..H / 4 {
        let mut lo = [vdupq_n_s16(0); 4];
        let mut hi = [vdupq_n_s16(0); 4];
        for i in 0..4 {
            let y = (4 * g + i) as isize;
            let a = ld16(&c1.row_n::<16>(y, 0));
            let b = ld16(&c2.row_n::<16>(y, 0));
            lo[i] = vreinterpretq_s16_u16(vsubl_u8(vget_low_u8(a), vget_low_u8(b)));
            hi[i] = vreinterpretq_s16_u16(vsubl_high_u8(a, b));
        }
        acc = vaddq_s16(acc, group8(lo[0], lo[1], lo[2], lo[3]));
        acc = vaddq_s16(acc, group8(hi[0], hi[1], hi[2], hi[3]));
    }
    vaddlvq_u16(vreinterpretq_u16_s16(acc)) as i32
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

/// `WelsSampleSatd4x4_AArch64_neon`.
#[inline]
pub fn satd_4x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { satd_4x4_neon(c1, c2) }
}

/// No upstream kernel: one `SATD_8x4` group.
#[inline]
pub fn satd_8x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    unsafe { satd_8w::<A, B, 4>(c1, c2) }
}

/// No upstream kernel: two 4x4s top-to-bottom, in the scalar's order.
#[inline]
pub fn satd_4x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4(c1, c2) + satd_4x4(&c1.advance(0, 4), &c2.advance(0, 4))
}

/// `WelsSampleSatd8x8_AArch64_neon`.
#[inline]
pub fn satd_8x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    unsafe { satd_8w::<A, B, 8>(c1, c2) }
}

/// `WelsSampleSatd16x8_AArch64_neon`.
#[inline]
pub fn satd_16x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    unsafe { satd_16w::<A, B, 8>(c1, c2) }
}

/// `WelsSampleSatd8x16_AArch64_neon`.
#[inline]
pub fn satd_8x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    unsafe { satd_8w::<A, B, 16>(c1, c2) }
}

/// `WelsSampleSatd16x16_AArch64_neon`.
#[inline]
pub fn satd_16x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    unsafe { satd_16w::<A, B, 16>(c1, c2) }
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::sample as scalar;
    use crate::safe::plane::PlaneCursor;

    fn make_test_planes(stride: usize, height: usize) -> (Vec<u8>, Vec<u8>) {
        let mut p1 = vec![0u8; stride * height];
        let mut p2 = vec![0u8; stride * height];
        for (i, b) in p1.iter_mut().enumerate() {
            *b = ((i * 19 + 7) & 0xFF) as u8;
        }
        for (i, b) in p2.iter_mut().enumerate() {
            *b = ((i * 29 + 23) & 0xFF) as u8;
        }
        (p1, p2)
    }

    #[test]
    fn test_satd_parity_identical() {
        let buf = [42u8; 64 * 16];
        let c = PlaneCursor::new(&buf, 0, 64);
        assert_eq!(satd_4x4(&c, &c), 0);
        assert_eq!(satd_8x8(&c, &c), 0);
        assert_eq!(satd_16x16(&c, &c), 0);
    }

    #[test]
    fn test_satd_parity_all_shapes() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(satd_4x4(&c1, &c2), scalar::satd_4x4(&c1, &c2), "satd_4x4 mismatch");
        assert_eq!(satd_8x4(&c1, &c2), scalar::satd_8x4(&c1, &c2), "satd_8x4 mismatch");
        assert_eq!(satd_4x8(&c1, &c2), scalar::satd_4x8(&c1, &c2), "satd_4x8 mismatch");
        assert_eq!(satd_8x8(&c1, &c2), scalar::satd_8x8(&c1, &c2), "satd_8x8 mismatch");
        assert_eq!(satd_16x8(&c1, &c2), scalar::satd_16x8(&c1, &c2), "satd_16x8 mismatch");
        assert_eq!(satd_8x16(&c1, &c2), scalar::satd_8x16(&c1, &c2), "satd_8x16 mismatch");
        assert_eq!(satd_16x16(&c1, &c2), scalar::satd_16x16(&c1, &c2), "satd_16x16 mismatch");
    }

    #[test]
    fn test_satd_random_blocks() {
        for seed in 0..100 {
            let mut p1 = [0u8; 16];
            let mut p2 = [0u8; 16];
            for i in 0..16 {
                p1[i] = ((seed * 37 + i * 11) & 0xFF) as u8;
                p2[i] = ((seed * 43 + i * 17) & 0xFF) as u8;
            }
            let c1 = PlaneCursor::new(&p1, 0, 4);
            let c2 = PlaneCursor::new(&p2, 0, 4);
            assert_eq!(satd_4x4(&c1, &c2), scalar::satd_4x4(&c1, &c2), "mismatch at seed {seed}");
        }
    }

    /// A 64-bit LCG, so a failing seed is replayable.
    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32) as u8
    }

    /// The wide kernels take a different route to the answer than the scalar — the
    /// `smax` identity, and one `.8h` accumulator for a whole 16x16 — so drive every
    /// shape over noise, ramps, and the two extremes that fill the lanes: all-`0xFF`
    /// against all-`0x00` is the largest `Σ|coeff|` a block can have, and it is where
    /// a lane that summed past `i16` would show.
    #[test]
    fn satd_parity_over_anchors_and_distributions() {
        let n = 64 * 64;
        let mut seed = 0x5DEECE66Du64;
        let noise1: Vec<u8> = (0..n).map(|_| lcg(&mut seed)).collect();
        let noise2: Vec<u8> = (0..n).map(|_| lcg(&mut seed)).collect();
        let (ramp1, ramp2) = make_test_planes(64, 64);
        let pairs = [
            ("ramps", ramp1, ramp2),
            ("noise", noise1, noise2),
            ("max-diff", vec![0xFFu8; n], vec![0x00u8; n]),
            ("identical", vec![0x5Au8; n], vec![0x5Au8; n]),
        ];
        for (name, p1, p2) in &pairs {
            for anchor in [64 * 16 + 16, 64 * 17 + 19, 64 * 18 + 22, 64 * 19 + 21] {
                let c1 = PlaneCursor::new(p1, anchor, 64);
                let c2 = PlaneCursor::new(p2, anchor, 64);
                let at = format!("{name} @ {anchor}");
                assert_eq!(satd_4x4(&c1, &c2), scalar::satd_4x4(&c1, &c2), "4x4 {at}");
                assert_eq!(satd_8x4(&c1, &c2), scalar::satd_8x4(&c1, &c2), "8x4 {at}");
                assert_eq!(satd_4x8(&c1, &c2), scalar::satd_4x8(&c1, &c2), "4x8 {at}");
                assert_eq!(satd_8x8(&c1, &c2), scalar::satd_8x8(&c1, &c2), "8x8 {at}");
                assert_eq!(satd_16x8(&c1, &c2), scalar::satd_16x8(&c1, &c2), "16x8 {at}");
                assert_eq!(satd_8x16(&c1, &c2), scalar::satd_8x16(&c1, &c2), "8x16 {at}");
                assert_eq!(satd_16x16(&c1, &c2), scalar::satd_16x16(&c1, &c2), "16x16 {at}");
            }
        }
    }
}
