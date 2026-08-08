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
// Safe kernels
// ============================================================================

// Both kernels write a **packed** 16x16 block: `pPred` advances by a literal 16 per
// row, and `kiStride` describes the reference surface only. That is what separates
// these two from their same-named 2-arg cousins in `decoder/get_intra_predictor.rs`
// (converted in T3), which predict in place on a strided plane. Same Wels names,
// different functions, and they must never be unified — hence `[u8; 256]` here where
// the decoder side takes a `PlaneCursorMut`.
//
// The two also take *different* reference shapes rather than a common one, because
// their reaches genuinely differ: V reads the sixteen samples of the row above and
// nothing else, H reads one sample from each of sixteen rows in the column to the
// left. A shared `(cursor)` parameter would make each kernel's contract claim the
// other's reach — the union-span mistake T4 recorded (`safety_refactor_log.md`,
// "per-kernel reach, not the union").

use crate::safe::plane::PlaneCursor;

/// C++: `WelsI16x16LumaPredV_c`, `codec/common/src/intra_pred_common.cpp`.
///
/// Copies the sixteen reconstructed samples above the macroblock down all sixteen
/// rows. `top` is the caller's proof that the row above exists — in Phase 5 it is
/// `refc.row(-1, 0, 16)`.
#[inline(always)]
pub fn i16x16_luma_pred_v(pred: &mut [u8; 256], top: &[u8; 16]) {
    for y in 0..16 {
        let row: &mut [u8; 16] = (&mut pred[y * 16..][..16]).try_into().unwrap();
        *row = *top;
    }
}

/// C++: `WelsI16x16LumaPredH_c`, `codec/common/src/intra_pred_common.cpp`.
///
/// Broadcasts the reconstructed sample left of each row across that row. Reads `x` at
/// `-1` for `y` in `0 .. 16` from `reference`, and nothing else.
///
/// The C++ walks rows 15 down to 0 because it carries two descending offsets; each row
/// is written once from an input the block does not contain, so ascending is the same
/// sixteen writes in a different order. `fill` replaces the `0x0101010101010101 *
/// value` broadcast — that trick was never a memory operation, only a way to spell a
/// wide store (`prompts/phase2.md` §3.3, taxonomy T7).
#[inline(always)]
pub fn i16x16_luma_pred_h(pred: &mut [u8; 256], reference: &PlaneCursor<'_>) {
    for y in 0..16 {
        let v = reference.at(-1, y as isize);
        let row: &mut [u8; 16] = (&mut pred[y * 16..][..16]).try_into().unwrap();
        row.fill(v);
    }
}

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
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
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

        unsafe {
            let p_ref = ref_buf.as_mut_ptr().add(mb_offset);
            WelsI16x16LumaPredV_c(pred_buf_c.as_mut_ptr(), p_ref, stride);
        }

        for row in 0..16 {
            for col in 0..16 {
                let expected = (10 + col) as u8;
                assert_eq!(pred_buf_c[row * 16 + col], expected);
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

        unsafe {
            let p_ref = ref_buf.as_mut_ptr().add(mb_offset);
            WelsI16x16LumaPredH_c(pred_buf_c.as_mut_ptr(), p_ref, stride);
        }

        for row in 0..16 {
            for col in 0..16 {
                let expected = (50 + row) as u8;
                assert_eq!(pred_buf_c[row * 16 + col], expected);
            }
        }
    }
}
