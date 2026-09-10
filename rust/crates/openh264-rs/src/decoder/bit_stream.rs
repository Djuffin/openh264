#![deny(unsafe_code)]
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

//! Decoder bitstream reading and RBSP/EBSP serialization utilities.
//!
//! `codec/decoder/core/inc/bit_stream.h`, `codec/decoder/core/src/bit_stream.cpp`.

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

use crate::safe::bits::BsCursor;

/// The reader's slop, in bytes past the logical end of the RBSP.
///
/// `dump_bits_aux` permits the read cursor to sit one byte past `pEndBuf` and then loads
/// two bytes there, so the largest index reachable is `len + 2`. The 4-byte initial prime
/// is bounded by the same number: `InitReadBits` refuses to start at or past
/// `pEndBuf - iEndOffset`, i.e. at `len - 1` at the latest.
pub const READER_SLOP: usize = 3;

/// The decoder's raw-bitstream accumulation buffer — `SDataBuffer { pHead, pEnd,
/// pStartPos, pCurPos }`.
///
/// Owns the EPB-stripped NAL payloads and the write position. The readable extent behind
/// an offset is `buf.len() - start`, derived at call time by
/// [`window_from`](Self::window_from), so nothing stores a length the buffer can outgrow.
///
/// The backing store is kept at its full allocated size and zero-filled, so the reader's
/// [`READER_SLOP`] past the last NAL stays in bounds. `buf.len()` is initialized bytes,
/// never spare `Vec` capacity.
#[derive(Debug, Default)]
pub struct RawDataBuffer {
    buf: Vec<u8>,
    /// The write position — `pCurPos - pHead`.
    cur: usize,
}

impl RawDataBuffer {
    /// A zero-filled buffer of `len` bytes — `WelsMalloczHelper`'s allocation, owned.
    /// Returns `Err` on allocation failure rather than aborting.
    pub fn try_new_zeroed(len: usize) -> Result<Self, ()> {
        let mut buf = Vec::new();
        buf.try_reserve_exact(len).map_err(|_| ())?;
        buf.resize(len, 0);
        Ok(Self { buf, cur: 0 })
    }

    /// Wraps existing bytes (tests; the write position starts at the end so the
    /// content reads as already-appended payload).
    pub fn from_vec(buf: Vec<u8>) -> Self {
        let cur = buf.len();
        Self { buf, cur }
    }

    /// Allocation size in bytes — `pEnd - pHead` / `iMaxBsBufferSizeInByte`.
    #[inline]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The write position — `pCurPos - pHead`.
    #[inline]
    pub fn cur(&self) -> usize {
        self.cur
    }

    /// Bytes writable at [`cur`](Self::cur) — `pEnd - pCurPos`. `cur <= len` is
    /// structural; a broken invariant saturates to 0 rather than wrapping.
    #[inline]
    pub fn remaining(&self) -> usize {
        debug_assert!(self.cur <= self.buf.len());
        self.buf.len().saturating_sub(self.cur)
    }

    /// Rewinds the write position to the head — `pCurPos = pHead`.
    #[inline]
    pub fn rewind(&mut self) {
        self.cur = 0;
    }

    /// Grows the buffer, keeping its contents and zero-filling the new tail.
    ///
    /// `ExpandBsBuffer`'s growth policy: the new size is
    /// `max(src_len * MAX_BUFFERED_NUM, len << 1)`. Returns `Err` on allocation
    /// failure.
    pub fn grow(&mut self, src_len: usize) -> Result<(), ()> {
        let new_len = std::cmp::max(
            src_len * crate::decoder::decoder_core::MAX_BUFFERED_NUM,
            self.buf.len() << 1,
        );
        self.grow_to(new_len)
    }

    /// Grows to exactly `new_len` (keeps `sSavedData` the same size as `sRawData`).
    /// A `new_len` at or below the current size is a no-op: growth only.
    pub fn grow_to(&mut self, new_len: usize) -> Result<(), ()> {
        if new_len <= self.buf.len() {
            return Ok(());
        }
        self.buf
            .try_reserve_exact(new_len - self.buf.len())
            .map_err(|_| ())?;
        self.buf.resize(new_len, 0);
        Ok(())
    }

    /// Appends one NAL payload at the write position, stripping emulation-prevention
    /// bytes (`00 00 03` → `00 00`) as `WelsDecodeBs`'s copy loop does. Returns
    /// `(start, len)` of the stripped payload within the buffer.
    ///
    /// The caller must ensure `remaining() >= payload.len() + 4`; the destination
    /// slice is taken once up front, so a violated contract panics there rather than
    /// byte-by-byte.
    pub fn append_ebsp_stripped(&mut self, payload: &[u8]) -> (usize, usize) {
        let start = self.cur;
        let dst = &mut self.buf[start..start + payload.len()];
        let mut dst_len = 0usize;
        let mut zero_run = 0u32;
        for &b in payload {
            if zero_run >= 2 && b == 0x03 {
                zero_run = 0;
                continue;
            }
            if b == 0 {
                zero_run += 1;
            } else {
                zero_run = 0;
            }
            dst[dst_len] = b;
            dst_len += 1;
        }
        self.cur = start + dst_len;
        (start, dst_len)
    }

    /// Zeroes the four reserved bytes at `at` — `pDstNal[iDstIdx .. iDstIdx+4] = 0`,
    /// which `WelsDecodeBs` writes before every `ParseNalHeader` call
    /// (`decoder.cpp:874`/`:875`). They are the guard bytes a refill is allowed to load
    /// past an RBSP end, and the bytes a zero-length NAL's header is read out of.
    ///
    /// The caller must ensure `remaining() >= len + 4` before appending, so
    /// `at + 4 <= len()` holds; the clamp keeps a violated contract from panicking.
    #[inline]
    pub fn zero_reserved(&mut self, at: usize) {
        let end = (at + 4).min(self.buf.len());
        if at < end {
            self.buf[at..end].fill(0);
        }
    }

    /// Parse-only's raw append — `sSavedData`'s half of `WelsDecodeBs`'s two-buffer
    /// copy, and the one thing [`append_ebsp_stripped`](Self::append_ebsp_stripped)
    /// must not do.
    ///
    /// `sRawData` holds the RBSP, with emulation-prevention bytes stripped, which is
    /// what the syntax readers want. Parse-only hands its caller a bitstream to feed to
    /// another decoder, so it must hand back the EBSP — the escaped bytes as they
    /// arrived — which is what `pSavedData` holds (`au_parser.cpp:340`/`:375`).
    ///
    /// Returns the start offset of the appended bytes — `pNalPos` as an offset.
    pub fn append_raw(&mut self, bytes: &[u8]) -> usize {
        let start = self.cur;
        self.buf[start..start + bytes.len()].copy_from_slice(bytes);
        self.cur = start + bytes.len();
        start
    }

    /// `if (pSavedData->pCurPos + iWriteLen > pSavedData->pEnd) pSavedData->pCurPos =
    /// pSavedData->pHead;` — `au_parser.cpp:337`/`:372`, the wrap that keeps a write
    /// inside the allocation by starting over at the head.
    ///
    /// Answers whether `need` bytes now fit: `false` means the buffer is smaller than
    /// one NAL and the caller must refuse rather than write.
    pub fn wrap_for(&mut self, need: usize) -> bool {
        if self.cur + need > self.buf.len() {
            self.cur = 0;
        }
        need <= self.buf.len()
    }

    /// The whole backing store.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// The readable window behind offset `start`: everything from `start` to the end of
    /// the allocation, derived from the owner at call time. `start <= len` holds for
    /// every offset the decoder mints, and growth never shrinks the buffer; the clamp
    /// routes a broken invariant to an empty window — every read then fails with
    /// `ERR_INFO_READ_OVERFLOW` — rather than panicking.
    #[inline(always)]
    pub fn window_from(&self, start: usize) -> &[u8] {
        debug_assert!(
            start <= self.buf.len(),
            "window start {} past allocation end {}",
            start,
            self.buf.len()
        );
        &self.buf[start.min(self.buf.len())..]
    }

    /// The RBSP window for a reader: the first [`BsCursor::len`] bytes of its readable
    /// window. This is what the CABAC engine reads through — `len` is the logical end
    /// of the RBSP (the C++ `pBuffEnd` its end ladder measures against), so `win.len()`
    /// is the ladder's selector and the engine computes no extent of its own.
    ///
    /// `window.len() >= cursor.len()` holds structurally — `WelsDecodeBs` refuses to
    /// append a payload without four bytes to spare, EPB stripping only shrinks it, and
    /// growth only widens the window — so the `min` is dead; the clamp keeps a violated
    /// contract on the error path instead of panicking.
    #[inline(always)]
    pub fn rbsp_window(&self, reader: &BsReader) -> &[u8] {
        let win = self.window_from(reader.start);
        debug_assert!(
            win.len() >= reader.cursor.len() || self.buf.is_empty(),
            "readable window {} narrower than the RBSP {}",
            win.len(),
            reader.cursor.len()
        );
        &win[..reader.cursor.len().min(win.len())]
    }

    /// Releases the allocation — the `WelsFreeHelper` calls in the uninit path.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A NAL's read state: where its bytes start in the owning [`RawDataBuffer`], plus
/// the detached [`BsCursor`].
///
/// `start` is an offset into the owner, so it survives the owner's growth; every window
/// is derived from the owner at call time. Consumers take
/// `(buf: &[u8], cursor: &mut BsCursor)`, produced by [`split`](Self::split).
#[derive(Clone, Copy, Debug, Default)]
pub struct BsReader {
    /// Offset of this NAL's payload in the owning [`RawDataBuffer`].
    pub start: usize,
    /// The position and accumulator.
    pub cursor: BsCursor,
}

impl BsReader {
    /// Splits into the two halves the consumers take: the bytes, and the position. The
    /// window is derived from `raw`, not from stored state.
    #[inline(always)]
    pub fn split<'a>(&'a mut self, raw: &'a RawDataBuffer) -> (&'a [u8], &'a mut BsCursor) {
        (raw.window_from(self.start), &mut self.cursor)
    }
}

/// Initializes bitstream reading registers and performs buffer boundary checks.
///
/// Matches `int32_t InitReadBits (PBitStringAux pBitString, intX_t iEndOffset)` in
/// `bit_stream.cpp`; the body is [`BsCursor::init_read_bits`].
pub fn InitReadBits(buf: &[u8], cursor: &mut BsCursor, iEndOffset: isize) -> i32 {
    match cursor.init_read_bits(buf, iEndOffset) {
        Ok(()) => ERR_NONE,
        Err(err) => err.0,
    }
}

/// Initializes the bit reader structure with an input RBSP buffer.
///
/// Matches `int32_t DecInitBits (PBitStringAux pBitString, const uint8_t* kpBuf, const int32_t kiSize)`
/// in `bit_stream.cpp`. The body is [`BsCursor::init`] over the window derived from the
/// owning buffer at `start` ([`RawDataBuffer::window_from`]). The
/// `(kiSize + 7) >> 3 <= 0` case returns [`ERR_INFO_INVALID_ACCESS`] from `init`.
pub fn DecInitBits(pReader: &mut BsReader, raw: &RawDataBuffer, start: usize, kiSize: i32) -> i32 {
    match BsCursor::init(raw.window_from(start), kiSize) {
        Ok(cursor) => {
            pReader.start = start;
            pReader.cursor = cursor;
            ERR_NONE
        }
        Err(err) => err.0,
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

    /// The reader reads [`READER_SLOP`] bytes past the RBSP it is handed; these tests
    /// supply the slack the decoder's raw buffer always has.
    fn with_slop(payload: &[u8]) -> Vec<u8> {
        let mut v = payload.to_vec();
        v.extend_from_slice(&[0u8; READER_SLOP]);
        v
    }

    #[test]
    fn test_dec_init_bits_and_init_read_bits() {
        let raw =
            RawDataBuffer::from_vec(with_slop(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22]));
        let mut bs = BsReader::default();

        let err = DecInitBits(&mut bs, &raw, 0, 64);
        assert_eq!(err, ERR_NONE);
        assert_eq!(bs.cursor.cur_bits(), 0xAABBCCDD);
        assert_eq!(bs.cursor.left_bits(), -16);
        assert_eq!(bs.cursor.bits(), 64);
        assert_eq!(bs.cursor.pos(), 4);
        assert_eq!(bs.cursor.len(), 8);
        assert_eq!(bs.start, 0);
        // The derived window is exactly the declared footprint plus the slop.
        assert_eq!(raw.window_from(bs.start).len(), 8 + READER_SLOP);
    }

    #[test]
    fn test_dec_init_bits_empty_window() {
        // A window with no bytes fails `init`'s 4-byte prime with READ_OVERFLOW rather
        // than reading anything; a non-positive size gives INVALID_ACCESS.
        let raw = RawDataBuffer::default();
        let mut bs = BsReader::default();
        assert_eq!(DecInitBits(&mut bs, &raw, 0, 32), ERR_INFO_READ_OVERFLOW);
        assert_eq!(DecInitBits(&mut bs, &raw, 0, 0), ERR_INFO_INVALID_ACCESS);
    }

    #[test]
    fn append_strips_emulation_prevention_and_advances() {
        let mut raw = RawDataBuffer::try_new_zeroed(64).unwrap();
        let (s1, l1) = raw.append_ebsp_stripped(&[0x00, 0x00, 0x03, 0x01, 0xAB]);
        assert_eq!((s1, l1), (0, 4));
        assert_eq!(&raw.bytes()[0..4], &[0x00, 0x00, 0x01, 0xAB]);
        let (s2, l2) = raw.append_ebsp_stripped(&[0xFF]);
        assert_eq!((s2, l2), (4, 1));
        assert_eq!(raw.cur(), 5);
        assert_eq!(raw.remaining(), 59);
        raw.rewind();
        assert_eq!(raw.cur(), 0);
    }

    /// A reader mid-read over an early NAL keeps decoding the same values when a later
    /// NAL forces the buffer to grow, because it stores an offset and derives its window
    /// from the owner at call time.
    #[test]
    fn p5_reader_survives_growth_mid_au() {
        // A payload of ascending bytes: ue(v) reads below give known values.
        let payload: Vec<u8> = (1..=32).collect();

        // Control: read the whole thing with no growth.
        let mut control_vals = Vec::new();
        {
            let raw = RawDataBuffer::from_vec(payload.clone());
            let mut rd = BsReader::default();
            assert_eq!(
                DecInitBits(&mut rd, &raw, 0, (payload.len() * 8) as i32),
                ERR_NONE
            );
            for _ in 0..12 {
                let (buf, cursor) = rd.split(&raw);
                control_vals.push(cursor.get_ue(buf).unwrap());
            }
        }

        // Growth run: same payload at the same offset, grown twice after the reader has
        // consumed part of the stream — the second time by more than 8x, so `grow`'s
        // `max` takes each arm once.
        let mut raw = RawDataBuffer::try_new_zeroed(48).unwrap();
        let (start, len) = raw.append_ebsp_stripped(&payload);
        assert_eq!(len, payload.len());
        let mut rd = BsReader::default();
        assert_eq!(
            DecInitBits(&mut rd, &raw, start, (len * 8) as i32),
            ERR_NONE
        );

        let mut vals = Vec::new();
        for _ in 0..4 {
            let (buf, cursor) = rd.split(&raw);
            vals.push(cursor.get_ue(buf).unwrap());
        }
        raw.grow(16).unwrap(); // 16 * 8 = 128 > 96 = len << 1
        assert_eq!(raw.len(), 128);
        raw.grow(20).unwrap(); // len << 1 = 256 > 160 = 20 * 8
        assert_eq!(raw.len(), 256);
        // The allocator may move the block or expand it in place, so the address is
        // deliberately not asserted; the property is that reads through an offset-based
        // reader are identical across the growth either way.
        for _ in 0..8 {
            let (buf, cursor) = rd.split(&raw);
            vals.push(cursor.get_ue(buf).unwrap());
        }
        assert_eq!(vals, control_vals);
    }

    #[test]
    fn test_rbsp2ebsp_emulation_prevention() {
        // [0x00, 0x00, 0x00] -> [0x00, 0x00, 0x03, 0x00]
        // [0x00, 0x00, 0x01] -> [0x00, 0x00, 0x03, 0x01]
        // [0x00, 0x00, 0x02] -> [0x00, 0x00, 0x03, 0x02]
        // [0x00, 0x00, 0x03] -> [0x00, 0x00, 0x03, 0x03]
        let src: [u8; 6] = [0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let mut dst: [u8; 10] = [0; 10];

        let written = rbsp_to_ebsp(&src, &mut dst);
        assert_eq!(written, 8);

        let expected = [0x00, 0x00, 0x03, 0x01, 0x00, 0x00, 0x03, 0x00];
        assert_eq!(&dst[0..8], &expected);
    }

    #[test]
    fn test_rbsp_to_ebsp_slice_helper() {
        let src = [0x00, 0x00, 0x02, 0xFF, 0x00, 0x00, 0x03];
        let mut dst = [0u8; 12];
        let count = rbsp_to_ebsp(&src, &mut dst);
        assert_eq!(count, 9);
        assert_eq!(
            &dst[0..9],
            &[0x00, 0x00, 0x03, 0x02, 0xFF, 0x00, 0x00, 0x03, 0x03]
        );
    }
}
