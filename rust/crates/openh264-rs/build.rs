// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

fn main() {
    cxx_build::bridge("src/api/cxx_api.rs")
        .std("c++14")
        .compile("openh264-rs-cxx");
    println!("cargo:rerun-if-changed=src/api/cxx_api.rs");
}
