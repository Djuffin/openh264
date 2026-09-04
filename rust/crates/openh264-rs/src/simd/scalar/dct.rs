//! Scalar forwards for the `dct` kernels — see the module header.

use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, SampleCursor};

#[inline(always)]
pub fn dct_4x4<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 16], pix1: &A, pix2: &B) {
    crate::encoder::encode_mb_aux::dct_4x4(dct, pix1, pix2)
}

#[inline(always)]
pub fn dct_four_4x4<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 64], pix1: &A, pix2: &B) {
    crate::encoder::encode_mb_aux::dct_four_4x4(dct, pix1, pix2)
}

#[inline(always)]
pub fn idct_res_add_pred(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16]) {
    crate::decoder::decode_mb_aux::idct_res_add_pred_c(pred, rs)
}

#[inline(always)]
pub fn idct_t4_rec(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 16]) {
    crate::encoder::decode_mb_aux::idct_t4_rec_c(rec, pred, dct)
}

#[inline(always)]
pub fn idct_t4_rec_in_place(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 16]) {
    crate::encoder::decode_mb_aux::idct_t4_rec_in_place_c(rec, dct)
}

#[inline(always)]
pub fn idct_four_t4_rec(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 64]) {
    crate::encoder::decode_mb_aux::idct_four_t4_rec_c(rec, pred, dct)
}

#[inline(always)]
pub fn idct_four_t4_rec_in_place(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 64]) {
    crate::encoder::decode_mb_aux::idct_four_t4_rec_in_place_c(rec, dct)
}

#[inline(always)]
pub fn idct_t4_rec_to_view(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 16]) {
    crate::encoder::decode_mb_aux::idct_t4_rec_to_view_c(rec, pred, pred_stride, dct)
}

#[inline(always)]
pub fn idct_four_t4_rec_to_view(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 64]) {
    crate::encoder::decode_mb_aux::idct_four_t4_rec_to_view_c(rec, pred, pred_stride, dct)
}

#[inline(always)]
pub fn idct_t4_rec_in_place_view(rec: &RecCursor<'_>, dct: &[i16; 16]) {
    crate::encoder::decode_mb_aux::idct_t4_rec_in_place_view_c(rec, dct)
}

#[inline(always)]
pub fn idct_four_t4_rec_in_place_view(rec: &RecCursor<'_>, dct: &[i16; 64]) {
    crate::encoder::decode_mb_aux::idct_four_t4_rec_in_place_view_c(rec, dct)
}

#[inline(always)]
pub fn idct_t4_rec_on_mb_in_place_view(rec: &RecCursor<'_>, dct: &[i16; 256]) {
    crate::encoder::decode_mb_aux::idct_t4_rec_on_mb_in_place_view_c(rec, dct)
}

#[inline(always)]
pub fn idct_rec_i16x16_dc(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dc: &[i16; 16]) {
    crate::encoder::decode_mb_aux::idct_rec_i16x16_dc_c(rec, pred, dc)
}

#[inline(always)]
pub fn idct_rec_i16x16_dc_to_view(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dc: &[i16; 16]) {
    crate::encoder::decode_mb_aux::idct_rec_i16x16_dc_to_view_c(rec, pred, pred_stride, dc)
}
