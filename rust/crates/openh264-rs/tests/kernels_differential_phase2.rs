//! Differential tests for the kernel conversions.

mod common;

use common::prng::Prng;
use openh264_rs::decoder::decode_mb_aux as dec_aux;
use openh264_rs::safe::plane::PlaneCursorMut;

/// Sample sizes are cut hard under Miri, which runs ~100x slower. The *shapes* are
/// identical either way — every stride, every boundary case — only the PRNG sample
/// counts shrink, and the full-size run happens on every `cargo test`.
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
/// comparison in which some `nzc` happened to be non-zero, so it gets its own entry.
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
// `common/mc.rs`, motion compensation
// ===========================================================================

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

// ===========================================================================
// `common/sad_common.rs` + `common/intra_pred_common.rs`
// ===========================================================================

use openh264_rs::common::sad_common as sad;
use openh264_rs::safe::plane::PlaneCursor;
use openh264_rs::encoder::rec_view::RecCursor;

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
        (4, 4) => sad::sample_sad::<4, 4, _>(c1, c2),
        (8, 4) => sad::sample_sad::<8, 4, _>(c1, c2),
        (4, 8) => sad::sample_sad::<4, 8, _>(c1, c2),
        (8, 8) => sad::sample_sad::<8, 8, _>(c1, c2),
        (16, 8) => sad::sample_sad::<16, 8, _>(c1, c2),
        (8, 16) => sad::sample_sad::<8, 16, _>(c1, c2),
        _ => sad::sample_sad::<16, 16, _>(c1, c2),
    }
}

fn safe_sad_four(w: usize, h: usize, c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>, out: &mut [i32; 4]) {
    match (w, h) {
        (4, 4) => sad::sample_sad_four::<4, 4, _>(c1, c2, out),
        (8, 4) => sad::sample_sad_four::<8, 4, _>(c1, c2, out),
        (4, 8) => sad::sample_sad_four::<4, 8, _>(c1, c2, out),
        (8, 8) => sad::sample_sad_four::<8, 8, _>(c1, c2, out),
        (16, 8) => sad::sample_sad_four::<16, 8, _>(c1, c2, out),
        (8, 16) => sad::sample_sad_four::<8, 16, _>(c1, c2, out),
        _ => sad::sample_sad_four::<16, 16, _>(c1, c2, out),
    }
}

/// The safe SAD kernels' declared reach, proven by exact-span allocations: an
/// over-read panics at the slice, and agreement with the same content in a
/// padded surface proves the values depend on nothing outside the span.
/// `sample_sad` reads `(h-1)*stride + w` from its anchor; `sample_sad_four`'s
/// reference side reads one row and one column beyond the block on every side.
/// The four-point arms are also checked to be four *distinct* points scoring
/// inside their own block bound.
#[test]
fn sad_kernels_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0x5AD0_0501);

    const SAD_SHAPES: &[(usize, usize)] = &[(4, 4), (8, 4), (4, 8), (8, 8), (16, 8), (8, 16), (16, 16)];

    for &(w, h) in SAD_SHAPES {
        for &s1 in &strides(w) {
            for &s2 in &strides(w) {
                let exact1 = rng.bytes((h - 1) * s1 + w);
                let exact2 = rng.bytes((h - 1) * s2 + w);
                let got = safe_sad(w, h, &PlaneCursor::new(&exact1, 0, s1), &PlaneCursor::new(&exact2, 0, s2));
                assert!(
                    (0..=(w * h * 255) as i32).contains(&got),
                    "sad {w}x{h}: {got} outside [0, {}] at strides {s1}/{s2}",
                    w * h * 255
                );

                let mut pad1 = rng.bytes(h * s1 + 64);
                let mut pad2 = rng.bytes(h * s2 + 64);
                pad1[..exact1.len()].copy_from_slice(&exact1);
                pad2[..exact2.len()].copy_from_slice(&exact2);
                let want = safe_sad(w, h, &PlaneCursor::new(&pad1, 0, s1), &PlaneCursor::new(&pad2, 0, s2));
                assert_eq!(got, want, "sad {w}x{h} strides {s1}/{s2}");
            }
        }
    }

    for &(w, h) in SAD_SHAPES {
        for &s1 in &strides(w) {
            for &s2 in &strides(w + 2) {
                let exact1 = rng.bytes((h - 1) * s1 + w);
                // The four-point reference reach: anchor at (1, 1) of a surface
                // spanning rows -1 .. h+1 and one column either side.
                let exact2 = rng.bytes((h + 1) * s2 + w + 1);
                let mut got = [0i32; 4];
                safe_sad_four(
                    w,
                    h,
                    &PlaneCursor::new(&exact1, 0, s1),
                    &PlaneCursor::new(&exact2, s2 + 1, s2),
                    &mut got,
                );
                for (k, &v) in got.iter().enumerate() {
                    assert!(
                        (0..=(w * h * 255) as i32).contains(&v),
                        "sad-four {w}x{h}: arm {k} scored {v}, outside [0, {}]",
                        w * h * 255
                    );
                }
                assert!(
                    got.iter().collect::<std::collections::HashSet<_>>().len() > 1,
                    "sad-four {w}x{h}: all four search points scored identically on noise                      (strides {s1}/{s2}, seed {:#x})",
                    rng.seed()
                );

                let mut pad2 = rng.bytes((h + 2) * s2 + 64);
                pad2[..exact2.len()].copy_from_slice(&exact2);
                let mut want = [0i32; 4];
                safe_sad_four(
                    w,
                    h,
                    &PlaneCursor::new(&exact1, 0, s1),
                    &PlaneCursor::new(&pad2, s2 + 1, s2),
                    &mut want,
                );
                assert_eq!(got, want, "sad-four {w}x{h} strides {s1}/{s2}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `processing/vaacalc.rs` + `processing/adaptive_quantization.rs`
// ---------------------------------------------------------------------------

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

/// The per-quadrant form of [`noise_i32`]: the safe VAA kernels type
/// `pSad8x8`/`pSd8x8` as one `[i32; 4]` per macroblock rather than a flat run.
fn noise_quads_i32(rng: &mut Prng, len: usize) -> Vec<[i32; 4]> {
    (0..len)
        .map(|_| std::array::from_fn(|_| rng.next_u32() as i32))
        .collect()
}

/// As [`noise_quads_i32`], for `pMad8x8`.
fn noise_quads_u8(rng: &mut Prng, len: usize) -> Vec<[u8; 4]> {
    (0..len)
        .map(|_| std::array::from_fn(|_| rng.next_u32() as u8))
        .collect()
}

/// **Span size.** Every plane is allocated to exactly `vaa_span`, so a shim that
/// claims one byte more is UB that Miri reports at the `from_raw_parts`, and one that
/// claims less panics inside the safe walk. The output arrays carry a noise tail that
/// neither may touch.
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
        let mut sad = noise_quads_i32(&mut rng, n);
        let mut sd = noise_quads_i32(&mut rng, n);
        let mut sum = noise_i32(&mut rng, n);
        let mut sqsum = noise_i32(&mut rng, n);
        let mut sqdiff = noise_i32(&mut rng, n);
        let mut mad = noise_quads_u8(&mut rng, n);
        let tail_sad = sad[mbs..].to_vec();
        let tail_sd = sd[mbs..].to_vec();
        let tail_sum = sum[mbs..].to_vec();
        let tail_sqsum = sqsum[mbs..].to_vec();
        let tail_sqdiff = sqdiff[mbs..].to_vec();
        let tail_mad = mad[mbs..].to_vec();

        let frame = vaa::vaa_calc_sad(&cur, &refp, w, h, stride, &mut sad);
        assert_eq!(&sad[mbs..], &tail_sad[..], "Sad wrote past the last MB, {at}");
        assert_eq!(
            frame,
            sad[..mbs].iter().flatten().sum::<i32>(),
            "the frame total is the sum of the parts, {at}"
        );

        vaa::vaa_calc_sad_var(&cur, &refp, w, h, stride, &mut sad, &mut sum, &mut sqsum);
        assert_eq!(&sad[mbs..], &tail_sad[..], "SadVar wrote past the last MB, {at}");
        assert_eq!(&sum[mbs..], &tail_sum[..], "SadVar wrote past sum16x16, {at}");
        assert_eq!(&sqsum[mbs..], &tail_sqsum[..], "SadVar wrote past sqsum16x16, {at}");

        vaa::vaa_calc_sad_ssd(
            &cur, &refp, w, h, stride, &mut sad, &mut sum, &mut sqsum, &mut sqdiff,
        );
        assert_eq!(&sqdiff[mbs..], &tail_sqdiff[..], "SadSsd wrote past sqdiff16x16, {at}");

        vaa::vaa_calc_sad_bgd(&cur, &refp, w, h, stride, &mut sad, &mut sd, &mut mad);
        assert_eq!(&sd[mbs..], &tail_sd[..], "SadBgd wrote past pSd8x8, {at}");
        assert_eq!(&mad[mbs..], &tail_mad[..], "SadBgd wrote past pMad8x8, {at}");

        vaa::vaa_calc_sad_ssd_bgd(
            &cur, &refp, w, h, stride, &mut sad, &mut sum, &mut sqsum, &mut sqdiff,
            &mut sd, &mut mad,
        );
        assert_eq!(&sad[mbs..], &tail_sad[..], "SadSsdBgd wrote past the last MB, {at}");
        assert_eq!(&sd[mbs..], &tail_sd[..], "SadSsdBgd wrote past pSd8x8, {at}");
        assert_eq!(&sqdiff[mbs..], &tail_sqdiff[..], "SadSsdBgd wrote past sqdiff16x16, {at}");
        assert_eq!(&mad[mbs..], &tail_mad[..], "SadSsdBgd wrote past pMad8x8, {at}");
    }
}

/// **Span anchor**, which sizing does not pin.
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

        let mut sad = vec![[0i32; 4]; mbs];
        let frame = vaa::vaa_calc_sad(&cur, &refp, w, h, stride, &mut sad);

        let mut expect_frame = 0i32;
        for mb in 0..mbs {
            for q in 0..4 {
                let want = 64 * delta(mb, q) as i32;
                assert_eq!(
                    sad[mb][q], want,
                    "macroblock {mb} quadrant {q} read the wrong 8x8 block, {at}"
                );
                expect_frame += want;
            }
        }
        assert_eq!(frame, expect_frame, "frame total, {at}");
    }
}

/// The two extremes of the variance probe's input domain, which the random sweep
/// never reaches and which are where the C++'s integer widths would bite if they
/// could.
///
/// A 16x16 block holds 256 samples of at most 255, so the `uint16_t` sums top out at
/// **65280** and the `uint32_t` squares at **16 646 400** — both short of wrapping.
/// The `wrapping_add`s the port carries are therefore unreachable, and this is the
/// test that says so.
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
// `common/deblocking_common.rs`, in-loop deblocking
// ===========================================================================

use openh264_rs::common::deblocking_common as deb;

// ===========================================================================
// border expansion (`common/expand_pic.rs`)
// ===========================================================================
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
// `encoder/encode_mb_aux.rs`, the forward transform / quant / scan family
// ===========================================================================
//
// Two spans in this family are exact reaches over strided reads — the 2x2
// Hadamard's `[i16; 49]` (DC raster positions 0/16/32/48 of the chroma group)
// and the DC-Hadamard's `[i16; 241]` (block 15's DC at index 240 of the
// 256-element luma buffer) — and the probes below hand each shim an allocation
// of exactly that size (an over-claim is UB Miri reports; an under-claim panics
// in the safe kernel), with a **golden direct run** of the safe kernel at
// the contract's own geometry pinning the anchor, and untouched-byte
// assertions pinning the touch set.

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
        let b1 = rng.bytes(3 * s1 + 4);
        let b2 = rng.bytes(3 * s2 + 4);
        let (k1, k2) = (b1.clone(), b2.clone());
        let mut dct = [0i16; 16];
        ema::dct_4x4(
            (&mut dct[..16]).try_into().unwrap(),
            &PlaneCursor::new(&b1, 0, s1),
            &PlaneCursor::new(&b2, 0, s2),
        );
        let mut golden = [0i16; 16];
        ema::dct_4x4(&mut golden, &PlaneCursor::new(&k1, 0, s1), &PlaneCursor::new(&k2, 0, s2));
        assert_eq!(dct, golden, "DctT4 shim vs direct at s1={s1} s2={s2}");
        assert_eq!((b1, b2), (k1, k2), "DctT4 shim moved a source byte");
    }
    for &(s1, s2) in &[(8usize, 16usize), (21, 8), (240, 25)] {
        let b1 = rng.bytes(7 * s1 + 8);
        let b2 = rng.bytes(7 * s2 + 8);
        let (k1, k2) = (b1.clone(), b2.clone());
        let mut dct = [0i16; 64];
        ema::dct_four_4x4(
            (&mut dct[..64]).try_into().unwrap(),
            &PlaneCursor::new(&b1, 0, s1),
            &PlaneCursor::new(&b2, 0, s2),
        );
        let mut golden = [0i16; 64];
        ema::dct_four_4x4(&mut golden, &PlaneCursor::new(&k1, 0, s1), &PlaneCursor::new(&k2, 0, s2));
        assert_eq!(dct, golden, "DctFourT4 shim vs direct at s1={s1} s2={s2}");
        assert_eq!((b1, b2), (k1, k2), "DctFourT4 shim moved a source byte");
    }

    // --- Quantizers. The relations *between* the kernels: `quant_four_4x4` is
    // `quant_4x4` on each quadrant, and `quant_four_4x4_max` is `quant_four_4x4`
    // plus a per-quadrant maximum.
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

        // **The two kernels are deliberately not cross-asserted**: `skip`
        // thresholds on `(1<<16 - 1) / mf - ff` in `i32` while the full kernel
        // truncates each butterfly output to `i16` before quantising, so "skip says
        // nothing survives" and "the full kernel counted zero" agree on every input
        // the encoder produces but are not the same predicate. An assertion that
        // they are is a test asserting its author's guess.
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
        let src = dct;

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
    type BlockCopy = fn(&RecCursor<'_>, &RecCursor<'_>);
    let kernels: &[(&str, usize, usize, BlockCopy)] = &[
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
                {
                    let d = RecCursor::over_owned(&mut dst, 0, ds);
                    let sc = RecCursor::over_owned(&mut src, 0, ss);
                    raw(&d, &sc);
                }
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
// `encoder/decode_mb_aux.rs`: the encoder's recon IDCT and dequantisation family
// ===========================================================================

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

        // `dequant_four_4x4` is `dequant_4x4` on each of the four blocks.
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
        direct: impl Fn(&mut PlaneCursorMut<'_>, &PlaneCursor<'_>, &[i16; N]),
    ) {
        let pred = rng.bytes((h - 1) * ps + w);
        let mut rec = rng.bytes((h - 1) * rs + w);
        let pred_before = pred.clone();
        let rec_before = rec.clone();
        let dct: [i16; N] = core::array::from_fn(|_| rng.range_i32(-32768, 32767) as i16);
        let m_dct = dct;

        // The sources must not move, and nothing outside each row's block may be
        // written.
        direct(
            &mut PlaneCursorMut::new(&mut rec, 0, rs),
            &PlaneCursor::new(&pred, 0, ps),
            &dct,
        );
        assert_eq!((&pred, m_dct), (&pred_before, dct), "{name}: a source moved");
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
            probe_rec::<16>("IDctT4Rec", &mut rng, 4, 4, rs, ps, eda::idct_t4_rec);
        }
    }
    for &(rs, ps) in &[(8usize, 16usize), (21, 8), (240, 25)] {
        for _ in 0..scale(10) {
            probe_rec::<64>("IDctFourT4Rec", &mut rng, 8, 8, rs, ps, eda::idct_four_t4_rec);
        }
    }
    for &(rs, ps) in &[(16usize, 16usize), (21, 16), (240, 25)] {
        for _ in 0..scale(10) {
            probe_rec::<16>("IDctRecI16x16Dc", &mut rng, 16, 16, rs, ps, eda::idct_rec_i16x16_dc);
        }
    }
}

// ===========================================================================
// `encoder/sample.rs`, the SATD family
// ===========================================================================

use openh264_rs::encoder::sample as satd;

type SafeSatd = fn(&PlaneCursor<'_>, &PlaneCursor<'_>) -> i32;

const SATD_SHAPES: &[(&str, usize, usize, SafeSatd)] = &[
    ("Satd4x4", 4, 4, |a, b| satd::satd_4x4(a, b)),
    ("Satd8x4", 8, 4, |a, b| satd::satd_8x4(a, b)),
    ("Satd4x8", 4, 8, |a, b| satd::satd_4x8(a, b)),
    ("Satd8x8", 8, 8, |a, b| satd::satd_8x8(a, b)),
    ("Satd16x8", 16, 8, |a, b| satd::satd_16x8(a, b)),
    ("Satd8x16", 8, 16, |a, b| satd::satd_8x16(a, b)),
    ("Satd16x16", 16, 16, |a, b| satd::satd_16x16(a, b)),
];

/// The safe kernels' declared reach is `(h-1)*stride + w` from the anchor on
/// each surface — exact-span allocations prove it is not under-claimed (an
/// over-read panics at the slice), and agreement with the same kernel run on
/// a padded copy of the same content proves the values do not depend on
/// anything outside the span.
#[test]
fn satd_kernels_stay_inside_the_spans_they_declare() {
    let mut rng = Prng::new(0x5A7D_59A9);

    for &(name, w, h, safe) in SATD_SHAPES {
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
// `encoder/deblocking.rs`'s duplicate deblocking kernels
// ===========================================================================

/// Alpha values from the ends and middle of `g_kuiAlphaTable` (0..=255), beta
/// values from `g_kiBetaTable` (0..=18).
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

/// `encoder/deblocking.rs`'s `WelsNonZeroCount_c` against the safe kernel it
/// duplicates.
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

