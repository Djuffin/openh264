//! Port of `codec/encoder/core/src/get_intra_predictor.cpp` — the encoder's intra
//! prediction sample generators and the `WelsInitIntraPredFuncs` table filler.
//!
//! These are **not** the decoder's predictors in `decoder/get_intra_predictor.rs`:
//! the encoder's take a separate cursor into the reconstructed frame and write into
//! a packed prediction buffer (stride 4 for I4x4, 8 for chroma, 16 for I16x16),
//! while the decoder's predict in place through a single plane cursor.
//!
//! Only the `_c` scalar variants exist here. The SIMD variants in the C++ are all
//! behind `uiCpuFlag` tests that do not fire on any target this port builds for.
//!
//! # Three same-named families, and they must never be unified
//!
//! The C++ `WelsI4x4LumaPredV_c` and its siblings are ported three times, into three
//! modules with three different signatures and three different destinations:
//!
//! | module | signature in this port | destination |
//! |---|---|---|
//! | `decoder/get_intra_predictor.rs` | `(&mut PlaneCursorMut)` | **in place**, strided |
//! | `common/intra_pred_common.rs` | `(&mut [u8; 256], top or ref)` | packed, 16x16 only |
//! | **this module** | `(&mut [u8; N], &RecCursor)` | **packed** candidate buffer |
//!
//! Same C++ names, different functions: never unify them, never delete one for the
//! other. The two 16x16 modes this module *does* share with `intra_pred_common`
//! (`V` and `H`) it imports rather than redefines — those two really are the same
//! function, and the table below installs the imported ones.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use crate::common::intra_pred_common::{i16x16_luma_pred_h, i16x16_luma_pred_v};
use crate::safe::plane::{PlaneCursor, RefSamples};
use crate::encoder::rec_view::RecCursor;
use crate::encoder::svc_base_layer_md::{
    C_PRED_DC, C_PRED_DC_128, C_PRED_DC_L, C_PRED_DC_T, C_PRED_H, C_PRED_P, C_PRED_V, I4_PRED_DC,
    I4_PRED_DC_128, I4_PRED_DC_L, I4_PRED_DC_T, I4_PRED_DDL, I4_PRED_DDL_TOP, I4_PRED_DDR,
    I4_PRED_H, I4_PRED_HD, I4_PRED_HU, I4_PRED_V, I4_PRED_VL, I4_PRED_VL_TOP, I4_PRED_VR,
};
use crate::encoder::svc_mode_decision::{
    I16_PRED_DC, I16_PRED_DC_128, I16_PRED_DC_L, I16_PRED_DC_T, I16_PRED_H, I16_PRED_P, I16_PRED_V,
};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

#[inline(always)]
fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        if -iX < 0 { 255 } else { 0 }
    } else {
        iX as u8
    }
}

// ============================================================================
// Safe kernels
// ============================================================================
//
// Every predictor in this module has **two surfaces with different rules**, and the
// signatures below say so:
//
//   * the **destination** is a *packed* candidate buffer — 16 bytes at an implicit
//     stride of 4 (I4x4), 64 at 8 (chroma), 256 at 16 (I16x16). It is one of the
//     mode-decision ping-pong halves (`pMemPredBlk4`, `pMemPredChroma`,
//     `pMemPredMb`; `svc_base_layer_md.rs:437`, `:734`, `svc_mode_decision.rs:1167`),
//     never a picture plane, so it is a fixed-size array and the reference
//     cursor's stride says nothing about it;
//   * the **reference** is the reconstructed plane, read at `x = -1` and
//     `y = -1` around the block. Those reads are in-allocation because a picture
//     plane is `PADDING_LENGTH`-padded on every side, and they are *correct*
//     because mode decision only offers a mode whose neighbours exist — the
//     availability tables `g_kiIntra16AvaliMode` / `g_kiIntra4AvailMode` /
//     `g_kiIntraChromaAvailMode` pick the candidate list from
//     `uiNeighborIntra`, which is why `…DcTop`, `…DcLeft`, `…DDLTop` and
//     `…VLTop` exist at all.
//
// **Per-kernel reference shapes, not one shared shape.** A predictor
// that reads only the row above takes `&[u8; N]` — it *cannot* touch the left
// column, which is the whole reason mode decision may offer it when the left
// neighbour is missing. A predictor that reads the left column takes a sample
// reader instead — `&impl RefSamples`, which a [`PlaneCursor`] and the `RecCursor`
// the shims pass both satisfy — and reads only that kernel's reach. The reach table
// is `REACH_*` below, and `ref_span` is the only place it becomes a byte span.

/// What one predictor reads of the reconstructed plane, relative to the block's
/// own `(0, 0)` — i.e. relative to the cursor's anchor.
///
/// This is the contract in data form: one `Reach` per kernel, never a union over a
/// family. `WelsI4x4LumaPredDcTop_c` and `WelsI4x4LumaPredDDR_c` are both 4x4 luma
/// predictors and their reaches have nothing in common.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reach {
    /// Samples of the row above, at `x` in `0..top`. Zero means the row above is
    /// not read at all.
    pub top: usize,
    /// Samples of the column to the left, at `y` in `0..left`. Zero means the
    /// column is not read at all.
    pub left: usize,
    /// Reads the corner sample at `(-1, -1)`.
    pub corner: bool,
}

impl Reach {
    const fn new(top: usize, left: usize, corner: bool) -> Self {
        Self { top, left, corner }
    }
}

/// The byte span a [`Reach`] needs at `stride`, as `(len, center)`: one row-major
/// run of `len` bytes with the block's own `(0, 0)` at offset `center`, so a cursor
/// anchored there addresses every sample the reach names and nothing outside it.
///
/// The span covers **reads only** — nothing in this module writes the reference,
/// and the anchor sample itself is *not* read by any predictor — so it is the
/// tightest span a reach can be given, which `ref_span_is_tight_at_both_ends` pins
/// at both ends.
///
/// A consequence worth stating: for a reach that reads only the row above, the span
/// lies **entirely above the anchor** and `center >= len`, which no [`PlaneCursor`]
/// will accept. That is not a defect — those kernels take `&[u8; N]` rather than a
/// cursor, precisely because their whole reach is one contiguous run.
///
/// # Panics
/// Never. A reach that reads nothing (`DcNA`) produces `(0, 0)`; those kernels take
/// no reference at all.
#[inline(always)]
pub fn ref_span(stride: usize, reach: Reach) -> (usize, usize) {
    let s = stride as isize;
    // Lowest byte read, and one past the highest, as signed offsets from the anchor.
    // Seeded from the first read rather than from zero: seeding at zero would drag
    // the anchor sample itself into every span, and no predictor here reads it.
    let (mut lo, mut hi) = if reach.corner {
        (-s - 1, -s)
    } else if reach.top > 0 {
        (-s, -s + reach.top as isize)
    } else if reach.left > 0 {
        // The last left sample sits at `(left-1)*stride - 1`; one past it is
        // `(left-1)*stride`.
        (-1, (reach.left as isize - 1) * s)
    } else {
        return (0, 0);
    };
    if reach.top > 0 {
        lo = lo.min(-s);
        hi = hi.max(-s + reach.top as isize);
    }
    if reach.left > 0 {
        lo = lo.min(-1);
        hi = hi.max((reach.left as isize - 1) * s);
    }
    ((hi - lo) as usize, (-lo) as usize)
}

// --- I4x4 luma: reaches ------------------------------------------------------

/// `WelsI4x4LumaPredDcNA_c` — reads nothing. Its C++ `pRef` parameter is dead.
pub const REACH_NONE: Reach = Reach::new(0, 0, false);
/// `WelsI4x4LumaPredV_c`, `…DcTop_c`, `…DDLTop_c`, `…VLTop_c` — the block's own
/// four top samples and nothing else.
pub const REACH_I4X4_TOP: Reach = Reach::new(4, 0, false);
/// `WelsI4x4LumaPredVL_c` — **seven** top samples: three past the right edge.
pub const REACH_I4X4_TOP7: Reach = Reach::new(7, 0, false);
/// `WelsI4x4LumaPredDDL_c` — **eight** top samples: four past the right edge, which
/// is why this mode is gated on the top-right neighbour.
pub const REACH_I4X4_TOP8: Reach = Reach::new(8, 0, false);
/// `WelsI4x4LumaPredH_c` — the four samples left of the block.
pub const REACH_I4X4_LEFT: Reach = Reach::new(0, 4, false);
/// `WelsI4x4LumaPredDc_c` — four above and four left.
pub const REACH_I4X4_DC: Reach = Reach::new(4, 4, false);
/// `WelsI4x4LumaPredDDR_c` — the corner, four above and four left.
pub const REACH_I4X4_DDR: Reach = Reach::new(4, 4, true);
/// `WelsI4x4LumaPredVR_c` — the corner, four above and **three** left.
pub const REACH_I4X4_VR: Reach = Reach::new(4, 3, true);
/// `WelsI4x4LumaPredHD_c` — the corner, **three** above and four left.
pub const REACH_I4X4_HD: Reach = Reach::new(3, 4, true);

// --- chroma 8x8: reaches -----------------------------------------------------

/// `WelsIChromaPredV_c`, `WelsIChromaPredDcTop_c` — the eight samples above.
pub const REACH_CHROMA_TOP: Reach = Reach::new(8, 0, false);
/// `WelsIChromaPredH_c`, `WelsIChromaPredDcLeft_c` — the eight samples left.
pub const REACH_CHROMA_LEFT: Reach = Reach::new(0, 8, false);
/// `WelsIChromaPredDc_c` — eight above and eight left.
pub const REACH_CHROMA_DC: Reach = Reach::new(8, 8, false);
/// `WelsIChromaPredPlane_c` — eight above and eight left **plus the corner**: the
/// `pTop[2 - i]` and `pLeft[(2 - i) * stride]` arms reach `-1` at `i == 3`.
pub const REACH_CHROMA_PLANE: Reach = Reach::new(8, 8, true);

// --- I16x16 luma: reaches ----------------------------------------------------

/// `WelsI16x16LumaPredV_c` (imported), `WelsI16x16LumaPredDcTop_c` — the sixteen
/// samples above.
pub const REACH_I16X16_TOP: Reach = Reach::new(16, 0, false);
/// `WelsI16x16LumaPredH_c` (imported), `WelsI16x16LumaPredDcLeft_c` — the sixteen
/// samples left.
pub const REACH_I16X16_LEFT: Reach = Reach::new(0, 16, false);
/// `WelsI16x16LumaPredDc_c` — sixteen above and sixteen left.
pub const REACH_I16X16_DC: Reach = Reach::new(16, 16, false);
/// `WelsI16x16LumaPredPlane_c` — sixteen above and sixteen left **plus the
/// corner**, for the same reason as chroma plane (`6 - i` reaches `-1` at
/// `i == 7`).
pub const REACH_I16X16_PLANE: Reach = Reach::new(16, 16, true);

// --- the reach table, by mode ------------------------------------------------
//
// The three lookups below are the reach table as mode decision sees it. Nothing in
// `src/` calls them — each shim names its own constant, which is statically known
// and folds — but they are what makes the availability argument *checkable*
// (`reach_table_agrees_with_the_availability_tables` in this module's tests).

/// Reference reach of the I4x4 luma predictor installed at `mode`.
pub const fn reach_i4x4(mode: i8) -> Reach {
    match mode {
        I4_PRED_V | I4_PRED_DC_T | I4_PRED_DDL_TOP | I4_PRED_VL_TOP => REACH_I4X4_TOP,
        I4_PRED_VL => REACH_I4X4_TOP7,
        I4_PRED_DDL => REACH_I4X4_TOP8,
        I4_PRED_H | I4_PRED_DC_L | I4_PRED_HU => REACH_I4X4_LEFT,
        I4_PRED_DC => REACH_I4X4_DC,
        I4_PRED_DDR => REACH_I4X4_DDR,
        I4_PRED_VR => REACH_I4X4_VR,
        I4_PRED_HD => REACH_I4X4_HD,
        _ => REACH_NONE, // I4_PRED_DC_128
    }
}

/// Reference reach of the chroma predictor installed at `mode`.
pub const fn reach_chroma(mode: i8) -> Reach {
    match mode {
        C_PRED_V | C_PRED_DC_T => REACH_CHROMA_TOP,
        C_PRED_H | C_PRED_DC_L => REACH_CHROMA_LEFT,
        C_PRED_DC => REACH_CHROMA_DC,
        C_PRED_P => REACH_CHROMA_PLANE,
        _ => REACH_NONE, // C_PRED_DC_128
    }
}

/// Reference reach of the I16x16 luma predictor installed at `mode`. `V` and `H`
/// are the imported `intra_pred_common` kernels; their reaches are stated here
/// because mode decision indexes one table, not two.
pub const fn reach_i16x16(mode: i8) -> Reach {
    match mode {
        I16_PRED_V | I16_PRED_DC_T => REACH_I16X16_TOP,
        I16_PRED_H | I16_PRED_DC_L => REACH_I16X16_LEFT,
        I16_PRED_DC => REACH_I16X16_DC,
        I16_PRED_P => REACH_I16X16_PLANE,
        _ => REACH_NONE, // I16_PRED_DC_128
    }
}

// --- I4x4 luma: kernels ------------------------------------------------------
//
// The C++ builds a 16-byte `uiSrc` scratch by *index assignment* and then moves it
// to `pPred` with `WelsFillingPred8x2to16` (two `u64` stores — a byte move, not
// arithmetic). Here the destination *is* that scratch, because every
// one of these modes assigns all sixteen positions. The index sets stay written out
// rather than folded into a formula: they are the mode's whole identity.

/// C++: `WelsI4x4LumaPredV_c`, `codec/encoder/core/src/get_intra_predictor.cpp:79`.
///
/// Each of the four rows is the four samples above the block. Takes `top` by value
/// shape rather than a cursor because it reads nothing else — mode decision offers
/// this mode when the row above exists whether or not the left column does.
#[inline(always)]
pub fn i4x4_luma_pred_v(pred: &mut [u8; 16], top: &[u8; 4]) {
    for y in 0..4 {
        let row: &mut [u8; 4] = (&mut pred[y * 4..][..4]).try_into().unwrap();
        *row = *top;
    }
}

/// C++: `WelsI4x4LumaPredH_c`, `:87`.
///
/// Row `y` is the sample at `(-1, y)` broadcast across it. Reads `[`[`REACH_I4X4_LEFT`]`]`
/// and nothing above, which is why it takes a cursor rather than a top array.
#[inline(always)]
pub fn i4x4_luma_pred_h(pred: &mut [u8; 16], reference: &impl RefSamples) {
    for y in 0..4 {
        let v = reference.at(-1, y as isize);
        let row: &mut [u8; 4] = (&mut pred[y * 4..][..4]).try_into().unwrap();
        row.fill(v);
    }
}

/// C++: `WelsI4x4LumaPredDc_c`, `:106`. Mean of the four left and four top
/// samples; reach [`REACH_I4X4_DC`].
#[inline(always)]
pub fn i4x4_luma_pred_dc(pred: &mut [u8; 16], reference: &impl RefSamples) {
    let mut sum: i32 = 4;
    for y in 0..4 {
        sum += reference.at(-1, y) as i32;
    }
    for v in reference.row_n::<4>(-1, 0) {
        sum += v as i32;
    }
    pred.fill((sum >> 3) as u8);
}

/// C++: `WelsI4x4LumaPredDcLeft_c`, `:114`. Mean of the four left samples only;
/// reach [`REACH_I4X4_LEFT`] — this is the mode decision picks when the row above
/// is unavailable, so the type must not be able to read it.
#[inline(always)]
pub fn i4x4_luma_pred_dc_left(pred: &mut [u8; 16], reference: &impl RefSamples) {
    let mut sum: i32 = 2;
    for y in 0..4 {
        sum += reference.at(-1, y) as i32;
    }
    pred.fill((sum >> 2) as u8);
}

/// C++: `WelsI4x4LumaPredDcTop_c`, `:121`. Mean of the four top samples only —
/// the mirror case, and correspondingly it takes only the top row.
#[inline(always)]
pub fn i4x4_luma_pred_dc_top(pred: &mut [u8; 16], top: &[u8; 4]) {
    let sum: i32 = 2 + top.iter().map(|&v| v as i32).sum::<i32>();
    pred.fill((sum >> 2) as u8);
}

/// C++: `WelsI4x4LumaPredDcNA_c`, `:127`. Neither neighbour exists; the block
/// predicts flat mid-grey. Takes no reference at all — the only honest shape for a
/// kernel whose C++ `pRef` parameter is dead.
#[inline(always)]
pub fn i4x4_luma_pred_dc_na(pred: &mut [u8; 16]) {
    pred.fill(0x80);
}

/// C++: `WelsI4x4LumaPredDDL_c`, `:134` — diagonal down-left.
///
/// Reads **eight** samples of the row above: four past the block's right edge, which
/// is the reach that makes this mode conditional on the top-right neighbour.
#[inline(always)]
pub fn i4x4_luma_pred_ddl(pred: &mut [u8; 16], top: &[u8; 8]) {
    let t = |i: usize| top[i] as i32;
    let ddl0 = ((2 + t(0) + t(2) + (t(1) << 1)) >> 2) as u8;
    let ddl1 = ((2 + t(1) + t(3) + (t(2) << 1)) >> 2) as u8;
    let ddl2 = ((2 + t(2) + t(4) + (t(3) << 1)) >> 2) as u8;
    let ddl3 = ((2 + t(3) + t(5) + (t(4) << 1)) >> 2) as u8;
    let ddl4 = ((2 + t(4) + t(6) + (t(5) << 1)) >> 2) as u8;
    let ddl5 = ((2 + t(5) + t(7) + (t(6) << 1)) >> 2) as u8;
    let ddl6 = ((2 + t(6) + t(7) + (t(7) << 1)) >> 2) as u8;
    pred[0] = ddl0;
    pred[1] = ddl1;
    pred[4] = ddl1;
    pred[2] = ddl2;
    pred[5] = ddl2;
    pred[8] = ddl2;
    pred[3] = ddl3;
    pred[6] = ddl3;
    pred[9] = ddl3;
    pred[12] = ddl3;
    pred[7] = ddl4;
    pred[10] = ddl4;
    pred[13] = ddl4;
    pred[11] = ddl5;
    pred[14] = ddl5;
    pred[15] = ddl6;
}

/// C++: `WelsI4x4LumaPredDDLTop_c`, `:164` — diagonal down-left with the top-right
/// neighbour replaced by `T3` repeated. Four top samples, no eighth.
#[inline(always)]
pub fn i4x4_luma_pred_ddl_top(pred: &mut [u8; 16], top: &[u8; 4]) {
    let t = |i: usize| top[i] as i32;
    let dlt0 = ((2 + t(0) + t(2) + (t(1) << 1)) >> 2) as u8;
    let dlt1 = ((2 + t(1) + t(3) + (t(2) << 1)) >> 2) as u8;
    let dlt2 = ((2 + t(2) + t(3) + (t(3) << 1)) >> 2) as u8;
    let dlt3 = ((2 + (t(3) << 2)) >> 2) as u8;
    // The C++ memsets ten bytes first and then overwrites seven of them; the four
    // assignments below that land inside `6..16` are the overwrites.
    pred[6..16].fill(dlt3);
    pred[0] = dlt0;
    pred[1] = dlt1;
    pred[4] = dlt1;
    pred[2] = dlt2;
    pred[5] = dlt2;
    pred[8] = dlt2;
    pred[3] = dlt3;
}

/// C++: `WelsI4x4LumaPredDDR_c`, `:186` — diagonal down-right. Reach
/// [`REACH_I4X4_DDR`]: the corner sample, four above and four left.
#[inline(always)]
pub fn i4x4_luma_pred_ddr(pred: &mut [u8; 16], reference: &impl RefSamples) {
    let lt = reference.at(-1, -1) as i32;
    let l0 = reference.at(-1, 0) as i32;
    let l1 = reference.at(-1, 1) as i32;
    let l2 = reference.at(-1, 2) as i32;
    let l3 = reference.at(-1, 3) as i32;
    let top = reference.row_n::<4>(-1, 0);
    let (t0, t1, t2, t3) = (top[0] as i32, top[1] as i32, top[2] as i32, top[3] as i32);
    let tl0 = 1 + lt + l0;
    let lt0 = 1 + lt + t0;
    let t01 = 1 + t0 + t1;
    let t12 = 1 + t1 + t2;
    let t23 = 1 + t2 + t3;
    let l01 = 1 + l0 + l1;
    let l12 = 1 + l1 + l2;
    let l23 = 1 + l2 + l3;
    let ddr0 = ((tl0 + lt0) >> 2) as u8;
    let ddr1 = ((lt0 + t01) >> 2) as u8;
    let ddr2 = ((t01 + t12) >> 2) as u8;
    let ddr3 = ((t12 + t23) >> 2) as u8;
    let ddr4 = ((tl0 + l01) >> 2) as u8;
    let ddr5 = ((l01 + l12) >> 2) as u8;
    let ddr6 = ((l12 + l23) >> 2) as u8;
    pred[0] = ddr0;
    pred[5] = ddr0;
    pred[10] = ddr0;
    pred[15] = ddr0;
    pred[1] = ddr1;
    pred[6] = ddr1;
    pred[11] = ddr1;
    pred[2] = ddr2;
    pred[7] = ddr2;
    pred[3] = ddr3;
    pred[4] = ddr4;
    pred[9] = ddr4;
    pred[14] = ddr4;
    pred[8] = ddr5;
    pred[13] = ddr5;
    pred[12] = ddr6;
}

/// C++: `WelsI4x4LumaPredVL_c`, `:228` — vertical left. **Seven** top samples, not
/// eight: `kuiVL9` is the last tap and it stops at `T6`.
#[inline(always)]
pub fn i4x4_luma_pred_vl(pred: &mut [u8; 16], top: &[u8; 7]) {
    let t = |i: usize| top[i] as i32;
    let vl0 = ((1 + t(0) + t(1)) >> 1) as u8;
    let vl1 = ((1 + t(1) + t(2)) >> 1) as u8;
    let vl2 = ((1 + t(2) + t(3)) >> 1) as u8;
    let vl3 = ((1 + t(3) + t(4)) >> 1) as u8;
    let vl4 = ((1 + t(4) + t(5)) >> 1) as u8;
    let vl5 = ((2 + t(0) + (t(1) << 1) + t(2)) >> 2) as u8;
    let vl6 = ((2 + t(1) + (t(2) << 1) + t(3)) >> 2) as u8;
    let vl7 = ((2 + t(2) + (t(3) << 1) + t(4)) >> 2) as u8;
    let vl8 = ((2 + t(3) + (t(4) << 1) + t(5)) >> 2) as u8;
    let vl9 = ((2 + t(4) + (t(5) << 1) + t(6)) >> 2) as u8;
    pred[0] = vl0;
    pred[1] = vl1;
    pred[8] = vl1;
    pred[2] = vl2;
    pred[9] = vl2;
    pred[3] = vl3;
    pred[10] = vl3;
    pred[4] = vl5;
    pred[5] = vl6;
    pred[12] = vl6;
    pred[6] = vl7;
    pred[13] = vl7;
    pred[7] = vl8;
    pred[14] = vl8;
    pred[11] = vl4;
    pred[15] = vl9;
}

/// C++: `WelsI4x4LumaPredVLTop_c`, `:265` — vertical left with the top-right
/// neighbour replaced by `T3` repeated.
///
/// The C++ walks from `pTopLeft = pRef - stride - 1` and indexes `+1 .. +4`, so it
/// *forms* a corner pointer but never reads through it; the four samples it reads
/// are the block's own top row.
#[inline(always)]
pub fn i4x4_luma_pred_vl_top(pred: &mut [u8; 16], top: &[u8; 4]) {
    let t = |i: usize| top[i] as i32;
    let vlt0 = ((1 + t(0) + t(1)) >> 1) as u8;
    let vlt1 = ((1 + t(1) + t(2)) >> 1) as u8;
    let vlt2 = ((1 + t(2) + t(3)) >> 1) as u8;
    let vlt3 = ((1 + (t(3) << 1)) >> 1) as u8;
    let vlt4 = ((2 + t(0) + (t(1) << 1) + t(2)) >> 2) as u8;
    let vlt5 = ((2 + t(1) + (t(2) << 1) + t(3)) >> 2) as u8;
    let vlt6 = ((2 + t(2) + (t(3) << 1) + t(3)) >> 2) as u8;
    let vlt7 = ((2 + (t(3) << 2)) >> 2) as u8;
    pred[0] = vlt0;
    pred[1] = vlt1;
    pred[8] = vlt1;
    pred[2] = vlt2;
    pred[9] = vlt2;
    pred[3] = vlt3;
    pred[10] = vlt3;
    pred[11] = vlt3;
    pred[4] = vlt4;
    pred[5] = vlt5;
    pred[12] = vlt5;
    pred[6] = vlt6;
    pred[13] = vlt6;
    pred[7] = vlt7;
    pred[14] = vlt7;
    pred[15] = vlt7;
}

/// C++: `WelsI4x4LumaPredVR_c`, `:294` — vertical right. Reach [`REACH_I4X4_VR`]:
/// the corner, four above, and **three** left — `L3` is never read.
#[inline(always)]
pub fn i4x4_luma_pred_vr(pred: &mut [u8; 16], reference: &impl RefSamples) {
    let lt = reference.at(-1, -1) as i32;
    let l0 = reference.at(-1, 0) as i32;
    let l1 = reference.at(-1, 1) as i32;
    let l2 = reference.at(-1, 2) as i32;
    let top = reference.row_n::<4>(-1, 0);
    let (t0, t1, t2, t3) = (top[0] as i32, top[1] as i32, top[2] as i32, top[3] as i32);
    let vr0 = ((1 + lt + t0) >> 1) as u8;
    let vr1 = ((1 + t0 + t1) >> 1) as u8;
    let vr2 = ((1 + t1 + t2) >> 1) as u8;
    let vr3 = ((1 + t2 + t3) >> 1) as u8;
    let vr4 = ((2 + l0 + (lt << 1) + t0) >> 2) as u8;
    let vr5 = ((2 + lt + (t0 << 1) + t1) >> 2) as u8;
    let vr6 = ((2 + t0 + (t1 << 1) + t2) >> 2) as u8;
    let vr7 = ((2 + t1 + (t2 << 1) + t3) >> 2) as u8;
    let vr8 = ((2 + lt + (l0 << 1) + l1) >> 2) as u8;
    let vr9 = ((2 + l0 + (l1 << 1) + l2) >> 2) as u8;
    pred[0] = vr0;
    pred[9] = vr0;
    pred[1] = vr1;
    pred[10] = vr1;
    pred[2] = vr2;
    pred[11] = vr2;
    pred[3] = vr3;
    pred[4] = vr4;
    pred[13] = vr4;
    pred[5] = vr5;
    pred[14] = vr5;
    pred[6] = vr6;
    pred[15] = vr6;
    pred[7] = vr7;
    pred[8] = vr8;
    pred[12] = vr9;
}

/// C++: `WelsI4x4LumaPredHU_c`, `:332` — horizontal up. Reach
/// [`REACH_I4X4_LEFT`]: four left samples, nothing above.
#[inline(always)]
pub fn i4x4_luma_pred_hu(pred: &mut [u8; 16], reference: &impl RefSamples) {
    let l0 = reference.at(-1, 0) as i32;
    let l1 = reference.at(-1, 1) as i32;
    let l2 = reference.at(-1, 2) as i32;
    let l3 = reference.at(-1, 3) as i32;
    let l01 = 1 + l0 + l1;
    let l12 = 1 + l1 + l2;
    let l23 = 1 + l2 + l3;
    let hu0 = (l01 >> 1) as u8;
    let hu1 = ((l01 + l12) >> 2) as u8;
    let hu2 = (l12 >> 1) as u8;
    let hu3 = ((l12 + l23) >> 2) as u8;
    let hu4 = (l23 >> 1) as u8;
    let hu5 = ((1 + l23 + (l3 << 1)) >> 2) as u8;
    pred[0] = hu0;
    pred[1] = hu1;
    pred[2] = hu2;
    pred[4] = hu2;
    pred[3] = hu3;
    pred[5] = hu3;
    pred[6] = hu4;
    pred[8] = hu4;
    pred[7] = hu5;
    pred[9] = hu5;
    pred[10..16].fill(l3 as u8);
}

/// C++: `WelsI4x4LumaPredHD_c`, `:363` — horizontal down. Reach
/// [`REACH_I4X4_HD`]: the corner, **three** above, four left — `T3` is never read.
#[inline(always)]
pub fn i4x4_luma_pred_hd(pred: &mut [u8; 16], reference: &impl RefSamples) {
    let lt = reference.at(-1, -1) as i32;
    let l0 = reference.at(-1, 0) as i32;
    let l1 = reference.at(-1, 1) as i32;
    let l2 = reference.at(-1, 2) as i32;
    let l3 = reference.at(-1, 3) as i32;
    let top = reference.row_n::<3>(-1, 0);
    let (t0, t1, t2) = (top[0] as i32, top[1] as i32, top[2] as i32);
    let hd0 = ((1 + lt + l0) >> 1) as u8;
    let hd1 = ((2 + l0 + (lt << 1) + t0) >> 2) as u8;
    let hd2 = ((2 + lt + (t0 << 1) + t1) >> 2) as u8;
    let hd3 = ((2 + t0 + (t1 << 1) + t2) >> 2) as u8;
    let hd4 = ((1 + l0 + l1) >> 1) as u8;
    let hd5 = ((2 + lt + (l0 << 1) + l1) >> 2) as u8;
    let hd6 = ((1 + l1 + l2) >> 1) as u8;
    let hd7 = ((2 + l0 + (l1 << 1) + l2) >> 2) as u8;
    let hd8 = ((1 + l2 + l3) >> 1) as u8;
    let hd9 = ((2 + l1 + (l2 << 1) + l3) >> 2) as u8;
    pred[0] = hd0;
    pred[6] = hd0;
    pred[1] = hd1;
    pred[7] = hd1;
    pred[2] = hd2;
    pred[3] = hd3;
    pred[4] = hd4;
    pred[10] = hd4;
    pred[5] = hd5;
    pred[11] = hd5;
    pred[8] = hd6;
    pred[14] = hd6;
    pred[9] = hd7;
    pred[15] = hd7;
    pred[12] = hd8;
    pred[13] = hd9;
}

// --- chroma 8x8: kernels -----------------------------------------------------

/// C++: `WelsIChromaPredV_c`, `:404`. Eight rows of the eight samples above.
#[inline(always)]
pub fn chroma_pred_v(pred: &mut [u8; 64], top: &[u8; 8]) {
    for y in 0..8 {
        let row: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        *row = *top;
    }
}

/// C++: `WelsIChromaPredH_c`, `:417`. Row `y` is `(-1, y)` broadcast.
///
/// The C++ walks rows 7 down to 0 carrying two descending offsets (and lets the
/// destination one wrap past zero on the last step); each row is written once from
/// an input the block does not contain, so ascending is the same eight writes in a
/// different order.
#[inline(always)]
pub fn chroma_pred_h(pred: &mut [u8; 64], reference: &impl RefSamples) {
    for y in 0..8 {
        let v = reference.at(-1, y as isize);
        let row: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        row.fill(v);
    }
}

/// C++: `WelsIChromaPredPlane_c`, `:433`. Reach [`REACH_CHROMA_PLANE`].
///
/// Arithmetic parity: every intermediate is `i32`.
/// `iTopSum`/`iLeftSum` are bounded by `10 * 255 = 2550`, `iLTshift` by
/// `510 << 4 = 8160`, and the per-sample expression by roughly `2^16` — nowhere
/// near `i32`.
#[inline(always)]
pub fn chroma_pred_plane(pred: &mut [u8; 64], reference: &impl RefSamples) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..4isize {
        top_sum += (i as i32 + 1)
            * (reference.at(4 + i, -1) as i32 - reference.at(2 - i, -1) as i32);
        left_sum += (i as i32 + 1)
            * (reference.at(-1, 4 + i) as i32 - reference.at(-1, 2 - i) as i32);
    }

    let lt_shift = (reference.at(-1, 7) as i32 + reference.at(7, -1) as i32) << 4;
    let top_shift = (17 * top_sum + 16) >> 5;
    let left_shift = (17 * left_sum + 16) >> 5;

    for i in 0..8i32 {
        let base = lt_shift + left_shift * (i - 3) + 16;
        let row: &mut [u8; 8] = (&mut pred[i as usize * 8..][..8]).try_into().unwrap();
        for (j, dst) in row.iter_mut().enumerate() {
            *dst = WelsClip1((base + top_shift * (j as i32 - 3)) >> 5);
        }
    }
}

/// C++: `WelsIChromaPredDc_c`, `:457`. Four quadrant means over the eight top and
/// eight left samples; reach [`REACH_CHROMA_DC`].
#[inline(always)]
pub fn chroma_pred_dc(pred: &mut [u8; 64], reference: &impl RefSamples) {
    let top = reference.row_n::<8>(-1, 0);
    let left = [
        reference.at(-1, 0),
        reference.at(-1, 1),
        reference.at(-1, 2),
        reference.at(-1, 3),
        reference.at(-1, 4),
        reference.at(-1, 5),
        reference.at(-1, 6),
        reference.at(-1, 7),
    ];
    /* caculate the iMean value */
    let mean1 = ((top[..4].iter().chain(left[..4].iter()).map(|&v| v as i32).sum::<i32>() + 4)
        >> 3) as u8;
    let sum2: u32 = top[4..].iter().map(|&v| v as u32).sum();
    let sum3: u32 = left[4..].iter().map(|&v| v as u32).sum();
    let mean2 = ((sum2 + 2) >> 2) as u8;
    let mean3 = ((sum3 + 2) >> 2) as u8;
    let mean4 = ((sum2 + sum3 + 4) >> 3) as u8;

    let top_mean = [mean1, mean1, mean1, mean1, mean2, mean2, mean2, mean2];
    let bottom_mean = [mean3, mean3, mean3, mean3, mean4, mean4, mean4, mean4];
    for y in 0..8 {
        let row: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        *row = if y < 4 { top_mean } else { bottom_mean };
    }
}

/// C++: `WelsIChromaPredDcLeft_c`, `:489`. Reach [`REACH_CHROMA_LEFT`] — the top
/// half of the block takes the mean of `L0..L3`, the bottom half `L4..L7`.
#[inline(always)]
pub fn chroma_pred_dc_left(pred: &mut [u8; 64], reference: &impl RefSamples) {
    let l = |y: isize| reference.at(-1, y) as i32;
    /* caculate the iMean value */
    let top_mean = ((l(0) + l(1) + l(2) + l(3) + 2) >> 2) as u8;
    let bottom_mean = ((l(4) + l(5) + l(6) + l(7) + 2) >> 2) as u8;
    pred[..32].fill(top_mean);
    pred[32..].fill(bottom_mean);
}

/// C++: `WelsIChromaPredDcTop_c`, `:512`. The left and right halves of every row
/// take the means of `T0..T3` and `T4..T7`; reads only the row above.
#[inline(always)]
pub fn chroma_pred_dc_top(pred: &mut [u8; 64], top: &[u8; 8]) {
    /* caculate the iMean value */
    let mean1 = ((top[..4].iter().map(|&v| v as i32).sum::<i32>() + 2) >> 2) as u8;
    let mean2 = ((top[4..].iter().map(|&v| v as i32).sum::<i32>() + 2) >> 2) as u8;
    let mean = [mean1, mean1, mean1, mean1, mean2, mean2, mean2, mean2];
    for y in 0..8 {
        let row: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        *row = mean;
    }
}

/// C++: `WelsIChromaPredDcNA_c`, `:529`. Neither neighbour exists.
#[inline(always)]
pub fn chroma_pred_dc_na(pred: &mut [u8; 64]) {
    pred.fill(0x80);
}

// --- I16x16 luma: kernels ----------------------------------------------------
//
// The vertical and horizontal modes are **not** here: they are shared with the
// decoder's common module and live in `common/intra_pred_common.rs`.
// `WelsInitIntraPredFuncs` installs those two by import.

/// C++: `WelsI16x16LumaPredPlane_c`, `:542`. Reach [`REACH_I16X16_PLANE`]; the same
/// argument as [`chroma_pred_plane`], with `iTopSum` bounded by `36 * 255`.
#[inline(always)]
pub fn i16x16_luma_pred_plane(pred: &mut [u8; 256], reference: &impl RefSamples) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..8isize {
        top_sum += (i as i32 + 1)
            * (reference.at(8 + i, -1) as i32 - reference.at(6 - i, -1) as i32);
        left_sum += (i as i32 + 1)
            * (reference.at(-1, 8 + i) as i32 - reference.at(-1, 6 - i) as i32);
    }

    let lt_shift = (reference.at(-1, 15) as i32 + reference.at(15, -1) as i32) << 4;
    let top_shift = (5 * top_sum + 32) >> 6;
    let left_shift = (5 * left_sum + 32) >> 6;

    for i in 0..16i32 {
        let base = lt_shift + left_shift * (i - 7) + 16;
        let row: &mut [u8; 16] = (&mut pred[i as usize * 16..][..16]).try_into().unwrap();
        for (j, dst) in row.iter_mut().enumerate() {
            *dst = WelsClip1((base + top_shift * (j as i32 - 7)) >> 5);
        }
    }
}

/// C++: `WelsI16x16LumaPredDc_c`, `:566`. Mean of the sixteen top and sixteen left
/// samples; reach [`REACH_I16X16_DC`].
#[inline(always)]
pub fn i16x16_luma_pred_dc(pred: &mut [u8; 256], reference: &impl RefSamples) {
    let mut sum: i32 = 16;
    for v in reference.row_n::<16>(-1, 0) {
        sum += v as i32;
    }
    for y in 0..16 {
        sum += reference.at(-1, y) as i32;
    }
    pred.fill((sum >> 5) as u8);
}

/// C++: `WelsI16x16LumaPredDcTop_c`, `:582`. Sixteen top samples only.
#[inline(always)]
pub fn i16x16_luma_pred_dc_top(pred: &mut [u8; 256], top: &[u8; 16]) {
    let sum: i32 = 8 + top.iter().map(|&v| v as i32).sum::<i32>();
    pred.fill((sum >> 4) as u8);
}

/// C++: `WelsI16x16LumaPredDcLeft_c`, `:595`. Reach [`REACH_I16X16_LEFT`] —
/// sixteen left samples only.
#[inline(always)]
pub fn i16x16_luma_pred_dc_left(pred: &mut [u8; 256], reference: &impl RefSamples) {
    let mut sum: i32 = 8;
    for y in 0..16 {
        sum += reference.at(-1, y) as i32;
    }
    pred.fill((sum >> 4) as u8);
}

/// C++: `WelsI16x16LumaPredDcNA_c`, `:610`. Neither neighbour exists.
#[inline(always)]
pub fn i16x16_luma_pred_dc_na(pred: &mut [u8; 256]) {
    pred.fill(0x80);
}
// ============================================================================
// C ABI shims
// ============================================================================
//
// The twenty-eight `Wels*_c` names below are shims: each takes off the cursor the
// samples its own kernel reads — nothing at all for the three `DcNA` modes — and
// hands them, with the packed destination, to the safe kernel above.
//
// The per-kernel notes share one availability argument, stated here once and
// referred to by each:
//
//   **Why the negative reads land inside the plane.** `rec` is a `SharedPlane`
//   cursor anchored at sample `(0, 0)` of the block in the *reconstructed* plane —
//   the port's form of `pMbCache->SPicData.pCsMb[i]` plus the block's coordinate
//   offset. Reads at `x = -1` and `y = -1` stay inside the allocation because both
//   codecs allocate picture planes with `PADDING_LENGTH` samples of border on every
//   side (32 luma, 16 chroma — `pic_queue.rs:AllocPicture`, `wels_preprocess.rs`),
//   so the padded prefix is part of the same cell slice. An anchor that left no
//   room for a kernel's reach would panic at the slice index rather than read out
//   of bounds — which is what each `# Panics` below says.
//
//   **Why the reads are correct.** Mode decision does not call an arbitrary
//   predictor: it indexes `g_kiIntra4AvailMode` / `g_kiIntra16AvaliMode` /
//   `g_kiIntraChromaAvailMode` with the block's neighbour mask and only offers
//   modes whose neighbours exist (`svc_base_layer_md.rs:437`, `:734`,
//   `svc_mode_decision.rs:1167`). That correspondence is asserted, offset by
//   offset, in `reach_table_agrees_with_the_availability_tables`.
//
//   **The destination.** `pred` is a *packed* candidate buffer — 16 bytes for
//   I4x4, 64 for chroma, 256 for I16x16, at implicit strides of 4, 8 and 16, about
//   which the cursor's own stride says nothing. Every one of these kernels writes
//   all of it, and none of them writes a byte more.
//
// Per-kernel, each note names only the samples that kernel reads. Those are the
// `REACH_*` constants; `ref_span` is where one becomes a byte span, and nothing
// else in this file does that arithmetic.

// --- I4x4 luma ---------------------------------------------------------------

/// C++: `WelsI4x4LumaPredV_c`, `get_intra_predictor.cpp:79`.
///
/// Reads [`REACH_I4X4_TOP`]: the four samples of the row above, at `(0..4, -1)`
/// from the anchor, and nothing else — this kernel never reads to the left.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredV_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_v(pred, &rec.row_n::<4>(-1, 0))
}

/// C++: `WelsI4x4LumaPredH_c`, `:87`.
///
/// Reads [`REACH_I4X4_LEFT`]: the four samples at `(-1, 0..4)`, the column left of
/// the block. This kernel never reads the row above.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredH_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_h(pred, rec)  // reach: REACH_I4X4_LEFT
}

/// C++: `WelsI4x4LumaPredDc_c`, `:106`.
///
/// Reads [`REACH_I4X4_DC`]: four samples above at `(0..4, -1)` and four to the
/// left at `(-1, 0..4)`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredDc_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_dc(pred, rec)  // reach: REACH_I4X4_DC
}

/// C++: `WelsI4x4LumaPredDcLeft_c`, `:114`.
///
/// Reads [`REACH_I4X4_LEFT`] — the four samples at `(-1, 0..4)`. This is the one
/// mode decision picks when the row above is *unavailable*, and it reads nothing
/// there.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredDcLeft_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_dc_left(pred, rec)  // reach: REACH_I4X4_LEFT
}

/// C++: `WelsI4x4LumaPredDcTop_c`, `:121`.
///
/// Reads [`REACH_I4X4_TOP`]: the four samples at `(0..4, -1)` — the mirror of
/// [`WelsI4x4LumaPredDcLeft_c`], offered when the left column is unavailable, and
/// correspondingly handed the top row alone.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredDcTop_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_dc_top(pred, &rec.row_n::<4>(-1, 0))
}

/// C++: `WelsI4x4LumaPredDcNA_c`, `:127`.
///
/// Reads nothing ([`REACH_NONE`]) — neither neighbour exists. The C++ takes a
/// `pRef` it never dereferences to fit the table's signature, and this shim keeps
/// the parameter for the same reason.
pub fn WelsI4x4LumaPredDcNA_c(pred: &mut [u8; 16], _rec: &RecCursor<'_>) {
    i4x4_luma_pred_dc_na(pred)
}

/// C++: `WelsI4x4LumaPredDDL_c`, `:134` — diagonal down-left.
///
/// Reads [`REACH_I4X4_TOP8`]: **eight** samples of the row above, `(0..8, -1)` —
/// four past the block's right edge, which is why `g_kiIntra4AvailMode` offers this
/// mode only at offsets whose top-right bit is set.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredDDL_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_ddl(pred, &rec.row_n::<8>(-1, 0))
}

/// C++: `WelsI4x4LumaPredDDLTop_c`, `:164` — down-left with the top-right
/// neighbour substituted.
///
/// Reads [`REACH_I4X4_TOP`]: the four samples at `(0..4, -1)`.
///
/// No availability offset offers this mode (see
/// `reach_table_agrees_with_the_availability_tables`).
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredDDLTop_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_ddl_top(pred, &rec.row_n::<4>(-1, 0))
}

/// C++: `WelsI4x4LumaPredDDR_c`, `:186` — diagonal down-right.
///
/// Reads [`REACH_I4X4_DDR`]: the corner at `(-1, -1)`, four samples above at
/// `(0..4, -1)` and four to the left at `(-1, 0..4)`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredDDR_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_ddr(pred, rec)  // reach: REACH_I4X4_DDR
}

/// C++: `WelsI4x4LumaPredVL_c`, `:228` — vertical left.
///
/// Reads [`REACH_I4X4_TOP7`]: **seven** samples of the row above, `(0..7, -1)` —
/// three past the block's right edge, not four: the last tap (`vl9`) stops at `T6`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredVL_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_vl(pred, &rec.row_n::<7>(-1, 0))
}

/// C++: `WelsI4x4LumaPredVLTop_c`, `:265` — vertical left with the top-right
/// neighbour substituted.
///
/// Reads [`REACH_I4X4_TOP`]: the four samples at `(0..4, -1)`.
///
/// Like `DDL_TOP`, no availability offset offers this mode.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredVLTop_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_vl_top(pred, &rec.row_n::<4>(-1, 0))
}

/// C++: `WelsI4x4LumaPredVR_c`, `:294` — vertical right.
///
/// Reads [`REACH_I4X4_VR`]: the corner at `(-1, -1)`, four samples above and
/// **three** to the left — `L3`, at `(-1, 3)`, is never read.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredVR_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_vr(pred, rec)  // reach: REACH_I4X4_VR
}

/// C++: `WelsI4x4LumaPredHU_c`, `:332` — horizontal up.
///
/// Reads [`REACH_I4X4_LEFT`] — the four samples at `(-1, 0..4)`, nothing above.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredHU_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_hu(pred, rec)  // reach: REACH_I4X4_LEFT
}

/// C++: `WelsI4x4LumaPredHD_c`, `:363` — horizontal down.
///
/// Reads [`REACH_I4X4_HD`]: the corner at `(-1, -1)`, **three** samples above
/// (`T3` is never read) and four to the left.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI4x4LumaPredHD_c(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    i4x4_luma_pred_hd(pred, rec)  // reach: REACH_I4X4_HD
}

// --- chroma 8x8 --------------------------------------------------------------

/// C++: `WelsIChromaPredV_c`, `:404`.
///
/// Reads [`REACH_CHROMA_TOP`]: the eight samples of the row above, `(0..8, -1)`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsIChromaPredV_c(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    chroma_pred_v(pred, &rec.row_n::<8>(-1, 0))
}

/// C++: `WelsIChromaPredH_c`, `:417`.
///
/// Reads [`REACH_CHROMA_LEFT`]: the eight samples at `(-1, 0..8)`, nothing above.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsIChromaPredH_c(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    chroma_pred_h(pred, rec)  // reach: REACH_CHROMA_LEFT
}

/// C++: `WelsIChromaPredPlane_c`, `:433`.
///
/// Reads [`REACH_CHROMA_PLANE`]: the corner at `(-1, -1)`, eight samples above at
/// `(0..8, -1)` and eight to the left at `(-1, 0..8)`. The corner is reached by the
/// `at(2 - i, -1)` and `at(-1, 2 - i)` arms at `i == 3`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsIChromaPredPlane_c(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    chroma_pred_plane(pred, rec)  // reach: REACH_CHROMA_PLANE
}

/// C++: `WelsIChromaPredDc_c`, `:457`.
///
/// Reads [`REACH_CHROMA_DC`]: eight samples above at `(0..8, -1)` and eight to the
/// left at `(-1, 0..8)`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsIChromaPredDc_c(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    chroma_pred_dc(pred, rec)  // reach: REACH_CHROMA_DC
}

/// C++: `WelsIChromaPredDcLeft_c`, `:489`.
///
/// Reads [`REACH_CHROMA_LEFT`] — the eight samples at `(-1, 0..8)`, nothing above.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsIChromaPredDcLeft_c(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    chroma_pred_dc_left(pred, rec)  // reach: REACH_CHROMA_LEFT
}

/// C++: `WelsIChromaPredDcTop_c`, `:512`.
///
/// Reads [`REACH_CHROMA_TOP`]: the eight samples at `(0..8, -1)`, nothing to the
/// left.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsIChromaPredDcTop_c(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    chroma_pred_dc_top(pred, &rec.row_n::<8>(-1, 0))
}

/// C++: `WelsIChromaPredDcNA_c`, `:529`.
///
/// Reads nothing ([`REACH_NONE`]) — neither neighbour exists; the reference
/// parameter is kept only to fit the dispatch table's signature.
pub fn WelsIChromaPredDcNA_c(pred: &mut [u8; 64], _rec: &RecCursor<'_>) {
    chroma_pred_dc_na(pred)
}

// --- I16x16 luma -------------------------------------------------------------

/// C++: `WelsI16x16LumaPredPlane_c`, `:542`.
///
/// Reads [`REACH_I16X16_PLANE`]: the corner at `(-1, -1)`, sixteen samples above at
/// `(0..16, -1)` and sixteen to the left at `(-1, 0..16)` — the corner via the
/// `at(6 - i, -1)` and `at(-1, 6 - i)` arms at `i == 7`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI16x16LumaPredPlane_c(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    i16x16_luma_pred_plane(pred, rec)  // reach: REACH_I16X16_PLANE
}

/// C++: `WelsI16x16LumaPredDc_c`, `:566`.
///
/// Reads [`REACH_I16X16_DC`]: sixteen samples above at `(0..16, -1)` and sixteen to
/// the left at `(-1, 0..16)`.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI16x16LumaPredDc_c(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    i16x16_luma_pred_dc(pred, rec)  // reach: REACH_I16X16_DC
}

/// C++: `WelsI16x16LumaPredDcTop_c`, `:582`.
///
/// Reads [`REACH_I16X16_TOP`]: the sixteen samples at `(0..16, -1)`, nothing to the
/// left.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI16x16LumaPredDcTop_c(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    i16x16_luma_pred_dc_top(pred, &rec.row_n::<16>(-1, 0))
}

/// C++: `WelsI16x16LumaPredDcLeft_c`, `:595`.
///
/// Reads [`REACH_I16X16_LEFT`] — the sixteen samples at `(-1, 0..16)`, nothing
/// above.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI16x16LumaPredDcLeft_c(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    i16x16_luma_pred_dc_left(pred, rec)  // reach: REACH_I16X16_LEFT
}

/// C++: `WelsI16x16LumaPredDcNA_c`, `:610`.
///
/// Reads nothing ([`REACH_NONE`]) — neither neighbour exists; the reference
/// parameter is kept only to fit the dispatch table's signature.
pub fn WelsI16x16LumaPredDcNA_c(pred: &mut [u8; 256], _rec: &RecCursor<'_>) {
    i16x16_luma_pred_dc_na(pred)
}

/// C++: `WelsI16x16LumaPredV_c`, `codec/common/src/intra_pred_common.cpp` — mode 0,
/// vertical. The kernel stays in `common`.
///
/// Reads [`REACH_I16X16_TOP`]: the sixteen samples of the row above, and nothing
/// else — in particular never to the left.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI16x16LumaPredV_c(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    i16x16_luma_pred_v(pred, &rec.row_n::<16>(-1, 0))
}

/// C++: `WelsI16x16LumaPredH_c`, `codec/common/src/intra_pred_common.cpp` — mode 1,
/// horizontal. The kernel stays in `common`.
///
/// Reads [`REACH_I16X16_LEFT`]: one sample per row at `x = -1`, and never the row
/// above — which is why it and the vertical one take different reference shapes
/// rather than a shared span that would have each claiming the other's reach.
///
/// # Panics
/// If `rec` is anchored so that this reach leaves the plane — `RecCursor` reads
/// are slice indexes.
pub fn WelsI16x16LumaPredH_c(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    i16x16_luma_pred_h(pred, rec)
}

/// `get_intra_predictor.cpp:614`. Installs the scalar predictor tables. The SIMD
/// overrides that follow in the C++ are all guarded by `kuiCpuFlag & WELS_CPU_*`,
/// which is 0 on every target this port builds for, so none are translated.
pub fn WelsInitIntraPredFuncs(pFuncList: &mut SWelsFuncPtrList, kuiCpuFlag: u32) {
    let fl = pFuncList;

    fl.pfGetLumaI16x16Pred[I16_PRED_V as usize] = Some(WelsI16x16LumaPredV_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_H as usize] = Some(WelsI16x16LumaPredH_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC as usize] = Some(WelsI16x16LumaPredDc_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_P as usize] = Some(WelsI16x16LumaPredPlane_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC_L as usize] = Some(WelsI16x16LumaPredDcLeft_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC_T as usize] = Some(WelsI16x16LumaPredDcTop_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC_128 as usize] = Some(WelsI16x16LumaPredDcNA_c);

    fl.pfGetLumaI4x4Pred[I4_PRED_V as usize] = Some(WelsI4x4LumaPredV_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_H as usize] = Some(WelsI4x4LumaPredH_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC as usize] = Some(WelsI4x4LumaPredDc_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC_L as usize] = Some(WelsI4x4LumaPredDcLeft_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC_T as usize] = Some(WelsI4x4LumaPredDcTop_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC_128 as usize] = Some(WelsI4x4LumaPredDcNA_c);

    fl.pfGetLumaI4x4Pred[I4_PRED_DDL as usize] = Some(WelsI4x4LumaPredDDL_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DDL_TOP as usize] = Some(WelsI4x4LumaPredDDLTop_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DDR as usize] = Some(WelsI4x4LumaPredDDR_c);

    fl.pfGetLumaI4x4Pred[I4_PRED_VL as usize] = Some(WelsI4x4LumaPredVL_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_VL_TOP as usize] = Some(WelsI4x4LumaPredVLTop_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_VR as usize] = Some(WelsI4x4LumaPredVR_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_HU as usize] = Some(WelsI4x4LumaPredHU_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_HD as usize] = Some(WelsI4x4LumaPredHD_c);

    fl.pfGetChromaPred[C_PRED_DC as usize] = Some(WelsIChromaPredDc_c);
    fl.pfGetChromaPred[C_PRED_H as usize] = Some(WelsIChromaPredH_c);
    fl.pfGetChromaPred[C_PRED_V as usize] = Some(WelsIChromaPredV_c);
    fl.pfGetChromaPred[C_PRED_P as usize] = Some(WelsIChromaPredPlane_c);
    fl.pfGetChromaPred[C_PRED_DC_L as usize] = Some(WelsIChromaPredDcLeft_c);
    fl.pfGetChromaPred[C_PRED_DC_T as usize] = Some(WelsIChromaPredDcTop_c);
    fl.pfGetChromaPred[C_PRED_DC_128 as usize] = Some(WelsIChromaPredDcNA_c);

    #[cfg(target_arch = "x86_64")]
    if (kuiCpuFlag & crate::common::cpu_core::WELS_CPU_SSE2) != 0 {
        use crate::simd::x86_64::intra_pred::*;
        fl.pfGetLumaI16x16Pred[I16_PRED_V as usize] = Some(enc_i16x16_luma_pred_v_sse2);
        fl.pfGetLumaI16x16Pred[I16_PRED_H as usize] = Some(enc_i16x16_luma_pred_h_sse2);
        fl.pfGetLumaI16x16Pred[I16_PRED_DC as usize] = Some(enc_i16x16_luma_pred_dc_sse2);
        fl.pfGetLumaI16x16Pred[I16_PRED_P as usize] = Some(enc_i16x16_luma_pred_plane_sse2);

        fl.pfGetChromaPred[C_PRED_DC as usize] = Some(enc_chroma_pred_dc);
        fl.pfGetChromaPred[C_PRED_H as usize] = Some(enc_chroma_pred_h);
        fl.pfGetChromaPred[C_PRED_V as usize] = Some(enc_chroma_pred_v_sse2);
        fl.pfGetChromaPred[C_PRED_P as usize] = Some(enc_chroma_pred_plane_sse2);

        fl.pfGetLumaI4x4Pred[I4_PRED_V as usize] = Some(enc_i4x4_luma_pred_v_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_H as usize] = Some(enc_i4x4_luma_pred_h_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_DC as usize] = Some(enc_i4x4_luma_pred_dc_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_DDL as usize] = Some(enc_i4x4_luma_pred_ddl_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_DDR as usize] = Some(enc_i4x4_luma_pred_ddr_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_VL as usize] = Some(enc_i4x4_luma_pred_vl_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_VR as usize] = Some(enc_i4x4_luma_pred_vr_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_HU as usize] = Some(enc_i4x4_luma_pred_hu_sse2);
        fl.pfGetLumaI4x4Pred[I4_PRED_HD as usize] = Some(enc_i4x4_luma_pred_hd_sse2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::encoder::rec_view::shared_plane_for_test;
    use crate::safe::plane::PaddedPlane;

    /// A reference plane with a known ramp, and the seam cursor at its `(0, 0)`.
    fn ramp_at_origin(w: usize, h: usize, pad: usize, stride: usize) -> PaddedPlane {
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                p.set(x, y, (((y + 64) * stride as isize + (x + 64)) % 251) as u8);
            }
        }
        p
    }

    fn flat_at_origin(w: usize, h: usize, pad: usize, stride: usize, v: u8) -> PaddedPlane {
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                p.set(x, y, v);
            }
        }
        p
    }

    #[test]
    fn i4x4_dc_na_fills_0x80() {
        let mut plane = ramp_at_origin(16, 16, 16, 48);
        let view = shared_plane_for_test(&mut plane);
        let mut pred = [0u8; 16];
        WelsI4x4LumaPredDcNA_c(&mut pred, &view.cursor(0, 0));
        assert_eq!(pred, [0x80u8; 16]);
    }

    #[test]
    fn i16x16_dc_na_fills_0x80() {
        let mut plane = ramp_at_origin(16, 16, 16, 48);
        let view = shared_plane_for_test(&mut plane);
        let mut pred = [0u8; 256];
        WelsI16x16LumaPredDcNA_c(&mut pred, &view.cursor(0, 0));
        assert!(pred.iter().all(|&b| b == 0x80));
    }

    #[test]
    fn chroma_dc_na_fills_0x80() {
        let mut plane = ramp_at_origin(16, 16, 16, 48);
        let view = shared_plane_for_test(&mut plane);
        let mut pred = [0u8; 64];
        WelsIChromaPredDcNA_c(&mut pred, &view.cursor(0, 0));
        assert_eq!(pred, [0x80u8; 64]);
    }

    /// I4x4 vertical replicates the four top samples down all four rows; horizontal
    /// replicates each left sample across its row.
    #[test]
    fn i4x4_v_and_h_replicate_their_edge() {
        let mut plane = ramp_at_origin(16, 16, 16, 48);
        let view = shared_plane_for_test(&mut plane);
        let rec = view.cursor(0, 0);

        let top: [u8; 4] = core::array::from_fn(|i| rec.at(i as isize, -1));
        let mut pred = [0u8; 16];
        WelsI4x4LumaPredV_c(&mut pred, &rec);
        for r in 0..4 {
            assert_eq!(&pred[r * 4..r * 4 + 4], &top, "V row {r}");
        }

        WelsI4x4LumaPredH_c(&mut pred, &rec);
        for r in 0..4 {
            let left = rec.at(-1, r as isize);
            assert_eq!(&pred[r * 4..r * 4 + 4], &[left; 4], "H row {r}");
        }
    }

    /// I16x16 DC is the rounded mean of the 16 top and 16 left samples; DcTop and
    /// DcLeft use only one edge with a different rounding shift.
    #[test]
    fn i16x16_dc_variants_match_their_definitions() {
        let mut plane = ramp_at_origin(16, 16, 16, 48);
        let view = shared_plane_for_test(&mut plane);
        let rec = view.cursor(0, 0);

        let top_sum: i32 = (0..16).map(|i| rec.at(i, -1) as i32).sum();
        let left_sum: i32 = (0..16).map(|i| rec.at(-1, i) as i32).sum();

        let mut pred = [0u8; 256];
        WelsI16x16LumaPredDc_c(&mut pred, &rec);
        assert!(pred.iter().all(|&b| b == pred[0]));
        assert_eq!(pred[0], ((16 + top_sum + left_sum) >> 5) as u8);

        WelsI16x16LumaPredDcTop_c(&mut pred, &rec);
        assert_eq!(pred[0], ((8 + top_sum) >> 4) as u8);

        WelsI16x16LumaPredDcLeft_c(&mut pred, &rec);
        assert_eq!(pred[0], ((8 + left_sum) >> 4) as u8);
    }

    /// Chroma vertical writes the same eight top samples into all eight rows of the
    /// stride-8 prediction block; horizontal broadcasts each left sample.
    #[test]
    fn chroma_v_and_h_replicate_their_edge() {
        let mut plane = ramp_at_origin(16, 16, 16, 48);
        let view = shared_plane_for_test(&mut plane);
        let rec = view.cursor(0, 0);
        let top: Vec<u8> = (0..8).map(|i| rec.at(i, -1)).collect();

        let mut pred = [0u8; 64];
        WelsIChromaPredV_c(&mut pred, &rec);
        for r in 0..8 {
            assert_eq!(&pred[r * 8..r * 8 + 8], &top[..], "V row {r}");
        }

        WelsIChromaPredH_c(&mut pred, &rec);
        for r in 0..8 {
            let left = rec.at(-1, r as isize);
            assert_eq!(&pred[r * 8..r * 8 + 8], &[left; 8], "H row {r}");
        }
    }

    /// A flat reference plane must produce a flat prediction for every mode — the
    /// cheapest check that the DDL/DDR/VL/VR/HU/HD tap patterns cover all 16 samples.
    #[test]
    fn all_i4x4_modes_are_flat_on_a_flat_plane() {
        let mut plane = flat_at_origin(16, 16, 16, 48, 137);
        let view = shared_plane_for_test(&mut plane);
        let rec = view.cursor(0, 0);

        let mut fl = SWelsFuncPtrList::default();
        WelsInitIntraPredFuncs(&mut fl, 0);

        for mode in 0..14usize {
            let Some(f) = fl.pfGetLumaI4x4Pred[mode] else { continue };
            let mut pred = [0u8; 16];
            f(&mut pred, &rec);
            let expected = if mode == I4_PRED_DC_128 as usize { 0x80 } else { 137 };
            assert!(
                pred.iter().all(|&b| b == expected),
                "mode {mode} produced {pred:?}, expected all {expected}"
            );
        }
    }

    /// Same for chroma and I16x16.
    #[test]
    fn all_chroma_and_i16x16_modes_are_flat_on_a_flat_plane() {
        let mut plane = flat_at_origin(16, 16, 24, 64, 91);
        let view = shared_plane_for_test(&mut plane);
        let rec = view.cursor(0, 0);

        let mut fl = SWelsFuncPtrList::default();
        WelsInitIntraPredFuncs(&mut fl, 0);

        for mode in 0..7usize {
            if let Some(f) = fl.pfGetChromaPred[mode] {
                let mut pred = [0u8; 64];
                f(&mut pred, &rec);
                let expected = if mode == C_PRED_DC_128 as usize { 0x80 } else { 91 };
                assert!(pred.iter().all(|&b| b == expected), "chroma mode {mode}");
            }
            if let Some(f) = fl.pfGetLumaI16x16Pred[mode] {
                let mut pred = [0u8; 256];
                f(&mut pred, &rec);
                let expected = if mode == I16_PRED_DC_128 as usize { 0x80 } else { 91 };
                assert!(pred.iter().all(|&b| b == expected), "i16x16 mode {mode}");
            }
        }
    }

    /// **The availability argument, checked.** Every mode an availability table
    /// offers must read only neighbours that table's index says exist.
    ///
    /// This is what the per-kernel [`Reach`] types are *for*. The shims' notes say
    /// the negative reads land inside the plane because it is `PADDING_LENGTH`-
    /// padded; this test says they are *correct* because mode decision never offers
    /// a mode whose neighbours are missing.
    ///
    /// Two facts the test pins that are easy to lose:
    ///
    /// * **`g_kiIntra4AvailMode` never offers `DDL_TOP` or `VL_TOP`.** Both are
    ///   installed in the dispatch table and neither is reachable through it — the
    ///   `*_TOP` variants exist for a top-right-substitution path the C++ tables do
    ///   not take either.
    /// * **The I16x16 and chroma tables have no top-left bit.** Their index is
    ///   `uiNeighborIntra & 0x07` = left | top<<1 | topright<<2, yet the plane mode
    ///   they offer at index 7 reads the corner at `(-1, -1)`. The corner is
    ///   available whenever left and top both are — raster order inside a slice —
    ///   so that is the rule asserted here, and the C++ relies on exactly the same
    ///   implication.
    #[test]
    fn reach_table_agrees_with_the_availability_tables() {
        use crate::encoder::svc_base_layer_md::{
            g_kiIntra4AvailCount, g_kiIntra4AvailMode, g_kiIntraChromaAvailMode,
        };
        use crate::encoder::svc_mode_decision::g_kiIntra16AvaliMode;

        // --- I4x4: index is left | top<<1 | topleft<<2 | topright<<3 -----------
        let mut i4x4_seen = [false; 14];
        for (idx, modes) in g_kiIntra4AvailMode.iter().enumerate() {
            let (left, top, topleft, topright) =
                (idx & 1 != 0, idx & 2 != 0, idx & 4 != 0, idx & 8 != 0);
            let count = g_kiIntra4AvailCount[idx] as usize;
            for &mode in &modes[..count] {
                // `I4_PRED_INVALID` and `I4_PRED_V` are **both zero** in the C++ and
                // in this port, so the padding value is indistinguishable from a
                // real mode; `g_kiIntra4AvailCount` is the only thing that says
                // where a row's live prefix ends. Hence the slice, not a filter.
                let r = reach_i4x4(mode);
                i4x4_seen[mode as usize] = true;
                assert!(r.left == 0 || left, "offset {idx:04b} offers mode {mode}, which reads left");
                assert!(r.top == 0 || top, "offset {idx:04b} offers mode {mode}, which reads above");
                assert!(
                    r.top <= 4 || topright,
                    "offset {idx:04b} offers mode {mode}, which reads {} samples above — past the \
                     block's right edge, into the top-right neighbour",
                    r.top
                );
                assert!(
                    !r.corner || topleft,
                    "offset {idx:04b} offers mode {mode}, which reads the corner"
                );
            }
        }
        assert!(!i4x4_seen[I4_PRED_DDL_TOP as usize], "DDL_TOP became reachable");
        assert!(!i4x4_seen[I4_PRED_VL_TOP as usize], "VL_TOP became reachable");
        for m in [
            I4_PRED_V, I4_PRED_H, I4_PRED_DC, I4_PRED_DDL, I4_PRED_DDR, I4_PRED_VR, I4_PRED_HD,
            I4_PRED_VL, I4_PRED_HU, I4_PRED_DC_L, I4_PRED_DC_T, I4_PRED_DC_128,
        ] {
            assert!(i4x4_seen[m as usize], "mode {m} is offered by no availability offset");
        }

        // --- I16x16 and chroma: index is left | top<<1 | topright<<2 -----------
        // No top-left bit; `corner` is legal exactly when left and top both are.
        for (table, name, reach) in [
            (&g_kiIntra16AvaliMode, "I16x16", reach_i16x16 as fn(i8) -> Reach),
            (&g_kiIntraChromaAvailMode, "chroma", reach_chroma as fn(i8) -> Reach),
        ] {
            for (idx, row) in table.iter().enumerate() {
                let (left, top) = (idx & 1 != 0, idx & 2 != 0);
                let count = row[4] as usize;
                for &mode in &row[..count] {
                    let r = reach(mode);
                    assert!(r.left == 0 || left, "{name} offset {idx:03b} mode {mode} reads left");
                    assert!(r.top == 0 || top, "{name} offset {idx:03b} mode {mode} reads above");
                    assert!(
                        !r.corner || (left && top),
                        "{name} offset {idx:03b} mode {mode} reads the corner without both \
                         neighbours"
                    );
                }
            }
        }
    }

    /// `ref_span` claims exactly what the reach describes and not a byte more: the
    /// anchor sits `center` bytes into the slice, the lowest read lands at 0, and
    /// the highest lands at `len - 1`.
    #[test]
    fn ref_span_is_tight_at_both_ends() {
        for stride in [4usize, 16, 33, 240] {
            for reach in [
                REACH_I4X4_TOP, REACH_I4X4_TOP7, REACH_I4X4_TOP8, REACH_I4X4_LEFT,
                REACH_I4X4_DC, REACH_I4X4_DDR, REACH_I4X4_VR, REACH_I4X4_HD,
                REACH_CHROMA_TOP, REACH_CHROMA_LEFT, REACH_CHROMA_DC, REACH_CHROMA_PLANE,
                REACH_I16X16_TOP, REACH_I16X16_LEFT, REACH_I16X16_DC, REACH_I16X16_PLANE,
            ] {
                let (len, center) = ref_span(stride, reach);
                let s = stride as isize;
                let c = center as isize;
                let mut lo = isize::MAX;
                let mut hi = isize::MIN;
                let mut note = |off: isize| {
                    lo = lo.min(off);
                    hi = hi.max(off);
                };
                if reach.corner {
                    note(-s - 1);
                }
                for x in 0..reach.top as isize {
                    note(-s + x);
                }
                for y in 0..reach.left as isize {
                    note(y * s - 1);
                }
                assert_eq!(c + lo, 0, "stride {stride} {reach:?}: span starts before the first read");
                assert_eq!(
                    c + hi,
                    len as isize - 1,
                    "stride {stride} {reach:?}: span outlives the last read"
                );
            }
        }
    }

    /// Every table slot the mode-decision code can index must be filled.
    #[test]
    fn init_fills_every_slot_the_md_layer_indexes() {
        let mut fl = SWelsFuncPtrList::default();
        WelsInitIntraPredFuncs(&mut fl, 0);

        for m in [I16_PRED_V, I16_PRED_H, I16_PRED_DC, I16_PRED_P, I16_PRED_DC_L, I16_PRED_DC_T, I16_PRED_DC_128] {
            assert!(fl.pfGetLumaI16x16Pred[m as usize].is_some(), "I16 mode {m}");
        }
        for m in [
            I4_PRED_V, I4_PRED_H, I4_PRED_DC, I4_PRED_DDL, I4_PRED_DDR, I4_PRED_VR, I4_PRED_HD,
            I4_PRED_VL, I4_PRED_HU, I4_PRED_DC_L, I4_PRED_DC_T, I4_PRED_DC_128, I4_PRED_DDL_TOP,
            I4_PRED_VL_TOP,
        ] {
            assert!(fl.pfGetLumaI4x4Pred[m as usize].is_some(), "I4 mode {m}");
        }
        for m in [C_PRED_DC, C_PRED_H, C_PRED_V, C_PRED_P, C_PRED_DC_L, C_PRED_DC_T, C_PRED_DC_128] {
            assert!(fl.pfGetChromaPred[m as usize].is_some(), "chroma mode {m}");
        }
    }
}
