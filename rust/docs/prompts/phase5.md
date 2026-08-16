# Phase 5 — decoder structural rewrite: the remainder, as a closed checklist

Re-planned 2026-08-13 under **D-fid-1** (structural fidelity to the C++ retired;
output equivalence — goldens, sweeps, conformance — remains the correctness
definition) and **D-gate-1** (sprint gating). Rules: plan §7.6; per-session scope
is the S20 closure; S24 at every face open. History: sessions A–O's record is the
log and git — this file carries only what remains.

**The anti-circles contract.** This checklist is **closed**: work enters it only
via an F-finding or Eugene. Every session's commits map to W-items in its log
entry. The progress metric is monotone and grep-able — **decoder `raw_ptr`
occurrences 767** (1283 at the re-plan, 1237 at session R's open, 980 at V's, 974 at W's open), → ~0 at exit
(excluding the named
survivor and prose); a session that closes no W-item and moves no metric is a stall
and says so — **and a W-item can close while the metric stands still**, which is
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
| W6 | **OPEN, and session U re-derived its size: the settlement was out by 4.5x** (S24, at the face). The settled design said "80 `(*pCtx)` dereferences over 44 functions, touching 24 fields" and that is what made W6 look like one face. Measured at `1423f8eb`: the file holds **202 raw-pointer types over 55 `unsafe fn`**, and `deny(unsafe_code)` fires on *every* `unsafe fn` and `unsafe` block, not only on the context ones. The view struct addresses **44 of 202** — `*mut DqLayerState` is 51, the plane/block pointers 38, `SWelsNeighAvail` 13, `SSlice` 10, `SNalUnit` 8, `BsCursor` 5. **The settlement's own census was also wrong at the moment it was written**: at `a158183c` the file held **86 derefs over 30 fields**, not 80 over 24; today it is 80 over 29, and the one field that went away is `pFmo` (F43's fix gave it an owner). The design does not move — it is still one per-slice view struct, built by one `unsafe` constructor per bracket top — but **the face does**: W6 is not one session, and a brief that schedules it as one is wrong before it starts (S20's clause). `cabac_rbsp_window`'s retirement was sized too: **18 call sites, one per function, 72 occurrences with their callers**, and the window must thread from the slice bracket top through the whole per-MB dispatch — W3's-hoist scale on its own. ~~Prior:~~ **DEFERRED (session S, Eugene's direction: parity first, safety refactoring later; still deferred through T, whose scope was parity)** — the design is settled below and unexecuted. 5.6: `decode_slice.rs` per P1 — **NZC cache DONE** (T5.R7, `fb079758`: `&mut [u8; 48]`, 18 signatures, 92 uses, three dead dispatch typedefs deleted); **F31's redundant zeroing DONE** (T5.R8, `5fbe61a2`). **Remaining**: the EC MC paths; `cabac_rbsp_window`'s retirement (21 sites; it returns `&'a [u8]` with an **unbounded** lifetime synthesized from a raw pointer — S25's shape, and the window is constant across a slice, so it is the bracket maneuver again); the signature leg (**D-fid-1: functions may merge — 148 is an upper bound, not a target**) | the phase's largest file | **The done-test is the closure, not the file** (session V, measured at the face): `decode_slice.rs` deny-clean requires the **49 `unsafe fn`s it calls across 10 modules** to be safe first, so this cell is exit condition 2 wearing one file's name — order in `phase5_session_w.md` §1, view struct last. ~~`decode_slice.rs` compiles under `#![deny(unsafe_code)]`~~ — ~~**BLOCKED, and not by conversion work**~~ (session R): the file holds 80 `(*pCtx)` dereferences over 44 functions taking `pCtx: *mut SWelsDecoderContext` and the lint forbids every one; the way through is the context arriving as a reference, which **T5.G1 removed deliberately** (S29). **No decoder module is deny-clean today** — 0, not "all but this one". Eugene's or the steward's call | W7 |
| W7 | **three items done (session U), the rest is blocked on W6 — and the checklist's own `unblocks` column already said so.** Done: the F43-class resolution sweep (`tools/find_shadowing_stubs.py`, clean); the **F40-class sweep crate-wide** as an instrument (`tools/find_elem_byte_confusion.py`) — **0 suspects over 81 files**, 11 byte-sized hits with the two non-obvious ones hand-read, and the tool proved against the pre-T5.O5 tree where it reports F40 and exits 1; **the first three deny-clean decoder modules** (`vlc_tables.rs`, `parameter_sets.rs`, `slice.rs`), where the phase had **0** — compiler-verified, and proved to bite. **Blocked**: 42 of the 51 retiring SHIMs are in `get_intra_predictor.rs`, and those shims exist *because* their callers hold raw plane pointers — that conversion is W6's, so 52 → 1 cannot precede it. Measured per module at `e6873fe1` (`unsafe fn` / raw-ptr types): `decode_slice` 55/202, `decoder_core` 77/74, `deblocking` 34/109, `mv_pred` 22/115, `parse_mb_syn_cavlc` 26/68, `nalu` 23/67, `decoder_context` 27/65, `manage_dec_ref` 24/57, `parse_mb_syn_cabac` 34/45, `get_intra_predictor` 42/44, `pic_queue` 7/41, `error_concealment` 13/32, `cabac_decoder` 14/12, `decode_mb_aux` 4/13, `fmo` 9/6, `picture` 2/8, `bit_stream` 2/5, `dec_golomb` 1/1. Remaining: 5.2's straggler sweep | sweeps + deletions | SHIM greps match the survivor list exactly; every decoder module deny-clean or on the exception list with its Phase 8 pointer — **9 of 22 carry the lint at session W's close, 8 of them allowing nothing** (V added `dec_golomb`, `bit_stream`, `fmo`; W added `picture`, `cabac_decoder`, and `pic_queue` with two named exceptions) | the exit |
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
turned out to be the **pointer family**, not the module. **Decoder `raw_ptr` 974 → 767
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

Sizes are `unsafe fn` / raw-pointer occurrences, re-derived at W's close with the
ratchet's own line-anchored unit (S24 — the brief's §1 figures were taken in a
different unit and four of fifteen differ by 1–3; no conversion's shape moved).

| # | family | at V | at W's close | state |
|---|---|---|---|---|
| ~~1~~ | ~~`fmo.rs`~~ | 9 / 6 | 0 / 2 | **DONE, T5.V4** |
| ~~2~~ | ~~`picture.rs`~~ | 2 / 8 | 0 / 6 | **DONE, T5.W1** — lint on; exception is S28's own instrument |
| 3 | `decode_mb_aux.rs` | 4 / 13 | 4 / 13 | untouched — its 4 `pIdct*Func` shims are **step 3**'s dispatch change |
| ~~4~~ | ~~`cabac_decoder.rs`~~ | 14 / 12 | **0 / 0** | **DONE, T5.W2** — the cleanest close of the phase |
| ~~5~~ | ~~`pic_queue.rs`~~ | 7 / 41 | 1 / 35 | **DONE, T5.W3** — lint on, two exceptions named (family 14; W3's resolver design) |
| 6 | `error_concealment.rs` | 13 / 32 | 13 / 23 | layer flipped (T5.W8); **context-blocked**, 11 of 13 take `pCtx` |
| 7 | `parse_mb_syn_cabac.rs` | 34 / 45 | 34 / **14** | layer + MV pair flipped (T5.W9/W12); **context-blocked**, 24 of 34 |
| 8 | `parse_mb_syn_cavlc.rs` | 26 / 68 | 22 / 26 | **PARTIAL** T5.W4/W8/W10/W13 — blocked on `SVlcTable`'s varying-length raw sub-tables |
| 9 | `mv_pred.rs` | 22 / 115 | 22 / 94 | layer + MV pair flipped (T5.W6/W12); its residue is **`as *mut u32` casts in W4's settled block helpers**, not a signature surface |
| 10 | `deblocking.rs` | 34 / 109 | 33 / 63 | **PARTIAL** T5.W5/W7/W14 — blocked on planes (step 3) and `pDec` |
| 11 | `manage_dec_ref.rs` | 24 / 57 | 24 / 37 | layer + `SRefPic` flipped (T5.W8/W11); **context-blocked**, 24 of 24 |
| 12 | `nalu.rs` | 23 / 67 | 23 / 68 | untouched — **context-blocked**, 16 of 23 |
| 13 | `get_intra_predictor.rs` | 42 / 44 | 42 / 44 | untouched — **the 42 retiring SHIMs, step 3**, and 0 of 42 take `pCtx` |
| 14 | `decoder_core.rs` | 77 / 74 | 77 / 74 | untouched — **context-blocked**, 61 of 77 |
| 15 | `decoder_context.rs` | 28 / 66 | 31 / 66 | the accessors' home — **grew by three** as families 4 and 5 sent theirs here, by design |
| 16 | `decode_slice.rs` | 55 / 202 | 55 / 184 | the view struct + the reference-flip, last. **Its 51 layer pointers need the grid split** (below) |


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

## Phase exit conditions (the definition of done)

1. Decoder `raw_ptr` ≈ 0: every occurrence in `src/decoder/` is on the survivor
   list (the output-contract consumer) or is prose.
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
