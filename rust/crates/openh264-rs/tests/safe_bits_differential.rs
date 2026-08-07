//! Differential tests: `safe::bits` against `SBitStringAux` + `dec_golomb` +
//! `vlc_encoder` (plan §2.2.2, taxonomy T3).
//!
//! The unit tests inside `src/safe/bits.rs` prove the cursors are *self*-consistent —
//! that what `BsWriter` writes, `BsCursor` reads back. These prove they are
//! *C*-consistent: identical values, identical error codes, identical cursor state and
//! identical output bytes to the implementations the codec uses today, over randomised
//! operation sequences and every truncation around the reader's slop boundary.
//!
//! This file lives outside `src/`, so unlike everything under `src/safe/` it may use
//! `unsafe` — it has to, because the reference implementations are raw-pointer code.
//! Every `unsafe` block here drives the old side of a comparison.
//!
//! Running this under Miri additionally checks the old implementations for UB; see
//! `rust/docs/phase1_findings.md`.

#![allow(non_snake_case)]

mod common;

use common::prng::Prng;

use openh264_rs::decoder::bit_stream::{DecInitBits, InitReadBits, SBitStringAux};
use openh264_rs::decoder::dec_golomb::{
    BsGetBits, BsGetOneBit, BsGetSe, BsGetTe0, BsGetTrailingBits, BsGetUe, CheckMoreRBSPData,
    GetLeadingZeroBits, ERR_NONE,
};
use openh264_rs::encoder::vlc_encoder::{
    BsFlush, BsGetBitsPos, BsRbspTrailingBits, BsSizeSE, BsSizeUE, BsWriteBits, BsWriteOneBit,
    BsWriteSE, BsWriteUE, InitBits,
};
use openh264_rs::safe::bits::{size_se, size_ue, trailing_bits, BsCursor, BsWriter};
use openh264_rs::safe::err::ErrInfo;

/// The RBSP plus the slack the C++ reader relies on. See `phase1_findings.md` §F4:
/// `dump_bits_aux` may sit one byte past the logical end and read two bytes there,
/// so the *allocation* must extend at least 3 bytes beyond it for the old reader to
/// be in bounds at all. Four keeps the initial 4-byte prime in bounds too.
fn rbsp_with_slack(payload: &[u8]) -> Vec<u8> {
    let mut v = payload.to_vec();
    v.extend_from_slice(&[0u8; 4]);
    v
}

/// Drives the old reader: `SBitStringAux` over `buf`, `kiSize` bits of payload.
fn old_init(buf: &[u8], size_bits: i32) -> (SBitStringAux, i32) {
    let mut bs = SBitStringAux::default();
    let err = unsafe { DecInitBits(&mut bs, buf.as_ptr(), size_bits) };
    (bs, err)
}

fn old_state(bs: &SBitStringAux) -> (usize, u32, i32) {
    let pos = (bs.pCurBuf as usize) - (bs.pStartBuf as usize);
    (pos, bs.uiCurBits, bs.iLeftBits)
}

fn new_state(c: &BsCursor) -> (usize, u32, i32) {
    (c.pos(), c.cur_bits(), c.left_bits())
}

/// `0` from the old side means success; anything else is the error code.
fn as_result<T>(code: i32, value: T) -> Result<T, ErrInfo> {
    if code == ERR_NONE {
        Ok(value)
    } else {
        Err(ErrInfo(code))
    }
}

#[test]
fn reader_init_matches_dec_init_bits() {
    let mut rng = Prng::new(0x8B17_0001);
    for payload_len in 0..24usize {
        let payload = rng.bytes(payload_len);
        let buf = rbsp_with_slack(&payload);
        // Every bit size in and around the payload, including sizes that are not a
        // whole number of bytes and the degenerate non-positive ones.
        for size_bits in -9i32..=(payload_len as i32 * 8 + 9) {
            let (bs, err) = old_init(&buf, size_bits);
            let old = as_result(err, ()).map(|()| old_state(&bs));
            let new = BsCursor::init(&buf, size_bits).map(|c| new_state(&c));
            assert_eq!(old, new, "payload {payload_len} bytes, kiSize {size_bits}");
        }
    }
}

#[test]
fn init_read_bits_matches_at_every_end_offset() {
    // `InitReadBits` is called with 1 by the CABAC init and 0 by decode_slice.
    let mut rng = Prng::new(0x8B17_0002);
    for payload_len in 1..12usize {
        let payload = rng.bytes(payload_len);
        let buf = rbsp_with_slack(&payload);
        let size_bits = payload_len as i32 * 8;
        for end_offset in 0..4isize {
            let (mut bs, err) = old_init(&buf, size_bits);
            assert_eq!(err, ERR_NONE);
            let mut c = BsCursor::init(&buf, size_bits).unwrap();

            let old_err = unsafe { InitReadBits(&mut bs, end_offset) };
            let old = as_result(old_err, ()).map(|()| old_state(&bs));
            let new = c.init_read_bits(&buf, end_offset).map(|()| new_state(&c));
            assert_eq!(old, new, "payload {payload_len}, end_offset {end_offset}");
        }
    }
}

/// One operation of a randomised read sequence.
#[derive(Clone, Copy, Debug)]
enum ReadOp {
    Bits(i32),
    OneBit,
    Ue,
    Se,
    Te0(i32),
}

impl ReadOp {
    fn random(rng: &mut Prng) -> Self {
        match rng.below(5) {
            0 => ReadOp::Bits(rng.range_i32(1, 16)),
            1 => ReadOp::OneBit,
            2 => ReadOp::Ue,
            3 => ReadOp::Se,
            _ => ReadOp::Te0(rng.range_i32(1, 8)),
        }
    }

    /// Applies the operation to both readers and returns `(old, new)` outcomes,
    /// widened to `i64` so every reader's value type compares in one place.
    fn apply(
        self,
        bs: &mut SBitStringAux,
        c: &mut BsCursor,
        buf: &[u8],
    ) -> (Result<i64, ErrInfo>, Result<i64, ErrInfo>) {
        match self {
            ReadOp::Bits(n) => {
                let mut code = 0u32;
                let err = unsafe { BsGetBits(bs, n, &mut code) };
                (
                    as_result(err, code as i64),
                    c.get_bits(buf, n).map(|v| v as i64),
                )
            }
            ReadOp::OneBit => {
                let mut code = 0u32;
                let err = unsafe { BsGetOneBit(bs, &mut code) } as i32;
                (
                    as_result(err, code as i64),
                    c.get_one_bit(buf).map(|v| v as i64),
                )
            }
            ReadOp::Ue => {
                let mut code = 0u32;
                let err = unsafe { BsGetUe(bs, &mut code) } as i32;
                (as_result(err, code as i64), c.get_ue(buf).map(|v| v as i64))
            }
            ReadOp::Se => {
                let mut code = 0i32;
                let err = unsafe { BsGetSe(bs, &mut code) };
                (as_result(err, code as i64), c.get_se(buf).map(|v| v as i64))
            }
            ReadOp::Te0(range) => {
                let mut code = 0u32;
                let err = unsafe { BsGetTe0(bs, range, &mut code) };
                (
                    as_result(err, code as i64),
                    c.get_te0(buf, range).map(|v| v as i64),
                )
            }
        }
    }
}

#[test]
fn reader_op_sequences_match_bit_for_bit() {
    let mut rng = Prng::new(0x8B17_0003);

    for round in 0..300 {
        // Buffer sizes from "shorter than one refill" up to a few KB.
        let payload_len = match round % 4 {
            0 => rng.below(9) as usize,
            1 => rng.below(64) as usize,
            2 => 1024 + rng.below(1024) as usize,
            _ => rng.below(24) as usize,
        };
        let payload = rng.bytes(payload_len);
        let buf = rbsp_with_slack(&payload);
        // Bit sizes that are not byte-aligned exercise the `(kiSize + 7) >> 3` edge.
        let size_bits = (payload_len as i32 * 8) - rng.below(8) as i32;

        let (mut bs, old_err) = old_init(&buf, size_bits);
        let new = BsCursor::init(&buf, size_bits);
        match (as_result(old_err, ()), new) {
            (Err(a), Err(b)) => {
                assert_eq!(a, b, "init disagreed, round {round}");
                continue;
            }
            (Ok(()), Ok(mut c)) => {
                assert_eq!(old_state(&bs), new_state(&c), "post-init state");
                for step in 0..64 {
                    let op = ReadOp::random(&mut rng);
                    let (old, new) = op.apply(&mut bs, &mut c, &buf);
                    assert_eq!(
                        old, new,
                        "round {round} step {step} op {op:?}, seed {:#x}",
                        rng.seed()
                    );
                    assert_eq!(
                        old_state(&bs),
                        new_state(&c),
                        "state after round {round} step {step} op {op:?}"
                    );
                    assert_eq!(
                        unsafe { CheckMoreRBSPData(&mut bs) },
                        c.check_more_rbsp_data(),
                        "CheckMoreRBSPData after round {round} step {step}"
                    );
                    if old.is_err() {
                        break; // the C++ callers stop at the first error too
                    }
                }
            }
            (a, b) => panic!("init disagreed: old {a:?}, new {b:?} (round {round})"),
        }
    }
}

#[test]
fn reader_matches_on_every_truncation_around_the_slop_boundary() {
    // The predicate under test is `iReadBytes > iAllowedBytes + 1`. Declared payload
    // lengths 0..=8 walk the cursor onto, over and past that boundary, while the
    // allocation stays big enough that the *old* reader is in bounds throughout.
    let mut rng = Prng::new(0x8B17_0004);
    for declared_len in 0..=8usize {
        for pattern in 0..6 {
            let payload: Vec<u8> = match pattern {
                0 => vec![0x00; 8],
                1 => vec![0xFF; 8],
                2 => vec![0x80; 8],
                3 => vec![0x01; 8],
                4 => vec![0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA],
                _ => rng.bytes(8),
            };
            let buf = rbsp_with_slack(&payload);
            let size_bits = declared_len as i32 * 8;

            let (mut bs, old_err) = old_init(&buf, size_bits);
            let new = BsCursor::init(&buf, size_bits);
            if old_err != ERR_NONE {
                assert_eq!(
                    new.map(|_| ()),
                    Err(ErrInfo(old_err)),
                    "init, declared {declared_len}"
                );
                continue;
            }
            let mut c = new.unwrap();

            // Read well past the end: both sides must fail at the same operation.
            for step in 0..32 {
                let mut code = 0u32;
                let old = as_result(unsafe { BsGetBits(&mut bs, 8, &mut code) }, code as i64);
                let got = c.get_bits(&buf, 8).map(|v| v as i64);
                assert_eq!(
                    old, got,
                    "declared {declared_len}, pattern {pattern}, step {step}"
                );
                assert_eq!(old_state(&bs), new_state(&c), "state, step {step}");
                if old.is_err() {
                    assert_eq!(old, Err(ErrInfo::READ_OVERFLOW));
                    break;
                }
            }
        }
    }
}

#[test]
fn ue_prefixes_longer_than_16_bits_match() {
    // BsGetUe splits its refill when the prefix exceeds 16 bits; those branches are
    // rare in real streams and easy to get wrong.
    for lead in 0..=31u32 {
        let mut payload = vec![0u8; 16];
        // `lead` zero bits, then a 1, then `lead` value bits of alternating pattern.
        let mut bit = 0usize;
        let put = |v: u32, n: usize, payload: &mut Vec<u8>, bit: &mut usize| {
            for i in (0..n).rev() {
                if (v >> i) & 1 != 0 {
                    payload[*bit / 8] |= 0x80 >> (*bit % 8);
                }
                *bit += 1;
            }
        };
        put(0, lead as usize, &mut payload, &mut bit);
        put(1, 1, &mut payload, &mut bit);
        put(0x5555_5555, lead as usize, &mut payload, &mut bit);

        let buf = rbsp_with_slack(&payload);
        let size_bits = payload.len() as i32 * 8;
        let (mut bs, err) = old_init(&buf, size_bits);
        assert_eq!(err, ERR_NONE);
        let mut c = BsCursor::init(&buf, size_bits).unwrap();

        let mut code = 0u32;
        let old = as_result(unsafe { BsGetUe(&mut bs, &mut code) } as i32, code);
        assert_eq!(old, c.get_ue(&buf), "leading zeros = {lead}");
        assert_eq!(old_state(&bs), new_state(&c), "state, leading zeros = {lead}");
    }
}

#[test]
fn bit_reads_over_16_reproduce_the_stale_low_bits() {
    // No decoder call site asks for more than 16 bits, because the refill only
    // guarantees 16 valid bits at rest — a wider read returns stale low bits. The
    // safe cursor reproduces that exactly rather than "fixing" it, which is what
    // makes it a drop-in replacement.
    let mut rng = Prng::new(0x8B17_0005);
    for _ in 0..200 {
        let payload = rng.bytes(64);
        let buf = rbsp_with_slack(&payload);
        let size_bits = payload.len() as i32 * 8;
        let (mut bs, _) = old_init(&buf, size_bits);
        let mut c = BsCursor::init(&buf, size_bits).unwrap();
        for _ in 0..16 {
            let n = rng.range_i32(17, 32);
            let mut code = 0u32;
            let old = as_result(unsafe { BsGetBits(&mut bs, n, &mut code) }, code);
            assert_eq!(old, c.get_bits(&buf, n), "n = {n}");
            assert_eq!(old_state(&bs), new_state(&c));
        }
    }
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
    values.extend((0..2000).map(|_| rng.next_u32()));

    for v in values {
        let old = GetLeadingZeroBits(v);
        let new = if v == 0 { -1 } else { v.leading_zeros() as i32 };
        assert_eq!(old, new, "GetLeadingZeroBits({v:#010x})");
    }
}

#[test]
fn trailing_bits_matches_for_every_byte() {
    for b in 0..=255u8 {
        let old = unsafe { BsGetTrailingBits(&b) };
        assert_eq!(old, trailing_bits(b), "byte {b:#04x}");
    }
}

// ===========================================================================
// BsWriter vs. the canonical vlc_encoder writer  (plan §2.2.2, F2)
// ===========================================================================

/// One operation of a randomised write sequence, in-contract for the canonical
/// writer: `1 <= n <= 31` and no bits set above bit `n-1`.
///
/// 32 is excluded because the canonical writer cannot survive it in a debug build —
/// see `writer_and_the_32_bit_word` and `phase1_findings.md` §F5. No encoder call
/// site asks for it (the widths in `src/encoder/` top out at 16 plus computed
/// Exp-Golomb lengths).
#[derive(Clone, Copy, Debug)]
enum WriteOp {
    Bits(i32, u32),
    OneBit(u32),
    Ue(u32),
    Se(i32),
}

impl WriteOp {
    fn random(rng: &mut Prng) -> Self {
        match rng.below(4) {
            0 => {
                let n = rng.range_i32(1, 31);
                WriteOp::Bits(n, rng.next_u32() & ((1u32 << n) - 1))
            }
            1 => WriteOp::OneBit(rng.below(2)),
            2 => WriteOp::Ue(rng.below(1 << 16)),
            _ => WriteOp::Se(rng.range_i32(-30000, 30000)),
        }
    }

    fn apply_old(self, bs: &mut SBitStringAux) {
        unsafe {
            match self {
                WriteOp::Bits(n, v) => {
                    BsWriteBits(bs, n, v);
                }
                WriteOp::OneBit(v) => {
                    BsWriteOneBit(bs, v);
                }
                WriteOp::Ue(v) => {
                    BsWriteUE(bs, v);
                }
                WriteOp::Se(v) => {
                    BsWriteSE(bs, v);
                }
            }
        }
    }

    fn apply_new(self, w: &mut BsWriter, buf: &mut [u8]) {
        match self {
            WriteOp::Bits(n, v) => w.write_bits(buf, n, v),
            WriteOp::OneBit(v) => w.write_one_bit(buf, v),
            WriteOp::Ue(v) => w.write_ue(buf, v),
            WriteOp::Se(v) => w.write_se(buf, v),
        }
    }
}

/// Runs one op sequence through both writers and asserts byte- and position-parity.
///
/// Note the old side is primed with `as_mut_ptr()`, not `as_ptr()`: `InitBits` takes
/// a `*const u8` and casts it to `*mut`, and `BsWriteBits` then writes through it, so
/// the pointer has to carry mutable provenance or the *test* is the UB.
fn assert_writers_agree(ops: &[WriteOp], flush: bool, label: &str) {
    const CAP: usize = 8192;
    let mut old_buf = vec![0u8; CAP];
    let mut new_buf = vec![0u8; CAP];

    let mut bs = SBitStringAux::default();
    unsafe { InitBits(&mut bs, old_buf.as_mut_ptr(), CAP as i32) };
    let mut w = BsWriter::new();

    for (i, op) in ops.iter().enumerate() {
        op.apply_old(&mut bs);
        op.apply_new(&mut w, &mut new_buf);
        assert_eq!(
            unsafe { BsGetBitsPos(&bs) },
            w.bits_pos(),
            "{label}: bit position after op {i} ({op:?})"
        );
        assert_eq!(
            (bs.pCurBuf as usize) - (bs.pStartBuf as usize),
            w.pos(),
            "{label}: byte position after op {i} ({op:?})"
        );
        assert_eq!(bs.iLeftBits, w.left_bits(), "{label}: iLeftBits after op {i}");
    }

    if flush {
        unsafe { BsFlush(&mut bs) };
        w.flush(&mut new_buf);
        assert_eq!(
            (bs.pCurBuf as usize) - (bs.pStartBuf as usize),
            w.pos(),
            "{label}: byte position after flush"
        );
    }
    assert_eq!(old_buf, new_buf, "{label}: output bytes");
}

#[test]
fn writer_op_sequences_are_byte_identical() {
    let mut rng = Prng::new(0x77E1_0001);
    for round in 0..300 {
        let n_ops = rng.below(48) as usize + 1;
        let ops: Vec<WriteOp> = (0..n_ops).map(|_| WriteOp::random(&mut rng)).collect();
        assert_writers_agree(&ops, round % 2 == 0, &format!("round {round}"));
    }
}

#[test]
fn writer_matches_at_the_accumulator_boundary() {
    // `iLen == iLeftBits` is where the four writer copies (F2) take different
    // branches to the same state. Hit it deliberately from every fill level. `fill`
    // starts at 1 because filling 0 bits and then writing 32 is the F5 case below.
    for fill in 1..32i32 {
        for extra in 0..3i32 {
            let mut ops = vec![WriteOp::Bits(fill, 0xFFFF_FFFF >> (32 - fill))];
            let left = 32 - fill;
            ops.push(WriteOp::Bits(
                left,
                0xA5A5_A5A5 & (0xFFFF_FFFFu32 >> (32 - left)),
            ));
            for _ in 0..extra {
                ops.push(WriteOp::Bits(9, 0x155));
            }
            assert_writers_agree(&ops, true, &format!("fill {fill}, extra {extra}"));
        }
    }
}

/// A full 32-bit write into an empty accumulator — `phase1_findings.md` §F5.
///
/// The canonical writer evaluates `uiCurBits << iLeftBits` with `iLeftBits == 32`,
/// which is UB in C++ and, in this port, a **debug-build panic**. `BsWriter` folds
/// that shift away (it can only be reached with an empty accumulator, where it
/// contributes nothing) and so produces the C++'s intended output in both profiles.
/// No encoder call site writes 32 bits at once today, which is why the gates never
/// saw this.
#[test]
fn writer_and_the_32_bit_word() {
    const CAP: usize = 64;
    let mut new_buf = vec![0u8; CAP];
    let mut w = BsWriter::new();
    w.write_bits(&mut new_buf, 32, 0xDEAD_BEEF);
    assert_eq!(&new_buf[..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(w.pos(), 4);
    assert_eq!(w.left_bits(), 32);
    assert_eq!(w.bits_pos(), 32);

    let mut old_buf = vec![0u8; CAP];
    let mut bs = SBitStringAux::default();
    unsafe { InitBits(&mut bs, old_buf.as_mut_ptr(), CAP as i32) };
    let old = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        BsWriteBits(&mut bs, 32, 0xDEAD_BEEF);
    }));

    if cfg!(debug_assertions) {
        assert!(
            old.is_err(),
            "F5: the canonical writer is expected to panic on a 32-bit write in a \
             debug build; if this now passes, re-check the finding"
        );
    } else {
        assert!(old.is_ok(), "release: the shift is masked, not checked");
        assert_eq!(&old_buf[..4], &new_buf[..4], "release output parity");
        assert_eq!((bs.pCurBuf as usize) - (bs.pStartBuf as usize), w.pos());
    }
}

#[test]
fn writer_snapshot_and_rollback_matches_the_cursor_stash() {
    // The safe replacement for StashMBStatus/StashPopMBStatus
    // (`svc_set_mb_syn_cavlc.rs:1057-1076`): the old code saves and restores
    // `pCurBuf`/`uiCurBits`/`iLeftBits`; the new one saves the whole `Copy` value.
    const CAP: usize = 4096;
    let mut rng = Prng::new(0x77E1_0002);

    for round in 0..100 {
        let mut old_buf = vec![0u8; CAP];
        let mut new_buf = vec![0u8; CAP];
        let mut bs = SBitStringAux::default();
        unsafe { InitBits(&mut bs, old_buf.as_mut_ptr(), CAP as i32) };
        let mut w = BsWriter::new();

        let prefix: Vec<WriteOp> = (0..rng.below(12) + 1)
            .map(|_| WriteOp::random(&mut rng))
            .collect();
        let discarded: Vec<WriteOp> = (0..rng.below(12) + 1)
            .map(|_| WriteOp::random(&mut rng))
            .collect();
        let kept: Vec<WriteOp> = (0..rng.below(12) + 1)
            .map(|_| WriteOp::random(&mut rng))
            .collect();

        for op in &prefix {
            op.apply_old(&mut bs);
            op.apply_new(&mut w, &mut new_buf);
        }

        // Stash.
        let saved_old = (bs.pCurBuf, bs.uiCurBits, bs.iLeftBits);
        let saved_old_bytes = old_buf.clone();
        let saved_new = w;
        let saved_new_bytes = new_buf.clone();

        for op in &discarded {
            op.apply_old(&mut bs);
            op.apply_new(&mut w, &mut new_buf);
        }

        // Pop.
        bs.pCurBuf = saved_old.0;
        bs.uiCurBits = saved_old.1;
        bs.iLeftBits = saved_old.2;
        old_buf.copy_from_slice(&saved_old_bytes);
        w = saved_new;
        new_buf.copy_from_slice(&saved_new_bytes);

        for op in &kept {
            op.apply_old(&mut bs);
            op.apply_new(&mut w, &mut new_buf);
        }
        unsafe { BsFlush(&mut bs) };
        w.flush(&mut new_buf);

        assert_eq!(
            (bs.pCurBuf as usize) - (bs.pStartBuf as usize),
            w.pos(),
            "round {round}: position after rollback"
        );
        assert_eq!(old_buf, new_buf, "round {round}: bytes after rollback");
    }
}

#[test]
fn rbsp_trailing_bits_matches() {
    let mut rng = Prng::new(0x77E1_0003);
    const CAP: usize = 1024;
    for round in 0..100 {
        let mut old_buf = vec![0u8; CAP];
        let mut new_buf = vec![0u8; CAP];
        let mut bs = SBitStringAux::default();
        unsafe { InitBits(&mut bs, old_buf.as_mut_ptr(), CAP as i32) };
        let mut w = BsWriter::new();

        for _ in 0..rng.below(24) + 1 {
            let op = WriteOp::random(&mut rng);
            op.apply_old(&mut bs);
            op.apply_new(&mut w, &mut new_buf);
        }
        unsafe { BsRbspTrailingBits(&mut bs) };
        w.rbsp_trailing_bits(&mut new_buf);

        assert_eq!(old_buf, new_buf, "round {round}");
        assert_eq!(
            (bs.pCurBuf as usize) - (bs.pStartBuf as usize),
            w.pos(),
            "round {round}: position"
        );
    }
}

#[test]
fn exp_golomb_sizes_match_the_table_driven_versions() {
    for v in 0..70000u32 {
        assert_eq!(BsSizeUE(v), size_ue(v), "BsSizeUE({v})");
    }
    for v in -35000i32..35000 {
        assert_eq!(BsSizeSE(v), size_se(v), "BsSizeSE({v})");
    }
    let mut rng = Prng::new(0x77E1_0004);
    for _ in 0..20000 {
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
    for round in 0..100 {
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

        let (mut bs, err) = old_init(&out, bits + 1);
        assert_eq!(err, ERR_NONE, "round {round}");
        for (i, op) in ops.iter().enumerate() {
            match *op {
                WriteOp::Bits(n, v) => {
                    let mut code = 0u32;
                    let e = unsafe { BsGetBits(&mut bs, n, &mut code) };
                    assert_eq!((e, code), (ERR_NONE, v), "round {round} op {i}");
                }
                WriteOp::OneBit(v) => {
                    let mut code = 0u32;
                    let e = unsafe { BsGetOneBit(&mut bs, &mut code) };
                    assert_eq!((e as i32, code), (ERR_NONE, v), "round {round} op {i}");
                }
                WriteOp::Ue(v) => {
                    let mut code = 0u32;
                    let e = unsafe { BsGetUe(&mut bs, &mut code) };
                    assert_eq!((e as i32, code), (ERR_NONE, v), "round {round} op {i}");
                }
                WriteOp::Se(v) => {
                    let mut code = 0i32;
                    let e = unsafe { BsGetSe(&mut bs, &mut code) };
                    assert_eq!((e, code), (ERR_NONE, v), "round {round} op {i}");
                }
            }
        }
    }
}
