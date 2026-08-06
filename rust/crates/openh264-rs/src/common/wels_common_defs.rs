#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code
)]

//! Types shared by the encoder and the decoder.
//!
//! Translated from `codec/common/inc/wels_common_defs.h`. Both codecs include that
//! header in C++, so these types have exactly one definition here rather than one
//! copy per module.

/// Bit-stream auxiliary reading / writing state.
///
/// Matches `TagBitStringAux` in `codec/common/inc/wels_common_defs.h:232`. The field
/// order is load-bearing — this is `#[repr(C)]` and the layout must match the C++
/// struct byte for byte:
///
/// ```text
/// pStartBuf, pEndBuf, iBits, iIndex, pCurBuf, uiCurBits, iLeftBits
/// ```
///
/// `iIndex` is `intX_t` in C++ (`codec/common/inc/typedefs.h:52`), i.e. `int64_t` on
/// LP64 / Win64 and `int32_t` otherwise — pointer-width, hence `isize`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagBitStringAux {
    /// Buffer start position.
    pub pStartBuf: *mut u8,
    /// Buffer end boundary (`pStartBuf + length`).
    pub pEndBuf: *mut u8,
    /// Count of bits of overall bitstream input.
    pub iBits: i32,

    /// Only for CAVLC usage.
    pub iIndex: isize,
    /// Current reading/writing position.
    pub pCurBuf: *mut u8,
    /// 32-bit accumulator of unwritten/unconsumed bits.
    pub uiCurBits: u32,
    /// Number of available bits left in the accumulator.
    pub iLeftBits: i32,
}

pub type SBitStringAux = TagBitStringAux;
pub type PBitStringAux = *mut SBitStringAux;

impl TagBitStringAux {
    /// Zero-initialized, matching C's `memset`/aggregate zero-initialization.
    ///
    /// Note `iLeftBits` is **0**, not 32: C++ never zero-inits this struct into a
    /// usable state, it calls `InitBits` (`codec/common/inc/golomb_common.h:67`) for
    /// writing or `DecInitBits` for reading, and those set `iLeftBits` themselves.
    pub const fn new() -> Self {
        Self {
            pStartBuf: std::ptr::null_mut(),
            pEndBuf: std::ptr::null_mut(),
            iBits: 0,
            iIndex: 0,
            pCurBuf: std::ptr::null_mut(),
            uiCurBits: 0,
            iLeftBits: 0,
        }
    }
}

impl Default for TagBitStringAux {
    fn default() -> Self {
        Self::new()
    }
}

/// NAL Unit Type (5 bits) per ITU-T H.264 / AVC and Annex G (SVC).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EWelsNalUnitType {
    NAL_UNIT_UNSPEC_0 = 0,
    NAL_UNIT_CODED_SLICE = 1,
    NAL_UNIT_CODED_SLICE_DPA = 2,
    NAL_UNIT_CODED_SLICE_DPB = 3,
    NAL_UNIT_CODED_SLICE_DPC = 4,
    NAL_UNIT_CODED_SLICE_IDR = 5,
    NAL_UNIT_SEI = 6,
    NAL_UNIT_SPS = 7,
    NAL_UNIT_PPS = 8,
    NAL_UNIT_AU_DELIMITER = 9,
    NAL_UNIT_END_OF_SEQ = 10,
    NAL_UNIT_END_OF_STR = 11,
    NAL_UNIT_FILLER_DATA = 12,
    NAL_UNIT_SPS_EXT = 13,
    NAL_UNIT_PREFIX = 14,
    NAL_UNIT_SUBSET_SPS = 15,
    NAL_UNIT_DEPTH_PARAM = 16,
    NAL_UNIT_RESV_17 = 17,
    NAL_UNIT_RESV_18 = 18,
    NAL_UNIT_AUX_CODED_SLICE = 19,
    NAL_UNIT_CODED_SLICE_EXT = 20,
    NAL_UNIT_MVC_SLICE_EXT = 21,
    NAL_UNIT_RESV_22 = 22,
    NAL_UNIT_RESV_23 = 23,
    NAL_UNIT_UNSPEC_24 = 24,
    NAL_UNIT_UNSPEC_25 = 25,
    NAL_UNIT_UNSPEC_26 = 26,
    NAL_UNIT_UNSPEC_27 = 27,
    NAL_UNIT_UNSPEC_28 = 28,
    NAL_UNIT_UNSPEC_29 = 29,
    NAL_UNIT_UNSPEC_30 = 30,
    NAL_UNIT_UNSPEC_31 = 31,
}

impl Default for EWelsNalUnitType {
    fn default() -> Self {
        EWelsNalUnitType::NAL_UNIT_UNSPEC_0
    }
}

impl From<i32> for EWelsNalUnitType {
    fn from(val: i32) -> Self {
        match val {
            0 => EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
            1 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE,
            2 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPA,
            3 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPB,
            4 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPC,
            5 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR,
            6 => EWelsNalUnitType::NAL_UNIT_SEI,
            7 => EWelsNalUnitType::NAL_UNIT_SPS,
            8 => EWelsNalUnitType::NAL_UNIT_PPS,
            9 => EWelsNalUnitType::NAL_UNIT_AU_DELIMITER,
            10 => EWelsNalUnitType::NAL_UNIT_END_OF_SEQ,
            11 => EWelsNalUnitType::NAL_UNIT_END_OF_STR,
            12 => EWelsNalUnitType::NAL_UNIT_FILLER_DATA,
            13 => EWelsNalUnitType::NAL_UNIT_SPS_EXT,
            14 => EWelsNalUnitType::NAL_UNIT_PREFIX,
            15 => EWelsNalUnitType::NAL_UNIT_SUBSET_SPS,
            16 => EWelsNalUnitType::NAL_UNIT_DEPTH_PARAM,
            17 => EWelsNalUnitType::NAL_UNIT_RESV_17,
            18 => EWelsNalUnitType::NAL_UNIT_RESV_18,
            19 => EWelsNalUnitType::NAL_UNIT_AUX_CODED_SLICE,
            20 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT,
            21 => EWelsNalUnitType::NAL_UNIT_MVC_SLICE_EXT,
            22 => EWelsNalUnitType::NAL_UNIT_RESV_22,
            23 => EWelsNalUnitType::NAL_UNIT_RESV_23,
            24 => EWelsNalUnitType::NAL_UNIT_UNSPEC_24,
            25 => EWelsNalUnitType::NAL_UNIT_UNSPEC_25,
            26 => EWelsNalUnitType::NAL_UNIT_UNSPEC_26,
            27 => EWelsNalUnitType::NAL_UNIT_UNSPEC_27,
            28 => EWelsNalUnitType::NAL_UNIT_UNSPEC_28,
            29 => EWelsNalUnitType::NAL_UNIT_UNSPEC_29,
            30 => EWelsNalUnitType::NAL_UNIT_UNSPEC_30,
            31 => EWelsNalUnitType::NAL_UNIT_UNSPEC_31,
            _ => EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
        }
    }
}

/// NAL Reference IDC (2 bits) indicating reference priority.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EWelsNalRefIdc {
    NRI_PRI_LOWEST = 0,
    NRI_PRI_LOW = 1,
    NRI_PRI_HIGH = 2,
    NRI_PRI_HIGHEST = 3,
}

impl Default for EWelsNalRefIdc {
    fn default() -> Self {
        EWelsNalRefIdc::NRI_PRI_LOWEST
    }
}

/// 1-Byte Base AVC NAL Unit Header.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SNalUnitHeader {
    pub uiForbiddenZeroBit: u8,
    pub uiNalRefIdc: u8,
    pub eNalUnitType: EWelsNalUnitType,
    pub uiReservedOneByte: u8,
}

impl Default for SNalUnitHeader {
    fn default() -> Self {
        Self {
            uiForbiddenZeroBit: 0,
            uiNalRefIdc: 0,
            eNalUnitType: EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
            uiReservedOneByte: 0,
        }
    }
}

/// Extended SVC NAL Unit Header.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SNalUnitHeaderExt {
    pub sNalUnitHeader: SNalUnitHeader,
    pub bIdrFlag: bool,
    pub uiPriorityId: u8,
    pub iNoInterLayerPredFlag: i8,
    pub uiDependencyId: u8,
    pub uiQualityId: u8,
    pub uiTemporalId: u8,
    pub bUseRefBasePicFlag: bool,
    pub bDiscardableFlag: bool,
    pub bOutputFlag: bool,
    pub uiReservedThree2Bits: u8,
    pub uiLayerDqId: u8,
    pub bNalExtFlag: bool,
}

impl Default for SNalUnitHeaderExt {
    fn default() -> Self {
        Self {
            sNalUnitHeader: SNalUnitHeader::default(),
            bIdrFlag: false,
            uiPriorityId: 0,
            iNoInterLayerPredFlag: 0,
            uiDependencyId: 0,
            uiQualityId: 0,
            uiTemporalId: 0,
            bUseRefBasePicFlag: false,
            bDiscardableFlag: false,
            bOutputFlag: false,
            uiReservedThree2Bits: 0,
            uiLayerDqId: 0,
            bNalExtFlag: false,
        }
    }
}

/// `EWelsSliceType` — `codec/common/inc/wels_common_defs.h:163`.
///
/// Note `P_SLICE` is **0** and `I_SLICE` is **2**. `svc_set_mb_syn_cavlc.rs` used to
/// shadow these with `I_SLICE: i32 = 0` / `P_SLICE: i32 = 1`, which inverted the
/// mb-type offset switch at `svc_set_mb_syn_cavlc.cpp:76`.
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

/// `g_kuiGolombUELength` — `codec/common/src/common_tables.cpp:886`, declared at
/// `codec/common/inc/wels_common_defs.h:79`. The number of bits `ue(v)` needs for
/// each value 0..255.
///
/// This is the canonical copy. Three encoder modules each carried their own, and
/// **two of them were wrong**: `svc_set_mb_syn_cavlc.rs` had 253 entries and
/// `vlc_encoder.rs` had 274, both diverging from the real table at index 125. The
/// short one is the copy the macroblock-layer writer indexes, so any `ue(v)` of 253
/// or more indexed out of bounds — which is how a 320x192 encode aborted at frame 7
/// — and every value from 125 up was written with the wrong bit count.
pub const g_kuiGolombUELength: [u32; 256] = [
    1, 3, 3, 5, 5, 5, 5, 7, 7, 7, 7, 7, 7, 7, 7, 9,
    9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 11,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 17,
];
