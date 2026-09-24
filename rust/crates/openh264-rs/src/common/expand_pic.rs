// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

// Copyright (c) 2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification,
// are permitted provided that the following conditions are met:
//
// * Redistributions of source code must retain the above copyright notice, this
//   list of conditions and the following disclaimer.
//
// * Redistributions in binary form must reproduce the above copyright notice, this
//   list of conditions and the following disclaimer in the documentation and/or
//   other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
// ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! Picture-border expansion, shared by encoder and decoder.
//!
//! `codec/common/inc/expand_pic.h`, `codec/common/src/expand_pic.cpp`.

// ============================================================================
// Safe kernel
// ============================================================================

/// C++: `ExpandPictureLuma_c` / `ExpandPictureChroma_c`,
/// `codec/common/src/expand_pic.cpp`.
///
/// Replicates the picture's border into the `pad`-pixel margin on all four
/// sides: the first/last rows are copied up/down `pad` times, each row's
/// first/last samples are smeared left/right, and the four corners take the
/// corner samples.
///
/// `buf` is the plane's full allocation — `(pic_h + 2*pad)` rows of `stride`
/// bytes or more — with the picture's `(0, 0)` at byte `pad * stride + pad`.
///
/// # Panics
/// If the geometry does not hold: `stride < pic_w + 2*pad`, a buffer shorter
/// than `(pic_h + 2*pad) * stride`, or `pic_w`/`pic_h` of zero.
pub fn expand_picture(buf: &mut [u8], stride: usize, pic_w: usize, pic_h: usize, pad: usize) {
    // The only two pads in use, specialised — see `expand_with`.
    match pad {
        PAD_LUMA => expand_with::<PAD_LUMA>(buf, stride, pic_w, pic_h),
        PAD_CHROMA => expand_with::<PAD_CHROMA>(buf, stride, pic_w, pic_h),
        _ => expand_any(buf, stride, pic_w, pic_h, pad),
    }
}

/// `PADDING_LENGTH`, and its chroma half — the only two pads
/// `ExpandPicture`'s call sites pass.
const PAD_LUMA: usize = 32;
const PAD_CHROMA: usize = 16;

/// [`expand_picture`] with the pad known at compile time. With `pad` a runtime
/// parameter each margin fill leaves a `memset` call, two of them inside the
/// per-row loop; a constant width lowers each to a couple of stores.
#[inline]
fn expand_with<const PAD: usize>(buf: &mut [u8], stride: usize, pic_w: usize, pic_h: usize) {
    expand_any(buf, stride, pic_w, pic_h, PAD);
}

/// The pad-as-a-parameter body. See [`expand_picture`] for the contract.
#[inline(always)]
fn expand_any(buf: &mut [u8], stride: usize, pic_w: usize, pic_h: usize, pad: usize) {
    assert!(pic_w > 0 && pic_h > 0, "cannot expand an empty picture");
    assert!(
        stride >= pic_w + 2 * pad,
        "stride {stride} cannot hold a padded row of {pic_w} + 2*{pad}"
    );
    assert!(
        buf.len() >= (pic_h + 2 * pad) * stride,
        "allocation of {} bytes cannot hold {} padded rows of {stride}",
        buf.len(),
        pic_h + 2 * pad
    );

    let origin = pad * stride + pad;
    let last = origin + (pic_h - 1) * stride;
    let tl = buf[origin];
    let tr = buf[origin + pic_w - 1];
    let bl = buf[last];
    let br = buf[last + pic_w - 1];

    // Pad the rows above and below, and their corners.
    for i in 0..pad {
        let strides = (1 + i) * stride;
        let top = origin - strides;
        let bottom = last + strides;

        buf.copy_within(origin..origin + pic_w, top);
        buf.copy_within(last..last + pic_w, bottom);

        buf[top - pad..top].fill(tl);
        buf[top + pic_w..top + pic_w + pad].fill(tr);
        buf[bottom - pad..bottom].fill(bl);
        buf[bottom + pic_w..bottom + pic_w + pad].fill(br);
    }

    // Pad left and right of every picture row.
    for r in 0..pic_h {
        let row = origin + r * stride;
        let left = buf[row];
        let right = buf[row + pic_w - 1];
        buf[row - pad..row].fill(left);
        buf[row + pic_w..row + pic_w + pad].fill(right);
    }
}
