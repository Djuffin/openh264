#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code
)]

//! Encoder function-pointer table.
//!
//! Translated from `codec/encoder/core/inc/wels_func_ptr_def.h`.

#![forbid(unsafe_code)]


use crate::encoder::rec_view::RecCursor;
use crate::common::mc::SMcFunc;
use crate::encoder::deblocking::DeblockingFunc;
use crate::encoder::encoder_context::{
    sWelsEncCtx, BLOCK_STATIC_IDC_ALL, BLOCK_SIZE_ALL, C_PRED_A, I16_PRED_DC_A, I4_PRED_A,
};
use crate::encoder::encode_mb_aux::{
    PCalculateSingleCtrFunc, PCopyFunc, PDctFunc, PGetNoneZeroCountFunc, PQuantizationDcFunc,
    PQuantization4x4Func, PQuantizationFunc, PQuantizationHadamardFunc, PQuantizationMaxFunc,
    PQuantizationSkipFunc,
    PScanFunc, PTransformHadamard4x4Func,
};
use crate::encoder::md::{
    PFillInterNeighborCacheFunc, PGetMbSignFromInterVaaFunc, PGetVarianceFromIntraVaaFunc,
    PUpdateMbMvFunc, SSampleDealingFunc, SWelsMD, SMB,
};
use crate::encoder::md::SMbCache;
use crate::encoder::rc::SWelsRcFunc;
use crate::encoder::svc_encode_mb::{PDeQuantization4x4Func, PDeQuantizationFunc};
use crate::encoder::svc_encode_slice::{BsWriter, SDqLayer, SDynamicSlicingStack, SSlice};
use crate::encoder::svc_motion_estimate::{
    PCalculateBlockFeatureOfFrame, PCalculateSatdFunc, PCalculateSingleBlockFeature,
    PCheckDirectionalMv, PFillQpelLocationByFeatureValueFunc, PInitializeHashforFeatureFunc,
    PLineFullSearchFunc, PMotionSearchFunc, PSearchMethodFunc, SMeFuncs,
    PUpdateFMESwitch,
};
use crate::encoder::wels_preprocess::SVAAFrameInfoExt;

// ============================================================================
// Function pointer typedefs
// ============================================================================

/// `wels_func_ptr_def.h:178`.
///
/// The C++ has one `PGetIntraPredFunc` serving all three tables. The destination is
/// a packed prediction block whose size is fixed per table — 16, 64 or 256 bytes —
/// so the safe form names the size, and a chroma predictor can no longer be
/// installed into the luma table by a slip of the index.
///
/// The reference is the **reconstruction picture**, read and never written, so it
/// arrives as the seam's read cursor.
pub type PGetLumaI4x4PredFunc = fn(pred: &mut [u8; 16], rec: &RecCursor<'_>);
/// [`PGetLumaI4x4PredFunc`] for the 8x8 chroma prediction block.
pub type PGetChromaPredFunc = fn(pred: &mut [u8; 64], rec: &RecCursor<'_>);
/// [`PGetLumaI4x4PredFunc`] for the 16x16 luma prediction block.
pub type PGetLumaI16x16PredFunc = fn(pred: &mut [u8; 256], rec: &RecCursor<'_>);

/// `wels_func_ptr_def.h:106`
pub type PIntraFineMdFunc = fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32;

/// `wels_func_ptr_def.h:107`
pub type PInterFineMdFunc = for<'a> fn(
    // The context and the mode-decision record share a lifetime. The fine-partition
    // body resolves the reference picture through the context, and that picture is
    // what `SWelsMD`'s cursors point into — so the slot has to say the two outlive
    // each other.
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
    bestCost: i32,
);

/// `wels_func_ptr_def.h:108`
pub type PInterMdFirstIntraModeFunc = fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool;

/// `wels_func_ptr_def.h:116`
pub type PInterMdBackgroundDecisionFunc = fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
    pKeepPskip: &mut bool,
) -> bool;

/// `wels_func_ptr_def.h:118`
pub type PMdBackgroundInfoUpdateFunc = extern "C" fn(
    pEncCtx: &sWelsEncCtx,
    pCurLayer: &SDqLayer,
    pCurMb: &mut SMB,
    bFlag: bool,
    kiRefPictureType: i32,
);

/// `wels_func_ptr_def.h:121`
pub type PInterMdScrollingPSkipDecisionFunc = fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
) -> bool;

/// `wels_func_ptr_def.h:123`
/// **Shared, and deliberately not `&mut`.** `SetScrollingMvToMdNull` is
/// fork-reachable, so an exclusive reference here would be N workers each taking a
/// `Unique` retag over the one video-analysis block every worker shares — a data
/// race whether or not anything is written through it. The real implementation only
/// *reads* the block (two scalars off `sScrollDetectInfo`); everything it writes
/// goes through `pMd`, which is already exclusive and per-macroblock.
///
/// **The slot carries the extension, not the base block.**
/// The C++ hands this slot an `SVAAFrameInfo*` and `SetScrollingMvToMd`
/// downcasts it (`static_cast<SVAAFrameInfoExt*>`) because upstream's screen
/// path always passes an extension in that parameter. The port's caller reaches
/// the extension through `sWelsEncCtx::vaa_ext_ref`, which answers `Some` only
/// under screen content, so the downcast has no subject and the parameter says
/// what the value is. `Option` because that accessor's answer is one — the arm
/// is unreachable once the body is installed (the installer requires the
/// extension), and it is the `Null` twin's answer.
pub type PSetScrollingMv = fn(pVaaExt: Option<&SVAAFrameInfoExt>, pMd: &mut SWelsMD<'_>);

/// `wels_func_ptr_def.h:125`
pub type PInterMdFunc = for<'a> fn(
    // As `PInterFineMdFunc`: the reference picture is resolved through the context,
    // and `SWelsMD`'s cursors point into it, so the slot says the two share a
    // lifetime.
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    slice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
);

/// `wels_func_ptr_def.h:64`
pub type PDeQuantizationHadamardFunc = fn(pRes: &mut [i16; 16], kuiMF: u16);

/// `wels_func_ptr_def.h:190`
/// **`pCoff` is a slice rather than a fixed array on purpose.**
/// The two call families walk different extents off the same flat cursor —
/// luma steps `sDct.iLumaBlock` (a `[[i16; 16]; 16]` read flat) in sixteens with
/// `iEndIdx = 15`, chroma DC steps `sDct.iChromaDc` (`[[i16; 4]; 2]`) in fours
/// with `iEndIdx = 3`. A `&[i16; 16]` would reach twelve elements past
/// `iChromaDc[1]`; the slice carries the extent the caller actually owns, and
/// the implementation's backward scan starts at `iEndIdx` inside it.
///
/// `pRun`/`pLevel` are the caller's own `[0u8; 16]` / `[0i16; 16]` locals, so
/// the fixed-array shape is exact there.
pub type PCavlcParamCalFunc = fn(
    pCoff: &[i16],
    pRun: &mut [u8; 16],
    pLevel: &mut [i16; 16],
    pTotalCoeffs: &mut i32,
    iEndIdx: i32,
) -> i32;

// ============================================================================
// Entropy-coder dispatch
// ============================================================================

/// Which entropy coder a slice is written with: `iEntropyCodingModeFlag`, as a type.
///
/// **This replaces four `Option<fn>` members of [`SWelsFuncPtrList`]**
/// (`wels_func_ptr_def.h:192-195`: `pfWelsSpatialWriteMbSyn`, `pfGetBsPosition`,
/// `pfStashMBStatus`, `pfStashPopMBStatus`) and their four typedefs. They were
/// never four independent choices: `InitCoeffFunc` set all four together, from one
/// `if`, on one boolean, so what the table actually held was a *configuration*, not
/// a dispatch.
///
/// The discriminants are `iEntropyCodingModeFlag`'s own values, and `Cavlc = 0`
/// is deliberately the zero one: `Cavlc` is the C++'s default entropy coder.
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
    /// The record comes as the grid window: both writers read same-slice
    /// neighbours for context modelling and write the current record's QP and
    /// MVD state, so `mbs` is exactly "my slice's records so far, current last".
    #[inline]
    pub fn WelsSpatialWriteMbSyn(
        self,
        pEncCtx: &sWelsEncCtx,
        pSlice: &mut SSlice,
        mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
        pSliceBsBuf: &mut [u8],
        pCtxOutBs: &mut Option<&mut crate::encoder::vlc_encoder::BsWriter>,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => {
                crate::encoder::svc_set_mb_syn_cavlc::WelsSpatialWriteMbSyn(pEncCtx, pSlice, mbs, pSliceBsBuf, pCtxOutBs)
            }
            EntropyCoder::Cabac => crate::encoder::svc_set_mb_syn_cabac::WelsSpatialWriteMbSynCabac(
                pEncCtx, pSlice, mbs, pSliceBsBuf, pCtxOutBs,
            ),
        }
    }

    /// `pfStashMBStatus` — snapshots the coder state before a macroblock, so an
    /// overflow or a slice-boundary step-back can re-encode it.
    ///
    /// `buf` is the slice's output buffer and is **used by the CABAC arm only**;
    /// `pBs` is the slice's writer (`slice_bs_writer`) and is **used by the CAVLC arm
    /// only**. Both are parameters here because the caller cannot know which arm it
    /// is calling, and every caller holds the context both derive from.
    ///
    /// `pBs` must be the slice's writer and `buf` the buffer it is positioned in.
    #[inline]
    pub fn StashMBStatus(
        self,
        buf: &mut [u8],
        pBs: &mut BsWriter,
        pDss: &mut SDynamicSlicingStack<'_>,
        pCabacCtx: &mut crate::encoder::set_mb_syn_cabac::SCabacCtx,
        kuiLastMbQp: u8,
        iMbSkipRun: i32,
    ) {
        match self {
            EntropyCoder::Cavlc => crate::encoder::svc_set_mb_syn_cavlc::StashMBStatusCavlc(
                pBs, pDss, kuiLastMbQp, iMbSkipRun,
            ),
            EntropyCoder::Cabac => crate::encoder::svc_set_mb_syn_cavlc::StashMBStatusCabac(
                buf, pDss, pCabacCtx, kuiLastMbQp, iMbSkipRun,
            ),
        }
    }

    /// `pfStashPopMBStatus` — restores what [`StashMBStatus`] saved, returning the
    /// stashed `iMbSkipRun`. See there for `buf` and `pBs`.
    ///
    /// [`StashMBStatus`]: EntropyCoder::StashMBStatus
    #[inline]
    pub fn StashPopMBStatus(
        self,
        buf: &mut [u8],
        pBs: &mut BsWriter,
        pDss: &mut SDynamicSlicingStack<'_>,
        pCabacCtx: &mut crate::encoder::set_mb_syn_cabac::SCabacCtx,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => {
                crate::encoder::svc_set_mb_syn_cavlc::StashPopMBStatusCavlc(pBs, pDss)
            }
            EntropyCoder::Cabac => {
                crate::encoder::svc_set_mb_syn_cavlc::StashPopMBStatusCabac(buf, pDss, pCabacCtx)
            }
        }
    }

    /// `pfGetBsPosition` — the slice writer's bit position, in the units each coder
    /// counts in. Needs no buffer on either arm: CAVLC reads the writer's own
    /// position (`pBs`, from `slice_bs_writer`) and CABAC subtracts two offsets held
    /// in the slice's coder state.
    #[inline]
    pub fn GetBsPosition(
        self,
        pBs: &BsWriter,
        pCabacCtx: &crate::encoder::set_mb_syn_cabac::SCabacCtx,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => crate::encoder::svc_set_mb_syn_cavlc::GetBsPosCavlc(pBs),
            EntropyCoder::Cabac => crate::encoder::svc_set_mb_syn_cavlc::GetBsPosCabac(pCabacCtx),
        }
    }
}

// ============================================================================
// SWelsFuncPtrList
// ============================================================================

/// `TagWelsFuncPointerList` — `codec/encoder/core/inc/wels_func_ptr_def.h:198`.
/// 1280 bytes and 70 members in C++, in C++ declaration order.
#[repr(C)]
pub struct SWelsFuncPtrList {
    pub pfFillInterNeighborCache: PFillInterNeighborCacheFunc,

    pub pfGetVarianceFromIntraVaa: PGetVarianceFromIntraVaaFunc,
    pub pfGetMbSignFromInterVaa: PGetMbSignFromInterVaaFunc,
    pub pfUpdateMbMv: PUpdateMbMvFunc,
    pub pfFirstIntraMode: Option<PInterMdFirstIntraModeFunc>,
    pub pfIntraFineMd: Option<PIntraFineMdFunc>,
    pub pfInterFineMd: Option<PInterFineMdFunc>,
    pub pfInterMd: Option<PInterMdFunc>,

    pub pfInterMdBackgroundDecision: PInterMdBackgroundDecisionFunc,
    pub pfMdBackgroundInfoUpdate: PMdBackgroundInfoUpdateFunc,

    pub pfSCDPSkipDecision: PInterMdScrollingPSkipDecisionFunc,
    pub pfSetScrollingMv: Option<PSetScrollingMv>,

    pub sMcFuncs: SMcFunc,
    pub sSampleDealingFuncs: SSampleDealingFunc,
    pub pfGetLumaI16x16Pred: [Option<PGetLumaI16x16PredFunc>; I16_PRED_DC_A],
    pub pfGetLumaI4x4Pred: [Option<PGetLumaI4x4PredFunc>; I4_PRED_A],
    pub pfGetChromaPred: [Option<PGetChromaPredFunc>; C_PRED_A],

    pub pfMotionSearch: [Option<PMotionSearchFunc>; BLOCK_STATIC_IDC_ALL],
    /// The slots the search family reaches — see [`SMeFuncs`].
    pub sMeFuncs: SMeFuncs,

    pub pfInitializeHashforFeature: Option<PInitializeHashforFeatureFunc>,
    pub pfFillQpelLocationByFeatureValue: Option<PFillQpelLocationByFeatureValueFunc>,
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateBlockFeatureOfFrame: [Option<PCalculateBlockFeatureOfFrame>; 2],
    pub pfUpdateFMESwitch: Option<PUpdateFMESwitch>,

    pub pfCopy16x16Aligned: PCopyFunc,
    pub pfCopy16x16NotAligned: PCopyFunc,
    pub pfCopy8x8Aligned: PCopyFunc,
    pub pfCopy16x8NotAligned: PCopyFunc,
    pub pfCopy8x16Aligned: PCopyFunc,
    pub pfCopy4x4: PCopyFunc,
    pub pfCopy8x4: PCopyFunc,
    pub pfCopy4x8: PCopyFunc,

    pub pfDctT4: PDctFunc,
    pub pfDctFourT4: PDctFunc,

    pub pfCalculateSingleCtr4x4: PCalculateSingleCtrFunc,
    /// DC/AC
    pub pfScan4x4: PScanFunc,
    pub pfScan4x4Ac: PScanFunc,

    pub pfQuantization4x4: PQuantization4x4Func,
    pub pfQuantizationFour4x4: PQuantizationFunc,
    pub pfQuantizationDc4x4: PQuantizationDcFunc,
    pub pfQuantizationFour4x4Max: PQuantizationMaxFunc,
    pub pfQuantizationHadamard2x2: PQuantizationHadamardFunc,
    pub pfQuantizationHadamard2x2Skip: PQuantizationSkipFunc,

    pub pfTransformHadamard4x4Dc: PTransformHadamard4x4Func,

    pub pfGetNoneZeroCount: PGetNoneZeroCountFunc,

    pub pfDequantization4x4: PDeQuantization4x4Func,
    pub pfDequantizationFour4x4: PDeQuantizationFunc,
    pub pfDequantizationIHadamard4x4: PDeQuantizationHadamardFunc,

    /* For Deblocking */
    pub pfDeblocking: DeblockingFunc,

    pub pfRc: SWelsRcFunc,

    pub pfCavlcParamCal: PCavlcParamCalFunc,

    /// `pfWelsSpatialWriteMbSyn`, `pfGetBsPosition`, `pfStashMBStatus` and
    /// `pfStashPopMBStatus` (`wels_func_ptr_def.h:192-195`) were four slots set
    /// together by one `if`.
    pub eEntropyCoder: EntropyCoder,

    /// `IWelsParametersetStrategy*` — C++ declares an 8-byte pointer to a
    /// polymorphic object; here it is an owned `Option<Box<_>>`, which is also
    /// 8 bytes by the null-pointer niche and which has a `Drop`.
    ///
    /// The name keeps its C++ `p` for diffability, but this member **owns** its
    /// object: `None` is the uninstalled state (and the all-zero pattern
    /// `WelsMallocz` produces), and dropping the box is `WELS_DELETE_OP`. Because
    /// the table itself is `WelsMallocz`'d and `WelsFree`'d, *this struct's* drop
    /// glue never runs — so `WelsUninitEncoderExt` `take()`s the field explicitly,
    /// at the same point `encoder_ext.cpp:1995` deletes it.
    pub pParametersetStrategy:
        Option<Box<crate::encoder::paraset_strategy::CWelsParametersetIdStrategyObj>>,
}

pub type TagWelsFuncPointerList = SWelsFuncPtrList;

impl Default for SWelsFuncPtrList {
    /// The twenty-nine slots below that name a kernel are plain `fn`, not
    /// `Option<fn>`: every one of them is written unconditionally by an installer
    /// (`WelsInitEncodingFuncs`, `WelsInitReconstructionFuncs`,
    /// `InitIntraAnalysisVaaInfo`, `InitCoeffFunc`,
    /// `InitFillNeighborCacheInterFunc`) that `InitFunctionPointers` calls on every
    /// path, before any frame is touched, so `None` is a state no dispatch could
    /// observe.
    ///
    /// Where an installer chooses between two kernels on a coding parameter —
    /// `WelsInitBGDFunc`, `WelsInitSCDPskipFunc`, `InitFillNeighborCacheInterFunc`
    /// — the flag picks *which*, never *whether*, so the slot is still always set.
    /// `Default` names the **disabled** arm of each (`..PskipFalse`,
    /// `..InfoNULL`, `..WithoutBGD`): a table nobody has configured yet should
    /// read as "this feature is off", not as "background detection is on".
    ///
    /// The slots that keep their `Option` are the ones where absence is real: the
    /// predictor and motion-search **arrays** (indexed by a mode the table does not
    /// fill densely), the screen-content and background-detection slots that only
    /// some configurations install, the per-frame `pfIntraFineMd` / `pfInterMd`
    /// that `SetFastCodingFunc` re-aims, and `pParametersetStrategy`, whose `None`
    /// is a construction *failure* this function's caller turns into
    /// `ENC_RETURN_MEMALLOCERR`.
    fn default() -> Self {
        Self {
            pfFillInterNeighborCache: crate::encoder::md::FillNeighborCacheInterWithoutBGD,
            pfGetVarianceFromIntraVaa: crate::encoder::md::AnalysisVaaInfoIntra_c,
            pfGetMbSignFromInterVaa: crate::encoder::md::MdInterAnalysisVaaInfo_c,
            pfUpdateMbMv: crate::encoder::md::UpdateMbMv_c,
            pfFirstIntraMode: None,
            pfIntraFineMd: None,
            pfInterFineMd: None,
            pfInterMd: None,
            pfInterMdBackgroundDecision: crate::encoder::svc_mode_decision::WelsMdInterJudgeBGDPskipFalse,
            pfMdBackgroundInfoUpdate: crate::encoder::svc_mode_decision::WelsMdUpdateBGDInfoNULL,
            pfSCDPSkipDecision: crate::encoder::svc_mode_decision::WelsMdInterJudgeSCDPskipFalse,
            pfSetScrollingMv: None,
            sMcFuncs: SMcFunc::default(),
            sSampleDealingFuncs: SSampleDealingFunc::default(),
            pfGetLumaI16x16Pred: [None; I16_PRED_DC_A],
            pfGetLumaI4x4Pred: [None; I4_PRED_A],
            pfGetChromaPred: [None; C_PRED_A],
            pfMotionSearch: [None; BLOCK_STATIC_IDC_ALL],
            sMeFuncs: SMeFuncs::default(),
            pfInitializeHashforFeature: None,
            pfFillQpelLocationByFeatureValue: None,
            pfCalculateBlockFeatureOfFrame: [None; 2],
            pfUpdateFMESwitch: None,
            pfCopy16x16Aligned: crate::encoder::encode_mb_aux::WelsCopy16x16_c,
            pfCopy16x16NotAligned: crate::encoder::encode_mb_aux::WelsCopy16x16_c,
            pfCopy8x8Aligned: crate::encoder::encode_mb_aux::WelsCopy8x8_c,
            pfCopy16x8NotAligned: crate::encoder::encode_mb_aux::WelsCopy16x8_c,
            pfCopy8x16Aligned: crate::encoder::encode_mb_aux::WelsCopy8x16_c,
            pfCopy4x4: crate::encoder::encode_mb_aux::WelsCopy4x4_c,
            pfCopy8x4: crate::encoder::encode_mb_aux::WelsCopy8x4_c,
            pfCopy4x8: crate::encoder::encode_mb_aux::WelsCopy4x8_c,
            pfDctT4: crate::encoder::encode_mb_aux::WelsDctT4_c,
            pfDctFourT4: crate::encoder::encode_mb_aux::WelsDctFourT4_c,
            pfCalculateSingleCtr4x4: crate::encoder::encode_mb_aux::calculate_single_ctr_4x4,
            pfScan4x4: crate::encoder::encode_mb_aux::scan_4x4_dc_ac,
            pfScan4x4Ac: crate::encoder::encode_mb_aux::scan_4x4_ac,
            pfQuantization4x4: crate::encoder::encode_mb_aux::quant_4x4,
            pfQuantizationFour4x4: crate::encoder::encode_mb_aux::quant_four_4x4,
            pfQuantizationDc4x4: crate::encoder::encode_mb_aux::quant_4x4_dc,
            pfQuantizationFour4x4Max: crate::encoder::encode_mb_aux::quant_four_4x4_max,
            pfQuantizationHadamard2x2: crate::encoder::encode_mb_aux::hadamard_quant_2x2,
            pfQuantizationHadamard2x2Skip: crate::encoder::encode_mb_aux::hadamard_quant_2x2_skip,
            pfTransformHadamard4x4Dc: crate::encoder::encode_mb_aux::hadamard_t4_dc,
            pfGetNoneZeroCount: crate::encoder::encode_mb_aux::get_none_zero_count,
            pfDequantization4x4: crate::encoder::decode_mb_aux::dequant_4x4,
            pfDequantizationFour4x4: crate::encoder::decode_mb_aux::dequant_four_4x4,
            pfDequantizationIHadamard4x4: crate::encoder::decode_mb_aux::dequant_ihadamard_4x4,
            pfDeblocking: DeblockingFunc::default(),
            pfRc: SWelsRcFunc::default(),
            pfCavlcParamCal: crate::encoder::svc_set_mb_syn_cavlc::CavlcParamCal_c,
            eEntropyCoder: EntropyCoder::default(),
            pParametersetStrategy: None,
        }
    }
}

