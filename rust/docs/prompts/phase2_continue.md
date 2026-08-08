# Session prompt — Safety refactor, Phase 2 continuation (T4 → T9)

You are continuing **Phase 2** of `rust/docs/safety_refactor_plan.md`.
**`prompts/phase2.md` remains the governing brief** — recipe (§3.1 two commits per
family), naming/shim conventions (§3.2), perf protocol (§3.3), gates (§5), non-goals
(§6) all apply unchanged. This file is the continuation state: what's done, what the
first sessions learned that you inherit as *rules*, and the remaining task list with
its carry-ins. Where this file and `phase2.md` disagree, this file is newer.

State as of **`34ad2eb4`** (tree clean, full battery green):

| | |
|---|---|
| Done | Preconditions (Phase 0 T5c/T5e/T6 — **Phase 0 is now complete except T7-fuzz, deferred by Eugene's direction**), T1 control, T2 pilot (`decoder/decode_mb_aux.rs`, 4 kernels, −1.3…1.8% — a win), T3 (`decoder/get_intra_predictor.rs`, 42 kernels, ±1% wash) |
| Remaining | T4 `mc.rs` → T5 `sad_common`+`intra_pred_common` → T6 `deblocking_common`+F1+expand → T7 four encoder files → T8 processing → T9 exit — ~74 of ~120 kernels, including the phase's two hardest items (both in T6) |
| Instruments | `rust/tools/gates.sh` + ratchet exist and are the workflow; gates.sh prints the F3 retry rule and a T7-SKIP line every run |
| Counts | tests 375 debug / 373 release / 20 ignored; sweeps 341/341 both profiles; ratchet at `34ad2eb4`: `unsafe_fn` 1365, `unsafe_block` 549, `raw_ptr` 5212, `SHIM(` 46, `no_mangle` 38 |

Suggested split of the remaining 3–4 sessions: **S1 = T4 + T5; S2 = T6 alone** (the
likeliest overrun — don't pair it with anything); **S3 = T7; S4 = T8 + T9** (fold into
S3 if room). Clean stops anywhere, per the standing exit protocol.

---

## 1. Read first

1. `rust/docs/safety_refactor_log.md` — the Phase 2 entry **in full**: the pilot retro
   verdicts, T3's four carry-forwards (restated below as rules), and the T4 carry-in
   list. This is the highest-density context you have.
2. `rust/docs/phase2_findings.md` — **F8** (the 8×8 IDCT's `i16` intermediates overflow;
   debug panics where the C++ wraps; pre-existing, open, unreachable on conformant
   streams). It generalizes — see rule R-e below.
3. `rust/docs/prompts/phase2.md` — re-skim §3 (recipe/conventions/perf) and your
   families' entries in §4; the per-family traps written there still hold.
4. `perf_baseline.md` — the Phase 2 T2/T3 rows are your evidence base; Phase 0 anchors
   are never overwritten.
5. Plan `## Progress` appendix — check off against reality before starting.

## 2. Rules the first sessions established (inherit, don't re-derive)

- **R-a — The perf mechanism, and why the budget is holding:** bounds checks land
  **per row, not per sample** (one `row_mut` per output row amortizes over the row's
  arithmetic), and `copy_from_slice`/`fill` on fixed windows compile to the same wide
  stores the punned `u32`/`u64` accesses existed for. Structure every remaining kernel
  this way first; measure second. §9's decoder-side perf risk is downgraded on T3's
  evidence — **the encoder side has had no measurement yet**; T4 (shared consumer) and
  T7 (encoder-only) are where that claim gets tested.
- **R-b — Punned accesses are almost always byte *moves*.** In 140 T3 accesses,
  `from_ne_bytes` was needed **zero** times — `fill(v)` and `copy_from_slice` covered
  everything. Check for genuine arithmetic use before reaching for `from_ne_bytes`;
  value-level tricks (`0x01010101u32.wrapping_mul(v)`) stay as-is.
- **R-c — The shim-span helper is the pattern.** T3's shims aren't derivable from the
  signature alone; one helper computes the slice span (`centre`, `len`, anchor,
  `top_reach`) and *nothing else does that arithmetic*. Reuse the helper approach per
  family; the contract sentence naming **why** the negative reach is legal
  (`PADDING_LENGTH`, or the caller's MV clamp) is what Phase 5 converts callers
  against — that sentence is the deliverable.
- **R-d — Differential-test discipline:** sweep availability/selector flags
  **exhaustively** (T3 found kernels that ignore a flag they take — a random sweep
  blurs exactly what a conversion gets wrong); anchor test blocks at **random legal
  offsets** (kernels written around unaligned stores must be exercised unaligned);
  compare **every written byte of the destination surface**, not the nominal block.
- **R-e — Arithmetic parity, the F8 rule (new, binding for T4/T7 especially):** when a
  kernel's intermediates can overflow, the safe kernel reproduces the **old Rust
  port's exact behavior** — same integer widths, same operations, which means the same
  debug-panic exposure the old code already has and the same release wrapping. Never
  widen, never add `wrapping_*` the old code lacked, never "fix" it — that is a
  behavior change on malformed input and it belongs to P13, later. Newly *noticed*
  overflow-capable intermediates get an F-finding (F8's format) and nothing else.
  Expect candidates in T4's 6-tap filter sums and T7's DCT/quant/SATD arithmetic —
  check the widths against both the old Rust and the C++ before writing the safe body,
  and bound differential-test inputs to the in-contract range where the old code
  doesn't panic (F7/F8 precedent).
- **R-f — Ratchet arithmetic during the strangler phase** (as re-characterized in the
  log, not as `phase2.md` loosely implied): `raw_ptr` is the metric that must fall and
  is strictly non-increasing per commit; `no_mangle` non-increasing; `SHIM(` rises
  only in a family's commit B; `unsafe_block` may rise **only by that commit's shim
  count** (each counted `from_raw_parts` replaces formerly-invisible derefs);
  `unsafe_fn` stays ~flat until Phase 5 deletes shims. Judge every commit against
  that shape; anything off-shape is a mistake, not noise.
- **R-g — F3 protocol, as practiced:** one release-`mt` hit at `t=4 sm=3` → re-run
  rather than argue, even when the change is structurally unrelated. More than one hit
  in a session → equal-count sweeps at HEAD vs the control commit, and append the
  measurement to F3. Any other config, any debug hit, any `st`/`def` hit: real, stop,
  revert, investigate. And log any moment where the missing fuzzer (T7-skip) would
  plausibly have caught something first — F8 is the first such entry; accumulation is
  the signal to re-raise T7 with Eugene.

## 3. Remaining tasks

### T4 — `common/mc.rs` (next action; carry-ins from the log, verbatim)
26 kernels + `InitMcFunc`; 6-tap Wiener filters; consumers in decoder MC/EC **and**
encoder MD/ME — the first family with encoder-side exposure, so record the encoder
signal (sweep wall time; `FFMPEG` bench if set) alongside `decode_1080p_bench`.
Carry-ins: (1) safe kernels take a **`PlaneCursor` (read) + `PlaneCursorMut` (write)
pair** — reference and current are different allocations; (2) the shim contract states
the **MV clamp verbatim** (`decode_slice.rs:1072-1091`) — unlike intra, legality comes
from the caller's clamp, and that clamp is the entire safety argument; (3) fold the
module-internal `pWelsMcFunc_c: [[fn; 4]; 4]` quarter-pel table into a `match` or a
table of **safe** fn pointers — the one dispatch table this phase may touch. R-e check:
the 6-tap sums' intermediate widths.

### T5 — `common/sad_common.rs` + `common/intra_pred_common.rs`
Per `phase2.md` §T5 unchanged: the `Four` SAD kernels read **outside the nominal
block** (`-stride`, `-1`, `+1`) — plane cursors, not block slices, and the
differential must cover the edge reads; absorb the three pre-existing unused
`sample_sad_*` safe wrappers into the new kernels (one safe SAD API, not two);
consumers are `encoder/sample.rs`'s installers + one direct call in
`processing/scene_change_detection.rs:103`. intra_pred_common is two 3-arg kernels —
mind the same-name-different-arity collision with the decoder file you already
converted.

### T6 — `deblocking_common` + the F1 surgery + expansion (schedule alone)
The phase's two hardest items live here; both have full briefs in `phase2.md` §T6 —
follow them. Compressed reminders: deblocking's `(iStrideX, iStrideY)` swap **is** the
V/H encoding — port as explicit `(step_x, step_y)` with the `-3*step` reach in the
contract; the decoder's untyped double-cast installer stays (Phase 4). The **F1 uiBS
surgery** (`encoder/deblocking.rs`): real type `[[[u8; 4]; 4]; 2]` end-to-end, the five
`from_raw_parts_mut(…, 32)` sites deleted, table-membership checked first, diff kept
surgical — it's the one Phase-2 task inside a Phase-6 file. The **expand shims** must
reconstruct the full allocation span from a mid-pointer with the per-variant pad
constant (32 luma / 16 chroma), explicit pad parameter where a call site can't prove
it; the two `mem::transmute` fn-pointer re-wraps stay (Phase 4). R-c applies doubly
here: the expand span helper and its contract sentence are the whole game.

### T7 — the four encoder kernel files (first real encoder perf signal)
`encode_mb_aux.rs`, `encoder/decode_mb_aux.rs`, `sample.rs` (installers keep
installing shims), `encoder/get_intra_predictor.rs` — per `phase2.md` §T7. This is
where R-a's mechanism gets its encoder-side test and where R-e is most likely to bite
(DCT/quant/SATD intermediates). Measure before and after per family: sweep wall time
(control ~21s release) + `FFMPEG` bench when available; if a regression shows up with
no decode-bench equivalent to localize it, bisect by family — that's why the commits
are two-per-family.

### T8 — `processing/{vaacalc,adaptive_quantization}.rs` kernels
Per `phase2.md` §T8: safe signatures with `&[u8]` planes + `&mut` out-slices; extend
the file's two real unit tests; the `IWelsVP` plumbing above is Phase 4's.

### T9 — Phase exit
Per `phase2.md` §T9, plus what's now known: final `SHIM(phase2)` and `no_mangle`
counts recorded (kernels must contribute zero no_mangles; api/ exports remain); full
battery + the Miri protocol (`--lib` + differential files with `scale()`); final
3-run medians and the per-family delta table; **append** a Phase 2 column to
`perf_baseline.md`; Progress appendix updated with hashes; log entry whose
next-action is "Phase 3, decoder read side first — read F4/F5/F7 before touching
anything, and F2 before the write side"; auto-memory updated (Phase 2 complete, where
the shims stand, ratchet shape).

## 4. Gates

Unchanged from `phase2.md` §5, with the R-f ratchet shape and R-g F3 protocol from
this file as the operative refinements. Frozen invariants: the original test suites'
counts, ignored-20, 341/341 both profiles, conformance hashes and frame counts, the
`#[ignore]` set. Per-family bench when on the decode path; sweep timing + `FFMPEG`
bench when on the encode path; session-end full `gates.sh`.

## 5. Non-goals

Everything `phase2.md` §6 lists, plus: no fixing **F8** (R-e is parity, not repair),
no reopening T7-fuzz without Eugene (log the signal instead), no dispatch-table work
beyond the two named module-internal exceptions (mc's quarter-pel fold; the F1
signature surgery), no Phase 3 early start. Surplus time: sharpen shim contracts and
differential edge coverage — T6's contracts especially, since they're the ones Phase 5
will lean on hardest.
