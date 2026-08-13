# Phase 5, session P — T5.P: the alias-clearing cluster, then 5.6

This session executes **W1–W5** of [`phase5.md`](phase5.md)'s closed checklist —
its done-tests are the face gates here. **D-fid-1 is in force**: the C++ is
reference, not template — functions may merge, split, and take Rust-shaped names
where that serves the safe design; output equivalence (goldens/sweeps) remains the
correctness definition, unchanged.

Governing: [`phase5.md`](phase5.md) verbatim; plan §7.4 (D-gate-1 and D-fid-1)
and §7.6 (S22 and S29 carry session O's new clauses; S20/S21/S24/S25/S28/S30 as
always); the session-O log entry and its hand-off — **its face order is this
brief's face order**. This file scopes the session and supersedes on disagreement;
fix disagreements in place. Counts measured at `11903757`; **re-grep at each
face's open** (S24 — mega-session rule).

**The class this session exists to clear, named**: *raw aliases into a container
block that container's ownership* (T5.N1's `pDec`, T5.O7's conviction). The
ordering rule is absolute — **aliases become ids/indices before their container
owns**. Faces drop from the end at seam boundaries if wall time runs out.

## 0. Start

1. Commit the inherited doc tail (the S22/S29 clauses, the Phase 8 inheritance
   note, this brief).
2. Open per **S27**: session O ended accepted. Cheap subset if `rust/tools/` and
   the toolchain are unchanged. Last recorded: **474 / 468 / 20**, Miri **334**
   (~942s), census **59**, goldens **57**, `raw_ptr` **4420**. Recount.
3. **No perf measurement** (D-gate-1). **Budget a probe run per container
   conversion, not one at close** — T5.O3 did and needed no second round; T5.O7/O8
   did not and the close battery took three rounds.

## 1. Face 0 — `pAccessUnitList` → `Option<Box<SAccessUnit>>`

53 sites; the head of the session because it is the **only** free-cascade entry
available — T5.O4/O7 boxed the nodes, so an owning `Box` at the access-unit level
retags nothing live. F19 per allocation; probe run before the next face.

## 2. Face 1 — the `pDec` step (before anything else structural)

**236 `.pDec` sites** (77 `decode_slice.rs`, 55 `decoder_core.rs`), **plus 94
`pRefList` and 209 `pRefPic`** → `PicId`/indices. This unblocks four things:
5.3's colocated split borrow, 5.3b, `Pool<Box<SPicture>>`, and 5.5's own `Drop`
teardown. S29 spelling throughout (including its new owned-local boundary);
S25 per function; probe runs at least per file converted.

## 3. Face 2 — the unblocked cascade

- `pDqLayersList`, `pPicBuff`, `pTempDec` become owned (their aliases died in
  face 1); the remaining decoder free-cascade entries die into `Drop` (R4).
- `Pool<Box<SPicture>>` + `mut_and_rest` — the split-borrow API 5.1 has owed
  since session N; the colocated `debug_assert!` (T5.N5) becomes the safe form.
- **The shell is not deletable** (session O's correction, at the site and
  phase5.md §5): the context is several MiB, `new_boxed` *is* the constructor —
  extend `make_zeroed_shell_valid` per owned field (S21), nothing more.
- Probe run per container.

## 4. Face 3 — colocated + 5.3b

`GetColocatedMb` onto `cur_and_ref`; `SetRectBlock`/`CopyRectBlock4Cols` onto the
grid; the remaining `LD*`/`ST*` punning (279 `mv_pred.rs` + 21
`parse_mb_syn_cavlc.rs` at last count — re-grep). S6 widths preserved; no window
hoisting (S8 #4).

## 5. Face 4 — P4, as a signature-narrowing closure

**208 `.pSps`/`.pPps` occurrences, nine files, four carriers** (context 134,
`SSliceHeader` 48, `SLayerInfo` 14) → active-paramset ids + lookup at use. The
S23 read is done and recorded (session O hand-off §3): the buffer is written on
the activation path, so **no lookup borrow outlives its expression** — F41 is
this exact question answered wrongly for `pParam`; do not repeat it.

## 6. Face 5 — 5.6

`decode_slice.rs` per P1: the EC MC paths; the `*mut u8` NZC cache family (167
uses, 96 in-file); F31's redundant memset; 5.2's straggler sweep;
`cabac_rbsp_window`'s retirement; the remaining decoder `SHIM(phase2)` adapters;
the 148-function signature leg (S25 per function, R1's field-precise rule);
`#![deny(unsafe_code)]` per module as each goes clean — the named exception is
`SPicture::data_ptr`'s output-contract consumer (Phase 8's, marker says so).
Fold into 5.2's straggler sweep: **F40's class, swept once crate-wide** — every
`copy_nonoverlapping`/`copy` whose count operand multiplies by an element size is
a candidate element-vs-byte confusion; F40 proved the class lives in this port,
and it is one grep plus one read per hit.

## 7. Gates — D-gate-1

Per commit (~3 min): build both profiles + tests + ratchet + census. Probe runs
per container/file as above (they are cheaper than the three-round close battery
they prevent). Once at close: full battery — goldens 57 frozen, sweeps 341/341
both profiles, benches bit-identical, Miri both probes; FAILs per S14/S16.
**Do not edit the working tree while the battery runs.**

## 8. Close

Log entry (per-face inventories, F19 answers, the punning count as found),
phase5.md marks and §0's rows, hand-off: whatever faces remain, else session Q —
**the exit, never compressed**, carrying every deferred measurement (session N's
stashed binaries, the niche verdict, the stop-line, the recovery/ledger
adjudication), the phase straggler sweep, and `prompts/phase6.md` per S19.

## 9. Non-goals

No perf measurement (D-gate-1). No F23/F38-class/F41/api/ work (Phase 8's — the
inventory note is in the plan). No F36 work (decoder threading's). No golden
movement. No pool/threading (F12/P10). No `get_unchecked` (S8). No shell
deletion (it is the constructor).
