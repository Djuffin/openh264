//! The aarch64 NEON kernel set — every entry point of `super::x86_64`, from
//! upstream's arm64 assembly under `codec/common/arm64/`,
//! `codec/encoder/core/arm64/` and `codec/decoder/core/arm64/`.
//!
//! NEON is part of the AArch64 baseline, so no feature test guards these kernels.
//! Each body is a `#[target_feature(enable = "neon")]` function: inside one the
//! value-only intrinsics are ordinary safe calls, and the only `unsafe` left is the
//! `vld1`/`vst1` loads and stores, each over a slice or array whose length the type
//! or the preceding index has already checked. The safe entry points call the bodies
//! through one `unsafe` block apiece.
//!
//! Every kernel is byte-exact with the scalar beside it, which in `quant.rs`,
//! `dct.rs` and `mc.rs` means widening an intermediate the asm holds at 16 bits.
//!
//! The module is compiled out under `cfg(miri)`, whose NEON shims stop short of the
//! byte-difference instructions used here; that lane takes the scalar forwards.

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
pub mod vaa;

/// Loads and stores shared by the kernels: the asm's `ld1`/`st1`, bounds-checked
/// by the slice index in front of the pointer.
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
    /// Zeroing the upper lanes lets every reduce here be the full-width one.
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

    /// True when any lane of a byte mask is set — the asm's `ZERO_JUMP_END`.
    #[inline]
    #[target_feature(enable = "neon")]
    pub(super) fn any_set(m: uint8x16_t) -> bool {
        vmaxvq_u8(m) != 0
    }
}
