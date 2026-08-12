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

pub use crate::decoder::picture::{SPicture, PPicture};
pub use crate::safe::plane::PaddedPlane;

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
//
// S25 for this file (T5.C3, enumerated with the conversion as plan §7.6 asks):
// *who else reaches this `SPicture` while a borrow of it is held?*
//
// The pool is where the question is sharpest, because `SPicBuff.ppPic` is
// `*mut *mut SPicture` and `pCtx->pDec` points **into that array** — the picture the
// decoder is writing is one of the slots the recycling scan walks. Four answers:
//
// 1. **`PrefetchPic` holds no borrow of a picture.** It reads `bUsedAsRef` and
//    `iRefCount` through the slot pointer, one field per expression, and writes
//    `iPicBuffIdx` on the winner after the scan has stopped. The two other prefetch
//    functions are shorter still. Nothing in this file takes a `&mut SPicture` that
//    spans a call, so the conversion introduces no borrow here at all: an owned
//    plane changes `AllocPicture`/`FreePicture` and leaves the scan untouched.
// 2. **The scan cannot see a half-built picture.** `CreatePicBuff` fills `ppPic`
//    before it sets `iCapacity`, and every prefetch returns early on `iCapacity == 0`
//    — so a picture is either absent from the pool or fully constructed. That was
//    true before and it is what lets `AllocPicture` hand back a `Box::into_raw`.
// 3. **The re-entrancy that does exist is one level up**, in `manage_dec_ref.rs`,
//    where `WelsInitRefList`'s concealment prefetch takes a slot from this pool and
//    copies into it from `pPreviousDecodedPictureInDpb` — another slot of the same
//    array. That pair is enumerated at its own site (T5.C2), guarded by
//    `pRef == prev_pic`, and pinned by the `narrow_16x16_idr_lost` golden row.
// 4. **`FreePicture` is the one place ownership actually moves**, and it is
//    reachable only from `DestroyPicBuff` (which nulls the slot it just freed) and
//    from `decoder_core.rs:1899` for `pTempDec` (which nulls `pCtx->pTempDec`). No
//    other pointer to a freed picture survives either path — which is the same
//    property `Box::from_raw` needs and the reason it can be used here.



/// `len` bytes of `fill`, or `None` if the allocation fails.
///
/// The C's `WelsMallocz` returned null on failure and `AllocPicture`'s callers all
/// test for it; `vec![fill; len]` would abort the process instead. `try_reserve_exact`
/// keeps the C's contract, which is `RawDataBuffer::try_new_zeroed`'s answer to the
/// same question at T3.4 — and it matters more here, because a plane is megabytes.
fn try_filled(len: usize, fill: u8) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len).ok()?;
    buf.resize(len, fill);
    Some(buf)
}

/// Allocates and initializes an [`SPicture`] container with its three owned sample
/// planes and its macroblock tracking metadata arrays.
///
/// **T5.C3: the picture is heap-constructed, not `WelsMallocz`'d.** A struct with
/// owned fields cannot come out of a zeroing malloc (S21/F19), so the header is a
/// `Box` and the planes are [`PaddedPlane`]s. What has *not* moved is the geometry:
/// every expression below is `AllocPicture`'s own arithmetic, because the kernels'
/// output depends on it byte for byte and the goldens are the referee.
///
/// The per-macroblock metadata (`pMbCorrectlyDecodedFlag`, `pMbType`, `pMv`,
/// `pRefIndex`) is *still* raw and still allocated through `pMemAlign`; it is not
/// planes and it is not this session's. [`FreePicture`] frees it, and F19's check —
/// which line frees this? — is answered per allocation there.
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

    let planes: [PaddedPlane; 3] = if bParseOnly {
        // The C set `iLinesize[i]` from the geometry and left `pData[i]`/`pBuffer[i]`
        // null: a parse-only decode reconstructs nothing. Strides, no bytes.
        [
            PaddedPlane::empty(iPicWidth as usize),
            PaddedPlane::empty(iPicChromaWidth as usize),
            PaddedPlane::empty(iPicChromaWidth as usize),
        ]
    } else {
        // One `WelsMallocz` of `iLumaSize + 2*iChromaSize` filled with 128, carved
        // into three by pointer arithmetic, became three allocations each filled with
        // 128. Nothing walks from one plane into the next — `pBuffer[1]` and
        // `pBuffer[2]` were only ever bases for their own plane's `pData` — so the
        // contiguity was incidental, and every plane's own bytes are unchanged.
        let (Some(y), Some(u), Some(v)) = (
            try_filled(iLumaSize as usize, 128),
            try_filled(iChromaSize as usize, 128),
            try_filled(iChromaSize as usize, 128),
        ) else {
            return std::ptr::null_mut();
        };
        // `AllocPicture`'s own origin expressions, kept verbatim. Both are
        // `pad*stride + pad` — luma at pad 32, chroma at pad 16 — and `from_parts`
        // recovers the pad by division, so it *checks* that identity rather than
        // assuming it. It also checks that the C's allocation is tall enough for the
        // padded picture, which the row-count alignment makes true with room over.
        let origin_y = ((1 + iPicWidth) * PADDING_LENGTH) as usize;
        let origin_c = (((1 + iPicChromaWidth) * PADDING_LENGTH) >> 1) as usize;
        [
            PaddedPlane::from_parts(
                y,
                iPicWidth as usize,
                origin_y,
                kiPicWidth as usize,
                kiPicHeight as usize,
            ),
            PaddedPlane::from_parts(
                u,
                iPicChromaWidth as usize,
                origin_c,
                (kiPicWidth >> 1) as usize,
                (kiPicHeight >> 1) as usize,
            ),
            PaddedPlane::from_parts(
                v,
                iPicChromaWidth as usize,
                origin_c,
                (kiPicWidth >> 1) as usize,
                (kiPicHeight >> 1) as usize,
            ),
        ]
    };
    let _ = iPicChromaHeight;

    let pPic: PPicture = Box::into_raw(Box::new(SPicture::with_planes(planes)));

    unsafe {
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

/// Deallocates an [`SPicture`] instance and all its associated buffers.
///
/// **T5.C3: the matching half of [`AllocPicture`]'s heap construction.** The three
/// planes are freed by the `Box`'s drop glue — there is no `pBuffer[0]` arm any more,
/// and no way to forget one. The per-macroblock metadata arrays are still raw and are
/// still freed here, through the allocator that made them.
///
/// F19, per allocation, after the change: the `Box` at [`AllocPicture`]'s
/// `Box::into_raw` by the `Box::from_raw` at the bottom of this function; the three
/// plane `Vec`s by that same drop; the six metadata arrays by the six `WelsFree`
/// calls between here and there. Balanced, and two of the three groups are now
/// balanced by the type system rather than by inspection.
///
/// # Safety
/// - `pPic` must point to an [`SPicture`] produced by [`AllocPicture`] and not yet
///   freed, or be null. It is reclaimed with `Box::from_raw`, so it must have come
///   from that function's `Box::into_raw` and from nowhere else.
/// - `pMa` must point to the [`CMemoryAlign`] allocator used to allocate the
///   macroblock metadata arrays.
pub unsafe fn FreePicture(pPic: PPicture, pMa: *mut CMemoryAlign) {
    if pPic.is_null() || pMa.is_null() {
        return;
    }
    unsafe {
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

        // The picture header and, with it, the three plane allocations.
        drop(Box::from_raw(pPic));
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
            assert!(!(*p_pic).data_ptr(0).is_null());

            // T5.C3: the geometry the C computed is now the plane's, and pinning it
            // here is what makes "same arithmetic" a check rather than a claim.
            // stride = WELS_ALIGN(160 + 64, 32) = 224, rows = WELS_ALIGN(120 + 64, 32)
            // = 192, so the luma allocation is 224*192 and the padded picture needs
            // 224*(120+64) — the alignment leaves eight spare rows, and `from_parts`
            // accepts that.
            assert_eq!((*p_pic).linesize(0), 224);
            assert_eq!((*p_pic).linesize(1), 112);
            assert_eq!((*p_pic).plane(0).pad(), 32);
            assert_eq!((*p_pic).plane(1).pad(), 16);
            assert_eq!((*p_pic).plane(0).origin(), (1 + 224) * 32);
            assert_eq!((*p_pic).plane(1).origin(), ((1 + 112) * 32) >> 1);
            assert_eq!((*p_pic).plane(0).as_slice().len(), 224 * 192);
            assert_eq!((*p_pic).plane(1).as_slice().len(), 112 * 96);
            // The 128 fill covers the whole allocation, corners included — the EC
            // prefetch and `narrow_16x16_idr_lost` both depend on it.
            assert!((*p_pic).plane(0).as_slice().iter().all(|&b| b == 128));
            assert_eq!((*p_pic).plane(0).at(-32, -32), 128);

            FreePicture(p_pic, &mut ma as *mut CMemoryAlign);
        }
        assert_eq!(ma.WelsGetMemoryUsage(), 0);
    }

    /// The `bParseOnly` arm: strides from the geometry, no sample memory, and a null
    /// `data_ptr` — the three properties the C's null `pData[i]` beside a non-zero
    /// `iLinesize[i]` encoded, which every caller still tests with `.is_null()`.
    #[test]
    fn test_alloc_picture_parse_only_carries_strides_and_no_bytes() {
        let mut ma = CMemoryAlign::new(32);
        let mut param = SDecodingParam { bParseOnly: true, ..Default::default() };
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut ma as *mut CMemoryAlign;
        ctx.pParam = &mut param as *mut SDecodingParam;

        unsafe {
            let p_pic = AllocPicture(&mut *ctx as *mut SWelsDecoderContext, 160, 120);
            assert!(!p_pic.is_null());
            assert_eq!((*p_pic).linesize(0), 224);
            assert_eq!((*p_pic).linesize(1), 112);
            assert_eq!((*p_pic).linesize(2), 112);
            assert!((*p_pic).plane(0).is_empty());
            assert!((*p_pic).data_ptr(0).is_null());
            assert!((*p_pic).data_ptr(1).is_null());
            assert!((*p_pic).data_ptr(2).is_null());
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
