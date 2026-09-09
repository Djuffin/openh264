//! Motion compensation on `wide` lane types — the twin of `simd::x86_64::mc`:
//! pixel averaging, chroma MC, the three half-pel Wiener filters and the quarter-pel
//! luma composite built from them.
//!
//! # What is emulated
//!
//! `pavgb` — the rounded byte average — has no `wide` wrapper and `u8x16` has no
//! shift, so [`avg_u8`] uses the identity `(a + b + 1) >> 1 == (a | b) - ((a ^ b) >> 1)`
//! with the halving done as a word shift and the bit that crosses into the next
//! byte masked off: five ops for the intrinsic's one.
//!
//! Everything else the filters need — word add, sub, shift, multiply, `packuswb` —
//! is a direct `wide` operation.

#![forbid(unsafe_code)]

use wide::bytemuck::cast;
use wide::{i16x8, u16x8, u8x16};

use super::lanes::{load16, load8, load_w, low4, low8, narrow, store_w, widen_hi, widen_lo};
use crate::common::mc::{
    avg_shaped, cen_shaped, chroma_shaped, filter_input_8bit, g_kuiABCD, hor_filter_input_16bit, hor_shaped, mc_copy,
    ver_shaped, McLeaves, WelsClip1,
};
use crate::safe::plane::{BlockRows, PlaneCursor, PlaneCursorMut, RefSamples};

// ============================================================================
// Block shapes
// ============================================================================

/// Rows per window cut — the twin of `simd::aarch64::mc::ROW_GROUP`, and for the
/// same reason: a filter body is well past the unroller's threshold at sixteen rows,
/// so `y * stride` stays symbolic and every per-row bounds check with it. One window
/// per group of four restores the constant offsets. See
/// [`PlaneSpanMut::window_mut`](crate::safe::plane::PlaneSpanMut::window_mut).
const ROW_GROUP: usize = 4;

// ============================================================================
// Pixel averaging
// ============================================================================

/// `((a + b + 1) >> 1)` per byte — see the module header.
#[inline(always)]
fn avg_u8(a: u8x16, b: u8x16) -> u8x16 {
    let x: u16x8 = cast(a ^ b);
    let half: u8x16 = cast((x >> 1i32) & u16x8::splat(0x7F7F));
    (a | b) - half
}

#[inline(always)]
fn avg_row<const W: usize>(out: &mut [u8; W], a: &[u8; W], b: &[u8; W]) {
    let mut x = 0;
    while x + 16 <= W {
        let v = avg_u8(load16(&a[x..]), load16(&b[x..]));
        out[x..x + 16].copy_from_slice(&v.to_array());
        x += 16;
    }
    if x + 8 <= W {
        let v = avg_u8(load8(&a[x..]), load8(&b[x..]));
        out[x..x + 8].copy_from_slice(&low8(v));
        x += 8;
    }
    if x + 4 <= W {
        let v = avg_u8(load_w::<4>(&a[x..]), load_w::<4>(&b[x..]));
        out[x..x + 4].copy_from_slice(&low4(v));
        x += 4;
    }
    while x < W {
        out[x] = ((a[x] as u32 + b[x] as u32 + 1) >> 1i32) as u8;
        x += 1;
    }
}

/// `PixelAvg` over one const-shape block: one span per operand, walked a
/// [`ROW_GROUP`] at a time.
#[inline(always)]
fn avg_block<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
) {
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
            *o = ((a.at(j as isize, dy) as u32 + b.at(j as isize, dy) as u32 + 1) >> 1i32) as u8;
        }
    }
}

pub fn pixel_avg<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    avg_shaped::<WideLeaves, A, B>(dst, a, b, width, height)
}

// ============================================================================
// Chroma MC
// ============================================================================

/// One output row of `W` samples from the bilinear taps `[A B; C D]` over the two
/// source rows of a one-row window pair. Sums peak at `64 * 255`, inside `i16`.
#[inline(always)]
fn chroma_row<R: BlockRows, const W: usize>(out: &mut [u8; W], r0: &R, r1: &R, w: [i16x8; 4]) {
    let s = widen_lo(load_w::<W>(&r0.row::<W>(0, 0))) * w[0]
        + widen_lo(load_w::<W>(&r0.row::<W>(0, 1))) * w[1]
        + widen_lo(load_w::<W>(&r1.row::<W>(0, 0))) * w[2]
        + widen_lo(load_w::<W>(&r1.row::<W>(0, 1))) * w[3];
    let v = (s + i16x8::splat(32)) >> 6i32;
    store_w::<W>(out, narrow(v, i16x8::ZERO));
}

/// The bilinear chroma filter over one const-shape block. Widths 8 and 4 take the
/// lane path; width 2 is scalar, as the intrinsic kernels have it.
#[inline(always)]
fn chroma_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    let (iA, iB, iC, iD) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    let s = src.span::<SW, SH>(0, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    if W == 8 || W == 4 {
        let lanes = [
            i16x8::splat(iA as i16),
            i16x8::splat(iB as i16),
            i16x8::splat(iC as i16),
            i16x8::splat(iD as i16),
        ];
        for y in 0..H {
            let (r0, r1) = (s.window::<SW>(y, 1), s.window::<SW>(y + 1, 1));
            chroma_row::<_, W>(d.row_mut::<W>(y, 0), &r0, &r1, lanes);
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
                    >> 6i32) as u8;
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
                >> 6i32) as u8;
        }
    }
}

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
    chroma_shaped::<WideLeaves, S>(src, dst, w, width, height)
}

// ============================================================================
// The 6-tap filter
// ============================================================================

/// `(p0 + p5) - 5 * (p1 + p4) + 20 * (p2 + p3)`, unrounded, as
/// `x = 4 * (p2 + p3) - (p1 + p4); val = (p0 + p5) + x + 4 * x`.
#[inline(always)]
fn filter_6tap_intermediate(p0: i16x8, p1: i16x8, p2: i16x8, p3: i16x8, p4: i16x8, p5: i16x8) -> i16x8 {
    let p14 = p1 + p4;
    let p23 = p2 + p3;
    let x = (p23 << 2i32) - p14;
    (p0 + p5) + x + (x << 2i32)
}

/// `WelsClip1((val + 16) >> 5)` as words, ready to `narrow`.
#[inline(always)]
fn filter_6tap_shifted(p0: i16x8, p1: i16x8, p2: i16x8, p3: i16x8, p4: i16x8, p5: i16x8) -> i16x8 {
    (filter_6tap_intermediate(p0, p1, p2, p3, p4, p5) + i16x8::splat(16)) >> 5i32
}

// ============================================================================
// Horizontal: McHorVer20
// ============================================================================

/// The six horizontal taps of `N` samples starting at `col` of a one-row window, as
/// words.
///
/// The taps are read as `N`-wide rows of the window rather than sliced out of one
/// `W + 5`-wide row: [`BlockRows::row`] hands a row over by value, so a row wider
/// than the load would be spilled to the stack and read back six times, where inside
/// a window each tap's offset is a constant and the load goes straight to the plane
/// or the cell view.
#[inline(always)]
fn htaps<R: BlockRows, const N: usize>(r: &R, col: usize) -> [i16x8; 6] {
    core::array::from_fn(|k| widen_lo(load_w::<N>(&r.row::<N>(0, col + k))))
}

/// One output row of the horizontal filter over the one-row window `r`.
///
/// `AVG` is 0, or the tap the result is averaged with — 2 for quarter-pel `(1, 0)`
/// and 3 for `(3, 0)`; see [`McLeaves`].
#[inline(always)]
fn hor_row<R: BlockRows, const W: usize, const AVG: usize>(out: &mut [u8; W], r: &R) {
    let mut col = 0;
    while col + 8 <= W {
        let [p0, p1, p2, p3, p4, p5] = htaps::<_, 8>(r, col);
        let mut v = narrow(filter_6tap_shifted(p0, p1, p2, p3, p4, p5), i16x8::ZERO);
        if AVG != 0 {
            v = avg_u8(v, load_w::<8>(&r.row::<8>(0, col + AVG)));
        }
        store_w::<8>(&mut out[col..], v);
        col += 8;
    }
    if col + 4 <= W {
        let [p0, p1, p2, p3, p4, p5] = htaps::<_, 4>(r, col);
        let mut v = narrow(filter_6tap_shifted(p0, p1, p2, p3, p4, p5), i16x8::ZERO);
        if AVG != 0 {
            v = avg_u8(v, load_w::<4>(&r.row::<4>(0, col + AVG)));
        }
        store_w::<4>(&mut out[col..], v);
        col += 4;
    }
    while col < W {
        let w = r.row::<6>(0, col);
        let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5i32);
        if AVG != 0 {
            v = ((v as u32 + w[AVG] as u32 + 1) >> 1i32) as u8;
        }
        out[col] = v;
        col += 1;
    }
}

/// `McHorVer20` over one const-shape block; see [`ROW_GROUP`].
#[inline(always)]
fn hor_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<SW, H>(0, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let mut y = 0;
    while y + ROW_GROUP <= H {
        let mut gd = d.window_mut::<W>(y, ROW_GROUP);
        for k in 0..ROW_GROUP {
            hor_row::<_, W, AVG>(gd.row_mut::<W>(k, 0), &s.window::<SW>(y + k, 1));
        }
        y += ROW_GROUP;
    }
    while y < H {
        hor_row::<_, W, AVG>(d.row_mut::<W>(y, 0), &s.window::<SW>(y, 1));
        y += 1;
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
            let w: [u8; 6] = core::array::from_fn(|k| src.at(x as isize + k as isize - 2, dy));
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5i32);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1i32) as u8;
            }
            *o = v;
        }
    }
}

pub fn mc_hor_ver20<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    hor_shaped::<WideLeaves, S, 0>(src, dst, width, height)
}

// ============================================================================
// Vertical: McHorVer02
// ============================================================================

/// A source row of `N` samples as words: the low eight in `[0]`, and for `N == 16`
/// the high eight in `[1]`.
#[inline(always)]
fn vrow<const N: usize>(r: &[u8; N]) -> [i16x8; 2] {
    let v = load_w::<N>(r);
    [widen_lo(v), if N == 16 { widen_hi(v) } else { i16x8::ZERO }]
}

/// The vertical filter at one width, with the five-row window carried in registers
/// and one new row read per output row, as the intrinsic kernel does.
#[inline(always)]
fn ver_lanes<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, SH>(-2, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let mut win: [[i16x8; 2]; 5] = core::array::from_fn(|k| vrow::<W>(&s.row::<W>(k, 0)));
    let mut y = 0;
    while y < H {
        let raw = s.row::<W>(y + 5, 0);
        let r5 = vrow::<W>(&raw);
        let lo = filter_6tap_shifted(win[0][0], win[1][0], win[2][0], win[3][0], win[4][0], r5[0]);
        let hi = if W == 16 {
            filter_6tap_shifted(win[0][1], win[1][1], win[2][1], win[3][1], win[4][1], r5[1])
        } else {
            i16x8::ZERO
        };
        let mut v = narrow(lo, hi);
        if AVG != 0 {
            v = avg_u8(v, load_w::<W>(&s.row::<W>(y + AVG, 0)));
        }
        store_w::<W>(d.row_mut::<W>(y, 0), v);
        win = [win[1], win[2], win[3], win[4], r5];
        y += 1;
    }
}

/// The widths the lane path has no form for: the scalar over the same span.
#[inline(always)]
fn ver_odd<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, SH>(-2, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let out = d.row_mut::<W>(y, 0);
        for (x, o) in out.iter_mut().enumerate() {
            let w: [u8; 6] = core::array::from_fn(|k| s.row::<1>(y + k, x)[0]);
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5i32);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1i32) as u8;
            }
            *o = v;
        }
    }
}

/// `McHorVer02` over one const-shape block: the width picks the path, and the
/// `match` folds because `W` is a constant.
#[inline(always)]
fn ver_block<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    match W {
        16 | 8 | 4 => ver_lanes::<S, W, H, SH, AVG>(src, dst),
        _ => ver_odd::<S, W, H, SH, AVG>(src, dst),
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
            let w: [u8; 6] = core::array::from_fn(|k| src.at(x as isize, dy + k as isize - 2));
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5i32);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1i32) as u8;
            }
            *o = v;
        }
    }
}

pub fn mc_hor_ver02<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    ver_shaped::<WideLeaves, S, 0>(src, dst, width, height)
}

// ============================================================================
// Centre: McHorVer22
// ============================================================================

/// `McHorVer22` over one const-shape block: the vertical 6-tap into 16-bit
/// intermediates over one `SW`-wide window per row, then the scalar horizontal pass
/// over those, as the intrinsic kernel has it.
#[inline(always)]
fn cen_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<SW, SH>(-2, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    // `iTmp` is the C++'s `int16_t[17 + 5]`, which is what bounds `SW` at 22.
    let mut iTmp = [0i16; 17 + 5];
    for y in 0..H {
        let r: [_; 6] = core::array::from_fn(|k| s.window::<SW>(y + k, 1));

        // Step 1: the vertical 6-tap into 16-bit `iTmp`, unrounded.
        let mut j = 0;
        while j + 8 <= SW {
            let taps: [i16x8; 6] = core::array::from_fn(|k| widen_lo(load8(&r[k].row::<8>(0, j))));
            let res = filter_6tap_intermediate(taps[0], taps[1], taps[2], taps[3], taps[4], taps[5]);
            iTmp[j..j + 8].copy_from_slice(res.as_array());
            j += 8;
        }
        if j + 4 <= SW {
            let taps: [i16x8; 6] = core::array::from_fn(|k| widen_lo(load_w::<4>(&r[k].row::<4>(0, j))));
            let res = filter_6tap_intermediate(taps[0], taps[1], taps[2], taps[3], taps[4], taps[5]);
            iTmp[j..j + 4].copy_from_slice(&res.as_array()[..4]);
            j += 4;
        }
        while j < SW {
            let t: [u8; 6] = core::array::from_fn(|k| r[k].row::<1>(0, j)[0]);
            iTmp[j] = filter_input_8bit(&t) as i16;
            j += 1;
        }

        // Step 2: the horizontal 6-tap over `iTmp`, in scalar, as the intrinsic kernel.
        let out = d.row_mut::<W>(y, 0);
        for (o, w) in out.iter_mut().zip(iTmp[..SW].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(w.try_into().expect("six taps")) + 512) >> 10i32);
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
            let w: [u8; 6] = core::array::from_fn(|k| src.at(x, dy + k as isize - 2));
            *t = filter_input_8bit(&w) as i16;
        }
        let out = dst.row_mut(dy, 0, width);
        for (o, w) in out.iter_mut().zip(iTmp[..n].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(w.try_into().expect("six taps")) + 512) >> 10i32);
        }
    }
}

pub fn mc_hor_ver22<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    cen_shaped::<WideLeaves, S>(src, dst, width, height)
}

// ============================================================================
// Luma quarter-pel
// ============================================================================

/// The `wide` leaf set: the shape dispatch above and the twelve quarter-pel
/// composites in `common/mc.rs`, instantiated over the four kernels in this file.
pub struct WideLeaves;

impl McLeaves for WideLeaves {
    #[inline(always)]
    fn hor<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        hor_block::<S, W, SW, H, AVG>(src, dst)
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
        ver_block::<S, W, H, SH, AVG>(src, dst)
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
        cen_block::<S, W, SW, H, SH>(src, dst)
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
        avg_block::<A, B, W, H>(dst, a, b)
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
        chroma_block::<S, W, SW, H, SH>(src, dst, w)
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

pub fn mc_luma<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    crate::common::mc::mc_luma_with::<WideLeaves, S>(src, dst, mv_x, mv_y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    // These MUST be the `_c` scalar kernels, not the same-named dispatchers:
    // the dispatchers route to the very kernels under test, which would
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
            *b = (s >> 16i32) as u8;
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
                    2 if qy == 0 => WideLeaves::hor::<_, 16, 21, 16, 2>(&src, &mut d),
                    3 if qy == 0 => WideLeaves::hor::<_, 16, 21, 16, 3>(&src, &mut d),
                    2 => WideLeaves::ver::<_, 16, 16, 21, 2>(&src, &mut d),
                    _ => WideLeaves::ver::<_, 16, 16, 21, 3>(&src, &mut d),
                }
            }
            assert_eq!(want, got, "fused quarter-pel ({qx}, {qy}) differs from the composite");
        }
    }
}
