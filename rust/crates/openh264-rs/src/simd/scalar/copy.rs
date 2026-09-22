#![forbid(unsafe_code)]
//! Scalar forwards for the `copy` kernels — see the module header.

use crate::encoder::encode_mb_aux::{
    WelsCopy8x8_c, WelsCopy8x16_c, WelsCopy16x8_c, WelsCopy16x16_c,
};
use crate::encoder::rec_view::RecCursor;

#[inline(always)]
pub fn copy_16x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    WelsCopy16x16_c(dst, src)
}

#[inline(always)]
pub fn copy_16x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    WelsCopy16x8_c(dst, src)
}

#[inline(always)]
pub fn copy_8x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    WelsCopy8x16_c(dst, src)
}

#[inline(always)]
pub fn copy_8x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    WelsCopy8x8_c(dst, src)
}

#[inline(always)]
pub fn copy_16x16_slice(dst: &RecCursor<'_>, src: &[u8], src_stride: usize) {
    let s = &src[..15 * src_stride + 16];
    for y in 0..16 {
        let row: &[u8; 16] = s[y * src_stride..][..16].try_into().expect("16 bytes");
        dst.write_row::<16>(y as isize, 0, row);
    }
}

#[inline(always)]
pub fn copy_8x8_slice(dst: &RecCursor<'_>, src: &[u8], src_stride: usize) {
    let s = &src[..7 * src_stride + 8];
    for y in 0..8 {
        let row: &[u8; 8] = s[y * src_stride..][..8].try_into().expect("8 bytes");
        dst.write_row::<8>(y as isize, 0, row);
    }
}
