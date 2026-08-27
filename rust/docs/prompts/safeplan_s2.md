# Safe-conversion plan — Session S2: the rest of the accessor layer, then owned fields

*Self-contained: everything you need is explained here in plain words; numbers in
parentheses (F…, S…, D…) point at entries in the project docs. Re-run every count
before quoting it — trust the tree over this document, and before acting on any
claim anywhere, re-read the code it describes (rule S68: a claim of absence gets
its grep, a cited line gets read). Findings are numbered `F…` in
`rust/docs/phase9_findings.md`; yours start at **F213**. The operative plan is
`rust/docs/safe_conversion_execution_plan.md` — read it first, its §10 amendments
included; this session is its S2, and it opens with S1's A-tail.*

## Where S1 left the tree

S1 landed **four checkpoints, each its own gated commit**, and stopped at a
checkpoint boundary per drop-from-the-end. It converted the small fry (A1), the
rate controller's per-layer accessor (A2), the reference list (A3) and the
parameter-set arrays plus the bitstream cursor (A4). The close gate — `gates.sh
session` — is green: 583/583 sweeps in both profiles, 561 debug / 554 release
tests, Miri 291/291 across four shards, gtest smoke 4/4.

**The design S1 laid down, which S2 continues** (it is written at the head of the
`impl sWelsEncCtx` block in `encoder_context.rs`, read it there):

* **Readers take `&self` and are what everyone calls, the fork included.** An
  in-fork site spells it `(*pCtx).name()` inside the unsafe block it already has:
  a per-call shared reborrow, never stored (S37).
* **Writers take `&mut self` and are single-threaded only.** Grep-checkable, and
  checked at every checkpoint by `rust/tools/safeplan_prohibitions.py <writer>…`,
  which classifies every `*mut sWelsEncCtx` body with the forksplit and reports
  any that reach a writer. It read **0 violations / 103 bodies** at S1's close.
* **Where a return stays raw** it is because the far end cannot carry a lifetime
  (F193's `SLayerBSInfo::pBsBuf`) or is a raw *field* a later stage converts.
  Those accessors are still *safe fns*: forming a pointer needs no `unsafe`.

**Read F208 before you write a line.** It is S1's most expensive lesson and it
will bite S2 harder, because `ctx_param` is in ~250 bodies:

> A `&self` accessor on `sWelsEncCtx` is a `SharedReadOnly` retag over the
> **entire context**. It may not be called while any `&mut`-shaped derivation
> into that same context is live. Where the context is `&mut sWelsEncCtx`,
> borrowck enforces it. **Where it is a raw pointer, nothing does but Miri.**

S1 shipped four green family gates with that bug in the tree; the session-close
Miri lane was the only instrument that saw it. Run
`python3 rust/tools/f208_reader_retag_scan.py` from the crate root after every
checkpoint (it exits non-zero on a body needing a hand read), and **do not defer
the Miri lane to the close** — for A7 especially, run it mid-checkpoint.

## What S2 does

**The A-tail first, then stage B.** In order:

| # | scope | sites (re-measure) | notes |
|---|---|---:|---|
| **A5** | `ctx_vaa` | 79 | 32 in `wels_preprocess.rs`, 17 `rc.rs`, 15 `ref_list_mgr_svc.rs`, 7 `svc_mode_decision.rs`, 3 `svc_base_layer_md.rs`. **14 writes** through the answer, all on the preprocess/ref-list side. Includes the `SVAAFrameInfoExt` downcast — **16** casts, not the brief's six: design an accessor pair or an enum, don't scatter them. `SCREEN_CONTENT(dormant)` is fenced (F177: this port never allocates an `Ext`), so the downcast arm is dead code and may not be *driven* — keep it compiling and keep the tag. |
| **A6** | `ctx_func_list` | 106 | **Priced in F212: take the flip.** `func_list(&self)` for the readers, `func_list_mut(&mut self)` for the one re-writer (`encoder_ext.rs:2466`, which already derives exactly that `&mut`). F191's objection — a reader holding the table across the frame-cadence re-write — becomes a *compile error* under the flip, because the re-writer needs `&mut`. The re-write surface is two fields (`pfIntraFineMd`, `sSampleDealingFuncs.pfMdCost`), one caller, single-threaded, and the fork never writes the table. The dispatch enums F191 prefers are a **different debt** and the plan already schedules them as **C1**; do not pull them forward. |
| **A7** | `ctx_param` | 247 | The monster, deliberately last. Split reader/writer; the writers are init/SetOption. Expect the F209 cascade to land here: `rc.rs`, `ref_list_mgr_svc.rs`, `encoder_ext.rs` shed `unsafe fn` only when their **last** unsafe callee does, and for most of them that is this one. |
| **B1–B3** | stage B as the plan has it | | singletons → `Option<Box<T>>`, owned mutex, MT lifecycle, `encoder_ext.rs` sweep, `wels_encoder_ext.rs` frame loop |

If the session must stop, stop at a checkpoint boundary and roll the tail forward
— never a half-landed checkpoint.

**Not S2's, and why** (F210): `ctx_dq_layer`'s 15 sites do not convert in stage
A. The fork *writes* the DQ layer (F191's 42 in-fork `*mut SDqLayer` parameters),
so the accessor can return neither `&mut` (S63) nor `&`; the ordering constraint
is **layer-before-accessor** and the layer is D2–D3's. The join tool reads 0 LIVE
at both ends of S1, so the "held jointly in the encode loop" framing S1's brief
used has no work behind it.

## The protocol that worked, compressed

Every checkpoint of S1 ran the same loop, and it is worth repeating verbatim:

1. **Classify before converting.** For each accessor, tabulate every body that
   reaches it by (a) the form of its context parameter and (b) whether it
   *writes* through the answer. S1's two big accessors both came back with no
   diagonal — every writer took `&mut sWelsEncCtx`, every in-fork body only read
   — which is F132's audited design showing up as a clean split, and it is what
   made the reader/writer split obvious rather than argued. Do not assume it
   holds for `ctx_vaa`; measure it.
2. **Convert naively, then let the compiler enumerate.** F196's method. A2 opened
   at 126 errors and A3 at 170; both resolved to ~20 bodies of work.
3. **Resolve in §4.6's order.** *Reorder* first — in practice, sink the accessor
   binding past the pure context reads, or lift those reads into locals. Nothing
   moves relative to anything else, so it is behaviour-preserving by
   construction. Where a *value* had to be substituted it is S62
   outcome-equality and it is named at the site. Reach for a **combined
   accessor** only when two *different fields* are genuinely wanted at once —
   S1 minted exactly one (`ref_list_and_ltr_mut`), on `ctx_paraset_arrays`'s
   precedent.
4. **Two recurring shapes worth naming**: an *orchestrator* (every branch
   re-enters through the context) holds nothing and re-derives per use;
   a body that only *reads* a `Copy` sub-struct copies it out instead of
   reborrowing, which lets writes to the rest of the struct coexist.
5. **Gate at `family`, not `commit`, for anything on the live camera path.** S1
   ran family on A2/A3/A4 and it cost ~3 min each. The commit level does not
   sweep, and a rate-control or reference-list change that survives the unit
   tests can still move a byte.
6. **Then**: `f208_reader_retag_scan.py`, `safeplan_prohibitions.py`, the retag
   grep, `unsafe_ratchet.sh generate`, commit.

## Numbers at S1's close (re-measure; do not quote these)

```
ratchet     raw_ptr 1137   unsafe_fn 580   unsafe_block 265   unsafe_impl 2
tracking    #[allow(unsafe_code)] outside src/api/ : 614
prohibition 2   &mut *pCtx-class retags: 22  (unchanged all session)
join        14 hazards, 0 LIVE, 14 moot   (was 19/0/19)
forksplit   103 bodies carry *mut sWelsEncCtx, 101 in-fork + 2 ST  (was 111)
miri lane   508 s wall / 1091 s cpu — and the cpu column is now SEEDED, so
            S61's 1.3x tripwire compares CPU against CPU from S2 on (F170)
```

The tracking number is the plan's single progress figure and it moved **627 →
614** across S1. Expect A7 to move it far more than A5 and A6 together, for
F209's reason.

## Ground rules, unchanged

- **Bit-exactness is stop-the-line.** A diffharness SHA divergence is a bug in
  your change. A test failure that does not reproduce follows the F3
  adjudication protocol in `phase0_findings.md` (5/5 re-run; a second hit
  escalates to head-vs-control alternation; never shrug, never bisect a phantom).
- No edits while a gate runs; one gate at a time; blockers become findings.
- A tag comes off only with the `unsafe` it annotates — never early, never stale.
  S1 stripped two `cursor` tags with `ctx_sps`/`ctx_pps`'s `unsafe`, and left the
  `fork-shared(S63)` tags on the routes that stayed raw.
- Every count you quote carries the command that produced it.

## What to report back

Plain prose: per-checkpoint commits with gate verdicts and the cascade's numbers;
the per-accessor outcome table (reader / writer / in-fork spelling / combined
accessor if any); A5's downcast decision; the join and forksplit headlines before
and after; both prohibitions plus the F208 scan at the close; the close gate's
Miri **CPU** number against S1's 1091 s (that is the tripwire now, not wall);
the tracking number's movement; every place this brief was wrong, quoting the
sentence; and the hand-off to S3.
