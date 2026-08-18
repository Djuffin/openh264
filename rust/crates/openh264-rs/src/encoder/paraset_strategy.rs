//! Port of `codec/encoder/core/src/paraset_strategy.cpp` and
//! `codec/encoder/core/inc/paraset_strategy.h`.
//!
//! **Partial by design.** C++ declares one abstract `IWelsParametersetStrategy` and
//! five concrete strategies (`CONSTANT_ID`, `INCREASING_ID`, `SPS_LISTING`,
//! `SPS_LISTING_AND_PPS_INCREASING`, `SPS_PPS_LISTING`). **Two** are ported:
//! `CONSTANT_ID` (the Phase-5 gate configuration) and `INCREASING_ID` (the
//! `FillDefault` value, and so the strategy an unconfigured encoder actually runs).
//! [`CreateParametersetStrategy`] returns `None` for the three listing strategies
//! rather than silently substituting the constant one, which would produce a stream
//! that decodes but does not match C++.
//!
//! ### One object, two kinds — T4b.2a
//!
//! C++ layers three classes: `CWelsParametersetIdConstant`, the abstract
//! `CWelsParametersetIdNonConstant` (which overrides `OutputCurrentStructure` and
//! `LoadPreviousStructure`), and `CWelsParametersetIdIncreasing` (which adds
//! `GetPpsIdOffset`, `GetSpsIdOffset` and `Update` on top). **All three carry the same
//! data members** — only their vtables differ — so this module models them as one
//! [`CWelsParametersetIdStrategyObj`] carrying a [`ParasetIdKind`] discriminant, and
//! the five methods that actually differ `match` on it. The other fifteen have one
//! body, which is exactly what the C++ vtables say: `ID_INCREASING_VTBL` used to point
//! fifteen of its twenty entries at the `ConstId_*` thunks.
//!
//! This replaced a hand-written C-style vtable (`IWelsParametersetStrategyVtbl`, 20
//! entries, 25 thunks, 2 static instances). The vtable existed because `SWelsFuncPtrList`
//! stores this as a plain 8-byte member and a `*mut dyn Trait` is a 16-byte fat pointer
//! that would mis-size the struct. `Option<Box<CWelsParametersetIdStrategyObj>>` is 8
//! bytes by the null-pointer niche, so the size is kept without the indirection — and
//! the `Box` makes the object's owner visible, which is how T4b.2a found the leak
//! recorded as F19.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

use std::ptr::null_mut;

use crate::api::codec_api::EParameterSetStrategy;
use crate::api::codec_api::RC_MODES::RC_OFF_MODE;
use crate::encoder::au_set::{WelsInitPps, WelsInitSps, WelsInitSubsetSps};
use crate::encoder::encoder_context::{
    sWelsEncCtx, SLogContext, SParaSetOffset, SParaSetOffsetVariable, MAX_PPS_COUNT, PARA_SET_TYPE,
};
use crate::encoder::param_svc::{
    SExistingParasetList, SSubsetSps, SWelsPPS, SWelsSPS, SWelsSvcCodingParam, MAX_SPS_COUNT,
};

/// `PARA_SET_TYPE_AVCSPS` / `_SUBSETSPS` / `_PPS` — `wels_const.h`.
pub const PARA_SET_TYPE_AVCSPS: usize = 0;
pub const PARA_SET_TYPE_SUBSETSPS: usize = 1;
pub const PARA_SET_TYPE_PPS: usize = 2;

/// `INVALID_ID` — `wels_const.h`; returned by `FindExistingSps` when no stored
/// parameter set matches the current configuration.
pub const INVALID_ID: i32 = -1;

/// Which of C++'s parameter-set id strategies an object implements.
///
/// C++ spells this as three classes with three vtables over one data layout;
/// [`CWelsParametersetIdStrategyObj`] spells it as this discriminant, and only the
/// five methods whose vtable entries actually differ read it.
///
/// `Constant = 0` matters: [`SWelsFuncPtrList`](crate::encoder::wels_func_ptr_def::SWelsFuncPtrList)
/// is built by `WelsMallocz`, so the all-zero pattern must be a declared variant —
/// see the S21 note on [`CWelsParametersetIdStrategyObj`].
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParasetIdKind {
    /// `CWelsParametersetIdConstant` — `paraset_strategy.h:96`. Every id offset is 0.
    Constant = 0,
    /// `CWelsParametersetIdIncreasing` — `paraset_strategy.h:208`, via the abstract
    /// `CWelsParametersetIdNonConstant` (`paraset_strategy.h:180`). Rotates the id
    /// written to the bitstream and records the delta back to the encoder-side id.
    Increasing = 1,
}

/// The parameter-set id strategy object — C++'s `CWelsParametersetIdConstant`,
/// `CWelsParametersetIdNonConstant` and `CWelsParametersetIdIncreasing` merged, since
/// the three declare identical data members (`paraset_strategy.h:96`, `:180`, `:208`)
/// and differ only in vtable. `eIdKind` says which of them this object is.
///
/// Field order after `eIdKind` mirrors the C++ object: `m_sParaSetOffset`,
/// `m_bSimulcastAVC`, `m_iSpatialLayerNum`, `m_iBasicNeededSpsNum`,
/// `m_iBasicNeededPpsNum`.
///
/// **S21, the construction audit.** This type is only ever built by
/// [`CreateParametersetStrategy`], which fills every field; it is never `mem::zeroed`
/// and never `WelsMallocz`'d. What *is* zero-constructed is the
/// `SWelsFuncPtrList` that holds it, and there the field is an
/// `Option<Box<CWelsParametersetIdStrategyObj>>` whose all-zero pattern is `None` by
/// the null-pointer niche — a valid value, not a dangling box. The owned field is
/// therefore sound at all-zero, which is the case the rule asks about.
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
/// `paraset_strategy.cpp:203`. The `Increasing` constructor
/// (`paraset_strategy.cpp:365`) chains to it and adds nothing.
impl CWelsParametersetIdStrategyObj {
    pub fn new(eIdKind: ParasetIdKind, bSimulcastAVC: bool, kiSpatialLayerNum: i32) -> Box<Self> {
        Box::new(Self {
            eIdKind,
            // C++ memsets m_sParaSetOffset to 0.
            m_sParaSetOffset: SParaSetOffset::default(),
            m_bSimulcastAVC: bSimulcastAVC,
            m_iSpatialLayerNum: kiSpatialLayerNum,
            m_iBasicNeededSpsNum: 1,
            m_iBasicNeededPpsNum: (1 + kiSpatialLayerNum) as u32,
        })
    }

    // ------------------------------------------------------------------
    // The five methods whose C++ vtable entries differ between the kinds.
    // Read each `match` against `ID_CONSTANT_VTBL` / `ID_INCREASING_VTBL` as they
    // were: the `Constant` arm is the `ConstId_*` body, the `Increasing` arm the
    // `IncId_*` / `NonConstId_*` one.
    // ------------------------------------------------------------------

    /// `GetPpsIdOffset` — `paraset_strategy.cpp:216` (Constant) / `:384` (Increasing).
    #[inline]
    pub fn GetPpsIdOffset(&self, kiPpsId: i32) -> i32 {
        match self.eIdKind {
            ParasetIdKind::Constant => 0,
            ParasetIdKind::Increasing => {
                self.m_sParaSetOffset.sParaSetOffsetVariable[PARA_SET_TYPE_PPS].iParaSetIdDelta
                    [kiPpsId as usize]
            }
        }
    }

    /// `GetSpsIdOffset` — `paraset_strategy.cpp:219` (Constant) / `:391` (Increasing).
    #[inline]
    pub fn GetSpsIdOffset(&self, kiPpsId: i32, kiSpsId: i32) -> i32 {
        match self.eIdKind {
            ParasetIdKind::Constant => 0,
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
        }
    }

    /// `Update` — `paraset_strategy.cpp:261` (Constant) / `:370` (Increasing).
    #[inline]
    pub fn Update(&mut self, kuiId: u32, iParasetType: i32) {
        match self.eIdKind {
            ParasetIdKind::Constant => {
                self.m_sParaSetOffset = SParaSetOffset::default();
            }
            ParasetIdKind::Increasing => {
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
    /// `paraset_strategy.cpp:292` (`CWelsParametersetIdNonConstant`). `pPpsIdList`,
    /// `pCtx` and `pExistingParasetList` are accepted and unused, as in C++.
    ///
    /// # Safety
    /// On an `Increasing` object, `pParaSetOffsetVariable` must be writable for
    /// `PARA_SET_TYPE` elements.
    pub unsafe fn OutputCurrentStructure(
        &mut self,
        pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
        _pPpsIdList: *mut i32,
        _pCtx: *mut sWelsEncCtx,
        _pExistingParasetList: *mut SExistingParasetList,
    ) {
        match self.eIdKind {
            ParasetIdKind::Constant => {}
            ParasetIdKind::Increasing => {
                for k in 0..PARA_SET_TYPE {
                    self.m_sParaSetOffset.sParaSetOffsetVariable[k].bUsedParaSetIdInBs =
                        [false; MAX_PPS_COUNT];
                }
                std::ptr::copy_nonoverlapping(
                    self.m_sParaSetOffset.sParaSetOffsetVariable.as_ptr(),
                    pParaSetOffsetVariable,
                    PARA_SET_TYPE,
                );
            }
        }
    }

    /// `LoadPreviousStructure` — `paraset_strategy.h:148` (Constant, empty) /
    /// `paraset_strategy.cpp:300` (`CWelsParametersetIdNonConstant`).
    ///
    /// # Safety
    /// On an `Increasing` object, `pParaSetOffsetVariable` must be readable for
    /// `PARA_SET_TYPE` elements.
    pub unsafe fn LoadPreviousStructure(
        &mut self,
        pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
        _pPpsIdList: *mut i32,
    ) {
        match self.eIdKind {
            ParasetIdKind::Constant => {}
            ParasetIdKind::Increasing => {
                std::ptr::copy_nonoverlapping(
                    pParaSetOffsetVariable as *const SParaSetOffsetVariable,
                    self.m_sParaSetOffset.sParaSetOffsetVariable.as_mut_ptr(),
                    PARA_SET_TYPE,
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // The fifteen with one body. `ID_INCREASING_VTBL` pointed all fifteen of these
    // entries at the `ConstId_*` thunks, which is C++ inheritance resolving them to
    // the base class — so there is nothing to `match` on.
    // ------------------------------------------------------------------

    /// `GetSpsIdOffsetList` — `paraset_strategy.cpp:223`.
    ///
    /// Returns a raw pointer because its callers hand it straight to the SPS writers,
    /// which take `*mut i32` from C++.
    #[inline]
    pub fn GetSpsIdOffsetList(&mut self, iParasetType: i32) -> *mut i32 {
        self.m_sParaSetOffset.sParaSetOffsetVariable[iParasetType as usize]
            .iParaSetIdDelta
            .as_mut_ptr()
    }

    /// `GetAllNeededParasetNum` — `paraset_strategy.cpp:227`.
    pub fn GetAllNeededParasetNum(&mut self) -> u32 {
        self.GetNeededSpsNum() + self.GetNeededSubsetSpsNum() + self.GetNeededPpsNum()
    }

    /// `GetNeededSpsNum` — `paraset_strategy.cpp:233`.
    pub fn GetNeededSpsNum(&mut self) -> u32 {
        // C++ tests `0 >= uiNeededSpsNum` on a uint32_t, i.e. exactly "== 0".
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

    /// `GetNeededSubsetSpsNum` — `paraset_strategy.cpp:241`.
    pub fn GetNeededSubsetSpsNum(&mut self) -> u32 {
        if self.m_sParaSetOffset.uiNeededSubsetSpsNum == 0 {
            self.m_sParaSetOffset.uiNeededSubsetSpsNum = if self.m_bSimulcastAVC {
                0
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

    /// `LoadPrevious` — `paraset_strategy.cpp:256`; a no-op. The listing strategies
    /// are the ones that override it, and none of them is ported.
    #[inline]
    pub fn LoadPrevious(
        &mut self,
        _pExistingParasetList: *mut SExistingParasetList,
        _pSpsArray: *mut SWelsSPS,
        _pSubsetArray: *mut SSubsetSps,
        _pPpsArray: *mut SWelsPPS,
    ) {
    }

    /// `UpdatePpsList` — `paraset_strategy.h:114`; empty body.
    #[inline]
    pub fn UpdatePpsList(&mut self, _pCtx: *mut sWelsEncCtx) {}

    /// `CheckParamCompatibility` — `paraset_strategy.h:116`; unconditionally true.
    #[inline]
    pub fn CheckParamCompatibility(
        &mut self,
        _pCodingParam: *mut SWelsSvcCodingParam,
        _pLogCtx: *mut SLogContext,
    ) -> bool {
        true
    }

    /// `GenerateNewSps` — `paraset_strategy.cpp:265`.
    ///
    /// # Safety
    /// `pCtx` must satisfy [`WelsGenerateNewSps`]'s contract.
    pub unsafe fn GenerateNewSps(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        kbUseSubsetSps: bool,
        iDlayerIndex: i32,
        iDlayerCount: i32,
        kuiSpsId: u32,
        pSps: *mut *mut SWelsSPS,
        pSubsetSps: *mut *mut SSubsetSps,
        bSVCBaselayer: bool,
    ) -> u32 {
        WelsGenerateNewSps(
            pCtx,
            kbUseSubsetSps,
            iDlayerIndex,
            iDlayerCount,
            kuiSpsId as i32,
            pSps,
            pSubsetSps,
            bSVCBaselayer,
        );
        kuiSpsId
    }

    /// `InitPps` — `paraset_strategy.cpp:276`.
    ///
    /// Note the literal `true` C++ passes for `kbDeblockingFilterPresentFlag`, ignoring
    /// the argument of the same name.
    ///
    /// # Safety
    /// `pCtx->pPPSArray` must hold at least `kuiPpsId + 1` entries.
    pub unsafe fn InitPps(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        _kiSpsId: u32,
        pSps: *mut SWelsSPS,
        pSubsetSps: *mut SSubsetSps,
        kuiPpsId: u32,
        _kbDeblockingFilterPresentFlag: bool,
        kbUsingSubsetSps: bool,
        kbEntropyCodingModeFlag: bool,
    ) -> u32 {
        WelsInitPps(
            (*pCtx).pPPSArray.add(kuiPpsId as usize),
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

    /// `UpdateParaSetNum` — `paraset_strategy.h:139`; empty.
    #[inline]
    pub fn UpdateParaSetNum(&mut self, _pCtx: *mut sWelsEncCtx) {}

    /// `GetCurrentPpsId` — `paraset_strategy.h:141`.
    #[inline]
    pub fn GetCurrentPpsId(&self, iPpsId: i32, _iIdrLoop: i32) -> i32 {
        iPpsId
    }

    /// `GetSpsIdx` — `paraset_strategy.h:150`.
    #[inline]
    pub fn GetSpsIdx(&self, _iIdx: i32) -> i32 {
        0
    }
}

/// `WelsGenerateNewSps` — `paraset_strategy.cpp:78` (file-static).
///
/// # Safety
/// `pCtx` must have `pSvcParam` set and `pSpsArray`/`pSubsetArray` allocated to at
/// least `kiSpsId + 1` entries.
pub unsafe fn WelsGenerateNewSps(
    pCtx: *mut sWelsEncCtx,
    kbUseSubsetSps: bool,
    iDlayerIndex: i32,
    iDlayerCount: i32,
    kiSpsId: i32,
    pSps: *mut *mut SWelsSPS,
    pSubsetSps: *mut *mut SSubsetSps,
    bSVCBaselayer: bool,
) -> i32 {
    let iRet;

    if !kbUseSubsetSps {
        *pSps = (*pCtx).pSpsArray.add(kiSpsId as usize);
    } else {
        *pSubsetSps = (*pCtx).pSubsetArray.add(kiSpsId as usize);
        *pSps = std::ptr::addr_of_mut!((**pSubsetSps).pSps);
    }

    let pParam = (*pCtx).pSvcParam;
    // S29's named shape. `WelsInitSps` takes `*mut SSpatialLayerConfig`, so the
    // reference here only existed to retag and be cast away — and its retag is
    // what invalidated `InitDqLayers`'s live pointer into the same layer.
    let pDlayerParam = std::ptr::addr_of_mut!((*pParam).sSpatialLayers[iDlayerIndex as usize]);
    // Need port pSps/pPps initialization due to spatial scalability changed
    if !kbUseSubsetSps {
        iRet = WelsInitSps(
            *pSps,
            pDlayerParam,
            std::ptr::addr_of_mut!((*pParam).sDependencyLayers[iDlayerIndex as usize]),
            (*pParam).uiIntraPeriod,
            (*pParam).iMaxNumRefFrame,
            kiSpsId as u32,
            (*pParam).bEnableFrameCroppingFlag,
            (*pParam).iRCMode != RC_OFF_MODE,
            iDlayerCount,
            bSVCBaselayer,
        );
    } else {
        iRet = WelsInitSubsetSps(
            *pSubsetSps,
            pDlayerParam,
            std::ptr::addr_of_mut!((*pParam).sDependencyLayers[iDlayerIndex as usize]),
            (*pParam).uiIntraPeriod,
            (*pParam).iMaxNumRefFrame,
            kiSpsId as u32,
            (*pParam).bEnableFrameCroppingFlag,
            (*pParam).iRCMode != RC_OFF_MODE,
            iDlayerCount,
        );
    }
    iRet
}

/// `ParasetIdAdditionIdAdjust` — `paraset_strategy.cpp:337`.
///
/// Rotates the id actually written to the bitstream, recording the delta from the
/// encoder-side id. `paraset_type = 0: SPS; = 1: PPS`.
///
/// The two `Debug*` helpers (`paraset_strategy.cpp:310`, `:327`) are `#if _DEBUG`
/// bodies; `_DEBUG` is not defined in this build, so they are empty and not ported.
/// `SParaSetOffset::eSpsPpsIdStrategy` is excluded by the same guard, so
/// `Update`'s first statement has no counterpart either.
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

    // update current layer's pCodingParam: for the current parameter set, change its
    // id_delta. C++ computes `uiNextIdInBs - kiEncId` in uint32 and stores it in an
    // int32, so the subtraction wraps rather than saturating.
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

/// The installed parameter-set strategy, borrowed for **one call**.
///
/// Deliberately not cached in a local. Several call sites either pass `pCtx` to a
/// method (`GenerateNewSps`, `InitPps`, `UpdatePpsList`, `UpdateParaSetNum`) or call a
/// function that reaches this same object back through `pCtx->pFuncList`
/// (`WelsWriteOneSPS`, `WelsWriteOnePPS`) — so a `&mut` held across them would alias
/// itself. Re-acquiring is one field read, and it makes the re-entrancy impossible to
/// get wrong rather than merely unlikely. Under the vtable this hazard was invisible:
/// a `*mut` cached in a local aliased freely and said nothing.
///
/// The unbound lifetime is the usual laundering this port does at a raw-pointer
/// boundary; callers keep the reference for the length of one expression.
///
/// # Safety
/// `pCtx` must be a live context whose `pFuncList` is non-null. The strategy must be
/// installed — `InitFunctionPointers` fails the encoder build when it is not, and the
/// call sites that run before that point test the field first. Panics rather than
/// dereferencing null if the invariant is broken; the vtable version was UB there.
#[inline]
pub unsafe fn ParasetStrategy<'a>(
    pCtx: *mut sWelsEncCtx,
) -> &'a mut CWelsParametersetIdStrategyObj {
    (*(*pCtx).pFuncList)
        .pParametersetStrategy
        .as_deref_mut()
        .expect("pParametersetStrategy is installed by InitFunctionPointers")
}

/// `IWelsParametersetStrategy::CreateParametersetStrategy` — `paraset_strategy.cpp:40`.
///
/// **Deviation from C++, deliberate.** C++ builds one of five strategies. Only
/// `CONSTANT_ID` (the Phase-5 gate configuration) and `INCREASING_ID` (the
/// `FillDefault` value) are ported; `SPS_LISTING`, `SPS_LISTING_AND_PPS_INCREASING`
/// and `SPS_PPS_LISTING` return `None` rather than falling through to the constant
/// strategy. C++'s `default:` label *does* fall through to `CONSTANT_ID`, but
/// reproducing that here would silently encode a listing strategy with constant
/// parameter-set ids, giving a decodable stream that does not match the reference. A
/// caller that gets `None` must fail, not continue — `InitFunctionPointers` returns
/// `ENC_RETURN_MEMALLOCERR`, as C++ does when the allocation itself fails.
///
/// The returned `Box` **is** the object's lifetime: dropping it is `WELS_DELETE_OP`.
/// There is no `DestroyParametersetStrategy` any more, which is what closes F19.
pub fn CreateParametersetStrategy(
    eSpsPpsIdStrategy: EParameterSetStrategy,
    bSimulcastAVC: bool,
    kiSpatialLayerNum: i32,
) -> Option<Box<CWelsParametersetIdStrategyObj>> {
    let eIdKind = match eSpsPpsIdStrategy {
        EParameterSetStrategy::CONSTANT_ID => ParasetIdKind::Constant,
        EParameterSetStrategy::INCREASING_ID => ParasetIdKind::Increasing,
        // SPS_LISTING, SPS_LISTING_AND_PPS_INCREASING, SPS_PPS_LISTING
        _ => return None,
    };
    Some(CWelsParametersetIdStrategyObj::new(
        eIdKind,
        bSimulcastAVC,
        kiSpatialLayerNum,
    ))
}

/// `CheckMatchedSps` — `paraset_strategy.cpp:106` (file-static).
///
/// # Safety
/// Both pointers must reference initialised `SWelsSPS` values.
pub unsafe fn CheckMatchedSps(pSps1: *const SWelsSPS, pSps2: *const SWelsSPS) -> bool {
    if (*pSps1).iMbWidth != (*pSps2).iMbWidth || (*pSps1).iMbHeight != (*pSps2).iMbHeight {
        return false;
    }

    if (*pSps1).uiLog2MaxFrameNum != (*pSps2).uiLog2MaxFrameNum
        || (*pSps1).iLog2MaxPocLsb != (*pSps2).iLog2MaxPocLsb
    {
        return false;
    }

    if (*pSps1).iNumRefFrames != (*pSps2).iNumRefFrames {
        return false;
    }

    if (*pSps1).bFrameCroppingFlag != (*pSps2).bFrameCroppingFlag
        || (*pSps1).sFrameCrop.iCropLeft != (*pSps2).sFrameCrop.iCropLeft
        || (*pSps1).sFrameCrop.iCropRight != (*pSps2).sFrameCrop.iCropRight
        || (*pSps1).sFrameCrop.iCropTop != (*pSps2).sFrameCrop.iCropTop
        || (*pSps1).sFrameCrop.iCropBottom != (*pSps2).sFrameCrop.iCropBottom
    {
        return false;
    }

    if (*pSps1).uiProfileIdc != (*pSps2).uiProfileIdc
        || (*pSps1).bConstraintSet0Flag != (*pSps2).bConstraintSet0Flag
        || (*pSps1).bConstraintSet1Flag != (*pSps2).bConstraintSet1Flag
        || (*pSps1).bConstraintSet2Flag != (*pSps2).bConstraintSet2Flag
        || (*pSps1).bConstraintSet3Flag != (*pSps2).bConstraintSet3Flag
        || (*pSps1).iLevelIdc != (*pSps2).iLevelIdc
    {
        return false;
    }

    true
}

/// `CheckMatchedSubsetSps` — `paraset_strategy.cpp:143` (file-static).
///
/// # Safety
/// Both pointers must reference initialised `SSubsetSps` values.
pub unsafe fn CheckMatchedSubsetSps(
    pSubsetSps1: *const SSubsetSps,
    pSubsetSps2: *const SSubsetSps,
) -> bool {
    if !CheckMatchedSps(&(*pSubsetSps1).pSps, &(*pSubsetSps2).pSps) {
        return false;
    }

    if (*pSubsetSps1).sSpsSvcExt.iExtendedSpatialScalability
        != (*pSubsetSps2).sSpsSvcExt.iExtendedSpatialScalability
        || (*pSubsetSps1).sSpsSvcExt.bAdaptiveTcoeffLevelPredFlag
            != (*pSubsetSps2).sSpsSvcExt.bAdaptiveTcoeffLevelPredFlag
        || (*pSubsetSps1).sSpsSvcExt.bSeqTcoeffLevelPredFlag
            != (*pSubsetSps2).sSpsSvcExt.bSeqTcoeffLevelPredFlag
        || (*pSubsetSps1).sSpsSvcExt.bSliceHeaderRestrictionFlag
            != (*pSubsetSps2).sSpsSvcExt.bSliceHeaderRestrictionFlag
    {
        return false;
    }

    true
}

/// `FindExistingSps` — `paraset_strategy.cpp:169`.
///
/// Returns the index of a stored parameter set matching the current configuration, or
/// [`INVALID_ID`].
///
/// # Safety
/// `pParam` must be initialised; `pSpsArray`/`pSubsetArray` must hold at least
/// `iSpsNumInUse` entries.
pub unsafe fn FindExistingSps(
    pParam: *mut SWelsSvcCodingParam,
    kbUseSubsetSps: bool,
    iDlayerIndex: i32,
    iDlayerCount: i32,
    iSpsNumInUse: i32,
    pSpsArray: *mut SWelsSPS,
    pSubsetArray: *mut SSubsetSps,
    bSVCBaseLayer: bool,
) -> i32 {
    // S29's named shape. `WelsInitSps` takes `*mut SSpatialLayerConfig`, so the
    // reference here only existed to retag and be cast away — and its retag is
    // what invalidated `InitDqLayers`'s live pointer into the same layer.
    let pDlayerParam = std::ptr::addr_of_mut!((*pParam).sSpatialLayers[iDlayerIndex as usize]);

    if !kbUseSubsetSps {
        let mut sTmpSps = SWelsSPS::default();
        WelsInitSps(
            &mut sTmpSps,
            pDlayerParam,
            std::ptr::addr_of_mut!((*pParam).sDependencyLayers[iDlayerIndex as usize]),
            (*pParam).uiIntraPeriod,
            (*pParam).iMaxNumRefFrame,
            0,
            (*pParam).bEnableFrameCroppingFlag,
            (*pParam).iRCMode != RC_OFF_MODE,
            iDlayerCount,
            bSVCBaseLayer,
        );
        for iId in 0..iSpsNumInUse {
            if CheckMatchedSps(&sTmpSps, pSpsArray.add(iId as usize)) {
                return iId;
            }
        }
    } else {
        let mut sTmpSubsetSps = SSubsetSps::default();
        WelsInitSubsetSps(
            &mut sTmpSubsetSps,
            pDlayerParam,
            std::ptr::addr_of_mut!((*pParam).sDependencyLayers[iDlayerIndex as usize]),
            (*pParam).uiIntraPeriod,
            (*pParam).iMaxNumRefFrame,
            0,
            (*pParam).bEnableFrameCroppingFlag,
            (*pParam).iRCMode != RC_OFF_MODE,
            iDlayerCount,
        );

        for iId in 0..iSpsNumInUse {
            if CheckMatchedSubsetSps(&sTmpSubsetSps, pSubsetArray.add(iId as usize)) {
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

    /// The counts are inherited, not overridden: `ID_INCREASING_VTBL` pointed them at
    /// the `ConstId_*` thunks, so the merged object must answer identically for both
    /// kinds. This is the test that would catch a `match` added where C++ has none.
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

    /// The three unported listing strategies must fail loudly rather than silently
    /// behave like `CONSTANT_ID`.
    #[test]
    fn unported_strategies_return_none() {
        for e in [
            EParameterSetStrategy::SPS_LISTING,
            EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING,
            EParameterSetStrategy::SPS_PPS_LISTING,
        ] {
            assert!(CreateParametersetStrategy(e, false, 1).is_none(), "{e:?}");
        }
    }

    /// `ParasetIdAdditionIdAdjust` rotates the id written to the bitstream and records
    /// the delta back to the encoder-side id (`paraset_strategy.cpp:337`). Walking a
    /// single encoder id 0 through repeated `Update` calls should produce deltas
    /// 0, 1, 2, … up to `MAX_SPS_COUNT - 1`, then wrap to 0.
    #[test]
    fn increasing_strategy_rotates_sps_id_in_bitstream() {
        let mut p = strategy(EParameterSetStrategy::INCREASING_ID);
        for expected in 0..MAX_SPS_COUNT as i32 {
            p.Update(0, PARA_SET_TYPE_AVCSPS as i32);
            assert_eq!(p.GetSpsIdOffset(0, 0), expected, "delta after update #{expected}");
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

    /// `Update` is one of the five that `match`: the constant kind resets the whole
    /// offset block where the increasing kind rotates. Pinning it stops the two arms
    /// from being collapsed by someone reading only the constant one.
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
