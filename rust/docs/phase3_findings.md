# Phase 3 findings

Things found while executing Phase 3 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— the bitstream layer — that are *not* Phase 3's job to fix at the moment they were
found. Recorded here so no later session has to rediscover them. Numbering continues
from [`phase2_findings.md`](phase2_findings.md) (F8–F14); Phase 0's F1–F3 and Phase 1's
F4–F7 are in their own files.

---

## F15 — A stream truncated one byte into a slice NAL takes the decoder out of bounds; the two build profiles disagree about what happens next

**Status: open, live on ordinary input, and it is T3.3's to fix.** Found by
`tests/malformed_stream_parity.rs` (T3.0) on its **first run**, before any conversion
— which is the argument for having built that test first.

### The two sites

`ParseNalHeader` converts a byte length into a bit length by looking at the last byte
of the payload:

| site | expression | reached by |
|---|---|---|
| `decoder/nalu.rs:762` | `BsGetTrailingBits(pNal.add(iNalSize as usize - 1))` | every VCL NAL (`NAL_UNIT_CODED_SLICE`, `..._IDR`, `..._EXT`) |
| `decoder/nalu.rs:675` | same expression | `NAL_UNIT_PREFIX` with `uiNalRefIdc != 0` |

A third instance, `nalu.rs:1000` (`ParseNonVclNal`), is **guarded** by
`if kiSrcLen <= 0 { return }` at `:995` and is not affected.

`iNalSize` reaches zero on input that is not exotic at all. `ParseNalHeader` strips
trailing zero bytes from the RBSP (`:536-545`) and then consumes the NAL header byte
(`:577-578`), so **any slice NAL whose payload is one non-zero byte followed by zero
or more zero bytes** arrives at the site with `iNalSize == 0`. Truncating a stream
exactly one byte past a slice's start code produces precisely that, and every base
stream in T3.0's corpus contains 10–11 such truncations.

### What each profile does

Measured with `MALFORMED_IGNORE_WITHHELD=1` (the diagnostic escape hatch in the parity
test), on `res/SarVui.264` truncated to 32 bytes:

| | behaviour |
|---|---|
| **debug** | `attempt to subtract with overflow` at `nalu.rs:762:88`. The panic unwinds out of the `extern "C"` vtable thunk, so it is a `panic in a function that cannot unwind` — the **process aborts**. Not a failed decode: a dead process. |
| **release** | `0usize - 1` wraps, so the argument is `pNal.add(usize::MAX)` — out-of-bounds pointer arithmetic, UB by the arithmetic alone even without the load ([F7]'s shape). The address it lands on is `pNal - 1`, the NAL header byte, which is inside the `sRawData` allocation; the header byte of a type-1/5 NAL is always odd, so `BsGetTrailingBits` returns 0, `iBitSize` is 0, `DecInitBits` fails its `pCurBuf >= pEndBuf` check, and the NAL is rejected with `dsBitstreamError`. |

So release "works", deterministically, by reading a byte it has no right to read and
getting the answer it would have wanted anyway. Per plan §7.2 gate 0 a debug/release
disagreement is UB evidence, never flakiness — and here both halves of the
disagreement are defects.

### Upstream has the same expression

`codec/decoder/core/src/au_parser.cpp:252` and `:396` are
`iBitSize = (iNalSize << 3) - BsGetTrailingBits (pNal + iNalSize - 1);`, i.e. the port
is a faithful transliteration of an upstream bug. In C++ `iNalSize` is `int32_t`, so
the expression decrements the pointer (UB by the standard, benign on every real
target); the port's `as usize` cast is what turns it into a checked-subtraction panic
in debug. **The C++ is not a specification to preserve here** — there is no correct
behaviour to be parity with, which is why the fix is a real fix rather than an S6
arithmetic-parity translation.

### What T3.0 did about it, and why it did not just record the outcome

The corpus **withholds** the triggering entries — see `withheld()` in
`tests/malformed_stream_parity.rs` — and writes a `WITHHELD` row naming this finding
in the golden table instead of a decode result. That is not squeamishness about the
abort (the test's worker/parent split survives an abort and names the entry that
caused it — that is how all 11 per-stream instances were enumerated in one run). It is
that a golden table has to hold in **both** profiles, and this input has no
profile-independent behaviour to record.

The rows are therefore visible, counted, and tied to this finding rather than silently
missing. `MALFORMED_IGNORE_WITHHELD=1` runs them anyway for diagnosis.

### Who fixes it

**T3.3**, the seam that converts `nalu.rs`'s payload pointers to `Range<usize>`: the
expression becomes an index into a slice, the zero-length case becomes explicit, and
`withheld()` is **deleted in that same commit** so the eleven-per-stream `WITHHELD`
rows fill in with real error codes. That golden diff is the seam's evidence, and it is
a required gate strengthening in the same sense as deleting `au_set.rs`'s F13
accommodation (T3.4).

Until then, the decoder aborts in a debug build on a truncated slice NAL. Nothing in
the gate battery except T3.0 says so.

[F7]: phase1_findings.md#f7--the-readers-boundary-pointers-go-out-of-bounds--ub-not-a-wrong-value--outside-the-codecs-own-invariants

---

## F16 — `READER_SLOP` was derived from the wrong half of the reader: `BsEndCavlc`'s prime reaches past it, without bound

**Status: resolved inside T3.1b** (the seam that surfaced it). Recorded because the
*derivation* was wrong for a whole session and the same mistake is available to T3.2
and T3.3, which both re-derive readable extents.

### What T3.1a wrote down

`READER_SLOP = 3`, with this reasoning on the constant (`decoder/bit_stream.rs`):

> `dump_bits_aux` permits the read cursor to sit **one** byte past `pEndBuf` and then
> loads **two** bytes there, so the largest index the family can touch is `len + 2`
> […] Three bytes of readable slack past the RBSP therefore covers **every read the
> family can make, at any position, for any operation.**

That last clause is false. It was derived from `dump_bits_aux` and the initial prime,
which is the whole of `dec_golomb.rs` — but not the whole of the read side.

### What was missed

`BsEndCavlc` (now [`BsCursor::end_cavlc`]) primes **four** bytes at `iIndex >> 3`.
`iIndex` is not bounded by `len`: the CAVLC residual decoder advances it by however
many bits each symbol consumed (`pBs->iIndex += iUsedBits`), reading through its own
`SReadBitsCache` from `pStartBuf + (iIndex >> 3)` — also unchecked. On a **truncated**
stream the parser keeps accepting symbols past the end of the RBSP, so `iIndex >> 3`
runs past `len` by an amount bounded by nothing except how many symbols it takes
before some validity check fires.

Measured: on truncations of six conformance streams, the raw reader's CAVLC prime
reached `len + 5` and beyond — `pos = len + 2` needing bytes through `len + 5`, where
the declared window ended at `len + 2`. It never faulted because `sRawData` is a 4 MiB
allocation and those bytes were inside it.

So the readable extent is **not a constant offset from the RBSP length**. It is a
property of the allocation, and the guarantee at `decoder_core.rs:3637`
(`pEnd - pCurPos >= len + 4`) is not sufficient either.

### How it was found

T3.1b handed the cursor a slice of `len + READER_SLOP` and the slice index panicked —
13 corpus entries across 6 streams, all `end_cavlc`'s `buf[pos..pos + 4]`. **All 53
conformance hashes were unaffected**, because well-formed streams never take the
residual decoder past the RBSP. Only T3.0's malformed corpus sees this, which is the
second time the "build the malformed-stream gate before the conversion it judges" rule
has paid for itself in this phase.

Note what the failure was *not*: not a pre-existing panic to record and quarantine per
S12, but the conversion narrowing a window relative to what the raw code actually read
— a bug in the boundary helper, found by the gate that exists for exactly that.

### Resolution

`BsReader` carries `avail`, the real distance from the reader's base to the end of the
raw-data allocation, and `readable_from(pHead, pEnd, p, len)` is the single site that
computes it — the `pHead..pEnd` boundary helper the phase brief specifies. The cursor
now sees precisely the bytes the raw reader saw, so the 2316 golden rows hold
unchanged. T3.3 deletes both when the owned buffer makes the extent a slice length.

### For T3.2 and T3.3

The CABAC engine's end ladder (`cabac_decoder.rs:732-784`) selects a 4/3/2/1-byte final
load from `pBuffEnd - pBuffCurr`, and its init primes 5 bytes. **Do not re-derive its
readable extent from `len` plus a constant** — take it from the same helper, which is
also why the brief wants exactly one slice-reconstruction site.

[`BsCursor::end_cavlc`]: ../crates/openh264-rs/src/safe/bits.rs

## F17 — The Miri gate cannot fail: `gates.sh` tests a pipeline's last command, and that command is `tail`

**Status: FIXED 2026-08-10 (Phase 3 session C), and the fix is proven by a known-red
run rather than asserted — see Resolution.** It was a gate defect, not a code defect,
and it invalidated every "Miri green" claim made through `gates.sh` between the gate's
widening at Phase 2's exit and that commit. Found at T3.1b by reading a baseline log
that said, four lines apart:

```
error: test failed, to rerun pass `--lib`
…
PASS  miri --lib (whole library, minus the F12/F13 skips)
```

### The mechanism

`gates.sh` sets `set -u`. It does **not** set `pipefail`. Both Miri steps are written:

```bash
elif (cd "$CRATE" && … cargo +nightly miri test --lib -- "${MIRI_SKIPS[@]}" 2>&1) \
       | tee "$LOGS/miri_lib.log" | tail -5; then
  pass "miri --lib …"
```

Without `pipefail`, the `if` sees the exit status of the **last** command in the
pipeline — `tail -5` — which succeeds whenever it can write its output. `cargo miri`'s
status is discarded. The same shape is in the phase-exit loop that runs Miri over the
differential integration files, so **both** Miri steps are inert.

The two bench steps use the same `if pipeline; then` form but end in `grep -E …`, which
*does* carry signal: it fails when nothing matches, i.e. when the bench produced no
output at all. That is why a contended `cargo bench` shows up as
`FAIL … non-zero exit` while a genuinely failing Miri run shows up as `PASS`. The
steps that do it correctly — `run_cargo_test` and `sweep_gate` — capture
`${PIPESTATUS[0]}` explicitly, which is the fix.

### Why it survived

Every session that ran Miri also ran it **by hand** while developing (`cargo +nightly
miri test --lib safe::` and the differential files), and those runs were real. So the
findings attributed to Miri are genuine and the skip list is honest; what was never
true is that `gates.sh` would have *stopped* a session on a Miri regression. The gate
was a reporter, not a gate.

This is S17's lesson a second time, with a different mechanism: *an instrument that
cannot fail is not an instrument.* S17 was written about `FFMPEG` being unset; the
remedy there was to make the skip loud. Here the remedy is `PIPESTATUS`.

### What to do

Restructure both Miri steps to capture `${PIPESTATUS[0]}`, exactly as `run_cargo_test`
does. Do **not** reach for a global `set -o pipefail`: `run_cargo_test` and
`sweep_gate` already handle their own pipelines and the bench steps' `grep` filters
would start failing the battery whenever a bench legitimately prints nothing matching.

Until that lands, treat "Miri green" in a log entry as meaning *someone ran it by
hand*, and say so in the entry.

### Resolution

Both Miri steps now go through one `run_miri` helper built on `run_cargo_test`'s
shape: `${PIPESTATUS[0]}` for the verdict, libtest's own `test result:` totals parsed
out of the log to corroborate it, and a **zero-test clause** — `passed == 0` fails —
because this script's own header already names that trap for `cargo test` ("a test
that stops being compiled in looks exactly like a test that passes") and a mistyped
`--skip` or a renamed module walks through everything else.

The audit was wider than the two Miri steps. Also fixed in the same commit:

* **Both bench steps.** They took their verdict from a display `grep`, so a bench that
  printed clean rows and *then* died was a `PASS`. Now: `PIPESTATUS[0]` (cargo's own
  status, previously discarded) **and** `PIPESTATUS[2]` (the grep matched something —
  the one signal the old form did carry, kept rather than traded away) **and** the
  existing `MISMATCH`/`DIFFER` log check.
* **The sweep step's missing-tally path.** Its verdict was already sound —
  `sweep_gate` captured `${PIPESTATUS[0]}` all along, so the session-C brief's table
  was wrong about this row, corrected there. What was unsound: when `sweep.sh` died
  before printing `PASS=n FAIL=n`, the display fell back to `tail -3` text, which
  reads like a result. That path is now an explicit `fail`.
* **The phase-exit differential-Miri loop** iterated a discovered file list, so an
  empty list was a step that silently reported nothing. Empty now fails loudly.
* **`OVERALL: PASS` / `OVERALL: FAIL (N steps failed)`** as the last line, matching the
  exit code — session B misread a wrapper's exit status for the script's, and there is
  now one unmissable string and nothing else to mistake for it.

The full pipeline audit is a comment block at the bottom of `gates.sh`, listing every
verdict in the script and where its status comes from. `set -o pipefail` stays
rejected, with the reason recorded there.

**The proof it can fail.** A fixed gate is plausible; this makes it proven. The
`--lib` step was run once with the **F12 skip deleted** — a real known-red input, not
an injected fake — and Miri found `wels_thread_pool`'s retag race
(`error: Undefined Behavior: Data race detected between (1) retag read on thread
CWelsThreadPool and (2) retag write … at alloc630943`, `wels_thread_pool.rs:435`).
`gates.sh` printed `FAIL  miri --lib (whole library, minus the F12/F13 skips): 0
passed / 0 failed, rc=1`, then `OVERALL: FAIL (1 steps failed)`, and exited 1. Note
the totals: Miri aborts the binary before libtest prints a `test result:` line, so the
`rc` clause is what carried this verdict and the totals clause is genuine
belt-and-braces.

That run also failed the *fix* on its first attempt, which is the argument for running
it at all: `bench_rc=${PIPESTATUS[0]}; bench_matched=${PIPESTATUS[2]}` is broken,
because the first assignment is itself a command and replaces `PIPESTATUS` before the
second expansion reads it — under `set -u` that aborted the whole battery at the
decode-bench step. The array must be captured in one shot
(`st=("${PIPESTATUS[@]}")`).

**What this does not invalidate.** Every finding attributed to Miri stands: F10, F12,
F13 and session B's 285/0 were all read out of the logs by hand by the session that
found them. What was never true is that the battery would have *stopped* a session on
a Miri regression. The gate starts existing on this commit.
