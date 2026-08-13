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

// The `SBitStringAux` / `TagBitStringAux` / `PBitStringAux` re-export lived here,
// with a note predicting when the type would die. It died at T3.4 — one seam
// earlier than that note guessed for the embedded fields, because converting the
// writer family forced the struct fields in the same commit rather than leaving
// them to Phase 6. `BsReader` + `BsCursor` are the read side; `BsWriter` is the
// write side; there is no third thing.

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

/// **`READER_SLOP` is not the readable extent** — found by T3.0's goldens at T3.1b,
/// recorded as `phase3_findings.md` §F16.
///
/// The derivation above covers `dump_bits_aux` only. `BsEndCavlc` primes **four** bytes
/// at `iIndex >> 3`, where `iIndex` is advanced by the residual decoder by however many
/// bits it consumed — and on a truncated stream that runs past the RBSP by an amount
/// bounded by nothing but how many symbols the parser accepts before erroring. The raw
/// reader was observed reaching `len + 5` and beyond on six conformance streams'
/// truncations; it was "safe" only because `sRawData` is a multi-MiB allocation and
/// those bytes were inside it.
///
/// So the readable extent is a property of the *allocation*, not a constant offset
/// from the RBSP length. Since T3.3 the allocation is [`RawDataBuffer`]'s `Vec` and
/// every window is derived from it at call time ([`RawDataBuffer::window_from`]);
/// there is no stored extent left to hold, or to go stale.

/// The decoder's raw-bitstream accumulation buffer — T3.3's replacement for
/// `SDataBuffer { pHead, pEnd, pStartPos, pCurPos }`.
///
/// Owns the bytes `WelsDecodeBs` accumulates (EPB-stripped NAL payloads) and the
/// write position. Everything else is **derived at call time**: the readable extent
/// behind any offset is `buf.len() - start`, computed by
/// [`window_from`](Self::window_from) against the buffer as it is *now*. Nothing
/// stores a length the buffer can outgrow, which is what makes F16's class of defect
/// (a stored extent surviving a growth) unrepresentable rather than guarded against.
///
/// The backing store is kept at its full allocated size and zero-filled —
/// `WelsMalloczHelper`'s semantics, preserved deliberately: the C allocation was
/// zeroed, and the reader's slop past the last NAL read those bytes (F4/F16). The
/// slop bytes behind every NAL except the last are the next NAL's real stream bytes
/// by construction; behind the last they are the same in-bounds zeroed/stale tail the
/// raw code read. `buf.len()` is initialized bytes — **never** spare `Vec` capacity.
///
/// `pStartPos` is not carried: in this port it was written at init and rebase and
/// read by nothing (the upstream parse-only rewind that consumes it was never
/// ported), so the struct stores one offset, not two.
#[derive(Debug, Default)]
pub struct RawDataBuffer {
    buf: Vec<u8>,
    /// The write position — was `pCurPos - pHead`.
    cur: usize,
}

impl RawDataBuffer {
    /// A zero-filled buffer of `len` bytes — `WelsMalloczHelper`'s allocation, owned.
    ///
    /// Fails like the C helper did (a null return became `ERR_INFO_OUT_OF_MEMORY`)
    /// instead of aborting inside `Vec`'s infallible growth.
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

    /// Allocation size in bytes — was `pEnd - pHead` / `iMaxBsBufferSizeInByte`.
    #[inline]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The write position — was `pCurPos - pHead`.
    #[inline]
    pub fn cur(&self) -> usize {
        self.cur
    }

    /// Bytes writable at [`cur`](Self::cur) — was `pEnd - pCurPos`, in comparison-safe
    /// form: `cur <= len` is structural (every mutation preserves it), and if it were
    /// ever broken the answer is 0, not a wrapped `usize`.
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
    /// This is `ExpandBsBuffer`'s growth policy, verbatim: the new size is
    /// `max(src_len * MAX_BUFFERED_NUM, len << 1)`. What is *not* here is the rest of
    /// that function — the pointer-rebasing block died with the pointers, because
    /// offsets survive a reallocation by definition (plan §2.2.2, P5), and there is no
    /// stored extent left to go stale (F16's second instance).
    ///
    /// Failure maps to the C path (`ERR_INFO_OUT_OF_MEMORY`) rather than aborting.
    pub fn grow(&mut self, src_len: usize) -> Result<(), ()> {
        let new_len = std::cmp::max(
            src_len * crate::decoder::decoder_core::MAX_BUFFERED_NUM,
            self.buf.len() << 1,
        );
        self.grow_to(new_len)
    }

    /// Grows to exactly `new_len` (used to keep `sSavedData` the same size as
    /// `sRawData`, as `ExpandBsBuffer` did). A `new_len` at or below the current size
    /// is a no-op, matching the C's grow-only reallocation.
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
    /// bytes (`00 00 03` → `00 00`) exactly as `WelsDecodeBs`'s copy loop did.
    /// Returns `(start, len)` of the stripped payload within the buffer.
    ///
    /// The caller has already ensured `remaining() >= payload.len() + 4` (the
    /// rewind/grow dance in `WelsDecodeBs`); the destination slice is taken once up
    /// front, so a violated contract panics on the slice take, not byte-by-byte.
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

    /// The whole backing store.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// The readable window behind offset `start`: everything from `start` to the end
    /// of the allocation — was `readable_from`'s `pEnd - p`, now derived from the
    /// owner at call time (the F16 rule). `start <= len` holds for every offset the
    /// decoder mints ([`append_ebsp_stripped`] returns positions inside the buffer,
    /// and growth never shrinks it); the clamp routes a broken invariant to an empty
    /// window — every read then fails with `ERR_INFO_READ_OVERFLOW` — rather than
    /// introducing a panic where the raw code read allocation bytes.
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

    /// The **RBSP window** for a reader: the first [`BsCursor::len`] bytes of its
    /// readable window. This is what the CABAC engine reads through (T3.2): `len` is
    /// the logical end of the RBSP — the C++ `pBuffEnd` the engine's end ladder
    /// measures against — so `win.len()` *is* the ladder's selector and the engine
    /// computes no extent of its own. See the read-extent audit in
    /// `cabac_decoder.rs`'s module docs.
    ///
    /// `window.len() >= cursor.len()` holds structurally — `WelsDecodeBs` refuses to
    /// append a payload without four bytes to spare, EPB stripping only shrinks it,
    /// and growth only widens the window — so the `min` is dead; the `debug_assert`
    /// keeps that checkable, and the clamp keeps a violated contract on the error
    /// path instead of adding a new panic (F4/F16's shape).
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
/// T3.1b's bridge (`{base, avail, cursor}` and the `from_raw_parts` that reconstructed
/// a slice from it) died at T3.3 with `SDataBuffer`: `start` is a position — an offset
/// into the owner, which survives the owner's growth by definition — and every window
/// is derived from the owner at call time. Plan §2.1.3's split is unchanged: consumers
/// take `(buf: &[u8], cursor: &mut BsCursor)`, produced by [`split`](Self::split).
///
/// `DqLayerState::pBitStringAux` remains `*mut BsReader` (the pointer is Phase 5's to
/// remove); what changed is that the thing behind it no longer holds a pointer or an
/// extent of its own.
#[derive(Clone, Copy, Debug, Default)]
pub struct BsReader {
    /// Offset of this NAL's payload in the owning [`RawDataBuffer`].
    pub start: usize,
    /// The position and accumulator. All the arithmetic lives here.
    pub cursor: BsCursor,
}

impl BsReader {
    /// Splits into the two halves the consumers take: the bytes, and the position.
    /// The window is derived from `raw` now, not from stored state — safe, and
    /// exactly as wide as the raw reader's was (`pEnd - p`).
    #[inline(always)]
    pub fn split<'a>(&'a mut self, raw: &'a RawDataBuffer) -> (&'a [u8], &'a mut BsCursor) {
        (raw.window_from(self.start), &mut self.cursor)
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
/// in `bit_stream.cpp`. The body is [`BsCursor::init`] over the window derived from
/// the owning buffer at `start` — was `readable_from`'s `pEnd - p`, now
/// [`RawDataBuffer::window_from`]. The old null-pointer guard has no offset
/// equivalent (there is no pointer); the `(kiSize + 7) >> 3 <= 0` case returns
/// [`ERR_INFO_INVALID_ACCESS`] from `init` exactly as the pre-check did.
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
    
    /// The reader reads `READER_SLOP` bytes past the RBSP it is handed — it always
    /// did (F4), and the decoder's raw buffer always has the slack (`WelsDecodeBs`
    /// sizes every payload with four bytes to spare). These tests supply it rather
    /// than relying on the reader not reaching that far.
    fn with_slop(payload: &[u8]) -> Vec<u8> {
        let mut v = payload.to_vec();
        v.extend_from_slice(&[0u8; READER_SLOP]);
        v
    }

    #[test]
    fn test_dec_init_bits_and_init_read_bits() {
        let raw = RawDataBuffer::from_vec(with_slop(&[
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22,
        ]));
        let mut bs = BsReader::default();

        let err = DecInitBits(&mut bs, &raw, 0, 64);
        assert_eq!(err, ERR_NONE);
        assert_eq!(bs.cursor.cur_bits(), 0xAABBCCDD);
        assert_eq!(bs.cursor.left_bits(), -16);
        assert_eq!(bs.cursor.bits(), 64);
        // `pCurBuf`/`pEndBuf` are `pos`/`len` now — the same two numbers, as offsets.
        assert_eq!(bs.cursor.pos(), 4);
        assert_eq!(bs.cursor.len(), 8);
        assert_eq!(bs.start, 0);
        // And the derived window is exactly the declared footprint.
        assert_eq!(raw.window_from(bs.start).len(), 8 + READER_SLOP);
    }

    #[test]
    fn test_dec_init_bits_empty_window() {
        // The nearest offset analogue of the old null-pointer rejection: a window
        // with no bytes fails `init`'s 4-byte prime with READ_OVERFLOW rather than
        // reading anything, and a non-positive size keeps INVALID_ACCESS parity.
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

    /// P5's test, asked for by the plan since rev 1: grow the buffer mid-AU and
    /// assert parse continuity. A reader over an early NAL is mid-read when a later
    /// NAL forces growth; because the reader stores an offset and every window is
    /// derived from the owner at call time, the values it decodes after the growth
    /// are identical to a control run with no growth — the latent-bug class that
    /// pointer rebasing fixed silently (and F16's stale-`avail` broke loudly) is
    /// pinned here. Growth genuinely reallocates: the initial size is small and the
    /// appended NAL is bigger than the whole buffer.
    #[test]
    fn p5_reader_survives_growth_mid_au() {
        // A payload of ascending bytes: ue(v) reads below give known values.
        let payload: Vec<u8> = (1..=32).collect();

        // Control: read the whole thing with no growth.
        let mut control_vals = Vec::new();
        {
            let raw = RawDataBuffer::from_vec(payload.clone());
            let mut rd = BsReader::default();
            assert_eq!(DecInitBits(&mut rd, &raw, 0, (payload.len() * 8) as i32), ERR_NONE);
            for _ in 0..12 {
                let (buf, cursor) = rd.split(&raw);
                control_vals.push(cursor.get_ue(buf).unwrap());
            }
        }

        // Growth run: same payload at the same offset, but the buffer is grown —
        // twice, the second time by more than 8x so `grow`'s `max` takes each arm
        // once — after the reader has consumed part of the stream.
        let mut raw = RawDataBuffer::try_new_zeroed(48).unwrap();
        let (start, len) = raw.append_ebsp_stripped(&payload);
        assert_eq!(len, payload.len());
        let mut rd = BsReader::default();
        assert_eq!(DecInitBits(&mut rd, &raw, start, (len * 8) as i32), ERR_NONE);

        let mut vals = Vec::new();
        for _ in 0..4 {
            let (buf, cursor) = rd.split(&raw);
            vals.push(cursor.get_ue(buf).unwrap());
        }
        raw.grow(16).unwrap(); // 16 * 8 = 128 > 96 = len << 1
        assert_eq!(raw.len(), 128);
        raw.grow(20).unwrap(); // len << 1 = 256 > 160 = 20 * 8
        assert_eq!(raw.len(), 256);
        // Whether the allocator moves the block or expands it in place is its
        // business (release builds have been seen doing either for this pattern);
        // the property pinned here — reads through an offset-based reader are
        // identical across the growth — must hold in both cases, so the address is
        // deliberately not asserted.
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
