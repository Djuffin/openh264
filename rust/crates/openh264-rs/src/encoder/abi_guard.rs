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
// **S6.B1 re-pin.** 88 was the C++ struct's size — five raw pointers, three scalars and
// a `[u32; 7]`. The storage owns its five buffers now, so the C++ number no longer
// describes this type and there is nothing to match it to: the type never crosses the
// ABI (`src/api/` does not name it, and `SPicture` holds it as a niche-optimised
// `Option<Box<..>>`, so `assert_size!(SPicture, ..)` is untouched). It stays pinned for
// the reason `SRefList` above is — one builder, one shape, and a silent change to it
// would be a silent change to how a reference frame stores its feature arena.
// **P10.1.B5 (D-scc-3): -24.** `pFeatureOfBlockPointer: Vec<u16>` is gone — it was
// the address of the layer's scratch (`SFeatureSearchPreparation::pFeatureOfBlock`),
// which reaches `CalculateFeatureOfBlock` as `&mut [u16]` now. **136, measured.**
assert_size!(SScreenBlockFeatureStorage, 136);

// codec/encoder/core/inc/parameter_sets.h
assert_size!(SWelsSPS, 56);
assert_size!(SWelsPPS, 16);
assert_size!(SSpsSvcExt, 4);
assert_size!(SSubsetSps, 60);

// codec/encoder/core/inc/slice.h. SSliceHeader excludes iSliceGroupChangeCycle,
// which sits inside `#if !defined(DISABLE_FMO_FEATURE)` at slice.h:124.
//
// **168 → 152 and 192 → 168 at T6.G3, and these are the port's own numbers now.**
// `SSliceHeader::pSps`/`pPps` and `SSliceHeaderExt::pSubsetSps` are deleted: all
// three were write-only or never touched at all, and the first two had their own
// replacement sitting in the same struct (`iSpsId`/`iPpsId`, which the C++ writes in
// the same statement block). `SSliceHeaderExt` loses its own 8 plus the 16 its
// embedded header lost. Measured.
assert_size!(SRefPicMarking, 100);
assert_size!(SRefPicListReorderSyntax, 16);
assert_size!(SSliceHeader, 152);
assert_size!(SSliceHeaderExt, 168);

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
//
// **T9.B25 (Phase 9 session B3): 176 -> 344, temporarily.** The struct carried
// a second, raw triple (+168) while the plane-family campaign converted the
// cost readers one function at a time. **Session F converted the last of them
// and deleted the triple — back to 176, exactly as this pin promised.**
assert_size!(SSampleDealingFunc, 176);
assert_size!(SRCSlicing, 44);
// **160 in the C++, and 208 is the port's own number since S11.46.** The struct
// held sixteen raw pointers into one block it did not own; T6.H1 gave it the
// block (`Vec<i32>`, 24 bytes) with the pointers as `Option<u32>` offsets (184),
// and S11.46 split that byte arena into two typed stores — `Vec<[i32; 24]>` and
// `Vec<i16>`, 24 bytes each — so the offsets became indices and the last raw
// cursor in the family went with them (+24). The pin stays because what it
// catches is unchanged — a field added, dropped, or given the wrong width, and a
// second declaration read at the wrong offsets — it just no longer claims
// correspondence with a C++ `sizeof`.
assert_size!(SStrideTables, 208);
// **96 -> 64 at session F**: the three per-block plane cursors (`pEncMb`/
// `pRefMb`/`pColoRefMb`) left the search block — the coordinates already in
// the struct carry the same information (the verified identity), and the
// search family takes the planes as parameters. -24 of pointer plus -8 of
// padding: without the pointer block the struct's tail packs at 2-byte
// alignment. Measured, not predicted.
// **64 -> 80 at S5.C4b**: `pMvdCost` stopped being a `*mut u16` and became a
// `MvdCostCursor` — the table as a `&[u16]` plus the index of the entry the raw
// cursor pointed at. `COST_MVD` indexes with a *signed* MVD, so the pointer was
// parked mid-table and no plain slice can stand in for it; the pair can. 8 bytes of
// thin pointer become 16 of fat pointer plus 8 of index, and the alignment is
// unchanged, so the tail packs exactly as it did. The struct is `repr(C)` and the
// cursor is not, which costs nothing here for the reason the note below gives: this
// family crosses no ABI, and the pin catches a field added, dropped or mis-widened
// rather than any correspondence with a C++ `sizeof`.
assert_size!(SWelsME, 80);
// **928 in the port, 4000 in the C++, and the gap is D-dead-2 (F122).** The struct
// carried `sMe4x4`/`sMe8x4`/`sMe4x8` — 32 `SWelsME` at 96 bytes, 3072 of the 4000 —
// whose only readers were `WelsMdInterMbRefinement`'s three sub-8x8 arms. Nothing in
// either encoder ever produced those partitions: upstream's sub-8x8 search is inside
// `#if 0 //Disable for sub8x8 modes for now` (`svc_mode_decision.cpp:634-661`), the
// same block D-dead-1 deleted `WelsMdP4x4`/`WelsMdP8x4`/`WelsMdP4x8` for. The pin
// stays, for the reason `SStrideTables`'s does: what it catches is a field added,
// dropped or mis-widened and a second declaration read at the wrong offsets — it just
// no longer claims correspondence with a C++ `sizeof`. `SWelsMD` is encoder-internal
// and crosses no ABI, so no header, no caller and no byte of output depends on the
// old number.
// **928 -> 640 at session F**: nine embedded `SWelsME` at -32 each (the
// container: sMe16x16 + sMe16x8[2] + sMe8x16[2] + sMe8x8[4]).
// **640 -> 800 at S5.C4b**: +16 for this struct's own `pMvdCost` and +16 for each
// of the nine `SWelsME` its `sMe` container embeds (sMe16x16 + sMe16x8[2] +
// sMe8x16[2] + sMe8x8[4]) — one `*mut u16` -> `MvdCostCursor` swap, counted ten
// times.
assert_size!(SWelsMD, 800);
// `SVAAFrameInfo`: 264 in the C++. Phase 6 session B dissolved the `IWelsVP`
// vtable and deleted the stored `pCalcResult` pointer from the two parameter
// blocks this embeds (`sAdaptiveQuantParam`, `sComplexityAnalysisParam` — the
// VAA result is handed to each plugin at its `Process` call instead), 8 bytes
// each: 248. `SVAAFrameInfoExt` embeds this one and moved by the same 16.
// **T6.F3**: +104. The six per-frame result arrays are the block's own `Vec`s, so
// six pointers become six fat pointers (+48) and `pVaaBackgroundMbFlag` a seventh
// (+16); `repr(C)` comes off with them and the compiler repacks. **352, measured.**
// **T9.X**: +24. `sAdaptiveQuantParam` loses two pointers (-16) and this struct
// gains the two owned `Vec`s they should have pointed at (+48) — `pMotionTextureUnit`
// and `pMotionTextureIndexToDeltaQp`, `encoder_ext.cpp:1721/:1724`, two allocations
// the port had never made at all (F177); `sComplexityAnalysisParam` then loses two
// more pointers (-16). **360, measured.**
// **S9.0c**: +160. The six `*mut u8` plane roots (-48) become two
// `Option<RoPicView>` (+208) — three `SharedPlane`s of (base, len, stride, origin)
// is 96 bytes and the `Option` has no niche over a raw base, so 104 each. The pin is
// a *drift tracker*, not an ABI contract — this struct has been off the C++'s 264
// since Phase 6 session B and `repr(C)` came off with T6.F3 — so it moves with a
// deliberate field change. **520, measured.**
// **S10.9: -16.** `sComplexityAnalysisParam` loses its last two pointers
// (`pBackgroundMbFlag`, `uiRefMbType`), which reach the plugin as slices at the
// `Process` call — the same move T9.X made for the two GOM arrays and Phase 6
// session B for `pCalcResult`. **`SVAAFrameInfo` is `Sync` as of this pin**; the
// `pCurY`/`pRefY` pair went to `usize` in the same checkpoint and cost no bytes.
// **504, measured.**
assert_size!(SVAAFrameInfo, 504);
// **S10.9: -16**, all of it its embedded `SVAAFrameInfo`. **1520, measured.**
// **S12.3: +176.** The screen block-static family stopped being pointers.
// `pVaaBlockStaticIdc` is one owned `SBlockStaticIdcStore` (40) where sixteen
// `*mut u8` stood (128), -88; `pVaaBestBlockStaticIdc` is an `Option<usize>` row
// number (16) where a pointer stood (8), +8; and the same swap inside
// `SRefInfoParam` (24 -> 32) is paid 32 times, once per slot of
// `sVaaStrBestRefCandidate` and `sVaaLtrBestRefCandidate`, +256. Nothing in the
// port allocates this struct (F177), so the growth is bytes no run ever takes.
// **1696, measured.**
// **P10.1.B2: -16**, all of it `sComplexityScreenParam` (D-scc-2). The struct is
// allocated now — `RequestMemorySvc` builds it under `SCREEN_CONTENT_REAL_TIME`
// as of P10.1.B3 — so the bytes are ones a screen encode takes. **1680, measured.**
assert_size!(SVAAFrameInfoExt, 1680);
// **T6.F3**: +104, all of it its embedded `SVAAFrameInfo`. **1368, measured.**
// **T9.X**: +8, all of it the same. **1376, measured.**
// **S9.0c**: +160, all of it its embedded `SVAAFrameInfo`. **1536, measured.**

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
// **T9.X**: `SAdaptiveQuantizationParam` 32 -> 8 — its two buffer pointers moved to
// `SVAAFrameInfo` as owned `Vec`s and reach the plugin as slices at the `Process`
// call, which is where this block's `pCalcResult` went at session B for the same
// reason. Two of `SVAAFrameInfo`'s four `!Sync` reasons go with them (F67/F164).
assert_size!(SAdaptiveQuantizationParam, 8);
// **T9.X**: 56 -> 40 — `pGomComplexity` and `pGomForegroundBlockNum` are the rate
// controller's `Vec`s and reach the plugin as slices; see `SAdaptiveQuantizationParam`
// above. **40, measured.**
// **S10.9**: 40 -> 24 — `pBackgroundMbFlag` and `uiRefMbType` go the same way, and
// with them `SVAAFrameInfo`'s last two `!Sync` reasons. **24, measured.**
assert_size!(SComplexityAnalysisParam, 24);

// **P10.1.B2 (D-scc-2)**: 72 -> 56 — `pGomComplexity` (`int*`) is gone; the screen
// plugin takes the GOM array as a slice at the call, as the camera one does, and
// the field was the last thing keeping `SVAAFrameInfoExt` `!Sync`. **56, measured.**
assert_size!(SComplexityAnalysisScreenParam, 56);

// Mid-tier types.
assert_size!(SSpatialLayerInternal, 68);
// **1240 -> 1232 at H2, and this one is a deliberate divergence from the C++ rather
// than a port shape.** **D-dead-7** (the user, 2026-08-26, from F183) deleted
// `pCurPath` (`param_svc.h:118`), an 8-byte `char*` with three writes and **no reader
// in either tree** — upstream declares it, nulls it and stores to it and never reads
// it anywhere in `codec/`. So `SWelsSvcCodingParam` is now one field short of its C++
// counterpart, on purpose, and this pin is where that is recorded: it is the file's
// only intentional field-count divergence. Every other pin here still means "the
// translation is field for field". Nothing outside the crate reads this layout (see
// the module note) — the struct is `param_svc.h`'s internal one, not
// `codec_app_def.h`'s `SEncParamExt`, which is untouched.
assert_size!(SWelsSvcCodingParam, 1232);


// **360 in the C++, and 440 is the port's own number since T6.H6** — the third pin
// in this file to make that move (`SPicture` at T6.F0, `SStrideTables` at T6.H1).
// The five raw pointers into the one `RcInitLayerMemory` block are five owned
// containers at 24 bytes each, +80. The pin stays for what it has always caught: a
// field added, dropped, or given the wrong width.
// **440 still at T9.C5**: `pGomCost` became `Vec<AtomicI32>`, the same three words
// — the only pin in this file a Miri data-race verdict has ever moved *without*
// moving. **416 at D-dead-3**, which deleted that field outright: nothing in either
// tree read it, so it was an accumulator, not state. **392 at D-dead-6**, which
// deleted `pGomComplexity` on the same ground and for the same reason — allocated,
// nulled and memset in the reference, read nowhere in either tree. Three owned
// containers at 24 bytes each now, +32 over the C++'s 360, and the arithmetic is
// the check: 416 - 24 = 392, one `Vec` gone and no repacking behind it. The pin
// stays for what it always caught — a field added, dropped, or given the wrong
// width.
assert_size!(SWelsSvcRc, 392);
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
// **+16 at T9.B30**: `SPicData` gains `iMbX`/`iMbY` (8 bytes), and `SMbCache` is
// `repr(C, align(16))`, so the struct rounds to the next multiple of 16. The pair is
// the port's own field — the coordinate the twelve plane pointers are a function of —
// and it is what lets a reader with no `SMB` in scope build a cursor. See the field's
// doc in `encoder_context.rs`.
// **-32 at T9.C4**: `SPicData` loses `pDecMb` — three pointers that were a second
// derivation of `pCsMb`'s three addresses, proved equal over 583 sweep rows in both
// profiles before being deleted — and the 8 bytes of `align(16)` rounding go with
// them. **5568, measured.**
// **-64 at S4.C2**: `SPicData` loses its last nine pointers — the `pEncMb`,
// `pRefMb` and `pCsMb` triples, resolved at use through
// `svc_encode_slice::{enc_mb, cs_mb, ref_mb}` from the `iMbX`/`iMbY` pair T9.B30
// added for exactly this. Nine pointers is 72 bytes and the number moves **64**:
// the struct is `repr(C, align(16))`, so 5568 - 72 = 5496 rounds back up to 5504.
// Measured, not derived — the arithmetic above is the explanation of a number the
// compiler was asked for, which is the only way to read a size delta on an
// aligned struct. **5504, measured.**
assert_size!(SMbCache, 5504);
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

// codec/encoder/core/inc/svc_enc_frame.h:77. **48 in the C++, 32 here since T6.G3**:
// its three parameter-set pointers are two id fields, and one of those two —
// `Option<LayerSps>` — is the tagged union the C++ spelled as "`pSubsetSpsP` null or
// not". The struct is no longer `repr(C)`, for `SDqLayer`'s reason: that `Option` has
// no C shape. The pin stays because it is the record of the port's own layout, which
// is what it has been since T6.C1 for everything around it.
assert_size!(SLayerInfo, 32);

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
// 768 release**, measured. **T6.G3**: -16, the `SLayerInfo` it embeds — **824 / 752**.
// **T9.B21**: +16, `pEncPic` (`Option<SrcPicId>`) and `pSrcPool` — the source
// picture said as a handle plus the pool that opens it, which is `pRefPic`/`pRefList`
// one picture over. **840 debug / 768 release**, measured.
// **T9.C2**: +168, `pRecView` (`Option<RecPicView>`) — the reconstruction seam:
// three shared planes at 32 bytes each and four shared per-macroblock arrays at 16,
// plus the `Option` discriminant, which costs a word because a captured base has no
// niche. **1008 debug / 936 release**, measured.
// **880 -> 760 debug, 808 -> 688 release at S5.D2b**: `sSliceBufferInfo` was
// `[SSliceBufferInfo; MAX_THREADS_NUM]` *inline* — four banks at 32 bytes each, 128 of
// them — and is a `Box` now, so the layer carries one pointer instead. -120 in both
// profiles, and the reason is not size: the banks had to leave the struct's own bytes
// so that a worker writing its bank stops racing a sibling body's whole-layer shared
// retag, which is what the `&SDqLayer` flip in D2/D3 needs (see the field's own note).
// **S9.0: +104 bytes in both profiles** — `pEncView: Option<RoPicView>`, the read
// half of the reconstruction seam, stamped beside `pRecView`. Three `RoPlane`s of
// (base, len, stride, origin) is 96 bytes and the `Option` has no niche to use over
// a raw base, so the discriminant costs 8 more. This pin does not describe a C-ABI
// contract — `SDqLayer` does not cross the boundary — it catches a *second
// declaration* of the type read at the wrong offsets, so it moves with a deliberate
// field addition. Both numbers measured, in the profile they name.
// **S10.5-S10.8: -64 in both profiles** — `pEncData`, `pCsData`, `pSrcPool` (all
// three write-only once the seams had taken their readers) and `pRefList` (live,
// re-resolved through the context on the layer's own dependency id). Deliberate
// field *removals*, so the pin moves with them for the reason stated above; every
// number re-measured in the profile it names, not adjusted by arithmetic.
// **`SDqLayer` is `Sync` as of this pin** — the four raw fields are gone.
// **P10.1.B4: +8 in both profiles** — `pFeatureSearchPreparation:
// Option<Box<SFeatureSearchPreparation>>` (`svc_enc_frame.h:126`), one word,
// niche-optimised. A deliberate field addition; both numbers measured in the
// profile they name. (The brief said no live `SDqLayer` size pin existed — this
// one is `assert_size_by_profile!`, which its grep for `assert_size!(` missed.)
assert_size_by_profile!(SDqLayer, debug 808, release 736);

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
// **+168 at T9.B25, temporarily**: `SSampleDealingFunc`'s transitional raw triple
// (see that pin above). Back to 1072 when it goes.
// **-104 at session F step 0** (F139's write-only slots, S18): the three
// `pfIDct*` slots (-24), `DeblockingFunc`'s eight kernel slots (-64), and
// `pfSampleSadHor8` (-16), all installed-and-never-read (or, for the last,
// never even installed). **-168 more at T9.F2b**: the transitional raw triple
// left `SSampleDealingFunc` (its pin above). **-16 at T9.F3**: the
// `pfDeblockingBSCalc` and `pfSetNZCZero` slots went direct (F118).
// **-8 at S4.C1**: `pfAccumulateSadForRc`, one `Option<PAccumulateSadFunc>` slot,
// deleted as dead (S18). Whole-tree grep at deletion found the type alias, this
// field and its one `None` initialiser and nothing else in `src/`, `tests/`,
// `benches/` or `rust/tools/` — never installed, never dispatched. The C++
// assigns it in `WelsInitEncodingFuncs`; the port's rate control reads its SADs
// directly, so the indirection never had a producer here.
assert_size!(SWelsFuncPtrList, 944);

// codec/encoder/core/inc/encoder_context.h:116. C++ is 98008 bytes, but that number
// embeds WELS_MUTEX (pthread_mutex_t, 64 B on darwin) by value where this port models
// the mutex as an opaque 8-byte handle, exactly as it does in SSliceThreading. So the
// expected size here is 98008 - 64 + 8; alignment is 8 either way, so no padding
// changes. Everything else -- including sWelsCabacContexts[4][52][460] at 95,680
// bytes -- is a faithful match, which is what makes this assertion worth having.
assert_size!(crate::encoder::encoder_context::SParaSetOffset, 1180);
assert_size!(crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics, 88);
// **T8.B6: 24 -> 32.** `SLogContext` is `common/utils.h:53`'s three `void*`s in
// the reference, and this port's is four members: the callback, the caller's
// context, the instance *address* and the trace level. The last two are what the
// reference reaches through `pfLog`'s back-pointer into the `welsCodecTrace` — a
// route that cannot be written here without the F38 hazard (see
// `common::wels_trace`), so the values travel instead of a pointer to them. Every
// `sWelsEncCtx` offset below `sLogCtx` moves by 8 and the context grows by 8;
// re-measured, as at T6.C1/T6.D5/T7.B4/T7.C6, not adjusted.
assert_size!(crate::encoder::encoder_context::SLogContext, 32);
// **T6.C1**: -40, the five per-macroblock scratch pointers deleted with `SMB`'s
// conversion (see `assert_size!(SMB, …)` above). Was `98008 - 64 + 8` = 97952, the
// C++ size with this port's 8-byte mutex handle in place of `pthread_mutex_t`.
// **T6.D5**: -8 more, `ppMbListD` deleted — `InitMbListD` cut one flat block across
// the layers and stored the cuts twice, and each layer owns its own `MbArray<SMB>`
// now. **97904, measured.**
// **T6.F1**: -16 debug / -112 release. `pEncPic`/`pDecPic`/`pRefPic` and
// `pRefList0[16]` are handles now — nineteen 8-byte pointers become nineteen 8-byte
// (debug) or 4-byte (release) handles.
// **T6.G2**: **-8 debug, 0 release**, and the asymmetry is the whole story.
// `pCurDqLayer` (an 8-byte pointer) becomes `iCurDqLayer` (`Option<LayerIdx>`, 2
// bytes, align 1). In release it lands in the padding the following pointer-to-
// pointer field already required, so *nothing* after it moves and the size does not
// change; in debug the preceding handles are 8 bytes rather than 4, the hole falls
// elsewhere, and every offset from `ppDqLayerList` on drops by 8. Both numbers are
// measured, in the profile they name — S36, and a reminder that predicting a
// `repr(C)` offset is not measuring it.
// **T6.G3**: **-8 both profiles.** `pSps`/`pPps` become `Option<SpsId>` /
// `Option<PpsId>` (2 and 4 bytes against 8 apiece) and `pSubsetSps` is deleted
// outright — the C++ declares it, nothing ever read or wrote it. Only the three pins
// after the parameter-set block move.
// **T6.I0**: **-8 both profiles.** `pPSOVector` — the C++'s `SParaSetOffset*`
// companion to the by-value `sPSOVector` — is deleted: declared, null-initialised,
// listed in the equality instrument, and never read or assigned anywhere in the
// port. It sits after all but one of the pinned offsets below, so **only
// `pMemAlign` moves**; the session brief predicted all fifteen would, and the
// measurement says otherwise (S36 again — 14 of the 15 pins precede the field).
// **T7.C5**: **+64 both profiles**, and it is one field. `pDynamicBsBuffer` was an
// array of four raw pointers into `CMemoryAlign` blocks and is `[Vec<u8>; 4]` now —
// four one-word pointers become four three-word `Vec`s, +16 apiece, and the array's
// alignment does not move so nothing else shifts. Both numbers measured, in the
// profile they name (S36). The field sits after all fifteen pinned offsets below, so
// **none of them moves** — which is the same thing T6.I0's note says about
// `pPSOVector`, and the reason to measure rather than predict.
// **T7.C6**: **-8 both profiles.** `pMemAlign` — one pointer — is deleted with the
// allocator it named. It sits after all fourteen surviving pins and before
// `pDynamicBsBuffer`, so nothing pinned moves and the size falls by exactly the
// pointer. Measured, in the profile it names (S36).
assert_size_by_profile!(sWelsEncCtx, debug 98064, release 97976);


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
// **T7.B4 moved eleven of them by 8 and the size by 24.** `pTaskManage` — a pointer
// typed as opaque, standing where C++ has `IWelsTaskManage*` — sat ahead of every pin
// below and is deleted with the pool it pointed at, so each offset from
// `ppRefPicListExt` on falls by exactly 8. The three that do not move (`sLogCtx`,
// `pSvcParam`, `iMvRange`) are the three that precede it. The size falls by 24 rather
// than the 16 the two deleted pointers occupy, because `mutexEncoderError` also went
// and `iEncoderError`/`bDeliveryFlag` now pack where its alignment padding was.
// This is the guard doing its job, not a failure: it fired on the first build after
// the field went, and the numbers below are re-measured, not adjusted.
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
// **T7.C6 deleted the fifteenth pin with its field.** `pMemAlign` was pinned here
// because `wels_preprocess.rs` reads it — and by this session it did not: both of its
// `let pMa = (*pCtx).pMemAlign;` lines were dead bindings, and the whole allocator has
// retired from `src/encoder`. A pin over a field that does not exist guards nothing;
// **fourteen remain**, and the property the block was written for is unchanged, since
// what it catches is a *second declaration* of this context reading fields at the
// wrong offsets. The other fourteen all precede the deleted field, so none of them
// moves.

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









