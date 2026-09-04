//! The fixed-shape macroblock copies — the twin of `simd::x86_64::copy`, and the
//! one kernel family where the portable API has nothing to offer.
//!
//! The intrinsic copies take the block's whole span as `&[Cell<u8>]`, turn it into a
//! raw pointer, and stride through it with `movdqu` pairs — which is exactly what
//! safe code cannot do: there is no way from `&[Cell<u8>]` to a 16-byte value
//! without either `unsafe` or reading the sixteen cells one at a time. So this file
//! keeps what the intrinsic kernel actually saved over the scalar slot — the span
//! is checked **once** per operand rather than twice per row — and reads and writes
//! each row cell by cell. Whether LLVM merges those sixteen `Cell::get`s into one
//! load is its decision; `benches/kernel_bench.rs` reports what it decided.
//!
//! No `wide` type appears here, because a lane type would have nothing to do:
//! a 16-byte array in, the same array out.

#![forbid(unsafe_code)]

use crate::encoder::rec_view::RecCursor;

/// `W` bytes of each of `h` rows, from one shared cursor to another, each on its own
/// stride. A row is read whole before any of it is written, as the scalar does.
///
/// Panics through `block_span` if either block leaves its buffer.
#[inline(always)]
fn copy_block<const W: usize>(dst: &RecCursor<'_>, src: &RecCursor<'_>, h: usize) {
    let s = src.block_span(0, 0, W, h);
    let d = dst.block_span(0, 0, W, h);
    let (ss, ds) = (src.stride(), dst.stride());
    for y in 0..h {
        let row = &s[y * ss..][..W];
        let out = &d[y * ds..][..W];
        let v: [u8; W] = core::array::from_fn(|i| row[i].get());
        for (c, &b) in out.iter().zip(v.iter()) {
            c.set(b);
        }
    }
}

/// C++: `WelsCopy16x16_sse2` / `WelsCopy16x16NotAligned_sse2`, `codec/common/x86/mb_copy.asm`.
#[inline]
pub fn copy_16x16_sse2(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16>(dst, src, 16);
}

/// C++: `WelsCopy16x8NotAligned_sse2`, `codec/common/x86/mb_copy.asm:201`.
#[inline]
pub fn copy_16x8_sse2(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16>(dst, src, 8);
}

/// The counterpart of `WelsCopy8x16_mmx`, `codec/common/x86/mb_copy.asm:245`.
#[inline]
pub fn copy_8x16_sse2(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<8>(dst, src, 16);
}

/// The counterpart of `WelsCopy8x8_mmx`, `codec/common/x86/mb_copy.asm:311`.
#[inline]
pub fn copy_8x8_sse2(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<8>(dst, src, 8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_mb_aux::{WelsCopy16x16_c, WelsCopy16x8_c, WelsCopy8x16_c, WelsCopy8x8_c};

    /// A plane of `stride * rows` distinct-ish bytes, as cells.
    fn plane(stride: usize, rows: usize, seed: u8) -> Vec<u8> {
        (0..stride * rows).map(|i| (i as u8).wrapping_mul(37).wrapping_add(seed)).collect()
    }

    /// Runs one shape through both the scalar slot body and the SSE2 kernel over
    /// identical planes, and requires the **whole plane** to match afterwards —
    /// not just the block. A kernel that ran a row long, or that walked the wrong
    /// stride, lands outside the block and only a whole-plane compare sees it.
    fn check(w: usize, h: usize, stride: usize, scalar: fn(&RecCursor, &RecCursor), simd: fn(&RecCursor, &RecCursor)) {
        // Two spare rows below the block and an anchor off (0, 0), so a kernel
        // that ignored the anchor or ran a row long has somewhere to land.
        let (ax, ay) = (3isize, 2isize);
        let rows = h + ay as usize + 2;
        let mut want = plane(stride, rows, 91);
        let mut got = want.clone();

        for (dst, f) in [(&mut want, scalar), (&mut got, simd)] {
            let mut s = plane(stride, rows, 0);
            let sc = RecCursor::over_owned(&mut s[..], 0, stride).advance(ax, ay);
            let dc = RecCursor::over_owned(&mut dst[..], 0, stride).advance(ax, ay);
            f(&dc, &sc);
        }

        assert_eq!(got, want, "{w}x{h} over stride {stride}");
    }

    #[test]
    fn copy_sse2_matches_the_scalar_slots() {
        for &stride in &[16usize, 24, 33, 64] {
            check(16, 16, stride.max(20), WelsCopy16x16_c, copy_16x16_sse2);
            check(16, 8, stride.max(20), WelsCopy16x8_c, copy_16x8_sse2);
            check(8, 16, stride.max(12), WelsCopy8x16_c, copy_8x16_sse2);
            check(8, 8, stride.max(12), WelsCopy8x8_c, copy_8x8_sse2);
        }
    }

    /// A stride equal to the block width leaves no gap between rows, so a kernel
    /// that over-wrote its row would corrupt the next one instead of landing in
    /// padding — the one geometry where an over-long store stays in bounds and
    /// still changes the answer.
    #[test]
    fn copy_sse2_is_exact_when_rows_are_contiguous() {
        check(16, 16, 16, WelsCopy16x16_c, copy_16x16_sse2);
        check(8, 8, 8, WelsCopy8x8_c, copy_8x8_sse2);
    }

    /// The two operands do **not** have to share a stride, and the one call site
    /// where they differ is the one this file exists for:
    /// `WelsMdBackgroundMbEnc` hands `pfCopy16x16Aligned` a picture plane as the
    /// destination and `RecCursor::over_owned(&mut sMemPredMb, .., 16)` — a
    /// stride-16 scratch array — as the source.
    ///
    /// This is not hypothetical: a first cut of `copy_block_sse2` asserted the
    /// strides equal, having only ever been run against the equal-stride shapes
    /// `check` builds, and it would have panicked on the first background
    /// macroblock. Nothing else in the suite reaches that path.
    #[test]
    fn copy_sse2_walks_each_operand_on_its_own_stride() {
        for &(dw, sw) in &[(64usize, 16usize), (16, 64), (33, 16), (16, 16)] {
            for (w, h, scalar, simd) in [
                (16usize, 16usize, WelsCopy16x16_c as fn(&RecCursor, &RecCursor), copy_16x16_sse2 as fn(&RecCursor, &RecCursor)),
                (16, 8, WelsCopy16x8_c, copy_16x8_sse2),
                (8, 16, WelsCopy8x16_c, copy_8x16_sse2),
                (8, 8, WelsCopy8x8_c, copy_8x8_sse2),
            ] {
                if dw < w || sw < w {
                    continue;
                }
                let mut want = plane(dw, h + 2, 91);
                let mut got = want.clone();
                for (dst, f) in [(&mut want, scalar), (&mut got, simd)] {
                    let mut s = plane(sw, h + 2, 0);
                    let sc = RecCursor::over_owned(&mut s[..], 0, sw);
                    let dc = RecCursor::over_owned(&mut dst[..], 0, dw);
                    f(&dc, &sc);
                }
                assert_eq!(got, want, "{w}x{h}, dst stride {dw}, src stride {sw}");
            }
        }
    }

    /// The span's **length** is the whole of the kernels' bounds safety, and
    /// nothing downstream reads it — they stride from `as_ptr()`. So a span that
    /// is too short cannot show up as a wrong answer, only as a panic that does
    /// not happen, and it takes a block sized to the gap to see it: this one
    /// overruns its buffer by less than one row's width, which a span that
    /// forgot to add `w` for the last row would accept.
    #[test]
    #[should_panic(expected = "out of range")]
    fn copy_sse2_rejects_a_block_whose_last_row_overruns() {
        let mut src = vec![0u8; 16 * 16];
        // 250 < the block's 256-byte span, but >= the 240 bytes a span missing
        // the last row's width would ask for.
        let mut dst = vec![0u8; 250];
        let s = RecCursor::over_owned(&mut src[..], 0, 16);
        let d = RecCursor::over_owned(&mut dst[..], 0, 16);
        copy_16x16_sse2(&d, &s);
    }

    /// `block_span` is what turns an out-of-range block into a panic instead of
    /// a pointer, so the kernels inherit the scalar's bounds behaviour rather
    /// than reading past the plane.
    #[test]
    #[should_panic(expected = "out of range")]
    fn copy_sse2_panics_rather_than_running_off_the_plane() {
        let mut buf = vec![0u8; 16 * 16];
        let mut src = vec![0u8; 16 * 16];
        let s = RecCursor::over_owned(&mut src, 0, 16);
        let d = RecCursor::over_owned(&mut buf, 0, 16).advance(0, 8);
        copy_16x16_sse2(&d, &s);
    }
}
