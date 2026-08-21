#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code
)]

//! Encoder function-pointer table.
//!
//! Translated from `codec/encoder/core/inc/wels_func_ptr_def.h`. `SWelsFuncPtrList`
//! previously existed as ten partial copies, the largest of which had 13 of its 70
//! members; it is 1280 bytes and every entry is dispatched through at encode time, so
//! a missing member silently shifts every later one.

#![deny(unsafe_code)]


use crate::common::mc::SMcFunc;
use crate::encoder::deblocking::{DeblockingFunc, PSetNoneZeroCountZeroFunc};
use crate::encoder::encoder_context::{
    sWelsEncCtx, BLOCK_STATIC_IDC_ALL, BLOCK_SIZE_ALL, C_PRED_A, I16_PRED_DC_A, I4_PRED_A,
};
use crate::encoder::encode_mb_aux::{
    PCalculateSingleCtrFunc, PCopyFunc, PDctFunc, PGetNoneZeroCountFunc, PQuantizationDcFunc,
    PQuantizationFunc, PQuantizationHadamardFunc, PQuantizationMaxFunc, PQuantizationSkipFunc,
    PScanFunc, PTransformHadamard4x4Func,
};
use crate::encoder::md::{
    PFillInterNeighborCacheFunc, PGetMbSignFromInterVaaFunc, PGetVarianceFromIntraVaaFunc,
    PUpdateMbMvFunc, SSampleDealingFunc, SWelsMD, SMB,
};
use crate::encoder::md::SMbCache;
use crate::encoder::rc::SWelsRcFunc;
use crate::encoder::svc_encode_mb::{PDeQuantizationFunc, PIDctFunc};
use crate::encoder::svc_encode_slice::{BsWriter, SDqLayer, SDynamicSlicingStack, SSlice};
use crate::encoder::svc_motion_estimate::{
    PCalculateBlockFeatureOfFrame, PCalculateSatdFunc, PCalculateSingleBlockFeature,
    PCheckDirectionalMv, PFillQpelLocationByFeatureValueFunc, PInitializeHashforFeatureFunc,
    PLineFullSearchFunc, PMotionSearchFunc, PSampleSadHor8Func, PSearchMethodFunc,
    PUpdateFMESwitch,
};
use crate::encoder::wels_preprocess::SVAAFrameInfo;

// ============================================================================
// Function pointer typedefs that had no Rust counterpart
// ============================================================================

/// `wels_func_ptr_def.h:178`. Note this is **not** the decoder's `PGetIntraPredFunc`,
/// which takes two arguments; the encoder's takes a separate reference pointer.
pub type PGetIntraPredFunc =
    unsafe extern "C" fn(pPrediction: *mut u8, pRef: *mut u8, kiStride: i32);

/// `wels_func_ptr_def.h:106`
pub type PIntraFineMdFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pCurMb: &mut SMB,
    pMbCache: *mut SMbCache,
) -> i32;

/// `wels_func_ptr_def.h:107`
pub type PInterFineMdFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: &mut SMB,
    bestCost: i32,
);

/// `wels_func_ptr_def.h:108`
pub type PInterMdFirstIntraModeFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pCurMb: &mut SMB,
    pMbCache: *mut SMbCache,
) -> bool;

/// `wels_func_ptr_def.h:111`
pub type PAccumulateSadFunc = unsafe extern "C" fn(
    pSumDiff: *mut u32,
    pGomForegroundBlockNum: *mut i32,
    iSad8x8: *mut i32,
    pVaaBgMbFlag: *mut i8,
);

/// `wels_func_ptr_def.h:116`
pub type PInterMdBackgroundDecisionFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: &mut SMB,
    pKeepPskip: *mut bool,
) -> bool;

/// `wels_func_ptr_def.h:118`
pub type PMdBackgroundInfoUpdateFunc = unsafe extern "C" fn(
    pCurLayer: *mut SDqLayer,
    pCurMb: &mut SMB,
    bFlag: bool,
    kiRefPictureType: i32,
);

/// `wels_func_ptr_def.h:121`
pub type PInterMdScrollingPSkipDecisionFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: &mut SMB,
) -> bool;

/// `wels_func_ptr_def.h:123`
pub type PSetScrollingMv =
    unsafe extern "C" fn(pVaa: *mut SVAAFrameInfo, pMd: &mut SWelsMD);

/// `wels_func_ptr_def.h:125`
pub type PInterMdFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: *mut SMB,
);

/// `wels_func_ptr_def.h:64`
pub type PDeQuantizationHadamardFunc = unsafe extern "C" fn(pRes: *mut i16, kuiMF: u16);

/// `wels_func_ptr_def.h:190`
pub type PCavlcParamCalFunc = unsafe extern "C" fn(
    pCoff: *mut i16,
    pRun: *mut u8,
    pLevel: *mut i16,
    pTotalCoeffs: *mut i32,
    iEndIdx: i32,
) -> i32;

// ============================================================================
// Entropy-coder dispatch — T4b.1
// ============================================================================

/// Which entropy coder a slice is written with: `iEntropyCodingModeFlag`, as a type.
///
/// **This replaces four `Option<fn>` members of [`SWelsFuncPtrList`]**
/// (`wels_func_ptr_def.h:192-195`: `pfWelsSpatialWriteMbSyn`, `pfGetBsPosition`,
/// `pfStashMBStatus`, `pfStashPopMBStatus`) and their four typedefs. They were
/// never four independent choices: `InitCoeffFunc` set all four together, from one
/// `if`, on one boolean, so what the table actually held was a *configuration*, not
/// a dispatch — the distinction plan §2.2.5 draws and Phase 4a deferred to here.
///
/// Three things fall out of saying so in the type system, and they are the reason
/// this is an `enum` rather than a `Box<dyn EntropyCoder>`:
///
/// * **The CAVLC arm drops `buf`.** T3.5 had to add `buf: &mut [u8]` to the stash
///   pair for CABAC's sake — `PropagateCarry` rewrites bytes behind the cursor, so
///   restoring the cursor alone would leave the output wrong — and the CAVLC
///   variants took it and ignored it, because a detached cursor is `Copy` and its
///   snapshot is a value (T3.4). One signature per arm means only the arm that
///   needs the buffer names it.
/// * **The CABAC thunk disappears.** `WelsSpatialWriteMbSynCabac` is a plain Rust
///   `fn` and the slot held an `extern "C"` pointer, so a bridging thunk existed.
///   With no slot there is no slot type, and the thunk was pure deletion.
/// * **The `is_some()` guards disappear** — with them the "installed?" question,
///   which had exactly one answer from `InitFunctionPointers` onward.
///
/// Per the brief's §1.2 the methods `match` at the call site and are `#[inline]`;
/// what this buys is a signature the compiler can see through, not speed — these
/// are per-macroblock calls with a runtime-selected arm, and 4a's finding is that
/// direct dispatch recovers scaffolding only where the caller supplies constant
/// dimensions.
///
/// The discriminants are `iEntropyCodingModeFlag`'s own values, and `Cavlc = 0`
/// is deliberately the zero one. That used to be load-bearing: it was what kept
/// `SWelsFuncPtrList`'s `mem::zeroed()` construction sound (S21). **T6.I1 wrote
/// that constructor out field by field**, so the property is no longer relied on
/// — it is kept because `Cavlc` is genuinely the C++'s default entropy coder, and
/// `#[default]` below now states that directly instead of a memset implying it.
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum EntropyCoder {
    #[default]
    Cavlc = 0,
    Cabac = 1,
}

impl EntropyCoder {
    /// `iEntropyCodingModeFlag != 0`, the one `if` this type replaces.
    #[inline]
    pub fn from_flag(iEntropyCodingModeFlag: i32) -> Self {
        if iEntropyCodingModeFlag != 0 {
            EntropyCoder::Cabac
        } else {
            EntropyCoder::Cavlc
        }
    }

    /// True for CABAC — for the call sites that still branch on the mode itself
    /// rather than on what it dispatches to (the CAVLC-only stash before a
    /// re-encode, `WelsInitSliceCabac`).
    #[inline]
    pub fn is_cabac(self) -> bool {
        self == EntropyCoder::Cabac
    }

    /// `pfWelsSpatialWriteMbSyn` — writes one macroblock's syntax elements.
    ///
    /// # Safety
    /// As the two implementations: `pEncCtx`, `pSlice` and `pCurMb` must be live
    /// and the slice's writer positioned in the buffer `slice_bs_buffer` returns.
    #[inline]
    // unsafe-cat: port-raw(Phase 7)
    #[allow(unsafe_code)]
    pub unsafe fn WelsSpatialWriteMbSyn(
        self,
        pEncCtx: *mut sWelsEncCtx,
        pSlice: *mut SSlice,
        pCurMb: *mut SMB,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => {
                crate::encoder::svc_set_mb_syn_cavlc::WelsSpatialWriteMbSyn(pEncCtx, pSlice, pCurMb)
            }
            EntropyCoder::Cabac => crate::encoder::svc_set_mb_syn_cabac::WelsSpatialWriteMbSynCabac(
                pEncCtx, pSlice, pCurMb,
            ),
        }
    }

    /// `pfStashMBStatus` — snapshots the coder state before a macroblock, so an
    /// overflow or a slice-boundary step-back can re-encode it.
    ///
    /// `buf` is the slice's output buffer and is **used by the CABAC arm only**;
    /// `pBs` is the slice's writer (`slice_writer`) and is **used by the CAVLC arm
    /// only** — see the type-level note. Both are parameters here because the
    /// caller cannot know which arm it is calling, and every caller holds the
    /// context both derive from.
    ///
    /// # Safety
    /// `pDss` and `pSlice` must be live, `pBs` must be `pSlice`'s writer and `buf`
    /// the buffer that writer is positioned in.
    #[inline]
    // unsafe-cat: port-raw(Phase 7)
    #[allow(unsafe_code)]
    pub unsafe fn StashMBStatus(
        self,
        buf: &mut [u8],
        pBs: *mut BsWriter,
        pDss: *mut SDynamicSlicingStack,
        pSlice: *mut SSlice,
        iMbSkipRun: i32,
    ) {
        match self {
            EntropyCoder::Cavlc => crate::encoder::svc_set_mb_syn_cavlc::StashMBStatusCavlc(
                pBs, pDss, pSlice, iMbSkipRun,
            ),
            EntropyCoder::Cabac => crate::encoder::svc_set_mb_syn_cavlc::StashMBStatusCabac(
                buf, pDss, pSlice, iMbSkipRun,
            ),
        }
    }

    /// `pfStashPopMBStatus` — restores what [`StashMBStatus`] saved, returning the
    /// stashed `iMbSkipRun`. See there for `buf` and `pBs`.
    ///
    /// # Safety
    /// As [`StashMBStatus`].
    ///
    /// [`StashMBStatus`]: EntropyCoder::StashMBStatus
    #[inline]
    // unsafe-cat: port-raw(Phase 7)
    #[allow(unsafe_code)]
    pub unsafe fn StashPopMBStatus(
        self,
        buf: &mut [u8],
        pBs: *mut BsWriter,
        pDss: *mut SDynamicSlicingStack,
        pSlice: *mut SSlice,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => {
                crate::encoder::svc_set_mb_syn_cavlc::StashPopMBStatusCavlc(pBs, pDss, pSlice)
            }
            EntropyCoder::Cabac => {
                crate::encoder::svc_set_mb_syn_cavlc::StashPopMBStatusCabac(buf, pDss, pSlice)
            }
        }
    }

    /// `pfGetBsPosition` — the slice writer's bit position, in the units each coder
    /// counts in. Needs no buffer on either arm: CAVLC reads the writer's own
    /// position (`pBs`, from `slice_writer`) and CABAC subtracts two offsets held
    /// in the slice's coder state (T3.5).
    ///
    /// # Safety
    /// `pSlice` must be live and `pBs` must be its writer.
    #[inline]
    // unsafe-cat: port-raw(Phase 7)
    #[allow(unsafe_code)]
    pub unsafe fn GetBsPosition(self, pBs: *mut BsWriter, pSlice: *mut SSlice) -> i32 {
        match self {
            EntropyCoder::Cavlc => crate::encoder::svc_set_mb_syn_cavlc::GetBsPosCavlc(pBs),
            EntropyCoder::Cabac => crate::encoder::svc_set_mb_syn_cavlc::GetBsPosCabac(pSlice),
        }
    }
}

// ============================================================================
// SWelsFuncPtrList
// ============================================================================

/// `TagWelsFuncPointerList` — `codec/encoder/core/inc/wels_func_ptr_def.h:198`.
/// 1280 bytes and 70 members in C++, in C++ declaration order; the port's size is
/// tracked by `encoder/abi_guard.rs`, which records each de-virtualization that
/// shrinks it.
#[repr(C)]
// T4b.2a: `Copy, Clone` came off when `pParametersetStrategy` became an owned
// `Option<Box<_>>`. Nothing copied the table by value -- it is only ever reached
// through `sWelsEncCtx::pFuncList`, a pointer -- so this is a derive that had been
// silently licensing a double-owner ever since the strategy was allocated at all.
pub struct SWelsFuncPtrList {
    // T4b.3b: `sExpandPicFunc: SExpandPicFunc` was the first member (24 bytes).
    // Both codecs installed the same three `_c` constants into it, so it is gone
    // and `common/expand_pic.rs::ExpandReferencingPicture` names them directly.
    // This is the first member deleted from this struct since T4b.1 -- and the
    // first time since Phase 4a's entry that `assert_size!` moves.
    pub pfFillInterNeighborCache: Option<PFillInterNeighborCacheFunc>,

    pub pfGetVarianceFromIntraVaa: Option<PGetVarianceFromIntraVaaFunc>,
    pub pfGetMbSignFromInterVaa: Option<PGetMbSignFromInterVaaFunc>,
    pub pfUpdateMbMv: Option<PUpdateMbMvFunc>,
    pub pfFirstIntraMode: Option<PInterMdFirstIntraModeFunc>,
    pub pfIntraFineMd: Option<PIntraFineMdFunc>,
    pub pfInterFineMd: Option<PInterFineMdFunc>,
    pub pfInterMd: Option<PInterMdFunc>,

    pub pfInterMdBackgroundDecision: Option<PInterMdBackgroundDecisionFunc>,
    pub pfMdBackgroundInfoUpdate: Option<PMdBackgroundInfoUpdateFunc>,

    pub pfSCDPSkipDecision: Option<PInterMdScrollingPSkipDecisionFunc>,
    pub pfSetScrollingMv: Option<PSetScrollingMv>,

    pub sMcFuncs: SMcFunc,
    pub sSampleDealingFuncs: SSampleDealingFunc,
    pub pfGetLumaI16x16Pred: [Option<PGetIntraPredFunc>; I16_PRED_DC_A],
    pub pfGetLumaI4x4Pred: [Option<PGetIntraPredFunc>; I4_PRED_A],
    pub pfGetChromaPred: [Option<PGetIntraPredFunc>; C_PRED_A],

    /// 1: for 16x16 square; 0: for 8x8 square
    pub pfSampleSadHor8: [Option<PSampleSadHor8Func>; 2],
    pub pfMotionSearch: [Option<PMotionSearchFunc>; BLOCK_STATIC_IDC_ALL],
    pub pfSearchMethod: [Option<PSearchMethodFunc>; BLOCK_SIZE_ALL],
    pub pfCalculateSatd: Option<PCalculateSatdFunc>,
    pub pfCheckDirectionalMv: Option<PCheckDirectionalMv>,

    pub pfInitializeHashforFeature: Option<PInitializeHashforFeatureFunc>,
    pub pfFillQpelLocationByFeatureValue: Option<PFillQpelLocationByFeatureValueFunc>,
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateBlockFeatureOfFrame: [Option<PCalculateBlockFeatureOfFrame>; 2],
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateSingleBlockFeature: [Option<PCalculateSingleBlockFeature>; 2],
    pub pfVerticalFullSearch: Option<PLineFullSearchFunc>,
    pub pfHorizontalFullSearch: Option<PLineFullSearchFunc>,
    pub pfUpdateFMESwitch: Option<PUpdateFMESwitch>,

    pub pfCopy16x16Aligned: Option<PCopyFunc>,
    pub pfCopy16x16NotAligned: Option<PCopyFunc>,
    pub pfCopy8x8Aligned: Option<PCopyFunc>,
    pub pfCopy16x8NotAligned: Option<PCopyFunc>,
    pub pfCopy8x16Aligned: Option<PCopyFunc>,
    pub pfCopy4x4: Option<PCopyFunc>,
    pub pfCopy8x4: Option<PCopyFunc>,
    pub pfCopy4x8: Option<PCopyFunc>,

    pub pfDctT4: Option<PDctFunc>,
    pub pfDctFourT4: Option<PDctFunc>,

    pub pfCalculateSingleCtr4x4: Option<PCalculateSingleCtrFunc>,
    /// DC/AC
    pub pfScan4x4: Option<PScanFunc>,
    pub pfScan4x4Ac: Option<PScanFunc>,

    pub pfQuantization4x4: Option<PQuantizationFunc>,
    pub pfQuantizationFour4x4: Option<PQuantizationFunc>,
    pub pfQuantizationDc4x4: Option<PQuantizationDcFunc>,
    pub pfQuantizationFour4x4Max: Option<PQuantizationMaxFunc>,
    pub pfQuantizationHadamard2x2: Option<PQuantizationHadamardFunc>,
    pub pfQuantizationHadamard2x2Skip: Option<PQuantizationSkipFunc>,

    pub pfTransformHadamard4x4Dc: Option<PTransformHadamard4x4Func>,

    pub pfGetNoneZeroCount: Option<PGetNoneZeroCountFunc>,

    pub pfDequantization4x4: Option<PDeQuantizationFunc>,
    pub pfDequantizationFour4x4: Option<PDeQuantizationFunc>,
    pub pfDequantizationIHadamard4x4: Option<PDeQuantizationHadamardFunc>,
    pub pfIDctFourT4: Option<PIDctFunc>,
    pub pfIDctT4: Option<PIDctFunc>,
    pub pfIDctI16x16Dc: Option<PIDctFunc>,

    /* For Deblocking */
    pub pfDeblocking: DeblockingFunc,
    pub pfSetNZCZero: Option<PSetNoneZeroCountZeroFunc>,

    pub pfRc: SWelsRcFunc,
    pub pfAccumulateSadForRc: Option<PAccumulateSadFunc>,

    // The three `pfSetMemZeroSize*` slots were here (`PSetMemoryZero`, i.e.
    // `fn(*mut c_void, i32)`): sizes times 8, times 64, times 64 aligned to 16.
    // All three were installed with the one `WelsSetMemZero_c` body and nothing
    // else, so the dispatch had one arm — deleted with the type, and the seven
    // call sites call `encoder_context::WelsSetMemZero_c` directly (S18, Phase 6
    // session B).

    pub pfCavlcParamCal: Option<PCavlcParamCalFunc>,

    /// `pfWelsSpatialWriteMbSyn`, `pfGetBsPosition`, `pfStashMBStatus` and
    /// `pfStashPopMBStatus` (`wels_func_ptr_def.h:192-195`) were four slots set
    /// together by one `if`; T4b.1 made them one [`EntropyCoder`]. -32 bytes of
    /// slots, +8 for the discriminant and its padding.
    pub eEntropyCoder: EntropyCoder,

    /// `IWelsParametersetStrategy*` — C++ declares an 8-byte pointer to a
    /// polymorphic object; **T4b.2a** made it an owned `Option<Box<_>>`, which is
    /// also 8 bytes by the null-pointer niche and which has a `Drop`.
    ///
    /// The name keeps its C++ `p` for diffability, but this member **owns** its
    /// object: `None` is the uninstalled state (and the all-zero pattern
    /// `WelsMallocz` produces), and dropping the box is `WELS_DELETE_OP`. Because
    /// the table itself is `WelsMallocz`'d and `WelsFree`'d, *this struct's* drop
    /// glue never runs — so `WelsUninitEncoderExt` `take()`s the field explicitly,
    /// at the same point `encoder_ext.cpp:1995` deletes it. See F19.
    pub pParametersetStrategy:
        Option<Box<crate::encoder::paraset_strategy::CWelsParametersetIdStrategyObj>>,
}

pub type TagWelsFuncPointerList = SWelsFuncPtrList;

impl Default for SWelsFuncPtrList {
    /// **T6.I1 — field-wise, replacing `unsafe { mem::zeroed() }`.**
    ///
    /// The zeroed version was sound and said so (S21): every member is a function
    /// pointer, an array of them, a POD sub-table of them, an `EntropyCoder` /
    /// `RCMode` whose zero discriminant is a declared variant, or an
    /// `Option<Box<_>>` whose all-zero is `None` by the null-pointer niche. But
    /// "sound" was a property re-argued in a comment every time a member was added,
    /// and the argument had to be re-checked by hand on each change — the member
    /// that would break it (any type without a valid all-zero bit pattern) is
    /// exactly the member nobody would notice adding.
    ///
    /// Written out, the compiler checks it instead, and the table stops being the
    /// last thing in the encoder context that needs an `unsafe` block to come into
    /// existence. Field for field this produces the same image the memset did; the
    /// three `init_fills_*` tests are unmodified across this change and are the
    /// proof, since they assert what `InitFunctionPointers` writes on top of it.
    fn default() -> Self {
        Self {
            pfFillInterNeighborCache: None,
            pfGetVarianceFromIntraVaa: None,
            pfGetMbSignFromInterVaa: None,
            pfUpdateMbMv: None,
            pfFirstIntraMode: None,
            pfIntraFineMd: None,
            pfInterFineMd: None,
            pfInterMd: None,
            pfInterMdBackgroundDecision: None,
            pfMdBackgroundInfoUpdate: None,
            pfSCDPSkipDecision: None,
            pfSetScrollingMv: None,
            sMcFuncs: SMcFunc::default(),
            sSampleDealingFuncs: SSampleDealingFunc::default(),
            pfGetLumaI16x16Pred: [None; I16_PRED_DC_A],
            pfGetLumaI4x4Pred: [None; I4_PRED_A],
            pfGetChromaPred: [None; C_PRED_A],
            pfSampleSadHor8: [None; 2],
            pfMotionSearch: [None; BLOCK_STATIC_IDC_ALL],
            pfSearchMethod: [None; BLOCK_SIZE_ALL],
            pfCalculateSatd: None,
            pfCheckDirectionalMv: None,
            pfInitializeHashforFeature: None,
            pfFillQpelLocationByFeatureValue: None,
            pfCalculateBlockFeatureOfFrame: [None; 2],
            pfCalculateSingleBlockFeature: [None; 2],
            pfVerticalFullSearch: None,
            pfHorizontalFullSearch: None,
            pfUpdateFMESwitch: None,
            pfCopy16x16Aligned: None,
            pfCopy16x16NotAligned: None,
            pfCopy8x8Aligned: None,
            pfCopy16x8NotAligned: None,
            pfCopy8x16Aligned: None,
            pfCopy4x4: None,
            pfCopy8x4: None,
            pfCopy4x8: None,
            pfDctT4: None,
            pfDctFourT4: None,
            pfCalculateSingleCtr4x4: None,
            pfScan4x4: None,
            pfScan4x4Ac: None,
            pfQuantization4x4: None,
            pfQuantizationFour4x4: None,
            pfQuantizationDc4x4: None,
            pfQuantizationFour4x4Max: None,
            pfQuantizationHadamard2x2: None,
            pfQuantizationHadamard2x2Skip: None,
            pfTransformHadamard4x4Dc: None,
            pfGetNoneZeroCount: None,
            pfDequantization4x4: None,
            pfDequantizationFour4x4: None,
            pfDequantizationIHadamard4x4: None,
            pfIDctFourT4: None,
            pfIDctT4: None,
            pfIDctI16x16Dc: None,
            pfDeblocking: DeblockingFunc::default(),
            pfSetNZCZero: None,
            pfRc: SWelsRcFunc::default(),
            pfAccumulateSadForRc: None,
            pfCavlcParamCal: None,
            eEntropyCoder: EntropyCoder::default(),
            pParametersetStrategy: None,
        }
    }
}
