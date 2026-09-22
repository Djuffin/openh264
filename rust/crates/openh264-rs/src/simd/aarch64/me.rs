//! Screen-content motion-estimation feature kernels — `SumOf8x8SingleBlock_AArch64_neon`,
//! `SumOf16x16SingleBlock_AArch64_neon`, `SumOf8x8BlockOfFrame_AArch64_neon`, and
//! `SumOf16x16BlockOfFrame_AArch64_neon`
//! (`codec/encoder/core/arm64/svc_motion_estimation_aarch64_neon.S`).

#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld8, ld8_u16, ld16, st8_u16};
use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{BlockRows, RefSamples};

#[inline]
#[target_feature(enable = "neon")]
fn sum_window8_u16(lo: uint16x8_t, hi: uint16x8_t) -> uint16x8_t {
    let s2_lo = vaddq_u16(lo, vextq_u16(lo, hi, 1));
    let s2_hi = vaddq_u16(hi, vextq_u16(hi, hi, 1));
    let s4_lo = vaddq_u16(s2_lo, vextq_u16(s2_lo, s2_hi, 2));
    let s4_hi = vaddq_u16(s2_hi, vextq_u16(s2_hi, s2_hi, 2));
    vaddq_u16(s4_lo, vextq_u16(s4_lo, s4_hi, 4))
}

#[inline]
#[target_feature(enable = "neon")]
fn sliding_sum8(row16: &[u8]) -> uint16x8_t {
    let r = ld16(row16);
    let lo = vmovl_u8(vget_low_u8(r));
    let hi = vmovl_high_u8(r);
    sum_window8_u16(lo, hi)
}

#[inline]
#[target_feature(enable = "neon")]
fn sliding_diff8(bot16: &[u8], top16: &[u8]) -> uint16x8_t {
    let b = ld16(bot16);
    let t = ld16(top16);
    let lo = vsubl_u8(vget_low_u8(b), vget_low_u8(t));
    let hi = vsubl_high_u8(b, t);
    sum_window8_u16(lo, hi)
}

#[inline]
#[target_feature(enable = "neon")]
fn sum_8x8_single(cRef: &RecCursor<'_>) -> i32 {
    let s = cRef.span::<8, 8>(0, 0);
    let mut acc = vaddl_u8(ld8(&s.row::<8>(0, 0)), ld8(&s.row::<8>(1, 0)));
    acc = vaddw_u8(acc, ld8(&s.row::<8>(2, 0)));
    acc = vaddw_u8(acc, ld8(&s.row::<8>(3, 0)));
    acc = vaddw_u8(acc, ld8(&s.row::<8>(4, 0)));
    acc = vaddw_u8(acc, ld8(&s.row::<8>(5, 0)));
    acc = vaddw_u8(acc, ld8(&s.row::<8>(6, 0)));
    acc = vaddw_u8(acc, ld8(&s.row::<8>(7, 0)));
    vaddlvq_u16(acc) as i32
}

#[inline]
#[target_feature(enable = "neon")]
fn sum_16x16_single(cRef: &RecCursor<'_>) -> i32 {
    let s = cRef.span::<16, 16>(0, 0);
    let mut acc0 = vpaddlq_u8(ld16(&s.row::<16>(0, 0)));
    let mut acc1 = vpaddlq_u8(ld16(&s.row::<16>(1, 0)));
    let mut y = 2usize;
    while y < 16 {
        acc0 = vpadalq_u8(acc0, ld16(&s.row::<16>(y, 0)));
        acc1 = vpadalq_u8(acc1, ld16(&s.row::<16>(y + 1, 0)));
        y += 2;
    }
    vaddlvq_u16(vaddq_u16(acc0, acc1)) as i32
}

#[target_feature(enable = "neon")]
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
                acc = vaddq_u16(acc, sliding_sum8(&kpRefPicture[r * stride + x..]));
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
            let cur = vaddq_u16(prev, diff);
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

#[target_feature(enable = "neon")]
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
                let diff16 = vaddq_u16(curr_diff8, next_diff8);
                let prev = ld8_u16(&prev_row[x..x + 8]);
                let cur = vaddq_u16(prev, diff16);
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
    // SAFETY: NEON is part of the AArch64 baseline.
    unsafe { sum_8x8_single(cRef) }
}

#[inline]
pub fn sum_of_16x16_single_block(cRef: &RecCursor<'_>) -> i32 {
    // SAFETY: NEON is part of the AArch64 baseline.
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
    // SAFETY: NEON is part of the AArch64 baseline.
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
    // SAFETY: NEON is part of the AArch64 baseline.
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
