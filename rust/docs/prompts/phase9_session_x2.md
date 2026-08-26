# Phase 9 — Session X2: the family closes — two rulings, the smoke, the scalars, the log referee

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F179** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` (78 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session X2 is

The `other` family's close (D-scope-3: this session finishes X's deferred work
cleanly so H2 gets an undiluted session for the seam). Four masses: the two
rulings the user took at X's close — **D-fid-3** (F173's abort clamped, and a
gtest smoke joins the session gate) and **D-dead-6** (`pGomComplexity` deleted) —
then X's deferred scalars, the F100 log referee, and F177's missing port.

**Not this session**: everything H2 owns — the in-fork read surface, the 5
slice-returning accessor APIs, **S67's audit of the 24 out-of-family retags** (do
not add a 25th), F167's owed Miri, and `nal_encap.rs`'s **two seam-shaped MT
tags** (X adjudicated them H2's; leave them); `ParasetStrategy` (F166, raw
permanently — it is in a file you convert; its tag and doc stay); the 2
`recon-seam` items (D-mt-3); F162's site (D-fid-2); `SCREEN_CONTENT(dormant)`
semantics (containers yes, behavior never); the au_parser cluster (J's); no perf
(D-gate-1).

## Rules that never bend — gating per the user's standing directive

- **Byte-identical every commit**: `gates.sh commit` before each; `family`
  (583/583 both profiles) after risky ones. `sweep.sh` refuses stale drivers —
  `diffharness/build.sh` after edits.
- **No Miri between commits — the session gate once at the close.** S61: the
  lane wall beside X's **507 s** (the smoke you add in step 0a lands *outside*
  the Miri lane, so lane-vs-lane stays clean); the **battery's total wall is
  re-measured against D-gate-6's 15-minute cap** and the baseline note records
  the regime addition, the way D-gate-6's own note did. The full fork pair is
  not owed (H paid both forms; the exit repeats it).
- **S68**: before executing on any finding's attribution, check out the commit
  it names and read the line it quotes — this session executes on F173, F174,
  and F177, all one session old; the check costs one read each.
- **S64 (+ the writer clause)** and **S59 (+ the escalation clause)** bind your
  greps and your planted faults: enumerate types outward, grep the *reference
  tree's writer* before believing a field inert, and when a verdict is
  thresholded, escalate the fault to the verdict and report both numbers.
- **F178's caveat**: the ratchet counts `*mut` and `no_mangle` in prose. If a
  gate trips on documentation, respell the prose to avoid the literal token and
  say so in the commit; per-file deltas of 1–2 on comment-only changes are
  noise. The real fix is J's (with F170).
- **Metrics live at both ends** (§7.1): `unsafe_ratchet.sh report` at start and
  close; today it reads raw_ptr **1369**, unsafe_fn **601**, unsafe_block
  **268**, shim **32**.
- **No tree edits while a gate runs; one battery at a time** (§7.5). Ratchet
  only down; rebaselines carry reasons. **Stay in lane**; blockers become
  findings.

## Step 0a — D-fid-3: the clamp, then the smoke

- **The clamp.** `GetRefMb` (`svc_mode_decision.rs:1470`-region) computes
  `kiRefMbIdx = (iMbY>>1)*refWidth + (iMbX>>1)` and reads it checked; upstream
  (`svc_mode_decision.cpp`, its own comment: "because current lower layer is
  half size on both vertical and horizontal") reads it **unchecked**, and
  non-2:1 simulcast breaks the invariant — upstream reads out of bounds, the
  port aborts (F173: `mb_xy 549 >= 549 (grid 61x9)`, panic-in-nounwind).
  The ruling: **clamp to the base layer's last valid record** where the
  invariant breaks — byte-identical wherever it holds (2:1 keeps the index in
  bounds, same record), defined and non-aborting exactly where upstream reads
  garbage. Document the divergence at the site (D-poc-1's pattern: the ruling,
  the reason, the finding).
- **Acceptance**: `bash rust/tools/abi_harness/gtest_stretch.sh --check`
  **completes with a tally** — the suite has not tallied since E3. Record the
  row's verdict: if `SimulcastAVC_SPS_PPS_LISTING` passes, say so; if it fails
  on bytes, it joins `gtest_known_failures.txt` **with the evidence and a
  finding** (the allowlist was exactly 8 + D-poc-1's row at 8b's close — any
  growth is named).
- **The smoke.** A filtered `--check` subset joins `gates.sh session`, budget
  **≤60 s**, placed in the native battery (not the Miri lane). Start from
  `--gtest_filter=EncodeDecodeTestAPI.Simulcast*` and trim or extend to
  budget — measure the wall and quote it. **S66, both directions**: with the
  clamp reverted the smoke must FAIL (the known positive); on the fixed tree it
  must PASS (the known negative); both runs recorded in the commit. Then
  re-measure the whole session gate against the 15-minute cap and add the
  regime line to `miri_wall_baseline.txt`'s comment block.

## Step 0b — D-dead-6: `pGomComplexity` deleted

Already a safe `Vec<f64>` after X's container work — the deletion is four safe
sites: field (`rc.rs:356`), default (`:446`), allocation (`:775`), per-frame
`fill(0.0)` (`:1569`), plus the S18-retirement comment at `:796` that now names
a deleted field. Quote both trees' read greps (upstream: `rc.h:188`,
`ratectl.cpp:73/:87/:668` — alloc, null, memset, no reader). Byte-neutral;
`gates.sh commit` proves it.

## Step 1 — the scalars (X's step 4, minus what X already did)

Per-file, measured today (`grep -cE 'unsafe (extern "C" )?fn'` / `grep -oE
'\*mut |\*const ' | wc -l`):

- **`paraset_strategy.rs`** (14 unsafe fn / 30 raw): the strategy object's
  internals — **minus `ParasetStrategy` itself** (F166's tag and doc stay).
  Enumerate the raw from the types outward (S64), not from this table.
- **`au_set.rs`** (11 / 21): `*mut SWelsSvcCodingParam` → `&`/`&mut`; the two
  `_pLogCtx: *mut SLogContext` parameters are unused by name — S54: read every
  caller, then **delete** the dead parameters rather than retype them.
- **`nal_encap.rs`** (7 / 9): the `bs_buffer` glue (`:137`) and `dst: *mut u8`
  (`:415`) → slices. **`WelsUnloadNal` is not a C-ABI boundary** (X verified:
  no export attribute, no dispatch slot, five Rust callers) — drop the
  `extern "C"` and let its signature go honest. The two seam MT tags stay
  (H2's).
- **`vlc_encoder.rs`** (1 / 6), **`wels_trace.rs`** (1 / 6 — step 2 leans on
  it), **`param_svc.rs`** (0 / 1).
- **`picture.rs`** (1 / 14) **mostly stays**: `:45–54` is
  `SScreenBlockFeatureStorage`'s own guts — Phase 10's dormant storage; make
  the tags precise, convert nothing dormant; the two `data_ptr` mints are live
  API and stay.
- Gate: `commit` per file; one `family` at the step's end.

## Step 2 — F100 + the log referee (X's step 5, unchanged)

- Port `TraceParamInfo` from `welsEncoderExt.cpp:505` (called at `:202/:229/
  :334` — the init/SetOption paths) and `LogStatistics` from `:569` into the
  empty bodies at `wels_encoder_ext.rs:2091/:2093`. Pure `WelsLog` formatting —
  no byte a gate can see, which is why the referee exists.
- The referee: both drivers speak the same C API —
  `SetOption(ENCODER_OPTION_TRACE_CALLBACK = 26, cb)` (+`_CONTEXT = 27`,
  `codec_api.rs:252`). Register a capturing callback in `cxx_enc.cpp` and
  `rust_enc`, run **one fixed config** (a `dl` row — the statistics log is
  config-dependent), capture both texts, diff. Normalize only what is honestly
  nondeterministic — timestamps, pointer values, version strings — and list
  every normalization in the script. Ship it beside `compare.sh`, not wired
  into the sweep; loud per S58 (nonzero exit on mismatch); **S66 both ways** —
  a planted format-string typo must fail it, the clean run must pass.

## Step 3 — F177's missing port: `SetRefMbType`

25 lines of C++ (`wels_preprocess.cpp:811`), called once (`:916`) to fill
`sComplexityAnalysisParam.uiRefMbType` from the ref picture's MB types. The port
never had it, which left the field permanently null with unguarded readers (all
dark behind `bEnableAdaptiveQuant = false` — F177). **Verify the caller's flag
gating first** (S68: read the line, both trees), then port the 25 lines and
restore the field's buffer the way X restored its two siblings (an owned
container reaching readers as a slice). Byte-neutral behind the flags — the
gates prove it; **if any byte moves, stop and file** (it would mean the path is
not dark, which is itself the finding). This removes `SVAAFrameInfo`'s
`*mut u32` `!Sync` reason: **re-run F67's probe at the close** (the scratch
`fn _s<T: Sync>(){} _s::<sWelsEncCtx>();` — read the E0277 member chain,
revert) and report the count — **ten → nine expected**, expected-vs-actual
stated (S60).

## Step 4 — close

- `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` once — **with the new
  smoke in it**; S61 lane wall vs **507 s**; the battery wall vs the 15-minute
  cap; the baseline note's regime line.
- F67's probe (step 3's expected ten → nine); both censuses (S58's respelling
  duty); findings from **F179**; the log; the charter row; metrics live at both
  ends.

**Drop order if short**: 3, then 2 — **both have slipped once already** (step 2
was X's step 5), so a second deferral goes to J only with the user's sign-off,
named in the report — it does not silently become a third session. Steps 0–1
are never dropped.

## What to report back

Plain prose: the gtest tally and the Simulcast row's verdict (pass, or
allowlisted with evidence); the smoke's filter, wall, and both calibration
runs; D-dead-6's greps; per-file closure of the scalars with live-at-both-ends
ratchet deltas; the referee's design, normalizations, and calibration; F67's
before/after with expected-vs-actual; every place this brief was wrong, quoting
the sentence; and what H2 and J inherit — H2's inheritance should now be
*only* the seam, the audit, F167's Miri, and the two nal_encap tags; if
anything else remains, name it and whose it is.
