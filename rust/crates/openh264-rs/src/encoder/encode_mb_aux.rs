// Copyright (c) 2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in
//      the documentation and/or other materials provided with the
//      distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

//! # Macroblock Auxiliary Encoding Kernels (`encode_mb_aux.rs`)
//!
//! Translated from `codec/encoder/core/inc/encode_mb_aux.h` and `codec/encoder/core/src/encode_mb_aux.cpp`.
//!
//! Implements the computational forward core for H.264 / AVC macroblock encoding:
//! 1. Forward 4x4 Integer Discrete Cosine Transform (FDCT): `WelsDctT4_c`, `WelsDctFourT4_c`
//! 2. Forward Hadamard Transforms: `WelsHadamardT4Dc_c`, `WelsHadamardQuant2x2_c`, `WelsHadamardQuant2x2Skip_c`
//! 3. Dead-Zone Forward Quantization: `WelsQuant4x4_c`, `WelsQuant4x4Dc_c`, `WelsQuantFour4x4_c`, `WelsQuantFour4x4Max_c`
//! 4. Zigzag Coefficient Scanning: `WelsScan4x4DcAc_c`, `WelsScan4x4Ac_c`, `WelsScan4x4Dc`
//! 5. Non-Zero Count & Bit-Cost Estimation: `WelsGetNoneZeroCount_c`, `WelsCalculateSingleCtr4x4_c`
//! 6. Dynamic SIMD Dispatch Table Initialization: `WelsInitEncodingFuncs`

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

// ============================================================================
// CPU Feature Flag Bitmasks (matching cpu_core.h)
// ============================================================================

// ============================================================================
// Quantization Lookup Tables (16-byte aligned in C)
// ============================================================================

/// Inter macroblock dead-zone rounding offset table `g_kiQuantInterFF[58][8]`.
/// Intra offset is indexed with an offset of +6 rows (`g_kiQuantInterFF[QP + 6]`).
#[repr(align(16))]
pub struct AlignedQuantTable58(pub [[i16; 8]; 58]);

pub static G_KI_QUANT_INTER_FF: AlignedQuantTable58 = AlignedQuantTable58(g_kiQuantInterFF);

pub const g_kiQuantInterFF: [[i16; 8]; 58] = [
    /* 0*/ [0, 1, 0, 1, 1, 1, 1, 1],
    /* 1*/ [0, 1, 0, 1, 1, 1, 1, 1],
    /* 2*/ [1, 1, 1, 1, 1, 1, 1, 1],
    /* 3*/ [1, 1, 1, 1, 1, 1, 1, 1],
    /* 4*/ [1, 1, 1, 1, 1, 2, 1, 2],
    /* 5*/ [1, 1, 1, 1, 1, 2, 1, 2],
    /* 6*/ [1, 1, 1, 1, 1, 2, 1, 2],
    /* 7*/ [1, 1, 1, 1, 1, 2, 1, 2],
    /* 8*/ [1, 2, 1, 2, 2, 3, 2, 3],
    /* 9*/ [1, 2, 1, 2, 2, 3, 2, 3],
    /*10*/ [1, 2, 1, 2, 2, 3, 2, 3],
    /*11*/ [1, 2, 1, 2, 2, 4, 2, 4],
    /*12*/ [2, 3, 2, 3, 3, 4, 3, 4],
    /*13*/ [2, 3, 2, 3, 3, 5, 3, 5],
    /*14*/ [2, 3, 2, 3, 3, 5, 3, 5],
    /*15*/ [2, 4, 2, 4, 4, 6, 4, 6],
    /*16*/ [3, 4, 3, 4, 4, 7, 4, 7],
    /*17*/ [3, 5, 3, 5, 5, 8, 5, 8],
    /*18*/ [3, 5, 3, 5, 5, 8, 5, 8],
    /*19*/ [4, 6, 4, 6, 6, 9, 6, 9],
    /*20*/ [4, 7, 4, 7, 7, 10, 7, 10],
    /*21*/ [5, 8, 5, 8, 8, 12, 8, 12],
    /*22*/ [5, 8, 5, 8, 8, 13, 8, 13],
    /*23*/ [6, 10, 6, 10, 10, 15, 10, 15],
    /*24*/ [7, 11, 7, 11, 11, 17, 11, 17],
    /*25*/ [7, 12, 7, 12, 12, 19, 12, 19],
    /*26*/ [9, 13, 9, 13, 13, 21, 13, 21],
    /*27*/ [9, 15, 9, 15, 15, 24, 15, 24],
    /*28*/ [11, 17, 11, 17, 17, 26, 17, 26],
    /*29*/ [12, 19, 12, 19, 19, 30, 19, 30],
    /*30*/ [13, 22, 13, 22, 22, 33, 22, 33],
    /*31*/ [15, 23, 15, 23, 23, 38, 23, 38],
    /*32*/ [17, 27, 17, 27, 27, 42, 27, 42],
    /*33*/ [19, 30, 19, 30, 30, 48, 30, 48],
    /*34*/ [21, 33, 21, 33, 33, 52, 33, 52],
    /*35*/ [24, 38, 24, 38, 38, 60, 38, 60],
    /*36*/ [27, 43, 27, 43, 43, 67, 43, 67],
    /*37*/ [29, 47, 29, 47, 47, 75, 47, 75],
    /*38*/ [35, 53, 35, 53, 53, 83, 53, 83],
    /*39*/ [37, 60, 37, 60, 60, 96, 60, 96],
    /*40*/ [43, 67, 43, 67, 67, 104, 67, 104],
    /*41*/ [48, 77, 48, 77, 77, 121, 77, 121],
    /*42*/ [53, 87, 53, 87, 87, 133, 87, 133],
    /*43*/ [59, 93, 59, 93, 93, 150, 93, 150],
    /*44*/ [69, 107, 69, 107, 107, 167, 107, 167],
    /*45*/ [75, 120, 75, 120, 120, 192, 120, 192],
    /*46*/ [85, 133, 85, 133, 133, 208, 133, 208],
    /*47*/ [96, 153, 96, 153, 153, 242, 153, 242],
    /*48*/ [107, 173, 107, 173, 173, 267, 173, 267],
    /*49*/ [117, 187, 117, 187, 187, 300, 187, 300],
    /*50*/ [139, 213, 139, 213, 213, 333, 213, 333],
    /*51*/ [149, 240, 149, 240, 240, 383, 240, 383],
    /* from here below is only for intra (QP 46..51 + 6) */
    /*46+6*/ [171, 267, 171, 267, 267, 417, 267, 417],
    /*47+6*/ [192, 307, 192, 307, 307, 483, 307, 483],
    /*48+6*/ [213, 347, 213, 347, 347, 533, 347, 533],
    /*49+6*/ [235, 373, 235, 373, 373, 600, 373, 600],
    /*50+6*/ [277, 427, 277, 427, 427, 667, 427, 667],
    /*51+6*/ [299, 480, 299, 480, 480, 767, 480, 767],
];

/// Intra quantization offset row index shift (`#define g_iQuantIntraFF (g_kiQuantInterFF + 6)`).
pub const G_I_QUANT_INTRA_FF_OFFSET: usize = 6;

/// Forward Quantization Multiplication Factor table `g_kiQuantMF[52][8]`.
#[repr(align(16))]
pub struct AlignedQuantTable52(pub [[i16; 8]; 52]);

pub static G_KI_QUANT_MF: AlignedQuantTable52 = AlignedQuantTable52(g_kiQuantMF);

pub const g_kiQuantMF: [[i16; 8]; 52] = [
    /* 0*/ [26214, 16132, 26214, 16132, 16132, 10486, 16132, 10486],
    /* 1*/ [23832, 14980, 23832, 14980, 14980, 9320, 14980, 9320],
    /* 2*/ [20164, 13108, 20164, 13108, 13108, 8388, 13108, 8388],
    /* 3*/ [18724, 11650, 18724, 11650, 11650, 7294, 11650, 7294],
    /* 4*/ [16384, 10486, 16384, 10486, 10486, 6710, 10486, 6710],
    /* 5*/ [14564, 9118, 14564, 9118, 9118, 5786, 9118, 5786],
    /* 6*/ [13107, 8066, 13107, 8066, 8066, 5243, 8066, 5243],
    /* 7*/ [11916, 7490, 11916, 7490, 7490, 4660, 7490, 4660],
    /* 8*/ [10082, 6554, 10082, 6554, 6554, 4194, 6554, 4194],
    /* 9*/ [9362, 5825, 9362, 5825, 5825, 3647, 5825, 3647],
    /*10*/ [8192, 5243, 8192, 5243, 5243, 3355, 5243, 3355],
    /*11*/ [7282, 4559, 7282, 4559, 4559, 2893, 4559, 2893],
    /*12*/ [6554, 4033, 6554, 4033, 4033, 2622, 4033, 2622],
    /*13*/ [5958, 3745, 5958, 3745, 3745, 2330, 3745, 2330],
    /*14*/ [5041, 3277, 5041, 3277, 3277, 2097, 3277, 2097],
    /*15*/ [4681, 2913, 4681, 2913, 2913, 1824, 2913, 1824],
    /*16*/ [4096, 2622, 4096, 2622, 2622, 1678, 2622, 1678],
    /*17*/ [3641, 2280, 3641, 2280, 2280, 1447, 2280, 1447],
    /*18*/ [3277, 2017, 3277, 2017, 2017, 1311, 2017, 1311],
    /*19*/ [2979, 1873, 2979, 1873, 1873, 1165, 1873, 1165],
    /*20*/ [2521, 1639, 2521, 1639, 1639, 1049, 1639, 1049],
    /*21*/ [2341, 1456, 2341, 1456, 1456, 912, 1456, 912],
    /*22*/ [2048, 1311, 2048, 1311, 1311, 839, 1311, 839],
    /*23*/ [1821, 1140, 1821, 1140, 1140, 723, 1140, 723],
    /*24*/ [1638, 1008, 1638, 1008, 1008, 655, 1008, 655],
    /*25*/ [1490, 936, 1490, 936, 936, 583, 936, 583],
    /*26*/ [1260, 819, 1260, 819, 819, 524, 819, 524],
    /*27*/ [1170, 728, 1170, 728, 728, 456, 728, 456],
    /*28*/ [1024, 655, 1024, 655, 655, 419, 655, 419],
    /*29*/ [910, 570, 910, 570, 570, 362, 570, 362],
    /*30*/ [819, 504, 819, 504, 504, 328, 504, 328],
    /*31*/ [745, 468, 745, 468, 468, 291, 468, 291],
    /*32*/ [630, 410, 630, 410, 410, 262, 410, 262],
    /*33*/ [585, 364, 585, 364, 364, 228, 364, 228],
    /*34*/ [512, 328, 512, 328, 328, 210, 328, 210],
    /*35*/ [455, 285, 455, 285, 285, 181, 285, 181],
    /*36*/ [410, 252, 410, 252, 252, 164, 252, 164],
    /*37*/ [372, 234, 372, 234, 234, 146, 234, 146],
    /*38*/ [315, 205, 315, 205, 205, 131, 205, 131],
    /*39*/ [293, 182, 293, 182, 182, 114, 182, 114],
    /*40*/ [256, 164, 256, 164, 164, 105, 164, 105],
    /*41*/ [228, 142, 228, 142, 142, 90, 142, 90],
    /*42*/ [205, 126, 205, 126, 126, 82, 126, 82],
    /*43*/ [186, 117, 186, 117, 117, 73, 117, 73],
    /*44*/ [158, 102, 158, 102, 102, 66, 102, 66],
    /*45*/ [146, 91, 146, 91, 91, 57, 91, 57],
    /*46*/ [128, 82, 128, 82, 82, 52, 82, 52],
    /*47*/ [114, 71, 114, 71, 71, 45, 71, 45],
    /*48*/ [102, 63, 102, 63, 63, 41, 63, 41],
    /*49*/ [93, 59, 93, 59, 59, 36, 59, 36],
    /*50*/ [79, 51, 79, 51, 51, 33, 51, 33],
    /*51*/ [73, 46, 73, 46, 46, 28, 46, 28],
];

/// CAVLC JVT-O079 rate cost estimation run-length penalty table.
pub const KI_TRUN_TABLE: [i32; 16] = [3, 2, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

// ============================================================================
// Function Pointer Types
// ============================================================================

pub type PCopyFunc = unsafe extern "C" fn(pDst: *mut u8, iStrideD: i32, pSrc: *mut u8, iStrideS: i32);
pub type PDctFunc = unsafe extern "C" fn(pDct: *mut i16, pSample1: *mut u8, iStride1: i32, pSample2: *mut u8, iStride2: i32);
pub type PCalculateSingleCtrFunc = unsafe extern "C" fn(pDct: *mut i16) -> i32;
pub type PScanFunc = unsafe extern "C" fn(pLevel: *mut i16, pDct: *mut i16);
pub type PQuantizationFunc = unsafe extern "C" fn(pDct: *mut i16, pFF: *const i16, pMF: *const i16);
pub type PQuantizationMaxFunc = unsafe extern "C" fn(pDct: *mut i16, pFF: *const i16, pMF: *const i16, pMax: *mut i16);
pub type PQuantizationDcFunc = unsafe extern "C" fn(pDct: *mut i16, iFF: i16, iMF: i16);
pub type PQuantizationSkipFunc = unsafe extern "C" fn(pDct: *mut i16, iFF: i16, iMF: i16) -> i32;
pub type PQuantizationHadamardFunc = unsafe extern "C" fn(pRes: *mut i16, kiFF: i16, iMF: i16, pDct: *mut i16, pBlock: *mut i16) -> i32;
pub type PTransformHadamard4x4Func = unsafe extern "C" fn(pLumaDc: *mut i16, pDct: *mut i16);
pub type PGetNoneZeroCountFunc = unsafe extern "C" fn(pLevel: *mut i16) -> i32;

// ============================================================================
// Encoder Function Pointer Table (SWelsFuncPtrList)
// ============================================================================

// ============================================================================
// Forward Discrete Cosine Transform (FDCT)
// ============================================================================

/// Computes pixel residual differencing followed by the 2D 4x4 Forward Integer DCT.
///
/// # Safety
/// - `pDct` must point to a writable buffer of at least 16 `i16` elements.
/// - `pPixel1` and `pPixel2` must point to valid readable pixel buffers with corresponding strides.
#[inline]
pub unsafe extern "C" fn WelsDctT4_c(
    pDct: *mut i16,
    mut pPixel1: *mut u8,
    iStride1: i32,
    mut pPixel2: *mut u8,
    iStride2: i32,
) {
    unsafe {
        let mut pData = [0i32; 16];
        let mut s = [0i32; 4];

        for i in (0..16).step_by(4) {
            let kiI1 = 1 + i;
            let kiI2 = 2 + i;
            let kiI3 = 3 + i;

            pData[i] = (*pPixel1.add(0) as i32) - (*pPixel2.add(0) as i32);
            pData[kiI1] = (*pPixel1.add(1) as i32) - (*pPixel2.add(1) as i32);
            pData[kiI2] = (*pPixel1.add(2) as i32) - (*pPixel2.add(2) as i32);
            pData[kiI3] = (*pPixel1.add(3) as i32) - (*pPixel2.add(3) as i32);

            pPixel1 = pPixel1.offset(iStride1 as isize);
            pPixel2 = pPixel2.offset(iStride2 as isize);

            // Horizontal 1D transform
            s[0] = pData[i] + pData[kiI3];
            s[3] = pData[i] - pData[kiI3];
            s[1] = pData[kiI1] + pData[kiI2];
            s[2] = pData[kiI1] - pData[kiI2];

            *pDct.add(i) = (s[0] + s[1]) as i16;
            *pDct.add(kiI2) = (s[0] - s[1]) as i16;
            *pDct.add(kiI1) = ((s[3] << 1) + s[2]) as i16;
            *pDct.add(kiI3) = (s[3] - (s[2] << 1)) as i16;
        }

        // Vertical 1D transform
        for i in 0..4 {
            let kiI4 = 4 + i;
            let kiI8 = 8 + i;
            let kiI12 = 12 + i;

            let d0 = *pDct.add(i) as i32;
            let d4 = *pDct.add(kiI4) as i32;
            let d8 = *pDct.add(kiI8) as i32;
            let d12 = *pDct.add(kiI12) as i32;

            s[0] = d0 + d12;
            s[3] = d0 - d12;
            s[1] = d4 + d8;
            s[2] = d4 - d8;

            *pDct.add(i) = (s[0] + s[1]) as i16;
            *pDct.add(kiI8) = (s[0] - s[1]) as i16;
            *pDct.add(kiI4) = ((s[3] << 1) + s[2]) as i16;
            *pDct.add(kiI12) = (s[3] - (s[2] << 1)) as i16;
        }
    }
}

/// Performs 4x4 FDCT on four adjacent 4x4 blocks forming an 8x8 quadrant.
///
/// # Safety
/// - `pDct` must point to a writable buffer of at least 64 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsDctFourT4_c(
    pDct: *mut i16,
    pPixel1: *mut u8,
    iStride1: i32,
    pPixel2: *mut u8,
    iStride2: i32,
) {
    unsafe {
        let stride_1 = (iStride1 << 2) as isize;
        let stride_2 = (iStride2 << 2) as isize;

        WelsDctT4_c(pDct, pPixel1, iStride1, pPixel2, iStride2);
        WelsDctT4_c(pDct.add(16), pPixel1.add(4), iStride1, pPixel2.add(4), iStride2);
        WelsDctT4_c(pDct.add(32), pPixel1.offset(stride_1), iStride1, pPixel2.offset(stride_2), iStride2);
        WelsDctT4_c(
            pDct.add(48),
            pPixel1.offset(stride_1 + 4),
            iStride1,
            pPixel2.offset(stride_2 + 4),
            iStride2,
        );
    }
}

// ============================================================================
// Forward Quantization Functions
// ============================================================================

/// In-place dead-zone forward quantization on a single 4x4 block (16 coefficients).
///
/// # Safety
/// - `pDct` must point to 16 contiguous `i16` elements.
/// - `pFF` and `pMF` must point to at least 8 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsQuant4x4_c(pDct: *mut i16, pFF: *const i16, pMF: *const i16) {
    unsafe {
        for i in (0..16).step_by(4) {
            let j = i & 0x07;
            for k in 0..4 {
                let val = *pDct.add(i + k);
                let iSign = (val as i32) >> 31;
                let abs_val = (iSign ^ (val as i32)) - iSign;
                let ff = *pFF.add(j + k) as i32;
                let mf = *pMF.add(j + k) as i32;
                let q = ((ff + abs_val) * mf) >> 16;
                *pDct.add(i + k) = ((iSign ^ q) - iSign) as i16;
            }
        }
    }
}

/// In-place forward quantization of 16 Hadamard-transformed Luma DC coefficients.
///
/// # Safety
/// - `pDct` must point to 16 contiguous `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsQuant4x4Dc_c(pDct: *mut i16, iFF: i16, iMF: i16) {
    unsafe {
        let ff = iFF as i32;
        let mf = iMF as i32;
        for i in 0..16 {
            let val = *pDct.add(i);
            let iSign = (val as i32) >> 31;
            let abs_val = (iSign ^ (val as i32)) - iSign;
            let q = ((ff + abs_val) * mf) >> 16;
            *pDct.add(i) = ((iSign ^ q) - iSign) as i16;
        }
    }
}

/// In-place forward quantization across 4 blocks (64 coefficients).
///
/// # Safety
/// - `pDct` must point to 64 contiguous `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsQuantFour4x4_c(pDct: *mut i16, pFF: *const i16, pMF: *const i16) {
    unsafe {
        for i in (0..64).step_by(4) {
            let j = i & 0x07;
            for k in 0..4 {
                let val = *pDct.add(i + k);
                let iSign = (val as i32) >> 31;
                let abs_val = (iSign ^ (val as i32)) - iSign;
                let ff = *pFF.add(j + k) as i32;
                let mf = *pMF.add(j + k) as i32;
                let q = ((ff + abs_val) * mf) >> 16;
                *pDct.add(i + k) = ((iSign ^ q) - iSign) as i16;
            }
        }
    }
}

/// In-place forward quantization across 4 blocks while computing max absolute levels `pMax[0..3]`.
///
/// # Safety
/// - `pDct` must point to 64 contiguous `i16` elements.
/// - `pMax` must point to 4 writable `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsQuantFour4x4Max_c(
    mut pDct: *mut i16,
    pFF: *const i16,
    pMF: *const i16,
    pMax: *mut i16,
) {
    unsafe {
        for k in 0..4 {
            let mut iMaxAbs: i16 = 0;
            for i in 0..16 {
                let j = i & 0x07;
                let val = *pDct.add(i);
                let iSign = (val as i32) >> 31;
                let abs_val = (iSign ^ (val as i32)) - iSign;
                let ff = *pFF.add(j) as i32;
                let mf = *pMF.add(j) as i32;
                let q_mag = (((ff + abs_val) * mf) >> 16) as i16;
                if iMaxAbs < q_mag {
                    iMaxAbs = q_mag;
                }
                *pDct.add(i) = ((iSign ^ (q_mag as i32)) - iSign) as i16;
            }
            pDct = pDct.add(16);
            *pMax.add(k) = iMaxAbs;
        }
    }
}

// ============================================================================
// Forward Hadamard Transforms
// ============================================================================

/// Fast early-termination skip check for 2x2 Chroma DC Hadamard transform and quantization.
/// Returns 1 if any transformed Chroma DC coefficient exceeds the zero-quantization threshold.
///
/// # Safety
/// - `pRs` must point to a residual buffer with readable entries at offsets 0, 16, 32, 48.
#[inline]
pub unsafe extern "C" fn WelsHadamardQuant2x2Skip_c(pRs: *mut i16, iFF: i16, iMF: i16) -> i32 {
    unsafe {
        let iThreshold: i32 = if iMF != 0 {
            (((1i32 << 16) - 1) / (iMF as i32)) - (iFF as i32)
        } else {
            0
        };

        let r0 = *pRs as i32;
        let r32 = *pRs.add(32) as i32;
        let r16 = *pRs.add(16) as i32;
        let r48 = *pRs.add(48) as i32;

        let s0 = r0 + r32;
        let s1 = r0 - r32;
        let s2 = r16 + r48;
        let s3 = r16 - r48;

        let d0 = s0 + s2;
        let d1 = s0 - s2;
        let d2 = s1 + s3;
        let d3 = s1 - s3;

        let abs_d0 = (d0 ^ (d0 >> 31)) - (d0 >> 31);
        let abs_d1 = (d1 ^ (d1 >> 31)) - (d1 >> 31);
        let abs_d2 = (d2 ^ (d2 >> 31)) - (d2 >> 31);
        let abs_d3 = (d3 ^ (d3 >> 31)) - (d3 >> 31);

        if abs_d0 > iThreshold || abs_d1 > iThreshold || abs_d2 > iThreshold || abs_d3 > iThreshold {
            1
        } else {
            0
        }
    }
}

/// 2x2 Forward Hadamard transform and quantization for Chroma DC coefficients.
/// Returns the count of non-zero quantized DC coefficients (`iDcNzc`).
///
/// # Safety
/// - `pRs` has DC positions 0, 16, 32, 48 read and cleared to 0.
/// - `pDct` and `pBlock` must point to writable buffers of at least 4 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsHadamardQuant2x2_c(
    pRs: *mut i16,
    kiFF: i16,
    iMF: i16,
    pDct: *mut i16,
    pBlock: *mut i16,
) -> i32 {
    unsafe {
        let r0 = *pRs as i32;
        let r32 = *pRs.add(32) as i32;
        let r16 = *pRs.add(16) as i32;
        let r48 = *pRs.add(48) as i32;

        let s0 = r0 + r32;
        let s1 = r0 - r32;
        let s2 = r16 + r48;
        let s3 = r16 - r48;

        *pRs = 0;
        *pRs.add(16) = 0;
        *pRs.add(32) = 0;
        *pRs.add(48) = 0;

        let d = [
            (s0 + s2) as i16,
            (s0 - s2) as i16,
            (s1 + s3) as i16,
            (s1 - s3) as i16,
        ];

        let ff = kiFF as i32;
        let mf = iMF as i32;
        let mut iDcNzc = 0;

        for i in 0..4 {
            let val = d[i];
            let iSign = (val as i32) >> 31;
            let abs_val = (iSign ^ (val as i32)) - iSign;
            let q = ((ff + abs_val) * mf) >> 16;
            let res = ((iSign ^ q) - iSign) as i16;
            *pDct.add(i) = res;
            *pBlock.add(i) = res;
            if res != 0 {
                iDcNzc += 1;
            }
        }

        iDcNzc
    }
}

/// 4x4 Forward Hadamard Transform on the 16 Luma DC coefficients of an Intra 16x16 macroblock.
///
/// # Safety
/// - `pLumaDc` must point to a writable buffer of 16 `i16` elements.
/// - `pDct` must point to the 256-element macroblock coefficient buffer.
#[inline]
pub unsafe extern "C" fn WelsHadamardT4Dc_c(pLumaDc: *mut i16, pDct: *mut i16) {
    unsafe {
        let mut p = [0i32; 16];
        let mut s = [0i32; 4];

        for i in (0..16).step_by(4) {
            let iIdx = ((i & 0x08) << 4) + ((i & 0x04) << 3);
            let d0 = *pDct.add(iIdx) as i32;
            let d80 = *pDct.add(iIdx + 80) as i32;
            let d16 = *pDct.add(iIdx + 16) as i32;
            let d64 = *pDct.add(iIdx + 64) as i32;

            s[0] = d0 + d80;
            s[3] = d0 - d80;
            s[1] = d16 + d64;
            s[2] = d16 - d64;

            p[i] = s[0] + s[1];
            p[i + 2] = s[0] - s[1];
            p[i + 1] = s[3] + s[2];
            p[i + 3] = s[3] - s[2];
        }

        for i in 0..4 {
            s[0] = p[i] + p[i + 12];
            s[3] = p[i] - p[i + 12];
            s[1] = p[i + 4] + p[i + 8];
            s[2] = p[i + 4] - p[i + 8];

            *pLumaDc.add(i) = (((s[0] + s[1] + 1) >> 1).clamp(-32768, 32767)) as i16;
            *pLumaDc.add(i + 8) = (((s[0] - s[1] + 1) >> 1).clamp(-32768, 32767)) as i16;
            *pLumaDc.add(i + 4) = (((s[3] + s[2] + 1) >> 1).clamp(-32768, 32767)) as i16;
            *pLumaDc.add(i + 12) = (((s[3] - s[2] + 1) >> 1).clamp(-32768, 32767)) as i16;
        }
    }
}

// ============================================================================
// Zigzag Scanning Functions
// ============================================================================

/// Reorders all 16 transform coefficients from 2D raster order in `pDct` to 1D zigzag scan order in `pLevel`.
///
/// # Safety
/// - `pLevel` and `pDct` must point to at least 16 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsScan4x4DcAc_c(pLevel: *mut i16, pDct: *mut i16) {
    unsafe {
        *pLevel.add(0) = *pDct.add(0);
        *pLevel.add(1) = *pDct.add(1);
        *pLevel.add(2) = *pDct.add(4);
        *pLevel.add(3) = *pDct.add(8);
        *pLevel.add(4) = *pDct.add(5);
        *pLevel.add(5) = *pDct.add(2);
        *pLevel.add(6) = *pDct.add(3);
        *pLevel.add(7) = *pDct.add(6);
        *pLevel.add(8) = *pDct.add(9);
        *pLevel.add(9) = *pDct.add(12);
        *pLevel.add(10) = *pDct.add(13);
        *pLevel.add(11) = *pDct.add(10);
        *pLevel.add(12) = *pDct.add(7);
        *pLevel.add(13) = *pDct.add(11);
        *pLevel.add(14) = *pDct.add(14);
        *pLevel.add(15) = *pDct.add(15);
    }
}

/// Reorders 15 AC coefficients into `pLevel[0..14]` (omitting DC at `pDct[0]`) and sets `pLevel[15] = 0`.
///
/// # Safety
/// - `pLevel` and `pDct` must point to at least 16 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsScan4x4Ac_c(pLevel: *mut i16, pDct: *mut i16) {
    unsafe {
        *pLevel.add(0) = *pDct.add(1);
        *pLevel.add(1) = *pDct.add(4);
        *pLevel.add(2) = *pDct.add(8);
        *pLevel.add(3) = *pDct.add(5);
        *pLevel.add(4) = *pDct.add(2);
        *pLevel.add(5) = *pDct.add(3);
        *pLevel.add(6) = *pDct.add(6);
        *pLevel.add(7) = *pDct.add(9);
        *pLevel.add(8) = *pDct.add(12);
        *pLevel.add(9) = *pDct.add(13);
        *pLevel.add(10) = *pDct.add(10);
        *pLevel.add(11) = *pDct.add(7);
        *pLevel.add(12) = *pDct.add(11);
        *pLevel.add(13) = *pDct.add(14);
        *pLevel.add(14) = *pDct.add(15);
        *pLevel.add(15) = 0;
    }
}

/// Reorders 16 DC coefficients into 1D zigzag scan order (identical to `WelsScan4x4DcAc_c`).
///
/// # Safety
/// - `pLevel` and `pDct` must point to at least 16 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsScan4x4Dc(pLevel: *mut i16, pDct: *mut i16) {
    unsafe {
        WelsScan4x4DcAc_c(pLevel, pDct);
    }
}

// ============================================================================
// Non-Zero Count and CAVLC Bit Scoring
// ============================================================================

/// Fast rate-distortion CAVLC bit-cost approximation for a 4x4 block based on JVT-O079.
///
/// # Safety
/// - `pDct` must point to at least 16 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsCalculateSingleCtr4x4_c(pDct: *mut i16) -> i32 {
    unsafe {
        let mut iSingleCtr: i32 = 0;
        let mut iIdx: i32 = 15;

        while iIdx >= 0 && *pDct.offset(iIdx as isize) == 0 {
            iIdx -= 1;
        }

        while iIdx >= 0 {
            iIdx -= 1;
            let mut iRun = iIdx;
            while iIdx >= 0 && *pDct.offset(iIdx as isize) == 0 {
                iIdx -= 1;
            }
            iRun -= iIdx;
            if (iRun as usize) < KI_TRUN_TABLE.len() {
                iSingleCtr += KI_TRUN_TABLE[iRun as usize];
            }
        }

        iSingleCtr
    }
}

/// Counts the number of non-zero coefficients in a 16-element array `pLevel`.
///
/// # Safety
/// - `pLevel` must point to at least 16 `i16` elements.
#[inline]
pub unsafe extern "C" fn WelsGetNoneZeroCount_c(pLevel: *mut i16) -> i32 {
    unsafe {
        let mut iCnt: i32 = 0;
        for i in 0..16 {
            if *pLevel.add(i) == 0 {
                iCnt += 1;
            }
        }
        16 - iCnt
    }
}

// ============================================================================
// Pixel Block Copy Fallbacks (matching copy_mb.h)
// ============================================================================

#[inline]
pub unsafe extern "C" fn WelsCopy4x4_c(mut pDst: *mut u8, iStrideD: i32, mut pSrc: *mut u8, iStrideS: i32) {
    unsafe {
        for _ in 0..4 {
            core::ptr::copy_nonoverlapping(pSrc, pDst, 4);
            pDst = pDst.offset(iStrideD as isize);
            pSrc = pSrc.offset(iStrideS as isize);
        }
    }
}

#[inline]
pub unsafe extern "C" fn WelsCopy8x4_c(mut pDst: *mut u8, iStrideD: i32, mut pSrc: *mut u8, iStrideS: i32) {
    unsafe {
        for _ in 0..4 {
            core::ptr::copy_nonoverlapping(pSrc, pDst, 8);
            pDst = pDst.offset(iStrideD as isize);
            pSrc = pSrc.offset(iStrideS as isize);
        }
    }
}

#[inline]
pub unsafe extern "C" fn WelsCopy4x8_c(mut pDst: *mut u8, iStrideD: i32, mut pSrc: *mut u8, iStrideS: i32) {
    unsafe {
        for _ in 0..8 {
            core::ptr::copy_nonoverlapping(pSrc, pDst, 4);
            pDst = pDst.offset(iStrideD as isize);
            pSrc = pSrc.offset(iStrideS as isize);
        }
    }
}

#[inline]
pub unsafe extern "C" fn WelsCopy8x8_c(mut pDst: *mut u8, iStrideD: i32, mut pSrc: *mut u8, iStrideS: i32) {
    unsafe {
        for _ in 0..8 {
            core::ptr::copy_nonoverlapping(pSrc, pDst, 8);
            pDst = pDst.offset(iStrideD as isize);
            pSrc = pSrc.offset(iStrideS as isize);
        }
    }
}

#[inline]
pub unsafe extern "C" fn WelsCopy16x8_c(mut pDst: *mut u8, iStrideD: i32, mut pSrc: *mut u8, iStrideS: i32) {
    unsafe {
        for _ in 0..8 {
            core::ptr::copy_nonoverlapping(pSrc, pDst, 16);
            pDst = pDst.offset(iStrideD as isize);
            pSrc = pSrc.offset(iStrideS as isize);
        }
    }
}

#[inline]
pub unsafe extern "C" fn WelsCopy8x16_c(mut pDst: *mut u8, iStrideD: i32, mut pSrc: *mut u8, iStrideS: i32) {
    unsafe {
        for _ in 0..16 {
            core::ptr::copy_nonoverlapping(pSrc, pDst, 8);
            pDst = pDst.offset(iStrideD as isize);
            pSrc = pSrc.offset(iStrideS as isize);
        }
    }
}

#[inline]
pub unsafe extern "C" fn WelsCopy16x16_c(mut pDst: *mut u8, iStrideD: i32, mut pSrc: *mut u8, iStrideS: i32) {
    unsafe {
        for _ in 0..16 {
            core::ptr::copy_nonoverlapping(pSrc, pDst, 16);
            pDst = pDst.offset(iStrideD as isize);
            pSrc = pSrc.offset(iStrideS as isize);
        }
    }
}

// ARM NEON fallbacks

// AArch64 NEON fallbacks

// Loongson MMI fallbacks

// Loongson LSX / LASX fallbacks

// ============================================================================
// Function Dispatch Table Initialization
// ============================================================================

/// Initializes the encoder function pointer table dynamically based on CPU feature flags.
///
/// # Safety
/// - `pFuncList` must point to a valid, writable `SWelsFuncPtrList` instance.
pub unsafe extern "C" fn WelsInitEncodingFuncs(pFuncList: *mut SWelsFuncPtrList, uiCpuFlag: u32) {
    if pFuncList.is_null() {
        return;
    }

    unsafe {
        let f = &mut *pFuncList;

        // Baseline C fallback functions
        f.pfCopy8x8Aligned = Some(WelsCopy8x8_c);
        f.pfCopy16x16Aligned = Some(WelsCopy16x16_c);
        f.pfCopy16x16NotAligned = Some(WelsCopy16x16_c);
        f.pfCopy16x8NotAligned = Some(WelsCopy16x8_c);
        f.pfCopy8x16Aligned = Some(WelsCopy8x16_c);
        f.pfCopy4x4 = Some(WelsCopy4x4_c);
        f.pfCopy8x4 = Some(WelsCopy8x4_c);
        f.pfCopy4x8 = Some(WelsCopy4x8_c);

        f.pfQuantizationHadamard2x2 = Some(WelsHadamardQuant2x2_c);
        f.pfQuantizationHadamard2x2Skip = Some(WelsHadamardQuant2x2Skip_c);
        f.pfTransformHadamard4x4Dc = Some(WelsHadamardT4Dc_c);

        f.pfDctT4 = Some(WelsDctT4_c);
        f.pfDctFourT4 = Some(WelsDctFourT4_c);

        f.pfScan4x4 = Some(WelsScan4x4DcAc_c);
        f.pfScan4x4Ac = Some(WelsScan4x4Ac_c);
        f.pfCalculateSingleCtr4x4 = Some(WelsCalculateSingleCtr4x4_c);

        f.pfGetNoneZeroCount = Some(WelsGetNoneZeroCount_c);

        f.pfQuantization4x4 = Some(WelsQuant4x4_c);
        f.pfQuantizationDc4x4 = Some(WelsQuant4x4Dc_c);
        f.pfQuantizationFour4x4 = Some(WelsQuantFour4x4_c);
        f.pfQuantizationFour4x4Max = Some(WelsQuantFour4x4Max_c);

    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fdct_t4() {
        let mut p1 = [
            10u8, 20, 30, 40,
            15,   25, 35, 45,
            20,   30, 40, 50,
            25,   35, 45, 55,
        ];
        let mut p2 = [0u8; 16];
        let mut dct = [0i16; 16];

        unsafe {
            WelsDctT4_c(dct.as_mut_ptr(), p1.as_mut_ptr(), 4, p2.as_mut_ptr(), 4);
        }

        assert_ne!(dct[0], 0);
    }

    #[test]
    fn test_quant_4x4() {
        let mut dct = [100i16; 16];
        let qp = 26usize;
        let ff = &g_kiQuantInterFF[qp];
        let mf = &g_kiQuantMF[qp];

        unsafe {
            WelsQuant4x4_c(dct.as_mut_ptr(), ff.as_ptr(), mf.as_ptr());
        }

        for &val in &dct {
            assert!(val >= 0);
        }
    }

    #[test]
    fn test_hadamard_quant_2x2() {
        let mut res = [0i16; 64];
        res[0] = 30;
        res[16] = 20;
        res[32] = 10;
        res[48] = 5;

        let mut dct = [0i16; 4];
        let mut block = [0i16; 4];

        let qp = 26usize;
        let ff = g_kiQuantInterFF[qp][0];
        let mf = g_kiQuantMF[qp][0];

        let nnz = unsafe {
            WelsHadamardQuant2x2_c(res.as_mut_ptr(), ff, mf, dct.as_mut_ptr(), block.as_mut_ptr())
        };

        assert_eq!(res[0], 0);
        assert_eq!(res[16], 0);
        assert_eq!(res[32], 0);
        assert_eq!(res[48], 0);
        assert!(nnz >= 0 && nnz <= 4);
    }

    #[test]
    fn test_zigzag_scan() {
        let mut dct = [0i16; 16];
        for i in 0..16 {
            dct[i] = (i + 1) as i16;
        }

        let mut level = [0i16; 16];
        unsafe {
            WelsScan4x4DcAc_c(level.as_mut_ptr(), dct.as_mut_ptr());
        }

        assert_eq!(level[0], dct[0]);
        assert_eq!(level[1], dct[1]);
        assert_eq!(level[2], dct[4]);
    }

    #[test]
    fn test_nonzero_count() {
        let mut level = [0i16; 16];
        level[0] = 5;
        level[3] = -2;
        level[7] = 1;

        let count = unsafe { WelsGetNoneZeroCount_c(level.as_mut_ptr()) };
        assert_eq!(count, 3);
    }

    #[test]
    fn test_init_encoding_funcs() {
        let mut func_list = SWelsFuncPtrList::default();
        unsafe {
            WelsInitEncodingFuncs(&mut func_list, WELS_CPU_SSE2);
        }

        assert!(func_list.pfDctT4.is_some());
        assert!(func_list.pfQuantization4x4.is_some());
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_FMA, WELS_CPU_LASX, WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MMXEXT, WELS_CPU_MSA, WELS_CPU_NEON, WELS_CPU_SSE2, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3};
