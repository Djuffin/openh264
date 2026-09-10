//! x86_64 SSE2 fixed-shape macroblock copies.
//!
//! C++: `codec/common/x86/mb_copy.asm`. These fill the encoder's `pfCopyNxM` slots,
//! whose scalar bodies go through
//! [`copy_rows_shared`](crate::encoder::rec_view::copy_rows_shared).
//!
//! Each kernel takes the block's whole span through
//! [`RecCursor::block_span`](crate::encoder::rec_view::RecCursor::block_span) — one
//! bounds check per operand for the whole block, rather than two per row per side —
//! and strides through it itself.
//!
//! A Rust slice carries no 16-byte alignment guarantee, so the aligned and unaligned
//! `Copy16x16` slots both get the unaligned form; `movdqu` on aligned data matches
//! `movdqa` on any CPU that runs this code.
//!
//! The 8-wide rows use `movq` in its SSE2 encoding (`_mm_loadl_epi64` /
//! `_mm_storel_epi64`), which touches no MMX state and needs no `emms`.

#![allow(unsafe_code)]

use core::arch::x86_64::*;

use crate::encoder::rec_view::RecCursor;

/// Copies `h` rows of 16 bytes from `src` to `dst`, each walking its own stride.
///
/// The strides may differ: `WelsMdBackgroundMbEnc` copies the stride-16
/// mode-decision scratch into a picture plane.
///
/// # Safety
/// Each span must cover `(h - 1) * its own stride + 16` bytes, which is what
/// [`RecCursor::block_span`] guarantees for the same `(16, h)`.
///
/// Overlapping operands are allowed: a row is read whole before any of it is
/// written back.
#[inline(always)]
unsafe fn copy_rows16<const H: usize>(
    mut d: *mut u8,
    dst_stride: usize,
    mut s: *const u8,
    src_stride: usize,
) {
    let mut y = 0;
    while y + 4 <= H {
        unsafe {
            let v0 = _mm_loadu_si128(s as *const __m128i);
            let v1 = _mm_loadu_si128(s.add(src_stride) as *const __m128i);
            let v2 = _mm_loadu_si128(s.add(src_stride * 2) as *const __m128i);
            let v3 = _mm_loadu_si128(s.add(src_stride * 3) as *const __m128i);
            _mm_storeu_si128(d as *mut __m128i, v0);
            _mm_storeu_si128(d.add(dst_stride) as *mut __m128i, v1);
            _mm_storeu_si128(d.add(dst_stride * 2) as *mut __m128i, v2);
            _mm_storeu_si128(d.add(dst_stride * 3) as *mut __m128i, v3);
            s = s.add(src_stride * 4);
            d = d.add(dst_stride * 4);
        }
        y += 4;
    }
    while y + 2 <= H {
        unsafe {
            let s1 = s.add(src_stride);
            let d1 = d.add(dst_stride);
            let v0 = _mm_loadu_si128(s as *const __m128i);
            let v1 = _mm_loadu_si128(s1 as *const __m128i);
            _mm_storeu_si128(d as *mut __m128i, v0);
            _mm_storeu_si128(d1 as *mut __m128i, v1);
            s = s1.add(src_stride);
            d = d1.add(dst_stride);
        }
        y += 2;
    }
    while y < H {
        unsafe {
            let v = _mm_loadu_si128(s as *const __m128i);
            _mm_storeu_si128(d as *mut __m128i, v);
            s = s.add(src_stride);
            d = d.add(dst_stride);
        }
        y += 1;
    }
}

/// The 8-wide form of [`copy_rows16`]; same contract with `8` for `16`.
#[inline(always)]
unsafe fn copy_rows8<const H: usize>(
    mut d: *mut u8,
    dst_stride: usize,
    mut s: *const u8,
    src_stride: usize,
) {
    let mut y = 0;
    while y + 4 <= H {
        unsafe {
            let v0 = _mm_loadl_epi64(s as *const __m128i);
            let v1 = _mm_loadl_epi64(s.add(src_stride) as *const __m128i);
            let v2 = _mm_loadl_epi64(s.add(src_stride * 2) as *const __m128i);
            let v3 = _mm_loadl_epi64(s.add(src_stride * 3) as *const __m128i);
            _mm_storel_epi64(d as *mut __m128i, v0);
            _mm_storel_epi64(d.add(dst_stride) as *mut __m128i, v1);
            _mm_storel_epi64(d.add(dst_stride * 2) as *mut __m128i, v2);
            _mm_storel_epi64(d.add(dst_stride * 3) as *mut __m128i, v3);
            s = s.add(src_stride * 4);
            d = d.add(dst_stride * 4);
        }
        y += 4;
    }
    while y + 2 <= H {
        unsafe {
            let s1 = s.add(src_stride);
            let d1 = d.add(dst_stride);
            let v0 = _mm_loadl_epi64(s as *const __m128i);
            let v1 = _mm_loadl_epi64(s1 as *const __m128i);
            _mm_storel_epi64(d as *mut __m128i, v0);
            _mm_storel_epi64(d1 as *mut __m128i, v1);
            s = s1.add(src_stride);
            d = d1.add(dst_stride);
        }
        y += 2;
    }
    while y < H {
        unsafe {
            let v = _mm_loadl_epi64(s as *const __m128i);
            _mm_storel_epi64(d as *mut __m128i, v);
            s = s.add(src_stride);
            d = d.add(dst_stride);
        }
        y += 1;
    }
}

/// `W` bytes of each of `H` rows, from one shared cursor to another.
///
/// Panics through `block_span` if either block leaves its buffer, before any
/// pointer is formed, so the kernels below run only over a validated span.
#[inline(always)]
fn copy_block<const W: usize, const H: usize>(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    let s = src.block_span(0, 0, W, H);
    let d = dst.block_span(0, 0, W, H);
    let s_ptr = s.as_ptr() as *const u8;
    let d_ptr = d.as_ptr() as *mut u8;
    match W {
        16 => unsafe { copy_rows16::<H>(d_ptr, dst.stride(), s_ptr, src.stride()) },
        8 => unsafe { copy_rows8::<H>(d_ptr, dst.stride(), s_ptr, src.stride()) },
        _ => unreachable!("only the 8- and 16-wide rows have kernels"),
    }
}

/// C++: `WelsCopy16x16_sse2` and `WelsCopy16x16NotAligned_sse2`,
/// `codec/common/x86/mb_copy.asm:68` and `:135`.
#[inline(always)]
pub fn copy_16x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16, 16>(dst, src);
}

/// C++: `WelsCopy16x8NotAligned_sse2`, `codec/common/x86/mb_copy.asm:201`.
#[inline(always)]
pub fn copy_16x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<16, 8>(dst, src);
}

/// C++: `WelsCopy8x16_mmx`, `codec/common/x86/mb_copy.asm:245`.
#[inline(always)]
pub fn copy_8x16(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<8, 16>(dst, src);
}

/// C++: `WelsCopy8x8_mmx`, `codec/common/x86/mb_copy.asm:311`.
#[inline(always)]
pub fn copy_8x8(dst: &RecCursor<'_>, src: &RecCursor<'_>) {
    copy_block::<8, 8>(dst, src);
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_mb_aux::{
        WelsCopy8x8_c, WelsCopy8x16_c, WelsCopy16x8_c, WelsCopy16x16_c,
    };

    /// A plane of `stride * rows` distinct-ish bytes, as cells.
    fn plane(stride: usize, rows: usize, seed: u8) -> Vec<u8> {
        (0..stride * rows)
            .map(|i| (i as u8).wrapping_mul(37).wrapping_add(seed))
            .collect()
    }

    /// Runs one shape through the scalar body and the SSE2 kernel over identical
    /// planes and compares the whole plane, so a write that runs a row long or walks
    /// the wrong stride is caught outside the block.
    fn check(
        w: usize,
        h: usize,
        stride: usize,
        scalar: fn(&RecCursor, &RecCursor),
        simd: fn(&RecCursor, &RecCursor),
    ) {
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

    /// A stride equal to the block width leaves no inter-row padding, so an
    /// over-long row store corrupts the next row instead of landing in padding.
    #[test]
    fn copy_is_exact_when_rows_are_contiguous() {
        check(16, 16, 16, WelsCopy16x16_c, copy_16x16);
        check(8, 8, 8, WelsCopy8x8_c, copy_8x8);
    }

    /// The two operands need not share a stride: `WelsMdBackgroundMbEnc` hands
    /// `pfCopy16x16Aligned` a picture plane as the destination and a stride-16
    /// scratch array as the source.
    #[test]
    fn copy_walks_each_operand_on_its_own_stride() {
        for &(dw, sw) in &[(64usize, 16usize), (16, 64), (33, 16), (16, 16)] {
            for (w, h, scalar, simd) in [
                (
                    16usize,
                    16usize,
                    WelsCopy16x16_c as fn(&RecCursor, &RecCursor),
                    copy_16x16 as fn(&RecCursor, &RecCursor),
                ),
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

    /// The span's length is the whole of the kernels' bounds safety, since they
    /// stride from `as_ptr()`. A block that overruns its buffer by less than one
    /// row's width still panics: a span that omitted the last row's width would
    /// accept it.
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

    /// `block_span` turns an out-of-range block into a panic rather than a pointer
    /// that reads past the plane.
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
