#![forbid(unsafe_code)]

//! Safe vocabulary types for the codec — the target types of the safety refactor.
//!
//! This module is the **new** half of the port. Everything else under `src/` is a
//! faithful transliteration of the C++ (raw pointers, `unsafe fn`, Hungarian names);
//! everything here is idiomatic, safe Rust that reproduces the C++ *semantics*
//! exactly while making its hazards unrepresentable. Phases 2–6 of
//! [`rust/docs/safety_refactor_plan.md`] convert codec code onto these types; Phase 1
//! (this module) builds and proves them while wired into nothing.
//!
//! # Policy
//!
//! * **`#![forbid(unsafe_code)]` in every file here**, not `deny`. Nothing in this
//!   module will ever need the exception; the crate's single permitted `unsafe`
//!   island is the C-ABI boundary in `src/api/` (plan §2.2.8).
//! * **Detached cursors** (plan §2.1.3). No type here stores a borrow into a buffer.
//!   Cursors are *positions*; buffers are *parameters*. The only lifetime-carrying
//!   types are the ephemeral views ([`plane::PlaneCursor`], [`plane::PlaneCursorMut`],
//!   [`pool::PoolRest`]) that never outlive the call chain that made them. This is
//!   what deletes `ExpandBsBuffer`'s pointer rebasing, the rollback stashes, and the
//!   self-referential context fields — offsets survive reallocation by definition.
//! * **Bounds come from slice indexing.** These types do not add bounds checks of
//!   their own beyond validating their construction invariant; they arrange for the
//!   arithmetic to land in a slice index, so an out-of-range access is a loud panic
//!   instead of silent corruption. Per plan P13 a panic in these types means a *port
//!   bug*: the C code would have corrupted memory at the same call.
//! * **No speculation.** Every public item here has either a differential test
//!   against the existing unsafe implementation or a named consumer in a later plan
//!   phase. Phases 2/3/5/6 are expected to *add* methods as real call sites appear.
//!
//! # Map
//!
//! | module | replaces | plan |
//! |---|---|---|
//! | [`plane`] | T2 — pixel-plane cursors with stride math and negative offsets | §2.2.1 |
//! | [`bits`] | T3 — detachable bit cursors (`SBitStringAux` reader *and* writer) | §2.2.2 |
//! | [`err`] | — | §10 D6 |
//!
//! [`rust/docs/safety_refactor_plan.md`]: ../../../../docs/safety_refactor_plan.md

pub mod bits;
pub mod err;
pub mod plane;

/// Deterministic PRNG for the property-style unit tests in this module.
///
/// Kept in a file of its own so the differential integration test can include the
/// *same* generator by path (`tests/common/prng.rs`), which is what makes a failing
/// seed reproducible across both test layers.
#[cfg(test)]
#[path = "prng.rs"]
pub(crate) mod prng;
