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

/// `decode_mb_aux.cpp:98`. Inverse Hadamard on the luma DC block, then scale by the
/// dequantisation multiplier. For qp >= 12.
///
/// # Safety
/// `pRes` must point to 16 writable `i16`.
pub unsafe extern "C" fn WelsDequantIHadamard4x4_c(pRes: *mut i16, kuiMF: u16) {
    let mut iTemp = [0i16; 4];

    let mut i = 0usize;
    while i < 16 {
        iTemp[0] = (*pRes.add(i)).wrapping_add(*pRes.add(i + 2));
        iTemp[1] = (*pRes.add(i)).wrapping_sub(*pRes.add(i + 2));
        iTemp[2] = (*pRes.add(i + 1)).wrapping_sub(*pRes.add(i + 3));
        iTemp[3] = (*pRes.add(i + 1)).wrapping_add(*pRes.add(i + 3));

        *pRes.add(i) = iTemp[0].wrapping_add(iTemp[3]);
        *pRes.add(i + 1) = iTemp[1].wrapping_add(iTemp[2]);
        *pRes.add(i + 2) = iTemp[1].wrapping_sub(iTemp[2]);
        *pRes.add(i + 3) = iTemp[0].wrapping_sub(iTemp[3]);
        i += 4;
    }

    for i in 0..4usize {
        iTemp[0] = (*pRes.add(i)).wrapping_add(*pRes.add(i + 8));
        iTemp[1] = (*pRes.add(i)).wrapping_sub(*pRes.add(i + 8));
        iTemp[2] = (*pRes.add(i + 4)).wrapping_sub(*pRes.add(i + 12));
        iTemp[3] = (*pRes.add(i + 4)).wrapping_add(*pRes.add(i + 12));

        *pRes.add(i) = iTemp[0].wrapping_add(iTemp[3]).wrapping_mul(kuiMF as i16);
        *pRes.add(i + 4) = iTemp[1].wrapping_add(iTemp[2]).wrapping_mul(kuiMF as i16);
        *pRes.add(i + 8) = iTemp[1].wrapping_sub(iTemp[2]).wrapping_mul(kuiMF as i16);
        *pRes.add(i + 12) = iTemp[0].wrapping_sub(iTemp[3]).wrapping_mul(kuiMF as i16);
    }
}

/// `decode_mb_aux.cpp:139`. Scales one 4x4 coefficient block in place.
///
/// # Safety
/// `pRes` must point to 16 writable `i16`; `kpMF` to 8 readable `u16`.
pub unsafe extern "C" fn WelsDequant4x4_c(pRes: *mut i16, kpMF: *const u16) {
    for i in 0..8usize {
        *pRes.add(i) = (*pRes.add(i)).wrapping_mul(*kpMF.add(i) as i16);
        *pRes.add(i + 8) = (*pRes.add(i + 8)).wrapping_mul(*kpMF.add(i) as i16);
    }
}

/// `decode_mb_aux.cpp:147`. Scales four 4x4 coefficient blocks in place.
///
/// # Safety
/// `pRes` must point to 64 writable `i16`; `kpMF` to 8 readable `u16`.
pub unsafe extern "C" fn WelsDequantFour4x4_c(pRes: *mut i16, kpMF: *const u16) {
    for i in 0..8usize {
        let mf = *kpMF.add(i) as i16;
        for k in 0..8usize {
            let idx = i + (k << 3);
            *pRes.add(idx) = (*pRes.add(idx)).wrapping_mul(mf);
        }
    }
}

/// `decode_mb_aux.cpp:223`. Luma IDCT of an I16x16 macroblock when only the DC
/// coefficients are non-zero.
///
/// # Safety
/// `pRec` and `pPred` must address 16 rows at their strides; `pDctDc` 16 readable
/// `i16`.
pub unsafe extern "C" fn WelsIDctRecI16x16Dc_c(
    pRec: *mut u8,
    iStride: i32,
    pPred: *mut u8,
    iPredStride: i32,
    pDctDc: *mut i16,
) {
    let mut pRec = pRec;
    let mut pPred = pPred;

    for i in 0..16i32 {
        for j in 0..16i32 {
            let dc = *pDctDc.offset(((i & 0x0C) + (j >> 2)) as isize) as i32;
            *pRec.offset(j as isize) = WelsClip1(*pPred.offset(j as isize) as i32 + ((dc + 32) >> 6));
        }
        pRec = pRec.offset(iStride as isize);
        pPred = pPred.offset(iPredStride as isize);
    }
}

/// `decode_mb_aux.cpp:209`. Applies `pfIDctFourT4` to the four 8x8 quadrants of a
/// macroblock.
///
/// # Safety
/// `pDst`/`pPred` must address 16 rows at their strides; `pDct` 256 readable `i16`.
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
pub unsafe fn WelsInitReconstructionFuncs(pFuncList: *mut SWelsFuncPtrList, _uiCpuFlag: u32) {
    let fl = &mut *pFuncList;

    fl.pfDequantization4x4 = Some(WelsDequant4x4_c);
    fl.pfDequantizationFour4x4 = Some(WelsDequantFour4x4_c);
    fl.pfDequantizationIHadamard4x4 = Some(WelsDequantIHadamard4x4_c);

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
        unsafe { WelsDequant4x4_c(res.as_mut_ptr(), mf.as_ptr()) };
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
        unsafe { WelsDequantFour4x4_c(res.as_mut_ptr(), mf.as_ptr()) };
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
        unsafe { WelsDequantIHadamard4x4_c(res.as_mut_ptr(), 1) };
        assert!(res.iter().all(|&v| v == 5), "{res:?}");
    }

    /// `WelsIDctRecI16x16Dc_c` adds a rounded DC per 4x4 block: block (i>>2, j>>2)
    /// takes `pDctDc[(i & 0x0C) + (j >> 2)]`.
    #[test]
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
    fn init_fills_every_reconstruction_slot() {
        let mut fl: SWelsFuncPtrList = unsafe { core::mem::zeroed() };
        unsafe { WelsInitReconstructionFuncs(&mut fl, 0) };
        assert!(fl.pfDequantization4x4.is_some());
        assert!(fl.pfDequantizationFour4x4.is_some());
        assert!(fl.pfDequantizationIHadamard4x4.is_some());
        assert!(fl.pfIDctT4.is_some());
        assert!(fl.pfIDctFourT4.is_some());
        assert!(fl.pfIDctI16x16Dc.is_some());
    }
}
