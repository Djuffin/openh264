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

// ============================================================================
// Non-Zero Count
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

// ============================================================================
// Hadamard Transforms of the Luma DC Block (SSE2)
// ============================================================================

/// Transposes four vectors of four `i32` lanes: lane `j` of result `k` becomes lane
/// `k` of input `j`.
#[target_feature(enable = "sse2")]
unsafe fn transpose4_epi32(
    v0: __m128i,
    v1: __m128i,
    v2: __m128i,
    v3: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    let a = _mm_unpacklo_epi32(v0, v1);
    let b = _mm_unpackhi_epi32(v0, v1);
    let c = _mm_unpacklo_epi32(v2, v3);
    let d = _mm_unpackhi_epi32(v2, v3);
    (
        _mm_unpacklo_epi64(a, c),
        _mm_unpackhi_epi64(a, c),
        _mm_unpacklo_epi64(b, d),
        _mm_unpackhi_epi64(b, d),
    )
}

/// Transposes four vectors whose **low four `i16` lanes** hold a row. The upper four
/// lanes of each result are the next row's data and are never read: the butterflies
/// are lane-wise and the stores are `_mm_storel_epi64`.
#[target_feature(enable = "sse2")]
unsafe fn transpose4_epi16_lo(
    v0: __m128i,
    v1: __m128i,
    v2: __m128i,
    v3: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    let a = _mm_unpacklo_epi16(v0, v1);
    let b = _mm_unpacklo_epi16(v2, v3);
    let lo = _mm_unpacklo_epi32(a, b);
    let hi = _mm_unpackhi_epi32(a, b);
    (lo, _mm_srli_si128(lo, 8), hi, _mm_srli_si128(hi, 8))
}

/// 4x4 forward Hadamard transform of the sixteen luma DC coefficients.
///
/// C++: `WelsHadamardT4Dc_sse2`, `codec/encoder/core/x86/dct.asm:78`.
///
/// # Layout
///
/// The DC coefficients are the `(0, 0)` of each 4x4 block, so within the macroblock's
/// 241-element span they sit 16 and 64 elements apart — sixteen scattered `i16` reads,
/// which is why upstream's `SSE2_Load4Col` is sixteen `movsx`/`movd` pairs and why the
/// gather below is written out. The four vectors hold the four inputs of one scalar row
/// per lane: lane `k` is the row at `i = 4k`, whose `idx` is `((i & 8) << 4) + ((i & 4) << 3)`,
/// i.e. 0, 32, 128, 160.
///
/// With the inputs laid out that way the row pass is lane-wise with no shuffle, one
/// transpose puts `p[4k + j]` in lane `j` of vector `k`, and the column pass is lane-wise
/// again. `packs` at the end saturates, which is exactly the scalar's
/// `.clamp(-32768, 32767) as i16`.
///
/// Arithmetic is `i32` throughout, as the scalar's is: `|input| <= 32768` bounds the
/// row pass at `|p| <= 131072` and the column pass at `|t0 ± t1| <= 524288`.
#[target_feature(enable = "sse2")]
unsafe fn hadamard_t4_dc_sse2_impl(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    unsafe {
        // Lane k = scalar row k. Within a row: A = dct[idx], B = dct[idx + 16],
        // C = dct[idx + 64], D = dct[idx + 80] — the scalar's d0, d16, d64, d80.
        let va = _mm_set_epi32(dct[160] as i32, dct[128] as i32, dct[32] as i32, dct[0] as i32);
        let vb = _mm_set_epi32(dct[176] as i32, dct[144] as i32, dct[48] as i32, dct[16] as i32);
        let vc = _mm_set_epi32(dct[224] as i32, dct[192] as i32, dct[96] as i32, dct[64] as i32);
        let vd = _mm_set_epi32(dct[240] as i32, dct[208] as i32, dct[112] as i32, dct[80] as i32);

        // Row pass. `pj` holds `p[4k + j]` in lane `k`.
        let s0 = _mm_add_epi32(va, vd);
        let s3 = _mm_sub_epi32(va, vd);
        let s1 = _mm_add_epi32(vb, vc);
        let s2 = _mm_sub_epi32(vb, vc);
        let p0 = _mm_add_epi32(s0, s1);
        let p1 = _mm_add_epi32(s3, s2);
        let p2 = _mm_sub_epi32(s0, s1);
        let p3 = _mm_sub_epi32(s3, s2);

        // `rk` now holds `p[4k + j]` in lane `j`, so the column pass is lane-wise.
        let (r0, r1, r2, r3) = transpose4_epi32(p0, p1, p2, p3);

        let one = _mm_set1_epi32(1);
        let t0 = _mm_add_epi32(r0, r3);
        let t3 = _mm_sub_epi32(r0, r3);
        let t1 = _mm_add_epi32(r1, r2);
        let t2 = _mm_sub_epi32(r1, r2);
        let o0 = _mm_srai_epi32(_mm_add_epi32(_mm_add_epi32(t0, t1), one), 1);
        let o1 = _mm_srai_epi32(_mm_add_epi32(_mm_add_epi32(t3, t2), one), 1);
        let o2 = _mm_srai_epi32(_mm_add_epi32(_mm_sub_epi32(t0, t1), one), 1);
        let o3 = _mm_srai_epi32(_mm_add_epi32(_mm_sub_epi32(t3, t2), one), 1);

        _mm_storeu_si128(luma_dc.as_mut_ptr() as *mut __m128i, _mm_packs_epi32(o0, o1));
        _mm_storeu_si128(
            luma_dc.as_mut_ptr().add(8) as *mut __m128i,
            _mm_packs_epi32(o2, o3),
        );
    }
}

/// 4x4 forward Hadamard transform of 16 luma DC coefficients using SSE2.
///
/// C++: `WelsHadamardT4Dc_sse2`, `codec/encoder/core/x86/dct.asm`.
#[inline]
pub fn hadamard_t4_dc_sse2(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    unsafe { hadamard_t4_dc_sse2_impl(luma_dc, dct) }
}

/// The inverse-Hadamard butterfly, which is the same in both passes.
///
/// `(a0, a1, a2, a3)` are the four taps of one line — a row in the first pass, a column
/// in the second — with one line per lane.
#[target_feature(enable = "sse2")]
unsafe fn ihadamard_butterfly_sse2(
    a0: __m128i,
    a1: __m128i,
    a2: __m128i,
    a3: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    let t0 = _mm_add_epi16(a0, a2);
    let t1 = _mm_sub_epi16(a0, a2);
    let t2 = _mm_sub_epi16(a1, a3);
    let t3 = _mm_add_epi16(a1, a3);
    (
        _mm_add_epi16(t0, t3),
        _mm_add_epi16(t1, t2),
        _mm_sub_epi16(t1, t2),
        _mm_sub_epi16(t0, t3),
    )
}

/// In-place dequantization and inverse 4x4 Hadamard transform of the luma DC block.
///
/// C++: `WelsDequantIHadamard4x4_sse2`, `codec/encoder/core/x86/quant.asm:332`.
///
/// # Layout
///
/// Each row is four contiguous `i16`, so a row is one 64-bit load. Transposing before
/// the first pass is what makes both passes the *same* lane-wise butterfly — the row
/// pass over `res[i..i+4]` and the column pass over `res[i], res[i+4], res[i+8],
/// res[i+12]` have identical tap structure, and only the operand layout differs. So the
/// shape is transpose, butterfly, transpose, butterfly, and
/// [`ihadamard_butterfly_sse2`] is written once.
///
/// # Where the multiply goes
///
/// The scalar multiplies by `mf` on the way out of the second pass; upstream's asm
/// multiplies on the way in, before the transform. Both are correct and give identical
/// results — the transform is linear and every operation is `wrapping` `i16`, so
/// `mf * (a ± b) ≡ mf * a ± mf * b (mod 2^16)`. This follows the scalar, which is what
/// the parity test compares against.
///
/// Every intrinsic here wraps rather than saturating (`_mm_add_epi16`, `_mm_sub_epi16`,
/// `_mm_mullo_epi16`), matching the scalar's `wrapping_add`/`wrapping_sub`/`wrapping_mul`
/// exactly. The wrapping is load-bearing, not incidental: the C++ is `int16_t`
/// throughout and its overflow is observable in the output.
#[target_feature(enable = "sse2")]
unsafe fn dequant_ihadamard_4x4_sse2_impl(res: &mut [i16; 16], mf: u16) {
    unsafe {
        let src = res.as_ptr();
        let r0 = _mm_loadl_epi64(src as *const __m128i);
        let r1 = _mm_loadl_epi64(src.add(4) as *const __m128i);
        let r2 = _mm_loadl_epi64(src.add(8) as *const __m128i);
        let r3 = _mm_loadl_epi64(src.add(12) as *const __m128i);

        // `cm` holds `res[4k + m]` in lane `k`, so the row pass is lane-wise.
        let (c0, c1, c2, c3) = transpose4_epi16_lo(r0, r1, r2, r3);
        let (w0, w1, w2, w3) = ihadamard_butterfly_sse2(c0, c1, c2, c3);
        // Back to one row per vector, which is what the column pass wants lane-wise.
        let (x0, x1, x2, x3) = transpose4_epi16_lo(w0, w1, w2, w3);
        let (y0, y1, y2, y3) = ihadamard_butterfly_sse2(x0, x1, x2, x3);

        let mfv = _mm_set1_epi16(mf as i16);
        let dst = res.as_mut_ptr();
        _mm_storel_epi64(dst as *mut __m128i, _mm_mullo_epi16(y0, mfv));
        _mm_storel_epi64(dst.add(4) as *mut __m128i, _mm_mullo_epi16(y1, mfv));
        _mm_storel_epi64(dst.add(8) as *mut __m128i, _mm_mullo_epi16(y2, mfv));
        _mm_storel_epi64(dst.add(12) as *mut __m128i, _mm_mullo_epi16(y3, mfv));
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

    // ========================================================================
    // The two luma-DC Hadamard kernels.
    //
    // Neither had a parity test before, which is how a "kernel" that was the scalar
    // copied verbatim — not one `_mm_*` call in either — sat in the dispatch tables
    // unnoticed (review §10). These are the real ports of `WelsHadamardT4Dc_sse2`
    // (`codec/encoder/core/x86/dct.asm:78`) and `WelsDequantIHadamard4x4_sse2`
    // (`codec/encoder/core/x86/quant.asm:332`), so they get the test first.
    //
    // Both sweep the **full `i16` input range**. That is not decoration: the ihadamard
    // is `int16_t` end to end in the C++ and its overflow is observable output, so a
    // kernel that widened anywhere would pass a small-coefficient sweep and diverge in
    // a real stream. The `hadamard_t4_dc` clamp is only reachable from large inputs too.
    // ========================================================================

    fn lcg_full_i16(seed: &mut u64) -> i16 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32) as u16 as i16
    }

    #[test]
    fn hadamard_t4_dc_sse2_parity() {
        use crate::encoder::encode_mb_aux::hadamard_t4_dc;

        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..2000 {
            let mut dct = [0i16; 241];
            for v in dct.iter_mut() {
                *v = lcg_full_i16(&mut seed);
            }
            let mut want = [0i16; 16];
            let mut got = [0i16; 16];
            hadamard_t4_dc(&mut want, &dct);
            hadamard_t4_dc_sse2(&mut got, &dct);
            assert_eq!(got, want);
        }
    }

    /// The saturating store is the one place the two could differ only at the extremes,
    /// so drive it deliberately: all sixteen DC coefficients at `i16::MIN`/`MAX` puts
    /// every output past the clamp in both directions.
    #[test]
    fn hadamard_t4_dc_sse2_saturates_like_the_scalar() {
        use crate::encoder::encode_mb_aux::hadamard_t4_dc;

        const DC_IDX: [usize; 16] = [
            0, 16, 64, 80, 32, 48, 96, 112, 128, 144, 192, 208, 160, 176, 224, 240,
        ];
        // Every assignment of the two extremes across the sixteen DC positions: 65536
        // cases, which covers the sign patterns that drive the clamp both ways.
        for pattern in 0u32..(1 << 16) {
            let mut dct = [0i16; 241];
            for (n, &idx) in DC_IDX.iter().enumerate() {
                dct[idx] = if pattern & (1 << n) != 0 { i16::MAX } else { i16::MIN };
            }
            let mut want = [0i16; 16];
            let mut got = [0i16; 16];
            hadamard_t4_dc(&mut want, &dct);
            hadamard_t4_dc_sse2(&mut got, &dct);
            assert_eq!(got, want, "pattern {pattern:#018b}");
        }

        // And a sanity check that this really does reach the clamp, so the test cannot
        // quietly stop exercising it.
        let mut dct = [0i16; 241];
        for &idx in &DC_IDX {
            dct[idx] = i16::MAX;
        }
        let mut want = [0i16; 16];
        hadamard_t4_dc(&mut want, &dct);
        assert_eq!(want[0], i16::MAX, "the all-MAX case should saturate the DC output");
    }

    #[test]
    fn dequant_ihadamard_4x4_sse2_parity() {
        let mut seed = 0x8A5C_D789_635D_2DFFu64;
        // Every `mf` the dequant tables can produce, plus the ends of the range: the
        // multiply wraps, so a value near `u16::MAX` is a different test from a small one.
        let mfs: Vec<u16> = (0..6)
            .map(|r| crate::encoder::svc_encode_mb::g_kuiDequantCoeff[r][0])
            .chain([0u16, 1, 0x7FFF, 0x8000, u16::MAX])
            .collect();

        for &mf in &mfs {
            for _ in 0..40 {
                let mut want = [0i16; 16];
                for v in want.iter_mut() {
                    *v = lcg_full_i16(&mut seed);
                }
                let mut got = want;
                dequant_ihadamard_4x4(&mut want, mf);
                dequant_ihadamard_4x4_sse2(&mut got, mf);
                assert_eq!(got, want, "mf = {mf}");
            }
        }
    }

    /// The wrapping is load-bearing — the C++ is `int16_t` throughout — so pin that the
    /// SSE2 kernel wraps rather than saturates, with inputs chosen to overflow every
    /// intermediate.
    #[test]
    fn dequant_ihadamard_4x4_sse2_wraps_like_the_scalar() {
        for &v in &[i16::MIN, i16::MAX, -1, 1] {
            for &mf in &[1u16, 2, 0x8000, u16::MAX] {
                let mut want = [v; 16];
                let mut got = [v; 16];
                dequant_ihadamard_4x4(&mut want, mf);
                dequant_ihadamard_4x4_sse2(&mut got, mf);
                assert_eq!(got, want, "v = {v}, mf = {mf}");
            }
        }
    }
}
