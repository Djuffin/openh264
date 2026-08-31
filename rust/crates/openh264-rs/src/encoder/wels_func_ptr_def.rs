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
use crate::encoder::wels_preprocess::SVAAFrameInfo;

// ============================================================================
// Function pointer typedefs that had no Rust counterpart
// ============================================================================

/// `wels_func_ptr_def.h:178`, **safe and split three ways since T9.C2**.
///
/// The C++ has one `PGetIntraPredFunc` serving all three tables, and the port had
/// one `unsafe extern "C" fn(*mut u8, *mut u8, i32)` doing the same. Both are the
/// F113/S52 shape: *one type serving several lengths*. The destination is a packed
/// prediction block whose size is fixed per table — 16, 64 or 256 bytes — so the
/// safe form names the size, and a chroma predictor can no longer be installed
/// into the luma table by a slip of the index.
///
/// The reference is the **reconstruction picture**, read and never written, so it
/// arrives as the seam's read cursor. Note what the type change buys beyond
/// soundness: `reference`/`top_row` used to turn `(pRef, kiStride)` into a slice
/// under a hand-written `# Safety` contract naming each kernel's reach; a
/// `RecCursor` bounds-checks every access against the whole plane allocation, so
/// the reach constants below are now a *correctness* statement about which
/// neighbours must be available, not a memory-safety one.
pub type PGetLumaI4x4PredFunc = fn(pred: &mut [u8; 16], rec: &RecCursor<'_>);
/// [`PGetLumaI4x4PredFunc`] for the 8x8 chroma prediction block.
pub type PGetChromaPredFunc = fn(pred: &mut [u8; 64], rec: &RecCursor<'_>);
/// [`PGetLumaI4x4PredFunc`] for the 16x16 luma prediction block.
pub type PGetLumaI16x16PredFunc = fn(pred: &mut [u8; 256], rec: &RecCursor<'_>);

/// `wels_func_ptr_def.h:106`
pub type PIntraFineMdFunc = unsafe fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32;

/// `wels_func_ptr_def.h:107`
pub type PInterFineMdFunc = for<'a> unsafe fn(
    // **S10.8: the context and the mode-decision record share a lifetime.** The
    // fine-partition body resolves the reference picture through the context now
    // (`layer_ref_pic` takes it since `SDqLayer::pRefList` went), and that picture
    // is what `SWelsMD`'s cursors point into — so the slot has to say the two
    // outlive each other, where before the layer's own raw field hid the tie.
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
    bestCost: i32,
);

/// `wels_func_ptr_def.h:108`
pub type PInterMdFirstIntraModeFunc = unsafe fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool;

// `PAccumulateSadFunc` (`wels_func_ptr_def.h:111`) stood here, with its
// `pfAccumulateSadForRc` slot below. **S18, deleted in S4.C1**: the slot was
// never installed and never dispatched — whole-tree grep at deletion found the
// type declaration, the field declaration and the one `None` initialiser and
// nothing else, in `src/`, `tests/`, `benches/` or `rust/tools/`. It is the
// rate-control SAD accumulator the C++ assigns in `WelsInitEncodingFuncs`; the
// port's rate control reads its SADs directly, so the indirection never had a
// producer. One `unsafe` fn-pointer alias retires without a conversion.

/// `wels_func_ptr_def.h:116`
/// **S4.C1**: `pKeepPskip` was `*mut bool` — a pure out-parameter whose single
/// dispatch site passes `&mut` on a local `bool`, and whose two implementations
/// only read-modify-write it. The leading context stays raw: this slot is
/// dispatched inside the fork, so S63 keeps it a pointer until the root converts.
/// `extern "C"` came off — nothing in this table crosses the C ABI (T4b.1).
pub type PInterMdBackgroundDecisionFunc = unsafe fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
    pKeepPskip: &mut bool,
) -> bool;

/// `wels_func_ptr_def.h:118`
pub type PMdBackgroundInfoUpdateFunc = unsafe extern "C" fn(
    // S10.8: the context, because the body resolves the reference picture through
    // it now that `SDqLayer::pRefList` is gone.
    pEncCtx: &sWelsEncCtx,
    pCurLayer: &SDqLayer,
    pCurMb: &mut SMB,
    bFlag: bool,
    kiRefPictureType: i32,
);

/// `wels_func_ptr_def.h:121`
pub type PInterMdScrollingPSkipDecisionFunc = unsafe fn(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
) -> bool;

/// `wels_func_ptr_def.h:123`
/// **S4.C1: shared, and deliberately not `&mut`.** `SetScrollingMvToMdNull` is
/// fork-reachable (`phase9_forksplit.py --why`: `<- WelsMdInterSecondaryModesEnc
/// <- WelsMdInterMb <- ... <- EncodeOnePartitionSizeLimited <- fork seed`), so an
/// exclusive reference here would be N workers each taking a `Unique` retag over
/// the one video-analysis block every worker shares — F223's second defect
/// verbatim, and a data race whether or not anything is written through it. The
/// real implementation only *reads* the block (the screen-content downcast, two
/// scalars off `sScrollDetectInfo`); everything it writes goes through `pMd`,
/// which is already exclusive and per-macroblock.
pub type PSetScrollingMv = unsafe fn(pVaa: &SVAAFrameInfo, pMd: &mut SWelsMD<'_>);

/// `wels_func_ptr_def.h:125`
pub type PInterMdFunc = for<'a> unsafe fn(
    // S10.8, as `PInterFineMdFunc`: the reference picture is resolved through the
    // context now, and `SWelsMD`'s cursors point into it, so the slot says the two
    // share a lifetime.
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    slice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
);

/// `wels_func_ptr_def.h:64`
pub type PDeQuantizationHadamardFunc = fn(pRes: &mut [i16; 16], kuiMF: u16);

/// `wels_func_ptr_def.h:190`
/// **S4.C1: safe, and `pCoff` is a slice rather than a fixed array on purpose.**
/// The two call families walk different extents off the same flat cursor —
/// luma steps `sDct.iLumaBlock` (a `[[i16; 16]; 16]` read flat) in sixteens with
/// `iEndIdx = 15`, chroma DC steps `sDct.iChromaDc` (`[[i16; 4]; 2]`) in fours
/// with `iEndIdx = 3`. A `&[i16; 16]` would reach twelve elements past
/// `iChromaDc[1]`; the slice carries the extent the caller actually owns, and
/// the implementation's backward scan starts at `iEndIdx` inside it.
///
/// `pRun`/`pLevel` are the caller's own `[0u8; 16]` / `[0i16; 16]` locals, so
/// the fixed-array shape is exact there. `extern "C"` came off with the raws:
/// nothing in this table crosses the C ABI (T4b.1's precedent).
pub type PCavlcParamCalFunc = fn(
    pCoff: &[i16],
    pRun: &mut [u8; 16],
    pLevel: &mut [i16; 16],
    pTotalCoeffs: &mut i32,
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
    /// The record comes as the grid window (E3): both writers read same-slice
    /// neighbours for context modelling and write the current record's QP and
    /// MVD state, so `mbs` is exactly "my slice's records so far, current last".
    ///
    /// # Safety
    /// As the two implementations: `pEncCtx` and `pSlice` must be live and the
    /// slice's writer positioned in the buffer `slice_bs_buffer` returns.
    #[inline]
    // unsafe-cat: fork-shared(S63)
    #[allow(unsafe_code)]
    pub unsafe fn WelsSpatialWriteMbSyn(
        self,
        pEncCtx: &sWelsEncCtx,
        pSlice: &mut SSlice,
        mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    ) -> i32 {
        match self {
            EntropyCoder::Cavlc => {
                crate::encoder::svc_set_mb_syn_cavlc::WelsSpatialWriteMbSyn(pEncCtx, pSlice, mbs)
            }
            EntropyCoder::Cabac => crate::encoder::svc_set_mb_syn_cabac::WelsSpatialWriteMbSynCabac(
                pEncCtx, pSlice, mbs,
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
    // unsafe-cat: fork-shared(S63)
    /// **T9.E6**: `pSlice` became the three things the two arms touch — the
    /// CABAC coder state, the last macroblock QP as a value, and (on the pop
    /// side, at the call sites) the restore of that QP — so no argument of
    /// this call names `SSlice` and nothing here retags the slice when the
    /// family flips (the two shape-B sites q1c reported here were exactly the
    /// `slice_writer` result held across the future `&mut *pSlice` argument).
    #[allow(unsafe_code)]
    pub unsafe fn StashMBStatus(
        self,
        buf: &mut [u8],
        pBs: &mut BsWriter,
        pDss: &mut SDynamicSlicingStack,
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
    /// # Safety
    /// As [`StashMBStatus`].
    ///
    /// [`StashMBStatus`]: EntropyCoder::StashMBStatus
    #[inline]
    // unsafe-cat: fork-shared(S63)
    /// **T9.E6**, as [`StashMBStatus`]: the caller restores
    /// `uiLastMbQp` from `sDss` beside the call — it owns both.
    ///
    /// [`StashMBStatus`]: EntropyCoder::StashMBStatus
    #[allow(unsafe_code)]
    pub unsafe fn StashPopMBStatus(
        self,
        buf: &mut [u8],
        pBs: &mut BsWriter,
        pDss: &mut SDynamicSlicingStack,
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
    /// position (`pBs`, from `slice_writer`) and CABAC subtracts two offsets held
    /// in the slice's coder state (T3.5).
    ///
    /// # Safety
    /// `pSlice` must be live and `pBs` must be its writer.
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
    pub pfGetLumaI16x16Pred: [Option<PGetLumaI16x16PredFunc>; I16_PRED_DC_A],
    pub pfGetLumaI4x4Pred: [Option<PGetLumaI4x4PredFunc>; I4_PRED_A],
    pub pfGetChromaPred: [Option<PGetChromaPredFunc>; C_PRED_A],

    // `pfSampleSadHor8: [Option<PSampleSadHor8Func>; 2]` stood here — the
    // screen-content SIMD horizontal-SAD pair. Zero writers and zero readers in
    // the whole tree (the C++ fills it only from SSE4.1 kernels this port does
    // not have). S18, session F step 0.
    pub pfMotionSearch: [Option<PMotionSearchFunc>; BLOCK_STATIC_IDC_ALL],
    /// The slots the search family reaches — see [`SMeFuncs`]. Same six
    /// members the table carried flat until session F, regrouped so the five
    /// de-virtualized typedefs can take `&SMeFuncs` instead of the table.
    pub sMeFuncs: SMeFuncs,

    pub pfInitializeHashforFeature: Option<PInitializeHashforFeatureFunc>,
    pub pfFillQpelLocationByFeatureValue: Option<PFillQpelLocationByFeatureValueFunc>,
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateBlockFeatureOfFrame: [Option<PCalculateBlockFeatureOfFrame>; 2],
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

    pub pfQuantization4x4: Option<PQuantization4x4Func>,
    pub pfQuantizationFour4x4: Option<PQuantizationFunc>,
    pub pfQuantizationDc4x4: Option<PQuantizationDcFunc>,
    pub pfQuantizationFour4x4Max: Option<PQuantizationMaxFunc>,
    pub pfQuantizationHadamard2x2: Option<PQuantizationHadamardFunc>,
    pub pfQuantizationHadamard2x2Skip: Option<PQuantizationSkipFunc>,

    pub pfTransformHadamard4x4Dc: Option<PTransformHadamard4x4Func>,

    pub pfGetNoneZeroCount: Option<PGetNoneZeroCountFunc>,

    pub pfDequantization4x4: Option<PDeQuantization4x4Func>,
    pub pfDequantizationFour4x4: Option<PDeQuantizationFunc>,
    pub pfDequantizationIHadamard4x4: Option<PDeQuantizationHadamardFunc>,
    // `pfIDctFourT4`/`pfIDctT4`/`pfIDctI16x16Dc` stood here — installed by
    // `WelsInitReconstructionFuncs`, asserted `is_some()`, and never called
    // (F138/F139): the reconstruction writes go through the seam's kernels
    // directly since T9.C2. S18, session F step 0; the kernels stay (the
    // differential tests drive them), only the write-only slots go.

    /* For Deblocking */
    pub pfDeblocking: DeblockingFunc,
    // `pfSetNZCZero` stood here — one writer (`WelsBlockFuncInit`), one
    // reader (`DeblockingBSCalc_c`), the reader direct since session F (F118).

    pub pfRc: SWelsRcFunc,

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
    /// **T6.I1 — field-wise, replacing `{ mem::zeroed() }`.**
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
            pfMotionSearch: [None; BLOCK_STATIC_IDC_ALL],
            sMeFuncs: SMeFuncs::default(),
            pfInitializeHashforFeature: None,
            pfFillQpelLocationByFeatureValue: None,
            pfCalculateBlockFeatureOfFrame: [None; 2],
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
            pfDeblocking: DeblockingFunc::default(),
            pfRc: SWelsRcFunc::default(),
            pfCavlcParamCal: None,
            eEntropyCoder: EntropyCoder::default(),
            pParametersetStrategy: None,
        }
    }
}
