//! x86_64 SSE2 Deblocking Filter Kernels (Phase 4).
//!
//! Accelerated implementations for Luma (Lt4 / Eq4) and Chroma (Lt4 / Eq4)
//! boundary filters for both horizontal and vertical edges.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use crate::safe::plane::PlaneSamples;

// ============================================================================
// Core SSE2 Vectorized Edge Filters
// ============================================================================

/// Vectorized 16-line Luma bS < 4 (Lt4) filter across contiguous sample rows.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
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
                tc[0] as i16, tc[0] as i16, tc[0] as i16, tc[0] as i16,
                tc[1] as i16, tc[1] as i16, tc[1] as i16, tc[1] as i16,
            )
        } else {
            _mm_setr_epi16(
                tc[2] as i16, tc[2] as i16, tc[2] as i16, tc[2] as i16,
                tc[3] as i16, tc[3] as i16, tc[3] as i16, tc[3] as i16,
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
#[target_feature(enable = "sse2")]
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
                    _mm_add_epi16(_mm_slli_epi16(p3_16, 1), _mm_add_epi16(_mm_slli_epi16(p2_16, 1), p2_16)),
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
                    _mm_add_epi16(_mm_slli_epi16(q3_16, 1), _mm_add_epi16(_mm_slli_epi16(q2_16, 1), q2_16)),
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
#[target_feature(enable = "sse2")]
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
        tc[0] as i16, tc[0] as i16, tc[1] as i16, tc[1] as i16,
        tc[2] as i16, tc[2] as i16, tc[3] as i16, tc[3] as i16,
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
#[target_feature(enable = "sse2")]
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
// Public Dispatch Functions
// ============================================================================

/// Accelerated Luma Lt4 filter (bS < 4).
/// # Preconditions
///
/// The scalar twin is stride-agnostic — it addresses in flat byte offsets
/// (`deblocking_common.rs:52`), so any `(step_x, step_y)` pair is meaningful to it.
/// This kernel addresses in 2D through the cursor instead (`row_n::<N>(dy, dx)`), so
/// the direction guard below testing `step_y == 1` / `step_x == 1` is only half the
/// contract: the *other* step must also be the cursor's own stride. A caller passing
/// `step_x = 2 * stride` for field addressing, or building the cursor over a different
/// pitch than the `iStride` it passes, still satisfies `step_y == 1`, so the scalar
/// fallback is not taken and this reads and writes the wrong samples. No current
/// caller violates it; the `debug_assert!` is what keeps that true.
pub fn deblock_luma_lt4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    if step_y == 1 {
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish.
        debug_assert_eq!(step_x, pix.stride() as isize);
        // Horizontal edge: taps step vertically in y (-3, -2, -1, 0, 1, 2)
        let mut p2 = pix.row_n::<16>(-3, 0);
        let mut p1 = pix.row_n::<16>(-2, 0);
        let mut p0 = pix.row_n::<16>(-1, 0);
        let mut q0 = pix.row_n::<16>(0, 0);
        let mut q1 = pix.row_n::<16>(1, 0);
        let mut q2 = pix.row_n::<16>(2, 0);

        unsafe {
            deblock_luma_lt4_16(
                &mut p2, &mut p1, &mut p0, &mut q0, &mut q1, &mut q2, alpha, beta, tc,
            );
        }

        pix.set_row_n::<16>(-2, 0, &p1);
        pix.set_row_n::<16>(-1, 0, &p0);
        pix.set_row_n::<16>(0, 0, &q0);
        pix.set_row_n::<16>(1, 0, &q1);
    } else if step_x == 1 {
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish.
        debug_assert_eq!(step_y, pix.stride() as isize);
        // Vertical edge: 16 rows, line i has taps at row i, cols -4..4
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
        unsafe {
            deblock_luma_lt4_16(
                t1, t2, t3, t4, t5, t6, alpha, beta, tc,
            );
        }

        for x in 2..=5 {
            for y in 0..16 {
                rows[y][x] = t[x][y];
            }
        }

        // **Write back only the columns the filter can modify.** The span read above is
        // wider because the kernel needs the outer taps, but only the inner columns are
        // assigned, and the scalar twin writes exactly those. Storing the whole span
        // would be value-neutral yet widen this kernel's write contract past the scalar
        // it must match — and at `iEdge == 0` the outer columns belong to the previous
        // macroblock.
        for i in 0..16 {
            let seg: &[u8; 4] = rows[i][2..6].try_into().expect("p1..q1");
            pix.set_row_n::<4>(i as isize, -2, seg);
        }
    } else {
        crate::common::deblocking_common::deblock_luma_lt4_scalar(pix, step_x, step_y, alpha, beta, tc);
    }
}

/// Accelerated Luma Eq4 filter (bS == 4).
/// # Preconditions
///
/// The scalar twin is stride-agnostic — it addresses in flat byte offsets
/// (`deblocking_common.rs:52`), so any `(step_x, step_y)` pair is meaningful to it.
/// This kernel addresses in 2D through the cursor instead (`row_n::<N>(dy, dx)`), so
/// the direction guard below testing `step_y == 1` / `step_x == 1` is only half the
/// contract: the *other* step must also be the cursor's own stride. A caller passing
/// `step_x = 2 * stride` for field addressing, or building the cursor over a different
/// pitch than the `iStride` it passes, still satisfies `step_y == 1`, so the scalar
/// fallback is not taken and this reads and writes the wrong samples. No current
/// caller violates it; the `debug_assert!` is what keeps that true.
pub fn deblock_luma_eq4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    if step_y == 1 {
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish.
        debug_assert_eq!(step_x, pix.stride() as isize);
        // Horizontal edge: taps step vertically in y (-4, -3, -2, -1, 0, 1, 2, 3)
        let p3 = pix.row_n::<16>(-4, 0);
        let mut p2 = pix.row_n::<16>(-3, 0);
        let mut p1 = pix.row_n::<16>(-2, 0);
        let mut p0 = pix.row_n::<16>(-1, 0);
        let mut q0 = pix.row_n::<16>(0, 0);
        let mut q1 = pix.row_n::<16>(1, 0);
        let mut q2 = pix.row_n::<16>(2, 0);
        let q3 = pix.row_n::<16>(3, 0);

        unsafe {
            deblock_luma_eq4_16(
                &p3, &mut p2, &mut p1, &mut p0, &mut q0, &mut q1, &mut q2, &q3, alpha, beta,
            );
        }

        pix.set_row_n::<16>(-3, 0, &p2);
        pix.set_row_n::<16>(-2, 0, &p1);
        pix.set_row_n::<16>(-1, 0, &p0);
        pix.set_row_n::<16>(0, 0, &q0);
        pix.set_row_n::<16>(1, 0, &q1);
        pix.set_row_n::<16>(2, 0, &q2);
    } else if step_x == 1 {
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish.
        debug_assert_eq!(step_y, pix.stride() as isize);
        // Vertical edge
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
        unsafe {
            deblock_luma_eq4_16(
                t0, t1, t2, t3, t4, t5, t6, t7,
                alpha, beta,
            );
        }

        for x in 1..=6 {
            for y in 0..16 {
                rows[y][x] = t[x][y];
            }
        }

        // **Write back only the columns the filter can modify.** The span read above is
        // wider because the kernel needs the outer taps, but only the inner columns are
        // assigned, and the scalar twin writes exactly those. Storing the whole span
        // would be value-neutral yet widen this kernel's write contract past the scalar
        // it must match — and at `iEdge == 0` the outer columns belong to the previous
        // macroblock.
        for i in 0..16 {
            let seg: &[u8; 6] = rows[i][1..7].try_into().expect("p2..q2");
            pix.set_row_n::<6>(i as isize, -3, seg);
        }
    } else {
        crate::common::deblocking_common::deblock_luma_eq4_scalar(pix, step_x, step_y, alpha, beta);
    }
}

/// Accelerated Chroma Lt4 filter (bS < 4).
/// # Preconditions
///
/// The scalar twin is stride-agnostic — it addresses in flat byte offsets
/// (`deblocking_common.rs:52`), so any `(step_x, step_y)` pair is meaningful to it.
/// This kernel addresses in 2D through the cursor instead (`row_n::<N>(dy, dx)`), so
/// the direction guard below testing `step_y == 1` / `step_x == 1` is only half the
/// contract: the *other* step must also be the cursor's own stride. A caller passing
/// `step_x = 2 * stride` for field addressing, or building the cursor over a different
/// pitch than the `iStride` it passes, still satisfies `step_y == 1`, so the scalar
/// fallback is not taken and this reads and writes the wrong samples. No current
/// caller violates it; the `debug_assert!` is what keeps that true.
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
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish. Cb and Cr are
        // separate planes, so both are checked.
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

        unsafe {
            deblock_chroma_lt4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta, tc);
        }

        cb_p0.copy_from_slice(&p0[..8]);
        cr_p0.copy_from_slice(&p0[8..]);
        cb_q0.copy_from_slice(&q0[..8]);
        cr_q0.copy_from_slice(&q0[8..]);

        cb.set_row_n::<8>(-1, 0, &cb_p0);
        cr.set_row_n::<8>(-1, 0, &cr_p0);
        cb.set_row_n::<8>(0, 0, &cb_q0);
        cr.set_row_n::<8>(0, 0, &cr_q0);
    } else if step_x == 1 {
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish. Cb and Cr are
        // separate planes, so both are checked.
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
        unsafe {
            deblock_chroma_lt4_16(t0, t1, t2, t3, alpha, beta, tc);
        }

        for y in 0..8 {
            cb_rows[y][1] = t[1][y];
            cb_rows[y][2] = t[2][y];
            cr_rows[y][1] = t[1][y + 8];
            cr_rows[y][2] = t[2][y + 8];
        }

        // **Write back only the columns the filter can modify.** The span read above is
        // wider because the kernel needs the outer taps, but only the inner columns are
        // assigned, and the scalar twin writes exactly those. Storing the whole span
        // would be value-neutral yet widen this kernel's write contract past the scalar
        // it must match — and at `iEdge == 0` the outer columns belong to the previous
        // macroblock.
        for i in 0..8 {
            let cb_seg: &[u8; 2] = cb_rows[i][1..3].try_into().expect("p0, q0");
            let cr_seg: &[u8; 2] = cr_rows[i][1..3].try_into().expect("p0, q0");
            cb.set_row_n::<2>(i as isize, -1, cb_seg);
            cr.set_row_n::<2>(i as isize, -1, cr_seg);
        }
    } else {
        crate::common::deblocking_common::deblock_chroma_lt4_scalar(
            cb, cr, step_x, step_y, alpha, beta, tc,
        );
    }
}

/// Accelerated Chroma Eq4 filter (bS == 4).
/// # Preconditions
///
/// The scalar twin is stride-agnostic — it addresses in flat byte offsets
/// (`deblocking_common.rs:52`), so any `(step_x, step_y)` pair is meaningful to it.
/// This kernel addresses in 2D through the cursor instead (`row_n::<N>(dy, dx)`), so
/// the direction guard below testing `step_y == 1` / `step_x == 1` is only half the
/// contract: the *other* step must also be the cursor's own stride. A caller passing
/// `step_x = 2 * stride` for field addressing, or building the cursor over a different
/// pitch than the `iStride` it passes, still satisfies `step_y == 1`, so the scalar
/// fallback is not taken and this reads and writes the wrong samples. No current
/// caller violates it; the `debug_assert!` is what keeps that true.
pub fn deblock_chroma_eq4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    if step_y == 1 {
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish. Cb and Cr are
        // separate planes, so both are checked.
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

        unsafe {
            deblock_chroma_eq4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta);
        }

        cb_p0.copy_from_slice(&p0[..8]);
        cr_p0.copy_from_slice(&p0[8..]);
        cb_q0.copy_from_slice(&q0[..8]);
        cr_q0.copy_from_slice(&q0[8..]);

        cb.set_row_n::<8>(-1, 0, &cb_p0);
        cr.set_row_n::<8>(-1, 0, &cr_p0);
        cb.set_row_n::<8>(0, 0, &cb_q0);
        cr.set_row_n::<8>(0, 0, &cr_q0);
    } else if step_x == 1 {
        // See the preconditions above: the cross-line step must be this cursor's
        // stride, which the direction guard alone does not establish. Cb and Cr are
        // separate planes, so both are checked.
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
        unsafe {
            deblock_chroma_eq4_16(t0, t1, t2, t3, alpha, beta);
        }

        for y in 0..8 {
            cb_rows[y][1] = t[1][y];
            cb_rows[y][2] = t[2][y];
            cr_rows[y][1] = t[1][y + 8];
            cr_rows[y][2] = t[2][y + 8];
        }

        // **Write back only the columns the filter can modify.** The span read above is
        // wider because the kernel needs the outer taps, but only the inner columns are
        // assigned, and the scalar twin writes exactly those. Storing the whole span
        // would be value-neutral yet widen this kernel's write contract past the scalar
        // it must match — and at `iEdge == 0` the outer columns belong to the previous
        // macroblock.
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
