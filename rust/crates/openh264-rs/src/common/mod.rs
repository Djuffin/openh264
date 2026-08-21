pub mod copy_mb;
pub mod cpu_core;
pub mod deblocking_common;
pub mod expand_pic;
pub mod intra_pred_common;
pub mod mc;
pub mod memory_align;
pub mod sad_common;
pub mod wels_common_defs;
// `wels_thread_pool` stood here — 933 lines of `CWelsThreadPool`, `CWelsTaskThread`,
// the two C++-list ports, `TaskPtr` with its `Send`/`Sync` pair, a mutable process-wide
// singleton and a `self as usize` laundering across a spawn. Deleted at T7.B4; the
// encoder forks with `std::thread::scope` and joins on the scope (D-mt-1).

pub use memory_align::CMemoryAlign;
