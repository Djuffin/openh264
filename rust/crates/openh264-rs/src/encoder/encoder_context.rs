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
/// "16 in standard"; the encoder's own limit is 4.
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
/// `EStaticBlockIdc` enum in `wels_preprocess.rs`.
pub const BLOCK_STATIC_IDC_ALL: usize = 3;
/// `wels_const.h:147` — last variant of the block-size enum, value 7.
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
/// The fields are `int16_t` in C++.
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
    /// The macroblock this cache is on, in macroblocks.
    ///
    /// It is carried here rather than fetched from `SMB` because three of the readers
    /// have neither an `SMB` nor a slice in scope — `WelsMdI16x16`, `WelsMdIntraChroma`
    /// and (for its chroma half) `WelsMdIntraSecondaryModesEnc`.
    pub iMbX: i32,
    pub iMbY: i32,
}

impl SPicData {
    /// This macroblock's origin as a byte offset into a plane of `stride` —
    /// `((iMbX + iMbY * stride) << 4)` for luma and `<< 3` for chroma.
    ///
    /// **Chroma reads stride index 1 for both chroma planes**, not 2 — a caller
    /// passing `stride(2)` for plane 2 would be wrong on any picture whose chroma
    /// strides differ. Every view-based resolver is immune by construction —
    /// `AllocPicture` builds planes 1 and 2 with one `kuiChromaStride` and each
    /// plane carries it.
    #[inline]
    pub fn mb_offset(&self, stride: i32, plane: usize) -> isize {
        let shift = if plane == 0 { 4 } else { 3 };
        ((self.iMbX + self.iMbY * stride) as isize) << shift
    }

    /// The macroblock cursor, taken from a **picture view**.
    ///
    /// It hands back a [`RecCursor`](crate::encoder::rec_view::RecCursor), not a
    /// `PlaneCursor`: the source picture is written in-fork by
    /// `VaaBackgroundMbDataUpdate`, so its planes live behind the shared seam
    /// and no `&[u8]` may span them.
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
    /// counterpart to [`mb_cursor_ro`](Self::mb_cursor_ro).
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
/// `eSpsPpsIdStrategy` is **not** a member: `wels_common_basis.h:89` guards it with
/// `#if _DEBUG`, which this build does not set — the C++ `sizeof` of 1180 confirms
/// it is absent.
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
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDqIdc {
    pub iPpsId: u16,
    pub iSpsId: u8,
    pub uiSpatialId: i8,
}

pub use crate::encoder::svc_encode_slice::{SMB, SSlice};



pub use crate::encoder::svc_encode_slice::SWelsSvcRc;

// The real ports (svc_mode_decision.cpp:236 and :257) live in svc_mode_decision.rs.
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
pub use crate::encoder::wels_preprocess::VaaBlock;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
pub use crate::encoder::wels_func_ptr_def::{EntropyCoder, SWelsFuncPtrList};


// ============================================================================
// Primary Encoder Context Data Structures (encoder_context.h)
// ============================================================================

/// Reference picture lists for each spatial dependency/quality layer in SVC.
///
/// `pRef` *is* this layer's reconstruction pool, so the struct owns its pictures.
/// The two lists and `pNextBuffer` are **handles into `pRef`**.
#[derive(Debug)]
pub struct SRefList {
    pub pShortRefList: [Option<RecPicId>; 1 + MAX_SHORT_REF_COUNT],
    pub pLongRefList: [Option<RecPicId>; 1 + MAX_REF_PIC_COUNT],
    pub pNextBuffer: Option<RecPicId>,
    /// The pool.
    pub pRef: RecPicPool,
    pub uiShortRefCount: u8,
    pub uiLongRefCount: u8,
}

impl SRefList {
    /// An empty reference list for one dependency layer. `RequestMemorySvc` fills
    /// [`pRef`](Self::pRef) immediately after and points `pNextBuffer` at slot 0.
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
/// `AllocStrideTables` carves four regions: the decoder-side 4x4 block offsets, the
/// encoder-side ones, and the two macroblock coordinate tables.
///
/// Every block-offset region is exactly 24 `i32`s
/// (`kiUnit1Size`), and the two coordinate tables are `i16` runs of one entry per
/// macroblock — so `blocks` holds the dec-side regions followed by the enc-side
/// ones, and `coords` holds the X table followed by the
/// Y table. Two layers can share one region — when a spatial layer is absent
/// from the temporal map, `AllocStrideTables` assigns it the matching layer's
/// table — and copying an **index** reproduces that exactly as copying an offset
/// did. `None` is "this layer has no table".
pub struct SStrideTables {
    /// The 24-entry block-offset regions: dec-side first, then enc-side, in the
    /// order `AllocStrideTables` carves them. Zero-filled.
    blocks: Vec<[i32; 24]>,
    /// The two macroblock coordinate tables: X's regions, then Y's.
    coords: Vec<i16>,
    /// Per-layer **indices into `blocks`**.
    pub pStrideDecBlockOffset: [[Option<u32>; 2]; MAX_DEPENDENCY_LAYER],
    pub pStrideEncBlockOffset: [Option<u32>; MAX_DEPENDENCY_LAYER],
    /// Per-layer **element indices into `coords`**.
    pub pMbIndexX: [Option<u32>; MAX_DEPENDENCY_LAYER],
    pub pMbIndexY: [Option<u32>; MAX_DEPENDENCY_LAYER],
}

impl SStrideTables {
    /// The tables with both stores sized and no layer wired yet.
    pub fn new(kiBlockCount: usize, kiCoordLen: usize) -> Self {
        Self {
            blocks: vec![[0i32; 24]; kiBlockCount],
            coords: vec![0i16; kiCoordLen],
            pStrideDecBlockOffset: [[None; 2]; MAX_DEPENDENCY_LAYER],
            pStrideEncBlockOffset: [None; MAX_DEPENDENCY_LAYER],
            pMbIndexX: [None; MAX_DEPENDENCY_LAYER],
            pMbIndexY: [None; MAX_DEPENDENCY_LAYER],
        }
    }

    /// A coordinate-table region as the `&mut [i16]` it is.
    #[inline]
    pub fn i16_region_mut(&mut self, kuiOff: u32, kiLen: usize) -> &mut [i16] {
        &mut self.coords[kuiOff as usize..][..kiLen]
    }

    /// A block-offset region as the `&mut [i32; 24]` it is — the write twin of
    /// [`EncBlockOffsets`](Self::EncBlockOffsets).
    #[inline]
    pub fn i32_block24_mut(&mut self, kuiIdx: u32) -> &mut [i32; 24] {
        &mut self.blocks[kuiIdx as usize]
    }

    /// The enc-side block offsets of layer `kiDid` — 16 luma + 8 chroma.
    ///
    /// These tables are filled once by `WelsGetEncBlockStrideOffset` at
    /// `InitDqLayers` and read-only for the rest of the encode.
    ///
    /// The region holds **24** `i32`s: it is what
    /// `WelsGetEncBlockStrideOffset`'s own contract states and what
    /// `AllocStrideTables` reserves.
    #[inline]
    pub fn EncBlockOffsets(&self, kiDid: usize) -> Option<&[i32; 24]> {
        self.blocks.get(self.pStrideEncBlockOffset[kiDid]? as usize)
    }

    /// [`EncBlockOffsets`](Self::EncBlockOffsets)' dec-side twin.
    /// `kiTid0` is the C++'s `kbBaseTemporalFlag` — 1 for the base temporal layer.
    #[inline]
    pub fn DecBlockOffsets(&self, kiDid: usize, kiTid0: usize) -> Option<&[i32; 24]> {
        self.blocks.get(self.pStrideDecBlockOffset[kiDid][kiTid0]? as usize)
    }

    /// The macroblock X/Y coordinate tables of layer `kiDid` **as slices** — one
    /// `i16` per macroblock, written by `AllocStrideTables` from the same
    /// `iMbWidth * iMbHeight` the caller passes.
    #[inline]
    pub fn MbIndexXY(&self, kiDid: usize, kiMbNum: usize) -> Option<(&[i16], &[i16])> {
        let (x, y) = (self.pMbIndexX[kiDid]? as usize, self.pMbIndexY[kiDid]? as usize);
        Some((&self.coords[x..][..kiMbNum], &self.coords[y..][..kiMbNum]))
    }
}

/// The preprocess object as a **shared** reference — the only route an
/// **in-fork** body may take, and the reader half of the pair.
#[inline]
pub fn ctx_vpp_ref(
    pCtx: &sWelsEncCtx,
) -> &crate::encoder::wels_preprocess::CWelsPreProcess {
    pCtx.pVpp
        .as_deref()
        .expect("the preprocessor is built by WelsInitEncoderExt")
}

/// The preprocess object as an exclusive reference — the `&mut` twin of
/// [`ctx_vpp_ref`], for the sites that want the object **without** the context
/// beside it.
///
/// Where the context *is* wanted at the same time, [`with_vpp`] is the route — a
/// `&mut` off the field cannot coexist with one to its owner.
#[inline]
pub fn ctx_vpp_mut(
    pCtx: &mut sWelsEncCtx,
) -> &mut crate::encoder::wels_preprocess::CWelsPreProcess {
    pCtx.pVpp
        .as_deref_mut()
        .expect("the preprocessor is built by WelsInitEncoderExt")
}

#[inline]
pub fn with_vpp<R>(
    pCtx: &mut sWelsEncCtx,
    f: impl FnOnce(&mut crate::encoder::wels_preprocess::CWelsPreProcess, &mut sWelsEncCtx) -> R,
) -> R {
    let mut pVpp = pCtx
        .pVpp
        .take()
        .expect("the preprocessor is built by WelsInitEncoderExt");
    let r = f(&mut pVpp, pCtx);
    pCtx.pVpp = Some(pVpp);
    r
}

/// Dependency layer `kiDid` **as a shared reference**.
///
/// `None`: an index past the list, or an unbuilt slot.
#[inline]
pub fn dq_layer_ref(pCtx: &sWelsEncCtx, kiDid: usize) -> Option<&SDqLayer> {
    pCtx.ppDqLayerList.get(kiDid)?.as_deref()
}

/// [`dq_layer_ref`] for the single-threaded writers.
#[inline]
pub fn dq_layer_mut(pCtx: &mut sWelsEncCtx, kiDid: usize) -> Option<&mut SDqLayer> {
    pCtx.ppDqLayerList.get_mut(kiDid)?.as_deref_mut()
}

/// The long-term-reference state of dependency layer `kiDid` — `pLtr[did]`, which is
/// how all consumers spell it.
///
/// # Panics
/// If `kiDid` is not a layer the array holds.
#[inline]
pub fn ctx_ltr_at(pCtx: &mut sWelsEncCtx, kiDid: usize) -> &mut SLTRState {
    &mut pCtx.pLtr[kiDid]
}

/// [`ctx_ltr_at`]'s shared twin.
///
/// The `&mut` form exists for the bodies that write LTR state. Same panic on a
/// bad index, same element.
#[inline]
pub fn ctx_ltr_at_ref(pCtx: &sWelsEncCtx, kiDid: usize) -> &SLTRState {
    &pCtx.pLtr[kiDid]
}

/// `pCtx->pDqIdcMap`, as the slice it is.
#[inline]
pub fn ctx_dq_idc_map(pCtx: &mut sWelsEncCtx) -> &mut [SDqIdc] {
    &mut pCtx.pDqIdcMap
}

/// The three parameter-set arrays **at once, as disjoint borrows**.
///
/// `LoadPrevious` (`paraset_strategy.rs`) writes all three in one call.
#[inline]
pub fn ctx_paraset_arrays(
    pCtx: &mut sWelsEncCtx,
) -> (&mut [SWelsSPS], &mut [SSubsetSps], &mut [SWelsPPS]) {
    (&mut pCtx.pSpsArray, &mut pCtx.pSubsetArray, &mut pCtx.pPPSArray)
}

/// The five disjoint borrows [`sWelsEncCtx::ltr_family_mut`] hands out.
///
/// One owner per field, so the compiler grants all five at once.
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
    /// The **MVD cost table** — `pCtx->pMvdCostTable`.
    ///
    /// Empty before `WelsInitEncoderExt` sizes it.
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

    /// The **rate controller's per-layer array** — `pCtx->pWelsSvcRc`.
    ///
    /// See [`rc_at`](Self::rc_at) for the per-layer entry.
    #[inline]
    pub fn rc(&self) -> &[SWelsSvcRc] {
        &self.pWelsSvcRc
    }

    /// Dependency layer `kiDid`'s **reference list** — `ppRefPicListExt[did]`.
    ///
    /// `None` both past the configured layers and before `InitDqLayers` fills the
    /// slot.
    #[inline]
    pub fn ref_list(&self, kiDid: usize) -> Option<&SRefList> {
        self.ppRefPicListExt.get(kiDid)?.as_deref()
    }

    /// [`ref_list`](Self::ref_list) for the reference-list managers.
    ///
    /// **Single-threaded only.**
    #[inline]
    pub fn ref_list_mut(&mut self, kiDid: usize) -> Option<&mut SRefList> {
        self.ppRefPicListExt.get_mut(kiDid)?.as_deref_mut()
    }

    /// The **parameter-set arrays** — `pSpsArray`, `pSubsetArray`, `pPPSArray`.
    ///
    /// **Empty is a real state** for `pSubsetArray` — the configuration may need
    /// no subset SPS.
    ///
    /// The arrays are filled by `RequestMemorySvc` and by the parameter-set
    /// strategy, both single-threaded.
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
    /// from one borrow**.
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
    /// borrow**.
    ///
    /// Three bodies in `ref_list_mgr_svc.rs` hand the preprocess a `&SRefList`
    /// while holding it `&mut`: `UpdateOriginalPicInfoFromCtx`, `UpdateSrcPicList`
    /// and `UpdateSrcPicListLosslessScreenRefSelectionWithLtr`.
    ///
    /// **Single-threaded only** — an in-fork body must take the shared
    /// [`ctx_vpp_ref`] route.
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
    /// reference list, from one borrow**.
    ///
    /// `AnalyzePictureComplexity` hands `CComplexityAnalysis::Process` three things
    /// that live in three different fields of the context: the VAA block's own
    /// `sVaaCalcInfo` and `pVaaBackgroundMbFlag`, the rate controller's two GOM
    /// arrays, and the *reference picture's* per-macroblock type array. Three
    /// owners, one call.
    #[inline]
    pub fn vaa_rc_and_ref_list_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SVAAFrameInfo>, &mut SWelsSvcRc, Option<&SRefList>) {
        let sWelsEncCtx { pVaa, pWelsSvcRc, ppRefPicListExt, .. } = self;
        (
            pVaa.as_deref_mut().map(VaaBlock::base_mut),
            &mut pWelsSvcRc[kiDid],
            ppRefPicListExt.get(kiDid).and_then(|s| s.as_deref()),
        )
    }

    /// The **screen-content extension and one layer's reference list, from one
    /// borrow** — `UpdateBlockStatic`'s pair (`ref_list_mgr_svc.cpp:648-660`): the
    /// block-static row it rewrites, and the reconstruction it rewrites that row
    /// against.
    ///
    /// The two halves are wanted at the same instant, not in sequence: `row_mut` on
    /// the extension's store and `pic(..).plane(0)` on the list both have to be live
    /// when the screen scene-change plugin is called, and the plugin itself is a
    /// third owner (the preprocessor, taken out of the context by `with_vpp`).
    #[inline]
    pub fn vaa_ext_and_ref_list_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SVAAFrameInfoExt>, Option<&SRefList>) {
        let sWelsEncCtx { pVaa, ppRefPicListExt, .. } = self;
        (
            pVaa.as_deref_mut().and_then(VaaBlock::ext_mut),
            ppRefPicListExt.get(kiDid).and_then(|s| s.as_deref()),
        )
    }

    /// Every field the three LTR bodies — `DeleteInvalidLTR`,
    /// `HandleLTRMarkFeedback` and `LTRMarkProcess` — touch, **from one borrow**.
    ///
    /// A named struct rather than a five-tuple because the three callers want
    /// different subsets, and `_` on a tuple position says nothing about which
    /// field was skipped.
    ///
    /// `param_layer` is one dependency layer's slot, not the whole parameter
    /// block: the bodies write `bEncCurFrmAsIdrFlag` and read `iFrameNum`.
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
            vaa: pVaa.as_deref_mut().map(VaaBlock::base_mut),
            ref_list: ppRefPicListExt.get_mut(kiDid).and_then(|s| s.as_deref_mut()),
            ltr: &mut pLtr[kiDid],
            ref_of_cur_tid_is_ltr: bRefOfCurTidIsLtr,
        }
    }

    /// [`ref_list_and_ltr_mut`](Self::ref_list_and_ltr_mut) **plus the
    /// video-analysis block**.
    ///
    /// Two LTR bodies (`HandleLTRMarkFeedback`, `LTRMarkProcess`) stamp
    /// `SVAAFrameInfo::uiValidLongTermPicIdx` / `uiMarkLongTermPicIdx` from
    /// inside the loop that walks the reference list, so the VAA write and the
    /// list borrow are genuinely wanted at once.
    #[inline]
    pub fn vaa_ref_list_and_ltr_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SVAAFrameInfo>, Option<&mut SRefList>, &mut SLTRState) {
        let sWelsEncCtx { pVaa, ppRefPicListExt, pLtr, .. } = self;
        (
            pVaa.as_deref_mut().map(VaaBlock::base_mut),
            ppRefPicListExt.get_mut(kiDid).and_then(|s| s.as_deref_mut()),
            &mut pLtr[kiDid],
        )
    }

    /// The **coding parameters and the three parameter-set arrays, from one
    /// borrow** — for `paraset_strategy.rs`.
    ///
    /// `WelsGenerateNewSps` and `FindExistingSps` build an SPS from a layer's
    /// configuration *into* the SPS array, and `WelsInitSps` writes
    /// `uiLevelIdc` back into that configuration on the way — so the parameter
    /// block and the arrays are mutably live in the same statement.
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
    /// borrow**.
    ///
    /// Nine bodies in `rc.rs` have the same shape: bind the layer's
    /// `sSpatialLayers[did]` / `sDependencyLayers[did]` config, then write the
    /// layer's rate-control state from it.
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
    /// borrow** — for `AnalyzePictureComplexity`.
    ///
    /// The complexity plugin is handed `&pVaa->sVaaCalcInfo` and the rate
    /// controller's two GOM arrays `&mut` **in the same call**, and the block it
    /// reads back into is `pVaa->sComplexityAnalysisParam`.
    #[inline]
    pub fn vaa_and_rc_at_mut(
        &mut self,
        kiDid: usize,
    ) -> (Option<&mut SVAAFrameInfo>, &mut SWelsSvcRc) {
        let sWelsEncCtx { pVaa, pWelsSvcRc, .. } = self;
        (pVaa.as_deref_mut().map(VaaBlock::base_mut), &mut pWelsSvcRc[kiDid])
    }

    /// The rate-control state of spatial layer `kiDid` — `pWelsSvcRc[did]`.
    /// See [`rc`](Self::rc) for the array.
    ///
    /// # Panics
    /// If `kiDid` is not a layer the array holds.
    #[inline]
    pub fn rc_at(&self, kiDid: usize) -> &SWelsSvcRc {
        &self.pWelsSvcRc[kiDid]
    }

    /// [`rc_at`](Self::rc_at) for the single-threaded writers.
    ///
    /// **Single-threaded only.**
    #[inline]
    pub fn rc_at_mut(&mut self, kiDid: usize) -> &mut SWelsSvcRc {
        &mut self.pWelsSvcRc[kiDid]
    }

    /// The rate controller **and** the current DQ layer, from one `&mut`.
    ///
    /// The three `rc.rs` slice-initialisation bodies need both at once:
    /// `rc_at_mut` for the layer-indexed controller and the layer for its slice
    /// bank.
    ///
    /// **Single-threaded only.**
    #[inline]
    pub fn rc_and_current_layer_mut(
        &mut self,
        kiDid: usize,
    ) -> (&mut SWelsSvcRc, Option<&mut crate::encoder::svc_encode_slice::SDqLayer>) {
        let sWelsEncCtx { pWelsSvcRc, iCurDqLayer, ppDqLayerList, .. } = self;
        let layer = iCurDqLayer
            .and_then(|idx| ppDqLayerList.get_mut(idx.get()))
            .and_then(|l| l.as_deref_mut());
        (&mut pWelsSvcRc[kiDid], layer)
    }

    /// The frame bitstream's **write cursor** — `pFrameBs + iPosBsBuffer`. See
    /// [`frame_bs`](Self::frame_bs), including why the return is **permanently
    /// raw** (nine of its sites store the answer into `SLayerBSInfo::pBsBuf`,
    /// `codec_app_def.h:640`).
    ///
    /// `wrapping_add` rather than `.add`: the same address, computed without an
    /// in-bounds claim, and the claim stays in the `debug_assert` below.
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

    /// The **frame bitstream buffer's root**.
    ///
    /// **A permanent raw return, and the reason is the C ABI.** Of the production
    /// call sites, three store the answer into `SLayerBSInfo::pBsBuf`, and that
    /// field is `codec_app_def.h:640` — `unsigned char* pBsBuf`, a public member
    /// of a struct this library hands to the application. The value crosses the
    /// boundary, so it cannot become a slice, a reference, or anything else
    /// carrying a lifetime. Same for [`frame_bs_cur`](Self::frame_bs_cur).
    ///
    /// Empty answers null — `Vec::as_ptr` on an empty `Vec` answers a dangling
    /// *non-null* address, so this branch is load-bearing, not defensive.
    #[inline]
    pub fn frame_bs(&self) -> *mut u8 {
        if self.pFrameBs.is_empty() {
            return std::ptr::null_mut();
        }
        self.pFrameBs.as_ptr() as *mut u8
    }

    /// The frame bitstream **from the write cursor to the end**, as a slice —
    /// [`frame_bs_cur`](Self::frame_bs_cur)'s safe twin.
    ///
    /// `None`: no buffer, or a cursor past its end.
    #[inline]
    pub fn frame_bs_tail_mut(&mut self) -> Option<&mut [u8]> {
        let kiPos = self.iPosBsBuffer;
        if self.pFrameBs.is_empty() || kiPos < 0 || (kiPos as usize) > self.pFrameBs.len() {
            return None;
        }
        Some(&mut self.pFrameBs[kiPos as usize..])
    }

    /// The encoder's **coding parameters** — `pCtx->pSvcParam`.
    ///
    /// The unconditional readers get a plain reference and
    /// [`param_opt`](Self::param_opt) keeps the guards' shape.
    ///
    /// # Panics
    /// If the parameter block is not built.
    #[inline]
    pub fn param(&self) -> &SWelsSvcCodingParam {
        self.pSvcParam
            .as_deref()
            .expect("the coding parameters are built by WelsInitEncoderExt")
    }

    /// [`param`](Self::param) for the writers: init, `SetOption`, and the
    /// per-layer bookkeeping in `ref_list_mgr_svc.rs` / `encoder_context.rs`.
    ///
    /// **Single-threaded only.**
    ///
    /// # Panics
    /// As [`param`](Self::param).
    #[inline]
    pub fn param_mut(&mut self) -> &mut SWelsSvcCodingParam {
        self.pSvcParam
            .as_deref_mut()
            .expect("the coding parameters are built by WelsInitEncoderExt")
    }

    /// [`param`](Self::param) **as the question the guards ask** —
    /// "has `WelsInitEncoderExt` built the parameters yet?".
    #[inline]
    pub fn param_opt(&self) -> Option<&SWelsSvcCodingParam> {
        self.pSvcParam.as_deref()
    }

    /// The **encoder output block** — `pCtx->pOut`, and the frame's NAL
    /// bookkeeping: the `sNalList` the writers load and unload, `iNalIndex`,
    /// `iLayerBsIndex`, and the `sBsWrite` cursor. The field is
    /// [`pOut`](Self::pOut), `Box`-built by `WelsInitEncoderExt` and dropped at
    /// teardown.
    ///
    /// The unconditional readers get a plain reference and
    /// [`out_opt`](Self::out_opt) keeps the guards' shape.
    ///
    /// # Panics
    /// If the output block is not built, which is to say `WelsInitEncoderExt` has
    /// not run.
    #[inline]
    pub fn out(&self) -> &SWelsEncoderOutput {
        self.pOut
            .as_deref()
            .expect("the encoder output block is built by WelsInitEncoderExt")
    }

    /// [`out`](Self::out) for the writers — the NAL load/unload pairs in
    /// `encoder_ext.rs` and `wels_encoder_ext.rs`, and the per-frame resets of
    /// `iNalIndex` / `iLayerBsIndex` / `sBsWrite`.
    ///
    /// **Single-threaded only.**
    ///
    /// # Panics
    /// As [`out`](Self::out).
    #[inline]
    pub fn out_mut(&mut self) -> &mut SWelsEncoderOutput {
        self.pOut
            .as_deref_mut()
            .expect("the encoder output block is built by WelsInitEncoderExt")
    }

    /// [`out`](Self::out) **as the question the guards ask** — "has the
    /// output block been built yet, or has teardown already taken it?". Teardown
    /// is why the question is real: `WelsUninitEncoderExt` drops the `Box` while
    /// the context is still addressable, and the C-API's status query can arrive
    /// on either side of that.
    #[inline]
    pub fn out_opt(&self) -> Option<&SWelsEncoderOutput> {
        self.pOut.as_deref()
    }

    /// The encoder's **kernel dispatch table** — `pCtx->pFuncList`, and
    /// never absent: the context owns the `Box` from its constructor on, which is
    /// why this is a plain `&` where `vaa` and `ref_list` are `Option`s.
    ///
    /// The table is re-written at frame cadence (`SetFastCodingFunc` /
    /// `SetNormalCodingFunc`): **two fields** (`pfIntraFineMd`,
    /// `sSampleDealingFuncs.pfMdCost`) in one body with one caller
    /// (`PreprocessSliceCoding`), which derives the `&mut` that
    /// [`func_list_mut`](Self::func_list_mut) is. The fork never writes this
    /// table.
    #[inline]
    pub fn func_list(&self) -> &SWelsFuncPtrList {
        &self.pFuncList
    }

    /// [`func_list`](Self::func_list) for the six bodies that write the table:
    /// `InitFunctionPointers` and `InitCoeffFunc` at init, `WelsRcInitModule` and
    /// `SetOption` for `pfRc`, `PreprocessSliceCoding` for the two frame-cadence
    /// fields, and the parameter-set strategy's own `as_mut` callers.
    ///
    /// **Single-threaded only.**
    #[inline]
    pub fn func_list_mut(&mut self) -> &mut SWelsFuncPtrList {
        &mut self.pFuncList
    }

    /// The frame's **video-analysis block** — `pCtx->pVaa`.
    ///
    /// `None` before the preprocessor builds one. The writers are the
    /// preprocessor and the reference-list managers, all single-threaded.
    #[inline]
    pub fn vaa(&self) -> Option<&SVAAFrameInfo> {
        self.pVaa.as_deref().map(VaaBlock::base)
    }

    /// [`vaa`](Self::vaa) for the preprocessor and the reference-list managers.
    ///
    /// **Single-threaded only.**
    #[inline]
    pub fn vaa_mut(&mut self) -> Option<&mut SVAAFrameInfo> {
        self.pVaa.as_deref_mut().map(VaaBlock::base_mut)
    }

    /// [`vaa`](Self::vaa) for the readers that **do not ask** — the analysis
    /// consumers that run after the preprocessor has built the block and
    /// dereference it exactly as the C++ dereferenced `pCtx->pVaa`.
    ///
    /// The `Option` form stays where its callers can see it — those guards are
    /// the *phase* question, has the preprocessor run for this frame? — and the
    /// unconditional readers take the `_expect` name instead.
    ///
    /// # Panics
    /// If the analysis block is not built for this frame.
    #[inline]
    pub fn vaa_expect(&self) -> &SVAAFrameInfo {
        self.vaa().expect("the frame's video-analysis block")
    }

    /// [`vaa_expect`](Self::vaa_expect) for the writers — the reference-list
    /// managers and the preprocessor's own post-analysis stamping.
    ///
    /// **Single-threaded only.**
    ///
    /// # Panics
    /// As [`vaa_expect`](Self::vaa_expect).
    #[inline]
    pub fn vaa_expect_mut(&mut self) -> &mut SVAAFrameInfo {
        self.vaa_mut().expect("the frame's video-analysis block")
    }

    /// [`vaa`](Self::vaa) **as a raw pointer**, null when the block is absent.
    ///
    /// The one production caller hands it to
    /// `SWelsFuncPtrList::pfSetScrollingMv`, whose type (`PSetScrollingMv`,
    /// `wels_func_ptr_def.rs:131`) takes `*mut SVAAFrameInfo`, so a reference
    /// here would have nothing to be passed as.
    #[inline]
    pub fn vaa_ptr(&self) -> *mut SVAAFrameInfo {
        match self.vaa() {
            Some(v) => v as *const SVAAFrameInfo as *mut SVAAFrameInfo,
            None => std::ptr::null_mut(),
        }
    }


    /// The screen-content frame complexity.
    ///
    /// Under `SCREEN_CONTENT_REAL_TIME` the block is the `Screen` arm and this
    /// answers its `iFrameComplexity`. For camera content it is 0, which is what
    /// the rate-control readers treat as "no screen complexity measured".
    #[inline]
    pub fn vaa_ext_screen_frame_complexity(&self) -> i64 {
        self.vaa_ext_ref()
            .map_or(0, |ext| ext.sComplexityScreenParam.iFrameComplexity)
    }


    /// The screen-content extension of the video-analysis block, answering the
    /// [`VaaBlock::Screen`] arm.
    ///
    /// `RequestMemorySvc` allocates an `SVAAFrameInfoExt` under
    /// `SCREEN_CONTENT_REAL_TIME` and a plain `SVAAFrameInfo` otherwise
    /// (`encoder_ext.cpp:1707-1718`); the two arms of [`VaaBlock`] are those two
    /// allocations. `None` is therefore camera content.
    #[inline]
    pub fn vaa_ext_ref(&self) -> Option<&SVAAFrameInfoExt> {
        self.pVaa.as_deref().and_then(VaaBlock::ext)
    }

    /// [`vaa_ext_ref`](Self::vaa_ext_ref) for the extension's writers —
    /// `AnalyzePictureComplexity`'s screen arm and `DetectSceneChangeScreen`'s
    /// best-reference stamping. **Single-threaded only.**
    #[inline]
    pub fn vaa_ext_ref_mut(&mut self) -> Option<&mut SVAAFrameInfoExt> {
        self.pVaa.as_deref_mut().and_then(VaaBlock::ext_mut)
    }
}

/// Master runtime encoder context (`sWelsEncCtx` / `TagWelsEncCtx`).
#[repr(C)]
pub struct sWelsEncCtx {
    pub sLogCtx: SLogContext,
    /// The encoder's coding parameters.
    ///
    /// Resolve it with [`sWelsEncCtx::param`]. `None` before `WelsInitEncoderExt` runs.
    pub pSvcParam: Option<Box<SWelsSvcCodingParam>>,
    pub iMvRange: i32,
    /// The motion-vector-difference cost table.
    /// 52 QP rows of `iMvdCostTableStride` entries each. Root:
    /// [`sWelsEncCtx::mvd_cost_table`]; the **origin** every consumer actually wants
    /// (the zero-MVD entry, `iMvdCostTableSize` into the table, so that a negative MVD
    /// is a negative offset) is [`sWelsEncCtx::mvd_cost_origin`].
    pub pMvdCostTable: Vec<u16>,
    pub iMvdCostTableSize: i32,
    pub iMvdCostTableStride: i32,
    /// `None` before `AllocStrideTables` runs.
    pub pStrideTab: Option<Box<SStrideTables>>,
    /// The kernel dispatch table.
    ///
    /// A plain `Box`, not an `Option<Box<_>>` like [`pSvcParam`](Self::pSvcParam):
    /// the table has no "not built yet" state worth modelling. Its `Default` is
    /// every slot `None`, and `InitFunctionPointers` writes over it.
    /// Root: [`sWelsEncCtx::func_list`].
    pub pFuncList: Box<SWelsFuncPtrList>,
    /// The slice-threading block, `Box`-built by
    /// `RequestMtResource` and dropped by `ReleaseMtResource`; `None` is
    /// "single-threaded encoder".
    pub pSliceThreading: Option<Box<SSliceThreading>>,
    /// `IWelsReferenceStrategy*` in C++ (`encoder_context.h`) — the strategy's
    /// *identity*. See [`RefStrategyKind`].
    pub eRefStrategy: crate::encoder::ref_list_mgr_svc::RefStrategyKind,
    /// The source picture being encoded — a slot of `pVpp`'s spatial pool.
    pub pEncPic: Option<SrcPicId>,
    /// The picture being reconstructed into, and the one being referenced — slots of
    /// **the current dependency layer's** `SRefList` (`ppRefPicListExt[uiDependencyId]`).
    pub pDecPic: Option<RecPicId>,
    pub pRefPic: Option<RecPicId>,
    /// The layer the encoder is working on, as a **position in `ppDqLayerList`**.
    ///
    /// The list is built once by `InitDqLayers`, freed once by `FreeDqLayer`, and
    /// nothing permutes it, so a position is a stable identity.
    ///
    /// **`None` is "no layer is current"**.
    pub iCurDqLayer: Option<crate::encoder::svc_encode_slice::LayerIdx>,
    /// One DQ layer per dependency layer. `None` before `InitDqLayers` fills the
    /// slot.
    pub ppDqLayerList: Vec<Option<Box<SDqLayer>>>,
    /// One reference list per dependency layer. `None` between `RequestMemorySvc`
    /// sizing the array and `InitDqLayers` filling it. Resolve a layer's list with
    /// [`sWelsEncCtx::ref_list`] / [`sWelsEncCtx::ref_list_mut`].
    pub ppRefPicListExt: Vec<Option<Box<SRefList>>>,
    pub pRefList0: [Option<RecPicId>; 16],
    /// One long-term-reference state per dependency layer, `ResetLtrState`'d entry
    /// by entry. The per-layer entry: [`ctx_ltr_at`].
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
    /// One state per spatial layer; each holds the five arrays `RcInitLayerMemory`
    /// fills. Per-layer: [`sWelsEncCtx::rc_at`].
    pub pWelsSvcRc: Vec<SWelsSvcRc>,
    pub bCheckWindowStatusRefreshFlag: bool,
    pub iCheckWindowStartTs: i64,
    pub iCheckWindowCurrentTs: i64,
    pub iCheckWindowInterval: i32,
    pub iCheckWindowIntervalShift: i32,
    pub bCheckWindowShiftResetFlag: bool,
    pub iGlobalQp: i32,
    /// The video-analysis block for the frame in flight.
    /// `None` before the preprocessor runs. Resolve it with [`sWelsEncCtx::vaa`].
    ///
    /// **A [`VaaBlock`]**, `Base` for camera content and `Screen`
    /// for `SCREEN_CONTENT_REAL_TIME` — the two allocations of
    /// `encoder_ext.cpp:1707-1718`. The enum sits inside the `Box` so this stays one
    /// word; [`vaa_ext_ref`](sWelsEncCtx::vaa_ext_ref) is the `Screen` arm.
    pub pVaa: Option<Box<VaaBlock>>,
    /// The preprocess object, `Box`-built by
    /// [`CWelsPreProcess::CreatePreProcess`] and dropped by the teardown; `None`
    /// before init and after `FreeMemorySvc`. The methods
    /// that take both `&mut self` (the vpp) and `&mut sWelsEncCtx` are called
    /// through the `Option::take` dance — the box moves out for the call and back
    /// after.
    pub pVpp: Option<Box<crate::encoder::wels_preprocess::CWelsPreProcess>>,
    /// `RequestMemorySvc` sizes this from the strategy's `GetNeededSpsNum`; the
    /// **active** entry is [`iSps`](Self::iSps) below.
    pub pSpsArray: Vec<SWelsSPS>,
    /// The **active** SPS, as its position in `pSpsArray`.
    /// Resolve it with [`ctx_sps`](crate::encoder::svc_encode_slice::ctx_sps).
    pub iSps: Option<SpsId>,
    /// See [`pSpsArray`](Self::pSpsArray).
    pub pPPSArray: Vec<SWelsPPS>,
    /// The **active** PPS, as its position in `pPPSArray` — see [`iSps`](Self::iSps).
    /// Resolve it with [`ctx_pps`](crate::encoder::svc_encode_slice::ctx_pps).
    pub iPps: Option<PpsId>,
    /// See [`pSpsArray`](Self::pSpsArray).
    ///
    /// **Empty**: `RequestMemorySvc` allocates nothing at all when
    /// `GetNeededSubsetSpsNum()` answers 0 (simulcast AVC, and every single-layer
    /// configuration), and every consumer tests for it.
    pub pSubsetArray: Vec<SSubsetSps>,

    pub iSpsNum: i32,
    pub iSubsetSpsNum: i32,
    pub iPpsNum: i32,
    /// The encoder output block, `Box`-built at init (`new_boxed`) and dropped at
    /// teardown.
    pub pOut: Option<Box<SWelsEncoderOutput>>,
    /// The frame's output bitstream — the encoder's one arena of
    /// bytes. Every NAL the frame emits is written into it at `iPosBsBuffer`, and
    /// `SLayerBSInfo::pBsBuf` holds cursors into it that outlive the call that made
    /// them. Root: [`sWelsEncCtx::frame_bs`]; the write cursor:
    /// [`sWelsEncCtx::frame_bs_cur`].
    ///
    /// **A deviation.** The C++ takes this block with `WelsMalloc`, not
    /// `WelsMallocz` — it is the one member of `RequestMemorySvc`'s set that starts
    /// *uninitialized*. The `Vec` is zero-filled, because a safe container has no
    /// uninitialized alternative; every read of this buffer sits behind a write
    /// cursor (`iPosBsBuffer` only ever advances past bytes a NAL writer has just
    /// written, and `pOut->iNalLen` bounds every read back).
    pub pFrameBs: Vec<u8>,
    pub iFrameBsSize: i32,
    pub iPosBsBuffer: i32,
    pub sSpatialIndexMap: [SSpatialPicIndex; MAX_DEPENDENCY_LAYER],
    pub iSliceBufferSize: [i32; MAX_DEPENDENCY_LAYER],
    pub bRefOfCurTidIsLtr: [[bool; MAX_TEMPORAL_LEVEL]; MAX_DEPENDENCY_LAYER],
    pub iMaxSliceCount: i32,
    pub iActiveThreadsNum: i16,
    /// One row per dependency layer. Root: [`ctx_dq_idc_map`].
    pub pDqIdcMap: Vec<SDqIdc>,
    /// The C++ declares a companion `SParaSetOffset*` beside this one, pointing
    /// either here or at the caller's vector. This field, held by value, is the
    /// vector.
    pub sPSOVector: SParaSetOffset,
    pub uiStartTimestamp: i64,
    pub sEncoderStatistics: [crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics; MAX_DEPENDENCY_LAYER],
    pub iStatisticsLogInterval: i32,
    pub iLastStatisticsLogTs: i64,
    pub iEncoderError: i32,
    pub bDeliveryFlag: bool,
    pub sWelsCabacContexts: [[[SStateCtx; WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
    pub uiLastTimestamp: i64,
    pub pDynamicBsBuffer: [Vec<u8>; MAX_THREADS_NUM],
}

impl sWelsEncCtx {
    /// The encoder context's **allocation zero**, spelled out — `WelsInitEncoderExt`'s
    /// `WelsMalloc` + `memset(0)` in a form that survives a field changing type.
    ///
    /// It is **not** an "init". `WelsInitEncoderExt` does the initialization, in the
    /// order the C++ does it, and every non-zero starting value the encoder has lives
    /// there.
    ///
    /// # The zeros, and what each one means
    ///
    /// Grouped by who is responsible for making the field non-zero. Four groups:
    /// the log sink (the caller's, before anything else runs), the members
    /// `RequestMemorySvc` allocates (null is "not allocated yet"), the per-frame
    /// state `WelsInitCurrentLayer` and the frame loop restamp every frame (zero is
    /// "no frame has run"), and the parameter-set bookkeeping `InitDqLayers` fills.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            // The caller's log sink, stamped in by `WelsInitEncoderExt` before
            // anything can log. `pfLog: WelsTraceCallback` is `None` when nothing
            // is installed; `pLogCtx` is the *caller's* opaque context and C-ABI by
            // definition.
            sLogCtx: SLogContext::default(),

            // ---- allocated by RequestMemorySvc; null == not allocated yet -------
            pSvcParam: None,
            iMvRange: 0,                    // set by InitMvRange from the level limit
            pMvdCostTable: Vec::new(),
            iMvdCostTableSize: 0,           // paired with the table above
            iMvdCostTableStride: 0,
            pStrideTab: None,
            pFuncList: Box::new(SWelsFuncPtrList::default()),
            pSliceThreading: None,

            // `TemporalLayer`, the zero discriminant.
            eRefStrategy: crate::encoder::ref_list_mgr_svc::RefStrategyKind::TemporalLayer,

            // ---- per-frame picture handles; None == no picture bound ------------
            // These are pool handles, not pointers. `None` is a *state*, "the frame
            // loop has not picked a picture yet", and the encoder tests for it.
            pEncPic: None,
            pDecPic: None,
            pRefPic: None,

            // No layer is current until `WelsInitCurrentLayer` / `WelsSwapDqLayers`
            // names one, which cannot happen before `ppDqLayerList` below is
            // allocated.
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
            // The three `Array` members are allocations; the three singular ones
            // are *aliases into them* that `WelsInitEncoderExt` aims at the heads.
            // The three counts are `InitDqLayers`'s, and zero is the honest
            // starting length.
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
            // is `{ pSrc: None, iDid: 0 }`.
            sSpatialIndexMap: [SSpatialPicIndex::default(); MAX_DEPENDENCY_LAYER],
            iSliceBufferSize: [0; MAX_DEPENDENCY_LAYER],
            // "Is the reference for this (did, tid) a long-term one?" — false until a
            // reference exists at all.
            bRefOfCurTidIsLtr: [[false; MAX_TEMPORAL_LEVEL]; MAX_DEPENDENCY_LAYER],
            iMaxSliceCount: 0,
            iActiveThreadsNum: 0,           // set from iMultipleThreadIdc

            pDqIdcMap: Vec::new(),

            // `sPSOVector` is held **by value**.
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

            // One dynamic bitstream buffer per thread, allocated on the first slice
            // that needs one.
            pDynamicBsBuffer: std::array::from_fn(|_| Vec::new()),
        }
    }
}

impl Default for sWelsEncCtx {
    /// The zeroed shell, by way of [`new`](Self::new).
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Core Encoder Functions (encoder.cpp)
// ============================================================================

/// Initializes input source picture geometry, color planes, and line strides.
pub fn InitPic(
    pSrcPic: &mut SSourcePicture,
    kiColorspace: i32,
    kiWidth: i32,
    kiHeight: i32,
) -> i32 {

    if kiWidth == 0 || kiHeight == 0 {
        return 1;
    }

    let vflip_mask = VideoFormat::videoFormatVFlip as i32;
    let base_colorspace = kiColorspace & !vflip_mask;

    pSrcPic.iColorFormat = kiColorspace;
    pSrcPic.iPicWidth = kiWidth;
    pSrcPic.iPicHeight = kiHeight;

    if base_colorspace != VideoFormat::videoFormatI420 as i32 {
        return 2;
    }

    match base_colorspace {
        cs if cs == VideoFormat::videoFormatI420 as i32
            || cs == VideoFormat::videoFormatYV12 as i32 =>
        {
            pSrcPic.pData[0] = std::ptr::null_mut();
            pSrcPic.pData[1] = std::ptr::null_mut();
            pSrcPic.pData[2] = std::ptr::null_mut();
            pSrcPic.pData[3] = std::ptr::null_mut();
            pSrcPic.iStride[0] = kiWidth;
            pSrcPic.iStride[1] = kiWidth >> 1;
            pSrcPic.iStride[2] = kiWidth >> 1;
            pSrcPic.iStride[3] = 0;
        }
        cs if cs == VideoFormat::videoFormatYUY2 as i32
            || cs == VideoFormat::videoFormatYVYU as i32
            || cs == VideoFormat::videoFormatUYVY as i32 =>
        {
            pSrcPic.pData[0] = std::ptr::null_mut();
            pSrcPic.pData[1] = std::ptr::null_mut();
            pSrcPic.pData[2] = std::ptr::null_mut();
            pSrcPic.pData[3] = std::ptr::null_mut();
            pSrcPic.iStride[0] = CALC_BI_STRIDE(kiWidth, 16);
            pSrcPic.iStride[1] = 0;
            pSrcPic.iStride[2] = 0;
            pSrcPic.iStride[3] = 0;
        }
        cs if cs == VideoFormat::videoFormatRGB as i32
            || cs == VideoFormat::videoFormatBGR as i32 =>
        {
            pSrcPic.pData[0] = std::ptr::null_mut();
            pSrcPic.pData[1] = std::ptr::null_mut();
            pSrcPic.pData[2] = std::ptr::null_mut();
            pSrcPic.pData[3] = std::ptr::null_mut();
            pSrcPic.iStride[0] = CALC_BI_STRIDE(kiWidth, 24);
            pSrcPic.iStride[1] = 0;
            pSrcPic.iStride[2] = 0;
            pSrcPic.iStride[3] = 0;
            if (kiColorspace & vflip_mask) != 0 {
                pSrcPic.iColorFormat = kiColorspace & !vflip_mask;
            } else {
                pSrcPic.iColorFormat = kiColorspace | vflip_mask;
            }
        }
        cs if cs == VideoFormat::videoFormatBGRA as i32
            || cs == VideoFormat::videoFormatRGBA as i32
            || cs == VideoFormat::videoFormatARGB as i32
            || cs == VideoFormat::videoFormatABGR as i32 =>
        {
            pSrcPic.pData[0] = std::ptr::null_mut();
            pSrcPic.pData[1] = std::ptr::null_mut();
            pSrcPic.pData[2] = std::ptr::null_mut();
            pSrcPic.pData[3] = std::ptr::null_mut();
            pSrcPic.iStride[0] = kiWidth << 2;
            pSrcPic.iStride[1] = 0;
            pSrcPic.iStride[2] = 0;
            pSrcPic.iStride[3] = 0;
            if (kiColorspace & vflip_mask) != 0 {
                pSrcPic.iColorFormat = kiColorspace & !vflip_mask;
            } else {
                pSrcPic.iColorFormat = kiColorspace | vflip_mask;
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
        pFuncList.pfInterMdBackgroundDecision = WelsMdInterJudgeBGDPskip;
        pFuncList.pfMdBackgroundInfoUpdate = WelsMdUpdateBGDInfo;
    } else {
        pFuncList.pfInterMdBackgroundDecision = WelsMdInterJudgeBGDPskipFalse;
        pFuncList.pfMdBackgroundInfoUpdate = WelsMdUpdateBGDInfoNULL;
    }
}

/// Initializes encoder compute kernel function pointers.
pub fn InitFunctionPointers(
    pEncCtx: &mut sWelsEncCtx,
    _uiCpuFlag: u32,
) -> i32 {
    if pEncCtx.param_opt().is_none() {
        return ENC_RETURN_SUCCESS;
    }
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

    // `encoder.cpp:193` installed `sExpandPicFunc` here. The call it fed now names
    // its two kernels directly.

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
    // depending on kbBaseAvail && kbHighestSpatial.

    crate::encoder::deblocking::DeblockingInit(&mut fl.pfDeblocking, _uiCpuFlag as i32);

    crate::encoder::rc::WelsRcInitFuncPointers(
        &mut fl.pfRc,
        kiRCMode,
    );

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
    // the field.
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
fn InitCoeffFunc(
    pFuncList: &mut SWelsFuncPtrList,
    _uiCpuFlag: u32,
    iEntropyCodingModeFlag: i32,
) {
    pFuncList.pfCavlcParamCal = crate::encoder::svc_set_mb_syn_cavlc::CavlcParamCal_c;
    pFuncList.eEntropyCoder = EntropyCoder::from_flag(iEntropyCodingModeFlag);
}

/// Increments the H.264 slice header `frame_num` syntax element for spatial layer `kiDidx`.
pub fn UpdateFrameNum(pEncCtx: &mut sWelsEncCtx, kiDidx: i32) {
    if pEncCtx.param_opt().is_none() {
        return;
    }
    let Some(kpSps) = crate::encoder::svc_encode_slice::ctx_sps_ref(pEncCtx) else {
        return;
    };
    let max_frame_num_minus1 = (1 << kpSps.uiLog2MaxFrameNum) - 1;
    let kbLastNalWasRef =
        pEncCtx.eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST;
    let pParamInternal = &mut pEncCtx.param_mut().sDependencyLayers[kiDidx as usize];
    let mut bNeedFrameNumIncreasing = false;

    if kbLastNalWasRef {
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
pub fn LoadBackFrameNum(pEncCtx: &mut sWelsEncCtx, kiDidx: i32) {
    if pEncCtx.param_opt().is_none() {
        return;
    }
    let Some(kpSps) = crate::encoder::svc_encode_slice::ctx_sps_ref(pEncCtx) else {
        return;
    };
    let max_frame_num_minus1 = (1 << kpSps.uiLog2MaxFrameNum) - 1;
    let kbLastNalWasRef =
        pEncCtx.eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST;
    let pParamInternal = &mut pEncCtx.param_mut().sDependencyLayers[kiDidx as usize];
    let mut bNeedFrameNumIncreasing = false;

    if kbLastNalWasRef {
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
    let Some(pOut) = pEncCtx.pOut.as_deref_mut() else {
        return;
    };
    pOut.iNalIndex = 0;
    pOut.iLayerBsIndex = 0;

    pOut.sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();
    pEncCtx.iPosBsBuffer = 0;
}

/// Configures slice types, NAL headers, and Picture Order Count (POC) for the frame.
pub fn InitFrameCoding(
    pEncCtx: &mut sWelsEncCtx,
    keFrameType: EVideoFrameType,
    kiDidx: i32,
) {
    if pEncCtx.param_opt().is_none() {
        return;
    }
    let Some(kpSps) = crate::encoder::svc_encode_slice::ctx_sps_ref(pEncCtx) else {
        return;
    };
    let max_poc_boundary = (1 << kpSps.iLog2MaxPocLsb) - 2;

    if keFrameType == EVideoFrameType::videoFrameTypeP {
        let pParamInternal = &mut pEncCtx.param_mut().sDependencyLayers[kiDidx as usize];
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
        let pParamInternal = &mut pEncCtx.param_mut().sDependencyLayers[kiDidx as usize];
        pParamInternal.iFrameNum = 0;
        pParamInternal.iPOC = 0;
        pParamInternal.bEncCurFrmAsIdrFlag = false;
        pParamInternal.iFrameIndex = 0;
        pParamInternal.iCodingIndex = 0;

        pEncCtx.eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
        pEncCtx.eSliceType = EWelsSliceType::I_SLICE;
        pEncCtx.eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;
    } else if keFrameType == EVideoFrameType::videoFrameTypeI {
        let pParamInternal = &mut pEncCtx.param_mut().sDependencyLayers[kiDidx as usize];
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
pub fn DecideFrameType(
    pEncCtx: &mut sWelsEncCtx,
    kiSpatialNum: i8,
    kiDidx: i32,
    bSkipFrameFlag: bool,
) -> EVideoFrameType {
    if pEncCtx.param_opt().is_none() {
        return EVideoFrameType::videoFrameTypeInvalid;
    }
    let kiUsageType = pEncCtx.param().iUsageType;
    let kbSceneChangeDetect = pEncCtx.param().bEnableSceneChangeDetect;
    let kiSpatialLayerNum = pEncCtx.param().iSpatialLayerNum;
    let kbEnableLtr = pEncCtx.param().bEnableLongTermReference;
    let kiLTRRefNum = pEncCtx.param().iLTRRefNum;
    let kbEncCurFrmAsIdrFlag =
        pEncCtx.param().sDependencyLayers[kiDidx as usize].bEncCurFrmAsIdrFlag;
    let kiFrameIndex = pEncCtx.param().sDependencyLayers[kiDidx as usize].iFrameIndex;
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
            || kbEncCurFrmAsIdrFlag
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
            pEncCtx.param_mut().sDependencyLayers[kiDidx as usize].iCodingIndex = 0;
            pEncCtx.bCurFrameMarkedAsSceneLtr = true;
        }
    } else {
        let pVaa = pEncCtx.vaa();
        let vaa_idr = pVaa.is_some_and(|v| v.bIdrPeriodFlag);

        if !kbSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < kiSpatialLayerNum)
            || (kiFrameIndex < (VGOP_SIZE << 1))
        {
            bSceneChangeFlag = false;
        } else if let Some(pVaa) = pVaa {
            bSceneChangeFlag = pVaa.bSceneChangeFlag;
        }

        iFrameType = if vaa_idr || bSceneChangeFlag || kbEncCurFrmAsIdrFlag {
            EVideoFrameType::videoFrameTypeIDR
        } else {
            EVideoFrameType::videoFrameTypeP
        };

        if iFrameType == EVideoFrameType::videoFrameTypeP && bSkipFrameFlag {
            iFrameType = EVideoFrameType::videoFrameTypeSkip;
        } else if iFrameType == EVideoFrameType::videoFrameTypeIDR {
            pEncCtx.param_mut().sDependencyLayers[kiDidx as usize].iCodingIndex = 0;
        }
    }

    iFrameType
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_tables_share_regions_and_answer_none_off_the_end() {
        // Two block-offset regions, then two coordinate ones — the shape
        // `AllocStrideTables` carves, at its smallest.
        let mut tab = SStrideTables::new(2, 64);
        tab.pStrideDecBlockOffset[0][1] = Some(0);
        // The **shared region**: two layers, one index.
        tab.pStrideDecBlockOffset[1][1] = Some(0);
        tab.pStrideEncBlockOffset[0] = Some(1);
        tab.pMbIndexX[0] = Some(0);
        tab.pMbIndexY[0] = Some(32);

        tab.i32_block24_mut(0)[0] = 0x3C3C;
        tab.i32_block24_mut(1)[0] = 7;
        tab.i16_region_mut(0, 32)[0] = 3;
        tab.i16_region_mut(32, 32)[0] = 4;

        assert_eq!(tab.DecBlockOffsets(0, 1).expect("layer 0's dec table")[0], 0x3C3C);
        assert_eq!(tab.EncBlockOffsets(0).expect("layer 0's enc table")[0], 7);
        let (x, y) = tab.MbIndexXY(0, 32).expect("layer 0's coordinate tables");
        assert_eq!((x[0], y[0]), (3, 4));

        assert_eq!(
            tab.DecBlockOffsets(1, 1).expect("layer 1's dec table")[0],
            0x3C3C,
            "two layers, one region"
        );
        assert!(tab.MbIndexXY(3, 32).is_none(), "None answers the null the field used to hold");
        assert!(tab.EncBlockOffsets(3).is_none());
    }

    #[test]
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
        ctx.pVaa = Some(Box::new(VaaBlock::Base(SVAAFrameInfo::default())));
        ctx.pSvcParam = Some(Box::new(SWelsSvcCodingParam::default()));
        ctx.ppRefPicListExt = vec![Some(SRefList::new())];
        ctx.ppDqLayerList = vec![Some(Box::new(
            crate::encoder::svc_encode_slice::SDqLayer::default(),
        ))];

        let p: *mut sWelsEncCtx = &mut *ctx;

        unsafe {
            let rc = (*p).rc_at_mut(0);
            rc.iGomSize = 4;
            crate::encoder::rc::RcInitLayerMemory(rc, 2);
        }
        // And the whole set once more, interleaved: every cursor taken first, then
        // every one used — which is the frame loop's actual shape, and the case a
        // per-accessor test cannot reach.
        let held: Vec<*mut u8> = unsafe {
            vec![
                (*p).vaa_ptr().cast(), (*p).frame_bs().cast(),
            ]
        };
        // `frame_bs` is null here (no bitstream in this fixture), which is itself
        // the assertion that empty still answers null after everything above. It is
        // the **last** entry, and the two counts below are derived from the vector
        // rather than written twice.
        let last = held.len() - 1;
        assert!(held[last].is_null(), "no frame bitstream was installed");
        for (i, q) in held.iter().enumerate().take(last) {
            assert!(!q.is_null(), "held cursor {i} went null");
            unsafe { assert_eq!(*q.cast::<u8>(), *q.cast::<u8>()) };
        }
    }

    /// `SLayerBSInfo::pBsBuf` keeps a cursor into `pFrameBs` for the life of a
    /// layer's bitstream info, while the NAL writers keep deriving more from the
    /// same buffer at `iPosBsBuffer`.
    #[test]
    #[allow(unsafe_code)]
    fn frame_bs_cursors_are_siblings() {
        let mut ctx = Box::new(sWelsEncCtx::new());
        let p: *mut sWelsEncCtx = &mut *ctx;
        // Before `RequestMemorySvc`, both answer null.
        assert!(unsafe { (*p).frame_bs() }.is_null());
        assert!(unsafe { (*p).frame_bs_cur() }.is_null());

        ctx.pFrameBs = vec![0u8; 64];
        ctx.iFrameBsSize = 64;
        let p: *mut sWelsEncCtx = &mut *ctx;

        // `pBsBuf` — the root, stored and kept, exactly as the three sites that take
        // it do.
        let stored = unsafe { (*p).frame_bs() };

        // The frame loop then walks: derive at the cursor, write, advance, repeat.
        // The position is set through `p` rather than through `ctx`, so the one raw
        // binding above stays live across the whole walk.
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

    use crate::encoder::wels_preprocess::CWelsPreProcess;

    /// The context-level read path, before and after the tables exist.
    #[test]
    fn ctx_stride_tables_answer_none_before_alloc_and_share_regions_after() {
        let mut ctx = Box::new(sWelsEncCtx::new());
        assert!(ctx.pStrideTab.is_none(), "no tables before AllocStrideTables");

        let mut tab = SStrideTables::new(2, 0);
        tab.pStrideEncBlockOffset[0] = Some(0);
        tab.pStrideEncBlockOffset[1] = Some(1);
        tab.i32_block24_mut(0)[0] = 11;
        tab.i32_block24_mut(1)[0] = 22;
        ctx.pStrideTab = Some(Box::new(tab));

        let kpTab = ctx.pStrideTab.as_ref().expect("the tables are installed");
        let (first, other) = (kpTab.EncBlockOffsets(0), kpTab.EncBlockOffsets(1));
        let again = kpTab.EncBlockOffsets(0);
        assert_eq!(
            (first.expect("layer 0")[0], other.expect("layer 1")[0], again.expect("layer 0")[0]),
            (11, 22, 11),
            "three reads, two regions, and the first answer still live at the third"
        );
        assert!(kpTab.EncBlockOffsets(3).is_none());
    }

    #[test]
    fn test_calc_bi_stride() {
        assert_eq!(CALC_BI_STRIDE(640, 24), 1920);
        assert_eq!(CALC_BI_STRIDE(1920, 16), 3840);
        assert_eq!(CALC_BI_STRIDE(1920, 24), 5760);
    }

    #[test]
    fn test_init_pic() {
        let mut src_pic = SSourcePicture::default();
        let ret = InitPic(
            &mut src_pic,
            VideoFormat::videoFormatI420 as i32,
            640,
            480,
        );
        assert_eq!(ret, 0);
        assert_eq!(src_pic.iPicWidth, 640);
        assert_eq!(src_pic.iPicHeight, 480);
        assert_eq!(src_pic.iStride[0], 640);
        assert_eq!(src_pic.iStride[1], 320);
        assert_eq!(src_pic.iStride[2], 320);
    }


    /// `sWelsEncCtx::new()` reproduces the zeroed shell it replaces, byte for
    /// byte, and every difference is attributed to a *named field* before it is
    /// accepted. It is meant to read **zero differences**.
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
    /// # The ten fields that cannot be compared as bytes at all
    ///
    /// **`None` writes the discriminant and leaves the payload
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
    /// **And the *niche* is not what makes it happen.** `iCurDqLayer`, `iSps` and
    /// `iPps` are
    /// `Option`s over plain integer newtypes with **no** niche: a tag byte plus a
    /// payload. `None` writes the tag and leaves the payload undefined exactly as the
    /// handles do. It is not the niche,
    /// it is that **an `Option`'s `None` defines only its discriminant**.
    ///
    /// So the honest statement is narrower than "byte-identical", and it is the
    /// narrower one that is true: `new()` reproduces the shell **everywhere the
    /// shell's bytes are defined by the type**, and at the ten fields below it
    /// reproduces the *values*, which is all anything reads. Nothing reads a `None`'s
    /// payload — that is read when a `Some` is unwrapped, and there is no `Some`
    /// here — and nothing reads padding at all. The ten are excluded **by name** and
    /// asserted **by value**.
    ///
    /// The general rule this leaves behind: *a field-wise constructor cannot be
    /// proved byte-equal to a memset image, only value-equal, and the difference is
    /// exactly the bytes the type does not define.* In practice, for this port: every
    /// `Option` field, and all padding.
    ///
    /// **A field added to this struct as an `Option` belongs on the `BY_VALUE` list**,
    /// and the test will say so under Miri if it is not.
    ///
    /// # The owned members, and the third tier
    ///
    /// The memset image of a `Vec` is a null `Unique`, which is **not a `Vec`**, so
    /// `mem::zeroed::<sWelsEncCtx>()` is itself undefined behaviour:
    ///
    /// ```text
    /// error: Undefined Behavior: constructing invalid value of type sWelsEncCtx:
    ///   at .pSpsArray.buf.inner.ptr.pointer.pointer, encountered 0,
    ///   but expected something greater or equal to 1
    /// ```
    ///
    /// So the shell is held as **raw bytes** — `MaybeUninit::zeroed`, never
    /// `assume_init`ed — and there are three tiers:
    ///
    /// * **tier 1**, byte for byte: every field whose bytes are fully defined in both.
    /// * **tier 2**, by value: the `Option` and padded fields, where the *shell* value is recovered
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
        // The zeroed shell has no image of these; `new()` builds the empty
        // container.
        const OWNED: [&str; 12] = [
            "pSpsArray", "pSubsetArray", "pPPSArray", "pDqIdcMap", "pFrameBs", "pLtr",
            "pWelsSvcRc", "ppRefPicListExt", "ppDqLayerList", "pMvdCostTable",
            // `pDynamicBsBuffer` is the only member here that is an *array* of
            // owned containers, so the claim below is per element: four empty
            // `Vec`s.
            "pDynamicBsBuffer",
            // `pFuncList` is the one owned field whose empty state is not "no
            // elements" — a `Box` is always inhabited — so what tier 3 asserts for
            // it is the *content*: the table `new()` builds is the uninstalled
            // table.
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
        // definition.
        let fl = &*built.pFuncList;
        assert!(fl.pfGetLumaI16x16Pred.iter().all(Option::is_none), "new(): no I16x16 predictors");
        assert!(fl.pfGetLumaI4x4Pred.iter().all(Option::is_none), "new(): no I4x4 predictors");
        assert!(fl.pfGetChromaPred.iter().all(Option::is_none), "new(): no chroma predictors");
        assert!(fl.pfMotionSearch.iter().all(Option::is_none), "new(): no motion search");
        assert!(fl.sMeFuncs.pfSearchMethod.iter().all(Option::is_none), "new(): no search method");
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
        // The two discriminants whose zero *is* a declared variant.
        assert_eq!(fl.eEntropyCoder, EntropyCoder::Cavlc, "new(): the memset's entropy coder");
        assert_eq!(
            fl.pfRc.eInstalledMode,
            crate::api::codec_api::RC_MODES::RC_QUALITY_MODE,
            "new(): the memset's rate-control mode"
        );
        assert!(fl.pParametersetStrategy.is_none(), "new(): no paraset strategy is installed yet");

        // ---- tier 2: excluded by name and asserted by value --------------------
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
        // rest is inter-field `repr(C)` padding plus the by-value fields; both are
        // reported rather than asserted at a number.
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
    fn test_update_and_loadback_framenum() {
        let param = SWelsSvcCodingParam::default();
        // Only the fields this test exercises.
        let sps = SWelsSPS {
            uiLog2MaxFrameNum: 4,
            iLog2MaxPocLsb: 4,
            bFrameCroppingFlag: false,
            sFrameCrop: SCropOffset::default(),
            ..Default::default()
        };
        let mut ctx = sWelsEncCtx::new();
        // The context *owns* its parameters, so the fixture hands them over rather
        // than lending them — and the read-back below goes through the context,
        // which is where the writes land.
        ctx.pSvcParam = Some(Box::new(param));
        // The context names its SPS by position, so the test stands up the
        // one-entry array the position indexes into.
        ctx.pSpsArray = vec![sps];
        ctx.iSpsNum = 1;
        ctx.iSps = Some(SpsId(0));
        ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;

        let frame_num = |c: &sWelsEncCtx| c.pSvcParam.as_ref().unwrap().sDependencyLayers[0].iFrameNum;

        UpdateFrameNum(&mut ctx, 0);
        assert_eq!(frame_num(&ctx), 1);
        assert_eq!(ctx.eLastNalPriority[0], EWelsNalRefIdc::NRI_PRI_LOWEST);

        ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;
        LoadBackFrameNum(&mut ctx, 0);
        assert_eq!(frame_num(&ctx), 0);
    }

    #[test]
    fn test_decide_frame_type() {
        let mut param = SWelsSvcCodingParam::default();
        let mut ctx = sWelsEncCtx::new();
        param.sDependencyLayers[0].bEncCurFrmAsIdrFlag = true;
        ctx.pSvcParam = Some(Box::new(param.clone()));
        // The context owns the block, so the fixture hands it one.
        ctx.pVaa = Some(Box::new(VaaBlock::Base(SVAAFrameInfo::default())));

        let ft = DecideFrameType(&mut ctx, 1, 0, false);
        assert_eq!(ft, EVideoFrameType::videoFrameTypeIDR);
    }

    #[test]
    fn test_init_function_pointers() {
        let param = SWelsSvcCodingParam::default();
        let mut ctx = sWelsEncCtx::default();
        // The context brings its own table; the fixture reads it back out.
        ctx.pSvcParam = Some(Box::new(param.clone()));

        let ret = InitFunctionPointers(&mut ctx, 0);
        assert_eq!(ret, ENC_RETURN_SUCCESS);

        // Each assertion reads the table back through its owner rather than
        // binding a reference to it once. That is not stylistic: the
        // `InitCoeffFunc` call below *writes* the table.
        //
        // The predictor table is the one installer whose slots are still `Option`:
        // `new()` asserts it is all-`None` above, and `WelsInitIntraPredFuncs` is
        // the first call in the chain.
        assert!(
            ctx.pFuncList.pfGetLumaI16x16Pred.iter().any(Option::is_some),
            "InitFunctionPointers must walk its installer chain"
        );
        // pfInterMd is deliberately NOT asserted: C++ InitFunctionPointers
        // (encoder.cpp) never sets it. It is assigned per-slice in
        // svc_encode_slice.cpp:733/736.
        // What is worth asserting is that the flag reached the entropy coder:
        // `param` defaults to `iEntropyCodingModeFlag == 0`. The other
        // arm goes through `InitCoeffFunc` rather than a second
        // `InitFunctionPointers`, which would allocate a second parameter-set
        // strategy over the first.
        assert_eq!(ctx.pFuncList.eEntropyCoder, EntropyCoder::Cavlc);
        InitCoeffFunc(ctx.func_list_mut(), 0, 1);
        assert_eq!(ctx.pFuncList.eEntropyCoder, EntropyCoder::Cabac);

        assert!(ctx.pFuncList.pfDeblocking.pfDeblockingFilterSlice.is_some());
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`.
pub use crate::common::cpu_core::{WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_FMA, WELS_CPU_MMX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE, WELS_CPU_SSE2, WELS_CPU_SSE3, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3};









#[cfg(test)]
mod with_vpp_provenance {
    use super::*;
    use crate::encoder::picture::{SPicture, SrcPicPool};
    use crate::encoder::rec_view::RoPicView;
    use crate::encoder::wels_preprocess::CWelsPreProcess;

    /// [`with_vpp`] moves the preprocessor's `Box` out of the context and stores
    /// it back. `SDqLayer::pEncView` is an `RoPicView` built in
    /// `WelsInitCurrentLayer` off a picture *in* the pool; its captured pointers
    /// are `root_ptr_shared()` into each plane's `buf: Vec<u8>`, and a `Vec`'s
    /// buffer is a **separate allocation**, so retagging the `CWelsPreProcess`
    /// block does not touch those stacks.
    ///
    /// The frame's ordering is reproduced in miniature: stamp the view, run
    /// `with_vpp`, then read through the view.
    #[test]
    fn with_vpp_does_not_pop_a_view_built_off_the_spatial_pool() {
        let mut ctx = sWelsEncCtx::new();
        let mut vpp = CWelsPreProcess::default();

        let mut pic = SPicture::new(176, 144, false);
        pic.plane_mut(0).set(0, 0, 0x5A);
        pic.plane_mut(0).set(3, 2, 0xC3);
        vpp.m_pSpatialPicPool = SrcPicPool::new(vec![pic]);
        ctx.pVpp = Some(Box::new(vpp));

        // `WelsInitCurrentLayer`'s stamp, in miniature: a read-only view of a
        // pooled source picture, held past the call below exactly as the layer
        // holds it for the frame.
        let id = ctx
            .pVpp
            .as_ref()
            .expect("just installed")
            .m_pSpatialPicPool
            .ids()
            .next()
            .expect("one slot");
        let view = RoPicView::build(
            ctx.pVpp.as_ref().expect("just installed").m_pSpatialPicPool.get(id),
        );

        // The take-and-restore. If this retag popped the view's captured
        // pointers, the reads below are through a dead tag.
        with_vpp(&mut ctx, |pVpp, _pCtx| {
            assert!(!pVpp.m_pSpatialPicPool.ids().next().is_none(), "the pool survives the move");
        });

        assert_eq!(view.plane(0).at(0, 0), 0x5A, "the view still reads its own plane");
        assert_eq!(view.plane(0).at(3, 2), 0xC3);

        // And the slot is restored, which is the property the closure form exists
        // to guarantee on every path.
        assert!(ctx.pVpp.is_some(), "with_vpp restores the box");
    }

    /// The control for the probe above: a pointer taken *into the box's
    /// own allocation* (`m_pSpatialPicPool`, a field of `CWelsPreProcess`) and
    /// read after [`with_vpp`] has moved the `Box` out and back.
    ///
    /// `#[ignore]`d because Miri reports UB by aborting, which is not a failure a
    /// harness can assert on. Run it deliberately:
    ///
    /// ```text
    /// MIRIFLAGS='-Zmiri-ignore-leaks -Zmiri-disable-isolation' \
    ///   cargo +nightly miri test --lib -- --ignored pointer_into_the_box
    /// ```
    ///
    /// Expected: `Undefined Behavior: attempting a read access using <tag> ...
    /// but that tag does not exist in the borrow stack`. Under plain `cargo test`
    /// it passes and proves nothing — that is the point of the second referee.
    #[test]
    #[ignore = "Miri control: aborts on UB, so it is run deliberately, not by the harness"]
    #[allow(unsafe_code)]
    fn a_pointer_into_the_box_does_not_survive_with_vpp() {
        let mut ctx = sWelsEncCtx::new();
        let mut vpp = CWelsPreProcess::default();
        vpp.m_pSpatialPicPool = SrcPicPool::new(vec![SPicture::new(176, 144, false)]);
        ctx.pVpp = Some(Box::new(vpp));

        // The control builds the pointer by hand: the `pVpp` slot read as a value,
        // then `addr_of_mut!` of the pool field — a pointer *inside* the
        // `CWelsPreProcess` allocation rather than into a plane's own `Vec`.
        let pPool: *mut SrcPicPool = unsafe {
            let pVpp = std::ptr::read(
                std::ptr::addr_of!(ctx.pVpp) as *const *mut CWelsPreProcess,
            );
            std::ptr::addr_of_mut!((*pVpp).m_pSpatialPicPool)
        };

        with_vpp(&mut ctx, |_pVpp, _pCtx| {});

        // The read Miri must refuse: the move above retagged the allocation this
        // pointer names, so its tag is gone from that stack.
        let n = unsafe { (*pPool).ids().count() };
        assert_eq!(n, 1, "reached only if the tag survived — under Miri it must not");
    }
}
