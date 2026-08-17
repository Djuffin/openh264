#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

// CPU feature flags from cpu_core.h

use crate::safe::plane::{PlaneCursor, PlaneCursorMut};

// Function pointer signatures matching mc.h
pub type PWelsMcFunc = unsafe extern "C" fn(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iMvX: i16,
    iMvY: i16,
    iWidth: i32,
    iHeight: i32,
);

pub type PWelsLumaHalfpelMcFunc = unsafe extern "C" fn(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
);

pub type PWelsSampleAveragingFunc = unsafe extern "C" fn(
    pDst: *mut u8,
    iDstStride: i32,
    pSrcA: *const u8,
    iSrcAStride: i32,
    pSrcB: *const u8,
    iSrcBStride: i32,
    iWidth: i32,
    iHeight: i32,
);

pub type PMcChromaWidthExtFunc = unsafe extern "C" fn(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    kpABCD: *const u8,
    iHeight: i32,
);

pub type PWelsSampleWidthAveragingFunc = unsafe extern "C" fn(
    pDst: *mut u8,
    iDstStride: i32,
    pSrcA: *const u8,
    iSrcAStride: i32,
    pSrcB: *const u8,
    iSrcBStride: i32,
    iHeight: i32,
);

pub type PWelsMcWidthHeightFunc = unsafe extern "C" fn(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
);

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TagMcFunc {
    pub pfLumaHalfpelHor: Option<PWelsLumaHalfpelMcFunc>,
    pub pfLumaHalfpelVer: Option<PWelsLumaHalfpelMcFunc>,
    pub pfLumaHalfpelCen: Option<PWelsLumaHalfpelMcFunc>,
    pub pMcChromaFunc: Option<PWelsMcFunc>,
    pub pMcLumaFunc: Option<PWelsMcFunc>,
    pub pfSampleAveraging: Option<PWelsSampleAveragingFunc>,
}

pub type SMcFunc = TagMcFunc;

impl Default for TagMcFunc {
    fn default() -> Self {
        Self {
            pfLumaHalfpelHor: None,
            pfLumaHalfpelVer: None,
            pfLumaHalfpelCen: None,
            pMcChromaFunc: None,
            pMcLumaFunc: None,
            pfSampleAveraging: None,
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
// Safe kernels (plan §Phase 2, recipe R2)
// ============================================================================
//
// These are the implementations; the `Mc*_c` / `PixelAvg_c` functions below are
// strangler shims (R7) that build cursors from the raw pointers and call in here,
// so no call site and no dispatch-table installer changes in this phase.
//
// **This is the first family whose kernels read one plane and write another**, so
// unlike the intra-prediction families they take a `PlaneCursor` (the reference
// picture, or an encoder search buffer) *and* a `PlaneCursorMut` (the destination)
// rather than one cursor over a single surface. The two are different allocations
// at every real call site.
//
// The reads reach outside the block by design — the 6-tap Wiener filter of H.264
// half-pel interpolation needs two samples before and three after each output
// sample, in whichever direction it runs. Where intra prediction's legality came
// from `PADDING_LENGTH` alone, **an MC read is legal because the caller clamped the
// motion vector first**; each shim's `# Safety` block states that clamp, and
// `luma_reach`/`src_span` below are the only places the resulting span is computed.
//
// Arithmetic parity (plan §Phase 2, R-e): every intermediate below keeps the width
// the old port used. The 6-tap sums were checked against both the old Rust and
// `codec/common/src/mc.cpp` — with byte inputs they are bounded by `510 * 20 =
// 10200` in `filter_input_8bit` and by `21420 + 25500 + 428400 = 475320` in
// `hor_filter_input_16bit`, so nothing here can overflow its `i32`, and the `as
// i16` narrowing in `mc_hor_ver22` is likewise inside range. No F-finding.

/// The 6-tap Wiener filter over six samples — the C++ `FilterInput8bitWithStride_c`
/// with its `kiOffset` walk already done by the caller, so `p[i]` is that kernel's
/// `pSrc[(i - 2) * kiOffset]`.
///
/// C++: `FilterInput8bitWithStride_c`, `codec/common/src/mc.cpp`.
///
/// The expression is the old port's term for term, including the `u32` accumulation
/// and the shift-and-add spellings of `*5` and `*20`.
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
/// **The width is a const parameter and not an argument, and that is a measured
/// decision rather than a stylistic one.** With a runtime length, `copy_from_slice`
/// lowers to a `_platform_memmove` *call* per row; with the width const, the whole
/// row is one pair of wide loads and stores — which is what the C++ `LD64`/`ST64A8`
/// pairs were hand-written to get. This path carries the zero-MV block, the
/// commonest luma case there is, and the difference measured **10.8x** on
/// `McLuma_c(0, 0)` for a 16x16 block (`docs/perf_baseline.md` §Phase 2 T4).
///
/// The bounds check lands once per row either way.
#[inline(always)]
pub(crate) fn copy_rows<const WIDTH: usize>(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    height: usize,
) {
    for dy in 0..height as isize {
        let s: &[u8; WIDTH] = src.row(dy, 0, WIDTH).try_into().unwrap();
        let d: &mut [u8; WIDTH] = dst.row_mut(dy, 0, WIDTH).try_into().unwrap();
        *d = *s;
    }
}

/// C++: `McCopyWidthEq2_c` — chroma only, the one width the copy path narrows to.
#[inline(always)]
pub fn mc_copy_width_eq2(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<2>(src, dst, height);
}

/// C++: `McCopyWidthEq4_c`.
#[inline(always)]
pub fn mc_copy_width_eq4(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<4>(src, dst, height);
}

/// C++: `McCopyWidthEq8_c`.
#[inline(always)]
pub fn mc_copy_width_eq8(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<8>(src, dst, height);
}

/// C++: `McCopyWidthEq16_c`.
#[inline(always)]
pub fn mc_copy_width_eq16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows::<16>(src, dst, height);
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
pub fn mc_copy(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    // Dispatched exactly as the C++ dispatches, and for the same reason it does:
    // each arm is a constant-width copy. See [`copy_rows`].
    match width {
        16 => copy_rows::<16>(src, dst, height),
        8 => copy_rows::<8>(src, dst, height),
        4 => copy_rows::<4>(src, dst, height),
        _ => copy_rows::<2>(src, dst, height),
    }
}

/// C++: `PixelAvg_c` — the rounded average of two surfaces, `SMcFunc::pfSampleAveraging`.
///
/// The three surfaces carry their own strides because the encoder's quarter-pel
/// refinement averages a `ME_REFINE_BUF_STRIDE` scratch buffer against the reference
/// picture (`encoder/md.rs:1059`).
#[inline(always)]
pub fn pixel_avg(
    dst: &mut PlaneCursorMut<'_>,
    a: &PlaneCursor<'_>,
    b: &PlaneCursor<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let ra = a.row(dy, 0, width);
        let rb = b.row(dy, 0, width);
        let out = dst.row_mut(dy, 0, width);
        for j in 0..width {
            out[j] = (((ra[j] as u32) + (rb[j] as u32) + 1) >> 1) as u8;
        }
    }
}

/// C++: `McHorVer20_c` — the horizontal half-pel filter, `(2, 0)` in quarter-pel.
///
/// Reads `x` in `-2 .. width + 3`, `y` in `0 .. height`.
#[inline(always)]
pub fn mc_hor_ver20(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        // One row window per output row, six-sample sliding windows inside it: the
        // bounds check lands per row, the filter arithmetic per sample.
        let row = src.row(dy, -2, width + 5);
        let out = dst.row_mut(dy, 0, width);
        for (o, w) in out.iter_mut().zip(row.windows(6)) {
            *o = WelsClip1((filter_input_8bit(w.try_into().unwrap()) + 16) >> 5);
        }
    }
}

/// C++: `McHorVer02_c` — the vertical half-pel filter, `(0, 2)` in quarter-pel.
///
/// Reads `x` in `0 .. width`, `y` in `-2 .. height + 3`.
#[inline(always)]
pub fn mc_hor_ver02(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    // Two shapes carry this loop, and both were measured rather than assumed.
    //
    // *A zipped column walk, not indexing.* Indexing seven same-length slices by `j`
    // leaves LLVM to prove seven bounds facts per output *sample*, and it does not.
    // The zip moves the whole check to the `row` calls, one per output row.
    //
    // *A rolling window, not six fresh `row` calls per output row.* Six calls means
    // six `center + dy * stride` multiplies per row where the C++ advanced one
    // pointer by one add; rotating five slices down and fetching only the new bottom
    // row costs one.
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        src.row(-2, 0, width),
        src.row(-1, 0, width),
        src.row(0, 0, width),
        src.row(1, 0, width),
        src.row(2, 0, width),
    );
    for dy in 0..height as isize {
        let r5 = src.row(dy + 3, 0, width);
        let out = dst.row_mut(dy, 0, width);
        for ((((((o, &a), &b), &c), &d), &e), &f) in
            out.iter_mut().zip(r0).zip(r1).zip(r2).zip(r3).zip(r4).zip(r5)
        {
            *o = WelsClip1((filter_input_8bit(&[a, b, c, d, e, f]) + 16) >> 5);
        }
        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
    }
}

/// C++: `McHorVer22_c` — the centre half-pel filter, `(2, 2)` in quarter-pel:
/// vertical 6-tap into 16-bit intermediates, then horizontal 6-tap over those.
///
/// Reads `x` in `-2 .. width + 3`, `y` in `-2 .. height + 3`.
///
/// `iTmp` is `[i16; 17 + 5]` as in the C++, and `width` above 17 indexes past it —
/// a panic here, exactly as in the old port, which used a Rust array too. The
/// encoder's half-pel refinement is what needs the 17: it filters `iWidth + 1`
/// columns (`encoder/md.rs:1289`).
#[inline(always)]
pub fn mc_hor_ver22(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut iTmp = [0i16; 17 + 5];
    let n = width + 5;
    // Zipped and rolling, for the reasons given in `mc_hor_ver02`.
    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        src.row(-2, -2, n),
        src.row(-1, -2, n),
        src.row(0, -2, n),
        src.row(1, -2, n),
        src.row(2, -2, n),
    );
    for dy in 0..height as isize {
        let r5 = src.row(dy + 3, -2, n);
        for ((((((t, &a), &b), &c), &d), &e), &f) in
            iTmp[..n].iter_mut().zip(r0).zip(r1).zip(r2).zip(r3).zip(r4).zip(r5)
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

/// A `16`-stride scratch surface for the quarter-pel kernels — the C++
/// `uint8_t uiTmp[256]`, which is why luma MC blocks are at most 16 wide and tall.
#[inline(always)]
fn scratch() -> [u8; 256] {
    [0u8; 256]
}

/// C++: `McHorVer01_c`.
#[inline(never)]
pub fn mc_hor_ver01(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver02(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg(dst, src, &PlaneCursor::new(&tmp, 0, 16), width, height);
}

/// C++: `McHorVer03_c`.
#[inline(never)]
pub fn mc_hor_ver03(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver02(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg(
        dst,
        &src.advance(0, 1),
        &PlaneCursor::new(&tmp, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer10_c`.
#[inline(never)]
pub fn mc_hor_ver10(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver20(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg(dst, src, &PlaneCursor::new(&tmp, 0, 16), width, height);
}

/// C++: `McHorVer11_c`.
#[inline(never)]
pub fn mc_hor_ver11(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver02(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    pixel_avg(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer12_c`.
#[inline(never)]
pub fn mc_hor_ver12(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    mc_hor_ver02(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    mc_hor_ver22(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg(
        dst,
        &PlaneCursor::new(&ver, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer13_c`.
#[inline(never)]
pub fn mc_hor_ver13(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver02(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    pixel_avg(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer21_c`.
#[inline(never)]
pub fn mc_hor_ver21(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    mc_hor_ver20(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver22(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer23_c`.
#[inline(never)]
pub fn mc_hor_ver23(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    mc_hor_ver20(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver22(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer30_c`.
#[inline(never)]
pub fn mc_hor_ver30(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    mc_hor_ver20(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    pixel_avg(
        dst,
        &src.advance(1, 0),
        &PlaneCursor::new(&hor, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer31_c`.
#[inline(never)]
pub fn mc_hor_ver31(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver02(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    pixel_avg(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer32_c`.
#[inline(never)]
pub fn mc_hor_ver32(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    mc_hor_ver02(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    mc_hor_ver22(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg(
        dst,
        &PlaneCursor::new(&ver, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

/// C++: `McHorVer33_c`.
#[inline(never)]
pub fn mc_hor_ver33(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver02(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    pixel_avg(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// C++: `McLuma_c` — quarter-pel dispatch on the low two bits of each MV component.
///
/// This `match` replaces the module-internal `pWelsMcFunc_c: [[fn; 4]; 4]` table of
/// raw-pointer function pointers. It is the one dispatch table Phase 2 may touch
/// (plan §Phase 2, §3.2) — it never left this module, so folding it changes no
/// caller and no typedef, and the arms are in the table's `[iMvX & 3][iMvY & 3]`
/// order so the two can be read against each other.
#[inline(always)]
pub fn mc_luma(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    match ((mv_x & 0x03) as u8, (mv_y & 0x03) as u8) {
        (0, 0) => mc_copy(src, dst, width, height),
        (0, 1) => mc_hor_ver01(src, dst, width, height),
        (0, 2) => mc_hor_ver02(src, dst, width, height),
        (0, 3) => mc_hor_ver03(src, dst, width, height),
        (1, 0) => mc_hor_ver10(src, dst, width, height),
        (1, 1) => mc_hor_ver11(src, dst, width, height),
        (1, 2) => mc_hor_ver12(src, dst, width, height),
        (1, 3) => mc_hor_ver13(src, dst, width, height),
        (2, 0) => mc_hor_ver20(src, dst, width, height),
        (2, 1) => mc_hor_ver21(src, dst, width, height),
        (2, 2) => mc_hor_ver22(src, dst, width, height),
        (2, 3) => mc_hor_ver23(src, dst, width, height),
        (3, 0) => mc_hor_ver30(src, dst, width, height),
        (3, 1) => mc_hor_ver31(src, dst, width, height),
        (3, 2) => mc_hor_ver32(src, dst, width, height),
        _ => mc_hor_ver33(src, dst, width, height),
    }
}

/// C++: `McChromaWithFragMv_c` — bilinear chroma interpolation at eighth-pel.
///
/// Reads `x` in `0 .. width + 1`, `y` in `0 .. height + 1`.
#[inline(always)]
pub fn mc_chroma_with_frag_mv(
    src: &PlaneCursor<'_>,
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
        let r0 = src.row(dy, 0, width + 1);
        let r1 = src.row(dy + 1, 0, width + 1);
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
pub fn mc_chroma(
    src: &PlaneCursor<'_>,
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

// ============================================================================
// Strangler shims (plan §4 R7) — the raw-pointer entry points `SMcFunc` still
// holds. Each builds the cursor pair its kernel needs and calls the safe
// implementation above.
// ============================================================================
//
// # Why an MC read outside the block is legal
//
// Every shim below reaches past the block it is given: the 6-tap filter needs
// two samples before and three after each output sample. Intra prediction could
// justify its one-sample reach from `PADDING_LENGTH` alone. **MC cannot** — the
// source pointer has already been displaced by a motion vector, so the reach is
// legal only because the caller clamped that vector first. The decoder's clamp
// is `BaseMC` (`decoder/decode_slice.rs:1069-1091`), quoted here because it is
// the entire safety argument and it is *exactly* calibrated to this reach:
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
// The encoder's callers are a different family of buffers and their own
// argument: ME refinement filters out of the reference picture into
// `pBufferInterPredMe` scratch (`encoder/md.rs:1043-1046`), and the search
// window is bounded before the call rather than by a clamp inside it.
//
// **Phase 5 converts callers against these contracts**, so they say what the
// caller must guarantee rather than what this code happens to do.

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

/// Per-kernel read reach, in `[iMvX & 3][iMvY & 3]` order — the same indexing the
/// deleted `pWelsMcFunc_c` table used, so the two read against each other.
///
/// It is not one reach for all sixteen: `McHorVer10_c` reads no row outside its
/// block and `McHorVer13_c` reads five, and a shim that claimed the union would be
/// asserting validity for rows its caller never promised.
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
/// computed; every shim gets its numbers from here.
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

/// Runs a safe `(src, dst, width, height)` kernel behind a raw-pointer entry point:
/// **the one place in this module where a raw pointer becomes a slice.**
///
/// `span_width` is the width the *span* covers, which is the kernel's `width`
/// everywhere except the copy path, where `McCopy_c` narrows it (see
/// [`copy_width`]) and claiming the nominal width would over-assert validity.
///
/// A non-positive width or height returns without touching memory, matching the
/// old kernels, whose loops simply did not run.
///
/// # Safety
/// `pSrc` and `pDst` point at sample `(0, 0)` of the source and destination blocks;
/// the source's `reach` neighbourhood and the destination's block must be valid for
/// reads and writes respectively, at the given strides, and the two spans must not
/// overlap.
#[inline(always)]
unsafe fn shim_wh(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
    reach: Reach,
    span_width: usize,
    f: impl FnOnce(&PlaneCursor<'_>, &mut PlaneCursorMut<'_>, usize, usize),
) {
    if iWidth <= 0 || iHeight <= 0 {
        return;
    }
    let (w, h) = (iWidth as usize, iHeight as usize);
    let (ss, ds) = (iSrcStride as usize, iDstStride as usize);
    let (slen, scenter) = src_span(ss, span_width, h, reach);
    let src = unsafe { std::slice::from_raw_parts(pSrc.sub(scenter), slen) };
    let dst = unsafe { std::slice::from_raw_parts_mut(pDst, block_span(ds, span_width, h)) };
    f(
        &PlaneCursor::new(src, scenter, ss),
        &mut PlaneCursorMut::new(dst, 0, ds),
        w,
        h,
    );
}

/// C++: `McCopyWidthEq2_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` and `pDst` point at sample `(0, 0)` of a 2 x `iHeight` block in surfaces
///   whose rows are `iSrcStride` / `iDstStride` bytes apart, and reads span
///   `[0, (iHeight - 1) * iSrcStride + 2)` from `pSrc`, writes the same shape from
///   `pDst`. Nothing outside the block is touched.
/// * The two spans must not overlap, and both strides must be positive.
#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq2_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_copy_width_eq2
    unsafe {
        shim_wh(pSrc, iSrcStride, pDst, iDstStride, 2, iHeight, R_COPY, 2, |s, d, _, h| {
            mc_copy_width_eq2(s, d, h)
        })
    };
}

/// C++: `McCopyWidthEq4_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// As [`McCopyWidthEq2_c`], with a block 4 samples wide.
#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq4_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_copy_width_eq4
    unsafe {
        shim_wh(pSrc, iSrcStride, pDst, iDstStride, 4, iHeight, R_COPY, 4, |s, d, _, h| {
            mc_copy_width_eq4(s, d, h)
        })
    };
}

/// C++: `McCopyWidthEq8_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// As [`McCopyWidthEq2_c`], with a block 8 samples wide.
#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq8_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_copy_width_eq8
    unsafe {
        shim_wh(pSrc, iSrcStride, pDst, iDstStride, 8, iHeight, R_COPY, 8, |s, d, _, h| {
            mc_copy_width_eq8(s, d, h)
        })
    };
}

/// C++: `McCopyWidthEq16_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// As [`McCopyWidthEq2_c`], with a block 16 samples wide.
#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq16_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_copy_width_eq16
    unsafe {
        shim_wh(pSrc, iSrcStride, pDst, iDstStride, 16, iHeight, R_COPY, 16, |s, d, _, h| {
            mc_copy_width_eq16(s, d, h)
        })
    };
}

/// C++: `McCopy_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` and `pDst` point at sample `(0, 0)` of a `w` x `iHeight` block, where
///   **`w` is 16, 8 or 4 if `iWidth` is exactly that and 2 otherwise** — this kernel
///   dispatches on the exact value and copies two bytes for anything else. Reads span
///   `[0, (iHeight - 1) * iSrcStride + w)` from `pSrc` and writes the same shape from
///   `pDst`; nothing outside the block is touched.
/// * The two spans must not overlap, and both strides must be positive.
#[inline(always)]
pub unsafe extern "C" fn McCopy_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_copy
    let span_width = copy_width(iWidth.max(0) as usize);
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            R_COPY,
            span_width,
            mc_copy,
        )
    };
}

/// C++: `HorFilterInput16bit_c`, `codec/common/src/mc.cpp`.
///
/// Nothing in the tree calls this any more — `mc_hor_ver22` uses
/// [`hor_filter_input_16bit`] directly. The raw entry point is kept so this phase
/// changes no signature; Phase 5 deletes it.
///
/// # Safety
/// `pSrc` must be valid to read for six `i16`s, `pSrc[0..6]`.
#[inline(always)]
pub unsafe fn HorFilterInput16bit_c(pSrc: *const i16) -> i32 {
    // SHIM(phase2) -> hor_filter_input_16bit
    let p = unsafe { std::slice::from_raw_parts(pSrc, 6) };
    hor_filter_input_16bit(p.try_into().unwrap())
}

/// C++: `FilterInput8bitWithStride_c`, `codec/common/src/mc.cpp`.
///
/// Nothing in the tree calls this any more — the half-pel kernels use
/// [`filter_input_8bit`] over a row window (horizontal) or six row windows
/// (vertical), which is where the per-row rather than per-sample bounds check comes
/// from. The raw entry point is kept so this phase changes no signature; Phase 5
/// deletes it.
///
/// # Safety
/// `kiOffset` is positive — 1 for the horizontal filter, the source stride for the
/// vertical one — and `pSrc[-2*kiOffset ..= 3*kiOffset]` must be valid to read.
#[inline(always)]
pub unsafe fn FilterInput8bitWithStride_c(pSrc: *const u8, kiOffset: i32) -> i32 {
    // SHIM(phase2) -> filter_input_8bit
    let k = kiOffset as isize;
    let span = unsafe { std::slice::from_raw_parts(pSrc.offset(-2 * k), (5 * k + 1) as usize) };
    let k = k as usize;
    let p = [span[0], span[k], span[2 * k], span[3 * k], span[4 * k], span[5 * k]];
    filter_input_8bit(&p)
}

/// C++: `PixelAvg_c`, `codec/common/src/mc.cpp` — `SMcFunc::pfSampleAveraging`.
///
/// # Safety
/// * `pDst`, `pSrcA` and `pSrcB` each point at sample `(0, 0)` of an `iWidth` x
///   `iHeight` block in a surface of its own stride; each span is
///   `[0, (iHeight - 1) * stride + iWidth)`. Nothing outside the blocks is touched.
/// * The destination span must not overlap either source span. The encoder does hand
///   this kernel two regions of one allocation (`pBufferInterPredMe`, at the fixed
///   640-byte offsets of `encoder/md.rs:1043-1046`) — those regions are disjoint,
///   which is what makes it legal.
/// * All three strides must be positive.
#[inline(always)]
pub unsafe extern "C" fn PixelAvg_c(
    pDst: *mut u8,
    iDstStride: i32,
    pSrcA: *const u8,
    iSrcAStride: i32,
    pSrcB: *const u8,
    iSrcBStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> pixel_avg
    if iWidth <= 0 || iHeight <= 0 {
        return;
    }
    let (w, h) = (iWidth as usize, iHeight as usize);
    let (ds, sa, sb) = (iDstStride as usize, iSrcAStride as usize, iSrcBStride as usize);
    let dst = unsafe { std::slice::from_raw_parts_mut(pDst, block_span(ds, w, h)) };
    let a = unsafe { std::slice::from_raw_parts(pSrcA, block_span(sa, w, h)) };
    let b = unsafe { std::slice::from_raw_parts(pSrcB, block_span(sb, w, h)) };
    pixel_avg(
        &mut PlaneCursorMut::new(dst, 0, ds),
        &PlaneCursor::new(a, 0, sa),
        &PlaneCursor::new(b, 0, sb),
        w,
        h,
    );
}

// The fifteen quarter-pel entry points. Written out one by one rather than generated
// from a macro on purpose: a macro folds fifteen `unsafe extern "C" fn` definitions
// into one line of source, which makes the unsafe ratchet report a 13-definition drop
// that did not happen (plan §7.1) — and it would hide each kernel's own reach behind a
// table lookup in the one place a reader is looking for it.

/// C++: `McHorVer20_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-0 .. iHeight + 0` — `LUMA_REACH[2][0]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * No internal scratch, so no width or height ceiling beyond the spans above.
#[inline(always)]
pub unsafe extern "C" fn McHorVer20_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver20
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[2][0],
            iWidth.max(0) as usize,
            mc_hor_ver20,
        )
    };
}

/// C++: `McHorVer02_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-0 .. iWidth + 0` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[0][2]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * No internal scratch, so no width or height ceiling beyond the spans above.
#[inline(always)]
pub unsafe extern "C" fn McHorVer02_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver02
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[0][2],
            iWidth.max(0) as usize,
            mc_hor_ver02,
        )
    };
}

/// C++: `McHorVer22_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[2][2]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` is at most 17: this kernel's 16-bit intermediates live in a
///   `[i16; 17 + 5]`, and the encoder's half-pel refinement is what needs the 17
///   (`encoder/md.rs:1289` filters `iWidth + 1` columns).
#[inline(always)]
pub unsafe extern "C" fn McHorVer22_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver22
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[2][2],
            iWidth.max(0) as usize,
            mc_hor_ver22,
        )
    };
}

/// C++: `McHorVer01_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-0 .. iWidth + 0` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[0][1]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer01_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver01
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[0][1],
            iWidth.max(0) as usize,
            mc_hor_ver01,
        )
    };
}

/// C++: `McHorVer03_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-0 .. iWidth + 0` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[0][3]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer03_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver03
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[0][3],
            iWidth.max(0) as usize,
            mc_hor_ver03,
        )
    };
}

/// C++: `McHorVer10_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-0 .. iHeight + 0` — `LUMA_REACH[1][0]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer10_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver10
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[1][0],
            iWidth.max(0) as usize,
            mc_hor_ver10,
        )
    };
}

/// C++: `McHorVer11_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[1][1]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer11_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver11
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[1][1],
            iWidth.max(0) as usize,
            mc_hor_ver11,
        )
    };
}

/// C++: `McHorVer12_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[1][2]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer12_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver12
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[1][2],
            iWidth.max(0) as usize,
            mc_hor_ver12,
        )
    };
}

/// C++: `McHorVer13_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[1][3]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer13_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver13
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[1][3],
            iWidth.max(0) as usize,
            mc_hor_ver13,
        )
    };
}

/// C++: `McHorVer21_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[2][1]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer21_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver21
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[2][1],
            iWidth.max(0) as usize,
            mc_hor_ver21,
        )
    };
}

/// C++: `McHorVer23_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[2][3]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer23_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver23
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[2][3],
            iWidth.max(0) as usize,
            mc_hor_ver23,
        )
    };
}

/// C++: `McHorVer30_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-0 .. iHeight + 0` — `LUMA_REACH[3][0]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer30_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver30
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[3][0],
            iWidth.max(0) as usize,
            mc_hor_ver30,
        )
    };
}

/// C++: `McHorVer31_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[3][1]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer31_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver31
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[3][1],
            iWidth.max(0) as usize,
            mc_hor_ver31,
        )
    };
}

/// C++: `McHorVer32_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[3][2]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer32_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver32
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[3][2],
            iWidth.max(0) as usize,
            mc_hor_ver32,
        )
    };
}

/// C++: `McHorVer33_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block in a surface
///   whose rows are `iSrcStride` bytes apart. This kernel reads `x` in
///   `-2 .. iWidth + 3` and `y` in `-2 .. iHeight + 3` — `LUMA_REACH[3][3]` — and all
///   of it must be valid to read.
/// * For a decoder reference picture that follows from the motion-vector clamp in
///   `BaseMC` (`decoder/decode_slice.rs:1069-1091`) against the 32-sample
///   `PADDING_LENGTH` border, as derived in this section's header. `PADDING_LENGTH`
///   alone is **not** sufficient: `pSrc` has already been displaced by the vector.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else. The two spans must
///   not overlap, and both strides are positive.
/// * `iWidth` and `iHeight` are at most 16: this kernel interpolates through a
///   `[u8; 256]` scratch at stride 16, exactly as the C++ does.
#[inline(always)]
pub unsafe extern "C" fn McHorVer33_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver33
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[3][3],
            iWidth.max(0) as usize,
            mc_hor_ver33,
        )
    };
}

/// Horizontal luma half-pel motion compensation — the C++ names this separately from
/// `McHorVer20_c` and then defines it as the same function.
///
/// # Safety
/// As [`McHorVer20_c`].
#[inline(always)]
pub unsafe extern "C" fn McHorizLuma_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver20
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            R_HOR,
            iWidth.max(0) as usize,
            mc_hor_ver20,
        )
    };
}

/// Vertical luma half-pel motion compensation — the C++ alias of `McHorVer02_c`.
///
/// # Safety
/// As [`McHorVer02_c`].
#[inline(always)]
pub unsafe extern "C" fn McVertLuma_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_hor_ver02
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            R_VER,
            iWidth.max(0) as usize,
            mc_hor_ver02,
        )
    };
}

/// C++: `McLuma_c`, `codec/common/src/mc.cpp` — `SMcFunc::pMcLumaFunc`.
///
/// # Safety
/// As the quarter-pel kernels this dispatches to (`LUMA_REACH[iMvX & 3][iMvY & 3]`
/// selects both the kernel and its reach); the widest case is two samples and two
/// rows before the block and three after, and the decoder's guarantee for it is the
/// `BaseMC` clamp quoted in this section's header. `iWidth` and `iHeight` are at most
/// 16, which is what the kernels' `[u8; 256]` scratch at stride 16 holds.
// Phase 4a: the composites are `#[inline]` now that their callers name them
// directly. Inlining is the mechanism the recovery thesis rests on — it is what
// folds the shim's span arithmetic against the caller's constant block sizes.
#[inline]
pub unsafe extern "C" fn McLuma_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iMvX: i16,
    iMvY: i16,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_luma
    let (x, y) = ((iMvX & 0x03) as usize, (iMvY & 0x03) as usize);
    let w = iWidth.max(0) as usize;
    // (0, 0) is the copy path, which narrows the width it touches.
    let span_width = if x == 0 && y == 0 { copy_width(w) } else { w };
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            LUMA_REACH[x][y],
            span_width,
            |s, d, w, h| mc_luma(s, d, iMvX, iMvY, w, h),
        )
    };
}

/// C++: `McChromaWithFragMv_c`, `codec/common/src/mc.cpp`.
///
/// # Safety
/// * `pSrc` points at sample `(0, 0)` of an `iWidth` x `iHeight` block whose
///   bilinear neighbourhood — `x` in `0 .. iWidth + 1`, `y` in `0 .. iHeight + 1`,
///   one sample right and one row below — is valid to read. For a decoder reference
///   picture that follows from the `BaseMC` clamp against the 16-sample chroma
///   border, per this section's header.
/// * `pDst` points at sample `(0, 0)` of the destination block; writes span
///   `[0, (iHeight - 1) * iDstStride + iWidth)` and nothing else.
/// * The two spans must not overlap, and both strides must be positive.
#[inline(always)]
pub unsafe extern "C" fn McChromaWithFragMv_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iMvX: i16,
    iMvY: i16,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_chroma_with_frag_mv
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            R_CHROMA,
            iWidth.max(0) as usize,
            |s, d, w, h| mc_chroma_with_frag_mv(s, d, iMvX, iMvY, w, h),
        )
    };
}

/// C++: `McChroma_c`, `codec/common/src/mc.cpp` — `SMcFunc::pMcChromaFunc`.
///
/// # Safety
/// As [`McChromaWithFragMv_c`] when either eighth-pel fraction is non-zero; as
/// [`McCopy_c`] — block only, and the same narrowing of `iWidth` — when both are
/// zero.
// Phase 4a: the composites are `#[inline]` now that their callers name them
// directly. Inlining is the mechanism the recovery thesis rests on — it is what
// folds the shim's span arithmetic against the caller's constant block sizes.
#[inline]
pub unsafe extern "C" fn McChroma_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iMvX: i16,
    iMvY: i16,
    iWidth: i32,
    iHeight: i32,
) {
    // SHIM(phase2) -> mc_chroma
    let frag = (iMvX & 0x07) != 0 || (iMvY & 0x07) != 0;
    let w = iWidth.max(0) as usize;
    let (reach, span_width) = if frag {
        (R_CHROMA, w)
    } else {
        (R_COPY, copy_width(w))
    };
    unsafe {
        shim_wh(
            pSrc,
            iSrcStride,
            pDst,
            iDstStride,
            iWidth,
            iHeight,
            reach,
            span_width,
            |s, d, w, h| mc_chroma(s, d, iMvX, iMvY, w, h),
        )
    };
}

pub unsafe extern "C" fn InitMcFunc(pMcFuncs: *mut SMcFunc, _uiCpuFlag: u32) {
    if pMcFuncs.is_null() {
        return;
    }
    let mc = &mut *pMcFuncs;
    mc.pfLumaHalfpelHor = Some(McHorVer20_c);
    mc.pfLumaHalfpelVer = Some(McHorVer02_c);
    mc.pfLumaHalfpelCen = Some(McHorVer22_c);
    mc.pfSampleAveraging = Some(PixelAvg_c);
    mc.pMcChromaFunc = Some(McChroma_c);
    mc.pMcLumaFunc = Some(McLuma_c);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan §5's de-virtualization mitigation, half one: `InitMcFunc` is a
    /// **constant function of its CPU-flag argument**.
    ///
    /// That is the claim direct dispatch actually rests on — that there is one
    /// function per slot to call, not a family selected at run time. Every
    /// `_sse2`/`_neon` variant in this port is a delegating stub or a dead
    /// extern (Phase 0 deleted 62 dead ones from this module alone), so
    /// `_uiCpuFlag` selects nothing; this pins that, for every flag value the
    /// port can produce rather than for the one this machine reports.
    ///
    /// **Why this compares two tables instead of a table against named
    /// functions.** The obvious assert-map — `t.pMcLumaFunc.unwrap() as usize
    /// == McLuma_c as usize`, the shape
    /// `encoder_deblocking_table_installs_the_common_shims` uses — is *unsound
    /// for these six functions*, and both a cross-crate and an in-crate draft
    /// of it failed before this one worked. Four of them are
    /// `#[inline(always)]`, and an `#[inline(always)]` function whose address
    /// is taken gets instantiated locally in whatever codegen unit takes it:
    /// the integration-test crate gets its own copy, and so does this `tests`
    /// submodule. Neither address is the one `InitMcFunc` stored. The
    /// deblocking assert-map only works because those kernels happen to carry
    /// no inline attribute — luck, not design, and worth knowing before the
    /// next table is de-virtualized.
    ///
    /// Both addresses here come from the same `InitMcFunc` instantiation, so
    /// the comparison is meaningful. The complementary half — that the
    /// installed function *is* the one the direct calls name — is behavioural
    /// and lives in `tests/kernels_differential_phase2.rs`
    /// (`mc_table_slots_match_the_direct_calls`), where identity is proven by
    /// output rather than by symbol address.
    /// Not under Miri: it mints a fresh synthetic address for each reified
    /// function pointer, so even two calls of the *same* installer compare
    /// unequal there. The property this test states is about symbol identity,
    /// which Miri deliberately does not model; the behavioural half
    /// (`mc_table_slots_match_the_direct_calls`) does run under Miri and is
    /// what covers this path there.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn init_mc_func_ignores_the_cpu_flag() {
        use crate::common::cpu_core::*;
        let flags: [u32; 10] = [
            0, u32::MAX,
            WELS_CPU_SSE2, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_AVX,
            WELS_CPU_AVX2, WELS_CPU_NEON, WELS_CPU_MMI, WELS_CPU_LSX,
        ];
        let mut base = SMcFunc::default();
        unsafe { InitMcFunc(&mut base, 0) };
        let addrs = |t: &SMcFunc| -> [usize; 6] {
            [
                t.pfLumaHalfpelHor.unwrap() as usize,
                t.pfLumaHalfpelVer.unwrap() as usize,
                t.pfLumaHalfpelCen.unwrap() as usize,
                t.pfSampleAveraging.unwrap() as usize,
                t.pMcChromaFunc.unwrap() as usize,
                t.pMcLumaFunc.unwrap() as usize,
            ]
        };
        const NAMES: [&str; 6] = [
            "pfLumaHalfpelHor", "pfLumaHalfpelVer", "pfLumaHalfpelCen",
            "pfSampleAveraging", "pMcChromaFunc", "pMcLumaFunc",
        ];
        let want = addrs(&base);
        for flag in flags {
            let mut t = SMcFunc::default();
            unsafe { InitMcFunc(&mut t, flag) };
            for (i, (got, expected)) in addrs(&t).into_iter().zip(want).enumerate() {
                assert_eq!(
                    got, expected,
                    "cpu flag {flag:#x} selected a different function for slot {}",
                    NAMES[i]
                );
            }
        }
    }

    /// The other half of the de-virtualization argument: the slots were never
    /// `None` at a call site, so unconditional direct calls preserve behaviour.
    ///
    /// This is the half that is easy to wave through. Five of the fifteen
    /// former call sites spelled the dispatch `if let Some(f) = ...`, which
    /// *silently skips the call* on `None` — for MC, that means leaving a
    /// prediction block unwritten rather than filling it. Replacing those with
    /// unconditional calls is behaviour-preserving only because `InitMcFunc`
    /// runs unconditionally at codec-open time on both sides
    /// (`WelsInitDecoderFuncs` via `WelsOpenDecoder`; the encoder's
    /// `InitFunctionPointers`), before any frame is touched. A
    /// default-constructed table being all-`None` is what makes the difference
    /// observable rather than academic.
    #[test]
    fn mc_table_is_all_none_before_init_and_all_some_after() {
        let t = SMcFunc::default();
        assert!(
            t.pMcLumaFunc.is_none() && t.pMcChromaFunc.is_none() && t.pfSampleAveraging.is_none()
                && t.pfLumaHalfpelHor.is_none() && t.pfLumaHalfpelVer.is_none()
                && t.pfLumaHalfpelCen.is_none(),
            "a default SMcFunc must be all-None, or the post-init claim proves nothing"
        );
        let mut t = SMcFunc::default();
        unsafe { InitMcFunc(&mut t, 0) };
        assert!(
            t.pMcLumaFunc.is_some() && t.pMcChromaFunc.is_some() && t.pfSampleAveraging.is_some()
                && t.pfLumaHalfpelHor.is_some() && t.pfLumaHalfpelVer.is_some()
                && t.pfLumaHalfpelCen.is_some(),
            "InitMcFunc must leave every slot populated"
        );
    }

    /// The two aliases really are `McHorVer20_c` and `McHorVer02_c`.
    ///
    /// This test used to anchor a 4x4 block at `src.as_ptr()` of a bare `[u8; 64]`
    /// and filter it, which reads `pSrc[-2]` (horizontal) and `pSrc[-2 * 8]`
    /// (vertical) — off the front of the array, in a test whose whole subject is a
    /// kernel that reaches outside its block. The old raw code did that read
    /// silently; the shim now materialises the span as a slice, so it is stated
    /// instead. The block is anchored inside a padded surface here, which is what a
    /// real caller hands these kernels.
    #[test]
    fn test_mc_horiz_and_vert_luma_aliases() {
        const STRIDE: usize = 16;
        const PAD: usize = 4;
        let mut src = [0u8; STRIDE * 16];
        for (i, v) in src.iter_mut().enumerate() {
            *v = i as u8;
        }
        let center = PAD * STRIDE + PAD;
        let mut dst_hor = [0u8; 64];
        let mut dst_vert = [0u8; 64];
        let mut want_hor = [0u8; 64];
        let mut want_vert = [0u8; 64];

        unsafe {
            McHorizLuma_c(src.as_ptr().add(center), STRIDE as i32, dst_hor.as_mut_ptr(), 8, 4, 4);
            McVertLuma_c(src.as_ptr().add(center), STRIDE as i32, dst_vert.as_mut_ptr(), 8, 4, 4);
            McHorVer20_c(src.as_ptr().add(center), STRIDE as i32, want_hor.as_mut_ptr(), 8, 4, 4);
            McHorVer02_c(src.as_ptr().add(center), STRIDE as i32, want_vert.as_mut_ptr(), 8, 4, 4);
        }

        assert!(dst_hor.iter().any(|&x| x != 0));
        assert!(dst_vert.iter().any(|&x| x != 0));
        assert_eq!(dst_hor, want_hor, "McHorizLuma_c must be McHorVer20_c");
        assert_eq!(dst_vert, want_vert, "McVertLuma_c must be McHorVer02_c");
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_3DNOW, WELS_CPU_3DNOWEXT, WELS_CPU_ALTIVEC, WELS_CPU_ARMv7, WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MMX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE, WELS_CPU_SSE2, WELS_CPU_SSE3, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3, WELS_CPU_VFPv3};
