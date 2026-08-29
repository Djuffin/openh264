# The `*mut sWelsEncCtx` → `&sWelsEncCtx` flip: tabulation

**S7 checkpoint 1.** The plan (stage D, and the S6 brief's "context question") specifies
this flip as: *tabulate every fork-reachable body by what it writes through the context,
move each worker-written field to atomics / `Cell`s / separately-allocated storage — each
move with its own targeted two-thread probe, control seen red — and only then flip bodies
to `&sWelsEncCtx`.* That is the shape the layer flip took across S5 and S6.

This document is the tabulation. **Its headline is that the storage-migration campaign
the plan specifies is not needed**, because the fields worker threads write have already
been moved. What the flip actually needs is a different and smaller list, below.

Everything here is compiler-measured, not inferred. Method and caveats are at the end.

---

## 1. The bodies

`106` bodies take the encoder context as a raw pointer. Measured two ways that agree:
`phase9_forksplit.py --list` reports 106, and a grep for the parameter reports 106 **only
if it catches fully-qualified paths** — `: *mut sWelsEncCtx` alone finds 102 and misses
four written `: *mut crate::encoder::encoder_context::sWelsEncCtx`, in two files
(`set_mb_syn_cabac.rs`, `svc_set_mb_syn_cabac.rs`) that a per-file survey built on that
grep does not list at all. The tool was right and the grep was short; the S6 close's
census inherits the same 4-body blind spot.

| file | in-fork | ST-flippable |
|---|---:|---:|
| `svc_encode_slice.rs` | 37 | 0 |
| `svc_mode_decision.rs` | 19 | 1 |
| `svc_base_layer_md.rs` | 15 | 0 |
| `rc.rs` | 10 | 0 |
| `slice_multi_threading.rs` | 6 | 0 |
| `svc_set_mb_syn_cavlc.rs` | 4 | 0 |
| `svc_encode_mb.rs` | 4 | 0 |
| `encoder_context.rs` | 3 | 3 |
| `md.rs` | 2 | 0 |
| `wels_func_ptr_def.rs` | 1 | 0 |
| `paraset_strategy.rs` | 0 | 1 |
| **total** | **101** | **5** |

Plus 9 function-pointer typedefs carrying the parameter, which must move with the bodies
behind them, and 5 test-local `let p: *mut sWelsEncCtx` bindings, which do not.

## 2. The write set — one write, and it is not in the fork

**The instrument.** All 106 parameters and 9 typedefs were flipped to `&sWelsEncCtx`, the
resulting call sites were rewritten mechanically off rustc's byte spans, and the residual
type errors were cleared until **zero** remained — which is the condition for MIR borrowck
to run over every body. What is left at that point is the complete set of things the flip
cannot do. It is five errors:

| # | site | error | what it is |
|---|---|---|---|
| 1 | `set_mb_syn_cabac.rs:830` | `E0596` cannot borrow `pEncCtx.sWelsCabacContexts` as mutable | **the only direct write through the context in the whole tree** |
| 2 | `encoder_ext.rs:2858` | `E0502` | `ParasetStrategy(pCtx).UpdatePpsList(pCtx)` — see §4(c): partly an artifact of the measurement's lifetime pass, and the underlying problem is worse |
| 3 | `wels_encoder_ext.rs:415` | `E0502` | `ParasetStrategy(pCtx)` borrow live across `pCtx.pOut.as_deref_mut()` |
| 4 | `wels_encoder_ext.rs:571` | `E0502` | as (3) |
| 5 | `wels_encoder_ext.rs:601` | `E0502` | as (2) |

**None of the five is fork-reachable.** `WelsCabacInit` (1) has exactly one caller —
`encoder_ext.rs:1628`, inside `WelsInitEncoderExt` — and wants `&mut sWelsEncCtx`, which
rustc suggests verbatim. `ParasetStrategy` (2–5) is in the tool's ST-FLIPPABLE list.

**Why this is not a surprise, stated precisely.** It does not mean worker threads write no
context state. It means every field they write already reaches them through interior
mutability or a separate allocation — which is what the tool's "IN-FORK (interior
mutability or lawful raw)" category has been counting all along, and what
`safeplan_prohibitions.py` reports when it says *"106 `*mut`-ctx bodies scanned against 0
writers — no violations"* (run it **from the crate root**; from the repo root it globs
nothing and prints a green-looking zero). The migration work the plan budgets for was
done incrementally by earlier sessions. The compiler now confirms it across all 106
bodies at once, which no previous measurement did.

## 3. The F239 column — empty, and controlled

F239: `&SDqLayer` is a retag over the *whole* struct, and it pops any field-precise
exclusive derivation live across it — invisibly to rustc, because the parent was a raw
pointer and borrowck does not track those. That defect reached the tree behind two green
sweeps and was caught only by Miri.

The same shape at the context level is: a raw context, `&mut (*pCtx).f` or
`addr_of_mut!((*pCtx).f)`, a later call passing that context, a later use of the
derivation. **A scan for that three-step span finds zero, and its control was seen red** —
injecting the pattern into `WriteSliceBs` produced the expected hit at
`slice_multi_threading.rs:941`; the clean tree reports none.

There is a structural reason to expect this to stay true, and it is worth stating because
it makes the context flip *safer* than the layer flip was: once a body's parameter is
`&sWelsEncCtx`, `&mut (*pCtx).f` inside it is a **compile error** (E0596), not silent UB.
The layer flip's hazard came from bodies that kept the layer raw while callees took `&`.
For the context, the equivalent transition sites are exactly the four `E0502`s in §2 —
**rustc catches them**, because those callers hold a reference, not a raw pointer. The one
place that keeps a raw context across the seam is `SliceJobHandle` (§4), and it is a
single, named object rather than a diffuse pattern.

## 4. The real blockers, in the order they must be done

Not writes. Three structural items:

**(a) Transitive raw passes — the flip must be root-down.** A body cannot take
`&sWelsEncCtx` while it still hands the context to a raw-taking callee; flipping one body
in isolation produces only `E0308` at its own onward calls and never reaches borrowck.
This is why the measurement had to flip all 106 at once, and it is why the landing must go
leaf-first. It is also the whole reason `ref_list_mgr_svc.rs` did not move in S6 (F240).

**(b) The fork seam — `SliceJobHandle`.** The handle stores the context
(`slice_multi_threading.rs:1276`) and carries it across `thread::scope`, under
`unsafe impl Send for SliceJobHandle` — one of the two `unsafe impl` lines the end state
keeps. Three sites read `job.pCtx` (`:1578`, `:1641`, `:1862`) and pass it onward. Making
the field `&'a sWelsEncCtx` adds a lifetime parameter to the struct and requires
`sWelsEncCtx: Sync` for the handle to stay `Send` — which is the same audited-`unsafe impl`
shape the reconstruction view already uses, not a new kind of claim. **This is the design
work of the flip**, and it should be its own checkpoint.

**(c) `ParasetStrategy` — a self-referential borrow, and the second piece of design
work.** *(Corrected after the first draft of this document; the first draft called this
mechanical and it is not.)*

`ParasetStrategy(pCtx)` returns `&'a mut CWelsParametersetIdStrategyObj`, and the object
it returns **lives inside the context** — `ctx.pFuncList.pParametersetStrategy`. Its
methods then take the context again: `UpdatePpsList(&mut self, pCtx: &mut sWelsEncCtx)`.
So `ParasetStrategy(pCtx).UpdatePpsList(pCtx)` wants the strategy and the whole context
mutably at the same time, which is why this call is spelled with raw pointers today.

The four `E0502`s the measurement reported at these sites were **partly an artifact of the
measurement**: the experiment's lifetime pass tied `ParasetStrategy`'s unbound `'a` to the
context parameter, and that tie is what produced them. Without the tie the signature would
hand out `&mut` derived from a `&`, which is unsound rather than merely inconvenient. The
honest statement is therefore stronger than the first draft's: `ParasetStrategy` cannot
take `&sWelsEncCtx` at all. It wants `&mut sWelsEncCtx` — it is not fork-reachable, so that
is allowed — and then `ParasetStrategy(pCtx).UpdatePpsList(pCtx)` becomes a double-`&mut`
(`E0499`), which needs the strategy lifted out of the context for the call
(`Option::take`/`mem::replace`) or `UpdatePpsList` narrowed to the fields it actually
touches. **22 call sites**, of which eight already spell the context
`std::ptr::addr_of_mut!(*ctx)` from an owned `Box`.

This is design work of the same kind as (b), and it should be its own checkpoint rather
than a step inside the flip.

Alongside these, the mechanical residue the measurement had to clear, which the landing
will also have to: **56 context `is_null()` call sites**, most of which retire with the
parameter and each of which needs its own caller enumeration first (§6), **10
re-casts** of the form `pCtx as *mut _` inside bodies that already had a reference, **6
accessors** whose unbound `'a` must tie to the context borrow (the `layer_ref_pic` shape
from S6), and **2 `SWelsMD<'_>` invariance sites** needing the lifetime threaded — all
identical in kind to what the layer flip cost.

## 5. Cost, measured

The whole-tree flip experiment touched **17 files, 217 lines** (`git diff --stat`) — 115
signatures (106 parameters + 9 typedefs) and the rest call sites, rewritten mechanically
off rustc's byte spans in three fixpoint passes. That is the signature-and-call-site cost.
It excludes (b), which is design rather than substitution.

## 6. Caveats on the measurement

Stated so the next session can judge the result rather than inherit it:

* The experiment replaced 56 context `is_null()` calls with `false` to reach borrowck.
  Those guards retire with the parameter regardless — a reference cannot be null — but the
  write-set result assumes that, and the landing must confirm each guard's call sites the
  way S6.D1 did for `SSliceArgument` rather than removing them on the general claim.
* Two boundary sites used a `pCtx as *const _ as *mut _` escape to type-check:
  `SliceJobHandle`'s constructor and the `WriteSliceBs` call sites. **Both bodies were
  themselves borrow-checked with `&sWelsEncCtx`** — only the handle's stored field stayed
  raw — so the escape narrows §4(b), not the write set.
* The experiment was reverted; nothing in this checkpoint changes the tree.
