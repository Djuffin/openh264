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
    unused_variables
)]

#![forbid(unsafe_code)]

use crate::encoder::rec_view::RecCursor;
use crate::encoder::rec_view::copy_block_to_view;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut};
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::encoder_context::SPicData;
pub use crate::encoder::param_svc::SWelsPPS;
pub use crate::encoder::encoder_context::SStrideTables;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
use crate::encoder::svc_encode_slice::current_layer_ref;
use crate::encoder::encode_mb_aux::{blk4x4, blk4x4_mut, blk_four4x4, blk_four4x4_mut, hadamard2x2_span,
    hadamard2x2_span_mut, hadamard_dc_span};
pub use crate::encoder::md::SMbCache;
use crate::encoder::md::{best_pred_i4x4_blk4_off, mem_pred_luma_off};
use crate::encoder::decode_mb_aux::{idct_four_t4_rec_to_view, idct_rec_i16x16_dc_to_view, idct_t4_rec_to_view};
use crate::encoder::svc_encode_slice::layer_rec_view;
use crate::encoder::svc_encode_slice::layer_rec_view_expect;
use crate::encoder::svc_encode_slice::current_layer_expect;
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
// S9.0: the duplicate declaration of `PDctFunc` that stood here is gone — the
// canonical one is `encode_mb_aux`'s, which `wels_func_ptr_def` already imported.
pub use crate::encoder::encode_mb_aux::PDctFunc;
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
/// **T9.D10**: F103's fourteenth, thirteenth and twelfth — the dequantisers were the
/// rest of the coefficient family's slot half, and they take no plane either. One raw
/// type served both spans here too, so it splits the way `PQuantizationFunc` did.
pub type PDeQuantizationFunc = fn(pRes: &mut [i16; 64], kpMF: &[u16; 8]);
pub type PDeQuantization4x4Func = fn(pRes: &mut [i16; 16], kpMF: &[u16; 8]);
pub type PDeQuantizationIHadamard4x4Func = unsafe extern "C" fn(*mut i16, u16);
// `PIDctFunc` and `PIDctI16x16DcFunc` stood here. The three slots they typed
// were write-only (F138/F139) and are deleted with their installs (S18, session
// F step 0); the second typedef had zero references even before that.
pub type PCopyAlignedFunc = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);
// `PSetMemoryZero = unsafe extern "C" fn(*mut c_void, i32)` was here; see
// `encoder_context::WelsSetMemZero_c` for why the three slots that used it went.

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

// **S9.1: `WelsIDctT4Rec_c` deleted.** It was a shim over the safe kernel below it,
// kept only because the differential tests drove it by name — its dispatch slot
// was deleted in S18 (F138/F139: installed, asserted, never called), and the
// production reconstruction has gone through the seam's kernels since T9.C2. The
// probe now drives the safe kernel directly and keeps the two assertions that
// were ever about more than the shim: sources unmoved, and no write beyond each
// row's block.

// **S9.1: `WelsIDctFourT4Rec_c` deleted.** It was a shim over the safe kernel below it,
// kept only because the differential tests drove it by name — its dispatch slot
// was deleted in S18 (F138/F139: installed, asserted, never called), and the
// production reconstruction has gone through the seam's kernels since T9.C2. The
// probe now drives the safe kernel directly and keeps the two assertions that
// were ever about more than the shim: sources unmoved, and no write beyond each
// row's block.



// ============================================================================
// Math & Transform Helpers (C reference fallbacks)
// ============================================================================

/// 4x4 Inverse Hadamard transform for Intra 16x16 Luma DC
///
/// Inputs above ±2047 can overflow the kernel's plain `i16` intermediates — a
/// debug panic where the C++ wraps (finding F11); the in-contract DC levels stay
/// far below it.
#[inline]
pub fn WelsIHadamard4x4Dc(pRes: &mut [i16; 16]) {
    crate::encoder::decode_mb_aux::ihadamard_4x4_dc(pRes);
}

/// Dequantization of 4x4 Luma DC coefficients for QP < 12
///
/// `kiQp` must be in `0..12`: at 12+ the shift count goes negative, which is a
/// debug panic and the raw port's own behaviour. The one caller is gated on
/// `uiQp < 12`. Not expressible in the type, so it stays a prose contract.
#[inline]
pub fn WelsDequantLumaDc4x4(pRes: &mut [i16; 16], kiQp: i32) {
    crate::encoder::decode_mb_aux::dequant_luma_dc_4x4(pRes, kiQp);
}

/// 2x2 Inverse Hadamard and dequantization for Chroma DC
///
#[inline]
pub fn WelsDequantIHadamard2x2Dc(pDct: &mut [i16; 4], kuiMF: u16) {
    crate::encoder::decode_mb_aux::dequant_ihadamard_2x2_dc(pDct, kuiMF);
}

// ============================================================================
// Core Macroblock Encoding Functions
// ============================================================================

/// Computes forward 4x4 integer DCT on all sixteen 4x4 luma blocks within a 16x16 macroblock.
///
/// Divides the 16x16 macroblock into four 8x8 quadrants and executes `pfDctFourT4` once per quadrant.
///
/// **S9.0**: the two strides are gone from the signature because the cursors carry
/// them, and the four quadrant offsets are now stated in samples rather than in
/// bytes-times-stride. They name the same addresses: the prediction scratch is
/// stride 16, so its old `+8 / +128 / +136` are `(8,0) / (0,8) / (8,8)`.
#[inline]
pub fn WelsDctMb(
    pRes: &mut [i16],
    pEncMb: &crate::encoder::rec_view::RecCursor<'_>,
    pBestPred: &crate::encoder::rec_view::RecCursor<'_>,
    pfDctFourT4: PDctFunc,
) {
    for (k, (dx, dy)) in [(0isize, 0isize), (8, 0), (0, 8), (8, 8)].into_iter().enumerate() {
        pfDctFourT4(
            &mut pRes[k << 6..],
            &pEncMb.advance(dx, dy),
            &pBestPred.advance(dx, dy),
        );
    }
}

/// Full DCT, DC Hadamard, quantization, scanning, inverse quantization, and local reconstruction
/// for an **Intra 16x16 Luma** macroblock.
///
pub fn WelsEncRecI16x16Y(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    let mut aDctT4Dc = [0i16; 16];
    let pFuncList = (*pEncCtx).func_list();
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let kiEncStride = (*pCurDqLayer).iEncStride[0];
    // **T9.D11**: no long-lived raw into `sCoeffLevel`. The DC write-back below is
    // indexed, and the two plane-taking slots derive their cursor where they use it —
    // a `&mut` to this field (`blk_four4x4_mut`) is a Unique retag over the whole
    // array and kills any raw held across it (F114).
    let kiRecStride = (*pCurDqLayer).iCsStride[0];
    // S9.0: the prediction scratch is an owned `[u8; 2*256+16]` on the cache, so a
    // cursor over it needs no raw at all. Stride 16 is the scratch's own geometry,
    // which the raw form passed as a literal at every call.
    let pBestPred = RecCursor::over_owned(
        &mut (*pMbCache).sMemPredMb,
        mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf),
        16,
    );
    let mut kpNoneZeroCountIdx = 0usize;
    let uiQp = (*pCurMb).uiLumaQp;
    let mut uiNoneZeroCountMbAc = 0u32;

    let pMF = &g_kiQuantMF[uiQp as usize];
    let pFF = get_quant_intra_ff(uiQp as usize);

    let encView = crate::encoder::svc_encode_slice::layer_enc_view_expect(&*pCurDqLayer);
    let pEncCur = (*pMbCache).SPicData.mb_cursor_ro(encView, 0);
    WelsDctMb(
        &mut (*pMbCache).sCoeffLevel,
        &pEncCur,
        &pBestPred,
        (*pFuncList).pfDctFourT4,
    );

    ((*pFuncList).pfTransformHadamard4x4Dc)(&mut aDctT4Dc, hadamard_dc_span(&(*pMbCache).sCoeffLevel, 0));
    ((*pFuncList).pfQuantizationDc4x4)(&mut aDctT4Dc, pFF[0] << 1, pMF[0] >> 1);
    ((*pFuncList).pfScan4x4)(&mut (*pMbCache).sDct.iLumaI16x16Dc, &aDctT4Dc);

    let uiCountI16x16Dc = ((*pFuncList).pfGetNoneZeroCount)(&(*pMbCache).sDct.iLumaI16x16Dc) as u32;

    for i in 0..4 {
        ((*pFuncList).pfQuantizationFour4x4)(blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, i << 6), pFF, pMF);
        let func = (*pFuncList).pfScan4x4Ac;
        for j in 0..4 {
            let k = (i << 2) + j;
            func(
                &mut (*pMbCache).sDct.iLumaBlock[k],
                blk4x4(&(*pMbCache).sCoeffLevel, k << 4),
            );
        }
    }

    for k in 0..16 {
        let uiNoneZeroCount = ((*pFuncList).pfGetNoneZeroCount)(&(*pMbCache).sDct.iLumaBlock[k]) as u32;
        let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
        kpNoneZeroCountIdx += 1;
        (*pCurMb).iNonZeroCount[offset] = uiNoneZeroCount as i8;
        uiNoneZeroCountMbAc += uiNoneZeroCount;
    }

    if uiCountI16x16Dc > 0 {
        if uiQp < 12 {
            WelsIHadamard4x4Dc(&mut aDctT4Dc);
            WelsDequantLumaDc4x4(&mut aDctT4Dc, uiQp as i32);
        } else {
            ((*pFuncList).pfDequantizationIHadamard4x4)(
                &mut aDctT4Dc,
                g_kuiDequantCoeff[uiQp as usize][0] >> 2,
            );
        }
    }

    if uiNoneZeroCountMbAc > 0 {
        (*pCurMb).uiCbp = 15;
        let func = (*pFuncList).pfDequantizationFour4x4;
        let qp_table = &g_kuiDequantCoeff[uiQp as usize];
        for i in 0..4 {
            func(blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, i << 6), qp_table);
        }

        // The scanned luma DC returns to block `k`'s DC slot, `sCoeffLevel[k * 16]`,
        // in the C++'s raster-to-zigzag order. Sixteen raw stores through a held
        // cursor before T9.D11; sixteen indexed stores now, same addresses.
        const KI_DC_SCAN: [usize; 16] = [0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15];
        for (k, &src) in KI_DC_SCAN.iter().enumerate() {
            (*pMbCache).sCoeffLevel[k << 4] = aDctT4Dc[src];
        }

        // **T9.C2 — the seam's idct consumers.** Four quadrant calls onto
        // `SPicData.pCsMb[0]`, a raw cursor into the reconstruction plane, become
        // four onto the seam's cursor at this macroblock's own origin. The
        // prediction operand is unchanged in every respect but its type: it was
        // always `sMemPredMb`'s luma half at stride 16, an owned arena array, so
        // it is a slice and a stride rather than a second raw.
        //
        // The four `pBestPred` offsets `0 / 8 / 128 / 136` at stride 16 are the
        // quadrant grid `(0,0) (8,0) (0,8) (8,8)`, which is also what the four
        // `pPred` offsets spell against `kiRecStride`; writing it once as
        // `QUADS` makes the two agree by construction instead of by inspection.
        //
        // **The slot is bypassed, not flipped** (F118): `pfIDctFourT4` is
        // installed unconditionally by `WelsInitEncodingFuncs` and constant after
        // init, so a fixed-size site may call the kernel directly and
        // byte-identically. `kiRecStride` leaves the call because the view
        // carries it.
        const QUADS: [(isize, isize); 4] = [(0, 0), (8, 0), (0, 8), (8, 8)];
        let view = layer_rec_view_expect(&*pCurDqLayer);
        let (lx, ly) = (*pMbCache).SPicData.luma_origin();
        let dst = view.plane(0).cursor(lx, ly);
        let kiPredOff = mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf);
        for (k, &(dx, dy)) in QUADS.iter().enumerate() {
            idct_four_t4_rec_to_view(
                &dst.advance(dx, dy),
                &(*pMbCache).sMemPredMb[kiPredOff + dy as usize * 16 + dx as usize..],
                16,
                blk_four4x4(&(*pMbCache).sCoeffLevel, k << 6),
            );
        }
    } else if uiCountI16x16Dc > 0 {
        // **F137**: this site is the one the plane census never listed, because
        // the census knew neither the `pfIDctI16x16Dc` slot nor its
        // `WelsIDctRecI16x16Dc_c` kernel. It writes the reconstruction plane from
        // the same `pPred` / `pBestPred` pair as the four calls above.
        let view = layer_rec_view_expect(&*pCurDqLayer);
        let (lx, ly) = (*pMbCache).SPicData.luma_origin();
        let kiPredOff = mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf);
        idct_rec_i16x16_dc_to_view(
            &view.plane(0).cursor(lx, ly),
            &(*pMbCache).sMemPredMb[kiPredOff..],
            16,
            &aDctT4Dc,
        );
    } else {
        // **T9.C2.** The residual-free branch: the prediction *is* the
        // reconstruction, copied straight across from `sMemPredMb`'s luma half.
        let view = layer_rec_view_expect(&*pCurDqLayer);
        let (lx, ly) = (*pMbCache).SPicData.luma_origin();
        let kiPredOff = mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf);
        copy_block_to_view::<16>(
            &(*pMbCache).sMemPredMb[kiPredOff..kiPredOff + 256],
            16,
            &view.plane(0).cursor(lx, ly),
            16,
        );
    }
}

/// Forward DCT, quantization, zigzag scan, inverse quantization, and local reconstruction
/// for a single **Intra 4x4 Luma** sub-block.
///
/// # Safety
/// All pointers in `pEncCtx`, `pCurMb`, and `pMbCache` must be properly initialized and valid.
pub fn WelsEncRecI4x4Y(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    uiI4x4Idx: u8,
) {
    let pFuncList = (*pEncCtx).func_list();
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let iEncStride = (*pCurDqLayer).iEncStride[0];
    let uiQp = (*pCurMb).uiLumaQp;

    // **T9.D11**: derived at each use, never held. The two slots below that still
    // take a plane (`pfDctT4`, `pfIDctT4`) need a raw into `sCoeffLevel`; the two
    // that do not take one now borrow the field safely, and a `&mut` to it is a
    // Unique retag over **the whole array** — `blk4x4_mut` takes `&mut [i16]`, so
    // the caller borrows all 384 coefficients before indexing. A raw held across
    // that call is dead, which is what Miri reported here (F114).
    let iRecStride = (*pCurDqLayer).iCsStride[0];

    let uiOffset = g_kuiMbCountScan4Idx[uiI4x4Idx as usize] as usize;
    // S9.0: source plane through the frame's read-only view, prediction scratch
    // through its own owned `[u8; 2*16]`. Stride 4 is the blk4 scratch's geometry,
    // which the raw form passed as a literal at the call.
    let encView = crate::encoder::svc_encode_slice::layer_enc_view_expect(&*pCurDqLayer);
    let pEncMb = (*pMbCache).SPicData.mb_cursor_ro(encView, 0);
    let pBestPred = RecCursor::over_owned(
        &mut (*pMbCache).sMemPredBlk4,
        best_pred_i4x4_blk4_off((*pMbCache).uiBestPredI4x4Blk4Half),
        4,
    );
    let kiBlk = uiI4x4Idx as usize;

    let pMF = &g_kiQuantMF[uiQp as usize];
    let pFF = get_quant_intra_ff(uiQp as usize);

    let did = (*pEncCtx).uiDependencyId as usize;
    let tid_is_zero = if (*pEncCtx).uiTemporalId == 0 { 1 } else { 0 };
    // T9.H2: the two lookups take `&sWelsEncCtx` — a shared reborrow every
    // worker may hold at once. S11.28: through the *bounded* accessors
    // (S11.2d's template), so the per-block read is an index into `[i32; 24]`
    // rather than a raw `.add` — the last two raw cursor reads in this file.
    let tab = (*pEncCtx).pStrideTab.as_ref().expect("the stride tables are built at init");
    let enc_block_offset =
        tab.EncBlockOffsets(did).expect("the enc block-offset table is built")[uiI4x4Idx as usize] as isize;
    let dec_block_offset =
        tab.DecBlockOffsets(did, tid_is_zero).expect("the dec block-offset table is built")[uiI4x4Idx as usize] as isize;

    let func = (*pFuncList).pfDctT4;
    // S9.0: `advance(n, 0)` moves the centre by exactly `n` bytes, which is what
    // `.offset(enc_block_offset)` did — the block offset is a byte offset, not a
    // sample coordinate. The prediction scratch's stride 4 rides in its cursor.
    func(
        &mut (*pMbCache).sCoeffLevel,
        &pEncMb.advance(enc_block_offset, 0),
        &pBestPred,
    );
    ((*pFuncList).pfQuantization4x4)(blk4x4_mut(&mut (*pMbCache).sCoeffLevel, 0), pFF, pMF);
    ((*pFuncList).pfScan4x4)(
        &mut (*pMbCache).sDct.iLumaBlock[kiBlk],
        blk4x4(&(*pMbCache).sCoeffLevel, 0),
    );

    let iNoneZeroCount = ((*pFuncList).pfGetNoneZeroCount)(&(*pMbCache).sDct.iLumaBlock[kiBlk]);
    (*pCurMb).iNonZeroCount[uiOffset] = iNoneZeroCount as i8;

    if iNoneZeroCount > 0 {
        (*pCurMb).uiCbp |= 1 << (uiI4x4Idx >> 2);
        ((*pFuncList).pfDequantization4x4)(
            blk4x4_mut(&mut (*pMbCache).sCoeffLevel, 0),
            &g_kuiDequantCoeff[uiQp as usize],
        );
        // **T9.C2.** `pPredI4x4` is `pPred.offset(dec_block_offset)`, and
        // `dec_block_offset` is a flat byte offset into a plane of `iRecStride`
        // — so `(off % stride, off / stride)` is the same address the raw form
        // reached, exactly. The 4x4 blocks sit at `dx, dy` in `{0,4,8,12}` and
        // the stride is never below 16, so neither term can wrap into the other.
        // Prediction is `sMemPredBlk4` at stride 4 (not 16 — this is the 4x4
        // arena, and its rows are four bytes).
        let view = layer_rec_view_expect(&*pCurDqLayer);
        let (lx, ly) = (*pMbCache).SPicData.luma_origin();
        let (dx, dy) = (dec_block_offset % iRecStride as isize, dec_block_offset / iRecStride as isize);
        let kiPredOff = best_pred_i4x4_blk4_off((*pMbCache).uiBestPredI4x4Blk4Half);
        idct_t4_rec_to_view(
            &view.plane(0).cursor(lx + dx, ly + dy),
            &(*pMbCache).sMemPredBlk4[kiPredOff..],
            4,
            blk4x4(&(*pMbCache).sCoeffLevel, 0),
        );
    } else {
        // **T9.C2.** As the `pfIDctT4` branch above: `dec_block_offset` divides by
        // `iRecStride` into the 4x4 block's `(dx, dy)` within the macroblock, and
        // the prediction is `sMemPredBlk4` at stride 4.
        let view = layer_rec_view_expect(&*pCurDqLayer);
        let (lx, ly) = (*pMbCache).SPicData.luma_origin();
        let (dx, dy) =
            (dec_block_offset % iRecStride as isize, dec_block_offset / iRecStride as isize);
        let kiPredOff = best_pred_i4x4_blk4_off((*pMbCache).uiBestPredI4x4Blk4Half);
        copy_block_to_view::<4>(
            &(*pMbCache).sMemPredBlk4[kiPredOff..kiPredOff + 16],
            4,
            &view.plane(0).cursor(lx + dx, ly + dy),
            4,
        );
    }
}

/// Quantization, coefficient zigzag scanning, JVT-O079 fast zero-residual thresholding,
/// dequantization, and CBP assignment for **Inter Luma (P/B frames)**.
///
/// # Safety
/// All pointers in `pFuncList`, `pCurMb`, and `pMbCache` must be properly initialized and valid.
pub fn WelsEncInterY(
    pFuncList: &SWelsFuncPtrList,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    let pfQuantizationFour4x4Max = pFuncList.pfQuantizationFour4x4Max;
    let pfScan4x4 = pFuncList.pfScan4x4;
    let pfCalculateSingleCtr4x4 = pFuncList.pfCalculateSingleCtr4x4;
    let pfGetNoneZeroCount = pFuncList.pfGetNoneZeroCount;
    let pfDequantizationFour4x4 = pFuncList.pfDequantizationFour4x4;

    // **T9.D11**: the two `WelsSetMemZero_c` calls are `fill(0)` now — this body has
    // no raw into `sCoeffLevel` at all, and cannot have one, because the residual
    // slots take `&mut [i16]` of the whole field (F114).
    let mut iSingleCtrMb = 0i32;
    let mut iSingleCtr8x8 = [0i32; 4];
    let uiQp = (*pCurMb).uiLumaQp;
    let pMF = &g_kiQuantMF[uiQp as usize];
    let pFF = &g_kiQuantInterFF[uiQp as usize];
    let mut aMax = [0i16; 16];

    for i in 0..4 {
        let func = pfQuantizationFour4x4Max;
        let max4: &mut [i16; 4] = (&mut aMax[i << 2..(i << 2) + 4]).try_into().expect("4");
        func(blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, i << 6), pFF, pMF, max4);
        iSingleCtr8x8[i] = 0;
        for j in 0..4 {
            let k = (i << 2) + j;
            let max_val = aMax[k];
            if max_val == 0 {
                // `WelsSetMemZero_c(pBlock, 32)` — 32 bytes is one 4x4 block of i16.
                (*pMbCache).sDct.iLumaBlock[k].fill(0);
            } else {
                let func = pfScan4x4;
                func(
                    &mut (*pMbCache).sDct.iLumaBlock[k],
                    blk4x4(&(*pMbCache).sCoeffLevel, k << 4),
                );
                if max_val > 1 {
                    iSingleCtr8x8[i] += 9;
                } else if iSingleCtr8x8[i] < 6 {
                    let func = pfCalculateSingleCtr4x4;
                    iSingleCtr8x8[i] += func(&(*pMbCache).sDct.iLumaBlock[k]);
                }
            }
        }
        iSingleCtrMb += iSingleCtr8x8[i];
    }

    // `WelsSetMemZero (pCurMb->pNonZeroCount, 16)` — the 16 luma entries only.
    (&mut (*pCurMb).iNonZeroCount)[0..16].fill(0);

    if iSingleCtrMb < 6 {
        // JVT-O079 zero-residual early cutoff
        // `WelsSetMemZero_c(pRes, 768)` — 768 bytes is all 384 coefficients.
        (*pMbCache).sCoeffLevel.fill(0);
    } else {
        let mut kpNoneZeroCountIdx = 0usize;
        for i in 0..4 {
            if iSingleCtr8x8[i] >= 4 {
                for j in 0..4 {
                    let iNoneZeroCount = pfGetNoneZeroCount(&(*pMbCache).sDct.iLumaBlock[(i << 2) + j]);
                    let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
                    kpNoneZeroCountIdx += 1;
                    (*pCurMb).iNonZeroCount[offset] = iNoneZeroCount as i8;
                }
                let func = pfDequantizationFour4x4;
                func(
                    blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, i << 6),
                    &g_kuiDequantCoeff[uiQp as usize],
                );
                (*pCurMb).uiCbp |= 1 << i;
            } else {
                // `WelsSetMemZero_c(pRes + i*64, 128)` — 128 bytes is one 64-coefficient quadrant.
                (*pMbCache).sCoeffLevel[i << 6..(i << 6) + 64].fill(0);
                kpNoneZeroCountIdx += 4;
            }
        }
    }
}

/// 2x2 Chroma DC Hadamard transform, 4x4 AC quantization, JVT-O079 thresholding,
/// and inverse dequantization for Chroma planes (`iUV = 1` for Cb, `iUV = 2` for Cr).
///
/// **T9.D6 — `pRes` was a `*mut i16` into `pMbCache->sCoeffLevel`, and it is a
/// `usize` index into that array now.** The C++ hands this function the arena *and* a
/// cursor into it, which is the one shape a `&mut SMbCache` parameter cannot survive:
/// whichever argument is evaluated second invalidates the first, and no ordering
/// helps. So the second path is deleted and the callee derives the cursor.
///
/// **It is not `(iUV - 1) * 64` off a fixed base, and that matters.** The two callers
/// use *different* bases: `WelsIMbChromaEncode` passes `pCoeffLevel + 0`
/// (`svc_encode_slice.cpp:475`) and `WelsPMbChromaEncode` passes `pCoeffLevel + 256`
/// (`:499`). The offset is caller state, not a function of `iUV`, so it stays a
/// parameter — as an index, which a retag cannot invalidate.
///
/// # Safety
/// `kiResOff .. kiResOff + 128` must be in bounds of `pMbCache->sCoeffLevel`.
pub fn WelsEncRecUV(
    pFuncList: &SWelsFuncPtrList,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    kiResOff: usize,
    iUV: i32,
) {
    let pfQuantizationHadamard2x2 = pFuncList.pfQuantizationHadamard2x2;
    let pfQuantizationFour4x4Max = pFuncList.pfQuantizationFour4x4Max;
    let pfScan4x4Ac = pFuncList.pfScan4x4Ac;
    let pfCalculateSingleCtr4x4 = pFuncList.pfCalculateSingleCtr4x4;
    let pfGetNoneZeroCount = pFuncList.pfGetNoneZeroCount;
    let pfDequantizationFour4x4 = pFuncList.pfDequantizationFour4x4;

    let kiInterFlag = !IS_INTRA((*pCurMb).uiMbType);
    let kiQp = (*pCurMb).uiChromaQp;
    let uiNoneZeroCountOffset = ((iUV - 1) << 1) as usize;
    let uiSubMbIdx = (16 + ((iUV - 1) << 2)) as usize;
    // **T9.D3**: one `dct()` derivation, not two. Both cursors named the same
    // `sDct` and the second call sat *between* the first cursor and its use, which
    // is the hazard shape exactly — `q1c.py --type SMbCache` flagged it here.
    // **T9.D8**: `iChromaBlock` is `[[i16; 16]; 8]`, so this plane's four blocks are
    // `[kiChromaBlk ..][0..4]` — the walking `pBlock` cursor is an index now.
    let kiChromaBlk = ((iUV - 1) << 2) as usize;
    let mut aDct2x2 = [0i16; 4];
    let mut aMax = [0i16; 4];
    let mut iSingleCtr8x8 = 0i32;

    let pMF = &g_kiQuantMF[kiQp as usize];
    let ff_idx = if !kiInterFlag {
        6 + kiQp as usize
    } else {
        kiQp as usize
    };
    let pFF = &g_kiQuantInterFF[ff_idx];

    let uiNoneZeroCountMbDc = pfQuantizationHadamard2x2(
        hadamard2x2_span_mut(&mut (*pMbCache).sCoeffLevel, kiResOff),
        pFF[0] << 1,
        pMF[0] >> 1,
        &mut aDct2x2,
        &mut (*pMbCache).sDct.iChromaDc[(iUV - 1) as usize],
    );

    let func = pfQuantizationFour4x4Max;
    func(blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, kiResOff), pFF, pMF, &mut aMax);

    for j in 0..4 {
        let k = kiChromaBlk + j;
        if aMax[j] == 0 {
            (*pMbCache).sDct.iChromaBlock[k].fill(0);
        } else {
            let func = pfScan4x4Ac;
            func(
                &mut (*pMbCache).sDct.iChromaBlock[k],
                blk4x4(&(*pMbCache).sCoeffLevel, kiResOff + (j << 4)),
            );
            if kiInterFlag {
                if aMax[j] > 1 {
                    iSingleCtr8x8 += 9;
                } else if iSingleCtr8x8 < 7 {
                    let func = pfCalculateSingleCtr4x4;
                    iSingleCtr8x8 += func(&(*pMbCache).sDct.iChromaBlock[k]);
                }
            } else {
                iSingleCtr8x8 = i32::MAX;
            }
        }
    }

    if iSingleCtr8x8 < 7 {
        // `WelsSetMemZero_c(pRes, 128)` — one 64-coefficient chroma group (T9.D11).
        (*pMbCache).sCoeffLevel[kiResOff..kiResOff + 64].fill(0);
        (*pCurMb).iNonZeroCount[16 + uiNoneZeroCountOffset] = 0;
        (*pCurMb).iNonZeroCount[16 + uiNoneZeroCountOffset + 1] = 0;
        (*pCurMb).iNonZeroCount[20 + uiNoneZeroCountOffset] = 0;
        (*pCurMb).iNonZeroCount[20 + uiNoneZeroCountOffset + 1] = 0;
    } else {
        let mut kpNoneZeroCountIdx = uiSubMbIdx;
        for j in 0..4 {
            let uiNoneZeroCount = pfGetNoneZeroCount(&(*pMbCache).sDct.iChromaBlock[kiChromaBlk + j]);
            let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
            kpNoneZeroCountIdx += 1;
            (*pCurMb).iNonZeroCount[offset] = uiNoneZeroCount as i8;
        }
        let func = pfDequantizationFour4x4;
        func(
            blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, kiResOff),
            &g_kuiDequantCoeff[(*pCurMb).uiChromaQp as usize],
        );
        (*pCurMb).uiCbp &= 0x0F;
        (*pCurMb).uiCbp |= 0x20;
    }

    if uiNoneZeroCountMbDc > 0 {
        WelsDequantIHadamard2x2Dc(&mut aDct2x2, g_kuiDequantCoeff[kiQp as usize][0]);
        if 2 != ((*pCurMb).uiCbp >> 4) {
            (*pCurMb).uiCbp |= 0x01 << 4;
        }
        for (k, &v) in aDct2x2.iter().enumerate() {
            (*pMbCache).sCoeffLevel[kiResOff + (k << 4)] = v;
        }
    }
}

// **`WelsRecPskip` stood here — deleted in T9.C2 on F135's ruling.**
//
// The port carried two copies of `svc_encode_mb.cpp:315`: this one, in the file
// the C++ puts it in, and a second in `svc_mode_decision.rs` beside the three
// call sites. Only the second was ever reached — all three callers
// (`svc_base_layer_md.cpp:1395`/`:1957`, `svc_mode_decision.cpp:440`) resolve
// there, and T9.C7 converted it to the reconstruction seam. This one had no
// caller in `src/`, `tests/` or `benches/`; both being `pub` in a `pub mod`,
// no compiler pass could say so (F129), only a per-symbol grep.
//
// The surviving implementation keeps the provenance: its doc cites
// `codec/encoder/core/src/svc_encode_mb.cpp:315`. Three blocked plane-census
// rows come off here as a **deletion**, never summed with a conversion (F128).

/// Fast early-termination test evaluating whether Luma (Y) residual qualifies for `P_SKIP`.
///
/// # Returns
/// - `true`: Residual is zero or negligible ($iSingleCtrMb < 6$), qualifying for `P_SKIP`.
/// - `false`: Non-zero significant residual detected.
///
/// # Safety
/// All pointers in `pEncCtx`, `pCurMb`, and `pMbCache` must be valid.
pub fn WelsTryPYskip(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool {
    let mut iSingleCtrMb = 0i32;
    let kuiQp = (*pCurMb).uiLumaQp;
    // **T9.D8**: `aMax` was `[u16; 4]` cast to `*mut i16` at the call. The kernel
    // writes `max_abs`, which starts at 0 and only grows, so every entry is
    // non-negative and the two spellings compare identically — the array is `i16`
    // now, which is what was always stored in it.
    let mut aMax = [0i16; 4];
    let pMF = &g_kiQuantMF[kuiQp as usize];
    let pFF = &g_kiQuantInterFF[kuiQp as usize];

    for i in 0..4 {
        ((*pEncCtx).func_list().pfQuantizationFour4x4Max)(blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, i << 6), pFF, pMF, &mut aMax);

        for j in 0..4 {
            let k = (i << 2) + j;
            if aMax[j] > 1 {
                return false;
            } else if aMax[j] == 1 {
                ((*pEncCtx).func_list().pfScan4x4)(
                    &mut (*pMbCache).sDct.iLumaBlock[k],
                    blk4x4(&(*pMbCache).sCoeffLevel, k << 4),
                );
                let func = (*pEncCtx).func_list().pfCalculateSingleCtr4x4;
                iSingleCtrMb += func(&(*pMbCache).sDct.iLumaBlock[k]);
            }
            if iSingleCtrMb >= 6 {
                return false;
            }
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
pub fn WelsTryPUVskip(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    iUV: i32,
) -> bool {
    // **T9.D8**: the base is an offset now, not a cursor.
    let kiResOff = if iUV == 1 { 256usize } else { 256 + 64 };

    let chroma_qp_index_offset = if let Some(pps) = crate::encoder::svc_encode_slice::layer_pps_ref(
        pEncCtx,
        crate::encoder::svc_encode_slice::current_layer_expect(pEncCtx),
    ) {
        pps.uiChromaQpIndexOffset as i32
    } else {
        0
    };
    let clipped_qp = ((*pCurMb).uiLumaQp as i32 + chroma_qp_index_offset).clamp(0, 51);
    let kuiQp = g_kuiChromaQpTable[clipped_qp as usize];

    let pMF = &g_kiQuantMF[kuiQp as usize];
    let pFF = &g_kiQuantInterFF[kuiQp as usize];

    let hadamard_skip = ((*pEncCtx).func_list().pfQuantizationHadamard2x2Skip)(
        hadamard2x2_span(&(*pMbCache).sCoeffLevel, kiResOff),
        pFF[0] << 1,
        pMF[0] >> 1,
    ) != 0;

    if hadamard_skip {
        false
    } else {
        let mut aMax = [0i16; 4];
        let mut iSingleCtrMb = 0i32;
        let kiChromaBlk = ((iUV - 1) << 2) as usize;

        ((*pEncCtx).func_list().pfQuantizationFour4x4Max)(blk_four4x4_mut(&mut (*pMbCache).sCoeffLevel, kiResOff), pFF, pMF, &mut aMax);

        for j in 0..4 {
            let k = kiChromaBlk + j;
            if aMax[j] > 1 {
                return false;
            } else if aMax[j] == 1 {
                ((*pEncCtx).func_list().pfScan4x4Ac)(
                    &mut (*pMbCache).sDct.iChromaBlock[k],
                    blk4x4(&(*pMbCache).sCoeffLevel, kiResOff + (j << 4)),
                );
                let func = (*pEncCtx).func_list().pfCalculateSingleCtr4x4;
                iSingleCtrMb += func(&(*pMbCache).sDct.iChromaBlock[k]);
            }
            if iSingleCtrMb >= 7 {
                return false;
            }
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
        WelsIHadamard4x4Dc(&mut dc_buf);
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
        WelsDequantIHadamard2x2Dc(&mut dct2x2, mf);
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
