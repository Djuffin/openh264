//! Motion compensation — luma quarter-pel, chroma eighth-pel, and the copy paths.
#![forbid(unsafe_code)]
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

// CPU feature flags from cpu_core.h

use crate::safe::plane::{BlockRows, PlaneCursor, PlaneCursorMut, RefSamples};

/// The kernel set the dispatch sites below call: `simd::x86_64` or `simd::aarch64` by default,
/// `simd::wide` under `--features wide`. Imported rather than spelled in full at each
/// site because the kernels share their names with the scalars in this module — which
/// is the point of the naming, and the reason the module qualifier has to stay.
use crate::simd::kernels;

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

/// A `WIDTH`x`HEIGHT` block, source to destination, **through one bounds-checked
/// span per operand**.
///
/// Both dimensions are const parameters and neither is an argument. The width has to
/// be, or `copy_from_slice` lowers to a `_platform_memmove` *call* per row where the
/// C++ `LD64`/`ST64A8` pairs were hand-written to get one wide load and one wide
/// store; the height has to be, or the row loop stays a loop and `y * stride` stays
/// symbolic, which is the one thing no span length can place inside a span. With
/// both const the loop unrolls, every row offset is a constant, and the checks the
/// two spans paid once are the only checks in the copy — see [`RefSamples::span`]
/// and [`PlaneSpanMut`].
///
/// This path carries the zero-MV block, the commonest luma case there is.
#[inline(always)]
fn copy_block<const WIDTH: usize, const HEIGHT: usize, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<WIDTH, HEIGHT>(0, 0);
    let mut d = dst.span_mut::<WIDTH, HEIGHT>(0, 0);
    for y in 0..HEIGHT {
        *d.row_mut::<WIDTH>(y, 0) = s.row::<WIDTH>(y, 0);
    }
}

/// `WIDTH` bytes of each of `height` rows, source to destination.
///
/// The heights an H.264 block copy can have are 16, 8, 4 and 2 — the luma
/// partitions and their chroma halves — and each is dispatched to a const
/// instantiation of [`copy_block`], which is where the argument for why that matters
/// is written down. Any other height falls back to the row-at-a-time walk, which is
/// what this whole function used to be: correct for every height, one pair of slice
/// checks per row.
#[inline(always)]
pub(crate) fn copy_rows<const WIDTH: usize, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    height: usize,
) {
    match height {
        16 => copy_block::<WIDTH, 16, _>(src, dst),
        8 => copy_block::<WIDTH, 8, _>(src, dst),
        4 => copy_block::<WIDTH, 4, _>(src, dst),
        2 => copy_block::<WIDTH, 2, _>(src, dst),
        _ => {
            note_runtime_shape();
            for dy in 0..height as isize {
                let s = src.row_n::<WIDTH>(dy, 0);
                let d: &mut [u8; WIDTH] = dst.row_mut(dy, 0, WIDTH).try_into().unwrap();
                *d = s;
            }
        }
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

// ============================================================================
// The block shapes, and the dispatch onto them
// ============================================================================
//
// **What the run-time width and height cost.** Every kernel below used to take
// `width` and `height` as arguments and read its input a row at a time through
// [`RefSamples::row_view`], the run-time-length accessor. On a plane cursor that is
// two slice checks a row; on the [`RecCursor`](crate::encoder::rec_view::RecCursor)
// the encoder hands them — a view over shared cells, which cannot lend a `&[u8]` —
// it is an anchor computation, two checks, a zeroed 32-byte `RowBuf` and a
// cell-by-cell copy loop, all before the kernel's own vector load.
//
// [`RefSamples::span`] is the accessor that removes the checks, and it needs the
// block's shape as **const parameters**. So each entry point matches `(width,
// height)` onto a const instantiation, exactly as [`copy_rows`] matches the height
// onto [`copy_block`], and the kernel inside cuts one span per operand and walks it
// with constant row offsets.
//
// **The shape table is the call sites'**, and [`SHAPES_LUMA`], [`SHAPES_REFINE_*`]
// and [`SHAPES_CHROMA`] below are it, written down so a test can drive every entry
// of it and assert that none reached the fallback.
//
// **`SW`, `SH`, and why they are parameters rather than arithmetic.** The horizontal
// filter reads `W + 5` samples a row, the vertical `H + 5` rows, the bilinear chroma
// one extra of each. Stable Rust has no arithmetic in a const-argument position, so
// `span::<{W + 5}, H>` cannot be written; the reach is passed as its own const
// parameter by the dispatch arm, which is the spelling `sad::sample_sad_four_16x16`
// already uses for its `W + 2`. Every arm below pairs them, and
// [`shapes_are_consistent`](tests::shapes_are_consistent) checks the pairing.

/// The seven luma partition shapes: `BaseMC`'s block sizes in the decoder, and the
/// sizes `mc_luma`'s quarter-pel composites run their leaves at.
#[cfg(test)]
pub(crate) const SHAPES_LUMA: [(usize, usize); 7] =
    [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)];

/// `MeRefineFracPixel`'s horizontal filter, `(kiW + 1, kiH)` at the four block sizes
/// the motion search refines (`svc_base_layer_md.rs`; the sub-8x8 partitions are
/// `#if 0` upstream and `unreachable!` here).
#[cfg(test)]
pub(crate) const SHAPES_REFINE_HOR: [(usize, usize); 4] = [(17, 16), (17, 8), (9, 16), (9, 8)];

/// The same refinement's vertical filter, `(kiW, kiH + 1)`.
#[cfg(test)]
pub(crate) const SHAPES_REFINE_VER: [(usize, usize); 4] = [(16, 17), (16, 9), (8, 17), (8, 9)];

/// The same refinement's centre filter, `(kiW + 1, kiH + 1)`.
#[cfg(test)]
pub(crate) const SHAPES_REFINE_CEN: [(usize, usize); 4] = [(17, 17), (17, 9), (9, 17), (9, 9)];

/// The chroma shapes: half of each luma partition, so the decoder's 4x4 partitions
/// reach 2x2 and the encoder's 8x8 ones reach 4x4.
#[cfg(test)]
pub(crate) const SHAPES_CHROMA: [(usize, usize); 7] =
    [(8, 8), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)];

#[cfg(test)]
thread_local! {
    /// Counts the shapes that reached a run-time-width fallback on **this thread**.
    ///
    /// The fallbacks exist so that an unforeseen shape is slow rather than a panic —
    /// the decoder's error-concealment slots hold these kernels and a `match` that
    /// panicked on a shape they passed would be a crash in production. But nothing
    /// the codec itself calls should reach one, and that is a claim worth testing
    /// rather than asserting: `mc_shapes_all_reach_a_const_arm` drives every entry of
    /// the tables above and reads this back. Thread-local because the test suite runs
    /// in parallel.
    pub(crate) static RUNTIME_SHAPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Called from every run-time-shape fallback arm; compiles to nothing outside tests.
#[inline(always)]
pub(crate) fn note_runtime_shape() {
    #[cfg(test)]
    RUNTIME_SHAPES.with(|c| c.set(c.get() + 1));
}

/// Runs `f` and reports how many MC calls inside it fell back to a run-time shape.
#[cfg(test)]
pub(crate) fn runtime_shapes_during(f: impl FnOnce()) -> usize {
    RUNTIME_SHAPES.with(|c| c.set(0));
    f();
    RUNTIME_SHAPES.with(|c| c.get())
}

/// `McHorVer20` and its two `_AVERAGE_WITH_` forms, dispatched onto a const shape.
///
/// `AVG` is 0 for the plain half-pel filter, or the tap the result is averaged with:
/// 2 for quarter-pel `(1, 0)` and 3 for `(3, 0)`.
#[inline(always)]
pub(crate) fn hor_shaped<L: McLeaves, S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    match (width, height) {
        (16, 16) => L::hor::<S, 16, 21, 16, AVG>(src, dst),
        (16, 8) => L::hor::<S, 16, 21, 8, AVG>(src, dst),
        (8, 16) => L::hor::<S, 8, 13, 16, AVG>(src, dst),
        (8, 8) => L::hor::<S, 8, 13, 8, AVG>(src, dst),
        (8, 4) => L::hor::<S, 8, 13, 4, AVG>(src, dst),
        (4, 8) => L::hor::<S, 4, 9, 8, AVG>(src, dst),
        (4, 4) => L::hor::<S, 4, 9, 4, AVG>(src, dst),
        (17, 16) => L::hor::<S, 17, 22, 16, AVG>(src, dst),
        (17, 8) => L::hor::<S, 17, 22, 8, AVG>(src, dst),
        (9, 16) => L::hor::<S, 9, 14, 16, AVG>(src, dst),
        (9, 8) => L::hor::<S, 9, 14, 8, AVG>(src, dst),
        _ => {
            note_runtime_shape();
            L::hor_any::<S, AVG>(src, dst, width, height)
        }
    }
}

/// `McHorVer02` and its `_AVERAGE_WITH_` forms — `AVG` as [`hor_shaped`]'s, for
/// quarter-pel `(0, 1)` and `(0, 3)`.
#[inline(always)]
pub(crate) fn ver_shaped<L: McLeaves, S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    match (width, height) {
        (16, 16) => L::ver::<S, 16, 16, 21, AVG>(src, dst),
        (16, 8) => L::ver::<S, 16, 8, 13, AVG>(src, dst),
        (8, 16) => L::ver::<S, 8, 16, 21, AVG>(src, dst),
        (8, 8) => L::ver::<S, 8, 8, 13, AVG>(src, dst),
        (8, 4) => L::ver::<S, 8, 4, 9, AVG>(src, dst),
        (4, 8) => L::ver::<S, 4, 8, 13, AVG>(src, dst),
        (4, 4) => L::ver::<S, 4, 4, 9, AVG>(src, dst),
        (16, 17) => L::ver::<S, 16, 17, 22, AVG>(src, dst),
        (16, 9) => L::ver::<S, 16, 9, 14, AVG>(src, dst),
        (8, 17) => L::ver::<S, 8, 17, 22, AVG>(src, dst),
        (8, 9) => L::ver::<S, 8, 9, 14, AVG>(src, dst),
        _ => {
            note_runtime_shape();
            L::ver_any::<S, AVG>(src, dst, width, height)
        }
    }
}

/// `McHorVer22`, dispatched onto a const shape.
#[inline(always)]
pub(crate) fn cen_shaped<L: McLeaves, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    match (width, height) {
        (16, 16) => L::cen::<S, 16, 21, 16, 21>(src, dst),
        (16, 8) => L::cen::<S, 16, 21, 8, 13>(src, dst),
        (8, 16) => L::cen::<S, 8, 13, 16, 21>(src, dst),
        (8, 8) => L::cen::<S, 8, 13, 8, 13>(src, dst),
        (8, 4) => L::cen::<S, 8, 13, 4, 9>(src, dst),
        (4, 8) => L::cen::<S, 4, 9, 8, 13>(src, dst),
        (4, 4) => L::cen::<S, 4, 9, 4, 9>(src, dst),
        (17, 17) => L::cen::<S, 17, 22, 17, 22>(src, dst),
        (17, 9) => L::cen::<S, 17, 22, 9, 14>(src, dst),
        (9, 17) => L::cen::<S, 9, 14, 17, 22>(src, dst),
        (9, 9) => L::cen::<S, 9, 14, 9, 14>(src, dst),
        _ => {
            note_runtime_shape();
            L::cen_any::<S>(src, dst, width, height)
        }
    }
}

/// `McSampleAvg`, dispatched onto a const shape. The refinement averages the block
/// sizes it searches; the composites average the luma partitions.
#[inline(always)]
pub(crate) fn avg_shaped<L: McLeaves, A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    match (width, height) {
        (16, 16) => L::avg::<A, B, 16, 16>(dst, a, b),
        (16, 8) => L::avg::<A, B, 16, 8>(dst, a, b),
        (8, 16) => L::avg::<A, B, 8, 16>(dst, a, b),
        (8, 8) => L::avg::<A, B, 8, 8>(dst, a, b),
        (8, 4) => L::avg::<A, B, 8, 4>(dst, a, b),
        (4, 8) => L::avg::<A, B, 4, 8>(dst, a, b),
        (4, 4) => L::avg::<A, B, 4, 4>(dst, a, b),
        _ => {
            note_runtime_shape();
            L::avg_any::<A, B>(dst, a, b, width, height)
        }
    }
}

/// `McChromaWithFragMv`, dispatched onto a const shape. The bilinear filter reads
/// one extra column and one extra row, so the spans are `W + 1` by `H + 1`.
#[inline(always)]
pub(crate) fn chroma_shaped<L: McLeaves, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
    width: usize,
    height: usize,
) {
    match (width, height) {
        (8, 8) => L::chroma::<S, 8, 9, 8, 9>(src, dst, w),
        (8, 4) => L::chroma::<S, 8, 9, 4, 5>(src, dst, w),
        (4, 8) => L::chroma::<S, 4, 5, 8, 9>(src, dst, w),
        (4, 4) => L::chroma::<S, 4, 5, 4, 5>(src, dst, w),
        (4, 2) => L::chroma::<S, 4, 5, 2, 3>(src, dst, w),
        (2, 4) => L::chroma::<S, 2, 3, 4, 5>(src, dst, w),
        (2, 2) => L::chroma::<S, 2, 3, 2, 3>(src, dst, w),
        _ => {
            note_runtime_shape();
            L::chroma_any::<S>(src, dst, w, width, height)
        }
    }
}

// ============================================================================
// Kernels
// ============================================================================

/// C++: `PixelAvg_c` — the rounded average of two surfaces, `SMcFunc::pfSampleAveraging`.
#[inline(always)]
pub fn pixel_avg_c<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    avg_shaped::<ScalarLeaves, A, B>(dst, a, b, width, height)
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
    kernels::mc::pixel_avg(dst, a, b, width, height)
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
    hor_shaped::<ScalarLeaves, S, 0>(src, dst, width, height)
}

/// Horizontal half-pel filter, dispatching to SSE2 if available.
#[inline(always)]
pub fn mc_hor_ver20<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    kernels::mc::mc_hor_ver20(src, dst, width, height)
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
    ver_shaped::<ScalarLeaves, S, 0>(src, dst, width, height)
}

/// Vertical half-pel filter, dispatching to SSE2 if available.
#[inline(always)]
pub fn mc_hor_ver02<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    kernels::mc::mc_hor_ver02(src, dst, width, height)
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
    cen_shaped::<ScalarLeaves, S>(src, dst, width, height)
}

/// Center half-pel filter, dispatching to SSE2 if available.
#[inline(always)]
pub fn mc_hor_ver22<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    kernels::mc::mc_hor_ver22(src, dst, width, height)
}

/// A `16`-stride scratch surface for the quarter-pel kernels — the C++
/// `uint8_t uiTmp[256]`, which is why luma MC blocks are at most 16 wide and tall.
#[inline(always)]
fn scratch() -> [u8; 256] {
    [0u8; 256]
}

// ============================================================================
// The quarter-pel composites, once
// ============================================================================

/// **The four half-pel leaves and the averaging step, as one substitutable set.**
///
/// Twelve of the fifteen `McHorVerXY` kernels are not filters at all: they are
/// compositions of `McHorVer20` (horizontal), `McHorVer02` (vertical),
/// `McHorVer22` (centre) and `McSampleAvg`, at fixed offsets the standard fixes.
/// Only those four do arithmetic, and only they have an SSE2 form worth writing;
/// `McChromaWithFragMv`'s bilinear filter is here too, because it wants the same
/// const-shape treatment and nothing else about it is shared.
///
/// The scalar and SSE2 chains share these bodies and differ only in `L`, so they cannot
/// drift apart. The trade is that a structural mistake in a composite can no longer be
/// caught by comparing two spellings, because there is one; the leaves stay
/// individually tested, and `mc_luma_parity` compares the two instantiations across all
/// sixteen quarter-pel positions.
///
/// # The const parameters
///
/// Every method takes the block's `W`x`H` **and the reach its reads need**, so the
/// kernel can cut one [`RefSamples::span`] per operand instead of a `row_view` per
/// row. `SW` is the row length the horizontal six-tap reads (`W + 5`) or the bilinear
/// chroma reads (`W + 1`); `SH` is the row count the vertical six-tap reads (`H + 5`)
/// or chroma's (`H + 1`). They are separate parameters because stable Rust has no
/// arithmetic in a const-argument position — see the shape tables above.
///
/// `AVG` on [`hor`](Self::hor) and [`ver`](Self::ver) is 0 for the plain half-pel
/// filter, or the tap the result is averaged with: 2 for the `_AVERAGE_WITH_0`
/// quarter-pel kernels and 3 for `_AVERAGE_WITH_1`. Only the sets that fuse those
/// forms instantiate it non-zero.
///
/// Each method has an `_any` twin taking the shape at run time. Those exist so that a
/// shape the tables above do not carry is **slow rather than a panic** — the decoder's
/// `SMcFunc` slots hold these kernels — and `mc_shapes_all_reach_a_const_arm` proves
/// nothing the codec calls reaches one.
pub trait McLeaves {
    /// **Whether this set fuses the `_AVERAGE_WITH_` quarter-pel kernels into the two
    /// direct filters**, as `McLuma_AArch64_neon` does — `McHorVer10/30/01/03` are
    /// the horizontal or vertical filter with a rounded average against one of its
    /// own taps, and the asm has them as single kernels rather than a filter into
    /// scratch and an averaging pass over it.
    ///
    /// The fused form is byte-identical to the composite it replaces — the same
    /// rounded average of the same two values — and `mc_luma_parity` says so for all
    /// sixteen positions in every set. Only the sets that have written the fused arms
    /// turn this on; the rest take the composites, and their `AVG != 0` instantiations
    /// are then never generated.
    const FUSED_QPEL: bool = false;

    /// `McHorVer20` — the horizontal half-pel filter. Reads `SW = W + 5` per row.
    fn hor<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    );
    fn hor_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    );
    /// `McHorVer02` — the vertical half-pel filter. Reads `SH = H + 5` rows.
    fn ver<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    );
    fn ver_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    );
    /// `McHorVer22` — the centre filter, vertical into 16-bit then horizontal.
    fn cen<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    );
    fn cen_any<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize);
    /// `McSampleAvg` — the rounded two-source average.
    fn avg<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
    );
    fn avg_any<A: RefSamples, B: RefSamples>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
        width: usize,
        height: usize,
    );
    /// `McChromaWithFragMv` — the bilinear filter by the four weights `g_kuiABCD`
    /// selects. Reads `SW = W + 1` per row over `SH = H + 1` rows.
    fn chroma<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
    );
    fn chroma_any<S: RefSamples + Copy>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
        width: usize,
        height: usize,
    );
}

// ----------------------------------------------------------------------------
// The scalar leaf bodies
// ----------------------------------------------------------------------------

/// `McHorVer20_c` over one const-shape block: **one span for the source, one for the
/// destination**, and `W` six-tap windows per row.
#[inline(always)]
fn hor_block_c<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<SW, H>(0, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let row = s.row::<SW>(y, 0);
        let out = d.row_mut::<W>(y, 0);
        for (o, w) in out.iter_mut().zip(row.windows(6)) {
            let w: &[u8; 6] = w.try_into().expect("six taps");
            let mut v = WelsClip1((filter_input_8bit(w) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// The run-time-shape twin, sample by sample through [`RefSamples::at`] — cold, and
/// spelled for simplicity rather than speed. See [`McLeaves`].
fn hor_any_c<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let w: [u8; 6] = std::array::from_fn(|k| src.at(x as isize + k as isize - 2, dy));
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// `McHorVer02_c` over one const-shape block: one `SH`-row span, and a six-row
/// window sliding down it.
#[inline(always)]
fn ver_block_c<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, SH>(-2, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        s.row::<W>(0, 0),
        s.row::<W>(1, 0),
        s.row::<W>(2, 0),
        s.row::<W>(3, 0),
        s.row::<W>(4, 0),
    );
    for y in 0..H {
        let r5 = s.row::<W>(y + 5, 0);
        let out = d.row_mut::<W>(y, 0);
        for j in 0..W {
            let w = [r0[j], r1[j], r2[j], r3[j], r4[j], r5[j]];
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
            }
            out[j] = v;
        }
        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
    }
}

/// The run-time-shape twin; see [`hor_any_c`].
fn ver_any_c<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let w: [u8; 6] = std::array::from_fn(|k| src.at(x as isize, dy + k as isize - 2));
            let mut v = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + w[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// `McHorVer22_c` over one const-shape block. `iTmp` is the C++'s `int16_t[17 + 5]`,
/// which is what bounds `SW` at 22 and so the width at 17.
#[inline(always)]
fn cen_block_c<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<SW, SH>(-2, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let mut iTmp = [0i16; 17 + 5];
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        s.row::<SW>(0, 0),
        s.row::<SW>(1, 0),
        s.row::<SW>(2, 0),
        s.row::<SW>(3, 0),
        s.row::<SW>(4, 0),
    );
    for y in 0..H {
        let r5 = s.row::<SW>(y + 5, 0);
        for (j, t) in iTmp[..SW].iter_mut().enumerate() {
            *t = filter_input_8bit(&[r0[j], r1[j], r2[j], r3[j], r4[j], r5[j]]) as i16;
        }
        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
        let out = d.row_mut::<W>(y, 0);
        for (o, w) in out.iter_mut().zip(iTmp[..SW].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(w.try_into().expect("six taps")) + 512) >> 10);
        }
    }
}

/// The run-time-shape twin; see [`hor_any_c`].
fn cen_any_c<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    let n = width + 5;
    let mut iTmp = [0i16; 17 + 5];
    for dy in 0..height as isize {
        for (j, t) in iTmp[..n].iter_mut().enumerate() {
            let x = j as isize - 2;
            let w: [u8; 6] = std::array::from_fn(|k| src.at(x, dy + k as isize - 2));
            *t = filter_input_8bit(&w) as i16;
        }
        let out = dst.row_mut(dy, 0, width);
        for (o, w) in out.iter_mut().zip(iTmp[..n].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(w.try_into().expect("six taps")) + 512) >> 10);
        }
    }
}

/// `PixelAvg_c` over one const-shape block: one span per operand and one per row.
#[inline(always)]
fn avg_block_c<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
) {
    let sa = a.span::<W, H>(0, 0);
    let sb = b.span::<W, H>(0, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let (ra, rb) = (sa.row::<W>(y, 0), sb.row::<W>(y, 0));
        let out = d.row_mut::<W>(y, 0);
        for j in 0..W {
            out[j] = (((ra[j] as u32) + (rb[j] as u32) + 1) >> 1) as u8;
        }
    }
}

/// The run-time-shape twin; see [`hor_any_c`].
fn avg_any_c<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (j, o) in out.iter_mut().enumerate() {
            *o = (((a.at(j as isize, dy) as u32) + (b.at(j as isize, dy) as u32) + 1) >> 1) as u8;
        }
    }
}

/// `McChromaWithFragMv_c` over one const-shape block: the `W + 1` by `H + 1` window
/// the bilinear filter reads, cut once.
#[inline(always)]
fn chroma_block_c<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    let (iA, iB, iC, iD) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    let s = src.span::<SW, SH>(0, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let (r0, r1) = (s.row::<SW>(y, 0), s.row::<SW>(y + 1, 0));
        let out = d.row_mut::<W>(y, 0);
        for j in 0..W {
            out[j] = ((iA * (r0[j] as i32)
                + iB * (r0[j + 1] as i32)
                + iC * (r1[j] as i32)
                + iD * (r1[j + 1] as i32)
                + 32)
                >> 6) as u8;
        }
    }
}

/// The run-time-shape twin; see [`hor_any_c`].
fn chroma_any_c<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
    width: usize,
    height: usize,
) {
    let (iA, iB, iC, iD) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (j, o) in out.iter_mut().enumerate() {
            let x = j as isize;
            *o = ((iA * (src.at(x, dy) as i32)
                + iB * (src.at(x + 1, dy) as i32)
                + iC * (src.at(x, dy + 1) as i32)
                + iD * (src.at(x + 1, dy + 1) as i32)
                + 32)
                >> 6) as u8;
        }
    }
}

/// The scalar leaf set. Every method is a `_c` kernel, so a composite instantiated
/// here cannot route into SIMD — which is what makes it usable as a parity reference.
pub struct ScalarLeaves;

impl McLeaves for ScalarLeaves {
    #[inline(always)]
    fn hor<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        hor_block_c::<S, W, SW, H, AVG>(src, dst)
    }
    #[inline(always)]
    fn hor_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    ) {
        hor_any_c::<S, AVG>(src, dst, width, height)
    }
    #[inline(always)]
    fn ver<S: RefSamples + Copy, const W: usize, const H: usize, const SH: usize, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        ver_block_c::<S, W, H, SH, AVG>(src, dst)
    }
    #[inline(always)]
    fn ver_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    ) {
        ver_any_c::<S, AVG>(src, dst, width, height)
    }
    #[inline(always)]
    fn cen<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        cen_block_c::<S, W, SW, H, SH>(src, dst)
    }
    #[inline(always)]
    fn cen_any<S: RefSamples + Copy>(src: &S, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
        cen_any_c::<S>(src, dst, width, height)
    }
    #[inline(always)]
    fn avg<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
    ) {
        avg_block_c::<A, B, W, H>(dst, a, b)
    }
    #[inline(always)]
    fn avg_any<A: RefSamples, B: RefSamples>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
        width: usize,
        height: usize,
    ) {
        avg_any_c::<A, B>(dst, a, b, width, height)
    }
    #[inline(always)]
    fn chroma<S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
    ) {
        chroma_block_c::<S, W, SW, H, SH>(src, dst, w)
    }
    #[inline(always)]
    fn chroma_any<S: RefSamples + Copy>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
        width: usize,
        height: usize,
    ) {
        chroma_any_c::<S>(src, dst, w, width, height)
    }
}

// ----------------------------------------------------------------------------
// The twelve composites, at a const shape
// ----------------------------------------------------------------------------
//
// Each is `McHorVerXY_c`'s body with the leaf shapes fixed by the caller, so the
// leaf's own `(width, height)` match is gone: `mc_luma_with` dispatches the shape
// once and everything below it is const. The intermediates stay the C++'s
// `uint8_t uiTmp[256]` at stride 16, which is what bounds a luma MC block at 16x16
// and why `mc_luma_with`'s const table is the luma partitions and nothing wider.

/// C++: `McHorVer01_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver01_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut tmp = scratch();
    L::ver::<_, W, H, SH, 0>(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16));
    L::avg::<_, _, W, H>(dst, src, &PlaneCursor::new(&tmp, 0, 16));
}

/// C++: `McHorVer03_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver03_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut tmp = scratch();
    L::ver::<_, W, H, SH, 0>(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16));
    L::avg::<_, _, W, H>(dst, &src.advance(0, 1), &PlaneCursor::new(&tmp, 0, 16));
}

/// C++: `McHorVer10_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver10_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut tmp = scratch();
    L::hor::<_, W, SW, H, 0>(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16));
    L::avg::<_, _, W, H>(dst, src, &PlaneCursor::new(&tmp, 0, 16));
}

/// C++: `McHorVer11_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver11_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    L::hor::<_, W, SW, H, 0>(src, &mut PlaneCursorMut::new(&mut hor, 0, 16));
    L::ver::<_, W, H, SH, 0>(src, &mut PlaneCursorMut::new(&mut ver, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&hor, 0, 16), &PlaneCursor::new(&ver, 0, 16));
}

/// C++: `McHorVer12_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver12_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    L::ver::<_, W, H, SH, 0>(src, &mut PlaneCursorMut::new(&mut ver, 0, 16));
    L::cen::<_, W, SW, H, SH>(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&ver, 0, 16), &PlaneCursor::new(&ctr, 0, 16));
}

/// C++: `McHorVer13_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver13_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    L::hor::<_, W, SW, H, 0>(&src.advance(0, 1), &mut PlaneCursorMut::new(&mut hor, 0, 16));
    L::ver::<_, W, H, SH, 0>(src, &mut PlaneCursorMut::new(&mut ver, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&hor, 0, 16), &PlaneCursor::new(&ver, 0, 16));
}

/// C++: `McHorVer21_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver21_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    L::hor::<_, W, SW, H, 0>(src, &mut PlaneCursorMut::new(&mut hor, 0, 16));
    L::cen::<_, W, SW, H, SH>(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&hor, 0, 16), &PlaneCursor::new(&ctr, 0, 16));
}

/// C++: `McHorVer23_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver23_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    L::hor::<_, W, SW, H, 0>(&src.advance(0, 1), &mut PlaneCursorMut::new(&mut hor, 0, 16));
    L::cen::<_, W, SW, H, SH>(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&hor, 0, 16), &PlaneCursor::new(&ctr, 0, 16));
}

/// C++: `McHorVer30_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver30_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut hor = scratch();
    L::hor::<_, W, SW, H, 0>(src, &mut PlaneCursorMut::new(&mut hor, 0, 16));
    L::avg::<_, _, W, H>(dst, &src.advance(1, 0), &PlaneCursor::new(&hor, 0, 16));
}

/// C++: `McHorVer31_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver31_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    L::hor::<_, W, SW, H, 0>(src, &mut PlaneCursorMut::new(&mut hor, 0, 16));
    L::ver::<_, W, H, SH, 0>(&src.advance(1, 0), &mut PlaneCursorMut::new(&mut ver, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&hor, 0, 16), &PlaneCursor::new(&ver, 0, 16));
}

/// C++: `McHorVer32_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver32_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    L::ver::<_, W, H, SH, 0>(&src.advance(1, 0), &mut PlaneCursorMut::new(&mut ver, 0, 16));
    L::cen::<_, W, SW, H, SH>(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&ver, 0, 16), &PlaneCursor::new(&ctr, 0, 16));
}

/// C++: `McHorVer33_c` — the composite, over `L`'s leaves.
#[inline(never)]
fn mc_hor_ver33_with<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    L::hor::<_, W, SW, H, 0>(&src.advance(0, 1), &mut PlaneCursorMut::new(&mut hor, 0, 16));
    L::ver::<_, W, H, SH, 0>(&src.advance(1, 0), &mut PlaneCursorMut::new(&mut ver, 0, 16));
    L::avg::<_, _, W, H>(dst, &PlaneCursor::new(&hor, 0, 16), &PlaneCursor::new(&ver, 0, 16));
}

/// `McLuma_c`'s `switch` on `(mv_x & 3, mv_y & 3)` at a shape fixed by the caller.
#[inline(always)]
fn luma_shaped<L: McLeaves, S: RefSamples + Copy, const W: usize, const SW: usize, const H: usize, const SH: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
) {
    match ((mv_x & 0x03) as u8, (mv_y & 0x03) as u8) {
        (0, 0) => copy_block::<W, H, S>(src, dst),
        // The four `_AVERAGE_WITH_` positions, fused where the set has them; see
        // [`McLeaves::FUSED_QPEL`].
        (0, 1) if L::FUSED_QPEL => L::ver::<_, W, H, SH, 2>(src, dst),
        (0, 3) if L::FUSED_QPEL => L::ver::<_, W, H, SH, 3>(src, dst),
        (1, 0) if L::FUSED_QPEL => L::hor::<_, W, SW, H, 2>(src, dst),
        (3, 0) if L::FUSED_QPEL => L::hor::<_, W, SW, H, 3>(src, dst),
        (0, 1) => mc_hor_ver01_with::<L, S, W, SW, H, SH>(src, dst),
        (0, 2) => L::ver::<_, W, H, SH, 0>(src, dst),
        (0, 3) => mc_hor_ver03_with::<L, S, W, SW, H, SH>(src, dst),
        (1, 0) => mc_hor_ver10_with::<L, S, W, SW, H, SH>(src, dst),
        (1, 1) => mc_hor_ver11_with::<L, S, W, SW, H, SH>(src, dst),
        (1, 2) => mc_hor_ver12_with::<L, S, W, SW, H, SH>(src, dst),
        (1, 3) => mc_hor_ver13_with::<L, S, W, SW, H, SH>(src, dst),
        (2, 0) => L::hor::<_, W, SW, H, 0>(src, dst),
        (2, 1) => mc_hor_ver21_with::<L, S, W, SW, H, SH>(src, dst),
        (2, 2) => L::cen::<_, W, SW, H, SH>(src, dst),
        (2, 3) => mc_hor_ver23_with::<L, S, W, SW, H, SH>(src, dst),
        (3, 0) => mc_hor_ver30_with::<L, S, W, SW, H, SH>(src, dst),
        (3, 1) => mc_hor_ver31_with::<L, S, W, SW, H, SH>(src, dst),
        (3, 2) => mc_hor_ver32_with::<L, S, W, SW, H, SH>(src, dst),
        _ => mc_hor_ver33_with::<L, S, W, SW, H, SH>(src, dst),
    }
}

/// The run-time-shape twin of [`luma_shaped`] — cold, for a shape
/// `SHAPES_LUMA` does not carry; see [`McLeaves`]. The same sixteen arms over the
/// `_any` leaves and the same 16-stride scratch, so it is correct wherever the const
/// path is and slow everywhere.
fn luma_any<L: McLeaves, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    w: usize,
    h: usize,
) {
    let (mut p, mut q) = (scratch(), scratch());
    let (qx, qy) = ((mv_x & 0x03) as u8, (mv_y & 0x03) as u8);
    // The leaf that fills a scratch plane, and the cursor to read it back through.
    macro_rules! hor_to {
        ($t:expr, $s:expr) => {
            L::hor_any::<_, 0>(&$s, &mut PlaneCursorMut::new(&mut $t, 0, 16), w, h)
        };
    }
    macro_rules! ver_to {
        ($t:expr, $s:expr) => {
            L::ver_any::<_, 0>(&$s, &mut PlaneCursorMut::new(&mut $t, 0, 16), w, h)
        };
    }
    macro_rules! cen_to {
        ($t:expr, $s:expr) => {
            L::cen_any(&$s, &mut PlaneCursorMut::new(&mut $t, 0, 16), w, h)
        };
    }
    macro_rules! plane {
        ($t:expr) => {
            PlaneCursor::new(&$t, 0, 16)
        };
    }
    match (qx, qy) {
        (0, 0) => mc_copy(src, dst, w, h),
        (0, 1) if L::FUSED_QPEL => L::ver_any::<_, 2>(src, dst, w, h),
        (0, 3) if L::FUSED_QPEL => L::ver_any::<_, 3>(src, dst, w, h),
        (1, 0) if L::FUSED_QPEL => L::hor_any::<_, 2>(src, dst, w, h),
        (3, 0) if L::FUSED_QPEL => L::hor_any::<_, 3>(src, dst, w, h),
        (0, 1) => {
            ver_to!(p, *src);
            L::avg_any(dst, src, &plane!(p), w, h)
        }
        (0, 2) => L::ver_any::<_, 0>(src, dst, w, h),
        (0, 3) => {
            ver_to!(p, *src);
            L::avg_any(dst, &src.advance(0, 1), &plane!(p), w, h)
        }
        (1, 0) => {
            hor_to!(p, *src);
            L::avg_any(dst, src, &plane!(p), w, h)
        }
        (1, 1) => {
            hor_to!(p, *src);
            ver_to!(q, *src);
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
        (1, 2) => {
            ver_to!(p, *src);
            cen_to!(q, *src);
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
        (1, 3) => {
            hor_to!(p, src.advance(0, 1));
            ver_to!(q, *src);
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
        (2, 0) => L::hor_any::<_, 0>(src, dst, w, h),
        (2, 1) => {
            hor_to!(p, *src);
            cen_to!(q, *src);
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
        (2, 2) => L::cen_any(src, dst, w, h),
        (2, 3) => {
            hor_to!(p, src.advance(0, 1));
            cen_to!(q, *src);
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
        (3, 0) => {
            hor_to!(p, *src);
            L::avg_any(dst, &src.advance(1, 0), &plane!(p), w, h)
        }
        (3, 1) => {
            hor_to!(p, *src);
            ver_to!(q, src.advance(1, 0));
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
        (3, 2) => {
            ver_to!(p, src.advance(1, 0));
            cen_to!(q, *src);
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
        _ => {
            hor_to!(p, src.advance(0, 1));
            ver_to!(q, src.advance(1, 0));
            L::avg_any(dst, &plane!(p), &plane!(q), w, h)
        }
    }
}

/// `mc_luma`'s fan-out, over whichever leaf set `L` names.
///
/// C++: `McLuma_c`'s `switch` on `(mv_x & 3, mv_y & 3)`, with the block shape
/// resolved to consts **once** — see the shape tables above. Every arm below is a
/// luma partition, which is also what bounds the composites' 16-stride scratch.
pub fn mc_luma_with<L: McLeaves, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    match (width, height) {
        (16, 16) => luma_shaped::<L, S, 16, 21, 16, 21>(src, dst, mv_x, mv_y),
        (16, 8) => luma_shaped::<L, S, 16, 21, 8, 13>(src, dst, mv_x, mv_y),
        (8, 16) => luma_shaped::<L, S, 8, 13, 16, 21>(src, dst, mv_x, mv_y),
        (8, 8) => luma_shaped::<L, S, 8, 13, 8, 13>(src, dst, mv_x, mv_y),
        (8, 4) => luma_shaped::<L, S, 8, 13, 4, 9>(src, dst, mv_x, mv_y),
        (4, 8) => luma_shaped::<L, S, 4, 9, 8, 13>(src, dst, mv_x, mv_y),
        (4, 4) => luma_shaped::<L, S, 4, 9, 4, 9>(src, dst, mv_x, mv_y),
        _ => {
            note_runtime_shape();
            luma_any::<L, S>(src, dst, mv_x, mv_y, width, height)
        }
    }
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
    mc_luma_with::<ScalarLeaves, S>(src, dst, mv_x, mv_y, width, height)
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
    kernels::mc::mc_luma(src, dst, mv_x, mv_y, width, height)
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
    chroma_shaped::<ScalarLeaves, S>(src, dst, pABCD, width, height)
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
    kernels::mc::mc_chroma(src, dst, mv_x, mv_y, width, height)
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
// Read reaches
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

/// C++: `InitMcFunc`, `codec/common/src/mc.cpp` — both codecs call it at open time.
///
/// # The table it fills is not this port's MC dispatch path
///
/// Upstream calls MC *through* `sMcFunc`, so its `uiCpuFlag` gate here is what selects
/// scalar or SSE2 for every motion-compensated block. This port does not: `md.rs:582`
/// states it outright — "MC and the half-pel filters are called directly, not via
/// `sMcFuncs`" — and `grep` for the six field names below finds no read anywhere
/// outside this file. `mc_luma`, `mc_chroma`, `pixel_avg` and the half-pel filters call
/// `simd::kernels` directly, and which kernel set that names was decided at compile
/// time.
///
/// The consequence, unchanged by editing this function: **a caller that restricts
/// `uiCpuFlag` to force scalar MC does not get it.** The slots go scalar; the code that
/// runs is whichever set the build selected. `--features scalar` is what forces scalar
/// MC, and it forces it everywhere at once. So `init_mc_func_cpu_flags` below is a
/// faithful test of *this function* and of nothing downstream of it — not evidence that
/// MC dispatch is gated by the argument.
///
/// The table is still filled, and the fields still exist (`decoder_context.rs:1281`,
/// `encoder/wels_func_ptr_def.rs:327`), because the struct is part of the ported
/// context layout. Closing the gap means routing MC back through the table.
pub fn InitMcFunc(pMcFuncs: &mut SMcFunc, uiCpuFlag: u32) {
    *pMcFuncs = SMcFunc::default();
    if (uiCpuFlag & WELS_CPU_SSE2) != 0 {
        pMcFuncs.pfLumaHalfpelHor = |s, d, w, h| kernels::mc::mc_hor_ver20(s, d, w, h);
        pMcFuncs.pfLumaHalfpelVer = |s, d, w, h| kernels::mc::mc_hor_ver02(s, d, w, h);
        pMcFuncs.pfLumaHalfpelCen = |s, d, w, h| kernels::mc::mc_hor_ver22(s, d, w, h);
        pMcFuncs.pfSampleAveraging = |dst, a, b, w, h| kernels::mc::pixel_avg(dst, a, b, w, h);
        pMcFuncs.pMcChromaFunc = |s, d, mx, my, w, h| kernels::mc::mc_chroma(s, d, mx, my, w, h);
        pMcFuncs.pMcLumaFunc = |s, d, mx, my, w, h| kernels::mc::mc_luma(s, d, mx, my, w, h);
    }
}


// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`.
pub use crate::common::cpu_core::{WELS_CPU_3DNOW, WELS_CPU_3DNOWEXT, WELS_CPU_ALTIVEC, WELS_CPU_ARMv7, WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MMX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE, WELS_CPU_SSE2, WELS_CPU_SSE3, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3, WELS_CPU_VFPv3};

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
    /// **Every shape the codec calls motion compensation with reaches a const
    /// instantiation**, and none of them falls through to a run-time-shape kernel.
    ///
    /// The tables at the head of this module are transcribed from the call sites —
    /// `decoder/decode_slice.rs`'s `BaseMC` for the luma partitions and their chroma
    /// halves, `encoder/svc_base_layer_md.rs` and `encoder/svc_mode_decision.rs` for
    /// the skip and partition candidates, and `encoder/md.rs`'s `MeRefineFracPixel`
    /// for the `kiW + 1` / `kiH + 1` half-pel buffers — and this drives every entry of
    /// them through the kernel set this build compiled, over **both** operand
    /// storages: the plain plane cursor the decoder hands them and the shared cell
    /// view the encoder does.
    ///
    /// The fallbacks it proves unreachable still have to exist and still have to be
    /// correct: the decoder's `SMcFunc` slots hold these kernels, and a `match` that
    /// panicked on a shape they were handed would be a crash in production. The
    /// parity tests in each kernel set drive shapes outside the tables for exactly
    /// that reason.
    ///
    /// Runs under Miri, which sees the scalar set — the const-shape kernels here are
    /// where the new safe accessor use lives, and this is what drives all of them.
    #[test]
    fn mc_shapes_all_reach_a_const_arm() {
        let base = filled_plane();
        let mut cells = filled_plane();
        let mut dst = vec![0u8; STRIDE * ROWS];
        let (src_c, dst_c) = (10 * STRIDE + 10, 20 * STRIDE + 10);

        let fallbacks = runtime_shapes_during(|| {
            let src = PlaneCursor::new(&base, src_c, STRIDE);
            let rec = crate::encoder::rec_view::RecCursor::over_owned(&mut cells, src_c, STRIDE);
            // Both operand storages, at each shape; a closure cannot stand in for
            // the source because the kernels are generic over it.
            macro_rules! both {
                ($call:ident $(, $arg:expr)*) => {{
                    kernels::mc::$call(&src, &mut PlaneCursorMut::new(&mut dst, dst_c, STRIDE) $(, $arg)*);
                    kernels::mc::$call(&rec, &mut PlaneCursorMut::new(&mut dst, dst_c, STRIDE) $(, $arg)*);
                }};
            }
            for &(w, h) in SHAPES_LUMA.iter().chain(SHAPES_REFINE_HOR.iter()) {
                both!(mc_hor_ver20, w, h);
            }
            for &(w, h) in SHAPES_LUMA.iter().chain(SHAPES_REFINE_VER.iter()) {
                both!(mc_hor_ver02, w, h);
            }
            for &(w, h) in SHAPES_LUMA.iter().chain(SHAPES_REFINE_CEN.iter()) {
                both!(mc_hor_ver22, w, h);
            }
            for &(w, h) in SHAPES_LUMA.iter() {
                // `pixel_avg`'s two operands are a scratch plane and either another
                // scratch plane or the reference, so both storages appear as `b`.
                let a = PlaneCursor::new(&base, src_c, STRIDE);
                kernels::mc::pixel_avg(&mut PlaneCursorMut::new(&mut dst, dst_c, STRIDE), &a, &src, w, h);
                kernels::mc::pixel_avg(&mut PlaneCursorMut::new(&mut dst, dst_c, STRIDE), &a, &rec, w, h);
                for qy in 0..4i16 {
                    for qx in 0..4i16 {
                        both!(mc_luma, qx, qy, w, h);
                    }
                }
            }
            for &(w, h) in SHAPES_CHROMA.iter() {
                for (mx, my) in [(0i16, 0i16), (3, 5)] {
                    both!(mc_chroma, mx, my, w, h);
                }
            }
        });
        assert_eq!(
            fallbacks, 0,
            "{fallbacks} motion-compensation calls at shapes the codec uses fell through to a \
             run-time-shape kernel; every one of them should have reached a const arm"
        );

        // And the counter is what makes that assertion mean anything, so show it
        // moves: 5x4 is a shape no table carries, and it has to reach the fallback.
        let odd = runtime_shapes_during(|| {
            let src = PlaneCursor::new(&base, src_c, STRIDE);
            kernels::mc::mc_hor_ver20(&src, &mut PlaneCursorMut::new(&mut dst, dst_c, STRIDE), 5, 4);
        });
        assert_eq!(odd, 1, "a shape outside the tables should reach the run-time fallback");
    }

    /// **The `_AVERAGE_WITH_` forms of the scalar filters agree with the composites
    /// they would replace.**
    ///
    /// [`McLeaves::FUSED_QPEL`] is off for [`ScalarLeaves`], so `mc_luma_c` takes the
    /// composite at quarter-pel `(1, 0)`, `(3, 0)`, `(0, 1)` and `(0, 3)` and the
    /// `AVG` arms of [`McLeaves::hor`] and [`McLeaves::ver`] are never instantiated by
    /// the codec. They are still part of the trait and still have to be right — the
    /// NEON set's fused kernels are the same arithmetic — so drive them here.
    #[test]
    fn the_fused_quarter_pel_arms_agree_with_the_composites() {
        let base = filled_plane();
        let src = PlaneCursor::new(&base, 10 * STRIDE + 10, STRIDE);
        let dst_c = 20 * STRIDE + 10;
        for (qx, qy) in [(1i16, 0i16), (3, 0), (0, 1), (0, 3)] {
            let mut want = vec![0u8; STRIDE * ROWS];
            let mut got = vec![0u8; STRIDE * ROWS];
            mc_luma_c(&src, &mut PlaneCursorMut::new(&mut want, dst_c, STRIDE), qx, qy, 16, 16);
            {
                let mut d = PlaneCursorMut::new(&mut got, dst_c, STRIDE);
                match (qx, qy) {
                    (1, 0) => ScalarLeaves::hor::<_, 16, 21, 16, 2>(&src, &mut d),
                    (3, 0) => ScalarLeaves::hor::<_, 16, 21, 16, 3>(&src, &mut d),
                    (_, 1) => ScalarLeaves::ver::<_, 16, 16, 21, 2>(&src, &mut d),
                    _ => ScalarLeaves::ver::<_, 16, 16, 21, 3>(&src, &mut d),
                }
            }
            assert_eq!(want, got, "fused quarter-pel ({qx}, {qy}) differs from the composite");
        }
    }

    /// **Scope: `InitMcFunc` only.** The slots this checks are never read — see the
    /// note on `InitMcFunc`. Passing does not mean a `uiCpuFlag` without
    /// `WELS_CPU_SSE2` produces scalar motion compensation; it does not.
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

        {
            let mut sse2_base = SMcFunc::default();
            InitMcFunc(&mut sse2_base, WELS_CPU_SSE2);
            let sse2_want = addrs(&sse2_base);
            // The `assert_ne!` that stood here — "the SSE2 flag installs something other
            // than the scalar" — is gone with the other two wiring assertions: under the
            // scalar `simd::kernels` alias it would compare two distinct forwards to the
            // same body and pass while nothing was accelerated. That property is now
            // `simd::tests::every_kernel_is_named_by_a_dispatch_site`, which asks it of
            // the kernels rather than of two function-pointer addresses.

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
