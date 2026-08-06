// Copyright (c) 2009-2013, Cisco Systems
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

//! # Macroblock Encoding & Local Reconstruction Engine (`svc_encode_mb`)
//!
//! Translated from `codec/encoder/core/inc/svc_encode_mb.h` and
//! `codec/encoder/core/src/svc_encode_mb.cpp`.
//!
//! This module coordinates forward DCT transformation, Hadamard transformation,
//! dead-zone scalar quantization, coefficient zigzag scanning, JVT-O079 fast zero-residual
//! early termination, inverse quantization/IDCT, and local reconstruction loops for H.264 / AVC / SVC
//! macroblock modes (Intra 16x16, Intra 4x4, Inter P/B luma, Chroma UV, and P_SKIP).

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ffi::c_void;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::encoder_context::SPicData;
pub use crate::encoder::param_svc::SWelsPPS;
pub use crate::encoder::encoder_context::SStrideTables;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
pub use crate::encoder::md::SMbCache;
pub use crate::encoder::md::SMB;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
pub use crate::encoder::encoder_context::sWelsEncCtx;

// ============================================================================
// Constants, Tables, and Bitmasks
// ============================================================================

pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_INTRA16x16: u32 = 0x00000002;
pub const MB_TYPE_INTRA8x8: u32 = 0x00000004;
pub const MB_TYPE_INTRA_PCM: u32 = 0x00000200;
pub const MB_TYPE_INTRA: u32 =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;

#[inline(always)]
pub fn IS_INTRA(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTRA) != 0
}

/// 4x4 block scan index mapping into pNonZeroCount[24]
pub static g_kuiMbCountScan4Idx: [u8; 24] = [
    0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15, 16, 17, 20, 21, 18, 19, 22, 23,
];

/// Chroma QP mapping table according to H.264 standard Table 8-15
pub static g_kuiChromaQpTable: [u8; 52] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37, 37, 38, 38, 38, 39,
    39, 39, 39,
];

/// Dead-zone rounding factor table for inter/intra quantization
pub static g_kiQuantInterFF: [[i16; 8]; 58] = [
    /*  0 */ [0, 1, 0, 1, 1, 1, 1, 1],
    /*  1 */ [0, 1, 0, 1, 1, 1, 1, 1],
    /*  2 */ [1, 1, 1, 1, 1, 1, 1, 1],
    /*  3 */ [1, 1, 1, 1, 1, 1, 1, 1],
    /*  4 */ [1, 1, 1, 1, 1, 2, 1, 2],
    /*  5 */ [1, 1, 1, 1, 1, 2, 1, 2],
    /*  6 */ [1, 1, 1, 1, 1, 2, 1, 2],
    /*  7 */ [1, 1, 1, 1, 1, 2, 1, 2],
    /*  8 */ [1, 2, 1, 2, 2, 3, 2, 3],
    /*  9 */ [1, 2, 1, 2, 2, 3, 2, 3],
    /* 10 */ [1, 2, 1, 2, 2, 3, 2, 3],
    /* 11 */ [1, 2, 1, 2, 2, 4, 2, 4],
    /* 12 */ [2, 3, 2, 3, 3, 4, 3, 4],
    /* 13 */ [2, 3, 2, 3, 3, 5, 3, 5],
    /* 14 */ [2, 3, 2, 3, 3, 5, 3, 5],
    /* 15 */ [2, 4, 2, 4, 4, 6, 4, 6],
    /* 16 */ [3, 4, 3, 4, 4, 7, 4, 7],
    /* 17 */ [3, 5, 3, 5, 5, 8, 5, 8],
    /* 18 */ [3, 5, 3, 5, 5, 8, 5, 8],
    /* 19 */ [4, 6, 4, 6, 6, 9, 6, 9],
    /* 20 */ [4, 7, 4, 7, 7, 10, 7, 10],
    /* 21 */ [5, 8, 5, 8, 8, 12, 8, 12],
    /* 22 */ [5, 8, 5, 8, 8, 13, 8, 13],
    /* 23 */ [6, 10, 6, 10, 10, 15, 10, 15],
    /* 24 */ [7, 11, 7, 11, 11, 17, 11, 17],
    /* 25 */ [7, 12, 7, 12, 12, 19, 12, 19],
    /* 26 */ [9, 13, 9, 13, 13, 21, 13, 21],
    /* 27 */ [9, 15, 9, 15, 15, 24, 15, 24],
    /* 28 */ [11, 17, 11, 17, 17, 26, 17, 26],
    /* 29 */ [12, 19, 12, 19, 19, 30, 19, 30],
    /* 30 */ [13, 22, 13, 22, 22, 33, 22, 33],
    /* 31 */ [15, 23, 15, 23, 23, 38, 23, 38],
    /* 32 */ [17, 27, 17, 27, 27, 42, 27, 42],
    /* 33 */ [19, 30, 19, 30, 30, 48, 30, 48],
    /* 34 */ [21, 33, 21, 33, 33, 52, 33, 52],
    /* 35 */ [24, 38, 24, 38, 38, 60, 38, 60],
    /* 36 */ [27, 43, 27, 43, 43, 67, 43, 67],
    /* 37 */ [29, 47, 29, 47, 47, 75, 47, 75],
    /* 38 */ [35, 53, 35, 53, 53, 83, 53, 83],
    /* 39 */ [37, 60, 37, 60, 60, 96, 60, 96],
    /* 40 */ [43, 67, 43, 67, 67, 104, 67, 104],
    /* 41 */ [48, 77, 48, 77, 77, 121, 77, 121],
    /* 42 */ [53, 87, 53, 87, 87, 133, 87, 133],
    /* 43 */ [59, 93, 59, 93, 93, 150, 93, 150],
    /* 44 */ [69, 107, 69, 107, 107, 167, 107, 167],
    /* 45 */ [75, 120, 75, 120, 120, 192, 120, 192],
    /* 46 */ [85, 133, 85, 133, 133, 208, 133, 208],
    /* 47 */ [96, 153, 96, 153, 153, 242, 153, 242],
    /* 48 */ [107, 173, 107, 173, 173, 267, 173, 267],
    /* 49 */ [117, 187, 117, 187, 187, 300, 187, 300],
    /* 50 */ [139, 213, 139, 213, 213, 333, 213, 333],
    /* 51 */ [149, 240, 149, 240, 240, 383, 240, 383],
    /* Intra offset +6 */
    /* 52 */ [171, 267, 171, 267, 267, 417, 267, 417],
    /* 53 */ [192, 307, 192, 307, 307, 483, 307, 483],
    /* 54 */ [213, 347, 213, 347, 347, 533, 347, 533],
    /* 55 */ [235, 373, 235, 373, 373, 600, 373, 600],
    /* 56 */ [277, 427, 277, 427, 427, 667, 427, 667],
    /* 57 */ [299, 480, 299, 480, 480, 767, 480, 767],
];

/// Intra quantization rounding factor table (alias for `g_kiQuantInterFF + 6`)
#[inline(always)]
pub fn get_quant_intra_ff(qp: usize) -> &'static [i16; 8] {
    &g_kiQuantInterFF[qp + 6]
}

/// Forward quantization multiplication factors table
pub static g_kiQuantMF: [[i16; 8]; 52] = [
    /*  0 */ [26214, 16132, 26214, 16132, 16132, 10486, 16132, 10486],
    /*  1 */ [23832, 14980, 23832, 14980, 14980, 9320, 14980, 9320],
    /*  2 */ [20164, 13108, 20164, 13108, 13108, 8388, 13108, 8388],
    /*  3 */ [18724, 11650, 18724, 11650, 11650, 7294, 11650, 7294],
    /*  4 */ [16384, 10486, 16384, 10486, 10486, 6710, 10486, 6710],
    /*  5 */ [14564, 9118, 14564, 9118, 9118, 5786, 9118, 5786],
    /*  6 */ [13107, 8066, 13107, 8066, 8066, 5243, 8066, 5243],
    /*  7 */ [11916, 7490, 11916, 7490, 7490, 4660, 7490, 4660],
    /*  8 */ [10082, 6554, 10082, 6554, 6554, 4194, 6554, 4194],
    /*  9 */ [9362, 5825, 9362, 5825, 5825, 3647, 5825, 3647],
    /* 10 */ [8192, 5243, 8192, 5243, 5243, 3355, 5243, 3355],
    /* 11 */ [7282, 4559, 7282, 4559, 4559, 2893, 4559, 2893],
    /* 12 */ [6554, 4033, 6554, 4033, 4033, 2622, 4033, 2622],
    /* 13 */ [5958, 3745, 5958, 3745, 3745, 2330, 3745, 2330],
    /* 14 */ [5041, 3277, 5041, 3277, 3277, 2097, 3277, 2097],
    /* 15 */ [4681, 2913, 4681, 2913, 2913, 1824, 2913, 1824],
    /* 16 */ [4096, 2622, 4096, 2622, 2622, 1678, 2622, 1678],
    /* 17 */ [3641, 2280, 3641, 2280, 2280, 1447, 2280, 1447],
    /* 18 */ [3277, 2017, 3277, 2017, 2017, 1311, 2017, 1311],
    /* 19 */ [2979, 1873, 2979, 1873, 1873, 1165, 1873, 1165],
    /* 20 */ [2521, 1639, 2521, 1639, 1639, 1049, 1639, 1049],
    /* 21 */ [2341, 1456, 2341, 1456, 1456, 912, 1456, 912],
    /* 22 */ [2048, 1311, 2048, 1311, 1311, 839, 1311, 839],
    /* 23 */ [1821, 1140, 1821, 1140, 1140, 723, 1140, 723],
    /* 24 */ [1638, 1008, 1638, 1008, 1008, 655, 1008, 655],
    /* 25 */ [1490, 936, 1490, 936, 936, 583, 936, 583],
    /* 26 */ [1260, 819, 1260, 819, 819, 524, 819, 524],
    /* 27 */ [1170, 728, 1170, 728, 728, 456, 728, 456],
    /* 28 */ [1024, 655, 1024, 655, 655, 419, 655, 419],
    /* 29 */ [910, 570, 910, 570, 570, 362, 570, 362],
    /* 30 */ [819, 504, 819, 504, 504, 328, 504, 328],
    /* 31 */ [745, 468, 745, 468, 468, 291, 468, 291],
    /* 32 */ [630, 410, 630, 410, 410, 262, 410, 262],
    /* 33 */ [585, 364, 585, 364, 364, 228, 364, 228],
    /* 34 */ [512, 328, 512, 328, 328, 210, 328, 210],
    /* 35 */ [455, 285, 455, 285, 285, 181, 285, 181],
    /* 36 */ [410, 252, 410, 252, 252, 164, 252, 164],
    /* 37 */ [372, 234, 372, 234, 234, 146, 234, 146],
    /* 38 */ [315, 205, 315, 205, 205, 131, 205, 131],
    /* 39 */ [293, 182, 293, 182, 182, 114, 182, 114],
    /* 40 */ [256, 164, 256, 164, 164, 105, 164, 105],
    /* 41 */ [228, 142, 228, 142, 142, 90, 142, 90],
    /* 42 */ [205, 126, 205, 126, 126, 82, 126, 82],
    /* 43 */ [186, 117, 186, 117, 117, 73, 117, 73],
    /* 44 */ [158, 102, 158, 102, 102, 66, 102, 66],
    /* 45 */ [146, 91, 146, 91, 91, 57, 91, 57],
    /* 46 */ [128, 82, 128, 82, 82, 52, 82, 52],
    /* 47 */ [114, 71, 114, 71, 71, 45, 71, 45],
    /* 48 */ [102, 63, 102, 63, 63, 41, 63, 41],
    /* 49 */ [93, 59, 93, 59, 59, 36, 59, 36],
    /* 50 */ [79, 51, 79, 51, 51, 33, 51, 33],
    /* 51 */ [73, 46, 73, 46, 46, 28, 46, 28],
];

/// Dequantization scaling multipliers table
pub static g_kuiDequantCoeff: [[u16; 8]; 52] = [
    /*  0 */ [10, 13, 10, 13, 13, 16, 13, 16],
    /*  1 */ [11, 14, 11, 14, 14, 18, 14, 18],
    /*  2 */ [13, 16, 13, 16, 16, 20, 16, 20],
    /*  3 */ [14, 18, 14, 18, 18, 23, 18, 23],
    /*  4 */ [16, 20, 16, 20, 20, 25, 20, 25],
    /*  5 */ [18, 23, 18, 23, 23, 29, 23, 29],
    /*  6 */ [20, 26, 20, 26, 26, 32, 26, 32],
    /*  7 */ [22, 28, 22, 28, 28, 36, 28, 36],
    /*  8 */ [26, 32, 26, 32, 32, 40, 32, 40],
    /*  9 */ [28, 36, 28, 36, 36, 46, 36, 46],
    /* 10 */ [32, 40, 32, 40, 40, 50, 40, 50],
    /* 11 */ [36, 46, 36, 46, 46, 58, 46, 58],
    /* 12 */ [40, 52, 40, 52, 52, 64, 52, 64],
    /* 13 */ [44, 56, 44, 56, 56, 72, 56, 72],
    /* 14 */ [52, 64, 52, 64, 64, 80, 64, 80],
    /* 15 */ [56, 72, 56, 72, 72, 92, 72, 92],
    /* 16 */ [64, 80, 64, 80, 80, 100, 80, 100],
    /* 17 */ [72, 92, 72, 92, 92, 116, 92, 116],
    /* 18 */ [80, 104, 80, 104, 104, 128, 104, 128],
    /* 19 */ [88, 112, 88, 112, 112, 144, 112, 144],
    /* 20 */ [104, 128, 104, 128, 128, 160, 128, 160],
    /* 21 */ [112, 144, 112, 144, 144, 184, 144, 184],
    /* 22 */ [128, 160, 128, 160, 160, 200, 160, 200],
    /* 23 */ [144, 184, 144, 184, 184, 232, 184, 232],
    /* 24 */ [160, 208, 160, 208, 208, 256, 208, 256],
    /* 25 */ [176, 224, 176, 224, 224, 288, 224, 288],
    /* 26 */ [208, 256, 208, 256, 256, 320, 256, 320],
    /* 27 */ [224, 288, 224, 288, 288, 368, 288, 368],
    /* 28 */ [256, 320, 256, 320, 320, 400, 320, 400],
    /* 29 */ [288, 368, 288, 368, 368, 464, 368, 464],
    /* 30 */ [320, 416, 320, 416, 416, 512, 416, 512],
    /* 31 */ [352, 448, 352, 448, 448, 576, 448, 576],
    /* 32 */ [416, 512, 416, 512, 512, 640, 512, 640],
    /* 33 */ [448, 576, 448, 576, 576, 736, 576, 736],
    /* 34 */ [512, 640, 512, 640, 640, 800, 640, 800],
    /* 35 */ [576, 736, 576, 736, 736, 928, 736, 928],
    /* 36 */ [640, 832, 640, 832, 832, 1024, 832, 1024],
    /* 37 */ [704, 896, 704, 896, 896, 1152, 896, 1152],
    /* 38 */ [832, 1024, 832, 1024, 1024, 1280, 1024, 1280],
    /* 39 */ [896, 1152, 896, 1152, 1152, 1472, 1152, 1472],
    /* 40 */ [1024, 1280, 1024, 1280, 1280, 1600, 1280, 1600],
    /* 41 */ [1152, 1472, 1152, 1472, 1472, 1856, 1472, 1856],
    /* 42 */ [1280, 1664, 1280, 1664, 1664, 2048, 1664, 2048],
    /* 43 */ [1408, 1792, 1408, 1792, 1792, 2304, 1792, 2304],
    /* 44 */ [1664, 2048, 1664, 2048, 2048, 2560, 2048, 2560],
    /* 45 */ [1792, 2304, 1792, 2304, 2304, 2944, 2304, 2944],
    /* 46 */ [2048, 2560, 2048, 2560, 2560, 3200, 2560, 3200],
    /* 47 */ [2304, 2944, 2304, 2944, 2944, 3712, 2944, 3712],
    /* 48 */ [2560, 3328, 2560, 3328, 3328, 4096, 3328, 4096],
    /* 49 */ [2816, 3584, 2816, 3584, 3584, 4608, 3584, 4608],
    /* 50 */ [3328, 4096, 3328, 4096, 4096, 5120, 4096, 5120],
    /* 51 */ [3584, 4608, 3584, 4608, 4608, 5888, 4608, 5888],
];

// ============================================================================
// Core Struct Definitions
// ============================================================================

pub const MAX_DEPENDENCY_LAYER: usize = 4;







// Function pointer signatures for SWelsFuncPtrList
pub type PDctFunc = unsafe extern "C" fn(*mut i16, *mut u8, i32, *mut u8, i32);
pub type PTransformHadamard4x4Func = unsafe extern "C" fn(*mut i16, *mut i16);
pub type PQuantizationFunc = unsafe extern "C" fn(*mut i16, *const i16, *const i16);
pub type PQuantizationDcFunc = unsafe extern "C" fn(*mut i16, i16, i16);
pub type PQuantizationFour4x4Func = unsafe extern "C" fn(*mut i16, *const i16, *const i16);
pub type PQuantizationMaxFunc = unsafe extern "C" fn(*mut i16, *const i16, *const i16, *mut i16);
pub type PQuantizationHadamardFunc =
    unsafe extern "C" fn(*mut i16, i16, i16, *mut i16, *mut i16) -> i32;
pub type PQuantizationHadamardSkipFunc = unsafe extern "C" fn(*mut i16, i16, i16) -> i32;
pub type PScanFunc = unsafe extern "C" fn(*mut i16, *mut i16);
pub type PCalculateSingleCtrFunc = unsafe extern "C" fn(*mut i16) -> i32;
pub type PGetNoneZeroCountFunc = unsafe extern "C" fn(*mut i16) -> i32;
pub type PDeQuantizationFunc = unsafe extern "C" fn(*mut i16, *const u16);
pub type PDeQuantizationIHadamard4x4Func = unsafe extern "C" fn(*mut i16, u16);
pub type PIDctFunc = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32, *mut i16);
pub type PIDctI16x16DcFunc = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32, *mut i16);
pub type PCopyAlignedFunc = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);
pub type PSetMemoryZero = unsafe extern "C" fn(*mut c_void, i32);

#[inline(always)]
fn WelsClip1(val: i32) -> u8 {
    if val < 0 {
        0
    } else if val > 255 {
        255
    } else {
        val as u8
    }
}

pub unsafe extern "C" fn WelsIDctT4Rec_c(
    pRec: *mut u8,
    iStride: i32,
    pPred: *mut u8,
    iPredStride: i32,
    pDct: *mut i16,
) {
    if pRec.is_null() || pPred.is_null() || pDct.is_null() {
        return;
    }
    let mut iTemp = [0i16; 16];

    let iDstStridex2 = iStride << 1;
    let iDstStridex3 = iStride + iDstStridex2;
    let iPredStridex2 = iPredStride << 1;
    let iPredStridex3 = iPredStride + iPredStridex2;

    for i in 0..4 {
        let iIdx = i << 2;
        let kiHorSumU = *pDct.add(iIdx) as i32 + *pDct.add(iIdx + 2) as i32;
        let kiHorDelU = *pDct.add(iIdx) as i32 - *pDct.add(iIdx + 2) as i32;
        let kiHorSumD = *pDct.add(iIdx + 1) as i32 + (*pDct.add(iIdx + 3) as i32 >> 1);
        let kiHorDelD = (*pDct.add(iIdx + 1) as i32 >> 1) - *pDct.add(iIdx + 3) as i32;

        iTemp[iIdx] = (kiHorSumU + kiHorSumD) as i16;
        iTemp[iIdx + 1] = (kiHorDelU + kiHorDelD) as i16;
        iTemp[iIdx + 2] = (kiHorDelU - kiHorDelD) as i16;
        iTemp[iIdx + 3] = (kiHorSumU - kiHorSumD) as i16;
    }

    for i in 0..4 {
        let kiVerSumL = iTemp[i] as i32 + iTemp[8 + i] as i32;
        let kiVerDelL = iTemp[i] as i32 - iTemp[8 + i] as i32;
        let kiVerDelR = (iTemp[4 + i] as i32 >> 1) - iTemp[12 + i] as i32;
        let kiVerSumR = iTemp[4 + i] as i32 + (iTemp[12 + i] as i32 >> 1);

        *pRec.add(i) = WelsClip1(*pPred.add(i) as i32 + ((kiVerSumL + kiVerSumR + 32) >> 6));
        *pRec.add(iStride as usize + i) = WelsClip1(*pPred.add(iPredStride as usize + i) as i32 + ((kiVerDelL + kiVerDelR + 32) >> 6));
        *pRec.add(iDstStridex2 as usize + i) = WelsClip1(*pPred.add(iPredStridex2 as usize + i) as i32 + ((kiVerDelL - kiVerDelR + 32) >> 6));
        *pRec.add(iDstStridex3 as usize + i) = WelsClip1(*pPred.add(iPredStridex3 as usize + i) as i32 + ((kiVerSumL - kiVerSumR + 32) >> 6));
    }
}

pub unsafe extern "C" fn WelsIDctFourT4_c(
    pRec: *mut u8,
    iStride: i32,
    pPred: *mut u8,
    iPredStride: i32,
    pDct: *mut i16,
) {
    if pRec.is_null() || pPred.is_null() || pDct.is_null() {
        return;
    }
    let iDstStridex4 = (iStride << 2) as usize;
    let iPredStridex4 = (iPredStride << 2) as usize;
    WelsIDctT4Rec_c(pRec, iStride, pPred, iPredStride, pDct);
    WelsIDctT4Rec_c(pRec.add(4), iStride, pPred.add(4), iPredStride, pDct.add(16));
    WelsIDctT4Rec_c(pRec.add(iDstStridex4), iStride, pPred.add(iPredStridex4), iPredStride, pDct.add(32));
    WelsIDctT4Rec_c(pRec.add(iDstStridex4 + 4), iStride, pPred.add(iPredStridex4 + 4), iPredStride, pDct.add(48));
}



// ============================================================================
// Math & Transform Helpers (C reference fallbacks)
// ============================================================================

/// 4x4 Inverse Hadamard transform for Intra 16x16 Luma DC
///
/// # Safety
/// - `pRes` must point to a 16-element `int16_t` buffer.
#[inline]
pub unsafe fn WelsIHadamard4x4Dc(pRes: *mut i16) {
    let mut iTemp = [0i16; 4];

    for i in (0..4).rev() {
        let kiIdx = i << 2;
        let kiIdx1 = 1 + kiIdx;
        let kiIdx2 = 1 + kiIdx1;
        let kiIdx3 = 1 + kiIdx2;

        iTemp[0] = *pRes.add(kiIdx) + *pRes.add(kiIdx2);
        iTemp[1] = *pRes.add(kiIdx) - *pRes.add(kiIdx2);
        iTemp[2] = *pRes.add(kiIdx1) - *pRes.add(kiIdx3);
        iTemp[3] = *pRes.add(kiIdx1) + *pRes.add(kiIdx3);

        *pRes.add(kiIdx) = iTemp[0] + iTemp[3];
        *pRes.add(kiIdx1) = iTemp[1] + iTemp[2];
        *pRes.add(kiIdx2) = iTemp[1] - iTemp[2];
        *pRes.add(kiIdx3) = iTemp[0] - iTemp[3];
    }

    for i in (0..4).rev() {
        let kiI4 = 4 + i;
        let kiI8 = 4 + kiI4;
        let kiI12 = 4 + kiI8;

        iTemp[0] = *pRes.add(i) + *pRes.add(kiI8);
        iTemp[1] = *pRes.add(i) - *pRes.add(kiI8);
        iTemp[2] = *pRes.add(kiI4) - *pRes.add(kiI12);
        iTemp[3] = *pRes.add(kiI4) + *pRes.add(kiI12);

        *pRes.add(i) = iTemp[0] + iTemp[3];
        *pRes.add(kiI4) = iTemp[1] + iTemp[2];
        *pRes.add(kiI8) = iTemp[1] - iTemp[2];
        *pRes.add(kiI12) = iTemp[0] - iTemp[3];
    }
}

/// Dequantization of 4x4 Luma DC coefficients for QP < 12
///
/// # Safety
/// - `pRes` must point to a 16-element `int16_t` buffer.
#[inline]
pub unsafe fn WelsDequantLumaDc4x4(pRes: *mut i16, kiQp: i32) {
    let mut i = 15isize;
    let kuiDequantValue = g_kuiDequantCoeff[(kiQp % 6) as usize][0] as i32;
    let kiQF0 = (kiQp / 6) as i16;
    let kiQF1 = 2 - kiQF0;
    let kiQF0S = (1 << (1 - kiQF0)) as i32;

    while i >= 0 {
        *pRes.offset(i) = ((*pRes.offset(i) as i32 * kuiDequantValue + kiQF0S) >> kiQF1) as i16;
        *pRes.offset(i - 1) =
            ((*pRes.offset(i - 1) as i32 * kuiDequantValue + kiQF0S) >> kiQF1) as i16;
        *pRes.offset(i - 2) =
            ((*pRes.offset(i - 2) as i32 * kuiDequantValue + kiQF0S) >> kiQF1) as i16;
        *pRes.offset(i - 3) =
            ((*pRes.offset(i - 3) as i32 * kuiDequantValue + kiQF0S) >> kiQF1) as i16;
        i -= 4;
    }
}

/// 2x2 Inverse Hadamard and dequantization for Chroma DC
///
/// # Safety
/// - `pDct` must point to a 4-element `int16_t` buffer.
#[inline]
pub unsafe fn WelsDequantIHadamard2x2Dc(pDct: *mut i16, kuiMF: u16) {
    let kiSumU = *pDct.add(0) as i32 + *pDct.add(2) as i32;
    let kiDelU = *pDct.add(0) as i32 - *pDct.add(2) as i32;
    let kiSumD = *pDct.add(1) as i32 + *pDct.add(3) as i32;
    let kiDelD = *pDct.add(1) as i32 - *pDct.add(3) as i32;

    let mf = kuiMF as i32;
    *pDct.add(0) = (((kiSumU + kiSumD) * mf) >> 1) as i16;
    *pDct.add(1) = (((kiSumU - kiSumD) * mf) >> 1) as i16;
    *pDct.add(2) = (((kiDelU + kiDelD) * mf) >> 1) as i16;
    *pDct.add(3) = (((kiDelU - kiDelD) * mf) >> 1) as i16;
}

// ============================================================================
// Core Macroblock Encoding Functions
// ============================================================================

/// Computes forward 4x4 integer DCT on all sixteen 4x4 luma blocks within a 16x16 macroblock.
///
/// Divides the 16x16 macroblock into four 8x8 quadrants and executes `pfDctFourT4` once per quadrant.
///
/// # Safety
/// - `pRes` must point to a writable `int16_t` buffer of at least 256 elements.
/// - `pEncMb` and `pBestPred` must point to valid image and prediction sample buffers.
#[inline]
pub unsafe fn WelsDctMb(
    pRes: *mut i16,
    pEncMb: *mut u8,
    iEncStride: i32,
    pBestPred: *mut u8,
    pfDctFourT4: Option<PDctFunc>,
) {
    if let Some(func) = pfDctFourT4 {
        func(pRes, pEncMb, iEncStride, pBestPred, 16);
        func(pRes.add(64), pEncMb.add(8), iEncStride, pBestPred.add(8), 16);
        func(
            pRes.add(128),
            pEncMb.offset(8 * iEncStride as isize),
            iEncStride,
            pBestPred.add(128),
            16,
        );
        func(
            pRes.add(192),
            pEncMb.offset(8 * iEncStride as isize + 8),
            iEncStride,
            pBestPred.add(136),
            16,
        );
    }
}

/// Full DCT, DC Hadamard, quantization, scanning, inverse quantization, and local reconstruction
/// for an **Intra 16x16 Luma** macroblock.
///
/// # Safety
/// All pointers in `pEncCtx`, `pCurMb`, and `pMbCache` must be properly initialized and valid.
pub unsafe fn WelsEncRecI16x16Y(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) {
    let mut aDctT4Dc = [0i16; 16];
    let pFuncList = (*pEncCtx).pFuncList;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let kiEncStride = (*pCurDqLayer).iEncStride[0];
    let mut pRes = (*pMbCache).pCoeffLevel;
    let pPred = (*pMbCache).SPicData.pCsMb[0];
    let kiRecStride = (*pCurDqLayer).iCsStride[0];
    let mut pBlock = (*(*pMbCache).pDct).iLumaBlock[0].as_mut_ptr();
    let pBestPred = (*pMbCache).pMemPredLuma;
    let mut kpNoneZeroCountIdx = 0usize;
    let uiQp = (*pCurMb).uiLumaQp;
    let mut uiNoneZeroCountMbAc = 0u32;

    let pMF = g_kiQuantMF[uiQp as usize].as_ptr();
    let pFF = get_quant_intra_ff(uiQp as usize).as_ptr();

    WelsDctMb(
        pRes,
        (*pMbCache).SPicData.pEncMb[0],
        kiEncStride,
        pBestPred,
        (*pFuncList).pfDctFourT4,
    );

    if let Some(func) = (*pFuncList).pfTransformHadamard4x4Dc {
        func(aDctT4Dc.as_mut_ptr(), pRes);
    }
    if let Some(func) = (*pFuncList).pfQuantizationDc4x4 {
        func(aDctT4Dc.as_mut_ptr(), (*pFF) << 1, (*pMF) >> 1);
    }
    if let Some(func) = (*pFuncList).pfScan4x4 {
        func(
            (*(*pMbCache).pDct).iLumaI16x16Dc.as_mut_ptr(),
            aDctT4Dc.as_mut_ptr(),
        );
    }

    let uiCountI16x16Dc = if let Some(func) = (*pFuncList).pfGetNoneZeroCount {
        func((*(*pMbCache).pDct).iLumaI16x16Dc.as_mut_ptr()) as u32
    } else {
        0
    };

    for _ in 0..4 {
        if let Some(func) = (*pFuncList).pfQuantizationFour4x4 {
            func(pRes, pFF, pMF);
        }
        if let Some(func) = (*pFuncList).pfScan4x4Ac {
            func(pBlock, pRes);
            func(pBlock.add(16), pRes.add(16));
            func(pBlock.add(32), pRes.add(32));
            func(pBlock.add(48), pRes.add(48));
        }
        pRes = pRes.add(64);
        pBlock = pBlock.add(64);
    }
    pRes = pRes.sub(256);
    pBlock = pBlock.sub(256);

    for _ in 0..16 {
        let uiNoneZeroCount = if let Some(func) = (*pFuncList).pfGetNoneZeroCount {
            func(pBlock) as u32
        } else {
            0
        };
        let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
        kpNoneZeroCountIdx += 1;
        *(*pCurMb).pNonZeroCount.add(offset) = uiNoneZeroCount as i8;
        uiNoneZeroCountMbAc += uiNoneZeroCount;
        pBlock = pBlock.add(16);
    }

    if uiCountI16x16Dc > 0 {
        if uiQp < 12 {
            WelsIHadamard4x4Dc(aDctT4Dc.as_mut_ptr());
            WelsDequantLumaDc4x4(aDctT4Dc.as_mut_ptr(), uiQp as i32);
        } else if let Some(func) = (*pFuncList).pfDequantizationIHadamard4x4 {
            func(aDctT4Dc.as_mut_ptr(), g_kuiDequantCoeff[uiQp as usize][0] >> 2);
        }
    }

    if uiNoneZeroCountMbAc > 0 {
        (*pCurMb).uiCbp = 15;
        if let Some(func) = (*pFuncList).pfDequantizationFour4x4 {
            let qp_table = g_kuiDequantCoeff[uiQp as usize].as_ptr();
            func(pRes, qp_table);
            func(pRes.add(64), qp_table);
            func(pRes.add(128), qp_table);
            func(pRes.add(192), qp_table);
        }

        *pRes.add(0) = aDctT4Dc[0];
        *pRes.add(16) = aDctT4Dc[1];
        *pRes.add(32) = aDctT4Dc[4];
        *pRes.add(48) = aDctT4Dc[5];
        *pRes.add(64) = aDctT4Dc[2];
        *pRes.add(80) = aDctT4Dc[3];
        *pRes.add(96) = aDctT4Dc[6];
        *pRes.add(112) = aDctT4Dc[7];
        *pRes.add(128) = aDctT4Dc[8];
        *pRes.add(144) = aDctT4Dc[9];
        *pRes.add(160) = aDctT4Dc[12];
        *pRes.add(176) = aDctT4Dc[13];
        *pRes.add(192) = aDctT4Dc[10];
        *pRes.add(208) = aDctT4Dc[11];
        *pRes.add(224) = aDctT4Dc[14];
        *pRes.add(240) = aDctT4Dc[15];

        if let Some(func) = (*pFuncList).pfIDctFourT4 {
            func(pPred, kiRecStride, pBestPred, 16, pRes);
            func(pPred.add(8), kiRecStride, pBestPred.add(8), 16, pRes.add(64));
            func(
                pPred.offset(kiRecStride as isize * 8),
                kiRecStride,
                pBestPred.add(128),
                16,
                pRes.add(128),
            );
            func(
                pPred.offset(kiRecStride as isize * 8 + 8),
                kiRecStride,
                pBestPred.add(136),
                16,
                pRes.add(192),
            );
        }
    } else if uiCountI16x16Dc > 0 {
        if let Some(func) = (*pFuncList).pfIDctI16x16Dc {
            func(pPred, kiRecStride, pBestPred, 16, aDctT4Dc.as_mut_ptr());
        }
    } else if let Some(func) = (*pFuncList).pfCopy16x16Aligned {
        func(pPred, kiRecStride, pBestPred, 16);
    }
}

/// Forward DCT, quantization, zigzag scan, inverse quantization, and local reconstruction
/// for a single **Intra 4x4 Luma** sub-block.
///
/// # Safety
/// All pointers in `pEncCtx`, `pCurMb`, and `pMbCache` must be properly initialized and valid.
pub unsafe fn WelsEncRecI4x4Y(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    uiI4x4Idx: u8,
) {
    let pFuncList = (*pEncCtx).pFuncList;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let iEncStride = (*pCurDqLayer).iEncStride[0];
    let uiQp = (*pCurMb).uiLumaQp;

    let pResI4x4 = (*pMbCache).pCoeffLevel;
    let pPred = (*pMbCache).SPicData.pCsMb[0];
    let iRecStride = (*pCurDqLayer).iCsStride[0];

    let uiOffset = g_kuiMbCountScan4Idx[uiI4x4Idx as usize] as usize;
    let pEncMb = (*pMbCache).SPicData.pEncMb[0];
    let pBestPred = (*pMbCache).pBestPredI4x4Blk4;
    let pBlock = (*(*pMbCache).pDct).iLumaBlock[uiI4x4Idx as usize].as_mut_ptr();

    let pMF = g_kiQuantMF[uiQp as usize].as_ptr();
    let pFF = get_quant_intra_ff(uiQp as usize).as_ptr();

    let did = (*pEncCtx).uiDependencyId as usize;
    let tid_is_zero = if (*pEncCtx).uiTemporalId == 0 { 1 } else { 0 };
    let pStrideEncBlockOffset = (*(*pEncCtx).pStrideTab).pStrideEncBlockOffset[did];
    let pStrideDecBlockOffset = (*(*pEncCtx).pStrideTab).pStrideDecBlockOffset[did][tid_is_zero];

    let enc_block_offset = *pStrideEncBlockOffset.add(uiI4x4Idx as usize) as isize;
    let dec_block_offset = *pStrideDecBlockOffset.add(uiI4x4Idx as usize) as isize;

    if let Some(func) = (*pFuncList).pfDctT4 {
        func(
            pResI4x4,
            pEncMb.offset(enc_block_offset),
            iEncStride,
            pBestPred,
            4,
        );
    }
    if let Some(func) = (*pFuncList).pfQuantization4x4 {
        func(pResI4x4, pFF, pMF);
    }
    if let Some(func) = (*pFuncList).pfScan4x4 {
        func(pBlock, pResI4x4);
    }

    let iNoneZeroCount = if let Some(func) = (*pFuncList).pfGetNoneZeroCount {
        func(pBlock)
    } else {
        0
    };
    *(*pCurMb).pNonZeroCount.add(uiOffset) = iNoneZeroCount as i8;

    let pPredI4x4 = pPred.offset(dec_block_offset);
    if iNoneZeroCount > 0 {
        (*pCurMb).uiCbp |= 1 << (uiI4x4Idx >> 2);
        if let Some(func) = (*pFuncList).pfDequantization4x4 {
            func(pResI4x4, g_kuiDequantCoeff[uiQp as usize].as_ptr());
        }
        if let Some(func) = (*pFuncList).pfIDctT4 {
            func(pPredI4x4, iRecStride, pBestPred, 4, pResI4x4);
        }
    } else if let Some(func) = (*pFuncList).pfCopy4x4 {
        func(pPredI4x4, iRecStride, pBestPred, 4);
    }
}

/// Quantization, coefficient zigzag scanning, JVT-O079 fast zero-residual thresholding,
/// dequantization, and CBP assignment for **Inter Luma (P/B frames)**.
///
/// # Safety
/// All pointers in `pFuncList`, `pCurMb`, and `pMbCache` must be properly initialized and valid.
pub unsafe fn WelsEncInterY(
    pFuncList: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) {
    let pfQuantizationFour4x4Max = (*pFuncList).pfQuantizationFour4x4Max;
    let pfSetMemZeroSize8 = (*pFuncList).pfSetMemZeroSize8;
    let pfSetMemZeroSize64 = (*pFuncList).pfSetMemZeroSize64;
    let pfScan4x4 = (*pFuncList).pfScan4x4;
    let pfCalculateSingleCtr4x4 = (*pFuncList).pfCalculateSingleCtr4x4;
    let pfGetNoneZeroCount = (*pFuncList).pfGetNoneZeroCount;
    let pfDequantizationFour4x4 = (*pFuncList).pfDequantizationFour4x4;

    let mut pRes = (*pMbCache).pCoeffLevel;
    let mut iSingleCtrMb = 0i32;
    let mut iSingleCtr8x8 = [0i32; 4];
    let mut pBlock = (*(*pMbCache).pDct).iLumaBlock[0].as_mut_ptr();
    let uiQp = (*pCurMb).uiLumaQp;
    let pMF = g_kiQuantMF[uiQp as usize].as_ptr();
    let pFF = g_kiQuantInterFF[uiQp as usize].as_ptr();
    let mut aMax = [0i16; 16];

    for i in 0..4 {
        if let Some(func) = pfQuantizationFour4x4Max {
            func(pRes, pFF, pMF, aMax.as_mut_ptr().add(i << 2));
        }
        iSingleCtr8x8[i] = 0;
        for j in 0..4 {
            let max_val = aMax[(i << 2) + j];
            if max_val == 0 {
                if let Some(func) = pfSetMemZeroSize8 {
                    func(pBlock as *mut c_void, 32);
                } else {
                    core::ptr::write_bytes(pBlock, 0, 16);
                }
            } else {
                if let Some(func) = pfScan4x4 {
                    func(pBlock, pRes);
                }
                if max_val > 1 {
                    iSingleCtr8x8[i] += 9;
                } else if iSingleCtr8x8[i] < 6 {
                    if let Some(func) = pfCalculateSingleCtr4x4 {
                        iSingleCtr8x8[i] += func(pBlock);
                    }
                }
            }
            pRes = pRes.add(16);
            pBlock = pBlock.add(16);
        }
        iSingleCtrMb += iSingleCtr8x8[i];
    }
    pBlock = pBlock.sub(256);
    pRes = pRes.sub(256);

    core::ptr::write_bytes((*pCurMb).pNonZeroCount, 0, 16);

    if iSingleCtrMb < 6 {
        // JVT-O079 zero-residual early cutoff
        if let Some(func) = pfSetMemZeroSize64 {
            func(pRes as *mut c_void, 768);
        } else {
            core::ptr::write_bytes(pRes, 0, 384);
        }
    } else {
        let mut kpNoneZeroCountIdx = 0usize;
        for i in 0..4 {
            if iSingleCtr8x8[i] >= 4 {
                for _ in 0..4 {
                    let iNoneZeroCount = if let Some(func) = pfGetNoneZeroCount {
                        func(pBlock)
                    } else {
                        0
                    };
                    let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
                    kpNoneZeroCountIdx += 1;
                    *(*pCurMb).pNonZeroCount.add(offset) = iNoneZeroCount as i8;
                    pBlock = pBlock.add(16);
                }
                if let Some(func) = pfDequantizationFour4x4 {
                    func(pRes, g_kuiDequantCoeff[uiQp as usize].as_ptr());
                }
                (*pCurMb).uiCbp |= 1 << i;
            } else {
                if let Some(func) = pfSetMemZeroSize64 {
                    func(pRes as *mut c_void, 128);
                } else {
                    core::ptr::write_bytes(pRes, 0, 64);
                }
                kpNoneZeroCountIdx += 4;
                pBlock = pBlock.add(64);
            }
            pRes = pRes.add(64);
        }
    }
}

/// 2x2 Chroma DC Hadamard transform, 4x4 AC quantization, JVT-O079 thresholding,
/// and inverse dequantization for Chroma planes (`iUV = 1` for Cb, `iUV = 2` for Cr).
///
/// # Safety
/// All pointers in `pFuncList`, `pCurMb`, `pMbCache`, and `pRes` must be properly initialized and valid.
pub unsafe fn WelsEncRecUV(
    pFuncList: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    mut pRes: *mut i16,
    iUV: i32,
) {
    let pfQuantizationHadamard2x2 = (*pFuncList).pfQuantizationHadamard2x2;
    let pfQuantizationFour4x4Max = (*pFuncList).pfQuantizationFour4x4Max;
    let pfSetMemZeroSize8 = (*pFuncList).pfSetMemZeroSize8;
    let pfSetMemZeroSize64 = (*pFuncList).pfSetMemZeroSize64;
    let pfScan4x4Ac = (*pFuncList).pfScan4x4Ac;
    let pfCalculateSingleCtr4x4 = (*pFuncList).pfCalculateSingleCtr4x4;
    let pfGetNoneZeroCount = (*pFuncList).pfGetNoneZeroCount;
    let pfDequantizationFour4x4 = (*pFuncList).pfDequantizationFour4x4;

    let kiInterFlag = !IS_INTRA((*pCurMb).uiMbType);
    let kiQp = (*pCurMb).uiChromaQp;
    let uiNoneZeroCountOffset = ((iUV - 1) << 1) as usize;
    let uiSubMbIdx = (16 + ((iUV - 1) << 2)) as usize;
    let iChromaDc = (*(*pMbCache).pDct).iChromaDc[(iUV - 1) as usize].as_mut_ptr();
    let mut pBlock = (*(*pMbCache).pDct).iChromaBlock[((iUV - 1) << 2) as usize].as_mut_ptr();
    let mut aDct2x2 = [0i16; 4];
    let mut aMax = [0i16; 4];
    let mut iSingleCtr8x8 = 0i32;

    let pMF = g_kiQuantMF[kiQp as usize].as_ptr();
    let ff_idx = if !kiInterFlag {
        6 + kiQp as usize
    } else {
        kiQp as usize
    };
    let pFF = g_kiQuantInterFF[ff_idx].as_ptr();

    let uiNoneZeroCountMbDc = if let Some(func) = pfQuantizationHadamard2x2 {
        func(pRes, (*pFF) << 1, (*pMF) >> 1, aDct2x2.as_mut_ptr(), iChromaDc)
    } else {
        0
    };

    if let Some(func) = pfQuantizationFour4x4Max {
        func(pRes, pFF, pMF, aMax.as_mut_ptr());
    }

    for j in 0..4 {
        if aMax[j] == 0 {
            if let Some(func) = pfSetMemZeroSize8 {
                func(pBlock as *mut c_void, 32);
            } else {
                core::ptr::write_bytes(pBlock, 0, 16);
            }
        } else {
            if let Some(func) = pfScan4x4Ac {
                func(pBlock, pRes);
            }
            if kiInterFlag {
                if aMax[j] > 1 {
                    iSingleCtr8x8 += 9;
                } else if iSingleCtr8x8 < 7 {
                    if let Some(func) = pfCalculateSingleCtr4x4 {
                        iSingleCtr8x8 += func(pBlock);
                    }
                }
            } else {
                iSingleCtr8x8 = i32::MAX;
            }
        }
        pRes = pRes.add(16);
        pBlock = pBlock.add(16);
    }
    pRes = pRes.sub(64);

    if iSingleCtr8x8 < 7 {
        if let Some(func) = pfSetMemZeroSize64 {
            func(pRes as *mut c_void, 128);
        } else {
            core::ptr::write_bytes(pRes, 0, 64);
        }
        *(*pCurMb).pNonZeroCount.add(16 + uiNoneZeroCountOffset) = 0;
        *(*pCurMb)
            .pNonZeroCount
            .add(16 + uiNoneZeroCountOffset + 1) = 0;
        *(*pCurMb).pNonZeroCount.add(20 + uiNoneZeroCountOffset) = 0;
        *(*pCurMb)
            .pNonZeroCount
            .add(20 + uiNoneZeroCountOffset + 1) = 0;
    } else {
        let mut kpNoneZeroCountIdx = uiSubMbIdx;
        pBlock = pBlock.sub(64);
        for _ in 0..4 {
            let uiNoneZeroCount = if let Some(func) = pfGetNoneZeroCount {
                func(pBlock)
            } else {
                0
            };
            let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
            kpNoneZeroCountIdx += 1;
            *(*pCurMb).pNonZeroCount.add(offset) = uiNoneZeroCount as i8;
            pBlock = pBlock.add(16);
        }
        if let Some(func) = pfDequantizationFour4x4 {
            func(
                pRes,
                g_kuiDequantCoeff[(*pCurMb).uiChromaQp as usize].as_ptr(),
            );
        }
        (*pCurMb).uiCbp &= 0x0F;
        (*pCurMb).uiCbp |= 0x20;
    }

    if uiNoneZeroCountMbDc > 0 {
        WelsDequantIHadamard2x2Dc(aDct2x2.as_mut_ptr(), g_kuiDequantCoeff[kiQp as usize][0]);
        if 2 != ((*pCurMb).uiCbp >> 4) {
            (*pCurMb).uiCbp |= 0x01 << 4;
        }
        *pRes.add(0) = aDct2x2[0];
        *pRes.add(16) = aDct2x2[1];
        *pRes.add(32) = aDct2x2[2];
        *pRes.add(48) = aDct2x2[3];
    }
}

/// Reconstructs a **P_SKIP** macroblock by copying motion-compensated samples directly
/// to the reconstructed frame buffer and clearing non-zero coefficient counts.
///
/// # Safety
/// All pointers in `pCurLayer`, `pFuncList`, `pCurMb`, and `pMbCache` must be valid.
pub unsafe fn WelsRecPskip(
    pCurLayer: *mut SDqLayer,
    pFuncList: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) {
    let iRecStride = (*pCurLayer).iCsStride.as_ptr();
    let pCsMb = (*pMbCache).SPicData.pCsMb.as_ptr();

    if let Some(func) = (*pFuncList).pfCopy16x16Aligned {
        func(*pCsMb.add(0), *iRecStride.add(0), (*pMbCache).pSkipMb, 16);
    }
    if let Some(func) = (*pFuncList).pfCopy8x8Aligned {
        func(
            *pCsMb.add(1),
            *iRecStride.add(1),
            (*pMbCache).pSkipMb.add(256),
            8,
        );
        func(
            *pCsMb.add(2),
            *iRecStride.add(2),
            (*pMbCache).pSkipMb.add(320),
            8,
        );
    }
    if let Some(func) = (*pFuncList).pfSetMemZeroSize8 {
        func((*pCurMb).pNonZeroCount as *mut c_void, 24);
    } else {
        core::ptr::write_bytes((*pCurMb).pNonZeroCount, 0, 24);
    }
}

/// Fast early-termination test evaluating whether Luma (Y) residual qualifies for `P_SKIP`.
///
/// # Returns
/// - `true`: Residual is zero or negligible ($iSingleCtrMb < 6$), qualifying for `P_SKIP`.
/// - `false`: Non-zero significant residual detected.
///
/// # Safety
/// All pointers in `pEncCtx`, `pCurMb`, and `pMbCache` must be valid.
pub unsafe fn WelsTryPYskip(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> bool {
    let mut iSingleCtrMb = 0i32;
    let mut pRes = (*pMbCache).pCoeffLevel;
    let kuiQp = (*pCurMb).uiLumaQp;
    let mut pBlock = (*(*pMbCache).pDct).iLumaBlock[0].as_mut_ptr();
    let mut aMax = [0u16; 4];
    let pMF = g_kiQuantMF[kuiQp as usize].as_ptr();
    let pFF = g_kiQuantInterFF[kuiQp as usize].as_ptr();

    for _ in 0..4 {
        if let Some(func) = (*(*pEncCtx).pFuncList).pfQuantizationFour4x4Max {
            func(pRes, pFF, pMF, aMax.as_mut_ptr() as *mut i16);
        }

        for j in 0..4 {
            if aMax[j] > 1 {
                return false;
            } else if aMax[j] == 1 {
                if let Some(func) = (*(*pEncCtx).pFuncList).pfScan4x4 {
                    func(pBlock, pRes);
                }
                if let Some(func) = (*(*pEncCtx).pFuncList).pfCalculateSingleCtr4x4 {
                    iSingleCtrMb += func(pBlock);
                }
            }
            if iSingleCtrMb >= 6 {
                return false;
            }
            pRes = pRes.add(16);
            pBlock = pBlock.add(16);
        }
    }
    true
}

/// Fast early-termination test evaluating whether Chroma (U or V) residual qualifies for `P_SKIP`.
///
/// # Returns
/// - `true`: Chroma residual is zero or negligible, qualifying for `P_SKIP`.
/// - `false`: Non-zero chroma DC or significant AC residual detected.
///
/// # Safety
/// All pointers in `pEncCtx`, `pCurMb`, and `pMbCache` must be valid.
pub unsafe fn WelsTryPUVskip(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    iUV: i32,
) -> bool {
    let mut pRes = if iUV == 1 {
        (*pMbCache).pCoeffLevel.add(256)
    } else {
        (*pMbCache).pCoeffLevel.add(256 + 64)
    };

    let pPpsP = (*(*pEncCtx).pCurDqLayer).sLayerInfo.pPpsP;
    let chroma_qp_index_offset = if !pPpsP.is_null() {
        (*pPpsP).uiChromaQpIndexOffset as i32
    } else {
        0
    };
    let clipped_qp = ((*pCurMb).uiLumaQp as i32 + chroma_qp_index_offset).clamp(0, 51);
    let kuiQp = g_kuiChromaQpTable[clipped_qp as usize];

    let pMF = g_kiQuantMF[kuiQp as usize].as_ptr();
    let pFF = g_kiQuantInterFF[kuiQp as usize].as_ptr();

    let hadamard_skip = if let Some(func) = (*(*pEncCtx).pFuncList).pfQuantizationHadamard2x2Skip {
        func(pRes, (*pFF) << 1, (*pMF) >> 1) != 0
    } else {
        false
    };

    if hadamard_skip {
        false
    } else {
        let mut aMax = [0u16; 4];
        let mut iSingleCtrMb = 0i32;
        let mut pBlock = (*(*pMbCache).pDct).iChromaBlock[((iUV - 1) << 2) as usize].as_mut_ptr();

        if let Some(func) = (*(*pEncCtx).pFuncList).pfQuantizationFour4x4Max {
            func(pRes, pFF, pMF, aMax.as_mut_ptr() as *mut i16);
        }

        for j in 0..4 {
            if aMax[j] > 1 {
                return false;
            } else if aMax[j] == 1 {
                if let Some(func) = (*(*pEncCtx).pFuncList).pfScan4x4Ac {
                    func(pBlock, pRes);
                }
                if let Some(func) = (*(*pEncCtx).pFuncList).pfCalculateSingleCtr4x4 {
                    iSingleCtrMb += func(pBlock);
                }
            }
            if iSingleCtrMb >= 7 {
                return false;
            }
            pRes = pRes.add(16);
            pBlock = pBlock.add(16);
        }
        true
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hadamard_4x4_dc_identity() {
        let mut dc_buf = [0i16; 16];
        dc_buf[0] = 16;
        unsafe {
            WelsIHadamard4x4Dc(dc_buf.as_mut_ptr());
        }
        // Since forward Hadamard on a DC impulse distributes energy across all 16 cells,
        // all 16 cells should equal 16.
        for val in dc_buf.iter() {
            assert_eq!(*val, 16);
        }
    }

    #[test]
    fn test_dequant_ihadamard_2x2_dc() {
        let mut dct2x2 = [2i16, 0, 0, 0];
        let mf: u16 = 10;
        unsafe {
            WelsDequantIHadamard2x2Dc(dct2x2.as_mut_ptr(), mf);
        }
        // kiSumU = 2 + 0 = 2, kiDelU = 2 - 0 = 2, kiSumD = 0, kiDelD = 0
        // pDct[0] = ((2 + 0) * 10) >> 1 = 10
        // pDct[1] = ((2 - 0) * 10) >> 1 = 10
        // pDct[2] = ((2 + 0) * 10) >> 1 = 10
        // pDct[3] = ((2 - 0) * 10) >> 1 = 10
        assert_eq!(dct2x2, [10, 10, 10, 10]);
    }

    #[test]
    fn test_chroma_qp_table_bounds() {
        assert_eq!(g_kuiChromaQpTable[0], 0);
        assert_eq!(g_kuiChromaQpTable[29], 29);
        assert_eq!(g_kuiChromaQpTable[30], 29);
        assert_eq!(g_kuiChromaQpTable[51], 39);
    }
}
