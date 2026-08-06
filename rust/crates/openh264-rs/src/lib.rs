//! Low-level C ABI type definitions matching OpenH264 C interface.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_imports,
    unsafe_op_in_unsafe_fn
)]

use std::ffi::c_void;

pub mod common;
pub mod decoder;
pub mod encoder;
pub mod processing;
pub mod api;

pub use crate::api::codec_api::*;

pub fn split_annexb_units(bitstream: &[u8]) -> Vec<&[u8]> {
    let mut start_indices = Vec::new();
    let mut i = 0;
    let len = bitstream.len();
    while i + 2 < len {
        if bitstream[i] == 0 && bitstream[i + 1] == 0 {
            if bitstream[i + 2] == 1 {
                start_indices.push(i);
                i += 3;
                continue;
            } else if i + 3 < len && bitstream[i + 2] == 0 && bitstream[i + 3] == 1 {
                start_indices.push(i);
                i += 4;
                continue;
            }
        }
        if let Some(pos) = bitstream[i + 1..].iter().position(|&b| b == 0) {
            i += 1 + pos;
        } else {
            break;
        }
    }

    let mut units = Vec::with_capacity(start_indices.len());
    for idx in 0..start_indices.len() {
        let start = start_indices[idx];
        let end = if idx + 1 < start_indices.len() {
            start_indices[idx + 1]
        } else {
            len
        };
        units.push(&bitstream[start..end]);
    }
    units
}

