#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Picture-border expansion, shared by encoder and decoder.
//!
//! Translated from `codec/common/inc/expand_pic.h` and
//! `codec/common/src/expand_pic.cpp`.

// ============================================================================
// Safe kernel
// ============================================================================

/// C++: `ExpandPictureLuma_c` / `ExpandPictureChroma_c`,
/// `codec/common/src/expand_pic.cpp` — one body, pad-parameterised, exactly as
/// the C++'s two copies differ only in `PADDING_LENGTH` vs `PADDING_LENGTH >> 1`.
///
/// Replicates the picture's border into the `pad`-pixel margin on all four
/// sides: the first/last rows are copied up/down `pad` times, each row's
/// first/last samples are smeared left/right, and the four corners take the
/// corner samples.
///
/// `buf` is the plane's **full allocation** — `(pic_h + 2*pad)` rows of
/// `stride` bytes or more (`AllocPicture` rounds the row count up) — with the
/// picture's `(0, 0)` at byte `pad * stride + pad`, which is where both
/// codecs' `AllocPicture`s put it (`pData = pBuffer + (1 + stride) * pad`).
/// This is a free function over that geometry rather than a `PaddedPlane`
/// method because in Phase 2 the allocation is still C-owned; the packaging
/// onto `Picture` happens in Phase 5 (plan §2.2.1).
///
/// Runtime-length `copy_within`/`fill` are the right idiom here, not a
/// per-call cost problem: this runs once per plane per reference picture, and
/// the C++ makes the identical `memcpy`/`memset` library calls.
///
/// # Panics
/// If the geometry does not hold: `stride < pic_w + 2*pad`, a buffer shorter
/// than `(pic_h + 2*pad) * stride`, or `pic_w`/`pic_h` of zero. The C++
/// equivalent would read and write out of the allocation.
pub fn expand_picture(buf: &mut [u8], stride: usize, pic_w: usize, pic_h: usize, pad: usize) {
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

/// `ExpandReferencingPicture` — `codec/common/src/expand_pic.cpp:388`, and the
/// **only** copy. Both codecs call this one function, as they do in the C++.
///
/// # What used to be here, and why it is gone (T4b.3b)
///
/// The C++ reaches the two kernels through `SExpandPicFunc`, a three-slot table
/// (`expand_pic.h:88`) filled by `InitExpandPictureFunc` (`expand_pic.cpp:351`)
/// from the CPU flag — `_sse2`/`_neon`/`_mmi` variants of the same result. This
/// port has no SIMD, so every install in both codecs set the same three
/// constants: `ExpandPictureLuma_c` and `ExpandPictureChroma_c` **twice**, so the
/// alignment index selected between two identical functions. The table, its
/// installer, its four `PExpandPictureFunc` typedefs and the two `SWelsFuncPtrList`
/// / `SWelsDecoderContext` members it occupied are all deleted; the kernels are
/// named directly.
///
/// # The three copies this replaces
///
/// The port had translated one C++ function three times, and two copies had
/// drifted apart in the `kiWidthUV < 16` case — chroma planes of a frame narrower
/// than 32 pixels:
///
/// | copy | sub-16 chroma |
/// |---|---|
/// | C++ `expand_pic.cpp:388` | `ExpandPictureChroma_c` on both planes |
/// | `encoder/ref_list_mgr_svc.rs` | same — correct |
/// | `decoder/error_concealment.rs` | `pExpChrom[0]`, which *happens* to be the same function here |
/// | `decoder/manage_dec_ref.rs` | **nothing at all** — the `else` was never written |
///
/// This body is the C++'s. See `phase4b_findings.md` F21 for the reachability
/// analysis: the corpus is 176x144 and wider, so no gate could have caught it.
///
/// # Divergence kept on purpose
///
/// The per-plane null guards are the union of what the three copies did (only
/// `manage_dec_ref`'s had them). The C++ dereferences unconditionally. Keeping
/// them cannot change output where the pointers are valid, and this runs once per
/// plane per reference picture, so the branches are free.
///
/// # Safety
/// `pData[0..=2]` are the plane origins of a picture allocated with
/// `PADDING_LENGTH` luma / `PADDING_LENGTH >> 1` chroma borders, with matching
/// `iStride[0..=2]`; see [`expand_picture`] for the geometry each kernel asserts.
/// Slices shorter than three entries panic rather than read out of bounds — the
/// C++ parameter is `uint8_t* pData[3]` decayed to a pointer and checks nothing.
pub unsafe fn ExpandReferencingPicture(pData: &[*mut u8], iWidth: i32, iHeight: i32, iStride: &[i32]) {
    use crate::decoder::decoder_core::{ExpandPictureChroma_c, ExpandPictureLuma_c};

    let pPicY = pData[0];
    let pPicCb = pData[1];
    let pPicCr = pData[2];
    let kiWidthY = iWidth;
    let kiHeightY = iHeight;
    let kiWidthUV = kiWidthY >> 1;
    let kiHeightUV = kiHeightY >> 1;

    if !pPicY.is_null() {
        ExpandPictureLuma_c(pPicY, iStride[0], kiWidthY, kiHeightY);
    }
    // Both former table slots held `ExpandPictureChroma_c`, so the C++'s
    // `pExpChrom[kbChrAligned]` and its `else` branch are one call here. The
    // alignment test is what would pick a `_sse2` variant in a SIMD build; it
    // selects nothing in this port, which is why the index is gone rather than
    // computed and discarded.
    if !pPicCb.is_null() {
        ExpandPictureChroma_c(pPicCb, iStride[1], kiWidthUV, kiHeightUV);
    }
    if !pPicCr.is_null() {
        ExpandPictureChroma_c(pPicCr, iStride[2], kiWidthUV, kiHeightUV);
    }
}
