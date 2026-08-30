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

## Two opening duties (instruments, before any conversion)

1. **Commit the span scanner as a tool.** The three-step-span scan that found the
   five aliasing defects exists only in session history — an instrument in prose
   is not an instrument. Rebuild it from the description above (finding F239 in
   `rust/docs/phase9_findings.md` has the full spec), commit it under
   `rust/tools/`, and prove both directions: the clean tree reports **zero**, and
   an injected pattern (the recorded control: planting one in `WriteSliceBs`
   produced a hit at `slice_multi_threading.rs:941`) reports **exactly it**. Run
   it after every conversion batch this session — the pixel family below is made
   of exactly the derivations it watches.
2. **The Miri lane baseline is stale.** `rust/tools/miri_wall_baseline.txt`'s
   data line still reads an older session's close. At your close, quote your
   lane's CPU seconds against it and replace the data line.

## What S8 does, in order (drop-from-the-end: if you must stop, stop at a commit
boundary and name everything not done)

### The pixel family — the largest remaining block

Of the remaining allows, the biggest tractable class is raw parameters whose
pointee is pixels or coefficients: **53 `*mut u8` plane roots and 10 `*mut i16`
coefficient blocks** at the last census (re-measure; the census method: classify
every allow by the full signature of the item it heads). They concentrate in
`encoder/deblocking.rs`, `mc.rs`, `copy_mb.rs` (motion compensation and copies),
`encode_mb_aux.rs` (DCT kernels), and the mode-decision/ME files. The conversion
currency already exists in the tree — the intra-prediction families take
`&mut [u8; N]` + `&RecCursor`, and the MVD-cost conversion shows the
biased-cursor form (a table plus an index, bias carried beside it).

The three rules that own this land:

* **Two raw cursors must come off ONE derivation** — if a body takes `&mut [u8]`
  and needs raw cursors inside, call `as_mut_ptr()` once and derive everything
  from it; a second call pops the first cursor.
* Every checkpoint here is a reference conversion → **Miri lane per checkpoint**,
  plus a span-scanner pass.
* A stride-based walk that C bounds by convention becomes a slice index that Rust
  bounds by panic — **keep the slice the whole allocation** (plane, table), not
  the row, so a stray-but-in-allocation read stays a read and not a new panic on
  the first stream that produces it (this exact choice was made for the MVD
  table; keep making it).

### Stage D's remainder

* **`BsWriter`'s six sites** — the last of the entropy coder's raw parameters.
* **`svc_encode_slice.rs` / `svc_enc_slice_segment.rs` remainder** — re-measure
  what is left after both flips; the blockers now are body dereferences and the
  files' own raw locals, not parameters.
* **`slice_multi_threading.rs` residue** (~25 sites at last count) — fork
  plumbing; anything that changes what workers share gets a targeted probe.
* **The move-memory pair** (`WelsMoveMemory_c` / `WelsMoveMemoryWrapper` in the
  preprocess area): its recorded constraints are that the source can be the
  caller's own C-ABI `SSourcePicture` buffer (raw by ABI) and that source and
  destination may be the same picture. Price it at the tree; if the safe form
  costs more than an audited allow at the ABI edge, leave it tagged with one
  line of reasoning and roll it to the exit list.

### The log-context family (analysed last session; the shape is decided)

Eighteen raw `*mut SLogContext` parameters funnel into one sink, `WelsLog` —
which is *already a safe fn* that null-checks and **copies the struct out**
before calling the application's callback. The copy is deliberate and documented
in the function: a tracing callback may re-enter the codec, and a live borrow
across that re-entry would alias.

**Convert by value, not `Option<&SLogContext>`**: the struct is `Copy` and four
words; a by-value parameter holds no borrow for a re-entrant callback to collide
with, where `Option<&>` would put a live borrow in all 63 calling frames across
a path no test exercises (nothing installs a re-entering callback). Null already
has a spelling — `SLogContext::default()` has `pfLog: None` and `WelsLog`
returns early on it, so a null pointer and a default context are observationally
identical today. 18 parameters, 63 call sites.

### The close (roll forward whatever doesn't fit)

* **E1 rest**: decoder residue (re-measure `decoder/picture.rs` and friends —
  largely retired); wrap the trace callback's user-supplied `*mut c_void` in a
  newtype whose construction and invocation live only in `src/api/`, so
  `common/` can seal.
* **E2**: seal each file as its last allow falls (leaf files only — `forbid` in
  a `mod.rs` seals the whole subtree); then delete `src/lib.rs`'s blanket
  `allow(unused_unsafe, unsafe_op_in_unsafe_fn, …)`, reduce the census allowlist
  to the api island plus the two `unsafe impl` lines, pin the ratchet at the
  floor.
* **E3 — the exit battery**: `bash rust/tools/gates.sh exit` — ABI export list,
  dlopen harness, upstream gtest, full Miri including the differential tests
  **and the two full-encode fork probes deferred all along**, both-profile
  sweeps, and **the benches**. The bench debt is sixteen checkpoints deep and it
  is the largest single risk left in the plan: if a bench regresses, every
  checkpoint is its own commit — bisect over them rather than guessing, and
  measure same-machine both sides with two after-runs (first runs have shown
  phantom ±3% swings).

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
