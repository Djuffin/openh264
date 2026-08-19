#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/vaacalc/` — the VAA (video analysis) statistics
//! plugin reached through `METHOD_VAA_STATISTICS`.
//!
//! All five kernels are translated. Only `pfVAACalcSad` is *reached* in the gate
//! configuration: the other four (`pfVAACalcSadVar`, `pfVAACalcSadSsd`,
//! `pfVAACalcSadBgd`, `pfVAACalcSadSsdBgd`) are selected by
//! `iCalcVar`/`iCalcSsd`/`iCalcBgd`, which `CWelsPreProcess::AnalyzeSpatialPic`
//! derives from rate control, adaptive quantisation and background detection — all
//! three off there. So `VAACalcSad_c` is the only one of the five whose performance
//! the benches can see, and the only one whose codegen this port has to keep clean.
//!
//! (This note used to read "only the `pfVAACalcSad` kernel is translated", which was
//! never true of the file and would have sent a straggler sweep looking for four
//! kernels that were already here.)

use crate::encoder::wels_preprocess::{SPixMap, SVAACalcParam, SVAACalcResult};

/// `EResult` — `codec/processing/interface/IWelsVP.h:54`.
pub const RET_SUCCESS: i32 = 0;
pub const RET_FAILED: i32 = -1;
pub const RET_INVALIDPARAM: i32 = -2;
pub const RET_OUTOFMEMORY: i32 = -3;
pub const RET_NOTSUPPORTED: i32 = -4;
pub const RET_UNEXPECTED: i32 = -5;

/// The two quantities every `VAACalc*` shim needs to turn its raw pointers into
/// slices: the plane span the walk reads, and the number of macroblocks it writes.
///
/// One helper, so that no shim does this arithmetic itself and a wrong span is one
/// bug rather than five (the T3 rule — `safety_refactor_log.md`, Phase 2 session A).
fn shim_extent(pic_width: i32, pic_height: i32, pic_stride: i32) -> (usize, usize) {
    let mbs = (pic_width >> 4).max(0) as usize * (pic_height >> 4).max(0) as usize;
    (vaa_span(pic_width, pic_height, pic_stride), mbs)
}

/// `VAACalcSad_c` — `codec/processing/src/vaacalc/vaacalcfuncs.cpp:39`.
///
/// Walks the picture macroblock by macroblock, writing the four 8x8 sums of
/// absolute differences of each macroblock into `pSad8x8[(mb_index << 2) + n]` and
/// accumulating the frame total into `*pFrameSad`.
///
/// # Safety
/// * `pCurData` and `pRefData` each point at sample `(0, 0)` of a luma plane whose
///   rows are `iPicStride` bytes apart, with at least
///   `vaa_span(iPicWidth, iPicHeight, iPicStride)` readable bytes from there. That is
///   the walk's **exact** reach and it is strictly forward — this family reads no row
///   above and no column left of its origin, so no padding is required in any
///   direction and `PADDING_LENGTH` does not enter the argument. A plane of
///   `iPicHeight * iPicStride` bytes, which is what every caller has, always contains
///   the span; the tighter figure is stated because it is what the shim claims.
/// * `pSad8x8` has room for `4 * (iPicWidth >> 4) * (iPicHeight >> 4)` `i32`s, i.e.
///   one `[i32; 4]` per macroblock — which is the type `SVAACalcResult::pSad8x8`
///   actually declares, and the cast below undoes the caller's own cast to `*mut i32`.
/// * `pFrameSad` is writable. See F9: it is an `i32` accumulated over the whole
///   picture and overflows above 32 896 macroblocks, exactly as the C++ does.
pub unsafe fn VAACalcSad_c(
    pCurData: *const u8,
    pRefData: *const u8,
    iPicWidth: i32,
    iPicHeight: i32,
    iPicStride: i32,
    pFrameSad: *mut i32,
    pSad8x8: *mut i32,
) {
    // SHIM(phase2) -> vaa_calc_sad
    unsafe {
        let (span, mbs) = shim_extent(iPicWidth, iPicHeight, iPicStride);
        *pFrameSad = vaa_calc_sad(
            core::slice::from_raw_parts(pCurData, span),
            core::slice::from_raw_parts(pRefData, span),
            iPicWidth,
            iPicHeight,
            iPicStride,
            core::slice::from_raw_parts_mut(pSad8x8 as *mut [i32; 4], mbs),
        );
    }
}

/// `CVAACalculation` — `codec/processing/src/vaacalc/vaacalculation.cpp`. The only
/// state the class carries across calls is the `SVAACalcParam` its `Set` stores.
#[derive(Default)]
pub struct CVAACalculation {
    pub m_sCalcParam: SVAACalcParam,
}

/// `VAACalcSadVar_c` — `codec/processing/src/vaacalc/vaacalcfuncs.cpp:121`.
///
/// `VAACalcSad_c` plus, per macroblock, the sum and the sum of squares of the
/// current picture's 256 luma samples. `CWelsPreProcess::AnalyzeSpatialPic` selects
/// it whenever `iRCMode >= RC_BITRATE_MODE` and the slice is an I slice
/// (`wels_preprocess.cpp:283`), because `AnalyzeGomComplexityViaVar` derives each
/// GOM's variance from those two sums.
///
/// # Safety
/// As [`VAACalcSad_c`], and `pSum16x16`/`psqsum16x16` must each have room for
/// `(iPicWidth >> 4) * (iPicHeight >> 4)` `i32`s.
pub unsafe fn VAACalcSadVar_c(
    pCurData: *const u8,
    pRefData: *const u8,
    iPicWidth: i32,
    iPicHeight: i32,
    iPicStride: i32,
    pFrameSad: *mut i32,
    pSad8x8: *mut i32,
    pSum16x16: *mut i32,
    psqsum16x16: *mut i32,
) {
    // SHIM(phase2) -> vaa_calc_sad_var
    unsafe {
        let (span, mbs) = shim_extent(iPicWidth, iPicHeight, iPicStride);
        *pFrameSad = vaa_calc_sad_var(
            core::slice::from_raw_parts(pCurData, span),
            core::slice::from_raw_parts(pRefData, span),
            iPicWidth,
            iPicHeight,
            iPicStride,
            core::slice::from_raw_parts_mut(pSad8x8 as *mut [i32; 4], mbs),
            core::slice::from_raw_parts_mut(pSum16x16, mbs),
            core::slice::from_raw_parts_mut(psqsum16x16, mbs),
        );
    }
}

/// `VAACalcSadSsd_c` — `vaacalcfuncs.cpp:225`.
///
/// `VAACalcSadVar_c` plus the per-macroblock sum of squared *differences*, which
/// `CAdaptiveQuantization` reads as the motion index.
///
/// # Safety
/// As [`VAACalcSadVar_c`], plus `psqdiff16x16`.
pub unsafe fn VAACalcSadSsd_c(
    pCurData: *const u8,
    pRefData: *const u8,
    iPicWidth: i32,
    iPicHeight: i32,
    iPicStride: i32,
    pFrameSad: *mut i32,
    pSad8x8: *mut i32,
    pSum16x16: *mut i32,
    psqsum16x16: *mut i32,
    psqdiff16x16: *mut i32,
) {
    // SHIM(phase2) -> vaa_calc_sad_ssd
    unsafe {
        let (span, mbs) = shim_extent(iPicWidth, iPicHeight, iPicStride);
        *pFrameSad = vaa_calc_sad_ssd(
            core::slice::from_raw_parts(pCurData, span),
            core::slice::from_raw_parts(pRefData, span),
            iPicWidth,
            iPicHeight,
            iPicStride,
            core::slice::from_raw_parts_mut(pSad8x8 as *mut [i32; 4], mbs),
            core::slice::from_raw_parts_mut(pSum16x16, mbs),
            core::slice::from_raw_parts_mut(psqsum16x16, mbs),
            core::slice::from_raw_parts_mut(psqdiff16x16, mbs),
        );
    }
}

/// `VAACalcSadBgd_c` — `vaacalcfuncs.cpp:462`.
///
/// SAD plus, per 8x8 block, the **signed** sum of differences and the maximum
/// absolute difference. `CBackgroundDetection` reads both.
///
/// # Safety
/// As [`VAACalcSad_c`], plus `pSd8x8` (4 `i32`s per macroblock) and `pMad8x8`
/// (4 `u8`s per macroblock).
pub unsafe fn VAACalcSadBgd_c(
    pCurData: *const u8,
    pRefData: *const u8,
    iPicWidth: i32,
    iPicHeight: i32,
    iPicStride: i32,
    pFrameSad: *mut i32,
    pSad8x8: *mut i32,
    pSd8x8: *mut i32,
    pMad8x8: *mut u8,
) {
    // SHIM(phase2) -> vaa_calc_sad_bgd
    unsafe {
        let (span, mbs) = shim_extent(iPicWidth, iPicHeight, iPicStride);
        *pFrameSad = vaa_calc_sad_bgd(
            core::slice::from_raw_parts(pCurData, span),
            core::slice::from_raw_parts(pRefData, span),
            iPicWidth,
            iPicHeight,
            iPicStride,
            core::slice::from_raw_parts_mut(pSad8x8 as *mut [i32; 4], mbs),
            core::slice::from_raw_parts_mut(pSd8x8 as *mut [i32; 4], mbs),
            core::slice::from_raw_parts_mut(pMad8x8 as *mut [u8; 4], mbs),
        );
    }
}

/// `VAACalcSadSsdBgd_c` — `vaacalcfuncs.cpp:640`. Everything the other four
/// compute, in one pass.
///
/// Note it squares `abs_diff`, not `diff`, where `VAACalcSadSsd_c` squares the
/// already-absolute `diff`. Same value; the safe side computes one form for both.
///
/// # Safety
/// The union of [`VAACalcSadSsd_c`]'s and [`VAACalcSadBgd_c`]'s requirements.
#[allow(clippy::too_many_arguments)]
pub unsafe fn VAACalcSadSsdBgd_c(
    pCurData: *const u8,
    pRefData: *const u8,
    iPicWidth: i32,
    iPicHeight: i32,
    iPicStride: i32,
    pFrameSad: *mut i32,
    pSad8x8: *mut i32,
    pSum16x16: *mut i32,
    psqsum16x16: *mut i32,
    psqdiff16x16: *mut i32,
    pSd8x8: *mut i32,
    pMad8x8: *mut u8,
) {
    // SHIM(phase2) -> vaa_calc_sad_ssd_bgd
    unsafe {
        let (span, mbs) = shim_extent(iPicWidth, iPicHeight, iPicStride);
        *pFrameSad = vaa_calc_sad_ssd_bgd(
            core::slice::from_raw_parts(pCurData, span),
            core::slice::from_raw_parts(pRefData, span),
            iPicWidth,
            iPicHeight,
            iPicStride,
            core::slice::from_raw_parts_mut(pSad8x8 as *mut [i32; 4], mbs),
            core::slice::from_raw_parts_mut(pSum16x16, mbs),
            core::slice::from_raw_parts_mut(psqsum16x16, mbs),
            core::slice::from_raw_parts_mut(psqdiff16x16, mbs),
            core::slice::from_raw_parts_mut(pSd8x8 as *mut [i32; 4], mbs),
            core::slice::from_raw_parts_mut(pMad8x8 as *mut [u8; 4], mbs),
        );
    }
}

//=================== Safe kernels =====================//

// The five `VAACalc*` kernels are one whole-picture walk that differs only in which
// per-block statistics it reports, and the C++ writes that walk out five times. Here
// it is written once: [`walk_picture`] over [`block_stats`], with three const flags
// selecting the accumulators. The flags are compile-time so the unused arithmetic is
// not emitted at all — which matters, because the gate configuration selects
// `VAACalcSad_c` and only `VAACalcSad_c` (`iCalcVar`/`iCalcSsd`/`iCalcBgd` are all
// off), so the cheapest of the five is the one whose codegen has to stay clean.
//
// Unlike every earlier family in this phase these kernels are **streaming**: one pass
// per frame over an entire picture, touching each sample once. A 1080p luma plane is
// ~2 MB against a 128 KB L1, so the walk is memory-bound end to end and per-row
// bookkeeping hides behind the misses instead of dominating them the way it does in a
// motion search (`perf_baseline.md` §Phase 2 T5). That is why the microbenchmark for
// this family runs at picture size and why the large working set is the honest one
// here — the caller's residency is the instrument's specification.

/// The per-8x8-block statistics the five `VAACalc*` kernels choose between.
///
/// Which fields are live is a compile-time decision — see [`block_stats`] — so a
/// kernel that does not report `sqsum` never emits the multiply that would produce
/// it. Fields its flags exclude stay zero.
#[derive(Clone, Copy, Default)]
struct BlockStats {
    /// Sum of absolute differences. Every variant computes this one.
    sad: i32,
    /// Sum of **signed** differences (`BGD`).
    sd: i32,
    /// Largest absolute difference in the block (`BGD`).
    mad: i32,
    /// Sum of squared differences (`SQDIFF`).
    sqdiff: i32,
    /// Sum of the current picture's samples (`VAR`).
    sum: i32,
    /// Sum of the squares of the current picture's samples (`VAR`).
    sqsum: i32,
}

/// Accumulate the eight samples at `c[FROM..FROM+8]` against `r[FROM..FROM+8]` into
/// one quadrant's statistics.
///
/// C++: the body of the innermost `for (l)` in every kernel in
/// `codec/processing/src/vaacalc/vaacalcfuncs.cpp`. The five copies differ in nothing
/// but which accumulators they keep, which is what the three flags select.
///
/// `FROM` is a const so that both call sites index a compile-time window of a
/// compile-time-sized array: there is no bounds check in here at all, and the eight
/// iterations are free to vectorise.
///
/// Passing the statistics **by value** rather than through `&mut` was tried, on the
/// theory that `s.mad`'s read-modify-write blocked the max from becoming a vector
/// reduction. It measured no better and slightly worse (1.52x against 1.33x at
/// 1080p) and is therefore not here — see [`half_mb_stats`] for what the remaining
/// `VAACalcSadBgd_c` gap actually is.
///
/// **Arithmetic parity.** Every accumulator is `i32` and every operation is the
/// C++'s, in the C++'s width; nothing is widened and no `wrapping_*` the old code
/// lacks is added. `VAACalcSadSsd_c` squares an already-absolute difference where
/// `VAACalcSadSsdBgd_c` squares `abs_diff` — the same value, so one form serves both.
#[inline(always)]
fn accumulate<const VAR: bool, const SQDIFF: bool, const BGD: bool, const FROM: usize>(
    s: &mut BlockStats,
    c: &[u8; 16],
    r: &[u8; 16],
) {
    for i in FROM..FROM + 8 {
        let cur_sample = c[i] as i32;
        let diff = cur_sample - r[i] as i32;
        let abs_diff = diff.abs();
        s.sad += abs_diff;
        if BGD {
            s.sd += diff;
            if abs_diff > s.mad {
                s.mad = abs_diff;
            }
        }
        if SQDIFF {
            s.sqdiff += abs_diff * abs_diff;
        }
        if VAR {
            s.sum += cur_sample;
            s.sqsum += cur_sample * cur_sample;
        }
    }
}

/// The statistics of the two 8x8 quadrants sitting side by side in one half of a
/// macroblock, from plane slices anchored at that half's top-left sample.
///
/// **This reads each row once, sixteen samples wide, and splits it in registers.**
/// The C++ walks a quadrant at a time, so it reads rows 0..8 once for the top-left
/// quadrant and *again* for the top-right; transliterating that shape costs two range
/// checks per row per quadrant, i.e. 64 per macroblock, and a disassembly of the first
/// version of this file showed all 64 of them surviving as `slice_index_fail` sites
/// (the same mechanism that parked T5's SAD kernels — `perf_baseline.md` §Phase 2 T5).
/// Reading a 16-wide row instead halves the checks to 32 per macroblock, makes each
/// window a fixed-size `[u8; 16]` whose inner loops need no check at all, and walks
/// the macroblock in one sequential pass instead of two overlapping ones.
///
/// **Why it is still bit-exact.** Regrouping only changes the order in which each
/// quadrant's own samples are accumulated, and every accumulator is an associative,
/// commutative `i32` sum (`mad` is a max, which is both as well). No sample moves
/// between quadrants, and no accumulator mixes with another. The frame total is still
/// summed in the C++'s quadrant order by [`walk_picture`], which is the one place the
/// order could be observed at all — see F9 for why that ordering is worth preserving.
///
/// **Where this shape does not pay, measured.** The 16-wide read is a clear win for
/// every flag combination except `BGD` alone, which lands at **1.44x** against the
/// raw kernel while the others sit at 0.69-0.97x (`perf_baseline.md` §Phase 2 T8).
/// The reason is visible in the disassembly and is not the bounds checks: the raw
/// `VAACalcSadBgd_c` issues 16 `uabd` and 16 `umax`, this one issues 8 and 8. `BGD`'s
/// two extra accumulators — a signed sum and a running maximum — are **per quadrant**
/// and cannot be merged across the row the way `sad` can, so each 8-sample half fills
/// only half a vector register and the loop vectorises at half width. Where `SQDIFF`
/// and `VAR` are also on there is enough other arithmetic to hide it (`SsdBgd` is
/// 0.97x), and where none of them are on the 16 samples go through one `uabd.16b`
/// (`Sad` is 0.86x). Only the middle case loses. Recorded rather than fixed: the fix
/// is a second, quadrant-shaped walk selected on `BGD`, which is a real option for
/// T7's similarly-shaped kernels but is more machinery than this family's measured
/// end-to-end cost justifies.
#[inline(always)]
fn half_mb_stats<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
) -> (BlockStats, BlockStats) {
    let mut left = BlockStats::default();
    let mut right = BlockStats::default();
    // Trim both planes to **exactly** the eight rows this half reads, once, before
    // the loop. Handed an open tail (`&cur[origin..]`) LLVM cannot relate the row
    // offset `k * stride` to the slice length and re-checks every row; handed a
    // window whose length it knows to be `7 * stride + 16` it can, and the eight
    // per-row checks collapse into the two above. Same reads either way — this is
    // the check placement changing, not the reach.
    let cur = &cur[..7 * stride + 16];
    let refp = &refp[..7 * stride + 16];
    for k in 0..8 {
        let base = k * stride;
        let c: &[u8; 16] = cur[base..base + 16].try_into().unwrap();
        let r: &[u8; 16] = refp[base..base + 16].try_into().unwrap();
        accumulate::<VAR, SQDIFF, BGD, 0>(&mut left, c, r);
        accumulate::<VAR, SQDIFF, BGD, 8>(&mut right, c, r);
    }
    (left, right)
}

/// Walks the picture macroblock by macroblock, handing each macroblock's four
/// quadrant statistics to `on_mb`, and returns the frame's total SAD.
///
/// C++: the `for (i) for (j)` nest shared by all five kernels in
/// `vaacalcfuncs.cpp`, quadrant order and accumulation order included — `iFrameSad`
/// sums in quadrant-within-macroblock-within-row order here exactly as it does there.
///
/// **The step quirk is reproduced, not corrected.** Between macroblock rows the C++
/// advances its cursors by `(iPicStride << 4) - iPicWidth` *after* having advanced 16
/// per macroblock, so a picture whose width is not a multiple of 16 shifts left by
/// the remainder on every macroblock row instead of landing on the next row's first
/// sample. [`vaa_span`] derives the read span from this same walk, so the two agree
/// by construction.
#[inline(always)]
fn walk_picture<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    mut on_mb: impl FnMut(usize, &[BlockStats; 4]),
) -> i32 {
    let mb_width = pic_width >> 4;
    let mb_height = pic_height >> 4;
    let stride = pic_stride as usize;
    let bottom = stride * 8;
    let step = ((pic_stride << 4) - pic_width) as usize;

    let mut frame_sad = 0i32;
    let mut mb_index = 0usize;
    let mut row_origin = 0usize;
    for _ in 0..mb_height {
        let mut mb_origin = row_origin;
        for _ in 0..mb_width {
            let (q0, q1) = half_mb_stats::<VAR, SQDIFF, BGD>(
                &cur[mb_origin..],
                &refp[mb_origin..],
                stride,
            );
            let (q2, q3) = half_mb_stats::<VAR, SQDIFF, BGD>(
                &cur[mb_origin + bottom..],
                &refp[mb_origin + bottom..],
                stride,
            );
            let stats = [q0, q1, q2, q3];
            // The frame total still accumulates in the C++'s quadrant order.
            for q in &stats {
                frame_sad += q.sad;
            }
            on_mb(mb_index, &stats);
            mb_index += 1;
            mb_origin += 16;
        }
        row_origin = mb_origin + step;
    }
    frame_sad
}

/// The exact number of bytes a `VAACalc*` walk reads from each plane, counted from
/// the origin the caller hands it.
///
/// **This is the only place that arithmetic lives.** The five shims size their slices
/// with it and `tests/kernels_differential_phase2.rs` pins it by running each shim
/// against allocations of exactly this length — too long and Miri reports the
/// over-claim at the `from_raw_parts`, too short and the safe kernel panics.
///
/// The last sample read is the bottom-right corner of the bottom-right macroblock's
/// bottom-right quadrant. That quadrant begins `8 * stride + 8` past its macroblock's
/// origin and spans eight further rows and columns, so it ends `15 * stride + 15`
/// past that origin — which is why a walk over a picture whose height is a multiple
/// of 16 needs no row below its last macroblock and no padding at all.
pub fn vaa_span(pic_width: i32, pic_height: i32, pic_stride: i32) -> usize {
    let mb_width = pic_width >> 4;
    let mb_height = pic_height >> 4;
    if mb_width <= 0 || mb_height <= 0 {
        return 0;
    }
    let (mb_width, mb_height) = (mb_width as usize, mb_height as usize);
    let stride = pic_stride as usize;
    let step = ((pic_stride << 4) - pic_width) as usize;
    // The same walk `walk_picture` performs, run to the last macroblock's origin.
    let last_mb = (mb_height - 1) * (mb_width * 16 + step) + (mb_width - 1) * 16;
    last_mb + 15 * stride + 16
}

/// C++: `VAACalcSad_c`, `codec/processing/src/vaacalc/vaacalcfuncs.cpp:39`.
///
/// Writes each macroblock's four 8x8 sums of absolute differences to `sad8x8[mb]` and
/// returns the frame total. Reads `vaa_span(..)` bytes of each plane and nothing
/// outside it; `sad8x8` needs one entry per macroblock.
pub fn vaa_calc_sad(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
) -> i32 {
    walk_picture::<false, false, false>(
        cur,
        refp,
        pic_width,
        pic_height,
        pic_stride,
        |mb, s| sad8x8[mb] = [s[0].sad, s[1].sad, s[2].sad, s[3].sad],
    )
}

/// C++: `VAACalcSadVar_c`, `vaacalcfuncs.cpp:121`.
///
/// [`vaa_calc_sad`] plus, per macroblock, the sum and the sum of squares of the
/// current picture's 256 luma samples — the two quantities `AnalyzeGomComplexityViaVar`
/// derives each GOM's variance from. `sum16x16` and `sqsum16x16` need one entry per
/// macroblock each.
pub fn vaa_calc_sad_var(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
) -> i32 {
    walk_picture::<true, false, false>(
        cur,
        refp,
        pic_width,
        pic_height,
        pic_stride,
        |mb, s| {
            sad8x8[mb] = [s[0].sad, s[1].sad, s[2].sad, s[3].sad];
            sum16x16[mb] = s[0].sum + s[1].sum + s[2].sum + s[3].sum;
            sqsum16x16[mb] = s[0].sqsum + s[1].sqsum + s[2].sqsum + s[3].sqsum;
        },
    )
}

/// C++: `VAACalcSadSsd_c`, `vaacalcfuncs.cpp:225`.
///
/// [`vaa_calc_sad_var`] plus the per-macroblock sum of squared *differences*, which
/// `CAdaptiveQuantization` reads as the motion index.
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_ssd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
    sqdiff16x16: &mut [i32],
) -> i32 {
    walk_picture::<true, true, false>(
        cur,
        refp,
        pic_width,
        pic_height,
        pic_stride,
        |mb, s| {
            sad8x8[mb] = [s[0].sad, s[1].sad, s[2].sad, s[3].sad];
            sum16x16[mb] = s[0].sum + s[1].sum + s[2].sum + s[3].sum;
            sqsum16x16[mb] = s[0].sqsum + s[1].sqsum + s[2].sqsum + s[3].sqsum;
            sqdiff16x16[mb] = s[0].sqdiff + s[1].sqdiff + s[2].sqdiff + s[3].sqdiff;
        },
    )
}

/// C++: `VAACalcSadBgd_c`, `vaacalcfuncs.cpp:462`.
///
/// SAD plus, per 8x8 block, the **signed** sum of differences and the maximum absolute
/// difference — both read by `CBackgroundDetection`. The maximum is accumulated as an
/// `i32` and stored as a `u8`, as in the C++; it cannot exceed 255, so the narrowing
/// is exact.
pub fn vaa_calc_sad_bgd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sd8x8: &mut [[i32; 4]],
    mad8x8: &mut [[u8; 4]],
) -> i32 {
    walk_picture::<false, false, true>(
        cur,
        refp,
        pic_width,
        pic_height,
        pic_stride,
        |mb, s| {
            sad8x8[mb] = [s[0].sad, s[1].sad, s[2].sad, s[3].sad];
            sd8x8[mb] = [s[0].sd, s[1].sd, s[2].sd, s[3].sd];
            mad8x8[mb] = [
                s[0].mad as u8,
                s[1].mad as u8,
                s[2].mad as u8,
                s[3].mad as u8,
            ];
        },
    )
}

/// C++: `VAACalcSadSsdBgd_c`, `vaacalcfuncs.cpp:640` — everything the other four
/// compute, in one pass.
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_ssd_bgd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
    sqdiff16x16: &mut [i32],
    sd8x8: &mut [[i32; 4]],
    mad8x8: &mut [[u8; 4]],
) -> i32 {
    walk_picture::<true, true, true>(
        cur,
        refp,
        pic_width,
        pic_height,
        pic_stride,
        |mb, s| {
            sad8x8[mb] = [s[0].sad, s[1].sad, s[2].sad, s[3].sad];
            sum16x16[mb] = s[0].sum + s[1].sum + s[2].sum + s[3].sum;
            sqsum16x16[mb] = s[0].sqsum + s[1].sqsum + s[2].sqsum + s[3].sqsum;
            sqdiff16x16[mb] = s[0].sqdiff + s[1].sqdiff + s[2].sqdiff + s[3].sqdiff;
            sd8x8[mb] = [s[0].sd, s[1].sd, s[2].sd, s[3].sd];
            mad8x8[mb] = [
                s[0].mad as u8,
                s[1].mad as u8,
                s[2].mad as u8,
                s[3].mad as u8,
            ];
        },
    )
}

impl CVAACalculation {
    /// `CVAACalculation::Set` — copies the caller's parameter block. Typed since
    /// Phase 6 session B (the `IWelsVP` vtable's `void*` is gone).
    pub fn Set(&mut self, param: &SVAACalcParam) -> i32 {
        self.m_sCalcParam = *param;
        RET_SUCCESS
    }

    /// `CVAACalculation::Process` — `vaacalculation.cpp:120`. Reads the current
    /// picture from `src` and the reference from `ref_pic` (the C++ passes the
    /// reference as `pDstPixMap`), and writes into `result`, which the caller hands
    /// over at the call rather than storing a pointer to it in the parameter block
    /// (take what you reach — the C++'s `pCalcResult` was that stored pointer).
    ///
    /// # Safety
    /// The pixel maps must describe readable luma planes of the stated geometry, and
    /// `result`'s arrays (`pSad8x8`, ...) must have room for the picture.
    pub unsafe fn Process(&mut self, src: &SPixMap, ref_pic: &SPixMap, result: &mut SVAACalcResult) -> i32 {
        let pCurData = src.pPixel[0];
        let pRefData = ref_pic.pPixel[0];
        let (iPicWidth, iPicHeight, iPicStride) = (src.sRect.iRectWidth, src.sRect.iRectHeight, src.iStride[0]);
        if pCurData.is_null() || pRefData.is_null() {
            return RET_INVALIDPARAM;
        }
        let pResult: *mut SVAACalcResult = result;

        (*pResult).pCurY = pCurData;
        (*pResult).pRefY = pRefData;

        // `vaacalculation.cpp:135` — the same nesting, in the same order.
        if self.m_sCalcParam.iCalcBgd {
            if self.m_sCalcParam.iCalcSsd {
                VAACalcSadSsdBgd_c(
                    pCurData,
                    pRefData,
                    iPicWidth,
                    iPicHeight,
                    iPicStride,
                    &mut (*pResult).iFrameSad as *mut i32,
                    (*pResult).pSad8x8 as *mut i32,
                    (*pResult).pSum16x16,
                    (*pResult).pSumOfSquare16x16,
                    (*pResult).pSsd16x16,
                    (*pResult).pSumOfDiff8x8 as *mut i32,
                    (*pResult).pMad8x8 as *mut u8,
                );
            } else {
                VAACalcSadBgd_c(
                    pCurData,
                    pRefData,
                    iPicWidth,
                    iPicHeight,
                    iPicStride,
                    &mut (*pResult).iFrameSad as *mut i32,
                    (*pResult).pSad8x8 as *mut i32,
                    (*pResult).pSumOfDiff8x8 as *mut i32,
                    (*pResult).pMad8x8 as *mut u8,
                );
            }
        } else if self.m_sCalcParam.iCalcSsd {
            VAACalcSadSsd_c(
                pCurData,
                pRefData,
                iPicWidth,
                iPicHeight,
                iPicStride,
                &mut (*pResult).iFrameSad as *mut i32,
                (*pResult).pSad8x8 as *mut i32,
                (*pResult).pSum16x16,
                (*pResult).pSumOfSquare16x16,
                (*pResult).pSsd16x16,
            );
        } else if self.m_sCalcParam.iCalcVar {
            VAACalcSadVar_c(
                pCurData,
                pRefData,
                iPicWidth,
                iPicHeight,
                iPicStride,
                &mut (*pResult).iFrameSad as *mut i32,
                (*pResult).pSad8x8 as *mut i32,
                (*pResult).pSum16x16,
                (*pResult).pSumOfSquare16x16,
            );
        } else {
            VAACalcSad_c(
                pCurData,
                pRefData,
                iPicWidth,
                iPicHeight,
                iPicStride,
                &mut (*pResult).iFrameSad as *mut i32,
                (*pResult).pSad8x8 as *mut i32,
            );
        }
        RET_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16x16 picture is one macroblock: four 8x8 SADs, and the frame total is
    /// their sum. Values checked against the C++ arithmetic by construction — a
    /// constant difference of `d` over an 8x8 block gives `64 * d`.
    #[test]
    fn calc_sad_one_macroblock() {
        let stride = 16i32;
        let cur = vec![100u8; 16 * 16];
        let mut refp = vec![100u8; 16 * 16];
        // Make the top-right quadrant differ by 3 and the bottom-left by 5.
        for y in 0..8 {
            for x in 8..16 {
                refp[y * 16 + x] = 103;
            }
        }
        for y in 8..16 {
            for x in 0..8 {
                refp[y * 16 + x] = 95;
            }
        }
        let mut sad8x8 = [0i32; 4];
        let mut frame_sad = 0i32;
        unsafe {
            VAACalcSad_c(
                cur.as_ptr(),
                refp.as_ptr(),
                16,
                16,
                stride,
                &mut frame_sad,
                sad8x8.as_mut_ptr(),
            );
        }
        assert_eq!(sad8x8, [0, 64 * 3, 64 * 5, 0]);
        assert_eq!(frame_sad, 64 * 3 + 64 * 5);
    }

    /// The kernel must advance by `(iPicStride << 4) - iPicWidth` between
    /// macroblock rows, so a picture whose stride exceeds its width still lands each
    /// macroblock's four sums at the right index.
    #[test]
    fn calc_sad_honours_stride_step() {
        let w = 32i32;
        let h = 32i32;
        let stride = 48i32;
        let cur = vec![10u8; (stride * h) as usize];
        let mut refp = vec![10u8; (stride * h) as usize];
        // Perturb only the last macroblock's bottom-right 8x8 quadrant.
        for y in 24..32 {
            for x in 24..32 {
                refp[(y * stride + x) as usize] = 11;
            }
        }
        let mut sad8x8 = [0i32; 16];
        let mut frame_sad = 0i32;
        unsafe {
            VAACalcSad_c(
                cur.as_ptr(),
                refp.as_ptr(),
                w,
                h,
                stride,
                &mut frame_sad,
                sad8x8.as_mut_ptr(),
            );
        }
        // mb_index 3 is the bottom-right macroblock; quadrant 3 is its bottom-right.
        assert_eq!(sad8x8[(3 << 2) + 3], 64);
        assert_eq!(frame_sad, 64);
        assert_eq!(sad8x8.iter().filter(|&&v| v != 0).count(), 1);
    }

    /// A width that is not a multiple of 16 makes the walk's step quirk observable,
    /// and this is the test that pins it now that the old-vs-new differential is
    /// gone.
    ///
    /// The C++ advances 16 bytes per macroblock and *then* by
    /// `(iPicStride << 4) - iPicWidth` at the end of the macroblock row. When the
    /// width is a multiple of 16 those two cancel to exactly sixteen rows. When it is
    /// not — here width 40, so `iMbWidth` is 2 and only 32 of the 40 columns are
    /// walked — the row advance comes up eight bytes short, and every macroblock row
    /// after the first starts *before* the row it looks like it should.
    ///
    /// That is faithful to `vaacalcfuncs.cpp` and is reproduced deliberately. A
    /// "corrected" walk that advanced a clean `16 * stride` would put the second
    /// macroblock row eight bytes later and read a different block, which is exactly
    /// what this asserts against.
    #[test]
    fn calc_sad_reproduces_the_step_quirk_at_a_width_that_is_not_a_multiple_of_16() {
        let (w, h, stride) = (40i32, 32i32, 64i32);
        let refp = vec![0u8; (h * stride) as usize];
        let mut cur = vec![0u8; (h * stride) as usize];

        // Macroblock row 1 begins at 1 * (2 * 16 + step) where step = 64*16 - 40,
        // i.e. byte 1016 — row 15, column 56 — and NOT at byte 1024 (row 16,
        // column 0). Mark the top-left 8x8 of the macroblock the quirky walk lands
        // on, and nothing else.
        let step = ((stride << 4) - w) as usize;
        let mb_row1 = 2 * 16 + step;
        assert_eq!(mb_row1, 1016, "the quirk's arithmetic, restated");
        for row in 0..8 {
            for col in 0..8 {
                cur[mb_row1 + row * stride as usize + col] = 7;
            }
        }

        let mut sad8x8 = [0i32; 16];
        let mut frame_sad = 0i32;
        unsafe {
            VAACalcSad_c(
                cur.as_ptr(),
                refp.as_ptr(),
                w,
                h,
                stride,
                &mut frame_sad,
                sad8x8.as_mut_ptr(),
            );
        }

        // Macroblock 2 is the first of the second row; quadrant 0 is its top-left.
        assert_eq!(sad8x8[2 << 2], 64 * 7, "the quirky walk did not land here");
        assert_eq!(frame_sad, 64 * 7);
        assert_eq!(
            sad8x8.iter().filter(|&&v| v != 0).count(),
            1,
            "the marked block leaked into another quadrant"
        );
    }
}
