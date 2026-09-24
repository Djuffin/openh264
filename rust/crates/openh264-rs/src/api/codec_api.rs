// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

#![forbid(unsafe_code)]
//! OpenH264 Public API Architecture (`codec_api.h`).
//!
//! Facade module re-exporting all items from:
//! - [`super::types`]: Shared codec constants, return codes, options, enums, and structs.
//! - [`super::encoder`]: Safe Rust `Encoder` core.
//! - [`super::decoder`]: Safe Rust `Decoder` core and display reordering buffer.
//! - [`super::c_api`]: Raw C ABI vtables (`ISVCEncoderVtbl`, `ISVCDecoderVtbl`), thunks, and `#[no_mangle]` exports.

pub use super::c_api::*;
pub use super::decoder::*;
pub use super::encoder::*;
pub use super::types::*;

#[cfg(test)]
pub(crate) use super::c_api::abi_test_driver;
