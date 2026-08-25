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

// `openh264_rs::common::mc` is no longer imported here: D-cov-1 (T9.B4) deleted both
// tests that named it. The module's two surviving raw entry points are dormant
// screen-content shims and its safe kernels are exercised by `mc.rs`'s own tests.

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

// **D-cov-1 (T9.B4): `mc_shims_stay_inside_the_spans_they_declare` deleted with its
// subjects.** It drove every `common/mc.rs` shim against allocations sized to exactly
// the span the shim declares — a contract test of the shims' own span arithmetic, and
// nothing else. The 26 shims it drove have no `src/` caller (the last four lost theirs
// at T9.B29) and are gone; the safe kernels underneath carry their bounds in
// `PlaneCursor`, so there is no span arithmetic left to pin. This file's own header
// states the doctrine: a shim test dies with the shim.
//
// `McLuma_c` and `McChroma_c` survive as `SCREEN_CONTENT(dormant)` — their only
// callers are `SvcMdSCDMbEnc`'s three, unreachable without
// `iUsageType == SCREEN_CONTENT_REAL_TIME` (F125) — and they retire in Phase 10 with
// the rest of that family. The helpers this test shared with the sad / intra-pred /
// vaa span tests below (`copy_width`, `strides`, `sizes`, `src_span`, `block_span`,
// `probe_span`, the `Reach` constants) all stay: they have other callers.


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

// `intra_pred_common as ipc` was imported here for the two shim-span entries
// retired in T9.C2; the module has no raw surface left to drive.
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

// **The two `common/intra_pred_common.rs` shim-span entries were retired here
// (T9.C2)**, with `i16x16_luma_pred_shims_stay_inside_the_spans_they_declare`.
//
// They probed `WelsI16x16LumaPredV_c` and `WelsI16x16LumaPredH_c` — the file's
// only two raw wrappers — against exactly-sized reference allocations, to prove
// the spans their `# Safety` contracts declared were tight in both directions.
// T9.C2 deleted both wrappers: the encoder's prediction tables are safe over
// `RecCursor` now, so the adapters moved to `encoder/get_intra_predictor.rs` and
// `common/intra_pred_common.rs` reached `#![deny(unsafe_code)]` with nothing raw
// left to bound. See the T9.C2 tombstone further down for the full accounting of
// which property each retired probe held and what holds it now.
//
// The two safe kernels keep their unit coverage in `common/intra_pred_common.rs`,
// rewritten in the same commit to drive them over a `PaddedPlane` instead of
// through the wrappers.

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
// T6 — border expansion (`common/expand_pic.rs`)
// ===========================================================================
//
// `2fe283e4` proved `expand_picture` against both raw variants over full
// allocations — every byte compared, minimum-legal and slack strides, exact
// and over-tall row counts, odd sizes; corner-swap and first-row-skip
// mutations died. The shim commit deleted that equivalence, because both
// sides now ran the same code.
//
// What stood here after that was a probe of `ExpandPictureLuma_c` /
// `ExpandPictureChroma_c`, which took a **mid-allocation** pointer and walked
// backwards to the allocation start (`expand_shim_span`). Those three items had
// no caller anywhere in `src/` — both codecs' pictures own their planes and hand
// `expand_picture` the allocation directly — and the probe's golden was
// `expand_picture` itself, so the equivalence it asserted was between a function
// and its own callee. T9.B2 deleted all three (S18, F104's census found them) and
// this is what the probe was really pinning, run against the kernel:
//
// * **Every padding byte is written and none is read**: two allocations whose
//   inputs differ only outside the picture rectangle must converge byte for byte.
// * **Slack columns stay untouched** at a wider-than-minimal stride, which is
//   what an aligned real allocation has.

use openh264_rs::common::expand_pic as exp;

#[test]
fn expand_picture_writes_every_padding_byte_and_reads_none() {
    let mut rng = Prng::new(0xE8_9A2D_0016);
    // The two border widths both codecs allocate: `PADDING_LENGTH` for luma and
    // `PADDING_LENGTH >> 1` for chroma.
    for &(pad, name) in &[(32usize, "luma"), (16usize, "chroma")] {
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
                    exp::expand_picture(&mut a, stride, w, h, pad);
                    exp::expand_picture(&mut b, stride, w, h, pad);

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
// `f233d506` proved all 21 safe kernels against the raw ones: exhaustive QP x
// {inter, intra} table sweeps for the quantizers and the 2x2 Hadamard (R-d's
// rule — a random sweep blurs exactly the lane-indexing mistakes a conversion
// makes), full-range `i16` coefficients everywhere they are in-bounds (for
// non-negative table factors `(ff + |v|) * mf <= 65534 * 32767 < i32::MAX`,
// derived at `encode_mb_aux::quant_one`; a negative `mf` could overflow and
// no table contains one), whole-destination compares, and sources asserted
// untouched. Three mutations (zigzag swap, quant lane index, DCT butterfly)
// were run against the entries and all three died. The shim commit deleted
// the equivalences, because both sides now run the same code.
//
// What survives is what the shims add: **span arithmetic**. Two spans in this
// family are exact reaches over strided reads — the 2x2 Hadamard's
// `[i16; 49]` (DC raster positions 0/16/32/48 of the chroma group) and the
// DC-Hadamard's `[i16; 241]` (block 15's DC at index 240 of the 256-element
// luma buffer) — and the probes below hand each shim an allocation of
// exactly that size (an over-claim is UB Miri reports; an under-claim panics
// in the safe kernel), with a **golden direct run** of the safe kernel at
// the contract's own geometry pinning the anchor (session E's probe lesson),
// and untouched-byte assertions pinning the touch set.

use openh264_rs::encoder::encode_mb_aux as ema;

/// Full-range random coefficients.
fn coeffs<const N: usize>(rng: &mut Prng) -> [i16; N] {
    core::array::from_fn(|_| rng.range_i32(-32768, 32767) as i16)
}

#[test]
fn encode_mb_aux_shims_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0xE4C0_59A9);

    // --- The two DCTs: exact pixel spans (3*stride + 4 / 7*stride + 8, the
    // forward-only reach), exact coefficient spans, golden direct run,
    // sources untouched.
    for &(s1, s2) in &[(4usize, 16usize), (21, 5), (240, 25)] {
        let mut b1 = rng.bytes(3 * s1 + 4);
        let mut b2 = rng.bytes(3 * s2 + 4);
        let (k1, k2) = (b1.clone(), b2.clone());
        let mut dct = [0i16; 16];
        unsafe {
            ema::WelsDctT4_c(
                dct.as_mut_ptr(),
                b1.as_mut_ptr(),
                s1 as i32,
                b2.as_mut_ptr(),
                s2 as i32,
            );
        }
        let mut golden = [0i16; 16];
        ema::dct_4x4(&mut golden, &PlaneCursor::new(&k1, 0, s1), &PlaneCursor::new(&k2, 0, s2));
        assert_eq!(dct, golden, "DctT4 shim vs direct at s1={s1} s2={s2}");
        assert_eq!((b1, b2), (k1, k2), "DctT4 shim moved a source byte");
    }
    for &(s1, s2) in &[(8usize, 16usize), (21, 8), (240, 25)] {
        let mut b1 = rng.bytes(7 * s1 + 8);
        let mut b2 = rng.bytes(7 * s2 + 8);
        let (k1, k2) = (b1.clone(), b2.clone());
        let mut dct = [0i16; 64];
        unsafe {
            ema::WelsDctFourT4_c(
                dct.as_mut_ptr(),
                b1.as_mut_ptr(),
                s1 as i32,
                b2.as_mut_ptr(),
                s2 as i32,
            );
        }
        let mut golden = [0i16; 64];
        ema::dct_four_4x4(&mut golden, &PlaneCursor::new(&k1, 0, s1), &PlaneCursor::new(&k2, 0, s2));
        assert_eq!(dct, golden, "DctFourT4 shim vs direct at s1={s1} s2={s2}");
        assert_eq!((b1, b2), (k1, k2), "DctFourT4 shim moved a source byte");
    }

    // --- Quantizers. **T9.D8 deleted the shims these rows used to compare
    // against**: the slots hold the safe kernels directly now, so a
    // "shim vs direct" assertion has nothing on its left-hand side. What is kept
    // is what was never tautological — the relations *between* the kernels, which
    // the walking cursors used to hide: `quant_four_4x4` is `quant_4x4` on each
    // quadrant, and `quant_four_4x4_max` is `quant_four_4x4` plus a per-quadrant
    // maximum. (F106's rule: a differential test whose two sides became the same
    // code is deleted, not re-pinned.)
    let ff = &ema::g_kiQuantInterFF[26];
    let mf = &ema::g_kiQuantMF[26];
    for _ in 0..scale(20) {
        let base: [i16; 64] = coeffs(&mut rng);

        let mut four = base;
        ema::quant_four_4x4(&mut four, ff, mf);
        for k in 0..4 {
            let mut one: [i16; 16] = base[k * 16..k * 16 + 16].try_into().unwrap();
            ema::quant_4x4(&mut one, ff, mf);
            assert_eq!(&four[k * 16..k * 16 + 16], &one[..], "QuantFour4x4 quadrant {k}");
        }

        let mut with_max = base;
        let mut g_max = [0i16; 4];
        ema::quant_four_4x4_max(&mut with_max, ff, mf, &mut g_max);
        assert_eq!(with_max, four, "QuantFour4x4Max wrote different coefficients");
        for k in 0..4 {
            let want = four[k * 16..k * 16 + 16].iter().map(|v| v.abs()).max().unwrap();
            assert_eq!(g_max[k], want, "QuantFour4x4Max quadrant {k} maximum");
            assert!(g_max[k] >= 0, "a max magnitude is never negative");
        }

        // The DC quantizer is the same dead zone with scalar factors.
        let sub: [i16; 16] = base[..16].try_into().unwrap();
        let mut dc = sub;
        ema::quant_4x4_dc(&mut dc, ff[0] << 1, mf[0] >> 1);
        let mut byhand = sub;
        for v in byhand.iter_mut() {
            let s = (*v as i32) >> 31;
            let abs = (s ^ (*v as i32)) - s;
            let q = ((((ff[0] << 1) as i32 + abs) * (mf[0] >> 1) as i32) >> 16) as i32;
            *v = ((s ^ q) - s) as i16;
        }
        assert_eq!(dc, byhand, "Quant4x4Dc dead zone");
    }

    // --- The 2x2 Hadamard pair: allocations of exactly 49 i16 (the declared
    // reach — a shim materializing 64 would be UB Miri reports), the touch
    // set pinned to {0, 16, 32, 48}, and the skip variant read-only.
    for _ in 0..scale(20) {
        let base: [i16; 49] = coeffs(&mut rng);
        let (ff, mf) = (ema::g_kiQuantInterFF[30][0] << 1, ema::g_kiQuantMF[30][0] >> 1);

        // The skip variant's read-only contract is now the `&[i16; 49]` parameter's
        // job rather than an assertion's. **The two kernels are deliberately not
        // cross-asserted**: `skip` thresholds on `(1<<16 - 1) / mf - ff` in `i32`
        // while the full kernel truncates each butterfly output to `i16` before
        // quantising, so "skip says nothing survives" and "the full kernel counted
        // zero" agree on every input the encoder produces but are not the same
        // predicate. An assertion that they are is a test asserting its author's
        // guess — this one did, and failed on the first random input.
        let _ = ema::hadamard_quant_2x2_skip(&base, ff, mf);

        let mut g = base;
        let (mut g_dct, mut g_blk) = ([0i16; 4], [0i16; 4]);
        let nnz = ema::hadamard_quant_2x2(&mut g, ff, mf, &mut g_dct, &mut g_blk);
        assert_eq!(g_dct, g_blk, "HadamardQuant2x2 writes the same four values twice");
        assert_eq!(
            nnz,
            g_dct.iter().filter(|&&v| v != 0).count() as i32,
            "the returned count is the non-zero count of what was written"
        );
        assert!((0..=4).contains(&nnz), "the 2x2 DC non-zero count is 0..=4");
        for (i, (&now, &was)) in g.iter().zip(base.iter()).enumerate() {
            if matches!(i, 0 | 16 | 32 | 48) {
                assert_eq!(now, 0, "HadamardQuant2x2 must zero its four DC slots");
            } else {
                assert_eq!(now, was, "HadamardQuant2x2 touched index {i}, outside its touch set");
            }
        }
    }

    // --- The DC Hadamard: source allocation of exactly 241 i16 (block 15's
    // DC at index 240 is the declared reach), source untouched.
    for _ in 0..scale(20) {
        let base: [i16; 241] = coeffs(&mut rng);
        let mut g = [0i16; 16];
        ema::hadamard_t4_dc(&mut g, &base);
        // The transform reads exactly the sixteen block DCs and nothing else:
        // zeroing every non-DC element must not change the result.
        let mut only_dc = [0i16; 241];
        for k in 0..16 {
            only_dc[k * 16] = base[k * 16];
        }
        let mut g2 = [0i16; 16];
        ema::hadamard_t4_dc(&mut g2, &only_dc);
        assert_eq!(g, g2, "HadamardT4Dc read outside the sixteen block DCs");
    }

    // --- Scans and scorers: exact 16-element views, sources untouched.
    for _ in 0..scale(20) {
        let dct: [i16; 16] = coeffs(&mut rng);
        let mut src = dct;

        let mut g = [0i16; 16];
        ema::scan_4x4_dc_ac(&mut g, &dct);
        let mut a = [0i16; 16];
        ema::WelsScan4x4Dc(&mut a, &src);
        assert_eq!(a, g, "Scan4x4Dc and Scan4x4DcAc disagree");

        // The AC scan is the DC/AC scan shifted by one zigzag position.
        let mut ac = [0i16; 16];
        ema::scan_4x4_ac(&mut ac, &dct);
        assert_eq!(&ac[..15], &g[1..], "Scan4x4Ac is not Scan4x4DcAc without the DC");

        // A scan is a permutation: the multiset of coefficients is preserved.
        let (mut sa, mut sb) = (g, dct);
        sa.sort_unstable();
        sb.sort_unstable();
        assert_eq!(sa, sb, "the zigzag scan is not a permutation");

        assert_eq!(
            ema::get_none_zero_count(&dct),
            dct.iter().filter(|&&v| v != 0).count() as i32,
            "GetNoneZeroCount"
        );
        let _ = ema::calculate_single_ctr_4x4(&dct);

        assert_eq!(src, dct, "a scan wrote through a read-only contract");
    }

    // --- The seven copies: exact spans both sides ((H-1)*stride + W), every
    // block byte equal to its source row (write-every-byte), bytes outside
    // the block untouched, source untouched.
    type RawCopy = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);
    let kernels: &[(&str, usize, usize, RawCopy)] = &[
        ("Copy4x4", 4, 4, ema::WelsCopy4x4_c),
        ("Copy8x4", 8, 4, ema::WelsCopy8x4_c),
        ("Copy4x8", 4, 8, ema::WelsCopy4x8_c),
        ("Copy8x8", 8, 8, ema::WelsCopy8x8_c),
        ("Copy16x8", 16, 8, ema::WelsCopy16x8_c),
        ("Copy8x16", 8, 16, ema::WelsCopy8x16_c),
        ("Copy16x16", 16, 16, ema::WelsCopy16x16_c),
    ];
    for &(name, w, h, raw) in kernels {
        for &ss in &[w, 21.max(w), 240] {
            for &ds in &[w, 29.max(w), 240] {
                let mut src = rng.bytes((h - 1) * ss + w);
                let mut dst = rng.bytes((h - 1) * ds + w);
                let src_before = src.clone();
                let dst_before = dst.clone();
                unsafe { raw(dst.as_mut_ptr(), ds as i32, src.as_mut_ptr(), ss as i32) };
                for y in 0..h {
                    assert_eq!(
                        &dst[y * ds..][..w],
                        &src_before[y * ss..][..w],
                        "{name} ss={ss} ds={ds}: row {y} not copied"
                    );
                    let tail = y * ds + w;
                    let next = ((y + 1) * ds).min(dst.len());
                    if tail < next {
                        assert_eq!(
                            &dst[tail..next],
                            &dst_before[tail..next],
                            "{name} ss={ss} ds={ds}: bytes beyond row {y}'s block moved"
                        );
                    }
                }
                assert_eq!(src, src_before, "{name} wrote its source");
            }
        }
    }
}

// ===========================================================================
// T7 — `encoder/decode_mb_aux.rs` (+ the five raw bodies in
// `svc_encode_mb.rs`): the encoder's recon IDCT and dequantisation family
// ===========================================================================
//
// `fc92dab0` proved all nine leaf kernels of the C++ `decode_mb_aux.cpp`
// against the raw ones: whole destination surfaces compared, sources
// asserted untouched, inputs per R-e (full `i16` range where the raw port is
// total, table MF factors, qp 0..12 for the gated luma-DC dequant, `+-2047`
// for `ihadamard_4x4_dc` per finding F11), one vertical-pass sign mutation
// killed. The shim commit deleted the equivalences; what survives is the
// span arithmetic, probed below with a golden direct run per shim and
// exact-span allocations.

use openh264_rs::encoder::decode_mb_aux as eda;
use openh264_rs::encoder::svc_encode_mb::g_kuiDequantCoeff;

/// Coefficients bounded to `+-bound`.
fn bounded_coeffs<const N: usize>(rng: &mut Prng, bound: i32) -> [i16; N] {
    core::array::from_fn(|_| rng.range_i32(-bound, bound) as i16)
}


#[test]
fn encoder_recon_shims_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0xEDA0_59A9);

    // Fixed-array shims: exact allocations, golden direct run.
    for _ in 0..scale(20) {
        let mf_row = &g_kuiDequantCoeff[usize::from(rng.below(52) as u16)];
        let base: [i16; 64] = coeffs(&mut rng);
        let sub: &[i16; 16] = (&base[..16]).try_into().unwrap();

        // **T9.D10 deleted the three dequantisation shims** these rows compared
        // against; what is left is the relation between the two survivors, which was
        // never tautological — `dequant_four_4x4` is `dequant_4x4` on each of the four
        // blocks. (F106's rule, as in T9.D8.)
        let mut g = base;
        eda::dequant_four_4x4(&mut g, mf_row);
        for k in 0..4 {
            let mut one: [i16; 16] = base[k * 16..k * 16 + 16].try_into().unwrap();
            eda::dequant_4x4(&mut one, mf_row);
            assert_eq!(&g[k * 16..k * 16 + 16], &one[..], "DequantFour4x4 block {k}");
        }

        // The inverse Hadamard with MF 1 spreads a lone DC evenly (gain 16 over the
        // two passes) and leaves an all-zero block alone.
        let mut dc = [0i16; 16];
        dc[0] = 5;
        eda::dequant_ihadamard_4x4(&mut dc, 1);
        assert!(dc.iter().all(|&v| v == 5), "{dc:?}");
        let mut zero = [0i16; 16];
        eda::dequant_ihadamard_4x4(&mut zero, mf_row[0] >> 2);
        assert_eq!(zero, [0i16; 16], "the inverse Hadamard of zero is zero");
        let _ = sub;

        let mut a4: [i16; 4] = coeffs(&mut rng);
        let mut g4 = a4;
        eda::WelsDequantIHadamard2x2Dc(&mut a4, mf_row[0]);
        eda::dequant_ihadamard_2x2_dc(&mut g4, mf_row[0]);
        assert_eq!(a4, g4, "DequantIHadamard2x2Dc shim vs direct");

        let qp = rng.below(12) as i32;
        let mut a = *sub;
        let mut g = *sub;
        eda::WelsDequantLumaDc4x4(&mut a, qp);
        eda::dequant_luma_dc_4x4(&mut g, qp);
        assert_eq!(a, g, "DequantLumaDc4x4 shim vs direct qp={qp}");

        let mut a: [i16; 16] = bounded_coeffs(&mut rng, 2047);
        let mut g = a;
        eda::WelsIHadamard4x4Dc(&mut a);
        eda::ihadamard_4x4_dc(&mut g);
        assert_eq!(a, g, "IHadamard4x4Dc shim vs direct");
    }

    // The recon IDCTs: exact spans on both surfaces ((H-1)*stride + W — an
    // over-claim is UB Miri reports), golden direct run pinning the anchor,
    // prediction and coefficients untouched, bytes outside the block
    // untouched.
    type RawRec = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32, *mut i16);
    fn probe_rec<const N: usize>(
        name: &str,
        rng: &mut Prng,
        w: usize,
        h: usize,
        rs: usize,
        ps: usize,
        raw: RawRec,
        direct: impl Fn(&mut PlaneCursorMut<'_>, &PlaneCursor<'_>, &[i16; N]),
    ) {
        let mut pred = rng.bytes((h - 1) * ps + w);
        let mut rec = rng.bytes((h - 1) * rs + w);
        let pred_before = pred.clone();
        let rec_before = rec.clone();
        let dct: [i16; N] = core::array::from_fn(|_| rng.range_i32(-32768, 32767) as i16);
        let mut m_dct = dct;

        unsafe {
            raw(
                rec.as_mut_ptr(),
                rs as i32,
                pred.as_mut_ptr(),
                ps as i32,
                m_dct.as_mut_ptr(),
            )
        };

        let mut golden = rec_before.clone();
        direct(
            &mut PlaneCursorMut::new(&mut golden, 0, rs),
            &PlaneCursor::new(&pred_before, 0, ps),
            &dct,
        );
        assert_eq!(rec, golden, "{name} rs={rs} ps={ps}: shim vs direct at the contract's anchor");
        assert_eq!((pred, m_dct), (pred_before, dct), "{name}: a source moved");
        for y in 0..h {
            let tail = y * rs + w;
            let next = ((y + 1) * rs).min(rec.len());
            if tail < next {
                assert_eq!(
                    &rec[tail..next],
                    &rec_before[tail..next],
                    "{name} rs={rs}: bytes beyond row {y}'s block moved"
                );
            }
        }
    }

    for &(rs, ps) in &[(4usize, 16usize), (21, 4), (240, 25)] {
        for _ in 0..scale(10) {
            probe_rec::<16>("IDctT4Rec", &mut rng, 4, 4, rs, ps, eda::WelsIDctT4Rec_c, eda::idct_t4_rec);
        }
    }
    for &(rs, ps) in &[(8usize, 16usize), (21, 8), (240, 25)] {
        for _ in 0..scale(10) {
            probe_rec::<64>("IDctFourT4Rec", &mut rng, 8, 8, rs, ps, eda::WelsIDctFourT4Rec_c, eda::idct_four_t4_rec);
        }
    }
    for &(rs, ps) in &[(16usize, 16usize), (21, 16), (240, 25)] {
        for _ in 0..scale(10) {
            probe_rec::<16>("IDctRecI16x16Dc", &mut rng, 16, 16, rs, ps, eda::WelsIDctRecI16x16Dc_c, eda::idct_rec_i16x16_dc);
        }
    }

    // The null-tolerance guard the C++ has and the recon shims keep.
    unsafe {
        eda::WelsIDctT4Rec_c(std::ptr::null_mut(), 4, std::ptr::null_mut(), 4, std::ptr::null_mut());
        eda::WelsIDctFourT4Rec_c(std::ptr::null_mut(), 8, std::ptr::null_mut(), 8, std::ptr::null_mut());
    }
}

// ===========================================================================
// T7 — `encoder/sample.rs`, the SATD family: PROVEN AND PARKED
// ===========================================================================
//
// No commit B exists for this family and none is planned before Phase 4a:
// D-perf-2's measurements are the tripwire projection (sad-class swap cost
// +16.8% median onto an encoder already ≈ +13% cumulative crosses +25%), so
// the safe kernels land proven and **uninstalled** — the raw
// `WelsSampleSatd*_c` bodies are the code that runs, exactly like the parked
// T5-sad family. These entries are therefore *live differentials*, not
// commit-A scaffolding: they outlive this session and die only when the
// family re-lands.
//
// F10's rule, applied from the start: the raw kernels bump their row
// pointers past the last row (`pSrc += iStride` after row 3), which is
// out-of-allocation pointer arithmetic on an exactly-sized buffer — so every
// **raw-side** buffer here spans whole rows (`h * stride`), and the
// **safe-side** exact-span probe below is the only place exact spans appear.
// This file runs under Miri at phase exits; whole-row raw buffers are what
// keep that run meaningful.

use openh264_rs::encoder::sample as satd;

type RawSatd = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32) -> i32;
type SafeSatd = fn(&PlaneCursor<'_>, &PlaneCursor<'_>) -> i32;

const SATD_SHAPES: &[(&str, usize, usize, RawSatd, SafeSatd)] = &[
    ("Satd4x4", 4, 4, satd::WelsSampleSatd4x4_c, satd::satd_4x4),
    ("Satd8x4", 8, 4, satd::WelsSampleSatd8x4_c, satd::satd_8x4),
    ("Satd4x8", 4, 8, satd::WelsSampleSatd4x8_c, satd::satd_4x8),
    ("Satd8x8", 8, 8, satd::WelsSampleSatd8x8_c, satd::satd_8x8),
    ("Satd16x8", 16, 8, satd::WelsSampleSatd16x8_c, satd::satd_16x8),
    ("Satd8x16", 8, 16, satd::WelsSampleSatd8x16_c, satd::satd_8x16),
    ("Satd16x16", 16, 16, satd::WelsSampleSatd16x16_c, satd::satd_16x16),
];

#[test]
fn satd_kernels_match_the_raw_ones() {
    let mut rng = Prng::new(0x5A7D_0007);

    for &(name, w, h, raw, safe) in SATD_SHAPES {
        for &(s1, s2) in &[(0usize, 0usize), (21, 0), (240, 25)] {
            let (s1, s2) = (s1.max(w), s2.max(w));
            for _ in 0..scale(60) {
                // Raw-side buffers sized to the raw kernels' pointer-arithmetic
                // footprint (F10) — and for the SATD *composites* that is more
                // than whole rows: each 4x4 sub-block's trailing bump computes
                // `sub_anchor + 4*stride`, and the bottom-right sub-block's
                // anchor is `c + (w-4) + (h-4)*stride`, so the farthest pointer
                // is `c + (w-4) + h*stride` — Miri flagged exactly this at
                // `h*stride` sizing. `(h+1)*stride` covers every legal anchor.
                let mut b1 = rng.bytes((h + 1) * s1);
                let mut b2 = rng.bytes((h + 1) * s2);
                let c1 = rng.below((s1 - w + 1) as u32) as usize;
                let c2 = rng.below((s2 - w + 1) as u32) as usize;

                let got_raw = unsafe {
                    raw(b1.as_mut_ptr().add(c1), s1 as i32, b2.as_mut_ptr().add(c2), s2 as i32)
                };
                let got_safe = satd_pair(&b1, c1, s1, &b2, c2, s2, safe);
                assert_eq!(
                    got_raw, got_safe,
                    "{name} s1={s1} s2={s2} c1={c1} c2={c2}, seed {:#x}",
                    rng.seed()
                );
            }
        }
    }
}

fn satd_pair(
    b1: &[u8],
    c1: usize,
    s1: usize,
    b2: &[u8],
    c2: usize,
    s2: usize,
    safe: SafeSatd,
) -> i32 {
    safe(&PlaneCursor::new(b1, c1, s1), &PlaneCursor::new(b2, c2, s2))
}

/// The safe kernels' declared reach is `(h-1)*stride + w` from the anchor on
/// each surface — exact-span allocations prove it is not under-claimed (an
/// over-read panics at the slice), and agreement with the same kernel run on
/// a padded copy of the same content proves the values do not depend on
/// anything outside the span. The raw kernels are deliberately absent here:
/// on exact spans their trailing pointer bump is F10's UB.
#[test]
fn satd_kernels_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0x5A7D_59A9);

    for &(name, w, h, _raw, safe) in SATD_SHAPES {
        for &(s1, s2) in &[(0usize, 0usize), (21, 25)] {
            let (s1, s2) = (s1.max(w), s2.max(w));
            for _ in 0..scale(20) {
                let exact1 = rng.bytes((h - 1) * s1 + w);
                let exact2 = rng.bytes((h - 1) * s2 + w);

                let got = safe(&PlaneCursor::new(&exact1, 0, s1), &PlaneCursor::new(&exact2, 0, s2));

                // Same content embedded in generously padded surfaces.
                let mut pad1 = rng.bytes(h * s1 + 64);
                let mut pad2 = rng.bytes(h * s2 + 64);
                pad1[..exact1.len()].copy_from_slice(&exact1);
                pad2[..exact2.len()].copy_from_slice(&exact2);
                let want = safe(&PlaneCursor::new(&pad1, 0, s1), &PlaneCursor::new(&pad2, 0, s2));

                assert_eq!(got, want, "{name} s1={s1} s2={s2}: exact-span disagreement");
            }
        }
    }
}

// ===========================================================================
// T9.C2 — the encoder intra-pred shim-span probes were retired here.
//
// This section held `safe_i4x4`/`safe_chroma`/`safe_i16x16`, `probe_intra` and
// `encoder_intra_pred_shims_stay_inside_the_spans_they_declare`. Every one of
// them existed to interrogate a **raw shim's hand-written span contract**: the
// twenty-eight `unsafe extern "C" fn(*mut u8, *mut u8, i32)` wrappers and the
// three helpers (`pred`, `top_row`, `reference`) they called. T9.C2 deleted all
// thirty-one, so the probes have no subject left — D-cov-1's shape, and F130's.
//
// The three properties they pinned, and what holds each now:
//
//   1. **Span tightness** — the reference allocation was sized to exactly
//      `ref_span`'s claim, so an over-claim was an out-of-bounds read Miri would
//      report. There is no longer a hand-written span to over-claim: the kernels
//      take a `RecCursor`, which bounds-checks every access against the whole
//      plane allocation, so an over-reach is a panic rather than UB. What the
//      probe could see and the bounds check cannot — an over-reach that stays
//      *inside* the plane but outside the mode's availability — was never this
//      probe's job either; it is
//      `reach_table_agrees_with_the_availability_tables`', and that test stands
//      unchanged in `encoder/get_intra_predictor.rs`. `ref_span` and the
//      `REACH_*` constants stay for it.
//
//   2. **Anchor** — the shim's output had to equal the safe kernel called at the
//      contract's own geometry, which killed an anchor off by a row or a column.
//      The shim *is* the safe kernel now, so the comparison is vacuous. The
//      geometry itself is still pinned, by the encoder-side unit tests that read
//      their expectations through the cursor (`i4x4_v_and_h_replicate_their_edge`
//      and its siblings) and, end to end, by the 583-row differential sweep — a
//      one-column anchor slip in `WelsI4x4LumaPredV_c` was planted in T9.C2f and
//      failed **210 of 210** `st` rows.
//
//   3. **Destination extent** — eight bytes of noise slack on each side of the
//      packed block, compared byte for byte, so a kernel writing one byte past
//      its block failed. The destination is `&mut [u8; N]` now: writing past it
//      does not compile.
//
// The two `common/intra_pred_common.rs` entries above went the same way and for
// the same reason; their kernels keep unit coverage in that module, rewritten in
// T9.C2f to drive the safe API over a `PaddedPlane`.
// ===========================================================================
// T9 straggler — `encoder/deblocking.rs`'s duplicate deblocking kernels (G-2)
//
// T6 converted the deblocking family in `common/deblocking_common.rs`, and the
// *decoder* installs those shims (`decoder/deblocking.rs:200` re-exports the
// module wholesale). The **encoder** does not: `encoder/deblocking.rs` carries
// its own copies of the same eight ABI wrappers over its own four inner
// kernels, and its own `DeblockingInit` installs them — so half the family was
// still live raw code on the encoder's mainline path. T9's straggler sweep
// found it.
//
// These entries prove the encoder's raw bodies against the safe kernels that
// already exist, which is what licenses the swap: the two sets differ only in
// local variable names, comments and one `*const`/`*mut` on `pTc`, and this
// says so by measurement rather than by reading. Entry shapes are T6's own,
// recovered from `bbb9348e` — the same three amplitude tiers (deblocking is
// *conditional*, and full-range noise almost never passes the gates), the same
// tc pool including the values that gate whole lines off, the same
// whole-buffer comparison.
//
// They go with the swap, as always.
// ===========================================================================

/// Alpha values from the ends and middle of `g_kuiAlphaTable` (0..=255), beta
/// values from `g_kiBetaTable` (0..=18). T6's set, recovered with the entries.
const ALPHAS: &[i32] = &[0, 1, 4, 20, 128, 255];
const BETAS: &[i32] = &[0, 2, 6, 18];

/// A noised surface of `rows * stride` with a per-call amplitude tier:
/// full-range, or narrow noise around a random base so the filter's conditions
/// actually pass. Deblocking is *conditional* — any alpha <= 255 is exceeded by
/// most random deltas, so full-range noise alone exercises only the skip path.
fn deblock_surface(rng: &mut Prng, rows: usize, stride: usize) -> Vec<u8> {
    match rng.below(3) {
        0 => rng.bytes(rows * stride),
        tier => {
            let amp = if tier == 1 { 16i32 } else { 3 };
            let base = rng.range_i32(amp, 255 - amp);
            (0..rows * stride).map(|_| (base + rng.range_i32(-amp, amp)) as u8).collect()
        }
    }
}

/// A random 4-entry tc0 group, biased to include the gate-closing values —
/// luma skips a line at `iTc0 < 0`, chroma at `iTc0 <= 0`, and that distinction
/// is exactly what a conversion gets wrong.
fn tc_group(rng: &mut Prng) -> [i8; 4] {
    let pool: &[i8] = &[-1, 0, 1, 2, 4, 9, 25];
    std::array::from_fn(|_| pool[rng.below(pool.len() as u32) as usize])
}

/// Anchor for an edge whose taps span `[-rb, rf]` along the tap axis and whose
/// `lines` lines run along the other. `vertical_taps` is the V-wrapper shape:
/// taps step by the stride, lines by one byte.
fn deblock_anchor(
    rng: &mut Prng,
    rows: usize,
    stride: usize,
    rb: usize,
    rf: usize,
    lines: usize,
    vertical_taps: bool,
) -> usize {
    let (row, col) = if vertical_taps {
        (rb + rng.below((rows - rb - rf) as u32) as usize,
         rng.below((stride - lines + 1) as u32) as usize)
    } else {
        (rng.below((rows - lines + 1) as u32) as usize,
         rb + rng.below((stride - rb - rf) as u32) as usize)
    };
    row * stride + col
}

/// Steps in bytes for one direction: V = `(stride, 1)`, H = `(1, stride)`.
fn steps(stride: usize, vertical_taps: bool) -> (isize, isize) {
    if vertical_taps { (stride as isize, 1) } else { (1, stride as isize) }
}

use openh264_rs::encoder::deblocking as encdeb;

// **Session F step 0: `encoder_deblocking_table_installs_the_common_shims`
// deleted with the eight slots it guarded.** F139 measured the encoder's eight
// `DeblockingFunc` kernel slots write-only — installed by `DeblockingInit`,
// read by nothing, because the `FilteringEdge*` dispatchers call the safe
// kernels directly since T9.C2 — and this test's whole property was "the
// installs aim at the common shims". With the slots gone there is no install
// left to misdirect and no reader a misdirection could reach; the property is
// void, not merely stale — D-cov-1's `mc_table_slots_match_the_direct_calls`
// reasoning, one block below. The decoder-side behavioural assert-map
// (`deblocking_table_slots_match_the_direct_calls`) stays: it drives the
// *common* table, which the decoder still dispatches through.

/// The remaining copies of `WelsNonZeroCount_c`: `common` has the shim over the
/// safe kernel, and `encoder/deblocking.rs` keeps a raw `extern "C"` duplicate
/// because `SWelsFuncPtrList::pfSetNZCZero` is still an `Option<fn>` slot and a
/// plain `unsafe fn` cannot fill one. They must agree.
///
/// **This test had a third arm until T4b.3c**, over `decoder/decode_slice.rs`'s
/// copy — the one that never got Phase 2's conversion and was still a hand-written
/// `if *p != 0 { *p = 1 }` loop. T4b.3c deleted `sBlockFunc`, the table that
/// installed it, and pointed its single reader at the `common` shim; the copy went
/// with the table. The arm is dropped rather than the test, because the duplicate
/// it guarded no longer exists — and it is worth recording that **this test is what
/// proved the deletion safe**, having asserted the two bodies byte-equal over 50
/// random 24-entry inputs since Phase 2. It also caught the deletion, by failing to
/// compile.
#[test]
fn nonzero_count_duplicates_agree() {
    let mut rng = Prng::new(0x0E0F_0003);
    for _ in 0..scale(50) {
        let seed: Vec<i8> = (0..24).map(|_| rng.range_i32(-128, 127) as i8).collect();
        let mut a: [i8; 24] = seed.clone().try_into().unwrap();
        let mut c: [i8; 24] = seed.try_into().unwrap();
        encdeb::WelsNonZeroCount_c(&mut a);
        deb::nonzero_count(&mut c);
        assert_eq!(a, c, "encoder copy disagrees with the safe kernel");
    }
}

// ============================================================================
// Phase 4a — dispatch assert-maps
// ============================================================================
//
// Plan §5's own mitigation for de-virtualization: *every replaced pointer
// provably pointed at exactly one function per config*. These tests are how
// that is proven, and they are written **before** the table they describe is
// deleted, not after.
//
// The claim splits in two, and the split is forced rather than stylistic:
//
//   1. The installer ignores its CPU-flag argument, so there is one function
//      per slot rather than a family selected at run time. Proven by address
//      equality across flag values, inside the library, where both addresses
//      come from the same instantiation — `common::mc::tests`,
//      `init_mc_func_ignores_the_cpu_flag`.
//   2. That one function is the one the direct call sites now name. Proven
//      **behaviourally**, here.
//
// Half 2 cannot be an address comparison. `#[inline(always)]` functions are
// instantiated in whatever codegen unit takes their address, so this crate's
// `mc::McLuma_c as usize` is a local copy, not the pointer `InitMcFunc` stored;
// four of the six MC shims are `#[inline(always)]` and the assert fails on all
// four. `encoder_deblocking_table_installs_the_common_shims` above only works
// cross-crate because its kernels happen to carry no inline attribute. Driving
// both sides over the same inputs and comparing every written byte is immune to
// that, and it is the property the call sites actually depend on.

// **D-cov-1 (T9.B4): `mc_table_slots_match_the_direct_calls` deleted.** Phase 4a's
// dispatch assert-map, driving `SMcFunc`'s six installed slots against the symbols the
// de-virtualized call sites name. It is spent, not merely stale: the slots hold the
// safe kernels themselves now (`mc_luma`, `mc_chroma`, `mc_hor_ver20`/`_02`/`_22`,
// `pixel_avg`), so "the slot and the direct call agree" has become "`mc_luma ==
// mc_luma`". The mistake it existed to catch — a slot re-pointed at a
// different-but-plausible function — is now a type error at `InitMcFunc`, because the
// three slot types name the safe signatures and nothing else has them.
//
// `init_mc_func_ignores_the_cpu_flag` and
// `mc_table_is_all_none_before_init_and_all_some_after` (both in `common/mc.rs`) stay:
// the flag-invariance and all-None-then-all-Some halves are still real properties of
// `InitMcFunc`, and they are what `encoder_context.rs:2464`'s construction assertion
// leans on. F124 was right that this test is Phase 4a's mitigation rather than Phase
// 2's span discipline, and it is retired here with its own reason.

/// Every `SDeblockingFunc` slot computes exactly what the function the direct
/// call sites name computes.
///
/// The behavioural half of the deblocking assert-map; see
/// `deblocking_init_ignores_the_cpu_flag` in the library for the flag-invariance
/// half and for why the two are split. Unlike
/// `encoder_deblocking_table_installs_the_common_shims` above — which compares
/// addresses and works only because these kernels happen to carry no inline
/// attribute — this compares outputs, so it keeps working if one ever gains
/// `#[inline]`.
///
/// The whole surface is compared, not the filtered lines: a kernel that writes
/// one sample outside its edge is exactly what this class of test exists for.
/// `tc` is swept over its negative gate (a negative entry means "do not filter
/// this group") as well as real thresholds, because that selector decides
/// whether the kernel touches memory at all — S10's rule that selectors are
/// swept, not sampled.
#[test]
fn deblocking_table_slots_match_the_direct_calls() {
    let mut rng = Prng::new(0x04A0_0002);
    let mut t = deb::SDeblockingFunc::default();
    unsafe { deb::DeblockingInit(&mut t, 0) };

    const STRIDE: usize = 64;
    const ROWS: usize = 48;
    // The strong luma filter reads p3 at -4*step_x; the widest reach in either
    // axis is 4 samples and edges run 16 lines, so an anchor 8 samples and 8
    // rows in, on a 64x48 surface, keeps every access inside for both the V
    // (step_x = stride) and H (step_x = 1) orientations.
    const ANCHOR: usize = 8 * STRIDE + 8;

    /// `(slot, direct)` over one surface, with a `tc` gate.
    macro_rules! cmp1_tc {
        ($base:expr, $tc:expr, $alpha:expr, $beta:expr, $slot:expr, $direct:expr, $name:literal) => {{
            let (mut a, mut b) = ($base.clone(), $base.clone());
            let (mut ta, mut tb) = ($tc, $tc);
            unsafe {
                $slot(a.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta, ta.as_mut_ptr());
                $direct(b.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta, tb.as_mut_ptr());
            }
            assert_eq!(a, b, concat!($name, ": slot vs direct disagree"));
        }};
    }
    /// `(slot, direct)` over one surface, no `tc`.
    macro_rules! cmp1 {
        ($base:expr, $alpha:expr, $beta:expr, $slot:expr, $direct:expr, $name:literal) => {{
            let (mut a, mut b) = ($base.clone(), $base.clone());
            unsafe {
                $slot(a.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta);
                $direct(b.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta);
            }
            assert_eq!(a, b, concat!($name, ": slot vs direct disagree"));
        }};
    }
    /// `(slot, direct)` over the Cb/Cr pair, with a `tc` gate.
    macro_rules! cmp2_tc {
        ($base:expr, $tc:expr, $alpha:expr, $beta:expr, $slot:expr, $direct:expr, $name:literal) => {{
            let (mut acb, mut acr) = ($base.clone(), $base.clone());
            let (mut bcb, mut bcr) = ($base.clone(), $base.clone());
            let (mut ta, mut tb) = ($tc, $tc);
            unsafe {
                $slot(acb.as_mut_ptr().add(ANCHOR), acr.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta, ta.as_mut_ptr());
                $direct(bcb.as_mut_ptr().add(ANCHOR), bcr.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta, tb.as_mut_ptr());
            }
            assert_eq!(acb, bcb, concat!($name, ": Cb slot vs direct disagree"));
            assert_eq!(acr, bcr, concat!($name, ": Cr slot vs direct disagree"));
        }};
    }
    /// `(slot, direct)` over the Cb/Cr pair, no `tc`.
    macro_rules! cmp2 {
        ($base:expr, $alpha:expr, $beta:expr, $slot:expr, $direct:expr, $name:literal) => {{
            let (mut acb, mut acr) = ($base.clone(), $base.clone());
            let (mut bcb, mut bcr) = ($base.clone(), $base.clone());
            unsafe {
                $slot(acb.as_mut_ptr().add(ANCHOR), acr.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta);
                $direct(bcb.as_mut_ptr().add(ANCHOR), bcr.as_mut_ptr().add(ANCHOR), STRIDE as i32, $alpha, $beta);
            }
            assert_eq!(acb, bcb, concat!($name, ": Cb slot vs direct disagree"));
            assert_eq!(acr, bcr, concat!($name, ": Cr slot vs direct disagree"));
        }};
    }

    for trial in 0..scale(200) {
        let alpha = rng.range_i32(0, 255);
        let beta = rng.range_i32(0, 18);
        let tc: [i8; 4] = match trial % 4 {
            0 => [-1, -1, -1, -1],           // gate closed on every group
            1 => [0, 0, 0, 0],               // open, but no clipping headroom
            2 => [-1, 3, -1, 12],            // mixed — the case a uniform sweep misses
            _ => [
                rng.range_i32(0, 25) as i8, rng.range_i32(0, 25) as i8,
                rng.range_i32(0, 25) as i8, rng.range_i32(0, 25) as i8,
            ],
        };
        let base: Vec<u8> = (0..STRIDE * ROWS).map(|_| rng.range_i32(0, 255) as u8).collect();

        cmp1_tc!(base, tc, alpha, beta, t.pfLumaDeblockingLT4Ver.unwrap(), deb::DeblockLumaLt4V_c, "LumaLT4Ver");
        cmp1_tc!(base, tc, alpha, beta, t.pfLumaDeblockingLT4Hor.unwrap(), deb::DeblockLumaLt4H_c, "LumaLT4Hor");
        cmp1!(base, alpha, beta, t.pfLumaDeblockingEQ4Ver.unwrap(), deb::DeblockLumaEq4V_c, "LumaEQ4Ver");
        cmp1!(base, alpha, beta, t.pfLumaDeblockingEQ4Hor.unwrap(), deb::DeblockLumaEq4H_c, "LumaEQ4Hor");

        cmp2_tc!(base, tc, alpha, beta, t.pfChromaDeblockingLT4Ver.unwrap(), deb::DeblockChromaLt4V_c, "ChromaLT4Ver");
        cmp2_tc!(base, tc, alpha, beta, t.pfChromaDeblockingLT4Hor.unwrap(), deb::DeblockChromaLt4H_c, "ChromaLT4Hor");
        cmp2!(base, alpha, beta, t.pfChromaDeblockingEQ4Ver.unwrap(), deb::DeblockChromaEq4V_c, "ChromaEQ4Ver");
        cmp2!(base, alpha, beta, t.pfChromaDeblockingEQ4Hor.unwrap(), deb::DeblockChromaEq4H_c, "ChromaEQ4Hor");

        cmp1_tc!(base, tc, alpha, beta, t.pfChromaDeblockingLT4Ver2.unwrap(), deb::DeblockChromaLt4V2_c, "ChromaLT4Ver2");
        cmp1_tc!(base, tc, alpha, beta, t.pfChromaDeblockingLT4Hor2.unwrap(), deb::DeblockChromaLt4H2_c, "ChromaLT4Hor2");
        cmp1!(base, alpha, beta, t.pfChromaDeblockingEQ4Ver2.unwrap(), deb::DeblockChromaEq4V2_c, "ChromaEQ4Ver2");
        cmp1!(base, alpha, beta, t.pfChromaDeblockingEQ4Hor2.unwrap(), deb::DeblockChromaEq4H2_c, "ChromaEQ4Hor2");
    }
}
