//! Inverse Discrete Cosine Transform (IDCT) and Macroblock Reconstruction Auxiliary Functions.
//!
//! Rust translation of:
//! - `codec/decoder/core/inc/decode_mb_aux.h`
//! - `codec/decoder/core/src/decode_mb_aux.cpp`
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

/// Global 4x4 sub-block raster scan lookup table (24 elements: 16 Luma + 4 Cb + 4 Cr).
pub const g_kuiScan8: [u8; 24] = [
    9, 10, 17, 18, // Luma 4x4 Block 0..3
    11, 12, 19, 20, // Luma 4x4 Block 4..7
    25, 26, 33, 34, // Luma 4x4 Block 8..11
    27, 28, 35, 36, // Luma 4x4 Block 12..15
    14, 15,         // Chroma Cb Block 0..1 / Cr Block 0..1
    22, 23,         // Chroma Cb Block 2..3 / Cr Block 2..3
    38, 39,
    46, 47,
];

/// Pixel clipping / saturation helper function clamping values to [0, 255].
#[inline(always)]
pub fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        (((-iX) >> 31) & 255) as u8
    } else {
        iX as u8
    }
}

/// Function pointer type for 4x4 IDCT and residual prediction addition.
pub type PIdctResAddPredFunc = unsafe extern "C" fn(pPred: *mut u8, kiStride: i32, pRs: *mut i16);

/// Function pointer type for batch 4-block IDCT and residual prediction addition.
pub type PIdctFourResAddPredFunc =
    unsafe extern "C" fn(pPred: *mut u8, iStride: i32, pRs: *mut i16, pNzc: *const i8);

/// Performs 2D 4x4 Inverse Integer Discrete Cosine Transform on scaled coefficients `pRs`,
/// sums the resulting spatial residuals with the prediction samples `pPred`,
/// saturates the pixel values to [0, 255], and writes them back to `pPred` in-place.
///
/// NOTE: `pRs` is NOT modified during transform calculation to maintain JSVM compliance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn IdctResAddPred_c(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        let mut iSrc = [0i16; 16];

        let kiStride2 = kiStride << 1;
        let kiStride3 = kiStride + kiStride2;

        for i in 0..4 {
            let kiY = i << 2;
            let r0 = *pRs.add(kiY) as i32;
            let r1 = *pRs.add(kiY + 1) as i32;
            let r2 = *pRs.add(kiY + 2) as i32;
            let r3 = *pRs.add(kiY + 3) as i32;

            let kiT0 = r0 + r2;
            let kiT1 = r0 - r2;
            let kiT2 = (r1 >> 1) - r3;
            let kiT3 = r1 + (r3 >> 1);

            iSrc[kiY] = (kiT0 + kiT3) as i16;
            iSrc[kiY + 1] = (kiT1 + kiT2) as i16;
            iSrc[kiY + 2] = (kiT1 - kiT2) as i16;
            iSrc[kiY + 3] = (kiT0 - kiT3) as i16;
        }

        for i in 0..4 {
            let s_i = iSrc[i] as i32;
            let s_i4 = iSrc[i + 4] as i32;
            let s_i8 = iSrc[i + 8] as i32;
            let s_i12 = iSrc[i + 12] as i32;

            let mut kT1 = s_i + s_i8;
            let mut kT2 = s_i4 + (s_i12 >> 1);
            let kT3 = (32 + kT1 + kT2) >> 6;
            let kT4 = (32 + kT1 - kT2) >> 6;

            let p0 = pPred.offset(i as isize);
            let p3 = pPred.offset((i as i32 + kiStride3) as isize);

            *p0 = WelsClip1(kT3 + *p0 as i32);
            *p3 = WelsClip1(kT4 + *p3 as i32);

            kT1 = s_i - s_i8;
            kT2 = (s_i4 >> 1) - s_i12;

            let p1 = pPred.offset((i as i32 + kiStride) as isize);
            let p2 = pPred.offset((i as i32 + kiStride2) as isize);

            *p1 = WelsClip1(((32 + kT1 + kT2) >> 6) + *p1 as i32);
            *p2 = WelsClip1(((32 + kT1 - kT2) >> 6) + *p2 as i32);
        }
    }
}

/// Performs 2D 8x8 Inverse Integer Discrete Cosine Transform on scaled coefficients `pRs`
/// for H.264 High Profile (FRExt) transform blocks, adds the residuals to `pPred`,
/// and saturates pixel values to [0, 255].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn IdctResAddPred8x8_c(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        let mut p = [0i16; 8];
        let mut b = [0i16; 8];
        let mut a = [0i16; 4];

        let mut iTmp = [0i16; 64];
        let mut iRes = [0i16; 64];

        // Horizontal 1D IDCT Pass
        for i in 0..8 {
            for j in 0..8 {
                p[j] = *pRs.add(j + (i << 3));
            }

            a[0] = p[0] + p[4];
            a[1] = p[0] - p[4];
            a[2] = p[6] - (p[2] >> 1);
            a[3] = p[2] + (p[6] >> 1);

            b[0] = a[0] + a[3];
            b[2] = a[1] - a[2];
            b[4] = a[1] + a[2];
            b[6] = a[0] - a[3];

            a[0] = -p[3] + p[5] - p[7] - (p[7] >> 1);
            a[1] = p[1] + p[7] - p[3] - (p[3] >> 1);
            a[2] = -p[1] + p[7] + p[5] + (p[5] >> 1);
            a[3] = p[3] + p[5] + p[1] + (p[1] >> 1);

            b[1] = a[0] + (a[3] >> 2);
            b[3] = a[1] + (a[2] >> 2);
            b[5] = a[2] - (a[1] >> 2);
            b[7] = a[3] - (a[0] >> 2);

            iTmp[0 + (i << 3)] = b[0] + b[7];
            iTmp[1 + (i << 3)] = b[2] - b[5];
            iTmp[2 + (i << 3)] = b[4] + b[3];
            iTmp[3 + (i << 3)] = b[6] + b[1];
            iTmp[4 + (i << 3)] = b[6] - b[1];
            iTmp[5 + (i << 3)] = b[4] - b[3];
            iTmp[6 + (i << 3)] = b[2] + b[5];
            iTmp[7 + (i << 3)] = b[0] - b[7];
        }

        // Vertical 1D IDCT Pass
        for i in 0..8 {
            for j in 0..8 {
                p[j] = iTmp[i + (j << 3)];
            }

            a[0] = p[0] + p[4];
            a[1] = p[0] - p[4];
            a[2] = p[6] - (p[2] >> 1);
            a[3] = p[2] + (p[6] >> 1);

            b[0] = a[0] + a[3];
            b[2] = a[1] - a[2];
            b[4] = a[1] + a[2];
            b[6] = a[0] - a[3];

            a[0] = -p[3] + p[5] - p[7] - (p[7] >> 1);
            a[1] = p[1] + p[7] - p[3] - (p[3] >> 1);
            a[2] = -p[1] + p[7] + p[5] + (p[5] >> 1);
            a[3] = p[3] + p[5] + p[1] + (p[1] >> 1);

            b[1] = a[0] + (a[3] >> 2);
            b[7] = a[3] - (a[0] >> 2);
            b[3] = a[1] + (a[2] >> 2);
            b[5] = a[2] - (a[1] >> 2);

            iRes[(0 << 3) + i] = b[0] + b[7];
            iRes[(1 << 3) + i] = b[2] - b[5];
            iRes[(2 << 3) + i] = b[4] + b[3];
            iRes[(3 << 3) + i] = b[6] + b[1];
            iRes[(4 << 3) + i] = b[6] - b[1];
            iRes[(5 << 3) + i] = b[4] - b[3];
            iRes[(6 << 3) + i] = b[2] + b[5];
            iRes[(7 << 3) + i] = b[0] - b[7];
        }

        for i in 0..8 {
            for j in 0..8 {
                let idx = (i as i32 * kiStride + j as i32) as isize;
                let dst_ptr = pPred.offset(idx);
                *dst_ptr = WelsClip1(((32 + iRes[(i << 3) + j] as i32) >> 6) + *dst_ptr as i32);
            }
        }
    }
}

/// Precomputes the 24-element byte offset lookup table `pBlockOffset`
/// from the top-left corner of a macroblock to each 4x4 sub-block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetI4LumaIChromaAddrTable(
    pBlockOffset: *mut i32,
    kiYStride: i32,
    kiUVStride: i32,
) {
    unsafe {
        let kuiScan0 = g_kuiScan8[0] as u32;

        for i in 0..16 {
            let kuiA = g_kuiScan8[i] as u32 - kuiScan0;
            let kuiX = kuiA & 0x07;
            let kuiY = kuiA >> 3;

            *pBlockOffset.add(i) = (kuiX as i32 + kiYStride * (kuiY as i32)) << 2;
        }

        for i in 0..4 {
            let kuiA = g_kuiScan8[i] as u32 - kuiScan0;
            let kuiX = kuiA & 0x07;
            let kuiY = kuiA >> 3;

            let offset = (kuiX as i32 + kiUVStride * (kuiY as i32)) << 2;
            *pBlockOffset.add(16 + i) = offset;
            *pBlockOffset.add(20 + i) = offset;
        }
    }
}

/// Batch 4-block IDCT and residual prediction addition using Non-Zero Count cache `pNzc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn IdctFourResAddPred_c(
    pPred: *mut u8,
    iStride: i32,
    pRs: *mut i16,
    pNzc: *const i8,
) {
    unsafe {
        if *pNzc.add(0) != 0 {
            IdctResAddPred_c(pPred.offset(0), iStride, pRs.add(0));
        }
        if *pNzc.add(1) != 0 {
            IdctResAddPred_c(pPred.offset(4), iStride, pRs.add(16));
        }
        if *pNzc.add(4) != 0 {
            IdctResAddPred_c(pPred.offset((4 * iStride) as isize), iStride, pRs.add(32));
        }
        if *pNzc.add(5) != 0 {
            IdctResAddPred_c(pPred.offset((4 * iStride + 4) as isize), iStride, pRs.add(48));
        }
    }
}

// Architecture SIMD fallback aliases
pub unsafe extern "C" fn IdctResAddPred_mmx(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        IdctResAddPred_c(pPred, kiStride, pRs);
    }
}

pub unsafe extern "C" fn IdctResAddPred_sse2(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        IdctResAddPred_c(pPred, kiStride, pRs);
    }
}

pub unsafe extern "C" fn IdctResAddPred_avx2(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        IdctResAddPred_c(pPred, kiStride, pRs);
    }
}

pub unsafe extern "C" fn IdctFourResAddPred_avx2(
    pPred: *mut u8,
    iStride: i32,
    pRs: *mut i16,
    pNzc: *const i8,
) {
    unsafe {
        IdctFourResAddPred_c(pPred, iStride, pRs, pNzc);
    }
}

pub unsafe extern "C" fn IdctResAddPred_neon(pred: *mut u8, stride: i32, rs: *mut i16) {
    unsafe {
        IdctResAddPred_c(pred, stride, rs);
    }
}

pub unsafe extern "C" fn IdctResAddPred_AArch64_neon(pred: *mut u8, stride: i32, rs: *mut i16) {
    unsafe {
        IdctResAddPred_c(pred, stride, rs);
    }
}

pub unsafe extern "C" fn IdctResAddPred_mmi(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        IdctResAddPred_c(pPred, kiStride, pRs);
    }
}

pub unsafe extern "C" fn IdctResAddPred_lsx(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        IdctResAddPred_c(pPred, kiStride, pRs);
    }
}

pub unsafe extern "C" fn IdctResAddPred8x8_lsx(pPred: *mut u8, kiStride: i32, pRs: *mut i16) {
    unsafe {
        IdctResAddPred8x8_c(pPred, kiStride, pRs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wels_clip1() {
        assert_eq!(WelsClip1(-10), 0);
        assert_eq!(WelsClip1(0), 0);
        assert_eq!(WelsClip1(128), 128);
        assert_eq!(WelsClip1(255), 255);
        assert_eq!(WelsClip1(300), 255);
    }

    #[test]
    fn test_idct_res_add_pred_c_zero_residual() {
        let mut pred = [128u8; 64];
        let mut rs = [0i16; 16];
        unsafe {
            IdctResAddPred_c(pred.as_mut_ptr(), 8, rs.as_mut_ptr());
        }
        for row in 0..4 {
            for col in 0..4 {
                assert_eq!(pred[row * 8 + col], 128);
            }
        }
    }

    #[test]
    fn test_idct_res_add_pred_c_dc_residual() {
        let mut pred = [128u8; 64];
        let mut rs = [0i16; 16];
        rs[0] = 64; // DC coeff = 64 -> (32 + 64) >> 6 = 1
        unsafe {
            IdctResAddPred_c(pred.as_mut_ptr(), 8, rs.as_mut_ptr());
        }
        for row in 0..4 {
            for col in 0..4 {
                assert_eq!(pred[row * 8 + col], 129);
            }
        }
    }

    #[test]
    fn test_get_i4_luma_i_chroma_addr_table() {
        let mut block_offset = [0i32; 24];
        unsafe {
            GetI4LumaIChromaAddrTable(block_offset.as_mut_ptr(), 32, 16);
        }
        assert_eq!(block_offset[0], 0);
        assert_eq!(block_offset[1], 4);
        assert_eq!(block_offset[2], 128);
        assert_eq!(block_offset[3], 132);
        assert_eq!(block_offset[16], 0);
        assert_eq!(block_offset[20], 0);
        assert_eq!(block_offset[17], 4);
        assert_eq!(block_offset[21], 4);
    }
}
