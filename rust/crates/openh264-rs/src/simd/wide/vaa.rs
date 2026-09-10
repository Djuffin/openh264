//! The VAA statistics kernels on `wide` lane types — the twin of
//! [`super::super::x86_64::vaa`] and of the NEON port in
//! [`super::super::aarch64::vaa`]. See either for what the five kernels compute and for
//! the walk they share with [`crate::processing::vaacalc`].
//!
//! # No `psadbw`
//!
//! `psadbw` (and its `uabd`/`uadalp` pair on NEON) reduces sixteen absolute byte
//! differences to two per-quadrant sums in one instruction. `wide` 1.7 has no wrapper
//! for it, so this pays the same bill `sad.rs` does: the difference is `max - min` on
//! `u8x16`, each byte half is zero-extended to an `i16x8` (`punpcklbw`/`punpckhbw`
//! against zero), and the two halves are accumulated separately — convenient here,
//! because the low half *is* the left quadrant and the high half the right. Six ops per
//! row where the intrinsic uses one, and one `pmaddwd`-against-ones reduce per quadrant
//! at the end. The two squared sums cost nothing: `i16x8::dot` is `pmaddwd`, which both
//! intrinsic sets use to widen a square on the way in.
//!
//! Lane bounds are the intrinsics'. An `i16x8` accumulator takes one byte per lane per
//! row over the eight rows of a half-macroblock, so it peaks at 2040; the `i32x4` square
//! accumulators peak at `128 * 255^2 = 8.3M` over a half-macroblock.
#![forbid(unsafe_code)]

use wide::bytemuck::cast;
use wide::{i16x8, i32x4, u8x16};

use super::lanes::{hsum_i16, load16, widen_hi, widen_lo};

// ============================================================================
// What one macroblock yields
// ============================================================================

/// The six output arrays the five kernels write between them, one entry per
/// macroblock each.
///
/// Every kernel names all six and writes the ones its flags select. The three flags are
/// const, so the writes a kernel does not make are not compiled and the slices behind
/// them are never indexed — which is why the entry points pass `&mut []` for those.
struct Outputs<'a> {
    sad8x8: &'a mut [[i32; 4]],
    sd8x8: &'a mut [[i32; 4]],
    mad8x8: &'a mut [[u8; 4]],
    sum16x16: &'a mut [i32],
    sqsum16x16: &'a mut [i32],
    sqdiff16x16: &'a mut [i32],
}

/// One 16x16 macroblock's statistics; see [`super::super::aarch64::vaa`].
#[derive(Clone, Copy, Default)]
struct MbStats {
    sad: [i32; 4],
    sd: [i32; 4],
    mad: [u8; 4],
    sum: i32,
    sqsum: i32,
    sqdiff: i32,
}

/// The two quadrants of one half-macroblock: `sad`, `sd` and `mad` per quadrant,
/// left then right, and the three macroblock-wide sums' share of this half.
#[derive(Clone, Copy, Default)]
struct HalfStats {
    sad: [i32; 2],
    sd: [i32; 2],
    mad: [u8; 2],
    sum: i32,
    sqsum: i32,
    sqdiff: i32,
}

/// `|a - b|` per byte, as in [`super::sad`].
#[inline(always)]
fn abs_diff(a: u8x16, b: u8x16) -> u8x16 {
    a.max(b) - a.min(b)
}

/// The eight rows of one half-macroblock, from plane slices anchored at its
/// top-left sample. The three flags select which accumulators are live.
#[inline(always)]
fn half_mb<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
) -> HalfStats {
    // Trimmed to exactly the eight rows this half reads, so the per-row indexing
    // below carries no check of its own — see `vaacalc.rs`'s `half_mb_stats`.
    let cur = &cur[..7 * stride + 16];
    let refp = &refp[..7 * stride + 16];

    // Index 0 is the left quadrant (bytes 0..8), index 1 the right (bytes 8..16).
    let mut sad = [i16x8::ZERO; 2];
    let mut cur_sum = [i16x8::ZERO; 2];
    let mut ref_sum = [i16x8::ZERO; 2];
    let mut mad = u8x16::ZERO;
    let mut sqsum = i32x4::ZERO;
    let mut sqdiff = i32x4::ZERO;

    for k in 0..8 {
        let base = k * stride;
        let a = load16(&cur[base..base + 16]);
        let b = load16(&refp[base..base + 16]);
        let d = abs_diff(a, b);
        let (dl, dh) = (widen_lo(d), widen_hi(d));
        sad[0] += dl;
        sad[1] += dh;
        if VAR || BGD {
            let (al, ah) = (widen_lo(a), widen_hi(a));
            // The current picture's sample sum serves both `sum16x16` and `sd8x8`.
            cur_sum[0] += al;
            cur_sum[1] += ah;
            if VAR {
                sqsum += al.dot(al) + ah.dot(ah);
            }
        }
        if BGD {
            ref_sum[0] += widen_lo(b);
            ref_sum[1] += widen_hi(b);
            mad = mad.max(d);
        }
        if SQDIFF {
            sqdiff += dl.dot(dl) + dh.dot(dh);
        }
    }

    let mut out = HalfStats {
        sad: [hsum_i16(sad[0]), hsum_i16(sad[1])],
        ..Default::default()
    };
    if VAR {
        out.sum = hsum_i16(cur_sum[0]) + hsum_i16(cur_sum[1]);
        out.sqsum = sqsum.reduce_add();
    }
    if BGD {
        out.sd = [
            hsum_i16(cur_sum[0]) - hsum_i16(ref_sum[0]),
            hsum_i16(cur_sum[1]) - hsum_i16(ref_sum[1]),
        ];
        // No horizontal byte max in the API, and none in SSE2 either — upstream's
        // `WELS_MAX_REG_SSE2` is three shift-and-max pairs. Over sixteen bytes already in
        // registers, a fold over the array is the same work without the shifts.
        let m: [u8; 16] = cast(mad);
        out.mad = [
            m[..8].iter().copied().max().unwrap_or(0),
            m[8..].iter().copied().max().unwrap_or(0),
        ];
    }
    if SQDIFF {
        out.sqdiff = sqdiff.reduce_add();
    }
    out
}

/// One macroblock: the top half, then the bottom half eight rows down.
#[inline(always)]
fn mb_stats<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
) -> MbStats {
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

/// The picture walk — the C's `for (i) for (j)` nest, step quirk included.
#[inline(always)]
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
// The entry points
// ============================================================================

/// `VAACalcSad`.
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
    walk::<false, false, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out)
}

/// `VAACalcSadVar`.
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
    walk::<true, false, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out)
}

/// `VAACalcSadSsd`.
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
    walk::<true, true, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out)
}

/// `VAACalcSadBgd`.
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
    walk::<false, false, true>(cur, refp, pic_width, pic_height, pic_stride, &mut out)
}

/// `VAACalcSadSsdBgd`.
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
    walk::<true, true, true>(cur, refp, pic_width, pic_height, pic_stride, &mut out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::vaacalc as reference;
    use crate::processing::vaacalc::vaa_span;

    /// Planes of **exactly** `vaa_span` bytes, so a kernel that reads one byte past
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

    /// The step quirk: a width that is not a multiple of 16 shifts every macroblock
    /// row left by the remainder, and the kernels must reproduce it rather than
    /// correct it — `vaacalc.rs`'s
    /// `calc_sad_reproduces_the_step_quirk_at_a_width_that_is_not_a_multiple_of_16`
    /// is the scalar half of this.
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
