//! Low-level C ABI type definitions matching OpenH264 C interface.
#![deny(unsafe_code)]
// The naming allows are a requirement, not debt: this crate is a line-by-line port
// and every identifier is diffable against the C++ it came from (`CODING_STYLE`).
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
// The first two are warn-by-default and were blanket-allowed per module while the
// port was landing files that only later gained callers; that phase is over, so a
// body with no caller and a parameter with no reader are findings again. The last
// three are allow-by-default and are on deliberately: this is a line-by-line port,
// and an import nothing uses, a path spelled out where the name is already in
// scope, and a cast from a type to itself are all residue of moving a C++ file
// across rather than anything the Rust needs.
#![warn(
    dead_code,
    unused_variables,
    unused_imports,
    unused_qualifications,
    trivial_numeric_casts
)]
#![deny(
    unused_assignments,
    unused_mut,
    unreachable_patterns,
    unused_parens
)]


pub mod common;
pub mod decoder;
pub mod encoder;
pub mod processing;
pub mod api;
pub mod safe;
pub mod simd;

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

