//! SIMD acceleration kernels and CPU feature detection for openh264-rs.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

use crate::common::cpu_core::*;

/// Detects available CPU SIMD features, once per process.
///
/// Respects the `OPENH264_NO_SIMD=1` environment variable to force scalar fallbacks
/// for testing and differential verification.
///
/// **The answer is latched, and that is the point.** This used to re-read the
/// environment on every call while [`has_sse2`] latched its own copy on the first,
/// so the two could disagree for the rest of the process: a host that decoded one
/// stream, then set `OPENH264_NO_SIMD=1` and opened a second decoder, got
/// `uiCpuFlag == 0` in every table slot while the per-call `has_sse2()` sites kept
/// returning `true` from the stale cache. The four-block residual path
/// (`decode_slice.rs:913/931/1873/2030`, reached through `pIdctFourResAddPredFunc`)
/// ran SSE2 while its single-block sibling ran scalar, on the same picture. Both
/// mechanisms now read this one word, so the switch is all-or-nothing.
///
/// The cost of latching is that the variable is a process-start switch rather than
/// a per-decoder one, which is what it already was in practice — nothing outside
/// this module reads it, and the tables a decoder builds at init never rebuild.
/// **The hot path is a load and a test, and it has to inline.** [`has_sse2`] is on
/// twenty-four per-call dispatch sites (`common/mc.rs`, `common/deblocking_common.rs`,
/// `encoder/decode_mb_aux.rs`, `decoder/decode_mb_aux.rs`), and it is `#[inline(always)]`
/// onto *this* — so if this does not inline, every one of those is a real call. It did
/// not: the one-time initialiser, `std::env::var_os` and all, used to sit in this body,
/// which is exactly the bulk that makes LLVM decline. The initialiser is now
/// [`latch_cpu_features`], `#[cold]` and `#[inline(never)]`, leaving this small enough
/// to fold into its callers.
#[inline]
pub fn detect_cpu_features() -> u32 {
    let cached = CPU_FEATURES.load(Ordering::Relaxed);
    if cached != 0 {
        return cached & !CPU_FEATURES_LATCHED;
    }
    latch_cpu_features()
}

/// Runs once per process; see [`detect_cpu_features`] for why it is out of line.
#[cold]
#[inline(never)]
fn latch_cpu_features() -> u32 {
    let flags = if std::env::var_os("OPENH264_NO_SIMD").is_some() {
        0
    } else {
        arch_cpu_features()
    };
    // Racing callers compute the same word from the same inputs, so the store is
    // idempotent and needs no compare-exchange.
    CPU_FEATURES.store(flags | CPU_FEATURES_LATCHED, Ordering::Relaxed);
    flags
}

/// The x86_64 feature probe. MMX, SSE and SSE2 are part of the baseline x86_64
/// instruction set, so those bits are unconditional.
#[cfg(target_arch = "x86_64")]
fn arch_cpu_features() -> u32 {
    let mut flags = WELS_CPU_MMX | WELS_CPU_MMXEXT | WELS_CPU_SSE | WELS_CPU_SSE2;

    if std::is_x86_feature_detected!("sse3") {
        flags |= WELS_CPU_SSE3;
    }
    if std::is_x86_feature_detected!("ssse3") {
        flags |= WELS_CPU_SSSE3;
    }
    if std::is_x86_feature_detected!("sse4.1") {
        flags |= WELS_CPU_SSE41;
    }
    if std::is_x86_feature_detected!("sse4.2") {
        flags |= WELS_CPU_SSE42;
    }
    if std::is_x86_feature_detected!("avx") {
        flags |= WELS_CPU_AVX;
    }
    if std::is_x86_feature_detected!("avx2") {
        flags |= WELS_CPU_AVX2;
    }
    if std::is_x86_feature_detected!("fma") {
        flags |= WELS_CPU_FMA;
    }

    flags
}

/// No SIMD kernels are translated for this architecture, so no feature bit is
/// ever set and every dispatch site takes its scalar fallback. Split out per
/// arch rather than `#[cfg]`-ing a block inside one function, so that `flags`
/// is only `mut` where it is actually mutated (`lib.rs` denies `unused_mut`).
#[cfg(not(target_arch = "x86_64"))]
fn arch_cpu_features() -> u32 {
    0
}

use std::sync::atomic::{AtomicU32, Ordering};

/// Bit 31, which no `WELS_CPU_*` flag uses (the highest is
/// `WELS_CPU_CACHELINE_64 = 0x4000_0000`), marks the word as computed — so that a
/// genuine all-scalar answer of `0` is distinguishable from "not yet asked".
const CPU_FEATURES_LATCHED: u32 = 1 << 31;

/// The process-wide feature word. See [`detect_cpu_features`] for why it is latched.
static CPU_FEATURES: AtomicU32 = AtomicU32::new(0);

/// Returns true if SSE2 is supported and not disabled by `OPENH264_NO_SIMD=1`.
#[inline(always)]
pub fn has_sse2() -> bool {
    (detect_cpu_features() & WELS_CPU_SSE2) != 0
}

/// Returns true if AVX2 is supported and not disabled by `OPENH264_NO_SIMD=1`.
///
/// Unlike SSE2 this is not x86_64 baseline, so it is a real runtime question: the
/// AVX2 SAD kernels execute `vpsadbw` and fault on any pre-Haswell Intel or
/// pre-Excavator AMD part.
///
/// # Why the `cfg!` is there and why it is not the whole answer
///
/// `cfg!(target_feature = "avx2")` is true only when the *crate* was compiled with
/// `-C target-feature=+avx2` or `-C target-cpu=native`, which is not the default on any
/// `x86_64-*` target — `rustc --print cfg` lists `sse`, `sse2`, `sse3`, `fxsr` and
/// `cmpxchg16b`, and nothing more. When someone does build that way the probe folds to a
/// constant and the branch disappears; otherwise this is the runtime test, unchanged.
///
/// It cannot replace the runtime test. Building with `+avx2` applies to the whole crate,
/// not to the kernels — LLVM auto-vectorises everything else with it too — so the
/// resulting `cdylib`, which is the drop-in a C consumer `dlopen`s, would fault on any
/// pre-Haswell part. One binary, unknown CPU, is the shape of the problem
/// `WelsCPUFeatureDetect` exists to solve upstream. Per-function AVX2 codegen is
/// `#[target_feature(enable = "avx2")]`, which `sad_16x_avx2` already carries.
///
/// One consequence to know: on such a build this answers `true` without consulting
/// `OPENH264_NO_SIMD`. That is consistent — a `+avx2` binary is AVX2 throughout, so the
/// switch could not have made it scalar anyway.
#[inline(always)]
pub fn has_avx2() -> bool {
    cfg!(target_feature = "avx2") || (detect_cpu_features() & WELS_CPU_AVX2) != 0
}
