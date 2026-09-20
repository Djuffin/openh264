//! Pure Rust differential test harness against upstream C++ OpenH264.

#![allow(unused)]

pub mod config;
pub mod cpp_engine;
pub mod diagnostics;
pub mod inputs;
pub mod presets;
pub mod runner;
pub mod rust_engine;

pub use config::{BaseInitMode, DiffConfig, LtrConfig, SliceConfig, SpatialLayersConfig};
pub use cpp_engine::CppLibrary;
pub use inputs::{YuvClip, generate_screen_clip, load_looped_res};
pub use presets::*;
pub use runner::run_diff_config;
