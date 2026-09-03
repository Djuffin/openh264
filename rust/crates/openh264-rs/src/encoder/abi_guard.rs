#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Compile-time size checks for encoder-internal structs.
//!
//! These are the internal counterpart to `api/abi_guard.rs`, which guards the public
//! C ABI. Nothing outside the crate depends on these layouts, but the port's rule is
//! that every `#[repr(C)]` struct is a statement-for-statement translation of a C++
//! one, and a size mismatch means a field was added, dropped, or given the wrong width.
//!
//! The expected values were produced by compiling a `sizeof` dump against
//! `codec/encoder/core/inc/*.h` on darwin/arm64 (LP64); they hold on any LP64 target.
//!
//! `SWelsPPS` excludes the nine FMO fields that live inside
//! `#if !defined(DISABLE_FMO_FEATURE)`: `as264_common.h:53` defines that macro
//! unconditionally, so they are not in the struct the C++ encoder actually compiles.

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use std::mem::size_of;

use crate::common::wels_common_defs::{SNalUnitHeader, SNalUnitHeaderExt};
use crate::encoder::encoder_context::{SCropOffset, SDCTCoeff, SMVComponentUnit, SMVUnitXY};
use crate::encoder::nal_encap::SWelsNalRaw;
use crate::encoder::param_svc::{SSpsSvcExt, SSubsetSps, SWelsPPS, SWelsSPS};
use crate::common::mc::SMcFunc;
use crate::encoder::encoder_context::{sWelsEncCtx, SLTRState, SSpatialPicIndex, SStrideTables};
use crate::encoder::md::{SMB, SMbCache, SMeRefinePointer, SSampleDealingFunc, SWelsMD};
use crate::encoder::svc_encode_slice::{SDqLayer, SLayerInfo, SSlice, SSliceBufferInfo};
use crate::encoder::picture::{SPicture, SScreenBlockFeatureStorage};
use crate::encoder::param_svc::{SSpatialLayerInternal, SWelsSvcCodingParam};
use crate::encoder::rc::{SRCSlicing, SWelsSvcRc};
use crate::encoder::slice_multi_threading::SSliceCtx;
use crate::encoder::ref_list_mgr_svc::{SLTRMarkingFeedback, SLTRRecoverRequest};
use crate::encoder::set_mb_syn_cabac::{SCabacCtx, SStateCtx};
use crate::encoder::svc_motion_estimate::SWelsME;
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
use crate::encoder::wels_preprocess::{
    SAdaptiveQuantizationParam, SComplexityAnalysisParam, SComplexityAnalysisScreenParam,
    SScrollDetectionParam, SVAACalcResult, SVAAFrameInfo, SVAAFrameInfoExt,
};
use crate::encoder::ref_list_mgr_svc::{SRefPicListReorderSyntax, SRefPicMarking};
use crate::encoder::svc_encode_slice::{SSliceHeader, SSliceHeaderExt};

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
// `Option<SrcPicId>` / `Option<RecPicId>` are **4 bytes in a release build and 8 in
// a debug build**: `pool::Id` carries a generation counter under `debug_assertions`
// (that is the whole staleness instrument — see `safe/pool.rs`), and the `NonZeroU32`
// niche keeps the `Option` free either way. So every struct that stores a picture
// handle has two sizes, and every offset after the first handle has two values.
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
// `SWelsNalRaw`: 40 in the C++ (`uint8_t* pRawData; int32_t iPayloadSize;
// SNalUnitHeaderExt sNalExt; int32_t iStartPos;`), without `pRawData` here — that was
// a cache of `buffer + iStartPos`, and the record keeps the offset while
// `WelsEncodeNal`'s caller names the buffer. Minus the 8-byte pointer, and the
// struct's alignment drops from 8 to 4: 32.
assert_size!(SWelsNalRaw, 32);

// codec/encoder/core/inc/picture.h
//
// 136 in the C++. The four per-macroblock side arrays (`uiRefMbType`, `pRefMbQp`,
// `pMbSkipSad`, `sMvList`) are owned `Vec`s rather than four `WelsMallocz`'d
// pointers, so the struct trades 4 pointers for 4 fat pointers (+48) and loses
// `#[repr(C)]` with them; Rust then packs the six one-byte flags into the hole after
// `iFrameAverageQp`. `pBuffer` + `pData[3]` + `iLineSize[3]` (44 bytes, 48 padded)
// are three owned `PaddedPlane`s (`Vec` + stride + origin + width + height + pad = 64
// bytes each, 192 total): the picture owns every byte it has.
assert_size!(SPicture, 344);

// codec/encoder/core/inc/encoder_context.h — `SRefList`. It has no C++ number worth
// asserting (it owns its pictures): 34 handles + a pool + two counts, profile-split
// like every other struct that stores a handle.
assert_size_by_profile!(
    crate::encoder::encoder_context::SRefList,
    debug 240,
    release 120
);
// 88 is the C++ struct's size — five raw pointers, three scalars and a `[u32; 7]`.
// The storage owns its five buffers here, so the C++ number no longer describes this
// type: it never crosses the ABI (`src/api/` does not name it, and `SPicture` holds it
// as a niche-optimised `Option<Box<..>>`, so `assert_size!(SPicture, ..)` is
// untouched). The layer's scratch (`SFeatureSearchPreparation::pFeatureOfBlock`) is
// not held here either — `CalculateFeatureOfBlock` takes it as `&mut [u16]`.
assert_size!(SScreenBlockFeatureStorage, 136);

// codec/encoder/core/inc/parameter_sets.h
assert_size!(SWelsSPS, 56);
assert_size!(SWelsPPS, 16);
assert_size!(SSpsSvcExt, 4);
assert_size!(SSubsetSps, 60);

// codec/encoder/core/inc/slice.h. SSliceHeader excludes iSliceGroupChangeCycle,
// which sits inside `#if !defined(DISABLE_FMO_FEATURE)` at slice.h:124.
//
// 168 and 192 in the C++, and these are the port's own numbers.
// `SSliceHeader::pSps`/`pPps` and `SSliceHeaderExt::pSubsetSps` are not carried: the
// first two have their replacement sitting in the same struct (`iSpsId`/`iPpsId`,
// which the C++ writes in the same statement block). `SSliceHeaderExt` loses its own
// 8 plus the 16 its embedded header lost.
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
// 248 in the C++. `SSampleDealingFunc::pfMdCost` and `pfMeCost` are `CostFamily`
// tags, 2 bytes where the C++ has two 8-byte `PSampleSadSatdCostFunc*` pointing
// *into this same struct's* sibling arrays (-16, then +8 of tail padding to keep the
// 8-byte alignment the fn-pointer arrays require), and the eight
// `pfIntra*Combined3*` slots (`*mut c_void`, never assigned on any target) are not
// carried (-64). Nothing crosses the C ABI with this layout: `SWelsFuncPtrList` is
// encoder-internal and `api/abi_guard.rs` guards the public surface separately.
assert_size!(SSampleDealingFunc, 176);
assert_size!(SRCSlicing, 44);
// 160 in the C++, where the struct holds sixteen raw pointers into one block it does
// not own. Here it owns the block as two typed stores — `Vec<[i32; 24]>` and
// `Vec<i16>`, 24 bytes each — and the pointers are indices into them: 208, the port's
// own number, with no correspondence to a C++ `sizeof`.
assert_size!(SStrideTables, 208);
// 96 in the C++. The three per-block plane cursors (`pEncMb`/`pRefMb`/`pColoRefMb`)
// are not in the search block — the coordinates already in the struct carry the same
// information, and the search family takes the planes as parameters — so the tail
// packs at 2-byte alignment. `pMvdCost` is a `MvdCostCursor` rather than a `*mut u16`:
// the table as a `&[u16]` plus the index of the entry the raw cursor points at.
// `COST_MVD` indexes with a *signed* MVD, so the pointer is parked mid-table and no
// plain slice can stand in for it; the pair can. The struct is `repr(C)` and the
// cursor is not, which costs nothing here: this family crosses no ABI.
assert_size!(SWelsME, 80);
// 4000 in the C++, of which 3072 is `sMe4x4`/`sMe8x4`/`sMe4x8` — 32 `SWelsME` at 96
// bytes — whose only readers are `WelsMdInterMbRefinement`'s three sub-8x8 arms.
// Nothing in either encoder ever produced those partitions: upstream's sub-8x8 search
// is inside `#if 0 //Disable for sub8x8 modes for now`
// (`svc_mode_decision.cpp:634-661`), so they are not carried here. What is left is
// the `sMe` container's nine embedded `SWelsME` (sMe16x16 + sMe16x8[2] + sMe8x16[2] +
// sMe8x8[4]) plus this struct's own `pMvdCost`. `SWelsMD` is encoder-internal and
// crosses no ABI.
assert_size!(SWelsMD, 800);
// `SVAAFrameInfo`: 264 in the C++, and the pin is a *drift tracker* rather than an
// ABI contract — `repr(C)` is off, so it moves with a deliberate field change. The
// six per-frame result arrays and `pVaaBackgroundMbFlag` are the block's own `Vec`s;
// `pMotionTextureUnit` and `pMotionTextureIndexToDeltaQp`
// (`encoder_ext.cpp:1721/:1724`) are owned here rather than pointed at from
// `sAdaptiveQuantParam`; and the six `*mut u8` plane roots are two
// `Option<RoPicView>` — three `SharedPlane`s of (base, len, stride, origin) is 96
// bytes and the `Option` has no niche over a raw base, so 104 each. The VAA result
// and the parameter blocks' buffers reach each plugin as slices at its `Process`
// call, so no `pCalcResult` pointer is stored. **`SVAAFrameInfo` is `Sync`**;
// `pCurY`/`pRefY` are `usize`.
assert_size!(SVAAFrameInfo, 504);
// Embeds `SVAAFrameInfo`, and its screen block-static family is not pointers:
// `pVaaBlockStaticIdc` is one owned `SBlockStaticIdcStore` (40) where the C++ has
// sixteen `*mut u8` (128); `pVaaBestBlockStaticIdc` is an `Option<usize>` row number
// (16) where a pointer stands (8); and the same swap inside `SRefInfoParam` (24 -> 32)
// is paid 32 times, once per slot of `sVaaStrBestRefCandidate` and
// `sVaaLtrBestRefCandidate`. `RequestMemorySvc` builds the struct under
// `SCREEN_CONTENT_REAL_TIME`, so these are bytes a screen encode takes.
assert_size!(SVAAFrameInfoExt, 1680);

// SSliceThreading is deliberately NOT asserted. C++ (mt_defs.h:68) embeds
// WELS_EVENT (pthread_cond_t, 48 B) and WELS_MUTEX (pthread_mutex_t, 64 B) by
// value, reaching 1256 bytes on darwin; those sizes are libc-specific, and this
// port models the primitives as opaque handles. Nothing crosses a C ABI here, so
// the field-for-field size correspondence that holds for the codec's own structs
// does not apply. Revisit if the threading types are ever given real bodies.

// `SVAACalcResult`: six owned arrays where the C++ has six pointers.
assert_size!(SVAACalcResult, 168);
assert_size!(SScrollDetectionParam, 32);
// 40 in the C++. Its two buffer pointers are `SVAAFrameInfo`'s owned `Vec`s, and they
// and the VAA result reach the plugin at the `Process` call rather than being stored
// here as a `pCalcResult` pointer.
assert_size!(SAdaptiveQuantizationParam, 8);
// 64 in the C++. `pGomComplexity` and `pGomForegroundBlockNum` are the rate
// controller's `Vec`s and reach the plugin as slices; `pBackgroundMbFlag` and
// `uiRefMbType` go the same way. See `SAdaptiveQuantizationParam` above.
assert_size!(SComplexityAnalysisParam, 24);

// 72 in the C++; `pGomComplexity` (`int*`) is not carried — the screen plugin takes
// the GOM array as a slice at the call, as the camera one does, which is what lets
// `SVAAFrameInfoExt` be `Sync`.
assert_size!(SComplexityAnalysisScreenParam, 56);

// Mid-tier types.
assert_size!(SSpatialLayerInternal, 68);
// 1240 in the C++, and the 8-byte difference is a deliberate divergence rather than a
// port shape: `pCurPath` (`param_svc.h:118`), a `char*` with three writes and **no reader
// in either tree** — upstream declares it, nulls it and stores to it and never reads
// it anywhere in `codec/` — is not carried. `SWelsSvcCodingParam` is therefore one
// field short of its C++ counterpart, on purpose, and this is the file's only
// intentional field-count divergence; every other pin here still means "the
// translation is field for field". Nothing outside the crate reads this layout (see
// the module note) — the struct is `param_svc.h`'s internal one, not
// `codec_app_def.h`'s `SEncParamExt`, which is untouched.
assert_size!(SWelsSvcCodingParam, 1232);


// 360 in the C++, where five raw pointers address one `RcInitLayerMemory` block.
// Three of the five are owned containers at 24 bytes each here, +32 over the C++'s
// 360; `pGomCost` and `pGomComplexity` are not carried — allocated, nulled and memset
// in the reference, read nowhere in either tree.
assert_size!(SWelsSvcRc, 392);
// 32 in the C++. `pOverallMbMap` is a `Vec<u16>` — one pointer becomes three words,
// and `repr(C)` comes off with it, so the compiler repacks the four small scalars
// ahead of it: 48.
assert_size!(SSliceCtx, 48);


// codec/encoder/core/inc/mb_cache.h, svc_enc_macroblock.h, svc_enc_frame.h
// 576 in the C++. The eight scratch buffers `AllocMbCacheAligned` malloc's per slice
// are inline arrays in the struct and their four ping-pong aliases are three
// half-selectors: -96 bytes of pointers, +5120 of buffers (528 + 768 + 384 + 32 +
// 2560 + 16 + 16 + 816), +3 of selectors and the alignment padding around them — the
// same memory the C++ allocates per slice, in one block instead of eight.
// `SPicData` carries `iMbX`/`iMbY` (8 bytes) instead of the `pEncMb`, `pRefMb` and
// `pCsMb` pointer triples, which `svc_encode_slice::{enc_mb, cs_mb, ref_mb}` resolve
// at use; it is the coordinate the plane pointers are a function of, and it is what
// lets a reader with no `SMB` in scope build a cursor (see the field's doc in
// `encoder_context.rs`). The struct is `repr(C, align(16))`, so its size rounds to
// the next multiple of 16.
assert_size!(SMbCache, 5504);
// 152 in the C++. The five per-macroblock scratch arrays the C++ reaches by pointer
// (`sMv`, `pRefIndex`, `pSadCost`, `pIntra4x4PredMode`, `pNonZeroCount`) are inline
// arrays in the struct: -40 bytes of pointers, +104 bytes of rows, and -8 of padding
// as the alignment falls from 8 to 4. 208.
assert_size!(SMB, 208);
// `SMeRefinePointer` is five raw byte pointers plus a function pointer in the C++ (48
// bytes) and here is `iStride` + `iHalfPixHV` (two `usize` offsets into
// `SMbCache.sBufferInterPredMe`), the `bQuarPixSwapped` selector that stands in for
// the `pQuarPixBest`/`pQuarPixTmp` `mem::swap`, and the same function pointer: 32. It
// is stack-local with one builder, so this pin is a shape record rather than an ABI
// one.
assert_size!(SMeRefinePointer, 32);

// codec/encoder/core/inc/svc_enc_frame.h:77. 48 in the C++, 32 here: its three
// parameter-set pointers are two id fields, and one of those two —
// `Option<LayerSps>` — is the tagged union the C++ spelled as "`pSubsetSpsP` null or
// not". The struct is not `repr(C)`, for `SDqLayer`'s reason: that `Option` has
// no C shape.
assert_size!(SLayerInfo, 32);

// codec/encoder/core/inc/svc_enc_frame.h
// 16 in the C++. `pSliceBuffer` is a `Vec<SSlice>` — a pointer becomes three words,
// and `repr(C)` comes off with it: 32.
assert_size!(SSliceBufferInfo, 32);
// 512 in the C++; these are the port's own numbers, measured in the profile they
// name. This pin does not describe a C-ABI contract — `SDqLayer` does not cross the
// boundary — it catches a *second declaration* of the type read at the wrong offsets.
// `pRefLayer` is an `Option<LayerIdx>` (2 bytes of position against 8 of address) and
// the layer carries its own 1-byte `iDqIdx`, with `repr(C)` off, so the compiler packs
// the struct's fifteen small scalars into the holes the C layout left;
// `ppSliceInLayer`, `pFirstMbIdxOfSlice`, `pCountMbNumInSlice` and the layer's own
// `MbArray<SMB>` are `Vec`s where the C++ has pointers; and
// `pRefPic`/`pDecPic`/`pRefOri[16]`/`pEncPic` are handles, which is what splits the
// two profiles. `sSliceBufferInfo` is a `Box` rather than four inline banks, so that a
// worker writing its bank does not race a sibling body's whole-layer shared retag (see
// the field's own note). `pRecView` (`Option<RecPicView>`) and `pEncView`
// (`Option<RoPicView>`) are the reconstruction seam: three shared planes at 32 bytes
// each and, for `pRecView`, four shared per-macroblock arrays at 16, plus an `Option`
// discriminant that costs a word because a captured base has no niche.
// **`SDqLayer` is `Sync`** — it holds no raw fields.
assert_size_by_profile!(SDqLayer, debug 808, release 736);

// codec/encoder/core/inc/wels_func_ptr_def.h
// 1280 in the C++, and the number tracks the port rather than the header:
// de-virtualization is the point. Four 8-byte entropy slots are one `EntropyCoder`
// discriminant (-32, +8 for the byte and its padding to the pointer that follows);
// `SWelsRcFunc`'s nine slots are one `RCMode` (72 -> 4 bytes, and the member's
// alignment drops from 8 to 4); `pParametersetStrategy` is an
// `Option<Box<CWelsParametersetIdStrategyObj>>`, pointer-sized by the null-pointer
// niche, so the 20-entry vtable behind it costs nothing here; the embedded
// `sExpandPicFunc` table (-24) is not carried, since
// `common/expand_pic.rs::ExpandReferencingPicture` names its kernels directly; and
// neither are the slots nothing reads — `SSampleDealingFunc`'s eight
// `pfIntra*Combined3*` (-64), the three `pfSetMemZeroSize*` (-24), the three
// `pfIDct*` (-24), `DeblockingFunc`'s eight kernel slots (-64), `pfSampleSadHor8`
// (-16), `pfDeblockingBSCalc` and `pfSetNZCZero` (-16, called direct), and
// `pfAccumulateSadForRc` (-8, whose SADs the port's rate control reads directly).
assert_size!(SWelsFuncPtrList, 944);

// codec/encoder/core/inc/encoder_context.h:116. C++ is 98008 bytes, but that number
// embeds WELS_MUTEX (pthread_mutex_t, 64 B on darwin) by value where this port models
// the mutex as an opaque 8-byte handle, exactly as it does in SSliceThreading. So the
// expected size here is 98008 - 64 + 8; alignment is 8 either way, so no padding
// changes. Everything else -- including sWelsCabacContexts[4][52][460] at 95,680
// bytes -- is a faithful match, which is what makes this assertion worth having.
assert_size!(crate::encoder::encoder_context::SParaSetOffset, 1180);
assert_size!(crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics, 88);
// `SLogContext` is `common/utils.h:53`'s three `void*`s in the reference, and this
// port's is four members: the callback, the caller's context, the instance *address*
// and the trace level. The last two are what the reference reaches through `pfLog`'s
// back-pointer into the `welsCodecTrace` — a route that cannot be written here (see
// `common::wels_trace`), so the values travel instead of a pointer to them.
assert_size!(crate::encoder::encoder_context::SLogContext, 32);
// The port's own layout, measured in the profile it names, against the C++'s
// `98008 - 64 + 8` = 97952. The five per-macroblock scratch pointers are inline in
// `SMB` (see `assert_size!(SMB, …)` above) and `ppMbListD` is not carried — each
// layer owns its own `MbArray<SMB>`. `pEncPic`/`pDecPic`/`pRefPic` and
// `pRefList0[16]` are handles, which is what splits the two profiles; `pCurDqLayer`
// is `iCurDqLayer` (`Option<LayerIdx>`, 2 bytes, align 1); `pSps`/`pPps` are
// `Option<SpsId>` / `Option<PpsId>` (2 and 4 bytes against 8 apiece); `pSubsetSps`
// and `pPSOVector` are not carried at all — the C++ declares them and nothing ever
// reads them — and neither is `pMemAlign`, with the allocator it named. Finally,
// `pDynamicBsBuffer` is `[Vec<u8>; 4]` where the C++ has four raw pointers into
// `CMemoryAlign` blocks.
assert_size_by_profile!(sWelsEncCtx, debug 98064, release 97976);


// The `sWelsEncCtx` fields the preprocessor touches, pinned at their C++
// offsets. `wels_preprocess.rs` used to declare its own 15-field `SWelsEncCtx` and
// alias `sWelsEncCtx` to it, so every one of these reads landed at the wrong offset
// the moment a real context was passed in -- which is exactly what
// `WelsEncoderEncodeExt` does when it calls `BuildSpatialPicList` / `AnalyzeSpatialPic`
// / `UpdateSpatialPictures`. A size assertion could not catch that (the fake struct
// was simply a different type), so the offsets are asserted directly.
//
// All are declared before `WELS_MUTEX mutexEncoderError` (encoder_context.h:230),
// the one member this port models differently, so each number below *was* the unmodified
// C++ `offsetof`, measured on darwin/arm64.
//
// These are the port's own layout, not the C++'s: `pTaskManage`, `ppMbListD` and the
// five per-macroblock scratch pointers (`pSadCostMb`, `pMvUnitBlock4x4`,
// `pRefIndexBlock4x4`, `pNonZeroCountBlocks`, `pIntra4x4PredModeBlocks`, whose
// contents are inline in `SMB`) all sat ahead of most of the pins. What the pins
// still catch does not depend on the numbers being the C++'s: it is a *second*
// declaration of this context, read at the wrong offsets.
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
// encoder_context.h:198 -- the element type of `sSpatialIndexMap`. `wels_preprocess.rs`
// carried a byte-identical copy of this under the invented name `SSpatialIndexMap`;
// a scan that compares identifiers cannot catch a rename, so the layout is pinned here.
// 16 in the C++; 12 debug / 8 release here — `pSrc` is an `Option<SrcPicId>`.
assert_size_by_profile!(SSpatialPicIndex, debug 12, release 8);
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, pSrc) == 0);
#[cfg(debug_assertions)]
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, iDid) == 8);
#[cfg(not(debug_assertions))]
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, iDid) == 4);









