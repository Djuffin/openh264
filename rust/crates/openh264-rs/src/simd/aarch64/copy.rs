//! The fixed-shape macroblock copies — `WelsCopy*_AArch64_neon`,
//! `codec/common/arm64/copy_mb_aarch64_neon.S`.
//!
//! Each is `ld1`/`st1` of one row per instruction, four rows per macro, walking both
//! strides. The asm keeps an aligned pair (`ld1 {v.2d}`) beside the unaligned one
//! (`ld1 {v.16b}`) for the two 16x16 slots; on AArch64 the two forms differ only in
//! their element size and neither faults on misalignment, so one kernel serves both
//! slots here, as it does in the x86_64 set. The 8-wide copies are `ld1 {v.d}[0]`,
//! which is the `vld1_u8` below.
//!
//! What these save over the scalar slot bodies is the same thing the x86_64 file
//! describes: `copy_rows_shared` re-derives and re-checks each row's cells, while a
//! kernel takes the block's whole span through `block_span` — one check per operand
//! — and strides through it itself, which is exactly the asm's `SIGN_EXTENSION` plus
//! post-indexed `ld1` prologue.
#![allow(unsafe_code)]

use core::arch::aarch64::*;
use core::cell::Cell;

use crate::encoder::rec_view::RecCursor;

/// Copies `h` rows of 16 bytes from `src` to `dst`, each walking its own stride.
///
/// Two strides rather than one because `WelsMdBackgroundMbEnc` copies the
/// mode-decision scratch — a stride-16 array — into a picture plane.
///
/// # Safety
/// Each span must cover `(h - 1) * its own stride + 16` bytes, which is what
/// [`RecCursor::block_span`] guarantees for the same `(16, h)`.
///
/// Overlapping operands keep the scalar's behaviour: a row is read whole before any
/// of it is written back.
#[target_feature(enable = "neon")]
unsafe fn copy_rows16(dst: &[Cell<u8>], dst_stride: usize, src: &[Cell<u8>], src_stride: usize, h: usize) {
    // `&[Cell<u8>]` is a shared reference to `UnsafeCell` contents, which is what
    // makes writing through a pointer derived from it sound — the same door
    // `Cell::as_ptr` opens for a single cell, widened to the slice's provenance so a
    // 16-byte store stays inside it.
    let s = src.as_ptr() as *const u8;
    let d = dst.as_ptr() as *mut u8;
    for y in 0..h {
        // SAFETY: the caller's span contract puts row `y`'s 16 bytes inside both slices.
        unsafe {
            let v = vld1q_u8(s.add(y * src_stride));
            vst1q_u8(d.add(y * dst_stride), v);
        }
    }
}

/// The 8-wide form of [`copy_rows16`]; same contract with `8` for `16`.
#[target_feature(enable = "neon")]
unsafe fn copy_rows8(dst: &[Cell<u8>], dst_stride: usize, src: &[Cell<u8>], src_stride: usize, h: usize) {
    let s = src.as_ptr() as *const u8;
    let d = dst.as_ptr() as *mut u8;
    for y in 0..h {
        // SAFETY: the caller's span contract puts row `y`'s 8 bytes inside both slices.
        unsafe {
            let v = vld1_u8(s.add(y * src_stride));
            vst1_u8(d.add(y * dst_stride), v);
        }
    }
}

/// `W` bytes of each of `h` rows, from one shared cursor to another.
///
/// Panics through `block_span` if either block leaves its buffer, before any
/// pointer is formed — which is what lets the kernels above be `unsafe` only
/// over an already-validated span.
#[inline(always)]
fn copy_block<const W: usize>(dst: &RecCursor<'_>, src: &RecCursor<'_>, h: usize) {
    let s = src.block_span(0, 0, W, h);
    let d = dst.block_span(0, 0, W, h);
    // SAFETY: both spans were just sized to `(h - 1) * stride + W` by `block_span`.
    match W {
        16 => unsafe { copy_rows16(d, dst.stride(), s, src.stride(), h) },
        8 => unsafe { copy_rows8(d, dst.stride(), s, src.stride(), h) },
        _ => unreachable!("only the 8- and 16-wide rows have kernels"),
    }
}

/// `WelsCopy16x16_AArch64_neon` and `WelsCopy16x16NotAligned_AArch64_neon` — see
/// the module header for why one kernel serves both.
#[inline]
pub fn copy_16x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16>(dst, src, 16);
}

/// `WelsCopy16x8NotAligned_AArch64_neon`.
#[inline]
pub fn copy_16x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16>(dst, src, 8);
}

/// `WelsCopy8x16_AArch64_neon`.
#[inline]
pub fn copy_8x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<8>(dst, src, 16);
}

/// `WelsCopy8x8_AArch64_neon`.
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

    /// Runs one shape through both the scalar slot body and the kernel here over
    /// identical planes, and requires the **whole plane** to match afterwards — not
    /// just the block. A kernel that ran a row long, or that walked the wrong stride,
    /// lands outside the block and only a whole-plane compare sees it.
    fn check(w: usize, h: usize, stride: usize, scalar: fn(&RecCursor, &RecCursor), simd: fn(&RecCursor, &RecCursor)) {
        // Two spare rows below the block and an anchor off (0, 0), so a kernel that
        // ignored the anchor or ran a row long has somewhere to land.
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

    /// The two operands do **not** have to share a stride: `WelsMdBackgroundMbEnc`
    /// hands `pfCopy16x16Aligned` a picture plane as the destination and a
    /// stride-16 scratch array as the source.
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

    /// The span's **length** is the whole of the kernels' bounds safety, and nothing
    /// downstream reads it — they stride from `as_ptr()`. A span that is too short
    /// can only show up as a panic that does not happen, and it takes a block sized
    /// to the gap to see it: this one overruns its buffer by less than one row's
    /// width, which a span that forgot to add `w` for the last row would accept.
    #[test]
    #[should_panic(expected = "out of range")]
    fn copy_rejects_a_block_whose_last_row_overruns() {
        let mut src = vec![0u8; 16 * 16];
        // 250 < the block's 256-byte span, but >= the 240 bytes a span missing the
        // last row's width would ask for.
        let mut dst = vec![0u8; 250];
        let s = RecCursor::over_owned(&mut src[..], 0, 16);
        let d = RecCursor::over_owned(&mut dst[..], 0, 16);
        copy_16x16(&d, &s);
    }

    /// `block_span` is what turns an out-of-range block into a panic instead of a
    /// pointer, so the kernels inherit the scalar's bounds behaviour rather than
    /// reading past the plane.
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
