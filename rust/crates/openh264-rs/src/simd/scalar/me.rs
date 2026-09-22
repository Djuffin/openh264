//! Scalar forwards for the motion-estimation feature kernels.

use crate::encoder::rec_view::RecCursor;
use crate::encoder::svc_motion_estimate;

#[inline(always)]
pub fn sum_of_8x8_single_block(cRef: &RecCursor<'_>) -> i32 {
    svc_motion_estimate::sum_of_8x8_single_block(cRef)
}

#[inline(always)]
pub fn sum_of_16x16_single_block(cRef: &RecCursor<'_>) -> i32 {
    svc_motion_estimate::sum_of_16x16_single_block(cRef)
}

#[inline(always)]
pub fn sum_of_8x8_block_of_frame(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    svc_motion_estimate::SumOf8x8BlockOfFrame_c(
        kpRefPicture,
        kiWidth,
        kiHeight,
        kiRefStride,
        pFeatureOfBlock,
        pTimesOfFeatureValue,
    );
}

#[inline(always)]
pub fn sum_of_16x16_block_of_frame(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    svc_motion_estimate::SumOf16x16BlockOfFrame_c(
        kpRefPicture,
        kiWidth,
        kiHeight,
        kiRefStride,
        pFeatureOfBlock,
        pTimesOfFeatureValue,
    );
}
