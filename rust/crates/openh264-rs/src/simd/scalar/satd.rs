//! Scalar forwards for the `satd` kernels — see the module header.

use crate::safe::plane::RefSamples;

#[inline(always)]
pub fn satd_4x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    crate::encoder::sample::satd_4x4(c1, c2)
}

#[inline(always)]
pub fn satd_8x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    crate::encoder::sample::satd_8x4(c1, c2)
}

#[inline(always)]
pub fn satd_4x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    crate::encoder::sample::satd_4x8(c1, c2)
}

#[inline(always)]
pub fn satd_8x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    crate::encoder::sample::satd_8x8(c1, c2)
}

#[inline(always)]
pub fn satd_16x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    crate::encoder::sample::satd_16x8(c1, c2)
}

#[inline(always)]
pub fn satd_8x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    crate::encoder::sample::satd_8x16(c1, c2)
}

#[inline(always)]
pub fn satd_16x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    crate::encoder::sample::satd_16x16(c1, c2)
}
