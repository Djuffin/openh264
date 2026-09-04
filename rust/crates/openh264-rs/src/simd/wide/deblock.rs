//! Deblocking on `wide` lane types — the twin of `simd::x86_64::deblock`: the
//! luma bS<4 and bS==4 filters and the chroma pair, for both edge directions.
//!
//! The arithmetic is word-lane throughout and maps one-to-one: `max`/`min`,
//! `simd_lt`/`simd_gt`, shifts, and `select` for the
//! intrinsic file's `and`/`andnot`/`or` triples — the same three ops on an SSE2
//! baseline, `pblendvb` where the build has SSE4.1. The early-outs use `none()`,
//! which is `pmovmskb` on the mask.
//!
//! The edge-direction wrappers at the bottom are the intrinsic file's, unchanged:
//! they were already safe code, and the preconditions they state apply here too.

#![forbid(unsafe_code)]

use wide::i16x8;

use super::lanes::{load8, low8, narrow, widen_lo};
use crate::safe::plane::PlaneSamples;

// ============================================================================
// Lane helpers
// ============================================================================

#[inline(always)]
fn abs_diff16(a: i16x8, b: i16x8) -> i16x8 {
    (a - b).max(b - a)
}

/// Eight taps of one line group, as words.
#[inline(always)]
fn taps(p: &[u8; 16], off: usize) -> i16x8 {
    widen_lo(load8(&p[off..]))
}

/// Eight results back, saturated to bytes.
#[inline(always)]
fn put(p: &mut [u8; 16], off: usize, v: i16x8) {
    p[off..off + 8].copy_from_slice(&low8(narrow(v, i16x8::ZERO)));
}

// ============================================================================
// Core 16-line filters
// ============================================================================

/// The luma bS<4 filter across 16 lines held as contiguous rows.
pub fn deblock_luma_lt4_16(
    p2: &mut [u8; 16],
    p1: &mut [u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &mut [u8; 16],
    q2: &mut [u8; 16],
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    let zero = i16x8::ZERO;
    let one = i16x8::splat(1);
    let four = i16x8::splat(4);
    let alpha_vec = i16x8::splat(alpha as i16);
    let beta_vec = i16x8::splat(beta as i16);
    let max_u8 = i16x8::splat(255);
    let low_byte = i16x8::splat(0x00FF);
    let minus_one = i16x8::splat(-1);

    for half in 0..2 {
        let (ta, tb) = (tc[2 * half] as i16, tc[2 * half + 1] as i16);
        let tc0_vec = i16x8::new([ta, ta, ta, ta, tb, tb, tb, tb]);
        let mask_tc0_ge_0 = tc0_vec.simd_gt(minus_one);

        let offset = half * 8;
        let p0_16 = taps(p0, offset);
        let p1_16 = taps(p1, offset);
        let p2_16 = taps(p2, offset);
        let q0_16 = taps(q0, offset);
        let q1_16 = taps(q1, offset);
        let q2_16 = taps(q2, offset);

        let cond_p0q0 = abs_diff16(p0_16, q0_16).simd_lt(alpha_vec);
        let cond_p1p0 = abs_diff16(p1_16, p0_16).simd_lt(beta_vec);
        let cond_q1q0 = abs_diff16(q1_16, q0_16).simd_lt(beta_vec);

        let mask_filter = mask_tc0_ge_0 & cond_p0q0 & cond_p1p0 & cond_q1q0;
        if mask_filter.none() {
            continue;
        }

        let cond_p2p0 = mask_filter & abs_diff16(p2_16, p0_16).simd_lt(beta_vec);
        let cond_q2q0 = mask_filter & abs_diff16(q2_16, q0_16).simd_lt(beta_vec);

        let avg_p0q0 = (p0_16 + q0_16 + one) >> 1i32;
        let neg_tc0 = zero - tc0_vec;

        let t_p1 = ((p2_16 + avg_p0q0) - (p1_16 << 1i32)) >> 1i32;
        let clip_p1 = t_p1.max(neg_tc0).min(tc0_vec);
        let new_p1_val = (p1_16 + clip_p1) & low_byte;
        let p1_out = cond_p2p0.select(new_p1_val, p1_16);

        let t_q1 = ((q2_16 + avg_p0q0) - (q1_16 << 1i32)) >> 1i32;
        let clip_q1 = t_q1.max(neg_tc0).min(tc0_vec);
        let new_q1_val = (q1_16 + clip_q1) & low_byte;
        let q1_out = cond_q2q0.select(new_q1_val, q1_16);

        let tc_i = tc0_vec - cond_p2p0 - cond_q2q0;
        let neg_tc_i = zero - tc_i;

        let diff_q0p0_x4 = (q0_16 - p0_16) << 2i32;
        let diff_p1q1 = p1_16 - q1_16;
        let t_deta = (diff_q0p0_x4 + diff_p1q1 + four) >> 3i32;
        let deta = t_deta.max(neg_tc_i).min(tc_i);

        let p0_cand = (p0_16 + deta).min(max_u8).max(zero);
        let p0_out = mask_filter.select(p0_cand, p0_16);

        let q0_cand = (q0_16 - deta).min(max_u8).max(zero);
        let q0_out = mask_filter.select(q0_cand, q0_16);

        put(p0, offset, p0_out);
        put(q0, offset, q0_out);
        put(p1, offset, p1_out);
        put(q1, offset, q1_out);
    }
}

/// The luma bS==4 filter across 16 lines held as contiguous rows.
pub fn deblock_luma_eq4_16(
    p3: &[u8; 16],
    p2: &mut [u8; 16],
    p1: &mut [u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &mut [u8; 16],
    q2: &mut [u8; 16],
    q3: &[u8; 16],
    alpha: i32,
    beta: i32,
) {
    let two = i16x8::splat(2);
    let four = i16x8::splat(4);
    let alpha_vec = i16x8::splat(alpha as i16);
    let beta_vec = i16x8::splat(beta as i16);
    let small_thresh = i16x8::splat(((alpha >> 2i32) + 2) as i16);

    for half in 0..2 {
        let offset = half * 8;
        let p3_16 = taps(p3, offset);
        let p2_16 = taps(p2, offset);
        let p1_16 = taps(p1, offset);
        let p0_16 = taps(p0, offset);
        let q0_16 = taps(q0, offset);
        let q1_16 = taps(q1, offset);
        let q2_16 = taps(q2, offset);
        let q3_16 = taps(q3, offset);

        let diff_p0q0 = abs_diff16(p0_16, q0_16);
        let cond_p0q0 = diff_p0q0.simd_lt(alpha_vec);
        let cond_p1p0 = abs_diff16(p1_16, p0_16).simd_lt(beta_vec);
        let cond_q1q0 = abs_diff16(q1_16, q0_16).simd_lt(beta_vec);

        let mask_filter = cond_p0q0 & cond_p1p0 & cond_q1q0;
        if mask_filter.none() {
            continue;
        }

        let cond_small = mask_filter & diff_p0q0.simd_lt(small_thresh);
        let cond_p2p0 = cond_small & abs_diff16(p2_16, p0_16).simd_lt(beta_vec);
        let cond_q2q0 = cond_small & abs_diff16(q2_16, q0_16).simd_lt(beta_vec);

        let p0_default = ((p1_16 << 1i32) + p0_16 + q1_16 + two) >> 2i32;
        let q0_default = ((q1_16 << 1i32) + q0_16 + p1_16 + two) >> 2i32;

        let p0_p2p0 = (p2_16 + ((p1_16 + p0_16 + q0_16) << 1i32) + q1_16 + four) >> 3i32;
        let p1_p2p0 = (p2_16 + p1_16 + p0_16 + q0_16 + two) >> 2i32;
        let p2_p2p0 = ((p3_16 << 1i32) + (p2_16 << 1i32) + p2_16 + p1_16 + p0_16 + q0_16 + four) >> 3i32;

        let q0_q2q0 = (p1_16 + ((p0_16 + q0_16 + q1_16) << 1i32) + q2_16 + four) >> 3i32;
        let q1_q2q0 = (p0_16 + q0_16 + q1_16 + q2_16 + two) >> 2i32;
        let q2_q2q0 = ((q3_16 << 1i32) + (q2_16 << 1i32) + q2_16 + q1_16 + q0_16 + p0_16 + four) >> 3i32;

        let p0_out = mask_filter.select(cond_p2p0.select(p0_p2p0, p0_default), p0_16);
        let p1_out = cond_p2p0.select(p1_p2p0, p1_16);
        let p2_out = cond_p2p0.select(p2_p2p0, p2_16);

        let q0_out = mask_filter.select(cond_q2q0.select(q0_q2q0, q0_default), q0_16);
        let q1_out = cond_q2q0.select(q1_q2q0, q1_16);
        let q2_out = cond_q2q0.select(q2_q2q0, q2_16);

        put(p0, offset, p0_out);
        put(p1, offset, p1_out);
        put(p2, offset, p2_out);
        put(q0, offset, q0_out);
        put(q1, offset, q1_out);
        put(q2, offset, q2_out);
    }
}

/// The chroma bS<4 filter across 16 lines (eight of Cb, eight of Cr).
pub fn deblock_chroma_lt4_16(
    p1: &[u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &[u8; 16],
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    let zero = i16x8::ZERO;
    let four = i16x8::splat(4);
    let alpha_vec = i16x8::splat(alpha as i16);
    let beta_vec = i16x8::splat(beta as i16);
    let max_u8 = i16x8::splat(255);

    let t = |i: usize| tc[i] as i16;
    let tc0_vec = i16x8::new([t(0), t(0), t(1), t(1), t(2), t(2), t(3), t(3)]);
    let mask_tc0_gt_0 = tc0_vec.simd_gt(zero);

    for half in 0..2 {
        if mask_tc0_gt_0.none() {
            continue;
        }
        let offset = half * 8;
        let p1_16 = taps(p1, offset);
        let p0_16 = taps(p0, offset);
        let q0_16 = taps(q0, offset);
        let q1_16 = taps(q1, offset);

        let cond_p0q0 = abs_diff16(p0_16, q0_16).simd_lt(alpha_vec);
        let cond_p1p0 = abs_diff16(p1_16, p0_16).simd_lt(beta_vec);
        let cond_q1q0 = abs_diff16(q1_16, q0_16).simd_lt(beta_vec);

        let mask_filter = mask_tc0_gt_0 & cond_p0q0 & cond_p1p0 & cond_q1q0;
        if mask_filter.none() {
            continue;
        }

        let diff_q0p0_x4 = (q0_16 - p0_16) << 2i32;
        let diff_p1q1 = p1_16 - q1_16;
        let t_deta = (diff_q0p0_x4 + diff_p1q1 + four) >> 3i32;
        let neg_tc0 = zero - tc0_vec;
        let deta = t_deta.max(neg_tc0).min(tc0_vec);

        let p0_cand = (p0_16 + deta).min(max_u8).max(zero);
        let q0_cand = (q0_16 - deta).min(max_u8).max(zero);

        put(p0, offset, mask_filter.select(p0_cand, p0_16));
        put(q0, offset, mask_filter.select(q0_cand, q0_16));
    }
}

/// The chroma bS==4 filter across 16 lines (eight of Cb, eight of Cr).
pub fn deblock_chroma_eq4_16(
    p1: &[u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &[u8; 16],
    alpha: i32,
    beta: i32,
) {
    let two = i16x8::splat(2);
    let alpha_vec = i16x8::splat(alpha as i16);
    let beta_vec = i16x8::splat(beta as i16);

    for half in 0..2 {
        let offset = half * 8;
        let p1_16 = taps(p1, offset);
        let p0_16 = taps(p0, offset);
        let q0_16 = taps(q0, offset);
        let q1_16 = taps(q1, offset);

        let cond_p0q0 = abs_diff16(p0_16, q0_16).simd_lt(alpha_vec);
        let cond_p1p0 = abs_diff16(p1_16, p0_16).simd_lt(beta_vec);
        let cond_q1q0 = abs_diff16(q1_16, q0_16).simd_lt(beta_vec);

        let mask_filter = cond_p0q0 & cond_p1p0 & cond_q1q0;
        if mask_filter.none() {
            continue;
        }

        let p0_cand = ((p1_16 << 1i32) + p0_16 + q1_16 + two) >> 2i32;
        let q0_cand = ((q1_16 << 1i32) + q0_16 + p1_16 + two) >> 2i32;

        put(p0, offset, mask_filter.select(p0_cand, p0_16));
        put(q0, offset, mask_filter.select(q0_cand, q0_16));
    }
}

// ============================================================================
// Edge-direction dispatch — the intrinsic file's, unchanged
// ============================================================================

/// The luma bS<4 filter over an edge. Preconditions as the intrinsic twin's: the
/// cross-line step must be the cursor's own stride, which the direction guard alone
/// does not establish; the `debug_assert!`s are what keep that true.
pub fn deblock_luma_lt4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    if step_y == 1 {
        debug_assert_eq!(step_x, pix.stride() as isize);
        let mut p2 = pix.row_n::<16>(-3, 0);
        let mut p1 = pix.row_n::<16>(-2, 0);
        let mut p0 = pix.row_n::<16>(-1, 0);
        let mut q0 = pix.row_n::<16>(0, 0);
        let mut q1 = pix.row_n::<16>(1, 0);
        let mut q2 = pix.row_n::<16>(2, 0);

        deblock_luma_lt4_16(&mut p2, &mut p1, &mut p0, &mut q0, &mut q1, &mut q2, alpha, beta, tc);

        pix.set_row_n::<16>(-2, 0, &p1);
        pix.set_row_n::<16>(-1, 0, &p0);
        pix.set_row_n::<16>(0, 0, &q0);
        pix.set_row_n::<16>(1, 0, &q1);
    } else if step_x == 1 {
        debug_assert_eq!(step_y, pix.stride() as isize);
        let mut rows = [[0u8; 8]; 16];
        for i in 0..16 {
            rows[i] = pix.row_n::<8>(i as isize, -4);
        }

        let mut t = [[0u8; 16]; 8];
        for y in 0..16 {
            for x in 0..8 {
                t[x][y] = rows[y][x];
            }
        }

        let [_, ref mut t1, ref mut t2, ref mut t3, ref mut t4, ref mut t5, ref mut t6, _] = t;
        deblock_luma_lt4_16(t1, t2, t3, t4, t5, t6, alpha, beta, tc);

        for x in 2..=5 {
            for y in 0..16 {
                rows[y][x] = t[x][y];
            }
        }

        // Write back only the columns the filter can modify; at `iEdge == 0` the
        // outer columns belong to the previous macroblock.
        for i in 0..16 {
            let seg: &[u8; 4] = rows[i][2..6].try_into().expect("p1..q1");
            pix.set_row_n::<4>(i as isize, -2, seg);
        }
    } else {
        crate::common::deblocking_common::deblock_luma_lt4_scalar(pix, step_x, step_y, alpha, beta, tc);
    }
}

/// The luma bS==4 filter over an edge. Preconditions as [`deblock_luma_lt4`].
pub fn deblock_luma_eq4(pix: &mut impl PlaneSamples, step_x: isize, step_y: isize, alpha: i32, beta: i32) {
    if step_y == 1 {
        debug_assert_eq!(step_x, pix.stride() as isize);
        let p3 = pix.row_n::<16>(-4, 0);
        let mut p2 = pix.row_n::<16>(-3, 0);
        let mut p1 = pix.row_n::<16>(-2, 0);
        let mut p0 = pix.row_n::<16>(-1, 0);
        let mut q0 = pix.row_n::<16>(0, 0);
        let mut q1 = pix.row_n::<16>(1, 0);
        let mut q2 = pix.row_n::<16>(2, 0);
        let q3 = pix.row_n::<16>(3, 0);

        deblock_luma_eq4_16(&p3, &mut p2, &mut p1, &mut p0, &mut q0, &mut q1, &mut q2, &q3, alpha, beta);

        pix.set_row_n::<16>(-3, 0, &p2);
        pix.set_row_n::<16>(-2, 0, &p1);
        pix.set_row_n::<16>(-1, 0, &p0);
        pix.set_row_n::<16>(0, 0, &q0);
        pix.set_row_n::<16>(1, 0, &q1);
        pix.set_row_n::<16>(2, 0, &q2);
    } else if step_x == 1 {
        debug_assert_eq!(step_y, pix.stride() as isize);
        let mut rows = [[0u8; 8]; 16];
        for i in 0..16 {
            rows[i] = pix.row_n::<8>(i as isize, -4);
        }

        let mut t = [[0u8; 16]; 8];
        for y in 0..16 {
            for x in 0..8 {
                t[x][y] = rows[y][x];
            }
        }

        let [ref t0, ref mut t1, ref mut t2, ref mut t3, ref mut t4, ref mut t5, ref mut t6, ref t7] = t;
        deblock_luma_eq4_16(t0, t1, t2, t3, t4, t5, t6, t7, alpha, beta);

        for x in 1..=6 {
            for y in 0..16 {
                rows[y][x] = t[x][y];
            }
        }

        for i in 0..16 {
            let seg: &[u8; 6] = rows[i][1..7].try_into().expect("p2..q2");
            pix.set_row_n::<6>(i as isize, -3, seg);
        }
    } else {
        crate::common::deblocking_common::deblock_luma_eq4_scalar(pix, step_x, step_y, alpha, beta);
    }
}

/// The chroma bS<4 filter over an edge of both chroma planes. Preconditions as
/// [`deblock_luma_lt4`], checked on both planes.
pub fn deblock_chroma_lt4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    if step_y == 1 {
        debug_assert_eq!(step_x, cb.stride() as isize);
        debug_assert_eq!(step_x, cr.stride() as isize);
        let cb_p1 = cb.row_n::<8>(-2, 0);
        let cr_p1 = cr.row_n::<8>(-2, 0);
        let mut cb_p0 = cb.row_n::<8>(-1, 0);
        let mut cr_p0 = cr.row_n::<8>(-1, 0);
        let mut cb_q0 = cb.row_n::<8>(0, 0);
        let mut cr_q0 = cr.row_n::<8>(0, 0);
        let cb_q1 = cb.row_n::<8>(1, 0);
        let cr_q1 = cr.row_n::<8>(1, 0);

        let mut p1 = [0u8; 16];
        let mut p0 = [0u8; 16];
        let mut q0 = [0u8; 16];
        let mut q1 = [0u8; 16];

        p1[..8].copy_from_slice(&cb_p1);
        p1[8..].copy_from_slice(&cr_p1);
        p0[..8].copy_from_slice(&cb_p0);
        p0[8..].copy_from_slice(&cr_p0);
        q0[..8].copy_from_slice(&cb_q0);
        q0[8..].copy_from_slice(&cr_q0);
        q1[..8].copy_from_slice(&cb_q1);
        q1[8..].copy_from_slice(&cr_q1);

        deblock_chroma_lt4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta, tc);

        cb_p0.copy_from_slice(&p0[..8]);
        cr_p0.copy_from_slice(&p0[8..]);
        cb_q0.copy_from_slice(&q0[..8]);
        cr_q0.copy_from_slice(&q0[8..]);

        cb.set_row_n::<8>(-1, 0, &cb_p0);
        cr.set_row_n::<8>(-1, 0, &cr_p0);
        cb.set_row_n::<8>(0, 0, &cb_q0);
        cr.set_row_n::<8>(0, 0, &cr_q0);
    } else if step_x == 1 {
        debug_assert_eq!(step_y, cb.stride() as isize);
        debug_assert_eq!(step_y, cr.stride() as isize);
        let mut cb_rows = [[0u8; 4]; 8];
        let mut cr_rows = [[0u8; 4]; 8];
        for i in 0..8 {
            cb_rows[i] = cb.row_n::<4>(i as isize, -2);
            cr_rows[i] = cr.row_n::<4>(i as isize, -2);
        }

        let mut t = [[0u8; 16]; 4];
        for y in 0..8 {
            for x in 0..4 {
                t[x][y] = cb_rows[y][x];
                t[x][y + 8] = cr_rows[y][x];
            }
        }

        let [ref t0, ref mut t1, ref mut t2, ref t3] = t;
        deblock_chroma_lt4_16(t0, t1, t2, t3, alpha, beta, tc);

        for y in 0..8 {
            cb_rows[y][1] = t[1][y];
            cb_rows[y][2] = t[2][y];
            cr_rows[y][1] = t[1][y + 8];
            cr_rows[y][2] = t[2][y + 8];
        }

        for i in 0..8 {
            let cb_seg: &[u8; 2] = cb_rows[i][1..3].try_into().expect("p0, q0");
            let cr_seg: &[u8; 2] = cr_rows[i][1..3].try_into().expect("p0, q0");
            cb.set_row_n::<2>(i as isize, -1, cb_seg);
            cr.set_row_n::<2>(i as isize, -1, cr_seg);
        }
    } else {
        crate::common::deblocking_common::deblock_chroma_lt4_scalar(cb, cr, step_x, step_y, alpha, beta, tc);
    }
}

/// The chroma bS==4 filter over an edge of both chroma planes. Preconditions as
/// [`deblock_chroma_lt4`].
pub fn deblock_chroma_eq4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    if step_y == 1 {
        debug_assert_eq!(step_x, cb.stride() as isize);
        debug_assert_eq!(step_x, cr.stride() as isize);
        let cb_p1 = cb.row_n::<8>(-2, 0);
        let cr_p1 = cr.row_n::<8>(-2, 0);
        let mut cb_p0 = cb.row_n::<8>(-1, 0);
        let mut cr_p0 = cr.row_n::<8>(-1, 0);
        let mut cb_q0 = cb.row_n::<8>(0, 0);
        let mut cr_q0 = cr.row_n::<8>(0, 0);
        let cb_q1 = cb.row_n::<8>(1, 0);
        let cr_q1 = cr.row_n::<8>(1, 0);

        let mut p1 = [0u8; 16];
        let mut p0 = [0u8; 16];
        let mut q0 = [0u8; 16];
        let mut q1 = [0u8; 16];

        p1[..8].copy_from_slice(&cb_p1);
        p1[8..].copy_from_slice(&cr_p1);
        p0[..8].copy_from_slice(&cb_p0);
        p0[8..].copy_from_slice(&cr_p0);
        q0[..8].copy_from_slice(&cb_q0);
        q0[8..].copy_from_slice(&cr_q0);
        q1[..8].copy_from_slice(&cb_q1);
        q1[8..].copy_from_slice(&cr_q1);

        deblock_chroma_eq4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta);

        cb_p0.copy_from_slice(&p0[..8]);
        cr_p0.copy_from_slice(&p0[8..]);
        cb_q0.copy_from_slice(&q0[..8]);
        cr_q0.copy_from_slice(&q0[8..]);

        cb.set_row_n::<8>(-1, 0, &cb_p0);
        cr.set_row_n::<8>(-1, 0, &cr_p0);
        cb.set_row_n::<8>(0, 0, &cb_q0);
        cr.set_row_n::<8>(0, 0, &cr_q0);
    } else if step_x == 1 {
        debug_assert_eq!(step_y, cb.stride() as isize);
        debug_assert_eq!(step_y, cr.stride() as isize);
        let mut cb_rows = [[0u8; 4]; 8];
        let mut cr_rows = [[0u8; 4]; 8];
        for i in 0..8 {
            cb_rows[i] = cb.row_n::<4>(i as isize, -2);
            cr_rows[i] = cr.row_n::<4>(i as isize, -2);
        }

        let mut t = [[0u8; 16]; 4];
        for y in 0..8 {
            for x in 0..4 {
                t[x][y] = cb_rows[y][x];
                t[x][y + 8] = cr_rows[y][x];
            }
        }

        let [ref t0, ref mut t1, ref mut t2, ref t3] = t;
        deblock_chroma_eq4_16(t0, t1, t2, t3, alpha, beta);

        for y in 0..8 {
            cb_rows[y][1] = t[1][y];
            cb_rows[y][2] = t[2][y];
            cr_rows[y][1] = t[1][y + 8];
            cr_rows[y][2] = t[2][y + 8];
        }

        for i in 0..8 {
            let cb_seg: &[u8; 2] = cb_rows[i][1..3].try_into().expect("p0, q0");
            let cr_seg: &[u8; 2] = cr_rows[i][1..3].try_into().expect("p0, q0");
            cb.set_row_n::<2>(i as isize, -1, cb_seg);
            cr.set_row_n::<2>(i as isize, -1, cr_seg);
        }
    } else {
        crate::common::deblocking_common::deblock_chroma_eq4_scalar(cb, cr, step_x, step_y, alpha, beta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::plane::PaddedPlane;

    fn make_test_plane(w: usize, h: usize, pad: usize, stride: usize) -> PaddedPlane {
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                p.set(x, y, (((x * 17) ^ (y * 31) ^ 0x5a) & 0xff) as u8);
            }
        }
        p
    }

    #[test]
    fn test_deblock_luma_lt4_parity() {
        let stride = 64;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz { (stride as isize, 1) } else { (1, stride as isize) };
            let mut plane_scalar = make_test_plane(32, 32, 16, stride);
            let mut plane_simd = plane_scalar.clone();

            let alpha = 20;
            let beta = 12;
            let tc = [2i8, 3, 1, 4];

            crate::common::deblocking_common::deblock_luma_lt4_scalar(
                &mut plane_scalar.cursor_mut(8, 8),
                step_x,
                step_y,
                alpha,
                beta,
                &tc,
            );

            deblock_luma_lt4(
                &mut plane_simd.cursor_mut(8, 8),
                step_x,
                step_y,
                alpha,
                beta,
                &tc,
            );

            for y in -16..32isize {
                for x in -16..32isize {
                    assert_eq!(
                        plane_scalar.cursor_mut(8, 8).at(x, y),
                        plane_simd.cursor_mut(8, 8).at(x, y),
                        "mismatch at ({x}, {y}) horiz={is_horiz}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_deblock_luma_eq4_parity() {
        let stride = 64;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz { (stride as isize, 1) } else { (1, stride as isize) };
            let mut plane_scalar = make_test_plane(32, 32, 16, stride);
            let mut plane_simd = plane_scalar.clone();

            let alpha = 24;
            let beta = 15;

            crate::common::deblocking_common::deblock_luma_eq4_scalar(
                &mut plane_scalar.cursor_mut(8, 8),
                step_x,
                step_y,
                alpha,
                beta,
            );

            deblock_luma_eq4(
                &mut plane_simd.cursor_mut(8, 8),
                step_x,
                step_y,
                alpha,
                beta,
            );

            for y in -16..32isize {
                for x in -16..32isize {
                    assert_eq!(
                        plane_scalar.cursor_mut(8, 8).at(x, y),
                        plane_simd.cursor_mut(8, 8).at(x, y),
                        "mismatch at ({x}, {y}) horiz={is_horiz}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_deblock_chroma_lt4_parity() {
        let stride = 32;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz { (stride as isize, 1) } else { (1, stride as isize) };
            let mut cb_scalar = make_test_plane(16, 16, 8, stride);
            let mut cr_scalar = make_test_plane(16, 16, 8, stride);
            let mut cb_simd = cb_scalar.clone();
            let mut cr_simd = cr_scalar.clone();

            let alpha = 18;
            let beta = 10;
            let tc = [1i8, 2, 0, 3];

            crate::common::deblocking_common::deblock_chroma_lt4_scalar(
                &mut cb_scalar.cursor_mut(4, 4),
                &mut cr_scalar.cursor_mut(4, 4),
                step_x,
                step_y,
                alpha,
                beta,
                &tc,
            );

            deblock_chroma_lt4(
                &mut cb_simd.cursor_mut(4, 4),
                &mut cr_simd.cursor_mut(4, 4),
                step_x,
                step_y,
                alpha,
                beta,
                &tc,
            );

            for y in -8..16isize {
                for x in -8..16isize {
                    assert_eq!(
                        cb_scalar.cursor_mut(4, 4).at(x, y),
                        cb_simd.cursor_mut(4, 4).at(x, y),
                        "cb mismatch at ({x}, {y}) horiz={is_horiz}"
                    );
                    assert_eq!(
                        cr_scalar.cursor_mut(4, 4).at(x, y),
                        cr_simd.cursor_mut(4, 4).at(x, y),
                        "cr mismatch at ({x}, {y}) horiz={is_horiz}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_deblock_chroma_eq4_parity() {
        let stride = 32;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz { (stride as isize, 1) } else { (1, stride as isize) };
            let mut cb_scalar = make_test_plane(16, 16, 8, stride);
            let mut cr_scalar = make_test_plane(16, 16, 8, stride);
            let mut cb_simd = cb_scalar.clone();
            let mut cr_simd = cr_scalar.clone();

            let alpha = 22;
            let beta = 14;

            crate::common::deblocking_common::deblock_chroma_eq4_scalar(
                &mut cb_scalar.cursor_mut(4, 4),
                &mut cr_scalar.cursor_mut(4, 4),
                step_x,
                step_y,
                alpha,
                beta,
            );

            deblock_chroma_eq4(
                &mut cb_simd.cursor_mut(4, 4),
                &mut cr_simd.cursor_mut(4, 4),
                step_x,
                step_y,
                alpha,
                beta,
            );

            for y in -8..16isize {
                for x in -8..16isize {
                    assert_eq!(
                        cb_scalar.cursor_mut(4, 4).at(x, y),
                        cb_simd.cursor_mut(4, 4).at(x, y),
                        "cb mismatch at ({x}, {y}) horiz={is_horiz}"
                    );
                    assert_eq!(
                        cr_scalar.cursor_mut(4, 4).at(x, y),
                        cr_simd.cursor_mut(4, 4).at(x, y),
                        "cr mismatch at ({x}, {y}) horiz={is_horiz}"
                    );
                }
            }
        }
    }
}
