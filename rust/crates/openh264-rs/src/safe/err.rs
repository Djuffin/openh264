#![forbid(unsafe_code)]

//! Error plumbing for the safe vocabulary types.
//!
//! Deliberately minimal: the codec keeps returning the C++ `int32_t` error codes
//! internally, so this is a transparent newtype over exactly those codes and
//! nothing more. No hierarchy, no `Display`, no conversion layer. Its whole job is
//! to keep a `Result` shape at the call sites, so that "forgot to check the error"
//! stops being possible while the *values* stay bit-identical to the C++.
//!
//! The codes themselves are **reused, never redefined**.

use crate::decoder::bit_stream;
use crate::decoder::dec_golomb;

/// A decoder error code, exactly as the C++ returns it.
///
/// `ErrInfo(0)` is never constructed: success is `Ok` (`ERR_NONE` is the absence of
/// an error, not a value of this type).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
#[repr(transparent)]
pub struct ErrInfo(pub i32);

impl ErrInfo {
    /// `ERR_INFO_INVALID_ACCESS` — a cursor was initialised outside its buffer.
    pub const INVALID_ACCESS: Self = Self(bit_stream::ERR_INFO_INVALID_ACCESS);
    /// `ERR_INFO_READ_OVERFLOW` — the read cursor ran past the end of the RBSP.
    pub const READ_OVERFLOW: Self = Self(bit_stream::ERR_INFO_READ_OVERFLOW);
    /// `ERR_INFO_READ_LEADING_ZERO` — an Exp-Golomb prefix of 32+ zero bits.
    pub const READ_LEADING_ZERO: Self = Self(dec_golomb::ERR_INFO_READ_LEADING_ZERO);

    /// The raw `int32_t` the C++ would have returned.
    #[inline]
    pub const fn code(self) -> i32 {
        self.0
    }
}

impl From<ErrInfo> for i32 {
    #[inline]
    fn from(e: ErrInfo) -> i32 {
        e.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_agree_with_the_duplicate_definitions() {
        // `decoder/bit_stream.rs` and `decoder/dec_golomb.rs` each declare their own
        // copy of these constants. This port has shipped duplicated constants
        // holding *different* values; this test is the tripwire for that happening
        // to the codes the safe reader returns.
        assert_eq!(
            bit_stream::ERR_INFO_INVALID_ACCESS,
            dec_golomb::ERR_INFO_INVALID_ACCESS
        );
        assert_eq!(
            bit_stream::ERR_INFO_READ_OVERFLOW,
            dec_golomb::ERR_INFO_READ_OVERFLOW
        );
        assert_eq!(bit_stream::ERR_NONE, dec_golomb::ERR_NONE);
    }

    #[test]
    fn codes_match_the_cpp_error_code_h_values() {
        // codec/decoder/core/inc/error_code.h: ERR_INFO_COMMON_BASE = 1.
        assert_eq!(ErrInfo::INVALID_ACCESS.code(), 2);
        assert_eq!(ErrInfo::READ_OVERFLOW.code(), 11);
        assert_eq!(ErrInfo::READ_LEADING_ZERO.code(), 12);
    }

    #[test]
    fn converts_to_the_raw_code() {
        let e = ErrInfo::READ_OVERFLOW;
        let raw: i32 = e.into();
        assert_eq!(raw, bit_stream::ERR_INFO_READ_OVERFLOW);
        assert_eq!(e.code(), raw);
    }
}
