//! x86_64 SSE2 Deblocking Filter Kernels.
//!
//! Accelerated implementations for Luma (Lt4 / Eq4) and Chroma (Lt4 / Eq4)
//! boundary filters for both horizontal and vertical edges.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use crate::common::deblocking_common::{
    deblock_chroma_eq4_scalar, deblock_chroma_lt4_scalar, deblock_luma_eq4_scalar,
    deblock_luma_lt4_scalar,
};
use crate::encoder::encoder_context::SMVUnitXY;
use crate::safe::plane::{BlockRows, PlaneSamples};

// ============================================================================
// Core SSE2 Vectorized Edge Filters
// ============================================================================

/// Vectorized 16-line Luma bS < 4 (Lt4) filter across contiguous sample rows.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn deblock_luma_lt4_16(
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
    let zero = _mm_setzero_si128();
    let one = _mm_set1_epi16(1);
    let four = _mm_set1_epi16(4);
    let alpha_vec = _mm_set1_epi16(alpha as i16);
    let beta_vec = _mm_set1_epi16(beta as i16);
    let max_u8 = _mm_set1_epi16(255);

    for half in 0..2 {
        let tc0_vec = if half == 0 {
            _mm_setr_epi16(
                tc[0] as i16,
                tc[0] as i16,
                tc[0] as i16,
                tc[0] as i16,
                tc[1] as i16,
                tc[1] as i16,
                tc[1] as i16,
                tc[1] as i16,
            )
        } else {
            _mm_setr_epi16(
                tc[2] as i16,
                tc[2] as i16,
                tc[2] as i16,
                tc[2] as i16,
                tc[3] as i16,
                tc[3] as i16,
                tc[3] as i16,
                tc[3] as i16,
            )
        };

        let mask_tc0_ge_0 = _mm_cmpgt_epi16(tc0_vec, _mm_set1_epi16(-1));

        let offset = half * 8;
        let p0_8 = _mm_loadl_epi64(p0.as_ptr().add(offset) as *const __m128i);
        let p1_8 = _mm_loadl_epi64(p1.as_ptr().add(offset) as *const __m128i);
        let p2_8 = _mm_loadl_epi64(p2.as_ptr().add(offset) as *const __m128i);
        let q0_8 = _mm_loadl_epi64(q0.as_ptr().add(offset) as *const __m128i);
        let q1_8 = _mm_loadl_epi64(q1.as_ptr().add(offset) as *const __m128i);
        let q2_8 = _mm_loadl_epi64(q2.as_ptr().add(offset) as *const __m128i);

        let p0_16 = _mm_unpacklo_epi8(p0_8, zero);
        let p1_16 = _mm_unpacklo_epi8(p1_8, zero);
        let p2_16 = _mm_unpacklo_epi8(p2_8, zero);
        let q0_16 = _mm_unpacklo_epi8(q0_8, zero);
        let q1_16 = _mm_unpacklo_epi8(q1_8, zero);
        let q2_16 = _mm_unpacklo_epi8(q2_8, zero);

        let diff_p0q0 = _mm_max_epi16(_mm_sub_epi16(p0_16, q0_16), _mm_sub_epi16(q0_16, p0_16));
        let cond_p0q0 = _mm_cmplt_epi16(diff_p0q0, alpha_vec);

        let diff_p1p0 = _mm_max_epi16(_mm_sub_epi16(p1_16, p0_16), _mm_sub_epi16(p0_16, p1_16));
        let cond_p1p0 = _mm_cmplt_epi16(diff_p1p0, beta_vec);

        let diff_q1q0 = _mm_max_epi16(_mm_sub_epi16(q1_16, q0_16), _mm_sub_epi16(q0_16, q1_16));
        let cond_q1q0 = _mm_cmplt_epi16(diff_q1q0, beta_vec);

        let mask_filter = _mm_and_si128(
            mask_tc0_ge_0,
            _mm_and_si128(cond_p0q0, _mm_and_si128(cond_p1p0, cond_q1q0)),
        );

        if _mm_movemask_epi8(mask_filter) == 0 {
            continue;
        }

        let diff_p2p0 = _mm_max_epi16(_mm_sub_epi16(p2_16, p0_16), _mm_sub_epi16(p0_16, p2_16));
        let cond_p2p0 = _mm_and_si128(mask_filter, _mm_cmplt_epi16(diff_p2p0, beta_vec));

        let diff_q2q0 = _mm_max_epi16(_mm_sub_epi16(q2_16, q0_16), _mm_sub_epi16(q0_16, q2_16));
        let cond_q2q0 = _mm_and_si128(mask_filter, _mm_cmplt_epi16(diff_q2q0, beta_vec));

        let avg_p0q0 = _mm_srai_epi16(_mm_add_epi16(_mm_add_epi16(p0_16, q0_16), one), 1);

        let t_p1 = _mm_srai_epi16(
            _mm_sub_epi16(_mm_add_epi16(p2_16, avg_p0q0), _mm_slli_epi16(p1_16, 1)),
            1,
        );
        let neg_tc0 = _mm_sub_epi16(zero, tc0_vec);
        let clip_p1 = _mm_min_epi16(_mm_max_epi16(t_p1, neg_tc0), tc0_vec);
        let new_p1_val = _mm_and_si128(_mm_add_epi16(p1_16, clip_p1), _mm_set1_epi16(0x00FF));
        let p1_out = _mm_or_si128(
            _mm_and_si128(cond_p2p0, new_p1_val),
            _mm_andnot_si128(cond_p2p0, p1_16),
        );

        let t_q1 = _mm_srai_epi16(
            _mm_sub_epi16(_mm_add_epi16(q2_16, avg_p0q0), _mm_slli_epi16(q1_16, 1)),
            1,
        );
        let clip_q1 = _mm_min_epi16(_mm_max_epi16(t_q1, neg_tc0), tc0_vec);
        let new_q1_val = _mm_and_si128(_mm_add_epi16(q1_16, clip_q1), _mm_set1_epi16(0x00FF));
        let q1_out = _mm_or_si128(
            _mm_and_si128(cond_q2q0, new_q1_val),
            _mm_andnot_si128(cond_q2q0, q1_16),
        );

        let tc_i = _mm_sub_epi16(_mm_sub_epi16(tc0_vec, cond_p2p0), cond_q2q0);
        let neg_tc_i = _mm_sub_epi16(zero, tc_i);

        let diff_q0p0_x4 = _mm_slli_epi16(_mm_sub_epi16(q0_16, p0_16), 2);
        let diff_p1q1 = _mm_sub_epi16(p1_16, q1_16);
        let t_deta = _mm_srai_epi16(
            _mm_add_epi16(_mm_add_epi16(diff_q0p0_x4, diff_p1q1), four),
            3,
        );
        let deta = _mm_min_epi16(_mm_max_epi16(t_deta, neg_tc_i), tc_i);

        let p0_cand = _mm_max_epi16(_mm_min_epi16(_mm_add_epi16(p0_16, deta), max_u8), zero);
        let p0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, p0_cand),
            _mm_andnot_si128(mask_filter, p0_16),
        );

        let q0_cand = _mm_max_epi16(_mm_min_epi16(_mm_sub_epi16(q0_16, deta), max_u8), zero);
        let q0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, q0_cand),
            _mm_andnot_si128(mask_filter, q0_16),
        );

        let p0_val = _mm_cvtsi128_si64(_mm_packus_epi16(p0_out, zero));
        let q0_val = _mm_cvtsi128_si64(_mm_packus_epi16(q0_out, zero));
        let p1_val = _mm_cvtsi128_si64(_mm_packus_epi16(p1_out, zero));
        let q1_val = _mm_cvtsi128_si64(_mm_packus_epi16(q1_out, zero));

        p0[offset..offset + 8].copy_from_slice(&p0_val.to_ne_bytes());
        q0[offset..offset + 8].copy_from_slice(&q0_val.to_ne_bytes());
        p1[offset..offset + 8].copy_from_slice(&p1_val.to_ne_bytes());
        q1[offset..offset + 8].copy_from_slice(&q1_val.to_ne_bytes());
    }
}

/// Vectorized 16-line Luma bS == 4 (Eq4) filter across contiguous sample rows.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn deblock_luma_eq4_16(
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
    let zero = _mm_setzero_si128();
    let two = _mm_set1_epi16(2);
    let four = _mm_set1_epi16(4);
    let alpha_vec = _mm_set1_epi16(alpha as i16);
    let beta_vec = _mm_set1_epi16(beta as i16);
    let small_thresh = _mm_set1_epi16(((alpha >> 2) + 2) as i16);

    for half in 0..2 {
        let offset = half * 8;
        let p3_8 = _mm_loadl_epi64(p3.as_ptr().add(offset) as *const __m128i);
        let p2_8 = _mm_loadl_epi64(p2.as_ptr().add(offset) as *const __m128i);
        let p1_8 = _mm_loadl_epi64(p1.as_ptr().add(offset) as *const __m128i);
        let p0_8 = _mm_loadl_epi64(p0.as_ptr().add(offset) as *const __m128i);
        let q0_8 = _mm_loadl_epi64(q0.as_ptr().add(offset) as *const __m128i);
        let q1_8 = _mm_loadl_epi64(q1.as_ptr().add(offset) as *const __m128i);
        let q2_8 = _mm_loadl_epi64(q2.as_ptr().add(offset) as *const __m128i);
        let q3_8 = _mm_loadl_epi64(q3.as_ptr().add(offset) as *const __m128i);

        let p3_16 = _mm_unpacklo_epi8(p3_8, zero);
        let p2_16 = _mm_unpacklo_epi8(p2_8, zero);
        let p1_16 = _mm_unpacklo_epi8(p1_8, zero);
        let p0_16 = _mm_unpacklo_epi8(p0_8, zero);
        let q0_16 = _mm_unpacklo_epi8(q0_8, zero);
        let q1_16 = _mm_unpacklo_epi8(q1_8, zero);
        let q2_16 = _mm_unpacklo_epi8(q2_8, zero);
        let q3_16 = _mm_unpacklo_epi8(q3_8, zero);

        let diff_p0q0 = _mm_max_epi16(_mm_sub_epi16(p0_16, q0_16), _mm_sub_epi16(q0_16, p0_16));
        let cond_p0q0 = _mm_cmplt_epi16(diff_p0q0, alpha_vec);

        let diff_p1p0 = _mm_max_epi16(_mm_sub_epi16(p1_16, p0_16), _mm_sub_epi16(p0_16, p1_16));
        let cond_p1p0 = _mm_cmplt_epi16(diff_p1p0, beta_vec);

        let diff_q1q0 = _mm_max_epi16(_mm_sub_epi16(q1_16, q0_16), _mm_sub_epi16(q0_16, q1_16));
        let cond_q1q0 = _mm_cmplt_epi16(diff_q1q0, beta_vec);

        let mask_filter = _mm_and_si128(cond_p0q0, _mm_and_si128(cond_p1p0, cond_q1q0));
        if _mm_movemask_epi8(mask_filter) == 0 {
            continue;
        }

        let cond_small = _mm_and_si128(mask_filter, _mm_cmplt_epi16(diff_p0q0, small_thresh));

        let diff_p2p0 = _mm_max_epi16(_mm_sub_epi16(p2_16, p0_16), _mm_sub_epi16(p0_16, p2_16));
        let cond_p2p0 = _mm_and_si128(cond_small, _mm_cmplt_epi16(diff_p2p0, beta_vec));

        let diff_q2q0 = _mm_max_epi16(_mm_sub_epi16(q2_16, q0_16), _mm_sub_epi16(q0_16, q2_16));
        let cond_q2q0 = _mm_and_si128(cond_small, _mm_cmplt_epi16(diff_q2q0, beta_vec));

        let p0_default = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(_mm_slli_epi16(p1_16, 1), _mm_add_epi16(p0_16, q1_16)),
                two,
            ),
            2,
        );
        let q0_default = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(_mm_slli_epi16(q1_16, 1), _mm_add_epi16(q0_16, p1_16)),
                two,
            ),
            2,
        );

        let p0_p2p0 = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(
                    _mm_add_epi16(
                        p2_16,
                        _mm_slli_epi16(_mm_add_epi16(p1_16, _mm_add_epi16(p0_16, q0_16)), 1),
                    ),
                    q1_16,
                ),
                four,
            ),
            3,
        );
        let p1_p2p0 = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(_mm_add_epi16(p2_16, p1_16), _mm_add_epi16(p0_16, q0_16)),
                two,
            ),
            2,
        );
        let p2_p2p0 = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(
                    _mm_add_epi16(
                        _mm_slli_epi16(p3_16, 1),
                        _mm_add_epi16(_mm_slli_epi16(p2_16, 1), p2_16),
                    ),
                    _mm_add_epi16(_mm_add_epi16(p1_16, p0_16), q0_16),
                ),
                four,
            ),
            3,
        );

        let q0_q2q0 = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(
                    _mm_add_epi16(
                        p1_16,
                        _mm_slli_epi16(_mm_add_epi16(p0_16, _mm_add_epi16(q0_16, q1_16)), 1),
                    ),
                    q2_16,
                ),
                four,
            ),
            3,
        );
        let q1_q2q0 = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(_mm_add_epi16(p0_16, q0_16), _mm_add_epi16(q1_16, q2_16)),
                two,
            ),
            2,
        );
        let q2_q2q0 = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(
                    _mm_add_epi16(
                        _mm_slli_epi16(q3_16, 1),
                        _mm_add_epi16(_mm_slli_epi16(q2_16, 1), q2_16),
                    ),
                    _mm_add_epi16(_mm_add_epi16(q1_16, q0_16), p0_16),
                ),
                four,
            ),
            3,
        );

        let p0_cand = _mm_or_si128(
            _mm_and_si128(cond_p2p0, p0_p2p0),
            _mm_andnot_si128(cond_p2p0, p0_default),
        );
        let p0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, p0_cand),
            _mm_andnot_si128(mask_filter, p0_16),
        );

        let p1_out = _mm_or_si128(
            _mm_and_si128(cond_p2p0, p1_p2p0),
            _mm_andnot_si128(cond_p2p0, p1_16),
        );
        let p2_out = _mm_or_si128(
            _mm_and_si128(cond_p2p0, p2_p2p0),
            _mm_andnot_si128(cond_p2p0, p2_16),
        );

        let q0_cand = _mm_or_si128(
            _mm_and_si128(cond_q2q0, q0_q2q0),
            _mm_andnot_si128(cond_q2q0, q0_default),
        );
        let q0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, q0_cand),
            _mm_andnot_si128(mask_filter, q0_16),
        );

        let q1_out = _mm_or_si128(
            _mm_and_si128(cond_q2q0, q1_q2q0),
            _mm_andnot_si128(cond_q2q0, q1_16),
        );
        let q2_out = _mm_or_si128(
            _mm_and_si128(cond_q2q0, q2_q2q0),
            _mm_andnot_si128(cond_q2q0, q2_16),
        );

        let p0_val = _mm_cvtsi128_si64(_mm_packus_epi16(p0_out, zero));
        let p1_val = _mm_cvtsi128_si64(_mm_packus_epi16(p1_out, zero));
        let p2_val = _mm_cvtsi128_si64(_mm_packus_epi16(p2_out, zero));
        let q0_val = _mm_cvtsi128_si64(_mm_packus_epi16(q0_out, zero));
        let q1_val = _mm_cvtsi128_si64(_mm_packus_epi16(q1_out, zero));
        let q2_val = _mm_cvtsi128_si64(_mm_packus_epi16(q2_out, zero));

        p0[offset..offset + 8].copy_from_slice(&p0_val.to_ne_bytes());
        p1[offset..offset + 8].copy_from_slice(&p1_val.to_ne_bytes());
        p2[offset..offset + 8].copy_from_slice(&p2_val.to_ne_bytes());
        q0[offset..offset + 8].copy_from_slice(&q0_val.to_ne_bytes());
        q1[offset..offset + 8].copy_from_slice(&q1_val.to_ne_bytes());
        q2[offset..offset + 8].copy_from_slice(&q2_val.to_ne_bytes());
    }
}

/// Vectorized 16-line Chroma bS < 4 (Lt4) filter across contiguous sample rows.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn deblock_chroma_lt4_16(
    p1: &[u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &[u8; 16],
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    let zero = _mm_setzero_si128();
    let four = _mm_set1_epi16(4);
    let alpha_vec = _mm_set1_epi16(alpha as i16);
    let beta_vec = _mm_set1_epi16(beta as i16);
    let max_u8 = _mm_set1_epi16(255);

    let tc0_vec = _mm_setr_epi16(
        tc[0] as i16,
        tc[0] as i16,
        tc[1] as i16,
        tc[1] as i16,
        tc[2] as i16,
        tc[2] as i16,
        tc[3] as i16,
        tc[3] as i16,
    );
    let mask_tc0_gt_0 = _mm_cmpgt_epi16(tc0_vec, zero);

    for half in 0..2 {
        if _mm_movemask_epi8(mask_tc0_gt_0) == 0 {
            continue;
        }
        let offset = half * 8;
        let p1_8 = _mm_loadl_epi64(p1.as_ptr().add(offset) as *const __m128i);
        let p0_8 = _mm_loadl_epi64(p0.as_ptr().add(offset) as *const __m128i);
        let q0_8 = _mm_loadl_epi64(q0.as_ptr().add(offset) as *const __m128i);
        let q1_8 = _mm_loadl_epi64(q1.as_ptr().add(offset) as *const __m128i);

        let p1_16 = _mm_unpacklo_epi8(p1_8, zero);
        let p0_16 = _mm_unpacklo_epi8(p0_8, zero);
        let q0_16 = _mm_unpacklo_epi8(q0_8, zero);
        let q1_16 = _mm_unpacklo_epi8(q1_8, zero);

        let diff_p0q0 = _mm_max_epi16(_mm_sub_epi16(p0_16, q0_16), _mm_sub_epi16(q0_16, p0_16));
        let cond_p0q0 = _mm_cmplt_epi16(diff_p0q0, alpha_vec);

        let diff_p1p0 = _mm_max_epi16(_mm_sub_epi16(p1_16, p0_16), _mm_sub_epi16(p0_16, p1_16));
        let cond_p1p0 = _mm_cmplt_epi16(diff_p1p0, beta_vec);

        let diff_q1q0 = _mm_max_epi16(_mm_sub_epi16(q1_16, q0_16), _mm_sub_epi16(q0_16, q1_16));
        let cond_q1q0 = _mm_cmplt_epi16(diff_q1q0, beta_vec);

        let mask_filter = _mm_and_si128(
            mask_tc0_gt_0,
            _mm_and_si128(cond_p0q0, _mm_and_si128(cond_p1p0, cond_q1q0)),
        );
        if _mm_movemask_epi8(mask_filter) == 0 {
            continue;
        }

        let diff_q0p0_x4 = _mm_slli_epi16(_mm_sub_epi16(q0_16, p0_16), 2);
        let diff_p1q1 = _mm_sub_epi16(p1_16, q1_16);
        let t_deta = _mm_srai_epi16(
            _mm_add_epi16(_mm_add_epi16(diff_q0p0_x4, diff_p1q1), four),
            3,
        );
        let neg_tc0 = _mm_sub_epi16(zero, tc0_vec);
        let deta = _mm_min_epi16(_mm_max_epi16(t_deta, neg_tc0), tc0_vec);

        let p0_cand = _mm_max_epi16(_mm_min_epi16(_mm_add_epi16(p0_16, deta), max_u8), zero);
        let q0_cand = _mm_max_epi16(_mm_min_epi16(_mm_sub_epi16(q0_16, deta), max_u8), zero);

        let p0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, p0_cand),
            _mm_andnot_si128(mask_filter, p0_16),
        );
        let q0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, q0_cand),
            _mm_andnot_si128(mask_filter, q0_16),
        );

        let p0_val = _mm_cvtsi128_si64(_mm_packus_epi16(p0_out, zero));
        let q0_val = _mm_cvtsi128_si64(_mm_packus_epi16(q0_out, zero));

        p0[offset..offset + 8].copy_from_slice(&p0_val.to_ne_bytes());
        q0[offset..offset + 8].copy_from_slice(&q0_val.to_ne_bytes());
    }
}

/// Vectorized 16-line Chroma bS == 4 (Eq4) filter across contiguous sample rows.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn deblock_chroma_eq4_16(
    p1: &[u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &[u8; 16],
    alpha: i32,
    beta: i32,
) {
    let zero = _mm_setzero_si128();
    let two = _mm_set1_epi16(2);
    let alpha_vec = _mm_set1_epi16(alpha as i16);
    let beta_vec = _mm_set1_epi16(beta as i16);

    for half in 0..2 {
        let offset = half * 8;
        let p1_8 = _mm_loadl_epi64(p1.as_ptr().add(offset) as *const __m128i);
        let p0_8 = _mm_loadl_epi64(p0.as_ptr().add(offset) as *const __m128i);
        let q0_8 = _mm_loadl_epi64(q0.as_ptr().add(offset) as *const __m128i);
        let q1_8 = _mm_loadl_epi64(q1.as_ptr().add(offset) as *const __m128i);

        let p1_16 = _mm_unpacklo_epi8(p1_8, zero);
        let p0_16 = _mm_unpacklo_epi8(p0_8, zero);
        let q0_16 = _mm_unpacklo_epi8(q0_8, zero);
        let q1_16 = _mm_unpacklo_epi8(q1_8, zero);

        let diff_p0q0 = _mm_max_epi16(_mm_sub_epi16(p0_16, q0_16), _mm_sub_epi16(q0_16, p0_16));
        let cond_p0q0 = _mm_cmplt_epi16(diff_p0q0, alpha_vec);

        let diff_p1p0 = _mm_max_epi16(_mm_sub_epi16(p1_16, p0_16), _mm_sub_epi16(p0_16, p1_16));
        let cond_p1p0 = _mm_cmplt_epi16(diff_p1p0, beta_vec);

        let diff_q1q0 = _mm_max_epi16(_mm_sub_epi16(q1_16, q0_16), _mm_sub_epi16(q0_16, q1_16));
        let cond_q1q0 = _mm_cmplt_epi16(diff_q1q0, beta_vec);

        let mask_filter = _mm_and_si128(cond_p0q0, _mm_and_si128(cond_p1p0, cond_q1q0));
        if _mm_movemask_epi8(mask_filter) == 0 {
            continue;
        }

        let p0_cand = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(_mm_slli_epi16(p1_16, 1), _mm_add_epi16(p0_16, q1_16)),
                two,
            ),
            2,
        );
        let q0_cand = _mm_srai_epi16(
            _mm_add_epi16(
                _mm_add_epi16(_mm_slli_epi16(q1_16, 1), _mm_add_epi16(q0_16, p1_16)),
                two,
            ),
            2,
        );

        let p0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, p0_cand),
            _mm_andnot_si128(mask_filter, p0_16),
        );
        let q0_out = _mm_or_si128(
            _mm_and_si128(mask_filter, q0_cand),
            _mm_andnot_si128(mask_filter, q0_16),
        );

        let p0_val = _mm_cvtsi128_si64(_mm_packus_epi16(p0_out, zero));
        let q0_val = _mm_cvtsi128_si64(_mm_packus_epi16(q0_out, zero));

        p0[offset..offset + 8].copy_from_slice(&p0_val.to_ne_bytes());
        q0[offset..offset + 8].copy_from_slice(&q0_val.to_ne_bytes());
    }
}

// ============================================================================
// SIMD Matrix Transpositions
// ============================================================================

/// Transposes two 8x8 blocks of bytes packed in pairs of rows across 8 `__m128i` registers.
///
/// Input: `r0..r7`, where register `ri` holds row `i` (bytes 0..7) and row `i+8` (bytes 8..15).
/// Output: `(c0..c7)`, where register `cj` holds column `j` across all 16 rows (bytes 0..15).
///
/// Since two independent 8x8 matrix transpositions form an involution ($A^{TT} = A$),
/// calling this function again on `(c0..c7)` transposes back into `(r0..r7)`.
#[inline(always)]
unsafe fn transpose_16x8_u8(
    r0: __m128i,
    r1: __m128i,
    r2: __m128i,
    r3: __m128i,
    r4: __m128i,
    r5: __m128i,
    r6: __m128i,
    r7: __m128i,
) -> (
    __m128i,
    __m128i,
    __m128i,
    __m128i,
    __m128i,
    __m128i,
    __m128i,
    __m128i,
) {
    // Stage 1: Interleave adjacent rows at byte level.
    // t0..t3 process lower 8x8 block (rows 0..7), u0..u3 process upper 8x8 block (rows 8..15).
    let t0 = _mm_unpacklo_epi8(r0, r1);
    let t1 = _mm_unpacklo_epi8(r2, r3);
    let t2 = _mm_unpacklo_epi8(r4, r5);
    let t3 = _mm_unpacklo_epi8(r6, r7);

    let u0 = _mm_unpackhi_epi8(r0, r1);
    let u1 = _mm_unpackhi_epi8(r2, r3);
    let u2 = _mm_unpackhi_epi8(r4, r5);
    let u3 = _mm_unpackhi_epi8(r6, r7);

    // Stage 2: Interleave 16-bit words.
    let w0 = _mm_unpacklo_epi16(t0, t1);
    let w1 = _mm_unpackhi_epi16(t0, t1);
    let w2 = _mm_unpacklo_epi16(t2, t3);
    let w3 = _mm_unpackhi_epi16(t2, t3);

    let v0 = _mm_unpacklo_epi16(u0, u1);
    let v1 = _mm_unpackhi_epi16(u0, u1);
    let v2 = _mm_unpacklo_epi16(u2, u3);
    let v3 = _mm_unpackhi_epi16(u2, u3);

    // Stage 3: Interleave 32-bit dwords.
    let d0 = _mm_unpacklo_epi32(w0, w2);
    let d1 = _mm_unpackhi_epi32(w0, w2);
    let d2 = _mm_unpacklo_epi32(w1, w3);
    let d3 = _mm_unpackhi_epi32(w1, w3);

    let e0 = _mm_unpacklo_epi32(v0, v2);
    let e1 = _mm_unpackhi_epi32(v0, v2);
    let e2 = _mm_unpacklo_epi32(v1, v3);
    let e3 = _mm_unpackhi_epi32(v1, v3);

    // Stage 4: Combine 64-bit halves from lower and upper blocks into 16-sample columns.
    let c0 = _mm_unpacklo_epi64(d0, e0);
    let c1 = _mm_unpackhi_epi64(d0, e0);
    let c2 = _mm_unpacklo_epi64(d1, e1);
    let c3 = _mm_unpackhi_epi64(d1, e1);
    let c4 = _mm_unpacklo_epi64(d2, e2);
    let c5 = _mm_unpackhi_epi64(d2, e2);
    let c6 = _mm_unpacklo_epi64(d3, e3);
    let c7 = _mm_unpackhi_epi64(d3, e3);

    (c0, c1, c2, c3, c4, c5, c6, c7)
}

/// Transposes 8 lines of 4-sample Cb and Cr taps into 4 16-byte column vectors.
///
/// Input: `r0..r7`, where register `ri` holds Cb row `i` in dword 0 (bytes 0..3)
/// and Cr row `i` in dword 1 (bytes 4..7).
/// Output: `(t0, t1, t2, t3)`, where each register `tj` contains column `j` across all 8 rows
/// of Cb in the lower 8 bytes, and all 8 rows of Cr in the upper 8 bytes.
#[inline(always)]
unsafe fn transpose_chroma_4x8_u8(
    r0: __m128i,
    r1: __m128i,
    r2: __m128i,
    r3: __m128i,
    r4: __m128i,
    r5: __m128i,
    r6: __m128i,
    r7: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    // Stage 1: byte unpack
    let a0 = _mm_unpacklo_epi8(r0, r1);
    let a1 = _mm_unpacklo_epi8(r2, r3);
    let a2 = _mm_unpacklo_epi8(r4, r5);
    let a3 = _mm_unpacklo_epi8(r6, r7);

    // Stage 2: word unpack
    let b0 = _mm_unpacklo_epi16(a0, a1);
    let b1 = _mm_unpackhi_epi16(a0, a1);
    let b2 = _mm_unpacklo_epi16(a2, a3);
    let b3 = _mm_unpackhi_epi16(a2, a3);

    // Stage 3: dword unpack
    let cb01 = _mm_unpacklo_epi32(b0, b2);
    let cb23 = _mm_unpackhi_epi32(b0, b2);
    let cr01 = _mm_unpacklo_epi32(b1, b3);
    let cr23 = _mm_unpackhi_epi32(b1, b3);

    // Stage 4: qword unpack combining Cb and Cr
    let t0 = _mm_unpacklo_epi64(cb01, cr01);
    let t1 = _mm_unpackhi_epi64(cb01, cr01);
    let t2 = _mm_unpacklo_epi64(cb23, cr23);
    let t3 = _mm_unpackhi_epi64(cb23, cr23);

    (t0, t1, t2, t3)
}

// ============================================================================
// Public Dispatch Functions
// ============================================================================

/// Accelerated Luma Lt4 filter (bS < 4).
/// # Preconditions
///
/// The direction guard below (`step_y == 1` / `step_x == 1`) is only half the
/// contract: this kernel addresses in 2D through the cursor, so the other step must
/// also be the cursor's own stride. A caller that satisfies the guard with a
/// different pitch reads and writes the wrong samples; the `debug_assert!` checks it.
pub fn deblock_luma_lt4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    if step_y == 1 {
        // The cross-line step must be this cursor's stride.
        debug_assert_eq!(step_x, pix.stride() as isize);
        // Horizontal edge: taps `-3 .. 2` step vertically — one 16-wide, 6-tall span.
        let (mut p2, mut p1, mut p0, mut q0, mut q1, mut q2) = {
            let s = pix.span::<16, 6>(-3, 0);
            (
                s.row::<16>(0, 0),
                s.row::<16>(1, 0),
                s.row::<16>(2, 0),
                s.row::<16>(3, 0),
                s.row::<16>(4, 0),
                s.row::<16>(5, 0),
            )
        };

        unsafe {
            deblock_luma_lt4_16(
                &mut p2, &mut p1, &mut p0, &mut q0, &mut q1, &mut q2, alpha, beta, tc,
            );
        }

        pix.set_block::<16, 4>(-2, 0, &[p1, p0, q0, q1]);
    } else if step_x == 1 {
        // The cross-line step must be this cursor's stride.
        debug_assert_eq!(step_y, pix.stride() as isize);
        // Vertical edge: 16 lines of taps at cols `-4 .. 4`, out of one span.
        unsafe {
            let (r0, r1, r2, r3, r4, r5, r6, r7) = {
                let s = pix.span::<8, 16>(0, -4);
                (
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(8, 0)),
                        i64::from_ne_bytes(s.row::<8>(0, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(9, 0)),
                        i64::from_ne_bytes(s.row::<8>(1, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(10, 0)),
                        i64::from_ne_bytes(s.row::<8>(2, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(11, 0)),
                        i64::from_ne_bytes(s.row::<8>(3, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(12, 0)),
                        i64::from_ne_bytes(s.row::<8>(4, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(13, 0)),
                        i64::from_ne_bytes(s.row::<8>(5, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(14, 0)),
                        i64::from_ne_bytes(s.row::<8>(6, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(15, 0)),
                        i64::from_ne_bytes(s.row::<8>(7, 0)),
                    ),
                )
            };

            let (_c0, c1, c2, c3, c4, c5, c6, _c7) =
                transpose_16x8_u8(r0, r1, r2, r3, r4, r5, r6, r7);

            let mut t1 = [0u8; 16];
            let mut t2 = [0u8; 16];
            let mut t3 = [0u8; 16];
            let mut t4 = [0u8; 16];
            let mut t5 = [0u8; 16];
            let mut t6 = [0u8; 16];
            _mm_storeu_si128(t1.as_mut_ptr() as *mut __m128i, c1);
            _mm_storeu_si128(t2.as_mut_ptr() as *mut __m128i, c2);
            _mm_storeu_si128(t3.as_mut_ptr() as *mut __m128i, c3);
            _mm_storeu_si128(t4.as_mut_ptr() as *mut __m128i, c4);
            _mm_storeu_si128(t5.as_mut_ptr() as *mut __m128i, c5);
            _mm_storeu_si128(t6.as_mut_ptr() as *mut __m128i, c6);

            deblock_luma_lt4_16(
                &mut t1, &mut t2, &mut t3, &mut t4, &mut t5, &mut t6, alpha, beta, tc,
            );

            let p1_vec = _mm_loadu_si128(t2.as_ptr() as *const __m128i);
            let p0_vec = _mm_loadu_si128(t3.as_ptr() as *const __m128i);
            let q0_vec = _mm_loadu_si128(t4.as_ptr() as *const __m128i);
            let q1_vec = _mm_loadu_si128(t5.as_ptr() as *const __m128i);

            // Interleave (p1, p0) and (q0, q1) pairs
            let a0 = _mm_unpacklo_epi8(p1_vec, p0_vec);
            let a1 = _mm_unpackhi_epi8(p1_vec, p0_vec);
            let b0 = _mm_unpacklo_epi8(q0_vec, q1_vec);
            let b1 = _mm_unpackhi_epi8(q0_vec, q1_vec);

            // Interleave pairs into 4-sample rows: [p1, p0, q0, q1]
            let row0_3 = _mm_unpacklo_epi16(a0, b0);
            let row4_7 = _mm_unpackhi_epi16(a0, b0);
            let row8_11 = _mm_unpacklo_epi16(a1, b1);
            let row12_15 = _mm_unpackhi_epi16(a1, b1);

            let mut out = [[0u8; 4]; 16];
            let ptr = out.as_mut_ptr() as *mut __m128i;
            _mm_storeu_si128(ptr.add(0), row0_3);
            _mm_storeu_si128(ptr.add(1), row4_7);
            _mm_storeu_si128(ptr.add(2), row8_11);
            _mm_storeu_si128(ptr.add(3), row12_15);

            // Write back only the columns the filter can modify: the span read above is
            // wider for the outer taps, and at `iEdge == 0` those outer columns belong to
            // the previous macroblock.
            pix.set_block::<4, 16>(0, -2, &out);
        }
    } else {
        deblock_luma_lt4_scalar(pix, step_x, step_y, alpha, beta, tc);
    }
}

/// Accelerated Luma Eq4 filter (bS == 4).
/// # Preconditions
///
/// The direction guard below (`step_y == 1` / `step_x == 1`) is only half the
/// contract: this kernel addresses in 2D through the cursor, so the other step must
/// also be the cursor's own stride. A caller that satisfies the guard with a
/// different pitch reads and writes the wrong samples; the `debug_assert!` checks it.
pub fn deblock_luma_eq4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    if step_y == 1 {
        // The cross-line step must be this cursor's stride.
        debug_assert_eq!(step_x, pix.stride() as isize);
        // Horizontal edge: taps `-4 .. 3` step vertically — one 16-wide, 8-tall span.
        let (p3, mut p2, mut p1, mut p0, mut q0, mut q1, mut q2, q3) = {
            let s = pix.span::<16, 8>(-4, 0);
            (
                s.row::<16>(0, 0),
                s.row::<16>(1, 0),
                s.row::<16>(2, 0),
                s.row::<16>(3, 0),
                s.row::<16>(4, 0),
                s.row::<16>(5, 0),
                s.row::<16>(6, 0),
                s.row::<16>(7, 0),
            )
        };

        unsafe {
            deblock_luma_eq4_16(
                &p3, &mut p2, &mut p1, &mut p0, &mut q0, &mut q1, &mut q2, &q3, alpha, beta,
            );
        }

        pix.set_block::<16, 6>(-3, 0, &[p2, p1, p0, q0, q1, q2]);
    } else if step_x == 1 {
        // The cross-line step must be this cursor's stride.
        debug_assert_eq!(step_y, pix.stride() as isize);
        // Vertical edge: 16 lines of taps `-4 .. 4`, out of one span.
        unsafe {
            let (r0, r1, r2, r3, r4, r5, r6, r7) = {
                let s = pix.span::<8, 16>(0, -4);
                (
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(8, 0)),
                        i64::from_ne_bytes(s.row::<8>(0, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(9, 0)),
                        i64::from_ne_bytes(s.row::<8>(1, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(10, 0)),
                        i64::from_ne_bytes(s.row::<8>(2, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(11, 0)),
                        i64::from_ne_bytes(s.row::<8>(3, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(12, 0)),
                        i64::from_ne_bytes(s.row::<8>(4, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(13, 0)),
                        i64::from_ne_bytes(s.row::<8>(5, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(14, 0)),
                        i64::from_ne_bytes(s.row::<8>(6, 0)),
                    ),
                    _mm_set_epi64x(
                        i64::from_ne_bytes(s.row::<8>(15, 0)),
                        i64::from_ne_bytes(s.row::<8>(7, 0)),
                    ),
                )
            };

            let (c0, mut c1, mut c2, mut c3, mut c4, mut c5, mut c6, c7) =
                transpose_16x8_u8(r0, r1, r2, r3, r4, r5, r6, r7);

            let mut t0 = [0u8; 16];
            let mut t1 = [0u8; 16];
            let mut t2 = [0u8; 16];
            let mut t3 = [0u8; 16];
            let mut t4 = [0u8; 16];
            let mut t5 = [0u8; 16];
            let mut t6 = [0u8; 16];
            let mut t7 = [0u8; 16];
            _mm_storeu_si128(t0.as_mut_ptr() as *mut __m128i, c0);
            _mm_storeu_si128(t1.as_mut_ptr() as *mut __m128i, c1);
            _mm_storeu_si128(t2.as_mut_ptr() as *mut __m128i, c2);
            _mm_storeu_si128(t3.as_mut_ptr() as *mut __m128i, c3);
            _mm_storeu_si128(t4.as_mut_ptr() as *mut __m128i, c4);
            _mm_storeu_si128(t5.as_mut_ptr() as *mut __m128i, c5);
            _mm_storeu_si128(t6.as_mut_ptr() as *mut __m128i, c6);
            _mm_storeu_si128(t7.as_mut_ptr() as *mut __m128i, c7);

            deblock_luma_eq4_16(
                &t0, &mut t1, &mut t2, &mut t3, &mut t4, &mut t5, &mut t6, &t7, alpha, beta,
            );

            c1 = _mm_loadu_si128(t1.as_ptr() as *const __m128i);
            c2 = _mm_loadu_si128(t2.as_ptr() as *const __m128i);
            c3 = _mm_loadu_si128(t3.as_ptr() as *const __m128i);
            c4 = _mm_loadu_si128(t4.as_ptr() as *const __m128i);
            c5 = _mm_loadu_si128(t5.as_ptr() as *const __m128i);
            c6 = _mm_loadu_si128(t6.as_ptr() as *const __m128i);

            let (r0_out, r1_out, r2_out, r3_out, r4_out, r5_out, r6_out, r7_out) =
                transpose_16x8_u8(c0, c1, c2, c3, c4, c5, c6, c7);

            let r_outs = [
                r0_out, r1_out, r2_out, r3_out, r4_out, r5_out, r6_out, r7_out,
            ];
            let mut out = [[0u8; 6]; 16];
            for i in 0..8 {
                let b_lo = (_mm_cvtsi128_si64(r_outs[i]) as u64).to_ne_bytes();
                let b_hi = (_mm_cvtsi128_si64(_mm_srli_si128(r_outs[i], 8)) as u64).to_ne_bytes();
                out[i].copy_from_slice(&b_lo[1..7]);
                out[i + 8].copy_from_slice(&b_hi[1..7]);
            }

            // Write back only the columns the filter can modify: the span read above is
            // wider for the outer taps, and at `iEdge == 0` those outer columns belong to
            // the previous macroblock.
            pix.set_block::<6, 16>(0, -3, &out);
        }
    } else {
        deblock_luma_eq4_scalar(pix, step_x, step_y, alpha, beta);
    }
}

/// Accelerated Chroma Lt4 filter (bS < 4).
/// # Preconditions
///
/// The direction guard below (`step_y == 1` / `step_x == 1`) is only half the
/// contract: this kernel addresses in 2D through the cursor, so the other step must
/// also be the cursor's own stride. A caller that satisfies the guard with a
/// different pitch reads and writes the wrong samples; the `debug_assert!` checks it.
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
        // The cross-line step must be this cursor's stride; Cb and Cr are separate
        // planes, so both are checked.
        debug_assert_eq!(step_x, cb.stride() as isize);
        debug_assert_eq!(step_x, cr.stride() as isize);
        // Taps `-2 .. 1` of each plane: one 8-wide, 4-tall span apiece.
        let (cb_p1, mut cb_p0, mut cb_q0, cb_q1, cr_p1, mut cr_p0, mut cr_q0, cr_q1) = {
            let (sb, sr) = (cb.span::<8, 4>(-2, 0), cr.span::<8, 4>(-2, 0));
            (
                sb.row::<8>(0, 0),
                sb.row::<8>(1, 0),
                sb.row::<8>(2, 0),
                sb.row::<8>(3, 0),
                sr.row::<8>(0, 0),
                sr.row::<8>(1, 0),
                sr.row::<8>(2, 0),
                sr.row::<8>(3, 0),
            )
        };

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

        unsafe {
            deblock_chroma_lt4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta, tc);
        }

        cb_p0.copy_from_slice(&p0[..8]);
        cr_p0.copy_from_slice(&p0[8..]);
        cb_q0.copy_from_slice(&q0[..8]);
        cr_q0.copy_from_slice(&q0[8..]);

        cb.set_block::<8, 2>(-1, 0, &[cb_p0, cb_q0]);
        cr.set_block::<8, 2>(-1, 0, &[cr_p0, cr_q0]);
    } else if step_x == 1 {
        // The cross-line step must be this cursor's stride; Cb and Cr are separate
        // planes, so both are checked.
        debug_assert_eq!(step_y, cb.stride() as isize);
        debug_assert_eq!(step_y, cr.stride() as isize);
        // Eight lines of taps `-2 .. 2` per plane, out of one span each.
        unsafe {
            let (r0, r1, r2, r3, r4, r5, r6, r7) = {
                let (sb, sr) = (cb.span::<4, 8>(0, -2), cr.span::<4, 8>(0, -2));
                (
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(0, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(0, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(1, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(1, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(2, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(2, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(3, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(3, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(4, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(4, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(5, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(5, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(6, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(6, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(7, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(7, 0)) as i32,
                        0,
                        0,
                    ),
                )
            };

            let (t0, t1, t2, t3) = transpose_chroma_4x8_u8(r0, r1, r2, r3, r4, r5, r6, r7);

            let mut arr_p1 = [0u8; 16];
            let mut arr_p0 = [0u8; 16];
            let mut arr_q0 = [0u8; 16];
            let mut arr_q1 = [0u8; 16];
            _mm_storeu_si128(arr_p1.as_mut_ptr() as *mut __m128i, t0);
            _mm_storeu_si128(arr_p0.as_mut_ptr() as *mut __m128i, t1);
            _mm_storeu_si128(arr_q0.as_mut_ptr() as *mut __m128i, t2);
            _mm_storeu_si128(arr_q1.as_mut_ptr() as *mut __m128i, t3);

            deblock_chroma_lt4_16(&arr_p1, &mut arr_p0, &mut arr_q0, &arr_q1, alpha, beta, tc);

            let p0 = _mm_loadu_si128(arr_p0.as_ptr() as *const __m128i);
            let q0 = _mm_loadu_si128(arr_q0.as_ptr() as *const __m128i);

            let cb_pairs = _mm_unpacklo_epi8(p0, q0);
            let cr_pairs = _mm_unpackhi_epi8(p0, q0);

            let mut out_cb = [[0u8; 2]; 8];
            let mut out_cr = [[0u8; 2]; 8];
            _mm_storeu_si128(out_cb.as_mut_ptr() as *mut __m128i, cb_pairs);
            _mm_storeu_si128(out_cr.as_mut_ptr() as *mut __m128i, cr_pairs);

            // Write back only the columns the filter can modify: the span read above is
            // wider for the outer taps, and at `iEdge == 0` those outer columns belong to
            // the previous macroblock.
            cb.set_block::<2, 8>(0, -1, &out_cb);
            cr.set_block::<2, 8>(0, -1, &out_cr);
        }
    } else {
        deblock_chroma_lt4_scalar(cb, cr, step_x, step_y, alpha, beta, tc);
    }
}

/// Accelerated Chroma Eq4 filter (bS == 4).
/// # Preconditions
///
/// The direction guard below (`step_y == 1` / `step_x == 1`) is only half the
/// contract: this kernel addresses in 2D through the cursor, so the other step must
/// also be the cursor's own stride. A caller that satisfies the guard with a
/// different pitch reads and writes the wrong samples; the `debug_assert!` checks it.
pub fn deblock_chroma_eq4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    if step_y == 1 {
        // The cross-line step must be this cursor's stride; Cb and Cr are separate
        // planes, so both are checked.
        debug_assert_eq!(step_x, cb.stride() as isize);
        debug_assert_eq!(step_x, cr.stride() as isize);
        // Taps `-2 .. 1` of each plane: one 8-wide, 4-tall span apiece.
        let (cb_p1, mut cb_p0, mut cb_q0, cb_q1, cr_p1, mut cr_p0, mut cr_q0, cr_q1) = {
            let (sb, sr) = (cb.span::<8, 4>(-2, 0), cr.span::<8, 4>(-2, 0));
            (
                sb.row::<8>(0, 0),
                sb.row::<8>(1, 0),
                sb.row::<8>(2, 0),
                sb.row::<8>(3, 0),
                sr.row::<8>(0, 0),
                sr.row::<8>(1, 0),
                sr.row::<8>(2, 0),
                sr.row::<8>(3, 0),
            )
        };

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

        unsafe {
            deblock_chroma_eq4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta);
        }

        cb_p0.copy_from_slice(&p0[..8]);
        cr_p0.copy_from_slice(&p0[8..]);
        cb_q0.copy_from_slice(&q0[..8]);
        cr_q0.copy_from_slice(&q0[8..]);

        cb.set_block::<8, 2>(-1, 0, &[cb_p0, cb_q0]);
        cr.set_block::<8, 2>(-1, 0, &[cr_p0, cr_q0]);
    } else if step_x == 1 {
        // The cross-line step must be this cursor's stride; Cb and Cr are separate
        // planes, so both are checked.
        debug_assert_eq!(step_y, cb.stride() as isize);
        debug_assert_eq!(step_y, cr.stride() as isize);
        // Eight lines of taps `-2 .. 2` per plane, out of one span each.
        unsafe {
            let (r0, r1, r2, r3, r4, r5, r6, r7) = {
                let (sb, sr) = (cb.span::<4, 8>(0, -2), cr.span::<4, 8>(0, -2));
                (
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(0, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(0, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(1, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(1, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(2, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(2, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(3, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(3, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(4, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(4, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(5, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(5, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(6, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(6, 0)) as i32,
                        0,
                        0,
                    ),
                    _mm_setr_epi32(
                        u32::from_ne_bytes(sb.row::<4>(7, 0)) as i32,
                        u32::from_ne_bytes(sr.row::<4>(7, 0)) as i32,
                        0,
                        0,
                    ),
                )
            };

            let (t0, t1, t2, t3) = transpose_chroma_4x8_u8(r0, r1, r2, r3, r4, r5, r6, r7);

            let mut arr_p1 = [0u8; 16];
            let mut arr_p0 = [0u8; 16];
            let mut arr_q0 = [0u8; 16];
            let mut arr_q1 = [0u8; 16];
            _mm_storeu_si128(arr_p1.as_mut_ptr() as *mut __m128i, t0);
            _mm_storeu_si128(arr_p0.as_mut_ptr() as *mut __m128i, t1);
            _mm_storeu_si128(arr_q0.as_mut_ptr() as *mut __m128i, t2);
            _mm_storeu_si128(arr_q1.as_mut_ptr() as *mut __m128i, t3);

            deblock_chroma_eq4_16(&arr_p1, &mut arr_p0, &mut arr_q0, &arr_q1, alpha, beta);

            let p0 = _mm_loadu_si128(arr_p0.as_ptr() as *const __m128i);
            let q0 = _mm_loadu_si128(arr_q0.as_ptr() as *const __m128i);

            let cb_pairs = _mm_unpacklo_epi8(p0, q0);
            let cr_pairs = _mm_unpackhi_epi8(p0, q0);

            let mut out_cb = [[0u8; 2]; 8];
            let mut out_cr = [[0u8; 2]; 8];
            _mm_storeu_si128(out_cb.as_mut_ptr() as *mut __m128i, cb_pairs);
            _mm_storeu_si128(out_cr.as_mut_ptr() as *mut __m128i, cr_pairs);

            // Write back only the columns the filter can modify: the span read above is
            // wider for the outer taps, and at `iEdge == 0` those outer columns belong to
            // the previous macroblock.
            cb.set_block::<2, 8>(0, -1, &out_cb);
            cr.set_block::<2, 8>(0, -1, &out_cr);
        }
    } else {
        deblock_chroma_eq4_scalar(cb, cr, step_x, step_y, alpha, beta);
    }
}

// ============================================================================
// Boundary Strength Calculation (bs_calc)
// ============================================================================

const COLUMN_MAJOR: [u8; 16] = [
    0, 4, 8, 12, // col 0
    1, 5, 9, 13, // col 1
    2, 6, 10, 14, // col 2
    3, 7, 11, 15, // col 3
];

const PACK_EVEN_BYTES_LO: [u8; 16] = [
    0, 2, 4, 6, 8, 10, 12, 14, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
];

const PACK_EVEN_BYTES_HI: [u8; 16] = [
    0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0, 2, 4, 6, 8, 10, 12, 14,
];

/// The four counts at `idx` of a neighbour's table, as one word.
#[inline]
fn nzc_word(nzc: &[i8; 24], idx: [usize; 4]) -> u32 {
    u32::from_ne_bytes(idx.map(|i| nzc[i] as u8))
}

/// A macroblock's sixteen luma counts as bytes. Only their non-zero-ness is read, so
/// the sign does not matter.
#[inline]
fn nzc_word_bytes(nzc: &[i8; 24]) -> [u8; 16] {
    std::array::from_fn(|i| nzc[i] as u8)
}

/// `BS_NZC_CHECK`'s tail: `2` wherever either of the two blocks has a coefficient.
#[inline]
#[target_feature(enable = "sse4.1")]
unsafe fn nzc_term(cur: __m128i, prev: __m128i) -> __m128i {
    let both = _mm_or_si128(cur, prev);
    let is_zero = _mm_cmpeq_epi8(both, _mm_setzero_si128());
    _mm_andnot_si128(is_zero, _mm_set1_epi8(2))
}

/// The `k`-th group of four motion vectors, as eight halfword lanes.
#[inline]
#[target_feature(enable = "sse4.1")]
unsafe fn ld_mv4<const K: usize>(mv: &[SMVUnitXY; 16]) -> __m128i {
    const { assert!(K < 4, "a macroblock holds four groups of four vectors") };
    _mm_loadu_si128(mv.as_ptr().cast::<__m128i>().add(K))
}

/// Four motion vectors gathered from scattered indices — the left neighbour's last
/// column, which is the one place contiguous load cannot serve.
#[inline]
#[target_feature(enable = "sse4.1")]
unsafe fn gather_mv4(mv: &[SMVUnitXY; 16], idx: [usize; 4]) -> __m128i {
    let mut w = [0i16; 8];
    for (j, &i) in idx.iter().enumerate() {
        w[2 * j] = mv[i].iMvX;
        w[2 * j + 1] = mv[i].iMvY;
    }
    _mm_loadu_si128(w.as_ptr() as *const __m128i)
}

/// `BS_COMPARE_MV`: for four vector pairs, a set mask where either component differs
/// by four or more — the whole-sample threshold `MB_BS_MV`/`SMB_EDGE_MV` test.
///
/// `|a - b| >= 4` is spelled as saturating subtraction each way and the larger of the two,
/// clamping differences past `i16::MAX` to `i16::MAX` (>= 4).
#[inline]
#[target_feature(enable = "sse4.1")]
unsafe fn mv_ge4(a: __m128i, b: __m128i) -> __m128i {
    let d = _mm_max_epi16(_mm_subs_epi16(a, b), _mm_subs_epi16(b, a));
    _mm_cmpgt_epi16(d, _mm_set1_epi16(3))
}

/// The four groups' comparisons folded to sixteen `1`/`0` bytes.
#[inline]
#[target_feature(enable = "sse4.1")]
unsafe fn mv_term(m: [__m128i; 4]) -> __m128i {
    let b01 = _mm_packs_epi16(m[0], m[1]);
    let b23 = _mm_packs_epi16(m[2], m[3]);

    let or01 = _mm_or_si128(b01, _mm_srli_si128(b01, 1));
    let or23 = _mm_or_si128(b23, _mm_srli_si128(b23, 1));

    let lo = _mm_shuffle_epi8(
        or01,
        _mm_loadu_si128(PACK_EVEN_BYTES_LO.as_ptr() as *const __m128i),
    );
    let hi = _mm_shuffle_epi8(
        or23,
        _mm_loadu_si128(PACK_EVEN_BYTES_HI.as_ptr() as *const __m128i),
    );

    _mm_and_si128(_mm_or_si128(lo, hi), _mm_set1_epi8(1))
}

/// The per-direction mask: the macroblock edge kept or cleared, the three interior
/// edges reduced to what this macroblock kind's rule allows.
#[inline]
#[target_feature(enable = "sse4.1")]
unsafe fn bs_mask(edge_present: bool, inside: u8) -> __m128i {
    let mut m = [inside; 16];
    m[..4].fill(if edge_present { 0xFF } else { 0 });
    _mm_loadu_si128(m.as_ptr() as *const __m128i)
}

/// Vectorized deblocking boundary strength calculation for one macroblock.
///
/// Accelerated using SSSE3/SSE4.1 intrinsics:
/// - `pshufb` for NZC column transpose and MV byte packing
/// - `pslldq` and `por` for boundary neighbor alignment
/// - `psubsw`, `pmaxsw`, `pcmpgtw` for MV threshold testing
/// - `pmaxub` and `pand` for composite strength and edge masking
#[inline]
#[target_feature(enable = "sse4.1")]
unsafe fn bs_calc_sse41(
    cur_nzc: &[i8; 24],
    cur_mv: &[SMVUnitXY; 16],
    left: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    top: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    inside: u8,
    bs: &mut [[[u8; 4]; 4]; 2],
) {
    let rows = _mm_loadu_si128(nzc_word_bytes(cur_nzc).as_ptr() as *const __m128i);
    let cols = _mm_shuffle_epi8(
        rows,
        _mm_loadu_si128(COLUMN_MAJOR.as_ptr() as *const __m128i),
    );

    let top_word = top.map_or(0, |(n, _)| nzc_word(n, [12, 13, 14, 15]));
    let left_word = left.map_or(0, |(n, _)| nzc_word(n, [3, 7, 11, 15]));

    let prev_rows = _mm_or_si128(_mm_slli_si128(rows, 4), _mm_cvtsi32_si128(top_word as i32));
    let prev_cols = _mm_or_si128(_mm_slli_si128(cols, 4), _mm_cvtsi32_si128(left_word as i32));

    let (r0, r1, r2, r3) = (
        ld_mv4::<0>(cur_mv),
        ld_mv4::<1>(cur_mv),
        ld_mv4::<2>(cur_mv),
        ld_mv4::<3>(cur_mv),
    );

    let t0 = _mm_unpacklo_epi32(r0, r1);
    let t1 = _mm_unpackhi_epi32(r0, r1);
    let t2 = _mm_unpacklo_epi32(r2, r3);
    let t3 = _mm_unpackhi_epi32(r2, r3);

    let c0 = _mm_unpacklo_epi64(t0, t2);
    let c1 = _mm_unpackhi_epi64(t0, t2);
    let c2 = _mm_unpacklo_epi64(t1, t3);
    let c3 = _mm_unpackhi_epi64(t1, t3);

    let zero_mv = _mm_setzero_si128();
    let prev_r = top.map_or(zero_mv, |(_, m)| ld_mv4::<3>(m));
    let prev_c = left.map_or(zero_mv, |(_, m)| gather_mv4(m, [3, 7, 11, 15]));

    let mv_rows = mv_term([
        mv_ge4(prev_r, r0),
        mv_ge4(r0, r1),
        mv_ge4(r1, r2),
        mv_ge4(r2, r3),
    ]);
    let mv_cols = mv_term([
        mv_ge4(prev_c, c0),
        mv_ge4(c0, c1),
        mv_ge4(c1, c2),
        mv_ge4(c2, c3),
    ]);

    let vertical = _mm_and_si128(
        _mm_max_epu8(nzc_term(cols, prev_cols), mv_cols),
        bs_mask(left.is_some(), inside),
    );
    let horizontal = _mm_and_si128(
        _mm_max_epu8(nzc_term(rows, prev_rows), mv_rows),
        bs_mask(top.is_some(), inside),
    );

    let (v, h) = bs.split_at_mut(1);
    _mm_storeu_si128(
        v[0].as_flattened_mut().as_mut_ptr() as *mut __m128i,
        vertical,
    );
    _mm_storeu_si128(
        h[0].as_flattened_mut().as_mut_ptr() as *mut __m128i,
        horizontal,
    );
}

/// The boundary strengths of one macroblock.
///
/// Accelerated implementation using SSSE3/SSE4.1 intrinsics; see
/// [`bs_calc_scalar`](crate::encoder::deblocking::bs_calc_scalar) for the contract.
#[inline]
pub fn bs_calc(
    cur_nzc: &[i8; 24],
    cur_mv: &[SMVUnitXY; 16],
    left: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    top: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    inside: u8,
    bs: &mut [[[u8; 4]; 4]; 2],
) {
    // SAFETY: SSE4.1/SSSE3 is baseline for x86_64 SIMD.
    unsafe {
        bs_calc_sse41(cur_nzc, cur_mv, left, top, inside, bs);
    }
}

// ============================================================================
// Unit Tests & Parity Verification
// ============================================================================

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
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut plane_scalar = make_test_plane(32, 32, 16, stride);
            let mut plane_simd = plane_scalar.clone();

            let alpha = 20;
            let beta = 12;
            let tc = [2i8, 3, 1, 4];

            deblock_luma_lt4_scalar(
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
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut plane_scalar = make_test_plane(32, 32, 16, stride);
            let mut plane_simd = plane_scalar.clone();

            let alpha = 24;
            let beta = 15;

            deblock_luma_eq4_scalar(
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
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut cb_scalar = make_test_plane(16, 16, 8, stride);
            let mut cr_scalar = make_test_plane(16, 16, 8, stride);
            let mut cb_simd = cb_scalar.clone();
            let mut cr_simd = cr_scalar.clone();

            let alpha = 18;
            let beta = 10;
            let tc = [1i8, 2, 0, 3];

            deblock_chroma_lt4_scalar(
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
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut cb_scalar = make_test_plane(16, 16, 8, stride);
            let mut cr_scalar = make_test_plane(16, 16, 8, stride);
            let mut cb_simd = cb_scalar.clone();
            let mut cr_simd = cr_scalar.clone();

            let alpha = 22;
            let beta = 14;

            deblock_chroma_eq4_scalar(
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

    #[test]
    fn test_bs_calc_parity() {
        use crate::encoder::deblocking::bs_calc_scalar;

        let mut seed = 0x123456789abcdef0u64;
        let mut rng = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 32) as u32
        };

        for _ in 0..1000 {
            let mut cur_nzc = [0i8; 24];
            for x in cur_nzc.iter_mut() {
                *x = if (rng() % 3) == 0 {
                    (rng() % 16) as i8
                } else {
                    0
                };
            }
            let mut cur_mv = [SMVUnitXY { iMvX: 0, iMvY: 0 }; 16];
            for m in cur_mv.iter_mut() {
                m.iMvX = (rng() as i16) % 100;
                m.iMvY = (rng() as i16) % 100;
            }

            let has_left = (rng() % 2) != 0;
            let mut left_nzc = [0i8; 24];
            let mut left_mv = [SMVUnitXY { iMvX: 0, iMvY: 0 }; 16];
            let left = if has_left {
                for x in left_nzc.iter_mut() {
                    *x = if (rng() % 3) == 0 {
                        (rng() % 16) as i8
                    } else {
                        0
                    };
                }
                for m in left_mv.iter_mut() {
                    m.iMvX = (rng() as i16) % 100;
                    m.iMvY = (rng() as i16) % 100;
                }
                Some((&left_nzc, &left_mv))
            } else {
                None
            };

            let has_top = (rng() % 2) != 0;
            let mut top_nzc = [0i8; 24];
            let mut top_mv = [SMVUnitXY { iMvX: 0, iMvY: 0 }; 16];
            let top = if has_top {
                for x in top_nzc.iter_mut() {
                    *x = if (rng() % 3) == 0 {
                        (rng() % 16) as i8
                    } else {
                        0
                    };
                }
                for m in top_mv.iter_mut() {
                    m.iMvX = (rng() as i16) % 100;
                    m.iMvY = (rng() as i16) % 100;
                }
                Some((&top_nzc, &top_mv))
            } else {
                None
            };

            let inside_masks = [0x00, 0x02, 0xFF];
            let inside = inside_masks[(rng() % 3) as usize];

            let mut bs_expected = [[[0u8; 4]; 4]; 2];
            let mut bs_actual = [[[0u8; 4]; 4]; 2];

            bs_calc_scalar(&cur_nzc, &cur_mv, left, top, inside, &mut bs_expected);
            bs_calc(&cur_nzc, &cur_mv, left, top, inside, &mut bs_actual);

            assert_eq!(bs_actual, bs_expected, "bs_calc mismatch");
        }
    }

    #[test]
    fn test_transpose_16x8_roundtrip() {
        let mut rows = [[0u8; 8]; 16];
        for y in 0..16 {
            for x in 0..8 {
                rows[y][x] = (y * 13 + x * 7 + 42) as u8;
            }
        }
        unsafe {
            let r0 = _mm_set_epi64x(i64::from_ne_bytes(rows[8]), i64::from_ne_bytes(rows[0]));
            let r1 = _mm_set_epi64x(i64::from_ne_bytes(rows[9]), i64::from_ne_bytes(rows[1]));
            let r2 = _mm_set_epi64x(i64::from_ne_bytes(rows[10]), i64::from_ne_bytes(rows[2]));
            let r3 = _mm_set_epi64x(i64::from_ne_bytes(rows[11]), i64::from_ne_bytes(rows[3]));
            let r4 = _mm_set_epi64x(i64::from_ne_bytes(rows[12]), i64::from_ne_bytes(rows[4]));
            let r5 = _mm_set_epi64x(i64::from_ne_bytes(rows[13]), i64::from_ne_bytes(rows[5]));
            let r6 = _mm_set_epi64x(i64::from_ne_bytes(rows[14]), i64::from_ne_bytes(rows[6]));
            let r7 = _mm_set_epi64x(i64::from_ne_bytes(rows[15]), i64::from_ne_bytes(rows[7]));

            let (c0, c1, c2, c3, c4, c5, c6, c7) =
                transpose_16x8_u8(r0, r1, r2, r3, r4, r5, r6, r7);

            let cols = [c0, c1, c2, c3, c4, c5, c6, c7];
            for x in 0..8 {
                let mut col_arr = [0u8; 16];
                _mm_storeu_si128(col_arr.as_mut_ptr() as *mut __m128i, cols[x]);
                for y in 0..16 {
                    assert_eq!(col_arr[y], rows[y][x], "mismatch at col {x}, row {y}");
                }
            }

            let (r0_out, r1_out, r2_out, r3_out, r4_out, r5_out, r6_out, r7_out) =
                transpose_16x8_u8(c0, c1, c2, c3, c4, c5, c6, c7);

            let r_outs = [
                r0_out, r1_out, r2_out, r3_out, r4_out, r5_out, r6_out, r7_out,
            ];
            for i in 0..8 {
                let b_lo = (_mm_cvtsi128_si64(r_outs[i]) as u64).to_ne_bytes();
                let b_hi = (_mm_cvtsi128_si64(_mm_srli_si128(r_outs[i], 8)) as u64).to_ne_bytes();
                assert_eq!(b_lo, rows[i], "roundtrip mismatch at row {i}");
                assert_eq!(b_hi, rows[i + 8], "roundtrip mismatch at row {}", i + 8);
            }
        }
    }

    #[test]
    fn test_transpose_chroma_4x8() {
        let mut cb_rows = [[0u8; 4]; 8];
        let mut cr_rows = [[0u8; 4]; 8];
        for y in 0..8 {
            for x in 0..4 {
                cb_rows[y][x] = (y * 5 + x * 3 + 10) as u8;
                cr_rows[y][x] = (y * 9 + x * 11 + 50) as u8;
            }
        }
        unsafe {
            let r0 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[0]) as i32,
                u32::from_ne_bytes(cr_rows[0]) as i32,
                0,
                0,
            );
            let r1 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[1]) as i32,
                u32::from_ne_bytes(cr_rows[1]) as i32,
                0,
                0,
            );
            let r2 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[2]) as i32,
                u32::from_ne_bytes(cr_rows[2]) as i32,
                0,
                0,
            );
            let r3 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[3]) as i32,
                u32::from_ne_bytes(cr_rows[3]) as i32,
                0,
                0,
            );
            let r4 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[4]) as i32,
                u32::from_ne_bytes(cr_rows[4]) as i32,
                0,
                0,
            );
            let r5 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[5]) as i32,
                u32::from_ne_bytes(cr_rows[5]) as i32,
                0,
                0,
            );
            let r6 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[6]) as i32,
                u32::from_ne_bytes(cr_rows[6]) as i32,
                0,
                0,
            );
            let r7 = _mm_setr_epi32(
                u32::from_ne_bytes(cb_rows[7]) as i32,
                u32::from_ne_bytes(cr_rows[7]) as i32,
                0,
                0,
            );

            let (t0, t1, t2, t3) = transpose_chroma_4x8_u8(r0, r1, r2, r3, r4, r5, r6, r7);

            let cols = [t0, t1, t2, t3];
            for x in 0..4 {
                let mut col_arr = [0u8; 16];
                _mm_storeu_si128(col_arr.as_mut_ptr() as *mut __m128i, cols[x]);
                for y in 0..8 {
                    assert_eq!(col_arr[y], cb_rows[y][x], "cb mismatch at col {x}, row {y}");
                    assert_eq!(
                        col_arr[y + 8],
                        cr_rows[y][x],
                        "cr mismatch at col {x}, row {y}"
                    );
                }
            }

            let cb_pairs = _mm_unpacklo_epi8(t1, t2);
            let cr_pairs = _mm_unpackhi_epi8(t1, t2);
            let mut out_cb = [[0u8; 2]; 8];
            let mut out_cr = [[0u8; 2]; 8];
            _mm_storeu_si128(out_cb.as_mut_ptr() as *mut __m128i, cb_pairs);
            _mm_storeu_si128(out_cr.as_mut_ptr() as *mut __m128i, cr_pairs);
            for y in 0..8 {
                assert_eq!(
                    out_cb[y],
                    [cb_rows[y][1], cb_rows[y][2]],
                    "out_cb mismatch at row {y}"
                );
                assert_eq!(
                    out_cr[y],
                    [cr_rows[y][1], cr_rows[y][2]],
                    "out_cr mismatch at row {y}"
                );
            }
        }
    }
}
