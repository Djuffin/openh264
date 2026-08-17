# Phase 5b — the decoder goes safe, one session

**Eugene, 2026-08-17: "I want next session to focus on making decoder safe…
I see no reason why this can't be finished in one session."** This session takes
`src/decoder/` from 167 `#[allow(unsafe_code)]` items / 160 unsafe blocks /
42 `unsafe fn` to **four named FFI items and nothing else**. Phase 6 (the
encoder) waits until this lands. **D-par-1**, **D-fid-1**, **D-perf-6**,
**S31**–**S33** in force. Counts at `6c3e7301`; re-grep at each face's open
(S24 — code/prose split always).

## The terminal rule (unchanged from AC, where it closed a phase)

**This session has no hand-off.** It ends when the done-test at the bottom reads
met and the close lands. A size is never a stop (every face below is measured; the
list is finite; S31 — compact, re-read this brief, continue). A question is never
a stop — the **decision ladder**: (1) settle by reading, write the settlement in
the log, proceed; (2) lint-scope questions default to the enumerated-exception
shape with a pointer; (3) behavior questions **never default** — park exactly that
item with a one-paragraph record and continue; parked items join the close's
stated list. A revert is a checkpoint: fix the blocker, re-attempt in-session
(the flip took three sessions because its blockers were unmeasured; yours are
measured, in this file). The only early end is a named Eugene/steward blocker
that survives all three rungs — that set should be empty.

## What "safe" means here, exactly

`#![deny(unsafe_code)]` stays on all 22 modules. The allow list shrinks to **four
items, named**: `api_alias` + `api_alias_mut` (`decoder_context.rs` — they deref
into `CWelsDecoderImpl`-owned objects; only Phase 8's ownership rewire dissolves
them) and `SPicture::data_ptr` + `data_ptr_ref` (`picture.rs` — the output
contract handing plane pointers across the C ABI). These four are the FFI
exception Eugene named. Every other allow, block, and `unsafe fn` goes.

## 0. Start

1. Commit the inherited doc tail (this brief, the plan note).
2. Open per **S27** (AC closed `OVERALL: PASS` 13/0/1): build both profiles +
   `--all-targets`, tests, ratchet, census. Recount: allow items **167**, unsafe
   blocks **160**, `unsafe fn` **42**, raw_ptr **236 = 173 code + 63 prose**.
3. **Probes budgeted to fire** — face 0 is aliasing-sensitive by construction.
   S32 beside any probe change; S33 on every number; breadcrumb per face.
4. **The guard that cannot move**: corpus **2690/17 + 2707/0**, conformance
   60/60, benches bit-identical, goldens untouched. Parity is the definition of
   correct (D-par-1); a face that moves any of it is wrong, not bold.

## 1. Face 0 — `PPicture`/F42: the Same arm becomes a type (23 items)

The one design face, and its type already exists: `PicRefs::classify` (T5.AB3)
answers Same-vs-Distinct. Execute:

1. The MC/EC entry points that today take a raw current-picture take the
   classification: `Distinct(&mut dst, &src)` runs the existing kernels;
   `Same(&mut pic)` runs a within-one-borrow variant (reads and writes through
   the single `&mut` — `copy_within`'s shape where copying, index-based where
   interleaved). The Same arm is **cold** — malformed streams only — so its
   spelling is chosen for soundness, not speed.
2. F42's covering test is the referee: it stays green through the conversion and
   red under a revert of the arm. The `p3_slice_copy_self_copy_guard` test
   guards the identity case — AB's `RefSlot` lesson says do not "simplify" it.
3. The 23 survivor items convert; their `#[allow]`s and the F42 argument at each
   item delete; `PicRefs::get`/`get_mut` return borrows everywhere.
4. **This supersedes the Phase 8 option-1/2 revisit** — record its closure in
   the findings and the owner table.
5. Probe run per seam — this face is what the probe budget exists for.

## 2. Face 1 — the parse tree (~50 sites: `decoder_core` 26, `nalu` 22, strays)

The nodes have been `Box`-owned in the AU's `Vec` since T5.O4; the raw
`PNalUnit`/`PSliceHeader` params are legacy threading. Cache-not-carrier, nth
application: indices into the AU where a function stores or compares, borrows
where it reads — the slice header is a field of the NAL, so one borrow serves
both. The parse-only output path (`SParserBsInfo`, API type) resolves at the api
boundary and is not this face's to convert. Byte-identical per commit.

## 3. Face 2 — `sMCRefMember` (2 `mem::zeroed` sites + helpers)

The C++'s MC descriptor becomes the `PlaneCursorMut` vocabulary the
reconstruction dispatch already uses (T5.AA2): cursors + strides built from the
resolved pictures at the two construction sites (`decode_slice.rs:1453`,
`:1659`), the zeroed init dying with the raw fields. S6 widths; F35 alignment.

## 4. Face 3 — the shells, measured first

1. **Measure `size_of::<SWelsDecoderContext>()` before choosing the recipe**
   (one test, printed). Phase 5 moved the bulk into owned containers; if the
   struct is now plain-constructible (≲ hundreds of KB), the
   `MaybeUninit::zeroed` shells (`decoder_core.rs:534`,
   `decoder_context.rs:1873`) become explicit field-wise constructors — F54's
   class retired permanently. If it is still MiB-scale, **Box the remaining
   inline arrays first** (which is the enabler, and wanted anyway), then
   construct plainly.
2. `nalu.rs`'s two `MaybeUninit` temp stores (`:1600`, `:1934`) convert with
   whatever the paramset types' constructors give; if a full initializer is
   disproportionate, rung 2 of the ladder applies (enumerated exception with a
   pointer) — but try first.
3. S21's question at every field: what does the all-zero pattern *mean* now.

## 5. Face 4 — the sweep, then the close

1. **Strip-and-build, whole decoder at once** (session Z's rule — per-module
   runs lie): every `unsafe fn` keyword stripped, the compiler keeps what is
   load-bearing; target **0**, because the four FFI items are safe signatures
   with internal blocks. Every remaining allow beyond the four is a bug in this
   session's own work — hunt it.
2. Done-test, all by grep, code/prose split stated:
   - `#[allow(unsafe_code)]` in `src/decoder/` = **4**, at the four named items;
   - `unsafe fn` in `src/decoder/` = **0**;
   - every `unsafe {` block lives inside the four items;
   - corpus 2690/17 + 2707/0, conformance 60/60, unmoved.
3. Full battery at `exit` level; F3 per S14 (step 0's hash shortcut is
   profile-dependent — AC's clause). Span per S2b, base `ac_head` — **no other
   perf work** (D-perf-6). Face 0's Same arm is cold; the span should read
   flat, and if it does not, the mechanism is named as a candidate, not
   claimed (S33).
4. The close: log entry from breadcrumbs (≤ 30 lines); phase5.md gains the
   Phase 5b addendum row (this session, its counts, the four-item list);
   `phase6.md`'s starting position refreshed if any number it quotes moved; §0
   refreshed; the findings' owner table updated (the option-1/2 revisit closed
   here; everything else unchanged).

## 6. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam. Full battery once at close. **Do not edit the working tree while
the battery runs.**

## 7. Non-goals

No encoder sites (F12/P10 — Phase 6's; the C++-home relocation rule stands). No
`api/` internals (Phase 8's — the four items *are* the boundary marker). No F36
work. No `get_unchecked` (S8). No golden movement. No perf work beyond the span.
No interior mutability on the planes (rejected at AB's ruling; face 0's design
makes it unnecessary). No re-opening settled designs.
