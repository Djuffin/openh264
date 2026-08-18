#![deny(unsafe_code)]
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

/*!
 * OpenH264 Decoder: Spatial Intra Prediction Module
 *
 * Implements 4x4 Luma, 8x8 Luma (High Profile), 8x8 Chroma, and 16x16 Luma
 * intra prediction algorithms according to ITU-T H.264 / ISO/IEC 14496-10.
 */

pub const I4x4_COUNT: usize = 4;
pub const I8x8_COUNT: usize = 8;
pub const I16x16_COUNT: usize = 16;

use crate::safe::plane::PlaneCursorMut;

// T5.X8: two more duplicate dispatch typedefs — `decoder_context.rs` holds the
// pair the tables are actually typed by, and these two, unused here, described the
// deleted wrappers' signature. S18's straggler class.

#[inline(always)]
pub fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        ((-iX) >> 31) as u8
    } else {
        iX as u8
    }
}

// ============================================================================
// Safe kernels (plan §Phase 2, recipe R2)
// ============================================================================
//
// These are the implementations; the `Wels*_c` functions below are strangler
// shims (R7) that build a `PlaneCursorMut` from the raw pointer and call in here,
// so no call site and no dispatch-table installer changes in this phase.
//
// Unlike the pilot family, **every kernel here reads outside its own block** —
// the row above at `dy == -1` and the column to the left at `dx == -1`, and for
// the diagonal modes the row above extends up to 8 (4x4) or 16 (8x8, 16x16)
// samples to the right of the block's left edge. That is legal because the block
// always sits inside a `PADDING_LENGTH`-padded picture plane; each shim's
// `# Safety` contract states the exact span, and it is the same contract for
// every kernel of a given block size.
//
// Two idioms carry the whole file, and both are plain byte moves rather than the
// word punning they replace (taxonomy T7):
//
//   `ST32(p, 0x01010101 * v)` / `ST64(p, 0x0101010101010101 * v)`
//       -> `row_mut(dy, 0, n).fill(v)`.  The multiply existed only to splat one
//          byte across a machine word for the store; `fill` is that, exactly, and
//          it is endian-neutral by construction rather than by argument.
//   `ST32(p, LD32(q))` / a window of a local `kuiList`
//       -> `row_mut(dy, 0, n).copy_from_slice(&list[k..k + n])`.  Also a pure
//          byte move. Note this file has **no** punned access that is used
//          arithmetically, so `u32::from_ne_bytes` is not needed anywhere in it.

/// The four samples of the row above a 4x4 block, `dx` in `0..4`.
#[inline]
fn top4(pred: &PlaneCursorMut<'_>) -> [u8; 4] {
    pred.row(-1, 0, 4).try_into().unwrap()
}

/// The eight samples of the row above a 4x4 block, `dx` in `0..8` — the diagonal
/// modes read four samples past the block's right edge.
#[inline]
fn top8(pred: &PlaneCursorMut<'_>) -> [u8; 8] {
    pred.row(-1, 0, 8).try_into().unwrap()
}

/// The four samples of the column left of a 4x4 block, `dy` in `0..4`.
#[inline]
fn left4(pred: &PlaneCursorMut<'_>) -> [u8; 4] {
    [pred.at(-1, 0), pred.at(-1, 1), pred.at(-1, 2), pred.at(-1, 3)]
}

/// Writes `rows[k]` of a 4x4 block from `list[off[k] .. off[k] + 4]`.
///
/// The C++ writes these four rows as four unaligned `u32` stores from sliding
/// windows of a local `kuiList`; the window offsets are the mode's whole identity,
/// which is why they stay explicit here rather than being folded into a formula.
#[inline]
fn write4x4_windows(pred: &mut PlaneCursorMut<'_>, list: &[u8], off: [usize; 4]) {
    for (dy, &o) in off.iter().enumerate() {
        pred.row_mut(dy as isize, 0, 4).copy_from_slice(&list[o..o + 4]);
    }
}

/// Fills all four rows of a 4x4 block with `v`.
#[inline]
fn fill4x4(pred: &mut PlaneCursorMut<'_>, v: u8) {
    for dy in 0..4 {
        pred.row_mut(dy, 0, 4).fill(v);
    }
}

/// C++: `WelsI4x4LumaPredV_c`, `codec/decoder/core/src/get_intra_predictor.cpp`.
pub fn i4x4_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = top4(pred);
    for dy in 0..4 {
        pred.row_mut(dy, 0, 4).copy_from_slice(&top);
    }
}

/// C++: `WelsI4x4LumaPredH_c`.
pub fn i4x4_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    let left = left4(pred);
    for (dy, &v) in left.iter().enumerate() {
        pred.row_mut(dy as isize, 0, 4).fill(v);
    }
}

/// C++: `WelsI4x4LumaPredDc_c`.
pub fn i4x4_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let left = left4(pred);
    let top = top4(pred);
    let sum: u32 = left.iter().chain(top.iter()).map(|&v| v as u32).sum::<u32>() + 4;
    fill4x4(pred, (sum >> 3) as u8);
}

/// C++: `WelsI4x4LumaPredDcLeft_c`.
pub fn i4x4_luma_pred_dc_left(pred: &mut PlaneCursorMut<'_>) {
    let left = left4(pred);
    let sum: u32 = left.iter().map(|&v| v as u32).sum::<u32>() + 2;
    fill4x4(pred, (sum >> 2) as u8);
}

/// C++: `WelsI4x4LumaPredDcTop_c`.
pub fn i4x4_luma_pred_dc_top(pred: &mut PlaneCursorMut<'_>) {
    let top = top4(pred);
    let sum: u32 = top.iter().map(|&v| v as u32).sum::<u32>() + 2;
    fill4x4(pred, (sum >> 2) as u8);
}

/// C++: `WelsI4x4LumaPredDcNA_c` — no neighbours available, so the mid-grey 128.
pub fn i4x4_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    fill4x4(pred, 0x80);
}

/// C++: `WelsI4x4LumaPredDDL_c`.
pub fn i4x4_luma_pred_ddl(pred: &mut PlaneCursorMut<'_>) {
    let t = top8(pred).map(|v| v as u32);

    let mut list = [0u8; 8];
    for k in 0..6 {
        list[k] = ((2 + t[k] + t[k + 2] + (t[k + 1] << 1)) >> 2) as u8;
    }
    list[6] = ((2 + t[6] + t[7] + (t[7] << 1)) >> 2) as u8;

    write4x4_windows(pred, &list, [0, 1, 2, 3]);
}

/// C++: `WelsI4x4LumaPredDDLTop_c` — the top-right block is unavailable, so `T3`
/// stands in for `T4..T7`.
pub fn i4x4_luma_pred_ddl_top(pred: &mut PlaneCursorMut<'_>) {
    let t = top4(pred).map(|v| v as u32);

    let t01 = 1 + t[0] + t[1];
    let t12 = 1 + t[1] + t[2];
    let t23 = 1 + t[2] + t[3];
    let t33 = 1 + (t[3] << 1);

    let d3 = (t33 >> 1) as u8;
    let list = [
        ((t01 + t12) >> 2) as u8,
        ((t12 + t23) >> 2) as u8,
        ((t23 + t33) >> 2) as u8,
        d3,
        d3,
        d3,
        d3,
        d3,
    ];

    write4x4_windows(pred, &list, [0, 1, 2, 3]);
}

/// C++: `WelsI4x4LumaPredDDR_c`.
pub fn i4x4_luma_pred_ddr(pred: &mut PlaneCursorMut<'_>) {
    let lt = pred.at(-1, -1) as u32;
    let l = left4(pred).map(|v| v as u32);
    let t = top4(pred).map(|v| v as u32);

    let tl0 = 1 + lt + l[0];
    let lt0 = 1 + lt + t[0];
    let t01 = 1 + t[0] + t[1];
    let t12 = 1 + t[1] + t[2];
    let t23 = 1 + t[2] + t[3];
    let l01 = 1 + l[0] + l[1];
    let l12 = 1 + l[1] + l[2];
    let l23 = 1 + l[2] + l[3];

    let list = [
        ((l12 + l23) >> 2) as u8,
        ((l01 + l12) >> 2) as u8,
        ((tl0 + l01) >> 2) as u8,
        ((tl0 + lt0) >> 2) as u8,
        ((lt0 + t01) >> 2) as u8,
        ((t01 + t12) >> 2) as u8,
        ((t12 + t23) >> 2) as u8,
        0,
    ];

    write4x4_windows(pred, &list, [3, 2, 1, 0]);
}

/// C++: `WelsI4x4LumaPredVL_c`.
pub fn i4x4_luma_pred_vl(pred: &mut PlaneCursorMut<'_>) {
    let t7: [u8; 7] = pred.row(-1, 0, 7).try_into().unwrap();
    let t = t7.map(|v| v as u32);

    let p = [
        1 + t[0] + t[1],
        1 + t[1] + t[2],
        1 + t[2] + t[3],
        1 + t[3] + t[4],
        1 + t[4] + t[5],
        1 + t[5] + t[6],
    ];

    let list = [
        (p[0] >> 1) as u8,
        (p[1] >> 1) as u8,
        (p[2] >> 1) as u8,
        (p[3] >> 1) as u8,
        (p[4] >> 1) as u8,
        ((p[0] + p[1]) >> 2) as u8,
        ((p[1] + p[2]) >> 2) as u8,
        ((p[2] + p[3]) >> 2) as u8,
        ((p[3] + p[4]) >> 2) as u8,
        ((p[4] + p[5]) >> 2) as u8,
    ];

    write4x4_windows(pred, &list, [0, 5, 1, 6]);
}

/// C++: `WelsI4x4LumaPredVLTop_c`.
pub fn i4x4_luma_pred_vl_top(pred: &mut PlaneCursorMut<'_>) {
    let t = top4(pred).map(|v| v as u32);

    let t01 = 1 + t[0] + t[1];
    let t12 = 1 + t[1] + t[2];
    let t23 = 1 + t[2] + t[3];
    let t33 = 1 + (t[3] << 1);

    let v3 = (t33 >> 1) as u8;
    let v7 = v3;
    let list = [
        (t01 >> 1) as u8,
        (t12 >> 1) as u8,
        (t23 >> 1) as u8,
        v3,
        v3,
        ((t01 + t12) >> 2) as u8,
        ((t12 + t23) >> 2) as u8,
        ((t23 + t33) >> 2) as u8,
        v7,
        v7,
    ];

    write4x4_windows(pred, &list, [0, 5, 1, 6]);
}

/// C++: `WelsI4x4LumaPredVR_c`.
pub fn i4x4_luma_pred_vr(pred: &mut PlaneCursorMut<'_>) {
    let lt = pred.at(-1, -1) as u32;
    let l = left4(pred).map(|v| v as u32);
    let t = top4(pred).map(|v| v as u32);

    let list = [
        ((2 + lt + (l[0] << 1) + l[1]) >> 2) as u8,
        ((1 + lt + t[0]) >> 1) as u8,
        ((1 + t[0] + t[1]) >> 1) as u8,
        ((1 + t[1] + t[2]) >> 1) as u8,
        ((1 + t[2] + t[3]) >> 1) as u8,
        ((2 + l[0] + (l[1] << 1) + l[2]) >> 2) as u8,
        ((2 + l[0] + (lt << 1) + t[0]) >> 2) as u8,
        ((2 + lt + (t[0] << 1) + t[1]) >> 2) as u8,
        ((2 + t[0] + (t[1] << 1) + t[2]) >> 2) as u8,
        ((2 + t[1] + (t[2] << 1) + t[3]) >> 2) as u8,
    ];

    write4x4_windows(pred, &list, [1, 6, 0, 5]);
}

/// C++: `WelsI4x4LumaPredHU_c`.
pub fn i4x4_luma_pred_hu(pred: &mut PlaneCursorMut<'_>) {
    let l = left4(pred).map(|v| v as u32);

    let l01 = 1 + l[0] + l[1];
    let l12 = 1 + l[1] + l[2];
    let l23 = 1 + l[2] + l[3];
    let l3 = l[3] as u8;

    let list = [
        (l01 >> 1) as u8,
        ((l01 + l12) >> 2) as u8,
        (l12 >> 1) as u8,
        ((l12 + l23) >> 2) as u8,
        (l23 >> 1) as u8,
        ((1 + l23 + (l[3] << 1)) >> 2) as u8,
        l3,
        l3,
        l3,
        l3,
    ];

    write4x4_windows(pred, &list, [0, 2, 4, 6]);
}

/// C++: `WelsI4x4LumaPredHD_c`.
pub fn i4x4_luma_pred_hd(pred: &mut PlaneCursorMut<'_>) {
    let lt = pred.at(-1, -1) as u32;
    let l = left4(pred).map(|v| v as u32);
    let t = top4(pred).map(|v| v as u32);

    let tl0 = 1 + lt + l[0];
    let lt0 = 1 + lt + t[0];
    let t01 = 1 + t[0] + t[1];
    let t12 = 1 + t[1] + t[2];
    let l01 = 1 + l[0] + l[1];
    let l12 = 1 + l[1] + l[2];
    let l23 = 1 + l[2] + l[3];

    let list = [
        (l23 >> 1) as u8,
        ((l12 + l23) >> 2) as u8,
        (l12 >> 1) as u8,
        ((l01 + l12) >> 2) as u8,
        (l01 >> 1) as u8,
        ((tl0 + l01) >> 2) as u8,
        (tl0 >> 1) as u8,
        ((tl0 + lt0) >> 2) as u8,
        ((lt0 + t01) >> 2) as u8,
        ((t01 + t12) >> 2) as u8,
    ];

    write4x4_windows(pred, &list, [6, 4, 2, 0]);
}

// --- 8x8 luma (High Profile) ------------------------------------------------
//
// Every 8x8 mode opens with the same prologue: low-pass the neighbours with a
// 3-tap `(a + 2b + c + 2) >> 2` before predicting from them. The C++ writes that
// prologue out longhand in each of the fourteen kernels, with the two ends
// special-cased on `bTLAvail`/`bTRAvail`; here it is three shared functions, which
// is the one place this file departs from a line-for-line transliteration. The
// per-mode *bodies* stay exactly as the C++ writes them.

/// `(a + 2b + c + 2) >> 2` — the 3-tap low-pass applied to every neighbour sample.
#[inline]
fn tap3(a: u8, b: u8, c: u8) -> u8 {
    ((a as u32 + ((b as u32) << 1) + c as u32 + 2) >> 2) as u8
}

/// `(3a + b + 2) >> 2` — the edge form, where the third sample does not exist and
/// `a` is weighted three times instead.
#[inline]
fn tap3_edge(a: u8, b: u8) -> u8 {
    ((a as u32 * 3 + b as u32 + 2) >> 2) as u8
}

/// `(a + b + 1) >> 1` — the 2-tap average the VL/HU/VR/HD modes use on half-pel
/// positions.
#[inline]
fn tap2(a: u8, b: u8) -> u8 {
    ((a as u32 + b as u32 + 1) >> 1) as u8
}

/// The eight filtered samples of the row above an 8x8 block.
///
/// `tl_avail` decides whether the first sample may use the top-left corner;
/// `tr_avail` whether the last may use `T8`. **`T8` is read only when `tr_avail`**,
/// exactly as the C++ does — reading it unconditionally would widen every shim's
/// contract by one byte for no gain.
fn i8x8_filter_top8(pred: &PlaneCursorMut<'_>, tl_avail: bool, tr_avail: bool) -> [u8; 8] {
    let t: [u8; 8] = pred.row(-1, 0, 8).try_into().unwrap();
    let mut f = [0u8; 8];
    f[0] = if tl_avail {
        tap3(pred.at(-1, -1), t[0], t[1])
    } else {
        tap3_edge(t[0], t[1])
    };
    for i in 1..7 {
        f[i] = tap3(t[i - 1], t[i], t[i + 1]);
    }
    f[7] = if tr_avail {
        tap3(t[6], t[7], pred.at(8, -1))
    } else {
        tap3_edge(t[7], t[6])
    };
    f
}

/// The eight filtered samples of the column left of an 8x8 block.
fn i8x8_filter_left8(pred: &PlaneCursorMut<'_>, tl_avail: bool) -> [u8; 8] {
    let l: [u8; 8] = std::array::from_fn(|i| pred.at(-1, i as isize));
    let mut f = [0u8; 8];
    f[0] = if tl_avail {
        tap3(pred.at(-1, -1), l[0], l[1])
    } else {
        tap3_edge(l[0], l[1])
    };
    for i in 1..7 {
        f[i] = tap3(l[i - 1], l[i], l[i + 1]);
    }
    f[7] = tap3_edge(l[7], l[6]);
    f
}

/// The filtered top-left corner sample — `tap3(L0, TL, T0)`, used by DDR, VR and HD.
#[inline]
fn i8x8_filter_tl(pred: &PlaneCursorMut<'_>) -> u8 {
    tap3(pred.at(-1, 0), pred.at(-1, -1), pred.at(0, -1))
}

/// Sixteen filtered samples of the row above an 8x8 block — DDL and VL predict from
/// samples up to eight columns past the block's right edge.
fn i8x8_filter_top16(pred: &PlaneCursorMut<'_>, tl_avail: bool) -> [u8; 16] {
    let t: [u8; 16] = pred.row(-1, 0, 16).try_into().unwrap();
    let mut f = [0u8; 16];
    f[0] = if tl_avail {
        tap3(pred.at(-1, -1), t[0], t[1])
    } else {
        tap3_edge(t[0], t[1])
    };
    for i in 1..15 {
        f[i] = tap3(t[i - 1], t[i], t[i + 1]);
    }
    f[15] = tap3_edge(t[15], t[14]);
    f
}

/// Sixteen filtered samples for the `*Top` variants, where the block to the top-right
/// is unavailable: only eight real samples exist and `T7` — **unfiltered**, as the C++
/// has it — stands in for the other eight.
fn i8x8_filter_top16_edge(pred: &PlaneCursorMut<'_>, tl_avail: bool) -> [u8; 16] {
    let t: [u8; 8] = pred.row(-1, 0, 8).try_into().unwrap();
    let mut f = [0u8; 16];
    f[0] = if tl_avail {
        tap3(pred.at(-1, -1), t[0], t[1])
    } else {
        tap3_edge(t[0], t[1])
    };
    for i in 1..7 {
        f[i] = tap3(t[i - 1], t[i], t[i + 1]);
    }
    f[7] = tap3_edge(t[7], t[6]);
    for slot in f[8..].iter_mut() {
        *slot = t[7];
    }
    f
}

/// Fills all eight rows of an 8x8 block with `v`.
#[inline]
fn fill8x8(pred: &mut PlaneCursorMut<'_>, v: u8) {
    for dy in 0..8 {
        pred.row_mut(dy, 0, 8).fill(v);
    }
}

/// C++: `WelsI8x8LumaPredV_c`.
///
/// The C++ packs the eight filtered samples into a `uint64_t` low byte first and
/// stores that word, which reproduces `f[0..8]` in memory **only on a little-endian
/// target**. Copying the bytes says what was meant and is correct everywhere; on
/// every target this project builds for the two are the same bytes.
pub fn i8x8_luma_pred_v(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, tr_avail: bool) {
    let f = i8x8_filter_top8(pred, tl_avail, tr_avail);
    for dy in 0..8 {
        pred.row_mut(dy, 0, 8).copy_from_slice(&f);
    }
}

/// C++: `WelsI8x8LumaPredH_c`.
pub fn i8x8_luma_pred_h(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, _tr_avail: bool) {
    let f = i8x8_filter_left8(pred, tl_avail);
    for (dy, &v) in f.iter().enumerate() {
        pred.row_mut(dy as isize, 0, 8).fill(v);
    }
}

/// C++: `WelsI8x8LumaPredDc_c`.
pub fn i8x8_luma_pred_dc(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, tr_avail: bool) {
    let fl = i8x8_filter_left8(pred, tl_avail);
    let ft = i8x8_filter_top8(pred, tl_avail, tr_avail);
    let total: u32 = fl.iter().chain(ft.iter()).map(|&v| v as u32).sum();
    fill8x8(pred, ((total + 8) >> 4) as u8);
}

/// C++: `WelsI8x8LumaPredDcLeft_c`.
pub fn i8x8_luma_pred_dc_left(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, _tr_avail: bool) {
    let fl = i8x8_filter_left8(pred, tl_avail);
    let total: u32 = fl.iter().map(|&v| v as u32).sum();
    fill8x8(pred, ((total + 4) >> 3) as u8);
}

/// C++: `WelsI8x8LumaPredDcTop_c`.
pub fn i8x8_luma_pred_dc_top(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, tr_avail: bool) {
    let ft = i8x8_filter_top8(pred, tl_avail, tr_avail);
    let total: u32 = ft.iter().map(|&v| v as u32).sum();
    fill8x8(pred, ((total + 4) >> 3) as u8);
}

/// C++: `WelsI8x8LumaPredDcNA_c`.
pub fn i8x8_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>, _tl_avail: bool, _tr_avail: bool) {
    fill8x8(pred, 0x80);
}

/// The shared body of `DDL` and `DDLTop`: both differ only in how their 16 filtered
/// top samples were produced.
fn i8x8_pred_ddl_body(pred: &mut PlaneCursorMut<'_>, f: &[u8; 16]) {
    for i in 0..8 {
        let row = pred.row_mut(i as isize, 0, 8);
        for j in 0..8 {
            row[j] = if i == 7 && j == 7 {
                tap3_edge(f[15], f[14])
            } else {
                tap3(f[i + j], f[i + j + 1], f[i + j + 2])
            };
        }
    }
}

/// C++: `WelsI8x8LumaPredDDL_c`.
pub fn i8x8_luma_pred_ddl(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, _tr_avail: bool) {
    let f = i8x8_filter_top16(pred, tl_avail);
    i8x8_pred_ddl_body(pred, &f);
}

/// C++: `WelsI8x8LumaPredDDLTop_c`.
pub fn i8x8_luma_pred_ddl_top(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, _tr_avail: bool) {
    let f = i8x8_filter_top16_edge(pred, tl_avail);
    i8x8_pred_ddl_body(pred, &f);
}

/// C++: `WelsI8x8LumaPredDDR_c`. `bTLAvail` is ignored by the C++ here — the corner is
/// filtered unconditionally — and the parameter is kept only for the table's ABI.
pub fn i8x8_luma_pred_ddr(pred: &mut PlaneCursorMut<'_>, _tl_avail: bool, tr_avail: bool) {
    let ftl = i8x8_filter_tl(pred);
    let fl = i8x8_filter_left8(pred, true);
    let ft = i8x8_filter_top8(pred, true, tr_avail);

    for i in 0..8usize {
        let row = pred.row_mut(i as isize, 0, 8);
        for j in 0..i.saturating_sub(1) {
            row[j] = tap3(fl[i - j - 2], fl[i - j - 1], fl[i - j]);
        }
        if i >= 1 {
            row[i - 1] = tap3(ftl, fl[0], fl[1]);
        }
        row[i] = tap3(ft[0], ftl, fl[0]);
        if i < 7 {
            row[i + 1] = tap3(ftl, ft[0], ft[1]);
        }
        for j in (i + 2)..8 {
            row[j] = tap3(ft[j - i - 2], ft[j - i - 1], ft[j - i]);
        }
    }
}

/// The shared body of `VL` and `VLTop`.
fn i8x8_pred_vl_body(pred: &mut PlaneCursorMut<'_>, f: &[u8; 16]) {
    for i in 0..8 {
        let base = i >> 1;
        let row = pred.row_mut(i as isize, 0, 8);
        if i & 0x01 == 0 {
            for j in 0..8 {
                row[j] = tap2(f[j + base], f[j + base + 1]);
            }
        } else {
            for j in 0..8 {
                row[j] = tap3(f[j + base], f[j + base + 1], f[j + base + 2]);
            }
        }
    }
}

/// C++: `WelsI8x8LumaPredVL_c`.
pub fn i8x8_luma_pred_vl(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, _tr_avail: bool) {
    let f = i8x8_filter_top16(pred, tl_avail);
    i8x8_pred_vl_body(pred, &f);
}

/// C++: `WelsI8x8LumaPredVLTop_c`.
pub fn i8x8_luma_pred_vl_top(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, _tr_avail: bool) {
    let f = i8x8_filter_top16_edge(pred, tl_avail);
    i8x8_pred_vl_body(pred, &f);
}

/// C++: `WelsI8x8LumaPredVR_c`. As with DDR, `bTLAvail` is unused by the C++ body.
pub fn i8x8_luma_pred_vr(pred: &mut PlaneCursorMut<'_>, _tl_avail: bool, tr_avail: bool) {
    let ftl = i8x8_filter_tl(pred);
    let fl = i8x8_filter_left8(pred, true);
    let ft = i8x8_filter_top8(pred, true, tr_avail);

    for i in 0..8i32 {
        let row = pred.row_mut(i as isize, 0, 8);
        for j in 0..8i32 {
            let z = (j << 1) - i;
            let zdiv = j - (i >> 1);
            row[j as usize] = if z >= 0 {
                if z & 0x01 == 0 {
                    if zdiv > 0 {
                        tap2(ft[(zdiv - 1) as usize], ft[zdiv as usize])
                    } else {
                        tap2(ftl, ft[0])
                    }
                } else if zdiv > 1 {
                    tap3(ft[(zdiv - 2) as usize], ft[(zdiv - 1) as usize], ft[zdiv as usize])
                } else {
                    tap3(ftl, ft[0], ft[1])
                }
            } else if z == -1 {
                tap3(fl[0], ftl, ft[0])
            } else if z < -2 {
                tap3(fl[(-z - 1) as usize], fl[(-z - 2) as usize], fl[(-z - 3) as usize])
            } else {
                tap3(fl[1], fl[0], ftl)
            };
        }
    }
}

/// C++: `WelsI8x8LumaPredHU_c`.
pub fn i8x8_luma_pred_hu(pred: &mut PlaneCursorMut<'_>, tl_avail: bool, _tr_avail: bool) {
    let fl = i8x8_filter_left8(pred, tl_avail);

    for i in 0..8i32 {
        let row = pred.row_mut(i as isize, 0, 8);
        for j in 0..8i32 {
            let z = j + (i << 1);
            row[j as usize] = if z < 13 {
                let h = (z >> 1) as usize;
                if z & 0x01 == 0 {
                    tap2(fl[h], fl[h + 1])
                } else {
                    tap3(fl[h], fl[h + 1], fl[h + 2])
                }
            } else if z == 13 {
                tap3_edge(fl[7], fl[6])
            } else {
                fl[7]
            };
        }
    }
}

/// C++: `WelsI8x8LumaPredHD_c`. As with DDR and VR, `bTLAvail` is unused by the body.
pub fn i8x8_luma_pred_hd(pred: &mut PlaneCursorMut<'_>, _tl_avail: bool, tr_avail: bool) {
    let ftl = i8x8_filter_tl(pred);
    let fl = i8x8_filter_left8(pred, true);
    let ft = i8x8_filter_top8(pred, true, tr_avail);

    for i in 0..8i32 {
        let row = pred.row_mut(i as isize, 0, 8);
        for j in 0..8i32 {
            let z = (i << 1) - j;
            let zdiv = i - (j >> 1);
            row[j as usize] = if z >= 0 {
                if z & 0x01 == 0 {
                    if zdiv == 0 {
                        tap2(ftl, fl[0])
                    } else {
                        tap2(fl[(zdiv - 1) as usize], fl[zdiv as usize])
                    }
                } else if zdiv == 1 {
                    tap3(ftl, fl[0], fl[1])
                } else {
                    tap3(fl[(zdiv - 2) as usize], fl[(zdiv - 1) as usize], fl[zdiv as usize])
                }
            } else if z == -1 {
                tap3(fl[0], ftl, ft[0])
            } else if z < -2 {
                tap3(ft[(-z - 1) as usize], ft[(-z - 2) as usize], ft[(-z - 3) as usize])
            } else {
                tap3(ft[1], ft[0], ftl)
            };
        }
    }
}

// --- 8x8 chroma -------------------------------------------------------------

/// C++: `WelsIChromaPredV_c`.
pub fn chroma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 8] = pred.row(-1, 0, 8).try_into().unwrap();
    for dy in 0..8 {
        pred.row_mut(dy, 0, 8).copy_from_slice(&top);
    }
}

/// C++: `WelsIChromaPredH_c`.
pub fn chroma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..8 {
        let v = pred.at(-1, dy);
        pred.row_mut(dy, 0, 8).fill(v);
    }
}

/// C++: `WelsIChromaPredPlane_c`.
pub fn chroma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    let mut h = 0i32;
    let mut v = 0i32;
    for i in 0..4i32 {
        h += (i + 1) * (pred.at(4 + i as isize, -1) as i32 - pred.at(2 - i as isize, -1) as i32);
        v += (i + 1) * (pred.at(-1, 4 + i as isize) as i32 - pred.at(-1, 2 - i as isize) as i32);
    }

    let a = (pred.at(-1, 7) as i32 + pred.at(7, -1) as i32) << 4;
    let b = (17 * h + 16) >> 5;
    let c = (17 * v + 16) >> 5;

    for i in 0..8i32 {
        let row = pred.row_mut(i as isize, 0, 8);
        for j in 0..8i32 {
            row[j as usize] = WelsClip1((a + b * (j - 3) + c * (i - 3) + 16) >> 5);
        }
    }
}

/// C++: `WelsIChromaPredDc_c` — four quadrant means, not one.
pub fn chroma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 8] = pred.row(-1, 0, 8).try_into().unwrap();
    let left: [u8; 8] = std::array::from_fn(|i| pred.at(-1, i as isize));

    let sum = |s: &[u8]| s.iter().map(|&v| v as u32).sum::<u32>();
    let sum_t0 = sum(&top[0..4]);
    let sum_t1 = sum(&top[4..8]);
    let sum_l0 = sum(&left[0..4]);
    let sum_l1 = sum(&left[4..8]);

    let m1 = ((sum_t0 + sum_l0 + 4) >> 3) as u8;
    let m2 = ((sum_t1 + 2) >> 2) as u8;
    let m3 = ((sum_l1 + 2) >> 2) as u8;
    let m4 = ((sum_t1 + sum_l1 + 4) >> 3) as u8;

    let up = [m1, m1, m1, m1, m2, m2, m2, m2];
    let down = [m3, m3, m3, m3, m4, m4, m4, m4];
    for dy in 0..8 {
        let src = if dy < 4 { &up } else { &down };
        pred.row_mut(dy, 0, 8).copy_from_slice(src);
    }
}

/// C++: `WelsIChromaPredDcLeft_c`.
pub fn chroma_pred_dc_left(pred: &mut PlaneCursorMut<'_>) {
    let left: [u8; 8] = std::array::from_fn(|i| pred.at(-1, i as isize));
    let sum = |s: &[u8]| s.iter().map(|&v| v as u32).sum::<u32>();
    let up = ((sum(&left[0..4]) + 2) >> 2) as u8;
    let down = ((sum(&left[4..8]) + 2) >> 2) as u8;
    for dy in 0..8 {
        pred.row_mut(dy, 0, 8).fill(if dy < 4 { up } else { down });
    }
}

/// C++: `WelsIChromaPredDcTop_c`.
pub fn chroma_pred_dc_top(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 8] = pred.row(-1, 0, 8).try_into().unwrap();
    let sum = |s: &[u8]| s.iter().map(|&v| v as u32).sum::<u32>();
    let m1 = ((sum(&top[0..4]) + 2) >> 2) as u8;
    let m2 = ((sum(&top[4..8]) + 2) >> 2) as u8;
    let m = [m1, m1, m1, m1, m2, m2, m2, m2];
    for dy in 0..8 {
        pred.row_mut(dy, 0, 8).copy_from_slice(&m);
    }
}

/// C++: `WelsIChromaPredDcNA_c`.
pub fn chroma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..8 {
        pred.row_mut(dy, 0, 8).fill(0x80);
    }
}

// --- 16x16 luma -------------------------------------------------------------

/// Fills all sixteen rows of a 16x16 block with `v`.
#[inline]
fn fill16x16(pred: &mut PlaneCursorMut<'_>, v: u8) {
    for dy in 0..16 {
        pred.row_mut(dy, 0, 16).fill(v);
    }
}

/// The sixteen samples of the column left of a 16x16 block.
#[inline]
fn left16(pred: &PlaneCursorMut<'_>) -> [u8; 16] {
    std::array::from_fn(|i| pred.at(-1, i as isize))
}

/// C++: `WelsI16x16LumaPredV_c`.
pub fn i16x16_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 16] = pred.row(-1, 0, 16).try_into().unwrap();
    for dy in 0..16 {
        pred.row_mut(dy, 0, 16).copy_from_slice(&top);
    }
}

/// C++: `WelsI16x16LumaPredH_c`.
pub fn i16x16_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..16 {
        let v = pred.at(-1, dy);
        pred.row_mut(dy, 0, 16).fill(v);
    }
}

/// C++: `WelsI16x16LumaPredPlane_c`.
pub fn i16x16_luma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    let mut h = 0i32;
    let mut v = 0i32;
    for i in 0..8i32 {
        h += (i + 1) * (pred.at(8 + i as isize, -1) as i32 - pred.at(6 - i as isize, -1) as i32);
        v += (i + 1) * (pred.at(-1, 8 + i as isize) as i32 - pred.at(-1, 6 - i as isize) as i32);
    }

    let a = (pred.at(-1, 15) as i32 + pred.at(15, -1) as i32) << 4;
    let b = (5 * h + 32) >> 6;
    let c = (5 * v + 32) >> 6;

    for i in 0..16i32 {
        let row = pred.row_mut(i as isize, 0, 16);
        for j in 0..16i32 {
            row[j as usize] = WelsClip1((a + b * (j - 7) + c * (i - 7) + 16) >> 5);
        }
    }
}

/// C++: `WelsI16x16LumaPredDc_c`.
pub fn i16x16_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 16] = pred.row(-1, 0, 16).try_into().unwrap();
    let left = left16(pred);
    let sum: i32 = top
        .iter()
        .chain(left.iter())
        .map(|&v| v as i32)
        .sum::<i32>();
    fill16x16(pred, ((16 + sum) >> 5) as u8);
}

/// C++: `WelsI16x16LumaPredDcTop_c`.
pub fn i16x16_luma_pred_dc_top(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 16] = pred.row(-1, 0, 16).try_into().unwrap();
    let sum: i32 = top.iter().map(|&v| v as i32).sum();
    fill16x16(pred, ((8 + sum) >> 4) as u8);
}

/// C++: `WelsI16x16LumaPredDcLeft_c`.
pub fn i16x16_luma_pred_dc_left(pred: &mut PlaneCursorMut<'_>) {
    let left = left16(pred);
    let sum: i32 = left.iter().map(|&v| v as i32).sum();
    fill16x16(pred, ((8 + sum) >> 4) as u8);
}

/// C++: `WelsI16x16LumaPredDcNA_c`.
pub fn i16x16_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    fill16x16(pred, 0x80);
}

// **T5.X8: the 42 strangler shims that stood here are deleted.**
//
// Each was an `unsafe extern "C" fn(pPred: *mut u8, kiStride: i32)` whose entire
// body rebuilt a `(len, center)` span from the stride, made one
// `slice::from_raw_parts_mut`, and called the kernel above it. They existed so that
// no call site and no dispatch-table installer had to change in Phase 2 (plan §4
// R7) — and Phase 5's reconstruction bracket now hands the kernels a
// `PlaneCursorMut` built from the picture's own plane, so there is nothing left for
// them to bridge. The dispatch tables in `decoder_core.rs` name the kernels
// directly; `PGetIntraPredFunc` is `Option<fn(&mut PlaneCursorMut<'_>)>`.
//
// `shim_span` went with them: the span it computed was the shim's way of saying
// "this kernel reads one row up and one column left"; `PaddedPlane` says it by
// being padded, and `PlaneCursorMut` bounds-checks the reach against the whole
// plane rather than against a re-derived window.

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_clip1() {
        assert_eq!(WelsClip1(-10), 0);
        assert_eq!(WelsClip1(0), 0);
        assert_eq!(WelsClip1(128), 128);
        assert_eq!(WelsClip1(255), 255);
        assert_eq!(WelsClip1(300), 255);
    }

    /// **T5.X8**: the shims are gone, so this exercises the kernels through the
    /// same cursor the dispatch tables now hand them. The fixture is unchanged —
    /// an 8-stride buffer with the block anchored at (1, 2) — so the values pinned
    /// are the values the pointer form pinned.
    #[test]
    fn test_i4x4_pred_v_h_dc() {
        let mut buf = [0u8; 64];
        let stride = 8usize;
        let center = 2 * stride + 1;

        // Top samples, one row above the block.
        buf[center - stride] = 10;
        buf[center - stride + 1] = 20;
        buf[center - stride + 2] = 30;
        buf[center - stride + 3] = 40;

        i4x4_luma_pred_v(&mut PlaneCursorMut::new(&mut buf, center, stride));
        assert_eq!(buf[center], 10);
        assert_eq!(buf[center + 1], 20);
        assert_eq!(buf[center + stride], 10);
        assert_eq!(buf[center + stride + 3], 40);

        // Left samples, one column left of the block.
        buf[center - 1] = 5;
        buf[center + stride - 1] = 15;
        buf[center + 2 * stride - 1] = 25;
        buf[center + 3 * stride - 1] = 35;

        i4x4_luma_pred_h(&mut PlaneCursorMut::new(&mut buf, center, stride));
        assert_eq!(buf[center], 5);
        assert_eq!(buf[center + 3], 5);
        assert_eq!(buf[center + stride], 15);
        assert_eq!(buf[center + 3 * stride + 3], 35);
    }
}
