# Phase 8b — Correctness and parity (charter)

*Steward, 2026-08-21, written at `cd1bc977` (code at `863eaec7`, Phase 8 closed at
`47126548` + T8.C8). Governing decision: **D-prio-1** — correctness and feature
parity before safety and performance. Phase 10 (screen content) follows this phase;
Phase 9 (the hygiene/safety endgame) follows both and keeps its number because 690
in-code tags name it.*

## 1. What this phase is for, and what it is not

**For:** a consumer who swaps `libopenh264` for the cdylib gets the reference's
answers — the same codes, bytes, frame counts, option values — on every entry point
and every feature the reference ships, or an *error* where the port does not ship it
yet. "Working" is measured by the reference's own instruments: upstream's
`test/api` (199 tests, `tools/abi_harness/gtest_stretch.sh`), the `ecref` corpus
(2902/17 output, 2919/0 codes), conformance 60/60, the diffharness sweeps, and the
per-entry-point referees (`ecref --nodelay`, the ABI harness).

**Not for:** raw-pointer conversions (Phase 9 — every new raw signature forced by a
raw neighbour is tagged `port-raw(Phase 9)` under D-exit-1), perf work (D-gate-1; one
span at the close), screen content (Phase 10 — its 7 gtest rows are *named* here with
that owner, not fixed here).

**The method this phase inherits (F82, S47):** an entry point with no referee had
neither half ported; 81 gtest failures read by fixture name hid 30 decoder assertions
for a day. Tally by where the assertion fires; give every entry point its own
instrument; and when a comment says "not ported", grep the named function before
believing it — three `encoder_ext.rs` doc blocks still say MT and size-limited
slicing are unported, and Phase 7 ported both.

## 2. The inventory at `863eaec7`

**The 44 remaining gtest rows, by assertion site** (re-extracted from
`tools/abi_harness/out/gtest/rust.log`, 155/199 passing):

| family | rows | assertion site | what the port has | reference |
|---|---|---|---|---|
| decoder `GetOption`/`SetOption` | **21** | `decode_api_test.cpp:45/81/132/204/348/404` ×3; `decoder_ec_test.cpp:701/789`; `encode_options_test.cpp:1990` | `decoder_get_opt_c` (`api/codec_api.rs:3375`) **4 arms**, `decoder_set_opt_c` (`:3299`) **6 arms**; everything else is `_ => {}` — success, nothing written, so `VCL_NAL`/`IS_REF_PIC` read caller garbage and `TEMPORAL_ID` stays −1. **And two feeders are stubs**: `GetVclNalTemporalId` at `decoder_core.rs:1061` is `{}` (called at `:3647`; upstream `decoder.cpp:716–724` writes `iFeedbackVclNalInAu`/`iFeedbackTidInAu`/`iFeedbackNalRefIdc` from the AU's last VCL NAL), and `uiCurIdrPicId` is declared (`decoder_context.rs:1820`), reset, and never written (upstream `decoder_core.cpp:1061`, under `LONG_TERM_REF`, which `decoder_context.h:67` defines) | `welsDecoderExt.cpp:584–695` (17 get arms), `:479–584` (10 set arms) |
| LTR + `ForceIntraFrame` | **3** | `ltr_test.cpp:39` — with `bEnableLongTermReference=1, iLTRRefNum=1`, marking period 2, every frame after `ForceIntraFrame(true)` must be IDR | `wels_encoder_ext.rs:1940` is faithful to `welsEncoderExt.cpp:491–505`; the cause is below it — `ForceCodingIDR` (`wels_encoder_ext.rs:629`), the LTR marking path, or `eFrameType` reporting. Unknown until read. | `welsEncoderExt.cpp:491`, `encoder_ext.cpp` `ForceCodingIDR` |
| parameter-set listing strategies | **6** | `encode_options_test.cpp:556/656/705/772/844/911` — `InitializeExt` returns 1 | `CreateParametersetStrategy` returns `None` for `SPS_LISTING`, `SPS_LISTING_AND_PPS_INCREASING`, `SPS_PPS_LISTING`; `paraset_strategy.rs:755` pins the refusal (the S48 shape, already right) | `paraset_strategy.cpp:404–713` (~310 lines, two classes + the increasing variant) |
| parse-only `DecodeParser` | **3** | `decode_api_test.cpp:923/1001/1048` — `iNalNum` stays 0 | `decoder_decode_parser_c` (`codec_api.rs:3500`) returns `dsErrorFree` and writes nothing; the decoder core already carries 12 + 7 `bParseOnly` sites (`decoder_core.rs`, `decode_slice.rs`) and `ParseOnlyBsBuffers` (`decoder_context.rs:646`) | `welsDecoderExt.cpp:1180–1262` `DecodeParser`, `:1301–1348` `ParseAccessUnit`; parse-only branches in `decoder_core.cpp` (8), `au_parser.cpp` (4), `decode_slice.cpp` (5), `pic_queue.cpp` (1), `parse_mb_syn_cabac.cpp` (1) |
| downsample + denoise | **3** | `HashFunctions.h:31` — `EncoderOutputTest` rows 4 (denoise on), 5 (2 spatial layers), 7 (4 layers): hashes differ | `METHOD_DOWNSAMPLE`/`METHOD_DENOISE` untranslated (`processing/mod.rs:29–39`); `BilateralDenoising` is an empty body (`wels_preprocess.rs:1590`); `DownsamplePadding` returns `RET_NOTSUPPORTED` at `:1647` **and both callers drop it (`:1240`, `:1327`)** — a two-layer encode reports success with un-downsampled layers. **Silent** (S48) | `codec/processing/src/denoise/*.cpp` 250 lines, `downsample/*.cpp` 594 lines |
| screen content | **7** | `BaseEncoderTest.cpp:92` ×5 (`EncoderOutputTest` rows 8–12, all `SCREEN_CONTENT_REAL_TIME`), `encode_options_test.cpp:2195`, `encoder_test.cpp:323` | the D-scr-1 guard (`encoder_ext.rs:817`) | **Phase 10's** |
| POC tiebreak | **1** | `HashFunctions.h:20` (`DecoderOutputTest.CompareOutput/39`, `test_scalinglist_jm.264`) | deliberate, argued at `decoder_conformance_test.rs:238`, corroborated by the JVT gold for `CABA2_SVA_B` which upstream itself fails | **D-poc-1**: keep (JVT-correct) or match upstream (bug-compatible drop-in) — **decided 2026-08-22: keep** — the row is permanent, with the reason |

**Off the gtest list:**

* **F80** — `IncreasePicBuff`/`DecreasePicBuff` never ported; `WelsRequestMem`'s
  third arm (same size, changed `num_ref_frames`) absent. No asset in `res/` is known
  to drive it. Measure; if `res/` has none, make one with the reference encoder.
* **The F77 instrument** — `MbGrid::get/get_mut/at/at_mut` (`safe/mb_grid.rs:272+`)
  index with a bare `self.data[mb_xy]`; a correct index into a stale allocation reads
  as an off-by-one. `#[track_caller]` + an explicit assert naming index, length and
  the grid's `mb_width×mb_height`.
* **The census.** F80, the listing strategies, the two plugins and the two stubs above
  were all found by accident. `tools/find_stub_bodies.py` exists (223 flagged of 1887
  functions) but **its `CPP_DIRS` omit `codec/decoder/plus/src` and
  `codec/encoder/plus/src`** — it never compared `GetOption`/`SetOption`/`DecodeParser`
  — **and it diffs call sets, so a C++ body that only assigns (like
  `GetVclNalTemporalId`) is invisible to it** (S46: the instrument's empty case).
  This phase widens it and adds a by-name completeness census: every C++ function
  definition under `codec/` checked for a port counterpart, the missing classified by
  reading — *dead in the reference* (SIMD-only, debug-only, no caller) / *renamed* /
  *missing* — into `rust/docs/phase8b_port_census.md`. The missing list is the rest
  of this phase's inventory, and it is what makes the exit claim honest.
* **Dead stubs to delete, not fix**: `GetPrevFrameNum` (`decoder_core.rs:1064`,
  returns 0) is the multi-threaded detour's helper; the port's single call site reads
  `pLastDecPicInfo.iPrevFrameNum` directly (`:4356`) as the C++'s single-threaded
  read does. Delete under S18; the census will list others.

## 3. Instruments, first

1. **The gtest tally becomes a gate.** `tools/abi_harness/gtest_known_failures.txt`
   — one row per known failure: `test · owner · reason` — and
   `gtest_stretch.sh --check`, which fails on any failing test **not** in the list
   and on any listed test that **passes** (a stale row is a lie about coverage). Wired
   into `gates.sh exit` beside the ABI harness (`gates.sh:461–485` is the pattern:
   `PIPESTATUS[0]` + a verdict line, both required). `--filter=<gtest pattern>` for
   per-family runs. The number then moves only by editing the list in the same commit
   as the fix.
2. **Every decoder entry point has a referee.** `DecodeFrame2` (corpus, conformance),
   `DecodeFrameNoDelay` (`ecref --nodelay`, T8.C8), `FlushFrame` (the reordering
   tests); `GetOption` gets `ecref --options` (per-call values of every get-able
   scalar option, from the C++) and `DecodeParser` gets one when parse-only lands.
3. **A diffharness spatial-layer preset** — `iSpatialLayerNum` 2/4 × denoise on/off
   × two rc modes, 720p and CIF — lands *with* the downsample + denoise port and is
   red before it (`rust_enc/main.rs:69/84` and `cxx_enc.cpp:85/101/123` hard-code 1
   layer and denoise off today; both sides gain the knobs).

   **Named `dl` (dependency layers), landed at T8b.C2.** This charter called it `sp`
   while it was still a plan, and session B independently added a *different* preset
   called **`ps`** (the five `eSpsPpsIdStrategy` values). The two were read as one
   name spelled two ways when session C's brief was written; they are two presets.
   `dl` is the multi-layer one and the only one that runs the downsampler; `ps` is
   the parameter-set one. Both are in `sweep.sh` and both run at the phase close.
4. **The census** (§2) — widened `find_stub_bodies.py` + `tools/port_census.py`.

## 4. Rules for this phase (standing rules apply; these are the phase's own)

1. **Parity is proved against the reference's answers, per fix, in the tree.** Each
   fix lands with (a) an in-tree test that pins the C++'s values (via `ecref`,
   `cxx_enc`, or the reference's own test logic re-stated), measured red before and
   green after; (b) the gtest rows it owns flipping, their allowlist rows deleted in
   the same commit; (c) `gates.sh commit` green. The gtest binary is external and
   ~40 s; the in-tree test is what runs per commit.
2. **Tally by assertion site** (S47). A fixture name is a surface.
3. **S48.** A feature this phase cannot finish in its session is left *refusing* at
   the entry point, with a test pinning the error code and an allowlist row naming
   the owner. No silent success.
4. **A divergence that looks better than the reference is reported, not decided**
   (D-poc-1 is the model; the CABA2_SVA_B and F72 classes are the precedents).
5. **New code is safe Rust where its signature allows.** A raw signature forced by a
   raw neighbour is tagged `port-raw(Phase 9)`; `unsafe_ratchet.sh check` stays green
   per commit, and a deliberate increase is rebaselined in the same commit with the
   reason in the message.
6. **D-gate-3 — the sweeps, Miri, the benches and the perf span run once, at the
   phase close; everything between is fast.** Per commit `gates.sh commit` (+ the
   commit's own in-tree parity test). Per session close: `gates.sh commit`,
   `gtest_stretch.sh --check`, `abi_exports.sh`, `abi_harness/run.sh`, and
   `compare_all.sh` when the decoder was touched — no sweeps, no Miri, no benches, no
   span. A family that could move bytes on a sweep configuration runs *one* targeted
   diffharness pair (seconds), never a preset. `gates.sh exit` unscoped + the 7-pair
   span with its null, once, at the close of the phase (D-gate-1/D-gate-2 still say
   no perf work and phase-exit Miri; D-gate-3 moves the sweeps there too).
7. **"Not ported" is re-measured before it is believed** — grep the named function;
   the stale `encoder_ext.rs:3022` block is the cautionary case.

## 5. Sessions and order

| session | work | tally expected |
|---|---|---|
| **A** (Phase 8 session C continuing) — **done**, `925ac828`..`5478ae7e` | the gate + allowlist (T8b.A1); the census instruments + `port_census.py` + the classification file (A2); the decoder option arms whole, the two stub feeders and the per-call reset block (A3, 21 rows); `ForceCodingIDR` — a stub, and an abort behind it (A4, 3 rows + F86); S48 refusals for denoise/downsample (A5); F80 reachable, asset built, **F87** (A6); the F77 assert (A7) | 155 → **179**, then → **165** by S48 |
| **B** — **done**, `b2c5cd7e`..`9d2c5e50`; brief [`phase8b_session_b.md`](phase8b_session_b.md) | the seed pin (S49) + `--seeds` (B1) — which found **F89**, a seventh listing-strategy row hidden by the clock; parse-only whole, with four missing pieces not three: the `sSavedData` capture as well (B2), `ecref --parse-only`, seven goldens, the parity test, the harness part; the three listing strategies + `FindExistingPps` + `WriteSavcParaset_Listing` + the three validation traces (B3), five byte-identical strategy pairs; the census reclass and two stale comment blocks (B4); `ecref --ec`/`--trace` + `ecref_rs`, F88 to seed 5 and F96 to a function (B5) | **164 → 173** (the honest pinned baseline is 164, not 165 — F89); the 17 init-refusal rows stay C's |
| **C** — brief [`phase8b_session_c.md`](phase8b_session_c.md) | downsample + denoise + the `dl` preset (**17** rows; `ParseOnly_General` and `SimulcastAVC_SPS_PPS_LISTING` return with them). **Headline risk**: the reference library links the **NEON** downsamplers (`nm libopenh264.a` → `*_AArch64_neon`), the port would translate the `_c` kernels, and upstream's own dual-hash `EncoderOutputTest` rows say the two average in a different order — byte parity against `cxx_enc`/the gtest goldens is **not** guaranteed by a faithful `_c` port. Measure `_c`-vs-NEON identity first (step 0); if they differ, the decision ladder in the brief. Then then the census's missing list, smallest first; **F80's port, which A found reachable and measured as F87** — `IncreasePicBuff`/`DecreasePicBuff` plus grow/shrink on `safe::Pool`, and the asset moves into `res/` in the same commit | → **191** |
| **D** (if needed) | census residue; the close | — |

*Session A's tally, read twice.* T8b.A4 closed at **179/199**, the brief's target.
T8b.A5's S48 refusal then took it to **165/199**: 14 rows that were passing *by never
checking bytes* now refuse at `InitializeExt`, because the port cannot downsample and
was silently encoding lower spatial layers from stale pool content. Both numbers are
true and they measure different things — how much of the reference the port
implements, and how much it implements without lying about it. The charter's S48
picks the second; put to the user as a fork at the time, and confirmed. All 17
downsample/denoise rows return together in session C.

**Session C inherits, beyond its own listing:** F87 (`dsOutOfMemory` where the
reference decodes cleanly) with its asset at `tests/data/f80/`, its generator
(`tools/make_numref_asset.cpp`) and the C++'s row; and the census's 12 `8b.C` rows.
From session B: **F96** (`InitRefPicList` returns `ERR_NONE` where the reference
returns `ERR_INFO_REFERENCE_PIC_LOST` under `ERROR_CON_DISABLE` — narrowed to one
function, with `ecref_rs --ec=0` as the referee), **F88** (a concealment divergence
under `ERROR_CON_SLICE_COPY`, reproducing at `--seeds=5..5`) and **F93** (parse-only
on a damaged stream, to be re-measured after F96). None is session C's *listing* work;
they are the queue behind it.

**Session B's tally note.** The pinned-seed baseline is **164/199, 35 rows**, not
165/34: `DecodeCrashTestAPI.DecoderCrashTest` draws its parameter-set strategy from
`rand() % 7` and takes a listing one on three draws of seven, so before S49 it flipped
between runs and was never allowlisted (**F89**). It left with the other six at T8b.B3.
The exit target is unchanged.

*Exit gate:* `gtest --check` green with an allowlist of **exactly** the 7 Phase 10
rows and D-poc-1's one (decided 2026-08-22: keep the JVT-correct tiebreak; the row is
permanent); the census's
missing list empty or every entry owned by name; `dl` in the sweep, byte-identical
both profiles **against whichever downsampler the reference actually runs** (the NEON
question, resolved in C step 0); every decoder slot with a named referee; `exit` battery PASS unscoped
**(the phase's only sweep + Miri + bench run, D-gate-3)**; the perf span stated
against D-perf-4's tripwire, unchanged by this phase. **On the 165 reading**: session A's S48
refusal took 14 multi-layer rows from passing-for-the-wrong-reason to failing-at-init, so
the headline number fell while the port got more honest; the exit allowlist is still the
7 + 1 above, which is why C's downsample port must return all 17 at once.

## 6. Hand-offs

To **Phase 10**: the 7 screen-content rows by name; `METHOD_SCROLL_DETECTION`,
`METHOD_SCENE_CHANGE_DETECTION_SCREEN`, `METHOD_COMPLEXITY_ANALYSIS_SCREEN`
(`processing/mod.rs` table) and `imagerotate` if the census finds it reachable only
there. To **Phase 9**: everything in the plan's *Inherited from Phase 8* block,
unchanged, plus whatever raw signatures this phase had to add under tag; and from
session A: **F84** (two orphaned threaded-decoder functions and the 18 pad kernels only
they reach — S18 deletions), **F85** (recorded as S22's clause; the tool's docstring
says it), and **F86's open half** — the unchecked `pShortRefList[iRefIdx + 1]` write
(`ref_list_mgr_svc.cpp:387–391`; the port panics at `ref_list_mgr_svc.rs:684`), a bound
to assert in the port's terms.
