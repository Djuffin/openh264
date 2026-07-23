#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

/*!
 * OpenH264 Decoder: Spatial Intra Prediction Module
 *
 * Implements 4x4 Luma, 8x8 Luma (High Profile), 8x8 Chroma, and 16x16 Luma
 * intra prediction algorithms according to ITU-T H.264 / ISO/IEC 14496-10.
 */

pub const I4x4_COUNT: usize = 4;
pub const I8x8_COUNT: usize = 8;
pub const I16x16_COUNT: usize = 16;

pub type PGetIntraPredFunc = unsafe extern "C" fn(pPred: *mut u8, kiLumaStride: i32);
pub type PGetIntraPred8x8Func =
    unsafe extern "C" fn(pPred: *mut u8, kiLumaStride: i32, bTLAvail: bool, bTRAvail: bool);

#[inline(always)]
pub fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        ((-iX) >> 31) as u8
    } else {
        iX as u8
    }
}

// ============================================================================
// Intra 4x4 Luma Prediction Functions
// ============================================================================

pub unsafe extern "C" fn WelsI4x4LumaPredV_c(pPred: *mut u8, kiStride: i32) {
    let kuiVal = (pPred.offset(-kiStride as isize) as *const u32).read_unaligned();

    (pPred as *mut u32).write_unaligned(kuiVal);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiVal);
    (pPred.offset((kiStride << 1) as isize) as *mut u32).write_unaligned(kuiVal);
    (pPred.offset(((kiStride << 1) + kiStride) as isize) as *mut u32).write_unaligned(kuiVal);
}

pub unsafe extern "C" fn WelsI4x4LumaPredH_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let kuiL0 = 0x01010101u32.wrapping_mul(*pPred.offset(-1) as u32);
    let kuiL1 = 0x01010101u32.wrapping_mul(*pPred.offset(-1 + kiStride as isize) as u32);
    let kuiL2 = 0x01010101u32.wrapping_mul(*pPred.offset(-1 + kiStride2 as isize) as u32);
    let kuiL3 = 0x01010101u32.wrapping_mul(*pPred.offset(-1 + kiStride3 as isize) as u32);

    (pPred as *mut u32).write_unaligned(kuiL0);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiL1);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiL2);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiL3);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDc_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let sum = *pPred.offset(-1) as u32
        + *pPred.offset(-1 + kiStride as isize) as u32
        + *pPred.offset(-1 + kiStride2 as isize) as u32
        + *pPred.offset(-1 + kiStride3 as isize) as u32
        + *pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(-kiStride as isize + 1) as u32
        + *pPred.offset(-kiStride as isize + 2) as u32
        + *pPred.offset(-kiStride as isize + 3) as u32
        + 4;
    let kuiMean = (sum >> 3) as u8;
    let kuiMean32 = 0x01010101u32.wrapping_mul(kuiMean as u32);

    (pPred as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiMean32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDcLeft_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let sum = *pPred.offset(-1) as u32
        + *pPred.offset(-1 + kiStride as isize) as u32
        + *pPred.offset(-1 + kiStride2 as isize) as u32
        + *pPred.offset(-1 + kiStride3 as isize) as u32
        + 2;
    let kuiMean = (sum >> 2) as u8;
    let kuiMean32 = 0x01010101u32.wrapping_mul(kuiMean as u32);

    (pPred as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiMean32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDcTop_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride2 + kiStride;
    let sum = *pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(-kiStride as isize + 1) as u32
        + *pPred.offset(-kiStride as isize + 2) as u32
        + *pPred.offset(-kiStride as isize + 3) as u32
        + 2;
    let kuiMean = (sum >> 2) as u8;
    let kuiMean32 = 0x01010101u32.wrapping_mul(kuiMean as u32);

    (pPred as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride2 as isize) as *mut u32).write_unaligned(kuiMean32);
    (pPred.offset(kiStride3 as isize) as *mut u32).write_unaligned(kuiMean32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDcNA_c(pPred: *mut u8, kiStride: i32) {
    let kuiDC32 = 0x80808080u32;

    (pPred as *mut u32).write_unaligned(kuiDC32);
    (pPred.offset(kiStride as isize) as *mut u32).write_unaligned(kuiDC32);
    (pPred.offset((kiStride << 1) as isize) as *mut u32).write_unaligned(kuiDC32);
    (pPred.offset(((kiStride << 1) + kiStride) as isize) as *mut u32).write_unaligned(kuiDC32);
}

pub unsafe extern "C" fn WelsI4x4LumaPredDDL_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptop = pPred.offset(-kiStride as isize);
    let kuiT0 = *ptop as u32;
    let kuiT1 = *ptop.offset(1) as u32;
    let kuiT2 = *ptop.offset(2) as u32;
    let kuiT3 = *ptop.offset(3) as u32;
    let kuiT4 = *ptop.offset(4) as u32;
    let kuiT5 = *ptop.offset(5) as u32;
    let kuiT6 = *ptop.offset(6) as u32;
    let kuiT7 = *ptop.offset(7) as u32;

    let kuiDDL0 = ((2 + kuiT0 + kuiT2 + (kuiT1 << 1)) >> 2) as u8;
    let kuiDDL1 = ((2 + kuiT1 + kuiT3 + (kuiT2 << 1)) >> 2) as u8;
    let kuiDDL2 = ((2 + kuiT2 + kuiT4 + (kuiT3 << 1)) >> 2) as u8;
    let kuiDDL3 = ((2 + kuiT3 + kuiT5 + (kuiT4 << 1)) >> 2) as u8;
    let kuiDDL4 = ((2 + kuiT4 + kuiT6 + (kuiT5 << 1)) >> 2) as u8;
    let kuiDDL5 = ((2 + kuiT5 + kuiT7 + (kuiT6 << 1)) >> 2) as u8;
    let kuiDDL6 = ((2 + kuiT6 + kuiT7 + (kuiT7 << 1)) >> 2) as u8;

    let kuiList: [u8; 8] = [
        kuiDDL0, kuiDDL1, kuiDDL2, kuiDDL3, kuiDDL4, kuiDDL5, kuiDDL6, 0,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(3) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredDDLTop_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptop = pPred.offset(-kiStride as isize);
    let kuiT0 = *ptop as u32;
    let kuiT1 = *ptop.offset(1) as u32;
    let kuiT2 = *ptop.offset(2) as u32;
    let kuiT3 = *ptop.offset(3) as u32;

    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiT33 = 1 + (kuiT3 << 1);

    let kuiDLT0 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiDLT1 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiDLT2 = ((kuiT23 + kuiT33) >> 2) as u8;
    let kuiDLT3 = (kuiT33 >> 1) as u8;

    let kuiList: [u8; 8] = [
        kuiDLT0, kuiDLT1, kuiDLT2, kuiDLT3, kuiDLT3, kuiDLT3, kuiDLT3, kuiDLT3,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(3) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredDDR_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptopleft = pPred.offset(-(kiStride + 1) as isize);
    let pleft = pPred.offset(-1);

    let kuiLT = *ptopleft as u32;
    let kuiL0 = *pleft as u32;
    let kuiL1 = *pleft.offset(kiStride as isize) as u32;
    let kuiL2 = *pleft.offset(kiStride2 as isize) as u32;
    let kuiL3 = *pleft.offset(kiStride3 as isize) as u32;

    let kuiT0 = *ptopleft.offset(1) as u32;
    let kuiT1 = *ptopleft.offset(2) as u32;
    let kuiT2 = *ptopleft.offset(3) as u32;
    let kuiT3 = *ptopleft.offset(4) as u32;

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

    let kuiList: [u8; 8] = [
        kuiDDR6, kuiDDR5, kuiDDR4, kuiDDR0, kuiDDR1, kuiDDR2, kuiDDR3, 0,
    ];

    (pPred as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(3) as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredVL_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptopleft = pPred.offset(-(kiStride + 1) as isize);

    let kuiT0 = *ptopleft.offset(1) as u32;
    let kuiT1 = *ptopleft.offset(2) as u32;
    let kuiT2 = *ptopleft.offset(3) as u32;
    let kuiT3 = *ptopleft.offset(4) as u32;
    let kuiT4 = *ptopleft.offset(5) as u32;
    let kuiT5 = *ptopleft.offset(6) as u32;
    let kuiT6 = *ptopleft.offset(7) as u32;

    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiT34 = 1 + kuiT3 + kuiT4;
    let kuiT45 = 1 + kuiT4 + kuiT5;
    let kuiT56 = 1 + kuiT5 + kuiT6;

    let kuiVL0 = (kuiT01 >> 1) as u8;
    let kuiVL1 = (kuiT12 >> 1) as u8;
    let kuiVL2 = (kuiT23 >> 1) as u8;
    let kuiVL3 = (kuiT34 >> 1) as u8;
    let kuiVL4 = (kuiT45 >> 1) as u8;
    let kuiVL5 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiVL6 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiVL7 = ((kuiT23 + kuiT34) >> 2) as u8;
    let kuiVL8 = ((kuiT34 + kuiT45) >> 2) as u8;
    let kuiVL9 = ((kuiT45 + kuiT56) >> 2) as u8;

    let kuiList: [u8; 10] = [
        kuiVL0, kuiVL1, kuiVL2, kuiVL3, kuiVL4, kuiVL5, kuiVL6, kuiVL7, kuiVL8, kuiVL9,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(5) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredVLTop_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;
    let ptopleft = pPred.offset(-(kiStride + 1) as isize);

    let kuiT0 = *ptopleft.offset(1) as u32;
    let kuiT1 = *ptopleft.offset(2) as u32;
    let kuiT2 = *ptopleft.offset(3) as u32;
    let kuiT3 = *ptopleft.offset(4) as u32;

    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiT23 = 1 + kuiT2 + kuiT3;
    let kuiT33 = 1 + (kuiT3 << 1);

    let kuiVL0 = (kuiT01 >> 1) as u8;
    let kuiVL1 = (kuiT12 >> 1) as u8;
    let kuiVL2 = (kuiT23 >> 1) as u8;
    let kuiVL3 = (kuiT33 >> 1) as u8;
    let kuiVL4 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiVL5 = ((kuiT12 + kuiT23) >> 2) as u8;
    let kuiVL6 = ((kuiT23 + kuiT33) >> 2) as u8;
    let kuiVL7 = kuiVL3;

    let kuiList: [u8; 10] = [
        kuiVL0, kuiVL1, kuiVL2, kuiVL3, kuiVL3, kuiVL4, kuiVL5, kuiVL6, kuiVL7, kuiVL7,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(5) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredVR_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;

    let kuiLT = *pPred.offset(-kiStride as isize - 1) as u32;
    let kuiL0 = *pPred.offset(-1) as u32;
    let kuiL1 = *pPred.offset(kiStride as isize - 1) as u32;
    let kuiL2 = *pPred.offset(kiStride2 as isize - 1) as u32;

    let kuiT0 = *pPred.offset(-kiStride as isize) as u32;
    let kuiT1 = *pPred.offset(1 - kiStride as isize) as u32;
    let kuiT2 = *pPred.offset(2 - kiStride as isize) as u32;
    let kuiT3 = *pPred.offset(3 - kiStride as isize) as u32;

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

    let kuiList: [u8; 10] = [
        kuiVR8, kuiVR0, kuiVR1, kuiVR2, kuiVR3, kuiVR9, kuiVR4, kuiVR5, kuiVR6, kuiVR7,
    ];

    (pPred as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(1) as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(5) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredHU_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;

    let kuiL0 = *pPred.offset(-1) as u32;
    let kuiL1 = *pPred.offset(kiStride as isize - 1) as u32;
    let kuiL2 = *pPred.offset(kiStride2 as isize - 1) as u32;
    let kuiL3 = *pPred.offset(kiStride3 as isize - 1) as u32;

    let kuiL01 = 1 + kuiL0 + kuiL1;
    let kuiL12 = 1 + kuiL1 + kuiL2;
    let kuiL23 = 1 + kuiL2 + kuiL3;

    let kuiHU0 = (kuiL01 >> 1) as u8;
    let kuiHU1 = ((kuiL01 + kuiL12) >> 2) as u8;
    let kuiHU2 = (kuiL12 >> 1) as u8;
    let kuiHU3 = ((kuiL12 + kuiL23) >> 2) as u8;
    let kuiHU4 = (kuiL23 >> 1) as u8;
    let kuiHU5 = ((1 + kuiL23 + (kuiL3 << 1)) >> 2) as u8;
    let kuiL3_u8 = kuiL3 as u8;

    let kuiList: [u8; 10] = [
        kuiHU0, kuiHU1, kuiHU2, kuiHU3, kuiHU4, kuiHU5, kuiL3_u8, kuiL3_u8, kuiL3_u8, kuiL3_u8,
    ];

    (pPred as *mut u32).write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(4) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
}

pub unsafe extern "C" fn WelsI4x4LumaPredHD_c(pPred: *mut u8, kiStride: i32) {
    let kiStride2 = kiStride << 1;
    let kiStride3 = kiStride + kiStride2;

    let kuiLT = *pPred.offset(-(kiStride + 1) as isize) as u32;
    let kuiL0 = *pPred.offset(-1) as u32;
    let kuiL1 = *pPred.offset(-1 + kiStride as isize) as u32;
    let kuiL2 = *pPred.offset(-1 + kiStride2 as isize) as u32;
    let kuiL3 = *pPred.offset(-1 + kiStride3 as isize) as u32;

    let kuiT0 = *pPred.offset(-kiStride as isize) as u32;
    let kuiT1 = *pPred.offset(-kiStride as isize + 1) as u32;
    let kuiT2 = *pPred.offset(-kiStride as isize + 2) as u32;

    let kuiTL0 = 1 + kuiLT + kuiL0;
    let kuiLT0 = 1 + kuiLT + kuiT0;
    let kuiT01 = 1 + kuiT0 + kuiT1;
    let kuiT12 = 1 + kuiT1 + kuiT2;
    let kuiL01 = 1 + kuiL0 + kuiL1;
    let kuiL12 = 1 + kuiL1 + kuiL2;
    let kuiL23 = 1 + kuiL2 + kuiL3;

    let kuiHD0 = (kuiTL0 >> 1) as u8;
    let kuiHD1 = ((kuiTL0 + kuiLT0) >> 2) as u8;
    let kuiHD2 = ((kuiLT0 + kuiT01) >> 2) as u8;
    let kuiHD3 = ((kuiT01 + kuiT12) >> 2) as u8;
    let kuiHD4 = (kuiL01 >> 1) as u8;
    let kuiHD5 = ((kuiTL0 + kuiL01) >> 2) as u8;
    let kuiHD6 = (kuiL12 >> 1) as u8;
    let kuiHD7 = ((kuiL01 + kuiL12) >> 2) as u8;
    let kuiHD8 = (kuiL23 >> 1) as u8;
    let kuiHD9 = ((kuiL12 + kuiL23) >> 2) as u8;

    let kuiList: [u8; 10] = [
        kuiHD8, kuiHD9, kuiHD6, kuiHD7, kuiHD4, kuiHD5, kuiHD0, kuiHD1, kuiHD2, kuiHD3,
    ];

    (pPred as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(6) as *const u32).read_unaligned());
    (pPred.offset(kiStride as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(4) as *const u32).read_unaligned());
    (pPred.offset(kiStride2 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr().offset(2) as *const u32).read_unaligned());
    (pPred.offset(kiStride3 as isize) as *mut u32)
        .write_unaligned((kuiList.as_ptr() as *const u32).read_unaligned());
}

// ============================================================================
// Intra 8x8 Luma Prediction Functions (High Profile)
// ============================================================================

pub unsafe extern "C" fn WelsI8x8LumaPredV_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    let mut uiTop: u64 = 0;
    for i in (0..8).rev() {
        uiTop = (uiTop << 8) | (uiPixelFilterT[i] as u64);
    }

    for i in 0..8 {
        (pPred.offset(kiStride as isize * i as isize) as *mut u64).write_unaligned(uiTop);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredH_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8 {
        let uiLeft = 0x0101010101010101u64.wrapping_mul(uiPixelFilterL[i] as u64);
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(uiLeft);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDc_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    let mut uiTotal: u32 = 0;
    for i in 0..8 {
        uiTotal += uiPixelFilterL[i] as u32;
        uiTotal += uiPixelFilterT[i] as u32;
    }

    let kuiMean = ((uiTotal + 8) >> 4) as u8;
    let kuiMean64 = 0x0101010101010101u64.wrapping_mul(kuiMean as u64);

    for i in 0..8 {
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(kuiMean64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDcLeft_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    let mut uiTotal: u32 = 0;
    for i in 0..8 {
        uiTotal += uiPixelFilterL[i] as u32;
    }

    let kuiMean = ((uiTotal + 4) >> 3) as u8;
    let kuiMean64 = 0x0101010101010101u64.wrapping_mul(kuiMean as u64);

    for i in 0..8 {
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(kuiMean64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDcTop_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 8];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    let mut uiTotal: u32 = 0;
    for i in 0..8 {
        uiTotal += uiPixelFilterT[i] as u32;
    }

    let kuiMean = ((uiTotal + 4) >> 3) as u8;
    let kuiMean64 = 0x0101010101010101u64.wrapping_mul(kuiMean as u64);

    for i in 0..8 {
        (pPred.offset(iStride[i] as isize) as *mut u64).write_unaligned(kuiMean64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDcNA_c(
    pPred: *mut u8,
    kiStride: i32,
    _bTLAvail: bool,
    _bTRAvail: bool,
) {
    let kuiDC64 = 0x8080808080808080u64;
    for i in 0..8 {
        (pPred.offset(kiStride as isize * i as isize) as *mut u64).write_unaligned(kuiDC64);
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDDL_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..15 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[15] = ((*pPred.offset(14 - kiStride as isize) as u32
        + *pPred.offset(15 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8 {
        for j in 0..8 {
            if i == 7 && j == 7 {
                *pPred.offset(j as isize + iStride[i] as isize) =
                    ((uiPixelFilterT[14] as u32 + 3 * uiPixelFilterT[15] as u32 + 2) >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[i + j] as u32
                    + ((uiPixelFilterT[i + j + 1] as u32) << 1)
                    + uiPixelFilterT[i + j + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDDLTop_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[7] = ((*pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    let fill_val = *pPred.offset(7 - kiStride as isize);
    for i in 8..16 {
        uiPixelFilterT[i] = fill_val;
    }

    for i in 0..8 {
        for j in 0..8 {
            if i == 7 && j == 7 {
                *pPred.offset(j as isize + iStride[i] as isize) =
                    ((uiPixelFilterT[14] as u32 + 3 * uiPixelFilterT[15] as u32 + 2) >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[i + j] as u32
                    + ((uiPixelFilterT[i + j + 1] as u32) << 1)
                    + uiPixelFilterT[i + j + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredDDR_c(
    pPred: *mut u8,
    kiStride: i32,
    _bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let uiPixelFilterTL = ((*pPred.offset(-1) as u32
        + ((*pPred.offset(-1 - kiStride as isize) as u32) << 1)
        + *pPred.offset(-kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-1) as u32) << 1)
        + *pPred.offset(-1 + iStride[1] as isize) as u32
        + 2)
        >> 2) as u8;
    uiPixelFilterT[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-kiStride as isize) as u32) << 1)
        + *pPred.offset(1 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    for i in 0..8usize {
        for j in 0..(i.saturating_sub(1)) {
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterL[i - j - 2] as u32
                + ((uiPixelFilterL[i - j - 1] as u32) << 1)
                + uiPixelFilterL[i - j] as u32
                + 2)
                >> 2) as u8;
        }

        if i >= 1 {
            let j = i - 1;
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterTL as u32
                + ((uiPixelFilterL[0] as u32) << 1)
                + uiPixelFilterL[1] as u32
                + 2)
                >> 2) as u8;
        }

        let j = i;
        *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[0] as u32
            + ((uiPixelFilterTL as u32) << 1)
            + uiPixelFilterL[0] as u32
            + 2)
            >> 2) as u8;

        if i < 7 {
            let j = i + 1;
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterTL as u32
                + ((uiPixelFilterT[0] as u32) << 1)
                + uiPixelFilterT[1] as u32
                + 2)
                >> 2) as u8;
        }

        for j in (i + 2)..8 {
            *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j - i - 2] as u32
                + ((uiPixelFilterT[j - i - 1] as u32) << 1)
                + uiPixelFilterT[j - i] as u32
                + 2)
                >> 2) as u8;
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredVL_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..15 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[15] = ((*pPred.offset(14 - kiStride as isize) as u32
        + *pPred.offset(15 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8 {
        if (i & 0x01) == 0 {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + uiPixelFilterT[j + (i >> 1) + 1] as u32
                    + 1)
                    >> 1) as u8;
            }
        } else {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + ((uiPixelFilterT[j + (i >> 1) + 1] as u32) << 1)
                    + uiPixelFilterT[j + (i >> 1) + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredVLTop_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterT = [0u8; 16];
    uiPixelFilterT[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-kiStride as isize) as u32) << 1)
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-kiStride as isize) as u32 * 3
            + *pPred.offset(1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterT[7] = ((*pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    let fill_val = *pPred.offset(7 - kiStride as isize);
    for i in 8..16 {
        uiPixelFilterT[i] = fill_val;
    }

    for i in 0..8 {
        if (i & 0x01) == 0 {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + uiPixelFilterT[j + (i >> 1) + 1] as u32
                    + 1)
                    >> 1) as u8;
            }
        } else {
            for j in 0..8 {
                *pPred.offset(j as isize + iStride[i] as isize) = ((uiPixelFilterT[j + (i >> 1)]
                    as u32
                    + ((uiPixelFilterT[j + (i >> 1) + 1] as u32) << 1)
                    + uiPixelFilterT[j + (i >> 1) + 2] as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredVR_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let uiPixelFilterTL = ((*pPred.offset(-1) as u32
        + ((*pPred.offset(-1 - kiStride as isize) as u32) << 1)
        + *pPred.offset(-kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-1) as u32) << 1)
        + *pPred.offset(-1 + iStride[1] as isize) as u32
        + 2)
        >> 2) as u8;
    uiPixelFilterT[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-kiStride as isize) as u32) << 1)
        + *pPred.offset(1 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    for i in 0..8i32 {
        for j in 0..8i32 {
            let izVR = (j << 1) - i;
            let izVRDiv = j - (i >> 1);
            if izVR >= 0 {
                if (izVR & 0x01) == 0 {
                    if izVRDiv > 0 {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterT[(izVRDiv - 1) as usize] as u32
                                + uiPixelFilterT[izVRDiv as usize] as u32
                                + 1)
                                >> 1) as u8;
                    } else {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterTL as u32 + uiPixelFilterT[0] as u32 + 1) >> 1) as u8;
                    }
                } else if izVRDiv > 1 {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterT[(izVRDiv - 2) as usize] as u32
                            + ((uiPixelFilterT[(izVRDiv - 1) as usize] as u32) << 1)
                            + uiPixelFilterT[izVRDiv as usize] as u32
                            + 2)
                            >> 2) as u8;
                } else {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterTL as u32
                            + ((uiPixelFilterT[0] as u32) << 1)
                            + uiPixelFilterT[1] as u32
                            + 2)
                            >> 2) as u8;
                }
            } else if izVR == -1 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterL[0]
                    as u32
                    + ((uiPixelFilterTL as u32) << 1)
                    + uiPixelFilterT[0] as u32
                    + 2)
                    >> 2) as u8;
            } else if izVR < -2 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) =
                    ((uiPixelFilterL[(-izVR - 1) as usize] as u32
                        + ((uiPixelFilterL[(-izVR - 2) as usize] as u32) << 1)
                        + uiPixelFilterL[(-izVR - 3) as usize] as u32
                        + 2)
                        >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterL[1]
                    as u32
                    + ((uiPixelFilterL[0] as u32) << 1)
                    + uiPixelFilterTL as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredHU_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    _bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let mut uiPixelFilterL = [0u8; 8];
    uiPixelFilterL[0] = if bTLAvail {
        ((*pPred.offset(-1 - kiStride as isize) as u32
            + ((*pPred.offset(-1) as u32) << 1)
            + *pPred.offset(-1 + iStride[1] as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(-1) as u32 * 3 + *pPred.offset(-1 + iStride[1] as isize) as u32 + 2)
            >> 2) as u8
    };

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
    }
    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    for i in 0..8i32 {
        for j in 0..8i32 {
            let izHU = j + (i << 1);
            if izHU < 13 {
                if (izHU & 0x01) == 0 {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterL[(izHU >> 1) as usize] as u32
                            + uiPixelFilterL[(1 + (izHU >> 1)) as usize] as u32
                            + 1)
                            >> 1) as u8;
                } else {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterL[(izHU >> 1) as usize] as u32
                            + ((uiPixelFilterL[(1 + (izHU >> 1)) as usize] as u32) << 1)
                            + uiPixelFilterL[(2 + (izHU >> 1)) as usize] as u32
                            + 2)
                            >> 2) as u8;
                }
            } else if izHU == 13 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) =
                    ((uiPixelFilterL[6] as u32 + 3 * uiPixelFilterL[7] as u32 + 2) >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = uiPixelFilterL[7];
            }
        }
    }
}

pub unsafe extern "C" fn WelsI8x8LumaPredHD_c(
    pPred: *mut u8,
    kiStride: i32,
    bTLAvail: bool,
    bTRAvail: bool,
) {
    let mut iStride = [0i32; 8];
    for i in 1..8 {
        iStride[i] = iStride[i - 1] + kiStride;
    }

    let uiPixelFilterTL = ((*pPred.offset(-1) as u32
        + ((*pPred.offset(-1 - kiStride as isize) as u32) << 1)
        + *pPred.offset(-kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let mut uiPixelFilterL = [0u8; 8];
    let mut uiPixelFilterT = [0u8; 8];

    uiPixelFilterL[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-1) as u32) << 1)
        + *pPred.offset(-1 + iStride[1] as isize) as u32
        + 2)
        >> 2) as u8;
    uiPixelFilterT[0] = ((*pPred.offset(-1 - kiStride as isize) as u32
        + ((*pPred.offset(-kiStride as isize) as u32) << 1)
        + *pPred.offset(1 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    for i in 1..7 {
        uiPixelFilterL[i] = ((*pPred.offset(-1 + iStride[i - 1] as isize) as u32
            + ((*pPred.offset(-1 + iStride[i] as isize) as u32) << 1)
            + *pPred.offset(-1 + iStride[i + 1] as isize) as u32
            + 2)
            >> 2) as u8;
        uiPixelFilterT[i] = ((*pPred.offset(i as isize - 1 - kiStride as isize) as u32
            + ((*pPred.offset(i as isize - kiStride as isize) as u32) << 1)
            + *pPred.offset(i as isize + 1 - kiStride as isize) as u32
            + 2)
            >> 2) as u8;
    }

    uiPixelFilterL[7] = ((*pPred.offset(-1 + iStride[6] as isize) as u32
        + *pPred.offset(-1 + iStride[7] as isize) as u32 * 3
        + 2)
        >> 2) as u8;

    uiPixelFilterT[7] = if bTRAvail {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + ((*pPred.offset(7 - kiStride as isize) as u32) << 1)
            + *pPred.offset(8 - kiStride as isize) as u32
            + 2)
            >> 2) as u8
    } else {
        ((*pPred.offset(6 - kiStride as isize) as u32
            + *pPred.offset(7 - kiStride as isize) as u32 * 3
            + 2)
            >> 2) as u8
    };

    for i in 0..8i32 {
        for j in 0..8i32 {
            let izHD = (i << 1) - j;
            let izHDDiv = i - (j >> 1);
            if izHD >= 0 {
                if (izHD & 0x01) == 0 {
                    if izHDDiv == 0 {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterTL as u32 + uiPixelFilterL[0] as u32 + 1) >> 1) as u8;
                    } else {
                        *pPred.offset(j as isize + iStride[i as usize] as isize) =
                            ((uiPixelFilterL[(izHDDiv - 1) as usize] as u32
                                + uiPixelFilterL[izHDDiv as usize] as u32
                                + 1)
                                >> 1) as u8;
                    }
                } else if izHDDiv == 1 {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterTL as u32
                            + ((uiPixelFilterL[0] as u32) << 1)
                            + uiPixelFilterL[1] as u32
                            + 2)
                            >> 2) as u8;
                } else {
                    *pPred.offset(j as isize + iStride[i as usize] as isize) =
                        ((uiPixelFilterL[(izHDDiv - 2) as usize] as u32
                            + ((uiPixelFilterL[(izHDDiv - 1) as usize] as u32) << 1)
                            + uiPixelFilterL[izHDDiv as usize] as u32
                            + 2)
                            >> 2) as u8;
                }
            } else if izHD == -1 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterL[0]
                    as u32
                    + ((uiPixelFilterTL as u32) << 1)
                    + uiPixelFilterT[0] as u32
                    + 2)
                    >> 2) as u8;
            } else if izHD < -2 {
                *pPred.offset(j as isize + iStride[i as usize] as isize) =
                    ((uiPixelFilterT[(-izHD - 1) as usize] as u32
                        + ((uiPixelFilterT[(-izHD - 2) as usize] as u32) << 1)
                        + uiPixelFilterT[(-izHD - 3) as usize] as u32
                        + 2)
                        >> 2) as u8;
            } else {
                *pPred.offset(j as isize + iStride[i as usize] as isize) = ((uiPixelFilterT[1]
                    as u32
                    + ((uiPixelFilterT[0] as u32) << 1)
                    + uiPixelFilterTL as u32
                    + 2)
                    >> 2) as u8;
            }
        }
    }
}

// ============================================================================
// Intra 8x8 Chroma Prediction Functions
// ============================================================================

pub unsafe extern "C" fn WelsIChromaPredV_c(pPred: *mut u8, kiStride: i32) {
    let kuiVal64 = (pPred.offset(-kiStride as isize) as *const u64).read_unaligned();
    let kiStride2 = kiStride << 1;
    let kiStride4 = kiStride2 << 1;

    (pPred as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(kiStride as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(kiStride2 as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset((kiStride2 + kiStride) as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(kiStride4 as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset((kiStride4 + kiStride) as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset((kiStride4 + kiStride2) as isize) as *mut u64).write_unaligned(kuiVal64);
    (pPred.offset(((kiStride << 3) - kiStride) as isize) as *mut u64).write_unaligned(kuiVal64);
}

pub unsafe extern "C" fn WelsIChromaPredH_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 3) - kiStride;
    for _ in 0..8 {
        let kuiVal8 = *pPred.offset(iTmp as isize - 1);
        let kuiVal64 = 0x0101010101010101u64.wrapping_mul(kuiVal8 as u64);
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiVal64);
        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsIChromaPredPlane_c(pPred: *mut u8, kiStride: i32) {
    let mut H: i32 = 0;
    let mut V: i32 = 0;
    let pTop = pPred.offset(-kiStride as isize);
    let pLeft = pPred.offset(-1);

    for i in 0..4i32 {
        H += (i + 1)
            * (*pTop.offset(4 + i as isize) as i32 - *pTop.offset(2 - i as isize) as i32);
        V += (i + 1)
            * (*pLeft.offset((4 + i as isize) * kiStride as isize) as i32
                - *pLeft.offset((2 - i as isize) * kiStride as isize) as i32);
    }

    let a = (*pLeft.offset(7 * kiStride as isize) as i32 + *pTop.offset(7) as i32) << 4;
    let b = (17 * H + 16) >> 5;
    let c = (17 * V + 16) >> 5;

    let mut row_ptr = pPred;
    for i in 0..8i32 {
        for j in 0..8i32 {
            let iTmp = (a + b * (j - 3) + c * (i - 3) + 16) >> 5;
            *row_ptr.offset(j as isize) = WelsClip1(iTmp);
        }
        row_ptr = row_ptr.offset(kiStride as isize);
    }
}

pub unsafe extern "C" fn WelsIChromaPredDc_c(pPred: *mut u8, kiStride: i32) {
    let kiL1 = kiStride - 1;
    let kiL2 = kiL1 + kiStride;
    let kiL3 = kiL2 + kiStride;
    let kiL4 = kiL3 + kiStride;
    let kiL5 = kiL4 + kiStride;
    let kiL6 = kiL5 + kiStride;
    let kiL7 = kiL6 + kiStride;

    let kuiM1 = ((*pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(1 - kiStride as isize) as u32
        + *pPred.offset(2 - kiStride as isize) as u32
        + *pPred.offset(3 - kiStride as isize) as u32
        + *pPred.offset(-1) as u32
        + *pPred.offset(kiL1 as isize) as u32
        + *pPred.offset(kiL2 as isize) as u32
        + *pPred.offset(kiL3 as isize) as u32
        + 4)
        >> 3) as u8;

    let kuiSum2 = *pPred.offset(4 - kiStride as isize) as u32
        + *pPred.offset(5 - kiStride as isize) as u32
        + *pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32;

    let kuiSum3 = *pPred.offset(kiL4 as isize) as u32
        + *pPred.offset(kiL5 as isize) as u32
        + *pPred.offset(kiL6 as isize) as u32
        + *pPred.offset(kiL7 as isize) as u32;

    let kuiM2 = ((kuiSum2 + 2) >> 2) as u8;
    let kuiM3 = ((kuiSum3 + 2) >> 2) as u8;
    let kuiM4 = ((kuiSum2 + kuiSum3 + 4) >> 3) as u8;

    let kuiMUP: [u8; 8] = [kuiM1, kuiM1, kuiM1, kuiM1, kuiM2, kuiM2, kuiM2, kuiM2];
    let kuiMDown: [u8; 8] = [kuiM3, kuiM3, kuiM3, kuiM3, kuiM4, kuiM4, kuiM4, kuiM4];

    let kuiUP64 = (kuiMUP.as_ptr() as *const u64).read_unaligned();
    let kuiDN64 = (kuiMDown.as_ptr() as *const u64).read_unaligned();

    (pPred as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL1 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL2 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL3 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL4 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL5 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL6 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL7 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
}

pub unsafe extern "C" fn WelsIChromaPredDcLeft_c(pPred: *mut u8, kiStride: i32) {
    let kiL1 = -1 + kiStride;
    let kiL2 = kiL1 + kiStride;
    let kiL3 = kiL2 + kiStride;
    let kiL4 = kiL3 + kiStride;
    let kiL5 = kiL4 + kiStride;
    let kiL6 = kiL5 + kiStride;
    let kiL7 = kiL6 + kiStride;

    let kuiMUP = ((*pPred.offset(-1) as u32
        + *pPred.offset(kiL1 as isize) as u32
        + *pPred.offset(kiL2 as isize) as u32
        + *pPred.offset(kiL3 as isize) as u32
        + 2)
        >> 2) as u8;

    let kuiMDown = ((*pPred.offset(kiL4 as isize) as u32
        + *pPred.offset(kiL5 as isize) as u32
        + *pPred.offset(kiL6 as isize) as u32
        + *pPred.offset(kiL7 as isize) as u32
        + 2)
        >> 2) as u8;

    let kuiUP64 = 0x0101010101010101u64.wrapping_mul(kuiMUP as u64);
    let kuiDN64 = 0x0101010101010101u64.wrapping_mul(kuiMDown as u64);

    (pPred as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL1 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL2 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL3 as isize + 1) as *mut u64).write_unaligned(kuiUP64);
    (pPred.offset(kiL4 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL5 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL6 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
    (pPred.offset(kiL7 as isize + 1) as *mut u64).write_unaligned(kuiDN64);
}

pub unsafe extern "C" fn WelsIChromaPredDcTop_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 3) - kiStride;
    let kuiM1 = ((*pPred.offset(-kiStride as isize) as u32
        + *pPred.offset(1 - kiStride as isize) as u32
        + *pPred.offset(2 - kiStride as isize) as u32
        + *pPred.offset(3 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;
    let kuiM2 = ((*pPred.offset(4 - kiStride as isize) as u32
        + *pPred.offset(5 - kiStride as isize) as u32
        + *pPred.offset(6 - kiStride as isize) as u32
        + *pPred.offset(7 - kiStride as isize) as u32
        + 2)
        >> 2) as u8;

    let kuiM: [u8; 8] = [kuiM1, kuiM1, kuiM1, kuiM1, kuiM2, kuiM2, kuiM2, kuiM2];
    let kuiM64 = (kuiM.as_ptr() as *const u64).read_unaligned();

    for _ in 0..8 {
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiM64);
        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsIChromaPredDcNA_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 3) - kiStride;
    let kuiDC64 = 0x8080808080808080u64;

    for _ in 0..8 {
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiDC64);
        iTmp -= kiStride;
    }
}

// ============================================================================
// Intra 16x16 Luma Prediction Functions
// ============================================================================

pub unsafe extern "C" fn WelsI16x16LumaPredV_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;
    let kuiTop1 = (pPred.offset(-kiStride as isize) as *const u64).read_unaligned();
    let kuiTop2 = (pPred.offset(-kiStride as isize + 8) as *const u64).read_unaligned();

    for _ in 0..16 {
        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiTop1);
        (pPred.offset(iTmp as isize + 8) as *mut u64).write_unaligned(kuiTop2);
        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredH_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;

    for _ in 0..16 {
        let kuiVal8 = *pPred.offset(iTmp as isize - 1);
        let kuiVal64 = 0x0101010101010101u64.wrapping_mul(kuiVal8 as u64);

        (pPred.offset(iTmp as isize) as *mut u64).write_unaligned(kuiVal64);
        (pPred.offset(iTmp as isize + 8) as *mut u64).write_unaligned(kuiVal64);

        iTmp -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredPlane_c(pPred: *mut u8, kiStride: i32) {
    let mut H: i32 = 0;
    let mut V: i32 = 0;
    let pTop = pPred.offset(-kiStride as isize);
    let pLeft = pPred.offset(-1);

    for i in 0..8i32 {
        H += (i + 1)
            * (*pTop.offset(8 + i as isize) as i32 - *pTop.offset(6 - i as isize) as i32);
        V += (i + 1)
            * (*pLeft.offset((8 + i as isize) * kiStride as isize) as i32
                - *pLeft.offset((6 - i as isize) * kiStride as isize) as i32);
    }

    let a = (*pLeft.offset(15 * kiStride as isize) as i32 + *pTop.offset(15) as i32) << 4;
    let b = (5 * H + 32) >> 6;
    let c = (5 * V + 32) >> 6;

    let mut row_ptr = pPred;
    for i in 0..16i32 {
        for j in 0..16i32 {
            let iTmp = (a + b * (j - 7) + c * (i - 7) + 16) >> 5;
            *row_ptr.offset(j as isize) = WelsClip1(iTmp);
        }
        row_ptr = row_ptr.offset(kiStride as isize);
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDc_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;
    let mut iSum: i32 = 0;

    for i in 0..16 {
        iSum += *pPred.offset(-1 + iTmp as isize) as i32
            + *pPred.offset(-kiStride as isize + (15 - i) as isize) as i32;
        iTmp -= kiStride;
    }

    let uiMean = ((16 + iSum) >> 5) as u8;
    let uiMean64 = 0x0101010101010101u64.wrapping_mul(uiMean as u64);

    let mut out_offset = (kiStride << 4) - kiStride;
    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(uiMean64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(uiMean64);
        out_offset -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDcTop_c(pPred: *mut u8, kiStride: i32) {
    let mut iSum: i32 = 0;
    for i in 0..16 {
        iSum += *pPred.offset(-kiStride as isize + i as isize) as i32;
    }

    let uiMean = ((8 + iSum) >> 4) as u8;
    let uiMean64 = 0x0101010101010101u64.wrapping_mul(uiMean as u64);

    let mut out_offset = (kiStride << 4) - kiStride;
    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(uiMean64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(uiMean64);
        out_offset -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDcLeft_c(pPred: *mut u8, kiStride: i32) {
    let mut iTmp = (kiStride << 4) - kiStride;
    let mut iSum: i32 = 0;

    for _ in 0..16 {
        iSum += *pPred.offset(-1 + iTmp as isize) as i32;
        iTmp -= kiStride;
    }

    let uiMean = ((8 + iSum) >> 4) as u8;
    let uiMean64 = 0x0101010101010101u64.wrapping_mul(uiMean as u64);

    let mut out_offset = (kiStride << 4) - kiStride;
    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(uiMean64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(uiMean64);
        out_offset -= kiStride;
    }
}

pub unsafe extern "C" fn WelsI16x16LumaPredDcNA_c(pPred: *mut u8, kiStride: i32) {
    let kuiDC64 = 0x8080808080808080u64;
    let mut out_offset = (kiStride << 4) - kiStride;

    for _ in 0..16 {
        (pPred.offset(out_offset as isize) as *mut u64).write_unaligned(kuiDC64);
        (pPred.offset(out_offset as isize + 8) as *mut u64).write_unaligned(kuiDC64);
        out_offset -= kiStride;
    }
}

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_clip1() {
        assert_eq!(WelsClip1(-10), 0);
        assert_eq!(WelsClip1(0), 0);
        assert_eq!(WelsClip1(128), 128);
        assert_eq!(WelsClip1(255), 255);
        assert_eq!(WelsClip1(300), 255);
    }

    #[test]
    fn test_i4x4_pred_v_h_dc() {
        let mut buf = [0u8; 64];
        let stride = 8;
        // pPred is at offset 16 (row 2, col 1)
        let pred_offset = 17;

        unsafe {
            let pPred = buf.as_mut_ptr().add(pred_offset);

            // Set top samples at pPred - stride
            *pPred.offset(-stride) = 10;
            *pPred.offset(-stride + 1) = 20;
            *pPred.offset(-stride + 2) = 30;
            *pPred.offset(-stride + 3) = 40;

            WelsI4x4LumaPredV_c(pPred, stride as i32);
            assert_eq!(*pPred.offset(0), 10);
            assert_eq!(*pPred.offset(1), 20);
            assert_eq!(*pPred.offset(stride), 10);
            assert_eq!(*pPred.offset(stride + 3), 40);

            // Set left samples
            *pPred.offset(-1) = 5;
            *pPred.offset(stride - 1) = 15;
            *pPred.offset(2 * stride - 1) = 25;
            *pPred.offset(3 * stride - 1) = 35;

            WelsI4x4LumaPredH_c(pPred, stride as i32);
            assert_eq!(*pPred.offset(0), 5);
            assert_eq!(*pPred.offset(3), 5);
            assert_eq!(*pPred.offset(stride), 15);
            assert_eq!(*pPred.offset(3 * stride + 3), 35);
        }
    }
}
