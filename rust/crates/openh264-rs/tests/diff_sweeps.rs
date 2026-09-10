//! Integration test suite executing full differential sweeps against C++ OpenH264 in-memory.

mod diff_harness;
use diff_harness::*;

#[test]
fn test_sweep_st() {
    let configs = presets::preset_st();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_mt() {
    let configs = presets::preset_mt();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_qp() {
    let configs = presets::preset_qp();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_def() {
    let configs = presets::preset_def();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_sl() {
    let configs = presets::preset_sl();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_ltr() {
    let configs = presets::preset_ltr();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_ps() {
    let configs = presets::preset_ps();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
#[cfg_attr(target_arch = "x86_64", ignore = "C++ reference on x86 uses SSE downsampler with divergent rounding")]
fn test_sweep_dl() {
    let configs = presets::preset_dl();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_bg() {
    let configs = presets::preset_bg();
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_scc_min() {
    let configs = presets::preset_scc(presets::SccTier::Min);
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}

#[test]
fn test_sweep_scc_gate() {
    let configs = presets::preset_scc(presets::SccTier::Gate);
    for (config, clip) in configs {
        run_diff_config(&config, &clip);
    }
}
