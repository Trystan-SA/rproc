use slint::{ModelRc, SharedString, VecModel};

use crate::settings::{REFRESH_PRESETS, Settings};
use crate::{MainWindow, RefreshPreset};

fn ss(s: &str) -> SharedString {
    s.into()
}

fn format_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}

pub fn apply(window: &MainWindow, settings: &Settings) {
    let current = settings.refresh_ms();
    let presets: Vec<RefreshPreset> = REFRESH_PRESETS
        .iter()
        .map(|(ms, label)| RefreshPreset {
            ms: *ms as i32,
            label: ss(label),
            selected: current == *ms,
        })
        .collect();
    window.set_set_presets(ModelRc::new(VecModel::from(presets)));
    window.set_set_refresh_ms(current as i32);
    window.set_set_refresh_label(ss(&format!(
        "Currently sampling every {}",
        format_ms(current)
    )));

    let daemon = settings.daemon_enabled();
    window.set_set_daemon_enabled(daemon);
    window.set_set_daemon_status(ss(if daemon {
        "Background sampler running"
    } else {
        "Background sampler off"
    }));

    let attribution = settings.attribution_enabled();
    window.set_set_attribution_enabled(attribution);
    window.set_set_attribution_status(ss(if attribution {
        "Attribution on — hover the CPU / Memory / Disk / GPU graphs"
    } else {
        "Attribution off"
    }));

    let gpu = settings.gpu_enabled();
    window.set_set_gpu_enabled(gpu);
    window.set_set_gpu_status(ss(if gpu {
        "GPU monitoring on"
    } else {
        "GPU monitoring off — NVML/CUDA not loaded"
    }));

    window.set_set_dark_mode(settings.dark_mode());

    window.set_set_version(ss(env!("CARGO_PKG_VERSION")));
    window.set_set_build(ss(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }));
}
