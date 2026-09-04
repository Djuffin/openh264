//! SAD and four-point SAD on `wide` lane types — the twin of `simd::x86_64::sad`.
//!
//! # The instruction that is missing
//!
//! `psadbw` sums sixteen absolute byte differences into one word in a single
//! instruction, and the intrinsic kernels are nothing but that and an add per row.
//! `wide` 1.3 has no wrapper for it, so the difference here is `max - min` on
//! `u8x16` (three ops), zero-extended to two `i16x8` (two ops) and accumulated (two
//! adds) — seven ops per row where the intrinsic uses two — with a `pmaddwd` reduce at
//! the end. This is the family where the portable API pays most.

#![forbid(unsafe_code)]

use wide::bytemuck::cast;
use wide::{i16x8, u8x16, u8x32};

use super::lanes::{hsum_i16, load4, load8, widen_hi, widen_lo};
use crate::safe::plane::RefSamples;

/// `|a - b|` per byte.
#[inline(always)]
fn abs_diff(a: u8x16, b: u8x16) -> u8x16 {
    a.max(b) - a.min(b)
}

// ============================================================================
// Row loops
// ============================================================================

/// The 16-wide row loop. One `i16x8` accumulator takes both byte halves: a lane
/// gains at most `2 * 255` per row over at most 16 rows, so it peaks at 8160.
#[inline(always)]
fn sad_16x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, dx: isize, dy: isize) -> i32 {
    let mut acc = i16x8::ZERO;
    for y in 0..H {
        let a = u8x16::new(sample1.row_n::<16>(y as isize, 0));
        let b = u8x16::new(sample2.row_n::<16>(y as isize + dy, dx));
        let d = abs_diff(a, b);
        acc = acc + widen_lo(d) + widen_hi(d);
    }
    hsum_i16(acc)
}

/// Two rows per step through `u8x32` — the shape of the AVX2 kernel.
///
/// `wide` selects its instruction set at compile time, so on a build without
/// `+avx2` in the target features `u8x32` is two `u8x16`s and this is the 16-wide
/// loop unrolled by two. It is a distinct kernel so the AVX2 table slot still points
/// at something other than the baseline slot, which `sample.rs`'s install test pins.
#[inline(always)]
fn sad_16x_two_rows<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    dx: isize,
    dy: isize,
) -> i32 {
    const { assert!(H % 2 == 0, "sad_16x_two_rows steps two rows; H must be even") };
    let mut acc = i16x8::ZERO;
    let mut y = 0isize;
    while (y as usize) < H {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        a[..16].copy_from_slice(&sample1.row_n::<16>(y, 0));
        a[16..].copy_from_slice(&sample1.row_n::<16>(y + 1, 0));
        b[..16].copy_from_slice(&sample2.row_n::<16>(y + dy, dx));
        b[16..].copy_from_slice(&sample2.row_n::<16>(y + 1 + dy, dx));
        let (va, vb) = (u8x32::new(a), u8x32::new(b));
        let d = va.max(vb) - va.min(vb);
        let [d0, d1]: [u8x16; 2] = cast(d);
        acc = acc + widen_lo(d0) + widen_hi(d0) + widen_lo(d1) + widen_hi(d1);
        y += 2;
    }
    hsum_i16(acc)
}

#[inline(always)]
fn sad_8x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, dx: isize, dy: isize) -> i32 {
    let mut acc = i16x8::ZERO;
    for y in 0..H {
        let a = load8(&sample1.row_n::<8>(y as isize, 0));
        let b = load8(&sample2.row_n::<8>(y as isize + dy, dx));
        acc = acc + widen_lo(abs_diff(a, b));
    }
    hsum_i16(acc)
}

#[inline(always)]
fn sad_4x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, dx: isize, dy: isize) -> i32 {
    let mut acc = i16x8::ZERO;
    for y in 0..H {
        let a = load4(&sample1.row_n::<4>(y as isize, 0));
        let b = load4(&sample2.row_n::<4>(y as isize + dy, dx));
        acc = acc + widen_lo(abs_diff(a, b));
    }
    hsum_i16(acc)
}

/// The four whole-sample neighbours — up, down, left, right — in one pass over
/// `sample1`, one accumulator each.
#[inline(always)]
fn sad_four_16x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    let mut acc = [i16x8::ZERO; 4];
    for y in 0..H {
        let y = y as isize;
        let a = u8x16::new(sample1.row_n::<16>(y, 0));
        let probes = [
            u8x16::new(sample2.row_n::<16>(y - 1, 0)),
            u8x16::new(sample2.row_n::<16>(y + 1, 0)),
            u8x16::new(sample2.row_n::<16>(y, -1)),
            u8x16::new(sample2.row_n::<16>(y, 1)),
        ];
        for k in 0..4 {
            let d = abs_diff(a, probes[k]);
            acc[k] = acc[k] + widen_lo(d) + widen_hi(d);
        }
    }
    for k in 0..4 {
        sad[k] = hsum_i16(acc[k]);
    }
}

#[inline(always)]
fn sad_four_8x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    let mut acc = [i16x8::ZERO; 4];
    for y in 0..H {
        let y = y as isize;
        let a = load8(&sample1.row_n::<8>(y, 0));
        let probes = [
            load8(&sample2.row_n::<8>(y - 1, 0)),
            load8(&sample2.row_n::<8>(y + 1, 0)),
            load8(&sample2.row_n::<8>(y, -1)),
            load8(&sample2.row_n::<8>(y, 1)),
        ];
        for k in 0..4 {
            acc[k] = acc[k] + widen_lo(abs_diff(a, probes[k]));
        }
    }
    for k in 0..4 {
        sad[k] = hsum_i16(acc[k]);
    }
}

#[inline(always)]
fn sad_four_4x<S: RefSamples, const H: usize>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    let mut acc = [i16x8::ZERO; 4];
    for y in 0..H {
        let y = y as isize;
        let a = load4(&sample1.row_n::<4>(y, 0));
        let probes = [
            load4(&sample2.row_n::<4>(y - 1, 0)),
            load4(&sample2.row_n::<4>(y + 1, 0)),
            load4(&sample2.row_n::<4>(y, -1)),
            load4(&sample2.row_n::<4>(y, 1)),
        ];
        for k in 0..4 {
            acc[k] = acc[k] + widen_lo(abs_diff(a, probes[k]));
        }
    }
    for k in 0..4 {
        sad[k] = hsum_i16(acc[k]);
    }
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

#[inline(always)]
pub fn sample_sad_16x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_16x::<S, 16>(sample1, sample2, 0, 0)
}

/// The AVX2 slot's kernel. Safe to install anywhere: nothing here needs AVX2, see
/// [`sad_16x_two_rows`].
#[inline(always)]
pub(crate) fn sample_sad_16x16_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_16x_two_rows::<S, 16>(sample1, sample2, 0, 0)
}

#[inline(always)]
pub fn sample_sad_16x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_16x::<S, 8>(sample1, sample2, 0, 0)
}

/// See [`sample_sad_16x16_avx2`].
#[inline(always)]
pub(crate) fn sample_sad_16x8_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_16x_two_rows::<S, 8>(sample1, sample2, 0, 0)
}

#[inline(always)]
pub fn sample_sad_8x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_8x::<S, 16>(sample1, sample2, 0, 0)
}

#[inline(always)]
pub fn sample_sad_8x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_8x::<S, 8>(sample1, sample2, 0, 0)
}

#[inline(always)]
pub fn sample_sad_4x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_4x::<S, 4>(sample1, sample2, 0, 0)
}

#[inline(always)]
pub fn sample_sad_8x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_8x::<S, 4>(sample1, sample2, 0, 0)
}

#[inline(always)]
pub fn sample_sad_4x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    sad_4x::<S, 8>(sample1, sample2, 0, 0)
}

#[inline(always)]
pub fn sample_sad_four_16x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    sad_four_16x::<S, 16>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_16x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    sad_four_16x::<S, 8>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_8x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    sad_four_8x::<S, 16>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_8x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    sad_four_8x::<S, 8>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_4x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    sad_four_4x::<S, 4>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_8x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    sad_four_8x::<S, 4>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_4x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    sad_four_4x::<S, 8>(sample1, sample2, sad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::sad_common::{sample_sad, sample_sad_four};
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

    /// The `_avx2` pair, which is not AVX2 code and needs no probe to run.
    ///
    /// `wide` has no runtime dispatch, so these two are 128-bit bodies that step two
    /// rows — the same lanes as every other kernel here, filling the slots the
    /// intrinsic twin fills with `vpsadbw`. There is no instruction to test for, on
    /// any target, so this simply runs.
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
    // The three tests above reach all sixteen kernels, but each at one anchor over one
    // input pattern, so a kernel wrong at another alignment — or only on inputs a ramp
    // never produces — passes them all.
    //
    // The sweep below runs every kernel over four anchors, one per residue class mod 8,
    // and five distributions. The all-`0xFF`/all-`0x00` pair and the near-identical
    // pair are the ends of the accumulator's range, where a `psadbw` accumulation that
    // widened or saturated wrongly would show.
    // ========================================================================

    /// A 64-bit LCG, so a failing seed is replayable.
    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32i32) as u8
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

                assert_eq!(
                    sample_sad_16x16(&c1, &c2),
                    sample_sad::<16, 16, _>(&c1, &c2),
                    "16x16 {at}"
                );
                assert_eq!(
                    sample_sad_16x8(&c1, &c2),
                    sample_sad::<16, 8, _>(&c1, &c2),
                    "16x8 {at}"
                );
                assert_eq!(
                    sample_sad_8x16(&c1, &c2),
                    sample_sad::<8, 16, _>(&c1, &c2),
                    "8x16 {at}"
                );
                assert_eq!(
                    sample_sad_8x8(&c1, &c2),
                    sample_sad::<8, 8, _>(&c1, &c2),
                    "8x8 {at}"
                );
                assert_eq!(
                    sample_sad_4x4(&c1, &c2),
                    sample_sad::<4, 4, _>(&c1, &c2),
                    "4x4 {at}"
                );
                assert_eq!(
                    sample_sad_8x4(&c1, &c2),
                    sample_sad::<8, 4, _>(&c1, &c2),
                    "8x4 {at}"
                );
                assert_eq!(
                    sample_sad_4x8(&c1, &c2),
                    sample_sad::<4, 8, _>(&c1, &c2),
                    "4x8 {at}"
                );
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

    /// The `_avx2` pair over the same sweep, and like `test_avx2_sad_parity` it runs
    /// everywhere: nothing in this module is gated on a CPU feature.
    #[test]
    fn avx2_sad_parity_over_anchors_and_distributions() {
        for (name, p1, p2) in input_pairs(64, 64) {
            for anchor in ANCHORS {
                let c1 = PlaneCursor::new(&p1, anchor, 64);
                let c2 = PlaneCursor::new(&p2, anchor, 64);
                let at = format!("{name} @ {anchor}");
                assert_eq!(
                    sample_sad_16x16_avx2(&c1, &c2),
                    sample_sad::<16, 16, _>(&c1, &c2),
                    "16x16 avx2 {at}"
                );
                assert_eq!(
                    sample_sad_16x8_avx2(&c1, &c2),
                    sample_sad::<16, 8, _>(&c1, &c2),
                    "16x8 avx2 {at}"
                );
            }
        }
    }

    /// The table site is the only thing standing between these kernels and a SIGILL,
    /// so pin what it installs.
    ///
    /// `WelsInitSampleSadFunc` fills `pfSampleSad[BLOCK_16x16]` from the AVX2 kernel
    /// exactly when `uiCpuFlag` asks for AVX2 *and* this CPU has it.
    ///
    /// **What this catches, and where.** The flag half is pinned on every host: drop
    /// the `uiCpuFlag & WELS_CPU_AVX2` test and the `WELS_CPU_SSE2`-only case starts
    /// returning the AVX2 pointer. The hardware half — dropping `has_avx2()` — can only
    /// fail this test on a machine without AVX2, because on one with it both spellings
    /// install the same kernel. That is the machine where it matters, and it is not
    /// this one, so treat a green run here as covering the flag half only.
    ///
    /// Function-pointer identity is the comparison, so the caveat on
    /// `common/mc.rs`'s `init_mc_func_cpu_flags` applies: both addresses come from the
    /// same `WelsInitSampleSadFunc` instantiation, which makes them comparable, but
    /// Miri mints a fresh synthetic address per reification and is excluded.
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

        // **The oracle is `has_avx2()`, not `is_x86_feature_detected!`.** They are not the
        // same question: the table arm consults the port's probe, which answers from the
        // build as well as the CPU — under `--features scalar` the feature word is `0`, so
        // a host that *has* AVX2 must still get the baseline entry. Asking the CPU
        // directly made this test fail there.
        if crate::simd::has_avx2() {
            assert_ne!(
                asked_for_avx2, baseline,
                "has_avx2() is true, so asking for AVX2 must change the installed kernel"
            );
        } else {
            assert_eq!(
                asked_for_avx2, baseline,
                "has_avx2() is false, so the flag alone must not install an AVX2 kernel"
            );
        }
    }
}
