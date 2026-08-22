#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/denoise/` — the plugin reached through
//! `METHOD_DENOISE`, behind `bEnableDenoise`.
//!
//! `CWelsPreProcess::SingleLayerPreprocess` (`wels_preprocess.cpp:394`) runs it on
//! the source picture, **in place**, before any downsampling or padding — so every
//! spatial layer of the frame is built from the denoised samples.
//!
//! **This plugin needs no kernel-selection decision (F99).**
//! `CDenoiser::InitDenoiseFunc` (`denoise.cpp:55`) has exactly one non-scalar arm
//! and it is `#if defined(X86_ASM)`; there is no NEON denoise anywhere in the tree,
//! and `nm` on the reference archive confirms it exports only
//! `BilateralLumaFilter8_c`, `WaverageChromaFilter8_c` and `Gauss3x3Filter`. On this
//! host the reference runs the same scalar filters this module does. Contrast
//! `downsample.rs`, where the aarch64 table *does* diverge from the scalar one.
//!
//! Every filter here is sequential in a way that matters: the row above the one
//! being filtered has already been overwritten by the previous iteration, and
//! within a group of eight the current row's earlier pixels have not. The C++ gets
//! that by accumulating into a local `aSample[8]` and `memcpy`-ing it back after the
//! group; this port does the same with an array and a slice write, for the same
//! reason and with the same visible ordering.

#![deny(unsafe_code)]

use crate::encoder::wels_preprocess::SPixMap;

use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};

/// `denoise.h:51-60`.
const DENOISE_GRAY_RADIUS: usize = 1;
const UV_WINDOWS_RADIUS: usize = 2;
const TAIL_OF_LINE8: usize = 7;
const DENOISE_Y_COMPONENT: u16 = 1;
const DENOISE_U_COMPONENT: u16 = 2;
const DENOISE_V_COMPONENT: u16 = 4;
const DENOISE_ALL_COMPONENT: u16 = 7;

/// `BilateralLumaFilter8_c` — `denoise_filter.cpp:41`.
///
/// Eight pixels at a time, each the centre of a 3x3 bilateral window whose weights
/// fall off with the grey difference from the centre. `plane[center ..]` is the
/// group's first pixel; the window reaches one row back and one column left, which
/// is why `BilateralDenoiseLuma` starts at `(radius, radius)`.
///
/// The eight results are written only after all eight are computed — the C++'s
/// `aSample[8]` plus `WelsMemcpy`. Removing that buffer would let pixel `i`'s window
/// see pixel `i-1`'s *filtered* value and change the output.
fn BilateralLumaFilter8(plane: &mut [u8], stride: usize, center: usize) {
    let mut aSample = [0u8; 8];
    for i in 0..8 {
        let mut nSum: i32 = 0;
        let mut nTotWeight: i32 = 0;
        let iCenterSample = plane[center + i] as i32;
        // `pCurLine = pSample - iStride - DENOISE_GRAY_RADIUS`, recomputed per pixel.
        let base = center + i - stride - DENOISE_GRAY_RADIUS;
        for y in 0..3usize {
            for x in 0..3usize {
                if x == 1 && y == 1 {
                    continue; // except center point
                }
                let iCurSample = plane[base + y * stride + x] as i32;
                let iCurWeight = (iCurSample - iCenterSample).abs();
                let iGreyDiff = 32 - iCurWeight;
                if iGreyDiff < 0 {
                    continue;
                }
                let iCurWeight = (iGreyDiff * iGreyDiff) >> 5;
                nSum += iCurSample * iCurWeight;
                nTotWeight += iCurWeight;
            }
        }
        nTotWeight = 256 - nTotWeight;
        nSum += iCenterSample * nTotWeight;
        aSample[i] = (nSum >> 8) as u8;
    }
    plane[center..center + 8].copy_from_slice(&aSample);
}

/// `WaverageChromaFilter8_c` — `denoise_filter.cpp:89`. The fixed 5x5 kernel
///
/// ```text
///   1  1  2  1  1
///   1  2  4  2  1
///   2  4 20  4  2
///   1  2  4  2  1
///   1  1  2  1  1
/// ```
///
/// whose weights sum to 64, hence the `>> 6`.
fn WaverageChromaFilter8(plane: &mut [u8], stride: usize, center: usize) {
    // `SUM_LINE1` / `SUM_LINE2` / `SUM_LINE3`, denoise_filter.cpp:80-82.
    #[inline]
    fn line1(p: &[u8], o: usize) -> i32 {
        p[o] as i32 + p[o + 1] as i32 + ((p[o + 2] as i32) << 1) + p[o + 3] as i32 + p[o + 4] as i32
    }
    #[inline]
    fn line2(p: &[u8], o: usize) -> i32 {
        p[o] as i32
            + ((p[o + 1] as i32) << 1)
            + ((p[o + 2] as i32) << 2)
            + ((p[o + 3] as i32) << 1)
            + p[o + 4] as i32
    }
    #[inline]
    fn line3(p: &[u8], o: usize) -> i32 {
        ((p[o] as i32) << 1)
            + ((p[o + 1] as i32) << 2)
            + (p[o + 2] as i32) * 20
            + ((p[o + 3] as i32) << 2)
            + ((p[o + 4] as i32) << 1)
    }

    let start = center - UV_WINDOWS_RADIUS * stride - UV_WINDOWS_RADIUS;
    let mut aSample = [0u8; 8];
    for i in 0..8usize {
        let sum = line1(plane, start + i)
            + line2(plane, start + stride + i)
            + line3(plane, start + 2 * stride + i)
            + line2(plane, start + 3 * stride + i)
            + line1(plane, start + 4 * stride + i);
        aSample[i] = (sum >> 6) as u8;
    }
    plane[center..center + 8].copy_from_slice(&aSample);
}

/// `Gauss3x3Filter` — `denoise_filter.cpp:114`. The tail of each line, where fewer
/// than eight pixels are left inside the border: a plain 3x3 Gaussian, weights
/// summing to 16.
fn Gauss3x3Filter(plane: &mut [u8], stride: usize, center: usize) {
    let l1 = center - stride - 1;
    let l2 = l1 + stride;
    let l3 = l2 + stride;
    let nSum = plane[l1] as i32
        + ((plane[l1 + 1] as i32) << 1)
        + plane[l1 + 2] as i32
        + ((plane[l2] as i32) << 1)
        + ((plane[l2 + 1] as i32) << 2)
        + ((plane[l2 + 2] as i32) << 1)
        + plane[l3] as i32
        + ((plane[l3 + 1] as i32) << 1)
        + plane[l3 + 2] as i32;
    plane[center] = (nSum >> 4) as u8;
}

/// `CDenoiser::BilateralDenoiseLuma` — `denoise.cpp:92`.
///
/// Interior only: a border of `m_uiSpaceRadius` pixels is left untouched on all four
/// sides. Each row runs the 8-wide filter while a whole group fits inside the right
/// border (`w < iWidth - radius - TAIL_OF_LINE8`) and finishes the row one pixel at a
/// time with the Gaussian.
fn BilateralDenoiseLuma(plane: &mut [u8], iWidth: usize, iHeight: usize, stride: usize) {
    let r = DENOISE_GRAY_RADIUS;
    if iWidth <= 2 * r || iHeight <= 2 * r {
        return;
    }
    for h in r..iHeight - r {
        let row = h * stride;
        let mut w = r;
        // `w < iWidth - radius - TAIL_OF_LINE8` — done in `usize` without the
        // subtraction, which would wrap for a picture narrower than the tail.
        while w + r + TAIL_OF_LINE8 < iWidth {
            BilateralLumaFilter8(plane, stride, row + w);
            w += 8;
        }
        while w < iWidth - r {
            Gauss3x3Filter(plane, stride, row + w);
            w += 1;
        }
    }
}

/// `CDenoiser::WaverageDenoiseChroma` — `denoise.cpp:107`. The same shape at
/// `UV_WINDOWS_RADIUS`, with the 5x5 weighted average instead of the bilateral one.
fn WaverageDenoiseChroma(plane: &mut [u8], iWidth: usize, iHeight: usize, stride: usize) {
    let r = UV_WINDOWS_RADIUS;
    if iWidth <= 2 * r || iHeight <= 2 * r {
        return;
    }
    for h in r..iHeight - r {
        let row = h * stride;
        let mut w = r;
        while w + r + TAIL_OF_LINE8 < iWidth {
            WaverageChromaFilter8(plane, stride, row + w);
            w += 8;
        }
        while w < iWidth - r {
            Gauss3x3Filter(plane, stride, row + w);
            w += 1;
        }
    }
}

/// `CDenoiser` — `denoise.h:80`. One field, and it is a constant in practice: the
/// constructor sets `m_uiType = DENOISE_ALL_COMPONENT` and nothing ever calls `Set`
/// on this plugin, so all three components are always filtered. Kept as a field
/// anyway because `Process`'s three arms read it and dropping it would erase the
/// only thing the C++ struct carries.
pub struct CDenoiser {
    pub m_uiType: u16,
}

impl Default for CDenoiser {
    fn default() -> Self {
        // `CDenoiser::CDenoiser`, denoise.cpp:44-53.
        Self {
            m_uiType: DENOISE_ALL_COMPONENT,
        }
    }
}

/// The three planes of the picture being denoised, as slices from their logical
/// origins, with the strides that go with them.
///
/// The C++ hands `CDenoiser::Process` an `SPixMap` of raw plane pointers; the
/// kernels here are pure arithmetic over slices and take no pointer at all
/// (charter: the processing kernels are 100% safe). `CWelsPreProcess` builds this
/// from `SPicture::plane_mut(i)`, which *is* the padded allocation.
pub struct DenoisePlanes<'a> {
    pub y: &'a mut [u8],
    pub u: &'a mut [u8],
    pub v: &'a mut [u8],
    pub stride: [usize; 3],
}

impl CDenoiser {
    /// `CDenoiser::Process` — `denoise.cpp:66`. The plugin method, kept so this
    /// object has the same surface as its four siblings in `SWelsVpContext`.
    ///
    /// [`Denoise`] is the body. The split exists because the caller holds the
    /// picture and the plugin through the same `&mut CWelsPreProcess`, and those two
    /// borrows do not overlap in fact — only in what a method call can express. The
    /// caller reads `m_uiType` (a `u16`, `Copy`) and then calls [`Denoise`] with the
    /// picture in hand.
    pub fn Process(&mut self, pSrc: &SPixMap, planes: &mut DenoisePlanes<'_>) -> i32 {
        Denoise(self.m_uiType, pSrc, planes)
    }
}

/// `CDenoiser::Process`'s body — `denoise.cpp:66`.
///
/// The C++ takes `(iType, pSrc, dst)` and ignores `dst` entirely: denoising is in
/// place. The `pSrcY == NULL || pSrcU == NULL || pSrcV == NULL` guard becomes the
/// empty-plane test, which is the same "nothing here" the null meant.
pub fn Denoise(m_uiType: u16, pSrc: &SPixMap, planes: &mut DenoisePlanes<'_>) -> i32 {
    if planes.y.is_empty() || planes.u.is_empty() || planes.v.is_empty() {
        return RET_INVALIDPARAM;
    }

    let iWidthY = pSrc.sRect.iRectWidth;
    let iHeightY = pSrc.sRect.iRectHeight;
    if iWidthY <= 0 || iHeightY <= 0 {
        return RET_INVALIDPARAM;
    }
    let iWidthY = iWidthY as usize;
    let iHeightY = iHeightY as usize;
    let iWidthUV = iWidthY >> 1;
    let iHeightUV = iHeightY >> 1;

    if m_uiType & DENOISE_Y_COMPONENT != 0 {
        BilateralDenoiseLuma(planes.y, iWidthY, iHeightY, planes.stride[0]);
    }
    if m_uiType & DENOISE_U_COMPONENT != 0 {
        WaverageDenoiseChroma(planes.u, iWidthUV, iHeightUV, planes.stride[1]);
    }
    if m_uiType & DENOISE_V_COMPONENT != 0 {
        WaverageDenoiseChroma(planes.v, iWidthUV, iHeightUV, planes.stride[2]);
    }

    RET_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Gaussian tail is a plain weighted average; a flat field is its own
    /// output, and a single bright pixel spreads by exactly the kernel.
    #[test]
    fn gauss_matches_its_kernel() {
        let stride = 8;
        let mut p = vec![10u8; stride * 5];
        Gauss3x3Filter(&mut p, stride, stride + 1);
        assert_eq!(p[stride + 1], 10, "a flat field is a fixed point");

        let mut p = vec![0u8; stride * 5];
        p[stride + 1] = 160; // the centre itself, weight 4 of 16
        Gauss3x3Filter(&mut p, stride, stride + 1);
        assert_eq!(p[stride + 1], ((160u32 * 4) >> 4) as u8);
    }

    /// `BilateralLumaFilter8`'s whole point: with every neighbour equal to the
    /// centre, all eight weights are `(32*32)>>5 = 32`, `nTotWeight` reaches 256 and
    /// the centre's own weight falls to zero — the output is still the input.
    #[test]
    fn bilateral_is_a_fixed_point_on_a_flat_field() {
        let stride = 16;
        let mut p = vec![77u8; stride * 4];
        BilateralLumaFilter8(&mut p, stride, stride + 1);
        assert!(p[stride + 1..stride + 9].iter().all(|&v| v == 77));
    }

    /// The group buffer is not an optimisation, it is the semantics: pixel `i`'s
    /// window must see pixel `i-1`'s **original** value. Running the same eight
    /// pixels one at a time gives a different answer, and this pins that they differ
    /// — so a later "simplification" that drops `aSample` fails here rather than in
    /// a golden hash.
    #[test]
    fn group_of_eight_reads_pre_filter_values() {
        let stride = 16;
        // Small variations on purpose: neighbours must be within the filter's
        // 32-level reach, or every weight is zero, the output equals the input, and
        // the two orders agree trivially.
        let mut ramp = vec![0u8; stride * 4];
        for (i, v) in ramp.iter_mut().enumerate() {
            *v = (100 + (i * 13) % 17) as u8;
        }
        let mut grouped = ramp.clone();
        BilateralLumaFilter8(&mut grouped, stride, stride + 1);

        // the same arithmetic, but writing each pixel back before the next reads
        let mut serial = ramp.clone();
        for i in 0..8 {
            let mut one = serial.clone();
            BilateralLumaFilter8(&mut one, stride, stride + 1);
            serial[stride + 1 + i] = one[stride + 1 + i];
        }
        assert_ne!(
            grouped[stride + 1..stride + 9],
            serial[stride + 1..stride + 9],
            "if these agree the test has stopped discriminating"
        );
    }

    /// The 5x5 chroma weights sum to 64, so a flat field survives exactly.
    #[test]
    fn waverage_weights_sum_to_64() {
        let stride = 24;
        let mut p = vec![200u8; stride * 8];
        WaverageChromaFilter8(&mut p, stride, 2 * stride + 2);
        assert!(p[2 * stride + 2..2 * stride + 10].iter().all(|&v| v == 200));
    }

    /// The border is never touched, and a picture too small to have an interior is
    /// left entirely alone rather than wrapping in the `usize` bounds.
    #[test]
    fn borders_and_degenerate_sizes() {
        let stride = 16;
        let mut p = vec![9u8; stride * 8];
        p[0] = 1;
        p[stride - 1] = 2;
        BilateralDenoiseLuma(&mut p, 12, 6, stride);
        assert_eq!(p[0], 1, "top-left border pixel untouched");

        for (w, h) in [(0usize, 0usize), (1, 1), (2, 2), (2, 10), (10, 2)] {
            let mut q = vec![5u8; stride * 12];
            BilateralDenoiseLuma(&mut q, w, h, stride);
            WaverageDenoiseChroma(&mut q, w, h, stride);
            assert!(q.iter().all(|&v| v == 5), "{w}x{h} should be a no-op");
        }
    }
}
