# Phase 5, session W — W6 bottom-up: the callees convert first

Phase 5 did not close at V. Exit conditions 1–3 are unmet (decoder `raw_ptr` 974;
6 of 22 modules deny-clean; `SHIM(` 52 against a survivor list of 1). Condition 5
**is** met and closed: the day two is paid, the window confirms, and the breach is
dispositioned as **D-perf-6**. **D-par-1**, **D-fid-1**, **D-perf-6**, **S31**–**S33**,
forcing rules v2 in force. Counts at `92d6fa75`; re-grep at each face's open (S24).

**Read first**: phase5.md §"The order the decomposition needs" (V re-derived
the closure at the face: `decode_slice.rs` calls 49 `unsafe fn`s in 10 modules, so
its done-test is the decoder's, and the view struct is W6's *last* step, not its
first).

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27**: V closed `OVERALL: PASS`/`FAIL(1)-all-F3` at exit → cheap
   subset if tools/toolchain unchanged. Last recorded: corpus output 2690/17,
   codes **2707/0**, conformance 60/60, decoder `raw_ptr` 974, `SHIM(` 52,
   deny-clean modules 6/22. Recount.
3. S32 on probes; S33 on every number; breadcrumb per face.

## 1. The order, and it is forced

Bottom-up by module. A module converts when **every function it exports stops
taking a raw pointer its callers must deref** — then its callers' calls are safe
calls, the module carries `#![deny(unsafe_code)]`, and the metric moves by
construction. The three recipes are unchanged (view struct, reference-flip,
plane/block-slice conversion); what V measured is which end to start from.

**The named list of families, in dependency order** (measured at V's head,
`unsafe fn` / raw-ptr occurrences):

| # | family | size | notes |
|---|---|---|---|
| ~~1~~ | ~~`fmo.rs`~~ | **DONE, T5.V4** | and it found F51 — `UninitFmoList` had no caller where the C++ has one |
| 2 | `picture.rs` | 5 / 8 | `pic_slot`/`same_picture` take `*const SPicture`; the tests that read padding through a raw pointer are **S28's mandated instrument** and are the module's enumerated exception |
| 3 | `decode_mb_aux.rs` | 6 / 13 | 4 `SHIM(phase2)` `extern "C"` shims installed into `pIdct*Func` tables — retiring them is a dispatch change, so this family is W6 step 3's, not a standalone |
| 4 | `cabac_decoder.rs` | 14 / 12 | 3 callees of `decode_slice`; `cabac_rbsp_window` lives here (step 4 of the decomposition) |
| 5 | `pic_queue.rs` | 7 / 41 | |
| 6 | `error_concealment.rs` | 15 / 32 | every export is `unsafe extern "C" fn (pCtx, pCurDqLayer)` |
| 7 | `parse_mb_syn_cabac.rs` | 34 / 45 | **15 of `decode_slice`'s 49** |
| 8 | `parse_mb_syn_cavlc.rs` | 27 / 68 | **14 of `decode_slice`'s 49** |
| 9 | `mv_pred.rs` | 22 / 115 | 3 of the 49 |
| 10 | `deblocking.rs` | 35 / 109 | 2 of the 49 |
| 11 | `manage_dec_ref.rs` | 24 / 57 | |
| 12 | `nalu.rs` | 23 / 67 | |
| 13 | `get_intra_predictor.rs` | 44 / 44 | **the 42 retiring SHIMs**; unblocks with its callers' plane pointers (step 3) |
| 14 | `decoder_core.rs` | 78 / 74 | |
| 15 | `decoder_context.rs` | 37 / 66 | the accessors and the view struct's home — **the enumerated exception**, converted last |
| 16 | `decode_slice.rs` | 59 / 202 | the view struct + the reference-flip, **after** 4/7/8/9/10 |

Total at V's open: **439 `unsafe fn`, 977 raw-pointer occurrences** over 16 modules; **430 / 974** after `fmo` landed. That is
the measured size of exit conditions 1–2, and it is not one session.

## 2. This session's scope — the queue, not a cap (steward's amendment)

The work queue is §1's list **entire**, entered in order. Families 2, 4, 5 and 6
first — no family stands in front of them (1 landed at V) — one commit each,
probe per seam, gate per commit; then 7 and 8, which unblock `decode_slice.rs`'s
CABAC/CAVLC calls and are half its cross-module surface; then straight down the
list. **A clean family boundary is a checkpoint, not an exit** (forcing rule 1;
S31 — compaction is not a stop). The session ends when the list ends or at
genuine exhaustion; the hand-off names the families remaining, sizes carried
forward.

Done-test per family: the module carries `#![deny(unsafe_code)]` **proved to bite**
(append a one-line `unsafe { *p }`, see the error, remove it), or its exceptions are
enumerated with a Phase pointer. Metric: decoder `raw_ptr` and deny-clean count,
both re-grepped at close.

## 3. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census. Probe
per seam (S32 beside any probe change). Full battery once at close. **Do not edit
the working tree while the battery runs.** F3 per S14 — V drew three hits across two batteries and adjudicated them; the ledger is current.

## 4. Close

Breadcrumb-built log entry (≤ 30 lines), phase5.md's family list marked, §0's
rows re-derived. This session's own span measured at close per S2b if code
landed — no other perf work (D-perf-6). Hand-off: the families remaining; if the
list ends, the next brief is the `decode_slice.rs`/view-struct endgame plus the
phase close.

## 5. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work (Phase 8's).
No F36 work. No `get_unchecked` (S8). No golden movement. **No perf work at all** —
D-perf-6 settled the disposition and the window is Phase 9's; the only measurement
owed is this session's own span at its close, per S2b, and only if code lands.
No re-opening settled designs.
