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
