// Copyright (c) 2009-2014, Cisco Systems
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

//! # OpenH264 Decoder: Error Concealment Engine
//!
//! Translated from `codec/decoder/core/inc/error_concealment.h` and
//! `codec/decoder/core/src/error_concealment.cpp`.
//!
//! Provides spatial and temporal error concealment algorithms (full frame copy,
//! selective collocated slice macroblock copy, and motion-compensated vector extrapolation)
//! to restore video continuity and decodability during network packet loss.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ptr;

// ============================================================================
// Constants and Error Concealment Modes
// ============================================================================

/// Error concealment method selector enumeration (`ERROR_CON_IDC`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ERROR_CON_IDC {
    #[default]
    ERROR_CON_DISABLE = 0,
    ERROR_CON_FRAME_COPY = 1,
    ERROR_CON_SLICE_COPY = 2,
    ERROR_CON_FRAME_COPY_CROSS_IDR = 3,
    ERROR_CON_SLICE_COPY_CROSS_IDR = 4,
    ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE = 5,
    ERROR_CON_SLICE_MV_COPY_CROSS_IDR = 6,
    ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE = 7,
}

// Error status bitmask flags
pub const ERR_NONE: i32 = 0;
pub const dsRefLost: i32 = 0x02;
pub const dsBitstreamError: i32 = 0x04;
pub const dsDataErrorConcealed: i32 = 0x20;

// CPU Feature Flags
pub const WELS_CPU_MMXEXT: u32 = 0x00000002;
pub const WELS_CPU_SSE2: u32 = 0x00000004;
pub const WELS_CPU_NEON: u32 = 0x00000008;
pub const WELS_CPU_LSX: u32 = 0x00000010;

// Reference picture list index
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const MAX_REF_PIC_COUNT: usize = 16;

// Macroblock type flags
pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_INTRA16x16: u32 = 0x00000002;
pub const MB_TYPE_INTRA8x8: u32 = 0x00000004;
pub const MB_TYPE_16x16: u32 = 0x00000008;
pub const MB_TYPE_16x8: u32 = 0x00000010;
pub const MB_TYPE_8x16: u32 = 0x00000020;
pub const MB_TYPE_8x8: u32 = 0x00000040;
pub const MB_TYPE_8x8_REF0: u32 = 0x00000080;
pub const MB_TYPE_SKIP: u32 = 0x00000100;
pub const MB_TYPE_INTRA_PCM: u32 = 0x00000200;
pub const MB_TYPE_INTRA_BL: u32 = 0x00000400;
pub const MB_TYPE_DIRECT: u32 = 0x00000800;

pub const MB_TYPE_INTER: u32 = MB_TYPE_16x16
    | MB_TYPE_16x8
    | MB_TYPE_8x16
    | MB_TYPE_8x8
    | MB_TYPE_8x8_REF0
    | MB_TYPE_SKIP
    | MB_TYPE_DIRECT;

// Sub-Macroblock types
pub const SUB_MB_TYPE_8x8: u32 = 0x00000001;
pub const SUB_MB_TYPE_8x4: u32 = 0x00000002;
pub const SUB_MB_TYPE_4x8: u32 = 0x00000004;
pub const SUB_MB_TYPE_4x4: u32 = 0x00000008;

// Helper Macros
#[inline]
pub fn IS_INTER(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTER) != 0
}

#[inline]
pub fn WELS_MAX<T: Ord>(x: T, y: T) -> T {
    std::cmp::max(x, y)
}

#[inline]
pub fn WELS_MIN<T: Ord>(x: T, y: T) -> T {
    std::cmp::min(x, y)
}

// ============================================================================
// Function Pointer Types & Helper Structs
// ============================================================================

pub type PCopyLumaFunc = Option<unsafe extern "C" fn(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32)>;
pub type PCopyChromaFunc = Option<unsafe extern "C" fn(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32)>;
pub type PExpandPictureFunc = unsafe extern "C" fn(pDst: *mut u8, iLinesize: i32, iPicWidth: i32, iPicHeight: i32);

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SCopyFunc {
    pub pCopyLumaFunc: PCopyLumaFunc,
    pub pCopyChromaFunc: PCopyChromaFunc,
}

impl Default for SCopyFunc {
    fn default() -> Self {
        Self {
            pCopyLumaFunc: Some(WelsCopy16x16_c),
            pCopyChromaFunc: Some(WelsCopy8x8_c),
        }
    }
}

pub use crate::decoder::decoder_core::SExpandPicFunc;

pub use crate::common::mc::SMcFunc;

/// Motion compensation reference frame descriptor (`sMCRefMember`).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagMCRefMember {
    pub pDstY: *mut u8,
    pub pDstU: *mut u8,
    pub pDstV: *mut u8,

    pub pSrcY: *mut u8,
    pub pSrcU: *mut u8,
    pub pSrcV: *mut u8,

    pub iSrcLineLuma: i32,
    pub iSrcLineChroma: i32,

    pub iDstLineLuma: i32,
    pub iDstLineChroma: i32,

    pub iPicWidth: i32,
    pub iPicHeight: i32,
}

pub type sMCRefMember = TagMCRefMember;

impl Default for TagMCRefMember {
    fn default() -> Self {
        Self {
            pDstY: ptr::null_mut(),
            pDstU: ptr::null_mut(),
            pDstV: ptr::null_mut(),
            pSrcY: ptr::null_mut(),
            pSrcU: ptr::null_mut(),
            pSrcV: ptr::null_mut(),
            iSrcLineLuma: 0,
            iSrcLineChroma: 0,
            iDstLineLuma: 0,
            iDstLineChroma: 0,
            iPicWidth: 0,
            iPicHeight: 0,
        }
    }
}

// ============================================================================
// Core Decoder Context Structs
// ============================================================================

pub use crate::decoder::decoder_context::{Picture, SPicture, PPicture, SDecodingParam};



pub use crate::decoder::parameter_sets::{SSps, SPosOffset as SFrameCrop};
pub use crate::decoder::decoder_core::{SDqLayer, PDqLayer, SLayerInfo};
pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext, SRefPic};


// ============================================================================
// Memory Block Copy Functions (C Reference & SIMD Implementations)
// ============================================================================

/// C reference fallback for 16x16 macroblock luma copying.
#[inline]
pub unsafe extern "C" fn WelsCopy16x16_c(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    let mut dst = pDst;
    let mut src = pSrc;
    for _ in 0..16 {
        ptr::copy_nonoverlapping(src, dst, 16);
        dst = dst.offset(iDstStride as isize);
        src = src.offset(iSrcStride as isize);
    }
}

/// C reference fallback for 8x8 chroma block copying.
#[inline]
pub unsafe extern "C" fn WelsCopy8x8_c(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    let mut dst = pDst;
    let mut src = pSrc;
    for _ in 0..8 {
        ptr::copy_nonoverlapping(src, dst, 8);
        dst = dst.offset(iDstStride as isize);
        src = src.offset(iSrcStride as isize);
    }
}

// SIMD Aliases and Accelerated Fallbacks
pub unsafe extern "C" fn WelsCopy16x16_sse2(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy16x16_c(pDst, iDstStride, pSrc, iSrcStride);
}

pub unsafe extern "C" fn WelsCopy8x8_mmx(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy8x8_c(pDst, iDstStride, pSrc, iSrcStride);
}

pub unsafe extern "C" fn WelsCopy16x16_neon(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy16x16_c(pDst, iDstStride, pSrc, iSrcStride);
}

pub unsafe extern "C" fn WelsCopy8x8_neon(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy8x8_c(pDst, iDstStride, pSrc, iSrcStride);
}

pub unsafe extern "C" fn WelsCopy16x16_AArch64_neon(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy16x16_c(pDst, iDstStride, pSrc, iSrcStride);
}

pub unsafe extern "C" fn WelsCopy8x8_AArch64_neon(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy8x8_c(pDst, iDstStride, pSrc, iSrcStride);
}

pub unsafe extern "C" fn WelsCopy16x16_lsx(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy16x16_c(pDst, iDstStride, pSrc, iSrcStride);
}

pub unsafe extern "C" fn WelsCopy8x8_lsx(pDst: *mut u8, iDstStride: i32, pSrc: *mut u8, iSrcStride: i32) {
    WelsCopy8x8_c(pDst, iDstStride, pSrc, iSrcStride);
}

// ============================================================================
// Core Error Concealment Functions
// ============================================================================

/// Initializes error concealment function pointer dispatch table and resets freeze output flag.
pub unsafe extern "C" fn InitErrorCon(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pParam.is_null() {
        return;
    }

    let ec_mode = (*(*pCtx).pParam).eEcActiveIdc;
    if ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
    {
        if ec_mode != ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE
            && ec_mode != ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
        {
            (*pCtx).bFreezeOutput = false;
        }

        (*pCtx).sCopyFunc.pCopyLumaFunc = Some(WelsCopy16x16_c);
        (*pCtx).sCopyFunc.pCopyChromaFunc = Some(WelsCopy8x8_c);

        let cpu_flag = (*pCtx).uiCpuFlag;

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if (cpu_flag & WELS_CPU_MMXEXT) != 0 {
                (*pCtx).sCopyFunc.pCopyChromaFunc = Some(WelsCopy8x8_mmx);
            }
            if (cpu_flag & WELS_CPU_SSE2) != 0 {
                (*pCtx).sCopyFunc.pCopyLumaFunc = Some(WelsCopy16x16_sse2);
            }
        }

        #[cfg(target_arch = "arm")]
        {
            if (cpu_flag & WELS_CPU_NEON) != 0 {
                (*pCtx).sCopyFunc.pCopyLumaFunc = Some(WelsCopy16x16_neon);
                (*pCtx).sCopyFunc.pCopyChromaFunc = Some(WelsCopy8x8_neon);
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            if (cpu_flag & WELS_CPU_NEON) != 0 {
                (*pCtx).sCopyFunc.pCopyLumaFunc = Some(WelsCopy16x16_AArch64_neon);
                (*pCtx).sCopyFunc.pCopyChromaFunc = Some(WelsCopy8x8_AArch64_neon);
            }
        }

        if (cpu_flag & WELS_CPU_LSX) != 0 {
            (*pCtx).sCopyFunc.pCopyChromaFunc = Some(WelsCopy8x8_lsx);
            (*pCtx).sCopyFunc.pCopyLumaFunc = Some(WelsCopy16x16_lsx);
        }
    }
}

/// Evaluates if error concealment is required by inspecting the macroblock decoding flags.
pub unsafe extern "C" fn NeedErrorCon(pCtx: PWelsDecoderContext) -> bool {
    if pCtx.is_null() || (*pCtx).pSps.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return false;
    }

    let iMbNum = (*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight;
    let pMbFlag = (*(*pCtx).pCurDqLayer).pMbCorrectlyDecodedFlag;
    if pMbFlag.is_null() {
        return false;
    }

    for i in 0..iMbNum {
        if !*pMbFlag.add(i as usize) {
            return true;
        }
    }
    false
}

/// Performs full-frame error concealment by copying pixel planes from the previous reference picture.
pub unsafe extern "C" fn DoErrorConFrameCopy(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pDec.is_null() || (*pCtx).pSps.is_null() {
        return;
    }

    let pDstPic = (*pCtx).pDec;
    let mut pSrcPic = if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb
    } else {
        ptr::null_mut()
    };

    let uiHeightInPixelY = ((*(*pCtx).pSps).iMbHeight as u32) << 4;
    let iStrideY = (*pDstPic).iLinesize[0];
    let iStrideUV = (*pDstPic).iLinesize[1];
    (*pDstPic).iMbEcedNum = ((*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight) as i32;

    if !(*pCtx).pParam.is_null() && !(*pCtx).pCurDqLayer.is_null() {
        if (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_FRAME_COPY
            && (*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt.bIdrFlag
        {
            pSrcPic = ptr::null_mut();
        }
    }

    if pSrcPic.is_null() {
        // Fill planes with neutral gray (128)
        if !(*pDstPic).pData[0].is_null() {
            ptr::write_bytes((*pDstPic).pData[0], 128, (uiHeightInPixelY as usize) * (iStrideY as usize));
        }
        if !(*pDstPic).pData[1].is_null() {
            ptr::write_bytes(
                (*pDstPic).pData[1],
                128,
                ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize),
            );
        }
        if !(*pDstPic).pData[2].is_null() {
            ptr::write_bytes(
                (*pDstPic).pData[2],
                128,
                ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize),
            );
        }
    } else if pSrcPic == pDstPic {
        // Prevent self-copy overlap
    } else {
        if !(*pDstPic).pData[0].is_null() && !(*pSrcPic).pData[0].is_null() {
            ptr::copy_nonoverlapping(
                (*pSrcPic).pData[0],
                (*pDstPic).pData[0],
                (uiHeightInPixelY as usize) * (iStrideY as usize),
            );
        }
        if !(*pDstPic).pData[1].is_null() && !(*pSrcPic).pData[1].is_null() {
            ptr::copy_nonoverlapping(
                (*pSrcPic).pData[1],
                (*pDstPic).pData[1],
                ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize),
            );
        }
        if !(*pDstPic).pData[2].is_null() && !(*pSrcPic).pData[2].is_null() {
            ptr::copy_nonoverlapping(
                (*pSrcPic).pData[2],
                (*pDstPic).pData[2],
                ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize),
            );
        }
    }
}

/// Performs macroblock-level error concealment by copying collocated undamaged macroblocks.
pub unsafe extern "C" fn DoErrorConSliceCopy(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pSps.is_null() || (*pCtx).pDec.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return;
    }

    let iMbWidth = (*(*pCtx).pSps).iMbWidth as usize;
    let iMbHeight = (*(*pCtx).pSps).iMbHeight as usize;
    let pDstPic = (*pCtx).pDec;
    let mut pSrcPic = if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb
    } else {
        ptr::null_mut()
    };

    if !(*pCtx).pParam.is_null() {
        if (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_SLICE_COPY
            && (*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt.bIdrFlag
        {
            pSrcPic = ptr::null_mut();
        }
    }

    let pMbCorrectlyDecodedFlag = (*(*pCtx).pCurDqLayer).pMbCorrectlyDecodedFlag;
    if pMbCorrectlyDecodedFlag.is_null() {
        return;
    }

    let iDstStride = (*pDstPic).iLinesize[0] as usize;

    if !pSrcPic.is_null() && pSrcPic == pDstPic {
        return;
    }

    for iMbY in 0..iMbHeight {
        for iMbX in 0..iMbWidth {
            let iMbXyIndex = iMbY * iMbWidth + iMbX;
            if !*pMbCorrectlyDecodedFlag.add(iMbXyIndex) {
                (*pDstPic).iMbEcedNum += 1;
                if !pSrcPic.is_null() {
                    let iSrcStride = (*pSrcPic).iLinesize[0] as usize;

                    // Y Component
                    let pDstData = (*pDstPic).pData[0].add(iMbY * 16 * iDstStride + iMbX * 16);
                    let pSrcData = (*pSrcPic).pData[0].add(iMbY * 16 * iSrcStride + iMbX * 16);
                    if let Some(f) = (*pCtx).sCopyFunc.pCopyLumaFunc {
                        f(pDstData, iDstStride as i32, pSrcData, iSrcStride as i32);
                    }

                    // U Component
                    let pDstDataU = (*pDstPic).pData[1].add(iMbY * 8 * (iDstStride / 2) + iMbX * 8);
                    let pSrcDataU = (*pSrcPic).pData[1].add(iMbY * 8 * (iSrcStride / 2) + iMbX * 8);
                    if let Some(f) = (*pCtx).sCopyFunc.pCopyChromaFunc {
                        f(
                            pDstDataU,
                            (iDstStride / 2) as i32,
                            pSrcDataU,
                            (iSrcStride / 2) as i32,
                        );
                    }

                    // V Component
                    let pDstDataV = (*pDstPic).pData[2].add(iMbY * 8 * (iDstStride / 2) + iMbX * 8);
                    let pSrcDataV = (*pSrcPic).pData[2].add(iMbY * 8 * (iSrcStride / 2) + iMbX * 8);
                    if let Some(f) = (*pCtx).sCopyFunc.pCopyChromaFunc {
                        f(
                            pDstDataV,
                            (iDstStride / 2) as i32,
                            pSrcDataV,
                            (iSrcStride / 2) as i32,
                        );
                    }
                } else {
                    // Fill lost MB with neutral gray (128)
                    let mut pDstData = (*pDstPic).pData[0].add(iMbY * 16 * iDstStride + iMbX * 16);
                    for _ in 0..16 {
                        ptr::write_bytes(pDstData, 128, 16);
                        pDstData = pDstData.add(iDstStride);
                    }

                    let mut pDstDataU = (*pDstPic).pData[1].add(iMbY * 8 * (iDstStride / 2) + iMbX * 8);
                    for _ in 0..8 {
                        ptr::write_bytes(pDstDataU, 128, 8);
                        pDstDataU = pDstDataU.add(iDstStride / 2);
                    }

                    let mut pDstDataV = (*pDstPic).pData[2].add(iMbY * 8 * (iDstStride / 2) + iMbX * 8);
                    for _ in 0..8 {
                        ptr::write_bytes(pDstDataV, 128, 8);
                        pDstDataV = pDstDataV.add(iDstStride / 2);
                    }
                }
            }
        }
    }
}

/// Fallback motion compensation handler for macroblock reconstruction.
#[inline]
pub unsafe extern "C" fn BaseMC(
    pCtx: PWelsDecoderContext,
    pMCRefMem: *mut sMCRefMember,
    _listIdx: i32,
    _iRefIdx: i8,
    iXOffset: i32,
    iYOffset: i32,
    _pMCFunc: *mut SMcFunc,
    _iBlkWidth: i32,
    _iBlkHeight: i32,
    iMVs: *mut [i16; 2],
) {
    if pMCRefMem.is_null() || iMVs.is_null() {
        return;
    }

    let mv = *iMVs;
    let iFullMVx = (iXOffset << 2) + (mv[0] as i32);
    let iFullMVy = (iYOffset << 2) + (mv[1] as i32);

    let iSrcPixOffsetLuma = (iFullMVx >> 2) + (iFullMVy >> 2) * (*pMCRefMem).iSrcLineLuma;
    let iSrcPixOffsetChroma = (iFullMVx >> 3) + (iFullMVy >> 3) * (*pMCRefMem).iSrcLineChroma;

    if !(*pMCRefMem).pDstY.is_null() && !(*pMCRefMem).pSrcY.is_null() {
        let pSrc = (*pMCRefMem).pSrcY.offset(iSrcPixOffsetLuma as isize);
        let pDst = (*pMCRefMem).pDstY;
        if let Some(f) = (*pCtx).sCopyFunc.pCopyLumaFunc {
            f(pDst, (*pMCRefMem).iDstLineLuma, pSrc, (*pMCRefMem).iSrcLineLuma);
        }
    }
    if !(*pMCRefMem).pDstU.is_null() && !(*pMCRefMem).pSrcU.is_null() {
        let pSrc = (*pMCRefMem).pSrcU.offset(iSrcPixOffsetChroma as isize);
        let pDst = (*pMCRefMem).pDstU;
        if let Some(f) = (*pCtx).sCopyFunc.pCopyChromaFunc {
            f(pDst, (*pMCRefMem).iDstLineChroma, pSrc, (*pMCRefMem).iSrcLineChroma);
        }
    }
    if !(*pMCRefMem).pDstV.is_null() && !(*pMCRefMem).pSrcV.is_null() {
        let pSrc = (*pMCRefMem).pSrcV.offset(iSrcPixOffsetChroma as isize);
        let pDst = (*pMCRefMem).pDstV;
        if let Some(f) = (*pCtx).sCopyFunc.pCopyChromaFunc {
            f(pDst, (*pMCRefMem).iDstLineChroma, pSrc, (*pMCRefMem).iSrcLineChroma);
        }
    }
}

/// Applies motion-compensated error concealment for a single lost macroblock.
pub unsafe extern "C" fn DoMbECMvCopy(
    pCtx: PWelsDecoderContext,
    pDec: PPicture,
    pRef: PPicture,
    _iMbXy: i32,
    iMbX: i32,
    iMbY: i32,
    pMCRefMem: *mut sMCRefMember,
) {
    if pDec == pRef || pDec.is_null() || pRef.is_null() || pMCRefMem.is_null() || pCtx.is_null() {
        return;
    }

    let mut iMVs = [0i16; 2];
    let iMbXInPix = iMbX << 4;
    let iMbYInPix = iMbY << 4;
    let iCurrPoc = (*pDec).iFramePoc;

    let pDst0 = (*pDec).pData[0].add((iMbXInPix + iMbYInPix * (*pMCRefMem).iDstLineLuma) as usize);
    let pDst1 = (*pDec).pData[1].add(((iMbXInPix >> 1) + (iMbYInPix >> 1) * (*pMCRefMem).iDstLineChroma) as usize);
    let pDst2 = (*pDec).pData[2].add(((iMbXInPix >> 1) + (iMbYInPix >> 1) * (*pMCRefMem).iDstLineChroma) as usize);

    if (*pDec).bIdrFlag || (*pCtx).pECRefPic[0].is_null() {
        let pSrcY = (*pMCRefMem).pSrcY.add((iMbY * 16 * (*pMCRefMem).iSrcLineLuma + iMbX * 16) as usize);
        if let Some(f) = (*pCtx).sCopyFunc.pCopyLumaFunc {
            f(pDst0, (*pMCRefMem).iDstLineLuma, pSrcY, (*pMCRefMem).iSrcLineLuma);
        }

        let pSrcU = (*pMCRefMem).pSrcU.add((iMbY * 8 * (*pMCRefMem).iSrcLineChroma + iMbX * 8) as usize);
        if let Some(f) = (*pCtx).sCopyFunc.pCopyChromaFunc {
            f(pDst1, (*pMCRefMem).iDstLineChroma, pSrcU, (*pMCRefMem).iSrcLineChroma);
        }

        let pSrcV = (*pMCRefMem).pSrcV.add((iMbY * 8 * (*pMCRefMem).iSrcLineChroma + iMbX * 8) as usize);
        if let Some(f) = (*pCtx).sCopyFunc.pCopyChromaFunc {
            f(pDst2, (*pMCRefMem).iDstLineChroma, pSrcV, (*pMCRefMem).iSrcLineChroma);
        }
        return;
    }

    if !(*pCtx).pECRefPic[0].is_null() {
        if (*pCtx).pECRefPic[0] == pRef {
            iMVs[0] = (*pCtx).iECMVs[0][0] as i16;
            iMVs[1] = (*pCtx).iECMVs[0][1] as i16;
        } else {
            let iScale0 = (*(*pCtx).pECRefPic[0]).iFramePoc - iCurrPoc;
            let iScale1 = (*pRef).iFramePoc - iCurrPoc;
            iMVs[0] = if iScale0 == 0 {
                0
            } else {
                ((*pCtx).iECMVs[0][0] * iScale1 / iScale0) as i16
            };
            iMVs[1] = if iScale0 == 0 {
                0
            } else {
                ((*pCtx).iECMVs[0][1] * iScale1 / iScale0) as i16
            };
        }

        (*pMCRefMem).pDstY = pDst0;
        (*pMCRefMem).pDstU = pDst1;
        (*pMCRefMem).pDstV = pDst2;

        let mut iFullMVx = (iMbXInPix << 2) + (iMVs[0] as i32);
        let mut iFullMVy = (iMbYInPix << 2) + (iMVs[1] as i32);

        let mut iPicWidthLeftLimit = 0;
        let mut iPicHeightTopLimit = 0;
        let mut iPicWidthRightLimit = (*pMCRefMem).iPicWidth;
        let mut iPicHeightBottomLimit = (*pMCRefMem).iPicHeight;

        if !(*pCtx).pSps.is_null() && (*(*pCtx).pSps).bFrameCroppingFlag {
            iPicWidthLeftLimit = (*pCtx).sFrameCrop.iLeftOffset * 2;
            iPicWidthRightLimit = (*pMCRefMem).iPicWidth - (*pCtx).sFrameCrop.iRightOffset * 2;
            iPicHeightTopLimit = (*pCtx).sFrameCrop.iTopOffset * 2;
            iPicHeightBottomLimit = (*pMCRefMem).iPicHeight - (*pCtx).sFrameCrop.iTopOffset * 2;
        }

        let iMinLeftOffset = (iPicWidthLeftLimit + 2) * 4;
        let iMaxRightOffset = (iPicWidthRightLimit - 18) * 4;
        let iMinTopOffset = (iPicHeightTopLimit + 2) * 4;
        let iMaxBottomOffset = (iPicHeightBottomLimit - 18) * 4;

        if iFullMVx < iMinLeftOffset {
            iFullMVx = (iFullMVx >> 2) * 4;
            iFullMVx = WELS_MAX(iPicWidthLeftLimit, iFullMVx);
        } else if iFullMVx > iMaxRightOffset {
            iFullMVx = (iFullMVx >> 2) * 4;
            iFullMVx = WELS_MIN((iPicWidthRightLimit - 16) * 4, iFullMVx);
        }

        if iFullMVy < iMinTopOffset {
            iFullMVy = (iFullMVy >> 2) * 4;
            iFullMVy = WELS_MAX(iPicHeightTopLimit, iFullMVy);
        } else if iFullMVy > iMaxBottomOffset {
            iFullMVy = (iFullMVy >> 2) * 4;
            iFullMVy = WELS_MIN((iPicHeightBottomLimit - 16) * 4, iFullMVy);
        }

        iMVs[0] = (iFullMVx - (iMbXInPix << 2)) as i16;
        iMVs[1] = (iFullMVy - (iMbYInPix << 2)) as i16;

        BaseMC(
            pCtx,
            pMCRefMem,
            -1,
            -1,
            iMbXInPix,
            iMbYInPix,
            &mut (*pCtx).sMcFunc,
            16,
            16,
            &mut iMVs,
        );
    }
}

/// Gathers motion vector statistics from correctly decoded macroblocks in the current picture.
pub unsafe extern "C" fn GetAvilInfoFromCorrectMb(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pSps.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return;
    }

    let iMbWidth = (*(*pCtx).pSps).iMbWidth;
    let iMbHeight = (*(*pCtx).pSps).iMbHeight;
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let pMbCorrectlyDecodedFlag = (*pCurDqLayer).pMbCorrectlyDecodedFlag;
    let pDec = (*pCurDqLayer).pDec;

    if pMbCorrectlyDecodedFlag.is_null() || pDec.is_null() {
        return;
    }

    let mut iInterMbCorrectNum = [0i32; 16];

    for r in 0..16 {
        (*pCtx).iECMVs[r][0] = 0;
        (*pCtx).iECMVs[r][1] = 0;
        (*pCtx).pECRefPic[r] = ptr::null_mut();
    }

    for iMbY in 0..iMbHeight {
        for iMbX in 0..iMbWidth {
            let iMbXyIndex = (iMbY * iMbWidth + iMbX) as usize;
            if *pMbCorrectlyDecodedFlag.add(iMbXyIndex) && !(*pDec).pMbType.is_null() {
                let iMBType = *(*pDec).pMbType.add(iMbXyIndex);
                if IS_INTER(iMBType) {
                    match iMBType {
                        MB_TYPE_SKIP | MB_TYPE_16x16 => {
                            if !(*pDec).pRefIndex[0].is_null() && !(*pDec).pMv[0].is_null() {
                                let ref_row = *(*pDec).pRefIndex[0].add(iMbXyIndex);
                                let mv_row = *(*pDec).pMv[0].add(iMbXyIndex);
                                let iRefIdx = ref_row[0] as usize;
                                if iRefIdx < 16 {
                                    let mv = mv_row[0];
                                    (*pCtx).iECMVs[iRefIdx][0] += mv[0] as i32;
                                    (*pCtx).iECMVs[iRefIdx][1] += mv[1] as i32;
                                    (*pCtx).pECRefPic[iRefIdx] = (*pCtx).sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                            }
                        }
                        MB_TYPE_16x8 => {
                            if !(*pDec).pRefIndex[0].is_null() && !(*pDec).pMv[0].is_null() {
                                let ref_row = *(*pDec).pRefIndex[0].add(iMbXyIndex);
                                let mv_row = *(*pDec).pMv[0].add(iMbXyIndex);
                                // Partition 0
                                let mut iRefIdx = ref_row[0] as usize;
                                if iRefIdx < 16 {
                                    let mv0 = mv_row[0];
                                    (*pCtx).iECMVs[iRefIdx][0] += mv0[0] as i32;
                                    (*pCtx).iECMVs[iRefIdx][1] += mv0[1] as i32;
                                    (*pCtx).pECRefPic[iRefIdx] = (*pCtx).sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                                // Partition 1
                                iRefIdx = ref_row[8] as usize;
                                if iRefIdx < 16 {
                                    let mv8 = mv_row[8];
                                    (*pCtx).iECMVs[iRefIdx][0] += mv8[0] as i32;
                                    (*pCtx).iECMVs[iRefIdx][1] += mv8[1] as i32;
                                    (*pCtx).pECRefPic[iRefIdx] = (*pCtx).sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                            }
                        }
                        MB_TYPE_8x16 => {
                            if !(*pDec).pRefIndex[0].is_null() && !(*pDec).pMv[0].is_null() {
                                let ref_row = *(*pDec).pRefIndex[0].add(iMbXyIndex);
                                let mv_row = *(*pDec).pMv[0].add(iMbXyIndex);
                                // Partition 0
                                let mut iRefIdx = ref_row[0] as usize;
                                if iRefIdx < 16 {
                                    let mv0 = mv_row[0];
                                    (*pCtx).iECMVs[iRefIdx][0] += mv0[0] as i32;
                                    (*pCtx).iECMVs[iRefIdx][1] += mv0[1] as i32;
                                    (*pCtx).pECRefPic[iRefIdx] = (*pCtx).sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                                // Partition 1
                                iRefIdx = ref_row[2] as usize;
                                if iRefIdx < 16 {
                                    let mv2 = mv_row[2];
                                    (*pCtx).iECMVs[iRefIdx][0] += mv2[0] as i32;
                                    (*pCtx).iECMVs[iRefIdx][1] += mv2[1] as i32;
                                    (*pCtx).pECRefPic[iRefIdx] = (*pCtx).sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                            }
                        }
                        MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
                            if !(*pCurDqLayer).pSubMbType.is_null()
                                && !(*pDec).pRefIndex[0].is_null()
                                && !(*pDec).pMv[0].is_null()
                            {
                                let sub_types = *(*pCurDqLayer).pSubMbType.add(iMbXyIndex);
                                let ref_row = *(*pDec).pRefIndex[0].add(iMbXyIndex);
                                let mv_row = *(*pDec).pMv[0].add(iMbXyIndex);
                                for i in 0..4 {
                                    let iSubMBType = sub_types[i];
                                    let iIIdx = ((i >> 1) << 3) + ((i & 1) << 1);
                                    let iRefIdx = ref_row[iIIdx] as usize;
                                    if iRefIdx < 16 {
                                        (*pCtx).pECRefPic[iRefIdx] = (*pCtx).sRefPic.pRefList[LIST_0][iRefIdx];
                                        match iSubMBType {
                                            SUB_MB_TYPE_8x8 => {
                                                let mv = mv_row[iIIdx];
                                                (*pCtx).iECMVs[iRefIdx][0] += mv[0] as i32;
                                                (*pCtx).iECMVs[iRefIdx][1] += mv[1] as i32;
                                                iInterMbCorrectNum[iRefIdx] += 1;
                                            }
                                            SUB_MB_TYPE_8x4 => {
                                                let mv0 = mv_row[iIIdx];
                                                let mv4 = mv_row[iIIdx + 4];
                                                (*pCtx).iECMVs[iRefIdx][0] += (mv0[0] as i32) + (mv4[0] as i32);
                                                (*pCtx).iECMVs[iRefIdx][1] += (mv0[1] as i32) + (mv4[1] as i32);
                                                iInterMbCorrectNum[iRefIdx] += 2;
                                            }
                                            SUB_MB_TYPE_4x8 => {
                                                let mv0 = mv_row[iIIdx];
                                                let mv1 = mv_row[iIIdx + 1];
                                                (*pCtx).iECMVs[iRefIdx][0] += (mv0[0] as i32) + (mv1[0] as i32);
                                                (*pCtx).iECMVs[iRefIdx][1] += (mv0[1] as i32) + (mv1[1] as i32);
                                                iInterMbCorrectNum[iRefIdx] += 2;
                                            }
                                            SUB_MB_TYPE_4x4 => {
                                                for j in 0..4 {
                                                    let iJIdx = ((j >> 1) << 2) + (j & 1);
                                                    let mv = mv_row[iIIdx + iJIdx];
                                                    (*pCtx).iECMVs[iRefIdx][0] += mv[0] as i32;
                                                    (*pCtx).iECMVs[iRefIdx][1] += mv[1] as i32;
                                                }
                                                iInterMbCorrectNum[iRefIdx] += 4;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    for i in 0..16 {
        if iInterMbCorrectNum[i] > 0 {
            (*pCtx).iECMVs[i][0] /= iInterMbCorrectNum[i];
            (*pCtx).iECMVs[i][1] /= iInterMbCorrectNum[i];
        }
    }
}

/// Driver for motion-compensated slice error concealment across all corrupted macroblocks.
pub unsafe extern "C" fn DoErrorConSliceMVCopy(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pSps.is_null() || (*pCtx).pDec.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return;
    }

    let iMbWidth = (*(*pCtx).pSps).iMbWidth as usize;
    let iMbHeight = (*(*pCtx).pSps).iMbHeight as usize;
    let pDstPic = (*pCtx).pDec;
    let pSrcPic = if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb
    } else {
        ptr::null_mut()
    };

    let pMbCorrectlyDecodedFlag = (*(*pCtx).pCurDqLayer).pMbCorrectlyDecodedFlag;
    if pMbCorrectlyDecodedFlag.is_null() {
        return;
    }

    let iDstStride = (*pDstPic).iLinesize[0] as usize;
    let mut sMCRefMem = TagMCRefMember::default();

    if !pSrcPic.is_null() {
        sMCRefMem.iSrcLineLuma = (*pSrcPic).iLinesize[0];
        sMCRefMem.iSrcLineChroma = (*pSrcPic).iLinesize[1];
        sMCRefMem.pSrcY = (*pSrcPic).pData[0];
        sMCRefMem.pSrcU = (*pSrcPic).pData[1];
        sMCRefMem.pSrcV = (*pSrcPic).pData[2];
        sMCRefMem.iDstLineLuma = (*pDstPic).iLinesize[0];
        sMCRefMem.iDstLineChroma = (*pDstPic).iLinesize[1];
        sMCRefMem.iPicWidth = (*pDstPic).iWidthInPixel;
        sMCRefMem.iPicHeight = (*pDstPic).iHeightInPixel;

        if pDstPic == pSrcPic {
            return;
        }
    }

    for iMbY in 0..iMbHeight {
        for iMbX in 0..iMbWidth {
            let iMbXyIndex = iMbY * iMbWidth + iMbX;
            if !*pMbCorrectlyDecodedFlag.add(iMbXyIndex) {
                (*pDstPic).iMbEcedNum += 1;
                if !pSrcPic.is_null() {
                    DoMbECMvCopy(pCtx, pDstPic, pSrcPic, iMbXyIndex as i32, iMbX as i32, iMbY as i32, &mut sMCRefMem);
                } else {
                    let mut pDstData = (*pDstPic).pData[0].add(iMbY * 16 * iDstStride + iMbX * 16);
                    for _ in 0..16 {
                        ptr::write_bytes(pDstData, 128, 16);
                        pDstData = pDstData.add(iDstStride);
                    }

                    let mut pDstDataU = (*pDstPic).pData[1].add(iMbY * 8 * (iDstStride / 2) + iMbX * 8);
                    for _ in 0..8 {
                        ptr::write_bytes(pDstDataU, 128, 8);
                        pDstDataU = pDstDataU.add(iDstStride / 2);
                    }

                    let mut pDstDataV = (*pDstPic).pData[2].add(iMbY * 8 * (iDstStride / 2) + iMbX * 8);
                    for _ in 0..8 {
                        ptr::write_bytes(pDstDataV, 128, 8);
                        pDstDataV = pDstDataV.add(iDstStride / 2);
                    }
                }
            }
        }
    }
}

/// Expand border pixels outward to allow out-of-bounds motion vector compensation.
pub unsafe fn ExpandReferencingPicture(
    pData: [*mut u8; 4],
    iWidth: i32,
    iHeight: i32,
    iStride: [i32; 4],
    pExpLuma: Option<PExpandPictureFunc>,
    pExpChrom: [Option<PExpandPictureFunc>; 2],
) {
    if let Some(func_luma) = pExpLuma {
        func_luma(pData[0], iStride[0], iWidth, iHeight);
    }
    let kiWidthUV = iWidth >> 1;
    let kiHeightUV = iHeight >> 1;
    let kbChrAligned = (kiWidthUV >= 16) && ((kiWidthUV & 0x0F) == 0);
    let idx = if kbChrAligned { 1 } else { 0 };

    if let Some(func_chroma) = pExpChrom[idx] {
        func_chroma(pData[1], iStride[1], kiWidthUV, kiHeightUV);
        func_chroma(pData[2], iStride[2], kiWidthUV, kiHeightUV);
    }
}

/// Fallback DPB reference marking routine.
pub unsafe extern "C" fn WelsMarkAsRef(pCtx: PWelsDecoderContext) -> i32 {
    crate::decoder::manage_dec_ref::WelsMarkAsRef(pCtx, std::ptr::null_mut())
}

/// Marks an error-concealed frame as a reference picture in the DPB and expands its borders.
pub unsafe extern "C" fn MarkECFrameAsRef(pCtx: PWelsDecoderContext) -> i32 {
    let iRet = WelsMarkAsRef(pCtx);
    if iRet != ERR_NONE {
        return iRet;
    }

    if !pCtx.is_null() && !(*pCtx).pDec.is_null() {
        ExpandReferencingPicture(
            (*(*pCtx).pDec).pData,
            (*(*pCtx).pDec).iWidthInPixel,
            (*(*pCtx).pDec).iHeightInPixel,
            (*(*pCtx).pDec).iLinesize,
            (*pCtx).sExpandPicFunc.pfExpandLumaPicture,
            (*pCtx).sExpandPicFunc.pfExpandChromaPicture,
        );
    }

    ERR_NONE
}

/// Top-level error concealment dispatcher.
pub unsafe extern "C" fn ImplementErrorCon(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pParam.is_null() {
        return;
    }

    let ec_mode = (*(*pCtx).pParam).eEcActiveIdc;

    if ec_mode == ERROR_CON_IDC::ERROR_CON_DISABLE {
        (*pCtx).iErrorCode |= dsBitstreamError;
        return;
    } else if ec_mode == ERROR_CON_IDC::ERROR_CON_FRAME_COPY
        || ec_mode == ERROR_CON_IDC::ERROR_CON_FRAME_COPY_CROSS_IDR
    {
        DoErrorConFrameCopy(pCtx);
    } else if ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
    {
        DoErrorConSliceCopy(pCtx);
    } else if ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE
    {
        GetAvilInfoFromCorrectMb(pCtx);
        DoErrorConSliceMVCopy(pCtx);
    }

    (*pCtx).iErrorCode |= dsDataErrorConcealed;
    if !(*pCtx).pDec.is_null() {
        (*(*pCtx).pDec).bIsComplete = false;
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_copy_16x16_and_8x8() {
        let mut src_buf = vec![0xABu8; 512];
        let mut dst_buf = vec![0u8; 512];

        unsafe {
            WelsCopy16x16_c(dst_buf.as_mut_ptr(), 32, src_buf.as_mut_ptr(), 32);
        }

        for row in 0..16 {
            for col in 0..16 {
                assert_eq!(dst_buf[row * 32 + col], 0xAB);
            }
        }
    }

    #[test]
    fn test_need_error_con() {
        let mut mb_flags = [true; 4];
        let mut sps = SSps {
            iMbWidth: 2,
            iMbHeight: 2,
            ..Default::default()
        };
        let mut dq_layer = SDqLayer {
            pMbCorrectlyDecodedFlag: mb_flags.as_mut_ptr(),
            ..Default::default()
        };
        let mut ctx = unsafe { Box::<SWelsDecoderContext>::new_zeroed().assume_init() };
        ctx.pCurDqLayer = &mut dq_layer as *mut _;
        ctx.pSps = &mut sps as *mut _;

        unsafe {
            assert_eq!(NeedErrorCon(&mut *ctx), false);
            mb_flags[2] = false;
            let _ = mb_flags;
            assert_eq!(NeedErrorCon(&mut *ctx), true);
        }
    }

    #[test]
    fn test_implement_error_con_disable() {
        let mut param = SDecodingParam {
            eEcActiveIdc: ERROR_CON_IDC::ERROR_CON_DISABLE,
            ..Default::default()
        };
        let mut ctx = unsafe { Box::<SWelsDecoderContext>::new_zeroed().assume_init() };
        ctx.pParam = &mut param as *mut _;

        unsafe {
            ImplementErrorCon(&mut *ctx);
            assert_eq!(ctx.iErrorCode & dsBitstreamError, dsBitstreamError);
        }
    }
}
