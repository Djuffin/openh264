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


// ===========================================================================
// T5 — `common/sad_common.rs` + `common/intra_pred_common.rs`
// ===========================================================================
//
// **This family is half-landed.** `intra_pred_common`'s two predictors are behind
// shims; `sad_common`'s fourteen SAD kernels were swapped and then **unswapped**,
// because the swap cost the encoder +16.8% median and breached §7.4's 10% ceiling
// (`perf_baseline.md` §Phase 2 T5). So the SAD entries below are commit-A entries
// again — a live differential against raw code that is still the code that runs —
// while the intra entries are shim properties. When the SAD swap re-lands, the
// differential goes and the span property stays.
//
// `56a3dbf9` proved all sixteen safe kernels against the raw ones — every SAD shape,
// every stride pair from the minimum legal to 240, random unaligned anchors on both
// surfaces, and a real independently-noised margin around the four-point kernels'
// blocks so their arms read neighbours rather than the block's own edge. Five
// mutations were run against those entries and all five died. The shim commit deleted
// them, because both sides now run the same code.
//
// What survives is what the shims add: **span arithmetic**, and the count of `int32_t`
// the four-point kernels write. Two of the four shim shapes here have a reach the
// pointer does not sit inside — `WelsI16x16LumaPredV_c` reads *only* above `pRef` —
// which is a shape T4 never had, and it is the one that most needs pinning.

use openh264_rs::common::intra_pred_common as ipc;
use openh264_rs::common::sad_common as sad;
use openh264_rs::safe::plane::PlaneCursor;

/// A noise surface with at least `pad` rows and columns of margin around a `w` x `h`
/// block, and a random legal anchor for it. The anchor is random so the block lands
/// unaligned: these kernels run on every partition of every macroblock at every search
/// position, so alignment is not something a caller can promise.
fn sad_surface(
    rng: &mut Prng,
    stride: usize,
    w: usize,
    h: usize,
    pad: usize,
) -> (Vec<u8>, usize) {
    assert!(stride >= w + 2 * pad);
    let rows = h + 2 * pad + 2;
    let buf = rng.bytes(rows * stride);
    let y = pad + rng.below((rows - h - 2 * pad) as u32) as usize;
    let x = pad + rng.below((stride - w - 2 * pad) as u32 + 1) as usize;
    (buf, y * stride + x)
}

/// Dispatches the const-generic safe kernel for a runtime shape, so the tables below
/// can stay tables. Every arm is also the assertion that the instantiation is wired to
/// the shape its raw counterpart claims.
fn safe_sad(w: usize, h: usize, c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    match (w, h) {
        (4, 4) => sad::sample_sad::<4, 4>(c1, c2),
        (8, 4) => sad::sample_sad::<8, 4>(c1, c2),
        (4, 8) => sad::sample_sad::<4, 8>(c1, c2),
        (8, 8) => sad::sample_sad::<8, 8>(c1, c2),
        (16, 8) => sad::sample_sad::<16, 8>(c1, c2),
        (8, 16) => sad::sample_sad::<8, 16>(c1, c2),
        _ => sad::sample_sad::<16, 16>(c1, c2),
    }
}

fn safe_sad_four(w: usize, h: usize, c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>, out: &mut [i32; 4]) {
    match (w, h) {
        (4, 4) => sad::sample_sad_four::<4, 4>(c1, c2, out),
        (8, 4) => sad::sample_sad_four::<8, 4>(c1, c2, out),
        (4, 8) => sad::sample_sad_four::<4, 8>(c1, c2, out),
        (8, 8) => sad::sample_sad_four::<8, 8>(c1, c2, out),
        (16, 8) => sad::sample_sad_four::<16, 8>(c1, c2, out),
        (8, 16) => sad::sample_sad_four::<8, 16>(c1, c2, out),
        _ => sad::sample_sad_four::<16, 16>(c1, c2, out),
    }
}

/// The safe SAD kernels against the raw ones they will replace once the swap can hold
/// the performance budget. Live again after the unswap — while `sad_common.rs` runs
/// raw code, this is what keeps the safe kernels honest.
#[test]
fn sad_kernels_match_the_raw_ones() {
    let mut rng = Prng::new(0x5AD0_0500);

    for &(name, raw, w, h) in SAD_SHIMS {
        for &s1 in &strides(w) {
            for &s2 in &strides(w) {
                for _ in 0..scale(40) {
                    let (mut b1, c1) = sad_surface(&mut rng, s1, w, h, 0);
                    let (mut b2, c2) = sad_surface(&mut rng, s2, w, h, 0);
                    let old = unsafe {
                        raw(b1.as_mut_ptr().add(c1), s1 as i32, b2.as_mut_ptr().add(c2), s2 as i32)
                    };
                    let new = safe_sad(
                        w,
                        h,
                        &PlaneCursor::new(&b1, c1, s1),
                        &PlaneCursor::new(&b2, c2, s2),
                    );
                    assert_eq!(old, new, "{name}: strides {s1}/{s2}, seed {:#x}", rng.seed());
                }
            }
        }
    }

    // `pad = 1` on the second surface is exactly the four-point reach, so the noise the
    // diamond's arms read is real data that differs from the block's own edge.
    for &(name, raw, w, h) in FOUR_SHIMS {
        for &s1 in &strides(w) {
            for &s2 in &strides(w + 2) {
                for _ in 0..scale(40) {
                    let (mut b1, c1) = sad_surface(&mut rng, s1, w, h, 0);
                    let (mut b2, c2) = sad_surface(&mut rng, s2, w, h, 1);
                    let mut old = [0i32; 4];
                    unsafe {
                        raw(
                            b1.as_mut_ptr().add(c1),
                            s1 as i32,
                            b2.as_mut_ptr().add(c2),
                            s2 as i32,
                            old.as_mut_ptr(),
                        )
                    };
                    let mut new = [0i32; 4];
                    safe_sad_four(
                        w,
                        h,
                        &PlaneCursor::new(&b1, c1, s1),
                        &PlaneCursor::new(&b2, c2, s2),
                        &mut new,
                    );
                    assert_eq!(
                        old, new,
                        "{name}: strides {s1}/{s2}, seed {:#x} (up, down, left, right)",
                        rng.seed()
                    );
                }
            }
        }
    }
}

type RawSad = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32) -> i32;
type RawFour = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32, *mut i32);

/// Named `_SHIMS` because that is what these entry points are when the swap is
/// landed; right now the SAD half is unswapped and they are the raw kernels. Both
/// readings are true of the spans, which is why the property survives the unswap.
const SAD_SHIMS: &[(&str, RawSad, usize, usize)] = &[
    ("Sad4x4", sad::WelsSampleSad4x4_c, 4, 4),
    ("Sad8x4", sad::WelsSampleSad8x4_c, 8, 4),
    ("Sad4x8", sad::WelsSampleSad4x8_c, 4, 8),
    ("Sad8x8", sad::WelsSampleSad8x8_c, 8, 8),
    ("Sad16x8", sad::WelsSampleSad16x8_c, 16, 8),
    ("Sad8x16", sad::WelsSampleSad8x16_c, 8, 16),
    ("Sad16x16", sad::WelsSampleSad16x16_c, 16, 16),
];

const FOUR_SHIMS: &[(&str, RawFour, usize, usize)] = &[
    ("SadFour4x4", sad::WelsSampleSadFour4x4_c, 4, 4),
    ("SadFour8x4", sad::WelsSampleSadFour8x4_c, 8, 4),
    ("SadFour4x8", sad::WelsSampleSadFour4x8_c, 4, 8),
    ("SadFour8x8", sad::WelsSampleSadFour8x8_c, 8, 8),
    ("SadFour16x8", sad::WelsSampleSadFour16x8_c, 16, 8),
    ("SadFour8x16", sad::WelsSampleSadFour8x16_c, 8, 16),
    ("SadFour16x16", sad::WelsSampleSadFour16x16_c, 16, 16),
];

/// Every `common/sad_common.rs` shim, against allocations sized to **exactly** the
/// span it declares.
///
/// Too small a span and the safe kernel indexes past its slice and panics, in any
/// build, on the first call. Too large and the shim materialises a reference to bytes
/// it does not own — undefined behaviour that produces perfectly plausible pixels and
/// is visible only to Miri, at the `from_raw_parts`. Run this file under Miri for that
/// half; `cargo test` gets the other.
#[test]
fn sad_shims_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0x5AD0_0501);

    // PARKED-FAMILY BUFFER SIZES (F10). While T5-sad is unswapped these Wels
    // names are the *raw* kernels, whose row loop bumps its pointers one stride
    // past the last row after the final iteration (`pSrc = pSrc.offset(iStride)`,
    // inherited from the C++'s `pSrc += iStride`). On a buffer that ends at the
    // block's last row that bump computes an out-of-allocation pointer — UB that
    // Miri reports even though nothing dereferences it (`phase2_findings.md`
    // F10). The buffers below are therefore sized to the raw kernels'
    // pointer-arithmetic footprint — whole rows plus the composite sub-block
    // offset, and two extra rows on the four-point reference side for the
    // down/right arms. **The shim-contract spans are `(h-1)*stride + w` and
    // `(h+1)*stride + w`: restore them in the commit that re-lands the swap**,
    // because the safe kernels never compute past their span and the exact size
    // is what makes an over-claiming shim Miri-visible.
    for &(name, f, w, h) in SAD_SHIMS {
        for &s1 in &strides(w) {
            for &s2 in &strides(w) {
                let mut b1 = rng.bytes(h * s1 + w);
                let mut b2 = rng.bytes(h * s2 + w);
                let got =
                    unsafe { f(b1.as_mut_ptr(), s1 as i32, b2.as_mut_ptr(), s2 as i32) };
                // A SAD is bounded by its own block; anything else means the kernel
                // summed samples it should not have reached.
                assert!(
                    (0..=(w * h * 255) as i32).contains(&got),
                    "{name}: {got} is outside [0, {}] at strides {s1}/{s2}",
                    w * h * 255
                );
            }
        }
    }

    for &(name, f, w, h) in FOUR_SHIMS {
        for &s1 in &strides(w) {
            for &s2 in &strides(w + 2) {
                let mut b1 = rng.bytes(h * s1 + w);
                let mut b2 = rng.bytes((h + 2) * s2 + w);
                // Eight slots, of which exactly four may be written. The back half is
                // the assertion that the shim's `[i32; 4]` really is four: a kernel
                // handed a bare `int32_t*` has nothing else stopping it at four, and
                // `pSad` at every live call site is a `[0i32; 4]` on the stack
                // (`svc_motion_estimate.rs:871`) with the caller's locals behind it.
                let mut sads = [0i32; 8];
                for (i, s) in sads.iter_mut().enumerate() {
                    *s = -(i as i32) - 1;
                }
                let guard = sads[4..].to_vec();
                unsafe {
                    f(
                        b1.as_mut_ptr(),
                        s1 as i32,
                        b2.as_mut_ptr().add(s2),
                        s2 as i32,
                        sads.as_mut_ptr(),
                    )
                };
                assert_eq!(
                    &sads[4..],
                    &guard[..],
                    "{name}: wrote past the four results it was given, at strides {s1}/{s2}"
                );
                for (k, &v) in sads[..4].iter().enumerate() {
                    assert!(
                        (0..=(w * h * 255) as i32).contains(&v),
                        "{name}: arm {k} scored {v}, outside [0, {}]",
                        w * h * 255
                    );
                }
                // The four arms are four *distinct* points. Equivalence against the
                // raw kernel could never have shown a shared misreading of the offsets
                // — the raw kernel was the reference, not an oracle — so this is the
                // entry that outlives it.
                assert!(
                    sads[..4].iter().collect::<std::collections::HashSet<_>>().len() > 1,
                    "{name}: all four search points scored identically on noise, so they \
                     are not four distinct points (strides {s1}/{s2}, seed {:#x})",
                    rng.seed()
                );

                // And they are the four *right* points. Sizing the span correctly does
                // not pin where it starts: a shim that built its slice from `pSample2`
                // instead of one row above it, and left the cursor anchored at
                // `iStride2`, reads a block shifted a whole row down and still returns
                // four plausible, distinct, in-range numbers. That mutation survived
                // everything above; it dies here. The reference calls anchor their own
                // cursors, so what is being compared is the shim's span arithmetic
                // against arithmetic written independently of it.
                let c1 = PlaneCursor::new(&b1, 0, s1);
                let c2 = PlaneCursor::new(&b2, s2, s2);
                let mut want = [0i32; 4];
                safe_sad_four(w, h, &c1, &c2, &mut want);
                assert_eq!(
                    &sads[..4],
                    &want[..],
                    "{name}: the shim's block is not where its contract says it is \
                     (strides {s1}/{s2})"
                );
            }
        }
    }
}

/// The two `common/intra_pred_common.rs` shims, same idea, plus the "writes every
/// byte" property.
///
/// The vertical one gets its over-reach checked at stride 16 only, and the reason is
/// worth writing down: its span lies **entirely above `pRef`**, so pinning the far end
/// means the allocation has to stop before `pRef` — which is possible only when `pRef`
/// is one past the end of it, i.e. when the stride equals the sixteen bytes read. At
/// wider strides the call still runs, and still catches an under-claim, but the bytes
/// after the span exist and Miri has nothing to complain about. T4's shims all had
/// `pSrc` inside their own span and never hit this.
#[test]
fn i16x16_luma_pred_shims_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0x1660_0501);

    // Vertical, tight in both directions: sixteen bytes, and `pRef` one past them.
    for _ in 0..scale(20) {
        let top = rng.bytes(16);
        let mut d1 = rng.bytes(256);
        let mut d2 = rng.bytes(256);
        unsafe {
            let p_ref = top.as_ptr().add(16) as *mut u8;
            ipc::WelsI16x16LumaPredV_c(d1.as_mut_ptr(), p_ref, 16);
            ipc::WelsI16x16LumaPredV_c(d2.as_mut_ptr(), p_ref, 16);
        }
        assert_eq!(d1, d2, "PredV left some of its 256 bytes unwritten");
        for y in 0..16 {
            assert_eq!(&d1[y * 16..][..16], &top[..], "PredV row {y}");
        }
    }

    // Horizontal, tight in both directions at every stride: the span runs from one
    // byte left of `pRef` to the last row's left neighbour.
    for &stride in &strides(16) {
        for _ in 0..scale(20) {
            let refs = rng.bytes(15 * stride + 1);
            let mut d1 = rng.bytes(256);
            let mut d2 = rng.bytes(256);
            unsafe {
                let p_ref = refs.as_ptr().add(1) as *mut u8;
                ipc::WelsI16x16LumaPredH_c(d1.as_mut_ptr(), p_ref, stride as i32);
                ipc::WelsI16x16LumaPredH_c(d2.as_mut_ptr(), p_ref, stride as i32);
            }
            assert_eq!(d1, d2, "PredH left some of its 256 bytes unwritten (stride {stride})");
            for y in 0..16 {
                let want = refs[y * stride];
                assert!(
                    d1[y * 16..][..16].iter().all(|&b| b == want),
                    "PredH row {y} is not the sample left of it (stride {stride})"
                );
            }
        }
    }

    // The safe kernels agree with the shims when driven directly, which is what pins
    // the packed-256 destination: a strided destination would put row `y` somewhere
    // else entirely.
    let refs = rng.bytes(15 * 32 + 1);
    let mut viashim = rng.bytes(256);
    let mut direct = [0u8; 256];
    unsafe {
        ipc::WelsI16x16LumaPredH_c(viashim.as_mut_ptr(), refs.as_ptr().add(1) as *mut u8, 32)
    };
    ipc::i16x16_luma_pred_h(&mut direct, &PlaneCursor::new(&refs, 1, 32));
    assert_eq!(&viashim[..], &direct[..]);
}

// ---------------------------------------------------------------------------
// T8 — `processing/vaacalc.rs` + `processing/adaptive_quantization.rs`
//
// The five `VAACalc*` whole-picture walkers and the 16x16 variance probe. Their
// old-vs-new entries went with the shim commit, as the recipe intends: the shims
// call the safe kernels, so a comparison would be comparing a function with itself.
//
// What survives is the span arithmetic, which is the only thing the shims add that
// the kernels do not. `vaa_span` and `mb_span` are the whole contract — get either
// wrong and the shim either reads outside the caller's plane (UB, and Miri says so)
// or claims less than the walk needs (a panic inside the safe kernel).
// ---------------------------------------------------------------------------

use openh264_rs::encoder::wels_preprocess::SMotionTextureUnit;
use openh264_rs::processing::adaptive_quantization as aq;
use openh264_rs::processing::vaacalc as vaa;

/// Macroblocks of slack allocated past the end of every output array, filled with
/// noise that the kernel may not disturb.
const SLACK_MBS: usize = 3;

/// Picture geometries worth driving. Every `(width, height, stride)` is legal for
/// the caller (`stride >= width`), and the set covers what this family's index
/// arithmetic can get wrong:
///
/// * one macroblock at the minimum legal stride, and the two geometries whose span
///   lands *exactly* on the end of the plane (`16x16@16` and `64x48@64`), where an
///   off-by-one in `vaa_span` is an immediate Miri report rather than a silent
///   over-read into slack;
/// * strides wider than the width, and strides that are not multiples of 16;
/// * **widths that are not multiples of 16**, which is where the walk's step quirk
///   lives. `VAACalc*` advances 16 per macroblock and then by
///   `(stride << 4) - width` at the end of the row, so a width of 40 leaves each
///   macroblock row starting eight bytes *before* the row it looks like it should;
/// * heights that are not multiples of 16, where the last partial row is dropped.
fn pictures() -> Vec<(i32, i32, i32)> {
    if cfg!(miri) {
        // Miri is the only instrument that sees the over-claim half of this test and
        // has to run, but at ~100x it cannot walk a 48x64 picture. The cut keeps a
        // tight-span geometry and the step quirk, and drops the merely-larger ones.
        return vec![(16, 16, 16), (40, 32, 64)];
    }
    vec![
        (16, 16, 16),
        (16, 16, 24),
        (32, 32, 32),
        (32, 32, 48),
        (64, 48, 64),
        (48, 64, 80),
        (40, 32, 64),
        (32, 40, 48),
        (24, 24, 33),
    ]
}

/// The byte offset of macroblock `mb`'s top-left sample, **restated independently**
/// of `walk_picture` — step quirk included. If the two ever disagree this test
/// fails, which is the point of restating it here rather than exporting it (the same
/// convention as `Reach` for `mc.rs` above).
fn mb_origin(mb: usize, w: i32, h: i32, stride: i32) -> usize {
    let _ = h;
    let mb_width = (w >> 4) as usize;
    let step = ((stride << 4) - w) as usize;
    (mb / mb_width) * (mb_width * 16 + step) + (mb % mb_width) * 16
}

/// The four quadrant offsets within a macroblock, in the kernels' order.
fn quadrant_offsets(stride: i32) -> [usize; 4] {
    let s = stride as usize;
    [0, 8, 8 * s, 8 * s + 8]
}

fn noise_i32(rng: &mut Prng, len: usize) -> Vec<i32> {
    (0..len).map(|_| rng.next_u32() as i32).collect()
}

/// **Span size.** Every plane is allocated to exactly `vaa_span`, so a shim that
/// claims one byte more is UB that Miri reports at the `from_raw_parts`, and one that
/// claims less panics inside the safe walk. The output arrays carry a noise tail that
/// neither may touch — the F1 defect class in the shape this family can have it.
///
/// All five walkers run, because they compute the same span from the same helper but
/// each writes a different set of output arrays, and an over-run of `pMad8x8` is not
/// caught by checking `pSad8x8`.
#[test]
fn vaa_shims_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0x7A08_5AA5);
    for &(w, h, stride) in &pictures() {
        let span = vaa::vaa_span(w, h, stride);
        let mbs = ((w >> 4) * (h >> 4)) as usize;
        let n = mbs + SLACK_MBS;
        let at = format!("{w}x{h}@{stride}");

        let cur = rng.bytes(span);
        let refp = rng.bytes(span);

        // Every output array each of the five kernels can write, allocated once and
        // re-noised per call so a kernel that fails to write an entry is visible.
        let mut sad = noise_i32(&mut rng, n * 4);
        let mut sd = noise_i32(&mut rng, n * 4);
        let mut sum = noise_i32(&mut rng, n);
        let mut sqsum = noise_i32(&mut rng, n);
        let mut sqdiff = noise_i32(&mut rng, n);
        let mut mad = rng.bytes(n * 4);
        let tail_sad = sad[mbs * 4..].to_vec();
        let tail_sd = sd[mbs * 4..].to_vec();
        let tail_sum = sum[mbs..].to_vec();
        let tail_sqsum = sqsum[mbs..].to_vec();
        let tail_sqdiff = sqdiff[mbs..].to_vec();
        let tail_mad = mad[mbs * 4..].to_vec();
        let mut frame = 0i32;

        unsafe {
            vaa::VAACalcSad_c(
                cur.as_ptr(), refp.as_ptr(), w, h, stride, &mut frame, sad.as_mut_ptr(),
            );
            assert_eq!(&sad[mbs * 4..], &tail_sad[..], "Sad wrote past the last MB, {at}");
            assert_eq!(
                frame,
                sad[..mbs * 4].iter().sum::<i32>(),
                "the frame total is the sum of the parts, {at}"
            );

            vaa::VAACalcSadVar_c(
                cur.as_ptr(), refp.as_ptr(), w, h, stride, &mut frame,
                sad.as_mut_ptr(), sum.as_mut_ptr(), sqsum.as_mut_ptr(),
            );
            assert_eq!(&sad[mbs * 4..], &tail_sad[..], "SadVar wrote past the last MB, {at}");
            assert_eq!(&sum[mbs..], &tail_sum[..], "SadVar wrote past sum16x16, {at}");
            assert_eq!(&sqsum[mbs..], &tail_sqsum[..], "SadVar wrote past sqsum16x16, {at}");

            vaa::VAACalcSadSsd_c(
                cur.as_ptr(), refp.as_ptr(), w, h, stride, &mut frame,
                sad.as_mut_ptr(), sum.as_mut_ptr(), sqsum.as_mut_ptr(), sqdiff.as_mut_ptr(),
            );
            assert_eq!(&sqdiff[mbs..], &tail_sqdiff[..], "SadSsd wrote past sqdiff16x16, {at}");

            vaa::VAACalcSadBgd_c(
                cur.as_ptr(), refp.as_ptr(), w, h, stride, &mut frame,
                sad.as_mut_ptr(), sd.as_mut_ptr(), mad.as_mut_ptr(),
            );
            assert_eq!(&sd[mbs * 4..], &tail_sd[..], "SadBgd wrote past pSd8x8, {at}");
            assert_eq!(&mad[mbs * 4..], &tail_mad[..], "SadBgd wrote past pMad8x8, {at}");

            vaa::VAACalcSadSsdBgd_c(
                cur.as_ptr(), refp.as_ptr(), w, h, stride, &mut frame,
                sad.as_mut_ptr(), sum.as_mut_ptr(), sqsum.as_mut_ptr(),
                sqdiff.as_mut_ptr(), sd.as_mut_ptr(), mad.as_mut_ptr(),
            );
            assert_eq!(&sad[mbs * 4..], &tail_sad[..], "SadSsdBgd wrote past the last MB, {at}");
            assert_eq!(&sd[mbs * 4..], &tail_sd[..], "SadSsdBgd wrote past pSd8x8, {at}");
            assert_eq!(&sqdiff[mbs..], &tail_sqdiff[..], "SadSsdBgd wrote past sqdiff16x16, {at}");
            assert_eq!(&mad[mbs * 4..], &tail_mad[..], "SadSsdBgd wrote past pMad8x8, {at}");
        }
    }
}

/// **Span anchor**, which sizing does not pin. Session C's span property test passed
/// six mutations and survived one that shifted the whole read down a row while
/// keeping the length right, so size and anchor get separate assertions.
///
/// Here every 8x8 quadrant of every macroblock is given its own distinct constant
/// difference, and each has to come back at its own index and nowhere else. A walk
/// anchored one row or one column off, or one that mixed up two quadrants, or one
/// that dropped the step quirk, lands the wrong constant somewhere — and because the
/// constants are distinct the failure names the quadrant it came from.
#[test]
fn vaa_shim_reads_each_quadrant_where_its_contract_says_it_does() {
    for &(w, h, stride) in &pictures() {
        let span = vaa::vaa_span(w, h, stride);
        let mbs = ((w >> 4) * (h >> 4)) as usize;
        let quads = quadrant_offsets(stride);
        let at = format!("{w}x{h}@{stride}");

        // A flat reference, and a current picture whose quadrant (mb, q) differs from
        // it by exactly `delta(mb, q)`. Distinct per quadrant, and small enough that
        // 64 * delta cannot collide with another quadrant's sum.
        let delta = |mb: usize, q: usize| ((mb * 4 + q) % 97 + 1) as u8;
        let refp = vec![0u8; span];
        let mut cur = vec![0u8; span];
        for mb in 0..mbs {
            for (q, &qo) in quads.iter().enumerate() {
                let origin = mb_origin(mb, w, h, stride) + qo;
                for row in 0..8 {
                    for col in 0..8 {
                        cur[origin + row * stride as usize + col] = delta(mb, q);
                    }
                }
            }
        }

        let mut sad = vec![0i32; mbs * 4];
        let mut frame = 0i32;
        unsafe {
            vaa::VAACalcSad_c(
                cur.as_ptr(), refp.as_ptr(), w, h, stride, &mut frame, sad.as_mut_ptr(),
            );
        }

        let mut expect_frame = 0i32;
        for mb in 0..mbs {
            for q in 0..4 {
                let want = 64 * delta(mb, q) as i32;
                assert_eq!(
                    sad[mb * 4 + q], want,
                    "macroblock {mb} quadrant {q} read the wrong 8x8 block, {at}"
                );
                expect_frame += want;
            }
        }
        assert_eq!(frame, expect_frame, "frame total, {at}");
    }
}

/// The variance probe's two spans, which are **independent** — it is the only kernel
/// in this phase that reads two planes at two different strides, and a shim that
/// sized both from one of them would pass every equal-stride case.
///
/// Both allocations are exactly `mb_span(stride)` long, so an over-claim is UB Miri
/// reports and an under-claim panics.
#[test]
fn sample_variance_shim_stays_inside_the_spans_it_declares() {
    let mut rng = Prng::new(0x7A08_0006);
    for &ref_stride in &strides(16) {
        for &src_stride in &strides(16) {
            for _ in 0..scale(25) {
                let refy = rng.bytes(aq::mb_span(ref_stride));
                let srcy = rng.bytes(aq::mb_span(src_stride));
                let mut got = SMotionTextureUnit {
                    uiMotionIndex: 0xAAAA,
                    uiTextureIndex: 0x5555,
                };
                unsafe {
                    aq::SampleVariance16x16_c(
                        refy.as_ptr(), ref_stride as i32,
                        srcy.as_ptr(), src_stride as i32,
                        &mut got,
                    );
                }
                // Both fields must have been written — the sentinel is not a value
                // this kernel can produce for both at once from random input.
                assert!(
                    !(got.uiMotionIndex == 0xAAAA && got.uiTextureIndex == 0x5555),
                    "left the sentinel in place at strides {ref_stride}/{src_stride}"
                );
                let direct = aq::sample_variance_16x16(&refy, ref_stride, &srcy, src_stride);
                assert_eq!(got.uiMotionIndex, direct.uiMotionIndex);
                assert_eq!(got.uiTextureIndex, direct.uiTextureIndex);
            }
        }
    }
}

/// The two extremes of the variance probe's input domain, which the random sweep
/// never reaches and which are where the C++'s integer widths would bite if they
/// could.
///
/// A 16x16 block holds 256 samples of at most 255, so the `uint16_t` sums top out at
/// **65280** and the `uint32_t` squares at **16 646 400** — both short of wrapping.
/// The `wrapping_add`s the port carries are therefore unreachable, and this is the
/// test that says so. It is also why the file's header no longer claims the
/// difference sum can wrap: it has exactly the same bound as the other one.
#[test]
fn sample_variance_16x16_accumulators_cannot_wrap() {
    for (refv, srcv) in [(255u8, 0u8), (0, 255), (255, 255), (0, 0)] {
        let refy = vec![refv; aq::mb_span(16)];
        let srcy = vec![srcv; aq::mb_span(16)];
        let got = aq::sample_variance_16x16(&refy, 16, &srcy, 16);
        // A uniform block has zero variance either way round, and a uniform
        // difference has zero variance of differences.
        assert_eq!(got.uiMotionIndex, 0, "uniform difference, ref {refv} src {srcv}");
        assert_eq!(got.uiTextureIndex, 0, "uniform picture, ref {refv} src {srcv}");
    }
}

// ===========================================================================
// T6 — `common/deblocking_common.rs`, in-loop deblocking
// ===========================================================================
//
// `bbb9348e` proved the six safe edge filters and `nonzero_count` against the
// raw kernels — all twelve V/H ABI wrappers, three amplitude tiers so every
// branch of the conditional filters executed, tc sweeps including the
// gate-closing negatives, three strides per shape, random anchors, every byte
// of every touched buffer compared, and three mutations (tc gate off-by-one,
// wrong write tap, step-axis swap) all killed. The shim commit deleted those
// equivalences, because both sides now run the same code.
//
// What survives is what the shims add: **span arithmetic**. Each shim
// materialises the slice its `# Safety` contract declares —
// `[-reach_back*step_x, reach_fwd*step_x + (lines-1)*step_y]` around the edge
// anchor — and the test below re-derives those spans independently and probes
// each shim with allocations of exactly that size. Deblocking needs one more
// assertion than `mc.rs` did, because its writes are *conditional*: a shim
// whose body silently stopped filtering would pass a pure in-bounds probe. The
// probe therefore drives a crafted 90-vs-110 step edge under maximal
// `alpha`/`beta`/`tc`, on which every line provably filters, and asserts both
// that the filter fired and that **no byte outside the declared touch set
// moved** — which pins the shim's anchor placement separately from its span
// size (T5's lesson: sizing a span does not pin where it starts).

use openh264_rs::common::deblocking_common as deb;

/// The touched-offset predicate, valid for both directions at once: within the
/// span (anchored at its own start), a byte at index `k` belongs to the edge's
/// touch set iff `k % stride` is under the dense width — `lines` for a V-shaped
/// call (taps step by the stride, lines are contiguous columns), the tap width
/// `rb + rf + 1` for an H-shaped one (taps are contiguous columns, lines step
/// by the stride).
fn deblock_touched(k: usize, stride: usize, dense_width: usize) -> bool {
    k % stride < dense_width
}

/// Exact-span probe for one single-plane deblocking shim.
///
/// * The buffer is `len` bytes and not one more, so an over-claimed span is a
///   `from_raw_parts_mut` past the allocation — UB that Miri reports; an
///   under-claimed one panics in the safe kernel at the first out-of-slice tap.
/// * The crafted step edge (p side 90, q side 110, alpha 255, beta 18, tc 25)
///   filters on every line, so the probe asserts the surface changed — a shim
///   that no-ops fails here.
/// * The same crafted buffer is run through the **safe kernel directly**, on a
///   cursor built at the anchor the contract names, and the two results must be
///   bit-identical. This is what pins the shim's anchor: an anchor shifted
///   along the line axis stays inside the touch set and filters plausibly, and
///   only a golden comparison sees it (the first version of this probe missed
///   exactly that mutation).
/// * Every byte outside the declared touch set must additionally be
///   bit-identical to the input — the claim the contract makes to callers about
///   which bytes may move at all.
fn probe_deblock_span(
    name: &str,
    stride: usize,
    rb: usize,
    rf: usize,
    lines: usize,
    vertical: bool,
    run: impl Fn(*mut u8, i32),
    direct: impl Fn(&mut [u8], usize, isize, isize),
) {
    let (sx, sy) = if vertical { (stride, 1) } else { (1, stride) };
    let back = rb * sx;
    let len = back + rf * sx + (lines - 1) * sy + 1;
    let dense_width = if vertical { lines } else { rb + rf + 1 };

    // 90 on the p side of the edge, 110 on the q side, on every touched byte;
    // sentinel noise elsewhere.
    let mut buf: Vec<u8> = (0..len).map(|k| 0x40 + (k % 191) as u8).collect();
    for j in 0..=(rb + rf) {
        for i in 0..lines {
            buf[j * sx + i * sy] = if j < rb { 90 } else { 110 };
        }
    }
    let before = buf.clone();
    let mut golden = buf.clone();

    run(unsafe { buf.as_mut_ptr().add(back) }, stride as i32);
    direct(&mut golden, back, sx as isize, sy as isize);

    assert_ne!(buf, before, "{name}: the always-filter edge did not filter (stride {stride})");
    assert_eq!(
        buf, golden,
        "{name}: shim output disagrees with the safe kernel run at the \
         contract's own anchor (stride {stride}, back {back}, len {len})"
    );
    for (k, (&now, &was)) in buf.iter().zip(before.iter()).enumerate() {
        if !deblock_touched(k, stride, dense_width) {
            assert_eq!(
                now, was,
                "{name}: byte {k} is outside the declared touch set and moved \
                 (stride {stride}, back {back}, len {len})"
            );
        }
    }
}

#[test]
fn deblock_shims_stay_inside_the_spans_they_declare() {
    let mut tc = [25i8; 4];
    let tcp = tc.as_mut_ptr();
    // Strides: the dense minimum for each shape, a non-multiple-of-16, and a
    // real picture stride.
    for &s in &[16usize, 21, 240] {
        // Luma, both tap directions: reach [-3,2] weak / [-4,3] strong, 16 lines.
        for &vert in &[true, false] {
            let (name_lt, name_eq) = if vert {
                ("DeblockLumaLt4V_c", "DeblockLumaEq4V_c")
            } else {
                ("DeblockLumaLt4H_c", "DeblockLumaEq4H_c")
            };
            probe_deblock_span(
                name_lt, s, 3, 2, 16, vert,
                |p, st| unsafe {
                    if vert { deb::DeblockLumaLt4V_c(p, st, 255, 18, tcp) }
                    else { deb::DeblockLumaLt4H_c(p, st, 255, 18, tcp) }
                },
                |b, back, sx, sy| {
                    deb::deblock_luma_lt4(&mut PlaneCursorMut::new(b, back, s), sx, sy, 255, 18, &[25; 4]);
                },
            );
            probe_deblock_span(
                name_eq, s, 4, 3, 16, vert,
                |p, st| unsafe {
                    if vert { deb::DeblockLumaEq4V_c(p, st, 255, 18) }
                    else { deb::DeblockLumaEq4H_c(p, st, 255, 18) }
                },
                |b, back, sx, sy| {
                    deb::deblock_luma_eq4(&mut PlaneCursorMut::new(b, back, s), sx, sy, 255, 18);
                },
            );

            // Single-buffer chroma variants: reach [-2,1], 8 lines.
            let (name_lt2, name_eq2) = if vert {
                ("DeblockChromaLt4V2_c", "DeblockChromaEq4V2_c")
            } else {
                ("DeblockChromaLt4H2_c", "DeblockChromaEq4H2_c")
            };
            probe_deblock_span(
                name_lt2, s, 2, 1, 8, vert,
                |p, st| unsafe {
                    if vert { deb::DeblockChromaLt4V2_c(p, st, 255, 18, tcp) }
                    else { deb::DeblockChromaLt4H2_c(p, st, 255, 18, tcp) }
                },
                |b, back, sx, sy| {
                    deb::deblock_chroma_lt42(&mut PlaneCursorMut::new(b, back, s), sx, sy, 255, 18, &[25; 4]);
                },
            );
            probe_deblock_span(
                name_eq2, s, 2, 1, 8, vert,
                |p, st| unsafe {
                    if vert { deb::DeblockChromaEq4V2_c(p, st, 255, 18) }
                    else { deb::DeblockChromaEq4H2_c(p, st, 255, 18) }
                },
                |b, back, sx, sy| {
                    deb::deblock_chroma_eq42(&mut PlaneCursorMut::new(b, back, s), sx, sy, 255, 18);
                },
            );

            // Two-plane chroma: probe each plane position through the pair
            // call, the other plane parked on its own exact-span scratch. The
            // planes are filtered independently, so the golden run may use any
            // content for the scratch plane.
            let (step_x, step_y) = if vert { (s, 1) } else { (1, s) };
            let scratch_len = 3 * step_x + 7 * step_y + 1;
            let scratch_anchor = 2 * step_x;
            for &probe_cb in &[true, false] {
                let pos = if probe_cb { "cb" } else { "cr" };
                probe_deblock_span(
                    &format!("DeblockChromaLt4{}_c[{pos}]", if vert { "V" } else { "H" }),
                    s, 2, 1, 8, vert,
                    |p, st| unsafe {
                        let mut other = vec![128u8; scratch_len];
                        let o = other.as_mut_ptr().add(scratch_anchor);
                        let (cb, cr) = if probe_cb { (p, o) } else { (o, p) };
                        if vert { deb::DeblockChromaLt4V_c(cb, cr, st, 255, 18, tcp) }
                        else { deb::DeblockChromaLt4H_c(cb, cr, st, 255, 18, tcp) }
                    },
                    |b, back, sx_, sy_| {
                        let mut other = vec![128u8; scratch_len];
                        let mut oc = PlaneCursorMut::new(&mut other, scratch_anchor, s);
                        let mut pc = PlaneCursorMut::new(b, back, s);
                        let (cb, cr) = if probe_cb { (&mut pc, &mut oc) } else { (&mut oc, &mut pc) };
                        deb::deblock_chroma_lt4(cb, cr, sx_, sy_, 255, 18, &[25; 4]);
                    },
                );
                probe_deblock_span(
                    &format!("DeblockChromaEq4{}_c[{pos}]", if vert { "V" } else { "H" }),
                    s, 2, 1, 8, vert,
                    |p, st| unsafe {
                        let mut other = vec![128u8; scratch_len];
                        let o = other.as_mut_ptr().add(scratch_anchor);
                        let (cb, cr) = if probe_cb { (p, o) } else { (o, p) };
                        if vert { deb::DeblockChromaEq4V_c(cb, cr, st, 255, 18) }
                        else { deb::DeblockChromaEq4H_c(cb, cr, st, 255, 18) }
                    },
                    |b, back, sx_, sy_| {
                        let mut other = vec![128u8; scratch_len];
                        let mut oc = PlaneCursorMut::new(&mut other, scratch_anchor, s);
                        let mut pc = PlaneCursorMut::new(b, back, s);
                        let (cb, cr) = if probe_cb { (&mut pc, &mut oc) } else { (&mut oc, &mut pc) };
                        deb::deblock_chroma_eq4(cb, cr, sx_, sy_, 255, 18);
                    },
                );
            }
        }
    }
}

/// The `WelsNonZeroCount_c` shim materialises exactly the 24 `i8` its contract
/// declares (an over-claim is Miri-visible UB against this exact-size array),
/// and normalisation is total on the full `i8` range including `i8::MIN`.
#[test]
fn nonzero_count_shim_stays_inside_its_span_and_normalises() {
    let mut rng = Prng::new(0xDEB3_0024);
    for _ in 0..scale(50) {
        let mut nzc: [i8; 24] = std::array::from_fn(|_| rng.next_u8() as i8);
        let want: [i8; 24] = std::array::from_fn(|i| (nzc[i] != 0) as i8);
        unsafe {
            deb::WelsNonZeroCount_c(nzc.as_mut_ptr());
        }
        assert_eq!(nzc, want, "seed {:#x}", rng.seed());
    }
    let mut extremes: [i8; 24] = [i8::MIN; 24];
    extremes[23] = 0;
    unsafe {
        deb::WelsNonZeroCount_c(extremes.as_mut_ptr());
    }
    assert_eq!(&extremes[..23], &[1i8; 23][..]);
    assert_eq!(extremes[23], 0);
}

// ===========================================================================
// T6 — border expansion (`common/expand_pic.rs` + `decoder_core.rs`'s shims)
// ===========================================================================
//
// `2fe283e4` proved `expand_picture` against both raw variants over full
// allocations — every byte compared, minimum-legal and slack strides, exact
// and over-tall row counts, odd sizes; corner-swap and first-row-skip
// mutations died. The shim commit deleted that equivalence, because both
// sides now run the same code.
//
// What survives is what the shims add, and it is the trickiest span in the
// phase: the caller hands a **mid-allocation** pointer (`pData[i]`, `pad`
// rows plus `pad` bytes in) and the shim walks *backwards* to the allocation
// start with the per-variant pad constant (32 luma, 16 chroma) before
// claiming `(h + 2*pad) * stride` bytes. The probes below hand each shim an
// allocation of exactly that size (an over-claim or a wrong-direction
// reconstruction is UB Miri reports; an under-claim asserts in the safe
// kernel), and three assertions pin the rest:
//
// * **Every padding byte is written and none is read**: two runs whose
//   inputs differ only in the padding bytes must produce identical
//   allocations.
// * **The anchor is where the contract says**: the shim's result must equal
//   `expand_picture` run directly on the same input — a reconstruction of
//   `pad*stride` instead of `pad*stride + pad` would land every write one
//   pad short.
// * **Slack columns stay untouched** at a wider-than-minimal stride, which is
//   what an aligned real allocation has.

use openh264_rs::common::expand_pic as exp;
use openh264_rs::decoder::decoder_core as dcore;

#[test]
fn expand_shims_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0xE8_9A2D_0016);
    type RawExpand = unsafe extern "C" fn(*mut u8, i32, i32, i32);
    let variants: &[(usize, RawExpand, &str)] = &[
        (32, dcore::ExpandPictureLuma_c as RawExpand, "luma"),
        (16, dcore::ExpandPictureChroma_c as RawExpand, "chroma"),
    ];
    for &(pad, shim, name) in variants {
        for &(w, h) in &[(16usize, 16usize), (9, 11)] {
            for &slack in &[0usize, 13] {
                let stride = w + 2 * pad + slack;
                let rows = h + 2 * pad;
                for _ in 0..scale(3) {
                    // `a` and `b` share the picture rectangle; everything
                    // outside it is noised independently.
                    let before_a = rng.bytes(rows * stride);
                    let mut before_b = rng.bytes(rows * stride);
                    for y in 0..h {
                        for x in 0..w {
                            let i = (pad + y) * stride + pad + x;
                            before_b[i] = before_a[i];
                        }
                    }
                    let mut a = before_a.clone();
                    let mut b = before_b.clone();
                    unsafe {
                        shim(a.as_mut_ptr().add(pad * stride + pad), stride as i32, w as i32, h as i32);
                        shim(b.as_mut_ptr().add(pad * stride + pad), stride as i32, w as i32, h as i32);
                    }

                    // The anchor: the shim must equal the safe kernel run at
                    // the geometry the contract names.
                    let mut golden = before_a.clone();
                    exp::expand_picture(&mut golden, stride, w, h, pad);
                    assert_eq!(
                        a, golden,
                        "{name} {w}x{h} stride {stride}: shim disagrees with the safe \
                         kernel at the contract's geometry (seed {:#x})",
                        rng.seed()
                    );

                    // Slack columns (an aligned allocation's tail) untouched.
                    if slack > 0 {
                        for y in 0..rows {
                            let sl = y * stride + 2 * pad + w;
                            assert_eq!(
                                &a[sl..(y + 1) * stride],
                                &before_a[sl..(y + 1) * stride],
                                "{name} {w}x{h}: slack columns of row {y} moved"
                            );
                        }
                        continue; // the all-padding-written property needs no slack
                    }

                    // Padding fully written from picture content, picture
                    // content untouched: inputs differing only in padding
                    // must converge byte-for-byte.
                    assert_eq!(
                        a, b,
                        "{name} {w}x{h} stride {stride}: some padding byte survived \
                         (was read or left unwritten) — seed {:#x}",
                        rng.seed()
                    );
                }
            }
        }
    }
}

// ===========================================================================
// T7 — `encoder/encode_mb_aux.rs`, the forward transform / quant / scan family
// ===========================================================================
//
// Commit-A entries: every kernel driven old-vs-new over the same inputs, the
// destination arrays compared in full and the pixel sources asserted
// untouched. The quantization sweeps are **exhaustive over the QP tables**
// (all 52 QPs, inter and intra rows — R-d's rule; a random sweep blurs
// exactly the lane-indexing mistakes a conversion makes), with coefficients
// at the full `i16` range, which is in-bounds here: for non-negative table
// factors `(ff + |v|) * mf <= 65534 * 32767 < i32::MAX` (the bound is derived
// at `encode_mb_aux::quant_one`; a negative `mf` could overflow, and no table
// contains one, so the sweep stays in the tables). These entries are deleted
// at the family's commit B, when both sides become the same code.

use openh264_rs::encoder::encode_mb_aux as ema;

/// Full-range random coefficients.
fn coeffs<const N: usize>(rng: &mut Prng) -> [i16; N] {
    core::array::from_fn(|_| rng.range_i32(-32768, 32767) as i16)
}

/// Sparse random coefficients — mostly zero, the distribution the run-length
/// scanners actually see.
fn sparse_coeffs<const N: usize>(rng: &mut Prng) -> [i16; N] {
    core::array::from_fn(|_| {
        if rng.below(4) == 0 {
            rng.range_i32(-32768, 32767) as i16
        } else {
            0
        }
    })
}

#[test]
fn encode_dct_kernels_match_the_raw_ones() {
    let mut rng = Prng::new(0xE4C0_DC70);

    for &(s1, s2) in &[(4usize, 16usize), (21, 4), (240, 25), (16, 16)] {
        for _ in 0..scale(100) {
            let (b1, c1) = surface(&mut rng, s1.max(4), 4, 4);
            let (b2, c2) = surface(&mut rng, s2.max(4), 4, 4);
            let mut raw = [0i16; 16];
            let mut safe = [0i16; 16];

            let (mut m1, mut m2) = (b1.clone(), b2.clone());
            unsafe {
                ema::WelsDctT4_c(
                    raw.as_mut_ptr(),
                    m1.as_mut_ptr().add(c1),
                    s1 as i32,
                    m2.as_mut_ptr().add(c2),
                    s2 as i32,
                );
            }
            ema::dct_4x4(
                &mut safe,
                &PlaneCursor::new(&b1, c1, s1),
                &PlaneCursor::new(&b2, c2, s2),
            );
            assert_eq!(raw, safe, "DctT4 s1={s1} s2={s2}, seed {:#x}", rng.seed());
            assert_eq!((m1, m2), (b1, b2), "DctT4 wrote a pixel source");
        }
    }

    for &(s1, s2) in &[(8usize, 16usize), (21, 8), (240, 25)] {
        for _ in 0..scale(60) {
            let (b1, c1) = surface(&mut rng, s1, 8, 8);
            let (b2, c2) = surface(&mut rng, s2, 8, 8);
            let mut raw = [0i16; 64];
            let mut safe = [0i16; 64];

            let (mut m1, mut m2) = (b1.clone(), b2.clone());
            unsafe {
                ema::WelsDctFourT4_c(
                    raw.as_mut_ptr(),
                    m1.as_mut_ptr().add(c1),
                    s1 as i32,
                    m2.as_mut_ptr().add(c2),
                    s2 as i32,
                );
            }
            ema::dct_four_4x4(
                &mut safe,
                &PlaneCursor::new(&b1, c1, s1),
                &PlaneCursor::new(&b2, c2, s2),
            );
            assert_eq!(raw, safe, "DctFourT4 s1={s1} s2={s2}, seed {:#x}", rng.seed());
            assert_eq!((m1, m2), (b1, b2), "DctFourT4 wrote a pixel source");
        }
    }
}

#[test]
fn encode_quant_kernels_match_the_raw_ones() {
    let mut rng = Prng::new(0xE4C0_0A47);

    for qp in 0..52usize {
        for intra in [false, true] {
            let ff = &ema::g_kiQuantInterFF[qp + if intra { 6 } else { 0 }];
            let mf = &ema::g_kiQuantMF[qp];

            for _ in 0..scale(6) {
                let base: [i16; 64] = coeffs(&mut rng);

                let sub: &[i16; 16] = (&base[..16]).try_into().unwrap();
                let mut raw = *sub;
                let mut safe = *sub;
                unsafe { ema::WelsQuant4x4_c(raw.as_mut_ptr(), ff.as_ptr(), mf.as_ptr()) };
                ema::quant_4x4(&mut safe, ff, mf);
                assert_eq!(raw, safe, "Quant4x4 qp={qp} intra={intra}");

                let mut raw = *sub;
                let mut safe = *sub;
                unsafe { ema::WelsQuant4x4Dc_c(raw.as_mut_ptr(), ff[0] << 1, mf[0] >> 1) };
                ema::quant_4x4_dc(&mut safe, ff[0] << 1, mf[0] >> 1);
                assert_eq!(raw, safe, "Quant4x4Dc qp={qp} intra={intra}");

                let mut raw = base;
                let mut safe = base;
                unsafe { ema::WelsQuantFour4x4_c(raw.as_mut_ptr(), ff.as_ptr(), mf.as_ptr()) };
                ema::quant_four_4x4(&mut safe, ff, mf);
                assert_eq!(raw, safe, "QuantFour4x4 qp={qp} intra={intra}");

                let mut raw = base;
                let mut safe = base;
                let mut raw_max = [0i16; 4];
                let mut safe_max = [0i16; 4];
                unsafe {
                    ema::WelsQuantFour4x4Max_c(
                        raw.as_mut_ptr(),
                        ff.as_ptr(),
                        mf.as_ptr(),
                        raw_max.as_mut_ptr(),
                    )
                };
                ema::quant_four_4x4_max(&mut safe, ff, mf, &mut safe_max);
                assert_eq!(raw, safe, "QuantFour4x4Max qp={qp} intra={intra}");
                assert_eq!(raw_max, safe_max, "QuantFour4x4Max max qp={qp} intra={intra}");
            }
        }
    }
}

#[test]
fn encode_hadamard_kernels_match_the_raw_ones() {
    let mut rng = Prng::new(0xE4C0_44D0);

    // T4Dc: the 256-coefficient luma buffer, full range (the kernel computes
    // in i32 with an explicit clip, total over all of it).
    for _ in 0..scale(200) {
        let mb: [i16; 256] = coeffs(&mut rng);
        let mut raw = [0i16; 16];
        let mut safe = [0i16; 16];
        let mut m = mb;
        unsafe { ema::WelsHadamardT4Dc_c(raw.as_mut_ptr(), m.as_mut_ptr()) };
        ema::hadamard_t4_dc(&mut safe, (&mb[..241]).try_into().unwrap());
        assert_eq!(raw, safe, "HadamardT4Dc, seed {:#x}", rng.seed());
        assert_eq!(m, mb, "HadamardT4Dc wrote its source");
    }

    // 2x2 + skip: factors exactly as the one caller passes them
    // (`ff << 1`, `mf >> 1`, `svc_encode_mb.rs` WelsEncRecUV), every QP row.
    for qp in 0..52usize {
        for intra in [false, true] {
            let ff = ema::g_kiQuantInterFF[qp + if intra { 6 } else { 0 }][0] << 1;
            let mf = ema::g_kiQuantMF[qp][0] >> 1;

            for _ in 0..scale(6) {
                let base: [i16; 64] = coeffs(&mut rng);

                let mut raw = base;
                let raw_ret = unsafe { ema::WelsHadamardQuant2x2Skip_c(raw.as_mut_ptr(), ff, mf) };
                let safe_ret =
                    ema::hadamard_quant_2x2_skip((&base[..49]).try_into().unwrap(), ff, mf);
                assert_eq!(raw_ret, safe_ret, "HadamardQuant2x2Skip qp={qp} intra={intra}");
                assert_eq!(raw, base, "HadamardQuant2x2Skip wrote its source");

                let mut raw = base;
                let mut safe = base;
                let mut raw_dct = [0i16; 4];
                let mut raw_blk = [0i16; 4];
                let mut safe_dct = [0i16; 4];
                let mut safe_blk = [0i16; 4];
                let raw_ret = unsafe {
                    ema::WelsHadamardQuant2x2_c(
                        raw.as_mut_ptr(),
                        ff,
                        mf,
                        raw_dct.as_mut_ptr(),
                        raw_blk.as_mut_ptr(),
                    )
                };
                let safe_ret = ema::hadamard_quant_2x2(
                    (&mut safe[..49]).try_into().unwrap(),
                    ff,
                    mf,
                    &mut safe_dct,
                    &mut safe_blk,
                );
                assert_eq!(raw_ret, safe_ret, "HadamardQuant2x2 return qp={qp}");
                assert_eq!(raw, safe, "HadamardQuant2x2 residual qp={qp}");
                assert_eq!(raw_dct, safe_dct, "HadamardQuant2x2 dct qp={qp}");
                assert_eq!(raw_blk, safe_blk, "HadamardQuant2x2 block qp={qp}");
            }
        }
    }
}

#[test]
fn encode_scan_kernels_match_the_raw_ones() {
    let mut rng = Prng::new(0xE4C0_5CA7);

    for i in 0..scale(400) {
        // Alternate dense and sparse inputs — the run-length walker
        // (`CalculateSingleCtr`) only branches interestingly on sparse ones.
        let dct: [i16; 16] = if i % 2 == 0 { coeffs(&mut rng) } else { sparse_coeffs(&mut rng) };
        let mut m = dct;

        let mut raw = [0i16; 16];
        let mut safe = [0i16; 16];
        unsafe { ema::WelsScan4x4DcAc_c(raw.as_mut_ptr(), m.as_mut_ptr()) };
        ema::scan_4x4_dc_ac(&mut safe, &dct);
        assert_eq!(raw, safe, "Scan4x4DcAc, seed {:#x}", rng.seed());

        let mut raw = [0i16; 16];
        unsafe { ema::WelsScan4x4Dc(raw.as_mut_ptr(), m.as_mut_ptr()) };
        assert_eq!(raw, safe, "Scan4x4Dc (alias of DcAc), seed {:#x}", rng.seed());

        let mut raw = [0i16; 16];
        let mut safe = [0i16; 16];
        unsafe { ema::WelsScan4x4Ac_c(raw.as_mut_ptr(), m.as_mut_ptr()) };
        ema::scan_4x4_ac(&mut safe, &dct);
        assert_eq!(raw, safe, "Scan4x4Ac, seed {:#x}", rng.seed());

        let raw_ctr = unsafe { ema::WelsCalculateSingleCtr4x4_c(m.as_mut_ptr()) };
        assert_eq!(
            raw_ctr,
            ema::calculate_single_ctr_4x4(&dct),
            "CalculateSingleCtr4x4, seed {:#x}",
            rng.seed()
        );

        let raw_nzc = unsafe { ema::WelsGetNoneZeroCount_c(m.as_mut_ptr()) };
        assert_eq!(
            raw_nzc,
            ema::get_none_zero_count(&dct),
            "GetNoneZeroCount, seed {:#x}",
            rng.seed()
        );

        assert_eq!(m, dct, "a scan kernel wrote its source");
    }
}

#[test]
fn encode_copy_kernels_match_the_raw_ones() {
    let mut rng = Prng::new(0xE4C0_C099);
    type RawCopy = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);
    type SafeCopy = fn(&PlaneCursor<'_>, &mut PlaneCursorMut<'_>);
    let kernels: &[(&str, usize, usize, RawCopy, SafeCopy)] = &[
        ("Copy4x4", 4, 4, ema::WelsCopy4x4_c, ema::copy_4x4),
        ("Copy8x4", 8, 4, ema::WelsCopy8x4_c, ema::copy_8x4),
        ("Copy4x8", 4, 8, ema::WelsCopy4x8_c, ema::copy_4x8),
        ("Copy8x8", 8, 8, ema::WelsCopy8x8_c, ema::copy_8x8),
        ("Copy16x8", 16, 8, ema::WelsCopy16x8_c, ema::copy_16x8),
        ("Copy8x16", 8, 16, ema::WelsCopy8x16_c, ema::copy_8x16),
        ("Copy16x16", 16, 16, ema::WelsCopy16x16_c, ema::copy_16x16),
    ];

    for &(name, w, h, raw, safe) in kernels {
        for &ss in &[w, 21.max(w), 240] {
            for &ds in &[w, 29.max(w), 240] {
                for _ in 0..scale(20) {
                    let (src, sc) = surface(&mut rng, ss, w, h);
                    let (dst, dc) = surface(&mut rng, ds, w, h);

                    let mut src_m = src.clone();
                    let mut dst_raw = dst.clone();
                    unsafe {
                        raw(
                            dst_raw.as_mut_ptr().add(dc),
                            ds as i32,
                            src_m.as_mut_ptr().add(sc),
                            ss as i32,
                        )
                    };

                    let mut dst_safe = dst.clone();
                    safe(
                        &PlaneCursor::new(&src, sc, ss),
                        &mut PlaneCursorMut::new(&mut dst_safe, dc, ds),
                    );

                    assert_eq!(
                        dst_raw, dst_safe,
                        "{name} ss={ss} ds={ds}, seed {:#x}",
                        rng.seed()
                    );
                    assert_eq!(src_m, src, "{name} wrote its source");
                }
            }
        }
    }
}
