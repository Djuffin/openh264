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
//! [`CComplexityAnalysisScreen`] (`METHOD_COMPLEXITY_ANALYSIS_SCREEN`), at the foot
//! of this file since P10.2.C5, is the screen-content counterpart: a different
//! measurement — the cheapest of vertical intra, horizontal intra and collocated
//! inter, per macroblock, summed into GOM buckets — reached only from
//! `SCREEN_CONTENT_REAL_TIME`.
//!
//! ## Unsigned arithmetic is load-bearing
//!
//! `uiGomSad`, `uiSampleSum` and `uiSquareSum` are `uint32_t` in C++ and the
//! `GOM_VAR` expression squares a sum that overflows 32 bits for any realistic GOM
//! (20 macroblocks * 256 samples * 255 = 1.3e6, squared = 1.7e12). The wrap is part
//! of the result, so every one of these is a `u32` with `wrapping_*` here.

// **S11.5 (step 5): sealed.** the complexity-analysis pass holds no `unsafe` at all —
// no product allow and no test instrument — so the `deny` it carried
// since its conversion becomes `forbid`, which no inner `allow` can
// reopen. This is the end state for a file that is simply done.
#![forbid(unsafe_code)]

use crate::common::intra_pred_common::{i16x16_luma_pred_h, i16x16_luma_pred_v};
use crate::common::sad_common::sample_sad;
use crate::encoder::wels_preprocess::{
    SComplexityAnalysisParam, SComplexityAnalysisScreenParam, SPixMap, SVAACalcResult,
};
use crate::safe::plane::PlaneCursor;

use super::scene_change_detection::ScdPlanes;

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
    pub fn Process(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomComplexity: &mut [i32],
        pGomForegroundBlockNum: &mut [i32],
        // S10.9: the two per-macroblock arrays, as slices — they were raws on
        // `SComplexityAnalysisParam` and are the caller's storage, not this
        // plugin's, exactly as the two GOM arrays above already were.
        pBackgroundMbFlag: &[i8],
        uiRefMbType: &[u32],
    ) -> i32 {
        match self.m_sComplexityAnalysisParam.iComplexityAnalysisMode {
            FRAME_SAD => self.AnalyzeFrameComplexityViaSad(
                pSrcPixMap, pRefPixMap, calc, pGomForegroundBlockNum,
                pBackgroundMbFlag, uiRefMbType,
            ),
            GOM_SAD => self.AnalyzeGomComplexityViaSad(
                pSrcPixMap, pRefPixMap, calc, pGomComplexity, pGomForegroundBlockNum,
                pBackgroundMbFlag, uiRefMbType,
            ),
            GOM_VAR => self.AnalyzeGomComplexityViaVar(
                pSrcPixMap, pRefPixMap, calc, pGomComplexity, pGomForegroundBlockNum,
            ),
            _ => return RET_INVALIDPARAM,
        }
        RET_SUCCESS
    }

    /// `CComplexityAnalysis::AnalyzeFrameComplexityViaSad` — `ComplexityAnalysis.cpp:96`.
    fn AnalyzeFrameComplexityViaSad(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomForegroundBlockNum: &mut [i32],
        pBackgroundMbFlag: &[i8],
        uiRefMbType: &[u32],
    ) {
        self.m_sComplexityAnalysisParam.iFrameComplexity = calc.iFrameSad as i64;

        if self.m_sComplexityAnalysisParam.iCalcBgd {
            //BGD control
            self.m_sComplexityAnalysisParam.iFrameComplexity =
                self.GetFrameSadExcludeBackground(
                    pSrcPixMap, pRefPixMap, calc, pGomForegroundBlockNum,
                    pBackgroundMbFlag, uiRefMbType,
                ) as i64;
        }
    }

    /// `CComplexityAnalysis::GetFrameSadExcludeBackground` — `ComplexityAnalysis.cpp:107`.
    ///
    /// The C++ returns `int32_t` from a `uint32_t` accumulator; the sign of that
    /// conversion is what `iFrameComplexity` then sign-extends, so the cast chain is
    /// kept as-is.
    fn GetFrameSadExcludeBackground(
        &mut self,
        pSrcPixMap: &SPixMap,
        _pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomForegroundBlockNum: &mut [i32],
        pBackgroundMbFlag: &[i8],
        uiRefMbType: &[u32],
    ) -> i32 {
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iMbWidth = iWidth >> 4;
        let iMbHeight = iHeight >> 4;
        let iMbNum = iMbWidth * iMbHeight;

        let iMbNumInGom = self.m_sComplexityAnalysisParam.iMbNumInGom;
        let iGomMbNum = (iMbNum + iMbNumInGom - 1) / iMbNumInGom;


        let mut uiFrameSad: u32 = 0;
        for j in 0..iGomMbNum {
            let iGomMbStartIndex = j * iMbNumInGom;
            let iGomMbEndIndex = WELS_MIN((j + 1) * iMbNumInGom, iMbNum);

            for i in iGomMbStartIndex..iGomMbEndIndex {
                if pBackgroundMbFlag[i as usize] == 0
                    || IS_INTRA(uiRefMbType[i as usize])
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
    fn AnalyzeGomComplexityViaSad(
        &mut self,
        pSrcPixMap: &SPixMap,
        _pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        pGomComplexity: &mut [i32],
        pGomForegroundBlockNum: &mut [i32],
        pBackgroundMbFlag: &[i8],
        uiRefMbType: &[u32],
    ) {
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iMbWidth = iWidth >> 4;
        let iMbHeight = iHeight >> 4;
        let iMbNum = iMbWidth * iMbHeight;

        let iMbNumInGom = self.m_sComplexityAnalysisParam.iMbNumInGom;
        let iGomMbNum = (iMbNum + iMbNumInGom - 1) / iMbNumInGom;


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
                        && pBackgroundMbFlag[i as usize] != 0
                        && !IS_INTRA(uiRefMbType[i as usize]);
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

// ============================================================================
// METHOD_COMPLEXITY_ANALYSIS_SCREEN — `ComplexityAnalysis.cpp:272-494`
// ============================================================================

/// `CComplexityAnalysisScreen` — `ComplexityAnalysis.h:87-105`,
/// `ComplexityAnalysis.cpp:272-494`.
///
/// A different measurement from the camera plugin above, not a variant of it: per
/// 16x16 macroblock it takes the cost of the cheapest of three predictions —
/// vertical intra, horizontal intra, and (on a P frame) the collocated inter block —
/// and sums those costs into GOM buckets `iMbRowInGom` macroblock rows tall.
/// `AnalyzePictureComplexity` feeds the buckets to `RcGomTargetBits` and the frame
/// total to `RcUpdateFrameComplexity`; nothing here reaches the bitstream except
/// through the QP the rate controller then chooses.
///
/// **D-scc-10: the GOM array is the rate controller's**, borrowed from
/// `pWelsSvcRc.pCurrentFrameGomSad` at the call, exactly as the camera plugin's is.
/// `iGomNumInFrame` is written back as *this plugin's* count of buckets, overwriting
/// the `iGomSize` the caller staged there — that is what the C++ does, and the
/// caller must not "preserve" the old value.
#[derive(Debug, Default)]
pub struct CComplexityAnalysisScreen {
    pub m_ComplexityAnalysisParam: SComplexityAnalysisScreenParam,
}

impl CComplexityAnalysisScreen {
    /// `CComplexityAnalysisScreen::Set` — `ComplexityAnalysis.cpp:339-346`.
    pub fn Set(&mut self, pParam: &SComplexityAnalysisScreenParam) -> i32 {
        self.m_ComplexityAnalysisParam = *pParam;
        RET_SUCCESS
    }

    /// `CComplexityAnalysisScreen::Get` — `ComplexityAnalysis.cpp:348-355`. The whole
    /// block, so `iGomNumInFrame` and `iFrameComplexity` both travel back.
    pub fn Get(&self, pParam: &mut SComplexityAnalysisScreenParam) -> i32 {
        *pParam = self.m_ComplexityAnalysisParam;
        RET_SUCCESS
    }

    /// `CComplexityAnalysisScreen::Process` — `ComplexityAnalysis.cpp:316-337`.
    ///
    /// `pRef` is `None` where the C++ pointer is null; `planes.refp` may be empty in
    /// that case and is not read.
    pub fn Process(
        &mut self,
        pSrc: &SPixMap,
        pRef: Option<&SPixMap>,
        planes: &ScdPlanes<'_>,
        pGomComplexity: &mut [i32],
    ) -> i32 {
        let bScrollFlag = self.m_ComplexityAnalysisParam.sScrollResult.bScrollDetectFlag;
        let iIdrFlag = self.m_ComplexityAnalysisParam.iIdrFlag;
        let iScrollMvX = self.m_ComplexityAnalysisParam.sScrollResult.iScrollMvX;
        let iScrollMvY = self.m_ComplexityAnalysisParam.sScrollResult.iScrollMvY;

        if self.m_ComplexityAnalysisParam.iMbRowInGom <= 0 {
            return RET_INVALIDPARAM;
        }
        if iIdrFlag == 0 && pRef.is_none() {
            return RET_INVALIDPARAM;
        }

        // The C++'s three-way `if` at `:327-334`, in its order. The `pRef` of the two
        // inter arms is `Some` because the intra arm above took every `None`.
        if iIdrFlag != 0 || pRef.is_none() {
            self.GomComplexityAnalysisIntra(pSrc, planes, pGomComplexity);
        } else {
            let pRef = pRef.expect("the intra arm took every None");
            let bScroll = !(!bScrollFlag || (iScrollMvX == 0 && iScrollMvY == 0));
            self.GomComplexityAnalysisInter(pSrc, pRef, planes, bScroll, pGomComplexity);
        }

        RET_SUCCESS
    }

    /// `CComplexityAnalysisScreen::GomComplexityAnalysisIntra` —
    /// `ComplexityAnalysis.cpp:357-411`.
    ///
    /// **The C++ names its two SADs backwards** and the values are what matter:
    /// `m_pIntraFunc[0]` is `WelsI16x16LumaPredV_c`, the *vertical* prediction, and
    /// its cost is stored in the variable called `iBlockSadH`; `m_pIntraFunc[1]` is
    /// the horizontal prediction and its cost goes in `iBlockSadV`. Since only
    /// `WELS_MIN` of the pair is ever read, the swap is invisible in the C++ — so the
    /// locals below are named for what they hold and this note is the crossreference.
    ///
    /// The vertical prediction needs the row above the macroblock and the horizontal
    /// one the column to its left, which is why each is guarded by `j > 0` / `i > 0`
    /// and why the top-left macroblock contributes nothing (`if (i || j)`).
    fn GomComplexityAnalysisIntra(
        &mut self,
        pSrc: &SPixMap,
        planes: &ScdPlanes<'_>,
        pGomComplexity: &mut [i32],
    ) {
        let iWidth = pSrc.sRect.iRectWidth;
        let iHeight = pSrc.sRect.iRectHeight;
        let iBlockWidth = (iWidth >> 4).max(0) as usize;
        let iBlockHeight = (iHeight >> 4).max(0) as usize;

        let mut iGomSad: i32 = 0;
        let mut iIdx: usize = 0;
        let iStrideY = planes.cur_stride;

        // `ENFORCE_STACK_ALIGN_1D (uint8_t, iMemPredMb, 256, 16)` — the 16x16
        // prediction block, at stride 16. Alignment was for the SSE2 kernels; the
        // scalar port needs none.
        let mut iMemPredMb = [0u8; 256];

        self.m_ComplexityAnalysisParam.iFrameComplexity = 0;

        for j in 0..iBlockHeight {
            for i in 0..iBlockWidth {
                let pTmpCur = PlaneCursor::new(planes.cur, j * 16 * iStrideY + i * 16, iStrideY);
                let (mut iSadPredV, mut iSadPredH) = (i32::MAX, i32::MAX);
                if j > 0 {
                    let top: &[u8; 16] = pTmpCur.row(-1, 0, 16).try_into().expect("16 samples");
                    i16x16_luma_pred_v(&mut iMemPredMb, top);
                    let pred = PlaneCursor::new(&iMemPredMb, 0, 16);
                    iSadPredV = sample_sad::<16, 16, _>(&pTmpCur, &pred);
                }
                if i > 0 {
                    i16x16_luma_pred_h(&mut iMemPredMb, &pTmpCur);
                    let pred = PlaneCursor::new(&iMemPredMb, 0, 16);
                    iSadPredH = sample_sad::<16, 16, _>(&pTmpCur, &pred);
                }
                if i != 0 || j != 0 {
                    iGomSad += WELS_MIN(iSadPredV, iSadPredH);
                }

                if i == iBlockWidth - 1
                    && ((j + 1) % self.m_ComplexityAnalysisParam.iMbRowInGom as usize == 0
                        || j == iBlockHeight - 1)
                {
                    pGomComplexity[iIdx] = iGomSad;
                    self.m_ComplexityAnalysisParam.iFrameComplexity += iGomSad as i64;
                    iIdx += 1;
                    iGomSad = 0;
                }
            }
        }
        self.m_ComplexityAnalysisParam.iGomNumInFrame = iIdx as i32;
    }

    /// `CComplexityAnalysisScreen::GomComplexityAnalysisInter` —
    /// `ComplexityAnalysis.cpp:413-494`.
    ///
    /// The intra pair as above, plus the collocated inter SAD; the bucket takes the
    /// minimum of the three. There is no `if (i || j)` guard here — at the top-left
    /// macroblock both intra costs are `i32::MAX` and the minimum is the inter cost,
    /// which is exactly what the C++ computes.
    ///
    /// **Two upstream facts about the scroll branch, ported as they are.**
    ///
    /// 1. The scrolled reference is read at `pTmpRef - iScrollMvY * iStrideX +
    ///    iScrollMvX` (`:451`) — **minus** on Y where the scene-change detector
    ///    (`SceneChangeDetection.h:170`) adds it for the same vector. Two plugins,
    ///    one vector, opposite conventions.
    /// 2. The bounds test is `iBlockPointY + iScrollMvY` against `iHeight - 8` while
    ///    the read subtracts, and `iHeight - 8`/`iWidth - 8` are an *8x8* margin for a
    ///    read that is 16x16 wide. So the guard does not bound the read it guards: a
    ///    macroblock near the top with a positive vector passes the test and reads
    ///    above the picture.
    ///
    /// Neither is reachable from the encoder, and the reason is the caller, not the
    /// plugin: `AnalyzePictureComplexity` zeroes `sScrollResult` before every `Set`
    /// (`wels_preprocess.cpp:863-865`), so `bScrollFlag` is false on every call the
    /// encoder makes and this whole branch is dark. It is ported literally; if
    /// anything ever does reach it, `PlaneCursor`'s bounds check is the referee and a
    /// panic there is this note, not a new mystery.
    fn GomComplexityAnalysisInter(
        &mut self,
        pSrc: &SPixMap,
        pRef: &SPixMap,
        planes: &ScdPlanes<'_>,
        bScrollFlag: bool,
        pGomComplexity: &mut [i32],
    ) {
        let iWidth = pSrc.sRect.iRectWidth;
        let iHeight = pSrc.sRect.iRectHeight;
        let iBlockWidth = (iWidth >> 4).max(0) as usize;
        let iBlockHeight = (iHeight >> 4).max(0) as usize;

        let mut iGomSad: i32 = 0;
        let mut iIdx: usize = 0;

        let iScrollMvX = self.m_ComplexityAnalysisParam.sScrollResult.iScrollMvX;
        let iScrollMvY = self.m_ComplexityAnalysisParam.sScrollResult.iScrollMvY;

        // `iStrideX` is the reference's, `iStrideY` the source's — this kernel, unlike
        // `ScrollDetectionCore`, does keep them apart.
        let iStrideX = planes.ref_stride;
        let iStrideY = planes.cur_stride;
        debug_assert_eq!(iStrideX, pRef.iStride[0] as usize);

        let mut iMemPredMb = [0u8; 256];

        self.m_ComplexityAnalysisParam.iFrameComplexity = 0;

        for j in 0..iBlockHeight {
            for i in 0..iBlockWidth {
                let iBlockPointX = (i << 4) as i32;
                let iBlockPointY = (j << 4) as i32;

                let pTmpCur = PlaneCursor::new(planes.cur, j * 16 * iStrideY + i * 16, iStrideY);
                let pTmpRef = PlaneCursor::new(planes.refp, j * 16 * iStrideX + i * 16, iStrideX);
                let mut iInterSad = sample_sad::<16, 16, _>(&pTmpCur, &pTmpRef);
                if bScrollFlag
                    && (iInterSad != 0)
                    && (iBlockPointX + iScrollMvX >= 0)
                    && (iBlockPointX + iScrollMvX <= iWidth - 8)
                    && (iBlockPointY + iScrollMvY >= 0)
                    && (iBlockPointY + iScrollMvY <= iHeight - 8)
                {
                    // Signed throughout: the vector may be negative, and folding it
                    // into a `usize` a component at a time would wrap.
                    let iRefScrollOff = (j * 16 * iStrideX + i * 16) as isize
                        - iScrollMvY as isize * iStrideX as isize
                        + iScrollMvX as isize;
                    let pTmpRefScroll = PlaneCursor::new(
                        planes.refp,
                        usize::try_from(iRefScrollOff)
                            .expect("the scroll offset addresses the reference plane"),
                        iStrideX,
                    );
                    let iScrollSad = sample_sad::<16, 16, _>(&pTmpCur, &pTmpRefScroll);

                    if iScrollSad < iInterSad {
                        iInterSad = iScrollSad;
                    }
                }

                let (mut iSadPredV, mut iSadPredH) = (i32::MAX, i32::MAX);

                if j > 0 {
                    let top: &[u8; 16] = pTmpCur.row(-1, 0, 16).try_into().expect("16 samples");
                    i16x16_luma_pred_v(&mut iMemPredMb, top);
                    let pred = PlaneCursor::new(&iMemPredMb, 0, 16);
                    iSadPredV = sample_sad::<16, 16, _>(&pTmpCur, &pred);
                }
                if i > 0 {
                    i16x16_luma_pred_h(&mut iMemPredMb, &pTmpCur);
                    let pred = PlaneCursor::new(&iMemPredMb, 0, 16);
                    iSadPredH = sample_sad::<16, 16, _>(&pTmpCur, &pred);
                }

                iGomSad += WELS_MIN(WELS_MIN(iSadPredV, iSadPredH), iInterSad);

                if i == iBlockWidth - 1
                    && ((j + 1) % self.m_ComplexityAnalysisParam.iMbRowInGom as usize == 0
                        || j == iBlockHeight - 1)
                {
                    pGomComplexity[iIdx] = iGomSad;
                    self.m_ComplexityAnalysisParam.iFrameComplexity += iGomSad as i64;
                    iIdx += 1;
                    iGomSad = 0;
                }
            }
        }
        self.m_ComplexityAnalysisParam.iGomNumInFrame = iIdx as i32;
    }
}

#[cfg(test)]
mod screen_tests {
    use super::*;

    fn pixmap(w: i32, h: i32) -> SPixMap {
        let mut m = SPixMap::default();
        m.iStride[0] = w;
        m.sRect.iRectWidth = w;
        m.sRect.iRectHeight = h;
        m
    }

    fn planes<'a>(cur: &'a [u8], refp: &'a [u8], w: usize) -> ScdPlanes<'a> {
        ScdPlanes { cur, cur_stride: w, refp, ref_stride: w }
    }

    fn param(iIdrFlag: i32, iMbRowInGom: i32) -> SComplexityAnalysisScreenParam {
        let mut p = SComplexityAnalysisScreenParam::default();
        p.iIdrFlag = iIdrFlag;
        p.iMbRowInGom = iMbRowInGom;
        p
    }

    /// A flat frame predicts perfectly in both directions, so every macroblock past
    /// the first costs nothing and the frame total is zero. What the test is really
    /// pinning is the **bucket count**: 320x192 is 20x12 macroblocks, `GOM_H_SCC` is
    /// 8, and the boundary rule `(j + 1) % 8 == 0 || j == bh - 1` fires at `j = 7`
    /// and `j = 11` — two buckets, not `ceil(12 / 8)` rounded some other way.
    #[test]
    fn a_flat_frame_costs_nothing_and_fills_two_gom_buckets() {
        const W: usize = 320;
        const H: usize = 192;
        let flat = vec![120u8; W * H];
        let mut gom = vec![-1i32; 64];
        let mut c = CComplexityAnalysisScreen::default();
        c.Set(&param(1, 8));
        assert_eq!(
            c.Process(&pixmap(W as i32, H as i32), None, &planes(&flat, &[], W), &mut gom),
            RET_SUCCESS
        );
        let mut out = SComplexityAnalysisScreenParam::default();
        c.Get(&mut out);
        assert_eq!(out.iGomNumInFrame, 2, "12 macroblock rows in GOMs of 8");
        assert_eq!(out.iFrameComplexity, 0);
        assert_eq!(&gom[..2], &[0, 0]);
        assert_eq!(gom[2], -1, "nothing was written past the buckets used");
    }

    /// A vertical gradient — row `y` holds the value `y` — computed by hand.
    ///
    /// 32x32 is 2x2 macroblocks and one bucket (`j == bh - 1` at `j = 1`). Of the
    /// four macroblocks, three cost nothing: `(0,0)` is skipped by the `if (i || j)`
    /// guard, and `(1,0)` and `(1,1)` have a *horizontal* prediction that is exact,
    /// because every row is constant. Only `(0,1)` has just the vertical prediction,
    /// which copies row 15 down over rows 16..31:
    ///
    ///     16 columns * sum(y - 15 for y in 16..=31) = 16 * (1 + 2 + ... + 16)
    ///                                              = 16 * 136 = 2176
    ///
    /// So the frame total is 2176 and the single bucket holds it. A port that lost
    /// the `i > 0` / `j > 0` guards, or the `if (i || j)` one, moves this number.
    #[test]
    fn a_vertical_gradient_costs_only_its_first_column_of_macroblocks() {
        const W: usize = 32;
        const H: usize = 32;
        let grad: Vec<u8> = (0..W * H).map(|k| (k / W) as u8).collect();
        let mut gom = vec![-1i32; 8];
        let mut c = CComplexityAnalysisScreen::default();
        c.Set(&param(1, 8));
        assert_eq!(
            c.Process(&pixmap(W as i32, H as i32), None, &planes(&grad, &[], W), &mut gom),
            RET_SUCCESS
        );
        let mut out = SComplexityAnalysisScreenParam::default();
        c.Get(&mut out);
        assert_eq!(out.iGomNumInFrame, 1);
        assert_eq!(gom[0], 2176);
        assert_eq!(out.iFrameComplexity, 2176);
    }

    /// A P frame whose reference *is* the current frame: the collocated inter SAD is
    /// zero for every macroblock, so the three-way minimum is zero everywhere —
    /// including at `(0,0)`, where the two intra costs are `i32::MAX` and only the
    /// absence of an `if (i || j)` guard in the inter kernel gives the right answer.
    #[test]
    fn an_inter_frame_against_itself_costs_nothing() {
        const W: usize = 320;
        const H: usize = 192;
        let mut state = 12345u32;
        let f: Vec<u8> = (0..W * H)
            .map(|_| {
                state = state.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff;
                (state >> 16) as u8
            })
            .collect();
        let mut gom = vec![-1i32; 64];
        let mut c = CComplexityAnalysisScreen::default();
        c.Set(&param(0, 8));
        let map = pixmap(W as i32, H as i32);
        assert_eq!(
            c.Process(&map, Some(&map), &planes(&f, &f, W), &mut gom),
            RET_SUCCESS
        );
        let mut out = SComplexityAnalysisScreenParam::default();
        c.Get(&mut out);
        assert_eq!(out.iGomNumInFrame, 2);
        assert_eq!(out.iFrameComplexity, 0);
        assert_eq!(&gom[..2], &[0, 0]);
    }

    /// `Process`'s two refusals — `ComplexityAnalysis.cpp:322-326`.
    #[test]
    fn process_refuses_a_zero_gom_height_or_a_missing_reference() {
        const W: usize = 64;
        const H: usize = 32;
        let f = vec![7u8; W * H];
        let mut gom = vec![0i32; 8];
        let map = pixmap(W as i32, H as i32);

        let mut c = CComplexityAnalysisScreen::default();
        c.Set(&param(1, 0));
        assert_eq!(
            c.Process(&map, None, &planes(&f, &[], W), &mut gom),
            RET_INVALIDPARAM,
            "iMbRowInGom <= 0"
        );
        c.Set(&param(1, -1));
        assert_eq!(c.Process(&map, None, &planes(&f, &[], W), &mut gom), RET_INVALIDPARAM);

        // A P frame with no reference: the C++'s `!iIdrFlag && pRef == NULL`.
        c.Set(&param(0, 8));
        assert_eq!(
            c.Process(&map, None, &planes(&f, &[], W), &mut gom),
            RET_INVALIDPARAM,
            "a P frame needs a reference"
        );
    }

    /// `Get` copies the **whole** block back, so `iGomNumInFrame` returns this
    /// plugin's bucket count and overwrites whatever the caller staged there
    /// (D-scc-10). `AnalyzePictureComplexity` stages `pWelsSvcRc->iGomSize`; the
    /// number that survives is the plugin's.
    #[test]
    fn get_overwrites_the_staged_gom_count() {
        const W: usize = 320;
        const H: usize = 192;
        let flat = vec![9u8; W * H];
        let mut gom = vec![0i32; 64];
        let mut c = CComplexityAnalysisScreen::default();
        let mut p = param(1, 8);
        p.iGomNumInFrame = 999; // the caller's `iGomSize`
        c.Set(&p);
        assert_eq!(
            c.Process(&pixmap(W as i32, H as i32), None, &planes(&flat, &[], W), &mut gom),
            RET_SUCCESS
        );
        c.Get(&mut p);
        assert_eq!(p.iGomNumInFrame, 2, "the plugin's count, not the staged 999");
    }
}
