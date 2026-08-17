> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session Q — the flip through the instruments: W3's flip + W4–W7

*(Naming: primes retired — the letter sequence resumes after P″. In older docs "Q"
was the exit's forward name; the exit is now session **S**.)*

This session executes **everything left of Phase 5 except the exit**: W3's flip,
then W4, W5, W6, W7 of [`phase5.md`](phase5.md)'s closed checklist. **D-fid-1**
and **D-gate-1** in force. Counts at `40feda32`; re-grep at each face's open
(S24 — directory *and unit*).

## The forcing rules (Eugene, 2026-08-14)

Sessions P, P′ and P″ each closed with scoped faces unstarted. The causes are
fixed where they can be (designs now settle in writing before the session; S24
carries the unit clause) and ruled against where they can't:

1. **A clean seam is a checkpoint, not an exit.** After every commit, open the
   next face immediately. The session ends at context exhaustion or at a blocker
   only Eugene or the steward can clear — never because a boundary was tidy. If
   faces remain at close, the hand-off names which of those two reasons applied.
2. **A reverted attempt whose blockers are settled is open work, not done work.**
   P″ reverted the flip and settled its six facts; face 0 is that re-attempt.
3. **Close-out is budgeted: ≤ 30 log lines + marks + hand-off.** The commits are
   the record (D-fid-1 retired provenance prose). A mid-session settlement is one
   log line, not an essay — the steward reconciles at close.
4. **Gate economy per D-gate-1 exactly**: cheap set per commit; probe per seam
   (keep it — it is what prevents three-round close batteries); **one** full
   battery, at close. No mid-session full batteries.
5. **Dropping a face requires rule 1's reason, written down.** Drop from the end
   only.

What the rules do **not** force: probe discipline, byte-exactness per commit, and
revert-at-the-seam stay absolute. P″'s revert was correct; closing instead of
re-attempting was the underdelivery.

## 0. Start

1. Commit the inherited doc tail (this brief, the mapping change).
2. Open per **S27**: P″ closed `OVERALL: PASS` at exit level → cheap subset if
   tools/toolchain unchanged. Last recorded: 474/468/20, Miri 334, census 59,
   goldens 57, decoder `raw_ptr` **1229**. Recount.
3. **Probe runs budgeted to fire** — this session makes the phase's largest
   semantic change. Probe per seam; a conviction is fixed by ordering (S29),
   not by widening.

## 1. Face 0 — the flip

§W3's six facts are **compiler-settled** (log §3 has the reasoning). Execute,
don't re-derive. Order inside the face — API prep lands and compiles *before*
the atomic seam:

1. Prep under `PPicture` slots: `PoolRest` gets a **hand-written `Copy`**;
   `PicRefs::get` returns **`*const SPicture`**; `SPicture::data_ptr` gains a
   **`&self` form** for `GetRefPic`'s three reference-side calls;
   `prefetch_free`/`next_for_thread` return **`Option<PicId>`**.
2. The atomic seam, one commit: `Pool<PPicture>` → `Pool<Option<Box<SPicture>>>`.
   `alloc_picture` feeds `Pool::replace`; `CreatePicBuff`'s partial-failure arm
   becomes the `Vec` going out of scope; `DestroyPicBuff` becomes **F37's reset
   plus a `drop`** (adjudicated here, test per its record); R4 by construction;
   F19 closes decoder-side. The four accessors are **deleted** — done-test:
   greps read 0; direct pool-field reads enumerate to bracket sites only.
3. The **83 write-path sites** (decoder_core 38, manage_dec_ref 32, EC 10,
   decode_slice 3 — re-grep): read for **span**, with the discriminator —
   **shared gets coexist freely; only a `&mut` span or a `replace` under a live
   borrow conflicts.** Per-use resolution survives wherever the result dies
   inside its expression. Known span-crossers: EC's ref→cur copies take
   `mut_and_rest`; the AU-start prefetch→stamp→list-build is consecutive
   expression-scoped borrows, not one span.
4. **The current-slot rule** (§W3 fact 2): a malformed stream can put the
   picture being decoded into a reference list; the C aliases and reads on.
   Resolve the current slot **through the mutable picture's own pointer**
   (`PoolRest::get(cur)` panics) — one borrow, S6 kept. **Record as the next
   F-number with a covering test**: T3.0's table first; else construct the
   stream; red-under-revert per F21.

## 2. Face 1 — `pDqLayersList` owns; `pCurDqLayer` was a cache

W2b's recipe, third use: one production stamp (`decoder_core.rs:3578`); the call
tree already threads the parameter. Delete the field; derive once at the loop
top (S29 spelling); the **81 mid-tree ctx-field reads** become param sites.
Done when `WelsMallocz|WelsFree` in `src/decoder/` reads **0** and the cascade
functions are deleted — **W3 closes**.

## 3. Face 2 — W4: colocated + 5.3b

`GetColocatedMb` takes cur and ref (both resolved at the slice bracket); T5.N5's
`debug_assert!` becomes the type-level fact. `SetRectBlock`/`CopyRectBlock4Cols`
onto the grid; punning → byte ops (**325 `LD*/ST*` tokens** — re-grep). S6
widths; no window hoisting (S8 #4). Done when the decoder LD/ST grep reads 0.

## 4. Face 3 — W5: P4

`pSps`/`pPps` → active-paramset ids + lookup at use (**205 occurrences, 4
carriers**). The S23 read is done (session O §3): no lookup borrow outlives its
expression — F41 was this answered wrongly; do not repeat it. Done when
`.pSps|.pPps` greps read 0.

## 5. Face 4 — W6: `decode_slice.rs` goes clean

Per P1: the EC MC paths; the NZC `*mut u8` cache family (~167 uses, re-grep);
F31's redundant memset; `cabac_rbsp_window`'s retirement; the signature leg —
**D-fid-1: functions may merge; 148 is an upper bound, not a target**. Done when
`decode_slice.rs` compiles under `#![deny(unsafe_code)]`.

## 6. Face 5 — W7: closure of the instruments

5.2's straggler sweep; **F40-class sweep crate-wide** (every
`copy_nonoverlapping`/`copy` whose count multiplies by an element size — one
grep, one read per hit); decoder `SHIM(phase2)` + `SHIM(phase5)` → **1 named
survivor** (`data_ptr`'s output-contract consumer, Phase 8's);
`#![deny(unsafe_code)]` on every decoder module, exceptions enumerated with
Phase 8 pointers. Done when SHIM greps match the survivor list exactly.

## 7. Gates — D-gate-1

Per commit (~3 min): build both profiles + tests + ratchet + census. Probe per
seam. Once at close: full battery — goldens 57 frozen, sweeps 341/341 both
profiles, benches bit-identical, Miri both probes; F3 per S14 (running total
42/14/21). **Do not edit the working tree while the battery runs.**

## 8. Close

Log entry (≤ 30 lines: per-face one-liners, the bracket-site list, the new
finding, F19/F37 answers), phase5.md strike-throughs and §0's rows, hand-off:
whatever remains per forcing rule 1, else **S = W8 — the exit, never
compressed** (session N's stashed binaries, the niche verdict, the stop-line,
the recovery/ledger adjudication, `prompts/phase6.md` per S19).

## 9. Non-goals

No perf measurement (D-gate-1). No encoder sites (F12/P10). No
F23/F38-class/F41/`api/` work (Phase 8's). No F36 work. No golden movement. No
`get_unchecked` (S8). No shell deletion. No re-opening settled designs — §W3's
six facts are compiler-settled; a tree disagreement is fixed in place and
logged, not re-litigated.
