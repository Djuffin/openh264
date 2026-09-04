//! Scalar forwards for the `sad` kernels — see the module header.

use crate::safe::plane::RefSamples;

#[inline(always)]
pub fn sample_sad_16x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<16, 16, _>(sample1, sample2)
}

#[inline(always)]
pub(crate) fn sample_sad_16x16_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<16, 16, _>(sample1, sample2)
}

#[inline(always)]
pub fn sample_sad_16x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<16, 8, _>(sample1, sample2)
}

#[inline(always)]
pub(crate) fn sample_sad_16x8_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<16, 8, _>(sample1, sample2)
}

#[inline(always)]
pub fn sample_sad_8x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<8, 16, _>(sample1, sample2)
}

#[inline(always)]
pub fn sample_sad_8x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<8, 8, _>(sample1, sample2)
}

#[inline(always)]
pub fn sample_sad_4x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<4, 4, _>(sample1, sample2)
}

#[inline(always)]
pub fn sample_sad_8x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<8, 4, _>(sample1, sample2)
}

#[inline(always)]
pub fn sample_sad_4x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    crate::common::sad_common::sample_sad::<4, 8, _>(sample1, sample2)
}

#[inline(always)]
pub fn sample_sad_four_16x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    crate::common::sad_common::sample_sad_four::<16, 16, _>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_16x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    crate::common::sad_common::sample_sad_four::<16, 8, _>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_8x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    crate::common::sad_common::sample_sad_four::<8, 16, _>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_8x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    crate::common::sad_common::sample_sad_four::<8, 8, _>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_4x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    crate::common::sad_common::sample_sad_four::<4, 4, _>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_8x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    crate::common::sad_common::sample_sad_four::<8, 4, _>(sample1, sample2, sad)
}

#[inline(always)]
pub fn sample_sad_four_4x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    crate::common::sad_common::sample_sad_four::<4, 8, _>(sample1, sample2, sad)
}
