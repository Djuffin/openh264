//! Parameter-set id strategies — `codec/encoder/core/src/paraset_strategy.cpp`,
//! `codec/encoder/core/inc/paraset_strategy.h`.
//!
//! Five strategies: `CONSTANT_ID`, `INCREASING_ID`, `SPS_LISTING`,
//! `SPS_LISTING_AND_PPS_INCREASING` and `SPS_PPS_LISTING`. All carry the same data
//! members, so they are one [`CWelsParametersetIdStrategyObj`] with a
//! [`ParasetIdKind`] discriminant, and only the methods that differ `match` on it.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

use crate::api::codec_api::EParameterSetStrategy;
use crate::api::codec_api::RC_MODES::RC_OFF_MODE;
use crate::common::wels_trace::{WELS_LOG_WARNING, WelsLog};
use crate::encoder::au_set::{WelsInitPps, WelsInitSps, WelsInitSubsetSps};
use crate::encoder::encoder_context::SWelsEncoderOutput;
use crate::encoder::encoder_context::{
    MAX_DQ_LAYER_NUM, MAX_PPS_COUNT, PARA_SET_TYPE, SLogContext, SParaSetOffset,
    SParaSetOffsetVariable, sWelsEncCtx,
};
use crate::encoder::param_svc::{
    MAX_SPS_COUNT, SExistingParasetList, SSubsetSps, SWelsPPS, SWelsSPS, SWelsSvcCodingParam,
};

/// `PARA_SET_TYPE_AVCSPS` / `_SUBSETSPS` / `_PPS` — `wels_const.h`.
pub const PARA_SET_TYPE_AVCSPS: usize = 0;
pub const PARA_SET_TYPE_SUBSETSPS: usize = 1;
pub const PARA_SET_TYPE_PPS: usize = 2;

/// `INVALID_ID` — `wels_const.h`; returned by `FindExistingSps` when no stored
/// parameter set matches the current configuration.
pub const INVALID_ID: i32 = -1;

/// Which parameter-set id strategy an object implements.
///
/// `Constant = 0` matters: [`SWelsFuncPtrList`](crate::encoder::wels_func_ptr_def::SWelsFuncPtrList)
/// is built by `WelsMallocz`, so the all-zero pattern must be a declared variant.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParasetIdKind {
    /// `CWelsParametersetIdConstant` — `paraset_strategy.h:96`. Every id offset is 0.
    Constant = 0,
    /// `CWelsParametersetIdIncreasing` — `paraset_strategy.h:208`. Rotates the id
    /// written to the bitstream, recording the delta to the encoder-side id.
    Increasing = 1,
    /// `CWelsParametersetSpsListing` — `paraset_strategy.h:231`. Keeps a list of SPSs
    /// and reuses one whenever the current configuration matches it, so a mid-stream
    /// re-initialisation can go back to an SPS the decoder already has.
    SpsListing = 2,
    /// `CWelsParametersetSpsListingPpsIncreasing` — `paraset_strategy.h:294`.
    /// `SpsListing` with `Increasing`'s two id hooks; nothing else differs.
    SpsListingPpsIncreasing = 3,
    /// `CWelsParametersetSpsPpsListing` — `paraset_strategy.h:270`. Lists PPSs as well,
    /// pre-expanding the array to `MAX_PPS_COUNT` entries and rotating through them by
    /// IDR round.
    SpsPpsListing = 4,
}

impl ParasetIdKind {
    /// The three listing kinds — a bitmask test on `eSpsPpsIdStrategy`
    /// (`codec_app_def.h:514-518`: 0x02, 0x03 and 0x06 all carry 0x02).
    #[inline]
    pub fn is_listing(self) -> bool {
        matches!(
            self,
            Self::SpsListing | Self::SpsListingPpsIncreasing | Self::SpsPpsListing
        )
    }

    /// The two kinds whose `GetPpsIdOffset` / `Update` rotate ids.
    #[inline]
    pub fn rotates_ids(self) -> bool {
        matches!(self, Self::Increasing | Self::SpsListingPpsIncreasing)
    }

    /// Every kind but `Constant`, the one that leaves `OutputCurrentStructure` and
    /// `LoadPreviousStructure` empty.
    #[inline]
    pub fn is_non_constant(self) -> bool {
        !matches!(self, Self::Constant)
    }
}

/// The parameter-set id strategy object — `paraset_strategy.h:96`, `:180`, `:208`,
/// whose data members are identical. `eIdKind` says which strategy this object is.
#[repr(C)]
pub struct CWelsParametersetIdStrategyObj {
    pub eIdKind: ParasetIdKind,
    pub m_sParaSetOffset: SParaSetOffset,
    pub m_bSimulcastAVC: bool,
    pub m_iSpatialLayerNum: i32,
    pub m_iBasicNeededSpsNum: u32,
    pub m_iBasicNeededPpsNum: u32,
}

/// `CWelsParametersetIdConstant::CWelsParametersetIdConstant` —
/// `paraset_strategy.cpp:203`. The kinds differ only in the two "basic needed"
/// counts.
impl CWelsParametersetIdStrategyObj {
    pub fn new(eIdKind: ParasetIdKind, bSimulcastAVC: bool, kiSpatialLayerNum: i32) -> Box<Self> {
        // `paraset_strategy.cpp:410-411` and `:545-546`.
        let (m_iBasicNeededSpsNum, m_iBasicNeededPpsNum) = match eIdKind {
            ParasetIdKind::Constant | ParasetIdKind::Increasing => {
                (1, (1 + kiSpatialLayerNum) as u32)
            }
            ParasetIdKind::SpsListing | ParasetIdKind::SpsListingPpsIncreasing => {
                (MAX_SPS_COUNT as u32, 1)
            }
            ParasetIdKind::SpsPpsListing => (MAX_SPS_COUNT as u32, MAX_PPS_COUNT as u32),
        };
        Box::new(Self {
            eIdKind,
            m_sParaSetOffset: SParaSetOffset::default(),
            m_bSimulcastAVC: bSimulcastAVC,
            m_iSpatialLayerNum: kiSpatialLayerNum,
            m_iBasicNeededSpsNum,
            m_iBasicNeededPpsNum,
        })
    }

    // ------------------------------------------------------------------
    // The five methods whose C++ vtable entries differ between the kinds.
    // ------------------------------------------------------------------

    /// `GetPpsIdOffset` — `paraset_strategy.cpp:216` (Constant) / `:384` (Increasing)
    /// / `:703` (SpsListingPpsIncreasing).
    #[inline]
    pub fn GetPpsIdOffset(&self, kiPpsId: i32) -> i32 {
        if self.eIdKind.rotates_ids() {
            self.m_sParaSetOffset.sParaSetOffsetVariable[PARA_SET_TYPE_PPS].iParaSetIdDelta
                [kiPpsId as usize]
        } else {
            0
        }
    }

    /// `GetSpsIdOffset` — `paraset_strategy.cpp:219` (Constant) / `:391` (Increasing).
    ///
    /// Only `Increasing` rotates the SPS id offset. `SpsListingPpsIncreasing` takes
    /// only `GetPpsIdOffset` and `Update` from it (`paraset_strategy.h:294-301`), so
    /// its SPS id offset is zero.
    #[inline]
    pub fn GetSpsIdOffset(&self, kiPpsId: i32, kiSpsId: i32) -> i32 {
        match self.eIdKind {
            ParasetIdKind::Increasing => {
                let kiParameterSetType =
                    if self.m_sParaSetOffset.bPpsIdMappingIntoSubsetsps[kiPpsId as usize] {
                        PARA_SET_TYPE_SUBSETSPS
                    } else {
                        PARA_SET_TYPE_AVCSPS
                    };
                self.m_sParaSetOffset.sParaSetOffsetVariable[kiParameterSetType].iParaSetIdDelta
                    [kiSpsId as usize]
            }
            ParasetIdKind::Constant
            | ParasetIdKind::SpsListing
            | ParasetIdKind::SpsListingPpsIncreasing
            | ParasetIdKind::SpsPpsListing => 0,
        }
    }

    /// `Update` — `paraset_strategy.cpp:261` (Constant) / `:370` (Increasing) /
    /// `:708` (SpsListingPpsIncreasing).
    ///
    /// The `Constant` arm resets the whole offset block, `uiInUseSpsNum` and
    /// `iPpsIdList` included. `SpsListing` and `SpsPpsListing` share that arm but
    /// never reach it: a listing strategy is routed to `WriteSavcParaset_Listing`,
    /// which calls only `UpdatePpsList`.
    #[inline]
    pub fn Update(&mut self, kuiId: u32, iParasetType: i32) {
        match self.eIdKind {
            ParasetIdKind::Constant | ParasetIdKind::SpsListing | ParasetIdKind::SpsPpsListing => {
                self.m_sParaSetOffset = SParaSetOffset::default();
            }
            ParasetIdKind::Increasing | ParasetIdKind::SpsListingPpsIncreasing => {
                let kuiMaxIdInBs = if iParasetType != PARA_SET_TYPE_PPS as i32 {
                    MAX_SPS_COUNT as u32
                } else {
                    MAX_PPS_COUNT as u32
                };
                ParasetIdAdditionIdAdjust(
                    &mut self.m_sParaSetOffset.sParaSetOffsetVariable[iParasetType as usize],
                    kuiId as i32,
                    kuiMaxIdInBs,
                );
            }
        }
    }

    /// `OutputCurrentStructure` — `paraset_strategy.h:145` (Constant, empty) /
    /// `paraset_strategy.cpp:292` (non-constant) / `:519` (SpsListing) / `:684`
    /// (SpsPpsListing). Only the listing kinds write the array parameters.
    ///
    /// Callers use [`ctx_strategy_and_paraset_arrays`].
    pub fn OutputCurrentStructure(
        &mut self,
        pParaSetOffsetVariable: &mut [SParaSetOffsetVariable; PARA_SET_TYPE],
        pPpsIdList: &mut [i32; MAX_DQ_LAYER_NUM * MAX_PPS_COUNT],
        pSpsArray: &[SWelsSPS],
        pSubsetArray: &[SSubsetSps],
        pPpsArray: &[SWelsPPS],
        pExistingParasetList: Option<&mut SExistingParasetList>,
    ) {
        if !self.eIdKind.is_non_constant() {
            return;
        }
        // `CWelsParametersetIdNonConstant::OutputCurrentStructure` —
        // `paraset_strategy.cpp:292`.
        for k in 0..PARA_SET_TYPE {
            self.m_sParaSetOffset.sParaSetOffsetVariable[k].bUsedParaSetIdInBs =
                [false; MAX_PPS_COUNT];
        }
        *pParaSetOffsetVariable = self.m_sParaSetOffset.sParaSetOffsetVariable;

        let Some(pExistingParasetList) = pExistingParasetList else {
            return;
        };
        if !self.eIdKind.is_listing() {
            return;
        }
        // `CWelsParametersetSpsListing::OutputCurrentStructure` — `:519`.
        pExistingParasetList.uiInUseSpsNum = self.m_sParaSetOffset.uiInUseSpsNum;
        // Both sides are exactly `MAX_SPS_COUNT` long: the listing kinds set
        // `m_iBasicNeededSpsNum = MAX_SPS_COUNT`, and the rest returned above.
        pExistingParasetList.sSps.copy_from_slice(pSpsArray);
        if !pSubsetArray.is_empty() {
            pExistingParasetList.uiInUseSubsetSpsNum = self.m_sParaSetOffset.uiInUseSubsetSpsNum;
            pExistingParasetList
                .sSubsetSps
                .copy_from_slice(pSubsetArray);
        } else {
            pExistingParasetList.uiInUseSubsetSpsNum = 0;
        }

        if self.eIdKind != ParasetIdKind::SpsPpsListing {
            return;
        }
        // `CWelsParametersetSpsPpsListing::OutputCurrentStructure` — `:684`.
        // `pPpsArray` and `sPps` are both exactly `MAX_PPS_COUNT` entries long.
        pExistingParasetList.uiInUsePpsNum = self.m_sParaSetOffset.uiInUsePpsNum;
        pExistingParasetList.sPps.copy_from_slice(pPpsArray);
        for (kiDid, kpRow) in self.m_sParaSetOffset.iPpsIdList.iter().enumerate() {
            pPpsIdList[kiDid * MAX_PPS_COUNT..][..MAX_PPS_COUNT].copy_from_slice(kpRow);
        }
    }

    /// `LoadPreviousStructure` — `paraset_strategy.h:148` (Constant, empty) /
    /// `paraset_strategy.cpp:300` (`CWelsParametersetIdNonConstant`).
    pub fn LoadPreviousStructure(
        &mut self,
        pParaSetOffsetVariable: &[SParaSetOffsetVariable; PARA_SET_TYPE],
        pPpsIdList: &mut [i32; MAX_DQ_LAYER_NUM * MAX_PPS_COUNT],
    ) {
        if !self.eIdKind.is_non_constant() {
            return;
        }
        self.m_sParaSetOffset.sParaSetOffsetVariable = *pParaSetOffsetVariable;
        // `CWelsParametersetSpsPpsListing::LoadPreviousStructure` — `:676`. Only that
        // kind carries the id list back in.
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            for (kiDid, kpRow) in self.m_sParaSetOffset.iPpsIdList.iter_mut().enumerate() {
                kpRow.copy_from_slice(&pPpsIdList[kiDid * MAX_PPS_COUNT..][..MAX_PPS_COUNT]);
            }
        }
    }

    // ------------------------------------------------------------------
    // The methods with one body, shared by every kind.
    // ------------------------------------------------------------------

    /// `GetSpsIdOffsetList` — `paraset_strategy.cpp:223`.
    #[inline]
    /// The delta table as a shared slice; consumers read one entry at `uiSpsId`.
    pub fn GetSpsIdOffsetList(&self, iParasetType: i32) -> &[i32] {
        &self.m_sParaSetOffset.sParaSetOffsetVariable[iParasetType as usize].iParaSetIdDelta
    }

    /// `GetAllNeededParasetNum` — `paraset_strategy.cpp:227`.
    pub fn GetAllNeededParasetNum(&mut self) -> u32 {
        self.GetNeededSpsNum() + self.GetNeededSubsetSpsNum() + self.GetNeededPpsNum()
    }

    /// `GetNeededSpsNum` — `paraset_strategy.cpp:233`.
    pub fn GetNeededSpsNum(&mut self) -> u32 {
        if self.m_sParaSetOffset.uiNeededSpsNum == 0 {
            self.m_sParaSetOffset.uiNeededSpsNum = self.m_iBasicNeededSpsNum
                * if self.m_bSimulcastAVC {
                    self.m_iSpatialLayerNum as u32
                } else {
                    1
                };
        }
        self.m_sParaSetOffset.uiNeededSpsNum
    }

    /// `GetNeededSubsetSpsNum` — `paraset_strategy.cpp:241` (Constant) / `:416`
    /// (SpsListing, which asks for the whole array rather than one per extra layer).
    pub fn GetNeededSubsetSpsNum(&mut self) -> u32 {
        if self.m_sParaSetOffset.uiNeededSubsetSpsNum == 0 {
            self.m_sParaSetOffset.uiNeededSubsetSpsNum = if self.m_bSimulcastAVC {
                0
            } else if self.eIdKind.is_listing() {
                MAX_SPS_COUNT as u32
            } else {
                (self.m_iSpatialLayerNum - 1) as u32
            };
        }
        self.m_sParaSetOffset.uiNeededSubsetSpsNum
    }

    /// `GetNeededPpsNum` — `paraset_strategy.cpp:248`.
    pub fn GetNeededPpsNum(&mut self) -> u32 {
        if self.m_sParaSetOffset.uiNeededPpsNum == 0 {
            self.m_sParaSetOffset.uiNeededPpsNum = self.m_iBasicNeededPpsNum
                * if self.m_bSimulcastAVC {
                    self.m_iSpatialLayerNum as u32
                } else {
                    1
                };
        }
        self.m_sParaSetOffset.uiNeededPpsNum
    }

    /// `LoadPrevious` — `paraset_strategy.cpp:256` (Constant, a no-op) / `:439`
    /// (SpsListing, which calls `LoadPreviousSps` then `LoadPreviousPps`).
    ///
    /// Across a mid-stream `InitializeExt`, `InitDqLayers` hands over the caller's
    /// `SExistingParasetList` and the previous encoder's parameter sets come back
    /// into the new one's arrays, so a configuration the decoder has already seen
    /// keeps its old id.
    pub fn LoadPrevious(
        &mut self,
        pExistingParasetList: Option<&SExistingParasetList>,
        pSpsArray: &mut [SWelsSPS],
        pSubsetArray: &mut [SSubsetSps],
        pPpsArray: &mut [SWelsPPS],
    ) {
        let Some(pExistingParasetList) = pExistingParasetList else {
            return;
        };
        if !self.eIdKind.is_listing() {
            return;
        }
        // `CWelsParametersetSpsListing::LoadPreviousSps` — `:424`.
        self.m_sParaSetOffset.uiInUseSpsNum = pExistingParasetList.uiInUseSpsNum;
        debug_assert!(
            pSpsArray.is_empty() || pSpsArray.len() >= MAX_SPS_COUNT,
            "pSpsArray holds {} entries; LoadPrevious copies {MAX_SPS_COUNT}",
            pSpsArray.len(),
        );
        if !pSpsArray.is_empty() {
            let n = MAX_SPS_COUNT.min(pSpsArray.len());
            pSpsArray[..n].copy_from_slice(&pExistingParasetList.sSps[..n]);
        }
        if self.GetNeededSubsetSpsNum() > 0 {
            self.m_sParaSetOffset.uiInUseSubsetSpsNum = pExistingParasetList.uiInUseSubsetSpsNum;
            debug_assert!(
                pSubsetArray.is_empty() || pSubsetArray.len() >= MAX_SPS_COUNT,
                "pSubsetArray holds {} entries; LoadPrevious copies {MAX_SPS_COUNT}",
                pSubsetArray.len(),
            );
            if !pSubsetArray.is_empty() {
                let n = MAX_SPS_COUNT.min(pSubsetArray.len());
                pSubsetArray[..n].copy_from_slice(&pExistingParasetList.sSubsetSps[..n]);
            }
        } else {
            self.m_sParaSetOffset.uiInUseSubsetSpsNum = 0;
        }
        // `CWelsParametersetSpsPpsListing::LoadPreviousPps` — `:549`. Only this kind
        // carries PPSs across.
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            self.m_sParaSetOffset.uiInUsePpsNum = pExistingParasetList.uiInUsePpsNum;
            debug_assert!(
                pPpsArray.is_empty() || pPpsArray.len() >= MAX_PPS_COUNT,
                "pPpsArray holds {} entries; LoadPrevious copies {MAX_PPS_COUNT}",
                pPpsArray.len(),
            );
            if !pPpsArray.is_empty() {
                let n = MAX_PPS_COUNT.min(pPpsArray.len());
                pPpsArray[..n].copy_from_slice(&pExistingParasetList.sPps[..n]);
            }
        }
    }

    /// `UpdatePpsList` — `paraset_strategy.h:114` (empty for four of the five kinds) /
    /// `paraset_strategy.cpp:560` (SpsPpsListing).
    ///
    /// Pre-expands `pps` from the `iPpsNum` distinct PPSs built to the full
    /// `MAX_PPS_COUNT`, each a copy of one of them with its own `iPpsId`, and fills
    /// `iPpsIdList[pps][idr_round]` with the id to use on each IDR round;
    /// `GetCurrentPpsId` reads it.
    ///
    /// Callers use [`ctx_strategy_and_pps`]. `pps` must hold `MAX_PPS_COUNT` entries;
    /// indexing panics rather than running past it.
    pub fn UpdatePpsList(&mut self, pps: &mut [SWelsPPS], pPpsNum: &mut i32) {
        if self.eIdKind != ParasetIdKind::SpsPpsListing {
            return;
        }
        let iPpsNum = *pPpsNum;
        if iPpsNum >= MAX_PPS_COUNT as i32 {
            return;
        }
        // `iUsePpsNum` is a divisor two statements down, so zero must not reach it.
        if iPpsNum <= 0 {
            return;
        }
        let iUsePpsNum = iPpsNum;
        for iIdrRound in 0..MAX_PPS_COUNT {
            for iPpsId in 0..iPpsNum as usize {
                self.m_sParaSetOffset.iPpsIdList[iPpsId][iIdrRound] =
                    ((iIdrRound * iUsePpsNum as usize + iPpsId) % MAX_PPS_COUNT) as i32;
            }
        }
        for iPpsId in iUsePpsNum as usize..MAX_PPS_COUNT {
            let src = pps[iPpsId % iUsePpsNum as usize];
            let dst = &mut pps[iPpsId];
            *dst = src;
            dst.iPpsId = iPpsId as u32;
            *pPpsNum += 1;
        }
        self.m_sParaSetOffset.uiInUsePpsNum = *pPpsNum as u32;
    }

    /// `CheckParamCompatibility` — `paraset_strategy.h:116` (unconditionally true) /
    /// `paraset_strategy.cpp:449` (the listing kinds).
    ///
    /// A listing strategy needs a single SVC spatial layer; with more it falls back to
    /// `CONSTANT_ID`.
    pub fn CheckParamCompatibility(
        &mut self,
        // `eSpsPpsIdStrategy` is written back when the listing strategy is refused.
        pCodingParam: &mut SWelsSvcCodingParam,
        pLogCtx: SLogContext,
    ) -> bool {
        if !self.eIdKind.is_listing() {
            return true;
        }
        if pCodingParam.iSpatialLayerNum > 1 && !pCodingParam.bSimulcastAVC {
            WelsLog(
                pLogCtx,
                WELS_LOG_WARNING,
                &format!(
                    "ParamValidationExt(), eSpsPpsIdStrategy setting ({:?}) with multiple svc SpatialLayers ({}) not supported! eSpsPpsIdStrategy adjusted to CONSTANT_ID",
                    pCodingParam.eSpsPpsIdStrategy, pCodingParam.iSpatialLayerNum
                ),
            );
            pCodingParam.eSpsPpsIdStrategy = EParameterSetStrategy::CONSTANT_ID;
            return false;
        }
        true
    }

    /// `CheckPpsGenerating` — `paraset_strategy.h:158` / `paraset_strategy.cpp:463`
    /// (SpsListing, always true) / `:586` (SpsPpsListing, false once the PPS list is
    /// full). `GenerateNewSps` is its only caller.
    #[inline]
    fn CheckPpsGenerating(&self) -> bool {
        match self.eIdKind {
            ParasetIdKind::SpsPpsListing => {
                (self.m_sParaSetOffset.uiInUsePpsNum as usize) < MAX_PPS_COUNT
            }
            _ => true,
        }
    }

    /// `SpsReset` — `paraset_strategy.cpp:466` (SpsListing) / `:600` (SpsPpsListing,
    /// which refuses with -1 because a reset would invalidate the PPS list). Called
    /// only from `GenerateNewSps` when the SPS list wraps.
    ///
    /// `pSpsArray` / `pSubsetArray` must hold `MAX_SPS_COUNT` entries.
    fn SpsReset(
        &mut self,
        pSpsArray: &mut [SWelsSPS],
        pSubsetArray: &mut [SSubsetSps],
        kbUseSubsetSps: bool,
    ) -> i32 {
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            return -1;
        }
        // `ZERO`, not `default()`: `Default` seeds `uiProfileIdc = PRO_BASELINE` and
        // the VUI `*_UNDEF` values, which are not zero.
        if !kbUseSubsetSps {
            self.m_sParaSetOffset.uiInUseSpsNum = 1;
            for i in 0..MAX_SPS_COUNT {
                pSpsArray[i] = SWelsSPS::ZERO;
            }
        } else {
            self.m_sParaSetOffset.uiInUseSubsetSpsNum = 1;
            for i in 0..MAX_SPS_COUNT {
                pSubsetArray[i] = SSubsetSps::ZERO;
            }
        }
        0
    }

    /// `GenerateNewSps` — `paraset_strategy.cpp:265`.
    pub fn GenerateNewSps(
        &mut self,
        pParam: &mut SWelsSvcCodingParam,
        pSpsArray: &mut [SWelsSPS],
        pSubsetArray: &mut [SSubsetSps],
        _pPpsArray: &mut [SWelsPPS],
        kbUseSubsetSps: bool,
        iDlayerIndex: i32,
        iDlayerCount: i32,
        kuiSpsId: u32,
        bSVCBaselayer: bool,
    ) -> u32 {
        if !self.eIdKind.is_listing() {
            WelsGenerateNewSps(
                pParam,
                pSpsArray,
                pSubsetArray,
                kbUseSubsetSps,
                iDlayerIndex,
                iDlayerCount,
                kuiSpsId as i32,
                bSVCBaselayer,
            );
            return kuiSpsId;
        }

        // `CWelsParametersetSpsListing::GenerateNewSps` — `paraset_strategy.cpp:475`.
        // Reuse an SPS the decoder already has if the configuration matches one;
        // otherwise take the next id, wrapping through `SpsReset`.
        let iSpsNumInUse = if kbUseSubsetSps {
            self.m_sParaSetOffset.uiInUseSubsetSpsNum
        } else {
            self.m_sParaSetOffset.uiInUseSpsNum
        } as i32;
        let kiFoundSpsId = FindExistingSps(
            pParam,
            kbUseSubsetSps,
            iDlayerIndex,
            iDlayerCount,
            iSpsNumInUse,
            pSpsArray,
            pSubsetArray,
            bSVCBaselayer,
        );
        if INVALID_ID != kiFoundSpsId {
            return kiFoundSpsId as u32;
        }
        if !self.CheckPpsGenerating() {
            // The caller compares the returned id against `u32::MAX`.
            return u32::MAX;
        }
        let mut kuiSpsId = if !kbUseSubsetSps {
            let id = self.m_sParaSetOffset.uiInUseSpsNum;
            self.m_sParaSetOffset.uiInUseSpsNum += 1;
            id
        } else {
            let id = self.m_sParaSetOffset.uiInUseSubsetSpsNum;
            self.m_sParaSetOffset.uiInUseSubsetSpsNum += 1;
            id
        };
        if kuiSpsId >= MAX_SPS_COUNT as u32 {
            if self.SpsReset(pSpsArray, pSubsetArray, kbUseSubsetSps) < 0 {
                return u32::MAX;
            }
            kuiSpsId = 0;
        }
        WelsGenerateNewSps(
            pParam,
            pSpsArray,
            pSubsetArray,
            kbUseSubsetSps,
            iDlayerIndex,
            iDlayerCount,
            kuiSpsId as i32,
            bSVCBaselayer,
        );
        kuiSpsId
    }

    /// `InitPps` — `paraset_strategy.cpp:276`. `kbDeblockingFilterPresentFlag` is
    /// ignored; `true` is passed through.
    ///
    /// `pps` must hold at least `kuiPpsId + 1` entries. Callers use
    /// [`ctx_strategy_and_pps`].
    pub fn InitPps(
        &mut self,
        pps: &mut [SWelsPPS],
        _kiSpsId: u32,
        pSps: Option<&SWelsSPS>,
        pSubsetSps: Option<&SSubsetSps>,
        kuiPpsId: u32,
        _kbDeblockingFilterPresentFlag: bool,
        kbUsingSubsetSps: bool,
        kbEntropyCodingModeFlag: bool,
    ) -> u32 {
        // `CWelsParametersetSpsPpsListing::InitPps` — `paraset_strategy.cpp:639`.
        // Only that kind looks for an existing PPS; the rest write the named slot.
        let mut kuiPpsId = kuiPpsId;
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            let kiFoundPpsId = FindExistingPps(
                pSps,
                pSubsetSps,
                kbUsingSubsetSps,
                _kiSpsId as i32,
                kbEntropyCodingModeFlag,
                self.m_sParaSetOffset.uiInUsePpsNum as i32,
                pps,
            );
            if INVALID_ID != kiFoundPpsId {
                kuiPpsId = kiFoundPpsId as u32;
                self.SetUseSubsetFlag(kuiPpsId, kbUsingSubsetSps);
                return kuiPpsId;
            }
            kuiPpsId = self.m_sParaSetOffset.uiInUsePpsNum;
            self.m_sParaSetOffset.uiInUsePpsNum += 1;
        }
        WelsInitPps(
            &mut pps[kuiPpsId as usize],
            pSps,
            pSubsetSps,
            kuiPpsId,
            true,
            kbUsingSubsetSps,
            kbEntropyCodingModeFlag,
        );
        self.SetUseSubsetFlag(kuiPpsId, kbUsingSubsetSps);
        kuiPpsId
    }

    /// `SetUseSubsetFlag` — `paraset_strategy.cpp:288`.
    #[inline]
    pub fn SetUseSubsetFlag(&mut self, iPpsId: u32, bUseSubsetSps: bool) {
        self.m_sParaSetOffset.bPpsIdMappingIntoSubsetsps[iPpsId as usize] = bUseSubsetSps;
    }

    /// `UpdateParaSetNum` — `paraset_strategy.h:139` (empty) / `paraset_strategy.cpp:515`
    /// (SpsListing) / `:664` (SpsPpsListing, which adds the PPS count).
    ///
    /// Tells the bitstream writer how many parameter sets to emit:
    /// `WriteSavcParaset_Listing` loops to `iSpsNum` and `iPpsNum`.
    ///
    /// Callers use [`ctx_strategy_and_counts`].
    pub fn UpdateParaSetNum(
        &mut self,
        pSpsNum: &mut i32,
        pSubsetSpsNum: &mut i32,
        pPpsNum: &mut i32,
    ) {
        if !self.eIdKind.is_listing() {
            return;
        }
        *pSpsNum = self.m_sParaSetOffset.uiInUseSpsNum as i32;
        *pSubsetSpsNum = self.m_sParaSetOffset.uiInUseSubsetSpsNum as i32;
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            *pPpsNum = self.m_sParaSetOffset.uiInUsePpsNum as i32;
        }
    }

    /// `GetCurrentPpsId` — `paraset_strategy.h:141` (the identity) /
    /// `paraset_strategy.cpp:671` (SpsPpsListing, which rotates by IDR round through
    /// the list `UpdatePpsList` built).
    #[inline]
    pub fn GetCurrentPpsId(&self, iPpsId: i32, iIdrLoop: i32) -> i32 {
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            self.m_sParaSetOffset.iPpsIdList[iPpsId as usize][iIdrLoop as usize]
        } else {
            iPpsId
        }
    }

    /// `GetSpsIdx` — `paraset_strategy.h:150` (always 0, there being one SPS) /
    /// `:252` (the listing kinds, the identity into their list).
    #[inline]
    pub fn GetSpsIdx(&self, iIdx: i32) -> i32 {
        if self.eIdKind.is_listing() { iIdx } else { 0 }
    }
}

/// `WelsGenerateNewSps` — `paraset_strategy.cpp:78` (file-static).
pub fn WelsGenerateNewSps(
    pParam: &mut SWelsSvcCodingParam,
    pSpsArray: &mut [SWelsSPS],
    pSubsetArray: &mut [SSubsetSps],
    kbUseSubsetSps: bool,
    iDlayerIndex: i32,
    iDlayerCount: i32,
    kiSpsId: i32,
    bSVCBaselayer: bool,
) -> i32 {
    let iRet;
    // The two layer records come out of one destructure: the callee writes
    // `uiLevelIdc` back while reading the internal record, disjoint fields of
    // `pParam`. The scalars come out before that borrow.
    let kuiIntraPeriod = pParam.uiIntraPeriod;
    let kiMaxNumRefFrame = pParam.iMaxNumRefFrame;
    let kbEnableFrameCropping = pParam.bEnableFrameCroppingFlag;
    let kbEnableRc = pParam.iRCMode != RC_OFF_MODE;
    let SWelsSvcCodingParam {
        sSpatialLayers,
        sDependencyLayers,
        ..
    } = &mut *pParam;
    let pDlayerParam = &mut sSpatialLayers[iDlayerIndex as usize];
    let pDlayerInternal = &sDependencyLayers[iDlayerIndex as usize];
    if !kbUseSubsetSps {
        iRet = WelsInitSps(
            &mut pSpsArray[kiSpsId as usize],
            pDlayerParam,
            pDlayerInternal,
            kuiIntraPeriod,
            kiMaxNumRefFrame,
            kiSpsId as u32,
            kbEnableFrameCropping,
            kbEnableRc,
            iDlayerCount,
            bSVCBaselayer,
        );
    } else {
        iRet = WelsInitSubsetSps(
            &mut pSubsetArray[kiSpsId as usize],
            pDlayerParam,
            pDlayerInternal,
            kuiIntraPeriod,
            kiMaxNumRefFrame,
            kiSpsId as u32,
            kbEnableFrameCropping,
            kbEnableRc,
            iDlayerCount,
        );
    }
    iRet
}

/// `ParasetIdAdditionIdAdjust` — `paraset_strategy.cpp:337`.
///
/// Rotates the id actually written to the bitstream, recording the delta from the
/// encoder-side id. `paraset_type = 0: SPS; = 1: PPS`.
fn ParasetIdAdditionIdAdjust(
    sParaSetOffsetVariable: &mut SParaSetOffsetVariable,
    kiCurEncoderParaSetId: i32,
    kuiMaxIdInBs: u32,
) {
    // SPS_ID in avc_sps and pSubsetSps will be different using this.
    // SPS_ID case example:
    // 1st  enter:  next_spsid_in_bs == 0;  spsid == 0;    delta == 0;      // actual 0
    // 1st  finish: next_spsid_in_bs == 1;
    // 2nd  enter:  next_spsid_in_bs == 1;  spsid == 0;    delta == 1;      // actual 1
    // 31st enter:  next_spsid_in_bs == 31; spsid == 0~2;  delta == 31~29;  // actual 31
    // 31st finish: next_spsid_in_bs == 0;
    let kiEncId = kiCurEncoderParaSetId;
    let mut uiNextIdInBs = sParaSetOffsetVariable.uiNextParaSetIdToUseInBs;

    // Change the current parameter set's id_delta. The subtraction is computed in u32
    // and stored in an i32, so it wraps rather than saturating.
    sParaSetOffsetVariable.iParaSetIdDelta[kiEncId as usize] =
        uiNextIdInBs.wrapping_sub(kiEncId as u32) as i32;
    // write pso data for the next update: mark the used id
    sParaSetOffsetVariable.bUsedParaSetIdInBs[uiNextIdInBs as usize] = true;

    // prepare for the next update: find the next available id
    uiNextIdInBs += 1;
    if uiNextIdInBs >= kuiMaxIdInBs {
        uiNextIdInBs = 0; // ensure the SPS_ID would not exceed MAX_SPS_COUNT
    }
    sParaSetOffsetVariable.uiNextParaSetIdToUseInBs = uiNextIdInBs;
}

/// The installed parameter-set strategy, borrowed for one call.
///
/// Not cached in a local: several call sites reach the same object back through
/// `pCtx->pFuncList`, so a `&mut` held across them would alias itself. Callers keep
/// the reference for the length of one expression.
///
/// # Panics
/// Panics if no strategy is installed. `InitFunctionPointers` installs one, and the
/// call sites that run before it test the field first.
#[inline]
pub fn ParasetStrategy(pCtx: &mut sWelsEncCtx) -> &mut CWelsParametersetIdStrategyObj {
    pCtx.pFuncList
        .pParametersetStrategy
        .as_deref_mut()
        .expect("pParametersetStrategy is installed by InitFunctionPointers")
}

/// The strategy and the PPS list it rewrites, as disjoint borrows of one context.
///
/// The strategy object lives inside the context at `pFuncList.pParametersetStrategy`,
/// so a method also taking the context would claim it twice; splitting at the source
/// is what the compiler can see is disjoint.
#[inline]
pub fn ctx_strategy_and_pps(
    pCtx: &mut sWelsEncCtx,
) -> (
    &mut CWelsParametersetIdStrategyObj,
    &mut [SWelsPPS],
    &mut i32,
) {
    (
        pCtx.pFuncList
            .pParametersetStrategy
            .as_deref_mut()
            .expect("pParametersetStrategy is installed by InitFunctionPointers"),
        &mut pCtx.pPPSArray,
        &mut pCtx.iPpsNum,
    )
}

/// The strategy and the three parameter-set arrays, for `LoadPrevious`.
#[inline]
pub fn ctx_strategy_and_paraset_arrays(
    pCtx: &mut sWelsEncCtx,
) -> (
    &mut CWelsParametersetIdStrategyObj,
    &mut [SWelsSPS],
    &mut [SSubsetSps],
    &mut [SWelsPPS],
) {
    (
        pCtx.pFuncList
            .pParametersetStrategy
            .as_deref_mut()
            .expect("pParametersetStrategy is installed by InitFunctionPointers"),
        &mut pCtx.pSpsArray,
        &mut pCtx.pSubsetArray,
        &mut pCtx.pPPSArray,
    )
}

/// The strategy, the coding parameters and the three arrays, for `GenerateNewSps` —
/// [`crate::encoder::encoder_context::sWelsEncCtx::param_and_paraset_arrays_mut`]'s
/// four, with the strategy alongside them.
#[inline]
pub fn ctx_strategy_and_param_arrays(
    pCtx: &mut sWelsEncCtx,
) -> (
    &mut CWelsParametersetIdStrategyObj,
    &mut SWelsSvcCodingParam,
    &mut [SWelsSPS],
    &mut [SSubsetSps],
    &mut [SWelsPPS],
) {
    let sWelsEncCtx {
        pFuncList,
        pSvcParam,
        pSpsArray,
        pSubsetArray,
        pPPSArray,
        ..
    } = pCtx;
    (
        pFuncList
            .pParametersetStrategy
            .as_deref_mut()
            .expect("pParametersetStrategy is installed by InitFunctionPointers"),
        pSvcParam
            .as_deref_mut()
            .expect("the coding parameters are built by WelsInitEncoderExt"),
        pSpsArray,
        pSubsetArray,
        pPPSArray,
    )
}

/// The strategy and the encoder output block, for the three parameter-set writers,
/// each of which holds the id-offset list live across a `pOut` write.
#[inline]
pub fn ctx_strategy_and_out(
    pCtx: &mut sWelsEncCtx,
) -> (&mut CWelsParametersetIdStrategyObj, &mut SWelsEncoderOutput) {
    let sWelsEncCtx {
        pFuncList, pOut, ..
    } = pCtx;
    (
        pFuncList
            .pParametersetStrategy
            .as_deref_mut()
            .expect("pParametersetStrategy is installed by InitFunctionPointers"),
        pOut.as_deref_mut().expect("pOut lives"),
    )
}

/// The strategy and the three parameter-set counts `UpdateParaSetNum` publishes.
#[inline]
pub fn ctx_strategy_and_counts(
    pCtx: &mut sWelsEncCtx,
) -> (
    &mut CWelsParametersetIdStrategyObj,
    &mut i32,
    &mut i32,
    &mut i32,
) {
    (
        pCtx.pFuncList
            .pParametersetStrategy
            .as_deref_mut()
            .expect("pParametersetStrategy is installed by InitFunctionPointers"),
        &mut pCtx.iSpsNum,
        &mut pCtx.iSubsetSpsNum,
        &mut pCtx.iPpsNum,
    )
}

/// `IWelsParametersetStrategy::CreateParametersetStrategy` — `paraset_strategy.cpp:40`.
///
/// Always `Some`: `EParameterSetStrategy` is a closed five-variant enum and all five
/// are mapped.
pub fn CreateParametersetStrategy(
    eSpsPpsIdStrategy: EParameterSetStrategy,
    bSimulcastAVC: bool,
    kiSpatialLayerNum: i32,
) -> Option<Box<CWelsParametersetIdStrategyObj>> {
    let eIdKind = match eSpsPpsIdStrategy {
        EParameterSetStrategy::CONSTANT_ID => ParasetIdKind::Constant,
        EParameterSetStrategy::INCREASING_ID => ParasetIdKind::Increasing,
        EParameterSetStrategy::SPS_LISTING => ParasetIdKind::SpsListing,
        EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING => {
            ParasetIdKind::SpsListingPpsIncreasing
        }
        EParameterSetStrategy::SPS_PPS_LISTING => ParasetIdKind::SpsPpsListing,
    };
    Some(CWelsParametersetIdStrategyObj::new(
        eIdKind,
        bSimulcastAVC,
        kiSpatialLayerNum,
    ))
}

/// `CheckMatchedSps` — `paraset_strategy.cpp:106` (file-static).
pub fn CheckMatchedSps(pSps1: &SWelsSPS, pSps2: &SWelsSPS) -> bool {
    if pSps1.iMbWidth != pSps2.iMbWidth || pSps1.iMbHeight != pSps2.iMbHeight {
        return false;
    }

    if pSps1.uiLog2MaxFrameNum != pSps2.uiLog2MaxFrameNum
        || pSps1.iLog2MaxPocLsb != pSps2.iLog2MaxPocLsb
    {
        return false;
    }

    if pSps1.iNumRefFrames != pSps2.iNumRefFrames {
        return false;
    }

    if pSps1.bFrameCroppingFlag != pSps2.bFrameCroppingFlag
        || pSps1.sFrameCrop.iCropLeft != pSps2.sFrameCrop.iCropLeft
        || pSps1.sFrameCrop.iCropRight != pSps2.sFrameCrop.iCropRight
        || pSps1.sFrameCrop.iCropTop != pSps2.sFrameCrop.iCropTop
        || pSps1.sFrameCrop.iCropBottom != pSps2.sFrameCrop.iCropBottom
    {
        return false;
    }

    if pSps1.uiProfileIdc != pSps2.uiProfileIdc
        || pSps1.bConstraintSet0Flag != pSps2.bConstraintSet0Flag
        || pSps1.bConstraintSet1Flag != pSps2.bConstraintSet1Flag
        || pSps1.bConstraintSet2Flag != pSps2.bConstraintSet2Flag
        || pSps1.bConstraintSet3Flag != pSps2.bConstraintSet3Flag
        || pSps1.iLevelIdc != pSps2.iLevelIdc
    {
        return false;
    }

    true
}

/// `CheckMatchedSubsetSps` — `paraset_strategy.cpp:143` (file-static).
pub fn CheckMatchedSubsetSps(pSubsetSps1: &SSubsetSps, pSubsetSps2: &SSubsetSps) -> bool {
    if !CheckMatchedSps(&pSubsetSps1.pSps, &pSubsetSps2.pSps) {
        return false;
    }

    if pSubsetSps1.sSpsSvcExt.iExtendedSpatialScalability
        != pSubsetSps2.sSpsSvcExt.iExtendedSpatialScalability
        || pSubsetSps1.sSpsSvcExt.bAdaptiveTcoeffLevelPredFlag
            != pSubsetSps2.sSpsSvcExt.bAdaptiveTcoeffLevelPredFlag
        || pSubsetSps1.sSpsSvcExt.bSeqTcoeffLevelPredFlag
            != pSubsetSps2.sSpsSvcExt.bSeqTcoeffLevelPredFlag
        || pSubsetSps1.sSpsSvcExt.bSliceHeaderRestrictionFlag
            != pSubsetSps2.sSpsSvcExt.bSliceHeaderRestrictionFlag
    {
        return false;
    }

    true
}

/// `FindExistingPps` — `paraset_strategy.cpp:608`.
///
/// Returns the index of a stored PPS the current configuration would produce, or
/// [`INVALID_ID`]. Its only caller is `SpsPpsListing`'s `InitPps`.
///
/// Six fields are compared rather than the whole struct: `iPpsId` and the
/// deblocking-filter idc fields differ between a stored entry and the probe by
/// construction, so a full compare would never match.
///
/// # Panics
/// Panics if `pPpsArray` holds fewer than `iPpsNumInUse` entries.
pub fn FindExistingPps(
    pSps: Option<&SWelsSPS>,
    pSubsetSps: Option<&SSubsetSps>,
    kbUseSubsetSps: bool,
    _iSpsId: i32,
    kbEntropyCodingFlag: bool,
    iPpsNumInUse: i32,
    pPpsArray: &[SWelsPPS],
) -> i32 {
    let mut sTmpPps = SWelsPPS::default();
    WelsInitPps(
        &mut sTmpPps,
        pSps,
        pSubsetSps,
        0,
        true,
        kbUseSubsetSps,
        kbEntropyCodingFlag,
    );

    for iId in 0..iPpsNumInUse {
        let p = &pPpsArray[iId as usize];
        if sTmpPps.iSpsId == p.iSpsId
            && sTmpPps.bEntropyCodingModeFlag == p.bEntropyCodingModeFlag
            && sTmpPps.iPicInitQp == p.iPicInitQp
            && sTmpPps.iPicInitQs == p.iPicInitQs
            && sTmpPps.uiChromaQpIndexOffset == p.uiChromaQpIndexOffset
            && sTmpPps.bDeblockingFilterControlPresentFlag == p.bDeblockingFilterControlPresentFlag
        {
            return iId;
        }
    }

    INVALID_ID
}

/// `FindExistingSps` — `paraset_strategy.cpp:169`.
///
/// Returns the index of a stored parameter set matching the current configuration, or
/// [`INVALID_ID`].
pub fn FindExistingSps(
    pParam: &mut SWelsSvcCodingParam,
    kbUseSubsetSps: bool,
    iDlayerIndex: i32,
    iDlayerCount: i32,
    iSpsNumInUse: i32,
    pSpsArray: &[SWelsSPS],
    pSubsetArray: &[SSubsetSps],
    bSVCBaseLayer: bool,
) -> i32 {
    // The two layer records come out of one destructure: the callee writes
    // `uiLevelIdc` back while reading the internal record, disjoint fields of
    // `pParam`. The scalars come out before that borrow.
    let kuiIntraPeriod = pParam.uiIntraPeriod;
    let kiMaxNumRefFrame = pParam.iMaxNumRefFrame;
    let kbEnableFrameCropping = pParam.bEnableFrameCroppingFlag;
    let kbEnableRc = pParam.iRCMode != RC_OFF_MODE;
    let SWelsSvcCodingParam {
        sSpatialLayers,
        sDependencyLayers,
        ..
    } = &mut *pParam;
    let pDlayerParam = &mut sSpatialLayers[iDlayerIndex as usize];
    let pDlayerInternal = &sDependencyLayers[iDlayerIndex as usize];

    if !kbUseSubsetSps {
        let mut sTmpSps = SWelsSPS::default();
        WelsInitSps(
            &mut sTmpSps,
            pDlayerParam,
            pDlayerInternal,
            kuiIntraPeriod,
            kiMaxNumRefFrame,
            0,
            kbEnableFrameCropping,
            kbEnableRc,
            iDlayerCount,
            bSVCBaseLayer,
        );
        for iId in 0..iSpsNumInUse {
            if CheckMatchedSps(&sTmpSps, &pSpsArray[iId as usize]) {
                return iId;
            }
        }
    } else {
        let mut sTmpSubsetSps = SSubsetSps::default();
        WelsInitSubsetSps(
            &mut sTmpSubsetSps,
            pDlayerParam,
            pDlayerInternal,
            kuiIntraPeriod,
            kiMaxNumRefFrame,
            0,
            kbEnableFrameCropping,
            kbEnableRc,
            iDlayerCount,
        );

        for iId in 0..iSpsNumInUse {
            if CheckMatchedSubsetSps(&sTmpSubsetSps, &pSubsetArray[iId as usize]) {
                return iId;
            }
        }
    }

    INVALID_ID
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strategy(e: EParameterSetStrategy) -> Box<CWelsParametersetIdStrategyObj> {
        CreateParametersetStrategy(e, false, 1).expect("ported strategy")
    }

    #[test]
    fn constant_strategy_reports_zero_id_offsets() {
        let p = strategy(EParameterSetStrategy::CONSTANT_ID);
        assert_eq!(p.eIdKind, ParasetIdKind::Constant);
        assert_eq!(p.GetPpsIdOffset(0), 0);
        assert_eq!(p.GetSpsIdOffset(0, 0), 0);
    }

    /// `m_iBasicNeededSpsNum` is 1 and `m_iBasicNeededPpsNum` is `1 + layers`;
    /// without simulcast AVC neither is scaled by the layer count and the subset-SPS
    /// count is `layers - 1` (`paraset_strategy.cpp:233-254`).
    #[test]
    fn constant_strategy_paraset_counts() {
        let mut p = strategy(EParameterSetStrategy::CONSTANT_ID);
        assert_eq!(p.GetNeededSpsNum(), 1);
        assert_eq!(p.GetNeededSubsetSpsNum(), 0);
        assert_eq!(p.GetNeededPpsNum(), 2);
        assert_eq!(p.GetAllNeededParasetNum(), 3);
    }

    /// The counts are shared, not per-kind: `Constant` and `Increasing` answer
    /// identically.
    #[test]
    fn both_kinds_share_the_inherited_counts() {
        let mut c = strategy(EParameterSetStrategy::CONSTANT_ID);
        let mut i = strategy(EParameterSetStrategy::INCREASING_ID);
        assert_eq!(i.eIdKind, ParasetIdKind::Increasing);
        assert_eq!(c.GetNeededSpsNum(), i.GetNeededSpsNum());
        assert_eq!(c.GetNeededSubsetSpsNum(), i.GetNeededSubsetSpsNum());
        assert_eq!(c.GetNeededPpsNum(), i.GetNeededPpsNum());
        assert_eq!(c.GetAllNeededParasetNum(), i.GetAllNeededParasetNum());
        assert_eq!(c.GetCurrentPpsId(3, 7), i.GetCurrentPpsId(3, 7));
        assert_eq!(c.GetSpsIdx(2), i.GetSpsIdx(2));
    }

    /// Every one of the five builds, and each maps to the class the header names.
    #[test]
    fn every_strategy_builds_and_maps_to_its_class() {
        let cases = [
            (EParameterSetStrategy::CONSTANT_ID, ParasetIdKind::Constant),
            (
                EParameterSetStrategy::INCREASING_ID,
                ParasetIdKind::Increasing,
            ),
            (
                EParameterSetStrategy::SPS_LISTING,
                ParasetIdKind::SpsListing,
            ),
            (
                EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING,
                ParasetIdKind::SpsListingPpsIncreasing,
            ),
            (
                EParameterSetStrategy::SPS_PPS_LISTING,
                ParasetIdKind::SpsPpsListing,
            ),
        ];
        for (e, kind) in cases {
            let p = CreateParametersetStrategy(e, false, 1).expect("all five build");
            assert_eq!(p.eIdKind, kind, "{e:?}");
        }
    }

    /// A listing strategy asks `RequestMemorySvc` for the whole array
    /// (`paraset_strategy.cpp:410-411`, `:545-546`), because that is what it fills.
    #[test]
    fn listing_kinds_ask_for_the_whole_array() {
        let mut sl = strategy(EParameterSetStrategy::SPS_LISTING);
        assert_eq!(sl.GetNeededSpsNum(), MAX_SPS_COUNT as u32);
        assert_eq!(sl.GetNeededSubsetSpsNum(), MAX_SPS_COUNT as u32);
        assert_eq!(sl.GetNeededPpsNum(), 1);

        let mut spl = strategy(EParameterSetStrategy::SPS_PPS_LISTING);
        assert_eq!(spl.GetNeededSpsNum(), MAX_SPS_COUNT as u32);
        assert_eq!(spl.GetNeededPpsNum(), MAX_PPS_COUNT as u32);

        // `SpsListingPpsIncreasing` shares `SpsListing`'s constructor.
        let mut sli = strategy(EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING);
        assert_eq!(sli.GetNeededSpsNum(), sl.GetNeededSpsNum());
        assert_eq!(sli.GetNeededPpsNum(), sl.GetNeededPpsNum());
    }

    /// `SpsListingPpsIncreasing` takes only `GetPpsIdOffset` and `Update` from
    /// `Increasing` (`paraset_strategy.h:294-301`): its PPS id rotates, its SPS id
    /// offset stays zero.
    #[test]
    fn sps_listing_pps_increasing_rotates_only_the_pps_id() {
        let mut p = strategy(EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING);
        // One rotation of the PPS id, as `WelsWriteOnePPS`'s caller would do.
        p.Update(0, PARA_SET_TYPE_PPS as i32);
        p.Update(0, PARA_SET_TYPE_PPS as i32);
        assert_ne!(p.GetPpsIdOffset(0), 0, "the PPS id must rotate");
        // …and the SPS side does not, however many times it is asked.
        p.Update(0, PARA_SET_TYPE_AVCSPS as i32);
        assert_eq!(
            p.GetSpsIdOffset(0, 0),
            0,
            "the SPS id offset is the Constant zero"
        );
    }

    /// `GetSpsIdx` — the constant kinds answer 0 for every index, having one SPS; a
    /// listing kind answers the index into its list.
    #[test]
    fn get_sps_idx_is_the_identity_only_for_listing_kinds() {
        assert_eq!(strategy(EParameterSetStrategy::CONSTANT_ID).GetSpsIdx(3), 0);
        assert_eq!(
            strategy(EParameterSetStrategy::INCREASING_ID).GetSpsIdx(3),
            0
        );
        assert_eq!(strategy(EParameterSetStrategy::SPS_LISTING).GetSpsIdx(3), 3);
        assert_eq!(
            strategy(EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING).GetSpsIdx(3),
            3
        );
        assert_eq!(
            strategy(EParameterSetStrategy::SPS_PPS_LISTING).GetSpsIdx(3),
            3
        );
    }

    /// `GetCurrentPpsId` — only `SPS_PPS_LISTING` rotates by IDR round, reading the
    /// list `UpdatePpsList` builds; with that list still zero it answers 0.
    #[test]
    fn get_current_pps_id_rotates_only_for_sps_pps_listing() {
        for e in [
            EParameterSetStrategy::CONSTANT_ID,
            EParameterSetStrategy::INCREASING_ID,
            EParameterSetStrategy::SPS_LISTING,
            EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING,
        ] {
            assert_eq!(strategy(e).GetCurrentPpsId(2, 5), 2, "{e:?}");
        }
        let mut p = strategy(EParameterSetStrategy::SPS_PPS_LISTING);
        assert_eq!(p.GetCurrentPpsId(2, 5), 0);
        // Fill the list by hand the way `UpdatePpsList` would for two PPSs, and the
        // rotation appears: round 5, pps 1 -> (5 * 2 + 1) % MAX_PPS_COUNT.
        for iIdrRound in 0..MAX_PPS_COUNT {
            for iPpsId in 0..2usize {
                p.m_sParaSetOffset.iPpsIdList[iPpsId][iIdrRound] =
                    ((iIdrRound * 2 + iPpsId) % MAX_PPS_COUNT) as i32;
            }
        }
        assert_eq!(
            p.GetCurrentPpsId(1, 5),
            ((5 * 2 + 1) % MAX_PPS_COUNT) as i32
        );
    }

    /// Repeated `Update` on encoder id 0 produces deltas 0, 1, 2, … up to
    /// `MAX_SPS_COUNT - 1`, then wraps to 0 (`paraset_strategy.cpp:337`).
    #[test]
    fn increasing_strategy_rotates_sps_id_in_bitstream() {
        let mut p = strategy(EParameterSetStrategy::INCREASING_ID);
        for expected in 0..MAX_SPS_COUNT as i32 {
            p.Update(0, PARA_SET_TYPE_AVCSPS as i32);
            assert_eq!(
                p.GetSpsIdOffset(0, 0),
                expected,
                "delta after update #{expected}"
            );
        }
        // 33rd update wraps uiNextParaSetIdToUseInBs back to 0.
        p.Update(0, PARA_SET_TYPE_AVCSPS as i32);
        assert_eq!(p.GetSpsIdOffset(0, 0), 0);
    }

    /// PPS ids rotate over `MAX_PPS_COUNT`, not `MAX_SPS_COUNT`.
    #[test]
    fn increasing_strategy_uses_pps_bound_for_pps_ids() {
        let mut p = strategy(EParameterSetStrategy::INCREASING_ID);
        for expected in 0..MAX_SPS_COUNT as i32 + 4 {
            p.Update(0, PARA_SET_TYPE_PPS as i32);
            assert_eq!(p.GetPpsIdOffset(0), expected);
        }
    }

    /// The constant kind's `Update` resets the whole offset block where the increasing
    /// kind rotates.
    #[test]
    fn constant_update_resets_rather_than_rotating() {
        let mut p = strategy(EParameterSetStrategy::CONSTANT_ID);
        for _ in 0..4 {
            p.Update(0, PARA_SET_TYPE_AVCSPS as i32);
            assert_eq!(p.GetSpsIdOffset(0, 0), 0);
        }
        p.SetUseSubsetFlag(1, true);
        p.Update(0, PARA_SET_TYPE_AVCSPS as i32);
        assert!(
            !p.m_sParaSetOffset.bPpsIdMappingIntoSubsetsps[1],
            "CONSTANT_ID's Update is a full reset of m_sParaSetOffset"
        );
    }
}
