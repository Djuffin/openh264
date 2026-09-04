//! Common test helper utilities for integration tests.

#![allow(dead_code, unused_imports)]

pub mod sha1;
pub mod y4m;

/// The deterministic PRNG the safe-vocabulary tests use, included from the library
/// so that a seed printed by an in-module unit test replays identically here.
#[path = "../../src/safe/prng.rs"]
pub mod prng;
pub use sha1::Sha1Hasher;
pub use y4m::compare_y4m_buffers;

/// **Loading the shipped C++ library at runtime.**
///
/// Both benches measure the port against the C++ build by `dlopen`ing it and calling
/// through the vtable it hands back; neither links it, because the point is to have
/// both encoders live in one process on one machine with one input. The two calls
/// that needs are POSIX-only, and `libc` does not declare Windows' equivalents at
/// all — `libc::dlopen` simply is not in the crate's `windows` module, which is what
/// broke the benches on this host. So the pair is behind one interface here, in the
/// module both benches already share, rather than duplicated per platform per bench.
///
/// `LoadLibraryW` rather than `LoadLibraryA` so a path through a user directory with
/// non-ASCII characters resolves; the ANSI entry point would mangle it under any
/// code page that cannot represent it.
pub mod dylib {
    use std::ffi::{CStr, c_void};
    use std::path::Path;

    /// Loads `path`, returning a null handle if it is not loadable. The handle is
    /// never closed — a bench holds its library for the life of the process, and
    /// unloading it under the pointers it handed out is what a crash looks like.
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
        fn GetProcAddress(hModule: *mut c_void, lpProcName: *const std::ffi::c_char) -> *mut c_void;
    }

    #[cfg(windows)]
    pub fn open(path: &Path) -> *mut c_void {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        unsafe { LoadLibraryW(wide.as_ptr()) }
    }

    #[cfg(windows)]
    pub fn sym(handle: *mut c_void, name: &CStr) -> *mut c_void {
        unsafe { GetProcAddress(handle, name.as_ptr()) }
    }
}
