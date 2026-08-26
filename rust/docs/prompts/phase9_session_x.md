# Phase 9 — Session X: the `other` family — ref-lists, RC, preprocess, paramsets, trace — and half the send-seam's contingency

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F172** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` (71 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session X is

The off-spine family, one session, cut large (D-scope-1/D-scope-2; X2 exists only
as the spillover name). After H the spine is `&mut` — what remains raw off it is
wide and shallow: rate control, the reference lists, the preprocess pipeline, the
parameter machinery, the bitstream scalars, trace. Four findings ride in because
they live in your files, and one deliverable reaches beyond the family:
**clearing `SVAAFrameInfo`'s raw members removes four of `sWelsEncCtx`'s twelve
`!Sync` reasons — half of the send-seam's F164 contingency** — and the close
measures it by re-running F67's probe (one build).

The surface, measured per file (`grep -cE 'unsafe (extern "C" )?fn' <f>` /
`grep -oE '\*mut |\*const ' <f> | wc -l`):

| file | unsafe fn | raw mentions |
|---|---:|---:|
| `encoder/wels_preprocess.rs` | 44 | 74 |
| `encoder/rc.rs` | 59 | 28 |
| `processing/vaacalc.rs` | 6 | 42 |
| `encoder/paraset_strategy.rs` | 14 | 30 |
| `encoder/au_set.rs` | 11 | 21 |
| `encoder/ref_list_mgr_svc.rs` | 31 | 8 |
| `encoder/picture.rs` | 1 | 14 |
| `encoder/nal_encap.rs` | 7 | 9 |
| `processing/adaptive_quantization.rs` | 2 | 7 |
| `encoder/vlc_encoder.rs` / `common/wels_trace.rs` | 1 / 1 | 6 / 6 |
| `processing/background_detection.rs` | 6 | 5 |
| the rest (`complexity_analysis`, `scene_change_detection`, `param_svc`, `copy_mb`) | ≤5 | ≤2 |

**Not this session**: the ctx remainder (H2's — the in-fork read surface, the 5
slice-returning accessor APIs, **S67's audit of the 24 out-of-family retags**; you
must not *add* a 25th: any new `&mut *pCtx`-class spelling gets F171's
check-what-the-body-holds treatment first); `ParasetStrategy` stays raw (F166 —
it is in your file; its ten call sites are sound by allocation and flipping it is
E0499); `WelsMdInterFinePartitionVaaOnScreen` stays (D-dead-5 keeps it dormant for
Phase 10); F162's site (D-fid-2); the 2 `recon-seam` items (D-mt-3); the au_parser
cluster (J's); `SCREEN_CONTENT(dormant)` semantics — you may convert a *container*
a dormant path shares, never its behavior; no perf (D-gate-1).

## Rules that never bend — gating per the user's standing directive

- **Byte-identical every commit**: `gates.sh commit` before each; `family`
  (583/583 both profiles) after risky ones — a preprocess-pipeline reshaping is
  risky by definition. `sweep.sh` refuses stale drivers; `diffharness/build.sh`
  after edits.
- **No Miri between commits — the session gate once at the close** (S61: quote
  the lane wall beside H's **543 s**). The full-drive fork pair is **not owed** —
  H paid both forms on the flipped tree; the phase exit repeats it.
- **The fork discipline follows your objects**: VAA and RC state is stamped
  pre-fork and **read in-fork** (`ctx_vaa`, `ctx_rc`, `ctx_rc_at` are in-fork
  accessors; `rc.rs` has 10 in-fork ctx bodies). Before reshaping any container,
  check whether its readers appear in the forksplit's in-fork column
  (`python3 rust/tools/phase9_forksplit.py --list`); an in-fork read path keeps
  its shape or stays raw (S63) — the close's small-drive fork probes referee.
  S65: filter any hazard report by whether the conversion it models can happen.
- **Metrics live at both ends** (§7.1): run `unsafe_ratchet.sh report` at start
  and close and quote those — three sessions running have reported
  baseline-relative deltas.
- **S64/S66** in anything you build or count: enumerate a family from the type
  outward; calibrate detectors on a known negative too.
- **S24/S54/S20/S58/S62** as always; ratchet only down, rebaselines carry
  reasons (F151's relocation baggage should come back *out* this session);
  no edits while a gate runs; one battery at a time.
- **Stay in lane**; blockers become findings.

## The riders, measured

- **Step 0 — D-dead-5's deletion**: `WelsRcDropFrameUpdate` (`rc.rs:2594`), dead
  in both trees — upstream's only mention is its own `WelsLog` line
  (`ratectl.cpp:1405`). Read greps quoted, tests and benches included (F119).
- **F86-open — the short-ref shift**: `ref_list_mgr_svc.rs:299–310` shifts
  `pShortRefList[k] = pShortRefList[k+1]` with safe indexing where the C++
  (`ref_list_mgr_svc.cpp:387–391`) writes unchecked. Prove the `k+1` bound from
  the callers' invariants (and say where the proof lives), or guard it exactly
  where upstream's implicit invariant holds. If any reachable input would make
  the port panic where upstream reads/writes out of bounds, **stop and file for
  a ruling** — that is F162's shape and the user rules it (D-fid pattern).
- **F100 — the two empty bodies**: `TraceParamInfo` and `LogStatistics`
  (`wels_encoder_ext.rs:2091/:2093`, literally `{}`). Port them from the C++
  (`welsEncoderExt.cpp`'s `WelsLog` bodies), and build **the smallest log
  referee** — no byte a gate can see, so the referee is new on both harness
  sides: both encoders accept a trace callback through the API; capture both
  sides' callback text on one fixed config and diff (normalize what is honestly
  nondeterministic — timestamps, pointers — and say what you normalized).
- **F117 + `copy_mb.rs`**: the `VaaBackgroundMbDataUpdate` copies stay raw by
  S57's ruling (dark sites convert never) — their allows become *precise* and
  the census rows keep their S57 note; `common/copy_mb.rs` (1 unsafe fn, 2 raw)
  reaches `#![deny(unsafe_code)]` with its remaining sites item-tagged.
- **F151 — the pixmap**: `scene_change_detection.rs:34` holds the relocated
  `sad_8x8_raw` because `Process(&SPixMap, &SPixMap)` (`:85`) walks
  `SPixMap.pPixel` raw. `SPixMap` (`wels_preprocess.rs:265`) is the preprocess
  pipeline's image handle — give it safe plane views (the `PaddedPlane` shape is
  the house pattern) through the pipeline, retire `sad_8x8_raw` onto the safe
  SAD kernels, and take back F151's ratchet rebaseline (+2/+3/+1 on that file —
  it should come out negative this session, stated in the commit).
- **F164's four — the deliverable beyond the family**: `SVAAFrameInfo`'s raw
  members are the send-seam's largest contingency block —
  `pMotionTextureUnit: *mut SMotionTextureUnit` (`wels_preprocess.rs:407`, read
  by `adaptive_quantization.rs:216/:305`), `SComplexityAnalysisParam`'s
  `pGomComplexity`/`pGomForegroundBlockNum: *mut i32` (`:425`+), and the
  `*mut u8`/`*mut u32` members F164's table lists (one of them,
  `pVaaBestBlockStaticIdc: *mut u8` at `:574`, is on the **Ext** struct —
  screen-content-coupled: convert the container, leave the semantics dormant).
  These are owned buffers pointed at raw — the `Vec`-with-index shape the seam
  already uses is the likely answer, but design against the readers, including
  the in-fork ones. **At the close, re-run F67's probe (one build) and report
  which of the twelve `!Sync` reasons fell** — that number is the session's
  headline beyond the ratchet.
- **`nal_encap.rs`'s 3 `MT` tags**: adjudicate — bitstream-shaped (yours, retire
  them) or the threading seam's (H2's, say so). G left them unattributed.

## Steps

0. **D-dead-5's deletion** (one commit, byte-neutral). `WelsRcDropFrameUpdate`
   (`rc.rs:2594`) goes with both trees' read greps quoted in the message
   (`grep -rn 'WelsRcDropFrameUpdate' src tests benches` on this side — F119:
   tests and benches included — and the `codec/` grep showing only
   `ratectl.cpp:1405`, its own log line). `gates.sh commit` proves neutrality.

1. **Ref-lists + LTR** (`ref_list_mgr_svc.rs`). Two motions, in order:
   - **F86 first, before any conversion in the file**: the short-ref shift at
     `:299–310` indexes `pShortRefList[k+1]` where the C++
     (`ref_list_mgr_svc.cpp:387–391`) writes unchecked. Read the callers, prove
     the `k+1` bound from their invariant (state where the proof lives — a
     comment at the site naming the invariant is enough), or guard it exactly
     where upstream's implicit invariant holds. **If any reachable input would
     panic where upstream reads out of bounds, stop and file for a ruling**
     (F162's shape, D-fid pattern) — do not convert around an open soundness
     question.
   - **The LTR structs flip**: the file's whole remaining raw surface is five
     parameters — `pLtr: *mut SLTRState` (`:202`, `:924`, `:1558`),
     `pLTRRecoverRequest: *mut SLTRRecoverRequest` (`:1028`),
     `pLTRMarkingFeedback: *mut SLTRMarkingFeedback` (`:1086`). The file has
     **zero in-fork bodies** (the forksplit's table: `ref_list_mgr_svc.rs`
     0/32), so these flip to `&mut` on the plain S20 pattern; the 31 `unsafe
     fn`s de-unsafe as the raw leaves their signatures (most are vestigial
     already — 8 raw mentions in 31 unsafe fns).
   - Gate: `commit` per cluster, one `family` after the file.

2. **Rate control** (`rc.rs`). The X half is the `*mut SWelsSvcRc` surface —
   **the in-fork ctx bodies (the file's 10, `WelsRcMbInit`, `RcCalculateMbQp`,
   `RcCalculateGomQp`…) are H2's and do not change** (S63).
   - `RcInitLayerMemory(pWelsSvcRc: *mut SWelsSvcRc)` (`:774`) → `&mut`.
   - The four raw accessors — `rc_temporal_over` (`:797`), `rc_gom_complexity`
     (`:812`), `rc_gom_fg_blocks` (`:827`), `rc_gom_sad` (`:842`) — retire onto
     slice-returning safe APIs over the three **live** GOM arrays (D-dead-3
     deleted their dead fourth sibling; these three are the mechanism with ten
     C++ uses). Check each accessor's callers against the in-fork column before
     touching its signature: an in-fork caller keeps a raw route (S63), and
     that route is then H2's to name, not yours to convert.
   - Gate: `commit` per cluster; `family` after the file.

3. **Preprocess + VAA** — the biggest step, ordered inside; `family` after 3a
   and after 3c (both risky):
   - **3a — `SPixMap` grows safe plane views** (`wels_preprocess.rs:265`).
     Design against the four consumer files' `Process(&SPixMap, &SPixMap)`
     methods — `scene_change_detection.rs:85`, `background_detection.rs:126`,
     `denoise.rs:236`, and `complexity_analysis.rs`'s six (`:105`–`:268`). The
     pipeline stamps pixmaps from pool pictures; route the views from the same
     roots (the `PaddedPlane` shape is the house pattern — S37, per call, never
     stored). Then **F151 closes**: `sad_8x8_raw`
     (`scene_change_detection.rs:34`) retires onto the safe SAD kernels, and
     the file's F151 rebaseline (+2 raw_ptr/+3 unsafe_block/+1 unsafe_fn) comes
     back out — state the reversal in the commit.
   - **3b — vaacalc's five raw kernels retire** (`VAACalcSad_c :62`,
     `SadVar :105`, `SadSsd :141`, `SadBgd :180`, `SadSsdBgd :218`): their safe
     twins already exist (`vaa_calc_sad :488` …), and the raw five's only
     remaining callers are the file's own differential tests
     (`:755/:790/:845`) — session F's SAD precedent applies verbatim: the
     old side retires per the test file's charter, the property re-anchors on
     the safe kernels, read greps include tests and benches (F119).
   - **3c — F164's members convert** (the send-seam deliverable):
     `pMotionTextureUnit: *mut SMotionTextureUnit` (`wels_preprocess.rs:407`;
     readers `adaptive_quantization.rs:216/:305`),
     `SComplexityAnalysisParam`'s `pGomComplexity`/`pGomForegroundBlockNum`
     (`:425`+), and the F164 table's `*mut u8`/`*mut u32` members — one of
     them, `pVaaBestBlockStaticIdc` (`:574`), sits on the **Ext** struct:
     screen-content-coupled, so **convert the container, do not touch the
     dormant semantics**. These are owned buffers pointed at raw; the
     `Vec`-with-index shape the seam uses is the likely answer, designed
     against the readers **including the in-fork ones** (`ctx_vaa`'s readers —
     an in-fork read path keeps its shape).
   - **3d — F117's allows become precise + `copy_mb.rs` denies**: the
     `VaaBackgroundMbDataUpdate` copies stay raw by S57's ruling with
     item-level allows and the census note kept; `common/copy_mb.rs` (1 unsafe
     fn, 2 raw) gets `#![deny(unsafe_code)]` with its sites item-tagged.
   - **Planted faults** (S55/S59/S64, reverted, honest counts): a pixmap plane
     root +1 — `dl` is the only preset running METHOD_DOWNSAMPLE and `bg`
     drives the background detector, so quote those; a VAA-kernel fault → `bg`
     rows. A 0-row fault means an inert field before an unreached path (F155).

4. **The scalars**, in this order:
   - `au_set.rs`: `*mut SWelsSvcCodingParam` → `&`/`&mut`; the `_pLogCtx:
     *mut SLogContext` parameters are unused by name — S54: read every caller,
     then delete the dead parameters rather than retype them.
   - `paraset_strategy.rs`: the strategy object's internals (30 raw mentions)
     — **minus `ParasetStrategy` itself** (F166: permanently raw; its tag and
     doc stay).
   - `nal_encap.rs`: the `bs_buffer` glue (`:137`) and `dst: *mut u8` (`:415`)
     → slices; the two `unsafe extern "C"` unload fns (`:334`, `:381`) are
     C-ABI boundary — tag them `C-ABI` if not already, they are lawful
     remainder; **adjudicate the 3 `MT` tags** (bitstream-shaped → retire
     here; seam-shaped → H2's, say so in the report).
   - `vlc_encoder.rs` (6), `wels_trace.rs` (6 — the glue step 5 will lean on),
     `param_svc.rs` (1).
   - `picture.rs` **mostly stays**: `:45–54` is `SScreenBlockFeatureStorage`'s
     own guts — Phase 10's dormant storage (F164 lists its pointer as Phase
     10's `!Sync` reason, not yours); make its tags precise and leave it. The
     two `data_ptr` mints (`:268`, `:289`) are live API from F156's fix — they
     stay.
   - Gate: `commit` per file; one `family` at the step's end.

5. **F100 + the log referee**:
   - Port `TraceParamInfo` from `welsEncoderExt.cpp:505` (called on the
     init/SetOption paths — `:202`, `:229`, `:334`) and `LogStatistics` from
     `:569` (the per-frame statistics log) into the empty bodies at
     `wels_encoder_ext.rs:2091/:2093`. Pure `WelsLog` formatting — no byte a
     gate can see (F100's whole point).
   - The referee: both drivers speak the same C API —
     `SetOption(ENCODER_OPTION_TRACE_CALLBACK = 26, cb)` (+`_CONTEXT = 27`)
     exists on both sides (`codec_api.rs:252`). Register a capturing callback
     in `cxx_enc.cpp` and `rust_enc`, run **one fixed config** (pick a `dl`
     row — the statistics log is config-dependent), capture both texts, diff.
     Normalize only what is honestly nondeterministic — timestamps, pointer
     values, version strings — and list every normalization in the script.
     Ship it as a small script beside `compare.sh` (not wired into the sweep),
     loud per S58: nonzero exit on mismatch, and calibrate it both ways (S66:
     a planted format-string typo must fail it; the clean run must pass).

6. **Close**, itemized:
   - `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` once — S61: lane
     wall beside H's **543 s**, ratio stated.
   - **F67's probe**: a scratch `fn _s<T: Sync>() {} _s::<sWelsEncCtx>();` —
     rustc's E0277 notes enumerate the offending member chain; record the
     before/after lists, revert the scratch, and report **which of the twelve
     `!Sync` reasons fell, by owner** — the session's headline beyond the
     ratchet.
   - Both censuses regenerated (S58's respelling duty — if `SPixMap`'s new
     views respell a fact a census keys on, the census learns it in the same
     commit).
   - `unsafe_ratchet.sh report` at the close against the start capture —
     live at both ends (§7.1); tags re-measured; the F151 reversal stated.
   - Findings from **F172**; the log; the charter row.

**Drop order if short**: 5, then 4 — each becomes X2's named frontier
(S60/F143); step 3 stops only at a file boundary (3a is never split); steps
0–2 are never dropped.

## What to report back

Plain prose: commits with live-at-both-ends ratchet deltas; F86's proof or its
ruling request; the pixmap's new shape described against two readers (one
scene-change, one downsample); the planted faults' honest counts; **F67's
before/after — which `!Sync` reasons fell and which remain, by owner**; the log
referee's design and its normalizations; the MT adjudication; every place this
brief was wrong, quoting the sentence; and what H2 and J inherit — H2's seam now
designs against whatever `SVAAFrameInfo` became, so describe that end state
carefully.
