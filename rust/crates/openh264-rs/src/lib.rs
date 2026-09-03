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
//! - `recon-seam` — D-mt-3's single seam, its argument at the site. (`send-seam
//!   (Phase 9)`, D-mt-1's, retired at S10.13 with its `unsafe impl`.)
//! - `lawful-single(...)`, `instrument(test)` — named singles and tags on tests.
//!
//! **Two categories left this list and are not coming back.**
//! `port-raw(Phase 9)` / `cursor` was never a category but the **queue** — the
//! conversion work a phase had not finished — and S11.52 emptied it. The single
//! item still spelled `port-raw` was retagged `lawful-single(F162)` at S12.4; it
//! is a deliberate reproduction of an out-of-bounds read the C++ performs, kept
//! because safe indexing would panic where upstream reads and a panic is not
//! byte-identical. `SCREEN_CONTENT(dormant)` was Phase 10's lane and held one
//! `from_raw_parts`; S12.3 converted it, so the lane's code was all safe before it
//! woke. P10.3.D4 translated the dispatch block that reaches it and P10.3.D7
//! retired the last tag against measured entry counts, so that category has no
//! members either — the lane is live code now, not a queue.
//!
//! **The floor is a list, not a number** (D-exit-4). `tools/unsafe_census.sh`
//! pins every allow outside `src/api/` by file and category and fails in both
//! directions — a new one and a retired one both take it red —
//! and `tools/unsafe_instrument_floor.txt` names the test instruments item by
//! item. Regenerate them in the commit that moves them, never to make a gate pass.
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
// `unsafe_op_in_unsafe_fn` **left this list at S12.1 and is now denied below.** The
// reason it stayed — "an inner `unsafe {}` per deref would add thousands of blocks"
// — was an estimate, and when S11 finally measured it the estimate was wrong by two
// orders of magnitude: after the conversion mass closed, the whole crate holds
// **135 unguarded unsafe operations, in 13 `unsafe fn` bodies, across 4 files**
// (121 sites / 10 fns in `api/codec_api.rs`, 9 / 1 in `encoder/wels_preprocess.rs`,
// 4 / 1 in `encoder/encoder_context.rs`, 1 / 1 in `api/version.rs`). 108 of the 135
// are raw-pointer derefs on the decoder's reordering thunks.
//
// The 13 blocks are rustc's own machine-applied shape — `) -> T { unsafe {` … `}}`
// — chosen over wrapping each of the 135 operations individually because most of
// them are *place* expressions (`&(*pCtx).sSpsPpsCtx`), which cannot take a block
// without restructuring the statement around them. Body-wrapping moves no interior
// line at all, so every line stays diffable against the C++ it was transliterated
// from, which is the property the whole allow list above exists to protect.
//
// What the deny buys, given that: the other 46 `unsafe fn` in the crate hold zero
// unguarded operations today, and a new one in any of them — or in any `unsafe fn`
// written from here on — is now a compile error rather than a warning nobody reads.
// Inside the 13 wrapped bodies it buys nothing; those stay watched by the ratchet's
// `unsafe_fn` metric and by the census (plan §7.1).
//
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_imports,
    unused_variables
)]
#![deny(
    unsafe_op_in_unsafe_fn,
    unused_assignments,
    unused_mut,
    unreachable_patterns,
    unused_parens
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

