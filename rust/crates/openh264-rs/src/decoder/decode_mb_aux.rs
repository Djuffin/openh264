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

// ---------------------------------------------------------------------------
// Safe kernels (plan §Phase 2, recipe R2). These are the implementations; the
// `*_c` functions below are strangler shims (R7) that build views from the raw
// pointers and call in here, so no call site and no dispatch-table installer
// changes in this phase.
//
// Every kernel in this file writes a fixed-size block and reaches *forward* only,
// from the block's own (0, 0) — no `-1` column, no `-stride` row. That is what
// makes the shims' contracts short: the reachable span is a function of the
// stride and the block size alone, so a shim needs no knowledge of the plane's
// padding to build a slice that exactly covers what the kernel touches.
// ---------------------------------------------------------------------------

use crate::safe::plane::PlaneCursorMut;

/// 4x4 inverse integer DCT of `rs`, added to the prediction block at `pred` and
/// saturated to `[0, 255]` in place.
///
/// C++: `IdctResAddPred_c`, `codec/decoder/core/src/decode_mb_aux.cpp`.
///
/// `rs` is read, never written — the JSVM compliance note on the C++ original.
/// The two 1-D passes are the C++'s, unchanged, **including the `as i16`
/// truncation of the horizontal pass's output**: `iSrc` is an `int16_t[16]` there
/// and the sums can exceed `i16`, so the truncation is observable and load-bearing.
/// What did change is the write loop, which the C++ walks column-major
/// (`for i in 0..4` over columns, four strided stores each). Every one of the 16
/// samples is read and written exactly once, so transposing the loop to row-major
/// is bit-exact, and it lets each row be one bounds check and a fixed-size window
/// instead of four (plan §7.4).
pub fn idct_res_add_pred(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16]) {
    let mut src = [0i16; 16];

    for i in 0..4 {
        let y = i << 2;
        let r0 = rs[y] as i32;
        let r1 = rs[y + 1] as i32;
        let r2 = rs[y + 2] as i32;
        let r3 = rs[y + 3] as i32;

        let t0 = r0 + r2;
        let t1 = r0 - r2;
        let t2 = (r1 >> 1) - r3;
        let t3 = r1 + (r3 >> 1);

        src[y] = (t0 + t3) as i16;
        src[y + 1] = (t1 + t2) as i16;
        src[y + 2] = (t1 - t2) as i16;
        src[y + 3] = (t0 - t3) as i16;
    }

    let mut res = [[0i32; 4]; 4];
    for i in 0..4 {
        let s0 = src[i] as i32;
        let s4 = src[i + 4] as i32;
        let s8 = src[i + 8] as i32;
        let s12 = src[i + 12] as i32;

        let t1 = s0 + s8;
        let t2 = s4 + (s12 >> 1);
        res[0][i] = (32 + t1 + t2) >> 6;
        res[3][i] = (32 + t1 - t2) >> 6;

        let t1 = s0 - s8;
        let t2 = (s4 >> 1) - s12;
        res[1][i] = (32 + t1 + t2) >> 6;
        res[2][i] = (32 + t1 - t2) >> 6;
    }

    for (dy, r) in res.iter().enumerate() {
        let row: &mut [u8; 4] = pred.row_mut(dy as isize, 0, 4).try_into().unwrap();
        for (p, &v) in row.iter_mut().zip(r.iter()) {
            *p = WelsClip1(v + *p as i32);
        }
    }
}

/// 8x8 inverse integer DCT (High Profile / FRExt) of `rs`, added to the prediction
/// block at `pred` and saturated to `[0, 255]` in place.
///
/// C++: `IdctResAddPred8x8_c`, `codec/decoder/core/src/decode_mb_aux.cpp`.
///
/// Both 1-D passes were already array-local in the port; only the final add loop
/// touched the plane, and it was already row-major, so this is the C++ line for
/// line with a `row_mut` window in place of the strided pointer.
pub fn idct_res_add_pred8x8(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 64]) {
    let mut p = [0i16; 8];
    let mut b = [0i16; 8];
    let mut a = [0i16; 4];

    let mut tmp = [0i16; 64];
    let mut res = [0i16; 64];

    // Horizontal 1D IDCT pass.
    for i in 0..8 {
        p.copy_from_slice(&rs[i << 3..][..8]);

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

        tmp[i << 3] = b[0] + b[7];
        tmp[1 + (i << 3)] = b[2] - b[5];
        tmp[2 + (i << 3)] = b[4] + b[3];
        tmp[3 + (i << 3)] = b[6] + b[1];
        tmp[4 + (i << 3)] = b[6] - b[1];
        tmp[5 + (i << 3)] = b[4] - b[3];
        tmp[6 + (i << 3)] = b[2] + b[5];
        tmp[7 + (i << 3)] = b[0] - b[7];
    }

    // Vertical 1D IDCT pass.
    for i in 0..8 {
        for j in 0..8 {
            p[j] = tmp[i + (j << 3)];
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

        res[i] = b[0] + b[7];
        res[(1 << 3) + i] = b[2] - b[5];
        res[(2 << 3) + i] = b[4] + b[3];
        res[(3 << 3) + i] = b[6] + b[1];
        res[(4 << 3) + i] = b[6] - b[1];
        res[(5 << 3) + i] = b[4] - b[3];
        res[(6 << 3) + i] = b[2] + b[5];
        res[(7 << 3) + i] = b[0] - b[7];
    }

    for i in 0..8 {
        let row: &mut [u8; 8] = pred.row_mut(i as isize, 0, 8).try_into().unwrap();
        for (j, dst) in row.iter_mut().enumerate() {
            *dst = WelsClip1(((32 + res[(i << 3) + j] as i32) >> 6) + *dst as i32);
        }
    }
}

/// The four 4x4 sub-blocks of one 8x8 quadrant, each IDCT-added if it has any
/// coefficient worth transforming.
///
/// C++: `IdctFourResAddPred_c`, `codec/decoder/core/src/decode_mb_aux.cpp`.
///
/// `nzc` is a window onto the macroblock's 8-wide non-zero-count raster, anchored
/// at this quadrant's top-left 4x4 block; the four sub-blocks are therefore at
/// `nzc[0]`, `nzc[1]`, `nzc[4]` and `nzc[5]`, which is why the parameter is a
/// `[i8; 6]` rather than a `[i8; 4]` — six is the exact reach, and stating it as a
/// fixed-size array is what stops a caller passing a window that ends at index 3.
/// A block also needs the transform when only its DC coefficient is non-zero
/// (the I16x16 luma DC case), hence the `|| rs[k << 4] != 0`.
pub fn idct_four_res_add_pred(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 64], nzc: &[i8; 6]) {
    const SUBS: [(isize, isize, usize); 4] = [(0, 0, 0), (4, 0, 1), (0, 4, 4), (4, 4, 5)];

    for (k, &(dx, dy, n)) in SUBS.iter().enumerate() {
        if nzc[n] != 0 || rs[k << 4] != 0 {
            let block: &[i16; 16] = rs[k << 4..][..16].try_into().unwrap();
            idct_res_add_pred(&mut pred.reborrow(dx, dy), block);
        }
    }
}

/// Precomputes the 24-element byte-offset table from a macroblock's top-left
/// corner to each of its 4x4 sub-blocks: 16 luma, then the 4 chroma offsets stored
/// twice (Cb at 16..20, Cr at 20..24, identical because the two planes share a
/// geometry).
///
/// C++: `GetI4LumaIChromaAddrTable`, `codec/decoder/core/src/decode_mb_aux.cpp`.
///
/// The destination is `[i32; 24]` rather than a pointer because the only caller
/// owns exactly that (`SWelsDecoderContext::iDecBlockOffsetArray`,
/// `decoder_context.rs:676`) — the size relationship stops being something the two
/// sides have to agree about by hand. That is finding F1's defect class, pre-empted.
pub fn i4_luma_ichroma_addr_table(block_offset: &mut [i32; 24], stride_y: i32, stride_uv: i32) {
    let scan0 = g_kuiScan8[0] as u32;

    for i in 0..16 {
        let a = g_kuiScan8[i] as u32 - scan0;
        let x = (a & 0x07) as i32;
        let y = (a >> 3) as i32;
        block_offset[i] = (x + stride_y * y) << 2;
    }

    for i in 0..4 {
        let a = g_kuiScan8[i] as u32 - scan0;
        let x = (a & 0x07) as i32;
        let y = (a >> 3) as i32;
        let offset = (x + stride_uv * y) << 2;
        block_offset[16 + i] = offset;
        block_offset[20 + i] = offset;
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
        // A block also needs the IDCT when only its DC coefficient (pRs[k*16])
        // is non-zero (I16x16 luma DC), matching `IdctFourResAddPred_c` in
        // `decode_mb_aux.cpp`.
        if *pNzc.add(0) != 0 || *pRs.add(0) != 0 {
            IdctResAddPred_c(pPred.offset(0), iStride, pRs.add(0));
        }
        if *pNzc.add(1) != 0 || *pRs.add(16) != 0 {
            IdctResAddPred_c(pPred.offset(4), iStride, pRs.add(16));
        }
        if *pNzc.add(4) != 0 || *pRs.add(32) != 0 {
            IdctResAddPred_c(pPred.offset((4 * iStride) as isize), iStride, pRs.add(32));
        }
        if *pNzc.add(5) != 0 || *pRs.add(48) != 0 {
            IdctResAddPred_c(pPred.offset((4 * iStride + 4) as isize), iStride, pRs.add(48));
        }
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
