// **S9.1: `common/` is sealed, and this is the subtree statement of it.**
//
// Every leaf in this module carries `#![forbid(unsafe_code)]` in its own right — the
// last two fell this session: `copy_mb.rs` when `copy_shim`'s only caller family
// (the seven `WelsCopyNxM_c` block copies) went to cursors, and `mc.rs` when
// `shim_wh`/`McLuma_c`/`McChroma_c` did the same. `wels_trace.rs` sealed at S8.9
// when the trace callback's `void*` became a token owned by `src/api/`.
//
// A `forbid` here seals the subtree rather than restating the leaves, so a new file
// added under `common/` inherits the rule instead of having to remember it.
#![forbid(unsafe_code)]

pub mod copy_mb;
pub mod cpu_core;
pub mod deblocking_common;
pub mod expand_pic;
pub mod intra_pred_common;
pub mod mc;
pub mod sad_common;
pub mod wels_common_defs;
pub mod wels_trace;
// `wels_thread_pool` stood here — 933 lines of `CWelsThreadPool`, `CWelsTaskThread`,
// the two C++-list ports, `TaskPtr` with its `Send`/`Sync` pair, a mutable process-wide
// singleton and a `self as usize` laundering across a spawn. Deleted at T7.B4; the
// encoder forks with `std::thread::scope` and joins on the scope (D-mt-1).

// **T8.A8: `memory_align` is deleted, and this line is its epitaph.** The module
// was the C's aligned allocator (`memory_align.h`/`.cpp`) plus an active-byte
// monitor. Phase 6 took the encoder's 45 call sites to 15, Phase 7 took the last
// 15 to none, and its own retirement note named the decoder as the remaining
// blocker: `SWelsDecoderContext::pMemAlign` surviving as a **null sentinel** and
// `CWelsDecoderImpl::align`, the object it pointed at. Both are gone at T8.A8 with
// the api-owned field inventory, and so is the file. Nothing in the crate
// allocates through the C's allocator any more.
