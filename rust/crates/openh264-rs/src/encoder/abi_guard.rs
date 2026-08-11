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
use crate::common::expand_pic::SExpandPicFunc;
use crate::common::mc::SMcFunc;
use crate::encoder::encoder_context::{sWelsEncCtx, SLTRState, SSpatialPicIndex, SStrideTables};
use crate::encoder::md::{SMB, SMbCache, SSampleDealingFunc, SWelsMD};
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
assert_size!(SWelsNalRaw, 40);

// codec/encoder/core/inc/picture.h
assert_size!(SPicture, 136);
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
assert_size!(SExpandPicFunc, 24);
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
assert_size!(SSampleDealingFunc, 240);
assert_size!(SRCSlicing, 44);
assert_size!(SSpatialPicIndex, 16);
assert_size!(SStrideTables, 160);
assert_size!(SWelsME, 96);
assert_size!(SWelsMD, 4000);
assert_size!(SVAAFrameInfo, 264);
assert_size!(SVAAFrameInfoExt, 1280);
// SSliceThreading is deliberately NOT asserted. C++ (mt_defs.h:68) embeds
// WELS_EVENT (pthread_cond_t, 48 B) and WELS_MUTEX (pthread_mutex_t, 64 B) by
// value, reaching 1256 bytes on darwin; those sizes are libc-specific, and this
// port models the primitives as opaque handles. Nothing crosses a C ABI here, so
// the field-for-field size correspondence that holds for the codec's own structs
// does not apply. Revisit if the threading types are ever given real bodies.
assert_size!(SVAACalcResult, 72);
assert_size!(SScrollDetectionParam, 32);
assert_size!(SAdaptiveQuantizationParam, 40);
assert_size!(SComplexityAnalysisParam, 64);
assert_size!(SComplexityAnalysisScreenParam, 72);

// Mid-tier types.
assert_size!(SSpatialLayerInternal, 68);
assert_size!(SWelsSvcCodingParam, 1240);


assert_size!(SWelsSvcRc, 360);
assert_size!(SSliceCtx, 32);


// codec/encoder/core/inc/mb_cache.h, svc_enc_macroblock.h, svc_enc_frame.h
assert_size!(SMbCache, 576);
assert_size!(SMB, 152);
assert_size!(SLayerInfo, 48);

// codec/encoder/core/inc/slice.h. `assert_size!(SSlice, 1584)` was here — see the
// `SWelsSliceBs` note above; `SSlice` embeds it by value.

// codec/encoder/core/inc/svc_enc_frame.h
assert_size!(SSliceBufferInfo, 16);
assert_size!(SDqLayer, 512);

// codec/encoder/core/inc/wels_func_ptr_def.h
// 1280 before Phase 4a; -8 for `SSampleDealingFunc`'s shrink above; -24 at T4b.1,
// where four 8-byte entropy slots became one `EntropyCoder` discriminant (-32, +8
// for the byte and its padding to the pointer that follows); -64 at T4b.1b, where
// `SWelsRcFunc`'s nine slots became one `RCMode` (72 -> 4 bytes, and the member's
// alignment drops from 8 to 4). The number tracks the port, not the C++ header,
// from Phase 4 on: de-virtualization is the point.
assert_size!(SWelsFuncPtrList, 1184);

// codec/encoder/core/inc/encoder_context.h:116. C++ is 98008 bytes, but that number
// embeds WELS_MUTEX (pthread_mutex_t, 64 B on darwin) by value where this port models
// the mutex as an opaque 8-byte handle, exactly as it does in SSliceThreading. So the
// expected size here is 98008 - 64 + 8; alignment is 8 either way, so no padding
// changes. Everything else -- including sWelsCabacContexts[4][52][460] at 95,680
// bytes -- is a faithful match, which is what makes this assertion worth having.
assert_size!(crate::encoder::encoder_context::SParaSetOffset, 1180);
assert_size!(crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics, 88);
assert_size!(crate::encoder::encoder_context::SLogContext, 24);
assert_size!(sWelsEncCtx, 98008 - 64 + 8);

// The fifteen `sWelsEncCtx` fields the preprocessor touches, pinned at their C++
// offsets. `wels_preprocess.rs` used to declare its own 15-field `SWelsEncCtx` and
// alias `sWelsEncCtx` to it, so every one of these reads landed at the wrong offset
// the moment a real context was passed in -- which is exactly what
// `WelsEncoderEncodeExt` does when it calls `BuildSpatialPicList` / `AnalyzeSpatialPic`
// / `UpdateSpatialPictures`. A size assertion could not catch that (the fake struct
// was simply a different type), so the offsets are asserted directly.
//
// All fifteen are declared before `WELS_MUTEX mutexEncoderError` (encoder_context.h:230),
// the one member this port models differently, so each number below is the unmodified
// C++ `offsetof`, measured on darwin/arm64.
macro_rules! assert_ctx_offset {
    ($field:ident, $off:expr) => {
        const _: () = assert!(std::mem::offset_of!(sWelsEncCtx, $field) == $off);
    };
}
assert_ctx_offset!(sLogCtx, 0);
assert_ctx_offset!(pSvcParam, 24);
assert_ctx_offset!(iMvRange, 40);
assert_ctx_offset!(ppRefPicListExt, 184);
assert_ctx_offset!(pLtr, 320);
assert_ctx_offset!(bCurFrameMarkedAsSceneLtr, 328);
assert_ctx_offset!(eSliceType, 332);
assert_ctx_offset!(uiDependencyId, 361);
assert_ctx_offset!(uiTemporalId, 362);
assert_ctx_offset!(pWelsSvcRc, 368);
assert_ctx_offset!(pVaa, 416);
assert_ctx_offset!(pVpp, 424);
assert_ctx_offset!(sSpatialIndexMap, 520);
assert_ctx_offset!(bRefOfCurTidIsLtr, 600);
assert_ctx_offset!(pMemAlign, 1824);

// encoder_context.h:198 -- the element type of `sSpatialIndexMap`. `wels_preprocess.rs`
// carried a byte-identical copy of this under the invented name `SSpatialIndexMap`;
// a scan that compares identifiers cannot catch a rename, so the layout is pinned here.
assert_size!(SSpatialPicIndex, 16);
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, pSrc) == 0);
const _: () = assert!(std::mem::offset_of!(SSpatialPicIndex, iDid) == 8);
