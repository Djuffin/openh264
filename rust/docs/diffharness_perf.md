# Why the debug sweep takes ~142s, and the one-line fix

**Status: APPLIED (2026-08-10), option 1.** `[profile.dev] opt-level = 3` with both
check flags on is in `rust_enc/Cargo.toml`; the debug sweep runs **341/341 in ~24s**
against 141s before. The accepted cost is that the debug sweep now shares release's
exposure to **F3**, so the retry rule was widened to cover `mt` `sm=3` in *both*
profiles — `gates.sh`, `phase2.md` §5, `phase2_continue.md` R-g, and `phase0_findings.md`
F3 (fourth measurement) all say so now.

## Where the time goes

One config of `compare.sh` spawns three processes. Timed interleaved with rotating
order, six rounds, medians, on the smallest clip (`CiscoVT2people_160x96` looped to
20 frames, 160x96):

| component | median ms |
|---|---|
| `cxx_enc` | 7.7 |
| **`rust_enc` (debug)** | **210.0** |
| `rust_enc` (release) | 7.9 |
| `h264dec` (the sanity decode) | 5.3 |
| process spawn floor (`/usr/bin/true`) | 2.6 |

**The debug `rust_enc` is 26.5x slower than the release one and is ~93% of the debug
sweep's wall time.** Everything else — the C++ reference encode, the reference decode,
341 rounds of shell — is the remaining ~7%. Parallelising the sweep is therefore *not*
the first lever, and it carries a real risk: F3 is a load-sensitive race
(`phase0_findings.md`), so running configs concurrently would raise its hit rate and
make the `mt` retry rule noisier.

## Why it is slow

`rust/tools/diffharness/rust_enc/Cargo.toml` declares no `[profile]` at all, so the
debug build is Cargo's default dev profile: **`opt-level = 0`**. The port is a DSP
codec; unoptimised it runs like one.

The useful accident is that `rust_enc` carries an empty `[workspace]` table, so it is
its **own workspace root**. A profile added there governs `rust_enc` *and* its path
dependency `openh264-rs` **without touching `rust/crates/openh264-rs/Cargo.toml`** and
without changing how `cargo test` builds. The change is contained to the tool.

## The candidate, measured

Five binaries built, stashed side by side, then timed interleaved with rotating order
after letting the machine settle (see the caveat below — this mattered):

| profile | median ms | vs stock | output |
|---|---|---|---|
| stock (`opt-level = 0`) | 210.0 | 1.0x | — |
| `opt-level = 1` + checks | 21.7 | 9.7x | byte-identical |
| `opt-level = 2` + checks | 20.3 | 10.4x | byte-identical |
| **`opt-level = 3` + checks** | **14.0** | **15.0x** | byte-identical |
| release (`opt-level = 3`, no checks) | 7.9 | 26.5x | byte-identical |

So:

```toml
[profile.dev]
opt-level = 3
debug-assertions = true
overflow-checks = true
```

The residual 1.8x between the candidate and release is the arithmetic checks
themselves, which are the entire reason the debug sweep exists — that is cost to keep,
not to chase.

`codegen-units` and `incremental` were tried and are **not** the lever: at
`opt-level = 3`, `codegen-units = 16` with `incremental = false` measured 47.2 ms
against plain O3's 48.7 ms in the same (contaminated) batch, i.e. no real difference.

## The checks are provably preserved

This is the part that matters, because the debug sweep's whole job is catching the
F8/F9 class — a C++ signed overflow that the port turns into a debug panic.

* `cargo build -v` on the candidate passes **`-C debug-assertions=on`** and
  **`-C opt-level=3`** to rustc. That is the authoritative answer.
* The `attempt to add with overflow` panic string is present in the candidate binary
  and absent from the release binary.
* Output is byte-identical to the stock debug build on every profile tried.

One false alarm worth recording: a `debug_assert!` message string
(`iDataLengthOfData == 4 …`) is present in the stock binary and **absent** in the O3
one. That is not the assertion being dropped — at `opt-level = 3` LLVM proves that
particular assert and eliminates the unreachable panic, so its string is stripped.
Strings cannot distinguish "eliminated because proven" from "never compiled in"; the
rustc flag check can, and did.

## The acceptance test, first run: FAILED — and what came of it

Applied the profile, rebuilt, ran `RUST_ENC_PROFILE=debug sweep.sh st mt def`:

* **Wall time 141s → 23.9s — a 5.9x speedup**, close to the release sweep's 21s, which
  is what one expects once the profiles differ only by the checks.
* **PASS=339 FAIL=2.** Both failures were `mt … t=4 sm=3 n=600` with the Rust output
  **zero bytes** — F3's exact fingerprint, but **in debug**, where `phase0_findings.md`
  records F3 as 0-in-1200 and where R-g says a hit is "real, stop, revert,
  investigate", never F3.

That first run forced the question below, and Eugene took **option 1**. Re-run after
the rule change, the same sweep is **341/341 in 24.9s**, and the full battery is green.

### What the follow-up measurement says

Alternating the two debug builds in one loop (R-g's protocol for comparing failure
rates, because sequential sampling of a load-sensitive race misleads), `mt` preset,
three rounds each:

| round | fast debug | stock debug |
|---|---|---|
| 1 | 120/120 | 120/120 |
| 2 | 120/120 | 120/120 |
| 3 | 120/120 | 120/120 |

**Not reproduced.** Total fast-debug exposure is 480 `mt` configurations for 2 events,
all of them in the first run; stock debug is clean here and historically 341/341.

That is two events and no reproduction, which is not enough to characterise a rate. It
is also not nothing: both carried F3's precise signature, and the mechanism is
plausible — F3 is a timing/load-sensitive race, the stock debug build is 26.5x slower
than release, and a build that runs at near-release speed plausibly reopens a window
that `opt-level = 0` had been holding shut. The acceptance run also came directly after
several optimising builds, so the machine was at its least quiescent.

### The decision, and what was chosen

Not a technical question — a gate-policy one. **Option 1 was taken, 2026-08-10.**

1. **CHOSEN — take the 5.9x and widen the F3 retry rule to cover `mt` at `sm=3`,
   `t` in {2,4}, short-or-zero output, in both profiles.** Cost, paid knowingly: the
   debug sweep is no longer the one gate with no exceptions; a debug failure at that
   signature is now a retry rather than a stop. Everything else in debug — `st`, `def`,
   any other `mt` configuration — remains an unconditional stop, so the exemption is
   exactly as narrow as F3's own signature and no narrower.
2. **Stay on `opt-level = 0` and keep the 141s.** Cost: every gate run pays two
   minutes, forever, for a property that 480 configurations could not show being
   violated.
3. **Split the difference**: optimised profile for `st`/`qp`/`def`, stock build for
   `mt`. Keeps the deterministic rule where the race lives and takes most of the win
   elsewhere — `mt` is 120 of 341 configurations, so this is roughly a 3x speedup
   rather than 5.9x. Costs a two-build dance in `build.sh` and `compare.sh`.

There is a genuine upside to option 1 worth weighing against its cost: F3 is a
pre-existing, unfixed encoder race that until now only appeared in release builds. A
fast **debug** reproducer — assertions live, symbols present, 6x faster to iterate on
— is a materially better instrument for whoever eventually fixes it than the release
build is.

## A methodology warning for whoever picks this up

Intermediate readings in this investigation were wrong twice, in the same way, and
both times the wrongness looked like a result:

* Timing **immediately after back-to-back optimising builds** inflated everything.
  In that state the release binary measured 39.7 ms; re-measured after settling it is
  7.9 ms. The O2/O3 candidates first measured 47-58 ms and are really 14-20 ms.
* The fix is the same one this phase keeps relearning: **build everything first, stash
  the binaries, let the machine settle, then time them interleaved with rotating
  order.** A number taken right after a build is not a measurement.

## Other levers, sized

* **Skip the `h264dec` sanity decode**: 5.3 ms x 341 ≈ 1.8s. Real but small, and it is
  a genuine check (it catches a stream that is byte-identical to a *broken* reference).
  Not worth losing.
* **Parallelise across configs**: the biggest remaining lever *after* the profile fix,
  since the sweep would then be ~30s of mostly-serial encoder work on a many-core
  machine. Blocked on the F3 interaction above — if it is ever done, the `mt` preset
  should stay serial and only `st`/`qp`/`def` fan out.
