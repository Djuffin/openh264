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
//! # Shapes, spans and windows
//!
//! Every kernel here takes its block's `W` and `H` as **const parameters** and reads
//! through [`RefSamples::span`](crate::safe::plane::RefSamples::span): one
//! bounds-checked cut per operand per block rather than a `row_view` per row, which
//! on the shared cell view the encoder hands them was a zeroed `RowBuf` and a
//! cell-by-cell copy loop before the vector load. `common::mc`'s `hor_shaped`,
//! `ver_shaped`, `cen_shaped`, `avg_shaped` and `chroma_shaped` are the dispatch onto
//! those shapes, and their tables are the codec's own call sites.
//!
//! Inside a kernel the rows come a [`ROW_GROUP`] at a time through
//! [`BlockRows::window`](crate::safe::plane::BlockRows::window) — see that constant
//! for why the block's own span is not enough — and the six taps of a filtered row
//! through [`hor_chunk`], which explains why they are separate loads and why the
//! chunking is a macro.
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
use crate::common::mc::{
    avg_shaped, cen_shaped, chroma_shaped, filter_input_8bit, g_kuiABCD, hor_filter_input_16bit, hor_shaped, mc_copy,
    ver_shaped, McLeaves, WelsClip1,
};
use crate::safe::plane::{BlockRows, PlaneCursorMut, RefSamples};

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

// ============================================================================
// Horizontal: McHorVer20, McHorVer10, McHorVer30
// ============================================================================

/// Rows per window cut.
///
/// **The row loops below are past the unroller's threshold at their real heights.**
/// A six-tap filter body is fifteen-odd instructions and a block is up to seventeen
/// rows, so LLVM keeps the loop — which leaves `y * stride` symbolic, and no span
/// length can be shown to contain a symbolic row offset (see
/// [`RefSamples::span`](crate::safe::plane::RefSamples::span)). Every per-row bounds
/// check then stays, which on a 16x16 average is ninety-six branches around three
/// instructions of work.
///
/// Cutting a window every `ROW_GROUP` rows restores the constant offsets: two checks
/// per operand per group, and none inside it, because a constant row index times the
/// window's own narrowed stride provably lands inside the window's own length. The
/// group's body is small enough to unroll, which is what makes the indices constant.
/// Four is the largest group that stays under the threshold for the six-tap filters,
/// and it divides every height the codec uses; the 5-, 9- and 17-row refinement
/// blocks leave one row over, which the tail loop takes a row at a time.
const ROW_GROUP: usize = 4;

/// The six taps of `N` outputs at column `x` of window row `y`, filtered and stored
/// — `N` is 16, 8 or 4, and `AVG` is [`hor_row`]'s.
///
/// # Why the taps come from a window, and why this is a macro
///
/// [`BlockRows::row`] hands a row over **by value**, which is right where the vector
/// load consumes the whole of it — a `[u8; 16]` is one `ldr q` and LLVM forwards it —
/// and wrong for a six-tap filter, whose row is `W + 5` samples read in six
/// overlapping loads: a `[u8; 21]` would be spilled to the stack and read back six
/// times. Loading each tap as its own `row::<N>(y, x + k)` keeps every load against
/// the original storage, plane or cell view alike.
///
/// That only pays if `y` and `x` are **constants** at the point the bounds checks are
/// decided, so that each tap's offset provably lands inside the window the caller
/// cut. As a function this was two chunks' worth of body at width 17, past what LLVM
/// will inline, and the row index arrived as an argument — which brought all twelve
/// checks back and cost the 17-wide refinement filter its whole speedup. A
/// `#[target_feature]` function cannot be `#[inline(always)]`, so the expansion has
/// to happen here.
macro_rules! hor_chunk {
    (16, $out:expr, $r:expr, $y:expr, $x:expr, $avg:expr) => {{
        let (r, y, x) = (&$r, $y, $x);
        let t = [
            ld16(&r.row::<16>(y, x)),
            ld16(&r.row::<16>(y, x + 1)),
            ld16(&r.row::<16>(y, x + 2)),
            ld16(&r.row::<16>(y, x + 3)),
            ld16(&r.row::<16>(y, x + 4)),
            ld16(&r.row::<16>(y, x + 5)),
        ];
        let mut v = filter6_16(&t);
        if $avg != 0 {
            v = vrhaddq_u8(v, t[$avg]);
        }
        st16(&mut $out[x..], v);
    }};
    (8, $out:expr, $r:expr, $y:expr, $x:expr, $avg:expr) => {{
        let (r, y, x) = (&$r, $y, $x);
        let t = [
            ld8(&r.row::<8>(y, x)),
            ld8(&r.row::<8>(y, x + 1)),
            ld8(&r.row::<8>(y, x + 2)),
            ld8(&r.row::<8>(y, x + 3)),
            ld8(&r.row::<8>(y, x + 4)),
            ld8(&r.row::<8>(y, x + 5)),
        ];
        let mut v = filter6_8(t);
        if $avg != 0 {
            v = vrhadd_u8(v, t[$avg]);
        }
        st8(&mut $out[x..], v);
    }};
    // The asm reads nine bytes and `ext`s between them; six four-byte loads against a
    // window that has already been bounds-checked are the same instruction count
    // without the shuffles.
    (4, $out:expr, $r:expr, $y:expr, $x:expr, $avg:expr) => {{
        let (r, y, x) = (&$r, $y, $x);
        let t = [
            ld4(&r.row::<4>(y, x)),
            ld4(&r.row::<4>(y, x + 1)),
            ld4(&r.row::<4>(y, x + 2)),
            ld4(&r.row::<4>(y, x + 3)),
            ld4(&r.row::<4>(y, x + 4)),
            ld4(&r.row::<4>(y, x + 5)),
        ];
        let mut v = filter6_8(t);
        if $avg != 0 {
            v = vrhadd_u8(v, t[$avg]);
        }
        st4(&mut $out[x..], v);
    }};
}

/// One output row of the horizontal filter at row `y` of the window `r`, which
/// starts at `x = -2` and holds `SW = W + 5` bytes.
///
/// `AVG` is 0, or the tap the result is averaged with: 2 (`src[0]`, the
/// `_AVERAGE_WITH_0` kernels, quarter-pel `(1, 0)`) or 3 (`src[1]`,
/// `_AVERAGE_WITH_1`, quarter-pel `(3, 0)`).
///
/// **This is a macro rather than a function, and that is not cosmetic.** At width 17
/// the body is two chunks and LLVM declines to inline it; the row index then arrives
/// as an argument rather than a constant, and with it symbolic every tap's bounds
/// check against the window comes back — which is the whole cost
/// [`ROW_GROUP`] exists to remove. A `#[target_feature]` function cannot be
/// `#[inline(always)]`, so the expansion has to happen here.
macro_rules! hor_row {
    ($out:expr, $r:expr, $y:expr, $w:expr, $avg:expr) => {{
        let out: &mut [u8; W] = $out;
        let mut x = 0;
        while x + 16 <= W {
            hor_chunk!(16, out, $r, $y, x, $avg);
            x += 16;
        }
        if x + 8 <= W {
            hor_chunk!(8, out, $r, $y, x, $avg);
            x += 8;
        }
        if x + 4 <= W {
            hor_chunk!(4, out, $r, $y, x, $avg);
            x += 4;
        }
        // The odd column of a 5-, 9- or 17-wide row: one more chunk, ending at it.
        if x < W {
            if W >= 8 {
                hor_chunk!(8, out, $r, $y, W - 8, $avg);
            } else if W >= 4 {
                hor_chunk!(4, out, $r, $y, W - 4, $avg);
            } else {
                while x < W {
                    let w = $r.row::<6>($y, x);
                    let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
                    if $avg != 0 {
                        v = ((v as u32 + w[$avg] as u32 + 1) >> 1) as u8;
                    }
                    out[x] = v;
                    x += 1;
                }
            }
        }
    }};
}

/// The horizontal filter over one const-shape block: **one span for the source, one
/// for the destination**, and `hor_row` per row over an `SW = W + 5` array.
///
/// The rows come out of the span by value — a `[u8; SW]` the vector loads read
/// straight from, one `ldr q` on the plane cursors and one gathered load on the
/// shared cell view — where `row_view` handed the cell view a zeroed `RowBuf` and a
/// copy loop first. See [`RefSamples::span`](crate::safe::plane::RefSamples::span).
#[inline]
#[target_feature(enable = "neon")]
fn hor_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<SW, H>(0, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let mut y = 0;
    while y + ROW_GROUP <= H {
        let g = s.window::<SW>(y, ROW_GROUP);
        let mut gd = d.window_mut::<W>(y, ROW_GROUP);
        for k in 0..ROW_GROUP {
            hor_row!(gd.row_mut::<W>(k, 0), &g, k, W, AVG);
        }
        y += ROW_GROUP;
    }
    while y < H {
        let g = s.window::<SW>(y, 1);
        hor_row!(d.row_mut::<W>(y, 0), &g, 0, W, AVG);
        y += 1;
    }
}

/// The run-time-shape twin — cold, and scalar: a shape the const table does not
/// carry is not one to write a vector tail for. See [`McLeaves`].
fn hor_any<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let w: [u8; 6] = std::array::from_fn(|k| src.at(x as isize + k as isize - 2, dy));
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

// ============================================================================
// Vertical: McHorVer02, McHorVer01, McHorVer03
// ============================================================================

/// `McHorVer02WidthEq16_AArch64_neon` (and its `Height17` and `_AVERAGE_WITH_`
/// forms): a six-row window of sixteen-byte rows, one `ld1` and one filter per
/// output row, the window sliding by one.
///
/// The rows come from **one `SH = H + 5` row span**, so the six taps of every output
/// row are constant offsets into a slice already proven long enough; the sliding
/// window is unchanged.
#[inline]
#[target_feature(enable = "neon")]
fn ver16_block<S: RefSamples + Copy, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<16, SH>(-2, 0);
    let mut d = dst.span_mut::<16, H>(0, 0);
    // The six-row window as its own bindings rather than an array the rows shift
    // through: the slide is then register renaming, not a `copy_within`.
    let (mut w0, mut w1, mut w2, mut w3, mut w4) = (
        ld16(&s.row::<16>(0, 0)),
        ld16(&s.row::<16>(1, 0)),
        ld16(&s.row::<16>(2, 0)),
        ld16(&s.row::<16>(3, 0)),
        ld16(&s.row::<16>(4, 0)),
    );
    let mut y = 0;
    while y + ROW_GROUP <= H {
        let g = s.window::<16>(y + 5, ROW_GROUP);
        let mut gd = d.window_mut::<16>(y, ROW_GROUP);
        for k in 0..ROW_GROUP {
            let w5 = ld16(&g.row::<16>(k, 0));
            let w = [w0, w1, w2, w3, w4, w5];
            let mut v = filter6_16(&w);
            if AVG != 0 {
                v = vrhaddq_u8(v, w[AVG]);
            }
            st16(gd.row_mut::<16>(k, 0), v);
            (w0, w1, w2, w3, w4) = (w1, w2, w3, w4, w5);
        }
        y += ROW_GROUP;
    }
    while y < H {
        let w5 = ld16(&s.row::<16>(y + 5, 0));
        let w = [w0, w1, w2, w3, w4, w5];
        let mut v = filter6_16(&w);
        if AVG != 0 {
            v = vrhaddq_u8(v, w[AVG]);
        }
        st16(d.row_mut::<16>(y, 0), v);
        (w0, w1, w2, w3, w4) = (w1, w2, w3, w4, w5);
        y += 1;
    }
}

/// `McHorVer02WidthEq8_AArch64_neon` and its forms.
#[inline]
#[target_feature(enable = "neon")]
fn ver8_block<S: RefSamples + Copy, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<8, SH>(-2, 0);
    let mut d = dst.span_mut::<8, H>(0, 0);
    // The six-row window as its own bindings rather than an array the rows shift
    // through: the slide is then register renaming, not a `copy_within`.
    let (mut w0, mut w1, mut w2, mut w3, mut w4) = (
        ld8(&s.row::<8>(0, 0)),
        ld8(&s.row::<8>(1, 0)),
        ld8(&s.row::<8>(2, 0)),
        ld8(&s.row::<8>(3, 0)),
        ld8(&s.row::<8>(4, 0)),
    );
    let mut y = 0;
    while y + ROW_GROUP <= H {
        let g = s.window::<8>(y + 5, ROW_GROUP);
        let mut gd = d.window_mut::<8>(y, ROW_GROUP);
        for k in 0..ROW_GROUP {
            let w5 = ld8(&g.row::<8>(k, 0));
            let w = [w0, w1, w2, w3, w4, w5];
            let mut v = filter6_8(w);
            if AVG != 0 {
                v = vrhadd_u8(v, w[AVG]);
            }
            st8(gd.row_mut::<8>(k, 0), v);
            (w0, w1, w2, w3, w4) = (w1, w2, w3, w4, w5);
        }
        y += ROW_GROUP;
    }
    while y < H {
        let w5 = ld8(&s.row::<8>(y + 5, 0));
        let w = [w0, w1, w2, w3, w4, w5];
        let mut v = filter6_8(w);
        if AVG != 0 {
            v = vrhadd_u8(v, w[AVG]);
        }
        st8(d.row_mut::<8>(y, 0), v);
        (w0, w1, w2, w3, w4) = (w1, w2, w3, w4, w5);
        y += 1;
    }
}

/// `McHorVer02WidthEq4_AArch64_neon` and its forms: the asm pairs two rows into one
/// register; this keeps one row per register with the upper lanes idle, which is
/// what its `Height5` form does anyway.
#[inline]
#[target_feature(enable = "neon")]
fn ver4_block<S: RefSamples + Copy, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<4, SH>(-2, 0);
    let mut d = dst.span_mut::<4, H>(0, 0);
    // The six-row window as its own bindings rather than an array the rows shift
    // through: the slide is then register renaming, not a `copy_within`.
    let (mut w0, mut w1, mut w2, mut w3, mut w4) = (
        ld4(&s.row::<4>(0, 0)),
        ld4(&s.row::<4>(1, 0)),
        ld4(&s.row::<4>(2, 0)),
        ld4(&s.row::<4>(3, 0)),
        ld4(&s.row::<4>(4, 0)),
    );
    let mut y = 0;
    while y + ROW_GROUP <= H {
        let g = s.window::<4>(y + 5, ROW_GROUP);
        let mut gd = d.window_mut::<4>(y, ROW_GROUP);
        for k in 0..ROW_GROUP {
            let w5 = ld4(&g.row::<4>(k, 0));
            let w = [w0, w1, w2, w3, w4, w5];
            let mut v = filter6_8(w);
            if AVG != 0 {
                v = vrhadd_u8(v, w[AVG]);
            }
            st4(gd.row_mut::<4>(k, 0), v);
            (w0, w1, w2, w3, w4) = (w1, w2, w3, w4, w5);
        }
        y += ROW_GROUP;
    }
    while y < H {
        let w5 = ld4(&s.row::<4>(y + 5, 0));
        let w = [w0, w1, w2, w3, w4, w5];
        let mut v = filter6_8(w);
        if AVG != 0 {
            v = vrhadd_u8(v, w[AVG]);
        }
        st4(d.row_mut::<4>(y, 0), v);
        (w0, w1, w2, w3, w4) = (w1, w2, w3, w4, w5);
        y += 1;
    }
}

/// The widths upstream has no routine for, at a const shape.
#[inline]
fn ver_odd_block<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, SH>(-2, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        s.row::<W>(0, 0),
        s.row::<W>(1, 0),
        s.row::<W>(2, 0),
        s.row::<W>(3, 0),
        s.row::<W>(4, 0),
    );
    for y in 0..H {
        let r5 = s.row::<W>(y + 5, 0);
        let out = d.row_mut::<W>(y, 0);
        for x in 0..W {
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

/// The vertical filter at a const shape: the width picks the lane count, and the
/// `match` folds because `W` is a constant.
#[inline]
fn ver_block<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe {
        match W {
            16 => ver16_block::<S, H, SH, AVG>(src, dst),
            8 => ver8_block::<S, H, SH, AVG>(src, dst),
            4 => ver4_block::<S, H, SH, AVG>(src, dst),
            _ => ver_odd_block::<S, W, H, SH, AVG>(src, dst),
        }
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`].
fn ver_any<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let w: [u8; 6] = std::array::from_fn(|k| src.at(x as isize, dy + k as isize - 2));
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
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

/// One row of the centre kernel's six-row window, as up to three eight-column
/// vectors — the chunking the vertical pass filters in, and never more than three
/// because `SW <= 22` (see the header's width paragraph).
///
/// The row arrives as **vectors, not bytes**: the window slides by one output row,
/// so a byte array here would be five array copies a row on top of the load. `r` is
/// a one-row window of width `SW`, which is what makes the chunk offsets constants
/// that fold — see [`taps16`].
#[inline]
#[target_feature(enable = "neon")]
fn cen_row<R: BlockRows, const SW: usize>(r: &R, y: usize) -> [uint8x8_t; 3] {
    const { assert!(SW >= 8, "the centre kernel's rows are at least W + 5 = 9 wide") };
    let mut v = [vdup_n_u8(0); 3];
    let (mut j, mut c) = (0usize, 0usize);
    while j + 8 <= SW {
        v[c] = ld8(&r.row::<8>(y, j));
        c += 1;
        j += 8;
    }
    // The columns the full chunks leave: one more, ending at `SW`, overlapping the
    // chunk before it exactly as the horizontal filter's last chunk does.
    if j < SW {
        v[c] = ld8(&r.row::<8>(y, SW - 8));
    }
    v
}

/// One output row of the centre kernel: the vertical 6-tap into `tmp`, the window
/// slide, then the horizontal 6-tap over `tmp` into `out`.
///
/// A macro for the reason [`hor_row`] is one — the row index has to stay a constant
/// at the point the bounds checks are decided, and a `#[target_feature]` function
/// cannot be `#[inline(always)]`. `W` and `SW` come from the block kernel's own const
/// parameters.
macro_rules! cen_out_row {
    ($out:expr, $new:expr, $tmp:expr, $w0:ident, $w1:ident, $w2:ident, $w3:ident, $w4:ident) => {{
        let w5 = $new;
        let out: &mut [u8; W] = $out;

        // The vertical pass, into 16-bit intermediates: eight columns per chunk,
        // and a last chunk ending at column `SW` for the columns the others leave.
        let (mut j, mut c) = (0usize, 0usize);
        while j + 8 <= SW {
            st8_i16(&mut $tmp[j..], tap6_8([$w0[c], $w1[c], $w2[c], $w3[c], $w4[c], w5[c]]));
            c += 1;
            j += 8;
        }
        if j < SW {
            st8_i16(&mut $tmp[SW - 8..], tap6_8([$w0[c], $w1[c], $w2[c], $w3[c], $w4[c], w5[c]]));
        }
        ($w0, $w1, $w2, $w3, $w4) = ($w1, $w2, $w3, $w4, w5);

        // The horizontal pass over them, the same way.
        let mut x = 0;
        while x + 8 <= W {
            st8(&mut out[x..], hor6_16bit(&$tmp[x..]));
            x += 8;
        }
        if x + 4 <= W {
            st4(&mut out[x..], hor6_16bit(&$tmp[x..]));
            x += 4;
        }
        if x < W {
            if W >= 8 {
                st8(&mut out[W - 8..], hor6_16bit(&$tmp[W - 8..]));
            } else if W >= 4 {
                st4(&mut out[W - 4..], hor6_16bit(&$tmp[W - 4..]));
            } else {
                while x < W {
                    let w: [i16; 6] = $tmp[x..x + 6].try_into().expect("six taps");
                    out[x] = WelsClip1((hor_filter_input_16bit(&w) + 512) >> 10);
                    x += 1;
                }
            }
        }
    }};
}

/// `McHorVer22WidthEq16_AArch64_neon` and the `Width17/9/5` forms; see the header
/// for the widths and for the horizontal pass's precision.
///
/// The reach is `SW = W + 5` columns by `SH = H + 5` rows and it is cut **once**;
/// the two passes and their arithmetic are unchanged, including the widened `.4s`
/// horizontal pass the header documents as this port's departure from the asm.
#[inline]
#[target_feature(enable = "neon")]
fn cen_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    // `iTmp` is `int16_t[17 + 5]` in the C++ and the widest caller is `md.rs`'s
    // `kiW + 1` with `kiW = 16`. The scratch here is wider so the eight-lane loads of
    // the horizontal pass may run past the last valid column, but the contract on
    // `width` is the C++'s, and the one the x86_64 kernel asserts.
    let s = src.span::<SW, SH>(-2, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let mut tmp = [0i16; 32];
    // Five separate row values rather than one array the window slides through:
    // each is three vectors, and keeping them as their own bindings is what lets the
    // slide below be register renaming instead of a `copy_within` over 144 bytes.
    let (mut w0, mut w1, mut w2, mut w3, mut w4) = (
        cen_row::<_, SW>(&s, 0),
        cen_row::<_, SW>(&s, 1),
        cen_row::<_, SW>(&s, 2),
        cen_row::<_, SW>(&s, 3),
        cen_row::<_, SW>(&s, 4),
    );
    let mut y = 0;
    while y + ROW_GROUP <= H {
        let g = s.window::<SW>(y + 5, ROW_GROUP);
        let mut gd = d.window_mut::<W>(y, ROW_GROUP);
        for k in 0..ROW_GROUP {
            cen_out_row!(gd.row_mut::<W>(k, 0), cen_row::<_, SW>(&g, k), tmp, w0, w1, w2, w3, w4);
        }
        y += ROW_GROUP;
    }
    while y < H {
        cen_out_row!(
            d.row_mut::<W>(y, 0),
            cen_row::<_, SW>(&s.window::<SW>(y + 5, 1), 0),
            tmp,
            w0,
            w1,
            w2,
            w3,
            w4
        );
        y += 1;
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`]. The `width <= 17` contract is
/// the C++'s and is what sizes `iTmp`.
fn cen_any<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    assert!(width <= 17, "mc_hor_ver22 width {width} exceeds the 17 iTmp is sized for");
    let n = width + 5;
    let mut tmp = [0i16; 32];
    for dy in 0..height as isize {
        for (j, t) in tmp[..n].iter_mut().enumerate() {
            let x = j as isize - 2;
            let w: [u8; 6] = std::array::from_fn(|k| src.at(x, dy + k as isize - 2));
            *t = filter_input_8bit(&w) as i16;
        }
        let out = dst.row_mut(dy, 0, width);
        for (o, w) in out.iter_mut().zip(tmp[..n].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(w.try_into().expect("six taps")) + 512) >> 10);
        }
    }
}

// ============================================================================
// Averaging and chroma
// ============================================================================

/// `PixelAvgWidthEq16/8/4_AArch64_neon`, one row: `urhadd` per chunk. The width is
/// a const parameter for the reason [`hor_row`]'s is.
#[inline]
#[target_feature(enable = "neon")]
fn avg_row<const W: usize>(out: &mut [u8; W], a: &[u8; W], b: &[u8; W]) {
    let mut x = 0;
    while x + 16 <= W {
        st16(&mut out[x..], vrhaddq_u8(ld16(&a[x..]), ld16(&b[x..])));
        x += 16;
    }
    if x + 8 <= W {
        st8(&mut out[x..], vrhadd_u8(ld8(&a[x..]), ld8(&b[x..])));
        x += 8;
    }
    if x + 4 <= W {
        st4(&mut out[x..], vrhadd_u8(ld4(&a[x..]), ld4(&b[x..])));
        x += 4;
    }
    while x < W {
        out[x] = ((a[x] as u32 + b[x] as u32 + 1) >> 1) as u8;
        x += 1;
    }
}

/// `PixelAvgWidthEq16/8/4_AArch64_neon` over one const-shape block: one span per
/// operand, then `urhadd` per chunk per row.
#[inline]
#[target_feature(enable = "neon")]
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
            *o = ((a.at(j as isize, dy) as u32 + b.at(j as isize, dy) as u32 + 1) >> 1) as u8;
        }
    }
}

/// `McChromaWidthEq8_AArch64_neon`: `umull`/`umlal` by the four byte weights, the
/// bottom row of one output row being the top row of the next.
#[inline]
#[target_feature(enable = "neon")]
fn chroma8_block<S: RefSamples + Copy, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    let (wa, wb, wc, wd) = (vdup_n_u8(w[0]), vdup_n_u8(w[1]), vdup_n_u8(w[2]), vdup_n_u8(w[3]));
    let s = src.span::<9, SH>(0, 0);
    let mut d = dst.span_mut::<8, H>(0, 0);
    // A one-row window per row, so the two overlapping eight-byte loads come
    // straight out of the plane or the cell view — see [`taps16`].
    let r = s.window::<9>(0, 1);
    let (mut a, mut b) = (ld8(&r.row::<8>(0, 0)), ld8(&r.row::<8>(0, 1)));
    for y in 0..H {
        let r = s.window::<9>(y + 1, 1);
        let (c, e) = (ld8(&r.row::<8>(0, 0)), ld8(&r.row::<8>(0, 1)));
        let t = vmull_u8(a, wa);
        let t = vmlal_u8(t, b, wb);
        let t = vmlal_u8(t, c, wc);
        let t = vmlal_u8(t, e, wd);
        st8(d.row_mut::<8>(y, 0), vrshrn_n_u16::<6>(t));
        a = c;
        b = e;
    }
}

/// `McChromaWidthEq4_AArch64_neon`, one row per register.
#[inline]
#[target_feature(enable = "neon")]
fn chroma4_block<S: RefSamples + Copy, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    let (wa, wb, wc, wd) = (vdup_n_u8(w[0]), vdup_n_u8(w[1]), vdup_n_u8(w[2]), vdup_n_u8(w[3]));
    let s = src.span::<5, SH>(0, 0);
    let mut d = dst.span_mut::<4, H>(0, 0);
    let r = s.window::<5>(0, 1);
    let (mut a, mut b) = (ld4(&r.row::<4>(0, 0)), ld4(&r.row::<4>(0, 1)));
    for y in 0..H {
        let r = s.window::<5>(y + 1, 1);
        let (c, e) = (ld4(&r.row::<4>(0, 0)), ld4(&r.row::<4>(0, 1)));
        let t = vmull_u8(a, wa);
        let t = vmlal_u8(t, b, wb);
        let t = vmlal_u8(t, c, wc);
        let t = vmlal_u8(t, e, wd);
        st4(d.row_mut::<4>(y, 0), vrshrn_n_u16::<6>(t));
        a = c;
        b = e;
    }
}

/// The 2-wide chroma block, which upstream has no NEON routine for: the scalar over
/// the same two spans.
#[inline]
fn chroma_odd_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    let (a, b, c, e) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    let s = src.span::<SW, SH>(0, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let (r0, r1) = (s.row::<SW>(y, 0), s.row::<SW>(y + 1, 0));
        let out = d.row_mut::<W>(y, 0);
        for j in 0..W {
            out[j] = ((a * r0[j] as i32 + b * r0[j + 1] as i32 + c * r1[j] as i32 + e * r1[j + 1] as i32 + 32) >> 6) as u8;
        }
    }
}

/// The bilinear chroma filter at a const shape: the width picks the kernel, and the
/// `match` folds because `W` is a constant.
#[inline]
fn chroma_block<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe {
        match W {
            8 => chroma8_block::<S, H, SH>(src, dst, w),
            4 => chroma4_block::<S, H, SH>(src, dst, w),
            _ => chroma_odd_block::<S, W, SW, H, SH>(src, dst, w),
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
    let (a, b, c, e) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (j, o) in out.iter_mut().enumerate() {
            let x = j as isize;
            *o = ((a * src.at(x, dy) as i32
                + b * src.at(x + 1, dy) as i32
                + c * src.at(x, dy + 1) as i32
                + e * src.at(x + 1, dy + 1) as i32
                + 32)
                >> 6) as u8;
        }
    }
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

/// **The NEON leaf set** — `McLeaves` over the const-shape blocks above, for the
/// shape dispatch and the quarter-pel composites in `common/mc.rs`.
pub struct NeonLeaves;

impl McLeaves for NeonLeaves {
    /// `McLuma_AArch64_neon` has `McHorVer10/30/01/03` as single kernels; this file's
    /// `AVG` parameter is them. See [`McLeaves::FUSED_QPEL`].
    const FUSED_QPEL: bool = true;

    #[inline(always)]
    fn hor<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        // SAFETY: NEON is baseline on aarch64; see the module header.
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
        // SAFETY: NEON is baseline on aarch64; see the module header.
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
        // SAFETY: NEON is baseline on aarch64; see the module header.
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

/// `PixelAvg_AArch64_neon`.
#[inline]
pub fn pixel_avg<A: RefSamples, B: RefSamples>(dst: &mut PlaneCursorMut<'_>, a: &A, b: &B, width: usize, height: usize) {
    avg_shaped::<NeonLeaves, A, B>(dst, a, b, width, height)
}

/// `McChroma_AArch64_neon`: the copy path on a whole-sample vector, else the
/// bilinear kernels.
#[inline]
pub fn mc_chroma<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize) {
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
fn mc_chroma_frac<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize) {
    if width == 0 {
        return;
    }
    let w = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    chroma_shaped::<NeonLeaves, S>(src, dst, w, width, height)
}

/// `McHorVer20_AArch64_neon` and `McHorVer20Width5Or9Or17_AArch64_neon`.
#[inline]
pub fn mc_hor_ver20<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    hor_shaped::<NeonLeaves, S, 0>(src, dst, width, height)
}

/// `McHorVer02_AArch64_neon` and `McHorVer02Height5Or9Or17_AArch64_neon`.
#[inline]
pub fn mc_hor_ver02<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    ver_shaped::<NeonLeaves, S, 0>(src, dst, width, height)
}

/// `McHorVer22_AArch64_neon` and `McHorVer22Width5Or9Or17Height5Or9Or17_AArch64_neon`.
#[inline]
pub fn mc_hor_ver22<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    cen_shaped::<NeonLeaves, S>(src, dst, width, height)
}

/// `McLuma_AArch64_neon`: the four fused quarter-pel kernels where upstream has
/// them, and the composites over the NEON leaves elsewhere — the split
/// [`McLeaves::FUSED_QPEL`] makes, inside `mc_luma_with`'s shape dispatch so the
/// fused arms are instantiated at the luma partitions and nowhere else.
#[inline]
pub fn mc_luma<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize) {
    crate::common::mc::mc_luma_with::<NeonLeaves, S>(src, dst, mv_x, mv_y, width, height)
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
        for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4), (17, 16), (9, 8), (5, 4), (2, 2)] {
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
        for &(w, h) in &[
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (17, 16), (17, 8), (9, 16), (9, 8),
            // Outside the const tables, so these drive the run-time fallback.
            (5, 8), (5, 4), (17, 17),
        ] {
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
        for &(w, h) in &[
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (16, 17), (16, 9), (8, 17), (8, 9),
            // Outside the const tables, so these drive the run-time fallback.
            (8, 5), (4, 5), (17, 17),
        ] {
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
        for &(w, h) in &[
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (17, 17), (17, 9), (9, 17), (9, 9),
            // Outside the const tables, so these drive the run-time fallback.
            (9, 5), (5, 5), (17, 16),
        ] {
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
}
