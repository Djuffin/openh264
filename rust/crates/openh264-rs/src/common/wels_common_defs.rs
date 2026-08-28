#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code
)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! Types shared by the encoder and the decoder.
//!
//! Translated from `codec/common/inc/wels_common_defs.h`. Both codecs include that
//! header in C++, so these types have exactly one definition here rather than one
//! copy per module.

use crate::safe::plane::PlaneCursor;


// `TagBitStringAux` / `SBitStringAux` / `PBitStringAux` were declared here — the
// pointer-triple bitstream cursor of `codec/common/inc/wels_common_defs.h:232`
// (`pStartBuf`/`pEndBuf`/`pCurBuf` plus `iBits`, `iIndex` and the 32-bit
// accumulator), and Phase 3's emblem: the type its §1.2 taxonomy named as the
// thing the phase existed to remove.
//
// It has no users left. T3.1b and T3.2 moved the decoder's read side onto
// `safe::bits::BsCursor`, T3.3 gave the decoder an owned buffer, and T3.4 moved
// the encoder's write side onto `BsWriter`; what remained by then was a handful of
// `pub use` re-exports and two `pub type PBitStringAux = *mut SBitStringAux`
// aliases that nothing named. The inventory that licensed this deletion is in the
// commit message.
//
// Nothing crossed a C ABI with this layout — it appears in no header under
// `codec/api/wels/`, and `api/abi_guard.rs` guards the public surface separately.
// The encoder CABAC writer kept its own `m_pBufStart`/`m_pBufCur`/`m_pBufEnd`
// triple inside `SCabacCtx`, which never used this struct. T3.5 converted it to
// three `usize` offsets, and with that the bitstream layer holds no pointer
// cursor of any kind, on either side.


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

// ============================================================================
// PSNR — `codec/common/src/utils.cpp:74-125`
// ============================================================================

/// `CONST_FACTOR_PSNR` — utils.cpp:78. `10.0 / log(10.0)`, in `double`.
pub const CONST_FACTOR_PSNR: f64 = 4.342944819032518; // 10.0 / ln(10.0)

/// `CALC_PSNR` — utils.cpp:79. The multiply `65025.0 * w * h` is `double`; only
/// the final result narrows to `float`.
#[inline]
pub fn CALC_PSNR(w: i32, h: i32, s: i64) -> f32 {
    (CONST_FACTOR_PSNR * (65025.0f64 * w as f64 * h as f64 / s as f64).ln()) as f32
}

/// `WelsCalcPsnr` — `codec/common/src/utils.cpp:101`. The mean-square error of
/// `tar` against `refc` over a `kiWidth` x `kiHeight` rectangle, as a PSNR in dB.
///
/// Returns the saturating `99.99` for an exact match, which is the reference's own
/// sentinel rather than an error. `iSqe` accumulates in `i64` and the per-pixel
/// difference is `i32`, exactly as the C++ (`int64_t` / `int32_t`); the widths are
/// part of the contract, not an implementation choice.
///
/// **The null-plane sentinel moved to the caller (T9.B3).** The raw form took two
/// `*const u8` and returned `-1.0` when either was null — the C++'s "no picture
/// bound". A plane cursor cannot be null, so the caller answers that question
/// where it already has the handle: `encoder_ext.rs`'s `LayerPlanePsnr` returns
/// `-1.0` for an unresolved picture or an empty plane, which is the same value at
/// the same times.
pub fn calc_psnr(
    tar: &PlaneCursor<'_>,
    refc: &PlaneCursor<'_>,
    kiWidth: i32,
    kiHeight: i32,
) -> f32 {
    let mut iSqe: i64 = 0;

    for y in 0..kiHeight {
        let t = tar.row(y as isize, 0, kiWidth as usize);
        let r = refc.row(y as isize, 0, kiWidth as usize);
        for (a, b) in t.iter().zip(r.iter()) {
            let kiT = *a as i32 - *b as i32;
            iSqe += (kiT * kiT) as i64;
        }
    }
    if iSqe == 0 {
        return 99.99;
    }
    CALC_PSNR(kiWidth, kiHeight, iSqe)
}

#[cfg(test)]
mod psnr_tests {
    use super::*;

    /// Expectations **measured** against `libopenh264.a`, not derived: a probe
    /// called `WelsCalcPsnr` on the same inputs and printed these values.
    /// See `rust/docs/encoder_port_status.md`, Phase 5.3. The golden is the
    /// reference implementation, so this survives the shim's deletion intact —
    /// contrast F106.
    #[test]
    fn test_wels_calc_psnr_matches_cxx() {
        const W: i32 = 32;
        const H: i32 = 16;
        let n = (W * H) as usize;
        let a: Vec<u8> = (0..n).map(|i| (i * 7) as u8).collect();
        let mut b: Vec<u8> = a.clone();

        let call = |t: &[u8], ts: i32, r: &[u8], rs: i32, w: i32, h: i32| {
            calc_psnr(
                &PlaneCursor::new(t, 0, ts as usize),
                &PlaneCursor::new(r, 0, rs as usize),
                w,
                h,
            )
        };

        // Exactly equal planes return the 99.99 sentinel, not +inf.
        assert_eq!(call(&a, W, &b, W, W, H), 99.99);

        for i in 0..n {
            b[i] = a[i] ^ 1;
        }
        assert!((call(&a, W, &b, W, W, H) - 48.130802).abs() < 1e-4);

        for i in 0..n {
            b[i] = 255 - a[i];
        }
        assert!((call(&a, W, &b, W, W, H) - 4.737283).abs() < 1e-4);

        let mut s: u32 = 12345;
        for i in 0..n {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            b[i] = ((s >> 16) & 0xff) as u8;
        }
        assert!((call(&a, W, &b, W, W, H) - 8.052674).abs() < 1e-4);

        // Distinct target and reference strides.
        assert!((call(&a, 40, &b, 48, 20, 8) - 7.720985).abs() < 1e-4);
    }
}
