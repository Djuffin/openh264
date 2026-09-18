fn main() {
    cxx_build::bridge("src/api/cxx_api.rs")
        .include("../../../codec/api/wels")
        .std("c++14")
        .compile("openh264-rs-cxx");
    println!("cargo:rerun-if-changed=src/api/cxx_api.rs");
}
