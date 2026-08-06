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
pub mod set_mb_syn_cabac;
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
pub mod wels_task_management;

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
