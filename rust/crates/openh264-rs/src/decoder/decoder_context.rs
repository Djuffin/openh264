#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! Master decoder execution context, function dispatch tables, DPB memory pool management,
//! bitstream demuxing, and statistics aggregation.

use crate::decoder::bit_stream::BsReader;
use crate::decoder::decode_slice::IntraPredConstraint;
use crate::decoder::error_concealment::{ERROR_CON_IDC, SMcFunc};
use crate::decoder::fmo::SFmo;
use crate::decoder::parse_mb_syn_cavlc::SVlcTable;
use crate::decoder::slice::EWelsSliceType;
use crate::safe::plane::PlaneCursorMut;
use std::ffi::c_void;

// ---------------------------------------------------------------------------
// Constants & Defines
// ---------------------------------------------------------------------------

pub const MAX_PRED_MODE_ID_I16x16: usize = 3;
pub const MAX_PRED_MODE_ID_CHROMA: usize = 3;
pub const MAX_PRED_MODE_ID_I4x4: usize = 8;
pub const WELS_QP_MAX: usize = 51;

pub const IMinInt32: i32 = -0x7FFFFFFF;

pub const NEW_CTX_OFFSET_MB_TYPE_I: usize = 3;
pub const NEW_CTX_OFFSET_SKIP: usize = 11;
pub const NEW_CTX_OFFSET_SUBMB_TYPE: usize = 21;
pub const NEW_CTX_OFFSET_B_SUBMB_TYPE: usize = 36;
pub const NEW_CTX_OFFSET_MVD: usize = 40;
pub const NEW_CTX_OFFSET_REF_NO: usize = 54;
pub const NEW_CTX_OFFSET_DELTA_QP: usize = 60;
pub const NEW_CTX_OFFSET_IPR: usize = 68;
pub const NEW_CTX_OFFSET_CIPR: usize = 64;
pub const NEW_CTX_OFFSET_CBP: usize = 73;
pub const NEW_CTX_OFFSET_CBF: usize = 85;
pub const NEW_CTX_OFFSET_MAP: usize = 105;
pub const NEW_CTX_OFFSET_LAST: usize = 166;
pub const NEW_CTX_OFFSET_ONE: usize = 227;
pub const NEW_CTX_OFFSET_ABS: usize = 232;
pub const NEW_CTX_OFFSET_TS_8x8_FLAG: usize = 399;
pub const CTX_NUM_MVD: usize = 7;
pub const CTX_NUM_CBP: usize = 4;
pub const NEW_CTX_OFFSET_TRANSFORM_SIZE_8X8_FLAG: usize = 399;
pub const NEW_CTX_OFFSET_MAP_8x8: usize = 402;
pub const NEW_CTX_OFFSET_LAST_8x8: usize = 417;
pub const NEW_CTX_OFFSET_ONE_8x8: usize = 426;
pub const NEW_CTX_OFFSET_ABS_8x8: usize = 431;

pub const SPS_PPS_BS_SIZE: usize = 128;

pub const OVERWRITE_NONE: i32 = 0;
pub const OVERWRITE_PPS: i32 = 1;
pub const OVERWRITE_SPS: i32 = 1 << 1;
pub const OVERWRITE_SUBSETSPS: i32 = 1 << 2;

pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MIN_REF_PIC_COUNT: usize = 1;
pub const MAX_SHORT_REF_COUNT: usize = 16;
pub const MAX_LONG_REF_COUNT: usize = 16;
pub const MAX_DPB_COUNT: usize = MAX_REF_PIC_COUNT + 1; // 17
pub const MAX_SPS_COUNT: usize = 32;
pub const MAX_PPS_COUNT: usize = 256;
pub const MAX_LAYER_NUM: usize = 8;
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MV_A: usize = 2;
pub const MB_COEFF_LIST_SIZE: usize = 256 + ((8 * 8) << 1); // 384
pub const MB_PARTITION_SIZE: usize = 4;
pub const MB_SUB_PARTITION_SIZE: usize = 4;
pub const WELS_CONTEXT_COUNT: usize = 460;

// Return & Error codes
pub const ERR_NONE: i32 = 0;
pub const ERR_INFO_INVALID_PARAM: i32 = 1;
pub const ERR_INFO_OUT_OF_MEMORY: i32 = 2;
pub const ERR_INFO_INVALID_PTR: i32 = 3;

pub const dsErrorFree: i32 = 0x00;
pub const dsFramePending: i32 = 0x01;
pub const dsRefLost: i32 = 0x02;
pub const dsBitstreamError: i32 = 0x04;
pub const dsDepLayerLost: i32 = 0x08;
pub const dsNoParamSets: i32 = 0x10;
pub const dsDataErrorConcealed: i32 = 0x20;
pub const dsRefListNullPtrs: i32 = 0x40;
pub const dsInvalidArgument: i32 = 0x1000;
pub const dsInitialOptExpected: i32 = 0x2000;
pub const dsOutOfMemory: i32 = 0x4000;
pub const dsDstBufNeedExpan: i32 = 0x8000;

// Error Concealment Modes
pub const ERROR_CON_DISABLE: i32 = 0;
pub const ERROR_CON_FRAME_COPY: i32 = 1;
pub const ERROR_CON_SLICE_COPY: i32 = 2;
pub const ERROR_CON_FRAME_COPY_CROSS_IDR: i32 = 3;
pub const ERROR_CON_SLICE_COPY_CROSS_IDR: i32 = 4;
pub const ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE: i32 = 5;
pub const ERROR_CON_SLICE_MV_COPY_CROSS_IDR: i32 = 6;
pub const ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE: i32 = 7;

// Slice Types
pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;
pub const SP_SLICE: i32 = 3;
pub const SI_SLICE: i32 = 4;

// Prediction Mode Index Identifiers
pub const I16_PRED_V: usize = 0;
pub const I16_PRED_H: usize = 1;
pub const I16_PRED_DC: usize = 2;
pub const I16_PRED_P: usize = 3;
pub const I16_PRED_DC_L: usize = 4;
pub const I16_PRED_DC_T: usize = 5;
pub const I16_PRED_DC_128: usize = 6;

pub const I4_PRED_V: usize = 0;
pub const I4_PRED_H: usize = 1;
pub const I4_PRED_DC: usize = 2;
pub const I4_PRED_DDL: usize = 3;
pub const I4_PRED_DDR: usize = 4;
pub const I4_PRED_VR: usize = 5;
pub const I4_PRED_HD: usize = 6;
pub const I4_PRED_VL: usize = 7;
pub const I4_PRED_HU: usize = 8;
pub const I4_PRED_DC_L: usize = 9;
pub const I4_PRED_DC_T: usize = 10;
pub const I4_PRED_DC_128: usize = 11;
pub const I4_PRED_DDL_TOP: usize = 12;
pub const I4_PRED_VL_TOP: usize = 13;

pub const C_PRED_DC: usize = 0;
pub const C_PRED_H: usize = 1;
pub const C_PRED_V: usize = 2;
pub const C_PRED_P: usize = 3;
pub const C_PRED_DC_L: usize = 4;
pub const C_PRED_DC_T: usize = 5;
pub const C_PRED_DC_128: usize = 6;

// `FEEDBACK_VCL_NAL_IN_AU` — `codec_app_def.h:190-194`, the three values
// `DECODER_OPTION_VCL_NAL` reports.
pub const FEEDBACK_NON_VCL_NAL: i32 = 0;
pub const FEEDBACK_VCL_NAL: i32 = 1;
pub const FEEDBACK_UNKNOWN_NAL: i32 = 2;

// ---------------------------------------------------------------------------
// CABAC & Bitstream Data Structures
// ---------------------------------------------------------------------------

pub use crate::decoder::cabac_decoder::{SWelsCabacCtx, SWelsCabacDecEngine};

pub use crate::decoder::bit_stream::RawDataBuffer;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SSpsBsInfo {
    pub pSpsBsBuf: [u8; SPS_PPS_BS_SIZE],
    pub iSpsId: i32,
    pub uiSpsBsLen: u16,
}

impl Default for SSpsBsInfo {
    fn default() -> Self {
        Self {
            pSpsBsBuf: [0u8; SPS_PPS_BS_SIZE],
            iSpsId: 0,
            uiSpsBsLen: 0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SPpsBsInfo {
    pub pPpsBsBuf: [u8; SPS_PPS_BS_SIZE],
    pub iPpsId: i32,
    pub uiPpsBsLen: u16,
}

impl Default for SPpsBsInfo {
    fn default() -> Self {
        Self {
            pPpsBsBuf: [0u8; SPS_PPS_BS_SIZE],
            iPpsId: 0,
            uiPpsBsLen: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Function Pointer Dispatch Types
// ---------------------------------------------------------------------------

pub type PGetIntraPredFunc = Option<fn(pred: &mut PlaneCursorMut<'_>)>;
pub type PIdctResAddPredFunc = Option<fn(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16])>;
pub type PIdctResAddPred8x8Func = Option<fn(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 64])>;
pub type PIdctFourResAddPredFunc =
    Option<fn(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 64], nzc: &[i8; 6])>;
pub type PGetIntraPred8x8Func =
    Option<fn(pred: &mut PlaneCursorMut<'_>, bTLAvail: bool, bTRAvail: bool)>;

// ---------------------------------------------------------------------------
// Auxiliary Data Structures
// ---------------------------------------------------------------------------

pub use crate::decoder::error_concealment::SCopyFunc;

/// The per-slice deblocking scratch — C++ `SDeblockingFilter`
/// (`decoder_context.h:214-223`).
#[derive(Copy, Clone, Default)]
pub struct SDeblockingFilter {
    pub eSliceType: i32,
    pub iSliceAlphaC0Offset: i8,
    pub iSliceBetaOffset: i8,
    pub iChromaQP: [i8; 2],
    pub iLumaQP: i8,

    /// The two reference lists as identities, snapshotted at filter init.
    ///
    /// Boundary strength only asks whether two entries name the same reference
    /// picture; `None` is an empty slot, and every non-null entry is a pool picture,
    /// so `Option<PicId>` equality is picture identity. Nothing writes a reference
    /// list during deblocking, so the snapshot stays valid.
    pub ref_ids: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
}

pub use crate::decoder::parameter_sets::SPosOffset;

pub use crate::decoder::parameter_sets::{SPps, SSps, SSubsetSps};

pub use crate::decoder::nalu::{SNalUnit, SNalUnitHeader, SNalUnitHeaderExt};

pub use crate::decoder::slice::{SRefBasePicMarking, SSliceHeader, SSliceHeaderExt};

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsDecoderSpsPpsCTX {
    pub sFrameCrop: SPosOffset,
    pub sSpsBuffer: [SSps; MAX_SPS_COUNT + 1],
    pub sPpsBuffer: [SPps; MAX_PPS_COUNT + 1],
    pub sSubsetSpsBuffer: [SSubsetSps; MAX_SPS_COUNT + 1],
    pub sPrefixNal: SNalUnit,
    /// Which SPS each dependency layer activated, as an id. Distinct [`SpsRef`]s
    /// name distinct slots, so `SpsRef` equality is SPS identity.
    pub pActiveLayerSps: [Option<SpsRef>; MAX_LAYER_NUM],
    pub bAvcBasedFlag: bool,
    pub bSpsExistAheadFlag: bool,
    pub bSubspsExistAheadFlag: bool,
    pub bPpsExistAheadFlag: bool,
    pub iSpsErrorIgnored: i32,
    pub iSubSpsErrorIgnored: i32,
    pub iPpsErrorIgnored: i32,
    pub bSpsAvailFlags: [bool; MAX_SPS_COUNT],
    pub bSubspsAvailFlags: [bool; MAX_SPS_COUNT],
    pub bPpsAvailFlags: [bool; MAX_PPS_COUNT],
    pub iPPSLastInvalidId: i32,
    pub iPPSInvalidNum: i32,
    pub iSPSLastInvalidId: i32,
    pub iSPSInvalidNum: i32,
    pub iSubSPSLastInvalidId: i32,
    pub iSubSPSInvalidNum: i32,
    pub iSeqId: i32,
    pub iOverwriteFlags: i32,
}

impl SWelsDecoderSpsPpsCTX {
    /// The all-zero parameter-set context, distinct from
    /// [`SWelsDecoderSpsPpsCTX::default`], where `bAvcBasedFlag` is `true` and the
    /// five `i*LastInvalidId`/`iSeqId` fields are `-1`.
    pub fn memset_zero() -> Self {
        Self {
            sFrameCrop: SPosOffset::default(),
            sSpsBuffer: [SSps::memset_zero(); MAX_SPS_COUNT + 1],
            sPpsBuffer: [SPps::memset_zero(); MAX_PPS_COUNT + 1],
            sSubsetSpsBuffer: [SSubsetSps::memset_zero(); MAX_SPS_COUNT + 1],
            sPrefixNal: SNalUnit::default(),
            pActiveLayerSps: [None; MAX_LAYER_NUM],
            bAvcBasedFlag: false,
            bSpsExistAheadFlag: false,
            bSubspsExistAheadFlag: false,
            bPpsExistAheadFlag: false,
            iSpsErrorIgnored: 0,
            iSubSpsErrorIgnored: 0,
            iPpsErrorIgnored: 0,
            bSpsAvailFlags: [false; MAX_SPS_COUNT],
            bSubspsAvailFlags: [false; MAX_SPS_COUNT],
            bPpsAvailFlags: [false; MAX_PPS_COUNT],
            iPPSLastInvalidId: 0,
            iPPSInvalidNum: 0,
            iSPSLastInvalidId: 0,
            iSPSInvalidNum: 0,
            iSubSPSLastInvalidId: 0,
            iSubSPSInvalidNum: 0,
            iSeqId: 0,
            iOverwriteFlags: 0,
        }
    }
}

impl Default for SWelsDecoderSpsPpsCTX {
    fn default() -> Self {
        Self {
            sFrameCrop: SPosOffset::default(),
            sSpsBuffer: [SSps::default(); MAX_SPS_COUNT + 1],
            sPpsBuffer: [SPps::default(); MAX_PPS_COUNT + 1],
            sSubsetSpsBuffer: [SSubsetSps::default(); MAX_SPS_COUNT + 1],
            sPrefixNal: SNalUnit::default(),
            pActiveLayerSps: [None; MAX_LAYER_NUM],
            bAvcBasedFlag: true,
            bSpsExistAheadFlag: false,
            bSubspsExistAheadFlag: false,
            bPpsExistAheadFlag: false,
            iSpsErrorIgnored: 0,
            iSubSpsErrorIgnored: 0,
            iPpsErrorIgnored: 0,
            bSpsAvailFlags: [false; MAX_SPS_COUNT],
            bSubspsAvailFlags: [false; MAX_SPS_COUNT],
            bPpsAvailFlags: [false; MAX_PPS_COUNT],
            iPPSLastInvalidId: -1,
            iPPSInvalidNum: 0,
            iSPSLastInvalidId: -1,
            iSPSInvalidNum: 0,
            iSubSPSLastInvalidId: -1,
            iSubSPSInvalidNum: 0,
            iSeqId: -1,
            iOverwriteFlags: 0,
        }
    }
}

pub use crate::decoder::picture::{SPicture, SPicture as Picture};

pub use crate::decoder::pic_queue::{PicId, PicPool, PicRefs, SPicBuff};

/// The decoder picture buffer's three lists, as slot handles.
///
/// `None` is an empty slot. Every non-null entry is a pool picture, so
/// `Option<PicId>` equality is picture identity.
#[derive(Debug, Copy, Clone)]
pub struct SRefPic {
    pub pRefList: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
    pub pShortRefList: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
    pub pLongRefList: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
    pub uiRefCount: [u8; LIST_A],
    pub uiShortRefCount: [u8; LIST_A],
    pub uiLongRefCount: [u8; LIST_A],
    pub iMaxLongTermFrameIdx: i32,
}

impl SRefPic {
    /// The all-zero reference-list set, distinct from [`SRefPic::default`], whose
    /// `iMaxLongTermFrameIdx` is `-1`.
    pub fn memset_zero() -> Self {
        Self {
            pRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pShortRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pLongRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            uiRefCount: [0; LIST_A],
            uiShortRefCount: [0; LIST_A],
            uiLongRefCount: [0; LIST_A],
            iMaxLongTermFrameIdx: 0,
        }
    }
}

impl Default for SRefPic {
    fn default() -> Self {
        Self {
            pRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pShortRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pLongRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            uiRefCount: [0; LIST_A],
            uiShortRefCount: [0; LIST_A],
            uiLongRefCount: [0; LIST_A],
            iMaxLongTermFrameIdx: -1,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsLastDecPicInfo {
    pub sLastNalHdrExt: SNalUnitHeaderExt,
    pub sLastSliceHeader: SSliceHeader,
    pub iPrevPicOrderCntMsb: i32,
    pub iPrevPicOrderCntLsb: i32,
    /// The last picture handed to the DPB, as a slot handle — always `pDec` at the
    /// moment of the write.
    pub pPreviousDecodedPictureInDpb: Option<PicId>,
    pub iPrevFrameNum: i32,
    pub bLastHasMmco5: bool,
    pub uiDecodingTimeStamp: u32,
}

impl Default for SWelsLastDecPicInfo {
    fn default() -> Self {
        Self {
            sLastNalHdrExt: SNalUnitHeaderExt::default(),
            sLastSliceHeader: SSliceHeader::default(),
            iPrevPicOrderCntMsb: 0,
            iPrevPicOrderCntLsb: 0,
            pPreviousDecodedPictureInDpb: None,
            iPrevFrameNum: -1,
            bLastHasMmco5: false,
            uiDecodingTimeStamp: 0,
        }
    }
}

pub use crate::api::codec_api::{SBufferInfo, SSysMEMBuffer};

/// Pictures waiting for output. The bumping process of C.4.5.3 keeps at most
/// [`GetDpbSize`](crate::decoder::decoder_core::GetDpbSize) of them; twice the
/// largest DPB bounds how far the display layer can lag it.
pub const PICT_INFO_LIST_SIZE: usize = 2 * MAX_DPB_COUNT;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPictInfo {
    pub sBufferInfo: SBufferInfo,
    pub iPOC: i32,
    pub iPicBuffIdx: i32,
    pub uiDecodingTimeStamp: u32,
    pub iSeqNum: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPictReoderingStatus {
    pub iPictInfoIndex: i32,
    pub iMinSeqNum: i32,
    pub iMinPOC: i32,
    pub iNumOfPicts: i32,
    pub iLastWrittenSeqNum: i32,
    pub iLastWrittenPOC: i32,
    pub iLargestBufferedPicIndex: i32,
    /// Coded video sequences as the display layer counts them: one more at every
    /// IDR, SPS change and MMCO 5, which are the boundaries C.4.4 empties the DPB
    /// across. A buffered picture from an older sequence is always ready to go out.
    pub iOutputSeqNum: i32,
    /// `pCtx.iSeqNum` as it stood when the previous picture was buffered, so a change
    /// of the core's own sequence counter is visible here. `IMinInt32` until the
    /// first picture.
    pub iPrevCoreSeqNum: i32,
    /// Refreshed from the active SPS on every completed picture — see
    /// [`NeedsPictureReordering`](crate::decoder::decoder_core::NeedsPictureReordering)
    /// and [`GetDpbSize`](crate::decoder::decoder_core::GetDpbSize).
    pub bReorderPictures: bool,
    pub iDpbSize: i32,
    /// VUI `max_num_reorder_frames`, or -1 when the VUI carries no bitstream
    /// restriction.
    pub iMaxNumReorderFrames: i32,
}

/// The parse-only output buffers, owned.
#[repr(C)]
#[derive(Debug, Default, Clone)]
pub struct ParseOnlyBsBuffers {
    pub iNalNum: i32,
    /// Per-NAL lengths, `iNalNum` used of `len()` allocated. `ExpandBsLenBuffer`
    /// grows it.
    pub pNalLenInByte: Vec<i32>,
    /// The parse-only destination buffer, `MAX_ACCESS_UNIT_CAPACITY` zeroed bytes.
    /// Allocation only: nothing reads or writes it.
    pub pDstBuff: Vec<u8>,
    pub iSpsWidthInPixel: i32,
    pub iSpsHeightInPixel: i32,
    pub uiInBsTimeStamp: u64,
    pub uiOutBsTimeStamp: u64,
}

pub use crate::api::codec_api::SDecoderStatistics;

pub use crate::common::wels_trace::SLogContext;

pub use crate::api::codec_api::{SVideoProperty, VIDEO_BITSTREAM_TYPE};

pub use crate::api::codec_api::SDecodingParam;

pub use crate::decoder::decoder_core::{DqLayerState, SLayerInfo};

pub use crate::decoder::nalu::SAccessUnit;

/// The access unit under construction. `None` before `WelsInitStaticMemory` and
/// after `WelsFreeStaticMemory`.
#[inline]
pub fn cur_au(au: &mut Option<Box<SAccessUnit>>) -> Option<&mut SAccessUnit> {
    au.as_deref_mut()
}

/// The picture a slot handle names, or null when there is none.
#[inline]
pub fn pool_pic(pool: &Option<Box<SPicBuff>>, slot: Option<PicId>) -> Option<&SPicture> {
    pool.as_deref()?.slot(slot?)
}

/// [`pool_pic`]'s mutable form.
#[inline]
pub fn pool_pic_mut(
    pool: &mut Option<Box<SPicBuff>>,
    slot: Option<PicId>,
) -> Option<&mut SPicture> {
    pool.as_deref_mut()?.slot_mut(slot?)
}

/// The pool itself, for the operations that act on the container rather than on one
/// picture.
#[inline]
pub fn pic_pool_mut(pCtx: &mut SWelsDecoderContext) -> Option<&mut SPicBuff> {
    pCtx.pPicBuff.as_deref_mut()
}

/// Which SPS buffer an id names, and the id.
///
/// Two arrays hold them: `sSpsBuffer` for AVC and `sSubsetSpsBuffer` for SVC,
/// selected by the NAL's extension flag.
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SpsRef {
    pub id: i32,
    /// `true` when the entry is `sSubsetSpsBuffer[id].sSps`.
    pub subset: bool,
}

/// The SPS an [`SpsRef`] names, or `None` when there is none.
#[inline]
pub fn sps_of(ps: &SWelsDecoderSpsPpsCTX, r: Option<SpsRef>) -> Option<&SSps> {
    let r = r?;
    if r.id < 0 || r.id as usize >= MAX_SPS_COUNT + 1 {
        return None;
    }
    let i = r.id as usize;
    Some(if r.subset {
        &ps.sSubsetSpsBuffer[i].sSps
    } else {
        &ps.sSpsBuffer[i]
    })
}

/// The [`SpsRef`] an id resolves to, or `None` where [`sps_of`] answers `None`.
#[inline]
pub fn sps_ref_of(ps: &SWelsDecoderSpsPpsCTX, r: Option<SpsRef>) -> Option<SpsRef> {
    if sps_of(ps, r).is_some() { r } else { None }
}

/// [`sps_of`]'s mutable form.
#[inline]
pub fn sps_of_mut(ps: &mut SWelsDecoderSpsPpsCTX, r: Option<SpsRef>) -> Option<&mut SSps> {
    let r = r?;
    if r.id < 0 || r.id as usize >= MAX_SPS_COUNT + 1 {
        return None;
    }
    let i = r.id as usize;
    Some(if r.subset {
        &mut ps.sSubsetSpsBuffer[i].sSps
    } else {
        &mut ps.sSpsBuffer[i]
    })
}

/// The PPS an id names, or `None` when there is none. [`sps_of`]'s shape.
#[inline]
pub fn pps_of(ps: &SWelsDecoderSpsPpsCTX, id: Option<i32>) -> Option<&SPps> {
    let id = id?;
    if id < 0 || id as usize >= MAX_PPS_COUNT + 1 {
        return None;
    }
    Some(&ps.sPpsBuffer[id as usize])
}

/// The FMO entry a PPS id names, or `None` when there is none.
///
/// `sFmoList` is `MAX_PPS_COUNT` entries indexed by PPS id: one FMO state per
/// parameter set, persisting across access units.
#[inline]
pub fn fmo_of(list: &[SFmo; MAX_PPS_COUNT], id: Option<i32>) -> Option<&SFmo> {
    let id = id?;
    if id < 0 || id as usize >= MAX_PPS_COUNT {
        return None;
    }
    Some(&list[id as usize])
}

/// [`fmo_of`]'s mutable form.
#[inline]
pub fn fmo_of_mut(list: &mut [SFmo; MAX_PPS_COUNT], id: Option<i32>) -> Option<&mut SFmo> {
    let id = id?;
    if id < 0 || id as usize >= MAX_PPS_COUNT {
        return None;
    }
    Some(&mut list[id as usize])
}

/// The active FMO entry.
#[inline]
pub fn active_fmo(list: &[SFmo; MAX_PPS_COUNT], id: Option<i32>) -> Option<&SFmo> {
    fmo_of(list, id)
}

/// The subset SPS an id names, or `None` when there is none. [`sps_of`]'s shape.
#[inline]
pub fn subset_sps_of(ps: &SWelsDecoderSpsPpsCTX, id: Option<i32>) -> Option<&SSubsetSps> {
    let id = id?;
    if id < 0 || id as usize >= MAX_SPS_COUNT + 1 {
        return None;
    }
    Some(&ps.sSubsetSpsBuffer[id as usize])
}

/// The active SPS.
#[inline]
pub fn active_sps(ps: &SWelsDecoderSpsPpsCTX, active: Option<SpsRef>) -> Option<&SSps> {
    sps_of(ps, active)
}

/// The active PPS.
#[inline]
pub fn active_pps(ps: &SWelsDecoderSpsPpsCTX, active: Option<i32>) -> Option<&SPps> {
    pps_of(ps, active)
}

/// `pCtx->pParam->bParseOnly`.
#[inline]
pub fn parse_only(pParam: &SDecodingParam) -> bool {
    pParam.bParseOnly
}

/// `pCtx->pParam->eEcActiveIdc`.
#[inline]
pub fn ec_active_idc(pParam: &SDecodingParam) -> ERROR_CON_IDC {
    pParam.eEcActiveIdc
}

/// The layer the access unit is decoding, or null when there is none.
#[inline]
pub fn cur_dq_layer(list: &mut Option<Box<DqLayerState>>) -> Option<&mut DqLayerState> {
    list.as_deref_mut()
}

/// The parse-only descriptor, or null when the decoder is not in parse-only mode.
#[inline]
pub fn parser_bs(bs: &mut Option<Box<ParseOnlyBsBuffers>>) -> Option<&mut ParseOnlyBsBuffers> {
    bs.as_deref_mut()
}

/// The pool for the api layer's two release paths.
#[inline]
pub fn pic_pool_ptr(pool: &mut Option<Box<SPicBuff>>) -> Option<&mut SPicBuff> {
    pool.as_deref_mut()
}

/// The bracket top's pool borrow, handed down as a [`PicRefs`]. Below a bracket
/// top, `pRefs.get(id)` is the only route back to a picture.
#[inline]
pub fn pic_refs(pool: &Option<Box<SPicBuff>>) -> PicRefs<'_> {
    PicRefs::over(pool.as_deref())
}

/// The bracket top, both halves from one borrow. `None` arms: no pool, or no
/// current picture.
#[inline]
pub fn cur_and_refs(
    pool: &mut Option<Box<SPicBuff>>,
    cur: Option<PicId>,
) -> (Option<&mut SPicture>, PicRefs<'_>) {
    pic_and_refs_mut(pool, cur)
}

/// [`cur_and_refs`] for a bracket whose mutable half is not `pDec` — the
/// error-concealment prefetch, which writes a freshly taken slot while reading
/// another.
#[inline]
pub fn pic_and_refs(
    pool: &mut Option<Box<SPicBuff>>,
    slot: Option<PicId>,
) -> (Option<&mut SPicture>, PicRefs<'_>) {
    match (pool.as_deref_mut(), slot) {
        (Some(pool), Some(id)) => pool.cur_and_rest_mut(id),
        (Some(pool), None) => (None, pool.refs()),
        (None, _) => (None, PicRefs::over(None)),
    }
}

/// Alias for [`pic_and_refs`].
pub use self::pic_and_refs as pic_and_refs_mut;

/// The slice header the decode loop last started on, resolved from
/// [`SWelsDecoderContext::slice_hdr_nal`].
#[inline]
pub fn slice_header_of(pCtx: &SWelsDecoderContext) -> Option<&SSliceHeader> {
    let i = pCtx.slice_hdr_nal?;
    pCtx.access_unit
        .as_deref()
        .and_then(|au| au.node(i))
        .map(|nal| &nal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader)
}

/// Entry `i` of reference list `list` — the handle. The lists live in the context,
/// so reading one below a bracket top is not a pool access.
#[inline]
pub fn ref_id(refs: &SRefPic, list: usize, i: usize) -> Option<PicId> {
    refs.pRefList[list][i]
}

/// The picture being decoded into.
#[inline]
pub fn dec_pic(pool: &mut Option<Box<SPicBuff>>, cur: Option<PicId>) -> Option<&mut SPicture> {
    pool_pic_mut(pool, cur)
}

/// Entry `i` of reference list `list` — `sRefPic.pRefList[list][i]` resolved.
#[inline]
pub fn ref_pic<'a>(
    pool: &'a Option<Box<SPicBuff>>,
    refs: &SRefPic,
    list: usize,
    i: usize,
) -> Option<&'a SPicture> {
    pool_pic(pool, refs.pRefList[list][i])
}

/// Entry `i` of the short-term list — `sRefPic.pShortRefList[LIST_0][i]` resolved.
#[inline]
pub fn short_ref_pic<'a>(
    pool: &'a Option<Box<SPicBuff>>,
    refs: &SRefPic,
    i: usize,
) -> Option<&'a SPicture> {
    pool_pic(pool, refs.pShortRefList[LIST_0][i])
}

/// [`short_ref_pic`]'s mutable form.
#[inline]
pub fn short_ref_pic_mut<'a>(
    pool: &'a mut Option<Box<SPicBuff>>,
    refs: &SRefPic,
    i: usize,
) -> Option<&'a mut SPicture> {
    pool_pic_mut(pool, refs.pShortRefList[LIST_0][i])
}

/// Entry `i` of the long-term list — `sRefPic.pLongRefList[LIST_0][i]` resolved.
#[inline]
pub fn long_ref_pic<'a>(
    pool: &'a Option<Box<SPicBuff>>,
    refs: &SRefPic,
    i: usize,
) -> Option<&'a SPicture> {
    pool_pic(pool, refs.pLongRefList[LIST_0][i])
}

/// The previous decoded picture's handle, without touching the pool.
#[inline]
pub fn prev_dpb_id(pLastDecPicInfo: &SWelsLastDecPicInfo) -> Option<PicId> {
    pLastDecPicInfo.pPreviousDecodedPictureInDpb
}

/// [`prev_dpb_id`]'s handle resolved through the pool.
#[inline]
pub fn prev_dpb_pic_mut(
    pool: &mut Option<Box<SPicBuff>>,
    prev: Option<PicId>,
) -> Option<&mut SPicture> {
    pool_pic_mut(pool, prev)
}

/// Whether an access unit exists and has at least one NAL queued in it.
#[inline]
pub fn au_has_nals(pCtx: &mut SWelsDecoderContext) -> bool {
    matches!(cur_au(&mut pCtx.access_unit), Some(au) if au.uiAvailUnitsNum > 0)
}

/// Ends the access unit at the last NAL parsed and flags it ready for decode.
/// Returns whether there was an access unit to end.
#[inline]
pub fn mark_au_ready(pCtx: &mut SWelsDecoderContext) -> bool {
    match cur_au(&mut pCtx.access_unit) {
        Some(au) if au.uiAvailUnitsNum > 0 => {
            au.uiEndPos = au.uiAvailUnitsNum - 1;
            pCtx.bAuReadyFlag = true;
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// The slice's view of the context
// ---------------------------------------------------------------------------

/// Everything the per-macroblock tree reaches in [`SWelsDecoderContext`], as
/// field-precise borrows — not the picture pool, and not the NAL, whose bit cursor
/// travels as a separate argument so the two borrows stay disjoint.
///
/// A copied field is one nothing below the bracket writes.
pub struct SliceCtx<'a> {
    /// The buffer every window is derived from. Nothing below the bracket writes it.
    pub sRawData: &'a RawDataBuffer,
    /// The RBSP window the CABAC engine reads, derived once at the bracket top:
    /// `sRawData[reader.start..][..reader.cursor.len()]`. Both bounds are fixed when
    /// the NAL is parsed; the cursor's position moves inside the window, the window
    /// does not. Shared, so the engine can be borrowed mutably alongside it.
    pub rbsp: &'a [u8],
    pub sCabacDecEngine: &'a mut SWelsCabacDecEngine,
    pub pCabacCtx: &'a mut [SWelsCabacCtx; WELS_CONTEXT_COUNT],
    pub bMbRefConcealed: &'a mut bool,
    pub iErrorCode: &'a mut i32,
    pub iTotalNumMbRec: &'a mut i32,
    /// The B-slice temporal-direct scratch picture, lazily allocated at the first B
    /// macroblock.
    pub pTempDec: &'a mut Option<Box<Picture>>,
    pub sSpsPpsCtx: &'a SWelsDecoderSpsPpsCTX,
    pub sFmoList: &'a [SFmo; MAX_PPS_COUNT],
    pub sRefPic: &'a SRefPic,
    /// `pCtx->pVlcTable` resolved. The table is filled once by `InitVlcTable` and
    /// never written again.
    pub pVlcTable: &'a SVlcTable,
    pub pDequant_coeff_buffer4x4: &'a [[[u16; 16]; 52]; 6],
    pub pDequant_coeff_buffer8x8: &'a [[[u16; 64]; 52]; 6],
    pub pGetI16x16LumaPredFunc: &'a [PGetIntraPredFunc; 7],
    pub pGetI4x4LumaPredFunc: &'a [PGetIntraPredFunc; 14],
    pub pGetIChromaPredFunc: &'a [PGetIntraPredFunc; 7],
    pub pGetI8x8LumaPredFunc: &'a [PGetIntraPred8x8Func; 14],
    pub pIdctResAddPredFunc: PIdctResAddPredFunc,
    pub pIdctFourResAddPredFunc: PIdctFourResAddPredFunc,
    pub pIdctResAddPredFunc8x8: PIdctResAddPred8x8Func,

    // --- copied scalars ---
    /// Written by the bracket tops, above this construction, and by nothing below.
    pub eSliceType: EWelsSliceType,
    pub eIntraPredConstraint: IntraPredConstraint,
    /// Both written by `WelsCalcDeqCoeffScalingList`, before this construction.
    pub bUseScalingList: bool,
    pub bDequantCoeff4x4Init: bool,
    /// Written once per access unit by `DecodeCurrentAccessUnit`, and by
    /// `manage_dec_ref`'s reference-list construction; both above the bracket.
    pub bRPLRError: bool,
    /// `pParam`'s two questions; neither answer changes after `Initialize`.
    /// `bEcActive` is `eEcActiveIdc != ERROR_CON_DISABLE`.
    pub bParseOnly: bool,
    pub bEcActive: bool,
    pub iCurSeqIntervalMaxPicWidth: i32,
    /// `GetThreadCount`, which is 0 and cannot change mid-slice.
    pub iThreadCount: i32,
    /// The active parameter sets, as the ids the context stores.
    pub active_sps: Option<SpsRef>,
    pub active_pps: Option<i32>,
    pub fmo_id: Option<i32>,
}

impl<'a> SliceCtx<'a> {
    /// [`sps_of`] against the view.
    #[inline]
    pub fn sps_of(&self, r: Option<SpsRef>) -> Option<&SSps> {
        let r = r?;
        if r.id < 0 || r.id as usize >= MAX_SPS_COUNT + 1 {
            return None;
        }
        let i = r.id as usize;
        Some(if r.subset {
            &self.sSpsPpsCtx.sSubsetSpsBuffer[i].sSps
        } else {
            &self.sSpsPpsCtx.sSpsBuffer[i]
        })
    }

    /// [`pps_of`] against the view.
    #[inline]
    pub fn pps_of(&self, id: Option<i32>) -> Option<&SPps> {
        let id = id?;
        if id < 0 || id as usize >= MAX_PPS_COUNT + 1 {
            return None;
        }
        Some(&self.sSpsPpsCtx.sPpsBuffer[id as usize])
    }

    /// [`active_sps`] against the view.
    #[inline]
    pub fn active_sps(&self) -> Option<&SSps> {
        self.sps_of(self.active_sps)
    }

    /// [`active_pps`] against the view.
    #[inline]
    pub fn active_pps(&self) -> Option<&SPps> {
        self.pps_of(self.active_pps)
    }

    /// [`active_fmo`] against the view.
    #[inline]
    pub fn active_fmo(&self) -> Option<&SFmo> {
        let id = self.fmo_id?;
        if id < 0 || id as usize >= MAX_PPS_COUNT {
            return None;
        }
        Some(&self.sFmoList[id as usize])
    }

    /// [`ref_id`] against the view.
    #[inline]
    pub fn ref_id(&self, list: usize, i: usize) -> Option<PicId> {
        self.sRefPic.pRefList[list][i]
    }

    /// The active SPS's `uiChromaFormatIdc`. 0 (monochrome) with no active SPS, a
    /// state the slice tree cannot reach: a slice header that activates no SPS never
    /// gets here.
    #[inline]
    pub fn uiChromaFormatIdc(&self) -> u8 {
        self.active_sps().map_or(0, |sps| sps.uiChromaFormatIdc)
    }

    /// The active PPS's `bTransform8x8ModeFlag`. `false` with no active PPS.
    #[inline]
    pub fn bTransform8x8ModeFlag(&self) -> bool {
        self.active_pps()
            .is_some_and(|pps| pps.bTransform8x8ModeFlag)
    }
}

/// The one construction of [`SliceCtx`].
///
/// `None` is a bracket with no NAL in flight; the view then carries an empty window
/// and a read through it fails with `ERR_INFO_READ_OVERFLOW`.
macro_rules! slice_view {
    ($ctx:expr, $reader:expr, $threads:expr) => {{
        let raw: &RawDataBuffer = &$ctx.sRawData;
        SliceCtx {
            sRawData: raw,
            rbsp: match $reader {
                Some(reader) => raw.rbsp_window(reader),
                None => &[],
            },
            sCabacDecEngine: &mut $ctx.sCabacDecEngine,
            pCabacCtx: &mut $ctx.pCabacCtx,
            bMbRefConcealed: &mut $ctx.bMbRefConcealed,
            iErrorCode: &mut $ctx.iErrorCode,
            iTotalNumMbRec: &mut $ctx.iTotalNumMbRec,
            pTempDec: &mut $ctx.pTempDec,
            sSpsPpsCtx: &$ctx.sSpsPpsCtx,
            sFmoList: &$ctx.sFmoList,
            sRefPic: &$ctx.sRefPic,
            pVlcTable: &$ctx.pVlcTable,
            pDequant_coeff_buffer4x4: &$ctx.pDequant_coeff_buffer4x4,
            pDequant_coeff_buffer8x8: &$ctx.pDequant_coeff_buffer8x8,
            pGetI16x16LumaPredFunc: &$ctx.pGetI16x16LumaPredFunc,
            pGetI4x4LumaPredFunc: &$ctx.pGetI4x4LumaPredFunc,
            pGetIChromaPredFunc: &$ctx.pGetIChromaPredFunc,
            pGetI8x8LumaPredFunc: &$ctx.pGetI8x8LumaPredFunc,
            pIdctResAddPredFunc: $ctx.pIdctResAddPredFunc,
            pIdctFourResAddPredFunc: $ctx.pIdctFourResAddPredFunc,
            pIdctResAddPredFunc8x8: $ctx.pIdctResAddPredFunc8x8,
            eSliceType: $ctx.eSliceType,
            eIntraPredConstraint: $ctx.eIntraPredConstraint,
            bUseScalingList: $ctx.bUseScalingList,
            bDequantCoeff4x4Init: $ctx.bDequantCoeff4x4Init,
            bRPLRError: $ctx.bRPLRError,
            bParseOnly: parse_only(&$ctx.pParam),
            bEcActive: ec_active_idc(&$ctx.pParam)
                != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE,
            iCurSeqIntervalMaxPicWidth: $ctx.iCurSeqIntervalMaxPicWidth,
            iThreadCount: $threads,
            active_sps: $ctx.active_sps,
            active_pps: $ctx.active_pps,
            fmo_id: $ctx.fmo_id,
        }
    }};
}

/// The view alone, for the brackets that hold no picture.
#[inline]
pub fn slice_ctx<'a>(pCtx: &'a mut SWelsDecoderContext, reader: Option<&BsReader>) -> SliceCtx<'a> {
    // Whole-context reads happen before the first field borrow.
    let iThreadCount = crate::decoder::decoder_core::GetThreadCount(pCtx);
    slice_view!(pCtx, reader, iThreadCount)
}

/// The bracket top's split — the pool's two halves and the view, out of one borrow
/// of the context.
#[inline]
pub fn slice_split<'a>(
    pCtx: &'a mut SWelsDecoderContext,
    nal: Option<usize>,
) -> (
    Option<&'a mut SPicture>,
    PicRefs<'a>,
    SliceCtx<'a>,
    Option<&'a mut SNalUnit>,
) {
    // Whole-context reads happen before the first field borrow.
    let iThreadCount = crate::decoder::decoder_core::GetThreadCount(pCtx);
    let cur = pCtx.pDec;
    let (pDec, pRefs) = pic_and_refs(&mut pCtx.pPicBuff, cur);
    let node = nal.and_then(|i| {
        pCtx.access_unit
            .as_deref_mut()
            .and_then(|au| au.node_mut(i))
    });
    // The reader travels by value, so the node's borrow leaves with it.
    let reader = node.as_deref().map(|n| n.sNalData.sVclNal.sSliceBitsRead);
    let view = slice_view!(pCtx, reader.as_ref(), iThreadCount);
    (pDec, pRefs, view, node)
}

/// The bracket top's split for a scope that writes the picture and reads no
/// references. `None` is no pool, or no current picture.
#[inline]
pub fn pic_split<'a>(
    pCtx: &'a mut SWelsDecoderContext,
) -> (Option<&'a mut SPicture>, SliceCtx<'a>) {
    let (pDec, _refs, view, _nal) = slice_split(pCtx, None);
    (pDec, view)
}

/// The reference set a DPB operation acts on: `sTmpRefPic` is the threading arm's,
/// `sRefPic` every other caller's.
#[inline]
pub fn ref_set(pCtx: &mut SWelsDecoderContext, tmp: bool) -> &mut SRefPic {
    if tmp {
        &mut pCtx.sTmpRefPic
    } else {
        &mut pCtx.sRefPic
    }
}

/// A view over a test context, with the caller's VLC table installed — the one piece
/// of wiring a fresh context lacks.
#[cfg(test)]
pub(crate) fn test_slice_ctx<'a>(
    ctx: &'a mut SWelsDecoderContext,
    vlc: &SVlcTable,
) -> SliceCtx<'a> {
    ctx.pVlcTable = *vlc;
    slice_ctx(ctx, None)
}

// ---------------------------------------------------------------------------
// Master Decoder Context
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct SWelsDecoderContext {
    pub sLogCtx: SLogContext,
    pub pArgDec: *mut c_void,
    pub sRawData: RawDataBuffer,
    pub sSavedData: RawDataBuffer,
    /// The decoding parameters, owned.
    ///
    /// Written by [`DecoderConfigParam`](crate::decoder::decoder_core::DecoderConfigParam)
    /// from `Initialize`; `SetOption(DECODER_OPTION_ERROR_CON_IDC)` writes
    /// `eEcActiveIdc` alone.
    pub pParam: SDecodingParam,
    pub uiCpuFlag: u32,
    pub eVideoType: VIDEO_BITSTREAM_TYPE,
    pub bHaveGotMemory: bool,
    pub iImgWidthInPixel: i32,
    pub iImgHeightInPixel: i32,
    pub iLastImgWidthInPixel: i32,
    pub iLastImgHeightInPixel: i32,
    pub bFreezeOutput: bool,
    pub sCurNalHead: SNalUnitHeader,
    pub eSliceType: EWelsSliceType,

    pub bUsedAsRef: bool,
    pub iFrameNum: i32,
    pub iErrorCode: i32,
    pub sFmoList: [SFmo; MAX_PPS_COUNT],
    /// The active FMO entry, as the PPS id that selects it. `sFmoList` is indexed by
    /// PPS id and nothing else.
    pub fmo_id: Option<i32>,
    pub iActiveFmoNum: i32,
    /// The pool slot being decoded into, reached with [`dec_pic`]. `None` is no
    /// picture in flight.
    pub pDec: Option<PicId>,
    /// The B-slice temporal-direct scratch picture, owned.
    ///
    /// Not a pool slot: allocated on its own at the first B macroblock by
    /// [`pic_queue::alloc_picture`](crate::decoder::pic_queue::alloc_picture), so it
    /// has no `PicId`. `None` means not allocated yet — the state the lazy
    /// allocation tests for.
    pub pTempDec: Option<Box<Picture>>,
    pub sRefPic: SRefPic,
    pub sTmpRefPic: SRefPic,
    /// `CWelsDecoderImpl::sVlcTable`, owned. Filled by `WelsOpenDecoder`.
    pub pVlcTable: SVlcTable,
    pub sBs: BsReader,
    pub sSpsPpsCtx: SWelsDecoderSpsPpsCTX,
    pub bHasNewSps: bool,
    pub sFrameCrop: SPosOffset,
    /// The decoded-picture pool, owned. Dropped by the context's drop glue;
    /// `DestroyPicBuff` also runs the reordering reset.
    pub pPicBuff: Option<Box<SPicBuff>>,
    pub iPicQueueNumber: i32,
    /// The access unit under construction, owned. Reach it with [`cur_au`].
    pub access_unit: Option<Box<SAccessUnit>>,
    /// The active parameter sets, as ids into the two buffers. `None` before the
    /// first slice header; [`active_sps`] and [`active_pps`] resolve them.
    pub active_sps: Option<SpsRef>,
    pub active_pps: Option<i32>,
    /// The one layer, owned, reached only through [`cur_dq_layer`]. `None` before
    /// `InitialDqLayersContext` and after `UninitialDqLayersContext`.
    pub pDqLayersList: Option<Box<DqLayerState>>,
    /// The NAL under decode, as its index in the access unit.
    pub nal_cur: Option<usize>,
    /// The NAL whose slice header the decode loop last started on.
    ///
    /// Distinct from [`nal_cur`](Self::nal_cur), which is stamped when the
    /// access-unit loop picks a NAL up; this one when `WelsDqLayerDecodeStart` starts
    /// decoding it. `None` before the first slice decode.
    pub slice_hdr_nal: Option<usize>,
    pub uiNalRefIdc: u8,
    pub iPicWidthReq: i32,
    pub iPicHeightReq: i32,
    pub uiTargetDqId: u8,
    pub bEndOfStreamFlag: bool,
    pub bInstantDecFlag: bool,
    pub bInitialDqLayersMem: bool,
    pub bOnlyOneLayerInCurAuFlag: bool,
    pub bReferenceLostAtT0Flag: bool,
    pub iTotalNumMbRec: i32,
    pub bParamSetsLostFlag: bool,
    pub bCurAuContainLtrMarkSeFlag: bool,
    pub iFrameNumOfAuMarkedLtr: i32,
    pub uiCurIdrPicId: u16,
    pub bNewSeqBegin: bool,
    pub bNextNewSeqBegin: bool,
    /// `CWelsDecoderImpl::iStreamSeqNum`, owned.
    pub pStreamSeqNum: i32,
    pub iSeqNum: i32,
    pub bFramePending: bool,
    pub bFrameFinish: bool,
    pub iNalNum: i32,
    pub sSpsBsInfo: [SSpsBsInfo; MAX_SPS_COUNT],
    pub sSubsetSpsBsInfo: [SSpsBsInfo; MAX_PPS_COUNT],
    pub sPpsBsInfo: [SPpsBsInfo; MAX_PPS_COUNT],
    /// The parse-only descriptor, **owned** and reached through [`parser_bs`].
    /// `None` outside parse-only mode.
    pub pParserBsInfo: Option<Box<ParseOnlyBsBuffers>>,
    pub pGetI16x16LumaPredFunc: [PGetIntraPredFunc; 7],
    pub pGetI4x4LumaPredFunc: [PGetIntraPredFunc; 14],
    pub pGetIChromaPredFunc: [PGetIntraPredFunc; 7],
    pub pIdctResAddPredFunc: PIdctResAddPredFunc,
    pub pIdctFourResAddPredFunc: PIdctFourResAddPredFunc,
    pub sMcFunc: SMcFunc,
    pub pGetI8x8LumaPredFunc: [PGetIntraPred8x8Func; 14],
    pub pIdctResAddPredFunc8x8: PIdctResAddPred8x8Func,
    pub sCopyFunc: SCopyFunc,
    pub iCurSeqIntervalTargetDependId: i32,
    pub iCurSeqIntervalMaxPicWidth: i32,
    pub iCurSeqIntervalMaxPicHeight: i32,
    /// See [`IntraPredConstraint`](crate::decoder::decode_slice::IntraPredConstraint).
    pub eIntraPredConstraint: IntraPredConstraint,
    pub iFeedbackVclNalInAu: i32,
    pub iFeedbackTidInAu: i32,
    pub iFeedbackNalRefIdc: i32,
    pub bAuReadyFlag: bool,
    pub bPrintFrameErrorTraceFlag: bool,
    pub iIgnoredErrorInfoPacketCount: i32,
    pub pTraceHandle: *mut c_void,
    /// `CWelsDecoderImpl::sLastDecPicInfo`, owned. Initialised at context
    /// construction to `WelsDecoderLastDecPicInfoDefaults`, which are not zeros.
    pub pLastDecPicInfo: SWelsLastDecPicInfo,
    pub sWelsCabacContexts: [[[SWelsCabacCtx; WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
    pub bCabacInited: bool,
    pub pCabacCtx: [SWelsCabacCtx; WELS_CONTEXT_COUNT],
    /// The arithmetic decoding engine, by value.
    pub sCabacDecEngine: SWelsCabacDecEngine,
    pub dDecTime: f64,
    /// `CWelsDecoderImpl::sDecoderStatistics`, owned. Handed out whole by
    /// `DECODER_OPTION_GET_STATISTICS`.
    pub pDecoderStatistics: SDecoderStatistics,
    pub iMbEcedNum: i32,
    pub iMbEcedPropNum: i32,
    pub iMbNum: i32,
    pub bMbRefConcealed: bool,
    pub bRPLRError: bool,
    pub iECMVs: [[i32; 2]; 16],
    /// The concealment reference list, as [`PicId`]s: every entry is a picture from
    /// `sRefPic.pRefList`.
    pub pECRefPic: [Option<PicId>; 16],
    pub uiTimeStamp: u64,
    pub uiDecodingTimeStamp: u32,
    pub pDequant_coeff_buffer4x4: [[[u16; 16]; 52]; 6],
    pub pDequant_coeff_buffer8x8: [[[u16; 64]; 52]; 6],
    pub iDequantCoeffPpsid: i32,
    pub bDequantCoeff4x4Init: bool,
    pub bUseScalingList: bool,
    pub lastReadyHeightOffset: [[i16; MAX_REF_PIC_COUNT]; LIST_A],
    /// `CWelsDecoderImpl::sPictInfoList`, owned.
    pub pPictInfoList: [SPictInfo; PICT_INFO_LIST_SIZE],
    /// `CWelsDecoderImpl::sReoderingStatus`, owned.
    pub pPictReoderingStatus: SPictReoderingStatus,
    /// `CWelsDecoder::m_bIsBaseline` — reordering is bypassed entirely for baseline
    /// profiles.
    pub bIsBaseline: bool,
    /// `CWelsDecoder::m_uiDecodeTimeStamp` — the monotonic counter stamped onto each
    /// decoded picture as [`uiDecodingTimeStamp`](Self::uiDecodingTimeStamp); the
    /// no-reorder release path orders buffered pictures by it.
    pub uiDecodeTimeStamp: u32,
}

impl Default for SWelsDecoderContext {
    fn default() -> Self {
        Self {
            sLogCtx: SLogContext::default(),
            pArgDec: std::ptr::null_mut(),
            sRawData: RawDataBuffer::default(),
            sSavedData: RawDataBuffer::default(),
            pParam: SDecodingParam::default(),
            uiCpuFlag: 0,
            eVideoType: VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC,
            bHaveGotMemory: false,
            iImgWidthInPixel: 0,
            iImgHeightInPixel: 0,
            iLastImgWidthInPixel: 0,
            iLastImgHeightInPixel: 0,
            bFreezeOutput: false,
            sCurNalHead: SNalUnitHeader::default(),
            eSliceType: EWelsSliceType::P_SLICE,
            bUsedAsRef: false,
            iFrameNum: 0,
            iErrorCode: 0,
            // `SFmo` is not `Copy`, so this is 256 calls rather than a repeat
            // expression.
            sFmoList: std::array::from_fn(|_| SFmo::default()),
            fmo_id: None,
            iActiveFmoNum: 0,
            pDec: None,
            pTempDec: None,
            sRefPic: SRefPic::memset_zero(),
            sTmpRefPic: SRefPic::memset_zero(),
            // Empty slices until `WelsOpenDecoder` fills the table.
            pVlcTable: SVlcTable {
                kpCoeffTokenVlcTable: [[&[]; 8]; 4],
                kpChromaCoeffTokenVlcTable: &[],
                kpZeroTable: [&[]; 7],
                kpTotalZerosTable: [[&[]; 15]; 2],
            },
            sBs: BsReader::default(),
            sSpsPpsCtx: SWelsDecoderSpsPpsCTX::memset_zero(),
            bHasNewSps: false,
            sFrameCrop: SPosOffset::default(),
            pPicBuff: None,
            iPicQueueNumber: 0,
            access_unit: None,
            active_sps: None,
            active_pps: None,
            pDqLayersList: None,
            nal_cur: None,
            slice_hdr_nal: None,
            uiNalRefIdc: 0,
            iPicWidthReq: 0,
            iPicHeightReq: 0,
            uiTargetDqId: 0,
            bEndOfStreamFlag: false,
            bInstantDecFlag: false,
            bInitialDqLayersMem: false,
            bOnlyOneLayerInCurAuFlag: false,
            bReferenceLostAtT0Flag: false,
            iTotalNumMbRec: 0,
            bParamSetsLostFlag: false,
            bCurAuContainLtrMarkSeFlag: false,
            iFrameNumOfAuMarkedLtr: 0,
            uiCurIdrPicId: 0,
            bNewSeqBegin: false,
            bNextNewSeqBegin: false,
            pStreamSeqNum: 0,
            iSeqNum: 0,
            bFramePending: false,
            bFrameFinish: false,
            iNalNum: 0,
            sSpsBsInfo: [SSpsBsInfo::default(); MAX_SPS_COUNT],
            sSubsetSpsBsInfo: [SSpsBsInfo::default(); MAX_PPS_COUNT],
            sPpsBsInfo: [SPpsBsInfo::default(); MAX_PPS_COUNT],
            pParserBsInfo: None,
            // Uninstalled until `WelsInitDecoderFuncs`.
            pGetI16x16LumaPredFunc: [None; 7],
            pGetI4x4LumaPredFunc: [None; 14],
            pGetIChromaPredFunc: [None; 7],
            pIdctResAddPredFunc: None,
            pIdctFourResAddPredFunc: None,
            sMcFunc: SMcFunc::default(),
            pGetI8x8LumaPredFunc: [None; 14],
            pIdctResAddPredFunc8x8: None,
            sCopyFunc: SCopyFunc::memset_zero(),
            iCurSeqIntervalTargetDependId: 0,
            iCurSeqIntervalMaxPicWidth: 0,
            iCurSeqIntervalMaxPicHeight: 0,
            eIntraPredConstraint: IntraPredConstraint::Constrain0,
            iFeedbackVclNalInAu: 0,
            iFeedbackTidInAu: 0,
            iFeedbackNalRefIdc: 0,
            bAuReadyFlag: false,
            bPrintFrameErrorTraceFlag: false,
            iIgnoredErrorInfoPacketCount: 0,
            pTraceHandle: std::ptr::null_mut(),
            pLastDecPicInfo: SWelsLastDecPicInfo::default(),
            // `WelsCabacGlobalInit` overwrites every entry on the first CABAC access
            // unit; `bCabacInited` is the guard.
            sWelsCabacContexts: [[[SWelsCabacCtx::default(); WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1];
                4],
            bCabacInited: false,
            pCabacCtx: [SWelsCabacCtx::default(); WELS_CONTEXT_COUNT],
            // A zeroed engine is inert: `pos = 0` against an empty window takes the
            // error arm.
            sCabacDecEngine: SWelsCabacDecEngine::default(),
            dDecTime: 0.0,
            pDecoderStatistics: SDecoderStatistics::default(),
            iMbEcedNum: 0,
            iMbEcedPropNum: 0,
            iMbNum: 0,
            bMbRefConcealed: false,
            bRPLRError: false,
            iECMVs: [[0; 2]; 16],
            pECRefPic: [None; 16],
            uiTimeStamp: 0,
            uiDecodingTimeStamp: 0,
            pDequant_coeff_buffer4x4: [[[0; 16]; 52]; 6],
            pDequant_coeff_buffer8x8: [[[0; 64]; 52]; 6],
            iDequantCoeffPpsid: 0,
            // `WelsCalcDeqCoeffScalingList` writes both buffers and this flag
            // together.
            bDequantCoeff4x4Init: false,
            bUseScalingList: false,
            lastReadyHeightOffset: [[0; MAX_REF_PIC_COUNT]; LIST_A],
            pPictInfoList: [SPictInfo::default(); PICT_INFO_LIST_SIZE],
            pPictReoderingStatus: SPictReoderingStatus::default(),
            bIsBaseline: false,
            uiDecodeTimeStamp: 0,
        }
    }
}

impl SWelsDecoderContext {
    /// The context on the heap — it is far too large to want on the stack.
    pub fn new_boxed() -> Box<Self> {
        Box::new(Self::default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::decode_mb_aux::idct_res_add_pred;
    use crate::decoder::pic_queue::{CreatePicBuff, DestroyPicBuff};

    #[test]
    fn test_decoder_constants() {
        assert_eq!(MAX_PRED_MODE_ID_I16x16, 3);
        assert_eq!(MAX_PRED_MODE_ID_CHROMA, 3);
        assert_eq!(MAX_PRED_MODE_ID_I4x4, 8);
        assert_eq!(WELS_QP_MAX, 51);
        assert_eq!(IMinInt32, -0x7FFFFFFF);
        assert_eq!(MAX_DPB_COUNT, 17);
    }

    #[test]
    fn test_idct_res_add_pred_c() {
        let mut pred = [128u8; 16];
        let rs = [0i16; 16];
        idct_res_add_pred(&mut PlaneCursorMut::new(&mut pred, 0, 4), &rs);
        for &val in &pred {
            assert_eq!(val, 128);
        }
    }

    /// The context is born with no active SPS.
    #[test]
    fn the_context_is_born_with_no_active_sps() {
        let ctx = SWelsDecoderContext::new_boxed();
        assert!(
            ctx.active_sps.is_none(),
            "active_sps was {:?}; the C's memset leaves a null SSps*",
            ctx.active_sps
        );
        // The prefix NAL's VCL arm, which nothing writes and nothing reads.
        assert!(
            ctx.sSpsPpsCtx
                .sPrefixNal
                .sNalData
                .sVclNal
                .sSliceHeaderExt
                .sSliceHeader
                .sps_ref
                .is_none()
        );
    }

    #[test]
    fn test_sps_pps_defaults() {
        let mut sps_pps_ctx = SWelsDecoderSpsPpsCTX::default();
        sps_pps_ctx.bAvcBasedFlag = true;
        sps_pps_ctx.iPPSLastInvalidId = -1;
        sps_pps_ctx.iSPSLastInvalidId = -1;
        assert!(sps_pps_ctx.bAvcBasedFlag);
        assert_eq!(sps_pps_ctx.iPPSLastInvalidId, -1);
        assert_eq!(sps_pps_ctx.iSPSLastInvalidId, -1);
    }

    #[test]
    fn test_pic_buff_creation_and_destruction() {
        let mut ctx = SWelsDecoderContext::new_boxed();

        {
            let pCtx = &mut *ctx;
            pCtx.pPicBuff = CreatePicBuff(false, 4, 64, 64);
            assert!(pCtx.pPicBuff.is_some());
            assert_eq!(pic_pool_mut(pCtx).map(|pool| pool.capacity()), Some(4));

            // `take()` reads the pool and leaves the context naming nothing.
            let pool = pCtx.pPicBuff.take();
            DestroyPicBuff(pCtx, pool);
            assert!(pCtx.pPicBuff.is_none());
        }
    }
}
