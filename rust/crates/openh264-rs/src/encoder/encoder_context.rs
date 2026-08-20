pub const MAX_DEPENDENCY_LAYER: usize = 4;
/// OpenH264 Video Encoder Core Context and State Machine
///
/// Translated from `codec/encoder/core/inc/encoder_context.h` and
/// `codec/encoder/core/src/encoder.cpp`.

use std::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;
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

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLogContext {
    pub pfLog: *mut c_void,
    pub pLogCtx: *mut c_void,
    pub pCodecInstance: *mut c_void,
}

impl Default for SLogContext {
    fn default() -> Self {
        Self {
            pfLog: std::ptr::null_mut(),
            pLogCtx: std::ptr::null_mut(),
            pCodecInstance: std::ptr::null_mut(),
        }
    }
}

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
    pub pEncMb: [*mut u8; 3],
    pub pDecMb: [*mut u8; 3],
    pub pRefMb: [*mut u8; 3],
    pub pCsMb: [*mut u8; 3],
}

impl Default for SPicData {
    fn default() -> Self {
        Self {
            pEncMb: [std::ptr::null_mut(); 3],
            pDecMb: [std::ptr::null_mut(); 3],
            pRefMb: [std::ptr::null_mut(); 3],
            pCsMb: [std::ptr::null_mut(); 3],
        }
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
    #[inline]
    fn root(&mut self) -> *mut u8 {
        self.base.as_mut_ptr().cast::<u8>()
    }

    #[inline]
    fn at_i32(&mut self, kiByteOffset: Option<u32>) -> *mut i32 {
        match kiByteOffset {
            // SAFETY: every offset stored here was produced by `AllocStrideTables`
            // carving the very block `base` is, and is a multiple of 4.
            Some(off) => unsafe { self.root().add(off as usize).cast::<i32>() },
            None => std::ptr::null_mut(),
        }
    }

    #[inline]
    fn at_i16(&mut self, kiByteOffset: Option<u32>) -> *mut i16 {
        match kiByteOffset {
            // SAFETY: as `at_i32`; the two coordinate regions are even-aligned.
            Some(off) => unsafe { self.root().add(off as usize).cast::<i16>() },
            None => std::ptr::null_mut(),
        }
    }

    /// `pStrideDecBlockOffset[kiDid][kiTid0]` as the cursor it used to be. `kiTid0`
    /// is the C++'s `kbBaseTemporalFlag` — 1 for the base temporal layer.
    #[inline]
    pub fn StrideDecBlockOffset(&mut self, kiDid: usize, kiTid0: usize) -> *mut i32 {
        let off = self.pStrideDecBlockOffset[kiDid][kiTid0];
        self.at_i32(off)
    }

    /// `pStrideEncBlockOffset[kiDid]` as the cursor it used to be.
    #[inline]
    pub fn StrideEncBlockOffset(&mut self, kiDid: usize) -> *mut i32 {
        let off = self.pStrideEncBlockOffset[kiDid];
        self.at_i32(off)
    }

    /// `pMbIndexX[kiDid]` as the cursor it used to be.
    #[inline]
    pub fn MbIndexX(&mut self, kiDid: usize) -> *mut i16 {
        let off = self.pMbIndexX[kiDid];
        self.at_i16(off)
    }

    /// `pMbIndexY[kiDid]` as the cursor it used to be.
    #[inline]
    pub fn MbIndexY(&mut self, kiDid: usize) -> *mut i16 {
        let off = self.pMbIndexY[kiDid];
        self.at_i16(off)
    }
}

/// [`SStrideTables::StrideDecBlockOffset`] reached through the context — the
/// spelling every consumer used when `pStrideTab` was a raw pointer.
///
/// The `&mut` these four take covers the `Option<Box<SStrideTables>>` field and the
/// 160-odd bytes it points at; the cursor they answer points into the *arena*, which
/// is a different allocation and is never retagged by any of them. That is the whole
/// reason repeated calls are safe to interleave with held cursors.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
pub unsafe fn ctx_stride_dec_block_offset(
    pCtx: *mut sWelsEncCtx,
    kiDid: usize,
    kiTid0: usize,
) -> *mut i32 {
    match (*pCtx).pStrideTab.as_mut() {
        Some(tab) => tab.StrideDecBlockOffset(kiDid, kiTid0),
        None => std::ptr::null_mut(),
    }
}

/// [`SStrideTables::StrideEncBlockOffset`] reached through the context.
///
/// # Safety
/// As [`ctx_stride_dec_block_offset`].
#[inline]
pub unsafe fn ctx_stride_enc_block_offset(pCtx: *mut sWelsEncCtx, kiDid: usize) -> *mut i32 {
    match (*pCtx).pStrideTab.as_mut() {
        Some(tab) => tab.StrideEncBlockOffset(kiDid),
        None => std::ptr::null_mut(),
    }
}

/// [`SStrideTables::MbIndexX`] reached through the context.
///
/// # Safety
/// As [`ctx_stride_dec_block_offset`].
#[inline]
pub unsafe fn ctx_mb_index_x(pCtx: *mut sWelsEncCtx, kiDid: usize) -> *mut i16 {
    match (*pCtx).pStrideTab.as_mut() {
        Some(tab) => tab.MbIndexX(kiDid),
        None => std::ptr::null_mut(),
    }
}

/// The **root** of `pCtx->pFrameBs` — T6.H4.
///
/// This is the one this session had to enumerate before converting, because the
/// cursors it hands out are *held*: `SLayerBSInfo::pBsBuf` keeps one for the life of
/// a layer's bitstream info while the NAL writers derive more from the same buffer.
/// There are **nineteen** cursor derivations in production — sixteen walking ones
/// through [`ctx_frame_bs_at`] (12 in `encoder_ext.rs`, 3 in `wels_encoder_ext.rs`,
/// 1 in `slice_multi_threading.rs`) and three of the root itself stored into a
/// `pBsBuf` — plus one null guard in `slice_multi_threading.rs`. The conversion adds
/// no `&mut` to any of them: as everywhere in this family, `Vec::as_mut_ptr` reads
/// the header, so the derivations are siblings and none pops another.
/// `frame_bs_cursors_are_siblings` is that as a test.
///
/// Empty answers null, which is what the field held before `RequestMemorySvc` ran.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
pub unsafe fn ctx_frame_bs(pCtx: *mut sWelsEncCtx) -> *mut u8 {
    let buf: &mut Vec<u8> = &mut (*pCtx).pFrameBs;
    if buf.is_empty() {
        return std::ptr::null_mut();
    }
    buf.as_mut_ptr()
}

/// The frame bitstream's write cursor at byte `kiPos` — `pFrameBs + iPosBsBuffer`,
/// which is how all fourteen of the walking derivations spell it. See
/// [`ctx_frame_bs`].
///
/// # Safety
/// As [`ctx_frame_bs`], and `kiPos` must be within the buffer.
#[inline]
pub unsafe fn ctx_frame_bs_at(pCtx: *mut sWelsEncCtx, kiPos: i32) -> *mut u8 {
    let root = ctx_frame_bs(pCtx);
    if root.is_null() {
        return std::ptr::null_mut();
    }
    debug_assert!(
        kiPos >= 0 && (kiPos as usize) <= (*pCtx).pFrameBs.len(),
        "frame bitstream cursor {kiPos} is outside the buffer"
    );
    root.add(kiPos as usize)
}

/// The **root** of `pCtx->pDqIdcMap` — T6.H3; see [`ctx_sps_array`] for the
/// spelling and for what empty means.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
pub unsafe fn ctx_dq_idc_map(pCtx: *mut sWelsEncCtx) -> *mut SDqIdc {
    let arr: &mut Vec<SDqIdc> = &mut (*pCtx).pDqIdcMap;
    if arr.is_empty() {
        return std::ptr::null_mut();
    }
    arr.as_mut_ptr()
}

/// The **root** of `pCtx->pSpsArray` — T6.H2, and S40's spelling again.
///
/// The three parameter-set arrays were `WelsMallocz`'d blocks that every consumer
/// indexed with `.add(id)`; they are `Vec`s now and this answers the same address
/// the block's head had, so `.add(id)` downstream is unchanged. `Vec::as_mut_ptr`
/// reads the pointer out of the header rather than forming a `&mut [T]` over the
/// array, so a caller may hold an entry cursor across a second call — which
/// `LoadPrevious` does with all three at once, and `WelsInitCurrentLayer` does per
/// layer.
///
/// **Empty answers null**, which is what `pSubsetArray` held whenever the
/// configuration needed no subset SPS, and what all three held before
/// `RequestMemorySvc` ran. Every `is_null()` guard downstream therefore still asks
/// the question it was written to ask — `Vec::as_mut_ptr` on an empty `Vec` answers
/// a dangling non-null address, so this branch is load-bearing, not defensive.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
pub unsafe fn ctx_sps_array(pCtx: *mut sWelsEncCtx) -> *mut SWelsSPS {
    let arr: &mut Vec<SWelsSPS> = &mut (*pCtx).pSpsArray;
    if arr.is_empty() {
        return std::ptr::null_mut();
    }
    arr.as_mut_ptr()
}

/// The **root** of `pCtx->pSubsetArray` — see [`ctx_sps_array`].
///
/// # Safety
/// As [`ctx_sps_array`].
#[inline]
pub unsafe fn ctx_subset_array(pCtx: *mut sWelsEncCtx) -> *mut SSubsetSps {
    let arr: &mut Vec<SSubsetSps> = &mut (*pCtx).pSubsetArray;
    if arr.is_empty() {
        return std::ptr::null_mut();
    }
    arr.as_mut_ptr()
}

/// The **root** of `pCtx->pPPSArray` — see [`ctx_sps_array`].
///
/// # Safety
/// As [`ctx_sps_array`].
#[inline]
pub unsafe fn ctx_pps_array(pCtx: *mut sWelsEncCtx) -> *mut SWelsPPS {
    let arr: &mut Vec<SWelsPPS> = &mut (*pCtx).pPPSArray;
    if arr.is_empty() {
        return std::ptr::null_mut();
    }
    arr.as_mut_ptr()
}

/// [`SStrideTables::MbIndexY`] reached through the context.
///
/// # Safety
/// As [`ctx_stride_dec_block_offset`].
#[inline]
pub unsafe fn ctx_mb_index_y(pCtx: *mut sWelsEncCtx, kiDid: usize) -> *mut i16 {
    match (*pCtx).pStrideTab.as_mut() {
        Some(tab) => tab.MbIndexY(kiDid),
        None => std::ptr::null_mut(),
    }
}

/// Master runtime encoder context (`sWelsEncCtx` / `TagWelsEncCtx`).
#[repr(C)]
pub struct sWelsEncCtx {
    pub sLogCtx: SLogContext,
    pub pSvcParam: *mut SWelsSvcCodingParam,
    pub iMvRange: i32,
    pub pMvdCostTable: *mut u16,
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
    pub pFuncList: *mut SWelsFuncPtrList,
    pub pSliceThreading: *mut SSliceThreading,
    pub pTaskManage: *mut c_void,
    /// `IWelsReferenceStrategy*` in C++ (`encoder_context.h`); **T4b.2b** made it
    /// the strategy's *identity* instead of a pointer to an object carrying only a
    /// back-pointer to this very struct. See [`RefStrategyKind`].
    ///
    /// **S20**: this is `#[repr(C)]` and the member sits between two 8-byte-aligned
    /// pointers, so the 7 bytes of padding that realign `pEncPic` exactly replace the
    /// 7 bytes the pointer loses. `assert_size!(sWelsEncCtx, ...)` does not move, and
    /// neither does any of the fifteen `assert_ctx_offset!` pins -- four of which
    /// (`ppRefPicListExt` 184, `pLtr` 320, `sSpatialIndexMap` 520, `pMemAlign` 1824)
    /// sit after this field and encode C++ `offsetof` values that must not change.
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
    pub ppDqLayerList: *mut *mut SDqLayer,
    pub ppRefPicListExt: *mut *mut SRefList,
    pub pRefList0: [Option<RecPicId>; 16],
    pub pLtr: *mut SLTRState,
    pub bCurFrameMarkedAsSceneLtr: bool,
    pub eSliceType: EWelsSliceType,
    pub eNalType: EWelsNalUnitType,
    pub eNalPriority: EWelsNalRefIdc,
    pub eLastNalPriority: [EWelsNalRefIdc; MAX_DEPENDENCY_LAYER],
    pub iNumRef0: u8,
    pub uiDependencyId: u8,
    pub uiTemporalId: u8,
    pub bNeedPrefixNalFlag: bool,
    pub pWelsSvcRc: *mut SWelsSvcRc,
    pub bCheckWindowStatusRefreshFlag: bool,
    pub iCheckWindowStartTs: i64,
    pub iCheckWindowCurrentTs: i64,
    pub iCheckWindowInterval: i32,
    pub iCheckWindowIntervalShift: i32,
    pub bCheckWindowShiftResetFlag: bool,
    pub iGlobalQp: i32,
    pub pVaa: *mut SVAAFrameInfo,
    pub pVpp: *mut crate::encoder::wels_preprocess::CWelsPreProcess,
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
    pub pOut: *mut SWelsEncoderOutput,
    /// The frame's output bitstream — **T6.H4, and the encoder's one arena of
    /// bytes.** Every NAL the frame emits is written into it at `iPosBsBuffer`, and
    /// `SLayerBSInfo::pBsBuf` holds cursors into it that outlive the call that made
    /// them. Root: [`ctx_frame_bs`]; the write cursor: [`ctx_frame_bs_at`].
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
    pub sPSOVector: SParaSetOffset,
    pub pPSOVector: *mut SParaSetOffset,
    pub pMemAlign: *mut CMemoryAlign,
    pub uiStartTimestamp: i64,
    pub sEncoderStatistics: [crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics; MAX_DEPENDENCY_LAYER],
    pub iStatisticsLogInterval: i32,
    pub iLastStatisticsLogTs: i64,
    pub iEncoderError: i32,
    pub mutexEncoderError: *mut c_void,
    pub bDeliveryFlag: bool,
    pub sWelsCabacContexts: [[[SStateCtx; WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
    pub uiLastTimestamp: i64,
    pub pDynamicBsBuffer: [*mut u8; MAX_THREADS_NUM],
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
            // The caller's log sink. `WelsInitEncoderExt` copies it in from
            // `SExistingParasetList`'s neighbourhood before anything can log; three
            // null `c_void`s mean "no sink installed", which is what the C tests for.
            sLogCtx: SLogContext::default(),

            // ---- allocated by RequestMemorySvc; null == not allocated yet -------
            // (Session H's list. Every one of these is freed by FreeMemorySvc, and
            // null is the value that makes the paired free a no-op — which is why
            // the C++ can call it on a half-built context after an early failure.)
            pSvcParam: std::ptr::null_mut(),
            iMvRange: 0,                    // set by InitMvRange from the level limit
            pMvdCostTable: std::ptr::null_mut(),
            iMvdCostTableSize: 0,           // paired with the table above
            iMvdCostTableStride: 0,
            pStrideTab: None,
            pFuncList: std::ptr::null_mut(),
            pSliceThreading: std::ptr::null_mut(),
            pTaskManage: std::ptr::null_mut(),

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
            ppDqLayerList: std::ptr::null_mut(),
            ppRefPicListExt: std::ptr::null_mut(),

            // `iNumRef0` below is the live length of this list; sixteen `None`s is the
            // empty list, and the two agree only because both are zero here.
            pRefList0: [None; 16],

            pLtr: std::ptr::null_mut(),
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
            pWelsSvcRc: std::ptr::null_mut(),
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
            pVaa: std::ptr::null_mut(),
            pVpp: std::ptr::null_mut(),

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
            pOut: std::ptr::null_mut(),
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

            // `sPSOVector` is held **by value** and `pPSOVector` points either at it
            // or at the caller's, so the value's zero has to be the zero of the
            // pointer's target. `SParaSetOffset::default()` is all-zero throughout
            // (its own impl, field for field), which is the id-strategy's "no id has
            // been handed out yet".
            sPSOVector: SParaSetOffset::default(),
            pPSOVector: std::ptr::null_mut(),

            pMemAlign: std::ptr::null_mut(),

            // ---- statistics and timestamps ---------------------------------------
            // Timestamps are absolute and in the caller's clock, so zero is a real
            // "not yet stamped" and every consumer compares against the previous one.
            uiStartTimestamp: 0,
            sEncoderStatistics:
                [crate::encoder::wels_encoder_ext::TagVideoEncoderStatistics::default(); MAX_DEPENDENCY_LAYER],
            iStatisticsLogInterval: 0,      // set from the param's log interval
            iLastStatisticsLogTs: 0,
            iEncoderError: 0,               // == ENC_RETURN_SUCCESS, and that matters
            mutexEncoderError: std::ptr::null_mut(),
            bDeliveryFlag: false,

            // The CABAC probability tables. Zero is `{ MPS = 0, state = 0 }`, which is
            // not a valid coding state — `WelsCabacContextInit` fills all four
            // models for every QP before any of it is read, exactly as the C++ does
            // after its own memset.
            sWelsCabacContexts: [[[SStateCtx::new(0); WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
            uiLastTimestamp: 0,

            // Phase 7's. One dynamic bitstream buffer per thread, allocated on the
            // first slice that needs one.
            pDynamicBsBuffer: [std::ptr::null_mut(); MAX_THREADS_NUM],
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
pub unsafe fn WelsInitBGDFunc(
    pFuncList: *mut SWelsFuncPtrList,
    kbEnableBackgroundDetection: bool,
) {
    if pFuncList.is_null() {
        return;
    }
    if kbEnableBackgroundDetection {
        (*pFuncList).pfInterMdBackgroundDecision = Some(WelsMdInterJudgeBGDPskip);
        (*pFuncList).pfMdBackgroundInfoUpdate = Some(WelsMdUpdateBGDInfo);
    } else {
        (*pFuncList).pfInterMdBackgroundDecision = Some(WelsMdInterJudgeBGDPskipFalse);
        (*pFuncList).pfMdBackgroundInfoUpdate = Some(WelsMdUpdateBGDInfoNULL);
    }
}

/// Initializes encoder compute kernel function pointers.
///
/// # Safety
/// `pEncCtx` and `pParam` must be valid non-null pointers.
pub unsafe fn InitFunctionPointers(
    pEncCtx: *mut sWelsEncCtx,
    pParam: *mut SWelsSvcCodingParam,
    _uiCpuFlag: u32,
) -> i32 {
    if pEncCtx.is_null() || pParam.is_null() || (*pEncCtx).pFuncList.is_null() {
        return ENC_RETURN_SUCCESS;
    }
    let pFuncList = (*pEncCtx).pFuncList;

    let bScreenContent = (*pParam).iUsageType == crate::api::codec_api::EUsageType::SCREEN_CONTENT_REAL_TIME;

    // `encoder.cpp:193` installed `sExpandPicFunc` here. T4b.3b deleted the table:
    // the call it fed now names its two kernels directly. The history is worth one
    // line, because this call was *missing* before Phase 4a found it, and with it
    // every slot stayed `None` and `WelsUpdateRefList`'s `ExpandReferencingPicture`
    // expanded nothing -- a bug that a table of optional function pointers can
    // have and a direct call cannot.

    /* Intra_Prediction_fn */
    crate::encoder::get_intra_predictor::WelsInitIntraPredFuncs(pFuncList, _uiCpuFlag);

    /* ME func */
    crate::encoder::svc_motion_estimate::WelsInitMeFunc(pFuncList, _uiCpuFlag, bScreenContent);

    /* sad, satd, average */
    crate::encoder::sample::WelsInitSampleSadFunc(pFuncList, _uiCpuFlag);

    WelsInitBGDFunc(pFuncList, (*pParam).bEnableBackgroundDetection);
    crate::encoder::svc_mode_decision::WelsInitSCDPskipFunc(
        pFuncList,
        bScreenContent
            && (*pParam).bEnableSceneChangeDetect
            && ((*(*pEncCtx).pSvcParam).iComplexityMode as i32)
                < (crate::api::codec_api::ECOMPLEXITY_MODE::HIGH_COMPLEXITY as i32),
    );

    // for pfGetVarianceFromIntraVaa function ptr adaptive by CPU features
    crate::encoder::md::InitIntraAnalysisVaaInfo(pFuncList, _uiCpuFlag);

    /* Motion compensation */
    crate::common::mc::InitMcFunc(&mut (*pFuncList).sMcFuncs, _uiCpuFlag);
    InitCoeffFunc(pFuncList, _uiCpuFlag, (*pParam).iEntropyCodingModeFlag);

    crate::encoder::encode_mb_aux::WelsInitEncodingFuncs(pFuncList, _uiCpuFlag);
    crate::encoder::decode_mb_aux::WelsInitReconstructionFuncs(pFuncList, _uiCpuFlag);

    // C++ does NOT set pfInterMd here. It is assigned per-slice in
    // svc_encode_slice.cpp:733/736 to WelsMdInterMbEnhancelayer or WelsMdInterMb
    // depending on kbBaseAvail && kbHighestSpatial. This line used to assign
    // WelsMdSpatialelInterMbIlfmdNoilp, which is a different function with a
    // different signature (its last parameter is Mb_Type, not SMbCache*) -- the
    // mem::transmute around it was what let that through. WelsMdInterMb is not
    // ported yet, so the assignment belongs with that work, not here.

    crate::encoder::deblocking::DeblockingInit(
        &mut (*pFuncList).pfDeblocking as *mut _,
        _uiCpuFlag as i32,
    );

    crate::encoder::rc::WelsRcInitFuncPointers(
        &mut (*pFuncList).pfRc,
        (*pParam).iRCMode,
    );

    crate::encoder::deblocking::WelsBlockFuncInit(
        &mut (*pFuncList).pfSetNZCZero as *mut _,
        _uiCpuFlag as i32,
    );

    crate::encoder::md::InitFillNeighborCacheInterFunc(
        pFuncList,
        (*pParam).bEnableBackgroundDetection as i32,
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
    (*pFuncList).pParametersetStrategy =
        crate::encoder::paraset_strategy::CreateParametersetStrategy(
            (*pParam).eSpsPpsIdStrategy,
            (*pParam).bSimulcastAVC,
            (*pParam).iSpatialLayerNum,
        );
    if (*pFuncList).pParametersetStrategy.is_none() {
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
unsafe fn InitCoeffFunc(
    pFuncList: *mut SWelsFuncPtrList,
    _uiCpuFlag: u32,
    iEntropyCodingModeFlag: i32,
) {
    (*pFuncList).pfCavlcParamCal = Some(crate::encoder::svc_set_mb_syn_cavlc::CavlcParamCal_c);
    (*pFuncList).eEntropyCoder = EntropyCoder::from_flag(iEntropyCodingModeFlag);
}

/// Increments the H.264 slice header `frame_num` syntax element for spatial layer `kiDidx`.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
pub unsafe fn UpdateFrameNum(pEncCtx: *mut sWelsEncCtx, kiDidx: i32) {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || ctx_sps(pEncCtx).is_null() {
        return;
    }
    let pParamInternal = std::ptr::addr_of_mut!((*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize]);
    let mut bNeedFrameNumIncreasing = false;

    if (*pEncCtx).eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        let max_frame_num_minus1 = (1 << (*ctx_sps(pEncCtx)).uiLog2MaxFrameNum) - 1;
        if (*pParamInternal).iFrameNum < max_frame_num_minus1 {
            (*pParamInternal).iFrameNum += 1;
        } else {
            (*pParamInternal).iFrameNum = 0;
        }
    }

    (*pEncCtx).eLastNalPriority[kiDidx as usize] = EWelsNalRefIdc::NRI_PRI_LOWEST;
}

/// Rolls back the `frame_num` counter if a reference frame encoding attempt fails.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
pub unsafe fn LoadBackFrameNum(pEncCtx: *mut sWelsEncCtx, kiDidx: i32) {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || ctx_sps(pEncCtx).is_null() {
        return;
    }
    let pParamInternal = std::ptr::addr_of_mut!((*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize]);
    let mut bNeedFrameNumIncreasing = false;

    if (*pEncCtx).eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        if (*pParamInternal).iFrameNum != 0 {
            (*pParamInternal).iFrameNum -= 1;
        } else {
            (*pParamInternal).iFrameNum = (1 << (*ctx_sps(pEncCtx)).uiLog2MaxFrameNum) - 1;
        }
    }
}

/// Reinitializes bitstream buffer write offsets and NAL indices.
///
/// # Safety
/// `pEncCtx` must be non-null and contain a valid `pOut`.
pub unsafe fn InitBitStream(pEncCtx: *mut sWelsEncCtx) {
    if pEncCtx.is_null() || (*pEncCtx).pOut.is_null() {
        return;
    }
    (*pEncCtx).iPosBsBuffer = 0;
    (*(*pEncCtx).pOut).iNalIndex = 0;
    (*(*pEncCtx).pOut).iLayerBsIndex = 0;

    // Was `InitBits(&…sBsWrite, …pBsBuffer, …uiSize)`. The buffer and its length stay
    // where they were; the writer is a position, and resetting it is all `InitBits`
    // did that still means anything. Its `kpBuf: *const u8` parameter — stored as
    // `pStartBuf: *mut u8` and written through — is deleted rather than amended
    // (`phase2_findings.md` F13, third site).
    (*(*pEncCtx).pOut).sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();
}

/// Configures slice types, NAL headers, and Picture Order Count (POC) for the frame.
///
/// # Safety
/// `pEncCtx` must be non-null and properly initialized.
pub unsafe fn InitFrameCoding(
    pEncCtx: *mut sWelsEncCtx,
    keFrameType: EVideoFrameType,
    kiDidx: i32,
) {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || ctx_sps(pEncCtx).is_null() {
        return;
    }
    let pParamInternal = std::ptr::addr_of_mut!((*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize]);

    if keFrameType == EVideoFrameType::videoFrameTypeP {
        (*pParamInternal).iFrameIndex += 1;

        let max_poc_boundary = (1 << (*ctx_sps(pEncCtx)).iLog2MaxPocLsb) - 2;
        if (*pParamInternal).iPOC < max_poc_boundary {
            (*pParamInternal).iPOC += 2;
        } else {
            (*pParamInternal).iPOC = 0;
        }

        UpdateFrameNum(pEncCtx, kiDidx);

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        (*pEncCtx).eSliceType = EWelsSliceType::P_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGH;
    } else if keFrameType == EVideoFrameType::videoFrameTypeIDR {
        (*pParamInternal).iFrameNum = 0;
        (*pParamInternal).iPOC = 0;
        (*pParamInternal).bEncCurFrmAsIdrFlag = false;
        (*pParamInternal).iFrameIndex = 0;

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
        (*pEncCtx).eSliceType = EWelsSliceType::I_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;

        (*pParamInternal).iCodingIndex = 0;
    } else if keFrameType == EVideoFrameType::videoFrameTypeI {
        let max_poc_boundary = (1 << (*ctx_sps(pEncCtx)).iLog2MaxPocLsb) - 2;
        if (*pParamInternal).iPOC < max_poc_boundary {
            (*pParamInternal).iPOC += 2;
        } else {
            (*pParamInternal).iPOC = 0;
        }

        UpdateFrameNum(pEncCtx, kiDidx);

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        (*pEncCtx).eSliceType = EWelsSliceType::I_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;
    }
}

/// Evaluates VAA scene change analysis, LTR feedback, and rate control constraints to classify frame coding type.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
pub unsafe fn DecideFrameType(
    pEncCtx: *mut sWelsEncCtx,
    kiSpatialNum: i8,
    kiDidx: i32,
    bSkipFrameFlag: bool,
) -> EVideoFrameType {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() {
        return EVideoFrameType::videoFrameTypeInvalid;
    }
    let pSvcParam = (*pEncCtx).pSvcParam;
    let pParamInternal = std::ptr::addr_of_mut!((*pSvcParam).sDependencyLayers[kiDidx as usize]);
    let mut iFrameType: EVideoFrameType;
    let mut bSceneChangeFlag = false;

    if (*pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
        let pVaa = (*pEncCtx).pVaa;
        let vaa_idr = !pVaa.is_null() && (*pVaa).bIdrPeriodFlag;

        if !(*pSvcParam).bEnableSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < (*pSvcParam).iSpatialLayerNum)
        {
            bSceneChangeFlag = false;
        } else if !pVaa.is_null() {
            bSceneChangeFlag = (*pVaa).bSceneChangeFlag;
        }

        if vaa_idr
            || (*pParamInternal).bEncCurFrmAsIdrFlag
            || (!(*pSvcParam).bEnableLongTermReference && bSceneChangeFlag && !bSkipFrameFlag)
        {
            iFrameType = EVideoFrameType::videoFrameTypeIDR;
        } else if (*pSvcParam).bEnableLongTermReference
            && (bSceneChangeFlag
                || (!pVaa.is_null() && (*pVaa).eSceneChangeIdc == ESceneChangeIdc::LARGE_CHANGED_SCENE))
        {
            let mut iActualLtrcount = 0;
            if !(*pEncCtx).ppRefPicListExt.is_null() {
                let ref_list_0 = *(*pEncCtx).ppRefPicListExt;
                if !ref_list_0.is_null() {
                    for i in 0..(*pSvcParam).iLTRRefNum {
                        let Some(id) = (*ref_list_0).pLongRefList[i as usize] else {
                            continue;
                        };
                        let pic = (*ref_list_0).pic(id);
                        if pic.bUsedAsRef && pic.bIsLongRef && pic.bIsSceneLTR {
                            iActualLtrcount += 1;
                        }
                    }
                }
            }
            if iActualLtrcount == (*pSvcParam).iLTRRefNum && bSceneChangeFlag {
                iFrameType = EVideoFrameType::videoFrameTypeIDR;
            } else {
                iFrameType = EVideoFrameType::videoFrameTypeP;
                (*pEncCtx).bCurFrameMarkedAsSceneLtr = true;
            }
        } else {
            iFrameType = EVideoFrameType::videoFrameTypeP;
        }

        if iFrameType == EVideoFrameType::videoFrameTypeP && bSkipFrameFlag {
            iFrameType = EVideoFrameType::videoFrameTypeSkip;
        } else if iFrameType == EVideoFrameType::videoFrameTypeIDR {
            (*pParamInternal).iCodingIndex = 0;
            (*pEncCtx).bCurFrameMarkedAsSceneLtr = true;
        }
    } else {
        let pVaa = (*pEncCtx).pVaa;
        let vaa_idr = !pVaa.is_null() && (*pVaa).bIdrPeriodFlag;

        if !(*pSvcParam).bEnableSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < (*pSvcParam).iSpatialLayerNum)
            || ((*pParamInternal).iFrameIndex < (VGOP_SIZE << 1))
        {
            bSceneChangeFlag = false;
        } else if !pVaa.is_null() {
            bSceneChangeFlag = (*pVaa).bSceneChangeFlag;
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

        let first = tab.StrideDecBlockOffset(0, 1);
        let second = tab.StrideDecBlockOffset(0, 1);
        assert_eq!(first, second, "the same table resolves to the same address");

        // The use that matters: the FIRST cursor, after the second derivation.
        unsafe { *first = 0x5A5A };
        assert_eq!(unsafe { *second }, 0x5A5A, "sibling cursors read each other's writes");
        unsafe { *second = 0x3C3C };
        assert_eq!(unsafe { *first }, 0x3C3C);

        // Cursors into the other three regions, live across a re-derivation of this one
        // — `AllocStrideTables` holds exactly this set while it fills the block.
        let enc = tab.StrideEncBlockOffset(0);
        let x = tab.MbIndexX(0);
        let y = tab.MbIndexY(0);
        let dec_again = tab.StrideDecBlockOffset(0, 1);
        unsafe {
            *enc = 7;
            *x = 3;
            *y = 4;
        }
        assert_eq!(unsafe { *dec_again }, 0x3C3C, "re-deriving did not disturb the region");
        assert_eq!(unsafe { (*enc, *x, *y) }, (7, 3, 4));

        assert_eq!(tab.StrideDecBlockOffset(1, 1), first, "two layers, one region");
        assert!(tab.MbIndexX(3).is_null(), "None answers the null the field used to hold");
        assert!(tab.StrideEncBlockOffset(3).is_null());
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
    fn frame_bs_cursors_are_siblings() {
        let mut ctx = Box::new(sWelsEncCtx::new());
        let p: *mut sWelsEncCtx = &mut *ctx;
        // Before `RequestMemorySvc`, both answer the null the raw field held.
        assert!(unsafe { ctx_frame_bs(p) }.is_null());
        assert!(unsafe { ctx_frame_bs_at(p, 0) }.is_null());

        ctx.pFrameBs = vec![0u8; 64];
        ctx.iFrameBsSize = 64;
        let p: *mut sWelsEncCtx = &mut *ctx;

        // `pBsBuf` — the root, stored and kept, exactly as the three sites that take
        // it do.
        let stored = unsafe { ctx_frame_bs(p) };

        // The frame loop then walks: derive at the cursor, write, advance, repeat.
        for i in 0..8i32 {
            let at = unsafe { ctx_frame_bs_at(p, i) };
            unsafe { *at = 0xA0 | i as u8 };
        }
        // The use that matters: the FIRST cursor, after eight later derivations.
        assert_eq!(unsafe { *stored }, 0xA0, "the stored pBsBuf still reaches the buffer");
        unsafe { *stored.add(8) = 0x5A };
        assert_eq!(unsafe { *ctx_frame_bs_at(p, 8) }, 0x5A);

        // And the whole buffer reads back through the container, which is the point
        // of owning it: no third party is needed to free or bound it.
        assert_eq!(&ctx.pFrameBs[..4], &[0xA0, 0xA1, 0xA2, 0xA3]);
        assert_eq!(ctx.pFrameBs.len(), ctx.iFrameBsSize as usize);
    }

    /// The same property through the context's four accessors, which is how every
    /// consumer outside `AllocStrideTables` reaches the tables.
    #[test]
    fn ctx_stride_accessors_are_sibling_derivations() {
        let mut ctx = Box::new(sWelsEncCtx::new());
        let p: *mut sWelsEncCtx = &mut *ctx;
        // Before `AllocStrideTables` runs, all four answer null — the value the raw
        // `pStrideTab` held, and the question every `is_null()` guard was written to ask.
        assert!(unsafe { ctx_stride_enc_block_offset(p, 0) }.is_null());
        assert!(unsafe { ctx_stride_dec_block_offset(p, 0, 1) }.is_null());
        assert!(unsafe { ctx_mb_index_x(p, 0) }.is_null());
        assert!(unsafe { ctx_mb_index_y(p, 0) }.is_null());

        let mut tab = SStrideTables::new(96 * 2);
        tab.pStrideEncBlockOffset[0] = Some(0);
        tab.pStrideEncBlockOffset[1] = Some(96);
        ctx.pStrideTab = Some(Box::new(tab));

        let p: *mut sWelsEncCtx = &mut *ctx;
        let first = unsafe { ctx_stride_enc_block_offset(p, 0) };
        let other = unsafe { ctx_stride_enc_block_offset(p, 1) };
        let again = unsafe { ctx_stride_enc_block_offset(p, 0) };
        unsafe {
            *first = 11;
            *other = 22;
        }
        assert_eq!(unsafe { (*again, *first, *other) }, (11, 11, 22));
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
        pSliceThreading, pTaskManage, eRefStrategy, pEncPic,
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
        pDqIdcMap, sPSOVector, pPSOVector, pMemAlign,
        uiStartTimestamp, sEncoderStatistics, iStatisticsLogInterval, iLastStatisticsLogTs,
        iEncoderError, mutexEncoderError, bDeliveryFlag, sWelsCabacContexts,
        uiLastTimestamp, pDynamicBsBuffer,
        ];
        assert_eq!(extents.len(), 69, "a field was added or removed without updating this list");

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
        const OWNED: [&str; 5] =
            ["pSpsArray", "pSubsetArray", "pPPSArray", "pDqIdcMap", "pFrameBs"];
        assert!(built.pSpsArray.is_empty(), "new(): no SPS array is allocated yet");
        assert!(built.pSubsetArray.is_empty(), "new(): no subset SPS array is allocated yet");
        assert!(built.pPPSArray.is_empty(), "new(): no PPS array is allocated yet");
        assert!(built.pDqIdcMap.is_empty(), "new(): no dq-idc map is allocated yet");
        assert!(built.pFrameBs.is_empty(), "new(): no frame bitstream is allocated yet");

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
        ctx.pSvcParam = &mut param;
        // T6.G3: the context names its SPS by position, so the test stands up the
        // one-entry array the position indexes into — `RequestMemorySvc`'s job on the
        // live path. `sps` outlives `ctx` in this scope.
        ctx.pSpsArray = vec![sps];
        ctx.iSpsNum = 1;
        ctx.iSps = Some(SpsId(0));
        ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;

        unsafe {
            UpdateFrameNum(&mut ctx, 0);
            assert_eq!(param.sDependencyLayers[0].iFrameNum, 1);
            assert_eq!(ctx.eLastNalPriority[0], EWelsNalRefIdc::NRI_PRI_LOWEST);

            ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;
            LoadBackFrameNum(&mut ctx, 0);
            assert_eq!(param.sDependencyLayers[0].iFrameNum, 0);
        }
    }

    #[test]
    fn test_decide_frame_type() {
        let mut param = SWelsSvcCodingParam::default();
        let mut vaa = SVAAFrameInfo::default();
        let mut ctx = sWelsEncCtx::new();
        param.sDependencyLayers[0].bEncCurFrmAsIdrFlag = true;
        ctx.pSvcParam = &mut param;
        ctx.pVaa = &mut vaa;

        unsafe {
            let ft = DecideFrameType(&mut ctx, 1, 0, false);
            assert_eq!(ft, EVideoFrameType::videoFrameTypeIDR);
        }
    }

    #[test]
    fn test_init_function_pointers() {
        unsafe {
            let mut func_list = SWelsFuncPtrList::default();
            let mut param = SWelsSvcCodingParam::default();
            let mut ctx = sWelsEncCtx::default();
            ctx.pFuncList = &mut func_list;
            ctx.pSvcParam = &mut param;

            let ret = InitFunctionPointers(&mut ctx, &mut param, 0);
            assert_eq!(ret, ENC_RETURN_SUCCESS);

            assert!(func_list.pfDctFourT4.is_some());
            assert!(func_list.pfIDctFourT4.is_some());
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
            assert_eq!(func_list.eEntropyCoder, EntropyCoder::Cavlc);
            InitCoeffFunc(&mut func_list, 0, 1);
            assert_eq!(func_list.eEntropyCoder, EntropyCoder::Cabac);

            assert!(func_list.pfDeblocking.pfDeblockingFilterSlice.is_some());
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_FMA, WELS_CPU_MMX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE, WELS_CPU_SSE2, WELS_CPU_SSE3, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3};
