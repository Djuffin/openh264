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
///
/// **The decoder no longer uses it.** T3.1b moved the read side onto [`BsReader`] +
/// [`BsCursor`], so what is left are the encoder's writers (`vlc_encoder.rs` and its
/// three near-copies) and the Phase-5/6 structs that embed it. It dies with those:
/// **T3.4** takes the writer family to `BsWriter`, **T3.6** takes `nal_encap.rs`, and
/// the last embedded fields (`SWelsSliceBs`, `SSlice::pSliceBsa`) go in Phase 6.
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

/// **`READER_SLOP` is not sufficient for the CAVLC pair** — found by T3.0's goldens at
/// T3.1b, recorded as `phase3_findings.md` §F16.
///
/// The derivation above covers `dump_bits_aux` only. `BsEndCavlc` primes **four** bytes
/// at `iIndex >> 3`, where `iIndex` is advanced by the residual decoder by however many
/// bits it consumed — and on a truncated stream that runs past the RBSP by an amount
/// bounded by nothing but how many symbols the parser accepts before erroring. The raw
/// reader was observed reaching `len + 5` and beyond on six conformance streams'
/// truncations; it was "safe" only because `sRawData` is a 4 MiB allocation and those
/// bytes were inside it.
///
/// So the readable extent cannot be a constant offset from the RBSP length: it is a
/// property of the *allocation*, and [`BsReader`] carries it as [`BsReader::avail`].

/// A [`BsCursor`] plus the base pointer of the bytes it reads.
///
/// SHIM(phase3) — **the** strangler boundary for the decoder read side, and the only
/// place in the crate that reconstructs a reader's slice from a pointer. T3.3 deletes
/// it: when `SDataBuffer` becomes an owned `Vec` and NAL payloads become
/// `Range<usize>`, the owner passes `&[u8]` directly and what is left here is a bare
/// `BsCursor`.
///
/// # Why the base pointer is bundled rather than carried alongside
///
/// Plan §2.1.3 makes `BsCursor` *detached* — offsets only, no buffer reference — and
/// the consumers below take `(buf: &[u8], cursor: &mut BsCursor)` accordingly. But
/// until T3.3 the bytes still live behind `sRawData`'s pointers, so *something* must
/// remember where a given reader's bytes start. The alternative to bundling is a
/// second base-pointer field beside every cursor field, kept coherent by hand — which
/// is exactly the shape plan §2.2.2 **[P3]** rejected for `iIndex`, and for the same
/// reason. Bundling keeps one owner, one lifetime, and one thing for T3.3 to delete.
///
/// This is a **deviation from the brief's default shape** for
/// `SDqLayer::pBitStringAux` (`*mut BsCursor`): the field is `*mut BsReader`, because
/// `decode_slice.rs` reads *through* that pointer and a bare cursor cannot produce the
/// buffer. Recorded in the log and the plan.
#[derive(Clone, Copy, Debug)]
pub struct BsReader {
    /// Base of the readable region — the C++ `pStartBuf`. Dies at T3.3.
    pub base: *mut u8,
    /// Bytes readable from [`base`](Self::base) — the distance to the end of the
    /// allocation the NAL sits in, **not** `len + READER_SLOP`. See the note on
    /// [`READER_SLOP`] for why the difference is load-bearing (F16). Dies at T3.3 with
    /// the pointer, when the owned buffer makes the extent a slice length.
    pub avail: usize,
    /// The position and accumulator. All the arithmetic lives here.
    pub cursor: BsCursor,
}

impl Default for BsReader {
    fn default() -> Self {
        Self {
            base: std::ptr::null_mut(),
            avail: 0,
            cursor: BsCursor::default(),
        }
    }
}

impl BsReader {
    /// The readable region behind this reader.
    ///
    /// SHIM(phase3) — the single site that does this arithmetic. `SBitStringAux`
    /// recorded where the bytes are but not how many are readable, so the length comes
    /// from [`READER_SLOP`]: exactly the footprint the raw pointer family had, no more.
    /// The bytes are the *same bytes the raw reader read* — nothing is zeroed, nothing
    /// is synthesised — so this is byte-identical by construction rather than by
    /// argument.
    ///
    /// This is the plan's **P6 resolution** for T3.1: neither of §2.2.2's two options
    /// (zeroed guard bytes; `get()` fallbacks) but the third one adoption made obvious
    /// — hand the cursor the slack the allocation already has. `WelsDecodeBs`
    /// (`decoder_core.rs:3637`) will not write a NAL payload unless `pEnd - pCurPos >=
    /// len + 4`, so every payload in `sRawData` has four readable bytes behind it, and
    /// `BsCursor` returns [`ERR_INFO_READ_OVERFLOW`] rather than reading past the slice
    /// if a caller ever breaks that. Zeroing the slack would be a *behaviour change* on
    /// malformed input (the slop feeds decoded values, not just the error predicate)
    /// and is deliberately not made here; T3.3 owns the owned-buffer form of the same
    /// question.
    ///
    /// # Safety
    /// `base` must be non-null and readable for [`avail`](Self::avail) bytes — the
    /// contract the raw reader family has always had (F4/F16), now written down. An
    /// uninitialised reader (null base) yields an empty slice, which every operation
    /// then rejects with [`ERR_INFO_READ_OVERFLOW`] instead of dereferencing it.
    #[inline(always)]
    pub unsafe fn buf<'a>(&self) -> &'a [u8] {
        if self.base.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.base, self.avail) }
    }

    /// Splits into the two halves the consumers take: the bytes, and the position.
    ///
    /// The slice's lifetime is unbounded and therefore does *not* borrow `self`, which
    /// is what lets a caller hold both halves at once. That is the shim's whole job and
    /// the reason it is `unsafe`; it is sound for exactly as long as `buf`'s contract
    /// holds.
    #[inline(always)]
    pub unsafe fn split<'a>(&'a mut self) -> (&'a [u8], &'a mut BsCursor) {
        let buf = unsafe { self.buf() };
        (buf, &mut self.cursor)
    }
}

/// Initializes bitstream reading registers and performs buffer boundary checks.
///
/// Matches `int32_t InitReadBits (PBitStringAux pBitString, intX_t iEndOffset)` in
/// `bit_stream.cpp`; the body is [`BsCursor::init_read_bits`], which does the same
/// comparison in offset arithmetic. That deletes the `pEndBuf.offset(-iEndOffset)`
/// computation of `phase1_findings.md` §F7 — a pointer before the allocation, UB by
/// the arithmetic alone — rather than preserving it.
pub fn InitReadBits(buf: &[u8], cursor: &mut BsCursor, iEndOffset: isize) -> i32 {
    match cursor.init_read_bits(buf, iEndOffset) {
        Ok(()) => ERR_NONE,
        Err(err) => err.0,
    }
}

/// Initializes the bit reader structure with an input RBSP buffer.
///
/// Matches `int32_t DecInitBits (PBitStringAux pBitString, const uint8_t* kpBuf, const int32_t kiSize)`
/// in `bit_stream.cpp`. The body is [`BsCursor::init`]; the pointer form's
/// `pTmp.offset(kiSizeBuf)` (F7's other site, UB for `kiSize < -7`) is gone, and the
/// logical end is now the cursor's own `len`.
///
/// # Safety
/// `kpBuf` must point to a readable buffer of at least `((kiSize + 7) >> 3) +
/// READER_SLOP` bytes. The slop is not new — the raw reader always read it (F4) — but
/// it is now part of the written contract.
pub unsafe fn DecInitBits(
    pReader: &mut BsReader,
    kpBuf: *const u8,
    kiSize: i32,
    kiAvail: usize,
) -> i32 {
    if kpBuf.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    // `BsCursor::init` rejects a non-positive payload before touching `buf`, and the
    // slice must not be built for one: `(kiSize + 7) >> 3` is where F7's negative
    // length came from.
    let end = (kiSize + 7) >> 3;
    if end <= 0 {
        return ERR_INFO_INVALID_ACCESS;
    }
    let len = end as usize;
    // The readable extent is the caller's, not `len + READER_SLOP`: see F16.
    let buf = unsafe { std::slice::from_raw_parts(kpBuf, kiAvail) };
    match BsCursor::init(buf, kiSize) {
        Ok(cursor) => {
            pReader.base = kpBuf as *mut u8;
            pReader.avail = kiAvail;
            pReader.cursor = cursor;
            ERR_NONE
        }
        Err(err) => err.0,
    }
}

/// Bytes readable from `p`, given the decoder's raw-data buffer.
///
/// SHIM(phase3) — the single site that turns `sRawData`'s pointer pair into a length,
/// and the one the brief asks for (`pHead..pEnd`). Every reader in the decoder gets its
/// extent from here, so T3.3 has exactly one thing to delete when `SDataBuffer` becomes
/// an owned `Vec` and this becomes `buf.len() - offset`.
///
/// `p` points into `[pHead, pEnd)` for every NAL the decoder parses, because
/// `WelsDecodeBs` copies each payload into that buffer before anything reads it. If it
/// somehow does not, the fallback is the old constant-derived window — which is what the
/// reader had before F16 and is still correct for everything except the CAVLC prime.
///
/// # Safety
/// `pHead`/`pEnd` must bound a single allocation, and `p` must be derived from it.
#[inline(always)]
pub unsafe fn readable_from(pHead: *const u8, pEnd: *const u8, p: *const u8, len: usize) -> usize {
    if pHead.is_null() || pEnd.is_null() || p < pHead || p >= pEnd {
        return len + READER_SLOP;
    }
    unsafe { pEnd.offset_from(p) as usize }
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
        let mut bs = BsReader::default();

        unsafe {
            let err = DecInitBits(&mut bs, buf.as_ptr(), 64, buf.len());
            assert_eq!(err, ERR_NONE);
            assert_eq!(bs.cursor.cur_bits(), 0xAABBCCDD);
            assert_eq!(bs.cursor.left_bits(), -16);
            assert_eq!(bs.cursor.bits(), 64);
            // `pCurBuf`/`pEndBuf` are `pos`/`len` now — the same two numbers, as offsets.
            assert_eq!(bs.cursor.pos(), 4);
            assert_eq!(bs.cursor.len(), 8);
            assert_eq!(bs.base, buf.as_ptr() as *mut u8);
            // And the boundary helper hands back exactly the declared footprint.
            assert_eq!(bs.buf().len(), 8 + READER_SLOP);
        }
    }

    #[test]
    fn test_dec_init_bits_null() {
        let mut bs = BsReader::default();
        unsafe {
            let err = DecInitBits(&mut bs, std::ptr::null(), 32, 0);
            assert_eq!(err, ERR_INFO_INVALID_ACCESS);
        }
    }

    #[test]
    fn an_uninitialised_reader_yields_an_empty_slice() {
        // The null-base case the raw functions rejected with ERR_INFO_INVALID_ACCESS:
        // the helper returns an empty slice rather than building one from a null
        // pointer, and every read then fails on the bounds instead of dereferencing.
        let bs = BsReader::default();
        assert!(unsafe { bs.buf() }.is_empty());
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
