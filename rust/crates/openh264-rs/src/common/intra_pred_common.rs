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
//! `codec/common/inc/intra_pred_common.h`, `codec/common/src/intra_pred_common.cpp`.
//!
//! Vertical and horizontal 16x16 luma spatial intra-frame prediction kernels.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

// ============================================================================
// Safe kernels
// ============================================================================

// Both kernels write a **packed** 16x16 block: the destination advances by 16 per row,
// and any stride describes the reference surface only. The same-named 2-arg kernels in
// `decoder/get_intra_predictor.rs` instead predict in place on a strided plane — hence
// `[u8; 256]` here where the decoder side takes a `PlaneCursorMut`.
//
// The reference shapes differ because the reaches differ: V reads the sixteen samples of
// the row above, H reads one sample from each of sixteen rows in the column to the left.

use crate::safe::plane::RefSamples;

/// C++: `WelsI16x16LumaPredV_c`, `codec/common/src/intra_pred_common.cpp`.
///
/// Copies the sixteen reconstructed samples above the macroblock down all sixteen rows.
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
#[inline(always)]
pub fn i16x16_luma_pred_h(pred: &mut [u8; 256], reference: &impl RefSamples) {
    for y in 0..16 {
        let v = reference.at(-1, y as isize);
        let row: &mut [u8; 16] = (&mut pred[y * 16..][..16]).try_into().unwrap();
        row.fill(v);
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::plane::PaddedPlane;

    #[test]
    fn test_i16x16_luma_pred_v() {
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
