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
/// `SBitStringAux` is a common-layer type (`codec/common/inc/wels_common_defs.h:232`)
/// shared by the encoder and the decoder, so it has a single definition in
/// [`crate::common::wels_common_defs`]. Re-exported here so the decoder's existing
/// `decoder::bit_stream::{TagBitStringAux, SBitStringAux, PBitStringAux}` paths are
/// unchanged.
pub use crate::common::wels_common_defs::{PBitStringAux, SBitStringAux, TagBitStringAux};

use crate::safe::bits::BsCursor;

/// The reader's slop, in bytes past the logical end of the RBSP
/// ([`phase1_findings.md`](../../../docs/phase1_findings.md) §F4).
///
/// `dump_bits_aux` permits the read cursor to sit **one** byte past `pEndBuf` and then
/// loads **two** bytes there, so the largest index the family can touch is `len + 2`.
/// The 4-byte initial prime is bounded by the same number, because `InitReadBits`
/// refuses to start at or past `pEndBuf - iEndOffset`, i.e. at `len - 1` at the
/// latest. Three bytes of readable slack past the RBSP therefore covers every read
/// the family can make, at any position, for any operation.
pub const READER_SLOP: usize = 3;

/// Reconstructs the readable region behind an `SBitStringAux` as a slice.
///
/// SHIM(phase3) — the strangler boundary for the decoder read side. `SBitStringAux`
/// records where the bytes are but not how many are readable, so the length comes from
/// [`READER_SLOP`]: exactly the footprint the raw pointer family had, no more. The
/// bytes are the *same bytes the raw reader read* — nothing is zeroed, nothing is
/// synthesised — so this is byte-identical by construction rather than by argument.
///
/// This is the plan's **P6 resolution** for T3.1: neither of §2.2.2's two options
/// (zeroed guard bytes; `get()` fallbacks) but the third one adoption made obvious —
/// hand the cursor the slack the allocation already has. `WelsDecodeBs`
/// (`decoder_core.rs:3637`) will not write a NAL payload unless `pEnd - pCurPos >=
/// len + 4`, so every payload in `sRawData` has four readable bytes behind it, and
/// `BsCursor` returns `ERR_INFO_READ_OVERFLOW` rather than reading past the slice if a
/// caller ever breaks that. Zeroing the slack would be a *behaviour change* on
/// malformed input (the slop feeds decoded values, not just the error predicate) and is
/// deliberately not made here; T3.3 owns the owned-buffer form of the same question.
///
/// # Safety
/// `start` must be non-null and readable for `len + READER_SLOP` bytes — the contract
/// the raw reader family has always had (F4), now written down.
#[inline(always)]
unsafe fn readable<'a>(start: *const u8, len: usize) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(start, len + READER_SLOP) }
}

/// Reads the cursor state an `SBitStringAux` holds.
///
/// SHIM(phase3), the read half of the pair with [`store_cursor`]. Returns `None` for
/// the null-pointer states the raw functions rejected.
#[inline(always)]
pub(crate) unsafe fn cursor_of(bs: &SBitStringAux) -> Option<BsCursor> {
    if bs.pStartBuf.is_null() || bs.pCurBuf.is_null() || bs.pEndBuf.is_null() {
        return None;
    }
    unsafe {
        let len = bs.pEndBuf.offset_from(bs.pStartBuf) as usize;
        let pos = bs.pCurBuf.offset_from(bs.pStartBuf) as usize;
        Some(BsCursor::from_parts(
            pos,
            bs.uiCurBits,
            bs.iLeftBits,
            len,
            bs.iBits,
        ))
    }
}

/// [`cursor_of`] plus the bytes behind the struct, for the operations that read.
#[inline(always)]
pub(crate) unsafe fn cursor_and_buf<'a>(bs: &SBitStringAux) -> Option<(BsCursor, &'a [u8])> {
    unsafe {
        let cursor = cursor_of(bs)?;
        Some((cursor, readable(bs.pStartBuf, cursor.len())))
    }
}

/// Writes a cursor's position and accumulator back into the struct the callers still
/// hold.
///
/// SHIM(phase3). Called on **every** return path, including the error ones: the raw
/// refill mutated `uiCurBits`/`iLeftBits` before its overflow check and left them
/// mutated on failure, and so must this.
#[inline(always)]
pub(crate) unsafe fn store_cursor(bs: &mut SBitStringAux, cursor: &BsCursor) {
    unsafe {
        bs.pCurBuf = bs.pStartBuf.add(cursor.pos());
        bs.uiCurBits = cursor.cur_bits();
        bs.iLeftBits = cursor.left_bits();
    }
}

/// Initializes bitstream reading registers and performs buffer boundary checks.
///
/// Matches `int32_t InitReadBits (PBitStringAux pBitString, intX_t iEndOffset)` in
/// `bit_stream.cpp`; the body is [`BsCursor::init_read_bits`], which does the same
/// comparison in offset arithmetic. That deletes the `pEndBuf.offset(-iEndOffset)`
/// computation of `phase1_findings.md` §F7 — a pointer before the allocation, UB by
/// the arithmetic alone — rather than preserving it.
///
/// # Safety
/// Requires `pBitString` to point to a valid `SBitStringAux` whose three pointers are
/// set, with `pStartBuf` readable for `(pEndBuf - pStartBuf) + READER_SLOP` bytes.
pub unsafe fn InitReadBits(pBitString: *mut SBitStringAux, iEndOffset: isize) -> i32 {
    if pBitString.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let bs = &mut *pBitString;
        // The raw version checked `pCurBuf`/`pEndBuf`; `pStartBuf` joins them because
        // the slice needs a base. Every caller sets all three together.
        let Some((mut cursor, buf)) = cursor_and_buf(bs) else {
            return ERR_INFO_INVALID_ACCESS;
        };
        match cursor.init_read_bits(buf, iEndOffset) {
            Ok(()) => {
                store_cursor(bs, &cursor);
                ERR_NONE
            }
            Err(err) => err.0,
        }
    }
}

/// Initializes the bit reader structure with an input RBSP buffer.
///
/// Matches `int32_t DecInitBits (PBitStringAux pBitString, const uint8_t* kpBuf, const int32_t kiSize)`
/// in `bit_stream.cpp`. The body is [`BsCursor::init`]; the pointer form's
/// `pTmp.offset(kiSizeBuf)` (F7's other site, UB for `kiSize < -7`) is gone, and the
/// end pointer is now derived from the cursor's own `len`.
///
/// # Safety
/// `kpBuf` must point to a readable buffer of at least `((kiSize + 7) >> 3) +
/// READER_SLOP` bytes. The slop is not new — the raw reader always read it (F4) — but
/// it is now part of the written contract.
pub unsafe fn DecInitBits(
    pBitString: *mut SBitStringAux,
    kpBuf: *const u8,
    kiSize: i32,
) -> i32 {
    if kpBuf.is_null() || pBitString.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        // `BsCursor::init` rejects a non-positive payload before touching `buf`, and
        // the slice must not be built for one: `(kiSize + 7) >> 3` is where F7's
        // negative length came from.
        let end = (kiSize + 7) >> 3;
        if end <= 0 {
            return ERR_INFO_INVALID_ACCESS;
        }
        let len = end as usize;
        let cursor = match BsCursor::init(readable(kpBuf, len), kiSize) {
            Ok(cursor) => cursor,
            Err(err) => return err.0,
        };

        let pTmp = kpBuf as *mut u8;
        let bs = &mut *pBitString;
        bs.pStartBuf = pTmp;
        bs.pEndBuf = pTmp.add(len);
        bs.iBits = kiSize;
        store_cursor(bs, &cursor);
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
    use super::*;
    
    /// `DecInitBits` reads `READER_SLOP` bytes past the RBSP it is handed — it always
    /// did (F4), the decoder's callers always had the slack (`decoder_core.rs:3637`
    /// sizes every payload with four bytes to spare), and since T3.1a the contract is
    /// written on the function. These tests supply it rather than relying on the raw
    /// reader not having reached that far.
    fn with_slop(payload: &[u8]) -> Vec<u8> {
        let mut v = payload.to_vec();
        v.extend_from_slice(&[0u8; READER_SLOP]);
        v
    }

    #[test]
    fn test_dec_init_bits_and_init_read_bits() {
        let buf = with_slop(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22]);
        let mut bs = SBitStringAux::default();

        unsafe {
            let err = DecInitBits(&mut bs, buf.as_ptr(), 64);
            assert_eq!(err, ERR_NONE);
            assert_eq!(bs.uiCurBits, 0xAABBCCDD);
            assert_eq!(bs.iLeftBits, -16);
            assert_eq!(bs.iBits, 64);
            assert_eq!(bs.pCurBuf, buf.as_ptr().add(4) as *mut u8);
            assert_eq!(bs.pEndBuf, buf.as_ptr().add(8) as *mut u8);
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
