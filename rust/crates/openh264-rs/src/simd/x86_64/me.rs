//! x86_64 SSE2 implementations of screen-content motion-estimation feature kernels
//! (`codec/encoder/core/x86/sample_sc.asm`).

#![allow(unsafe_code)]

use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{BlockRows, RefSamples};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn sum_window8_epi16(lo: __m128i, hi: __m128i) -> __m128i {
    let s2_lo = _mm_add_epi16(
        lo,
        _mm_or_si128(_mm_srli_si128(lo, 2), _mm_slli_si128(hi, 14)),
    );
    let s2_hi = _mm_add_epi16(hi, _mm_srli_si128(hi, 2));
    let s4_lo = _mm_add_epi16(
        s2_lo,
        _mm_or_si128(_mm_srli_si128(s2_lo, 4), _mm_slli_si128(s2_hi, 12)),
    );
    let s4_hi = _mm_add_epi16(s2_hi, _mm_srli_si128(s2_hi, 4));
    _mm_add_epi16(
        s4_lo,
        _mm_or_si128(_mm_srli_si128(s4_lo, 8), _mm_slli_si128(s4_hi, 8)),
    )
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn sliding_sum8(row16: &[u8]) -> __m128i {
    let r: &[u8; 16] = row16[..16].try_into().expect("16 bytes");
    let zero = _mm_setzero_si128();
    let v = unsafe { _mm_loadu_si128(r.as_ptr() as *const __m128i) };
    let lo = _mm_unpacklo_epi8(v, zero);
    let hi = _mm_unpackhi_epi8(v, zero);
    sum_window8_epi16(lo, hi)
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn sliding_diff8(bot16: &[u8], top16: &[u8]) -> __m128i {
    let b_arr: &[u8; 16] = bot16[..16].try_into().expect("16 bytes");
    let t_arr: &[u8; 16] = top16[..16].try_into().expect("16 bytes");
    let zero = _mm_setzero_si128();
    let b = unsafe { _mm_loadu_si128(b_arr.as_ptr() as *const __m128i) };
    let t = unsafe { _mm_loadu_si128(t_arr.as_ptr() as *const __m128i) };
    let lo = _mm_sub_epi16(_mm_unpacklo_epi8(b, zero), _mm_unpacklo_epi8(t, zero));
    let hi = _mm_sub_epi16(_mm_unpackhi_epi8(b, zero), _mm_unpackhi_epi8(t, zero));
    sum_window8_epi16(lo, hi)
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn ld8_u16(r: &[u16]) -> __m128i {
    let r: &[u16; 8] = r[..8].try_into().expect("8 u16s");
    unsafe { _mm_loadu_si128(r.as_ptr() as *const __m128i) }
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn st8_u16(out: &mut [u16], v: __m128i) {
    let out: &mut [u16; 8] = (&mut out[..8]).try_into().expect("8 u16s");
    unsafe { _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, v) }
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn sum_8x8_single(cRef: &RecCursor<'_>) -> i32 {
    unsafe {
        let s = cRef.span::<8, 8>(0, 0);
        let zero = _mm_setzero_si128();
        let mut acc = _mm_setzero_si128();
        for y in 0..8 {
            let r = s.row::<8>(y, 0);
            let v = _mm_loadl_epi64(r.as_ptr() as *const __m128i);
            acc = _mm_add_epi64(acc, _mm_sad_epu8(v, zero));
        }
        _mm_cvtsi128_si32(acc)
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "sse2")]
fn sum_16x16_single(cRef: &RecCursor<'_>) -> i32 {
    unsafe {
        let s = cRef.span::<16, 16>(0, 0);
        let zero = _mm_setzero_si128();
        let mut acc = _mm_setzero_si128();
        for y in 0..16 {
            let r = s.row::<16>(y, 0);
            let v = _mm_loadu_si128(r.as_ptr() as *const __m128i);
            acc = _mm_add_epi64(acc, _mm_sad_epu8(v, zero));
        }
        let hi = _mm_srli_si128(acc, 8);
        _mm_cvtsi128_si32(_mm_add_epi32(acc, hi))
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
fn sum_8x8_frame(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    if kiWidth <= 0 || kiHeight <= 0 {
        return;
    }
    let width = kiWidth as usize;
    let height = kiHeight as usize;
    let stride = kiRefStride as usize;

    // Row 0
    {
        let row0_feat = &mut pFeatureOfBlock[..width];
        let mut x = 0usize;
        while x + 8 < width {
            let mut acc = sliding_sum8(&kpRefPicture[x..]);
            for r in 1..8 {
                acc = _mm_add_epi16(acc, sliding_sum8(&kpRefPicture[r * stride + x..]));
            }
            st8_u16(&mut row0_feat[x..x + 8], acc);
            for &s in &row0_feat[x..x + 8] {
                pTimesOfFeatureValue[s as usize] += 1;
            }
            x += 8;
        }
        let mut sum = if x == 0 {
            let mut s = 0i32;
            for r in 0..8 {
                let row = &kpRefPicture[r * stride..r * stride + 8];
                for &b in row {
                    s += b as i32;
                }
            }
            row0_feat[0] = s as u16;
            pTimesOfFeatureValue[s as usize] += 1;
            x = 1;
            s
        } else {
            row0_feat[x - 1] as i32
        };
        for tx in x..width {
            for r in 0..8 {
                let row = &kpRefPicture[r * stride..];
                sum += row[tx + 7] as i32 - row[tx - 1] as i32;
            }
            let s = sum as u16;
            row0_feat[tx] = s;
            pTimesOfFeatureValue[s as usize] += 1;
        }
    }

    // Subsequent rows y = 1..height
    for y in 1..height {
        let (prev_part, cur_part) = pFeatureOfBlock.split_at_mut(y * width);
        let prev_row = &prev_part[(y - 1) * width..y * width];
        let cur_row = &mut cur_part[..width];
        let top_row = &kpRefPicture[(y - 1) * stride..(y - 1) * stride + width + 7];
        let bot_row = &kpRefPicture[(y + 7) * stride..(y + 7) * stride + width + 7];

        let mut x = 0usize;
        while x + 8 < width {
            let prev = ld8_u16(&prev_row[x..x + 8]);
            let diff = sliding_diff8(&bot_row[x..x + 16], &top_row[x..x + 16]);
            let cur = _mm_add_epi16(prev, diff);
            st8_u16(&mut cur_row[x..x + 8], cur);
            for &s in &cur_row[x..x + 8] {
                pTimesOfFeatureValue[s as usize] += 1;
            }
            x += 8;
        }

        let mut diff_sum = if x == 0 {
            let mut d = 0i32;
            for c in 0..8 {
                d += bot_row[c] as i32 - top_row[c] as i32;
            }
            let s = (prev_row[0] as i32 + d) as u16;
            cur_row[0] = s;
            pTimesOfFeatureValue[s as usize] += 1;
            x = 1;
            d
        } else {
            cur_row[x - 1] as i32 - prev_row[x - 1] as i32
        };

        for tx in x..width {
            diff_sum += (bot_row[tx + 7] as i32 - top_row[tx + 7] as i32)
                - (bot_row[tx - 1] as i32 - top_row[tx - 1] as i32);
            let s = (prev_row[tx] as i32 + diff_sum) as u16;
            cur_row[tx] = s;
            pTimesOfFeatureValue[s as usize] += 1;
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
fn sum_16x16_frame(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    if kiWidth <= 0 || kiHeight <= 0 {
        return;
    }
    let width = kiWidth as usize;
    let height = kiHeight as usize;
    let stride = kiRefStride as usize;

    // Row 0
    {
        let row0_feat = &mut pFeatureOfBlock[..width];
        let mut sum = 0i32;
        for r in 0..16 {
            let row = &kpRefPicture[r * stride..r * stride + 16];
            for &b in row {
                sum += b as i32;
            }
        }
        row0_feat[0] = sum as u16;
        pTimesOfFeatureValue[sum as usize] += 1;

        for tx in 1..width {
            for r in 0..16 {
                let row = &kpRefPicture[r * stride..];
                sum += row[tx + 15] as i32 - row[tx - 1] as i32;
            }
            let s = sum as u16;
            row0_feat[tx] = s;
            pTimesOfFeatureValue[s as usize] += 1;
        }
    }

    // Subsequent rows y = 1..height
    for y in 1..height {
        let (prev_part, cur_part) = pFeatureOfBlock.split_at_mut(y * width);
        let prev_row = &prev_part[(y - 1) * width..y * width];
        let cur_row = &mut cur_part[..width];
        let top_row = &kpRefPicture[(y - 1) * stride..(y - 1) * stride + width + 15];
        let bot_row = &kpRefPicture[(y + 15) * stride..(y + 15) * stride + width + 15];

        let mut x = 0usize;
        if x + 8 < width {
            let mut curr_diff8 = sliding_diff8(&bot_row[0..16], &top_row[0..16]);
            while x + 8 < width {
                let next_diff8 = sliding_diff8(&bot_row[x + 8..x + 24], &top_row[x + 8..x + 24]);
                let diff16 = _mm_add_epi16(curr_diff8, next_diff8);
                let prev = ld8_u16(&prev_row[x..x + 8]);
                let cur = _mm_add_epi16(prev, diff16);
                st8_u16(&mut cur_row[x..x + 8], cur);
                for &s in &cur_row[x..x + 8] {
                    pTimesOfFeatureValue[s as usize] += 1;
                }
                curr_diff8 = next_diff8;
                x += 8;
            }
        }

        let mut diff_sum = if x == 0 {
            let mut d = 0i32;
            for c in 0..16 {
                d += bot_row[c] as i32 - top_row[c] as i32;
            }
            let s = (prev_row[0] as i32 + d) as u16;
            cur_row[0] = s;
            pTimesOfFeatureValue[s as usize] += 1;
            x = 1;
            d
        } else {
            cur_row[x - 1] as i32 - prev_row[x - 1] as i32
        };

        for tx in x..width {
            diff_sum += (bot_row[tx + 15] as i32 - top_row[tx + 15] as i32)
                - (bot_row[tx - 1] as i32 - top_row[tx - 1] as i32);
            let s = (prev_row[tx] as i32 + diff_sum) as u16;
            cur_row[tx] = s;
            pTimesOfFeatureValue[s as usize] += 1;
        }
    }
}

#[inline]
pub fn sum_of_8x8_single_block(cRef: &RecCursor<'_>) -> i32 {
    unsafe { sum_8x8_single(cRef) }
}

#[inline]
pub fn sum_of_16x16_single_block(cRef: &RecCursor<'_>) -> i32 {
    unsafe { sum_16x16_single(cRef) }
}

#[inline]
pub fn sum_of_8x8_block_of_frame(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    unsafe {
        sum_8x8_frame(
            kpRefPicture,
            kiWidth,
            kiHeight,
            kiRefStride,
            pFeatureOfBlock,
            pTimesOfFeatureValue,
        )
    }
}

#[inline]
pub fn sum_of_16x16_block_of_frame(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    unsafe {
        sum_16x16_frame(
            kpRefPicture,
            kiWidth,
            kiHeight,
            kiRefStride,
            pFeatureOfBlock,
            pTimesOfFeatureValue,
        )
    }
}
