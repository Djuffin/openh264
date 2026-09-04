//! Scalar forwards for the `quant` kernels — see the module header.

#[inline(always)]
pub fn quant_4x4(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    crate::encoder::encode_mb_aux::quant_4x4(dct, ff, mf)
}

#[inline(always)]
pub fn quant_4x4_dc(dct: &mut [i16; 16], ff: i16, mf: i16) {
    crate::encoder::encode_mb_aux::quant_4x4_dc(dct, ff, mf)
}

#[inline(always)]
pub fn quant_four_4x4(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8]) {
    crate::encoder::encode_mb_aux::quant_four_4x4(dct, ff, mf)
}

#[inline(always)]
pub fn quant_four_4x4_max(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8], max: &mut [i16; 4]) {
    crate::encoder::encode_mb_aux::quant_four_4x4_max(dct, ff, mf, max)
}

#[inline(always)]
pub fn dequant_4x4(res: &mut [i16; 16], mf: &[u16; 8]) {
    crate::encoder::decode_mb_aux::dequant_4x4(res, mf)
}

#[inline(always)]
pub fn dequant_four_4x4(res: &mut [i16; 64], mf: &[u16; 8]) {
    crate::encoder::decode_mb_aux::dequant_four_4x4(res, mf)
}

#[inline(always)]
pub fn get_none_zero_count(level: &[i16; 16]) -> i32 {
    crate::encoder::encode_mb_aux::get_none_zero_count(level)
}

#[inline(always)]
pub fn hadamard_t4_dc(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    crate::encoder::encode_mb_aux::hadamard_t4_dc(luma_dc, dct)
}

#[inline(always)]
pub fn dequant_ihadamard_4x4(res: &mut [i16; 16], mf: u16) {
    crate::encoder::decode_mb_aux::dequant_ihadamard_4x4(res, mf)
}
