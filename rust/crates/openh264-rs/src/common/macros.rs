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

//! Shared arithmetic, clamping, rounding, median, and alignment helpers
//! (`codec/common/inc/macros.h`).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

/// Clamps a signed 32-bit sample `iX` to the unsigned 8-bit pixel range `[0, 255]`.
///
/// C++: `WelsClip1` in `codec/common/inc/macros.h:168`.
#[inline(always)]
pub const fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        if iX < 0 { 0 } else { 255 }
    } else {
        iX as u8
    }
}

/// Three-operand clamp `((iX) < (iY) ? (iY) : ((iX) > (iZ) ? (iZ) : (iX)))`.
///
/// Note: Unlike `Ord::clamp`, this preserves the C++ ternary evaluation order and
/// returns `iY` without panicking when `iY > iZ` (relied upon by rate control in `rc.rs`).
#[inline(always)]
pub fn WELS_CLIP3<T: PartialOrd + Copy>(iX: T, iY: T, iZ: T) -> T {
    if iX < iY {
        iY
    } else if iX > iZ {
        iZ
    } else {
        iX
    }
}

/// `const fn` specialization of [`WELS_CLIP3`] for `i32`.
#[inline(always)]
pub const fn WELS_CLIP3_I32(iX: i32, iY: i32, iZ: i32) -> i32 {
    if iX < iY {
        iY
    } else if iX > iZ {
        iZ
    } else {
        iX
    }
}

/// Clamps a quantization parameter to `[0, 51]`.
#[inline(always)]
pub const fn CLIP3_QP_0_51(x: i32) -> i32 {
    WELS_CLIP3_I32(x, 0, 51)
}

/// Signed 32-bit absolute value `((a) > 0 ? (a) : -(a))`.
#[inline(always)]
pub const fn WELS_ABS(iX: i32) -> i32 {
    if iX > 0 { iX } else { -iX }
}

/// Power-of-two upward alignment `((x) + (n) - 1) & ~((n) - 1)`.
#[inline(always)]
pub const fn WELS_ALIGN(x: i32, n: i32) -> i32 {
    (x + n - 1) & !(n - 1)
}

/// `usize` power-of-two upward alignment.
#[inline(always)]
pub const fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
}

/// Generic minimum `((x) < (y) ? (x) : (y))`.
#[inline(always)]
pub fn WELS_MIN<T: PartialOrd + Copy>(x: T, y: T) -> T {
    if x < y { x } else { y }
}

/// Generic maximum `((x) > (y) ? (x) : (y))`.
#[inline(always)]
pub fn WELS_MAX<T: PartialOrd + Copy>(x: T, y: T) -> T {
    if x > y { x } else { y }
}

/// Non-negative `f64` to `i32` rounding (`(i32)(0.5 + (x))`).
#[inline(always)]
pub fn WELS_ROUND(x: f64) -> i32 {
    (0.5 + x) as i32
}

/// Non-negative `f32` to `i32` rounding (`(i32)(0.5 + (x))`).
#[inline(always)]
pub fn WELS_ROUND_f(x: f32) -> i32 {
    (0.5 + x) as i32
}

/// Non-negative `f64` to `i64` rounding (`(i64)(0.5 + (x))`).
#[inline(always)]
pub fn WELS_ROUND64(x: f64) -> i64 {
    (0.5 + x) as i64
}

/// Rounded integer division `((y) == 0 ? ((x) / ((y) + 1)) : (((y) / 2 + (x)) / (y)))`.
#[inline(always)]
pub const fn WELS_DIV_ROUND(x: i32, y: i32) -> i32 {
    if y == 0 { x / (y + 1) } else { (y / 2 + x) / y }
}

/// 64-bit rounded integer division `((y) == 0 ? ((x) / ((y) + 1)) : (((y) / 2 + (x)) / (y)))`.
#[inline(always)]
pub const fn WELS_DIV_ROUND64(x: i64, y: i64) -> i64 {
    if y == 0 { x / (y + 1) } else { (y / 2 + x) / y }
}

/// Median of three values (`WelsMedian` in `codec/common/inc/macros.h`).
#[inline(always)]
pub fn WelsMedian<T: Ord + Copy>(a: T, b: T, c: T) -> T {
    let (min_ab, max_ab) = if a <= b { (a, b) } else { (b, a) };
    if c < min_ab {
        min_ab
    } else if c > max_ab {
        max_ab
    } else {
        c
    }
}
