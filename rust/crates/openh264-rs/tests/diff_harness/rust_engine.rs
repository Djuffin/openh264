//! Wrapper for the in-tree Rust OpenH264 encoder.

use openh264_rs::api::codec_api::*;

pub struct RustEngine;

impl RustEngine {
    pub fn create_encoder() -> *mut ISVCEncoder {
        let mut p: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = unsafe { WelsCreateSVCEncoder(&mut p) };
        assert_eq!(ret, 0, "Rust WelsCreateSVCEncoder failed with code {}", ret);
        assert!(!p.is_null(), "Rust WelsCreateSVCEncoder returned null pointer");
        p
    }

    pub fn destroy_encoder(p: *mut ISVCEncoder) {
        if !p.is_null() {
            unsafe { WelsDestroySVCEncoder(p) };
        }
    }
}
