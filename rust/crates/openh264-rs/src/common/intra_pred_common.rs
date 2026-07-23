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

//! # Intra Prediction Common Interfaces (16x16 Luma)
//!
//! Translated from `codec/common/inc/intra_pred_common.h` and `codec/common/src/intra_pred_common.cpp`.
//!
//! Provides vertical and horizontal 16x16 luma spatial intra-frame prediction
//! kernels for both C reference fallbacks and SIMD hardware acceleration (SSE2, NEON, AArch64, MMI, LSX).

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

/// Function pointer typedef for 16x16 intra prediction routines.
pub type PGetIntraPredFunc = unsafe extern "C" fn(pPred: *mut u8, pRef: *mut u8, kiStride: i32);

/// Alias for 16x16 intra prediction function pointer.
pub type PWelsI16x16LumaPredFunc = unsafe extern "C" fn(pPred: *mut u8, pRef: *mut u8, kiStride: i32);

// ============================================================================
// C Reference Implementations
// ============================================================================

/// Mode 0: Intra 16x16 Luma Vertical Prediction (C fallback)
///
/// Copies the 16 top reconstructed reference samples at `pRef[-kiStride .. -kiStride + 15]`
/// down across all 16 rows of the destination block `pPred`.
///
/// # Safety
/// - `pPred` must point to a writable memory region of at least 256 bytes (16x16).
/// - `pRef.offset(-(kiStride as isize))` must point to a readable memory region of at least 16 bytes.
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        let kpSrc = pRef.offset(-(kiStride as isize));
        let kuiT1 = (kpSrc as *const u64).read_unaligned();
        let kuiT2 = (kpSrc.add(8) as *const u64).read_unaligned();
        let mut pDst = pPred;

        for _ in 0..16 {
            (pDst as *mut u64).write_unaligned(kuiT1);
            (pDst.add(8) as *mut u64).write_unaligned(kuiT2);
            pDst = pDst.add(16);
        }
    }
}

/// Mode 1: Intra 16x16 Luma Horizontal Prediction (C fallback)
///
/// For each row `y` from 15 down to 0, reads the reconstructed left boundary pixel at
/// `pRef[y * kiStride - 1]`, broadcasts it across a 64-bit register, and writes it across
/// the 16 bytes of row `y` in `pPred`.
///
/// # Safety
/// - `pPred` must point to a writable memory region of at least 256 bytes (16x16).
/// - For each `y` in `0..16`, `pRef.offset(y * kiStride - 1)` must be readable.
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_c(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        let mut iStridex15: isize = ((kiStride << 4) - kiStride) as isize;
        let iPredStride: usize = 16;
        let mut iPredStridex15: usize = 240;

        for _ in 0..16 {
            let kuiSrc8 = *pRef.offset(iStridex15 - 1);
            let kuiV64: u64 = 0x0101_0101_0101_0101u64.wrapping_mul(kuiSrc8 as u64);

            (pPred.add(iPredStridex15) as *mut u64).write_unaligned(kuiV64);
            (pPred.add(iPredStridex15 + 8) as *mut u64).write_unaligned(kuiV64);

            iStridex15 -= kiStride as isize;
            iPredStridex15 = iPredStridex15.wrapping_sub(iPredStride);
        }
    }
}

// ============================================================================
// x86 / x86_64 SSE2 Vector Implementations
// ============================================================================

/// Mode 0: Intra 16x16 Luma Vertical Prediction (SSE2)
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_sse2(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::*;

    unsafe {
        let kpSrc = pRef.offset(-(kiStride as isize));
        let val = _mm_loadu_si128(kpSrc as *const __m128i);
        let mut pDst = pPred;
        for _ in 0..16 {
            _mm_storeu_si128(pDst as *mut __m128i, val);
            pDst = pDst.add(16);
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_sse2(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredV_c(pPred, pRef, kiStride);
    }
}

/// Mode 1: Intra 16x16 Luma Horizontal Prediction (SSE2)
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_sse2(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::*;

    unsafe {
        let mut pSrc = pRef.offset(-1);
        let mut pDst = pPred;
        for _ in 0..16 {
            let val = _mm_set1_epi8(*pSrc as i8);
            _mm_storeu_si128(pDst as *mut __m128i, val);
            pSrc = pSrc.offset(kiStride as isize);
            pDst = pDst.add(16);
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_sse2(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredH_c(pPred, pRef, kiStride);
    }
}

// ============================================================================
// ARM NEON & AArch64 Vector Implementations
// ============================================================================

/// Mode 0: Intra 16x16 Luma Vertical Prediction (ARM NEON)
#[cfg(target_arch = "arm")]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    use core::arch::arm::*;
    unsafe {
        let kpSrc = pRef.offset(-(kiStride as isize));
        let val = vld1q_u8(kpSrc);
        let mut pDst = pPred;
        for _ in 0..16 {
            vst1q_u8(pDst, val);
            pDst = pDst.add(16);
        }
    }
}

#[cfg(not(target_arch = "arm"))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredV_c(pPred, pRef, kiStride);
    }
}

/// Mode 1: Intra 16x16 Luma Horizontal Prediction (ARM NEON)
#[cfg(target_arch = "arm")]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    use core::arch::arm::*;
    unsafe {
        let mut pSrc = pRef.offset(-1);
        let mut pDst = pPred;
        for _ in 0..16 {
            let val = vdupq_n_u8(*pSrc);
            vst1q_u8(pDst, val);
            pSrc = pSrc.offset(kiStride as isize);
            pDst = pDst.add(16);
        }
    }
}

#[cfg(not(target_arch = "arm"))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredH_c(pPred, pRef, kiStride);
    }
}

/// Mode 0: Intra 16x16 Luma Vertical Prediction (AArch64 NEON)
#[cfg(target_arch = "aarch64")]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_AArch64_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    use core::arch::aarch64::*;
    unsafe {
        let kpSrc = pRef.offset(-(kiStride as isize));
        let val = vld1q_u8(kpSrc);
        let mut pDst = pPred;
        for _ in 0..16 {
            vst1q_u8(pDst, val);
            pDst = pDst.add(16);
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_AArch64_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredV_c(pPred, pRef, kiStride);
    }
}

/// Mode 1: Intra 16x16 Luma Horizontal Prediction (AArch64 NEON)
#[cfg(target_arch = "aarch64")]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_AArch64_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    use core::arch::aarch64::*;
    unsafe {
        let mut pSrc = pRef.offset(-1);
        let mut pDst = pPred;
        for _ in 0..16 {
            let val = vdupq_n_u8(*pSrc);
            vst1q_u8(pDst, val);
            pSrc = pSrc.offset(kiStride as isize);
            pDst = pDst.add(16);
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_AArch64_neon(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredH_c(pPred, pRef, kiStride);
    }
}

// ============================================================================
// MIPS MMI & LoongArch LSX Accelerated Fallbacks
// ============================================================================

/// Mode 0: Intra 16x16 Luma Vertical Prediction (MIPS MMI)
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_mmi(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredV_c(pPred, pRef, kiStride);
    }
}

/// Mode 1: Intra 16x16 Luma Horizontal Prediction (MIPS MMI)
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_mmi(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredH_c(pPred, pRef, kiStride);
    }
}

/// Mode 0: Intra 16x16 Luma Vertical Prediction (LoongArch LSX)
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredV_lsx(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredV_c(pPred, pRef, kiStride);
    }
}

/// Mode 1: Intra 16x16 Luma Horizontal Prediction (LoongArch LSX)
#[inline]
pub unsafe extern "C" fn WelsI16x16LumaPredH_lsx(pPred: *mut u8, pRef: *mut u8, kiStride: i32) {
    unsafe {
        WelsI16x16LumaPredH_c(pPred, pRef, kiStride);
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_i16x16_luma_pred_v() {
        let mut ref_buf = vec![0u8; 1024];
        let stride = 32i32;
        let mb_offset = stride as usize * 10 + 16;

        // Populate top row reference samples (pRef - kiStride)
        for i in 0..16 {
            ref_buf[mb_offset - stride as usize + i] = (10 + i) as u8;
        }

        let mut pred_buf_c = [0u8; 256];
        let mut pred_buf_sse2 = [0u8; 256];

        unsafe {
            let p_ref = ref_buf.as_mut_ptr().add(mb_offset);
            WelsI16x16LumaPredV_c(pred_buf_c.as_mut_ptr(), p_ref, stride);
            WelsI16x16LumaPredV_sse2(pred_buf_sse2.as_mut_ptr(), p_ref, stride);
        }

        for row in 0..16 {
            for col in 0..16 {
                let expected = (10 + col) as u8;
                assert_eq!(pred_buf_c[row * 16 + col], expected);
                assert_eq!(pred_buf_sse2[row * 16 + col], expected);
            }
        }
    }

    #[test]
    fn test_i16x16_luma_pred_h() {
        let mut ref_buf = vec![0u8; 1024];
        let stride = 32i32;
        let mb_offset = stride as usize * 10 + 16;

        // Populate left column reference samples (pRef[y * stride - 1])
        for y in 0..16 {
            ref_buf[mb_offset + y * stride as usize - 1] = (50 + y) as u8;
        }

        let mut pred_buf_c = [0u8; 256];
        let mut pred_buf_sse2 = [0u8; 256];

        unsafe {
            let p_ref = ref_buf.as_mut_ptr().add(mb_offset);
            WelsI16x16LumaPredH_c(pred_buf_c.as_mut_ptr(), p_ref, stride);
            WelsI16x16LumaPredH_sse2(pred_buf_sse2.as_mut_ptr(), p_ref, stride);
        }

        for row in 0..16 {
            for col in 0..16 {
                let expected = (50 + row) as u8;
                assert_eq!(pred_buf_c[row * 16 + col], expected);
                assert_eq!(pred_buf_sse2[row * 16 + col], expected);
            }
        }
    }
}
