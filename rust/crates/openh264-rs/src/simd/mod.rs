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

#[cfg(feature = "wide")]
pub mod wide;

/// **The kernel set the dispatch sites call.** Every `has_sse2()` arm and every
/// `WELS_CPU_SSE2` table install spells its kernel as `crate::simd::kernels::…`, and
/// this alias decides what that resolves to: the hand-written `core::arch` intrinsics
/// in [`x86_64`] by default, or the `wide`-crate kernels in [`wide`] under
/// `--features wide`. The two modules export the same names with the same signatures,
/// so the sites do not change — only this line does.
///
/// Both modules are compiled whenever they can be; the feature only moves the alias.
/// That is what lets `benches/kernel_bench.rs` time the three implementations of one
/// kernel in one process.
#[cfg(all(target_arch = "x86_64", not(feature = "wide")))]
pub use x86_64 as kernels;
#[cfg(feature = "wide")]
pub use wide as kernels;

use crate::common::cpu_core::*;

/// Detects available CPU SIMD features, once per process.
///
/// Respects `OPENH264_NO_SIMD=1`, which forces scalar fallbacks for differential
/// verification. Latching makes it a process-start switch: every dispatch site reads
/// this one word, so the switch is all-or-nothing rather than half-applied.
///
/// Keep the body this small. [`has_sse2`] is `#[inline(always)]` onto it from
/// twenty-four per-call dispatch sites, and it only folds into them because the
/// one-time initialiser lives out of line in [`latch_cpu_features`].
#[inline]
pub fn detect_cpu_features() -> u32 {
    // `Acquire` here pairs with the `Release` in `latch_cpu_features`, so a thread that
    // sees the ready flag also sees the word stored before it was set.
    if CPU_FEATURES_READY.load(Ordering::Acquire) {
        return CPU_FEATURES.load(Ordering::Relaxed);
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
    // Racing callers compute the same word from the same inputs, so both stores are
    // idempotent and neither needs a compare-exchange. The word goes first and the flag
    // second, under `Release`, so no reader can see the flag without the word.
    CPU_FEATURES.store(flags, Ordering::Relaxed);
    CPU_FEATURES_READY.store(true, Ordering::Release);
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

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The process-wide feature word, and whether it has been computed yet.
///
/// **Two cells rather than a sentinel bit inside the word.** Every bit of the `u32` is
/// spoken for — `WELS_CPU_CACHELINE_128` is `0x8000_0000` (`common/cpu_core.rs:49`) and
/// upstream sets it for real (`codec/common/src/cpu.cpp:207`) — so a marker inside the
/// word would be masked out of a live flag as soon as `arch_cpu_features` grows to
/// report cache-line size. A separate cell cannot collide with any future flag.
///
/// `0` has to stay a legitimate answer, and is: `arch_cpu_features` returns it on every
/// non-x86_64 target and under `OPENH264_NO_SIMD=1`.
static CPU_FEATURES: AtomicU32 = AtomicU32::new(0);
static CPU_FEATURES_READY: AtomicBool = AtomicBool::new(false);

/// Returns true if SSE2 is supported and not disabled by `OPENH264_NO_SIMD=1`.
#[inline(always)]
pub fn has_sse2() -> bool {
    (detect_cpu_features() & WELS_CPU_SSE2) != 0
}

/// Returns true if AVX2 is supported and not disabled by `OPENH264_NO_SIMD=1`.
///
/// Unlike SSE2 this is not x86_64 baseline, so it is a real runtime question: the AVX2
/// SAD kernels execute `vpsadbw` and fault on any pre-Haswell Intel or pre-Excavator
/// AMD part.
///
/// The `cfg!` folds the branch away for a build that already guarantees AVX2, and is
/// false by default on every `x86_64-*` target. **It cannot replace the runtime test:**
/// `-C target-feature=+avx2` applies to the whole crate, so LLVM would vectorise
/// everything else with it too and the `cdylib` a C consumer `dlopen`s would fault on
/// an older CPU. Per-function AVX2 codegen is `#[target_feature(enable = "avx2")]`,
/// which `sad_16x_avx2` carries. On such a build this answers `true` without consulting
/// `OPENH264_NO_SIMD`, which is consistent — that binary is AVX2 throughout.
#[inline(always)]
pub fn has_avx2() -> bool {
    cfg!(target_feature = "avx2") || (detect_cpu_features() & WELS_CPU_AVX2) != 0
}
