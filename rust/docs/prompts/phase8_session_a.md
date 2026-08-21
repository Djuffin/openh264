# Phase 8, session A — the api inventory, F23, and the decoder's boundary ownership

You are executing the first session of Phase 8 (charter:
`rust/docs/prompts/phase8.md`). Work the steps in order, commit per unit, run
the gates as stated, report in the format at the end.

## Context

- Repo `/Users/eugene/projects/openh264`, branch `rust3`; crate
  `rust/crates/openh264-rs/`, paths relative to its `src/`. C++ at `codec/` is
  the reference; the public ABI (`codec/api/wels/codec_api.h`) is the contract
  and **does not change**.
- Start: commit `b2e2c9d7` (Phase 7 closed; 37 encoder modules + all decoder
  modules under `deny(unsafe_code)`; `src/api/` exempt). Docs at close:
  `safety_refactor_log.md`, plan §0 row, `phase8.md` §3, `perf_baseline.md`,
  `phase8_findings.md` (create it on the first finding).
- Commits: `refactor(T8.A<n>)` / `fix(T8.A<n>)` / `gate(T8.A<n>)` /
  `docs(T8.A-close)`.

## Hard rules

1. **Gates**: `gates.sh commit` per commit; `family` per step; `exit` at the
   close — **unscoped** (no `MIRI_SCOPE`): this phase touches both codecs, so
   the three decoder probes run in every Miri step again. Conformance 60/60,
   corpus 2707 agreeing, sweeps 369/369 both profiles, benches bit-identical.
2. **No public-ABI change**: vtable structs, slot order, factories, exported
   names, struct layouts crossing the boundary — untouched; `api/abi_guard.rs`
   asserts are re-pinned only if a *non-boundary* struct moves (it shouldn't
   this session).
3. **F23's rule**: the pointer a thunk receives as `this` derives from the
   whole impl allocation, never from an 8-byte base-struct borrow. No `&mut
   self` method survives on `ISVCDecoder`/`ISVCEncoder`.
4. **Wrong-length sweep output is a defect** (F3 is closed). Stop and fix.
5. **Anchors, not surfaces** (2026-08-20, Phase 7's close). Re-grep first.
6. **No behavior change** beyond F37/F41's fixes, each with its covering test
   and its byte gates unmoved (conformance/corpus are the referee for both).
7. **Perf**: one span at the close, both benches, 7 pairs + null; bisect on a
   breach before acting.
8. **Overflow**: stop green at a whole-family boundary; B inherits.

## Current state — verified facts

- `src/api/`: `abi_guard.rs` 121 lines (45 asserts), `codec_api.rs` 2,664
  (246 raw, 25 `unsafe fn`, 44 `extern "C"`), `mod.rs` 3.
- **F23's surface**: `impl ISVCEncoder` at `codec_api.rs:1130–:1191` — 8
  `&mut self` methods; `impl ISVCDecoder` at `:1263–:1380` — 4; **55 call
  sites** across `src/api`, `tests/`, `benches/` (grep
  `\.Initialize(\|\.DecodeFrame2(\|\.EncodeFrame(\|\.Uninitialize(` etc.).
  The Miri-covered integration paths already call through the raw vtable
  (`abi_test_driver`, a module inside `codec_api.rs`) — that is the pattern
  that is sound.
- **The decoder's twelve api-owned fields**: stamped at **10 `addr_of_mut!`
  sites** in `codec_api.rs` from `CWelsDecoderImpl` members
  (`sLastDecPicInfo`, `sDecoderStatistics`, `sPictInfoList`,
  `sReoderingStatus`, `iStreamSeqNum`, `sVlcTable`, `bIsBaseline`,
  `iLastBufferedIdx`, `pPicBuff`, `uiDecodeTimeStamp`, plus `align`); read
  back in the decoder through `api_alias`/`api_alias_mut`
  (`decoder/decoder_context.rs:1232`/`:1245`) — the section note above them
  states the safety obligation and names this phase as the owner. The
  decoder carries exactly three `#[allow(unsafe_code)]` items: these two and
  `SPicture::data_ptr`.
- **F41**: `decoder_init_c` overwrites `CWelsDecoderImpl::param` on every
  `Initialize` call before checking for an existing context, and the
  context's `pParam` points at that member — the C++ context owns its *own*
  copy written by `DecoderConfigParam`'s memcpy (`decoder_core.cpp:795`'s
  free path tests `pParam->bParseOnly`).
- **F37**: the port calls `ResetReorderingPictureBuffers` in exactly one
  place (`WelsCreateDecoder`); C++ `DestroyPicBuff` opens with it
  (`decoder.cpp:260`), so a re-init leaves `sPictInfoList` naming freed slots.
- **`memory_align.rs`**: alive only via `ctx_box.pMemAlign =
  addr_of_mut!((*dec_impl).align)` (`codec_api.rs:1531`) and 17
  `pMemAlign` sentinel sites in `src/decoder` + `src/api` (null tests /
  pass-throughs; the C++ decoder's real allocations through it all have
  owned counterparts since Phase 5 — name them when you delete, S44).
- **The instruments and their scopes** (S22's backlog): `tools/census.sh`
  (read its scope), `tools/find_dup_types.sh` (comment at line 7: encoder/
  common/decoder/processing — api absent), `tools/find_stub_bodies.py`
  (`ROOT` list at lines 39–42 — api absent), `tools/find_shadowing_stubs.py`
  (read its scope), `tools/find_elem_byte_confusion.py` (takes paths — pass
  `src/api`).

## Step 0 — the api inventory

1. Extend each instrument's scope to include `src/api` (one-line edits; keep
   every other scope as is). Run all five.
2. Triage every hit: fix it now if it is this session's family (F23 class,
   stamps, dups inside api), otherwise enumerate it with an owner (B, C, or
   a phase). The F52/F43 lesson applies: a shadowing stub or duplicate type in
   `api/` could have been hiding for eight phases — read both lines of every
   candidate.

Accept: all five instruments include `src/api` permanently; the backlog
table in the log (hit, verdict, owner); `census.sh`'s allowlist updated only
with reasons.

## Step 1 — F23 and its encoder twin

1. Write the covering test first if none exists: a Miri-visible call through
   one decoder convenience and one encoder convenience (the shape the finding
   shows — red under Miri today). Keep it; it goes green in step 3 below.
2. Re-spell the 12 methods per hard rule 3 — read the 55 callers before
   choosing between (a) associated functions taking `this: *mut ISVCDecoder`
   / `*mut ISVCEncoder` that derive the impl pointer first, or (b) methods on
   `CWelsDecoderImpl`/`CWelsH264SVCEncoderImpl` taking `&mut self` of the
   **whole** object (the allocation root) with thin `*mut` entry points for
   consumers holding the base pointer. Either is sound; pick by what the 55
   callers read most naturally and say which.
3. Re-spell the 55 callers; `abi_test_driver` unchanged.

Accept: `grep -c '&mut self' codec_api.rs` inside the two `impl` blocks → 0;
the covering test green under Miri; every integration test and bench green;
`family` green.

## Step 2 — the decoder's boundary ownership

1. **F37**: add the reset at the top of `DestroyPicBuff` (the C++'s own
   site); add a re-init probe (Initialize → decode a few frames → Uninitialize
   → Initialize → decode) that would have reached the freed slots. Conformance
   and corpus unmoved.
2. **F41**: the context owns its parameter copy (`SDecodingParam` inline or
   boxed in the constructor-built context); `decoder_init_c` writes the
   context's copy through `DecoderConfigParam`'s path; `CWelsDecoderImpl::param`
   keeps whatever the api layer itself still reads, or goes. Covering test:
   change the api-side param after `Initialize` and verify teardown reads the
   context's copy.
3. **The twelve fields**: move the ten C++-member fields from
   `CWelsDecoderImpl` into the context it owns (the impl reads them through
   `pCtx`), or hand owned values at construction — the internal layout of
   `CWelsDecoderImpl` is not ABI (only `base` at offset 0 is). The 10
   `addr_of_mut!` stamps → 0; `api_alias`/`api_alias_mut` deleted; the
   decoder's allow items 3 → 1 (`data_ptr`, whose one production use is
   `DecodeFrameConstruction`'s `ppDst` writes across the ABI — it stays, or
   moves to `api/` with the write).
4. **`memory_align.rs`**: delete the `align` member and its stamp, the
   `pMemAlign` field and its 17 sentinel sites (each read first: a null test
   on a field that is always null deletes; a pass-through deletes with its
   parameter), then the file. If a site turns out live, stop and name it.

Accept: `grep -rn 'addr_of_mut' src/api/codec_api.rs` → 0 stamp sites;
`grep -rn 'api_alias' src/` → 0; `grep -rln 'allow(unsafe_code)' src/decoder`
→ at most the `data_ptr` site; `ls src/common/memory_align.rs` → gone (or its
last live user named); `family` green; conformance/corpus unmoved.

## Step 3 — close

1. The span (both benches); `gates.sh exit` unscoped — all seven probes + the
   MT probe + the F23 covering test by name.
2. Log entry: the inventory table, the F23 re-spelling chosen and why, the
   three fixes with their covering tests, what B inherits exactly (the encoder
   boundary: `m_pEncContext`, the trace pair, `m_pCodecInstance`, the 18
   thunks, the 8 ctx lines, the 27 `c_void`).
3. Plan §0 row (A spent, B next); `phase8.md` §3; `perf_baseline.md`.

## Do not touch

| what | why |
|---|---|
| vtable structs, slot order, factories, exported names, boundary struct layouts | the ABI contract |
| `encoder/wels_encoder_ext.rs` internals, the 18 thunks' bodies beyond what F23 forces | session B |
| `crate-type`, exports, external harness | session C |
| Phase 9's `port-raw` inventory, parked families, `SCREEN_CONTENT(dormant)` | later phases |

## Report back, in this order

1. One line: steps landed; `exit` verdict; HEAD; tree state.
2. The inventory: hits per instrument, fixed vs enumerated (with owners).
3. F23: the spelling chosen, callers re-spelled (n of 55), the covering test's
   red/green.
4. F37, F41, the twelve fields, `memory_align.rs` — one line each with the
   covering test or the grep.
5. The span.
6. Anything found and not fixed, with owner.
7. What B inherits, exactly.
