//! Scalar forwards for the `copy` kernels — see the module header.

use crate::encoder::rec_view::RecCursor;

#[inline(always)]
pub fn copy_16x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    crate::encoder::encode_mb_aux::WelsCopy16x16_c(dst, src)
}

#[inline(always)]
pub fn copy_16x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    crate::encoder::encode_mb_aux::WelsCopy16x8_c(dst, src)
}

#[inline(always)]
pub fn copy_8x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    crate::encoder::encode_mb_aux::WelsCopy8x16_c(dst, src)
}

#[inline(always)]
pub fn copy_8x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    crate::encoder::encode_mb_aux::WelsCopy8x8_c(dst, src)
}
