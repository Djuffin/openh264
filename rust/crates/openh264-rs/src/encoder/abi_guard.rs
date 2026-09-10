#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

//! Compile-time size checks for encoder-internal structs.
//!
//! The internal counterpart to `api/abi_guard.rs`, which guards the public C ABI.
//! Nothing outside the crate depends on these layouts; a size mismatch here means a
//! field was added, dropped, or given the wrong width. The sizes hold on any LP64
//! target.
//!
//! `SWelsPPS` excludes the nine FMO fields inside `#if !defined(DISABLE_FMO_FEATURE)`:
//! `as264_common.h:53` defines that macro unconditionally, so they are not in the
//! struct the C++ encoder actually compiles.

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use std::mem::size_of;

use crate::common::mc::SMcFunc;
use crate::common::wels_common_defs::{SNalUnitHeader, SNalUnitHeaderExt};
use crate::encoder::encoder_context::SParaSetOffset;
use crate::encoder::encoder_context::{SCropOffset, SDCTCoeff, SMVComponentUnit, SMVUnitXY};
use crate::encoder::encoder_context::{SLTRState, SSpatialPicIndex, SStrideTables, sWelsEncCtx};
use crate::encoder::md::{SMB, SMbCache, SMeRefinePointer, SSampleDealingFunc, SWelsMD};
use crate::encoder::nal_encap::SWelsNalRaw;
use crate::encoder::param_svc::{SSpatialLayerInternal, SWelsSvcCodingParam};
use crate::encoder::param_svc::{SSpsSvcExt, SSubsetSps, SWelsPPS, SWelsSPS};
use crate::encoder::picture::{SPicture, SScreenBlockFeatureStorage};
use crate::encoder::rc::{SRCSlicing, SWelsSvcRc};
use crate::encoder::ref_list_mgr_svc::{SLTRMarkingFeedback, SLTRRecoverRequest};
use crate::encoder::ref_list_mgr_svc::{SRefPicListReorderSyntax, SRefPicMarking};
use crate::encoder::set_mb_syn_cabac::{SCabacCtx, SStateCtx};
use crate::encoder::slice_multi_threading::SSliceCtx;
use crate::encoder::svc_encode_slice::{SDqLayer, SLayerInfo, SSliceBufferInfo};
use crate::encoder::svc_encode_slice::{SSliceHeader, SSliceHeaderExt};
use crate::encoder::svc_motion_estimate::SWelsME;
use crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics;
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
use crate::encoder::wels_preprocess::{
    SAdaptiveQuantizationParam, SComplexityAnalysisParam, SComplexityAnalysisScreenParam,
    SScrollDetectionParam, SVAACalcResult, SVAAFrameInfo, SVAAFrameInfoExt,
};

macro_rules! assert_size {
    ($t:ty, $n:expr) => {
        const _: () = assert!(
            size_of::<$t>() == $n,
            concat!(stringify!($t), " must match the C++ struct size"),
        );
    };
}

// ---------------------------------------------------------------------------
// Profile-split pins
//
// `Option<SrcPicId>` / `Option<RecPicId>` are 4 bytes in a release build and 8 in a
// debug build: `pool::Id` carries a generation counter under `debug_assertions`, and the
// `NonZeroU32` niche keeps the `Option` free either way. So every struct that stores a
// picture handle has two sizes, and every offset after the first handle has two values.
// ---------------------------------------------------------------------------
macro_rules! assert_size_by_profile {
    ($t:ty, debug $d:expr, release $r:expr) => {
        #[cfg(debug_assertions)]
        const _: () = assert!(size_of::<$t>() == $d, concat!(stringify!($t), " (debug)"));
        #[cfg(not(debug_assertions))]
        const _: () = assert!(size_of::<$t>() == $r, concat!(stringify!($t), " (release)"));
    };
}

macro_rules! assert_ctx_offset_by_profile {
    ($field:ident, debug $d:expr, release $r:expr) => {
        #[cfg(debug_assertions)]
        const _: () = assert!(std::mem::offset_of!(sWelsEncCtx, $field) == $d);
        #[cfg(not(debug_assertions))]
        const _: () = assert!(std::mem::offset_of!(sWelsEncCtx, $field) == $r);
    };
}

// codec/common/inc/wels_common_defs.h
assert_size!(SNalUnitHeader, 12);
assert_size!(SNalUnitHeaderExt, 24);

// codec/encoder/core/inc/nal_encap.h
//
// `SWelsNalRaw` does not carry `pRawData`: it was a cache of `buffer + iStartPos`, and
// the record keeps the offset while `WelsEncodeNal`'s caller names the buffer.
assert_size!(SWelsNalRaw, 32);

// codec/encoder/core/inc/picture.h
//
// The four per-macroblock side arrays (`uiRefMbType`, `pRefMbQp`, `pMbSkipSad`,
// `sMvList`) are owned `Vec`s and the sample memory is three owned `PaddedPlane`s, so
// `#[repr(C)]` is off: the picture owns every byte it has.
assert_size!(SPicture, 344);

// codec/encoder/core/inc/encoder_context.h — `SRefList` owns its pictures: 34 handles,
// a pool and two counts, profile-split like every struct that stores a handle.
assert_size_by_profile!(
    crate::encoder::encoder_context::SRefList,
    debug 240,
    release 120
);
// The storage owns its five buffers and crosses no ABI. The layer's scratch
// (`SFeatureSearchPreparation::pFeatureOfBlock`) is not held here either —
// `CalculateFeatureOfBlock` takes it as `&mut [u16]`.
assert_size!(SScreenBlockFeatureStorage, 136);

// codec/encoder/core/inc/parameter_sets.h
assert_size!(SWelsSPS, 56);
assert_size!(SWelsPPS, 16);
assert_size!(SSpsSvcExt, 4);
assert_size!(SSubsetSps, 60);

// codec/encoder/core/inc/slice.h. `SSliceHeader` excludes `iSliceGroupChangeCycle`,
// which sits inside `#if !defined(DISABLE_FMO_FEATURE)` at slice.h:124. Neither it nor
// `SSliceHeaderExt` carries a parameter-set pointer: `iSpsId` / `iPpsId` stand in.
assert_size!(SRefPicMarking, 100);
assert_size!(SRefPicListReorderSyntax, 16);
assert_size!(SSliceHeader, 152);
assert_size!(SSliceHeaderExt, 168);

// codec/encoder/core/inc/wels_common_basis.h, mb_cache.h
assert_size!(SMVUnitXY, 4);
assert_size!(SCropOffset, 8);
assert_size!(SDCTCoeff, 816);
assert_size!(SMVComponentUnit, 146);

// SStateCtx is a single packed byte (set_mb_syn_cabac.h:55) — as two u8 fields it
// would double sWelsCabacContexts[4][52][460].
assert_size!(SStateCtx, 1);
assert_size!(SCabacCtx, 504);
assert_size!(SLTRState, 60);
assert_size!(SLTRMarkingFeedback, 16);
assert_size!(SLTRRecoverRequest, 20);
assert_size!(SMcFunc, 48);
// `pfMdCost` and `pfMeCost` are `CostFamily` tags where the C++ has two function
// pointers into this same struct's sibling arrays; the eight `pfIntra*Combined3*` slots,
// `*mut c_void` never assigned on any target, are not carried.
assert_size!(SSampleDealingFunc, 176);
assert_size!(SRCSlicing, 44);
// Owns its block as two typed stores — `Vec<[i32; 24]>` and `Vec<i16>` — where the C++
// holds sixteen raw pointers into one block it does not own.
assert_size!(SStrideTables, 208);
// The three per-block plane cursors (`pEncMb`/`pRefMb`/`pColoRefMb`) are not carried:
// the coordinates already in the struct carry the same information and the search family
// takes the planes as parameters. `pMvdCost` is a `MvdCostCursor` — the table as a
// `&[u16]` plus the index the raw cursor points at — because `COST_MVD` indexes with a
// signed MVD, which parks the pointer mid-table.
assert_size!(SWelsME, 80);
// `sMe4x4`/`sMe8x4`/`sMe4x8` are not carried: upstream's sub-8x8 search is inside
// `#if 0` (`svc_mode_decision.cpp:634-661`), so nothing ever produced those partitions.
// What is left is the `sMe` container's nine embedded `SWelsME` (sMe16x16 + sMe16x8[2] +
// sMe8x16[2] + sMe8x8[4]) and `pMvdCost`, plus three fields with no C++ counterpart:
// `sctx`, the slice context the P-slice mode decision re-resolves per macroblock; `mbc`,
// the macroblock's nine plane cursors; and `mbi`, the reference picture's three entries
// for this macroblock. All three are the C++'s pointer arithmetic off
// `pCurLayer`/`kiMbX`/`kiMbY`, hoisted out of the macroblock loop.
assert_size!(SWelsMD, 1208);
// `SVAAFrameInfo` is not `repr(C)`, so this pin is a drift tracker rather than an ABI
// contract. The six per-frame result arrays and `pVaaBackgroundMbFlag` are owned `Vec`s;
// `pMotionTextureUnit` and `pMotionTextureIndexToDeltaQp` are owned here rather than
// pointed at from `sAdaptiveQuantParam`; and the six `*mut u8` plane roots are two
// `Option<RoPicView>`. The VAA result and the parameter blocks' buffers reach each plugin
// as slices at its `Process` call, so no `pCalcResult` pointer is stored.
// `SVAAFrameInfo` is `Sync`; `pCurY`/`pRefY` are `usize`.
assert_size!(SVAAFrameInfo, 504);
// Embeds `SVAAFrameInfo`. Its screen block-static family is owned rather than pointed
// at: `pVaaBlockStaticIdc` is one `SBlockStaticIdcStore` where the C++ has sixteen
// `*mut u8`, `pVaaBestBlockStaticIdc` is a row number, and the same swap inside
// `SRefInfoParam` is paid once per slot of `sVaaStrBestRefCandidate` and
// `sVaaLtrBestRefCandidate`.
assert_size!(SVAAFrameInfoExt, 1680);

// `SSliceThreading` is deliberately NOT asserted: C++ (mt_defs.h:68) embeds
// `pthread_cond_t` and `pthread_mutex_t` by value, whose sizes are libc-specific, and
// this port models the primitives as opaque handles.

// `SVAACalcResult`: six owned arrays where the C++ has six pointers.
assert_size!(SVAACalcResult, 168);
assert_size!(SScrollDetectionParam, 32);
// Its two buffer pointers are `SVAAFrameInfo`'s owned `Vec`s, and they and the VAA
// result reach the plugin at the `Process` call rather than being stored here.
assert_size!(SAdaptiveQuantizationParam, 8);
// `pGomComplexity`, `pGomForegroundBlockNum`, `pBackgroundMbFlag` and `uiRefMbType`
// reach the plugin as slices instead, as for `SAdaptiveQuantizationParam` above.
assert_size!(SComplexityAnalysisParam, 24);

// `pGomComplexity` is not carried — the screen plugin takes the GOM array as a slice at
// the call, as the camera one does, which is what lets `SVAAFrameInfoExt` be `Sync`.
assert_size!(SComplexityAnalysisScreenParam, 56);

// Mid-tier types.
assert_size!(SSpatialLayerInternal, 68);
// `pCurPath` (`param_svc.h:118`) is not carried: a `char*` with three writes and no
// reader in either tree. This is `param_svc.h`'s internal struct, not
// `codec_app_def.h`'s `SEncParamExt`, which is untouched.
assert_size!(SWelsSvcCodingParam, 1232);

// Three of the five raw pointers into the C++'s one `RcInitLayerMemory` block are owned
// containers here; `pGomCost` and `pGomComplexity` are not carried, being read nowhere.
assert_size!(SWelsSvcRc, 392);
// `pOverallMbMap` is a `Vec<u16>`, and `repr(C)` comes off with it, so the compiler
// repacks the four small scalars ahead of it.
assert_size!(SSliceCtx, 48);

// codec/encoder/core/inc/mb_cache.h, svc_enc_macroblock.h, svc_enc_frame.h
//
// The eight scratch buffers `AllocMbCacheAligned` mallocs per slice are inline arrays
// here and their four ping-pong aliases are three half-selectors: the same memory the
// C++ allocates per slice, in one block instead of eight. `SPicData` carries
// `iMbX`/`iMbY` instead of the `pEncMb`, `pRefMb` and `pCsMb` pointer triples, which
// `svc_encode_slice::{enc_mb, cs_mb, ref_mb}` resolve at use. The struct is
// `repr(C, align(16))`, so its size rounds to the next multiple of 16.
assert_size!(SMbCache, 5504);
// The five per-macroblock scratch arrays the C++ reaches by pointer (`sMv`, `pRefIndex`,
// `pSadCost`, `pIntra4x4PredMode`, `pNonZeroCount`) are inline arrays in the struct.
assert_size!(SMB, 208);
// Where the C++ has five raw byte pointers, this holds `iStride` and `iHalfPixHV` —
// offsets into `SMbCache.sBufferInterPredMe` — plus the `bQuarPixSwapped` selector that
// stands in for the `pQuarPixBest`/`pQuarPixTmp` swap, and the same function pointer.
assert_size!(SMeRefinePointer, 32);

// codec/encoder/core/inc/svc_enc_frame.h:77. Three parameter-set pointers become two id
// fields, one of them an `Option<LayerSps>` where the C++ tested `pSubsetSpsP` for null.
// Not `repr(C)`, for `SDqLayer`'s reason: that `Option` has no C shape.
assert_size!(SLayerInfo, 32);

// codec/encoder/core/inc/svc_enc_frame.h
//
// `pSliceBuffer` is a `Vec<SSlice>`, and `repr(C)` comes off with it.
assert_size!(SSliceBufferInfo, 32);
// `SDqLayer` crosses no C-ABI boundary; this pin catches a *second declaration* of the
// type read at the wrong offsets. `pRefLayer` is an `Option<LayerIdx>` and the layer
// carries its own 1-byte `iDqIdx`, with `repr(C)` off, so the compiler packs the struct's
// fifteen small scalars into the holes the C layout left; `ppSliceInLayer`,
// `pFirstMbIdxOfSlice`, `pCountMbNumInSlice` and the layer's own `MbArray<SMB>` are
// `Vec`s; and `pRefPic`/`pDecPic`/`pRefOri[16]`/`pEncPic` are handles, which is what
// splits the two profiles. `sSliceBufferInfo` is a `Box` rather than four inline banks, so
// that a worker writing its bank does not race a sibling body's whole-layer shared retag.
// `pRecView` and `pEncView` are the reconstruction seam. `SDqLayer` is `Sync` — it holds
// no raw fields.
assert_size_by_profile!(SDqLayer, debug 808, release 736);

// codec/encoder/core/inc/wels_func_ptr_def.h
//
// De-virtualized: the four entropy slots are one `EntropyCoder` discriminant,
// `SWelsRcFunc`'s nine slots are one `RCMode`, and `pParametersetStrategy` is an
// `Option<Box<CWelsParametersetIdStrategyObj>>` whose 20-entry vtable costs nothing here.
// The embedded `sExpandPicFunc` table is not carried — `common/expand_pic.rs` names its
// kernels directly — and neither are the slots nothing reads: `SSampleDealingFunc`'s
// eight `pfIntra*Combined3*`, the three `pfSetMemZeroSize*`, the three `pfIDct*`,
// `DeblockingFunc`'s eight kernel slots, `pfSampleSadHor8`, `pfDeblockingBSCalc`,
// `pfSetNZCZero` and `pfAccumulateSadForRc`.
assert_size!(SWelsFuncPtrList, 944);

// codec/encoder/core/inc/encoder_context.h:116. `WELS_MUTEX` is an opaque 8-byte handle
// here, as it is in `SSliceThreading`.
assert_size!(SParaSetOffset, 1180);
assert_size!(TagVideoEncoderStatistics, 88);
// `SLogContext` (`common/utils.h:53`) is four members here: the callback, the caller's
// context, the instance *address* and the trace level — the values the reference reaches
// through `pfLog`'s back-pointer (see `common::wels_trace`).
assert_size!(crate::encoder::encoder_context::SLogContext, 32);
// The five per-macroblock scratch pointers are inline in `SMB` and `ppMbListD` is not
// carried — each layer owns its own `MbArray<SMB>`. `pEncPic`/`pDecPic`/`pRefPic` and
// `pRefList0[16]` are handles, which is what splits the two profiles; `pCurDqLayer` is
// `iCurDqLayer` (`Option<LayerIdx>`); `pSps`/`pPps` are `Option<SpsId>` /
// `Option<PpsId>`; `pSubsetSps`, `pPSOVector` and `pMemAlign` are not carried at all; and
// `pDynamicBsBuffer` is `[Vec<u8>; 4]` where the C++ has four raw pointers.
assert_size_by_profile!(sWelsEncCtx, debug 98064, release 97976);

// The `sWelsEncCtx` fields the preprocessor touches, pinned at their offsets. What they
// catch is a *second declaration* of this context, read at the wrong offsets — which a
// size assertion cannot see, the second declaration being a different type.
macro_rules! assert_ctx_offset {
    ($field:ident, $off:expr) => {
        const _: () = assert!(std::mem::offset_of!(sWelsEncCtx, $field) == $off);
    };
}
assert_ctx_offset!(sLogCtx, 0);
assert_ctx_offset!(pSvcParam, 32);
assert_ctx_offset!(iMvRange, 40);
assert_ctx_offset_by_profile!(ppRefPicListExt, debug 160, release 152);
assert_ctx_offset_by_profile!(pLtr, debug 312, release 240);
assert_ctx_offset_by_profile!(bCurFrameMarkedAsSceneLtr, debug 336, release 264);
assert_ctx_offset_by_profile!(eSliceType, debug 340, release 268);
assert_ctx_offset_by_profile!(uiDependencyId, debug 369, release 297);
assert_ctx_offset_by_profile!(uiTemporalId, debug 370, release 298);
assert_ctx_offset_by_profile!(pWelsSvcRc, debug 376, release 304);
assert_ctx_offset_by_profile!(pVaa, debug 440, release 368);
assert_ctx_offset_by_profile!(pVpp, debug 448, release 376);
assert_ctx_offset_by_profile!(sSpatialIndexMap, debug 600, release 528);
assert_ctx_offset_by_profile!(bRefOfCurTidIsLtr, debug 664, release 576);
// encoder_context.h:198 — the element type of `sSpatialIndexMap`. The layout is pinned
// because a scan that compares identifiers cannot catch a renamed duplicate of the type.
// `pSrc` is an `Option<SrcPicId>`, which splits the two profiles.
assert_size_by_profile!(SSpatialPicIndex, debug 12, release 8);
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, pSrc) == 0);
#[cfg(debug_assertions)]
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, iDid) == 8);
#[cfg(not(debug_assertions))]
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, iDid) == 4);
