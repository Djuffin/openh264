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
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SStrideTables {
    pub pStrideDecBlockOffset: [[*mut i32; 2]; MAX_DEPENDENCY_LAYER],
    pub pStrideEncBlockOffset: [*mut i32; MAX_DEPENDENCY_LAYER],
    pub pMbIndexX: [*mut i16; MAX_DEPENDENCY_LAYER],
    pub pMbIndexY: [*mut i16; MAX_DEPENDENCY_LAYER],
}

impl Default for SStrideTables {
    fn default() -> Self {
        Self {
            pStrideDecBlockOffset: [[std::ptr::null_mut(); 2]; MAX_DEPENDENCY_LAYER],
            pStrideEncBlockOffset: [std::ptr::null_mut(); MAX_DEPENDENCY_LAYER],
            pMbIndexX: [std::ptr::null_mut(); MAX_DEPENDENCY_LAYER],
            pMbIndexY: [std::ptr::null_mut(); MAX_DEPENDENCY_LAYER],
        }
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
    pub pStrideTab: *mut SStrideTables,
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
    pub pSpsArray: *mut SWelsSPS,
    pub pSps: *mut SWelsSPS,
    pub pPPSArray: *mut SWelsPPS,
    pub pPps: *mut SWelsPPS,
    pub pSubsetArray: *mut SSubsetSps,
    pub pSubsetSps: *mut SSubsetSps,
    pub iSpsNum: i32,
    pub iSubsetSpsNum: i32,
    pub iPpsNum: i32,
    pub pOut: *mut SWelsEncoderOutput,
    pub pFrameBs: *mut u8,
    pub iFrameBsSize: i32,
    pub iPosBsBuffer: i32,
    pub sSpatialIndexMap: [SSpatialPicIndex; MAX_DEPENDENCY_LAYER],
    pub iSliceBufferSize: [i32; MAX_DEPENDENCY_LAYER],
    pub bRefOfCurTidIsLtr: [[bool; MAX_TEMPORAL_LEVEL]; MAX_DEPENDENCY_LAYER],
    pub iMaxSliceCount: i32,
    pub iActiveThreadsNum: i16,
    pub pDqIdcMap: *mut SDqIdc,
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
            pStrideTab: std::ptr::null_mut(),
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
            pSpsArray: std::ptr::null_mut(),
            pSps: std::ptr::null_mut(),
            pPPSArray: std::ptr::null_mut(),
            pPps: std::ptr::null_mut(),
            pSubsetArray: std::ptr::null_mut(),
            pSubsetSps: std::ptr::null_mut(),
            iSpsNum: 0,
            iSubsetSpsNum: 0,
            iPpsNum: 0,

            // ---- output bitstream ------------------------------------------------
            pOut: std::ptr::null_mut(),
            pFrameBs: std::ptr::null_mut(),
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

            pDqIdcMap: std::ptr::null_mut(),

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
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || (*pEncCtx).pSps.is_null() {
        return;
    }
    let pParamInternal = std::ptr::addr_of_mut!((*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize]);
    let mut bNeedFrameNumIncreasing = false;

    if (*pEncCtx).eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        let max_frame_num_minus1 = (1 << (*(*pEncCtx).pSps).uiLog2MaxFrameNum) - 1;
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
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || (*pEncCtx).pSps.is_null() {
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
            (*pParamInternal).iFrameNum = (1 << (*(*pEncCtx).pSps).uiLog2MaxFrameNum) - 1;
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
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || (*pEncCtx).pSps.is_null() {
        return;
    }
    let pParamInternal = std::ptr::addr_of_mut!((*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize]);

    if keFrameType == EVideoFrameType::videoFrameTypeP {
        (*pParamInternal).iFrameIndex += 1;

        let max_poc_boundary = (1 << (*(*pEncCtx).pSps).iLog2MaxPocLsb) - 2;
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
        let max_poc_boundary = (1 << (*(*pEncCtx).pSps).iLog2MaxPocLsb) - 2;
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
    /// # F64 — the seven fields that cannot be compared as bytes at all
    ///
    /// The first draft *was* a field-extent walk and Miri failed it anyway, at
    /// `pEncPic`: **a niche-optimized `Option` writes the niche and leaves the rest
    /// of the payload undefined**, so `None` of an `Option<SrcPicId>` is four defined
    /// bytes (the `NonZeroU32` index, zero) followed by four *uninitialized* ones
    /// (`pool::Id`'s generation counter, which exists only under
    /// `debug_assertions`). The shell wrote zeros there. `new()` does not. The
    /// same holds one level down, inside `SSpatialPicIndex::pSrc`; and **interior
    /// `repr(C)` padding does it again without any niche at all** —
    /// `SParaSetOffsetVariable` has 3 bytes between `bUsedParaSetIdInBs[57]` and
    /// `uiNextParaSetIdToUseInBs`, and `TagVideoEncoderStatistics` has 4 before
    /// `iStatisticsTs` and 4 of tail, none of which a struct literal writes.
    ///
    /// So the honest statement is narrower than "byte-identical", and it is the
    /// narrower one that is true: `new()` reproduces the shell **everywhere the
    /// shell's bytes are defined by the type**, and at the seven fields below it
    /// reproduces the *values*, which is all anything reads. Nothing reads a `None`
    /// handle's generation — that is read when a `Some` is resolved, and there is no
    /// `Some` here — and nothing reads padding at all. The seven are excluded **by
    /// name** and asserted **by value**; excluding them silently would have been the
    /// failure this test exists to prevent, one level up.
    ///
    /// The general rule this leaves behind: *a field-wise constructor cannot be
    /// proved byte-equal to a memset image, only value-equal, and the difference is
    /// exactly the bytes the type does not define.*
    #[test]
    fn ctx_new_reproduces_the_zeroed_shell() {
        use std::mem::{offset_of, size_of, size_of_val};

        let built = Box::new(sWelsEncCtx::new());
        let shell: Box<sWelsEncCtx> = Box::new(unsafe { std::mem::zeroed() });

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
        pVaa, pVpp, pSpsArray, pSps,
        pPPSArray, pPps, pSubsetArray, pSubsetSps,
        iSpsNum, iSubsetSpsNum, iPpsNum, pOut,
        pFrameBs, iFrameBsSize, iPosBsBuffer, sSpatialIndexMap,
        iSliceBufferSize, bRefOfCurTidIsLtr, iMaxSliceCount, iActiveThreadsNum,
        pDqIdcMap, sPSOVector, pPSOVector, pMemAlign,
        uiStartTimestamp, sEncoderStatistics, iStatisticsLogInterval, iLastStatisticsLogTs,
        iEncoderError, mutexEncoderError, bDeliveryFlag, sWelsCabacContexts,
        uiLastTimestamp, pDynamicBsBuffer,
        ];
        assert_eq!(extents.len(), 70, "a field was added or removed without updating this list");

        // ---- tier 2: the F64 fields, excluded by name and asserted by value ----
        const BY_VALUE: [&str; 7] = [
            // niche-carrying: `None` leaves pool::Id's generation half undefined
            "pEncPic", "pDecPic", "pRefPic", "pRefList0", "sSpatialIndexMap",
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

        for (which, c) in [("new()", &built), ("shell", &shell)] {
            assert!(c.pEncPic.is_none(), "{which}: no source picture is bound");
            assert!(c.pDecPic.is_none(), "{which}: nothing is being reconstructed into");
            assert!(c.pRefPic.is_none(), "{which}: no reference picture is bound");
            assert!(c.pRefList0.iter().all(|h| h.is_none()), "{which}: list 0 is empty");
            assert!(
                c.sSpatialIndexMap.iter().all(|e| e.pSrc.is_none() && e.iDid == 0),
                "{which}: the spatial index map holds no pictures"
            );
            assert!(paraset_is_zero(&c.sPSOVector), "{which}: no parameter-set id handed out");
            assert!(
                c.sEncoderStatistics.iter().all(stats_are_zero),
                "{which}: nothing has been encoded"
            );
        }

        let a = (&*built as *const sWelsEncCtx).cast::<u8>();
        let b = (&*shell as *const sWelsEncCtx).cast::<u8>();

        // ---- tier 1: everything else, byte for byte, attributed by name -------
        let (mut compared, mut excluded) = (0usize, 0usize);
        let mut diffs: Vec<String> = Vec::new();
        for (name, off, len) in &extents {
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
        assert!(compared > 0 && compared + excluded <= total);
        println!(
            "ctx_new_reproduces_the_zeroed_shell: {compared}/{total} bytes compared byte-wise \
             across {} fields, {excluded} in the {} F64 fields (compared by value), {} of \
             inter-field repr(C) padding",
            extents.len() - BY_VALUE.len(),
            BY_VALUE.len(),
            total - compared - excluded
        );
    }

    #[test]
    fn test_update_and_loadback_framenum() {
        let mut param = SWelsSvcCodingParam::default();
        // Only the fields this test exercises; SWelsSPS is now the full
        // parameter_sets.h:43 struct rather than the four-field copy that used to
        // live in this module.
        let mut sps = SWelsSPS {
            uiLog2MaxFrameNum: 4,
            iLog2MaxPocLsb: 4,
            bFrameCroppingFlag: false,
            sFrameCrop: SCropOffset::default(),
            ..Default::default()
        };
        let mut ctx = sWelsEncCtx::new();
        ctx.pSvcParam = &mut param;
        ctx.pSps = &mut sps;
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
