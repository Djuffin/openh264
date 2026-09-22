#![deny(unsafe_code)]

//! CABAC decoder engine — `cabac_decoder.h`, `cabac_decoder.cpp`.
//!
//! # Read extents
//!
//! Only two functions load bytes; nothing else in this file touches the buffer. Every
//! `Decode*` entry point reaches it through [`Read32BitsCabac`] alone.
//!
//! [`InitCabacDecEngineFromBS`] — the 5-byte prime, max index `len + 2`.
//! `curr = pos - remaining_bytes` with `remaining_bytes = ((-left_bits) >> 3) + 2 ∈
//! [0, 4]` (`left_bits ∈ [-16, 15]` on every path that reaches here), guarded by
//! `curr <= len - 2`, then loads `curr[0..=4]`. **Needs `avail >= len + 3`**, so this is
//! the one site that takes the wider [`RawDataBuffer::window_from`] window rather than
//! the RBSP one, and it reads through `get`: a violated contract is an error return, not
//! a panic and not a read past the allocation.
//!
//! [`Read32BitsCabac`] — the 4/3/2/1 end ladder, max index `len - 1`, because its
//! selector is measured against `pBuffEnd`:
//!
//! | `iLeftBytes` | loads | largest index |
//! |---|---|---|
//! | `<= 0` | none — error return | — |
//! | `1` / `2` / `3` | `curr[0..n)` | `curr + n - 1 = len - 1` |
//! | `>= 4` | `curr[0..4)` | `curr + 3 <= len - 1` |
//!
//! It needs `avail >= len` and nothing more, so it takes a slice of exactly `len` bytes
//! ([`RawDataBuffer::rbsp_window`]) and `buf.len()` *is* `pBuffEnd - pBuffStart`.
//! `iLeftBytes` is genuinely negative in practice: init leaves the position at
//! `curr + 5 <= len + 3`, so a stream truncated into its first CABAC bytes enters the
//! ladder at `-3`, takes the `<= 0` arm and returns `ERR_CABAC_NO_BS_TO_READ` having
//! loaded nothing. The predicate is therefore the **comparison** `pos >= len` and not a
//! subtraction: `len - pos` in `usize` would wrap to a huge positive and select the
//! 4-byte arm.
//!
//! [`RestoreCabacDecEngineToBS`] loads nothing — position only.
//!
//! `len` = `cursor.len()` = `pBuffEnd - pBuffStart`, the logical RBSP end. The readable
//! extent past it is derived from the owning [`RawDataBuffer`] at call time
//! (`window_from`); `window.len() >= len + 4` structurally, since `WelsDecodeBs` sizes
//! every payload with four bytes to spare, which covers the prime's `len + 2`.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

pub const WELS_CABAC_HALF: u64 = 0x01FE;
pub const WELS_CABAC_QUARTER: u64 = 0x0100;
pub use crate::common::cabac_tables::{
    CTX_NA, WELS_CONTEXT_COUNT, g_kiCabacGlobalContextIdx, g_kuiCabacRangeLps, g_kuiStateTransTable,
};
pub const WELS_QP_MAX: i32 = 51;

pub const ERR_NONE: i32 = 0;
pub const ERR_LEVEL_MB_DATA: i32 = 7;
pub const ERR_INFO_INVALID_ACCESS: i32 = 2;
pub const ERR_CABAC_NO_BS_TO_READ: i32 = 201;
pub const ERR_CABAC_UNEXPECTED_VALUE: i32 = 202;

pub const I_SLICE: u8 = 2;

#[inline(always)]
pub const fn GENERATE_ERROR_NO(iErrLevel: i32, iErrInfo: i32) -> i32 {
    (iErrLevel << 16) | (iErrInfo & 0xFFFF)
}

pub use crate::common::macros::WELS_CLIP3_I32 as WELS_CLIP3;

pub const g_kRenormTable256: [u8; 256] = [
    6, 6, 6, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];

pub const g_kMvdBinPos2Ctx: [i16; 8] = [0, 1, 2, 3, 3, 3, 3, 3];

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SWelsCabacCtx {
    pub uiState: u8,
    pub uiMPS: u8,
}

/// The decoder context's four CABAC model tables — `sWelsCabacContexts`'s own type.
pub type CabacModelTables = [[[SWelsCabacCtx; WELS_CONTEXT_COUNT]; WELS_QP_MAX as usize + 1]; 4];

/// The arithmetic-decoding engine state — a **detached position**.
///
/// `uiRange`, `uiOffset` and `iBitsLeft` are the arithmetic state; `pos` replaces the
/// C++ pointer triple `pBuffStart`/`pBuffCurr`/`pBuffEnd`. The buffer is the caller's
/// and is passed per call, and its RBSP end is `buf.len()` of the window
/// ([`RawDataBuffer::rbsp_window`]). Field order matters: `uiRange` and `uiOffset` stay
/// adjacent so they load as a pair.
///
/// `SWelsCabacDecEngine::default()` zeroes this at allocation, and a zeroed
/// engine is inert rather than null-pointered: `pos = 0` with an empty window takes the
/// ladder's error arm.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SWelsCabacDecEngine {
    pub uiRange: u64,
    pub uiOffset: u64,
    pub iBitsLeft: i32,
    /// Byte offset into the slice's RBSP — the C++ `pBuffCurr - pBuffStart`.
    pub pos: usize,
}

pub use crate::decoder::bit_stream::{BsReader, RawDataBuffer};

pub use crate::decoder::decoder_context::SWelsDecoderContext;

const fn build_cabac_model_tables() -> CabacModelTables {
    let mut contexts = [[[SWelsCabacCtx {
        uiState: 0,
        uiMPS: 0,
    }; WELS_CONTEXT_COUNT]; WELS_QP_MAX as usize + 1]; 4];
    let mut iModel = 0;
    while iModel < 4 {
        let mut iQp = 0i32;
        while iQp <= WELS_QP_MAX {
            let mut iIdx = 0;
            while iIdx < WELS_CONTEXT_COUNT {
                let m = g_kiCabacGlobalContextIdx[iIdx][iModel][0] as i32;
                let n = g_kiCabacGlobalContextIdx[iIdx][iModel][1] as i32;
                let iPreCtxState = WELS_CLIP3(((m * iQp) >> 4) + n, 1, 126);
                let (uiStateIdx, uiValMps) = if iPreCtxState <= 63 {
                    ((63 - iPreCtxState) as u8, 0)
                } else {
                    ((iPreCtxState - 64) as u8, 1)
                };
                contexts[iModel][iQp as usize][iIdx] = SWelsCabacCtx {
                    uiState: uiStateIdx,
                    uiMPS: uiValMps,
                };
                iIdx += 1;
            }
            iQp += 1;
        }
        iModel += 1;
    }
    contexts
}

pub static G_WELS_CABAC_CONTEXTS: CabacModelTables = build_cabac_model_tables();

// 1. CABAC context initialization
pub fn WelsCabacGlobalInit(contexts: &mut CabacModelTables, inited: &mut bool) {
    *contexts = G_WELS_CABAC_CONTEXTS;
    *inited = true;
}

pub fn WelsCabacContextInit(
    inited: &mut bool,
    active: &mut [SWelsCabacCtx; WELS_CONTEXT_COUNT],
    eSliceType: u8,
    iCabacInitIdc: i32,
    iQp: i32,
) {
    let iIdx = if eSliceType as i32 == I_SLICE as i32 {
        0
    } else {
        (iCabacInitIdc + 1) as usize
    };
    *inited = true;
    let qp_idx = iQp as usize;
    let model_idx = iIdx;
    *active = G_WELS_CABAC_CONTEXTS[model_idx][qp_idx];
}

// 2. Decoding engine initialization
/// Primes the engine from the CAVLC cursor's position — the only place in this module
/// that reads past the RBSP (`len + 2`, needing `avail >= len + 3`).
///
/// # The rewind cannot underflow
///
/// `curr = pos - remaining_bytes` with `remaining_bytes ∈ [0, 4]` (from
/// `left_bits ∈ [-16, 15]`). Every path into this function has primed the cursor since
/// the last write to `pos` — `DecInitBits` → `BsCursor::init` sets `pos = 4` for the
/// slice-header path, and `InitReadBits` → `init_read_bits` does `pos += 4` for the
/// I_PCM re-entry — and both leave **`pos >= 4`**. The bound is tight: a cursor primed
/// and not yet advanced gives `pos = 4, remaining_bytes = 4`, landing exactly on
/// `curr = 0`.
pub fn InitCabacDecEngineFromBS(
    pDecEngine: &mut SWelsCabacDecEngine,
    pBsAux: &mut BsReader,
    raw: &RawDataBuffer,
) -> i32 {
    {
        let pos = pBsAux.cursor.pos() as isize;
        let len = pBsAux.cursor.len() as isize;
        let iRemainingBits = -pBsAux.cursor.left_bits();
        let iRemainingBytes = ((iRemainingBits >> 3) + 2) as isize;
        debug_assert!(
            (0..=4).contains(&iRemainingBytes),
            "left_bits {} out of [-16, 15]",
            pBsAux.cursor.left_bits()
        );
        debug_assert!(
            pos >= iRemainingBytes,
            "CABAC init rewind underflows: pos {} - {} (a primed cursor has pos >= 4)",
            pos,
            iRemainingBytes
        );
        let iCurr = pos - iRemainingBytes;
        // `pCurr >= pEndBuf - 1`, in offsets. Signed on both sides so a zero-length
        // window compares rather than wraps.
        if iCurr >= len - 1 {
            return ERR_INFO_INVALID_ACCESS;
        }
        let curr = iCurr as usize;

        // The wider window, derived from the owning buffer at call time. The guard above
        // bounds `curr <= len - 2`, so `curr + 5 <= len + 3 <= window.len()`; the `get`
        // is therefore unreachable-None, and routes a violated contract to the error
        // path instead of past the end of the allocation.
        let buf = raw.window_from(pBsAux.start);
        let b = match buf.get(curr..curr + 5) {
            Some(b) => b,
            None => return ERR_INFO_INVALID_ACCESS,
        };

        let mut uiOffset = ((b[0] as u64) << 16) | ((b[1] as u64) << 8) | (b[2] as u64);
        uiOffset <<= 16;
        uiOffset |= ((b[3] as u64) << 8) | (b[4] as u64);

        pDecEngine.uiOffset = uiOffset;
        pDecEngine.iBitsLeft = 31;
        pDecEngine.pos = curr + 5;
        pDecEngine.uiRange = WELS_CABAC_HALF;
        pBsAux.cursor.hand_off_to_cabac();

        ERR_NONE
    }
}

/// Hands the position back to the CAVLC cursor. Loads nothing; only the position moves.
///
/// # The rewind cannot underflow either
///
/// `pos - (bits_left >> 3)` with `bits_left <= 63` (init sets 31; `Read32BitsCabac`
/// adds at most 32 before any consumer subtracts), so the rewind is at most 7 bytes.
/// The quantity is invariant under a refill — `pos += k` and `bits_left += 8k` cancel —
/// and only *increases* under renormalisation, starting from `curr + 5 - 3 = curr + 2`.
/// On the error path `bits_left` goes negative, the arithmetic shift goes negative and
/// the position moves *forward*, which is why this is done in `isize` and cast once.
pub fn RestoreCabacDecEngineToBS(pDecEngine: &mut SWelsCabacDecEngine, pBsAux: &mut BsReader) {
    {
        let back = (pDecEngine.iBitsLeft >> 3) as isize;
        let pos = pDecEngine.pos as isize - back;
        debug_assert!(pos >= 0, "CABAC restore rewind underflows: pos {}", pos);
        pDecEngine.pos = pos as usize;
        pDecEngine.iBitsLeft = 0;
        pBsAux.cursor.restore_from_cabac(pDecEngine.pos);
    }
}

// 3. Actual decoding
/// The refill — the 4/3/2/1 end ladder, bounded by `len - 1`.
///
/// `win` is the RBSP window ([`RawDataBuffer::rbsp_window`]): `win.len()` **is** the C++
/// `pBuffEnd - pBuffStart`, so the selector is the slice's own length and the engine
/// computes no extent of its own.
///
/// The `pos >= win.len()` test is `iLeftBytes <= 0` written as a comparison rather than a
/// subtraction: `pos` legitimately exceeds `win.len()` after init on a truncated stream,
/// and `win.len() - pos` in `usize` would wrap to a huge positive and select the 4-byte
/// arm.
///
/// The arms use `first_chunk::<N>()` so the width is a *type* and the bounds checks fold.
/// `>= 4` is tested first to put the common case at the top; the four widths are a
/// disjoint partition, so the order is free. The final `else` is `tail.len() == 1` — `0`
/// was rejected by the guard above — and its `tail[0]` folds on that fact.
///
/// `#[inline(always)]` keeps the refill from costing `DecodeBinCabac` a stack frame on
/// every bin, refill or not.
#[inline(always)]
pub fn Read32BitsCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    uiValue: &mut u32,
    iNumBitsRead: &mut i32,
) -> i32 {
    {
        let pos = pDecEngine.pos;
        *iNumBitsRead = 0;
        *uiValue = 0;
        if pos >= win.len() {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ);
        }
        let tail = &win[pos..];
        let (v, n, width) = if let Some(b) = tail.first_chunk::<4>() {
            (u32::from_be_bytes(*b), 32, 4)
        } else if let Some(b) = tail.first_chunk::<3>() {
            (
                ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32),
                24,
                3,
            )
        } else if let Some(b) = tail.first_chunk::<2>() {
            (((b[0] as u32) << 8) | (b[1] as u32), 16, 2)
        } else {
            (tail[0] as u32, 8, 1)
        };
        *uiValue = v;
        pDecEngine.pos = pos + width;
        *iNumBitsRead = n;
        ERR_NONE
    }
}

pub fn DecodeBinCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    pBinCtx: &mut SWelsCabacCtx,
    uiBit: &mut u32,
) -> i32 {
    {
        let iErrorInfo: i32;
        let uiState = pBinCtx.uiState as usize;
        let mut uiBinVal = pBinCtx.uiMPS as u32;
        let mut uiOffset = pDecEngine.uiOffset;
        let mut uiRange = pDecEngine.uiRange;

        let mut iRenorm: i32 = 1;
        let range_idx = ((uiRange >> 6) & 0x03) as usize;
        let uiRangeLPS = g_kuiCabacRangeLps[uiState][range_idx] as u64;
        uiRange -= uiRangeLPS;

        if uiOffset >= (uiRange << pDecEngine.iBitsLeft) {
            // LPS
            uiOffset -= uiRange << pDecEngine.iBitsLeft;
            uiBinVal ^= 1;
            if uiState == 0 {
                pBinCtx.uiMPS ^= 1;
            }
            pBinCtx.uiState = g_kuiStateTransTable[uiState][0];
            iRenorm = g_kRenormTable256[uiRangeLPS as usize] as i32;
            uiRange = uiRangeLPS << iRenorm;
        } else {
            // MPS
            pBinCtx.uiState = g_kuiStateTransTable[uiState][1];
            if uiRange >= WELS_CABAC_QUARTER {
                pDecEngine.uiRange = uiRange;
                *uiBit = uiBinVal;
                return ERR_NONE;
            } else {
                uiRange <<= 1;
            }
        }

        // Renorm
        pDecEngine.uiRange = uiRange;
        pDecEngine.iBitsLeft -= iRenorm;
        *uiBit = uiBinVal;

        if pDecEngine.iBitsLeft > 0 {
            pDecEngine.uiOffset = uiOffset;
            return ERR_NONE;
        }

        let mut uiVal: u32 = 0;
        let mut iNumBitsRead: i32 = 0;
        iErrorInfo = Read32BitsCabac(win, pDecEngine, &mut uiVal, &mut iNumBitsRead);
        pDecEngine.uiOffset = (uiOffset << iNumBitsRead) | (uiVal as u64);
        pDecEngine.iBitsLeft += iNumBitsRead;

        if iErrorInfo != 0 && pDecEngine.iBitsLeft < 0 {
            return iErrorInfo;
        }
        ERR_NONE
    }
}

pub fn DecodeBypassCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    uiBinVal: &mut u32,
) -> i32 {
    {
        let iErrorInfo: i32;
        let mut iBitsLeft = pDecEngine.iBitsLeft;
        let mut uiOffset = pDecEngine.uiOffset;

        if iBitsLeft <= 0 {
            let mut uiVal: u32 = 0;
            let mut iNumBitsRead: i32 = 0;
            iErrorInfo = Read32BitsCabac(win, pDecEngine, &mut uiVal, &mut iNumBitsRead);
            uiOffset = (uiOffset << iNumBitsRead) | (uiVal as u64);
            iBitsLeft = iNumBitsRead;
            if iErrorInfo != 0 && iBitsLeft == 0 {
                return iErrorInfo;
            }
        }

        iBitsLeft -= 1;
        let uiRangeValue = pDecEngine.uiRange << iBitsLeft;
        if uiOffset >= uiRangeValue {
            pDecEngine.iBitsLeft = iBitsLeft;
            pDecEngine.uiOffset = uiOffset - uiRangeValue;
            *uiBinVal = 1;
            return ERR_NONE;
        }

        pDecEngine.iBitsLeft = iBitsLeft;
        pDecEngine.uiOffset = uiOffset;
        *uiBinVal = 0;
        ERR_NONE
    }
}

pub fn DecodeTerminateCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    uiBinVal: &mut u32,
) -> i32 {
    {
        let mut iErrorInfo = ERR_NONE;
        let uiRange = pDecEngine.uiRange - 2;
        let uiOffset = pDecEngine.uiOffset;

        if uiOffset >= (uiRange << pDecEngine.iBitsLeft) {
            *uiBinVal = 1;
        } else {
            *uiBinVal = 0;
            // Renorm
            if uiRange < WELS_CABAC_QUARTER {
                let iRenorm = g_kRenormTable256[uiRange as usize] as i32;
                pDecEngine.uiRange = uiRange << iRenorm;
                pDecEngine.iBitsLeft -= iRenorm;
                if pDecEngine.iBitsLeft < 0 {
                    let mut uiVal: u32 = 0;
                    let mut iNumBitsRead: i32 = 0;
                    iErrorInfo = Read32BitsCabac(win, pDecEngine, &mut uiVal, &mut iNumBitsRead);
                    pDecEngine.uiOffset = (pDecEngine.uiOffset << iNumBitsRead) | (uiVal as u64);
                    pDecEngine.iBitsLeft += iNumBitsRead;
                }
                if iErrorInfo != 0 && pDecEngine.iBitsLeft < 0 {
                    return iErrorInfo;
                }
                return ERR_NONE;
            } else {
                pDecEngine.uiRange = uiRange;
                return ERR_NONE;
            }
        }
        ERR_NONE
    }
}

// 4. Unary parsing
/// `pBinCtx` is indexed: `pBinCtx[0]` for the first bin and `pBinCtx[iCtxOffset]` for
/// every bin after it, so the caller hands over exactly `iCtxOffset + 1` contexts.
pub fn DecodeUnaryBinCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    pBinCtx: &mut [SWelsCabacCtx],
    iCtxOffset: i32,
    uiSymVal: &mut u32,
) -> i32 {
    {
        *uiSymVal = 0;
        let mut uiFirstBin: u32 = 0;
        let err = DecodeBinCabac(win, pDecEngine, &mut pBinCtx[0], &mut uiFirstBin);
        if err != 0 {
            return err;
        }
        if uiFirstBin == 0 {
            *uiSymVal = 0;
            return ERR_NONE;
        }

        let ctx_idx = iCtxOffset as usize;
        let mut sym_val: u32 = 0;
        loop {
            let mut uiCode: u32 = 0;
            let err = DecodeBinCabac(win, pDecEngine, &mut pBinCtx[ctx_idx], &mut uiCode);
            if err != 0 {
                return err;
            }
            sym_val += 1;
            if uiCode == 0 {
                break;
            }
        }
        *uiSymVal = sym_val;
        ERR_NONE
    }
}

// 5. EXGk parsing
pub fn DecodeExpBypassCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    mut iCount: i32,
    uiSymVal: &mut u32,
) -> i32 {
    {
        let mut uiCode: u32 = 0;
        let mut iSymTmp: i32 = 0;
        let mut iSymTmp2: i32 = 0;
        *uiSymVal = 0;

        loop {
            let err = DecodeBypassCabac(win, pDecEngine, &mut uiCode);
            if err != 0 {
                return err;
            }
            if uiCode == 1 {
                iSymTmp += 1 << iCount;
                iCount += 1;
            }
            if uiCode == 0 || iCount == 16 {
                break;
            }
        }

        if iCount == 16 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_UNEXPECTED_VALUE);
        }

        while iCount > 0 {
            iCount -= 1;
            let err = DecodeBypassCabac(win, pDecEngine, &mut uiCode);
            if err != 0 {
                return err;
            }
            if uiCode == 1 {
                iSymTmp2 |= 1 << iCount;
            }
        }

        *uiSymVal = (iSymTmp + iSymTmp2) as u32;
        ERR_NONE
    }
}

pub fn DecodeUEGLevelCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    pBinCtx: &mut SWelsCabacCtx,
    uiBinVal: &mut u32,
) -> u32 {
    {
        let mut uiCode: u32 = 0;
        let err = DecodeBinCabac(win, pDecEngine, pBinCtx, &mut uiCode);
        if err != 0 {
            return err as u32;
        }
        if uiCode == 0 {
            *uiBinVal = 0;
            return ERR_NONE as u32;
        }

        let mut uiTmp: u32 = 0;
        let mut uiCount: u32 = 1;
        uiCode = 0;

        loop {
            let err = DecodeBinCabac(win, pDecEngine, pBinCtx, &mut uiTmp);
            if err != 0 {
                return err as u32;
            }
            uiCode += 1;
            uiCount += 1;
            if uiTmp == 0 || uiCount == 13 {
                break;
            }
        }

        if uiTmp != 0 {
            let err = DecodeExpBypassCabac(win, pDecEngine, 0, &mut uiTmp);
            if err != 0 {
                return err as u32;
            }
            uiCode += uiTmp + 1;
        }

        *uiBinVal = uiCode;
        ERR_NONE as u32
    }
}

/// `pBinCtx` is indexed through `g_kMvdBinPos2Ctx`, whose largest entry is 3, so the
/// caller hands over four contexts.
pub fn DecodeUEGMvCabac(
    win: &[u8],
    pDecEngine: &mut SWelsCabacDecEngine,
    pBinCtx: &mut [SWelsCabacCtx],
    _iMaxC: u32,
    uiCode: &mut u32,
) -> i32 {
    {
        let mut first_code: u32 = 0;
        let err = DecodeBinCabac(
            win,
            pDecEngine,
            &mut pBinCtx[g_kMvdBinPos2Ctx[0] as usize],
            &mut first_code,
        );
        if err != 0 {
            return err;
        }
        if first_code == 0 {
            *uiCode = 0;
            return ERR_NONE;
        }

        let mut uiTmp: u32 = 0;
        let mut uiCount: usize = 1;
        let mut code: u32 = 0;

        loop {
            let ctx_offset = g_kMvdBinPos2Ctx[uiCount] as usize;
            uiCount += 1;
            let err = DecodeBinCabac(win, pDecEngine, &mut pBinCtx[ctx_offset], &mut uiTmp);
            if err != 0 {
                return err;
            }
            code += 1;
            if uiTmp == 0 || uiCount == 8 {
                break;
            }
        }

        if uiTmp != 0 {
            let err = DecodeExpBypassCabac(win, pDecEngine, 3, &mut uiTmp);
            if err != 0 {
                return err;
            }
            code += uiTmp + 1;
        }

        *uiCode = code;
        ERR_NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renorm_table_bounds() {
        assert_eq!(g_kRenormTable256.len(), 256);
        assert_eq!(g_kRenormTable256[0], 6);
        assert_eq!(g_kRenormTable256[7], 6);
        assert_eq!(g_kRenormTable256[8], 5);
        assert_eq!(g_kRenormTable256[128], 1);
        assert_eq!(g_kRenormTable256[255], 1);
    }

    #[test]
    fn test_cabac_global_init() {
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.eSliceType = crate::decoder::slice::EWelsSliceType::I_SLICE;
        ctx.bCabacInited = false;
        WelsCabacContextInit(
            &mut ctx.bCabacInited,
            &mut ctx.pCabacCtx,
            crate::decoder::slice::EWelsSliceType::I_SLICE as u8,
            0,
            26,
        );
        assert!(ctx.bCabacInited);
        // The active contexts *are* the model row the two indices select.
        assert_eq!(ctx.pCabacCtx, G_WELS_CABAC_CONTEXTS[0][26]);
    }

    // -----------------------------------------------------------------------
    // The CAVLC↔CABAC handoff: both readers live in one position space, and the
    // handoff is a `usize` in each direction.
    // -----------------------------------------------------------------------

    use crate::decoder::bit_stream::{DecInitBits, READER_SLOP};

    /// An RBSP buffer with standard decoder parsing slack headroom.
    fn rbsp_with_slack(payload: &[u8]) -> Vec<u8> {
        let mut v = payload.to_vec();
        v.extend_from_slice(&[0u8; READER_SLOP + 1]);
        v
    }

    #[test]
    fn cavlc_to_cabac_and_back_restores_the_cursor_at_a_known_offset() {
        // 16 bytes of RBSP; the CAVLC side consumes 20 bits, so the handoff
        // happens mid-byte with a partially-spent accumulator — the case where
        // `iRemainingBytes`'s rewind actually does something.
        let payload: [u8; 16] = [
            0xA5, 0x3C, 0x91, 0x08, 0xFF, 0x00, 0x7E, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE,
            0xF0, 0x11,
        ];
        let raw = RawDataBuffer::from_vec(rbsp_with_slack(&payload));
        let mut bs = BsReader::default();
        let mut engine = SWelsCabacDecEngine::default();

        {
            let err = DecInitBits(&mut bs, &raw, 0, (payload.len() * 8) as i32);
            assert_eq!(err, ERR_NONE);

            let (b, cursor) = bs.split(&raw);
            for n in [8, 8, 4] {
                cursor.get_bits(b, n).expect("20 bits of a 16-byte RBSP");
            }
            let bits_consumed = 20;
            // Where the CAVLC side stands: `pos` is the refill point, and the
            // accumulator holds the bits between the two.
            let pos_before = bs.cursor.pos();
            let left_before = bs.cursor.left_bits();

            assert_eq!(
                InitCabacDecEngineFromBS(&mut engine, &mut bs, &raw),
                ERR_NONE
            );

            // The engine started where the *bits* had got to, not where the
            // refill pointer had: `curr = pos - ((-left_bits >> 3) + 2)`, and it
            // primed five bytes from there.
            let remaining_bytes = (((-left_before) >> 3) + 2) as usize;
            assert_eq!(engine.pos, pos_before - remaining_bytes + 5);
            assert_eq!(engine.iBitsLeft, 31);
            assert_eq!(engine.uiRange, WELS_CABAC_HALF);
            // The handoff spent the cursor's accumulator, and the position is
            // untouched until the engine gives it back.
            assert_eq!(bs.cursor.left_bits(), 0);
            assert_eq!(bs.cursor.pos(), pos_before);

            // Consume some bins so the engine's position is genuinely its own,
            // then hand back.
            let win = raw.rbsp_window(&bs);
            assert_eq!(
                win.len(),
                payload.len(),
                "the window is the RBSP, not the allocation"
            );
            let mut ctx = SWelsCabacCtx {
                uiState: 20,
                uiMPS: 1,
            };
            let mut bit: u32 = 0;
            for _ in 0..64 {
                assert_eq!(
                    DecodeBinCabac(win, &mut engine, &mut ctx, &mut bit),
                    ERR_NONE
                );
            }
            let engine_pos = engine.pos;
            let engine_bits = engine.iBitsLeft;

            RestoreCabacDecEngineToBS(&mut engine, &mut bs);

            // One `usize`, both directions, and the full cursor state after it.
            assert_eq!(engine.pos, engine_pos - ((engine_bits >> 3) as usize));
            assert_eq!(bs.cursor.pos(), engine.pos);
            assert_eq!(bs.cursor.cur_bits(), 0);
            assert_eq!(bs.cursor.left_bits(), 0);
            assert_eq!(bs.cursor.cavlc_bit_pos_state(), 0);
            assert_eq!(engine.iBitsLeft, 0);
            // `len`/`bits` describe the RBSP and must survive the whole trip.
            assert_eq!(bs.cursor.len(), payload.len());
            assert_eq!(bs.cursor.bits(), (payload.len() * 8) as i32);

            // And the cursor is usable again: re-prime and read.
            let (b, cursor) = bs.split(&raw);
            assert_eq!(
                crate::decoder::bit_stream::InitReadBits(b, cursor, 1),
                ERR_NONE
            );
            assert!(cursor.get_bits(b, 8).is_ok());
            let _ = bits_consumed;
        }
    }

    #[test]
    fn the_end_ladder_stops_at_the_rbsp_and_never_reads_the_slack() {
        // The ladder is bounded by `len`, not by `avail`. Drive the engine off
        // the end of a short RBSP and assert it errors with the position at
        // `len` rather than walking into the slack bytes the allocation has.
        let payload: [u8; 8] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let buf = rbsp_with_slack(&payload);
        let mut engine = SWelsCabacDecEngine::default();
        let win = &buf[..payload.len()];

        {
            let mut value: u32 = 0;
            let mut bits: i32 = 0;
            // Walk the ladder from the last four bytes down: 4, then 3/2/1.
            engine.pos = 4;
            assert_eq!(
                Read32BitsCabac(win, &mut engine, &mut value, &mut bits),
                ERR_NONE
            );
            assert_eq!((bits, engine.pos), (32, 8));
            // At the end: no bytes left, error, nothing loaded, position frozen.
            assert_eq!(
                Read32BitsCabac(win, &mut engine, &mut value, &mut bits),
                GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ)
            );
            assert_eq!((value, bits, engine.pos), (0, 0, 8));

            for (start, want_bits, want_pos) in [(5usize, 24, 8), (6, 16, 8), (7, 8, 8)] {
                engine.pos = start;
                assert_eq!(
                    Read32BitsCabac(win, &mut engine, &mut value, &mut bits),
                    ERR_NONE
                );
                assert_eq!((bits, engine.pos), (want_bits, want_pos));
            }

            // `pos` past the end — reachable after init on a truncated stream,
            // where `iLeftBytes` is negative. The comparison form must take the
            // error arm; a `usize` subtraction would wrap and select the 4-byte
            // load.
            for start in [9usize, 12, 64] {
                engine.pos = start;
                assert_eq!(
                    Read32BitsCabac(win, &mut engine, &mut value, &mut bits),
                    GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ)
                );
                assert_eq!(engine.pos, start);
            }
        }
    }

    #[test]
    fn init_rejects_a_position_at_the_end_guard_rather_than_reading_there() {
        // The guard is `pCurr >= pEndBuf - 1`, and the rewind is what puts
        // `pCurr` behind `pos`. A cursor parked at the very end of a short RBSP
        // must be refused, not primed.
        let payload: [u8; 5] = [0x80, 0x00, 0x00, 0x00, 0x00];
        let raw = RawDataBuffer::from_vec(rbsp_with_slack(&payload));
        let mut bs = BsReader::default();
        let mut engine = SWelsCabacDecEngine::default();
        {
            assert_eq!(
                DecInitBits(&mut bs, &raw, 0, (payload.len() * 8) as i32),
                ERR_NONE
            );
            // Park the cursor one past the end so the rewind lands on the guard:
            // at pos == len with left_bits == 0, remaining_bytes == 2 and
            // curr == len - 2 is still inside.
            bs.cursor.set_pos(payload.len() + 1);
            bs.cursor.restore_from_cabac(payload.len() + 1); // left_bits = 0
            assert_eq!(
                InitCabacDecEngineFromBS(&mut engine, &mut bs, &raw),
                ERR_INFO_INVALID_ACCESS
            );
            // Nothing was written into the engine.
            assert_eq!(engine, SWelsCabacDecEngine::default());
        }
    }
}
