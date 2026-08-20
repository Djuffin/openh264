# Phase 6, session H — the context flip, second third: the members own

## What this session does, and why

Session G gave the context a constructor and turned its aliases into ids. This
session makes the **members own**: every allocation `RequestMemorySvc` takes from
`CMemoryAlign` on the context's behalf becomes an owned container built by the
constructor path, and the free cascade becomes `Drop` — with F19's check run per
member, because *an owner that exists in source but on no live path is a leak
wearing a destructor* (the paraset strategy leaked ~1.2 KB per teardown exactly
that way before T4b.2a).

Session I then runs the S37 inventory, the deny sweep, and the phase close.
Nothing here takes `&mut sWelsEncCtx`, and the dispatch tables stay untouched.

**Execution order is the section order; drop whole steps from the end** (3, then
2) with the reason logged. Steps 0–1 are the core.

## Ground rules

- **Gates**: `gates.sh commit` per commit, `family` per step, one `exit` at the
  close. Sweeps read **369/369** per profile (`st mt def sl ltr`).
- **Miri is encoder-scoped from this session on (D-gate-2, direction
  2026-08-20)**: step 0 builds the knob, and every Miri run this session and next
  uses it — the three decoder probes and the decoder-module tests do not run
  during Phase 6 sessions. The full unscoped `--lib` step runs **once**, at
  session I's phase close, as the handoff gate.
- **Miri once, at the close**, as before — now scoped.
- **Perf**: one span at the close, 7 pairs, fresh null. On a breach: **attribute
  per commit before acting** (S39). The known risks: `pFrameBs` and the MVD table
  moving behind root accessors — both per-frame/per-slice derivations, F5's
  amortization pattern applies.
- **Re-grep every count and line number** (S24 — session G measured this brief's
  predecessor drifting three times; the numbers below were read 2026-08-20 at G's
  close and are line anchors, not surfaces).
- **Layout changes re-pin in the same commit** (S36) — all 15 `assert_ctx_offset!`
  pins and the context size, both profiles where they split.
- **Constructor proofs are value-equality, not byte-equality** (S41/F64): a
  field-wise constructor differs from a memset image in exactly the bytes the
  type does not define — `Option` payloads (`None` writes only its tag,
  niche or not) and `repr(C)` padding. Reuse G's two-tier instrument shape:
  byte-compare the fully-defined fields, value-compare the rest, report the
  padding count.
- **Root accessors are S40-spelled**: the address is read out of the container's
  header (`Vec::as_ptr()` on a raw place, `PaddedPlane::root_ptr` the model), no
  `&mut` formed, and the covering Miri test derives twice and uses the first
  cursor.
- Sweep flake protocol unchanged (the F3 signature → 5× re-run → record in
  `phase0_findings.md`; measurements 77–78's loop lessons stand).

## The map (verified 2026-08-20, G's close)

**The allocation sequence** is `RequestMemorySvc` (`encoder_ext.rs:979`) — read
it top to bottom; it is the work list. **The free cascade** is the `WelsFree`
run at `encoder_ext.rs:1766–1810±` — it shrinks member-by-member as owners land,
and what remains at the end must be exactly the Phase-7 set.

| member | alloc / free anchors | becomes |
|---|---|---|
| `pStrideTab` | `:276` / `:1771` | `Box<SStrideTables>` — **it has raw innards of its own** (`pMbIndexX`/`pMbIndexY` per-layer pointers, read at `:579–:580`): convert them to owned `Vec`s inside the Box in the same step, take-what-you-reach |
| `pDqIdcMap` | `:883` / `:1775` | `Vec<SDqIdc>` |
| `pFrameBs` | `:1107` / `:1791` | `Vec<u8>` + an S40 root accessor — **this is an arena of bytes** (S37): NAL writers hold cursors derived from it across calls; the accessor hands out the root, offsets stay the writers' idiom |
| `pLtr` | `:1158` | `Vec<SLTRState>` (one per dependency layer) |
| `pWelsSvcRc` | `:1177` | `Vec<SWelsSvcRc>`; `rc.rs`'s 19 H-tagged lines (its own header attribution, T6.G4) convert with it |
| `pSpsArray` / `pPPSArray` / `pSubsetArray` | `:844` / `:1801`–`:1805` | `Vec<SWelsSPS>` / `Vec<SWelsPPS>` / `Vec<SSubsetSps>`; G's `SpsId`/`PpsId` resolution accessors retarget to the `Vec` roots, S40-spelled |
| `pMvdCostTable` | `:1293` / `:1857` | `Vec<u16>`, and **P11 lands**: the `(row root, bias)` accessor; the two record fields (`SWelsMD.pMvdCost`, `SWelsME.pMvdCost`) stay `*mut u16` per E's settlement but now derive from the owned table's root — enumerate them for I's inventory |
| `pPSOVector` | (find it beside the paraset allocs) | owned — this struct *is* F19's precedent; run the leak check first |
| `pSvcParam` | **verify the allocation site first** | if `pMa`-allocated on the context's behalf → `Box<SWelsSvcCodingParam>`; if the api layer owns it → leave raw and enumerate for Phase 8 (the decoder's F41 shape — do not guess, read) |
| `ppDqLayerList` | (list block) | `Vec<Box<SDqLayer>>` — the layers are Box-built since D; the *list* becomes the owner |
| `ppRefPicListExt` | `:1255±` | `Vec<Box<SRefList>>` — Box-built since F |
| `pVpp` / `pVaa` / `pOut` | — | `Box<CWelsPreProcess>` / `Box<SVAAFrameInfo>` / `Box<SWelsEncoderOutput>` — all three pointees own already (F, F, T3.6); `Create*`/`Destroy*` fold into `new`/`Drop` |

**Not this session's** (stay raw, enumerated): `pFuncList` and `SWelsFuncPtrList`'s
7 `mem::zeroed` sites (I), `pSliceThreading`/`pTaskManage`/`mutexEncoderError`/
`pDynamicBsBuffer` and `RequestMtResource`'s allocations (Phase 7), `pMemAlign`
itself — it survives this session serving exactly the Phase-7 set, and its census
is step 3's deliverable.

**The remaining `mem::zeroed` sites** (20 total, attributed in G's log): the
`SSliceHeader`/`SSliceHeaderExt`/`SWelsOut` family and per-type `Default`s across
ten files. This session takes every type that now holds (or gains) an owned or
`Option` field; a zeroed all-POD `Default` is *legal* (S21) and stays if nothing
forces it — do not churn types that need nothing.

## Step 0 — the Miri scope knob (D-gate-2)

`gates.sh`'s Miri `--lib` step gains an encoder-only mode (env knob or flag —
your call, documented in the script header): it must exclude the three decoder
probes and the decoder-module tests, and keep all four encoder probes plus the
encoder/processing/common test set. Verify by reading the run's own test list,
record the scoped count and wall time beside the unscoped 349/1411 s, and use it
for every Miri run in this session. The unscoped step runs once more in Phase 6 —
at I's phase close.

## Step 1 — the members own, in `RequestMemorySvc`'s own order

Walk the table above in allocation order. Per member, one commit-group:
constructor side (the `Vec`/`Box` built where the malloc was), free side (the
`WelsFree` lines deleted; the cascade function shrinks), **F19 check** (name the
live path that drops it — for the context that is the api boundary's teardown;
follow it once and cite it), consumers re-anchored (accessors S40-spelled where
cursors are handed out), sizes re-pinned.

Two members carry their own audits:
- `pFrameBs`: enumerate every cursor derivation before converting (the NAL
  encap writers, `FrameBsRealloc`'s `pNalLen` interplay from F60's fix) — the
  arena rule says the owner change must not add a single `&mut` on that path.
- `pSvcParam`: the ownership read decides Phase 8's inventory; write the verdict
  in the log either way.

**Check** per member: `gates.sh commit`; per the step: `family`, sweeps 369/369.

## Step 2 — the zeroed `Default`s that now lie

For each type G's log attributed to H (`SSliceHeader`, `SSliceHeaderExt`,
`SWelsOut`, and any type step 1 gave an owned field): field-wise constructor,
S41's two-tier proof, zero-meaning documented. Types that remain all-POD keep
their zeroed `Default` with a one-line comment saying why that is sound.

**Check**: `grep -rn 'mem::zeroed' src/encoder src/processing` reads only the
enumerated survivors (I's 7 + instruments).

## Step 3 — the allocator census

`grep -rn 'WelsMalloc\|WelsMallocz\|WelsFree' src/encoder src/processing` —
every remaining hit is attributed to Phase 7 (MT resources) or deleted. The
census lands in the log as a table; `memory_align.rs`'s death certificate is
Phase 7's to sign, but the list of what still calls it is this session's.

## Step 4 — close

1. Span (S39 on breach), scoped-Miri `exit`, ratchet re-baselined.
2. Log: the member table with before/after, the F19 citations, the `pSvcParam`
   verdict, the zeroed survivors, the allocator census, sizes/pins, F3
   measurements if any.
3. Plan §0 (H spent, I next) and phase6.md §6's H row; `perf_baseline.md`.

## Stays raw / untouched — the boundary list

| what | why | whose |
|---|---|---|
| `&mut sWelsEncCtx` anywhere; the S37 inventory | I's opening act | I |
| `pFuncList`, `SWelsFuncPtrList`, its 7 zeroed sites, dispatch dissolution | with the deny sweep | I |
| MT members and `RequestMtResource`, `pMemAlign`'s survival, `sSliceBs.pBs` | thread machinery | Phase 7 |
| `wels_encoder_ext.rs` internals; `pSvcParam` if the read says api-owned | ABI boundary | Phase 8 |
| E/F/G named survivors; the two `pMvdCost` record fields | measured settlements | I's inventory / Phase 9 |
| `SCREEN_CONTENT(dormant)` | fenced | Phase 10 |

## Done-test

- `RequestMemorySvc` allocates nothing except the enumerated Phase-7 set; the
  free cascade contains only that set; every converted member has its F19
  citation.
- `mem::zeroed` reads only the enumerated survivors; every new constructor has
  an S41 two-tier proof.
- The Miri scope knob exists, is documented, and every Miri run this session
  used it (counts + wall time recorded).
- Sizes and pins re-measured; span inside its null band or S39-attributed;
  scoped `exit` PASS.
