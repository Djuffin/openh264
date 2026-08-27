#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/complexityanalysis/ComplexityAnalysis.cpp` — the
//! plugin reached through `METHOD_COMPLEXITY_ANALYSIS`.
//!
//! `CWelsPreProcess::AnalyzePictureComplexity` selects one of three modes from the
//! rate-control mode and the slice type (`wels_preprocess.cpp:896-908`):
//!
//! | `iRCMode` | slice | mode |
//! |---|---|---|
//! | `RC_QUALITY_MODE` | P | `FRAME_SAD` |
//! | `RC_BITRATE_MODE`, `RC_TIMESTAMP_MODE` | P | `GOM_SAD` |
//! | `RC_BITRATE_MODE`, `RC_TIMESTAMP_MODE` | I | `GOM_VAR` |
//! | anything else | — | not called |
//!
//! `Get` returns `iFrameComplexity`, which `RcCalculatePictureQp` and
//! `RcCalculateIdrQp` divide by the running complexity mean to scale the QP step;
//! `GOM_SAD`/`GOM_VAR` additionally fill `pWelsSvcRc->pCurrentFrameGomSad`, which
//! `RcGomTargetBits` uses to split a slice's bit budget between GOMs.
//!
//! `CComplexityAnalysisScreen` (`METHOD_COMPLEXITY_ANALYSIS_SCREEN`) is not ported;
//! it is reached only from `SCREEN_CONTENT_REAL_TIME`.
//!
//! ## Unsigned arithmetic is load-bearing
//!
//! `uiGomSad`, `uiSampleSum` and `uiSquareSum` are `uint32_t` in C++ and the
//! `GOM_VAR` expression squares a sum that overflows 32 bits for any realistic GOM
//! (20 macroblocks * 256 samples * 255 = 1.3e6, squared = 1.7e12). The wrap is part
//! of the result, so every one of these is a `u32` with `wrapping_*` here.

#![deny(unsafe_code)]

use crate::encoder::wels_preprocess::{SComplexityAnalysisParam, SPixMap, SVAACalcResult};

use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};

/// `EComplexityAnalysisMode` — `IWelsVP.h:215`.
pub const FRAME_SAD: i32 = 0;
pub const GOM_SAD: i32 = -1;
pub const GOM_VAR: i32 = -2;

/// `MB_WIDTH_LUMA` — `wels_const_common.h:50`.
const MB_WIDTH_LUMA: i32 = 16;

/// `IS_INTRA(type)` — `wels_common_defs.h:305`, `(type) & MB_TYPE_INTRA`.
#[inline]
fn IS_INTRA(uiMbType: u32) -> bool {
    (uiMbType & crate::encoder::svc_encode_mb::MB_TYPE_INTRA) != 0
}

#[inline]
fn WELS_MIN(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}

/// `CComplexityAnalysis` — `ComplexityAnalysis.h:61`.
///
/// The C++ object keeps the whole `SComplexityAnalysisParam` by value (`Set` copies
/// it in, `Get` copies `iFrameComplexity` back out), so this does too.
pub struct CComplexityAnalysis {
    /// `m_pfGomSad`. Selected per call by `InitGomSadFunc`, so it is not stored as a
    /// function pointer here — `iCalcBgd` is the whole selection.
    pub m_sComplexityAnalysisParam: SComplexityAnalysisParam,
}

impl Default for CComplexityAnalysis {
    fn default() -> Self {
        // `CComplexityAnalysis::CComplexityAnalysis` memsets the param to zero.
        Self {
            m_sComplexityAnalysisParam: SComplexityAnalysisParam::default(),
        }
    }
}

impl CComplexityAnalysis {
    /// `CComplexityAnalysis::Set` — copies the caller's parameter block in whole.
    /// Typed since Phase 6 session B (the `IWelsVP` vtable's `void*` is gone).
    pub fn Set(&mut self, param: &SComplexityAnalysisParam) -> i32 {
        self.m_sComplexityAnalysisParam = *param;
        RET_SUCCESS
    }

    /// `CComplexityAnalysis::Get` — writes back `iFrameComplexity` and nothing else.
    pub fn Get(&self, param: &mut SComplexityAnalysisParam) -> i32 {
        param.iFrameComplexity = self.m_sComplexityAnalysisParam.iFrameComplexity;
        RET_SUCCESS
    }

    /// `CComplexityAnalysis::Process`. `calc` is the VAA statistics of this picture
    /// pair, handed over at the call (the C++ stored `pCalcResult` in the parameter
    /// block; take what you reach).
    ///
    /// # Safety
    /// The pointers stored by the preceding [`Set`](Self::Set) must still be valid,
    /// `pSrcPixMap` must describe the current picture, and `calc`'s arrays must cover
    /// its macroblocks.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    /// **T9.X** — `pGomComplexity` and `pGomForegroundBlockNum` arrive as slices
    /// rather than as two `*mut`-i32 on the parameter block. They were never this
    /// object's memory: they point into the *rate controller's* `pCurrentFrameGomSad`
    /// and `pGomForegroundBlockNum` `Vec`s, which `AnalyzePictureComplexity` aims at
    /// them one line before the call. Handing them over at the call is the move
    /// session B made for `pCalcResult`, and it is what retires `SVAAFrameInfo`'s
    /// `*mut`-i32 `!Sync` reason (F67/F164).
    pub unsafe fn Process(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomComplexity: &mut [i32],
        pGomForegroundBlockNum: &mut [i32],
    ) -> i32 {
        match self.m_sComplexityAnalysisParam.iComplexityAnalysisMode {
            FRAME_SAD => self.AnalyzeFrameComplexityViaSad(
                pSrcPixMap, pRefPixMap, calc, pGomForegroundBlockNum,
            ),
            GOM_SAD => self.AnalyzeGomComplexityViaSad(
                pSrcPixMap, pRefPixMap, calc, pGomComplexity, pGomForegroundBlockNum,
            ),
            GOM_VAR => self.AnalyzeGomComplexityViaVar(
                pSrcPixMap, pRefPixMap, calc, pGomComplexity, pGomForegroundBlockNum,
            ),
            _ => return RET_INVALIDPARAM,
        }
        RET_SUCCESS
    }

    /// `CComplexityAnalysis::AnalyzeFrameComplexityViaSad` — `ComplexityAnalysis.cpp:96`.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn AnalyzeFrameComplexityViaSad(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomForegroundBlockNum: &mut [i32],
    ) {
        self.m_sComplexityAnalysisParam.iFrameComplexity = calc.iFrameSad as i64;

        if self.m_sComplexityAnalysisParam.iCalcBgd {
            //BGD control
            self.m_sComplexityAnalysisParam.iFrameComplexity =
                self.GetFrameSadExcludeBackground(
                    pSrcPixMap, pRefPixMap, calc, pGomForegroundBlockNum,
                ) as i64;
        }
    }

    /// `CComplexityAnalysis::GetFrameSadExcludeBackground` — `ComplexityAnalysis.cpp:107`.
    ///
    /// The C++ returns `int32_t` from a `uint32_t` accumulator; the sign of that
    /// conversion is what `iFrameComplexity` then sign-extends, so the cast chain is
    /// kept as-is.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn GetFrameSadExcludeBackground(
        &mut self,
        pSrcPixMap: &SPixMap,
        _pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomForegroundBlockNum: &mut [i32],
    ) -> i32 {
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iMbWidth = iWidth >> 4;
        let iMbHeight = iHeight >> 4;
        let iMbNum = iMbWidth * iMbHeight;

        let iMbNumInGom = self.m_sComplexityAnalysisParam.iMbNumInGom;
        let iGomMbNum = (iMbNum + iMbNumInGom - 1) / iMbNumInGom;

        let pBackgroundMbFlag = self.m_sComplexityAnalysisParam.pBackgroundMbFlag;
        let uiRefMbType = self.m_sComplexityAnalysisParam.uiRefMbType;

        let mut uiFrameSad: u32 = 0;
        for j in 0..iGomMbNum {
            let iGomMbStartIndex = j * iMbNumInGom;
            let iGomMbEndIndex = WELS_MIN((j + 1) * iMbNumInGom, iMbNum);

            for i in iGomMbStartIndex..iGomMbEndIndex {
                if *pBackgroundMbFlag.offset(i as isize) == 0
                    || IS_INTRA(*uiRefMbType.offset(i as isize))
                {
                    pGomForegroundBlockNum[j as usize] += 1;
                    let sad8x8 = &calc.pSad8x8[(i as isize) as usize];
                    uiFrameSad = uiFrameSad.wrapping_add(sad8x8[0] as u32);
                    uiFrameSad = uiFrameSad.wrapping_add(sad8x8[1] as u32);
                    uiFrameSad = uiFrameSad.wrapping_add(sad8x8[2] as u32);
                    uiFrameSad = uiFrameSad.wrapping_add(sad8x8[3] as u32);
                }
            }
        }

        uiFrameSad as i32
    }

    /// `CComplexityAnalysis::AnalyzeGomComplexityViaSad` — `ComplexityAnalysis.cpp:169`.
    ///
    /// `InitGomSadFunc` picks `GomSampleSad` or `GomSampleSadExceptBackground` from
    /// `iCalcBgd`; both are inlined below because the choice is a single predicate.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn AnalyzeGomComplexityViaSad(
        &mut self,
        pSrcPixMap: &SPixMap,
        _pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomComplexity: &mut [i32],
        pGomForegroundBlockNum: &mut [i32],
    ) {
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iMbWidth = iWidth >> 4;
        let iMbHeight = iHeight >> 4;
        let iMbNum = iMbWidth * iMbHeight;

        let iMbNumInGom = self.m_sComplexityAnalysisParam.iMbNumInGom;
        let iGomMbNum = (iMbNum + iMbNumInGom - 1) / iMbNumInGom;

        let pBackgroundMbFlag = self.m_sComplexityAnalysisParam.pBackgroundMbFlag;
        let uiRefMbType = self.m_sComplexityAnalysisParam.uiRefMbType;

        let mut uiFrameSad: u32 = 0;
        // `InitGomSadFunc (m_pfGomSad, iCalcBgd)`.
        let bExceptBackground = self.m_sComplexityAnalysisParam.iCalcBgd;

        for j in 0..iGomMbNum {
            let mut uiGomSad: u32 = 0;

            let iGomMbStartIndex = j * iMbNumInGom;
            let iGomMbEndIndex = WELS_MIN((j + 1) * iMbNumInGom, iMbNum);
            let mut iGomMbRowNum =
                (iGomMbEndIndex + iMbWidth - 1) / iMbWidth - iGomMbStartIndex / iMbWidth;

            let mut iMbStartIndex = iGomMbStartIndex;
            let mut iMbEndIndex =
                WELS_MIN((iMbStartIndex / iMbWidth + 1) * iMbWidth, iGomMbEndIndex);

            loop {
                for i in iMbStartIndex..iMbEndIndex {
                    // The fourth argument of the C++ call is
                    // `pBackgroundMbFlag[i] && !IS_INTRA (uiRefMbType[i])`. Only
                    // `GomSampleSadExceptBackground` reads it, so it is evaluated
                    // only on that arm here — C++ evaluates it either way, but
                    // `GomSampleSad` discards it, and on the `GomSampleSad` arm
                    // `uiRefMbType` may legitimately be null (`AnalyzePictureComplexity`
                    // only assigns it when there is a reference picture).
                    let uiBackgroundMbFlag = bExceptBackground
                        && *pBackgroundMbFlag.offset(i as isize) != 0
                        && !IS_INTRA(*uiRefMbType.offset(i as isize));
                    if !bExceptBackground || !uiBackgroundMbFlag {
                        pGomForegroundBlockNum[j as usize] += 1;
                        let sad8x8 = &calc.pSad8x8[(i as isize) as usize];
                        uiGomSad = uiGomSad.wrapping_add(sad8x8[0] as u32);
                        uiGomSad = uiGomSad.wrapping_add(sad8x8[1] as u32);
                        uiGomSad = uiGomSad.wrapping_add(sad8x8[2] as u32);
                        uiGomSad = uiGomSad.wrapping_add(sad8x8[3] as u32);
                    }
                }

                iMbStartIndex = iMbEndIndex;
                iMbEndIndex = WELS_MIN(iMbEndIndex + iMbWidth, iGomMbEndIndex);

                iGomMbRowNum -= 1;
                if iGomMbRowNum == 0 {
                    break;
                }
            }
            pGomComplexity[j as usize] = uiGomSad as i32;
            uiFrameSad = uiFrameSad.wrapping_add(pGomComplexity[j as usize] as u32);
        }
        self.m_sComplexityAnalysisParam.iFrameComplexity = uiFrameSad as i64;
    }

    /// `CComplexityAnalysis::AnalyzeGomComplexityViaVar` — `ComplexityAnalysis.cpp:222`.
    fn AnalyzeGomComplexityViaVar(
        &mut self,
        pSrcPixMap: &SPixMap,
        _pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomComplexity: &mut [i32],
        pGomForegroundBlockNum: &mut [i32],
    ) {
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iMbWidth = iWidth >> 4;
        let iMbHeight = iHeight >> 4;
        let iMbNum = iMbWidth * iMbHeight;

        let iMbNumInGom = self.m_sComplexityAnalysisParam.iMbNumInGom;
        let iGomMbNum = (iMbNum + iMbNumInGom - 1) / iMbNumInGom;

        let mut uiFrameSad: u32 = 0;

        for j in 0..iGomMbNum {
            let mut uiSampleSum: u32 = 0;
            let mut uiSquareSum: u32 = 0;

            let iGomMbStartIndex = j * iMbNumInGom;
            let iGomMbEndIndex = WELS_MIN((j + 1) * iMbNumInGom, iMbNum);
            let mut iGomMbRowNum =
                (iGomMbEndIndex + iMbWidth - 1) / iMbWidth - iGomMbStartIndex / iMbWidth;

            let mut iMbStartIndex = iGomMbStartIndex;
            let mut iMbEndIndex =
                WELS_MIN((iMbStartIndex / iMbWidth + 1) * iMbWidth, iGomMbEndIndex);

            let iGomSampleNum = (iMbEndIndex - iMbStartIndex) * MB_WIDTH_LUMA * MB_WIDTH_LUMA;

            loop {
                for i in iMbStartIndex..iMbEndIndex {
                    uiSampleSum = uiSampleSum
                        .wrapping_add(calc.pSum16x16[(i as isize) as usize] as u32);
                    uiSquareSum = uiSquareSum.wrapping_add(
                        calc.pSumOfSquare16x16[(i as isize) as usize] as u32,
                    );
                }

                iMbStartIndex = iMbEndIndex;
                iMbEndIndex = WELS_MIN(iMbEndIndex + iMbWidth, iGomMbEndIndex);

                iGomMbRowNum -= 1;
                if iGomMbRowNum == 0 {
                    break;
                }
            }

            // `uiSquareSum - (uiSampleSum * uiSampleSum / iGomSampleNum)` — all three
            // operations are 32-bit unsigned in C++, and the square overflows.
            let mean = uiSampleSum
                .wrapping_mul(uiSampleSum)
                .wrapping_div(iGomSampleNum as u32);
            pGomComplexity[j as usize] = uiSquareSum.wrapping_sub(mean) as i32;
            uiFrameSad = uiFrameSad.wrapping_add(pGomComplexity[j as usize] as u32);
        }
        self.m_sComplexityAnalysisParam.iFrameComplexity = uiFrameSad as i64;
    }
}
