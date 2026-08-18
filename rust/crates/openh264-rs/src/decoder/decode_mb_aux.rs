//! Inverse Discrete Cosine Transform (IDCT) and Macroblock Reconstruction Auxiliary Functions.
//!
//! Rust translation of:
//! - `codec/decoder/core/inc/decode_mb_aux.h`
//! - `codec/decoder/core/src/decode_mb_aux.cpp`
#![deny(unsafe_code)]
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

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
pub use crate::decoder::decode_slice::{g_kuiScan8};

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

// **T5.X8: the four `SHIM(phase2)` entry points that stood here are deleted**, with
// the two dispatch typedefs that described them. Each rebuilt a slice from a raw
// pointer and a stride and called the kernel above it; the dispatch tables hold the
// kernels themselves now (`decoder_context.rs`'s `PIdctResAddPredFunc` and friends
// take a `PlaneCursorMut`), and the reconstruction bracket builds the cursor from
// the picture's own plane.
//
// `GetI4LumaIChromaAddrTable` and `i4_luma_ichroma_addr_table` went with them, and
// so did `SWelsDecoderContext::iDecBlockOffsetArray`: the table held **byte** offsets
// of the 16 luma and 8 chroma 4x4 blocks, which is why it had to be recomputed
// whenever a picture's stride changed. A block's position inside its macroblock is a
// pair of sample coordinates and no stride enters it — `decode_slice.rs`'s `blk4_xy`
// is that pair, computed from `g_kuiScan8` exactly as the table's own body did.

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
        let rs = [0i16; 16];
        idct_res_add_pred(&mut PlaneCursorMut::new(&mut pred, 0, 8), &rs);
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
        idct_res_add_pred(&mut PlaneCursorMut::new(&mut pred, 0, 8), &rs);
        for row in 0..4 {
            for col in 0..4 {
                assert_eq!(pred[row * 8 + col], 129);
            }
        }
    }

    /// **T5.X8**: the byte-offset table this file used to build is gone, and
    /// `decode_slice.rs`'s `blk4_xy` replaces it. The values it produced are pinned
    /// here in the units they moved to — a 32-byte luma stride made block 1 offset
    /// 4 (`x = 4`) and block 2 offset 128 (`y = 4`), which is what these coordinates
    /// say without a stride in them.
    #[test]
    fn blk4_xy_is_the_deleted_offset_table_with_the_stride_factored_out() {
        use crate::decoder::decode_slice::blk4_xy;
        assert_eq!(blk4_xy(0), (0, 0));
        assert_eq!(blk4_xy(1), (4, 0));
        assert_eq!(blk4_xy(2), (0, 4));
        assert_eq!(blk4_xy(3), (4, 4));
        // And the whole 4x4 grid of 4x4 blocks is covered exactly once.
        let mut seen: Vec<(isize, isize)> = (0..16).map(blk4_xy).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 16);
        assert!(seen.iter().all(|&(x, y)| (0..16).contains(&x) && (0..16).contains(&y)));
    }
}
