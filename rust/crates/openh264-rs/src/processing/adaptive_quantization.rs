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

#![forbid(unsafe_code)]

use crate::encoder::wels_preprocess::{SAdaptiveQuantizationParam, SMotionTextureUnit, SPixMap, SVAACalcResult};

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
/// * `pRefY` and `pSrcY` each point at the top-left sample of a 16x16 macroblock in
///   a luma plane whose rows are `iRefStride` / `iSrcStride` bytes apart, with at
///   least `mb_span(stride)` = `15 * stride + 16` readable bytes from there. The
///   reach is strictly forward — sixteen rows of sixteen samples, no row above and
///   no column left — so this shim needs no padding constant to justify itself.
/// * **The two strides are independent.** `CAdaptiveQuantization::Process` walks the
///   reference and source pictures with separate strides taken from separate
///   `SPixMap`s, and they are not equal in general; each plane's span is computed
///   from its own.
/// * `pMotionTexture` is writable.
// `SampleVariance16x16_c` stood here — the raw-pointer shim over
// [`sample_variance_16x16`]. **S11.43, deleted: its last production caller
// (`Process`'s macroblock walk) drives the safe kernel directly**, and the
// span-instrument test whose subject it was went with it — the property it
// pinned (the shim's two declared spans) is the kernel's slice types now.

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
    /// `CAdaptiveQuantization::Set`. Typed since Phase 6 session B (the `IWelsVP`
    /// vtable's `void*` is gone).
    pub fn Set(&mut self, param: &SAdaptiveQuantizationParam) -> i32 {
        self.m_sAdaptiveQuantParam = *param;
        RET_SUCCESS
    }

    /// `CAdaptiveQuantization::Get` — writes back only the frame average.
    pub fn Get(&self, param: &mut SAdaptiveQuantizationParam) -> i32 {
        param.iAverMotionTextureIndexToDeltaQp = self.m_sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp;
        RET_SUCCESS
    }

    /// `CAdaptiveQuantization::Process` — `AdaptiveQuantization.cpp:57`. `calc` is
    /// the VAA statistics of this picture pair, handed over at the call (the C++
    /// stored `pCalcResult` in the parameter block; take what you reach).
    ///
    /// S11.43: the safety section is a signature now — the planes arrive as
    /// borrows and `calc`'s arrays bound every read.
    pub fn Process(
        &mut self,
        pSrcPixMap: &SPixMap,
        _pRefPixMap: &SPixMap,
        // S11.43: the two luma planes as borrows (`ScdPlanes`' shape); the pixel
        // maps carry geometry only.
        planes: crate::processing::vaacalc::VaaCalcPlanes<'_>,
        calc: &SVAACalcResult,
        pMotionTexture: &mut [SMotionTextureUnit],
        pMotionTextureIndexToDeltaQp: &mut [i8],
    ) -> i32 {
        let iWidth = pSrcPixMap.sRect.iRectWidth;
        let iHeight = pSrcPixMap.sRect.iRectHeight;
        let iMbWidth = iWidth >> 4;
        let iMbHeight = iHeight >> 4;
        let iMbTotalNum = iMbWidth * iMbHeight;

        let mut iAverageMotionIndex: i64 = 0;
        let mut iAverageTextureIndex: i64 = 0;

        let iRefStride = _pRefPixMap.iStride[0];
        let iCurStride = pSrcPixMap.iStride[0];

        // Reuse the VAA statistics when they were computed over exactly this pair
        // of pictures; otherwise recompute per macroblock.
        // S10.9: the comparison is between addresses, and both sides say so now —
        // a slice's address is the number the raw root was.
        if calc.pRefY == planes.refp.as_ptr() as usize
            && calc.pCurY == planes.cur.as_ptr() as usize
        {
            let mut iMbIndex = 0isize;
            for _j in 0..iMbHeight {
                for _i in 0..iMbWidth {
                    let sad8x8 = &calc.pSad8x8[(iMbIndex) as usize];
                    let mut iSumDiff = sad8x8[0];
                    iSumDiff += sad8x8[1];
                    iSumDiff += sad8x8[2];
                    iSumDiff += sad8x8[3];

                    let iSQDiff = calc.pSsd16x16[(iMbIndex) as usize];
                    let mut uiSum = calc.pSum16x16[(iMbIndex) as usize];
                    let iSQSum = calc.pSumOfSquare16x16[(iMbIndex) as usize];

                    // Every one of these is `int32_t` in C++ and the result is
                    // stored into a `uint16_t` field, so the truncation is at the
                    // store, not at the arithmetic.
                    iSumDiff >>= 8;
                    let mt = &mut pMotionTexture[iMbIndex as usize];
                    mt.uiMotionIndex = ((iSQDiff >> 8) - iSumDiff * iSumDiff) as u16;

                    uiSum >>= 8;
                    mt.uiTextureIndex = ((iSQSum >> 8) - uiSum * uiSum) as u16;

                    iAverageMotionIndex += mt.uiMotionIndex as i64;
                    iAverageTextureIndex += mt.uiTextureIndex as i64;
                    iMbIndex += 1;
                }
            }
        } else {
            // S11.43: the row/column pointer walk is the same arithmetic on
            // indices, each macroblock's origin bounds-checked by the reslice.
            let mut iMbIndex = 0usize;
            let (mut iRefRow, mut iCurRow) = (0usize, 0usize);
            for _j in 0..iMbHeight {
                let (mut iRefOff, mut iCurOff) = (iRefRow, iCurRow);
                for _i in 0..iMbWidth {
                    let mt = &mut pMotionTexture[iMbIndex];
                    *mt = sample_variance_16x16(
                        &planes.refp[iRefOff..],
                        iRefStride as usize,
                        &planes.cur[iCurOff..],
                        iCurStride as usize,
                    );
                    iAverageMotionIndex += mt.uiMotionIndex as i64;
                    iAverageTextureIndex += mt.uiTextureIndex as i64;
                    iMbIndex += 1;
                    iRefOff += MB_WIDTH_LUMA as usize;
                    iCurOff += MB_WIDTH_LUMA as usize;
                }
                iRefRow += (iRefStride << 4) as usize;
                iCurRow += (iCurStride << 4) as usize;
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
        for j in 0..iMbHeight {
            for i in 0..iMbWidth {
                let mt = pMotionTexture[(j * iMbWidth + i) as usize];
                let mut a = WELS_DIV_ROUND64(
                    mt.uiTextureIndex as i64 * AQ_INT_MULTIPLY * AQ_TIME_INT_MULTIPLY,
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
                    mt.uiMotionIndex as i64 * AQ_INT_MULTIPLY * AQ_TIME_INT_MULTIPLY,
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

                pMotionTextureIndexToDeltaQp[(j * iMbWidth + i) as usize] =
                    (iMotionTextureIndexToDeltaQp as i64 / AQ_QSTEP_INT_MULTIPLY) as i8;
                iAverMotionTextureIndexToDeltaQp += iMotionTextureIndexToDeltaQp;
            }
        }

        self.m_sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp =
            iAverMotionTextureIndexToDeltaQp / iMbTotalNum;

        RET_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A block whose samples are all equal has zero texture index, and a block whose
    /// difference from the reference is uniform has zero motion index — for any
    /// stride pair. Checked against the C++ arithmetic by construction:
    /// `uiSquare >> 8` is `(256 * d^2) >> 8 = d^2` and `(uiSum >> 8)^2` is
    /// `((256 * d) >> 8)^2 = d^2`, so the two cancel exactly.
    #[test]
    fn variance_of_a_uniform_block_is_zero() {
        for &(ref_stride, src_stride) in &[(16usize, 16usize), (16, 33), (48, 17)] {
            for d in [0u8, 1, 17, 255] {
                let refy = vec![d; mb_span(ref_stride)];
                let srcy = vec![0u8; mb_span(src_stride)];
                let got = sample_variance_16x16(&refy, ref_stride, &srcy, src_stride);
                assert_eq!(got.uiMotionIndex, 0, "difference {d}, strides {ref_stride}/{src_stride}");
                assert_eq!(got.uiTextureIndex, 0, "strides {ref_stride}/{src_stride}");
            }
        }
    }

    /// The two strides are read independently, which is the one thing a shim sizing
    /// both spans from a single stride would get wrong.
    ///
    /// The source plane is laid out so that its 16x16 block is uniform at its *own*
    /// stride but would be ragged at the reference's, so a kernel that walked the
    /// source at `iRefStride` reports a non-zero texture index where the right answer
    /// is zero.
    #[test]
    fn the_two_strides_are_independent() {
        let (ref_stride, src_stride) = (64usize, 20usize);
        let refy = vec![0u8; mb_span(ref_stride)];
        // A whole 16 rows rather than `mb_span`, so the inter-row gap after the last
        // row exists to be (wrongly) read. The kernel is still only entitled to
        // `mb_span`; this test is about which bytes it picks, not how many.
        let mut srcy = vec![0u8; 16 * src_stride];
        for row in 0..16 {
            for col in 0..src_stride {
                // 100 inside the block, 200 in the inter-row gap the walk must skip.
                srcy[row * src_stride + col] = if col < 16 { 100 } else { 200 };
            }
        }
        let got = sample_variance_16x16(&refy, ref_stride, &srcy, src_stride);
        assert_eq!(got.uiTextureIndex, 0, "the source walk picked up the row gap");
        assert_eq!(got.uiMotionIndex, 0);
    }

    /// The kernel reads exactly `mb_span`, which is what the shim allocates: sixteen
    /// rows of sixteen samples reaching forward only. A plane one byte shorter is a
    /// panic, and this pins that the span is not over-stated either — the last byte
    /// of the allocation is the last sample of the block, so it must be read.
    #[test]
    fn the_span_is_exactly_the_block() {
        let stride = 24usize;
        assert_eq!(mb_span(stride), 15 * stride + 16);

        let refy = vec![0u8; mb_span(stride)];
        let mut srcy = vec![0u8; mb_span(stride)];
        // Only the very last sample of the block differs.
        *srcy.last_mut().unwrap() = 255;
        let got = sample_variance_16x16(&refy, stride, &srcy, stride);
        assert_ne!(
            got.uiTextureIndex, 0,
            "the last sample of the declared span was never read"
        );
    }
}
