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
