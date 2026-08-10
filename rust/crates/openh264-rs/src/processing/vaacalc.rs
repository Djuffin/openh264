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

use crate::encoder::wels_preprocess::{SVAACalcParam, SVAACalcResult};

/// `EResult` — `codec/processing/interface/IWelsVP.h:54`.
pub const RET_SUCCESS: i32 = 0;
pub const RET_FAILED: i32 = -1;
pub const RET_INVALIDPARAM: i32 = -2;
pub const RET_OUTOFMEMORY: i32 = -3;
pub const RET_NOTSUPPORTED: i32 = -4;
pub const RET_UNEXPECTED: i32 = -5;

/// `VAACalcSad_c` — `codec/processing/src/vaacalc/vaacalcfuncs.cpp:39`.
///
/// Walks the picture macroblock by macroblock, writing the four 8x8 sums of
/// absolute differences of each macroblock into `pSad8x8[(mb_index << 2) + n]` and
/// accumulating the frame total into `*pFrameSad`.
///
/// # Safety
/// `pCurData` and `pRefData` must each address at least `iPicHeight * iPicStride`
/// readable bytes; `pSad8x8` must have room for `4 * (iPicWidth >> 4) *
/// (iPicHeight >> 4)` `i32`s.
pub unsafe fn VAACalcSad_c(
    pCurData: *const u8,
    pRefData: *const u8,
    iPicWidth: i32,
    iPicHeight: i32,
    iPicStride: i32,
    pFrameSad: *mut i32,
    pSad8x8: *mut i32,
) {
    let mut tmp_ref = pRefData;
    let mut tmp_cur = pCurData;
    let iMbWidth = iPicWidth >> 4;
    let mb_height = iPicHeight >> 4;
    let mut mb_index = 0isize;
    let pic_stride_x8 = (iPicStride << 3) as isize;
    let step = ((iPicStride << 4) - iPicWidth) as isize;

    *pFrameSad = 0;
    for _i in 0..mb_height {
        for _j in 0..iMbWidth {
            // The four quadrants in the C++'s order: top-left, top-right,
            // bottom-left, bottom-right.
            for (n, offset) in [
                (0isize, 0isize),
                (1, 8),
                (2, pic_stride_x8),
                (3, pic_stride_x8 + 8),
            ] {
                let mut l_sad = 0i32;
                let mut tmp_cur_row = tmp_cur.offset(offset);
                let mut tmp_ref_row = tmp_ref.offset(offset);
                for _k in 0..8 {
                    for l in 0..8isize {
                        let diff =
                            (*tmp_cur_row.offset(l) as i32 - *tmp_ref_row.offset(l) as i32).abs();
                        l_sad += diff;
                    }
                    tmp_cur_row = tmp_cur_row.offset(iPicStride as isize);
                    tmp_ref_row = tmp_ref_row.offset(iPicStride as isize);
                }
                *pFrameSad += l_sad;
                *pSad8x8.offset((mb_index << 2) + n) = l_sad;
            }

            tmp_ref = tmp_ref.offset(16);
            tmp_cur = tmp_cur.offset(16);
            mb_index += 1;
        }
        tmp_ref = tmp_ref.offset(step);
        tmp_cur = tmp_cur.offset(step);
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
    let mut tmp_ref = pRefData;
    let mut tmp_cur = pCurData;
    let iMbWidth = iPicWidth >> 4;
    let mb_height = iPicHeight >> 4;
    let mut mb_index = 0isize;
    let pic_stride_x8 = (iPicStride << 3) as isize;
    let step = ((iPicStride << 4) - iPicWidth) as isize;

    *pFrameSad = 0;
    for _i in 0..mb_height {
        for _j in 0..iMbWidth {
            *pSum16x16.offset(mb_index) = 0;
            *psqsum16x16.offset(mb_index) = 0;

            // The four quadrants in the C++'s order: top-left, top-right,
            // bottom-left, bottom-right.
            for (n, offset) in [
                (0isize, 0isize),
                (1, 8),
                (2, pic_stride_x8),
                (3, pic_stride_x8 + 8),
            ] {
                let mut l_sad = 0i32;
                let mut l_sum = 0i32;
                let mut l_sqsum = 0i32;
                let mut tmp_cur_row = tmp_cur.offset(offset);
                let mut tmp_ref_row = tmp_ref.offset(offset);
                for _k in 0..8 {
                    for l in 0..8isize {
                        let cur = *tmp_cur_row.offset(l) as i32;
                        l_sad += (cur - *tmp_ref_row.offset(l) as i32).abs();
                        l_sum += cur;
                        l_sqsum += cur * cur;
                    }
                    tmp_cur_row = tmp_cur_row.offset(iPicStride as isize);
                    tmp_ref_row = tmp_ref_row.offset(iPicStride as isize);
                }
                *pFrameSad += l_sad;
                *pSad8x8.offset((mb_index << 2) + n) = l_sad;
                *pSum16x16.offset(mb_index) += l_sum;
                *psqsum16x16.offset(mb_index) += l_sqsum;
            }

            tmp_ref = tmp_ref.offset(16);
            tmp_cur = tmp_cur.offset(16);
            mb_index += 1;
        }
        tmp_ref = tmp_ref.offset(step);
        tmp_cur = tmp_cur.offset(step);
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
    let mut tmp_ref = pRefData;
    let mut tmp_cur = pCurData;
    let iMbWidth = iPicWidth >> 4;
    let mb_height = iPicHeight >> 4;
    let mut mb_index = 0isize;
    let pic_stride_x8 = (iPicStride << 3) as isize;
    let step = ((iPicStride << 4) - iPicWidth) as isize;

    *pFrameSad = 0;
    for _i in 0..mb_height {
        for _j in 0..iMbWidth {
            *pSum16x16.offset(mb_index) = 0;
            *psqsum16x16.offset(mb_index) = 0;
            *psqdiff16x16.offset(mb_index) = 0;

            for (n, offset) in QUADRANTS(pic_stride_x8) {
                let (mut l_sad, mut l_sqdiff, mut l_sum, mut l_sqsum) = (0i32, 0i32, 0i32, 0i32);
                let mut tmp_cur_row = tmp_cur.offset(offset);
                let mut tmp_ref_row = tmp_ref.offset(offset);
                for _k in 0..8 {
                    for l in 0..8isize {
                        let cur = *tmp_cur_row.offset(l) as i32;
                        let diff = (cur - *tmp_ref_row.offset(l) as i32).abs();
                        l_sad += diff;
                        l_sqdiff += diff * diff;
                        l_sum += cur;
                        l_sqsum += cur * cur;
                    }
                    tmp_cur_row = tmp_cur_row.offset(iPicStride as isize);
                    tmp_ref_row = tmp_ref_row.offset(iPicStride as isize);
                }
                *pFrameSad += l_sad;
                *pSad8x8.offset((mb_index << 2) + n) = l_sad;
                *pSum16x16.offset(mb_index) += l_sum;
                *psqsum16x16.offset(mb_index) += l_sqsum;
                *psqdiff16x16.offset(mb_index) += l_sqdiff;
            }

            tmp_ref = tmp_ref.offset(16);
            tmp_cur = tmp_cur.offset(16);
            mb_index += 1;
        }
        tmp_ref = tmp_ref.offset(step);
        tmp_cur = tmp_cur.offset(step);
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
    let mut tmp_ref = pRefData;
    let mut tmp_cur = pCurData;
    let iMbWidth = iPicWidth >> 4;
    let mb_height = iPicHeight >> 4;
    let mut mb_index = 0isize;
    let pic_stride_x8 = (iPicStride << 3) as isize;
    let step = ((iPicStride << 4) - iPicWidth) as isize;

    *pFrameSad = 0;
    for _i in 0..mb_height {
        for _j in 0..iMbWidth {
            for (n, offset) in QUADRANTS(pic_stride_x8) {
                let (mut l_sad, mut l_sd, mut l_mad) = (0i32, 0i32, 0i32);
                let mut tmp_cur_row = tmp_cur.offset(offset);
                let mut tmp_ref_row = tmp_ref.offset(offset);
                for _k in 0..8 {
                    for l in 0..8isize {
                        let diff = *tmp_cur_row.offset(l) as i32 - *tmp_ref_row.offset(l) as i32;
                        let abs_diff = diff.abs();
                        l_sd += diff;
                        l_sad += abs_diff;
                        if abs_diff > l_mad {
                            l_mad = abs_diff;
                        }
                    }
                    tmp_cur_row = tmp_cur_row.offset(iPicStride as isize);
                    tmp_ref_row = tmp_ref_row.offset(iPicStride as isize);
                }
                *pFrameSad += l_sad;
                *pSad8x8.offset((mb_index << 2) + n) = l_sad;
                *pSd8x8.offset((mb_index << 2) + n) = l_sd;
                *pMad8x8.offset((mb_index << 2) + n) = l_mad as u8;
            }

            tmp_ref = tmp_ref.offset(16);
            tmp_cur = tmp_cur.offset(16);
            mb_index += 1;
        }
        tmp_ref = tmp_ref.offset(step);
        tmp_cur = tmp_cur.offset(step);
    }
}

/// `VAACalcSadSsdBgd_c` — `vaacalcfuncs.cpp:640`. Everything the other four
/// compute, in one pass.
///
/// Note it squares `abs_diff`, not `diff`, where `VAACalcSadSsd_c` squares the
/// already-absolute `diff`. Same value; kept as written.
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
    let mut tmp_ref = pRefData;
    let mut tmp_cur = pCurData;
    let iMbWidth = iPicWidth >> 4;
    let mb_height = iPicHeight >> 4;
    let mut mb_index = 0isize;
    let pic_stride_x8 = (iPicStride << 3) as isize;
    let step = ((iPicStride << 4) - iPicWidth) as isize;

    *pFrameSad = 0;
    for _i in 0..mb_height {
        for _j in 0..iMbWidth {
            *pSum16x16.offset(mb_index) = 0;
            *psqsum16x16.offset(mb_index) = 0;
            *psqdiff16x16.offset(mb_index) = 0;

            for (n, offset) in QUADRANTS(pic_stride_x8) {
                let (mut l_sad, mut l_sqdiff, mut l_sum, mut l_sqsum, mut l_sd, mut l_mad) =
                    (0i32, 0i32, 0i32, 0i32, 0i32, 0i32);
                let mut tmp_cur_row = tmp_cur.offset(offset);
                let mut tmp_ref_row = tmp_ref.offset(offset);
                for _k in 0..8 {
                    for l in 0..8isize {
                        let cur = *tmp_cur_row.offset(l) as i32;
                        let diff = cur - *tmp_ref_row.offset(l) as i32;
                        let abs_diff = diff.abs();
                        l_sd += diff;
                        if abs_diff > l_mad {
                            l_mad = abs_diff;
                        }
                        l_sad += abs_diff;
                        l_sqdiff += abs_diff * abs_diff;
                        l_sum += cur;
                        l_sqsum += cur * cur;
                    }
                    tmp_cur_row = tmp_cur_row.offset(iPicStride as isize);
                    tmp_ref_row = tmp_ref_row.offset(iPicStride as isize);
                }
                *pFrameSad += l_sad;
                *pSad8x8.offset((mb_index << 2) + n) = l_sad;
                *pSum16x16.offset(mb_index) += l_sum;
                *psqsum16x16.offset(mb_index) += l_sqsum;
                *psqdiff16x16.offset(mb_index) += l_sqdiff;
                *pSd8x8.offset((mb_index << 2) + n) = l_sd;
                *pMad8x8.offset((mb_index << 2) + n) = l_mad as u8;
            }

            tmp_ref = tmp_ref.offset(16);
            tmp_cur = tmp_cur.offset(16);
            mb_index += 1;
        }
        tmp_ref = tmp_ref.offset(step);
        tmp_cur = tmp_cur.offset(step);
    }
}

/// The four 8x8 quadrants of a macroblock in the order every `VAACalc*` kernel
/// unrolls them: top-left, top-right, bottom-left, bottom-right.
#[inline]
#[allow(non_snake_case)]
fn QUADRANTS(pic_stride_x8: isize) -> [(isize, isize); 4] {
    [
        (0, 0),
        (1, 8),
        (2, pic_stride_x8),
        (3, pic_stride_x8 + 8),
    ]
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

/// One 8x8 quadrant's statistics, from two plane slices anchored at its top-left
/// sample.
///
/// C++: the innermost `for (k) for (l)` of every kernel in
/// `codec/processing/src/vaacalc/vaacalcfuncs.cpp` — the five copies differ in
/// nothing but which accumulators they keep.
///
/// **Arithmetic parity.** Every accumulator is `i32` and every operation is the
/// C++'s, in the C++'s width; nothing is widened and no `wrapping_*` the old code
/// lacks is added. `VAACalcSadSsd_c` squares an already-absolute difference where
/// `VAACalcSadSsdBgd_c` squares `abs_diff` — the same value, so one form serves both,
/// and the old port's comment saying so is preserved on the shim.
#[inline(always)]
fn block_stats<const VAR: bool, const SQDIFF: bool, const BGD: bool>(
    cur: &[u8],
    refp: &[u8],
    stride: usize,
) -> BlockStats {
    let mut s = BlockStats::default();
    for k in 0..8 {
        let base = k * stride;
        // Fixed-size windows, so the eight-sample inner loop carries no bounds check
        // at all and the two range checks land once per row (plan §7.4's idiom list).
        let c: &[u8; 8] = cur[base..base + 8].try_into().unwrap();
        let r: &[u8; 8] = refp[base..base + 8].try_into().unwrap();
        for (&cv, &rv) in c.iter().zip(r.iter()) {
            let cur_sample = cv as i32;
            let diff = cur_sample - rv as i32;
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
    s
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
    let quadrants = [0usize, 8, stride * 8, stride * 8 + 8];
    let step = ((pic_stride << 4) - pic_width) as usize;

    let mut frame_sad = 0i32;
    let mut mb_index = 0usize;
    let mut row_origin = 0usize;
    for _ in 0..mb_height {
        let mut mb_origin = row_origin;
        for _ in 0..mb_width {
            let mut stats = [BlockStats::default(); 4];
            for (n, quadrant) in quadrants.iter().enumerate() {
                stats[n] = block_stats::<VAR, SQDIFF, BGD>(
                    &cur[mb_origin + quadrant..],
                    &refp[mb_origin + quadrant..],
                    stride,
                );
                frame_sad += stats[n].sad;
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
    /// `CVAACalculation::Set` — copies the caller's parameter block.
    ///
    /// # Safety
    /// `pParam` must point to a valid `SVAACalcParam`.
    pub unsafe fn Set(&mut self, _iType: i32, pParam: *mut core::ffi::c_void) -> i32 {
        if pParam.is_null() {
            return RET_INVALIDPARAM;
        }
        self.m_sCalcParam = *(pParam as *mut SVAACalcParam);
        RET_SUCCESS
    }

    /// `CVAACalculation::Process` — `vaacalculation.cpp:120`.
    ///
    /// # Safety
    /// The pixel maps must describe readable planes, and `m_sCalcParam.pCalcResult`
    /// must point at an `SVAACalcResult` whose `pSad8x8` has room for the picture.
    pub unsafe fn Process(
        &mut self,
        _iType: i32,
        pCurData: *mut u8,
        pRefData: *mut u8,
        iPicWidth: i32,
        iPicHeight: i32,
        iPicStride: i32,
    ) -> i32 {
        let pResult: *mut SVAACalcResult = self.m_sCalcParam.pCalcResult;
        if pCurData.is_null() || pRefData.is_null() {
            return RET_INVALIDPARAM;
        }
        if pResult.is_null() {
            return RET_INVALIDPARAM;
        }

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
}
