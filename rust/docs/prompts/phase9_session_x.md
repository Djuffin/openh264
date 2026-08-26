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

0. **D-dead-5** (one commit, byte-neutral, both trees' read greps).
1. **Ref-lists + LTR** (`ref_list_mgr_svc.rs`, F86 inside — its proof or its
   ruling before the file's conversions land).
2. **Rate control** (`rc.rs` — the biggest unsafe-fn count; its 10 in-fork
   bodies keep their read shape, S63).
3. **Preprocess + VAA** (`wels_preprocess.rs`, `vaacalc.rs`,
   `background_detection.rs`, `complexity_analysis.rs`,
   `adaptive_quantization.rs`, `scene_change_detection.rs`): `SPixMap`'s views,
   F151's kernel retired, F164's four members, F117's precise allows +
   `copy_mb.rs` deny. Planted fault once per new conversion shape
   (S55/S59/S64): perturb a field that varies — a pixmap plane root +1 should
   fail the presets that drive preprocess (`dl` is the only one running
   METHOD_DOWNSAMPLE; `bg` drives the background detector) — quote honest
   counts, and a 0-row fault means an inert field before an unreached path.
4. **The scalars**: `paraset_strategy.rs` (minus `ParasetStrategy` itself),
   `au_set.rs`, `nal_encap.rs` (+ the MT adjudication), `vlc_encoder.rs`,
   `picture.rs`, `param_svc.rs`, `wels_trace.rs`.
5. **F100 + the log referee**.
6. **Close**: the session gate once (S61 vs 543 s); **F67's probe re-run and the
   fallen-reasons count reported**; both censuses (S58's respelling duty);
   findings from **F172**; the log; the charter row; metrics live at both ends.

**Drop order if short**: 5, then 4 — each becomes X2's named frontier
(S60/F143); step 3 stops only at a file boundary; steps 0–2 are never dropped.

## What to report back

Plain prose: commits with live-at-both-ends ratchet deltas; F86's proof or its
ruling request; the pixmap's new shape described against two readers (one
scene-change, one downsample); the planted faults' honest counts; **F67's
before/after — which `!Sync` reasons fell and which remain, by owner**; the log
referee's design and its normalizations; the MT adjudication; every place this
brief was wrong, quoting the sentence; and what H2 and J inherit — H2's seam now
designs against whatever `SVAAFrameInfo` became, so describe that end state
carefully.
