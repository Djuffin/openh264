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
//!
//! **This module exists because of where the C++ puts these functions**
//! (T5.AB2; steward's ruling at `6b6dd9a3`, `phase5_session_ab.md` §2). The
//! seven `WelsCopy*_c` kernels are `common/`'s in the C++ and both codecs
//! include the same header, but the port had translated them **twice**: once in
//! `encoder/encode_mb_aux.rs` as Phase-2 shims over these safe kernels, and once
//! in `decoder/error_concealment.rs` as raw row loops. F43's own class — one C++
//! function with two ports, one converted and one not — found by W7's straggler
//! sweep at session AA's close and not fixable there, because the safe kernels
//! were stranded in `encoder/` and the decoder cannot import from it.
//!
//! Moving the definitions here restores F22's rule (*home = the C++'s home*)
//! rather than bending F12/P10: **no encoder site is converted**, only import
//! spellings change on the encoder side, and the decoder's two raw loops become
//! calls to the kernels the encoder was already calling.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]
#![forbid(unsafe_code)]

//! **T9.X — this module denies.** The seven kernels were always safe; the one
//! remaining raw body is [`copy_shim`], the C-ABI-shaped span constructor the
//! encoder's seven `WelsCopy*_c` entry points share, and it is tagged at the item.
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

// **S9.0c: `copy_shim` is gone, and this file seals with it.** It built exact-reach
// slices out of two raw pointers for the seven `WelsCopyNxM_c` shims. Those shims
// take `RecCursor`s now — the dispatch slot carries no raw pointer — so the only
// `unsafe` this module ever had retires with its single caller.
