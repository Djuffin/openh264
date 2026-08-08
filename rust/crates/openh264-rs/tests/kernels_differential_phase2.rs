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
// The first family that reads one plane and writes another, and the first with
// encoder-side consumers. Every entry below drives the old raw kernel and the new
// safe one over the same noise and compares the **whole** destination buffer.
//
// Three things these tests do on purpose, per the carry-forwards the intra-prediction
// family left (`safety_refactor_log.md`, Phase 2 entry):
//
//   * **Selector sweeps are exhaustive, not random.** `McLuma_c` picks one of sixteen
//     kernels from `(iMvX & 3, iMvY & 3)` and `McChroma_c` one of sixty-five weight
//     sets from `(iMvX & 7, iMvY & 7)`; a random sweep blurs exactly the distinction a
//     conversion is most likely to get wrong, so both are driven over every value —
//     including negative MVs, where `&` on a signed type is the whole trick.
//   * **Anchors are random and unaligned** in both the source and the destination.
//   * **Source geometry is per-kernel**, sized to that kernel's exact read reach, which
//     is the same table the shims will encode. `McHorVer13_c` reaching two rows above
//     and three below is not the same fact as `McHorVer10_c` reaching neither.
//
// Size lists differ by kernel for a reason that is not cosmetic: the half-pel kernels
// are called by the encoder's ME refinement at `iWidth + 1` / `iHeight + 1`, i.e. up
// to 17 (`encoder/md.rs:1196,1229,1289`), while the quarter-pel composites interpolate
// through a `uint8_t[256]` scratch at stride 16 and are therefore 16x16 at most.

use openh264_rs::common::mc;
use openh264_rs::safe::plane::PlaneCursor;

/// `(left, top, right, bottom)`: the kernel reads `x` in `-left .. width + right`
/// and `y` in `-top .. height + bottom`, relative to `pSrc`.
type Reach = (usize, usize, usize, usize);

const R_COPY: Reach = (0, 0, 0, 0);
const R_HOR: Reach = (2, 0, 3, 0);
const R_VER: Reach = (0, 2, 0, 3);
const R_CEN: Reach = (2, 2, 3, 3);

/// Luma MC block shapes, plus the encoder's `+1` half-pel refinement shapes.
const HALFPEL_SIZES: &[(usize, usize)] = &[
    (16, 16),
    (16, 8),
    (8, 16),
    (8, 8),
    (8, 4),
    (4, 8),
    (4, 4),
    (17, 16),
    (16, 17),
    (17, 17),
    (9, 8),
    (5, 4),
    (2, 2),
];

/// The quarter-pel composites' scratch is `[u8; 256]` at stride 16, so 16 is the hard
/// ceiling in both dimensions — the same ceiling the C++ has.
const QPEL_SIZES: &[(usize, usize)] = &[
    (16, 16),
    (16, 8),
    (8, 16),
    (8, 8),
    (8, 4),
    (4, 8),
    (4, 4),
    (2, 2),
];

/// Chroma shapes: half the luma partition sizes.
const CHROMA_SIZES: &[(usize, usize)] = &[(8, 8), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)];

/// `McCopy_c` dispatches on the *exact* width and copies two bytes for anything that
/// is not 16, 8 or 4 — so the list has to contain widths that are none of those.
const COPY_SIZES: &[(usize, usize)] = &[(16, 16), (8, 8), (4, 4), (2, 2), (6, 4), (3, 2), (12, 3)];

/// Strides worth driving: the minimum legal one (where an off-by-one in the span
/// arithmetic shows up first), a non-multiple of 16, and a real picture stride.
fn strides(min: usize) -> Vec<usize> {
    let mut v: Vec<usize> = [min, min + 7, 32, 240]
        .into_iter()
        .filter(|&s| s >= min.max(1))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// A noise-filled source surface and a random legal anchor for a `width` x `height`
/// block whose kernel reads `reach` beyond it.
fn src_surface(
    rng: &mut Prng,
    stride: usize,
    width: usize,
    height: usize,
    reach: Reach,
) -> (Vec<u8>, usize) {
    let (left, top, right, bottom) = reach;
    let rows = top + height + bottom + 2;
    let buf = rng.bytes(rows * stride);
    let row = top + rng.below((rows - (top + height + bottom)) as u32) as usize;
    let col = left + rng.below((stride - (left + width + right)) as u32 + 1) as usize;
    (buf, row * stride + col)
}

/// A noise-filled destination surface and a random legal anchor. Noise rather than a
/// constant so a write on the wrong row cannot hide.
fn dst_surface(rng: &mut Prng, stride: usize, width: usize, height: usize) -> (Vec<u8>, usize) {
    let rows = height + 2;
    let buf = rng.bytes(rows * stride);
    let row = rng.below((rows - height) as u32 + 1) as usize;
    let col = rng.below((stride - width) as u32 + 1) as usize;
    (buf, row * stride + col)
}

/// The `(pSrc, iSrcStride, pDst, iDstStride, iWidth, iHeight)` shape — seventeen of
/// this family's kernels share it.
type OldWh = unsafe extern "C" fn(*const u8, i32, *mut u8, i32, i32, i32);
type NewWh = fn(&PlaneCursor<'_>, &mut PlaneCursorMut<'_>, usize, usize);

fn check_wh(name: &str, old: OldWh, new: NewWh, reach: Reach, sizes: &[(usize, usize)], seed: u64) {
    let mut rng = Prng::new(seed);
    let (left, _, right, _) = reach;
    for &(w, h) in sizes {
        for &ss in &strides(left + w + right) {
            for &ds in &strides(w) {
                for _ in 0..scale(30) {
                    let (src, sc) = src_surface(&mut rng, ss, w, h, reach);
                    let (dst, dc) = dst_surface(&mut rng, ds, w, h);
                    let mut a = dst.clone();
                    let mut b = dst;
                    unsafe {
                        old(
                            src.as_ptr().add(sc),
                            ss as i32,
                            a.as_mut_ptr().add(dc),
                            ds as i32,
                            w as i32,
                            h as i32,
                        );
                    }
                    new(
                        &PlaneCursor::new(&src, sc, ss),
                        &mut PlaneCursorMut::new(&mut b, dc, ds),
                        w,
                        h,
                    );
                    assert_eq!(
                        a,
                        b,
                        "{name} {w}x{h} src_stride {ss} anchor {sc} / dst_stride {ds} \
                         anchor {dc}, seed {:#x}",
                        rng.seed()
                    );
                }
            }
        }
    }
}

#[test]
fn mc_width_height_kernels_match_the_raw_ones() {
    let cases: &[(&str, OldWh, NewWh, Reach, &[(usize, usize)])] = &[
        ("McCopy_c", mc::McCopy_c, mc::mc_copy, R_COPY, COPY_SIZES),
        ("McHorVer20_c", mc::McHorVer20_c, mc::mc_hor_ver20, R_HOR, HALFPEL_SIZES),
        ("McHorVer02_c", mc::McHorVer02_c, mc::mc_hor_ver02, R_VER, HALFPEL_SIZES),
        ("McHorVer22_c", mc::McHorVer22_c, mc::mc_hor_ver22, R_CEN, HALFPEL_SIZES),
        ("McHorizLuma_c", mc::McHorizLuma_c, mc::mc_hor_ver20, R_HOR, HALFPEL_SIZES),
        ("McVertLuma_c", mc::McVertLuma_c, mc::mc_hor_ver02, R_VER, HALFPEL_SIZES),
        ("McHorVer01_c", mc::McHorVer01_c, mc::mc_hor_ver01, R_VER, QPEL_SIZES),
        ("McHorVer03_c", mc::McHorVer03_c, mc::mc_hor_ver03, R_VER, QPEL_SIZES),
        ("McHorVer10_c", mc::McHorVer10_c, mc::mc_hor_ver10, R_HOR, QPEL_SIZES),
        ("McHorVer11_c", mc::McHorVer11_c, mc::mc_hor_ver11, R_CEN, QPEL_SIZES),
        ("McHorVer12_c", mc::McHorVer12_c, mc::mc_hor_ver12, R_CEN, QPEL_SIZES),
        ("McHorVer13_c", mc::McHorVer13_c, mc::mc_hor_ver13, R_CEN, QPEL_SIZES),
        ("McHorVer21_c", mc::McHorVer21_c, mc::mc_hor_ver21, R_CEN, QPEL_SIZES),
        ("McHorVer23_c", mc::McHorVer23_c, mc::mc_hor_ver23, R_CEN, QPEL_SIZES),
        ("McHorVer30_c", mc::McHorVer30_c, mc::mc_hor_ver30, R_HOR, QPEL_SIZES),
        ("McHorVer31_c", mc::McHorVer31_c, mc::mc_hor_ver31, R_CEN, QPEL_SIZES),
        ("McHorVer32_c", mc::McHorVer32_c, mc::mc_hor_ver32, R_CEN, QPEL_SIZES),
        ("McHorVer33_c", mc::McHorVer33_c, mc::mc_hor_ver33, R_CEN, QPEL_SIZES),
    ];
    for (i, &(name, old, new, reach, sizes)) in cases.iter().enumerate() {
        check_wh(name, old, new, reach, sizes, 0x4C40_0000 + i as u64);
    }
}

/// The four fixed-width copy kernels — `(pSrc, iSrcStride, pDst, iDstStride, iHeight)`,
/// with the width baked into the name.
#[test]
fn mc_fixed_width_copy_kernels_match_the_raw_ones() {
    type Old = unsafe extern "C" fn(*const u8, i32, *mut u8, i32, i32);
    type New = fn(&PlaneCursor<'_>, &mut PlaneCursorMut<'_>, usize);
    let cases: &[(&str, Old, New, usize)] = &[
        ("McCopyWidthEq2_c", mc::McCopyWidthEq2_c, mc::mc_copy_width_eq2, 2),
        ("McCopyWidthEq4_c", mc::McCopyWidthEq4_c, mc::mc_copy_width_eq4, 4),
        ("McCopyWidthEq8_c", mc::McCopyWidthEq8_c, mc::mc_copy_width_eq8, 8),
        ("McCopyWidthEq16_c", mc::McCopyWidthEq16_c, mc::mc_copy_width_eq16, 16),
    ];
    let mut rng = Prng::new(0x4C40_0100);
    for &(name, old, new, w) in cases {
        for &h in &[1usize, 2, 4, 8, 16, 17] {
            for &ss in &strides(w) {
                for &ds in &strides(w) {
                    for _ in 0..scale(30) {
                        let (src, sc) = src_surface(&mut rng, ss, w, h, R_COPY);
                        let (dst, dc) = dst_surface(&mut rng, ds, w, h);
                        let mut a = dst.clone();
                        let mut b = dst;
                        unsafe {
                            old(
                                src.as_ptr().add(sc),
                                ss as i32,
                                a.as_mut_ptr().add(dc),
                                ds as i32,
                                h as i32,
                            );
                        }
                        new(
                            &PlaneCursor::new(&src, sc, ss),
                            &mut PlaneCursorMut::new(&mut b, dc, ds),
                            h,
                        );
                        assert_eq!(
                            a, b,
                            "{name} h={h} src_stride {ss}/{sc} dst_stride {ds}/{dc}, seed {:#x}",
                            rng.seed()
                        );
                    }
                }
            }
        }
    }
}

/// `PixelAvg_c` — three surfaces, three independent strides. The encoder averages a
/// `ME_REFINE_BUF_STRIDE` scratch against the reference picture, so the strides really
/// do differ at a live call site (`encoder/md.rs:1059`).
#[test]
fn pixel_avg_matches_the_raw_one() {
    let mut rng = Prng::new(0x4C40_0200);
    for &(w, h) in HALFPEL_SIZES {
        for &sa in &strides(w) {
            for &sb in &strides(w) {
                for &ds in &strides(w) {
                    for _ in 0..scale(20) {
                        let (pa, ca) = src_surface(&mut rng, sa, w, h, R_COPY);
                        let (pb, cb) = src_surface(&mut rng, sb, w, h, R_COPY);
                        let (dst, dc) = dst_surface(&mut rng, ds, w, h);
                        let mut a = dst.clone();
                        let mut b = dst;
                        unsafe {
                            mc::PixelAvg_c(
                                a.as_mut_ptr().add(dc),
                                ds as i32,
                                pa.as_ptr().add(ca),
                                sa as i32,
                                pb.as_ptr().add(cb),
                                sb as i32,
                                w as i32,
                                h as i32,
                            );
                        }
                        mc::pixel_avg(
                            &mut PlaneCursorMut::new(&mut b, dc, ds),
                            &PlaneCursor::new(&pa, ca, sa),
                            &PlaneCursor::new(&pb, cb, sb),
                            w,
                            h,
                        );
                        assert_eq!(a, b, "PixelAvg_c {w}x{h} {sa}/{sb}/{ds}, seed {:#x}", rng.seed());
                    }
                }
            }
        }
    }
}

/// `McLuma_c` over **every** `(iMvX & 3, iMvY & 3)` pair, driven from both a positive
/// and a negative MV so the sign behaviour of `&` on `i16` is pinned rather than
/// assumed. This is the entry that proves the folded quarter-pel dispatch: the safe
/// side is a `match`, the old side is the `pWelsMcFunc_c[[fn; 4]; 4]` table, and if
/// the two disagree about which kernel an MV selects, only an exhaustive sweep sees it.
#[test]
fn mc_luma_matches_the_raw_one_over_every_quarter_pel_phase() {
    let mut rng = Prng::new(0x4C40_0300);
    for &(w, h) in QPEL_SIZES {
        for &ss in &strides(2 + w + 3) {
            for &ds in &strides(w) {
                for phase in 0..16i16 {
                    // Same (x & 3, y & 3) from a positive and a negative vector.
                    let mvs = [
                        (phase & 3, phase >> 2),
                        ((phase & 3) - 64, (phase >> 2) - 64),
                    ];
                    for (mvx, mvy) in mvs {
                        for _ in 0..scale(6) {
                            let (src, sc) = src_surface(&mut rng, ss, w, h, R_CEN);
                            let (dst, dc) = dst_surface(&mut rng, ds, w, h);
                            let mut a = dst.clone();
                            let mut b = dst;
                            unsafe {
                                mc::McLuma_c(
                                    src.as_ptr().add(sc),
                                    ss as i32,
                                    a.as_mut_ptr().add(dc),
                                    ds as i32,
                                    mvx,
                                    mvy,
                                    w as i32,
                                    h as i32,
                                );
                            }
                            mc::mc_luma(
                                &PlaneCursor::new(&src, sc, ss),
                                &mut PlaneCursorMut::new(&mut b, dc, ds),
                                mvx,
                                mvy,
                                w,
                                h,
                            );
                            assert_eq!(
                                a, b,
                                "McLuma_c {w}x{h} mv ({mvx},{mvy}) {ss}/{ds}, seed {:#x}",
                                rng.seed()
                            );
                        }
                    }
                }
            }
        }
    }
}

/// `McChroma_c` and `McChromaWithFragMv_c` over **every** `(iMvX & 7, iMvY & 7)` pair —
/// all 64 weight quadruples of `g_kuiABCD`, including the `(0, 0)` one where
/// `McChroma_c` takes the copy path instead and `McChromaWithFragMv_c` does not.
#[test]
fn mc_chroma_matches_the_raw_ones_over_every_eighth_pel_phase() {
    let mut rng = Prng::new(0x4C40_0400);
    for &(w, h) in CHROMA_SIZES {
        for &ss in &strides(w + 1) {
            for &ds in &strides(w) {
                for phase in 0..64i16 {
                    let mvs = [(phase & 7, phase >> 3), ((phase & 7) - 64, (phase >> 3) - 64)];
                    for (mvx, mvy) in mvs {
                        for _ in 0..scale(3) {
                            let (src, sc) = src_surface(&mut rng, ss, w, h, (0, 0, 1, 1));
                            let (dst, dc) = dst_surface(&mut rng, ds, w, h);
                            let (mut a, mut b) = (dst.clone(), dst.clone());
                            let (mut c, mut d) = (dst.clone(), dst);
                            unsafe {
                                mc::McChroma_c(
                                    src.as_ptr().add(sc),
                                    ss as i32,
                                    a.as_mut_ptr().add(dc),
                                    ds as i32,
                                    mvx,
                                    mvy,
                                    w as i32,
                                    h as i32,
                                );
                                mc::McChromaWithFragMv_c(
                                    src.as_ptr().add(sc),
                                    ss as i32,
                                    c.as_mut_ptr().add(dc),
                                    ds as i32,
                                    mvx,
                                    mvy,
                                    w as i32,
                                    h as i32,
                                );
                            }
                            mc::mc_chroma(
                                &PlaneCursor::new(&src, sc, ss),
                                &mut PlaneCursorMut::new(&mut b, dc, ds),
                                mvx,
                                mvy,
                                w,
                                h,
                            );
                            mc::mc_chroma_with_frag_mv(
                                &PlaneCursor::new(&src, sc, ss),
                                &mut PlaneCursorMut::new(&mut d, dc, ds),
                                mvx,
                                mvy,
                                w,
                                h,
                            );
                            assert_eq!(
                                a, b,
                                "McChroma_c {w}x{h} mv ({mvx},{mvy}) {ss}/{ds}, seed {:#x}",
                                rng.seed()
                            );
                            assert_eq!(
                                c, d,
                                "McChromaWithFragMv_c {w}x{h} mv ({mvx},{mvy}) {ss}/{ds}, \
                                 seed {:#x}",
                                rng.seed()
                            );
                        }
                    }
                }
            }
        }
    }
}
