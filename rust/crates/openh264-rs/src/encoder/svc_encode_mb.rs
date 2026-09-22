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

//! Macroblock encoding and local reconstruction —
//! `codec/encoder/core/src/svc_encode_mb.cpp`.
//!
//! Forward DCT, Hadamard transform, dead-zone scalar quantization, coefficient zigzag
//! scanning, JVT-O079 fast zero-residual early termination, inverse quantization/IDCT
//! and local reconstruction, for Intra 16x16, Intra 4x4, Inter P/B luma, chroma UV and
//! P_SKIP macroblocks.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

use crate::encoder::decode_mb_aux::{
    idct_four_t4_rec_to_view, idct_rec_i16x16_dc_to_view, idct_t4_rec_to_view,
};
use crate::encoder::encode_mb_aux::{
    blk_four4x4, blk_four4x4_mut, blk4x4, blk4x4_mut, hadamard_dc_span, hadamard2x2_span,
    hadamard2x2_span_mut,
};
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SPicData;
pub use crate::encoder::encoder_context::SStrideTables;
pub use crate::encoder::encoder_context::sWelsEncCtx;
pub use crate::encoder::md::SMB;
pub use crate::encoder::md::SMbCache;
use crate::encoder::md::{best_pred_i4x4_blk4_off, mem_pred_luma_off};
pub use crate::encoder::param_svc::SWelsPPS;
use crate::encoder::rec_view::{RecCursor, copy_block_to_view};
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
use crate::encoder::svc_encode_slice::{
    current_layer_expect, layer_enc_view_expect, layer_pps_ref, layer_rec_view_expect,
};
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

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

pub use crate::common::common_tables::{
    g_kiQuantInterFF, g_kiQuantMF, g_kuiChromaQpTable, g_kuiDequantCoeff, g_kuiMbCountScan4Idx,
};

/// Intra quantization rounding factor table (alias for `g_kiQuantInterFF + 6`)
#[inline(always)]
pub fn get_quant_intra_ff(qp: usize) -> &'static [i16; 8] {
    &g_kiQuantInterFF[qp + 6]
}

// ============================================================================
// Core Struct Definitions
// ============================================================================

pub const MAX_DEPENDENCY_LAYER: usize = 4;

// Function pointer signatures for SWelsFuncPtrList
use crate::encoder::decode_mb_aux::{
    dequant_ihadamard_2x2_dc, dequant_luma_dc_4x4, ihadamard_4x4_dc,
};
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
pub type PDeQuantizationFunc = fn(pRes: &mut [i16; 64], kpMF: &[u16; 8]);
pub type PDeQuantization4x4Func = fn(pRes: &mut [i16; 16], kpMF: &[u16; 8]);
pub type PDeQuantizationIHadamard4x4Func = unsafe extern "C" fn(*mut i16, u16);
pub type PCopyAlignedFunc = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);

// ============================================================================
// Math & Transform Helpers
// ============================================================================

/// 4x4 inverse Hadamard transform for Intra 16x16 luma DC.
///
/// Inputs above ±2047 overflow the kernel's `i16` intermediates and panic in debug;
/// in-contract DC levels stay far below that.
#[inline]
pub fn WelsIHadamard4x4Dc(pRes: &mut [i16; 16]) {
    ihadamard_4x4_dc(pRes);
}

/// Dequantization of 4x4 luma DC coefficients for QP < 12.
///
/// `kiQp` must be in `0..12`: at 12 and above the shift count goes negative, which
/// panics in debug. The one caller is gated on `uiQp < 12`.
#[inline]
pub fn WelsDequantLumaDc4x4(pRes: &mut [i16; 16], kiQp: i32) {
    dequant_luma_dc_4x4(pRes, kiQp);
}

/// 2x2 inverse Hadamard and dequantization for chroma DC.
#[inline]
pub fn WelsDequantIHadamard2x2Dc(pDct: &mut [i16; 4], kuiMF: u16) {
    dequant_ihadamard_2x2_dc(pDct, kuiMF);
}

// ============================================================================
// Core Macroblock Encoding Functions
// ============================================================================

/// Forward 4x4 integer DCT over all sixteen 4x4 luma blocks of a macroblock, as four
/// 8x8 quadrants with one `pfDctFourT4` call each.
#[inline]
pub fn WelsDctMb(
    pRes: &mut [i16],
    pEncMb: &RecCursor<'_>,
    pBestPred: &RecCursor<'_>,
    pfDctFourT4: PDctFunc,
) {
    for (k, (dx, dy)) in [(0isize, 0isize), (8, 0), (0, 8), (8, 8)]
        .into_iter()
        .enumerate()
    {
        pfDctFourT4(
            &mut pRes[k << 6..],
            &pEncMb.advance(dx, dy),
            &pBestPred.advance(dx, dy),
        );
    }
}

/// Full DCT, DC Hadamard, quantization, scanning, inverse quantization and local
/// reconstruction for an Intra 16x16 luma macroblock.
pub fn WelsEncRecI16x16Y(pEncCtx: &sWelsEncCtx, pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    let mut aDctT4Dc = [0i16; 16];
    let pFuncList = pEncCtx.func_list();
    let pCurDqLayer = current_layer_expect(pEncCtx);
    // The prediction scratch is owned by the cache and has stride 16.
    let pBestPred = RecCursor::over_owned(
        &mut pMbCache.sMemPredMb,
        mem_pred_luma_off(pMbCache.uiMemPredLumaHalf),
        16,
    );
    let mut kpNoneZeroCountIdx = 0usize;
    let uiQp = pCurMb.uiLumaQp;
    let mut uiNoneZeroCountMbAc = 0u32;

    let pMF = &g_kiQuantMF[uiQp as usize];
    let pFF = get_quant_intra_ff(uiQp as usize);

    let encView = layer_enc_view_expect(pCurDqLayer);
    let pEncCur = pMbCache.SPicData.mb_cursor_ro(encView, 0);
    WelsDctMb(
        &mut pMbCache.sCoeffLevel,
        &pEncCur,
        &pBestPred,
        pFuncList.pfDctFourT4,
    );

    (pFuncList.pfTransformHadamard4x4Dc)(&mut aDctT4Dc, hadamard_dc_span(&pMbCache.sCoeffLevel, 0));
    (pFuncList.pfQuantizationDc4x4)(&mut aDctT4Dc, pFF[0] << 1, pMF[0] >> 1);
    (pFuncList.pfScan4x4)(&mut pMbCache.sDct.iLumaI16x16Dc, &aDctT4Dc);

    let uiCountI16x16Dc = (pFuncList.pfGetNoneZeroCount)(&pMbCache.sDct.iLumaI16x16Dc) as u32;

    for i in 0..4 {
        (pFuncList.pfQuantizationFour4x4)(
            blk_four4x4_mut(&mut pMbCache.sCoeffLevel, i << 6),
            pFF,
            pMF,
        );
        let func = pFuncList.pfScan4x4Ac;
        for j in 0..4 {
            let k = (i << 2) + j;
            func(
                &mut pMbCache.sDct.iLumaBlock[k],
                blk4x4(&pMbCache.sCoeffLevel, k << 4),
            );
        }
    }

    for k in 0..16 {
        let uiNoneZeroCount = (pFuncList.pfGetNoneZeroCount)(&pMbCache.sDct.iLumaBlock[k]) as u32;
        let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
        kpNoneZeroCountIdx += 1;
        pCurMb.iNonZeroCount[offset] = uiNoneZeroCount as i8;
        uiNoneZeroCountMbAc += uiNoneZeroCount;
    }

    if uiCountI16x16Dc > 0 {
        if uiQp < 12 {
            WelsIHadamard4x4Dc(&mut aDctT4Dc);
            WelsDequantLumaDc4x4(&mut aDctT4Dc, uiQp as i32);
        } else {
            (pFuncList.pfDequantizationIHadamard4x4)(
                &mut aDctT4Dc,
                g_kuiDequantCoeff[uiQp as usize][0] >> 2,
            );
        }
    }

    if uiNoneZeroCountMbAc > 0 {
        pCurMb.uiCbp = 15;
        let func = pFuncList.pfDequantizationFour4x4;
        let qp_table = &g_kuiDequantCoeff[uiQp as usize];
        for i in 0..4 {
            func(blk_four4x4_mut(&mut pMbCache.sCoeffLevel, i << 6), qp_table);
        }

        // The scanned luma DC returns to block `k`'s DC slot, `sCoeffLevel[k * 16]`,
        // in raster-to-zigzag order.
        const KI_DC_SCAN: [usize; 16] = [0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15];
        for (k, &src) in KI_DC_SCAN.iter().enumerate() {
            pMbCache.sCoeffLevel[k << 4] = aDctT4Dc[src];
        }

        // The 8x8 quadrant grid, applied to both the prediction scratch at stride 16
        // and the reconstruction plane at its own stride.
        const QUADS: [(isize, isize); 4] = [(0, 0), (8, 0), (0, 8), (8, 8)];
        let view = layer_rec_view_expect(pCurDqLayer);
        let (lx, ly) = pMbCache.SPicData.luma_origin();
        let dst = view.plane(0).cursor(lx, ly);
        let kiPredOff = mem_pred_luma_off(pMbCache.uiMemPredLumaHalf);
        for (k, &(dx, dy)) in QUADS.iter().enumerate() {
            idct_four_t4_rec_to_view(
                &dst.advance(dx, dy),
                &pMbCache.sMemPredMb[kiPredOff + dy as usize * 16 + dx as usize..],
                16,
                blk_four4x4(&pMbCache.sCoeffLevel, k << 6),
            );
        }
    } else if uiCountI16x16Dc > 0 {
        let view = layer_rec_view_expect(pCurDqLayer);
        let (lx, ly) = pMbCache.SPicData.luma_origin();
        let kiPredOff = mem_pred_luma_off(pMbCache.uiMemPredLumaHalf);
        idct_rec_i16x16_dc_to_view(
            &view.plane(0).cursor(lx, ly),
            &pMbCache.sMemPredMb[kiPredOff..],
            16,
            &aDctT4Dc,
        );
    } else {
        // Residual-free: the prediction is the reconstruction, copied straight across
        // from `sMemPredMb`'s luma half.
        let view = layer_rec_view_expect(pCurDqLayer);
        let (lx, ly) = pMbCache.SPicData.luma_origin();
        let kiPredOff = mem_pred_luma_off(pMbCache.uiMemPredLumaHalf);
        copy_block_to_view::<16, 16>(
            &pMbCache.sMemPredMb[kiPredOff..kiPredOff + 256],
            &view.plane(0).cursor(lx, ly),
        );
    }
}

/// Forward DCT, quantization, zigzag scan, inverse quantization and local reconstruction
/// for a single Intra 4x4 luma sub-block.
pub fn WelsEncRecI4x4Y(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    uiI4x4Idx: u8,
) {
    let pFuncList = pEncCtx.func_list();
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let uiQp = pCurMb.uiLumaQp;

    let uiOffset = g_kuiMbCountScan4Idx[uiI4x4Idx as usize] as usize;
    // The source plane comes through the frame's read-only view; the prediction scratch
    // is owned by the cache and has stride 4.
    let encView = layer_enc_view_expect(pCurDqLayer);
    let pEncMb = pMbCache.SPicData.mb_cursor_ro(encView, 0);
    let pBestPred = RecCursor::over_owned(
        &mut pMbCache.sMemPredBlk4,
        best_pred_i4x4_blk4_off(pMbCache.uiBestPredI4x4Blk4Half),
        4,
    );
    let kiBlk = uiI4x4Idx as usize;
    let dx = crate::encoder::svc_base_layer_md::g_kiCoordinateIdx4x4X[kiBlk] as isize;
    let dy = crate::encoder::svc_base_layer_md::g_kiCoordinateIdx4x4Y[kiBlk] as isize;

    let pMF = &g_kiQuantMF[uiQp as usize];
    let pFF = get_quant_intra_ff(uiQp as usize);

    let func = pFuncList.pfDctT4;
    func(
        &mut pMbCache.sCoeffLevel,
        &pEncMb.advance(dx, dy),
        &pBestPred,
    );
    (pFuncList.pfQuantization4x4)(blk4x4_mut(&mut pMbCache.sCoeffLevel, 0), pFF, pMF);
    (pFuncList.pfScan4x4)(
        &mut pMbCache.sDct.iLumaBlock[kiBlk],
        blk4x4(&pMbCache.sCoeffLevel, 0),
    );

    let iNoneZeroCount = (pFuncList.pfGetNoneZeroCount)(&pMbCache.sDct.iLumaBlock[kiBlk]);
    pCurMb.iNonZeroCount[uiOffset] = iNoneZeroCount as i8;

    let view = layer_rec_view_expect(pCurDqLayer);
    let (lx, ly) = pMbCache.SPicData.luma_origin();
    let kiPredOff = best_pred_i4x4_blk4_off(pMbCache.uiBestPredI4x4Blk4Half);

    if iNoneZeroCount > 0 {
        pCurMb.uiCbp |= 1 << (uiI4x4Idx >> 2);
        (pFuncList.pfDequantization4x4)(
            blk4x4_mut(&mut pMbCache.sCoeffLevel, 0),
            &g_kuiDequantCoeff[uiQp as usize],
        );
        idct_t4_rec_to_view(
            &view.plane(0).cursor(lx + dx, ly + dy),
            &pMbCache.sMemPredBlk4[kiPredOff..],
            4,
            blk4x4(&pMbCache.sCoeffLevel, 0),
        );
    } else {
        copy_block_to_view::<4, 4>(
            &pMbCache.sMemPredBlk4[kiPredOff..kiPredOff + 16],
            &view.plane(0).cursor(lx + dx, ly + dy),
        );
    }
}

/// Quantization, coefficient zigzag scanning, JVT-O079 fast zero-residual thresholding,
/// dequantization and CBP assignment for inter luma (P/B frames).
pub fn WelsEncInterY(pFuncList: &SWelsFuncPtrList, pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    let pfQuantizationFour4x4Max = pFuncList.pfQuantizationFour4x4Max;
    let pfScan4x4 = pFuncList.pfScan4x4;
    let pfCalculateSingleCtr4x4 = pFuncList.pfCalculateSingleCtr4x4;
    let pfGetNoneZeroCount = pFuncList.pfGetNoneZeroCount;
    let pfDequantizationFour4x4 = pFuncList.pfDequantizationFour4x4;

    let mut iSingleCtrMb = 0i32;
    let mut iSingleCtr8x8 = [0i32; 4];
    let uiQp = pCurMb.uiLumaQp;
    let pMF = &g_kiQuantMF[uiQp as usize];
    let pFF = &g_kiQuantInterFF[uiQp as usize];
    let mut aMax = [0i16; 16];

    for i in 0..4 {
        let func = pfQuantizationFour4x4Max;
        let max4: &mut [i16; 4] = (&mut aMax[i << 2..(i << 2) + 4]).try_into().expect("4");
        func(
            blk_four4x4_mut(&mut pMbCache.sCoeffLevel, i << 6),
            pFF,
            pMF,
            max4,
        );
        iSingleCtr8x8[i] = 0;
        for j in 0..4 {
            let k = (i << 2) + j;
            let max_val = aMax[k];
            if max_val == 0 {
                pMbCache.sDct.iLumaBlock[k].fill(0);
            } else {
                let func = pfScan4x4;
                func(
                    &mut pMbCache.sDct.iLumaBlock[k],
                    blk4x4(&pMbCache.sCoeffLevel, k << 4),
                );
                if max_val > 1 {
                    iSingleCtr8x8[i] += 9;
                } else if iSingleCtr8x8[i] < 6 {
                    let func = pfCalculateSingleCtr4x4;
                    iSingleCtr8x8[i] += func(&pMbCache.sDct.iLumaBlock[k]);
                }
            }
        }
        iSingleCtrMb += iSingleCtr8x8[i];
    }

    // The 16 luma entries only.
    (&mut pCurMb.iNonZeroCount)[0..16].fill(0);

    if iSingleCtrMb < 6 {
        // JVT-O079 zero-residual early cutoff: all 384 coefficients.
        pMbCache.sCoeffLevel.fill(0);
    } else {
        let mut kpNoneZeroCountIdx = 0usize;
        for i in 0..4 {
            if iSingleCtr8x8[i] >= 4 {
                for j in 0..4 {
                    let iNoneZeroCount =
                        pfGetNoneZeroCount(&pMbCache.sDct.iLumaBlock[(i << 2) + j]);
                    let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
                    kpNoneZeroCountIdx += 1;
                    pCurMb.iNonZeroCount[offset] = iNoneZeroCount as i8;
                }
                let func = pfDequantizationFour4x4;
                func(
                    blk_four4x4_mut(&mut pMbCache.sCoeffLevel, i << 6),
                    &g_kuiDequantCoeff[uiQp as usize],
                );
                pCurMb.uiCbp |= 1 << i;
            } else {
                pMbCache.sCoeffLevel[i << 6..(i << 6) + 64].fill(0);
                kpNoneZeroCountIdx += 4;
            }
        }
    }
}

/// 2x2 chroma DC Hadamard transform, 4x4 AC quantization, JVT-O079 thresholding and
/// inverse dequantization for one chroma plane (`iUV = 1` for Cb, `iUV = 2` for Cr).
///
/// `kiResOff` is the caller's base into `sCoeffLevel`, not a function of `iUV`:
/// `WelsIMbChromaEncode` passes 0 and `WelsPMbChromaEncode` passes 256.
///
/// # Panics
/// If `kiResOff .. kiResOff + 64` is out of bounds of `pMbCache.sCoeffLevel`; one
/// chroma group is 64 coefficients.
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

    let kiInterFlag = !IS_INTRA(pCurMb.uiMbType);
    let kiQp = pCurMb.uiChromaQp;
    let uiNoneZeroCountOffset = ((iUV - 1) << 1) as usize;
    let uiSubMbIdx = (16 + ((iUV - 1) << 2)) as usize;
    // `iChromaBlock` is `[[i16; 16]; 8]`, so this plane's four blocks are
    // `[kiChromaBlk ..][0..4]`.
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
        hadamard2x2_span_mut(&mut pMbCache.sCoeffLevel, kiResOff),
        pFF[0] << 1,
        pMF[0] >> 1,
        &mut aDct2x2,
        &mut pMbCache.sDct.iChromaDc[(iUV - 1) as usize],
    );

    let func = pfQuantizationFour4x4Max;
    func(
        blk_four4x4_mut(&mut pMbCache.sCoeffLevel, kiResOff),
        pFF,
        pMF,
        &mut aMax,
    );

    for j in 0..4 {
        let k = kiChromaBlk + j;
        if aMax[j] == 0 {
            pMbCache.sDct.iChromaBlock[k].fill(0);
        } else {
            let func = pfScan4x4Ac;
            func(
                &mut pMbCache.sDct.iChromaBlock[k],
                blk4x4(&pMbCache.sCoeffLevel, kiResOff + (j << 4)),
            );
            if kiInterFlag {
                if aMax[j] > 1 {
                    iSingleCtr8x8 += 9;
                } else if iSingleCtr8x8 < 7 {
                    let func = pfCalculateSingleCtr4x4;
                    iSingleCtr8x8 += func(&pMbCache.sDct.iChromaBlock[k]);
                }
            } else {
                iSingleCtr8x8 = i32::MAX;
            }
        }
    }

    if iSingleCtr8x8 < 7 {
        pMbCache.sCoeffLevel[kiResOff..kiResOff + 64].fill(0);
        pCurMb.iNonZeroCount[16 + uiNoneZeroCountOffset] = 0;
        pCurMb.iNonZeroCount[16 + uiNoneZeroCountOffset + 1] = 0;
        pCurMb.iNonZeroCount[20 + uiNoneZeroCountOffset] = 0;
        pCurMb.iNonZeroCount[20 + uiNoneZeroCountOffset + 1] = 0;
    } else {
        let mut kpNoneZeroCountIdx = uiSubMbIdx;
        for j in 0..4 {
            let uiNoneZeroCount = pfGetNoneZeroCount(&pMbCache.sDct.iChromaBlock[kiChromaBlk + j]);
            let offset = g_kuiMbCountScan4Idx[kpNoneZeroCountIdx] as usize;
            kpNoneZeroCountIdx += 1;
            pCurMb.iNonZeroCount[offset] = uiNoneZeroCount as i8;
        }
        let func = pfDequantizationFour4x4;
        func(
            blk_four4x4_mut(&mut pMbCache.sCoeffLevel, kiResOff),
            &g_kuiDequantCoeff[pCurMb.uiChromaQp as usize],
        );
        pCurMb.uiCbp &= 0x0F;
        pCurMb.uiCbp |= 0x20;
    }

    if uiNoneZeroCountMbDc > 0 {
        WelsDequantIHadamard2x2Dc(&mut aDct2x2, g_kuiDequantCoeff[kiQp as usize][0]);
        if 2 != (pCurMb.uiCbp >> 4) {
            pCurMb.uiCbp |= 0x01 << 4;
        }
        for (k, &v) in aDct2x2.iter().enumerate() {
            pMbCache.sCoeffLevel[kiResOff + (k << 4)] = v;
        }
    }
}

/// Whether the luma residual qualifies for `P_SKIP`: `true` when it is zero or
/// negligible (`iSingleCtrMb < 6`).
pub fn WelsTryPYskip(pEncCtx: &sWelsEncCtx, pCurMb: &mut SMB, pMbCache: &mut SMbCache) -> bool {
    let mut iSingleCtrMb = 0i32;
    let kuiQp = pCurMb.uiLumaQp;
    let mut aMax = [0i16; 4];
    let pMF = &g_kiQuantMF[kuiQp as usize];
    let pFF = &g_kiQuantInterFF[kuiQp as usize];

    for i in 0..4 {
        (pEncCtx.func_list().pfQuantizationFour4x4Max)(
            blk_four4x4_mut(&mut pMbCache.sCoeffLevel, i << 6),
            pFF,
            pMF,
            &mut aMax,
        );

        for j in 0..4 {
            let k = (i << 2) + j;
            if aMax[j] > 1 {
                return false;
            } else if aMax[j] == 1 {
                (pEncCtx.func_list().pfScan4x4)(
                    &mut pMbCache.sDct.iLumaBlock[k],
                    blk4x4(&pMbCache.sCoeffLevel, k << 4),
                );
                let func = pEncCtx.func_list().pfCalculateSingleCtr4x4;
                iSingleCtrMb += func(&pMbCache.sDct.iLumaBlock[k]);
            }
            if iSingleCtrMb >= 6 {
                return false;
            }
        }
    }
    true
}

/// Whether the chroma residual for plane `iUV` qualifies for `P_SKIP`: `true` when both
/// the DC and the significant AC are zero.
pub fn WelsTryPUVskip(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    iUV: i32,
) -> bool {
    let kiResOff = if iUV == 1 { 256usize } else { 256 + 64 };

    let chroma_qp_index_offset =
        if let Some(pps) = layer_pps_ref(pEncCtx, current_layer_expect(pEncCtx)) {
            pps.uiChromaQpIndexOffset as i32
        } else {
            0
        };
    let clipped_qp = (pCurMb.uiLumaQp as i32 + chroma_qp_index_offset).clamp(0, 51);
    let kuiQp = g_kuiChromaQpTable[clipped_qp as usize];

    let pMF = &g_kiQuantMF[kuiQp as usize];
    let pFF = &g_kiQuantInterFF[kuiQp as usize];

    let hadamard_skip = (pEncCtx.func_list().pfQuantizationHadamard2x2Skip)(
        hadamard2x2_span(&pMbCache.sCoeffLevel, kiResOff),
        pFF[0] << 1,
        pMF[0] >> 1,
    ) != 0;

    if hadamard_skip {
        false
    } else {
        let mut aMax = [0i16; 4];
        let mut iSingleCtrMb = 0i32;
        let kiChromaBlk = ((iUV - 1) << 2) as usize;

        (pEncCtx.func_list().pfQuantizationFour4x4Max)(
            blk_four4x4_mut(&mut pMbCache.sCoeffLevel, kiResOff),
            pFF,
            pMF,
            &mut aMax,
        );

        for j in 0..4 {
            let k = kiChromaBlk + j;
            if aMax[j] > 1 {
                return false;
            } else if aMax[j] == 1 {
                (pEncCtx.func_list().pfScan4x4Ac)(
                    &mut pMbCache.sDct.iChromaBlock[k],
                    blk4x4(&pMbCache.sCoeffLevel, kiResOff + (j << 4)),
                );
                let func = pEncCtx.func_list().pfCalculateSingleCtr4x4;
                iSingleCtrMb += func(&pMbCache.sDct.iChromaBlock[k]);
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
        // A DC impulse spreads evenly across all 16 cells.
        for val in dc_buf.iter() {
            assert_eq!(*val, 16);
        }
    }

    #[test]
    fn test_dequant_ihadamard_2x2_dc() {
        let mut dct2x2 = [2i16, 0, 0, 0];
        let mf: u16 = 10;
        WelsDequantIHadamard2x2Dc(&mut dct2x2, mf);
        // ((2 +- 0) * 10) >> 1 = 10 in every cell.
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
