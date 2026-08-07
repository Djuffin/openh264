#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

// CPU feature flags from cpu_core.h

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

// Declarations of extern assembly routines matching mc.h prototypes
unsafe extern "C" {
    // ARM NEON (32-bit)
    pub fn McCopyWidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McCopyWidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McCopyWidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McChromaWidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, pWeights: *mut i32, iHeight: i32);
    pub fn McChromaWidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, pWeights: *mut i32, iHeight: i32);
    pub fn PixelAvgWidthEq16_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *mut u8, pSrcB: *mut u8, iHeight: i32);
    pub fn PixelAvgWidthEq8_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *mut u8, pSrcB: *mut u8, iHeight: i32);
    pub fn PixelAvgWidthEq4_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *mut u8, pSrcB: *mut u8, iHeight: i32);
    pub fn McHorVer01WidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer01WidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer01WidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer03WidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer03WidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer03WidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer10WidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer10WidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer10WidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer30WidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer30WidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer30WidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer20WidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer20WidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer20WidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer02WidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer02WidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer02WidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer22WidthEq16_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer22WidthEq8_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer22WidthEq4_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn PixStrideAvgWidthEq16_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcStrideA: i32, pSrcB: *const u8, iSrcStrideB: i32, iHeight: i32);
    pub fn PixStrideAvgWidthEq8_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcStrideA: i32, pSrcB: *const u8, iSrcStrideB: i32, iHeight: i32);

    // ARM NEON AArch64 (64-bit)
    pub fn McCopyWidthEq4_AArch64_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McCopyWidthEq8_AArch64_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McCopyWidthEq16_AArch64_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McChromaWidthEq8_AArch64_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, pWeights: *mut i32, iHeight: i32);
    pub fn McChromaWidthEq4_AArch64_neon(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, pWeights: *mut i32, iHeight: i32);
    pub fn PixelAvgWidthEq16_AArch64_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
    pub fn PixelAvgWidthEq8_AArch64_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
    pub fn PixelAvgWidthEq4_AArch64_neon(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);

    // x86 MMX / SSE2 / SSSE3 / AVX2
    pub fn McHorVer20WidthEq4_mmx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McChromaWidthEq4_mmx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, kpABCD: *const u8, iHeight: i32);
    pub fn McCopyWidthEq8_mmx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn PixelAvgWidthEq4_mmx(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
    pub fn PixelAvgWidthEq8_mmx(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
    pub fn McChromaWidthEq8_sse2(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, kpABCD: *const u8, iHeight: i32);
    pub fn McCopyWidthEq16_sse2(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer20WidthEq8_sse2(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer20WidthEq16_sse2(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McHorVer02WidthEq8_sse2(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn PixelAvgWidthEq16_sse2(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
    pub fn McCopyWidthEq16_sse3(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McChromaWidthEq8_ssse3(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, kpABCD: *const u8, iHeight: i32);
    pub fn McHorVer02_ssse3(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iWidth: i32, iHeight: i32);
    pub fn McHorVer20_ssse3(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iWidth: i32, iHeight: i32);

    // Loongson LSX
    pub fn McCopyWidthEq4_lsx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McCopyWidthEq8_lsx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McCopyWidthEq16_lsx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, iHeight: i32);
    pub fn McChromaWidthEq4_lsx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, pABCD: *const u8, iHeight: i32);
    pub fn McChromaWidthEq8_lsx(pSrc: *const u8, iSrcStride: i32, pDst: *mut u8, iDstStride: i32, pABCD: *const u8, iHeight: i32);
    pub fn PixelAvgWidthEq4_lsx(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
    pub fn PixelAvgWidthEq8_lsx(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
    pub fn PixelAvgWidthEq16_lsx(pDst: *mut u8, iDstStride: i32, pSrcA: *const u8, iSrcAStride: i32, pSrcB: *const u8, iSrcBStride: i32, iHeight: i32);
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
