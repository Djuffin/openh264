//! Motion compensation — `McHorVer*_AArch64_neon`, `McChromaWidthEq*_AArch64_neon`
//! and `PixelAvgWidthEq*_AArch64_neon`, `codec/common/arm64/mc_aarch64_neon.S`, with
//! `McLuma_AArch64_neon`'s dispatch from `codec/common/src/mc.cpp`.
//!
//! # The 6-tap filter
//!
//! `FILTER_6TAG_8BITS`: `uaddl` the outer and inner tap pairs, `mla` by 20, `mls` by
//! 5, `sqrshrun #5` — `(v + 16) >> 5` saturated to a byte. The word lanes hold
//! `[-2550, 10710]` and cannot overflow. That body serves the horizontal kernel (taps
//! along the row), the vertical one (taps down a six-row window) and, with the
//! `_AVERAGE_WITH_0`/`_1` tails — a rounded average against the centre tap or the
//! one after it — the four fused quarter-pel kernels `McHorVer10/30/01/03`, which
//! [`mc_luma`] dispatches to directly as upstream's `McLuma_AArch64_neon` does. The
//! other eight quarter-pel positions are `common::mc`'s composites over this file's
//! leaves, as they are upstream (`McHorVer11_AArch64_neon` and the rest are C++
//! wrappers over the same asm leaves).
//!
//! # The centre kernel, and where this departs from the asm
//!
//! `McHorVer22` filters vertically into 16-bit intermediates and then horizontally
//! over those. The asm's horizontal pass (`FILTER_3_IN_16BITS_TO_8BITS`) keeps its
//! `.8h` lanes by computing `(a - 5b + 20c) / 16` as `(((a - b) >> 2 - b + c) >> 2) + c`
//! — an exact decomposition, but one whose intermediate `((a - b) >> 2) - b + c` can
//! reach -33150 on adversarial neighbouring columns and wrap. The C computes the
//! 6-tap in `int`, and so does the scalar here, so this pass widens to `.4s`
//! (`saddl`, `mla`/`mls` by scalar, `sqrshrun #10`) and agrees with it everywhere.
//! The vertical pass is the asm's `FILTER_6TAG_8BITS_TO_16BITS`, exact in `.8h`.
//!
//! # Widths
//!
//! Upstream has one routine per width (4, 8, 16) and separate `Width5/9/17` routines
//! for the encoder's half-pel search buffers, which the port reaches through the same
//! entry points with `kiW + 1`. Each kernel here takes any width: sixteen-, eight-
//! and four-lane chunks, and for the odd column of a 5-, 9- or 17-wide row one more
//! chunk, ending at the last column and overlapping the one before — the overlap
//! rewrites bytes with the values they already hold, and it is cheaper than the
//! asm's one-lane `FILTER_SINGLE_TAG_8BITS`, let alone a scalar tail. Loads at the
//! six tap offsets replace the asm's `ext` chains; both are one instruction per tap.
//!
//! `pixel_avg` is `urhadd`, the one-instruction form of the asm's `uaddl`/`rshrn #1`
//! pair; `mc_chroma` is `umull`/`umlal` by the four byte weights and `rshrn #6`.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld16, ld4, ld8, ld8_i16, st16, st4, st8, st8_i16};
use crate::common::mc::{filter_input_8bit, g_kuiABCD, hor_filter_input_16bit, mc_copy, McLeaves, WelsClip1};
use crate::safe::plane::{PlaneCursorMut, RefSamples};

// ============================================================================
// The 6-tap filter
// ============================================================================

/// `FILTER_6TAG_8BITS_TO_16BITS1`: the unclipped word result over eight lanes.
#[inline]
#[target_feature(enable = "neon")]
fn tap6_8(p: [uint8x8_t; 6]) -> int16x8_t {
    let t = vaddl_u8(p[0], p[5]);
    let t = vmlaq_n_u16(t, vaddl_u8(p[2], p[3]), 20);
    let t = vmlsq_n_u16(t, vaddl_u8(p[1], p[4]), 5);
    vreinterpretq_s16_u16(t)
}

/// `FILTER_6TAG_8BITS_TO_16BITS2`: the same on the high halves.
#[inline]
#[target_feature(enable = "neon")]
fn tap6_hi(p: &[uint8x16_t; 6]) -> int16x8_t {
    let t = vaddl_high_u8(p[0], p[5]);
    let t = vmlaq_n_u16(t, vaddl_high_u8(p[2], p[3]), 20);
    let t = vmlsq_n_u16(t, vaddl_high_u8(p[1], p[4]), 5);
    vreinterpretq_s16_u16(t)
}

#[inline]
#[target_feature(enable = "neon")]
fn tap6_lo(p: &[uint8x16_t; 6]) -> int16x8_t {
    tap6_8([
        vget_low_u8(p[0]),
        vget_low_u8(p[1]),
        vget_low_u8(p[2]),
        vget_low_u8(p[3]),
        vget_low_u8(p[4]),
        vget_low_u8(p[5]),
    ])
}

/// `FILTER_6TAG_8BITS1`: eight clipped bytes.
#[inline]
#[target_feature(enable = "neon")]
fn filter6_8(p: [uint8x8_t; 6]) -> uint8x8_t {
    vqrshrun_n_s16::<5>(tap6_8(p))
}

/// `FILTER_6TAG_8BITS1` + `FILTER_6TAG_8BITS2`: sixteen.
#[inline]
#[target_feature(enable = "neon")]
fn filter6_16(p: &[uint8x16_t; 6]) -> uint8x16_t {
    vqrshrun_high_n_s16::<5>(vqrshrun_n_s16::<5>(tap6_lo(p)), tap6_hi(p))
}

/// The six taps of sixteen outputs, from a row slice that starts at `x - 2`.
#[inline]
#[target_feature(enable = "neon")]
fn taps16(r: &[u8]) -> [uint8x16_t; 6] {
    [ld16(&r[0..]), ld16(&r[1..]), ld16(&r[2..]), ld16(&r[3..]), ld16(&r[4..]), ld16(&r[5..])]
}

/// The six taps of eight outputs.
#[inline]
#[target_feature(enable = "neon")]
fn taps8(r: &[u8]) -> [uint8x8_t; 6] {
    [ld8(&r[0..]), ld8(&r[1..]), ld8(&r[2..]), ld8(&r[3..]), ld8(&r[4..]), ld8(&r[5..])]
}

/// The six taps of four outputs, valid in the low four lanes: nine bytes read as
/// two eight-lane vectors, and `ext` for the offsets between them.
#[inline]
#[target_feature(enable = "neon")]
fn taps4(r: &[u8]) -> [uint8x8_t; 6] {
    let a = ld8(&r[..8]);
    let b = ld8(&r[1..9]);
    [a, vext_u8::<1>(a, a), vext_u8::<2>(a, a), vext_u8::<3>(a, a), vext_u8::<4>(a, a), vext_u8::<4>(b, b)]
}

// ============================================================================
// Horizontal: McHorVer20, McHorVer10, McHorVer30
// ============================================================================

/// Sixteen outputs of the horizontal filter at column `x`.
#[inline]
#[target_feature(enable = "neon")]
fn hor_chunk16<const AVG: usize>(out: &mut [u8], row: &[u8], x: usize) {
    let t = taps16(&row[x..]);
    let mut v = filter6_16(&t);
    if AVG != 0 {
        v = vrhaddq_u8(v, t[AVG]);
    }
    st16(&mut out[x..], v);
}

/// Eight outputs of the horizontal filter at column `x`.
#[inline]
#[target_feature(enable = "neon")]
fn hor_chunk8<const AVG: usize>(out: &mut [u8], row: &[u8], x: usize) {
    let t = taps8(&row[x..]);
    let mut v = filter6_8(t);
    if AVG != 0 {
        v = vrhadd_u8(v, t[AVG]);
    }
    st8(&mut out[x..], v);
}

/// Four outputs of the horizontal filter at column `x`.
#[inline]
#[target_feature(enable = "neon")]
fn hor_chunk4<const AVG: usize>(out: &mut [u8], row: &[u8], x: usize) {
    let t = taps4(&row[x..]);
    let mut v = filter6_8(t);
    if AVG != 0 {
        v = vrhadd_u8(v, t[AVG]);
    }
    st4(&mut out[x..], v);
}

/// One output row of the horizontal filter over `row`, which starts at `x = -2` and
/// holds `width + 5` bytes.
///
/// `AVG` is 0, or the tap the result is averaged with: 2 (`src[0]`, the
/// `_AVERAGE_WITH_0` kernels, quarter-pel `(1, 0)`) or 3 (`src[1]`,
/// `_AVERAGE_WITH_1`, quarter-pel `(3, 0)`).
#[inline]
#[target_feature(enable = "neon")]
fn hor_row<const AVG: usize>(out: &mut [u8], row: &[u8], width: usize) {
    let mut x = 0;
    while x + 16 <= width {
        hor_chunk16::<AVG>(out, row, x);
        x += 16;
    }
    if x + 8 <= width {
        hor_chunk8::<AVG>(out, row, x);
        x += 8;
    }
    if x + 4 <= width {
        hor_chunk4::<AVG>(out, row, x);
        x += 4;
    }
    // The odd column of a 5-, 9- or 17-wide row: one more chunk, ending at it.
    if x < width {
        if width >= 8 {
            hor_chunk8::<AVG>(out, row, width - 8);
        } else if width >= 4 {
            hor_chunk4::<AVG>(out, row, width - 4);
        } else {
            while x < width {
                let w: [u8; 6] = row[x..x + 6].try_into().expect("six taps");
                let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
                if AVG != 0 {
                    v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
                }
                out[x] = v;
                x += 1;
            }
        }
    }
}

#[inline]
#[target_feature(enable = "neon")]
fn hor<S: RefSamples + Copy, const AVG: usize>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    for dy in 0..height as isize {
        let row = src.row_view(dy, -2, width + 5);
        let out = dst.row_mut(dy, 0, width);
        hor_row::<AVG>(out, &row, width);
    }
}

// ============================================================================
// Vertical: McHorVer02, McHorVer01, McHorVer03
// ============================================================================

/// `McHorVer02WidthEq16_AArch64_neon` (and its `Height17` and `_AVERAGE_WITH_`
/// forms): a six-row window of sixteen-byte rows, one `ld1` and one filter per
/// output row, the window sliding by one.
#[inline]
#[target_feature(enable = "neon")]
fn ver16<S: RefSamples + Copy, const AVG: usize>(src: &S, dst: &mut PlaneCursorMut<'_>, height: usize) {
    let mut w = [vdupq_n_u8(0); 6];
    for i in 0..5 {
        w[i] = ld16(&src.row_view(i as isize - 2, 0, 16));
    }
    for dy in 0..height as isize {
        w[5] = ld16(&src.row_view(dy + 3, 0, 16));
        let mut v = filter6_16(&w);
        if AVG != 0 {
            v = vrhaddq_u8(v, w[AVG]);
        }
        st16(dst.row_mut(dy, 0, 16), v);
        w.copy_within(1..6, 0);
    }
}

/// `McHorVer02WidthEq8_AArch64_neon` and its forms.
#[inline]
#[target_feature(enable = "neon")]
fn ver8<S: RefSamples + Copy, const AVG: usize>(src: &S, dst: &mut PlaneCursorMut<'_>, height: usize) {
    let mut w = [vdup_n_u8(0); 6];
    for i in 0..5 {
        w[i] = ld8(&src.row_view(i as isize - 2, 0, 8));
    }
    for dy in 0..height as isize {
        w[5] = ld8(&src.row_view(dy + 3, 0, 8));
        let mut v = filter6_8(w);
        if AVG != 0 {
            v = vrhadd_u8(v, w[AVG]);
        }
        st8(dst.row_mut(dy, 0, 8), v);
        w.copy_within(1..6, 0);
    }
}

/// `McHorVer02WidthEq4_AArch64_neon` and its forms: the asm pairs two rows into one
/// register; this keeps one row per register with the upper lanes idle, which is
/// what its `Height5` form does anyway.
#[inline]
#[target_feature(enable = "neon")]
fn ver4<S: RefSamples + Copy, const AVG: usize>(src: &S, dst: &mut PlaneCursorMut<'_>, height: usize) {
    let mut w = [vdup_n_u8(0); 6];
    for i in 0..5 {
        w[i] = ld4(&src.row_view(i as isize - 2, 0, 4));
    }
    for dy in 0..height as isize {
        w[5] = ld4(&src.row_view(dy + 3, 0, 4));
        let mut v = filter6_8(w);
        if AVG != 0 {
            v = vrhadd_u8(v, w[AVG]);
        }
        st4(dst.row_mut(dy, 0, 4), v);
        w.copy_within(1..6, 0);
    }
}

/// The widths upstream has no routine for.
#[inline]
fn ver_any<S: RefSamples + Copy, const AVG: usize>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
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
        for x in 0..width {
            let w = [r0[x], r1[x], r2[x], r3[x], r4[x], r5[x]];
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
            }
            out[x] = v;
        }
        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
    }
}

#[inline]
#[target_feature(enable = "neon")]
fn ver<S: RefSamples + Copy, const AVG: usize>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    match width {
        16 => ver16::<S, AVG>(src, dst, height),
        8 => ver8::<S, AVG>(src, dst, height),
        4 => ver4::<S, AVG>(src, dst, height),
        _ => ver_any::<S, AVG>(src, dst, width, height),
    }
}

// ============================================================================
// Centre: McHorVer22
// ============================================================================

/// The horizontal 6-tap over eight 16-bit intermediates, in `.4s` — see the header.
/// `t` starts at the column two before the first output.
#[inline]
#[target_feature(enable = "neon")]
fn hor6_16bit(t: &[i16]) -> uint8x8_t {
    let t0 = ld8_i16(&t[0..]);
    let t1 = ld8_i16(&t[1..]);
    let t2 = ld8_i16(&t[2..]);
    let t3 = ld8_i16(&t[3..]);
    let t4 = ld8_i16(&t[4..]);
    let t5 = ld8_i16(&t[5..]);
    let a_lo = vaddl_s16(vget_low_s16(t0), vget_low_s16(t5));
    let a_hi = vaddl_high_s16(t0, t5);
    let b_lo = vaddl_s16(vget_low_s16(t1), vget_low_s16(t4));
    let b_hi = vaddl_high_s16(t1, t4);
    let c_lo = vaddl_s16(vget_low_s16(t2), vget_low_s16(t3));
    let c_hi = vaddl_high_s16(t2, t3);
    let x_lo = vmlsq_n_s32(vmlaq_n_s32(a_lo, c_lo, 20), b_lo, 5);
    let x_hi = vmlsq_n_s32(vmlaq_n_s32(a_hi, c_hi, 20), b_hi, 5);
    vqmovn_u16(vcombine_u16(vqrshrun_n_s32::<10>(x_lo), vqrshrun_n_s32::<10>(x_hi)))
}

/// `McHorVer22WidthEq16_AArch64_neon` and the `Width17/9/5` forms; see the header
/// for the widths and for the horizontal pass's precision.
#[inline]
#[target_feature(enable = "neon")]
fn cen<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    // `iTmp` is `int16_t[17 + 5]` in the C++ and the widest caller is `md.rs`'s
    // `kiW + 1` with `kiW = 16`. The scratch here is wider so the eight-lane loads of
    // the horizontal pass may run past the last valid column, but the contract on
    // `width` is the C++'s, and the one the x86_64 kernel asserts.
    assert!(width <= 17, "mc_hor_ver22 width {width} exceeds the 17 iTmp is sized for");
    let n = width + 5;
    let mut tmp = [0i16; 32];
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        src.row_view(-2, -2, n),
        src.row_view(-1, -2, n),
        src.row_view(0, -2, n),
        src.row_view(1, -2, n),
        src.row_view(2, -2, n),
    );
    for dy in 0..height as isize {
        let r5 = src.row_view(dy + 3, -2, n);

        // The vertical pass, into 16-bit intermediates: eight columns per chunk,
        // and a last chunk ending at column `n` for the columns the others leave.
        let mut j = 0;
        while j + 8 <= n {
            let t = tap6_8([ld8(&r0[j..]), ld8(&r1[j..]), ld8(&r2[j..]), ld8(&r3[j..]), ld8(&r4[j..]), ld8(&r5[j..])]);
            st8_i16(&mut tmp[j..], t);
            j += 8;
        }
        if j < n {
            if n >= 8 {
                let j = n - 8;
                let t = tap6_8([ld8(&r0[j..]), ld8(&r1[j..]), ld8(&r2[j..]), ld8(&r3[j..]), ld8(&r4[j..]), ld8(&r5[j..])]);
                st8_i16(&mut tmp[j..], t);
            } else {
                while j < n {
                    tmp[j] = filter_input_8bit(&[r0[j], r1[j], r2[j], r3[j], r4[j], r5[j]]) as i16;
                    j += 1;
                }
            }
        }
        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);

        // The horizontal pass over them, the same way.
        let out = dst.row_mut(dy, 0, width);
        let mut x = 0;
        while x + 8 <= width {
            st8(&mut out[x..], hor6_16bit(&tmp[x..]));
            x += 8;
        }
        if x + 4 <= width {
            st4(&mut out[x..], hor6_16bit(&tmp[x..]));
            x += 4;
        }
        if x < width {
            if width >= 8 {
                st8(&mut out[width - 8..], hor6_16bit(&tmp[width - 8..]));
            } else if width >= 4 {
                st4(&mut out[width - 4..], hor6_16bit(&tmp[width - 4..]));
            } else {
                while x < width {
                    let w: [i16; 6] = tmp[x..x + 6].try_into().expect("six taps");
                    out[x] = WelsClip1((hor_filter_input_16bit(&w) + 512) >> 10);
                    x += 1;
                }
            }
        }
    }
}

// ============================================================================
// Averaging and chroma
// ============================================================================

/// `PixelAvgWidthEq16/8/4_AArch64_neon`, one row: `urhadd` per chunk.
#[inline]
#[target_feature(enable = "neon")]
fn avg_row(out: &mut [u8], a: &[u8], b: &[u8], width: usize) {
    let mut x = 0;
    while x + 16 <= width {
        st16(&mut out[x..], vrhaddq_u8(ld16(&a[x..]), ld16(&b[x..])));
        x += 16;
    }
    if x + 8 <= width {
        st8(&mut out[x..], vrhadd_u8(ld8(&a[x..]), ld8(&b[x..])));
        x += 8;
    }
    if x + 4 <= width {
        st4(&mut out[x..], vrhadd_u8(ld4(&a[x..]), ld4(&b[x..])));
        x += 4;
    }
    while x < width {
        out[x] = ((a[x] as u32 + b[x] as u32 + 1) >> 1) as u8;
        x += 1;
    }
}

/// `McChromaWidthEq8_AArch64_neon`: `umull`/`umlal` by the four byte weights, the
/// bottom row of one output row being the top row of the next.
#[inline]
#[target_feature(enable = "neon")]
fn chroma8<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, w: &[u8; 4], height: usize) {
    let (wa, wb, wc, wd) = (vdup_n_u8(w[0]), vdup_n_u8(w[1]), vdup_n_u8(w[2]), vdup_n_u8(w[3]));
    let r = src.row_view(0, 0, 9);
    let (mut a, mut b) = (ld8(&r[..8]), ld8(&r[1..]));
    for dy in 0..height as isize {
        let r = src.row_view(dy + 1, 0, 9);
        let (c, d) = (ld8(&r[..8]), ld8(&r[1..]));
        let s = vmull_u8(a, wa);
        let s = vmlal_u8(s, b, wb);
        let s = vmlal_u8(s, c, wc);
        let s = vmlal_u8(s, d, wd);
        st8(dst.row_mut(dy, 0, 8), vrshrn_n_u16::<6>(s));
        a = c;
        b = d;
    }
}

/// `McChromaWidthEq4_AArch64_neon`, one row per register.
#[inline]
#[target_feature(enable = "neon")]
fn chroma4<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, w: &[u8; 4], height: usize) {
    let (wa, wb, wc, wd) = (vdup_n_u8(w[0]), vdup_n_u8(w[1]), vdup_n_u8(w[2]), vdup_n_u8(w[3]));
    let r = src.row_view(0, 0, 5);
    let (mut a, mut b) = (ld4(&r[..4]), ld4(&r[1..]));
    for dy in 0..height as isize {
        let r = src.row_view(dy + 1, 0, 5);
        let (c, d) = (ld4(&r[..4]), ld4(&r[1..]));
        let s = vmull_u8(a, wa);
        let s = vmlal_u8(s, b, wb);
        let s = vmlal_u8(s, c, wc);
        let s = vmlal_u8(s, d, wd);
        st4(dst.row_mut(dy, 0, 4), vrshrn_n_u16::<6>(s));
        a = c;
        b = d;
    }
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

/// `PixelAvg_AArch64_neon`.
#[inline]
pub fn pixel_avg<A: RefSamples, B: RefSamples>(dst: &mut PlaneCursorMut<'_>, a: &A, b: &B, width: usize, height: usize) {
    for dy in 0..height as isize {
        let ra = a.row_view(dy, 0, width);
        let rb = b.row_view(dy, 0, width);
        let out = dst.row_mut(dy, 0, width);
        // SAFETY: NEON is baseline on aarch64; see the module header.
        unsafe { avg_row(out, &ra, &rb, width) }
    }
}

/// `McChroma_AArch64_neon`: the copy path on a whole-sample vector, else the
/// bilinear kernels, else — width 2 — the scalar.
#[inline]
pub fn mc_chroma<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize) {
    if (mv_x & 0x07) == 0 && (mv_y & 0x07) == 0 {
        mc_copy(src, dst, width, height);
        return;
    }
    if width == 0 {
        return;
    }
    let w = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    match width {
        8 => unsafe { chroma8(src, dst, w, height) },
        4 => unsafe { chroma4(src, dst, w, height) },
        _ => {
            let (a, b, c, d) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
            for dy in 0..height as isize {
                let r0 = src.row_view(dy, 0, width + 1);
                let r1 = src.row_view(dy + 1, 0, width + 1);
                let out = dst.row_mut(dy, 0, width);
                for j in 0..width {
                    out[j] = ((a * r0[j] as i32 + b * r0[j + 1] as i32 + c * r1[j] as i32 + d * r1[j + 1] as i32 + 32) >> 6) as u8;
                }
            }
        }
    }
}

/// `McHorVer20_AArch64_neon` and `McHorVer20Width5Or9Or17_AArch64_neon`.
#[inline]
pub fn mc_hor_ver20<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    unsafe { hor::<S, 0>(src, dst, width, height) }
}

/// `McHorVer02_AArch64_neon` and `McHorVer02Height5Or9Or17_AArch64_neon`.
#[inline]
pub fn mc_hor_ver02<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    unsafe { ver::<S, 0>(src, dst, width, height) }
}

/// `McHorVer22_AArch64_neon` and `McHorVer22Width5Or9Or17Height5Or9Or17_AArch64_neon`.
#[inline]
pub fn mc_hor_ver22<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    unsafe { cen(src, dst, width, height) }
}

/// **The NEON leaf set** — `McLeaves` with the three filters and the average above,
/// for the quarter-pel composites in `common/mc.rs`.
pub struct NeonLeaves;

impl McLeaves for NeonLeaves {
    #[inline(always)]
    fn hor<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        mc_hor_ver20(src, dst, width, height)
    }
    #[inline(always)]
    fn ver<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        mc_hor_ver02(src, dst, width, height)
    }
    #[inline(always)]
    fn cen<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        mc_hor_ver22(src, dst, width, height)
    }
    #[inline(always)]
    fn avg<A: RefSamples, B: RefSamples>(dst: &mut PlaneCursorMut<'_>, a: &A, b: &B, width: usize, height: usize) {
        pixel_avg(dst, a, b, width, height)
    }
}

/// `McLuma_AArch64_neon`: the four fused quarter-pel kernels where upstream has
/// them, and the composites over the NEON leaves elsewhere.
///
/// The fused arms are byte-identical to the composites they replace — a rounded
/// average of the same two values — and `test_mc_luma_parity` says so for all
/// sixteen positions.
#[inline]
pub fn mc_luma<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize) {
    match ((mv_x & 0x03) as u8, (mv_y & 0x03) as u8) {
        (1, 0) => unsafe { hor::<S, 2>(src, dst, width, height) },
        (3, 0) => unsafe { hor::<S, 3>(src, dst, width, height) },
        (0, 1) => unsafe { ver::<S, 2>(src, dst, width, height) },
        (0, 3) => unsafe { ver::<S, 3>(src, dst, width, height) },
        _ => crate::common::mc::mc_luma_with::<NeonLeaves, S>(src, dst, mv_x, mv_y, width, height),
    }
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    // These MUST be the `_c` scalar kernels, not the same-named dispatchers: the
    // dispatchers route to the very kernels under test.
    use crate::common::mc::{
        mc_chroma_with_frag_mv, mc_hor_ver02_c as scalar_hor_ver02, mc_hor_ver20_c as scalar_hor_ver20,
        mc_hor_ver22_c as scalar_hor_ver22, mc_luma_c as scalar_luma, pixel_avg_c as scalar_pixel_avg,
    };
    use crate::encoder::rec_view::RecCursor;
    use crate::safe::plane::PlaneCursor;

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

    /// The pattern the header's overflow analysis is about: columns alternating
    /// between the filter's extremes, so the vertical outputs hit `10710` and
    /// `-2550` on neighbouring columns and the centre kernel's horizontal pass sees
    /// its largest intermediates.
    fn adversarial_plane() -> Vec<u8> {
        let mut v = vec![0u8; STRIDE * ROWS];
        for y in 0..ROWS {
            for x in 0..STRIDE {
                // Rows -2, 3 and 0, 1 of the six-tap window high, rows -1, 2 low, on
                // even columns; the opposite on odd columns.
                let high_row = matches!(y % 6, 0 | 2 | 3 | 5);
                let even = x % 2 == 0;
                v[y * STRIDE + x] = if high_row == even { 255 } else { 0 };
            }
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
            scalar_pixel_avg(&mut PlaneCursorMut::new(&mut dst_scalar, 10 * STRIDE + 8, STRIDE), &ca, &cb, w, h);
            pixel_avg(&mut PlaneCursorMut::new(&mut dst_simd, 10 * STRIDE + 8, STRIDE), &ca, &cb, w, h);
            assert_eq!(dst_scalar, dst_simd, "pixel_avg mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_chroma_parity() {
        let base = filled_plane();
        let src = PlaneCursor::new(&base, 10 * STRIDE + 10, STRIDE);
        let dst_c = 20 * STRIDE + 10;
        for &(w, h) in &[(8, 8), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)] {
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
                    mc_chroma(&src, &mut PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE), dx, dy, w, h);
                    assert_eq!(dst_scalar, dst_simd, "mc_chroma mismatch at {w}x{h} with mv=({dx}, {dy})");
                }
            }
        }
    }

    fn check_hor20(base: &[u8]) {
        let src = PlaneCursor::new(base, 10 * STRIDE + 10, STRIDE);
        let dst_c = 20 * STRIDE + 10;
        for &(w, h) in &[(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4), (17, 16), (9, 8), (5, 4), (17, 17)] {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];
            scalar_hor_ver20(&src, &mut PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE), w, h);
            mc_hor_ver20(&src, &mut PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE), w, h);
            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver20 mismatch at {w}x{h}");
        }
    }

    fn check_ver02(base: &[u8]) {
        let src = PlaneCursor::new(base, 10 * STRIDE + 10, STRIDE);
        let dst_c = 20 * STRIDE + 10;
        for &(w, h) in &[(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4), (16, 17), (8, 9), (4, 5), (17, 17)] {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];
            scalar_hor_ver02(&src, &mut PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE), w, h);
            mc_hor_ver02(&src, &mut PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE), w, h);
            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver02 mismatch at {w}x{h}");
        }
    }

    fn check_ver22(base: &[u8]) {
        let src = PlaneCursor::new(base, 10 * STRIDE + 10, STRIDE);
        let dst_c = 20 * STRIDE + 10;
        for &(w, h) in &[(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4), (17, 17), (9, 9), (5, 5), (17, 16), (9, 8)] {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];
            scalar_hor_ver22(&src, &mut PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE), w, h);
            mc_hor_ver22(&src, &mut PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE), w, h);
            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver22 mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_hor_ver20_parity() {
        check_hor20(&filled_plane());
        check_hor20(&adversarial_plane());
    }

    #[test]
    fn test_mc_hor_ver02_parity() {
        check_ver02(&filled_plane());
        check_ver02(&adversarial_plane());
    }

    /// The centre kernel over noise and over the adversarial plane — the latter is
    /// where the asm's 16-bit horizontal pass would wrap, and where this one's
    /// widened pass has to agree with the scalar.
    #[test]
    fn test_mc_hor_ver22_parity() {
        check_ver22(&filled_plane());
        check_ver22(&adversarial_plane());
    }

    #[test]
    fn test_mc_luma_parity() {
        for base in [filled_plane(), adversarial_plane()] {
            let src = PlaneCursor::new(&base, 10 * STRIDE + 10, STRIDE);
            let dst_c = 20 * STRIDE + 10;
            for &(w, h) in &[(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)] {
                for qy in 0..4i16 {
                    for qx in 0..4i16 {
                        let mut dst_scalar = vec![0u8; STRIDE * ROWS];
                        let mut dst_simd = vec![0u8; STRIDE * ROWS];
                        scalar_luma(&src, &mut PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE), qx, qy, w, h);
                        mc_luma(&src, &mut PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE), qx, qy, w, h);
                        assert_eq!(dst_scalar, dst_simd, "mc_luma mismatch at {w}x{h} with qpos=({qx}, {qy})");
                    }
                }
            }
        }
    }

    /// The same kernels reached through the shared cursor, whose rows arrive by
    /// value: the encoder's source picture is one of these.
    #[test]
    fn mc_parity_through_the_shared_cursor() {
        let mut base = filled_plane();
        let dst_c = 20 * STRIDE + 10;
        let mut want = vec![0u8; STRIDE * ROWS];
        let mut got = vec![0u8; STRIDE * ROWS];
        for (qx, qy) in [(0i16, 0i16), (1, 0), (2, 0), (3, 0), (0, 1), (0, 2), (0, 3), (1, 1), (2, 2), (3, 3)] {
            {
                let src = PlaneCursor::new(&base, 10 * STRIDE + 10, STRIDE);
                scalar_luma(&src, &mut PlaneCursorMut::new(&mut want, dst_c, STRIDE), qx, qy, 16, 16);
            }
            let src = RecCursor::over_owned(&mut base, 10 * STRIDE + 10, STRIDE);
            mc_luma(&src, &mut PlaneCursorMut::new(&mut got, dst_c, STRIDE), qx, qy, 16, 16);
            assert_eq!(want, got, "mc_luma via RecCursor at qpos=({qx}, {qy})");
        }
    }
}
