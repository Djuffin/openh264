//! Motion compensation — luma quarter-pel, chroma eighth-pel, and the copy paths.
#![forbid(unsafe_code)]
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

// CPU feature flags from cpu_core.h

use crate::safe::plane::{PlaneCursor, PlaneCursorMut, RefSamples};

// Function pointer signatures matching mc.h.
pub type PWelsMcFunc =
    fn(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, mv_x: i16, mv_y: i16, width: usize, height: usize);

pub type PWelsLumaHalfpelMcFunc =
    fn(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize);

pub type PWelsSampleAveragingFunc =
    fn(dst: &mut PlaneCursorMut<'_>, a: &PlaneCursor<'_>, b: &PlaneCursor<'_>, width: usize, height: usize);

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TagMcFunc {
    pub pfLumaHalfpelHor: PWelsLumaHalfpelMcFunc,
    pub pfLumaHalfpelVer: PWelsLumaHalfpelMcFunc,
    pub pfLumaHalfpelCen: PWelsLumaHalfpelMcFunc,
    pub pMcChromaFunc: PWelsMcFunc,
    pub pMcLumaFunc: PWelsMcFunc,
    pub pfSampleAveraging: PWelsSampleAveragingFunc,
}

pub type SMcFunc = TagMcFunc;

impl Default for TagMcFunc {
    /// The kernels are wrapped in non-capturing closures rather than named
    /// directly: they are generic over the cursor type, so a bare path does not
    /// coerce to a slot type that is higher-ranked over the cursor's lifetime,
    /// and a non-capturing closure does.
    fn default() -> Self {
        Self {
            pfLumaHalfpelHor: |s, d, w, h| mc_hor_ver20_c(s, d, w, h),
            pfLumaHalfpelVer: |s, d, w, h| mc_hor_ver02_c(s, d, w, h),
            pfLumaHalfpelCen: |s, d, w, h| mc_hor_ver22_c(s, d, w, h),
            pMcChromaFunc: |s, d, mx, my, w, h| mc_chroma_c(s, d, mx, my, w, h),
            pMcLumaFunc: |s, d, mx, my, w, h| mc_luma_c(s, d, mx, my, w, h),
            pfSampleAveraging: |dst, a, b, w, h| pixel_avg_c(dst, a, b, w, h),
        }
    }
}

// Chroma interpolation weight lookup table: g_kuiABCD[dy][dx]
pub static g_kuiABCD: [[[u8; 4]; 8]; 8] = [
    // dy = 0
    [
        [64, 0, 0, 0],
        [56, 8, 0, 0],
        [48, 16, 0, 0],
        [40, 24, 0, 0],
        [32, 32, 0, 0],
        [24, 40, 0, 0],
        [16, 48, 0, 0],
        [8, 56, 0, 0],
    ],
    // dy = 1
    [
        [56, 0, 8, 0],
        [49, 7, 7, 1],
        [42, 14, 6, 2],
        [35, 21, 5, 3],
        [28, 28, 4, 4],
        [21, 35, 3, 5],
        [14, 42, 2, 6],
        [7, 49, 1, 7],
    ],
    // dy = 2
    [
        [48, 0, 16, 0],
        [42, 6, 14, 2],
        [36, 12, 12, 4],
        [30, 18, 10, 6],
        [24, 24, 8, 8],
        [18, 30, 6, 10],
        [12, 36, 4, 12],
        [6, 42, 2, 14],
    ],
    // dy = 3
    [
        [40, 0, 24, 0],
        [35, 5, 21, 3],
        [30, 10, 18, 6],
        [25, 15, 15, 9],
        [20, 20, 12, 12],
        [15, 25, 9, 15],
        [10, 30, 6, 18],
        [5, 35, 3, 21],
    ],
    // dy = 4
    [
        [32, 0, 32, 0],
        [28, 4, 28, 4],
        [24, 8, 24, 8],
        [20, 12, 20, 12],
        [16, 16, 16, 16],
        [12, 20, 12, 20],
        [8, 24, 8, 24],
        [4, 28, 4, 28],
    ],
    // dy = 5
    [
        [24, 0, 40, 0],
        [21, 3, 35, 5],
        [18, 6, 30, 10],
        [15, 9, 25, 15],
        [12, 12, 20, 20],
        [9, 15, 15, 25],
        [6, 18, 10, 30],
        [3, 21, 5, 35],
    ],
    // dy = 6
    [
        [16, 0, 48, 0],
        [14, 2, 42, 6],
        [12, 4, 36, 12],
        [10, 6, 30, 18],
        [8, 8, 24, 24],
        [6, 10, 18, 30],
        [4, 12, 12, 36],
        [2, 14, 6, 42],
    ],
    // dy = 7
    [
        [8, 0, 56, 0],
        [7, 1, 49, 7],
        [6, 2, 42, 14],
        [5, 3, 35, 21],
        [4, 4, 28, 28],
        [3, 5, 21, 35],
        [2, 6, 14, 42],
        [1, 7, 7, 49],
    ],
];

#[inline(always)]
pub fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        if iX < 0 {
            0
        } else {
            255
        }
    } else {
        iX as u8
    }
}

// ============================================================================
// Kernels
// ============================================================================
//
// These kernels read one plane and write another: they take a `PlaneCursor` (the
// reference picture, or an encoder search buffer) *and* a `PlaneCursorMut` (the
// destination) rather than one cursor over a single surface. The two are different
// allocations at every real call site.
//
// The reads reach outside the block by design — the 6-tap Wiener filter of H.264
// half-pel interpolation needs two samples before and three after each output
// sample, in whichever direction it runs. An MC read is legal because the caller
// clamped the motion vector first.
//
// Every intermediate below keeps the width `codec/common/src/mc.cpp` uses: with
// byte inputs the 6-tap sums are bounded by `510 * 20 = 10200` in
// `filter_input_8bit` and by `21420 + 25500 + 428400 = 475320` in
// `hor_filter_input_16bit`, so nothing here can overflow its `i32`, and the `as
// i16` narrowing in `mc_hor_ver22` is likewise inside range.

/// The 6-tap Wiener filter over six samples — the C++ `FilterInput8bitWithStride_c`
/// with its `kiOffset` walk already done by the caller, so `p[i]` is that kernel's
/// `pSrc[(i - 2) * kiOffset]`.
///
/// C++: `FilterInput8bitWithStride_c`, `codec/common/src/mc.cpp`.
#[inline(always)]
pub fn filter_input_8bit(p: &[u8; 6]) -> i32 {
    let kuiPix05 = (p[0] as u32) + (p[5] as u32);
    let kuiPix14 = (p[1] as u32) + (p[4] as u32);
    let kuiPix23 = (p[2] as u32) + (p[3] as u32);

    (kuiPix05 as i32)
        - (((kuiPix14 << 2) + kuiPix14) as i32)
        + (((kuiPix23 << 4) + (kuiPix23 << 2)) as i32)
}

/// The same filter over the 16-bit intermediates of the centre kernel.
///
/// C++: `HorFilterInput16bit_c`, `codec/common/src/mc.cpp`.
#[inline(always)]
pub fn hor_filter_input_16bit(p: &[i16; 6]) -> i32 {
    let iPix05 = (p[0] as i32) + (p[5] as i32);
    let iPix14 = (p[1] as i32) + (p[4] as i32);
    let iPix23 = (p[2] as i32) + (p[3] as i32);
    iPix05 - (iPix14 * 5) + (iPix23 * 20)
}

/// `WIDTH` bytes of each of `height` rows, source to destination.
///
/// The width is a const parameter and not an argument: with a runtime length,
/// `copy_from_slice` lowers to a `_platform_memmove` *call* per row; with the width
/// const, the whole row is one pair of wide loads and stores — which is what the C++
/// `LD64`/`ST64A8` pairs were hand-written to get. This path carries the zero-MV
/// block, the commonest luma case there is.
///
/// The bounds check lands once per row either way.
#[inline(always)]
pub(crate) fn copy_rows<const WIDTH: usize, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    height: usize,
) {
    for dy in 0..height as isize {
        let sv = src.row_view(dy, 0, WIDTH);
        let s: &[u8; WIDTH] = (&*sv).try_into().unwrap();
        let d: &mut [u8; WIDTH] = dst.row_mut(dy, 0, WIDTH).try_into().unwrap();
        *d = *s;
    }
}

/// C++: `McCopyWidthEq2_c` — chroma only, the one width the copy path narrows to.
#[inline(always)]
pub fn mc_copy_width_eq2<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<2, _>(src, dst, height);
}

/// C++: `McCopyWidthEq4_c`.
#[inline(always)]
pub fn mc_copy_width_eq4<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<4, _>(src, dst, height);
}

/// C++: `McCopyWidthEq8_c`.
#[inline(always)]
pub fn mc_copy_width_eq8<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<8, _>(src, dst, height);
}

/// C++: `McCopyWidthEq16_c`.
#[inline(always)]
pub fn mc_copy_width_eq16<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<16, _>(src, dst, height);
}

/// The width `McCopy_c` actually copies for a nominal `width`.
///
/// The C++ dispatches on the exact value and treats **everything that is not 16, 8
/// or 4 as 2** — the comment there reads "here iWidth == 2". Reproduced rather than
/// generalised to a `width`-byte copy: a caller passing 3 gets two bytes from the
/// C++ and would get three from the obvious rewrite.
#[inline(always)]
fn copy_width(width: usize) -> usize {
    match width {
        16 => 16,
        8 => 8,
        4 => 4,
        _ => 2,
    }
}

/// C++: `McCopy_c`.
#[inline(always)]
pub fn mc_copy<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    // Dispatched exactly as the C++ dispatches, and for the same reason it does:
    // each arm is a constant-width copy. See [`copy_rows`].
    match width {
        16 => copy_rows::<16, _>(src, dst, height),
        8 => copy_rows::<8, _>(src, dst, height),
        4 => copy_rows::<4, _>(src, dst, height),
        _ => copy_rows::<2, _>(src, dst, height),
    }
}

/// C++: `PixelAvg_c` — the rounded average of two surfaces, `SMcFunc::pfSampleAveraging`.
#[inline(always)]
pub fn pixel_avg_c<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let ra = a.row_view(dy, 0, width);
        let rb = b.row_view(dy, 0, width);
        let out = dst.row_mut(dy, 0, width);
        for j in 0..width {
            out[j] = (((ra[j] as u32) + (rb[j] as u32) + 1) >> 1) as u8;
        }
    }
}

/// Rounded average of two surfaces, dispatching to SSE2 if available.
#[inline(always)]
pub fn pixel_avg<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if crate::simd::has_sse2() {
        crate::simd::x86_64::mc::pixel_avg_sse2(dst, a, b, width, height);
        return;
    }
    pixel_avg_c(dst, a, b, width, height);
}

/// C++: `McHorVer20_c` — the horizontal half-pel filter, `(2, 0)` in quarter-pel.
///
/// Reads `x` in `-2 .. width + 3`, `y` in `0 .. height`.
#[inline(always)]
pub fn mc_hor_ver20_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let row = src.row_view(dy, -2, width + 5);
        let out = dst.row_mut(dy, 0, width);
        for (o, w) in out.iter_mut().zip(row.windows(6)) {
            *o = WelsClip1((filter_input_8bit(w.try_into().unwrap()) + 16) >> 5);
        }
    }
}

/// Horizontal half-pel filter, dispatching to SSE2 if available.
#[inline(always)]
pub fn mc_hor_ver20<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if crate::simd::has_sse2() {
        crate::simd::x86_64::mc::mc_hor_ver20_sse2(src, dst, width, height);
        return;
    }
    mc_hor_ver20_c(src, dst, width, height);
}

/// C++: `McHorVer02_c` — the vertical half-pel filter, `(0, 2)` in quarter-pel.
///
/// Reads `x` in `0 .. width`, `y` in `-2 .. height + 3`.
#[inline(always)]
pub fn mc_hor_ver02_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        src.row_view(-2, 0, width),
        src.row_view(-1, 0, width),
        src.row_view(0, 0, width),
        src.row_view(1, 0, width),
        src.row_view(2, 0, width),
    );
    for dy in 0..height as isize {
        let r5 = src.row_view(dy + 3, 0, width);
        let out = dst.row_mut(dy, 0, width);
        for ((((((o, &a), &b), &c), &d), &e), &f) in out
            .iter_mut()
            .zip(r0.iter())
            .zip(r1.iter())
            .zip(r2.iter())
            .zip(r3.iter())
            .zip(r4.iter())
            .zip(r5.iter())
        {
            *o = WelsClip1((filter_input_8bit(&[a, b, c, d, e, f]) + 16) >> 5);
        }
        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
    }
}

/// Vertical half-pel filter, dispatching to SSE2 if available.
#[inline(always)]
pub fn mc_hor_ver02<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if crate::simd::has_sse2() {
        crate::simd::x86_64::mc::mc_hor_ver02_sse2(src, dst, width, height);
        return;
    }
    mc_hor_ver02_c(src, dst, width, height);
}

/// C++: `McHorVer22_c` — the centre half-pel filter, `(2, 2)` in quarter-pel:
/// vertical 6-tap into 16-bit intermediates, then horizontal 6-tap over those.
///
/// Reads `x` in `-2 .. width + 3`, `y` in `-2 .. height + 3`.
#[inline(always)]
pub fn mc_hor_ver22_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut iTmp = [0i16; 17 + 5];
    let n = width + 5;
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        src.row_view(-2, -2, n),
        src.row_view(-1, -2, n),
        src.row_view(0, -2, n),
        src.row_view(1, -2, n),
        src.row_view(2, -2, n),
    );
    for dy in 0..height as isize {
        let r5 = src.row_view(dy + 3, -2, n);
        for ((((((t, &a), &b), &c), &d), &e), &f) in iTmp[..n]
            .iter_mut()
            .zip(r0.iter())
            .zip(r1.iter())
            .zip(r2.iter())
            .zip(r3.iter())
            .zip(r4.iter())
            .zip(r5.iter())
        {
            *t = filter_input_8bit(&[a, b, c, d, e, f]) as i16;
        }
        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
        let out = dst.row_mut(dy, 0, width);
        for (o, w) in out.iter_mut().zip(iTmp[..n].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(w.try_into().unwrap()) + 512) >> 10);
        }
    }
}

/// Center half-pel filter, dispatching to SSE2 if available.
#[inline(always)]
pub fn mc_hor_ver22<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if crate::simd::has_sse2() {
        crate::simd::x86_64::mc::mc_hor_ver22_sse2(src, dst, width, height);
        return;
    }
    mc_hor_ver22_c(src, dst, width, height);
}

/// A `16`-stride scratch surface for the quarter-pel kernels — the C++
/// `uint8_t uiTmp[256]`, which is why luma MC blocks are at most 16 wide and tall.
#[inline(always)]
fn scratch() -> [u8; 256] {
    [0u8; 256]
}

/// C++: `McHorVer01_c`.
#[inline(never)]
pub fn mc_hor_ver01_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver02_c(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg_c(dst, src, &PlaneCursor::new(&tmp, 0, 16), width, height);
}

/// C++: `McHorVer03_c`.
#[inline(never)]
pub fn mc_hor_ver03_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver02_c(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &src.advance(0, 1),
        &PlaneCursor::new(&tmp, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer10_c`.
#[inline(never)]
pub fn mc_hor_ver10_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver20_c(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg_c(dst, src, &PlaneCursor::new(&tmp, 0, 16), width, height);
}

/// C++: `McHorVer11_c`.
#[inline(never)]
pub fn mc_hor_ver11_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_c(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver02_c(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer12_c`.
#[inline(never)]
pub fn mc_hor_ver12_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    mc_hor_ver02_c(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    mc_hor_ver22_c(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&ver, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer13_c`.
#[inline(never)]
pub fn mc_hor_ver13_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_c(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver02_c(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer21_c`.
#[inline(never)]
pub fn mc_hor_ver21_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    mc_hor_ver20_c(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver22_c(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer23_c`.
#[inline(never)]
pub fn mc_hor_ver23_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    mc_hor_ver20_c(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver22_c(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer30_c`.
#[inline(never)]
pub fn mc_hor_ver30_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    mc_hor_ver20_c(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &src.advance(1, 0),
        &PlaneCursor::new(&hor, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer31_c`.
#[inline(never)]
pub fn mc_hor_ver31_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_c(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver02_c(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer32_c`.
#[inline(never)]
pub fn mc_hor_ver32_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    mc_hor_ver02_c(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    mc_hor_ver22_c(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&ver, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer33_c`.
#[inline(never)]
pub fn mc_hor_ver33_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_c(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver02_c(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    pixel_avg_c(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McLuma_c` — quarter-pel dispatch on the low two bits of each MV component.
///
/// The arms are in `[iMvX & 3][iMvY & 3]` order.
#[inline(always)]
pub fn mc_luma_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    match ((mv_x & 0x03) as u8, (mv_y & 0x03) as u8) {
        (0, 0) => mc_copy(src, dst, width, height),
        (0, 1) => mc_hor_ver01_c(src, dst, width, height),
        (0, 2) => mc_hor_ver02_c(src, dst, width, height),
        (0, 3) => mc_hor_ver03_c(src, dst, width, height),
        (1, 0) => mc_hor_ver10_c(src, dst, width, height),
        (1, 1) => mc_hor_ver11_c(src, dst, width, height),
        (1, 2) => mc_hor_ver12_c(src, dst, width, height),
        (1, 3) => mc_hor_ver13_c(src, dst, width, height),
        (2, 0) => mc_hor_ver20_c(src, dst, width, height),
        (2, 1) => mc_hor_ver21_c(src, dst, width, height),
        (2, 2) => mc_hor_ver22_c(src, dst, width, height),
        (2, 3) => mc_hor_ver23_c(src, dst, width, height),
        (3, 0) => mc_hor_ver30_c(src, dst, width, height),
        (3, 1) => mc_hor_ver31_c(src, dst, width, height),
        (3, 2) => mc_hor_ver32_c(src, dst, width, height),
        _ => mc_hor_ver33_c(src, dst, width, height),
    }
}

#[inline(always)]
pub fn mc_luma<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if crate::simd::has_sse2() {
        crate::simd::x86_64::mc::mc_luma_sse2(src, dst, mv_x, mv_y, width, height);
        return;
    }
    mc_luma_c(src, dst, mv_x, mv_y, width, height);
}

/// C++: `McChromaWithFragMv_c` — bilinear chroma interpolation at eighth-pel.
///
/// Reads `x` in `0 .. width + 1`, `y` in `0 .. height + 1`.
#[inline(always)]
pub fn mc_chroma_with_frag_mv<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if width == 0 {
        return;
    }
    let pABCD = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    let iA = pABCD[0] as i32;
    let iB = pABCD[1] as i32;
    let iC = pABCD[2] as i32;
    let iD = pABCD[3] as i32;

    for dy in 0..height as isize {
        let r0 = src.row_view(dy, 0, width + 1);
        let r1 = src.row_view(dy + 1, 0, width + 1);
        let out = dst.row_mut(dy, 0, width);
        for j in 0..width {
            out[j] = ((iA * (r0[j] as i32)
                + iB * (r0[j + 1] as i32)
                + iC * (r1[j] as i32)
                + iD * (r1[j + 1] as i32)
                + 32)
                >> 6) as u8;
        }
    }
}

/// C++: `McChroma_c` — the copy path when the eighth-pel fraction is zero.
#[inline(always)]
pub fn mc_chroma_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if (mv_x & 0x07) == 0 && (mv_y & 0x07) == 0 {
        mc_copy(src, dst, width, height);
    } else {
        mc_chroma_with_frag_mv(src, dst, mv_x, mv_y, width, height);
    }
}

#[inline(always)]
pub fn mc_chroma<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if crate::simd::has_sse2() {
        crate::simd::x86_64::mc::mc_chroma_sse2(src, dst, mv_x, mv_y, width, height);
        return;
    }
    mc_chroma_c(src, dst, mv_x, mv_y, width, height);
}

// ============================================================================
// The same-picture arm — motion compensation in one borrow
// ============================================================================
//
// **What this family is for.** A malformed stream can put the picture being decoded
// into its own reference list (`decoder/pic_queue.rs`), and the C++ resolves that
// entry and motion-compensates from the picture it is writing. Every kernel above
// takes two cursors because the two pictures are two allocations at every
// *well-formed* call site; here they are one, so there is no second cursor to
// build. `decoder/pic_queue.rs`'s `PicRefs::classify` is what tells the two apart,
// and these are what its `RefSlot::Current` arm runs.
//
// **The shape.** One `PlaneCursorMut` anchored at the destination block, plus the
// source anchor `(sx, sy)` *relative to that same anchor* — legal because one plane
// has one stride, so a source displaced by a motion vector is a relative offset and
// nothing else. Reads go through `at`, writes through `set`, and the two interleave
// exactly as the C++'s `pSrc[j]` / `pDst[j]` do.
//
// **Why the ordering is the whole contract.** When the source window overlaps the
// destination block, motion compensation reads samples it has already written, and
// *which* ones depends on the loop order. So these reproduce the C++'s order rather
// than a faster equivalent: raster within each output row for the direct filters, a
// per-output-row 16-bit intermediate for the centre kernel, and `copy_within` (not a
// block copy) for the integer-MV path. The composite quarter-pel kernels are the
// cheap case and are *not* rewritten: they already build their intermediates into
// 16-stride scratch surfaces, so the source reads all finish before the first
// destination write, and they reuse the two-cursor kernels above through a shared
// borrow of this cursor. Only the four kernels that write the destination straight
// from the source are index-based.
//
// **Cold by construction** — malformed input only — so the spelling is chosen for
// soundness, not speed: `at` per sample where the two-cursor form hoists a row.

/// The six horizontal taps the 6-tap filter reads for output column `x` of row `y`.
#[inline(always)]
fn taps_h(p: &PlaneCursorMut<'_>, x: isize, y: isize) -> [u8; 6] {
    [
        p.at(x - 2, y),
        p.at(x - 1, y),
        p.at(x, y),
        p.at(x + 1, y),
        p.at(x + 2, y),
        p.at(x + 3, y),
    ]
}

/// The six vertical taps, for the same output sample.
#[inline(always)]
fn taps_v(p: &PlaneCursorMut<'_>, x: isize, y: isize) -> [u8; 6] {
    [
        p.at(x, y - 2),
        p.at(x, y - 1),
        p.at(x, y),
        p.at(x, y + 1),
        p.at(x, y + 2),
        p.at(x, y + 3),
    ]
}

/// [`mc_copy`]'s same-plane form — `McCopy_c` when source and destination are one
/// allocation. The width narrowing is [`copy_width`]'s, so a caller passing 3 moves
/// two samples here exactly as it does there.
#[inline(never)]
fn same_copy(p: &mut PlaneCursorMut<'_>, sx: isize, sy: isize, width: usize, height: usize) {
    let w = copy_width(width);
    for dy in 0..height as isize {
        p.copy_row_within(sx, sy + dy, dy, w);
    }
}

/// [`mc_hor_ver20`]'s same-plane form.
#[inline(never)]
fn same_hor_ver20(p: &mut PlaneCursorMut<'_>, sx: isize, sy: isize, width: usize, height: usize) {
    for dy in 0..height as isize {
        for dx in 0..width as isize {
            let t = taps_h(p, sx + dx, sy + dy);
            p.set(dx, dy, WelsClip1((filter_input_8bit(&t) + 16) >> 5));
        }
    }
}

/// [`mc_hor_ver02`]'s same-plane form.
#[inline(never)]
fn same_hor_ver02(p: &mut PlaneCursorMut<'_>, sx: isize, sy: isize, width: usize, height: usize) {
    for dy in 0..height as isize {
        for dx in 0..width as isize {
            let t = taps_v(p, sx + dx, sy + dy);
            p.set(dx, dy, WelsClip1((filter_input_8bit(&t) + 16) >> 5));
        }
    }
}

/// [`mc_hor_ver22`]'s same-plane form — the `iTmp` row is the C++'s own, refilled
/// per output row, which is what puts each row's reads before that row's writes and
/// each row's writes before the *next* row's reads.
#[inline(never)]
fn same_hor_ver22(p: &mut PlaneCursorMut<'_>, sx: isize, sy: isize, width: usize, height: usize) {
    let mut iTmp = [0i16; 17 + 5];
    let n = width + 5;
    for dy in 0..height as isize {
        for (j, t) in iTmp[..n].iter_mut().enumerate() {
            let taps = taps_v(p, sx + j as isize - 2, sy + dy);
            *t = filter_input_8bit(&taps) as i16;
        }
        for dx in 0..width {
            let w: &[i16; 6] = iTmp[dx..][..6].try_into().unwrap();
            p.set(
                dx as isize,
                dy,
                WelsClip1((hor_filter_input_16bit(w) + 512) >> 10),
            );
        }
    }
}

/// [`pixel_avg`]'s same-plane form for the quarter-pel kernels whose *first* input
/// is the source picture itself — `(0,1)`, `(0,3)`, `(1,0)`, `(3,0)`. The other
/// eight average two scratch surfaces and need nothing from here.
#[inline(never)]
fn same_avg_with_src(
    p: &mut PlaneCursorMut<'_>,
    sx: isize,
    sy: isize,
    b: &[u8; 256],
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        for dx in 0..width as isize {
            let a = p.at(sx + dx, sy + dy) as u32;
            let v = ((a + (b[(dy as usize) * 16 + dx as usize] as u32) + 1) >> 1) as u8;
            p.set(dx, dy, v);
        }
    }
}

/// A read cursor on the source window, for the phases that only read.
#[inline(always)]
fn same_src<'p>(p: &'p PlaneCursorMut<'_>, sx: isize, sy: isize) -> PlaneCursor<'p> {
    p.as_ref().advance(sx, sy)
}

/// [`mc_luma`]'s same-plane form — the same sixteen arms in the same order.
///
/// `(sx, sy)` is the source anchor relative to `p`'s own, in samples.
pub fn mc_luma_same(
    p: &mut PlaneCursorMut<'_>,
    sx: isize,
    sy: isize,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    // The eight composite arms: both averaged inputs are 16-stride scratch, so the
    // source reads finish inside `hor`/`ver`/`ctr` and the destination writes come
    // after — the two-cursor kernels are reused verbatim, through a shared borrow.
    macro_rules! avg_two {
        ($fa:ident($dxa:expr, $dya:expr), $fb:ident($dxb:expr, $dyb:expr)) => {{
            let (mut a, mut b) = (scratch(), scratch());
            {
                let src = same_src(p, sx, sy);
                $fa(
                    &src.advance($dxa, $dya),
                    &mut PlaneCursorMut::new(&mut a, 0, 16),
                    width,
                    height,
                );
                $fb(
                    &src.advance($dxb, $dyb),
                    &mut PlaneCursorMut::new(&mut b, 0, 16),
                    width,
                    height,
                );
            }
            pixel_avg(
                p,
                &PlaneCursor::new(&a, 0, 16),
                &PlaneCursor::new(&b, 0, 16),
                width,
                height,
            );
        }};
    }
    // The four arms that average the source picture against one scratch surface.
    macro_rules! avg_src {
        ($f:ident, $adx:expr, $ady:expr) => {{
            let mut t = scratch();
            {
                let src = same_src(p, sx, sy);
                $f(&src, &mut PlaneCursorMut::new(&mut t, 0, 16), width, height);
            }
            same_avg_with_src(p, sx + $adx, sy + $ady, &t, width, height);
        }};
    }

    match ((mv_x & 0x03) as u8, (mv_y & 0x03) as u8) {
        (0, 0) => same_copy(p, sx, sy, width, height),
        (0, 1) => avg_src!(mc_hor_ver02, 0, 0),
        (0, 2) => same_hor_ver02(p, sx, sy, width, height),
        (0, 3) => avg_src!(mc_hor_ver02, 0, 1),
        (1, 0) => avg_src!(mc_hor_ver20, 0, 0),
        (1, 1) => avg_two!(mc_hor_ver20(0, 0), mc_hor_ver02(0, 0)),
        (1, 2) => avg_two!(mc_hor_ver02(0, 0), mc_hor_ver22(0, 0)),
        (1, 3) => avg_two!(mc_hor_ver20(0, 1), mc_hor_ver02(0, 0)),
        (2, 0) => same_hor_ver20(p, sx, sy, width, height),
        (2, 1) => avg_two!(mc_hor_ver20(0, 0), mc_hor_ver22(0, 0)),
        (2, 2) => same_hor_ver22(p, sx, sy, width, height),
        (2, 3) => avg_two!(mc_hor_ver20(0, 1), mc_hor_ver22(0, 0)),
        (3, 0) => avg_src!(mc_hor_ver20, 1, 0),
        (3, 1) => avg_two!(mc_hor_ver20(0, 0), mc_hor_ver02(1, 0)),
        (3, 2) => avg_two!(mc_hor_ver02(1, 0), mc_hor_ver22(0, 0)),
        _ => avg_two!(mc_hor_ver20(0, 1), mc_hor_ver02(1, 0)),
    }
}

/// [`mc_chroma`]'s same-plane form.
pub fn mc_chroma_same(
    p: &mut PlaneCursorMut<'_>,
    sx: isize,
    sy: isize,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if (mv_x & 0x07) == 0 && (mv_y & 0x07) == 0 {
        same_copy(p, sx, sy, width, height);
        return;
    }
    if width == 0 {
        return;
    }
    let pABCD = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    let (iA, iB, iC, iD) = (
        pABCD[0] as i32,
        pABCD[1] as i32,
        pABCD[2] as i32,
        pABCD[3] as i32,
    );
    for dy in 0..height as isize {
        for dx in 0..width as isize {
            let (x, y) = (sx + dx, sy + dy);
            let v = ((iA * (p.at(x, y) as i32)
                + iB * (p.at(x + 1, y) as i32)
                + iC * (p.at(x, y + 1) as i32)
                + iD * (p.at(x + 1, y + 1) as i32)
                + 32)
                >> 6) as u8;
            p.set(dx, dy, v);
        }
    }
}

// ============================================================================
// Read reaches and spans
// ============================================================================
//
// A kernel reaches past the block it is given: the 6-tap filter needs two samples
// before and three after each output sample. The source has already been displaced
// by a motion vector, so the reach is legal only because the caller clamped that
// vector first. The decoder's clamp is `BaseMC`
// (`decoder/decode_slice.rs:1069-1091`), and it is *exactly* calibrated to this
// reach:
//
// ```text
// const PADDING_LENGTH: i32 = 32;
// iFullMVx = WELS_CLIP3(iFullMVx, (-PADDING_LENGTH + 2) * 4,
//                       (pMCRefMem.iPicWidth  + PADDING_LENGTH - 19) * 4);
// iFullMVy = WELS_CLIP3(iFullMVy, (-PADDING_LENGTH + 2) * 4,
//                       (pMCRefMem.iPicHeight + PADDING_LENGTH - 19) * 4);
// pSrcY = pMCRefMem.pSrcY.offset((iFullMVx >> 2) + (iFullMVy >> 2) * iSrcLineLuma);
// ```
//
// Read the arithmetic out: the integer part of the vector lands in
// `-30 ..= width + 13`, so a 16-wide block reaching `x - 2` at the low end and
// `x + 16 + 3` at the high end touches `-32 ..= width + 32` — precisely the
// 32-sample luma border `AllocPicture` allocates (`decoder/pic_queue.rs`), with
// nothing to spare at either end. The `+ 2` and the `- 19` in the clamp are that
// margin: `19 == 16 + 3`. Chroma is the same argument at half scale against the
// 16-sample chroma border.
//
// The encoder's callers are a different family of buffers: ME refinement filters
// out of the reference picture into `pBufferInterPredMe` scratch
// (`encoder/md.rs:1043-1046`), and the search window is bounded before the call
// rather than by a clamp inside it.

/// The samples a kernel reads around `pSrc`'s `(0, 0)`: `x` in
/// `-left .. width + right`, `y` in `-top .. height + bottom`.
#[derive(Clone, Copy)]
struct Reach {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

/// Copy path: the block and nothing else.
const R_COPY: Reach = Reach { left: 0, top: 0, right: 0, bottom: 0 };
/// Horizontal 6-tap: two samples left, three right.
const R_HOR: Reach = Reach { left: 2, top: 0, right: 3, bottom: 0 };
/// Vertical 6-tap: two rows above, three below.
const R_VER: Reach = Reach { left: 0, top: 2, right: 0, bottom: 3 };
/// Both, which is also the union over every quarter-pel kernel.
const R_CEN: Reach = Reach { left: 2, top: 2, right: 3, bottom: 3 };
/// Bilinear chroma: one sample right and one row below, for the `(1 - a)` terms.
const R_CHROMA: Reach = Reach { left: 0, top: 0, right: 1, bottom: 1 };

/// Per-kernel read reach, in `[iMvX & 3][iMvY & 3]` order.
///
/// It is not one reach for all sixteen: `McHorVer10_c` reads no row outside its
/// block and `McHorVer13_c` reads five.
///
/// **`static`, not `const`, and that is worth 8% of decode time.** A `const` is
/// substituted at each use, so `LUMA_REACH[x][y]` with runtime indices makes the
/// compiler materialise all sixteen entries on the stack at every `McLuma_c` call
/// and then index that copy — 64 stores to read four words. As a `static` it lives
/// in rodata and the same expression is one load.
static LUMA_REACH: [[Reach; 4]; 4] = [
    [R_COPY, R_VER, R_VER, R_VER],
    [R_HOR, R_CEN, R_CEN, R_CEN],
    [R_HOR, R_CEN, R_CEN, R_CEN],
    [R_HOR, R_CEN, R_CEN, R_CEN],
];

/// `(slice length, cursor centre)` for a source slice anchored at
/// `pSrc - top*stride - left`.
///
/// This and [`block_span`] are the only places in the module where a span is
/// computed.
#[inline]
fn src_span(stride: usize, width: usize, height: usize, r: Reach) -> (usize, usize) {
    let center = r.top * stride + r.left;
    let len = center + (height + r.bottom - 1) * stride + width + r.right;
    (len, center)
}

/// Bytes spanned by a `width` x `height` block at `stride`, from its own `(0, 0)` —
/// the destination surface of every kernel here, and the source of the ones that
/// read nothing outside their block.
#[inline]
fn block_span(stride: usize, width: usize, height: usize) -> usize {
    (height - 1) * stride + width
}

/// C++: `InitMcFunc`, `codec/common/src/mc.cpp` — both codecs call it at open time.
pub fn InitMcFunc(pMcFuncs: &mut SMcFunc, uiCpuFlag: u32) {
    *pMcFuncs = SMcFunc::default();
    #[cfg(target_arch = "x86_64")]
    if (uiCpuFlag & crate::common::cpu_core::WELS_CPU_SSE2) != 0 {
        pMcFuncs.pfLumaHalfpelHor = |s, d, w, h| crate::simd::x86_64::mc::mc_hor_ver20_sse2(s, d, w, h);
        pMcFuncs.pfLumaHalfpelVer = |s, d, w, h| crate::simd::x86_64::mc::mc_hor_ver02_sse2(s, d, w, h);
        pMcFuncs.pfLumaHalfpelCen = |s, d, w, h| crate::simd::x86_64::mc::mc_hor_ver22_sse2(s, d, w, h);
        pMcFuncs.pfSampleAveraging = |dst, a, b, w, h| crate::simd::x86_64::mc::pixel_avg_sse2(dst, a, b, w, h);
        pMcFuncs.pMcChromaFunc = |s, d, mx, my, w, h| crate::simd::x86_64::mc::mc_chroma_sse2(s, d, mx, my, w, h);
        pMcFuncs.pMcLumaFunc = |s, d, mx, my, w, h| crate::simd::x86_64::mc::mc_luma_sse2(s, d, mx, my, w, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic plane: one buffer, `STRIDE` bytes per row, `ROWS` rows.
    #[cfg(test)]
    const STRIDE: usize = 64;
    #[cfg(test)]
    const ROWS: usize = 64;

    #[cfg(test)]
    fn filled_plane() -> Vec<u8> {
        // A cheap deterministic fill with no run-length structure, so a kernel that
        // reads the wrong tap cannot accidentally agree.
        let mut v = vec![0u8; STRIDE * ROWS];
        let mut s: u32 = 0x1234_5678;
        for b in v.iter_mut() {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *b = (s >> 16) as u8;
        }
        v
    }

    /// The block shapes `BaseMC` actually dispatches, luma and chroma.
    #[cfg(test)]
    const LUMA_SHAPES: [(usize, usize); 7] =
        [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)];

    /// **The same-picture arm equals the two-cursor arm wherever both are defined.**
    ///
    /// These kernels exist because a source that *is* the destination cannot be
    /// spelled as a cursor pair; that makes the two forms incomparable exactly on the
    /// overlapping case and comparable everywhere else. This drives every one of the
    /// sixteen quarter-pel arms at every block shape with the two windows disjoint
    /// inside one buffer, and requires the whole buffer — not just the block — to come
    /// out equal, so a kernel writing outside its block is caught too.
    #[test]
    fn same_picture_luma_matches_the_two_cursor_kernels_when_the_windows_are_disjoint() {
        let base = filled_plane();
        // Source at (8, 8), destination at (8, 40): the filter reaches y in 5..27 and
        // the block writes y in 40..56, so nothing overlaps.
        let (src_c, dst_c) = (8 * STRIDE + 8, 40 * STRIDE + 8);
        for (width, height) in LUMA_SHAPES {
            for mv_x in 0..4i16 {
                for mv_y in 0..4i16 {
                    let mut one = base.clone();
                    mc_luma_same(
                        &mut PlaneCursorMut::new(&mut one, dst_c, STRIDE),
                        0,
                        -32,
                        mv_x,
                        mv_y,
                        width,
                        height,
                    );

                    let src = base.clone();
                    let mut two = base.clone();
                    mc_luma(
                        &PlaneCursor::new(&src, src_c, STRIDE),
                        &mut PlaneCursorMut::new(&mut two, dst_c, STRIDE),
                        mv_x,
                        mv_y,
                        width,
                        height,
                    );

                    assert_eq!(one, two, "luma ({mv_x}, {mv_y}) at {width}x{height}");
                }
            }
        }
    }

    /// [`mc_chroma_same`] against [`mc_chroma`], over all sixty-four eighth-pel
    /// fractions — the copy arm included, because `(0, 0)` is the one that reaches
    /// [`same_copy`].
    #[test]
    fn same_picture_chroma_matches_the_two_cursor_kernel_when_the_windows_are_disjoint() {
        let base = filled_plane();
        let (src_c, dst_c) = (8 * STRIDE + 8, 40 * STRIDE + 8);
        for (width, height) in [(8usize, 8usize), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)] {
            for mv_x in 0..8i16 {
                for mv_y in 0..8i16 {
                    let mut one = base.clone();
                    mc_chroma_same(
                        &mut PlaneCursorMut::new(&mut one, dst_c, STRIDE),
                        0,
                        -32,
                        mv_x,
                        mv_y,
                        width,
                        height,
                    );

                    let src = base.clone();
                    let mut two = base.clone();
                    mc_chroma(
                        &PlaneCursor::new(&src, src_c, STRIDE),
                        &mut PlaneCursorMut::new(&mut two, dst_c, STRIDE),
                        mv_x,
                        mv_y,
                        width,
                        height,
                    );

                    assert_eq!(one, two, "chroma ({mv_x}, {mv_y}) at {width}x{height}");
                }
            }
        }
    }

    /// **The overlapping case, where the two forms are not comparable and the C++'s
    /// loop order is the whole specification.**
    ///
    /// The reference here is a transliteration of `McHorVer20_c` over one buffer —
    /// raster order, each output sample written before the next one's taps are read —
    /// so what this pins is the property the two-cursor kernel cannot state: that
    /// `same_hor_ver20` reads a sample it has already written exactly when the C++
    /// does. The geometry puts the destination two rows below the source, inside the
    /// filter's own vertical reach.
    #[test]
    fn the_overlapping_arm_reproduces_the_c_loop_order() {
        let base = filled_plane();
        let dst_c = 10 * STRIDE + 8;
        let (width, height) = (16usize, 16usize);
        // Source one row above the destination: rows 9..25 read, rows 10..26 written.
        let (sx, sy) = (0isize, -1isize);

        let mut got = base.clone();
        same_hor_ver20(
            &mut PlaneCursorMut::new(&mut got, dst_c, STRIDE),
            sx,
            sy,
            width,
            height,
        );

        let mut want = base.clone();
        for i in 0..height as isize {
            for j in 0..width as isize {
                let s = (dst_c as isize + (sy + i) * STRIDE as isize + sx + j) as usize;
                let taps: [u8; 6] = [
                    want[s - 2],
                    want[s - 1],
                    want[s],
                    want[s + 1],
                    want[s + 2],
                    want[s + 3],
                ];
                let d = (dst_c as isize + i * STRIDE as isize + j) as usize;
                want[d] = WelsClip1((filter_input_8bit(&taps) + 16) >> 5);
            }
        }
        assert_eq!(got, want);
        assert_ne!(got, base, "the geometry has to actually write something");
    }

    /// [`PlaneCursorMut::copy_row_within`] is memmove, not memcpy: the integer-MV arm
    /// of a self-referencing macroblock copies a row onto itself displaced by a few
    /// samples, and the C++'s `LD64`/`ST64` pairs make that overlap defined by
    /// accident where a Rust block copy would make it UB.
    #[test]
    fn the_copy_arm_is_defined_when_the_row_overlaps_itself() {
        let mut buf = vec![0u8; STRIDE * 4];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        let want: Vec<u8> = {
            let mut w = buf.clone();
            let (s, d) = (STRIDE + 8 + 3, STRIDE + 8);
            let row: Vec<u8> = w[s..s + 16].to_vec();
            w[d..d + 16].copy_from_slice(&row);
            w
        };
        same_copy(
            &mut PlaneCursorMut::new(&mut buf, STRIDE + 8, STRIDE),
            3,
            0,
            16,
            1,
        );
        assert_eq!(buf, want);
    }

    /// `InitMcFunc` installs SIMD kernels when `WELS_CPU_SSE2` is present on x86_64,
    /// and scalar defaults otherwise.
    ///
    /// **Why this compares two tables instead of a table against named
    /// functions.** The obvious assert-map — `t.pMcLumaFunc as usize
    /// == McLuma_c as usize` — is *unsound for these six functions*. Four of
    /// them are `#[inline(always)]`, and an `#[inline(always)]` function whose
    /// address is taken gets instantiated locally in whatever codegen unit takes
    /// it: the integration-test crate gets its own copy, and so does this
    /// `tests` submodule. Neither address is the one `InitMcFunc` stored.
    ///
    /// Both addresses here come from the same `InitMcFunc` instantiation, so
    /// the comparison is meaningful. Not under Miri: it mints a fresh synthetic
    /// address for each reified function pointer, so even two calls of the
    /// *same* installer compare unequal there.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn init_mc_func_cpu_flags() {
        use crate::common::cpu_core::*;
        let scalar_flags: [u32; 5] = [
            0, WELS_CPU_NEON, WELS_CPU_MMI, WELS_CPU_LSX, WELS_CPU_MMX,
        ];
        let mut base = SMcFunc::default();
        InitMcFunc(&mut base, 0);
        let addrs = |t: &SMcFunc| -> [usize; 6] {
            [
                t.pfLumaHalfpelHor as usize,
                t.pfLumaHalfpelVer as usize,
                t.pfLumaHalfpelCen as usize,
                t.pfSampleAveraging as usize,
                t.pMcChromaFunc as usize,
                t.pMcLumaFunc as usize,
            ]
        };
        const NAMES: [&str; 6] = [
            "pfLumaHalfpelHor", "pfLumaHalfpelVer", "pfLumaHalfpelCen",
            "pfSampleAveraging", "pMcChromaFunc", "pMcLumaFunc",
        ];
        let scalar_want = addrs(&base);
        for flag in scalar_flags {
            let mut t = SMcFunc::default();
            InitMcFunc(&mut t, flag);
            for (i, (got, expected)) in addrs(&t).into_iter().zip(scalar_want).enumerate() {
                assert_eq!(
                    got, expected,
                    "scalar cpu flag {flag:#x} selected a different function for slot {}",
                    NAMES[i]
                );
            }
        }

        #[cfg(target_arch = "x86_64")]
        {
            let mut sse2_base = SMcFunc::default();
            InitMcFunc(&mut sse2_base, WELS_CPU_SSE2);
            let sse2_want = addrs(&sse2_base);
            for (i, (got, expected)) in sse2_want.into_iter().zip(scalar_want).enumerate() {
                assert_ne!(
                    got, expected,
                    "SSE2 flag should install a SIMD function for slot {}",
                    NAMES[i]
                );
            }

            let sse2_flags: [u32; 6] = [
                WELS_CPU_SSE2,
                WELS_CPU_SSE2 | WELS_CPU_SSE41,
                WELS_CPU_SSE2 | WELS_CPU_SSE42,
                WELS_CPU_SSE2 | WELS_CPU_AVX,
                WELS_CPU_SSE2 | WELS_CPU_AVX2,
                u32::MAX,
            ];
            for flag in sse2_flags {
                let mut t = SMcFunc::default();
                InitMcFunc(&mut t, flag);
                for (i, (got, expected)) in addrs(&t).into_iter().zip(sse2_want).enumerate() {
                    assert_eq!(
                        got, expected,
                        "SSE2 cpu flag {flag:#x} selected a different function for slot {}",
                        NAMES[i]
                    );
                }
            }
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`.
pub use crate::common::cpu_core::{WELS_CPU_3DNOW, WELS_CPU_3DNOWEXT, WELS_CPU_ALTIVEC, WELS_CPU_ARMv7, WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MMX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE, WELS_CPU_SSE2, WELS_CPU_SSE3, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3, WELS_CPU_VFPv3};
