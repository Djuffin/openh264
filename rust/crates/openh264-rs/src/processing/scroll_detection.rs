#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/src/scrolldetection/` — the plugin reached through
//! `METHOD_SCROLL_DETECTION`.
//!
//! `CWelsPreProcessScreen::DetectSceneChange` runs it once per P frame, against the
//! *first* available reference only (`wels_preprocess.cpp:1176-1201`, the
//! `iScdIdx == 0` block), and hands the vector it finds to two consumers: the screen
//! scene-change detector, which uses it to call a block `SCROLLED_STATIC`
//! (`SceneChangeDetection.h:158-191`), and — from P10.3 — the mode decision's
//! `JudgeScrollSkip` and the scrolled motion search.
//!
//! **What it detects.** A horizontal band of the current frame is picked, a single
//! *textured* row inside it is chosen ([`CheckLine`] — four or more distinct values,
//! or two or three with more than three changes), and the reference frame is scanned
//! outward from that row's own position for a row that matches it. On a match a
//! window of up to fifty rows around the pair is verified. The answer is the vertical
//! displacement, "previous position minus current position" — so content that has
//! moved **up** by eight rows reports `iScrollMvY = +8`, and upstream's own gtest,
//! which moves content *down* by 512, expects `-512`.
//!
//! **`iScrollMvX` is always zero.** [`ScrollDetectionCore`] sets it so unconditionally
//! (`ScrollDetectionFuncs.cpp:191`); the field exists because the parameter struct and
//! the consumers carry it, not because this detector can produce one. The `X` handling
//! in the consumers is ported all the same — it is upstream's, and a caller driving
//! the processing library directly could set `sMaskRect` and reach the other path.
//!
//! **T9.X — safe.** `SPixMap` carries the geometry, as it does for every other plugin
//! in this directory; the pixels arrive as [`ScdPlanes`] and every read is slice
//! indexing. Nothing here dereferences `SPixMap::pPixel`.

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use super::scene_change_detection::ScdPlanes;
use super::vaacalc::{RET_INVALIDPARAM, RET_SUCCESS};
use crate::encoder::wels_preprocess::{SPixMap, SScrollDetectionParam};

/// `ScrollDetectionFuncs.h:44-47`.
pub const MINIMUM_DETECT_WIDTH: i32 = 50; // no less than 16
pub const CHECK_OFFSET: i32 = 25;
pub const MAX_SCROLL_MV_Y: i32 = 511;
pub const REGION_NUMBER: i32 = 9;

/// A row-and-column offset into a plane, the safe form of the C++'s
/// `pY + row * iStride + x`.
///
/// Every row and column this file computes is non-negative — [`ScrollDetectionCore`]'s
/// own clamps guarantee it, and the module's proof is in that function's comment — so
/// a negative here is a port defect in the arithmetic above it. `try_from` says that
/// with the offset in hand, rather than wrapping into a bounds-check panic a hundred
/// lines away.
#[inline]
fn at(row: i32, iStride: usize, x: i32) -> usize {
    usize::try_from(row as isize * iStride as isize + x as isize)
        .expect("scroll detection addresses a non-negative plane offset")
}

/// `CheckLine` — `ScrollDetectionFuncs.cpp:37-64`.
///
/// Does this row carry enough texture to be worth matching? One colour, never; two or
/// three, only with more than three transitions; four or more, always.
///
/// `iColorMap` is `int32_t[8]` upstream and `u32[8]` here: `RECORD_COLOR` sets bit
/// `v & 31` of word `v >> 5`, so the array is a 256-bit set over the byte values and
/// the element's signedness never reaches the answer — `1 << 31` is the sign bit in
/// C++ and simply the top bit here, and both are counted the same by the popcount
/// loop below.
pub fn CheckLine(pData: &[u8], iWidth: i32) -> i32 {
    let iQualified;
    let mut iColorMap = [0u32; 8];
    let mut iChangedTimes = 0i32;
    let mut iColorCounts = 0i32;

    // RECORD_COLOR (pData[0], iColorMap)
    let t = pData[0];
    iColorMap[(t >> 5) as usize] |= 1u32 << (t & 31);

    for i in 1..iWidth as usize {
        let t = pData[i];
        iColorMap[(t >> 5) as usize] |= 1u32 << (t & 31);
        iChangedTimes += (pData[i] != pData[i - 1]) as i32;
    }
    for i in 0..8 {
        for j in 0..32 {
            iColorCounts += ((iColorMap[i] >> j) & 1) as i32;
        }
    }

    match iColorCounts {
        1 => iQualified = 0,
        2 | 3 => iQualified = (iChangedTimes > 3) as i32,
        _ => iQualified = 1,
    }
    iQualified
}

/// `SelectTestLine` — `ScrollDetectionFuncs.cpp:70-96`.
///
/// The row to match on, searched outward from the band's middle: `mid`, `mid + 1`,
/// `mid - 1`, `mid + 2`, … The first row [`CheckLine`] qualifies wins; `-1` if none in
/// the band does.
///
/// The C++'s `TestPos` is live after the loop, and which of the two assignments left
/// it there is the answer — so the loop is written with the same two statements rather
/// than as a search that returns early.
///
/// `pY` is the plane's origin (the C++'s `pSrcPixMap->pPixel[0]`), not a row.
pub fn SelectTestLine(
    pY: &[u8],
    iWidth: i32,
    iHeight: i32,
    iPicHeight: i32,
    iStride: i32,
    iOffsetX: i32,
    iOffsetY: i32,
) -> i32 {
    let kiHalfHeight = iHeight >> 1;
    let kiMidPos = iOffsetY + kiHalfHeight;
    let mut TestPos = kiMidPos;
    let iStride = iStride as usize;

    let mut iOffsetAbs = 0;
    while iOffsetAbs < kiHalfHeight {
        TestPos = kiMidPos + iOffsetAbs;
        if TestPos < iPicHeight {
            if CheckLine(&pY[at(TestPos, iStride, iOffsetX)..], iWidth) != 0 {
                break;
            }
        }
        TestPos = kiMidPos - iOffsetAbs;
        if TestPos >= 0 {
            if CheckLine(&pY[at(TestPos, iStride, iOffsetX)..], iWidth) != 0 {
                break;
            }
        }
        iOffsetAbs += 1;
    }
    if iOffsetAbs == kiHalfHeight {
        TestPos = -1;
    }
    TestPos
}

/// `CompareLine` — `ScrollDetectionFuncs.cpp:99-108`. 0 when the two rows are equal,
/// 1 otherwise.
///
/// **Two upstream behaviours are load-bearing here and neither is tidied.**
///
/// 1. The first twelve bytes are compared *unconditionally*, as three `LD32`s, before
///    `kiWidth` is consulted at all — so a caller passing a width below twelve still
///    reads twelve bytes from each row. Both slices must therefore hold twelve bytes
///    however narrow the nominal comparison is, which is exactly the reach the C++
///    pointer arithmetic has.
/// 2. `iCmp` is seeded **1**, and the `memcmp` that would clear it runs only when
///    `kiWidth > 12`. So for `kiWidth <= 12` this function answers "different" even
///    when all twelve bytes are equal. That is upstream's, recorded as a finding
///    rather than fixed; the encoder never reaches it, because every width
///    [`ScrollDetectionCore`] is called with is at least 24 (see
///    [`CScrollDetection::ScrollDetectionWithoutMask`]) and the mask path refuses
///    anything at or below [`MINIMUM_DETECT_WIDTH`].
///
/// The three `LD32` comparisons are one twelve-byte slice comparison here: `LD32` is a
/// four-byte load and the C++ compares the loads for equality, so the twelve bytes are
/// tested for equality either way and no endianness enters (taxonomy T7 — a wide load
/// spelled as a wide load, not as a value).
pub fn CompareLine(pYSrc: &[u8], pYRef: &[u8], kiWidth: i32) -> i32 {
    let mut iCmp = 1;

    if pYSrc[..12] != pYRef[..12] {
        return 1;
    }
    if kiWidth > 12 {
        let n = kiWidth as usize - 12;
        iCmp = (pYSrc[12..12 + n] != pYRef[12..12 + n]) as i32;
    }
    iCmp
}

/// `ScrollDetectionCore` — `ScrollDetectionFuncs.cpp:110-198`.
///
/// **The reference's stride is used for both frames** (`iYStride =
/// pRefPixMap->iStride[0]`, `:118`). The encoder's two pictures come from one pool and
/// share a geometry, so the strides are equal; the debug assertion below is that
/// claim, and the port keeps the C++'s single stride rather than "fixing" it into two.
///
/// **`pSrcPixMap` contributed only `pPixel[0]`** in the C++, which is
/// `planes.cur` here — so the parameter is gone and the reference map, which
/// contributes `sRect.iRectHeight` and the stride, stays.
///
/// **Why plain slice indexing is safe without a single added clamp.** Write `lo =
/// iMinHeight`, `hi = iMaxHeight`. `iTestPos` is in `[lo, hi]`: [`SelectTestLine`]
/// searches `[iOffsetY + 1, iOffsetY + 2 * (iHeight >> 1) - 1]` and refuses any row
/// below 0 or at or above `iPicHeight`, and both bounds of that band are inside
/// `[lo, hi]` by the definitions of `iMinHeight`/`iMaxHeight`. In the downward branch
/// `iSearchPos <= hi` is tested, `iLowOffset <= hi - iSearchPos`, and `iCheckedLines -
/// iLowOffset <= iTestPos - lo`; so the reference window runs
/// `[iSearchPos - (iCheckedLines - iLowOffset), iSearchPos + iLowOffset - 1] ⊆ [lo, hi]`
/// and the source window `[iTestPos - (iCheckedLines - iLowOffset), iTestPos +
/// iLowOffset - 1] ⊆ [lo, hi]`. In the upward branch `iSearchPos >= lo` is tested,
/// `iUpOffset <= iSearchPos - lo` and `iCheckedLines - iUpOffset <= hi - iTestPos`,
/// which bound the same two windows the same way. So every row read lies in
/// `[iMinHeight, iMaxHeight] ⊆ [0, iPicHeight - 1]`, and if the indexing below ever
/// panics the port of *this arithmetic* is wrong — not the plane it reads.
pub fn ScrollDetectionCore(
    pRefPixMap: &SPixMap,
    planes: &ScdPlanes<'_>,
    iWidth: i32,
    iHeight: i32,
    iOffsetX: i32,
    iOffsetY: i32,
    sScrollDetectionParam: &mut SScrollDetectionParam,
) {
    let mut bScrollDetected = false;
    let iPicHeight = pRefPixMap.sRect.iRectHeight;
    let iMinHeight = iOffsetY.max(0);
    let iMaxHeight = (iOffsetY + iHeight - 1).min(iPicHeight - 1);

    let pYRef = planes.refp;
    let pYSrc = planes.cur;
    debug_assert_eq!(
        planes.cur_stride, planes.ref_stride,
        "ScrollDetectionCore reads both frames with the reference's stride"
    );
    let iYStride = pRefPixMap.iStride[0];
    let kiStride = iYStride as usize;

    let iTestPos = SelectTestLine(pYSrc, iWidth, iHeight, iPicHeight, iYStride, iOffsetX, iOffsetY);

    if iTestPos == -1 {
        sScrollDetectionParam.bScrollDetectFlag = false;
        return;
    }
    // `pYLine` — the source's test row. Kept as an offset, because the two windows
    // below step away from it in both directions.
    let iLineOff = at(iTestPos, kiStride, iOffsetX);
    let iMaxAbs = (iTestPos - iMinHeight - 1)
        .max(iMaxHeight - iTestPos)
        .min(MAX_SCROLL_MV_Y);
    // The C++'s `int32_t iSearchPos = 0` initializer at `:115` is dead — this
    // assignment (`:140`) always precedes the first read — so the declaration moves
    // here rather than carrying a value the port would have to `#[allow]` away.
    let mut iSearchPos = iTestPos;
    let mut iOffsetAbs = 0;
    while iOffsetAbs <= iMaxAbs {
        iSearchPos = iTestPos + iOffsetAbs;
        if iSearchPos <= iMaxHeight {
            let iTmpOff = at(iSearchPos, kiStride, iOffsetX);
            if CompareLine(&pYSrc[iLineOff..], &pYRef[iTmpOff..], iWidth) == 0 {
                let iLowOffset = (iMaxHeight - iSearchPos).min(CHECK_OFFSET);
                let iCheckedLines = (iTestPos - iMinHeight + iLowOffset).min(2 * CHECK_OFFSET);
                let iBack = (iCheckedLines - iLowOffset) as usize * kiStride;
                let mut i = 0;
                while i < iCheckedLines {
                    let step = i as usize * kiStride;
                    if CompareLine(
                        &pYSrc[iLineOff - iBack + step..],
                        &pYRef[iTmpOff - iBack + step..],
                        iWidth,
                    ) != 0
                    {
                        break;
                    }
                    i += 1;
                }
                if i == iCheckedLines {
                    bScrollDetected = true;
                    break;
                }
            }
        }

        iSearchPos = iTestPos - iOffsetAbs - 1;
        if iSearchPos >= iMinHeight {
            let iTmpOff = at(iSearchPos, kiStride, iOffsetX);
            if CompareLine(&pYSrc[iLineOff..], &pYRef[iTmpOff..], iWidth) == 0 {
                let iUpOffset = (iSearchPos - iMinHeight).min(CHECK_OFFSET);
                let iCheckedLines = (iMaxHeight - iTestPos + iUpOffset).min(2 * CHECK_OFFSET);
                let iBack = iUpOffset as usize * kiStride;
                let mut i = 0;
                while i < iCheckedLines {
                    let step = i as usize * kiStride;
                    if CompareLine(
                        &pYSrc[iLineOff - iBack + step..],
                        &pYRef[iTmpOff - iBack + step..],
                        iWidth,
                    ) != 0
                    {
                        break;
                    }
                    i += 1;
                }
                if i == iCheckedLines {
                    bScrollDetected = true;
                    break;
                }
            }
        }
        iOffsetAbs += 1;
    }

    if !bScrollDetected {
        sScrollDetectionParam.bScrollDetectFlag = false;
    } else {
        sScrollDetectionParam.bScrollDetectFlag = true;
        sScrollDetectionParam.iScrollMvY = iSearchPos - iTestPos; // pre_pos - cur_pos, change to mv
        sScrollDetectionParam.iScrollMvX = 0;
    }
}

/// `CScrollDetection` — `ScrollDetection.h:46-67`.
///
/// D-scc-7: the struct owns its parameter block and carries typed `Set`/`Get`/
/// `Process`, the shape every plugin in this directory has had since the `IWelsVP`
/// vtable was dissolved. The framework's `CheckValid` (`WelsFrameWork.cpp:221-256`) is
/// not reproduced — the dissolved vtable never had it — but `Process`'s own validity
/// checks are the C++'s.
#[derive(Debug, Default)]
pub struct CScrollDetection {
    pub m_sScrollDetectionParam: SScrollDetectionParam,
}

impl CScrollDetection {
    /// `CScrollDetection::Set` — `ScrollDetection.cpp:56-62`. The `pParam == NULL`
    /// refusal is spelled by the reference type.
    pub fn Set(&mut self, pParam: &SScrollDetectionParam) -> i32 {
        self.m_sScrollDetectionParam = *pParam;
        RET_SUCCESS
    }

    /// `CScrollDetection::Get` — `ScrollDetection.cpp:64-70`. Copies the whole
    /// parameter block back, as the C++ assignment does.
    pub fn Get(&self, pParam: &mut SScrollDetectionParam) -> i32 {
        *pParam = self.m_sScrollDetectionParam;
        RET_SUCCESS
    }

    /// `CScrollDetection::Process` — `ScrollDetection.cpp:40-53`.
    ///
    /// The C++'s two null-pixel disjuncts are the two empty-slice tests: an
    /// unallocated plane is what `pPixel[0] == NULL` named.
    pub fn Process(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        planes: &ScdPlanes<'_>,
    ) -> i32 {
        if planes.refp.is_empty()
            || planes.cur.is_empty()
            || pRefPixMap.sRect.iRectWidth != pSrcPixMap.sRect.iRectWidth
            || pRefPixMap.sRect.iRectHeight != pSrcPixMap.sRect.iRectHeight
        {
            return RET_INVALIDPARAM;
        }

        if !self.m_sScrollDetectionParam.bMaskInfoAvailable {
            self.ScrollDetectionWithoutMask(pSrcPixMap, pRefPixMap, planes);
        } else {
            self.ScrollDetectionWithMask(pSrcPixMap, pRefPixMap, planes);
        }

        RET_SUCCESS
    }

    /// `CScrollDetection::ScrollDetectionWithMask` — `ScrollDetection.cpp:71-89`.
    ///
    /// **D-scc-9: ported although nothing in the encoder can reach it.** No writer of
    /// `bMaskInfoAvailable` exists under `codec/` — the encoder's one caller zeroes the
    /// parameter block before every `Set` (`wels_preprocess.cpp:1181`) — so this branch
    /// is dead in practice. It is eleven statements, a consumer driving the processing
    /// library directly can request it, and arguing the deadness costs more than the
    /// port. The C++'s branch order is kept.
    fn ScrollDetectionWithMask(
        &mut self,
        _pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        planes: &ScdPlanes<'_>,
    ) {
        let iStartX;
        let iStartY;
        let mut iWidth;
        let iHeight;

        iStartX = self.m_sScrollDetectionParam.sMaskRect.iRectLeft;
        iStartY = self.m_sScrollDetectionParam.sMaskRect.iRectTop;
        iWidth = self.m_sScrollDetectionParam.sMaskRect.iRectWidth;
        iHeight = self.m_sScrollDetectionParam.sMaskRect.iRectHeight;

        iWidth /= 2;
        let iStartX = iStartX + iWidth / 2;

        self.m_sScrollDetectionParam.iScrollMvX = 0;
        self.m_sScrollDetectionParam.iScrollMvY = 0;
        self.m_sScrollDetectionParam.bScrollDetectFlag = false;

        if iStartX >= 0 && iWidth > MINIMUM_DETECT_WIDTH && iHeight > 2 * CHECK_OFFSET {
            ScrollDetectionCore(
                pRefPixMap,
                planes,
                iWidth,
                iHeight,
                iStartX,
                iStartY,
                &mut self.m_sScrollDetectionParam,
            );
        }
    }

    /// `CScrollDetection::ScrollDetectionWithoutMask` — `ScrollDetection.cpp:91-113`.
    ///
    /// Nine probe regions in a 3x3 grid over the frame, tried in order until one both
    /// detects and reports a non-zero vector. The first region row starts *above* the
    /// picture (`iStartY` is negative for `i < 3`), which is deliberate: the band is
    /// seven eighths of the picture tall and the grid centres it, so
    /// [`ScrollDetectionCore`]'s `iMinHeight`/`iMaxHeight` clamps do the trimming.
    ///
    /// `-h * 7 / 48` truncates toward zero on the negative operand in both languages
    /// (unary minus binds tighter than `*` in each, and both divide toward zero), so
    /// `h = 100` gives `-14` on both sides, not `-15`.
    ///
    /// **`iWidth` here is at least 24 for any picture the encoder passes**, which is
    /// what keeps [`CompareLine`]'s twelve-byte floor out of reach: `iWidth =
    /// kiRegionWidth / 2 = (w - 2 * (h >> 4)) / 6`, and the smallest `scc` geometry,
    /// 160x96, gives `(160 - 12) / 6 = 24`.
    fn ScrollDetectionWithoutMask(
        &mut self,
        pSrcPixMap: &SPixMap,
        pRefPixMap: &SPixMap,
        planes: &ScdPlanes<'_>,
    ) {
        let kiPicBorderWidth = pSrcPixMap.sRect.iRectHeight >> 4;
        let kiRegionWidth = (pSrcPixMap.sRect.iRectWidth - (kiPicBorderWidth << 1)) / 3;
        let kiRegionHeight = (pSrcPixMap.sRect.iRectHeight * 7) >> 3;
        let kiHieghtStride = pSrcPixMap.sRect.iRectHeight * 5 / 24;

        for i in 0..REGION_NUMBER {
            let mut iStartX = kiPicBorderWidth + (i % 3) * kiRegionWidth;
            let iStartY = -pSrcPixMap.sRect.iRectHeight * 7 / 48 + (i / 3) * kiHieghtStride;
            let mut iWidth = kiRegionWidth;
            let iHeight = kiRegionHeight;

            iWidth /= 2;
            iStartX += iWidth / 2;

            ScrollDetectionCore(
                pRefPixMap,
                planes,
                iWidth,
                iHeight,
                iStartX,
                iStartY,
                &mut self.m_sScrollDetectionParam,
            );

            if self.m_sScrollDetectionParam.bScrollDetectFlag
                && self.m_sScrollDetectionParam.iScrollMvY != 0
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::wels_preprocess::SRect;

    /// The `gen_screen_clip.py` page, small enough to keep in a test: paper 235, text
    /// lines every 12 rows from row 4, cells 8 columns apart, four inks rotating word
    /// by word so a line carries the four-or-more distinct values [`CheckLine`] wants,
    /// and every fifth line a two-value rule instead.
    ///
    /// Deterministic by the same 31-bit LCG the generator uses, so a failure here is
    /// reproducible without the generator.
    fn page(w: usize, h: usize, seed: u32) -> Vec<u8> {
        const PAPER: u8 = 235;
        const INKS: [u8; 4] = [16, 48, 96, 144];
        let mut state = seed;
        let mut next = move || {
            state = state.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff;
            state >> 8
        };
        // Sixteen 7x10 glyphs; glyph 0 blank, the rest with at least 12 set bits.
        let mut glyphs = vec![[0u16; 10]; 16];
        for g in 1..16 {
            loop {
                let mut rows = [0u16; 10];
                let mut bits = 0;
                for r in 0..10 {
                    rows[r] = (next() & 0x7f) as u16;
                    bits += rows[r].count_ones();
                }
                if bits >= 12 {
                    glyphs[g] = rows;
                    break;
                }
            }
        }

        let mut buf = vec![PAPER; w * h];
        let cells = (w.saturating_sub(16)) / 8;
        let mut line_idx = 0;
        let mut y0 = 4;
        while y0 + 10 < h {
            if line_idx % 5 == 4 {
                // A rule: one row of 200 across the width, at the line's middle.
                let yr = y0 + 5;
                for x in 0..w {
                    buf[yr * w + x] = 200;
                }
            } else {
                let mut ink = (next() % 4) as usize;
                let mut c = 0;
                while c < cells {
                    let word = 3 + (next() % 5) as usize;
                    for _ in 0..word {
                        if c >= cells {
                            break;
                        }
                        let g = (next() % 16) as usize;
                        let x0 = 8 + 8 * c;
                        for r in 0..10 {
                            for b in 0..7 {
                                if glyphs[g][r] & (1 << b) != 0 && x0 + b < w {
                                    buf[(y0 + r) * w + x0 + b] = INKS[ink];
                                }
                            }
                        }
                        c += 1;
                    }
                    ink = (ink + 1) % 4;
                }
            }
            line_idx += 1;
            y0 += 12;
        }
        buf
    }

    fn pixmap(w: i32, h: i32, stride: i32) -> SPixMap {
        let mut m = SPixMap::default();
        m.iStride[0] = stride;
        m.sRect.iRectWidth = w;
        m.sRect.iRectHeight = h;
        m
    }

    /// Runs the detector over a `cur`/`ref` pair of one geometry.
    fn detect(cur: &[u8], refp: &[u8], w: i32, h: i32) -> SScrollDetectionParam {
        let mut d = CScrollDetection::default();
        d.Set(&SScrollDetectionParam::default());
        let map = pixmap(w, h, w);
        let planes = ScdPlanes {
            cur,
            cur_stride: w as usize,
            refp,
            ref_stride: w as usize,
        };
        assert_eq!(d.Process(&map, &map, &planes), RET_SUCCESS);
        let mut out = SScrollDetectionParam::default();
        d.Get(&mut out);
        out
    }

    /// The sign convention, both ways, and it is upstream's: `iScrollMvY =
    /// iSearchPos - iTestPos` is "previous position minus current position", so
    /// content that moved **up** by eight rows reports `+8`. Upstream's own gtest
    /// moves content *down* by 512 and expects `-512`.
    #[test]
    fn a_scrolled_page_reports_its_displacement_with_upstreams_sign() {
        const W: usize = 320;
        const H: usize = 192;
        const K: usize = 8;
        let tall = page(W, H + 2 * K, 7);

        // Content moved UP by K: cur[y] = page[K + y], ref[y] = page[y].
        let refp: Vec<u8> = tall[..W * H].to_vec();
        let cur: Vec<u8> = tall[K * W..][..W * H].to_vec();
        let r = detect(&cur, &refp, W as i32, H as i32);
        assert!(r.bScrollDetectFlag, "a scrolled page must be detected");
        assert_eq!(r.iScrollMvY, K as i32, "content up by {K} rows is +{K}");
        assert_eq!(r.iScrollMvX, 0, "this detector never produces an X vector");

        // Content moved DOWN by K: the two frames swap roles.
        let r = detect(&refp, &cur, W as i32, H as i32);
        assert!(r.bScrollDetectFlag);
        assert_eq!(r.iScrollMvY, -(K as i32), "content down by {K} rows is -{K}");
        assert_eq!(r.iScrollMvX, 0);
    }

    /// A flat frame has one colour per row, so [`CheckLine`] disqualifies every row of
    /// every region and [`SelectTestLine`] answers `-1` nine times. No pixel of the
    /// reference is ever compared.
    #[test]
    fn flat_frames_detect_nothing() {
        const W: i32 = 320;
        const H: i32 = 192;
        let flat = vec![128u8; (W * H) as usize];
        let r = detect(&flat, &flat, W, H);
        assert!(!r.bScrollDetectFlag);
        assert_eq!(r.iScrollMvY, 0);
    }

    /// Two unrelated pages: rows qualify, but no reference row matches a source row
    /// well enough to survive the fifty-row window check.
    #[test]
    fn unrelated_frames_detect_nothing() {
        const W: usize = 320;
        const H: usize = 192;
        let a = page(W, H, 11);
        let b = page(W, H, 4242);
        let r = detect(&a, &b, W as i32, H as i32);
        assert!(
            !r.bScrollDetectFlag,
            "two unrelated pages are not a scroll: got mv {}",
            r.iScrollMvY
        );
    }

    /// An identical pair *is* detected, at zero — the first probe region matches its
    /// own row at `iOffsetAbs == 0`. `ScrollDetectionWithoutMask` therefore does not
    /// stop there (its break wants a non-zero vector) and runs all nine regions, each
    /// reporting the same thing. This pins that a still frame reports
    /// `bScrollDetectFlag = 1, iScrollMvY = 0` rather than "no scroll" — the two are
    /// different inputs to the scene-change detector's `(!iScrollMvX || !iScrollMvY)`
    /// test, which both satisfy.
    #[test]
    fn an_identical_pair_detects_a_zero_vector() {
        const W: usize = 320;
        const H: usize = 192;
        let p = page(W, H, 3);
        let r = detect(&p, &p, W as i32, H as i32);
        assert!(r.bScrollDetectFlag);
        assert_eq!(r.iScrollMvY, 0);
        assert_eq!(r.iScrollMvX, 0);
    }

    /// The mask path on a sub-rectangle of the same scrolled page finds the same
    /// vector the grid does.
    #[test]
    fn the_mask_path_finds_the_same_vector() {
        const W: usize = 320;
        const H: usize = 192;
        const K: usize = 8;
        let tall = page(W, H + 2 * K, 21);
        let refp: Vec<u8> = tall[..W * H].to_vec();
        let cur: Vec<u8> = tall[K * W..][..W * H].to_vec();

        let mut d = CScrollDetection::default();
        let mut p = SScrollDetectionParam::default();
        p.bMaskInfoAvailable = true;
        // Wide enough that `iWidth /= 2` still clears MINIMUM_DETECT_WIDTH, tall
        // enough to clear 2 * CHECK_OFFSET.
        p.sMaskRect = SRect { iRectTop: 20, iRectLeft: 8, iRectWidth: 260, iRectHeight: 140 };
        d.Set(&p);
        let map = pixmap(W as i32, H as i32, W as i32);
        let planes = ScdPlanes { cur: &cur, cur_stride: W, refp: &refp, ref_stride: W };
        assert_eq!(d.Process(&map, &map, &planes), RET_SUCCESS);
        let mut out = SScrollDetectionParam::default();
        d.Get(&mut out);
        assert!(out.bScrollDetectFlag);
        assert_eq!(out.iScrollMvY, K as i32);
    }

    /// A mask narrower than [`MINIMUM_DETECT_WIDTH`] refuses before reading a pixel —
    /// which is why the planes below are empty of anything the core could match, and
    /// why the assertion is that nothing was detected rather than that nothing
    /// panicked. `iWidth` is halved *before* the test, so 100 becomes 50 and `50 > 50`
    /// is false: the boundary is exclusive, as upstream writes it.
    #[test]
    fn a_narrow_mask_refuses_without_reading() {
        const W: usize = 320;
        const H: usize = 192;
        let p0 = page(W, H, 5);
        let mut d = CScrollDetection::default();
        let mut p = SScrollDetectionParam::default();
        p.bMaskInfoAvailable = true;
        p.iScrollMvY = 99; // must be cleared by the mask path before its width test
        p.sMaskRect = SRect { iRectTop: 10, iRectLeft: 0, iRectWidth: 100, iRectHeight: 140 };
        d.Set(&p);
        let map = pixmap(W as i32, H as i32, W as i32);
        let planes = ScdPlanes { cur: &p0, cur_stride: W, refp: &p0, ref_stride: W };
        assert_eq!(d.Process(&map, &map, &planes), RET_SUCCESS);
        let mut out = SScrollDetectionParam::default();
        d.Get(&mut out);
        assert!(!out.bScrollDetectFlag);
        assert_eq!(out.iScrollMvY, 0, "the mask path zeroes the vector first");

        // A short mask is refused the same way: 2 * CHECK_OFFSET is also exclusive.
        p.sMaskRect = SRect { iRectTop: 10, iRectLeft: 0, iRectWidth: 260, iRectHeight: 50 };
        d.Set(&p);
        assert_eq!(d.Process(&map, &map, &planes), RET_SUCCESS);
        d.Get(&mut out);
        assert!(!out.bScrollDetectFlag);
    }

    /// `Process`'s own validity checks: an unallocated plane is the C++'s
    /// `pPixel[0] == NULL`, and two rectangles of different size are its size test.
    #[test]
    fn process_refuses_an_empty_plane_or_a_size_mismatch() {
        let mut d = CScrollDetection::default();
        let map = pixmap(320, 192, 320);
        let buf = vec![0u8; 320 * 192];

        let empty = ScdPlanes { cur: &[], cur_stride: 320, refp: &buf, ref_stride: 320 };
        assert_eq!(d.Process(&map, &map, &empty), RET_INVALIDPARAM);
        let empty = ScdPlanes { cur: &buf, cur_stride: 320, refp: &[], ref_stride: 320 };
        assert_eq!(d.Process(&map, &map, &empty), RET_INVALIDPARAM);

        let planes = ScdPlanes { cur: &buf, cur_stride: 320, refp: &buf, ref_stride: 320 };
        let other = pixmap(320, 96, 320);
        assert_eq!(d.Process(&map, &other, &planes), RET_INVALIDPARAM);
        let other = pixmap(160, 192, 320);
        assert_eq!(d.Process(&map, &other, &planes), RET_INVALIDPARAM);
    }

    /// [`CheckLine`]'s three arms, at the boundaries upstream writes them.
    #[test]
    fn check_line_counts_colours_then_transitions() {
        assert_eq!(CheckLine(&[7u8; 32], 32), 0, "one colour never qualifies");

        // Two colours, exactly three transitions: not enough.
        let mut row = [1u8; 32];
        row[8..16].fill(2);
        row[24..].fill(2);
        assert_eq!(CheckLine(&row, 32), 0, "two colours and 3 changes is short");
        // One more transition tips it.
        row[20] = 2;
        assert_eq!(CheckLine(&row, 32), 1);

        // Four colours qualify however still they are.
        let mut row = [1u8; 32];
        row[8..16].fill(2);
        row[16..24].fill(3);
        row[24..].fill(4);
        assert_eq!(CheckLine(&row, 32), 1, "four colours always qualify");

        // The colour map is a 256-bit set: 32 and 64 land in different words, and
        // 31/32 in adjacent words at the `>> 5` boundary.
        let mut row = [31u8; 32];
        row[16..].fill(32);
        assert_eq!(CheckLine(&row, 32), 0, "two colours, one change");
    }

    /// [`CompareLine`]'s two upstream quirks, pinned so a later "simplification"
    /// fails here rather than in a byte diff: twelve bytes are read whatever the
    /// width, and a width at or below twelve answers "different" even when those
    /// twelve bytes are equal.
    #[test]
    fn compare_line_keeps_its_twelve_byte_floor() {
        let a = [9u8; 16];
        let b = [9u8; 16];
        assert_eq!(CompareLine(&a, &b, 16), 0, "equal rows compare equal");
        assert_eq!(
            CompareLine(&a, &b, 8),
            1,
            "a width at or below 12 answers `different` even on equal bytes"
        );
        assert_eq!(CompareLine(&a, &b, 12), 1, "12 is `at or below`, not `above`");

        // A difference inside the first twelve bytes short-circuits at any width.
        let mut c = b;
        c[11] = 8;
        assert_eq!(CompareLine(&a, &c, 16), 1);
        // A difference past twelve is only seen when the width reaches it.
        let mut d = b;
        d[13] = 8;
        assert_eq!(CompareLine(&a, &d, 16), 1);
        assert_eq!(CompareLine(&a, &d, 13), 0, "byte 13 is outside a width of 13");
    }
}
