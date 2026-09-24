// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

//! x86_64 SIMD implementations (SSE2, SSSE3, SSE4.1, AVX2).
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
