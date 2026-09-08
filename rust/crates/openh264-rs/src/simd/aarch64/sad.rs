//! SAD and four-point SAD — `WelsSampleSad*_AArch64_neon` and
//! `WelsSampleSadFour*_AArch64_neon`, `codec/encoder/core/arm64/pixel_aarch64_neon.S`.
//!
//! The asm's shape, per row: `uabdl`/`uabal` the first eight bytes into a `.8h`
//! accumulator and `uabal2` the second eight into the same one, then `saddlv` the
//! eight lanes at the end. One accumulator takes a whole block: a lane gains at most
//! `2 * 255` per row over at most 16 rows, 8160, inside `u16`.
//!
//! The four-point kernels walk `sample2`'s rows once for the up and down probes —
//! the asm loads `H + 2` rows and reads them twice, at offsets 0 and 2 — and once
//! each for the left and right probes.
//!
//! Upstream has no 8x4 or 4x8 kernel; the port fills those slots with the same row
//! loop at those heights, as the x86_64 set does. Nor is there a second, wider tier
//! on aarch64: `has_avx2()` is false on every aarch64 build, so the `_avx2` slots
//! are never installed and forward to the baseline kernel to keep the alias total.
//!
//! # Where this departs from the asm
//!
//! The asm runs one accumulator per block. On an out-of-order core that makes a
//! sixteen-row block a chain of thirty-two dependent `uabal`s, and it measured at
//! half the speed of the scalar loop LLVM vectorises for itself. The loops here keep
//! two rows in flight — four accumulators for the sixteen-wide shapes, two for the
//! narrower ones — and add them once at the end; the lane bound is the same 8160.
//!
//! # How the rows are addressed
//!
//! Each operand is cut once into a `RefSamples::span` — a slice the compiler knows to
//! be `(H - 1) * stride + W` long — and the rows are indexed inside it. That leaves
//! **one cut per operand per call**, two compare-and-branch pairs and both before the
//! loop, where a `row_n` walk left two per row: the 16x16 SAD emitted 64 such pairs
//! around its 32 `uabal`s and now emits none at all inside the loop. The rows are in
//! range by construction and LLVM proves it from the span's length.
//!
//! The four-point kernels cut `sample2` `W + 2` wide and `H + 2` tall — the reach of
//! all four probes — and read the probes at span columns 0, 1 and 2, which is where
//! the asm reads them. Their row loops take that block a `G`-row window at a time,
//! because only a *constant* row offset inside a span folds and a loop LLVM does not
//! unroll has no constant offsets; see `sad_four_16x`.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld16, ld4, ld8};
use crate::safe::plane::{BlockRows, RefSamples};

// ============================================================================
// Row loops
// ============================================================================

/// Two rows per step into four accumulators; see the header.
#[inline]
#[target_feature(enable = "neon")]
fn sad_16x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, dx: isize, dy: isize) -> i32 {
    const { assert!(H % 2 == 0, "sad_16x steps two rows; H must be even") };
    let (s1, s2) = (sample1.span::<16, H>(0, 0), sample2.span::<16, H>(dy, dx));
    let mut acc = [vdupq_n_u16(0); 4];
    let mut y = 0usize;
    while y < H {
        let (a, b) = (ld16(&s1.row::<16>(y, 0)), ld16(&s2.row::<16>(y, 0)));
        acc[0] = vabal_u8(acc[0], vget_low_u8(a), vget_low_u8(b));
        acc[1] = vabal_high_u8(acc[1], a, b);
        let (a, b) = (ld16(&s1.row::<16>(y + 1, 0)), ld16(&s2.row::<16>(y + 1, 0)));
        acc[2] = vabal_u8(acc[2], vget_low_u8(a), vget_low_u8(b));
        acc[3] = vabal_high_u8(acc[3], a, b);
        y += 2;
    }
    vaddlvq_u16(vaddq_u16(vaddq_u16(acc[0], acc[1]), vaddq_u16(acc[2], acc[3]))) as i32
}

/// Two rows per step into two accumulators.
#[inline]
#[target_feature(enable = "neon")]
fn sad_8x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, dx: isize, dy: isize) -> i32 {
    const { assert!(H % 2 == 0, "sad_8x steps two rows; H must be even") };
    let (s1, s2) = (sample1.span::<8, H>(0, 0), sample2.span::<8, H>(dy, dx));
    let mut acc = [vdupq_n_u16(0); 2];
    let mut y = 0usize;
    while y < H {
        acc[0] = vabal_u8(acc[0], ld8(&s1.row::<8>(y, 0)), ld8(&s2.row::<8>(y, 0)));
        acc[1] = vabal_u8(acc[1], ld8(&s1.row::<8>(y + 1, 0)), ld8(&s2.row::<8>(y + 1, 0)));
        y += 2;
    }
    vaddlvq_u16(vaddq_u16(acc[0], acc[1])) as i32
}

/// The asm loads each row into `.s[0]` and reduces over `.4h`; `ld4` zeroes the
/// upper lanes instead, so the full-width reduce is exact.
#[inline]
#[target_feature(enable = "neon")]
fn sad_4x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, dx: isize, dy: isize) -> i32 {
    const { assert!(H % 2 == 0, "sad_4x steps two rows; H must be even") };
    let (s1, s2) = (sample1.span::<4, H>(0, 0), sample2.span::<4, H>(dy, dx));
    let mut acc = [vdupq_n_u16(0); 2];
    let mut y = 0usize;
    while y < H {
        acc[0] = vabal_u8(acc[0], ld4(&s1.row::<4>(y, 0)), ld4(&s2.row::<4>(y, 0)));
        acc[1] = vabal_u8(acc[1], ld4(&s1.row::<4>(y + 1, 0)), ld4(&s2.row::<4>(y + 1, 0)));
        y += 2;
    }
    vaddlvq_u16(vaddq_u16(acc[0], acc[1])) as i32
}

/// The four whole-sample neighbours — up, down, left, right — two accumulators
/// each (low and high halves), so no chain is longer than the block is tall.
///
/// # The probe span
///
/// All four probes of a row live in one `W + 2` by `G + 2` window of `sample2` cut at
/// `(-1, -1)`: the up and down probes are span rows `j` and `j + 2` at column 1, the
/// left and right are span row `j + 1` at columns 0 and 2. That is exactly what the
/// asm's `LOAD_8X8_2` plus two extra rows, read at offsets 0 and 2, does.
///
/// # `G`: how many rows one cut covers
///
/// Only a **constant** row offset inside a span folds; a symbolic `y * stride` is
/// something LLVM cannot place inside the span, and the per-row checks would stay.
/// So a cut covers `G` rows, the rows inside are read at constant offsets, and their
/// checks all fold into that one cut. `G` has to be small enough that the `G`-row
/// walk unrolls — that is what makes those offsets constant — so it is `H` for the
/// shapes that unroll whole and a divisor of `H` for the ones that do not.
///
/// The two block spans are cut **once**, outside the loop, and the groups are
/// [`BlockRows::window`]s of them: cutting from the cursor per group would put the
/// stride validation inside the loop, which measured 1.5x slower on the 16x16 shape.
/// `HW` is `H + 2` and is a separate parameter only because stable Rust cannot
/// compute `H + 2` in a const-argument position.
#[inline]
#[target_feature(enable = "neon")]
fn sad_four_16x<S: RefSamples, const H: usize, const HW: usize, const G: usize>(
    sample1: &S,
    sample2: &S,
    sad: &mut [i32; 4],
) {
    const { assert!(G % 2 == 0 && H % G == 0, "the block is a whole number of G-row cuts") };
    const { assert!(HW == H + 2, "the probe span is two rows taller than the block") };
    let mut acc = [[vdupq_n_u16(0); 2]; 4];
    let s1 = sample1.span::<16, H>(0, 0);
    let s2 = sample2.span::<18, HW>(-1, -1);
    let mut y = 0usize;
    while y < H {
        let s1 = s1.window::<16>(y, G);
        let s2 = s2.window::<18>(y, G + 2);
        for j in 0..G {
            let a = ld16(&s1.row::<16>(j, 0));
            let probes = [
                ld16(&s2.row::<16>(j, 1)),
                ld16(&s2.row::<16>(j + 2, 1)),
                ld16(&s2.row::<16>(j + 1, 0)),
                ld16(&s2.row::<16>(j + 1, 2)),
            ];
            for (k, p) in probes.into_iter().enumerate() {
                acc[k][0] = vabal_u8(acc[k][0], vget_low_u8(a), vget_low_u8(p));
                acc[k][1] = vabal_high_u8(acc[k][1], a, p);
            }
        }
        y += G;
    }
    for k in 0..4 {
        sad[k] = vaddlvq_u16(vaddq_u16(acc[k][0], acc[k][1])) as i32;
    }
}

/// See [`sad_four_16x`] for the probe span and for what `G` and `GW` select.
///
/// The two accumulators are the row parity here, not the byte halves, so the inner
/// pair walk is written out: the accumulator index has to be a constant for the
/// array to stay in registers.
#[inline]
#[target_feature(enable = "neon")]
fn sad_four_8x<S: RefSamples, const H: usize, const HW: usize, const G: usize>(
    sample1: &S,
    sample2: &S,
    sad: &mut [i32; 4],
) {
    const { assert!(G % 2 == 0 && H % G == 0, "the block is a whole number of G-row cuts") };
    const { assert!(HW == H + 2, "the probe span is two rows taller than the block") };
    let mut acc = [[vdupq_n_u16(0); 2]; 4];
    let s1 = sample1.span::<8, H>(0, 0);
    let s2 = sample2.span::<10, HW>(-1, -1);
    let mut y = 0usize;
    while y < H {
        let s1 = s1.window::<8>(y, G);
        let s2 = s2.window::<10>(y, G + 2);
        let mut i = 0usize;
        while i < G {
            for j in 0..2usize {
                let yy = i + j;
                let a = ld8(&s1.row::<8>(yy, 0));
                let probes = [
                    ld8(&s2.row::<8>(yy, 1)),
                    ld8(&s2.row::<8>(yy + 2, 1)),
                    ld8(&s2.row::<8>(yy + 1, 0)),
                    ld8(&s2.row::<8>(yy + 1, 2)),
                ];
                for (k, p) in probes.into_iter().enumerate() {
                    acc[k][j] = vabal_u8(acc[k][j], a, p);
                }
            }
            i += 2;
        }
        y += G;
    }
    for k in 0..4 {
        sad[k] = vaddlvq_u16(vaddq_u16(acc[k][0], acc[k][1])) as i32;
    }
}

/// See [`sad_four_16x`] for the probe span and for what `G` and `GW` select.
///
/// The two accumulators are the row parity here, not the byte halves, so the inner
/// pair walk is written out: the accumulator index has to be a constant for the
/// array to stay in registers.
#[inline]
#[target_feature(enable = "neon")]
fn sad_four_4x<S: RefSamples, const H: usize, const HW: usize, const G: usize>(
    sample1: &S,
    sample2: &S,
    sad: &mut [i32; 4],
) {
    const { assert!(G % 2 == 0 && H % G == 0, "the block is a whole number of G-row cuts") };
    const { assert!(HW == H + 2, "the probe span is two rows taller than the block") };
    let mut acc = [[vdupq_n_u16(0); 2]; 4];
    let s1 = sample1.span::<4, H>(0, 0);
    let s2 = sample2.span::<6, HW>(-1, -1);
    let mut y = 0usize;
    while y < H {
        let s1 = s1.window::<4>(y, G);
        let s2 = s2.window::<6>(y, G + 2);
        let mut i = 0usize;
        while i < G {
            for j in 0..2usize {
                let yy = i + j;
                let a = ld4(&s1.row::<4>(yy, 0));
                let probes = [
                    ld4(&s2.row::<4>(yy, 1)),
                    ld4(&s2.row::<4>(yy + 2, 1)),
                    ld4(&s2.row::<4>(yy + 1, 0)),
                    ld4(&s2.row::<4>(yy + 1, 2)),
                ];
                for (k, p) in probes.into_iter().enumerate() {
                    acc[k][j] = vabal_u8(acc[k][j], a, p);
                }
            }
            i += 2;
        }
        y += G;
    }
    for k in 0..4 {
        sad[k] = vaddlvq_u16(vaddq_u16(acc[k][0], acc[k][1])) as i32;
    }
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

/// `WelsSampleSad16x16_AArch64_neon`.
#[inline]
pub fn sample_sad_16x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { sad_16x::<S, 16>(sample1, sample2, 0, 0) }
}

/// The AVX2 slot's kernel — never installed on aarch64; see the module header.
#[inline]
pub(crate) fn sample_sad_16x16_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sample_sad_16x16(sample1, sample2)
}

/// `WelsSampleSad16x8_AArch64_neon`.
#[inline]
pub fn sample_sad_16x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x::<S, 8>(sample1, sample2, 0, 0) }
}

/// See [`sample_sad_16x16_avx2`].
#[inline]
pub(crate) fn sample_sad_16x8_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sample_sad_16x8(sample1, sample2)
}

/// `WelsSampleSad8x16_AArch64_neon`.
#[inline]
pub fn sample_sad_8x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x::<S, 16>(sample1, sample2, 0, 0) }
}

/// `WelsSampleSad8x8_AArch64_neon`.
#[inline]
pub fn sample_sad_8x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x::<S, 8>(sample1, sample2, 0, 0) }
}

/// `WelsSampleSad4x4_AArch64_neon`.
#[inline]
pub fn sample_sad_4x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_4x::<S, 4>(sample1, sample2, 0, 0) }
}

/// No upstream kernel; the 8-wide loop at height 4.
#[inline]
pub fn sample_sad_8x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x::<S, 4>(sample1, sample2, 0, 0) }
}

/// No upstream kernel; the 4-wide loop at height 8.
#[inline]
pub fn sample_sad_4x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_4x::<S, 8>(sample1, sample2, 0, 0) }
}

/// `WelsSampleSadFour16x16_AArch64_neon`.
#[inline]
pub fn sample_sad_four_16x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sad_four_16x::<S, 16, 18, 4>(sample1, sample2, sad) }
}

/// `WelsSampleSadFour16x8_AArch64_neon`.
#[inline]
pub fn sample_sad_four_16x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sad_four_16x::<S, 8, 10, 4>(sample1, sample2, sad) }
}

/// `WelsSampleSadFour8x16_AArch64_neon`.
#[inline]
pub fn sample_sad_four_8x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sad_four_8x::<S, 16, 18, 8>(sample1, sample2, sad) }
}

/// `WelsSampleSadFour8x8_AArch64_neon`.
#[inline]
pub fn sample_sad_four_8x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sad_four_8x::<S, 8, 10, 8>(sample1, sample2, sad) }
}

/// `WelsSampleSadFour4x4_AArch64_neon`.
#[inline]
pub fn sample_sad_four_4x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sad_four_4x::<S, 4, 6, 4>(sample1, sample2, sad) }
}

/// No upstream kernel; the 8-wide loop at height 4.
#[inline]
pub fn sample_sad_four_8x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sad_four_8x::<S, 4, 6, 4>(sample1, sample2, sad) }
}

/// No upstream kernel; the 4-wide loop at height 8.
#[inline]
pub fn sample_sad_four_4x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sad_four_4x::<S, 8, 10, 8>(sample1, sample2, sad) }
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::sad_common::{sample_sad, sample_sad_four};
    use crate::encoder::rec_view::RecCursor;
    use crate::safe::plane::PlaneCursor;

    fn make_test_planes(stride: usize, height: usize) -> (Vec<u8>, Vec<u8>) {
        let mut p1 = vec![0u8; stride * height];
        let mut p2 = vec![0u8; stride * height];
        for (i, b) in p1.iter_mut().enumerate() {
            *b = ((i * 17 + 5) & 0xFF) as u8;
        }
        for (i, b) in p2.iter_mut().enumerate() {
            *b = ((i * 31 + 13) & 0xFF) as u8;
        }
        (p1, p2)
    }

    #[test]
    fn test_sad_parity_all_shapes() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(sample_sad_16x16(&c1, &c2), sample_sad::<16, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_16x8(&c1, &c2), sample_sad::<16, 8, _>(&c1, &c2));
        assert_eq!(sample_sad_8x16(&c1, &c2), sample_sad::<8, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_8x8(&c1, &c2), sample_sad::<8, 8, _>(&c1, &c2));
        assert_eq!(sample_sad_4x4(&c1, &c2), sample_sad::<4, 4, _>(&c1, &c2));
        assert_eq!(sample_sad_8x4(&c1, &c2), sample_sad::<8, 4, _>(&c1, &c2));
        assert_eq!(sample_sad_4x8(&c1, &c2), sample_sad::<4, 8, _>(&c1, &c2));
    }

    /// The `_avx2` pair is the baseline kernel under another name here, and runs
    /// everywhere: there is no instruction to test for on this target.
    #[test]
    fn test_avx2_sad_parity() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(sample_sad_16x16_avx2(&c1, &c2), sample_sad::<16, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_16x8_avx2(&c1, &c2), sample_sad::<16, 8, _>(&c1, &c2));
    }

    #[test]
    fn test_sample_sad_four_parity() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 16 + 16, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 16 + 16, 64);

        let mut expected = [0i32; 4];
        let mut actual = [0i32; 4];

        sample_sad_four::<16, 16, _>(&c1, &c2, &mut expected);
        sample_sad_four_16x16(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "16x16 four-point SAD mismatch");

        sample_sad_four::<16, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_16x8(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "16x8 four-point SAD mismatch");

        sample_sad_four::<8, 16, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x16(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x16 four-point SAD mismatch");

        sample_sad_four::<8, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x8(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x8 four-point SAD mismatch");

        sample_sad_four::<4, 4, _>(&c1, &c2, &mut expected);
        sample_sad_four_4x4(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "4x4 four-point SAD mismatch");

        sample_sad_four::<8, 4, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x4(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x4 four-point SAD mismatch");

        sample_sad_four::<4, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_4x8(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "4x8 four-point SAD mismatch");
    }

    // ========================================================================
    // Input and anchor coverage.
    //
    // The sweep below runs every kernel over four anchors, one per residue class mod 8,
    // and five distributions. The all-`0xFF`/all-`0x00` pair and the near-identical
    // pair are the ends of the accumulator's range, where a lane that widened or
    // saturated wrongly would show.
    // ========================================================================

    /// A 64-bit LCG, so a failing seed is replayable.
    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32) as u8
    }

    /// The five input distributions, each a pair of planes of `stride * height` bytes.
    fn input_pairs(stride: usize, height: usize) -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
        let n = stride * height;
        let mut seed = 0x5DEECE66Du64;
        let noise1: Vec<u8> = (0..n).map(|_| lcg(&mut seed)).collect();
        let noise2: Vec<u8> = (0..n).map(|_| lcg(&mut seed)).collect();

        let mut near = noise1.clone();
        // A handful of differing bytes: the accumulator spends most of its range at 0.
        for (i, b) in near.iter_mut().enumerate() {
            if i % 97 == 0 {
                *b = b.wrapping_add(1);
            }
        }

        let (ramp1, ramp2) = make_test_planes(stride, height);
        vec![
            ("ramps", ramp1, ramp2),
            ("noise", noise1.clone(), noise2),
            ("max-diff", vec![0xFFu8; n], vec![0x00u8; n]),
            ("near-identical", noise1, near),
            ("identical", vec![0x5Au8; n], vec![0x5Au8; n]),
        ]
    }

    /// Four anchors covering every residue mod 8, so the aligned case is not the only
    /// one tested. Each leaves at least 16 rows and 16 columns of margin on all sides.
    const ANCHORS: [usize; 4] = [64 * 16 + 16, 64 * 17 + 19, 64 * 18 + 22, 64 * 19 + 21];

    #[test]
    fn sad_parity_over_anchors_and_distributions() {
        for (name, p1, p2) in input_pairs(64, 64) {
            for anchor in ANCHORS {
                let c1 = PlaneCursor::new(&p1, anchor, 64);
                let c2 = PlaneCursor::new(&p2, anchor, 64);
                let at = format!("{name} @ {anchor}");

                assert_eq!(sample_sad_16x16(&c1, &c2), sample_sad::<16, 16, _>(&c1, &c2), "16x16 {at}");
                assert_eq!(sample_sad_16x8(&c1, &c2), sample_sad::<16, 8, _>(&c1, &c2), "16x8 {at}");
                assert_eq!(sample_sad_8x16(&c1, &c2), sample_sad::<8, 16, _>(&c1, &c2), "8x16 {at}");
                assert_eq!(sample_sad_8x8(&c1, &c2), sample_sad::<8, 8, _>(&c1, &c2), "8x8 {at}");
                assert_eq!(sample_sad_4x4(&c1, &c2), sample_sad::<4, 4, _>(&c1, &c2), "4x4 {at}");
                assert_eq!(sample_sad_8x4(&c1, &c2), sample_sad::<8, 4, _>(&c1, &c2), "8x4 {at}");
                assert_eq!(sample_sad_4x8(&c1, &c2), sample_sad::<4, 8, _>(&c1, &c2), "4x8 {at}");
            }
        }
    }

    #[test]
    fn sample_sad_four_parity_over_anchors_and_distributions() {
        for (name, p1, p2) in input_pairs(64, 64) {
            for anchor in ANCHORS {
                let c1 = PlaneCursor::new(&p1, anchor, 64);
                let c2 = PlaneCursor::new(&p2, anchor, 64);
                let at = format!("{name} @ {anchor}");
                let (mut want, mut got) = ([0i32; 4], [0i32; 4]);

                sample_sad_four::<16, 16, _>(&c1, &c2, &mut want);
                sample_sad_four_16x16(&c1, &c2, &mut got);
                assert_eq!(got, want, "16x16 four-point {at}");

                sample_sad_four::<16, 8, _>(&c1, &c2, &mut want);
                sample_sad_four_16x8(&c1, &c2, &mut got);
                assert_eq!(got, want, "16x8 four-point {at}");

                sample_sad_four::<8, 16, _>(&c1, &c2, &mut want);
                sample_sad_four_8x16(&c1, &c2, &mut got);
                assert_eq!(got, want, "8x16 four-point {at}");

                sample_sad_four::<8, 8, _>(&c1, &c2, &mut want);
                sample_sad_four_8x8(&c1, &c2, &mut got);
                assert_eq!(got, want, "8x8 four-point {at}");

                sample_sad_four::<4, 4, _>(&c1, &c2, &mut want);
                sample_sad_four_4x4(&c1, &c2, &mut got);
                assert_eq!(got, want, "4x4 four-point {at}");

                sample_sad_four::<8, 4, _>(&c1, &c2, &mut want);
                sample_sad_four_8x4(&c1, &c2, &mut got);
                assert_eq!(got, want, "8x4 four-point {at}");

                sample_sad_four::<4, 8, _>(&c1, &c2, &mut want);
                sample_sad_four_4x8(&c1, &c2, &mut got);
                assert_eq!(got, want, "4x8 four-point {at}");
            }
        }
    }

    /// The tables hand these kernels `RecCursor`s, whose rows arrive by value
    /// (`RowBuf`) rather than as a borrowed slice — the other `Row` type the loads
    /// have to accept.
    #[test]
    fn sad_parity_through_the_shared_cursor() {
        for (name, mut p1, mut p2) in input_pairs(64, 64) {
            for anchor in ANCHORS {
                let (want, want4) = {
                    let c1 = PlaneCursor::new(&p1, anchor, 64);
                    let c2 = PlaneCursor::new(&p2, anchor, 64);
                    let mut s = [0i32; 4];
                    sample_sad_four::<16, 16, _>(&c1, &c2, &mut s);
                    (sample_sad::<16, 16, _>(&c1, &c2), s)
                };
                let c1 = RecCursor::over_owned(&mut p1, anchor, 64);
                let c2 = RecCursor::over_owned(&mut p2, anchor, 64);
                assert_eq!(sample_sad_16x16(&c1, &c2), want, "16x16 via RecCursor, {name} @ {anchor}");
                let mut got = [0i32; 4];
                sample_sad_four_16x16(&c1, &c2, &mut got);
                assert_eq!(got, want4, "16x16 four-point via RecCursor, {name} @ {anchor}");
            }
        }
    }

    /// The AVX2 slot never changes on this target: `has_avx2()` is false on every
    /// aarch64 build, so asking for AVX2 must leave the baseline kernel installed.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn init_sample_sad_installs_avx2_only_where_the_cpu_has_it() {
        use crate::common::cpu_core::{WELS_CPU_AVX2, WELS_CPU_SSE2};
        use crate::encoder::svc_mode_decision::BLOCK_16x16;
        use crate::encoder::sample::WelsInitSampleSadFunc;
        use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

        let slot = |flags: u32| {
            let mut fl = SWelsFuncPtrList::default();
            WelsInitSampleSadFunc(&mut fl, flags);
            fl.sSampleDealingFuncs.pfSampleSad[BLOCK_16x16].map(|f| f as usize)
        };

        let baseline = slot(WELS_CPU_SSE2);
        let asked_for_avx2 = slot(WELS_CPU_SSE2 | WELS_CPU_AVX2);
        assert!(baseline.is_some() && asked_for_avx2.is_some());
        assert!(!crate::simd::has_avx2(), "no aarch64 build can answer true here");
        assert_eq!(asked_for_avx2, baseline, "the flag alone must not install an AVX2 kernel");
    }
}
