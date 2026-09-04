//! Scalar forwards for the `mc` kernels — see the module header.

use crate::safe::plane::{PlaneCursorMut, RefSamples};

#[inline(always)]
pub fn pixel_avg<A: RefSamples, B: RefSamples>( dst: &mut PlaneCursorMut<'_>, a: &A, b: &B, width: usize, height: usize, ) {
    crate::common::mc::pixel_avg_c(dst, a, b, width, height)
}

#[inline(always)]
pub fn mc_chroma<S: RefSamples + Copy>( src: &S, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize, ) {
    crate::common::mc::mc_chroma_c(src, dst, mv_x, mv_y, width, height)
}

#[inline(always)]
pub fn mc_hor_ver20<S: RefSamples + Copy>( src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize, ) {
    crate::common::mc::mc_hor_ver20_c(src, dst, width, height)
}

#[inline(always)]
pub fn mc_hor_ver02<S: RefSamples + Copy>( src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize, ) {
    crate::common::mc::mc_hor_ver02_c(src, dst, width, height)
}

#[inline(always)]
pub fn mc_hor_ver22<S: RefSamples + Copy>( src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize, ) {
    crate::common::mc::mc_hor_ver22_c(src, dst, width, height)
}

#[inline(always)]
pub fn mc_luma<S: RefSamples + Copy>( src: &S, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize, ) {
    crate::common::mc::mc_luma_c(src, dst, mv_x, mv_y, width, height)
}
