#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! Exponential-Golomb entropy decoding and bitstream parsing routines.
//!
//! Rust translation of:
//! - `codec/decoder/core/inc/dec_golomb.h`
//! - Associated documentation in `rust/docs/dec_golomb.h.md`

use crate::decoder::bit_stream::{PBitStringAux, SBitStringAux, TagBitStringAux};

// Error and status return codes matching OpenH264 error_code.h
pub const ERR_NONE: i32 = 0;
pub const ERR_INVALID_PARAMETERS: i32 = 1;
pub const ERR_MALLOC_FAILED: i32 = 2;
pub const ERR_API_FAILED: i32 = 3;
pub const ERR_BOUND: i32 = 31;

pub const ERR_INFO_COMMON_BASE: i32 = 1;
pub const ERR_INFO_OUT_OF_MEMORY: i32 = ERR_INFO_COMMON_BASE;
pub const ERR_INFO_INVALID_ACCESS: i32 = ERR_INFO_COMMON_BASE + 1; // 2
pub const ERR_INFO_INVALID_PTR: i32 = ERR_INFO_COMMON_BASE + 2;
pub const ERR_INFO_INVALID_PARAM: i32 = ERR_INFO_COMMON_BASE + 3;
pub const ERR_INFO_READ_OVERFLOW: i32 = ERR_INFO_COMMON_BASE + 10; // 11
pub const ERR_INFO_READ_LEADING_ZERO: i32 = ERR_INFO_COMMON_BASE + 11; // 12

// Syntax Element Offsets & Sizing Constants
pub const BIT_DEPTH_LUMA_OFFSET: i32 = 8;
pub const BIT_DEPTH_CHROMA_OFFSET: i32 = 8;
pub const LOG2_MAX_FRAME_NUM_OFFSET: i32 = 4;
pub const LOG2_MAX_PIC_ORDER_CNT_LSB_OFFSET: i32 = 4;
pub const PIC_WIDTH_IN_MBS_OFFSET: i32 = 1;
pub const PIC_HEIGHT_IN_MAP_UNITS_OFFSET: i32 = 1;
pub const BIT_DEPTH_AUX_OFFSET: i32 = 8;
pub const NUM_SLICE_GROUPS_OFFSET: i32 = 1;
pub const RUN_LENGTH_OFFSET: i32 = 1;
pub const SLICE_GROUP_CHANGE_RATE_OFFSET: i32 = 1;
pub const PIC_SIZE_IN_MAP_UNITS_OFFSET: i32 = 1;
pub const NUM_REF_IDX_L0_DEFAULT_ACTIVE_OFFSET: i32 = 1;
pub const NUM_REF_IDX_L1_DEFAULT_ACTIVE_OFFSET: i32 = 1;
pub const PIC_INIT_QP_OFFSET: i32 = 26;
pub const PIC_INIT_QS_OFFSET: i32 = 26;
pub const NUM_REF_IDX_L0_ACTIVE_OFFSET: i32 = 1;
pub const NUM_REF_IDX_L1_ACTIVE_OFFSET: i32 = 1;

pub const MAX_MB_SIZE: i32 = 36864;
pub const EXTENDED_SAR: i32 = 255;

// Lookup Tables

/// CAVLC Coded Block Pattern (CBP) lookup table for Intra 4x4 macroblocks (YUV 4:2:0).
pub const g_kuiIntra4x4CbpTable: [u8; 48] = [
    47, 31, 15, 0, 23, 27, 29, 30, 7, 11, 13, 14, 39, 43, 45, 46, // 0..15
    16, 3, 5, 10, 12, 19, 21, 26, 28, 35, 37, 42, 44, 1, 2, 4, // 16..31
    8, 17, 18, 20, 24, 6, 9, 22, 25, 32, 33, 34, 36, 40, 38, 41, // 32..47
];

/// CAVLC Coded Block Pattern (CBP) lookup table for Intra 4x4 monochrome macroblocks (YUV 4:0:0).
pub const g_kuiIntra4x4CbpTable400: [u8; 16] = [
    15, 0, 7, 11, 13, 14, 3, 5, 10, 12, 1, 2, 4, 8, 6, 9,
];

/// CAVLC Coded Block Pattern (CBP) lookup table for Inter macroblocks (YUV 4:2:0).
pub const g_kuiInterCbpTable: [u8; 48] = [
    0, 16, 1, 2, 4, 8, 32, 3, 5, 10, 12, 15, 47, 7, 11, 13, // 0..15
    14, 6, 9, 31, 35, 37, 42, 44, 33, 34, 36, 40, 39, 43, 45, 46, // 16..31
    17, 18, 20, 24, 19, 21, 26, 28, 23, 27, 29, 30, 22, 25, 38, 41, // 32..47
];

/// CAVLC Coded Block Pattern (CBP) lookup table for Inter monochrome macroblocks (YUV 4:0:0).
pub const g_kuiInterCbpTable400: [u8; 16] = [
    0, 1, 2, 4, 8, 3, 5, 10, 12, 15, 7, 11, 13, 14, 6, 9,
];

/// Fast lookup table mapping an 8-bit unsigned byte to its number of leading zero bits.
pub const g_kuiLeadingZeroTable: [u8; 256] = [
    8, 7, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, // 0x00..0x0F
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, // 0x10..0x1F
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, // 0x20..0x2F
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, // 0x30..0x3F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0x40..0x4F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0x50..0x5F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0x60..0x6F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0x70..0x7F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x80..0x8F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x90..0x9F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xA0..0xAF
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xB0..0xBF
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xC0..0xCF
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xD0..0xDF
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xE0..0xEF
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xF0..0xFF
];

/// 16-entry lookup table mapping 4-bit nibbles to bit counts.
pub const g_kuiPrefix8BitsTable: [u32; 16] = [
    0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3,
];

// Helper Functions & Macros

/// Two's complement integer negation macro `NEG_NUM(iX) = (1 + ~(iX))`.
#[inline(always)]
pub const fn NEG_NUM(iX: i32) -> i32 {
    1 + (!iX)
}

/// Peeks top `iNumBits` from MSB-aligned `iCurBits`.
#[inline(always)]
pub fn UBITS(iCurBits: u32, iNumBits: i32) -> u32 {
    if iNumBits <= 0 {
        0
    } else if iNumBits >= 32 {
        iCurBits
    } else {
        iCurBits >> (32 - iNumBits)
    }
}

/// Internal helper to dump bits and refill the 32-bit bitstream register.
#[inline(always)]
pub unsafe fn dump_bits_aux(pBs: PBitStringAux, iNumBits: i32) -> u32 {
    unsafe {
        let bs = &mut *pBs;
        bs.uiCurBits = bs.uiCurBits.wrapping_shl(iNumBits as u32);
        bs.iLeftBits += iNumBits;
        if bs.iLeftBits > 0 {
            let iAllowedBytes = (bs.pEndBuf as isize) - (bs.pStartBuf as isize);
            let iReadBytes = (bs.pCurBuf as isize) - (bs.pStartBuf as isize);
            if iReadBytes > iAllowedBytes + 1 {
                return ERR_INFO_READ_OVERFLOW as u32;
            }
            let b0 = *bs.pCurBuf as u32;
            let b1 = *bs.pCurBuf.add(1) as u32;
            let word = (b0 << 8) | b1;
            let shift = bs.iLeftBits as u32;
            if shift < 32 {
                bs.uiCurBits |= word.wrapping_shl(shift);
            }
            bs.iLeftBits -= 16;
            bs.pCurBuf = bs.pCurBuf.add(2);
        }
        ERR_NONE as u32
    }
}

/// Reads arbitrary `iNumBits` (1..32) from the bitstream.
///
/// Matches `int32_t BsGetBits (PBitStringAux pBs, int32_t iNumBits, uint32_t* pCode)` in `dec_golomb.h`.
#[inline(always)]
pub unsafe fn BsGetBits(pBs: PBitStringAux, iNumBits: i32, pCode: *mut u32) -> i32 {
    unsafe {
        let iRc = UBITS((*pBs).uiCurBits, iNumBits);
        let err = dump_bits_aux(pBs, iNumBits);
        if err != ERR_NONE as u32 {
            return err as i32;
        }
        *pCode = iRc;
        ERR_NONE
    }
}

/// Counts the bit length of the prefix for `uiValue`.
///
/// Matches `uint32_t GetPrefixBits (uint32_t uiValue)` in `dec_golomb.h`.
#[inline(always)]
pub fn GetPrefixBits(mut uiValue: u32) -> u32 {
    let mut iNumBit: u32 = 0;

    if (uiValue & 0xffff0000) != 0 {
        uiValue >>= 16;
        iNumBit += 16;
    }
    if (uiValue & 0xff00) != 0 {
        uiValue >>= 8;
        iNumBit += 8;
    }
    if (uiValue & 0xf0) != 0 {
        uiValue >>= 4;
        iNumBit += 4;
    }
    iNumBit += g_kuiPrefix8BitsTable[(uiValue & 0x0f) as usize];

    32 - iNumBit
}

/// Reads a single bit from the bitstream.
///
/// Matches `uint32_t BsGetOneBit (PBitStringAux pBs, uint32_t* pCode)` in `dec_golomb.h`.
#[inline(always)]
pub unsafe fn BsGetOneBit(pBs: PBitStringAux, pCode: *mut u32) -> u32 {
    unsafe { BsGetBits(pBs, 1, pCode) as u32 }
}

/// Fast lookup-table calculation of the number of leading zero bits in `iCurBits`.
///
/// Matches `int32_t GetLeadingZeroBits (uint32_t iCurBits)` in `dec_golomb.h`.
#[inline(always)]
pub fn GetLeadingZeroBits(iCurBits: u32) -> i32 {
    let mut uiValue = UBITS(iCurBits, 8);
    if uiValue != 0 {
        return g_kuiLeadingZeroTable[uiValue as usize] as i32;
    }

    uiValue = UBITS(iCurBits, 16);
    if uiValue != 0 {
        return (g_kuiLeadingZeroTable[uiValue as usize] as i32) + 8;
    }

    uiValue = UBITS(iCurBits, 24);
    if uiValue != 0 {
        return (g_kuiLeadingZeroTable[uiValue as usize] as i32) + 16;
    }

    uiValue = iCurBits;
    if uiValue != 0 {
        return (g_kuiLeadingZeroTable[uiValue as usize] as i32) + 24;
    }

    -1
}

/// Decodes an unsigned Exponential-Golomb (`ue(v)`) code from the bitstream.
///
/// Matches `uint32_t BsGetUe (PBitStringAux pBs, uint32_t* pCode)` in `dec_golomb.h`.
#[inline(always)]
pub unsafe fn BsGetUe(pBs: PBitStringAux, pCode: *mut u32) -> u32 {
    unsafe {
        let mut iValue: u32 = 0;
        let iLeadingZeroBits = GetLeadingZeroBits((*pBs).uiCurBits);

        if iLeadingZeroBits == -1 {
            return ERR_INFO_READ_LEADING_ZERO as u32;
        } else if iLeadingZeroBits > 16 {
            let mut err = dump_bits_aux(pBs, 16);
            if err != ERR_NONE as u32 {
                return err;
            }
            err = dump_bits_aux(pBs, iLeadingZeroBits + 1 - 16);
            if err != ERR_NONE as u32 {
                return err;
            }
        } else {
            let err = dump_bits_aux(pBs, iLeadingZeroBits + 1);
            if err != ERR_NONE as u32 {
                return err;
            }
        }

        if iLeadingZeroBits != 0 {
            iValue = UBITS((*pBs).uiCurBits, iLeadingZeroBits);
            let err = dump_bits_aux(pBs, iLeadingZeroBits);
            if err != ERR_NONE as u32 {
                return err;
            }
        }

        *pCode = (1u32.wrapping_shl(iLeadingZeroBits as u32))
            .wrapping_sub(1)
            .wrapping_add(iValue);
        ERR_NONE as u32
    }
}

/// Decodes a signed Exponential-Golomb (`se(v)`) code from the bitstream.
///
/// Matches `int32_t BsGetSe (PBitStringAux pBs, int32_t* pCode)` in `dec_golomb.h`.
#[inline(always)]
pub unsafe fn BsGetSe(pBs: PBitStringAux, pCode: *mut i32) -> i32 {
    unsafe {
        let mut uiCodeNum: u32 = 0;
        let uiRet = BsGetUe(pBs, &mut uiCodeNum);
        if uiRet != ERR_NONE as u32 {
            return uiRet as i32;
        }

        if (uiCodeNum & 0x01) != 0 {
            *pCode = ((uiCodeNum + 1) >> 1) as i32;
        } else {
            *pCode = NEG_NUM((uiCodeNum >> 1) as i32);
        }
        ERR_NONE
    }
}

/// Decodes a truncated Exponential-Golomb (`te(v)`) code constrained by range `iRange`.
///
/// Matches `int32_t BsGetTe0 (PBitStringAux pBs, int32_t iRange, uint32_t* pCode)` in `dec_golomb.h`.
#[inline(always)]
pub unsafe fn BsGetTe0(pBs: PBitStringAux, iRange: i32, pCode: *mut u32) -> i32 {
    unsafe {
        if iRange == 1 {
            *pCode = 0;
        } else if iRange == 2 {
            let uiRet = BsGetOneBit(pBs, pCode);
            if uiRet != ERR_NONE as u32 {
                return uiRet as i32;
            }
            *pCode ^= 1;
        } else {
            let uiRet = BsGetUe(pBs, pCode);
            if uiRet != ERR_NONE as u32 {
                return uiRet as i32;
            }
        }
        ERR_NONE
    }
}

/// Counts the number of trailing zero bits following the `rbsp_stop_one_bit` in `pBuf`.
///
/// Matches `int32_t BsGetTrailingBits (uint8_t* pBuf)` in `dec_golomb.h`.
#[inline(always)]
pub unsafe fn BsGetTrailingBits(pBuf: *const u8) -> i32 {
    unsafe {
        let mut uiValue = *pBuf as u32;
        let mut iRetNum: i32 = 0;

        while iRetNum < 9 {
            if (uiValue & 1) != 0 {
                return iRetNum;
            }
            uiValue >>= 1;
            iRetNum += 1;
        }

        0
    }
}

/// Checks whether additional RBSP syntax elements remain before `rbsp_trailing_bits()`.
///
/// Matches `bool CheckMoreRBSPData (PBitStringAux pBsAux)` in `dec_golomb.h`.
#[inline(always)]
pub unsafe fn CheckMoreRBSPData(pBsAux: PBitStringAux) -> bool {
    unsafe {
        let offset_bytes =
            ((*pBsAux).pCurBuf as isize) - ((*pBsAux).pStartBuf as isize) - 2;
        let bits_consumed = (offset_bytes << 3) as i32;
        let bits_remaining = (*pBsAux).iBits - bits_consumed - (*pBsAux).iLeftBits;
        bits_remaining > 1
    }
}

// Validation & error-checking macros translated from C++

#[macro_export]
macro_rules! WELS_READ_VERIFY {
    ($ui_ret:expr) => {{
        let ui_ret_tmp = $ui_ret as u32;
        if ui_ret_tmp != $crate::decoder::dec_golomb::ERR_NONE as u32 {
            return ui_ret_tmp as i32;
        }
    }};
}

#[macro_export]
macro_rules! WELS_CHECK_SE_BOTH_ERROR_NOLOG {
    ($val:expr, $lower_bound:expr, $upper_bound:expr, $syntax_name:expr, $ret_code:expr) => {
        if ($val < $lower_bound) || ($val > $upper_bound) {
            return $ret_code;
        }
    };
}

#[macro_export]
macro_rules! WELS_CHECK_SE_LOWER_ERROR_NOLOG {
    ($val:expr, $lower_bound:expr, $syntax_name:expr, $ret_code:expr) => {
        if $val < $lower_bound {
            return $ret_code;
        }
    };
}

#[macro_export]
macro_rules! WELS_CHECK_SE_UPPER_ERROR_NOLOG {
    ($val:expr, $upper_bound:expr, $syntax_name:expr, $ret_code:expr) => {
        if $val > $upper_bound {
            return $ret_code;
        }
    };
}

#[cfg(test)]
mod tests {
        use crate::decoder::bit_stream::DecInitBits;

    #[test]
    fn test_neg_num() {
        assert_eq!(NEG_NUM(0), 0);
        assert_eq!(NEG_NUM(1), -1);
        assert_eq!(NEG_NUM(-1), 1);
        assert_eq!(NEG_NUM(42), -42);
    }

    #[test]
    fn test_get_prefix_bits() {
        assert_eq!(GetPrefixBits(0x80000000), 1);
        assert_eq!(GetPrefixBits(0x00000001), 32);
        assert_eq!(GetPrefixBits(0x00000100), 24);
    }

    #[test]
    fn test_get_leading_zero_bits() {
        assert_eq!(GetLeadingZeroBits(0x80000000), 0);
        assert_eq!(GetLeadingZeroBits(0x40000000), 1);
        assert_eq!(GetLeadingZeroBits(0x00800000), 8);
        assert_eq!(GetLeadingZeroBits(0x00008000), 16);
        assert_eq!(GetLeadingZeroBits(0x00000080), 24);
        assert_eq!(GetLeadingZeroBits(0x00000001), 31);
        assert_eq!(GetLeadingZeroBits(0x00000000), -1);
    }

    #[test]
    fn test_bs_get_ue_values() {
        // Bitstream encoding:
        // '1' (0b10000000 = 0x80) -> codeNum = 0
        // '010' (0b01000000 = 0x40) -> codeNum = 1
        // '011' (0b01100000 = 0x60) -> codeNum = 2
        // '00100' (0b00100000 = 0x20) -> codeNum = 3
        let buf: [u8; 8] = [0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mut bs = SBitStringAux::default();

        unsafe {
            let err = DecInitBits(&mut bs, buf.as_ptr(), 64);
            assert_eq!(err, ERR_NONE);

            let mut code: u32 = 999;
            let ret = BsGetUe(&mut bs, &mut code);
            assert_eq!(ret, ERR_NONE as u32);
            assert_eq!(code, 0);
        }
    }

    #[test]
    fn test_bs_get_se_values() {
        // '010' = ue(1) -> se(+1)
        // '011' = ue(2) -> se(-1)
        let buf: [u8; 8] = [0b01001100, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mut bs = SBitStringAux::default();

        unsafe {
            DecInitBits(&mut bs, buf.as_ptr(), 64);

            let mut se_code1: i32 = 0;
            let ret1 = BsGetSe(&mut bs, &mut se_code1);
            assert_eq!(ret1, ERR_NONE);
            assert_eq!(se_code1, 1);

            let mut se_code2: i32 = 0;
            let ret2 = BsGetSe(&mut bs, &mut se_code2);
            assert_eq!(ret2, ERR_NONE);
            assert_eq!(se_code2, -1);
        }
    }

    #[test]
    fn test_bs_get_te0() {
        let buf: [u8; 8] = [0b10100000, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mut bs = SBitStringAux::default();

        unsafe {
            DecInitBits(&mut bs, buf.as_ptr(), 64);

            let mut code: u32 = 99;
            // iRange = 1: returns 0 directly without bit consumption
            let ret = BsGetTe0(&mut bs, 1, &mut code);
            assert_eq!(ret, ERR_NONE);
            assert_eq!(code, 0);

            // iRange = 2: reads 1 bit (which is '1') -> code = 1 ^ 1 = 0
            let ret2 = BsGetTe0(&mut bs, 2, &mut code);
            assert_eq!(ret2, ERR_NONE);
            assert_eq!(code, 0);
        }
    }

    #[test]
    fn test_trailing_bits() {
        let byte1 = 0b00001000u8;
        unsafe {
            assert_eq!(BsGetTrailingBits(&byte1), 3);
        }
    }
}
