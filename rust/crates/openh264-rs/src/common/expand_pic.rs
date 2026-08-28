#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

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

// `ExpandReferencingPicture` and the two `_c` kernels stood here; all three are
// gone (T9.B2, S18).
//
// # What was here, and the two reasons it went
//
// The C++ reaches the border-expansion kernels through `SExpandPicFunc`, a
// three-slot table (`expand_pic.h:88`) filled by `InitExpandPictureFunc`
// (`expand_pic.cpp:351`) from the CPU flag. This port has no SIMD, so every
// install in both codecs set the same three constants and the alignment index
// selected between two identical functions. T4b.3b deleted the table, its
// installer, its four `PExpandPictureFunc` typedefs and the two
// `SWelsFuncPtrList` / `SWelsDecoderContext` members it occupied, leaving one
// `ExpandReferencingPicture` over `ExpandPictureLuma_c` / `ExpandPictureChroma_c`
// — three raw plane origins, because the two codecs' pictures did not own their
// planes. (That consolidation also fixed a real divergence: of the port's three
// copies of the C++ function, `decoder/manage_dec_ref.rs`'s had never written the
// `kiWidthUV < 16` arm at all — chroma planes of a frame narrower than 32 pixels.
// `phase4b_findings.md` F21 has the reachability analysis; the corpus is 176x144
// and wider, so no gate could have caught it.)
//
// Then both codecs' pictures came to own their planes —
// `decoder::picture::SPicture::expand_as_reference` (T5.AC5) and
// `encoder::picture::SPicture::expand_as_reference` (T6.F4) — and each hands
// [`expand_picture`] the plane's own allocation. `ExpandReferencingPicture` lost
// its last caller there and was deleted; the two `_c` kernels and
// `expand_shim_span` (the one place a mid-plane `pDst` was rebuilt into a whole
// allocation) were kept "as the C-shaped subjects
// `tests/kernels_differential_phase2.rs` runs against the reference".
//
// They were not run against the reference. The test's golden was
// [`expand_picture`] — the function the shim itself calls — so the equivalence it
// asserted was tautological, and the only live code under it was
// `expand_shim_span`'s own arithmetic. The two properties that are about the
// *kernel* (every padding byte written and none read; slack columns untouched)
// moved onto [`expand_picture`] directly in the same commit, and this file is now
// `#![deny(unsafe_code)]`.
//
// Session B's plane census (F104) is what made the deletion visible: nothing in
// `src/` named either kernel.
