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
//! **Nothing here is ever called on a correct build.** `simd::has_simd()` is false
//! wherever this module is selected, because `arch_cpu_features` clears every bit on a
//! target with no kernel set, so each dispatch site takes its scalar arm before
//! reaching the alias. These bodies exist so the names resolve, and forward rather
//! than `unreachable!()` so that a future kernel set which does set the bit gets
//! correct output rather than a panic.
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
