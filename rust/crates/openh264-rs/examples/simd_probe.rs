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
//! Each half is present only where its module is: the `isa` probes are the SSE2
//! kernels on x86_64 and the NEON kernels on aarch64, and nothing elsewhere; the
//! `wide` probes need the feature. On aarch64 with `--features wide` this emits both
//! halves, which is how you read the hand-written NEON next to what `wide`'s lanes
//! lowered to.

// A build with neither probe set — off x86_64 and aarch64, without `--features wide`
// — emits no probes at all, so main's fixtures go unread. That is the honest outcome
// for a codegen instrument on a target with no kernels to read the codegen of.
#![allow(non_snake_case, unused_imports, unused_variables, unused_mut)]

use openh264_rs::encoder::rec_view::RecCursor;
use openh264_rs::safe::plane::{PlaneCursor, PlaneCursorMut};
#[cfg(target_arch = "x86_64")]
use openh264_rs::simd::x86_64 as isa;
#[cfg(all(target_arch = "aarch64", not(miri)))]
use openh264_rs::simd::aarch64 as isa;
#[cfg(feature = "wide")]
use openh264_rs::simd::wide as wd;

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_sad_16x16(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
    isa::sad::sample_sad_16x16(a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_satd_4x4(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
    isa::satd::satd_4x4(a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_sad_8x8(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
    isa::sad::sample_sad_8x8(a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_sad_four_16x16(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>, s: &mut [i32; 4]) {
    isa::sad::sample_sad_four_16x16(a, b, s)
}

/// The zero-motion-vector copy — `mc_luma`/`mc_chroma` with `(0, 0)`, which is
/// `common::mc::mc_copy` and nothing else. Both operand storages are probed because
/// both occur: a plane cursor over the reference picture where the encoder is
/// single-threaded, and the shared cell view under the reconstruction seam. What the
/// assembly should show is `HEIGHT` load/store pairs and **no per-row bounds
/// branch**; the checks belong to the two spans, once each.
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_mc_luma_zero_16x16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_luma(src, dst, 0, 0, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_mc_luma_zero_16x16_cells(src: &RecCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_luma(src, dst, 0, 0, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_mc_chroma_zero_8x8_cells(src: &RecCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_chroma(src, dst, 0, 0, 8, 8)
}

/// The same kernels over the **shared cell view**, which is the operand type every
/// motion-search and mode-decision call actually hands them (`encoder/md.rs`'s slot
/// signature is `fn(&RecCursor, &RecCursor) -> i32`); the `PlaneCursor` probes above
/// are the processing library's and the bench's path. Both have to be checkless.
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_sad_16x16_cells(a: &RecCursor<'_>, b: &RecCursor<'_>) -> i32 {
    isa::sad::sample_sad_16x16(a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_sad_8x8_cells(a: &RecCursor<'_>, b: &RecCursor<'_>) -> i32 {
    isa::sad::sample_sad_8x8(a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_sad_four_16x16_cells(a: &RecCursor<'_>, b: &RecCursor<'_>, s: &mut [i32; 4]) {
    isa::sad::sample_sad_four_16x16(a, b, s)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_satd_16x16(a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) -> i32 {
    isa::satd::satd_16x16(a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_satd_16x16_cells(a: &RecCursor<'_>, b: &RecCursor<'_>) -> i32 {
    isa::satd::satd_16x16(a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_dequant_ihadamard(res: &mut [i16; 16], mf: u16) {
    isa::quant::dequant_ihadamard_4x4(res, mf)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hadamard_t4_dc(out: &mut [i16; 16], dct: &[i16; 241]) {
    isa::quant::hadamard_t4_dc(out, dct)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_quant_4x4(d: &mut [i16; 16], ff: &[i16; 8], mf: &[i16; 8]) {
    isa::quant::quant_4x4(d, ff, mf)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_dct_4x4(d: &mut [i16; 16], a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) {
    isa::dct::dct_4x4(d, a, b)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_pixel_avg_16x16(dst: &mut PlaneCursorMut<'_>, a: &PlaneCursor<'_>, b: &PlaneCursor<'_>) {
    isa::mc::pixel_avg(dst, a, b, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver02_16x16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver02(src, dst, 16, 16)
}

/// The four motion-compensation kernels over **both** operand storages: the plain
/// plane cursor the decoder hands them and the shared cell view the encoder does.
/// What the assembly should show is the tap loads, the filter and the stores with
/// **no per-row bounds branch** — one span per operand per block, and the row
/// offsets inside it constant. The horizontal probe is at width 17 because that is
/// `MeRefineFracPixel`'s `kiW + 1`, the shape with the overlapping last chunk.
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_pixel_avg_16x16_cells(dst: &mut PlaneCursorMut<'_>, a: &PlaneCursor<'_>, b: &RecCursor<'_>) {
    isa::mc::pixel_avg(dst, a, b, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver02_16x16_cells(src: &RecCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver02(src, dst, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver20_16x16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver20(src, dst, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver20_16x16_cells(src: &RecCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver20(src, dst, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver20_17x16_cells(src: &RecCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver20(src, dst, 17, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver22_16x16(src: &PlaneCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver22(src, dst, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_hor_ver22_16x16_cells(src: &RecCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_hor_ver22(src, dst, 16, 16)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_mc_chroma_frac_8x8_cells(src: &RecCursor<'_>, dst: &mut PlaneCursorMut<'_>) {
    isa::mc::mc_chroma(src, dst, 3, 5, 8, 8)
}

/// The deblocking edge filters over the **shared cell view**, which is the operand
/// type every encoder call hands them, one probe per branch: `step_y == 1` is the
/// horizontal edge (taps step by the stride) and `step_x == 1` the vertical one (taps
/// step by one byte, sixteen lines gathered and transposed). What the assembly should
/// show is the tap loads and stores with **no per-row bounds branch** — the read side
/// is one span per call and the write side one `write_row` per line.
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_deblock_luma_lt4_h_cells(pix: &mut RecCursor<'_>, tc: &[i8; 4]) {
    isa::deblock::deblock_luma_lt4(pix, 64, 1, 40, 20, tc)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_deblock_luma_lt4_v_cells(pix: &mut RecCursor<'_>, tc: &[i8; 4]) {
    isa::deblock::deblock_luma_lt4(pix, 1, 64, 40, 20, tc)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_deblock_luma_eq4_h_cells(pix: &mut RecCursor<'_>) {
    isa::deblock::deblock_luma_eq4(pix, 64, 1, 40, 20)
}

#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_isa_deblock_luma_eq4_v_cells(pix: &mut RecCursor<'_>) {
    isa::deblock::deblock_luma_eq4(pix, 1, 64, 40, 20)
}

/// One row into the shared view: the store [`RecCursor::write_row`] promises. The
/// assembly should be a bounds check and a single `stur q0`.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_write_row_16(dst: &RecCursor<'_>, v: &[u8; 16]) {
    dst.write_row::<16>(3, 0, v)
}

/// The skip reconstruction's luma copy: sixteen rows out of a contiguous prediction
/// buffer into the shared view. One vector store per row is the whole of it.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_copy_block_to_view_16(src: &[u8], dst: &RecCursor<'_>) {
    openh264_rs::encoder::rec_view::copy_block_to_view::<16, 16>(src, dst)
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
    #[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri))))]
    {
        total += probe_isa_sad_16x16(&ca, &cb) + probe_isa_satd_4x4(&ca, &cb);
        total += probe_isa_sad_8x8(&ca, &cb) + probe_isa_satd_16x16(&ca, &cb);
        let mut four = [0i32; 4];
        probe_isa_sad_four_16x16(&ca, &cb, &mut four);
        total += four[0];
        {
            let (mut ra, mut rb) = (vec![7u8; 64 * 64], vec![9u8; 64 * 64]);
            let ka = RecCursor::over_owned(&mut ra, 20 * 64 + 19, 64);
            let kb = RecCursor::over_owned(&mut rb, 20 * 64 + 19, 64);
            total += probe_isa_sad_16x16_cells(&ka, &kb) + probe_isa_sad_8x8_cells(&ka, &kb);
            total += probe_isa_satd_16x16_cells(&ka, &kb);
            probe_isa_sad_four_16x16_cells(&ka, &kb, &mut four);
            total += four[1];
        }
        probe_isa_dequant_ihadamard(&mut d, 3);
        probe_isa_hadamard_t4_dc(&mut m, &big);
        probe_isa_quant_4x4(&mut d, &ff, &mf);
        probe_isa_dct_4x4(&mut d, &ca, &cb);
        probe_isa_pixel_avg_16x16(&mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64), &ca, &cb);
        probe_isa_hor_ver02_16x16(&ca, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
        probe_isa_hor_ver22_16x16(&ca, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
        probe_isa_hor_ver20_16x16(&ca, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
        probe_isa_mc_luma_zero_16x16(&ca, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
        {
            let mut ra = vec![7u8; 64 * 64];
            let ka = RecCursor::over_owned(&mut ra, 20 * 64 + 19, 64);
            probe_isa_mc_luma_zero_16x16_cells(&ka, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
            probe_isa_mc_chroma_zero_8x8_cells(&ka, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
            probe_isa_pixel_avg_16x16_cells(&mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64), &ca, &ka);
            probe_isa_hor_ver02_16x16_cells(&ka, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
            probe_isa_hor_ver20_17x16_cells(&ka, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
            probe_isa_hor_ver20_16x16_cells(&ka, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
            probe_isa_hor_ver22_16x16_cells(&ka, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
            probe_isa_mc_chroma_frac_8x8_cells(&ka, &mut PlaneCursorMut::new(&mut o, 20 * 64 + 19, 64));
        }
        {
            let mut ra = vec![7u8; 64 * 64];
            let mut ka = RecCursor::over_owned(&mut ra, 20 * 64 + 19, 64);
            let tc = [3i8, 2, 3, 1];
            probe_isa_deblock_luma_lt4_h_cells(&mut ka, &tc);
            probe_isa_deblock_luma_lt4_v_cells(&mut ka, &tc);
            probe_isa_deblock_luma_eq4_h_cells(&mut ka);
            probe_isa_deblock_luma_eq4_v_cells(&mut ka);
        }
    }
    {
        let src = vec![5u8; 256];
        let mut ra = vec![7u8; 64 * 64];
        let ka = RecCursor::over_owned(&mut ra, 20 * 64 + 19, 64);
        probe_copy_block_to_view_16(&src, &ka);
        probe_write_row_16(&ka, &[3u8; 16]);
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
