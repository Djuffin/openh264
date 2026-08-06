#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Picture-border expansion function pointers, shared by encoder and decoder.
//!
//! Translated from `codec/common/inc/expand_pic.h`.

/// `PExpandPictureFunc` — `expand_pic.h:86`.
pub type PExpandPictureFunc = unsafe extern "C" fn(*mut u8, i32, i32, i32);

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
