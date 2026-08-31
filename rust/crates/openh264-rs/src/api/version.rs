//! The codec's version, and the two C-ABI entry points that report it.
//!
//! **S11.5 moved these here from `encoder/wels_encoder_ext.rs`** (plan step 4).
//! They are `#[unsafe(no_mangle)]` exports — part of the frozen FFI surface
//! `libopenh264` consumers link against — and the plan's end state gives the
//! C-ABI island (`src/api/`) as the one place outside `rec_view.rs` where
//! `unsafe` lives. Leaving them in the encoder would have meant carrying a
//! `deny`-with-exceptions file into that end state for two functions that are
//! not encoder logic at all.
//!
//! **The exported symbol set does not change**, which is the whole constraint
//! on this move: `tools/abi_exports.sh` lists both names and
//! `tools/abi_harness` resolves and calls them through `dlopen`. Only the
//! module path moved; the symbols, their signatures and their values are
//! untouched.

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
///
/// The body is safe; the allow is here for `#[unsafe(no_mangle)]` itself
/// (F241), which the lint counts as an unsafe item because a duplicate symbol
/// name is a link-time hazard the compiler cannot check.
#[unsafe(no_mangle)]
// unsafe-cat: C-ABI
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
// unsafe-cat: C-ABI
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsGetCodecVersionEx(pVersion: *mut OpenH264Version) {
    if !pVersion.is_null() {
        *pVersion = G_ST_CODEC_VERSION;
    }
}
