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

// ===========================================================================
// T4 — `common/mc.rs`, motion compensation
// ===========================================================================
//
// `46053993` proved all 24 safe kernels against the raw ones — every luma block
// shape plus the encoder's `+1` half-pel shapes, every chroma shape, three strides
// per surface, random unaligned anchors in both, **exhaustive** sweeps of the
// selectors (all 16 `(iMvX & 3, iMvY & 3)` pairs for `McLuma_c`, all 64 `(& 7)`
// pairs for the chroma pair, each from a positive and a negative vector), and every
// byte of the destination compared rather than the nominal block. The shim commit
// then deleted those equivalences, because both sides now run the same code.
//
// What survives is the one thing the shims add that the kernels do not: **span
// arithmetic**. Each shim turns `pSrc` and `pDst` into slices whose lengths it
// derives from the kernel's read reach, and its `# Safety` contract states that
// derivation as the caller's obligation. The test below is the assertion that the
// contract is neither too small nor too large.

use openh264_rs::common::mc;

/// `(left, top, right, bottom)`: the kernel reads `x` in `-left .. width + right`
/// and `y` in `-top .. height + bottom`, relative to `pSrc`. An independent
/// restatement of `mc.rs`'s private `Reach` — if the two ever disagree, this test
/// fails, which is the point of restating it rather than exporting it.
type Reach = (usize, usize, usize, usize);

const R_COPY: Reach = (0, 0, 0, 0);
const R_HOR: Reach = (2, 0, 3, 0);
const R_VER: Reach = (0, 2, 0, 3);
const R_CEN: Reach = (2, 2, 3, 3);
const R_CHROMA: Reach = (0, 0, 1, 1);

/// Luma MC block shapes, plus the encoder's `+1` half-pel refinement shapes
/// (`encoder/md.rs:1196,1229,1289`).
const HALFPEL_SIZES: &[(usize, usize)] = &[(16, 16), (16, 8), (8, 4), (4, 4), (17, 17), (2, 2)];

/// The quarter-pel composites interpolate through a `[u8; 256]` at stride 16, so 16
/// is the ceiling in both dimensions — the same ceiling the C++ has.
const QPEL_SIZES: &[(usize, usize)] = &[(16, 16), (16, 8), (8, 16), (8, 4), (4, 4), (2, 2)];

const CHROMA_SIZES: &[(usize, usize)] = &[(8, 8), (8, 4), (4, 2), (2, 2)];

/// `McCopy_c` copies two bytes for any width that is not 16, 8 or 4, so the list has
/// to contain widths that are none of those — the span must narrow with it.
const COPY_SIZES: &[(usize, usize)] = &[(16, 16), (8, 8), (4, 4), (2, 2), (6, 4), (3, 2)];

/// The width `McCopy_c` actually touches — `mc.rs`'s `copy_width`, restated.
fn copy_width(width: usize) -> usize {
    match width {
        16 => 16,
        8 => 8,
        4 => 4,
        _ => 2,
    }
}

/// Strides worth driving: the minimum legal one, where an off-by-one in the span
/// arithmetic shows up first, and two larger ones.
///
/// Under Miri only the minimum survives — see [`sizes`] for why the cut falls here
/// and not on the selector sweeps.
fn strides(min: usize) -> Vec<usize> {
    let min = min.max(1);
    if cfg!(miri) {
        return vec![min];
    }
    let mut v: Vec<usize> = [min, min + 7, 32, 240].into_iter().filter(|&s| s >= min).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Block shapes, cut to two under Miri.
///
/// Miri is the only instrument that sees the *over*-reach half of [`probe_span`], so
/// it has to run — but at ~100x it cannot run everything. The cut falls on the stride
/// and shape sweeps and **not** on the selector sweeps: an MV chooses which reach a
/// shim declares, so those stay exhaustive under Miri too. Shapes and strides only
/// re-run the same span arithmetic with different numbers.
fn sizes(all: &'static [(usize, usize)]) -> &'static [(usize, usize)] {
    if cfg!(miri) { &all[..2] } else { all }
}

/// `(slice length, offset of `pSrc` within it)` — `mc.rs`'s `src_span`, restated.
fn src_span(stride: usize, width: usize, height: usize, r: Reach) -> (usize, usize) {
    let (left, top, right, bottom) = r;
    let center = top * stride + left;
    (center + (height + bottom - 1) * stride + width + right, center)
}

/// Bytes spanned by a `width` x `height` block at `stride` — `mc.rs`'s `block_span`.
fn block_span(stride: usize, width: usize, height: usize) -> usize {
    (height - 1) * stride + width
}

/// Runs one shim against source and destination allocations sized to **exactly** the
/// span its contract declares, and asserts two things at once.
///
/// * **The declared span is not too large.** The source buffer is `src_span` bytes
///   and not one more, so a shim that materialises a longer slice from a pointer into
///   it is constructing a reference to memory it does not own — undefined behaviour,
///   which Miri reports at the `from_raw_parts` rather than as a wrong pixel. This is
///   the direction no output comparison can see, because reading a byte past the end
///   of a generously-sized test buffer produces perfectly plausible pixels.
/// * **The declared span is not too small.** If it is, the safe kernel indexes past
///   its slice and panics, in any build, on the first call.
///
/// The assertion on top is a property worth keeping in its own right: running the
/// same call into two differently-noised destinations must produce identical blocks,
/// i.e. the kernel **writes every byte of its block and reads none of them**. A
/// kernel that skipped a sample would leave the two destinations' noise showing.
fn probe_span(
    name: &str,
    rng: &mut Prng,
    reach: Reach,
    span_width: usize,
    h: usize,
    ss: usize,
    ds: usize,
    run: impl Fn(*const u8, i32, *mut u8, i32),
) {
    let (slen, sc) = src_span(ss, span_width, h, reach);
    let src = rng.bytes(slen);
    let mut d1 = rng.bytes(block_span(ds, span_width, h));
    let mut d2 = rng.bytes(block_span(ds, span_width, h));
    unsafe {
        run(src.as_ptr().add(sc), ss as i32, d1.as_mut_ptr(), ds as i32);
        run(src.as_ptr().add(sc), ss as i32, d2.as_mut_ptr(), ds as i32);
    }
    for y in 0..h {
        assert_eq!(
            &d1[y * ds..][..span_width],
            &d2[y * ds..][..span_width],
            "{name}: row {y} of the destination block still shows what was there \
             before the call — the kernel did not write every byte of its block \
             (src_stride {ss}, dst_stride {ds}, seed {:#x})",
            rng.seed()
        );
    }
}

/// Every `common/mc.rs` shim, against allocations sized to exactly the span it
/// declares. See [`probe_span`] for what each call proves; run this file under Miri
/// for the over-reach half of it.
#[test]
fn mc_shims_stay_inside_the_spans_they_declare() {
    type Wh = unsafe extern "C" fn(*const u8, i32, *mut u8, i32, i32, i32);
    let wh: &[(&str, Wh, Reach, &[(usize, usize)])] = &[
        ("McCopy_c", mc::McCopy_c, R_COPY, COPY_SIZES),
        ("McHorVer20_c", mc::McHorVer20_c, R_HOR, HALFPEL_SIZES),
        ("McHorVer02_c", mc::McHorVer02_c, R_VER, HALFPEL_SIZES),
        ("McHorVer22_c", mc::McHorVer22_c, R_CEN, HALFPEL_SIZES),
        ("McHorizLuma_c", mc::McHorizLuma_c, R_HOR, HALFPEL_SIZES),
        ("McVertLuma_c", mc::McVertLuma_c, R_VER, HALFPEL_SIZES),
        ("McHorVer01_c", mc::McHorVer01_c, R_VER, QPEL_SIZES),
        ("McHorVer03_c", mc::McHorVer03_c, R_VER, QPEL_SIZES),
        ("McHorVer10_c", mc::McHorVer10_c, R_HOR, QPEL_SIZES),
        ("McHorVer11_c", mc::McHorVer11_c, R_CEN, QPEL_SIZES),
        ("McHorVer12_c", mc::McHorVer12_c, R_CEN, QPEL_SIZES),
        ("McHorVer13_c", mc::McHorVer13_c, R_CEN, QPEL_SIZES),
        ("McHorVer21_c", mc::McHorVer21_c, R_CEN, QPEL_SIZES),
        ("McHorVer23_c", mc::McHorVer23_c, R_CEN, QPEL_SIZES),
        ("McHorVer30_c", mc::McHorVer30_c, R_HOR, QPEL_SIZES),
        ("McHorVer31_c", mc::McHorVer31_c, R_CEN, QPEL_SIZES),
        ("McHorVer32_c", mc::McHorVer32_c, R_CEN, QPEL_SIZES),
        ("McHorVer33_c", mc::McHorVer33_c, R_CEN, QPEL_SIZES),
    ];
    let mut rng = Prng::new(0x4C40_0500);
    for &(name, f, reach, sizes_of) in wh {
        for &(w, h) in sizes(sizes_of) {
            // Only the copy path narrows; everything else spans its nominal width.
            let sw = if name == "McCopy_c" { copy_width(w) } else { w };
            for &ss in &strides(reach.0 + sw + reach.2) {
                for &ds in &strides(sw) {
                    probe_span(name, &mut rng, reach, sw, h, ss, ds, |s, ssz, d, dsz| unsafe {
                        f(s, ssz, d, dsz, w as i32, h as i32)
                    });
                }
            }
        }
    }

    type Fixed = unsafe extern "C" fn(*const u8, i32, *mut u8, i32, i32);
    let fixed: &[(&str, Fixed, usize)] = &[
        ("McCopyWidthEq2_c", mc::McCopyWidthEq2_c, 2),
        ("McCopyWidthEq4_c", mc::McCopyWidthEq4_c, 4),
        ("McCopyWidthEq8_c", mc::McCopyWidthEq8_c, 8),
        ("McCopyWidthEq16_c", mc::McCopyWidthEq16_c, 16),
    ];
    for &(name, f, w) in fixed {
        for &h in &[1usize, 4, 16, 17] {
            for &ss in &strides(w) {
                for &ds in &strides(w) {
                    probe_span(name, &mut rng, R_COPY, w, h, ss, ds, |s, ssz, d, dsz| unsafe {
                        f(s, ssz, d, dsz, h as i32)
                    });
                }
            }
        }
    }

    // Both MV-dispatching entry points, over every selector value: the reach — and so
    // the span — is chosen by the vector, which is what makes an exhaustive sweep the
    // only honest one here.
    for &(w, h) in sizes(QPEL_SIZES) {
        for phase in 0..16i16 {
            let (mvx, mvy) = (phase & 3, phase >> 2);
            let reach = match (mvx, mvy) {
                (0, 0) => R_COPY,
                (0, _) => R_VER,
                (_, 0) => R_HOR,
                _ => R_CEN,
            };
            let sw = if (mvx, mvy) == (0, 0) { copy_width(w) } else { w };
            for &ss in &strides(reach.0 + sw + reach.2) {
                for &ds in &strides(sw) {
                    probe_span("McLuma_c", &mut rng, reach, sw, h, ss, ds, |s, ssz, d, dsz| unsafe {
                        mc::McLuma_c(s, ssz, d, dsz, mvx, mvy, w as i32, h as i32)
                    });
                }
            }
        }
    }

    for &(w, h) in sizes(CHROMA_SIZES) {
        for phase in 0..64i16 {
            let (mvx, mvy) = (phase & 7, phase >> 3);
            let frag = mvx != 0 || mvy != 0;
            let (reach, sw) = if frag { (R_CHROMA, w) } else { (R_COPY, copy_width(w)) };
            for &ss in &strides(sw + reach.2) {
                for &ds in &strides(sw) {
                    probe_span("McChroma_c", &mut rng, reach, sw, h, ss, ds, |s, ssz, d, dsz| unsafe {
                        mc::McChroma_c(s, ssz, d, dsz, mvx, mvy, w as i32, h as i32)
                    });
                    probe_span(
                        "McChromaWithFragMv_c",
                        &mut rng,
                        R_CHROMA,
                        w,
                        h,
                        ss.max(w + 1),
                        ds,
                        |s, ssz, d, dsz| unsafe {
                            mc::McChromaWithFragMv_c(s, ssz, d, dsz, mvx, mvy, w as i32, h as i32)
                        },
                    );
                }
            }
        }
    }

    // Three surfaces, three independent strides — the encoder averages a
    // `ME_REFINE_BUF_STRIDE` scratch against the reference picture, so the strides
    // really do differ at a live call site (`encoder/md.rs:1059`).
    for &(w, h) in sizes(HALFPEL_SIZES) {
        for &sa in &strides(w) {
            for &sb in &strides(w) {
                for &ds in &strides(w) {
                    let other = rng.bytes(block_span(sb, w, h));
                    probe_span("PixelAvg_c", &mut rng, R_COPY, w, h, sa, ds, |s, ssz, d, dsz| unsafe {
                        mc::PixelAvg_c(d, dsz, s, ssz, other.as_ptr(), sb as i32, w as i32, h as i32)
                    });
                }
            }
        }
    }
}
