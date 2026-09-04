//! The scalar kernel set — every entry point of [`super::x86_64`] and [`super::wide`],
//! forwarding to the scalar body the codec would otherwise have called directly.
//!
//! **This module is not an implementation; it is what makes the alias total.** A
//! target with no vector kernels (off x86_64, without `--features wide`) still has to
//! resolve `simd::kernels::…`, or every one of the dispatch sites would need a `#[cfg]`
//! saying so. It is compiled only on such a target — where it is what
//! [`super::kernels`] names — so it can never shadow a real kernel on a build that has
//! one.
//!
//! **These run.** `--features scalar` is the port's `USE_ASM=No`, and on that build the
//! twenty-two direct dispatch sites — motion compensation, deblocking, the IDCTs — call
//! straight through here to the scalar body. The `pfXxx` tables do not: the feature word
//! is `0`, so they install their scalar arm without going through a forward.
//! `#[inline(always)]` on every one, so the hop costs nothing.
//!
//! Generated shape, one line each: same name, same signature, calls the scalar.

#![forbid(unsafe_code)]
#![allow(non_snake_case, unused_variables)]

pub mod copy;
pub mod dct;
pub mod deblock;
pub mod intra_pred;
pub mod mc;
pub mod quant;
pub mod sad;
pub mod satd;
pub mod score;
