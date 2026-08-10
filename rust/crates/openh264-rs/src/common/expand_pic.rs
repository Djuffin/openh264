#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Picture-border expansion, shared by encoder and decoder.
//!
//! Translated from `codec/common/inc/expand_pic.h` and
//! `codec/common/src/expand_pic.cpp`.

/// `PExpandPictureFunc` — `expand_pic.h:86`.
pub type PExpandPictureFunc = unsafe extern "C" fn(*mut u8, i32, i32, i32);

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

/// `SExpandPicFunc` — `codec/common/inc/expand_pic.h:88`. 24 bytes: one luma entry
/// plus a **two-element** chroma array (Cb, Cr), not two scalars.
///
/// Both encoder copies were wrong before this became the single definition:
/// `encoder_context.rs` named the fields `pfExpandPicLuma`/`pfExpandPicChroma`, and
/// `ref_list_mgr_svc.rs` had the right names but made the chroma entry a scalar,
/// leaving the struct 16 bytes instead of 24.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SExpandPicFunc {
    pub pfExpandLumaPicture: Option<PExpandPictureFunc>,
    pub pfExpandChromaPicture: [Option<PExpandPictureFunc>; 2],
}

/// `InitExpandPictureFunc` — `codec/common/src/expand_pic.cpp:351`.
///
/// The SIMD branches (`X86_ASM`, `HAVE_NEON`, `HAVE_NEON_AARCH64`) select faster
/// kernels for the same result; this port has none, so it assigns the `_c` scalar
/// kernels unconditionally — which is what the C++ does on this target too, since
/// `WelsCPUFeatureDetect` measures `0x00000000` here.
///
/// This function was **missing entirely**, and with it the encoder's
/// `sExpandPicFunc` was never populated. `ExpandReferencingPicture` then found
/// `None` in every slot and expanded nothing, so the reference picture's padding
/// border stayed zero and every motion search that looked outside the frame
/// compared against black.
///
/// # Safety
/// `pExpandPicFunc` must point to a valid `SExpandPicFunc`.
pub unsafe fn InitExpandPictureFunc(pExpandPicFunc: *mut SExpandPicFunc, _kuiCPUFlag: u32) {
    (*pExpandPicFunc).pfExpandLumaPicture =
        Some(crate::decoder::decoder_core::ExpandPictureLuma_c);
    (*pExpandPicFunc).pfExpandChromaPicture[0] =
        Some(crate::decoder::decoder_core::ExpandPictureChroma_c);
    (*pExpandPicFunc).pfExpandChromaPicture[1] =
        Some(crate::decoder::decoder_core::ExpandPictureChroma_c);
}
