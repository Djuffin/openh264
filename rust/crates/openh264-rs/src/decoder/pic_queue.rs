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

//! # Decoded Picture Buffer Pool & Recycled Picture Queue (`pic_queue.rs`)
//!
//! Translated from `codec/decoder/core/inc/pic_queue.h` and `codec/decoder/core/src/pic_queue.cpp`.
//!
//! Provides the pre-allocated recycled picture buffer pool ([`SPicBuff`]) and
//! reconstructed picture object ([`SPicture`]) memory management for the H.264 / AVC
//! video decoder.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;
use crate::decoder::decoder_context::SDecodingParam;

// ============================================================================
// Constants & Geometry Macro Definitions
// ============================================================================

/// Pixel boundary alignment applied to frame buffer width and height dimensions.
pub const PICTURE_RESOLUTION_ALIGNMENT: i32 = 32;

/// Perimeter reference extension padding in pixels around all 4 edges.
pub const PADDING_LENGTH: i32 = 32;

/// Chroma reference extension padding in pixels.
pub const CHROMA_PADDING_LENGTH: i32 = 16;

/// Motion vector list indices.
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;

/// Sub-block counts and motion vector component counts per macroblock.
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MV_A: usize = 2;

// ============================================================================
// Data Structures & Enums
// ============================================================================

/// Slice types in H.264 standard bitstream syntax.
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

pub use crate::decoder::picture::{SPicture, PPicture};

/// Recycled picture buffer queue container.
///
/// Matches [`TagPicBuff`](file:///usr/local/google/home/ezemtsov/projects/openh264/codec/decoder/core/inc/pic_queue.h#L45-L49).
#[repr(C)]
#[derive(Debug)]
pub struct TagPicBuff {
    /// Array of pointers to [`SPicture`] objects (capacity: `iCapacity`).
    pub ppPic: *mut *mut SPicture,
    /// Total capacity size of the queue pool.
    pub iCapacity: i32,
    /// Current circular cursor index within the `ppPic` array.
    pub iCurrentIdx: i32,
}

pub type SPicBuff = TagPicBuff;
pub type PPicBuff = *mut SPicBuff;

impl Default for TagPicBuff {
    fn default() -> Self {
        Self {
            ppPic: std::ptr::null_mut(),
            iCapacity: 0,
            iCurrentIdx: 0,
        }
    }
}

pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};

// ============================================================================
// Helper Macros / Inline Functions
// ============================================================================

/// Alignment calculation macro matching `WELS_ALIGN(x, n)`.
#[inline]
pub const fn WELS_ALIGN(x: i32, n: i32) -> i32 {
    (x + (n - 1)) & !(n - 1)
}

pub use crate::decoder::decoder_core::GetThreadCount;

// ============================================================================
// Picture Memory Lifecycle Functions
// ============================================================================

/// Allocates and initializes an [`SPicture`] container with SIMD-aligned sample
/// planes and macroblock tracking metadata arrays.
///
/// # Safety
/// - `pCtx` must point to a valid [`SWelsDecoderContext`] containing a valid `pMemAlign`.
/// - Memory allocated must be freed using [`FreePicture`].
pub unsafe fn AllocPicture(
    pCtx: PWelsDecoderContext,
    kiPicWidth: i32,
    kiPicHeight: i32,
) -> PPicture {
    if pCtx.is_null() {
        return std::ptr::null_mut();
    }
    let pMa = unsafe { (*pCtx).pMemAlign };
    if pMa.is_null() {
        return std::ptr::null_mut();
    }

    let pPic = unsafe {
        (*pMa).WelsMallocz(
            std::mem::size_of::<SPicture>() as u32,
            b"PPicture\0".as_ptr() as *const c_char,
        ) as PPicture
    };
    if pPic.is_null() {
        return std::ptr::null_mut();
    }

    let iPicWidth = WELS_ALIGN(kiPicWidth + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);
    let iPicHeight = WELS_ALIGN(kiPicHeight + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);
    let iPicChromaWidth = iPicWidth >> 1;
    let iPicChromaHeight = iPicHeight >> 1;

    let iLumaSize = iPicWidth * iPicHeight;
    let iChromaSize = iPicChromaWidth * iPicChromaHeight;

    let bParseOnly = unsafe {
        if !(*pCtx).pParam.is_null() {
            (*(*pCtx).pParam).bParseOnly
        } else {
            false
        }
    };

    if bParseOnly {
        unsafe {
            (*pPic).pBuffer[0] = std::ptr::null_mut();
            (*pPic).pBuffer[1] = std::ptr::null_mut();
            (*pPic).pBuffer[2] = std::ptr::null_mut();
            (*pPic).pData[0] = std::ptr::null_mut();
            (*pPic).pData[1] = std::ptr::null_mut();
            (*pPic).pData[2] = std::ptr::null_mut();
            (*pPic).iLinesize[0] = iPicWidth;
            (*pPic).iLinesize[1] = iPicChromaWidth;
            (*pPic).iLinesize[2] = iPicChromaWidth;
        }
    } else {
        let total_size = (iLumaSize + (iChromaSize << 1)) as u32;
        let pBuf0 = unsafe {
            (*pMa).WelsMallocz(
                total_size,
                b"_pic->buffer[0]\0".as_ptr() as *const c_char,
            ) as *mut u8
        };
        if pBuf0.is_null() {
            unsafe { FreePicture(pPic, pMa) };
            return std::ptr::null_mut();
        }

        unsafe {
            std::ptr::write_bytes(pBuf0, 128u8, total_size as usize);
            (*pPic).pBuffer[0] = pBuf0;
            (*pPic).iLinesize[0] = iPicWidth;
            (*pPic).iLinesize[1] = iPicChromaWidth;
            (*pPic).iLinesize[2] = iPicChromaWidth;

            (*pPic).pBuffer[1] = pBuf0.add(iLumaSize as usize);
            (*pPic).pBuffer[2] = (*pPic).pBuffer[1].add(iChromaSize as usize);

            (*pPic).pData[0] = (*pPic).pBuffer[0].add(((1 + (*pPic).iLinesize[0]) * PADDING_LENGTH) as usize);
            (*pPic).pData[1] = (*pPic).pBuffer[1].add((((1 + (*pPic).iLinesize[1]) * PADDING_LENGTH) >> 1) as usize);
            (*pPic).pData[2] = (*pPic).pBuffer[2].add((((1 + (*pPic).iLinesize[2]) * PADDING_LENGTH) >> 1) as usize);
        }
    }

    unsafe {
        (*pPic).iPlanes = 3;
        (*pPic).iWidthInPixel = kiPicWidth;
        (*pPic).iHeightInPixel = kiPicHeight;
        (*pPic).iFrameNum = -1;
        (*pPic).iRefCount = 0;
        (*pPic).pSetUnRef = None;
    }

    let uiMbWidth = ((kiPicWidth + 15) >> 4) as u32;
    let uiMbHeight = ((kiPicHeight + 15) >> 4) as u32;
    let uiMbCount = uiMbWidth * uiMbHeight;

    unsafe {
        (*pPic).pMbCorrectlyDecodedFlag = (*pMa).WelsMallocz(
            uiMbCount * std::mem::size_of::<bool>() as u32,
            b"pPic->pMbCorrectlyDecodedFlag\0".as_ptr() as *const c_char,
        ) as *mut bool;

        (*pPic).pMbType = (*pMa).WelsMallocz(
            uiMbCount * std::mem::size_of::<u32>() as u32,
            b"pPic->pMbType\0".as_ptr() as *const c_char,
        ) as *mut u32;

        let mv_size = uiMbCount * (std::mem::size_of::<i16>() * MV_A * MB_BLOCK4x4_NUM) as u32;
        (*pPic).pMv[LIST_0] = (*pMa).WelsMallocz(
            mv_size,
            b"pPic->pMv[]\0".as_ptr() as *const c_char,
        ) as *mut [[i16; 2]; 16];
        (*pPic).pMv[LIST_1] = (*pMa).WelsMallocz(
            mv_size,
            b"pPic->pMv[]\0".as_ptr() as *const c_char,
        ) as *mut [[i16; 2]; 16];

        let ref_size = uiMbCount * (std::mem::size_of::<i8>() * MB_BLOCK4x4_NUM) as u32;
        (*pPic).pRefIndex[LIST_0] = (*pMa).WelsMallocz(
            ref_size,
            b"pCtx->sMb.pRefIndex[]\0".as_ptr() as *const c_char,
        ) as *mut [i8; 16];
        (*pPic).pRefIndex[LIST_1] = (*pMa).WelsMallocz(
            ref_size,
            b"pCtx->sMb.pRefIndex[]\0".as_ptr() as *const c_char,
        ) as *mut [i8; 16];

    }

    pPic
}

/// Deallocates an [`SPicture`] instance and all its associated buffers and event primitives.
///
/// # Safety
/// - `pPic` must point to an [`SPicture`] allocated by [`AllocPicture`] or be null.
/// - `pMa` must point to the [`CMemoryAlign`] allocator used to allocate `pPic`.
pub unsafe fn FreePicture(pPic: PPicture, pMa: *mut CMemoryAlign) {
    if pPic.is_null() || pMa.is_null() {
        return;
    }
    unsafe {
        if !(*pPic).pBuffer[0].is_null() {
            (*pMa).WelsFree(
                (*pPic).pBuffer[0] as *mut c_void,
                b"pPic->pBuffer[0]\0".as_ptr() as *const c_char,
            );
            (*pPic).pBuffer[0] = std::ptr::null_mut();
        }

        if !(*pPic).pMbCorrectlyDecodedFlag.is_null() {
            (*pMa).WelsFree(
                (*pPic).pMbCorrectlyDecodedFlag as *mut c_void,
                b"pPic->pMbCorrectlyDecodedFlag\0".as_ptr() as *const c_char,
            );
            (*pPic).pMbCorrectlyDecodedFlag = std::ptr::null_mut();
        }

        if !(*pPic).pMbType.is_null() {
            (*pMa).WelsFree(
                (*pPic).pMbType as *mut c_void,
                b"pPic->pMbType\0".as_ptr() as *const c_char,
            );
            (*pPic).pMbType = std::ptr::null_mut();
        }

        for listIdx in LIST_0..LIST_A {
            if !(*pPic).pMv[listIdx].is_null() {
                (*pMa).WelsFree(
                    (*pPic).pMv[listIdx] as *mut c_void,
                    b"pPic->pMv[]\0".as_ptr() as *const c_char,
                );
                (*pPic).pMv[listIdx] = std::ptr::null_mut();
            }
            if !(*pPic).pRefIndex[listIdx].is_null() {
                (*pMa).WelsFree(
                    (*pPic).pRefIndex[listIdx] as *mut c_void,
                    b"pPic->pRefIndex[]\0".as_ptr() as *const c_char,
                );
                (*pPic).pRefIndex[listIdx] = std::ptr::null_mut();
            }
        }

        (*pMa).WelsFree(
            pPic as *mut c_void,
            b"pPic\0".as_ptr() as *const c_char,
        );
    }
}

// ============================================================================
// Queue Retrieval Interface Routines
// ============================================================================

/// Retrieves an available, recyclable [`SPicture`] node from the picture buffer pool.
///
/// Performs a 2-pass circular scan:
/// 1. Pass 1: Scans candidate indices from `iCurrentIdx + 1` to `iCapacity - 1`.
/// 2. Pass 2: Wraps around and scans from `0` to `iCurrentIdx`.
///
/// A slot is eligible for recycling if `pPic != NULL && !pPic->bUsedAsRef && pPic->iRefCount <= 0`.
///
/// # Safety
/// `pPicBuf` must point to a valid [`SPicBuff`] pool structure.
pub unsafe fn PrefetchPic(pPicBuf: PPicBuff) -> PPicture {
    if pPicBuf.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        if (*pPicBuf).iCapacity == 0 || (*pPicBuf).ppPic.is_null() {
            return std::ptr::null_mut();
        }

        let mut iPicIdx: i32;
        let mut pPic: PPicture = std::ptr::null_mut();

        // Pass 1: Scan forward from iCurrentIdx + 1 to iCapacity - 1
        iPicIdx = (*pPicBuf).iCurrentIdx + 1;
        while iPicIdx < (*pPicBuf).iCapacity {
            let pCandidate = *(*pPicBuf).ppPic.add(iPicIdx as usize);
            if !pCandidate.is_null()
                && !(*pCandidate).bUsedAsRef
                && (*pCandidate).iRefCount <= 0
            {
                pPic = pCandidate;
                break;
            }
            iPicIdx += 1;
        }

        if !pPic.is_null() {
            (*pPicBuf).iCurrentIdx = iPicIdx;
            (*pPic).iPicBuffIdx = iPicIdx;
            return pPic;
        }

        // Pass 2: Wrap around and scan from index 0 to iCurrentIdx.
        // `iPicIdx < iCapacity` guards a read past `ppPic` that the C++ loop
        // (`iPicIdx <= pPicBuf->iCurrentIdx`) does not: each failed prefetch leaves
        // iCurrentIdx one higher, so once the DPB is exhausted it exceeds iCapacity
        // and the next call loads a wild pointer. C++ never reaches that state.
        iPicIdx = 0;
        while iPicIdx <= (*pPicBuf).iCurrentIdx && iPicIdx < (*pPicBuf).iCapacity {
            let pCandidate = *(*pPicBuf).ppPic.add(iPicIdx as usize);
            if !pCandidate.is_null()
                && !(*pCandidate).bUsedAsRef
                && (*pCandidate).iRefCount <= 0
            {
                pPic = pCandidate;
                break;
            }
            iPicIdx += 1;
        }

        (*pPicBuf).iCurrentIdx = iPicIdx;
        if !pPic.is_null() {
            (*pPic).iPicBuffIdx = iPicIdx;
        }
        pPic
    }
}

/// Retrieves the next circular picture node in round-robin FIFO sequence for multi-threaded decoding.
///
/// # Safety
/// `pPicBuf` must point to a valid [`SPicBuff`] pool structure.
pub unsafe fn PrefetchPicForThread(pPicBuf: PPicBuff) -> PPicture {
    if pPicBuf.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        if (*pPicBuf).iCapacity == 0 || (*pPicBuf).ppPic.is_null() {
            return std::ptr::null_mut();
        }

        let cur_idx = (*pPicBuf).iCurrentIdx as usize;
        let pPic = *(*pPicBuf).ppPic.add(cur_idx);
        if !pPic.is_null() {
            (*pPic).iPicBuffIdx = (*pPicBuf).iCurrentIdx;
        }

        (*pPicBuf).iCurrentIdx += 1;
        if (*pPicBuf).iCurrentIdx >= (*pPicBuf).iCapacity {
            (*pPicBuf).iCurrentIdx = 0;
        }
        pPic
    }
}

/// Retrieves an explicit picture node by its recorded buffer pool index (`iLastPicBuffIdx`).
///
/// # Safety
/// `pPicBuf` must point to a valid [`SPicBuff`] pool structure.
pub unsafe fn PrefetchLastPicForThread(pPicBuf: PPicBuff, iLastPicBuffIdx: i32) -> PPicture {
    if pPicBuf.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        if (*pPicBuf).iCapacity == 0 || (*pPicBuf).ppPic.is_null() {
            return std::ptr::null_mut();
        }

        let mut pPic: PPicture = std::ptr::null_mut();
        if iLastPicBuffIdx >= 0 && iLastPicBuffIdx < (*pPicBuf).iCapacity {
            pPic = *(*pPicBuf).ppPic.add(iLastPicBuffIdx as usize);
        }
        pPic
    }
}

// ============================================================================
// Buffer Pool Lifecycle Helpers (CreatePicBuff / DestroyPicBuff)
// ============================================================================

/// Allocates an [`SPicBuff`] queue pool structure and pre-allocates `kiSize` [`SPicture`] nodes.
///
/// # Safety
/// - `pCtx` must point to a valid [`SWelsDecoderContext`] containing `pMemAlign`.
/// - `ppPicBuf` must point to a writable `*mut SPicBuff` pointer variable.
pub unsafe fn CreatePicBuff(
    pCtx: PWelsDecoderContext,
    ppPicBuf: *mut PPicBuff,
    kiSize: i32,
    kiPicWidth: i32,
    kiPicHeight: i32,
) -> i32 {
    if pCtx.is_null() || ppPicBuf.is_null() {
        return 1;
    }
    let pMa = unsafe { (*pCtx).pMemAlign };
    if pMa.is_null() {
        return 1;
    }

    let pPicBuf = unsafe {
        (*pMa).WelsMallocz(
            std::mem::size_of::<SPicBuff>() as u32,
            b"PPicBuff\0".as_ptr() as *const c_char,
        ) as PPicBuff
    };
    if pPicBuf.is_null() {
        return 1;
    }

    let ppPicArray = unsafe {
        (*pMa).WelsMallocz(
            (kiSize as usize * std::mem::size_of::<PPicture>()) as u32,
            b"ppPic\0".as_ptr() as *const c_char,
        ) as *mut PPicture
    };
    if ppPicArray.is_null() {
        unsafe {
            (*pMa).WelsFree(pPicBuf as *mut c_void, b"pPicBuf\0".as_ptr() as *const c_char);
            *ppPicBuf = std::ptr::null_mut();
        }
        return 1;
    }

    unsafe {
        (*pPicBuf).ppPic = ppPicArray;
        for i in 0..kiSize as usize {
            let pPic = AllocPicture(pCtx, kiPicWidth, kiPicHeight);
            if pPic.is_null() {
                DestroyPicBuff(pCtx, &mut (pPicBuf as PPicBuff), pMa);
                *ppPicBuf = std::ptr::null_mut();
                return 1;
            }
            *ppPicArray.add(i) = pPic;
        }
        (*pPicBuf).iCapacity = kiSize;
        (*pPicBuf).iCurrentIdx = 0;
        *ppPicBuf = pPicBuf;
    }

    0
}

/// Releases all picture slots and deallocates the [`SPicBuff`] queue structure.
///
/// # Safety
/// - `ppPicBuf` must point to a valid `*mut SPicBuff` pointer variable.
/// - `pMa` must point to the [`CMemoryAlign`] allocator instance.
pub unsafe fn DestroyPicBuff(
    _pCtx: PWelsDecoderContext,
    ppPicBuf: *mut PPicBuff,
    pMa: *mut CMemoryAlign,
) {
    if ppPicBuf.is_null() || pMa.is_null() {
        return;
    }
    let pPicBuf = unsafe { *ppPicBuf };
    if pPicBuf.is_null() {
        return;
    }

    unsafe {
        if !(*pPicBuf).ppPic.is_null() {
            for i in 0..(*pPicBuf).iCapacity as usize {
                let pPic = *(*pPicBuf).ppPic.add(i);
                if !pPic.is_null() {
                    FreePicture(pPic, pMa);
                    *(*pPicBuf).ppPic.add(i) = std::ptr::null_mut();
                }
            }
            (*pMa).WelsFree(
                (*pPicBuf).ppPic as *mut c_void,
                b"pPicBuf->ppPic\0".as_ptr() as *const c_char,
            );
            (*pPicBuf).ppPic = std::ptr::null_mut();
        }
        (*pMa).WelsFree(
            pPicBuf as *mut c_void,
            b"pPicBuf\0".as_ptr() as *const c_char,
        );
        *ppPicBuf = std::ptr::null_mut();
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_picture_alignment_geometry() {
        let width = 320;
        let height = 240;
        let aligned_w = WELS_ALIGN(width + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);
        let aligned_h = WELS_ALIGN(height + (PADDING_LENGTH << 1), PICTURE_RESOLUTION_ALIGNMENT);

        assert_eq!(aligned_w, 384);
        assert_eq!(aligned_h, 320);
        assert_eq!(aligned_w % 32, 0);
        assert_eq!(aligned_h % 32, 0);
    }

    #[test]
    fn test_alloc_and_free_picture() {
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        unsafe {
            let p_pic = AllocPicture(&mut *ctx as *mut SWelsDecoderContext, 160, 120);
            assert!(!p_pic.is_null());
            assert_eq!((*p_pic).iWidthInPixel, 160);
            assert_eq!((*p_pic).iHeightInPixel, 120);
            assert!(!(*p_pic).pBuffer[0].is_null());
            assert!(!(*p_pic).pData[0].is_null());

            FreePicture(p_pic, &mut ma as *mut CMemoryAlign);
        }
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    #[test]
    fn test_prefetch_pic_circular_scan() {
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        let mut p_pic_buf: PPicBuff = std::ptr::null_mut();
        unsafe {
            let ret = CreatePicBuff(
                &mut *ctx as *mut SWelsDecoderContext,
                &mut p_pic_buf as *mut PPicBuff,
                4,
                64,
                64,
            );
            assert_eq!(ret, 0);
            assert!(!p_pic_buf.is_null());

            // First prefetch gets index 1 (Pass 1 scan from iCurrentIdx + 1)
            let pic1 = PrefetchPic(p_pic_buf);
            assert!(!pic1.is_null());
            assert_eq!((*p_pic_buf).iCurrentIdx, 1);

            // Mark pic1 as used as reference
            (*pic1).bUsedAsRef = true;

            // Second prefetch skips index 1, finds index 2
            let pic2 = PrefetchPic(p_pic_buf);
            assert!(!pic2.is_null());
            assert_eq!((*p_pic_buf).iCurrentIdx, 2);

            DestroyPicBuff(&mut *ctx as *mut SWelsDecoderContext, &mut p_pic_buf as *mut PPicBuff, &mut ma as *mut CMemoryAlign);
        }
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    #[test]
    fn test_prefetch_pic_for_thread() {
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam::default();
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        let mut p_pic_buf: PPicBuff = std::ptr::null_mut();
        unsafe {
            CreatePicBuff(
                &mut *ctx as *mut SWelsDecoderContext,
                &mut p_pic_buf as *mut PPicBuff,
                3,
                64,
                64,
            );

            let pic0 = PrefetchPicForThread(p_pic_buf);
            assert_eq!((*pic0).iPicBuffIdx, 0);
            assert_eq!((*p_pic_buf).iCurrentIdx, 1);

            let pic1 = PrefetchPicForThread(p_pic_buf);
            assert_eq!((*pic1).iPicBuffIdx, 1);
            assert_eq!((*p_pic_buf).iCurrentIdx, 2);

            let pic2 = PrefetchPicForThread(p_pic_buf);
            assert_eq!((*pic2).iPicBuffIdx, 2);
            assert_eq!((*p_pic_buf).iCurrentIdx, 0); // Wraps around

            let pic_lookup = PrefetchLastPicForThread(p_pic_buf, 1);
            assert_eq!(pic_lookup, pic1);

            DestroyPicBuff(&mut *ctx as *mut SWelsDecoderContext, &mut p_pic_buf as *mut PPicBuff, &mut ma as *mut CMemoryAlign);
        }
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }
}
