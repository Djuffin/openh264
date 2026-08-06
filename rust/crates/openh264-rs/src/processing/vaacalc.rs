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

        if self.m_sCalcParam.iCalcBgd || self.m_sCalcParam.iCalcSsd || self.m_sCalcParam.iCalcVar {
            // pfVAACalcSadBgd / pfVAACalcSadSsdBgd / pfVAACalcSadSsd / pfVAACalcSadVar
            // are not translated; see the module docs.
            return RET_NOTSUPPORTED;
        }

        VAACalcSad_c(
            pCurData,
            pRefData,
            iPicWidth,
            iPicHeight,
            iPicStride,
            &mut (*pResult).iFrameSad as *mut i32,
            (*pResult).pSad8x8 as *mut i32,
        );
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
