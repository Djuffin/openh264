//! Scalar forwards for the `deblock` kernels — see the module header.

use crate::safe::plane::PlaneSamples;
use crate::encoder::encoder_context::SMVUnitXY;
use crate::common::deblocking_common::{deblock_chroma_eq4_scalar, deblock_chroma_lt4_scalar, deblock_luma_eq4_scalar, deblock_luma_lt4_scalar};

#[inline(always)]
pub fn deblock_luma_lt4( pix: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32, tc: &[i8; 4], ) {
    deblock_luma_lt4_scalar(pix, step_x, step_y, alpha, beta, tc)
}

#[inline(always)]
pub fn deblock_luma_eq4(pix: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32) {
    deblock_luma_eq4_scalar(pix, step_x, step_y, alpha, beta)
}

#[inline(always)]
pub fn deblock_chroma_lt4( cb: &mut impl PlaneSamples, cr: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32, tc: &[i8; 4], ) {
    deblock_chroma_lt4_scalar(cb, cr, step_x, step_y, alpha, beta, tc)
}

#[inline(always)]
pub fn deblock_chroma_eq4( cb: &mut impl PlaneSamples, cr: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32, ) {
    deblock_chroma_eq4_scalar(cb, cr, step_x, step_y, alpha, beta)
}

/// The boundary strengths of one macroblock — see
/// [`bs_calc_scalar`](crate::encoder::deblocking::bs_calc_scalar), which this is.
#[inline(always)]
pub fn bs_calc(
    cur_nzc: &[i8; 24],
    cur_mv: &[SMVUnitXY; 16],
    left: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    top: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    inside: u8,
    bs: &mut [[[u8; 4]; 4]; 2],
) {
    crate::encoder::deblocking::bs_calc_scalar(cur_nzc, cur_mv, left, top, inside, bs)
}
