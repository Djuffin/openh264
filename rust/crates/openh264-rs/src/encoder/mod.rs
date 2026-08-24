#![deny(unsafe_code)]
pub mod abi_guard;
pub mod au_set;
pub mod deblocking;
pub mod encode_mb_aux;
pub mod decode_mb_aux;
pub mod encoder_context;
pub mod get_intra_predictor;
pub mod md;
pub mod nal_encap;
pub mod param_svc;
pub mod paraset_strategy;
pub mod picture;
pub mod rec_view;
pub mod set_mb_syn_cabac;
// **T6.J6's exemption retired at T7.C8, on schedule.** `#![deny(unsafe_code)]` above
// is an inner attribute on the `encoder` module, so it reaches every file below it —
// and until this session two of them were exempted here, at the declaration rather
// than by leaving the deny off `mod.rs`, so that the boundary was visible where a
// reader would look for it. `wels_task_management` was the first and it no longer
// exists (T7.B4); `slice_multi_threading` was the second and it carries its own
// `#![deny(unsafe_code)]` now, with every one of its 26 unsafe items allowed and
// tagged. **The declaration list below has no exemptions left**, which is what Phase 6
// promised for this phase's close.
pub mod slice_multi_threading;
pub mod encoder_ext;
pub mod svc_base_layer_md;
pub mod svc_enc_slice_segment;
pub mod svc_encode_mb;
pub mod svc_encode_slice;
pub mod svc_mode_decision;
pub mod svc_motion_estimate;
pub mod svc_set_mb_syn_cabac;
pub mod svc_set_mb_syn_cavlc;
pub mod vlc_encoder;
pub mod ref_list_mgr_svc;
pub mod sample;
pub mod rc;
pub mod wels_encoder_ext;
pub mod wels_func_ptr_def;
pub mod wels_preprocess;
// `wels_task_management` stood here — `CWelsBaseTask` and its discriminant, the four
// wrapper types, `CWelsTaskList`, `WelsTaskBarrier`, `CWelsTaskManageBase` and
// `CWelsTaskManageOne`, and two more `Send`/`Sync` pairs. Deleted at T7.B4 with the
// pool it drove. Its allow retired with it; `slice_multi_threading`'s is the last one
// left in this module, and step 7 takes that.

/// Whether an `OH264_*DUMP` debugging dump is switched on, cached so the hot paths
/// that call it pay one relaxed load rather than an environment scan.
///
/// These dumps are the differential-bisection technique described in
/// `rust/docs/encoder_port_status.md`: patch **both** encoders to print the same
/// per-macroblock / per-block state, `diff` the two, and narrow. The C++ half is a
/// throwaway patch (`git checkout codec/` afterwards); this half stays so only the
/// C++ side has to be re-patched next time.
///
/// | variable | printed at |
/// |---|---|
/// | `OH264_MBDUMP` | per macroblock in `WelsMdInterMbLoop`, after the mode decision |
/// | `OH264_MEDUMP` | per motion-search call, inputs and result |
/// | `OH264_FPDUMP` | per macroblock in `WelsMdInterFinePartitionVaa` |
/// | `OH264_RECDUMP` | per frame in `WelsUpdateRefList`, a checksum of each reconstructed plane |
pub fn dump_enabled(cell: &std::sync::OnceLock<bool>, var: &str) -> bool {
    *cell.get_or_init(|| std::env::var_os(var).is_some())
}
