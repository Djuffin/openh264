//! Strongly-typed configuration structures for the OpenH264 differential test harness.

use openh264_rs::api::codec_api::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseInitMode {
    /// InitializeExt with explicit gate config (standard).
    Explicit,
    /// Initialize(SEncParamBase) — the BaseEncoderTest::InitWithParam path.
    Base,
    /// GetDefaultParams + InitializeExt with width/height/framerate/bitrate/threads only.
    DefaultFlow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceConfig {
    pub mode: SliceModeEnum,
    /// Slice count for mode 1/2, MB rows per slice for mode 2 (if specified),
    /// or byte constraint for mode 3 (SM_SIZELIMITED_SLICE).
    pub arg: u32,
}

impl Default for SliceConfig {
    fn default() -> Self {
        Self {
            mode: SliceModeEnum::SM_SINGLE_SLICE,
            arg: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LtrConfig {
    pub num_ref: i32,
    pub mark_period: u32,
    /// LTR feedback bitmask: bit 0 = marking success, bit 1 = recovery request.
    pub feedback_mask: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialLayersConfig {
    pub num_layers: i32,
    pub denoise: bool,
}

#[derive(Debug, Clone)]
pub struct DiffConfig {
    pub label: String,
    pub width: i32,
    pub height: i32,
    pub frames: usize,
    pub qp: i32,
    pub cabac: bool,
    pub gop: i32,
    pub rc_mode: Option<RC_MODES>,
    pub base_init: BaseInitMode,
    pub slice: SliceConfig,
    pub threads: u16,
    pub complexity: ECOMPLEXITY_MODE,
    pub ltr: Option<LtrConfig>,
    pub ps_strategy: EParameterSetStrategy,
    pub spatial_layers: Option<SpatialLayersConfig>,
    pub background_detection: bool,
    pub usage: EUsageType,
    pub lossless: bool,
    pub set_opt_ext_frame: Option<usize>,
}

impl DiffConfig {
    pub fn new(label: impl Into<String>, width: i32, height: i32, frames: usize) -> Self {
        Self {
            label: label.into(),
            width,
            height,
            frames,
            qp: 26,
            cabac: false,
            gop: -1,
            rc_mode: Some(RC_MODES::RC_OFF_MODE),
            base_init: BaseInitMode::Explicit,
            slice: SliceConfig::default(),
            threads: 1,
            complexity: ECOMPLEXITY_MODE::LOW_COMPLEXITY,
            ltr: None,
            ps_strategy: EParameterSetStrategy::CONSTANT_ID,
            spatial_layers: None,
            background_detection: false,
            usage: EUsageType::CAMERA_VIDEO_REAL_TIME,
            lossless: false,
            set_opt_ext_frame: None,
        }
    }
}
