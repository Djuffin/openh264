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
//! This file exists because `SWelsPPS` had drifted to roughly nine times its real size:
//! the port had transcribed the nine FMO fields that live inside
//! `#if !defined(DISABLE_FMO_FEATURE)`, and `as264_common.h:53` defines that macro
//! unconditionally, so they are not in the struct the C++ encoder actually compiles.
//! A size assertion catches that class of mistake the moment it is written.

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
// **Profile-split pins — T6.F1.**
//
// `Option<SrcPicId>` / `Option<RecPicId>` are **4 bytes in a release build and 8 in
// a debug build**: `pool::Id` carries a generation counter under `debug_assertions`
// (that is the whole staleness instrument — see `safe/pool.rs`), and the `NonZeroU32`
// niche keeps the `Option` free either way. So every struct that stores a picture
// handle now has two sizes, and every offset after the first handle has two values.
//
// The choice was between splitting these pins and giving `Id` a generation field in
// both profiles. The second would make `Id` 8 bytes in release everywhere, including
// the decoder's `[[Option<PicId>; 16]; 2]` per-macroblock deblocking arrays, which
// `safe/pool.rs` documents the niche as existing for. A measured decoder cost to keep
// an assertion one line shorter is the wrong trade, so the pins split.
//
// Both numbers are measured, in the profile they name. What the pins still catch is
// what they were written for: a second declaration of the type, read at the wrong
// offsets.
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
//
// `assert_size!(SBitStringAux, 48)` was here. The type is a pointer-triple cursor
// (`pStartBuf`/`pCurBuf`/`pEndBuf` plus the accumulator), and T3.4 replaced the
// encoder's use of it with `safe::bits::BsWriter`, which is `{pos, cur_bits,
// left_bits}` — 16 bytes, no pointers, and no correspondence to a C++ layout to
// assert. The plan's rule (§Phase 6.6) is that each assert dies in the commit that
// de-C-ifies its struct rather than the struct being contorted to keep it green.
assert_size!(SNalUnitHeader, 12);
assert_size!(SNalUnitHeaderExt, 24);

// codec/encoder/core/inc/nal_encap.h
//
// `SWelsSliceBs` (176) and `SWelsEncoderOutput` (96) went the same way in the same
// commit: each embedded an `SBitStringAux` by value and now embeds a `BsWriter`,
// which is 32 bytes smaller. `SSlice` (1584, below) embeds `SWelsSliceBs` in turn,
// so it lost the same 32.
//
// `SWelsNalRaw`: 40 in the C++ (`uint8_t* pRawData; int32_t iPayloadSize;
// SNalUnitHeaderExt sNalExt; int32_t iStartPos;`). Phase 6 session B deleted
// `pRawData` — a cache of `buffer + iStartPos` that the encoder probe caught being
// killed by the writer's `&mut sBsBuffer[..]` between load and encode; the record
// keeps the offset and `WelsEncodeNal`'s caller names the buffer. Minus the
// 8-byte pointer, and the struct's alignment drops from 8 to 4: 32.
assert_size!(SWelsNalRaw, 32);

// codec/encoder/core/inc/picture.h
//
// **136 in the C++, and 192 is the port's own number since T6.F0.** The four
// per-macroblock side arrays (`uiRefMbType`, `pRefMbQp`, `pMbSkipSad`, `sMvList`) are
// owned `Vec`s rather than four `WelsMallocz`'d pointers, so the struct trades 4
// pointers for 4 fat pointers (+48) and loses `#[repr(C)]` with them; Rust then packs
// the six one-byte flags into the hole after `iFrameAverageQp`, so the measured total
// is 192 rather than the 200 a `repr(C)` layout would give. Measured, not predicted.
// **T6.F2**: +152. `pBuffer` + `pData[3]` + `iLineSize[3]` (44 bytes, 48 padded) are
// three owned `PaddedPlane`s (`Vec` + stride + origin + width + height + pad = 64
// bytes each, 192 total). The picture owns every byte it has, and `CMemoryAlign` has
// nothing left to allocate for one. **344, measured.**
assert_size!(SPicture, 344);

// codec/encoder/core/inc/encoder_context.h — `SRefList`, **newly pinned at T6.F1**.
// It has no C++ number worth asserting any more (it owns its pictures), and it is
// pinned for the reason `SMeRefinePointer` is: one builder, one shape, and a silent
// change to it would be a silent change to how a dependency layer holds its
// reference pictures. 34 handles + a pool + two counts; profile-split like every
// other struct that stores a handle.
assert_size_by_profile!(
    crate::encoder::encoder_context::SRefList,
    debug 240,
    release 120
);
assert_size!(SScreenBlockFeatureStorage, 88);

// codec/encoder/core/inc/parameter_sets.h
assert_size!(SWelsSPS, 56);
assert_size!(SWelsPPS, 16);
assert_size!(SSpsSvcExt, 4);
assert_size!(SSubsetSps, 60);

// codec/encoder/core/inc/slice.h. SSliceHeader excludes iSliceGroupChangeCycle,
// which sits inside `#if !defined(DISABLE_FMO_FEATURE)` at slice.h:124.
assert_size!(SRefPicMarking, 100);
assert_size!(SRefPicListReorderSyntax, 16);
assert_size!(SSliceHeader, 168);
assert_size!(SSliceHeaderExt, 192);

// codec/encoder/core/inc/wels_common_basis.h, mb_cache.h
assert_size!(SMVUnitXY, 4);
assert_size!(SCropOffset, 8);
assert_size!(SDCTCoeff, 816);
assert_size!(SMVComponentUnit, 146);

// Leaf types unified in the same pass. SStateCtx is a single packed byte
// (set_mb_syn_cabac.h:55) — the encoder_context.rs copy had it as two u8 fields,
// which would have doubled sWelsCabacContexts[4][52][460].
assert_size!(SStateCtx, 1);
assert_size!(SCabacCtx, 504);
assert_size!(SLTRState, 60);
assert_size!(SLTRMarkingFeedback, 16);
assert_size!(SLTRRecoverRequest, 20);
// `SExpandPicFunc` (24) was asserted here until T4b.3b deleted the struct: every
// install in both codecs set the same three `_c` constants, so the table was a
// dispatch with one arm. `common/expand_pic.rs::ExpandReferencingPicture` names
// the kernels directly and takes no function pointers.
assert_size!(SMcFunc, 48);
// 248 in the C++ and in this port until Phase 4a. `SSampleDealingFunc::pfMdCost`
// and `pfMeCost` were `PSampleSadSatdCostFunc*` pointing *into this same
// struct's* sibling arrays — F13's fourth site, and UB under Stacked Borrows the
// moment anything took `&mut SWelsFuncPtrList`. They are now `CostFamily` tags,
// which is 2 bytes where two 8-byte pointers used to sit (-16, then +8 of tail
// padding to keep the 8-byte alignment the fn-pointer arrays require).
//
// This is the first deliberate divergence from a C++ layout in the port, and the
// file's own premise says a size change means "a field was added, dropped, or
// given the wrong width" — here it means the first, on purpose. Phase 4's job is
// to stop transliterating the dispatch tables; Phase 6.6 deletes these asserts
// entirely as the structs de-C-ify. Nothing crosses the C ABI with this layout:
// `SWelsFuncPtrList` is encoder-internal and `api/abi_guard.rs` guards the
// public surface separately.
// Phase 6 session B took the eight `pfIntra*Combined3*` slots (`*mut c_void`,
// never assigned on any target, guarded by `assert_no_combined3`) out with the
// guard: -64, measured 176.
assert_size!(SSampleDealingFunc, 176);
assert_size!(SRCSlicing, 44);
assert_size!(SStrideTables, 160);
assert_size!(SWelsME, 96);
assert_size!(SWelsMD, 4000);
// `SVAAFrameInfo`: 264 in the C++. Phase 6 session B dissolved the `IWelsVP`
// vtable and deleted the stored `pCalcResult` pointer from the two parameter
// blocks this embeds (`sAdaptiveQuantParam`, `sComplexityAnalysisParam` — the
// VAA result is handed to each plugin at its `Process` call instead), 8 bytes
// each: 248. `SVAAFrameInfoExt` embeds this one and moved by the same 16.
// **T6.F3**: +104. The six per-frame result arrays are the block's own `Vec`s, so
// six pointers become six fat pointers (+48) and `pVaaBackgroundMbFlag` a seventh
// (+16); `repr(C)` comes off with them and the compiler repacks. **352, measured.**
assert_size!(SVAAFrameInfo, 352);
// **T6.F3**: +104, all of it its embedded `SVAAFrameInfo`. **1368, measured.**
assert_size!(SVAAFrameInfoExt, 1368);
// SSliceThreading is deliberately NOT asserted. C++ (mt_defs.h:68) embeds
// WELS_EVENT (pthread_cond_t, 48 B) and WELS_MUTEX (pthread_mutex_t, 64 B) by
// value, reaching 1256 bytes on darwin; those sizes are libc-specific, and this
// port models the primitives as opaque handles. Nothing crosses a C ABI here, so
// the field-for-field size correspondence that holds for the codec's own structs
// does not apply. Revisit if the threading types are ever given real bodies.
// **T6.F3**: +96, six owned arrays where six pointers were. **168, measured.**
assert_size!(SVAACalcResult, 168);
assert_size!(SScrollDetectionParam, 32);
// 40 and 64 in the C++; each lost its stored `pCalcResult` pointer at Phase 6
// session B (see `SVAAFrameInfo` above): 32 and 56, measured.
assert_size!(SAdaptiveQuantizationParam, 32);
assert_size!(SComplexityAnalysisParam, 56);
assert_size!(SComplexityAnalysisScreenParam, 72);

// Mid-tier types.
assert_size!(SSpatialLayerInternal, 68);
assert_size!(SWelsSvcCodingParam, 1240);


assert_size!(SWelsSvcRc, 360);
// 32 in the C++ and in this port until **T6.D7**, which made `pOverallMbMap` a
// `Vec<u16>` — one pointer becomes three words, and `repr(C)` comes off with it, so
// the compiler repacks the four small scalars ahead of it. **48, measured**, and the
// number tracks the port from here.
assert_size!(SSliceCtx, 48);


// codec/encoder/core/inc/mb_cache.h, svc_enc_macroblock.h, svc_enc_frame.h
// 576 in the C++ and in this port until **T6.C3**, which moved the eight scratch
// buffers `AllocMbCacheAligned` malloc'd per slice into the struct as inline arrays
// and replaced their four ping-pong aliases with three half-selectors: -96 bytes of
// pointers, +5120 of buffers (528 + 768 + 384 + 32 + 2560 + 16 + 16 + 816), +3 of
// selectors and the alignment padding around them. **5600, measured** — and it is the
// same memory the C++ allocates per slice, in one block instead of eight.
// `SSlice`, which embeds it, is **6544** (was 1520).
// **5584 since T6.F0**: `pEncSad`, the last alias into an `SPicture`, is deleted —
// one pointer plus the 8 bytes of padding its removal let the compiler reclaim.
assert_size!(SMbCache, 5584);
// 152 in the C++ and in this port until **T6.C1**, which moved the five
// per-macroblock scratch arrays the C++ reaches by pointer (`sMv`, `pRefIndex`,
// `pSadCost`, `pIntra4x4PredMode`, `pNonZeroCount`) into the struct as inline
// arrays: -40 bytes of pointers, +104 bytes of rows, and -8 of padding as the
// alignment falls from 8 to 4. **208, measured.** The number tracks the port from
// here, exactly as `SWelsFuncPtrList`'s does.
assert_size!(SMB, 208);
// **T6.E3**: newly pinned, because the record's shape changed. `SMeRefinePointer`
// was five raw byte pointers plus a function pointer — 48 bytes — and is now `iStride` +
// `iHalfPixHV` (two `usize` offsets into `SMbCache.sBufferInterPredMe`), the
// `bQuarPixSwapped` selector that replaced the `pQuarPixBest`/`pQuarPixTmp`
// `mem::swap`, and the same function pointer. **32, measured.** It is stack-local
// with one builder, so this pin is a shape record rather than an ABI one — which is
// exactly why it should exist: the next session to touch the record sees the number
// move.
assert_size!(SMeRefinePointer, 32);
assert_size!(SLayerInfo, 48);

// codec/encoder/core/inc/slice.h. `assert_size!(SSlice, 1584)` was here — see the
// `SWelsSliceBs` note above; `SSlice` embeds it by value.

// codec/encoder/core/inc/svc_enc_frame.h
// 16 in the C++ and in this port until **T6.D8**, which made `pSliceBuffer` a
// `Vec<SSlice>` — a pointer becomes three words, and `repr(C)` comes off with it.
// **32, measured**, and the number tracks the port from here.
assert_size!(SSliceBufferInfo, 32);
// 512 in the C++, 504 after **T6.D2** deleted the dead `pFeatureSearchPreparation`
// pointer (S18), and **480 after T6.D3**, which made `pRefLayer` an
// `Option<LayerIdx>` (8 bytes of address -> 2 of position), added the layer's own
// 1-byte `iDqIdx`, and dropped `repr(C)` with them — so the compiler now packs the
// struct's fifteen small scalars into the holes the C layout left, and **496 after
// T6.D4** made `ppSliceInLayer` a `Vec<SliceIdx>` (one pointer becomes a three-word
// `Vec`), **528 after T6.D5** gave the layer its own `MbArray<SMB>` (the same
// trade again), and **560 after T6.D6** made `pFirstMbIdxOfSlice` and
// `pCountMbNumInSlice` `Vec<i32>` (two more pointers become two more three-word
// `Vec`s), **576 after T6.D7** grew the inline `sSliceEncCtx` by the same trade, and
// **640 after T6.D8** grew the four inline `sSliceBufferInfo` banks by 16 apiece.
// **Measured** at each step; the number tracks the port, not the C++.
// **T6.F1**: `pRefPic`/`pDecPic`/`pRefOri[16]` become handles and `pRefList` is
// added — 712 debug / 640 release. **T6.F5**: +128 for the two `SRefPicView`s the
// macroblock loop reads instead of resolving a handle per access — **840 debug /
// 768 release**, measured.
assert_size_by_profile!(SDqLayer, debug 840, release 768);

// codec/encoder/core/inc/wels_func_ptr_def.h
// 1280 before Phase 4a; -8 for `SSampleDealingFunc`'s shrink above; -24 at T4b.1,
// where four 8-byte entropy slots became one `EntropyCoder` discriminant (-32, +8
// for the byte and its padding to the pointer that follows); -64 at T4b.1b, where
// `SWelsRcFunc`'s nine slots became one `RCMode` (72 -> 4 bytes, and the member's
// alignment drops from 8 to 4). The number tracks the port, not the C++ header,
// from Phase 4 on: de-virtualization is the point.
//
// **T4b.2a moved it by 0, and that is the ledger entry.** `pParametersetStrategy`
// went from a raw pointer-to-vtable-object to
// `Option<Box<CWelsParametersetIdStrategyObj>>`, which is pointer-sized by the
// null-pointer niche -- so a whole 20-entry vtable, two static instances and 25
// thunks came out of the crate without this number twitching. Size is the wrong
// instrument for that seam; the ratchet (raw_ptr -92, unsafe_fn -26) is the right
// one. Stated here so the next reader does not go looking for the missing bytes.
//
// -24 at T4b.3b: `sExpandPicFunc`, the first member and the only *embedded table*
// in this struct rather than a slot. It moves the number where T4b.2a and T4b.3a
// could not, and the reason is the same one stated above -- size measures bytes of
// members, so it sees a 24-byte struct leave and cannot see a vtable leave. Both
// seams were the same size of change; only one of them is legible here.
// -64 at Phase 6 session B for `SSampleDealingFunc`'s eight deleted slots (above),
// and -24 more for the three `pfSetMemZeroSize*` slots deleted in the same face:
// 1072, measured.
assert_size!(SWelsFuncPtrList, 1072);

// codec/encoder/core/inc/encoder_context.h:116. C++ is 98008 bytes, but that number
// embeds WELS_MUTEX (pthread_mutex_t, 64 B on darwin) by value where this port models
// the mutex as an opaque 8-byte handle, exactly as it does in SSliceThreading. So the
// expected size here is 98008 - 64 + 8; alignment is 8 either way, so no padding
// changes. Everything else -- including sWelsCabacContexts[4][52][460] at 95,680
// bytes -- is a faithful match, which is what makes this assertion worth having.
assert_size!(crate::encoder::encoder_context::SParaSetOffset, 1180);
assert_size!(crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics, 88);
assert_size!(crate::encoder::encoder_context::SLogContext, 24);
// **T6.C1**: -40, the five per-macroblock scratch pointers deleted with `SMB`'s
// conversion (see `assert_size!(SMB, …)` above). Was `98008 - 64 + 8` = 97952, the
// C++ size with this port's 8-byte mutex handle in place of `pthread_mutex_t`.
// **T6.D5**: -8 more, `ppMbListD` deleted — `InitMbListD` cut one flat block across
// the layers and stored the cuts twice, and each layer owns its own `MbArray<SMB>`
// now. **97904, measured.**
// **T6.F1**: -16 debug / -112 release. `pEncPic`/`pDecPic`/`pRefPic` and
// `pRefList0[16]` are handles now — nineteen 8-byte pointers become nineteen 8-byte
// (debug) or 4-byte (release) handles.
assert_size_by_profile!(sWelsEncCtx, debug 97888, release 97792);


// The fifteen `sWelsEncCtx` fields the preprocessor touches, pinned at their C++
// offsets. `wels_preprocess.rs` used to declare its own 15-field `SWelsEncCtx` and
// alias `sWelsEncCtx` to it, so every one of these reads landed at the wrong offset
// the moment a real context was passed in -- which is exactly what
// `WelsEncoderEncodeExt` does when it calls `BuildSpatialPicList` / `AnalyzeSpatialPic`
// / `UpdateSpatialPictures`. A size assertion could not catch that (the fake struct
// was simply a different type), so the offsets are asserted directly.
//
// All fifteen are declared before `WELS_MUTEX mutexEncoderError` (encoder_context.h:230),
// the one member this port models differently, so each number below *was* the unmodified
// C++ `offsetof`, measured on darwin/arm64.
//
// **T6.C1 moved all fifteen and they are re-pinned to measured values; T6.D5 moved
// thirteen of them again**, by a further 8, deleting `ppMbListD` from ahead of them.
// The two that do not move are the two that precede it.**T6.C1's account:** Five raw
// pointers (`pSadCostMb`, `pMvUnitBlock4x4`, `pRefIndexBlock4x4`,
// `pNonZeroCountBlocks`, `pIntra4x4PredModeBlocks`) sat at offsets 32-96 -- ahead of
// every pin below -- and their contents are inline in `SMB` now, so each offset from
// `iMvRange` on falls by exactly 40 (`sLogCtx` and `pSvcParam` precede them and do not
// move). These are the port's own layout from here, not the C++'s; what the pins still
// catch is the thing they were written for -- a *second* declaration of this context,
// which is how `wels_preprocess.rs` once read every one of these fields at the wrong
// offset -- and that property does not depend on the numbers being the C++'s.
macro_rules! assert_ctx_offset {
    ($field:ident, $off:expr) => {
        const _: () = assert!(std::mem::offset_of!(sWelsEncCtx, $field) == $off);
    };
}
assert_ctx_offset!(sLogCtx, 0);
assert_ctx_offset!(pSvcParam, 24);
assert_ctx_offset!(iMvRange, 32);
assert_ctx_offset_by_profile!(ppRefPicListExt, debug 136, release 120);
assert_ctx_offset_by_profile!(pLtr, debug 272, release 192);
assert_ctx_offset_by_profile!(bCurFrameMarkedAsSceneLtr, debug 280, release 200);
assert_ctx_offset_by_profile!(eSliceType, debug 284, release 204);
assert_ctx_offset_by_profile!(uiDependencyId, debug 313, release 233);
assert_ctx_offset_by_profile!(uiTemporalId, debug 314, release 234);
assert_ctx_offset_by_profile!(pWelsSvcRc, debug 320, release 240);
assert_ctx_offset_by_profile!(pVaa, debug 368, release 288);
assert_ctx_offset_by_profile!(pVpp, debug 376, release 296);
assert_ctx_offset_by_profile!(sSpatialIndexMap, debug 472, release 392);
assert_ctx_offset_by_profile!(bRefOfCurTidIsLtr, debug 536, release 440);
assert_ctx_offset_by_profile!(pMemAlign, debug 1760, release 1664);

// encoder_context.h:198 -- the element type of `sSpatialIndexMap`. `wels_preprocess.rs`
// carried a byte-identical copy of this under the invented name `SSpatialIndexMap`;
// a scan that compares identifiers cannot catch a rename, so the layout is pinned here.
// 16 in the C++; **12 debug / 8 release since T6.F1** — `pSrc` is an `Option<SrcPicId>`.
assert_size_by_profile!(SSpatialPicIndex, debug 12, release 8);
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, pSrc) == 0);
#[cfg(debug_assertions)]
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, iDid) == 8);
#[cfg(not(debug_assertions))]
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, iDid) == 4);




