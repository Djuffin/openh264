# Phase 5, session V — the escalation answer, then W6

**Phase 5 did not close at session U.** Exit conditions 1–3 are unmet (decoder
`raw_ptr` 980; 3 of 22 modules deny-clean; `SHIM(` 52 against a survivor list of
1) and condition 5 **escalated** rather than reconciled: the stop-line is
breached. This session answers the escalation and then starts W6 for real.
**D-fid-1**, **D-gate-1**, **S31**–**S33**, forcing rules v2 in force. Counts at
`e6873fe1`; re-grep at each face's open (S24).

**Read first**: `perf_baseline.md` §Phase 5 exit (the escalation table and its six
options), and phase5.md's W6 row (U re-derived the size; the settlement was out by
4.5x).

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27**: U closed `OVERALL: PASS` at `exit` → cheap subset if
   tools/toolchain unchanged. Last recorded: 482/476/20, census 59, sweeps
   341/341 both profiles, decoder `raw_ptr` **980**, `SHIM(` **52**, deny-clean
   decoder modules **3**. Recount.
3. S33 on every number written.

## 1. Face 0 — day two, before anything else and before any code lands

The stop-line verdict rests on **one day's** reading, and this phase has twice had
a second day overturn a first (session N's bisect; the niche's asymmetry). The
binaries are stashed; **no build is needed**.

1. **7-pair null**, same day, same machine (S2b: the null runs at the verdict's
   pair count — U measured 2.56 points wide at 7 pairs against 0.22 at 3).
2. **The window at 7 pairs**: `n_head` (`d0b7f399`) → `u_head` (`e6873fe1`).
   U read **+2.77% median / +3.58% CB**.
3. **The niche at 3 pairs**: `n_head` → `o_niche` (`74d02058`). U read CB +0.15%
   (the control, flat), Main −0.72%, High −0.99%, inside the floor.
4. **Not** the bisect halves — they are at the resolution limit and a second day
   buys a second unresolvable answer (session K's law; U's halves already
   disagree with the whole).
5. Write the verdict either way, then **take the escalation table to Eugene** with
   both days on it. If the breach survives, the decision about options 1/4/5/6 is
   Eugene's, not this session's.

**No code lands before this face closes** — the window's endpoint must stay
`e6873fe1` or the reading is not a second day of the same thing.

## 2. Face 1 — W6, sized honestly, and it is not one face

The settled design is unchanged (phase5.md §Session S's two settlements: one
per-slice view struct, one `unsafe` constructor per bracket top, in
`decoder_context.rs`). What changed is the size. Measured at `1423f8eb`:
`decode_slice.rs` holds **202 raw-pointer types over 55 `unsafe fn`**, and
`deny(unsafe_code)` fires on every one — the view struct addresses **44**.

Take **one seam per session** and commit it green. In dependency order:

1. **The view struct** (44 `*mut SWelsDecoderContext`, 80 `(*pCtx)` derefs over 29
   fields, 26 functions dereferencing and 18 passing through). `&mut` for the
   raw-data reader, the CABAC engine, the flag/counter set; `&` for tables and
   config; copied scalars where S23 clears them — **verify constancy per field and
   log the check**; `pParam`'s scalars copied inside the constructor so F41's raw
   field never escapes it.
2. **`*mut DqLayerState`** — 51 types, the largest remaining carrier.
3. **The plane/block pointers** (`*mut u8` 17, `*mut i16` 11, `*mut u32` 10) —
   these are what the 42 `get_intra_predictor.rs` SHIMs exist for, so W7's
   52 → 1 unblocks here and nowhere earlier.
4. **`cabac_rbsp_window`'s retirement** — 18 call sites, one per function, 72
   occurrences with their callers; the window is constant across a slice
   (`BsCursor::len` is the logical end, set at init, not the remaining bytes —
   verified at U), so it is the bracket maneuver, threaded from the slice bracket
   top through the per-MB dispatch. W3's-hoist scale on its own.

Probe per seam (S32: probe count is the budget knob). Done-test per seam is the
seam's own greps; **`decode_slice.rs` deny-clean is the item's done-test, not any
one session's.**

## 3. Face 2 — W7's remainder, behind W6

5.2's straggler sweep; `SHIM(` 52 → 1 named survivor (`data_ptr`'s
output-contract consumer, Phase 8's) — **42 of the 51 are in
`get_intra_predictor.rs` and retire with face 1 item 3, not before**;
`#![deny(unsafe_code)]` per decoder module as each becomes clean, exceptions
enumerated with their Phase pointer.

## 4. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam, S32's arithmetic beside any addition. Full battery once at
`exit` at close; F3 per S14. **Do not edit the working tree while the battery
runs**, and **do not build while a perf pair runs**.

## 5. Close

Log entry from breadcrumbs (≤ 30 lines), phase5.md marks, §0 refreshed, hand-off
one ahead. `phase6.md` is written at the phase's **exit**, not before (S19) —
Phase 5 has not exited.

## 6. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work (Phase
8's). No F36 work. No `get_unchecked` (S8). No golden movement. No re-opening
settled designs — the view struct's *design* is settled; only its size moved. No
code before face 0 closes.
