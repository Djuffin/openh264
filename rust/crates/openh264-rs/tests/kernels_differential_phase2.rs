//! Differential tests for the Phase 2 kernel conversions (plan §Phase 2, recipe R2).
//!
//! Each entry drives the **old** raw-pointer kernel and the **new** safe one over the
//! same random inputs and asserts they agree bit for bit — not on the nominal block,
//! but on **every byte of the destination surface**. Comparing whole buffers is the
//! point: a kernel that writes one byte past its block is finding F1's defect class,
//! and a block-shaped assertion is blind to exactly that.
//!
//! Entries here are **deliberately short-lived**. A family's commit A adds them while
//! the old code is untouched; the family's commit B replaces the old body with a shim
//! onto the safe kernel, at which point the comparison is tautological and the entry
//! is deleted in that same commit. What survives past a family's conversion is only
//! tests that pin a *property* rather than an equivalence.
//!
//! This file lives outside `src/`, so unlike everything under `src/safe/` it may use
//! `unsafe` — it has to, because the old side *is* raw-pointer code. Running it under
//! Miri therefore also checks those raw accesses for UB, which is how F7 was found
//! (`rust/docs/phase1_findings.md`).

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

/// Strides these kernels must survive.
///
/// `4`/`8` are the minimum legal stride for the 4x4 and 8x8 kernels — a surface
/// exactly as wide as the block. `17`, `21` and `33` are not multiples of 16, which is
/// what catches an implementation that assumed a row is a whole number of words. `240`
/// is a real QCIF luma stride and `1952` a real 1080p one, both as `AllocPicture`
/// computes them.
const STRIDES_4X4: [usize; 6] = [4, 17, 21, 33, 240, 1952];
const STRIDES_8X8: [usize; 6] = [8, 17, 21, 33, 240, 1952];

/// A residual block of `n` coefficients bounded by `±bound`.
///
/// The bound is a per-kernel decision, not a taste one — see [`BOUND_4X4`] and
/// [`BOUND_8X8`].
fn residual(rng: &mut Prng, n: usize, bound: i32) -> Vec<i16> {
    (0..n).map(|_| rng.range_i32(-bound, bound) as i16).collect()
}

/// The 4x4 IDCT is exercised at the full range of `i16`.
///
/// It does its arithmetic in `i32` and truncates with an explicit `as i16`
/// (`iSrc[kiY] = (kiT0 + kiT3) as i16`), so it cannot overflow, and the truncation is
/// exactly the behaviour that has to be preserved. Small coefficients would never
/// reach it.
const BOUND_4X4: i32 = i16::MAX as i32;

/// The 8x8 IDCT is exercised only up to ±512, and that is a **property of the code
/// under test**, not a weakening of the test.
///
/// Unlike the 4x4, its two 1-D passes are `i16`-native (`a[0] = p[0] + p[4]` over an
/// `int16_t[8]`), so large coefficients overflow the intermediates. Worst-case gain is
/// 7.875x per pass — `b[1] = a[0] + (a[3] >> 2)` with `a` up to 3.5x the input, and
/// `iTmp = b[0] + b[7]` summing a 3.5x and a 4.375x term — so ~62x over both passes,
/// and `32767 / 62 ≈ 528`. Above that the *old* kernel panics with "attempt to add
/// with overflow" in a debug build, where the C++ has signed-overflow UB and in
/// practice wraps. That divergence is pre-existing, is not this phase's to fix, and is
/// written up as `rust/docs/phase2_findings.md` §F8; a differential test run above the
/// bound would be measuring the panic rather than the kernel.
const BOUND_8X8: i32 = 512;

/// A destination surface of `rows` rows of `stride` bytes, filled with noise, plus a
/// legal anchor offset for a `bw` x `bh` block inside it.
///
/// Noise rather than a constant on purpose: a constant surface cannot show a write
/// that landed on the wrong row.
fn surface(rng: &mut Prng, stride: usize, bw: usize, bh: usize) -> (Vec<u8>, usize) {
    let rows = bh + 3;
    let buf = rng.bytes(rows * stride);
    // Anchors so that the whole block fits: the kernels reach [0, (bh-1)*stride + bw).
    let max_row = rows - bh;
    let max_col = stride - bw;
    let center = rng.below(max_row as u32) as usize * stride + rng.below(max_col as u32 + 1) as usize;
    (buf, center)
}

#[test]
fn idct_res_add_pred_matches_the_raw_kernel() {
    let mut rng = Prng::new(0x2DC7_0001);

    for &stride in &STRIDES_4X4 {
        for i in 0..scale(400) {
            let (buf, center) = surface(&mut rng, stride, 4, 4);
            let rs: [i16; 16] =
                residual(&mut rng, 16, if i % 3 == 0 { BOUND_4X4 } else { 64 })
                    .try_into()
                    .unwrap();

            let mut old = buf.clone();
            let mut new = buf.clone();

            let mut rs_old = rs;
            unsafe {
                dec_aux::IdctResAddPred_c(
                    old.as_mut_ptr().add(center),
                    stride as i32,
                    rs_old.as_mut_ptr(),
                );
            }
            dec_aux::idct_res_add_pred(&mut PlaneCursorMut::new(&mut new, center, stride), &rs);

            assert_eq!(
                rs_old, rs,
                "the old kernel modified its residual (stride {stride}, seed {:#x})",
                rng.seed()
            );
            assert_eq!(
                old,
                new,
                "whole-surface mismatch at stride {stride}, center {center}, seed {:#x}",
                rng.seed()
            );
        }
    }
}

#[test]
fn idct_res_add_pred8x8_matches_the_raw_kernel() {
    let mut rng = Prng::new(0x2DC7_0002);

    for &stride in &STRIDES_8X8 {
        for i in 0..scale(300) {
            let (buf, center) = surface(&mut rng, stride, 8, 8);
            let rs: [i16; 64] =
                residual(&mut rng, 64, if i % 3 == 0 { BOUND_8X8 } else { 64 })
                    .try_into()
                    .unwrap();

            let mut old = buf.clone();
            let mut new = buf.clone();

            let mut rs_old = rs;
            unsafe {
                dec_aux::IdctResAddPred8x8_c(
                    old.as_mut_ptr().add(center),
                    stride as i32,
                    rs_old.as_mut_ptr(),
                );
            }
            dec_aux::idct_res_add_pred8x8(&mut PlaneCursorMut::new(&mut new, center, stride), &rs);

            assert_eq!(rs_old, rs, "the old 8x8 kernel modified its residual");
            assert_eq!(
                old,
                new,
                "whole-surface mismatch at stride {stride}, center {center}, seed {:#x}",
                rng.seed()
            );
        }
    }
}

#[test]
fn idct_four_res_add_pred_matches_the_raw_kernel() {
    let mut rng = Prng::new(0x2DC7_0003);

    for &stride in &STRIDES_8X8 {
        for i in 0..scale(300) {
            let (buf, center) = surface(&mut rng, stride, 8, 8);
            let rs: [i16; 64] =
                residual(&mut rng, 64, if i % 3 == 0 { BOUND_4X4 } else { 64 })
                    .try_into()
                    .unwrap();

            // The interesting axis is the skip decision, so most windows are mostly
            // zero: a kernel that transformed a block it should have skipped, or
            // skipped one it should have transformed, only shows up here.
            let nzc: [i8; 6] = std::array::from_fn(|_| {
                if rng.below(3) == 0 { rng.next_u8() as i8 } else { 0 }
            });

            let mut old = buf.clone();
            let mut new = buf.clone();

            let mut rs_old = rs;
            unsafe {
                dec_aux::IdctFourResAddPred_c(
                    old.as_mut_ptr().add(center),
                    stride as i32,
                    rs_old.as_mut_ptr(),
                    nzc.as_ptr(),
                );
            }
            dec_aux::idct_four_res_add_pred(
                &mut PlaneCursorMut::new(&mut new, center, stride),
                &rs,
                &nzc,
            );

            assert_eq!(rs_old, rs, "the old four-block kernel modified its residual");
            assert_eq!(
                old,
                new,
                "whole-surface mismatch at stride {stride}, center {center}, nzc {nzc:?}, \
                 seed {:#x}",
                rng.seed()
            );
        }
    }
}

/// The DC-only path deserves its own entry rather than being left to chance: a block
/// with `nzc == 0` and a non-zero DC coefficient must still be transformed (the
/// I16x16 luma DC case), and the random `nzc` above would hit it only occasionally.
#[test]
fn idct_four_res_add_pred_transforms_a_dc_only_block_with_zero_nzc() {
    let mut rng = Prng::new(0x2DC7_0004);
    let stride = 240usize;

    for k in 0..4 {
        let (buf, center) = surface(&mut rng, stride, 8, 8);
        let mut rs = [0i16; 64];
        rs[k << 4] = 64; // (32 + 64) >> 6 == 1 added to every sample of that sub-block
        let nzc = [0i8; 6];

        let mut old = buf.clone();
        let mut new = buf.clone();

        let mut rs_old = rs;
        unsafe {
            dec_aux::IdctFourResAddPred_c(
                old.as_mut_ptr().add(center),
                stride as i32,
                rs_old.as_mut_ptr(),
                nzc.as_ptr(),
            );
        }
        dec_aux::idct_four_res_add_pred(
            &mut PlaneCursorMut::new(&mut new, center, stride),
            &rs,
            &nzc,
        );

        assert_ne!(old, buf, "sub-block {k} was skipped, so nothing was transformed");
        assert_eq!(old, new, "DC-only sub-block {k}");
    }
}

#[test]
fn i4_luma_ichroma_addr_table_matches_the_raw_kernel() {
    let mut rng = Prng::new(0x2DC7_0005);

    // Real luma/chroma stride pairs, then random ones.
    let mut pairs: Vec<(i32, i32)> = vec![(240, 120), (352, 176), (1952, 976), (32, 16)];
    for _ in 0..scale(200) {
        let y = rng.range_i32(16, 4096);
        pairs.push((y, y / 2));
    }

    for (stride_y, stride_uv) in pairs {
        let mut old = [0i32; 24];
        let mut new = [0i32; 24];
        unsafe {
            dec_aux::GetI4LumaIChromaAddrTable(old.as_mut_ptr(), stride_y, stride_uv);
        }
        dec_aux::i4_luma_ichroma_addr_table(&mut new, stride_y, stride_uv);
        assert_eq!(old, new, "strides ({stride_y}, {stride_uv})");
    }
}
