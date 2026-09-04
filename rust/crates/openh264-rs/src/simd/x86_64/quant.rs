//! SSE2 implementations of Quantization, Dequantization, Hadamard Transforms,
//! and Non-Zero Coefficient Counting.
#![allow(unsafe_code)]

use core::arch::x86_64::*;

// ============================================================================
// Forward Quantization
// ============================================================================

/// In-place dead-zone quantization of 8 consecutive 16-bit coefficients.
///
/// Matches C++ `SSE2_Quant8` in `codec/encoder/core/x86/quant.asm`.
#[target_feature(enable = "sse2")]
unsafe fn quant_8_sse2(v: __m128i, ff: __m128i, mf: __m128i) -> __m128i {
    let zero = _mm_setzero_si128();
    let sign = _mm_cmpgt_epi16(zero, v); // 0xFFFF where v < 0, 0 where v >= 0
    let abs = _mm_sub_epi16(_mm_xor_si128(v, sign), sign);
    let abs_ff = _mm_adds_epu16(abs, ff);
    let q = _mm_mulhi_epu16(abs_ff, mf);
    _mm_sub_epi16(_mm_xor_si128(q, sign), sign)
}

/// In-place dead-zone quantization of 8 consecutive 16-bit coefficients,
/// returning both the signed quantized values and the un-signed magnitudes.
#[target_feature(enable = "sse2")]
unsafe fn quant_8_with_mag_sse2(v: __m128i, ff: __m128i, mf: __m128i) -> (__m128i, __m128i) {
    let zero = _mm_setzero_si128();
    let sign = _mm_cmpgt_epi16(zero, v);
    let abs = _mm_sub_epi16(_mm_xor_si128(v, sign), sign);
    let abs_ff = _mm_adds_epu16(abs, ff);
    let q_mag = _mm_mulhi_epu16(abs_ff, mf);
    let q_signed = _mm_sub_epi16(_mm_xor_si128(q_mag, sign), sign);
    (q_signed, q_mag)
}

/// Horizontal maximum of 8 unsigned 16-bit values in an XMM register.
#[target_feature(enable = "sse2")]
unsafe fn hmax_u16_sse2(m: __m128i) -> i16 {
    let m1 = _mm_shuffle_epi32(m, 0b01_00_11_10);
    let m2 = _mm_max_epi16(m, m1);
    let m3 = _mm_shufflelo_epi16(m2, 0b01_00_11_10);
    let m4 = _mm_max_epi16(m2, m3);
    let m5 = _mm_srli_epi32(m4, 16);
    let m6 = _mm_max_epi16(m4, m5);
    _mm_cvtsi128_si32(m6) as i16
}

/// In-place dead-zone forward quantization of a 4x4 block using SSE2.
///
/// C++: `WelsQuant4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[target_feature(enable = "sse2")]
unsafe fn quant_4x4_sse2_impl(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    unsafe {
        let vff = _mm_loadu_si128(ff.as_ptr() as *const __m128i);
        let vmf = _mm_loadu_si128(mf.as_ptr() as *const __m128i);

        let v0 = _mm_loadu_si128(dct.as_ptr() as *const __m128i);
        let v1 = _mm_loadu_si128(dct.as_ptr().add(8) as *const __m128i);

        let q0 = quant_8_sse2(v0, vff, vmf);
        let q1 = quant_8_sse2(v1, vff, vmf);

        _mm_storeu_si128(dct.as_mut_ptr() as *mut __m128i, q0);
        _mm_storeu_si128(dct.as_mut_ptr().add(8) as *mut __m128i, q1);
    }
}

/// In-place dead-zone forward quantization of a 4x4 block using SSE2.
///
/// C++: `WelsQuant4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_4x4_sse2(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    unsafe { quant_4x4_sse2_impl(dct, ff, mf) }
}

#[target_feature(enable = "sse2")]
unsafe fn quant_4x4_dc_sse2_impl(dct: &mut [i16; 16], ff: i16, mf: i16) {
    unsafe {
        let vff = _mm_set1_epi16(ff);
        let vmf = _mm_set1_epi16(mf);

        let v0 = _mm_loadu_si128(dct.as_ptr() as *const __m128i);
        let v1 = _mm_loadu_si128(dct.as_ptr().add(8) as *const __m128i);

        let q0 = quant_8_sse2(v0, vff, vmf);
        let q1 = quant_8_sse2(v1, vff, vmf);

        _mm_storeu_si128(dct.as_mut_ptr() as *mut __m128i, q0);
        _mm_storeu_si128(dct.as_mut_ptr().add(8) as *mut __m128i, q1);
    }
}

/// In-place quantization of 16 Hadamard-transformed luma DC coefficients using SSE2.
///
/// C++: `WelsQuant4x4Dc_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_4x4_dc_sse2(dct: &mut [i16; 16], ff: i16, mf: i16) {
    unsafe { quant_4x4_dc_sse2_impl(dct, ff, mf) }
}

#[target_feature(enable = "sse2")]
unsafe fn quant_four_4x4_sse2_impl(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8]) {
    unsafe {
        let vff = _mm_loadu_si128(ff.as_ptr() as *const __m128i);
        let vmf = _mm_loadu_si128(mf.as_ptr() as *const __m128i);

        for i in (0..64).step_by(8) {
            let v = _mm_loadu_si128(dct.as_ptr().add(i) as *const __m128i);
            let q = quant_8_sse2(v, vff, vmf);
            _mm_storeu_si128(dct.as_mut_ptr().add(i) as *mut __m128i, q);
        }
    }
}

/// In-place dead-zone quantization of four consecutive 4x4 blocks using SSE2.
///
/// C++: `WelsQuantFour4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_four_4x4_sse2(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8]) {
    unsafe { quant_four_4x4_sse2_impl(dct, ff, mf) }
}

#[target_feature(enable = "sse2")]
unsafe fn quant_four_4x4_max_sse2_impl(
    dct: &mut [i16; 64],
    ff: &[i16; 8],
    mf: &[i16; 8],
    max: &mut [i16; 4],
) {
    unsafe {
        let vff = _mm_loadu_si128(ff.as_ptr() as *const __m128i);
        let vmf = _mm_loadu_si128(mf.as_ptr() as *const __m128i);

        for k in 0..4usize {
            let off = k << 4;
            let v0 = _mm_loadu_si128(dct.as_ptr().add(off) as *const __m128i);
            let v1 = _mm_loadu_si128(dct.as_ptr().add(off + 8) as *const __m128i);

            let (q0, mag0) = quant_8_with_mag_sse2(v0, vff, vmf);
            let (q1, mag1) = quant_8_with_mag_sse2(v1, vff, vmf);

            _mm_storeu_si128(dct.as_mut_ptr().add(off) as *mut __m128i, q0);
            _mm_storeu_si128(dct.as_mut_ptr().add(off + 8) as *mut __m128i, q1);

            let m = _mm_max_epi16(mag0, mag1);
            max[k] = hmax_u16_sse2(m);
        }
    }
}

/// In-place dead-zone quantization of four 4x4 blocks with early-termination max levels using SSE2.
///
/// C++: `WelsQuantFour4x4Max_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_four_4x4_max_sse2(
    dct: &mut [i16; 64],
    ff: &[i16; 8],
    mf: &[i16; 8],
    max: &mut [i16; 4],
) {
    unsafe { quant_four_4x4_max_sse2_impl(dct, ff, mf, max) }
}

// ============================================================================
// Dequantization
// ============================================================================

#[target_feature(enable = "sse2")]
unsafe fn dequant_4x4_sse2_impl(res: &mut [i16; 16], mf: &[u16; 8]) {
    unsafe {
        let vmf = _mm_loadu_si128(mf.as_ptr() as *const __m128i);
        let v0 = _mm_loadu_si128(res.as_ptr() as *const __m128i);
        let v1 = _mm_loadu_si128(res.as_ptr().add(8) as *const __m128i);

        _mm_storeu_si128(res.as_mut_ptr() as *mut __m128i, _mm_mullo_epi16(v0, vmf));
        _mm_storeu_si128(res.as_mut_ptr().add(8) as *mut __m128i, _mm_mullo_epi16(v1, vmf));
    }
}

/// In-place dequantization of one 4x4 coefficient block using SSE2.
///
/// C++: `WelsDequant4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn dequant_4x4_sse2(res: &mut [i16; 16], mf: &[u16; 8]) {
    unsafe { dequant_4x4_sse2_impl(res, mf) }
}

#[target_feature(enable = "sse2")]
unsafe fn dequant_four_4x4_sse2_impl(res: &mut [i16; 64], mf: &[u16; 8]) {
    unsafe {
        let vmf = _mm_loadu_si128(mf.as_ptr() as *const __m128i);
        for k in 0..8usize {
            let v = _mm_loadu_si128(res.as_ptr().add(k << 3) as *const __m128i);
            _mm_storeu_si128(res.as_mut_ptr().add(k << 3) as *mut __m128i, _mm_mullo_epi16(v, vmf));
        }
    }
}

/// In-place dequantization of four consecutive 4x4 coefficient blocks using SSE2.
///
/// C++: `WelsDequantFour4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn dequant_four_4x4_sse2(res: &mut [i16; 64], mf: &[u16; 8]) {
    unsafe { dequant_four_4x4_sse2_impl(res, mf) }
}

#[target_feature(enable = "sse2")]
unsafe fn dequant_ihadamard_4x4_sse2_impl(res: &mut [i16; 16], mf: u16) {
    let mut t = [0i16; 4];

    for i in (0..16).step_by(4) {
        t[0] = res[i].wrapping_add(res[i + 2]);
        t[1] = res[i].wrapping_sub(res[i + 2]);
        t[2] = res[i + 1].wrapping_sub(res[i + 3]);
        t[3] = res[i + 1].wrapping_add(res[i + 3]);

        res[i] = t[0].wrapping_add(t[3]);
        res[i + 1] = t[1].wrapping_add(t[2]);
        res[i + 2] = t[1].wrapping_sub(t[2]);
        res[i + 3] = t[0].wrapping_sub(t[3]);
    }

    for i in 0..4usize {
        t[0] = res[i].wrapping_add(res[i + 8]);
        t[1] = res[i].wrapping_sub(res[i + 8]);
        t[2] = res[i + 4].wrapping_sub(res[i + 12]);
        t[3] = res[i + 4].wrapping_add(res[i + 12]);

        res[i] = t[0].wrapping_add(t[3]).wrapping_mul(mf as i16);
        res[i + 4] = t[1].wrapping_add(t[2]).wrapping_mul(mf as i16);
        res[i + 8] = t[1].wrapping_sub(t[2]).wrapping_mul(mf as i16);
        res[i + 12] = t[0].wrapping_sub(t[3]).wrapping_mul(mf as i16);
    }
}

/// In-place dequantization and inverse 4x4 Hadamard transform of luma DC block using SSE2.
///
/// C++: `WelsDequantIHadamard4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn dequant_ihadamard_4x4_sse2(res: &mut [i16; 16], mf: u16) {
    unsafe { dequant_ihadamard_4x4_sse2_impl(res, mf) }
}

// ============================================================================
// Non-Zero Count & Hadamard
// ============================================================================

#[target_feature(enable = "sse2")]
unsafe fn get_none_zero_count_sse2_impl(level: &[i16; 16]) -> i32 {
    unsafe {
        let zero = _mm_setzero_si128();
        let v0 = _mm_loadu_si128(level.as_ptr() as *const __m128i);
        let v1 = _mm_loadu_si128(level.as_ptr().add(8) as *const __m128i);

        let eq0 = _mm_cmpeq_epi16(v0, zero);
        let eq1 = _mm_cmpeq_epi16(v1, zero);

        let mask0 = _mm_movemask_epi8(eq0);
        let mask1 = _mm_movemask_epi8(eq1);

        let zero_words = ((mask0.count_ones() + mask1.count_ones()) >> 1) as i32;
        16 - zero_words
    }
}

/// Count of non-zero coefficients in a 16-element level array using SSE2.
///
/// C++: `WelsGetNoneZeroCount_sse2`, `codec/encoder/core/x86/score.asm`.
#[inline]
pub fn get_none_zero_count_sse2(level: &[i16; 16]) -> i32 {
    unsafe { get_none_zero_count_sse2_impl(level) }
}

/// 4x4 forward Hadamard transform of 16 luma DC coefficients using SSE2.
///
/// C++: `WelsHadamardT4Dc_sse2`, `codec/encoder/core/src/encode_mb_aux.cpp`.
pub fn hadamard_t4_dc_sse2(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    // 16 luma DC coefficients can be computed via 32-bit arithmetic to avoid overflow
    let mut p = [0i32; 16];
    let mut s = [0i32; 4];

    for i in (0..16).step_by(4) {
        let idx = ((i & 0x08) << 4) + ((i & 0x04) << 3);
        let d0 = dct[idx] as i32;
        let d80 = dct[idx + 80] as i32;
        let d16 = dct[idx + 16] as i32;
        let d64 = dct[idx + 64] as i32;

        s[0] = d0 + d80;
        s[3] = d0 - d80;
        s[1] = d16 + d64;
        s[2] = d16 - d64;

        p[i] = s[0] + s[1];
        p[i + 2] = s[0] - s[1];
        p[i + 1] = s[3] + s[2];
        p[i + 3] = s[3] - s[2];
    }

    for i in 0..4usize {
        s[0] = p[i] + p[i + 12];
        s[3] = p[i] - p[i + 12];
        s[1] = p[i + 4] + p[i + 8];
        s[2] = p[i + 4] - p[i + 8];

        luma_dc[i] = ((s[0] + s[1] + 1) >> 1).clamp(-32768, 32767) as i16;
        luma_dc[i + 8] = ((s[0] - s[1] + 1) >> 1).clamp(-32768, 32767) as i16;
        luma_dc[i + 4] = ((s[3] + s[2] + 1) >> 1).clamp(-32768, 32767) as i16;
        luma_dc[i + 12] = ((s[3] - s[2] + 1) >> 1).clamp(-32768, 32767) as i16;
    }
}

// ============================================================================
// Unit Tests & Parity
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_mb_aux::{
        get_none_zero_count, quant_4x4, quant_4x4_dc, quant_four_4x4, quant_four_4x4_max,
        g_kiQuantMF, G_KI_QUANT_INTER_FF,
    };
    use crate::encoder::decode_mb_aux::{dequant_4x4, dequant_four_4x4, dequant_ihadamard_4x4};

    fn lcg(seed: &mut u64) -> i16 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 32) as i32 % 4000 - 2000) as i16
    }

    #[test]
    fn test_quant_4x4_parity() {
        let mut seed = 42u64;
        let ff = G_KI_QUANT_INTER_FF.0[16];
        let mf = g_kiQuantMF[16];

        for _ in 0..100 {
            let mut block_c = [0i16; 16];
            for v in block_c.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut block_simd = block_c;

            quant_4x4(&mut block_c, &ff, &mf);
            quant_4x4_sse2(&mut block_simd, &ff, &mf);

            assert_eq!(block_simd, block_c);
        }
    }

    #[test]
    fn test_quant_4x4_dc_parity() {
        let mut seed = 123u64;
        let ff = 17i16;
        let mf = 26i16;

        for _ in 0..100 {
            let mut block_c = [0i16; 16];
            for v in block_c.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut block_simd = block_c;

            quant_4x4_dc(&mut block_c, ff, mf);
            quant_4x4_dc_sse2(&mut block_simd, ff, mf);

            assert_eq!(block_simd, block_c);
        }
    }

    #[test]
    fn test_quant_four_4x4_parity() {
        let mut seed = 999u64;
        let ff = G_KI_QUANT_INTER_FF.0[22];
        let mf = g_kiQuantMF[22];

        for _ in 0..50 {
            let mut block_c = [0i16; 64];
            for v in block_c.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut block_simd = block_c;

            quant_four_4x4(&mut block_c, &ff, &mf);
            quant_four_4x4_sse2(&mut block_simd, &ff, &mf);

            assert_eq!(block_simd, block_c);
        }
    }

    #[test]
    fn test_quant_four_4x4_max_parity() {
        let mut seed = 7777u64;
        let ff = G_KI_QUANT_INTER_FF.0[28];
        let mf = g_kiQuantMF[28];

        for _ in 0..50 {
            let mut block_c = [0i16; 64];
            for v in block_c.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut block_simd = block_c;

            let mut max_c = [0i16; 4];
            let mut max_simd = [0i16; 4];

            quant_four_4x4_max(&mut block_c, &ff, &mf, &mut max_c);
            quant_four_4x4_max_sse2(&mut block_simd, &ff, &mf, &mut max_simd);

            assert_eq!(block_simd, block_c);
            assert_eq!(max_simd, max_c);
        }
    }

    #[test]
    fn test_dequant_4x4_parity() {
        let mut seed = 8888u64;
        let mf: [u16; 8] = [10, 13, 16, 13, 10, 13, 16, 13];

        for _ in 0..100 {
            let mut block_c = [0i16; 16];
            for v in block_c.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut block_simd = block_c;

            dequant_4x4(&mut block_c, &mf);
            dequant_4x4_sse2(&mut block_simd, &mf);

            assert_eq!(block_simd, block_c);
        }
    }

    #[test]
    fn test_dequant_four_4x4_parity() {
        let mut seed = 5555u64;
        let mf: [u16; 8] = [10, 13, 16, 13, 10, 13, 16, 13];

        for _ in 0..50 {
            let mut block_c = [0i16; 64];
            for v in block_c.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut block_simd = block_c;

            dequant_four_4x4(&mut block_c, &mf);
            dequant_four_4x4_sse2(&mut block_simd, &mf);

            assert_eq!(block_simd, block_c);
        }
    }

    #[test]
    fn test_get_none_zero_count_parity() {
        let mut seed = 3333u64;
        for _ in 0..100 {
            let mut block = [0i16; 16];
            for v in block.iter_mut() {
                let r = lcg(&mut seed);
                *v = if r % 3 == 0 { 0 } else { r };
            }
            let c_count = get_none_zero_count(&block);
            let simd_count = get_none_zero_count_sse2(&block);
            assert_eq!(simd_count, c_count);
        }
    }
}
