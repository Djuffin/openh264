#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/backgrounddetection/BackgroundDetection.cpp` —
//! the plugin reached through `METHOD_BACKGROUND_DETECTION`.
//!
//! `CWelsPreProcess::AnalyzeSpatialPic` calls it for every P slice when
//! `bEnableBackgroundDetection` is set, which `FillDefault` leaves **on**. It fills
//! `pVaa->pVaaBackgroundMbFlag`, one byte per macroblock, which
//! `WelsMdInterJudgeBGDPskip` uses to force P_SKIP on static background and which
//! `CComplexityAnalysis`'s background-excluding SAD kernels read.
//!
//! The work happens on 16x16 "OU"s (`BGD_OU_SIZE`), which at
//! `LOG2_BGD_OU_SIZE == 4` is exactly one macroblock — `OU_SIZE_IN_MB` is 1 — so
//! the OU grid and the macroblock grid coincide for the sizes this encoder builds.
//! The code is transcribed as written rather than specialised to that.

#![deny(unsafe_code)]

use crate::encoder::wels_preprocess::{SBGDInterface, SPixMap, SVAACalcResult};

use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};

/// `BackgroundDetection.cpp:36-47`.
const LOG2_BGD_OU_SIZE: i32 = 4;
const LOG2_BGD_OU_SIZE_UV: i32 = LOG2_BGD_OU_SIZE - 1;
const BGD_OU_SIZE: i32 = 1 << LOG2_BGD_OU_SIZE;
const BGD_OU_SIZE_UV: i32 = BGD_OU_SIZE >> 1;
const BGD_THD_SAD: i32 = 2 * BGD_OU_SIZE * BGD_OU_SIZE;
const BGD_THD_ASD_UV: i32 = 4 * BGD_OU_SIZE_UV;
const LOG2_MB_SIZE: i32 = 4;
const OU_SIZE_IN_MB: i32 = BGD_OU_SIZE >> 4;
const Q_FACTOR: i32 = 8;

const OU_LEFT: i8 = 0x01;
const OU_RIGHT: i8 = 0x02;
const OU_TOP: i8 = 0x04;
const OU_BOTTOM: i8 = 0x08;

/// `SBackgroundOU` — `BackgroundDetection.h:51`.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SBackgroundOU {
    pub iBackgroundFlag: i32,
    pub iSAD: i32,
    pub iSD: i32,
    pub iMAD: i32,
    pub iMinSubMad: i32,
    pub iMaxDiffSubSd: i32,
}

/// `CBackgroundDetection::vBGDParam` — `BackgroundDetection.h:69`.
///
/// `pOU_array` is a `Vec` here rather than a `WelsMalloc` block; nothing outside
/// the plugin sees it, and the C++ reallocates it on the same growth rule.
#[derive(Default)]
struct vBGDParam {
    // **S10.12: `pCur`, `pRef` and `pBackgroundMbFlag` are gone from here.** They
    // were the two pixmaps' plane roots and the caller's per-macroblock flag array,
    // copied in at each `Process` and read back out two calls deeper — working
    // state, stored. Storing them made `CBackgroundDetection`, and through it
    // `CWelsPreProcess` and `sWelsEncCtx`, **`!Sync`**, which was the last thing
    // between `SliceJobHandle` and carrying a reference across the encode fork.
    //
    // They are parameters now, which is the move T9.X made for the two GOM arrays
    // and Phase 6 session B for `pCalcResult`. The geometry below stays: it is this
    // plugin's own derived state, not a borrow of the caller's.
    iBgdWidth: i32,
    iBgdHeight: i32,
    iStride: [i32; 3],
    pOU_array: Vec<SBackgroundOU>,
}

/// `CBackgroundDetection` — `BackgroundDetection.h:60`.
pub struct CBackgroundDetection {
    m_BgdParam: vBGDParam,
    m_iLargestFrameSize: i32,
}

impl Default for CBackgroundDetection {
    fn default() -> Self {
        Self {
            m_BgdParam: vBGDParam {
                iBgdWidth: 0,
                iBgdHeight: 0,
                iStride: [0; 3],
                pOU_array: Vec::new(),
            },
            m_iLargestFrameSize: 0,
        }
    }
}

#[inline]
fn WELS_MAX(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}

#[inline]
fn WELS_MIN(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}

impl CBackgroundDetection {
    /// `CBackgroundDetection::Set`. Typed since Phase 6 session B (the `IWelsVP`
    /// vtable's `void*` is gone). The C++ class has no `Get` override
    /// (`IStrategy::Get` returned success without writing), and nothing called it.
    /// **S10.12: nothing left to store.** It stashed `param.pBackgroundMbFlag`,
    /// which reaches `Process` as a slice now. Kept as the port's counterpart of
    /// `CBackgroundDetection::Set` — the C++ method exists and the call site still
    /// makes the call — but it has no state to take.
    pub fn Set(&mut self, _param: &SBGDInterface) -> i32 {
        RET_SUCCESS
    }

    /// `CBackgroundDetection::Process` — `BackgroundDetection.cpp:63`. `calc` is the
    /// VAA statistics of this picture pair, handed over at the call (the C++ stored
    /// `pCalcRes` in the parameter block; take what you reach).
    ///
    /// # Safety
    /// Both pixel maps must describe readable Y/U/V planes, the pointer stored by
    /// the preceding [`Set`](Self::Set) must still be valid, and `calc`'s arrays must
    /// cover the picture's macroblocks.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn Process(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        calc: &SVAACalcResult,
        // S10.12: the caller's per-macroblock flag array, as a slice.
        pVaaBackgroundMbFlag: &mut [i8],
    ) -> i32 {
        for i in 0..3 {
            self.m_BgdParam.iStride[i] = pSrcPixMap.iStride[i];
        }
        self.m_BgdParam.iBgdWidth = pSrcPixMap.sRect.iRectWidth;
        self.m_BgdParam.iBgdHeight = pSrcPixMap.sRect.iRectHeight;

        let iCurFrameSize = self.m_BgdParam.iBgdWidth * self.m_BgdParam.iBgdHeight;
        if self.m_BgdParam.pOU_array.is_empty() || iCurFrameSize > self.m_iLargestFrameSize {
            let iMaxOUWidth = (BGD_OU_SIZE - 1 + self.m_BgdParam.iBgdWidth) >> LOG2_BGD_OU_SIZE;
            let iMaxOUHeight = (BGD_OU_SIZE - 1 + self.m_BgdParam.iBgdHeight) >> LOG2_BGD_OU_SIZE;
            self.m_BgdParam.pOU_array =
                vec![SBackgroundOU::default(); (iMaxOUWidth * iMaxOUHeight) as usize];
            self.m_iLargestFrameSize = iCurFrameSize;
        }
        if self.m_BgdParam.pOU_array.is_empty() {
            return RET_INVALIDPARAM;
        }

        // 1st step: foreground/background coarse division
        self.ForegroundBackgroundDivision(calc);
        // 2nd step: foreground dilation and background erosion
        self.ForegroundDilationAndBackgroundErosion(pSrcPixMap, pRefPixMap, pVaaBackgroundMbFlag);
        RET_SUCCESS
    }

    /// `CBackgroundDetection::GetOUParameters` — `BackgroundDetection.cpp:114`.
    fn GetOUParameters(sVaaCalcInfo: &SVAACalcResult, iMbIndex: i32, pBgdOU: &mut SBackgroundOU) {
        let idx = iMbIndex as usize;
        let iSubSAD = sVaaCalcInfo.pSad8x8[idx];
        let iSubSD = sVaaCalcInfo.pSumOfDiff8x8[idx];
        let iSubMAD = sVaaCalcInfo.pMad8x8[idx];

        pBgdOU.iSD = iSubSD[0] + iSubSD[1] + iSubSD[2] + iSubSD[3];
        pBgdOU.iSAD = iSubSAD[0] + iSubSAD[1] + iSubSAD[2] + iSubSAD[3];
        pBgdOU.iSD = pBgdOU.iSD.abs();

        // `iSubMAD` is `uint8_t[4]` in C++ and every use widens to `int`.
        let m: [i32; 4] = [
            iSubMAD[0] as i32,
            iSubMAD[1] as i32,
            iSubMAD[2] as i32,
            iSubMAD[3] as i32,
        ];
        pBgdOU.iMAD = WELS_MAX(WELS_MAX(m[0], m[1]), WELS_MAX(m[2], m[3]));
        pBgdOU.iMinSubMad = WELS_MIN(WELS_MIN(m[0], m[1]), WELS_MIN(m[2], m[3]));

        pBgdOU.iMaxDiffSubSd = WELS_MAX(WELS_MAX(iSubSD[0], iSubSD[1]), WELS_MAX(iSubSD[2], iSubSD[3]))
            - WELS_MIN(WELS_MIN(iSubSD[0], iSubSD[1]), WELS_MIN(iSubSD[2], iSubSD[3]));
    }

    /// `CBackgroundDetection::ForegroundBackgroundDivision` — `BackgroundDetection.cpp:157`.
    fn ForegroundBackgroundDivision(&mut self, calc: &SVAACalcResult) {
        let iPicWidthInOU = self.m_BgdParam.iBgdWidth >> LOG2_BGD_OU_SIZE;
        let iPicHeightInOU = self.m_BgdParam.iBgdHeight >> LOG2_BGD_OU_SIZE;
        let iPicWidthInMb = (15 + self.m_BgdParam.iBgdWidth) >> 4;

        let mut ou = 0usize;
        for j in 0..iPicHeightInOU {
            for i in 0..iPicWidthInOU {
                let pBackgroundOU = &mut self.m_BgdParam.pOU_array[ou];
                Self::GetOUParameters(
                    calc,
                    (j * iPicWidthInMb + i) << (LOG2_BGD_OU_SIZE - LOG2_MB_SIZE),
                    pBackgroundOU,
                );

                pBackgroundOU.iBackgroundFlag = 0;
                if pBackgroundOU.iMAD > 63 {
                    ou += 1;
                    continue;
                }
                if (pBackgroundOU.iMaxDiffSubSd <= pBackgroundOU.iSAD >> 3
                    || pBackgroundOU.iMaxDiffSubSd <= BGD_OU_SIZE * Q_FACTOR)
                    && pBackgroundOU.iSAD < (BGD_THD_SAD << 1)
                {
                    if pBackgroundOU.iSAD <= BGD_OU_SIZE * Q_FACTOR {
                        pBackgroundOU.iBackgroundFlag = 1;
                    } else {
                        pBackgroundOU.iBackgroundFlag = if pBackgroundOU.iSAD < BGD_THD_SAD {
                            (pBackgroundOU.iSD < (pBackgroundOU.iSAD * 3) >> 2) as i32
                        } else {
                            (pBackgroundOU.iSD << 1 < pBackgroundOU.iSAD) as i32
                        };
                    }
                }
                ou += 1;
            }
        }
    }

    /// `CBackgroundDetection::CalculateAsdChromaEdge` — `BackgroundDetection.cpp:189`.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn CalculateAsdChromaEdge(pOriRef: *const u8, pOriCur: *const u8, iStride: i32) -> i32 {
        let mut ASD: i32 = 0;
        let mut pRef = pOriRef;
        let mut pCur = pOriCur;
        for _idx in 0..BGD_OU_SIZE_UV {
            ASD += *pCur as i32 - *pRef as i32;
            pRef = pRef.offset(iStride as isize);
            pCur = pCur.offset(iStride as isize);
        }
        ASD.abs()
    }

    /// `CBackgroundDetection::ForegroundDilation23Luma` — `BackgroundDetection.cpp:200`.
    fn ForegroundDilation23Luma(pBackgroundOU: &SBackgroundOU, nb: &[SBackgroundOU; 4]) -> bool {
        let (pOU_L, pOU_R, pOU_U, pOU_D) = (&nb[0], &nb[1], &nb[2], &nb[3]);

        if pBackgroundOU.iMAD > pBackgroundOU.iMinSubMad << 1 {
            // `(flag - 1) & mad` is a branchless select: -1 (all ones) when the
            // flag is 0, 0 when it is 1. Kept verbatim.
            let aForegroundMad = [
                (pOU_L.iBackgroundFlag - 1) & pOU_L.iMAD,
                (pOU_R.iBackgroundFlag - 1) & pOU_R.iMAD,
                (pOU_U.iBackgroundFlag - 1) & pOU_U.iMAD,
                (pOU_D.iBackgroundFlag - 1) & pOU_D.iMAD,
            ];
            let iMaxNbrForegroundMad = WELS_MAX(
                WELS_MAX(aForegroundMad[0], aForegroundMad[1]),
                WELS_MAX(aForegroundMad[2], aForegroundMad[3]),
            );

            let not = |f: i32| (f == 0) as i32;
            let aBackgroundMad = [
                (not(pOU_L.iBackgroundFlag) - 1) & pOU_L.iMAD,
                (not(pOU_R.iBackgroundFlag) - 1) & pOU_R.iMAD,
                (not(pOU_U.iBackgroundFlag) - 1) & pOU_U.iMAD,
                (not(pOU_D.iBackgroundFlag) - 1) & pOU_D.iMAD,
            ];
            let iMaxNbrBackgroundMad = WELS_MAX(
                WELS_MAX(aBackgroundMad[0], aBackgroundMad[1]),
                WELS_MAX(aBackgroundMad[2], aBackgroundMad[3]),
            );

            return (iMaxNbrForegroundMad > pBackgroundOU.iMinSubMad << 2)
                || (pBackgroundOU.iMAD > iMaxNbrBackgroundMad << 1
                    && pBackgroundOU.iMAD <= (iMaxNbrForegroundMad * 3) >> 1);
        }
        false
    }

    /// `CBackgroundDetection::ForegroundDilation23Chroma` — `BackgroundDetection.cpp:232`.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn ForegroundDilation23Chroma(
        &self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        iNeighbourForegroundFlags: i8,
        iStartSamplePos: i32,
        iPicStrideUV: i32,
    ) -> bool {
        const kaOUPos: [i8; 4] = [OU_LEFT, OU_RIGHT, OU_TOP, OU_BOTTOM];
        let aEdgeOffset: [i32; 4] = [
            0,
            BGD_OU_SIZE_UV - 1,
            0,
            iPicStrideUV * (BGD_OU_SIZE_UV - 1),
        ];
        let iStride: [i32; 4] = [iPicStrideUV, iPicStrideUV, 1, 1];

        // V first (red; human skin weighs more on it), then U.
        for plane in [2usize, 1usize] {
            for i in 0..4 {
                if iNeighbourForegroundFlags & kaOUPos[i] != 0 {
                    let off = (iStartSamplePos + aEdgeOffset[i]) as isize;
                    // S10.12: the pixmaps are the caller's, handed in rather than
                    // copied onto this struct at `Process`. Same two addresses.
                    let pRefC = pRefPixMap.pPixel[plane].offset(off);
                    let pCurC = pSrcPixMap.pPixel[plane].offset(off);
                    if Self::CalculateAsdChromaEdge(pRefC, pCurC, iStride[i]) > BGD_THD_ASD_UV {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// `CBackgroundDetection::ForegroundDilation` — `BackgroundDetection.cpp:263`.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn ForegroundDilation(
        &self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        pBackgroundOU: &mut SBackgroundOU,
        nb: &[SBackgroundOU; 4],
        iChromaSampleStartPos: i32,
    ) {
        let iPicStrideUV = self.m_BgdParam.iStride[1];
        let iSumNeighBackgroundFlags = nb[0].iBackgroundFlag
            + nb[1].iBackgroundFlag
            + nb[2].iBackgroundFlag
            + nb[3].iBackgroundFlag;

        if pBackgroundOU.iSAD > BGD_OU_SIZE * Q_FACTOR {
            match iSumNeighBackgroundFlags {
                0 | 1 => {
                    pBackgroundOU.iBackgroundFlag = 0;
                }
                2 | 3 => {
                    pBackgroundOU.iBackgroundFlag =
                        !Self::ForegroundDilation23Luma(pBackgroundOU, nb) as i32;

                    // chroma component check
                    if pBackgroundOU.iBackgroundFlag == 1 {
                        let n = |f: i32| (f == 0) as i8;
                        let iNeighbourForegroundFlags = n(nb[0].iBackgroundFlag)
                            | (n(nb[1].iBackgroundFlag) << 1)
                            | (n(nb[2].iBackgroundFlag) << 2)
                            | (n(nb[3].iBackgroundFlag) << 3);
                        pBackgroundOU.iBackgroundFlag = !self.ForegroundDilation23Chroma(
                            pSrcPixMap,
                            pRefPixMap,
                            iNeighbourForegroundFlags,
                            iChromaSampleStartPos,
                            iPicStrideUV,
                        ) as i32;
                    }
                }
                _ => {}
            }
        }
    }

    /// `CBackgroundDetection::BackgroundErosion` — `BackgroundDetection.cpp:292`.
    fn BackgroundErosion(pBackgroundOU: &mut SBackgroundOU, nb: &[SBackgroundOU; 4]) {
        if pBackgroundOU.iMaxDiffSubSd <= BGD_OU_SIZE * Q_FACTOR {
            let iSumNeighBackgroundFlags = nb[0].iBackgroundFlag
                + nb[1].iBackgroundFlag
                + nb[2].iBackgroundFlag
                + nb[3].iBackgroundFlag;
            let sumNbrBGsad = (nb[0].iSAD & (-nb[0].iBackgroundFlag))
                + (nb[2].iSAD & (-nb[2].iBackgroundFlag))
                + (nb[1].iSAD & (-nb[1].iBackgroundFlag))
                + (nb[3].iSAD & (-nb[3].iBackgroundFlag));
            if pBackgroundOU.iSAD * iSumNeighBackgroundFlags <= (3 * sumNbrBGsad) >> 1 {
                if iSumNeighBackgroundFlags == 4 {
                    pBackgroundOU.iBackgroundFlag = 1;
                } else if (nb[0].iBackgroundFlag & nb[1].iBackgroundFlag) != 0
                    || (nb[2].iBackgroundFlag & nb[3].iBackgroundFlag) != 0
                {
                    pBackgroundOU.iBackgroundFlag =
                        !Self::ForegroundDilation23Luma(pBackgroundOU, nb) as i32;
                }
            }
        }
    }

    /// `CBackgroundDetection::ForegroundDilationAndBackgroundErosion` —
    /// `BackgroundDetection.cpp:329`.
    ///
    /// The C++ carries four raw neighbour pointers into the same array it is
    /// mutating. Rust's aliasing rules make that awkward, so each iteration copies
    /// the four neighbours by value before touching the current OU — every callee
    /// reads them and none writes them, so the values are the same.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn ForegroundDilationAndBackgroundErosion(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        pVaaBackgroundMbFlag: &mut [i8],
    ) {
        let iPicStrideUV = self.m_BgdParam.iStride[1];
        let iPicWidthInOU = self.m_BgdParam.iBgdWidth >> LOG2_BGD_OU_SIZE;
        let iPicHeightInOU = self.m_BgdParam.iBgdHeight >> LOG2_BGD_OU_SIZE;
        let iOUStrideUV = iPicStrideUV << (LOG2_BGD_OU_SIZE - 1);
        let iPicWidthInMb = (15 + self.m_BgdParam.iBgdWidth) >> 4;

        // **S10.12: an index cursor, not a roving pointer.** The walk moved a
        // `*mut i8` forward one OU-row at a time and reached *backwards* one row to
        // clear the OU above (`offset(-(OU_SIZE_IN_MB * iPicWidthInMb))`). Both are
        // integer arithmetic on the same array; as indices they are the same
        // addresses and every one of them is bounds-checked.
        let kiRowStep = (OU_SIZE_IN_MB * iPicWidthInMb) as isize;
        let mut iRowBase: isize = 0;

        // Indices into pOU_array, mirroring the C++'s pointers.
        let mut cur = 0usize; // pBackgroundOU
        let mut nb_top = 0isize; // pOUNeighbours[2]

        for j in 0..iPicHeightInOU {
            let mut iSkipFlag = iRowBase;
            let mut nb_left = cur as isize;
            // `pBackgroundOU + (iPicWidthInOU & ((j == iPicHeightInOU-1) - 1))`:
            // the mask is 0 on the last row and -1 (all ones) otherwise.
            let last_row = j == iPicHeightInOU - 1;
            let mut nb_bottom = cur as isize + (iPicWidthInOU & ((last_row as i32) - 1)) as isize;

            for i in 0..iPicWidthInOU {
                let nb_right = cur as isize + (i < iPicWidthInOU - 1) as isize;

                let nb = [
                    self.m_BgdParam.pOU_array[nb_left as usize],
                    self.m_BgdParam.pOU_array[nb_right as usize],
                    self.m_BgdParam.pOU_array[nb_top as usize],
                    self.m_BgdParam.pOU_array[nb_bottom as usize],
                ];

                let mut ou = self.m_BgdParam.pOU_array[cur];
                if ou.iBackgroundFlag != 0 {
                    self.ForegroundDilation(
                        pSrcPixMap,
                        pRefPixMap,
                        &mut ou,
                        &nb,
                        j * iOUStrideUV + (i << LOG2_BGD_OU_SIZE_UV),
                    );
                } else {
                    Self::BackgroundErosion(&mut ou, &nb);
                }
                self.m_BgdParam.pOU_array[cur] = ou;

                // check the up OU
                if j > 1
                    && i > 0
                    && i < iPicWidthInOU - 1
                    && self.m_BgdParam.pOU_array[nb_top as usize].iBackgroundFlag == 1
                {
                    let up = self.m_BgdParam.pOU_array[nb_top as usize];
                    if up.iSAD > BGD_OU_SIZE * Q_FACTOR {
                        let l = self.m_BgdParam.pOU_array[(nb_top - 1) as usize].iBackgroundFlag;
                        let r = self.m_BgdParam.pOU_array[(nb_top + 1) as usize].iBackgroundFlag;
                        let u = self.m_BgdParam.pOU_array
                            [(nb_top - iPicWidthInOU as isize) as usize]
                            .iBackgroundFlag;
                        let d = self.m_BgdParam.pOU_array
                            [(nb_top + iPicWidthInOU as isize) as usize]
                            .iBackgroundFlag;
                        if l + r + u + d <= 1 {
                            pVaaBackgroundMbFlag[(iSkipFlag - kiRowStep) as usize] = 0;
                            self.m_BgdParam.pOU_array[nb_top as usize].iBackgroundFlag = 0;
                        }
                    }
                }

                pVaaBackgroundMbFlag[iSkipFlag as usize] =
                    self.m_BgdParam.pOU_array[cur].iBackgroundFlag as i8;

                // preparation for the next OU
                iSkipFlag += OU_SIZE_IN_MB as isize;
                nb_left = cur as isize;
                nb_top += 1;
                nb_bottom += 1;
                cur += 1;
            }
            nb_top = cur as isize - iPicWidthInOU as isize;
            iRowBase += kiRowStep;
        }
    }
}
