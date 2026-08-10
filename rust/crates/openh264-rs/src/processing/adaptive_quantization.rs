#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/adaptivequantization/AdaptiveQuantization.cpp` —
//! the plugin reached through `METHOD_ADAPTIVE_QUANT`.
//!
//! `CWelsPreProcess::AnalyzeSpatialPic` calls it for every P slice when
//! `bEnableAdaptiveQuant` is set, which `FillDefault` leaves **on**. It writes a
//! per-macroblock QP delta into `pMotionTextureIndexToDeltaQp`, which
//! `RcCalculateMbQp` adds to the slice QP, and a frame average into
//! `iAverMotionTextureIndexToDeltaQp`, which `RcCalculatePictureQp` and
//! `WelsRcPictureInitDisable` subtract from the global QP.
//!
//! ## Integer widths are load-bearing
//!
//! `SampleVariance16x16_c` accumulates the two sums in `uint16_t` and the two
//! squares in `uint32_t`. Both are `u16` here with `wrapping_add`, faithfully —
//! but **neither sum can actually reach the wrap**: a 16x16 block is 256 samples of
//! at most 255, so both top out at 65280, 255 short of `uint16_t`'s range. (An
//! earlier version of this note claimed the difference sum could wrap. It cannot;
//! it has exactly the same bound as `uiCurSum`, and
//! `sample_variance_16x16_accumulators_cannot_wrap` in
//! `tests/kernels_differential_phase2.rs` drives both extremes to pin it.) The
//! `wrapping_add`s stay regardless, because reproducing the C++'s declared widths
//! is the rule, not reproducing only the widths that turn out to matter.
//!
//! The products `uiSum * uiSum` and `uiCurSum * uiCurSum` are `int` in C++ (integer
//! promotion of `uint16_t`), and the result is stored back into a `uint16_t` field,
//! so the truncation happens at the store.

use crate::encoder::wels_preprocess::{SAdaptiveQuantizationParam, SMotionTextureUnit, SPixMap};
use core::ffi::c_void;

use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};

/// `util.h:61-64`.
const AQ_INT_MULTIPLY: i64 = 10000000;
const AQ_TIME_INT_MULTIPLY: i64 = 10000;
const AQ_QSTEP_INT_MULTIPLY: i64 = 100;
const AQ_PESN: i64 = 10;

/// `AdaptiveQuantization.cpp:38-42`.
const AVERAGE_TIME_MOTION: i64 = 3000;
const AVERAGE_TIME_TEXTURE_QUALITYMODE: i64 = 10000;
const AVERAGE_TIME_TEXTURE_BITRATEMODE: i64 = 8750;
const MODEL_ALPHA: i64 = 9910;
const MODEL_TIME: i64 = 58185;

/// `MB_WIDTH_LUMA` — `wels_const_common.h:50`.
const MB_WIDTH_LUMA: i32 = 16;

/// `EAQModes` — `IWelsVP.h:198`.
pub const AQ_QUALITY_MODE: i32 = 0;
pub const AQ_BITRATE_MODE: i32 = 1;

#[inline]
fn WELS_DIV_ROUND64(x: i64, y: i64) -> i64 {
    if y == 0 {
        x / (y + 1)
    } else {
        ((y / 2) + x) / y
    }
}

/// `SampleVariance16x16_c` — `AdaptiveQuantization.cpp:245`.
///
/// # Safety
/// Both planes must have at least 16 readable rows of 16 bytes at the given
/// strides; `pMotionTexture` must be writable.
pub unsafe fn SampleVariance16x16_c(
    pRefY: *const u8,
    iRefStride: i32,
    pSrcY: *const u8,
    iSrcStride: i32,
    pMotionTexture: *mut SMotionTextureUnit,
) {
    let mut uiCurSquare: u32 = 0;
    let mut uiSquare: u32 = 0;
    let mut uiCurSum: u16 = 0;
    let mut uiSum: u16 = 0;

    let mut pRef = pRefY;
    let mut pSrc = pSrcY;
    for _y in 0..MB_WIDTH_LUMA {
        for x in 0..MB_WIDTH_LUMA as isize {
            let src = *pSrc.offset(x);
            let uiDiff = (*pRef.offset(x) as i32 - src as i32).unsigned_abs();
            uiSum = uiSum.wrapping_add(uiDiff as u16);
            uiSquare = uiSquare.wrapping_add(uiDiff.wrapping_mul(uiDiff));

            uiCurSum = uiCurSum.wrapping_add(src as u16);
            uiCurSquare = uiCurSquare.wrapping_add((src as u32) * (src as u32));
        }
        pRef = pRef.offset(iRefStride as isize);
        pSrc = pSrc.offset(iSrcStride as isize);
    }

    // `uiSum * uiSum` promotes to `int` in C++ and the store back into the
    // `uint16_t` field truncates.
    uiSum >>= 8;
    (*pMotionTexture).uiMotionIndex =
        ((uiSquare >> 8) as i32 - (uiSum as i32) * (uiSum as i32)) as u16;

    uiCurSum >>= 8;
    (*pMotionTexture).uiTextureIndex =
        ((uiCurSquare >> 8) as i32 - (uiCurSum as i32) * (uiCurSum as i32)) as u16;
}

//=================== Safe kernels =====================//

/// The bytes [`sample_variance_16x16`] reads from one plane, counted from the
/// macroblock's top-left sample.
///
/// Sixteen rows of sixteen samples reaching forward only, so the last sample sits
/// `15 * stride + 15` past the origin and the block needs no padding in any
/// direction. The two shims size their slices with this and nothing else does the
/// arithmetic; `tests/kernels_differential_phase2.rs` pins it.
pub fn mb_span(stride: usize) -> usize {
    15 * stride + 16
}

/// C++: `SampleVariance16x16_c`, `AdaptiveQuantization.cpp:245`.
///
/// The variance proxies for one macroblock: `uiMotionIndex` from the difference
/// against the reference, `uiTextureIndex` from the current picture alone.
///
/// **Arithmetic parity is the whole difficulty here** (plan §7.4 / the R-e rule). The
/// C++ accumulates the two sums in `uint16_t` and the two squares in `uint32_t`, so
/// `uiSum` and `uiCurSum` genuinely wrap at 65536 — `uiCurSum` reaches 65280 for a
/// block of maximum-brightness samples and stops just short, while the difference sum
/// can go over. The products then promote to `int` and truncate at the store into the
/// `uint16_t` field. Every one of those widths is reproduced below, wrap for wrap:
/// nothing is widened, nothing is clamped, and the `wrapping_*` calls are the ones the
/// old port already carried. Repairing this belongs to whoever owns the C++'s
/// arithmetic, not to a conversion.
pub fn sample_variance_16x16(
    refy: &[u8],
    ref_stride: usize,
    srcy: &[u8],
    src_stride: usize,
) -> SMotionTextureUnit {
    let mut cur_square: u32 = 0;
    let mut square: u32 = 0;
    let mut cur_sum: u16 = 0;
    let mut sum: u16 = 0;

    for y in 0..MB_WIDTH_LUMA as usize {
        let ref_base = y * ref_stride;
        let src_base = y * src_stride;
        // Fixed-size windows: the sixteen-sample inner loop carries no bounds check,
        // and the two range checks land once per row.
        let r: &[u8; 16] = refy[ref_base..ref_base + 16].try_into().unwrap();
        let s: &[u8; 16] = srcy[src_base..src_base + 16].try_into().unwrap();
        for (&rv, &sv) in r.iter().zip(s.iter()) {
            let diff = (rv as i32 - sv as i32).unsigned_abs();
            sum = sum.wrapping_add(diff as u16);
            square = square.wrapping_add(diff.wrapping_mul(diff));

            cur_sum = cur_sum.wrapping_add(sv as u16);
            cur_square = cur_square.wrapping_add((sv as u32) * (sv as u32));
        }
    }

    // `uiSum * uiSum` promotes to `int` in C++ and the store back into the
    // `uint16_t` field truncates.
    sum >>= 8;
    cur_sum >>= 8;
    SMotionTextureUnit {
        uiMotionIndex: ((square >> 8) as i32 - (sum as i32) * (sum as i32)) as u16,
        uiTextureIndex: ((cur_square >> 8) as i32 - (cur_sum as i32) * (cur_sum as i32)) as u16,
    }
}

/// `CAdaptiveQuantization` — `AdaptiveQuantization.h`.
pub struct CAdaptiveQuantization {
    pub m_sAdaptiveQuantParam: SAdaptiveQuantizationParam,
}

impl Default for CAdaptiveQuantization {
    fn default() -> Self {
        Self {
            m_sAdaptiveQuantParam: SAdaptiveQuantizationParam::default(),
        }
    }
}

impl CAdaptiveQuantization {
    /// `CAdaptiveQuantization::Set`.
    ///
    /// # Safety
    /// `pParam` must point at an `SAdaptiveQuantizationParam`.
    pub unsafe fn Set(&mut self, _iType: i32, pParam: *mut c_void) -> i32 {
        if pParam.is_null() {
            return RET_INVALIDPARAM;
        }
        self.m_sAdaptiveQuantParam = *(pParam as *const SAdaptiveQuantizationParam);
        RET_SUCCESS
    }

    /// `CAdaptiveQuantization::Get` — writes back only the frame average.
    ///
    /// # Safety
    /// `pParam` must point at an `SAdaptiveQuantizationParam`.
    pub unsafe fn Get(&mut self, _iType: i32, pParam: *mut c_void) -> i32 {
        if pParam.is_null() {
            return RET_INVALIDPARAM;
        }
        (*(pParam as *mut SAdaptiveQuantizationParam)).iAverMotionTextureIndexToDeltaQp =
            self.m_sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp;
        RET_SUCCESS
    }

    /// `CAdaptiveQuantization::Process` — `AdaptiveQuantization.cpp:57`.
    ///
    /// # Safety
    /// The pointers stored by the preceding [`Set`](Self::Set) must still be valid,
    /// and both pixel maps must describe readable luma planes.
    pub unsafe fn Process(
        &mut self,
        _iType: i32,
        pSrcPixMap: *mut SPixMap,
        pRefPixMap: *mut SPixMap,
    ) -> i32 {
        let iWidth = (*pSrcPixMap).sRect.iRectWidth;
        let iHeight = (*pSrcPixMap).sRect.iRectHeight;
        let iMbWidth = iWidth >> 4;
        let iMbHeight = iHeight >> 4;
        let iMbTotalNum = iMbWidth * iMbHeight;

        let mut iAverageMotionIndex: i64 = 0;
        let mut iAverageTextureIndex: i64 = 0;

        let mut pRefFrameY = (*pRefPixMap).pPixel[0] as *const u8;
        let mut pCurFrameY = (*pSrcPixMap).pPixel[0] as *const u8;
        let iRefStride = (*pRefPixMap).iStride[0];
        let iCurStride = (*pSrcPixMap).iStride[0];

        let mut pMotionTexture = self.m_sAdaptiveQuantParam.pMotionTextureUnit;
        let pVaaCalcResults = self.m_sAdaptiveQuantParam.pCalcResult;

        // Reuse the VAA statistics when they were computed over exactly this pair
        // of pictures; otherwise recompute per macroblock.
        if (*pVaaCalcResults).pRefY as *const u8 == pRefFrameY
            && (*pVaaCalcResults).pCurY as *const u8 == pCurFrameY
        {
            let mut iMbIndex = 0isize;
            for _j in 0..iMbHeight {
                for _i in 0..iMbWidth {
                    let sad8x8 = &*(*pVaaCalcResults).pSad8x8.offset(iMbIndex);
                    let mut iSumDiff = sad8x8[0];
                    iSumDiff += sad8x8[1];
                    iSumDiff += sad8x8[2];
                    iSumDiff += sad8x8[3];

                    let iSQDiff = *(*pVaaCalcResults).pSsd16x16.offset(iMbIndex);
                    let mut uiSum = *(*pVaaCalcResults).pSum16x16.offset(iMbIndex);
                    let iSQSum = *(*pVaaCalcResults).pSumOfSquare16x16.offset(iMbIndex);

                    // Every one of these is `int32_t` in C++ and the result is
                    // stored into a `uint16_t` field, so the truncation is at the
                    // store, not at the arithmetic.
                    iSumDiff >>= 8;
                    (*pMotionTexture).uiMotionIndex = ((iSQDiff >> 8) - iSumDiff * iSumDiff) as u16;

                    uiSum >>= 8;
                    (*pMotionTexture).uiTextureIndex = ((iSQSum >> 8) - uiSum * uiSum) as u16;

                    iAverageMotionIndex += (*pMotionTexture).uiMotionIndex as i64;
                    iAverageTextureIndex += (*pMotionTexture).uiTextureIndex as i64;
                    pMotionTexture = pMotionTexture.add(1);
                    iMbIndex += 1;
                }
            }
        } else {
            for _j in 0..iMbHeight {
                let mut pRefFrameTmp = pRefFrameY;
                let mut pCurFrameTmp = pCurFrameY;
                for _i in 0..iMbWidth {
                    SampleVariance16x16_c(
                        pRefFrameTmp,
                        iRefStride,
                        pCurFrameTmp,
                        iCurStride,
                        pMotionTexture,
                    );
                    iAverageMotionIndex += (*pMotionTexture).uiMotionIndex as i64;
                    iAverageTextureIndex += (*pMotionTexture).uiTextureIndex as i64;
                    pMotionTexture = pMotionTexture.add(1);
                    pRefFrameTmp = pRefFrameTmp.offset(MB_WIDTH_LUMA as isize);
                    pCurFrameTmp = pCurFrameTmp.offset(MB_WIDTH_LUMA as isize);
                }
                pRefFrameY = pRefFrameY.offset((iRefStride << 4) as isize);
                pCurFrameY = pCurFrameY.offset((iCurStride << 4) as isize);
            }
        }

        iAverageMotionIndex =
            WELS_DIV_ROUND64(iAverageMotionIndex * AQ_INT_MULTIPLY, iMbTotalNum as i64);
        iAverageTextureIndex =
            WELS_DIV_ROUND64(iAverageTextureIndex * AQ_INT_MULTIPLY, iMbTotalNum as i64);
        if iAverageMotionIndex <= AQ_PESN && iAverageMotionIndex >= -AQ_PESN {
            iAverageMotionIndex = AQ_INT_MULTIPLY;
        }
        if iAverageTextureIndex <= AQ_PESN && iAverageTextureIndex >= -AQ_PESN {
            iAverageTextureIndex = AQ_INT_MULTIPLY;
        }

        let mut iAverMotionTextureIndexToDeltaQp: i32 = 0;
        iAverageMotionIndex = WELS_DIV_ROUND64(
            AVERAGE_TIME_MOTION * iAverageMotionIndex,
            AQ_TIME_INT_MULTIPLY,
        );

        iAverageTextureIndex = if self.m_sAdaptiveQuantParam.iAdaptiveQuantMode == AQ_QUALITY_MODE {
            WELS_DIV_ROUND64(
                AVERAGE_TIME_TEXTURE_QUALITYMODE * iAverageTextureIndex,
                AQ_TIME_INT_MULTIPLY,
            )
        } else {
            WELS_DIV_ROUND64(
                AVERAGE_TIME_TEXTURE_BITRATEMODE * iAverageTextureIndex,
                AQ_TIME_INT_MULTIPLY,
            )
        };

        let iAQ_EPSN: i64 =
            -(AQ_PESN * AQ_TIME_INT_MULTIPLY * AQ_QSTEP_INT_MULTIPLY / AQ_INT_MULTIPLY);
        let mut pMotionTexture = self.m_sAdaptiveQuantParam.pMotionTextureUnit;
        for j in 0..iMbHeight {
            for i in 0..iMbWidth {
                let mut a = WELS_DIV_ROUND64(
                    (*pMotionTexture).uiTextureIndex as i64 * AQ_INT_MULTIPLY * AQ_TIME_INT_MULTIPLY,
                    iAverageTextureIndex,
                );
                let mut iQStep = WELS_DIV_ROUND64(
                    (a - AQ_TIME_INT_MULTIPLY) * AQ_QSTEP_INT_MULTIPLY,
                    a + MODEL_ALPHA,
                );
                let iLumaTextureDeltaQp = MODEL_TIME * iQStep; // range +- 6

                let mut iMotionTextureIndexToDeltaQp =
                    (iLumaTextureDeltaQp / AQ_TIME_INT_MULTIPLY) as i32;

                a = WELS_DIV_ROUND64(
                    (*pMotionTexture).uiMotionIndex as i64 * AQ_INT_MULTIPLY * AQ_TIME_INT_MULTIPLY,
                    iAverageMotionIndex,
                );
                iQStep = WELS_DIV_ROUND64(
                    (a - AQ_TIME_INT_MULTIPLY) * AQ_QSTEP_INT_MULTIPLY,
                    a + MODEL_ALPHA,
                );
                let iLumaMotionDeltaQp = MODEL_TIME * iQStep; // range +- 6

                if (self.m_sAdaptiveQuantParam.iAdaptiveQuantMode == AQ_QUALITY_MODE
                    && iLumaMotionDeltaQp < iAQ_EPSN)
                    || self.m_sAdaptiveQuantParam.iAdaptiveQuantMode == AQ_BITRATE_MODE
                {
                    iMotionTextureIndexToDeltaQp +=
                        (iLumaMotionDeltaQp / AQ_TIME_INT_MULTIPLY) as i32;
                }

                *self
                    .m_sAdaptiveQuantParam
                    .pMotionTextureIndexToDeltaQp
                    .offset((j * iMbWidth + i) as isize) =
                    (iMotionTextureIndexToDeltaQp as i64 / AQ_QSTEP_INT_MULTIPLY) as i8;
                iAverMotionTextureIndexToDeltaQp += iMotionTextureIndexToDeltaQp;
                pMotionTexture = pMotionTexture.add(1);
            }
        }

        self.m_sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp =
            iAverMotionTextureIndexToDeltaQp / iMbTotalNum;

        RET_SUCCESS
    }
}
