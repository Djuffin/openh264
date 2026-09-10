//! Common test helper utilities for integration tests.

// `#[path]`-included into several test crates, each of which uses only some helpers.
#![allow(dead_code)]

pub mod sha1;
pub mod y4m;

/// The library's deterministic PRNG, so a seed printed by a unit test replays here.
#[path = "../../src/safe/prng.rs"]
pub mod prng;
#[allow(unused_imports)]
pub use sha1::Sha1Hasher;
#[allow(unused_imports)]
pub use y4m::compare_y4m_buffers;

/// Loading a shared library at runtime, so both codecs can live in one process.
///
/// `LoadLibraryW` rather than `LoadLibraryA`, so a path with non-ASCII characters resolves.
pub mod dylib {
    use std::ffi::{CStr, c_void};
    use std::path::Path;

    /// Loads `path`, returning a null handle if it is not loadable. The handle is
    /// never closed: the library must outlive every pointer it handed out.
    #[cfg(unix)]
    pub fn open(path: &Path) -> *mut c_void {
        let Ok(c_path) = std::ffi::CString::new(path.to_str().unwrap_or_default()) else {
            return std::ptr::null_mut();
        };
        unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW) }
    }

    /// Resolves an exported symbol, or null if the library does not export it.
    #[cfg(unix)]
    pub fn sym(handle: *mut c_void, name: &CStr) -> *mut c_void {
        unsafe { libc::dlsym(handle, name.as_ptr()) }
    }

    #[cfg(windows)]
    unsafe extern "system" {
        fn LoadLibraryW(lpLibFileName: *const u16) -> *mut c_void;
        fn GetProcAddress(hModule: *mut c_void, lpProcName: *const std::ffi::c_char)
        -> *mut c_void;
    }

    #[cfg(windows)]
    pub fn open(path: &Path) -> *mut c_void {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe { LoadLibraryW(wide.as_ptr()) }
    }

    #[cfg(windows)]
    pub fn sym(handle: *mut c_void, name: &CStr) -> *mut c_void {
        unsafe { GetProcAddress(handle, name.as_ptr()) }
    }
}
