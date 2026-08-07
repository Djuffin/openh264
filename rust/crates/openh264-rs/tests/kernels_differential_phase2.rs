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
