//! Codegen probes for the SIMD kernel pairs: one `#[unsafe(no_mangle)]` wrapper per kernel per
//! implementation, so the emitted assembly can be read function by function.
//!
//! ```text
//! cargo rustc --release --features wide --example simd_probe -- --emit asm
//! ```
//!
//! then read `target/release/examples/simd_probe-*.s`. Not a program worth running;
//! `main` calls each probe once so nothing is dead.
//!
//! Each half is present only where its module is: the `isa` probes off x86_64 have
//! nothing to probe, the `wide` probes need the feature. On aarch64 with
//! `--features wide` this emits the `wide` half alone, which is how you read what
//! those lanes lowered to — NEON there rather than SSE2.

// A build with neither probe set — off x86_64, without `--features wide` — emits no
// probes at all, so main's fixtures go unread. That is the honest outcome for a codegen
// instrument on a target with no kernels to read the codegen of.
#![allow(non_snake_case, unused_imports, unused_variables, unused_mut)]

use openh264_rs::safe::plane::{PlaneCursor, PlaneCursorMut};
#[cfg(target_arch = "x86_64")]
use openh264_rs::simd::x86_64 as isa;
#[cfg(feature = "wide")]
use openh264_rs::simd::wide as wd;

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_sad_16x16(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
    isa::sad::sample_sad_16x16(a, b)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_satd_4x4(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
    isa::satd::satd_4x4(a, b)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_dequant_ihadamard(res: &mut [i16; 16], mf: u16) {
    isa::quant::dequant_ihadamard_4x4(res, mf)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hadamard_t4_dc(out: &mut [i16; 16], dct: &[i16; 241]) {
    isa::quant::hadamard_t4_dc(out, dct)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_quant_4x4(d: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    isa::quant::quant_4x4(d, ff, mf)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_dct_4x4(d: &mut [i16; 16], a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) {
    isa::dct::dct_4x4(d, a, b)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_pixel_avg_16x16(dst: &mut PlaneCursorMut<'_>, a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) {
    isa::mc::pixel_avg(dst, a, b, 16, 16)
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver02_16x16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver02(src, dst, 16, 16)
}

#[cfg(feature = "wide")]
mod wide_probes {
    use super::*;

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_sad_16x16(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
        wd::sad::sample_sad_16x16(a, b)
    }

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_satd_4x4(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
        wd::satd::satd_4x4(a, b)
    }

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_dequant_ihadamard(res: &mut [i16; 16], mf: u16) {
        wd::quant::dequant_ihadamard_4x4(res, mf)
    }

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_hadamard_t4_dc(out: &mut [i16; 16], dct: &[i16; 241]) {
        wd::quant::hadamard_t4_dc(out, dct)
    }

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_quant_4x4(d: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
        wd::quant::quant_4x4(d, ff, mf)
    }

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_dct_4x4(d: &mut [i16; 16], a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) {
        wd::dct::dct_4x4(d, a, b)
    }

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_pixel_avg_16x16(dst: &mut PlaneCursorMut<'_>, a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) {
        wd::mc::pixel_avg(dst, a, b, 16, 16)
    }

    #[unsafe(no_mangle)]
    #[inline(never)]
    pub fn probe_wide_hor_ver02_16x16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
        wd::mc::mc_hor_ver02(src, dst, 16, 16)
    }
}

fn main() {
    let a = vec![7u8; 64 * 64];
    let b = vec![9u8; 64 * 64];
    let mut o = vec![0u8; 64 * 64];
    let (ca, cb) = (PlaneCursor::new(&a, 20 * 64 + 19, 64), PlaneCursor::new(&b, 20 * 64 + 19, 64));
    let mut d = [0i16; 16];
    let mut m = [0i16; 16];
    let big = [0i16; 241];
    let (ff, mf) = ([1i16; 8], [2i16; 8]);
    #[allow(unused_mut)]
    let mut total = 0i32;
    #[cfg(target_arch = "x86_64")]
    {
        total += probe_isa_sad_16x16(&ca, &cb) + probe_isa_satd_4x4(&ca, &cb);
        probe_isa_dequant_ihadamard(&mut d, 3);
        probe_isa_hadamard_t4_dc(&mut m, &big);
        probe_isa_quant_4x4(&mut d, &ff, &mf);
        probe_isa_dct_4x4(&mut d, &ca, &cb);
        probe_isa_pixel_avg_16x16(&mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64), &ca, &cb);
        probe_isa_hor_ver02_16x16(&ca, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
    }
    #[cfg(feature = "wide")]
    {
        use wide_probes::*;
        total += probe_wide_sad_16x16(&ca, &cb) + probe_wide_satd_4x4(&ca, &cb);
        probe_wide_dequant_ihadamard(&mut d, 3);
        probe_wide_hadamard_t4_dc(&mut m, &big);
        probe_wide_quant_4x4(&mut d, &ff, &mf);
        probe_wide_dct_4x4(&mut d, &ca, &cb);
        probe_wide_pixel_avg_16x16(&mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64), &ca, &cb);
        probe_wide_hor_ver02_16x16(&ca, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
    }
    println!("{total} {} {} {}", d[0], m[0], o[20 * 64 + 19]);
}
