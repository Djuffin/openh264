// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

//! The scalar kernel set — every entry point of `simd::x86_64` and `simd::aarch64`,
//! forwarding to the scalar body the codec would otherwise have called directly.
//!
//! Compiled only under `--features scalar` or on a target with no vector kernels (off
//! x86_64 and aarch64, or under Miri on aarch64), where it is what [`super::kernels`]
//! names, so it can never shadow a real kernel. One `#[inline(always)]` forward each:
//! same name, same signature.

#![forbid(unsafe_code)]
#![allow(non_snake_case)]

pub mod copy;
pub mod dct;
pub mod deblock;
pub mod intra_pred;
pub mod mc;
pub mod me;
pub mod quant;
pub mod sad;
pub mod satd;
pub mod score;
pub mod vaa;
