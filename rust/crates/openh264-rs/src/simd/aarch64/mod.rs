//! The aarch64 NEON kernel set — every entry point of `super::x86_64`, transcribed
//! from upstream's own arm64 assembly in `codec/common/arm64/`,
//! `codec/encoder/core/arm64/` and `codec/decoder/core/arm64/`.
//!
//! # What was ported, and from where
//!
//! | file | upstream source |
//! |---|---|
//! | `sad.rs`, `satd.rs` | `codec/encoder/core/arm64/pixel_aarch64_neon.S` |
//! | `dct.rs`, `quant.rs` | `codec/encoder/core/arm64/reconstruct_aarch64_neon.S`, `codec/decoder/core/arm64/block_add_aarch64_neon.S` |
//! | `mc.rs` | `codec/common/arm64/mc_aarch64_neon.S` |
//! | `deblock.rs` | `codec/common/arm64/deblocking_aarch64_neon.S` |
//! | `intra_pred.rs` | `codec/common/arm64/intra_pred_common_aarch64_neon.S`, `codec/{encoder,decoder}/core/arm64/intra_pred_aarch64_neon.S` |
//! | `copy.rs` | `codec/common/arm64/copy_mb_aarch64_neon.S` |
//! | `score.rs` | none — upstream keeps `WelsCalculateSingleCtr4x4_c` on arm64; see the file |
//!
//! Each kernel names the asm routine it came from, and each file's header says
//! where and why it departs from the asm. There are four such departures, and every
//! one is a place the asm disagrees with its own C at the ends of the input range —
//! a 16-bit lane that wraps where the C's `int` does not. **This port's contract is
//! byte parity with the scalar beside it**, checked by the unit tests over the full
//! range, so those four places widen (`quant.rs`, `dct.rs`, `mc.rs`) or spell the
//! sign step the scalar's way (`quant.rs`); the asm's instruction sequence is kept
//! everywhere else.
//!
//! # The instruction set, and what `unsafe` covers
//!
//! NEON is part of the AArch64 baseline — every aarch64 target Rust ships enables it
//! and upstream's `cpu.cpp` does no runtime probe on arm64 either — so there is no
//! feature test in front of these kernels. Each body is a `#[target_feature(enable =
//! "neon")]` function: inside one, the value-only intrinsics are ordinary safe calls,
//! and the only `unsafe` left is the `vld1`/`vst1` pointer loads and stores, each
//! over a slice or array whose length the type or the preceding index has already
//! checked. The safe entry points call the bodies through one `unsafe` block apiece,
//! which is the same shape as `super::x86_64`'s `_impl` functions.
//!
//! # Miri
//!
//! Miri interprets `core::arch` intrinsics only where it has a shim, and its NEON
//! coverage stops at the first byte-difference instruction this set uses, so the
//! module is compiled out under `cfg(miri)` and that lane takes the scalar forwards,
//! exactly as it did before this module existed. The `unsafe` here is therefore not
//! Miri-checked; it is the load/store shape Miri does check on x86_64.

#![allow(unsafe_code)]

pub mod copy;
pub mod dct;
pub mod deblock;
pub mod intra_pred;
pub mod mc;
pub mod quant;
pub mod sad;
pub mod satd;
pub mod score;

/// Loads and stores shared by the kernels: the `ld1`/`st1` of the asm, with the
/// bounds check the asm leaves to its caller done by the slice index in front of
/// the pointer.
mod lanes {
    use core::arch::aarch64::*;

    /// `ld1 {v.16b}` — sixteen bytes into sixteen lanes.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn ld16(r: &[u8]) -> uint8x16_t {
        let r: &[u8; 16] = r[..16].try_into().expect("16 bytes");
        // SAFETY: `r` is an array of exactly 16 bytes, which is what `vld1q_u8` reads.
        unsafe { vld1q_u8(r.as_ptr()) }
    }

    /// `ld1 {v.8b}` — eight bytes into eight lanes.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn ld8(r: &[u8]) -> uint8x8_t {
        let r: &[u8; 8] = r[..8].try_into().expect("8 bytes");
        // SAFETY: `r` is an array of exactly 8 bytes, which is what `vld1_u8` reads.
        unsafe { vld1_u8(r.as_ptr()) }
    }

    /// `ld1 {v.s}[0]` — four bytes into the low four lanes, the upper four zero.
    ///
    /// The asm leaves the upper lanes holding whatever the register had and reduces
    /// over `.4h`; zeroing them lets every reduce here be the full-width one.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn ld4(r: &[u8]) -> uint8x8_t {
        let w = u32::from_ne_bytes(r[..4].try_into().expect("4 bytes"));
        vcreate_u8(w as u64)
    }

    /// `ld1 {v.8h}` — eight coefficients.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn ld8_i16(r: &[i16]) -> int16x8_t {
        let r: &[i16; 8] = r[..8].try_into().expect("8 coefficients");
        // SAFETY: `r` is an array of exactly 8 `i16`, which is what `vld1q_s16` reads.
        unsafe { vld1q_s16(r.as_ptr()) }
    }

    /// `ld1 {v.4h}` — four coefficients.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn ld4_i16(r: &[i16]) -> int16x4_t {
        let r: &[i16; 4] = r[..4].try_into().expect("4 coefficients");
        // SAFETY: `r` is an array of exactly 4 `i16`, which is what `vld1_s16` reads.
        unsafe { vld1_s16(r.as_ptr()) }
    }

    /// `st1 {v.16b}`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn st16(out: &mut [u8], v: uint8x16_t) {
        let out: &mut [u8; 16] = (&mut out[..16]).try_into().expect("16 bytes");
        // SAFETY: `out` is an array of exactly 16 bytes, which is what `vst1q_u8` writes.
        unsafe { vst1q_u8(out.as_mut_ptr(), v) }
    }

    /// `st1 {v.8b}`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn st8(out: &mut [u8], v: uint8x8_t) {
        let out: &mut [u8; 8] = (&mut out[..8]).try_into().expect("8 bytes");
        // SAFETY: `out` is an array of exactly 8 bytes, which is what `vst1_u8` writes.
        unsafe { vst1_u8(out.as_mut_ptr(), v) }
    }

    /// `st1 {v.s}[0]` — the low four lanes.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn st4(out: &mut [u8], v: uint8x8_t) {
        out[..4].copy_from_slice(&low4(v));
    }

    /// The low four lanes as an array.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn low4(v: uint8x8_t) -> [u8; 4] {
        vget_lane_u32::<0>(vreinterpret_u32_u8(v)).to_ne_bytes()
    }

    /// The eight lanes as an array.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn to8(v: uint8x8_t) -> [u8; 8] {
        vget_lane_u64::<0>(vreinterpret_u64_u8(v)).to_ne_bytes()
    }

    /// The sixteen lanes as an array.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn to16(v: uint8x16_t) -> [u8; 16] {
        let mut out = [0u8; 16];
        st16(&mut out, v);
        out
    }

    /// `st1 {v.8h}`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn st8_i16(out: &mut [i16], v: int16x8_t) {
        let out: &mut [i16; 8] = (&mut out[..8]).try_into().expect("8 coefficients");
        // SAFETY: `out` is an array of exactly 8 `i16`, which is what `vst1q_s16` writes.
        unsafe { vst1q_s16(out.as_mut_ptr(), v) }
    }

    /// `st1 {v.4h}`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn st4_i16(out: &mut [i16], v: int16x4_t) {
        let out: &mut [i16; 4] = (&mut out[..4]).try_into().expect("4 coefficients");
        // SAFETY: `out` is an array of exactly 4 `i16`, which is what `vst1_s16` writes.
        unsafe { vst1_s16(out.as_mut_ptr(), v) }
    }

    /// True when any lane of a byte mask is set — the asm's `ZERO_JUMP_END`, which
    /// ORs the two halves of the register and branches on zero.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn any_set(m: uint8x16_t) -> bool {
        vmaxvq_u8(m) != 0
    }
}
