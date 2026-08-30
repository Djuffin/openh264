//! Differential tests: `safe::bits` against the implementations the codec used
//! before Phase 3 (plan §2.2.2, taxonomy T3).
//!
//! This file lives outside `src/`, so unlike everything under `src/safe/` it may use
//! `unsafe` — it has to, because the reference implementations are raw-pointer code.
//! Every `unsafe` block here drives the old side of a comparison.
//!
//! Running this under Miri additionally checks those old implementations for UB; see
//! `rust/docs/phase1_findings.md`.
//!
//! # The reader half retired at T3.1b (plan §2, differential retirement)
//!
//! T3.1a moved the reader's *bodies* onto [`BsCursor`] behind unchanged raw
//! signatures, and this file's reader tests were retired *in place* then: they stopped
//! comparing two implementations and began proving the shim was faithful.
//! **T3.1b deleted the shim**, so there is no longer a second implementation for them
//! to compare against, and tests that compare a thing to itself are worse than no
//! tests — they read as coverage. They are deleted here, in the commit that deletes
//! what they tested, and their burden passes to:
//!
//! * `tests/malformed_stream_parity.rs` (T3.0) — 2316 golden rows recorded against the
//!   *raw* reader, covering error-code parity at every truncation and slop boundary,
//!   still holding byte for byte;
//! * the 53 conformance hashes and the frame counts beside them;
//! * `src/safe/bits.rs`'s own unit tests, which keep the properties (the slop
//!   predicate, the 16-bit ceiling, the Exp-Golomb spec examples).
//!
//! That handover is why T3.0 was built before the conversion rather than after it.
//!
//! * **`GetLeadingZeroBits` and `BsGetTrailingBits`** — the table-driven originals are
//!   still in `dec_golomb.rs` and still used, so these compare real alternatives.
//! * **The CAVLC mode** (plan §2.2.2 [P3]) — against a *frozen transliteration* of
//!   `BsStartCavlc`/`BsEndCavlc`, kept below because it is the only executable
//!   statement of what parity means for that pair now that the port's copy is gone.
//! # The writer half retired at T3.4 (plan §2, differential retirement)
//!
//! T3.4 face 2 moved `vlc_encoder.rs`'s writer family onto [`BsWriter`] and deleted
//! the raw bodies, so the same rule that retired the reader half applies: with one
//! implementation left there is nothing to compare, and a test that compares a thing
//! to itself reads as coverage it does not provide. `writer_op_sequences_are_byte_
//! identical`, `writer_matches_at_the_accumulator_boundary`,
//! `writer_snapshot_and_rollback_matches_the_cursor_stash`, `rbsp_trailing_bits_
//! matches` and `writer_and_the_32_bit_word` are deleted here, in the commit that
//! deletes what they tested. Their burden passes to:
//!
//! * the **encoder sweeps**, 341 configurations in both build profiles, which are
//!   byte-exactness against the C++ encoder itself — a stronger referee for the
//!   writer than any in-tree comparison, and the one F2 named;
//! * `src/safe/bits.rs`'s own unit tests, which keep the properties: the
//!   accumulator boundary, the whole-word flush, the snapshot/rollback round trip,
//!   `te(v)`, `align`'s one-bit padding, and the out-of-space panic;
//! * `written_streams_read_back_through_the_old_reader` below, which still closes
//!   the writer-to-reader loop.
//!
//! **F5 closed with them** (`phase1_findings.md`). The finding was that the
//! canonical writer's `uiCurBits << iLeftBits` panics in a debug build when
//! `iLeftBits == 32`, and its own "who fixes it" line named this commit: *"Phase
//! 3.2, in the commit that collapses the four writer copies (F2) — the fix is the
//! same one `BsWriter` already carries."* Nothing was repaired; the expression was
//! deleted along with the body holding it, and the replacement already folded the
//! shift away. The path was unreachable (no `BsWriteBits` width in `src/encoder/`
//! reaches 32), which is why the sweeps cannot tell the difference and why this is
//! a deletion rather than a behaviour change.
//!
//! # What is still a genuine two-implementation comparison
//!
//! * **`BsSizeUE`/`BsSizeSE`** — the table-driven originals survive in
//!   `vlc_encoder.rs` because the mode-decision cost functions want a code length
//!   without writing anything, so `exp_golomb_sizes_match_the_table_driven_versions`
//!   still compares real alternatives.

#![allow(non_snake_case)]

mod common;

use common::prng::Prng;

use openh264_rs::decoder::dec_golomb::{
    BsGetBits, BsGetOneBit, BsGetSe, BsGetTrailingBits, BsGetUe, GetLeadingZeroBits, ERR_NONE,
};
use openh264_rs::encoder::vlc_encoder::{BsSizeSE, BsSizeUE};
use openh264_rs::safe::bits::{size_se, size_ue, trailing_bits, BsCursor, BsWriter};

/// Sample sizes are cut hard under Miri, which runs ~100x slower and would otherwise
/// turn a phase-exit gate into an hour. The *shapes* tested are identical — every
/// bit phase, every boundary, every operation kind — only the randomised round counts
/// shrink, and the full-size run happens on every `cargo test`.
fn scale(n: usize) -> usize {
    if cfg!(miri) {
        (n / 25).max(2)
    } else {
        n
    }
}

/// The RBSP plus the slack the C++ reader relies on. See `phase1_findings.md` §F4:
/// `dump_bits_aux` may sit one byte past the logical end and read two bytes there, and
/// `BsEndCavlc` primes four bytes at an arbitrary byte offset, so the *allocation* must
/// extend past the declared RBSP for the old code to be in bounds at all. Eight bytes,
/// not three, because the CAVLC closes below deliberately land at and past the declared
/// end.
fn rbsp_with_slack(payload: &[u8]) -> Vec<u8> {
    let mut v = payload.to_vec();
    v.extend_from_slice(&[0u8; 8]);
    v
}

#[test]
fn leading_zero_bits_matches_the_table() {
    // `GetLeadingZeroBits` walks `g_kuiLeadingZeroTable`; the safe reader uses
    // `leading_zeros()`. Same function, proven over every byte-boundary case plus a
    // PRNG sample. (`leading_zero_bits` is private, so drive it through `get_ue`'s
    // observable behaviour on a one-shot cursor... or directly compare the peeks.)
    let mut rng = Prng::new(0x8B17_0006);
    let mut values: Vec<u32> = vec![0, 1, 2, 3, 0x80, 0xFF, 0x100, 0x8000, 0xFFFF, 0x80_0000];
    values.extend((0..32).map(|i| 1u32 << i));
    values.extend((0..32).map(|i| u32::MAX >> i));
    values.extend((0..scale(2000)).map(|_| rng.next_u32()));

    for v in values {
        let old = GetLeadingZeroBits(v);
        let new = if v == 0 { -1 } else { v.leading_zeros() as i32 };
        assert_eq!(old, new, "GetLeadingZeroBits({v:#010x})");
    }
}

#[test]
fn trailing_bits_matches_for_every_byte() {
    for b in 0..=255u8 {
        let old = BsGetTrailingBits(&b);
        assert_eq!(old, trailing_bits(b), "byte {b:#04x}");
    }
}

// ===========================================================================
// CAVLC mode vs. a frozen BsStartCavlc/BsEndCavlc  (plan §2.2.2 [P3])
// ===========================================================================

/// The reader state the C++ pair operated on, as a plain struct.
///
/// This is deliberately **not** `SBitStringAux`: the port's copy of that struct is on
/// its way out (T3.3/T3.4), and pinning this comparison to it would make the reference
/// implementation drift with the refactor it is supposed to be judging. The five fields
/// below are the ones `BsStartCavlc`/`BsEndCavlc` read or write, expressed as offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RawBs {
    pos: usize,
    cur_bits: u32,
    left_bits: i32,
    len: usize,
    bits: i32,
    index: isize,
}

/// A frozen transliteration of `BsStartCavlc`
/// (`codec/decoder/core/src/parse_mb_syn_cavlc.cpp`; the port's copy lived at
/// `parse_mb_syn_cavlc.rs:2229` until T3.1b deleted it).
///
/// **Do not "fix" this to match the safe side** — it is the reference, and its whole
/// value is that it was written from the C++ and then left alone (S6).
fn raw_bs_start_cavlc(bs: &mut RawBs) {
    bs.index = ((bs.pos as isize) << 3) - (16 - bs.left_bits as isize);
}

/// A frozen transliteration of `BsEndCavlc`, same provenance as
/// [`raw_bs_start_cavlc`]. The 4-byte load is the C++'s unconditional one; the caller
/// supplies the slack that makes it legal (F4).
fn raw_bs_end_cavlc(bs: &mut RawBs, buf: &[u8]) {
    bs.pos = (bs.index >> 3) as usize;
    let b0 = buf[bs.pos] as u32;
    let b1 = buf[bs.pos + 1] as u32;
    let b2 = buf[bs.pos + 2] as u32;
    let b3 = buf[bs.pos + 3] as u32;
    let uiCache32Bit = (b0 << 24) | (b1 << 16) | (b2 << 8) | b3;
    bs.cur_bits = uiCache32Bit << ((bs.index & 0x07) as u32);
    bs.pos += 4;
    bs.left_bits = -16 + ((bs.index & 0x07) as i32);
}

fn raw_of(c: &BsCursor) -> RawBs {
    RawBs {
        pos: c.pos(),
        cur_bits: c.cur_bits(),
        left_bits: c.left_bits(),
        len: c.len(),
        bits: c.bits(),
        // Not `cavlc_bit_pos()`: that one asserts the mode is live, and half these
        // comparisons happen after `end_cavlc` has closed it. See the accessor's docs.
        index: c.cavlc_bit_pos_state(),
    }
}

/// Drives both sides through `start` → *n* bits consumed → `end`, from whatever state
/// they are already in, and compares all six fields at both steps.
///
/// `used_bits` is what the residual path reports via `pBs->iIndex += iUsedBits`
/// (`parse_mb_syn_cavlc.rs`, `WelsResidualBlockCavlc`) — the only write it makes.
fn assert_cavlc_cycle_matches(
    raw: &mut RawBs,
    c: &mut BsCursor,
    buf: &[u8],
    used_bits: isize,
    label: &str,
) {
    raw_bs_start_cavlc(raw);
    c.start_cavlc();
    assert_eq!(*raw, raw_of(c), "{label}: state after BsStartCavlc");
    assert_eq!(
        c.cavlc_bit_pos(),
        raw.index,
        "{label}: the guarded accessor reads iIndex"
    );

    raw.index += used_bits;
    c.advance_cavlc_bits(used_bits);

    raw_bs_end_cavlc(raw, buf);
    c.end_cavlc(buf);
    assert_eq!(*raw, raw_of(c), "{label}: state after BsEndCavlc");
}

#[test]
fn cavlc_mode_matches_the_raw_pair_from_prng_cursor_states() {
    // Random *cursor states*, not just random buffers: the pair's arithmetic is a
    // function of (pos, left_bits), so the interesting axis is how many bits have been
    // consumed before the mode opens. Reads of random widths walk `left_bits` through
    // its whole -16..=0 range and `pos` through every byte phase.
    let mut rng = Prng::new(0x8B17_0010);

    for round in 0..scale(200) {
        let payload_len = 48 + rng.below(48) as usize;
        let payload = rng.bytes(payload_len);
        let buf = rbsp_with_slack(&payload);
        let size_bits = payload.len() as i32 * 8;

        let mut c = BsCursor::init(&buf, size_bits).unwrap();
        let mut raw = raw_of(&c);

        for step in 0..scale(24).max(4) {
            let n = 1 + rng.below(16) as i32;
            let mut code = 0u32;
            if BsGetBits(&buf, &mut c, n, &mut code) != 0 {
                break;
            }
            raw = raw_of(&c);

            // `iUsedBits` in the real residual path is a symbol count, tens of bits at
            // most; 0 is legal (the `uiTotalCoeff == 0` early return).
            //
            // Bounded so the close point's 4-byte prime stays inside the allocation.
            // Past that the *raw* pair reads out of bounds — F4's pre-existing
            // condition, which this test must not manufacture: the safe side would
            // panic on the slice index (correctly, per plan §2.2.2) and the comparison
            // would be against UB rather than against behaviour.
            let bit_pos = (c.pos() as isize) * 8 - (16 - c.left_bits() as isize);
            let headroom_bits = ((buf.len() - 4) as isize * 8) - bit_pos;
            if headroom_bits < 0 {
                break;
            }
            let used = rng.below(48.min(headroom_bits as u32 + 1)) as isize;
            assert_cavlc_cycle_matches(
                &mut raw,
                &mut c,
                &buf,
                used,
                &format!("round {round} step {step} used {used}"),
            );
        }
    }
}

#[test]
fn cavlc_mode_matches_at_every_bit_phase() {
    // Exhaustive over the axis the arithmetic actually turns on — `idx & 7` — rather
    // than sampled (S10: sweep the selector, don't randomise it). Every starting phase
    // crossed with every `iUsedBits` phase, so both the `>> 3` reseat and the
    // `-16 + (idx & 7)` bias are exercised at all 64 combinations.
    let payload: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(37).wrapping_add(11)).collect();
    let buf = rbsp_with_slack(&payload);
    let size_bits = payload.len() as i32 * 8;

    for skip in 0..8i32 {
        for used in 0..8isize {
            let mut c = BsCursor::init(&buf, size_bits).unwrap();
            if skip > 0 {
                let mut code = 0u32;
                assert_eq!(BsGetBits(&buf, &mut c, skip, &mut code), 0);
            }
            let mut raw = raw_of(&c);
            assert_cavlc_cycle_matches(&mut raw, &mut c, &buf, used, &format!("skip {skip} used {used}"));

            // …and the cursor reads on from where the raw pair left it, which is what
            // the mode is for. 16 bits is the widest read the codec makes.
            let mut plain = BsCursor::init(&buf, size_bits).unwrap();
            let mut code = 0u32;
            if skip > 0 {
                assert_eq!(BsGetBits(&buf, &mut plain, skip, &mut code), 0);
            }
            for _ in 0..used {
                assert_eq!(BsGetBits(&buf, &mut plain, 1, &mut code), 0);
            }
            for _ in 0..4 {
                let (mut a, mut b) = (0u32, 0u32);
                let ra = BsGetBits(&buf, &mut c, 16, &mut a);
                let rb = BsGetBits(&buf, &mut plain, 16, &mut b);
                assert_eq!((ra, a), (rb, b), "reads after the cycle, skip {skip} used {used}");
            }
        }
    }
}

#[test]
fn cavlc_mode_matches_where_the_prime_leans_on_the_slop() {
    // `BsEndCavlc` loads 4 bytes at `iIndex >> 3` with no bounds test of its own — the
    // same `READER_SLOP` regime as every other prime in the family (F4, plan §2.2.2
    // [P3]). Here the mode closes at and past the *declared* RBSP end, so the load
    // reaches into the slack.
    //
    // S12/F10 sizing: the raw side gets the real slack (`rbsp_with_slack`'s 8 bytes),
    // because a raw implementation's footprint exceeds its read footprint and an
    // exactly-sized buffer would make Miri flag the reference rather than the port. The
    // safe side is handed the same slice, and `end_cavlc` indexes it — so a buffer short
    // of the contract panics here instead of reading past the allocation.
    let mut rng = Prng::new(0x8B17_0011);

    for declared_len in 1..=8usize {
        for pattern in 0..5 {
            let payload: Vec<u8> = match pattern {
                0 => vec![0x00; 8],
                1 => vec![0xFF; 8],
                2 => vec![0x80; 8],
                3 => vec![0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA],
                _ => rng.bytes(8),
            };
            let buf = rbsp_with_slack(&payload);
            let size_bits = declared_len as i32 * 8;

            let Ok(c0) = BsCursor::init(&buf, size_bits) else {
                continue;
            };

            for used in [0isize, 8, 16, 24, 32, 40, 48, 56] {
                let mut c = c0;
                let mut raw = raw_of(&c);
                let idx_bytes = (c.pos() as isize) - 2 + (used >> 3);
                // Only exercise closes whose 4-byte prime is inside the allocation: past
                // that the raw pair reads out of bounds, which is F4's pre-existing
                // condition and not something this test should manufacture.
                if idx_bytes < 0 || idx_bytes as usize + 4 > buf.len() {
                    continue;
                }
                assert_cavlc_cycle_matches(
                    &mut raw,
                    &mut c,
                    &buf,
                    used,
                    &format!("declared_len {declared_len} pattern {pattern} used {used}"),
                );
            }
        }
    }
}

// ===========================================================================
// What survives of the writer half  (see the module header)
// ===========================================================================

/// One operation of a randomised write sequence, sized for the writer's contract:
/// `1 <= n <= 32` and no bits set above bit `n-1`.
#[derive(Clone, Copy, Debug)]
enum WriteOp {
    Bits(i32, u32),
    OneBit(u32),
    Ue(u32),
    Se(i32),
}

impl WriteOp {
    fn apply_new(self, w: &mut BsWriter, buf: &mut [u8]) {
        match self {
            WriteOp::Bits(n, v) => w.write_bits(buf, n, v),
            WriteOp::OneBit(v) => w.write_one_bit(buf, v),
            WriteOp::Ue(v) => w.write_ue(buf, v),
            WriteOp::Se(v) => w.write_se(buf, v),
        }
    }
}

#[test]
fn exp_golomb_sizes_match_the_table_driven_versions() {
    for v in 0..if cfg!(miri) { 2000 } else { 70000u32 } {
        assert_eq!(BsSizeUE(v), size_ue(v), "BsSizeUE({v})");
    }
    for v in if cfg!(miri) { -1000 } else { -35000i32 }..if cfg!(miri) { 1000 } else { 35000 } {
        assert_eq!(BsSizeSE(v), size_se(v), "BsSizeSE({v})");
    }
    let mut rng = Prng::new(0x77E1_0004);
    for _ in 0..scale(20000) {
        // The canonical writer computes `kuiValue + 1`, so u32::MAX is out of contract
        // for both sides.
        let v = rng.next_u32() & 0x7FFF_FFFF;
        assert_eq!(BsSizeUE(v), size_ue(v), "BsSizeUE({v})");
    }
}

#[test]
fn written_streams_read_back_through_the_old_reader() {
    // Closes the loop: new writer -> old reader, so a shared misunderstanding
    // between the two new types could not hide the error.
    let mut rng = Prng::new(0x77E1_0005);
    for round in 0..scale(100) {
        let mut out = vec![0u8; 4096];
        let mut w = BsWriter::new();
        let ops: Vec<WriteOp> = (0..rng.below(32) + 1)
            .map(|_| match rng.below(3) {
                0 => WriteOp::Bits(rng.range_i32(1, 16), rng.next_u32() & 0xFFFF),
                1 => WriteOp::Ue(rng.below(1 << 14)),
                _ => WriteOp::Se(rng.range_i32(-8000, 8000)),
            })
            .map(|op| match op {
                // Re-mask, since Bits above may have produced a too-wide value.
                WriteOp::Bits(n, v) => WriteOp::Bits(n, v & ((1u32 << n) - 1)),
                other => other,
            })
            .collect();
        for op in &ops {
            op.apply_new(&mut w, &mut out);
        }
        let bits = w.bits_pos();
        w.rbsp_trailing_bits(&mut out);
        // The reader's 4-byte prime and 3-byte slop must be inside the buffer (F4);
        // `out` is generously sized above, so this only asserts the contract holds.
        assert!(out.len() >= ((bits as usize + 1 + 7) >> 3) + 3);

        let mut bs = BsCursor::init(&out, bits + 1).expect("round {round}: init");
        for (i, op) in ops.iter().enumerate() {
            match *op {
                WriteOp::Bits(n, v) => {
                    let mut code = 0u32;
                    let e = BsGetBits(&out, &mut bs, n, &mut code);
                    assert_eq!((e, code), (ERR_NONE, v), "round {round} op {i}");
                }
                WriteOp::OneBit(v) => {
                    let mut code = 0u32;
                    let e = BsGetOneBit(&out, &mut bs, &mut code);
                    assert_eq!((e as i32, code), (ERR_NONE, v), "round {round} op {i}");
                }
                WriteOp::Ue(v) => {
                    let mut code = 0u32;
                    let e = BsGetUe(&out, &mut bs, &mut code);
                    assert_eq!((e as i32, code), (ERR_NONE, v), "round {round} op {i}");
                }
                WriteOp::Se(v) => {
                    let mut code = 0i32;
                    let e = BsGetSe(&out, &mut bs, &mut code);
                    assert_eq!((e, code), (ERR_NONE, v), "round {round} op {i}");
                }
            }
        }
    }
}
