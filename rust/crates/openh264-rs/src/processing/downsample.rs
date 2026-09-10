#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

//! Resamplers for the lower spatial layers — `codec/processing/src/downsample/`, the
//! plugin reached through `METHOD_DOWNSAMPLE`. `CWelsPreProcess::SingleLayerPreprocess`
//! scales the source into each lower spatial layer through `DownsamplePadding`, one step
//! per layer.
//!
//! Dyadic half, quarter and one-third box averages, plus a general-ratio bilinear
//! resample; luma and chroma both take the accurate general-ratio kernel and there is no
//! fast variant. [`Downsample`]'s second arm — repeated halving through a scratch buffer
//! — is the normal path, so a 4:1 step is two cascaded half-averages rather than the
//! quarter kernel. The first arm takes sources too large for the scratch: wider than
//! `2 * MAX_SAMPLE_WIDTH` (3840) or taller than `2 * MAX_SAMPLE_HEIGHT` (2176), which
//! `ParamValidationExt` admits.

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};

/// `downsample.cpp:37-38`.
const MAX_SAMPLE_WIDTH: usize = 1920;
const MAX_SAMPLE_HEIGHT: usize = 1088;

/// `WELS_ALIGN` — `macros.h:85`.
#[inline]
fn WELS_ALIGN(x: usize, n: usize) -> usize {
    (x + n - 1) & !(n - 1)
}

/// `WELS_ROUND` — `macros.h:120`: `(int32_t)(0.5 + x)`.
#[inline]
fn WELS_ROUND(x: f32) -> i32 {
    (0.5 + x as f64) as i32
}

/// `DyadicBilinearDownsampler_c` — `downsamplefuncs.cpp:47`. A 2x2 box average with
/// the rounding done in two halves: each row pair first, then the two rows.
///
/// `kiSrcWidth` is **not** the picture width — see [`DownsampleHalfAverage`], which
/// rounds it up to a multiple of 32 or 16 and so writes into the destination's padding.
fn DyadicBilinearDownsampler(
    pDst: &mut [u8],
    kiDstStride: usize,
    pSrc: &[u8],
    kiSrcStride: usize,
    kiSrcWidth: usize,
    kiSrcHeight: usize,
) {
    let kiDstWidth = kiSrcWidth >> 1;
    let kiDstHeight = kiSrcHeight >> 1;
    for j in 0..kiDstHeight {
        let dstLine = j * kiDstStride;
        let srcLine = j * (kiSrcStride << 1);
        for i in 0..kiDstWidth {
            let kiSrcX = srcLine + (i << 1);
            let kiTempRow1 = (pSrc[kiSrcX] as i32 + pSrc[kiSrcX + 1] as i32 + 1) >> 1;
            let kiTempRow2 =
                (pSrc[kiSrcX + kiSrcStride] as i32 + pSrc[kiSrcX + kiSrcStride + 1] as i32 + 1)
                    >> 1;
            pDst[dstLine + i] = ((kiTempRow1 + kiTempRow2 + 1) >> 1) as u8;
        }
    }
}

/// `DyadicBilinearQuarterDownsampler_c` — `downsamplefuncs.cpp:71`. The same 2x2
/// average taken on a 4-pixel grid, so it averages a quarter of each 4x4 cell and
/// drops the rest. Reachable only through [`Downsample`]'s first arm.
fn DyadicBilinearQuarterDownsampler(
    pDst: &mut [u8],
    kiDstStride: usize,
    pSrc: &[u8],
    kiSrcStride: usize,
    kiSrcWidth: usize,
    kiSrcHeight: usize,
) {
    let kiDstWidth = kiSrcWidth >> 2;
    let kiDstHeight = kiSrcHeight >> 2;
    for j in 0..kiDstHeight {
        let dstLine = j * kiDstStride;
        let srcLine = j * (kiSrcStride << 2);
        for i in 0..kiDstWidth {
            let kiSrcX = srcLine + (i << 2);
            let kiTempRow1 = (pSrc[kiSrcX] as i32 + pSrc[kiSrcX + 1] as i32 + 1) >> 1;
            let kiTempRow2 =
                (pSrc[kiSrcX + kiSrcStride] as i32 + pSrc[kiSrcX + kiSrcStride + 1] as i32 + 1)
                    >> 1;
            pDst[dstLine + i] = ((kiTempRow1 + kiTempRow2 + 1) >> 1) as u8;
        }
    }
}

/// `DyadicBilinearOneThirdDownsampler_c` — `downsamplefuncs.cpp:95`.
///
/// Unlike its two siblings the last parameter is the **destination** height, not the
/// source height. Reachable only through [`Downsample`]'s first arm.
fn DyadicBilinearOneThirdDownsampler(
    pDst: &mut [u8],
    kiDstStride: usize,
    pSrc: &[u8],
    kiSrcStride: usize,
    kiSrcWidth: usize,
    kiDstHeight: usize,
) {
    let kiDstWidth = kiSrcWidth / 3;
    for j in 0..kiDstHeight {
        let dstLine = j * kiDstStride;
        let srcLine = j * (kiSrcStride * 3);
        for i in 0..kiDstWidth {
            let kiSrcX = srcLine + i * 3;
            let kiTempRow1 = (pSrc[kiSrcX] as i32 + pSrc[kiSrcX + 1] as i32 + 1) >> 1;
            let kiTempRow2 =
                (pSrc[kiSrcX + kiSrcStride] as i32 + pSrc[kiSrcX + kiSrcStride + 1] as i32 + 1)
                    >> 1;
            pDst[dstLine + i] = ((kiTempRow1 + kiTempRow2 + 1) >> 1) as u8;
        }
    }
}

/// `GeneralBilinearAccurateDownsampler_c` — `downsamplefuncs.cpp:187`.
///
/// A 16.15 fixed-point bilinear resample.
///
/// * the **last column of every row** and the **whole last row** are nearest-neighbour
///   copies, not interpolations, which avoids reading one past the source; the sampled
///   position differs from what interpolation would give;
/// * the accumulator is 64-bit: the four weight products reach `(2^15-1)^2 * 255`,
///   which overflows `i32`.
fn GeneralBilinearAccurateDownsampler(
    pDst: &mut [u8],
    kiDstStride: usize,
    kiDstWidth: usize,
    kiDstHeight: usize,
    pSrc: &[u8],
    kiSrcStride: usize,
    kiSrcWidth: usize,
    kiSrcHeight: usize,
) {
    if kiDstWidth == 0 || kiDstHeight == 0 {
        return;
    }
    const kiScaleBit: i32 = 15;
    const kiScale: i32 = 1 << kiScaleBit;
    let iScalex = WELS_ROUND(kiSrcWidth as f32 / kiDstWidth as f32 * kiScale as f32);
    let iScaley = WELS_ROUND(kiSrcHeight as f32 / kiDstHeight as f32 * kiScale as f32);

    let mut pByLineDst = 0usize;
    let mut iYInverse: i32 = 1 << (kiScaleBit - 1);
    for _i in 0..kiDstHeight - 1 {
        let iYy = (iYInverse >> kiScaleBit) as usize;
        let iFv = (iYInverse & (kiScale - 1)) as i64;

        let pBySrc = iYy * kiSrcStride;
        let mut pByDst = pByLineDst;
        let mut iXInverse: i32 = 1 << (kiScaleBit - 1);
        for _j in 0..kiDstWidth - 1 {
            let iXx = (iXInverse >> kiScaleBit) as usize;
            let iFu = (iXInverse & (kiScale - 1)) as i64;

            let pByCurrent = pBySrc + iXx;
            let a = pSrc[pByCurrent] as i64;
            let b = pSrc[pByCurrent + 1] as i64;
            let c = pSrc[pByCurrent + kiSrcStride] as i64;
            let d = pSrc[pByCurrent + kiSrcStride + 1] as i64;

            let ks = kiScale as i64;
            let mut x = (ks - 1 - iFu) * (ks - 1 - iFv) * a
                + iFu * (ks - 1 - iFv) * b
                + (ks - 1 - iFu) * iFv * c
                + iFu * iFv * d
                + (1i64 << (2 * kiScaleBit - 1));
            x >>= 2 * kiScaleBit;
            x = x.clamp(0, 255);
            pDst[pByDst] = x as u8;
            pByDst += 1;

            iXInverse += iScalex;
        }
        pDst[pByDst] = pSrc[pBySrc + ((iXInverse >> kiScaleBit) as usize)];
        pByLineDst += kiDstStride;
        iYInverse += iScaley;
    }

    // last row special
    {
        let iYy = (iYInverse >> kiScaleBit) as usize;
        let pBySrc = iYy * kiSrcStride;
        let mut pByDst = pByLineDst;
        let mut iXInverse: i32 = 1 << (kiScaleBit - 1);
        for _j in 0..kiDstWidth {
            let iXx = (iXInverse >> kiScaleBit) as usize;
            pDst[pByDst] = pSrc[pBySrc + iXx];
            pByDst += 1;
            iXInverse += iScalex;
        }
    }
}

/// `CDownsampling::DownsampleHalfAverage` — `downsample.cpp:279`.
///
/// The branch selects only the **width passed**: the source width rounded up to a
/// multiple of 32 when the source stride is 32-aligned, of 16 otherwise. That rounding
/// is why the destination gets more columns than its nominal width, its padding written.
fn DownsampleHalfAverage(
    pDst: &mut [u8],
    iDstStride: usize,
    pSrc: &[u8],
    iSrcStride: usize,
    iSrcWidth: usize,
    iSrcHeight: usize,
) {
    let w = if iSrcStride & 31 == 0 {
        WELS_ALIGN(iSrcWidth & !1, 32)
    } else {
        WELS_ALIGN(iSrcWidth & !1, 16)
    };
    DyadicBilinearDownsampler(pDst, iDstStride, pSrc, iSrcStride, w, iSrcHeight);
}

/// The scratch the multi-pass arm halves through — `m_pSampleBuffer[2][3]`,
/// `downsample.cpp:56-66`.
///
/// The six buffers are `Vec`s grown on first use, which keeps the ~6 MB off every
/// encoder that never takes this path.
#[derive(Default)]
pub struct SampleBuffer {
    bufs: [[Vec<u8>; 3]; 2],
}

impl SampleBuffer {
    fn ensure(&mut self) {
        for set in self.bufs.iter_mut() {
            if set[0].is_empty() {
                set[0] = vec![0u8; MAX_SAMPLE_WIDTH * MAX_SAMPLE_HEIGHT];
                set[1] = vec![0u8; MAX_SAMPLE_WIDTH * MAX_SAMPLE_HEIGHT / 4];
                set[2] = vec![0u8; MAX_SAMPLE_WIDTH * MAX_SAMPLE_HEIGHT / 4];
            }
        }
    }
}

/// `CDownsampling` — `downsample.h:170`. The plugin object; the scratch is its only
/// state.
#[derive(Default)]
pub struct CDownsampling {
    pub m_pSampleBuffer: SampleBuffer,
}

/// One plane trio to read from, as slices from their logical origins.
pub struct DownsampleSrc<'a> {
    pub planes: [&'a [u8]; 3],
    pub stride: [usize; 3],
    pub width: i32,
    pub height: i32,
}

/// One plane trio to write to.
pub struct DownsampleDst<'a> {
    pub planes: [&'a mut [u8]; 3],
    pub stride: [usize; 3],
    pub width: i32,
    pub height: i32,
}

impl CDownsampling {
    /// `CDownsampling::Process` — `downsample.cpp:144`. See [`Downsample`].
    pub fn Process(&mut self, pSrc: &DownsampleSrc<'_>, pDst: &mut DownsampleDst<'_>) -> i32 {
        Downsample(&mut self.m_pSampleBuffer, pSrc, pDst)
    }
}

/// `CDownsampling::Process`'s body — `downsample.cpp:144`.
///
/// Free rather than a method: the caller holds the two pictures and the plugin through
/// one `&mut CWelsPreProcess`, and those borrows are disjoint in fact but not in what a
/// method call can express.
pub fn Downsample(
    scratch: &mut SampleBuffer,
    pSrc: &DownsampleSrc<'_>,
    pDst: &mut DownsampleDst<'_>,
) -> i32 {
    if pSrc.width <= 0 || pSrc.height <= 0 || pDst.width <= 0 || pDst.height <= 0 {
        return RET_INVALIDPARAM;
    }
    let mut iSrcWidthY = pSrc.width as usize;
    let mut iSrcHeightY = pSrc.height as usize;
    let iDstWidthY = pDst.width as usize;
    let iDstHeightY = pDst.height as usize;

    let mut iSrcWidthUV = iSrcWidthY >> 1;
    let mut iSrcHeightUV = iSrcHeightY >> 1;
    let iDstWidthUV = iDstWidthY >> 1;
    let iDstHeightUV = iDstHeightY >> 1;

    if iSrcWidthY <= iDstWidthY || iSrcHeightY <= iDstHeightY {
        return RET_INVALIDPARAM;
    }
    // copied out before `pDst.planes` is destructured: `pDst` is one borrow
    let dst_stride = pDst.stride;

    // ---- arm 1: no scratch big enough (an oversized source). One pass, kernel
    // picked by the exact ratio.
    if (iSrcWidthY >> 1) > MAX_SAMPLE_WIDTH || (iSrcHeightY >> 1) > MAX_SAMPLE_HEIGHT {
        let [dy, du, dv] = &mut pDst.planes;
        if (iSrcWidthY >> 1) == iDstWidthY && (iSrcHeightY >> 1) == iDstHeightY {
            DownsampleHalfAverage(
                dy,
                dst_stride[0],
                pSrc.planes[0],
                pSrc.stride[0],
                iSrcWidthY,
                iSrcHeightY,
            );
            DownsampleHalfAverage(
                du,
                dst_stride[1],
                pSrc.planes[1],
                pSrc.stride[1],
                iSrcWidthUV,
                iSrcHeightUV,
            );
            DownsampleHalfAverage(
                dv,
                dst_stride[2],
                pSrc.planes[2],
                pSrc.stride[2],
                iSrcWidthUV,
                iSrcHeightUV,
            );
        } else if (iSrcWidthY >> 2) == iDstWidthY && (iSrcHeightY >> 2) == iDstHeightY {
            DyadicBilinearQuarterDownsampler(
                dy,
                dst_stride[0],
                pSrc.planes[0],
                pSrc.stride[0],
                iSrcWidthY,
                iSrcHeightY,
            );
            DyadicBilinearQuarterDownsampler(
                du,
                dst_stride[1],
                pSrc.planes[1],
                pSrc.stride[1],
                iSrcWidthUV,
                iSrcHeightUV,
            );
            DyadicBilinearQuarterDownsampler(
                dv,
                dst_stride[2],
                pSrc.planes[2],
                pSrc.stride[2],
                iSrcWidthUV,
                iSrcHeightUV,
            );
        } else if (iSrcWidthY / 3) == iDstWidthY && (iSrcHeightY / 3) == iDstHeightY {
            // the odd one out: this kernel's last argument is the *destination* height
            DyadicBilinearOneThirdDownsampler(
                dy,
                dst_stride[0],
                pSrc.planes[0],
                pSrc.stride[0],
                iSrcWidthY,
                iDstHeightY,
            );
            DyadicBilinearOneThirdDownsampler(
                du,
                dst_stride[1],
                pSrc.planes[1],
                pSrc.stride[1],
                iSrcWidthUV,
                iDstHeightUV,
            );
            DyadicBilinearOneThirdDownsampler(
                dv,
                dst_stride[2],
                pSrc.planes[2],
                pSrc.stride[2],
                iSrcWidthUV,
                iDstHeightUV,
            );
        } else {
            GeneralBilinearAccurateDownsampler(
                dy,
                dst_stride[0],
                iDstWidthY,
                iDstHeightY,
                pSrc.planes[0],
                pSrc.stride[0],
                iSrcWidthY,
                iSrcHeightY,
            );
            GeneralBilinearAccurateDownsampler(
                du,
                dst_stride[1],
                iDstWidthUV,
                iDstHeightUV,
                pSrc.planes[1],
                pSrc.stride[1],
                iSrcWidthUV,
                iSrcHeightUV,
            );
            GeneralBilinearAccurateDownsampler(
                dv,
                dst_stride[2],
                iDstWidthUV,
                iDstHeightUV,
                pSrc.planes[2],
                pSrc.stride[2],
                iSrcWidthUV,
                iSrcHeightUV,
            );
        }
        return RET_SUCCESS;
    }

    // ---- arm 2: the normal path. Halve repeatedly through the scratch until one
    // more halving would undershoot, then finish with an exact half-average (if the
    // target is exactly half) or the general-ratio kernel.
    scratch.ensure();

    let mut iHalfSrcWidth = iSrcWidthY >> 1;
    let mut iHalfSrcHeight = iSrcHeightY >> 1;
    let mut stY = pSrc.stride[0];
    let mut stU = pSrc.stride[1];
    let mut stV = pSrc.stride[2];
    // Which buffer the *source* currently lives in. `None` on the first pass, when it
    // is still the caller's picture; `Some(i)` afterwards.
    let mut src_at: Option<usize> = None;
    let mut write_to = 0usize;

    let (lo, hi) = scratch.bufs.split_at_mut(1);
    let (buf0, buf1) = (&mut lo[0], &mut hi[0]);

    loop {
        if iHalfSrcWidth == iDstWidthY && iHalfSrcHeight == iDstHeightY {
            // last step: straight into the caller's destination
            let [dy, du, dv] = &mut pDst.planes;
            let (sy, su, sv) = match src_at {
                None => (pSrc.planes[0], pSrc.planes[1], pSrc.planes[2]),
                Some(0) => (&buf0[0][..], &buf0[1][..], &buf0[2][..]),
                _ => (&buf1[0][..], &buf1[1][..], &buf1[2][..]),
            };
            DownsampleHalfAverage(dy, dst_stride[0], sy, stY, iSrcWidthY, iSrcHeightY);
            DownsampleHalfAverage(du, dst_stride[1], su, stU, iSrcWidthUV, iSrcHeightUV);
            DownsampleHalfAverage(dv, dst_stride[2], sv, stV, iSrcWidthUV, iSrcHeightUV);
            break;
        } else if iHalfSrcWidth > iDstWidthY && iHalfSrcHeight > iDstHeightY {
            // one more halving, into the scratch set we are not reading
            let dstStY = WELS_ALIGN(iHalfSrcWidth, 32);
            let dstStU = WELS_ALIGN(iHalfSrcWidth >> 1, 32);
            let dstStV = WELS_ALIGN(iHalfSrcWidth >> 1, 32);
            {
                let (rd, wr): (&[Vec<u8>; 3], &mut [Vec<u8>; 3]) = match (src_at, write_to) {
                    (None, 0) => (&*buf1, &mut *buf0), // rd unused; see below
                    (None, _) => (&*buf0, &mut *buf1),
                    (Some(0), _) => (&*buf0, &mut *buf1),
                    (Some(_), _) => (&*buf1, &mut *buf0),
                };
                let (sy, su, sv) = match src_at {
                    None => (pSrc.planes[0], pSrc.planes[1], pSrc.planes[2]),
                    Some(_) => (&rd[0][..], &rd[1][..], &rd[2][..]),
                };
                let [wy, wu, wv] = wr;
                DownsampleHalfAverage(wy, dstStY, sy, stY, iSrcWidthY, iSrcHeightY);
                DownsampleHalfAverage(wu, dstStU, su, stU, iSrcWidthUV, iSrcHeightUV);
                DownsampleHalfAverage(wv, dstStV, sv, stV, iSrcWidthUV, iSrcHeightUV);
            }
            src_at = Some(write_to);
            write_to = 1 - write_to;

            iSrcWidthY = iHalfSrcWidth;
            iSrcWidthUV = iHalfSrcWidth >> 1;
            iSrcHeightY = iHalfSrcHeight;
            iSrcHeightUV = iHalfSrcHeight >> 1;
            stY = dstStY;
            stU = dstStU;
            stV = dstStV;
            iHalfSrcWidth >>= 1;
            iHalfSrcHeight >>= 1;
        } else {
            // one more halving would undershoot: resample to the target directly
            let [dy, du, dv] = &mut pDst.planes;
            let (sy, su, sv) = match src_at {
                None => (pSrc.planes[0], pSrc.planes[1], pSrc.planes[2]),
                Some(0) => (&buf0[0][..], &buf0[1][..], &buf0[2][..]),
                _ => (&buf1[0][..], &buf1[1][..], &buf1[2][..]),
            };
            GeneralBilinearAccurateDownsampler(
                dy,
                dst_stride[0],
                iDstWidthY,
                iDstHeightY,
                sy,
                stY,
                iSrcWidthY,
                iSrcHeightY,
            );
            GeneralBilinearAccurateDownsampler(
                du,
                dst_stride[1],
                iDstWidthUV,
                iDstHeightUV,
                su,
                stU,
                iSrcWidthUV,
                iSrcHeightUV,
            );
            GeneralBilinearAccurateDownsampler(
                dv,
                dst_stride[2],
                iDstWidthUV,
                iDstHeightUV,
                sv,
                stV,
                iSrcWidthUV,
                iSrcHeightUV,
            );
            break;
        }
    }

    RET_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(len: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(1103515245).wrapping_add(12345);
                ((s >> 16) & 0xff) as u8
            })
            .collect()
    }

    /// The 2x2 box average rounds **twice**: each row pair first, then the pair of row
    /// results, which is not the same as one flat `(sum + 2) >> 2`.
    #[test]
    fn dyadic_rounds_in_two_halves() {
        // [3 0 / 0 0]: rows give 2 and 0, then (2+0+1)>>1 = 1.
        let mut dst = [0u8; 1];
        DyadicBilinearDownsampler(&mut dst, 1, &[3u8, 0, 0, 0], 2, 2, 2);
        assert_eq!(dst[0], 1);

        // [255 254 / 253 252]: rows give 255 and 253, then 254 — a flat
        // (255+254+253+252+2)>>2 gives 254 too, so this cell does not discriminate.
        let mut dst = [0u8; 1];
        DyadicBilinearDownsampler(&mut dst, 1, &[255u8, 254, 253, 252], 2, 2, 2);
        assert_eq!(dst[0], 254);

        // [1 0 / 0 0]: rows give 1 (the +1 rounds it up) and 0, then 1; a flat
        // (1+2)>>2 gives 0. This cell does.
        let mut dst = [0u8; 1];
        DyadicBilinearDownsampler(&mut dst, 1, &[1u8, 0, 0, 0], 2, 2, 2);
        assert_eq!(
            dst[0], 1,
            "two-stage rounding lifts this to 1; a flat average gives 0"
        );
    }

    /// `DownsampleHalfAverage`'s alignment branch is the subtle one: the width
    /// it passes is rounded up, so the destination gets `align(w)/2` columns, not
    /// `w/2`. A 32-aligned source stride rounds to 32, otherwise to 16.
    #[test]
    fn half_average_rounds_the_width_up_by_stride_alignment() {
        let (w, h) = (40usize, 4usize);
        // 32-aligned stride -> width rounds to 64 -> 32 destination columns
        let src = ramp(64 * (h + 2), 7);
        let mut d32 = vec![0u8; 64 * h];
        DownsampleHalfAverage(&mut d32, 32, &src, 64, w, h);
        assert_ne!(d32[31], 0, "column 31 written: the width was rounded to 64");

        // 16-aligned but not 32-aligned stride -> width rounds to 48 -> 24 columns
        let src = ramp(48 * (h + 2), 7);
        let mut d16 = vec![0u8; 48 * h];
        DownsampleHalfAverage(&mut d16, 32, &src, 48, w, h);
        assert_ne!(d16[23], 0, "column 23 written");
        assert_eq!(
            d16[24], 0,
            "column 24 untouched: the width rounded to 48, not 64"
        );
    }

    /// The general kernel's last row and last column are nearest-neighbour copies,
    /// not interpolations. On a source that is constant per row, that makes the last
    /// row exactly one of the source rows.
    #[test]
    fn general_last_row_is_a_copy() {
        let (sw, sh, dw, dh) = (16usize, 16usize, 5usize, 5usize);
        let stride = 16usize;
        let mut src = vec![0u8; stride * sh];
        for r in 0..sh {
            for c in 0..sw {
                src[r * stride + c] = (r * 15) as u8;
            }
        }
        let mut dst = vec![0u8; 8 * dh];
        GeneralBilinearAccurateDownsampler(&mut dst, 8, dw, dh, &src, stride, sw, sh);
        // last row: iYInverse after dh-1 steps, nearest-neighbour from that source row
        let last = &dst[8 * (dh - 1)..8 * (dh - 1) + dw];
        assert!(
            last.iter().all(|&v| v == last[0]),
            "a row-constant source gives a constant last row: {last:?}"
        );
        assert_eq!(last[0] % 15, 0, "and it is a source row verbatim");
    }

    /// `Process` returns `RET_INVALIDPARAM` when the destination is not strictly
    /// smaller in both dimensions; the caller relies on that check.
    #[test]
    fn refuses_upsample_and_equal_size() {
        let mut ds = CDownsampling::default();
        let sy = vec![0u8; 64 * 64];
        let (su, sv) = (vec![0u8; 32 * 32], vec![0u8; 32 * 32]);
        let (mut dy, mut du, mut dv) = (vec![0u8; 64 * 64], vec![0u8; 32 * 32], vec![0u8; 32 * 32]);
        for (w, h) in [(32i32, 32i32), (16, 32), (32, 16), (64, 64)] {
            let src = DownsampleSrc {
                planes: [&sy, &su, &sv],
                stride: [64, 32, 32],
                width: 32,
                height: 32,
            };
            let mut dst = DownsampleDst {
                planes: [&mut dy, &mut du, &mut dv],
                stride: [64, 32, 32],
                width: w,
                height: h,
            };
            assert_eq!(
                ds.Process(&src, &mut dst),
                RET_INVALIDPARAM,
                "{w}x{h} from 32x32"
            );
        }
    }

    /// The scratch is grown once and reused; a second `Process` must not reallocate
    /// or carry state between frames that changes the answer.
    #[test]
    fn scratch_is_reusable_and_idempotent() {
        let (sw, sh) = (64usize, 64usize);
        let sy = ramp(64 * 80, 3);
        let su = ramp(32 * 48, 5);
        let sv = ramp(32 * 48, 9);
        let mut ds = CDownsampling::default();
        let run = |ds: &mut CDownsampling| {
            let (mut dy, mut du, mut dv) =
                (vec![0u8; 32 * 40], vec![0u8; 16 * 24], vec![0u8; 16 * 24]);
            let src = DownsampleSrc {
                planes: [&sy, &su, &sv],
                stride: [64, 32, 32],
                width: sw as i32,
                height: sh as i32,
            };
            let mut dst = DownsampleDst {
                planes: [&mut dy, &mut du, &mut dv],
                stride: [32, 16, 16],
                width: 16,
                height: 16,
            };
            assert_eq!(ds.Process(&src, &mut dst), RET_SUCCESS);
            (dy, du, dv)
        };
        let a = run(&mut ds);
        let b = run(&mut ds);
        assert_eq!(a, b, "the same input twice must give the same output");
    }
}
