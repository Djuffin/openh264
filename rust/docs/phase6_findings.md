# Phase 6 findings

Findings opened during Phase 6 (the encoder structural rewrite). Numbering
continues `phase5_findings.md`, which ends at F56.

---

## F57 — `MvdCostInit`'s second cursor walks off the end of the MVD cost table

**Status: FIXED 2026-08-18 (Phase 6 session A), by accommodation rather than by
repair — F14's move, for F14's reason.** Found by the encoder aliasing probe
(`encoder_initialisation_runs_under_the_aliasing_checker`) on the first run in
which it got past F13's site, which is the finding's real subject: this had been
in the tree since the port began and no gate could see it.

### What it is

`RequestMemorySvc` allocates the inter MVD cost table as `52 * stride * 2` bytes,
where `stride = 1 + 2 * kuiMvdInterTableSize` (`encoder_ext.rs`). `MvdCostInit`
(`md.rs`) fills it with two cursors, one per half of each row:

* `pNegMvd` starts at the table's base, writes `kiSz` entries, writes one more,
  then advances `kiSz + 1` — a full stride per row. After the 52nd row it sits
  **exactly one past the end**, which `offset` permits.
* `pPosMvd` starts at `base + (kiSz + 1)` and advances by the same stride. After
  the 52nd row it sits `(kiSz + 1)` elements **beyond** the end — 1042 bytes on
  the probe's configuration.

That last bump is computed and never dereferenced. Forming it is UB in Rust and
in C alike, and the C++ upstream forms the identical pointer: the port did not
introduce this, it transliterated it.

### Why no gate has ever seen it

The same reason F14 survived: nothing observable comes of it. The value is
computed into a register and discarded, so byte-exact sweeps, conformance and
both benches agree with it. Only Miri can see it, and **the encoder had no
Miri-covered path that reached encoder initialisation** until this session.

### The fix

`52 * kuiMvdCacheAlignedSize + 2 * ((stride >> 1) + 1)` — the smallest allocation
that makes the arithmetic legal, which is F14's `2 * 256 + 16` again. The extra
bytes are never read, never written, and never addressed except by the one bump
this exists to keep in bounds, so no encoded byte can move. Deleting the term
restores the UB and the live encoder probe catches it.

**S12 met in production for the second time**, and the rule's own sentence covers
it: a raw kernel's pointer footprint is bigger than its write footprint. F14 was
the first, in `svc_encode_slice.rs`; this is the second, and both were found by
the same instrument the first time it was pointed at the code.

---

## F58 — the encoder reads a never-written reference picture's visible luma

**Status: FIXED 2026-08-18 (Phase 6 session A), by accommodation.** Found by the
encoder probe at `processing/vaacalc.rs:307`, three fixes after F57.

### What it is

`AllocPicture` (`wels_preprocess.rs`) takes the picture's sample buffer from
`WelsMalloc`, **not** `WelsMallocz` — faithfully, because the C++ does the same
(`codec/encoder/core/src/picture_handle.cpp:76`). On the **first** frame,
`AnalyzeSpatialPic` calls `VaaCalculation` with a reference picture that nothing
has written yet (`wels_preprocess.cpp:289` does this too), and `VAACalcSad` reads
it. Miri reports the read at `pData[0] + 16` — **row 0 of the visible luma**, not
padding — of an 18,432-byte buffer that is uninitialised end to end.

Reading uninitialised memory is indeterminate-but-tolerated for `unsigned char`
in C and **Undefined Behaviour in Rust**. This is the class where a faithful
transliteration cannot stay faithful: the port has to choose a value.

### The fix, and why it cannot move a byte in practice

`WelsMallocz` for `pPic->pBuffer`. Zeroing is the smallest thing that makes the
read defined, and it is what both implementations observe anyway: a fresh 18 KB
`malloc` is served from zero pages, which is why 341/341 has always agreed. The
port becomes deterministic where the C++ is merely lucky, which is the strictly
better half of D-fid-1 — and the sweeps are what say so rather than this
paragraph.

### What it says about the instrument

F58 is not an aliasing defect at all — it is an initialisation defect that only a
whole-program interpreter can see. The probe that found it was built to find
aliasing. **The value of a Miri probe is not the class of bug it was aimed at**,
which is the third time this project has learned that (F1 behind the release
segfault, F14 behind F13's skip, and now this behind F57).

---

## F13's remaining production site — closed

Not a new finding; recorded here because Phase 6 is where it closed.
`phase2_findings.md`'s F13 named four sites and left one open for "Phase 6
(encoder context restructuring)": `InitDqLayers` taking `&mut ...sSliceArgument`
while a live pointer into the same layer was in scope. **The encoder probe
reproduced it exactly on its first execution**, at `encoder_ext.rs:822`, with
`svc_encode_slice.rs`'s `InitSliceInLayer` as the invalidator — a cross-module
pair, which is why no single-module reading had ever found it.

It closed as a family rather than a site, S29's spelling at **20** derivations of
`&mut (*<raw>).sSpatialLayers[i]` / `.sDependencyLayers[i]` across
`encoder_ext.rs`, `paraset_strategy.rs`, `ref_list_mgr_svc.rs`,
`encoder_context.rs`, `wels_encoder_ext.rs` and `wels_preprocess.rs`. The
`--skip encoder_ext` line in `gates.sh` names F13 and **stays** for now: it is a
test-name filter, the production site behind it is fixed, and what remains is the
`encoder_ext` unit tests' own backlog, which 6.6 owns along with deleting the
line. See the session A log entry for the enumeration.
