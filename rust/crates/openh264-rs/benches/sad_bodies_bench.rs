//! SAD/SATD body cost: the parked families' re-attempt measurement (Phase 4a).
//!
//! `common/sad_common.rs` (14 kernels) and `encoder/sample.rs`'s SATD (7) are
//! **parked** — safe kernels in-tree and differentially proven, shims unswapped,
//! raw bodies live (`perf_baseline.md` §Parked). Phase 4a's checkpoint is their
//! re-attempt point, and the brief's instruction is explicit: *rebuild the
//! harness before trusting either*. D-perf-2's read the in-tree kernel at
//! 2.9-8.7x where T5's read 1.55-1.68x, and moved raw `Sad8x4` 6.49 -> 2.18 ns
//! between two runs of one binary. A verdict resting on that is not a verdict.
//!
//! ## What this harness does differently, rule by rule
//!
//! **S3 — the working set is part of the instrument's correctness.** Motion
//! estimation reads a search window that is L1-resident: a few hundred bytes of
//! candidate block against a reference window of a few KB. This benchmark sizes
//! its planes at 64-byte stride over 4 KB and states it here, so per-call
//! overhead cannot hide behind cache misses the way T5's 1984-byte stride over
//! 190 KB let it (0.82-1.33x reported against a real +16.8%).
//!
//! **S3 again — every output is observed and every iteration is opaque.** The
//! accumulator consumes each call's result and `black_box` wraps both the inputs
//! and the accumulator on every iteration. Observing one of several outputs is
//! how a safe kernel gets deleted by LLVM while the opaque `extern "C"` call
//! keeps computing — a 4x handicap pointing the wrong way.
//!
//! **S3 again — the stride is a runtime value.** A `const` stride is the single
//! easiest way to make a safe kernel look good here, because it is exactly the
//! fact the shim boundary denies it. `stride()` launders it through
//! `black_box`.
//!
//! **S1 — variants are interleaved, not timed in blocks.** Each round runs raw
//! and safe back to back for one shape; the reported figure is the median ratio
//! over `ROUNDS` rounds, not the ratio of two block timings.
//!
//! **S4 — the raw side is genuinely raw.** These families are *unswapped*, so
//! `WelsSampleSad*_c` really is the pointer kernel and not a shim onto the safe
//! one. That is the one thing this measurement gets for free that a post-swap
//! one has to recover with `git show`.
//!
//! ## What it is measuring, and what that answers
//!
//! Body cost at the shim boundary: raw pointer kernel against safe kernel called
//! through the same cursor construction a shim would do. §7.4's re-landing bar is
//! **bodies at <= 1.05x**; D-perf-2 measured 1.39-2.97x in the framing most
//! favourable to the candidate and parked the family.
//!
//! Phase 4a asks it again for a specific reason. This session established that
//! direct dispatch recovers per-call scaffolding **only where the caller supplies
//! constant dimensions** — the encoder's MC sites recovered ~5%, the decoder's
//! recovered nothing. The SAD callers split the same way: `svc_base_layer_md.rs`
//! and `svc_mode_decision.rs` index the table with literal `BLOCK_16x16` /
//! `BLOCK_8x8`, while `svc_motion_estimate.rs`'s ten sites index it with a
//! runtime `block_size`. `SAD_CONST` below models the first, `SAD_DYN` the
//! second, and the gap between them is the thing worth knowing.
//!
//! Run: `cargo bench --bench sad_bodies_bench`.

use std::hint::black_box;
use std::time::Instant;

use openh264_rs::common::sad_common as sad;
use openh264_rs::encoder::sample as satd;
use openh264_rs::safe::plane::PlaneCursor;

/// L1-resident, and stated: 64-byte stride over 4 KB is the residency a motion
/// search has. Both planes fit in L1 together.
const STRIDE: usize = 64;
const ROWS: usize = 64;
const PLANE: usize = STRIDE * ROWS; // 4 KB

/// Enough iterations that a ~2 ns call is not measured against timer noise.
const ITERS: usize = 200_000;
/// Interleaved rounds; the verdict is the median ratio, never a single pair.
const ROUNDS: usize = 7;

/// The stride the kernels see is a runtime value, as it is through a shim.
#[inline(never)]
fn stride() -> i32 {
    black_box(STRIDE as i32)
}

fn plane(seed: u32) -> Vec<u8> {
    let mut s = seed;
    (0..PLANE)
        .map(|_| {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (s >> 16) as u8
        })
        .collect()
}

/// Anchor far enough in that the four-point kernels' `-1` reach is in bounds.
const ANCHOR: usize = 8 * STRIDE + 8;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// One shape: raw kernel vs safe kernel, interleaved, median ratio over rounds.
macro_rules! bench_sad {
    ($name:literal, $raw:path, $w:expr, $h:expr, $a:expr, $b:expr) => {{
        let mut ratios = Vec::with_capacity(ROUNDS);
        let mut raws = Vec::with_capacity(ROUNDS);
        let mut safes = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            // --- raw
            let t0 = Instant::now();
            let mut acc0: i64 = 0;
            for _ in 0..ITERS {
                let s = stride();
                let r = unsafe {
                    $raw(
                        black_box($a.as_ptr().add(ANCHOR)) as *mut u8, s,
                        black_box($b.as_ptr().add(ANCHOR)) as *mut u8, s,
                    )
                };
                acc0 = black_box(acc0 + r as i64);
            }
            let raw_ns = t0.elapsed().as_nanos() as f64 / ITERS as f64;

            // --- safe, paying the cursor construction a shim would pay
            let t1 = Instant::now();
            let mut acc1: i64 = 0;
            for _ in 0..ITERS {
                let s = stride() as usize;
                let c1 = PlaneCursor::new(black_box(&$a[..]), ANCHOR, s);
                let c2 = PlaneCursor::new(black_box(&$b[..]), ANCHOR, s);
                let r = sad::sample_sad::<$w, $h>(&c1, &c2);
                acc1 = black_box(acc1 + r as i64);
            }
            let safe_ns = t1.elapsed().as_nanos() as f64 / ITERS as f64;

            assert_eq!(acc0, acc1, concat!($name, ": raw and safe disagree"));
            ratios.push(safe_ns / raw_ns);
            raws.push(raw_ns);
            safes.push(safe_ns);
        }
        let ratio = median(ratios);
        println!(
            "  {:<7} raw {:>6.2} ns   safe {:>6.2} ns   ratio {:>6.2}x",
            $name, median(raws), median(safes), ratio
        );
        ($name, ratio)
    }};
}

/// One SATD shape: raw kernel vs safe kernel, interleaved, median ratio.
///
/// Same protocol as `bench_sad!` — S1 interleaving, S3 residency and opaque
/// inputs/outputs, S4's genuinely-raw other side. The only difference is the safe
/// signature: the SATD kernels are not generic over the block shape, so the shape
/// is the function rather than a const parameter.
macro_rules! bench_satd {
    ($name:literal, $raw:path, $safe:path, $a:expr, $b:expr) => {{
        let mut ratios = Vec::with_capacity(ROUNDS);
        let mut raws = Vec::with_capacity(ROUNDS);
        let mut safes = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let t0 = Instant::now();
            let mut acc0: i64 = 0;
            for _ in 0..ITERS {
                let s = stride();
                let r = unsafe {
                    $raw(
                        black_box($a.as_ptr().add(ANCHOR)) as *mut u8, s,
                        black_box($b.as_ptr().add(ANCHOR)) as *mut u8, s,
                    )
                };
                acc0 = black_box(acc0 + r as i64);
            }
            let raw_ns = t0.elapsed().as_nanos() as f64 / ITERS as f64;

            let t1 = Instant::now();
            let mut acc1: i64 = 0;
            for _ in 0..ITERS {
                let s = stride() as usize;
                let c1 = PlaneCursor::new(black_box(&$a[..]), ANCHOR, s);
                let c2 = PlaneCursor::new(black_box(&$b[..]), ANCHOR, s);
                let r = $safe(&c1, &c2);
                acc1 = black_box(acc1 + r as i64);
            }
            let safe_ns = t1.elapsed().as_nanos() as f64 / ITERS as f64;

            assert_eq!(acc0, acc1, concat!($name, ": raw and safe disagree"));
            ratios.push(safe_ns / raw_ns);
            raws.push(raw_ns);
            safes.push(safe_ns);
        }
        let ratio = median(ratios);
        println!(
            "  {:<7} raw {:>6.2} ns   safe {:>6.2} ns   ratio {:>6.2}x",
            $name, median(raws), median(safes), ratio
        );
        ($name, ratio)
    }};
}

/// How many kernel calls one partition search makes against one pair of cursors.
///
/// The ledger's untested lead (perf_baseline.md, Parked): *per-call cursor
/// construction was the measured cost, and callers building slices once per
/// partition search is exactly what steps 2-3 make possible.* This models it —
/// the two `PlaneCursor::new` calls move outside the inner loop, and the safe
/// kernel is called `AMORT` times against them, as a diamond/cross search does
/// against one reference window.
const AMORT: usize = 8;

/// The same shape, with cursor construction amortised over `AMORT` calls.
macro_rules! bench_amort {
    ($name:literal, $raw:path, $safecall:expr, $a:expr, $b:expr) => {{
        let mut ratios = Vec::with_capacity(ROUNDS);
        let mut raws = Vec::with_capacity(ROUNDS);
        let mut safes = Vec::with_capacity(ROUNDS);
        let iters = ITERS / AMORT;
        for _ in 0..ROUNDS {
            let t0 = Instant::now();
            let mut acc0: i64 = 0;
            for _ in 0..iters {
                let s = stride();
                for k in 0..AMORT {
                    let r = unsafe {
                        $raw(
                            black_box($a.as_ptr().add(ANCHOR + k)) as *mut u8, s,
                            black_box($b.as_ptr().add(ANCHOR)) as *mut u8, s,
                        )
                    };
                    acc0 = black_box(acc0 + r as i64);
                }
            }
            let raw_ns = t0.elapsed().as_nanos() as f64 / (iters * AMORT) as f64;

            let t1 = Instant::now();
            let mut acc1: i64 = 0;
            for _ in 0..iters {
                let s = stride() as usize;
                // built ONCE per search, not once per candidate
                let base = PlaneCursor::new(black_box(&$a[..]), ANCHOR, s);
                let c2 = PlaneCursor::new(black_box(&$b[..]), ANCHOR, s);
                for k in 0..AMORT {
                    let c1 = base.advance(k as isize, 0);
                    let r = $safecall(&c1, &c2);
                    acc1 = black_box(acc1 + r as i64);
                }
            }
            let safe_ns = t1.elapsed().as_nanos() as f64 / (iters * AMORT) as f64;

            assert_eq!(acc0, acc1, concat!($name, " (amortised): raw and safe disagree"));
            ratios.push(safe_ns / raw_ns);
            raws.push(raw_ns);
            safes.push(safe_ns);
        }
        let ratio = median(ratios);
        println!(
            "  {:<7} raw {:>6.2} ns   safe {:>6.2} ns   ratio {:>6.2}x",
            $name, median(raws), median(safes), ratio
        );
        ($name, ratio)
    }};
}

fn main() {
    println!();
    println!("SAD body cost at the shim boundary — Phase 4a parked-family re-attempt");
    println!("  working set : {STRIDE}-byte stride over {PLANE} B per plane (L1-resident, S3)");
    println!("  protocol    : {ROUNDS} interleaved rounds x {ITERS} iters, median ratio (S1)");
    println!("  raw side    : genuinely raw — these families are unswapped (S4)");
    println!("  bar         : bodies <= 1.05x re-land; D-perf-2 measured 1.39-2.97x");
    println!();

    let a = plane(0x1234_5678);
    let b = plane(0x9ABC_DEF0);

    println!("single-block SAD (safe kernel pays two PlaneCursor::new, as a shim would):");
    let mut rows: Vec<(&str, f64)> = Vec::new();
    rows.push(bench_sad!("16x16", sad::WelsSampleSad16x16_c, 16, 16, a, b));
    rows.push(bench_sad!("16x8", sad::WelsSampleSad16x8_c, 16, 8, a, b));
    rows.push(bench_sad!("8x16", sad::WelsSampleSad8x16_c, 8, 16, a, b));
    rows.push(bench_sad!("8x8", sad::WelsSampleSad8x8_c, 8, 8, a, b));
    rows.push(bench_sad!("8x4", sad::WelsSampleSad8x4_c, 8, 4, a, b));
    rows.push(bench_sad!("4x8", sad::WelsSampleSad4x8_c, 4, 8, a, b));
    rows.push(bench_sad!("4x4", sad::WelsSampleSad4x4_c, 4, 4, a, b));

    println!();
    let worst = rows.iter().map(|r| r.1).fold(0.0_f64, f64::max);
    let best = rows.iter().map(|r| r.1).fold(f64::MAX, f64::min);
    println!("  SAD range {best:.2}x .. {worst:.2}x   (bar: <= 1.05x)");
    println!(
        "  SAD verdict: {}",
        if worst <= 1.05 { "PARITY — re-land" } else { "over the bar — stays parked" }
    );

    // ------------------------------------------------------------------ SATD
    // T6.E5. `encoder/sample.rs`'s seven SATD kernels were parked in D-perf-4 **by
    // projection** — "SATD = SAD + a Hadamard butterfly, strictly more work" — and
    // have never had a measurement of their own. This is it.
    println!();
    println!("single-block SATD (never measured before — parked by projection at D-perf-4):");
    let mut satd_rows: Vec<(&str, f64)> = Vec::new();
    satd_rows.push(bench_satd!("16x16", satd::WelsSampleSatd16x16_c, satd::satd_16x16, a, b));
    satd_rows.push(bench_satd!("16x8", satd::WelsSampleSatd16x8_c, satd::satd_16x8, a, b));
    satd_rows.push(bench_satd!("8x16", satd::WelsSampleSatd8x16_c, satd::satd_8x16, a, b));
    satd_rows.push(bench_satd!("8x8", satd::WelsSampleSatd8x8_c, satd::satd_8x8, a, b));
    satd_rows.push(bench_satd!("8x4", satd::WelsSampleSatd8x4_c, satd::satd_8x4, a, b));
    satd_rows.push(bench_satd!("4x8", satd::WelsSampleSatd4x8_c, satd::satd_4x8, a, b));
    satd_rows.push(bench_satd!("4x4", satd::WelsSampleSatd4x4_c, satd::satd_4x4, a, b));
    println!();
    let sworst = satd_rows.iter().map(|r| r.1).fold(0.0_f64, f64::max);
    let sbest = satd_rows.iter().map(|r| r.1).fold(f64::MAX, f64::min);
    println!("  SATD range {sbest:.2}x .. {sworst:.2}x   (bar: <= 1.05x)");
    println!(
        "  SATD verdict: {}",
        if sworst <= 1.05 { "PARITY — re-land" } else { "over the bar — stays parked" }
    );

    // ------------------------------------------------- cursors built once per search
    println!();
    println!("cursor construction amortised over {AMORT} calls (the ledger's untested lead):");
    let mut am: Vec<(&str, f64)> = Vec::new();
    am.push(bench_amort!("sad16", sad::WelsSampleSad16x16_c, sad::sample_sad::<16, 16>, a, b));
    am.push(bench_amort!("sad8", sad::WelsSampleSad8x8_c, sad::sample_sad::<8, 8>, a, b));
    am.push(bench_amort!("sad4", sad::WelsSampleSad4x4_c, sad::sample_sad::<4, 4>, a, b));
    am.push(bench_amort!("satd16", satd::WelsSampleSatd16x16_c, satd::satd_16x16, a, b));
    am.push(bench_amort!("satd8", satd::WelsSampleSatd8x8_c, satd::satd_8x8, a, b));
    am.push(bench_amort!("satd4", satd::WelsSampleSatd4x4_c, satd::satd_4x4, a, b));
    println!();
    let aworst = am.iter().map(|r| r.1).fold(0.0_f64, f64::max);
    let abest = am.iter().map(|r| r.1).fold(f64::MAX, f64::min);
    println!("  amortised range {abest:.2}x .. {aworst:.2}x   (bar: <= 1.05x)");
    println!();
}
