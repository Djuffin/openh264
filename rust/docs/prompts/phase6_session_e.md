# Phase 6, session E — the records, the parameter families, and the third SAD/SATD attempt

Rules by tag from plan §7.6; recipes from `phase6.md` §1. Gate rhythm per D-gate-1:
`gates.sh commit` per commit, `family` per face, one `exit` at close. **Miri runs
once, at the session close, as the `exit` battery's `--lib` step — not between faces**
(direction, 2026-08-19; D's precedent held). S32's cost is why: the four encoder
probes together are ≈460 s per run and the dynamic-slice probe alone ≈357 s.

## 0. Starting position (verify before starting)

HEAD ≥ `a96cb999`, clean; session D's `exit` PASS 13/0/1. Encoder-side (`src/encoder`
+ `src/processing`, ratchet patterns): `raw_ptr` 2331, `unsafe_fn` 684. Re-grep every
count below before acting on it (S24). Sites by receiver file, measured 2026-08-19:

| family | total | where |
|---|---|---|
| `*mut SWelsMD` | 53 | svc_mode_decision 24, svc_base_layer_md 18, wels_func_ptr_def 7, svc_encode_slice 4 |
| `*mut SWelsME` | 39 | svc_motion_estimate 21, svc_base_layer_md 12, svc_mode_decision 4, md 2 |
| `*mut SMVUnitXY` | 65 | svc_base_layer_md 34, svc_mode_decision 27, svc_motion_estimate 2, **picture 1 + wels_preprocess 1 = session F's, do not touch** |
| `*mut SMeRefinePointer` | 17 | svc_base_layer_md 14, md 3 |
| `*mut SCabacCtx` | 25 | svc_set_mb_syn_cabac 13, set_mb_syn_cabac 12 |
| `*mut SMB` | 128 | svc_mode_decision 37, svc_base_layer_md 18, svc_encode_slice 14, svc_set_mb_syn_cabac 13, deblocking 11, rc 9, wels_func_ptr_def 8, svc_encode_mb 7, svc_set_mb_syn_cavlc 4, md 4, encoder_ext 2, slice_multi_threading 1 (**MT file — leave, Phase 7**) |
| `*mut SMbCache` | 96 | svc_mode_decision 41, svc_base_layer_md 16, md 16, svc_set_mb_syn_cabac 8, svc_encode_mb 7, wels_func_ptr_def 5, svc_encode_slice 2, svc_set_mb_syn_cavlc 1 |

Hosts, all owned already: `SWelsMD` is stack-built at four sites
(`svc_encode_slice.rs:1450/:1566/:2076/:2090`), 4000 bytes, 41 inline `SWelsME` in
`sMe`; `SMbCache` is `SSlice.sMbCacheInfo` (`svc_encode_slice.rs:277`); `SCabacCtx` is
`SSlice.sCabacCtx` (`:299`), already pointer-free inside (offsets since 4b); `SMB` rows
live in the layer's `MbArray<SMB>` (D), reached via `mb_at`/`mb_list_root`.

## Settlements (made by reading; do not re-litigate)

**(a) The screen-content ME half is dead.** Parameter validation returns
`ENC_RETURN_UNSUPPORTED_PARA` for `SCREEN_CONTENT_REAL_TIME` (T6.D2's guard — find the
site, cite it in the commit; if it does not stand, stop and report instead). D deleted
the *preparation* half; the *search* half is equally unreachable: the feature-search
family in `svc_motion_estimate.rs`, `SWelsME.pRefFeatureStorage`, and
`ref_list_mgr_svc.rs:1142`'s allocation branch.

**(b) `pMvdCost` stays raw this session** — both record fields (`SWelsMD`, `SWelsME`)
alias the context's `pMvdCostTable` allocation (`encoder_ext.rs:1293`), the same
argument that keeps `pEncSad` for F. The stored pointer is a **row root**
(`svc_encode_slice.rs:1418`), not mid-table. P11's `(root, bias)` accessor executes at
G when the table becomes owned.

**(c) `SWelsME.pEncMb`/`pRefMb`/`pColoRefMb` stay raw fields** — plane cursors into
pictures, session F's family. Everything `*mut u8` + stride is out of scope except
inside face 3's kernel boundary.

**(d) Dispatch-slot types retype in place and lose `extern "C"`.** The slot fn types
in `wels_func_ptr_def.rs` naming `*mut SWelsMD`/`*mut SMB`/`*mut SMbCache` take the
converted references instead. They are internal dispatch — no C caller exists, no
installed fn is `no_mangle` (verify by grep before the commit). A slot whose params
become references drops `extern "C"` (a non-`repr(C)`-safe reference behind `extern
"C"` trips `improper_ctypes_definitions`); the table mechanism itself stays — G
dissolves it.

**(e) Port-added null guards delete with reference params.** `WelsMdP16x16`-style
entry checks (`if pFunc.is_null() || …return i32::MAX`) have no C++ counterpart —
verify each against the C++ before deleting (D-fid-1: deletion *restores* fidelity).

**(f) A param that names what another param already reaches is a cache — delete it,
derive in the callee** (session B's settlement (c) pattern). Applies to every
signature carrying both `pSlice` and `pMbCache`/`sMbCacheInfo`/`sCabacCtx`: the cache
param goes, the callee derives from the slice it already gets. Never a live `&mut`
into a slice field alongside continued use of the same slice pointer (S25, F13's
class). Enumerate the pairs per face before editing.

**(g) `pSlice`/`pCurLayer` params convert opportunistically, not by mandate.**
Encoder-side `*mut SSlice` ≈103 and `*mut SDqLayer` ≈78 sit in the same signatures.
Convert one only where the S25 audit is clean — the callee does not re-reach the same
object through `pCtx` while the `&mut` lives. `&mut T` coerces to `*mut T` at call
sites, so a converted caller feeds an unconverted callee without churn. Default: leave
raw; enumerate survivors in the log for G. `pEncCtx`/`pFunc` params stay raw (G).

**(h) `SMeRefinePointer` is a derived cursor pack** — all five buffers are fixed
offsets into `buffer_inter_pred_me(pMbCache)` (`md.rs:1242`), and
`pQuarPixBest`/`pQuarPixTmp` ping-pong (session C's half-selector recipe). Fields
become offsets/selectors; readers derive from the `&mut SMbCache` in scope. One
builder (`svc_base_layer_md.rs:1576`). `pfCopyBlockByMode` stays a fn-pointer field
(its params are F's plane family).

**(i) `*mut SMVUnitXY` params point into `SMbCache`'s MV/ref caches and `SMB.sMv`
rows** — both inline since C/D. Retype to `&mut [SMVUnitXY]`/`&[SMVUnitXY; N]` or
(host, index) from what the callee already receives; mid-array cursors re-derive from
the array root (S28/S29 spelling, `dangerous_implicit_autorefs` enforces it).

**(j) Derives are the compiler's call**: drop `Copy`/`Clone` on `SWelsMD` (4000 B) if
nothing copies by value — C's `SMbCache` precedent. Keep `repr(C)` where no change
forces otherwise; every layout change re-pins its `assert_size!` in the same commit
(S36).

## Face 0 — two chores before any conversion

**0a — the `sl` sweep preset** (F60's hole closes in the referee, not just the probe).
Twelve rows ≈ T6.D1's `compare.sh` set: `st`, single-threaded, 320x192,
`SM_SIZELIMITED_SLICE`, constraint values chosen so the coded slice count **crosses
`iMaxSliceNum` = 35** (measure it; T6.D1 read 37 at the analogous constraint), both
entropy coders, a couple of rc modes. Wire: `sweep.sh` gains the `sl` case and adds it
to `all`; `gates.sh` `sweep_gate` runs `st mt def sl` (line ≈199; the tally parse is
count-agnostic — nothing hardcodes 341). Record the new totals; every later gate quote
uses them.

**0b — the dead screen-content search half (S18, strip-and-build enumerates).**
Delete from the guard outward; expected set — `WelsMotionEstimateSearchStatic`,
`WelsMotionEstimateSearchScrolled`, `WelsDiamondCrossFeatureSearch`,
`FeatureSearchOne`, `SetFeatureSearchIn`, `SaveFeatureSearchOut`, `SFeatureSearchIn`,
`SFeatureSearchOut`, `PerformFMEPreprocess`, `SWelsME.pRefFeatureStorage` with
`InitMe`'s param (`svc_mode_decision.rs:1204`), `ref_list_mgr_svc.rs:1142`'s branch,
and `SPicture.pScreenBlockFeatureStorage` with its `Reset` touch (`picture.rs:120`)
once its last reader goes — let the compiler name the list; `uiSliceFMECostDown`
**stays** (the real search writes it, D measured). The ref-strategy table's
SCREEN_CONTENT rows stay — session F's file. Re-pin `SWelsME` (96 → expect 88),
`SWelsMD` (4000 → expect 3672), `SPicture` if the field goes (S36); update phase6.md
§5's `SPicture` row note — F's fourteen-row table loses the storage row. Run 0b
**before** the conversions so no face converts dead code.

## Face 1 — the syntax-writer closure

`set_mb_syn_cabac.rs` + `svc_set_mb_syn_cabac.rs` + `svc_set_mb_syn_cavlc.rs` +
`svc_encode_mb.rs`: `SCabacCtx` 25, their `SMB` ~24, `SMbCache` ~16, one S20 closure.
Settlement (f) drives the shape. Gate: `family`.

## Face 2 — the MD/ME closure, in three commits-groups

**2a — MD**: `svc_mode_decision.rs` + `svc_base_layer_md.rs` + `md.rs` + the four
`sMd` roots in `svc_encode_slice.rs`, with the dispatch slots (settlement d) and the
guards (e). `&mut SWelsMD` chains from the stack roots; `sMe` sub-records become field
borrows.
**2b — ME**: `svc_motion_estimate.rs` + `InitMe` + `SMeRefinePointer` (h) +
`SMVUnitXY` (i).
**2c — the residue**: `deblocking.rs` 11, `rc.rs` 9, `encoder_ext.rs` 2 `*mut SMB`
sites and stragglers the greps find — same recipes; rc's context pointers are G's, do
not follow them.

## Face 3 — the third SAD/SATD attempt (boxed; drop-from-the-end starts here)

Preconditions: faces 1–2 landed. S3–S5/S10–S11 revive **for this face only**.

1. **The owed SATD measurement first**: `encoder/sample.rs`'s 7 SATD kernels on the
   L1-resident microbench (harness per `perf_baseline.md`'s parked-families ledger).
   Record solo numbers — owed regardless of the attempt's outcome.
2. **The attempt**: direct dispatch at the now-converted call sites (D-perf-3's
   `mc.rs` precedent) — safe-signature kernels, slices built **once per partition
   search**, not per call (D-perf-2: per-call cursor construction was the measured
   cost); most call sites are fixed-`BLOCK_16x16`, so monomorphic calls are available.
   Scope: SAD (14, `common/sad_common.rs`) + SATD (7). If one family clears and the
   other does not, land the one.
3. **Bar and exits, both acceptable**: bodies ≤1.05x L1-resident → swap lands, ledger
   updated. Otherwise → third park, dated verdict, re-attempt named (Phase 9). Hard
   stop at the face boundary — one attempt, one verdict (D-perf-2's clause). The
   stream instrument stays the session span (D-gate-1); no mid-face stream reads.

## Close

Span: 7 pairs against the session-start HEAD, one span (D-gate-1); S14 for any F3
hit. `exit` battery; log entry with: the family counts before/after (survivors
enumerated — G's `pSlice`/`pCurLayer` leftovers, F's two `SMVUnitXY` sites, the MT
file's one `SMB` site), sizes re-pinned, the sweep's new totals, the SATD verdict.
Update plan §0 and phase6.md §6's E row.

## Non-goals

`pEncCtx`/`pCtx` params and everything context-reached (G); `pCurDqLayer` (G);
`pMvdCost` fields and `pMvdCostTable` (G, P11); `pEncMb`/`pRefMb`/`pColoRefMb` fields,
`pEncSad`, `SPicture` beyond 0b's dead field, SVAA, `picture.rs`/`wels_preprocess.rs`
`SMVUnitXY` sites (all F); `wels_encoder_ext.rs` (Phase 8); MT files —
`slice_multi_threading.rs`, `wels_task_management.rs`, `wels_thread_pool` (Phase 7);
`sSliceBs.pBs` (Phase 7); no new `Vec` fields anywhere (S21 — no host changes this
session); no sweep rows removed or reordered (0a only adds).

## Done-test

`*mut SWelsMD`/`SWelsME`/`SMVUnitXY`/`SMeRefinePointer`/`SCabacCtx`/`SMB`/`SMbCache`
each read **0 code sites** in `src/encoder src/processing` except the enumerated
survivors; the feature-search family is gone and sizes re-pinned; the `sl` preset runs
in both profiles in `gates.sh`; the SATD solo measurement is recorded and the attempt
has a dated verdict; span inside its null band, tripwire unbreached; `exit` PASS.
