# Safe-conversion plan — Session S11: the bitstream seam, the conversion mass, and the enumerated floor

*Everything you need is in this file. Re-run every count before quoting it — trust
the tree over this document. Before acting on any claim about the code, read the
lines it describes; a claim that something is absent gets its own grep. Every count
excludes comments or says that it doesn't. Before honoring any recorded deferral or
refusal, re-verify its **premise** against today's battery — a deferral's premise
expires, not just its conclusion.*

## The project, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 video
codec (the C++ reference sits at the repo root under `codec/`). It ships as a
drop-in `libopenh264` replacement, so it must stay **byte-identical** to the C++
on every stream the test harness runs — bit-exactness is stop-the-line. What
remains is *conversion*: raw pointers and `unsafe` that are sound but
unnecessary. The end state: `#![forbid(unsafe_code)]` in every file outside the
C-ABI layer (`src/api/`), except `rec_view.rs` (deny + exactly one product allow —
the tree's **single remaining `unsafe impl`**, `Sync for SharedCells`) and the
~17 files whose *test modules* carry Miri/provenance instruments, which take
`deny` with each test allow enumerated in the pinned census (decision D-exit-4).
So the tracking number's honest floor is the enumerated instrument set (~39–41),
not zero.

Progress: `#[allow(unsafe_code)]` outside `src/api/` —
`bash rust/tools/safeplan_tracking.sh` prints it. **327** at the time of writing.
By tag census that is ~136 `port-raw` + ~125 `fork-shared` (the convertible
mass), 39 `instrument(test)` (stay, enumerated), 23 `SCREEN_CONTENT(dormant)`
(convert this session, ruling D-scope-6), ~4 audited residue.

## The architecture, current

* **`sWelsEncCtx` is `Sync`.** The job handle carries `&'a sWelsEncCtx` across
  the spawn; `Send` is compiler-derived. One `unsafe impl` exists in the whole
  tree (`rec_view.rs:152`). Do not add a second — if a type needs `Send`/`Sync`,
  make its fields qualify, the way S10 did.
* **The DQ layer passes as `&SDqLayer`**; the source picture's planes ride the
  shared seam (`SharedPlane`) because background detection writes them in-fork
  under the default configuration — never a bare `&[u8]` over a source plane.
* **Never form a `&mut` to anything context-, layer-, or source-picture-reachable
  inside the fork** — creating one is a race under Miri's model even unwritten.
  A worker-written field belongs in an atomic, a `Cell` behind the seam, or a
  per-worker disjoint range proven by `split_at_mut`.
* **A field-precise `&mut` derivation held across a later whole-struct reborrow
  gets popped** — single-threaded, sweep-invisible.
  `python3 rust/tools/f239_span_scan.py` detects the shape; run it after every
  batch.
* **When a raw exists to dodge the borrow checker, ask which borrow rule** —
  S10's most reusable result. Width (a whole-struct borrow taken to read one
  field → narrow the parameter to the field), or exclusivity (callers whose
  receiver is already `&mut` cannot coexist with a live fork → safe twins for
  the single-threaded majority). Classify call sites by whether a fork is
  genuinely involved before designing anything; both prior seams split ~4:1
  single-threaded.

## Verification, sized to fit

* Per checkpoint: `bash rust/tools/gates.sh commit` (~15–19 min) or `family`
  (adds the differential sweep) for live-path changes. Reference/slice
  conversions also run `MIRI_SCOPE=encoder bash rust/tools/gates.sh session`
  (~20 min) — it has caught four defects the sweeps certified.
* Worker-shared data changes get a targeted two-thread Miri probe, hand-built
  without an encoder, its control seen red at a calibrated iteration count (a
  calibration is a property of the probe — measure it, don't inherit another
  probe's number). Five examples in the tree.
* After every batch: `python3 rust/tools/deunsafe_cascade.py` (converging
  form), then the span scan, then seal any file whose last product allow fell
  (leaf files only).
* Tools run from the crate root; every checker result carries its denominator;
  after feeding a baseline file, verify the reader consumed it (the Miri
  baseline is **newest-first — prepend**).
* One gate at a time; non-reproducing failures re-run five times; tags come off
  only with their unsafe.
* **No benches this session** (ruling D-gate-8: the whole bench debt clears at
  E3, reaffirmed by the user with S10's 7% catch known — do not add one).

## The steps (drop-from-the-end; each one gated commit; name everything not done)

### Step 0 — S10's close-out debts, if not already paid (check first)

The previous session closed owing: the Miri-lane ratio (last gate read **2.16**
against the 1.3 tripwire; a fix was applied but never re-measured — one
`session` gate run settles it; prepend the baseline with the settled number and
file the finding either way), findings **F264 onward** for eleven checkpoints
whose reasoning lives only in commit messages, and both plan tables (session
map + dated log), which still describe the session's first close. Also three
hygiene items: one stale `send-seam` tag survives its deleted impl (find it;
re-justify or remove), and two tags are spelled `SCREEN_CONTENT(dormant` —
missing close-paren. **Verify what of this is already done before redoing any
of it** — the tree may have moved.

### Step 1 — the bitstream seam (the last structural item)

`slice_bs_buffer` hands each worker a `&mut [u8]` carved out of the shared
context — 32 mentions across `svc_set_mb_syn_cavlc.rs`, `svc_set_mb_syn_cabac.rs`,
`svc_encode_slice.rs`, `nal_encap.rs`, `encoder_context.rs`,
`wels_func_ptr_def.rs`. The enabling fact is new: the job handle can carry
borrows across the spawn now, so the buffer ranges can be **partitioned before
the fork** (`split_at_mut` family — one worker, one disjoint `&mut [u8]`,
proven by the compiler) and threaded down the writer chain instead of
re-derived from the context inside it. Method: enumerate the writer chain from
the fork's entry to the deepest bitstream write; thread the range as a
parameter; one targeted probe for the partition itself, control seen red.
Gate: `session` + span scan on every commit here.

### Step 2 — the conversion mass

With both seams down, the remainder is body work on established currencies.
Per-file allows at 327 (re-measure; the census tags tell you which portion of
each file is `instrument(test)` or screen-content — subtract those from the
convertible count):

| file | allows | | file | allows |
|---|---:|---|---|---:|
| `svc_encode_slice.rs` | 55 | | `svc_motion_estimate.rs` | 17 |
| `svc_mode_decision.rs` | 32 | | `slice_multi_threading.rs` | 17 |
| `encoder_ext.rs` | 30 | | `wels_preprocess.rs` | 16 |
| `encoder_context.rs` | 26 | | `rc.rs` | 13 |
| `ref_list_mgr_svc.rs` | 19 | | `svc_set_mb_syn_cabac.rs` | 8 |
| `wels_encoder_ext.rs` | 18 | | `paraset_strategy.rs` | 7 |
| `svc_base_layer_md.rs` | 18 | | `nal_encap.rs` | 6 |

Run the cascade first on each file — much falls without hand edits once
callees are safe. An allow retires only with its body's **last** raw operation:
sequence to finish bodies, not to touch many. Camera-path files
(`ref_list_mgr_svc.rs`, `rc.rs`) gate at `family` minimum; the five-times rule
for any flake there.

### Step 3 — the screen-content casts convert (ruling D-scope-6)

The 23 `SCREEN_CONTENT(dormant)` allows are the `SVAAFrameInfoExt` downcast
family — an arm the port can never reach (nothing allocates an `Ext`). Convert
the downcast to a safe shape — an enum over the two frame-info forms, or an
accessor that returns `Option<&SVAAFrameInfoExt>` answering `None` today —
line-for-line, keeping the scaffolding a future screen-content effort would
inherit. **Dark-code discipline**: no sweep row, no test, no gate executes this
arm; the compiler and your review are the only referees; deletion was
considered by the user and declined for exactly that blindness.

### Step 4 — the version exports move home

The two C-ABI version exports live in `encoder/wels_encoder_ext.rs`, but the
end state's forbid-exception list doesn't include that file. Move the two
`#[unsafe(no_mangle)]` exports into `src/api/` where the rest of the ABI
lives — the exported symbol set must not change (`tools/abi_sizes.sh` and the
export-list check referee this). Gate: `family` + the ABI check.

### Step 5 — the enumerated floor (D-exit-4's implementation)

When a file's *product* allows are gone but its test module still carries
instruments: `#![deny(unsafe_code)]` at the top, each test allow left in place
under its `instrument(test)` tag, and the census allowlist gains the enumerated
entries. Everything else seals `forbid` as it empties. At the end of this step
the tracking number should read approximately the instrument set and nothing
else — quote the census by tag to prove it.

### Step 6 — E2, the final flip

Delete `src/lib.rs`'s crate-wide `allow(unused_unsafe, unsafe_op_in_unsafe_fn,
…)`; reduce the census allowlist to the api island + `rec_view.rs`'s one
product line + the enumerated instrument set; regenerate and **pin** the
ratchet; prove the seal enforces by injecting a violation into a sealed file
and watching it rejected. Gate: `session` + `cargo check --all-targets`.

### Step 7 — E3, the exit battery

`bash rust/tools/gates.sh exit`: ABI export list, dlopen harness, upstream
gtest ratchet, full Miri including the differential tests **and the two
full-encode fork probes** (`rust/tools/fork_join_probe.sh`, ~59 min as a pair,
tripwire against its baseline), both-profile sweeps, and **both benches against
the perf budget — the entire debt, thirty-plus checkpoints, by the user's
standing ruling**. Same machine both sides, two after-runs; a real regression
bisects over the per-checkpoint commits (S10's 7% decoder catch shows both
that regressions are real and that one bench localizes them). Budget real time
for whatever the battery surfaces — it is the plan's last and largest risk.

## Findings and the report

Findings: `rust/docs/phase9_findings.md`, appended, numbered — start after
whatever the S10 close-out reached (it owed F264 onward; check the count). A
blocker needing the user's ruling becomes a finding and stops that checkpoint.
At the close: both tables in `rust/docs/safe_conversion_execution_plan.md`,
the Miri baseline **prepended**, the ratchet regenerated downward and — if E2
landed — pinned.

Report in plain prose: per-checkpoint commits with gate verdicts; the seam
probe with its control seen red; the census by tag at the close; the tracking
number's movement; every place this brief was wrong, quoting the sentence; and
a roll-forward line naming everything owed. If E3 runs, its full verdict — and
if it doesn't, say so in the first paragraph, not the last.
