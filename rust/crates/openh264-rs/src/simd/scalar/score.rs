//! Scalar forwards for the `score` kernels — see the module header.

#[inline(always)]
pub fn calculate_single_ctr_4x4(dct: &[i16; 16]) -> i32 {
    crate::encoder::encode_mb_aux::calculate_single_ctr_4x4(dct)
}
