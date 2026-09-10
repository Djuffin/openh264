//! Preset generators matching the configuration matrix of `sweep.sh`.

use super::config::{BaseInitMode, DiffConfig, LtrConfig, SliceConfig, SpatialLayersConfig};
use super::inputs::{generate_screen_clip, load_looped_res, YuvClip};
use openh264_rs::api::codec_api::*;

const SMALL_INPUTS: [(&str, i32, i32); 3] = [
    ("res/CiscoVT2people_160x96_6fps.yuv", 160, 96),
    ("res/CiscoVT2people_320x192_12fps.yuv", 320, 192),
    ("res/Static_152_100.yuv", 152, 100),
];

const SLICES: [(SliceModeEnum, u32); 5] = [
    (SliceModeEnum::SM_FIXEDSLCNUM_SLICE, 2),
    (SliceModeEnum::SM_FIXEDSLCNUM_SLICE, 4),
    (SliceModeEnum::SM_RASTER_SLICE, 3),
    (SliceModeEnum::SM_SIZELIMITED_SLICE, 1500),
    (SliceModeEnum::SM_SIZELIMITED_SLICE, 600),
];

/// Single-threaded preset: 210 configurations.
pub fn preset_st() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(210);
    for &(rel_path, w, h) in &SMALL_INPUTS {
        let clip = load_looped_res(rel_path, w, h, 16);
        for &rc in &[-1, 0, 1, 2, 3] {
            let rc_enum = match rc {
                0 => RC_MODES::RC_QUALITY_MODE,
                1 => RC_MODES::RC_BITRATE_MODE,
                2 => RC_MODES::RC_BUFFERBASED_MODE,
                3 => RC_MODES::RC_TIMESTAMP_MODE,
                _ => RC_MODES::RC_OFF_MODE,
            };
            for &base in &[0, 1] {
                for &gop in &[-1, 2, 8] {
                    for &cabac in &[false, true] {
                        let label = format!(
                            "st {} rc={} base={} gop={} cabac={}",
                            clip.name, rc, base, gop, cabac as i32
                        );
                        let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
                        cfg.rc_mode = Some(rc_enum);
                        cfg.base_init = if base == 1 {
                            BaseInitMode::Base
                        } else {
                            BaseInitMode::Explicit
                        };
                        cfg.gop = gop;
                        cfg.cabac = cabac;
                        cfg.threads = 1;
                        out.push((cfg, load_looped_res(rel_path, w, h, 16)));
                    }
                }
            }
        }
        for &(sm, sn) in &SLICES {
            for &cabac in &[false, true] {
                let label = format!(
                    "st {} sm={:?} n={} cabac={}",
                    clip.name, sm, sn, cabac as i32
                );
                let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
                cfg.slice = SliceConfig { mode: sm, arg: sn };
                cfg.cabac = cabac;
                cfg.gop = -1;
                cfg.rc_mode = Some(RC_MODES::RC_QUALITY_MODE);
                cfg.threads = 1;
                out.push((cfg, load_looped_res(rel_path, w, h, 16)));
            }
        }
    }
    out
}

/// Multi-threaded slice preset: 120 configurations.
pub fn preset_mt() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(120);
    for &(rel_path, w, h) in &SMALL_INPUTS {
        let clip = load_looped_res(rel_path, w, h, 16);
        for &thr in &[2, 4] {
            for &(sm, sn) in &SLICES {
                for &cabac in &[false, true] {
                    for &rc in &[0, 1] {
                        let label = format!(
                            "mt {} t={} sm={:?} n={} cabac={} rc={}",
                            clip.name, thr, sm, sn, cabac as i32, rc
                        );
                        let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
                        cfg.threads = thr;
                        cfg.slice = SliceConfig { mode: sm, arg: sn };
                        cfg.cabac = cabac;
                        cfg.rc_mode = Some(if rc == 1 {
                            RC_MODES::RC_BITRATE_MODE
                        } else {
                            RC_MODES::RC_QUALITY_MODE
                        });
                        cfg.gop = -1;
                        out.push((cfg, load_looped_res(rel_path, w, h, 16)));
                    }
                }
            }
        }
    }
    out
}

/// Quantizer parameter breadth: 312 configurations (all 52 QPs).
pub fn preset_qp() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(312);
    for &(rel_path, w, h) in &SMALL_INPUTS {
        for qp in 0..52 {
            for &cabac in &[false, true] {
                let label = format!(
                    "qp {} qp={} cabac={}",
                    rel_path, qp, cabac as i32
                );
                let mut cfg = DiffConfig::new(label, w, h, 3);
                cfg.qp = qp;
                cfg.cabac = cabac;
                cfg.gop = -1;
                cfg.rc_mode = Some(RC_MODES::RC_OFF_MODE);
                cfg.threads = 1;
                out.push((cfg, load_looped_res(rel_path, w, h, 3)));
            }
        }
    }
    out
}

/// Default initialization flow: 11 configurations.
pub fn preset_def() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(11);
    for &(rel_path, w, h) in &SMALL_INPUTS {
        for &thr in &[1, 2, 4] {
            let label = format!("def {} t={}", rel_path, thr);
            let clip = load_looped_res(rel_path, w, h, 72);
            let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
            cfg.base_init = BaseInitMode::DefaultFlow;
            cfg.threads = thr;
            out.push((cfg, clip));
        }
    }
    // 720p HD clip
    let p720 = "res/Cisco_Absolute_Power_1280x720_30fps.yuv";
    for &thr in &[1, 4] {
        let label = format!("def 720p t={}", thr);
        let clip = load_looped_res(p720, 1280, 720, 40);
        let mut cfg = DiffConfig::new(label, 1280, 720, clip.num_frames());
        cfg.base_init = BaseInitMode::DefaultFlow;
        cfg.threads = thr;
        out.push((cfg, clip));
    }
    out
}

/// Size-limited slices driving `FrameBsRealloc`: 12 configurations.
pub fn preset_sl() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(12);
    let (rel_path, w, h) = ("res/CiscoVT2people_320x192_12fps.yuv", 320, 192);
    let sl_rows: [(i32, u32); 3] = [(26, 401), (16, 601), (10, 501)];

    for &(qp, con) in &sl_rows {
        for &rc in &[-1, 2] {
            for &cabac in &[false, true] {
                let label = format!("sl 320x192 qp={} con={} rc={} cabac={}", qp, con, rc, cabac as i32);
                let clip = load_looped_res(rel_path, w, h, 16);
                let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
                cfg.qp = qp;
                cfg.slice = SliceConfig {
                    mode: SliceModeEnum::SM_SIZELIMITED_SLICE,
                    arg: con,
                };
                cfg.rc_mode = Some(if rc == 2 {
                    RC_MODES::RC_BUFFERBASED_MODE
                } else {
                    RC_MODES::RC_OFF_MODE
                });
                cfg.cabac = cabac;
                cfg.gop = -1;
                cfg.threads = 1;
                out.push((cfg, clip));
            }
        }
    }
    out
}

/// Long-term reference feedback: 16 configurations.
pub fn preset_ltr() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(16);
    let inputs = [
        ("res/CiscoVT2people_160x96_6fps.yuv", 160, 96),
        ("res/CiscoVT2people_320x192_12fps.yuv", 320, 192),
    ];
    let ltr_rows: [(i32, u32); 8] = [
        (0, 0), (0, 1), (0, 2), (0, 3),
        (8, 0), (8, 1), (8, 2), (8, 3),
    ];

    for &(rel_path, w, h) in &inputs {
        for &(gop, fb) in &ltr_rows {
            let label = format!("ltr {} gop={} fb={}", rel_path, gop, fb);
            let clip = load_looped_res(rel_path, w, h, 72);
            let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
            cfg.qp = 26;
            cfg.cabac = true;
            cfg.gop = gop;
            cfg.rc_mode = Some(RC_MODES::RC_OFF_MODE);
            cfg.threads = 1;
            cfg.ltr = Some(LtrConfig {
                num_ref: 2,
                mark_period: 4,
                feedback_mask: fb,
            });
            out.push((cfg, clip));
        }
    }
    out
}

/// Parameter Set Strategies (eSpsPpsIdStrategy): 90 configurations.
pub fn preset_ps() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(90);
    let strategies = [
        (0, EParameterSetStrategy::CONSTANT_ID),
        (1, EParameterSetStrategy::INCREASING_ID),
        (2, EParameterSetStrategy::SPS_LISTING),
        (3, EParameterSetStrategy::SPS_LISTING_AND_PPS_INCREASING),
        (6, EParameterSetStrategy::SPS_PPS_LISTING),
    ];

    for &(rel_path, w, h) in &SMALL_INPUTS {
        for &(st_val, strategy) in &strategies {
            for &gop in &[-1, 4, 1] {
                for &cabac in &[false, true] {
                    let label = format!(
                        "ps {} strategy={} gop={} cabac={}",
                        rel_path, st_val, gop, cabac as i32
                    );
                    let clip = load_looped_res(rel_path, w, h, 16);
                    let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
                    cfg.ps_strategy = strategy;
                    cfg.gop = gop;
                    cfg.cabac = cabac;
                    cfg.rc_mode = Some(RC_MODES::RC_QUALITY_MODE);
                    cfg.threads = 1;
                    out.push((cfg, clip));
                }
            }
        }
    }
    out
}

/// Dependency & Spatial Layers: 76 configurations.
pub fn preset_dl() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(76);
    for &(rel_path, w, h) in &SMALL_INPUTS {
        for &n in &[2, 3, 4] {
            for &dn in &[false, true] {
                for &gop in &[-1, 4] {
                    for &cabac in &[false, true] {
                        let label = format!(
                            "dl {} layers={} denoise={} gop={} cabac={}",
                            rel_path, n, dn as i32, gop, cabac as i32
                        );
                        let clip = load_looped_res(rel_path, w, h, 16);
                        let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
                        cfg.spatial_layers = Some(SpatialLayersConfig {
                            num_layers: n,
                            denoise: dn,
                        });
                        cfg.gop = gop;
                        cfg.cabac = cabac;
                        cfg.rc_mode = Some(RC_MODES::RC_QUALITY_MODE);
                        cfg.threads = 1;
                        out.push((cfg, clip));
                    }
                }
            }
        }
    }
    // 720p HD clip cascading halvings
    let p720 = "res/Cisco_Absolute_Power_1280x720_30fps.yuv";
    for &n in &[2, 4] {
        for &dn in &[false, true] {
            let label = format!("dl 720p layers={} denoise={}", n, dn as i32);
            let clip = load_looped_res(p720, 1280, 720, 6);
            let mut cfg = DiffConfig::new(label, 1280, 720, clip.num_frames());
            cfg.spatial_layers = Some(SpatialLayersConfig {
                num_layers: n,
                denoise: dn,
            });
            cfg.gop = -1;
            cfg.cabac = false;
            cfg.rc_mode = Some(RC_MODES::RC_QUALITY_MODE);
            cfg.threads = 1;
            out.push((cfg, clip));
        }
    }
    out
}

/// Background detection: 24 configurations.
pub fn preset_bg() -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::with_capacity(24);
    for &(rel_path, w, h) in &SMALL_INPUTS {
        for &rc in &[-1, 2] {
            for &gop in &[-1, 4] {
                for &cabac in &[false, true] {
                    for &thr in &[1, 4] {
                        let label = format!(
                            "bg {} rc={} gop={} cabac={} t={}",
                            rel_path, rc, gop, cabac as i32, thr
                        );
                        let clip = load_looped_res(rel_path, w, h, 72);
                        let mut cfg = DiffConfig::new(label, w, h, clip.num_frames());
                        cfg.background_detection = true;
                        cfg.rc_mode = Some(if rc == 2 {
                            RC_MODES::RC_BUFFERBASED_MODE
                        } else {
                            RC_MODES::RC_OFF_MODE
                        });
                        cfg.gop = gop;
                        cfg.cabac = cabac;
                        cfg.threads = thr;
                        out.push((cfg, clip));
                    }
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SccTier {
    Min,  // 28 rows
    Gate, // 108 rows
    All,  // 148 rows
}

/// Screen Content Coding preset with pure Rust generated synthetic test clips.
pub fn preset_scc(tier: SccTier) -> Vec<(DiffConfig, YuvClip)> {
    let mut out = Vec::new();

    // 7 inputs: 3 looped camera clips + 4 synthetic screen clips
    let inputs: Vec<YuvClip> = vec![
        load_looped_res("res/CiscoVT2people_160x96_6fps.yuv", 160, 96, 60),
        load_looped_res("res/CiscoVT2people_320x192_12fps.yuv", 320, 192, 60),
        load_looped_res("res/Static_152_100.yuv", 152, 100, 60),
        generate_screen_clip("scc_text_320x192_k3", 320, 192, 60, 3, 20, 7, 1),
        generate_screen_clip("scc_text_320x192_k17", 320, 192, 60, 17, 20, 0, 2),
        generate_screen_clip("scc_text_160x96_k1", 160, 96, 60, 1, 0, 7, 3),
        generate_screen_clip("scc_text_640x368_k8", 640, 368, 40, 8, 20, 7, 4),
    ];

    // Min tier (28 rows): RC off, single slice, 1 thread
    for clip in &inputs {
        for &gop in &[-1, 4] {
            for &cabac in &[false, true] {
                let label = format!("scc-min {} gop={} cabac={}", clip.name, gop, cabac as i32);
                let mut cfg = DiffConfig::new(label, clip.width, clip.height, clip.num_frames());
                cfg.usage = EUsageType::SCREEN_CONTENT_REAL_TIME;
                cfg.gop = gop;
                cfg.cabac = cabac;
                cfg.rc_mode = Some(RC_MODES::RC_OFF_MODE);
                cfg.threads = 1;
                out.push((cfg, YuvClip {
                    name: clip.name.clone(),
                    width: clip.width,
                    height: clip.height,
                    frames_yuv: clip.frames_yuv.clone(),
                }));
            }
        }
    }

    if tier == SccTier::Min {
        return out;
    }

    // Wide tier (120 rows) on 5 selected inputs: inputs[0, 1, 2, 3, 6]
    let wide_indices = [0, 1, 2, 3, 6];
    for &idx in &wide_indices {
        let clip = &inputs[idx];
        for &rc in &[1, 2] {
            let rc_enum = if rc == 1 {
                RC_MODES::RC_BITRATE_MODE
            } else {
                RC_MODES::RC_BUFFERBASED_MODE
            };
            for &gop in &[-1, 4] {
                for &cabac in &[false, true] {
                    // Single slice, 1 thread
                    let label = format!(
                        "scc {} rc={} gop={} cabac={} sm=0 t=1",
                        clip.name, rc, gop, cabac as i32
                    );
                    let mut cfg = DiffConfig::new(label, clip.width, clip.height, clip.num_frames());
                    cfg.usage = EUsageType::SCREEN_CONTENT_REAL_TIME;
                    cfg.rc_mode = Some(rc_enum);
                    cfg.gop = gop;
                    cfg.cabac = cabac;
                    cfg.threads = 1;
                    out.push((cfg, YuvClip {
                        name: clip.name.clone(),
                        width: clip.width,
                        height: clip.height,
                        frames_yuv: clip.frames_yuv.clone(),
                    }));

                    // Size-limited slices, 4 threads (skipped in Gate tier due to C++ reference race)
                    if tier == SccTier::All {
                        let label = format!(
                            "scc {} rc={} gop={} cabac={} sm=3 t=4",
                            clip.name, rc, gop, cabac as i32
                        );
                        let mut cfg = DiffConfig::new(label, clip.width, clip.height, clip.num_frames());
                        cfg.usage = EUsageType::SCREEN_CONTENT_REAL_TIME;
                        cfg.rc_mode = Some(rc_enum);
                        cfg.gop = gop;
                        cfg.cabac = cabac;
                        cfg.slice = SliceConfig {
                            mode: SliceModeEnum::SM_SIZELIMITED_SLICE,
                            arg: 1500,
                        };
                        cfg.threads = 4;
                        out.push((cfg, YuvClip {
                            name: clip.name.clone(),
                            width: clip.width,
                            height: clip.height,
                            frames_yuv: clip.frames_yuv.clone(),
                        }));
                    }
                }
            }
        }

        // LTR over lossless link
        for &rc in &[-1, 2] {
            let rc_enum = if rc == 2 {
                RC_MODES::RC_BUFFERBASED_MODE
            } else {
                RC_MODES::RC_OFF_MODE
            };
            for &cabac in &[false, true] {
                for &thr in &[1, 4] {
                    let label = format!(
                        "scc {} rc={} cabac={} t={} ltr=4 lossless",
                        clip.name, rc, cabac as i32, thr
                    );
                    let mut cfg = DiffConfig::new(label, clip.width, clip.height, clip.num_frames());
                    cfg.usage = EUsageType::SCREEN_CONTENT_REAL_TIME;
                    cfg.rc_mode = Some(rc_enum);
                    cfg.cabac = cabac;
                    cfg.threads = thr;
                    cfg.lossless = true;
                    cfg.ltr = Some(LtrConfig {
                        num_ref: 4,
                        mark_period: 30,
                        feedback_mask: 0,
                    });
                    out.push((cfg, YuvClip {
                        name: clip.name.clone(),
                        width: clip.width,
                        height: clip.height,
                        frames_yuv: clip.frames_yuv.clone(),
                    }));
                }
            }
        }
    }

    out
}
