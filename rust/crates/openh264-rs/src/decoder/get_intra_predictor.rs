#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
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

pub type PGetIntraPredFunc = unsafe extern "C" fn(pPred: *mut u8, kiLumaStride: i32);
pub type PGetIntraPred8x8Func =
    unsafe extern "C" fn(pPred: *mut u8, kiLumaStride: i32, bTLAvail: bool, bTRAvail: bool);

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

// ============================================================================
// Intra 4x4 Luma Prediction Functions
// ============================================================================

pub unsafe extern "C" fn WelsI4x4LumaPredV_c(pPred: *mut u8, kiStride: i32) {
    let kuiVal = (pPred.offset(-kiStride as isize) as *const u32).read_unaligned();

    (pPred as *mut u32).write_unaligned(kuiVal);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiVal);
    (pPred.offset((kiStride << 1) as isize) as *mut u32).write_unaligned(kuiVal);
    (pPred.offset(((kiStride << 1) + kiStride) as isize) as *mut u32).write_unaligned(kuiVal);
}

pub unsafe extern "C" fn WelsI4x4LumaPredH_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let kuiL0 = 0x01010101u32.wrapping_mul(*pPred.offset(-1) as u32);
    let kuiL1 = 0x01010101u32.wrapping_mul(*pPred.offset(-1 + kiStride as isize) as u32);
    let kuiL2 = 0x01010101u32.wrapping_mul(*pPred.offset(-1 + kiStride2 as isize) as u32);
    let kuiL3 = 0x01010101u32.wrapping_mul(*pPred.offset(-1 + kiStride3 as isize) as u32);

    (pPred as *mut u32).write_unaligned(kuiL0);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiL1);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiL2);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiL3);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDc_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let sum = *pPred.offset(-1) as u32
        + *pPred.offset(-1 + kiStride as isize) as u32
        + *pPred.offset(-1 + kiStride2 as isize) as u32
        + *pPred.offset(-1 + kiStride3 as isize) as u32
        + *pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(-kiStride as isize + 1) as u32
        + *pPred.offset(-kiStride as isize + 2) as u32
        + *pPred.offset(-kiStride as isize + 3) as u32
        + 4;
    let kuiMean = (sum >> 3) as u8;
    let kuiMean32 = 0x01010101u32.wrapping_mul(kuiMean as u32);

    (pPred as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiMean32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDcLeft_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let sum = *pPred.offset(-1) as u32
        + *pPred.offset(-1 + kiStride as isize) as u32
        + *pPred.offset(-1 + kiStride2 as isize) as u32
        + *pPred.offset(-1 + kiStride3 as isize) as u32
        + 2;
    let kuiMean = (sum >> 2) as u8;
    let kuiMean32 = 0x01010101u32.wrapping_mul(kuiMean as u32);

    (pPred as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiMean32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDcTop_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let sum = *pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(-kiStride as isize + 1) as u32
        + *pPred.offset(-kiStride as isize + 2) as u32
        + *pPred.offset(-kiStride as isize + 3) as u32
        + 2;
    let kuiMean = (sum >> 2) as u8;
    let kuiMean32 = 0x01010101u32.wrapping_mul(kuiMean as u32);

    (pPred as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiMean32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDcNA_c(pPred: *mut u8, kiStride: i32) {
    let kuiDC32 = 0x80808080u32;

    (pPred as *mut u32).write_unaligned(kuiDC32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiDC32);
    (pPred.offset((kiStride << 1) as isize) as *mut u32).write_unaligned(kuiDC32);
    (pPred.offset(((kiStride << 1) + kiStride) as isize) as *mut u32).write_unaligned(kuiDC32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDDL_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptop = pPred.offset(-kiStride as isize);
    let kuiT0 = *ptop as u32;
    let kuiT1 = *ptop.offset(1) as u32;
    let kuiT2 = *ptop.offset(2) as u32;
    let kuiT3 = *ptop.offset(3) as u32;
    let kuiT4 = *ptop.offset(4) as u32;
    let kuiT5 = *ptop.offset(5) as u32;
    let kuiT6 = *ptop.offset(6) as u32;
    let kuiT7 = *ptop.offset(7) as u32;

    let kuiDDL0 = ((2 + kuiT0 + kuiT2 + (kuiT1 << 1)) >> 2) as u8;
    let kuiDDL1 = ((2 + kuiT1 + kuiT3 + (kuiT2 << 1)) >> 2) as u8;
    let kuiDDL2 = ((2 + kuiT2 + kuiT4 + (kuiT3 << 1)) >> 2) as u8;
    let kuiDDL3 = ((2 + kuiT3 + kuiT5 + (kuiT4 << 1)) >> 2) as u8;
    let kuiDDL4 = ((2 + kuiT4 + kuiT6 + (kuiT5 << 1)) >> 2) as u8;
    let kuiDDL5 = ((2 + kuiT5 + kuiT7 + (kuiT6 << 1)) >> 2) as u8;
    let kuiDDL6 = ((2 + kuiT6 + kuiT7 + (kuiT7 << 1)) >> 2) as u8;

    let kuiList: [u8; 8] = [
        kuiDDL0, kuiDDL1, kuiDDL2, kuiDDL3, kuiDDL4, kuiDDL5, kuiDDL6, 0,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(3) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredDDLTop_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptop = pPred.offset(-kiStride as isize);
    let kuiT0 = *ptop as u32;
    let kuiT1 = *ptop.offset(1) as u32;
    let kuiT2 = *ptop.offset(2) as u32;
    let kuiT3 = *ptop.offset(3) as u32;

    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiT33 = 1 + (kuiT3 << 1);

    let kuiDLT0 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiDLT1 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiDLT2 = ((kuiT23 + kuiT33) >> 2) as u8;
    let kuiDLT3 = (kuiT33 >> 1) as u8;

    let kuiList: [u8; 8] = [
        kuiDLT0, kuiDLT1, kuiDLT2, kuiDLT3, kuiDLT3, kuiDLT3, kuiDLT3, kuiDLT3,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(3) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredDDR_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptopleft = pPred.offset(-(kiStride + 1) as isize);
    let pleft = pPred.offset(-1);

    let kuiLT = *ptopleft as u32;
    let kuiL0 = *pleft as u32;
    let kuiL1 = *pleft.offset(kiStride as isize) as u32;
    let kuiL2 = *pleft.offset(kiStride2 as isize) as u32;
    let kuiL3 = *pleft.offset(kiStride3 as isize) as u32;

    let kuiT0 = *ptopleft.offset(1) as u32;
    let kuiT1 = *ptopleft.offset(2) as u32;
    let kuiT2 = *ptopleft.offset(3) as u32;
    let kuiT3 = *ptopleft.offset(4) as u32;

    let kuiTL0 = 1 + kuiLT + kuiL0;
    let kuiLT0 = 1 + kuiLT + kuiT0;
    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiL01 = 1 + kuiL0 + kuiL1;
    let kuiL12 = 1 + kuiL1 + kuiL2;
    let kuiL23 = 1 + kuiL2 + kuiL3;

    let kuiDDR0 = ((kuiTL0 + kuiLT0) >> 2) as u8;
    let kuiDDR1 = ((kuiLT0 + kuiT01) >> 2) as u8;
    let kuiDDR2 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiDDR3 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiDDR4 = ((kuiTL0 + kuiL01) >> 2) as u8;
    let kuiDDR5 = ((kuiL01 + kuiL12) >> 2) as u8;
    let kuiDDR6 = ((kuiL12 + kuiL23) >> 2) as u8;

    let kuiList: [u8; 8] = [
        kuiDDR6, kuiDDR5, kuiDDR4, kuiDDR0, kuiDDR1, kuiDDR2, kuiDDR3, 0,
    ];

    (pPred as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(3) as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredVL_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptopleft = pPred.offset(-(kiStride + 1) as isize);

    let kuiT0 = *ptopleft.offset(1) as u32;
    let kuiT1 = *ptopleft.offset(2) as u32;
    let kuiT2 = *ptopleft.offset(3) as u32;
    let kuiT3 = *ptopleft.offset(4) as u32;
    let kuiT4 = *ptopleft.offset(5) as u32;
    let kuiT5 = *ptopleft.offset(6) as u32;
    let kuiT6 = *ptopleft.offset(7) as u32;

    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiT34 = 1 + kuiT3 + kuiT4;
    let kuiT45 = 1 + kuiT4 + kuiT5;
    let kuiT56 = 1 + kuiT5 + kuiT6;

    let kuiVL0 = (kuiT01 >> 1) as u8;
    let kuiVL1 = (kuiT12 >> 1) as u8;
    let kuiVL2 = (kuiT23 >> 1) as u8;
    let kuiVL3 = (kuiT34 >> 1) as u8;
    let kuiVL4 = (kuiT45 >> 1) as u8;
    let kuiVL5 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiVL6 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiVL7 = ((kuiT23 + kuiT34) >> 2) as u8;
    let kuiVL8 = ((kuiT34 + kuiT45) >> 2) as u8;
    let kuiVL9 = ((kuiT45 + kuiT56) >> 2) as u8;

    let kuiList: [u8; 10] = [
        kuiVL0, kuiVL1, kuiVL2, kuiVL3, kuiVL4, kuiVL5, kuiVL6, kuiVL7, kuiVL8, kuiVL9,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(5) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredVLTop_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptopleft = pPred.offset(-(kiStride + 1) as isize);

    let kuiT0 = *ptopleft.offset(1) as u32;
    let kuiT1 = *ptopleft.offset(2) as u32;
    let kuiT2 = *ptopleft.offset(3) as u32;
    let kuiT3 = *ptopleft.offset(4) as u32;

    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiT33 = 1 + (kuiT3 << 1);

    let kuiVL0 = (kuiT01 >> 1) as u8;
    let kuiVL1 = (kuiT12 >> 1) as u8;
    let kuiVL2 = (kuiT23 >> 1) as u8;
    let kuiVL3 = (kuiT33 >> 1) as u8;
    let kuiVL4 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiVL5 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiVL6 = ((kuiT23 + kuiT33) >> 2) as u8;
    let kuiVL7 = kuiVL3;

    let kuiList: [u8; 10] = [
        kuiVL0, kuiVL1, kuiVL2, kuiVL3, kuiVL3, kuiVL4, kuiVL5, kuiVL6, kuiVL7, kuiVL7,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(5) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredVR_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;

    let kuiLT = *pPred.offset(-kiStride as isize - 1) as u32;
    let kuiL0 = *pPred.offset(-1) as u32;
    let kuiL1 = *pPred.offset(kiStride as isize - 1) as u32;
    let kuiL2 = *pPred.offset(kiStride2 as isize - 1) as u32;

    let kuiT0 = *pPred.offset(-kiStride as isize) as u32;
    let kuiT1 = *pPred.offset(1 - kiStride as isize) as u32;
    let kuiT2 = *pPred.offset(2 - kiStride as isize) as u32;
    let kuiT3 = *pPred.offset(3 - kiStride as isize) as u32;

    let kuiVR0 = ((1 + kuiLT + kuiT0) >> 1) as u8;
    let kuiVR1 = ((1 + kuiT0 + kuiT1) >> 1) as u8;
    let kuiVR2 = ((1 + kuiT1 + kuiT2) >> 1) as u8;
    let kuiVR3 = ((1 + kuiT2 + kuiT3) >> 1) as u8;
    let kuiVR4 = ((2 + kuiL0 + (kuiLT << 1) + kuiT0) >> 2) as u8;
    let kuiVR5 = ((2 + kuiLT + (kuiT0 << 1) + kuiT1) >> 2) as u8;
    let kuiVR6 = ((2 + kuiT0 + (kuiT1 << 1) + kuiT2) >> 2) as u8;
    let kuiVR7 = ((2 + kuiT1 + (kuiT2 << 1) + kuiT3) >> 2) as u8;
    let kuiVR8 = ((2 + kuiLT + (kuiL0 << 1) + kuiL1) >> 2) as u8;
    let kuiVR9 = ((2 + kuiL0 + (kuiL1 << 1) + kuiL2) >> 2) as u8;

    let kuiList: [u8; 10] = [
        kuiVR8, kuiVR0, kuiVR1, kuiVR2, kuiVR3, kuiVR9, kuiVR4, kuiVR5, kuiVR6, kuiVR7,
    ];

    (pPred as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(5) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredHU_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;

    let kuiL0 = *pPred.offset(-1) as u32;
    let kuiL1 = *pPred.offset(kiStride as isize - 1) as u32;
    let kuiL2 = *pPred.offset(kiStride2 as isize - 1) as u32;
    let kuiL3 = *pPred.offset(kiStride3 as isize - 1) as u32;

    let kuiL01 = 1 + kuiL0 + kuiL1;
    let kuiL12 = 1 + kuiL1 + kuiL2;
    let kuiL23 = 1 + kuiL2 + kuiL3;

    let kuiHU0 = (kuiL01 >> 1) as u8;
    let kuiHU1 = ((kuiL01 + kuiL12) >> 2) as u8;
    let kuiHU2 = (kuiL12 >> 1) as u8;
    let kuiHU3 = ((kuiL12 + kuiL23) >> 2) as u8;
    let kuiHU4 = (kuiL23 >> 1) as u8;
    let kuiHU5 = ((1 + kuiL23 + (kuiL3 << 1)) >> 2) as u8;
    let kuiL3_u8 = kuiL3 as u8;

    let kuiList: [u8; 10] = [
        kuiHU0, kuiHU1, kuiHU2, kuiHU3, kuiHU4, kuiHU5, kuiL3_u8, kuiL3_u8, kuiL3_u8, kuiL3_u8,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(4) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredHD_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;

    let kuiLT = *pPred.offset(-(kiStride + 1) as isize) as u32;
    let kuiL0 = *pPred.offset(-1) as u32;
    let kuiL1 = *pPred.offset(-1 + kiStride as isize) as u32;
    let kuiL2 = *pPred.offset(-1 + kiStride2 as isize) as u32;
    let kuiL3 = *pPred.offset(-1 + kiStride3 as isize) as u32;

    let kuiT0 = *pPred.offset(-kiStride as isize) as u32;
    let kuiT1 = *pPred.offset(-kiStride as isize + 1) as u32;
    let kuiT2 = *pPred.offset(-kiStride as isize + 2) as u32;

    let kuiTL0 = 1 + kuiLT + kuiL0;
    let kuiLT0 = 1 + kuiLT + kuiT0;
    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiL01 = 1 + kuiL0 + kuiL1;
    let kuiL12 = 1 + kuiL1 + kuiL2;
    let kuiL23 = 1 + kuiL2 + kuiL3;

    let kuiHD0 = (kuiTL0 >> 1) as u8;
    let kuiHD1 = ((kuiTL0 + kuiLT0) >> 2) as u8;
    let kuiHD2 = ((kuiLT0 + kuiT01) >> 2) as u8;
    let kuiHD3 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiHD4 = (kuiL01 >> 1) as u8;
    let kuiHD5 = ((kuiTL0 + kuiL01) >> 2) as u8;
    let kuiHD6 = (kuiL12 >> 1) as u8;
    let kuiHD7 = ((kuiL01 + kuiL12) >> 2) as u8;
    let kuiHD8 = (kuiL23 >> 1) as u8;
    let kuiHD9 = ((kuiL12 + kuiL23) >> 2) as u8;

    let kuiList: [u8; 10] = [
        kuiHD8, kuiHD9, kuiHD6, kuiHD7, kuiHD4, kuiHD5, kuiHD0, kuiHD1, kuiHD2, kuiHD3,
    ];

    (pPred as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(4) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
}

// ============================================================================
// Intra 8x8 Luma Prediction Functions (High Profile)
// ============================================================================

pub unsafe extern "C" fn WelsI8x8LumaPredV_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    let mut uiTop: u64 = 0;
    for i in (0..8).rev() {
        uiTop = (uiTop << 8) | (uiPixelFilterT[i] as u64);
    }

    for i in 0..8 {
        (pPred.offset(kiStride as isize * i as isize) as *mut u64).write_unaligned(uiTop);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredH_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8 {
        let uiLeft = 0x0101010101010101u64.wrapping_mul(uiPixelFilterL[i] as u64);
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(uiLeft);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDc_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    let mut uiTotal: u32 = 0;
    for i in 0..8 {
        uiTotal += uiPixelFilterL[i] as u32;
        uiTotal += uiPixelFilterT[i] as u32;
    }

    let kuiMean = ((uiTotal + 8) >> 4) as u8;
    let kuiMean64 = 0x0101010101010101u64.wrapping_mul(kuiMean as u64);

    for i in 0..8 {
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(kuiMean64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDcLeft_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    let mut uiTotal: u32 = 0;
    for i in 0..8 {
        uiTotal += uiPixelFilterL[i] as u32;
    }

    let kuiMean = ((uiTotal + 4) >> 3) as u8;
    let kuiMean64 = 0x0101010101010101u64.wrapping_mul(kuiMean as u64);

    for i in 0..8 {
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(kuiMean64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDcTop_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 8];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    let mut uiTotal: u32 = 0;
    for i in 0..8 {
        uiTotal += uiPixelFilterT[i] as u32;
    }

    let kuiMean = ((uiTotal + 4) >> 3) as u8;
    let kuiMean64 = 0x0101010101010101u64.wrapping_mul(kuiMean as u64);

    for i in 0..8 {
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(kuiMean64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDcNA_c(
    pPred: *mut u8,
    kiStride: i32,
    _bTLAvail: bool,
    _bTRAvail: bool,
) {
    let kuiDC64 = 0x8080808080808080u64;
    for i in 0..8 {
        (pPred.offset(kiStride as isize * i as isize) as *mut u64).write_unaligned(kuiDC64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDDL_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..15 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[15] = ((*pPred.offset(14 - kiStride as isize) as u32
        + *pPred.offset(15 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8 {
        for j in 0..8 {
            if i == 7 && j == 7 {
                *pPred.offset(j as isize + iStride[i] as isize) =
                    ((uiPixelFilterT[14] as u32 + 3 * uiPixelFilterT[15] as u32 + 2) >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[i + j] as u32
                    + ((uiPixelFilterT[i + j + 1] as u32) << 1)
                    + uiPixelFilterT[i + j + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDDLTop_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[7] = ((*pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    let fill_val = *pPred.offset(7 - kiStride as isize);
    for i in 8..16 {
        uiPixelFilterT[i] = fill_val;
    }

    for i in 0..8 {
        for j in 0..8 {
            if i == 7 && j == 7 {
                *pPred.offset(j as isize + iStride[i] as isize) =
                    ((uiPixelFilterT[14] as u32 + 3 * uiPixelFilterT[15] as u32 + 2) >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[i + j] as u32
                    + ((uiPixelFilterT[i + j + 1] as u32) << 1)
                    + uiPixelFilterT[i + j + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDDR_c(
    pPred: *mut u8,
    kiStride: i32,
    _bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let uiPixelFilterTL = ((*pPred.offset(-1) as u32
        + ((*pPred.offset(-1 - kiStride as isize) as u32) << 1)
        + *pPred.offset(-kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-1) as u32) << 1)
        + *pPred.offset(-1 + iStride[1] as isize) as u32
        + 2)
        >> 2) as u8;
    uiPixelFilterT[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-kiStride as isize) as u32) << 1)
        + *pPred.offset(1 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    for i in 0..8usize {
        for j in 0..(i.saturating_sub(1)) {
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterL[i - j - 2] as u32
                + ((uiPixelFilterL[i - j - 1] as u32) << 1)
                + uiPixelFilterL[i - j] as u32
                + 2)
                >> 2) as u8;
        }

        if i >= 1 {
            let j = i - 1;
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterTL as u32
                + ((uiPixelFilterL[0] as u32) << 1)
                + uiPixelFilterL[1] as u32
                + 2)
                >> 2) as u8;
        }

        let j = i;
        *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[0] as u32
            + ((uiPixelFilterTL as u32) << 1)
            + uiPixelFilterL[0] as u32
            + 2)
            >> 2) as u8;

        if i < 7 {
            let j = i + 1;
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterTL as u32
                + ((uiPixelFilterT[0] as u32) << 1)
                + uiPixelFilterT[1] as u32
                + 2)
                >> 2) as u8;
        }

        for j in (i + 2)..8 {
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j - i - 2] as u32
                + ((uiPixelFilterT[j - i - 1] as u32) << 1)
                + uiPixelFilterT[j - i] as u32
                + 2)
                >> 2) as u8;
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredVL_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..15 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[15] = ((*pPred.offset(14 - kiStride as isize) as u32
        + *pPred.offset(15 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8 {
        if (i & 0x01) == 0 {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + uiPixelFilterT[j + (i >> 1) + 1] as u32
                    + 1)
                    >> 1) as u8;
            }
        } else {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + ((uiPixelFilterT[j + (i >> 1) + 1] as u32) << 1)
                    + uiPixelFilterT[j + (i >> 1) + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredVLTop_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[7] = ((*pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    let fill_val = *pPred.offset(7 - kiStride as isize);
    for i in 8..16 {
        uiPixelFilterT[i] = fill_val;
    }

    for i in 0..8 {
        if (i & 0x01) == 0 {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + uiPixelFilterT[j + (i >> 1) + 1] as u32
                    + 1)
                    >> 1) as u8;
            }
        } else {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + ((uiPixelFilterT[j + (i >> 1) + 1] as u32) << 1)
                    + uiPixelFilterT[j + (i >> 1) + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredVR_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let uiPixelFilterTL = ((*pPred.offset(-1) as u32
        + ((*pPred.offset(-1 - kiStride as isize) as u32) << 1)
        + *pPred.offset(-kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-1) as u32) << 1)
        + *pPred.offset(-1 + iStride[1] as isize) as u32
        + 2)
        >> 2) as u8;
    uiPixelFilterT[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-kiStride as isize) as u32) << 1)
        + *pPred.offset(1 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    for i in 0..8i32 {
        for j in 0..8i32 {
            let izVR = (j << 1) - i;
            let izVRDiv = j - (i >> 1);
            if izVR >= 0 {
                if (izVR & 0x01) == 0 {
                    if izVRDiv > 0 {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterT[(izVRDiv - 1) as usize] as u32
                                + uiPixelFilterT[izVRDiv as usize] as u32
                                + 1)
                                >> 1) as u8;
                    } else {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterTL as u32 + uiPixelFilterT[0] as u32 + 1) >> 1) as u8;
                    }
                } else if izVRDiv > 1 {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterT[(izVRDiv - 2) as usize] as u32
                            + ((uiPixelFilterT[(izVRDiv - 1) as usize] as u32) << 1)
                            + uiPixelFilterT[izVRDiv as usize] as u32
                            + 2)
                            >> 2) as u8;
                } else {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterTL as u32
                            + ((uiPixelFilterT[0] as u32) << 1)
                            + uiPixelFilterT[1] as u32
                            + 2)
                            >> 2) as u8;
                }
            } else if izVR == -1 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterL[0]
                    as u32
                    + ((uiPixelFilterTL as u32) << 1)
                    + uiPixelFilterT[0] as u32
                    + 2)
                    >> 2) as u8;
            } else if izVR < -2 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) =
                    ((uiPixelFilterL[(-izVR - 1) as usize] as u32
                        + ((uiPixelFilterL[(-izVR - 2) as usize] as u32) << 1)
                        + uiPixelFilterL[(-izVR - 3) as usize] as u32
                        + 2)
                        >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterL[1]
                    as u32
                    + ((uiPixelFilterL[0] as u32) << 1)
                    + uiPixelFilterTL as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredHU_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8i32 {
        for j in 0..8i32 {
            let izHU = j + (i << 1);
            if izHU < 13 {
                if (izHU & 0x01) == 0 {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterL[(izHU >> 1) as usize] as u32
                            + uiPixelFilterL[(1 + (izHU >> 1)) as usize] as u32
                            + 1)
                            >> 1) as u8;
                } else {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterL[(izHU >> 1) as usize] as u32
                            + ((uiPixelFilterL[(1 + (izHU >> 1)) as usize] as u32) << 1)
                            + uiPixelFilterL[(2 + (izHU >> 1)) as usize] as u32
                            + 2)
                            >> 2) as u8;
                }
            } else if izHU == 13 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) =
                    ((uiPixelFilterL[6] as u32 + 3 * uiPixelFilterL[7] as u32 + 2) >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = uiPixelFilterL[7];
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredHD_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let uiPixelFilterTL = ((*pPred.offset(-1) as u32
        + ((*pPred.offset(-1 - kiStride as isize) as u32) << 1)
        + *pPred.offset(-kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-1) as u32) << 1)
        + *pPred.offset(-1 + iStride[1] as isize) as u32
        + 2)
        >> 2) as u8;
    uiPixelFilterT[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-kiStride as isize) as u32) << 1)
        + *pPred.offset(1 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    for i in 0..8i32 {
        for j in 0..8i32 {
            let izHD = (i << 1) - j;
            let izHDDiv = i - (j >> 1);
            if izHD >= 0 {
                if (izHD & 0x01) == 0 {
                    if izHDDiv == 0 {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterTL as u32 + uiPixelFilterL[0] as u32 + 1) >> 1) as u8;
                    } else {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterL[(izHDDiv - 1) as usize] as u32
                                + uiPixelFilterL[izHDDiv as usize] as u32
                                + 1)
                                >> 1) as u8;
                    }
                } else if izHDDiv == 1 {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterTL as u32
                            + ((uiPixelFilterL[0] as u32) << 1)
                            + uiPixelFilterL[1] as u32
                            + 2)
                            >> 2) as u8;
                } else {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterL[(izHDDiv - 2) as usize] as u32
                            + ((uiPixelFilterL[(izHDDiv - 1) as usize] as u32) << 1)
                            + uiPixelFilterL[izHDDiv as usize] as u32
                            + 2)
                            >> 2) as u8;
                }
            } else if izHD == -1 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterL[0]
                    as u32
                    + ((uiPixelFilterTL as u32) << 1)
                    + uiPixelFilterT[0] as u32
                    + 2)
                    >> 2) as u8;
            } else if izHD < -2 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) =
                    ((uiPixelFilterT[(-izHD - 1) as usize] as u32
                        + ((uiPixelFilterT[(-izHD - 2) as usize] as u32) << 1)
                        + uiPixelFilterT[(-izHD - 3) as usize] as u32
                        + 2)
                        >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterT[1]
                    as u32
                    + ((uiPixelFilterT[0] as u32) << 1)
                    + uiPixelFilterTL as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

// ============================================================================
// Intra 8x8 Chroma Prediction Functions
// ============================================================================

pub unsafe extern "C" fn WelsIChromaPredV_c(pPred: *mut u8, kiStride: i32) {
    let kuiVal64 = (pPred.offset(-kiStride as isize) as *const u64).read_unaligned();
    let kiStride2 = kiStride << 1;
    let kiStride4 = kiStride2 << 1;

    (pPred as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(kiStride as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(kiStride2 as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset((kiStride2 + kiStride) as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(kiStride4 as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset((kiStride4 + kiStride) as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset((kiStride4 + kiStride2) as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(((kiStride << 3) - kiStride) as isize) as *mut u64).write_unaligned(kuiVal64);
}

pub unsafe extern "C" fn WelsIChromaPredH_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 3) - kiStride;
    for _ in 0..8 {
        let kuiVal8 = *pPred.offset(iTmp as isize - 1);
        let kuiVal64 = 0x0101010101010101u64.wrapping_mul(kuiVal8 as u64);
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiVal64);
        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsIChromaPredPlane_c(pPred: *mut u8, kiStride: i32) {
    let mut H: i32 = 0;
    let mut V: i32 = 0;
    let pTop = pPred.offset(-kiStride as isize);
    let pLeft = pPred.offset(-1);

    for i in 0..4i32 {
        H += (i + 1)
            * (*pTop.offset(4 + i as isize) as i32 - *pTop.offset(2 - i as isize) as i32);
        V += (i + 1)
            * (*pLeft.offset((4 + i as isize) * kiStride as isize) as i32
                - *pLeft.offset((2 - i as isize) * kiStride as isize) as i32);
    }

    let a = (*pLeft.offset(7 * kiStride as isize) as i32 + *pTop.offset(7) as i32) << 4;
    let b = (17 * H + 16) >> 5;
    let c = (17 * V + 16) >> 5;

    let mut row_ptr = pPred;
    for i in 0..8i32 {
        for j in 0..8i32 {
            let iTmp = (a + b * (j - 3) + c * (i - 3) + 16) >> 5;
            *row_ptr.offset(j as isize) = WelsClip1(iTmp);
        }
        row_ptr = row_ptr.offset(kiStride as isize);
    }
}

pub unsafe extern "C" fn WelsIChromaPredDc_c(pPred: *mut u8, kiStride: i32) {
    let kiL1 = kiStride - 1;
    let kiL2 = kiL1 + kiStride;
    let kiL3 = kiL2 + kiStride;
    let kiL4 = kiL3 + kiStride;
    let kiL5 = kiL4 + kiStride;
    let kiL6 = kiL5 + kiStride;
    let kiL7 = kiL6 + kiStride;

    let kuiM1 = ((*pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(1 - kiStride as isize) as u32
        + *pPred.offset(2 - kiStride as isize) as u32
        + *pPred.offset(3 - kiStride as isize) as u32
        + *pPred.offset(-1) as u32
        + *pPred.offset(kiL1 as isize) as u32
        + *pPred.offset(kiL2 as isize) as u32
        + *pPred.offset(kiL3 as isize) as u32
        + 4)
        >> 3) as u8;

    let kuiSum2 = *pPred.offset(4 - kiStride as isize) as u32
        + *pPred.offset(5 - kiStride as isize) as u32
        + *pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32;

    let kuiSum3 = *pPred.offset(kiL4 as isize) as u32
        + *pPred.offset(kiL5 as isize) as u32
        + *pPred.offset(kiL6 as isize) as u32
        + *pPred.offset(kiL7 as isize) as u32;

    let kuiM2 = ((kuiSum2 + 2) >> 2) as u8;
    let kuiM3 = ((kuiSum3 + 2) >> 2) as u8;
    let kuiM4 = ((kuiSum2 + kuiSum3 + 4) >> 3) as u8;

    let kuiMUP: [u8; 8] = [kuiM1, kuiM1, kuiM1, kuiM1, kuiM2, kuiM2, kuiM2, kuiM2];
    let kuiMDown: [u8; 8] = [kuiM3, kuiM3, kuiM3, kuiM3, kuiM4, kuiM4, kuiM4, kuiM4];

    let kuiUP64 = (kuiMUP.as_ptr() as *const u64).read_unaligned();
    let kuiDN64 = (kuiMDown.as_ptr() as *const u64).read_unaligned();

    (pPred as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL1 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL2 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL3 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL4 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL5 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL6 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL7 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
}

pub unsafe extern "C" fn WelsIChromaPredDcLeft_c(pPred: *mut u8, kiStride: i32) {
    let kiL1 = -1 + kiStride;
    let kiL2 = kiL1 + kiStride;
    let kiL3 = kiL2 + kiStride;
    let kiL4 = kiL3 + kiStride;
    let kiL5 = kiL4 + kiStride;
    let kiL6 = kiL5 + kiStride;
    let kiL7 = kiL6 + kiStride;

    let kuiMUP = ((*pPred.offset(-1) as u32
        + *pPred.offset(kiL1 as isize) as u32
        + *pPred.offset(kiL2 as isize) as u32
        + *pPred.offset(kiL3 as isize) as u32
        + 2)
        >> 2) as u8;

    let kuiMDown = ((*pPred.offset(kiL4 as isize) as u32
        + *pPred.offset(kiL5 as isize) as u32
        + *pPred.offset(kiL6 as isize) as u32
        + *pPred.offset(kiL7 as isize) as u32
        + 2)
        >> 2) as u8;

    let kuiUP64 = 0x0101010101010101u64.wrapping_mul(kuiMUP as u64);
    let kuiDN64 = 0x0101010101010101u64.wrapping_mul(kuiMDown as u64);

    (pPred as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL1 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL2 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL3 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL4 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL5 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL6 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL7 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
}

pub unsafe extern "C" fn WelsIChromaPredDcTop_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 3) - kiStride;
    let kuiM1 = ((*pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(1 - kiStride as isize) as u32
        + *pPred.offset(2 - kiStride as isize) as u32
        + *pPred.offset(3 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;
    let kuiM2 = ((*pPred.offset(4 - kiStride as isize) as u32
        + *pPred.offset(5 - kiStride as isize) as u32
        + *pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let kuiM: [u8; 8] = [kuiM1, kuiM1, kuiM1, kuiM1, kuiM2, kuiM2, kuiM2, kuiM2];
    let kuiM64 = (kuiM.as_ptr() as *const u64).read_unaligned();

    for _ in 0..8 {
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiM64);
        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsIChromaPredDcNA_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 3) - kiStride;
    let kuiDC64 = 0x8080808080808080u64;

    for _ in 0..8 {
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiDC64);
        iTmp -= kiStride;
    }
}

// ============================================================================
// Intra 16x16 Luma Prediction Functions
// ============================================================================

pub unsafe extern "C" fn WelsI16x16LumaPredV_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;
    let kuiTop1 = (pPred.offset(-kiStride as isize) as *const u64).read_unaligned();
    let kuiTop2 = (pPred.offset(-kiStride as isize + 8) as *const u64).read_unaligned();

    for _ in 0..16 {
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiTop1);
        (pPred.offset(iTmp as isize + 8) as *mut u64).write_unaligned(kuiTop2);
        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredH_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;

    for _ in 0..16 {
        let kuiVal8 = *pPred.offset(iTmp as isize - 1);
        let kuiVal64 = 0x0101010101010101u64.wrapping_mul(kuiVal8 as u64);

        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiVal64);
        (pPred.offset(iTmp as isize + 8) as *mut u64).write_unaligned(kuiVal64);

        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredPlane_c(pPred: *mut u8, kiStride: i32) {
    let mut H: i32 = 0;
    let mut V: i32 = 0;
    let pTop = pPred.offset(-kiStride as isize);
    let pLeft = pPred.offset(-1);

    for i in 0..8i32 {
        H += (i + 1)
            * (*pTop.offset(8 + i as isize) as i32 - *pTop.offset(6 - i as isize) as i32);
        V += (i + 1)
            * (*pLeft.offset((8 + i as isize) * kiStride as isize) as i32
                - *pLeft.offset((6 - i as isize) * kiStride as isize) as i32);
    }

    let a = (*pLeft.offset(15 * kiStride as isize) as i32 + *pTop.offset(15) as i32) << 4;
    let b = (5 * H + 32) >> 6;
    let c = (5 * V + 32) >> 6;

    let mut row_ptr = pPred;
    for i in 0..16i32 {
        for j in 0..16i32 {
            let iTmp = (a + b * (j - 7) + c * (i - 7) + 16) >> 5;
            *row_ptr.offset(j as isize) = WelsClip1(iTmp);
        }
        row_ptr = row_ptr.offset(kiStride as isize);
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDc_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;
    let mut iSum: i32 = 0;

    for i in 0..16 {
        iSum += *pPred.offset(-1 + iTmp as isize) as i32
            + *pPred.offset(-kiStride as isize + (15 - i) as isize) as i32;
        iTmp -= kiStride;
    }

    let uiMean = ((16 + iSum) >> 5) as u8;
    let uiMean64 = 0x0101010101010101u64.wrapping_mul(uiMean as u64);

    let mut out_offset = (kiStride << 4) - kiStride;
    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(uiMean64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(uiMean64);
        out_offset -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDcTop_c(pPred: *mut u8, kiStride: i32) {
    let mut iSum: i32 = 0;
    for i in 0..16 {
        iSum += *pPred.offset(-kiStride as isize + i as isize) as i32;
    }

    let uiMean = ((8 + iSum) >> 4) as u8;
    let uiMean64 = 0x0101010101010101u64.wrapping_mul(uiMean as u64);

    let mut out_offset = (kiStride << 4) - kiStride;
    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(uiMean64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(uiMean64);
        out_offset -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDcLeft_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;
    let mut iSum: i32 = 0;

    for _ in 0..16 {
        iSum += *pPred.offset(-1 + iTmp as isize) as i32;
        iTmp -= kiStride;
    }

    let uiMean = ((8 + iSum) >> 4) as u8;
    let uiMean64 = 0x0101010101010101u64.wrapping_mul(uiMean as u64);

    let mut out_offset = (kiStride << 4) - kiStride;
    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(uiMean64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(uiMean64);
        out_offset -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDcNA_c(pPred: *mut u8, kiStride: i32) {
    let kuiDC64 = 0x8080808080808080u64;
    let mut out_offset = (kiStride << 4) - kiStride;

    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(kuiDC64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(kuiDC64);
        out_offset -= kiStride;
    }
}

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

    #[test]
    fn test_i4x4_pred_v_h_dc() {
        let mut buf = [0u8; 64];
        let stride = 8;
        // pPred is at offset 16 (row 2, col 1)
        let pred_offset = 17;

        unsafe {
            let pPred = buf.as_mut_ptr().add(pred_offset);

            // Set top samples at pPred - stride
            *pPred.offset(-stride) = 10;
            *pPred.offset(-stride + 1) = 20;
            *pPred.offset(-stride + 2) = 30;
            *pPred.offset(-stride + 3) = 40;

            WelsI4x4LumaPredV_c(pPred, stride as i32);
            assert_eq!(*pPred.offset(0), 10);
            assert_eq!(*pPred.offset(1), 20);
            assert_eq!(*pPred.offset(stride), 10);
            assert_eq!(*pPred.offset(stride + 3), 40);

            // Set left samples
            *pPred.offset(-1) = 5;
            *pPred.offset(stride - 1) = 15;
            *pPred.offset(2 * stride - 1) = 25;
            *pPred.offset(3 * stride - 1) = 35;

            WelsI4x4LumaPredH_c(pPred, stride as i32);
            assert_eq!(*pPred.offset(0), 5);
            assert_eq!(*pPred.offset(3), 5);
            assert_eq!(*pPred.offset(stride), 15);
            assert_eq!(*pPred.offset(3 * stride + 3), 35);
        }
    }
}
