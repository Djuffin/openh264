//! Scalar forwards for the `deblock` kernels — see the module header.

use crate::safe::plane::PlaneSamples;

#[inline(always)]
pub fn deblock_luma_lt4( pix: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32, tc: &[i8; 4], ) {
    crate::common::deblocking_common::deblock_luma_lt4_scalar(pix, step_x, step_y, alpha, beta, tc)
}

#[inline(always)]
pub fn deblock_luma_eq4(pix: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32) {
    crate::common::deblocking_common::deblock_luma_eq4_scalar(pix, step_x, step_y, alpha, beta)
}

#[inline(always)]
pub fn deblock_chroma_lt4( cb: &mut impl PlaneSamples, cr: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32, tc: &[i8; 4], ) {
    crate::common::deblocking_common::deblock_chroma_lt4_scalar(cb, cr, step_x, step_y, alpha, beta, tc)
}

#[inline(always)]
pub fn deblock_chroma_eq4( cb: &mut impl PlaneSamples, cr: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32, ) {
    crate::common::deblocking_common::deblock_chroma_eq4_scalar(cb, cr, step_x, step_y, alpha, beta)
}
