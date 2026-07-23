#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! Decoder bitstream reading and RBSP/EBSP serialization utilities.
//!
//! Translated from `codec/decoder/core/inc/bit_stream.h` and `codec/decoder/core/src/bit_stream.cpp`.

// Error codes matching `codec/decoder/core/inc/error_code.h`
pub const ERR_NONE: i32 = 0;
pub const ERR_INVALID_PARAMETERS: i32 = 1;
pub const ERR_MALLOC_FAILED: i32 = 2;
pub const ERR_API_FAILED: i32 = 3;

pub const ERR_INFO_COMMON_BASE: i32 = 1;
pub const ERR_INFO_OUT_OF_MEMORY: i32 = ERR_INFO_COMMON_BASE;
pub const ERR_INFO_INVALID_ACCESS: i32 = ERR_INFO_COMMON_BASE + 1; // 2
pub const ERR_INFO_INVALID_PTR: i32 = ERR_INFO_COMMON_BASE + 2;
pub const ERR_INFO_INVALID_PARAM: i32 = ERR_INFO_COMMON_BASE + 3;
pub const ERR_INFO_READ_OVERFLOW: i32 = ERR_INFO_COMMON_BASE + 10;

/// Auxiliary bitstream structure for parsing NAL units / RBSP data.
///
/// Matches `TagBitStringAux` / `SBitStringAux` from `codec/common/inc/wels_common_defs.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagBitStringAux {
    /// Pointer to the start position of the RBSP buffer
    pub pStartBuf: *mut u8,
    /// Pointer to the end boundary of the buffer (pStartBuf + buffer_byte_length)
    pub pEndBuf: *mut u8,
    /// Total count of bits in the bitstream payload
    pub iBits: i32,

    /// Auxiliary index tracker (used for CAVLC)
    pub iIndex: isize,
    /// Current byte reading/writing position cursor
    pub pCurBuf: *mut u8,
    /// 32-bit register accumulator holding unconsumed MSB-aligned bits
    pub uiCurBits: u32,
    /// Refill balance / available bit balance in the accumulator window
    pub iLeftBits: i32,
}

pub type SBitStringAux = TagBitStringAux;
pub type PBitStringAux = *mut SBitStringAux;

impl Default for TagBitStringAux {
    fn default() -> Self {
        Self {
            pStartBuf: std::ptr::null_mut(),
            pEndBuf: std::ptr::null_mut(),
            iBits: 0,
            iIndex: 0,
            pCurBuf: std::ptr::null_mut(),
            uiCurBits: 0,
            iLeftBits: 0,
        }
    }
}

impl TagBitStringAux {
    pub const fn new() -> Self {
        Self {
            pStartBuf: std::ptr::null_mut(),
            pEndBuf: std::ptr::null_mut(),
            iBits: 0,
            iIndex: 0,
            pCurBuf: std::ptr::null_mut(),
            uiCurBits: 0,
            iLeftBits: 0,
        }
    }
}

/// Reads 4 consecutive bytes from `pDstNal` and packs them into a big-endian 32-bit integer.
///
/// Matches `inline uint32_t GetValue4Bytes (uint8_t* pDstNal)` in `bit_stream.cpp`.
#[inline(always)]
pub unsafe fn GetValue4Bytes(pDstNal: *const u8) -> u32 {
    unsafe {
        let b0 = *pDstNal as u32;
        let b1 = *pDstNal.add(1) as u32;
        let b2 = *pDstNal.add(2) as u32;
        let b3 = *pDstNal.add(3) as u32;
        (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
    }
}

/// Initializes bitstream reading registers and performs buffer boundary checks.
///
/// Matches `int32_t InitReadBits (PBitStringAux pBitString, intX_t iEndOffset)` in `bit_stream.cpp`.
///
/// # Safety
/// Requires `pBitString` to point to a valid `SBitStringAux` whose `pCurBuf` and `pEndBuf` pointers
/// are set properly.
pub unsafe fn InitReadBits(pBitString: *mut SBitStringAux, iEndOffset: isize) -> i32 {
    if pBitString.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let bs = &mut *pBitString;
        if bs.pCurBuf.is_null() || bs.pEndBuf.is_null() {
            return ERR_INFO_INVALID_ACCESS;
        }
        let end_limit = bs.pEndBuf.offset(-iEndOffset);
        if bs.pCurBuf >= end_limit {
            return ERR_INFO_INVALID_ACCESS;
        }
        bs.uiCurBits = GetValue4Bytes(bs.pCurBuf);
        bs.pCurBuf = bs.pCurBuf.add(4);
        bs.iLeftBits = -16;
        ERR_NONE
    }
}

/// Initializes the bit reader structure with an input RBSP buffer.
///
/// Matches `int32_t DecInitBits (PBitStringAux pBitString, const uint8_t* kpBuf, const int32_t kiSize)`
/// in `bit_stream.cpp`.
///
/// # Safety
/// `kpBuf` must point to a readable memory buffer containing at least `(kiSize + 7) >> 3` bytes.
pub unsafe fn DecInitBits(
    pBitString: *mut SBitStringAux,
    kpBuf: *const u8,
    kiSize: i32,
) -> i32 {
    if kpBuf.is_null() || pBitString.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let kiSizeBuf = ((kiSize + 7) >> 3) as isize;
        let pTmp = kpBuf as *mut u8;

        let bs = &mut *pBitString;
        bs.pStartBuf = pTmp;
        bs.pEndBuf = pTmp.offset(kiSizeBuf);
        bs.iBits = kiSize;
        bs.pCurBuf = bs.pStartBuf;

        let iErr = InitReadBits(pBitString, 0);
        if iErr != ERR_NONE {
            return iErr;
        }
        ERR_NONE
    }
}

/// Converts Raw Byte Sequence Payload (RBSP) to Encapsulated Byte Sequence Payload (EBSP)
/// by injecting emulation prevention bytes (`0x03`) after any sequence of two consecutive `0x00`
/// bytes followed by a byte `<= 3`.
///
/// Matches `void RBSP2EBSP (uint8_t* pDstBuf, uint8_t* pSrcBuf, const int32_t kiSize)` in `bit_stream.cpp`.
///
/// # Safety
/// `pDstBuf` and `pSrcBuf` must point to valid memory buffers. `pDstBuf` must be large enough
/// to hold the resulting EBSP data.
pub unsafe fn RBSP2EBSP(pDstBuf: *mut u8, pSrcBuf: *const u8, kiSize: i32) {
    if pDstBuf.is_null() || pSrcBuf.is_null() || kiSize <= 0 {
        return;
    }
    unsafe {
        let mut pSrcPointer = pSrcBuf;
        let mut pDstPointer = pDstBuf;
        let pSrcEnd = pSrcBuf.offset(kiSize as isize);
        let mut iZeroCount: i32 = 0;

        while pSrcPointer < pSrcEnd {
            let val = *pSrcPointer;
            if iZeroCount == 2 && val <= 3 {
                // add the emulation prevention code 0x03
                *pDstPointer = 3;
                pDstPointer = pDstPointer.add(1);
                iZeroCount = 0;
            }
            if val == 0 {
                iZeroCount += 1;
            } else {
                iZeroCount = 0;
            }
            *pDstPointer = val;
            pDstPointer = pDstPointer.add(1);
            pSrcPointer = pSrcPointer.add(1);
        }
    }
}

/// Safe helper for RBSP to EBSP conversion on slices.
/// Returns the number of bytes written to `dst`.
pub fn rbsp_to_ebsp(src: &[u8], dst: &mut [u8]) -> usize {
    let mut dst_idx = 0;
    let mut zero_count = 0;

    for &val in src {
        if zero_count == 2 && val <= 3 {
            if dst_idx < dst.len() {
                dst[dst_idx] = 3;
                dst_idx += 1;
            }
            zero_count = 0;
        }
        if val == 0 {
            zero_count += 1;
        } else {
            zero_count = 0;
        }
        if dst_idx < dst.len() {
            dst[dst_idx] = val;
            dst_idx += 1;
        }
    }
    dst_idx
}

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_get_value_4_bytes() {
        let buf: [u8; 4] = [0x12, 0x34, 0x56, 0x78];
        unsafe {
            let val = GetValue4Bytes(buf.as_ptr());
            assert_eq!(val, 0x12345678);
        }
    }

    #[test]
    fn test_dec_init_bits_and_init_read_bits() {
        let buf: [u8; 8] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22];
        let mut bs = SBitStringAux::default();

        unsafe {
            let err = DecInitBits(&mut bs, buf.as_ptr(), 64);
            assert_eq!(err, ERR_NONE);
            assert_eq!(bs.uiCurBits, 0xAABBCCDD);
            assert_eq!(bs.iLeftBits, -16);
            assert_eq!(bs.iBits, 64);
            assert_eq!(bs.pCurBuf, buf.as_ptr().add(4) as *mut u8);
        }
    }

    #[test]
    fn test_dec_init_bits_null() {
        let mut bs = SBitStringAux::default();
        unsafe {
            let err = DecInitBits(&mut bs, std::ptr::null(), 32);
            assert_eq!(err, ERR_INFO_INVALID_ACCESS);
        }
    }

    #[test]
    fn test_rbsp2ebsp_emulation_prevention() {
        // [0x00, 0x00, 0x00] -> [0x00, 0x00, 0x03, 0x00]
        // [0x00, 0x00, 0x01] -> [0x00, 0x00, 0x03, 0x01]
        // [0x00, 0x00, 0x02] -> [0x00, 0x00, 0x03, 0x02]
        // [0x00, 0x00, 0x03] -> [0x00, 0x00, 0x03, 0x03]
        let src: [u8; 6] = [0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let mut dst: [u8; 10] = [0; 10];

        unsafe {
            RBSP2EBSP(dst.as_mut_ptr(), src.as_ptr(), src.len() as i32);
        }

        let expected = [0x00, 0x00, 0x03, 0x01, 0x00, 0x00, 0x03, 0x00];
        assert_eq!(&dst[0..8], &expected);
    }

    #[test]
    fn test_rbsp_to_ebsp_slice_helper() {
        let src = [0x00, 0x00, 0x02, 0xFF, 0x00, 0x00, 0x03];
        let mut dst = [0u8; 12];
        let count = rbsp_to_ebsp(&src, &mut dst);
        assert_eq!(count, 9);
        assert_eq!(&dst[0..9], &[0x00, 0x00, 0x03, 0x02, 0xFF, 0x00, 0x00, 0x03, 0x03]);
    }
}
