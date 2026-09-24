// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

#![forbid(unsafe_code)]
//! Scalar forwards for the `score` kernels — see the module header.

#[inline(always)]
pub fn calculate_single_ctr_4x4(dct: &[i16; 16]) -> i32 {
    crate::encoder::encode_mb_aux::calculate_single_ctr_4x4(dct)
}
