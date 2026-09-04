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

/// Detects available CPU SIMD features at runtime.
///
/// Respects the `OPENH264_NO_SIMD=1` environment variable to force scalar fallbacks
/// for testing and differential verification.
pub fn detect_cpu_features() -> u32 {
    if std::env::var_os("OPENH264_NO_SIMD").is_some() {
        return 0;
    }

    let mut flags = 0u32;

    #[cfg(target_arch = "x86_64")]
    {
        // On x86_64, MMX, SSE, and SSE2 are part of the baseline x86_64 instruction set.
        flags |= WELS_CPU_MMX | WELS_CPU_MMXEXT | WELS_CPU_SSE | WELS_CPU_SSE2;

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
    }

    flags
}
