//! The VAA (video analysis) statistics kernels — `VAACalcSad_sse2`,
//! `VAACalcSadVar_sse2`, `VAACalcSadSsd_sse2`, `VAACalcSadBgd_sse2` and
//! `VAACalcSadSsdBgd_sse2`, `codec/processing/src/x86/vaa.asm`.
//!
//! One whole-picture walk in five shapes: 16x16 macroblocks in raster order, four
//! 8x8 quadrants each in the order top-left, top-right, bottom-left, bottom-right,
//! with `sad`, `sd` and `mad` per quadrant and `sum`, `sqsum` and `sqdiff` per
//! macroblock. [`crate::processing::vaacalc`] is the reference this must match byte
//! for byte, and the five kernels are one body with three const flags rather than
//! five copies, as they are there.
//!
//! `psadbw` against a whole 16-byte register lands the sum of bytes 0..8 in the low
//! quadword and of bytes 8..16 in the high one — exactly the two quadrants of a row, so a
//! register is never split to keep them apart. The rest follows the asm's macros:
//! `WELS_SAD_SD_MAD_16x1_SSE2`'s `psadbw` against zero for each side's sample sum
//! (`sd` is `sum(cur) - sum(ref)`, per quadrant); `pmaxub`/`pminub`/`psubb` for the
//! absolute difference, with a running `pmaxub` for `mad`;
//! `punpcklbw`/`punpckhbw` against zero and `pmaddwd` for both squared sums; and
//! `WELS_MAX_REG_SSE2`'s three shift-and-max pairs to reduce `mad`, which leaves
//! each quadrant's maximum in byte 0 and byte 8 of the register.
//!
//! Nothing overflows. A `psadbw` accumulator takes at most `8 * 255` per row over
//! the eight rows of a half-macroblock, 16320, and `paddd` is safe on it because the
//! upper dword of each quadword is always zero. The `pmaddwd` accumulators peak at
//! `256 * 255^2 = 16.6M` over a whole macroblock.
//!
//! The width loop counts `pic_width >> 4` macroblocks, as `VAACalcSad*_c` and
//! [`crate::processing::vaacalc::vaa_span`] do, rather than counting `pic_width` down by
//! 16. The step between macroblock rows is `(pic_stride << 4) - pic_width` after sixteen
//! bytes per macroblock, including the quirk that a ragged width shifts every macroblock
//! row left by the remainder. Loads are unaligned (`movdqu`), there being no alignment
//! precondition on the stride. `sd` and `sum` subtract and add after the reduce, in the
//! same 32-bit width.
//!
//! Every read stays inside `vaa_span(pic_width, pic_height, pic_stride)` bytes of
//! each plane; the last macroblock's window is `15 * stride + 16` bytes.
#![allow(unsafe_code)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

// ============================================================================
// What one macroblock yields
// ============================================================================

/// The six output arrays the five kernels write between them, one entry per
/// macroblock each.
///
/// Every kernel names all six and writes the ones its flags select. The three flags are
/// const, so the writes a kernel does not make are not compiled and the slices behind them
/// are never indexed — which is why the entry points pass `&mut []` for those. Handing the
/// walk the arrays rather than a per-macroblock struct keeps a macroblock's results in
/// registers.
struct Outputs<'a> {
    sad8x8: &'a mut [[i32; 4]],
    sd8x8: &'a mut [[i32; 4]],
    mad8x8: &'a mut [[u8; 4]],
    sum16x16: &'a mut [i32],
    sqsum16x16: &'a mut [i32],
    sqdiff16x16: &'a mut [i32],
}

/// One 16x16 macroblock's statistics, in the C++'s quadrant order — top-left,
/// top-right, bottom-left, bottom-right — with the three macroblock-wide sums.
/// Fields the flags exclude are never written and stay zero.
#[derive(Clone, Copy, Default)]
struct MbStats {
    sad: [i32; 4],
    sd: [i32; 4],
    mad: [u8; 4],
    sum: i32,
    sqsum: i32,
    sqdiff: i32,
}

/// The two quadrants sitting side by side in one half of a macroblock: eight rows,
/// sixteen samples wide.
#[derive(Clone, Copy, Default)]
struct HalfStats {
    sad: [i32; 2],
    sd: [i32; 2],
    mad: [u8; 2],
    sum: i32,
    sqsum: i32,
    sqdiff: i32,
}

/// The low dword of the low quadword, and of the high one — the two quadrants of a
/// `psadbw` accumulator.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn halves_i32(v: __m128i) -> [i32; 2] {
    [
        _mm_cvtsi128_si32(v),
        _mm_cvtsi128_si32(_mm_srli_si128(v, 8)),
    ]
}

/// Sum of the four dwords — the asm's `pshufd`/`paddd` pair, twice.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn sum_i32(v: __m128i) -> i32 {
    let v = _mm_add_epi32(v, _mm_srli_si128(v, 8));
    _mm_cvtsi128_si32(_mm_add_epi32(v, _mm_srli_si128(v, 4)))
}

/// The eight rows of one half-macroblock, from plane slices anchored at its
/// top-left sample.
///
/// `WELS_SAD_16x2_SSE2`, `WELS_SAD_SUM_SQSUM_16x1_SSE2`,
/// `WELS_SAD_SUM_SQSUM_SQDIFF_16x1_SSE2`, `WELS_SAD_SD_MAD_16x1_SSE2` and
/// `WELS_SAD_BGD_SQDIFF_16x1_SSE2` are all this loop with a different subset of the
/// accumulators live, which is what the three flags select.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn half_mb<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
) -> HalfStats {
    // Trim both planes to exactly the eight rows this half reads, once, before the loop,
    // so the per-row bounds checks inside it fold away.
    let cur = &cur[..7 * stride + 16];
    let refp = &refp[..7 * stride + 16];

    // SAFETY: every load below is a `movdqu` of sixteen bytes at `base`, and the two
    // slices are exactly `7 * stride + 16` long with `base <= 7 * stride`, so the
    // read is inside them. Everything else is register-only.
    unsafe {
        let zero = _mm_setzero_si128();
        // `psadbw` accumulators: the low quadword is the left quadrant, the high one
        // the right.
        let mut sad = zero;
        let mut cur_sum = zero;
        let mut ref_sum = zero;
        let mut mad = zero;
        let mut sqsum = zero;
        let mut sqdiff = zero;

        for k in 0..8 {
            let base = k * stride;
            let a = _mm_loadu_si128(cur[base..base + 16].as_ptr() as *const __m128i);
            let b = _mm_loadu_si128(refp[base..base + 16].as_ptr() as *const __m128i);

            sad = _mm_add_epi32(sad, _mm_sad_epu8(a, b));
            if VAR || BGD {
                // The current picture's sample sum serves both `sum16x16` (the whole
                // macroblock) and `sd8x8` (per quadrant).
                cur_sum = _mm_add_epi32(cur_sum, _mm_sad_epu8(a, zero));
            }
            if BGD {
                ref_sum = _mm_add_epi32(ref_sum, _mm_sad_epu8(b, zero));
                // `pmaxub`/`pminub`/`psubb` — SSE2 has no unsigned byte subtract.
                let d = _mm_sub_epi8(_mm_max_epu8(a, b), _mm_min_epu8(a, b));
                mad = _mm_max_epu8(mad, d);
            }
            if VAR {
                let lo = _mm_unpacklo_epi8(a, zero);
                let hi = _mm_unpackhi_epi8(a, zero);
                sqsum = _mm_add_epi32(sqsum, _mm_madd_epi16(lo, lo));
                sqsum = _mm_add_epi32(sqsum, _mm_madd_epi16(hi, hi));
            }
            if SQDIFF {
                let d = _mm_sub_epi8(_mm_max_epu8(a, b), _mm_min_epu8(a, b));
                let lo = _mm_unpacklo_epi8(d, zero);
                let hi = _mm_unpackhi_epi8(d, zero);
                sqdiff = _mm_add_epi32(sqdiff, _mm_madd_epi16(lo, lo));
                sqdiff = _mm_add_epi32(sqdiff, _mm_madd_epi16(hi, hi));
            }
        }

        let mut out = HalfStats {
            sad: halves_i32(sad),
            ..Default::default()
        };
        if VAR || BGD {
            let c = halves_i32(cur_sum);
            if VAR {
                out.sum = c[0] + c[1];
            }
            if BGD {
                let r = halves_i32(ref_sum);
                out.sd = [c[0] - r[0], c[1] - r[1]];
                // `WELS_MAX_REG_SSE2`: shifting the whole register right by 4, 2 and
                // 1 bytes and taking `pmaxub` each time leaves the maximum of bytes
                // 0..8 in byte 0 and of bytes 8..16 in byte 8, because a byte only
                // ever receives from a higher one.
                let m = _mm_max_epu8(mad, _mm_srli_si128(mad, 4));
                let m = _mm_max_epu8(m, _mm_srli_si128(m, 2));
                let m = _mm_max_epu8(m, _mm_srli_si128(m, 1));
                let [lo, hi] = halves_i32(m);
                out.mad = [lo as u8, hi as u8];
            }
        }
        if VAR {
            out.sqsum = sum_i32(sqsum);
        }
        if SQDIFF {
            out.sqdiff = sum_i32(sqdiff);
        }
        out
    }
}

/// One macroblock: the top half, then the bottom half eight rows down.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn mb_stats<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
) -> MbStats {
    // The window a macroblock reads, checked once: sixteen rows of sixteen bytes,
    // the last of which ends `15 * stride + 16` past the origin.
    let cur = &cur[..15 * stride + 16];
    let refp = &refp[..15 * stride + 16];
    let bottom = 8 * stride;
    let top = half_mb::<VAR, SQDIFF, BGD>(cur, refp, stride);
    let bot = half_mb::<VAR, SQDIFF, BGD>(&cur[bottom..], &refp[bottom..], stride);
    MbStats {
        sad: [top.sad[0], top.sad[1], bot.sad[0], bot.sad[1]],
        sd: [top.sd[0], top.sd[1], bot.sd[0], bot.sd[1]],
        mad: [top.mad[0], top.mad[1], bot.mad[0], bot.mad[1]],
        sum: top.sum + bot.sum,
        sqsum: top.sqsum + bot.sqsum,
        sqdiff: top.sqdiff + bot.sqdiff,
    }
}

/// The picture walk, macroblock by macroblock, returning the frame's total SAD.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn walk<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    out: &mut Outputs<'_>,
) -> i32 {
    let mb_width = pic_width >> 4;
    let mb_height = pic_height >> 4;
    let stride = pic_stride as usize;
    let step = ((pic_stride << 4) - pic_width) as usize;

    let mut frame_sad = 0i32;
    let mut mb_index = 0usize;
    let mut row_origin = 0usize;
    for _ in 0..mb_height {
        let mut mb_origin = row_origin;
        for _ in 0..mb_width {
            let s = mb_stats::<VAR, SQDIFF, BGD>(&cur[mb_origin..], &refp[mb_origin..], stride);
            // The frame total accumulates in the C++'s quadrant order.
            frame_sad += s.sad[0] + s.sad[1] + s.sad[2] + s.sad[3];
            out.sad8x8[mb_index] = s.sad;
            if VAR {
                out.sum16x16[mb_index] = s.sum;
                out.sqsum16x16[mb_index] = s.sqsum;
            }
            if SQDIFF {
                out.sqdiff16x16[mb_index] = s.sqdiff;
            }
            if BGD {
                out.sd8x8[mb_index] = s.sd;
                out.mad8x8[mb_index] = s.mad;
            }
            mb_index += 1;
            mb_origin += 16;
        }
        row_origin = mb_origin + step;
    }
    frame_sad
}

// ============================================================================
// Entry points
// ============================================================================

/// `VAACalcSad_sse2`.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn vaa_calc_sad(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
) -> i32 {
    let mut out = Outputs {
        sad8x8,
        sd8x8: &mut [],
        mad8x8: &mut [],
        sum16x16: &mut [],
        sqsum16x16: &mut [],
        sqdiff16x16: &mut [],
    };
    // SAFETY: SSE2 is part of the x86_64 baseline.
    unsafe { walk::<false, false, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadVar_sse2`.
#[cfg(target_arch = "x86_64")]
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_var(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
) -> i32 {
    let mut out = Outputs {
        sad8x8,
        sd8x8: &mut [],
        mad8x8: &mut [],
        sum16x16,
        sqsum16x16,
        sqdiff16x16: &mut [],
    };
    // SAFETY: SSE2 is part of the x86_64 baseline.
    unsafe { walk::<true, false, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadSsd_sse2`.
#[cfg(target_arch = "x86_64")]
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_ssd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
    sqdiff16x16: &mut [i32],
) -> i32 {
    let mut out = Outputs {
        sad8x8,
        sd8x8: &mut [],
        mad8x8: &mut [],
        sum16x16,
        sqsum16x16,
        sqdiff16x16,
    };
    // SAFETY: SSE2 is part of the x86_64 baseline.
    unsafe { walk::<true, true, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadBgd_sse2`.
#[cfg(target_arch = "x86_64")]
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_bgd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sd8x8: &mut [[i32; 4]],
    mad8x8: &mut [[u8; 4]],
) -> i32 {
    let mut out = Outputs {
        sad8x8,
        sd8x8,
        mad8x8,
        sum16x16: &mut [],
        sqsum16x16: &mut [],
        sqdiff16x16: &mut [],
    };
    // SAFETY: SSE2 is part of the x86_64 baseline.
    unsafe { walk::<false, false, true>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadSsdBgd_sse2`.
#[cfg(target_arch = "x86_64")]
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_ssd_bgd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
    sqdiff16x16: &mut [i32],
    sd8x8: &mut [[i32; 4]],
    mad8x8: &mut [[u8; 4]],
) -> i32 {
    let mut out = Outputs {
        sad8x8,
        sd8x8,
        mad8x8,
        sum16x16,
        sqsum16x16,
        sqdiff16x16,
    };
    // SAFETY: SSE2 is part of the x86_64 baseline.
    unsafe { walk::<true, true, true>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::vaacalc as reference;
    use crate::processing::vaacalc::vaa_span;

    /// Planes of exactly `vaa_span` bytes, so a kernel that reads one byte past
    /// what the walk is allowed to touch panics here instead of silently working.
    fn planes(w: i32, h: i32, stride: i32, f: impl Fn(usize) -> (u8, u8)) -> (Vec<u8>, Vec<u8>) {
        let n = vaa_span(w, h, stride);
        let mut cur = vec![0u8; n];
        let mut refp = vec![0u8; n];
        for i in 0..n {
            let (c, r) = f(i);
            cur[i] = c;
            refp[i] = r;
        }
        (cur, refp)
    }

    fn noise(seed: u64) -> impl Fn(usize) -> (u8, u8) {
        move |i| {
            let mut x = (i as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ seed;
            x ^= x >> 29;
            x = x.wrapping_mul(0xBF58476D1CE4E5B9);
            x ^= x >> 32;
            ((x >> 8) as u8, (x >> 40) as u8)
        }
    }

    /// Runs all five kernels against the scalar reference and compares every output
    /// array and every return value.
    #[track_caller]
    fn check(cur: &[u8], refp: &[u8], w: i32, h: i32, stride: i32, what: &str) {
        let mbs = ((w >> 4) * (h >> 4)).max(0) as usize;
        let z4 = || vec![[0i32; 4]; mbs];
        let zm = || vec![[0u8; 4]; mbs];
        let z = || vec![0i32; mbs];

        // vaa_calc_sad
        let (mut a, mut e) = (z4(), z4());
        let ga = vaa_calc_sad(cur, refp, w, h, stride, &mut a);
        let ge = reference::vaa_calc_sad(cur, refp, w, h, stride, &mut e);
        assert_eq!((ga, &a), (ge, &e), "vaa_calc_sad: {what}");

        // vaa_calc_sad_var
        let (mut a, mut e) = (z4(), z4());
        let (mut asum, mut esum) = (z(), z());
        let (mut asq, mut esq) = (z(), z());
        let ga = vaa_calc_sad_var(cur, refp, w, h, stride, &mut a, &mut asum, &mut asq);
        let ge = reference::vaa_calc_sad_var(cur, refp, w, h, stride, &mut e, &mut esum, &mut esq);
        assert_eq!(
            (ga, &a, &asum, &asq),
            (ge, &e, &esum, &esq),
            "vaa_calc_sad_var: {what}"
        );

        // vaa_calc_sad_ssd
        let (mut a, mut e) = (z4(), z4());
        let (mut asum, mut esum) = (z(), z());
        let (mut asq, mut esq) = (z(), z());
        let (mut asd, mut esd) = (z(), z());
        let ga = vaa_calc_sad_ssd(
            cur, refp, w, h, stride, &mut a, &mut asum, &mut asq, &mut asd,
        );
        let ge = reference::vaa_calc_sad_ssd(
            cur, refp, w, h, stride, &mut e, &mut esum, &mut esq, &mut esd,
        );
        assert_eq!(
            (ga, &a, &asum, &asq, &asd),
            (ge, &e, &esum, &esq, &esd),
            "vaa_calc_sad_ssd: {what}"
        );

        // vaa_calc_sad_bgd
        let (mut a, mut e) = (z4(), z4());
        let (mut asd, mut esd) = (z4(), z4());
        let (mut amad, mut emad) = (zm(), zm());
        let ga = vaa_calc_sad_bgd(cur, refp, w, h, stride, &mut a, &mut asd, &mut amad);
        let ge = reference::vaa_calc_sad_bgd(cur, refp, w, h, stride, &mut e, &mut esd, &mut emad);
        assert_eq!(
            (ga, &a, &asd, &amad),
            (ge, &e, &esd, &emad),
            "vaa_calc_sad_bgd: {what}"
        );

        // vaa_calc_sad_ssd_bgd
        let (mut a, mut e) = (z4(), z4());
        let (mut asum, mut esum) = (z(), z());
        let (mut asq, mut esq) = (z(), z());
        let (mut asqd, mut esqd) = (z(), z());
        let (mut asd, mut esd) = (z4(), z4());
        let (mut amad, mut emad) = (zm(), zm());
        let ga = vaa_calc_sad_ssd_bgd(
            cur, refp, w, h, stride, &mut a, &mut asum, &mut asq, &mut asqd, &mut asd, &mut amad,
        );
        let ge = reference::vaa_calc_sad_ssd_bgd(
            cur, refp, w, h, stride, &mut e, &mut esum, &mut esq, &mut esqd, &mut esd, &mut emad,
        );
        assert_eq!(
            (ga, &a, &asum, &asq, &asqd, &asd, &amad),
            (ge, &e, &esum, &esq, &esqd, &esd, &emad),
            "vaa_calc_sad_ssd_bgd: {what}"
        );
    }

    /// The geometries the encoder actually hands these kernels, plus strides wider
    /// than the width. Planes are exactly `vaa_span` long, so this is also the
    /// over-read test: a kernel that reads past the walk's last sample panics.
    #[test]
    fn parity_over_geometries() {
        for &(w, h, stride) in &[
            (16, 16, 16),
            (16, 16, 32),
            (32, 16, 48),
            (48, 32, 64),
            (64, 64, 64),
            (64, 64, 96),
            (176, 144, 176),
            (176, 144, 192),
            (320, 48, 336),
        ] {
            let (cur, refp) = planes(w, h, stride, noise(0xA5A5 ^ w as u64));
            check(
                &cur,
                &refp,
                w,
                h,
                stride,
                &format!("noise {w}x{h} stride {stride}"),
            );
        }
    }

    /// The ends of the range: a maximal difference everywhere, no difference at all,
    /// and a pattern whose per-row sums alternate.
    #[test]
    fn parity_at_the_extremes() {
        let (w, h, stride) = (48, 32, 64);

        let (cur, refp) = planes(w, h, stride, |_| (0, 255));
        check(&cur, &refp, w, h, stride, "0 against 255");

        let (cur, refp) = planes(w, h, stride, |_| (255, 0));
        check(&cur, &refp, w, h, stride, "255 against 0");

        let (cur, refp) = planes(w, h, stride, |i| ((i & 0xFF) as u8, (i & 0xFF) as u8));
        check(&cur, &refp, w, h, stride, "identical planes");

        let (cur, refp) = planes(
            w,
            h,
            stride,
            |i| {
                if i & 1 == 0 { (255, 0) } else { (0, 255) }
            },
        );
        check(&cur, &refp, w, h, stride, "alternating stripes");

        let (cur, refp) = planes(w, h, stride, |i| {
            (
                if (i / stride as usize) & 1 == 0 {
                    255
                } else {
                    0
                },
                128,
            )
        });
        check(&cur, &refp, w, h, stride, "alternating rows");
    }

    /// `mad8x8` is the largest absolute difference in a quadrant, so it is the one
    /// output a uniform plane cannot exercise: put exactly one big difference in
    /// each quadrant, at a different offset in each, over an otherwise flat picture.
    #[test]
    fn parity_of_the_per_quadrant_maximum() {
        let (w, h, stride) = (32, 32, 48);
        let (mut cur, mut refp) = planes(w, h, stride, |_| (100, 100));
        let span = cur.len();
        let mut k = 0usize;
        for mb_y in 0..2usize {
            for mb_x in 0..2usize {
                for q in 0..4usize {
                    let (qy, qx) = (q / 2, q % 2);
                    // A different sample within each quadrant, and a different
                    // magnitude, so a kernel that maxes over the wrong eight bytes
                    // or writes the wrong quadrant is caught.
                    let y = mb_y * 16 + qy * 8 + (k % 8);
                    let x = mb_x * 16 + qx * 8 + ((k * 3) % 8);
                    let i = y * stride as usize + x;
                    assert!(i < span);
                    cur[i] = 100 + (7 * (k as u8 + 1)) % 150;
                    refp[i] = 100 - (5 * (k as u8 + 1)) % 90;
                    k += 1;
                }
            }
        }
        check(
            &cur,
            &refp,
            w,
            h,
            stride,
            "one maximal difference per quadrant",
        );
    }

    /// The step quirk: a width that is not a multiple of 16 shifts every macroblock row
    /// left by the remainder, and the kernels reproduce it rather than correct it.
    #[test]
    fn parity_at_a_width_that_is_not_a_multiple_of_16() {
        for &(w, h, stride) in &[(40, 32, 64), (24, 48, 32), (72, 32, 80), (33, 32, 64)] {
            let (cur, refp) = planes(w, h, stride, noise(0x5EED ^ w as u64));
            check(
                &cur,
                &refp,
                w,
                h,
                stride,
                &format!("ragged {w}x{h} stride {stride}"),
            );
        }
    }

    /// A picture smaller than one macroblock has no macroblocks to walk, and the
    /// kernels must write nothing and return zero rather than index anything.
    #[test]
    fn parity_below_one_macroblock() {
        for &(w, h) in &[(8, 16), (16, 8), (15, 15)] {
            let cur = vec![7u8; 64 * 64];
            let refp = vec![3u8; 64 * 64];
            check(&cur, &refp, w, h, 64, &format!("degenerate {w}x{h}"));
        }
    }
}
