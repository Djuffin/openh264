//! x86_64 SSE2 fixed-shape macroblock copies.
//!
//! Translated from `codec/common/x86/mb_copy.asm`. These fill the encoder's
//! `pfCopyNxM` slots, whose scalar bodies go through
//! [`copy_rows_shared`](crate::encoder::rec_view::copy_rows_shared).
//!
//! # What is actually being saved here
//!
//! Not the moves. `copy_rows_shared` already lowers its `[u8; W]` row copy to a
//! `movups` pair — the emitted `WelsCopy16x16_c` holds the 32 the block needs.
//! What it also holds is **62 bounds-check branches**: each row re-derives its
//! address from the cursor's anchor and re-slices both cells, twice per row per
//! side. These kernels take the block's whole span through
//! [`RecCursor::block_span`](crate::encoder::rec_view::RecCursor::block_span) —
//! one check per operand for the whole block — and stride through it themselves,
//! which is exactly what the asm's `lea r4, [r1+2*r1]` prologue sets up.
//!
//! # Alignment, and why there is one kernel where upstream has two
//!
//! `mb_copy.asm` ships `WelsCopy16x16_sse2` (`movdqa` both sides) and
//! `WelsCopy16x16NotAligned_sse2` (`movdqu` loads, `movdqa` stores) and wires
//! them to separate slots. A Rust slice carries no 16-byte alignment guarantee,
//! so both slots get the unaligned form. That costs nothing on any CPU that can
//! run this port: `movdqu` on aligned data has matched `movdqa` since Nehalem,
//! and the split survives upstream only because it predates that.
//!
//! # The 8-wide copies
//!
//! Upstream has these in MMX only (`WelsCopy8x8_mmx`, `WelsCopy8x16_mmx`), which
//! is not worth reproducing: MMX aliases the x87 register file, so every kernel
//! has to be paired with an `emms`, and the port has no MMX anywhere else. The
//! 8-wide rows here use `movq` in its SSE2 encoding — `_mm_loadl_epi64` /
//! `_mm_storel_epi64` — which touches no MMX state and needs no `emms`.

#![allow(unsafe_code)]

use core::arch::x86_64::*;
use core::cell::Cell;

use crate::encoder::rec_view::RecCursor;

/// Copies `h` rows of 16 bytes from `src` to `dst`, each walking its own stride.
///
/// **Two strides, not one.** `WelsMdBackgroundMbEnc` copies the mode-decision
/// scratch — a stride-16 array — into a picture plane, so the operands routinely
/// disagree; `copy_rows_shared` handles that by asking each cursor for its own
/// row, and so does this.
///
/// # Safety
/// Each span must cover `(h - 1) * its own stride + 16` bytes, which is what
/// [`RecCursor::block_span`] guarantees for the same `(16, h)`.
///
/// Overlapping operands are fine, and the scalar's behaviour under them is
/// preserved: a row is read whole before any of it is written back, exactly as
/// `copy_rows_shared` reads into a `[u8; 16]` before writing.
#[target_feature(enable = "sse2")]
unsafe fn copy_rows16(
    dst: &[Cell<u8>],
    dst_stride: usize,
    src: &[Cell<u8>],
    src_stride: usize,
    h: usize,
) {
    unsafe {
        // `&[Cell<u8>]` is a shared reference to `UnsafeCell` contents, which is
        // what makes writing through a pointer derived from it sound — the same
        // door `Cell::as_ptr` opens for a single cell, widened to the slice's
        // provenance so a 16-byte store stays inside it.
        let s = src.as_ptr() as *const u8;
        let d = dst.as_ptr() as *mut u8;
        for y in 0..h {
            let v = _mm_loadu_si128(s.add(y * src_stride) as *const __m128i);
            _mm_storeu_si128(d.add(y * dst_stride) as *mut __m128i, v);
        }
    }
}

/// The 8-wide form of [`copy_rows16`]; same contract with `8` for `16`.
#[target_feature(enable = "sse2")]
unsafe fn copy_rows8(
    dst: &[Cell<u8>],
    dst_stride: usize,
    src: &[Cell<u8>],
    src_stride: usize,
    h: usize,
) {
    unsafe {
        let s = src.as_ptr() as *const u8;
        let d = dst.as_ptr() as *mut u8;
        for y in 0..h {
            let v = _mm_loadl_epi64(s.add(y * src_stride) as *const __m128i);
            _mm_storel_epi64(d.add(y * dst_stride) as *mut __m128i, v);
        }
    }
}

/// `W` bytes of each of `h` rows, from one shared cursor to another.
///
/// Panics through `block_span` if either block leaves its buffer, before any
/// pointer is formed — which is what lets the kernels below be `unsafe` only
/// over an already-validated span.
#[inline(always)]
fn copy_block<const W: usize>(dst: &RecCursor<'_>, src: &RecCursor<'_>, h: usize) {
    let s = src.block_span(0, 0, W, h);
    let d = dst.block_span(0, 0, W, h);
    match W {
        16 => unsafe { copy_rows16(d, dst.stride(), s, src.stride(), h) },
        8 => unsafe { copy_rows8(d, dst.stride(), s, src.stride(), h) },
        _ => unreachable!("only the 8- and 16-wide rows have kernels"),
    }
}

/// C++: `WelsCopy16x16_sse2` and `WelsCopy16x16NotAligned_sse2`,
/// `codec/common/x86/mb_copy.asm:68` and `:135` — see the module header for why
/// one kernel serves both.
#[inline]
pub fn copy_16x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16>(dst, src, 16);
}

/// C++: `WelsCopy16x8NotAligned_sse2`, `codec/common/x86/mb_copy.asm:201`.
#[inline]
pub fn copy_16x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16>(dst, src, 8);
}

/// The SSE2 counterpart of `WelsCopy8x16_mmx`, `codec/common/x86/mb_copy.asm:245`
/// — see the module header on why this is not MMX.
#[inline]
pub fn copy_8x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<8>(dst, src, 16);
}

/// The SSE2 counterpart of `WelsCopy8x8_mmx`, `codec/common/x86/mb_copy.asm:311`.
#[inline]
pub fn copy_8x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<8>(dst, src, 8);
}

// ============================================================================
// Unit Tests
// ============================================================================

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
    fn copy_matches_the_scalar_slots() {
        for &stride in &[16usize, 24, 33, 64] {
            check(16, 16, stride.max(20), WelsCopy16x16_c, copy_16x16);
            check(16, 8, stride.max(20), WelsCopy16x8_c, copy_16x8);
            check(8, 16, stride.max(12), WelsCopy8x16_c, copy_8x16);
            check(8, 8, stride.max(12), WelsCopy8x8_c, copy_8x8);
        }
    }

    /// A stride equal to the block width leaves no gap between rows, so a kernel
    /// that over-wrote its row would corrupt the next one instead of landing in
    /// padding — the one geometry where an over-long store stays in bounds and
    /// still changes the answer.
    #[test]
    fn copy_is_exact_when_rows_are_contiguous() {
        check(16, 16, 16, WelsCopy16x16_c, copy_16x16);
        check(8, 8, 8, WelsCopy8x8_c, copy_8x8);
    }

    /// The two operands do **not** have to share a stride, and the one call site
    /// where they differ is the one this file exists for:
    /// `WelsMdBackgroundMbEnc` hands `pfCopy16x16Aligned` a picture plane as the
    /// destination and `RecCursor::over_owned(&mut sMemPredMb, .., 16)` — a
    /// stride-16 scratch array — as the source.
    ///
    /// This is not hypothetical: a first cut of `copy_block` asserted the
    /// strides equal, having only ever been run against the equal-stride shapes
    /// `check` builds, and it would have panicked on the first background
    /// macroblock. Nothing else in the suite reaches that path.
    #[test]
    fn copy_walks_each_operand_on_its_own_stride() {
        for &(dw, sw) in &[(64usize, 16usize), (16, 64), (33, 16), (16, 16)] {
            for (w, h, scalar, simd) in [
                (16usize, 16usize, WelsCopy16x16_c as fn(&RecCursor, &RecCursor), copy_16x16 as fn(&RecCursor, &RecCursor)),
                (16, 8, WelsCopy16x8_c, copy_16x8),
                (8, 16, WelsCopy8x16_c, copy_8x16),
                (8, 8, WelsCopy8x8_c, copy_8x8),
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
    fn copy_rejects_a_block_whose_last_row_overruns() {
        let mut src = vec![0u8; 16 * 16];
        // 250 < the block's 256-byte span, but >= the 240 bytes a span missing
        // the last row's width would ask for.
        let mut dst = vec![0u8; 250];
        let s = RecCursor::over_owned(&mut src[..], 0, 16);
        let d = RecCursor::over_owned(&mut dst[..], 0, 16);
        copy_16x16(&d, &s);
    }

    /// `block_span` is what turns an out-of-range block into a panic instead of
    /// a pointer, so the kernels inherit the scalar's bounds behaviour rather
    /// than reading past the plane.
    #[test]
    #[should_panic(expected = "out of range")]
    fn copy_panics_rather_than_running_off_the_plane() {
        let mut buf = vec![0u8; 16 * 16];
        let mut src = vec![0u8; 16 * 16];
        let s = RecCursor::over_owned(&mut src, 0, 16);
        let d = RecCursor::over_owned(&mut buf, 0, 16).advance(0, 8);
        copy_16x16(&d, &s);
    }
}
