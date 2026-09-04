//! Scalar forwards for the `intra_pred` kernels — see the module header.

use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::PlaneCursorMut;

#[inline(always)]
pub fn enc_i16x16_luma_pred_v(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI16x16LumaPredV_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i16x16_luma_pred_v(pred)
}

#[inline(always)]
pub fn enc_i16x16_luma_pred_h(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI16x16LumaPredH_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i16x16_luma_pred_h(pred)
}

#[inline(always)]
pub fn enc_i16x16_luma_pred_dc(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI16x16LumaPredDc_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i16x16_luma_pred_dc(pred)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_dc_top(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i16x16_luma_pred_dc_top(pred)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i16x16_luma_pred_dc_na(pred)
}

#[inline(always)]
pub fn enc_i16x16_luma_pred_plane(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI16x16LumaPredPlane_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i16x16_luma_pred_plane(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_v(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsIChromaPredV_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::chroma_pred_v(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_h(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsIChromaPredH_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::chroma_pred_h(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_dc(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsIChromaPredDc_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::chroma_pred_dc(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_plane(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsIChromaPredPlane_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::chroma_pred_plane(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_v(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredV_c(pred, rec)
}

#[inline(always)]
pub fn dec_i4x4_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i4x4_luma_pred_v(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_h(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredH_c(pred, rec)
}

#[inline(always)]
pub fn dec_i4x4_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i4x4_luma_pred_h(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_dc(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredDc_c(pred, rec)
}

#[inline(always)]
pub fn dec_i4x4_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    crate::decoder::get_intra_predictor::i4x4_luma_pred_dc(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_ddl(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredDDL_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_ddr(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredDDR_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_vr(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredVR_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_hd(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredHD_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_vl(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredVL_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_hu(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    crate::encoder::get_intra_predictor::WelsI4x4LumaPredHU_c(pred, rec)
}
