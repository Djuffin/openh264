// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

//! Low-level C ABI type definitions matching OpenH264 C interface.
#![deny(unsafe_code)]
// Identifiers keep the C++ names.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(
    dead_code,
    unused_variables,
    unused_imports,
    unused_qualifications,
    trivial_numeric_casts,
    unused_assignments,
    unused_mut,
    unreachable_patterns,
    unused_parens,
    unsafe_op_in_unsafe_fn,
    unused_unsafe
)]

pub mod api;
pub mod common;
pub mod decoder;
pub mod encoder;
pub mod processing;
pub mod safe;
pub mod simd;

pub use crate::api::codec_api::*;

#[derive(Clone, Debug)]
pub struct AnnexBUnits<'a> {
    bitstream: &'a [u8],
    curr: Option<(usize, usize)>,
}

impl<'a> AnnexBUnits<'a> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.curr.is_none()
    }
}

#[inline]
fn find_start_code(bitstream: &[u8], mut i: usize) -> Option<(usize, usize)> {
    let len = bitstream.len();
    while i + 2 < len {
        if bitstream[i] == 0 && bitstream[i + 1] == 0 {
            if bitstream[i + 2] == 1 {
                return Some((i, i + 3));
            } else if i + 3 < len && bitstream[i + 2] == 0 && bitstream[i + 3] == 1 {
                return Some((i, i + 4));
            }
        }
        if let Some(pos) = bitstream[i + 1..].iter().position(|&b| b == 0) {
            i += 1 + pos;
        } else {
            break;
        }
    }
    None
}

impl<'a> Iterator for AnnexBUnits<'a> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let (start, scan_from) = self.curr?;
        let next = find_start_code(self.bitstream, scan_from);
        self.curr = next;
        let end = match next {
            Some((next_start, _)) => next_start,
            None => self.bitstream.len(),
        };
        Some(&self.bitstream[start..end])
    }
}

pub fn split_annexb_units(bitstream: &[u8]) -> AnnexBUnits<'_> {
    AnnexBUnits {
        bitstream,
        curr: find_start_code(bitstream, 0),
    }
}
