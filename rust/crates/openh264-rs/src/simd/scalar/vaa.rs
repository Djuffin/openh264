//! Scalar forwards for the `vaa` kernels — see the module header.

#[inline(always)]
pub fn vaa_calc_sad(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
) -> i32 {
    crate::processing::vaacalc::vaa_calc_sad(cur, refp, pic_width, pic_height, pic_stride, sad8x8)
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_var(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
) -> i32 {
    crate::processing::vaacalc::vaa_calc_sad_var(
        cur, refp, pic_width, pic_height, pic_stride, sad8x8, sum16x16, sqsum16x16,
    )
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_ssd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
    sqdiff16x16: &mut [i32],
) -> i32 {
    crate::processing::vaacalc::vaa_calc_sad_ssd(
        cur,
        refp,
        pic_width,
        pic_height,
        pic_stride,
        sad8x8,
        sum16x16,
        sqsum16x16,
        sqdiff16x16,
    )
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_bgd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sd8x8: &mut [[i32; 4]],
    mad8x8: &mut [[u8; 4]],
) -> i32 {
    crate::processing::vaacalc::vaa_calc_sad_bgd(
        cur, refp, pic_width, pic_height, pic_stride, sad8x8, sd8x8, mad8x8,
    )
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn vaa_calc_sad_ssd_bgd(
    cur: &[u8],
    refp: &[u8],
    pic_width: i32,
    pic_height: i32,
    pic_stride: i32,
    sad8x8: &mut [[i32; 4]],
    sum16x16: &mut [i32],
    sqsum16x16: &mut [i32],
    sqdiff16x16: &mut [i32],
    sd8x8: &mut [[i32; 4]],
    mad8x8: &mut [[u8; 4]],
) -> i32 {
    crate::processing::vaacalc::vaa_calc_sad_ssd_bgd(
        cur,
        refp,
        pic_width,
        pic_height,
        pic_stride,
        sad8x8,
        sum16x16,
        sqsum16x16,
        sqdiff16x16,
        sd8x8,
        mad8x8,
    )
}
