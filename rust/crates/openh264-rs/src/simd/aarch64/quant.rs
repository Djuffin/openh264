//! Quantisation, dequantisation, the luma-DC Hadamard pair and the non-zero count —
//! `WelsQuant*_AArch64_neon`, `WelsDequant*_AArch64_neon`, `WelsHadamardT4Dc_AArch64_neon`,
//! `WelsDequantIHadamard4x4_AArch64_neon` and `WelsGetNoneZeroCount_AArch64_neon`,
//! all in `codec/encoder/core/arm64/reconstruct_aarch64_neon.S`.
//!
//! # Where this departs from the asm, and why
//!
//! `NEWQUANT_COEF_EACH_16BITS` is `saba` (a wrapping `ff + |coef|` in word lanes),
//! `smull` (a *signed* multiply by `mf`) and `shrn #16`. That is exact only while
//! `ff + |coef|` stays below 32768; past it the sum wraps negative and the signed
//! product goes wrong. The C — and this port's scalar — computes `(ff + |coef|) * mf`
//! in `int`, and the range is reachable: a luma DC block after the forward Hadamard
//! can sit at 32640 with `ff` up to 1534 on top. So the sum here is an unsigned
//! saturating add (`uqadd`, which never actually saturates: 32768 + 1534 is well
//! inside `u16`) and the multiply is `umull`, and the pair agrees with the scalar
//! over the whole `i16` range, which `quant_matches_the_scalar_at_the_extremes` holds.
//!
//! The sign step is spelled the scalar's way too. The asm restores the sign with
//! `cmgt coef, #0` / `bif` / `shl` / `sub` — `q - 2q` for every coefficient that is
//! *not* positive, zero included. The scalar negates only where `coef < 0`. The two
//! differ only when `coef == 0` and `(ff * mf) >> 16` is non-zero, which no table in
//! the codec produces (the dead-zone offset is a fraction of a quantiser step), but
//! the scalar is the contract, so the select here is on `coef < 0`.
//!
//! Everything else is the asm: the `mul` of the dequantisers, the widening
//! butterflies and `uzp` transposes of the forward Hadamard, and the `uzp`/`zip`/
//! `rev32` dance of the inverse one, which keeps both rows of a register in play at
//! once and is transcribed step for step.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld4_i16, ld8_i16, st8_i16};

// ============================================================================
// Forward quantisation
// ============================================================================

/// Dead-zone quantisation of eight coefficients: the signed result and the unsigned
/// magnitude — `NEWQUANT_COEF_EACH_16BITS_MAX`, with the widths the header explains.
#[inline]
#[target_feature(enable = "neon")]
fn quant_8_with_mag(v: int16x8_t, ff: uint16x8_t, mf: uint16x8_t) -> (int16x8_t, uint16x8_t) {
    let negative = vcltzq_s16(v);
    // `|i16::MIN|` wraps to `0x8000`, which read as `u16` is the magnitude wanted.
    let abs = vreinterpretq_u16_s16(vabsq_s16(v));
    let biased = vqaddq_u16(abs, ff);
    let lo = vmull_u16(vget_low_u16(biased), vget_low_u16(mf));
    let hi = vmull_high_u16(biased, mf);
    let mag = vcombine_u16(vshrn_n_u32::<16>(lo), vshrn_n_u32::<16>(hi));
    let q = vreinterpretq_s16_u16(mag);
    (vbslq_s16(negative, vnegq_s16(q), q), mag)
}

#[inline]
#[target_feature(enable = "neon")]
fn quant_8(v: int16x8_t, ff: uint16x8_t, mf: uint16x8_t) -> int16x8_t {
    quant_8_with_mag(v, ff, mf).0
}

#[inline]
#[target_feature(enable = "neon")]
fn ld8_u16(r: &[i16]) -> uint16x8_t {
    vreinterpretq_u16_s16(ld8_i16(r))
}

/// `WelsQuant4x4_AArch64_neon`.
#[inline]
#[target_feature(enable = "neon")]
fn quant_4x4_neon(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    let (vff, vmf) = (ld8_u16(ff), ld8_u16(mf));
    let q0 = quant_8(ld8_i16(&dct[..8]), vff, vmf);
    let q1 = quant_8(ld8_i16(&dct[8..]), vff, vmf);
    st8_i16(&mut dct[..8], q0);
    st8_i16(&mut dct[8..], q1);
}

/// `WelsQuant4x4Dc_AArch64_neon`: the same with `dup`ped factors.
#[inline]
#[target_feature(enable = "neon")]
fn quant_4x4_dc_neon(dct: &mut [i16; 16], ff: i16, mf: i16) {
    let (vff, vmf) = (vdupq_n_u16(ff as u16), vdupq_n_u16(mf as u16));
    let q0 = quant_8(ld8_i16(&dct[..8]), vff, vmf);
    let q1 = quant_8(ld8_i16(&dct[8..]), vff, vmf);
    st8_i16(&mut dct[..8], q0);
    st8_i16(&mut dct[8..], q1);
}

/// `WelsQuantFour4x4_AArch64_neon`.
#[inline]
#[target_feature(enable = "neon")]
fn quant_four_4x4_neon(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8]) {
    let (vff, vmf) = (ld8_u16(ff), ld8_u16(mf));
    for chunk in dct.chunks_exact_mut(8) {
        let q = quant_8(ld8_i16(chunk), vff, vmf);
        st8_i16(chunk, q);
    }
}

/// `WelsQuantFour4x4Max_AArch64_neon`: `SELECT_MAX_IN_ABS_COEF` is `umax` of the
/// two magnitude vectors of a block and `umaxv` across the lanes.
#[inline]
#[target_feature(enable = "neon")]
fn quant_four_4x4_max_neon(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8], max: &mut [i16; 4]) {
    let (vff, vmf) = (ld8_u16(ff), ld8_u16(mf));
    for (k, block) in dct.chunks_exact_mut(16).enumerate() {
        let (q0, mag0) = quant_8_with_mag(ld8_i16(&block[..8]), vff, vmf);
        let (q1, mag1) = quant_8_with_mag(ld8_i16(&block[8..]), vff, vmf);
        st8_i16(&mut block[..8], q0);
        st8_i16(&mut block[8..], q1);
        max[k] = vmaxvq_u16(vmaxq_u16(mag0, mag1)) as i16;
    }
}

/// `WelsQuant4x4_AArch64_neon`.
#[inline]
pub fn quant_4x4(dct: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { quant_4x4_neon(dct, ff, mf) }
}

/// `WelsQuant4x4Dc_AArch64_neon`.
#[inline]
pub fn quant_4x4_dc(dct: &mut [i16; 16], ff: i16, mf: i16) {
    unsafe { quant_4x4_dc_neon(dct, ff, mf) }
}

/// `WelsQuantFour4x4_AArch64_neon`.
#[inline]
pub fn quant_four_4x4(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8]) {
    unsafe { quant_four_4x4_neon(dct, ff, mf) }
}

/// `WelsQuantFour4x4Max_AArch64_neon`.
#[inline]
pub fn quant_four_4x4_max(dct: &mut [i16; 64], ff: &[i16; 8], mf: &[i16; 8], max: &mut [i16; 4]) {
    unsafe { quant_four_4x4_max_neon(dct, ff, mf, max) }
}

// ============================================================================
// Dequantisation
// ============================================================================

#[inline]
#[target_feature(enable = "neon")]
fn ld8_mf(mf: &[u16; 8]) -> int16x8_t {
    let mf: [i16; 8] = mf.map(|m| m as i16);
    ld8_i16(&mf)
}

/// `WelsDequant4x4_AArch64_neon`: `mul .8h`, wrapping like the scalar's `wrapping_mul`.
#[inline]
#[target_feature(enable = "neon")]
fn dequant_4x4_neon(res: &mut [i16; 16], mf: &[u16; 8]) {
    let vmf = ld8_mf(mf);
    let r0 = vmulq_s16(ld8_i16(&res[..8]), vmf);
    let r1 = vmulq_s16(ld8_i16(&res[8..]), vmf);
    st8_i16(&mut res[..8], r0);
    st8_i16(&mut res[8..], r1);
}

/// `WelsDequantFour4x4_AArch64_neon`.
#[inline]
#[target_feature(enable = "neon")]
fn dequant_four_4x4_neon(res: &mut [i16; 64], mf: &[u16; 8]) {
    let vmf = ld8_mf(mf);
    for chunk in res.chunks_exact_mut(8) {
        let r = vmulq_s16(ld8_i16(chunk), vmf);
        st8_i16(chunk, r);
    }
}

/// `WelsDequant4x4_AArch64_neon`.
#[inline]
pub fn dequant_4x4(res: &mut [i16; 16], mf: &[u16; 8]) {
    unsafe { dequant_4x4_neon(res, mf) }
}

/// `WelsDequantFour4x4_AArch64_neon`.
#[inline]
pub fn dequant_four_4x4(res: &mut [i16; 64], mf: &[u16; 8]) {
    unsafe { dequant_four_4x4_neon(res, mf) }
}

// ============================================================================
// Non-zero count
// ============================================================================

/// `WelsGetNoneZeroCount_AArch64_neon`: `ZERO_COUNT_IN_2_QUARWORD` — `cmeq #0` on
/// both words vectors, `uzp1` to a byte per coefficient, `ushr #7` to a 0/1, `addv`.
#[inline]
#[target_feature(enable = "neon")]
fn get_none_zero_count_neon(level: &[i16; 16]) -> i32 {
    let z0 = vceqzq_s16(ld8_i16(&level[..8]));
    let z1 = vceqzq_s16(ld8_i16(&level[8..]));
    let zero = vuzp1q_u8(vreinterpretq_u8_u16(z0), vreinterpretq_u8_u16(z1));
    let zeros = vaddvq_u8(vshrq_n_u8::<7>(zero)) as i32;
    16 - zeros
}

/// `WelsGetNoneZeroCount_AArch64_neon`.
#[inline]
pub fn get_none_zero_count(level: &[i16; 16]) -> i32 {
    unsafe { get_none_zero_count_neon(level) }
}

// ============================================================================
// The luma-DC Hadamard pair
// ============================================================================

/// `WelsHadamardT4Dc_AArch64_neon`.
///
/// # Layout
///
/// The asm gathers the sixteen DCs with `ld1 {v.h}[i]` at a 32-byte step into four
/// vectors — `v0 = [0 4 8 12]`, `v1 = [1 5 9 13]`, `v2 = [2 6 10 14]`,
/// `v3 = [3 7 11 15]` in its own numbering — which is: lane `k` of vector `j` holds
/// the `j`th input of the scalar's row `k`, the row whose `idx` is
/// `((i & 8) << 4) + ((i & 4) << 3)` for `i = 4k`, i.e. 0, 32, 128, 160. The row
/// pass is then lane-wise in `.4s` (`saddl`/`ssubl` widen on the way in), the
/// `uzp1`/`uzp2` pairs transpose so the column pass is lane-wise too, and
/// `sqrshrn #1` is the scalar's `((x + 1) >> 1).clamp(-32768, 32767)` in one
/// instruction.
#[inline]
#[target_feature(enable = "neon")]
fn hadamard_t4_dc_neon(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    // Lane k = scalar row k. Within a row: A = dct[idx], B = dct[idx + 16],
    // C = dct[idx + 64], D = dct[idx + 80] — the scalar's d0, d16, d64, d80.
    let va = ld4_i16(&[dct[0], dct[32], dct[128], dct[160]]);
    let vb = ld4_i16(&[dct[16], dct[48], dct[144], dct[176]]);
    let vc = ld4_i16(&[dct[64], dct[96], dct[192], dct[224]]);
    let vd = ld4_i16(&[dct[80], dct[112], dct[208], dct[240]]);

    // ROW_TRANSFORM_0_STEP + TRANSFORM_4BYTES: `pj` holds `p[4k + j]` in lane `k`.
    let s0 = vaddl_s16(va, vd);
    let s3 = vsubl_s16(va, vd);
    let s1 = vaddl_s16(vb, vc);
    let s2 = vsubl_s16(vb, vc);
    let p0 = vaddq_s32(s0, s1);
    let p1 = vaddq_s32(s3, s2);
    let p2 = vsubq_s32(s0, s1);
    let p3 = vsubq_s32(s3, s2);

    // The `uzp` transpose: `rj` now holds `p[4j + i]` in lane `i`.
    let v4 = vuzp1q_s32(p0, p1);
    let v5 = vuzp2q_s32(p0, p1);
    let v6 = vuzp1q_s32(p2, p3);
    let v7 = vuzp2q_s32(p2, p3);
    let r0 = vuzp1q_s32(v4, v6);
    let r2 = vuzp2q_s32(v4, v6);
    let r1 = vuzp1q_s32(v5, v7);
    let r3 = vuzp2q_s32(v5, v7);

    // COL_TRANSFORM_0_STEP + TRANSFORM_4BYTES.
    let t0 = vaddq_s32(r0, r3);
    let t3 = vsubq_s32(r0, r3);
    let t1 = vaddq_s32(r1, r2);
    let t2 = vsubq_s32(r1, r2);
    let o0 = vaddq_s32(t0, t1);
    let o1 = vaddq_s32(t3, t2);
    let o2 = vsubq_s32(t0, t1);
    let o3 = vsubq_s32(t3, t2);

    st8_i16(&mut luma_dc[..8], vqrshrn_high_n_s32::<1>(vqrshrn_n_s32::<1>(o0), o1));
    st8_i16(&mut luma_dc[8..], vqrshrn_high_n_s32::<1>(vqrshrn_n_s32::<1>(o2), o3));
}

/// `WelsHadamardT4Dc_AArch64_neon`.
#[inline]
pub fn hadamard_t4_dc(luma_dc: &mut [i16; 16], dct: &[i16; 241]) {
    unsafe { hadamard_t4_dc_neon(luma_dc, dct) }
}

/// `IHDM_4x4_TOTAL_16BITS`: the inverse-Hadamard butterfly on each of the two rows a
/// register holds as `[a0 a1 a2 a3 | b0 b1 b2 b3]`.
///
/// `uzp1`/`uzp2` on the 32-bit view split each row into its `(0, 1)` and `(2, 3)`
/// pairs; their sum and difference are `(t0, t3)` and `(t1, t2)`; `zip1` re-pairs
/// them as `[t0 t1 t3 t2]`, the same split gives `(t0 + t3, t1 + t2)` and
/// `(t0 - t3, t1 - t2)`, and `rev32` turns the latter into `(t1 - t2, t0 - t3)` —
/// `res[2], res[3]` — before the final `zip1` puts each row back in order.
#[inline]
#[target_feature(enable = "neon")]
fn ihdm_rows(v: int16x8_t) -> int16x8_t {
    let v32 = vreinterpretq_s32_s16(v);
    let hi = vreinterpretq_s16_s32(vuzp2q_s32(v32, v32));
    let lo = vreinterpretq_s16_s32(vuzp1q_s32(v32, v32));
    let sum = vaddq_s16(lo, hi);
    let dif = vsubq_s16(lo, hi);
    let z = vreinterpretq_s32_s16(vzip1q_s16(sum, dif));
    let hi = vreinterpretq_s16_s32(vuzp2q_s32(z, z));
    let lo = vreinterpretq_s16_s32(vuzp1q_s32(z, z));
    let sum = vaddq_s16(lo, hi);
    let dif = vrev32q_s16(vsubq_s16(lo, hi));
    vreinterpretq_s16_s32(vzip1q_s32(vreinterpretq_s32_s16(sum), vreinterpretq_s32_s16(dif)))
}

/// `MATRIX_TRANSFORM_EACH_16BITS_2x8_OUT2`: the 4x4 held as `[row0 | row1]`,
/// `[row2 | row3]` comes back as `[col0 | col1]`, `[col2 | col3]`.
#[inline]
#[target_feature(enable = "neon")]
fn transpose_2x8(v0: int16x8_t, v1: int16x8_t) -> (int16x8_t, int16x8_t) {
    let (a32, b32) = (vreinterpretq_s32_s16(v0), vreinterpretq_s32_s16(v1));
    let a = vreinterpretq_s16_s32(vuzp1q_s32(a32, b32)); // [0 1 4 5 | 8 9 12 13]
    let b = vreinterpretq_s16_s32(vuzp2q_s32(a32, b32)); // [2 3 6 7 | 10 11 14 15]
    let c = vreinterpretq_s64_s16(vuzp1q_s16(a, b)); // [0 4 8 12 | 2 6 10 14]
    let d = vreinterpretq_s64_s16(vuzp2q_s16(a, b)); // [1 5 9 13 | 3 7 11 15]
    (
        vreinterpretq_s16_s64(vzip1q_s64(c, d)), // [0 4 8 12 | 1 5 9 13]
        vreinterpretq_s16_s64(vzip2q_s64(c, d)), // [2 6 10 14 | 3 7 11 15]
    )
}

/// `WelsDequantIHadamard4x4_AArch64_neon`: rows, transpose, columns scaled by `mf`,
/// transpose back. The multiply sits after the second pass, as the scalar's does.
#[inline]
#[target_feature(enable = "neon")]
fn dequant_ihadamard_4x4_neon(res: &mut [i16; 16], mf: u16) {
    let v0 = ihdm_rows(ld8_i16(&res[..8]));
    let v1 = ihdm_rows(ld8_i16(&res[8..]));
    let (v0, v1) = transpose_2x8(v0, v1);
    let mfv = vdupq_n_s16(mf as i16);
    let v0 = vmulq_s16(ihdm_rows(v0), mfv);
    let v1 = vmulq_s16(ihdm_rows(v1), mfv);
    let (v0, v1) = transpose_2x8(v0, v1);
    st8_i16(&mut res[..8], v0);
    st8_i16(&mut res[8..], v1);
}

/// `WelsDequantIHadamard4x4_AArch64_neon`.
#[inline]
pub fn dequant_ihadamard_4x4(res: &mut [i16; 16], mf: u16) {
    unsafe { dequant_ihadamard_4x4_neon(res, mf) }
}

// ============================================================================
// Unit Tests & Parity
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::decode_mb_aux as dec;
    use crate::encoder::encode_mb_aux as enc;
    use crate::encoder::encode_mb_aux::{g_kiQuantMF, G_KI_QUANT_INTER_FF};

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
            let mut want = [0i16; 16];
            for v in want.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut got = want;
            enc::quant_4x4(&mut want, &ff, &mf);
            quant_4x4(&mut got, &ff, &mf);
            assert_eq!(got, want);
        }
    }

    #[test]
    fn test_quant_4x4_dc_parity() {
        let mut seed = 123u64;
        for _ in 0..100 {
            let mut want = [0i16; 16];
            for v in want.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut got = want;
            enc::quant_4x4_dc(&mut want, 17, 26);
            quant_4x4_dc(&mut got, 17, 26);
            assert_eq!(got, want);
        }
    }

    #[test]
    fn test_quant_four_4x4_parity() {
        let mut seed = 999u64;
        let ff = G_KI_QUANT_INTER_FF.0[22];
        let mf = g_kiQuantMF[22];
        for _ in 0..50 {
            let mut want = [0i16; 64];
            for v in want.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut got = want;
            enc::quant_four_4x4(&mut want, &ff, &mf);
            quant_four_4x4(&mut got, &ff, &mf);
            assert_eq!(got, want);
        }
    }

    #[test]
    fn test_quant_four_4x4_max_parity() {
        let mut seed = 7777u64;
        let ff = G_KI_QUANT_INTER_FF.0[28];
        let mf = g_kiQuantMF[28];
        for _ in 0..50 {
            let mut want = [0i16; 64];
            for v in want.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut got = want;
            let (mut max_want, mut max_got) = ([0i16; 4], [0i16; 4]);
            enc::quant_four_4x4_max(&mut want, &ff, &mf, &mut max_want);
            quant_four_4x4_max(&mut got, &ff, &mf, &mut max_got);
            assert_eq!(got, want);
            assert_eq!(max_got, max_want);
        }
    }

    /// The header's reason for `uqadd`/`umull` over the asm's `saba`/`smull`: every
    /// QP's factors against coefficients at the ends of the range, where the
    /// biased magnitude leaves `i16` and a signed multiply would go wrong. The
    /// scalar is the reference, over its full input space.
    #[test]
    fn quant_matches_the_scalar_at_the_extremes() {
        let extremes = [i16::MIN, -32767, -32000, -31234, -1, 0, 1, 31234, 32000, 32767];
        // `g_kiQuantMF` has 52 rows to the FF table's 58; every QP has both.
        for qp in 0..g_kiQuantMF.len() {
            let ff = G_KI_QUANT_INTER_FF.0[qp];
            let mf = g_kiQuantMF[qp];
            for &v in &extremes {
                let mut want = [v; 16];
                let mut got = [v; 16];
                enc::quant_4x4(&mut want, &ff, &mf);
                quant_4x4(&mut got, &ff, &mf);
                assert_eq!(got, want, "quant_4x4, qp {qp}, v {v}");

                let mut want = [v; 64];
                let mut got = [v; 64];
                let (mut mw, mut mg) = ([0i16; 4], [0i16; 4]);
                enc::quant_four_4x4_max(&mut want, &ff, &mf, &mut mw);
                quant_four_4x4_max(&mut got, &ff, &mf, &mut mg);
                assert_eq!((got, mg), (want, mw), "quant_four_4x4_max, qp {qp}, v {v}");

                // The DC path's callers pass `ff << 1` and `mf >> 1`.
                let mut want = [v; 16];
                let mut got = [v; 16];
                enc::quant_4x4_dc(&mut want, ff[0] << 1, mf[0] >> 1);
                quant_4x4_dc(&mut got, ff[0] << 1, mf[0] >> 1);
                assert_eq!(got, want, "quant_4x4_dc, qp {qp}, v {v}");
            }
        }
    }

    #[test]
    fn test_dequant_4x4_parity() {
        let mut seed = 8888u64;
        let mf: [u16; 8] = [10, 13, 16, 13, 10, 13, 16, 13];
        for _ in 0..100 {
            let mut want = [0i16; 16];
            for v in want.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut got = want;
            dec::dequant_4x4(&mut want, &mf);
            dequant_4x4(&mut got, &mf);
            assert_eq!(got, want);
        }
    }

    #[test]
    fn test_dequant_four_4x4_parity() {
        let mut seed = 5555u64;
        let mf: [u16; 8] = [10, 13, 16, 13, 10, 13, 16, 13];
        for _ in 0..50 {
            let mut want = [0i16; 64];
            for v in want.iter_mut() {
                *v = lcg(&mut seed);
            }
            let mut got = want;
            dec::dequant_four_4x4(&mut want, &mf);
            dequant_four_4x4(&mut got, &mf);
            assert_eq!(got, want);
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
            assert_eq!(get_none_zero_count(&block), enc::get_none_zero_count(&block));
        }
        // Every mask, since the count is a function of the zero pattern alone.
        for mask in 0u32..=0xFFFF {
            let block: [i16; 16] = core::array::from_fn(|i| if (mask >> i) & 1 != 0 { 256 } else { 0 });
            assert_eq!(get_none_zero_count(&block), enc::get_none_zero_count(&block), "mask {mask:#06x}");
        }
    }

    // ========================================================================
    // The two luma-DC Hadamard kernels, over the full `i16` input range: the
    // ihadamard is `int16_t` end to end and its overflow is observable output, and
    // `hadamard_t4_dc`'s clamp is only reachable from large inputs.
    // ========================================================================

    fn lcg_full_i16(seed: &mut u64) -> i16 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32) as u16 as i16
    }

    #[test]
    fn hadamard_t4_dc_parity() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..2000 {
            let mut dct = [0i16; 241];
            for v in dct.iter_mut() {
                *v = lcg_full_i16(&mut seed);
            }
            let mut want = [0i16; 16];
            let mut got = [0i16; 16];
            enc::hadamard_t4_dc(&mut want, &dct);
            hadamard_t4_dc(&mut got, &dct);
            assert_eq!(got, want);
        }
    }

    /// All sixteen DCs at `i16::MIN`/`MAX` in every pattern: 65536 cases that drive
    /// the `sqrshrn` saturation both ways.
    #[test]
    fn hadamard_t4_dc_saturates_like_the_scalar() {
        const DC_IDX: [usize; 16] = [0, 16, 64, 80, 32, 48, 96, 112, 128, 144, 192, 208, 160, 176, 224, 240];
        for pattern in 0u32..(1 << 16) {
            let mut dct = [0i16; 241];
            for (n, &idx) in DC_IDX.iter().enumerate() {
                dct[idx] = if pattern & (1 << n) != 0 { i16::MAX } else { i16::MIN };
            }
            let mut want = [0i16; 16];
            let mut got = [0i16; 16];
            enc::hadamard_t4_dc(&mut want, &dct);
            hadamard_t4_dc(&mut got, &dct);
            assert_eq!(got, want, "pattern {pattern:#018b}");
        }
    }

    #[test]
    fn dequant_ihadamard_4x4_parity() {
        let mut seed = 0x8A5C_D789_635D_2DFFu64;
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
                dec::dequant_ihadamard_4x4(&mut want, mf);
                dequant_ihadamard_4x4(&mut got, mf);
                assert_eq!(got, want, "mf = {mf}");
            }
        }
    }

    /// The wrapping is load-bearing — the C++ is `int16_t` throughout — so pin that
    /// the kernel wraps rather than saturates, with inputs chosen to overflow every
    /// intermediate.
    #[test]
    fn dequant_ihadamard_4x4_wraps_like_the_scalar() {
        for &v in &[i16::MIN, i16::MAX, -1, 1] {
            for &mf in &[1u16, 2, 0x8000, u16::MAX] {
                let mut want = [v; 16];
                let mut got = [v; 16];
                dec::dequant_ihadamard_4x4(&mut want, mf);
                dequant_ihadamard_4x4(&mut got, mf);
                assert_eq!(got, want, "v = {v}, mf = {mf}");
            }
        }
    }

    /// The transpose is the one step with no arithmetic to check it, so drive it
    /// with a block whose every coefficient is its own index — after both passes the
    /// scalar's answer is the oracle, but a permutation mistake shows as a
    /// recognisable shuffle rather than as noise.
    #[test]
    fn dequant_ihadamard_4x4_on_an_index_ramp() {
        let mut want: [i16; 16] = core::array::from_fn(|i| i as i16 * 100 - 700);
        let mut got = want;
        dec::dequant_ihadamard_4x4(&mut want, 3);
        dequant_ihadamard_4x4(&mut got, 3);
        assert_eq!(got, want);
    }
}
