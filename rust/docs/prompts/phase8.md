# Phase 8 — the C-ABI boundary: safe cores, thunks as translators, the drop-in made real

The charter. Session briefs cite it; standing rules are plan §7.6; the plan's
§4 Phase 8 section carries the five steps and the inheritance blocks.

## 0. Starting position (Phase 7's close, `b2e2c9d7`; measured 2026-08-20)

- `src/api/` is **2,788 lines** in three files and is the only module outside
  the thread seam never scanned by the project's instruments (S22's clause —
  its `deny(unsafe_code)` exemption propagated into every sweep's scope).
  `codec_api.rs` 2,664 lines: **246** raw-pointer tokens, **25** `unsafe fn`,
  **44** `extern "C"` functions, **18** vtable thunks (`*_c`: 9 encoder, 9
  decoder), **12** `&mut self` convenience methods on the 8-byte vtable base
  structs (8 on `ISVCEncoder` at `:1130–:1191`, 4 on `ISVCDecoder` at
  `:1263–:1380`) called from **55** sites across api/tests/benches — **F23**
  and its encoder twin: each is UB today, the borrow eight bytes wide while
  the thunk writes the impl object past it.
- ~~**Exports: 5 of upstream's 7.**~~ **Wrong, corrected at T8.C3 and left visible.**
  `WelsGetCodecVersion` and `WelsGetCodecVersionEx` have carried
  `#[unsafe(no_mangle)]` throughout — `git show b2e2c9d7:` has both — so the *seven*
  were always attributed. What the count missed is that `no_mangle` occurs in **four**
  files, not two: fourteen `WelsSampleSad*_c` in `common/sad_common.rs` and three
  `WelsCabac*` in `encoder/set_mb_syn_cabac.rs`, every one of them also a symbol
  `libopenh264` exports. **With `crate-type` set the export list is 24, not 5**, and
  the real gate is "exactly seven, no more and no fewer" rather than "add the two
  missing". No `crate-type` is set at the open (rlib only), which is why nothing had
  ever looked. `api/abi_guard.rs` pins **53 assertions over 20 types** (the "45" is
  also a miscount).
- **The impl objects.** `CWelsDecoderImpl` (`codec_api.rs:1388`): `base`,
  `pVtbl: Box<_>`, `pCtx: *mut SWelsDecoderContext`, `align: CMemoryAlign`,
  `param: SDecodingParam`, `bEndOfStream`, and ten C++-member fields
  (`sLastDecPicInfo`, `sDecoderStatistics`, `sPictInfoList[16]`,
  `sReoderingStatus`, `iStreamSeqNum`, `sVlcTable`, `bIsBaseline`,
  `iLastBufferedIdx`, `pPicBuff`, `uiDecodeTimeStamp`) that are **stamped
  into the context by `addr_of_mut!` at 10 sites** — the decoder's twelve
  api-owned fields, read back through `api_alias`/`api_alias_mut`
  (`decoder/decoder_context.rs:1232`/`:1245`, two of the decoder's three
  allow items). `CWelsH264SVCEncoderImpl` (`:1381`): `base`, `pVtbl`,
  `inner: CWelsH264SVCEncoder` — which (`encoder/wels_encoder_ext.rs:1576`)
  holds `m_pEncContext: *mut sWelsEncCtx` (raw), `m_pWelsTrace: *mut
  welsCodecTrace` (a `Box::into_raw`), and `welsCodecTrace` holds the raw C
  trace pair (`m_fpTrace`, `m_pTraceCtx: *mut c_void`) plus
  `m_pCodecInstance: *mut c_void` — a back-pointer to the encoder object
  (F38's class).
- **`wels_encoder_ext.rs`**: 136 raw tokens, 15 `unsafe fn`, 14 `extern "C"`,
  the 8 `*mut sWelsEncCtx` boundary lines, 12 of the encoder's 27 remaining
  `c_void` occurrences.
- **`common/memory_align.rs` is alive by two threads**: the stamp
  `ctx_box.pMemAlign = addr_of_mut!((*dec_impl).align)` (`codec_api.rs:1531`)
  and `SWelsDecoderContext::pMemAlign` as a null sentinel at 17 sites. The
  allocator is structurally dead on both sides (Phase 7 measured it).
- **Open findings owned here**: F23 (+ twin), F37 (`DestroyPicBuff` never
  resets the reordering buffers; one C++ call at `decoder.cpp:260`), F41
  (`pCtx->pParam` aliases `CWelsDecoderImpl::param`, overwritten on every
  `Initialize`), the F38-class back-pointer inventory over `api/`.
- **Assets**: `test/api/` is upstream's gtest API suite, already built once
  (`.o` files present); `tools/diffharness/cxx_enc.cpp` is a C++ driver
  compiled against `codec/api/wels` headers — the template for the external
  dlopen driver. Every in-process gate (conformance 60/60, corpus 2707,
  sweeps 369/369, Miri with all seven probes, benches) stays exactly as is.

## 1. The design (plan §2.2.8, unchanged) — and the one rule this phase adds

The externally visible API does not change at all. The safety line moves up
to it: `CWelsDecoderImpl`/`CWelsH264SVCEncoderImpl` own the safe cores; each
thunk becomes translate-in → safe call → translate-out with a written
`# Safety` contract; the vtable structs, slot order (incl. the
`DecodeParser`/`DecodeFrameEx` stubs), factories and version functions are
untouched; the crate ships `rlib + cdylib + staticlib`; the cdylib exports
exactly upstream's 7; an external C++ driver `dlopen`s it and reproduces the
in-process hashes; `api/` ends as the crate's one `#[allow(unsafe_code)]`
module (crate-root `deny` flips in Phase 9).

**The rule (S28 at the ABI layer, from F23)**: the pointer a thunk receives as
`this` must derive from the **whole impl allocation**, never from a borrow of
the 8-byte base struct. Consequently no `&mut self` method may live on
`ISVCDecoder`/`ISVCEncoder`; consumer conveniences take `this: *mut _` or live
on the impl objects.

## 2. The referee

- In-process battery unchanged; **Miri runs unscoped in this phase** (both
  codecs are touched; D-gate-2 was an encoder-phase scoping) — the three
  decoder probes, the four encoder probes, the MT probe, and the
  `abi_test_driver` paths all by name.
- **The api inventory first** (S22): every instrument extended to `src/api`
  and run before anything is rewired; its backlog is session A's first table.
- **New gates this phase builds**: (i) exported-symbol list of the cdylib ==
  upstream's 7; (ii) `api/abi_guard.rs` pins every boundary-crossing struct
  (`SParserBsInfo`, `OpenH264Version`, `SDecoderCapability`, …); (iii) the
  external-ABI harness — conformance + encode-loopback through the dylib,
  hashes == in-process; stretch: upstream's `test/api` gtest suite against
  the dylib.
  **All three built (T8.C3/C4/C5); (i) and (iii) are steps of `gates.sh exit`.**
  (ii) is a compile-time guard rather than a gate step — 242 assertions over 51
  types, and "every boundary-crossing struct" turned out to mean **three** headers,
  not two: `SBufferInfo`, `SSysMEMBuffer`, `EVideoFormatType`, `EVideoFrameType` and
  `CM_RETURN` all live in `codec_def.h`, which the other two include.
- F23's covering test: the existing Miri-visible reproducer (decoder init
  through a convenience method), red before, green after, kept.
- The caveat the plan records: `DecodeFrame2` vs `DecodeFrameNoDelay` ordering
  is a known divergence class; the boundary preserves exact call-sequence
  semantics.

## 3. Session plan

- **A — DONE** (2026-08-21, `82e1e54f..01926eb6`, eight commits, `exit`
  unscoped green). The whole brief landed, and three of its counts did not
  survive a re-grep: the conveniences are **19**, their callers **122**
  (seven in the diffharness's own cargo project, which only the sweep
  builds), and **F37 was already fixed at T5.O1** — what was missing was its
  public-API probe. The inventory found **six duplicate boundary
  declarations, two divergent**; four unified, `SParserBsInfo` (two entities
  under one name — a rename) → D5/Phase 9, `WelsTraceCallback` → B.
  **F23 and F41 closed**, each with a covering test measured red. **10 stamps
  → 0, 69 `api_alias` call sites → 0, both accessors deleted, `memory_align.rs`
  deleted**; `src/decoder/` is down to two allow items, both `data_ptr`'s
  Miri instruments, with no production item left. Crate `raw_ptr` −57,
  `unsafe_block` −23, `unsafe_fn` −5. Span: no measurable movement on either
  bench. **F76 opened**, owner B.
- **B — DONE** (2026-08-21, `51a0956a..e0f6c03d`, ten commits plus a
  three-commit close, **`exit` unscoped `OVERALL: PASS` 13/0/1**). The whole brief landed, and the session's shape is worth stating: **every
  step was nominally an ownership change, and every one of them surfaced a parity
  defect the byte gates could not see.** **F76 CLOSED** in three commits (the two
  `DecoderConfigParam` statements plus both clamps; the live re-init rebuild;
  `welsDecoderExt.cpp:815–905` whole), six covering tests each measured red, corpus
  **2707/0** and conformance **60/60** unmoved at all three. The `eEcActiveIdc` clamp
  had to move **up** to the boundary and run on the wire bytes: in Rust `*pParam` is
  undefined for exactly the inputs the clamp exists to handle. Two reset arms are
  transcribed but **measurably unreachable** (0 of 2707 golden rows; 62 assets × 4
  modes × 2 declarations → state union `0x36`). **F78 NEW/CLOSED** — the encoder had
  a *second*, entirely dead C-ABI surface (nine `ext_*` thunks, two factories
  upstream does not declare, a `vptr` never read); rule 6 found it because the
  charter's inventory counted `*_c` in `src/api` and this set is neither. **F79
  NEW/CLOSED** — the trace callback never fired on either codec: `WelsLog` was a stub
  twice, the encoder had **zero** call sites against the reference's 87, and the
  decoder's seventeen had no destination. The reference's `SLogContext`
  back-pointer **cannot be transliterated** (F38's class, on the logging path), so
  the struct carries the settings and the option arms re-stamp the copy. **F77 NEW,
  OPEN, Phase 9** — `res/Error_I_P.264` aborts the process through an `extern "C"`
  thunk; pre-existing, in no gate, deliberately not fixed under rule 4. Ownership:
  `m_pEncContext`/`m_pWelsTrace`/`pCtx` all owned, two S42 roots taking the *slot*.
  **19 thunks / 19 contracts**; `Decoder` and `Encoder` carved with the **`Send`
  verdict measured** (neither; 14 `E0277`s enumerated). `c_void`: **26 code, all
  C-ABI and tagged**, 21 prose. Ratchet `raw_ptr` 2277 → 2225, `unsafe_fn` 809 → 802.
  Span: no measurable movement on either bench.

- **A — done** (2026-08-21, nine commits): the inventory (four dup boundary
  types fixed), F23 closed on both codecs (19 conveniences, 116 callers), F41
  closed, F37 proved, the twelve fields owned, `memory_align.rs` deleted.
- **B — done** (2026-08-21, fourteen commits): F76 closed (seven arms), F78
  and F79 closed (the trace delivers), both boundary objects own their
  contexts and traces, 19 `# Safety` contracts, `Decoder`/`Encoder` carved
  with the `Send` verdict measured (no: 14 `E0277`s, all Phase 9's), the
  `c_void` line attributed; F77 filed.
- **C — DONE** (2026-08-21, `eb939c34..`, eleven commits; **`exit` unscoped
  `OVERALL: PASS` 15/0/1 with both new gates in it**, plus T8.C8 after the close). The whole brief landed, and **three of its premises did not
  survive a re-grep** — each was worth more than the step it belonged to.
  **F77 was not an off-by-one**: `WelsDecodeSlice`'s bound is ported faithfully, and
  `res/Error_I_P.264` changes resolution three times (352x288 → 640x480 → 352x288)
  against a port that had **never handled a resolution change at all** —
  `SyncPictureResolutionExt` had only `WelsRequestMem`'s first-allocation case while
  `InitialDqLayersContext` re-sized the layer unconditionally, so the layer went to
  1200 macroblocks with the pictures still at 396. Fixed against the C++'s own
  output (five frames, seventeen codes, all identical); the asset joins the corpus
  (**2919 rows, 0 code divergences**) and regenerating the goldens rewrote **no
  existing table**. **P13**: all 24 `extern "C"` entry points inside `catch_unwind`,
  profile verdict read rather than assumed, three covering tests measured red as a
  SIGABRT. **Exports were 24, not 5** — the version functions already had
  `#[no_mangle]`, and fourteen `WelsSampleSad*_c` plus three `WelsCabac*` were
  escaping, every one a name `libopenh264` also exports; **7/7** with a gate,
  red-proved by exit code. **51 boundary types from three headers, not 27 from two**
  — 51 sizes, 51 alignments, 140 offsets, from a committed C dump compiled as C and
  C++ and diffed; **F81** (a 1-byte `SDeliveryStatus` against the ABI's 12) fell out
  of the first build under it. **The harness**: 58/58 conformance and 14/14 encode
  loopback through `dlopen`, both profiles, plus the version pair, the capability
  block and F77's stream returning a code with the process alive — and it settles
  that the C++ abstract-class call path agrees with the Rust `#[repr(C)]` vtable.
  **The gtest stretch is a number, and it moved the same day it was written**:
  118/199 against the reference's 199/199 — then a challenge to how the failures had
  been tallied (by *fixture name*, when the fixtures are encode->decode round trips)
  found **F82**, `DecodeFrameNoDelay` forwarding once where the reference calls
  `DecodeFrame2` twice, on top of a null-input arm this port guarded and the reference
  does not. **155/199** after T8.C8, 50 of 51 decoder-output rows passing throughout. **D-api-1** executed, and found that `welsDecoderExt.cpp:164`'s
  `SetTraceLevel (WELS_LOG_ERROR)` was never ported; session B's predicted flood is
  **4 lines in the whole suite**. **`api/` denies** with 44 tagged allow items, and
  the internal `SParserBsInfo` is `ParseOnlyBsBuffers`. Span: no measurable movement
  on either bench.

Three sessions was the estimate and three is what it took. The exit gate did not
bend.

## 4. Non-goals

Any public-ABI change; renames (D5, Phase 9); the workspace split (D4,
post-Phase 9); perf recovery (Phase 9, D-perf-6); the parked families and
Phase 9's `port-raw` inventory; everything `SCREEN_CONTENT(dormant)`.
