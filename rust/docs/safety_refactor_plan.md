# Eliminating raw pointers and `unsafe` from the openh264 Rust port

**Status:** proposal, 2026-08-07 (rev 2: drop-in C ABI is now a requirement — see §2.2.8, D2 resolved). Written against branch `rust3` @ `eb463dbd`.
**Scope:** everything under `rust/crates/openh264-rs/` (src, tests, benches) plus `rust/tools/diffharness/`.
**Goal:** the crate builds as a **drop-in replacement for libopenh264** — same exported symbols, same vtable-based `ISVCEncoder`/`ISVCDecoder` object model, same `#[repr(C)]` structs — while *everything behind that boundary* is safe Rust: zero raw pointers, zero `unsafe`, zero `unsafe impl Send/Sync` anywhere except the single FFI boundary module (`src/api/`), which shrinks to a thin, auditable translation layer. Every byte-exactness gate that exists today keeps passing at every intermediate step.

---

## 0. Status — read this first

*Maintained at every phase exit. One screen; everything below §1 is the original
proposal and its accumulated history, and stays as written.*

| | |
|---|---|
| **Phase 4b complete** | 2026-08-11, sessions A–C. **Every dispatch family this phase set out to convert is converted.** Session A: T4b.1 (`08b7c29d`), T4b.1b (`3e583b9a`) — thirteen `Option<fn>` slots became two enums. Session B: T4b.2a (`d6c78c1b`), T4b.2b (`be67a754`) deleted both strategy vtables (27 entries, 5 static instances, 38 thunks); T4b.3a (`33b1f0f3`) deleted the intra-pred constraint family and 19 of the crate's 21 `transmute` calls. Session C: T4b.3b (`d1c1a7d4`) took **the last two** — which turned out to reinterpret a type into itself — and with them the whole `SExpandPicFunc` table across *both* codecs (S18); T4b.3c (`f2e3c5af`) took `sBlockFunc`, a struct the port had **declared twice** and bridged with a double cast doing the same job as the transmutes. **Read the ratchet, not the size assert**: `assert_size!(SWelsFuncPtrList)` 1272 → 1184 → **1160**, and it sat unmoved through three seams that deleted two vtables and 25 thunks, because `Option<Box<_>>` is pointer-sized — it moved only when an embedded 24-byte struct left. Across the phase: `raw_ptr` 5001 → **4815**, `unsafe_fn` 1286 → **1250**, **`transmute` 23 → 4, and all four are prose — the crate contains zero calls**. Two findings fixed (F19, F21). Brief: [`prompts/archive/phase4b.md`](prompts/archive/phase4b.md), superseded-historical |
| **Phase 3 complete** | 2026-08-11, sessions A–F. **The bitstream layer contains no pointer cursor on either side.** T3.0–T3.3 closed the decoder read side, T3.4 closed the encoder write side and deleted `SBitStringAux` (§1.2's emblem for T3), T3.5 converted the CABAC coder's own triple — the last one — and T3.6 gave the frame output owned `Vec` buffers, retiring the first encoder free-cascade entries. Brief: [`prompts/archive/phase3.md`](prompts/archive/phase3.md), superseded-historical |
| **Phase 5 COMPLETE** | 2026-08-17, sessions A–AC (29 sessions). **The decoder's structural rewrite is done and every file of `src/decoder/` carries `#![deny(unsafe_code)]` — 22 of 22, where the phase began at 0.** Decoder `raw_ptr` **1283 → 236** (173 code + 63 prose) and `unsafe fn` **~1400 → 42**; `SHIM(` **3** with a real remainder of 0; `PPicture` **30** by its own signature grep. **Exit conditions 2–6 met; condition 1 is met by the done-test's definition — *every occurrence converts, or is enumerated with its argument and its Phase pointer* — and NOT by its own ≈0 reading, and `phase5.md` states both lines rather than one.** The 173 code occurrences that remain are inside **168 items carrying `#[allow(unsafe_code)]`, in five families, all owned by Phase 8**: `PPicture` (F42, 23 ruled survivors), the parse tree's raw slots (a Miri verdict, not an omission), the api boundary (twelve context fields reached through **two** accessors), `sMCRefMember`, and the zeroed shells. **The parity work is the phase's other half and it holds**: the malformed corpus reads **2707 output / 2707 codes agreeing** against the C++ referee where session S opened at 641/541, after F43–F51 — four subsystems that had never run, a `DECODING_STATE` that could not hold a bit set, and real CAVLC UB no probe reached. **`exit` battery `OVERALL: PASS` 13/0/1** at `5ebaf904`, both sweeps 341/341 with no F3 hit, Miri `--lib` 330/0 plus 20/7/3, conformance goldens never moved. **Perf: D-perf-6 stands** — cumulative CB ≈ **+24.3…+24.9%** against a ≈+23% stop-line (breached, dispositioned to the Phase 9 perf pass), D-perf-4's +25% *median* tripwire unbreached; AC's own span is flat but for the one CAVLC row, with the mechanism named and not claimed. **What the phase is worth carrying for is the method**, and `phase6.md` §1 is that: the ordering rule (aliases become ids before their container owns), cache-not-carrier, take-what-you-reach, the bracket maneuver (five applications), **the pointer family as the working unit rather than the module**, the vestigial-keyword sweep run whole, **strip-and-build as the enumerator**, settlements in writing, and the decision ladder. |
| **Phase 5b COMPLETE** | 2026-08-17/18, sessions A–D. **`src/decoder/` is safe, and the one thing the phase parked is closed.** **Session D** (2026-08-18, `318c9f87`/`00a702ed`) ran two faces. **F56 was ruled by the steward and fixed** — `None` is the faithful value at all three sites, the `Some(SpsRef { id: 0 })` having been `Option`'s niche in a `bool` rather than a transcription — and it landed **only with the referee beside it**: corpus **2690/17 + 2707/0 over 2707 rows** and conformance **60/60**, unmoved, run before the commit and again at HEAD; the byte comparison reads **573,574 of 573,576** with the two differing offsets attributed by `offset_of!`, three covering tests are **measured** red under their own reverts, and the `else` arm the fix unblocks is instrumented at **0 hits** across all three stream suites. **The lesson is the method**: no gate this project owns can see this class, so the instrument is the byte comparison and the decision is a ruling, never a default. **Face 1 deleted the dead pointer spellings** — `sMCRefMember`, `CopySpsPps` with `PWelsDecoderContext`, `PicPool::slot_at`/`slot_ptr`, the second `PDeblockingFilterMbFunc` declaration, `DestroyPicBuff`'s dead `pMa`, and the write-only `SSps::pSLevelLimits` — taking decoder `raw_ptr` **131 → 113 (47 code + 66 prose)** and leaving a residue **enumerated by category** rather than zeroed: 16 api-owned fields and accessors, 6 the log callback's C shape, 10 values crossing the C ABI or the pool's machinery, 15 test fixtures feeding the first group. `exit` battery **`OVERALL: PASS` 13/0/1**. Brief: [`prompts/phase5b_session_d.md`](prompts/phase5b_session_d.md). *Sessions A–C, below, are the phase's body.* **`src/decoder/` is safe.** The phase took the decoder from Phase 5's **167 `#[allow(unsafe_code)]` items / 160 `unsafe {` blocks / 42 `unsafe fn` / 236 `raw_ptr`** to **3 / 6 / 0 / 131** (67 code + 64 prose), with `#![deny(unsafe_code)]` on all 22 modules throughout. **All five of AC's enumerated families closed**, each by conversion rather than by a better-argued exception: **F42/`PPicture`** and **`sMCRefMember`** at T5b.1/T5b.2 — the arm stops being an address and becomes an identity (`PicRefs::resolve`, plus a same-picture MC kernel family), which is *neither* of the two options Phase 8 had reserved; **the parse tree** at T5b.3 — the access unit's slots own, and both stored aliases are indices; **the zeroed shells** at T5b.4/T5b.5; **the api boundary** at T5b.6. What is left is three items in two files: `api_alias`/`api_alias_mut` (the twelve api-owned context fields, **Phase 8's** with F23/F41) and `SPicture::data_ptr` with the one Miri test S28 mandates for it — its single production use is `DecodeFrameConstruction`'s three `ppDst[i]` writes across the C ABI. **Three results are worth carrying forward.** (1) **F55/S34** — *an index is not a pointer under a container that permutes its slots*: the access unit's two rotations left the slice-header index naming the successor's NAL, took 11 of 60 conformance assets, and no line of the diff was near the swap; the corollary is that **when frame counts hold and hashes move, compare the hash *sets* before calling it a pixel divergence** — session A spent a face on a "corruption" that was a reordering. (2) **The shells' recipe, and its measured premise**: `size_of::<SWelsDecoderContext>()` is **573,576 bytes** against a doc comment that said "several MiB" and concluded a by-value constructor was impossible at any point in the phase — Phase 5's owned containers had spent that size years of prose ago. What replaces a shell is a `memset_zero` beside the `Default` on every type whose `Default` is deliberately *not* its zero (seven of them here), each field's zero *meaning* written out, and a **temporary byte-comparison test against the shell it replaces** — which is how the equivalence became a measurement: 573,574 of 573,576 bytes identical, the two exceptions attributed by `offset_of!`. (3) **F56, open and parked** — F54's niche artifact on two fields F54 did not reach, `active_sps` and every fresh NAL node, both reading `Some(SpsRef { id: 0 })` where the C memsets a null. It is parked by the decision ladder's third rung (a behaviour question is never defaulted), transcribed unchanged at both sites, and closes in two lines plus a corpus run. **Gates**: `exit` battery **`OVERALL: FAIL` 12/1/1 at `f5ba2395`, and the one failure is adjudicated to a verdict rather than cited**: the release sweep hit F3's signature, S14's step 0 was *taken and did not acquit* (this window is not decoder-only, so base and head build different `rust_enc` binaries — the first time that has been measured rather than assumed since AC's clause), step 1 re-ran the configuration **5/5 byte-identical**, and an isolated `mt` preset then failed at a *different* configuration. Acquitted as F3, measurement 65. Everything else passes: tests 479/473/20, debug sweep **341/341**, both benches bit-identical, Miri `--lib` **334/0** plus 20/7/3, the three decoder probes **3/3**, conformance **60/60**, corpus **2690/17 + 2707/0** unmoved. **Perf**: the whole 5b window (`5ebaf904` → `f5ba2395`, three sessions as one span per D-gate-1) reads **CB −0.87%** at 7 pairs against a −0.21% null floor — the ledger's own row is *faster* — so cumulative CB falls to ≈ **+23.2…+23.8%** carrying AC's higher exit reading, ≈ **+22.6…+23.2%** carrying its lower; the two CABAC B-frame rows rise ≈1%, with `PicRefs::classify` on every B-slice resolution named as a **candidate, not a claim** (S33). D-perf-4's +25% median tripwire unbreached; D-perf-6's disposition unchanged. Brief: [`prompts/phase5b.md`](prompts/phase5b.md), sessions [B](prompts/phase5b_session_b.md) and [C](prompts/phase5b_session_c.md). |
| **Phase 6 COMPLETE** | 2026-08-20, sessions A–J (ten sessions). **`src/encoder/` and `src/processing/` carry `#![deny(unsafe_code)]` on all 36 modules the phase owns, and every one of the 775 exemptions names the phase that retires it.** That is the deliverable: the encoder's unsafe surface is no longer a number, it is a list with owners — `port-raw(Phase 9)` **595**, `port-raw(Phase 7)` **89**, `cursor` **60**, `MT` **16**, `SCREEN_CONTENT(dormant)` **10**, `C-ABI` **5**, **zero untagged**, and **three modules** (`abi_guard.rs`, `param_svc.rs`, `processing/mod.rs`) carry no allow at all. The fifth category is **D-exit-1** (§7.4, steward, 2026-08-20), ruling F65. **The arc, and the two rows that disagree**: encoder+processing `raw_ptr` **2669 → 1849 (−31%)** while `unsafe_fn` went **710 → 709**. The phase took raw pointers out of struct *fields* — owned `Vec`s, `Box`es, `Option<LayerIdx>` where a pointer used to be — and `deny(unsafe_code)` counts *signatures*. That asymmetry is the phase's own finding, produced twice: **F65** (the sweep's residue is owned by Phases 7–10, so condition 2 could not be met as written) and **F66** (the context conversion cannot be made at all yet). **F66 is session J's main result.** Step 1 was done — 109 signatures (not 117; four of the 113 ctx-only non-MT take `*mut *mut sWelsEncCtx`), root-down in five depth levels, four commits, every one green — and then **reverted**, because a `&mut sWelsEncCtx` function-entry retag invalidates the **whole** context (`[0x0..0x17ee8]`, Miri's own range) where a raw write pops only the offsets it touches: **64 of the 109 have a call site holding a context-derived cursor across the call**, 93 sites in 28 callers, three of them cursor accessors. Miri found 2 of the 93 — that ratio is coverage, not defect rate — and **rustc reports none**. Not fixable by reordering (several such calls reallocate the container), not by keeping the clean 45 (a heuristic detector and an unstable set), and **not by ordering at all** — it is a caller-side property, orthogonal to the root-down rule. Handed to **Phase 9** with its detector, to be run as the precondition and not the post-mortem. **Findings**: F57, F58, F59, F60, F62, F63 fixed; F64 understood and instrumented; **F61 open → Phase 7**; F65 ruled; **F66 open → Phase 9**. Four of the eight defects were found by the `exit` battery's Miri step or the encoder aliasing probe and **none by a byte-level gate**; F62 was found by reading. Briefs: [`prompts/phase6.md`](prompts/phase6.md) and its ten session briefs |
| **Phase 7 in progress** | Session **A** done 2026-08-20 (`b08d6c47..`, 5 commits, one `.rs` file touched, +31/−26). **A's result is a finding, not a conversion: F67.** The phase's design rests on two premises about *scheduling* — index-based disjointness and order-based assembly — and session A verified both in writing. The target shape `s.spawn(|| encode_slice(job, shared))` needs a third thing nobody wrote down: `shared` must cross the spawn, which is a property of the **type**. Measured rather than argued — a reverted probe asserting `sWelsEncCtx: Sync` gives **12 `E0277`s over 12 distinct blocking raw-pointer types**, five of them nested inside types Phases 8 and 10 own. And the **105 tagged items the phase was handed are, item for item, the slice-encode call tree's raw signatures** (`svc_encode_slice` 40, `svc_mode_decision` 15, `svc_enc_slice_segment` 11, `rc` 11, …) with **zero in the three seam files**: the inventory and the blocker are the same object. So **the context split is a precondition of the fork/join, not a consequence**, and the two-session plan had them backwards. Three orderings are on the table in [`prompts/phase7.md`](prompts/phase7.md) §3 and **the decision is the phase owner's, before B starts**. Session A landed instead: **F3's ablation before-arm at 3000 runs, 25 hits, ≈1/120** — *not* the ~1/11 Phase 6 handed over, which was a 9-in-96 sample, so the charter's `P(0/500) ≈ e⁻⁴⁵` goes with it and **B's after-arm must be 2000 runs** (measurement 88); the **eight dead C++ event fields** deleted from `SSliceThreading`, every one proved dead on both sides (T7.A2); and a **`pThreadBsBuffer` leak fixed that has been in the tree since the raw translation** — the C++ frees, the port only nulled, so every `iMultipleThreadIdc > 1` encoder leaked `iCountBsLen` bytes per worker in every build ever shipped (T7.A3). **F68 opened**: `c_vs_rust_bench` never sets `uiSliceMode`, `GetDefaultParams` leaves it `SM_SINGLE_SLICE`, and that is the *first* arm of the slice-mode chain — so **every `[4t]` row in the encoder bench runs the single-threaded path** (four threads buys 0.0–1.4% at every resolution), and the charter's "rebuild the pool only if the span shows spawn cost" can currently be neither met nor refuted. **0 of 105 tags retired, 0 new `unsafe` written, 36 of 38 modules still denied**; span decode **−0.13%** / encode **+0.00%**, no median breach, cumulative deficit unmoved at ≈+15…+17% against the +25% tripwire. F3 measurement 89 acquitted the session's two sweep hits by alternation (HEAD 9/500 vs control 7/500). |
the decoder's picture ownership: `PicPool`'s slots are `Option<Box<SPicture>>`, so the pool is the pictures' one owner,
`AllocPicture`/`FreePicture`'s raw pair is deleted, **F19 closes for the pool and the pictures** and **R4 is discharged by construction**.
The type change cost **six** production edits — W2a/W2b had removed every alias and T5.P″2/3 had moved the decode path to three bracket tops first —
and the face was the question no compiler asks: *which resolutions may coexist*. What settled it was not reading 88 sites but **splitting the resolver
family by capability and letting the compiler enumerate the exceptions**: shared by default, `_mut` where a path writes, ten writers found in one build.
Nine bracket sites take one borrow split into the picture they write and a view of the rest; the tell for four of them is a self-copy guard sitting
*between* two derivations that can name one slot. New finding **F42** (a reference list can name the picture being decoded, and `PoolRest::get` panics
on exactly that slot — answered from the mutable half's own pointer, one tag; covering test red under revert per F21; **no gate this project owns could
have found it**, S6's never-widen default on ungated input). The probe was spent on this seam and is **green** (Miri `--lib` 336/0, 955s).
Session Q was scoped for the flip *plus* W4–W7 under Eugene's forcing rules (2026-08-14) and **ended on context with W4–W7 untouched**, which the
hand-off names per rule 1. W3's tail is `pDqLayersList` + `pCurDqLayer`'s 81 mid-tree reads, `fmo.rs`'s map (25 sites; `TagFmo` is `repr(C) Copy`
inside an array — S21), and the parse-only parser buffers (read F41's boundary note first). Decoder `raw_ptr` **1237**, and S16's least flattering
reading yet — the flip moved the metric *up*, because the resolvers spell `*const`/`*mut` where they spelled the `PPicture` alias, and an alias is
invisible to an instrument counting pointer types written. **5.1 and 5.4 are CLOSED** (session N). 5.1's deferred half landed as `PicPool` (T5.N1 — `SPicBuff`'s `ppPic`/`iCapacity`/`iCurrentIdx` triple becomes one `Pool<PPicture>` plus a cursor, the recycling predicate becomes a method, and the C++'s partial-failure free path stops leaking) and `PicId` (T5.N2 — `SPicture` carries the slot it was allocated into, stamped once by the pool, and `same_picture` is plan P3's predicate over it; **not** `iPicBuffIdx`, which the C writes at prefetch and the reordering path reads). 5.4 closed with T5.N3 — **`pCsData`, the decoder's last plane-pointer mirror, died with nothing replacing it**, because its three readers already take the layer that carries the picture — and T5.N4, which put both reference lists into the deblocking filter as `PicId`s and deleted the `c_void` erasure the 4x4 paths used. **The scheduled third face did not land and the reason is structural**: the colocated reads need `Pool::mut_and_rest`, and a pool-issued `&mut` invalidates every raw `pDec` alias under stacked borrows — F24/F25/F28's class, installed deliberately. So **the split-borrow API waits on `pDec` carrying a `PicId`: 236 `.pDec` sites, all in `src/decoder/`** (77 `decode_slice.rs`, 55 `decoder_core.rs`), plus 94 `pRefList` and 209 `pRefPic` — a step, not a face, and 5.3's colocated work plus 5.3b sit behind it. What did land is that face's S25 enumeration as a `debug_assert!` (T5.N5), green across 341/341 in both profiles. **Perf moved and this is the first Phase 5 session that is not free**: the whole span read **+1.24% CB** at 7 pairs against a null band of −0.14%…+0.34%, every decode row over the ceiling, so **cumulative CB ≈ +21.6…+21.9%** and headroom falls from ≈2.3 to **≈1.1…1.4 points** for 5.5 and 5.6. S5's bisect puts **all** of it in Face 3 (+1.17/+2.25/+2.06%) and **none** in Face 1 (−0.72% CB, CABAC rows inside the band); the B-slice asymmetry points at T5.N4's `Option<PicId>` representation, unverified, with the one-build experiment written down. **The reading owes a day-two confirmation and it is session O's first item.** New finding **F37** (`DestroyPicBuff` does not reset the reordering buffers where the C++ does — a re-init leaves `sPictInfoList` naming slots of a freed pool; owner 5.5/Phase 8). **5.2's structural work is done** (session M): all 22 array families are on `MbGrid`, the layer is `DqLayerState`, its 45 scratch-cache parameters are borrows, and `pBitStringAux` — the last pointer the layer cached beside its owner — is gone; what 5.2 still owes is its straggler sweep and one family (the `*mut u8` non-zero-count caches, 96 of whose 167 uses are in `decode_slice.rs`, so 5.6's by P1). **5.3a is done and F22 is closed** (T5.M4): eight duplicate function pairs became one each, home = the C++'s home, resolved per function rather than per module — and it closed on the signature convergence T5.M2 had produced the same morning, exactly as the finding predicted it would. 5.3 still owes 5.3b (279 punning sites in `mv_pred.rs`, `SetRectBlock` onto the grid) and the colocated reads. **The perf position is settled and no longer provisional**: the whole 5.2 flip read **+2.93%** CB on day one and **+2.57%** on day two against a null band 0.17 points wide, so cumulative CB is **≈ +20.4…+20.7%** against the ≈+23% stop-line — **≈2.3 points of headroom** for all of 5.3–5.6, and it is not re-derived from per-family sums again. Session M's own four faces read **−0.77% CB as one span**, below the null floor on every row. **The two-sided lesson about the harness is now complete**: one family read three times gave +0.03%, +0.27%, −0.13% — disagreeing in sign — while the 22-family span read twice on two days agreed row by row; only the size of the measured thing changed. Historical detail from sessions A–J follows. 5.1 is closed but for `PicPool`/identity (deferred, accepted 2026-08-11); **5.2 is past half and executing Eugene's direction — probe first, then flip** (D-perf-5's return, §7.4). **Session J did both halves.** The probe: `grid_48x32.264`, 3x2 macroblocks with CABAC, the 8x8 transform and B slices, is a second un-ignored Miri gate (T5.J2), coverage **proven** the F21 way — revert T5.I2 and it goes red at `parse_mb_syn_cabac.rs:1242` while the old one-macroblock probe stays green. It **found a real UB on its first run**: F35, a 2-aligned `[i16; 2]` punned as an `i32` across the B-direct MV paths, plus the two block helpers that were legal only because `WelsMallocz` over-aligns and would have become UB at the next flip (T5.J1). Both invisible to every byte gate — an unaligned load is the same instruction here — and unreachable by a stream with no B slices and no neighbours. The flip: **15 of 22 families are on the grid** — `pRefIndex` (T5.J3), then session K's `pMv`, `pMbType` and `pSliceIdc` (T5.K1–T5.K3), including the single hottest family by profile (`pMv`, 3.9%) — **and every one of them reads as free**. **Seven remain, ordered by re-derived heat in the brief; re-derive again, because the order moved between J and K.** **Session K's day-two confirmation is the result that governs what follows, and it is about the instrument.** Three 7-pair readings of T5.J3's one span read +0.03%, +0.27% and −0.13% — disagreeing in sign — against three session null bands that disagree at least as much (+0.13…+0.45%, −0.16…−0.02%, −0.22…+0.25%). **A per-family cost of ≈0.3% CB — the size that decides whether the remaining families fit under the ≈+23% stop-line — is below this harness's resolution at any reachable pair count.** So the next measurement is the **cumulative** span (pre-flip base → HEAD, ~20%, resolved trivially), not another per-family one; S2b carries the pair-count clause this earned, flagged as session K's proposal rather than a decision. Cumulative CB ≈ +19.2…+20.1%, unmoved. **F36** (dormant): the port's multi-threaded slice loop never writes `pSliceIdc`, which is what every neighbour-availability test compares — dead behind `GetThreadCount() == 0`, owned by whoever ports decoder threading. Brief written at 4b's exit per S19: [`prompts/phase5.md`](prompts/phase5.md). Per-session scope is **the S20 closure, not the file**. The three rules it inherits by name are **S24** (re-grep every shape-deciding count), **S25** (Phase 5 *is* raw-to-borrow conversion, so the re-entrancy audit is planned work) and **F19's class** (a `Box::into_raw` with no live `from_raw` is a leak no gate here can see — 5.5 runs that check decoder-side). Order per D-seq-1: 4b → 5 → 6 → 7 → 8 → 9 |
| **Governing decisions** | **D-perf-4** (§7.4 v3): swap-and-ledger by default, +25% median cumulative tripwire, no optimization boxes, byte-exactness never traded. **D-perf-5** (2026-08-12): recovery-first at 5.2's midpoint — answered, executed, and closed: the flip is measured whole and day-two-confirmed (cumulative CB ≈ +20.4…+20.7%). **D-gate-1** (2026-08-12): **sprint gating until Phase 5's exit** (§7.4) — all mid-phase perf moves to phase exits; per-commit gate is build+tests+ratchet+census; the full battery runs once per session at close; a parallel encoder-only track is authorized, tracks never share a file. **D-seq-1**: 4a ran before Phase 3 — done. **D-perf-3's fallback is now spent, on the decode side only** (see the finding below) |
| **The checkpoint's result** | **Encoder -5.11% median, all 28 usable rows faster or flat, none regressed**; decode -0.19% (flat). Cumulative vs Phase 2's start is now ≈ **+8.9% encoder** (was +14.73% — back under 10% for the first time since T4) and ≈ +17.8 / +10.1 / +9.6% decode (unmoved). [`perf_baseline.md`](perf_baseline.md) §Phase 4a |
| **Phase 3's perf, and its lesson** | **Nothing measurable, on a phase that added bounds checking to both bitstream hot paths.** Encoder **+0.00%** at both pair counts on the span that could have moved it; decode's best-converged number is **−1.93%** (5 pairs, whole phase). **No ledger row opens.** The lesson is bigger than the numbers and is now **S2b**: 3 → 5 pairs moved whole-phase decode from +0.68% to −1.93%, *changing the sign*, on a session whose null floor ran to +22.57% on one encode row. When a median lands outside the null band, **re-run at more pairs before reaching for a mechanism** |
| **The finding that governs Phases 5/6** | **Direct dispatch recovers per-call scaffolding only where the caller supplies constant dimensions.** Encoder call sites pass literal block sizes and recovered; the decoder's arrive as parameters through `BaseMC` (~1300 instructions, not inlinable) and recovered nothing. Measured on two families in both codecs, with the `#[inline]` fixes built and reverted rather than reasoned about. **Every remaining decode ledger row is downgraded to Phase 5**, which is the phase that makes those dimensions static |
| **Ledger** | **No row is `pending`.** Two closed as noise, two downgraded to Phase 5 with the reason, four carried into the aggregate ([`perf_baseline.md`](perf_baseline.md) §Ledger) |
| **Parked** | `common/sad_common.rs` (14) and `encoder/sample.rs` SATD (7) — **re-attempted and re-parked 2026-08-10**, second dated verdict, on a rebuilt harness (1.41x–4.94x against a ≤1.05x bar). One debt owed: SATD still has no measurement of its own |
| **Ratchet** | *(Phase 5b session D, 2026-08-18, `00a702ed`)* `unsafe_fn` **830**, `raw_ptr` **3259**, `unsafe_block` **456**, `mem_zeroed` **27**, `SHIM(` **112**; decoder-only `raw_ptr` **113** (**47 code + 66 prose**), decoder `unsafe_fn` **0**, blocks **6**, allow items **3 by grep**. Session D's face 1 took the code column from 67 to 47 by subtraction only — `sMCRefMember`, `CopySpsPps` with the `PWelsDecoderContext` typedef, `PicPool::slot_at`/`slot_ptr`, the second `PDeblockingFilterMbFunc` declaration (census 59 → 58), `DestroyPicBuff`'s dead `pMa`, and `SSps::pSLevelLimits`, which was write-only. **The 47 that remain are enumerated by category and handed to Phase 8 as a list**: 16 api-owned fields and accessors, 6 the log callback's C shape, 10 values crossing the C ABI or the pool's machinery, 15 unit-test fixtures feeding the first group. *(Prior at session C, `f5ba2395`: 830 / 3277 / 456 / 27 / 112, decoder-only 131 = 67 + 64.)* |
| **Ratchet, the Phase 5b window's shape** | *(Phase 5b session C, 2026-08-17, `f5ba2395`)* `unsafe_fn` **830**, `raw_ptr` **3277**, `unsafe_block` **456**, `mem_zeroed` **27**, `SHIM(` **112**; decoder-only `raw_ptr` **131** (67 code + 64 prose), decoder `unsafe_fn` **0**. The Phase 5b window moved four of these by more than any single Phase 5 session did: `unsafe_block` −159, `unsafe_fn` −44, `raw_ptr` −94. **Read the shape.** The `unsafe_fn` column falls to *zero in `src/decoder/`* while the crate total falls only 44, because the keyword was already almost all vestigial there — 143 blocks and 20 definitions covered **169** sites the compiler actually needed, and strip-and-build is what separated the two. The `raw_ptr` column's decoder share was **64/131 prose** here and is **66/113** after session D — the highest prose fraction the metric has ever had, and that is S16's floor arriving: what is left is comment text recording what a pointer used to be. **One increase is recorded rather than hidden**: `api/codec_api.rs` raw_ptr 226 → 240 and `unsafe_block` 42 → 44, because the three aliasing probes' vtable driver moved there — it drives the ABI through a raw `*mut ISVCDecoder` deliberately (F23), so its `unsafe` is the boundary's. *(Prior: Phase 5 session Y — 1155 / 3520 / 526 / 30 / 114, decoder-only 383; session X — 1156 / 3593 / 531 / 115.)* |
| **Gates** | *(Phase 5b session D, 2026-08-18, `19c0ba48`)* **482 debug / 476 release / 20 ignored** (three covering tests for F56, each measured red under its own revert), census **58** (`PDeblockingFilterMbFunc`'s allowlist entry retired because the duplicate is gone, not silenced), decode conformance **60/60**, corpus **2690/17 output and 2707/0 codes over 2707 rows** unmoved — run twice, before T5b.8's commit and again at the session's HEAD. Close was **`exit` 13 passed / 0 failed / 1 skipped**: Miri `--lib` **337/0** plus the differential targets (**20 / 7 / 3**), both sweeps **341/341** with **no F3 hit in either profile**, both benches bit-identical, ratchet clean. **S14 step 0 was run as a build anyway** and is informational, nothing having hit: base and head build different `rust_enc` binaries in *both* profiles (debug `ef8477b3…` vs `cad0acbd…`, release `6b1be2a4…` vs `50d1e52d…`), so **AC's clause — release can eliminate a decoder-only change — does not reach a window that deletes decoder types and a struct field**. *(Prior: Phase 5b session C — 479/473/20, `exit` 12/1/1 with the one failure adjudicated to F3 measurement 65, Miri 334/0.)* |
| **Gates, session C's close** | *(Phase 5b session C, 2026-08-17, `f5ba2395`)* **479 debug / 473 release / 20 ignored** (two tests deleted with the subjects they instrumented — the dead copy shims' and `data_ptr_ref`'s), census **59**, decode conformance **60/60** with zero golden hash literals changed all session, corpus **2690/17 output and 2707/0 codes** unmoved, both benches bit-identical, ratchet clean. Close was **`exit` 12 passed / 1 failed / 1 skipped**: Miri `--lib` **334/0** plus the three differential targets (**20 / 7 / 3**), debug sweep **341/341**, the three decoder probes **3/3** in 1100.3s, and the one failure a single F3 hit in the release sweep — **adjudicated to a verdict**, not cited: step 0's hash shortcut was taken and did *not* acquit, step 1 re-ran the configuration 5/5 byte-identical, and an isolated `mt` preset then failed at a different configuration (F3 measurement 65). *(Prior: Phase 5 session Y — 481/475/20, `exit` 12/1/1, Miri 336/0; session X — 484/478/20, `exit` 13/0/1.)* |
| **Standing rules** | **§7.6** below, now S1–S43. Phase 3+ briefs cite it rather than copying rules forward. **S20** (a struct conversion's decomposable unit is the signature-reachability closure, not one struct) and **S21** (the construction audit, and a duplicate-family finding must inventory every function per copy) were hoisted out of Phase 3 and are what Phases 5 and 6 are made of; **S22** (a repaired instrument has a backlog — run everything it covers at every level, the session it is repaired) is F17 → F18's lesson; **S23** (a cached table becomes a derived value only if the source cannot change behind the cache — a property of the *update paths*, read one field at a time) and **S23b** (an alternation that returns 0/0 has not run yet — alternate at the level the hits occur) are 4b session A's; **S24** (a count that decides a conversion's *shape* comes from a grep over the definitions, never from a doc or a brief) is 4b session B's, earned by two wrong premises in one brief; **S25** (a raw-to-borrow conversion surfaces pre-existing aliasing — the re-entrancy audit is part of the conversion, enumerated with the S20 closure) is session B's second, hoisted from its log §4, and Phases 5/6 inherit it by name; **S26** (a brief is an execution document — facts, order, gates, non-goals; cite rules by tag, no history retelling, no provenance narration) is Eugene's direction at Phase 5's open. **Section revised 2026-08-11** (Eugene's direction): R-letter tags moved to the preamble map, **S14 refreshed to the measured protocol** (briefs now cite it instead of restating F3), **S17 generalized** — its specific check is automated in `gates.sh` — and S3–S5/S10–S11 tagged **[dormant: kernel work]** until 6.3's re-attempt. Instrument: `rust/tools/perfpair.py`, which implements S1/S2/S17. **S14 gained one step-2 shortcut at Phase 5 session D**: before alternating, hash the sweep binary against the base's — if they match, the alternation is vacuous by construction and the hash is the acquittal (F3, measurements 27–28). Step 2 is unchanged for any session whose commits reach `rust_enc`. |
| **Open findings** | **The open set at Phase 5's close (session AC's exit pass), each with an owner.** **F3** → **Phase 7**, where it closes by ablation: **85 measurements** (sessions E–H added 71–85, all acquitted; alternation and acquittal totals last reconciled in full at session D's close: 22 / 43 then) — **measurement 85 (session H) is the first non-zero-length instance: a short stream at 40124 of 41938 bytes, so the tell is *wrong length*, never just empty output** (reconciled against `phase0_findings.md` at Phase 5b session D's close; session C's measurement 65 was the last, and session D added none — both its sweeps read 341/341); signature unchanged (`mt`, `sm=3`, `t∈{2,4}`, any wrong length, either profile, either clip); **rate re-anchored at Phase 7 session A: ≈1/120 pooled over 3000 runs under saturating load (measurement 88), density-sensitive (89: 1/120 → 1/63)** — the older ≈1/307 was the battery-interleaved reading and the ~1/11 hand-over was a 9-in-96 sample; **89 measurements** at A's close. AC added a protocol clause worth having: **step 0's hash shortcut is profile-dependent** — `rust_enc` never calls the decoder and release eliminates it, so a decoder-only change can hash identically in release where it does not in debug. Hash the profile the hit occurred in. **And do not read the clause as a rule** (session D, measured): a decoder-only window that *deletes types and a struct field* moved the release binary too — base `6b1be2a4…` vs head `50d1e52d…`, one clean build each at one path — so the clause says the shortcut *can* apply in release, never that it does. Hash it; do not predict it. **F23**, **F38-class**, **F41**, the **`src/api/` instrument inventory** (S22's clause — `api/`'s `deny` exemption propagated into every instrument's scope, so all of them run over `api/` before anything is rewired) and **the twelve api-owned decoder-context fields** → **Phase 8**. **The parse tree's raw slots and the zeroed shells are CLOSED at Phase 5b** (`6fe88832`, `1abc379a`) — the slots own and both shells are field-wise constructors. **F56 is CLOSED** (opened session C, **ruled by the steward 2026-08-18 and fixed at `318c9f87`, session D**): F54's niche artifact on two fields F54 did not reach — `SWelsDecoderContext::active_sps` and every fresh NAL node read back as `Some(SpsRef { id: 0 })` where the C memsets a null. `None` is the faithful value at all three sites, and the ruling was made rather than defaulted because the referee suite is what makes a behaviour question answerable: the fix landed **only with the corpus (2690/17 + 2707/0 over 2707 rows) and conformance (60/60) beside it, both unmoved**, the byte comparison reading **573,574 of 573,576** with the two differing offsets attributed by `offset_of!`, and three covering tests each measured red under its own revert. **The arm it unblocks is still unreached — 0 hits across all three stream suites, instrumented** — which is the finding's point: *no gate this project owns can see this class*, so the instrument is the byte comparison against the zero image and the decision is a ruling. Phase 6 inherits that as a method, not as an open item. **`PPicture`'s option-1/2 revisit and `sMCRefMember` are CLOSED at Phase 5b** (`7a4ad7b5`), and the revisit closed by *neither* option: the F42 arm was answered by **identity** rather than by an address — `PicRefs::resolve` for the readers, a same-picture kernel family for motion compensation — so no parity divergence and no interior mutability. `phase5_findings.md` §"CLOSED at Phase 5b" carries the argument; it is the shape the encoder's pool should reach for at 6.1/6.2. **The parse tree stays open with a measured lead**: Phase 5b built the conversion whole (owned slots, both stored aliases as indices, the NAL union as a struct) and reverted it at the gate — 11 of 60 conformance assets, every one a B-slice stream, frame counts correct; the patch is preserved and two hypotheses are eliminated. **F52 is CLOSED at Phase 6 session B** (2026-08-18): the six encoder-side shadowing-stub candidates were adjudicated by opening both lines each — four are trait-method *declarations* the sweep had scored as empty bodies (`Uninit`, `InitFrame`, `ExecuteTasks`, `OnTaskStop`), `WelsRcPostFrameSkipping` is a faithful `return false` (`ratectl.cpp:1015`) beside its `RCMode` dispatcher, `push_back` is two methods on two types; `find_shadowing_stubs.py` now skips declarations (22 → 18 candidate names, measured) and carries a `--self-test` that keeps F52's own stub shape printing. `phase6_findings.md`. **F60 is CLOSED in the session that found it** (Phase 6 session D): `FrameBsRealloc` moved `pOut->sNalLen` and never re-aimed the layers at it — three SIGBUS crashes and four byte divergences against the C++ before, all twelve configurations byte-identical after, on a path no sweep configuration reaches. **F61 is OPEN and owned by Phase 7**: the MT slice-bank growth never re-stamps the layer's slice list, and **the C++ has the same shape**, so it is a shared latent defect rather than a port divergence; session D's position spelling removed its memory-safety half and left the synchronisation half. **F57**, **F58** (Phase 6 session A) and **F59** (session B) are all **CLOSED in the session that found them** — the encoder Miri probe's three new findings, each fixed with red-before and green-after observed (`phase6_findings.md`). **F13 is CLOSED**: session A fixed its last production site as a 20-derivation family, and session B measured the backlog behind its Miri skip (exactly two tests, both green under the step's own flags) and **deleted the skip** — the `--lib` step now skips `wels_thread_pool` (F12, Phase 7) and nothing else. **F36** → decoder threading, or deletion (the port's MT slice loop is a partial translation; `WelsMarkAsRef`'s `pLastDec` is its last visible surface and both callers pass `None`). **F37** → Phase 8. **F4, F6, F7** (S6-parity) and **F8–F12, F14** → per file, owners as recorded. **The `CABA2_SVA_B` annotation stands** — 17 rows, expected-divergent, generated into the golden header so regeneration cannot lose it; a recorded position, not a defect. **Closed in Phase 5**: F15–F22, F24–F35, F39, F40, F42–F51, F53 — and F54's rule is folded into S21. *The findings files are the histories; this row is the open set.* |
| **The absent instrument** | Phase 0's T7 (fuzzing) was deferred by direction and there is still no corpus net. The tally of findings a fuzzer would plausibly have reached first now stands at **F8, F9, F10 (×3), F11, F12, F13, F14, and F3's eighth measurement** — re-raising T7 is Eugene's call |

**Where the live documents are:** the phase brief in [`prompts/`](prompts/) is what a
session executes; [`safety_refactor_log.md`](safety_refactor_log.md) is the per-session
record and the highest-density context available; [`perf_baseline.md`](perf_baseline.md)
holds every measurement and both ledgers; the findings files hold what was found and
deliberately not fixed. This plan holds the strategy and, in §7.6, the rules.


---

## 1. Where we are

### 1.1 Inventory

The port is a deliberate, faithful C transliteration: `#[repr(C)]` structs, Hungarian names, pointer graphs identical to the C++, `Option<unsafe extern "C" fn>` dispatch tables, a re-implementation of C's aligned-malloc header trick on top of `extern "C" malloc/free`, and `Default` via `mem::zeroed()`. That was the right call for getting to byte-exactness; it is also why the crate is wall-to-wall unsafe.

| Metric (src/ only) | Count |
|---|---|
| Lines of Rust | 85,182 in 74 files |
| `unsafe fn` | ~922 (469 in decoder/ = 69% of all decoder fns) |
| `unsafe {}` blocks | ~612 |
| `*mut` / `*const` type tokens | ~5,000 |
| Raw derefs `(*p…` | ~11,000 (≈4,300 decoder, ≈6,900 encoder) |
| Pointer arithmetic (`.add/.offset/.sub/.offset_from`) | ~4,100 |
| `mem::transmute` | 23 (21 in `decode_slice.rs` — all fn-ptr type erasure) |
| `unsafe impl Send`/`Sync` | 10 (5 type pairs, all in encoder threading) |
| `mem::zeroed()` | 26 (mostly `Default` impls of pointer-laden structs) |
| `#[unsafe(no_mangle)]` exports | ~42 |

**The single most important config fact:** `src/lib.rs:10` sets `#![allow(unsafe_op_in_unsafe_fn)]`. Inside the 922 `unsafe fn` bodies, every deref and every `.add()` is unguarded and invisible to `unsafe {}` counting. The *real* unit of work is "unsafe fn bodies converted", not "unsafe blocks removed". Progress must be measured with a ratchet script (§7.1), not by grepping for `unsafe {`.

### 1.2 Why the unsafe exists — a taxonomy

Every unsafe use in the crate falls into one of ten categories. Each has a standard safe replacement; none requires novel research. This table is the spine of the whole plan.

| # | Pattern | Where it lives | Safe replacement |
|---|---|---|---|
| T1 | **Malloc-style allocation + manual free cascades.** `CMemoryAlign` header-trick allocator over `extern malloc/free`; 141 call sites encoder-side; 300-line `WelsUninitEncoderExt` free cascade; 10 allocations per decoder picture. Three coexisting allocator families (`CMemoryAlign`, `Box::into_raw`, `alloc_zeroed`). | `common/memory_align.rs`, `encoder/encoder_ext.rs:998-1863`, `decoder/pic_queue.rs:177-405`, `decoder/decoder_core.rs:2955-3101` | Owned `Vec<T>`/`Box<T>` fields + `Drop`. Alignment is only needed for future SIMD; `#[repr(align(16))]` wrappers where it matters. |
| T2 | **Pixel-plane cursors with stride math and negative offsets into padding.** `pData[i]` points into the *middle* of a padded allocation (32 px luma / 16 px chroma borders); MB cursors advance by `.add(16)`; MV clamp is the only bounds check; border expansion writes above row 0. | `decode_slice.rs:1944, 1072-1091`, `decoder_core.rs:637-651`, `svc_base_layer_md.rs:327-358`, all DSP kernels | A `PaddedPlane` owner + `(x, y)`-addressed views where logical coordinates may be negative but the *biased* slice index is always in range (§2.2.1). |
| T3 | **Detachable byte/bit cursors.** `SBitStringAux{pStartBuf,pCurBuf,pEndBuf}` (reader *and* writer), `SDataBuffer` (4 pointers), CABAC engines' second cursor triple, CAVLC↔CABAC cursor handoff, dynamic-slice rollback snapshots of `pCurBuf`, and `ExpandBsBuffer` manually rebasing every outstanding pointer after realloc (`decoder_core.rs:1758-1842`). | `common/wels_common_defs.rs:30-46`, `decoder/{bit_stream,dec_golomb,cabac_decoder,nalu}.rs`, `encoder/{vlc_encoder,set_mb_syn_cabac,svc_set_mb_syn_cavlc,nal_encap}.rs` | Offset-based cursors that don't carry a buffer reference (§2.2.2). Deletes the realloc-rebase code outright; rollback snapshot becomes a saved `usize`. |
| T4 | **Multi-alias object graphs.** One `SPicture` reachable through up to 9 decoder locations (DPB pool, ref lists, `pDec`, `pECRefPic`, per-picture `pRefPic` graph with cycles…) and 4 encoder locations. Three pointer-identity comparisons in the decoder carry real semantics; the encoder has none. | `decoder/{pic_queue,manage_dec_ref,picture}.rs`, `encoder/{ref_list_mgr_svc,encoder_context}.rs` | Picture arena + copyable `PicId` handles; identity comparison = handle equality (§2.2.3). |
| T5 | **Derived-pointer caches / interior self-reference.** Encoder `SMB` fields point into ctx-level flat arrays (5 pointers × ~8k MBs, wired once at `encoder_ext.rs:611-617`); decoder `SDqLayer` per-MB arrays alias `pCtx->sMb` — the *same allocation reachable through two paths* (`decoder_core.rs:3843-3869`); `pSps/pPps` point into arrays inside the same struct; `SMbCache` sub-allocates one aligned block; `pMvdCost` points into the middle of a table. | `encoder/encoder_ext.rs`, `decoder/decoder_core.rs`, `encoder/md.rs:352-384`, `svc_motion_estimate.rs:176-195` | Delete the cached pointer, keep the index: `mb_idx * stride` recomputed at use, `sps_id` instead of `pSps`, `(table, bias)` instead of mid-pointers. Struct-of-arrays `MbGrid` with one owner (§2.2.4). |
| T6 | **Function-pointer dispatch tables.** Decoder: ~15 table families + 21 transmutes for type erasure. Encoder: `SWelsFuncPtrList`, 70 members, ~180 call sites — but ~55 members only "select" between N identical scalar functions (all SIMD variants delegate to `_c`); ~15 are genuine algorithm dispatch. Plus three hand-rolled *internal* C++ vtables (`IWelsVP`, `IWelsParametersetStrategy` 16 entries *(miscount — **20**, measured at Phase 3's exit; the error propagated into one session brief before being caught)*, `IWelsReferenceStrategy` 7 entries). The public `ISVCEncoder/ISVCDecoder` vtable emulation is **not** in this category — it is the drop-in ABI contract and stays (T10/§2.2.8). | `encoder/wels_func_ptr_def.rs`, `decoder/decoder_core.rs:1913-1995`, `encoder/paraset_strategy.rs:50-115`, `ref_list_mgr_svc.rs:1560-1574`, `processing/mod.rs` | Direct calls for CPU dispatch (nothing dispatches to different code today); `enum` + `match` for config dispatch; `Box<dyn Trait>` for the two real strategy objects; safe `fn(...)` pointers are still allowed where a table is genuinely convenient (§2.2.5). |
| T7 | **Word-punning (`LD32/ST32` idiom).** 4 pixels as one unaligned `u32`; NZC cache fills via `ST32(p, LD32(q))`; `(pCache as *mut u16).write_unaligned` over `i8` arrays (provenance-violating today). ~140 punned accesses in decoder `get_intra_predictor.rs` alone, 92 in `mv_pred.rs`. | `decoder/get_intra_predictor.rs`, `mv_pred.rs:247-455`, `parse_mb_syn_cavlc.rs:694-748`, `svc_mode_decision.rs:817,846` | `u32::from_ne_bytes`/`to_ne_bytes` on slice windows, or `copy_from_slice` for the pure 4-byte moves. Same codegen, zero unsafe. |
| T8 | **`*mut ctx` threading of the god-structs.** `SWelsDecoderContext` (112 fields, 28 pointers), `sWelsEncCtx` (76 fields, 37 pointers), passed as `*mut` through every signature; `rc.rs` has 751 raw derefs that are *only* `(*pCtx).field` accesses. | everywhere | `&mut` receivers on decomposed subsystem structs; field-precise parameters where borrows must split (§2.2.6, R1). |
| T9 | **Thread-sharing via laundered pointers.** `TaskPtr(*mut dyn IWelsTask)`, ctx pointers cast to `usize` to cross `thread::spawn`, 5 `unsafe impl Send/Sync` pairs, `Mutex` boxed and erased to `*mut c_void` to match C field types. Disjointness is *already* index-based (`m_iThreadIdx` slot claiming); sync is *already* `std::sync`. Decoder has **no** threading (stubs only). | `common/wels_thread_pool.rs`, `encoder/wels_task_management.rs`, `slice_multi_threading.rs:162-308` | Scoped threads / a safe persistent pool with `Box<dyn FnOnce + Send>` jobs over channels; per-thread resources become owned disjoint chunks (§2.2.7). |
| T10 | **C-ABI ceremony.** Vtable structs + 24 `extern "C"` thunks + `Box::into_raw` factories; `#[repr(C)]` on ~everything; `abi_guard.rs` size asserts; `mem::zeroed` Defaults; `struct_bytes_eq` memcmp equality. | `api/codec_api.rs`, both `abi_guard.rs` | Split by role: the *public* surface (vtables, thunks, factories, POD structs, `api/abi_guard.rs`) is the drop-in ABI contract — it **stays**, rewired internally onto safe `Decoder`/`Encoder` cores, as the crate's one `unsafe` module (§2.2.8). The *internal* ceremony — `repr(C)` on codec-internal structs, `encoder/abi_guard.rs` asserts, `mem::zeroed` Defaults, memcmp equality — gets deleted as structs de-C-ify. |

### 1.3 What is already safe (build on it, don't reinvent)

- `decoder/vlc_tables.rs`, `slice.rs`, `parameter_sets.rs`, both `abi_guard.rs`, `encoder/param_svc.rs` — effectively value-semantics already.
- `manage_dec_ref.rs` opens nearly every fn with `let ctx = &mut *pCtx;` and then uses safe field syntax — 80% converted in spirit; it's the template for the T8 recipe.
- `sad_common.rs:415-441` has three unused slice-based SAD wrappers — the kernel-conversion proof of concept.
- `WelsTaskBarrier` (`wels_task_management.rs:627-659`) is a fully safe `Mutex<i32>`+`Condvar` — the model for the thread pool rewrite. `CWelsList<T>` is already a `Vec` wrapper.
- `tests/common/` (sha1, y4m) is 100% safe; `split_annexb_units` and `TC0_TBL_LOOKUP` are safe islands already used from unsafe code.
- `benches/decode_1080p_bench.rs:162` defines a `trait Decoder` abstracting the Rust and dlopen'd C++ decoders behind one interface — the natural seam for the future safe public API.

### 1.4 The safety nets (why this refactor is tractable at all)

1. **Byte-exactness gates already exist**: decoder conformance (SHA-1 per plane over ~20 JVT streams), e2e vs gold `.y4m` + ffmpeg cross-check, encode/decode loopback SHA-1, option-sweep parity vs C++, and the diffharness (`rust/tools/diffharness/`) comparing whole bitstreams against a C++ build, including a threads dimension and slice-mode sweeps.
2. **The external ABI is a commitment, not a constraint inherited by accident.** Today the crate is rlib-only, so nothing outside the repo can call it — every consumer is in-repo Rust, which means the refactor can rewire the *inside* of the API layer freely, with the tests as the witness. But the decided end state (D2, resolved 2026-08-07) is a **drop-in `libopenh264` replacement**: the vtable emulation, the 7 upstream exports, and the `#[repr(C)]` structs pinned by `api/abi_guard.rs` are permanent public surface. Upstream's own header design is what makes this sound: the C++ interfaces have **no virtual destructors** and pin `EXTAPI` (`__cdecl` on Windows), precisely so that the C struct-of-function-pointers view and the C++ Itanium/MSVC vtable view coincide — the port's existing `lpVtbl` emulation is already that shape. All 7 exports upstream ships already exist in the port (`WelsCreateSVCEncoder`, `WelsDestroySVCEncoder`, `WelsCreateDecoder`, `WelsDestroyDecoder`, `WelsGetDecoderCapability` in `codec_api.rs`; `WelsGetCodecVersion`, `WelsGetCodecVersionEx` in `wels_encoder_ext.rs`). Known fidelity gap to carry, not fix, in this plan: `DecodeParser`/`DecodeFrameEx` are real upstream vtable slots that the port stubs — the slots stay (layout!), the stub behavior is documented as a limitation.
3. **The C++ tree is the permanent reference**: every phase keeps function-level correspondence with `codec/` so divergences can still be triangulated with h264dec ground truth and the gtest repro flow.

Known verification caveats (from prior sessions, they still apply): a Rust decoder error silently drops frames from the conformance hash — **always compare frame counts, not just hashes**; JVT/ffmpeg golds can't judge B-slice streams where upstream C++ itself diverges (the `#[ignore]`s in e2e are permanent fixtures, their set must not change — measured 2026-08-07 as **20** tests, all in `tests/e2e_conformance_test.rs`, not the 13 earlier revisions of this plan claimed; `cargo test --test e2e_conformance_test -- --ignored --list` is the authority); all C++-vs-Rust perf numbers were long believed scalar-vs-scalar — **corrected at Phase 5 session L**: the checked-in dylib dispatches NEON (`WelsCPUFeatureDetect = 0x000006`), so that ratio compares scalar Rust against NEON C++; §7.4's measurement-doctrine note is the authority. Every cumulative figure in this plan is a Rust-vs-Rust chain and is unaffected.

Two sharper caveats about the *encoder's* evidence were measured 2026-08-07 at `eb463dbd`; both were resolved later the same day, with one loose end that Phase 0 owns:

1. **The `GetDefaultParams`-path divergence: found and fixed** (`fa67432f`). Rust `FillDefaultExt` (`encoder/param_svc.rs`) dropped two assignments the C++ `FillDefault(SEncParamExt&)` makes (`codec/encoder/core/inc/param_svc.h:181-182`): `bFixRCOverShoot = true` (first bites at the 10th coded frame, the first VGOP boundary) and `iIdrBitrateRatio = 400` (first bites at the second IDR). The defaults path is now verified byte-identical on four sequences up to 720p, and the diffharness gained a `def` sweep preset (`909c368b`, drivers take `baseinit=2`) that runs the real default config — frame skip, adaptive quant, scene change, background detection, overshoot fix all ON — long enough to cross a VGOP boundary and a second IDR. **`sweep.sh def` is part of the gate battery from here on**, alongside `st` and `mt`. Moral for every later phase: when the C++ serves one function to two structs, diff both Rust copies.
2. **The release-build segfault: root-caused and closed** (Phase 0 task T3, 2026-08-07 — full write-up in [`phase0_findings.md`](phase0_findings.md) §F1). It was a **stack buffer overflow on the mainline encode path**: `DeblockingMbAvcbase` declared `uiBS` as `[[u8; 4]; 4]` (16 bytes) where the C++ declares `uint8_t uiBS[2][4][4]` (32 bytes), and five `from_raw_parts_mut(uiBS as *mut u8, 32)` sites in `DeblockingBSCalc_c` wrote the full 32. The reported crash site — a null `(*pCurMb).pNonZeroCount` in `WelsNonZeroCount_c` — was a *symptom of the smashed frame*, not the cause; instrumentation showed `pNonZeroCount` was never null across 378 configurations, and the faulting build actually died in `DeblockingFilterFrameAvcbase` on a null-plus-8 load. It "stopped reproducing" because the overflow is layout-sensitive and lands harmlessly in most builds: pristine `eb463dbd` release is clean, but the same build with one added `eprintln!` in `deblocking.rs` segfaults 5/5 deterministically. `e6fce464` ("encoding benchmark") widened `uiBS` to `[[[u8; 4]; 4]; 2]` without mentioning it, which is the actual fix; narrowing it back at HEAD reintroduces the crash 5/5, which is the proof. **Morals, both load-bearing for later phases:** a debug/release disagreement is UB evidence and never flakiness (§7.2 gate 0 — both profiles are now gated via `RUST_ENC_PROFILE`), and "it no longer reproduces" is not a resolution. This bug class dies structurally in Phase 2, when kernels stop taking `*mut u8` and start taking real array types that cannot be written past.

Also: commit `eb463dbd` deleted the port's handoff/status docs and the diffharness from git. The diffharness is back in-tree (`909c368b`); the handoff/status docs are still only in git history — Phase 0 recovers them.

---

## 2. Target architecture

### 2.1 Principles

1. **Byte-exact at every merge.** No phase may change any decoded pixel, any encoded bitstream, any error code, or the conformance frame counts. Refactoring and behavior change never share a commit.
2. **Indices, not pointers; owners, not graphs.** Every "many pointers to one thing" becomes "one owner + copyable indices". Every interior pointer becomes a recomputed offset. This is not just a safety move — it's what deletes `ExpandBsBuffer` rebasing, the free cascades, and the `Default = zeroed` hacks.
3. **Detached cursors.** State structs store positions (`usize`), never references into buffers. Long-lived structs holding `&'a` borrows are forbidden in the design — that's how a C port stays refactorable and how self-reference dies.
4. **Convert leaves first, gods last.** DSP kernels and bitstream cursors have thousands of call sites but trivial contracts; the context structs have trivial call counts but decide the ownership model. Do the mechanical mass early, the pivot late, with strangler shims (R7) so every PR compiles and passes gates.
5. **Zero new runtime dependencies.** The crate currently depends on `libc` alone (and only from benches); the end state depends on nothing. `std` threads, `std::sync`, `std::alloc` suffice. Dev-dependencies (fuzzing) are fine.
6. ~~**Keep the C++ diffability until the end.**~~ *(Retired by D-fid-1, 2026-08-13 — §7.4.)* Wels names, function granularity, and comment headers stay through the structural phases; renaming is a separate, optional, final phase (§10 D5).

### 2.2 Core vocabulary types

These live in a new `src/safe/` module (name bikesheddable), built and unit-tested in Phase 1, adopted incrementally. Sketches below are contracts, not final code.

**Built as of Phase 1** (2026-08-07): `src/safe/{plane,bits,pool,mb_grid,err}.rs`, every file `#![forbid(unsafe_code)]`, 63 in-module unit tests plus 18 differential tests against the implementations they replace (`tests/safe_plane_differential.rs`, `tests/safe_bits_differential.rs`), all Miri-clean. Where the implementation deviates from a sketch below, the sketch has been updated and the deviation flagged **[P1]** with its reason — the plan must never lag the code.

#### 2.2.1 `PaddedPlane` / plane views — replaces T2

The invariant the C code relies on: a plane allocation is `(pad + h + pad) × stride` bytes and `pData` points at `(0,0)` *inside* it, so `y ∈ [-pad, h+pad)` reads are in-allocation. Encode exactly that:

```rust
pub struct PaddedPlane {
    buf: Vec<u8>,          // owns >= (pad + h + pad) * stride
    stride: usize,
    origin: usize,         // byte offset of logical (0,0); origin = pad*stride + pad
    width: usize, height: usize, pad: usize,
}
impl PaddedPlane {
    pub fn new(width: usize, height: usize, pad: usize, stride: usize) -> Self;   // zeroed
    /// Validates the invariant; `pad` is recovered as `origin % stride`. Phase 2 shims feed this.
    pub fn from_parts(buf: Vec<u8>, stride: usize, origin: usize, width: usize, height: usize) -> Self;
    #[inline] pub fn at(&self, x: isize, y: isize) -> u8;
    #[inline] pub fn set(&mut self, x: isize, y: isize, v: u8);
    #[inline] pub fn row(&self, y: isize, x0: isize, len: usize) -> &[u8];
    #[inline] pub fn row_mut(&mut self, y: isize, x0: isize, len: usize) -> &mut [u8];
    /// Movable sub-views anchored at an MB origin — the safe version of the roving `pDstY`.
    pub fn cursor(&self, x: isize, y: isize) -> PlaneCursor<'_>;
    pub fn cursor_mut(&mut self, x: isize, y: isize) -> PlaneCursorMut<'_>;
    /// Escape hatch for `chunks_exact` row-walking and whole-plane fills.
    pub fn as_slice(&self) -> &[u8];  pub fn as_mut_slice(&mut self) -> &mut [u8];
    pub fn width/height/pad/stride/origin(&self) -> usize;
}
/// Read view; `Copy`, so rebasing is a value operation.
pub struct PlaneCursor<'a>    { buf: &'a [u8],     center: usize, stride: usize }
pub struct PlaneCursorMut<'a> { buf: &'a mut [u8], center: usize, stride: usize }
// both: new(buf, center, stride) [validated], at(dx,dy), row(dy,dx0,len),
//       advance(dx,dy) -> Self (by value), center(), stride()
// mut also: set(dx,dy,v), row_mut(dy,dx0,len), as_ref() -> PlaneCursor<'_>
// One private `fn idx(center, dx, dy, stride) -> usize` does all the biasing.
```

Key points:

- **Same-plane read-while-write is a non-problem** once access goes through one `&mut` view: intra prediction reading `(-1, dy)` and `(dx, -1)` while writing `(0..16, 0..16)`, and in-place deblocking across MB edges, are serial reads/writes through a single cursor — safe Rust is fine with that. The thing that is *actually* illegal today (two live `*mut` paths to one allocation, T5) dies with the aliasing, not with the arithmetic.
- Kernels take `PlaneCursorMut` instead of `(pPred: *mut u8, kiStride: i32)`. **[P2] The `(buf, center, stride)` triple escape hatch is not needed and should not be used** — see the pilot verdicts below.

**Pilot verdicts (Phase 2 T2, `decoder/decode_mb_aux.rs`).** The Phase 1 hand-off asked two questions before the API was adopted by ~120 kernels. Both are answered, and the answers are binding for the rest of Phase 2:

1. **Cursor, not triple — the choice was a false one.** `PlaneCursorMut` *is* the triple: three fields, passed by `&mut`, whose `row_mut` is one add and one slice index. The only per-call cost the triple avoids is the `assert!` in `new`, which runs once per kernel invocation, not once per row. The decode bench came out **1.3–1.8% faster** with the safe kernels live ([`perf_baseline.md`](perf_baseline.md) §Phase 2 T2, 3 runs per side). Use the cursor everywhere; reach for the triple only with a bench number in hand that says otherwise.
2. **`row_mut` hoists fine — but the C++ loop order is what has to change.** The 4x4 IDCT wrote its output *column*-major (four strided stores per column), which no row-window idiom can help. Transposing it to row-major made each row one bounds check and one `&mut [u8; 4]`. That transposition is bit-exact **only because every sample of the block is read and written exactly once**, which is a per-kernel proof obligation, not a blanket licence. Check it before transposing; when it fails, keep the C++ order.
3. **`PlaneCursorMut::reborrow(dx, dy)` was added** (`952c8b1d`'s API could not express it). `advance` consumes the cursor, which is right for a walk (`pDstY.add(16)`) and wrong for a composite kernel that hands each sub-block to an inner kernel and carries on — `IdctFourResAddPred_c` calling `IdctResAddPred_c` four times. Expect the same shape in `mc.rs` and `deblocking_common.rs`.
4. **Shim contracts are short exactly when a kernel reaches forward only.** These four do, so the reachable span is `(bh-1)*stride + bw` — derivable from the signature, needing no knowledge of the plane's padding. The families that read `-1`/`-stride` (`get_intra_predictor`), `-3*stride` (deblocking) or write the padding (`expand_pic`) will need `from_raw_parts_mut(p.sub(k*stride + k), …)` and a contract that names the padding constant. Budget for that; it is where the phase's real documentation value is.
- The MC clamp (`decode_slice.rs:1072-1091`) stays byte-for-byte: the clamp guarantees the biased index is in range; slice indexing then *proves* it, converting a silent-corruption class into a loud panic if the port ever miscomputes.
- Cross-plane pairs (MC: read ref picture, write current) are two views of two different `PaddedPlane`s — no conflict. Same-picture src/dst in error concealment goes through a `copy_within`-style method on one plane.
- Border expansion is a **free function over the raw geometry for now** — Phase 2 T6 built it as `expand_picture(&mut [u8], stride, w, h, pad)` in `common/expand_pic.rs`, because in Phase 2 the allocation is still C-owned and arrives at the shim as a mid-plane pointer. The original intent stands as the Phase 5 packaging step: once `Picture` owns `PaddedPlane`s, the free function becomes (or is wrapped by) a `PaddedPlane` method, and the shim's span reconstruction (`expand_shim_span`, `decoder_core.rs`) dies with the shim.

#### 2.2.2 Detached bit cursors — replaces T3

```rust
/// Reader state; NO buffer reference inside. The buffer is passed to each call.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct BsCursor { pos: usize, cur_bits: u32, left_bits: i32, len: usize, bits: i32 }
//                                                               ^^^^^^^^^^^^^^^^^^^^^ [P1]

impl BsCursor {                                   // mirrors, in order:
    pub fn init(buf: &[u8], size_bits: i32) -> Result<Self, ErrInfo>;          // DecInitBits
    pub fn init_read_bits(&mut self, buf: &[u8], end_offset: isize) -> …;      // InitReadBits
    pub fn get_bits(&mut self, buf: &[u8], n: i32) -> Result<u32, ErrInfo>;    // BsGetBits
    pub fn get_one_bit / get_ue / get_se / get_te0(…);                         // BsGet*
    pub fn peek_bits(&self, n: i32) -> u32;                                    // UBITS
    pub fn check_more_rbsp_data(&self) -> bool;                                // CheckMoreRBSPData
    pub fn pos/len/bits/cur_bits/left_bits(&self);                             // state, for parity tests
}
pub fn trailing_bits(byte: u8) -> i32;                                         // BsGetTrailingBits

/// Writer: owns position only; the output slice is a parameter, as for the reader.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BsWriter { pos: usize, cur_bits: u32, left_bits: i32 }
// new() [= InitBits], write_bits, write_one_bit, write_ue, write_se, flush,
// rbsp_trailing_bits, bits_pos [= BsGetBitsPos], pos, left_bits;
// free fns size_ue/size_se [= BsSizeUE/BsSizeSE, computed rather than table-driven].
```

- **[P1] `BsCursor` carries `len` and `bits`, not three fields.** `len` is `pEndBuf - pStartBuf`, the *logical* end of the RBSP, which is not the same as the length of the slice passed in — the allocation legitimately continues past it, and the slop below depends on the difference. Without it, error-code parity at the end of a NAL is not expressible. `bits` is `iBits`, needed only by `check_more_rbsp_data`. Both are state in the C++ struct too; nothing is stored that `SBitStringAux` did not already hold.
- **[P1] The slop reads three bytes past the RBSP, not one** (and the initial prime reads four bytes from the start regardless of NAL length). `BsCursor` reproduces the *predicate* exactly and expresses the loads through `get`, so it is identical to the C++ given ≥3 bytes of slack and strictly safer without. Measured and written up as [`phase1_findings.md`](phase1_findings.md) §F4; Phase 3.1 owns the guard-byte decision.
- **[P1] `BsWriter` implements only the canonical (`vlc_encoder.rs`) semantics.** No guard, no masking, no wrapping variant is smuggled in from the other three copies — Phase 3.2 must decide those explicitly (F2). Building it turned up [`phase1_findings.md`](phase1_findings.md) §F5: the canonical writer panics in debug builds on a 32-bit write into an empty accumulator; `BsWriter` does not, and a test pins both profiles.
- **[P3] `BsCursor` gains explicit CAVLC-mode support — the Phase 1 omission of `iIndex` was wrong** (found by T3.1b's inventory: `BsStartCavlc`/`BsEndCavlc`, `parse_mb_syn_cavlc.rs:2230-2248`, and the residual path between them are live consumers). `iIndex` is a *second representation of the same position* — an absolute bit index the CAVLC residual decode reads through while the accumulator is deliberately stale. Decision: mirror it 1:1 as a mode, not a parallel field to keep coherent by hand: `cavlc_bit_pos: isize` + `start_cavlc()` (`(pos << 3) − (16 − left_bits)`, the 16-bit half-window convention preserved exactly) + `end_cavlc(buf)` (reseat `pos = idx >> 3`, 4-byte BE prime shifted by `idx & 7`, `pos += 4`, `left_bits = −16 + (idx & 7)` — negative on purpose, parity-exact), plus a `#[cfg(debug_assertions)]` mode flag asserted by both families of accessors so the stale-accumulator misuse the C++ could never detect panics in debug. Differential-tested against the raw pair on random states and round-trips before T3.1b converts any consumer. The mode dissolves when Phase 5 restructures the residual path; until then the field accessors are the consumers' mechanical conversion target.
- **[P3] P6's resolution is `READER_SLOP = 3` over the real allocation — not zero guard bytes.** T3.1a established that the slop bytes **feed decoded values on some malformed streams**, not just the error predicate, so zeroing them would have changed output; the reader instead declares the slack it always read and is handed the genuine allocation bytes (`decoder_core.rs:3637` guarantees them). Byte-identical by construction, proven by T3.0's 2316 goldens + all 53 conformance hashes.
- **[P3] T3.1b's storage shape deviates from the brief's default: `BsReader`, not a bare `*mut BsCursor`.** The plan makes `BsCursor` *detached* (§2.1.3 — offsets only, no buffer reference), and the ~20 decoder consumers do take `(buf: &[u8], cursor: &mut BsCursor)` as specified. But until T3.3 the bytes live behind `sRawData`'s pointers, so something must remember where a given reader's bytes start *and how many are readable*. `BsReader { base, avail, cursor }` is that something: one `SHIM(phase3)` type, one `buf()` that reconstructs the slice, one thing for T3.3 to delete. The rejected alternative — base and extent as loose fields beside each cursor, kept coherent by hand — is the same shape this section rejected for `iIndex` above, and for the same reason. `SDqLayer::pBitStringAux` is therefore `*mut BsReader` (the pointer is Phase 5's to remove; the base and extent inside it are T3.3's).
- **[P3] `READER_SLOP` is not the readable extent — corrected at T3.1b, `phase3_findings.md` §F16.** The `= 3` derivation covers `dump_bits_aux` only. `BsEndCavlc` primes **four** bytes at `iIndex >> 3`, and `iIndex` is advanced by the residual decoder past the RBSP end without bound on truncated input — measured reaching `len + 5` and beyond. The readable extent is a property of the *allocation*, not a constant offset from the RBSP length, and `decoder_core.rs:3637`'s `len + 4` guarantee is not sufficient either. `BsReader::avail` carries it, computed by the one `readable_from(pHead, pEnd, …)` helper. **T3.2's CABAC end ladder and 5-byte init prime must take their extent from the same helper, not re-derive one.**
- **[P3] T3.2's engine shape (2026-08-10): `SWelsCabacDecEngine` is `{uiRange, uiOffset, iBitsLeft, pos: usize}` — the pointer triple is deleted, and the buffer is a per-call `win: &[u8]` that is the RBSP window, not the allocation.** The audit (in `cabac_decoder.rs`'s module docs, per-site as F16 demands) showed the engine needs **two different extents**: the end ladder is bounded by the *logical* end (`len − 1` — its selector is measured against `pBuffEnd`, so it never reads the slack), while only the once-per-slice init prime reaches past it (`len + 2`). So the hot path takes `BsReader::rbsp_window()` — the first `cursor.len()` bytes of `buf()` — making `win.len()` *be* `pBuffEnd − pBuffStart` with no extent arithmetic inside the engine, and init alone uses the wider `buf()`/`avail` window through `get`. The ladder's `iLeftBytes <= 0` predicate is expressed as the comparison `pos >= win.len()`, never the subtraction: `pos` legitimately exceeds `len` after init on truncated input, and a `usize` subtraction would wrap and select the 4-byte arm — a new OOB where the raw code errored. One `SHIM(phase3)` accessor, `cabac_rbsp_window(pCtx)`, is the single deref chain producing the window for `parse_mb_syn_cabac.rs`'s 18 consumers (call-expression edits only, per the brief); it dies with `pBitStringAux`. Field order in the struct keeps `uiRange`/`uiOffset` adjacent — the release build pairs them into one `ldp`, checked in the seam's disassembly diff.

- **[P3] T3.3's owned buffer is `RawDataBuffer { buf: Vec<u8>, cur: usize }` — one offset, not two, and no stored extent anywhere (2026-08-11).** The sketch below said "two `usize` offsets", and the session brief's version stored `start`, `cur` and `end`. Both are more than the port needs, and one of them is the F16 defect in miniature: `end` (`pEnd − pHead`) is a *stored copy of the allocation size*, which is precisely the class that goes stale — it is `buf.len()`. `pStartPos` is **write-only in this port** (upstream's parse-only rewind that reads it was never transliterated), so `start` has nothing to store either. The governing rule, written on the type: **every readable extent is derived from the owning buffer at window-creation time** — `window_from(start)` (was `readable_from`) and `rbsp_window(reader)` — and a reader stores an *offset*, which a reallocation cannot invalidate. That is what makes F16's class unrepresentable rather than guarded, and why `ExpandBsBuffer` could be deleted outright: its rebases and its stale-`avail` repair had nothing left to maintain. The backing `Vec` is kept at full allocated size and zero-filled (`WelsMallocz`'s semantics), so `buf.len()` is *initialized bytes* and never spare capacity — the slop the reader has always read (F4) is the same real bytes it always read.
- `SDataBuffer{pHead,pEnd,pStartPos,pCurPos}` → `Vec<u8>` + two `usize` offsets *(superseded by the [P3] note above: one offset)*. `ExpandBsBuffer`'s pointer-rebasing block (`decoder_core.rs:1816-1842`) is deleted, not converted — offsets survive realloc by definition.
- NAL units store `Range<usize>` into the AU buffer instead of `pNalPos: *mut u8`. *(T3.3: a `start` offset plus the cursor's own `len`, which is the same pair without a second representation of the length; `pNalPos` itself was dead and was deleted rather than converted.)*
- The CABAC decoder engine stores `{ pos: usize, range: u64, offset: u64, bits_left: i32 }`; the CAVLC↔CABAC handoff (`cabac_decoder.rs:712-716`) becomes an assignment of one `usize` to another.
- The encoder's dynamic-slice rollback (`StashMBStatus`/`StashPopMBStatus`, `svc_set_mb_syn_cavlc.rs:1057-1076`) snapshots a `BsWriter` by value — it's `Copy`.
- Two deliberate C quirks must be handled consciously, not accidentally:
  1. `dec_golomb.rs:128-150` allows the read cursor to sit 1 byte past `pEndBuf` and then loads 2 bytes. Reproduce by appending ≥4 zero guard bytes to the raw-data `Vec` (deterministic, in-bounds) and keeping the same `pos > len+1 → ERR_INFO_READ_OVERFLOW` predicate. Verify on the malformed-stream tests that error codes don't shift.
  2. `BsWriteBits` has **no** end-of-buffer check (`vlc_encoder.rs:367-382`) — sizing is the caller's contract. The safe writer keeps the same unchecked *semantics* w.r.t. output bytes but gets real bounds from slice indexing; buffer sizing logic is unchanged, so a panic here means a real pre-existing overflow bug.
- The writer exists **twice** (verbatim copy at `svc_encode_slice.rs:498-585`) — dedupe before converting.

#### 2.2.3 Picture arena + handles — replaces T4

```rust
/// Index into a pool; identity == handle equality. Generation is debug-only (D1) and
/// is NOT part of `PartialEq` — equality must mean the same thing in both profiles.
#[derive(Copy, Clone, Debug)]
pub struct Id { index: u32, #[cfg(debug_assertions)] generation: u32 }

/// Generic [P1]: the encoder needs the same shape (6.1/6.2), so `PicId` becomes a
/// newtype/alias over `Id` and `PicPool` over `Pool<Picture>` in Phase 5.1.
pub struct Pool<T> { slots: Vec<T>, #[cfg(debug_assertions)] generations: Vec<u32> }
impl<T> Pool<T> {
    pub fn new(slots: Vec<T>) -> Self;   pub fn len(&self) -> usize;
    pub fn id(&self, index: usize) -> Id;         // mints a handle, stamped
    pub fn ids(&self) -> impl Iterator<Item = Id>;
    pub fn iter(&self) -> impl Iterator<Item = (Id, &T)>;   // the `find_free` predicate rides here
    pub fn get(&self, id: Id) -> &T;    pub fn get_mut(&mut self, id: Id) -> &mut T;
    pub fn pair_mut(&mut self, a: Id, b: Id) -> (&mut T, &mut T);          // panics if a == b
    /// The critical split: one picture &mut while any others are read.
    pub fn mut_and_rest(&mut self, cur: Id) -> (&mut T, PoolRest<'_, T>);  // [P1]
    pub fn replace(&mut self, id: Id, value: T) -> T;   // recycling; bumps the generation
}
pub struct PoolRest<'a, T> { … }   // get(id) -> &T, panics on the mutably-held slot
```

- **[P1] `mut_and_rest(cur)` replaces `cur_and_ref`/`cur_and_refs(cur, refs)`.** The reference *list* carried no weight: its only job was rejecting `cur ∈ refs`, which `PoolRest::get` does anyway at the point of access, and taking it would force either an allocation or an arbitrary fixed capacity into a per-macroblock path. Splitting the slot span is allocation-free, subsumes `cur_and_ref`, and handles the B-slice case (one `&mut` + two `&`, P1) without a special method.
- **`find_free` is not a method**; it is `pool.iter().find(…)`, because the predicate (`bUsedAsRef`/`iRefCount`) belongs to `Picture`, which Phase 5.1 defines.

- All nine decoder alias sites (`pDec`, `pTempDec`, ref lists, `pECRefPic`, `pPreviousDecodedPictureInDpb`, `SPicture::pRefPic`, `SDeblockingFilter::pRefPics`, thread ctx, pool) become `Option<PicId>` / `[Option<PicId>; N]`. The per-picture `pRefPic` graph (cycles!) is just data as handles — no ownership cycles because handles don't own.
- The three decoder pointer-identity comparisons that carry semantics — `deblocking.rs:258` (boundary strength: "same reference picture?"), `manage_dec_ref.rs:739` (self-copy guard), `error_concealment.rs:599` — become `PicId == PicId`, which is *exactly* the same predicate. The encoder needs no care at all: the survey found zero pointer-identity comparisons; all matching is by `iFrameNum`/`iFramePoc` values.
- Recycling can hand a stale `PicId` the same slot a fresh picture occupies — identical to the C++ hazard, so behavior is preserved; a `debug_assertions`-only generation counter catches logic rot in tests without changing release semantics. **Implemented in Phase 1** (D1 resolved); `replace` bumps it, every accessor checks it, and `PartialEq` deliberately ignores it.
- `Picture` itself becomes: 3 × `PaddedPlane` + owned `Vec`s for the per-MB metadata (`pMbType`, `pMv`, `pRefIndex`, `pNzc`, `pMbCorrectlyDecodedFlag`) + value fields. Ten manual allocations and `FreePicture` collapse into struct construction and `Drop`.
- Encoder side identical shape; the pool ownership moves out of `CWelsPreProcess` (see §3 P10 for the cycle it currently forms).

#### 2.2.4 `MbGrid` — single owner for per-MB metadata — replaces T5

Decoder today: `pCtx->sMb.pXXX[0]` (25 arrays) and `pCurDqLayer->pXXX` are the same allocations reachable through two paths — instant UB under `&mut` rules and the #1 blocker for borrow-checking `decode_slice.rs`. Encoder today: each `SMB` caches 5 pointers into ctx flat arrays.

**Phase 1 built the addressing, not the fields** — `MbDims` (grid geometry: `mb_xy`, `xy_of`, `count`, and `left`/`top`/`top_left`/`top_right` mirroring the guards at `mv_pred.rs:485-510`, availability logic deliberately excluded) and `MbArray<T>` (one owned per-MB array over an `MbDims`). The field union below is Phase 5.2's to decide, since only it knows which of the `sMb`/`SDqLayer`/`SMB` fields survive; `MbGrid` is then a struct of `MbArray`s.

```rust
pub struct MbGrid {                    // one per dq-layer; owns everything per-MB
    pub mb_type: Vec<u32>,
    pub mv: Vec<[[i16; 2]; 16]>,       // indexed [mb_xy]
    pub ref_index: Vec<[i8; 16]>,
    pub nzc: Vec<[i8; 24]>,
    pub slice_idc: Vec<i32>,
    pub scaled_tcoeff: Vec<[i16; 384]>,
    pub luma_qp: Vec<u8>, pub chroma_qp: Vec<[u8; 2]>,
    …                                   // the full union of sMb/SDqLayer/SMB-pointer fields
}
```

- Neighbor access (`left = xy-1`, `top = xy-mb_width`) is plain indexing with the existing availability checks as the bounds logic — reads of `[xy-1]` while writing `[xy]` through one `&mut MbGrid` are safe Rust, no aliasing question at all.
- The decoder's repeated `if !pDec.is_null() { pDec->pMbType } else { pCurDqLayer->pMbType }` fork stays as an explicit two-source choice (`grid` vs `pool.get(dec_id).mb_meta`) — same logic, borrow-checkable because they are genuinely different owners.
- Encoder `SMB` drops its 5 pointer fields; accessors compute `mb_idx * K` — the survey calls this "the highest-leverage single change in the port".
- `SMbCache` (encoder, `#[repr(C, align(16))]` with hand-inserted padding) becomes a plain struct of fixed-size arrays (`[u8; 256]`, `[i16; 384]`, …) with `#[repr(align(16))]` kept for future SIMD; the 13 sub-allocation pointers into one arena die with the arena. Hand-inserted padding fields and the ABI asserts for it are deleted.

#### 2.2.5 Dispatch policy — replaces T6

Three tiers, in place of every `Option<unsafe extern "C" fn>`:

1. **CPU-feature tables → direct calls.** Nothing in the port dispatches to anything but the `_c` kernel (all `_sse2/_neon/_mmi/_lsx` entries are delegating stubs or dead externs — including 62 never-referenced `extern` declarations in `mc.rs:793-863`). Delete the tables, call the function. When real SIMD arrives, reintroduce as safe fn-pointer tables selected once at init (`fn(&mut PlaneCursorMut, …)` pointers are safe types) or `#[target_feature]` behind a tiny dispatch shim — a decision for that day, invisible to this plan.
2. **Config dispatch → `enum` + `match`.** CAVLC-vs-CABAC slice writer (`pfWelsSpatialWriteMbSyn`), MD strategy family (`pfInterMd` & co., ~15 members), search method tables (`pfSearchMethod[]`), stash/pop pair, `pfRc` — all selected once from encoder config: an `enum EntropyCoder { Cavlc, Cabac }` etc., matched at the call site. The 21 `transmute`s in `decode_slice.rs` exist only to erase concrete fn types into `*mut c_void`-parameter typedefs — they disappear with the tables.
3. **Real strategy objects → traits.** `IWelsParametersetStrategy` (16-entry vtable) and `IWelsReferenceStrategy` (7-entry) become `Box<dyn ParamsetStrategy>` / `Box<dyn RefStrategy>` — textbook conversions, zero aliasing subtlety per the survey. `IWelsVP` (the processing library's 7-slot vtable, one implementation, two call sites) doesn't even need a trait: inline `SWelsVpContext` into `CWelsPreProcess` and `match` on the method enum. Watch the documented footgun: today a `None` slot silently returns success (`processing/mod.rs:5-20` records the bug this caused) — the rewrite makes those paths statically total.

#### 2.2.6 Context decomposition — replaces T8

`SWelsDecoderContext` and `sWelsEncCtx` don't survive as monoliths; `&mut everything` through one struct would fight the borrow checker at every two-subsystem call. Decompose along the lines the code already draws:

```rust
pub struct Decoder {
    params: DecodingParams,
    raw: RawDataBuffer,            // Vec + offsets (was sRawData/sSavedData)
    au: AccessUnitList,            // NAL bookkeeping, ranges not pointers
    paramsets: ParamSetStore,      // owned SPS/PPS arrays; "active" = sps_id/pps_id (kills pSps/pPps self-ref)
    dpb: PicPool,
    layer: DqLayerState,           // incl. MbGrid
    cabac: CabacState,             // ctx tables + engine (offsets only)
    dec_state: FrameState,         // pDec/pTempDec as Option<PicId>, flags, counters
    ec: EcState,
    stats: DecoderStatistics,
    last_pic: LastDecPicInfo,
    reorder: ReorderState,
    logger: Logger,
}
```

- Functions become methods on the subsystem that owns their data; cross-subsystem functions take the 2-3 parts they need as separate `&`/`&mut` parameters (`fn decode_mb(layer: &mut DqLayerState, dpb: &mut PicPool, cabac: &mut CabacState, …)`). This is the same move `manage_dec_ref.rs` already made informally.
- Wels function names are kept (`WelsDecodeSlice` stays `WelsDecodeSlice` until the optional rename phase) so the C++ diff column survives.
- `Default` becomes derivable per subsystem; all 26 `mem::zeroed()` sites die here.
- Same treatment for `sWelsEncCtx` → `Encoder { params, funcs-gone, layers, dpb, rc, vaa, preprocess, slicing, out, stats, … }` per the encoder survey's field classification (12 owned singletons, 7 owned flat arrays, 6 owned arrays-of-owned, 7 non-owning cursors→indices, ref-list aliases→`PicId`, per-thread buffers→`Vec<Vec<u8>>`).
- The logger back-pointer (`sLogCtx.pCodecInstance` pointing at the encoder itself) is replaced by a `Logger` value that owns its sink — in the safe API a `Box<dyn Fn(Level, &str)>`; the C trace-callback POD remains only as API surface translated at the boundary.

#### 2.2.7 Threading model (encoder only) — replaces T9

Facts from the survey that make this easy: tasks are fork/join per frame with a barrier; disjointness is already expressed as "task owns index `m_iThreadIdx` of several parallel arrays" (claimed via `QueryEmptyThread` under a mutex); sync primitives are already `std::sync`; a single-threaded inline fallback already exists; the C++ event machinery was already found dead and omitted. The decoder has no threading at all — its stubs get deleted, and future decoder MT (if ever) is designed fresh on the safe architecture.

Target shape:

```rust
pub struct SliceJob<'f> {
    slice_idx: usize,
    bs_buf: &'f mut [u8],          // this thread's chunk of the bs-buffer pool
    slice: &'f mut SliceState,      // from Vec<SliceState>, split per job
    // read-only shared: &EncConfig, &PicPool view, &LayerConst
}
std::thread::scope(|s| {
    for job in jobs { s.spawn(|| encode_slice(job, shared)); }
});                                  // join == the barrier
```

- Fixed slice modes: partition `Vec<SliceState>` and the per-thread bs buffers with `split_at_mut`/`chunks_mut`, one job per slice, exactly the current claiming logic minus the mutex.
- `SM_SIZELIMITED` (dynamic slicing): a work queue of slice indices via `mpsc` + per-worker owned scratch; results stitched in slice order (the assembly is already order-based, which is why MT output is deterministic today). Determinism gate: MT output byte-equals today's MT output (already exercised by diffharness's threads argument).
- The persistent pool (avoids re-spawning threads per frame) is rebuilt safely if profiling shows spawn cost matters: worker threads + `mpsc::Sender<Job>` where `Job: FnOnce + Send`, completion counted by the existing safe `WelsTaskBarrier`. `TaskPtr`, the `usize` laundering, the `*mut c_void` mutexes (`WelsMutexInit`), the `static mut GLOBAL_THREAD_POOL` (→ `OnceLock`), and 5 of the 5 `unsafe impl` pairs — **amended by D-mt-1 (2026-08-20): 5 → 1 named `send-seam(Phase 9)` impl, the last retiring with Phase 9's context split** — are deleted.
- Deblocking runs after join, as today.

#### 2.2.8 The API: safe core + preserved C-ABI boundary — replaces T10

Two layers. The **safe core API** is what the whole crate converges on internally:

```rust
pub struct Decoder { inner: DecoderState }          // Send; one instance == one C object
impl Decoder {
    pub fn new(params: &SDecodingParam) -> Result<Self, DecoderError>;
    pub fn decode_frame(&mut self, nal: &[u8]) -> Result<DecodeOutput<'_>, DecoderError>;
    pub fn set_option(&mut self, opt: DecoderOption) -> …;   // typed enum internally
    pub fn flush(&mut self) -> Option<DecodeOutput<'_>>;
}
pub struct DecodeOutput<'a> { pub planes: [PlaneRef<'a>; 3], pub info: SBufferInfo }
```

The borrowed-return problem the C API papers over (`ppDst` planes point into the DPB; `SFrameBSInfo.pBsBuf` aliases encoder buffers) is solved here with lifetime-bound views (`DecodeOutput<'a>` borrowing `&'a mut self`), matching the C contract "valid until the next call" — the borrow checker now *enforces* what the C header only documents. Exporting this layer publicly for Rust consumers is free and worth doing.

The **C-ABI boundary** (`src/api/codec_api.rs`) is permanent public surface and stays exactly as the drop-in contract requires:

- `ISVCEncoderVtbl`/`ISVCDecoderVtbl` structs, the `lpVtbl`-first object layout, all vtable slots **including** the `DecodeParser`/`DecodeFrameEx` stubs (slot order is ABI; stub behavior is a documented limitation), the 7 `no_mangle` factories/version functions, and `EXTAPI` calling-convention parity. What *changes* is only what's behind the thunks: `CWelsDecoderImpl { base, pVtbl, pCtx: *mut SWelsDecoderContext, … }` becomes `{ base, pVtbl, inner: Decoder }` — each thunk null-checks, downcasts `pThis`, translates raw arguments to safe types, calls the safe core, and translates back (filling `SFrameBSInfo`/`ppDst` with pointers derived from the borrowed views, whose validity window is identical to today's).
- `SEncParamExt`, `SDecodingParam`, `SFrameBSInfo`, `SBufferInfo`, `SParserBsInfo`, `OpenH264Version` & co. stay `#[repr(C)]`; `api/abi_guard.rs` grows to pin every struct that crosses the boundary (add the few it doesn't yet cover).
- `(ENCODER_OPTION, *mut c_void)` Get/SetOption pairs: the boundary does the `void*` decoding it already does, then calls typed internals. The trace-callback option stores the raw C `(fn, void*)` pair inside the boundary and wraps it as the internal `Logger` sink — the only place a C callback is ever invoked.
- Crate becomes `crate-type = ["rlib", "cdylib", "staticlib"]`. Soname/version-script naming to match a system `libopenh264.so.N` is a packaging concern outside the crate (document the linker flags; don't encode a distro's soname in Cargo).
- Threading contract of the C API: instances are not internally synchronized but must be usable from different threads (one thread at a time per instance), and separate instances must work concurrently. Safe core enforces this by construction: `Decoder: Send` / `Encoder: Send` asserted with compile-time tests, no thread-locals, and the encoder's process-wide thread-pool singleton (matching C++'s refcounted global) lives in an `OnceLock`.
- **This module is the one place `unsafe` survives**, under `#![deny(unsafe_code)]` at the crate root with a scoped `#[allow(unsafe_code)]` on `api` (deny, not forbid, precisely so this exception can exist). Target size: a few hundred lines of pure translation, reviewable in one sitting, with a `# Safety` comment per thunk stating the C contract it relies on.
- Tests/benches/diffharness **keep driving the C API forever** — that's now a feature, not transitional debt: the conformance suite continuously exercises the exact surface external users get, and the measuring instrument never moves at all (the Phase 8 test-migration risk from rev 1 of this plan disappears).

### 2.3 Definition of done

- Crate root carries `#![deny(unsafe_code)]`; the **only** `#[allow(unsafe_code)]` in the tree is on `src/api/` (the FFI boundary, §2.2.8). Every other module — decoder, encoder, common, processing, the safe core API — is unsafe-free and raw-pointer-free. `libc` removed from `[dependencies]`.
- `*mut`/`*const` appear in exactly two places: the `#[repr(C)]` structs and vtable/function-pointer types that *are* the public C ABI, and the boundary thunks that translate them. No raw pointer ever flows past the boundary into codec code.
- The crate builds as `["rlib", "cdylib", "staticlib"]`; the cdylib exports exactly the 7 upstream symbols (plus nothing that collides), and the external-ABI harness (§7.2 gate 7) passes: the Rust dylib, loaded via `dlopen` by a C++ driver compiled against upstream's `codec_api.h`, runs the conformance flows with hashes identical to the in-process runs.
- Benches keep their `dlopen` unsafe (dev-only, inherently FFI); the fuzz crate and diffharness drivers are safe (the diffharness's Rust side may use the C API like any external consumer would).
- All conformance/e2e/loopback/option tests pass with unchanged hashes, unchanged frame counts, unchanged `#[ignore]` set; diffharness sweep unchanged in both build profiles; benches within the agreed budget (§7.4).

---

## 3. The hard problems, called out (with their resolutions)

Most of the 85k lines convert mechanically once §2.2 exists. These are the places that don't, each with the decided approach — so no future session has to re-litigate them:

- **P1 — `decode_slice.rs` (5,261 lines) touches everything at once**: plane cursors, dispatch, MbGrid, DPB borrow splits, B-slice dual `sMCRefMember` cursor structs, 19 transmutes. Resolution: it is converted *last* within Phase 5, after every vocabulary type is proven elsewhere; the B-slice MC path uses `PicPool::cur_and_refs` to hold current-`&mut` + two ref-`&` simultaneously; `sMCRefMember` becomes `{ pic: PicId, plane_origin: (isize, isize) }` resolved at the kernel call.
- **P2 — the `sMb`↔`SDqLayer` double path** (`decoder_core.rs:3843-3869`) is UB *today* and blocks everything. Resolution: `MbGrid` (§2.2.4) becomes the single owner in Phase 5.2, `SMbCache`'s 25 arrays deleted; do it in one PR per array-family with shims.
- **P3 — decoder pointer-identity semantics** (3 sites). Resolution: `PicId` equality; add a targeted regression test per site (boundary-strength on a stream with duplicate-POC refs; EC self-copy stream) before converting.
- **P4 — self-referential ctx fields** (`pSps/pPps` into own arrays; dequant caches). Resolution: active-paramset *ids* + lookup at use; dequant tables addressed `[pps_idx][qp]` directly — the cached mid-pointers were pure C convenience.
- **P5 — `ExpandBsBuffer` rebase + NAL payload pointers surviving realloc.** Resolution: ranges/offsets (§2.2.2); the rebase function is deleted; add a unit test that grows the buffer mid-AU and checks parse continuity (there's a latent-bug class here that offsets fix *silently* — make it visible). **RESOLVED at T3.3** (2026-08-11): `RawDataBuffer` owns the bytes, readers store offsets, and `ExpandBsBuffer` is deleted outright — nothing needs rebasing. The predicted latent class did surface *before* it could be fixed silently: F16's second instance was exactly a stale extent across this growth path, recorded as a finding first per S12, then made unrepresentable (no stored extents anywhere, §2.2 [P3] note).
- **P6 — the overread slop** in `dec_golomb.rs:128-150` (three bytes, not one — F4). **RESOLVED at T3.1a** (`96fb04a4`): not guard bytes — the slop *feeds decoded values*, so zeroing changes output — but a declared `READER_SLOP = 3` over the real allocation bytes the reader always read (§2.2.2 [P3]); error-code parity proven by T3.0's golden corpus.
- **P7 — word-punning**. Resolution: `from_ne_bytes/to_ne_bytes` everywhere (T7); these are byte-moves, endian-neutral in effect; codegen verified by the perf gate. The two `write_unaligned`-over-`i8[]` sites in `svc_mode_decision.rs:817,846` are rewritten as two byte stores with a comment, since they're provenance-violating today.
- **P8 — `wels_preprocess.rs` ownership cycle**: it owns the spatial `SPicture` pool *and* holds `m_pEncCtx: *mut sWelsEncCtx` while the ctx holds `pVpp` — the one genuine cycle in the crate. Resolution: invert control — the pic pool moves up into `Encoder`; preprocess methods take `(&mut PrepState, &mut PicPool, &EncConfig)` parameters instead of reaching back through a stored ctx pointer. No `Rc/Weak` anywhere.
- **P9 — VAA out-pointer bundle** (`SVAACalcResult` holding 8 raw out-pointers into encoder arrays). Resolution: `Process` takes an `&mut VaaOutputs { sad8x8: &mut [[i32;4]], … }` view struct built fresh per call from encoder-owned `Vec`s; the stored-pointer struct dies.
- **P10 — encoder slice-list realloc invalidating outstanding `SSlice` pointers** (`ReallocateSliceList`, `svc_encode_slice.rs:2520+`). Resolution: `Vec<SliceState>` + indices everywhere; "current slice" is `usize`. The survey confirmed `SSlice` has no parent back-pointers and every entry point already receives ctx+slice as parameters — signatures become `(ctx parts, slice_idx)`.
- **P11 — `pMvdCost` mid-table pointer with negative indexing.** Resolution: `(table: &[u16], bias: usize)` pair; accessor `mvd_cost(d: i32) = table[(bias as i32 + d) as usize]`.
- **P12 — `struct_bytes_eq` memcmp equality on SPS/PPS** (padding-sensitive). Resolution: derived `PartialEq` on value structs; padding is deterministically zero today (structs are zero-initialized), so field-wise equality is behavior-preserving; keep one test comparing overwrite-detection behavior on the SPS-switch conformance streams.
- **P13 — panic policy.** Decoder: bitstream-derived values must reach error codes, never panics — the C++ error paths already exist and are ported; slice-index panics are reserved for port bugs (they'd be silent corruption in C). Fuzzing (§7.3) enforces "no panics on arbitrary input" continuously *(2026-08-11: T7 remains deferred by direction, so nothing enforces this continuously yet — T3.0's 2316-row malformed-stream golden corpus is the standing approximation, and §0's "absent instrument" row keeps the tally of what a fuzzer would have caught first)*. Encoder: API-boundary validation as today; internal panics = bugs.
- **P14 — losing C++ diffability mid-refactor** *(retired by D-fid-1, 2026-08-13: diffability is no longer a constraint; the C++ is reference, not template — §7.4)*.** Resolution: function names/granularity preserved until the final optional rename; every converted fn keeps its `/// C++: codec/decoder/core/src/foo.cpp WelsFoo` header; module docs keep the source-file mapping; byte-gates at every merge mean h264dec triangulation stays available throughout.

---

## 4. Mechanical recipes

The repeatable transformations, so sessions can be scripted/parallelized. Each recipe = one PR shape.

- **R1 — `*mut Ctx` → `&mut` receiver sweep.** For files that only do `(*pCtx).field` (e.g. `rc.rs`: 751 derefs, 1 pointer-arith op; `au_set.rs` same profile): change ~33 signatures to `&mut`, drop `unsafe`, let rustc list every call site. Purely textual; ideal early-win recipe once its callers hold references.
- **R2 — kernel designature.** `(pPred: *mut u8, kiStride: i32)` → `(c: &mut PlaneCursorMut)` / fixed blocks `(dct: &mut [i16; 16], ff: &[i16; 8])`. Applies to ~120 kernels across `*_mb_aux`, `get_intra_predictor` ×2, `mc.rs`, `sad_common`, `intra_pred_common`, `deblocking_common`, `expand_pic`, `vaacalc`, `sample.rs`.
- **R3 — pointer-field → index-field.** `pCurDqLayer: *mut SDqLayer` → `cur_dq: usize`; `pRefList0: [*mut SPicture; 16]` → `[Option<PicId>; 16]`; `pSps: *mut SSps` → `active_sps: u8`.
- **R4 — free-cascade → `Drop`.** Delete `WelsUninitEncoderExt`-style cascades as their targets become owned; the `CMemoryAlign` leak counter is replaced by Rust ownership (keep its byte-usage stat only if the MEMORY statistics API needs it).
- **R5 — vtable → trait/enum** (§2.2.5 tiers).
- **R6 — `mem::zeroed` → `#[derive(Default)]`** as pointer fields disappear per struct.
- **R7 — strangler shims.** When converting a callee whose callers are still raw: keep the old `unsafe fn` signature as a 3-line adapter (`from_raw_parts` + call safe core), marked `// SHIM(phaseN)`, deleted when the last caller converts. This keeps every PR small, compiling, and gate-green. CI greps for `SHIM(` to ensure the count trends to zero by each phase's exit.

---

## 5. Phase plan

Estimates are in focused sessions (the unit this port has been built in). Total: **≈50–65 sessions** (revised 2026-08-08 from ≈45–60 after Phase 2's scale correction; see the calibration row in §9 — count before sizing, and treat per-phase figures as −0/+40% ranges). Every phase ends with the full gate battery (§7.5) green.

### Phase 0 — Guardrails, dead-code purge, tooling *(3 sessions)*

1. ~~**Close out the release-segfault question**~~ **DONE (T3, 2026-08-07)** — reproduced at `eb463dbd`, root-caused to a 16-byte `uiBS` written 32 bytes deep in `DeblockingMbAvcbase`/`DeblockingBSCalc_c`, and confirmed already fixed at HEAD by `e6fce464`. Details in §1.4 and [`phase0_findings.md`](phase0_findings.md) §F1. No fix commit was needed in Phase 0.
2. Recover the deleted handoff/status docs into `rust/docs/` (`git show eb463dbd^:rust/docs/...`) — they hold measured facts (knobs the C++ itself rejects, non-byte-verifiable comparisons, the zsh word-splitting trap in sweeps) the gate battery depends on. Mark them "recovered, claims unverified". Resolve the uncommitted `sweep.sh` modification sitting in the working tree (inspect, then commit or drop with a reason).
3. Record baselines: conformance hashes + frame counts, diffharness sweep outputs **in both build profiles** across `st`, `mt`, and the new `def` preset, bench numbers (3-run medians), unsafe-ratchet counts (§7.1) checked in as `rust/tools/unsafe_baseline.json`.
4. Delete dead unsafe now (no behavior change): the 62 unreferenced SIMD externs (`mc.rs:793-863`); SIMD delegating stubs everywhere they exist (`encode_mb_aux.rs:772-930`, `sample.rs`, `intra_pred_common.rs` `_mmi/_lsx/_sse2/_neon` aliases, decoder equivalents) with tables re-pointed at `_c`; decoder threading scaffolding (`SWelsDecThreadInfo`, `SWelsDecoderThreadCTX`, `EventCreate`, dead `pReadyEvent`/thread-gated `pNzc` alloc paths, `GetThreadCount` → literal 1); duplicated bitstream writer (`svc_encode_slice.rs:498-585`); `use std::ffi::c_void` in lib.rs.
5. Stand up the fuzz crate (`rust/fuzz/`, cargo-fuzz, decode target seeded from `res/`), run it for a baseline corpus; it runs continuously from here on.
6. Add the ratchet script + a `just`/shell gate runner so every session can run the whole battery in one command.

*Exit gate: release and debug builds both pass the battery with identical bytes; ratchet baseline committed. Risk: the segfault fix itself — contained by the diffharness before/after comparison in the debug profile, where behavior is defined.*

### Phase 1 — Safe vocabulary types *(3 sessions)*

Build `src/safe/`: `PaddedPlane`/`PlaneCursorMut`, `BsCursor`/`BsWriter`, `PicId`/`PicPool` skeleton, `MbGrid` skeleton, `DecodeError` plumbing helpers. Unit tests including property tests against the existing unsafe implementations (same random inputs → same outputs), run under **Miri**. Nothing is wired into the codec yet.

*Exit gate: unit tests + Miri clean; codec untouched. Risk: API-shape mistakes — mitigated by piloting on one consumer each in Phase 2/3 before mass adoption.*

### Phase 2 — Leaf DSP kernels, both codecs *(6–8 sessions; resized 2026-08-08)*

*Scale correction from execution: the "~120 kernels" figure undercounted by ~60% — the real total, counted as `unsafe fn` definitions in the phase's files, is ≈190, and per-family estimates in briefs have been wrong in both directions (T2 "~13" was 4; T3 "44" was 42; T4's "26" held). Count the file before sizing the session, and treat every kernel figure in this section as an estimate.*

R2 across: `decoder/decode_mb_aux.rs` → `encoder/encode_mb_aux.rs` + `sample.rs` + `encoder/decode_mb_aux.rs` → `common/sad_common.rs` + `intra_pred_common.rs` + `mc.rs` (fold the `[[fn;4];4]` quarter-pel table into a `match` or safe-fn table) → `common/deblocking_common.rs` (mind the stride-swap V/H trick and `-3*stride` reads — cursor handles both) → `common/expand_pic.rs` + `decoder_core.rs` expand functions → `processing/vaacalc.rs` + `adaptive_quantization.rs` kernels → both `get_intra_predictor.rs` (the biggest: 44 decoder kernels + encoder set; scriptable — uniform signature, uniform punning pattern; keep the decoder/encoder same-name-different-signature functions distinct).

Callers keep calling through R7 shims where tables still exist; tables themselves get retyped to safe fn pointers as each family completes. Delete `#[unsafe(no_mangle)]`/`extern "C"` from all kernels (nothing links them; benches compare via dlopen, not symbol interposition).

**[P2] What this phase does and does not move on the ratchet.** Measured on the pilot family and true of every family in it: `no_mangle` falls, `SHIM(` rises, `unsafe_block` **rises**, and `unsafe_fn` and `raw_ptr` do not move at all. That is not a disappointment, it is the strangler pattern's arithmetic — the shim keeps the raw signature, so the pointers and the `unsafe fn` stay until Phase 5 converts the *callers* and deletes the shim; and the explicit `unsafe {}` around each `from_raw_parts` is *counted* unsafe replacing the *uncounted* unguarded derefs that `#![allow(unsafe_op_in_unsafe_fn)]` hides inside an `unsafe fn` body (§1.1). The metric got more honest, not worse. Consequence for the workflow: **commit B of each family regenerates the baseline**, after running `check` and confirming the increases are confined to the file being converted. Phase 2's real deliverable is the safe kernels and their written contracts; the count collapse is Phases 4–5 cashing this in.

*Exit gate: full battery; bench delta budget applies for the first time. Risk: perf regressions from bounds checks in 4×4 loops — see §7.4 idioms; convert `get_intra_predictor` early in the phase to get the worst case measured.*

### Phase 3 — Bitstream layer *(4–5 sessions)*

1. Decoder read side: `bit_stream.rs`, `dec_golomb.rs` (guard-byte design P6), `cabac_decoder.rs` (offset engine + handoff), `SDataBuffer` → `RawDataBuffer`, `nalu.rs` payload ranges, delete `ExpandBsBuffer` rebasing.
2. Encoder write side: `vlc_encoder.rs` → safe `BsWriter`; `set_mb_syn_cabac.rs` cursor triple; `svc_set_mb_syn_cavlc.rs` rollback snapshots + `pEndBuf - pCurBuf` space checks → `len - pos`; `nal_encap.rs` owned buffers.
3. `SBitStringAux` itself remains as a compat shell only where still-unconverted structs embed it (`SDqLayer::pBitStringAux` waits for Phase 5); the shell is deleted in Phase 5/6. *(Outcome, 2026-08-11: the shell was never needed — T3.4's inventory came back empty and the type was deleted outright. `SDqLayer::pBitStringAux` had already been retyped `*mut BsReader` at T3.1b; only its pointer-ness remains, and that is Phase 5's.)*

*Exit gate: battery + a dedicated malformed-stream error-code parity test (systematic truncations, EPB edges, degenerate NALs — exact error codes and cursor/frame-count behaviour). The originally-specified fuzz burn-in is removed from this gate (2026-08-10, by direction — T7 stays deferred; §0's "absent instrument" row tracks the tally), which is why the parity test's corpus must carry the malformed-input burden alone. Risk: the P6 slop and CABAC end-ladder byte counting — both have precise existing tests (truncated streams in conformance set).*

### Phase 4 — Dispatch de-virtualization *(3 sessions; split 4a/4b and resequenced by D-seq-1, 2026-08-10)*

**D-seq-1 (2026-08-10): Phase 4a runs immediately after Phase 2's exit, before Phase 3.** The §8 dependency graph always permitted it (P2 → P4 with P3 on a parallel track); what makes it the right schedule now is the ledger: cumulative decode is ≈ +17% on the fastest stream and encoder ≈ +11%, all of it fixed per-call scaffolding that direct dispatch is expected to let LLVM inline away. Running 4a first (a) executes the recovery checkpoint while the deficits are still only kernel-family deficits, (b) gives Phase 3's new decoder-side shim families (bitstream cursors, on the same hot streams) a recovered baseline and full tripwire headroom to land into, (c) triggers the parked families' first re-attempt point (T5-sad and any T7 parkees — earlier re-landing means less time with raw kernels live, which is the safety goal speaking, and F10 showed parked raw code is where latent UB sits), and (d) deletes real unsafe surface in its own right: the `Option<unsafe extern "C" fn>` tables, the 21+2 transmutes, and the untyped installer casts T6 deliberately left behind (`decoder_core.rs:1906`, `:920-921`).

- **Phase 4a (first, ~2 sessions):** kernel-dispatch de-virtualization — `mc.rs`'s consumers first (the D-perf-3 protocol, preserved verbatim: direct calls to `#[inline]` shims, `Option`-unwrap semantics verified post-init, one-pair measurement both benches), then the decoder kernel tables (intra-pred arrays, `sBlockFunc`, `sDeblockingFunc`, `sMcFunc`, expand, the cache-fill transmutes in `decode_slice.rs`), then the encoder's ~55 CPU-dispatch members. **Checkpoint duties:** re-measure every ledger row and record recovery; re-attempt every parked family (bodies re-measured with dispatch direct — D-perf-2's slices-and-offsets idea is on the table here); if recovery does not materialize, downgrade the ledger's remaining recovery claims to Phase 5/6 as D-perf-3's fallback prescribed.
- **Phase 4b (after Phase 3, ~1 session):** the config-dispatch enums that touch what Phase 3 rewrites (`pfWelsSpatialWriteMbSyn` CAVLC/CABAC, RC-mode fn table → enum) plus the strategy vtables (`ParamsetStrategy`, `RefStrategy`) and `IWelsVP` if Phase 3/6 haven't absorbed them naturally.

Tier 1 deletions (tables → direct calls; the Phase 0/2 work makes this mostly deletion), tier 2 enums (`EntropyCoder`, MD/ME strategy enums, RC mode — note `SWelsRcFunc` maps to an enum over the five RC modes already proven byte-identical), tier 3 traits (`ParamsetStrategy`, `RefStrategy`), `IWelsVP` inlined per §2.2.5, all 21+2 transmutes gone, `wels_func_ptr_def.rs` reduced to the ~15 algorithm choices and then dissolved into config-typed fields on `Encoder`.

*Exit gate: battery, **plus the D-perf-1 recovery checkpoint**: re-measure every deficit-ledger family's streams with dispatch direct (shims now inlinable into callers — span arithmetic folds against caller constants, `from_raw_parts` inlines to ~nothing) and record the recovery in the ledger. Risk: low — every replaced pointer provably pointed at exactly one function per config; assert-map the table-init functions before deleting them.*

### Phase 5 — Decoder structural rewrite *(9–12 sessions)* — the first pivot

Order inside the phase (each step lands with shims + full gates):

1. **5.1 Picture & DPB**: `Picture` re-struct (PaddedPlanes + Vecs), `PicPool`, `pic_queue.rs` recycling predicate, `manage_dec_ref.rs` (near-mechanical per survey), `error_concealment.rs` identity sites, P3 regression tests first.
2. **5.2 MbGrid**: kill `sMb`/`SDqLayer` double path (P2); `SDqLayer` → `DqLayerState` with owned `MbGrid`; re-point `parse_mb_syn_*` cache fills (their 30-entry scratch caches become `&mut` locals passed down — ~40 signatures, mechanical). *(Added at Phase 3's exit: size every commit here by the **S20 closure, computed first** — `SDqLayer` is embedded and asserted widely, and T3.4 proved a signature change forces every reachable struct into one commit. Phase 3's one surviving straggler name, `SDqLayer::pBitStringAux` — type already `*mut BsReader` — retires in this step, and `cabac_decoder.rs`'s `SHIM(phase5)` rbsp accessor dies with it.)*
3. **5.3 Neighbor & MV machinery**: `mv_pred.rs` (punning → byte ops, `SetRectBlock` → typed generic on the grid), colocated reads via `cur_and_ref`.
4. **5.4 Deblocking driver** (`decoder/deblocking.rs`): `SDeblockingFilter` holds `PicId`s + plane cursors built per-MB; identity compare per P3.
5. **5.5 `decoder_core.rs`**: allocation → constructors (`AllocPicture` dies into `Picture::new`), paramset store (P4), context decomposition per §2.2.6, `Drop` teardown, `Default` derives. *(Added at Phase 3's exit: **S21 applies with force** — unlike the encoder's `pOut`, which is reached through a pointer, the decoder context embeds its buffers **by value**, so every owned field lands inside `mem::zeroed` reach. The `MaybeUninit` shell + `new_boxed()` already exists at `decoder_context.rs:769` from T3.3; this step extends it per owned field and is the step that eventually replaces the shell with a real constructor.)*
6. **5.6 `decode_slice.rs`** last (P1), including EC MC paths; delete the last decoder shims *(2026-08-11: there is no `SBitStringAux` shell to delete — the type died outright at T3.4; what remains for this step is the decoder's `SHIM(phase2)` kernel adapters, whose callers convert in 5.2–5.4)*; decoder modules get `#![deny(unsafe_code)]` one by one.

*Exit gate: battery with special attention to frame-count parity and the `#[ignore]` set; ~~long fuzz run~~ *(2026-08-11: T7 remains deferred by direction — in its place, T3.0's 2316-row malformed-stream golden table gates this phase in both profiles, per Phase 3's exit)*; decoder src/ is unsafe-free; **every §7.4 deficit-ledger entry whose shims died in this phase must clear** (the ledger empties as the scaffolding it measures is deleted) — *and this is the phase that collects Phase 4a's downgraded decode rows: direct dispatch recovered nothing there because `BaseMC` passes runtime dimensions, and 5.x is what makes those dimensions static (≈ +17.8 / +10.1 / +9.6% cumulative decode is the debt; the ~7-point CB headroom under the tripwire is this phase's to spend)*. Risk: highest of the plan — this is where borrow-splits are designed for real. Mitigation: strict step order above, each step's shims keep the tree green, and any step can pause for a session boundary without leaving broken state.*

### Phase 6 — Encoder structural rewrite *(10–14 sessions)* — the second pivot

Same playbook, informed by Phase 5's patterns:

1. **6.1** `ref_list_mgr_svc.rs` → `PicId` lists (survey: zero identity comparisons, pure index conversion) + `RefStrategy` trait from Phase 4.
2. **6.2** Picture pool ownership out of `wels_preprocess.rs` (P8 inversion); `SVAACalcResult` → out-views (P9); `processing/` fully safe here.
3. **6.3** `SMbCache` → owned fixed arrays; plane cursors in MD/ME (`md.rs`, `svc_base_layer_md.rs`, `svc_mode_decision.rs`, `svc_motion_estimate.rs` incl. P11, `svc_encode_mb.rs`) — the hot path; watch the bench budget per-PR. *(Added 2026-08-10/11: this step owes the **third attempt at the parked families** — `common/sad_common.rs` (14) and `encoder/sample.rs` SATD (7), twice parked on dated verdicts (1.41x–4.94x vs a ≤1.05x bar) — re-tried here because caller conversion is what changes their calling convention; SATD still owes a measurement of its own before any verdict.)*
4. **6.4** Slice/layer structures: `Vec<SliceState>` + indices (P10), `svc_enc_slice_segment.rs` maps → `Vec<u16>`, `encoder/deblocking.rs` finishing its half-done slice conversion. *(Added at Phase 3's exit — this step inherits Phase 3's two deliberate exclusions: **`SWelsSliceBs.pBsBuffer`**, left raw because `SetOneSliceBsBufferUnderMultithread` aliases it to `pThreadBsBuffer[kiThreadIdx]` — the port's last two `SHIM(phase3)` markers guard exactly this and retire when the thread-buffer ownership is redesigned here/Phase 7 — and **`SWelsNalRaw.pRawData`**, left raw because one struct type serves both the frame-level NAL list (owner now a `Vec`, T3.6) and the slice-level list pointing into the MT-aliased buffer: one type cannot hold offsets into two owners until both owners are real.)*
5. **6.5** R1 sweeps: `rc.rs`, `au_set.rs`, `paraset_strategy` remnants, `wels_encoder_ext.rs` internals. *(Disposition, steward 2026-08-19 — this step maps onto no phase6.md §6 session, so it is split by family rather than owned whole: `rc.rs`'s `SMB`/`SSlice` signature sites land with session E's parameter families and its 56 `*mut sWelsEncCtx` with 6.6/G's flip; `au_set.rs` + `paraset_strategy.rs` fold into session G's opening face; `wels_encoder_ext.rs` internals are the ABI boundary and move to **Phase 8**'s rewire beside the `src/api/` inventory — the decoder's api-owned fields set the precedent.)*
6. **6.6** The pivot: `encoder_context.rs` + `encoder_ext.rs` — `RequestMemorySvc` → constructors, free cascade → `Drop`, field taxonomy applied (§2.2.6), `encoder/abi_guard.rs` asserts deleted struct-by-struct in the same commits that de-`repr(C)` them. *(Added at 4b session B: the free-cascade inventory here runs **F19's check on every `Box::into_raw` member** — an owner that exists in source but on no live path is a leak wearing a destructor. The paraset strategy leaked ~1.2 KB per teardown and per param-adjust reset that way, invisible to every gate, until T4b.2a's ownership audit asked "which line frees this?" per allocation.)*

*Exit gate: battery + diffharness full sweep (slice modes × RC modes × threads) byte-identical. Risk: 6.3 perf; 6.6 is a big-bang file but by then it's the last raw consumer standing.*

### Phase 7 — Threading rework *(3–4 sessions)*

§2.2.7: scoped fork/join for fixed slicing; channel work-queue for SM_SIZELIMITED; safe persistent pool only if spawn cost shows up in benches; delete `TaskPtr`, laundering, `void*` mutexes, `static mut` singleton, all 5 `unsafe impl` pairs; `wels_task_management.rs`/`slice_multi_threading.rs`/`wels_thread_pool.rs` collapse to a fraction of their size.

*Exit gate: MT determinism (MT bitstream == today's MT bitstream, across thread counts), loom-style stress not required — the model is fork/join with owned splits. Risk: SM_SIZELIMITED work distribution must not change slice boundaries; it's index-driven, keep the claiming order identical.*

*(Added at Phase 3's exit: **this phase is F3's experiment, and F3 closes here one way or the other.** Fourteen measurements, four alternations, four acquittals of the trees under test; the signature — nondeterministic wrong-length MT output at `sm=3, t∈{2,4}`, now seen on a third input beyond the two Cisco clips — has survived every phase unchanged, which points at the pool/claiming machinery this phase deletes (F12 and P10 are the candidate mechanisms on record). If the fork/join rewrite makes it vanish, that is the mechanism confirmed by ablation; if it persists on the rewritten pool, it is a pre-existing defect upstream of the pool and finally has a small surface to corner it in. Either exit is a resolution; carrying the signature past Phase 7 unexplained is not.)*

**Inherited from Phase 6 (written session I, 2026-08-20; every item re-verified
against the tree before it was written down).**

* **F61** — the multi-threaded slice-bank growth that never re-stamps the layer's
  slice list. Unchanged and untouched by Phase 6.
* **F3's ablation**, now at **86 measurements / 26 alternations / 59 acquittals**.
  Two things Phase 6 added that change how this should be opened: the output clause
  covers *any* wrong length (a short stream since measurement 85, a longer one since
  measurement 6 — "zero-byte" is not the tell), and measurement 86 found the
  **fastest-reproducing configuration on record**: `mt CiscoVT2people_320x192_12fps
  sm=3 n=600 t=4 cabac=1` fails **~1 in 11** under load against the finding's ~1/800.
  **Open the ablation there**, not on the `160x96 n=600` configuration that accounts
  for most of measurements 80-85; it reproduces roughly a hundred times faster.
* **F12** and the thread pool — every worker takes `&mut` to the one shared pool.
  Still the only `--skip` in the Miri `--lib` gate.
* **The four live allocator call sites left in the encoder**: `DynamicSliceBs`
  (`encoder_ext.rs:1117`/`:1754`) and `sSliceBs.pBs` (`svc_encode_slice.rs:2978`/`:3007`).
  Phase 6 took `src/encoder`+`src/processing` from 45 to 12; these four plus the 8
  dormant screen-content ones are all that remain.
* **`pMemAlign`** (`encoder_context.rs:1121`, still `*mut CMemoryAlign`) and with it
  the retirement of **`common/memory_align.rs`** — those four sites are the only
  things keeping the custom allocator alive.
* **The MT context members and both MT files** (`wels_task_management.rs`,
  `slice_multi_threading.rs`), which carry 19 of the encoder's `*mut sWelsEncCtx`
  parameters and are the two modules exempted from the `deny(unsafe_code)` sweep.
* **Session E's one MT `*mut SMB` site**: `slice_multi_threading.rs:364`
  (`pMbList: *mut SMB`), the single macroblock-list pointer crossing a thread boundary.


### Phase 8 — C-ABI boundary hardening *(3–4 sessions)*

§2.2.8. The externally visible API does not change at all; this phase moves the safety line up to it and makes the drop-in story real:

1. Carve the safe core `Decoder`/`Encoder` types out of what phases 5–7 produced (mostly naming/visibility — the subsystems already exist); assert `Send` via compile tests; export the safe API for Rust consumers.
2. Rewire the boundary: `CWelsDecoderImpl`/`CWelsH264SVCEncoderImpl` hold the safe types; each of the 24 thunks becomes translate-in → safe call → translate-out with a written `# Safety` contract; the vtable structs, slot order (including `DecodeParser`/`DecodeFrameEx` stubs), factories, and version functions are untouched. *(Added 2026-08-11, Phase 5 session D: this rewire owns **F23** — all 19 `&mut self` convenience methods on the 8-byte vtable base structs are UB today, the borrow narrowed to `ISVCDecoder` while the thunk writes the impl object at offset 0x20; every integration test and Rust consumer hits it. The receivers become raw-pointer-based or move to the impl object — S28's provenance law at the ABI layer.)* *(Added 2026-08-13, Phase 5 session O: this phase also inherits **eight of the decoder context's raw fields, which are `CWelsDecoderImpl` members** — 5.5's closure excluded them by ownership — plus **F41** (the context's `pParam` aliasing the API object's live parameter block; same ownership question as F38's back-pointers, fixed with them). And **Phase 8 opens with the `src/api/` inventory**: the census, dup, double-cast and stub-body sweeps have never scanned the module, because its `deny(unsafe_code)` exemption silently propagated into every instrument's scope — S22's clause, F38 the proof. Run them all over `api/` before rewiring anything.)* Trace-callback plumbing lands here (raw C pair stored at the boundary, wrapped as the internal `Logger` sink).
3. `crate-type = ["rlib", "cdylib", "staticlib"]`; extend `api/abi_guard.rs` to pin every boundary-crossing struct (add `SParserBsInfo`, `OpenH264Version`, capability struct if missing).
4. Build the **external-ABI harness**: a small C++ driver compiled against upstream `codec_api.h` that `dlopen`s the Rust cdylib and runs the decoder-conformance and loopback flows; its hashes must equal the in-process results. Stretch, high-value: point upstream's gtest API suites at the Rust dylib — the repo already contains the gtest tree and the `gtest_repro` flow.
5. Scoped-lint endgame for the module: `api/` gets `#[allow(unsafe_code)]`; crate root gets `#![deny(unsafe_code)]` (this flips in Phase 9 once stragglers are gone).

Tests, benches, and diffharness are deliberately **not** ported off the C API — they keep exercising the drop-in surface permanently (§2.2.8). The C++-side flow caveat stands: `DecodeFrame2`-vs-`DecodeFrameNoDelay` ordering differences are a known divergence class, and the boundary must preserve exact call-sequence semantics.

*Exit gate: battery + the new external-ABI harness green; exported-symbol list of the cdylib == upstream's 7. Risk: pointer-filling of output structs from borrowed views must reproduce the exact validity windows C callers assume — mitigated by the harness driving the same sequences C consumers use.*

**Inherited from Phase 6 (written session I, 2026-08-20; verified against the tree).**

* **`wels_encoder_ext.rs` internals** — 8 `*mut sWelsEncCtx` parameters, untouched by
  Phase 6 by direction. This is the encoder's api boundary, where the reference to
  the context is born.
* **The `c_void` C-ABI line** — 58 `c_void` occurrences across `src/encoder`, the
  encoder's counterpart to the decoder's boundary work.
* **`pSvcParam` is NOT inherited.** Session H's verdict, re-verified: the field is
  `Option<Box<SWelsSvcCodingParam>>` (`encoder_context.rs:940`) and the context owns
  it. Named here only so the next reader does not re-open the question.


### Phase 9 — Hygiene endgame *(2–3 sessions)*

`#![deny(unsafe_code)]` at crate root with the single scoped `#[allow(unsafe_code)]` on `src/api/` (§2.2.8); remove every other `#![allow(...)]` blanket except naming allowances (kept until D5 is decided); `libc` dropped; clippy (`pedantic` triage) + final Miri/fuzz marathons; docs: module-level C++ provenance headers verified present; ratchet file replaced by the lint (its only remaining job is watching `api/`'s size); optional D5 rename executed or explicitly declined; revisit D4 — the workspace split that turns `deny`+`allow` into a true `#![forbid(unsafe_code)]` core crate.

**Inherited from Phase 6 (written session I, 2026-08-20; verified against the tree).**

* **The SAD/SATD third park** — 1.30-5.68x / 1.36-4.05x, still parked.
* **`SMbCache`'s 72 kernels-take-slices sites** (H's count; the type is named at 113
  places in `src/encoder` in total).
* **F5's ~10 side-array resolutions.**
* **H's Vec-vs-`Option<Box>` accessor cost** on {LTR, rc, ref lists} —
  `WelsRcMbInitGom` and `ctx_rc_at` are the two named sites.
* **D-perf-6's parked recovery.**
* **The `pMvdCost` record fields — and there are *four*, not two.** Session I's brief
  named two (`SWelsMD::pMvdCost` at `md.rs:197`, `SWelsME::pMvdCost` at
  `svc_motion_estimate.rs:154`); the tree also has **`SWelsME::pMvdCostX` and
  `pMvdCostY`** (`svc_motion_estimate.rs:209`/`:210`), the same per-macroblock cache
  of `origin + uiLumaQp * stride` split per axis. All four are re-stamped by
  `WelsInitInterMDStruc` on every macroblock.
* **NEW — the five self-referential dispatch slot types** (session I, T6.I2).
  `PDeblockingBSCalc`, `PDeblockingFilterSlice`, `PMotionSearchFunc`,
  `PSearchMethodFunc` and `PLineFullSearchFunc` each name `*mut SWelsFuncPtrList` in
  their own signature, so the table is handed back into its own callees and **22
  functions cannot take a reference to it** — the survivor list in `8ffdd064`'s
  message. Retiring them is a de-virtualization of Phase 4a's kind (what it did to
  `pfMdCost`), not an ownership change, which is why Phase 6 could not do it.


### Phase 10 — Screen content re-enable *(feature port; 3–5 sessions; D-scr-1's owner)*

The port rejects `SCREEN_CONTENT_REAL_TIME` at one init-time guard
(`encoder_ext.rs:817`) because the FME machinery behind it was never ported;
the mode's other branches are already translated (30 C++ usage-type sites against
40 in the port — the code exists and is simply never true). Scope:

1. Re-port the FME **preparation half** deleted at T6.D2 — C++
   `svc_motion_estimate.cpp:648` (`RequestFeatureSearchPreparation` /
   `ReleaseFeatureSearchPreparation` / `SFeatureSearchPreparation` /
   `UpdateFMESwitch`), the allocation at `encoder_ext.cpp:1032`/`:1129`, and the
   per-frame `PerformFMEPreprocess` call at `:2747`.
2. Un-fence the in-tree **search half** (tagged `SCREEN_CONTENT(dormant: Phase 10)`
   at Phase 6 session E) and convert it with the rest — it was left raw because
   nothing could verify a conversion until this phase makes it reachable.
3. Port the **VAA screen extension** (`SVAAFrameInfoExt` + `RequestMemoryVaaScreen`,
   C++ `encoder_ext.cpp:1708`; port note `encoder_ext.rs:1174`) and the **screen
   complexity analysis** (`CComplexityAnalysisScreen`; port note
   `processing/complexity_analysis.rs:21`).
4. Wire `SPicture.pScreenBlockFeatureStorage` allocation — the ref-list branch
   (`ref_list_mgr_svc.rs:1142`) and the strategy rows are already in place.
5. Remove the guard.
6. **The referee first, per this project's own rule**: a screen-content sweep
   preset and byte parity against the C++ over those configurations, plus a Miri
   probe for the FME path — nothing lands without them.

Runs after Phase 9 by default; nothing in it blocks or is blocked by Phases 7–9
once Phase 6's encoder structures are settled.

*Exit gate: the screen-content sweep byte-identical against the C++ in both
profiles; the `SCREEN_CONTENT(dormant)` tag count reads zero.*

---

## 6. File → phase map

| Phase | Files (fate) |
|---|---|
| 0 | `mc.rs` (dead externs), `encode_mb_aux.rs`/`sample.rs`/`intra_pred_common.rs`/decoder kernels (dead SIMD stubs), `codec_api.rs` (2 stubs), decoder thread scaffolding in `decoder_context.rs`/`picture.rs`/`pic_queue.rs`/`decoder_core.rs`, `svc_encode_slice.rs` (writer dupe), `lib.rs` |
| 1 | new `safe/` module (+ unit tests) |
| 2 | `decoder/decode_mb_aux.rs`, `encoder/{encode_mb_aux,decode_mb_aux,sample}.rs`, `common/{sad_common,intra_pred_common,mc,deblocking_common,expand_pic}.rs`, `decoder/get_intra_predictor.rs`, `encoder/get_intra_predictor.rs`, `processing/{vaacalc,adaptive_quantization}.rs` (kernels) |
| 3 | `decoder/{bit_stream,dec_golomb,cabac_decoder,nalu}.rs`, `encoder/{vlc_encoder,set_mb_syn_cabac,svc_set_mb_syn_cavlc,nal_encap}.rs`, `common/wels_common_defs.rs` (SBitStringAux shell) |
| 4 | `encoder/wels_func_ptr_def.rs` (dissolved), `encoder/paraset_strategy.rs`, `ref_list_mgr_svc.rs` (vtable half), `processing/mod.rs` + `wels_preprocess.rs` (IWelsVP half), `decoder/decoder_core.rs:1913-1995` + `decode_slice.rs` transmutes, `decoder/decode_slice.rs`/`deblocking.rs` table calls |
| 5 | `decoder/{picture,pic_queue,manage_dec_ref,error_concealment,mv_pred,parse_mb_syn_cavlc,parse_mb_syn_cabac,deblocking,decoder_core,decode_slice,decoder_context,fmo,slice,parameter_sets}.rs` (last four mostly type/receiver touch-ups) |
| 6 | `encoder/{ref_list_mgr_svc,picture,wels_preprocess,md,svc_base_layer_md,svc_mode_decision,svc_motion_estimate,svc_encode_mb,svc_encode_slice,svc_enc_slice_segment,deblocking,rc,au_set,param_svc,wels_encoder_ext,encoder_context,encoder_ext,abi_guard}.rs`, `processing/{background_detection,complexity_analysis,scene_change_detection}.rs`, `common/memory_align.rs` (deleted) |
| 7 | `common/wels_thread_pool.rs`, `encoder/{wels_task_management,slice_multi_threading}.rs` |
| 8 | `api/codec_api.rs` (internals rewired; ABI surface frozen), `api/abi_guard.rs` (extended), `Cargo.toml` (cdylib), new external-ABI harness under `tools/`; `tests/*`/`benches/*`/`diffharness/rust_enc` deliberately untouched — they stay on the C API |
| 9 | `lib.rs`, crate-wide lint/doc passes |

Untouched throughout: `decoder/vlc_tables.rs`, `common/cpu_core.rs` (trivial), `tests/common/`.

---

## 7. Verification & tooling

**Inherited from Phase 6 (written session I, 2026-08-20).**

* **The `SCREEN_CONTENT(dormant: Phase 10)` census: 24 tagged items**, across
  `svc_motion_estimate.rs` (13), `picture.rs` (5), `svc_mode_decision.rs` (2), and
  one each in `encoder_context.rs`, `rc.rs`, `ref_list_mgr_svc.rs` and
  `wels_preprocess.rs`. **Two of the 24 are function-level tags covering the 8
  dormant allocator call sites** (`svc_motion_estimate.rs:1529` over
  `RequestScreenBlockFeatureStorage`, `:1607` over its free), so the census is 24
  items *including* the allocator family rather than 16 plus 8 — session I's brief
  had it as "16 tagged + 8 allocator", and the grep disagrees (hard rule 4).


### 7.1 The unsafe ratchet
`rust/tools/unsafe_ratchet.sh`: per-file counts of `unsafe fn`, `unsafe {`, `*mut|*const`, `transmute`, `unsafe impl`, `mem::zeroed`, `no_mangle`, `SHIM(` vs a checked-in baseline JSON. CI (or the session-start checklist) fails on any increase; each phase commits a decreased baseline. This — not `unsafe_op_in_unsafe_fn` churn — is the progress meter. (Flipping that lint crate-wide would add thousands of `unsafe {}` wrappers only to delete them again; don't. It gets enabled per-module as modules go safe, where it's vacuous.)

Built in Phase 2, session A. Invocations, from anywhere:

```bash
bash rust/tools/unsafe_ratchet.sh generate   # (re)write rust/tools/unsafe_baseline.json
bash rust/tools/unsafe_ratchet.sh check      # non-zero if any file × metric increased
bash rust/tools/unsafe_ratchet.sh report     # same output, always exit 0
```

Two counting rules the script encodes, both learned the hard way:

- **`unsafe fn` alone does not match `unsafe extern "C" fn`.** Phase 0's T5b deleted 113 definitions spelled that way and the naive pattern reported no change at all. The pattern is `unsafe (extern "C" )?fn`.
- **That pattern also matches function-pointer *types*** — `Option<unsafe extern "C" fn(..)>` occurs ~210 times in the dispatch tables. Definitions are therefore counted **line-anchored**; the pointer types show up in `raw_ptr`/`transmute`, which is where Phase 4 will show its work. Baseline at `956a8c07`: `unsafe_fn` 1372, `unsafe_block` 507, `raw_ptr` 5390, `transmute` 23, `unsafe_impl` 11, `mem_zeroed` 26, `no_mangle` 42, `SHIM(` 0. (§1.1's "~922 `unsafe fn`" was the naive count; 1372 is the real one.)

### 7.2 The gate battery (every merge)
0. Both build profiles compile and agree (debug **and** `--release`; the pre-Phase-0 release segfault means any debug/release divergence is treated as UB evidence, not flakiness).
1. `cargo test` and `cargo test --release` — all green, `#[ignore]` set unchanged.
2. Conformance SHA-1s **and frame counts** unchanged (a decode error that drops frames must not hide behind a "different hash" — check counts first).
3. Loopback + option-sweep + LTR tests unchanged.
4. Diffharness: at minimum the gate configuration; full sweep (`st`, `mt`, `def` — slice modes × RC × threads × the defaults path) at phase exits.
5. Ratchet decreased or equal; `SHIM(` count per plan.
6. Fuzz: corpus regression run, no new panics/timeouts.
7. From Phase 8 on: external-ABI harness (C++ driver + Rust cdylib via dlopen) hashes match in-process results; cdylib exported-symbol list unchanged.

### 7.3 Fuzzing (from Phase 0, forever)
`fuzz_targets/decode_annexb.rs`: feed arbitrary bytes through the full decode+flush sequence; assert no panic, no abort, bounded memory. Seed with `res/`. After Phase 3, add a structured mutator target for NAL-level mutation. Stretch (worth it by Phase 5): differential target comparing Rust vs dlopen'd C++ on error codes + output hashes — it converts the byte-exactness property into a fuzzable invariant.

### 7.4 Performance budget

#### Current regime — v3, decision D-perf-4 (Eugene, 2026-08-09): safety first, speed tracked and recovered at checkpoints

*Supersedes the v2 gating rules below (kept as decision history). Direction: interim perf must not consume sessions or block conversion — the big unsafe mass is in Phases 3/5/6, and every session spent on a perf box delays it. We still want to be as fast as possible: fast idioms stay mandatory, everything stays measured and ledgered, and recovery is consolidated at the checkpoints instead of litigated per family.*

- **Priority order for interim work: byte-exactness > safety progress (ratchet) > speed.** Speed is never traded silently — it is ledgered and recovered at defined points — but it no longer stops a phase.
- **No hard ceiling; one tripwire.** The v2 "≤10% per stream, breach stops the phase" rule is retired. Instead: if a swap would push the **cumulative deficit past +25% median on any bench stream**, that family **parks** (proven, uninstalled — the existing §Parked machinery) instead of swapping; everything under the tripwire **swaps and ledgers by default**. A single family projecting >15% on a stream gets flagged in its ledger entry but still lands if the cumulative holds. No session stops, no escalation mid-phase — egregious surprises go in the log for Eugene, work continues.
- **Measurement, lightweight but honest:** commit B runs **one interleaved pair on both benches** (enough for a ledger entry and a tripwire check; the full 3-pair-medians protocol moves to phase exits only). The instrument rules stand — interleaved pairs (never sequential), null run before any "noise" claim, working set sized from the real caller, post-swap "raw" recovered via `git show`. The per-family microbenchmark is now **diagnostic, not mandatory** — build it when a number surprises, not before every family.
- **No optimization boxes during conversion phases.** If a family lands ugly: one *short* look (profile + disassembly, an hour, not a half-session box), then ledger-or-park by the tripwire arithmetic and move on. Multi-attempt investigations (T4's, T5-sad's) are retired until the recovery checkpoints.
- **The recovery checkpoints are where speed comes back**, and they are unchanged: **Phase 4** (direct dispatch — re-measure every ledgered family; the `mc.rs` dispatch-forward experiment from D-perf-3 folds back into Phase 4 proper as its first task, and D-perf-2's slices-and-offsets idea for tiny-block kernels is queued there too); **Phase 5/6** (shim deletion and caller conversion — scaffolding cost and parked families' raw bodies both die here by construction); and a **dedicated perf-recovery pass at Phase 9**, driven by the ledger, with the end-state target unchanged: **≤10% cumulative vs C++ scalar at Phase 9, aspirationally parity**. The ledger and §Parked tables are the contract that "for now" means *for now*.
- **Fast-by-construction stays binding:** the idiom catalog (const-generic copy widths, zips, exact-span trims, inline parity, `row()` over walkers) and the negative-results list are still rules — writing the fast version first costs nothing and is most of "as fast as possible".

**D-perf-5 (Eugene, 2026-08-12): recovery-first at 5.2's midpoint.** Session H's
first eleven grid families cost **+1.32% decode / +2.05% CB at 7 pairs** (cumulative
CB ≈ +19.9% against the +25% tripwire), and the eleven remaining families are the
hot ones (`pNzc`, `pMv`, `pMvd`, `pRefIndex`, `pScaledTCoeff`). Direction: **before
any hot family flips, a dedicated session retrofits per-MB window accessors** (S8's
amortization idiom — one bounds check per MB per array, the C++'s own hoisted-pointer
shape) **across the flipped eleven and measures the recovery at ≥7 pairs.** The hot
families flip only with the proven idiom and the recovered headroom. The
tripwire-vs-S20 question — whether the wire may park a family mid-closure — is
deferred until that measurement exists; if it still binds afterwards, it returns to
Eugene with the data.
**D-perf-5's measurement exists, and it is negative (Phase 5 session I, 2026-08-12).**
The retrofit landed across all eleven families and does mechanically what the direction
asked — 76 bounds checks per macroblock on the CAVLC path become 38 — and recovers
**nothing**: decode median **+0.14%** at 7 pairs against a bar of −0.66%, every row
inside the session's null band. §3.4's look found the reason rather than a defect: the
functions involved are **1–3% of decode time** by profile, so a 2% recovery was never
available there. Two facts for the decision that follows, both new:

* **The flip's own span re-measured at half.** Session H's two stashed binaries, same
  7-pair protocol, a different day: **+0.65% decode median / +1.18% CB** against the
  recorded +1.32% / +2.05%, with the row carrying the median swapping ends. The cost is
  real — every row positive in both readings — and its *magnitude* is not stable at seven
  pairs. The cumulative CB figure this decision rests on (≈ +19.9%) is high by roughly a
  factor of two. **S2b extends: seven pairs is not a guarantee of convergence either, and
  a second reading on a different day beats more pairs on the same one.**
* **The null result does not transfer to the hot eleven.** The families retrofitted here
  are one scalar per macroblock, where a window only amortises repeat visits. `pNzc`,
  `pMv`, `pMvd` and `pScaledTCoeff` are indexed *inside* the record in inner loops, where
  the element re-type (already built at T5.H2) makes the inner index const-bounded — a
  different mechanism, untested.

**Direction on the return (Eugene, 2026-08-12): probe first, then flip.** Session J
builds a second probe stream with a real macroblock grid before any hot family moves
(F34: every Miri verdict this phase was bounded by a one-MB stream — the neighbour
paths the hot families exercise have never run under the checker). Then the hot
eleven flip **one family per commit under D-perf-4's normal swap-and-ledger**,
7 pairs each with a second-day confirmation on any reading that decides something
(the S2b extension above), **hottest first** so the wire question resolves earliest.
Stop-line: cumulative CB ≈ **+23%** — stop at a family boundary and escalate with
the data. Parking, if it comes to that, is family-granular and S20-coherent; its
cost is phase-exit debt, not a broken tree.

**D-perf-6 (Phase 5 session V, 2026-08-15, under Eugene's closing order): the Phase
5 stop-line breach is recorded and its recovery deferred to the Phase 9 perf pass.**
The escalation table's **option 6**, taken after option 2 (its own recommendation)
was spent. The measurement: the D-gate-1 window `d0b7f399`→`e6873fe1` read **+3.58%
CB / +2.77% decode median** at 7 pairs in session U's morning and **+3.38% CB /
+2.65%** at 7 pairs ≈10 hours later — same sign, same row order, medians 0.12 points
apart, every decode row above its own reading's null ceiling, encode flat in both.
So **cumulative CB ≈ +25.0…+25.5% against the ≈+23% stop-line: over by ≈2.0–2.5
points**, and D-perf-4's +25% *median* tripwire is not breached (both stated).
What the decision is, precisely:
- **Recovery is owed and scheduled, not waived.** It goes to the Phase 9 perf pass
  driven by the ledger — D-perf-4's own disposition for a deficit nothing attributes
  — on the precedent the ledger already carries (Phase 2's shim families).
- **Nothing parks, because nothing is attributed.** The window's bisect does not
  resolve (the CB rows sum, the medians do not; session K's law), and a per-session
  split is finer, so it would resolve less rather than more. Option 5 has option 4
  as its prerequisite and option 4 is refused on those grounds.
- **The line does not move.** Option 1 — re-baseline to ≈+26% — is refused: the
  phase exits *over* its stop-line with the overage named, rather than under a line
  redrawn to fit it.
- **The exit is not blocked by it.** Exit condition 5 is "the perf adjudication done
  and written"; it is done, written, day-two'd, and dispositioned. The breach is a
  ledger fact Phase 6 inherits, not an open question.
`perf_baseline.md` §Phase 5 exit carries both readings, the two nulls, the niche's
second reading, and this disposition.

**D-par-1 (Eugene, 2026-08-14, mid-session S, verbatim): "if InitErrorCon is called
in C++ code, it needs to be called in Rust code as well. we aim for test parity
first, safety refactoring comes later."** What it reorders: behavioral parity with
the C++ decoder — outputs *and* API-visible behavior (return codes, frame timing)
on the full corpus including damaged input — outranks the remaining safety work
(W6/W7's view struct and the `deny` rollout), which stands **deferred, not
cancelled**. What it does not change: output equivalence stays the correctness
definition (D-fid-1's reference clause), byte gates per commit, the goldens'
authority — with the F43 precedent that a golden pinning *port* behavior against a
divergent C++ regenerates from the C++'s answer (`tools/ecref`), each move logged.
Executed from session S on: F43/F44/F45 fixed under it (truncation-row agreement
641/541 → **1182/0**), F46 scheduled as the next parity item, W6/W7 wait for the
phase's resumption.

**D-fid-1 (Eugene, 2026-08-13): structural fidelity to the C++ is retired; the C++
remains the reference for correctness, optimization, and inspiration.** The safe
rewrite has already diverged structurally (ids for pointers, owned containers,
constructors, `Drop`), so function-level correspondence stops being a constraint:
functions may merge, split, restructure, and take Rust-shaped names where that
serves the safe design. **What does not change**: output equivalence stays the
correctness definition — the goldens, sweeps, and conformance hashes *are* the
C++-as-reference, and D2's drop-in guarantee stands. Behavioral divergence from
the C++ is still a finding or a decision, never a side effect. Consequences:
§2.1.6 and P14 are retired as constraints (provenance headers become optional
documentation); D5's rename question is unlocked early where naming serves
clarity; S6 is rescoped in place (see its note).

**Session I stopped there, per the direction's own §3.4.** Whether the hot families flip,
and on what basis, is Eugene's call on these numbers. The tripwire-vs-S20 question stays
deferred and untouched.

**D-gate-1 (Eugene, 2026-08-12): sprint gating, in force until Phase 5's exit, then
revisited.** Direction: finish fast; gates were consuming 1.5–2h per session.
- **All mid-phase perf measurement moves to phase exits** — no nulls, pairs, or spans
  during sessions. The 5.2 cumulative is settled (+20.4…+20.7% CB, day-two-confirmed);
  the ≈+23% stop-line is checked at the phase exit with the full S2/S2b protocol.
  S2b/S2c stand in letter but issue no mid-phase verdicts.
- **Per-commit gate (~3 min): build both profiles + tests + ratchet + census.**
- **Once per session, at close: the full battery** — decode goldens, sweeps 341/341
  both profiles, benches-bit-identical, Miri with both probes. FAILs adjudicated per
  S14/S16 as always; a byte divergence found at close is bisected across the
  session's commits (they are small by S20 discipline — that is the trade accepted).
- **A parallel encoder track is authorized**: a second worktree, encoder-only files,
  scoped to work no Phase 5 session touches. Hard rule: the two tracks never edit
  the same file; a closure that crosses `common/` stops and waits for the decoder
  track. Each track runs its own per-commit gate; the session-close battery runs on
  the merged tree.
- **Phase exits keep the full protocol** — perf included. Byte-exactness remains
  never-traded; what this decision moves is *when* it is checked, not whether.

*The v2 rules and decision records D-perf-1…3 follow as history; where they say "hard ceiling", "breach stops the phase", "body-parity precondition", or "half-session box", D-perf-4 supersedes them.*

*Restated 2026-08-08 after T4 (`common/mc.rs`) became the first family to exceed the original flat budget and the investigation showed the flat budget was measuring the wrong thing during a strangler phase. Decision record below.*

- Baseline: Phase 0 bench medians (`c_vs_rust_bench`, `decode_1080p_bench`). Every cumulative figure in this plan is a chain of **Rust-vs-Rust** spans anchored there, which is what makes them additive. **The Rust-vs-C++ ratio the decode bench also prints is a different quantity and is not scalar-vs-scalar** (corrected Phase 5 session L): the checked-in `libopenh264.2.6.0.dylib` reports `WelsCPUFeatureDetect = 0x000006` and the bench's own header prints `C++ SIMD : ACTIVE`, so that column compares scalar Rust against NEON C — +56.8% on Constrained Baseline against +3.1% on Main at session L's HEAD, a spread that is the assembly's, not the port's. The end-state "≤10% vs C++ scalar" target below needs a scalar-only dylib to be measurable at all; it has never been measured against this one.
- **Measurement protocol (normative since T4, extended by T5):** sequential `cargo bench` runs of the *same binary* drift ~3%, which cannot judge a 5% budget. Build control and candidate binaries, keep both on disk, run them **interleaved in one loop**, 3+ pairs, medians — and interleave *variants inside* a microbenchmark process too, not just binaries. For hot families, build the per-kernel scratch microbenchmark (old-via-`git show` vs new, per phase and block shape) *first*, not after the numbers look bad — it is what separates kernel cost from scaffolding cost. Profile (`/usr/bin/sample`) and **disassemble** before theorizing; T4 and T5 each burned two build-and-bench cycles on hypotheses that one disassembly refuted.
- **A microbenchmark's working set is part of its correctness (T5).** T5's SAD microbenchmark ran at a 1984-byte stride over 190 KB, so every row access missed cache and the safe kernels' per-row overhead hid behind the misses: it reported 0.82–1.33x where the encoder reported **+16.8%**. The same benchmark L1-resident reported 1.0–2.0x and agreed. **Size the working set from the real caller, state it beside the numbers, and measure both residencies when the caller's is not obvious.** Four more microbenchmark bugs T5 hit, each of which produced a plausible table first: no per-iteration `black_box` (LLVM caches results); observing only one of several outputs (LLVM deletes the rest of the kernel while the opaque raw call keeps computing it); a `const` stride where the real kernel takes a runtime one; variants timed in blocks rather than interleaved.
- **`FFMPEG` must be set for every gate run.** It was unset for the whole of Phase 2 sessions A and B, so `c_vs_rust_bench` skipped and the encoder was **unmeasured** across three families. T5's regression is the first one it would have caught. Encoder-side families (T7 especially) have no other end-to-end instrument.
- **Bisect a swap by file before optimising it.** T5 read as +13.9% overall; one extra build and two bench runs split it into a +16.8% half and a +0.57% half, which turned a wholesale revert into a narrow one.
- **Two ledgers, not one budget:**
  - **End-state regression** — cost intrinsic to the safe kernels/algorithms themselves, as isolated by the paired microbenchmark. Budget unchanged: bodies at ≤1.05x per family (investigate anything over), ≤10% cumulative at Phase 9 on the full-stream benches. This is the number that must hold forever.
  - **Scaffolding deficit** — overhead attributable to the strangler shims (span arithmetic, `from_raw_parts`, constructor asserts at the raw boundary), which Phase 5 deletes with the shims. A family may carry a scaffolding deficit past 5% **only if all three hold**: (a) the paired microbenchmark shows kernel bodies at ≤1.05x, (b) the overhead is demonstrated to be fixed per-call shim cost, not per-sample kernel cost, and (c) a **deficit ledger** entry in `perf_baseline.md` names the family, the measured deficit per stream, and the phase that deletes it. Hard ceiling regardless: **≤10% total regression on any bench stream at any commit** — a breach stops the phase for recovery (slim the specific shims, or unswap that family's commit B as the last resort; the two-commit discipline exists so unswap stays cheap).
- **Recovery checkpoints:** Phase 4 (direct dispatch makes shims inlinable into callers — measure the ledgered families' streams and record the recovery); Phase 5 (shim deletion — every ledger entry must clear; the ledger going empty is part of Phase 5's exit gate).
- Safe-but-fast idioms, as measured through T4 (order matters): **const-generic widths for copies** — `copy_from_slice` on a runtime length is a `memmove` *call*, and `copy_rows::<16>` vs `copy_rows(16)` was 10.8x vs ~1x; fixed-size array windows (`try_into().unwrap()` → `&[u8; 16]`); **zip iterators instead of indexing several slices by `j`** (LLVM will not prove seven bounds facts per sample); a rolling row window over re-fetching rows; `#[inline(always)]`/`#[inline(never)]` parity with the C++'s `inline`-plus-table structure; `row()`'s statically-sized window per row — **not** `chunks()`-based walkers, which measured worse everywhere they were tried (`perf_baseline.md` §Phase 2 T4, the row-walker table). `get_unchecked` is **banned** — if a hot loop can't be made fast safely, restructure the loop, don't reintroduce unsafe.
- Expect some *wins*: direct calls replacing indirect dispatch (Phase 4) and `match` replacing fn-pointer tables are LLVM-inlinable where the C design never was; T2 measured 1.3–1.8% *faster* and T3 a wash.

- **Parked families (added 2026-08-09 after T5-sad):** a family whose *bodies* cannot reach ≤1.05x on the honest microbenchmark at the shim boundary does not qualify for the deficit ledger (nothing later deletes body cost) and does not stay swapped — it is **parked**: safe kernels in-tree, differentially proven, spans pinned, shims unswapped, with a named re-attempt point (the Phase 4 direct-dispatch checkpoint, or the phase that converts its callers). Parked families are tracked in the ledger doc next to the deficits. Corollary, binding for every remaining call-dense tiny-block family (SAD/SATD class): **measure bodies on the L1-resident microbenchmark *before* swapping** — body parity is a precondition of the swap attempt, not something to discover from the stream bench afterwards.
- **Microbenchmark validity (added 2026-08-09):** per-call overhead must be measured **L1-resident** — T5's harness ran a 1984-byte stride over 190 KB and the overhead hid behind cache misses (0.82–1.33x reported against a real +16.8%). And the encoder bench is only a gate when it actually runs: `gates.sh` now locates ffmpeg itself after silently skipping the encoder bench for three encoder-touching families.

**Decision D-perf-1 (2026-08-08): T4's ~7–8% is carried as a scaffolding deficit, not reverted.** Basis: the kernel bodies measure 0.88–0.99x (at parity or better — the end-state cost is zero or negative); the residual is 7–16 ns of fixed per-call shim overhead on the most call-dense family in the codec; three structural mitigations were built, paired-measured, and rejected, so further optimization attempts are closed; and unswapping would forfeit the full battery's continuous exercise of the safe kernels while concentrating re-swap risk exactly where the strangler pattern exists to avoid it. The deficit is ledgered in `perf_baseline.md`, bounded by the ≤10% ceiling, checkpointed at Phase 4, and must clear at Phase 5. *Addendum 2026-08-09: with the encoder bench now actually running, T4's encoder-stream contribution was never isolated — measure it at the next session start and append it to T4's ledger row.*

**Decision D-perf-3 is OPEN and blocks nothing yet, but it is the phase's largest
outstanding question (raised 2026-08-09): the encoder-side ceiling is breached.**
D-perf-1's addendum asked for `mc.rs`'s encoder contribution to be isolated now that
the encoder bench actually runs. It is **+7.6% median and +16.7% worst** across 28
usable stream/thread rows, against a same-binary null floor of ±2% — eleven rows over
§7.4's 10%-per-stream hard ceiling — and T8's +3.5% sits on top of it, so the live
cumulative encoder deficit is roughly **+11%**. T4's ledger *eligibility* is unchanged
(bodies 0.88–0.99x, residual is fixed per-call scaffolding Phase 5 deletes); what is
new is the magnitude, and its cause is structural rather than fixable: motion
estimation calls MC far more often per frame than decoding does, so a fixed per-call
shim cost is levied far more often. The three options are carry-with-a-raised-ceiling,
revert `mc.rs` until Phase 5 converts its callers, or pull Phase 4's direct-dispatch
checkpoint forward for `mc.rs` alone (direct calls are what make a shim inlinable, and
inlining is exactly what deletes a per-call cost). Evidence and the full table are in
`perf_baseline.md` §"The encoder-side ceiling is breached". **This is Eugene's call,
as D-perf-1 was.**

**Decision D-perf-3 (2026-08-09): the encoder ceiling breach is recovered by pulling Phase 4's direct-dispatch checkpoint forward for `mc.rs` alone — boxed, with unswap as the pre-authorized same-session fallback.** The breach: T4's encoder contribution measured +7.6% median / +16.7% worst (11 rows over the 10% ceiling; cumulative with T8 ≈ +11%). Of the three options: raising the ceiling the first time it binds would convert the mechanism from a gate into a suggestion — rejected; unswapping outright clears the breach but retreats from a within-budget decode position and, worse, leaves the **Phase 4 recovery hypothesis untested** while T6/T7 accumulate more scaffolding deficits premised on it. Direct dispatch for `mc.rs`'s consumers is a surgical slice of Phase 4 (~13 call sites already enumerated in T4's log), it attacks the measured mechanism exactly (fixed per-call cost that inlining removes — reinforced by D-perf-2's finding that cursor construction is a large per-call cost at small block sizes), and it converts the ledger's load-bearing assumption into evidence either way. Success metric: every encoder stream's cumulative deficit back ≤10% (mc median expected ≲5%), decode not regressed, all gates green. Fallback if the box closes unmet: revert the dispatch commit, unswap `mc.rs` commit B, park the family (both sides), and **downgrade every "Phase 4 checkpoint" claim in the ledger to "re-evaluate at caller conversion"** — if inlining doesn't recover scaffolding cost here, it won't elsewhere, and the plan must stop promising it. Corollary rule, binding from T6 on (session D's finding): **a per-call deficit scales with call count — an encoder-side family's budget is not its decoder-side budget; every commit B measures both benches before merging.**

**Decision D-perf-2 (2026-08-09): T5-sad gets one bounded re-landing attempt, scheduled with T8 in the next session — T6's order is unaffected, and T7 inherits the verdict either way.** The question the attempt answers is not "can SAD be a bit faster" but the technique question that gates T7's ~30 SAD/SATD-shaped kernels: whether per-row cost on tiny fixed blocks with runtime strides can be made to fold in safe code at the shim boundary at all. Time-box: half a session, hard stop, disassembly-first (both prior guesses measured null; `row_windows` alone was not enough). Exit states, both acceptable: (a) bodies reach ≤1.05x L1-resident and the family re-swaps inside the ceiling — the technique transfers to T7; or (b) it doesn't, T5-sad stays **parked** until the Phase 4 checkpoint or its callers convert (ME is Phase 6.3), and T7's SAD/SATD families go **prove-and-park** from the start instead of burning swap-measure-unswap cycles. What is not acceptable is a third unbounded investigation: one attempt, one verdict, recorded.

**Decision D-scr-1 (2026-08-19): screen content is planned scope, not dead scope.** `SCREEN_CONTENT_REAL_TIME` is live, supported code in the C++ — validation accepts it with constraints (`encoder_ext.cpp:274`/`:411`), the FME storage allocates for it (`:1032`, `:1129`), and `PerformFMEPreprocess` runs per frame (`:2747`) — while the port blocks the mode with exactly one **port-added init-time guard** (`encoder_ext.rs:817`, "feature search storage is not ported"). Ruling: the in-tree search half is **kept, raw and unconverted**, tagged `SCREEN_CONTENT(dormant: Phase 10)` at Phase 6 session E — S18's delete rule does not apply to named future scope, and converting it now would be unverifiable because no gate reaches it. The preparation half deleted at T6.D2 returns as a re-port from the C++ in **Phase 10** (§4), which owns the guard's removal, the VAA screen extension, the screen complexity analysis, the storage-allocation wiring, and the harness coverage that makes any of it checkable. *Clause (session E): a dormant item that shares a dispatch-slot type with live functions takes the slot's retyped signature, body unchanged — three functions took one-line `*mut SWelsME` → `&mut SWelsME` edits this way, recorded in the log rather than smoothed over.*

**Decision D-gate-2 (2026-08-20, direction at session G's close): Miri runs encoder-scoped for the remainder of Phase 6.** The `--lib` Miri step excludes the three decoder probes and the decoder-module tests in every remaining Phase 6 session — Phase 6 does not touch the decoder, and the decoder's share is roughly half of the step's ≈1400 s. Session H builds the knob in `gates.sh` (step 0) and records the scoped count and wall time beside the unscoped 349/1411 s. **The full unscoped step runs once more inside Phase 6 — at session I's phase close, as the handoff gate** (steward's interpretation of the direction; veto by saying so). G's step 0 is the shape that keeps this safe: the decoder's own S40 test now exists and runs with the decoder's own batteries, so scoping Phase 6's runs forfeits no coverage a Phase 6 session could have created.

**Decision D-exit-1 (2026-08-20, steward, ruling F65 by its option 1): condition 2 of Phase 6's exit gains a fifth lawful category — `port-raw(<phase>)`.** A `port-raw(Phase 7)` / `port-raw(Phase 9)` tag marks a port-internal raw-pointer *signature* whose parameter family is owned by that phase; the tag dies with the item when the owning phase converts it, and an untagged or unowned `unsafe` remains a build error — which is everything condition 2 was written to protect (the enumeration, not the lint). F65's option 2 — deferring condition 2 past Phases 7–10 — is rejected: it would run three phases inside an open Phase 6 and make "COMPLETE" an event nobody can schedule against. The reason the fifth category is honest rather than a loophole is session I's own asymmetry measurement: encoder `raw_ptr` fell 2669 → 1849 (−31%) across the phase while `unsafe_fn` went 710 → 709 — Phase 6 takes raw pointers out of *data*, and `deny(unsafe_code)` counts *signatures*, which die only when Phases 7 and 9 move the families their parameters name. The in-code tags are the census, self-maintaining: each is deleted by the same commit that converts its item.

**Decision D-mt-1 (2026-08-20, steward, ruling F67 by its option 2): the fork/join crosses one named seam.** One job type carries the raw context pointer across `thread::scope` with the phase's **single** `unsafe impl Send`, documented by a three-part safety argument — premise 1 (slot disjointness, proved T7.A1), premise 2 (order-based assembly, proved T7.A1), and session A's bound (concurrency capped at the *allocated buffer* count, not the slot count) — and tagged `send-seam(Phase 9)`: it retires when Phase 9's context split makes the job naturally `Send`. The workers' access pattern does not change; only the machinery shrinks (5 `unsafe impl` pairs → 1; the pool, the C++-list ports, the `static mut` and the claiming mutex deleted). **Option 1 rejected**: building the context split now is Phase-9-shaped work against S42's own ordering (the arena-root converts after its cursor families retire) and pulls five P8/P10-owned `!Sync` types forward across D-scr-1's fence. **Option 3 rejected**: keeping the crossing machinery alive muddies the ablation this phase exists to run — F3's candidate mechanism must be *deleted*, not redecorated, for either verdict to mean anything. §2.2.7's "all 5 `unsafe impl` pairs are deleted" is amended by this ruling to "5 → 1 named seam, the last retiring in Phase 9"; A's hard rule 7 is superseded for exactly that one type.

### 7.5 Session workflow
Each session: run battery → pick next plan item → convert with shims → battery → commit with ratchet update → update this doc's checkbox (add a `## Progress` appendix on first execution session). The phase structure is deliberately interruptible; no step leaves the tree broken across a session boundary.

The battery is `rust/tools/gates.sh` (built in Phase 2, session A), in four levels so the
cheap ones can run per commit:

```bash
bash rust/tools/gates.sh commit   # cargo test debug+release + ratchet          (~2 min)
bash rust/tools/gates.sh family   # + diffharness st/mt/def in BOTH profiles    (~5 min)
FFMPEG=/opt/homebrew/bin/ffmpeg bash rust/tools/gates.sh full    # + benches + Miri --lib
FFMPEG=/opt/homebrew/bin/ffmpeg bash rust/tools/gates.sh exit    # + Miri over tests/*differential*
```

It prints one `PASS`/`FAIL`/`SKIP` line per gate and exits non-zero on any failure.
Three behaviours are deliberate: the test gate checks the **ignored count is exactly 20**
as well as the exit status (a test that stops being compiled looks like a test that
passes); a sweep failure prints **F3's retry rule** next to the result rather than
leaving each session to rediscover it; and the encoder bench **SKIPs loudly** without
`FFMPEG`, because its fallback measures the frame-skip path rather than the encode
kernels (`perf_baseline.md` §2). The fuzz gate (§7.2 gate 6) prints a permanent SKIP
until T7 lands.

### 7.6 Standing working rules

*The criterion for being here is that a current session needs the rule **verbatim**.
Briefs cite this section by tag instead of copying rules forward (S26). Rules marked
**[dormant: kernel work]** apply only when kernel conversion or a kernel microbench
is in scope — next expected use is 6.3's parked-family re-attempt. Section revised
2026-08-11: S14 refreshed to the measured protocol; S17 automated. Pre-Phase-3 logs
cite these rules by their original R-letters: S1=R-h, S2=R-l, S3=R-j, S4=R-m, S5=R-k,
S6=R-e, S7=R-c, S8=R-i, S9=R-o, S10=R-d, S14=R-g, S16=R-f+R-p.*

#### Measurement

- **S1 — measure with interleaved pairs, never sequential runs.** Two runs
  of the *same* binary drift ~3%. Keep both binaries on disk, alternate them inside
  one loop, take medians over 3+ pairs. An unpaired reading of T4 said +5.1% where
  the paired one said +8.7%, and the session acted on the wrong number for a while.
  Profile with `/usr/bin/sample` and **disassemble before theorising** — T4's and
  T5's first two theories were each wrong and cost a build-bench cycle.
- **S2 — run a null before calling anything noise.** Same binary in both
  slots, same harness, fresh **per session**: the floor is not a constant (session D
  ±1.8%, session E ±5.4%, sessions F and G ≈±2.5-3%). It costs one pass of a bench
  that is already built, and in T8 it converted a suspected artefact into a real
  +3.5%. **`Spatial Ramps` is excluded from every verdict** — it has moved ±38%,
  -11% and +226% between runs of one binary — but record what it says, because
  gradient content is the per-block-scaffolding path at its purest and Phase 4a's
  checkpoint wants that datapoint.
- **S2b — when a median lands outside the null band, the first move is more pairs,
  not a diagnosis** (Phase 3 exit, 2026-08-11). Three interleaved pairs is a floor,
  not a guarantee of convergence. At Phase 3's exit, going 3 → 5 pairs moved
  whole-phase decode from **+0.68% to −1.93%** — a 2.6-point swing that changed the
  *sign* — and moved a second span's decode median from +1.21% to +0.16%, one row
  going +1.36% → −0.03%. That session's null had already said why: the encode band
  was −1.53% … **+22.57%**, same binary against itself. The +1.21% figure, taken at
  face value, was about to be written up as a real cost of three seams that touch no
  decoder code. **Two measurements of one span disagreeing in sign is evidence the
  effect is below the measurement error**, and D-perf-4's disposition for that is
  diagnostic-only — but you only get that evidence by re-running at a higher pair
  count before reaching for a mechanism. **Seven pairs is not a guarantee either**
  (Phase 5 session I): the flip's span re-measured at half from the same two stashed
  binaries on a different day — a second reading on a different day beats more pairs
  on the same one, so any reading a decision rests on gets a day-two confirmation.
  **And the null must be run at the verdict's pair count** (Phase 5 session K,
  proposed by that session; ratified 2026-08-12).
  T5.J3's day-two confirmation was judged against a **3**-pair null while the verdict
  was **7** pairs. The 3-pair band read −0.16%…−0.02%: 0.14 points wide, entirely on
  one side of zero, and it put the verdict *outside*. A 7-pair null run straight
  afterwards on the same machine read −0.22%…+0.25% — 0.47 points wide, straddling
  zero — and put it *inside*. A band is a min-to-max over the bench's rows, so it is a
  sample like anything else, and a three-pair sample of it is not a floor for a
  seven-pair verdict. **The larger result from the same session, and the one that
  matters more:** three 7-pair readings of that one span read **+0.03%, +0.27%,
  −0.13%** — disagreeing in sign — against three session nulls that disagree at least
  as much. **When the effect and the yardstick are the same size, no reachable pair
  count decides it; measure something bigger.** For 5.2 that means the cumulative span
  rather than the per-family one: the stop-line gates on cumulative CB, which is ~20%
  and far above the floor, not on a sum of per-family numbers each below it.
  **And at the resolution limit, per-unit readings under-report *systematically*,
  not just noisily** (session L's proof): fifteen families each measured "free" —
  +0.24%, +0.04%, +0.04% CB — while the whole-flip span read **+2.93% CB**; an
  effect at the resolution limit hides one unit at a time, and the units' sum is
  not the span. A string of at-the-limit "free" verdicts is a prompt to measure
  the span, never a conclusion.
- **S2c — a seam that cannot plausibly move may waive its per-seam medians**
  (Phase 5 session G, adopted 2026-08-12). Conditions, all required: the face's
  output is byte-identical (the battery's goldens/sweeps/bench-identity are the
  proof, not an argument); it touches no kernel, allocation path, dispatch, or
  shim retirement; and the waiver is stated in the log with those conditions
  checked off. Pointer-spelling and prose-only faces qualify; anything structural
  does not. The phase exit's whole-span 3-pair protocol still runs regardless —
  the waiver skips a measurement that cannot inform, never the one that can.
- **S3 [dormant: kernel work] — a microbenchmark's working set is part of its correctness.**
  T5's SAD bench at a 1984-byte stride over 190 KB reported 0.82-1.33x where the
  encoder reported +16.8%; the same bench L1-resident reported 1.0-2.0x and agreed.
  Size the working set from the real caller, state it beside the numbers, and
  measure both residencies when the caller's is unclear. Four further ways a
  microbench lies, each of which produced a plausible table first: no per-iteration
  `black_box`; observing one of several outputs (LLVM deletes the rest of the safe
  kernel while the opaque `extern "C"` call keeps computing it — a 4x handicap
  pointing the wrong way); a `const` stride where the kernel takes a runtime `i32`;
  and variants timed in blocks rather than interleaved.
- **S4 [dormant: kernel work] — after a swap, the "raw" side of a microbench is the shim.**
  Calling `WelsFoo_c` post-swap measures the safe kernel wrapped in a shim against
  the safe kernel. T8's first table read 0.99-1.07x for exactly this reason; the
  true figures were 0.69-1.47x. Measure bodies **before** commit B, or recover them
  with `git show <commit-A>:<file>` into the bench crate.
- **S5 [dormant: kernel work] — bisect a swap by file before optimising it.** T5 read as +13.9%
  overall; one extra build and two bench runs split it into a +16.8% half and a
  +0.57% half, turning a wholesale revert into a narrow one. The two-commit-per-family
  discipline makes the unswap cheap; a per-file bisect makes it small.

#### Conversion

- **S6 — arithmetic parity, not repair** *(rescoped by D-fid-1, 2026-08-13: structural faithfulness is retired; behavioral faithfulness remains the gate. A fix restoring C++ behavior lands as a finding with a test — F21/F37/F40 precedent. A change diverging from both trees is a decision, not a session's call. On ungated paths — malformed input the corpus misses — the never-widen default below stands, because there the gates cannot referee)*.** When a kernel's intermediates can
  overflow, the safe kernel reproduces the **old Rust port's exact behaviour**: same
  widths, same operations, the same debug-panic exposure and the same release
  wrapping. Never widen, never add a `wrapping_*` the old code lacked, never "fix" it
  — that is a behaviour change on malformed input. Check the widths against both the
  old Rust and the C++ *before* writing each safe body, bound differential inputs to
  the range where the old code does not panic, and write the derivation next to the
  bound. A newly *noticed* overflow-capable intermediate gets an F-finding and
  nothing else (F8, F9, F11).
- **S7 — one span helper per family, and the contract sentence is the
  deliverable.** Shims are not derivable from the signature alone. One helper
  computes the slice span and *nothing else does that arithmetic*. The sentence
  naming **why** a negative reach is legal (`PADDING_LENGTH`, an MV clamp, an
  availability gate) is what Phase 5 converts callers against. Prefer **per-kernel**
  reaches to a family union — a shared parameter makes each kernel's contract claim
  its neighbour's reach.
- **S8 — the fast-idiom catalog, and its binding negative results.**
  Bounds checks land per *row*, not per sample: one `row_mut` per output row
  amortises over the row's arithmetic. `copy_from_slice` on a **runtime** length is a
  `memmove` call — use const-generic widths (`copy_rows::<16>`), as the C++ dispatches
  fixed widths. Zip iterators instead of indexing N same-length slices by `j`. Roll
  the row window rather than re-fetching. Match the C++'s inline structure
  (`#[inline(always)]` inner kernels, `#[inline(never)]` on composites carrying big
  scratch). `fill`/`copy_from_slice` on fixed windows compile to the same wide stores
  the punned `u32`/`u64` accesses existed for — and in 140 T3 accesses `from_ne_bytes`
  was needed **zero** times. **Negative results, do not re-propose without reading
  `perf_baseline.md` §Phase 2 T4 first:** by-value cursors (wash),
  dispatch-over-shims (wash), `chunks()`-based row walkers (**worse everywhere**).
  `get_unchecked` is banned — restructure the loop instead.
  **Fourth negative result, and it is the expensive one to re-propose (Phase 5 session I,
  D-perf-5): hoisting a per-macroblock window over an array that is already one scalar
  per macroblock is a wash.** The retrofit turned 76 bounds checks per macroblock into 38
  across the decoder's CAVLC per-MB functions — the reduction is real and countable in the
  opcode histogram — and moved the bench **+0.14% decode median at 7 pairs**, inside the
  null band. Two reasons, both general: the checks are perfectly predicted and the length
  they load is in L1 beside the pointer; and the functions holding them were **1–3% of
  decode time** by profile, so the amortisation had nothing to amortise. It also *costs*
  code size — lifting a load out of a loop made one body simple enough for LLVM to unroll
  it much harder, 703 → 961 instructions. **Bounds-check amortisation pays per *sample*,
  as the row-level idiom above does; it does not pay per *macroblock*.** Check where the
  time is before hoisting: `/usr/bin/sample` first, histogram second.
  **And count the checks with an opcode histogram, not a landing-pad walk** — an index is
  `ldr`+`cmp`+`b.ls` and those three deltas move together, whereas walking back from
  `bl panic_bounds_check` undercounts badly, because LLVM merges the pads and most guard
  branches leave the function body for a shared thunk. The walk said the checks were a
  minority of the flip's cost; the histogram said ~87%.
- **S9 — the exact-span trim is the per-row-bounds-check fix that works.**
  Handed an open tail (`&plane[origin..]`) LLVM cannot relate `k * stride + W` to the
  length and re-checks every row; handed a window it knows is `(H-1)*stride + W` long,
  the checks fold. It took T8's walk from 1.82x to 1.02x. Try it first on any kernel
  whose disassembly shows `slice_index_fail` in a row loop. **It does not rescue
  SAD** — measured — so it is a technique, not a cure.

#### Proof

- **S10 [dormant: kernel work] — differential discipline.** Sweep selector/availability flags
  **exhaustively**, never randomly: T3 found kernels that ignore a flag they take, and
  a random sweep blurs exactly what a conversion gets wrong. Anchor test blocks at
  **random legal offsets** — kernels written around unaligned stores must be exercised
  unaligned. Compare **every written byte of the destination surface**, not the
  nominal block. Kill at least one deliberate mutation per family before commit A.
- **S11 [dormant: kernel work] (Phase 2 session E) — every span probe gets a golden direct run.** Span size, touch
  set, and anchor must be pinned by three *independent* mechanisms: an exact-span
  allocation under Miri, a touch-set assertion, and the shim's output compared against
  the safe kernel invoked directly at the contract's own geometry. A +1-anchor
  mutation walks straight through exact-span and touch-set assertions alone, because
  a shift along the line axis stays inside the touch set and filters plausibly.
- **S12 (F10) — a raw kernel's pointer footprint is bigger than its read footprint.**
  Kernels transliterated from C bump the row pointer after the *last* row, and a
  composite's sub-blocks bump from their **own** anchors. Any test handing an
  exactly-sized buffer to a raw kernel must size it **`(h + 1) * stride`**. Exact
  spans are for the *safe* side; restore them on the raw side only at re-landing.
- **S13 (Phase 2 session G) — an accommodation is only as wide as the instrument that found
  it.** F10 was recorded twice and fixed only in the differential file, because that
  was the only file Miri ran; the same UB sat in the kernels' own unit tests for three
  sessions. When a rule is first applied, **run the instrument everywhere it could
  apply**, not only where it fired. (S22 is this same law aimed at the instruments
  themselves and their scope lists.)

#### Instruments and gates

- **S14 — the F3 protocol** (text refreshed 2026-08-11 to the measured facts;
  history in `phase0_findings.md` F3 — forty-one measurements, fourteen
  alternations, twenty acquittals). **Signature:** `mt`, `sm=3`, `t∈{2,4}`, output
  of **any** wrong length — zero, short, or long — in **either** profile, on
  **either** bench clip (session N's measurement 37 took `320x192` out of it, as
  session K took `n=600` and session M took `cabac`/`rc`; every clause that has ever
  looked like a condition has turned out to be a rate artifact).
  `n=600` predominates and **is a rate artifact, not a condition** (corrected at
  Phase 5 session K, which drew the first `n=1500` hit in 34 measurements): `sm=3`'s
  `n` is the per-slice **byte budget**, so a smaller `n` cuts more slices per frame
  and performs more of the slice-list growths the race lives in. **Measured rate:**
  ≈ 1/800 configurations under a battery's interleaved sweeps, but **≈ 1/307 under
  sustained back-to-back presets** (session K, 6442 configurations), ≈ 1/100–150 on
  susceptible configurations — load density is part of the signature, and a
  battery's sweeps often draw zero.
  **Protocol:**
  0. **Hash shortcut** (session D, tenth-plus acquittal): if the two trees under
     comparison build byte-identical `rust_enc` binaries (e.g. the diff is
     docs/tests-only), base and head are *one binary* — record the hash equality
     and acquit by construction; no re-run or alternation can say more.
     **"Decoder-only" is not the trigger; "test-only" is** (session P): the driver
     links the whole lib, so production decoder code changes the encoder binary even
     though the sweep never runs a decoder. Hash it before assuming — one build.
  1. One hit → re-run that configuration 5×. A reproduction in isolation is valid
     evidence. Two *different* wrong lengths from one binary+configuration = race,
     not divergence — a deterministic port bug repeats its bytes.
  2. Two or more hits → alternate **whole `mt` presets back to back**, 12 per
     side, both binaries built once and swapped inside one loop, machine otherwise
     idle. Expect hits on both sides; the verdict is whether HEAD is *worse*, and
     S23b applies — a 0/0 alternation has not run, and a sweep inside a battery is
     not an alternation.
  3. Anything outside the signature — `st`/`def`, other configurations, wrong
     *bytes* rather than wrong length — is **real**: stop, revert, investigate.
  4. Append every measurement to F3 **at adjudication time, not at session
     close** — and the close reconciles `phase0_findings.md`'s running total
     against the log entry's claims. (Phase 5 session G found the ledger four
     sessions in arrears; it surfaced only because a zero-hit session had to look
     up where the numbering stood.) A clean sweep is a sample, not a 341/341
     signal; a session-start hit is a tendency, not a rule.
- **S15 — the Miri gate, and its skip list.** `gates.sh` runs Miri over the **whole
  library** (`--lib`, `-Zmiri-ignore-leaks`) plus the differential integration files
  at phase exits. The skips in that step are a **work queue, not a settled state**:
  each names a module whose production code still trips Miri and cites the finding
  that owns it (F12, F13). Deleting a line is part of fixing the thing it names; **no
  skip may be added without a finding**. Byte-exactness does not imply soundness —
  341/341 identical sweeps ran on top of F1's stack overflow for the port's whole
  life, and the widened gate found seven more defects in one afternoon.
- **S16 — read the ratchet's shape, not its sign.** `raw_ptr` counts
  *occurrences* — signatures, casts, **and pointer types written in prose**, so
  documenting a contract inflates it. A shim keeping its raw signature keeps its
  count; a shared helper may legitimately add one or two, and paying that beats N
  copies of the span arithmetic. `SHIM(` rises only in a family's commit B;
  `unsafe_block` may rise by that commit's shim count plus its shared helpers;
  `no_mangle` non-increasing; `unsafe_fn` ~flat until raw bodies are *deleted* rather
  than strangled, and then it falls hard. **Never fold shims into a macro** — it makes
  `unsafe_fn` report a drop that did not happen and hides the `SHIM(` markers. Commit
  B regenerates the baseline, with the reason in the commit message. **A metric can
  have a prose floor**: `transmute` reads 4 with zero calls behind it (all doc
  comments, since T4b.3b) — when a metric's remaining count is prose, say so
  wherever the metric is quoted, or the next phase chases a number that cannot move.
  **Read per-file deltas, never the total** (Phase 5 session G): in a total, a
  one-line prose mention cancels against a real deletion elsewhere and both go
  unexamined — the ratchet's own regeneration caught one prose delta only because
  the per-file table was read; `mem_zeroed` is S21's live construction-audit count
  and has drifted by prose twice.
- **S17 — an instrument may skip only loudly, naming what goes unmeasured.**
  The incident: `FFMPEG` was unset for two whole sessions, `c_vs_rust_bench`
  silently skipped, and T5's +16.8% encoder regression reached a commit before
  anything noticed. **The specific check is now automated** — `gates.sh` falls back
  to ffmpeg on PATH and, when absent, prints an UNMEASURED banner instead of
  skipping silently — so this rule's live content is the general law, applied to
  every new gate step and instrument.

#### Phase exits

- **S18 — the straggler sweep.** Before closing a phase, list every definition the
  phase's per-family passes were supposed to reach and subtract the ones carrying a
  conversion marker; group the remainder by file. Each hit is **converted**,
  **deleted-dead**, or **listed in the log with its owner phase** — no fourth option.
  Phase 2's sweep found `encoder/deblocking.rs` running a raw duplicate of a family
  the phase had already converted, on the encoder's mainline path, for four sessions.
  **Corollary, applied at conversion time rather than discovered at phase exit:
  "converted" is a per-consumer claim.** When a common module serves both codecs,
  commit B grep-verifies *every* consumer's installer and every local duplicate
  (this port duplicates freely — F2's four writers, T6's four `SDeblockingFunc`s);
  a re-export picked up by one codec is not a family conversion. The claim goes in
  the commit message with the greps that prove it.
- **S19 — update §0 and hand off one brief ahead.** Every phase exit refreshes the
  status preamble (§0) and writes the *next* phase's brief in `prompts/`, then stamps
  its own brief superseded-historical. One brief ahead, not two: the phase after next
  gets written at the next exit, when its inputs are real.
- **S20 — the decomposable unit of a struct-field conversion is the
  signature-reachability closure, not one struct** (Phase 3 session E, T3.4:
  planned faces "adopt the writer functions" and "flip the embedding struct's
  field" could not land separately — a function taking `(buf, &mut BsWriter)`
  *forces* the type of every field that feeds it, by-value embedding propagates
  the layout change transitively, and `const` ABI asserts leave no intermediate
  green state). Before sizing commits in any struct conversion — **Phases 5 and 6
  are made of these** — compute the closure: every struct reachable from the
  changed signature through fields, embeddings, and layout asserts lands together;
  what separates cleanly is pure subtraction (deleting the dead type) and pure
  addition (the proven new API). A session brief that draws a commit boundary
  through the middle of a closure is wrong before it starts, and the correction
  protocol applies to it. **A closure's consumer inventory classifies by use, and
  "lifecycle" is not one class** (T5.E2): the free path is a consumer of whatever
  sizes it — `sMb`'s "129 of 130 accesses are lifecycle" hid that the free's
  `numMb` reads the *allocation's* dimensions where the layer carries the current
  slice's, and following the closure verbatim would have freed with the wrong
  size. Name what the lifecycle arithmetic reads.
- **S21 — the construction audit (Phase 3 session D).** Any struct gaining
  an owned field gets its construction paths audited *in the same commit*:
  `mem::zeroed` construction — 26 sites at Phase 0's count, the encoder and
  decoder god-contexts included — makes field-type changes UB at a distance, and
  the incident that proved it was on the live decoder-creation path with every
  test green. All-integer state (`BsWriter`-shaped) is valid at all-zero and the
  audit discharges by stating so; owned pointers (`Vec`, `Box`) are not, and the
  pattern is the decoder's `MaybeUninit` shell + heap-constructing `new_boxed()`
  with a comment naming the phase that deletes the shell. A duplicate-family
  finding, relatedly, must inventory **every function of the family per copy**,
  not one representative — F2's table compared `BsWriteBits` alone and missed
  `nal_encap.rs`'s divergent `BsFlush` store width for three phases.
- **S22 — a repaired instrument has a backlog, and it surfaces only at the level
  and moment the repaired gate first runs** (Phase 3 exit, F17 → F18). F18 sat red
  for four phases because three conditions stacked: the test predates F17's fix,
  F17 meant the Miri gate could not fail at all, and only the once-per-phase
  `exit` battery level runs Miri over the integration tests — so the first
  `exit`-level run after the repair was the first moment the defect *could*
  surface, and it did, immediately. Whoever repairs a gate must, in the same
  session, deliberately run everything the gate covers at every level it covers,
  rather than waiting for the next scheduled run: "the gate starts existing on
  this commit" (F17's line) implies everything it watches is unaudited until it
  has actually run there. **An instrument's scope list is part of the instrument**
  (Phase 5 session A, F22): `find_stub_bodies.py` reported "none found" for three
  phases because its directory list omitted `src/decoder`, and
  `find_dup_types.sh` read the encoder only until 4b session C — before trusting
  a tool's silence, read what it actually scans. **And an exemption in one
  instrument propagates to every instrument that copied its scope** (Phase 5
  session O, F38): `src/api/` is exempt from `deny(unsafe_code)` by design, and
  that exemption had silently become exemption from the census, the dup scan, and
  every inventory the phase ran — F38 lived there the whole time. A module exempt
  from a *lint* must be listed explicitly in every *other* instrument's scope, or
  it leaves all of them.
- **S23 — a cached table becomes a derived value only if the source cannot
  change behind the cache, and that is a property of the update paths, not of
  the dispatch** (Phase 4b session A, T4b.1/T4b.1b). De-virtualizing a
  configuration table invites replacing "the selector installed at init" with
  "read the parameter at the call site". Those are the same value only while
  nothing writes the parameter without re-running the installer — so the check
  is a read of **every** update path, per field, not a look at the table. Two
  fields of one struct, set by one init function, gave opposite answers in one
  session: `iEntropyCodingModeFlag` cannot change without a full re-init
  (`WelsEncoderParamAdjust`'s no-reset arm does not copy it), so deriving is
  safe; `iRCMode` *is* copied there and the table is *not* re-pointed, so the
  installed callbacks legitimately lag the configured mode, and deriving would
  have silently repaired a live behaviour — S6's territory, reached from an
  unexpected direction. When the answer is "they can diverge", store the
  installed value and **name the field for it** (`eInstalledMode`), so the lag
  is documented rather than accidental.
- **S23b — an alternation that returns 0/0 has not run yet.** S14 says to
  alternate both trees in one loop; it does not say what to alternate. Running
  the *hitting configuration in isolation* gave HEAD 0/40 and control 0/40 for
  a race whose sweeps had just produced it twice — because the race needs the
  load of a full sweep. Re-run at the level the hits occur (whole presets, 341
  configurations back to back) it gave HEAD 4/12 vs control 5/12 and, for the
  first time, F3's actual rate. **A no-reproduction result on both sides is a
  statement about the harness, not about the trees.**
- **S24 — a count that decides a conversion's shape comes from a grep over the
  definitions, at the moment of converting** (Phase 4b session B). That session's
  brief carried two scouted counts and **both were wrong**, each taken from a comment
  rather than from the code. Its §1 said `IWelsParametersetStrategy` had one ported
  implementor — it has two, and the second is `FillDefault`'s value, so converting on
  the brief's plan would have deleted three live overrides and mis-encoded every
  default-configured stream. Its §3 said `decode_slice.rs`'s transmutes were data
  puns — all 19 are fn-pointer type erasure, a different problem with a different
  fix — and said `transmute` was 23 calls, of which **two are prose**. Byte gates
  would have caught the first (the ids are stream-visible) and nothing would have
  caught the second, but in both cases the *design* would have been decided wrong
  before the first line was written.
  **The rule**: implementor counts, family sizes, call-site counts, and metric values
  that determine *what shape* a conversion takes are re-derived by `grep` in the
  session that acts on them. A brief, a hand-off, a module doc and a prior session's
  scouting are all leads, never evidence. This is S23 pointed at prose instead of at
  cached configuration, and it comes from the same failure mode: reading a summary of
  a fact instead of the fact. **And the grep's unit must match the code's** (F29):
  a line-anchored grep counted 25 of 29 sites because the four that mattered were
  formatted across three lines each — a count from a grep of the code is *still a
  summary* when the code's unit is the expression. Match multiline, normalize
  whitespace first, or confirm the count with a second instrument; the miss cost an
  8-minute Miri round trip that had already named two of the four. **A claim about
  what an instrument does is settled by reading the instrument's source** (session
  M): two of that session's three wrong premises were not stale counts but
  summaries of tool behaviour — the census key's removal semantics, the SHIM
  metric's fall prediction — and both were settled in minutes by reading the tool
  rather than the summary. **And a count carries the directory it was taken in**
  (Phase 5 session P): that session's brief sized one face with a tree-wide
  `pRefPic` figure and a decoder-only `pRefList` figure in the same sentence, and
  said the scope of neither — together they placed 228 sites of `src/encoder/`,
  which F12/P10 excludes, inside a decoder step. This is the harder variant to
  catch: each number is *correct* for some domain, so nothing looks stale, and only
  the sum is absurd. Write the directory beside the number, and re-derive both.
  **And a count carries the *unit* it was taken in** (Phase 5 session P′) — token,
  field access, receiver, or definition. That session's brief sized a five-field
  conversion at 279 `pRefPic`-and-friends tokens where the tree held 142 field
  accesses, because `pRefPic` **names two types**: `SPicture`'s reference-list field
  (6 accesses) and a `*mut SRefPic` local in `manage_dec_ref.rs` (106). This is the
  harder variant again: the directory clause above is caught by asking *where*, and
  this one is caught only by asking *what the token names* — a token count of an
  identifier that names two types is not a size, it is a coincidence. The same
  session's other miss is the cheap version of it: a pointer-anchored grep for
  `(*layer).pDec` read 134 where the tree held 160, because 25 of the sites spell the
  receiver as a reference. **Match the grep's unit to the code's unit — the receiver,
  not just the field — and say which unit the number is in.**
- **S25 — converting a raw pointer to a borrow does not introduce an aliasing
  question, it surfaces one that was already there** (Phase 4b session B, T4b.2a —
  the log entry's §4). `WelsWriteParameterSets` held a `*mut` to the strategy across
  calls that re-enter the same object through `pCtx->pFuncList`; `InitDqLayers` did
  the same. As raw pointers the aliasing was invisible; as `&mut` it is UB, and the
  conversion could not compile until it was faced. The fix shape: re-acquire through
  one helper at each use, and no borrow outlives one expression. **Phases 5 and 6
  are made of raw-to-borrow conversions**, so the re-entrancy audit — *who else can
  reach this object while I hold it?* — is part of each conversion's planned work,
  enumerated with the S20 closure, not a surprise discovered at compile time.
- **S26 — a brief is an execution document** (2026-08-11, Eugene's direction).
  Contents: facts (file:line, counts, commands), order, gates, non-goals. Cite
  standing rules by tag; never retell their history. No provenance narration, no
  meta-commentary about the brief itself, no rhetoric. If a sentence does not
  change what the session does, delete it.
- **S27 — a docs-only tail does not buy a full battery** (2026-08-11, Eugene's
  direction; predicate clarified 2026-08-12 after session E named the gap). At
  session open, when **all** hold — the inherited tail touches only `rust/docs/`,
  the previous session ended **accepted** (`OVERALL: PASS`, or `FAIL` where every
  failing step was adjudicated to a standing finding under its protocol and
  acquitted in that session's log entry — session D's FAIL(1)-all-F3 qualifies;
  any unadjudicated failure does not), and `rust/tools/` and the toolchain are
  unchanged — open with the cheap subset: build both profiles,
  tests, ratchet, census. Run the S2 null at the first moment a perf verdict needs
  it; the first code face's battery covers the rest. Any condition failing → full
  battery at open (a changed instrument also owes S22's backlog run). If the first
  face's battery then fails, re-run at the parent commit first, to split
  environment drift from the face.
- **S28 — a pointer derived from a safe container carries the provenance of the
  deriving expression, not of the allocation** (Phase 5 session C, T5.C3).
  `plane.as_mut_slice()[origin..].as_mut_ptr()` is safe code with the correct
  address and UB at the first read into the padding behind it — the slice index
  narrowed provenance to `[origin..]`, and the padding is what the accessor exists
  to reach. Nine gates passed on it; only Miri failed, and only because one test
  read backward through the pointer. The rule: **shim accessors derive from the
  allocation root** — `as_mut_ptr().wrapping_add(origin)`, never through a
  narrowing slice — with the reason in the accessor's doc comment, and **every such
  accessor gets a Miri test that reads the pointer's full legal reach in both
  directions**. No byte-level gate can see this class; the test is the instrument.
- **S29 — inside a raw-reached struct, derive field pointers with `addr_of_mut!`,
  never through an intermediate `&mut`; an escaping borrow is the worst class**
  (Phase 5 session E, F24/F25). `addr_of_mut!` creates no reference, so the derived
  pointer carries the parent allocation's provenance and "whose retag is on top"
  stops being a question — F24's 141 use sites cost four changed lines because the
  sites already spelled the dereference. Classify by escape: a nested borrow hurts
  one function; a borrow **stored into another struct** (`= &mut (*p).field`
  shapes — grep-able) hurts everything downstream. **The decoder's `&mut *pCtx`
  inventory is closed** (T5.G1: 11 bindings and 13 nested borrows; `src/decoder/`
  now holds zero, and no function in the port takes `&mut SWelsDecoderContext`) —
  what 5.2–5.6 still convert per file touched is the *wider* class F28 named: any
  `&mut` of anything reachable from `pCtx`, held across a call. This is
  S25's complement: S25 finds the overlaps, S29 is the spelling that removes them.
  **`&mut X as *mut T` is the same defect with the cast already written** — the
  reference exists and retags before the cast discards it; `addr_of_mut!` is the
  fix, and the shape hides from a grep for `&mut *`. **The spelling's boundary**
  (session O, T5.O8): `addr_of_mut!` rescues derivations through a *raw parent* —
  a write through an **owned local** pops borrows regardless of spelling, and only
  *ordering* fixes that. Reaching for the spelling where the invalidator is the
  local itself is the misfire to avoid.
- **S30 — a brief freezes when its session starts** (2026-08-12, from session N's
  collision). Mid-session changes of direction reach the session from its operator,
  in the session, with the decision recorded in §7.4/§0 — never as an edit to the
  brief file on disk. A brief that changes under a running session is **data, not
  direction**: quote it in the log, leave it uncommitted, keep executing the brief
  as read at open, and raise it in the report — which is exactly what session N
  did, and it was correct both times (the steward's edit was real, and refusing it
  was still right, because the file carried no provenance the session could check).
  Retro-editing a *closed* session's brief is the same violation aimed backwards:
  it falsifies the record of what was asked.

- **S31 — context is a working buffer, not the session's memory** (Phase 5
  session Q, Eugene's direction 2026-08-14). Session Q closed "on context" at
  ~300K of a 1M window with four scoped faces untouched — the stop reason was
  uncheckable, so compliance substituted for delivery. The rule: session state
  lives **on disk** — the brief, the checklist, the settlements, the log draft —
  and the harness compacts and continues; after each compaction, re-read the
  brief and the open face's settlement, re-grep the counts (S24), and keep
  working. A session with faces remaining may close only for (a) a blocker only
  Eugene or the steward can clear, named in the hand-off, or (b) genuine
  exhaustion, which is **not before the second compaction**. Anything earlier is
  an **early exit** and the log labels it as one. What makes this safe is already
  standing practice: every seam committed (compaction can lose only unrecorded
  reasoning), probe per seam, and the **per-face breadcrumb** — one log line
  appended as each face closes — so a stop at any point costs nothing to make
  honest.

- **S32 — the Miri gate's cost is decoder instantiations, not decoded
  macroblocks** (Phase 5 session S; the measured record is
  `phase5_findings.md`'s S32 entry). `gates.sh exit` minus Miri is 103 seconds;
  each probe pays for a multi-MiB `Initialize` under the interpreter, flat
  against decoded work (36 macroblocks 268.9s, 16 macroblocks 196.0s). So
  shortening a probe's stream saves ~nothing and removing a probe saves a
  quarter of the wall: **probe count is the budget knob, probe coverage is the
  value** — T5.S3 retired one probe on exactly this arithmetic, and any future
  "make Miri faster" proposal starts from it.

  **Re-measured and amended at Phase 6 session A** (2026-08-18), per step, machine
  idle, against a **0.8%** null band taken from two identical `--lib` runs:
  * **The decoder numbers hold, with one refinement.** `--lib` **835s**, of which
    the three probes are **665s** and the other 334 tests **164s**; the invocation
    floor is 5s and the *compile* share is **2s** — cargo-miri emits MIR, so a Miri
    step is essentially all interpretation. But `fmo` (one frame) costs **186s**
    against grid's 249s and ec's 248s (~30 frames each), so the instantiation floor
    is ~186s and decoding runs **~2s a frame**: shortening a stream saves about a
    **quarter**, not nothing. The knob is still probe count.
  * **S32 transfers to the encoder on picture size, measured.** Encoder
    initialisation under Miri reads **30.79 / 30.85 / 30.85 / 31.19s** at 48x32,
    96x64, 192x128 and 384x256 — **flat to 1.3% across 64x the area**, against a
    0.13% null band. Those are Miri's own `finished in` figures, which are its
    **virtual** clock rather than host time: two identical `--lib` runs read
    1349.57s *both times, to the hundredth*, where their wall clocks differed 0.8%.
    It is the better instrument for comparing configurations and the wrong one for
    quoting a budget in seconds — convert before doing that. The cost is the instantiation there too, so a probe's geometry
    is free and is chosen for coverage (F34) rather than for budget.
  * ~~**The frame-count axis is owed, not answered.**~~ **Measured at Phase 6
    session B**, once the encode probe went live: the same 48x32 geometry at
    **2 / 3 / 6 frames reads 71.29 / 80.53 / 102.28 s** (a second 3-frame run
    77.90 s), against session A's initialisation floor of ≈ 31 s virtual (≈ 19 s
    of host time by session A's wall/virtual ratio, 835 / 1349.57). **Scaling,
    not flat: ≈ 7.7 s per frame** — the encoder's per-frame cost under Miri is about
    four times the decoder's ≈ 2 s, which is motion estimation and mode decision
    being interpreted. So on the encoder the two axes differ: picture size is free
    (above) and frame count is not, and the probe stays at **3 frames**, F34's floor
    (two frames, the second inter-coded, a moving source) plus one. **And the unit
    changed under it**: the `--lib` step now runs with `-Zmiri-disable-isolation`
    for the encode probe's `WelsTime()`, and with isolation off Miri's clock *is*
    the host's — `finished in` stops being the deterministic virtual figure and
    becomes wall time (the two 3-frame runs differ by 3.3%, where session A's two
    isolated `--lib` runs agreed to the hundredth). Every figure in this bullet is
    a host second; the virtual unit is no longer available for that step, and a
    budget quoted for it is quoted in wall time from here on.
  * **Retirement is not always available.** Session A measured each decoder probe's
    reach by whole-library source coverage and **all three reach code no other
    reaches** (142 / 73 / 9 unique functions), so no probe could be retired to make
    room. A budget argument that assumes a retirement must measure the reach first.

- **S33 — a recorded outcome is a measurement, never a prediction** (Phase 5
  session S's own corrections, all caught and reverted in-session). Three
  instances in one session: a new test pinned the *port's* hash instead of the
  C++'s to stay green; two asset tests were claimed red-under-revert before the
  revert was run (they weren't); a hash was written for a revert outcome never
  measured. The rule: a gate outcome, a golden value, or a red-under-revert
  claim is written only after the command ran, from its output; an expected
  value is labeled a prediction and never occupies a result's slot. Corollary,
  from the same session's two instrument bugs (the missing `FlushFrame` drain;
  awk reading `0x0` as hex): *an instrument that disagrees with a hand count is
  wrong until proven otherwise* — the hand count arbitrates.

- **S34 — an alias converted to an index is faithful only while the container never
  reorders** (Phase 5b session B, F55 — the measured record is `phase5_findings.md`'s
  F55 entry). The C stores a pointer *into* a node and reorders the **pointer array**
  around it, so the alias survives; the port's owned slots reorder by moving the slots,
  and the index the alias became is exactly what moves. The access unit's two rotations
  left `slice_hdr_nal` naming the successor access unit's NAL, and the api judged every
  B-slice picture by the next picture's slice header — 11 of 60 conformance assets, and
  no line of the diff was near the swap. **Before converting a stored alias to an index,
  grep the container for `swap`, `rotate`, `retain`, `remove`, `sort` and `drain`**: each
  hit is a site the index spelling has to be told about and the pointer spelling did not.
  Carry the index through the permutation, or give the node an identity that is not its
  position. Corollary for the gates, from the same finding: **when frame counts hold and
  hashes move, compare the hash *sets* before calling it a pixel divergence** — session A
  spent a face on "corruption" that was a reordering, and one set comparison separates
  them.

- **S35 — a C `const void*` is not a `*const`: type the parameter from the body,
  not the prototype** (Phase 6 session B, T6.B8/T6.B9). `InitPic` was typed
  `*const SSourcePicture` because `encoder_context.cpp` writes `const void* kpSrc` —
  and then casts the `const` away and writes eight fields through it. The lie is
  legal in C++ and Undefined Behaviour in Rust: the unit test passed a shared
  reference and Miri failed the first write. The `exit` battery caught it, one commit
  after the instrument that could see it was widened. Rule: when a `void*` (or any
  erased pointer) is retyped, the mutability comes from what the callee *does* —
  read the body — and a function that writes says `*mut`. The same reading applies
  to every `c_void` still enumerated for Phases 7 and 8.

- **S36 — a layout edit re-pins its size assertion in the same commit; an
  instrument an edit invalidates is re-aimed, never deleted** (Phase 6 session D).
  T6.D7 dropped `assert_size!(SDqLayer, …)` rather than re-pin it, and the layer
  went two commits with its size documented in a comment and checked by nothing —
  while the same session re-pinned six other sizes correctly. The failing assertion
  is the instrument *working*; the response is to measure the new size and pin it,
  and a diff that deletes an assertion, a probe, a ratchet entry, or a gate step is
  reviewed as a finding, not as cleanup. (The one lawful deletion is S18's: the
  *guarded thing* is gone.)

- **S37 — an arena cannot take a `&mut`: type the parameter from the aliasing
  structure, not from the ownership** (Phase 6 session E, T6.E6). `SMbCache` has
  one owner, and the retype to `&mut SMbCache` was still wrong 62 times: its
  consumers hold raw cursors into *different regions* of it **across calls**
  (`pCurRS` into `sCoeffLevel`, `pBestPred` into `sMemPredMb`, `pBlock` into
  `sDct`), and a reference covering all 5600 bytes invalidates every one of them,
  including the caller's — `WelsEncRecUV` takes the arena *and* a cursor into it
  in one call, so no argument ordering can work. The test is behavioral, not
  structural: *do consumers hold live derived pointers into disjoint parts across
  calls?* If yes, it is an arena, and the sound moves are deleting the second
  path (E deleted fourteen cache params), split-borrow views, or kernels that
  take slices — never a whole-struct reference. The byte gates read 353/353 over
  a tree carrying 58 of these; **only Miri sees the class**. The rule governs
  what follows by name: `SPicture`'s plane cursors (session F) and `sWelsEncCtx`
  itself (session G — which is *why* the flip is accessors first, view struct
  last). **Session H's corollary, measured**: the aliasing decides the *owned
  shape* too, not the allocation pattern — one C++ block becomes one `Vec` with
  byte offsets where two consumers can name one region (`SStrideTables`), and
  several independent containers where no region is ever aliased (`SWelsSvcRc`).

- **S38 — a reborrow at a coercion site is a retag no lint sees** (Phase 6
  session E). `&mut *pCurMb` passed where `*mut SMB` is expected compiles
  silently — the reference is created, coerced straight back to a pointer, and
  the provenance is narrowed to that one spelling — and neither rustc nor
  `dangerous_implicit_autorefs` flags it. Pass the pointer variable itself and
  let the compiler demand a reference only where the parameter's type is one.
  When a conversion partially reverts, strip *every* reborrow the attempt
  introduced and re-derive the survivors from the compiler's own type errors —
  close the class, not the instances.

- **S42 — parameter soundness lives at call sites, not in signatures** (Phase 6
  session J, F66). A `&mut T` parameter retags the *whole object* at function
  entry — Miri's own range, `[0x0..size]` — where a raw write pops only the
  offsets it touches. So a conversion that passes every callee-side audit
  (session I's Q1/Q2) still explodes any **caller** holding a `T`-derived
  cursor across the call: the encoder context had **93 such sites over 28
  callers** (the worst single function held 68), four more hid inside a
  strategy object where no signature census could see them, and Miri's probes
  reached **2 of the 93** — coverage, not defect rate; rustc reports none at
  any opt level. The inventory therefore enumerates *callers*, and the
  instrument is F66's call-site detector, not a signature grep. Corollary:
  leaf-type parameters convert freely, but an arena-root parameter (S37)
  converts **last**, after every cursor family into it has retired — 109
  green conversions were reverted whole for this rule.

- **S43 — a concurrency design has three premises: disjointness, ordering,
  and the `Send`/`Sync` of what crosses — and the third is a type property,
  compiled rather than argued** (Phase 7 session A, F67). The fork/join's two
  scheduling premises verified cleanly; the design still could not be built,
  because `shared` must cross the spawn and `sWelsEncCtx` is `!Sync` for 12
  distinct reasons the compiler enumerated in one reverted probe
  (`assert sWelsEncCtx: Sync`, read the `E0277`s). Cost the third premise the
  same way **before scheduling the fork**: the probe is three lines, and its
  error list is simultaneously the work inventory — the 105 items Phase 7 was
  handed to retire turned out to be, item for item, the same object.

- **S39 — a pre-authorized revert is aimed by attribution, not by suspicion**
  (Phase 6 session F). The brief marked the plane step separable and
  solo-revertable; the span then breached at +4.67% and the per-commit
  attribution read LTR +0.58%, **the id flip +1.45%**, the planes **+0.24%** —
  the authorized revert would have fixed nothing. When a span breaches, bisect
  the window commit-by-commit *before* spending any contingency: the revert
  target is the measured contributor, and a remedy that is not a revert may beat
  the one that was planned (T6.F5's once-a-frame stamping took the breach to
  +2.72% with the handles still the truth).

- **S40 — a root accessor must be retag-stable: S28 answers provenance, not
  aliasing** (Phase 6 session F, F63). `plane.as_mut_slice().as_mut_ptr()`
  derives from the allocation root and is still wrong: `&mut self.buf` is a
  `Unique` retag over the whole allocation, so each call pops the pointer the
  previous call handed out — and the encoder asks the same picture twice a
  frame. An accessor that hands out cursors reads the address out of the
  container's header with no reference formed (`PaddedPlane::root_ptr` is the
  model), so repeated calls are sibling `SharedReadWrite` derivations. The two
  spellings are indistinguishable at the call site and to every byte gate
  (369/369 both ways); the covering Miri test calls the accessor **twice and
  uses the first cursor after the second call**. Any accessor that survives on
  "no caller re-derives yet" — the decoder's `data_ptr` today — inherits that
  test rather than the assumption.

- **S41 — a field-wise constructor is value-equal to a memset image, never
  byte-equal, and the difference is the bytes the type does not define** (Phase 6
  session G, F64). Miri rejected the byte-comparison proof four times, each
  correction still too narrow, and the fourth round forced the rule to be about
  `Option`, not about niches: **a niche moves the discriminant, it does not
  write the payload** — `None` writes only its tag, so an `Option` field's
  payload bytes are undefined whether or not the discriminant borrowed a niche.
  `repr(C)` padding is the same class. The instrument that works is two-tier:
  byte-compare the fields whose bytes are fully defined, value-compare the rest,
  and **report the padding count** so the uncovered bytes are a number, not an
  assumption (G's context: 63 fields byte-compared at 96103/97888, 7 by value,
  53 padding bytes reported). **Session H's clause: the comparison shell must be
  `MaybeUninit` bytes, never a typed zeroed value** — the moment the type's
  first `Vec` field landed, `mem::zeroed::<sWelsEncCtx>()` *inside the
  instrument itself* became UB (a memset `Vec` is a null `Unique`) and kept
  compiling and passing; owned fields form a third `OWNED` tier that reports
  its own shrunk reach.

---

## 8. Sequencing rationale & parallelism

```
P0 ──► P1 ──► P2 (kernels) ──► P4 (dispatch) ──► P5 (decoder pivot) ──► P8 (API) ──► P9 ──► P10
         └──► P3 (bitstream) ──┘                └► P6 (encoder pivot) ─► P7 (threads) ┘
```

- **Common-first** because both codecs consume the same vocabulary types and kernels; **decoder pivot before encoder pivot** because the decoder has the strongest external ground truth (JVT streams + h264dec triangulation), no threading, and is 30% smaller — it's where the arena/grid/borrow-split patterns get proven before the bigger encoder applies them.
- **Phase 4a before Phase 3 (D-seq-1, 2026-08-10):** the graph above always allowed P4 directly after P2; with the Phase 2 ledger carrying decode +17% / encoder +11% of shim scaffolding and the parked families waiting on the dispatch checkpoint, 4a's recovery-and-unpark pass runs first so Phase 3's new shim families land on a recovered baseline. 4b (the entropy/RC enums that touch what Phase 3 rewrites) stays after Phase 3.
- **Threading after encoder ownership** (the split-borrow shape is unknowable before `sWelsEncCtx` is decomposed) and **the API boundary last** — the boundary can only shrink to a thin translation shim once the safe core it translates *to* exists, and until then the C API (and the tests driving it) stays byte-for-byte still.
- Parallelizable across concurrent sessions/agents: within Phase 2 (kernel families are independent), Phase 3's read vs write sides, Phase 5 vs early Phase 6 steps (6.1/6.2 don't depend on decoder work — only on Phases 1–4), and any R1 sweep. The pivots (5.5/5.6, 6.6) and Phase 7 are single-track.

---

## 9. Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| More latent UB like the release-deblocking segfault surfaces mid-refactor (an optimizer or layout change flips behavior that was never defined) | **High** (raised: the known instance turned out to be a stack overflow executing thousands of times per frame on the mainline path, silently, for the port's whole life) | High | Phase 0 root-caused the known instance (§1.4, already fixed by `e6fce464`) and added the dual-profile gate; any later debug/release disagreement halts the phase until root-caused — it is always a pre-existing bug being exposed, and fixing it is in scope. Note the instance was *invisible to every gate*: 341/341 byte-identical sweeps ran on top of it. Byte-exactness does not imply soundness, which is the argument for the fuzzer (§7.3) and for Phase 2 replacing `*mut u8` kernel arguments with real array types |
| Silent behavior change on malformed streams (EC paths, slop P6) | Medium | High (conformance can't see it) | Error-code parity tests, fuzz differential vs C++, malformed-stream corpus from the ignored e2e cases |
| Perf: scaffolding deficit on call-dense families (T4's ~7% is the instance; intra worst-case measured a wash, so kernel-intrinsic risk is largely retired) | Medium | Medium | §7.4 two-ledger rules + ≤10% hard ceiling; Phase 4 recovery checkpoint; Phase 5 must clear the ledger; unswap-commit-B as last resort |
| Estimates derived from the surveys run low (Phase 2's kernel count was −60%; per-family brief figures wrong in both directions) | High | Low–Medium | Count before sizing every session; phases 5/6 are file/deref-based (more robust) but treat all session counts as −0/+40% ranges |
| `decode_slice.rs`/`encoder_ext.rs` pivots stall mid-way | Medium | High | Strict internal step order, shims keep tree green, each step is a session-sized unit |
| Borrow-splits force design churn late (a subsystem needs 4 parts at once) | Medium | Medium | §2.2.6 decomposition reviewed against the top-20 fattest call paths *before* Phase 5 starts (one session of dry-run signature sketching) |
| MT output nondeterminism after Phase 7 | Low | High | Slice-order stitching preserved, determinism gate across thread counts, diffharness threads sweep |
| Losing the C++ debugging column | Low | High | P14 conventions, gates at every merge keep divergence windows one-PR wide |
| Boundary translation subtly changes C-visible semantics (output-pointer validity windows, option `void*` decoding, callback reentrancy) | Medium | High | Tests never leave the C API; external-ABI harness drives the dylib exactly as a C consumer; thunk-by-thunk `# Safety` contracts reviewed against upstream `codec_api.h` docs |
| ABI layout drift on platforms we don't build (Windows `EXTAPI`, 32-bit) | Low | Medium | Inherited from upstream's no-virtual-dtor + `__cdecl` design; `api/abi_guard.rs` pins struct layouts; add per-platform CI only when a platform is actually claimed |

---

## 10. Open decisions (need Eugene's call, none block Phases 0–4)

- **D1 — Handle hygiene: RESOLVED (2026-08-07, by implementing the recommendation).** Plain indices with release semantics identical to C recycling, plus a `#[cfg(debug_assertions)]` generation counter per slot, bumped by `Pool::replace` and checked in every accessor. Handle equality ignores the generation in both profiles, so identity semantics (P3) cannot differ between debug and release. Built and tested in Phase 1 (`src/safe/pool.rs`); no full generational arena.
- **D2 — C ABI future: RESOLVED (2026-08-07): drop-in replacement is a requirement.** The C-ABI surface (vtables, 7 exports, `repr(C)` structs, slot order) is frozen as public contract; `src/api/` is the sole `unsafe` module; crate ships as cdylib/staticlib; §2.2.8 and Phase 8 encode the consequences. Follow-on packaging (soname, version script, pkg-config) is tracked outside this plan.
- **D3 — Decoder MT:** the C++ has a threaded decoder the port stubbed out. This plan deletes the stubs. If decoder MT is wanted later, it's a fresh design on the safe architecture (frame-level pipelining with `PicPool` + per-frame `MbGrid` is well-suited) — flagging so the deletion isn't a surprise.
- **D4 — Workspace split:** single crate (status quo, recommended through Phase 9) vs splitting afterwards into `openh264-core` (pure safe codec, `#![forbid(unsafe_code)]` — the strongest possible statement) + `openh264-capi` (the cdylib boundary, the only unsafe). With D2 resolved, the split earns more: it turns "deny with one allow" into a hard `forbid`, and gives Rust consumers a dependency with zero unsafe anywhere in its tree. Still deferred to post-Phase-9 — module boundaries make the split mechanical then.
- **D5 — The great rename:** after Phase 9, keep Wels/Hungarian names (maximum C++ diffability, zero churn) vs rename to idiomatic snake_case with `/// C++:` mapping headers (better for external contributors, one-time loss of easy diffing). Can be decided per-module; recommendation: decide only once the C++ reference has stopped earning its keep.
- **D6 — Error style:** keep i32 error-code returns internally (maximum C++ parity, recommended through Phase 8) vs migrate internals to `Result<_, DecodeError>` during Phase 9 (the public API gets `Result` at Phase 8 regardless, translating at the boundary).

---

*Sources: three subsystem surveys (decoder, encoder, common/api/processing) conducted 2026-08-07 against `eb463dbd`, with file:line references verified at survey time; per-file difficulty tiers and counts come from those surveys. The taxonomy table (§1.2) is the index back into the details.*

---

## Progress

Per-phase checklist, updated at the end of every session. The running narrative —
gate numbers, what was tried, what to do next — is in
[`safety_refactor_log.md`](safety_refactor_log.md); findings that are recorded rather
than fixed are in [`phase0_findings.md`](phase0_findings.md).

### Phase 0 — guardrails, dead-code purge, tooling

- [x] **T1 — session-start control run.** Green at `353791f7`: `cargo test` and
      `cargo test --release` 294 passed / 0 failed / 20 ignored; `sweep.sh st mt def`
      341/341 byte-identical; both benches all-rows bit-identical.
- [x] **T2 — resolve the dangling working tree.** The `sweep.sh` modification had
      already been committed as `353791f7` before this session started. The plan
      update found in its place was committed as inherited (`87127430`), and the
      session brief recorded in-tree (`156b9abb`).
- [x] **T3 — the `eb463dbd` release segfault.** Reproduced, root-caused, closed. It was
      a stack buffer overflow (16-byte `uiBS`, 32-byte writes), not a pointer-wiring
      gap; already fixed at HEAD by `e6fce464`, so no fix commit was needed. Recorded
      in §1.4, `phase0_findings.md` §F1, and auto-memory. `86b40ed1`
- [x] **T4 — recover docs, record baselines.** `ac5b91d2` (handoff, status log and four
      audit scripts, each marked recovered-and-unverified), `89c05aa2`
      (`perf_baseline.md`, 3-run medians, machine info, and the measured proof that the
      encoder bench measures frame-skipping rather than encoding without `FFMPEG` set).
- [x] **T5a — 62 unreferenced SIMD externs** in `mc.rs`. `89aa6109`
- [x] **T5b — SIMD delegating stubs.** 113 definitions across four files, plus the
      CPU-flag branches that installed them, machine-verified to delegate to exactly the
      `_c` already in each slot. `a2a19d3c`, `fbe45f11`
- [x] **T5c — decoder threading scaffolding.** Done in the Phase 2 session A, in four
      commits, dead duplicates first and entry points last: `3e09a814`, `e3ca3e17`,
      `5fe41f17`, `01ad9ba1`.
      `GetThreadCount` is a literal **`0`**, not `1`, with the reason
      (`api/codec_api.rs:1831` branches on `<= 0`) now in its docstring rather than only
      in the log. `Picture::pNzc` and `Picture::pReadyEvent` went with it, and
      `GetPNzc`'s picture-vs-layer fork collapsed to the layer source because the
      picture side was statically unreachable. Totals: `unsafe_fn` 1372 → 1365,
      `raw_ptr` 5390 → 5352, `unsafe_block` 511 → 507.
- [x] **T5d — duplicated bitstream writer.** No deletion: there are four copies, not
      two, and they are not identical. Recorded as `phase0_findings.md` §F2 per the stop
      rule; Phase 3.2 owns the dedupe. `dc8487cb`
- [x] **T5e — small stragglers.** `4e174285`. The dead `use std::ffi::c_void` in
      `lib.rs` was the whole yield: `RUSTFLAGS="--force-warn dead_code" cargo build`
      turned up nothing belonging to the T5a–T5c families, only pre-existing unrelated
      dead code that the phase's own rule keeps out of scope.
- [x] **T6 — ratchet script + gate runner.** `99f4ab5c`, in the Phase 2 session A.
      Both instruments and their invocations are documented in §7.1/§7.5; the counting
      traps that motivated the note are encoded in the script.
- [ ] **T7 — fuzz crate.** Not started, and **deferred by direction** during the Phase 1
      session (2026-08-07): "skip fuzzing, do phase 1". `cargo-fuzz` is still not
      installed; nightly now is (Phase 1 needed it for Miri). Nothing else depends on
      it, but §7.2 gate 6 and §7.3 stay unavailable until it lands.
- [x] **T8 — bookkeeping.** This appendix, `safety_refactor_log.md`, and the auto-memory
      updates. Re-run at the end of each Phase 0 session.

Tooling that landed alongside: `da5c06ae` made the diffharness able to build and run a
**release** driver at all (`RUST_ENC_PROFILE`), which is what makes §7.2 gate 0
executable; `f1c90948` fixed a bash 3.2 regression in it.

### Phase 1 — safe vocabulary types

**Started ahead of Phase 0's exit gate, by direction** (2026-08-07). Phase 0's T5c,
T5e and T6 are still open and T7 is deferred; the session was told to skip fuzzing and
proceed to Phase 1. That is sound for *this* phase specifically — Phase 1 changes no
codec code, adds no `unsafe`, and its own gate is the full battery, which was run as a
control first and again at the end — but the missing ratchet script (T6) means Phase 1
could not run `unsafe_ratchet.sh check`. Substituted: the raw counts below, and the
structural fact that every file in `src/safe/` is `#![forbid(unsafe_code)]`.

- [x] **T1 — control battery.** `cargo test` and `cargo test --release`
      294 / 0 / 20 each; `sweep.sh st mt def` 341/341 debug, 340/341 release with the
      single failure at `t=4 sm=3 n=600` — F3's exact signature. The exit run behaved
      the same way, so the rate was measured against the control commit directly (8 mt
      sweeps each, 960 configurations: 1 failure at `53e211f7`, 2 at HEAD — noise, same
      signature). F3's write-up updated with both refinements.
- [x] **T2 — module skeleton, error plumbing, test PRNG.** `448c8118`. Note for the
      record: the plan said the error codes live in `decoder_context.rs`; they do not —
      that file has only `ERR_NONE`, and the reader codes are declared *twice*, in
      `decoder/bit_stream.rs` and `decoder/dec_golomb.rs`. `err.rs` reuses them and
      carries a test pinning the two copies together.
- [x] **T3 — `PaddedPlane` + plane cursors.** `952c8b1d`. §2.2.1 updated to the built
      API.
- [x] **T4 + T5 — `BsCursor` and `BsWriter`.** `5b47b1fe`. §2.2.2 updated; two
      deviations and two findings (F4, F5) recorded.
- [x] **T6 — `Pool`/handles and the `MbGrid` skeleton.** `e3a09459`, `066471ea`.
      §2.2.3 updated (`mut_and_rest` replaces `cur_and_refs`, generic over `T`);
      D1 resolved by implementing the recommendation.
- [x] **T7 — error plumbing.** Landed with T2 as `ErrInfo`; per D6 it stays a
      transparent newtype over the C++ `int32_t` codes.
- [x] **T8 — Miri.** `cargo +nightly miri test --lib safe::` clean, 63/63. Both
      differential files clean under Miri as well — including the *old* unsafe
      implementations they drive, which is free UB coverage of `dec_golomb`,
      `bit_stream` and `vlc_encoder`. No test needed `#[cfg_attr(miri, ignore)]`.
      Sample counts scale down under `cfg!(miri)` so the gate stays minutes, not hours.
- [x] **T9 — bookkeeping.** This appendix, `phase1_findings.md`, the log entry, and the
      §2.2/§10 updates above.

Counts, for the ratchet baseline whenever Phase 0 T6 lands: `src/safe/` is 2,517
lines over 7 files — 1,681 of implementation and documentation, the rest in-module
tests — and contains **zero** `unsafe` of any spelling (the only three occurrences of
the word are in `mod.rs`'s prose). 63 in-module unit tests. The
differential tests add 18 more in two integration files that *do* use `unsafe` (they
must — they drive the raw-pointer reference implementations). Test totals moved 294 → **375** in debug and **373** in release; the two-test
difference is deliberate — `pool`'s stale-handle tests are `#[cfg(debug_assertions)]`,
because the behaviour they check only exists in a debug build. Every *pre-existing* test
binary keeps its exact control count, and the 20-test `#[ignore]` set is unchanged.

### Phase 2 — leaf DSP kernels, both codecs

Sized at 5–7 sessions, resized to 6–8 on 2026-08-08, briefly 8–9, and **7–8 as of
D-perf-4** (four spent through session D; the recovery session is cancelled; remaining:
T6 alone, T7 across two, T9). Original basis: (real total ≈190 fns, not ~120;
T7 alone is 69 — bigger than T3 and T4 combined). Session A landed the preconditions,
the control and the pilot; session B landed T4 and the budget investigation.
Findings from this phase are in [`phase2_findings.md`](phase2_findings.md).

- [x] **Phase 0 T6 — ratchet script + gate runner.** `99f4ab5c`. Both instruments
      built and documented in §7.1/§7.5. Two counting traps encoded: the pattern is
      `unsafe (extern "C" )?fn`, and definitions are counted line-anchored so the ~210
      `Option<unsafe extern "C" fn(..)>` table entries don't inflate it. Baseline at
      `956a8c07`: `unsafe_fn` **1372** (not §1.1's naive 922), `unsafe_block` 507,
      `raw_ptr` 5390, `transmute` 23, `unsafe_impl` 11, `mem_zeroed` 26, `no_mangle` 42,
      `SHIM(` 0. `check` verified to fail on an increase.
- [x] **T1 — control run.** Green at `afcdd785` on the full `exit` battery:
      `cargo test` 375/0/20 and 373/0/20; `sweep.sh st mt def` 341/341 in **both**
      profiles (no F3 hit this session); both benches bit-identical; all three Miri
      invocations clean. Perf control in [`perf_baseline.md`](perf_baseline.md)
      §Phase 2.
- [x] **T2 — PILOT: `decoder/decode_mb_aux.rs`.** `ba13bdbd` (safe kernels +
      differential proof), `ee7818fb` (swap + shims, equivalence entries deleted).
      **Four kernels, not the ~13 the brief expected** — the C++ file has more, the
      Rust port has `IdctResAddPred_c`, `IdctResAddPred8x8_c`, `IdctFourResAddPred_c`
      and `GetI4LumaIChromaAddrTable` and nothing else (314 LOC, not 374).
- [x] **Pilot retro.** Verdicts in §2.2.1 (cursor beats triple; the loop order, not the
      API, is what has to change; `reborrow` added; shim contracts are short only for
      forward-reaching kernels) and in §Phase 2 (what the ratchet does and does not
      move during a strangler phase). Numbers in `perf_baseline.md` §Phase 2 T2:
      **1.3–1.8% faster**, not slower.
- [x] **Phase 0 T5c/T5e — decoder threading scaffolding + stragglers.** `3e09a814`,
      `e3ca3e17`, `5fe41f17`, `01ad9ba1`, `4e174285`. Taken after the pilot rather than
      before it (the session log argues the ordering); Phase 0's checklist above now
      records the outcome. **Phase 0's only remaining open item is T7, the fuzz crate,
      deferred by direction** — so §7.2 gate 6 stays unavailable and `gates.sh` prints a
      permanent SKIP for it. F8 is the first recorded case of a finding a fuzzer would
      plausibly have reached first.
- [x] **T3 — `decoder/get_intra_predictor.rs`** — the designated perf worst case,
      **42** kernels (not 44): 14 I4x4, 14 I8x8, 7 chroma, 7 I16x16. `b7f48311`
      (safe kernels + differential proof), `a4828187` (swap + shims). Verdicts:
      **the worst case is a wash** — every decode-bench row inside ±1%, which is the
      per-side spread (`perf_baseline.md` §Phase 2 T3), so §9's "perf regression in
      MD/ME/intra hot loops" risk can be downgraded. Bounds checks land **per row**,
      not per sample, and `copy_from_slice`/`fill` on fixed-size windows keep the wide
      stores the punned `u32`/`u64` accesses gave. All ~140 punned accesses in the file
      turned out to be pure byte *moves*, so `u32::from_ne_bytes` — T7's nominated
      replacement — was not needed anywhere in it. These are also the first shims whose
      span is not derivable from the signature: they anchor at `pPred - stride - 1` and
      their contract names `PADDING_LENGTH` as the reason the `-1` row and column exist.
- [x] **T4 — `common/mc.rs`**, motion compensation. **26 kernels** (24 safe bodies,
      two of them shared by the `McHorizLuma_c`/`McVertLuma_c` aliases) plus two filter
      helpers; `InitMcFunc` left alone as dispatch plumbing. `46053993` (safe kernels +
      differential proof), `ea52b387` (swap + 28 shims), `e26164e6` (the rejected row
      walker). Firsts for the phase: **two planes** — a `PlaneCursor` read + a
      `PlaneCursorMut` write, since reference and current are different allocations —
      and the first **encoder-side consumers** (`pfLumaHalfpelHor/Ver/Cen` at
      `iWidth + 1` up to 17). The module-internal `pWelsMcFunc_c` quarter-pel table is
      folded into `mc_luma`'s `match` and deleted, per §3.2's single exception.
      **This family is over budget: +8.2% / +7.2% / +7.0%** against §7.4's 5% —
      investigated at length, written up in `perf_baseline.md` §Phase 2 T4, and the
      residual is per-call overhead in the *shim* layer, which Phase 5 deletes. Three
      structural fixes were tried and rejected by paired measurement (by-value cursors,
      shim-to-shim dispatch, and a `rows()`/`rows_mut()` walker added to the Phase 1
      plane API and then reverted). Three real defects were found and fixed: a runtime
      `copy_from_slice` lowering to a `memmove` **call** (10.8x on the zero-MV copy),
      per-sample bounds checks across seven same-length slices, and missing
      `#[inline(always)]`/`#[inline(never)]` parity with the C++.
- [x] **D-perf-1 — the T4 budget call (2026-08-08).** Carried as a scaffolding
      deficit, not reverted; §7.4 restated with the end-state/scaffolding split, the
      interleaved-pairs measurement protocol, the deficit ledger
      (`perf_baseline.md` §Ledger), the ≤10% hard ceiling, and the Phase 4/Phase 5
      recovery checkpoints. Further T4 optimization attempts are **closed** — three
      structural fixes were built, paired-measured, and rejected; do not reopen
      without new evidence of a different mechanism.
- [x] **T5-intra — `common/intra_pred_common.rs`.** `56a3dbf9` (A, with the SAD kernels)
      and `209d3c66` (B). Two 16x16 luma predictors behind shims, **+0.57% median** on
      the encoder bench. Per-kernel reference shapes rather than a shared one — V reads
      the row above, H reads the column left — and both write a *packed* `[u8; 256]`,
      which is what distinguishes them from the same-named 2-arg decoder cousins.
- [~] **T5-sad — `common/sad_common.rs`.** Safe kernels written and differentially proven
      (`56a3dbf9`); swapped in `209d3c66` and **unswapped in `11f82d41`**. The swap cost
      the encoder **+16.8% median, +78% worst**, breaching §7.4's 10% ceiling. The raw
      kernels run; the safe ones are dead but proven and their spans are pinned.
      Re-landing needs a **per-row** overhead fix: the corrected (L1-resident)
      microbenchmark shows cost independent of block width — 4x8, 8x8 and 16x8 all
      ~7.0 ns through the safe kernel — so the per-sample arithmetic is already free and
      only the 16-wide shapes amortise the per-row bounds and iterator work. Rolling the
      row offset and `u8::abs_diff` were both measured and changed nothing.
- [x] **`PlaneCursor::row_windows`.** `840a48dc`. One bounds check per block instead of
      two per row, for kernels whose stride and buffer length are runtime values and
      whose checks therefore cannot fold. Does **not** repeal T4's `chunks` verdict —
      `mc.rs` keeps `row()`, where the widths are const and the checks do fold — and the
      rule is written on the method.
- [x] **D-perf-2 — the T5-sad sequencing call (2026-08-09), and its verdict
      (2026-08-09).** The one bounded attempt was spent. The candidate was T8's
      exact-span shape, which had just taken that family from 2.51x to 1.02x and was
      not in T5's rejected set; on SAD it measured **1.39-2.97x L1-resident even in
      the framing most favourable to it**. **T5-sad stays parked and T7's SAD/SATD go
      prove-and-park from the start.** The box is closed. One thing found on the way
      that a later reopening should start from: `PlaneCursor::new` costs enough at
      this block size that paying it twice per call moved 4x4 from 1.61x to 4.93x, so
      handing these kernels slices-and-offsets instead of cursors is a live idea —
      and a convention change, hence not this phase's. `perf_baseline.md` §Parked.
- [x] **D-perf-3 — the encoder ceiling breach (decided 2026-08-09; superseded by
      D-perf-4 before execution).** T4's encoder side measured +7.6% median / +16.7%
      worst; cumulative with T8 ≈ +11% against the then-live 10% ceiling. The decided
      recovery (mc.rs direct-dispatch forward, unswap fallback) was **cancelled as a
      dedicated session** when D-perf-4 retired the ceiling: the breach no longer
      blocks, the deficit stays ledgered, and the dispatch-forward experiment returns
      to **Phase 4 as its first task**. The rule it produced survives: an
      encoder-side family's budget is not its decoder-side budget; commit B measures
      both benches (now as one interleaved pair each).
- [x] **D-perf-4 — the regime change (Eugene, 2026-08-09).** Safety first; §7.4 v3:
      no hard ceiling, +25%-cumulative tripwire parks a family instead of stopping a
      phase; swap-and-ledger is the default; no optimization boxes; lightweight
      commit-B measurement; recovery consolidated at Phase 4 / Phase 5–6 / a Phase 9
      perf pass with the end-state target unchanged. **T6 is unblocked and is the
      next action.**
- [x] **T6 — `common/deblocking_common.rs` + F1's `uiBS` surgery + expansion
      (`bbb9348e`, `64633e5f`, `d878916b`, `2fe283e4`, `9c32f890`).** All three items
      landed in one session under D-perf-4. Deblocking: 7 safe kernels (6 edge
      filters porting the `(iStrideX, iStrideY)` V/H trick as one `(step_x, step_y)`
      body each, + `nonzero_count`), the 12 ABI wrappers *are* the shims, 6 raw inner
      kernels deleted, all 14 `no_mangle` exports gone; decode +7.8/+2.6/+2.3%,
      encoder flat (not a consumer), ledgered — cumulative decode ≈ +17/+10/+10%,
      under the tripwire. The F1 surgery: `uiBS` is `[[[u8; 4]; 4]; 2]` through the
      `PDeblockingBSCalc` typedef and every signature, the five 32-byte
      `from_raw_parts_mut` sites and the 32-byte read deleted; byte-exact, encoder
      noise-level. Expansion: one pad-parameterised `expand_picture` in
      `common/expand_pic.rs`; the two `_c` shims reconstruct the full allocation
      from the mid-pointer (`expand_shim_span`, pads 32/16) — no call site needed
      the explicit-pad fallback; perf inside the noise floor. Findings: **F10**
      (the parked raw SAD kernels' trailing pointer bump is UB on exactly-sized
      buffers — test accommodated, T7's prove-and-park tests inherit the rule).
      The differential span probes gained a **golden direct-run comparison** after
      a +1-anchor mutation survived the touch-set assertion — span size, touch set
      and anchor are now pinned independently.
- [~] **T7 — the four encoder kernel files. Part 1 complete (session F,
      2026-08-10); part 2 (`encoder/get_intra_predictor.rs`) remains.**
      **F-1 `encoder/encode_mb_aux.rs`**: 21 kernels + installer; `f233d506` (safe
      kernels + differential proof, three mutations killed), `b1fb7448` (swap + 21
      shims). Fixed-array signatures throughout — the two strided Hadamard reads
      carry exact-reach types `[i16; 49]` and `[i16; 241]`. Encoder +2.1% median /
      +9.9% worst usable (T4's per-call mechanism on flat content), decode flat;
      cumulative encoder ≈ +13%, under the tripwire → ledgered.
      **F-2 `encoder/decode_mb_aux.rs`**: **9 leaf kernels by recount** (the brief
      said 6 — five raw bodies live in `svc_encode_mb.rs`; the OnMb composite and
      installer are dispatch plumbing); `fc92dab0` (A), `55e6d7fe` (B, 9 shims).
      Noise-level on both benches. Finding **F11**: `WelsIHadamard4x4Dc`'s plain
      `i16` adds panic in debug above ±2047 where the C++ wraps — third member of
      F8's class, reproduced not repaired.
      **F-3 `encoder/sample.rs` SATD**: proven-and-parked directly per D-perf-2,
      `1383acb5` + `20e84e47` — 7 safe kernels, nothing installs them, raw bodies
      live, re-attempt at Phase 4a with T5-sad. F10 got a **second instance**: the
      composites' sub-block bumps reach past whole-row buffers, so raw-side
      differential buffers are `(h+1)*stride` now (Miri caught the under-sizing
      mid-session). One F3 hit with a **new signature shape** (Rust output *longer*)
      was attributed by the alternating-loop protocol (24/24 clean on the exact
      config; A 0/360 vs B 1/360 preset configs) and appended to F3 as its sixth
      measurement — the signature's output clause is now "any wrong length".
- [x] **T8 — `processing/{vaacalc,adaptive_quantization}.rs` (`d41244c2`,
      `af98f6ab`).** **6 kernels** as recounted (vaacalc 5 + `SampleVariance16x16_c`),
      not the continuation brief's 11. No dispatch table and no `no_mangle` anywhere
      in the family, so all six shims are plain `unsafe fn`. Bodies at 0.69-1.04x
      against the raw ones except `VAACalcSadBgd_c` at **1.44x**, which is recorded
      with its mechanism rather than fixed; end-to-end +3.48% median on the encoder,
      inside the ceiling. Two microbenchmark errors and their fixes are in
      `perf_baseline.md` §Phase 2 T8 — the important one is that **after a swap the
      "raw" side of a microbenchmark is the shim**, so bodies must be measured before
      swapping or recovered with `git show`. Findings: **F9** (`iFrameSad` overflows
      above 32 896 macroblocks) plus two false claims corrected in the converted
      files' own headers.
- [x] **A new instrument step: the null run.** Before attributing an encoder-bench
      reading to noise, run the **same binary in both slots** through the identical
      harness. Measured here at median +0.00% / max +1.81% / zero rows over 5%, which
      turned a suspected artefact into a real +3.48%. Also re-condemned Spatial Ramps
      (-38% between two runs of one binary). Cheap, decisive, and it belongs in front
      of every "that's just noise" conclusion from here on.
- [x] **T7 part 2 — `encoder/get_intra_predictor.rs`** (`6e0009d3` A, `cbc18d94` B).
      **26 kernels + the installer** as the brief's recount said; the two I16x16 modes
      it installs but does not define come from `common/intra_pred_common.rs` (T5).
      Per-kernel reference shapes: a top-only predictor takes `&[u8; N]` and *cannot*
      read the left column, which is why mode decision may offer it when the left
      neighbour is missing. **R-d's exhaustive-selector rule landed on the availability
      offset**, and `reach_table_agrees_with_the_availability_tables` now asserts, over
      all 16 I4x4 offsets and all 8 of the others, that every offered mode reads only
      neighbours that offset declares — turning the shims' central contract sentence
      into a checked property. It also pinned that `DDL_TOP`/`VL_TOP` are installed but
      unreachable, and that the I16x16/chroma tables have no top-left bit while their
      plane mode reads the corner. Encoder +0.42%, inside the null floor.
- [x] **T9 — phase exit. Phase 2 is complete** (`a6361ee3`, `19b1391f`, and this
      appendix). All seven exit steps ran, in order, and two of them found work:
      * **Straggler sweep** — `encoder/deblocking.rs` was carrying live raw copies of
        the deblocking family T6 had converted in `common/deblocking_common.rs`, with
        its own installer, on the encoder's mainline path. Converted as **G-2**
        (`b3da21e1` A, `ee8735df` B): proven equivalent first, then 13 raw bodies
        deleted and 8 wrappers re-exported. The other 50 raw kernel-shaped definitions
        are listed with their owner phases in the session-G log entry.
      * **Counts** — `SHIM(phase2)` **154** across 13 files; `no_mangle` **24**, of
        which zero come from any file Phase 2 converted (5 `api/` exports, 2 in
        `wels_encoder_ext.rs`, 14 in the parked `sad_common.rs`, 3 CABAC init).
      * **Miri widened** from `--lib safe::` (63 tests) to the whole library minus four
        finding-cited skips (**267 tests, 81s**). It found seven defects immediately:
        four test-side (fixed, including **F10's third instance**) and three in
        production (**F12**, **F13** — recorded, not repaired).
      * **Final perf** — three interleaved pairs per bench: decoder
        **+16.6 / +10.6 / +9.9%**, encoder **+14.73% median**, against ledger
        predictions of ≈ +17/+10/+10% and ≈ +14%. **The ledger is validated as an
        instrument**, which is what D-perf-4 was betting on.
      * **Fuzz-absence tally** presented for Eugene with its composition analysed:
        Miri owns five of the eight, and the case for T7 now rests on **F3** and the
        F8/F11 arithmetic class — the input-space instrument that is still missing.
      * **Consolidation deliverables** — §0 (status preamble), §7.6 (standing rules
        S1–S19), `prompts/archive/phase4a.md`. Both Phase 2 briefs stamped
        superseded-historical. No renumbering; phase numbers stay permanent
        identifiers and execution order is owned by this appendix and §8.

### Phase 4a — dispatch de-virtualization + the recovery checkpoint

**Complete** (2026-08-10, session A: `32dc05c5`…`25a8e287` + close-out; full record
in the log's Phase 4a entry).

- [x] **`SMcFunc` de-virtualized** — assert-maps first (`32dc05c5`), then 15 sites
      direct (`c9d416de`); `pMCFunc` parameter deleted.
- [x] **F13's fourth site fixed** — `pfMdCost`/`pfMeCost` → `CostFamily` enum, the
      `svc_mode_decision` Miri skip **deleted** (`ededdee8`), which immediately
      exposed **F14** (`sad_common`'s fourth latent-UB finding — the
      parked-raw-code thesis is now a measurement, not a prediction).
- [x] **Decoder `SDeblockingFunc` → direct calls** (12 slots, 22 sites, `3a67cd8e`).
- [x] **The checkpoint ran and split the ledger** (`perf_baseline.md` §Phase 4a):
      encoder recovered to ≈ **+8.9% cumulative** — under 10% for the first time
      since T4 — while **decode did not move**. The finding Phases 5/6 inherit:
      *direct dispatch recovers per-call scaffolding only where the caller supplies
      constant dimensions*; `BaseMC`'s runtime sizes and ~1300 instructions defeat
      inlining, so the decode rows are **downgraded to Phase 5** (D-perf-3's
      fallback, spent on the decode half only). Spatial Ramps halved, as predicted.
- [x] **Parked families' second verdict** (`25a8e287`, rebuilt harness): SAD and
      SATD stay parked; next re-attempt is caller conversion (Phases 5/6).
- [x] Scope cut on evidence, not clock: decoder intra-pred arrays, `sBlockFunc`,
      expand, the `decode_slice.rs` cache-fill transmutes, and the encoder's ~55
      CPU-dispatch members are **not** de-virtualized — `transmute` still 23, the
      phase's named unfinished business, owned by 4b/5.

### Phase 3 — bitstream layer

**COMPLETE**, 2026-08-11, sessions A–F. Brief:
[`prompts/archive/phase3.md`](prompts/archive/phase3.md) (superseded-historical); seam numbering
T3.0–T3.6 is that brief's. **The layer contains no pointer cursor on either side, and
no raw output allocation the encoder owns alone.**

- [x] **T3.0 — the malformed-stream error-code parity test.** 2316 golden rows over 11
      base streams plus a degenerate set, generated in-test, byte-identical in both
      profiles (36 s debug / 2.4 s release). Runs the corpus in a child process because a
      decoder panic crosses an `extern "C"` thunk and *aborts*; the parent names the
      entry that died. **Found [`phase3_findings.md`](phase3_findings.md) F15 on its
      first run** — `nalu.rs:762`/`:675` index one byte before the payload when a NAL
      strips to its header byte, debug aborting where release does out-of-bounds pointer
      arithmetic. Those entries are `WITHHELD` rows until T3.3 fixes the sites.
- [x] **T3.1a — the read side's bodies become `BsCursor`**, behind unchanged raw
      signatures. **P6 resolved** (F4): not zeroed guard bytes and not `get()` fallbacks
      but a declared `READER_SLOP = 3` over the real allocation — byte-identical by
      construction. **F7 fixed**, both sites, with both differential accommodations
      deleted in the same commit and the comparisons widened to their whole ranges.
      Decode +0.45% (3 pairs), from shim field-marshalling that T3.1b deletes.
- [x] **T3.1b — DONE**, and with it **T3.1 closes**. Step 0 (`1bf5a235`): `BsCursor`
      gains the CAVLC mode per §2.2.2 **[P3]** — `cavlc_bit_pos` + `start_cavlc`/
      `end_cavlc`, parity-exact including the −16 half-window, plus the debug-only
      coherence guard and a hand-written `PartialEq` over the six C-mirrored fields;
      differential-proven against the raw pair, three mutations killed, before any
      consumer moved. The ownership move: `ctx.sBs` and `SVclNal::sSliceBitsRead` become
      `BsReader` (§2.2.2 [P3]'s recorded deviation from a bare `BsCursor`), 20 consumer
      functions move from `pBs` to `(buf, &mut cursor)`, `SDqLayer::pBitStringAux`
      becomes `*mut BsReader`, and T3.1a's marshalling layer
      (`cursor_of`/`store_cursor`/`read_with`/`BsCursor::from_parts`) is **deleted**.
      The reader family is now **safe fns** — no `unsafe` left in `dec_golomb.rs`'s
      entry points. `ExpandBsBuffer`'s three pointer rebases collapse to one (P5's
      prediction, early). Found and fixed **F16** (`READER_SLOP` is not the readable
      extent — `BsEndCavlc`'s prime is unbounded on truncated input); the reader half of
      `safe_bits_differential.rs` is retired per §2, with a frozen transliteration of
      the CAVLC pair kept as the parity reference.
- [x] **T3.2 — the CABAC engine is a detached position** (`00c6cf9f`, session C — the
      session that first fixed F17 and proved the gate red, `eae61b94`, so this is the
      first seam judged by a battery that can fail). The pointer triple is deleted;
      `{uiRange, uiOffset, iBitsLeft, pos}` over a per-call RBSP window
      (`BsReader::rbsp_window` — see §2.2.2 [P3] for the two-extents result of the
      step-0 read-extent audit, F16's lesson as procedure). Handoff is one `usize` each
      way with the round-trip pinning test; the end ladder is comparison-form and
      proven to stop at the RBSP. S1 executed in the brief's order: disassemble →
      convert → disassemble caught the first shape's lost inline + three bounds-check
      paths before any bench ran; final shape matches the raw reference point for
      point. Pair: decode +0.19/+0.76/+0.27 against a ±2% floor (flat-to-win, no
      ledger row); encoder wash. Miri 288/0; goldens 2316 unmoved; one release-sweep
      F3 hit re-run 5/5 clean (tenth measurement, the fixed gate's first stop).
- [x] **T3.3 — the ownership seam, and with it the decoder read side is *fully*
      safe-owned** (session D, 2026-08-11: `345308cb`, `1db28c4f`, `8b611076`). Four
      faces. **(1)** `SDataBuffer{pHead,pEnd,pStartPos,pCurPos}` → `RawDataBuffer
      { buf: Vec<u8>, cur }` — one offset, not the sketch's three, because a stored
      `end` is the allocation size and `pStartPos` was write-only in this port. The
      rule the seam is judged by, stated on the type: **every readable extent is
      derived from the owning buffer at call time; nothing stores a length the buffer
      can outgrow.** **(2)** T3.1b's `BsReader` bridge is **deleted** — `base`/`avail`/
      `buf()`/`split()`/`readable_from` and the last `from_raw_parts` in the read path
      — and `ExpandBsBuffer` with it (growth is `Vec` growth; the rebases were pointer
      maintenance offsets make meaningless), closing **F16's second instance** by
      construction: nothing is left that can go stale. P5's grow-mid-AU test is in
      tree. **(3)** `nalu.rs` payload identity is an offset; `DecodeNalHeaderExt` takes
      a slice (it had no length parameter at all); `DetectStartCodePrefix`, a duplicate
      `SVclNal`/`SNalData` trio, a duplicate `DecodeNalHeaderExt`, `pNalPos` and two
      dead parameter pairs are deleted-dead per S18. **(4) F15 is FIXED** — one
      comparison-form helper, an empty RBSP has bit size 0 and takes the existing error
      path, both profiles now agree — and `withheld()` is deleted, the goldens
      regenerated: **105 `WITHHELD` rows → 105 recorded outcomes, 12 files, nothing
      else moved** (classified mechanically). Ratchet `raw_ptr` **5106 → 5076**, the
      phase's biggest drop, and `SHIM(` **158 → 155**, its first fall. Pair: decode
      −0.43/+0.36/+0.32 against a ±0.6% floor; encoder median +0.00%. No ledger row.
- [x] **T3.4 — the encoder write side, and `SBitStringAux` dies** (session E,
      2026-08-11: `13912ffd`, `5bd19deb`, `fb4e7c29`). Face 1 collapsed **F2's four
      writer copies** onto the canonical family (fifteen functions and four dead
      transliterations deleted, zero bytes of output moved) and found F2's own
      inventory one divergence short — `nal_encap.rs` also carried a `BsFlush` storing
      only the bytes it advanced over where the canonical always stores a full 32-bit
      word; every gate was blind to it because the next write overwrites those bytes.
      Faces 2+3 could not separate and landed as one commit — **S20 was hoisted from
      exactly this** — taking the encoder onto `BsWriter`, deleting `InitBits`
      (**F13 site 3 closed**: it declared `*const u8`, stored `*mut u8`, and wrote
      through it, so every honest caller was UB), deleting `au_set.rs`'s two Miri
      accommodations, and **closing F5** by deletion exactly where its "who fixes it"
      line predicted. Then `SBitStringAux` — §1.2's emblem for T3 — is deleted, with
      the inventory that licensed it. Ratchet `unsafe_fn` 1327 → 1296.
- [x] **T3.5 — the CABAC coder's own triple** (session F, 2026-08-11: `f828a55c`,
      `45ab079c`). **Step 0's write-extent audit found two engines, not one**:
      `svc_set_mb_syn_cabac.rs` carried a second transliteration of nine core
      functions and five tables, and because a module-local item beats a `use`, the
      syntax layer ran the local copy while the slice-tail flush ran the canonical one
      — two engines, one `SCabacCtx`, split at flush time. Upstream has no such
      duplication; face 1 deleted it, with all nine divergences enumerated per copy
      and `BypassOne`'s mask-vs-branch form (which differs for `uiBin ∉ {0,1}`) checked
      at all six call sites rather than assumed. Face 2 converted
      `m_pBufStart`/`m_pBufCur`/`m_pBufEnd` to three `usize` offsets. **The audit's
      load-bearing finding: `m_pBufEnd` was assigned and read by nothing, here or
      upstream** — writes are bounded *below* by `m_iBufStart` in `PropagateCarry`'s
      comparison (which is also what stops `pos - 1` wrapping a `usize`) and bounded
      *above* only by the slice indexing this seam introduced. `PropagateCarry` is a
      safe `fn` now. `BsWriter::set_pos` is deleted for `BsWriter::at`, making the
      empty-accumulator invariant structural instead of asserted. 4b's fence held for
      `pfGetBsPosition` and `pfWelsSpatialWriteMbSyn` and **broke for the stash pair**,
      whose byte copy genuinely needs the buffer.
- [x] **T3.6 — the frame output owns its buffers** (session F: `32b73efb`).
      `SWelsEncoderOutput.{pBsBuffer, sNalList, pNalLen}` → `Vec<u8>` /
      `Vec<SWelsNalRaw>` / `Vec<i32>`, with `uiSize` and `iCountNals` **deleted rather
      than converted** per T3.3's standard. Four `WelsMallocz` calls → one `new_boxed`
      constructor; **four `WelsFree` cascade entries → one `drop`, the first encoder
      cascade entries to fall (R4 in miniature)**; `FrameBsRealloc`'s
      allocate-copy-free pair → two `resize` calls. **S20's closure was small because
      `pOut` is a pointer**, so nothing embeds the struct by value; **S21 discharged on
      the same fact** — the encoder context is `mem::zeroed()`, and zero is a valid
      null pointer where it would not be a valid `Vec`, so no `MaybeUninit` shell was
      needed. Scoped deliberately: `SWelsSliceBs.pBsBuffer` is **excluded** because
      `SetOneSliceBsBufferUnderMultithread` aliases it to `pThreadBsBuffer[idx]`
      (verified, not survey-vintage), and `SWelsNalRaw.pRawData` stays raw because that
      struct is shared with the excluded list. Both reasons written at their sites,
      naming Phase 6.
- [x] **Phase 3 exit** (session F: `e3fc68e8` + the exit commits). S18 straggler sweep:
      the era's names are **zero in code** — the last 112 `BitStringAux` identifiers
      were vestigial parameter names on `*mut BsWriter` and are renamed; one field,
      the decoder's `SDqLayer::pBitStringAux`, is listed with its owner (Phase 5).
      `SHIM(phase3)` ends at **2, both enumerated and both owned by Phase 6**. Full
      3-pair-then-5-pair medians both benches; **S19**: [`prompts/archive/phase4b.md`](prompts/archive/phase4b.md)
      written. The first `exit`-level Miri run since F17's repair caught **F18** (a
      Phase 4a address-comparison test, red since birth behind the broken gate; test
      defect, `#[cfg_attr(miri, ignore)]` with the write-up at the site) — the lesson
      is hoisted as **S22**. The exit's doc set and F18's fix landed as `81e3cf9a`.

### Phase 4b — configuration dispatch, the strategy vtables, 4a's leftovers

Brief: [`prompts/archive/phase4b.md`](prompts/archive/phase4b.md), live. Per **D-seq-1**
(Phase 3 → 4b → 5 → 6 → 7 → 8 → 9). The 4a/4b fence was **lifted** at Phase 3's exit —
the config-dispatch tables name the bitstream writer, and the writer is one family with
a stable signature.

- [x] **T4b.1 — entropy-coder dispatch** *(session A, `08b7c29d`)*. Four `Option<fn>`
      slots → `enum EntropyCoder`, four `#[inline]` methods that `match` at the call
      site. `WelsSpatialWriteMbSynCabacThunk`, four typedefs, four `is_some()` asserts
      and three `extern "C"`s deleted; **the CAVLC arm sheds the `buf` parameter T3.5
      was forced to add**, which is the win an enum buys over a trait object.
      `assert_size!(SWelsFuncPtrList)` 1272 → 1248.
- [x] **T4b.1b — rate-control dispatch** *(session A, `3e583b9a`)*. `SWelsRcFunc`'s nine
      slots → one `eInstalledMode: RCMode` and nine methods; nine typedefs deleted,
      nineteen call sites de-guarded. Assert 1248 → **1184**. The seam produced **S23**:
      the installed mode is deliberately *not* read live, because
      `WelsEncoderParamAdjust`'s no-reset arm changes `iRCMode` without re-pointing the
      table, so deriving would have silently repaired a live lag.
- [x] **T4b.2a — the parameter-set strategy** *(session B, `d6c78c1b`)*. The 20-entry
      `IWelsParametersetStrategyVtbl`, **two** static instances and 25 thunks → one
      `ParasetIdKind` inside a merged `CWelsParametersetIdStrategyObj`; the field is
      `Option<Box<_>>`. **Session A's scouted "one ported implementor" was wrong** —
      `INCREASING_ID` is ported too and is `FillDefault`'s value (**F20**). Found and
      fixed **F19**, a leak of the strategy object on every teardown that C++ does not
      have. Assert unmoved at 1184: `Option<Box<_>>` is pointer-sized.
- [x] **T4b.2b — the reference strategy** *(session B, `be67a754`)*. Three static
      vtables and 13 thunks → `RefStrategyKind`, with `pCtx` a parameter instead of a
      stored back-pointer; `Init`, `Destroy`, both factories and a **free-cascade
      entry** deleted rather than converted. `sWelsEncCtx`'s assert does **not** move —
      `#[repr(C)]` padding absorbs it exactly, which also spares four `assert_ctx_offset!`
      pins that sit after the field.
- [x] **T4b.3a — the intra-pred constraint family** *(session B, `33b1f0f3`)*. Three
      `SWelsDecoderContext` slots whose typedefs declared `*mut c_void`/`extern "C"`
      where the stored functions had concrete types → one `IntraPredConstraint`.
      **`transmute` 23 → 5**, its first movement since Phase 0; the crate now holds
      **two** transmute calls. Closes §3 items 1 and 2 together — they were one seam.
- [x] **T4b.3b — the expand family, and the crate's last two `transmute` calls**
      *(session C, `d1c1a7d4`)*. The two calls turned out to reinterpret a type into
      **itself**: all four of the port's `PExpandPictureFunc` typedefs are the same
      type. `SExpandPicFunc`, `InitExpandPictureFunc`, the four typedefs, the forwarding
      wrapper and members of **both** `SWelsFuncPtrList` and `SWelsDecoderContext`
      deleted — an S18 cross-codec conversion, since the struct lives in `common/`.
      **`assert_size!(SWelsFuncPtrList)` 1184 → 1160**, its first movement since Phase
      4a's entry. **`transmute` 5 → 4, and the crate now contains zero calls.**
      Found **F21** on the way: one C++ function translated three times, two copies
      drifted in the sub-16-chroma case.
- [x] **T4b.3c — `sBlockFunc`** *(session C, `f2e3c5af`)*. Three members, of which the
      port and the C++ both read exactly one; the two block-zeroing slots are called
      from nowhere in either tree. The struct was **declared twice** and bridged with
      `as *mut _ as *mut _` — a double cast doing a `transmute`'s job **without
      appearing in the metric**, which is the finding worth carrying to Phase 5. The
      decoder's third `WelsNonZeroCount_c` went with it; its single reader now calls the
      `common` shim.
- [~] **T4b.3 item 5 — the CPU-dispatch filler** *(not started, deliberately)*. The
      brief made it non-displacing and the exit had priority. `SWelsFuncPtrList` still
      has **62 `pub pf` members**, roughly 55 of them selecting between identical `_c`
      functions (an estimate from the 4a survey, never re-measured — S24 applies). The
      one named straggler with a live slot is **`pfSetNZCZero`**, the last member of the
      NZC family T4b.3c closed decoder-side; it is encoder-side and therefore 6.5's, and
      the Phase 5 brief flags it as a six-line opportunity.
- [x] **Phase 4b exit** *(session C)* — whole and uncompressed per the brief's §4: S18
      straggler sweep (no internal dispatch vtable survives; the remaining `Vtbl` names
      are the public C ABI), the full 3-pair perf protocol over the phase (entry
      `6e15c907`), bookkeeping, and **S19: [`prompts/phase5.md`](prompts/phase5.md)**.

**Phase 4b is complete.**

### Phase 5 — decoder structural rewrite (the first pivot)

Brief: [`prompts/phase5.md`](prompts/phase5.md), live. Findings:
[`phase5_findings.md`](phase5_findings.md).

- [x] **T5.A1 — the dead shadow declarations** *(session A, `126af95c`)*. Nine
      declarations with zero readers, of which `manage_dec_ref.rs` held an island of
      nine names referencing only each other — its `SPps` had **two** fields where
      `parameter_sets::SPps` has forty, and `nalu.rs` glob-imports that module. Inert
      only because a local item beats a glob import and one hand-maintained explicit
      import list says so. 216 lines deleted; type duplicates **17 → 12**.
- [x] **T5.A2 — the double-cast census** *(session A, `40c489f6`)*. **S24: the brief's
      "121" is the laundering family; the command it quotes reads 100.** Of the 94
      casts with both targets inferred, **93 were no-ops** — deleting them and letting
      rustc type-check the argument compiled unchanged. The 94th bridged two
      declarations of `SDeblockingFunc` (identical field for field, six aliases
      differing only in parameter names). Unified onto `common/deblocking_common.rs`;
      inferred-target double casts **94 → 0**.
- [x] **T5.A3 — `SPartMbInfo`** *(session A, `2ae87d9a`)*. Four declarations, each live
      in its own module; `mv_pred.rs` named the first field `iMbType` where C++ and the
      other three name it `iType`. Unified with its two sub-MB-type tables after an
      element-by-element value comparison.
- [x] **T5.A4 — twelve decoder tables** *(session A, `31312084`)*. C++ defines each
      once; the port had 30 copies. Unified only after each copy's initializer was
      flattened to a token sequence and proved equal. **Three tables recorded as
      must-not-merge**: `g_kuiAlphaTable`/`g_kiBetaTable`/`g_kiTc0Table` are `[52+24]`
      with a `+12` index bias in the decoder and `[52+12]` unbiased in the encoder —
      divergent **in the C++ too**. Table duplicates **27 → 17**.
- [x] **T5.A5 — the census becomes a gate, and F22** *(session A, `2aa51ddb`)*.
      `find_stub_bodies.py` had never scanned `src/decoder`; widened, its duplicate-body
      count went **51 → 198** (156 decoder-touching) and it immediately produced
      **F22**. New `rust/tools/census.sh` + `census_allowlist.txt` (61 entries with
      class and owner, three budgets), wired into `gates.sh` at `commit` level and
      proven red on a planted duplicate and a planted double cast.
- [x] **T5.A6 — the P3 regression tests** *(session A, `888f7aa7`)*. Five tests over
      the three decoder pointer-identity sites, each giving the two pictures the **same
      POC** so a POC-based rewrite fails them. Tests **435 → 440** / **429 → 434**,
      Miri **295 → 300**. S12 collected again (an exactly-sized plane is an
      out-of-bounds `offset` for a C-transliterated copy kernel; Miri caught it).
- [x] **T5.B1 — F21's pin** *(session B, `0fd0c9cc`)*. Golden movement authorized;
      three additive narrow-frame decode assets (16x16, 24x18 cropped from 32x32, and
      16x16 with a lost IDR), C++ decoder goldens, generator in
      `rust/tools/make_narrow_assets.py`. **Only the concealing row pins F21** — the
      two clean rows are green under a revert of the fix, because the divergent copy's
      one call site is `WelsInitRefList`'s concealment prefetch and no cleanly-decoding
      stream reaches it. Coverage proven by reverting `d1c1a7d4` in a scratch worktree.
      Rows **53 → 56**, none moved; tests **440 → 443** / **434 → 437**.
- [x] **T5.B2 — the S25 audit, and F13's decoder site** *(session B, `ff12e966`)*. The brief's
      named hazard, `SPicture::unref`, was **dead** — one caller, its own unit test,
      and no C++ counterpart — so it is deleted rather than restructured. The audit it
      was supposed to support found **nine live functions** in `manage_dec_ref.rs`
      holding `&mut *pCtx` and/or `&mut *pRefPic` across calls that re-enter through
      the same raw pointers; `pRefPic` is always `&mut ctx.sRefPic`, a *subfield* of
      the context, so the two overlap. `MMCOProcess`'s `MMCO_SET_MAX_LONG` loop
      terminates on a count the re-entrant call decrements. All nine now name
      `(*pCtx)` / `(*pRefPic)` per use — the C++'s own spelling. **F13's
      `manage_dec_ref` site closed**: six `as_ptr()`/`as_mut_ptr()` shifts, not one,
      all now `copy_within` behind one helper — and behind the skip sat three *test*
      defects of the same class. `--skip manage_dec_ref` deleted; Miri **300 → 304**.
- [ ] **5.1 Picture & DPB — steps 1 and 2 done, the plane conversion is not started.**
      Session A's closure holds, with two corrections from session B's recount: `unref`
      is gone, and `iPlanes` is written-never-read in the port *and* in the C++, so
      fixing the count at three is subtraction. Nothing embeds the decoder's `SPicture`
      by value, it has **no** `assert_size!` or offset pins, and
      `AllocPicture`/`FreePicture` balance on all eight allocations (F19's check, run
      and clean).
- [ ] 5.2 MbGrid · 5.3 Neighbor & MV (owns **F22**) · 5.4 Deblocking driver ·
      5.5 `decoder_core.rs` · 5.6 `decode_slice.rs` — not started.
