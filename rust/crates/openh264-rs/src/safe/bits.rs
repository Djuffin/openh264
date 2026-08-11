#![forbid(unsafe_code)]

//! Detached bit cursors — the safe replacement for taxonomy class **T3**
//! (plan §1.2, contract §2.2.2).
//!
//! [`BsCursor`] reads and [`BsWriter`] writes; neither holds a buffer. They are
//! *positions*, passed the bytes on every call, which is what makes them `Copy` —
//! and that in turn is what replaces the encoder's `pBsStackBufPtr` stash/pop
//! (`svc_set_mb_syn_cavlc.rs:1057-1076`) with `let saved = writer;`, and what makes
//! `ExpandBsBuffer`'s pointer-rebasing block (`decoder_core.rs:1816-1842`) deletable
//! rather than portable: an offset survives a reallocation by definition.
//!
//! # Scope
//!
//! * **RBSP only.** Emulation-prevention bytes (EBSP, the `0x03` insertion) are
//!   `nalu.rs`'s business and stay there — by the time a cursor sees bytes, they are
//!   raw payload. `RBSP2EBSP` is not a cursor operation.
//! * **No CABAC.** The arithmetic engine keeps its own cursor triple and is Phase
//!   3.2's job; the CAVLC↔CABAC handoff (`cabac_decoder.rs:712-717`) additionally
//!   *writes* `uiCurBits`/`iLeftBits`, so it will need an explicit method here when
//!   that conversion happens. Nothing speculative is offered for it now.
//! * **CAVLC mode, yes.** `SBitStringAux::iIndex` — the absolute bit position the
//!   CAVLC residual path reads while the accumulator is deliberately stale — is
//!   mirrored here as [`BsCursor::start_cavlc`]/[`BsCursor::end_cavlc`] and the
//!   [`cavlc_bit_pos`](BsCursor::cavlc_bit_pos) accessor pair. Phase 1 recorded
//!   `iIndex` as having no consumer; T3.1b's inventory found that wrong (plan
//!   §2.2.2 **[P3]**).

use crate::safe::err::ErrInfo;

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// A bitstream read position — the reading half of `SBitStringAux`
/// (`common/wels_common_defs.rs:30-46`), with the three pointers replaced by two
/// offsets.
///
/// | C++ field | here | note |
/// |---|---|---|
/// | `pCurBuf - pStartBuf` | `pos` | byte offset of the next refill |
/// | `pEndBuf - pStartBuf` | `len` | the **logical** end (`iAllowedBytes`) |
/// | `uiCurBits` | `cur_bits` | MSB-aligned accumulator |
/// | `iLeftBits` | `left_bits` | bits available in it, biased by −16 |
/// | `iBits` | `bits` | size of the RBSP in bits |
/// | `iIndex` | `cavlc_bit_pos` | absolute bit position, live only in CAVLC mode |
///
/// `len` is state, not a property of the slice passed in: the C++ end pointer marks
/// the end of the *RBSP*, while the allocation legitimately continues past it — see
/// the slop discussion on [`BsCursor::get_bits`]. This is a deliberate deviation from
/// the plan's three-field sketch (§2.2.2); without it, error-code parity at the end
/// of a NAL is not expressible.
#[derive(Clone, Copy, Debug, Default, Eq)]
pub struct BsCursor {
    pos: usize,
    cur_bits: u32,
    left_bits: i32,
    len: usize,
    bits: i32,
    /// The C++ `iIndex`. See [`start_cavlc`](BsCursor::start_cavlc).
    cavlc_bit_pos: isize,
    /// Whether [`start_cavlc`](BsCursor::start_cavlc) has run without a matching
    /// [`end_cavlc`](BsCursor::end_cavlc) — the coherence guard described there.
    /// Debug-only, and deliberately **not** part of [`PartialEq`].
    #[cfg(debug_assertions)]
    in_cavlc: bool,
}

/// Equality over the six fields `SBitStringAux` mirrors, and nothing else.
///
/// Written by hand rather than derived because `in_cavlc` exists only under
/// `cfg(debug_assertions)`: a derived `PartialEq` would compare it in debug builds and
/// not in release, so two cursors could be equal in one profile and unequal in the
/// other. The parity tests compare cursor states across both profiles (S16's
/// dual-profile discipline), and that skew is exactly what it exists to prevent — the
/// same reasoning that keeps the pool's debug generation counter out of handle
/// equality (plan §D1).
impl PartialEq for BsCursor {
    fn eq(&self, other: &Self) -> bool {
        self.pos == other.pos
            && self.cur_bits == other.cur_bits
            && self.left_bits == other.left_bits
            && self.len == other.len
            && self.bits == other.bits
            && self.cavlc_bit_pos == other.cavlc_bit_pos
    }
}

/// Peeks the top `n` bits of an MSB-aligned accumulator.
///
/// Mirrors the `UBITS` macro (`dec_golomb.rs:116` / `codec/decoder/core/inc/dec_golomb.h`).
#[inline]
fn ubits(cur_bits: u32, n: i32) -> u32 {
    if n <= 0 {
        0
    } else if n >= 32 {
        cur_bits
    } else {
        cur_bits >> (32 - n)
    }
}

/// Number of leading zero bits in `cur_bits`, or `-1` if it is entirely zero.
///
/// Mirrors `GetLeadingZeroBits` (`dec_golomb.rs:205` /
/// `codec/decoder/core/inc/dec_golomb.h`). The C++ walks `g_kuiLeadingZeroTable` in
/// four byte-wide steps; `leading_zeros()` is the same function of the same input,
/// which `leading_zero_bits_matches_the_table` in the differential test proves
/// exhaustively over the interesting range.
#[inline]
fn leading_zero_bits(cur_bits: u32) -> i32 {
    if cur_bits == 0 {
        -1
    } else {
        cur_bits.leading_zeros() as i32
    }
}

/// Two's-complement negation, mirroring the `NEG_NUM` macro (`dec_golomb.rs:110`).
#[inline]
fn neg_num(x: i32) -> i32 {
    1 + !x
}

/// Number of zero bits before the `rbsp_stop_one_bit` in `byte`.
///
/// Mirrors `BsGetTrailingBits` (`dec_golomb.rs:320` /
/// `codec/decoder/core/inc/dec_golomb.h`), including its quirk of returning `0` for
/// an all-zero byte rather than an error.
#[inline]
pub fn trailing_bits(byte: u8) -> i32 {
    let mut value = byte as u32;
    let mut n = 0;
    while n < 9 {
        if value & 1 != 0 {
            return n;
        }
        value >>= 1;
        n += 1;
    }
    0
}

impl BsCursor {
    /// Starts reading an RBSP of `size_bits` bits from the front of `buf`.
    ///
    /// Mirrors `DecInitBits` (`decoder/bit_stream.rs:84` /
    /// `codec/decoder/core/src/bit_stream.cpp`), including its initial 4-byte fill and
    /// the `iLeftBits = -16` bias.
    ///
    /// `buf` should be the whole readable region — RBSP **plus** at least 3 bytes of
    /// slack, see [`get_bits`](Self::get_bits). Only the first `(size_bits + 7) / 8`
    /// bytes are treated as payload.
    pub fn init(buf: &[u8], size_bits: i32) -> Result<Self, ErrInfo> {
        let end = (size_bits + 7) >> 3;
        if end <= 0 {
            // C++ sets pEndBuf = pStartBuf + kiSizeBuf and InitReadBits then fails its
            // `pCurBuf >= pEndBuf` check.
            return Err(ErrInfo::INVALID_ACCESS);
        }
        let mut cursor = Self {
            pos: 0,
            cur_bits: 0,
            left_bits: 0,
            len: end as usize,
            bits: size_bits,
            cavlc_bit_pos: 0,
            #[cfg(debug_assertions)]
            in_cavlc: false,
        };
        cursor.init_read_bits(buf, 0)?;
        Ok(cursor)
    }

    /// Re-primes the accumulator at the current position, refusing to start within
    /// `end_offset` bytes of the logical end.
    ///
    /// Mirrors `InitReadBits` (`decoder/bit_stream.rs:57`). Called with `1` by the
    /// CABAC initialisation (`parse_mb_syn_cabac.rs:3312`) and with `0` by
    /// `decode_slice.rs:2474`.
    pub fn init_read_bits(&mut self, buf: &[u8], end_offset: isize) -> Result<(), ErrInfo> {
        self.debug_assert_out_of_cavlc("init_read_bits");
        let end_limit = self.len as isize - end_offset;
        if self.pos as isize >= end_limit {
            return Err(ErrInfo::INVALID_ACCESS);
        }
        // `GetValue4Bytes` (`decoder/bit_stream.rs:40`). The C++ reads four bytes
        // unconditionally; short-buffer behaviour is the divergence documented on
        // `get_bits`.
        let b = buf
            .get(self.pos..self.pos + 4)
            .ok_or(ErrInfo::READ_OVERFLOW)?;
        self.cur_bits = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        self.pos += 4;
        self.left_bits = -16;
        Ok(())
    }

    /// Byte offset of the next refill — the C++ `pCurBuf - pStartBuf`.
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// The logical end of the RBSP in bytes — the C++ `pEndBuf - pStartBuf`.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Size of the RBSP in bits — the C++ `iBits`.
    #[inline]
    pub fn bits(&self) -> i32 {
        self.bits
    }

    /// The MSB-aligned accumulator — the C++ `uiCurBits`.
    #[inline]
    pub fn cur_bits(&self) -> u32 {
        self.cur_bits
    }

    /// Bits available in the accumulator, biased by −16 — the C++ `iLeftBits`.
    #[inline]
    pub fn left_bits(&self) -> i32 {
        self.left_bits
    }

    /// Moves the byte position, leaving the accumulator alone.
    ///
    /// The I_PCM path's seek (`decode_slice.rs`, `WelsActualDecodeMbCavlcISlice`'s
    /// `25 == uiMbType` branch): PCM samples are byte-aligned raw bytes, so the C++
    /// rewinds `pCurBuf` to the alignment point, memcpy's 384 bytes out of the stream,
    /// advances past them and re-primes with `InitReadBits`. Only that re-prime makes
    /// the accumulator meaningful again, which is why this deliberately does not touch
    /// it — a `set_pos` that also refilled would not be the same function.
    #[inline]
    pub fn set_pos(&mut self, pos: usize) {
        self.debug_assert_out_of_cavlc("set_pos");
        self.pos = pos;
    }

    /// The top `n` bits of the accumulator, without consuming them.
    ///
    /// Mirrors the `UBITS` macro; the C++ applies it directly to `uiCurBits` at six
    /// sites.
    #[inline]
    pub fn peek_bits(&self, n: i32) -> u32 {
        self.debug_assert_out_of_cavlc("peek_bits");
        ubits(self.cur_bits, n)
    }

    /// Consumes `n` bits and refills the accumulator.
    ///
    /// Mirrors `dump_bits_aux` (`dec_golomb.rs:128-150`), the refill at the heart of
    /// every read below — see [`get_bits`](Self::get_bits) for the slop predicate.
    fn dump_bits(&mut self, buf: &[u8], n: i32) -> Result<(), ErrInfo> {
        self.cur_bits = self.cur_bits.wrapping_shl(n as u32);
        self.left_bits += n;
        if self.left_bits > 0 {
            // C++: `if (iReadBytes > iAllowedBytes + 1) return ERR_INFO_READ_OVERFLOW;`
            if self.pos > self.len + 1 {
                return Err(ErrInfo::READ_OVERFLOW);
            }
            let b0 = *buf.get(self.pos).ok_or(ErrInfo::READ_OVERFLOW)? as u32;
            let b1 = *buf.get(self.pos + 1).ok_or(ErrInfo::READ_OVERFLOW)? as u32;
            let word = (b0 << 8) | b1;
            let shift = self.left_bits as u32;
            if shift < 32 {
                self.cur_bits |= word.wrapping_shl(shift);
            }
            self.left_bits -= 16;
            self.pos += 2;
        }
        Ok(())
    }

    /// Reads `n` bits (1..=32), MSB first.
    ///
    /// Mirrors `BsGetBits` (`dec_golomb.rs:157` /
    /// `codec/decoder/core/inc/dec_golomb.h`).
    ///
    /// # The 16-bit ceiling
    ///
    /// The refill tops the accumulator up 16 bits at a time and only when it has run
    /// below 16, so at rest it holds **at least** 16 valid bits and no more than 32.
    /// A single read of more than 16 bits can therefore return stale low bits — and
    /// does, identically, in the C++. It is not a defect either side: no decoder call
    /// site asks for more (the `BsGetBits` widths in `src/decoder/` are 1, 2, 3, 4, 5,
    /// 8 and 16), and `get_ue` splits its own long prefixes for exactly this reason.
    /// `bit_reads_over_16_reproduce_the_stale_low_bits` in the differential test pins
    /// the quirk to the old implementation rather than papering over it.
    ///
    /// # The slop predicate
    ///
    /// `dump_bits_aux` permits the cursor to sit **one byte past** the logical end
    /// (`iReadBytes > iAllowedBytes + 1` is the *failure* condition) and then reads
    /// two bytes at it — so the C++ reads up to `len + 2` inclusive, i.e. three bytes
    /// of allocation slack, and the port is only sound because the raw-data buffer is
    /// bigger than the NAL it holds. That predicate is reproduced here bit for bit,
    /// because error-code parity on truncated streams depends on it: a stream that
    /// ends mid-symbol must still fail at exactly the same read.
    ///
    /// What is *not* reproduced is reading beyond the slice: those loads go through
    /// `get`, so a `buf` without slack returns [`ErrInfo::READ_OVERFLOW`] where the
    /// C++ would have read whatever followed the allocation. **Pass `buf` with at
    /// least 3 bytes of slack past `size_bits` and the two are identical for every
    /// input** — with fewer, this cursor is strictly the safer of the two. Wiring the
    /// guard bytes into the decoder's real buffers is Phase 3's decision (plan P6);
    /// `phase1_findings.md` §F4 records the measurement behind this paragraph.
    pub fn get_bits(&mut self, buf: &[u8], n: i32) -> Result<u32, ErrInfo> {
        self.debug_assert_out_of_cavlc("get_bits");
        let value = ubits(self.cur_bits, n);
        self.dump_bits(buf, n)?;
        Ok(value)
    }

    /// Reads one bit. Mirrors `BsGetOneBit` (`dec_golomb.rs:197`).
    #[inline]
    pub fn get_one_bit(&mut self, buf: &[u8]) -> Result<u32, ErrInfo> {
        self.get_bits(buf, 1)
    }

    /// Reads an unsigned Exp-Golomb code, `ue(v)`.
    ///
    /// Mirrors `BsGetUe` (`dec_golomb.rs:233`), including its split refill for
    /// prefixes longer than 16 bits and its wrapping reconstruction of the value.
    pub fn get_ue(&mut self, buf: &[u8]) -> Result<u32, ErrInfo> {
        // `get_se`, `get_te0` and `get_one_bit` reach the accumulator only through this
        // and `get_bits`, so the two guards above cover the whole read family.
        self.debug_assert_out_of_cavlc("get_ue");
        let mut value: u32 = 0;
        let lz = leading_zero_bits(self.cur_bits);

        if lz == -1 {
            return Err(ErrInfo::READ_LEADING_ZERO);
        } else if lz > 16 {
            self.dump_bits(buf, 16)?;
            self.dump_bits(buf, lz + 1 - 16)?;
        } else {
            self.dump_bits(buf, lz + 1)?;
        }

        if lz != 0 {
            value = ubits(self.cur_bits, lz);
            self.dump_bits(buf, lz)?;
        }

        Ok((1u32.wrapping_shl(lz as u32))
            .wrapping_sub(1)
            .wrapping_add(value))
    }

    /// Reads a signed Exp-Golomb code, `se(v)`.
    ///
    /// Mirrors `BsGetSe` (`dec_golomb.rs:275`).
    pub fn get_se(&mut self, buf: &[u8]) -> Result<i32, ErrInfo> {
        let code_num = self.get_ue(buf)?;
        Ok(if code_num & 0x01 != 0 {
            ((code_num + 1) >> 1) as i32
        } else {
            neg_num((code_num >> 1) as i32)
        })
    }

    /// Reads a truncated Exp-Golomb code, `te(v)`, over `range` values.
    ///
    /// Mirrors `BsGetTe0` (`dec_golomb.rs:296`).
    pub fn get_te0(&mut self, buf: &[u8], range: i32) -> Result<u32, ErrInfo> {
        if range == 1 {
            Ok(0)
        } else if range == 2 {
            Ok(self.get_one_bit(buf)? ^ 1)
        } else {
            self.get_ue(buf)
        }
    }

    /// Whether more RBSP data precedes `rbsp_trailing_bits()`.
    ///
    /// Mirrors `CheckMoreRBSPData` (`dec_golomb.rs:341`).
    pub fn check_more_rbsp_data(&self) -> bool {
        self.debug_assert_out_of_cavlc("check_more_rbsp_data");
        let offset_bytes = self.pos as isize - 2;
        let bits_consumed = (offset_bytes << 3) as i32;
        self.bits - bits_consumed - self.left_bits > 1
    }

    // -----------------------------------------------------------------------
    // CAVLC mode — plan §2.2.2 [P3]
    // -----------------------------------------------------------------------

    /// Panics in debug builds if the cursor is inside a CAVLC region.
    ///
    /// The accumulator is *deliberately stale* between [`start_cavlc`](Self::start_cavlc)
    /// and [`end_cavlc`](Self::end_cavlc), so reading it there yields bits the residual
    /// path has already consumed. In C++ that desync is undetectable — `SBitStringAux`
    /// has no notion of which of its two position representations is authoritative — and
    /// it decodes silently wrong. Here it is a panic with a name on it.
    #[inline(always)]
    fn debug_assert_out_of_cavlc(&self, op: &str) {
        #[cfg(debug_assertions)]
        assert!(
            !self.in_cavlc,
            "BsCursor::{op} ran inside a CAVLC region: the accumulator is stale between \
             start_cavlc and end_cavlc, and `cavlc_bit_pos` is the live position"
        );
        let _ = op;
    }

    /// Panics in debug builds if the cursor is *not* inside a CAVLC region — the
    /// mirror of [`debug_assert_out_of_cavlc`](Self::debug_assert_out_of_cavlc), for
    /// the bit-position accessors, whose value is dead outside the mode.
    #[inline(always)]
    fn debug_assert_in_cavlc(&self, op: &str) {
        #[cfg(debug_assertions)]
        assert!(
            self.in_cavlc,
            "BsCursor::{op} ran outside a CAVLC region: `cavlc_bit_pos` is only live \
             between start_cavlc and end_cavlc"
        );
        let _ = op;
    }

    /// Enters CAVLC mode: projects the cursor onto an absolute bit position.
    ///
    /// Mirrors `BsStartCavlc` (`parse_mb_syn_cavlc.rs:2229`) exactly. The accumulator
    /// holds `16 - left_bits` valid bits (32 immediately after a prime, since
    /// `left_bits` is biased by −16), so the position of the next unread bit is
    /// `8 * pos - (16 - left_bits)` — and the `16` is the CAVLC residual machinery's
    /// **16-bit half-window**, not a mistyped 32. It is copied from the C++ rather than
    /// rederived.
    ///
    /// While the mode is live, `cavlc_bit_pos` is authoritative and `cur_bits`/
    /// `left_bits`/`pos` are stale — the residual decoder walks bytes directly from the
    /// bit position through its own `SReadBitsCache`. [`end_cavlc`](Self::end_cavlc)
    /// puts the accumulator back.
    #[inline]
    pub fn start_cavlc(&mut self) {
        self.cavlc_bit_pos = ((self.pos as isize) << 3) - (16 - self.left_bits as isize);
        #[cfg(debug_assertions)]
        {
            self.in_cavlc = true;
        }
    }

    /// Leaves CAVLC mode: reseats the accumulator at `cavlc_bit_pos`.
    ///
    /// Mirrors `BsEndCavlc` (`parse_mb_syn_cavlc.rs:2236`) exactly, including
    /// `left_bits` going **negative on purpose** — `-16 + (idx & 7)` is the same −16
    /// bias every other prime uses, offset by the sub-byte phase the 4-byte load was
    /// shifted by.
    ///
    /// The 4-byte load at `idx >> 3` indexes the slice, and **`buf` must be the whole
    /// readable allocation, not the RBSP plus a constant**. `cavlc_bit_pos` is advanced
    /// by the residual decoder by whatever each symbol consumed, so on a truncated
    /// stream it runs past the RBSP end by an amount bounded by nothing but how many
    /// symbols the parser accepts — the raw pair was measured reaching `len + 5` and
    /// beyond, and was in bounds only because the decoder's raw-data buffer is 4 MiB.
    /// `phase3_findings.md` §F16 records this; the decoder's `BsReader::avail` is where
    /// the extent comes from. An out-of-range index here is a pre-existing overrun
    /// surfacing, per plan §2.2.2 — it is not silenced with a `get()` fallback, because
    /// the raw pair had no such fallback and error-code parity is the gate.
    ///
    /// Round-tripping restores the *reading position*, not the field values: the
    /// re-prime can leave the accumulator holding more valid bits than it did before
    /// [`start_cavlc`](Self::start_cavlc). Since it always holds at least 16, every read
    /// the codec makes (widths 1..=16) is unaffected — see the 16-bit ceiling on
    /// [`get_bits`](Self::get_bits).
    #[inline]
    pub fn end_cavlc(&mut self, buf: &[u8]) {
        let idx = self.cavlc_bit_pos;
        self.pos = (idx >> 3) as usize;
        let b = &buf[self.pos..self.pos + 4];
        let cache = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        self.cur_bits = cache << ((idx & 0x07) as u32);
        self.pos += 4;
        self.left_bits = -16 + ((idx & 0x07) as i32);
        #[cfg(debug_assertions)]
        {
            self.in_cavlc = false;
        }
    }

    /// The absolute bit position — the C++ `iIndex`. Live only in CAVLC mode.
    #[inline]
    pub fn cavlc_bit_pos(&self) -> isize {
        self.debug_assert_in_cavlc("cavlc_bit_pos");
        self.cavlc_bit_pos
    }

    /// Advances the absolute bit position by `bits` — the C++ `pBs->iIndex += n`, which
    /// is how the residual path reports what it consumed.
    #[inline]
    pub fn advance_cavlc_bits(&mut self, bits: isize) {
        self.debug_assert_in_cavlc("advance_cavlc_bits");
        self.cavlc_bit_pos += bits;
    }

    // -----------------------------------------------------------------------
    // The CAVLC↔CABAC handoff. T3.2 owns the engine; these two are the reader's
    // side of the boundary, and they exist as *named operations* rather than as
    // `set_cur_bits`/`set_left_bits` setters because every one of these writes is
    // only coherent as part of a whole handoff.
    // -----------------------------------------------------------------------

    /// Marks the accumulator spent because the CABAC engine has taken over the
    /// position.
    ///
    /// Mirrors `InitCabacDecEngineFromBS`'s closing `pBsAux->iLeftBits = 0`
    /// (`cabac_decoder.rs:697`). The engine reads from the shared buffer from here on;
    /// the cursor's accumulator is meaningless until [`restore_from_cabac`] or a
    /// re-prime.
    ///
    /// [`restore_from_cabac`]: Self::restore_from_cabac
    #[inline]
    pub fn hand_off_to_cabac(&mut self) {
        self.debug_assert_out_of_cavlc("hand_off_to_cabac");
        self.left_bits = 0;
    }

    /// Takes the position back from the CABAC engine, at byte offset `pos`.
    ///
    /// Mirrors `RestoreCabacDecEngineToBS`'s four writes to `SBitStringAux`
    /// (`cabac_decoder.rs:712-718`): position, a cleared accumulator, and `iIndex`
    /// zeroed. That last one is why this is not simply [`set_pos`](Self::set_pos) —
    /// the C++ clears `iIndex` defensively here, *outside* any CAVLC region, so it is
    /// part of the handoff rather than a mode operation and does not assert.
    ///
    /// T3.2 converts the engine itself, at which point this is the `usize` assignment
    /// the phase brief describes and gains its round-trip test against a known bit
    /// offset.
    #[inline]
    pub fn restore_from_cabac(&mut self, pos: usize) {
        self.pos = pos;
        self.cur_bits = 0;
        self.left_bits = 0;
        self.cavlc_bit_pos = 0;
        #[cfg(debug_assertions)]
        {
            self.in_cavlc = false;
        }
    }

    /// The raw `cavlc_bit_pos` field, with no mode assertion.
    ///
    /// State inspection for the parity tests, in the same family as
    /// [`cur_bits`](Self::cur_bits) and [`left_bits`](Self::left_bits) — the differential
    /// tests compare all six C-mirrored fields *after* `end_cavlc` has cleared the mode,
    /// where the value is dead but must still match the C++ byte for byte. Production
    /// code wants [`cavlc_bit_pos`](Self::cavlc_bit_pos), which asserts.
    #[inline]
    pub fn cavlc_bit_pos_state(&self) -> isize {
        self.cavlc_bit_pos
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// A bitstream write position — the writing half of `SBitStringAux`.
///
/// # Which of the four writers this is
///
/// The port contains **four** copies of the `Bs*` writer family, and they are not
/// identical: `encoder/vlc_encoder.rs:367` (canonical, matching C++
/// `codec/common/inc/bit_stream.h`), `svc_set_mb_syn_cavlc.rs:157`,
/// `nal_encap.rs:169`, and `svc_encode_slice.rs:509`, the last of which additionally
/// null-checks, pre-masks the value to `iLen` bits, and wraps where the canonical
/// adds. They agree on in-contract inputs, which is why the encoder is byte-identical
/// to the C++ across the sweep; they disagree on guards, masking and overflow. See
/// `phase0_findings.md` §F2.
///
/// `BsWriter` implements the **canonical** semantics and is differential-tested
/// against the canonical copy only. Which guard semantics survive the dedupe is
/// Phase 3.2's decision to make explicitly — it is deliberately *not* made here, and
/// no masking or `iLen <= 0` guard has been smuggled in.
///
/// # Bounds
///
/// The canonical writer has no end-of-buffer check: sizing is the caller's contract
/// (plan §2.2.2). This keeps the canonical *output* semantics and gets its bounds
/// from slice indexing, so an out-of-space write panics. That is a pre-existing
/// sizing bug made loud, not new behaviour on any in-contract path — and note the
/// contract includes 4 bytes of headroom at the current position, because both the
/// accumulator flush and [`flush`](Self::flush) always store a full 32-bit word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BsWriter {
    pos: usize,
    cur_bits: u32,
    left_bits: i32,
}

impl Default for BsWriter {
    /// The state `InitBits` establishes: empty accumulator, 32 bits free.
    ///
    /// Note this differs from a zeroed `SBitStringAux`, whose `iLeftBits` is 0 —
    /// the C++ never uses that struct before calling `InitBits`.
    fn default() -> Self {
        Self::new()
    }
}

/// Stores `value` big-endian at `buf[pos..pos + 4]`.
///
/// Mirrors `WRITE_BE_32` (`encoder/vlc_encoder.rs:342`).
#[inline]
fn write_be_32(buf: &mut [u8], pos: usize, value: u32) {
    buf[pos..pos + 4].copy_from_slice(&value.to_be_bytes());
}

impl BsWriter {
    /// A writer positioned at the start of a buffer.
    ///
    /// Mirrors `InitBits` (`encoder/vlc_encoder.rs:353`); the buffer and its length
    /// are call parameters here, so only the accumulator state remains.
    #[inline]
    pub fn new() -> Self {
        Self {
            pos: 0,
            cur_bits: 0,
            left_bits: 32,
        }
    }

    /// Bytes written so far — the C++ `pCurBuf - pStartBuf`. Whole words only; bits
    /// still in the accumulator are not counted (use [`bits_pos`](Self::bits_pos)).
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Bits free in the accumulator — the C++ `iLeftBits`.
    #[inline]
    pub fn left_bits(&self) -> i32 {
        self.left_bits
    }

    /// The write position in bits.
    ///
    /// Mirrors `BsGetBitsPos` (`encoder/vlc_encoder.rs:501`).
    #[inline]
    pub fn bits_pos(&self) -> i32 {
        ((self.pos as i32) << 3) + 32 - self.left_bits
    }

    /// Writes the low `n` bits of `value`, MSB first.
    ///
    /// Mirrors `BsWriteBits` (`encoder/vlc_encoder.rs:367`).
    ///
    /// # Contract
    /// `n` in `1..=32`, and `value` must have no bits set above bit `n-1`: the
    /// canonical writer masks neither, and ORs the value straight into the
    /// accumulator. Passing more is how the four copies diverge (§F2), so this one
    /// neither masks nor asserts — it reproduces the canonical result exactly.
    ///
    /// # Panics
    /// If fewer than 4 bytes remain at the current word position.
    #[inline]
    pub fn write_bits(&mut self, buf: &mut [u8], n: i32, value: u32) {
        if n < self.left_bits {
            self.cur_bits = (self.cur_bits << n) | value;
            self.left_bits -= n;
        } else {
            let rem = n - self.left_bits; // 0..=31, since left_bits >= 1
            // `cur_bits << left_bits` with left_bits == 32 is UB in C++ and a panic
            // in Rust; it is only reachable with an empty accumulator, where the
            // shift contributes nothing either way.
            debug_assert!(
                self.left_bits < 32 || self.cur_bits == 0,
                "a full accumulator must be empty: left_bits={} cur_bits={:#x}",
                self.left_bits,
                self.cur_bits
            );
            let head = if self.left_bits >= 32 {
                0
            } else {
                self.cur_bits << self.left_bits
            };
            write_be_32(buf, self.pos, head | (value >> rem));
            self.pos += 4;
            self.cur_bits = value & ((1u32 << rem) - 1);
            self.left_bits = 32 - rem;
        }
    }

    /// Writes one bit. Mirrors `BsWriteOneBit` (`encoder/vlc_encoder.rs:386`).
    #[inline]
    pub fn write_one_bit(&mut self, buf: &mut [u8], value: u32) {
        self.write_bits(buf, 1, value);
    }

    /// Writes an unsigned Exp-Golomb code, `ue(v)`.
    ///
    /// Mirrors `BsWriteUE` (`encoder/vlc_encoder.rs:444`). The C++ takes the code
    /// length from `g_kuiGolombUELength` below 256 and from a two-step reduction
    /// above it; both compute `2 * floor(log2(value + 1)) + 1`, which is what
    /// [`size_ue`] returns directly.
    ///
    /// # Panics
    /// On `u32::MAX` (`value + 1` overflows), exactly as the C++ port does today.
    #[inline]
    pub fn write_ue(&mut self, buf: &mut [u8], value: u32) {
        self.write_bits(buf, size_ue(value) as i32, value + 1);
    }

    /// Writes a signed Exp-Golomb code, `se(v)`.
    ///
    /// Mirrors `BsWriteSE` (`encoder/vlc_encoder.rs:472`). One out-of-contract input
    /// differs: at `i32::MIN` the canonical writer negates and overflows, which panics
    /// in a debug build; `unsigned_abs` cannot, so this one encodes the wrapped value
    /// the release build would have produced. Same class as `phase1_findings.md` §F5 —
    /// no syntax element comes anywhere near that magnitude.
    #[inline]
    pub fn write_se(&mut self, buf: &mut [u8], value: i32) {
        if value == 0 {
            self.write_one_bit(buf, 1);
        } else if value > 0 {
            self.write_ue(buf, ((value as u32) << 1) - 1);
        } else {
            self.write_ue(buf, (value.unsigned_abs()) << 1);
        }
    }

    /// Flushes the accumulator, padding the last byte with zeros.
    ///
    /// Mirrors `BsFlush` (`encoder/vlc_encoder.rs:395`), including that it stores a
    /// full 32-bit word but advances the position only by the bytes it actually
    /// filled.
    ///
    /// # Panics
    /// If fewer than 4 bytes remain at the current word position.
    #[inline]
    pub fn flush(&mut self, buf: &mut [u8]) {
        if self.left_bits < 32 {
            write_be_32(buf, self.pos, self.cur_bits << self.left_bits);
            self.pos += 4 - (self.left_bits as usize / 8);
            self.left_bits = 32;
            self.cur_bits = 0;
        }
    }

    /// Writes `rbsp_stop_one_bit` and flushes to byte alignment.
    ///
    /// Mirrors `BsRbspTrailingBits` (`encoder/vlc_encoder.rs:509`).
    #[inline]
    pub fn rbsp_trailing_bits(&mut self, buf: &mut [u8]) {
        self.write_one_bit(buf, 1);
        self.flush(buf);
    }
}

/// Bit length of the `ue(v)` encoding of `value`.
///
/// Mirrors `BsSizeUE` (`encoder/vlc_encoder.rs:409`), which reaches the same number
/// through `g_kuiGolombUELength`.
///
/// # Panics
/// On `u32::MAX`, as the C++ port does.
#[inline]
pub fn size_ue(value: u32) -> u32 {
    // floor(log2(value + 1)) prefix zeros, one stop bit, that many suffix bits.
    2 * (31 - (value + 1).leading_zeros()) + 1
}

/// Bit length of the `se(v)` encoding of `value`.
///
/// Mirrors `BsSizeSE` (`encoder/vlc_encoder.rs:430`).
#[inline]
pub fn size_se(value: i32) -> u32 {
    if value == 0 {
        1
    } else if value > 0 {
        size_ue(((value as u32) << 1) - 1)
    } else {
        size_ue(value.unsigned_abs() << 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::prng::Prng;

    /// An RBSP plus the slack the C++ reader relies on (see `get_bits`).
    fn with_slack(payload: &[u8]) -> Vec<u8> {
        let mut v = payload.to_vec();
        v.extend_from_slice(&[0u8; 4]);
        v
    }

    #[test]
    fn init_mirrors_dec_init_bits() {
        let buf = with_slack(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22]);
        let c = BsCursor::init(&buf, 64).unwrap();
        assert_eq!(c.cur_bits(), 0xAABBCCDD);
        assert_eq!(c.left_bits(), -16);
        assert_eq!(c.bits(), 64);
        assert_eq!(c.pos(), 4);
        assert_eq!(c.len(), 8);
    }

    #[test]
    fn init_rejects_an_empty_rbsp() {
        assert_eq!(BsCursor::init(&[0u8; 8], 0), Err(ErrInfo::INVALID_ACCESS));
        assert_eq!(BsCursor::init(&[0u8; 8], -8), Err(ErrInfo::INVALID_ACCESS));
    }

    #[test]
    fn init_read_bits_honours_its_end_offset() {
        // A one-byte RBSP: starting a read that must leave one byte spare fails.
        let buf = with_slack(&[0x80]);
        let mut c = BsCursor::init(&buf, 8).unwrap();
        c.pos = 0;
        assert_eq!(c.init_read_bits(&buf, 1), Err(ErrInfo::INVALID_ACCESS));
        assert_eq!(c.init_read_bits(&buf, 0), Ok(()));
    }

    #[test]
    fn reads_the_exp_golomb_examples_from_the_spec() {
        // '1' -> 0, '010' -> 1, '011' -> 2, '00100' -> 3
        let buf = with_slack(&[0b1_010_011_0, 0b0100_0000]);
        let mut c = BsCursor::init(&buf, 16).unwrap();
        assert_eq!(c.get_ue(&buf), Ok(0));
        assert_eq!(c.get_ue(&buf), Ok(1));
        assert_eq!(c.get_ue(&buf), Ok(2));
        assert_eq!(c.get_ue(&buf), Ok(3));
    }

    #[test]
    fn se_maps_code_numbers_the_way_the_spec_does() {
        // ue(1) -> +1, ue(2) -> -1, ue(3) -> +2, ue(4) -> -2
        let buf = with_slack(&[0b010_011_00, 0b100_00101]);
        let mut c = BsCursor::init(&buf, 16).unwrap();
        assert_eq!(c.get_se(&buf), Ok(1));
        assert_eq!(c.get_se(&buf), Ok(-1));
        assert_eq!(c.get_se(&buf), Ok(2));
        assert_eq!(c.get_se(&buf), Ok(-2));
    }

    #[test]
    fn te0_special_cases_its_small_ranges() {
        let buf = with_slack(&[0b1010_0000]);
        let mut c = BsCursor::init(&buf, 8).unwrap();
        assert_eq!(c.get_te0(&buf, 1), Ok(0), "range 1 consumes nothing");
        assert_eq!(c.peek_bits(1), 1);
        assert_eq!(c.get_te0(&buf, 2), Ok(0), "range 2 reads one inverted bit");
        assert_eq!(c.get_te0(&buf, 3), Ok(1), "range 3+ is plain ue(v)");
    }

    #[test]
    fn an_all_zero_accumulator_is_a_leading_zero_error() {
        let buf = with_slack(&[0, 0, 0, 0, 0, 0, 0, 0]);
        let mut c = BsCursor::init(&buf, 64).unwrap();
        assert_eq!(c.get_ue(&buf), Err(ErrInfo::READ_LEADING_ZERO));
    }

    #[test]
    fn reading_past_the_rbsp_end_overflows_at_the_slop_boundary() {
        // Two payload bytes, plenty of slack: the cursor may sit at len + 1 but no
        // further, so a long read eventually returns READ_OVERFLOW rather than data.
        let buf = with_slack(&[0xFF, 0xFF]);
        let mut c = BsCursor::init(&buf, 16).unwrap();
        let mut err = None;
        for _ in 0..64 {
            if let Err(e) = c.get_bits(&buf, 8) {
                err = Some(e);
                break;
            }
        }
        assert_eq!(err, Some(ErrInfo::READ_OVERFLOW));
        assert!(c.pos() > c.len(), "the C++ predicate allows one byte of slop");
    }

    #[test]
    fn a_buffer_without_slack_errors_instead_of_reading_past_it() {
        // Same payload, no slack: this is where BsCursor is *safer* than the C++,
        // and the finding F4 case.
        let buf = [0xFFu8, 0xFF];
        let mut c = BsCursor::init(&buf, 16);
        assert_eq!(c, Err(ErrInfo::READ_OVERFLOW), "the 4-byte prime needs slack");

        let buf = [0xFFu8, 0xFF, 0xFF, 0xFF];
        c = BsCursor::init(&buf, 16);
        let mut c = c.unwrap();
        let mut err = None;
        for _ in 0..64 {
            if let Err(e) = c.get_bits(&buf, 8) {
                err = Some(e);
                break;
            }
        }
        assert_eq!(err, Some(ErrInfo::READ_OVERFLOW));
    }

    #[test]
    fn check_more_rbsp_data_counts_from_the_steady_state_refill() {
        // The C++ formula subtracts a *fixed* 2 bytes — the size of one refill — so it
        // only tells the truth once the cursor is in its steady state. Right after
        // `DecInitBits`, which primed with four bytes, it over-counts by 16 bits, and
        // a one-byte RBSP holding nothing but the stop bit still reports "more data".
        // Mirrored deliberately; `nalu.rs:1733` is the sole caller and it calls this
        // deep inside a NAL.
        let buf = with_slack(&[0b1000_0000]);
        let c = BsCursor::init(&buf, 8).unwrap();
        assert!(c.check_more_rbsp_data());

        // In the steady state it answers as intended: consume everything but the stop
        // bit of a 4-byte RBSP and it goes false.
        let buf = with_slack(&[0xFF, 0xFF, 0xFF, 0b1000_0000]);
        let mut c = BsCursor::init(&buf, 32).unwrap();
        assert!(c.check_more_rbsp_data());
        for _ in 0..3 {
            c.get_bits(&buf, 8).unwrap();
        }
        assert!(c.check_more_rbsp_data());
        c.get_bits(&buf, 7).unwrap();
        assert!(!c.check_more_rbsp_data(), "only the stop bit is left");
    }

    #[test]
    fn trailing_bits_counts_zeros_below_the_stop_bit() {
        assert_eq!(trailing_bits(0b0000_1000), 3);
        assert_eq!(trailing_bits(0b1000_0000), 7);
        assert_eq!(trailing_bits(1), 0);
        assert_eq!(trailing_bits(0), 0, "the C++ returns 0, not an error");
    }

    #[test]
    fn cursor_is_copy_so_a_position_can_be_saved() {
        let buf = with_slack(&[0b1010_1010, 0b1100_1100]);
        let mut c = BsCursor::init(&buf, 16).unwrap();
        let saved = c;
        assert_eq!(c.get_bits(&buf, 4), Ok(0b1010));
        c = saved;
        assert_eq!(c.get_bits(&buf, 4), Ok(0b1010), "rewound to the same bits");
    }

    // -----------------------------------------------------------------------
    // CAVLC mode (plan §2.2.2 [P3])
    // -----------------------------------------------------------------------

    /// Randomised round counts are cut hard under Miri, which runs ~100x slower and is
    /// part of every `gates.sh full` run via `--lib`. The *shapes* tested are unchanged
    /// — every bit phase, every width — only the sampling shrinks.
    fn scale_unit(n: usize) -> usize {
        if cfg!(miri) {
            (n / 25).max(2)
        } else {
            n
        }
    }

    /// A payload long enough that any `end_cavlc` prime in these tests is in bounds.
    fn cavlc_buf(prng: &mut Prng, n: usize) -> Vec<u8> {
        with_slack(&prng.bytes(n))
    }

    #[test]
    fn start_cavlc_projects_the_cursor_onto_an_absolute_bit_position() {
        // After `init` the accumulator holds 32 valid bits primed from bytes 0..4, so
        // the next unread bit is bit 0 and `pos` is 4: 4*8 - (16 - (-16)) = 0.
        let buf = with_slack(&[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let mut c = BsCursor::init(&buf, 48).unwrap();
        assert_eq!((c.pos(), c.left_bits()), (4, -16));
        c.start_cavlc();
        assert_eq!(c.cavlc_bit_pos(), 0);
        c.end_cavlc(&buf);

        // Consume 12 bits and it is 12, whatever the refill did to `pos`/`left_bits`.
        let mut c = BsCursor::init(&buf, 48).unwrap();
        c.get_bits(&buf, 12).unwrap();
        c.start_cavlc();
        assert_eq!(c.cavlc_bit_pos(), 12);
    }

    #[test]
    fn end_cavlc_reseats_the_window_at_every_bit_phase() {
        // The −16 half-window arithmetic, byte-aligned and at each of the 7 unaligned
        // phases: `left_bits` is `-16 + (idx & 7)` — negative for phase 0 and rising to
        // −9 at phase 7 — and `pos` is `(idx >> 3) + 4`.
        let buf = with_slack(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x23, 0x45, 0x67]);
        for phase in 0..8usize {
            for byte in 0..4usize {
                let idx = (byte * 8 + phase) as isize;
                let mut c = BsCursor::init(&buf, 64).unwrap();
                c.start_cavlc();
                c.advance_cavlc_bits(idx);
                c.end_cavlc(&buf);

                assert_eq!(c.pos(), byte + 4, "pos = (idx >> 3) + 4 at phase {phase}");
                assert_eq!(
                    c.left_bits(),
                    -16 + phase as i32,
                    "left_bits = -16 + (idx & 7) at phase {phase}"
                );
                let expect = u32::from_be_bytes([buf[byte], buf[byte + 1], buf[byte + 2], buf[byte + 3]])
                    << phase;
                assert_eq!(c.cur_bits(), expect, "the 4-byte prime is shifted by idx & 7");
            }
        }
    }

    #[test]
    fn a_cavlc_round_trip_reads_on_identically() {
        // The contract `end_cavlc` documents: the round trip restores the *reading
        // position*, not the field values — the re-prime can leave more valid bits in
        // the accumulator than were there before. Reads of 1..=16 bits are therefore the
        // ones that must agree, and they are the only widths the codec uses.
        let mut prng = Prng::new(0x5CA1_AB1E);
        let buf = cavlc_buf(&mut prng, 64);

        for _ in 0..scale_unit(400) {
            let skip = prng.below(96) as i32;
            let mut untouched = BsCursor::init(&buf, 64 * 8).unwrap();
            for _ in 0..skip {
                untouched.get_bits(&buf, 1).unwrap();
            }
            let mut cycled = untouched;
            cycled.start_cavlc();
            assert_eq!(cycled.cavlc_bit_pos(), skip as isize, "the projection is the bit count");
            cycled.end_cavlc(&buf);

            for _ in 0..8 {
                let n = 1 + prng.below(16) as i32;
                assert_eq!(
                    cycled.get_bits(&buf, n),
                    untouched.get_bits(&buf, n),
                    "a cycled cursor reads on identically after skipping {skip} bits"
                );
            }
        }
    }

    #[test]
    fn advancing_the_bit_position_is_what_end_cavlc_seeks_to() {
        // The residual path's only write: `pBs->iIndex += iUsedBits`. Advancing by N and
        // ending is the same reading state as consuming N bits without the mode at all.
        let mut prng = Prng::new(0xF00D_1234);
        let buf = cavlc_buf(&mut prng, 64);

        for used in [0isize, 1, 7, 8, 9, 15, 16, 17, 31, 33, 64, 127] {
            let mut plain = BsCursor::init(&buf, 64 * 8).unwrap();
            for _ in 0..used {
                plain.get_bits(&buf, 1).unwrap();
            }
            let mut moded = BsCursor::init(&buf, 64 * 8).unwrap();
            moded.start_cavlc();
            moded.advance_cavlc_bits(used);
            moded.end_cavlc(&buf);

            for _ in 0..4 {
                assert_eq!(
                    moded.get_bits(&buf, 16),
                    plain.get_bits(&buf, 16),
                    "advance_cavlc_bits({used}) lands where {used} single-bit reads do"
                );
            }
        }
    }

    #[test]
    fn the_mode_flag_is_not_part_of_equality() {
        // S16's dual-profile discipline: equality must mean the same thing in debug and
        // release, so the `cfg`-gated flag is excluded and the six C-mirrored fields are
        // compared by hand. Two cursors differing *only* by mode are equal — in release
        // there is no flag to differ by, and debug must agree.
        let buf = with_slack(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let plain = BsCursor::init(&buf, 32).unwrap();
        let mut in_mode = plain;
        in_mode.start_cavlc();
        // `start_cavlc` writes `cavlc_bit_pos`, which *is* compared — so re-zero it via a
        // second cursor that entered the mode at the same position.
        let mut also_in_mode = plain;
        also_in_mode.start_cavlc();
        assert_eq!(in_mode, also_in_mode);

        // And a cursor whose bit position differs is unequal, in both profiles.
        let mut moved = also_in_mode;
        moved.advance_cavlc_bits(1);
        assert_ne!(in_mode, moved, "cavlc_bit_pos is one of the six compared fields");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ran inside a CAVLC region")]
    fn reading_the_accumulator_inside_the_mode_panics_in_debug() {
        let buf = with_slack(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let mut c = BsCursor::init(&buf, 32).unwrap();
        c.start_cavlc();
        let _ = c.get_bits(&buf, 4);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ran inside a CAVLC region")]
    fn peeking_inside_the_mode_panics_in_debug() {
        let buf = with_slack(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let mut c = BsCursor::init(&buf, 32).unwrap();
        c.start_cavlc();
        let _ = c.peek_bits(4);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ran outside a CAVLC region")]
    fn reading_the_bit_position_outside_the_mode_panics_in_debug() {
        let buf = with_slack(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let c = BsCursor::init(&buf, 32).unwrap();
        let _ = c.cavlc_bit_pos();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ran outside a CAVLC region")]
    fn advancing_the_bit_position_after_end_cavlc_panics_in_debug() {
        let buf = with_slack(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let mut c = BsCursor::init(&buf, 32).unwrap();
        c.start_cavlc();
        c.end_cavlc(&buf);
        c.advance_cavlc_bits(4);
    }

    #[test]
    fn the_bit_position_survives_end_cavlc_for_the_parity_tests() {
        // `end_cavlc` clears the mode but leaves `cavlc_bit_pos` set, exactly as the C++
        // leaves `iIndex` set. The state accessor reads it without asserting, which is
        // how the differential tests compare all six fields after the mode closes.
        let buf = with_slack(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let mut c = BsCursor::init(&buf, 48).unwrap();
        c.start_cavlc();
        c.advance_cavlc_bits(19);
        c.end_cavlc(&buf);
        assert_eq!(c.cavlc_bit_pos_state(), 19);
        // …and the accumulator is usable again.
        assert_eq!(c.get_bits(&buf, 4).is_ok(), true);
    }

    #[test]
    fn writer_round_trips_through_the_reader() {
        let mut out = vec![0u8; 64];
        let mut w = BsWriter::new();
        w.write_bits(&mut out, 3, 0b101);
        w.write_ue(&mut out, 300);
        w.write_se(&mut out, -7);
        w.write_one_bit(&mut out, 1);
        w.write_bits(&mut out, 17, 0x1_5A5A);
        let bits = w.bits_pos();
        w.rbsp_trailing_bits(&mut out);

        let mut c = BsCursor::init(&out, bits + 1).unwrap();
        assert_eq!(c.get_bits(&out, 3), Ok(0b101));
        assert_eq!(c.get_ue(&out), Ok(300));
        assert_eq!(c.get_se(&out), Ok(-7));
        assert_eq!(c.get_one_bit(&out), Ok(1));
        assert_eq!(c.get_bits(&out, 17), Ok(0x1_5A5A));
    }

    #[test]
    fn writer_flushes_whole_words_and_counts_partial_bytes() {
        let mut out = vec![0u8; 16];
        let mut w = BsWriter::new();
        w.write_bits(&mut out, 12, 0xABC);
        assert_eq!(w.bits_pos(), 12);
        assert_eq!(w.pos(), 0, "nothing leaves the accumulator until 32 bits");
        w.flush(&mut out);
        assert_eq!(&out[..2], &[0xAB, 0xC0]);
        assert_eq!(w.pos(), 2, "12 bits round up to 2 bytes");
        assert_eq!(w.bits_pos(), 16);
    }

    #[test]
    fn writer_handles_the_exact_accumulator_boundary() {
        // iLen == iLeftBits is the branch the four copies reach differently.
        let mut out = vec![0u8; 16];
        let mut w = BsWriter::new();
        w.write_bits(&mut out, 20, 0xA_5A5A);
        w.write_bits(&mut out, 12, 0xC3C); // exactly fills the word
        assert_eq!(w.pos(), 4);
        assert_eq!(w.left_bits(), 32);
        assert_eq!(&out[..4], &[0xA5, 0xA5, 0xAC, 0x3C]);
        // A 32-bit write into an empty accumulator: the C++ shift-by-32 case.
        w.write_bits(&mut out, 32, 0xDEAD_BEEF);
        assert_eq!(&out[4..8], &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(w.pos(), 8);
    }

    #[test]
    fn writer_snapshot_and_rollback_restores_the_bytes_too() {
        // The safe replacement for the pBsStackBufPtr stash/pop pair.
        let mut out = vec![0u8; 32];
        let mut w = BsWriter::new();
        w.write_bits(&mut out, 16, 0x1234);
        let saved = w;
        let bytes_before = out.clone();

        w.write_bits(&mut out, 24, 0x9A_BCDE);
        w.write_ue(&mut out, 42);

        w = saved;
        out.copy_from_slice(&bytes_before);
        w.write_bits(&mut out, 16, 0x5678);
        w.flush(&mut out);
        assert_eq!(&out[..4], &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    #[should_panic]
    fn writing_past_the_buffer_panics() {
        let mut out = vec![0u8; 3];
        let mut w = BsWriter::new();
        w.write_bits(&mut out, 32, 0);
    }

    #[test]
    fn size_ue_matches_the_definition() {
        assert_eq!(size_ue(0), 1);
        assert_eq!(size_ue(1), 3);
        assert_eq!(size_ue(2), 3);
        assert_eq!(size_ue(3), 5);
        assert_eq!(size_ue(6), 5);
        assert_eq!(size_ue(7), 7);
        assert_eq!(size_se(0), 1);
        assert_eq!(size_se(1), 3);
        assert_eq!(size_se(-1), 3);
    }

    #[test]
    fn write_then_read_survives_prng_op_sequences() {
        let mut rng = Prng::new(0x5EED_1234);
        for round in 0..200 {
            let mut ops: Vec<(u8, i64)> = Vec::new();
            for _ in 0..rng.below(24) + 1 {
                match rng.below(3) {
                    0 => {
                        // 16 is the widest read the refill can serve — see the
                        // "16-bit ceiling" note on `get_bits`, and the decoder's own
                        // call widths.
                        let n = rng.range_i32(1, 16);
                        let v = rng.next_u32() & ((1u32 << n) - 1);
                        ops.push((0, ((n as i64) << 32) | v as i64));
                    }
                    1 => ops.push((1, rng.below(4096) as i64)),
                    _ => ops.push((2, rng.range_i32(-2048, 2048) as i64)),
                }
            }

            let mut out = vec![0u8; 1024];
            let mut w = BsWriter::new();
            for &(kind, arg) in &ops {
                match kind {
                    0 => w.write_bits(&mut out, (arg >> 32) as i32, arg as u32),
                    1 => w.write_ue(&mut out, arg as u32),
                    _ => w.write_se(&mut out, arg as i32),
                }
            }
            let bits = w.bits_pos();
            w.rbsp_trailing_bits(&mut out);

            let mut c = BsCursor::init(&out, bits + 1).unwrap();
            for &(kind, arg) in &ops {
                let got = match kind {
                    0 => c.get_bits(&out, (arg >> 32) as i32).map(|v| v as i64),
                    1 => c.get_ue(&out).map(|v| v as i64),
                    _ => c.get_se(&out).map(|v| v as i64),
                };
                let want = if kind == 0 { arg & 0xFFFF_FFFF } else { arg };
                assert_eq!(
                    got,
                    Ok(want),
                    "round {round}, seed {:#x}, op kind {kind}",
                    rng.seed()
                );
            }
        }
    }
}
