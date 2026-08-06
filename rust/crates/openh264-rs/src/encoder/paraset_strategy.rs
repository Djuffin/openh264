//! Port of `codec/encoder/core/src/paraset_strategy.cpp` and
//! `codec/encoder/core/inc/paraset_strategy.h`.
//!
//! **Partial by design.** C++ declares one abstract `IWelsParametersetStrategy` and
//! five concrete strategies (`CONSTANT_ID`, `INCREASING_ID`, `SPS_LISTING`,
//! `SPS_LISTING_AND_PPS_INCREASING`, `SPS_PPS_LISTING`). Only
//! `CWelsParametersetIdConstant` — the `CONSTANT_ID` strategy the Phase-5 gate
//! configuration uses — is ported here. `CreateParametersetStrategy` returns an
//! explicit error for the other four rather than silently substituting the constant
//! strategy, which would produce a stream that decodes but does not match C++.
//!
//! ### Why a C-style vtable
//!
//! `sWelsEncCtx` and `SWelsFuncPtrList` both store this as a plain 8-byte
//! `IWelsParametersetStrategy*`. A Rust `*mut dyn Trait` is a 16-byte fat pointer and
//! would mis-size both structs — the exact defect Phase 2 found in
//! `ref_list_mgr_svc.rs`'s `IWelsReferenceStrategy`. So the interface is modelled the
//! way C++ lays it out: a thin pointer to an object whose first word is a pointer to a
//! static vtable. `IWelsParametersetStrategyVtbl` lists the methods in the same order
//! `paraset_strategy.h:50-93` declares them, so the two can be read side by side.
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

/// Virtual-function table for `IWelsParametersetStrategy`
/// (`paraset_strategy.h:50`). Entries are in C++ declaration order.
///
/// Every entry takes the object pointer as its first argument, standing in for the
/// implicit `this`.
#[repr(C)]
pub struct IWelsParametersetStrategyVtbl {
    pub Destroy: unsafe extern "C" fn(*mut IWelsParametersetStrategy),
    pub GetPpsIdOffset: unsafe extern "C" fn(*mut IWelsParametersetStrategy, i32) -> i32,
    pub GetSpsIdOffset: unsafe extern "C" fn(*mut IWelsParametersetStrategy, i32, i32) -> i32,
    pub GetSpsIdOffsetList: unsafe extern "C" fn(*mut IWelsParametersetStrategy, i32) -> *mut i32,
    pub GetAllNeededParasetNum: unsafe extern "C" fn(*mut IWelsParametersetStrategy) -> u32,
    pub GetNeededSpsNum: unsafe extern "C" fn(*mut IWelsParametersetStrategy) -> u32,
    pub GetNeededSubsetSpsNum: unsafe extern "C" fn(*mut IWelsParametersetStrategy) -> u32,
    pub GetNeededPpsNum: unsafe extern "C" fn(*mut IWelsParametersetStrategy) -> u32,
    pub LoadPrevious: unsafe extern "C" fn(
        *mut IWelsParametersetStrategy,
        *mut SExistingParasetList,
        *mut SWelsSPS,
        *mut SSubsetSps,
        *mut SWelsPPS,
    ),
    pub Update: unsafe extern "C" fn(*mut IWelsParametersetStrategy, u32, i32),
    pub UpdatePpsList: unsafe extern "C" fn(*mut IWelsParametersetStrategy, *mut sWelsEncCtx),
    pub CheckParamCompatibility: unsafe extern "C" fn(
        *mut IWelsParametersetStrategy,
        *mut SWelsSvcCodingParam,
        *mut SLogContext,
    ) -> bool,
    pub GenerateNewSps: unsafe extern "C" fn(
        *mut IWelsParametersetStrategy,
        *mut sWelsEncCtx,
        bool,
        i32,
        i32,
        u32,
        *mut *mut SWelsSPS,
        *mut *mut SSubsetSps,
        bool,
    ) -> u32,
    pub InitPps: unsafe extern "C" fn(
        *mut IWelsParametersetStrategy,
        *mut sWelsEncCtx,
        u32,
        *mut SWelsSPS,
        *mut SSubsetSps,
        u32,
        bool,
        bool,
        bool,
    ) -> u32,
    pub SetUseSubsetFlag: unsafe extern "C" fn(*mut IWelsParametersetStrategy, u32, bool),
    pub UpdateParaSetNum: unsafe extern "C" fn(*mut IWelsParametersetStrategy, *mut sWelsEncCtx),
    pub GetCurrentPpsId: unsafe extern "C" fn(*mut IWelsParametersetStrategy, i32, i32) -> i32,
    pub OutputCurrentStructure: unsafe extern "C" fn(
        *mut IWelsParametersetStrategy,
        *mut SParaSetOffsetVariable,
        *mut i32,
        *mut sWelsEncCtx,
        *mut SExistingParasetList,
    ),
    pub LoadPreviousStructure:
        unsafe extern "C" fn(*mut IWelsParametersetStrategy, *mut SParaSetOffsetVariable, *mut i32),
    pub GetSpsIdx: unsafe extern "C" fn(*mut IWelsParametersetStrategy, i32) -> i32,
}

/// `IWelsParametersetStrategy` — the abstract base. One pointer wide, matching the
/// vptr-only layout C++ gives a class with no data members.
#[repr(C)]
pub struct IWelsParametersetStrategy {
    pub pVtbl: *const IWelsParametersetStrategyVtbl,
}

impl IWelsParametersetStrategy {
    /// `pParametersetStrategy->GetPpsIdOffset (iPpsId)`.
    ///
    /// # Safety
    /// `pThis` must be a live strategy object built by [`CreateParametersetStrategy`].
    pub unsafe fn GetPpsIdOffset(pThis: *mut IWelsParametersetStrategy, iPpsId: i32) -> i32 {
        ((*(*pThis).pVtbl).GetPpsIdOffset)(pThis, iPpsId)
    }

    /// `pParametersetStrategy->GetSpsIdOffset (iPpsId, iSpsId)`.
    ///
    /// # Safety
    /// `pThis` must be a live strategy object built by [`CreateParametersetStrategy`].
    pub unsafe fn GetSpsIdOffset(
        pThis: *mut IWelsParametersetStrategy,
        iPpsId: i32,
        iSpsId: i32,
    ) -> i32 {
        ((*(*pThis).pVtbl).GetSpsIdOffset)(pThis, iPpsId, iSpsId)
    }

    /// `pParametersetStrategy->GetSpsIdOffsetList (iParasetType)`.
    ///
    /// # Safety
    /// `pThis` must be a live strategy object built by [`CreateParametersetStrategy`].
    pub unsafe fn GetSpsIdOffsetList(
        pThis: *mut IWelsParametersetStrategy,
        iParasetType: i32,
    ) -> *mut i32 {
        ((*(*pThis).pVtbl).GetSpsIdOffsetList)(pThis, iParasetType)
    }
}

/// `CWelsParametersetIdConstant` — `paraset_strategy.h:96`.
///
/// Layout mirrors the C++ object: vptr first (as the embedded base), then
/// `m_sParaSetOffset`, `m_bSimulcastAVC`, `m_iSpatialLayerNum`,
/// `m_iBasicNeededSpsNum`, `m_iBasicNeededPpsNum`.
#[repr(C)]
pub struct CWelsParametersetIdConstant {
    pub base: IWelsParametersetStrategy,
    pub m_sParaSetOffset: SParaSetOffset,
    pub m_bSimulcastAVC: bool,
    pub m_iSpatialLayerNum: i32,
    pub m_iBasicNeededSpsNum: u32,
    pub m_iBasicNeededPpsNum: u32,
}

/// `CWelsParametersetIdConstant::CWelsParametersetIdConstant` —
/// `paraset_strategy.cpp:203`.
impl CWelsParametersetIdConstant {
    pub fn new(bSimulcastAVC: bool, kiSpatialLayerNum: i32) -> Box<Self> {
        Box::new(Self {
            base: IWelsParametersetStrategy {
                pVtbl: &ID_CONSTANT_VTBL,
            },
            // C++ memsets m_sParaSetOffset to 0.
            m_sParaSetOffset: SParaSetOffset::default(),
            m_bSimulcastAVC: bSimulcastAVC,
            m_iSpatialLayerNum: kiSpatialLayerNum,
            m_iBasicNeededSpsNum: 1,
            m_iBasicNeededPpsNum: (1 + kiSpatialLayerNum) as u32,
        })
    }
}

/// Recovers the concrete type from the interface pointer. Sound only because
/// `CWelsParametersetIdConstant` is `#[repr(C)]` with `base` as its first field.
#[inline]
unsafe fn as_const_id(pThis: *mut IWelsParametersetStrategy) -> *mut CWelsParametersetIdConstant {
    pThis as *mut CWelsParametersetIdConstant
}

unsafe extern "C" fn ConstId_Destroy(pThis: *mut IWelsParametersetStrategy) {
    // Mirrors `WELS_DELETE_OP`; the allocation came from `Box::new` in `new`.
    drop(Box::from_raw(as_const_id(pThis)));
}

/// `CWelsParametersetIdConstant::GetPpsIdOffset` — `paraset_strategy.cpp:216`.
unsafe extern "C" fn ConstId_GetPpsIdOffset(
    _pThis: *mut IWelsParametersetStrategy,
    _iPpsId: i32,
) -> i32 {
    0
}

/// `CWelsParametersetIdConstant::GetSpsIdOffset` — `paraset_strategy.cpp:219`.
unsafe extern "C" fn ConstId_GetSpsIdOffset(
    _pThis: *mut IWelsParametersetStrategy,
    _iPpsId: i32,
    _iSpsId: i32,
) -> i32 {
    0
}

/// `CWelsParametersetIdConstant::GetSpsIdOffsetList` — `paraset_strategy.cpp:223`.
unsafe extern "C" fn ConstId_GetSpsIdOffsetList(
    pThis: *mut IWelsParametersetStrategy,
    iParasetType: i32,
) -> *mut i32 {
    let p = as_const_id(pThis);
    (*p).m_sParaSetOffset.sParaSetOffsetVariable[iParasetType as usize]
        .iParaSetIdDelta
        .as_mut_ptr()
}

/// `CWelsParametersetIdConstant::GetAllNeededParasetNum` — `paraset_strategy.cpp:227`.
unsafe extern "C" fn ConstId_GetAllNeededParasetNum(pThis: *mut IWelsParametersetStrategy) -> u32 {
    ConstId_GetNeededSpsNum(pThis)
        + ConstId_GetNeededSubsetSpsNum(pThis)
        + ConstId_GetNeededPpsNum(pThis)
}

/// `CWelsParametersetIdConstant::GetNeededSpsNum` — `paraset_strategy.cpp:233`.
unsafe extern "C" fn ConstId_GetNeededSpsNum(pThis: *mut IWelsParametersetStrategy) -> u32 {
    let p = as_const_id(pThis);
    // C++ tests `0 >= uiNeededSpsNum` on a uint32_t, i.e. exactly "== 0".
    if (*p).m_sParaSetOffset.uiNeededSpsNum == 0 {
        (*p).m_sParaSetOffset.uiNeededSpsNum = (*p).m_iBasicNeededSpsNum
            * if (*p).m_bSimulcastAVC {
                (*p).m_iSpatialLayerNum as u32
            } else {
                1
            };
    }
    (*p).m_sParaSetOffset.uiNeededSpsNum
}

/// `CWelsParametersetIdConstant::GetNeededSubsetSpsNum` — `paraset_strategy.cpp:241`.
unsafe extern "C" fn ConstId_GetNeededSubsetSpsNum(pThis: *mut IWelsParametersetStrategy) -> u32 {
    let p = as_const_id(pThis);
    if (*p).m_sParaSetOffset.uiNeededSubsetSpsNum == 0 {
        (*p).m_sParaSetOffset.uiNeededSubsetSpsNum = if (*p).m_bSimulcastAVC {
            0
        } else {
            ((*p).m_iSpatialLayerNum - 1) as u32
        };
    }
    (*p).m_sParaSetOffset.uiNeededSubsetSpsNum
}

/// `CWelsParametersetIdConstant::GetNeededPpsNum` — `paraset_strategy.cpp:248`.
unsafe extern "C" fn ConstId_GetNeededPpsNum(pThis: *mut IWelsParametersetStrategy) -> u32 {
    let p = as_const_id(pThis);
    if (*p).m_sParaSetOffset.uiNeededPpsNum == 0 {
        (*p).m_sParaSetOffset.uiNeededPpsNum = (*p).m_iBasicNeededPpsNum
            * if (*p).m_bSimulcastAVC {
                (*p).m_iSpatialLayerNum as u32
            } else {
                1
            };
    }
    (*p).m_sParaSetOffset.uiNeededPpsNum
}

/// `CWelsParametersetIdConstant::LoadPrevious` — `paraset_strategy.cpp:256`; a no-op.
unsafe extern "C" fn ConstId_LoadPrevious(
    _pThis: *mut IWelsParametersetStrategy,
    _pExistingParasetList: *mut SExistingParasetList,
    _pSpsArray: *mut SWelsSPS,
    _pSubsetArray: *mut SSubsetSps,
    _pPpsArray: *mut SWelsPPS,
) {
}

/// `CWelsParametersetIdConstant::Update` — `paraset_strategy.cpp:261`.
unsafe extern "C" fn ConstId_Update(
    pThis: *mut IWelsParametersetStrategy,
    _kuiId: u32,
    _iParasetType: i32,
) {
    (*as_const_id(pThis)).m_sParaSetOffset = SParaSetOffset::default();
}

/// `CWelsParametersetIdConstant::UpdatePpsList` — `paraset_strategy.h:114`; empty body.
unsafe extern "C" fn ConstId_UpdatePpsList(
    _pThis: *mut IWelsParametersetStrategy,
    _pCtx: *mut sWelsEncCtx,
) {
}

/// `CWelsParametersetIdConstant::CheckParamCompatibility` — `paraset_strategy.h:116`;
/// unconditionally true.
unsafe extern "C" fn ConstId_CheckParamCompatibility(
    _pThis: *mut IWelsParametersetStrategy,
    _pCodingParam: *mut SWelsSvcCodingParam,
    _pLogCtx: *mut SLogContext,
) -> bool {
    true
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
        *pSps = &mut (**pSubsetSps).pSps;
    }

    let pParam = (*pCtx).pSvcParam;
    let pDlayerParam = &mut (*pParam).sSpatialLayers[iDlayerIndex as usize] as *mut _;
    // Need port pSps/pPps initialization due to spatial scalability changed
    if !kbUseSubsetSps {
        iRet = WelsInitSps(
            *pSps,
            pDlayerParam,
            &mut (*pParam).sDependencyLayers[iDlayerIndex as usize],
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
            &mut (*pParam).sDependencyLayers[iDlayerIndex as usize],
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

/// `CWelsParametersetIdConstant::GenerateNewSps` — `paraset_strategy.cpp:265`.
unsafe extern "C" fn ConstId_GenerateNewSps(
    _pThis: *mut IWelsParametersetStrategy,
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

/// `CWelsParametersetIdConstant::InitPps` — `paraset_strategy.cpp:276`.
///
/// Note the literal `true` C++ passes for `kbDeblockingFilterPresentFlag`, ignoring
/// the argument of the same name.
unsafe extern "C" fn ConstId_InitPps(
    pThis: *mut IWelsParametersetStrategy,
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
    ConstId_SetUseSubsetFlag(pThis, kuiPpsId, kbUsingSubsetSps);
    kuiPpsId
}

/// `CWelsParametersetIdConstant::SetUseSubsetFlag` — `paraset_strategy.cpp:288`.
unsafe extern "C" fn ConstId_SetUseSubsetFlag(
    pThis: *mut IWelsParametersetStrategy,
    iPpsId: u32,
    bUseSubsetSps: bool,
) {
    (*as_const_id(pThis))
        .m_sParaSetOffset
        .bPpsIdMappingIntoSubsetsps[iPpsId as usize] = bUseSubsetSps;
}

/// `CWelsParametersetIdConstant::UpdateParaSetNum` — `paraset_strategy.h:139`; empty.
unsafe extern "C" fn ConstId_UpdateParaSetNum(
    _pThis: *mut IWelsParametersetStrategy,
    _pCtx: *mut sWelsEncCtx,
) {
}

/// `CWelsParametersetIdConstant::GetCurrentPpsId` — `paraset_strategy.h:141`.
unsafe extern "C" fn ConstId_GetCurrentPpsId(
    _pThis: *mut IWelsParametersetStrategy,
    iPpsId: i32,
    _iIdrLoop: i32,
) -> i32 {
    iPpsId
}

/// `CWelsParametersetIdConstant::OutputCurrentStructure` — `paraset_strategy.h:145`;
/// empty.
unsafe extern "C" fn ConstId_OutputCurrentStructure(
    _pThis: *mut IWelsParametersetStrategy,
    _pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
    _pPpsIdList: *mut i32,
    _pCtx: *mut sWelsEncCtx,
    _pExistingParasetList: *mut SExistingParasetList,
) {
}

/// `CWelsParametersetIdConstant::LoadPreviousStructure` — `paraset_strategy.h:148`;
/// empty.
unsafe extern "C" fn ConstId_LoadPreviousStructure(
    _pThis: *mut IWelsParametersetStrategy,
    _pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
    _pPpsIdList: *mut i32,
) {
}

/// `CWelsParametersetIdConstant::GetSpsIdx` — `paraset_strategy.h:150`.
unsafe extern "C" fn ConstId_GetSpsIdx(_pThis: *mut IWelsParametersetStrategy, _iIdx: i32) -> i32 {
    0
}

/// The single static vtable shared by every `CWelsParametersetIdConstant` instance,
/// as C++ shares one vtable per class.
pub static ID_CONSTANT_VTBL: IWelsParametersetStrategyVtbl = IWelsParametersetStrategyVtbl {
    Destroy: ConstId_Destroy,
    GetPpsIdOffset: ConstId_GetPpsIdOffset,
    GetSpsIdOffset: ConstId_GetSpsIdOffset,
    GetSpsIdOffsetList: ConstId_GetSpsIdOffsetList,
    GetAllNeededParasetNum: ConstId_GetAllNeededParasetNum,
    GetNeededSpsNum: ConstId_GetNeededSpsNum,
    GetNeededSubsetSpsNum: ConstId_GetNeededSubsetSpsNum,
    GetNeededPpsNum: ConstId_GetNeededPpsNum,
    LoadPrevious: ConstId_LoadPrevious,
    Update: ConstId_Update,
    UpdatePpsList: ConstId_UpdatePpsList,
    CheckParamCompatibility: ConstId_CheckParamCompatibility,
    GenerateNewSps: ConstId_GenerateNewSps,
    InitPps: ConstId_InitPps,
    SetUseSubsetFlag: ConstId_SetUseSubsetFlag,
    UpdateParaSetNum: ConstId_UpdateParaSetNum,
    GetCurrentPpsId: ConstId_GetCurrentPpsId,
    OutputCurrentStructure: ConstId_OutputCurrentStructure,
    LoadPreviousStructure: ConstId_LoadPreviousStructure,
    GetSpsIdx: ConstId_GetSpsIdx,
};

// ============================================================================
// CWelsParametersetIdNonConstant / CWelsParametersetIdIncreasing
//
// C++ layers three classes here: CWelsParametersetIdNonConstant overrides
// OutputCurrentStructure and LoadPreviousStructure, and CWelsParametersetIdIncreasing
// adds GetPpsIdOffset, GetSpsIdOffset and Update on top. Rust has no implementation
// inheritance, so the vtable below reuses the `ConstId_*` thunks verbatim for the
// members that are not overridden — which is precisely what the C++ vtable does.
//
// The two `Debug*` helpers (`paraset_strategy.cpp:310`, `:327`) are `#if _DEBUG`
// bodies; `_DEBUG` is not defined in this build, so they are empty and not ported.
// `SParaSetOffset::eSpsPpsIdStrategy` is excluded by the same guard, so
// `Update`'s first statement has no counterpart either.
// ============================================================================

/// `CWelsParametersetIdIncreasing` — `paraset_strategy.h:208`. Same data layout as
/// `CWelsParametersetIdConstant`; only the vtable differs.
pub type CWelsParametersetIdIncreasing = CWelsParametersetIdConstant;

/// `ParasetIdAdditionIdAdjust` — `paraset_strategy.cpp:337`.
///
/// Rotates the id actually written to the bitstream, recording the delta from the
/// encoder-side id. `paraset_type = 0: SPS; = 1: PPS`.
///
/// # Safety
/// `sParaSetOffsetVariable` must be non-null; `kiCurEncoderParaSetId` must index
/// `iParaSetIdDelta` and `uiNextParaSetIdToUseInBs` must index `bUsedParaSetIdInBs`.
pub unsafe fn ParasetIdAdditionIdAdjust(
    sParaSetOffsetVariable: *mut SParaSetOffsetVariable,
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
    let mut uiNextIdInBs = (*sParaSetOffsetVariable).uiNextParaSetIdToUseInBs;

    // update current layer's pCodingParam: for the current parameter set, change its
    // id_delta. C++ computes `uiNextIdInBs - kiEncId` in uint32 and stores it in an
    // int32, so the subtraction wraps rather than saturating.
    (*sParaSetOffsetVariable).iParaSetIdDelta[kiEncId as usize] =
        uiNextIdInBs.wrapping_sub(kiEncId as u32) as i32;
    // write pso data for the next update: mark the used id
    (*sParaSetOffsetVariable).bUsedParaSetIdInBs[uiNextIdInBs as usize] = true;

    // prepare for the next update: find the next available id
    uiNextIdInBs += 1;
    if uiNextIdInBs >= kuiMaxIdInBs {
        uiNextIdInBs = 0; // ensure the SPS_ID would not exceed MAX_SPS_COUNT
    }
    (*sParaSetOffsetVariable).uiNextParaSetIdToUseInBs = uiNextIdInBs;
}

/// `CWelsParametersetIdIncreasing::Update` — `paraset_strategy.cpp:370`.
unsafe extern "C" fn IncId_Update(
    pThis: *mut IWelsParametersetStrategy,
    kuiId: u32,
    iParasetType: i32,
) {
    let p = as_const_id(pThis);
    ParasetIdAdditionIdAdjust(
        &mut (*p).m_sParaSetOffset.sParaSetOffsetVariable[iParasetType as usize],
        kuiId as i32,
        if iParasetType != PARA_SET_TYPE_PPS as i32 {
            MAX_SPS_COUNT as u32
        } else {
            MAX_PPS_COUNT as u32
        },
    );
}

/// `CWelsParametersetIdIncreasing::GetPpsIdOffset` — `paraset_strategy.cpp:384`.
unsafe extern "C" fn IncId_GetPpsIdOffset(
    pThis: *mut IWelsParametersetStrategy,
    kiPpsId: i32,
) -> i32 {
    let p = as_const_id(pThis);
    (*p).m_sParaSetOffset.sParaSetOffsetVariable[PARA_SET_TYPE_PPS].iParaSetIdDelta
        [kiPpsId as usize]
}

/// `CWelsParametersetIdIncreasing::GetSpsIdOffset` — `paraset_strategy.cpp:391`.
unsafe extern "C" fn IncId_GetSpsIdOffset(
    pThis: *mut IWelsParametersetStrategy,
    kiPpsId: i32,
    kiSpsId: i32,
) -> i32 {
    let p = as_const_id(pThis);
    let kiParameterSetType = if (*p).m_sParaSetOffset.bPpsIdMappingIntoSubsetsps[kiPpsId as usize] {
        PARA_SET_TYPE_SUBSETSPS
    } else {
        PARA_SET_TYPE_AVCSPS
    };
    (*p).m_sParaSetOffset.sParaSetOffsetVariable[kiParameterSetType].iParaSetIdDelta
        [kiSpsId as usize]
}

/// `CWelsParametersetIdNonConstant::OutputCurrentStructure` —
/// `paraset_strategy.cpp:292`. `pPpsIdList`, `pCtx` and `pExistingParasetList` are
/// accepted and unused, as in C++.
unsafe extern "C" fn NonConstId_OutputCurrentStructure(
    pThis: *mut IWelsParametersetStrategy,
    pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
    _pPpsIdList: *mut i32,
    _pCtx: *mut sWelsEncCtx,
    _pExistingParasetList: *mut SExistingParasetList,
) {
    let p = as_const_id(pThis);
    for k in 0..PARA_SET_TYPE {
        (*p).m_sParaSetOffset.sParaSetOffsetVariable[k].bUsedParaSetIdInBs = [false; MAX_PPS_COUNT];
    }
    std::ptr::copy_nonoverlapping(
        (*p).m_sParaSetOffset.sParaSetOffsetVariable.as_ptr(),
        pParaSetOffsetVariable,
        PARA_SET_TYPE,
    );
}

/// `CWelsParametersetIdNonConstant::LoadPreviousStructure` —
/// `paraset_strategy.cpp:300`.
unsafe extern "C" fn NonConstId_LoadPreviousStructure(
    pThis: *mut IWelsParametersetStrategy,
    pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
    _pPpsIdList: *mut i32,
) {
    let p = as_const_id(pThis);
    std::ptr::copy_nonoverlapping(
        pParaSetOffsetVariable as *const SParaSetOffsetVariable,
        (*p).m_sParaSetOffset.sParaSetOffsetVariable.as_mut_ptr(),
        PARA_SET_TYPE,
    );
}

/// Vtable for `CWelsParametersetIdIncreasing`. Entries not overridden by the
/// `NonConstant`/`Increasing` subclasses point at the `CWelsParametersetIdConstant`
/// implementation, exactly as C++ inheritance resolves them.
pub static ID_INCREASING_VTBL: IWelsParametersetStrategyVtbl = IWelsParametersetStrategyVtbl {
    Destroy: ConstId_Destroy,
    GetPpsIdOffset: IncId_GetPpsIdOffset,
    GetSpsIdOffset: IncId_GetSpsIdOffset,
    GetSpsIdOffsetList: ConstId_GetSpsIdOffsetList,
    GetAllNeededParasetNum: ConstId_GetAllNeededParasetNum,
    GetNeededSpsNum: ConstId_GetNeededSpsNum,
    GetNeededSubsetSpsNum: ConstId_GetNeededSubsetSpsNum,
    GetNeededPpsNum: ConstId_GetNeededPpsNum,
    LoadPrevious: ConstId_LoadPrevious,
    Update: IncId_Update,
    UpdatePpsList: ConstId_UpdatePpsList,
    CheckParamCompatibility: ConstId_CheckParamCompatibility,
    GenerateNewSps: ConstId_GenerateNewSps,
    InitPps: ConstId_InitPps,
    SetUseSubsetFlag: ConstId_SetUseSubsetFlag,
    UpdateParaSetNum: ConstId_UpdateParaSetNum,
    GetCurrentPpsId: ConstId_GetCurrentPpsId,
    OutputCurrentStructure: NonConstId_OutputCurrentStructure,
    LoadPreviousStructure: NonConstId_LoadPreviousStructure,
    GetSpsIdx: ConstId_GetSpsIdx,
};

/// `IWelsParametersetStrategy::CreateParametersetStrategy` — `paraset_strategy.cpp:40`.
///
/// Returns a raw pointer the caller owns; release it with
/// [`DestroyParametersetStrategy`].
///
/// **Deviation from C++, deliberate.** C++ builds one of five strategies. Only
/// `CONSTANT_ID` (the Phase-5 gate configuration) and `INCREASING_ID` (the
/// `FillDefault` value) are ported; `SPS_LISTING`, `SPS_LISTING_AND_PPS_INCREASING`
/// and `SPS_PPS_LISTING` return null rather than falling through to the constant
/// strategy. C++'s `default:` label *does* fall through to `CONSTANT_ID`, but
/// reproducing that here would silently encode a listing strategy with constant
/// parameter-set ids, giving a decodable stream that does not match the reference. A
/// caller that gets null must fail, not continue — `InitFunctionPointers` returns
/// `ENC_RETURN_MEMALLOCERR`, as C++ does when the allocation itself fails.
pub fn CreateParametersetStrategy(
    eSpsPpsIdStrategy: EParameterSetStrategy,
    bSimulcastAVC: bool,
    kiSpatialLayerNum: i32,
) -> *mut IWelsParametersetStrategy {
    match eSpsPpsIdStrategy {
        EParameterSetStrategy::CONSTANT_ID => {
            Box::into_raw(CWelsParametersetIdConstant::new(bSimulcastAVC, kiSpatialLayerNum))
                as *mut IWelsParametersetStrategy
        }
        EParameterSetStrategy::INCREASING_ID => {
            let mut p = CWelsParametersetIdIncreasing::new(bSimulcastAVC, kiSpatialLayerNum);
            p.base.pVtbl = &ID_INCREASING_VTBL;
            Box::into_raw(p) as *mut IWelsParametersetStrategy
        }
        // SPS_LISTING, SPS_LISTING_AND_PPS_INCREASING, SPS_PPS_LISTING
        _ => null_mut(),
    }
}

/// Counterpart to [`CreateParametersetStrategy`]; dispatches through the vtable so the
/// right concrete destructor runs.
///
/// # Safety
/// `pStrategy` must have come from [`CreateParametersetStrategy`] and must not be used
/// afterwards.
pub unsafe fn DestroyParametersetStrategy(pStrategy: *mut IWelsParametersetStrategy) {
    if !pStrategy.is_null() {
        ((*(*pStrategy).pVtbl).Destroy)(pStrategy);
    }
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
    let pDlayerParam = &mut (*pParam).sSpatialLayers[iDlayerIndex as usize] as *mut _;

    if !kbUseSubsetSps {
        let mut sTmpSps = SWelsSPS::default();
        WelsInitSps(
            &mut sTmpSps,
            pDlayerParam,
            &mut (*pParam).sDependencyLayers[iDlayerIndex as usize],
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
            &mut (*pParam).sDependencyLayers[iDlayerIndex as usize],
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

    #[test]
    fn constant_strategy_reports_zero_id_offsets() {
        let p = CreateParametersetStrategy(EParameterSetStrategy::CONSTANT_ID, false, 1);
        assert!(!p.is_null());
        unsafe {
            assert_eq!(IWelsParametersetStrategy::GetPpsIdOffset(p, 0), 0);
            assert_eq!(IWelsParametersetStrategy::GetSpsIdOffset(p, 0, 0), 0);
            DestroyParametersetStrategy(p);
        }
    }

    /// `m_iBasicNeededSpsNum` is 1 and `m_iBasicNeededPpsNum` is `1 + layers`;
    /// without simulcast AVC neither is scaled by the layer count and the subset-SPS
    /// count is `layers - 1` (`paraset_strategy.cpp:233-254`).
    #[test]
    fn constant_strategy_paraset_counts() {
        let p = CreateParametersetStrategy(EParameterSetStrategy::CONSTANT_ID, false, 1);
        unsafe {
            assert_eq!(((*(*p).pVtbl).GetNeededSpsNum)(p), 1);
            assert_eq!(((*(*p).pVtbl).GetNeededSubsetSpsNum)(p), 0);
            assert_eq!(((*(*p).pVtbl).GetNeededPpsNum)(p), 2);
            assert_eq!(((*(*p).pVtbl).GetAllNeededParasetNum)(p), 3);
            DestroyParametersetStrategy(p);
        }
    }

    /// The three unported listing strategies must fail loudly rather than silently
    /// behave like `CONSTANT_ID`.
    #[test]
    fn unported_strategies_return_null() {
        assert!(CreateParametersetStrategy(EParameterSetStrategy::SPS_LISTING, false, 1).is_null());
        assert!(CreateParametersetStrategy(
            EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING,
            false,
            1
        )
        .is_null());
        assert!(
            CreateParametersetStrategy(EParameterSetStrategy::SPS_PPS_LISTING, false, 1).is_null()
        );
    }

    /// `ParasetIdAdditionIdAdjust` rotates the id written to the bitstream and records
    /// the delta back to the encoder-side id (`paraset_strategy.cpp:337`). Walking a
    /// single encoder id 0 through repeated `Update` calls should produce deltas
    /// 0, 1, 2, … up to `MAX_SPS_COUNT - 1`, then wrap to 0.
    #[test]
    fn increasing_strategy_rotates_sps_id_in_bitstream() {
        let p = CreateParametersetStrategy(EParameterSetStrategy::INCREASING_ID, false, 1);
        assert!(!p.is_null());
        unsafe {
            for expected in 0..MAX_SPS_COUNT as i32 {
                ((*(*p).pVtbl).Update)(p, 0, PARA_SET_TYPE_AVCSPS as i32);
                assert_eq!(
                    IWelsParametersetStrategy::GetSpsIdOffset(p, 0, 0),
                    expected,
                    "delta after update #{expected}"
                );
            }
            // 33rd update wraps uiNextParaSetIdToUseInBs back to 0.
            ((*(*p).pVtbl).Update)(p, 0, PARA_SET_TYPE_AVCSPS as i32);
            assert_eq!(IWelsParametersetStrategy::GetSpsIdOffset(p, 0, 0), 0);
            DestroyParametersetStrategy(p);
        }
    }

    /// PPS ids rotate over `MAX_PPS_COUNT`, not `MAX_SPS_COUNT`.
    #[test]
    fn increasing_strategy_uses_pps_bound_for_pps_ids() {
        let p = CreateParametersetStrategy(EParameterSetStrategy::INCREASING_ID, false, 1);
        unsafe {
            for expected in 0..MAX_SPS_COUNT as i32 + 4 {
                ((*(*p).pVtbl).Update)(p, 0, PARA_SET_TYPE_PPS as i32);
                assert_eq!(IWelsParametersetStrategy::GetPpsIdOffset(p, 0), expected);
            }
            DestroyParametersetStrategy(p);
        }
    }
}
