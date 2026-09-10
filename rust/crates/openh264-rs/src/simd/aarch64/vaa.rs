//! The VAA (video analysis) statistics kernels — `VAACalcSad_AArch64_neon`,
//! `VAACalcSadVar_AArch64_neon`, `VAACalcSadSsd_AArch64_neon`,
//! `VAACalcSadBgd_AArch64_neon` and `VAACalcSadSsdBgd_AArch64_neon`,
//! `codec/processing/src/arm64/vaa_calc_aarch64_neon.S`.
//!
//! One whole-picture walk in five shapes. The picture is covered by 16x16
//! macroblocks in raster order, each split into four 8x8 quadrants (top-left,
//! top-right, bottom-left, bottom-right); `sad`, `sd` and `mad` are reported per
//! quadrant and `sum`, `sqsum` and `sqdiff` per macroblock. As in
//! [`crate::processing::vaacalc`], the five kernels are one body with three const
//! flags rather than five copies: a flag that is off emits none of its arithmetic.
//!
//! # Lane layout
//!
//! Rows are sixteen bytes wide and cover both quadrants of a half-macroblock, so no
//! register is ever split: each row accumulates with `uadalp`, which sums adjacent
//! byte pairs into eight 16-bit lanes, so lanes 0..3 are bytes 0..7 — the left
//! quadrant — and lanes 4..7 the right. Every per-quadrant quantity rides in one
//! register that way until the macroblock ends. The differences are `uabd`, `mad` is
//! a running `umax`, the two squared sums are `umull` into `uadalp`, and `sd` is
//! sum(cur) - sum(ref).
//!
//! Lane widths cannot overflow. A 16-bit `uadalp` lane takes two bytes per row, so
//! over the eight rows of a half it holds at most 4080 and over sixteen 8160; a
//! whole quadrant's SAD is at most `64 * 255 = 16320`, which is why the reduce widens
//! to 32 bits before adding the two halves. The 32-bit square accumulators peak at
//! `256 * 255^2 = 16.6M` over a whole macroblock.
//!
//! # Two departures from the asm
//!
//! - **The width loop counts macroblocks, not bytes.** The asm's inner loop only
//!   terminates when `pic_width` is a multiple of 16. This loops `pic_width >> 4`
//!   times, as `VAACalcSad*_c` and [`crate::processing::vaacalc::vaa_span`] do, so a
//!   ragged width walks the same macroblocks the scalar walks. The step between
//!   macroblock rows is `(pic_stride << 4) - pic_width` after sixteen bytes per
//!   macroblock, including the quirk that a ragged width shifts every macroblock row
//!   left by the remainder.
//! - **The four quadrant results are folded pairwise, not with four cross-lane
//!   reduces.** One `uaddlp` per half folds eight 16-bit lanes into four 32-bit ones
//!   — lanes 0, 1 still the left quadrant and 2, 3 the right — and one `addp` of the
//!   two halves lands `[top-left, top-right, bottom-left, bottom-right]` in a single
//!   register, stored with one `st1`. `mad` is the same shape with three `umaxp`, and
//!   `sd` is `sub` of the two reduced registers. Same sums and maxima in the same
//!   32-bit width, additions regrouped, and nothing crosses to the integer
//!   registers.
//!
//! Every read stays inside `vaa_span(pic_width, pic_height, pic_stride)` bytes of
//! each plane — the last macroblock's window is `15 * stride + 16` bytes and no row
//! below the last macroblock row is touched — which lets
//! `CVAACalculation::Process` trim both planes to that length and turn a geometry
//! bug into a panic instead of a read past the plane.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld16, low4};

// ============================================================================
// Where a macroblock's statistics go
// ============================================================================

/// The six output arrays the five kernels write between them, one entry per
/// macroblock each.
///
/// Every kernel names all six and writes the ones its flags select. The three flags
/// are const, so the writes a kernel does not make are not compiled and the slices
/// behind them are never indexed — which is why the entry points pass `&mut []` for
/// those.
struct Outputs<'a> {
    sad8x8: &'a mut [[i32; 4]],
    sd8x8: &'a mut [[i32; 4]],
    mad8x8: &'a mut [[u8; 4]],
    sum16x16: &'a mut [i32],
    sqsum16x16: &'a mut [i32],
    sqdiff16x16: &'a mut [i32],
}

/// One half-macroblock's accumulators, unreduced.
///
/// Sixteen-byte rows cover both quadrants at once, so every 16-bit accumulator here
/// holds the left quadrant in lanes 0..4 and the right in lanes 4..8 — the pairing
/// `uadalp` gives for free. Reducing is [`reduce_quads`]'s job, once per macroblock.
#[derive(Clone, Copy)]
struct HalfAcc {
    sad: uint16x8_t,
    cur: uint16x8_t,
    refs: uint16x8_t,
    mad: uint8x16_t,
    sqsum: uint32x4_t,
    sqdiff: uint32x4_t,
}

/// The four quadrant totals of one macroblock, `[top-left, top-right, bottom-left,
/// bottom-right]`, from the two halves' 16-bit accumulators.
///
/// `uaddlp` folds each half's eight lanes into four 32-bit ones — lanes 0, 1 are
/// still the left quadrant and 2, 3 the right — and `addp` of the two halves folds
/// those into the four output words, in order.
///
/// Bounds: a half's lane holds at most `8 * 2 * 255 = 4080`, so a pair is 8160 and a
/// quadrant total 16320.
#[inline]
#[target_feature(enable = "neon")]
fn reduce_quads(top: uint16x8_t, bot: uint16x8_t) -> uint32x4_t {
    vpaddq_u32(vpaddlq_u16(top), vpaddlq_u16(bot))
}

/// `st1 {v.4s}` — one macroblock's four quadrant words.
#[inline]
#[target_feature(enable = "neon")]
fn st_quads(out: &mut [i32; 4], v: uint32x4_t) {
    // SAFETY: `out` is an array of exactly four `i32`, which is what `vst1q_s32` writes.
    unsafe { vst1q_s32(out.as_mut_ptr(), vreinterpretq_s32_u32(v)) }
}

/// The eight rows of one half-macroblock, from plane slices anchored at its
/// top-left sample.
///
/// `SAD_SD_MAD_8x16BYTES`, `SAD_SSD_BGD_8x16BYTES_1/_2`, `SAD_SSD_8x16BYTES_1/_2`
/// and `SAD_VAR_8x16BYTES_1/_2` are all this loop with a different subset of the
/// accumulators live, which the three flags select.
#[inline]
#[target_feature(enable = "neon")]
fn half_mb<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
) -> HalfAcc {
    // Trim both planes to exactly the eight rows this half reads, once, before the
    // loop: with an open tail LLVM cannot relate `k * stride` to the length and
    // re-checks every row.
    let cur = &cur[..7 * stride + 16];
    let refp = &refp[..7 * stride + 16];

    // One accumulator per quantity. The squared sums are the exception: `umull`
    // widens eight bytes at a time, so the low and high halves of a row produce two
    // `.8h` vectors that cannot be added before `uadalp` widens them again.
    let mut sad = vdupq_n_u16(0);
    let mut cur_sum = vdupq_n_u16(0);
    let mut ref_sum = vdupq_n_u16(0);
    let mut mad = vdupq_n_u8(0);
    let mut sqsum = [vdupq_n_u32(0); 2];
    let mut sqdiff = [vdupq_n_u32(0); 2];

    for k in 0..8 {
        let base = k * stride;
        let a = ld16(&cur[base..base + 16]);
        let b = ld16(&refp[base..base + 16]);
        let d = vabdq_u8(a, b);

        // `uadalp`: lanes 0..3 take bytes 0..7 — the left quadrant — and 4..7 the right.
        sad = vpadalq_u8(sad, d);
        if VAR || BGD {
            // The current picture's sample sum serves both `sum16x16` (the whole
            // macroblock) and `sd8x8` (per quadrant), so one accumulator answers both.
            cur_sum = vpadalq_u8(cur_sum, a);
        }
        if BGD {
            ref_sum = vpadalq_u8(ref_sum, b);
            mad = vmaxq_u8(mad, d);
        }
        if VAR {
            sqsum[0] = vpadalq_u16(sqsum[0], vmull_u8(vget_low_u8(a), vget_low_u8(a)));
            sqsum[1] = vpadalq_u16(sqsum[1], vmull_high_u8(a, a));
        }
        if SQDIFF {
            sqdiff[0] = vpadalq_u16(sqdiff[0], vmull_u8(vget_low_u8(d), vget_low_u8(d)));
            sqdiff[1] = vpadalq_u16(sqdiff[1], vmull_high_u8(d, d));
        }
    }

    HalfAcc {
        sad,
        cur: cur_sum,
        refs: ref_sum,
        mad,
        sqsum: vaddq_u32(sqsum[0], sqsum[1]),
        sqdiff: vaddq_u32(sqdiff[0], sqdiff[1]),
    }
}

/// One macroblock — the top half, then the bottom half eight rows down — reduced and
/// written straight into `out` at `mb`. Returns the macroblock's total SAD.
#[inline]
#[target_feature(enable = "neon")]
fn mb_stats<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
    mb: usize,
    out: &mut Outputs<'_>,
) -> i32 {
    // The window a macroblock reads, checked once: sixteen rows of sixteen bytes,
    // the last of which ends `15 * stride + 16` past the origin.
    let cur = &cur[..15 * stride + 16];
    let refp = &refp[..15 * stride + 16];
    let bottom = 8 * stride;
    let top = half_mb::<VAR, SQDIFF, BGD>(cur, refp, stride);
    let bot = half_mb::<VAR, SQDIFF, BGD>(&cur[bottom..], &refp[bottom..], stride);

    let sad = reduce_quads(top.sad, bot.sad);
    st_quads(&mut out.sad8x8[mb], sad);
    if VAR || BGD {
        let c = reduce_quads(top.cur, bot.cur);
        if VAR {
            // The four quadrant sums are the macroblock's 256 samples, once each.
            out.sum16x16[mb] = vaddvq_u32(c) as i32;
        }
        if BGD {
            let r = reduce_quads(top.refs, bot.refs);
            // `sum(cur) - sum(ref)` per quadrant in 32 bits; neither side exceeds
            // `64 * 255`.
            let sd = vsubq_s32(vreinterpretq_s32_u32(c), vreinterpretq_s32_u32(r));
            st_quads(&mut out.sd8x8[mb], vreinterpretq_u32_s32(sd));
            // Three `umaxp` fold `[top | bottom]` down to the four quadrant maxima
            // in bytes 0..4.
            let m = vpmaxq_u8(top.mad, bot.mad);
            let m = vpmaxq_u8(m, m);
            let m = vpmaxq_u8(m, m);
            out.mad8x8[mb] = low4(vget_low_u8(m));
        }
    }
    if VAR {
        out.sqsum16x16[mb] = vaddvq_u32(vaddq_u32(top.sqsum, bot.sqsum)) as i32;
    }
    if SQDIFF {
        out.sqdiff16x16[mb] = vaddvq_u32(vaddq_u32(top.sqdiff, bot.sqdiff)) as i32;
    }
    vaddvq_u32(sad) as i32
}

/// The picture walk, macroblock by macroblock, returning the frame's total SAD.
///
/// The same walk as `vaacalc.rs`'s `walk_picture`, step quirk included. The frame
/// total accumulates a macroblock's four quadrants at a time, in the vector unit.
#[inline]
#[target_feature(enable = "neon")]
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
            frame_sad += mb_stats::<VAR, SQDIFF, BGD>(
                &cur[mb_origin..],
                &refp[mb_origin..],
                stride,
                mb_index,
                out,
            );
            mb_index += 1;
            mb_origin += 16;
        }
        row_origin = mb_origin + step;
    }
    frame_sad
}

// ============================================================================
// The entry points, named as the kernels they replace
// ============================================================================

/// `VAACalcSad_AArch64_neon`, line 62.
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
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { walk::<false, false, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadVar_AArch64_neon`, line 504.
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
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { walk::<true, false, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadSsd_AArch64_neon`, line 402.
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
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { walk::<true, true, false>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadBgd_AArch64_neon`, line 120.
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
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { walk::<false, false, true>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

/// `VAACalcSadSsdBgd_AArch64_neon`, line 259.
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
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { walk::<true, true, true>(cur, refp, pic_width, pic_height, pic_stride, &mut out) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::vaacalc as reference;
    use crate::processing::vaacalc::vaa_span;

    /// Planes of exactly `vaa_span` bytes, so a kernel that reads one byte past what
    /// the walk may touch panics here instead of silently working.
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

    /// The geometries the encoder hands these kernels, plus strides wider than the
    /// width. Planes are exactly `vaa_span` long, so this is also the over-read test:
    /// a kernel that reads past the walk's last sample panics.
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
    /// correct it.
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
