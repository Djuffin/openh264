//! Port of the reconstruction half of `codec/encoder/core/src/decode_mb_aux.cpp` —
//! the dequantisation and inverse-transform kernels, plus
//! `WelsInitReconstructionFuncs`, which installs them.
//!
//! `WelsIHadamard4x4Dc`, `WelsDequantLumaDc4x4`, `WelsDequantIHadamard2x2Dc`,
//! `WelsIDctT4Rec_c` and `WelsIDctFourT4Rec_c` were already ported into
//! `svc_encode_mb.rs`; they are re-exported here so the table filler reads like the
//! C++ and so this module is the single place that describes the file.
//!
//! Only the `_c` scalar variants exist. The SIMD overrides in the C++ are behind
//! `uiCpuFlag` tests that do not fire on any target this port builds for.

#![allow(non_snake_case, dead_code)]

#![deny(unsafe_code)]

use crate::encoder::svc_encode_mb::PIDctFunc;
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

pub use crate::encoder::svc_encode_mb::{
    WelsDequantIHadamard2x2Dc, WelsDequantLumaDc4x4, WelsIDctFourT4Rec_c, WelsIDctT4Rec_c,
    WelsIHadamard4x4Dc,
};

#[inline(always)]
fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        if -iX < 0 { 255 } else { 0 }
    } else {
        iX as u8
    }
}

// ---------------------------------------------------------------------------
// Safe kernels (plan §Phase 2, recipe R2). These are the implementations for
// the whole C++ `decode_mb_aux.cpp` family — including the five kernels whose
// raw bodies live in `svc_encode_mb.rs` (the port split the file; this module
// re-exports them and is the single place that describes it). The raw `Wels*`
// functions in both files are strangler shims (R7) onto these.
//
// Arithmetic parity (rule R-e): each kernel reproduces the raw port's widths
// and operations exactly — including two shapes worth naming. The dequant
// kernels' `wrapping_mul`/`wrapping_add` in `i16` match the C++'s implicit
// `int -> int16_t` narrowing (truncation mod 2^16 commutes with + and *, so
// wrapping `i16` arithmetic equals computing in `int` and narrowing each
// store). And `ihadamard_4x4_dc` keeps the port's **plain** `i16` additions,
// which can overflow and panic in a debug build where the C++ wraps — that is
// finding F11 (`phase2_findings.md`), reproduced rather than repaired.
// ---------------------------------------------------------------------------

use crate::encoder::svc_encode_mb::g_kuiDequantCoeff;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut};

/// Inverse 4x4 Hadamard of the luma DC block, then scale by the
/// dequantisation multiplier. The qp >= 12 path (the qp < 12 path is
/// [`ihadamard_4x4_dc`] + [`dequant_luma_dc_4x4`]).
///
/// All arithmetic is wrapping `i16`, exactly the raw port's — equal to the
/// C++'s `int` arithmetic narrowed at every `int16_t` store, so it is total
/// over the full input range.
///
/// C++: `WelsDequantIHadamard4x4_c`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn dequant_ihadamard_4x4(res: &mut [i16; 16], mf: u16) {
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

/// Scale one 4x4 coefficient block in place: lane `i & 7` of the MF row
/// multiplies coefficients `i` and `i + 8`.
///
/// C++: `WelsDequant4x4_c`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn dequant_4x4(res: &mut [i16; 16], mf: &[u16; 8]) {
    for i in 0..8usize {
        res[i] = res[i].wrapping_mul(mf[i] as i16);
        res[i + 8] = res[i + 8].wrapping_mul(mf[i] as i16);
    }
}

/// Scale four consecutive 4x4 coefficient blocks in place with one MF row.
///
/// C++: `WelsDequantFour4x4_c`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn dequant_four_4x4(res: &mut [i16; 64], mf: &[u16; 8]) {
    for i in 0..8usize {
        let m = mf[i] as i16;
        for k in 0..8usize {
            res[i + (k << 3)] = res[i + (k << 3)].wrapping_mul(m);
        }
    }
}

/// Luma reconstruction of an I16x16 macroblock when only the DC coefficients
/// are non-zero: block `(i >> 2, j >> 2)` adds its rounded DC
/// (`(dc + 32) >> 6`) to every prediction sample, saturated to `[0, 255]`.
///
/// C++: `WelsIDctRecI16x16Dc_c`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn idct_rec_i16x16_dc(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dc: &[i16; 16]) {
    for i in 0..16usize {
        let r: &mut [u8; 16] = rec.row_mut(i as isize, 0, 16).try_into().unwrap();
        let p: &[u8; 16] = pred.row(i as isize, 0, 16).try_into().unwrap();
        for (j, (rv, &pv)) in r.iter_mut().zip(p.iter()).enumerate() {
            let d = dc[(i & 0x0C) + (j >> 2)] as i32;
            *rv = WelsClip1(pv as i32 + ((d + 32) >> 6));
        }
    }
}

/// 4x4 inverse integer DCT of `dct`, added to the prediction block and
/// saturated into the reconstruction block.
///
/// The horizontal pass narrows each intermediate with `as i16`
/// (`iTemp[16]` is `int16_t` in the C++ and the sums can exceed it — the
/// truncation is observable and load-bearing); the vertical pass and the
/// `+32 >> 6` rounding run in `i32`, total over the full coefficient range.
///
/// C++: `WelsIDctT4Rec_c`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn idct_t4_rec(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 16]) {
    let res = idct_t4_residual(dct);
    for (dy, r) in res.iter().enumerate() {
        let p: &[u8; 4] = pred.row(dy as isize, 0, 4).try_into().unwrap();
        let out: &mut [u8; 4] = rec.row_mut(dy as isize, 0, 4).try_into().unwrap();
        for ((o, &pv), &v) in out.iter_mut().zip(p.iter()).zip(r.iter()) {
            *o = WelsClip1(pv as i32 + ((v + 32) >> 6));
        }
    }
}

/// [`idct_t4_rec`] with the prediction already *in* `rec` — the inter-macroblock
/// reconstruction, where the C++ passes the reconstruction plane as both `pRec`
/// and `pPred` (`OutputPMbWithoutConstructCsRsNoCopy`, `svc_encode_slice.cpp`) and
/// the kernel adds each residual to the sample it then overwrites. Element-wise
/// that is well defined; as two Rust references over one span it is not, and the
/// encoder aliasing probe caught the shim building exactly that pair (**F59**,
/// Phase 6 session B). Same arithmetic, one cursor: the sample is read where
/// [`idct_t4_rec`] reads `pred`, and written where it writes `rec`.
pub fn idct_t4_rec_in_place(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 16]) {
    let res = idct_t4_residual(dct);
    for (dy, r) in res.iter().enumerate() {
        let out: &mut [u8; 4] = rec.row_mut(dy as isize, 0, 4).try_into().unwrap();
        for (o, &v) in out.iter_mut().zip(r.iter()) {
            *o = WelsClip1(*o as i32 + ((v + 32) >> 6));
        }
    }
}

/// The transform half of [`idct_t4_rec`]: the 4x4 residual before the
/// `+32 >> 6` rounding and the add to the prediction. Shared by the two-plane and
/// the in-place reconstruction so the arithmetic exists once.
#[inline]
fn idct_t4_residual(dct: &[i16; 16]) -> [[i32; 4]; 4] {
    let mut tmp = [0i16; 16];

    for i in 0..4usize {
        let idx = i << 2;
        let sum_u = dct[idx] as i32 + dct[idx + 2] as i32;
        let del_u = dct[idx] as i32 - dct[idx + 2] as i32;
        let sum_d = dct[idx + 1] as i32 + (dct[idx + 3] as i32 >> 1);
        let del_d = (dct[idx + 1] as i32 >> 1) - dct[idx + 3] as i32;

        tmp[idx] = (sum_u + sum_d) as i16;
        tmp[idx + 1] = (del_u + del_d) as i16;
        tmp[idx + 2] = (del_u - del_d) as i16;
        tmp[idx + 3] = (sum_u - sum_d) as i16;
    }

    // The C++ walks columns with four strided stores each; every sample is
    // written exactly once, so transposing to row-major is bit-exact and each
    // row becomes one bounds check and a fixed-size window (the decoder
    // pilot's shape, plan §7.4).
    let mut res = [[0i32; 4]; 4];
    for i in 0..4usize {
        let sum_l = tmp[i] as i32 + tmp[8 + i] as i32;
        let del_l = tmp[i] as i32 - tmp[8 + i] as i32;
        let del_r = (tmp[4 + i] as i32 >> 1) - tmp[12 + i] as i32;
        let sum_r = tmp[4 + i] as i32 + (tmp[12 + i] as i32 >> 1);

        res[0][i] = sum_l + sum_r;
        res[1][i] = del_l + del_r;
        res[2][i] = del_l - del_r;
        res[3][i] = sum_l - sum_r;
    }
    res
}

/// [`idct_t4_rec`] over the four 4x4 blocks of one 8x8 quadrant.
///
/// C++: `WelsIDctFourT4Rec_c`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn idct_four_t4_rec(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
        idct_t4_rec(&mut rec.reborrow(dx, dy), &pred.advance(dx, dy), sub);
    }
}

/// [`idct_t4_rec_in_place`] over the four 4x4 blocks of one 8x8 quadrant (F59).
pub fn idct_four_t4_rec_in_place(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
        idct_t4_rec_in_place(&mut rec.reborrow(dx, dy), sub);
    }
}

/// Inverse 4x4 Hadamard for the I16x16 luma DC block, qp < 12 path.
///
/// The additions are **plain `i16`**, exactly the raw port's: with a 16x
/// worst-case gain across the two passes, an input above `+-2047` can
/// overflow an intermediate — a debug build panics where the C++'s `int`
/// arithmetic narrows (finding F11, `phase2_findings.md`). Reproduced, not
/// repaired; in-contract DC levels are far below the threshold.
///
/// C++: `WelsIHadamard4x4Dc`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn ihadamard_4x4_dc(res: &mut [i16; 16]) {
    let mut t = [0i16; 4];

    for i in (0..4usize).rev() {
        let idx = i << 2;
        t[0] = res[idx] + res[idx + 2];
        t[1] = res[idx] - res[idx + 2];
        t[2] = res[idx + 1] - res[idx + 3];
        t[3] = res[idx + 1] + res[idx + 3];

        res[idx] = t[0] + t[3];
        res[idx + 1] = t[1] + t[2];
        res[idx + 2] = t[1] - t[2];
        res[idx + 3] = t[0] - t[3];
    }

    for i in (0..4usize).rev() {
        t[0] = res[i] + res[i + 8];
        t[1] = res[i] - res[i + 8];
        t[2] = res[i + 4] - res[i + 12];
        t[3] = res[i + 4] + res[i + 12];

        res[i] = t[0] + t[3];
        res[i + 4] = t[1] + t[2];
        res[i + 8] = t[1] - t[2];
        res[i + 12] = t[0] - t[3];
    }
}

/// Dequantisation of the 16 luma DC coefficients for qp < 12:
/// `(v * mf + round) >> shift` in `i32`, narrowed per store.
///
/// `qp < 12` is part of the contract: at `qp >= 12` the shift count
/// `1 - qp/6` goes negative, which panics in a debug build exactly as the
/// raw port does (the one caller is gated on `uiQp < 12`,
/// `svc_encode_mb.rs:586`).
///
/// C++: `WelsDequantLumaDc4x4`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn dequant_luma_dc_4x4(res: &mut [i16; 16], qp: i32) {
    let value = g_kuiDequantCoeff[(qp % 6) as usize][0] as i32;
    let qf0 = qp / 6;
    let qf1 = 2 - qf0;
    let qf0s = 1i32 << (1 - qf0);

    for v in res.iter_mut() {
        *v = ((*v as i32 * value + qf0s) >> qf1) as i16;
    }
}

/// Inverse 2x2 Hadamard and dequantisation of the four chroma DC
/// coefficients: butterfly in `i32`, `* mf >> 1`, narrowed per store. Total
/// for any table `mf` (max 5888: `|sum| <= 2^17`, product `< 2^31`).
///
/// C++: `WelsDequantIHadamard2x2Dc`, `codec/encoder/core/src/decode_mb_aux.cpp`.
pub fn dequant_ihadamard_2x2_dc(dct: &mut [i16; 4], mf: u16) {
    let sum_u = dct[0] as i32 + dct[2] as i32;
    let del_u = dct[0] as i32 - dct[2] as i32;
    let sum_d = dct[1] as i32 + dct[3] as i32;
    let del_d = dct[1] as i32 - dct[3] as i32;

    let m = mf as i32;
    dct[0] = (((sum_u + sum_d) * m) >> 1) as i16;
    dct[1] = (((sum_u - sum_d) * m) >> 1) as i16;
    dct[2] = (((del_u + del_d) * m) >> 1) as i16;
    dct[3] = (((del_u - del_d) * m) >> 1) as i16;
}




/// `decode_mb_aux.cpp:223`. Luma IDCT of an I16x16 macroblock when only the DC
/// coefficients are non-zero.
///
/// # Safety
/// * `pRec` points at sample `(0, 0)` of a 16x16 block; bytes
///   `[0, 15*iStride + 16)` from it must be readable and writable.
/// * `pPred` points at sample `(0, 0)` of a 16x16 block; bytes
///   `[0, 15*iPredStride + 16)` from it must be readable. Only read.
/// * Both reach forward only; strides `>= 16` and positive; the two spans
///   are disjoint (the callers hand a recon-plane cursor and a prediction
///   scratch, `svc_encode_mb.rs:640-651`).
/// * `pDctDc` points at 16 readable, `i16`-aligned `i16`, disjoint from both.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsIDctRecI16x16Dc_c(
    pRec: *mut u8,
    iStride: i32,
    pPred: *mut u8,
    iPredStride: i32,
    pDctDc: *mut i16,
) {
    // SHIM(phase2) -> idct_rec_i16x16_dc
    let (rs, ps) = (iStride as usize, iPredStride as usize);
    let rec = unsafe { std::slice::from_raw_parts_mut(pRec, 15 * rs + 16) };
    let pred = unsafe { std::slice::from_raw_parts(pPred, 15 * ps + 16) };
    let dc: &[i16; 16] = unsafe { std::slice::from_raw_parts(pDctDc, 16) }.try_into().unwrap();
    idct_rec_i16x16_dc(
        &mut PlaneCursorMut::new(rec, 0, rs),
        &PlaneCursor::new(pred, 0, ps),
        dc,
    );
}

/// `decode_mb_aux.cpp:209`. Applies `pfIDctFourT4` to the four 8x8 quadrants of a
/// macroblock.
///
/// # Safety
/// `pDst`/`pPred` must address 16 rows at their strides; `pDct` 256 readable `i16`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsIDctT4RecOnMb(
    pDst: *mut u8,
    iDstStride: i32,
    pPred: *mut u8,
    iPredStride: i32,
    pDct: *mut i16,
    pfIDctFourT4: PIDctFunc,
) {
    let iDstStridex8 = (iDstStride << 3) as isize;
    let iPredStridex8 = (iPredStride << 3) as isize;

    pfIDctFourT4(pDst, iDstStride, pPred, iPredStride, pDct);
    pfIDctFourT4(pDst.add(8), iDstStride, pPred.add(8), iPredStride, pDct.add(64));
    pfIDctFourT4(
        pDst.offset(iDstStridex8),
        iDstStride,
        pPred.offset(iPredStridex8),
        iPredStride,
        pDct.add(128),
    );
    pfIDctFourT4(
        pDst.offset(iDstStridex8).add(8),
        iDstStride,
        pPred.offset(iPredStridex8).add(8),
        iPredStride,
        pDct.add(192),
    );
}

/// `decode_mb_aux.cpp:251`. Installs the scalar dequantisation and IDCT tables.
///
/// # Safety
/// `pFuncList` must be a valid, writable `SWelsFuncPtrList`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitReconstructionFuncs(pFuncList: &mut SWelsFuncPtrList, _uiCpuFlag: u32) {
    let fl = &mut *pFuncList;

    fl.pfDequantization4x4 = Some(dequant_4x4);
    fl.pfDequantizationFour4x4 = Some(dequant_four_4x4);
    fl.pfDequantizationIHadamard4x4 = Some(dequant_ihadamard_4x4);

    fl.pfIDctT4 = Some(WelsIDctT4Rec_c);
    fl.pfIDctFourT4 = Some(WelsIDctFourT4Rec_c);
    fl.pfIDctI16x16Dc = Some(WelsIDctRecI16x16Dc_c);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dequantisation is an elementwise multiply by the eight-entry MF row, applied to
    /// both halves of the 4x4 block.
    #[test]
    fn dequant4x4_multiplies_by_the_mf_row() {
        let mut res: [i16; 16] = core::array::from_fn(|i| (i as i16) + 1);
        let mf: [u16; 8] = [2, 3, 4, 5, 6, 7, 8, 9];
        dequant_4x4(&mut res, &mf);
        for i in 0..8 {
            assert_eq!(res[i], (i as i16 + 1) * mf[i] as i16, "low half {i}");
            assert_eq!(res[i + 8], (i as i16 + 9) * mf[i] as i16, "high half {i}");
        }
    }

    /// The four-block variant applies the same row to all eight 8-coefficient groups.
    #[test]
    fn dequant_four4x4_covers_all_64_coefficients() {
        let mut res = [1i16; 64];
        let mf: [u16; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        dequant_four_4x4(&mut res, &mf);
        for k in 0..8usize {
            for i in 0..8usize {
                assert_eq!(res[i + (k << 3)], mf[i] as i16, "group {k} lane {i}");
            }
        }
    }

    /// A DC-only inverse Hadamard with MF 1 spreads `pRes[0]` evenly over all 16
    /// positions (gain 16 across the two passes).
    #[test]
    fn dequant_ihadamard4x4_spreads_dc() {
        let mut res = [0i16; 16];
        res[0] = 5;
        dequant_ihadamard_4x4(&mut res, 1);
        assert!(res.iter().all(|&v| v == 5), "{res:?}");
    }

    /// `WelsIDctRecI16x16Dc_c` adds a rounded DC per 4x4 block: block (i>>2, j>>2)
    /// takes `pDctDc[(i & 0x0C) + (j >> 2)]`.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn idct_reci16x16dc_adds_per_block_dc() {
        let stride = 32usize;
        let mut rec = vec![0u8; stride * 20];
        let pred = vec![100u8; stride * 20];
        // 64 in the DC slot -> (64 + 32) >> 6 == 1
        let mut dc = [0i16; 16];
        dc[5] = 64;

        unsafe {
            WelsIDctRecI16x16Dc_c(
                rec.as_mut_ptr(),
                stride as i32,
                pred.as_ptr() as *mut u8,
                stride as i32,
                dc.as_mut_ptr(),
            );
        }
        for i in 0..16usize {
            for j in 0..16usize {
                let idx = ((i as i32 & 0x0C) + (j as i32 >> 2)) as usize;
                let expected = if idx == 5 { 101 } else { 100 };
                assert_eq!(rec[i * stride + j], expected, "({i},{j})");
            }
        }
    }

    /// Every slot the reconstruction path dereferences must be filled.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn init_fills_every_reconstruction_slot() {
        // Zeroing this table is sound for the reason its own `Default` gives
        // (`wels_func_ptr_def.rs`, S21); session I converts both with the dispatch
        // tables. T6.H12 enumerated it here rather than leaving it to a grep.
        let mut fl = SWelsFuncPtrList::default();
        unsafe { WelsInitReconstructionFuncs(&mut fl, 0) };
        assert!(fl.pfDequantization4x4.is_some());
        assert!(fl.pfDequantizationFour4x4.is_some());
        assert!(fl.pfDequantizationIHadamard4x4.is_some());
        assert!(fl.pfIDctT4.is_some());
        assert!(fl.pfIDctFourT4.is_some());
        assert!(fl.pfIDctI16x16Dc.is_some());
    }
}
