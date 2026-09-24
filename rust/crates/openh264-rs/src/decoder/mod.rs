// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

//! The decoder's module tree.

#![forbid(unsafe_code)]

pub mod bit_stream;
pub mod cabac_decoder;
pub mod deblocking;
pub mod dec_golomb;
pub mod decode_mb_aux;
pub(crate) mod decode_slice;
pub mod decoder_context;
pub(crate) mod decoder_core;
pub mod error_concealment;
pub mod fmo;
pub mod get_intra_predictor;
pub mod manage_dec_ref;
pub mod mv_pred;
pub(crate) mod nalu;
pub mod parameter_sets;
pub mod parse_mb_syn_cabac;
pub mod parse_mb_syn_cavlc;
pub(crate) mod pic_queue;
pub mod picture;
pub(crate) mod slice;
pub mod vlc_tables;
