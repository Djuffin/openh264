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

//! # Fixed-shape macroblock copies (`copy_mb.h` / `copy_mb.cpp`)
//!
//! Translated from `codec/common/inc/copy_mb.h` and
//! `codec/common/src/copy_mb.cpp`.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]
#![forbid(unsafe_code)]
#![deny(unsafe_code)]

use crate::common::mc::copy_rows;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, RefSamples};

/// C++: `WelsCopy4x4_c`.
#[inline(always)]
pub fn copy_4x4<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>) {
    copy_rows::<4, _>(src, dst, 4);
}

/// C++: `WelsCopy8x4_c`.
#[inline(always)]
pub fn copy_8x4<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>) {
    copy_rows::<8, _>(src, dst, 4);
}

/// C++: `WelsCopy4x8_c`.
#[inline(always)]
pub fn copy_4x8<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>) {
    copy_rows::<4, _>(src, dst, 8);
}

/// C++: `WelsCopy8x8_c`.
#[inline(always)]
pub fn copy_8x8<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>) {
    copy_rows::<8, _>(src, dst, 8);
}

/// C++: `WelsCopy16x8_c`.
#[inline(always)]
pub fn copy_16x8<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>) {
    copy_rows::<16, _>(src, dst, 8);
}

/// C++: `WelsCopy8x16_c`.
#[inline(always)]
pub fn copy_8x16<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>) {
    copy_rows::<8, _>(src, dst, 16);
}

/// C++: `WelsCopy16x16_c`.
#[inline(always)]
pub fn copy_16x16<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>) {
    copy_rows::<16, _>(src, dst, 16);
}
