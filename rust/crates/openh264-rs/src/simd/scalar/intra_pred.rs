//! Scalar forwards for the `intra_pred` kernels — see the module header.

use crate::decoder::get_intra_predictor::{
    chroma_pred_dc, chroma_pred_h, chroma_pred_plane, chroma_pred_v, i4x4_luma_pred_dc,
    i4x4_luma_pred_h, i4x4_luma_pred_v, i16x16_luma_pred_dc, i16x16_luma_pred_dc_na,
    i16x16_luma_pred_dc_top, i16x16_luma_pred_h, i16x16_luma_pred_plane, i16x16_luma_pred_v,
};
use crate::encoder::get_intra_predictor::{
    WelsI4x4LumaPredDDL_c, WelsI4x4LumaPredDDR_c, WelsI4x4LumaPredDc_c, WelsI4x4LumaPredH_c,
    WelsI4x4LumaPredHD_c, WelsI4x4LumaPredHU_c, WelsI4x4LumaPredV_c, WelsI4x4LumaPredVL_c,
    WelsI4x4LumaPredVR_c, WelsI16x16LumaPredDc_c, WelsI16x16LumaPredH_c, WelsI16x16LumaPredPlane_c,
    WelsI16x16LumaPredV_c, WelsIChromaPredDc_c, WelsIChromaPredH_c, WelsIChromaPredPlane_c,
    WelsIChromaPredV_c,
};
use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursorMut, RefSamples};

#[inline(always)]
pub fn enc_i16x16_luma_pred_v(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    WelsI16x16LumaPredV_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    i16x16_luma_pred_v(pred)
}

#[inline(always)]
pub fn enc_i16x16_luma_pred_h(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    WelsI16x16LumaPredH_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    i16x16_luma_pred_h(pred)
}

#[inline(always)]
pub fn enc_i16x16_luma_pred_dc(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    WelsI16x16LumaPredDc_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    i16x16_luma_pred_dc(pred)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_dc_top(pred: &mut PlaneCursorMut<'_>) {
    i16x16_luma_pred_dc_top(pred)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    i16x16_luma_pred_dc_na(pred)
}

#[inline(always)]
pub fn enc_i16x16_luma_pred_plane(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    WelsI16x16LumaPredPlane_c(pred, rec)
}

#[inline(always)]
pub fn dec_i16x16_luma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    i16x16_luma_pred_plane(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_v(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    WelsIChromaPredV_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    chroma_pred_v(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_h(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    WelsIChromaPredH_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    chroma_pred_h(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_dc(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    WelsIChromaPredDc_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    chroma_pred_dc(pred)
}

#[inline(always)]
pub fn enc_chroma_pred_plane(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    WelsIChromaPredPlane_c(pred, rec)
}

#[inline(always)]
pub fn dec_chroma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    chroma_pred_plane(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_v(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredV_c(pred, rec)
}

#[inline(always)]
pub fn dec_i4x4_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    i4x4_luma_pred_v(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_h(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredH_c(pred, rec)
}

#[inline(always)]
pub fn dec_i4x4_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    i4x4_luma_pred_h(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_dc(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredDc_c(pred, rec)
}

#[inline(always)]
pub fn dec_i4x4_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    i4x4_luma_pred_dc(pred)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_ddl(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredDDL_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_ddr(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredDDR_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_vr(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredVR_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_hd(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredHD_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_vl(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredVL_c(pred, rec)
}

#[inline(always)]
pub fn enc_i4x4_luma_pred_hu(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    WelsI4x4LumaPredHU_c(pred, rec)
}

#[inline(always)]
pub fn intra_16x16_combined3_sad(
    pred: &mut [u8; 256],
    rec: &RecCursor<'_>,
    enc: &RecCursor<'_>,
    lambda: i32,
) -> (u8, i32) {
    let top = rec.row_n::<16>(-1, 0);
    let mut sum_top: i32 = 0;
    for &x in &top {
        sum_top += x as i32;
    }
    let mut left = [0u8; 16];
    let mut sum_left: i32 = 0;
    for y in 0..16 {
        let val = rec.at(-1, y as isize);
        left[y] = val;
        sum_left += val as i32;
    }

    let dc_val = ((16 + sum_top + sum_left) >> 5) as u8;

    let mut sad_v: i32 = 0;
    let mut sad_h: i32 = 0;
    let mut sad_dc: i32 = 0;

    for y in 0..16 {
        let enc_row = enc.row_n::<16>(y as isize, 0);
        let h_val = left[y];
        for x in 0..16 {
            let p = enc_row[x] as i32;
            sad_v += (p - top[x] as i32).abs();
            sad_h += (p - h_val as i32).abs();
            sad_dc += (p - dc_val as i32).abs();
        }
    }

    let cost_v = sad_v + lambda * 1;
    let cost_h = sad_h + lambda * 3;
    let cost_dc = sad_dc + lambda * 3;

    let (best_mode, best_cost) = if cost_dc < cost_h && cost_dc < cost_v {
        (2u8, cost_dc)
    } else if cost_h < cost_v {
        (1u8, cost_h)
    } else {
        (0u8, cost_v)
    };

    match best_mode {
        0 => {
            for y in 0..16 {
                pred[y * 16..(y + 1) * 16].copy_from_slice(&top);
            }
        }
        1 => {
            for y in 0..16 {
                pred[y * 16..(y + 1) * 16].fill(left[y]);
            }
        }
        _ => {
            pred.fill(dc_val);
        }
    }

    (best_mode, best_cost)
}
