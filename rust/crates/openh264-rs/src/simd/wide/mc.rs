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
use crate::common::mc::{filter_input_8bit, g_kuiABCD, hor_filter_input_16bit, mc_copy, WelsClip1};
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, RefSamples};

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
fn pixel_avg_row(out: &mut [u8], a: &[u8], b: &[u8]) {
    let width = out.len();
    let mut x = 0;
    while x + 16 <= width {
        let v = avg_u8(load16(&a[x..]), load16(&b[x..]));
        out[x..x + 16].copy_from_slice(&v.to_array());
        x += 16;
    }
    if x + 8 <= width {
        let v = avg_u8(load8(&a[x..]), load8(&b[x..]));
        out[x..x + 8].copy_from_slice(&low8(v));
        x += 8;
    }
    if x + 4 <= width {
        let v = avg_u8(load_w::<4>(&a[x..]), load_w::<4>(&b[x..]));
        out[x..x + 4].copy_from_slice(&low4(v));
        x += 4;
    }
    while x < width {
        out[x] = ((a[x] as u32 + b[x] as u32 + 1) >> 1i32) as u8;
        x += 1;
    }
}

pub fn pixel_avg_sse2<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let ra = a.row_view(dy, 0, width);
        let rb = b.row_view(dy, 0, width);
        let out = dst.row_mut(dy, 0, width);
        pixel_avg_row(out, &ra[..], &rb[..]);
    }
}

// ============================================================================
// Chroma MC
// ============================================================================

/// One output row of `W` samples from the bilinear taps `[A B; C D]` over two
/// source rows of `W + 1` samples. Sums peak at `64 * 255`, inside `i16`.
#[inline(always)]
fn mc_chroma_row<const W: usize>(out: &mut [u8], r0: &[u8], r1: &[u8], w: [i16x8; 4]) {
    let s = widen_lo(load_w::<W>(r0)) * w[0]
        + widen_lo(load_w::<W>(&r0[1..])) * w[1]
        + widen_lo(load_w::<W>(r1)) * w[2]
        + widen_lo(load_w::<W>(&r1[1..])) * w[3];
    let v = (s + i16x8::splat(32)) >> 6i32;
    store_w::<W>(out, narrow(v, i16x8::ZERO));
}

pub fn mc_chroma_sse2<S: RefSamples + Copy>(
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
    if width == 0 {
        return;
    }

    let pABCD = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    let (iA, iB, iC, iD) = (pABCD[0] as i16, pABCD[1] as i16, pABCD[2] as i16, pABCD[3] as i16);

    if width == 8 || width == 4 {
        let w = [i16x8::splat(iA), i16x8::splat(iB), i16x8::splat(iC), i16x8::splat(iD)];
        for dy in 0..height as isize {
            let r0 = src.row_view(dy, 0, width + 1);
            let r1 = src.row_view(dy + 1, 0, width + 1);
            let out = dst.row_mut(dy, 0, width);
            if width == 8 {
                mc_chroma_row::<8>(out, &r0[..], &r1[..], w);
            } else {
                mc_chroma_row::<4>(out, &r0[..], &r1[..], w);
            }
        }
    } else {
        // Scalar fallback for width 2 or arbitrary widths, as the intrinsic kernel.
        for dy in 0..height as isize {
            let r0 = src.row_view(dy, 0, width + 1);
            let r1 = src.row_view(dy + 1, 0, width + 1);
            let out = dst.row_mut(dy, 0, width);
            for j in 0..width {
                out[j] = (((iA as i32) * (r0[j] as i32)
                    + (iB as i32) * (r0[j + 1] as i32)
                    + (iC as i32) * (r1[j] as i32)
                    + (iD as i32) * (r1[j + 1] as i32)
                    + 32)
                    >> 6i32) as u8;
            }
        }
    }
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

/// The six horizontal taps of `W` samples starting at `col`, as words.
#[inline(always)]
fn htaps<const W: usize>(row: &[u8], col: usize) -> [i16x8; 6] {
    core::array::from_fn(|k| widen_lo(load_w::<W>(&row[col + k..])))
}

pub fn mc_hor_ver20_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let row = src.row_view(dy, -2, width + 5);
        let row = &row[..];
        let out = dst.row_mut(dy, 0, width);

        let mut col = 0;
        while col + 8 <= width {
            let [p0, p1, p2, p3, p4, p5] = htaps::<8>(row, col);
            store_w::<8>(&mut out[col..], narrow(filter_6tap_shifted(p0, p1, p2, p3, p4, p5), i16x8::ZERO));
            col += 8;
        }
        if col + 4 <= width {
            let [p0, p1, p2, p3, p4, p5] = htaps::<4>(row, col);
            store_w::<4>(&mut out[col..], narrow(filter_6tap_shifted(p0, p1, p2, p3, p4, p5), i16x8::ZERO));
            col += 4;
        }
        while col < width {
            let w: [u8; 6] = row[col..col + 6].try_into().expect("6 taps");
            out[col] = WelsClip1((filter_input_8bit(&w) + 16) >> 5i32);
            col += 1;
        }
    }
}

// ============================================================================
// Vertical: McHorVer02
// ============================================================================

/// A source row of `W` samples as words: the low eight in `[0]`, and for `W == 16`
/// the high eight in `[1]`.
#[inline(always)]
fn vrow<const W: usize>(r: &[u8]) -> [i16x8; 2] {
    let v = load_w::<W>(r);
    [widen_lo(v), if W == 16 { widen_hi(v) } else { i16x8::ZERO }]
}

/// The vertical filter at one width, with the five-row window carried in registers
/// and one new row read per output row, as the intrinsic kernel does.
#[inline(always)]
fn mc_hor_ver02_w<const W: usize, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    height: usize,
) {
    let mut win = [[i16x8::ZERO; 2]; 5];
    for (k, dy) in (-2..=2isize).enumerate() {
        win[k] = vrow::<W>(&src.row_view(dy, 0, W)[..]);
    }
    for dy in 0..height as isize {
        let r5 = vrow::<W>(&src.row_view(dy + 3, 0, W)[..]);
        let lo = filter_6tap_shifted(win[0][0], win[1][0], win[2][0], win[3][0], win[4][0], r5[0]);
        let hi = if W == 16 {
            filter_6tap_shifted(win[0][1], win[1][1], win[2][1], win[3][1], win[4][1], r5[1])
        } else {
            i16x8::ZERO
        };
        let out = dst.row_mut(dy, 0, W);
        store_w::<W>(out, narrow(lo, hi));
        win = [win[1], win[2], win[3], win[4], r5];
    }
}

pub fn mc_hor_ver02_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    match width {
        16 => mc_hor_ver02_w::<16, S>(src, dst, height),
        8 => mc_hor_ver02_w::<8, S>(src, dst, height),
        4 => mc_hor_ver02_w::<4, S>(src, dst, height),
        _ => {
            // Scalar fallback for non-standard widths, as the intrinsic kernel.
            let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
                src.row_view(-2, 0, width),
                src.row_view(-1, 0, width),
                src.row_view(0, 0, width),
                src.row_view(1, 0, width),
                src.row_view(2, 0, width),
            );
            for dy in 0..height as isize {
                let r5 = src.row_view(dy + 3, 0, width);
                let out = dst.row_mut(dy, 0, width);
                for ((((((o, &a), &b), &c), &d), &e), &f) in out
                    .iter_mut()
                    .zip(r0.iter())
                    .zip(r1.iter())
                    .zip(r2.iter())
                    .zip(r3.iter())
                    .zip(r4.iter())
                    .zip(r5.iter())
                {
                    *o = WelsClip1((filter_input_8bit(&[a, b, c, d, e, f]) + 16) >> 5i32);
                }
                (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
            }
        }
    }
}

// ============================================================================
// Centre: McHorVer22
// ============================================================================

pub fn mc_hor_ver22_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut iTmp = [0i16; 17 + 5];
    let n = width + 5;
    // Same precondition as the intrinsic kernel, for the same reason: `iTmp` is
    // sized as the C++'s `int16_t iTmp[17 + 5]`. The stores below are bounds-checked
    // slice copies, so a wider caller would panic rather than overwrite the frame,
    // but the contract is stated in one place.
    assert!(width <= 17, "mc_hor_ver22 width {width} exceeds the 17 iTmp is sized for");

    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        src.row_view(-2, -2, n),
        src.row_view(-1, -2, n),
        src.row_view(0, -2, n),
        src.row_view(1, -2, n),
        src.row_view(2, -2, n),
    );

    for dy in 0..height as isize {
        let r5 = src.row_view(dy + 3, -2, n);

        // Step 1: the vertical 6-tap into 16-bit `iTmp`, unrounded.
        let mut j = 0;
        while j + 8 <= n {
            let taps = [&r0[..], &r1[..], &r2[..], &r3[..], &r4[..], &r5[..]].map(|r| widen_lo(load8(&r[j..])));
            let res = filter_6tap_intermediate(taps[0], taps[1], taps[2], taps[3], taps[4], taps[5]);
            iTmp[j..j + 8].copy_from_slice(res.as_array());
            j += 8;
        }
        if j + 4 <= n {
            let taps = [&r0[..], &r1[..], &r2[..], &r3[..], &r4[..], &r5[..]].map(|r| widen_lo(load_w::<4>(&r[j..])));
            let res = filter_6tap_intermediate(taps[0], taps[1], taps[2], taps[3], taps[4], taps[5]);
            iTmp[j..j + 4].copy_from_slice(&res.as_array()[..4]);
            j += 4;
        }
        while j < n {
            iTmp[j] = filter_input_8bit(&[r0[j], r1[j], r2[j], r3[j], r4[j], r5[j]]) as i16;
            j += 1;
        }

        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);

        // Step 2: the horizontal 6-tap over `iTmp`, in scalar, as the intrinsic kernel.
        let out = dst.row_mut(dy, 0, width);
        for (o, w) in out.iter_mut().zip(iTmp[..n].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(w.try_into().unwrap()) + 512) >> 10i32);
        }
    }
}

// ============================================================================
// Luma quarter-pel
// ============================================================================

/// The `wide` leaf set: the twelve quarter-pel composites in `common/mc.rs`
/// instantiated over the three filters and the average above.
pub struct WideLeaves;

impl crate::common::mc::McLeaves for WideLeaves {
    #[inline(always)]
    fn hor<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        mc_hor_ver20_sse2(src, dst, width, height)
    }
    #[inline(always)]
    fn ver<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        mc_hor_ver02_sse2(src, dst, width, height)
    }
    #[inline(always)]
    fn cen<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        mc_hor_ver22_sse2(src, dst, width, height)
    }
    #[inline(always)]
    fn avg<A: RefSamples, B: RefSamples>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
        width: usize,
        height: usize,
    ) {
        pixel_avg_sse2(dst, a, b, width, height)
    }
}

pub fn mc_luma_sse2<S: RefSamples + Copy>(
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
    // the dispatchers route to the very SSE2 kernels under test, which would
    // make every assertion below a tautology.
    use crate::common::mc::{
        mc_chroma_with_frag_mv, mc_hor_ver02_c as scalar_hor_ver02,
        mc_hor_ver20_c as scalar_hor_ver20, mc_hor_ver22_c as scalar_hor_ver22,
        mc_luma_c as scalar_luma, pixel_avg_c as scalar_pixel_avg,
    };

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

        for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4), (17, 16), (9, 8), (5, 4)] {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, 10 * STRIDE + 8, STRIDE);
            scalar_pixel_avg(&mut cur_scalar, &ca, &cb, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, 10 * STRIDE + 8, STRIDE);
            pixel_avg_sse2(&mut cur_simd, &ca, &cb, w, h);

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
                    mc_chroma_sse2(&src, &mut cur_simd, dx, dy, w, h);

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
            (17, 16), (9, 8), (5, 4),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver20(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver20_sse2(&src, &mut cur_simd, w, h);

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
            (16, 17), (8, 9), (4, 5),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver02(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver02_sse2(&src, &mut cur_simd, w, h);

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
            (17, 17), (9, 9), (5, 5),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver22(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver22_sse2(&src, &mut cur_simd, w, h);

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
                    mc_luma_sse2(&src, &mut cur_simd, qx, qy, w, h);

                    assert_eq!(
                        dst_scalar, dst_simd,
                        "mc_luma mismatch at {w}x{h} with qpos=({qx}, {qy})"
                    );
                }
            }
        }
    }
}
