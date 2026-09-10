//! Dynamic loader and wrapper for the upstream C++ OpenH264 shared library.
#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use std::ffi::{c_void, CStr};
use std::path::{Path, PathBuf};

type CppWelsCreateSVCEncoderFn = unsafe extern "C" fn(ppEncoder: *mut *mut ISVCEncoder) -> i32;
type CppWelsDestroySVCEncoderFn = unsafe extern "C" fn(pEncoder: *mut ISVCEncoder);

#[cfg(unix)]
unsafe fn dlopen(path: &Path) -> *mut c_void {
    let Ok(c_path) = std::ffi::CString::new(path.to_str().unwrap_or_default()) else {
        return std::ptr::null_mut();
    };
    unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW) }
}

#[cfg(unix)]
unsafe fn dlsym(handle: *mut c_void, name: &CStr) -> *mut c_void {
    unsafe { libc::dlsym(handle, name.as_ptr()) }
}

#[cfg(windows)]
unsafe fn dlopen(path: &Path) -> *mut c_void {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe extern "system" {
        fn LoadLibraryW(lpLibFileName: *const u16) -> *mut c_void;
    }
    unsafe { LoadLibraryW(wide.as_ptr()) }
}

#[cfg(windows)]
unsafe fn dlsym(handle: *mut c_void, name: &CStr) -> *mut c_void {
    unsafe extern "system" {
        fn GetProcAddress(hModule: *mut c_void, lpProcName: *const std::ffi::c_char) -> *mut c_void;
    }
    unsafe { GetProcAddress(handle, name.as_ptr()) }
}

pub struct CppLibrary {
    _handle: *mut c_void,
    create_fn: CppWelsCreateSVCEncoderFn,
    destroy_fn: CppWelsDestroySVCEncoderFn,
}

// Safety: The functions loaded from C++ libopenh264 are thread-safe for creating encoder instances.
unsafe impl Send for CppLibrary {}
unsafe impl Sync for CppLibrary {}

impl CppLibrary {
    pub fn get() -> &'static Self {
        static INSTANCE: std::sync::OnceLock<CppLibrary> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(|| {
            Self::load().unwrap_or_else(|| {
                panic!(
                    "Failed to locate or load libopenh264 shared library (libopenh264.so/dylib/dll).\n\
                     Please build the C++ reference library by running `make -j libraries` in the repository root."
                );
            })
        })
    }

    fn repo_root() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let candidates = [
            manifest_dir.join("../../../"),
            manifest_dir.join("../../"),
            manifest_dir.join("../"),
        ];
        for c in candidates {
            if c.join("res").exists() && (c.join("codec").exists() || c.join("libopenh264.so").exists()) {
                if let Ok(canon) = c.canonicalize() {
                    return canon;
                }
                return c;
            }
        }
        manifest_dir
    }

    pub fn load() -> Option<Self> {
        let root = Self::repo_root();
        let mut candidates = Vec::new();
        if let Ok(custom) = std::env::var("LIBOPENH264_SO") {
            candidates.push(PathBuf::from(custom));
        }
        candidates.extend([
            root.join("libopenh264.so"),
            root.join("libopenh264.dylib"),
            root.join("openh264.dll"),
            root.join("libopenh264.dll"),
            PathBuf::from("/usr/local/lib/libopenh264.so"),
            PathBuf::from("/usr/lib/libopenh264.so"),
        ]);

        for path in &candidates {
            if !path.exists() {
                continue;
            }
            unsafe {
                let handle = dlopen(path);
                if !handle.is_null() {
                    let create_sym = dlsym(handle, c"WelsCreateSVCEncoder");
                    let destroy_sym = dlsym(handle, c"WelsDestroySVCEncoder");
                    if !create_sym.is_null() && !destroy_sym.is_null() {
                        return Some(Self {
                            _handle: handle,
                            create_fn: std::mem::transmute::<*mut c_void, CppWelsCreateSVCEncoderFn>(
                                create_sym,
                            ),
                            destroy_fn: std::mem::transmute::<
                                *mut c_void,
                                CppWelsDestroySVCEncoderFn,
                            >(destroy_sym),
                        });
                    }
                }
            }
        }
        None
    }

    pub fn create_encoder(&self) -> *mut ISVCEncoder {
        let mut p: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = unsafe { (self.create_fn)(&mut p) };
        assert_eq!(ret, 0, "C++ WelsCreateSVCEncoder failed with code {}", ret);
        assert!(!p.is_null(), "C++ WelsCreateSVCEncoder returned null pointer");
        p
    }

    pub fn destroy_encoder(&self, p: *mut ISVCEncoder) {
        if !p.is_null() {
            unsafe { (self.destroy_fn)(p) };
        }
    }
}
