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
    unused_variables
)]

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use crate::decoder::decoder_context::{
    PicRefs, SpsRef, active_pps, active_sps, cur_and_refs, dec_pic, pps_of, prev_dpb_id, sps_of,
};

// ============================================================================
// Constants and Error Concealment Modes
// ============================================================================

/// Error concealment method selector enumeration (`ERROR_CON_IDC`).
// Same enum as the public API's; the decoder context stores the caller's value.
pub use crate::api::codec_api::ERROR_CON_IDC;

// Error status bitmask flags
pub const ERR_NONE: i32 = 0;
pub const dsRefLost: i32 = 0x02;
pub const dsBitstreamError: i32 = 0x04;
pub const dsDataErrorConcealed: i32 = 0x20;

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

/// The C++ needs the indirection because it selects a SIMD form from the CPU
/// flags; this port has no SIMD.
#[derive(Debug, Copy, Clone)]
pub struct SCopyFunc {
    pub bInstalled: bool,
}

impl SCopyFunc {
    /// The zero pattern — `bInstalled: false`, the state the decoder context is born
    /// in and leaves at `WelsInitDecoderFuncs`. [`Default`] is the *installed* table,
    /// which is what every other constructor of this type wants.
    pub fn memset_zero() -> Self {
        Self { bInstalled: false }
    }
}

impl Default for SCopyFunc {
    fn default() -> Self {
        Self { bInstalled: true }
    }
}

pub use crate::common::copy_mb::{copy_16x16, copy_8x8};
pub use crate::common::mc::SMcFunc;

// ============================================================================
// Core Decoder Context Structs
// ============================================================================

pub use crate::decoder::decoder_context::{Picture, SPicture, SDecodingParam};
pub use crate::decoder::decoder_context::pic_and_refs_mut;
use crate::decoder::decoder_context::ec_active_idc;
pub use crate::decoder::pic_queue::{PicId, RefSlot};
pub use crate::decoder::picture::{same_picture, pic_slot};
pub use crate::safe::plane::PaddedPlane;

pub use crate::decoder::parameter_sets::{SSps, SPosOffset as SFrameCrop};
pub use crate::decoder::decoder_core::{DqLayerState, SLayerInfo, MbDims};
pub use crate::decoder::decoder_context::{SWelsDecoderContext, SRefPic};

// ============================================================================
// Core Error Concealment Functions
// ============================================================================

/// Initializes error concealment function pointer dispatch table and resets freeze output flag.
pub extern "C" fn InitErrorCon(pCtx: &mut SWelsDecoderContext) {
    let ec_mode = pCtx.pParam.eEcActiveIdc;
    if ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
    {
        if ec_mode != ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE
            && ec_mode != ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
        {
            pCtx.bFreezeOutput = false;
        }

        pCtx.sCopyFunc.bInstalled = true;
    }
}

/// Evaluates if error concealment is required by inspecting the macroblock decoding flags.
pub extern "C" fn NeedErrorCon(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) -> bool {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return false;
    };
    let Some(sps) = active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps) else {
        return false;
    };

    let iMbNum = sps.iMbWidth * sps.iMbHeight;

    for i in 0..iMbNum {
        if !*pCurDqLayer.grid.mb_correctly_decoded_flag.get(i as usize) {
            return true;
        }
    }
    false
}

/// Performs full-frame error concealment by copying pixel planes from the previous reference picture.
pub extern "C" fn DoErrorConFrameCopy(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) {
    if pCtx.pDec.is_none() {
        return;
    }
    let Some((iMbWidth, iMbHeight)) =
        active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps).map(|sps| (sps.iMbWidth, sps.iMbHeight))
    else {
        return;
    };

    let prev = prev_dpb_id(&pCtx.pLastDecPicInfo);
    let (pDstPic, pRefs) = pic_and_refs_mut(&mut pCtx.pPicBuff, pCtx.pDec);
    let Some(pDstPic) = pDstPic else {
        return;
    };
    let mut pSrcPic = pRefs.classify(prev);

    let uiHeightInPixelY = (iMbHeight as u32) << 4;
    let iStrideY = pDstPic.linesize(0);
    let iStrideUV = pDstPic.linesize(1);
    pDstPic.iMbEcedNum = (iMbWidth * iMbHeight) as i32;

    if pCurDqLayer.is_some() {
        if ec_active_idc(&pCtx.pParam) == ERROR_CON_IDC::ERROR_CON_FRAME_COPY
            && pCurDqLayer.as_ref().unwrap().sLayerInfo.sNalHeaderExt.bIdrFlag
        {
            pSrcPic = RefSlot::Empty;
        }
    }

    if matches!(pSrcPic, RefSlot::Empty) {
        // Fill planes with neutral gray (128).
        let spans = [
            (0usize, (uiHeightInPixelY as usize) * (iStrideY as usize)),
            (1, ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize)),
            (2, ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize)),
        ];
        for (i, len) in spans {
            let plane = pDstPic.plane_mut(i);
            if !plane.is_empty() {
                let base = plane.origin();
                plane.as_mut_slice()[base..base + len].fill(128);
            }
        }
    } else if let RefSlot::Other(pSrcPic) = pSrcPic {
        let spans = [
            (0usize, (uiHeightInPixelY as usize) * (iStrideY as usize)),
            (1, ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize)),
            (2, ((uiHeightInPixelY >> 1) as usize) * (iStrideUV as usize)),
        ];
        for (i, len) in spans {
            if pDstPic.plane(i).is_empty() || pSrcPic.plane(i).is_empty() {
                continue;
            }
            let (src_base, dst_base) = (pSrcPic.plane(i).origin(), pDstPic.plane(i).origin());
            let src = &pSrcPic.plane(i).as_slice()[src_base..src_base + len];
            pDstPic.plane_mut(i).as_mut_slice()[dst_base..dst_base + len].copy_from_slice(src);
        }
    }
}

/// Performs macroblock-level error concealment by copying collocated undamaged macroblocks.
pub extern "C" fn DoErrorConSliceCopy(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return;
    };
    if pCtx.pDec.is_none() {
        return;
    }

    let Some((iMbWidth, iMbHeight)) = active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps)
        .map(|sps| (sps.iMbWidth as usize, sps.iMbHeight as usize))
    else {
        return;
    };
    let prev = prev_dpb_id(&pCtx.pLastDecPicInfo);
    let (pDstPic, pRefs) = pic_and_refs_mut(&mut pCtx.pPicBuff, pCtx.pDec);
    let Some(pDstPic) = pDstPic else {
        return;
    };
    let mut pSrcPic = pRefs.classify(prev);

    if ec_active_idc(&pCtx.pParam) == ERROR_CON_IDC::ERROR_CON_SLICE_COPY
        && pCurDqLayer.sLayerInfo.sNalHeaderExt.bIdrFlag
    {
        pSrcPic = RefSlot::Empty;
    }

    // The self-copy arm returns **before the loop**, so it never reaches
    // `iMbEcedNum`. That is a third behaviour, not a second: an empty slot conceals
    // (counting each macroblock) and the current picture does nothing at all.
    if matches!(pSrcPic, RefSlot::Current) {
        return;
    }
    let pSrcPic = match pSrcPic {
        RefSlot::Other(pic) => Some(pic),
        _ => None,
    };

    let installed = pCtx.sCopyFunc.bInstalled;

    for iMbY in 0..iMbHeight {
        for iMbX in 0..iMbWidth {
            let iMbXyIndex = iMbY * iMbWidth + iMbX;
            if *pCurDqLayer.grid.mb_correctly_decoded_flag.get(iMbXyIndex) {
                continue;
            }
            pDstPic.iMbEcedNum += 1;
            match pSrcPic {
                Some(pSrcPic) if installed => {
                    for (plane, size) in [(0usize, 16isize), (1, 8), (2, 8)] {
                        let (x, y) = ((iMbX as isize) * size, (iMbY as isize) * size);
                        let src = pSrcPic.plane(plane).cursor(x, y);
                        let mut dst = pDstPic.plane_mut(plane).cursor_mut(x, y);
                        if plane == 0 {
                            copy_16x16(&src, &mut dst);
                        } else {
                            copy_8x8(&src, &mut dst);
                        }
                    }
                }
                // The installed-but-no-source arm and the not-installed arm are
                // **different**: with no source the C++ gray-fills, and with no
                // kernels installed it called through a null slot, which this port
                // answered by doing nothing.
                None => {
                    for (plane, size) in [(0usize, 16isize), (1, 8), (2, 8)] {
                        let (x, y) = ((iMbX as isize) * size, (iMbY as isize) * size);
                        let p = pDstPic.plane_mut(plane);
                        for r in 0..size {
                            p.row_mut(y + r, x, size as usize).fill(128);
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }
}

/// What the EC motion-copy family needs out of the context.
#[derive(Clone, Copy)]
pub struct EcMvCtx {
    /// `pCtx->sCopyFunc.bInstalled`.
    pub bCopyInstalled: bool,
    /// `pCtx->pECRefPic[0]`.
    pub ec_ref: Option<PicId>,
    /// `pCtx->iECMVs[0]`.
    pub iECMVs: [i32; 2],
    /// `pCtx->sFrameCrop`, or `None` when the active SPS does not crop — the
    /// `bFrameCroppingFlag` test, answered at the caller where the SPS is reachable.
    pub crop: Option<SFrameCrop>,
}

/// Fallback motion compensation handler for macroblock reconstruction.
#[inline]
fn BaseMC(
    bCopyInstalled: bool,
    src: &SPicture,
    dst: &mut SPicture,
    iXOffset: i32,
    iYOffset: i32,
    iMVs: [i16; 2],
) {
    let iFullMVx = (iXOffset << 2) + (iMVs[0] as i32);
    let iFullMVy = (iYOffset << 2) + (iMVs[1] as i32);

    // The C added `(iFullMVx >> 2) + (iFullMVy >> 2) * iSrcLineLuma` to the source
    // *plane origin*, so what is left once the stride belongs to the plane is the
    // sample coordinate; chroma shifts by three, as `decode_slice.rs`'s `BaseMC` does.
    let (sx_l, sy_l) = ((iFullMVx >> 2) as isize, (iFullMVy >> 2) as isize);
    let (sx_c, sy_c) = ((iFullMVx >> 3) as isize, (iFullMVy >> 3) as isize);
    let (dx_l, dy_l) = (iXOffset as isize, iYOffset as isize);
    let (dx_c, dy_c) = ((iXOffset >> 1) as isize, (iYOffset >> 1) as isize);

    if !bCopyInstalled {
        return;
    }
    // The C's three `!pDst*.is_null() && !pSrc*.is_null()` guards are the two
    // pictures' planes being allocated; an empty `PaddedPlane` is what a null
    // `pData[i]` was, and the cursor would panic rather than read it.
    if src.plane(0).is_empty() || dst.plane(0).is_empty() {
        return;
    }
    copy_16x16(
        &src.plane(0).cursor(sx_l, sy_l),
        &mut dst.plane_mut(0).cursor_mut(dx_l, dy_l),
    );
    for i in 1..3usize {
        if src.plane(i).is_empty() || dst.plane(i).is_empty() {
            continue;
        }
        copy_8x8(
            &src.plane(i).cursor(sx_c, sy_c),
            &mut dst.plane_mut(i).cursor_mut(dx_c, dy_c),
        );
    }
}

/// Applies motion-compensated error concealment for a single lost macroblock.
fn DoMbECMvCopy(
    ec: &EcMvCtx,
    pDec: &mut SPicture,
    pRef: &SPicture,
    iEcRefFramePoc: Option<i32>,
    iMbX: i32,
    iMbY: i32,
    iPicWidth: i32,
    iPicHeight: i32,
) {
    let mut iMVs = [0i16; 2];
    let iMbXInPix = iMbX << 4;
    let iMbYInPix = iMbY << 4;
    let iCurrPoc = pDec.iFramePoc;

    if pDec.bIdrFlag || ec.ec_ref.is_none() {
        // The zero-motion copy: source and destination at the same macroblock.
        BaseMC(ec.bCopyInstalled, pRef, pDec, iMbXInPix, iMbYInPix, [0, 0]);
        return;
    }

    if ec.ec_ref == pRef.pic_id() {
        iMVs[0] = ec.iECMVs[0] as i16;
        iMVs[1] = ec.iECMVs[1] as i16;
    } else {
        let iScale0 = iEcRefFramePoc.unwrap_or(0) - iCurrPoc;
        let iScale1 = pRef.iFramePoc - iCurrPoc;
        iMVs[0] = if iScale0 == 0 {
            0
        } else {
            (ec.iECMVs[0] * iScale1 / iScale0) as i16
        };
        iMVs[1] = if iScale0 == 0 {
            0
        } else {
            (ec.iECMVs[1] * iScale1 / iScale0) as i16
        };
    }

    let mut iFullMVx = (iMbXInPix << 2) + (iMVs[0] as i32);
    let mut iFullMVy = (iMbYInPix << 2) + (iMVs[1] as i32);

    let mut iPicWidthLeftLimit = 0;
    let mut iPicHeightTopLimit = 0;
    let mut iPicWidthRightLimit = iPicWidth;
    let mut iPicHeightBottomLimit = iPicHeight;

    if let Some(crop) = ec.crop {
        iPicWidthLeftLimit = crop.iLeftOffset * 2;
        iPicWidthRightLimit = iPicWidth - crop.iRightOffset * 2;
        iPicHeightTopLimit = crop.iTopOffset * 2;
        // The C reads `iTopOffset` twice here rather than `iBottomOffset`; kept.
        iPicHeightBottomLimit = iPicHeight - crop.iTopOffset * 2;
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

    BaseMC(ec.bCopyInstalled, pRef, pDec, iMbXInPix, iMbYInPix, iMVs);
}

/// Gathers motion vector statistics from correctly decoded macroblocks in the current picture.
pub extern "C" fn GetAvilInfoFromCorrectMb(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return;
    };

    let Some((iMbWidth, iMbHeight)) =
        active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps).map(|sps| (sps.iMbWidth, sps.iMbHeight))
    else {
        return;
    };
    let Some(pDec) = dec_pic(&mut pCtx.pPicBuff, pCtx.pDec) else {
        return;
    };

    let mut iInterMbCorrectNum = [0i32; 16];

    for r in 0..16 {
        pCtx.iECMVs[r][0] = 0;
        pCtx.iECMVs[r][1] = 0;
        pCtx.pECRefPic[r] = None;
    }

    for iMbY in 0..iMbHeight {
        for iMbX in 0..iMbWidth {
            let iMbXyIndex = (iMbY * iMbWidth + iMbX) as usize;
            if *pCurDqLayer.grid.mb_correctly_decoded_flag.get(iMbXyIndex)
                && !pDec.pMbType.as_slice().is_empty() {
                let iMBType = *pDec.pMbType.get(iMbXyIndex);
                if IS_INTER(iMBType) {
                    match iMBType {
                        MB_TYPE_SKIP | MB_TYPE_16x16 => {
                            if !pDec.pRefIndex[0].as_slice().is_empty() && !pDec.pMv[0].as_slice().is_empty() {
                                let ref_row = *pDec.pRefIndex[0].get(iMbXyIndex);
                                let mv_row = *pDec.pMv[0].get(iMbXyIndex);
                                let iRefIdx = ref_row[0] as usize;
                                if iRefIdx < 16 {
                                    let mv = mv_row[0];
                                    pCtx.iECMVs[iRefIdx][0] += mv[0] as i32;
                                    pCtx.iECMVs[iRefIdx][1] += mv[1] as i32;
                                    pCtx.pECRefPic[iRefIdx] = pCtx.sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                            }
                        }
                        MB_TYPE_16x8 => {
                            if !pDec.pRefIndex[0].as_slice().is_empty() && !pDec.pMv[0].as_slice().is_empty() {
                                let ref_row = *pDec.pRefIndex[0].get(iMbXyIndex);
                                let mv_row = *pDec.pMv[0].get(iMbXyIndex);
                                // Partition 0
                                let mut iRefIdx = ref_row[0] as usize;
                                if iRefIdx < 16 {
                                    let mv0 = mv_row[0];
                                    pCtx.iECMVs[iRefIdx][0] += mv0[0] as i32;
                                    pCtx.iECMVs[iRefIdx][1] += mv0[1] as i32;
                                    pCtx.pECRefPic[iRefIdx] = pCtx.sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                                // Partition 1
                                iRefIdx = ref_row[8] as usize;
                                if iRefIdx < 16 {
                                    let mv8 = mv_row[8];
                                    pCtx.iECMVs[iRefIdx][0] += mv8[0] as i32;
                                    pCtx.iECMVs[iRefIdx][1] += mv8[1] as i32;
                                    pCtx.pECRefPic[iRefIdx] = pCtx.sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                            }
                        }
                        MB_TYPE_8x16 => {
                            if !pDec.pRefIndex[0].as_slice().is_empty() && !pDec.pMv[0].as_slice().is_empty() {
                                let ref_row = *pDec.pRefIndex[0].get(iMbXyIndex);
                                let mv_row = *pDec.pMv[0].get(iMbXyIndex);
                                // Partition 0
                                let mut iRefIdx = ref_row[0] as usize;
                                if iRefIdx < 16 {
                                    let mv0 = mv_row[0];
                                    pCtx.iECMVs[iRefIdx][0] += mv0[0] as i32;
                                    pCtx.iECMVs[iRefIdx][1] += mv0[1] as i32;
                                    pCtx.pECRefPic[iRefIdx] = pCtx.sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                                // Partition 1
                                iRefIdx = ref_row[2] as usize;
                                if iRefIdx < 16 {
                                    let mv2 = mv_row[2];
                                    pCtx.iECMVs[iRefIdx][0] += mv2[0] as i32;
                                    pCtx.iECMVs[iRefIdx][1] += mv2[1] as i32;
                                    pCtx.pECRefPic[iRefIdx] = pCtx.sRefPic.pRefList[LIST_0][iRefIdx];
                                    iInterMbCorrectNum[iRefIdx] += 1;
                                }
                            }
                        }
                        MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
                            if !pDec.pRefIndex[0].as_slice().is_empty()
                                && !pDec.pMv[0].as_slice().is_empty()
                            {
                                let sub_types = *pCurDqLayer.grid.sub_mb_type.get(iMbXyIndex);
                                let ref_row = *pDec.pRefIndex[0].get(iMbXyIndex);
                                let mv_row = *pDec.pMv[0].get(iMbXyIndex);
                                for i in 0..4 {
                                    let iSubMBType = sub_types[i];
                                    let iIIdx = ((i >> 1) << 3) + ((i & 1) << 1);
                                    let iRefIdx = ref_row[iIIdx] as usize;
                                    if iRefIdx < 16 {
                                        pCtx.pECRefPic[iRefIdx] = pCtx.sRefPic.pRefList[LIST_0][iRefIdx];
                                        match iSubMBType {
                                            SUB_MB_TYPE_8x8 => {
                                                let mv = mv_row[iIIdx];
                                                pCtx.iECMVs[iRefIdx][0] += mv[0] as i32;
                                                pCtx.iECMVs[iRefIdx][1] += mv[1] as i32;
                                                iInterMbCorrectNum[iRefIdx] += 1;
                                            }
                                            SUB_MB_TYPE_8x4 => {
                                                let mv0 = mv_row[iIIdx];
                                                let mv4 = mv_row[iIIdx + 4];
                                                pCtx.iECMVs[iRefIdx][0] += (mv0[0] as i32) + (mv4[0] as i32);
                                                pCtx.iECMVs[iRefIdx][1] += (mv0[1] as i32) + (mv4[1] as i32);
                                                iInterMbCorrectNum[iRefIdx] += 2;
                                            }
                                            SUB_MB_TYPE_4x8 => {
                                                let mv0 = mv_row[iIIdx];
                                                let mv1 = mv_row[iIIdx + 1];
                                                pCtx.iECMVs[iRefIdx][0] += (mv0[0] as i32) + (mv1[0] as i32);
                                                pCtx.iECMVs[iRefIdx][1] += (mv0[1] as i32) + (mv1[1] as i32);
                                                iInterMbCorrectNum[iRefIdx] += 2;
                                            }
                                            SUB_MB_TYPE_4x4 => {
                                                for j in 0..4 {
                                                    let iJIdx = ((j >> 1) << 2) + (j & 1);
                                                    let mv = mv_row[iIIdx + iJIdx];
                                                    pCtx.iECMVs[iRefIdx][0] += mv[0] as i32;
                                                    pCtx.iECMVs[iRefIdx][1] += mv[1] as i32;
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
            pCtx.iECMVs[i][0] /= iInterMbCorrectNum[i];
            pCtx.iECMVs[i][1] /= iInterMbCorrectNum[i];
        }
    }
}

/// Driver for motion-compensated slice error concealment across all corrupted macroblocks.
pub fn DoErrorConSliceMVCopy(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return;
    };
    if pCtx.pDec.is_none() {
        return;
    }

    let Some((iMbWidth, iMbHeight)) = active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps)
        .map(|sps| (sps.iMbWidth as usize, sps.iMbHeight as usize))
    else {
        return;
    };
    let ec = EcMvCtx {
        bCopyInstalled: pCtx.sCopyFunc.bInstalled,
        ec_ref: pCtx.pECRefPic[0],
        iECMVs: [pCtx.iECMVs[0][0], pCtx.iECMVs[0][1]],
        crop: active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps)
            .filter(|sps| sps.bFrameCroppingFlag)
            .map(|_| pCtx.sFrameCrop),
    };
    let prev = prev_dpb_id(&pCtx.pLastDecPicInfo);
    let ec_ref = pCtx.pECRefPic[0];
    let (pDstPic, pRefs) = pic_and_refs_mut(&mut pCtx.pPicBuff, pCtx.pDec);
    let Some(pDstPic) = pDstPic else {
        return;
    };
    let iEcRefFramePoc = pRefs.classify(ec_ref).poc();

    // `RefSlot::Current` is the `same_picture(pDstPic, pSrcPic)` early return, and
    // `RefSlot::Empty` is the null source that falls through to the grey fill.
    let pSrcPic = match pRefs.classify(prev) {
        RefSlot::Current => return,
        RefSlot::Other(pic) => Some(pic),
        RefSlot::Empty => None,
    };
    let (iPicWidth, iPicHeight) = (pDstPic.iWidthInPixel, pDstPic.iHeightInPixel);

    for iMbY in 0..iMbHeight {
        for iMbX in 0..iMbWidth {
            let iMbXyIndex = iMbY * iMbWidth + iMbX;
            if !*pCurDqLayer.grid.mb_correctly_decoded_flag.get(iMbXyIndex) {
                pDstPic.iMbEcedNum += 1;
                match pSrcPic {
                    Some(pSrcPic) => DoMbECMvCopy(
                        &ec,
                        pDstPic,
                        pSrcPic,
                        iEcRefFramePoc,
                        iMbX as i32,
                        iMbY as i32,
                        iPicWidth,
                        iPicHeight,
                    ),
                    // The grey fill, on plane cursors.
                    None => {
                        let (x, y) = ((iMbX as isize) << 4, (iMbY as isize) << 4);
                        let mut cur = pDstPic.plane_mut(0).cursor_mut(x, y);
                        for dy in 0..16 {
                            cur.row_mut(dy, 0, 16).fill(128);
                        }
                        let (x, y) = ((iMbX as isize) << 3, (iMbY as isize) << 3);
                        for i in 1..3usize {
                            let mut cur = pDstPic.plane_mut(i).cursor_mut(x, y);
                            for dy in 0..8 {
                                cur.row_mut(dy, 0, 8).fill(128);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Fallback DPB reference marking routine.
pub extern "C" fn WelsMarkAsRef(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) -> i32 {
    crate::decoder::manage_dec_ref::WelsMarkAsRef(pCtx, pCurDqLayer, None)
}

/// Marks an error-concealed frame as a reference picture in the DPB and expands its borders.
pub extern "C" fn MarkECFrameAsRef(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) -> i32 {
    let iRet = WelsMarkAsRef(pCtx, pCurDqLayer);
    if iRet != ERR_NONE {
        return iRet;
    }

    if pCtx.pDec.is_some() {
        if let Some(pDec) = dec_pic(&mut pCtx.pPicBuff, pCtx.pDec) {
            pDec.expand_as_reference();
        }
    }

    ERR_NONE
}

/// Top-level error concealment dispatcher.
pub extern "C" fn ImplementErrorCon(pCtx: &mut SWelsDecoderContext, mut pCurDqLayer: Option<&mut DqLayerState>) {
    let ec_mode = pCtx.pParam.eEcActiveIdc;

    if ec_mode == ERROR_CON_IDC::ERROR_CON_DISABLE {
        pCtx.iErrorCode |= dsBitstreamError;
        return;
    } else if ec_mode == ERROR_CON_IDC::ERROR_CON_FRAME_COPY
        || ec_mode == ERROR_CON_IDC::ERROR_CON_FRAME_COPY_CROSS_IDR
    {
        DoErrorConFrameCopy(pCtx, pCurDqLayer.as_deref_mut());
    } else if ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
    {
        DoErrorConSliceCopy(pCtx, pCurDqLayer.as_deref_mut());
    } else if ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR
        || ec_mode == ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE
    {
        GetAvilInfoFromCorrectMb(pCtx, pCurDqLayer.as_deref_mut());
        {
            DoErrorConSliceMVCopy(pCtx, pCurDqLayer.as_deref_mut());
        }
    }

    pCtx.iErrorCode |= dsDataErrorConcealed;
    if let Some(pDec) = dec_pic(&mut pCtx.pPicBuff, pCtx.pDec) {
        pDec.bIsComplete = false;
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_need_error_con() {
        let sps = SSps {
            iMbWidth: 2,
            iMbHeight: 2,
            ..Default::default()
        };
        let mut dq_layer = { DqLayerState::for_grid(MbDims::new(2, 2)) };
        dq_layer.grid.mb_correctly_decoded_flag.as_mut_slice().fill(true);
        let mut ctx = SWelsDecoderContext::new_boxed();

        ctx.sSpsPpsCtx.sSpsBuffer[0] = sps;
        ctx.active_sps = Some(SpsRef { id: 0, subset: false });

        {
            assert_eq!(NeedErrorCon(&mut *ctx, Some(&mut dq_layer)), false);
            *dq_layer.grid.mb_correctly_decoded_flag.get_mut(2) = false;
            assert_eq!(NeedErrorCon(&mut *ctx, Some(&mut dq_layer)), true);
        }
    }

    /// `DoErrorConSliceCopy`: when the previous decoded picture *is* the
    /// destination, the function returns before writing anything. A second picture
    /// with the same POC is a different picture and must be copied from.
    #[test]
    fn p3_slice_copy_self_copy_guard_is_by_identity() {
        const W: usize = 2;
        const H: usize = 2;
        const STRIDE: usize = W * 16;
        // A kernel transliterated from C bumps its row pointer *after* the last
        // row, so a test buffer sized exactly `h * stride` is UB at the final
        // `offset`. One spare row on every plane.
        const PLANE: usize = STRIDE * (H * 16 + 1);

        // Unpadded (`origin` 0) and `STRIDE`-wide — the function derives the
        // chroma stride as `iDstStride / 2` from luma and never reads
        // `linesize(1)`.
        let planes = |fill: u8| {
            [
                PaddedPlane::from_parts(vec![fill; PLANE], STRIDE, 0, W * 16, H * 16),
                PaddedPlane::from_parts(vec![fill; PLANE], STRIDE / 2, 0, W * 8, H * 8),
                PaddedPlane::from_parts(vec![fill; PLANE], STRIDE / 2, 0, W * 8, H * 8),
            ]
        };

        let run = |same_object: bool| -> u8 {
            // `dst` carries a marker; a real copy overwrites it with `src`'s.
            let mut dst = SPicture::with_planes(planes(0xAA), MbDims::none());
            dst.iWidthInPixel = (W * 16) as i32;
            dst.iHeightInPixel = (H * 16) as i32;
            dst.iFramePoc = 7;

            let mut src = SPicture::with_planes(planes(0x11), MbDims::none());
            src.iFramePoc = 7; // duplicate POC on purpose

            let sps = SSps { iMbWidth: W as u32, iMbHeight: H as u32, ..Default::default() };
            // every MB lost, so EC has work to do.
            let mut dq_layer = { DqLayerState::for_grid(MbDims::new(W, H)) };
            let mut last = crate::decoder::decoder_context::SWelsLastDecPicInfo::default();
            let mut ctx = SWelsDecoderContext::new_boxed();

            {
                // Slot 0 is the destination, slot 1 the source; `PicPool::over`
                // stamps each with its `PicId`, which is what makes the identity
                // this test is about a slot comparison.
                let pool = crate::decoder::pic_queue::PicPool::over(vec![
                    Some(Box::new(dst)),
                    Some(Box::new(src)),
                ]);
                let dst_id = pool.id(0);
                let src_id = pool.id(1);
                last.pPreviousDecodedPictureInDpb =
                    Some(if same_object { dst_id } else { src_id });
        ctx.sSpsPpsCtx.sSpsBuffer[0] = sps;
        ctx.active_sps = Some(SpsRef { id: 0, subset: false });
                ctx.pPicBuff = Some(pool);
                ctx.pDec = Some(dst_id);
                ctx.pLastDecPicInfo = last;
                // The copy itself goes through the context's copy-function pair;
                // `new_boxed()` leaves it zeroed, which would make both arms write
                // nothing and the test vacuous.
                ctx.sCopyFunc = SCopyFunc::default();
                DoErrorConSliceCopy(&mut *ctx, Some(&mut dq_layer));
                // The destination is the pool's, so the marker is read back out
                // of the slot.
                let pool = ctx.pPicBuff.as_deref().expect("the fixture's pool");
                pool.slot(dst_id).expect("the fixture's slot").plane(0).at(0, 0)
            }
        };

        assert_eq!(run(true), 0xAA, "src == dst must return before any write");
        assert_eq!(
            run(false),
            0x11,
            "a distinct picture with the same POC is a valid concealment source"
        );
    }

    /// The motion-compensated path must not conceal a macroblock from the
    /// picture it is writing into.
    #[test]
    fn p3_mb_ec_mv_copy_self_reference_guard_is_by_identity() {
        const W: usize = 2;
        const H: usize = 2;
        const STRIDE: usize = W * 16;
        const PLANE: usize = STRIDE * H * 16;

        let planes = |fill: u8| {
            [
                PaddedPlane::from_parts(vec![fill; PLANE], STRIDE, 0, W * 16, H * 16),
                PaddedPlane::from_parts(vec![fill; PLANE], STRIDE / 2, 0, W * 8, H * 8),
                PaddedPlane::from_parts(vec![fill; PLANE], STRIDE / 2, 0, W * 8, H * 8),
            ]
        };

        let run = |same_object: bool| -> u8 {
            let mut dst = SPicture::with_planes(planes(0xAA), MbDims::none());
            dst.iWidthInPixel = (W * 16) as i32;
            dst.iHeightInPixel = (H * 16) as i32;
            dst.iFramePoc = 7;
            let mut src = SPicture::with_planes(planes(0x11), MbDims::none());
            src.iFramePoc = 7; // duplicate POC on purpose

            let sps = SSps { iMbWidth: W as u32, iMbHeight: H as u32, ..Default::default() };
            let mut dq_layer = { DqLayerState::for_grid(MbDims::new(W, H)) };
            let mut last = crate::decoder::decoder_context::SWelsLastDecPicInfo::default();
            let mut ctx = SWelsDecoderContext::new_boxed();

            let dst_id = {
                let pool = crate::decoder::pic_queue::PicPool::over(vec![
                    Some(Box::new(dst)),
                    Some(Box::new(src)),
                ]);
                let dst_id = pool.id(0);
                let src_id = pool.id(1);
                last.pPreviousDecodedPictureInDpb =
                    Some(if same_object { dst_id } else { src_id });
                ctx.sSpsPpsCtx.sSpsBuffer[0] = sps;
                ctx.active_sps = Some(SpsRef { id: 0, subset: false });
                // `new_boxed` zeroes the context, so `bInstalled` starts `false`.
                // `Initialize` sets it; without this line the copy arm below would
                // pass for the wrong reason.
                ctx.sCopyFunc.bInstalled = true;
                ctx.pPicBuff = Some(pool);
                ctx.pDec = Some(dst_id);
                ctx.pLastDecPicInfo = last;
                dst_id
            };

            DoErrorConSliceMVCopy(&mut ctx, Some(&mut dq_layer));
            let out = ctx
                .pPicBuff
                .as_deref()
                .expect("the fixture's pool")
                .slot(dst_id)
                .expect("the fixture's slot")
                .plane(0)
                .at(0, 0);
            ctx.pLastDecPicInfo = Default::default();
            out
        };

        assert_eq!(run(true), 0xAA, "src == dst must return before any write");
        assert_eq!(
            run(false),
            0x11,
            "a distinct picture with the same POC is a valid concealment source"
        );
    }

    #[test]
    fn test_implement_error_con_disable() {
        let param = SDecodingParam {
            eEcActiveIdc: ERROR_CON_IDC::ERROR_CON_DISABLE,
            ..Default::default()
        };
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pParam = param;

        {
            ImplementErrorCon(&mut *ctx, None);
            assert_eq!(ctx.iErrorCode & dsBitstreamError, dsBitstreamError);
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`.
pub use crate::common::cpu_core::{WELS_CPU_LSX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE2};
