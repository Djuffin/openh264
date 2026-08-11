# Phase 4b — configuration dispatch, the strategy vtables, and 4a's unfinished business

**Governing:** [`safety_refactor_plan.md`](../safety_refactor_plan.md) — §0 for where the
port stands, **§7.6 for the standing rules (S1–S21)**, §7.4 for D-perf-4. This file
scopes the phase and supersedes on disagreement; fix disagreements in place.

**Why this phase exists, and why now.** Phase 4a de-virtualized *kernel* dispatch —
tables whose entries are CPU-variant implementations of one arithmetic operation. It
deliberately left the tables whose entries are **configuration choices**: which entropy
coder, which rate-control mode, which parameter-set strategy. Those were fenced behind
Phase 3 because their signatures name the bitstream writer, and rewriting a signature
while its parameter list was still moving would have meant doing it twice. **Phase 3 is
complete**, the writer is one family with a stable signature, and the fence is lifted.

**Estimated 1–2 sessions.** The enum work is one seam; 4a's leftovers are the second.
If both fit, the phase is one session; plan the boundary at the seam, never mid-seam.

---

## 0. Start here

Tree should be clean. Run the control battery first (`bash rust/tools/gates.sh full`,
`OVERALL:` is the verdict) and recount — **431 debug / 425 release / 20 ignored** and
Miri **291** at Phase 3's exit. The recount rule has paid twelve times; it is cheaper
than every occasion it has caught something.

**Expect an F3 hit on the opening control battery.** It has happened on the
session-start commit, before a line was changed, in *three consecutive sessions* now.
Apply S14: single hit with the signature (`mt`, `sm=3`, `t∈{2,4}`, wrong length) →
re-run that configuration; more than one → alternate HEAD and control in one loop.
Do not let it eat the session's first hour, and do not treat it as a regression until
the alternation says so. Four alternations, four acquittals.

---

## 1. T4b.1 — the entropy-coder and rate-control dispatch become enums (seam)

### 1.1 What is actually there

`SWelsFuncPtrList` (`wels_func_ptr_def.rs`) holds **66 `pf*` members**. Most are 4a's
kernel dispatch and are already direct. The configuration ones are these, and they are
set together in `encoder_context.rs:752-766` by one `if` on
`iEntropyCodingModeFlag`:

| slot | CAVLC | CABAC |
|---|---|---|
| `pfWelsSpatialWriteMbSyn` | `WelsSpatialWriteMbSynCavlc` | `WelsSpatialWriteMbSynCabacThunk` |
| `pfStashMBStatus` | `StashMBStatusCavlc` | `StashMBStatusCabac` |
| `pfStashPopMBStatus` | `StashPopMBStatusCavlc` | `StashPopMBStatusCabac` |
| `pfGetBsPosition` | `GetBsPosCavlc` | `GetBsPosCabac` |

**One boolean selects all four.** That is an `enum EntropyCoder { Cavlc, Cabac }` with
four methods, and the four `Option<fn>` slots plus their `is_some()` assertions go.

**Phase 3 left you two things at this boundary, and both are load-bearing:**

* **`WelsSpatialWriteMbSynCabacThunk` exists only to bridge a plain Rust `fn` to the
  `extern "C"` slot type** (`svc_set_mb_syn_cavlc.rs:1106`). With an enum there is no
  slot type, so the thunk is pure deletion.
* **`PStashMBStatus`/`PStashPopMBStatus` gained a `buf: &mut [u8]` parameter at T3.5**,
  and the CAVLC variants take it and ignore it. That asymmetry is real, not an
  oversight: CABAC's stash must copy emitted bytes because `PropagateCarry` rewrites
  bytes behind the cursor; CAVLC's snapshot is a `Copy` of the cursor and needs nothing.
  **Under an enum the parameter can be dropped from the CAVLC arm entirely** — which is
  the point of doing this as an enum rather than a trait object, and is the first
  concrete win to bank.

The **RC-mode table** is the same shape one level up: `pfRc.*` slots selected by
`iRCMode`. Convert it in the same seam or the next, but do not interleave it with the
entropy work — one dispatch family per commit, so a byte-exactness failure names its
cause.

### 1.2 The rule that governs the conversion

4a's finding, restated because it decides the design: **direct dispatch recovers
per-call scaffolding only where the caller supplies constant dimensions.** These are
*per-macroblock* calls with runtime-selected behaviour, so do not expect the enum to
buy speed — expect it to buy the deleted `Option`, the deleted thunk, the deleted
parameter, and a signature the compiler can see through. Measure anyway (one
interleaved pair per bench per seam, `rust/tools/perfpair.py`), and if a row moves,
ledger it per D-perf-4 rather than optimizing.

**Match on the enum at the call site; do not build a vtable out of it.** A
`match self { Cavlc => …, Cabac => … }` inside a `#[inline]` method is what lets the
literal-argument folding survive — the same mechanism §4 of Phase 3's brief describes
for `get_bits(n)`.

### 1.3 Face gate

Full battery; **sweeps 341/341 both profiles are the referee** and both `cabac=0` and
`cabac=1` configurations are live here; decoder goldens must not move at all; one
interleaved pair both benches; Miri; ratchet.

---

## 2. T4b.2 — the two strategy vtables become traits

Two hand-rolled C++ vtables survive, both in the encoder, both with a
`pVtbl: *const …Vtbl` first member and `Destroy` as entry zero:

* **`IWelsParametersetStrategyVtbl`** (`paraset_strategy.rs:50`) — **20 entries**
  (`Destroy`, `GetPpsIdOffset`, `GetSpsIdOffset`, `GetSpsIdOffsetList`, four
  `GetNeeded*Num`, `LoadPrevious`, …). Note: an earlier draft of this brief said 16;
  the count is 20, verified at Phase 3's exit.
* **`IWelsReferenceStrategyVtbl`** (`ref_list_mgr_svc.rs:1560`) — **7 entries**
  (`Destroy`, `BuildRefList`, `MarkPic`, `UpdateRefList`, `EndofUpdateRefList`,
  `AfterBuildRefList`, `Init`).

Both are internal — neither crosses the public C ABI, which `api/abi_guard.rs` guards
separately — so they can become Rust traits with `Box<dyn …>` owners, or enums if the
implementor set is closed and small. **Check which before choosing:** an enum beats a
trait object here for the same reason it does in §1, but only if there are genuinely
two or three implementors.

`Destroy` as entry zero is the C++ virtual destructor. Under a trait it is `Drop`, and
the explicit `Destroy` calls become scope ends — that is **R4 in miniature again**, the
pattern T3.6 ran for the frame output's free cascade. Apply **S21**: these objects are
constructed through `WelsMallocz`'d allocations, so every construction path gets audited
in the commit that gives them an owned field, and a zeroed `Box`/`Vec` is UB at a
distance. Apply **S20** first: compute the signature-reachability closure before sizing
commits — anything embedding these by value, and every `abi_guard` assert pinning a
member, lands together or the tree is never green.

## 3. T4b.3 — 4a's named unfinished business

Carried forward verbatim from Phase 4a's exit, still unstarted:

* **The decoder intra-pred *mode* tables** — 4a converted the kernels, not the mode
  selection.
* **`sBlockFunc`** — the deblocking block dispatch pair.
* **The expand fn-pointer re-wraps** — `pfExpandLumaPicture`-class slots that 4a rewrapped
  rather than removed.
* **`decode_slice.rs`'s cache-fill `transmute`s.** **`transmute` is 23 and has not moved
  since Phase 0** — it is the one ratchet metric no phase has touched, and it is called
  out here because it will otherwise keep not moving. These are the bulk of it.
* **The encoder's ~55 CPU-dispatch members**, including **`WelsInitSampleSadFunc`**,
  whose deletion unblocks nothing but cleans the F13-adjacent struct. Low value, low
  risk, good filler at a seam boundary.

## 4. What Phase 3 hands you

* **The bitstream layer contains no pointer cursor, on either side.** `BsReader` and
  `BsWriter` are detached cursors, buffers are passed per call, and `SBitStringAux` is
  deleted. If you find yourself adding a buffer *field* to anything, stop.
* **T3.3's standard: extents are `buf.len()`, never a field.** T3.6 deleted `uiSize`
  and `iCountNals` rather than converting them. Do not reintroduce the shape.
* **Two `SHIM(phase3)` markers survive, both narrowed and both owned by Phase 6**:
  `bs_buffer` (`nal_encap.rs`) and `slice_bs_buffer` (`svc_encode_slice.rs`), which now
  guard only `SWelsSliceBs`'s MT-aliased buffer. **They are not yours.** The thread
  buffer pool is F12/P10 territory.
* **One straggler name, listed with its owner**: the decoder's
  `SDqLayer::pBitStringAux`, whose type is already `*mut BsReader`. Phase 5's.
* **The parked families are unchanged**: `common/sad_common.rs` (14) and
  `encoder/sample.rs` SATD (7), second dated verdict, re-attempt owed **at caller
  conversion (Phase 6.3)** — not here. SATD still owes a measurement of its own.

## 5. Gates

Per **S14–S17** unchanged. Plus: **T3.0's 2316-row malformed-stream golden table is now
a permanent instrument** and runs in the battery — it must stay green and `WITHHELD`-free
in both profiles, and it goes on gating Phases 5 and 8. One interleaved pair per bench
per seam; full 3-pair medians at the phase exit only. Ratchet per R-f's shape.

## 6. Non-goals

No Phase 5 or 6 pulls: `SDqLayer`, `SMbCache`, the picture pool, `SSlice` layout, the
thread buffer pool, `SWelsSliceBs` — shells and comments instead. No re-opening the
parked families (second dated verdict; Phase 6.3). No fixing F8/F9/F11-class arithmetic
(**S6**). No `get_unchecked`, ever (**S8**). No golden movement. And the standing
temptation warning, fourth phase running: the seams are ordered so the tree stays green
— do not reorder them because one looks quicker.
