//! The codec's version, and the two C-ABI entry points that report it.

#![deny(unsafe_code)]
use crate::api::codec_api::OpenH264Version;

/// The version this build reports — `2.6.0`, matching the C++ tree at the root.
///
/// Read by `WelsGetCodecVersion`/`WelsGetCodecVersionEx` below and by the
/// encoder's identifier string (`wels_encoder_ext.rs`), which is why it stays
/// `pub`.
pub static G_ST_CODEC_VERSION: OpenH264Version = OpenH264Version {
    uMajor: 2,
    uMinor: 6,
    uRevision: 0,
    uReserved: 0,
};

/// `WelsGetCodecVersion` — the by-value form.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub extern "C" fn WelsGetCodecVersion() -> OpenH264Version {
    G_ST_CODEC_VERSION
}

/// `WelsGetCodecVersionEx` — the out-parameter form.
///
/// # Safety
/// `pVersion` must be null or point to a writable `OpenH264Version`. The null
/// test is the C++'s and is kept: this is an application-supplied pointer, so
/// the guard is a real state rather than an unreachable one.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsGetCodecVersionEx(pVersion: *mut OpenH264Version) { unsafe {
    if !pVersion.is_null() {
        *pVersion = G_ST_CODEC_VERSION;
    }
}}
