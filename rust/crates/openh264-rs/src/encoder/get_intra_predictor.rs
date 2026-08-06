//! Port of `codec/encoder/core/src/get_intra_predictor.cpp` — the encoder's intra
//! prediction sample generators and the `WelsInitIntraPredFuncs` table filler.
//!
//! These are **not** the decoder's predictors in `decoder/get_intra_predictor.rs`:
//! the encoder's take a separate `pRef` pointer into the reconstructed frame and
//! write into a packed prediction buffer (stride 4 for I4x4, 8 for chroma, 16 for
//! I16x16), while the decoder's predict in place with a single stride argument.
//!
//! Only the `_c` scalar variants exist here. The SIMD variants in the C++ are all
//! behind `uiCpuFlag` tests that do not fire on any target this port builds for;
//! `WelsCPUFeatureDetect` measured `0x00000000` against `libopenh264.a` on darwin/arm64.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use crate::common::intra_pred_common::{WelsI16x16LumaPredH_c, WelsI16x16LumaPredV_c};
use crate::encoder::svc_base_layer_md::{
    C_PRED_DC, C_PRED_DC_128, C_PRED_DC_L, C_PRED_DC_T, C_PRED_H, C_PRED_P, C_PRED_V, I4_PRED_DC,
    I4_PRED_DC_128, I4_PRED_DC_L, I4_PRED_DC_T, I4_PRED_DDL, I4_PRED_DDL_TOP, I4_PRED_DDR,
    I4_PRED_H, I4_PRED_HD, I4_PRED_HU, I4_PRED_V, I4_PRED_VL, I4_PRED_VL_TOP, I4_PRED_VR,
};
use crate::encoder::svc_mode_decision::{
    I16_PRED_DC, I16_PRED_DC_128, I16_PRED_DC_L, I16_PRED_DC_T, I16_PRED_H, I16_PRED_P, I16_PRED_V,
};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

const I8x8_PRED_STRIDE: usize = 8;

#[inline(always)]
fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        if -iX < 0 { 255 } else { 0 }
    } else {
        iX as u8
    }
}

#[inline(always)]
unsafe fn LD32(p: *const u8) -> u32 {
    (p as *const u32).read_unaligned()
}
#[inline(always)]
unsafe fn LD64(p: *const u8) -> u64 {
    (p as *const u64).read_unaligned()
}
#[inline(always)]
unsafe fn ST64(p: *mut u8, v: u64) {
    (p as *mut u64).write_unaligned(v);
}

/// `get_intra_predictor.cpp:55`. Writes `pSrc[0..8]` into both halves of the 16-byte
/// I4x4 prediction block.
#[inline(always)]
unsafe fn WelsFillingPred8to16(pPred: *mut u8, pSrc: *const u8) {
    let v = LD64(pSrc);
    ST64(pPred, v);
    ST64(pPred.add(8), v);
}

/// `get_intra_predictor.cpp:59`. Copies all 16 bytes.
#[inline(always)]
unsafe fn WelsFillingPred8x2to16(pPred: *mut u8, pSrc: *const u8) {
    ST64(pPred, LD64(pSrc));
    ST64(pPred.add(8), LD64(pSrc.add(8)));
}

/// `get_intra_predictor.cpp:63`. Broadcasts one byte across all 16.
#[inline(always)]
unsafe fn WelsFillingPred1to16(pPred: *mut u8, kuiSrc: u8) {
    let v = 0x0101_0101_0101_0101u64.wrapping_mul(kuiSrc as u64);
    ST64(pPred, v);
    ST64(pPred.add(8), v);
}

// ============================================================================
// I4x4 luma — `get_intra_predictor.cpp:79-398`
// ============================================================================

/// `get_intra_predictor.cpp:79`.
///
/// # Safety
/// `pPred` must be writable for 16 bytes; `pRef` must have the top row readable.
pub unsafe extern "C" fn WelsI4x4LumaPredV_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let kuiSrc = LD32(pRef.offset(-(kiStride as isize)));
    let uiSrcx2: [u32; 2] = [kuiSrc, kuiSrc];

    WelsFillingPred8to16(pPred, uiSrcx2.as_ptr() as *const u8);
}

/// `get_intra_predictor.cpp:87`.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`]; `pRef`'s four left neighbours must be readable.
pub unsafe extern "C" fn WelsI4x4LumaPredH_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let kiStridex2Left = (kiStride << 1) - 1;
    let kiStridex3Left = kiStride + kiStridex2Left;
    let kuiHor1 = *pRef.offset(-1);
    let kuiHor2 = *pRef.offset((kiStride - 1) as isize);
    let kuiHor3 = *pRef.offset(kiStridex2Left as isize);
    let kuiHor4 = *pRef.offset(kiStridex3Left as isize);
    let uiSrc: [u8; 16] = [
        kuiHor1, kuiHor1, kuiHor1, kuiHor1,
        kuiHor2, kuiHor2, kuiHor2, kuiHor2,
        kuiHor3, kuiHor3, kuiHor3, kuiHor3,
        kuiHor4, kuiHor4, kuiHor4, kuiHor4,
    ];

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:106`.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`]; both the top row and the four left neighbours must be
/// readable.
pub unsafe extern "C" fn WelsI4x4LumaPredDc_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let kuiDcValue = ((*pRef.offset(-1) as i32
        + *pRef.offset(s - 1) as i32
        + *pRef.offset((kiStride << 1) as isize - 1) as i32
        + *pRef.offset(((kiStride << 1) + kiStride) as isize - 1) as i32
        + *pRef.offset(-s) as i32
        + *pRef.offset(1 - s) as i32
        + *pRef.offset(2 - s) as i32
        + *pRef.offset(3 - s) as i32
        + 4)
        >> 3) as u8;

    WelsFillingPred1to16(pPred, kuiDcValue);
}

/// `get_intra_predictor.cpp:114`.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`]; the four left neighbours must be readable.
pub unsafe extern "C" fn WelsI4x4LumaPredDcLeft_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let kuiDcValue = ((*pRef.offset(-1) as i32
        + *pRef.offset(s - 1) as i32
        + *pRef.offset((kiStride << 1) as isize - 1) as i32
        + *pRef.offset(((kiStride << 1) + kiStride) as isize - 1) as i32
        + 2)
        >> 2) as u8;

    WelsFillingPred1to16(pPred, kuiDcValue);
}

/// `get_intra_predictor.cpp:121`.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`].
pub unsafe extern "C" fn WelsI4x4LumaPredDcTop_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let kuiDcValue = ((*pRef.offset(-s) as i32
        + *pRef.offset(1 - s) as i32
        + *pRef.offset(2 - s) as i32
        + *pRef.offset(3 - s) as i32
        + 2)
        >> 2) as u8;

    WelsFillingPred1to16(pPred, kuiDcValue);
}

/// `get_intra_predictor.cpp:127`.
///
/// # Safety
/// `pPred` must be writable for 16 bytes. `pRef` is unread.
pub unsafe extern "C" fn WelsI4x4LumaPredDcNA_c(pPred: *mut u8, _pRef: *mut u8, _kiStride: i32) {
    WelsFillingPred1to16(pPred, 0x80);
}

/// `get_intra_predictor.cpp:134` — down left.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`]; eight top samples must be readable.
pub unsafe extern "C" fn WelsI4x4LumaPredDDL_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let t = |i: isize| *pRef.offset(i - s) as i32;
    let (kuiT0, kuiT1, kuiT2, kuiT3) = (t(0), t(1), t(2), t(3));
    let (kuiT4, kuiT5, kuiT6, kuiT7) = (t(4), t(5), t(6), t(7));
    let kuiDDL0 = ((2 + kuiT0 + kuiT2 + (kuiT1 << 1)) >> 2) as u8;
    let kuiDDL1 = ((2 + kuiT1 + kuiT3 + (kuiT2 << 1)) >> 2) as u8;
    let kuiDDL2 = ((2 + kuiT2 + kuiT4 + (kuiT3 << 1)) >> 2) as u8;
    let kuiDDL3 = ((2 + kuiT3 + kuiT5 + (kuiT4 << 1)) >> 2) as u8;
    let kuiDDL4 = ((2 + kuiT4 + kuiT6 + (kuiT5 << 1)) >> 2) as u8;
    let kuiDDL5 = ((2 + kuiT5 + kuiT7 + (kuiT6 << 1)) >> 2) as u8;
    let kuiDDL6 = ((2 + kuiT6 + kuiT7 + (kuiT7 << 1)) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    uiSrc[0] = kuiDDL0;
    uiSrc[1] = kuiDDL1;
    uiSrc[4] = kuiDDL1;
    uiSrc[2] = kuiDDL2;
    uiSrc[5] = kuiDDL2;
    uiSrc[8] = kuiDDL2;
    uiSrc[3] = kuiDDL3;
    uiSrc[6] = kuiDDL3;
    uiSrc[9] = kuiDDL3;
    uiSrc[12] = kuiDDL3;
    uiSrc[7] = kuiDDL4;
    uiSrc[10] = kuiDDL4;
    uiSrc[13] = kuiDDL4;
    uiSrc[11] = kuiDDL5;
    uiSrc[14] = kuiDDL5;
    uiSrc[15] = kuiDDL6;

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:164` — down left, right-top replaced by padding.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`].
pub unsafe extern "C" fn WelsI4x4LumaPredDDLTop_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let t = |i: isize| *pRef.offset(i - s) as i32;
    let (kuiT0, kuiT1, kuiT2, kuiT3) = (t(0), t(1), t(2), t(3));
    let kuiDLT0 = ((2 + kuiT0 + kuiT2 + (kuiT1 << 1)) >> 2) as u8;
    let kuiDLT1 = ((2 + kuiT1 + kuiT3 + (kuiT2 << 1)) >> 2) as u8;
    let kuiDLT2 = ((2 + kuiT2 + kuiT3 + (kuiT3 << 1)) >> 2) as u8;
    let kuiDLT3 = ((2 + (kuiT3 << 2)) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    // memset first, then the individual assignments overwrite part of it.
    uiSrc[6..16].fill(kuiDLT3);
    uiSrc[0] = kuiDLT0;
    uiSrc[1] = kuiDLT1;
    uiSrc[4] = kuiDLT1;
    uiSrc[2] = kuiDLT2;
    uiSrc[5] = kuiDLT2;
    uiSrc[8] = kuiDLT2;
    uiSrc[3] = kuiDLT3;

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:186` — down right.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`]; the top-left sample, four top and four left samples
/// must be readable.
pub unsafe extern "C" fn WelsI4x4LumaPredDDR_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let kiStridex2 = kiStride << 1;
    let kiStridex3 = kiStride + kiStridex2;
    let kuiLT = *pRef.offset(-s - 1) as i32;
    let kuiL0 = *pRef.offset(-1) as i32;
    let kuiL1 = *pRef.offset((kiStride - 1) as isize) as i32;
    let kuiL2 = *pRef.offset((kiStridex2 - 1) as isize) as i32;
    let kuiL3 = *pRef.offset((kiStridex3 - 1) as isize) as i32;
    let kuiT0 = *pRef.offset(-s) as i32;
    let kuiT1 = *pRef.offset(1 - s) as i32;
    let kuiT2 = *pRef.offset(2 - s) as i32;
    let kuiT3 = *pRef.offset(3 - s) as i32;
    let kuiTL0 = 1 + kuiLT + kuiL0;
    let kuiLT0 = 1 + kuiLT + kuiT0;
    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiL01 = 1 + kuiL0 + kuiL1;
    let kuiL12 = 1 + kuiL1 + kuiL2;
    let kuiL23 = 1 + kuiL2 + kuiL3;
    let kuiDDR0 = ((kuiTL0 + kuiLT0) >> 2) as u8;
    let kuiDDR1 = ((kuiLT0 + kuiT01) >> 2) as u8;
    let kuiDDR2 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiDDR3 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiDDR4 = ((kuiTL0 + kuiL01) >> 2) as u8;
    let kuiDDR5 = ((kuiL01 + kuiL12) >> 2) as u8;
    let kuiDDR6 = ((kuiL12 + kuiL23) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    uiSrc[0] = kuiDDR0;
    uiSrc[5] = kuiDDR0;
    uiSrc[10] = kuiDDR0;
    uiSrc[15] = kuiDDR0;
    uiSrc[1] = kuiDDR1;
    uiSrc[6] = kuiDDR1;
    uiSrc[11] = kuiDDR1;
    uiSrc[2] = kuiDDR2;
    uiSrc[7] = kuiDDR2;
    uiSrc[3] = kuiDDR3;
    uiSrc[4] = kuiDDR4;
    uiSrc[9] = kuiDDR4;
    uiSrc[14] = kuiDDR4;
    uiSrc[8] = kuiDDR5;
    uiSrc[13] = kuiDDR5;
    uiSrc[12] = kuiDDR6;

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:228` — vertical left.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`]; seven top samples must be readable.
pub unsafe extern "C" fn WelsI4x4LumaPredVL_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let t = |i: isize| *pRef.offset(i - s) as i32;
    let (kuiT0, kuiT1, kuiT2, kuiT3) = (t(0), t(1), t(2), t(3));
    let (kuiT4, kuiT5, kuiT6) = (t(4), t(5), t(6));
    let kuiVL0 = ((1 + kuiT0 + kuiT1) >> 1) as u8;
    let kuiVL1 = ((1 + kuiT1 + kuiT2) >> 1) as u8;
    let kuiVL2 = ((1 + kuiT2 + kuiT3) >> 1) as u8;
    let kuiVL3 = ((1 + kuiT3 + kuiT4) >> 1) as u8;
    let kuiVL4 = ((1 + kuiT4 + kuiT5) >> 1) as u8;
    let kuiVL5 = ((2 + kuiT0 + (kuiT1 << 1) + kuiT2) >> 2) as u8;
    let kuiVL6 = ((2 + kuiT1 + (kuiT2 << 1) + kuiT3) >> 2) as u8;
    let kuiVL7 = ((2 + kuiT2 + (kuiT3 << 1) + kuiT4) >> 2) as u8;
    let kuiVL8 = ((2 + kuiT3 + (kuiT4 << 1) + kuiT5) >> 2) as u8;
    let kuiVL9 = ((2 + kuiT4 + (kuiT5 << 1) + kuiT6) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    uiSrc[0] = kuiVL0;
    uiSrc[1] = kuiVL1;
    uiSrc[8] = kuiVL1;
    uiSrc[2] = kuiVL2;
    uiSrc[9] = kuiVL2;
    uiSrc[3] = kuiVL3;
    uiSrc[10] = kuiVL3;
    uiSrc[4] = kuiVL5;
    uiSrc[5] = kuiVL6;
    uiSrc[12] = kuiVL6;
    uiSrc[6] = kuiVL7;
    uiSrc[13] = kuiVL7;
    uiSrc[7] = kuiVL8;
    uiSrc[14] = kuiVL8;
    uiSrc[11] = kuiVL4;
    uiSrc[15] = kuiVL9;

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:265` — vertical left, right-top replaced by padding.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`].
pub unsafe extern "C" fn WelsI4x4LumaPredVLTop_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let pTopLeft = pRef.offset(-(kiStride as isize) - 1);
    let kuiT0 = *pTopLeft.add(1) as i32;
    let kuiT1 = *pTopLeft.add(2) as i32;
    let kuiT2 = *pTopLeft.add(3) as i32;
    let kuiT3 = *pTopLeft.add(4) as i32;
    let kuiVLT0 = ((1 + kuiT0 + kuiT1) >> 1) as u8;
    let kuiVLT1 = ((1 + kuiT1 + kuiT2) >> 1) as u8;
    let kuiVLT2 = ((1 + kuiT2 + kuiT3) >> 1) as u8;
    let kuiVLT3 = ((1 + (kuiT3 << 1)) >> 1) as u8;
    let kuiVLT4 = ((2 + kuiT0 + (kuiT1 << 1) + kuiT2) >> 2) as u8;
    let kuiVLT5 = ((2 + kuiT1 + (kuiT2 << 1) + kuiT3) >> 2) as u8;
    let kuiVLT6 = ((2 + kuiT2 + (kuiT3 << 1) + kuiT3) >> 2) as u8;
    let kuiVLT7 = ((2 + (kuiT3 << 2)) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    uiSrc[0] = kuiVLT0;
    uiSrc[1] = kuiVLT1;
    uiSrc[8] = kuiVLT1;
    uiSrc[2] = kuiVLT2;
    uiSrc[9] = kuiVLT2;
    uiSrc[3] = kuiVLT3;
    uiSrc[10] = kuiVLT3;
    uiSrc[11] = kuiVLT3;
    uiSrc[4] = kuiVLT4;
    uiSrc[5] = kuiVLT5;
    uiSrc[12] = kuiVLT5;
    uiSrc[6] = kuiVLT6;
    uiSrc[13] = kuiVLT6;
    uiSrc[7] = kuiVLT7;
    uiSrc[14] = kuiVLT7;
    uiSrc[15] = kuiVLT7;

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:294` — vertical right.
///
/// # Safety
/// See [`WelsI4x4LumaPredDDR_c`].
pub unsafe extern "C" fn WelsI4x4LumaPredVR_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let kiStridex2 = kiStride << 1;
    let kuiLT = *pRef.offset(-s - 1) as i32;
    let kuiL0 = *pRef.offset(-1) as i32;
    let kuiL1 = *pRef.offset((kiStride - 1) as isize) as i32;
    let kuiL2 = *pRef.offset((kiStridex2 - 1) as isize) as i32;
    let kuiT0 = *pRef.offset(-s) as i32;
    let kuiT1 = *pRef.offset(1 - s) as i32;
    let kuiT2 = *pRef.offset(2 - s) as i32;
    let kuiT3 = *pRef.offset(3 - s) as i32;
    let kuiVR0 = ((1 + kuiLT + kuiT0) >> 1) as u8;
    let kuiVR1 = ((1 + kuiT0 + kuiT1) >> 1) as u8;
    let kuiVR2 = ((1 + kuiT1 + kuiT2) >> 1) as u8;
    let kuiVR3 = ((1 + kuiT2 + kuiT3) >> 1) as u8;
    let kuiVR4 = ((2 + kuiL0 + (kuiLT << 1) + kuiT0) >> 2) as u8;
    let kuiVR5 = ((2 + kuiLT + (kuiT0 << 1) + kuiT1) >> 2) as u8;
    let kuiVR6 = ((2 + kuiT0 + (kuiT1 << 1) + kuiT2) >> 2) as u8;
    let kuiVR7 = ((2 + kuiT1 + (kuiT2 << 1) + kuiT3) >> 2) as u8;
    let kuiVR8 = ((2 + kuiLT + (kuiL0 << 1) + kuiL1) >> 2) as u8;
    let kuiVR9 = ((2 + kuiL0 + (kuiL1 << 1) + kuiL2) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    uiSrc[0] = kuiVR0;
    uiSrc[9] = kuiVR0;
    uiSrc[1] = kuiVR1;
    uiSrc[10] = kuiVR1;
    uiSrc[2] = kuiVR2;
    uiSrc[11] = kuiVR2;
    uiSrc[3] = kuiVR3;
    uiSrc[4] = kuiVR4;
    uiSrc[13] = kuiVR4;
    uiSrc[5] = kuiVR5;
    uiSrc[14] = kuiVR5;
    uiSrc[6] = kuiVR6;
    uiSrc[15] = kuiVR6;
    uiSrc[7] = kuiVR7;
    uiSrc[8] = kuiVR8;
    uiSrc[12] = kuiVR9;

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:332` — horizontal up.
///
/// # Safety
/// See [`WelsI4x4LumaPredV_c`]; the four left neighbours must be readable.
pub unsafe extern "C" fn WelsI4x4LumaPredHU_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let kiStridex2 = kiStride << 1;
    let kiStridex3 = kiStride + kiStridex2;
    let kuiL0 = *pRef.offset(-1) as i32;
    let kuiL1 = *pRef.offset((kiStride - 1) as isize) as i32;
    let kuiL2 = *pRef.offset((kiStridex2 - 1) as isize) as i32;
    let kuiL3 = *pRef.offset((kiStridex3 - 1) as isize) as i32;
    let kuiL01 = 1 + kuiL0 + kuiL1;
    let kuiL12 = 1 + kuiL1 + kuiL2;
    let kuiL23 = 1 + kuiL2 + kuiL3;
    let kuiHU0 = (kuiL01 >> 1) as u8;
    let kuiHU1 = ((kuiL01 + kuiL12) >> 2) as u8;
    let kuiHU2 = (kuiL12 >> 1) as u8;
    let kuiHU3 = ((kuiL12 + kuiL23) >> 2) as u8;
    let kuiHU4 = (kuiL23 >> 1) as u8;
    let kuiHU5 = ((1 + kuiL23 + (kuiL3 << 1)) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    uiSrc[0] = kuiHU0;
    uiSrc[1] = kuiHU1;
    uiSrc[2] = kuiHU2;
    uiSrc[4] = kuiHU2;
    uiSrc[3] = kuiHU3;
    uiSrc[5] = kuiHU3;
    uiSrc[6] = kuiHU4;
    uiSrc[8] = kuiHU4;
    uiSrc[7] = kuiHU5;
    uiSrc[9] = kuiHU5;
    uiSrc[10..16].fill(kuiL3 as u8);

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

/// `get_intra_predictor.cpp:363` — horizontal down.
///
/// # Safety
/// See [`WelsI4x4LumaPredDDR_c`].
pub unsafe extern "C" fn WelsI4x4LumaPredHD_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let kiStridex2 = kiStride << 1;
    let kiStridex3 = kiStride + kiStridex2;
    let kuiLT = *pRef.offset(-s - 1) as i32;
    let kuiL0 = *pRef.offset(-1) as i32;
    let kuiL1 = *pRef.offset((kiStride - 1) as isize) as i32;
    let kuiL2 = *pRef.offset((kiStridex2 - 1) as isize) as i32;
    let kuiL3 = *pRef.offset((kiStridex3 - 1) as isize) as i32;
    let kuiT0 = *pRef.offset(-s) as i32;
    let kuiT1 = *pRef.offset(1 - s) as i32;
    let kuiT2 = *pRef.offset(2 - s) as i32;
    let kuiHD0 = ((1 + kuiLT + kuiL0) >> 1) as u8;
    let kuiHD1 = ((2 + kuiL0 + (kuiLT << 1) + kuiT0) >> 2) as u8;
    let kuiHD2 = ((2 + kuiLT + (kuiT0 << 1) + kuiT1) >> 2) as u8;
    let kuiHD3 = ((2 + kuiT0 + (kuiT1 << 1) + kuiT2) >> 2) as u8;
    let kuiHD4 = ((1 + kuiL0 + kuiL1) >> 1) as u8;
    let kuiHD5 = ((2 + kuiLT + (kuiL0 << 1) + kuiL1) >> 2) as u8;
    let kuiHD6 = ((1 + kuiL1 + kuiL2) >> 1) as u8;
    let kuiHD7 = ((2 + kuiL0 + (kuiL1 << 1) + kuiL2) >> 2) as u8;
    let kuiHD8 = ((1 + kuiL2 + kuiL3) >> 1) as u8;
    let kuiHD9 = ((2 + kuiL1 + (kuiL2 << 1) + kuiL3) >> 2) as u8;
    let mut uiSrc = [0u8; 16];
    uiSrc[0] = kuiHD0;
    uiSrc[6] = kuiHD0;
    uiSrc[1] = kuiHD1;
    uiSrc[7] = kuiHD1;
    uiSrc[2] = kuiHD2;
    uiSrc[3] = kuiHD3;
    uiSrc[4] = kuiHD4;
    uiSrc[10] = kuiHD4;
    uiSrc[5] = kuiHD5;
    uiSrc[11] = kuiHD5;
    uiSrc[8] = kuiHD6;
    uiSrc[14] = kuiHD6;
    uiSrc[9] = kuiHD7;
    uiSrc[15] = kuiHD7;
    uiSrc[12] = kuiHD8;
    uiSrc[13] = kuiHD9;

    WelsFillingPred8x2to16(pPred, uiSrc.as_ptr());
}

// ============================================================================
// Chroma 8x8 — `get_intra_predictor.cpp:404-539`
// ============================================================================

/// `get_intra_predictor.cpp:404`.
///
/// # Safety
/// `pPred` must be writable for 64 bytes; the eight top samples must be readable.
pub unsafe extern "C" fn WelsIChromaPredV_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let kuiSrc64 = LD64(pRef.offset(-(kiStride as isize)));

    for i in 0..8usize {
        ST64(pPred.add(i * 8), kuiSrc64);
    }
}

/// `get_intra_predictor.cpp:417`.
///
/// # Safety
/// See [`WelsIChromaPredV_c`]; the eight left neighbours must be readable.
pub unsafe extern "C" fn WelsIChromaPredH_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let mut iStridex7 = (kiStride << 3) - kiStride;
    let mut iI8x8Stridex7 = (I8x8_PRED_STRIDE << 3) - I8x8_PRED_STRIDE;

    for _ in 0..8 {
        let kuiLeft = *pRef.offset((iStridex7 - 1) as isize);
        let kuiSrc64 = 0x0101_0101_0101_0101u64.wrapping_mul(kuiLeft as u64);
        ST64(pPred.add(iI8x8Stridex7), kuiSrc64);

        iStridex7 -= kiStride;
        iI8x8Stridex7 = iI8x8Stridex7.wrapping_sub(I8x8_PRED_STRIDE);
    }
}

/// `get_intra_predictor.cpp:433`.
///
/// # Safety
/// See [`WelsIChromaPredV_c`]; both the top row and the eight left neighbours must be
/// readable.
pub unsafe extern "C" fn WelsIChromaPredPlane_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let mut iTopSum: i32 = 0;
    let mut iLeftSum: i32 = 0;
    let pTop = pRef.offset(-(kiStride as isize));
    let pLeft = pRef.offset(-1);

    for i in 0..4i32 {
        iTopSum += (i + 1) * (*pTop.offset((4 + i) as isize) as i32 - *pTop.offset((2 - i) as isize) as i32);
        iLeftSum += (i + 1)
            * (*pLeft.offset(((4 + i) * kiStride) as isize) as i32
                - *pLeft.offset(((2 - i) * kiStride) as isize) as i32);
    }

    let iLTshift = (*pLeft.offset((7 * kiStride) as isize) as i32 + *pTop.offset(7) as i32) << 4;
    let iTopshift = (17 * iTopSum + 16) >> 5;
    let iLeftshift = (17 * iLeftSum + 16) >> 5;

    let mut pDst = pPred;
    for i in 0..8i32 {
        for j in 0..8i32 {
            *pDst.offset(j as isize) =
                WelsClip1((iLTshift + iTopshift * (j - 3) + iLeftshift * (i - 3) + 16) >> 5);
        }
        pDst = pDst.add(I8x8_PRED_STRIDE);
    }
}

/// `get_intra_predictor.cpp:457`.
///
/// # Safety
/// See [`WelsIChromaPredPlane_c`].
pub unsafe extern "C" fn WelsIChromaPredDc_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    let kuiL1 = kiStride - 1;
    let kuiL2 = kuiL1 + kiStride;
    let kuiL3 = kuiL2 + kiStride;
    let kuiL4 = kuiL3 + kiStride;
    let kuiL5 = kuiL4 + kiStride;
    let kuiL6 = kuiL5 + kiStride;
    let kuiL7 = kuiL6 + kiStride;
    let at = |o: i32| *pRef.offset(o as isize) as i32;
    /*caculate the iMean value*/
    let kuiMean1 = ((*pRef.offset(-s) as i32
        + *pRef.offset(1 - s) as i32
        + *pRef.offset(2 - s) as i32
        + *pRef.offset(3 - s) as i32
        + *pRef.offset(-1) as i32
        + at(kuiL1)
        + at(kuiL2)
        + at(kuiL3)
        + 4)
        >> 3) as u8;
    let kuiSum2 = *pRef.offset(4 - s) as u32
        + *pRef.offset(5 - s) as u32
        + *pRef.offset(6 - s) as u32
        + *pRef.offset(7 - s) as u32;
    let kuiSum3 =
        at(kuiL4) as u32 + at(kuiL5) as u32 + at(kuiL6) as u32 + at(kuiL7) as u32;
    let kuiMean2 = ((kuiSum2 + 2) >> 2) as u8;
    let kuiMean3 = ((kuiSum3 + 2) >> 2) as u8;
    let kuiMean4 = ((kuiSum2 + kuiSum3 + 4) >> 3) as u8;

    let kuiTopMean: [u8; 8] =
        [kuiMean1, kuiMean1, kuiMean1, kuiMean1, kuiMean2, kuiMean2, kuiMean2, kuiMean2];
    let kuiBottomMean: [u8; 8] =
        [kuiMean3, kuiMean3, kuiMean3, kuiMean3, kuiMean4, kuiMean4, kuiMean4, kuiMean4];
    let kuiTopMean64 = LD64(kuiTopMean.as_ptr());
    let kuiBottomMean64 = LD64(kuiBottomMean.as_ptr());

    for i in 0..4usize {
        ST64(pPred.add(i * 8), kuiTopMean64);
    }
    for i in 4..8usize {
        ST64(pPred.add(i * 8), kuiBottomMean64);
    }
}

/// `get_intra_predictor.cpp:489`.
///
/// # Safety
/// See [`WelsIChromaPredV_c`]; the eight left neighbours must be readable.
pub unsafe extern "C" fn WelsIChromaPredDcLeft_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let kuiL1 = kiStride - 1;
    let kuiL2 = kuiL1 + kiStride;
    let kuiL3 = kuiL2 + kiStride;
    let kuiL4 = kuiL3 + kiStride;
    let kuiL5 = kuiL4 + kiStride;
    let kuiL6 = kuiL5 + kiStride;
    let kuiL7 = kuiL6 + kiStride;
    let at = |o: i32| *pRef.offset(o as isize) as i32;
    /*caculate the iMean value*/
    let kuiTopMean = ((*pRef.offset(-1) as i32 + at(kuiL1) + at(kuiL2) + at(kuiL3) + 2) >> 2) as u8;
    let kuiBottomMean = ((at(kuiL4) + at(kuiL5) + at(kuiL6) + at(kuiL7) + 2) >> 2) as u8;
    let kuiTopMean64 = 0x0101_0101_0101_0101u64.wrapping_mul(kuiTopMean as u64);
    let kuiBottomMean64 = 0x0101_0101_0101_0101u64.wrapping_mul(kuiBottomMean as u64);

    for i in 0..4usize {
        ST64(pPred.add(i * 8), kuiTopMean64);
    }
    for i in 4..8usize {
        ST64(pPred.add(i * 8), kuiBottomMean64);
    }
}

/// `get_intra_predictor.cpp:512`.
///
/// # Safety
/// See [`WelsIChromaPredV_c`].
pub unsafe extern "C" fn WelsIChromaPredDcTop_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let s = kiStride as isize;
    /*caculate the iMean value*/
    let kuiMean1 = ((*pRef.offset(-s) as i32
        + *pRef.offset(1 - s) as i32
        + *pRef.offset(2 - s) as i32
        + *pRef.offset(3 - s) as i32
        + 2)
        >> 2) as u8;
    let kuiMean2 = ((*pRef.offset(4 - s) as i32
        + *pRef.offset(5 - s) as i32
        + *pRef.offset(6 - s) as i32
        + *pRef.offset(7 - s) as i32
        + 2)
        >> 2) as u8;
    let kuiMean: [u8; 8] =
        [kuiMean1, kuiMean1, kuiMean1, kuiMean1, kuiMean2, kuiMean2, kuiMean2, kuiMean2];
    let kuiMean64 = LD64(kuiMean.as_ptr());

    for i in 0..8usize {
        ST64(pPred.add(i * 8), kuiMean64);
    }
}

/// `get_intra_predictor.cpp:529`.
///
/// # Safety
/// `pPred` must be writable for 64 bytes. `pRef` is unread.
pub unsafe extern "C" fn WelsIChromaPredDcNA_c(pPred: *mut u8, _pRef: *mut u8, _kiStride: i32) {
    let kuiDcValue64 = 0x8080_8080_8080_8080u64;
    for i in 0..8usize {
        ST64(pPred.add(i * 8), kuiDcValue64);
    }
}

// ============================================================================
// I16x16 luma — `get_intra_predictor.cpp:542-612`
// ============================================================================

/// `get_intra_predictor.cpp:542`.
///
/// # Safety
/// `pPred` must be writable for 256 bytes; the 16 top samples and 16 left neighbours
/// must be readable.
pub unsafe extern "C" fn WelsI16x16LumaPredPlane_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let mut iTopSum: i32 = 0;
    let mut iLeftSum: i32 = 0;
    let pTop = pRef.offset(-(kiStride as isize));
    let pLeft = pRef.offset(-1);
    let iPredStride: usize = 16;

    for i in 0..8i32 {
        iTopSum += (i + 1) * (*pTop.offset((8 + i) as isize) as i32 - *pTop.offset((6 - i) as isize) as i32);
        iLeftSum += (i + 1)
            * (*pLeft.offset(((8 + i) * kiStride) as isize) as i32
                - *pLeft.offset(((6 - i) * kiStride) as isize) as i32);
    }

    let iLTshift = (*pLeft.offset((15 * kiStride) as isize) as i32 + *pTop.offset(15) as i32) << 4;
    let iTopshift = (5 * iTopSum + 32) >> 6;
    let iLeftshift = (5 * iLeftSum + 32) >> 6;

    let mut pDst = pPred;
    for i in 0..16i32 {
        for j in 0..16i32 {
            *pDst.offset(j as isize) =
                WelsClip1((iLTshift + iTopshift * (j - 7) + iLeftshift * (i - 7) + 16) >> 5);
        }
        pDst = pDst.add(iPredStride);
    }
}

/// `get_intra_predictor.cpp:566`.
///
/// # Safety
/// See [`WelsI16x16LumaPredPlane_c`].
pub unsafe extern "C" fn WelsI16x16LumaPredDc_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let mut iStridex15 = (kiStride << 4) - kiStride;
    let mut iSum: i32 = 0;

    /*caculate the iMean value*/
    for i in (0..16i32).rev() {
        iSum += *pRef.offset((-1 + iStridex15) as isize) as i32
            + *pRef.offset((-kiStride + i) as isize) as i32;
        iStridex15 -= kiStride;
    }
    let iMean = ((16 + iSum) >> 5) as u8;
    core::ptr::write_bytes(pPred, iMean, 256);
}

/// `get_intra_predictor.cpp:582`.
///
/// # Safety
/// See [`WelsI16x16LumaPredPlane_c`]; only the 16 top samples are read.
pub unsafe extern "C" fn WelsI16x16LumaPredDcTop_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let mut iSum: i32 = 0;

    /*caculate the iMean value*/
    for i in (0..16i32).rev() {
        iSum += *pRef.offset((-kiStride + i) as isize) as i32;
    }
    let iMean = ((8 + iSum) >> 4) as u8;
    core::ptr::write_bytes(pPred, iMean, 256);
}

/// `get_intra_predictor.cpp:595`.
///
/// # Safety
/// See [`WelsI16x16LumaPredPlane_c`]; only the 16 left neighbours are read.
pub unsafe extern "C" fn WelsI16x16LumaPredDcLeft_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    let mut iStridex15 = (kiStride << 4) - kiStride;
    let mut iSum: i32 = 0;

    /*caculate the iMean value*/
    for _ in 0..16 {
        iSum += *pRef.offset((-1 + iStridex15) as isize) as i32;
        iStridex15 -= kiStride;
    }
    let iMean = ((8 + iSum) >> 4) as u8;
    core::ptr::write_bytes(pPred, iMean, 256);
}

/// `get_intra_predictor.cpp:610`.
///
/// # Safety
/// `pPred` must be writable for 256 bytes. `pRef` is unread.
pub unsafe extern "C" fn WelsI16x16LumaPredDcNA_c(pPred: *mut u8, _pRef: *mut u8, _kiStride: i32) {
    core::ptr::write_bytes(pPred, 0x80, 256);
}

/// `get_intra_predictor.cpp:614`. Installs the scalar predictor tables. The SIMD
/// overrides that follow in the C++ are all guarded by `kuiCpuFlag & WELS_CPU_*`,
/// which is 0 on every target this port builds for, so none are translated.
///
/// # Safety
/// `pFuncList` must be a valid, writable `SWelsFuncPtrList`.
pub unsafe fn WelsInitIntraPredFuncs(pFuncList: *mut SWelsFuncPtrList, _kuiCpuFlag: u32) {
    let fl = &mut *pFuncList;

    fl.pfGetLumaI16x16Pred[I16_PRED_V as usize] = Some(WelsI16x16LumaPredV_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_H as usize] = Some(WelsI16x16LumaPredH_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC as usize] = Some(WelsI16x16LumaPredDc_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_P as usize] = Some(WelsI16x16LumaPredPlane_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC_L as usize] = Some(WelsI16x16LumaPredDcLeft_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC_T as usize] = Some(WelsI16x16LumaPredDcTop_c);
    fl.pfGetLumaI16x16Pred[I16_PRED_DC_128 as usize] = Some(WelsI16x16LumaPredDcNA_c);

    fl.pfGetLumaI4x4Pred[I4_PRED_V as usize] = Some(WelsI4x4LumaPredV_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_H as usize] = Some(WelsI4x4LumaPredH_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC as usize] = Some(WelsI4x4LumaPredDc_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC_L as usize] = Some(WelsI4x4LumaPredDcLeft_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC_T as usize] = Some(WelsI4x4LumaPredDcTop_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DC_128 as usize] = Some(WelsI4x4LumaPredDcNA_c);

    fl.pfGetLumaI4x4Pred[I4_PRED_DDL as usize] = Some(WelsI4x4LumaPredDDL_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DDL_TOP as usize] = Some(WelsI4x4LumaPredDDLTop_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_DDR as usize] = Some(WelsI4x4LumaPredDDR_c);

    fl.pfGetLumaI4x4Pred[I4_PRED_VL as usize] = Some(WelsI4x4LumaPredVL_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_VL_TOP as usize] = Some(WelsI4x4LumaPredVLTop_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_VR as usize] = Some(WelsI4x4LumaPredVR_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_HU as usize] = Some(WelsI4x4LumaPredHU_c);
    fl.pfGetLumaI4x4Pred[I4_PRED_HD as usize] = Some(WelsI4x4LumaPredHD_c);

    fl.pfGetChromaPred[C_PRED_DC as usize] = Some(WelsIChromaPredDc_c);
    fl.pfGetChromaPred[C_PRED_H as usize] = Some(WelsIChromaPredH_c);
    fl.pfGetChromaPred[C_PRED_V as usize] = Some(WelsIChromaPredV_c);
    fl.pfGetChromaPred[C_PRED_P as usize] = Some(WelsIChromaPredPlane_c);
    fl.pfGetChromaPred[C_PRED_DC_L as usize] = Some(WelsIChromaPredDcLeft_c);
    fl.pfGetChromaPred[C_PRED_DC_T as usize] = Some(WelsIChromaPredDcTop_c);
    fl.pfGetChromaPred[C_PRED_DC_128 as usize] = Some(WelsIChromaPredDcNA_c);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a reference plane with a known ramp and a `pRef` pointing at (1,1) so the
    /// top row and left column are both in range.
    fn ramp_plane(stride: usize, rows: usize) -> Vec<u8> {
        (0..stride * rows).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn i4x4_dc_na_fills_0x80() {
        let mut pred = [0u8; 16];
        unsafe { WelsI4x4LumaPredDcNA_c(pred.as_mut_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(pred, [0x80u8; 16]);
    }

    #[test]
    fn i16x16_dc_na_fills_0x80() {
        let mut pred = [0u8; 256];
        unsafe { WelsI16x16LumaPredDcNA_c(pred.as_mut_ptr(), core::ptr::null_mut(), 0) };
        assert!(pred.iter().all(|&b| b == 0x80));
    }

    #[test]
    fn chroma_dc_na_fills_0x80() {
        let mut pred = [0u8; 64];
        unsafe { WelsIChromaPredDcNA_c(pred.as_mut_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(pred, [0x80u8; 64]);
    }

    /// I4x4 vertical replicates the four top samples down all four rows; horizontal
    /// replicates each left sample across its row.
    #[test]
    fn i4x4_v_and_h_replicate_their_edge() {
        let stride = 32usize;
        let mut plane = ramp_plane(stride, 24);
        let refp = unsafe { plane.as_mut_ptr().add(stride + 1) };

        let top: [u8; 4] = [
            plane[1], plane[2], plane[3], plane[4],
        ];
        let mut pred = [0u8; 16];
        unsafe { WelsI4x4LumaPredV_c(pred.as_mut_ptr(), refp, stride as i32) };
        for r in 0..4 {
            assert_eq!(&pred[r * 4..r * 4 + 4], &top, "V row {r}");
        }

        unsafe { WelsI4x4LumaPredH_c(pred.as_mut_ptr(), refp, stride as i32) };
        for r in 0..4 {
            let left = plane[(1 + r) * stride];
            assert_eq!(&pred[r * 4..r * 4 + 4], &[left; 4], "H row {r}");
        }
    }

    /// I16x16 DC is the rounded mean of the 16 top and 16 left samples; DcTop and
    /// DcLeft use only one edge with a different rounding shift.
    #[test]
    fn i16x16_dc_variants_match_their_definitions() {
        let stride = 48usize;
        let mut plane = ramp_plane(stride, 40);
        let refp = unsafe { plane.as_mut_ptr().add(stride + 1) };

        let top_sum: i32 = (0..16).map(|i| plane[1 + i] as i32).sum();
        let left_sum: i32 = (0..16).map(|i| plane[(1 + i) * stride] as i32).sum();

        let mut pred = [0u8; 256];
        unsafe { WelsI16x16LumaPredDc_c(pred.as_mut_ptr(), refp, stride as i32) };
        assert!(pred.iter().all(|&b| b == pred[0]));
        assert_eq!(pred[0], ((16 + top_sum + left_sum) >> 5) as u8);

        unsafe { WelsI16x16LumaPredDcTop_c(pred.as_mut_ptr(), refp, stride as i32) };
        assert_eq!(pred[0], ((8 + top_sum) >> 4) as u8);

        unsafe { WelsI16x16LumaPredDcLeft_c(pred.as_mut_ptr(), refp, stride as i32) };
        assert_eq!(pred[0], ((8 + left_sum) >> 4) as u8);
    }

    /// Chroma vertical writes the same eight top samples into all eight rows of the
    /// stride-8 prediction block; horizontal broadcasts each left sample.
    #[test]
    fn chroma_v_and_h_replicate_their_edge() {
        let stride = 32usize;
        let mut plane = ramp_plane(stride, 24);
        let refp = unsafe { plane.as_mut_ptr().add(stride + 1) };
        let top: Vec<u8> = (0..8).map(|i| plane[1 + i]).collect();

        let mut pred = [0u8; 64];
        unsafe { WelsIChromaPredV_c(pred.as_mut_ptr(), refp, stride as i32) };
        for r in 0..8 {
            assert_eq!(&pred[r * 8..r * 8 + 8], &top[..], "V row {r}");
        }

        unsafe { WelsIChromaPredH_c(pred.as_mut_ptr(), refp, stride as i32) };
        for r in 0..8 {
            let left = plane[(1 + r) * stride];
            assert_eq!(&pred[r * 8..r * 8 + 8], &[left; 8], "H row {r}");
        }
    }

    /// A flat reference plane must produce a flat prediction for every mode — the
    /// cheapest check that the DDL/DDR/VL/VR/HU/HD tap patterns cover all 16 samples.
    #[test]
    fn all_i4x4_modes_are_flat_on_a_flat_plane() {
        let stride = 32usize;
        let mut plane = vec![137u8; stride * 24];
        let refp = unsafe { plane.as_mut_ptr().add(stride * 4 + 4) };

        let mut fl: SWelsFuncPtrList = unsafe { core::mem::zeroed() };
        unsafe { WelsInitIntraPredFuncs(&mut fl, 0) };

        for mode in 0..14usize {
            let Some(f) = fl.pfGetLumaI4x4Pred[mode] else { continue };
            let mut pred = [0u8; 16];
            unsafe { f(pred.as_mut_ptr(), refp, stride as i32) };
            let expected = if mode == I4_PRED_DC_128 as usize { 0x80 } else { 137 };
            assert!(
                pred.iter().all(|&b| b == expected),
                "mode {mode} produced {pred:?}, expected all {expected}"
            );
        }
    }

    /// Same for chroma and I16x16.
    #[test]
    fn all_chroma_and_i16x16_modes_are_flat_on_a_flat_plane() {
        let stride = 48usize;
        let mut plane = vec![91u8; stride * 40];
        let refp = unsafe { plane.as_mut_ptr().add(stride * 17 + 17) };

        let mut fl: SWelsFuncPtrList = unsafe { core::mem::zeroed() };
        unsafe { WelsInitIntraPredFuncs(&mut fl, 0) };

        for mode in 0..7usize {
            if let Some(f) = fl.pfGetChromaPred[mode] {
                let mut pred = [0u8; 64];
                unsafe { f(pred.as_mut_ptr(), refp, stride as i32) };
                let expected = if mode == C_PRED_DC_128 as usize { 0x80 } else { 91 };
                assert!(pred.iter().all(|&b| b == expected), "chroma mode {mode}");
            }
            if let Some(f) = fl.pfGetLumaI16x16Pred[mode] {
                let mut pred = [0u8; 256];
                unsafe { f(pred.as_mut_ptr(), refp, stride as i32) };
                let expected = if mode == I16_PRED_DC_128 as usize { 0x80 } else { 91 };
                assert!(pred.iter().all(|&b| b == expected), "i16x16 mode {mode}");
            }
        }
    }

    /// Every table slot the mode-decision code can index must be filled — this is the
    /// regression test for the defect that motivated the module: the three tables were
    /// declared but never populated, so `WelsMdI16x16` unwrapped a `None`.
    #[test]
    fn init_fills_every_slot_the_md_layer_indexes() {
        let mut fl: SWelsFuncPtrList = unsafe { core::mem::zeroed() };
        unsafe { WelsInitIntraPredFuncs(&mut fl, 0) };

        for m in [I16_PRED_V, I16_PRED_H, I16_PRED_DC, I16_PRED_P, I16_PRED_DC_L, I16_PRED_DC_T, I16_PRED_DC_128] {
            assert!(fl.pfGetLumaI16x16Pred[m as usize].is_some(), "I16 mode {m}");
        }
        for m in [
            I4_PRED_V, I4_PRED_H, I4_PRED_DC, I4_PRED_DDL, I4_PRED_DDR, I4_PRED_VR, I4_PRED_HD,
            I4_PRED_VL, I4_PRED_HU, I4_PRED_DC_L, I4_PRED_DC_T, I4_PRED_DC_128, I4_PRED_DDL_TOP,
            I4_PRED_VL_TOP,
        ] {
            assert!(fl.pfGetLumaI4x4Pred[m as usize].is_some(), "I4 mode {m}");
        }
        for m in [C_PRED_DC, C_PRED_H, C_PRED_V, C_PRED_P, C_PRED_DC_L, C_PRED_DC_T, C_PRED_DC_128] {
            assert!(fl.pfGetChromaPred[m as usize].is_some(), "chroma mode {m}");
        }
    }
}
