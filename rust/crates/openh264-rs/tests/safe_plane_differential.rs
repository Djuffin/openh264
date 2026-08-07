//! Differential tests: `safe::plane` against the raw mid-pointer arithmetic it
//! replaces (plan §2.2.1, taxonomy T2).
//!
//! The unit tests inside `src/safe/plane.rs` prove `PaddedPlane` is *self*-consistent.
//! These prove it is *C*-consistent: for every legal logical coordinate, the sample it
//! returns is the sample `pData[i][y * iLinesize[i] + x]` returns, over the exact
//! layouts `AllocPicture` builds.
//!
//! This file lives outside `src/`, so unlike everything under `src/safe/` it may use
//! `unsafe` — it has to, because the reference implementation *is* raw-pointer code.
//! Every `unsafe` block here drives the old side of a comparison.
//!
//! Running this under Miri additionally checks those raw accesses for UB; see
//! `rust/docs/phase1_findings.md`.

mod common;

use common::prng::Prng;
use openh264_rs::safe::plane::PaddedPlane;

/// The luma geometry `AllocPicture` (`decoder/pic_queue.rs:198-252`) computes for a
/// picture of `w` x `h`, with `PADDING_LENGTH = 32` and
/// `PICTURE_RESOLUTION_ALIGNMENT = 32`.
fn alloc_picture_luma_geometry(w: usize, h: usize) -> (usize, usize, usize) {
    let align = |v: usize| v.div_ceil(32) * 32;
    let stride = align(w + 64);
    let rows = align(h + 64);
    let origin = (1 + stride) * 32; // pData[0] - pBuffer[0]
    (stride, rows, origin)
}

/// The chroma geometry of the same picture: half the luma stride, 16 px of padding.
fn alloc_picture_chroma_geometry(w: usize, h: usize) -> (usize, usize, usize) {
    let (luma_stride, luma_rows, _) = alloc_picture_luma_geometry(w, h);
    let stride = luma_stride >> 1;
    let rows = luma_rows >> 1;
    let origin = ((1 + stride) * 32) >> 1;
    (stride, rows, origin)
}

#[test]
fn plane_samples_match_raw_mid_pointer_arithmetic() {
    let mut rng = Prng::new(0x9A5D_0001);

    for &(w, h) in &[(176usize, 144usize), (16, 16), (320, 192), (1920, 1080)] {
        for &(stride, rows, origin, pad) in &[
            {
                let (s, r, o) = alloc_picture_luma_geometry(w, h);
                (s, r, o, 32usize)
            },
            {
                let (s, r, o) = alloc_picture_chroma_geometry(w, h);
                (s, r, o, 16usize)
            },
        ] {
            let (pw, ph) = if pad == 32 { (w, h) } else { (w / 2, h / 2) };
            let bytes = rng.bytes(rows * stride);

            let plane = PaddedPlane::from_parts(bytes.clone(), stride, origin, pw, ph);
            assert_eq!(plane.pad(), pad, "pad recovered from the origin");

            // The C++ view of the same allocation: one pointer into the middle.
            let p_data = unsafe { bytes.as_ptr().add(origin) };

            let (xlo, xhi) = (-(pad as isize), (pw + pad) as isize);
            let (ylo, yhi) = (-(pad as isize), (ph + pad) as isize);

            // Every corner and edge of the legal range, then a PRNG sample of it.
            let mut coords: Vec<(isize, isize)> = vec![
                (xlo, ylo),
                (xhi - 1, ylo),
                (xlo, yhi - 1),
                (xhi - 1, yhi - 1),
                (0, 0),
                (-1, -1),
                (pw as isize - 1, ph as isize - 1),
            ];
            for _ in 0..3000 {
                coords.push((
                    rng.range_i32(xlo as i32, xhi as i32 - 1) as isize,
                    rng.range_i32(ylo as i32, yhi as i32 - 1) as isize,
                ));
            }

            for (x, y) in coords {
                let want = unsafe { *p_data.offset(y * stride as isize + x) };
                assert_eq!(
                    plane.at(x, y),
                    want,
                    "at({x}, {y}) on {pw}x{ph} pad {pad} stride {stride}, seed {:#x}",
                    rng.seed()
                );
            }

            // Rows, including rows that start inside the left padding.
            for _ in 0..500 {
                let y = rng.range_i32(ylo as i32, yhi as i32 - 1) as isize;
                let x0 = rng.range_i32(xlo as i32, xhi as i32 - 1) as isize;
                let len = rng.below((xhi - x0) as u32) as usize + 1;
                let want = unsafe {
                    std::slice::from_raw_parts(p_data.offset(y * stride as isize + x0), len)
                };
                assert_eq!(plane.row(y, x0, len), want, "row({y}, {x0}, {len})");
            }
        }
    }
}

#[test]
fn plane_cursors_match_the_roving_macroblock_pointer() {
    // The access pattern of `decode_slice.rs:1944` and `svc_base_layer_md.rs:327-358`:
    // anchor at an MB origin, then read and write at small signed offsets.
    let mut rng = Prng::new(0x9A5D_0002);
    let (w, h) = (176usize, 144usize);
    let (stride, rows, origin) = alloc_picture_luma_geometry(w, h);
    let bytes = rng.bytes(rows * stride);

    let plane = PaddedPlane::from_parts(bytes.clone(), stride, origin, w, h);
    let p_data = unsafe { bytes.as_ptr().add(origin) };

    for _ in 0..400 {
        let mb_x = rng.below((w / 16) as u32) as isize;
        let mb_y = rng.below((h / 16) as u32) as isize;
        // `pDstY = pData[0] + ((iMbY * iLumaStride + iMbX) << 4)`
        let mb_off = (mb_y * stride as isize + mb_x) << 4;
        let p_dst = unsafe { p_data.offset(mb_off) };
        let cursor = plane.cursor(mb_x * 16, mb_y * 16);

        for _ in 0..64 {
            // Intra prediction reaches one row up and one column left of the block.
            let dx = rng.range_i32(-1, 16) as isize;
            let dy = rng.range_i32(-1, 16) as isize;
            let want = unsafe { *p_dst.offset(dy * stride as isize + dx) };
            assert_eq!(cursor.at(dx, dy), want, "cursor at ({dx}, {dy})");
        }

        // `pDstY.add(16)` — the next macroblock along the row.
        if mb_x + 1 < (w / 16) as isize {
            let advanced = cursor.advance(16, 0);
            let p_next = unsafe { p_dst.add(16) };
            for dy in -1isize..16 {
                let want = unsafe { *p_next.offset(dy * stride as isize) };
                assert_eq!(advanced.at(0, dy), want, "after advance, row {dy}");
            }
        }
    }
}

#[test]
fn plane_writes_land_where_the_raw_pointer_would_have_put_them() {
    let (w, h) = (64usize, 48usize);
    let (stride, rows, origin) = alloc_picture_luma_geometry(w, h);
    let mut rng = Prng::new(0x9A5D_0003);

    let mut raw = vec![0u8; rows * stride];
    let mut plane = PaddedPlane::from_parts(vec![0u8; rows * stride], stride, origin, w, h);

    for _ in 0..2000 {
        let x = rng.range_i32(-32, (w + 32) as i32 - 1) as isize;
        let y = rng.range_i32(-32, (h + 32) as i32 - 1) as isize;
        let v = rng.next_u8();
        plane.set(x, y, v);
        unsafe {
            *raw.as_mut_ptr().add(origin).offset(y * stride as isize + x) = v;
        }
    }
    // Border expansion's shape: whole rows written above row 0, through both views.
    for y in -32isize..0 {
        let fill: Vec<u8> = (0..w).map(|_| rng.next_u8()).collect();
        plane.row_mut(y, 0, w).copy_from_slice(&fill);
        unsafe {
            std::slice::from_raw_parts_mut(
                raw.as_mut_ptr().add(origin).offset(y * stride as isize),
                w,
            )
            .copy_from_slice(&fill);
        }
    }
    assert_eq!(plane.as_slice(), &raw[..]);
}
