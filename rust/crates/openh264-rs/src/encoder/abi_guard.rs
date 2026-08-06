#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Compile-time size checks for encoder-internal structs.
//!
//! These are the internal counterpart to `api/abi_guard.rs`, which guards the public
//! C ABI. Nothing outside the crate depends on these layouts, but the port's rule is
//! that every `#[repr(C)]` struct is a statement-for-statement translation of a C++
//! one, and a size mismatch means a field was added, dropped, or given the wrong width.
//!
//! The expected values were produced by compiling a `sizeof` dump against
//! `codec/encoder/core/inc/*.h` on darwin/arm64 (LP64); they hold on any LP64 target.
//!
//! This file exists because `SWelsPPS` had drifted to roughly nine times its real size:
//! the port had transcribed the nine FMO fields that live inside
//! `#if !defined(DISABLE_FMO_FEATURE)`, and `as264_common.h:53` defines that macro
//! unconditionally, so they are not in the struct the C++ encoder actually compiles.
//! A size assertion catches that class of mistake the moment it is written.

use std::mem::size_of;

use crate::common::wels_common_defs::{SBitStringAux, SNalUnitHeader, SNalUnitHeaderExt};
use crate::encoder::encoder_context::{SCropOffset, SDCTCoeff, SMVComponentUnit, SMVUnitXY};
use crate::encoder::nal_encap::{SWelsEncoderOutput, SWelsNalRaw, SWelsSliceBs};
use crate::encoder::param_svc::{SSpsSvcExt, SSubsetSps, SWelsPPS, SWelsSPS};
use crate::encoder::picture::{SPicture, SScreenBlockFeatureStorage};

macro_rules! assert_size {
    ($t:ty, $n:expr) => {
        const _: () = assert!(
            size_of::<$t>() == $n,
            concat!(stringify!($t), " must match the C++ struct size"),
        );
    };
}

// codec/common/inc/wels_common_defs.h
assert_size!(SBitStringAux, 48);
assert_size!(SNalUnitHeader, 12);
assert_size!(SNalUnitHeaderExt, 24);

// codec/encoder/core/inc/nal_encap.h
assert_size!(SWelsNalRaw, 40);
assert_size!(SWelsSliceBs, 176);
assert_size!(SWelsEncoderOutput, 96);

// codec/encoder/core/inc/picture.h
assert_size!(SPicture, 136);
assert_size!(SScreenBlockFeatureStorage, 88);

// codec/encoder/core/inc/parameter_sets.h
assert_size!(SWelsSPS, 56);
assert_size!(SWelsPPS, 16);
assert_size!(SSpsSvcExt, 4);
assert_size!(SSubsetSps, 60);

// codec/encoder/core/inc/wels_common_basis.h, mb_cache.h
assert_size!(SMVUnitXY, 4);
assert_size!(SCropOffset, 8);
assert_size!(SDCTCoeff, 816);
assert_size!(SMVComponentUnit, 146);
