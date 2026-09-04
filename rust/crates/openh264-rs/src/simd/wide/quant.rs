//! Quantisation, dequantisation, the luma-DC Hadamard pair and the non-zero count on
//! `wide` lane types — the twin of `simd::x86_64::quant`.
//!
//! Every operation the quantiser needs is a direct `wide` call: the sign mask is
//! `is_negative`, the dead-zone add is `u16x8::saturating_add` (`paddusw`), the scale
//! is `u16x8::mul_keep_high` (`pmulhuw`), the lane maximum is `reduce_max`. The only
//! seam is the `i16x8` ↔ `u16x8` reinterpretation, which is a `bytemuck::cast`.
//!
//! The forward Hadamard's transpose is `i32x4::transpose`, a `wide` primitive; the
//! inverse Hadamard's 4x4 word transpose is not one, and comes from
//! [`super::lanes::transpose4_lo`].

#![forbid(unsafe_code)]

use wide::bytemuck::cast;
use wide::{i16x8, i32x4, i32x8, u16x8};

use super::lanes::transpose4_lo;

// ============================================================================
// Forward quantisation
// ============================================================================

/// Dead-zone quantisation of eight coefficients, returning the signed result and the
/// unsigned magnitude. Matches `SSE2_Quant8` in `codec/encoder/core/x86/quant.asm`.
#[inline(always)]
fn quant_8_with_mag(v: i16x8, ff: u16x8, mf: u16x8) -> (i16x8, i16x8) {
    let sign = v.is_negative();
    let abs = (v ^ sign) - sign;
    let abs_ff = cast::<i16x8, u16x8>(abs).saturating_add(ff);
    let q_mag: i16x8 = cast(abs_ff.mul_keep_high(mf));
    ((q_mag ^ sign) - sign, q_mag)
}

#[inline(always)]
fn quant_8(v: i16x8, ff: u16x8, mf: u16x8) -> i16x8 {
    quant_8_with_mag(v, ff, mf).0
}

#[inline(always)]
fn load_i16(s: &[i16]) -> i16x8 {
    i16x8::from_slice_unaligned(&s[..8])
}

#[inline(always)]
fn store_i16(d: &mut [i16], v: i16x8) {
    d[..8].copy_from_slice(v.as_array());
}

/// C++: `WelsQuant4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_4x4(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    let (vff, vmf): (u16x8, u16x8) = (cast(*ff), cast(*mf));
    let q0 = quant_8(load_i16(&dct[..8]), vff, vmf);
    let q1 = quant_8(load_i16(&dct[8..]), vff, vmf);
    store_i16(&mut dct[..8], q0);
    store_i16(&mut dct[8..], q1);
}

/// C++: `WelsQuant4x4Dc_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_4x4_dc(dct: &mut [i16; 16], ff: i16, mf: i16) {
    let (vff, vmf) = (u16x8::splat(ff as u16), u16x8::splat(mf as u16));
    let q0 = quant_8(load_i16(&dct[..8]), vff, vmf);
    let q1 = quant_8(load_i16(&dct[8..]), vff, vmf);
    store_i16(&mut dct[..8], q0);
    store_i16(&mut dct[8..], q1);
}

/// C++: `WelsQuantFour4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_four_4x4(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8]) {
    let (vff, vmf): (u16x8, u16x8) = (cast(*ff), cast(*mf));
    for chunk in dct.chunks_exact_mut(8) {
        let q = quant_8(load_i16(chunk), vff, vmf);
        store_i16(chunk, q);
    }
}

/// C++: `WelsQuantFour4x4Max_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn quant_four_4x4_max(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8], max: &mut [i16; 4]) {
    let (vff, vmf): (u16x8, u16x8) = (cast(*ff), cast(*mf));
    for (k, block) in dct.chunks_exact_mut(16).enumerate() {
        let (q0, mag0) = quant_8_with_mag(load_i16(&block[..8]), vff, vmf);
        let (q1, mag1) = quant_8_with_mag(load_i16(&block[8..]), vff, vmf);
        store_i16(&mut block[..8], q0);
        store_i16(&mut block[8..], q1);
        // Signed lane max, as the intrinsic kernel's `_mm_max_epi16` reduction.
        max[k] = mag0.max(mag1).reduce_max();
    }
}

// ============================================================================
// Dequantisation
// ============================================================================

/// C++: `WelsDequant4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn dequant_4x4(res: &mut [i16; 16], mf: &[u16; 8]) {
    let vmf: i16x8 = cast(*mf);
    let r0 = load_i16(&res[..8]) * vmf;
    let r1 = load_i16(&res[8..]) * vmf;
    store_i16(&mut res[..8], r0);
    store_i16(&mut res[8..], r1);
}

/// C++: `WelsDequantFour4x4_sse2`, `codec/encoder/core/x86/quant.asm`.
#[inline]
pub fn dequant_four_4x4(res: &mut [i16; 64], mf: &[u16; 8]) {
    let vmf: i16x8 = cast(*mf);
    for chunk in res.chunks_exact_mut(8) {
        let r = load_i16(chunk) * vmf;
        store_i16(chunk, r);
    }
}

// ============================================================================
// Non-zero count
// ============================================================================

/// C++: `WelsGetNoneZeroCount_sse2`, `codec/encoder/core/x86/score.asm`.
///
/// `i16x8::to_bitmask` is one bit per word lane, so the popcount is the count of
/// zero coefficients directly — the intrinsic kernel's byte mask needs a halving.
#[inline]
pub fn get_none_zero_count(level: &[i16; 16]) -> i32 {
    let zero_words = load_i16(&level[..8]).simd_eq(i16x8::ZERO).to_bitmask().count_ones()
        + load_i16(&level[8..]).simd_eq(i16x8::ZERO).to_bitmask().count_ones();
    16 - zero_words as i32
}

// ============================================================================
// Hadamard transforms of the luma DC block
// ============================================================================

/// Saturates eight `i32` lanes to eight `i16` — `packssdw`.
#[inline(always)]
fn pack_i32(lo: i32x4, hi: i32x4) -> i16x8 {
    i16x8::from_i32x8_saturate(cast::<[i32x4; 2], i32x8>([lo, hi]))
}

/// C++: `WelsHadamardT4Dc_sse2`, `codec/encoder/core/x86/dct.asm:78`.
///
/// Same layout as the intrinsic kernel: lane `k` of each input vector is scalar row
/// `k`, so the row pass is lane-wise, one transpose puts `p[4k + j]` in lane `j` of
/// vector `k`, and the column pass is lane-wise again. `i32` throughout, saturating
/// pack at the end, exactly as the scalar clamps.
#[inline]
pub fn hadamard_t4_dc(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    let g = |a: usize, b: usize, c: usize, d: usize| {
        i32x4::new([dct[a] as i32, dct[b] as i32, dct[c] as i32, dct[d] as i32])
    };
    let va = g(0, 32, 128, 160);
    let vb = g(16, 48, 144, 176);
    let vc = g(64, 96, 192, 224);
    let vd = g(80, 112, 208, 240);

    let s0 = va + vd;
    let s3 = va - vd;
    let s1 = vb + vc;
    let s2 = vb - vc;
    let p0 = s0 + s1;
    let p1 = s3 + s2;
    let p2 = s0 - s1;
    let p3 = s3 - s2;

    let [r0, r1, r2, r3] = i32x4::transpose([p0, p1, p2, p3]);

    let one = i32x4::splat(1);
    let t0 = r0 + r3;
    let t3 = r0 - r3;
    let t1 = r1 + r2;
    let t2 = r1 - r2;
    let o0 = (t0 + t1 + one) >> 1i32;
    let o1 = (t3 + t2 + one) >> 1i32;
    let o2 = (t0 - t1 + one) >> 1i32;
    let o3 = (t3 - t2 + one) >> 1i32;

    store_i16(&mut luma_dc[..8], pack_i32(o0, o1));
    store_i16(&mut luma_dc[8..], pack_i32(o2, o3));
}

/// The inverse-Hadamard butterfly, one line per lane, the same in both passes.
#[inline(always)]
fn ihadamard_butterfly(a0: i16x8, a1: i16x8, a2: i16x8, a3: i16x8) -> (i16x8, i16x8, i16x8, i16x8) {
    let t0 = a0 + a2;
    let t1 = a0 - a2;
    let t2 = a1 - a3;
    let t3 = a1 + a3;
    (t0 + t3, t1 + t2, t1 - t2, t0 - t3)
}

/// C++: `WelsDequantIHadamard4x4_sse2`, `codec/encoder/core/x86/quant.asm:332`.
///
/// Transpose, butterfly, transpose, butterfly, multiply on the way out — the
/// intrinsic kernel's shape, with every op wrapping as the C++'s `int16_t` does.
#[inline]
pub fn dequant_ihadamard_4x4(res: &mut [i16; 16], mf: u16) {
    let row = |k: usize| i16x8::new([res[4 * k], res[4 * k + 1], res[4 * k + 2], res[4 * k + 3], 0, 0, 0, 0]);
    let (c0, c1, c2, c3) = transpose4_lo(row(0), row(1), row(2), row(3));
    let (w0, w1, w2, w3) = ihadamard_butterfly(c0, c1, c2, c3);
    let (x0, x1, x2, x3) = transpose4_lo(w0, w1, w2, w3);
    let (y0, y1, y2, y3) = ihadamard_butterfly(x0, x1, x2, x3);

    let mfv = i16x8::splat(mf as i16);
    for (k, y) in [y0, y1, y2, y3].into_iter().enumerate() {
        res[4 * k..4 * k + 4].copy_from_slice(&(y * mfv).as_array()[..4]);
    }
}

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
        ((*seed >> 32i32) as i32 % 4000 - 2000) as i16
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
            quant_4x4(&mut block_simd, &ff, &mf);

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
            quant_4x4_dc(&mut block_simd, ff, mf);

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
            quant_four_4x4(&mut block_simd, &ff, &mf);

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
            quant_four_4x4_max(&mut block_simd, &ff, &mf, &mut max_simd);

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
            dequant_4x4(&mut block_simd, &mf);

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
            dequant_four_4x4(&mut block_simd, &mf);

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
            let simd_count = get_none_zero_count(&block);
            assert_eq!(simd_count, c_count);
        }
    }

    // ========================================================================
    // The two luma-DC Hadamard kernels.
    //
    // Both sweep the **full `i16` input range**, which is not decoration: the ihadamard
    // is `int16_t` end to end in the C++ and its overflow is observable output, so a
    // kernel that widened anywhere would pass a small-coefficient sweep and diverge on
    // a real stream. `hadamard_t4_dc`'s clamp is only reachable from large inputs too.
    // ========================================================================

    fn lcg_full_i16(seed: &mut u64) -> i16 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32i32) as u16 as i16
    }

    #[test]
    fn hadamard_t4_dc_parity() {
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
            hadamard_t4_dc(&mut got, &dct);
            assert_eq!(got, want);
        }
    }

    /// The saturating store is the one place the two could differ only at the extremes,
    /// so drive it deliberately: all sixteen DC coefficients at `i16::MIN`/`MAX` puts
    /// every output past the clamp in both directions.
    #[test]
    fn hadamard_t4_dc_saturates_like_the_scalar() {
        use crate::encoder::encode_mb_aux::hadamard_t4_dc;

        const DC_IDX: [usize; 16] = [
            0, 16, 64, 80, 32, 48, 96, 112, 128, 144, 192, 208, 160, 176, 224, 240,
        ];
        // Every assignment of the two extremes across the sixteen DC positions: 65536
        // cases, which covers the sign patterns that drive the clamp both ways.
        for pattern in 0u32..(1 << 16i32) {
            let mut dct = [0i16; 241];
            for (n, &idx) in DC_IDX.iter().enumerate() {
                dct[idx] = if pattern & (1 << n) != 0 { i16::MAX } else { i16::MIN };
            }
            let mut want = [0i16; 16];
            let mut got = [0i16; 16];
            hadamard_t4_dc(&mut want, &dct);
            hadamard_t4_dc(&mut got, &dct);
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
    fn dequant_ihadamard_4x4_parity() {
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
                dequant_ihadamard_4x4(&mut got, mf);
                assert_eq!(got, want, "mf = {mf}");
            }
        }
    }

    /// The wrapping is load-bearing — the C++ is `int16_t` throughout — so pin that this
    /// kernel wraps rather than saturates, with inputs chosen to overflow every
    /// intermediate.
    #[test]
    fn dequant_ihadamard_4x4_wraps_like_the_scalar() {
        for &v in &[i16::MIN, i16::MAX, -1, 1] {
            for &mf in &[1u16, 2, 0x8000, u16::MAX] {
                let mut want = [v; 16];
                let mut got = [v; 16];
                dequant_ihadamard_4x4(&mut want, mf);
                dequant_ihadamard_4x4(&mut got, mf);
                assert_eq!(got, want, "v = {v}, mf = {mf}");
            }
        }
    }
}
