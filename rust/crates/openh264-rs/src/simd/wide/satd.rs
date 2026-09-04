//! SATD on `wide` lane types — the twin of `simd::x86_64::satd`.
//!
//! # A different layout, for a reason
//!
//! The intrinsic kernel keeps one row per register in the low four lanes, runs the
//! row butterflies lane-wise, transposes with four `punpck`s, and butterflies again.
//! `wide` has no word-lane unpack, so the transpose would be an array cast for LLVM
//! to lower — and while [`super::lanes::transpose4_lo`] exists for the kernels that
//! need it, this one does not: the 4x4 block fits two registers as `[row0 | row1]`
//! and `[row2 | row3]`, and with that layout the vertical butterflies need only a
//! half swap and the horizontal ones a quad rotate and an adjacent swap — all
//! `pshufd`-class permutes.
//!
//! The horizontal pass produces each of its four outputs twice (once per half of a
//! lane pair, with a sign that the absolute value erases), so the lane sum is twice
//! the transform's `Σ|coeff|` and the kernel halves it before the final rounding.

#![forbid(unsafe_code)]

use wide::{i16x8, u8x16};

use super::lanes::{hsum_i16, rotate_quads, swap_adjacent, swap_halves, widen_hi, widen_lo, HIGH_HALF, QUAD_HIGH_PAIR};
use crate::safe::plane::RefSamples;

/// The horizontal Hadamard of two rows held as `[row_a | row_b]`, returned as a
/// vector whose lanes sum to `2 * (Σ|H(row_a)| + Σ|H(row_b)|)`.
///
/// Per four-lane row `[m0 m1 m2 m3]`: `x = [h0 h1 h0 h1]` and `y = [h2 h3 -h2 -h3]`
/// with `h0 = m0 + m2, h1 = m1 + m3, h2 = m0 - m2, h3 = m1 - m3`; blending the pairs
/// gives `z = [h0 h1 -h2 -h3]`, and `z ± swap_adjacent(z)` are the four outputs, each
/// twice up to sign.
#[inline(always)]
fn hpass_abs(p: i16x8) -> i16x8 {
    let r = rotate_quads(p);
    let x = p + r;
    let y = p - r;
    let z = QUAD_HIGH_PAIR.blend(y, x);
    let zs = swap_adjacent(z);
    (z + zs).abs() + (z - zs).abs()
}

#[inline(always)]
fn satd_4x4_impl<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut a = [0u8; 16];
    let mut b = [0u8; 16];
    for i in 0..4 {
        a[i * 4..][..4].copy_from_slice(&c1.row_n::<4>(i as isize, 0));
        b[i * 4..][..4].copy_from_slice(&c2.row_n::<4>(i as isize, 0));
    }
    let (va, vb) = (u8x16::new(a), u8x16::new(b));
    let lo = widen_lo(va) - widen_lo(vb); // [row0 | row1]
    let hi = widen_hi(va) - widen_hi(vb); // [row2 | row3]

    // Vertical butterflies. `p = [s0 | s1]`, `m = [s2 | s3]` with
    // `s0 = r0 + r2, s1 = r1 + r3, s2 = r0 - r2, s3 = r1 - r3`.
    let p = lo + hi;
    let m = lo - hi;
    let (ps, ms) = (swap_halves(p), swap_halves(m));
    let t = p + ps; // [s0 + s1 | same]
    let u = p - ps; // [s0 - s1 | negated]
    let v = m + ms; // [s2 + s3 | same]
    let w = m - ms; // [s2 - s3 | negated]

    // The intermediate rows, two per register, in the scalar's order.
    let rows01 = HIGH_HALF.blend(v, t); // [s0 + s1 | s2 + s3]
    let rows23 = HIGH_HALF.blend(u, w); // [s2 - s3 | s0 - s1]

    // Lanes peak at 4 * 4080 = 16320, inside `i16`; the sum is taken in `i32`.
    let doubled = hsum_i16(hpass_abs(rows01) + hpass_abs(rows23));
    let satd = doubled >> 1i32;
    (satd + 1) >> 1i32
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

#[inline(always)]
pub fn satd_4x4_sse2<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4_impl(c1, c2)
}

#[inline(always)]
pub fn satd_8x4_sse2<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4_sse2(c1, c2) + satd_4x4_sse2(&c1.advance(4, 0), &c2.advance(4, 0))
}

#[inline(always)]
pub fn satd_4x8_sse2<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4_sse2(c1, c2) + satd_4x4_sse2(&c1.advance(0, 4), &c2.advance(0, 4))
}

#[inline(always)]
pub fn satd_8x8_sse2<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut satd = satd_4x4_sse2(c1, c2);
    satd += satd_4x4_sse2(&c1.advance(4, 0), &c2.advance(4, 0));
    satd += satd_4x4_sse2(&c1.advance(0, 4), &c2.advance(0, 4));
    satd += satd_4x4_sse2(&c1.advance(4, 4), &c2.advance(4, 4));
    satd
}

#[inline(always)]
pub fn satd_16x8_sse2<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_8x8_sse2(c1, c2) + satd_8x8_sse2(&c1.advance(8, 0), &c2.advance(8, 0))
}

#[inline(always)]
pub fn satd_8x16_sse2<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_8x8_sse2(c1, c2) + satd_8x8_sse2(&c1.advance(0, 8), &c2.advance(0, 8))
}

#[inline(always)]
pub fn satd_16x16_sse2<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut satd = satd_8x8_sse2(c1, c2);
    satd += satd_8x8_sse2(&c1.advance(8, 0), &c2.advance(8, 0));
    satd += satd_8x8_sse2(&c1.advance(0, 8), &c2.advance(0, 8));
    satd += satd_8x8_sse2(&c1.advance(8, 8), &c2.advance(8, 8));
    satd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::sample::{
        satd_16x16, satd_16x8, satd_4x4, satd_4x8, satd_8x16, satd_8x4, satd_8x8,
    };
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
        let buf = [42u8; 64];
        let c = PlaneCursor::new(&buf, 0, 8);
        assert_eq!(satd_4x4_sse2(&c, &c), 0);
        assert_eq!(satd_8x8_sse2(&c, &c), 0);
        assert_eq!(satd_4x4_sse2(&c, &c), satd_4x4(&c, &c));
    }

    #[test]
    fn test_satd_parity_all_shapes() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(satd_4x4_sse2(&c1, &c2), satd_4x4(&c1, &c2), "satd_4x4 mismatch");
        assert_eq!(satd_8x4_sse2(&c1, &c2), satd_8x4(&c1, &c2), "satd_8x4 mismatch");
        assert_eq!(satd_4x8_sse2(&c1, &c2), satd_4x8(&c1, &c2), "satd_4x8 mismatch");
        assert_eq!(satd_8x8_sse2(&c1, &c2), satd_8x8(&c1, &c2), "satd_8x8 mismatch");
        assert_eq!(satd_16x8_sse2(&c1, &c2), satd_16x8(&c1, &c2), "satd_16x8 mismatch");
        assert_eq!(satd_8x16_sse2(&c1, &c2), satd_8x16(&c1, &c2), "satd_8x16 mismatch");
        assert_eq!(satd_16x16_sse2(&c1, &c2), satd_16x16(&c1, &c2), "satd_16x16 mismatch");
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
            assert_eq!(
                satd_4x4_sse2(&c1, &c2),
                satd_4x4(&c1, &c2),
                "mismatch at seed {seed}"
            );
        }
    }
}
