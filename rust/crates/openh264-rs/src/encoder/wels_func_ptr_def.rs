#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

//! Encoder function-pointer table — `codec/encoder/core/inc/wels_func_ptr_def.h`.

#![forbid(unsafe_code)]

use crate::common::mc::SMcFunc;
use crate::encoder::deblocking::DeblockingFunc;
use crate::encoder::decode_mb_aux::{dequant_4x4, dequant_four_4x4, dequant_ihadamard_4x4};
use crate::encoder::encode_mb_aux::{
    PCalculateSingleCtrFunc, PCopyFunc, PDctFunc, PGetNoneZeroCountFunc, PQuantization4x4Func,
    PQuantizationDcFunc, PQuantizationFunc, PQuantizationHadamardFunc, PQuantizationMaxFunc,
    PQuantizationSkipFunc, PScanFunc, PTransformHadamard4x4Func,
};
use crate::encoder::encode_mb_aux::{
    WelsCopy4x4_c, WelsCopy4x8_c, WelsCopy8x4_c, WelsCopy8x8_c, WelsCopy8x16_c, WelsCopy16x8_c,
    WelsCopy16x16_c, WelsDctFourT4_c, WelsDctT4_c, calculate_single_ctr_4x4, get_none_zero_count,
    hadamard_quant_2x2, hadamard_quant_2x2_skip, hadamard_t4_dc, quant_4x4, quant_4x4_dc,
    quant_four_4x4, quant_four_4x4_max, scan_4x4_ac, scan_4x4_dc_ac,
};
use crate::encoder::encoder_context::{
    BLOCK_STATIC_IDC_ALL, C_PRED_A, I4_PRED_A, I16_PRED_DC_A, sWelsEncCtx,
};
use crate::encoder::md::{
    AnalysisVaaInfoIntra_c, FillNeighborCacheInterWithoutBGD, MdInterAnalysisVaaInfo_c, SMbCache,
    UpdateMbMv_c,
};
use crate::encoder::md::{
    PFillInterNeighborCacheFunc, PGetMbSignFromInterVaaFunc, PGetVarianceFromIntraVaaFunc,
    PUpdateMbMvFunc, SMB, SSampleDealingFunc, SWelsMD,
};
use crate::encoder::paraset_strategy::CWelsParametersetIdStrategyObj;
use crate::encoder::rc::SWelsRcFunc;
use crate::encoder::rec_view::RecCursor;
use crate::encoder::set_mb_syn_cabac::SCabacCtx;
use crate::encoder::svc_encode_mb::{PDeQuantization4x4Func, PDeQuantizationFunc};
use crate::encoder::svc_encode_slice::{BsWriter, SDqLayer, SDynamicSlicingStack, SSlice};
use crate::encoder::svc_mode_decision::{
    WelsMdInterJudgeBGDPskipFalse, WelsMdInterJudgeSCDPskipFalse, WelsMdUpdateBGDInfoNULL,
};
use crate::encoder::svc_motion_estimate::{
    PCalculateBlockFeatureOfFrame, PFillQpelLocationByFeatureValueFunc,
    PInitializeHashforFeatureFunc, PMotionSearchFunc, PUpdateFMESwitch, SMeFuncs,
};
use crate::encoder::svc_set_mb_syn_cabac::WelsSpatialWriteMbSynCabac;
use crate::encoder::svc_set_mb_syn_cavlc::{
    CavlcParamCal_c, GetBsPosCabac, GetBsPosCavlc, StashMBStatusCabac, StashMBStatusCavlc,
    StashPopMBStatusCabac, StashPopMBStatusCavlc,
};
use crate::encoder::wels_preprocess::SVAAFrameInfoExt;
use crate::safe::mb_grid::{MbSplit, MbWindow};

// ============================================================================
// Function pointer typedefs
// ============================================================================

/// Intra predictor for the 4x4 luma prediction block. There is one type per block
/// size — 16, 64 or 256 bytes — so a predictor cannot be installed into the wrong
/// table. `rec` is the reconstruction picture, read and never written.
pub type PGetLumaI4x4PredFunc = fn(pred: &mut [u8; 16], rec: &RecCursor<'_>);
/// [`PGetLumaI4x4PredFunc`] for the 8x8 chroma prediction block.
pub type PGetChromaPredFunc = fn(pred: &mut [u8; 64], rec: &RecCursor<'_>);
/// [`PGetLumaI4x4PredFunc`] for the 16x16 luma prediction block.
pub type PGetLumaI16x16PredFunc = fn(pred: &mut [u8; 256], rec: &RecCursor<'_>);

pub type PIntraFineMdFunc = fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32;

/// The context and the mode-decision record share a lifetime: the body resolves the
/// reference picture through the context, and `SWelsMD`'s cursors point into it.
pub type PInterFineMdFunc = for<'a> fn(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
    bestCost: i32,
);

pub type PInterMdFirstIntraModeFunc = fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool;

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

pub type PInterMdScrollingPSkipDecisionFunc = fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
) -> bool;

/// `pVaaExt` is shared, not exclusive: this slot is fork-reachable, so N workers hold
/// it at once over the one video-analysis block they all share. The implementation only
/// reads two scalars off `sScrollDetectInfo`; everything it writes goes through `pMd`,
/// which is exclusive and per-macroblock.
///
/// It carries the extension rather than the base block, reached through
/// `sWelsEncCtx::vaa_ext_ref`, which answers `Some` only under screen content. `None`
/// is unreachable once a real body is installed, since the installer requires the
/// extension.
pub type PSetScrollingMv = fn(pVaaExt: Option<&SVAAFrameInfoExt>, pMd: &mut SWelsMD<'_>);

/// The context and the mode-decision record share a lifetime, as for
/// [`PInterFineMdFunc`].
pub type PInterMdFunc = for<'a> fn(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    slice: &mut SSlice,
    mbs: &mut MbSplit<'_, SMB>,
);

pub type PDeQuantizationHadamardFunc = fn(pRes: &mut [i16; 16], kuiMF: u16);

/// `pCoff` is a slice, not a fixed array, because the two call families walk different
/// extents: luma steps `sDct.iLumaBlock` in sixteens with `iEndIdx = 15`, chroma DC
/// steps `sDct.iChromaDc` (`[[i16; 4]; 2]`) in fours with `iEndIdx = 3`, where a
/// `&[i16; 16]` would reach twelve elements past the end. The backward scan starts at
/// `iEndIdx` inside the slice.
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
/// Carries the four dispatches `pfWelsSpatialWriteMbSyn`, `pfGetBsPosition`,
/// `pfStashMBStatus` and `pfStashPopMBStatus`, which are one configuration rather than
/// four independent choices.
///
/// The discriminants are `iEntropyCodingModeFlag`'s own values, so `Cavlc` is the zero
/// one and hence the default.
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum EntropyCoder {
    #[default]
    Cavlc = 0,
    Cabac = 1,
}

impl EntropyCoder {
    /// CABAC when `iEntropyCodingModeFlag != 0`, CAVLC otherwise.
    #[inline]
    pub fn from_flag(iEntropyCodingModeFlag: i32) -> Self {
        if iEntropyCodingModeFlag != 0 {
            EntropyCoder::Cabac
        } else {
            EntropyCoder::Cavlc
        }
    }

    /// True for CABAC, for the call sites that branch on the mode itself rather than on
    /// what it dispatches to (the CAVLC-only stash before a re-encode,
    /// `WelsInitSliceCabac`).
    #[inline]
    pub fn is_cabac(self) -> bool {
        self == EntropyCoder::Cabac
    }

    /// `pfWelsSpatialWriteMbSyn` — writes one macroblock's syntax elements.
    ///
    /// `mbs` is this slice's records so far with the current one last: both writers read
    /// same-slice neighbours for context modelling and write the current record's QP and
    /// MVD state.
    #[inline]
    pub fn WelsSpatialWriteMbSyn(
        self,
        pEncCtx: &sWelsEncCtx,
        pSlice: &mut SSlice,
        mbs: &mut MbWindow<'_, SMB>,
        pSliceBsBuf: &mut [u8],
        pCtxOutBs: &mut Option<&mut BsWriter>,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => crate::encoder::svc_set_mb_syn_cavlc::WelsSpatialWriteMbSyn(
                pEncCtx,
                pSlice,
                mbs,
                pSliceBsBuf,
                pCtxOutBs,
            ),
            EntropyCoder::Cabac => {
                WelsSpatialWriteMbSynCabac(pEncCtx, pSlice, mbs, pSliceBsBuf, pCtxOutBs)
            }
        }
    }

    /// `pfStashMBStatus` — snapshots the coder state before a macroblock, so an
    /// overflow or a slice-boundary step-back can re-encode it.
    ///
    /// `buf` is the slice's output buffer, used by the CABAC arm only; `pBs` is the
    /// slice's writer (`slice_bs_writer`), used by the CAVLC arm only. `pBs` must be
    /// that writer and `buf` the buffer it is positioned in.
    #[inline]
    pub fn StashMBStatus(
        self,
        buf: &mut [u8],
        pBs: &mut BsWriter,
        pDss: &mut SDynamicSlicingStack<'_>,
        pCabacCtx: &mut SCabacCtx,
        kuiLastMbQp: u8,
        iMbSkipRun: i32,
    ) {
        match self {
            EntropyCoder::Cavlc => StashMBStatusCavlc(pBs, pDss, kuiLastMbQp, iMbSkipRun),
            EntropyCoder::Cabac => {
                StashMBStatusCabac(buf, pDss, pCabacCtx, kuiLastMbQp, iMbSkipRun)
            }
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
        pCabacCtx: &mut SCabacCtx,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => StashPopMBStatusCavlc(pBs, pDss),
            EntropyCoder::Cabac => StashPopMBStatusCabac(buf, pDss, pCabacCtx),
        }
    }

    /// `pfGetBsPosition` — the slice writer's bit position, in the units each coder
    /// counts in. Needs no buffer on either arm: CAVLC reads the writer's own
    /// position (`pBs`, from `slice_bs_writer`) and CABAC subtracts two offsets held
    /// in the slice's coder state.
    #[inline]
    pub fn GetBsPosition(self, pBs: &BsWriter, pCabacCtx: &SCabacCtx) -> i32 {
        match self {
            EntropyCoder::Cavlc => GetBsPosCavlc(pBs),
            EntropyCoder::Cabac => GetBsPosCabac(pCabacCtx),
        }
    }
}

// ============================================================================
// SWelsFuncPtrList
// ============================================================================

/// `TagWelsFuncPointerList` — `codec/encoder/core/inc/wels_func_ptr_def.h`.
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

    /// Dispatches `pfWelsSpatialWriteMbSyn`, `pfGetBsPosition`, `pfStashMBStatus` and
    /// `pfStashPopMBStatus`.
    pub eEntropyCoder: EntropyCoder,

    /// `IWelsParametersetStrategy*`, owned. `None` is the uninstalled state.
    ///
    /// This struct's drop glue never runs, so `WelsUninitEncoderExt` `take()`s the field
    /// explicitly to free the object.
    pub pParametersetStrategy: Option<Box<CWelsParametersetIdStrategyObj>>,
}

pub type TagWelsFuncPointerList = SWelsFuncPtrList;

impl Default for SWelsFuncPtrList {
    /// The kernel slots are plain `fn`, not `Option<fn>`: every one is written
    /// unconditionally by an installer (`WelsInitEncodingFuncs`,
    /// `WelsInitReconstructionFuncs`, `InitIntraAnalysisVaaInfo`, `InitCoeffFunc`,
    /// `InitFillNeighborCacheInterFunc`) that `InitFunctionPointers` calls on every path
    /// before any frame is touched.
    ///
    /// Where an installer chooses between two kernels on a coding parameter
    /// (`WelsInitBGDFunc`, `WelsInitSCDPskipFunc`, `InitFillNeighborCacheInterFunc`) the
    /// flag picks which, never whether, so the slot is still always set. The defaults
    /// name the disabled arm of each (`..PskipFalse`, `..InfoNULL`, `..WithoutBGD`), so
    /// an unconfigured table reads as "this feature is off".
    ///
    /// The slots that keep their `Option` are the ones where absence is real: the
    /// predictor and motion-search arrays, which are not filled densely; the
    /// screen-content and background-detection slots that only some configurations
    /// install; the per-frame `pfIntraFineMd`/`pfInterMd` that `SetFastCodingFunc`
    /// re-aims; and `pParametersetStrategy`, whose `None` is a construction failure the
    /// caller turns into `ENC_RETURN_MEMALLOCERR`.
    fn default() -> Self {
        Self {
            pfFillInterNeighborCache: FillNeighborCacheInterWithoutBGD,
            pfGetVarianceFromIntraVaa: AnalysisVaaInfoIntra_c,
            pfGetMbSignFromInterVaa: MdInterAnalysisVaaInfo_c,
            pfUpdateMbMv: UpdateMbMv_c,
            pfFirstIntraMode: None,
            pfIntraFineMd: None,
            pfInterFineMd: None,
            pfInterMd: None,
            pfInterMdBackgroundDecision: WelsMdInterJudgeBGDPskipFalse,
            pfMdBackgroundInfoUpdate: WelsMdUpdateBGDInfoNULL,
            pfSCDPSkipDecision: WelsMdInterJudgeSCDPskipFalse,
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
            pfCopy16x16Aligned: WelsCopy16x16_c,
            pfCopy16x16NotAligned: WelsCopy16x16_c,
            pfCopy8x8Aligned: WelsCopy8x8_c,
            pfCopy16x8NotAligned: WelsCopy16x8_c,
            pfCopy8x16Aligned: WelsCopy8x16_c,
            pfCopy4x4: WelsCopy4x4_c,
            pfCopy8x4: WelsCopy8x4_c,
            pfCopy4x8: WelsCopy4x8_c,
            pfDctT4: WelsDctT4_c,
            pfDctFourT4: WelsDctFourT4_c,
            pfCalculateSingleCtr4x4: calculate_single_ctr_4x4,
            pfScan4x4: scan_4x4_dc_ac,
            pfScan4x4Ac: scan_4x4_ac,
            pfQuantization4x4: quant_4x4,
            pfQuantizationFour4x4: quant_four_4x4,
            pfQuantizationDc4x4: quant_4x4_dc,
            pfQuantizationFour4x4Max: quant_four_4x4_max,
            pfQuantizationHadamard2x2: hadamard_quant_2x2,
            pfQuantizationHadamard2x2Skip: hadamard_quant_2x2_skip,
            pfTransformHadamard4x4Dc: hadamard_t4_dc,
            pfGetNoneZeroCount: get_none_zero_count,
            pfDequantization4x4: dequant_4x4,
            pfDequantizationFour4x4: dequant_four_4x4,
            pfDequantizationIHadamard4x4: dequant_ihadamard_4x4,
            pfDeblocking: DeblockingFunc::default(),
            pfRc: SWelsRcFunc::default(),
            pfCavlcParamCal: CavlcParamCal_c,
            eEntropyCoder: EntropyCoder::default(),
            pParametersetStrategy: None,
        }
    }
}
