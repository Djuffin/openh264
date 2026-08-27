# Safe-conversion plan — Session S1: the context accessor layer becomes safe methods

*Self-contained: everything you need is explained here in plain words; numbers in
parentheses (F…, S…, D…) point at entries in the project docs. Re-run every count
before quoting it — trust the tree over this document, and before acting on any
claim anywhere, re-read the code it describes (rule S68: a claim of absence gets
its grep, a cited line gets read). Findings are numbered `F…` in
`rust/docs/phase9_findings.md`; yours start at **F208** (the count prints 107
today). The operative plan is `rust/docs/safe_conversion_execution_plan.md` —
read it first, its §10 amendments included; this session is its S1.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the
C++ reference is at the repo root, `codec/`). It ships as a drop-in
`libopenh264` replacement and must stay **byte-identical** to the C++ on every
stream the harness runs. Phase 9 (closed 2026-08-27) eliminated every reachable
undefined behavior and ran the multi-threaded fork under Miri; the operative
plan now converts everything that remains to *safe* Rust — the end state is
`#![forbid(unsafe_code)]` everywhere outside the C-ABI island plus two audited
`unsafe impl` lines. The prior corpus (rules S1–S69 in
`safety_refactor_plan.md` §7.6, findings F1–F207) remains binding.

## What session S1 does

The encoder context `sWelsEncCtx` is reached through ~20 accessor functions
that take a raw context pointer and hand back raw pointers into it —
`ctx_param`, `ctx_vaa`, `ctx_rc_at`, and the rest. Roughly 200 of the
remaining unsafe functions are unsafe *only* because they call these. S1
converts the whole layer: **every accessor becomes a safe method on
`sWelsEncCtx`, returning references**, and the de-unsafe cascade (stripping
`unsafe` from signatures whose bodies became safe, then from their callers) is
each checkpoint's second half, enforced by the ratchet.

The fresh counts (mentions including defs/docs, `grep -rn '\b<name>(' src`):

| accessor | sites | | accessor | sites |
|---|---:|---|---|---:|
| `ctx_param` | 247 | | `ctx_dq_layer` | 15 |
| `ctx_func_list` | 106 | | `ctx_sps_array` / `ctx_pps_array` | 15 / 15 |
| `ctx_vaa` | 79 | | `ctx_subset_array` | 13 |
| `ctx_rc_at` | 60 | | `ctx_frame_bs` / `ctx_rc` | 9 / 6 |
| `ctx_ref_list` | 36 | | `ctx_mvd_cost_table` / `_origin` | 5 / 5 |
| `ctx_frame_bs_cur` | 22 | | `rc_gom_fg_blocks` / `rc_gom_sad` | 2 / 5 |
| slice-local: `ctx_sps` 19, `ctx_pps` 3, `ctx_ref_pic` 4, `ctx_pic_ref` 2 (`svc_encode_slice.rs`) | | | | |

## The design: one accessor, two paths

This is the session's load-bearing rule, and it exists because of how Phase 9
ended. The encoder has two worlds:

- **Single-threaded code** (init, teardown, the frame loop between forks)
  holds `&mut sWelsEncCtx` since Phase 9's flip. It calls the new methods
  directly: `pCtx.param()`, `pCtx.param_mut()`.
- **Fork-reachable code** (the per-slice workers) holds `*mut sWelsEncCtx`
  **permanently for now** — N workers cannot each hold `&mut` to one
  allocation (rule S63, the phase's central theorem). S1 does **not** change
  those parameter types (that falls in S3/S7, when the context's raw fields
  are gone and it becomes `Sync`). What changes is the call spelling: an
  in-fork site calls the **`&self` reader only**, through a per-call shared
  reborrow — `(*pCtx).param()` inside its existing unsafe block — held for
  the expression, never stored (S37). Shared reborrows coexist across N
  workers; the fields workers *write* are already atomics or `Cell`s behind
  the audited seam (F132's nine rounds), which shared references permit.

So each accessor becomes: a **`&self` reader** (everyone), plus a **`&mut
self` writer** only where single-threaded writers need one. Two hard
prohibitions, both grep-checkable at every checkpoint:

1. **No in-fork site ever uses a `&mut self` method** — the fork-reachability
   tool classifies every call site (`python3 rust/tools/phase9_forksplit.py
   --list`), and
2. **the `&mut *pCtx`-class retag count never grows** (`grep -rn '&mut
   \*pCtx\|&mut \*\*ppCtx\|&mut \*pEncCtx' src/encoder | grep -v ':\s*//' |
   wc -l` — re-measure at start, quote at every checkpoint). Rule S67's
   audit blessed the existing ones; a new one is a finding, not a convenience.

**The starting position is favorable and measured**: the hazard/fork join tool
(`python3 rust/tools/phase9_ctx_join.py`) reads **0 LIVE hazards** on the
single-threaded side today — the flips are unobstructed — and **19 moot
sites** (all in-fork, by file: `slice_multi_threading.rs` 5,
`svc_encode_slice.rs` 5, `svc_mode_decision.rs` 5, `svc_set_mb_syn_cavlc.rs`
2, `rc.rs` 1, `svc_encode_mb.rs` 1), which is precisely the in-fork call-site
list your `&self`-reader conversions must walk. Run the join before **every**
checkpoint; its multi-accessor-holder map is where split-borrow work hides.

## The checkpoints, A1 → A7 (drop-from-the-end: the tail rolls to S2's front)

Each checkpoint = one gated commit (`bash rust/tools/gates.sh commit`, 15–19
min). Where two accessor results are held live in one caller, the split-borrow
protocol (plan §4.6) applies in order: reorder the uses; one combined method
returning disjoint `&mut` fields from a single borrow (the in-tree example is
`LoadPreviousStructure`, `wels_encoder_ext.rs`); copy-out/write-back only
where the C semantics provably allow it (S62: outcome-equality when a value is
substituted).

- **A1 — the small fry** (~32 sites: `ctx_mvd_cost_table`/`_origin`,
  `ctx_rc`, `ctx_frame_bs`, `rc_gom_*`): re-validates the method end to end —
  reader/writer split, in-fork spelling, cascade, gate.
- **A2 — `ctx_rc_at`** (60): the rate controller's per-layer state; `rc.rs`'s
  unsafe-fn count should collapse in the cascade.
- **A3 — `ctx_ref_list` + `ctx_dq_layer`** (36 + 15): held **jointly** in the
  encode loop — join analysis first, combined-accessor likely.
- **A4 — the parameter-set arrays + `ctx_frame_bs_cur`** (15+15+13 + 22),
  **folding in the four slice-local accessors** (`ctx_sps` 19 chains *through*
  `ctx_sps_array` — F165 recorded the chain; converting the array first makes
  the chained one a safe composition).
- **A5 — `ctx_vaa`** (79): includes the `SVAAFrameInfoExt` downcast — six
  callers cast the result; design an enum or an accessor pair, don't scatter
  casts.
- **A6 — `ctx_func_list`** (106): **plan §10.1 binds here.** The dispatch
  table is rewritten at frame cadence (`SetFastCodingFunc`/
  `SetNormalCodingFunc`), so no borrow of it may live across a frame
  boundary: the pattern is copy-the-(Copy)-fn-pointer-to-a-local first,
  per-dispatch, instantaneous. Before choosing the flip at all, **price the
  alternative F191 named** — finishing the Phase 4b dispatch enums — and say
  in the report which you took and why.
- **A7 — `ctx_param`** (247): the monster, deliberately last — by now most of
  its sites sit in already-converted callers. Split into `&self` reader +
  `&mut self` writer; the writers are the init/SetOption paths.

**Session close** (this completes stage A): `gates.sh family` (583/583 both
profiles), then the `session`-level gate — the Miri lane's wall goes beside
H3's **506 s** and, new since the instrument fix (F170), the **CPU column**
is the number the 1.3× tripwire fires on; quote both. Regenerate the ratchet
baseline downward (`unsafe_ratchet.sh generate` — standing practice), both
censuses, findings from **F208**, and the session log line per plan §9.

## Ground rules, compressed

- **Bit-exactness is stop-the-line**: a diffharness SHA divergence is a bug in
  your change, full stop. The two live-camera-path files this session touches
  most (`rc.rs`, `ref_list_mgr_svc.rs`) sit near the project's historic flake
  (F3) — a test failure that does not reproduce follows the adjudication
  protocol in `phase0_findings.md` (5/5 re-run; second hit escalates to
  head-vs-control alternation; never shrug, never bisect a phantom).
- **Metrics live at both ends** (§7.1): `bash rust/tools/unsafe_ratchet.sh
  report` at start and close — today `raw_ptr` **1180**, `unsafe_fn` **593**,
  `unsafe_block` **266**. The plan's single tracking number:
  `#[allow(unsafe_code)]` sites outside `src/api/` — **627 today**
  (`grep -rn 'allow(unsafe_code)' src --include='*.rs' | grep -v '/api/' |
  wc -l`; the plan's §9 says 611 — the drift is F203's two found files plus
  the census fix, worth one corrective line in your report).
- **A tag comes off only with the unsafe it annotates** — never stripped
  early, never left stale; untagged unsafe fails the census gate.
- No edits while a gate runs; one gate at a time; no Miri between commits;
  blockers become findings; every count you quote carries its command.

## What to report back

Plain prose: per-checkpoint commits with gate verdicts and the cascade's
numbers; the per-accessor outcome table (reader / writer / in-fork spelling /
combined-accessor if any); the join tool's headline before and after; A6's
flip-vs-enum decision with its reasoning; both prohibitions' grep results at
the close; the close gate's wall and CPU numbers beside 506 s; the tracking
number's movement (627 → ?); every place this brief was wrong, quoting the
sentence; and the hand-off to S2 — which, per the amended plan, opens with
whatever A-tail rolled forward, then the owned-fields stage.
