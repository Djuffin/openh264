# Safe-conversion plan — Session S8: the pixel family, stage D's remainder, and the log context

*Everything you need is in this file. Re-run every count before quoting it — trust
the tree over this document. Before acting on any claim about the code, read the
lines it describes; a claim that something is absent gets its own grep before you
build on it. Every count you produce excludes comments or says that it doesn't —
this tree documents its own history so thoroughly that naive greps count the
documentation (a recent "56 call sites" was eleven; the other 45 were notes).*

## The project, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 video
codec (the C++ reference sits at the repo root under `codec/`). It ships as a
drop-in `libopenh264` replacement, so it must stay **byte-identical** to the C++ on
every stream the test harness runs — bit-exactness is stop-the-line: if a
differential sweep reports one divergent SHA after your change, the change is
wrong, no matter how much safer it looks. All undefined behavior has already been
eliminated; what remains is *conversion* — raw pointers and `unsafe` functions
that are sound but unnecessary, replaced with safe Rust. The end state is
`#![forbid(unsafe_code)]` in every file outside the C-ABI layer (`src/api/`), plus
exactly two audited `unsafe impl` lines that make the multi-threaded seam possible
(`Sync` for the reconstruction view, `Send` for the slice job handle).

Progress is one number: `#[allow(unsafe_code)]` attributes outside `src/api/`.
`bash rust/tools/safeplan_tracking.sh` prints it — 431 at the time of writing.

## Where the architecture stands (it changed last session — read this)

The two big shared structures have **already flipped to references**:

* The DQ layer passes as `&SDqLayer` (nine writer bodies keep raw by audit).
* The encoder context passes as `&sWelsEncCtx` / `&mut sWelsEncCtx` everywhere —
  the fork included. Zero bodies take `*mut sWelsEncCtx` as a parameter any more;
  the six surviving raw-context *sites* are five `ctx_*_raw` slot readers and the
  job handle's `*const` field, all audited to stay.

This works because every field the worker threads write is an atomic, a `Cell`
behind the audited seam, or separately-allocated storage — verified by the
compiler across all former raw bodies, and guarded by targeted Miri probes. The
rules that keep it true:

* **Never form a `&mut` to anything context- or layer-reachable inside the
  fork.** Under Miri's aliasing model, merely creating one is a data race even if
  nothing is written through it.
* **A shared `&T` claims the whole struct** — it races any concurrent write to
  any byte inside. If you add or move a field that workers write, it must join
  the atomic/`Cell`/boxed discipline, with a targeted probe.
* **A field-precise derivation held across a whole-struct reborrow gets popped**
  — the aliasing defect class Miri caught last session, *single-threaded*, after
  both differential sweeps had certified the change byte-identical twice. The
  three-step shape: (1) a `&mut`/`addr_of_mut!` into one field; (2) a later
  whole-struct `&T` or `&*ptr` — which pops step 1's tag; (3) a later use of the
  popped derivation. The fix is live-range narrowing: derive at the use, from the
  still-live parent. Read-only (`addr_of!`) derivations are immune — shared
  siblings coexist.

## Verification, sized to fit

Hour-scale runs are struck by the user's direction: the two full-encode threaded
Miri tests (`fork_join_encodes_*`, ~59 minutes as a pair) do **not** run in this
session — they wait for the exit battery. Small Miri runs of unit tests are the
ceiling. The regime:

* **Per checkpoint** (one checkpoint = one gated commit):
  `bash rust/tools/gates.sh commit` (~15–19 min), or `bash rust/tools/gates.sh
  family` for anything on the live encode path (adds the differential sweep, every
  config, both profiles, byte-compared against the C++).
* **A byte gate cannot see an aliasing bug that changes no byte.** Any checkpoint
  that converts a raw pointer into a reference or slice must also run the Miri
  lane: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (~20 min, capped).
  It has caught three such defects that the sweep certified as byte-identical —
  and Miri stops at the *first* error, so after fixing one, scan for siblings
  (the span scanner below) instead of trusting a green re-run.
* **For changes to data the worker threads share**, write a small targeted Miri
  test: hand-build the structure, spawn two threads doing what the workers do.
  Four examples sit in the tree (three in `svc_encode_slice.rs`, one arena test
  in `svc_motion_estimate.rs`). Two hard rules: run each probe's *control* — a
  deliberately-broken variant — and see it red before trusting a pass; and use
  enough iterations that the control actually fails (8 rounds was blind, 200 was
  a referee, in this tree's own history).
* **Run all tools from the crate root** (`rust/crates/openh264-rs`) — from
  anywhere else they match nothing and print a **green-looking zero**. Quote
  every checker result with its denominator ("0 violations / 106 bodies"), never
  as a bare zero.
* One gate at a time; no edits while a gate runs. A test failure that does not
  reproduce is re-run five times before any conclusion.
* Every remaining `unsafe` carries a `// unsafe-cat: …` tag; a tag comes off only
  with the unsafe it annotates.

## The steps

Numbered and ordered; each conversion step is one gated commit. Drop-from-the-end:
if you must stop, stop at a commit boundary and name everything not done. After
**every** conversion step: run the de-unsafe cascade (the converging form — strip
`unsafe` only from declarations whose signature carries no raw pointer, iterate,
revert what fails), then the span scanner, and seal any file whose last allow fell
(`#![forbid(unsafe_code)]`, leaf files only — a `forbid` in a `mod.rs` seals the
whole subtree). Counts below were measured at 431 the day this brief was written —
re-measure each before starting its step.

### Step 0 — commit the span scanner (instrument, before any conversion)

The three-step-span scan that found five aliasing defects last session exists only
in session history — an instrument in prose is not an instrument. Rebuild it from
finding F239's spec (`rust/docs/phase9_findings.md`): flag any binding derived
`&mut`/`addr_of_mut!` from a struct field that is still used after a later
whole-struct `&T`/`&*ptr` reborrow of the same struct. Commit it under
`rust/tools/`, and prove both directions: the clean tree reports **zero**, and the
recorded control — injecting the pattern into `WriteSliceBs` — reports exactly one
hit (`slice_multi_threading.rs:941`). No gate needed beyond `cargo test` on the
tool itself; it changes no product code.

### Step 1 — the log-context family, by value

19 `*mut SLogContext` mentions across six files (`encoder_ext.rs`,
`svc_enc_slice_segment.rs`, `au_set.rs`, `paraset_strategy.rs`,
`wels_encoder_ext.rs`, `common/wels_trace.rs`), ~63 call sites, one sink
(`WelsLog`, already a safe fn that copies the struct out). Convert **by value**
per the analysis above — never `Option<&SLogContext>`. Null spells as
`SLogContext::default()` (`pfLog: None`; `WelsLog` returns early on it, so a null
pointer and a default context are observationally identical today). Gate:
`family` — 63 sites cross the live path. Expected cascade: `au_set.rs` (10
allows) and `paraset_strategy.rs` (7) should empty or nearly; `wels_trace.rs`
drops to its irreducible `*mut c_void` core (step 9's subject).

### Step 2 — the entropy coder's last raw parameters

8 `*mut BsWriter` mentions: `svc_encode_slice.rs` 5, `svc_set_mb_syn_cavlc.rs` 2,
`nal_encap.rs` 1. The safe bit-writer exists and earlier landings already moved
the stash/position family onto references — this is the tail. Gate: `session`
(reference conversion). Expected cascade: `nal_encap.rs` (9 allows) and the two
`svc_set_mb_syn_*` files (7 + 8) close or nearly.

### Step 3 — coefficient blocks and DCT kernels

The `i16` family: `encode_mb_aux.rs` (12 allows; 13 `u8` + 4 `i16` raw mentions),
`svc_encode_mb.rs` (6; 8 + 17), `decode_mb_aux.rs` (3). These are
transform/quantisation kernels walking 4×4/8×8 coefficient blocks and their pixel
sources. Whole-table slices (never row-bounded — a stray-but-in-allocation read
must stay a read, not become a fresh panic); the biased-cursor form (table +
index, bias carried beside) where a pointer parks mid-table. Gate: `session` +
scanner pass.

### Step 4 — the mode-decision and motion-estimate land

Two sub-checkpoints, both `session` + scanner:

* **4a — the parameter roots**: `md.rs` (5 allows; 13 raw-`u8` mentions) and
  `svc_motion_estimate.rs` (17; 6) — plane-root parameters onto slices/cursors,
  same currency as step 3.
* **4b — the body campaign in the MD pair**: `svc_mode_decision.rs` (33 allows;
  only 3 raw-`u8` mentions — the blockage is *bodies*: bare dereferences and
  calls into whatever steps 1–4a haven't yet cleared) and `svc_base_layer_md.rs`
  (18; 1). Run the cascade first — much of these two may fall without hand
  edits once their callees are safe; convert by hand only what the cascade
  reports blocked on its own body.

### Step 5 — deblocking and the common pixel files

`encoder/deblocking.rs` (5 allows; 3 raw-`u8`), `common/mc.rs` (3),
`common/copy_mb.rs` (1), `common/deblocking_common.rs` (1). Small, finishes the
old D4. Note the two `mc`/`copy_mb` files live in **`common/`**, not `encoder/`
(this plan's own checkpoint table had that wrong). Gate: `session`.

### Step 6 — the body-deref campaign in the big single-threaded files

These files' allows sit on safe-looking signatures; the blockage is raw locals,
slot reads and dereferences inside bodies. Two sub-checkpoints:

* **6a — the camera path**: `ref_list_mgr_svc.rs` (27 allows — an earlier count
  found ~52 bare dereferences inside; its accessor edges are safe now) and
  `rc.rs` (22). Both sit near the project's historic flake — the five-times
  re-run rule applies to any non-reproducing failure here. Gate: `family`
  minimum, `session` if any reference conversion happens.
* **6b — the frame-loop files**: `encoder_ext.rs` (32), `wels_encoder_ext.rs`
  (24), `encoder_context.rs` (24 — this one holds the five `ctx_*_raw` slot
  readers, of which two are flippable per the fork-split tool's own listing:
  `ctx_src_pool_raw` and `ctx_vpp_raw`; the three in-fork ones stay). Gate:
  `family`.

### Step 7 — the slice core

The biggest single item: `svc_encode_slice.rs` (67 allows; 13 fns still carry a
raw parameter — five of them the `BsWriter`s step 2 takes), plus
`svc_enc_slice_segment.rs` (20) and `slice_multi_threading.rs` (23). The nine
layer-writer bodies stay raw by audit. Anything here that changes what worker
threads share gets its own targeted two-thread Miri probe, control seen red.
Likely two commits: the segment/slice-argument side, then the multi-threading
residue. Gate: `session` + scanner on every commit.

### Step 8 — preprocess and processing

`wels_preprocess.rs` (17) — including the move-memory pair
(`WelsMoveMemory_c`/`WelsMoveMemoryWrapper`): its recorded constraints are that
the source can be the caller's own C-ABI `SSourcePicture` buffer and that source
and destination may be the same picture. Price it at the tree; if the safe form
costs more than an audited allow at the ABI edge, leave it tagged with one line
of reasoning. Then the four `processing/` files (`background_detection.rs` 5,
`complexity_analysis.rs` 4, `adaptive_quantization.rs` 2, `vaacalc.rs` 1). Gate:
`family`.

### Step 9 — E1: decoder residue and the trace newtype

The decoder's 8 remaining allows (`picture.rs` 3, `pic_queue.rs` 2,
`decoder_core.rs`/`nalu.rs`/`decode_slice.rs` 1 each), then the trace callback:
wrap the user-supplied `*mut c_void` (`common/wels_trace.rs`, 2 allows) in a
newtype whose construction and invocation live only in `src/api/`, so `common/`
seals completely. Do this **after** step 1 — the by-value conversion reshapes the
same file. Gate: `family`.

### Step 10 — E2: the final flip

Only when steps 1–9 are done (drop-from-the-end boundary): delete `src/lib.rs`'s
crate-wide `allow(unused_unsafe, unsafe_op_in_unsafe_fn, …)`; reduce the census
allowlist to the api island plus the two `unsafe impl` lines; regenerate the
ratchet baseline and **pin it** — from here any new `unsafe` outside the island
fails CI. Verify the seal enforces: plant a `fn __seal_probe(p: *const u8)`
dereference in a sealed file, watch it rejected, remove it. Gate: `session`,
plus `cargo check --all-targets` clean.

### Step 11 — E3: the exit battery

`bash rust/tools/gates.sh exit` — ABI export list, dlopen harness, upstream
gtest against the known-failures ratchet, full Miri including the differential
tests **and the two full-encode fork probes deferred all along** (~59 min as a
parallel pair; run them via `rust/tools/fork_join_probe.sh`, which compares
against the baseline and warns past 1.3×), both-profile sweeps, and **the
benches**. The bench debt is sixteen checkpoints deep and is the largest single
risk left: measure same-machine both sides, two after-runs (first runs have
shown phantom ±3% swings), and if a regression is real, bisect over the
per-checkpoint commits rather than guessing. At the close, advance
`rust/tools/miri_wall_baseline.txt`'s data line — it still reads an older
session's close.

## Working rules that earned their keep (beyond the aliasing section above)

* **Ask the compiler, not a regex** — for writer classification, for the
  de-unsafe cascade, for call-site enumeration. The converging cascade form
  strips `unsafe` only from declarations whose signature carries no raw pointer;
  stripping declarations and blocks together diverges. Rerun the cascade after
  every conversion batch — it has retired as many allows as hand conversion.
* **In any tabulation you write, label rows measured or inferred** — in the last
  one, the measured section held exactly and *both* inferred entries were wrong,
  in opposite directions. Promote an inferred row to measured before executing
  on it.
* **Read the comments at the site you're converting** — one of those wrong
  inferences contradicted a ruled decision recorded in a comment the author had
  read past. The tree's comments carry rulings, not just history.
* **The prohibition checker counts comments** and prints a green-looking zero
  when run from the wrong directory — denominators, always.

## Findings and the report

Findings live in `rust/docs/phase9_findings.md`, appended and numbered; yours
start at **F247**. A blocker that needs the user's ruling becomes a finding and
stops that checkpoint. At the close, add the session's row to **both** tables in
`rust/docs/safe_conversion_execution_plan.md` — the session map *and* the dated
log table — and advance the Miri baseline file.

Report back in plain prose: per-checkpoint commits with gate verdicts; the span
scanner's results per batch; any probes added, each with its control seen red;
the tracking number's movement; every place this brief was wrong, quoting the
sentence; and a roll-forward line naming **everything** owed — checkpoints,
instruments, benches, findings alike.
