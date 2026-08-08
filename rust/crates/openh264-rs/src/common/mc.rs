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

/// `width` bytes of each of `height` rows, source to destination.
///
/// One `copy_from_slice` per row: the same wide moves the `LD64`/`ST64A8` pairs in
/// the C++ existed for, with the bounds check landing per row rather than per sample.
#[inline(always)]
fn copy_rows(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    for dy in 0..height as isize {
        dst.row_mut(dy, 0, width).copy_from_slice(src.row(dy, 0, width));
    }
}

/// C++: `McCopyWidthEq2_c` — chroma only, the one width the copy path narrows to.
pub fn mc_copy_width_eq2(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows(src, dst, 2, height);
}

/// C++: `McCopyWidthEq4_c`.
pub fn mc_copy_width_eq4(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows(src, dst, 4, height);
}

/// C++: `McCopyWidthEq8_c`.
pub fn mc_copy_width_eq8(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows(src, dst, 8, height);
}

/// C++: `McCopyWidthEq16_c`.
pub fn mc_copy_width_eq16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, height: usize) {
    copy_rows(src, dst, 16, height);
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
pub fn mc_copy(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>, width: usize, height: usize) {
    copy_rows(src, dst, copy_width(width), height);
}

/// C++: `PixelAvg_c` — the rounded average of two surfaces, `SMcFunc::pfSampleAveraging`.
///
/// The three surfaces carry their own strides because the encoder's quarter-pel
/// refinement averages a `ME_REFINE_BUF_STRIDE` scratch buffer against the reference
/// picture (`encoder/md.rs:1059`).
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
pub fn mc_hor_ver02(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        // Six row windows, then a column walk across them — six bounds checks per
        // output row rather than six per output sample.
        let r = [
            src.row(dy - 2, 0, width),
            src.row(dy - 1, 0, width),
            src.row(dy, 0, width),
            src.row(dy + 1, 0, width),
            src.row(dy + 2, 0, width),
            src.row(dy + 3, 0, width),
        ];
        let out = dst.row_mut(dy, 0, width);
        for j in 0..width {
            let p = [r[0][j], r[1][j], r[2][j], r[3][j], r[4][j], r[5][j]];
            out[j] = WelsClip1((filter_input_8bit(&p) + 16) >> 5);
        }
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
pub fn mc_hor_ver22(
    src: &PlaneCursor<'_>,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut iTmp = [0i16; 17 + 5];
    let n = width + 5;
    for dy in 0..height as isize {
        let r = [
            src.row(dy - 2, -2, n),
            src.row(dy - 1, -2, n),
            src.row(dy, -2, n),
            src.row(dy + 1, -2, n),
            src.row(dy + 2, -2, n),
            src.row(dy + 3, -2, n),
        ];
        for j in 0..n {
            let p = [r[0][j], r[1][j], r[2][j], r[3][j], r[4][j], r[5][j]];
            iTmp[j] = filter_input_8bit(&p) as i16;
        }
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

#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq2_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    for _ in 0..iHeight {
        std::ptr::copy_nonoverlapping(pSrc, pDst, 2);
        pDst = pDst.offset(iDstStride as isize);
        pSrc = pSrc.offset(iSrcStride as isize);
    }
}

#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq4_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    for _ in 0..iHeight {
        std::ptr::copy_nonoverlapping(pSrc, pDst, 4);
        pDst = pDst.offset(iDstStride as isize);
        pSrc = pSrc.offset(iSrcStride as isize);
    }
}

#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq8_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    for _ in 0..iHeight {
        std::ptr::copy_nonoverlapping(pSrc, pDst, 8);
        pDst = pDst.offset(iDstStride as isize);
        pSrc = pSrc.offset(iSrcStride as isize);
    }
}

#[inline(always)]
pub unsafe extern "C" fn McCopyWidthEq16_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iHeight: i32,
) {
    for _ in 0..iHeight {
        std::ptr::copy_nonoverlapping(pSrc, pDst, 16);
        pDst = pDst.offset(iDstStride as isize);
        pSrc = pSrc.offset(iSrcStride as isize);
    }
}

#[inline(always)]
pub unsafe extern "C" fn McCopy_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    if iWidth == 16 {
        McCopyWidthEq16_c(pSrc, iSrcStride, pDst, iDstStride, iHeight);
    } else if iWidth == 8 {
        McCopyWidthEq8_c(pSrc, iSrcStride, pDst, iDstStride, iHeight);
    } else if iWidth == 4 {
        McCopyWidthEq4_c(pSrc, iSrcStride, pDst, iDstStride, iHeight);
    } else {
        McCopyWidthEq2_c(pSrc, iSrcStride, pDst, iDstStride, iHeight);
    }
}

#[inline(always)]
pub unsafe fn HorFilterInput16bit_c(pSrc: *const i16) -> i32 {
    let iPix05 = (*pSrc.add(0) as i32) + (*pSrc.add(5) as i32);
    let iPix14 = (*pSrc.add(1) as i32) + (*pSrc.add(4) as i32);
    let iPix23 = (*pSrc.add(2) as i32) + (*pSrc.add(3) as i32);
    iPix05 - (iPix14 * 5) + (iPix23 * 20)
}

#[inline(always)]
pub unsafe fn FilterInput8bitWithStride_c(pSrc: *const u8, kiOffset: i32) -> i32 {
    let kiOffset1 = kiOffset as isize;
    let kiOffset2 = kiOffset1 << 1;
    let kiOffset3 = kiOffset1 + kiOffset2;
    let kuiPix05 = (*pSrc.offset(-kiOffset2) as u32) + (*pSrc.offset(kiOffset3) as u32);
    let kuiPix14 = (*pSrc.offset(-kiOffset1) as u32) + (*pSrc.offset(kiOffset2) as u32);
    let kuiPix23 = (*pSrc as u32) + (*pSrc.offset(kiOffset1) as u32);

    (kuiPix05 as i32)
        - (((kuiPix14 << 2) + kuiPix14) as i32)
        + (((kuiPix23 << 4) + (kuiPix23 << 2)) as i32)
}

#[inline(always)]
pub unsafe extern "C" fn PixelAvg_c(
    mut pDst: *mut u8,
    iDstStride: i32,
    mut pSrcA: *const u8,
    iSrcAStride: i32,
    mut pSrcB: *const u8,
    iSrcBStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    for _ in 0..iHeight {
        for j in 0..iWidth as isize {
            *pDst.offset(j) = (((*pSrcA.offset(j) as u32) + (*pSrcB.offset(j) as u32) + 1) >> 1) as u8;
        }
        pDst = pDst.offset(iDstStride as isize);
        pSrcA = pSrcA.offset(iSrcAStride as isize);
        pSrcB = pSrcB.offset(iSrcBStride as isize);
    }
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer20_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    for _ in 0..iHeight {
        for j in 0..iWidth as isize {
            *pDst.offset(j) = WelsClip1((FilterInput8bitWithStride_c(pSrc.offset(j), 1) + 16) >> 5);
        }
        pDst = pDst.offset(iDstStride as isize);
        pSrc = pSrc.offset(iSrcStride as isize);
    }
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer02_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    for _ in 0..iHeight {
        for j in 0..iWidth as isize {
            *pDst.offset(j) =
                WelsClip1((FilterInput8bitWithStride_c(pSrc.offset(j), iSrcStride) + 16) >> 5);
        }
        pDst = pDst.offset(iDstStride as isize);
        pSrc = pSrc.offset(iSrcStride as isize);
    }
}

/// Horizontal luma half-pel motion compensation (`McHorizLuma_c`, alias for `McHorVer20_c`).
#[inline(always)]
pub unsafe extern "C" fn McHorizLuma_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    McHorVer20_c(pSrc, iSrcStride, pDst, iDstStride, iWidth, iHeight);
}

/// Vertical luma half-pel motion compensation (`McVertLuma_c`, alias for `McHorVer02_c`).
#[inline(always)]
pub unsafe extern "C" fn McVertLuma_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    McHorVer02_c(pSrc, iSrcStride, pDst, iDstStride, iWidth, iHeight);
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer22_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut iTmp = [0i16; 17 + 5];
    for _ in 0..iHeight {
        for j in 0..(iWidth + 5) as isize {
            iTmp[j as usize] = FilterInput8bitWithStride_c(pSrc.offset(-2 + j), iSrcStride) as i16;
        }
        for k in 0..iWidth as isize {
            *pDst.offset(k) = WelsClip1((HorFilterInput16bit_c(iTmp.as_ptr().offset(k)) + 512) >> 10);
        }
        pSrc = pSrc.offset(iSrcStride as isize);
        pDst = pDst.offset(iDstStride as isize);
    }
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer01_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiTmp = [0u8; 256];
    McHorVer02_c(pSrc, iSrcStride, uiTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(pDst, iDstStride, pSrc, iSrcStride, uiTmp.as_ptr(), 16, iWidth, iHeight);
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer03_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiTmp = [0u8; 256];
    McHorVer02_c(pSrc, iSrcStride, uiTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        pSrc.offset(iSrcStride as isize),
        iSrcStride,
        uiTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer10_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiTmp = [0u8; 256];
    McHorVer20_c(pSrc, iSrcStride, uiTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(pDst, iDstStride, pSrc, iSrcStride, uiTmp.as_ptr(), 16, iWidth, iHeight);
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer11_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiHorTmp = [0u8; 256];
    let mut uiVerTmp = [0u8; 256];
    McHorVer20_c(pSrc, iSrcStride, uiHorTmp.as_mut_ptr(), 16, iWidth, iHeight);
    McHorVer02_c(pSrc, iSrcStride, uiVerTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiHorTmp.as_ptr(),
        16,
        uiVerTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer12_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiVerTmp = [0u8; 256];
    let mut uiCtrTmp = [0u8; 256];
    McHorVer02_c(pSrc, iSrcStride, uiVerTmp.as_mut_ptr(), 16, iWidth, iHeight);
    McHorVer22_c(pSrc, iSrcStride, uiCtrTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiVerTmp.as_ptr(),
        16,
        uiCtrTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer13_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiHorTmp = [0u8; 256];
    let mut uiVerTmp = [0u8; 256];
    McHorVer20_c(
        pSrc.offset(iSrcStride as isize),
        iSrcStride,
        uiHorTmp.as_mut_ptr(),
        16,
        iWidth,
        iHeight,
    );
    McHorVer02_c(pSrc, iSrcStride, uiVerTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiHorTmp.as_ptr(),
        16,
        uiVerTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer21_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiHorTmp = [0u8; 256];
    let mut uiCtrTmp = [0u8; 256];
    McHorVer20_c(pSrc, iSrcStride, uiHorTmp.as_mut_ptr(), 16, iWidth, iHeight);
    McHorVer22_c(pSrc, iSrcStride, uiCtrTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiHorTmp.as_ptr(),
        16,
        uiCtrTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer23_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiHorTmp = [0u8; 256];
    let mut uiCtrTmp = [0u8; 256];
    McHorVer20_c(
        pSrc.offset(iSrcStride as isize),
        iSrcStride,
        uiHorTmp.as_mut_ptr(),
        16,
        iWidth,
        iHeight,
    );
    McHorVer22_c(pSrc, iSrcStride, uiCtrTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiHorTmp.as_ptr(),
        16,
        uiCtrTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer30_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiHorTmp = [0u8; 256];
    McHorVer20_c(pSrc, iSrcStride, uiHorTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        pSrc.offset(1),
        iSrcStride,
        uiHorTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer31_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiHorTmp = [0u8; 256];
    let mut uiVerTmp = [0u8; 256];
    McHorVer20_c(pSrc, iSrcStride, uiHorTmp.as_mut_ptr(), 16, iWidth, iHeight);
    McHorVer02_c(pSrc.offset(1), iSrcStride, uiVerTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiHorTmp.as_ptr(),
        16,
        uiVerTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer32_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiVerTmp = [0u8; 256];
    let mut uiCtrTmp = [0u8; 256];
    McHorVer02_c(pSrc.offset(1), iSrcStride, uiVerTmp.as_mut_ptr(), 16, iWidth, iHeight);
    McHorVer22_c(pSrc, iSrcStride, uiCtrTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiVerTmp.as_ptr(),
        16,
        uiCtrTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

#[inline(always)]
pub unsafe extern "C" fn McHorVer33_c(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let mut uiHorTmp = [0u8; 256];
    let mut uiVerTmp = [0u8; 256];
    McHorVer20_c(
        pSrc.offset(iSrcStride as isize),
        iSrcStride,
        uiHorTmp.as_mut_ptr(),
        16,
        iWidth,
        iHeight,
    );
    McHorVer02_c(pSrc.offset(1), iSrcStride, uiVerTmp.as_mut_ptr(), 16, iWidth, iHeight);
    PixelAvg_c(
        pDst,
        iDstStride,
        uiHorTmp.as_ptr(),
        16,
        uiVerTmp.as_ptr(),
        16,
        iWidth,
        iHeight,
    );
}

pub static pWelsMcFunc_c: [[PWelsMcWidthHeightFunc; 4]; 4] = [
    [McCopy_c, McHorVer01_c, McHorVer02_c, McHorVer03_c],
    [McHorVer10_c, McHorVer11_c, McHorVer12_c, McHorVer13_c],
    [McHorVer20_c, McHorVer21_c, McHorVer22_c, McHorVer23_c],
    [McHorVer30_c, McHorVer31_c, McHorVer32_c, McHorVer33_c],
];

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
    let x_idx = (iMvX & 0x03) as usize;
    let y_idx = (iMvY & 0x03) as usize;
    pWelsMcFunc_c[x_idx][y_idx](pSrc, iSrcStride, pDst, iDstStride, iWidth, iHeight);
}

#[inline(always)]
pub unsafe extern "C" fn McChromaWithFragMv_c(
    mut pSrc: *const u8,
    iSrcStride: i32,
    mut pDst: *mut u8,
    iDstStride: i32,
    iMvX: i16,
    iMvY: i16,
    iWidth: i32,
    iHeight: i32,
) {
    let mut pSrcNext = pSrc.offset(iSrcStride as isize);
    let pABCD = &g_kuiABCD[(iMvY & 0x07) as usize][(iMvX & 0x07) as usize];
    let iA = pABCD[0] as i32;
    let iB = pABCD[1] as i32;
    let iC = pABCD[2] as i32;
    let iD = pABCD[3] as i32;

    for _ in 0..iHeight {
        for j in 0..iWidth as isize {
            *pDst.offset(j) = ((iA * (*pSrc.offset(j) as i32)
                + iB * (*pSrc.offset(j + 1) as i32)
                + iC * (*pSrcNext.offset(j) as i32)
                + iD * (*pSrcNext.offset(j + 1) as i32)
                + 32)
                >> 6) as u8;
        }
        pDst = pDst.offset(iDstStride as isize);
        pSrc = pSrcNext;
        pSrcNext = pSrcNext.offset(iSrcStride as isize);
    }
}

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
    let kiD8x = iMvX & 0x07;
    let kiD8y = iMvY & 0x07;
    if kiD8x == 0 && kiD8y == 0 {
        McCopy_c(pSrc, iSrcStride, pDst, iDstStride, iWidth, iHeight);
    } else {
        McChromaWithFragMv_c(pSrc, iSrcStride, pDst, iDstStride, iMvX, iMvY, iWidth, iHeight);
    }
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

    #[test]
    fn test_mc_horiz_and_vert_luma_aliases() {
        unsafe {
            let mut src = [0u8; 64];
            for i in 0..64 {
                src[i] = i as u8;
            }
            let mut dst_hor = [0u8; 64];
            let mut dst_vert = [0u8; 64];

            McHorizLuma_c(src.as_ptr(), 8, dst_hor.as_mut_ptr(), 8, 4, 4);
            McVertLuma_c(src.as_ptr(), 8, dst_vert.as_mut_ptr(), 8, 4, 4);

            assert!(dst_hor.iter().any(|&x| x != 0));
            assert!(dst_vert.iter().any(|&x| x != 0));
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_3DNOW, WELS_CPU_3DNOWEXT, WELS_CPU_ALTIVEC, WELS_CPU_ARMv7, WELS_CPU_AVX, WELS_CPU_AVX2, WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MMX, WELS_CPU_MMXEXT, WELS_CPU_NEON, WELS_CPU_SSE, WELS_CPU_SSE2, WELS_CPU_SSE3, WELS_CPU_SSE41, WELS_CPU_SSE42, WELS_CPU_SSSE3, WELS_CPU_VFPv3};
