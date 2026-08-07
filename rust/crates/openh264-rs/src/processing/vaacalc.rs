#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/vaacalc/` — the VAA (video analysis) statistics
//! plugin reached through `METHOD_VAA_STATISTICS`.
//!
//! Only the `pfVAACalcSad` kernel is translated. The other three
//! (`pfVAACalcSadVar`, `pfVAACalcSadSsd`, `pfVAACalcSadBgd`,
//! `pfVAACalcSadSsdBgd`) are selected by `iCalcVar`/`iCalcSsd`/`iCalcBgd`, which
//! `CWelsPreProcess::AnalyzeSpatialPic` derives from rate control, adaptive
//! quantisation and background detection — all three off in the gate
//! configuration. [`CVAACalculation::Process`] returns `RET_NOTSUPPORTED` for those
//! combinations rather than silently computing the wrong statistic.

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
