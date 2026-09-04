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
    unused_variables
)]

#![forbid(unsafe_code)]

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

/// The fixed-shape block-copy slot.
///
/// Both operands are `RecCursor` because the slot's two callers disagree
/// about storage: the background path copies picture-to-picture, while the
/// mode-decision path copies an owned prediction scratch into a picture plane. A
/// function-pointer table cannot be generic, so the scratch reaches the same type
/// through `RecCursor::over_owned`.
pub type PCopyFunc =
    fn(pDst: &crate::encoder::rec_view::RecCursor<'_>, pSrc: &crate::encoder::rec_view::RecCursor<'_>);
/// The forward-DCT slot.
pub type PDctFunc = fn(
    pDct: &mut [i16],
    pSample1: &crate::encoder::rec_view::RecCursor<'_>,
    pSample2: &crate::encoder::rec_view::RecCursor<'_>,
);

// The odd lengths below are the reach, not the block: `hadamard_quant_2x2` and
// its `Skip` twin read `rs[0]`, `rs[16]`, `rs[32]`, `rs[48]` — so their span is
// 49, not 64 — and `hadamard_t4_dc` reads block 15's DC at index 240, so its
// span is 241.
pub type PCalculateSingleCtrFunc = fn(pDct: &[i16; 16]) -> i32;
pub type PScanFunc = fn(pLevel: &mut [i16; 16], pDct: &[i16; 16]);
pub type PQuantization4x4Func = fn(pDct: &mut [i16; 16], pFF: &[i16; 8], pMF: &[i16; 8]);
pub type PQuantizationFunc = fn(pDct: &mut [i16; 64], pFF: &[i16; 8], pMF: &[i16; 8]);
pub type PQuantizationMaxFunc =
    fn(pDct: &mut [i16; 64], pFF: &[i16; 8], pMF: &[i16; 8], pMax: &mut [i16; 4]);
pub type PQuantizationDcFunc = fn(pDct: &mut [i16; 16], iFF: i16, iMF: i16);
pub type PQuantizationSkipFunc = fn(pDct: &[i16; 49], iFF: i16, iMF: i16) -> i32;
pub type PQuantizationHadamardFunc =
    fn(pRes: &mut [i16; 49], kiFF: i16, iMF: i16, pDct: &mut [i16; 4], pBlock: &mut [i16; 4]) -> i32;
pub type PTransformHadamard4x4Func = fn(pLumaDc: &mut [i16; 16], pDct: &[i16; 241]);
pub type PGetNoneZeroCountFunc = fn(pLevel: &[i16; 16]) -> i32;

/// The 4x4 block at coefficient offset `off`, where the callee reads one block.
#[inline]
pub fn blk4x4(a: &[i16], off: usize) -> &[i16; 16] {
    a[off..off + 16].try_into().expect("a 4x4 block is 16 coefficients")
}

/// [`blk4x4`], mutably.
#[inline]
pub fn blk4x4_mut(a: &mut [i16], off: usize) -> &mut [i16; 16] {
    (&mut a[off..off + 16]).try_into().expect("a 4x4 block is 16 coefficients")
}

/// [`blk_four4x4_mut`], shared — the reconstruction kernels only read their
/// coefficients.
#[inline]
pub fn blk_four4x4(a: &[i16], off: usize) -> &[i16; 64] {
    a[off..off + 64].try_into().expect("four 4x4 blocks are 64 coefficients")
}

/// The whole macroblock's 256 luma coefficients at `off` — `WelsIDctT4RecOnMb`'s
/// span, which it walks as four quadrants of 64.
#[inline]
pub fn blk_mb256(a: &[i16], off: usize) -> &[i16; 256] {
    a[off..off + 256].try_into().expect("a macroblock's luma is 256 coefficients")
}

/// The four 4x4 blocks at coefficient offset `off`, where the callee reads a
/// quadrant.
#[inline]
pub fn blk_four4x4_mut(a: &mut [i16], off: usize) -> &mut [i16; 64] {
    (&mut a[off..off + 64]).try_into().expect("four 4x4 blocks are 64 coefficients")
}

/// The 2x2-Hadamard span at `off`: `rs[0]`, `rs[16]`, `rs[32]`, `rs[48]` and nothing
/// past index 48.
#[inline]
pub fn hadamard2x2_span(a: &[i16], off: usize) -> &[i16; 49] {
    a[off..off + 49].try_into().expect("the 2x2 Hadamard reaches index 48")
}

/// [`hadamard2x2_span`], mutably.
#[inline]
pub fn hadamard2x2_span_mut(a: &mut [i16], off: usize) -> &mut [i16; 49] {
    (&mut a[off..off + 49]).try_into().expect("the 2x2 Hadamard reaches index 48")
}

/// The luma-DC Hadamard's span from `off`: block 15's DC sits at index 240.
#[inline]
pub fn hadamard_dc_span(a: &[i16], off: usize) -> &[i16; 241] {
    a[off..off + 241].try_into().expect("the luma DC Hadamard reaches index 240")
}

// ============================================================================
// Encoder Function Pointer Table (SWelsFuncPtrList)
// ============================================================================

// The two pixel-reading kernels (the forward DCTs) reach forward only from their
// own (0, 0) — no `-1` column, no `-stride` row. The DCT and Hadamard
// intermediates are `i32`; on all in-contract inputs the values stay far inside
// `i32` (the per-kernel bounds are derived in the doc comments below), so none of
// these kernels can panic in a debug build. The `as i16` narrowings are the C++'s
// own implicit `int -> int16_t` conversions, kept where the C++ has them.

use crate::common::copy_mb::{copy_16x16, copy_16x8, copy_4x4, copy_4x8, copy_8x16, copy_8x4, copy_8x8};
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, SampleCursor};

/// The kernel set the dispatch sites below call: `simd::x86_64` or `simd::aarch64` by default,
/// `simd::wide` under `--features wide`. Imported rather than spelled in full at each
/// site because the kernels share their names with the scalars in this module — which
/// is the point of the naming, and the reason the module qualifier has to stay.
use crate::simd::kernels;

/// Residual of two 4x4 pixel blocks, then the 2-D forward integer DCT, into
/// raster order.
///
/// C++: `WelsDctT4_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
///
/// Bound derivation (why this is total): inputs are `u8` pixels, so each
/// residual is in `[-255, 255]`; one 1-D pass gains at most 6x
/// (`|2a + b - c - 2d| <= 6 * 255`), so after both passes every value is
/// inside `+-36 * 255 = +-9180` — no `i32` intermediate and no `i16` store
/// can overflow on any input the signature admits. (The C++ computes the same
/// values in `int16_t` scratch; the two agree everywhere because the values
/// fit `i16` too.)
pub fn dct_4x4<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 16], pix1: &A, pix2: &B) {
    let mut data = [0i32; 16];
    let mut s = [0i32; 4];

    for row in 0..4usize {
        let i = row << 2;
        // `row_n` by value, because the source operand may be a shared
        // interior-mutable plane and a shared view cannot lend a slice into cells.
        let r1 = pix1.row_n::<4>(row as isize, 0);
        let r2 = pix2.row_n::<4>(row as isize, 0);
        for k in 0..4 {
            data[i + k] = r1[k] as i32 - r2[k] as i32;
        }

        // Horizontal 1D transform.
        s[0] = data[i] + data[i + 3];
        s[3] = data[i] - data[i + 3];
        s[1] = data[i + 1] + data[i + 2];
        s[2] = data[i + 1] - data[i + 2];

        dct[i] = (s[0] + s[1]) as i16;
        dct[i + 2] = (s[0] - s[1]) as i16;
        dct[i + 1] = ((s[3] << 1) + s[2]) as i16;
        dct[i + 3] = (s[3] - (s[2] << 1)) as i16;
    }

    // Vertical 1D transform.
    for i in 0..4usize {
        let d0 = dct[i] as i32;
        let d4 = dct[4 + i] as i32;
        let d8 = dct[8 + i] as i32;
        let d12 = dct[12 + i] as i32;

        s[0] = d0 + d12;
        s[3] = d0 - d12;
        s[1] = d4 + d8;
        s[2] = d4 - d8;

        dct[i] = (s[0] + s[1]) as i16;
        dct[8 + i] = (s[0] - s[1]) as i16;
        dct[4 + i] = ((s[3] << 1) + s[2]) as i16;
        dct[12 + i] = (s[3] - (s[2] << 1)) as i16;
    }
}

/// [`dct_4x4`] on the four 4x4 blocks of one 8x8 quadrant, 16 coefficients
/// each, in the C++'s block order: top-left, top-right, bottom-left,
/// bottom-right.
///
/// C++: `WelsDctFourT4_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn dct_four_4x4<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 64], pix1: &A, pix2: &B) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &mut [i16; 16] = (&mut dct[k << 4..][..16]).try_into().unwrap();
        dct_4x4(sub, &pix1.advance(dx, dy), &pix2.advance(dx, dy));
    }
}

/// Dead-zone quantization of one coefficient: `sign(v) * (((ff + |v|) * mf) >> 16)`.
///
/// Bound derivation (why the widths hold): `|v| <= 32767` and the tables'
/// `ff <= 767 << 1`, `mf <= 26214`, so `(ff + |v|) * mf < 2^31` — but the
/// contract this family actually needs is weaker and worth stating once:
/// for any **non-negative** `ff` and `mf` up to `i16::MAX`,
/// `(ff + |v|) * mf <= 65534 * 32767 < i32::MAX`, so the product cannot
/// overflow. A negative `mf` could, and no table contains one.
#[inline(always)]
fn quant_one(v: i16, ff: i32, mf: i32) -> i16 {
    let sign = (v as i32) >> 31;
    let abs = (sign ^ (v as i32)) - sign;
    let q = ((ff + abs) * mf) >> 16;
    ((sign ^ q) - sign) as i16
}

/// In-place dead-zone forward quantization of a 4x4 block. `ff`/`mf` are one
/// 8-lane row of the QP tables; lane `i & 0x07` quantizes coefficient `i`,
/// exactly the C++'s indexing.
///
/// C++: `WelsQuant4x4_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn quant_4x4(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    for i in (0..16).step_by(4) {
        let j = i & 0x07;
        for k in 0..4 {
            dct[i + k] = quant_one(dct[i + k], ff[j + k] as i32, mf[j + k] as i32);
        }
    }
}

/// In-place quantization of the 16 Hadamard-transformed luma DC coefficients
/// with scalar factors (the callers pass `ff << 1`, `mf >> 1`).
///
/// C++: `WelsQuant4x4Dc_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn quant_4x4_dc(dct: &mut [i16; 16], ff: i16, mf: i16) {
    let (ff, mf) = (ff as i32, mf as i32);
    for v in dct.iter_mut() {
        *v = quant_one(*v, ff, mf);
    }
}

/// In-place dead-zone quantization of four consecutive 4x4 blocks.
///
/// C++: `WelsQuantFour4x4_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn quant_four_4x4(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8]) {
    for i in (0..64).step_by(4) {
        let j = i & 0x07;
        for k in 0..4 {
            dct[i + k] = quant_one(dct[i + k], ff[j + k] as i32, mf[j + k] as i32);
        }
    }
}

/// [`quant_four_4x4`], also returning each block's maximum absolute quantized
/// level in `max[0..4]` — the callers' early-zero test.
///
/// C++: `WelsQuantFour4x4Max_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn quant_four_4x4_max(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8], max: &mut [i16; 4]) {
    for (k, m) in max.iter_mut().enumerate() {
        let mut max_abs: i16 = 0;
        for i in 0..16 {
            let j = i & 0x07;
            let v = dct[(k << 4) + i];
            let sign = (v as i32) >> 31;
            let abs = (sign ^ (v as i32)) - sign;
            let q_mag = (((ff[j] as i32 + abs) * mf[j] as i32) >> 16) as i16;
            if max_abs < q_mag {
                max_abs = q_mag;
            }
            dct[(k << 4) + i] = ((sign ^ (q_mag as i32)) - sign) as i16;
        }
        *m = max_abs;
    }
}

/// The four chroma DC coefficients a 2x2 Hadamard reads, at raster positions
/// 0, 16, 32 and 48 of the chroma coefficient group — index 48 is the reach,
/// which is why the parameter is `[i16; 49]` and not `[i16; 64]`: 49 elements
/// is exactly the span the kernel touches.
#[inline(always)]
fn hadamard_2x2_butterfly(rs: &[i16; 49]) -> [i32; 4] {
    let (r0, r16, r32, r48) = (rs[0] as i32, rs[16] as i32, rs[32] as i32, rs[48] as i32);
    let s0 = r0 + r32;
    let s1 = r0 - r32;
    let s2 = r16 + r48;
    let s3 = r16 - r48;
    [s0 + s2, s0 - s2, s1 + s3, s1 - s3]
}

/// Early-termination test for the 2x2 chroma DC Hadamard: 1 if any transformed
/// coefficient would survive quantization at `(ff, mf)`, else 0.
///
/// The threshold division requires `mf != 0`; the callers' `mf` comes from
/// `g_kiQuantMF >> 1`, whose smallest entry is 28 >> 1 = 14.
///
/// C++: `WelsHadamardQuant2x2Skip_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn hadamard_quant_2x2_skip(rs: &[i16; 49], ff: i16, mf: i16) -> i32 {
    let threshold: i32 = if mf != 0 {
        ((1i32 << 16) - 1) / (mf as i32) - (ff as i32)
    } else {
        0
    };
    let d = hadamard_2x2_butterfly(rs);
    let over = d.iter().any(|&v| {
        let abs = (v ^ (v >> 31)) - (v >> 31);
        abs > threshold
    });
    over as i32
}

/// 2x2 forward Hadamard of the four chroma DC coefficients, quantization into
/// `dct` and `block`, and the DC positions of `rs` cleared. Returns the count
/// of non-zero quantized levels.
///
/// The butterfly is computed in `i32` and narrowed per the C++'s implicit
/// `int -> int16_t` store (`|d| <= 4 * 32767` can exceed `i16`, and the
/// truncation is the C++'s own behaviour, kept).
///
/// C++: `WelsHadamardQuant2x2_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn hadamard_quant_2x2(
    rs: &mut [i16; 49],
    ff: i16,
    mf: i16,
    dct: &mut [i16; 4],
    block: &mut [i16; 4],
) -> i32 {
    let d = hadamard_2x2_butterfly(rs);
    rs[0] = 0;
    rs[16] = 0;
    rs[32] = 0;
    rs[48] = 0;

    let (ff, mf) = (ff as i32, mf as i32);
    let mut dc_nzc = 0;
    for i in 0..4 {
        let q = quant_one(d[i] as i16, ff, mf);
        dct[i] = q;
        block[i] = q;
        if q != 0 {
            dc_nzc += 1;
        }
    }
    dc_nzc
}

/// 4x4 forward Hadamard of the 16 luma DC coefficients of an I16x16
/// macroblock, `(x + 1) >> 1` rounded and clipped to `i16`.
///
/// `dct` is the macroblock's 256-coefficient luma buffer; the DC of raster
/// block `k` sits at `dct[k * 16]`, and the highest one read is block 15's at
/// index 240 — hence `[i16; 241]`, the exact reach. Computed in `i32` with an
/// explicit clip, as the C++ does (`WELS_CLIP3`), so it is total over the full
/// `i16` input range.
///
/// C++: `WelsHadamardT4Dc_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn hadamard_t4_dc(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    let mut p = [0i32; 16];
    let mut s = [0i32; 4];

    for i in (0..16).step_by(4) {
        let idx = ((i & 0x08) << 4) + ((i & 0x04) << 3);
        let d0 = dct[idx] as i32;
        let d80 = dct[idx + 80] as i32;
        let d16 = dct[idx + 16] as i32;
        let d64 = dct[idx + 64] as i32;

        s[0] = d0 + d80;
        s[3] = d0 - d80;
        s[1] = d16 + d64;
        s[2] = d16 - d64;

        p[i] = s[0] + s[1];
        p[i + 2] = s[0] - s[1];
        p[i + 1] = s[3] + s[2];
        p[i + 3] = s[3] - s[2];
    }

    for i in 0..4usize {
        s[0] = p[i] + p[i + 12];
        s[3] = p[i] - p[i + 12];
        s[1] = p[i + 4] + p[i + 8];
        s[2] = p[i + 4] - p[i + 8];

        luma_dc[i] = ((s[0] + s[1] + 1) >> 1).clamp(-32768, 32767) as i16;
        luma_dc[i + 8] = ((s[0] - s[1] + 1) >> 1).clamp(-32768, 32767) as i16;
        luma_dc[i + 4] = ((s[3] + s[2] + 1) >> 1).clamp(-32768, 32767) as i16;
        luma_dc[i + 12] = ((s[3] - s[2] + 1) >> 1).clamp(-32768, 32767) as i16;
    }
}

/// The 4x4 zigzag permutation: raster position of scan position `i`.
const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

/// All 16 coefficients of `dct`, zigzag-reordered into `level`.
///
/// C++: `WelsScan4x4DcAc_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
/// (`WelsScan4x4Dc` is the same permutation; its shim calls in here too.)
pub fn scan_4x4_dc_ac(level: &mut [i16; 16], dct: &[i16; 16]) {
    for (l, &z) in level.iter_mut().zip(ZIGZAG.iter()) {
        *l = dct[z];
    }
}

/// The 15 AC coefficients of `dct` (DC omitted), zigzag-reordered into
/// `level[0..15]`, with `level[15] = 0`.
///
/// C++: `WelsScan4x4Ac_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn scan_4x4_ac(level: &mut [i16; 16], dct: &[i16; 16]) {
    for (l, &z) in level.iter_mut().zip(ZIGZAG[1..].iter()) {
        *l = dct[z];
    }
    level[15] = 0;
}

/// JVT-O079 CAVLC bit-cost estimate: for each run of zeros between non-zero
/// coefficients (scanning from the high end), add the run-length penalty.
///
/// C++: `WelsCalculateSingleCtr4x4_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn calculate_single_ctr_4x4(dct: &[i16; 16]) -> i32 {
    let mut single_ctr: i32 = 0;
    let mut idx: i32 = 15;

    while idx >= 0 && dct[idx as usize] == 0 {
        idx -= 1;
    }

    while idx >= 0 {
        idx -= 1;
        let mut run = idx;
        while idx >= 0 && dct[idx as usize] == 0 {
            idx -= 1;
        }
        run -= idx;
        if (run as usize) < KI_TRUN_TABLE.len() {
            single_ctr += KI_TRUN_TABLE[run as usize];
        }
    }

    single_ctr
}

/// Count of non-zero coefficients in a 16-element level array.
///
/// C++: `WelsGetNoneZeroCount_c`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn get_none_zero_count(level: &[i16; 16]) -> i32 {
    16 - level.iter().filter(|&&v| v == 0).count() as i32
}

// ============================================================================
// Forward Discrete Cosine Transform (FDCT)
// ============================================================================

/// Computes pixel residual differencing followed by the 2D 4x4 Forward Integer DCT.
///
/// Both pixel cursors are anchored at sample `(0, 0)` of their 4x4 block and are
/// only read; the kernel reaches forward only — no `-1` column, no `-stride` row.
///
/// Every call path is a `pfDctT4` table slot; its one caller, `WelsEncRecI4x4Y`
/// (`svc_encode_mb.rs`), hands a source macroblock cursor and the stride-4 I4x4
/// prediction scratch.
///
/// # Panics
/// If `pDct` holds fewer than 16 coefficients, or if either cursor's 4x4 block
/// leaves its plane.
#[inline]
pub fn WelsDctT4_c(
    pDct: &mut [i16],
    pPixel1: &crate::encoder::rec_view::RecCursor<'_>,
    pPixel2: &crate::encoder::rec_view::RecCursor<'_>,
) {
    let dct: &mut [i16; 16] = (&mut pDct[..16]).try_into().unwrap();
    dct_4x4(dct, pPixel1, pPixel2);
}

/// Performs 4x4 FDCT on four adjacent 4x4 blocks forming an 8x8 quadrant.
///
/// Both pixel cursors are anchored at sample `(0, 0)` of their 8x8 block and are
/// only read; the kernel reaches forward only.
///
/// # Panics
/// If `pDct` holds fewer than 64 coefficients, or if either cursor's 8x8 block
/// leaves its plane.
#[inline]
pub fn WelsDctFourT4_c(
    pDct: &mut [i16],
    pPixel1: &crate::encoder::rec_view::RecCursor<'_>,
    pPixel2: &crate::encoder::rec_view::RecCursor<'_>,
) {
    let dct: &mut [i16; 64] = (&mut pDct[..64]).try_into().unwrap();
    dct_four_4x4(dct, pPixel1, pPixel2);
}

pub fn WelsDctT4_sse2(
    pDct: &mut [i16],
    pPixel1: &crate::encoder::rec_view::RecCursor<'_>,
    pPixel2: &crate::encoder::rec_view::RecCursor<'_>,
) {
    let dct: &mut [i16; 16] = (&mut pDct[..16]).try_into().unwrap();
    kernels::dct::dct_4x4(dct, pPixel1, pPixel2);
}

pub fn WelsDctFourT4_sse2(
    pDct: &mut [i16],
    pPixel1: &crate::encoder::rec_view::RecCursor<'_>,
    pPixel2: &crate::encoder::rec_view::RecCursor<'_>,
) {
    let dct: &mut [i16; 64] = (&mut pDct[..64]).try_into().unwrap();
    kernels::dct::dct_four_4x4(dct, pPixel1, pPixel2);
}

// ============================================================================
// Forward Quantization Functions
// ============================================================================





// ============================================================================
// Forward Hadamard Transforms
// ============================================================================




// ============================================================================
// Zigzag Scanning Functions
// ============================================================================



/// Reorders 16 DC coefficients into 1D zigzag scan order (identical to `WelsScan4x4DcAc_c`).
///
/// Unlike its two neighbours this one is **not** installed in an `SWelsFuncPtrList`
/// slot and the encoder never calls it; its only caller in the workspace is
/// `tests/kernels_differential_phase2.rs`.
#[inline]
pub fn WelsScan4x4Dc(pLevel: &mut [i16; 16], pDct: &[i16; 16]) {
    scan_4x4_dc_ac(pLevel, pDct);
}

// ============================================================================
// Non-Zero Count and CAVLC Bit Scoring
// ============================================================================



// ============================================================================
// Pixel Block Copy Fallbacks (matching copy_mb.h)
// ============================================================================

/// Copies a 4x4 block of samples from `pSrc` to `pDst`, row by row through
/// `copy_rows_shared`.
#[inline]
pub fn WelsCopy4x4_c(
    pDst: &crate::encoder::rec_view::RecCursor<'_>,
    pSrc: &crate::encoder::rec_view::RecCursor<'_>,
) {
    crate::encoder::rec_view::copy_rows_shared::<4>(pDst, pSrc, 4);
}

/// Copies an 8-wide, 4-tall block of samples from `pSrc` to `pDst`, row by row
/// through `copy_rows_shared`.
#[inline]
pub fn WelsCopy8x4_c(
    pDst: &crate::encoder::rec_view::RecCursor<'_>,
    pSrc: &crate::encoder::rec_view::RecCursor<'_>,
) {
    crate::encoder::rec_view::copy_rows_shared::<8>(pDst, pSrc, 4);
}

/// Copies a 4-wide, 8-tall block of samples from `pSrc` to `pDst`, row by row
/// through `copy_rows_shared`.
#[inline]
pub fn WelsCopy4x8_c(
    pDst: &crate::encoder::rec_view::RecCursor<'_>,
    pSrc: &crate::encoder::rec_view::RecCursor<'_>,
) {
    crate::encoder::rec_view::copy_rows_shared::<4>(pDst, pSrc, 8);
}

/// Copies an 8x8 block of samples from `pSrc` to `pDst`, row by row through
/// `copy_rows_shared`.
///
/// (The decoder's error-concealment module has its own same-named kernel —
/// different function, never unify.)
#[inline]
pub fn WelsCopy8x8_c(
    pDst: &crate::encoder::rec_view::RecCursor<'_>,
    pSrc: &crate::encoder::rec_view::RecCursor<'_>,
) {
    crate::encoder::rec_view::copy_rows_shared::<8>(pDst, pSrc, 8);
}

/// Copies a 16-wide, 8-tall block of samples from `pSrc` to `pDst`, row by row
/// through `copy_rows_shared`.
#[inline]
pub fn WelsCopy16x8_c(
    pDst: &crate::encoder::rec_view::RecCursor<'_>,
    pSrc: &crate::encoder::rec_view::RecCursor<'_>,
) {
    crate::encoder::rec_view::copy_rows_shared::<16>(pDst, pSrc, 8);
}

/// Copies an 8-wide, 16-tall block of samples from `pSrc` to `pDst`, row by row
/// through `copy_rows_shared`.
#[inline]
pub fn WelsCopy8x16_c(
    pDst: &crate::encoder::rec_view::RecCursor<'_>,
    pSrc: &crate::encoder::rec_view::RecCursor<'_>,
) {
    crate::encoder::rec_view::copy_rows_shared::<8>(pDst, pSrc, 16);
}

/// Copies a 16x16 block of samples from `pSrc` to `pDst`, row by row through
/// `copy_rows_shared`.
///
/// (Same name-collision note as [`WelsCopy8x8_c`].)
#[inline]
pub fn WelsCopy16x16_c(
    pDst: &crate::encoder::rec_view::RecCursor<'_>,
    pSrc: &crate::encoder::rec_view::RecCursor<'_>,
) {
    crate::encoder::rec_view::copy_rows_shared::<16>(pDst, pSrc, 16);
}

// ARM NEON fallbacks

// AArch64 NEON fallbacks

// Loongson MMI fallbacks

// Loongson LSX / LASX fallbacks

// ============================================================================
// Function Dispatch Table Initialization
// ============================================================================

/// Initializes the encoder function pointer table dynamically based on CPU feature flags.
pub extern "C" fn WelsInitEncodingFuncs(pFuncList: &mut SWelsFuncPtrList, uiCpuFlag: u32) {

    let f = &mut *pFuncList;

    // Baseline C fallback functions
    f.pfCopy8x8Aligned = WelsCopy8x8_c;
    f.pfCopy16x16Aligned = WelsCopy16x16_c;
    f.pfCopy16x16NotAligned = WelsCopy16x16_c;
    f.pfCopy16x8NotAligned = WelsCopy16x8_c;
    f.pfCopy8x16Aligned = WelsCopy8x16_c;
    f.pfCopy4x4 = WelsCopy4x4_c;
    f.pfCopy8x4 = WelsCopy8x4_c;
    f.pfCopy4x8 = WelsCopy4x8_c;

    f.pfQuantizationHadamard2x2 = hadamard_quant_2x2;
    f.pfQuantizationHadamard2x2Skip = hadamard_quant_2x2_skip;
    f.pfTransformHadamard4x4Dc = hadamard_t4_dc;

    f.pfDctT4 = WelsDctT4_c;
    f.pfDctFourT4 = WelsDctFourT4_c;

    f.pfScan4x4 = scan_4x4_dc_ac;
    f.pfScan4x4Ac = scan_4x4_ac;
    f.pfCalculateSingleCtr4x4 = calculate_single_ctr_4x4;

    f.pfGetNoneZeroCount = get_none_zero_count;

    f.pfQuantization4x4 = quant_4x4;
    f.pfQuantizationDc4x4 = quant_4x4_dc;
    f.pfQuantizationFour4x4 = quant_four_4x4;
    f.pfQuantizationFour4x4Max = quant_four_4x4_max;

    if (uiCpuFlag & WELS_CPU_SSE2) != 0 {
        // Both 16x16 slots take the unaligned kernel; `simd/x86_64/copy.rs`
        // explains why upstream's aligned/not-aligned split is not reproduced.
        f.pfCopy16x16Aligned = kernels::copy::copy_16x16;
        f.pfCopy16x16NotAligned = kernels::copy::copy_16x16;
        f.pfCopy16x8NotAligned = kernels::copy::copy_16x8;
        f.pfCopy8x16Aligned = kernels::copy::copy_8x16;
        f.pfCopy8x8Aligned = kernels::copy::copy_8x8;

        f.pfCalculateSingleCtr4x4 = kernels::score::calculate_single_ctr_4x4;

        // `pfScan4x4` and `pfScan4x4Ac` stay scalar on purpose: `scan_4x4_dc_ac`
        // already compiles to a shorter shuffle sequence than `score.asm`'s, for
        // the reason `simd/x86_64/score.rs` sets out.

        f.pfDctT4 = WelsDctT4_sse2;
        f.pfDctFourT4 = WelsDctFourT4_sse2;
        f.pfTransformHadamard4x4Dc = kernels::quant::hadamard_t4_dc;
        f.pfGetNoneZeroCount = kernels::quant::get_none_zero_count;
        f.pfQuantization4x4 = kernels::quant::quant_4x4;
        f.pfQuantizationDc4x4 = kernels::quant::quant_4x4_dc;
        f.pfQuantizationFour4x4 = kernels::quant::quant_four_4x4;
        f.pfQuantizationFour4x4Max = kernels::quant::quant_four_4x4_max;
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// **`pfScan4x4` and `pfScan4x4Ac` are scalar by decision, not by omission.**
    /// Both kernels were written, measured against the scalar, and dropped — the
    /// reasons are at `simd/x86_64/score.rs`. Wiring one in should have to come here
    /// and delete this test, rather than happening quietly.
    #[test]
    fn the_scan_slots_stay_scalar_under_the_simd_flag() {
        use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

        let mut scalar = SWelsFuncPtrList::default();
        WelsInitEncodingFuncs(&mut scalar, 0);
        let mut simd = SWelsFuncPtrList::default();
        WelsInitEncodingFuncs(&mut simd, WELS_CPU_SSE2);

        for (name, a, b) in [
            ("pfScan4x4", scalar.pfScan4x4 as usize, simd.pfScan4x4 as usize),
            ("pfScan4x4Ac", scalar.pfScan4x4Ac as usize, simd.pfScan4x4Ac as usize),
        ] {
            assert_eq!(a, b, "{name} gained a SIMD kernel — see simd/x86_64/score.rs first");
        }
    }

    /// The DCT kernels are generic over [`SampleCursor`], and the source operand
    /// reaches them as a `RecCursor` over a shared interior-mutable plane rather
    /// than as a `PlaneCursor` over a slice — because the source picture is
    /// written in-fork by `VaaBackgroundMbDataUpdate`. Two storages, one kernel:
    /// this pins that they read identically.
    ///
    /// The second assertion biases the shared cursor by one sample and requires
    /// the outputs to *differ* — if the kernel ignored its source operand, or if
    /// both arms silently read the same cursor, the equality above would pass for
    /// the wrong reason and this line would fail.
    #[test]
    fn dct_reads_a_shared_source_plane_exactly_as_a_slice_cursor_does() {
        use crate::encoder::rec_view::shared_plane_for_test;
        use crate::safe::plane::PaddedPlane;

        let (w, h, pad, stride) = (32usize, 16usize, 16usize, 64usize);
        let mut plane = PaddedPlane::new(w, h, pad, stride);
        for (i, b) in plane.as_mut_slice().iter_mut().enumerate() {
            *b = ((i * 37 + 11) & 0xFF) as u8;
        }
        let pred = [7u8; 256];
        let predc = PlaneCursor::new(&pred, 0, 16);

        let mut via_slice = [0i16; 64];
        dct_four_4x4(&mut via_slice, &plane.cursor(0, 0), &predc);

        let shared = shared_plane_for_test(&mut plane);
        let mut via_shared = [0i16; 64];
        dct_four_4x4(&mut via_shared, &shared.cursor(0, 0), &predc);
        assert_eq!(
            via_slice, via_shared,
            "a RecCursor source and a PlaneCursor source over the same bytes disagree"
        );

        // control
        let mut biased = [0i16; 64];
        dct_four_4x4(&mut biased, &shared.cursor(1, 0), &predc);
        assert_ne!(
            via_shared, biased,
            "control: a one-sample bias changed nothing, so this test cannot fail"
        );
    }

    #[test]
    fn test_fdct_t4() {
        let p1 = [
            10u8, 20, 30, 40,
            15,   25, 35, 45,
            20,   30, 40, 50,
            25,   35, 45, 55,
        ];
        let p2 = [0u8; 16];
        let mut dct = [0i16; 16];

        dct_4x4(&mut dct, &PlaneCursor::new(&p1, 0, 4), &PlaneCursor::new(&p2, 0, 4));

        assert_ne!(dct[0], 0);
    }

    #[test]
    fn test_quant_4x4() {
        let mut dct = [100i16; 16];
        let qp = 26usize;
        let ff = &g_kiQuantInterFF[qp];
        let mf = &g_kiQuantMF[qp];

        quant_4x4(&mut dct, ff, mf);

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

        let span: &mut [i16; 49] = (&mut res[..49]).try_into().unwrap();
        let nnz = hadamard_quant_2x2(span, ff, mf, &mut dct, &mut block);

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
        scan_4x4_dc_ac(&mut level, &dct);

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

        let count = get_none_zero_count(&level);
        assert_eq!(count, 3);
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`.
pub use crate::common::cpu_core::{WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_FMA, WELS_CPU_LASX, WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MMXEXT, WELS_CPU_MSA, WELS_CPU_NEON, WELS_CPU_SSE2, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3};
