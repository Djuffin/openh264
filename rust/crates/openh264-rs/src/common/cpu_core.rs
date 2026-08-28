#![allow(non_upper_case_globals)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! CPU feature flags — `codec/common/inc/cpu_core.h`.
//!
//! The single definition for every `WELS_CPU_*` bit. Eight modules used to
//! declare their own subsets and they did **not** agree: `WELS_CPU_NEON` had
//! seven distinct values across eight copies, `WELS_CPU_LSX` five across six,
//! `WELS_CPU_AVX`/`AVX2`/`SSE42`/`SSSE3`/`SSE41`/`FMA`/`MMI`/`MSA` three or four
//! each. Every one of them is a mask tested against `WelsCPUFeatureDetect`'s
//! result, which is `0x00000000` on this target, so the wrong values are dead
//! today — but the first SIMD dispatch that goes live would have selected
//! kernels at random.
//!
//! The values below are `cpu_core.h` verbatim, including the deliberate reuse
//! across architectures (`WELS_CPU_NEON` and `WELS_CPU_SSE` are both `0x4`,
//! `WELS_CPU_MMI` and `WELS_CPU_MMX` are both `0x1`): the header namespaces them
//! by target, not by value, and so does this module.

pub const WELS_CPU_MMX: u32 = 0x00000001;
pub const WELS_CPU_MMXEXT: u32 = 0x00000002;
pub const WELS_CPU_SSE: u32 = 0x00000004;
pub const WELS_CPU_SSE2: u32 = 0x00000008;
pub const WELS_CPU_SSE3: u32 = 0x00000010;
pub const WELS_CPU_SSE41: u32 = 0x00000020;
pub const WELS_CPU_3DNOW: u32 = 0x00000040;
pub const WELS_CPU_3DNOWEXT: u32 = 0x00000080;
pub const WELS_CPU_ALTIVEC: u32 = 0x00000100;
pub const WELS_CPU_SSSE3: u32 = 0x00000200;
pub const WELS_CPU_SSE42: u32 = 0x00000400;

/* CPU features application extensive */
pub const WELS_CPU_FPU: u32 = 0x00001000;
pub const WELS_CPU_HTT: u32 = 0x00002000;
pub const WELS_CPU_CMOV: u32 = 0x00004000;
pub const WELS_CPU_MOVBE: u32 = 0x00008000;
pub const WELS_CPU_AES: u32 = 0x00010000;
pub const WELS_CPU_FMA: u32 = 0x00020000;
pub const WELS_CPU_AVX: u32 = 0x00000800;

/// `cpu_core.h:71-75` — `0x00040000` under `HAVE_AVX2`, otherwise `0`. The
/// library build this port tracks does not define `HAVE_AVX2`; the value is kept
/// at the defined one so the bit stays distinguishable, and no dispatch reads it
/// on this target.
pub const WELS_CPU_AVX2: u32 = 0x00040000;

pub const WELS_CPU_AVX512F: u32 = 0x00080000;
pub const WELS_CPU_AVX512CD: u32 = 0x00100000;
pub const WELS_CPU_AVX512DQ: u32 = 0x00200000;
pub const WELS_CPU_AVX512BW: u32 = 0x00400000;
pub const WELS_CPU_AVX512VL: u32 = 0x00800000;

pub const WELS_CPU_CACHELINE_16: u32 = 0x10000000;
pub const WELS_CPU_CACHELINE_32: u32 = 0x20000000;
pub const WELS_CPU_CACHELINE_64: u32 = 0x40000000;
pub const WELS_CPU_CACHELINE_128: u32 = 0x80000000;

/* For the android OS */
pub const WELS_CPU_ARMv7: u32 = 0x000001;
pub const WELS_CPU_VFPv3: u32 = 0x000002;
pub const WELS_CPU_NEON: u32 = 0x000004;

/* For loongson */
pub const WELS_CPU_MMI: u32 = 0x00000001;
pub const WELS_CPU_MSA: u32 = 0x00000002;
pub const WELS_CPU_LSX: u32 = 0x00000003;
pub const WELS_CPU_LASX: u32 = 0x00000004;

#[cfg(test)]
mod tests {
    use super::*;

    /// Values transcribed from `codec/common/inc/cpu_core.h:46-98`.
    #[test]
    fn test_cpu_flags_match_cpu_core_h() {
        assert_eq!(WELS_CPU_MMX, 0x00000001);
        assert_eq!(WELS_CPU_MMXEXT, 0x00000002);
        assert_eq!(WELS_CPU_SSE, 0x00000004);
        assert_eq!(WELS_CPU_SSE2, 0x00000008);
        assert_eq!(WELS_CPU_SSE3, 0x00000010);
        assert_eq!(WELS_CPU_SSE41, 0x00000020);
        assert_eq!(WELS_CPU_SSSE3, 0x00000200);
        assert_eq!(WELS_CPU_SSE42, 0x00000400);
        assert_eq!(WELS_CPU_AVX, 0x00000800);
        assert_eq!(WELS_CPU_FMA, 0x00020000);
        assert_eq!(WELS_CPU_AVX2, 0x00040000);
        assert_eq!(WELS_CPU_NEON, 0x000004);
        assert_eq!(WELS_CPU_MMI, 0x00000001);
        assert_eq!(WELS_CPU_MSA, 0x00000002);
        assert_eq!(WELS_CPU_LSX, 0x00000003);
        assert_eq!(WELS_CPU_LASX, 0x00000004);
    }
}
