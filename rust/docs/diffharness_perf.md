# Why the debug sweep takes ~142s, and the one-line fix

Investigation only — **nothing is applied**. The tree is stock; this note is the
record so the change can be made deliberately rather than rediscovered.

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

## Expected effect, and what is still unverified

Debug sweep is ~93% `rust_enc`, so 141s should fall to roughly **25-35s** — the same
order as the release sweep's 21s, which is what one would expect once the two profiles
differ only by the checks.

**Not yet done:** run the full `sweep.sh st mt def` under the candidate profile and
confirm **341/341** and the actual wall time. That is the acceptance test and it has
not been run. Apply the profile, run it, and record the real number here before
believing the estimate.

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
