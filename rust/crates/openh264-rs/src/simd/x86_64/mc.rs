//! SSE2 implementations of Motion Compensation (MC) kernels:
//! - Pixel averaging (`pixel_avg`)
//! - Chroma motion compensation (`mc_chroma`)
//! - Horizontal 6-tap Wiener filter (`mc_hor_ver20`)
//! - Vertical 6-tap Wiener filter (`mc_hor_ver02`)
//! - 2D center 6x6-tap Wiener filter (`mc_hor_ver22`)
//! - Luma quarter-pel motion compensation (`mc_luma`)
#![allow(unsafe_code)]

use core::arch::x86_64::*;
use crate::common::mc::{
    avg_shaped, cen_shaped, chroma_shaped, filter_input_8bit, g_kuiABCD, hor_filter_input_16bit, hor_shaped, mc_copy,
    ver_shaped, McLeaves, WelsClip1,
};
use crate::safe::plane::{BlockRows, PlaneCursorMut, RefSamples};
use crate::common::mc::mc_luma_with;

// ============================================================================
// Block shapes and lane moves
// ============================================================================

/// Rows per window cut — the twin of `simd::aarch64::mc::ROW_GROUP`, and for the
/// same reason: a filter body is well past the unroller's threshold at sixteen rows,
/// so `y * stride` stays symbolic and every per-row bounds check with it. One window
/// per group of four restores the constant offsets. See
/// [`PlaneSpanMut::window_mut`](crate::safe::plane::PlaneSpanMut::window_mut).
const ROW_GROUP: usize = 4;

/// Sixteen bytes of a span row as a vector.
#[target_feature(enable = "sse2")]
unsafe fn ld16(r: &[u8; 16]) -> __m128i {
    unsafe { _mm_loadu_si128(r.as_ptr() as *const __m128i) }
}

/// Eight bytes of a span row in the low half of a vector.
///
/// Through `i64` rather than `_mm_loadl_epi64` because [`BlockRows::row`] hands a row
/// over **by value**: a `[u8; 8]` is one integer register, and taking its address
/// would put it back on the stack for the load to read out again. Where the row does
/// come from memory the `from_le_bytes` folds back into the `movq` it would have
/// been.
#[target_feature(enable = "sse2")]
unsafe fn ld8(r: &[u8; 8]) -> __m128i {
    _mm_cvtsi64_si128(i64::from_le_bytes(*r))
}

/// Four bytes of a span row in the low quarter of a vector; see [`ld8`].
#[target_feature(enable = "sse2")]
unsafe fn ld4(r: &[u8; 4]) -> __m128i {
    _mm_cvtsi32_si128(i32::from_le_bytes(*r))
}

/// Sixteen bytes of `v` to the start of `out`.
#[target_feature(enable = "sse2")]
unsafe fn st16(out: &mut [u8], v: __m128i) {
    unsafe { _mm_storeu_si128(out[..16].as_mut_ptr() as *mut __m128i, v) }
}

/// The low eight bytes of `v` to the start of `out`; see [`ld8`].
#[target_feature(enable = "sse2")]
unsafe fn st8(out: &mut [u8], v: __m128i) {
    out[..8].copy_from_slice(&_mm_cvtsi128_si64(v).to_le_bytes())
}

/// The low four bytes of `v` to the start of `out`; see [`ld8`].
#[target_feature(enable = "sse2")]
unsafe fn st4(out: &mut [u8], v: __m128i) {
    out[..4].copy_from_slice(&_mm_cvtsi128_si32(v).to_le_bytes())
}

/// Eight bytes of a window row widened to eight words.
#[target_feature(enable = "sse2")]
unsafe fn w8<R: BlockRows>(r: &R, y: usize, x: usize) -> __m128i {
    unsafe { _mm_unpacklo_epi8(ld8(&r.row::<8>(y, x)), _mm_setzero_si128()) }
}

/// Four bytes of a window row widened to four words in the low half.
#[target_feature(enable = "sse2")]
unsafe fn w4<R: BlockRows>(r: &R, y: usize, x: usize) -> __m128i {
    unsafe { _mm_unpacklo_epi8(ld4(&r.row::<4>(y, x)), _mm_setzero_si128()) }
}

// ============================================================================
// Pixel Averaging (SSE2)
// ============================================================================

/// Rounded pixel average of two rows: `((a + b + 1) >> 1) as u8`, `pavgb`.
///
/// The width is a const parameter so the chunk chain below has a constant trip count
/// — a row loop whose body still contains a loop is one the unroller declines, and
/// with it every per-row bounds check stays. See [`ROW_GROUP`].
#[target_feature(enable = "sse2")]
unsafe fn avg_row<const W: usize>(out: &mut [u8; W], a: &[u8; W], b: &[u8; W]) {
    unsafe {
        let mut x = 0;
        while x + 16 <= W {
            st16(&mut out[x..], _mm_avg_epu8(ld16(a[x..][..16].try_into().unwrap()), ld16(b[x..][..16].try_into().unwrap())));
            x += 16;
        }
        if x + 8 <= W {
            st8(&mut out[x..], _mm_avg_epu8(ld8(a[x..][..8].try_into().unwrap()), ld8(b[x..][..8].try_into().unwrap())));
            x += 8;
        }
        if x + 4 <= W {
            st4(&mut out[x..], _mm_avg_epu8(ld4(a[x..][..4].try_into().unwrap()), ld4(b[x..][..4].try_into().unwrap())));
            x += 4;
        }
        while x < W {
            out[x] = (((a[x] as u32) + (b[x] as u32) + 1) >> 1) as u8;
            x += 1;
        }
    }
}

/// `PixelAvg` over one const-shape block: one span per operand, walked a
/// [`ROW_GROUP`] at a time.
#[target_feature(enable = "sse2")]
unsafe fn avg_block<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
) {
    unsafe {
        let sa = a.span::<W, H>(0, 0);
        let sb = b.span::<W, H>(0, 0);
        let mut d = dst.span_mut::<W, H>(0, 0);
        let mut y = 0;
        while y + ROW_GROUP <= H {
            let (ga, gb) = (sa.window::<W>(y, ROW_GROUP), sb.window::<W>(y, ROW_GROUP));
            let mut gd = d.window_mut::<W>(y, ROW_GROUP);
            for k in 0..ROW_GROUP {
                avg_row::<W>(gd.row_mut::<W>(k, 0), &ga.row::<W>(k, 0), &gb.row::<W>(k, 0));
            }
            y += ROW_GROUP;
        }
        while y < H {
            avg_row::<W>(d.row_mut::<W>(y, 0), &sa.row::<W>(y, 0), &sb.row::<W>(y, 0));
            y += 1;
        }
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`].
fn avg_any<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (j, o) in out.iter_mut().enumerate() {
            *o = (((a.at(j as isize, dy) as u32) + (b.at(j as isize, dy) as u32) + 1) >> 1) as u8;
        }
    }
}

/// Public safe entry point for SSE2 pixel averaging.
pub fn pixel_avg<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    avg_shaped::<Sse2Leaves, A, B>(dst, a, b, width, height)
}

// ============================================================================
// Chroma MC (SSE2)
// ============================================================================

/// One output row of the bilinear filter at width 8 or 4, over the two one-row
/// windows `r0` and `r1`.
#[target_feature(enable = "sse2")]
unsafe fn chroma_row<R: BlockRows, const W: usize>(
    out: &mut [u8; W],
    r0: &R,
    r1: &R,
    vA: __m128i,
    vB: __m128i,
    vC: __m128i,
    vD: __m128i,
) {
    unsafe {
        let (p00, p01, p10, p11) = if W == 8 {
            (w8(r0, 0, 0), w8(r0, 0, 1), w8(r1, 0, 0), w8(r1, 0, 1))
        } else {
            (w4(r0, 0, 0), w4(r0, 0, 1), w4(r1, 0, 0), w4(r1, 0, 1))
        };
        let sum = _mm_add_epi16(
            _mm_add_epi16(_mm_mullo_epi16(p00, vA), _mm_mullo_epi16(p01, vB)),
            _mm_add_epi16(_mm_mullo_epi16(p10, vC), _mm_mullo_epi16(p11, vD)),
        );
        let shifted = _mm_srli_epi16(_mm_add_epi16(sum, _mm_set1_epi16(32)), 6);
        let packed = _mm_packus_epi16(shifted, _mm_setzero_si128());
        if W == 8 {
            st8(out, packed);
        } else {
            st4(out, packed);
        }
    }
}

/// The bilinear chroma filter over one const-shape block. Widths 8 and 4 take the
/// lane path; width 2 is the scalar, as upstream has it.
#[target_feature(enable = "sse2")]
unsafe fn chroma_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    unsafe {
        let (iA, iB, iC, iD) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
        let s = src.span::<SW, SH>(0, 0);
        let mut d = dst.span_mut::<W, H>(0, 0);
        if W == 8 || W == 4 {
            let (vA, vB, vC, vD) = (
                _mm_set1_epi16(iA as i16),
                _mm_set1_epi16(iB as i16),
                _mm_set1_epi16(iC as i16),
                _mm_set1_epi16(iD as i16),
            );
            for y in 0..H {
                let (r0, r1) = (s.window::<SW>(y, 1), s.window::<SW>(y + 1, 1));
                chroma_row::<_, W>(d.row_mut::<W>(y, 0), &r0, &r1, vA, vB, vC, vD);
            }
        } else {
            for y in 0..H {
                let (r0, r1) = (s.row::<SW>(y, 0), s.row::<SW>(y + 1, 0));
                let out = d.row_mut::<W>(y, 0);
                for j in 0..W {
                    out[j] = ((iA * (r0[j] as i32)
                        + iB * (r0[j + 1] as i32)
                        + iC * (r1[j] as i32)
                        + iD * (r1[j + 1] as i32)
                        + 32)
                        >> 6) as u8;
                }
            }
        }
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`].
fn chroma_any<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
    width: usize,
    height: usize,
) {
    let (iA, iB, iC, iD) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (j, o) in out.iter_mut().enumerate() {
            let x = j as isize;
            *o = ((iA * (src.at(x, dy) as i32)
                + iB * (src.at(x + 1, dy) as i32)
                + iC * (src.at(x, dy + 1) as i32)
                + iD * (src.at(x + 1, dy + 1) as i32)
                + 32)
                >> 6) as u8;
        }
    }
}

/// Public safe entry point for SSE2 chroma MC.
#[inline]
pub fn mc_chroma<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if (mv_x & 0x07) == 0 && (mv_y & 0x07) == 0 {
        mc_copy(src, dst, width, height);
        return;
    }
    mc_chroma_frac(src, dst, mv_x, mv_y, width, height)
}

/// The fractional half of [`mc_chroma`], **out of line on purpose**.
///
/// The whole-sample vector is the common chroma case and it is a block copy; with the
/// bilinear dispatch in the same body the entry point was too large to inline, so
/// `mc_copy`'s width and height arrived as run-time values at a call site that had
/// them as constants, and the copy paid two jump tables it should not have. Split,
/// the entry point is a test and a copy — small enough to inline — and this is one
/// call on the path that does real work.
#[inline(never)]
fn mc_chroma_frac<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if width == 0 {
        return;
    }
    let w = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    chroma_shaped::<Sse2Leaves, S>(src, dst, w, width, height)
}

// ============================================================================
// 6-Tap Filter Helpers (SSE2)
// ============================================================================

/// Vectorized 6-tap Wiener filter on 8 samples:
/// `val = (p0 + p5) - 5 * (p1 + p4) + 20 * (p2 + p3)`
/// `res = WelsClip1((val + 16) >> 5)`
///
/// Implemented using the identity:
/// `x = 4 * (p2 + p3) - (p1 + p4)`
/// `val = (p0 + p5) + x + (x << 2)`
#[target_feature(enable = "sse2")]
unsafe fn filter_6tap_8_samples(
    p0: __m128i,
    p1: __m128i,
    p2: __m128i,
    p3: __m128i,
    p4: __m128i,
    p5: __m128i,
) -> __m128i {
    let p14 = _mm_add_epi16(p1, p4);
    let p23 = _mm_add_epi16(p2, p3);
    let x = _mm_sub_epi16(_mm_slli_epi16(p23, 2), p14);
    let p05 = _mm_add_epi16(p0, p5);
    let sum = _mm_add_epi16(p05, _mm_add_epi16(x, _mm_slli_epi16(x, 2)));
    let rounded = _mm_add_epi16(sum, _mm_set1_epi16(16));
    let shifted = _mm_srai_epi16(rounded, 5);
    _mm_packus_epi16(shifted, _mm_setzero_si128())
}

/// Computes the unclipped 16-bit intermediate for 2D filter:
/// `val = (p0 + p5) - 5 * (p1 + p4) + 20 * (p2 + p3)`
#[target_feature(enable = "sse2")]
unsafe fn filter_6tap_intermediate_8_samples(
    p0: __m128i,
    p1: __m128i,
    p2: __m128i,
    p3: __m128i,
    p4: __m128i,
    p5: __m128i,
) -> __m128i {
    let p14 = _mm_add_epi16(p1, p4);
    let p23 = _mm_add_epi16(p2, p3);
    let x = _mm_sub_epi16(_mm_slli_epi16(p23, 2), p14);
    let p05 = _mm_add_epi16(p0, p5);
    _mm_add_epi16(p05, _mm_add_epi16(x, _mm_slli_epi16(x, 2)))
}

// ============================================================================
// Horizontal 6-Tap Filter: McHorVer20 (SSE2)
// ============================================================================

/// One output row of the horizontal filter at row `y` of the window `r`, which
/// starts at `x = -2` and holds `W + 5` bytes.
///
/// `AVG` is 0, or the tap the result is averaged with — 2 for quarter-pel `(1, 0)`
/// and 3 for `(3, 0)`; see [`McLeaves`]. A macro rather than a function for the
/// reason `simd::aarch64::mc`'s twin is one: the row index has to stay a constant at
/// the point the bounds checks are decided, and a `#[target_feature]` function
/// cannot be `#[inline(always)]`.
macro_rules! hor_row {
    ($out:expr, $r:expr, $y:expr, $avg:expr) => {{
        let out: &mut [u8; W] = $out;
        let mut col = 0;
        while col + 8 <= W {
            let mut v = filter_6tap_8_samples(
                w8($r, $y, col),
                w8($r, $y, col + 1),
                w8($r, $y, col + 2),
                w8($r, $y, col + 3),
                w8($r, $y, col + 4),
                w8($r, $y, col + 5),
            );
            if $avg != 0 {
                v = _mm_avg_epu8(v, ld8(&$r.row::<8>($y, col + $avg)));
            }
            st8(&mut out[col..], v);
            col += 8;
        }
        if col + 4 <= W {
            let mut v = filter_6tap_8_samples(
                w4($r, $y, col),
                w4($r, $y, col + 1),
                w4($r, $y, col + 2),
                w4($r, $y, col + 3),
                w4($r, $y, col + 4),
                w4($r, $y, col + 5),
            );
            if $avg != 0 {
                v = _mm_avg_epu8(v, ld4(&$r.row::<4>($y, col + $avg)));
            }
            st4(&mut out[col..], v);
            col += 4;
        }
        while col < W {
            let t = $r.row::<6>($y, col);
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if $avg != 0 {
                v = ((v as u32 + t[$avg] as u32 + 1) >> 1) as u8;
            }
            out[col] = v;
            col += 1;
        }
    }};
}

/// `McHorVer20` over one const-shape block: one span for the source, one for the
/// destination, and a window per [`ROW_GROUP`] rows.
#[target_feature(enable = "sse2")]
unsafe fn hor_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    unsafe {
        let s = src.span::<SW, H>(0, -2);
        let mut d = dst.span_mut::<W, H>(0, 0);
        let mut y = 0;
        while y + ROW_GROUP <= H {
            let g = s.window::<SW>(y, ROW_GROUP);
            let mut gd = d.window_mut::<W>(y, ROW_GROUP);
            for k in 0..ROW_GROUP {
                hor_row!(gd.row_mut::<W>(k, 0), &g, k, AVG);
            }
            y += ROW_GROUP;
        }
        while y < H {
            let g = s.window::<SW>(y, 1);
            hor_row!(d.row_mut::<W>(y, 0), &g, 0, AVG);
            y += 1;
        }
    }
}

/// The run-time-shape twin — cold, and scalar; see [`McLeaves`].
fn hor_any<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let t: [u8; 6] = std::array::from_fn(|k| src.at(x as isize + k as isize - 2, dy));
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + t[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// Public safe entry point for SSE2 horizontal half-pel filter.
pub fn mc_hor_ver20<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    hor_shaped::<Sse2Leaves, S, 0>(src, dst, width, height)
}

// ============================================================================
// Vertical 6-Tap Filter: McHorVer02 (SSE2)
// ============================================================================

/// The vertical filter at width 16, 8 or 4: the five-row window carried in widened
/// registers and one new row read per output row.
#[target_feature(enable = "sse2")]
unsafe fn ver_lanes<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    unsafe {
        let s = src.span::<W, SH>(-2, 0);
        let mut d = dst.span_mut::<W, H>(0, 0);
        // `[lo, hi]` per row; the high half is idle below width 16.
        let row = |y: usize| -> [__m128i; 2] {
            if W == 16 {
                [w8(&s, y, 0), w8(&s, y, 8)]
            } else if W == 8 {
                [w8(&s, y, 0), _mm_setzero_si128()]
            } else {
                [w4(&s, y, 0), _mm_setzero_si128()]
            }
        };
        let (mut r0, mut r1, mut r2, mut r3, mut r4) = (row(0), row(1), row(2), row(3), row(4));
        for y in 0..H {
            let r5 = row(y + 5);
            let mut v = filter_6tap_8_samples(r0[0], r1[0], r2[0], r3[0], r4[0], r5[0]);
            let out = d.row_mut::<W>(y, 0);
            if W == 16 {
                let hi = filter_6tap_8_samples(r0[1], r1[1], r2[1], r3[1], r4[1], r5[1]);
                if AVG != 0 {
                    let tap = ld16(&s.row::<16>(y + AVG, 0));
                    let both = _mm_avg_epu8(_mm_unpacklo_epi64(v, hi), tap);
                    st16(out, both);
                } else {
                    st8(&mut out[..], v);
                    st8(&mut out[8..], hi);
                }
            } else if W == 8 {
                if AVG != 0 {
                    v = _mm_avg_epu8(v, ld8(&s.row::<8>(y + AVG, 0)));
                }
                st8(out, v);
            } else {
                if AVG != 0 {
                    v = _mm_avg_epu8(v, ld4(&s.row::<4>(y + AVG, 0)));
                }
                st4(out, v);
            }
            (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
        }
    }
}

/// The widths the lane path has no form for: the scalar over the same span.
#[target_feature(enable = "sse2")]
unsafe fn ver_odd<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, SH>(-2, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let out = d.row_mut::<W>(y, 0);
        for (x, o) in out.iter_mut().enumerate() {
            let t: [u8; 6] = std::array::from_fn(|k| s.row::<1>(y + k, x)[0]);
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + t[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// `McHorVer02` over one const-shape block: the width picks the path, and the
/// `match` folds because `W` is a constant.
#[target_feature(enable = "sse2")]
unsafe fn ver_block<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    unsafe {
        match W {
            16 | 8 | 4 => ver_lanes::<S, W, H, SH, AVG>(src, dst),
            _ => ver_odd::<S, W, H, SH, AVG>(src, dst),
        }
    }
}

/// The run-time-shape twin — cold, and scalar; see [`McLeaves`].
fn ver_any<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let t: [u8; 6] = std::array::from_fn(|k| src.at(x as isize, dy + k as isize - 2));
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + t[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// Public safe entry point for SSE2 vertical half-pel filter.
pub fn mc_hor_ver02<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    ver_shaped::<Sse2Leaves, S, 0>(src, dst, width, height)
}

// ============================================================================
// 2D Center 6x6-Tap Filter: McHorVer22 (SSE2)
// ============================================================================

/// `McHorVer22` over one const-shape block: the vertical 6-tap into `iTmp` over a
/// six-row window per output row, then the scalar horizontal pass over those.
///
/// `iTmp` is `[i16; 17 + 5]` as in the C++ (`int16_t iTmp[17 + 5]` against
/// `for (j = 0; j < iWidth + 5; j++)`), which is what bounds `SW` at 22 — and the
/// vertical pass below stores through a raw pointer, so a wider `SW` would run off
/// the stack frame rather than panic. [`cen_shaped`] only instantiates the shapes
/// the codec calls; [`cen_any`] states the bound for everything else.
#[target_feature(enable = "sse2")]
unsafe fn cen_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    unsafe {
        const { assert!(SW <= 17 + 5, "mc_hor_ver22 width exceeds the 17 iTmp is sized for") };
        let s = src.span::<SW, SH>(-2, -2);
        let mut d = dst.span_mut::<W, H>(0, 0);
        let mut iTmp = [0i16; 17 + 5];
        for y in 0..H {
            // The six tap rows as one window: two checks, after which every row and
            // column offset inside it is a constant and folds.
            let g = s.window::<SW>(y, 6);

            // Step 1: Vertical 6-tap filter into iTmp
            let mut j = 0;
            while j + 8 <= SW {
                let res = filter_6tap_intermediate_8_samples(
                    w8(&g, 0, j),
                    w8(&g, 1, j),
                    w8(&g, 2, j),
                    w8(&g, 3, j),
                    w8(&g, 4, j),
                    w8(&g, 5, j),
                );
                _mm_storeu_si128(iTmp[j..][..8].as_mut_ptr() as *mut __m128i, res);
                j += 8;
            }
            if j + 4 <= SW {
                let res = filter_6tap_intermediate_8_samples(
                    w4(&g, 0, j),
                    w4(&g, 1, j),
                    w4(&g, 2, j),
                    w4(&g, 3, j),
                    w4(&g, 4, j),
                    w4(&g, 5, j),
                );
                _mm_storel_epi64(iTmp[j..][..4].as_mut_ptr() as *mut __m128i, res);
                j += 4;
            }
            while j < SW {
                let t: [u8; 6] = std::array::from_fn(|k| g.row::<1>(k, j)[0]);
                iTmp[j] = filter_input_8bit(&t) as i16;
                j += 1;
            }

            // Step 2: Horizontal 6-tap filter over 16-bit intermediate iTmp
            let out = d.row_mut::<W>(y, 0);
            for (o, t) in out.iter_mut().zip(iTmp[..SW].windows(6)) {
                *o = WelsClip1((hor_filter_input_16bit(t.try_into().expect("six taps")) + 512) >> 10);
            }
        }
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`]. The `width <= 17` contract is
/// the C++'s and is what sizes `iTmp`.
fn cen_any<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    assert!(width <= 17, "mc_hor_ver22 width {width} exceeds the 17 iTmp is sized for");
    let n = width + 5;
    let mut iTmp = [0i16; 17 + 5];
    for dy in 0..height as isize {
        for (j, t) in iTmp[..n].iter_mut().enumerate() {
            let x = j as isize - 2;
            let p: [u8; 6] = std::array::from_fn(|k| src.at(x, dy + k as isize - 2));
            *t = filter_input_8bit(&p) as i16;
        }
        let out = dst.row_mut(dy, 0, width);
        for (o, t) in out.iter_mut().zip(iTmp[..n].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(t.try_into().expect("six taps")) + 512) >> 10);
        }
    }
}

/// Public safe entry point for SSE2 center half-pel filter.
pub fn mc_hor_ver22<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    cen_shaped::<Sse2Leaves, S>(src, dst, width, height)
}

// ============================================================================
// Luma Quarter-Pel MC (SSE2)
// ============================================================================

/// **The SSE2 leaf set** — `McLeaves` with the four real kernels in this file.
///
/// The twelve quarter-pel composites live once, in `common/mc.rs`; this is the whole of
/// the SSE2 side of them. See [`crate::common::mc::McLeaves`].
pub struct Sse2Leaves;

impl McLeaves for Sse2Leaves {
    #[inline(always)]
    fn hor<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { hor_block::<S, W, SW, H, AVG>(src, dst) }
    }
    #[inline(always)]
    fn hor_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    ) {
        hor_any::<S, AVG>(src, dst, width, height)
    }
    #[inline(always)]
    fn ver<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { ver_block::<S, W, H, SH, AVG>(src, dst) }
    }
    #[inline(always)]
    fn ver_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    ) {
        ver_any::<S, AVG>(src, dst, width, height)
    }
    #[inline(always)]
    fn cen<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { cen_block::<S, W, SW, H, SH>(src, dst) }
    }
    #[inline(always)]
    fn cen_any<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        cen_any::<S>(src, dst, width, height)
    }
    #[inline(always)]
    fn avg<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { avg_block::<A, B, W, H>(dst, a, b) }
    }
    #[inline(always)]
    fn avg_any<A: RefSamples, B: RefSamples>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
        width: usize,
        height: usize,
    ) {
        avg_any::<A, B>(dst, a, b, width, height)
    }
    #[inline(always)]
    fn chroma<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { chroma_block::<S, W, SW, H, SH>(src, dst, w) }
    }
    #[inline(always)]
    fn chroma_any<S: RefSamples + Copy>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
        width: usize,
        height: usize,
    ) {
        chroma_any::<S>(src, dst, w, width, height)
    }
}

/// Public safe entry point for SSE2 luma quarter-pel MC.
pub fn mc_luma<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    mc_luma_with::<Sse2Leaves, S>(src, dst, mv_x, mv_y, width, height)
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::safe::plane::PlaneCursor;
    use super::*;
    // These MUST be the `_c` scalar kernels, not the same-named dispatchers:
    // the dispatchers route to the very SSE2 kernels under test, which would
    // make every assertion below a tautology.
    use crate::common::mc::{
        mc_chroma_with_frag_mv, mc_hor_ver02_c as scalar_hor_ver02,
        mc_hor_ver20_c as scalar_hor_ver20, mc_hor_ver22_c as scalar_hor_ver22,
        mc_luma_c as scalar_luma, pixel_avg_c as scalar_pixel_avg,
    };
    use crate::encoder::rec_view::RecCursor;
    use crate::common::mc::McLeaves;

    const STRIDE: usize = 64;
    const ROWS: usize = 64;

    fn filled_plane() -> Vec<u8> {
        let mut v = vec![0u8; STRIDE * ROWS];
        let mut s: u32 = 0xdead_beef;
        for b in v.iter_mut() {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *b = (s >> 16) as u8;
        }
        v
    }

    #[test]
    fn test_pixel_avg_parity() {
        let a = filled_plane();
        let mut b = a.clone();
        for x in b.iter_mut() {
            *x = x.wrapping_add(42);
        }

        let ca = PlaneCursor::new(&a, 10 * STRIDE + 8, STRIDE);
        let cb = PlaneCursor::new(&b, 12 * STRIDE + 8, STRIDE);

        for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4), (17, 16), (9, 8), (5, 4), (2, 2)] {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, 10 * STRIDE + 8, STRIDE);
            scalar_pixel_avg(&mut cur_scalar, &ca, &cb, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, 10 * STRIDE + 8, STRIDE);
            pixel_avg(&mut cur_simd, &ca, &cb, w, h);

            assert_eq!(dst_scalar, dst_simd, "pixel_avg mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_chroma_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [(8, 8), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)];

        for &(w, h) in &shapes {
            for dy in 0..8i16 {
                for dx in 0..8i16 {
                    let mut dst_scalar = vec![0u8; STRIDE * ROWS];
                    let mut dst_simd = vec![0u8; STRIDE * ROWS];

                    let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
                    if (dx & 7) == 0 && (dy & 7) == 0 {
                        mc_copy(&src, &mut cur_scalar, w, h);
                    } else {
                        mc_chroma_with_frag_mv(&src, &mut cur_scalar, dx, dy, w, h);
                    }

                    let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
                    mc_chroma(&src, &mut cur_simd, dx, dy, w, h);

                    assert_eq!(
                        dst_scalar, dst_simd,
                        "mc_chroma mismatch at {w}x{h} with mv=({dx}, {dy})"
                    );
                }
            }
        }
    }

    #[test]
    fn test_mc_hor_ver20_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (17, 16), (17, 8), (9, 16), (9, 8),
            // Outside the const tables, so these drive the run-time fallback.
            (5, 8), (5, 4), (17, 17),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver20(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver20(&src, &mut cur_simd, w, h);

            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver20 mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_hor_ver02_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (16, 17), (16, 9), (8, 17), (8, 9),
            // Outside the const tables, so these drive the run-time fallback.
            (8, 5), (4, 5), (17, 17),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver02(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver02(&src, &mut cur_simd, w, h);

            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver02 mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_hor_ver22_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (17, 17), (17, 9), (9, 17), (9, 9),
            // Outside the const tables, so these drive the run-time fallback.
            (9, 5), (5, 5), (17, 16),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver22(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver22(&src, &mut cur_simd, w, h);

            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver22 mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_luma_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)];

        for &(w, h) in &shapes {
            for qy in 0..4i16 {
                for qx in 0..4i16 {
                    let mut dst_scalar = vec![0u8; STRIDE * ROWS];
                    let mut dst_simd = vec![0u8; STRIDE * ROWS];

                    let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
                    scalar_luma(&src, &mut cur_scalar, qx, qy, w, h);

                    let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
                    mc_luma(&src, &mut cur_simd, qx, qy, w, h);

                    assert_eq!(
                        dst_scalar, dst_simd,
                        "mc_luma mismatch at {w}x{h} with qpos=({qx}, {qy})"
                    );
                }
            }
        }
    }
    /// The same kernels reached through the shared cursor, whose rows arrive by
    /// value: the encoder's reference picture is one of these, and it is what the
    /// half-pel refinement filters read at `kiW + 1` by `kiH + 1`.
    #[test]
    fn mc_parity_through_the_shared_cursor() {
        let mut base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let mut want = vec![0u8; STRIDE * ROWS];
        let mut got = vec![0u8; STRIDE * ROWS];
        macro_rules! pair {
            ($scalar:expr, $simd:expr) => {{
                want.iter_mut().for_each(|b| *b = 0);
                got.iter_mut().for_each(|b| *b = 0);
                {
                    let src = PlaneCursor::new(&base, src_c, STRIDE);
                    let mut d = PlaneCursorMut::new(&mut want, dst_c, STRIDE);
                    #[allow(clippy::redundant_closure_call)]
                    ($scalar)(&src, &mut d);
                }
                {
                    let src = RecCursor::over_owned(&mut base, src_c, STRIDE);
                    let mut d = PlaneCursorMut::new(&mut got, dst_c, STRIDE);
                    #[allow(clippy::redundant_closure_call)]
                    ($simd)(&src, &mut d);
                }
                assert_eq!(want, got);
            }};
        }
        for (qx, qy) in [(0i16, 0i16), (1, 0), (2, 0), (3, 0), (0, 1), (0, 2), (0, 3), (1, 1), (2, 2), (3, 3)] {
            for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)] {
                pair!(
                    |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_luma(s, d, qx, qy, w, h),
                    |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_luma(s, d, qx, qy, w, h)
                );
            }
        }
        // The half-pel refinement shapes, which only ever run over this cursor.
        for (w, h) in [(17, 16), (17, 8), (9, 16), (9, 8)] {
            pair!(
                |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_hor_ver20(s, d, w, h),
                |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_hor_ver20(s, d, w, h)
            );
        }
        for (w, h) in [(16, 17), (16, 9), (8, 17), (8, 9)] {
            pair!(
                |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_hor_ver02(s, d, w, h),
                |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_hor_ver02(s, d, w, h)
            );
        }
        for (w, h) in [(17, 17), (17, 9), (9, 17), (9, 9)] {
            pair!(
                |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_hor_ver22(s, d, w, h),
                |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_hor_ver22(s, d, w, h)
            );
        }
        for (w, h) in [(8, 8), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)] {
            for (mx, my) in [(0i16, 0i16), (3, 5), (1, 0), (0, 7)] {
                pair!(
                    |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| {
                        if (mx & 7) == 0 && (my & 7) == 0 {
                            mc_copy(s, d, w, h)
                        } else {
                            mc_chroma_with_frag_mv(s, d, mx, my, w, h)
                        }
                    },
                    |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_chroma(s, d, mx, my, w, h)
                );
            }
        }
        // `pixel_avg`'s second operand is the reference block itself in four of the
        // quarter-pixel candidates, so it too sees this cursor.
        for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8)] {
            let a = filled_plane();
            want.iter_mut().for_each(|b| *b = 0);
            got.iter_mut().for_each(|b| *b = 0);
            {
                let ca = PlaneCursor::new(&a, src_c, STRIDE);
                let cb = PlaneCursor::new(&base, src_c, STRIDE);
                scalar_pixel_avg(&mut PlaneCursorMut::new(&mut want, dst_c, STRIDE), &ca, &cb, w, h);
            }
            {
                let ca = PlaneCursor::new(&a, src_c, STRIDE);
                let cb = RecCursor::over_owned(&mut base, src_c, STRIDE);
                pixel_avg(&mut PlaneCursorMut::new(&mut got, dst_c, STRIDE), &ca, &cb, w, h);
            }
            assert_eq!(want, got, "pixel_avg via RecCursor at {w}x{h}");
        }
    }

    /// **The `_AVERAGE_WITH_` forms of the two direct filters agree with the
    /// composites they would replace.**
    ///
    /// [`McLeaves::FUSED_QPEL`] is off for this set, so `mc_luma` takes the composite
    /// at quarter-pel `(1, 0)`, `(3, 0)`, `(0, 1)` and `(0, 3)` and the `AVG` arms of
    /// [`McLeaves::hor`] and [`McLeaves::ver`] are never instantiated by the codec.
    /// They are still part of the trait and still have to be right, so drive them
    /// here: the fused form is a rounded average against one of the filter's own
    /// taps, and that is what the composite computes with an averaging pass.
    #[test]
    fn the_fused_quarter_pel_arms_agree_with_the_composites() {
        let base = filled_plane();
        let src = PlaneCursor::new(&base, 10 * STRIDE + 10, STRIDE);
        let dst_c = 20 * STRIDE + 10;
        for (qx, qy, avg) in [(1i16, 0i16, 2usize), (3, 0, 3), (0, 1, 2), (0, 3, 3)] {
            let mut want = vec![0u8; STRIDE * ROWS];
            let mut got = vec![0u8; STRIDE * ROWS];
            mc_luma(&src, &mut PlaneCursorMut::new(&mut want, dst_c, STRIDE), qx, qy, 16, 16);
            {
                let mut d = PlaneCursorMut::new(&mut got, dst_c, STRIDE);
                match avg {
                    2 if qy == 0 => Sse2Leaves::hor::<_, 16, 21, 16, 2>(&src, &mut d),
                    3 if qy == 0 => Sse2Leaves::hor::<_, 16, 21, 16, 3>(&src, &mut d),
                    2 => Sse2Leaves::ver::<_, 16, 16, 21, 2>(&src, &mut d),
                    _ => Sse2Leaves::ver::<_, 16, 16, 21, 3>(&src, &mut d),
                }
            }
            assert_eq!(want, got, "fused quarter-pel ({qx}, {qy}) differs from the composite");
        }
    }
}
