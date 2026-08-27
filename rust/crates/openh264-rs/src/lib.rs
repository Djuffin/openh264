//! Low-level C ABI type definitions matching OpenH264 C interface.
//!
//! # The crate's unsafe posture (Phase 9's exit, session J)
//!
//! `#![deny(unsafe_code)]` sits at the **root**, so a new `unsafe` anywhere in the
//! crate is a compile error unless the item carries a tagged `#[allow(unsafe_code)]`
//! naming its category. The per-file `deny`s that predate this stay where they are:
//! they are redundant with the root and they are also the record of which module
//! reached the bar in which phase, which is worth more than the four lines they cost.
//!
//! The lawful categories, and where their reasons live:
//!
//! - `C-ABI` / `C-ABI(test)` — the frozen FFI boundary (`src/api/`, the `#[repr(C)]`
//!   types and the boundary thunks). This is the surface the ratchet watches from
//!   here on (plan §7.1).
//! - `fork-shared(S63)` — the encoder's multi-threaded fork hands one context to N
//!   workers, so its reachable bodies keep raw context parameters permanently. The
//!   soundness argument is written once, at `rust/docs/phase9_disposition.md` §2,
//!   and refereed by the two multi-threaded fork/join Miri probes.
//! - `recon-seam`, `send-seam(Phase 9)` — D-mt-3's and D-mt-1's single seams, each
//!   with its argument at the site.
//! - `SCREEN_CONTENT(dormant)` — Phase 10's lane.
//! - `lawful-single(...)`, `instrument(test)` — named singles and tags on tests.
//! - `port-raw(Phase 9)` / `cursor` — the **queue**, not a category: the conversion
//!   work the phase did not finish, itemized in `phase9_disposition.md` §4-5 and
//!   handed to maintenance by D-exit-3. It only ever shrinks (D-exit-1).
#![deny(unsafe_code)]
// The naming allows are a requirement, not debt: this crate is a line-by-line port
// and every identifier is diffable against the C++ it came from (`CODING_STYLE`).
//
// **The other four were measured at session J** rather than trimmed by taste. With
// all four removed, a clean `cargo build --lib` reports, by lint:
//
//   unused_unsafe     0   -> DELETED: it suppressed nothing.
//   dead_code         0   -> DELETED: it suppressed nothing either. (The one dead
//                            item the root allow had been hiding, `encoder_ext`'s
//                            `tag!` macro, fires as `unused_macros` and is deleted
//                            in the same commit; F129's crate-wide dead-item scan
//                            is a different measurement — it strips the *module*
//                            allows too, and those stay.)
//   unused_imports   89   -> KEPT, and the number is why. It is measured against
//                            the lib alone, and the `#[cfg(test)]` modules use many
//                            of the same names: `cargo fix` acting on the lint's
//                            own suggestions — twice, once with `--all-targets` —
//                            broke the build both times (`SpsRef`, `SDecodingParam`
//                            and 14 more, unresolved in test code). The count is
//                            confounded, not actionable, and the queue that owns
//                            the real cleanup is `phase9_disposition.md`.
//   unused_variables  8   -> KEPT: they are C++ parameter names on transliterated
//                            signatures, and `_`-prefixing them costs the
//                            diffability the naming allows above exist to protect.
//
// `unsafe_op_in_unsafe_fn` stays for the same transliteration reason: requiring an
// inner `unsafe {}` per deref would add thousands of blocks to code the phase is
// deleting rather than blessing. The ratchet's `unsafe_fn` metric is what watches
// that surface instead (plan §7.1).
//
// Not allowed here, and deliberately visible: `unused_assignments` (17) and
// `unreachable_patterns` (2), the C's `int x = 0; … x = f();` idiom and its
// switch arms. They have been warnings since before this phase; leaving them
// visible is what keeps a *new* one findable.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_imports,
    unused_variables,
    unsafe_op_in_unsafe_fn
)]


pub mod common;
pub mod decoder;
pub mod encoder;
pub mod processing;
pub mod api;
pub mod safe;

pub use crate::api::codec_api::*;

pub fn split_annexb_units(bitstream: &[u8]) -> Vec<&[u8]> {
    let mut start_indices = Vec::new();
    let mut i = 0;
    let len = bitstream.len();
    while i + 2 < len {
        if bitstream[i] == 0 && bitstream[i + 1] == 0 {
            if bitstream[i + 2] == 1 {
                start_indices.push(i);
                i += 3;
                continue;
            } else if i + 3 < len && bitstream[i + 2] == 0 && bitstream[i + 3] == 1 {
                start_indices.push(i);
                i += 4;
                continue;
            }
        }
        if let Some(pos) = bitstream[i + 1..].iter().position(|&b| b == 0) {
            i += 1 + pos;
        } else {
            break;
        }
    }

    let mut units = Vec::with_capacity(start_indices.len());
    for idx in 0..start_indices.len() {
        let start = start_indices[idx];
        let end = if idx + 1 < start_indices.len() {
            start_indices[idx + 1]
        } else {
            len
        };
        units.push(&bitstream[start..end]);
    }
    units
}

