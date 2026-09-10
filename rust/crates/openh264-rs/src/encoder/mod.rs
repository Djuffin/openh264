#![deny(unsafe_code)]
pub mod abi_guard;
pub mod au_set;
pub mod deblocking;
pub mod decode_mb_aux;
pub mod encode_mb_aux;
pub mod encoder_context;
pub mod encoder_ext;
pub mod get_intra_predictor;
pub mod md;
pub mod nal_encap;
pub mod param_svc;
pub mod paraset_strategy;
pub mod picture;
pub mod rc;
pub mod rec_view;
pub mod ref_list_mgr_svc;
pub mod sample;
pub mod set_mb_syn_cabac;
pub mod slice_multi_threading;
pub mod svc_base_layer_md;
pub mod svc_enc_slice_segment;
pub mod svc_encode_mb;
pub mod svc_encode_slice;
pub mod svc_mode_decision;
pub mod svc_motion_estimate;
pub mod svc_set_mb_syn_cabac;
pub mod svc_set_mb_syn_cavlc;
pub mod vlc_encoder;
pub mod wels_encoder_ext;
pub mod wels_func_ptr_def;
pub mod wels_preprocess;
pub mod worker_pool;

/// Whether an `OH264_*DUMP` debugging dump is switched on, cached so callers pay one
/// relaxed load rather than an environment scan.
///
/// Recognised variables: `OH264_MBDUMP` (per macroblock, after the mode decision),
/// `OH264_MEDUMP` (per motion search), `OH264_FPDUMP` (per fine-partition macroblock),
/// `OH264_RECDUMP` (per frame, a checksum of each reconstructed plane).
pub fn dump_enabled(cell: &std::sync::OnceLock<bool>, var: &str) -> bool {
    *cell.get_or_init(|| std::env::var_os(var).is_some())
}
