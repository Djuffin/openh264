//! The decoder's module tree.
//!
//! **T5.AC12: the lint is on every file below, and on this one.** All 22 modules
//! of `src/decoder/` carry `#![deny(unsafe_code)]` as of session AC; this file
//! declares them and holds no code, so its own lint is the tree's statement rather
//! than a check on anything here.

#![deny(unsafe_code)]

pub mod bit_stream;
pub mod cabac_decoder;
pub mod dec_golomb;
pub mod decode_mb_aux;
pub mod decode_slice;
pub mod decoder_context;
pub mod decoder_core;
pub mod error_concealment;
pub mod fmo;
pub mod get_intra_predictor;
pub mod manage_dec_ref;
pub mod mv_pred;
pub mod nalu;
pub mod parameter_sets;
pub mod parse_mb_syn_cabac;
pub mod parse_mb_syn_cavlc;
pub mod pic_queue;
pub mod picture;
pub mod slice;
pub mod deblocking;
pub mod vlc_tables;
