//! Differential tests for the Phase 2 kernel conversions (plan §Phase 2, recipe R2).
//!
//! While a family is being converted this file holds one entry per kernel, driving the
//! **old** raw-pointer implementation and the **new** safe one over the same random
//! inputs and asserting they agree bit for bit — not on the nominal block, but on
//! **every byte of the destination surface**. Comparing whole buffers is the point: a
//! kernel that writes one byte past its block is finding F1's defect class, and a
//! block-shaped assertion is blind to exactly that.
//!
//! Those entries are **deliberately short-lived**. A family's commit A adds them while
//! the old code is untouched; the family's commit B replaces the old body with a shim
//! onto the safe kernel, at which point the comparison is tautological and the entry is
//! deleted in that same commit. What survives is only tests that pin a *property*.
//!
//! This file lives outside `src/`, so unlike everything under `src/safe/` it may use
//! `unsafe` — it has to, because the old side *is* raw-pointer code. Running it under
//! Miri therefore also checks those raw accesses for UB, which is how F7 was found
//! (`rust/docs/phase1_findings.md`).
//!
//! ## Converted so far
//!
//! * `decoder/decode_mb_aux.rs` — all four kernels. `ba13bdbd` proved them against the
//!   raw ones across six strides (the minimum legal, three non-multiples of 16, and two
//!   real picture strides) with the 4x4 pair driven at the full `i16` coefficient range;
//!   the shim commit then deleted those equivalences. The entry that survives below is
//!   a property, not a comparison.

mod common;

use common::prng::Prng;
use openh264_rs::decoder::decode_mb_aux as dec_aux;
use openh264_rs::decoder::get_intra_predictor as pred;
use openh264_rs::safe::plane::PlaneCursorMut;

/// Sample sizes are cut hard under Miri, which runs ~100x slower and would otherwise
/// turn a phase-exit gate into an hour. The *shapes* are identical either way — every
/// stride, every boundary case — only the PRNG sample counts shrink, and the full-size
/// run happens on every `cargo test`.
fn scale(n: usize) -> usize {
    if cfg!(miri) { (n / 100).max(2) } else { n }
}

/// A destination surface of `stride`-byte rows filled with noise, plus a legal anchor
/// offset for a `bw` x `bh` block inside it.
///
/// Noise rather than a constant on purpose: a constant surface cannot show a write that
/// landed on the wrong row.
fn surface(rng: &mut Prng, stride: usize, bw: usize, bh: usize) -> (Vec<u8>, usize) {
    let rows = bh + 3;
    let buf = rng.bytes(rows * stride);
    let center = rng.below((rows - bh) as u32) as usize * stride
        + rng.below((stride - bw) as u32 + 1) as usize;
    (buf, center)
}

/// A noise surface with an intra-prediction block anchored inside it, padded the way a
/// real picture plane is: at least one row above and one column left of the block, and
/// enough room to the right of the block's top row for the diagonal modes' reach.
///
/// `top_reach` is how far past the block's left edge the row above is read — 8 for the
/// 4x4 modes (`DDL` reads `T4..T7`), 16 for the 8x8 and 16x16 ones. Anchoring at a
/// *random* legal position rather than a fixed one is what catches a kernel that
/// assumed its block was word-aligned, which several of these were written to be.
fn pred_surface(
    rng: &mut Prng,
    stride: usize,
    bw: usize,
    bh: usize,
    top_reach: usize,
) -> (Vec<u8>, usize) {
    let rows = bh + 3;
    let buf = rng.bytes(rows * stride);
    // Row >= 1 leaves the row above; column in 1..=stride-max(bw, top_reach) leaves the
    // column to the left and the top row's reach.
    let max_col = stride - bw.max(top_reach);
    let center = (1 + rng.below((rows - bh - 1) as u32) as usize) * stride
        + 1
        + rng.below(max_col as u32) as usize;
    (buf, center)
}

/// Strides for the intra-prediction families: the minimum that can hold the widest
/// read (the block plus its left column plus the diagonal reach), two non-multiples of
/// 16, and two real picture strides.
const STRIDES_PRED_4X4: [usize; 5] = [9, 21, 33, 240, 1952];
const STRIDES_PRED_8X8: [usize; 5] = [17, 21, 33, 240, 1952];

/// A sub-block whose only non-zero coefficient is its DC must still be transformed,
/// even though its non-zero count is zero.
///
/// This is the I16x16 luma DC case: those DC coefficients arrive through a separate
/// path and land in `pScaledTCoeff[k * 16]` after the NZC cache was filled, so
/// `IdctFourResAddPred_c`'s skip test is `nzc[n] != 0 || pRs[k << 4] != 0`, and the
/// second half of that disjunction is the whole reason 16x16 intra macroblocks decode
/// correctly. A conversion that dropped it would still pass every random-input
/// comparison in which some `nzc` happened to be non-zero, so it gets its own entry —
/// and because it states a property rather than an equivalence, it outlives the
/// family's shim commit.
#[test]
fn idct_four_res_add_pred_transforms_a_dc_only_block_with_zero_nzc() {
    let mut rng = Prng::new(0x2DC7_0004);

    for &stride in &[8usize, 21, 240] {
        for _ in 0..scale(50) {
            for k in 0..4usize {
                let (before, center) = surface(&mut rng, stride, 8, 8);
                let mut rs = [0i16; 64];
                rs[k << 4] = 64; // (32 + 64) >> 6 == 1 on every sample of that sub-block
                let nzc = [0i8; 6];

                let mut after = before.clone();
                dec_aux::idct_four_res_add_pred(
                    &mut PlaneCursorMut::new(&mut after, center, stride),
                    &rs,
                    &nzc,
                );

                // Exactly the 16 samples of sub-block k, each +1 with saturation, and
                // not one byte else in the surface.
                let bx = [0usize, 4, 0, 4][k];
                let by = [0usize, 0, 4, 4][k];
                let mut want = before.clone();
                for y in 0..4 {
                    for x in 0..4 {
                        let i = center + (by + y) * stride + bx + x;
                        want[i] = before[i].saturating_add(1);
                    }
                }
                assert_eq!(
                    after, want,
                    "DC-only sub-block {k} at stride {stride}, center {center}, seed {:#x}",
                    rng.seed()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// T3 — decoder/get_intra_predictor.rs
// ---------------------------------------------------------------------------

/// Every 4x4 luma intra-prediction mode, old kernel against new, over the whole
/// destination surface.
///
/// The surface check is doing real work here in a way it was not for the pilot: these
/// kernels write four rows through unaligned `u32` stores and read a neighbourhood that
/// extends past their own block, so an off-by-one in either direction lands on a byte
/// the block-shaped assertion would never look at.
#[test]
fn i4x4_luma_pred_modes_match_the_raw_kernels() {
    type Old = unsafe extern "C" fn(*mut u8, i32);
    type New = fn(&mut PlaneCursorMut<'_>);
    let modes: &[(&str, Old, New)] = &[
        ("V", pred::WelsI4x4LumaPredV_c, pred::i4x4_luma_pred_v),
        ("H", pred::WelsI4x4LumaPredH_c, pred::i4x4_luma_pred_h),
        ("Dc", pred::WelsI4x4LumaPredDc_c, pred::i4x4_luma_pred_dc),
        ("DcLeft", pred::WelsI4x4LumaPredDcLeft_c, pred::i4x4_luma_pred_dc_left),
        ("DcTop", pred::WelsI4x4LumaPredDcTop_c, pred::i4x4_luma_pred_dc_top),
        ("DcNA", pred::WelsI4x4LumaPredDcNA_c, pred::i4x4_luma_pred_dc_na),
        ("DDL", pred::WelsI4x4LumaPredDDL_c, pred::i4x4_luma_pred_ddl),
        ("DDLTop", pred::WelsI4x4LumaPredDDLTop_c, pred::i4x4_luma_pred_ddl_top),
        ("DDR", pred::WelsI4x4LumaPredDDR_c, pred::i4x4_luma_pred_ddr),
        ("VL", pred::WelsI4x4LumaPredVL_c, pred::i4x4_luma_pred_vl),
        ("VLTop", pred::WelsI4x4LumaPredVLTop_c, pred::i4x4_luma_pred_vl_top),
        ("VR", pred::WelsI4x4LumaPredVR_c, pred::i4x4_luma_pred_vr),
        ("HU", pred::WelsI4x4LumaPredHU_c, pred::i4x4_luma_pred_hu),
        ("HD", pred::WelsI4x4LumaPredHD_c, pred::i4x4_luma_pred_hd),
    ];

    let mut rng = Prng::new(0x2DC7_0010);
    for &stride in &STRIDES_PRED_4X4 {
        for (name, old_fn, new_fn) in modes {
            for _ in 0..scale(200) {
                let (before, center) = pred_surface(&mut rng, stride, 4, 4, 8);

                let mut old = before.clone();
                let mut new = before.clone();

                unsafe { old_fn(old.as_mut_ptr().add(center), stride as i32) };
                new_fn(&mut PlaneCursorMut::new(&mut new, center, stride));

                assert_eq!(
                    old, new,
                    "I4x4 {name} at stride {stride}, center {center}, seed {:#x}",
                    rng.seed()
                );
            }
        }
    }
}

/// Every 8x8 luma intra-prediction mode, both availability flags, old against new.
///
/// `bTLAvail`/`bTRAvail` are swept exhaustively rather than randomly: they select
/// different formulas for the first and last filtered neighbour, and three of the
/// fourteen kernels ignore `bTLAvail` entirely — a distinction a random sweep would
/// blur and a conversion could silently get wrong.
#[test]
fn i8x8_luma_pred_modes_match_the_raw_kernels() {
    type Old = unsafe extern "C" fn(*mut u8, i32, bool, bool);
    type New = fn(&mut PlaneCursorMut<'_>, bool, bool);
    let modes: &[(&str, Old, New)] = &[
        ("V", pred::WelsI8x8LumaPredV_c, pred::i8x8_luma_pred_v),
        ("H", pred::WelsI8x8LumaPredH_c, pred::i8x8_luma_pred_h),
        ("Dc", pred::WelsI8x8LumaPredDc_c, pred::i8x8_luma_pred_dc),
        ("DcLeft", pred::WelsI8x8LumaPredDcLeft_c, pred::i8x8_luma_pred_dc_left),
        ("DcTop", pred::WelsI8x8LumaPredDcTop_c, pred::i8x8_luma_pred_dc_top),
        ("DcNA", pred::WelsI8x8LumaPredDcNA_c, pred::i8x8_luma_pred_dc_na),
        ("DDL", pred::WelsI8x8LumaPredDDL_c, pred::i8x8_luma_pred_ddl),
        ("DDLTop", pred::WelsI8x8LumaPredDDLTop_c, pred::i8x8_luma_pred_ddl_top),
        ("DDR", pred::WelsI8x8LumaPredDDR_c, pred::i8x8_luma_pred_ddr),
        ("VL", pred::WelsI8x8LumaPredVL_c, pred::i8x8_luma_pred_vl),
        ("VLTop", pred::WelsI8x8LumaPredVLTop_c, pred::i8x8_luma_pred_vl_top),
        ("VR", pred::WelsI8x8LumaPredVR_c, pred::i8x8_luma_pred_vr),
        ("HU", pred::WelsI8x8LumaPredHU_c, pred::i8x8_luma_pred_hu),
        ("HD", pred::WelsI8x8LumaPredHD_c, pred::i8x8_luma_pred_hd),
    ];

    let mut rng = Prng::new(0x2DC7_0011);
    for &stride in &STRIDES_PRED_8X8 {
        for (name, old_fn, new_fn) in modes {
            for &tl in &[false, true] {
                for &tr in &[false, true] {
                    for _ in 0..scale(60) {
                        let (before, center) = pred_surface(&mut rng, stride, 8, 8, 16);

                        let mut old = before.clone();
                        let mut new = before.clone();

                        unsafe { old_fn(old.as_mut_ptr().add(center), stride as i32, tl, tr) };
                        new_fn(&mut PlaneCursorMut::new(&mut new, center, stride), tl, tr);

                        assert_eq!(
                            old, new,
                            "I8x8 {name} tl={tl} tr={tr} at stride {stride}, center {center}, \
                             seed {:#x}",
                            rng.seed()
                        );
                    }
                }
            }
        }
    }
}

/// Every 8x8 chroma and 16x16 luma mode, old against new.
#[test]
fn chroma_and_i16x16_pred_modes_match_the_raw_kernels() {
    type Old = unsafe extern "C" fn(*mut u8, i32);
    type New = fn(&mut PlaneCursorMut<'_>);

    let chroma: &[(&str, Old, New)] = &[
        ("V", pred::WelsIChromaPredV_c, pred::chroma_pred_v),
        ("H", pred::WelsIChromaPredH_c, pred::chroma_pred_h),
        ("Plane", pred::WelsIChromaPredPlane_c, pred::chroma_pred_plane),
        ("Dc", pred::WelsIChromaPredDc_c, pred::chroma_pred_dc),
        ("DcLeft", pred::WelsIChromaPredDcLeft_c, pred::chroma_pred_dc_left),
        ("DcTop", pred::WelsIChromaPredDcTop_c, pred::chroma_pred_dc_top),
        ("DcNA", pred::WelsIChromaPredDcNA_c, pred::chroma_pred_dc_na),
    ];
    let luma16: &[(&str, Old, New)] = &[
        ("V", pred::WelsI16x16LumaPredV_c, pred::i16x16_luma_pred_v),
        ("H", pred::WelsI16x16LumaPredH_c, pred::i16x16_luma_pred_h),
        ("Plane", pred::WelsI16x16LumaPredPlane_c, pred::i16x16_luma_pred_plane),
        ("Dc", pred::WelsI16x16LumaPredDc_c, pred::i16x16_luma_pred_dc),
        ("DcTop", pred::WelsI16x16LumaPredDcTop_c, pred::i16x16_luma_pred_dc_top),
        ("DcLeft", pred::WelsI16x16LumaPredDcLeft_c, pred::i16x16_luma_pred_dc_left),
        ("DcNA", pred::WelsI16x16LumaPredDcNA_c, pred::i16x16_luma_pred_dc_na),
    ];

    let mut rng = Prng::new(0x2DC7_0012);
    for &(label, modes, size) in &[("chroma", chroma, 8usize), ("i16x16", luma16, 16usize)] {
        for &stride in &STRIDES_PRED_8X8 {
            if stride < size + 1 {
                continue;
            }
            for (name, old_fn, new_fn) in modes {
                for _ in 0..scale(120) {
                    let (before, center) = pred_surface(&mut rng, stride, size, size, size);

                    let mut old = before.clone();
                    let mut new = before.clone();

                    unsafe { old_fn(old.as_mut_ptr().add(center), stride as i32) };
                    new_fn(&mut PlaneCursorMut::new(&mut new, center, stride));

                    assert_eq!(
                        old, new,
                        "{label} {name} at stride {stride}, center {center}, seed {:#x}",
                        rng.seed()
                    );
                }
            }
        }
    }
}
