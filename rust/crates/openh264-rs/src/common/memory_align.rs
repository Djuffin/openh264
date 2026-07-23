//! SIMD-aligned memory allocator and active memory monitor.
//!
//! Translated from `codec/common/inc/memory_align.h` and `codec/common/src/memory_align.cpp`.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ffi::{c_char, c_void};

unsafe extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn free(ptr: *mut c_void);
}

/// SIMD-aligned memory allocator with custom byte alignment.
///
/// Allocates a buffer from the system heap with sufficient padding and metadata headers
/// such that the returned pointer is aligned to `kiAlign` bytes (which must be a power of 2).
///
/// # Safety
/// Caller must ensure that `kiAlign` is a valid power-of-two alignment. The returned pointer
/// must be deallocated using [`WelsFree`].
pub unsafe fn WelsMalloc(
    kuiSize: u32,
    _kpTag: *const c_char,
    kiAlign: u32,
) -> *mut c_void {
    let kiSizeOfVoidPointer = std::mem::size_of::<*mut c_void>() as u32;
    let kiSizeOfInt = std::mem::size_of::<i32>() as u32;
    let kiAlignedBytes = kiAlign.wrapping_sub(1);
    let kiTrialRequestedSize = kuiSize + kiAlignedBytes + kiSizeOfVoidPointer + kiSizeOfInt;
    let kiActualRequestedSize = kiTrialRequestedSize;
    let kiPayloadSize = kuiSize as i32;

    let pBuf = unsafe { malloc(kiActualRequestedSize as usize) };
    if pBuf.is_null() {
        return std::ptr::null_mut();
    }

    let mut pAlignedBuffer = unsafe {
        pBuf.add((kiAlignedBytes + kiSizeOfVoidPointer + kiSizeOfInt) as usize)
    };
    let addr = pAlignedBuffer as usize;
    pAlignedBuffer = (addr - (addr & (kiAlignedBytes as usize))) as *mut u8;

    unsafe {
        let pVoidPtrLocation = pAlignedBuffer.sub(kiSizeOfVoidPointer as usize) as *mut *mut u8;
        *pVoidPtrLocation = pBuf;

        let pIntLocation = pAlignedBuffer.sub((kiSizeOfVoidPointer + kiSizeOfInt) as usize) as *mut i32;
        *pIntLocation = kiPayloadSize;
    }

    pAlignedBuffer as *mut c_void
}

/// Frees an aligned memory buffer previously allocated by [`WelsMalloc`] or [`WelsMallocz`].
///
/// # Safety
/// `pPointer` must be a pointer previously returned by [`WelsMalloc`] or [`WelsMallocz`], or null.
pub unsafe fn WelsFree(pPointer: *mut c_void, _kpTag: *const c_char) {
    if !pPointer.is_null() {
        unsafe {
            let pVoidPtrLocation = (pPointer as *mut *mut c_void).sub(1);
            let pRawBuf = *pVoidPtrLocation;
            free(pRawBuf);
        }
    }
}

/// Standalone helper allocating 16-byte aligned, zero-initialized memory.
///
/// # Safety
/// The returned pointer must be deallocated using [`WelsFree`].
pub unsafe fn WelsMallocz(kuiSize: u32, kpTag: *const c_char) -> *mut c_void {
    let pPointer = unsafe { WelsMalloc(kuiSize, kpTag, 16) };
    if pPointer.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        std::ptr::write_bytes(pPointer as *mut u8, 0, kuiSize as usize);
    }
    pPointer
}

/// SIMD-aligned memory allocator context and active memory monitor.
#[repr(C)]
#[derive(Debug)]
pub struct CMemoryAlign {
    pub m_nCacheLineSize: u32,
    pub m_nMemoryUsageInBytes: u32,
}

impl CMemoryAlign {
    /// Creates a new `CMemoryAlign` instance with the specified cache-line alignment in bytes.
    /// If `kuiCacheLineSize` is 0 or not a multiple of 16, it defaults to 16 (`0x10`).
    pub fn new(kuiCacheLineSize: u32) -> Self {
        let cache_line_size = if kuiCacheLineSize == 0 || (kuiCacheLineSize & 0x0f) != 0 {
            0x10
        } else {
            kuiCacheLineSize
        };
        Self {
            m_nCacheLineSize: cache_line_size,
            m_nMemoryUsageInBytes: 0,
        }
    }

    /// Allocates memory aligned to `m_nCacheLineSize` bytes and tracks aggregate byte usage.
    ///
    /// # Safety
    /// The returned pointer must be freed using [`CMemoryAlign::WelsFree`].
    pub unsafe fn WelsMalloc(&mut self, kuiSize: u32, kpTag: *const c_char) -> *mut c_void {
        let pPointer = unsafe { WelsMalloc(kuiSize, kpTag, self.m_nCacheLineSize) };
        if !pPointer.is_null() {
            let kiSizeOfVoidPointer = std::mem::size_of::<*mut c_void>();
            let kiSizeOfInt = std::mem::size_of::<i32>();
            let pIntLocation = unsafe {
                (pPointer as *mut u8).sub(kiSizeOfVoidPointer + kiSizeOfInt) as *const i32
            };
            let payload_size = unsafe { *pIntLocation };
            let kiMemoryLength = payload_size
                + (self.m_nCacheLineSize as i32)
                - 1
                + (kiSizeOfVoidPointer as i32)
                + (kiSizeOfInt as i32);
            self.m_nMemoryUsageInBytes = self
                .m_nMemoryUsageInBytes
                .wrapping_add(kiMemoryLength as u32);
        }
        pPointer
    }

    /// Allocates zero-initialized memory aligned to `m_nCacheLineSize` bytes.
    ///
    /// # Safety
    /// The returned pointer must be freed using [`CMemoryAlign::WelsFree`].
    pub unsafe fn WelsMallocz(&mut self, kuiSize: u32, kpTag: *const c_char) -> *mut c_void {
        let pPointer = unsafe { self.WelsMalloc(kuiSize, kpTag) };
        if pPointer.is_null() {
            return std::ptr::null_mut();
        }
        unsafe {
            std::ptr::write_bytes(pPointer as *mut u8, 0, kuiSize as usize);
        }
        pPointer
    }

    /// Frees an aligned memory buffer and decrements tracked memory usage.
    ///
    /// # Safety
    /// `pPointer` must have been allocated by this `CMemoryAlign` instance, or null.
    pub unsafe fn WelsFree(&mut self, pPointer: *mut c_void, kpTag: *const c_char) {
        if !pPointer.is_null() {
            let kiSizeOfVoidPointer = std::mem::size_of::<*mut c_void>();
            let kiSizeOfInt = std::mem::size_of::<i32>();
            let pIntLocation = unsafe {
                (pPointer as *mut u8).sub(kiSizeOfVoidPointer + kiSizeOfInt) as *const i32
            };
            let payload_size = unsafe { *pIntLocation };
            let kiMemoryLength = payload_size
                + (self.m_nCacheLineSize as i32)
                - 1
                + (kiSizeOfVoidPointer as i32)
                + (kiSizeOfInt as i32);
            self.m_nMemoryUsageInBytes = self
                .m_nMemoryUsageInBytes
                .wrapping_sub(kiMemoryLength as u32);
        }
        unsafe {
            WelsFree(pPointer, kpTag);
        }
    }

    /// Returns the cache line alignment size in bytes.
    #[inline]
    pub fn WelsGetCacheLineSize(&self) -> u32 {
        self.m_nCacheLineSize
    }

    /// Returns the active allocated memory usage in bytes.
    #[inline]
    pub fn WelsGetMemoryUsage(&self) -> u32 {
        self.m_nMemoryUsageInBytes
    }
}

impl Default for CMemoryAlign {
    fn default() -> Self {
        Self::new(16)
    }
}

impl Drop for CMemoryAlign {
    fn drop(&mut self) {
        debug_assert_eq!(
            self.m_nMemoryUsageInBytes, 0,
            "Memory leak detected in CMemoryAlign: {} bytes still active",
            self.m_nMemoryUsageInBytes
        );
    }
}

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_memory_align_creation() {
        let ma16 = CMemoryAlign::new(16);
        assert_eq!(ma16.WelsGetCacheLineSize(), 16);
        assert_eq!(ma16.WelsGetMemoryUsage(), 0);

        let ma32 = CMemoryAlign::new(32);
        assert_eq!(ma32.WelsGetCacheLineSize(), 32);

        // Invalid sizes default to 16
        let ma0 = CMemoryAlign::new(0);
        assert_eq!(ma0.WelsGetCacheLineSize(), 16);

        let ma15 = CMemoryAlign::new(15);
        assert_eq!(ma15.WelsGetCacheLineSize(), 16);
    }

    #[test]
    fn test_malloc_and_free() {
        let mut ma = CMemoryAlign::new(32);
        unsafe {
            let ptr = ma.WelsMalloc(1024, std::ptr::null());
            assert!(!ptr.is_null());
            assert_eq!((ptr as usize) % 32, 0);
            assert!(ma.WelsGetMemoryUsage() > 1024);

            ma.WelsFree(ptr, std::ptr::null());
            assert_eq!(ma.WelsGetMemoryUsage(), 0);
        }
    }

    #[test]
    fn test_mallocz() {
        let mut ma = CMemoryAlign::new(16);
        unsafe {
            let ptr = ma.WelsMallocz(256, std::ptr::null());
            assert!(!ptr.is_null());
            assert_eq!((ptr as usize) % 16, 0);

            let slice = std::slice::from_raw_parts(ptr as *const u8, 256);
            for &byte in slice {
                assert_eq!(byte, 0);
            }

            ma.WelsFree(ptr, std::ptr::null());
            assert_eq!(ma.WelsGetMemoryUsage(), 0);
        }
    }
}
