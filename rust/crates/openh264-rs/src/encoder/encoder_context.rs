#![deny(unsafe_code)]
pub const MAX_DEPENDENCY_LAYER: usize = 4;
/// OpenH264 Video Encoder Core Context and State Machine
///
/// Translated from `codec/encoder/core/inc/encoder_context.h` and
/// `codec/encoder/core/src/encoder.cpp`.

use std::ffi::c_char;
use crate::api::codec_api::ECOMPLEXITY_MODE::*;
use crate::{
    EUsageType, RCMode, SEncParamExt, SEncoderStatistics, SSliceArgument,
    SSpatialLayerConfig, SSourcePicture, VideoFormat,
    MAX_QUALITY_LAYER_NUM, MAX_TEMPORAL_LAYER_NUM,
};

// ============================================================================
// Core Constants
// ============================================================================

pub const MAX_TEMPORAL_LEVEL: usize = MAX_TEMPORAL_LAYER_NUM;
/// `wels_const.h:113` — `(1<<(MAX_TEMPORAL_LEVEL-1))` = 8.
pub const MAX_GOP_SIZE: usize = 1 << (MAX_TEMPORAL_LEVEL - 1);
/// `wels_const.h:115` — `(MAX_GOP_SIZE>>1)` = 4. The trailing C++ comment says
/// "16 in standard", which is what this port had hard-coded; the encoder's own
/// limit is 4.
pub const MAX_SHORT_REF_COUNT: usize = MAX_GOP_SIZE >> 1;
pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MAX_QUALITY_LEVEL: usize = MAX_QUALITY_LAYER_NUM;
pub const WELS_QP_MAX: usize = 51;
pub const WELS_CONTEXT_COUNT: usize = 460;
pub const MAX_THREADS_NUM: usize = 4;
pub const I420_PLANES: usize = 3;
pub const BASE_DEPENDENCY_ID: i32 = 0;
pub const VGOP_SIZE: i32 = 8;

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 1;

// CPU Feature Bit Flags

// Complexity modes come from api::codec_api::ECOMPLEXITY_MODE.

// Intra Prediction Mode Count Constants
pub const I16_PRED_DC_A: usize = 7;
pub const I4_PRED_A: usize = 14;
pub const C_PRED_A: usize = 7;
/// Last variant of `EStaticBlockIdc` (`IWelsVP.h:148`) — value **3**, matching the
/// `EStaticBlockIdc` enum in `wels_preprocess.rs`. This was 5 here, which over-sized
/// `SWelsFuncPtrList::pfMotionSearch[BLOCK_STATIC_IDC_ALL]`.
pub const BLOCK_STATIC_IDC_ALL: usize = 3;
/// `wels_const.h:147` — last variant of the block-size enum, value 7. This was 8 here,
/// which would have over-sized `SScreenBlockFeatureStorage::uiSadCostThreshold` and the
/// `SSampleDealingFunc` / `SWelsFuncPtrList` function-pointer tables.
pub const BLOCK_SIZE_ALL: usize = 7;
/// `wels_const.h:131` — `MAX_DEPENDENCY_LAYER`.
pub const MAX_DQ_LAYER_NUM: usize = MAX_DEPENDENCY_LAYER;
/// `wels_const.h:51-52` — `MAX_PPS_COUNT_LIMITED`.
pub const MAX_PPS_COUNT: usize = 57;
/// `wels_const.h:54` — SPS+PPS.
pub const PARA_SET_TYPE: usize = 3;

// ============================================================================
// Bit Arithmetic Macros & Helpers
// ============================================================================

/// Calculates 4-byte (32-bit DWORD) aligned row stride for bitmap image buffers.
#[inline(always)]
pub fn CALC_BI_STRIDE(width: i32, bitcount: i32) -> i32 {
    ((width * bitcount + 31) & !31) >> 3 
}

// ============================================================================
// Enums
// ============================================================================

pub use crate::common::wels_common_defs::EWelsSliceType;


// Re-export EVideoFrameType from crate root
pub use crate::EVideoFrameType;

// ============================================================================
// Core Supporting Structures
// ============================================================================

// **T8.B6: `SLogContext` was declared here and in `decoder/decoder_context.rs`**
// — the census's `type SLogContext x2` — and this copy typed all three of its
// members `*mut c_void`, so the callback the encoder was supposed to reach was an
// erased pointer nothing could have called even if something had tried. One
// declaration now, in `common::wels_trace`, where `utils.h` puts it.
pub use crate::common::wels_trace::SLogContext;

/// `SMVUnitXY` — codec/encoder/core/inc/wels_common_basis.h:50. 4 bytes.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SMVUnitXY {
    pub iMvX: i16,
    pub iMvY: i16,
}

impl SMVUnitXY {
    #[inline(always)]
    pub fn new(x: i16, y: i16) -> Self {
        Self { iMvX: x, iMvY: y }
    }

    #[inline(always)]
    pub fn sDeltaMv(&mut self, v0: SMVUnitXY, v1: SMVUnitXY) -> &mut Self {
        self.iMvX = v0.iMvX.wrapping_sub(v1.iMvX);
        self.iMvY = v0.iMvY.wrapping_sub(v1.iMvY);
        self
    }

    #[inline(always)]
    pub fn sAssignMv(&mut self, v0: SMVUnitXY) -> &mut Self {
        self.iMvX = v0.iMvX;
        self.iMvY = v0.iMvY;
        self
    }
}

/// `SCropOffset` — codec/encoder/core/inc/wels_common_basis.h:105.
/// The fields are `int16_t` in C++; this copy had them as i32.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SCropOffset {
    pub iCropLeft: i16,
    pub iCropRight: i16,
    pub iCropTop: i16,
    pub iCropBottom: i16,
}

/// `SDCTCoeff` — codec/encoder/core/inc/mb_cache.h:62.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDCTCoeff {
    pub iLumaBlock: [[i16; 16]; 16],
    pub iLumaI16x16Dc: [i16; 16],
    pub iChromaBlock: [[i16; 16]; 8],
    pub iChromaDc: [[i16; 4]; 2],
}

impl Default for SDCTCoeff {
    fn default() -> Self {
        Self {
            iLumaBlock: [[0; 16]; 16],
            iLumaI16x16Dc: [0; 16],
            iChromaBlock: [[0; 16]; 8],
            iChromaDc: [[0; 4]; 2],
        }
    }
}

/// `SPicData` — codec/encoder/core/inc/mb_cache.h:130.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPicData {
    // `pEncMb`, `pRefMb` and `pCsMb` — three `[*mut u8; 3]` cursor triples — stood
    // here, with `pDecMb` between the first two.
    //
    // **`pDecMb` went first, S18/T9.C4**, proved redundant rather than assumed so:
    // `WelsMdIntraInit` stamped it from `pDecPic.planes()` and stamped `pCsMb` from
    // `(*pCurLayer).pCsData` — two derivations of *one* address, because
    // `WelsInitCurrentLayer` fills `pCsData` from that same `planes()` call. A
    // `debug_assert_eq!` of the two, in both branches of the stamp, was carried
    // through a whole `gates.sh family` (583 rows x both profiles) and never fired;
    // planting `.wrapping_add(1)` on one side aborted every row of the first preset,
    // so the assertion had teeth.
    //
    // **The other nine went in S4.C2**, on the argument this field pair was added to
    // make: every one of them was `root + ((iMbX + iMbY * stride) << shift)`, so a
    // reader holding the pair and the picture never needed the pointer. They are
    // resolved at use now — `svc_encode_slice::{enc_mb, cs_mb, ref_mb}` — which
    // deletes both the storage and the *roving*: the stamps recomputed the triples
    // at a row or slice start and otherwise **walked** them a macroblock at a time,
    // and a walked cursor is only correct while the walk's guard holds. The guard
    // did hold (the walk ran exactly when the previous macroblock was this one's
    // left neighbour, making `previous + 16` and the absolute form the same
    // address), which is why this conversion is byte-identical — but it was an
    // invariant spread across two functions and nine assignments, and now it is
    // three expressions.
    /// The macroblock this cache is on, in macroblocks — **T9.B30, and the port's
    /// own field**, not the C++'s. It was carried beside the twelve pointers; it is
    /// what remains of them.
    ///
    /// Every one of the twelve pointers above is the same function of this pair and a
    /// picture: `plane(i) + ((iMbX + iMbY * stride) << (4 for luma, 3 for chroma))`,
    /// which is exactly how `WelsMdIntraInit`/`WelsMdInterInit` stamp them. A reader
    /// that has the pair and the picture does not need the pointer, and a coordinate
    /// is the one form of this information no retag can invalidate (S54's value half,
    /// F112's rule for the arena's roots).
    ///
    /// It is carried here rather than fetched from `SMB` because three of the readers
    /// have neither an `SMB` nor a slice in scope — `WelsMdI16x16`, `WelsMdIntraChroma`
    /// and (for its chroma half) `WelsMdIntraSecondaryModesEnc` — and threading a
    /// fourth parameter through their dispatch slots would be a bigger change than
    /// the eight bytes this costs.
    pub iMbX: i32,
    pub iMbY: i32,
}

impl SPicData {
    /// **The offset the twelve deleted pointers were.** This macroblock's origin as
    /// a byte offset into a plane of `stride` — `((iMbX + iMbY * stride) << 4)` for
    /// luma and `<< 3` for chroma, which is exactly how `WelsMdIntraInit` and
    /// `WelsMdInterInit` computed each cursor before stamping it (S4.C2).
    ///
    /// **Chroma reads stride index 1 for both chroma planes**, not 2 — the stamps
    /// computed one `iOffsetUV` from `stride(1)` and applied it to planes 1 *and* 2.
    /// [`stride_idx`](Self::stride_idx) is that rule, named once so a resolver
    /// cannot get it subtly wrong.
    #[inline]
    pub fn mb_offset(&self, stride: i32, plane: usize) -> isize {
        let shift = if plane == 0 { 4 } else { 3 };
        (((self.iMbX + self.iMbY * stride) as isize) << shift)
    }

    /// The stride index a plane resolves through: luma 0, **both chroma planes 1**.
    #[inline]
    pub fn stride_idx(plane: usize) -> usize {
        if plane == 0 { 0 } else { 1 }
    }

    /// **The macroblock cursor the twelve stored pointers used to be** (S4.C2), and
    /// a **safe** fn: forming and offsetting a raw pointer needs no `unsafe` — only
    /// dereferencing one does, and that belongs to the kernels these are handed to.
    ///
    /// `SPicData` carried three `[*mut u8; 3]` triples that `WelsMdIntraInit` and
    /// `WelsMdInterInit` stamped once per macroblock and then **walked** — advancing
    /// each by one macroblock width for every macroblock that was neither its row's
    /// first nor its slice's first. Every one of them was
    /// `roots[plane] + ((iMbX + iMbY * stride) << shift)`, which is what T9.B30 put
    /// the coordinate pair here to say: "a reader that has the pair and the picture
    /// does not need the pointer". This resolves that expression at use.
    ///
    /// **It is byte-identical, and the walk's own guard is why**: the advance ran
    /// exactly when the previous macroblock was this one's left neighbour, so
    /// `previous + 16` and the absolute form name the same address. The invariant
    /// was spread over two functions and nine assignments; now there is nothing to
    /// keep in sync.
    ///
    /// **The chroma stride rule lives here and nowhere else.** Both chroma planes
    /// resolve through stride index **1** — the stamps computed a single `iOffsetUV`
    /// from `stride(1)` and applied it to planes 1 *and* 2 — so a caller passing
    /// `strides[2]` for plane 2 would be wrong on any picture whose chroma strides
    /// differ. [`stride_idx`](Self::stride_idx) is that rule; this is its consumer.
    #[inline]
    pub fn mb_cursor(&self, roots: &[*mut u8; 3], strides: &[i32; 3], plane: usize) -> *mut u8 {
        roots[plane].wrapping_offset(self.mb_offset(strides[Self::stride_idx(plane)], plane))
    }

    /// The same macroblock cursor, taken from a **picture view** instead of a raw
    /// plane root — S9.0, and the safe replacement for `mb_cursor` on the source
    /// planes.
    ///
    /// It hands back a [`RecCursor`](crate::encoder::rec_view::RecCursor), not a
    /// `PlaneCursor`: the source picture is written in-fork by
    /// `VaaBackgroundMbDataUpdate` (F117), so its planes live behind the shared seam
    /// and no `&[u8]` may span them. See `RoPicView`'s note for why S9.0a's
    /// slice-based form was wrong.
    ///
    /// **Byte-identical to the raw form, and the arithmetic is why.**
    /// `mb_offset` is `((iMbX + iMbY * stride) << shift)`, which expands to
    /// `(iMbX << shift) + (iMbY << shift) * stride` — exactly `x + y * stride` for
    /// the `(x, y)` that [`luma_origin`](Self::luma_origin) and
    /// [`chroma_origin`](Self::chroma_origin) give. The raw root is the plane's
    /// *padded origin*, and `RoPlane::cursor` anchors from that same origin, so the
    /// two name one address.
    ///
    /// **On the chroma stride rule** ([`stride_idx`](Self::stride_idx)): the raw
    /// form resolves both chroma planes through `strides[1]`, while a view's plane
    /// carries its own. They agree by construction — `AllocPicture` builds planes 1
    /// and 2 with one `kuiChromaStride` (`picture.rs:262-273`) and `iLineSize` is
    /// read back off those same planes — so this cannot be the divergence
    /// `stride_idx` was written to prevent. The rule stays where it is for the raw
    /// callers that remain.
    #[inline]
    pub fn mb_cursor_ro<'a>(
        &self,
        view: &'a crate::encoder::rec_view::RoPicView,
        plane: usize,
    ) -> crate::encoder::rec_view::RecCursor<'a> {
        let (x, y) = if plane == 0 { self.luma_origin() } else { self.chroma_origin() };
        view.plane(plane).cursor(x, y)
    }

    /// The macroblock cursor over the **reconstruction** view — the write half's
    /// counterpart to [`mb_cursor_ro`](Self::mb_cursor_ro), with the same geometry
    /// argument. `pCsData`'s raw roots stand for exactly these bytes.
    #[inline]
    pub fn mb_cursor_rec<'a>(
        &self,
        view: &'a crate::encoder::rec_view::RecPicView,
        plane: usize,
    ) -> crate::encoder::rec_view::RecCursor<'a> {
        let (x, y) = if plane == 0 { self.luma_origin() } else { self.chroma_origin() };
        view.plane(plane).cursor(x, y)
    }

    /// The macroblock's origin in luma samples — `(iMbX << 4, iMbY << 4)`.
    #[inline]
    pub fn luma_origin(&self) -> (isize, isize) {
        ((self.iMbX as isize) << 4, (self.iMbY as isize) << 4)
    }

    /// The macroblock's origin in chroma samples — `(iMbX << 3, iMbY << 3)`.
    #[inline]
    pub fn chroma_origin(&self) -> (isize, isize) {
        ((self.iMbX as isize) << 3, (self.iMbY as isize) << 3)
    }
}

impl Default for SPicData {
    fn default() -> Self {
        Self { iMbX: 0, iMbY: 0 }
    }
}

/// `SMVComponentUnit` — codec/encoder/core/inc/wels_common_basis.h:66.
/// Luma only: the MV cache is 5x6-1 = 29 entries, the ref-index cache 5x6 = 30.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SMVComponentUnit {
    pub sMotionVectorCache: [SMVUnitXY; 29],
    pub iRefIndexCache: [i8; 30],
}

impl Default for SMVComponentUnit {
    fn default() -> Self {
        Self {
            sMotionVectorCache: [SMVUnitXY::default(); 29],
            iRefIndexCache: [0; 30],
        }
    }
}






pub use crate::encoder::svc_encode_slice::SDqLayer;
use crate::encoder::svc_encode_slice::ctx_sps;

pub use crate::encoder::wels_encoder_ext::{SSpatialLayerInternal, SWelsSvcCodingParam};



pub use crate::encoder::nal_encap::SWelsEncoderOutput;


/// `TagParaSetOffsetVariable` — `codec/encoder/core/inc/wels_common_basis.h:72`.
/// 80 bytes. Note `iParaSetIdDelta` is `[MAX_DQ_LAYER_NUM]`; the `+1` in the header
/// is commented out.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SParaSetOffsetVariable {
    /// delta between SPS_ID_in_bs and sps_id_in_encoder, per dq-layer; may be negative
    pub iParaSetIdDelta: [i32; MAX_DQ_LAYER_NUM],
    /// marks the used SPS_ID with 1
    pub bUsedParaSetIdInBs: [bool; MAX_PPS_COUNT],
    /// the next SPS_ID_in_bs, for all layers
    pub uiNextParaSetIdToUseInBs: u32,
}

impl Default for SParaSetOffsetVariable {
    fn default() -> Self {
        Self {
            iParaSetIdDelta: [0; MAX_DQ_LAYER_NUM],
            bUsedParaSetIdInBs: [false; MAX_PPS_COUNT],
            uiNextParaSetIdToUseInBs: 0,
        }
    }
}

/// `TagParaSetOffset` — `codec/encoder/core/inc/wels_common_basis.h:79`. 1180 bytes.
///
/// This was a `[i32; 32]` placeholder (128 bytes). `eSpsPpsIdStrategy` is **not** a
/// member: `wels_common_basis.h:89` guards it with `#if _DEBUG`, which this build
/// does not set — the C++ `sizeof` of 1180 confirms it is absent.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SParaSetOffset {
    pub sParaSetOffsetVariable: [SParaSetOffsetVariable; PARA_SET_TYPE],
    pub bPpsIdMappingIntoSubsetsps: [bool; MAX_DQ_LAYER_NUM],
    pub iPpsIdList: [[i32; MAX_PPS_COUNT]; MAX_DQ_LAYER_NUM],

    pub uiNeededSpsNum: u32,
    pub uiNeededSubsetSpsNum: u32,
    pub uiNeededPpsNum: u32,

    pub uiInUseSpsNum: u32,
    pub uiInUseSubsetSpsNum: u32,
    pub uiInUsePpsNum: u32,
}

impl Default for SParaSetOffset {
    fn default() -> Self {
        Self {
            sParaSetOffsetVariable: [SParaSetOffsetVariable::default(); PARA_SET_TYPE],
            bPpsIdMappingIntoSubsetsps: [false; MAX_DQ_LAYER_NUM],
            iPpsIdList: [[0; MAX_PPS_COUNT]; MAX_DQ_LAYER_NUM],
            uiNeededSpsNum: 0,
            uiNeededSubsetSpsNum: 0,
            uiNeededPpsNum: 0,
            uiInUseSpsNum: 0,
            uiInUseSubsetSpsNum: 0,
            uiInUsePpsNum: 0,
        }
    }
}

/// `TagDqIdc` — `codec/encoder/core/inc/dq_map.h:50`. 4 bytes.
///
/// This port previously declared `{ uiDId, uiQId, uiTId }`, which is neither the
/// field set nor the size of the C++ struct; `InitDqLayers` writes `iPpsId`,
/// `iSpsId` and `uiSpatialId`.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDqIdc {
    pub iPpsId: u16,
    pub iSpsId: u8,
    pub uiSpatialId: i8,
}

pub use crate::encoder::svc_encode_slice::{SMB, SSlice};



pub use crate::encoder::svc_encode_slice::SWelsSvcRc;

// The real ports (svc_mode_decision.cpp:236 and :257) live in svc_mode_decision.rs;
// this module used to carry stubs that took *mut c_void and returned false.
use crate::encoder::svc_mode_decision::{
    WelsMdInterJudgeBGDPskip, WelsMdInterJudgeBGDPskipFalse, WelsMdUpdateBGDInfo,
    WelsMdUpdateBGDInfoNULL,
};


pub use crate::encoder::deblocking::DeblockingFunc as SDeblockingFunc;

pub use crate::encoder::rc::SWelsRcFunc;
pub use crate::encoder::nal_encap::EWelsNalUnitType;
pub use crate::encoder::nal_encap::EWelsNalRefIdc;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::picture::{RecPicId, RecPicPool, SrcPicId, SrcPicPool};
pub use crate::encoder::param_svc::SWelsSPS;
pub use crate::encoder::param_svc::SWelsPPS;
pub use crate::encoder::param_svc::SSubsetSps;
pub use crate::encoder::param_svc::{PpsId, SpsId, SubsetSpsId};
pub use crate::encoder::wels_preprocess::ESceneChangeIdc;
pub use crate::encoder::set_mb_syn_cabac::SStateCtx;
pub use crate::encoder::md::SMcFunc;
pub use crate::encoder::slice_multi_threading::SSliceThreading;
pub use crate::encoder::wels_preprocess::SVAAFrameInfo;
pub use crate::encoder::wels_preprocess::SVAAFrameInfoExt;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
pub use crate::encoder::wels_func_ptr_def::{EntropyCoder, SWelsFuncPtrList};


// ============================================================================
// Primary Encoder Context Data Structures (encoder_context.h)
// ============================================================================

/// Reference picture lists for each spatial dependency/quality layer in SVC.
///
/// **Not `#[repr(C)]` and not `Copy` since T6.F1**, and `Box`-built with a real
/// constructor: `pRef` *is* this layer's reconstruction pool, so the struct owns its
/// pictures and a `WelsMallocz`'d shell would be UB at the pool's first drop (S21).
/// It is reached only through `sWelsEncCtx::ppRefPicListExt`, a raw pointer field in
/// the zeroed context — T3.6's precedent, the same argument `SDqLayer` was rebuilt
/// on in session D.
///
/// The two lists and `pNextBuffer` are **handles into `pRef`**, not addresses.
#[derive(Debug)]
pub struct SRefList {
    pub pShortRefList: [Option<RecPicId>; 1 + MAX_SHORT_REF_COUNT],
    pub pLongRefList: [Option<RecPicId>; 1 + MAX_REF_PIC_COUNT],
    pub pNextBuffer: Option<RecPicId>,
    /// The pool. Was `[*mut SPicture; 1 + MAX_REF_PIC_COUNT]`, allocated one picture
    /// at a time by `RequestMemorySvc` and freed one at a time by `FreeMemorySvc`.
    pub pRef: RecPicPool,
    pub uiShortRefCount: u8,
    pub uiLongRefCount: u8,
}

impl SRefList {
    /// An empty reference list for one dependency layer. `RequestMemorySvc` fills
    /// [`pRef`](Self::pRef) immediately after and points `pNextBuffer` at slot 0,
    /// exactly as the C++ does over its `WelsMallocz`'d block.
    ///
    /// **F56**: every field's zero is written out. The C++ takes this from
    /// `WelsMallocz`, so the lists are null and both counts are 0 — and `None` is
    /// that null, ruled rather than inherited from a zero image.
    pub fn new() -> Box<SRefList> {
        Box::new(SRefList {
            pShortRefList: [None; 1 + MAX_SHORT_REF_COUNT],
            pLongRefList: [None; 1 + MAX_REF_PIC_COUNT],
            pNextBuffer: None,
            pRef: RecPicPool::empty(),
            uiShortRefCount: 0,
            uiLongRefCount: 0,
        })
    }

    /// The picture a handle out of one of this list's arrays names.
    ///
    /// Every `RecPicId` in `pShortRefList`, `pLongRefList`, `pNextBuffer`, the
    /// context's `pDecPic`/`pRefPic`/`pRefList0` and the layer's `pDecPic`/`pRefPic`
    /// is a slot of *this* list's `pRef`, so this is the one resolution they all use.
    #[inline]
    pub fn pic(&self, id: RecPicId) -> &SPicture {
        self.pRef.get(id)
    }

    /// Mutable form of [`pic`](Self::pic).
    #[inline]
    pub fn pic_mut(&mut self, id: RecPicId) -> &mut SPicture {
        self.pRef.get_mut(id)
    }
}


/// Long-Term Reference (LTR) State Machine.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLTRState {
    pub uiLtrMarkState: u32,
    pub iLtrMarkFbFrameNum: i32,
    pub iLastRecoverFrameNum: i32,
    pub iLastCorFrameNumDec: i32,
    pub iCurFrameNumInDec: i32,
    pub iLTRMarkMode: i32,
    pub iLTRMarkSuccessNum: i32,
    pub iCurLtrIdx: i32,
    pub iLastLtrIdx: [i32; MAX_TEMPORAL_LAYER_NUM],
    pub iSceneLtrIdx: i32,
    pub uiLtrMarkInterval: u32,
    pub bLTRMarkingFlag: bool,
    pub bLTRMarkEnable: bool,
    pub bReceivedT0LostFlag: bool,
}

impl Default for SLTRState {
    fn default() -> Self {
        Self {
            uiLtrMarkState: 0,
            iLtrMarkFbFrameNum: 0,
            iLastRecoverFrameNum: 0,
            iLastCorFrameNumDec: 0,
            iCurFrameNumInDec: 0,
            iLTRMarkMode: 0,
            iLTRMarkSuccessNum: 0,
            iCurLtrIdx: 0,
            iLastLtrIdx: [0; MAX_TEMPORAL_LAYER_NUM],
            iSceneLtrIdx: 0,
            uiLtrMarkInterval: 0,
            bLTRMarkingFlag: false,
            bLTRMarkEnable: false,
            bReceivedT0LostFlag: false,
        }
    }
}

/// Planar YUV 4:2:0 source picture paired with spatial dependency ID.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSpatialPicIndex {
    pub pSrc: Option<SrcPicId>,
    pub iDid: i32,
}

impl Default for SSpatialPicIndex {
    fn default() -> Self {
        Self {
            pSrc: None,
            iDid: 0,
        }
    }
}

/// Stride and coordinate lookup tables across spatial dependency layers.
///
/// **T6.H1 — the arena is owned, and the four tables are offsets into it.**
///
/// `AllocStrideTables` takes *one* block from the allocator (`tag!("pBase")`) and
/// carves four regions out of it: the decoder-side 4x4 block offsets, the
/// encoder-side ones, and the two macroblock coordinate tables. The C++ stores a
/// pointer per layer into that one block, and so did this port; the block itself
/// was reachable only as `pStrideDecBlockOffset[0][1]`, which is how
/// `WelsUninitEncoderExt` used to free it.
///
/// It is an arena, so it converts as an arena: **one owned `Vec`, and the per-layer
/// fields become byte offsets into it.** Per-field `Vec`s would be four allocations
/// where the C++ has one, and — the reason they are actually wrong rather than
/// merely different — they would change *what a cursor may reach*. Two layers can
/// share one region: when a spatial layer is absent from the temporal map,
/// `AllocStrideTables` assigns it the matching layer's table, so
/// `pStrideDecBlockOffset[i][t]` and `[j][t]` are the same address. Copying an
/// offset reproduces that exactly; four separate allocations could not.
///
/// The offsets are **bytes**, because the arena's own arithmetic is in bytes
/// (`kiUnit1Size` is `24 * sizeof(int32_t)`) and keeping it that way leaves
/// `AllocStrideTables` a statement-for-statement translation. `None` is the null
/// the field used to hold: "this layer has no table", which
/// `AllocStrideTables` writes for every layer past `kiNumSpatialLayers`.
///
/// The backing store is `Vec<i32>` rather than `Vec<u8>` for its **alignment**: the
/// two block-offset regions are read as `i32`, and a `Vec<u8>` is only byte-aligned.
/// Every `i32` region starts at a multiple of `kiUnit1Size` (96) and every `i16`
/// region at an even offset, so both casts below are aligned by construction.
pub struct SStrideTables {
    /// The one block. Zero-filled, as `WelsMallocz` left it.
    base: Vec<i32>,
    pub pStrideDecBlockOffset: [[Option<u32>; 2]; MAX_DEPENDENCY_LAYER],
    pub pStrideEncBlockOffset: [Option<u32>; MAX_DEPENDENCY_LAYER],
    pub pMbIndexX: [Option<u32>; MAX_DEPENDENCY_LAYER],
    pub pMbIndexY: [Option<u32>; MAX_DEPENDENCY_LAYER],
}

impl SStrideTables {
    /// The tables with their arena sized at `kiNeedAllocSize` **bytes** and no layer
    /// wired yet — `WelsMallocz(iNeedAllocSize)` plus the memset the struct itself got.
    pub fn new(kiNeedAllocSize: i32) -> Self {
        let words = (kiNeedAllocSize.max(0) as usize).div_ceil(std::mem::size_of::<i32>());
        Self {
            base: vec![0i32; words],
            pStrideDecBlockOffset: [[None; 2]; MAX_DEPENDENCY_LAYER],
            pStrideEncBlockOffset: [None; MAX_DEPENDENCY_LAYER],
            pMbIndexX: [None; MAX_DEPENDENCY_LAYER],
            pMbIndexY: [None; MAX_DEPENDENCY_LAYER],
        }
    }

    /// The arena's **root address**, in bytes — S40's spelling, and the property the
    /// four accessors below inherit.
    ///
    /// `Vec::as_mut_ptr` reads the pointer out of the `Vec`'s own header; it does not
    /// form a `&mut [i32]` over the block. So two calls are sibling derivations that
    /// coexist, and a caller may hold a cursor from the first across the second —
    /// which is what `AllocStrideTables` does throughout (it carves four running
    /// cursors out of the block and advances them in interleaved loops) and what
    /// `svc_encode_mb.rs` does per macroblock. See `PaddedPlane::root_ptr` for the
    /// same statement on the picture planes, and F63 for what the other spelling did.
    ///
    /// **`pub` since T9.C4**: the four accessors below became `&self`/`*const`
    /// so the fork's workers stop retagging the whole
    /// `Option<Box<SStrideTables>>` to read a lookup table, and the writable
    /// derivation has to live somewhere. It lives at the four `AllocStrideTables`
    /// sites that fill the block, on the calling thread, before anything spawns —
    /// spelled at the site (session D's rule for accessors with one caller each)
    /// rather than as four `_mut` twins nobody else may use.
    #[inline]
    pub fn root(&mut self) -> *mut u8 {
        self.base.as_mut_ptr().cast::<u8>()
    }

    /// **`&self`, and T9.C4 is why.** These tables are filled once by
    /// `WelsGetEncBlockStrideOffset` at `InitDqLayers` and read-only for the
    /// rest of the encode — but the accessors took `&mut self` all the way down
    /// to `Vec::as_mut_ptr`, so every worker of the fork retagged the whole
    /// `Option<Box<SStrideTables>>` field to read a lookup table. Miri reported
    /// exactly that, as a data race on
    /// `Option<Box<SStrideTables>>`, once the reconstruction picture stopped
    /// being the first thing it tripped over. Shared reads do not race with
    /// each other, so the read path is `&self` and `*const`, and the one writer
    /// keeps its own `_mut` twin below.
    #[inline]
    // unsafe-cat: fork-shared(S63)
    #[allow(unsafe_code)]
    fn at_i32(&self, kiByteOffset: Option<u32>) -> *const i32 {
        match kiByteOffset {
            // SAFETY: every offset stored here was produced by `AllocStrideTables`
            // carving the very block `base` is, and is a multiple of 4.
            Some(off) => unsafe { self.base.as_ptr().cast::<u8>().add(off as usize).cast::<i32>() },
            None => std::ptr::null(),
        }
    }

    #[inline]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn at_i16(&self, kiByteOffset: Option<u32>) -> *const i16 {
        match kiByteOffset {
            // SAFETY: as `at_i32`; the two coordinate regions are even-aligned.
            Some(off) => unsafe { self.base.as_ptr().cast::<u8>().add(off as usize).cast::<i16>() },
            None => std::ptr::null(),
        }
    }

    /// `pStrideDecBlockOffset[kiDid][kiTid0]` as the cursor it used to be. `kiTid0`
    /// is the C++'s `kbBaseTemporalFlag` — 1 for the base temporal layer.
    #[inline]
    pub fn StrideDecBlockOffset(&self, kiDid: usize, kiTid0: usize) -> *const i32 {
        let off = self.pStrideDecBlockOffset[kiDid][kiTid0];
        self.at_i32(off)
    }

    /// `pStrideEncBlockOffset[kiDid]` as the cursor it used to be.
    #[inline]
    pub fn StrideEncBlockOffset(&self, kiDid: usize) -> *const i32 {
        let off = self.pStrideEncBlockOffset[kiDid];
        self.at_i32(off)
    }

    /// `pMbIndexX[kiDid]` as the cursor it used to be.
    #[inline]
    pub fn MbIndexX(&self, kiDid: usize) -> *const i16 {
        let off = self.pMbIndexX[kiDid];
        self.at_i16(off)
    }

    /// `pMbIndexY[kiDid]` as the cursor it used to be.
    #[inline]
    pub fn MbIndexY(&self, kiDid: usize) -> *const i16 {
        let off = self.pMbIndexY[kiDid];
        self.at_i16(off)
    }
}

/// [`SStrideTables::StrideDecBlockOffset`] reached through the context — the
/// spelling every consumer used when `pStrideTab` was a raw pointer.
///
/// **`&sWelsEncCtx`, and session H2 is why — the in-fork read surface, first
/// family.** These four are pure lookups into tables `WelsGetEncBlockStrideOffset`
/// fills once at `InitDqLayers` and nothing writes again; every worker of the fork
/// reads them and none writes them. Taking the context by `*mut` or `&mut` to
/// perform a read is the shape F132 round 2 already removed one level down, inside
/// [`SStrideTables`] itself ("literally two characters twice, `&mut` -> `&`"), and
/// leaving it at *this* level meant the fork's lawful read still had to be spelled
/// through a raw. A shared borrow is what the operation actually is, so:
///
/// * the two `*mut` ones lose `unsafe fn` and their `cursor` tags — with
///   `&sWelsEncCtx` the body is a field read, and there is no `unsafe` left in it;
/// * the two that already took `&mut` lose it (S63: nothing in-fork takes `&mut`),
///   and any number of workers may now hold the borrow at once, which is the
///   property that makes the read lawful rather than merely sound;
/// * **no new seam item.** Shared reads do not race with shared reads, so this
///   needs no `UnsafeCell` crossing and no `Sync` impl — the count of seam items
///   stays at D-mt-3's two.
///
/// The **return stays `*const`**, and the reason is the arena, not reluctance:
/// `SStrideTables` stores byte *offsets* into one flat `Vec<i32>` and records no
/// region lengths, so a slice API cannot be formed here without inventing a bound
/// the C++ never had. That is J's, named in the log.
///
/// The cursor these answer points into the arena, which is a different allocation
/// from the context and is never retagged by any of them — which is why repeated
/// calls are safe to interleave with held cursors, and why the shared borrow of the
/// context above says nothing about writes to the arena below.
#[inline]
pub fn ctx_stride_dec_block_offset(
    pCtx: &sWelsEncCtx,
    kiDid: usize,
    kiTid0: usize,
) -> *const i32 {
    match pCtx.pStrideTab.as_ref() {
        Some(tab) => tab.StrideDecBlockOffset(kiDid, kiTid0),
        None => std::ptr::null(),
    }
}

/// [`SStrideTables::StrideEncBlockOffset`] reached through the context. See
/// [`ctx_stride_dec_block_offset`] for why this family takes `&sWelsEncCtx`.
#[inline]
pub fn ctx_stride_enc_block_offset(pCtx: &sWelsEncCtx, kiDid: usize) -> *const i32 {
    match pCtx.pStrideTab.as_ref() {
        Some(tab) => tab.StrideEncBlockOffset(kiDid),
        None => std::ptr::null(),
    }
}

/// [`SStrideTables::MbIndexX`] reached through the context. See
/// [`ctx_stride_dec_block_offset`].
#[inline]
pub fn ctx_mb_index_x(pCtx: &sWelsEncCtx, kiDid: usize) -> *const i16 {
    match pCtx.pStrideTab.as_ref() {
        Some(tab) => tab.MbIndexX(kiDid),
        None => std::ptr::null(),
    }
}


/// The dispatch table **as a raw pointer, read out of the `Box`'s slot** — the
/// one derivation A6 could not flip, at its **two** callers.
///
/// **Why it survives.** Both callers write through the table
/// (`ParasetStrategy` reaches `pParametersetStrategy` `&mut`;
/// `CWelsH264SVCEncoder::SetOption`'s rate-control arm re-points `pfRc`), and
/// both hold the context as a **raw**. [`func_list_mut`](sWelsEncCtx::func_list_mut)
/// needs `&mut self`, so calling it from either body means a whole-context `&mut`
/// retag taken through a raw root — the shape S63 forbids and both of the
/// session's prohibition checks count. Neither body is fork-reachable, so the
/// retag would in fact be harmless; the rule is deliberately not
/// case-by-case, because F208 is what happens when a whole-context retag is
/// argued site by site. Reading the slot as a pointer *value* (F71) forms no
/// reference to the context at all, which is what the old accessor did at all
/// 121 sites and what these two keep.
///
/// Everything else uses [`sWelsEncCtx::func_list`] /
/// [`sWelsEncCtx::func_list_mut`]. `ctx_ref_list_raw` below survives A3 for the
/// neighbouring reason (provenance rather than root shape) — F211's pair is a
/// trio now.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_func_list_raw(pCtx: &sWelsEncCtx) -> *mut SWelsFuncPtrList {
    std::ptr::read(std::ptr::addr_of!((*pCtx).pFuncList) as *const *mut SWelsFuncPtrList)
}

/// The encoder output block **as a raw pointer, read out of the `Box`'s slot** —
/// F71's spelling, minted for the two fork-reachable bodies whose `pOut` arm is
/// main-thread-only by measurement (F217): `slice_bs_buffer` and `slice_writer`.
/// Their context parameter is a raw, so a `&mut`-shaped route would be a
/// whole-context retag through a raw root (prohibition 2); the slot read carries
/// the block's own provenance instead.
///
/// Null exactly where the field is `None`: before init, after teardown.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_out_raw(pCtx: *const sWelsEncCtx) -> *mut SWelsEncoderOutput {
    std::ptr::read(std::ptr::addr_of!((*pCtx).pOut) as *const *mut SWelsEncoderOutput)
}

/// The preprocessor's **spatial picture pool** as a raw pointer, read out of
/// `pVpp`'s slot — F71's spelling, and F211's *provenance* category rather than
/// debt: the answer is **stored** in `SDqLayer::pSrcPool` and read by the fork for
/// a whole frame, so it must carry the pool's own provenance. A `&mut`-derived
/// cast would stamp a fresh `Unique` that the next `pVpp` reborrow pops, leaving
/// the fork reading through a dead tag — and F208 is the proof that no byte gate
/// sees that.
///
/// # Safety
/// `pCtx` must point to a live encoder context whose `pVpp` is `Some`.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_src_pool_raw(pCtx: *const sWelsEncCtx) -> *mut SrcPicPool {
    let pVpp = std::ptr::read(
        std::ptr::addr_of!((*pCtx).pVpp)
            as *const *mut crate::encoder::wels_preprocess::CWelsPreProcess,
    );
    std::ptr::addr_of_mut!((*pVpp).m_pSpatialPicPool)
}

/// The preprocess object **as a reference off a slot read** — the route every
/// body uses that needs the vpp and the context at once.
///
/// **Why not `pVpp.as_deref_mut()`, and Miri is what said so.** `SDqLayer::pSrcPool`
/// stores a raw into `m_pSpatialPicPool`, a *field of this allocation*, and the fork
/// reads it for a whole frame. Reaching the object through the `Box` mints a fresh
/// `Unique` over the whole block on every `as_deref`/store (F215's rule, one
/// allocation further in), so the two routes pop each other: a shared retag through
/// `pSrcPool` kills the `Box` tag, and re-storing the `Box` kills `pSrcPool`. The
/// S3.B1 first draft used an `Option::take` dance here and Miri refused it at
/// `WelsSliceHeaderExtInit` — the byte gates were 583/583 in both profiles through it.
///
/// Reading the slot as a *value* mints nothing: every derivation is a sibling off
/// the allocation's own tag, which is exactly what the `*mut CWelsPreProcess` field
/// gave these callers before B1 owned it. The ownership moved; the aliasing did not.
///
/// # Safety
/// `pCtx` must point to a live encoder context whose `pVpp` is `Some`.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_vpp_raw<'a>(
    pCtx: *const sWelsEncCtx,
) -> &'a mut crate::encoder::wels_preprocess::CWelsPreProcess {
    let pVpp = std::ptr::read(
        std::ptr::addr_of!((*pCtx).pVpp)
            as *const *mut crate::encoder::wels_preprocess::CWelsPreProcess,
    );
    &mut *pVpp
}

/// The preprocess object as a **shared** reference off the same slot read — the
/// only route an **in-fork** body may take, and the reader half of the pair.
///
/// **This split is not stylistic; the MT probes are what forced it.** S3.B1's
/// first draft let `ctx_pic_ref` and `WelsSliceHeaderExtInit` — both fork-reachable
/// (F217 names them) — reach the object through [`ctx_vpp_raw`]. A `&mut` retag is
/// a *write* as far as the data-race model is concerned, so N workers each taking
/// one is S63's violation with no read of the object needed to make it real:
///
/// ```text
/// Data race detected between (1) retag write on thread `unnamed-2`
///   and (2) retag write of type CWelsPreProcess on thread `unnamed-3`
/// ```
///
/// The encode shards cannot see this — they are single-threaded — and both byte
/// sweeps were 583/583 through it. `fork_join_encodes_a_frame_whose_slice_boundary_is_mid_row`
/// is what refused it, which is plan §4.7's whole argument in one failure.
///
/// Shared retags coexist with each other and with the `pSrcPool` reads (F208's
/// direction is `&mut`-shaped derivations, and there are none here), so this is the
/// route for every fork-reachable read.
///
/// # Safety
/// `pCtx` must point to a live encoder context whose `pVpp` is `Some`.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_vpp_ref<'a>(
    pCtx: *const sWelsEncCtx,
) -> &'a crate::encoder::wels_preprocess::CWelsPreProcess {
    let pVpp = std::ptr::read(
        std::ptr::addr_of!((*pCtx).pVpp)
            as *const *const crate::encoder::wels_preprocess::CWelsPreProcess,
    );
    &*pVpp
}

/// The coding parameters **as a raw pointer, read out of the `Box`'s slot** — the
/// root the twenty-six per-layer *cursors* are taken from.
///
/// **Why the cursors could not come off `param_mut`, and Miri is what said so.**
/// `sSpatialLayers[d]` / `sDependencyLayers[d]` are held as raw cursors across
/// calls that reach the parameters again — `InitDqLayers` derives two and then
/// calls `WelsGenerateNewSps`, which re-derives the same layer; `RequestMemorySvc`
/// holds one across `AcquireLayersNals`. Under `ctx_param` that worked, because
/// the accessor read the slot as a *value* and every `addr_of_mut!` off it
/// inherited the block's own tag. [`param_mut`](sWelsEncCtx::param_mut) is a real
/// `&mut`: **each call is a fresh `Unique` retag over the whole 0x4d0-byte
/// block**, so the second call pops the first call's cursors, and the read that
/// follows is through a dead tag. A7's first Miri run refused exactly that, at
/// `encoder_ext.rs:1269` and again at `:808`; no byte gate saw either.
///
/// So the cursors keep F71's spelling and this is where it lives. Everything that
/// merely reads or writes a field goes through [`param`](sWelsEncCtx::param) /
/// [`param_mut`](sWelsEncCtx::param_mut) — 230 of A7's 258 sites do.
///
/// **Single-threaded only.** Every caller is init, per-frame bookkeeping or the
/// C-API surface; the fork reads parameters through `param` and writes none.
///
/// # Safety
/// `pCtx`'s parameter block must be built (`WelsInitEncoderExt`); the return is
/// null before that, exactly as the raw field was.
#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ctx_param_raw(pCtx: &sWelsEncCtx) -> *mut SWelsSvcCodingParam {
    std::ptr::read(std::ptr::addr_of!(pCtx.pSvcParam) as *const *mut SWelsSvcCodingParam)
}

/// Dependency layer `kiDid`'s reference list **as a raw pointer, read out of the
/// slot** — the one derivation A3 left raw, and the only caller is
/// `WelsInitCurrentLayer`'s stamp of `SDqLayer::pRefList`.
///
/// **Why it survives the conversion.** `SDqLayer::pRefList` is a raw *field*
/// (stage C's, plan §3c-5), and the value stored in it is read for the whole
/// frame by the fork — `layer_ref_pic`, and `deblocking.rs`'s two null guards. It
/// must therefore carry the reference list's **own** provenance, which is what
/// reading the slot as a pointer value gives it (F71). A `&mut`-derived cast
/// would stamp a fresh `Unique` that the next
/// [`ref_list_mut`](sWelsEncCtx::ref_list_mut) call pops, leaving the fork
/// reading through a dead tag — a soundness regression the byte gates would not
/// see. So the accessor's old body lives on here, at one site, until the field it
/// feeds is gone. Everything else uses [`sWelsEncCtx::ref_list`] /
/// [`sWelsEncCtx::ref_list_mut`].
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_ref_list_raw(pCtx: &sWelsEncCtx, kiDid: usize) -> *mut SRefList {
    let arr = std::ptr::addr_of!(pCtx.ppRefPicListExt);
    if kiDid >= (*arr).len() {
        return std::ptr::null_mut();
    }
    std::ptr::read((*arr).as_ptr().add(kiDid) as *const *mut SRefList)
}

/// Dependency layer `kiDid`'s **DQ layer** — `ppDqLayerList[did]`, and null where the
/// slot's pointer was null. T6.H8; the same shape as [`sWelsEncCtx::ref_list`], and
/// [`current_layer`](crate::encoder::svc_encode_slice::current_layer) resolves
/// `iCurDqLayer` through it.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_dq_layer(pCtx: &sWelsEncCtx, kiDid: usize) -> *mut SDqLayer {
    // **F71.** No `&mut` to the `Vec` and no reference to the slot. See the family
    // note above `ctx_func_list_raw`.
    let arr = std::ptr::addr_of!((*pCtx).ppDqLayerList);
    if kiDid >= (*arr).len() {
        return std::ptr::null_mut();
    }
    // `Option<Box<T>>` is guaranteed to be one pointer wide with `None` as null, so
    // the slot is *read as a pointer value* rather than borrowed as an `Option`.
    // The value read carries the heap block's own provenance — nothing here retags
    // the layer, which is what lets two workers resolve it at once.
    std::ptr::read((*arr).as_ptr().add(kiDid) as *const *mut SDqLayer)
}

/// The long-term-reference state of dependency layer `kiDid` — `pLtr[did]`, which is
/// how all consumers spell it.
///
/// **T9.H3 (F196/F197) — the real reference, and a safe `fn`.** Until this session
/// the raw return was load-bearing at seventy-five sites that used the LTR state
/// and the context in the same breath; every one of those coexistences is
/// borrow-lawful now (raw roots and scalars derived first, re-borrows after the
/// calls that re-derive the same state, two callees narrowed), so the borrow
/// checker referees every caller. The root accessor `ctx_ltr` went with the raw:
/// its last caller reads `pLtr.first()` directly.
///
/// # Panics
/// If `kiDid` is not a layer the array holds — the old `debug_assert` made
/// unconditional. The old empty-array `null` return was never survivable: every
/// caller dereferenced the answer.
#[inline]
pub fn ctx_ltr_at(pCtx: &mut sWelsEncCtx, kiDid: usize) -> &mut SLTRState {
    &mut pCtx.pLtr[kiDid]
}

/// `pCtx->pDqIdcMap`, as the slice it is — T6.H3, **converted at T9.H2 (step 4)**.
///
/// This answered the root as `*mut SDqIdc` and both production callers immediately
/// wrote `.add(did)`; S54's rule says an accessor every caller offsets and
/// dereferences is an accessor returning the element. It returns the slice, the two
/// callers index it, and **neither holds anything any more** — which is the part
/// worth stating, because the held cursor was the whole reason this accessor needed
/// F71's `addr_of!` spelling and a row in the sibling-derivation test. `InitParaSet`
/// used to carry `pDqIdc` across `GenerateNewSps` and `InitPps` and write through it
/// afterwards; it now writes in two tight scopes and the binding is gone. The
/// sibling property is not asserted for this accessor any longer because a
/// reference API cannot have it.
///
/// Empty answers an empty slice where this answered null — the callers' `is_null()`
/// guards were never here (both index unconditionally, as the C++ does), and an
/// out-of-range layer id is now a bounded panic instead of a walk off a `Vec`
/// buffer.
///
/// The other four of the brief's five stay raw for reasons measured at their
/// callers — see F193, and the notes on [`sWelsEncCtx::frame_bs`] /
/// [`sWelsEncCtx::frame_bs_cur`].
#[inline]
pub fn ctx_dq_idc_map(pCtx: &mut sWelsEncCtx) -> &mut [SDqIdc] {
    &mut pCtx.pDqIdcMap
}

/// The three parameter-set arrays **at once, as disjoint borrows** — T9.H2, and the
/// answer to the shape that blocked X2's ~36.
///
/// `LoadPrevious` (`paraset_strategy.rs`) writes all three in one call, and its call
/// site supplied them as three separate `ctx_*_array(*ppCtx)` raws. Converting those
/// to slices one accessor at a time is impossible — three `&mut` out of one context
/// through three separate calls is what the borrow checker exists to refuse, and it
/// is what the charter recorded as "`LoadPrevious`'s four-simultaneous-`&mut`
/// projections shape, S63's forbidden retag exactly".
///
/// **The shape was never the problem; the call count was.** Rust permits
/// `(&mut s.a, &mut s.b, &mut s.c)` from one `&mut s` inside a single body — the
/// three fields are disjoint and the compiler can see it. What it cannot see is
/// three accessor calls each claiming the whole context. So this is one call, and
/// the block dissolves.
///
/// The three are `Vec`s on the context, so the returned slices borrow the *context*
/// (not the buffers' provenance, which is what the raw spellings deliberately
/// answered). That is the right trade here: the caller wants to write them, the
/// borrow is short, and no cursor outlives it.
#[inline]
pub fn ctx_paraset_arrays(
    pCtx: &mut sWelsEncCtx,
) -> (&mut [SWelsSPS], &mut [SSubsetSps], &mut [SWelsPPS]) {
    (&mut pCtx.pSpsArray, &mut pCtx.pSubsetArray, &mut pCtx.pPPSArray)
}

/// [`SStrideTables::MbIndexY`] reached through the context. See
/// [`ctx_stride_dec_block_offset`].
#[inline]
pub fn ctx_mb_index_y(pCtx: &sWelsEncCtx, kiDid: usize) -> *const i16 {
    match pCtx.pStrideTab.as_ref() {
        Some(tab) => tab.MbIndexY(kiDid),
        None => std::ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// **The safe accessor layer** — stage A of the safe-conversion plan.
//
// Everything the god-struct used to hand out through a raw accessor is handed
// out here instead, by a **safe method**, and on one design:
//
// * **Readers take `&self`, and they are what everyone calls — the fork
//   included.** An in-fork site spells the call `(*pCtx).name()` inside the
//   `unsafe` block it already has: a per-call *shared* reborrow, used in the
//   expression and never stored (S37). N workers may hold shared reborrows of
//   one context at once, which is precisely why this is the only path the fork
//   is allowed. S63 forbids the `&mut` one at any duration — a reference
//   argument is strongly protected for the whole call (F192), so "briefly" buys
//   nothing. The fields workers actually write are already atomics or `Cell`s
//   behind the audited seam (D-mt-3's two `unsafe impl`s), and a shared
//   reference reaches those.
// * **Writers take `&mut self`, and they are single-threaded only.** The rule is
//   grep-checkable and is checked at every checkpoint: a `*_mut` accessor may
//   not appear in a body whose context parameter is `*mut sWelsEncCtx`.
//   `rust/tools/phase9_forksplit.py --list` classifies those bodies; every body
//   that already takes `&mut sWelsEncCtx` was adjudicated single-threaded by the
//   Phase 9 flip, which is what made that flip legal in the first place.
//
// Where a return **stays raw** it is because the value crosses a boundary that
// cannot carry a lifetime — F193's `SLayerBSInfo::pBsBuf`, or a raw struct field
// a later stage converts. Those accessors are still *safe fns*: forming a raw
// pointer needs no `unsafe`, only dereferencing one does, and the dereference
// belongs to whoever owns the far end. That is the whole of what "the accessor
// became safe" claims for them, and it is worth being exact about, because it is
// the difference between this layer's `unsafe` and the next layer's.
// ---------------------------------------------------------------------------
/// The five disjoint borrows [`sWelsEncCtx::ltr_family_mut`] hands out.
///
/// One owner per field, so the compiler grants all five at once — which is the
/// whole point: the raw roots this replaces existed only because two whole-context
/// accessor calls could not coexist.
pub struct LtrFamilyMut<'a> {
    /// One dependency layer's parameter slot — `sDependencyLayers[kiDid]`.
    pub param_layer: &'a mut crate::encoder::param_svc::SSpatialLayerInternal,
    /// The video-analysis block, absent before the preprocess builds it.
    pub vaa: Option<&'a mut SVAAFrameInfo>,
    /// The dependency layer's reference list, absent before it is allocated.
    pub ref_list: Option<&'a mut SRefList>,
    /// The dependency layer's long-term-reference state.
    pub ltr: &'a mut SLTRState,
    /// `bRefOfCurTidIsLtr`, indexed `[did][tid]`.
    pub ref_of_cur_tid_is_ltr: &'a mut [[bool; MAX_TEMPORAL_LEVEL]; MAX_DEPENDENCY_LAYER],
}

impl sWelsEncCtx {
    /// The **MVD cost table** — T6.H9, `pCtx->pMvdCostTable`.
    ///
    /// Empty before `WelsInitEncoderExt` sizes it, which is the state the raw
    /// accessor answered with a null and its callers asked about with
    /// `is_null()`; the question is `is_empty()` now and it is the same question.
    #[inline]
    pub fn mvd_cost_table(&self) -> &[u16] {
        &self.pMvdCostTable
    }

    /// [`mvd_cost_table`](Self::mvd_cost_table) for its **one** writer:
    /// `MvdCostInit` fills the whole table once, inside `WelsInitEncoderExt`.
    /// Single-threaded by construction — the fork only reads this table.
    #[inline]
    pub fn mvd_cost_table_mut(&mut self) -> &mut [u16] {
        &mut self.pMvdCostTable
    }

    // `mvd_cost_origin(&self) -> *mut u16` stood here. **S5.C4b** retired it: its
    // two callers now take `svc_encode_slice::ctx_mvd_cost_origin`, which answers
    // with a `MvdCostCursor` instead of a raw pointer and reaches the table through
    // a *field* projection rather than a whole-context `&self` — read that function's
    // header for why the difference matters under the fork. The `debug_assert` this
    // carried moved with the body; nothing else here had a second reader.

    /// The **rate controller's per-layer array** — T6.H6, `pCtx->pWelsSvcRc`.
    ///
    /// The raw root answered null on an empty `Vec` and its two callers asked
    /// `is_null()` before indexing; both ask `is_empty()` now. See
    /// [`rc_at`](Self::rc_at) for the per-layer entry, which is how the other
    /// sixty consumers spell it.
    #[inline]
    pub fn rc(&self) -> &[SWelsSvcRc] {
        &self.pWelsSvcRc
    }

    /// Dependency layer `kiDid`'s **reference list** — `ppRefPicListExt[did]`,
    /// T6.H7.
    ///
    /// **`Option`, because the absence is a state and sixteen callers already
    /// asked about it.** The raw accessor answered null both past the configured
    /// layers and before `InitDqLayers` fills the slot, and more than half its
    /// production sites opened with `if pRefList.is_null()`. Those guards become
    /// `let Some(..) else`, which is the same branch with the question asked in
    /// the type; the sites that never asked say `.expect` and name the assumption
    /// they were making silently.
    ///
    /// In-fork this is the only path — `ctx_ref_pic`/`ctx_pic_ref` resolve a
    /// picture through it and read, which is what a shared reborrow is for. The
    /// writers are all reference-list management, and all single-threaded.
    #[inline]
    pub fn ref_list(&self, kiDid: usize) -> Option<&SRefList> {
        self.ppRefPicListExt.get(kiDid)?.as_deref()
    }

    /// [`ref_list`](Self::ref_list) for the reference-list managers.
    ///
    /// **Single-threaded only** — see [`rc_at_mut`](Self::rc_at_mut).
    #[inline]
    pub fn ref_list_mut(&mut self, kiDid: usize) -> Option<&mut SRefList> {
        self.ppRefPicListExt.get_mut(kiDid)?.as_deref_mut()
    }

    /// The **parameter-set arrays** — `pSpsArray`, `pSubsetArray`, `pPPSArray`
    /// (T6.H2), as the slices they have been since `RequestMemorySvc` stopped
    /// calling `WelsMallocz`.
    ///
    /// The raw roots answered null on an empty `Vec` and every consumer offset
    /// them with `.add(id)`; the slices are indexed, and the two `is_null()`
    /// guards ask `is_empty()`. **Empty is a real state** for `pSubsetArray` —
    /// the configuration may need no subset SPS — so the question those guards
    /// asked is the question they still ask.
    ///
    /// **Readers for the fork, writers for init.** The arrays are filled by
    /// `RequestMemorySvc` and by the parameter-set strategy, both single-threaded;
    /// nothing in the fork writes them, and a whole-tree grep for a write through
    /// `layer_sps` / `layer_pps` / `layer_subset_sps`'s answers returns nothing.
    /// The three `layer_*` accessors keep raw returns because their far end is
    /// `SDqLayer::sLayerInfo`, stage C's; they derive them from these readers.
    ///
    /// [`paraset_arrays`](Self::paraset_arrays) answers all three at once, which
    /// is what `LoadPrevious` needs (T9.H2).
    #[inline]
    pub fn sps_array(&self) -> &[SWelsSPS] {
        &self.pSpsArray
    }

    /// [`sps_array`](Self::sps_array), mutably. Single-threaded only.
    #[inline]
    pub fn sps_array_mut(&mut self) -> &mut [SWelsSPS] {
        &mut self.pSpsArray
    }

    /// The **subset SPS array** — see [`sps_array`](Self::sps_array).
    #[inline]
    pub fn subset_array(&self) -> &[SSubsetSps] {
        &self.pSubsetArray
    }

    /// [`subset_array`](Self::subset_array), mutably. Single-threaded only.
    #[inline]
    pub fn subset_array_mut(&mut self) -> &mut [SSubsetSps] {
        &mut self.pSubsetArray
    }

    /// The **PPS array** — see [`sps_array`](Self::sps_array).
    #[inline]
    pub fn pps_array(&self) -> &[SWelsPPS] {
        &self.pPPSArray
    }

    /// [`pps_array`](Self::pps_array), mutably. Single-threaded only.
    #[inline]
    pub fn pps_array_mut(&mut self) -> &mut [SWelsPPS] {
        &mut self.pPPSArray
    }

    /// A dependency layer's **reference list and its long-term-reference state,
    /// from one borrow** — §4.6's combined accessor, and the shape
    /// `ref_list_mgr_svc.rs` wants at nine of its bodies.
    ///
    /// The two live in different fields of the context, so the compiler can see
    /// they are disjoint once they are projected inside a single body — which is
    /// exactly the block `ctx_paraset_arrays` dissolved for `LoadPrevious`
    /// (T9.H2): "the shape was never the problem; the call count was". Two
    /// accessor calls each claim the whole context; one call projects both.
    ///
    /// The file's own T9.H3 note asks for the same order this enforces — every
    /// raw root derived first, the reference-shaped borrows last and together.
    #[inline]
    pub fn ref_list_and_ltr_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SRefList>, &mut SLTRState) {
        let sWelsEncCtx { ppRefPicListExt, pLtr, .. } = self;
        (
            ppRefPicListExt.get_mut(kiDid).and_then(|s| s.as_deref_mut()),
            &mut pLtr[kiDid],
        )
    }

    /// The **preprocess object and a dependency layer's reference list, from one
    /// borrow** — §4.6's combined accessor, and what retires `ctx_vpp_raw` from
    /// `ref_list_mgr_svc.rs` (S10.5a).
    ///
    /// Three bodies there hand the preprocess a `&SRefList` while holding it
    /// `&mut`: `UpdateOriginalPicInfoFromCtx`, `UpdateSrcPicList` and
    /// `UpdateSrcPicListLosslessScreenRefSelectionWithLtr`. Their comments describe
    /// the workaround they used — "the vpp is *taken* (S3.B1) so the shared borrow
    /// of the list and the `&mut` receiver are borrows of two different owners" —
    /// which is a `ctx_vpp_raw` slot read standing in for a disjointness the
    /// compiler could have granted. `pVpp` and `ppRefPicListExt` **are** two
    /// different fields; projecting them together says so directly.
    ///
    /// This is the safe half of the pair `ctx_vpp_raw` / `ctx_vpp` documents. It
    /// is **single-threaded only**, exactly as `ctx_vpp_raw` is: an in-fork body
    /// must still take the shared `ctx_vpp` route, because a `&mut` retag of the
    /// preprocess object from N workers is S63's violation with no read needed to
    /// make it real.
    #[inline]
    pub fn vpp_and_ref_list_mut(
        &mut self,
        kiDid: usize,
    ) -> (
        Option<&mut crate::encoder::wels_preprocess::CWelsPreProcess>,
        Option<&SRefList>,
    ) {
        let sWelsEncCtx { pVpp, ppRefPicListExt, .. } = self;
        (
            pVpp.as_deref_mut(),
            ppRefPicListExt.get(kiDid).and_then(|s| s.as_deref()),
        )
    }

    /// The **video-analysis block, one layer's rate-control state, and that layer's
    /// reference list, from one borrow** — §4.6's combined accessor, widened by one
    /// field for S10.9.
    ///
    /// `AnalyzePictureComplexity` hands `CComplexityAnalysis::Process` three things
    /// that live in three different fields of the context: the VAA block's own
    /// `sVaaCalcInfo` and `pVaaBackgroundMbFlag`, the rate controller's two GOM
    /// arrays, and — since the `uiRefMbType` raw left
    /// `SComplexityAnalysisParam` — the *reference picture's* per-macroblock type
    /// array. Three owners, one call.
    #[inline]
    pub fn vaa_rc_and_ref_list_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SVAAFrameInfo>, &mut SWelsSvcRc, Option<&SRefList>) {
        let sWelsEncCtx { pVaa, pWelsSvcRc, ppRefPicListExt, .. } = self;
        (
            pVaa.as_deref_mut(),
            &mut pWelsSvcRc[kiDid],
            ppRefPicListExt.get(kiDid).and_then(|s| s.as_deref()),
        )
    }

    /// Every field the three LTR bodies touch, **from one borrow** — §4.6's
    /// combined accessor taken to its natural end, and what retires
    /// `ctx_param_raw` and two `addr_of_mut!` roots from this family (S10.5a).
    ///
    /// **The shape this dissolves is F239's.** `DeleteInvalidLTR`,
    /// `HandleLTRMarkFeedback` and `LTRMarkProcess` each opened with
    ///
    /// ```text
    /// let pParamInternal = addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[uiDid]);
    /// let bRefOfCurTidIsLtr = addr_of_mut!(pCtx.bRefOfCurTidIsLtr);
    /// let (pVaa, pRefList, pLtr) = pCtx.vaa_ref_list_and_ltr_mut(uiDid);
    /// ```
    ///
    /// — field-precise raw cursors **held across** a later whole-context reborrow,
    /// which is the derivation F239 records as popped and sweep-invisible. Those
    /// bodies carry a T9.H3 comment explaining the ordering they adopted to
    /// survive it ("every raw root first, the reference-shaped borrows last"): a
    /// hand-maintained rule that exists only because two accessors each claim the
    /// whole context. One accessor projecting all five fields needs no ordering
    /// rule, and the compiler — not a comment — is what keeps it true.
    ///
    /// A named struct rather than a five-tuple because the three callers want
    /// different subsets, and `_` on a tuple position says nothing about which
    /// field was skipped.
    ///
    /// `param_layer` is one dependency layer's slot, not the whole parameter
    /// block: the bodies write `bEncCurFrmAsIdrFlag` and read `iFrameNum`, and
    /// narrowing here is the safe spelling of the `addr_of_mut!` it replaces.
    #[inline]
    pub fn ltr_family_mut(&mut self, kiDid: usize) -> LtrFamilyMut<'_> {
        let sWelsEncCtx {
            pSvcParam, pVaa, ppRefPicListExt, pLtr, bRefOfCurTidIsLtr, ..
        } = self;
        LtrFamilyMut {
            param_layer: &mut pSvcParam
                .as_deref_mut()
                .expect("the coding parameters are built by WelsInitEncoderExt")
                .sDependencyLayers[kiDid],
            vaa: pVaa.as_deref_mut(),
            ref_list: ppRefPicListExt.get_mut(kiDid).and_then(|s| s.as_deref_mut()),
            ltr: &mut pLtr[kiDid],
            ref_of_cur_tid_is_ltr: bRefOfCurTidIsLtr,
        }
    }

    /// [`ref_list_and_ltr_mut`](Self::ref_list_and_ltr_mut) **plus the
    /// video-analysis block** — the same combined-accessor move, one field wider.
    ///
    /// Two LTR bodies (`HandleLTRMarkFeedback`, `LTRMarkProcess`) stamp
    /// `SVAAFrameInfo::uiValidLongTermPicIdx` / `uiMarkLongTermPicIdx` from
    /// inside the loop that walks the reference list, so the VAA write and the
    /// list borrow are genuinely wanted at once. Under the raw accessor the two
    /// never met the borrow checker: `ctx_vaa` read the `Box`'s slot as a
    /// *value*, so the pointer it handed out survived every later retag of the
    /// context (F71). [`vaa_mut`](Self::vaa_mut) is a real `&mut`, so it does
    /// not — which is the conversion working, not a regression, and this is
    /// §4.6's second remedy for it.
    #[inline]
    pub fn vaa_ref_list_and_ltr_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SVAAFrameInfo>, Option<&mut SRefList>, &mut SLTRState) {
        let sWelsEncCtx { pVaa, ppRefPicListExt, pLtr, .. } = self;
        (
            pVaa.as_deref_mut(),
            ppRefPicListExt.get_mut(kiDid).and_then(|s| s.as_deref_mut()),
            &mut pLtr[kiDid],
        )
    }

    /// The **coding parameters and the three parameter-set arrays, from one
    /// borrow** — §4.6's combined accessor for `paraset_strategy.rs`.
    ///
    /// `WelsGenerateNewSps` and `FindExistingSps` build an SPS from a layer's
    /// configuration *into* the SPS array, and `WelsInitSps` writes
    /// `uiLevelIdc` back into that configuration on the way — so the parameter
    /// block and the arrays are mutably live in the same statement. Four fields,
    /// one owner; `ctx_paraset_arrays` is the same move one field narrower, and
    /// T9.H2's ruling on it applies here verbatim: "the shape was never the
    /// problem; the call count was".
    #[inline]
    pub fn param_and_paraset_arrays_mut(
        &mut self,
    ) -> (&mut SWelsSvcCodingParam, &mut [SWelsSPS], &mut [SSubsetSps], &mut [SWelsPPS]) {
        let sWelsEncCtx { pSvcParam, pSpsArray, pSubsetArray, pPPSArray, .. } = self;
        (
            pSvcParam
                .as_deref_mut()
                .expect("the coding parameters are built by WelsInitEncoderExt"),
            pSpsArray,
            pSubsetArray,
            pPPSArray,
        )
    }

    /// The **coding parameters and one layer's rate-control state, from one
    /// borrow** — §4.6's combined accessor, and the one A7 could not do without.
    ///
    /// Nine bodies in `rc.rs` have the same shape: bind the layer's
    /// `sSpatialLayers[did]` / `sDependencyLayers[did]` config, then write the
    /// layer's rate-control state from it. Under the raw accessor the config
    /// borrow was of the *parameter block's* allocation and the rate controller's
    /// `&mut` was of the context, so the two never met; `param` borrows the
    /// context, so they do. Two fields, one owner, one call.
    #[inline]
    pub fn param_and_rc_at_mut(
        &mut self,
        kiDid: usize,
    ) -> (&SWelsSvcCodingParam, &mut SWelsSvcRc) {
        let sWelsEncCtx { pSvcParam, pWelsSvcRc, .. } = self;
        (
            pSvcParam
                .as_deref()
                .expect("the coding parameters are built by WelsInitEncoderExt"),
            &mut pWelsSvcRc[kiDid],
        )
    }

    /// The **video-analysis block and one layer's rate-control state, from one
    /// borrow** — §4.6's combined accessor for `AnalyzePictureComplexity`.
    ///
    /// The complexity plugin is handed `&pVaa->sVaaCalcInfo` and the rate
    /// controller's two GOM arrays `&mut` **in the same call**, and the block it
    /// reads back into is `pVaa->sComplexityAnalysisParam`. Three fields, two
    /// owners, one statement — the raw accessors never had to say so, and two
    /// separate `&mut` accessor calls each claim the whole context.
    #[inline]
    pub fn vaa_and_rc_at_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SVAAFrameInfo>, &mut SWelsSvcRc) {
        let sWelsEncCtx { pVaa, pWelsSvcRc, .. } = self;
        (pVaa.as_deref_mut(), &mut pWelsSvcRc[kiDid])
    }

    /// The rate-control state of spatial layer `kiDid` — `pWelsSvcRc[did]`, which
    /// is how all sixty consumers spell it. See [`rc`](Self::rc) for the array.
    ///
    /// **The reader/writer split is the whole of A2, and it fell out measured
    /// rather than argued.** Every body that *writes* rate-control state through
    /// this accessor takes the context by `&mut sWelsEncCtx` — thirty-one of
    /// them, all single-threaded — and every body the forksplit puts **in-fork**
    /// reads and writes nothing: `WelsRcMbInitGom`, `WelsRcMbInfoUpdateGom`,
    /// `RcCalculateMbQp`, `RcCalculateGomQp`, `RcGomTargetBits`,
    /// `RcJudgeBaseUsability` and the `Disable` pair keep their mutable state in
    /// `SSlice::sSlicingOverRc`, which is per-slice and not shared. That is
    /// F132's audited design showing up as a clean split, and it is why the fork
    /// needs no interior mutability here.
    ///
    /// # Panics
    /// If `kiDid` is not a layer the array holds. The raw accessor's empty-array
    /// `null` return was never survivable — every consumer dereferenced the
    /// answer, and the two that guarded first asked [`rc`](Self::rc) whether the
    /// array was empty, which they still do. Same ruling as `ctx_ltr_at`'s
    /// (T9.H3).
    #[inline]
    pub fn rc_at(&self, kiDid: usize) -> &SWelsSvcRc {
        &self.pWelsSvcRc[kiDid]
    }

    /// [`rc_at`](Self::rc_at) for the thirty-one single-threaded writers.
    ///
    /// **Single-threaded only** — the prohibition this session checks at every
    /// checkpoint is that no body taking `*mut sWelsEncCtx` calls this.
    #[inline]
    pub fn rc_at_mut(&mut self, kiDid: usize) -> &mut SWelsSvcRc {
        &mut self.pWelsSvcRc[kiDid]
    }

    /// The frame bitstream's **write cursor** — `pFrameBs + iPosBsBuffer`. See
    /// [`frame_bs`](Self::frame_bs), including why the return is **permanently
    /// raw** (F193: nine of its sites store the answer into
    /// `SLayerBSInfo::pBsBuf`, `codec_app_def.h:640`).
    ///
    /// **T9.G5 — this took the position as a parameter, and it was
    /// `ctx_frame_bs_at`.** It never varied: all 18 production callers passed
    /// `(*pCtx).iPosBsBuffer`. S54 — a two-argument accessor whose second argument
    /// is always a field of the first is a one-argument accessor written 18 times.
    /// The write position is not a parameter; it is the invariant, and it lives
    /// here where the bounds assert can name it.
    ///
    /// `wrapping_add` rather than `.add`: the same address, computed without an
    /// in-bounds claim, so the accessor is safe and the claim stays in the
    /// `debug_assert` that always made it.
    #[inline]
    pub fn frame_bs_cur(&self) -> *mut u8 {
        let root = self.frame_bs();
        if root.is_null() {
            return std::ptr::null_mut();
        }
        let kiPos = self.iPosBsBuffer;
        debug_assert!(
            kiPos >= 0 && (kiPos as usize) <= self.pFrameBs.len(),
            "frame bitstream cursor {kiPos} is outside the buffer"
        );
        root.wrapping_add(kiPos as usize)
    }

    /// The **frame bitstream buffer's root** — T6.H4.
    ///
    /// **A permanent raw return, and the reason is the C ABI — F193**, measured
    /// at the callers rather than assumed. Of the production call sites, three
    /// store the answer into `SLayerBSInfo::pBsBuf`, and that field is
    /// `codec_app_def.h:640` — `unsigned char* pBsBuf`, a public member of a
    /// struct this library hands to the application. The value crosses the
    /// boundary, so it cannot become a slice, a reference, or anything else
    /// carrying a lifetime, in this stage or any later one. A session that
    /// reaches for "make the accessors return slices" should stop at this note
    /// rather than pay S54's cost again to learn it. Same for
    /// [`frame_bs_cur`](Self::frame_bs_cur).
    ///
    /// What *did* convert is the `unsafe`: the derivation is `Vec::as_ptr`
    /// through a shared borrow — character for character what the raw accessor's
    /// `addr_of!` spelling reached — so this reads the `Vec` header and hands out
    /// the buffer's own provenance, and repeated calls are **siblings**, none
    /// popping another. That property is load-bearing (`SLayerBSInfo::pBsBuf`
    /// keeps a cursor live while the NAL writers derive more from the same
    /// buffer at `iPosBsBuffer`) and `frame_bs_cursors_are_siblings` is it as a
    /// Miri test. `&self` rather than `&mut self` for the same reason: the
    /// walking derivations coexist, and a shared borrow says so.
    ///
    /// Empty answers null, which is what the field held before `RequestMemorySvc`
    /// ran — `Vec::as_ptr` on an empty `Vec` answers a dangling *non-null*
    /// address, so this branch is load-bearing, not defensive.
    #[inline]
    pub fn frame_bs(&self) -> *mut u8 {
        if self.pFrameBs.is_empty() {
            return std::ptr::null_mut();
        }
        self.pFrameBs.as_ptr() as *mut u8
    }

    /// The encoder's **coding parameters** — T6.H1, `pCtx->pSvcParam`, and the
    /// most-reached field on the struct: 258 textual sites when A7 opened.
    ///
    /// **Three accessors, because the sites ask two different questions.** Some
    /// 220 of them dereference the answer unconditionally, exactly as the C++
    /// does; 35 open with `if ctx_param(..).is_null()`, which is asking whether
    /// `WelsInitEncoderExt` has run. So the unconditional readers get a plain
    /// reference and [`param_opt`](Self::param_opt) keeps the guards' shape —
    /// A2's ruling for `rc_at`, at ten times the call count.
    ///
    /// # Panics
    /// If the parameter block is not built. Every caller of this accessor
    /// dereferenced the raw one without asking, so the panic replaces a null
    /// dereference, not a branch; the callers that *do* ask keep asking, through
    /// [`param_opt`](Self::param_opt).
    #[inline]
    pub fn param(&self) -> &SWelsSvcCodingParam {
        self.pSvcParam
            .as_deref()
            .expect("the coding parameters are built by WelsInitEncoderExt")
    }

    /// [`param`](Self::param) for the writers: init, `SetOption`, and the
    /// per-layer bookkeeping in `ref_list_mgr_svc.rs` / `encoder_context.rs`.
    ///
    /// **Single-threaded only** — see [`rc_at_mut`](Self::rc_at_mut). A7's
    /// classification found no diagonal here either: every body that writes
    /// through the parameters holds the context by `&mut sWelsEncCtx` or is on
    /// the C-API/init path, and every one of the fork-reachable bodies —
    /// `RcCalculateMbQp`, `RcJudgeBaseUsability`, `WelsRcMbInitDisable` and
    /// `svc_encode_slice.rs`'s slice bodies — only reads.
    ///
    /// # Panics
    /// As [`param`](Self::param).
    #[inline]
    pub fn param_mut(&mut self) -> &mut SWelsSvcCodingParam {
        self.pSvcParam
            .as_deref_mut()
            .expect("the coding parameters are built by WelsInitEncoderExt")
    }

    /// [`param`](Self::param) **as the question the thirty-five guards ask** —
    /// "has `WelsInitEncoderExt` built the parameters yet?". The raw accessor
    /// answered null and they asked `is_null()`; this is the same branch with
    /// the question in the type.
    #[inline]
    pub fn param_opt(&self) -> Option<&SWelsSvcCodingParam> {
        self.pSvcParam.as_deref()
    }

    /// The encoder's **kernel dispatch table** — T6.I1, `pCtx->pFuncList`, and
    /// never absent: the context owns the `Box` from its constructor on, which is
    /// why this is a plain `&` where `vaa` and `ref_list` are `Option`s. The
    /// twenty-six `if let Some(..) = ctx_func_list(..).as_ref()` guards A6 found
    /// were asking about a null a `Box` cannot hold.
    ///
    /// **F212's flip, and F191's objection answered by taking it.** F191 ruled
    /// that this accessor "cannot take a shared projection at all", because the
    /// table is re-written at frame cadence (`SetFastCodingFunc` /
    /// `SetNormalCodingFunc`) and a reader could hold one across the re-write.
    /// Measured: the re-write is **two fields** (`pfIntraFineMd`,
    /// `sSampleDealingFuncs.pfMdCost`) in one body with one caller
    /// (`PreprocessSliceCoding`), which already derived exactly the `&mut` that
    /// [`func_list_mut`](Self::func_list_mut) is. So under the flip "a reader
    /// holds one across the re-write" stops being a hazard and becomes a
    /// **compile error** wherever the context is a reference — borrowck refuses
    /// precisely what F191 was worried about. The dispatch *enums* F191 prefers
    /// are a different debt and the plan schedules them at C1.
    ///
    /// Where the context is a raw — the fork — borrowck referees nothing and
    /// F208's rule applies as it does to every other reader here: the call is a
    /// shared reborrow of the **whole** context, used in the expression and never
    /// stored across a `&mut`-shaped derivation into it. The fork never writes
    /// this table, so a shared projection is all it has ever needed.
    #[inline]
    pub fn func_list(&self) -> &SWelsFuncPtrList {
        &self.pFuncList
    }

    /// [`func_list`](Self::func_list) for the six bodies that write the table:
    /// `InitFunctionPointers` and `InitCoeffFunc` at init, `WelsRcInitModule` and
    /// `SetOption` for `pfRc`, `PreprocessSliceCoding` for the two frame-cadence
    /// fields, and the parameter-set strategy's own `as_mut` callers.
    ///
    /// **Single-threaded only** — see [`rc_at_mut`](Self::rc_at_mut). It is the
    /// half of the flip that makes F191's hazard unrepresentable: no `&` to the
    /// table can be live across a call that needs this.
    #[inline]
    pub fn func_list_mut(&mut self) -> &mut SWelsFuncPtrList {
        &mut self.pFuncList
    }

    /// The frame's **video-analysis block** — T6.H10, `pCtx->pVaa`.
    ///
    /// **`Option`, because the absence is a state the callers already asked
    /// about.** The raw accessor answered null before the preprocessor builds
    /// one, and a dozen consumers opened with `if !ctx_vaa(..).is_null()` while
    /// it was raw; those
    /// guards are `let Some(..) else` / `.is_some()` now, which is the same
    /// branch with the question asked in the type.
    ///
    /// In-fork this is the only path — the workers read the analysis the
    /// preprocessor wrote and never write it back. The writers are the
    /// preprocessor and the reference-list managers, all single-threaded.
    #[inline]
    pub fn vaa(&self) -> Option<&SVAAFrameInfo> {
        self.pVaa.as_deref()
    }

    /// [`vaa`](Self::vaa) for the preprocessor and the reference-list managers.
    ///
    /// **Single-threaded only** — see [`rc_at_mut`](Self::rc_at_mut).
    #[inline]
    pub fn vaa_mut(&mut self) -> Option<&mut SVAAFrameInfo> {
        self.pVaa.as_deref_mut()
    }

    /// [`vaa`](Self::vaa) **as a raw pointer**, null when the block is absent.
    ///
    /// **The return stays raw, and the far end is why.** The one production
    /// caller hands it to `SWelsFuncPtrList::pfSetScrollingMv`, whose type
    /// (`PSetScrollingMv`, `wels_func_ptr_def.rs:131`) takes `*mut
    /// SVAAFrameInfo` — a C1 alias, so a reference here would have nothing to be
    /// passed as. The accessor itself is a safe fn: forming the pointer needs no
    /// `unsafe`, only dereferencing it does, and that belongs to the far end.
    /// It is also the root [`vaa_ext`](Self::vaa_ext) casts.
    #[inline]
    pub fn vaa_ptr(&self) -> *mut SVAAFrameInfo {
        match self.pVaa.as_deref() {
            Some(v) => v as *const SVAAFrameInfo as *mut SVAAFrameInfo,
            None => std::ptr::null_mut(),
        }
    }

    /// The video-analysis block **downcast to its screen-content extension** —
    /// `static_cast<SVAAFrameInfoExt*>(pCtx->pVaa)`, upstream's spelling.
    ///
    /// **Named once here so the cast is not fifteen separate claims.** Every
    /// consumer is inside the `SCREEN_CONTENT(dormant: Phase 10)` fence: the
    /// screen strategies (`RefStrategyKind::Screen`/`LosslessWithLtr`), the SCC
    /// rate-control arms, and `pfSetScrollingMv`'s judging arm — none of which a
    /// camera-usage preset can select, and the port never installs the one
    /// function that would allocate an `Ext` in the first place
    /// (`RequestMemoryVaaScreen`, F177). So `pVaa` is an `SVAAFrameInfo` and
    /// nothing else, the cast reads past the end of that allocation, and the
    /// arms are kept **compiling** rather than kept *correct* — Phase 10's job,
    /// which is what the tag says.
    #[inline]
    // unsafe-cat: SCREEN_CONTENT(dormant)
    pub fn vaa_ext(&self) -> *mut SVAAFrameInfoExt {
        self.vaa_ptr().cast()
    }

    /// The screen-content frame complexity — **the one read the dormant cast is
    /// made for, named once so it is not six separate claims** (S10.5a').
    ///
    /// This is [`vaa_ext`](Self::vaa_ext)'s own rationale applied one level down.
    /// That accessor exists because "the cast is not fifteen separate claims"; the
    /// *read through* it was still spelled out at six sites in `rc.rs`, each
    /// carrying its own `allow(unsafe_code)` and — wrongly — a
    /// `port-raw(Phase 9)` tag. The operation they perform is not Phase 9's: it is
    /// the `SVAAFrameInfoExt` downcast, which reads past the end of an
    /// `SVAAFrameInfo` because the port never installs `RequestMemoryVaaScreen`
    /// (F177). Six bodies were therefore counted as convertible when the thing
    /// blocking them belongs to Phase 10.
    ///
    /// So the claim is made here, once, under the tag that actually describes it,
    /// and the six callers become safe. Nothing about the read's correctness
    /// changes — it is exactly as dormant and exactly as wrong as it was, and
    /// Phase 10 now has one site to fix instead of six.
    ///
    /// # Safety
    /// **Unsound whenever it is reached**, by construction — see
    /// [`vaa_ext`](Self::vaa_ext). Every caller is inside the
    /// `SCREEN_CONTENT(dormant)` fence, which no camera-usage preset can select.
    #[inline]
    // unsafe-cat: SCREEN_CONTENT(dormant)
    #[allow(unsafe_code)]
    pub fn vaa_ext_screen_frame_complexity(&self) -> i64 {
        unsafe { (*self.vaa_ext()).sComplexityScreenParam.iFrameComplexity }
    }

    /// [`vaa_ext`](Self::vaa_ext) for its **one** writer, `AnalyzePictureComplexity`'s
    /// screen arm (`wels_preprocess.rs`), which takes `&mut
    /// SVAAFrameInfoExt::sComplexityScreenParam`.
    ///
    /// It exists so that arm's pointer is derived through a `&mut self` rather
    /// than through [`vaa_ext`](Self::vaa_ext)'s shared root: a write through a
    /// `SharedReadOnly`-derived raw is UB the moment it is driven, and dormant
    /// code should not be made *newly* wrong by the conversion that stops
    /// driving it. Single-threaded only, like every other `*_mut` here.
    #[inline]
    // unsafe-cat: SCREEN_CONTENT(dormant)
    pub fn vaa_ext_mut(&mut self) -> *mut SVAAFrameInfoExt {
        match self.pVaa.as_deref_mut() {
            Some(v) => (v as *mut SVAAFrameInfo).cast(),
            None => std::ptr::null_mut(),
        }
    }
}

/// Master runtime encoder context (`sWelsEncCtx` / `TagWelsEncCtx`).
#[repr(C)]
pub struct sWelsEncCtx {
    pub sLogCtx: SLogContext,
    /// The encoder's coding parameters — **T6.H11, owned, and the ownership read is
    /// the reason it is owned rather than left raw.**
    ///
    /// The brief's open question was whether this is the decoder's F41 shape — the
    /// context aliasing a block the api object owns and outlives — in which case it
    /// would stay raw and go to Phase 8. It is not. `WelsInitEncoderExt` calls
    /// `AllocCodingParam` to take a block **from the context's own `pMemAlign`**, and
    /// then *copies* the caller's parameters into it by value
    /// (`*pCtx.param_mut() = *pCodingParam`); `WelsUninitEncoderExt` frees it.
    /// The api object never hands the context a pointer to anything it owns — the
    /// only other writers in the tree are unit-test fixtures. The context is the sole
    /// owner, and now says so.
    ///
    /// Resolve it with [`sWelsEncCtx::param`]. `None` before `WelsInitEncoderExt` runs.
    pub pSvcParam: Option<Box<SWelsSvcCodingParam>>,
    pub iMvRange: i32,
    /// The motion-vector-difference cost table — **T6.H9, and plan item P11 landing.**
    /// 52 QP rows of `iMvdCostTableStride` entries each, plus F57's overshoot. Root:
    /// [`sWelsEncCtx::mvd_cost_table`]; the **origin** every consumer actually wants
    /// (the zero-MVD entry, `iMvdCostTableSize` into the table, so that a negative MVD
    /// is a negative offset) is [`sWelsEncCtx::mvd_cost_origin`].
    pub pMvdCostTable: Vec<u16>,
    pub iMvdCostTableSize: i32,
    pub iMvdCostTableStride: i32,
    // encoder_context.h:129-136 carries five per-macroblock scratch arrays here --
    // `pMvUnitBlock4x4`, `pRefIndexBlock4x4`, `pNonZeroCountBlocks`,
    // `pIntra4x4PredModeBlocks` and `pSadCostMb` (above). **T6.C1** moved all five
    // into `SMB` as inline arrays, so the context neither allocates them, wires
    // them, nor frees them; the two parity banks the first two carried are
    // unnecessary once every macroblock owns its row.
    /// **T6.H1 — owned.** `RequestMemorySvc` used to `WelsMallocz` this and
    /// `WelsUninitEncoderExt` to free it, together with the one block hanging off it.
    /// `None` is the null the raw pointer held before `AllocStrideTables` runs, and
    /// the drop that replaces both frees is the context's own — see the field's
    /// accessors, [`ctx_stride_enc_block_offset`] and its three siblings.
    pub pStrideTab: Option<Box<SStrideTables>>,
    /// **T6.I1 — owned.** The kernel dispatch table, `WelsMallocz`'d at
    /// `WelsInitEncoderExt` and `WelsFree`'d in the teardown cascade until this
    /// session; the context owns the `Box` and it drops with the context.
    ///
    /// A plain `Box`, not an `Option<Box<_>>` like [`pSvcParam`](Self::pSvcParam):
    /// the table has no "not built yet" state worth modelling. Its `Default` is
    /// every slot `None`, which is bit-for-bit the image `WelsMallocz` produced,
    /// so the context is born with the same table the C++ memsets and
    /// `InitFunctionPointers` writes over it exactly as before. Every null check
    /// on the *field* dies with the null; the checks on raw table *parameters*
    /// stay, because the survivors still take one. Root: [`sWelsEncCtx::func_list`].
    pub pFuncList: Box<SWelsFuncPtrList>,
    /// **S3.B1 — owned.** The slice-threading block, `Box`-built by
    /// `RequestMtResource` and dropped by `ReleaseMtResource`; `None` is the null
    /// that meant "single-threaded encoder". The fork reads it through
    /// [`ctx_slice_threading_raw`](crate::encoder::slice_multi_threading::ctx_slice_threading_raw)
    /// — a slot read (F71), so worker cursors carry the block's own provenance.
    pub pSliceThreading: Option<Box<SSliceThreading>>,
    // `pTaskManage: *mut c_void` stood here — an `IWelsTaskManage*` in C++, erased to
    // `c_void` because the port could not name the type from this module. It held the
    // one reference to the process-wide thread pool. Deleted at T7.B4; the encoder
    // forks with `std::thread::scope`, and a scope has no object to own.
    // It is also one of the twelve `!Sync` reasons F67 counted, so the count is
    // eleven now — see the finding's disposition.
    /// `IWelsReferenceStrategy*` in C++ (`encoder_context.h`); **T4b.2b** made it
    /// the strategy's *identity* instead of a pointer to an object carrying only a
    /// back-pointer to this very struct. See [`RefStrategyKind`].
    ///
    /// **S20**: this is `#[repr(C)]` and the member sits between two 8-byte-aligned
    /// pointers, so the 7 bytes of padding that realign `pEncPic` exactly replace the
    /// 7 bytes the pointer loses — the change that introduced this field moved
    /// neither `assert_size!(sWelsEncCtx, ...)` nor any of the fifteen
    /// `assert_ctx_offset!` pins.
    ///
    /// The numbers this note used to quote are gone: session H's owned members are
    /// `Vec`s at 16 bytes over the pointer each, so every pin after the first of them
    /// has moved, and they are the port's own measured offsets rather than C++
    /// `offsetof` values. What the pins still catch is a field added, dropped or
    /// re-widened without anyone noticing — which is why they are re-measured in the
    /// commit that moves them and never deleted.
    pub eRefStrategy: crate::encoder::ref_list_mgr_svc::RefStrategyKind,
    /// The source picture being encoded — a slot of `pVpp`'s spatial pool.
    pub pEncPic: Option<SrcPicId>,
    /// The picture being reconstructed into, and the one being referenced — slots of
    /// **the current dependency layer's** `SRefList` (`ppRefPicListExt[uiDependencyId]`).
    pub pDecPic: Option<RecPicId>,
    pub pRefPic: Option<RecPicId>,
    /// The layer the encoder is working on, as a **position in `ppDqLayerList`** —
    /// T6.G2. It was a raw `SDqLayer` alias into the list two lines down.
    ///
    /// The list is built once by `InitDqLayers`, freed once by `FreeDqLayer`, and
    /// nothing permutes it (S34, re-grepped), so a position is a stable identity and
    /// an index is faithful where the address was. Every one of the ~150 consumers
    /// resolves it through [`current_layer`](crate::encoder::svc_encode_slice::current_layer),
    /// which answers the same raw layer cursor this field used to hold — the
    /// cursor idiom downstream is unchanged.
    ///
    /// **`None` is "no layer is current" and is why the constructor had to land
    /// first**: `LayerIdx` is a plain `u8` with no niche, so the all-zero image of
    /// this field is `Some(LayerIdx(0))` — the base layer — and a zeroed shell
    /// would have silently produced a context that already had one (F56/S21).
    pub iCurDqLayer: Option<crate::encoder::svc_encode_slice::LayerIdx>,
    /// **T6.H8 — owned.** One DQ layer per dependency layer. As with
    /// [`ppRefPicListExt`](Self::ppRefPicListExt), the `SDqLayer`s have been
    /// `Box`-built since T6.D3 and the list is simply their owner now; `None` is the
    /// null a slot held before `InitDqLayers` filled it. Resolve a layer with
    /// [`ctx_dq_layer`], or the *current* one with
    /// [`current_layer`](crate::encoder::svc_encode_slice::current_layer).
    pub ppDqLayerList: Vec<Option<Box<SDqLayer>>>,
    /// **T6.H7 — owned.** One reference list per dependency layer. The array was a
    /// `WelsMallocz`'d block of pointers; the `SRefList`s themselves have been
    /// `Box`-built since T6.F1, so the list is simply their owner now. `None` is the
    /// null a slot held between `RequestMemorySvc` sizing the array and
    /// `InitDqLayers` filling it. Resolve a layer's list with
    /// [`sWelsEncCtx::ref_list`] / [`sWelsEncCtx::ref_list_mut`].
    pub ppRefPicListExt: Vec<Option<Box<SRefList>>>,
    pub pRefList0: [Option<RecPicId>; 16],
    /// **T6.H5 — owned.** One long-term-reference state per dependency layer,
    /// `WelsMallocz`'d and then `ResetLtrState`'d entry by entry. Root:
    /// [`ctx_ltr`]; the per-layer entry: [`ctx_ltr_at`].
    pub pLtr: Vec<SLTRState>,
    pub bCurFrameMarkedAsSceneLtr: bool,
    pub eSliceType: EWelsSliceType,
    pub eNalType: EWelsNalUnitType,
    pub eNalPriority: EWelsNalRefIdc,
    pub eLastNalPriority: [EWelsNalRefIdc; MAX_DEPENDENCY_LAYER],
    pub iNumRef0: u8,
    pub uiDependencyId: u8,
    pub uiTemporalId: u8,
    pub bNeedPrefixNalFlag: bool,
    /// **T6.H6 — owned, and it took the rate controller's own allocations with it.**
    /// One state per spatial layer; each holds the five arrays `RcInitLayerMemory`
    /// used to carve from a `CMemoryAlign` block. Root: [`ctx_rc`]; per-layer:
    /// [`sWelsEncCtx::rc_at`].
    pub pWelsSvcRc: Vec<SWelsSvcRc>,
    pub bCheckWindowStatusRefreshFlag: bool,
    pub iCheckWindowStartTs: i64,
    pub iCheckWindowCurrentTs: i64,
    pub iCheckWindowInterval: i32,
    pub iCheckWindowIntervalShift: i32,
    pub bCheckWindowShiftResetFlag: bool,
    pub iGlobalQp: i32,
    /// The video-analysis block for the frame in flight — **T6.H10, owned.** It has
    /// been `Box`-built and has owned its seven per-frame arrays since T6.F3; this is
    /// the last step, giving the `Box` an owner so `Create`/`Destroy` are `new`/`Drop`.
    /// `None` before the preprocessor runs. Resolve it with [`sWelsEncCtx::vaa`].
    pub pVaa: Option<Box<SVAAFrameInfo>>,
    /// **S3.B1 — owned.** The preprocess object, `Box`-built by
    /// [`CWelsPreProcess::CreatePreProcess`] and dropped by the teardown; `None` is
    /// the null the raw held before init and after `FreeMemorySvc`. The methods
    /// that take both `&mut self` (the vpp) and `&mut sWelsEncCtx` are called
    /// through the `Option::take` dance — the box moves out for the call and back
    /// after, which is a pointer move and leaves no aliasing for either referee.
    /// In-fork bodies read it via `(*pCtx).pVpp.as_deref()` — a *field*-scoped
    /// shared borrow, narrower than any accessor retag (F208).
    pub pVpp: Option<Box<crate::encoder::wels_preprocess::CWelsPreProcess>>,
    /// **T6.H2 — owned.** `RequestMemorySvc` sized this from the strategy's
    /// `GetNeededSpsNum` and `WelsMallocz`'d it; the length is the same number and
    /// the entries are the same zeros. Reach its root with [`ctx_sps_array`]; the
    /// **active** entry is [`iSps`](Self::iSps) below.
    pub pSpsArray: Vec<SWelsSPS>,
    /// The **active** SPS, as its position in `pSpsArray` — T6.G3. It was a pointer
    /// into that array, aimed at the head by `WelsInitEncoderExt` and never re-aimed,
    /// which is what `Some(SpsId(0))` says without a second address to keep true.
    /// Resolve it with [`ctx_sps`](crate::encoder::svc_encode_slice::ctx_sps).
    pub iSps: Option<SpsId>,
    /// **T6.H2 — owned**; see [`pSpsArray`](Self::pSpsArray). Root: [`ctx_pps_array`].
    pub pPPSArray: Vec<SWelsPPS>,
    /// The **active** PPS, as its position in `pPPSArray` — see [`iSps`](Self::iSps).
    /// Resolve it with [`ctx_pps`](crate::encoder::svc_encode_slice::ctx_pps).
    pub iPps: Option<PpsId>,
    /// **T6.H2 — owned**; see [`pSpsArray`](Self::pSpsArray). Root: [`ctx_subset_array`].
    ///
    /// **Empty is the null the raw pointer held**: `RequestMemorySvc` allocated
    /// nothing at all when `GetNeededSubsetSpsNum()` answered 0 (simulcast AVC, and
    /// every single-layer configuration), and every consumer tests for it.
    pub pSubsetArray: Vec<SSubsetSps>,
    // **`pSubsetSps` stood here and is deleted, not converted** (T6.G3). The C++
    // declares it (`encoder_context.h`) and this port transcribed it; neither ever
    // read it, and neither ever wrote it. `WelsInitCurrentLayer` aims
    // `pCurDq->sLayerInfo.pSubsetSpsP` at `pSubsetArray[iCurSpsId]` and every subset
    // consumer goes through the layer. A field that is only ever a declaration is
    // not an alias to convert; it is a line to remove.

    pub iSpsNum: i32,
    pub iSubsetSpsNum: i32,
    pub iPpsNum: i32,
    /// **S3.B1 — owned.** The encoder output block, `Box`-built at init
    /// (`new_boxed`) and dropped at teardown; `None` is the raw's null. The two
    /// fork-reachable readers (`slice_bs_buffer`, `slice_writer`) resolve it
    /// through [`ctx_out_raw`] on their **main-thread-only** arm — F217's probe is
    /// the measurement that the arm never runs in-fork.
    pub pOut: Option<Box<SWelsEncoderOutput>>,
    /// The frame's output bitstream — **T6.H4, and the encoder's one arena of
    /// bytes.** Every NAL the frame emits is written into it at `iPosBsBuffer`, and
    /// `SLayerBSInfo::pBsBuf` holds cursors into it that outlive the call that made
    /// them. Root: [`sWelsEncCtx::frame_bs`]; the write cursor:
    /// [`sWelsEncCtx::frame_bs_cur`].
    ///
    /// **A recorded deviation.** The C++ takes this block with `WelsMalloc`, not
    /// `WelsMallocz` — it is the one member of `RequestMemorySvc`'s set that starts
    /// *uninitialized*. The `Vec` is zero-filled, because a safe container has no
    /// uninitialized alternative; that is sound because every read of this buffer
    /// sits behind a write cursor (`iPosBsBuffer` only ever advances past bytes a NAL
    /// writer has just written, and `pOut->iNalLen` bounds every read back), so no
    /// consumer can observe the difference between "uninitialized" and "zero".
    pub pFrameBs: Vec<u8>,
    pub iFrameBsSize: i32,
    pub iPosBsBuffer: i32,
    pub sSpatialIndexMap: [SSpatialPicIndex; MAX_DEPENDENCY_LAYER],
    pub iSliceBufferSize: [i32; MAX_DEPENDENCY_LAYER],
    pub bRefOfCurTidIsLtr: [[bool; MAX_TEMPORAL_LEVEL]; MAX_DEPENDENCY_LAYER],
    pub iMaxSliceCount: i32,
    pub iActiveThreadsNum: i16,
    /// **T6.H3 — owned.** One row per dependency layer, `WelsMallocz`'d at
    /// `RequestMemorySvc` and freed in the cascade; `SDqIdc` is four bytes of POD and
    /// its derived `Default` *is* the memset image. Root: [`ctx_dq_idc_map`].
    pub pDqIdcMap: Vec<SDqIdc>,
    /// The C++ declares a companion `SParaSetOffset*` beside this one, pointing
    /// either here or at the caller's vector. **T6.I0 deleted that pointer**: in
    /// the whole port it was declared, null-initialised, and listed in the
    /// equality instrument — never read, never assigned anywhere. This field,
    /// held by value, is the vector.
    pub sPSOVector: SParaSetOffset,
    // **`pMemAlign: *mut CMemoryAlign` stood here — T7.C6.** The C++'s aligned
    // allocator, `WelsInitEncoderExt`'s first allocation and `WelsUninitEncoderExt`'s
    // last free. Phase 6 took 45 of its call sites to 15 and Phase 7 took the last
    // 15 to zero (T7.C4 the slice bitstreams, T7.C5 the two MT buffer arrays, T7.C6
    // the two unreachable screen-content functions), so what was left was a heap
    // object the encoder constructed, carried through every allocation signature in
    // the crate, and destroyed — with **no reader anywhere**, including the two dead
    // `let pMa = (*pCtx).pMemAlign;` bindings the preprocessor still had. Deleted with
    // the field, the constructor, the free and the parameter chain.
    pub uiStartTimestamp: i64,
    pub sEncoderStatistics: [crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics; MAX_DEPENDENCY_LAYER],
    pub iStatisticsLogInterval: i32,
    pub iLastStatisticsLogTs: i64,
    pub iEncoderError: i32,
    // `mutexEncoderError: *mut c_void` stood here, guarding `FinishTask`'s
    // `iEncoderError |= m_eTaskResult` from the worker threads. Results travel back
    // through the join now and the calling thread ORs them, so the field has no
    // writer left. Deleted at T7.B4 — F67's twelve `!Sync` reasons are ten.
    pub bDeliveryFlag: bool,
    pub sWelsCabacContexts: [[[SStateCtx; WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
    pub uiLastTimestamp: i64,
    pub pDynamicBsBuffer: [Vec<u8>; MAX_THREADS_NUM],
}

impl sWelsEncCtx {
    /// The encoder context's **allocation zero**, spelled out — `WelsInitEncoderExt`'s
    /// `WelsMalloc` + `memset(0)` in a form that survives a field changing type.
    ///
    /// # Why a constructor at all, when the shell it replaces was correct
    ///
    /// `mem::zeroed()` is not a construction, it is a *bet*: that every field's
    /// all-zero bit pattern is a value that field is allowed to hold. The bet is
    /// currently sound and has been audited field by field (S21) — three of the
    /// enums below carry their zero variant *because* of that audit, and
    /// `ref_strategy_zero_is_the_default_arm` is the assertion that keeps one of them
    /// honest. But it is a bet that has to be re-won on every field that changes
    /// type, silently, by whoever changes it, and Phase 6 sessions G and H change
    /// almost every field in this struct. `Option<LayerIdx>` — the very next step —
    /// has **no niche**: `LayerIdx` is a plain `u8`, so all-zero is `Some(LayerIdx(0))`,
    /// which is layer zero, not "no layer". The shell would keep compiling and
    /// keep producing a context whose current layer is silently the base layer.
    ///
    /// So the constructor lands **before** anything owns and before any field flips
    /// (F56/S21: zeros are ruled, not defaulted). What it buys is spent in steps 2
    /// and 3 and in session H; what it costs is this comment.
    ///
    /// # What it is not
    ///
    /// It is **not** an "init". `WelsInitEncoderExt` does the initialization, in the
    /// order the C++ does it, and every non-zero starting value the encoder has lives
    /// there. Session F's `SPicture` lesson applies here in reverse: there,
    /// `with_planes` had to reproduce the *memset*, not `Default`, because the live
    /// value of `eSliceType` was `P_SLICE` and `Default` said `UNKNOWN_SLICE`. Here
    /// the memset **is** the semantics, so nothing may be imported into `new()` that
    /// the zeroed shell did not have. `ctx_new_reproduces_the_zeroed_shell` is that
    /// rule as a test, and it is meant to read zero differences.
    ///
    /// # The zeros, and what each one means
    ///
    /// Grouped by who is responsible for making the field non-zero. Four groups:
    /// the log sink (the caller's, before anything else runs), the members
    /// `RequestMemorySvc` allocates (session H turns these into owned containers,
    /// and null is "not allocated yet"), the per-frame state `WelsInitCurrentLayer`
    /// and the frame loop restamp every frame (zero is "no frame has run"), and the
    /// parameter-set bookkeeping `InitDqLayers` fills.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            // The caller's log sink, stamped in by `WelsInitEncoderExt` before
            // anything can log. **T8.B6/T8.B10**: this used to be three `*mut
            // c_void`s and "no sink installed" was three nulls; it is typed now —
            // `pfLog: WelsTraceCallback`, `None` when nothing is installed — and
            // the level travels with it. The one member that is still a pointer is
            // `pLogCtx`, which is the *caller's* opaque context and C-ABI by
            // definition.
            sLogCtx: SLogContext::default(),

            // ---- allocated by RequestMemorySvc; null == not allocated yet -------
            // (Session H's list. Every one of these is freed by FreeMemorySvc, and
            // null is the value that makes the paired free a no-op — which is why
            // the C++ can call it on a half-built context after an early failure.)
            pSvcParam: None,
            iMvRange: 0,                    // set by InitMvRange from the level limit
            pMvdCostTable: Vec::new(),
            iMvdCostTableSize: 0,           // paired with the table above
            iMvdCostTableStride: 0,
            pStrideTab: None,
            pFuncList: Box::new(SWelsFuncPtrList::default()),
            pSliceThreading: None,

            // `TemporalLayer`, the zero discriminant, is the variant the old
            // `CreateReferenceStrategy` factory's `_ =>` arm produced — asserted in
            // `ref_list_mgr_svc::tests::ref_strategy_zero_is_the_default_arm`.
            eRefStrategy: crate::encoder::ref_list_mgr_svc::RefStrategyKind::TemporalLayer,

            // ---- per-frame picture handles; None == no picture bound ------------
            // These are pool handles, not pointers, since T6.F1. `None` is the whole
            // reason the handles have a niche: it is a *state*, "the frame loop has
            // not picked a picture yet", and the encoder tests for it.
            pEncPic: None,
            pDecPic: None,
            pRefPic: None,

            // No layer is current until `WelsInitCurrentLayer` / `WelsSwapDqLayers`
            // names one, which cannot happen before `ppDqLayerList` below is
            // allocated. `None`, not `Some(LayerIdx(0))` — see the field.
            iCurDqLayer: None,
            ppDqLayerList: Vec::new(),
            ppRefPicListExt: Vec::new(),

            // `iNumRef0` below is the live length of this list; sixteen `None`s is the
            // empty list, and the two agree only because both are zero here.
            pRefList0: [None; 16],

            pLtr: Vec::new(),
            bCurFrameMarkedAsSceneLtr: false,

            // ---- per-frame NAL/slice state; restamped every frame ---------------
            // `P_SLICE` (0) is not a placeholder: it is the value the memset leaves
            // and the one an encode inherits until `DecideFrameType` runs.
            eSliceType: EWelsSliceType::P_SLICE,
            eNalType: EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
            eNalPriority: EWelsNalRefIdc::NRI_PRI_LOWEST,
            // Per-dependency-layer memory of the last NAL's priority, read back by
            // `LoadBackFrameNum` when a frame is dropped. Lowest == nothing sent yet.
            eLastNalPriority: [EWelsNalRefIdc::NRI_PRI_LOWEST; MAX_DEPENDENCY_LAYER],
            iNumRef0: 0,                    // the live length of pRefList0
            uiDependencyId: 0,              // the base layer, and a real starting value
            uiTemporalId: 0,                // likewise: T0
            bNeedPrefixNalFlag: false,

            // ---- rate control ---------------------------------------------------
            pWelsSvcRc: Vec::new(),
            // The check-window trio. Zero timestamps mean "the window has never
            // opened"; `WelsRcInitFuncPointers` and the first frame set all of them.
            bCheckWindowStatusRefreshFlag: false,
            iCheckWindowStartTs: 0,
            iCheckWindowCurrentTs: 0,
            iCheckWindowInterval: 0,
            iCheckWindowIntervalShift: 0,
            bCheckWindowShiftResetFlag: false,
            iGlobalQp: 0,                   // overwritten by WelsRcPictureInitGom etc.

            // ---- preprocessing --------------------------------------------------
            pVaa: None,
            pVpp: None,

            // ---- parameter sets: the arrays, their aliases, their counts --------
            // The three `Array` members are allocations (H's); the three singular
            // ones are *aliases into them* that `WelsInitEncoderExt` aims at the
            // heads — step 3 of this session makes them ids. The three counts are
            // `InitDqLayers`'s, and zero is the honest starting length.
            pSpsArray: Vec::new(),
            iSps: None,
            pPPSArray: Vec::new(),
            iPps: None,
            pSubsetArray: Vec::new(),
            iSpsNum: 0,
            iSubsetSpsNum: 0,
            iPpsNum: 0,

            // ---- output bitstream ------------------------------------------------
            pOut: None,
            pFrameBs: Vec::new(),
            iFrameBsSize: 0,                // paired with pFrameBs
            iPosBsBuffer: 0,                // the write cursor into it, rewound per AU

            // The spatial pool's per-layer index map. `SSpatialPicIndex::default()`
            // is `{ pSrc: None, iDid: 0 }`, which *is* this struct's zero image —
            // spelled through `Default` rather than repeated, because that impl is
            // itself the statement that the two agree.
            sSpatialIndexMap: [SSpatialPicIndex::default(); MAX_DEPENDENCY_LAYER],
            iSliceBufferSize: [0; MAX_DEPENDENCY_LAYER],
            // "Is the reference for this (did, tid) a long-term one?" — false until a
            // reference exists at all.
            bRefOfCurTidIsLtr: [[false; MAX_TEMPORAL_LEVEL]; MAX_DEPENDENCY_LAYER],
            iMaxSliceCount: 0,
            iActiveThreadsNum: 0,           // set from iMultipleThreadIdc

            pDqIdcMap: Vec::new(),

            // `sPSOVector` is held **by value**, and it is the only one: the C++'s
            // companion pointer is deleted at T6.I0 (see the field's doc comment).
            // `SParaSetOffset::default()` is all-zero throughout (its own impl,
            // field for field), which is the id-strategy's "no id has been handed
            // out yet".
            sPSOVector: SParaSetOffset::default(),


            // ---- statistics and timestamps ---------------------------------------
            // Timestamps are absolute and in the caller's clock, so zero is a real
            // "not yet stamped" and every consumer compares against the previous one.
            uiStartTimestamp: 0,
            sEncoderStatistics:
                [crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics::default(); MAX_DEPENDENCY_LAYER],
            iStatisticsLogInterval: 0,      // set from the param's log interval
            iLastStatisticsLogTs: 0,
            iEncoderError: 0,               // == ENC_RETURN_SUCCESS, and that matters
            bDeliveryFlag: false,

            // The CABAC probability tables. Zero is `{ MPS = 0, state = 0 }`, which is
            // not a valid coding state — `WelsCabacContextInit` fills all four
            // models for every QP before any of it is read, exactly as the C++ does
            // after its own memset.
            sWelsCabacContexts: [[[SStateCtx::new(0); WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
            uiLastTimestamp: 0,

            // Phase 7's. One dynamic bitstream buffer per thread, allocated on the
            // first slice that needs one.
            pDynamicBsBuffer: std::array::from_fn(|_| Vec::new()),
        }
    }
}

impl Default for sWelsEncCtx {
    /// The zeroed shell, by way of [`new`](Self::new) — same bytes, ruled rather
    /// than bet on.
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Core Encoder Functions (encoder.cpp)
// ============================================================================

/// Initializes input source picture geometry, color planes, and line strides.
///
/// # Safety
/// `pSrcPic` must point to a valid, writable `SSourcePicture` or be null.
///
/// **`*mut`, not `*const`** (Phase 6 session B). The C++ signature is
/// `InitPic(const void* kpSrc, …)` and its first act is
/// `SSourcePicture* pSrcPic = (SSourcePicture*)kpSrc;` — it casts the `const`
/// away and writes eight fields through it. Typing the parameter `*const` here
/// preserved that lie, and the `exit` battery's Miri step caught what the lie
/// costs in Rust: the unit test passed `&src_pic`, a *shared* reference, and the
/// first write through the cast pointer is UB — "that tag only grants
/// SharedReadOnly permission". A function that writes says `*mut`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitPic(
    pSrcPic: *mut SSourcePicture,
    kiColorspace: i32,
    kiWidth: i32,
    kiHeight: i32,
) -> i32 {

    if pSrcPic.is_null() || kiWidth == 0 || kiHeight == 0 {
        return 1;
    }

    let vflip_mask = VideoFormat::videoFormatVFlip as i32;
    let base_colorspace = kiColorspace & !vflip_mask;

    (*pSrcPic).iColorFormat = kiColorspace;
    (*pSrcPic).iPicWidth = kiWidth;
    (*pSrcPic).iPicHeight = kiHeight;

    if base_colorspace != VideoFormat::videoFormatI420 as i32 {
        return 2;
    }

    match base_colorspace {
        cs if cs == VideoFormat::videoFormatI420 as i32
            || cs == VideoFormat::videoFormatYV12 as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = kiWidth;
            (*pSrcPic).iStride[1] = kiWidth >> 1;
            (*pSrcPic).iStride[2] = kiWidth >> 1;
            (*pSrcPic).iStride[3] = 0;
        }
        cs if cs == VideoFormat::videoFormatYUY2 as i32
            || cs == VideoFormat::videoFormatYVYU as i32
            || cs == VideoFormat::videoFormatUYVY as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = CALC_BI_STRIDE(kiWidth, 16);
            (*pSrcPic).iStride[1] = 0;
            (*pSrcPic).iStride[2] = 0;
            (*pSrcPic).iStride[3] = 0;
        }
        cs if cs == VideoFormat::videoFormatRGB as i32
            || cs == VideoFormat::videoFormatBGR as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = CALC_BI_STRIDE(kiWidth, 24);
            (*pSrcPic).iStride[1] = 0;
            (*pSrcPic).iStride[2] = 0;
            (*pSrcPic).iStride[3] = 0;
            if (kiColorspace & vflip_mask) != 0 {
                (*pSrcPic).iColorFormat = kiColorspace & !vflip_mask;
            } else {
                (*pSrcPic).iColorFormat = kiColorspace | vflip_mask;
            }
        }
        cs if cs == VideoFormat::videoFormatBGRA as i32
            || cs == VideoFormat::videoFormatRGBA as i32
            || cs == VideoFormat::videoFormatARGB as i32
            || cs == VideoFormat::videoFormatABGR as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = kiWidth << 2;
            (*pSrcPic).iStride[1] = 0;
            (*pSrcPic).iStride[2] = 0;
            (*pSrcPic).iStride[3] = 0;
            if (kiColorspace & vflip_mask) != 0 {
                (*pSrcPic).iColorFormat = kiColorspace & !vflip_mask;
            } else {
                (*pSrcPic).iColorFormat = kiColorspace | vflip_mask;
            }
        }
        _ => return 2,
    }

    0
}

/// Wires background detection function pointers into the encoder function table.
///
/// # Safety
/// `pFuncList` must be non-null and point to a valid `SWelsFuncPtrList`.
pub fn WelsInitBGDFunc(
    pFuncList: &mut SWelsFuncPtrList,
    kbEnableBackgroundDetection: bool,
) {
    if kbEnableBackgroundDetection {
        pFuncList.pfInterMdBackgroundDecision = Some(WelsMdInterJudgeBGDPskip);
        pFuncList.pfMdBackgroundInfoUpdate = Some(WelsMdUpdateBGDInfo);
    } else {
        pFuncList.pfInterMdBackgroundDecision = Some(WelsMdInterJudgeBGDPskipFalse);
        pFuncList.pfMdBackgroundInfoUpdate = Some(WelsMdUpdateBGDInfoNULL);
    }
}

/// Initializes encoder compute kernel function pointers.
pub fn InitFunctionPointers(
    pEncCtx: &mut sWelsEncCtx,
    _uiCpuFlag: u32,
) -> i32 {
    // A third arm testing the table for null was here; T6.I1 made it an owned
    // `Box`, so there is no null to test.
    // T9.H: the `pEncCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    // **A7: the `pParam` argument is gone** — T9.G6's note above this call in
    // `encoder_ext.rs` said the argument was hoisted because "the call takes the
    // context retag and this argument reads through the same context", which is
    // F192's shape written out. The callee holds the context, so it derives the
    // parameters itself and there is no second route left to hoist.
    if pEncCtx.param_opt().is_none() {
        return ENC_RETURN_SUCCESS;
    }
    // **T6.I2.** One `&mut` for the whole function, derived from the owner once —
    // rule 6's shape. The alternative, a fresh `&mut *pFuncList` at each of the
    // fourteen calls below, is the shape that compiles and is UB: each one pops
    // the raw the next call re-uses. The survivors that still take `*mut` get a
    // reborrow (`&mut *fl`) rather than the binding, so `fl` outlives them.
    // §4.6, reorder: the one context read this body makes below — the complexity
    // mode, for `WelsInitSCDPskipFunc`'s argument — is lifted above the table's
    // `&mut`. Under the raw accessor the two coexisted silently; under the flip
    // the compiler asks, and the answer is a scalar copied one statement earlier.
    let kiComplexityMode = pEncCtx.param().iComplexityMode as i32;
    let kbEnableBackgroundDetection = pEncCtx.param().bEnableBackgroundDetection;
    let kbEnableSceneChangeDetect = pEncCtx.param().bEnableSceneChangeDetect;
    let kiEntropyCodingModeFlag = pEncCtx.param().iEntropyCodingModeFlag;
    let kiRCMode = pEncCtx.param().iRCMode;
    let keSpsPpsIdStrategy = pEncCtx.param().eSpsPpsIdStrategy;
    let kbSimulcastAVC = pEncCtx.param().bSimulcastAVC;
    let kiSpatialLayerNum = pEncCtx.param().iSpatialLayerNum;
    let bScreenContent = pEncCtx.param().iUsageType
        == crate::api::codec_api::EUsageType::SCREEN_CONTENT_REAL_TIME;
    let fl: &mut SWelsFuncPtrList = pEncCtx.func_list_mut();

    // `encoder.cpp:193` installed `sExpandPicFunc` here. T4b.3b deleted the table:
    // the call it fed now names its two kernels directly. The history is worth one
    // line, because this call was *missing* before Phase 4a found it, and with it
    // every slot stayed `None` and `WelsUpdateRefList`'s `ExpandReferencingPicture`
    // expanded nothing -- a bug that a table of optional function pointers can
    // have and a direct call cannot.

    /* Intra_Prediction_fn */
    crate::encoder::get_intra_predictor::WelsInitIntraPredFuncs(&mut *fl, _uiCpuFlag);

    /* ME func */
    crate::encoder::svc_motion_estimate::WelsInitMeFunc(&mut *fl, _uiCpuFlag, bScreenContent);

    /* sad, satd, average */
    crate::encoder::sample::WelsInitSampleSadFunc(&mut *fl, _uiCpuFlag);

    WelsInitBGDFunc(&mut *fl, kbEnableBackgroundDetection);
    crate::encoder::svc_mode_decision::WelsInitSCDPskipFunc(
        &mut *fl,
        bScreenContent
            && kbEnableSceneChangeDetect
            && kiComplexityMode
                < (crate::api::codec_api::ECOMPLEXITY_MODE::HIGH_COMPLEXITY as i32),
    );

    // for pfGetVarianceFromIntraVaa function ptr adaptive by CPU features
    crate::encoder::md::InitIntraAnalysisVaaInfo(&mut *fl, _uiCpuFlag);

    /* Motion compensation */
    crate::common::mc::InitMcFunc(&mut fl.sMcFuncs, _uiCpuFlag);
    InitCoeffFunc(&mut *fl, _uiCpuFlag, kiEntropyCodingModeFlag);

    crate::encoder::encode_mb_aux::WelsInitEncodingFuncs(&mut *fl, _uiCpuFlag);
    crate::encoder::decode_mb_aux::WelsInitReconstructionFuncs(&mut *fl, _uiCpuFlag);

    // C++ does NOT set pfInterMd here. It is assigned per-slice in
    // svc_encode_slice.cpp:733/736 to WelsMdInterMbEnhancelayer or WelsMdInterMb
    // depending on kbBaseAvail && kbHighestSpatial. This line used to assign
    // WelsMdSpatialelInterMbIlfmdNoilp, which is a different function with a
    // different signature (its last parameter is Mb_Type, not SMbCache*) -- the
    // mem::transmute around it was what let that through. WelsMdInterMb is not
    // ported yet, so the assignment belongs with that work, not here.

    crate::encoder::deblocking::DeblockingInit(&mut fl.pfDeblocking, _uiCpuFlag as i32);

    crate::encoder::rc::WelsRcInitFuncPointers(
        &mut fl.pfRc,
        kiRCMode,
    );

    // `WelsBlockFuncInit(&mut fl.pfSetNZCZero, ..)` stood here — the slot and
    // its installer went when `DeblockingBSCalc_c` went direct (session F).

    crate::encoder::md::InitFillNeighborCacheInterFunc(
        &mut *fl,
        kbEnableBackgroundDetection as i32,
    );

    // encoder.cpp:227. Only CONSTANT_ID and INCREASING_ID are ported, so this returns
    // `None` — and hence ENC_RETURN_MEMALLOCERR — for the three listing strategies
    // rather than quietly substituting one; see
    // `paraset_strategy::CreateParametersetStrategy`.
    //
    // The assignment drops whatever was installed before, which is the only way this
    // can be reached twice: `WelsUninitEncoderExt` runs between two inits and takes
    // the field. **S23**: the object caches `eSpsPpsIdStrategy` as a
    // `ParasetIdKind`, and it cannot lag the live parameter — see the type's doc.
    fl.pParametersetStrategy =
        crate::encoder::paraset_strategy::CreateParametersetStrategy(
            keSpsPpsIdStrategy,
            kbSimulcastAVC,
            kiSpatialLayerNum,
        );
    if fl.pParametersetStrategy.is_none() {
        return ENC_RETURN_MEMALLOCERR;
    }

    ENC_RETURN_SUCCESS
}

/// `set_mb_syn_cavlc.cpp:305`. Selects the coefficient-writing entry points for the
/// configured entropy coder.
///
/// The SSE2/SSE4.2 `CavlcParamCal` variants are x86-only and this target reports
/// no CPU features (`WelsCPUFeatureDetect` returns 0), so only the `_c` kernel is
/// ever assigned.
///
/// **T4b.1**: the four entropy slots this function used to fill from one `if` are
/// one [`EntropyCoder`] now, so the `if` *is* the assignment. What is left of the
/// C++ shape is `pfCavlcParamCal`, which is CPU dispatch and Phase 4a's kind.
fn InitCoeffFunc(
    pFuncList: &mut SWelsFuncPtrList,
    _uiCpuFlag: u32,
    iEntropyCodingModeFlag: i32,
) {
    pFuncList.pfCavlcParamCal = Some(crate::encoder::svc_set_mb_syn_cavlc::CavlcParamCal_c);
    pFuncList.eEntropyCoder = EntropyCoder::from_flag(iEntropyCodingModeFlag);
}

/// Increments the H.264 slice header `frame_num` syntax element for spatial layer `kiDidx`.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateFrameNum(pEncCtx: &mut sWelsEncCtx, kiDidx: i32) {
    // T9.H: the `pEncCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pEncCtx.param_opt().is_none() || ctx_sps(pEncCtx).is_null() {
        return;
    }
    // T9.G4: the `ctx_sps` read below is hoisted above the cursor rather than left
    // inside the branch. `uiLog2MaxFrameNum` is a sequence-parameter constant and
    // `ctx_sps` is null-guarded above, so reading it unconditionally is pure — and
    // it is the whole-context call this body used to make with a cursor live.
    let max_frame_num_minus1 = (1 << (*ctx_sps(pEncCtx)).uiLog2MaxFrameNum) - 1;
    let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pEncCtx)).sDependencyLayers[kiDidx as usize]);
    let mut bNeedFrameNumIncreasing = false;

    if pEncCtx.eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        if (*pParamInternal).iFrameNum < max_frame_num_minus1 {
            (*pParamInternal).iFrameNum += 1;
        } else {
            (*pParamInternal).iFrameNum = 0;
        }
    }

    pEncCtx.eLastNalPriority[kiDidx as usize] = EWelsNalRefIdc::NRI_PRI_LOWEST;
}

/// Rolls back the `frame_num` counter if a reference frame encoding attempt fails.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn LoadBackFrameNum(pEncCtx: &mut sWelsEncCtx, kiDidx: i32) {
    // T9.H: the `pEncCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pEncCtx.param_opt().is_none() || ctx_sps(pEncCtx).is_null() {
        return;
    }
    // T9.G4: the `ctx_sps` read below is hoisted above the cursor rather than left
    // inside the branch. `uiLog2MaxFrameNum` is a sequence-parameter constant and
    // `ctx_sps` is null-guarded above, so reading it unconditionally is pure — and
    // it is the whole-context call this body used to make with a cursor live.
    let max_frame_num_minus1 = (1 << (*ctx_sps(pEncCtx)).uiLog2MaxFrameNum) - 1;
    let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pEncCtx)).sDependencyLayers[kiDidx as usize]);
    let mut bNeedFrameNumIncreasing = false;

    if pEncCtx.eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        if (*pParamInternal).iFrameNum != 0 {
            (*pParamInternal).iFrameNum -= 1;
        } else {
            (*pParamInternal).iFrameNum = max_frame_num_minus1;
        }
    }
}

/// Reinitializes bitstream buffer write offsets and NAL indices.
///
/// # Safety
/// `pEncCtx` must be non-null and contain a valid `pOut`.
pub fn InitBitStream(pEncCtx: &mut sWelsEncCtx) {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    let Some(pOut) = pEncCtx.pOut.as_deref_mut() else {
        return;
    };
    pOut.iNalIndex = 0;
    pOut.iLayerBsIndex = 0;

    // Was `InitBits(&…sBsWrite, …pBsBuffer, …uiSize)`. The buffer and its length stay
    // where they were; the writer is a position, and resetting it is all `InitBits`
    // did that still means anything. Its `kpBuf: *const u8` parameter — stored as
    // `pStartBuf: *mut u8` and written through — is deleted rather than amended
    // (`phase2_findings.md` F13, third site).
    pOut.sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();
    pEncCtx.iPosBsBuffer = 0;
}

/// Configures slice types, NAL headers, and Picture Order Count (POC) for the frame.
///
/// # Safety
/// `pEncCtx` must be non-null and properly initialized.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitFrameCoding(
    pEncCtx: &mut sWelsEncCtx,
    keFrameType: EVideoFrameType,
    kiDidx: i32,
) {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    if pEncCtx.param_opt().is_none() || ctx_sps(pEncCtx).is_null() {
        return;
    }
    // T9.G4, with `UpdateFrameNum`'s: `iLog2MaxPocLsb` is hoisted above the cursor
    // (a sequence constant, and `ctx_sps` is null-guarded above), and the cursor is
    // derived **per branch** rather than once at the top. The three branches are
    // exclusive and each one's last use of it precedes its `UpdateFrameNum` call, so
    // nothing here was unsound; deriving per branch is what makes that visible to a
    // reader and to the detector, and it is what the borrow checker will need when
    // this body takes `&mut`.
    let max_poc_boundary = (1 << (*ctx_sps(pEncCtx)).iLog2MaxPocLsb) - 2;

    if keFrameType == EVideoFrameType::videoFrameTypeP {
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pEncCtx)).sDependencyLayers[kiDidx as usize]);
        (*pParamInternal).iFrameIndex += 1;

        if (*pParamInternal).iPOC < max_poc_boundary {
            (*pParamInternal).iPOC += 2;
        } else {
            (*pParamInternal).iPOC = 0;
        }

        UpdateFrameNum(pEncCtx, kiDidx);

        pEncCtx.eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        pEncCtx.eSliceType = EWelsSliceType::P_SLICE;
        pEncCtx.eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGH;
    } else if keFrameType == EVideoFrameType::videoFrameTypeIDR {
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pEncCtx)).sDependencyLayers[kiDidx as usize]);
        (*pParamInternal).iFrameNum = 0;
        (*pParamInternal).iPOC = 0;
        (*pParamInternal).bEncCurFrmAsIdrFlag = false;
        (*pParamInternal).iFrameIndex = 0;

        pEncCtx.eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
        pEncCtx.eSliceType = EWelsSliceType::I_SLICE;
        pEncCtx.eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;

        (*pParamInternal).iCodingIndex = 0;
    } else if keFrameType == EVideoFrameType::videoFrameTypeI {
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pEncCtx)).sDependencyLayers[kiDidx as usize]);
        if (*pParamInternal).iPOC < max_poc_boundary {
            (*pParamInternal).iPOC += 2;
        } else {
            (*pParamInternal).iPOC = 0;
        }

        UpdateFrameNum(pEncCtx, kiDidx);

        pEncCtx.eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        pEncCtx.eSliceType = EWelsSliceType::I_SLICE;
        pEncCtx.eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;
    }
}

/// Evaluates VAA scene change analysis, LTR feedback, and rate control constraints to classify frame coding type.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DecideFrameType(
    pEncCtx: &mut sWelsEncCtx,
    kiSpatialNum: i8,
    kiDidx: i32,
    bSkipFrameFlag: bool,
) -> EVideoFrameType {
    // T9.H: the `pEncCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pEncCtx.param_opt().is_none() {
        return EVideoFrameType::videoFrameTypeInvalid;
    }
    // A7, §4.6 reorder: every parameter field this body reads is a scalar, so they
    // come out first and the block's `&mut` is over by the time the video-analysis
    // and reference-list readers are called. `pParamInternal` stays a raw cursor
    // (F71's asymmetry: `addr_of_mut!` inherits the parent's tag rather than
    // minting a child, so a later shared reader cannot pop it) and no `param`
    // call follows it in this body.
    let kiUsageType = pEncCtx.param().iUsageType;
    let kbSceneChangeDetect = pEncCtx.param().bEnableSceneChangeDetect;
    let kiSpatialLayerNum = pEncCtx.param().iSpatialLayerNum;
    let kbEnableLtr = pEncCtx.param().bEnableLongTermReference;
    let kiLTRRefNum = pEncCtx.param().iLTRRefNum;
    let pParamInternal =
        std::ptr::addr_of_mut!((*ctx_param_raw(pEncCtx)).sDependencyLayers[kiDidx as usize]);
    let mut iFrameType: EVideoFrameType;
    let mut bSceneChangeFlag = false;

    if kiUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
        let pVaa = pEncCtx.vaa();
        let vaa_idr = pVaa.is_some_and(|v| v.bIdrPeriodFlag);

        if !kbSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < kiSpatialLayerNum)
        {
            bSceneChangeFlag = false;
        } else if let Some(pVaa) = pVaa {
            bSceneChangeFlag = pVaa.bSceneChangeFlag;
        }

        if vaa_idr
            || (*pParamInternal).bEncCurFrmAsIdrFlag
            || (!kbEnableLtr && bSceneChangeFlag && !bSkipFrameFlag)
        {
            iFrameType = EVideoFrameType::videoFrameTypeIDR;
        } else if kbEnableLtr
            && (bSceneChangeFlag
                || pVaa.is_some_and(|v| v.eSceneChangeIdc == ESceneChangeIdc::LARGE_CHANGED_SCENE))
        {
            let mut iActualLtrcount = 0;
            {
                if let Some(ref_list_0) = pEncCtx.ref_list(0) {
                    for i in 0..kiLTRRefNum {
                        let Some(id) = ref_list_0.pLongRefList[i as usize] else {
                            continue;
                        };
                        let pic = ref_list_0.pic(id);
                        if pic.bUsedAsRef && pic.bIsLongRef && pic.bIsSceneLTR {
                            iActualLtrcount += 1;
                        }
                    }
                }
            }
            if iActualLtrcount == kiLTRRefNum && bSceneChangeFlag {
                iFrameType = EVideoFrameType::videoFrameTypeIDR;
            } else {
                iFrameType = EVideoFrameType::videoFrameTypeP;
                pEncCtx.bCurFrameMarkedAsSceneLtr = true;
            }
        } else {
            iFrameType = EVideoFrameType::videoFrameTypeP;
        }

        if iFrameType == EVideoFrameType::videoFrameTypeP && bSkipFrameFlag {
            iFrameType = EVideoFrameType::videoFrameTypeSkip;
        } else if iFrameType == EVideoFrameType::videoFrameTypeIDR {
            (*pParamInternal).iCodingIndex = 0;
            pEncCtx.bCurFrameMarkedAsSceneLtr = true;
        }
    } else {
        let pVaa = pEncCtx.vaa();
        let vaa_idr = pVaa.is_some_and(|v| v.bIdrPeriodFlag);

        if !kbSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < kiSpatialLayerNum)
            || ((*pParamInternal).iFrameIndex < (VGOP_SIZE << 1))
        {
            bSceneChangeFlag = false;
        } else if let Some(pVaa) = pVaa {
            bSceneChangeFlag = pVaa.bSceneChangeFlag;
        }

        iFrameType = if vaa_idr || bSceneChangeFlag || (*pParamInternal).bEncCurFrmAsIdrFlag {
            EVideoFrameType::videoFrameTypeIDR
        } else {
            EVideoFrameType::videoFrameTypeP
        };

        if iFrameType == EVideoFrameType::videoFrameTypeP && bSkipFrameFlag {
            iFrameType = EVideoFrameType::videoFrameTypeSkip;
        } else if iFrameType == EVideoFrameType::videoFrameTypeIDR {
            (*pParamInternal).iCodingIndex = 0;
        }
    }

    iFrameType
}

/// Zeroes `iSize` bytes at `pDst`.
///
/// **Was a three-slot dispatch** (`pfSetMemZeroSize8`, `pfSetMemZeroSize64`,
/// `pfSetMemZeroSize64Aligned16`, type `PSetMemoryZero = fn(*mut c_void, i32)`),
/// installed with this one `_c` body and nothing else on any target the port
/// builds for — the C++ installs SIMD variants there. The slots, the fn type and
/// the `extern "C"` thunk are deleted (S18, Phase 6 session B); the seven call
/// sites call this directly, and each already had this exact fallback in its
/// `else` arm.
///
/// # Safety
/// `pDst` must point to valid writable memory of at least `iSize` bytes.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsSetMemZero_c(pDst: *mut u8, iSize: i32) {
    if !pDst.is_null() && iSize > 0 {
        std::ptr::write_bytes(pDst, 0, iSize as usize);
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// **S40, for the arena `SStrideTables` now owns: the accessor is asked twice
    /// and the first cursor is used after the second call.**
    ///
    /// The four accessors hand out raw cursors their callers keep — `AllocStrideTables`
    /// carves the block with four of them live at once, and `svc_encode_mb.rs` holds
    /// two per macroblock — so the spelling has to be retag-stable. It is
    /// `PaddedPlane::root_ptr`'s: read the address out of the `Vec`'s header, never
    /// through a `&mut [i32]` over the block. F63 is what the other spelling costs.
    ///
    /// Red-proofed at the commit that added it: spelling `root` as
    /// `self.base.as_mut_slice().as_mut_ptr()` fails this test under Miri with
    /// "attempting a write access using <tag> but that tag does not exist in the
    /// borrow stack" at the first write below, and passes without Miri.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn stride_table_accessors_leave_the_first_cursor_usable() {
        // Two 96-byte block-offset regions, then two 64-byte coordinate ones — the
        // shape `AllocStrideTables` carves, at its smallest.
        let mut tab = SStrideTables::new(96 * 2 + 64 * 2);
        tab.pStrideDecBlockOffset[0][1] = Some(0);
        // The **shared region**: a layer absent from the temporal map is given the
        // matching layer's table, and two layers name one address.
        tab.pStrideDecBlockOffset[1][1] = Some(0);
        tab.pStrideEncBlockOffset[0] = Some(96);
        tab.pMbIndexX[0] = Some(96 * 2);
        tab.pMbIndexY[0] = Some(96 * 2 + 64);

        // **T9.C4 split this test's two halves apart, and the split is the point.**
        // The write path is `root()`; the read path is the four `&self` accessors.
        // They must not be interleaved on one `Vec` — `as_ptr`'s contract forbids
        // writing through what it returns — so the writes go first, in one `&mut`
        // stretch that holds four sibling cursors at once, which is exactly what
        // `AllocStrideTables` does.
        let (dec, enc, x, y) = unsafe {
            let dec = tab.root().add(0).cast::<i32>();
            let enc = tab.root().add(96).cast::<i32>();
            let x = tab.root().add(96 * 2).cast::<i16>();
            let y = tab.root().add(96 * 2 + 64).cast::<i16>();
            // The use that matters: the FIRST cursor, after three more derivations.
            *dec = 0x3C3C;
            *enc = 7;
            *x = 3;
            *y = 4;
            (dec, enc, x, y)
        };
        assert_eq!(unsafe { (*dec, *enc, *x, *y) }, (0x3C3C, 7, 3, 4));

        // And the read accessors resolve to those same four addresses, twice each.
        let first = tab.StrideDecBlockOffset(0, 1);
        let second = tab.StrideDecBlockOffset(0, 1);
        assert_eq!(first, second, "the same table resolves to the same address");
        assert_eq!(first, dec.cast_const(), "the read path names what the write path wrote");
        assert_eq!(unsafe { *first }, 0x3C3C, "and reads it back");
        assert_eq!(tab.StrideEncBlockOffset(0), enc.cast_const());
        assert_eq!(tab.MbIndexX(0), x.cast_const());
        assert_eq!(tab.MbIndexY(0), y.cast_const());

        assert_eq!(tab.StrideDecBlockOffset(1, 1), first, "two layers, one region");
        assert!(tab.MbIndexX(3).is_null(), "None answers the null the field used to hold");
        assert!(tab.StrideEncBlockOffset(3).is_null());
    }

    /// **S40 for the whole family, so that "every new root accessor has the test" is
    /// a fact rather than an argument from shared spelling.**
    ///
    /// `frame_bs_cursors_are_siblings` and the two stride-table tests below cover the
    /// two accessors whose cursors are demonstrably *held* across later derivations.
    /// The other fifteen this session added share their spelling — `&mut` over the
    /// container field, then the address out of the container's own header — and
    /// sharing a spelling is exactly the assumption S40 says an accessor may not
    /// survive on. So each of them is asked the same question here: derive twice,
    /// then write through the **first** cursor and read it back through the second.
    ///
    /// Red-proofed as a family: spelling `ctx_sps_array`'s root
    /// `arr.as_mut_slice().as_mut_ptr()` fails this test under Miri with "attempting
    /// a write access using <548144> ... but that tag does not exist in the borrow
    /// stack", and passes without Miri — the same failure every other arm has under
    /// the same substitution, since they share the spelling. It is one test rather
    /// than nineteen because the property is one property; each arm names its
    /// accessor in the assertion message so a failure says which.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn every_container_accessor_hands_out_sibling_cursors() {
        let mut ctx = Box::new(sWelsEncCtx::new());

        // Everything the accessors resolve through, at its smallest live shape.
        ctx.pSpsArray = vec![crate::encoder::param_svc::SWelsSPS::ZERO; 2];
        ctx.pSubsetArray = vec![crate::encoder::param_svc::SSubsetSps::ZERO; 2];
        ctx.pPPSArray = vec![crate::encoder::param_svc::SWelsPPS::ZERO; 2];
        ctx.pDqIdcMap = vec![SDqIdc::default(); 2];
        ctx.pLtr = vec![SLTRState::default(); 2];
        ctx.pWelsSvcRc = (0..2).map(|_| SWelsSvcRc::default()).collect();
        ctx.pMvdCostTable = vec![0u16; 64];
        ctx.iMvdCostTableSize = 8;
        ctx.pVaa = Some(Box::new(SVAAFrameInfo::default()));
        ctx.pSvcParam = Some(Box::new(SWelsSvcCodingParam::default()));
        ctx.ppRefPicListExt = vec![Some(SRefList::new())];
        ctx.ppDqLayerList = vec![Some(Box::new(
            crate::encoder::svc_encode_slice::SDqLayer::default(),
        ))];

        let p: *mut sWelsEncCtx = &mut *ctx;

        // Each arm: derive, derive again, then use the FIRST cursor. The write goes
        // through cursor 1 and the read back through cursor 2, so a spelling that
        // pops the first tag fails on the write and one that pops the second fails
        // on the read.
        macro_rules! siblings {
            ($name:literal, $get:expr, $write:expr, $read:expr) => {{
                let first = unsafe { $get };
                let second = unsafe { $get };
                assert_eq!(first, second, concat!($name, ": same slot, same address"));
                unsafe { $write(first) };
                assert!(unsafe { $read(second) }, concat!($name, ": the first cursor is still live"));
            }};
        }

        // **T9.H12, amended by T9.H3: the `&mut`-taking, raw-returning middle
        // state is gone from this family.** When this comment was written,
        // `ctx_dq_idc_map`, `ctx_ltr` and `ctx_ltr_at` took `&mut sWelsEncCtx`
        // and still answered raws, so their rows asserted that a cursor survives
        // a whole-context entry retag — F163's allocation argument (the `Vec`
        // buffers are separate allocations). H2 converted `ctx_dq_idc_map` to a
        // real slice and H3 converted `ctx_ltr_at` to a real reference (deleting
        // `ctx_ltr` outright), so those rows are gone the way `ctx_dq_idc_map`'s
        // went. The accessors below are in-fork and stay raw under S63.
        // The three parameter-set arrays had rows here until A4. They are slices
        // now (`sps_array`, `subset_array`, `pps_array`), so they join the
        // references below: no sibling property to assert, the borrow checker
        // referees the coexistences Miri used to.
        // `ctx_dq_idc_map` had a row here until T9.H2 step 4. It returns
        // `&mut [SDqIdc]` now, so it has no sibling property to assert — two
        // derivations cannot coexist, and the borrow checker says so at the call
        // rather than Miri saying so at run time. That is the trade the conversion
        // makes and it is the direction the phase is going.
        // `ctx_ltr` and `ctx_ltr_at` had rows here until T9.H3. The root is
        // deleted; the layer accessor returns `&mut SLTRState` now, so — as with
        // `ctx_dq_idc_map` above — it has no sibling property to assert: two
        // derivations cannot coexist, and the borrow checker says so at the call.
        // `ctx_rc` and `ctx_mvd_cost_table` had rows here until A1 of the
        // safe-conversion plan, and `ctx_rc_at` until A2. All three return
        // references now (`rc`, `mvd_cost_table`, `rc_at`/`rc_at_mut`), so — as
        // with `ctx_dq_idc_map` and `ctx_ltr_at` above — there is no sibling
        // property left to assert: two derivations cannot coexist and the borrow
        // checker says so at the call rather than Miri saying so at run time.
        // The whole rate-control branch of this family is references now.
        // `mvd_cost_origin` had a row here until S5.C4b, and it leaves for the
        // reason `ctx_param` and `ctx_ref_list` did: the accessor is gone, and its
        // successor (`svc_encode_slice::ctx_mvd_cost_origin`) answers with a
        // `MvdCostCursor` rather than a `*mut u16`. There is no raw sibling left to
        // assert stability of — the far end this row existed to serve,
        // `SWelsMD::pMvdCost`, is a borrow now, so the coexistence it measured is
        // the borrow checker's to referee.
        // `ctx_vaa` had a row here until A5. `vaa`/`vaa_mut` are references now,
        // refereed by the borrow checker like the rows above, and what stays raw —
        // `vaa_ptr`, the root `pfSetScrollingMv` needs and `vaa_ext` casts — is
        // derived through `&self`, so the row's write half would be UB rather
        // than a weaker assertion. Its read half survives in the interleaved
        // `held` list below, which takes a `vaa_ptr` cursor alongside every other
        // and reads through it after all of them exist.
        // `ctx_param` had a row here until A7. `param`/`param_mut` are references
        // now, refereed by the borrow checker like every other row this list has
        // lost: two derivations cannot coexist, and the compiler says so at the
        // call rather than Miri saying so at run time.
        // `ctx_ref_list` had a row here until A3 — `ref_list` returns
        // `Option<&SRefList>` now, so it joins the references above: no sibling
        // property to assert, and the borrow checker referees the coexistences
        // that Miri used to.
        siblings!("ctx_dq_layer", ctx_dq_layer(&*p, 0),
            |q: *mut crate::encoder::svc_encode_slice::SDqLayer| (*q).iMbWidth = 11,
            |q: *mut crate::encoder::svc_encode_slice::SDqLayer| (*q).iMbWidth == 11);

        // The rate controller's own five hung off `ctx_rc_at` here, one level
        // down — the only accessors in the family that reached through another
        // accessor. Four are retired and the fifth is a slice, so what is left is
        // the fixture the peers above still need.
        unsafe {
            let rc = (*p).rc_at_mut(0);
            rc.iGomSize = 4;
            crate::encoder::rc::RcInitLayerMemory(rc, 2);
        }
        // T9.X: `rc_temporal_over` and `rc_gom_complexity` were retired (S18 — the
        // first onto direct `Vec` indexing at its ten callers, the second for having
        // no production caller at all). **A1 retires the last two the same way.**
        // `rc_gom_fg_blocks` had no production caller either — this test was its
        // only one, which is S18's own criterion — and `rc_gom_sad` is
        // `SWelsSvcRc::gom_sad`, a slice, at both of its in-fork readers.

        // And the whole set once more, interleaved: every cursor taken first, then
        // every one used — which is the frame loop's actual shape, and the case a
        // per-accessor test cannot reach.
        let held: Vec<*mut u8> = unsafe {
            vec![
                // `ctx_dq_idc_map` left this list at T9.H2 step 4, and `ctx_ltr`
                // at T9.H3 (deleted with the raw family) — a real reference
                // cannot be *held* alongside the others, which is the point.
                // `mvd_cost_origin` left at S5.C4b for exactly that reason: its
                // successor answers with a `MvdCostCursor`, and a borrow is not
                // something this list can hold beside three raw cursors.
                (*p).vaa_ptr().cast(),
                ctx_dq_layer(&*p, 0).cast(), (*p).frame_bs().cast(),
            ]
        };
        // `frame_bs` is null here (no bitstream in this fixture), which is itself
        // the assertion that empty still answers null after everything above. It is
        // the **last** entry, and the two counts below are derived from the vector
        // rather than written twice — the literal `10`/`take(10)` pair went stale the
        // moment `ctx_dq_idc_map` left this list at T9.H2 step 4, and an index
        // computed from `held.len()` cannot.
        let last = held.len() - 1;
        assert!(held[last].is_null(), "no frame bitstream was installed");
        for (i, q) in held.iter().enumerate().take(last) {
            assert!(!q.is_null(), "held cursor {i} went null");
            unsafe { assert_eq!(*q.cast::<u8>(), *q.cast::<u8>()) };
        }
    }

    /// **S40 for the frame bitstream — the one buffer whose cursors are *stored*.**
    ///
    /// `SLayerBSInfo::pBsBuf` keeps a cursor into `pFrameBs` for the life of a
    /// layer's bitstream info, while the NAL writers keep deriving more from the same
    /// buffer at `iPosBsBuffer`: nineteen derivations across the tree, sixteen of
    /// them walking. So this is the accessor where the retag-stable spelling is not
    /// a precaution but the whole conversion — the first cursor is live across every
    /// later one by construction, not by accident.
    ///
    /// Red-proofed with `ctx_frame_bs` spelled `buf.as_mut_slice().as_mut_ptr()`:
    /// under Miri the read back through `stored` fails with "attempting a read
    /// access using <565587> ... but that tag does not exist in the borrow stack",
    /// and without Miri it passes.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn frame_bs_cursors_are_siblings() {
        let mut ctx = Box::new(sWelsEncCtx::new());
        let p: *mut sWelsEncCtx = &mut *ctx;
        // Before `RequestMemorySvc`, both answer the null the raw field held.
        assert!(unsafe { (*p).frame_bs() }.is_null());
        assert!(unsafe { (*p).frame_bs_cur() }.is_null());

        ctx.pFrameBs = vec![0u8; 64];
        ctx.iFrameBsSize = 64;
        let p: *mut sWelsEncCtx = &mut *ctx;

        // `pBsBuf` — the root, stored and kept, exactly as the three sites that take
        // it do.
        let stored = unsafe { (*p).frame_bs() };

        // The frame loop then walks: derive at the cursor, write, advance, repeat.
        // T9.G5: the walk drives `iPosBsBuffer`, because that is what the accessor
        // reads now and what the production loop has always advanced. The position
        // is set through `p` rather than through `ctx`, so the one raw binding above
        // stays live across the whole walk — which is the sibling property this test
        // exists for, and one fewer mint besides.
        for i in 0..8i32 {
            unsafe {
                (*p).iPosBsBuffer = i;
                *(*p).frame_bs_cur() = 0xA0 | i as u8;
            }
        }
        // The use that matters: the FIRST cursor, after eight later derivations.
        unsafe {
            assert_eq!(*stored, 0xA0, "the stored pBsBuf still reaches the buffer");
            *stored.add(8) = 0x5A;
            (*p).iPosBsBuffer = 8;
            assert_eq!(*(*p).frame_bs_cur(), 0x5A);
        }

        // And the whole buffer reads back through the container, which is the point
        // of owning it: no third party is needed to free or bound it.
        assert_eq!(&ctx.pFrameBs[..4], &[0xA0, 0xA1, 0xA2, 0xA3]);
        assert_eq!(ctx.pFrameBs.len(), ctx.iFrameBsSize as usize);
    }

    /// The same property through the context's four accessors, which is how every
    /// consumer outside `AllocStrideTables` reaches the tables.
    use crate::encoder::wels_preprocess::CWelsPreProcess;

    // **F192's two probes stood here and are retired by their own fix (T9.H2).**
    //
    // They minted F167's shape — an owner `Box<sWelsEncCtx>`, the root raw via
    // `addr_of_mut!`, `CWelsPreProcess::m_pEncCtx` set the way `CreatePreProcess`
    // set it, the object reached through `pCtx.pVpp`, and a driver taking
    // `&mut sWelsEncCtx` — and Miri refused the `&mut` form in one line while
    // passing the shared one. The trace is quoted in F192.
    //
    // **`m_pEncCtx` no longer exists**, so neither probe has a subject: nothing in
    // this encoder stores a second route to the context any more, and a test cannot
    // exercise a field that is not declared. Deleting them with the field is the
    // honest bookkeeping — a test kept alive past its subject is a claim of coverage
    // nobody is providing. What guards the shape from here is that there is nothing
    // left to guard: reintroducing it means declaring a new stored context pointer,
    // which is a design change and not an accident.

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn ctx_stride_accessors_are_sibling_derivations() {
        let mut ctx = Box::new(sWelsEncCtx::new());
        let p: *mut sWelsEncCtx = &mut *ctx;
        // Before `AllocStrideTables` runs, all four answer null — the value the raw
        // `pStrideTab` held, and the question every `is_null()` guard was written to ask.
        assert!(unsafe { ctx_stride_enc_block_offset(&*p, 0) }.is_null());
        assert!(unsafe { ctx_stride_dec_block_offset(&*p, 0, 1) }.is_null());
        assert!(unsafe { ctx_mb_index_x(&*p, 0) }.is_null());
        assert!(unsafe { ctx_mb_index_y(&*p, 0) }.is_null());

        let mut tab = SStrideTables::new(96 * 2);
        tab.pStrideEncBlockOffset[0] = Some(0);
        tab.pStrideEncBlockOffset[1] = Some(96);
        ctx.pStrideTab = Some(Box::new(tab));

        let p: *mut sWelsEncCtx = &mut *ctx;
        // The write path, once (T9.C4): `root()` under the one `&mut` the init-time
        // filler takes.
        unsafe {
            let tab = (*p).pStrideTab.as_mut().unwrap();
            *tab.root().add(0).cast::<i32>() = 11;
            *tab.root().add(96).cast::<i32>() = 22;
        }
        // The read path, three times, two of them naming the same region — the
        // property that matters is that the first answer is still usable after the
        // third call, which is what every in-fork consumer relies on.
        let first = unsafe { ctx_stride_enc_block_offset(&*p, 0) };
        let other = unsafe { ctx_stride_enc_block_offset(&*p, 1) };
        let again = unsafe { ctx_stride_enc_block_offset(&*p, 0) };
        assert_eq!(first, again);
        assert_eq!(unsafe { (*again, *first, *other) }, (11, 11, 22));
    }

    #[test]
    fn test_calc_bi_stride() {
        assert_eq!(CALC_BI_STRIDE(640, 24), 1920);
        assert_eq!(CALC_BI_STRIDE(1920, 16), 3840);
        assert_eq!(CALC_BI_STRIDE(1920, 24), 5760);
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_init_pic() {
        let mut src_pic = SSourcePicture::default();
        let ret = unsafe {
            InitPic(
                &mut src_pic,
                VideoFormat::videoFormatI420 as i32,
                640,
                480,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(src_pic.iPicWidth, 640);
        assert_eq!(src_pic.iPicHeight, 480);
        assert_eq!(src_pic.iStride[0], 640);
        assert_eq!(src_pic.iStride[1], 320);
        assert_eq!(src_pic.iStride[2], 320);
    }


    /// **The temporary instrument for T6.G1**: `sWelsEncCtx::new()` reproduces the
    /// zeroed shell it replaces, byte for byte, and every difference is
    /// attributed to a *named field* before it is accepted.
    ///
    /// It is meant to read **zero differences**. A constructor whose whole claim is
    /// "same bytes, ruled instead of bet on" has to prove the first half, and the
    /// only proof that catches a field the author quietly gave a live starting value
    /// is a comparison against the thing being replaced. The freedom the constructor
    /// buys is spent in later steps — where fields change *type* and the shell stops
    /// being reproducible at all — and this test dies at the first such step rather
    /// than being weakened to keep passing.
    ///
    /// # Why the comparison is per field and not one `memcmp`
    ///
    /// A `#[repr(C)]` struct has padding, and a struct literal does not write it.
    /// A `zeroed()` shell does. So a flat `memcmp` over `size_of::<sWelsEncCtx>()`
    /// compares bytes the constructor is not obliged to define, reads uninitialized
    /// memory doing it (UB, and Miri says so), and reports differences that mean
    /// nothing. Walking the fields compares exactly the bytes that have values,
    /// names the field when they differ, and reports the residue separately so the
    /// coverage is visible rather than assumed.
    ///
    /// # F64 — the ten fields that cannot be compared as bytes at all
    ///
    /// The first draft *was* a field-extent walk and Miri failed it anyway, at
    /// `pEncPic`: **`None` writes the discriminant and leaves the payload
    /// undefined**, so `None` of an `Option<SrcPicId>` is four defined bytes (the
    /// `NonZeroU32` niche, zero) followed by four *uninitialized* ones (`pool::Id`'s
    /// generation counter, which exists only under `debug_assertions`). The shell
    /// wrote zeros there. `new()` does not. The same holds one level down, inside
    /// `SSpatialPicIndex::pSrc`; and **interior `repr(C)` padding does it with no
    /// `Option` involved at all** — `SParaSetOffsetVariable` has 3 bytes between
    /// `bUsedParaSetIdInBs[57]` and `uiNextParaSetIdToUseInBs`, and
    /// `TagVideoEncoderStatistics` has 4 before `iStatisticsTs` and 4 of tail, none
    /// of which a struct literal writes.
    ///
    /// **And the *niche* is not what makes it happen** — that took a second reading,
    /// paid for by the `exit` battery. `iCurDqLayer`, `iSps` and `iPps` are
    /// `Option`s over plain integer newtypes with **no** niche: a tag byte plus a
    /// payload. `None` writes the tag and leaves the payload undefined exactly as the
    /// handles do. The first cut of this list said "niche-carrying" and excluded only
    /// the handles, which is the same mistake one level down: it is not the niche,
    /// it is that **an `Option`'s `None` defines only its discriminant**.
    ///
    /// So the honest statement is narrower than "byte-identical", and it is the
    /// narrower one that is true: `new()` reproduces the shell **everywhere the
    /// shell's bytes are defined by the type**, and at the ten fields below it
    /// reproduces the *values*, which is all anything reads. Nothing reads a `None`'s
    /// payload — that is read when a `Some` is unwrapped, and there is no `Some`
    /// here — and nothing reads padding at all. The ten are excluded **by name** and
    /// asserted **by value**; excluding them silently would have been the failure
    /// this test exists to prevent, one level up.
    ///
    /// The general rule this leaves behind: *a field-wise constructor cannot be
    /// proved byte-equal to a memset image, only value-equal, and the difference is
    /// exactly the bytes the type does not define.* In practice, for this port: every
    /// `Option` field, and all padding.
    ///
    /// **A field added to this struct as an `Option` belongs on the `BY_VALUE` list**,
    /// and the test will say so under Miri if it is not — which is how each of these
    /// four rounds was found.
    ///
    /// # T6.H2: the shell stopped being a value, and the test grew a third tier
    ///
    /// Session H makes members of this struct **own** their memory, and the first
    /// `Vec` field ends the sentence "`new()` reproduces the memset image" as
    /// literally as it was written — because the memset image of a `Vec` is a null
    /// `Unique`, which is **not a `Vec`**. `mem::zeroed::<sWelsEncCtx>()` was itself
    /// undefined behaviour the moment `pSpsArray` changed type; it kept compiling and
    /// kept passing, which is exactly the failure mode this test exists to catch, one
    /// level up again. Measured, not argued — the old line under Miri reads:
    ///
    /// ```text
    /// error: Undefined Behavior: constructing invalid value of type sWelsEncCtx:
    ///   at .pSpsArray.buf.inner.ptr.pointer.pointer, encountered 0,
    ///   but expected something greater or equal to 1
    /// ```
    ///
    /// So the shell is held as **raw bytes** now — `MaybeUninit::zeroed`, never
    /// `assume_init`ed — and there are three tiers:
    ///
    /// * **tier 1**, byte for byte: every field whose bytes are fully defined in both.
    /// * **tier 2**, by value: the F64 fields, where the *shell* value is recovered
    ///   by `ptr::read` out of the zero image (sound precisely because their all-zero
    ///   bit pattern **is** a value of their type — an `Option`'s `None`, a zeroed
    ///   POD) and only `new()`'s undefined bytes are the problem.
    /// * **tier 3**, `OWNED`: the containers, where the zero image is not a value at
    ///   all, so there is nothing to recover and nothing to compare. What is asserted
    ///   is that `new()` builds the **empty** container — which is what the null the
    ///   raw pointer held meant, and what every consumer's `is_null()` still reads
    ///   through the root accessors.
    ///
    /// Tier 3 is the one that shrinks this test's reach, so it is named and counted
    /// in the output rather than left to be inferred from what is missing.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn ctx_new_reproduces_the_zeroed_shell() {
        use std::mem::{offset_of, size_of, size_of_val};

        let built = Box::new(sWelsEncCtx::new());
        // The memset image, as bytes. **Not** a zeroed *value* of the type: see the
        // header — three fields have no valid all-zero value, so materialising one
        // would be UB before the first comparison ran.
        let shell = Box::new(std::mem::MaybeUninit::<sWelsEncCtx>::zeroed());

        // (name, offset, size) for every field, taken off a real instance so the
        // sizes are the compiler's and not a transcription.
        macro_rules! extents {
            ($($f:ident),* $(,)?) => {
                vec![$((stringify!($f), offset_of!(sWelsEncCtx, $f), size_of_val(&built.$f))),*]
            };
        }
        let extents: Vec<(&str, usize, usize)> = extents![
        sLogCtx, pSvcParam, iMvRange, pMvdCostTable,
        iMvdCostTableSize, iMvdCostTableStride, pStrideTab, pFuncList,
        pSliceThreading, eRefStrategy, pEncPic,
        pDecPic, pRefPic, iCurDqLayer, ppDqLayerList,
        ppRefPicListExt, pRefList0, pLtr, bCurFrameMarkedAsSceneLtr,
        eSliceType, eNalType, eNalPriority, eLastNalPriority,
        iNumRef0, uiDependencyId, uiTemporalId, bNeedPrefixNalFlag,
        pWelsSvcRc, bCheckWindowStatusRefreshFlag, iCheckWindowStartTs, iCheckWindowCurrentTs,
        iCheckWindowInterval, iCheckWindowIntervalShift, bCheckWindowShiftResetFlag, iGlobalQp,
        pVaa, pVpp, pSpsArray, iSps,
        pPPSArray, iPps, pSubsetArray, iSpsNum,
        iSubsetSpsNum, iPpsNum, pOut,
        pFrameBs, iFrameBsSize, iPosBsBuffer, sSpatialIndexMap,
        iSliceBufferSize, bRefOfCurTidIsLtr, iMaxSliceCount, iActiveThreadsNum,
        pDqIdcMap, sPSOVector,
        uiStartTimestamp, sEncoderStatistics, iStatisticsLogInterval, iLastStatisticsLogTs,
        iEncoderError, bDeliveryFlag, sWelsCabacContexts,
        uiLastTimestamp, pDynamicBsBuffer,
        ];
        // 68 until T7.B4 took `pTaskManage` and `mutexEncoderError` out with the pool,
        // 66 until T7.C6 took `pMemAlign`.
        assert_eq!(extents.len(), 65, "a field was added or removed without updating this list");

        let b = shell.as_ptr().cast::<u8>();

        // One field of the memset image, read back as a value. Sound only for fields
        // whose all-zero bit pattern is a value of their type, which is every field
        // below and no field on `OWNED`.
        macro_rules! shell_field {
            ($f:ident) => {
                // SAFETY: `b` is `size_of::<sWelsEncCtx>()` zero bytes with the
                // struct's alignment, and the read is inside `$f`'s extent.
                unsafe { std::ptr::read(b.add(offset_of!(sWelsEncCtx, $f)).cast()) }
            };
        }

        // ---- tier 3: the owned containers -------------------------------------
        // The zeroed shell has no image of these. `RequestMemorySvc` used to
        // `WelsMallocz` each of them and `FreeMemorySvc` to free it; `new()` builds
        // the empty container, which is the null the raw pointer held, and which
        // `ctx_sps_array` and its siblings answer as null so that every downstream
        // `is_null()` guard still asks its question.
        const OWNED: [&str; 12] = [
            "pSpsArray", "pSubsetArray", "pPPSArray", "pDqIdcMap", "pFrameBs", "pLtr",
            "pWelsSvcRc", "ppRefPicListExt", "ppDqLayerList", "pMvdCostTable",
            // **T7.C5.** `pDynamicBsBuffer` joined this tier when it became
            // `[Vec<u8>; MAX_THREADS_NUM]`. It is the only member here that is an
            // *array* of owned containers, so the claim below is per element: four
            // empty `Vec`s, which is what the four null pointers held and what
            // `RequestMemorySvc` still leaves when the encoder is not built for
            // dynamic slicing under CABAC.
            "pDynamicBsBuffer",
            // **T6.I1.** `pFuncList` joined this tier when it became a `Box`. It is
            // the one owned field whose empty state is not "no elements" — a `Box`
            // is always inhabited — so what tier 3 asserts for it is the *content*:
            // the table `new()` builds is the uninstalled table, which is the image
            // `WelsMallocz` used to hand back. That is the whole behavioural claim
            // of T6.I1, and it is asserted below rather than assumed.
            "pFuncList",
        ];
        // `pVaa` is `Option<Box<_>>`: its `None` is the null pointer and defines all
        // eight of its bytes, so it stays in tier 1 with `pStrideTab`.
        assert!(built.pSpsArray.is_empty(), "new(): no SPS array is allocated yet");
        assert!(built.pSubsetArray.is_empty(), "new(): no subset SPS array is allocated yet");
        assert!(built.pPPSArray.is_empty(), "new(): no PPS array is allocated yet");
        assert!(built.pDqIdcMap.is_empty(), "new(): no dq-idc map is allocated yet");
        assert!(built.pFrameBs.is_empty(), "new(): no frame bitstream is allocated yet");
        assert!(built.pLtr.is_empty(), "new(): no LTR state array is allocated yet");
        assert!(built.pWelsSvcRc.is_empty(), "new(): no rate-control state is allocated yet");
        assert!(built.ppRefPicListExt.is_empty(), "new(): no reference lists are allocated yet");
        assert!(built.ppDqLayerList.is_empty(), "new(): no DQ layers are allocated yet");
        assert!(built.pMvdCostTable.is_empty(), "new(): no MVD cost table is allocated yet");
        assert!(
            built.pDynamicBsBuffer.iter().all(Vec::is_empty),
            "new(): no dynamic-slice CABAC restore buffers are allocated yet"
        );

        // `pFuncList`: not "empty" — uninstalled. One assertion per *kind* of member
        // the table has, which is what makes this a statement about the whole struct
        // rather than about the fields that happened to get written down: a leading
        // and a trailing plain slot, each of the three predictor arrays, the two
        // embedded POD sub-tables, both enum discriminants, and the owned box.
        // Field for field the claim is `SWelsFuncPtrList::default()`'s own
        // definition; the three `init_fills_*` tests pin what gets written on top.
        let fl = &*built.pFuncList;
        assert!(fl.pfFillInterNeighborCache.is_none(), "new(): the table is uninstalled");
        assert!(fl.pfCavlcParamCal.is_none(), "new(): the table is uninstalled");
        assert!(fl.pfGetLumaI16x16Pred.iter().all(Option::is_none), "new(): no I16x16 predictors");
        assert!(fl.pfGetLumaI4x4Pred.iter().all(Option::is_none), "new(): no I4x4 predictors");
        assert!(fl.pfGetChromaPred.iter().all(Option::is_none), "new(): no chroma predictors");
        assert!(fl.pfMotionSearch.iter().all(Option::is_none), "new(): no motion search");
        assert!(fl.sMeFuncs.pfSearchMethod.iter().all(Option::is_none), "new(): no search method");
        assert!(fl.sMcFuncs.pMcLumaFunc.is_none(), "new(): no motion compensation");
        assert!(
            fl.sSampleDealingFuncs.pfSampleSad.iter().all(Option::is_none)
                && fl.sSampleDealingFuncs.pfMdCost == crate::encoder::md::CostFamily::Unset
                && fl.sSampleDealingFuncs.pfMeCost == crate::encoder::md::CostFamily::Unset,
            "new(): no sample-dealing kernels, and neither cost family is selected"
        );
        assert!(
            fl.pfDeblocking.pfDeblockingFilterSlice.is_none(),
            "new(): no deblocking kernels"
        );
        // The two discriminants whose zero *is* a declared variant — which is what
        // made the old memset-image constructor sound, and is now stated by
        // `Default` writing the variant out.
        assert_eq!(fl.eEntropyCoder, EntropyCoder::Cavlc, "new(): the memset's entropy coder");
        assert_eq!(
            fl.pfRc.eInstalledMode,
            crate::api::codec_api::RC_MODES::RC_QUALITY_MODE,
            "new(): the memset's rate-control mode"
        );
        assert!(fl.pParametersetStrategy.is_none(), "new(): no paraset strategy is installed yet");

        // ---- tier 2: the F64 fields, excluded by name and asserted by value ----
        const BY_VALUE: [&str; 10] = [
            // `Option` with a niche: `None` leaves pool::Id's generation half undefined
            "pEncPic", "pDecPic", "pRefPic", "pRefList0", "sSpatialIndexMap",
            // `Option` without one: `None` writes the tag and leaves the payload byte
            "iCurDqLayer", "iSps", "iPps",
            // interior repr(C) padding a struct literal does not write
            "sPSOVector", "sEncoderStatistics",
        ];

        let paraset_is_zero = |p: &SParaSetOffset| {
            p.sParaSetOffsetVariable.iter().all(|v| {
                v.iParaSetIdDelta.iter().all(|&d| d == 0)
                    && v.bUsedParaSetIdInBs.iter().all(|&b| !b)
                    && v.uiNextParaSetIdToUseInBs == 0
            }) && p.bPpsIdMappingIntoSubsetsps.iter().all(|&b| !b)
                && p.iPpsIdList.iter().all(|r| r.iter().all(|&i| i == 0))
                && (p.uiNeededSpsNum, p.uiNeededSubsetSpsNum, p.uiNeededPpsNum) == (0, 0, 0)
                && (p.uiInUseSpsNum, p.uiInUseSubsetSpsNum, p.uiInUsePpsNum) == (0, 0, 0)
        };
        let stats_are_zero = |s: &crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics| {
            (s.uiWidth, s.uiHeight, s.uiBitRate, s.uiAverageFrameQP) == (0, 0, 0, 0)
                && (s.fAverageFrameSpeedInMs, s.fAverageFrameRate, s.fLatestFrameRate)
                    == (0.0, 0.0, 0.0)
                && (s.uiInputFrameCount, s.uiSkippedFrameCount, s.uiResolutionChangeTimes) == (0, 0, 0)
                && (s.uiIDRReqNum, s.uiIDRSentNum, s.uiLTRSentNum) == (0, 0, 0)
                && (s.iStatisticsTs, s.iTotalEncodedBytes) == (0, 0)
                && (s.iLastStatisticsBytes, s.iLastStatisticsFrameCount) == (0, 0)
        };

        // The same ten fields from both images: `new()`'s by field access, the
        // shell's by reading its zero bytes back as a value.
        let pairs: [(&str, bool, bool); 10] = [
            ("pEncPic", built.pEncPic.is_none(), {
                let v: Option<SrcPicId> = shell_field!(pEncPic);
                v.is_none()
            }),
            ("pDecPic", built.pDecPic.is_none(), {
                let v: Option<RecPicId> = shell_field!(pDecPic);
                v.is_none()
            }),
            ("pRefPic", built.pRefPic.is_none(), {
                let v: Option<RecPicId> = shell_field!(pRefPic);
                v.is_none()
            }),
            ("pRefList0", built.pRefList0.iter().all(|h| h.is_none()), {
                let v: [Option<RecPicId>; 16] = shell_field!(pRefList0);
                v.iter().all(|h| h.is_none())
            }),
            (
                "sSpatialIndexMap",
                built.sSpatialIndexMap.iter().all(|e| e.pSrc.is_none() && e.iDid == 0),
                {
                    let v: [SSpatialPicIndex; MAX_DEPENDENCY_LAYER] =
                        shell_field!(sSpatialIndexMap);
                    v.iter().all(|e| e.pSrc.is_none() && e.iDid == 0)
                },
            ),
            ("iCurDqLayer", built.iCurDqLayer.is_none(), {
                let v: Option<crate::encoder::svc_encode_slice::LayerIdx> =
                    shell_field!(iCurDqLayer);
                v.is_none()
            }),
            ("iSps", built.iSps.is_none(), {
                let v: Option<SpsId> = shell_field!(iSps);
                v.is_none()
            }),
            ("iPps", built.iPps.is_none(), {
                let v: Option<PpsId> = shell_field!(iPps);
                v.is_none()
            }),
            ("sPSOVector", paraset_is_zero(&built.sPSOVector), {
                let v: SParaSetOffset = shell_field!(sPSOVector);
                paraset_is_zero(&v)
            }),
            (
                "sEncoderStatistics",
                built.sEncoderStatistics.iter().all(stats_are_zero),
                {
                    let v: [crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics;
                        MAX_DEPENDENCY_LAYER] = shell_field!(sEncoderStatistics);
                    v.iter().all(stats_are_zero)
                },
            ),
        ];
        for (name, in_new, in_shell) in pairs {
            assert!(in_new, "new(): {name} is not the value the memset image holds");
            assert!(in_shell, "shell: {name} is not what this test claims it is");
        }

        let a = (&*built as *const sWelsEncCtx).cast::<u8>();

        // ---- tier 1: everything else, byte for byte, attributed by name -------
        let (mut compared, mut excluded, mut owned) = (0usize, 0usize, 0usize);
        let mut diffs: Vec<String> = Vec::new();
        for (name, off, len) in &extents {
            if OWNED.contains(name) {
                owned += len;
                continue;
            }
            if BY_VALUE.contains(name) {
                excluded += len;
                continue;
            }
            compared += len;
            for k in 0..*len {
                // SAFETY: `off + k` is inside `name`'s extent, and every byte of a
                // field outside `BY_VALUE` is defined by its type in both images —
                // scalars, pointers, `repr(C)` enums, and arrays of those, none of
                // which have a niche or interior padding.
                let (x, y) = unsafe { (*a.add(off + k), *b.add(off + k)) };
                if x != y {
                    diffs.push(format!(
                        "{name} (offset {off}, +{k}): new()=0x{x:02x} shell=0x{y:02x}"
                    ));
                    break; // one line per field is enough to name it
                }
            }
        }

        assert!(
            diffs.is_empty(),
            "sWelsEncCtx::new() is not the zeroed shell — {} field(s) differ:\n  {}",
            diffs.len(),
            diffs.join("\n  ")
        );

        // Coverage, so "zero differences" cannot be true by comparing nothing. The
        // rest is inter-field `repr(C)` padding plus the seven F64 fields; both are
        // reported rather than asserted at a number, because both move whenever a
        // field's width does — and every step after this one moves a field's width.
        let total = size_of::<sWelsEncCtx>();
        assert!(compared > 0 && compared + excluded + owned <= total);
        println!(
            "ctx_new_reproduces_the_zeroed_shell: {compared}/{total} bytes compared byte-wise \
             across {} fields, {excluded} in the {} F64 fields (compared by value), {owned} in \
             the {} owned fields (no zero image to compare against), {} of inter-field repr(C) \
             padding",
            extents.len() - BY_VALUE.len() - OWNED.len(),
            BY_VALUE.len(),
            OWNED.len(),
            total - compared - excluded - owned
        );
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_update_and_loadback_framenum() {
        let mut param = SWelsSvcCodingParam::default();
        // Only the fields this test exercises; SWelsSPS is now the full
        // parameter_sets.h:43 struct rather than the four-field copy that used to
        // live in this module.
        let sps = SWelsSPS {
            uiLog2MaxFrameNum: 4,
            iLog2MaxPocLsb: 4,
            bFrameCroppingFlag: false,
            sFrameCrop: SCropOffset::default(),
            ..Default::default()
        };
        let mut ctx = sWelsEncCtx::new();
        // **T6.H11**: the context *owns* its parameters, so the fixture hands them
        // over rather than lending them — and the read-back below goes through the
        // context, which is where the writes land. Aliasing a stack local here was
        // the fixture standing in for an ownership the live path never had.
        ctx.pSvcParam = Some(Box::new(param));
        // T6.G3: the context names its SPS by position, so the test stands up the
        // one-entry array the position indexes into — `RequestMemorySvc`'s job on the
        // live path.
        ctx.pSpsArray = vec![sps];
        ctx.iSpsNum = 1;
        ctx.iSps = Some(SpsId(0));
        ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;

        let frame_num = |c: &sWelsEncCtx| c.pSvcParam.as_ref().unwrap().sDependencyLayers[0].iFrameNum;

        unsafe {
            UpdateFrameNum(&mut ctx, 0);
            assert_eq!(frame_num(&ctx), 1);
            assert_eq!(ctx.eLastNalPriority[0], EWelsNalRefIdc::NRI_PRI_LOWEST);

            ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;
            LoadBackFrameNum(&mut ctx, 0);
            assert_eq!(frame_num(&ctx), 0);
        }
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_decide_frame_type() {
        let mut param = SWelsSvcCodingParam::default();
        let mut ctx = sWelsEncCtx::new();
        param.sDependencyLayers[0].bEncCurFrmAsIdrFlag = true;
        ctx.pSvcParam = Some(Box::new(param.clone()));
        // T6.H10: the context owns the block, so the fixture hands it one.
        ctx.pVaa = Some(Box::new(SVAAFrameInfo::default()));

        unsafe {
            let ft = DecideFrameType(&mut ctx, 1, 0, false);
            assert_eq!(ft, EVideoFrameType::videoFrameTypeIDR);
        }
    }

    #[test]
    fn test_init_function_pointers() {
        // S5.E2b: every call this made is a safe `fn` now.
        let mut param = SWelsSvcCodingParam::default();
        let mut ctx = sWelsEncCtx::default();
        // T6.I1: the context brings its own table, so the fixture no longer
        // aims the field at a stack one — it reads the context's back out.
        ctx.pSvcParam = Some(Box::new(param.clone()));

        let ret = InitFunctionPointers(&mut ctx, 0);
        assert_eq!(ret, ENC_RETURN_SUCCESS);

        // Each assertion reads the table back through its owner rather than
        // binding a reference to it once. That is not stylistic: the
        // `InitCoeffFunc` call below *writes* the table, and a `&` held across
        // it is the exact shape this session's step-1 checker exists to reject.
        assert!(ctx.pFuncList.pfDctFourT4.is_some());
        // pfInterMd is deliberately NOT asserted: C++ InitFunctionPointers
        // (encoder.cpp) never sets it. It is assigned per-slice in
        // svc_encode_slice.cpp:733/736. This assertion passed only because the
        // port assigned the wrong function here behind a mem::transmute.
        // The four entropy slots this used to assert `is_some()` on are one
        // `EntropyCoder` since T4b.1, and "installed" is no longer a state it
        // can be in. What is still worth asserting is that the flag reached
        // it: `param` defaults to `iEntropyCodingModeFlag == 0`. The other
        // arm goes through `InitCoeffFunc` rather than a second
        // `InitFunctionPointers`, which would allocate a second parameter-set
        // strategy over the first.
        assert_eq!(ctx.pFuncList.eEntropyCoder, EntropyCoder::Cavlc);
        InitCoeffFunc(ctx.func_list_mut(), 0, 1);
        assert_eq!(ctx.pFuncList.eEntropyCoder, EntropyCoder::Cabac);

        assert!(ctx.pFuncList.pfDeblocking.pfDeblockingFilterSlice.is_some());
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_FMA, WELS_CPU_MMX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE, WELS_CPU_SSE2, WELS_CPU_SSE3, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3};








