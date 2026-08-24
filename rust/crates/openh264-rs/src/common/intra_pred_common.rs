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
// **T9.C2**: the file's last raw pointer left with the two `(pPred, pRef, kiStride)`
// shims, so the port's safety floor applies here now.
#![deny(unsafe_code)]

// `PGetIntraPredFunc` and `PWelsI16x16LumaPredFunc` stood here — two names for one
// `unsafe extern "C" fn(*mut u8, *mut u8, i32)`, deleted with the shims they
// described (T9.C2). Nothing outside this file ever named either: the encoder has
// its own three types, now safe and split by prediction size, and the decoder's
// `PGetIntraPredFunc` is a different signature entirely (`decoder_context.rs:244`).

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

use crate::safe::plane::{PlaneCursor, RefSamples};

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
pub fn i16x16_luma_pred_h(pred: &mut [u8; 256], reference: &impl RefSamples) {
    for y in 0..16 {
        let v = reference.at(-1, y as isize);
        let row: &mut [u8; 16] = (&mut pred[y * 16..][..16]).try_into().unwrap();
        row.fill(v);
    }
}

// ============================================================================
// C Reference Implementations
// ============================================================================

// **The two `(pPred, pRef, kiStride)` shims stood here — deleted in T9.C2.**
//
// They were this file's only unsafe: `from_raw_parts` over a caller-promised
// reach, once per prediction mode, under a hand-written `# Safety` contract. The
// encoder's intra-prediction tables were their only callers, and those tables now
// hold safe `fn(&mut [u8; N], &RecCursor<'_>)` — so the adapters moved to
// `encoder/get_intra_predictor.rs`, beside the other twenty-six, where the
// encoder-side `RecCursor` belongs. `common` gains no dependency on `encoder`,
// and this file reaches `#![deny(unsafe_code)]`.
//
// The kernels themselves are unchanged and still live above: `i16x16_luma_pred_v`
// takes the sixteen samples above the block by value, `i16x16_luma_pred_h` reads
// its left column through `RefSamples`.

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::plane::PaddedPlane;
    
    #[test]
    fn test_i16x16_luma_pred_v() {
        // Was driven through the raw shim; drives the kernel directly since T9.C2,
        // over an ordinary `PaddedPlane` — which is what the `PlaneCursor` half of
        // `RefSamples` is for, and what keeps these kernels testable without a
        // reconstruction picture.
        let mut plane = PaddedPlane::new(16, 16, 8, 32);
        for i in 0..16isize {
            plane.set(i, -1, (10 + i) as u8);
        }

        let mut pred_buf_c = [0u8; 256];
        let top: [u8; 16] = core::array::from_fn(|i| plane.at(i as isize, -1));
        i16x16_luma_pred_v(&mut pred_buf_c, &top);

        for row in 0..16 {
            for col in 0..16 {
                let expected = (10 + col) as u8;
                assert_eq!(pred_buf_c[row * 16 + col], expected);
            }
        }
    }

    #[test]
    fn test_i16x16_luma_pred_h() {
        let mut plane = PaddedPlane::new(16, 16, 8, 32);
        for y in 0..16isize {
            plane.set(-1, y, (50 + y) as u8);
        }

        let mut pred_buf_c = [0u8; 256];
        i16x16_luma_pred_h(&mut pred_buf_c, &plane.cursor(0, 0));

        for row in 0..16 {
            for col in 0..16 {
                let expected = (50 + row) as u8;
                assert_eq!(pred_buf_c[row * 16 + col], expected);
            }
        }
    }
}
