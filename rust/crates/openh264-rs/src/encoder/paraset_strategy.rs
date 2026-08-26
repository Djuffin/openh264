//! Port of `codec/encoder/core/src/paraset_strategy.cpp` and
//! `codec/encoder/core/inc/paraset_strategy.h`.
//!
//! **Complete since T8b.B3.** C++ declares one abstract `IWelsParametersetStrategy`
//! and five concrete strategies; all five are here — `CONSTANT_ID`, `INCREASING_ID`,
//! `SPS_LISTING`, `SPS_LISTING_AND_PPS_INCREASING` and `SPS_PPS_LISTING`. Until
//! T8b.B3 the last three were `None` out of [`CreateParametersetStrategy`] and
//! `InitializeExt` refused, which was the honest shape while they were unported (S48)
//! and cost seven `test/api` rows.
//!
//! ### One object, five kinds — T4b.2a, extended by T8b.B3
//!
//! C++ layers six classes: `CWelsParametersetIdConstant`, the abstract
//! `CWelsParametersetIdNonConstant` (which overrides `OutputCurrentStructure` and
//! `LoadPreviousStructure`), `CWelsParametersetIdIncreasing` (which adds
//! `GetPpsIdOffset`, `GetSpsIdOffset` and `Update` on top), `CWelsParametersetSpsListing`,
//! `CWelsParametersetSpsListingPpsIncreasing` and `CWelsParametersetSpsPpsListing`.
//! **All six carry the same data members** — only their vtables differ — so this
//! module models them as one [`CWelsParametersetIdStrategyObj`] carrying a
//! [`ParasetIdKind`] discriminant, and the methods that actually differ `match` on it.
//!
//! Read a `match` here as the C++ class tree resolving one virtual call. Where an arm
//! is missing from a `match`, the C++ inherits — and a `_ =>` catch-all would hide
//! exactly that, so the arms are written out.
//!
//! `SPS_LISTING_AND_PPS_INCREASING` is its own kind rather than a flag on
//! `SpsListing`: in C++ it is a class, `CWelsParametersetSpsListingPpsIncreasing`,
//! which overrides precisely `GetPpsIdOffset` and `Update` and inherits everything
//! else from `CWelsParametersetSpsListing`. One kind per class keeps the map from the
//! header to this file a lookup rather than an argument.
//!
//! This replaced a hand-written C-style vtable (`IWelsParametersetStrategyVtbl`, 20
//! entries, 25 thunks, 2 static instances). The vtable existed because `SWelsFuncPtrList`
//! stores this as a plain 8-byte member and a `*mut dyn Trait` is a 16-byte fat pointer
//! that would mis-size the struct. `Option<Box<CWelsParametersetIdStrategyObj>>` is 8
//! bytes by the null-pointer niche, so the size is kept without the indirection — and
//! the `Box` makes the object's owner visible, which is how T4b.2a found the leak
//! recorded as F19.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

#![deny(unsafe_code)]

use std::ptr::null_mut;

use crate::api::codec_api::EParameterSetStrategy;
use crate::api::codec_api::RC_MODES::RC_OFF_MODE;
use crate::encoder::au_set::{WelsInitPps, WelsInitSps, WelsInitSubsetSps};
use crate::encoder::encoder_context::{
    ctx_param, ctx_pps_array, ctx_sps_array, ctx_subset_array, sWelsEncCtx, SLogContext,
    SParaSetOffset,
    SParaSetOffsetVariable, MAX_DQ_LAYER_NUM, MAX_PPS_COUNT, PARA_SET_TYPE,
    ctx_func_list,
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
    /// `CWelsParametersetSpsListing` — `paraset_strategy.h:231`. Keeps a list of SPSs
    /// and reuses an existing one whenever the current configuration matches it, so a
    /// mid-stream re-initialisation can go back to an SPS the decoder already has.
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
    /// The three listing kinds — `SPS_LISTING & eSpsPpsIdStrategy` in the C, which is
    /// a **bitmask** test (`codec_app_def.h:514-518`: 0x02, 0x03 and 0x06 all carry
    /// 0x02) and reads as an equality test if skimmed.
    #[inline]
    pub fn is_listing(self) -> bool {
        matches!(self, Self::SpsListing | Self::SpsListingPpsIncreasing | Self::SpsPpsListing)
    }

    /// The two kinds whose `GetPpsIdOffset` / `Update` rotate ids —
    /// `CWelsParametersetIdIncreasing` and the class that borrows its two methods.
    #[inline]
    pub fn rotates_ids(self) -> bool {
        matches!(self, Self::Increasing | Self::SpsListingPpsIncreasing)
    }

    /// The one kind that overrides `OutputCurrentStructure`/`LoadPreviousStructure`
    /// away from `CWelsParametersetIdNonConstant`'s — everything except `Constant`
    /// derives from it, so `Constant` is the exception.
    #[inline]
    pub fn is_non_constant(self) -> bool {
        !matches!(self, Self::Constant)
    }
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
/// (`paraset_strategy.cpp:365`) chains to it and adds nothing; the two listing
/// constructors (`:404`, `:538`) chain to it and then overwrite the two "basic
/// needed" counts, which is the only thing any constructor in the tree changes.
impl CWelsParametersetIdStrategyObj {
    pub fn new(eIdKind: ParasetIdKind, bSimulcastAVC: bool, kiSpatialLayerNum: i32) -> Box<Self> {
        // `paraset_strategy.cpp:410-411` (SpsListing, inherited by
        // SpsListingPpsIncreasing) and `:545-546` (SpsPpsListing).
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
            // C++ memsets m_sParaSetOffset to 0.
            m_sParaSetOffset: SParaSetOffset::default(),
            m_bSimulcastAVC: bSimulcastAVC,
            m_iSpatialLayerNum: kiSpatialLayerNum,
            m_iBasicNeededSpsNum,
            m_iBasicNeededPpsNum,
        })
    }

    // ------------------------------------------------------------------
    // The five methods whose C++ vtable entries differ between the kinds.
    // Read each `match` against `ID_CONSTANT_VTBL` / `ID_INCREASING_VTBL` as they
    // were: the `Constant` arm is the `ConstId_*` body, the `Increasing` arm the
    // `IncId_*` / `NonConstId_*` one.
    // ------------------------------------------------------------------

    /// `GetPpsIdOffset` — `paraset_strategy.cpp:216` (Constant) / `:384`
    /// (Increasing) / `:703` (SpsListingPpsIncreasing, whose body is the comment
    /// "same as CWelsParametersetIdIncreasing::GetPpsIdOffset" and then that body).
    /// `SpsListing` and `SpsPpsListing` inherit the Constant one.
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
    /// **Only `Increasing` overrides this one**, not `SpsListingPpsIncreasing`: that
    /// class borrows `GetPpsIdOffset` and `Update` from `CWelsParametersetIdIncreasing`
    /// and nothing else (`paraset_strategy.h:294-301`), so its SPS id offset is the
    /// Constant zero. The asymmetry is the reference's; it is written out here because
    /// a `rotates_ids()` test would silently "fix" it.
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
    /// `:708` (SpsListingPpsIncreasing, "same as CWelsParametersetIdIncreasing::Update").
    ///
    /// **`SpsListing` and `SpsPpsListing` inherit the Constant arm, which memsets the
    /// whole offset block** — including `uiInUseSpsNum` and `iPpsIdList`, the listing
    /// state. That would be destructive, and it never runs: every `Update` call site
    /// is inside `WriteSsvcParaset` or `WriteSavcParaset` (`encoder_ext.cpp:2890`,
    /// `:2908`, `:2937`, `:3177`, `:3213`), and a listing strategy is routed to
    /// `WriteSavcParaset_Listing` instead, which calls only `UpdatePpsList`. Written
    /// as the reference has it rather than as it would have to be if it were reached.
    #[inline]
    pub fn Update(&mut self, kuiId: u32, iParasetType: i32) {
        match self.eIdKind {
            ParasetIdKind::Constant
            | ParasetIdKind::SpsListing
            | ParasetIdKind::SpsPpsListing => {
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
    /// `paraset_strategy.cpp:292` (`CWelsParametersetIdNonConstant`) / `:519`
    /// (SpsListing) / `:684` (SpsPpsListing). The three trailing parameters are
    /// unused by the first two and are what the listing kinds write through.
    ///
    /// # Safety
    /// On an `Increasing` object, `pParaSetOffsetVariable` must be writable for
    /// `PARA_SET_TYPE` elements.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn OutputCurrentStructure(
        &mut self,
        pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
        pPpsIdList: *mut i32,
        pCtx: &mut sWelsEncCtx,
        pExistingParasetList: *mut SExistingParasetList,
    ) {
        if !self.eIdKind.is_non_constant() {
            return;
        }
        // `CWelsParametersetIdNonConstant::OutputCurrentStructure`
        // (`paraset_strategy.cpp:292`) — every kind but `Constant` runs it, and the
        // two listing kinds run it *and then* copy their lists out below.
        for k in 0..PARA_SET_TYPE {
            self.m_sParaSetOffset.sParaSetOffsetVariable[k].bUsedParaSetIdInBs =
                [false; MAX_PPS_COUNT];
        }
        std::ptr::copy_nonoverlapping(
            self.m_sParaSetOffset.sParaSetOffsetVariable.as_ptr(),
            pParaSetOffsetVariable,
            PARA_SET_TYPE,
        );

        // T9.H8: the trailing `|| pCtx.is_null()` is gone — a `&mut sWelsEncCtx`
        // cannot be null. The listing and paraset-list conditions are unchanged.
        if !self.eIdKind.is_listing() || pExistingParasetList.is_null() {
            return;
        }
        // `CWelsParametersetSpsListing::OutputCurrentStructure` — `:519`.
        (*pExistingParasetList).uiInUseSpsNum = self.m_sParaSetOffset.uiInUseSpsNum;
        std::ptr::copy_nonoverlapping(
            ctx_sps_array(pCtx),
            (*pExistingParasetList).sSps.as_mut_ptr(),
            MAX_SPS_COUNT,
        );
        // The C tests `NULL != pCtx->pSubsetArray`; the port's accessor is a pointer
        // into the context's own storage and the test is the same one.
        if !ctx_subset_array(pCtx).is_null() {
            (*pExistingParasetList).uiInUseSubsetSpsNum = self.m_sParaSetOffset.uiInUseSubsetSpsNum;
            std::ptr::copy_nonoverlapping(
                ctx_subset_array(pCtx),
                (*pExistingParasetList).sSubsetSps.as_mut_ptr(),
                MAX_SPS_COUNT,
            );
        } else {
            (*pExistingParasetList).uiInUseSubsetSpsNum = 0;
        }

        if self.eIdKind != ParasetIdKind::SpsPpsListing {
            return;
        }
        // `CWelsParametersetSpsPpsListing::OutputCurrentStructure` — `:684`.
        //
        // **The reference reads `pCtx->pPps` here, not `pCtx->pPPSArray`** — a single
        // `SWelsPPS` member — and copies `MAX_PPS_COUNT` of them out of it. That is an
        // over-read of 56 structs past the end of one; the port copies the array the
        // sentence means. See F94.
        (*pExistingParasetList).uiInUsePpsNum = self.m_sParaSetOffset.uiInUsePpsNum;
        std::ptr::copy_nonoverlapping(
            ctx_pps_array(pCtx),
            (*pExistingParasetList).sPps.as_mut_ptr(),
            MAX_PPS_COUNT,
        );
        if !pPpsIdList.is_null() {
            std::ptr::copy_nonoverlapping(
                self.m_sParaSetOffset.iPpsIdList.as_ptr() as *const i32,
                pPpsIdList,
                MAX_DQ_LAYER_NUM * MAX_PPS_COUNT,
            );
        }
    }

    /// `LoadPreviousStructure` — `paraset_strategy.h:148` (Constant, empty) /
    /// `paraset_strategy.cpp:300` (`CWelsParametersetIdNonConstant`).
    ///
    /// # Safety
    /// On an `Increasing` object, `pParaSetOffsetVariable` must be readable for
    /// `PARA_SET_TYPE` elements.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn LoadPreviousStructure(
        &mut self,
        pParaSetOffsetVariable: *mut SParaSetOffsetVariable,
        pPpsIdList: *mut i32,
    ) {
        if !self.eIdKind.is_non_constant() {
            return;
        }
        std::ptr::copy_nonoverlapping(
            pParaSetOffsetVariable as *const SParaSetOffsetVariable,
            self.m_sParaSetOffset.sParaSetOffsetVariable.as_mut_ptr(),
            PARA_SET_TYPE,
        );
        // `CWelsParametersetSpsPpsListing::LoadPreviousStructure` — `:676`. Only that
        // kind carries the id list back in; `SpsListing` and
        // `SpsListingPpsIncreasing` inherit the non-constant body above.
        if self.eIdKind == ParasetIdKind::SpsPpsListing && !pPpsIdList.is_null() {
            std::ptr::copy_nonoverlapping(
                pPpsIdList as *const i32,
                self.m_sParaSetOffset.iPpsIdList.as_mut_ptr() as *mut i32,
                MAX_DQ_LAYER_NUM * MAX_PPS_COUNT,
            );
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

    /// `GetNeededSubsetSpsNum` — `paraset_strategy.cpp:241` (Constant, inherited by
    /// `Increasing`) / `:416` (SpsListing, inherited by the other two listing kinds).
    /// The listing form asks for the whole array rather than one per extra layer,
    /// because that is what "listing" means.
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
    /// This is the hook that makes a listing strategy *mean* something across a
    /// mid-stream `InitializeExt`: `InitDqLayers` hands it the caller's
    /// `SExistingParasetList` (`encoder_ext.cpp:1161`) and the previous encoder's
    /// parameter sets come back into the new one's arrays, so a configuration the
    /// decoder has already seen keeps its old id.
    ///
    /// # Safety
    /// On a listing kind the four pointers must be valid: `pExistingParasetList` for
    /// reading, and the three arrays for `MAX_SPS_COUNT` / `MAX_SPS_COUNT` /
    /// `MAX_PPS_COUNT` writable elements. A null `pExistingParasetList` is the C's own
    /// early return.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn LoadPrevious(
        &mut self,
        pExistingParasetList: *mut SExistingParasetList,
        pSpsArray: *mut SWelsSPS,
        pSubsetArray: *mut SSubsetSps,
        pPpsArray: *mut SWelsPPS,
    ) {
        if !self.eIdKind.is_listing() || pExistingParasetList.is_null() {
            return;
        }
        // `CWelsParametersetSpsListing::LoadPreviousSps` — `:424`.
        self.m_sParaSetOffset.uiInUseSpsNum = (*pExistingParasetList).uiInUseSpsNum;
        if !pSpsArray.is_null() {
            std::ptr::copy_nonoverlapping(
                (*pExistingParasetList).sSps.as_ptr(),
                pSpsArray,
                MAX_SPS_COUNT,
            );
        }
        if self.GetNeededSubsetSpsNum() > 0 {
            self.m_sParaSetOffset.uiInUseSubsetSpsNum =
                (*pExistingParasetList).uiInUseSubsetSpsNum;
            if !pSubsetArray.is_null() {
                std::ptr::copy_nonoverlapping(
                    (*pExistingParasetList).sSubsetSps.as_ptr(),
                    pSubsetArray,
                    MAX_SPS_COUNT,
                );
            }
        } else {
            self.m_sParaSetOffset.uiInUseSubsetSpsNum = 0;
        }
        // `CWelsParametersetSpsPpsListing::LoadPreviousPps` — `:549`. The other two
        // listing kinds inherit the empty `CWelsParametersetIdConstant` body
        // (`paraset_strategy.h:155`), so only this one carries PPSs across.
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            self.m_sParaSetOffset.uiInUsePpsNum = (*pExistingParasetList).uiInUsePpsNum;
            if !pPpsArray.is_null() {
                std::ptr::copy_nonoverlapping(
                    (*pExistingParasetList).sPps.as_ptr(),
                    pPpsArray,
                    MAX_PPS_COUNT,
                );
            }
        }
    }

    /// `UpdatePpsList` — `paraset_strategy.h:114` (empty for four of the five kinds) /
    /// `paraset_strategy.cpp:560` (SpsPpsListing).
    ///
    /// Pre-expands `pPPSArray` from the `iPpsNum` distinct PPSs the encoder actually
    /// built to the full `MAX_PPS_COUNT`, each a copy of one of them with its own
    /// `iPpsId`, and fills `iPpsIdList[pps][idr_round]` with the id to use on each IDR
    /// round. `GetCurrentPpsId` is the reader.
    ///
    /// # Safety
    /// On `SpsPpsListing`, `pCtx` must be live with `pPPSArray` allocated to
    /// `MAX_PPS_COUNT` entries — which is what `m_iBasicNeededPpsNum = MAX_PPS_COUNT`
    /// asks `RequestMemorySvc` for.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn UpdatePpsList(&mut self, pCtx: *mut sWelsEncCtx) {
        if self.eIdKind != ParasetIdKind::SpsPpsListing || pCtx.is_null() {
            return;
        }
        let iPpsNum = (*pCtx).iPpsNum;
        if iPpsNum >= MAX_PPS_COUNT as i32 {
            return;
        }
        // `assert (pCtx->iPpsNum <= MAX_DQ_LAYER_NUM)` — a debug assert in the C, and
        // an early return here rather than a panic: `iUsePpsNum` is a divisor two
        // statements down, so zero would be a division by zero in both trees.
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
        let pps = ctx_pps_array(pCtx);
        for iPpsId in iUsePpsNum as usize..MAX_PPS_COUNT {
            *pps.add(iPpsId) = *pps.add(iPpsId % iUsePpsNum as usize);
            (*pps.add(iPpsId)).iPpsId = iPpsId as u32;
            (*pCtx).iPpsNum += 1;
        }
        self.m_sParaSetOffset.uiInUsePpsNum = (*pCtx).iPpsNum as u32;
    }

    /// `CheckParamCompatibility` — `paraset_strategy.h:116` (unconditionally true) /
    /// `paraset_strategy.cpp:449` (SpsListing and the two kinds below it).
    ///
    /// The listing form is the same rule `ParamValidationExt` applies before the
    /// object exists (`encoder_ext.cpp:467-473`), applied again where the object can
    /// see it: more than one SVC spatial layer and the strategy falls back to
    /// `CONSTANT_ID`.
    ///
    /// # Safety
    /// On a listing kind, `pCodingParam` must be a writable coding-parameter block.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn CheckParamCompatibility(
        &mut self,
        pCodingParam: *mut SWelsSvcCodingParam,
        pLogCtx: *mut SLogContext,
    ) -> bool {
        if !self.eIdKind.is_listing() || pCodingParam.is_null() {
            return true;
        }
        if (*pCodingParam).iSpatialLayerNum > 1 && !(*pCodingParam).bSimulcastAVC {
            crate::common::wels_trace::WelsLog(
                pLogCtx,
                crate::common::wels_trace::WELS_LOG_WARNING,
                &format!(
                    "ParamValidationExt(), eSpsPpsIdStrategy setting ({:?}) with multiple svc SpatialLayers ({}) not supported! eSpsPpsIdStrategy adjusted to CONSTANT_ID",
                    (*pCodingParam).eSpsPpsIdStrategy,
                    (*pCodingParam).iSpatialLayerNum
                ),
            );
            (*pCodingParam).eSpsPpsIdStrategy = EParameterSetStrategy::CONSTANT_ID;
            return false;
        }
        true
    }

    /// `CheckPpsGenerating` — `paraset_strategy.h:158` / `paraset_strategy.cpp:463`
    /// (SpsListing, always true) / `:586` (SpsPpsListing, false once the PPS list is
    /// full). Not a hook the encoder calls: `GenerateNewSps` is its only caller.
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
    /// # Safety
    /// `pCtx` must be live with `pSpsArray` / `pSubsetArray` allocated to
    /// `MAX_SPS_COUNT` entries.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    unsafe fn SpsReset(&mut self, pCtx: *mut sWelsEncCtx, kbUseSubsetSps: bool) -> i32 {
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            return -1;
        }
        // `SWelsSPS::ZERO`, not `default()`: F56's rule, and T6.G3 established it at
        // exactly this memset. `Default` seeds `uiProfileIdc = PRO_BASELINE` and the
        // VUI `*_UNDEF` values, which are not zero.
        if !kbUseSubsetSps {
            self.m_sParaSetOffset.uiInUseSpsNum = 1;
            for i in 0..MAX_SPS_COUNT {
                *ctx_sps_array(pCtx).add(i) = SWelsSPS::ZERO;
            }
        } else {
            self.m_sParaSetOffset.uiInUseSubsetSpsNum = 1;
            for i in 0..MAX_SPS_COUNT {
                *ctx_subset_array(pCtx).add(i) = SSubsetSps::ZERO;
            }
        }
        0
    }

    /// `GenerateNewSps` — `paraset_strategy.cpp:265`.
    ///
    /// # Safety
    /// `pCtx` must satisfy [`WelsGenerateNewSps`]'s contract.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn GenerateNewSps(
        &mut self,
        pCtx: &mut sWelsEncCtx,
        kbUseSubsetSps: bool,
        iDlayerIndex: i32,
        iDlayerCount: i32,
        kuiSpsId: u32,
        bSVCBaselayer: bool,
    ) -> u32 {
        if !self.eIdKind.is_listing() {
            WelsGenerateNewSps(
                pCtx,
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
        let kiFoundSpsId = FindExistingSps(
            ctx_param(pCtx),
            kbUseSubsetSps,
            iDlayerIndex,
            iDlayerCount,
            if kbUseSubsetSps {
                self.m_sParaSetOffset.uiInUseSubsetSpsNum
            } else {
                self.m_sParaSetOffset.uiInUseSpsNum
            } as i32,
            ctx_sps_array(pCtx),
            ctx_subset_array(pCtx),
            bSVCBaselayer,
        );
        if INVALID_ID != kiFoundSpsId {
            // The C also writes `pSps`/`pSubsetSps` here; T6.G3 deleted those two
            // out-parameters because every caller recomputed them from this return
            // value in the next statement.
            return kiFoundSpsId as u32;
        }
        if !self.CheckPpsGenerating() {
            // `return -1` on a `uint32_t` in the C — the caller compares against
            // `(uint32_t)-1`, so the bit pattern is what travels.
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
            if self.SpsReset(pCtx, kbUseSubsetSps) < 0 {
                return u32::MAX;
            }
            kuiSpsId = 0;
        }
        WelsGenerateNewSps(
            pCtx,
            kbUseSubsetSps,
            iDlayerIndex,
            iDlayerCount,
            kuiSpsId as i32,
            bSVCBaselayer,
        );
        kuiSpsId
    }

    /// `InitPps` — `paraset_strategy.cpp:276`.
    ///
    /// Note the literal `true` C++ passes for `kbDeblockingFilterPresentFlag`, ignoring
    /// the argument of the same name.
    ///
    /// **The two SPS parameters are `Option`s since T6.G3** — see [`WelsInitPps`],
    /// which this forwards to unchanged.
    ///
    /// # Safety
    /// `pCtx->pPPSArray` must hold at least `kuiPpsId + 1` entries.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn InitPps(
        &mut self,
        pCtx: &mut sWelsEncCtx,
        _kiSpsId: u32,
        pSps: Option<&SWelsSPS>,
        pSubsetSps: Option<&SSubsetSps>,
        kuiPpsId: u32,
        _kbDeblockingFilterPresentFlag: bool,
        kbUsingSubsetSps: bool,
        kbEntropyCodingModeFlag: bool,
    ) -> u32 {
        // `CWelsParametersetSpsPpsListing::InitPps` — `paraset_strategy.cpp:639`.
        // Only that kind looks for an existing PPS; the other four write the slot the
        // caller named.
        let mut kuiPpsId = kuiPpsId;
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            let kiFoundPpsId = FindExistingPps(
                pSps,
                pSubsetSps,
                kbUsingSubsetSps,
                _kiSpsId as i32,
                kbEntropyCodingModeFlag,
                self.m_sParaSetOffset.uiInUsePpsNum as i32,
                ctx_pps_array(pCtx),
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
            &mut *ctx_pps_array(pCtx).add(kuiPpsId as usize),
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
    /// (SpsListing) / `:664` (SpsPpsListing, which chains and adds the PPS count).
    ///
    /// This is what tells the bitstream writer how many parameter sets to emit;
    /// `WriteSavcParaset_Listing` loops to `iSpsNum` and `iPpsNum`, so a strategy that
    /// left this empty would write one of each and produce a stream missing the very
    /// list it was configured for.
    ///
    /// # Safety
    /// On a listing kind, `pCtx` must be a live encoder context.
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn UpdateParaSetNum(&mut self, pCtx: &mut sWelsEncCtx) {
        // T9.H8: the trailing `|| pCtx.is_null()` is gone — a `&mut sWelsEncCtx`
        // cannot be null. The listing condition is unchanged.
        if !self.eIdKind.is_listing() {
            return;
        }
        pCtx.iSpsNum = self.m_sParaSetOffset.uiInUseSpsNum as i32;
        pCtx.iSubsetSpsNum = self.m_sParaSetOffset.uiInUseSubsetSpsNum as i32;
        if self.eIdKind == ParasetIdKind::SpsPpsListing {
            pCtx.iPpsNum = self.m_sParaSetOffset.uiInUsePpsNum as i32;
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

    /// `GetSpsIdx` — `paraset_strategy.h:150` (always 0) / `:252` (the listing kinds,
    /// the identity). The constant form is right when there is one SPS; a listing
    /// strategy has a list and the caller's index is the index.
    #[inline]
    pub fn GetSpsIdx(&self, iIdx: i32) -> i32 {
        if self.eIdKind.is_listing() {
            iIdx
        } else {
            0
        }
    }
}

/// `WelsGenerateNewSps` — `paraset_strategy.cpp:78` (file-static).
///
/// # Safety
/// `pCtx` must have `pSvcParam` set and `pSpsArray`/`pSubsetArray` allocated to at
/// least `kiSpsId + 1` entries.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsGenerateNewSps(
    pCtx: *mut sWelsEncCtx,
    kbUseSubsetSps: bool,
    iDlayerIndex: i32,
    iDlayerCount: i32,
    kiSpsId: i32,
    bSVCBaselayer: bool,
) -> i32 {
    let iRet;
    // **T6.G3: the two `*mut *mut` out-parameters are gone**, and with them the
    // block that filled them. They handed the caller back exactly
    // `pSpsArray[kiSpsId]` / `pSubsetArray[kiSpsId]` — values it recomputed from the
    // id this function returns, in the very next statement — so they were a second
    // copy of the return value carrying a pointer's failure modes. Each arm below
    // now reaches its own array once, where it uses it, and the two-arm derivation
    // that stood here (which computed one pointer the caller's arm never read) is
    // gone with them.
    let pParam = ctx_param(pCtx);
    // S29's named shape. `WelsInitSps` takes `*mut SSpatialLayerConfig`, so the
    // reference here only existed to retag and be cast away — and its retag is
    // what invalidated `InitDqLayers`'s live pointer into the same layer.
    let pDlayerParam = std::ptr::addr_of_mut!((*pParam).sSpatialLayers[iDlayerIndex as usize]);
    // Need port pSps/pPps initialization due to spatial scalability changed
    if !kbUseSubsetSps {
        iRet = WelsInitSps(
            &mut *ctx_sps_array(pCtx).add(kiSpsId as usize),
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
            &mut *ctx_subset_array(pCtx).add(kiSpsId as usize),
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
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn ParasetStrategy<'a>(
    pCtx: *mut sWelsEncCtx,
) -> &'a mut CWelsParametersetIdStrategyObj {
    (*ctx_func_list(pCtx))
        .pParametersetStrategy
        .as_deref_mut()
        .expect("pParametersetStrategy is installed by InitFunctionPointers")
}

/// `IWelsParametersetStrategy::CreateParametersetStrategy` — `paraset_strategy.cpp:40`.
///
/// All five strategies since T8b.B3. Until then the three listing ones returned
/// `None` and `InitFunctionPointers` failed the encoder build — the S48 shape, which
/// is right for an unported feature and wrong once it is ported.
///
/// **The `Option` stays, and it is now always `Some`.** `EParameterSetStrategy` is a
/// closed five-variant enum (`codec_api.rs:556`), so with all five mapped the `match`
/// is exhaustive and C++'s `default:` label has nothing left to catch — the value a
/// caller hands in has already been checked against the five by
/// `SWelsSvcCodingParam`'s own transcode (`param_svc.rs:688-694`), which is where a
/// sixth would be refused. The return type is left as it is because every call site
/// is written against it and because `InitFunctionPointers` still needs a failure
/// path for the allocation the C checks.
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
///
/// # Safety
/// Both pointers must reference initialised `SWelsSPS` values.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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

/// `FindExistingPps` — `paraset_strategy.cpp:608` (T8b.B3).
///
/// Returns the index of a stored PPS the current configuration would produce, or
/// [`INVALID_ID`]. Its only caller is `SpsPpsListing`'s `InitPps`.
///
/// The reference opens with `#if !defined(DISABLE_FMO_FEATURE) return INVALID_ID;
/// #endif` — dead, because `as264_common.h:53` defines that macro unconditionally, the
/// same disposition the port already gives the FMO blocks in `au_set.rs`.
///
/// The comparison is the reference's six fields, not a whole-struct compare:
/// `iPpsId` and the deblocking-filter *idc* fields differ between an existing entry
/// and the probe by construction, so comparing everything would never match.
///
/// # Safety
/// `pPpsArray` must hold at least `iPpsNumInUse` entries.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FindExistingPps(
    pSps: Option<&SWelsSPS>,
    pSubsetSps: Option<&SSubsetSps>,
    kbUseSubsetSps: bool,
    _iSpsId: i32,
    kbEntropyCodingFlag: bool,
    iPpsNumInUse: i32,
    pPpsArray: *mut SWelsPPS,
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
        let p = &*pPpsArray.add(iId as usize);
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
///
/// # Safety
/// `pParam` must be initialised; `pSpsArray`/`pSubsetArray` must hold at least
/// `iSpsNumInUse` entries.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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

    /// **This replaces `unported_strategies_return_none`** (T8b.B3), which asserted
    /// the three listing strategies came back `None`. Every one of the five now
    /// builds, and each maps to the class the header names.
    #[test]
    fn every_strategy_builds_and_maps_to_its_class() {
        let cases = [
            (EParameterSetStrategy::CONSTANT_ID, ParasetIdKind::Constant),
            (EParameterSetStrategy::INCREASING_ID, ParasetIdKind::Increasing),
            (EParameterSetStrategy::SPS_LISTING, ParasetIdKind::SpsListing),
            (
                EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING,
                ParasetIdKind::SpsListingPpsIncreasing,
            ),
            (EParameterSetStrategy::SPS_PPS_LISTING, ParasetIdKind::SpsPpsListing),
        ];
        for (e, kind) in cases {
            let p = CreateParametersetStrategy(e, false, 1).expect("all five build");
            assert_eq!(p.eIdKind, kind, "{e:?}");
        }
    }

    /// The constructors' one difference — `paraset_strategy.cpp:410-411` and
    /// `:545-546`. A listing strategy asks `RequestMemorySvc` for the whole array
    /// because that is what it will fill; getting this wrong is an out-of-bounds
    /// write into `pSpsArray` the first time the list grows, not a wrong id.
    #[test]
    fn listing_kinds_ask_for_the_whole_array() {
        let mut sl = strategy(EParameterSetStrategy::SPS_LISTING);
        assert_eq!(sl.GetNeededSpsNum(), MAX_SPS_COUNT as u32);
        assert_eq!(sl.GetNeededSubsetSpsNum(), MAX_SPS_COUNT as u32);
        assert_eq!(sl.GetNeededPpsNum(), 1);

        let mut spl = strategy(EParameterSetStrategy::SPS_PPS_LISTING);
        assert_eq!(spl.GetNeededSpsNum(), MAX_SPS_COUNT as u32);
        assert_eq!(spl.GetNeededPpsNum(), MAX_PPS_COUNT as u32);

        // `SpsListingPpsIncreasing` inherits `SpsListing`'s constructor whole.
        let mut sli = strategy(EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING);
        assert_eq!(sli.GetNeededSpsNum(), sl.GetNeededSpsNum());
        assert_eq!(sli.GetNeededPpsNum(), sl.GetNeededPpsNum());
    }

    /// **The asymmetry `paraset_strategy.h:294-301` creates, pinned.**
    /// `CWelsParametersetSpsListingPpsIncreasing` overrides exactly `GetPpsIdOffset`
    /// and `Update` — so its *PPS* id rotates like `Increasing`'s and its *SPS* id
    /// offset stays the Constant zero. A `rotates_ids()` test on `GetSpsIdOffset`
    /// would look tidier and be wrong; this is the test that says so.
    #[test]
    fn sps_listing_pps_increasing_rotates_only_the_pps_id() {
        let mut p = strategy(EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING);
        // One rotation of the PPS id, as `WelsWriteOnePPS`'s caller would do.
        p.Update(0, PARA_SET_TYPE_PPS as i32);
        p.Update(0, PARA_SET_TYPE_PPS as i32);
        assert_ne!(p.GetPpsIdOffset(0), 0, "the PPS id must rotate");
        // …and the SPS side does not, however many times it is asked.
        p.Update(0, PARA_SET_TYPE_AVCSPS as i32);
        assert_eq!(p.GetSpsIdOffset(0, 0), 0, "the SPS id offset is the Constant zero");
    }

    /// `GetSpsIdx` — the constant kinds answer 0 for every index because they have one
    /// SPS; a listing kind has a list and answers the index. `WelsWriteOneSPS` uses it
    /// to pick which SPS to write, so a listing strategy that answered 0 would write
    /// the first SPS `iSpsNum` times.
    #[test]
    fn get_sps_idx_is_the_identity_only_for_listing_kinds() {
        assert_eq!(strategy(EParameterSetStrategy::CONSTANT_ID).GetSpsIdx(3), 0);
        assert_eq!(strategy(EParameterSetStrategy::INCREASING_ID).GetSpsIdx(3), 0);
        assert_eq!(strategy(EParameterSetStrategy::SPS_LISTING).GetSpsIdx(3), 3);
        assert_eq!(
            strategy(EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING).GetSpsIdx(3),
            3
        );
        assert_eq!(strategy(EParameterSetStrategy::SPS_PPS_LISTING).GetSpsIdx(3), 3);
    }

    /// `GetCurrentPpsId` — only `SPS_PPS_LISTING` rotates by IDR round, and it reads
    /// the list `UpdatePpsList` builds. With the list still zero (no `UpdatePpsList`
    /// yet) it answers 0 rather than the identity, which is the observable difference
    /// from the other four.
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
        assert_eq!(p.GetCurrentPpsId(1, 5), ((5 * 2 + 1) % MAX_PPS_COUNT) as i32);
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
