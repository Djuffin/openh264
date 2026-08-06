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
pub const WELS_CPU_MMX: u32 = 0x00000001;
pub const WELS_CPU_MMXEXT: u32 = 0x00000002;
pub const WELS_CPU_SSE: u32 = 0x00000004;
pub const WELS_CPU_SSE2: u32 = 0x00000008;
pub const WELS_CPU_SSE3: u32 = 0x00000010;
pub const WELS_CPU_SSSE3: u32 = 0x00000020;
pub const WELS_CPU_SSE41: u32 = 0x00000040;
pub const WELS_CPU_SSE42: u32 = 0x00000080;
pub const WELS_CPU_AVX: u32 = 0x00000100;
pub const WELS_CPU_FMA: u32 = 0x00000200;
pub const WELS_CPU_AVX2: u32 = 0x00000400;
pub const WELS_CPU_NEON: u32 = 0x00000800;

// Complexity modes come from api::codec_api::ECOMPLEXITY_MODE.

// Intra Prediction Mode Count Constants
pub const I16_PRED_DC_A: usize = 7;
pub const I4_PRED_A: usize = 14;
pub const C_PRED_A: usize = 7;
pub const BLOCK_STATIC_IDC_ALL: usize = 5;
/// `wels_const.h:147` — last variant of the block-size enum, value 7. This was 8 here,
/// which would have over-sized `SScreenBlockFeatureStorage::uiSadCostThreshold` and the
/// `SSampleDealingFunc` / `SWelsFuncPtrList` function-pointer tables.
pub const BLOCK_SIZE_ALL: usize = 7;

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

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsSliceType {
    #[default]
    P_SLICE = 0,
    B_SLICE = 1,
    I_SLICE = 2,
    SP_SLICE = 3,
    SI_SLICE = 4,
    UNKNOWN_SLICE = 5,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ESceneChangeIdc {
    #[default]
    NO_SCENE_CHANGE = 0,
    SIMILAR_SCENE = 1,
    LARGE_CHANGED_SCENE = 2,
}

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

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsSPS {
    pub uiLog2MaxFrameNum: u32,
    pub iLog2MaxPocLsb: i32,
    pub bFrameCroppingFlag: bool,
    pub sFrameCrop: SCropOffset,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsPPS {
    pub iPicInitQp: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSubsetSps {
    pub pSps: SWelsSPS,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLayerInfo {
    pub pSpsP: *mut SWelsSPS,
    pub pSubsetSpsP: *mut SSubsetSps,
}

impl Default for SLayerInfo {
    fn default() -> Self {
        Self {
            pSpsP: std::ptr::null_mut(),
            pSubsetSpsP: std::ptr::null_mut(),
        }
    }
}

pub use crate::encoder::svc_encode_slice::SDqLayer;

pub use crate::encoder::wels_encoder_ext::{SSpatialLayerInternal, SWelsSvcCodingParam};

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVAAFrameInfo {
    pub bIdrPeriodFlag: bool,
    pub bSceneChangeFlag: bool,
    pub eSceneChangeIdc: ESceneChangeIdc,
}

pub use crate::common::wels_common_defs::SBitStringAux;

pub use crate::encoder::nal_encap::SWelsEncoderOutput;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SStateCtx {
    pub uiState: u8,
    pub uiMPS: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SParaSetOffset {
    pub sParaSetOffsetArray: [i32; 32],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDqIdc {
    pub uiDId: u8,
    pub uiQId: u8,
    pub uiTId: u8,
}

pub use crate::encoder::svc_encode_slice::{SMB, SSlice};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceThreading {
    pub uiSliceNum: i32,
    pub mutexSliceNumUpdate: *mut c_void,
    pub pThreadBsBuffer: [*mut u8; MAX_THREADS_NUM],
}

impl Default for SSliceThreading {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub use crate::encoder::svc_encode_slice::SWelsSvcRc;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SExpandPicFunc {
    pub pfExpandPicLuma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>,
    pub pfExpandPicChroma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMcFunc {
    pub pfLumaHalfPelHoriz: Option<unsafe extern "C" fn(*mut u8, i32, *const u8, i32, i32, i32)>,
    pub pfLumaHalfPelVert: Option<unsafe extern "C" fn(*mut u8, i32, *const u8, i32, i32, i32)>,
    pub pfChromaInterpolation: Option<unsafe extern "C" fn(*mut u8, i32, *const u8, i32, i32, i32, i32, i32)>,
}

pub use crate::encoder::deblocking::DeblockingFunc as SDeblockingFunc;

pub use crate::encoder::rc::SWelsRcFunc;
pub use crate::encoder::nal_encap::EWelsNalUnitType;
pub use crate::encoder::nal_encap::EWelsNalRefIdc;
pub use crate::encoder::picture::SPicture;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsFuncPtrList {
    pub sExpandPicFunc: SExpandPicFunc,
    pub sMcFuncs: SMcFunc,
    pub pfDeblocking: SDeblockingFunc,
    pub pfSetMemZeroSize8: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    pub pfSetMemZeroSize64Aligned16: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    pub pfSetMemZeroSize64: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    pub pfInterMdBackgroundDecision: Option<
        unsafe extern "C" fn(*mut sWelsEncCtx, *mut c_void, *mut c_void, *mut SMB, *mut c_void, *mut bool) -> bool,
    >,
    pub pfMdBackgroundInfoUpdate: Option<
        unsafe extern "C" fn(*mut SDqLayer, *mut SMB, bool, i32),
    >,
    pub pfDctFourT4: Option<unsafe extern "C" fn(pDct: *mut i16, pSample1: *mut u8, iStride1: i32, pSample2: *mut u8, iStride2: i32)>,
    pub pfIDctFourT4: Option<unsafe extern "C" fn(pRec: *mut u8, iStride: i32, pPred: *mut u8, iPredStride: i32, pRes: *mut i16)>,
    pub pfInterMd: Option<unsafe extern "C" fn(pCtx: *mut sWelsEncCtx, pMd: *mut crate::encoder::svc_encode_slice::SWelsMD, pSlice: *mut SSlice, pCurMb: *mut SMB, pMbCache: *mut crate::encoder::svc_encode_slice::SMbCache)>,
    pub pfWelsSpatialWriteMbSyn: Option<unsafe extern "C" fn(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, pCurMb: *mut SMB) -> i32>,
    pub pfStashMBStatus: Option<unsafe extern "C" fn(pDss: *mut crate::encoder::svc_encode_slice::SDynamicSlicingStack, pSlice: *mut SSlice, iMbSkipRun: i32)>,
    pub pfStashPopMBStatus: Option<unsafe extern "C" fn(pDss: *mut crate::encoder::svc_encode_slice::SDynamicSlicingStack, pSlice: *mut SSlice) -> i32>,
    pub pfGetBsPosition: Option<unsafe extern "C" fn(pSlice: *mut SSlice) -> i32>,
    pub pfSetNZCZero: Option<unsafe extern "C" fn(*mut i8, i32)>,
    pub pfRc: SWelsRcFunc,
    pub pParametersetStrategy: *mut c_void,
}

// ============================================================================
// Primary Encoder Context Data Structures (encoder_context.h)
// ============================================================================

/// Reference picture lists for each spatial dependency/quality layer in SVC.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefList {
    pub pShortRefList: [*mut SPicture; 1 + MAX_SHORT_REF_COUNT],
    pub pLongRefList: [*mut SPicture; 1 + MAX_REF_PIC_COUNT],
    pub pNextBuffer: *mut SPicture,
    pub pRef: [*mut SPicture; 1 + MAX_REF_PIC_COUNT],
    pub uiShortRefCount: u8,
    pub uiLongRefCount: u8,
}

impl Default for SRefList {
    fn default() -> Self {
        Self {
            pShortRefList: [std::ptr::null_mut(); 1 + MAX_SHORT_REF_COUNT],
            pLongRefList: [std::ptr::null_mut(); 1 + MAX_REF_PIC_COUNT],
            pNextBuffer: std::ptr::null_mut(),
            pRef: [std::ptr::null_mut(); 1 + MAX_REF_PIC_COUNT],
            uiShortRefCount: 0,
            uiLongRefCount: 0,
        }
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
    pub pSrc: *mut SPicture,
    pub iDid: i32,
}

impl Default for SSpatialPicIndex {
    fn default() -> Self {
        Self {
            pSrc: std::ptr::null_mut(),
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
    pub pSadCostMb: *mut i32,
    pub iMvRange: i32,
    pub pMvdCostTable: *mut u16,
    pub iMvdCostTableSize: i32,
    pub iMvdCostTableStride: i32,
    pub pMvUnitBlock4x4: *mut SMVUnitXY,
    pub pRefIndexBlock4x4: *mut i8,
    pub pNonZeroCountBlocks: *mut i8,
    pub pIntra4x4PredModeBlocks: *mut i8,
    pub ppMbListD: *mut *mut SMB,
    pub pStrideTab: *mut SStrideTables,
    pub pFuncList: *mut SWelsFuncPtrList,
    pub pSliceThreading: *mut SSliceThreading,
    pub pTaskManage: *mut c_void,
    pub pReferenceStrategy: *mut c_void,
    pub pEncPic: *mut SPicture,
    pub pDecPic: *mut SPicture,
    pub pRefPic: *mut SPicture,
    pub pCurDqLayer: *mut SDqLayer,
    pub ppDqLayerList: *mut *mut SDqLayer,
    pub ppRefPicListExt: *mut *mut SRefList,
    pub pRefList0: [*mut SPicture; 16],
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
    pub pVpp: *mut c_void,
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

impl Default for sWelsEncCtx {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// ============================================================================
// Background Detection Fallback Callbacks
// ============================================================================

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskip(
    _pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: *mut c_void,
    _slice: *mut c_void,
    _pCurMb: *mut SMB,
    _pMbCache: *mut c_void,
    _pKeepPskip: *mut bool,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfo(
    _pCurLayer: *mut SDqLayer,
    _pCurMb: *mut SMB,
    _bFlag: bool,
    _kiRefPictureType: i32,
) {}

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskipFalse(
    _pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: *mut c_void,
    _slice: *mut c_void,
    _pCurMb: *mut SMB,
    _pMbCache: *mut c_void,
    _pKeepPskip: *mut bool,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfoNULL(
    _pCurLayer: *mut SDqLayer,
    _pCurMb: *mut SMB,
    _bFlag: bool,
    _kiRefPictureType: i32,
) {}

// ============================================================================
// Core Encoder Functions (encoder.cpp)
// ============================================================================

/// Initializes input source picture geometry, color planes, and line strides.
///
/// # Safety
/// `kpSrc` must point to a valid `SSourcePicture` struct or be null.
pub unsafe fn InitPic(
    kpSrc: *const c_void,
    kiColorspace: i32,
    kiWidth: i32,
    kiHeight: i32,
) -> i32 {
    let pSrcPic = kpSrc as *mut SSourcePicture;

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

    (*pFuncList).pfSetMemZeroSize8 = Some(WelsSetMemZero_c_extern);
    (*pFuncList).pfSetMemZeroSize64Aligned16 = Some(WelsSetMemZero_c_extern);
    (*pFuncList).pfSetMemZeroSize64 = Some(WelsSetMemZero_c_extern);

    WelsInitBGDFunc(pFuncList, (*pParam).bEnableBackgroundDetection);

    (*pFuncList).pfDctFourT4 = Some(crate::encoder::encode_mb_aux::WelsDctFourT4_c);
    (*pFuncList).pfIDctFourT4 = Some(crate::encoder::svc_encode_mb::WelsIDctFourT4_c);

    (*pFuncList).pfInterMd = Some(std::mem::transmute(
        crate::encoder::svc_mode_decision::WelsMdSpatialelInterMbIlfmdNoilp as *const ()
    ));
    (*pFuncList).pfWelsSpatialWriteMbSyn = Some(std::mem::transmute(
        crate::encoder::svc_set_mb_syn_cavlc::WelsSpatialWriteMbSyn as *const ()
    ));
    (*pFuncList).pfStashMBStatus = Some(std::mem::transmute(
        crate::encoder::svc_set_mb_syn_cavlc::StashMBStatusCavlc as *const ()
    ));
    (*pFuncList).pfStashPopMBStatus = Some(std::mem::transmute(
        crate::encoder::svc_set_mb_syn_cavlc::StashPopMBStatusCavlc as *const ()
    ));
    (*pFuncList).pfGetBsPosition = Some(std::mem::transmute(
        crate::encoder::svc_set_mb_syn_cavlc::GetBsPosCavlc as *const ()
    ));

    crate::encoder::deblocking::DeblockingInit(
        &mut (*pFuncList).pfDeblocking as *mut _,
        _uiCpuFlag as i32,
    );

    crate::encoder::rc::WelsRcInitFuncPointers(
        &mut (*pFuncList).pfRc,
        (*pParam).iRCMode,
    );

    ENC_RETURN_SUCCESS
}

/// Increments the H.264 slice header `frame_num` syntax element for spatial layer `kiDidx`.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
pub unsafe fn UpdateFrameNum(pEncCtx: *mut sWelsEncCtx, kiDidx: i32) {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || (*pEncCtx).pSps.is_null() {
        return;
    }
    let pParamInternal = &mut (*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize];
    let mut bNeedFrameNumIncreasing = false;

    if (*pEncCtx).eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        let max_frame_num_minus1 = (1 << (*(*pEncCtx).pSps).uiLog2MaxFrameNum) - 1;
        if pParamInternal.iFrameNum < max_frame_num_minus1 {
            pParamInternal.iFrameNum += 1;
        } else {
            pParamInternal.iFrameNum = 0;
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
    let pParamInternal = &mut (*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize];
    let mut bNeedFrameNumIncreasing = false;

    if (*pEncCtx).eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        if pParamInternal.iFrameNum != 0 {
            pParamInternal.iFrameNum -= 1;
        } else {
            pParamInternal.iFrameNum = (1 << (*(*pEncCtx).pSps).uiLog2MaxFrameNum) - 1;
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

    // `uiSize` is uint32_t in C++ and InitBits takes int32_t; the narrowing conversion
    // is implicit there, explicit here.
    crate::encoder::vlc_encoder::InitBits(
        &mut (*(*pEncCtx).pOut).sBsWrite,
        (*(*pEncCtx).pOut).pBsBuffer,
        (*(*pEncCtx).pOut).uiSize as i32,
    );
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
    let pParamInternal = &mut (*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize];

    if keFrameType == EVideoFrameType::videoFrameTypeP {
        pParamInternal.iFrameIndex += 1;

        let max_poc_boundary = (1 << (*(*pEncCtx).pSps).iLog2MaxPocLsb) - 2;
        if pParamInternal.iPOC < max_poc_boundary {
            pParamInternal.iPOC += 2;
        } else {
            pParamInternal.iPOC = 0;
        }

        UpdateFrameNum(pEncCtx, kiDidx);

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        (*pEncCtx).eSliceType = EWelsSliceType::P_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGH;
    } else if keFrameType == EVideoFrameType::videoFrameTypeIDR {
        pParamInternal.iFrameNum = 0;
        pParamInternal.iPOC = 0;
        pParamInternal.bEncCurFrmAsIdrFlag = false;
        pParamInternal.iFrameIndex = 0;

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
        (*pEncCtx).eSliceType = EWelsSliceType::I_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;

        pParamInternal.iCodingIndex = 0;
    } else if keFrameType == EVideoFrameType::videoFrameTypeI {
        let max_poc_boundary = (1 << (*(*pEncCtx).pSps).iLog2MaxPocLsb) - 2;
        if pParamInternal.iPOC < max_poc_boundary {
            pParamInternal.iPOC += 2;
        } else {
            pParamInternal.iPOC = 0;
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
    let pParamInternal = &mut (*pSvcParam).sDependencyLayers[kiDidx as usize];
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
            || pParamInternal.bEncCurFrmAsIdrFlag
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
                    let pLongTermRefList = (*ref_list_0).pLongRefList.as_ptr();
                    for i in 0..(*pSvcParam).iLTRRefNum {
                        let pic = *pLongTermRefList.add(i as usize);
                        if !pic.is_null()
                            && (*pic).bUsedAsRef
                            && (*pic).bIsLongRef
                            && (*pic).bIsSceneLTR
                        {
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
            pParamInternal.iCodingIndex = 0;
            (*pEncCtx).bCurFrameMarkedAsSceneLtr = true;
        }
    } else {
        let pVaa = (*pEncCtx).pVaa;
        let vaa_idr = !pVaa.is_null() && (*pVaa).bIdrPeriodFlag;

        if !(*pSvcParam).bEnableSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < (*pSvcParam).iSpatialLayerNum)
            || (pParamInternal.iFrameIndex < (VGOP_SIZE << 1))
        {
            bSceneChangeFlag = false;
        } else if !pVaa.is_null() {
            bSceneChangeFlag = (*pVaa).bSceneChangeFlag;
        }

        iFrameType = if vaa_idr || bSceneChangeFlag || pParamInternal.bEncCurFrmAsIdrFlag {
            EVideoFrameType::videoFrameTypeIDR
        } else {
            EVideoFrameType::videoFrameTypeP
        };

        if iFrameType == EVideoFrameType::videoFrameTypeP && bSkipFrameFlag {
            iFrameType = EVideoFrameType::videoFrameTypeSkip;
        } else if iFrameType == EVideoFrameType::videoFrameTypeIDR {
            pParamInternal.iCodingIndex = 0;
        }
    }

    iFrameType
}

/// Portable C/C++ fallback for memory clearing.
///
/// # Safety
/// `pDst` must point to valid writable memory of at least `iSize` bytes.
pub unsafe fn WelsSetMemZero_c(pDst: *mut c_void, iSize: i32) {
    if !pDst.is_null() && iSize > 0 {
        std::ptr::write_bytes(pDst as *mut u8, 0, iSize as usize);
    }
}

pub unsafe extern "C" fn WelsSetMemZero_c_extern(pDst: *mut c_void, iSize: i32) {
    WelsSetMemZero_c(pDst, iSize);
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
                &mut src_pic as *mut SSourcePicture as *mut c_void,
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

    #[test]
    fn test_update_and_loadback_framenum() {
        let mut param = SWelsSvcCodingParam::default();
        let mut sps = SWelsSPS {
            uiLog2MaxFrameNum: 4,
            iLog2MaxPocLsb: 4,
            bFrameCroppingFlag: false,
            sFrameCrop: SCropOffset::default(),
        };
        let mut ctx = unsafe { std::mem::zeroed::<sWelsEncCtx>() };
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
        let mut ctx = unsafe { std::mem::zeroed::<sWelsEncCtx>() };
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

            assert!(func_list.pfSetMemZeroSize8.is_some());
            assert!(func_list.pfSetMemZeroSize64Aligned16.is_some());
            assert!(func_list.pfSetMemZeroSize64.is_some());

            assert!(func_list.pfDctFourT4.is_some());
            assert!(func_list.pfIDctFourT4.is_some());
            assert!(func_list.pfInterMd.is_some());
            assert!(func_list.pfWelsSpatialWriteMbSyn.is_some());
            assert!(func_list.pfStashMBStatus.is_some());
            assert!(func_list.pfStashPopMBStatus.is_some());
            assert!(func_list.pfGetBsPosition.is_some());

            assert!(func_list.pfDeblocking.pfDeblockingFilterSlice.is_some());
        }
    }
}
