#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! Picture-border expansion, shared by encoder and decoder.
//!
//! `codec/common/inc/expand_pic.h`, `codec/common/src/expand_pic.cpp`.

// ============================================================================
// Safe kernel
// ============================================================================

/// C++: `ExpandPictureLuma_c` / `ExpandPictureChroma_c`,
/// `codec/common/src/expand_pic.cpp`.
///
/// Replicates the picture's border into the `pad`-pixel margin on all four
/// sides: the first/last rows are copied up/down `pad` times, each row's
/// first/last samples are smeared left/right, and the four corners take the
/// corner samples.
///
/// `buf` is the plane's full allocation — `(pic_h + 2*pad)` rows of `stride`
/// bytes or more — with the picture's `(0, 0)` at byte `pad * stride + pad`.
///
/// # Panics
/// If the geometry does not hold: `stride < pic_w + 2*pad`, a buffer shorter
/// than `(pic_h + 2*pad) * stride`, or `pic_w`/`pic_h` of zero.
pub fn expand_picture(buf: &mut [u8], stride: usize, pic_w: usize, pic_h: usize, pad: usize) {
    // The only two pads in use, specialised — see `expand_with`.
    match pad {
        PAD_LUMA => expand_with::<PAD_LUMA>(buf, stride, pic_w, pic_h),
        PAD_CHROMA => expand_with::<PAD_CHROMA>(buf, stride, pic_w, pic_h),
        _ => expand_any(buf, stride, pic_w, pic_h, pad),
    }
}

/// `PADDING_LENGTH`, and its chroma half — the only two pads
/// `ExpandPicture`'s call sites pass.
const PAD_LUMA: usize = 32;
const PAD_CHROMA: usize = 16;

/// [`expand_picture`] with the pad known at compile time. With `pad` a runtime
/// parameter each margin fill leaves a `memset` call, two of them inside the
/// per-row loop; a constant width lowers each to a couple of stores.
#[inline]
fn expand_with<const PAD: usize>(buf: &mut [u8], stride: usize, pic_w: usize, pic_h: usize) {
    expand_any(buf, stride, pic_w, pic_h, PAD);
}

/// The pad-as-a-parameter body. See [`expand_picture`] for the contract.
#[inline(always)]
fn expand_any(buf: &mut [u8], stride: usize, pic_w: usize, pic_h: usize, pad: usize) {
    assert!(pic_w > 0 && pic_h > 0, "cannot expand an empty picture");
    assert!(
        stride >= pic_w + 2 * pad,
        "stride {stride} cannot hold a padded row of {pic_w} + 2*{pad}"
    );
    assert!(
        buf.len() >= (pic_h + 2 * pad) * stride,
        "allocation of {} bytes cannot hold {} padded rows of {stride}",
        buf.len(),
        pic_h + 2 * pad
    );

    let origin = pad * stride + pad;
    let last = origin + (pic_h - 1) * stride;
    let tl = buf[origin];
    let tr = buf[origin + pic_w - 1];
    let bl = buf[last];
    let br = buf[last + pic_w - 1];

    // Pad the rows above and below, and their corners.
    for i in 0..pad {
        let strides = (1 + i) * stride;
        let top = origin - strides;
        let bottom = last + strides;

        buf.copy_within(origin..origin + pic_w, top);
        buf.copy_within(last..last + pic_w, bottom);

        buf[top - pad..top].fill(tl);
        buf[top + pic_w..top + pic_w + pad].fill(tr);
        buf[bottom - pad..bottom].fill(bl);
        buf[bottom + pic_w..bottom + pic_w + pad].fill(br);
    }

    // Pad left and right of every picture row.
    for r in 0..pic_h {
        let row = origin + r * stride;
        let left = buf[row];
        let right = buf[row + pic_w - 1];
        buf[row - pad..row].fill(left);
        buf[row + pic_w..row + pic_w + pad].fill(right);
    }
}
