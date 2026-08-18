# Phase 5b, session C — the shells, the sweep, the four-item close

Runs after session B lands the parse tree at 60/60. This session ends Phase 5b:
**allow items → 4, `unsafe fn` → 0**. **D-par-1**, **D-fid-1**, **D-perf-6**,
**S31**–**S33**, the terminal rule and the decision ladder in force. Counts at
session B's close; re-grep at each face's open (S24, code/prose split).

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27**: build both profiles + `--all-targets`, tests, ratchet,
   census. Recount allow items, blocks, `unsafe fn`, raw_ptr.
3. Probes per seam; S33 on every number; breadcrumbs.

## 1. Face 0 — the shells, with session A's correction honored

`SSps::default()`/`SPps::default()` are **not all-zero** (bit depths 8,
`bFrameMbsOnlyFlag`, `uiNumSliceGroups` 1, `iPicInitQp` 26) — measured, so
"replace the shell with `Default`" is a **wrong recipe**. The right one:

1. Measure `size_of::<SWelsDecoderContext>()` first (the 5b gate, still
   unrun). If plain-constructible, the two `MaybeUninit::zeroed` shells
   (`decoder_core.rs:534`, `decoder_context.rs:1873`) become **explicit
   field-wise constructors whose values are the C's `memset` semantics** —
   every field's all-zero *meaning* written out (S21/F54), which is exactly
   what `Default` gets wrong. If still MiB-scale: Box the remaining inline
   arrays first, then construct plainly.
2. `nalu.rs`'s two `MaybeUninit` temp stores (`:1600`, `:1934`): same
   treatment; ladder rung 2 if a full initializer is disproportionate — try
   first.

## 2. Face 1 — the sweep, then the Phase 5b close

1. **Strip-and-build, whole decoder at once** (Z's rule): target `unsafe fn`
   **0**. Every allow beyond the four named FFI items (`api_alias`,
   `api_alias_mut`, `data_ptr`, `data_ptr_ref`) is this session's bug — hunt
   it. Done-test by grep: allow = **4** at the named items; `unsafe fn` = 0;
   every `unsafe {` inside the four; corpus 2690/17 + 2707/0 and conformance
   60/60 unmoved.
2. Full battery at `exit` level; F3 per S14.
3. **The 5b window span** per S2b: base `ac_head` (`6c3e7301`) → this
   session's last code commit — sessions A+B+C are the window. No other perf
   work (D-perf-6).
4. The close: log from breadcrumbs (≤ 30 lines); phase5.md's 5b addendum
   updated with the final counts and the four-item list; `phase6.md`'s
   starting numbers refreshed if any moved; §0; the findings' owner table
   (the Phase 8 revisit stays closed; any new F from session B carries its
   owner).

## 3. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam. Full battery once at close. **Do not edit the working tree
while the battery runs.**

## 4. Non-goals

No encoder sites (F12/P10). No `api/` internals (the four items are the
boundary). No F36 work. No `get_unchecked` (S8). No golden movement. No perf
work beyond the span. No `Default` for the shells (measured wrong). No
re-opening settled designs.
