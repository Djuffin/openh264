# Phase 5 — decoder structural rewrite: the remainder, as a closed checklist

Re-planned 2026-08-13 under **D-fid-1** (structural fidelity to the C++ retired;
output equivalence — goldens, sweeps, conformance — remains the correctness
definition) and **D-gate-1** (sprint gating). Rules: plan §7.6; per-session scope
is the S20 closure; S24 at every face open. History: sessions A–O's record is the
log and git — this file carries only what remains.

**The anti-circles contract.** This checklist is **closed**: work enters it only
via an F-finding or Eugene. Every session's commits map to W-items in its log
entry. The progress metric is monotone and grep-able — **decoder `raw_ptr`
occurrences 276 at session AB's close** (1283 at the re-plan, 1237 at R's open, 980
at V's, 974 at W's, 765 at X's, 456 at Y's, 310 at Z's, 278 at AA's), → ~0 at exit
(excluding the named survivors and prose); a session that closes no W-item and moves
no metric is a stall and says so.
**The metric has two floors and both are now measured** (session AB): a *visible*
one — **52 of the 276 are prose**, so report the split or the number is a fifth
wrong — and a *hidden* one, which is `PPicture`. An alias spelling reads **1** in
this instrument however many signatures carry it, so exit condition 1 counts that
family by **its own signature grep** (30 at AB's close, all of them the enumerated
survivor and its producers). S16 from both sides — **and a W-item can close while the metric stands still**, which is
what W2a did (85 sites, `raw_ptr` +0): two pointer fields became handles and four
accessor signatures were born, and the instrument counts pointer *types written*
(S16). Sizes below were re-measured at `eef8a90b` on 2026-08-13; re-grep at
the face. **And a size carries the directory it was taken in** (session P §0): the
re-plan's `pRefPic` and `pRefList` figures were taken at *different* scopes and said
so nowhere — together they put **228 sites of `src/encoder/`** inside W2, which
F12/P10 puts outside this phase entirely.

## The blocker class, named once

*A raw alias into a container blocks that container's ownership.* Everything
below W1 was sequenced by this one fact (T5.N1, T5.O7). The ordering rule:
**aliases become ids/indices before their container owns.** W2 is the key; it
unblocks W3–W5 and half of W6.

**Session P used the rule forwards, and it is predictive.** W1 was available
because T5.O4 had already given the NAL nodes their own allocations; W2a was
available because `PicPool` never owned the pictures its slots address; W2b's layer
copy is not available because its readers cannot reach the container at all. Three
calls made from the rule, no probe needed to make any of them, and all three probe
runs green on the first attempt. **Ask the rule before scheduling a conversion, not
after Miri does.**

## The checklist

| # | item | size (measured) | done when | unblocks |
|---|---|---|---|---|
| ~~W1~~ | ~~`pAccessUnitList` → `Option<Box<SAccessUnit>>`~~ — **DONE, T5.P1** (`a3b68334`) | 54 sites | — | W3's first cascade entry, taken |
| ~~W2a~~ | ~~the context's two pool aliases: `pDec`/`pECRefPic` → `PicId`~~ — **DONE, T5.P2** (`eef8a90b`) | 85 sites | — | `dec_pic`/`ec_ref_pic` are now the **only two sites** W3 has to convert |
| ~~W2b~~ | ~~the layer's `pDec` + the reference lists~~ — **DONE, T5.P′1 + T5.P′2** (`59dbbb0b`, `48355825`). The layer's `pDec` was a **cache** and died; `pRef` was dead. The lists became `Option<PicId>`. **302 real sites, not 438**: `pRefPic`'s 113 were one identifier naming two types (S24's new clause) | 160 layer `.pDec` + 142 list field accesses | — | **every raw alias into `PicPool` is gone** — W3 is unblocked |
| ~~W3~~ | **DONE — the tail landed at T5.R1–R4** (`2fd3e46c`, `029ea5af`, `bdbab19a`, `c16232cb`): the `pCurDqLayer` cache died first (85 sites, 48 functions, 8 files — a stored derivation through an owning `Box` cannot survive the next derivation, so the cache had to go before the list could own), then `pDqLayersList`, `TagFmo`'s map and the parse-only descriptor became owners. **`WelsMallocz|WelsFree` in `src/decoder/` reads 0**, `WelsMalloczHelper`/`WelsFreeHelper` deleted, F19 closed for the layer, half of F41 closed, F40 unrepresentable, `iMaxNalNum` deleted (F16). ~~Prior:~~ ownership cascade — ~~seam 1, T5.P′3~~ (`6758d885`): `SPicture` finishes owning itself, so `AllocPicture`'s `Box` is a *complete* owner. ~~seam 2, T5.P″1~~ (`acf5bfd1`): `pTempDec` and `pPicBuff` are owned fields, `alloc_picture` is the constructor, `CreatePicBuff`/`DestroyPicBuff` shrink, `PrefetchPic` deleted. ~~seam 3, T5.P″2 + T5.P″3~~ (`d7a1d130`, `b07e407c`): **the hoist** — 79 per-use resolutions become two derivations at three slice bracket tops, threaded through 30 functions and the per-macroblock dispatch type. ~~seam 4, T5.Q1 + T5.Q2~~ (`539212bd`, `a8eaa3e5`): **the flip** — `Pool<Option<Box<SPicture>>>`, the resolver family split shared/`_mut` so the compiler enumerates the writers, five brackets where a span really crossed, **F42** found and fixed with a red-under-revert test, **F19 closed decoder-side**, **R4 by construction**, F37's reset kept beside a `drop`. **Remaining**: `pDqLayersList` owned + `pCurDqLayer`'s deletion (**81 mid-tree ctx-field reads**, re-grepped at `a8eaa3e5`); `fmo.rs`'s map (25 sites, one file); the parser buffers (3 `WelsMalloczHelper` calls, all under `bParseOnly`, and `SParserBsInfo` is a public API type) | `pDqLayersList` + `fmo.rs`'s map + the parser buffers | zero `WelsMallocz`/`WelsFree` in `src/decoder/` (**8 call sites left, none a picture's**); cascade functions deleted; probe green per container | 5.5 closes |
| ~~W4~~ | **DONE, T5.R5** (`65a3e056`): 219 packed-word uses became assignments of the values they moved, `LD*`/`ST*` and both byte-pointer block helpers deleted, decoder LD/ST grep **0 in code** (37 prose — S16's floor), **F35's second half closed**, T5.N5's `debug_assert!` deleted because **F42 disproved it**. ~~Prior:~~ colocated + 5.3b: `GetColocatedMb` on `cur_and_ref`; `SetRectBlock`/`CopyRectBlock4Cols` on the grid; punning → byte ops | 325 `LD*/ST*` tokens remaining | decoder `LD32\|ST32\|LD16\|ST16\|LD64\|ST64` grep reads 0 | `mv_pred.rs` deny-ready |
| ~~W5~~ | **DONE, T5.R6** (`50e8a1ac`): 197 sites, 4 carriers; `SpsRef = (id, subset)` because the C picks between two SPS buffers with the extension flag and the pointer carried that choice; five `addr_of_mut!` lookups take **no borrow at all** — F41's question answered by construction. Done-test reads **2**, both `0..pSps.field` range operators, not field accesses. ~~Prior:~~ P4: `pSps`/`pPps` → active-paramset ids + lookup | 205 field occurrences (131 + 74), 4 carriers | `.pSps\|.pPps` greps read 0; no lookup borrow outlives its expression (F41's mistake, not repeated) | context sheds 2 raw fields |
| W6 | **OPEN — three of its four steps are done and the fourth is the context** (session X). Step 2's grid split and the layer flip landed whole (T5.X1–X3: `decode_slice.rs`'s 51 layer pointers → 0), step 3's plane/dispatch conversion landed whole (T5.X8: `SHIM(` 52 → 7, 46 Phase-2 wrappers deleted, `get_intra_predictor.rs` deny-clean), and step 1 — **the view struct** — is what is left, sized by measurement rather than by settlement in §"The context, measured at the face" below. ~~Prior:~~ **and session U re-derived its size: the settlement was out by 4.5x** (S24, at the face). The settled design said "80 `(*pCtx)` dereferences over 44 functions, touching 24 fields" and that is what made W6 look like one face. Measured at `1423f8eb`: the file holds **202 raw-pointer types over 55 `unsafe fn`**, and `deny(unsafe_code)` fires on *every* `unsafe fn` and `unsafe` block, not only on the context ones. The view struct addresses **44 of 202** — `*mut DqLayerState` is 51, the plane/block pointers 38, `SWelsNeighAvail` 13, `SSlice` 10, `SNalUnit` 8, `BsCursor` 5. **The settlement's own census was also wrong at the moment it was written**: at `a158183c` the file held **86 derefs over 30 fields**, not 80 over 24; today it is 80 over 29, and the one field that went away is `pFmo` (F43's fix gave it an owner). The design does not move — it is still one per-slice view struct, built by one `unsafe` constructor per bracket top — but **the face does**: W6 is not one session, and a brief that schedules it as one is wrong before it starts (S20's clause). `cabac_rbsp_window`'s retirement was sized too: **18 call sites, one per function, 72 occurrences with their callers**, and the window must thread from the slice bracket top through the whole per-MB dispatch — W3's-hoist scale on its own. ~~Prior:~~ **DEFERRED (session S, Eugene's direction: parity first, safety refactoring later; still deferred through T, whose scope was parity)** — the design is settled below and unexecuted. 5.6: `decode_slice.rs` per P1 — **NZC cache DONE** (T5.R7, `fb079758`: `&mut [u8; 48]`, 18 signatures, 92 uses, three dead dispatch typedefs deleted); **F31's redundant zeroing DONE** (T5.R8, `5fbe61a2`). **Remaining**: the EC MC paths; `cabac_rbsp_window`'s retirement (21 sites; it returns `&'a [u8]` with an **unbounded** lifetime synthesized from a raw pointer — S25's shape, and the window is constant across a slice, so it is the bracket maneuver again); the signature leg (**D-fid-1: functions may merge — 148 is an upper bound, not a target**) | the phase's largest file | **The done-test is the closure, not the file** (session V, measured at the face): `decode_slice.rs` deny-clean requires the **49 `unsafe fn`s it calls across 10 modules** to be safe first, so this cell is exit condition 2 wearing one file's name — order in `phase5_session_w.md` §1, view struct last. ~~`decode_slice.rs` compiles under `#![deny(unsafe_code)]`~~ — ~~**BLOCKED, and not by conversion work**~~ (session R): the file holds 80 `(*pCtx)` dereferences over 44 functions taking `pCtx: *mut SWelsDecoderContext` and the lint forbids every one; the way through is the context arriving as a reference, which **T5.G1 removed deliberately** (S29). **No decoder module is deny-clean today** — 0, not "all but this one". Eugene's or the steward's call | W7 |
| W7 | **three items done (session U), the rest is blocked on W6 — and the checklist's own `unblocks` column already said so.** Done: the F43-class resolution sweep (`tools/find_shadowing_stubs.py`, clean); the **F40-class sweep crate-wide** as an instrument (`tools/find_elem_byte_confusion.py`) — **0 suspects over 81 files**, 11 byte-sized hits with the two non-obvious ones hand-read, and the tool proved against the pre-T5.O5 tree where it reports F40 and exits 1; **the first three deny-clean decoder modules** (`vlc_tables.rs`, `parameter_sets.rs`, `slice.rs`), where the phase had **0** — compiler-verified, and proved to bite. **Blocked**: ~~42 of the 51 retiring SHIMs are in `get_intra_predictor.rs`~~ — **done at T5.X8**, with `decode_mb_aux.rs`'s four: `SHIM(` reads **7**, of which 1 is the named survivor, 2 are `expand_picture` bridges in `decoder_core.rs` and 4 are prose. **Blocked**: Measured per module at `e6873fe1` (`unsafe fn` / raw-ptr types): `decode_slice` 55/202, `decoder_core` 77/74, `deblocking` 34/109, `mv_pred` 22/115, `parse_mb_syn_cavlc` 26/68, `nalu` 23/67, `decoder_context` 27/65, `manage_dec_ref` 24/57, `parse_mb_syn_cabac` 34/45, `get_intra_predictor` 42/44, `pic_queue` 7/41, `error_concealment` 13/32, `cabac_decoder` 14/12, `decode_mb_aux` 4/13, `fmo` 9/6, `picture` 2/8, `bit_stream` 2/5, `dec_golomb` 1/1. ~~Remaining: 5.2's straggler sweep~~ — **DONE, session AB**: the sweep's two real hits (`WelsCopy16x16_c`/`WelsCopy8x8_c`, one C++ function ported twice) are fixed by moving the kernels to `common/` (T5.AB2), `find_shadowing_stubs.py` reads **19 candidates and every one a trait-impl method or F52's encoder-side set**, and **the survivor list is restated** as `data_ptr` + `data_ptr_ref` + `PPicture`'s 23 (§"Phase exit conditions"). W7 is closed but for the deny-clean count, which is W6's arithmetic | sweeps + deletions | SHIM greps match the survivor list exactly; every decoder module deny-clean or on the exception list with its Phase 8 pointer — **11 of 22 carry the lint at session X's close, 10 of them allowing nothing** (X added `get_intra_predictor` and `decode_mb_aux`, both to zero `unsafe fn` and zero raw pointers) (V added `dec_golomb`, `bit_stream`, `fmo`; W added `picture`, `cabac_decoder`, and `pic_queue` with two named exceptions) | the exit |
| ~~W8~~ | **DONE — the adjudication landed whole at session U** (`perf_baseline.md` §Phase 5 exit). N's stashed binaries got their owed day two: the whole span confirms (**+0.87%** against day one's +1.24%, every row above its own null's ceiling) and **the bisect does not** — Face 1's CB row flips sign (−0.72% → +1.31%), so S2b's clause applies and the halves were never resolvable. **The niche's motivating observation evaporated with it**: day one's CB-is-half asymmetry, which is what pointed at `Option<PicId>` on B slices, does not replicate. The niche itself reads **directionally consistent, unresolved** (3 pairs: CB +0.15% — the control, flat, as a B-slice mechanism must be on a stream without B-slices; Main −0.72%, High −0.99%, inside the 7-pair floor); its 1-pair reading was an artifact its own control convicted. **The window** (`d0b7f399`→`e6873fe1`, 69 commits) reads **+2.77% median / +3.58% CB at 7 pairs** and +2.63% / +3.87% at 3, every decode row above the 7-pair null's ceiling, encode flat — **real**. Its bisect does not resolve (CB rows sum, medians do not; session K's law). **Stop-line: BREACHED.** Cumulative CB **≈ +25.2…+25.8%** against ≈+23% — over by ≈2.2…2.8 points. D-perf-4's +25% *median* tripwire is not breached, and both are stated. **The escalation table is written and option 2 is the recommendation: day-two the window first, because it is free and this phase has twice had a second day overturn a first.** Every reading is provisional (no second day available); the hand-off names exactly what day two re-runs and all six binaries are stashed, so day two needs no build | one session | ~~`OVERALL: PASS` at `exit`~~; ledger reconciled or escalated with the table — **escalated**; ~~phase6.md exists~~ — **not written, and deliberately: Phase 5 has not exited** | Phase 6 |
| ~~W8 (prior)~~ | **PARTIAL (sessions S and T)** — the full battery ran at `exit` level in both (T: 482/476/20, Miri `--lib` 337/0 plus 20/7/3 on the differential targets, sweeps 341/341 both profiles, benches bit-identical, F3 zero hits) and §0 is refreshed twice; **the perf adjudication was not done in either and is the largest single item outstanding** — postponed out of T by direction (Eugene, 2026-08-14), and `phase6.md` is deliberately unwritten because the phase did not close. **the exit — never compressed**: all D-gate-1-deferred measurement (session N's stashed binaries, the niche verdict, the ≈+23% stop-line, the recovery/ledger adjudication vs ≈+21.6–21.9% cumulative CB), 3-pair+day-two protocol, §0 refresh, `prompts/phase6.md` (S19), briefs stamped historical | one session | `OVERALL: PASS` at `exit` level; ledger reconciled or escalated with the table; phase6.md exists | Phase 6 |

## Session mapping

**P** = W1 + W2a — **spent** (`a3b68334`, `eef8a90b`). **P′** = W2b + W3's first seam —
**spent** (`59dbbb0b`, `48355825`, `6758d885`;
[`phase5_session_p2.md`](phase5_session_p2.md)). **P″** = W3's seams 2 and 3 — **spent**
(`acf5bfd1`, `d7a1d130`, `b07e407c`; [`phase5_session_p3.md`](phase5_session_p3.md)).
**Q** = W3's flip — **spent** (`539212bd`, `a8eaa3e5`, `dd26eea6`;
[`phase5_session_q.md`](phase5_session_q.md)). It was scoped for the flip *plus* W4–W7
under the **forcing rules** written at its head (Eugene, 2026-08-14: a clean seam is a
checkpoint, not an exit; a reverted attempt with settled blockers is open work;
close-out ≤ 30 log lines; drops require a written reason), and it **ended on context
with W4–W7 untouched** — rule 1's first reason, named in the hand-off.
**Superseded (Eugene, 2026-08-14): that stop was an early exit** — ~300K of a 1M
window, and "context" was an uncheckable claim. **S31** (plan §7.6) now defines
exhaustion — not before the second compaction; state lives on disk; compact,
re-read, continue — and labels anything earlier an early exit.
**R** = W3's tail + W4 + W5 + W6's first two legs — **spent** (`2fd3e46c`, `029ea5af`,
`bdbab19a`, `c16232cb`, `65a3e056`, `50e8a1ac`, `fb079758`, `5fbe61a2`;
[`phase5_session_r.md`](phase5_session_r.md)). Scoped for everything but the exit under
**S31**, it stopped with W6's tail and W7 open **on rule 1's blocker, not on context** —
no compaction ever ran, and W6's done-test needs a decision only Eugene or the steward
can make (the row says which). **S** = **spent** (7 commits through `bfe3c80f`;
[`phase5_session_s.md`](phase5_session_s.md)). Scoped as the last session (F43 +
W6's tail + W7 + W8), it became **the parity pivot** instead: F43 opened into F44
and F45 (compensating defects — EC had never run in five phases) and F47 (real
CAVLC UB behind a probe gap), Eugene redirected mid-session (**D-par-1**, plan
§7.4: *test parity first, safety refactoring later*), and the session delivered
the parity work — truncation-row agreement 641/541 → **1182/0**, ten of eleven
corpus streams exact, four instruments and two probes added — with W6/W7
**deferred** and W8's adjudication **owed**. The phase stays **open**.
**T** = **spent** (6 commits, `252650aa`…`2e7b31fc`;
[`phase5_session_t.md`](phase5_session_t.md)) — the parity closure, and it closed.
**F46 fixed**: the corpus's truncation rows go **1142/1176 → 2318 agree / 0 differ**
on the `DECODING_STATE` the API returns, with no pixel moved. One type
(`DECODING_STATE` is a bit set the C header spells as an `enum`, so the port was
collapsing the accumulator to one bit), five unported error-signal sites — chiefly
`UpdateAccessUnit`'s mosaic-avoidance block, which left **`dsRefLost` with one
producer against the C++'s four** — and one inverted `if` that made the port accept a
truncated SPS the C++ rejects. **Face 1 confirmed**: `CABA2_SVA_B`'s 12 rows are the
documented POC tie-break, settled by per-frame hash multisets (identical sets, one
pair swapped) and annotated in the golden's generated header. **Face 2**: 13 tables,
2318 rows, codes 2318/0, output 2306/12. **The perf adjudication was postponed out
of T** (Eugene, 2026-08-14) and is U's. **U** = **spent** (4 commits, `cd50e3f6`…`e6873fe1`;
[`phase5_session_u.md`](phase5_session_u.md)) — **and it did not close the phase.**
Face 0 landed whole: the `--all-targets` gate hole is shut (proved red under
T5.T5's revert), and **every one of the corpus's 2707 rows now has a C++ referee**
where 389 had none — `ecref --stdin`, a harness corpus dump, and `compare_all.sh`
that checks the dump rather than trusting it. Its first run found three causes;
two were one-liners and are fixed (**F48**, the deleted `DetectStartCodePrefix`'s
*verdict*; **F49**, `IS_SPS_NAL` carrying the subset-SPS the C++ header excludes),
one is scoped with a reproduction (**F50**, 24 rows, `ParseSps` rejecting what the
C++ accepts). The corpus reads **output 2690/17, codes 2683/24**, and all 17 output
rows are the documented `CABA2_SVA_B` tie-break — the 5 new ones settled by
per-frame multiset, positions 3 and 4 swapped, nothing else. **W8 landed whole**
(row above). **W6 did not start and W7 is three items in**: U re-derived W6's size
at the face and the settlement was out by 4.5x, which is why — the rows say it.
**phase6.md is again deliberately unwritten**: exit conditions 1–3 are unmet, so
Phase 5 has not exited, and the next brief is a Phase 5 one
([`phase5_session_v.md`](phase5_session_v.md)).
**V** = **spent** (5 commits, `676231fa`…`92d6fa75`;
[`phase5_session_v.md`](phase5_session_v.md)) — scoped as the closing session
(Eugene, 2026-08-15: *"make the next session finish the work of this phase"*), and it
did not close the phase. **Face 0 landed whole and exit condition 5 is discharged**:
the day two is paid, the window **confirms** (+3.38% CB / +2.65% decode median at 7
pairs against U's +3.58% / +2.77%, ≈10 hours apart on the same calendar day — the
separation stated rather than implied), the niche loses the *directionally consistent*
half of its label, and the breach — cumulative CB ≈ **+25.0…+25.5%** against ≈+23% —
is dispositioned as **D-perf-6**: recovery to the Phase 9 perf pass, nothing parked
because nothing is attributed, the line not re-baselined to fit the result. **F50
closed on one token** — the C++'s `return false` from an `int32_t` is `ERR_NONE`, so
an unsupported `profile_idc` is *success* — taking corpus codes to **2707/0** with no
frame count, dimension or plane hash moved. **W6 did not land, and V's measurement of
why is the hand-off's content**: `decode_slice.rs` calls 49 `unsafe fn`s across 10
modules, so its done-test is the decoder's closure (439 `unsafe fn`, 977 raw-pointer
occurrences over 16 modules) and the decomposition's order **inverts** — callees
first, view struct last (§"The order the decomposition needs" below). Three modules
went deny-clean under the corrected order (**6 of 22**), and `fmo`'s conversion found
**F51**: `ResetFmoList` reset the counter and not the list, so `UninitFmoList` had no
caller where the C++ has one — fixed, with a red-under-revert test. Decoder `raw_ptr`
**974**. The next brief is a Phase 5 one
([`phase5_session_w.md`](phase5_session_w.md)); `phase6.md` stays unwritten for the
third session running, because exit conditions 1–3 are unmet.
**W** = **spent** (16 commits, `b045bd42`…`e6579e3c`;
[`phase5_session_w.md`](phase5_session_w.md)) — W6 bottom-up, and the unit that worked
turned out to be the **pointer family**, not the module. **Decoder `raw_ptr` 974 → 765
(−21%)**; files carrying `#![deny(unsafe_code)]` **6 → 9**, eight allowing nothing.
`picture.rs` and `cabac_decoder.rs` closed (the second to **zero** raw pointers),
`pic_queue.rs` closed with two exceptions named at the items; then the layer flip
(`*mut DqLayerState` 115 → 54 over five modules), `SWelsNeighAvail` (32 → 0),
`SRefPic` (11 → 0), the MV pair, the intra-pred array, the nzc record.
**The mechanism: a callee stops taking a raw pointer by taking what it actually
reaches** — three context fields, one `bool`, one 24-byte record — which is the view
struct at small scale and is why five families converted before it exists.
**What it found is the reason to do it** (S25): four recurring shapes of `&mut` held
across a call that takes the same object, including T5.I1's window across
`PredMvBDirectSpatial` — F24/F25/F28's shape, in two files. No value moved anywhere.
Two `exit` batteries `OVERALL: PASS` 13/0/1, corpus **2690/17 and 2707/0** unmoved,
F3 measurement 46 adjudicated. **The phase stays open**: exit conditions 1–3 unmet.
**AB** = **spent** (3 commits, `41149605`…`11002e4c`;
[`phase5_session_ab.md`](phase5_session_ab.md)) — **and it did not close the
phase**, for a reason that is arithmetic rather than a blocker. Decoder `raw_ptr`
**278 → 276**, `unsafe fn` **204 → 184**, deny-clean **12 → 13 of 22**, `SHIM(`
**3** unmoved, `exit` battery **13/0/1 — the phase's second fully clean one**.
Both of AA's rulings were executed as ruled. **`PPicture` is done**: its
convertible half is converted and the family's own grep reads **69 → 30** — 23
survivors named at their items with the F42 argument, and the 7 producers that
are the survivor's home. `mv_pred.rs` went deny-clean on the back of it (18 → 3
`unsafe fn`), and **71 `as_mut_ptr()` + `.add(i)` writes became indexing**, which
is what actually removed the keyword. The copy kernels moved to
`common/copy_mb.rs` — the C++'s own home — deduplicating `copy_rows` on the way,
and the decoder's second translation of `WelsCopy16x16_c` is now the same shim
the encoder's is. **The brief's own measurable question is answered yes**:
`PicRefs::classify` expresses error concealment's `same_picture` guard as a type,
and two of its three copy brackets run on borrows. **The session's own S24
corrections are three**: the brief's 41/23 split was 39/23 over 69 (two prose,
seven producers uncounted), and its perf base was inherited stale from AA's brief.
**What is left is size, and it is now measured**: decoder `raw_ptr` 276 = **224
code + 52 prose**, over nine modules, with no open design question in front of any
of them. The next brief is a Phase 5 one
([`phase5_session_ac.md`](phase5_session_ac.md)); `phase6.md` stays unwritten for
the eighth session running.
**AA** = **spent** (4 commits, `00a281bb`…`2de8703f`;
[`phase5_session_aa.md`](phase5_session_aa.md)) — **and it did not close the
phase**. Decoder `raw_ptr` **310 → 278**, `unsafe fn` **230 → 204**, deny-clean
**11 → 12 of 22**, `SHIM(` **6 → 3**, and the `exit` battery reads **13/0/1 —
the phase's first fully clean one** (Y and Z each closed at 12/1/1 on an F3 hit).
The layer bracket landed and `PDqLayer` is deleted: the two brackets that derived
it now **move** the layer out of the context for the loop, which is `slice_split`'s
maneuver in its other shape. The decided `common/` boundary landed at
`deblocking.rs` — **0 `unsafe fn`, lint on, one exception named**; the eight edge
filters take plane cursors and call `common/`'s safe kernels, and the decoder's raw
entry points into `common/deblocking_common` read 0. W7's straggler sweep took
`SHIM(` to 3 (real remainder **0**: one prose tombstone and `data_ptr` with its
shared form) and deleted `mb_grid_ptr`; the DPB lists take handles.
**The session's finding is a size the metric could not show**: `PDqLayer` was 16
signatures, and the largest family left is **`PPicture` at 78** — invisible to
`raw_ptr`, which counts spellings, not aliases (S16). **41 of the remaining 64 are
unblocked**; the other 23 need a decision only Eugene or the steward can make,
because there the raw pointer is load-bearing (F42) rather than vestigial. The next
brief is a Phase 5 one ([`phase5_session_ab.md`](phase5_session_ab.md)) and it opens
with that decision; `phase6.md` stays unwritten for the seventh session running.
**Z** = **spent** (5 commits, `92add69d`…`2a0e0330`;
[`phase5_session_z.md`](phase5_session_z.md)) — **faces 0, 1 and 2 landed whole,
and the phase did not close**. Decoder `raw_ptr` **383 → 310**, decoder
`unsafe fn` **326 → 230**, `SHIM(` 6 and deny-clean 11 of 22 unmoved.
**The context flip that X and Y each attempted and reverted is landed and
Miri-green**: the crate-wide grep for `pCtx: *mut SWelsDecoderContext` reads
**0**. Face 0 shed the accessor class Y's verdict named — and the clause that
made the difference is the *parameter*, not the return type: every accessor takes
the **field** it reaches, which is what repairs `FmoParamUpdate`'s four-things-
from-one-context call and what kept face 1 at Y's measured size instead of adding
an error per accessor site. **F54** is what that cost: `Option<SpsRef>`'s niche is
in a `bool`, so the zeroed shell read `Some(SpsRef { id: 0, … })` and every stream
decoded to zero frames until S21's audit clause was applied. Face 2 shed **68**
vestigial `unsafe fn` by strip-and-build, and added the rule the compiler cannot
supply: a signature that names a raw pointer keeps the keyword. **The session
reverted the flip once, mid-way, on scope, and was told to keep going** (Eugene,
in-session) — the re-attempt reached the same 27 errors in three scripted steps.
Faces 3 and 4 (`common/`, W7) did not start. The next brief is a Phase 5 one
([`phase5_session_aa.md`](phase5_session_aa.md)); `phase6.md` stays unwritten for
the sixth session running.
**Y** = **spent** (4 commits, `ab7ec744`…`dff3f78b`;
[`phase5_session_y.md`](phase5_session_y.md)) — **and it did not close the phase**.
Decoder `raw_ptr` **456 → 383**, `SHIM(` **7 → 6**, deny-clean **11 of 22** unmoved.
**Face 0's first half landed**: `SliceCtx` and `slice_split` — the slice's view of
the context, `pPicBuff` deliberately outside it — and **75 functions below the
bracket stopped taking a context** (228 → 153 still take it) (`decode_slice.rs` 89 → 36 raw-pointer
occurrences, `parse_mb_syn_cavlc` 26 → 18, `parse_mb_syn_cabac` 14 → 10). Both of
X's compiled facts landed with it, and `cabac_ctx_base` (29 sites) and
`cabac_rbsp_window` (18) retired with no callers left — W6 step 3's last
retirement. Two prep commits cleared aliases the view could not coexist with: the
context's own dequant pointer arrays (T5.Y1) and `SDeblockingFilter.pLoopf`, written
twice and read never (T5.Y3a). **The flip above it was attempted, compiled whole —
72 errors, five named shapes, all fixed, 342 unit tests and 60 goldens green — and
reverted on a Miri verdict** that is a class rather than a list: *every raw pointer
an accessor derives from the context dies at the next call that takes the context*.
Three instances in one probe run, the last of them two accessor results in one call,
each invalidating the other. **So the flip's blocker is the accessors' return types,
not the signatures** — session Z's face 0, with the size measured at the revert (60
bindings, ~180 call sites). The next brief is a Phase 5 one
([`phase5_session_z.md`](phase5_session_z.md)); `phase6.md` stays unwritten for the
fifth session running, because exit conditions 1–3 are unmet.
**X** = **spent** (10 commits, `4d36cb6c`…`ec672022`;
[`phase5_session_x.md`](phase5_session_x.md)) — scoped as the endgame, and **it did not
close the phase**. Decoder `raw_ptr` **765 → 456 (−40%)**, `SHIM(` **52 → 7**,
deny-clean **9 → 11 of 22**, decoder `unsafe fn` **381 → 327** (439 at V's open). Face 0 landed whole and
the settlement held: `ParseCbfInfoCabac`'s reach is what it said, with one clause the
face corrected (the mb_type is the *picture's*, not the grid's — S24), and
`decode_slice.rs`'s 51 layer pointers fell without `tcoeff_and_rest` and without new
API on the grid. Face 1 took `mv_pred` 94 → 8, `deblocking` 63 → 23 and `nalu` 68 → 49,
and **two of those families were dead rather than convertible** — `SetRectBlock` and
`CopyRectBlock4Cols` had zero callers anywhere in the crate since T5.R5 and were 82 of
`mv_pred`'s raw casts. Face 2 landed whole: the four reconstruction dispatch types take
a `PlaneCursorMut`, 46 Phase-2 strangler wrappers are deleted, and two caches died with
them (`DqLayerState::pPred` and `SWelsDecoderContext::iDecBlockOffsetArray`).
**F52** is the session's finding — a sixth F43-class stub, `CheckAccessUnitBoundaryExt`
returning `true` unconditionally in the module that calls it, shadowing the real
80-line implementation which had no caller at all; **F43's own sweep had run clean over
it twice**, on an off-by-a-brace in its statement counter, and the instrument is fixed
with the finding. **Face 3 did not land, and X's measurement of why is the hand-off's
content**: the flip was attempted, produced 145 errors that reduce to one shape, and was
**reverted rather than half-landed** (§"The context, measured at the face" below).
**F53** is the second finding, and the `exit` battery is what found it: a raw alias
into the layer, sound while its parent was a pointer and UB the moment the parent
became a borrow, **with no line of it edited** — S29's boundary clause met as a Miri
failure for the second session running (T5.W11's was the same sentence one level
down). 26 bindings and 104 use sites re-derived, then a grep sweep for the shape
decoder-wide, which found four more on paths no probe drives.
F3 drew five hits across three batteries and the **sixteenth alternation** acquitted
them — HEAD 5, control 7, 1440 configurations per side. The next brief is a Phase 5 one
([`phase5_session_y.md`](phase5_session_y.md)); `phase6.md` stays unwritten for the
fourth session running, because exit conditions 1–3 are unmet.

**The residual-chain blocker, settled by reading (steward, at `361592a7`).** W
stopped `decode_slice.rs`'s layer conversion on "`&mut` into `grid.scaled_tcoeff`
plus `&mut` whole layer in one call — needs the grid's `mut_and_rest`". Reading
the chain dissolves it: `ParseResidualBlockCabac` uses its layer parameter
**once** — forwarding to `ParseCbfInfoCabac` (`parse_mb_syn_cabac.rs:2966`) —
and `ParseCbfInfoCabac`'s whole reach (`:2669–2733`) is **two scalars
(`iMbXyIndex`, `iMbWidth`), `&mut` on one grid family (`cbf_dc`), and read-only
mb_type**. Nothing in the chain touches `scaled_tcoeff`. So **take-what-you-reach
applies twice down the forward chain** — `ParseCbfInfoCabac` takes the narrow
set, the residual functions drop the layer and forward it — and the caller's
borrows become **disjoint grid fields**, which split-borrow in safe code with no
new API. `tcoeff_and_rest` (PoolRest's shape on the grid) is the fallback only
if a residual-path callee is found reaching `scaled_tcoeff` itself; none read so
far does. The `pDec` operand's dual path is verified at the face (S24).
A probe run per container/file
converted, and **budget it to fire**: session P ran three green, session P′ ran three and
the second convicted `AddShortTermToList` mid-face, session P″ ran three green over two
motion faces and a one-caller lifecycle swap. No perf measurement before W8 (D-gate-1).

**Session P″ costs one more session than it was scoped for**, and the reason is a size:
the hoist is 148 call sites, not 115 (§W3, corrected below), and the flip behind it is 83
consumer sites in the *write* paths — DPB, concealment, the access-unit loop — where the
settlement's own worked example (the decode path) does not apply. The flip is atomic: the
slot type change forces every consumer in one commit, so there is no half of it to land.

The re-plan's "two sessions if P reaches W5" did not hold, and neither did the
correction that replaced it. W2b was **not** two carriers — the layer's `pDec` was a
cache with one stamp site, and it deleted rather than converting — so W2b cost one
session's first two faces rather than a session. What cost the rest of session P′ was
W3's first seam (204 sites) and the discovery in §W3 below. Count is **three work
sessions plus the exit** from here — **superseded 2026-08-14** (Q absorbs what was R),
and **corrected at Q's close**: the flip alone was a session, so the count is
**R (W3's tail + W4 + W5), a second pass (W6 + W7), S (W8)**. The flip's own size is
the lesson worth carrying: the *type* change was six production edits, and the face was
the aliasing question behind it — which no count in this file was measuring.

**W2b's design question — settled by reading the tree (steward, at `c8ebc20f`), and
executed as settled (T5.P′1).** The settlement was right on every clause; the S23 check
it gated is recorded at the deleted field and in session P′'s log §1. Kept here because
the *method* is what W3 needs next: a design settled in writing before the first edit
executed in one pass, where W2b's earlier framing would have opened a 148-function leg.
The choice was false: the layer's `pDec` is a **cache of `dec_pic(pCtx)`** — one
stamp site in the whole decoder (`decoder_core.rs:3704` → `InitDqLayerInfo`
`:3448`), and its null arm is **parse-only mode**, not threading (`GetThreadCount`
≡ 0, `decoder_core.rs:705`). So the field dies rather than converts: readers that
hold `pCtx` derive (the two plane consumers, `decode_slice.rs:2063`/`:2115`,
already take it as their first parameter); the few layer-only leaves
(`PredPSkipMvFromNeighbor` `mv_pred.rs:439`'s shape) take `PPicture` from their
ctx-holding callers or merge into them (D-fid-1); identity compares become `PicId`
equality. **It is not the 148-function leg arriving from the other side — it is a
handful of leaves.** `DqLayerState::pRef` is **dead** — zero readers, zero
writers — and deletes first. One S23 check gates the mechanical pass (the
`pDec = None` mid-AU resets); the P′ brief carries it with sites. The non-null
arm's real target — `SPicture.pMv`/`pRefIndex`, still raw allocations — is W3's,
named in its row.

## W3's design question — settled by reading the tree, executed and corrected (T5.P″2/3)

The surface is **148 call sites, not 115 and not hundreds** — the settled figure was out
by a third and its smallest per-file number was four times the truth, because three of
`deblocking.rs`'s four were **prose** (S16's floor, S24's unit clause; re-derived at the
face and corrected in place per the brief). The tree at `f5a3eac8`: decoder_core 38,
decode_slice 35, manage_dec_ref 24, cabac 17, ctx bodies+helpers 10, cavlc 8, mv_pred 8,
EC 7, deblocking 1. **The design did not move** — the conversion has the same shape at 148
as at 115, which is why the disagreement was logged rather than re-opened.

With owned slots a result stops being a copy (T5.N1's invariant) and becomes
a derivation through the Box's tag — any two live results conflict. So per-use
resolution dies and **accesses move inside brackets**: a scope that borrows the pool
once at its top and threads the resolution down, touching the pool nowhere else
inside. The decode bracket is **the slice** — `dec_pic` and both ref lists are
constant across one slice's MB loop; EC, DPB and output ops bracket per operation.

Execution is the ids-before-ownership maneuver one level up — **derivations move to
bracket tops before brackets become borrows** — in two steps:

1. ~~**The hoist, under `PPicture` slots.**~~ **DONE — T5.P″2 (`d7a1d130`) + T5.P″3
   (`b07e407c`).** Per-use accessor calls became parameters threaded from bracket tops;
   pure motion, byte-identical, three green probes. `PicRefs` is the view the settlement
   asked for, `ref_id` is `ref_pic` split at the line the flip moves, and the decode path
   now reaches the pool at **three** bracket tops and nowhere below them. `MapColToList0`
   is why the view is a resolver rather than a resolved array: it is the one site whose
   handle comes out of another *picture*.
2. ~~**The flip, brackets only.**~~ **DONE — T5.Q1 (`539212bd`) + T5.Q2 (`a8eaa3e5`).**
   All six settled facts held; the type change itself cost **six** production edits, and
   what cost the face was the part no compiler could ask for — *which resolutions may
   coexist*. The answer that scaled: split the resolver family into a shared default and
   `_mut` forms and let the compiler enumerate the writers (ten sites), rather than read
   88 by eye. **F42** is the new finding — a reference list can name the picture being
   decoded, and `PoolRest::get` panics on exactly that slot. The six facts, as executed:
   - `PicRefs::get` returns **`*const SPicture`** — below a bracket top the decode path
     only *reads* a reference, verified by grep over every ref-bound local: zero writes.
   - **The current slot resolves through the mutable picture's own pointer**, never
     through the rest: `PoolRest::get(cur)` *panics*, and a malformed stream can legally
     put the picture being decoded in a reference list, which the C aliases and reads on.
     Same tag, so one borrow, and S6's never-widen default is kept on an ungated path no
     gate here can see.
   - `PoolRest` needs a **hand-written `Copy`** (a derive adds `T: Copy`; `T` is
     `Option<Box<SPicture>>`), or every macroblock-tree signature grows a lifetime.
   - `prefetch_free`/`next_for_thread` return **`Option<PicId>`**, not a pointer.
   - `SPicture::data_ptr` takes `&mut self`; `GetRefPic`'s three calls are the only
     reference-side uses and need a `&self` form.
   - `CreatePicBuff`'s partial-failure arm becomes the `Vec` going out of scope;
     `DestroyPicBuff` becomes F37's reset plus a `drop`, and R4 is discharged by
     construction.
   **83 consumer sites remain** (decoder_core 38, manage_dec_ref 32, error_concealment 10,
   decode_slice 3) and they are the *write* paths, where per-use resolution survives
   wherever the result does not outlive its expression — read each for **span**, not for
   count. F19 closes decoder-side here; F37 is adjudicated here, per its record. The four
   accessors are **deleted** — done-test: their greps read 0 and direct `.pPicBuff` reads
   enumerate to bracket sites only.

W4's colocated falls out of the bracket: inside the slice bracket cur and ref are
already resolved, so `GetColocatedMb` takes both as parameters and T5.N5's
`debug_assert!` becomes `mut_and_rest`'s type-level fact.

**And `pDqLayersList` is W2b again, not W1** (the row undersold it): the list is one
`Box::into_raw` (`decoder_core.rs:2780`) but **`pCurDqLayer` is its cache** — one
production stamp (`:3578`), ~580 uses, of which **81 read the ctx field mid-tree**
(decode_slice 25, decoder_core 15, cabac 12, manage_dec_ref 11, EC 11, rest 7)
while the call tree already threads it as a parameter. The field deletes the way
`pDec`'s did: derive once at the loop top from the owned Box, thread the existing
parameter, the 81 field-reads become param sites. ~~`pTempDec` alone is truly
W1-shaped, and `pPicBuff` → owned is safe **before** the flip~~ — **both DONE, T5.P″1
(`acf5bfd1`)**, and the ordering claim held: under `PPicture` slots a `pool_pic` result's
provenance is `AllocPicture`'s, not the pool `Box`'s.

## Session S's two settlements (steward, at `a158183c` — both by reading, not by fiat)

**W6's deny-blocker, settled.** The 80 `(*pCtx)` dereferences across
`decode_slice.rs`'s 44 raw-context functions touch **24 fields** (census at
`a158183c`; top of the list: `sRawData` 11, `bMbRefConcealed` 11,
`sCabacDecEngine` 7, `pParam` 6, `eIntraPredConstraint` 6, the dequant family 10,
the fn-table family 11). That is not 44 signature rewrites and not an
exception-list surrender — it is **one per-slice view struct**, built by one
`unsafe` constructor per bracket top (the same three tops the pool and layer
already thread from), living in `decoder_context.rs`: field-precise borrows (S29)
packaged as `&mut` for the three state machines (the raw-data reader, the CABAC
engine, the flag/counter set), `&` for tables and config, **copied scalars** where
S23 clears them (constant across one slice — verify per field), and `pParam`'s
scalars copied *inside* the constructor so F41's raw field never escapes it. The
44 functions take the view; `decode_slice.rs` compiles under
`#![deny(unsafe_code)]`; the constructor is the enumerated exception, and it does
not live in `decode_slice.rs`.

**F43's fix plan.** Delete the five stubs and the second, structurally different
`SFmo` (`decoder_core.rs:569`); imports resolve to the real
`error_concealment.rs`/`fmo.rs` bodies; `pFmo` gains an owner (W1's recipe) and
`FmoParamUpdate` is called where the C++ calls it. **Expected gate movement,
verified rather than assumed**: the two concealed-macro goldens carry the
**identical hash today** (`narrow_16x16_n` = `narrow_16x16_idr_lost` =
`754db24b…`) — the tell that concealment never ran — and they regenerate **against
the C++ dylib** (output equivalence's reference), each move logged with the C++
agreement shown. The malformed parity rows run live against the dylib and are
expected to hold (their damage is NAL-level, rejected before EC on both sides); a
row that moves is evidence, logged, not noise. New coverage per F21's rule: a
slice-level-damage asset reaching `DoErrorConSliceCopy`/`MVCopy`, and an FMO
stream (conformance clip or constructed), both **red under re-stubbing
`NeedErrorCon`**. F43's general rule — *a stub shadowing a real implementation is
invisible to every name-matching instrument* — becomes W7's new sweep: every
function name defined in two modules where a caller's own module hosts one
locally, enumerated and resolution-checked.

## W6 decomposed — the closing settlement (steward, at `ba4da8d8`)

**Order superseded (session V, measured at the face)**: the three recipes stand,
the sequence below does not — `deny(unsafe_code)` fires on *calling* an
`unsafe fn`, so the callees convert first, bottom-up, and the view struct is
W6's **last** step (§"The order the decomposition needs", below). Kept unedited
under it because the record of a wrong settlement is part of the record.

U's census stands (202 raw-pointer types over 55 `unsafe fn` at `1423f8eb`), and
its "not one session" reading was right **for conversion in place**. The
decomposition is what makes it one forced session: the 202 collapse under three
recipes the phase has already proven, in order —

1. **The view struct** (settled since session R) kills the 44
   `*mut SWelsDecoderContext` — and removes the *reason* every other parameter
   is raw: S29's objection (a `&mut` param retag popping threaded raw aliases)
   dies with the last `pCtx` parameter.
2. **The reference-flip** — the bracket-borrow maneuver's third application,
   after the pool's (Q) and the layer's (R). The bracket tops already own every
   container, so `*mut DqLayerState` 51, `SWelsNeighAvail` 13, `SSlice` 10,
   `SNalUnit` 8, `BsCursor` 5 — **87 types** — become `&mut`/`&` threaded from
   the same three tops the raw params thread from today. Re-spelling once step 1
   lands; byte-identical; probe per seam.
3. **The plane/block-slice conversion**: the ~38 scalar-pointer types are
   cursors into pictures and the grid, and they are **the same work as 42 of the
   51 `SHIM(phase2)` retirements** — the shims exist to bridge exactly these
   callers to the Phase-2-safe kernels. One family at a time; S6 widths; F35's
   alignment rules.
4. `cabac_rbsp_window` (18 sites, 72 occurrences) rides step 2's threading —
   the window is one more `&[u8]` handed from the slice bracket top.

None of the four is novel. The schedule risk is size alone, which S31 addresses
(compaction is not a stop); if V still splits, the remainder is a **named list of
families**, not an open question.

## The order the decomposition needs — measured at the face (session V, at `3180ac39`)

The three recipes stand. **Their order does not**, and the fact that moves it was
taken by grep at the face rather than from the settlement (S24).

`decode_slice.rs` calls **49 distinct `unsafe fn`s across 10 other modules** — 41 by
fully-qualified path (`parse_mb_syn_cabac` 15, `parse_mb_syn_cavlc` 14, `mv_pred` 3,
`cabac_decoder` 3, `deblocking` 2, and one each in `pic_queue`, `fmo`, `decoder_core`
and `common::deblocking_common`) plus 8 imported from `decoder_context` (`active_fmo`,
`active_pps`, `active_sps`, `cur_and_refs`, `pps_of`, `ref_id`, `sps_of`,
`slice_bit_reader`). `#![deny(unsafe_code)]` fires on an `unsafe` **block**, and
*calling* an `unsafe fn` requires one. **So W6's done-test cannot be written against
one file**: its S20 closure is the transitive callee set, and that set is the decoder
— 16 non-deny modules holding **439 `unsafe fn` and 977 raw-pointer occurrences** at
V's open.

**Step 1 is blocked by the same fact, specifically.** The view struct hands out `&mut`
borrows of context fields; every one of those 49 callees still takes
`pCtx: *mut SWelsDecoderContext` and reaches the same fields through it. A
`&mut (*pCtx).sCabacDecEngine` held across `ParseEndOfSliceCabac(pCtx, …)` is
F24/F25/F28's shape exactly — the parent's use pops the child's retag — and the Miri
probes exist to catch it. The decomposition's sentence is right and its scope was
wrong: S29's objection dies with **the last `pCtx` parameter**, which is a
decoder-wide event, not a `decode_slice.rs` one.

**So the order inverts: the callees convert first, bottom-up, and the view struct is
W6's last step rather than its first.** Nothing else about the decomposition moves —
the reference-flip is still the bracket maneuver, the plane/block-slice conversion is
still 42 of the 51 SHIM retirements, `cabac_rbsp_window` still rides the flip. The
named list of families in dependency order, with a size each, is
[`phase5_session_w.md`](phase5_session_w.md) §1 — which is what "never *the rest*"
means here.

**Executed under the corrected order at V**: `dec_golomb`, `bit_stream` (T5.V3) and
`fmo` (T5.V4) are deny-clean — **6 of 22** — and the first two needed no conversion at
all: a raw duplicate of the safe helper beside it deleted, an accessor moved to the
module it belonged in, five vestigial `unsafe` blocks removed. `fmo`'s conversion is
the recipe in miniature (nine `unsafe fn`, one shape, `Option<&T>` with the null test
kept where it was) and it **found F51** — `UninitFmoList` had no caller where the C++
has one.

## The family queue, marked at session W's close

Sizes are `unsafe fn` / raw-pointer occurrences, re-derived at **X's** close with the
ratchet's own line-anchored unit (S24 — the brief's §1 figures were taken in a
different unit and four of fifteen differ by 1–3; no conversion's shape moved).

| # | family | at V | at X's close | state |
|---|---|---|---|---|
| ~~1~~ | ~~`fmo.rs`~~ | 9 / 6 | 0 / 2 | **DONE, T5.V4** |
| ~~2~~ | ~~`picture.rs`~~ | 2 / 8 | 0 / 6 | **DONE, T5.W1** — lint on; exception is S28's own instrument |
| ~~3~~ | ~~`decode_mb_aux.rs`~~ | 4 / 13 | **0 / 0** | **DONE, T5.X8** — lint on; the 4 `pIdct*Func` shims are deleted, not converted |
| ~~4~~ | ~~`cabac_decoder.rs`~~ | 14 / 12 | **0 / 0** | **DONE, T5.W2** — the cleanest close of the phase |
| ~~5~~ | ~~`pic_queue.rs`~~ | 7 / 41 | 1 / 35 | **DONE, T5.W3** — lint on, two exceptions named (family 14; W3's resolver design) |
| 6 | `error_concealment.rs` | 13 / 32 | 13 / 23 | layer flipped (T5.W8); **context-blocked**, 11 of 13 take `pCtx` |
| 7 | `parse_mb_syn_cabac.rs` | 34 / 45 | 34 / **14** | layer + MV pair flipped (T5.W9/W12); **context-blocked**, 24 of 34 |
| 8 | `parse_mb_syn_cavlc.rs` | 26 / 68 | 22 / 26 | **PARTIAL** T5.W4/W8/W10/W13 — blocked on `SVlcTable`'s varying-length raw sub-tables |
| 9 | `mv_pred.rs` | 22 / 115 | 21 / **8** | **T5.X4** — the two block helpers were **dead** (zero callers since T5.R5) and deleted; residue is 4 `pCtx` + 1 `colocPic` |
| 10 | `deblocking.rs` | 34 / 109 | 33 / **23** | **T5.X5** — BS words de-punned, MV/ref arrays whole, edge filters take `&[u8; 4]`; blocked on `common/`'s kernels and `pCtx` |
| 11 | `manage_dec_ref.rs` | 24 / 57 | 24 / 35 | layer + `SRefPic` flipped (T5.W8/W11); **context-blocked**, 24 of 24 |
| 12 | `nalu.rs` | 23 / 67 | 18 / **49** | **T5.X6/X7** — the parse out-params are borrows and `CheckAccessUnitBoundaryExt` is safe; **F52** found here |
| ~~13~~ | ~~`get_intra_predictor.rs`~~ | 42 / 44 | **0 / 1** | **DONE, T5.X8/X9** — lint on, allowing nothing; the 42 SHIMs deleted |
| 14 | `decoder_core.rs` | 77 / 74 | **75 / 68** | the `expand_picture` bridges and the dispatch installers moved at T5.X8/X11; **context-blocked**, 61 of 75 |
| 15 | `decoder_context.rs` | 28 / 66 | 31 / **59** | the accessors' home and **the view struct's** — converted last, and the enumerated exception |
| 16 | `decode_slice.rs` | 55 / 202 | 55 / **89** | **T5.X1–X3/X8/X10** — the 51 layer pointers, the reconstruction bracket and two out-param families; the view struct is what is left |


**What W measured that re-orders what is left.** The queue's order came from "callee
of `decode_slice`", and the axis that actually gates a family is *how much of it takes
the context*: 24/24, 61/77, 44/55, 31/30, 24/34, 16/23 and 11/13 for families 11, 14,
16, 15, 7, 12 and 6 — against 0/42, 0/4, 3/34, 4/22 and 6/26 for 13, 3, 10, 9 and 8.
**And the better unit is the pointer family, not the module**: the layer, the
neighbour-availability struct, the reference-picture set, the MV pair, the intra-pred
array and the nzc record each converted across every module at once, and the same four
aliasing shapes recurred — a reading the first time and a recognition afterwards.

**The three structural moves are what remains, and each is a face rather than a
family:**

1. **The grid split** — `decode_slice.rs`'s 51 layer pointers stop on exactly one
   thing: the residual path hands `ParseResidualBlockCabac` a `&mut` **into**
   `grid.scaled_tcoeff` *and* a `&mut` to the whole layer, in one call. That wants
   `PoolRest`'s `mut_and_rest` maneuver (T5.Q1) applied to the grid — a disjoint split,
   not a parameter flip. It is the next step of W6 step 2 and it unblocks the rest of
   families 8, 9, 10 and 16 with it.
2. **The plane/dispatch conversion** (step 3) — family 13's 42 SHIMs, family 3's four
   `pIdct*Func` shims, `deblocking`'s eight edge filters and
   `PDeblockingFilterMbFunc`. Takes `SHIM(` 52 → 1.
3. **The context** (family 15's view struct) — 61 `pCtx` parameters in `decoder_core`,
   44 in `decode_slice`, 24 each in `manage_dec_ref` and `parse_mb_syn_cabac`, 16 in
   `nalu`, 11 in `error_concealment`. W showed the mechanism is available already at
   small field counts; what these need is the packaging.

## The context, measured at the face (session X, at `ec672022`)

**The view struct is W6's last step and the only one left, and its blocker is one
shape.** Session X flipped `pCtx: *mut SWelsDecoderContext` → `&mut` across the
decoder to measure it rather than estimate it: **145 errors**, of which 87 are
`is_null()` on a reference (mechanical) and the remainder reduce to this —

> `WelsDecodeSlice`'s bracket top does `let (pDec, pRefs) = cur_and_refs(pCtx);` and
> `PicRefs<'a>` borrows `pPicBuff` for the whole slice. Below it the per-macroblock
> dispatch is `pDecMbFunc(pCtx, dq, pDec, pRefs, pNalCur, uiEosFlag)` — **the context
> whole, beside a live borrow of one of its fields.**

As raw pointers the two coexisted silently; as borrows they cannot. This is S29's
objection arriving exactly where session V said it would, and **the answer is the
settled one**: the bracket top splits the context once and everything below takes
pieces, never `pCtx`. `pPicBuff` is **not** in the view — `PicRefs` and `pDec` already
travel as their own parameters, which is what makes the split expressible at all.

**The size, by grep at `ec672022`**: 220 functions take the context —
`decoder_core` 61, `decode_slice` 44, `decoder_context` 25, `manage_dec_ref` 24,
`parse_mb_syn_cabac` 24, `nalu` 15, `error_concealment` 11, `parse_mb_syn_cavlc` 6,
`mv_pred` 4, `deblocking` 3, `pic_queue` 3.

**Two facts the attempt established and did not land**, both of which compiled:
the CABAC call's three operands are three *disjoint field paths* (`sRawData`,
`sCabacDecEngine`, `pCabacCtx`) once the window borrows the field rather than the
context — `cabac_window_of(&pCtx.sRawData, slice_bit_reader(pCtx))`; and
`slice_bit_reader` needs only `&SWelsDecoderContext`, because the reader lives in the
**NAL**, reached through `pNalCur`'s value.

**And one scope fact, found and deliberately not acted on**: `deblocking.rs`'s eight
edge filters call `common::deblocking_common`'s `unsafe fn` kernels, so the module
cannot carry `#![deny(unsafe_code)]` without either converting those kernels — which
are `common/`, not `src/decoder/` — or an enumerated exception with a Phase pointer.
`decode_mb_aux` and `error_concealment` have the same shape. It is a decision, and it
should be written down rather than settled by whichever is easier when it is met.

## The context, measured twice — sessions X and Y

**X measured the flip and reverted it; Y split the slice off it, landed that, and
reverted the flip again one level deeper.** The two measurements are the same face
seen from two distances, and together they name the whole of what is left.

**What Y landed** (`f82fa258`): `SliceCtx<'a>` in `decoder_context.rs` — the slice's
view of the context as field-precise borrows, with **`pPicBuff` deliberately not
among them** — and `slice_split`, which hands back the pool's two halves *and* the
view out of one borrow. That last part is the correction the face made to its own
settlement: with the context a borrow, taking `cur_and_refs` and then the view is two
mutable borrows of one object, so the split has to happen **inside one function**,
where the disjointness is the compiler's business. Six bracket tops take it
(`WelsDecodeSlice`, `WelsDecodeAndConstructSlice`, `WelsTargetSliceConstruction`,
`ComputeColocatedTemporalScaling`, `CheckRefPicturesComplete`,
`DoErrorConSliceMVCopy`) — not the three the brief named, which were the *layer*
brackets.

**What Y measured and did not land.** The flip of the remaining 153 signatures opens
at **72 errors** (145 before the slice view took the tree out of it), and all of them
reduce to five shapes with fixes that compile: the null guards (28, moved to the
boundary that still holds a pointer), the bracket (2, `slice_split`), the access unit
(8, `pCtx.access_unit.as_deref_mut()` — S29 in safe code), the reference set (13
functions, where a selector travels instead of a `&mut SRefPic`), and the bitstream
window (5 functions, where the offset travels instead of the slice). The tree
compiled and passed 342 unit tests and 60 conformance goldens.

**Miri refused it, and the verdict is the finding**:

> Every raw pointer a `decoder_context` accessor derives from the context — `sps_of`,
> `active_sps`, `pps_of`, `fmo_of`, `dec_pic`, `pool_pic_mut` and their siblings —
> dies at the next call that takes the context. A `Unique` function-entry retag on
> `&mut SWelsDecoderContext` pops the derivation, and the next read through it is
> undefined.

Three instances in one probe run, each more general than the last: a pointer *into*
the context passed beside it (`CheckSpsActive`, fixable by passing the index); a
binding held across one call (`AllocPicBuffOnNewSeqBegin`, fixable by reading two
values); and **two accessor results in one call, each invalidating the other**
(`FmoParamUpdate(fmo_of(pCtx, …), sps_of(pCtx, …), …)`), which no site-local repair
fixes. So the accessors return borrows first — `Option<&SSps>`, the shape
`SliceCtx`'s own methods already have — and then the rule is a compile error at every
site instead of a Miri verdict on the paths a probe happens to drive. **Size at the
revert: 60 bindings of an accessor result into a local** (`manage_dec_ref` 35,
`decoder_core` 15, `deblocking` 3, `decode_slice` 3, `error_concealment` 2, `nalu`
2), against ~180 call sites of the family.

**And the cheap half of what is left is now visible**: the slice view left ~150
`unsafe fn` whose bodies are already safe — `parse_mb_syn_cabac` carries 34 against
**10** raw-pointer occurrences, `mv_pred` 21 against 4, `decode_slice` 54 against 32.
Dropping the keyword module by module is what takes the deny-clean count off 11.

**Per-module sizes at Y's close** (raw-pointer occurrences / `unsafe fn`):
`decoder_core` 67/75, `decoder_context` 54/28, `nalu` 35/18, `decode_slice` 32/54,
`pic_queue` 30/1, `error_concealment` 22/13, `deblocking` 20/33,
`parse_mb_syn_cavlc` 18/22, `manage_dec_ref` 11/24, `parse_mb_syn_cabac` 10/34,
`mv_pred` 4/21. **The two columns have come apart**, and that is the session's
structural news: the modules the slice view converted hold three to five times more
`unsafe fn` than raw pointers, because the keyword outlived the pointers it was there
for.

## The context, closed — sessions X, Y and Z

X flipped the whole decoder and reverted (145 errors, one shape). Y split the
slice off it, landed that, and reverted the flip again on a Miri verdict one
level deeper. **Z removed what the verdict named and landed the flip.** The
crate-wide grep for `pCtx: *mut SWelsDecoderContext` reads **0**.

**What made the third attempt land was a parameter choice, not a return type.**
Face 0's accessors return borrows — that is what Y's hand-off asked for — but the
clause that mattered is that each takes **the field it reaches** (`sSpsPpsCtx`,
`sFmoList`, `pPicBuff`, `sRefPic`, `pDqLayersList`, `pParserBsInfo`,
`access_unit`) rather than the context. A whole-context borrow satisfies the
return-type requirement and is still wrong: it conflicts with every disjoint
field a caller touches beside the result, so the flip would have gained one error
per accessor site — roughly 180 — instead of none. It is also the only shape that
repairs the call Y's verdict called unfixable: `FmoParamUpdate(fmo_of(…),
sps_of(…), pps_of(…), &mut iActiveFmoNum)` is four disjoint field borrows.

**The flip's own measurement**, for the record: 138 signatures, **79** errors at
the open — 62 `is_null()` on a reference, 11 api-boundary type mismatches, **5
genuine borrow conflicts**, 1 unbounded lifetime — closing on Y's five shapes.
Two of them cost more than the brief implied. `slice_split` **was not in the
tree**: this file and session Z's brief both said Y landed it, and it was part of
Y's *reverted* flip (S24, aimed at our own documents). And the reference set's
thirteen functions **cannot be converted mechanically** — a blind rewrite catches
`pRefPicMarking` and `pRefPicListReorderSyn` and lands the helper where neither
name is in scope, turning 27 errors into 113.

**Miri's two findings were ordering, not aliasing**, which is T5.O8's sentence
confirmed at scale: `slice_ctx` called `GetThreadCount(pCtx)` in the middle of
its struct literal, and that function had taken `&mut` for a parameter unused
since F36 stubbed it — a `Unique` retag over the whole context, popping every
field borrow above it. And `ParseDecRefPicMarking` took an `SSps` pointer its
*caller* derived from the context it passes beside it: Y's verdict arriving from
a hand-written alias rather than from an accessor.

**What is left is no longer blocked by any of this.** Decoder `unsafe fn` 230 and
`raw_ptr` 310 at Z's close, and the two are proportional again — the vestigial
keyword is gone (face 2 shed 68 of it by strip-and-build), so what remains is
genuinely raw: the layer bracket (`PDqLayer`), the parse tree's `PNalUnit` /
`PSliceHeader`, and `common/`'s kernels. The order and sizes are
[`phase5_session_aa.md`](phase5_session_aa.md) §1.


## `PPicture` — the family the metric cannot see (session AA, at `2de8703f`)

**Measured, because the brief's largest family was not the largest.** `PDqLayer`
read 16 signatures and converted in one seam (T5.AA1); `PPicture` read **78
signatures over 10 modules** at the face, and **64 at the session's close** after
`deblocking.rs`'s eleven and `manage_dec_ref`'s three converted. No brief has named it because
`raw_ptr` counts `*mut `/`*const ` *spellings* and `PPicture` is a `type` alias — the
whole family reads **1** in the instrument, at the line that declares it. S16's
prose-floor clause arriving from the other side: a metric can have a *hidden* floor
as well as a visible one, and this one has hidden the phase's largest remaining
conversion for six sessions.

**Two halves, and only one is blocked.**

*41 of the remaining 64 do not carry `PicRefs` beside the picture* and convert the
way `deblocking.rs`'s eleven did at T5.AA2 — `decoder_context::pic_split` is the
bracket, `(Option<&mut SPicture>, SliceCtx)`, sound wherever the scope resolves no
reference pictures. Per module: `decode_slice` 18, `mv_pred` 12,
`parse_mb_syn_cabac` 6, `parse_mb_syn_cavlc` 3, `manage_dec_ref` 2.

*23 carry both, and there the raw pointer is load-bearing.*
`PicPool::cur_and_rest` hands the current picture as `*mut SPicture` and the rest as
`PicRefs`; `PicRefs::get` answers the **current** slot from `cur_ptr`, a pointer
sharing the mutable half's tag, because a malformed stream can legally put the
picture being decoded into a reference list and the C++ resolves it and reads on
(**F42**; `PoolRest::get` panics on that slot). Sharing a tag is what makes the
aliasing sound. As a borrow, every function-entry retag on the picture pops
`cur_ptr` and the next read through the F42 arm is UB — session Y's verdict, one
container over, with the pointer this time *earning* its place. `mv_pred`'s
strip-and-build prices it: **470 errors, every one a dereference of `pDec`.**

**The three ways out, none of which a session may pick** (`phase5_session_ab.md`
§0): retire the F42 arm — a behaviour change on malformed input, which S6's
never-widen default forbids; give the planes interior mutability — a design change
that reaches the encoder's picture type; or make `PPicture` the phase's **second
enumerated survivor** with a Phase pointer, which exit condition 2 already admits in
shape and exit condition 1 would have to name the way it names `data_ptr`.

**RULED and EXECUTED — option 3** (steward at `6b6dd9a3`; session AB, T5.AB1).

The family's own grep, which exit condition 1 needs because an alias spelling is
invisible to `raw_ptr`, is the **signature** unit: a `fn` name to the `{` or `;`
that ends the declaration, parens balanced. It reads **69 at AB's open and 30 at
its close**, and the 30 are all of the survivor and none of anything else —

| | at AB's open | at AB's close | what they are |
|---|---|---|---|
| convertible consumers | 39 | **0** | converted, three borrow shapes, T5.AB1 |
| survivors (`PicRefs` beside the picture) | 23 | **23** | `#[allow(unsafe_code)]` at the item, F42 argument written there |
| producers | 7 | **7** | `cur_and_rest`, `slot_at_mut`, `cur_and_refs`, `pic_and_refs`, `slice_split`, the two thread prefetches — the survivor's own home |

Per module, survivors: `decode_slice` 16, `mv_pred` 2, `parse_mb_syn_cabac` 2,
`parse_mb_syn_cavlc` 2, `error_concealment` 1.

**Two of the brief's own figures were wrong and both are S16's floor** (session AB,
at the face): `manage_dec_ref`'s "2 unblocked" are doc comments quoting the C++
signature, and the 7 producers were not counted at all because they live in the
type's own module. One counts prose as code; the other omits code for living
next to the definition.

**The revisit is Phase 8's**, in its inheritance list: retire the F42 arm, or give
the planes interior mutability. Neither is a lint question — the first is a parity
question on the input class D-par-1 spent three sessions bringing to 2707/0, and it
stays **Eugene-level whenever proposed**.

## Phase exit conditions (the definition of done)

**Status at session AB's close** (`11002e4c`): **1 unmet, and measured rather
than estimated** — decoder `raw_ptr` **276**, of which **224 are code and 52 are
prose** (S16's floor, counted this session); nine modules hold the 224, and
`PPicture`'s own grep reads **30**, all of them the enumerated survivor or its
producers. **2 unmet** (**13 of 22** modules deny-clean — `mv_pred.rs` added at
T5.AB1 with three exceptions named at their items). **3 met in substance, and the
list is restated below** (`SHIM(` **3**: one prose tombstone in `decode_mb_aux.rs`
and `SPicture::data_ptr` with its shared form `data_ptr_ref` — the real remainder
is **0**). **4 met** (`exit` battery at `11002e4c` **13 passed / 0 failed / 1
skipped**: tests 474/468/20, census 59, ratchet clean, both benches
bit-identical, **both sweeps 341/341**, Miri `--lib` 329/0 plus 20/7/3). **5 met
and closed** (D-perf-6; AB's own span is flat inside its null). **6 unmet** and
deliberately so — `phase6.md` is written at the exit, and the phase has not
exited.

**The survivor list, restated (W7's closure, session AB).** It is **two accessor
forms and one family**:

1. `SPicture::data_ptr` and `SPicture::data_ptr_ref` — logical `(0, 0)` of a
   plane as a raw pointer, for the kernels that still take a pointer and a
   stride. One `&mut self` form and one `&self` form, the second added because a
   reference resolves out of `PoolRest`. **Phase 8's**, with the output contract.
2. **`PPicture` (= `*mut SPicture`) at 23 signatures**, ruled at `6b6dd9a3` and
   executed at T5.AB1 — each carrying `#[allow(unsafe_code)]` at the item with
   the F42 argument. **Counted by its own grep** (the signature unit, §"`PPicture`"
   above), because `raw_ptr` counts spellings and this one is a `type` alias.
   **Phase 8's**, with the option-1/2 revisit.

Everything else in a `SHIM(`/`raw_ptr` grep of `src/decoder/` is prose or is
still to convert.

1. Decoder `raw_ptr` ≈ 0: every occurrence in `src/decoder/` is on the survivor
   list or is prose — and **the two survivors are counted by different greps**
   (steward's ruling at `6b6dd9a3`, executed at T5.AB1). `data_ptr`/`data_ptr_ref`
   are `raw_ptr` occurrences and fall out of that count. **`PPicture` does not**:
   it is a `type` alias, so the whole family reads **1** in the instrument, at the
   line that declares it — the condition is met for it when its **own signature
   grep** reads the enumerated 23 and nothing else. Report both numbers, and
   report `raw_ptr` split into code and prose (S16's floor is not a rounding
   error here: 52 of 276 at AB's close).
2. Every decoder module `#![deny(unsafe_code)]`, exceptions enumerated with
   Phase 8 pointers.
3. `SHIM(` decoder share = the survivor list exactly; census green; F40-class
   sweep run with its results recorded.
4. Gates: full battery `OVERALL: PASS` at `exit` level — the decoder conformance
   goldens (**58 asset rows** at session T, re-counted there: 55 `asset_test!` + 3
   `asset_test_concealed!`; this line read "57" from before session S added the
   damaged-input and FMO rows) none ever moved, sweeps 341/341 both profiles,
   benches bit-identical, Miri both probes, the widened `exit`-level Miri targets
   (S22: the backlog check).
5. The perf adjudication done and written: stop-line verdict, niche verdict,
   ledger reconciled toward the recovery expectation — or the escalation table in
   front of Eugene.
6. `prompts/phase6.md` written; §0 refreshed; open findings each carry an owner
   (expected open set: F3→Phase 7, F23/F38-class/F41 + the `api/` inventory →
   Phase 8, F36→decoder-threading-or-deletion, F4/F6–F14 survivors per file).

## Standing constraints (unchanged by D-fid-1)

Output equivalence per commit (goldens/sweeps/benches — the C++-as-reference).
No golden movement. No `get_unchecked` (S8). No window hoisting (S8 #4). No
pool/threading edits (F12/P10 — encoder side). F3 per S14. The shell is the
constructor (session O's correction).
