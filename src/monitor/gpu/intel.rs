//! Intel i915 / xe GPU sampling.
//!
//! sysfs exposes Intel GPU frequency and temperature but not utilization. The
//! i915/xe PMU can report engine-busy, but `perf_event_open` needs CAP_PERFMON
//! or `kernel.perf_event_paranoid <= 2`, so it silently fails on most desktops.
//!
//! Instead we derive utilization the way `intel_gpu_top`/nvtop do for global
//! busyness: each DRM client exposes cumulative per-engine busy nanoseconds in
//! `/proc/<pid>/fdinfo/<fd>` (`drm-engine-render`, `drm-engine-compute`, ...).
//! Busy% over an interval is `Σ Δengine_ns / Δwall_ns`, summed across clients of
//! this card. No special privilege is needed to read fdinfo for processes we can
//! already see, which is why it works where the perf path is refused.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use super::{GpuInfo, pci_model, read_file_f32, read_file_u64};

pub(super) fn read(device: &Path, fdinfo: Option<&mut IntelFdinfo>) -> GpuInfo {
    let cur_freq = read_file_u64(&device.join("gt_act_freq_mhz"))
        .or_else(|| read_file_u64(&device.join("gt/gt0/rps_act_freq_mhz")))
        .unwrap_or(0) as u32;
    let mut temp_c = 0.0;
    if let Ok(rd) = fs::read_dir(device.join("hwmon")) {
        for entry in rd.flatten() {
            if let Some(v) = read_file_f32(&entry.path().join("temp1_input")) {
                temp_c = v / 1000.0;
                break;
            }
        }
    }
    // NaN sentinel = "utilization unavailable" (couldn't resolve the card's PCI
    // slot to match fdinfo against). The UI renders this as "N/A" rather than a
    // misleading 0 %.
    let util_pct = match fdinfo {
        Some(f) => f.sample(),
        None => f32::NAN,
    };
    GpuInfo {
        vendor: "Intel".into(),
        name: pci_model(device).unwrap_or_else(|| "Intel GPU".into()),
        util_pct,
        mem_used: 0,
        mem_total: 0,
        temp_c,
        power_w: 0.0,
        clock_mhz: cur_freq,
        mem_clock_mhz: 0,
        driver: "i915/xe".into(),
    }
}

/// Per-card fdinfo utilization sampler. Matches DRM clients by the card's PCI
/// slot (`drm-pdev`) so multiple Intel GPUs are disambiguated.
pub(super) struct IntelFdinfo {
    pdev: String,
    prev: HashMap<u64, u64>,
    last: Option<Instant>,
}

impl IntelFdinfo {
    pub(super) fn new(device: &Path) -> Option<Self> {
        let pdev = pdev_of(device)?;
        Some(Self {
            pdev,
            prev: HashMap::new(),
            last: None,
        })
    }

    fn sample(&mut self) -> f32 {
        let now = Instant::now();
        let cur = self.collect_busy_ns();

        let util = match self.last {
            None => 0.0,
            Some(last) => {
                let dt = now.duration_since(last).as_nanos();
                if dt == 0 {
                    0.0
                } else {
                    // Engine counters are cumulative and monotonic per client;
                    // a counter reset (cur < prev) contributes nothing.
                    let busy: u64 = cur
                        .iter()
                        .filter_map(|(cid, &ns)| Some(ns.saturating_sub(*self.prev.get(cid)?)))
                        .sum();
                    ((busy as f64 / dt as f64) * 100.0).min(100.0) as f32
                }
            }
        };

        self.prev = cur;
        self.last = Some(now);
        util
    }

    /// One busy-ns total per DRM client of this card. Keyed by client id so the
    /// same client seen through several fds/pids is counted once.
    fn collect_busy_ns(&self) -> HashMap<u64, u64> {
        let mut out = HashMap::new();
        let Ok(procs) = fs::read_dir("/proc") else {
            return out;
        };
        for proc in procs.flatten() {
            let name = proc.file_name();
            let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) else {
                continue;
            };
            for fd in fds.flatten() {
                // Cheap pre-filter: only read fdinfo for fds pointing at the DRM
                // subsystem, skipping the thousands of socket/file fds.
                match fs::read_link(fd.path()) {
                    Ok(target) if target.starts_with("/dev/dri/") => {}
                    _ => continue,
                }
                let fd_name = fd.file_name();
                let info = format!("/proc/{pid}/fdinfo/{}", fd_name.to_string_lossy());
                let Ok(content) = fs::read_to_string(&info) else {
                    continue;
                };
                if let Some((cid, ns)) = parse_fdinfo(&content, &self.pdev) {
                    out.insert(cid, ns);
                }
            }
        }
        out
    }
}

/// Parses a DRM fdinfo file, returning `(client_id, busy_ns)` for the render +
/// compute engines if the entry belongs to `pdev`, else `None`.
fn parse_fdinfo(content: &str, pdev: &str) -> Option<(u64, u64)> {
    let mut client_id = None;
    let mut matched_pdev = false;
    let mut busy_ns = 0u64;
    for line in content.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let val = val.trim();
        match key.trim() {
            "drm-pdev" => {
                if val != pdev {
                    return None;
                }
                matched_pdev = true;
            }
            "drm-client-id" => client_id = val.parse().ok(),
            "drm-engine-render" | "drm-engine-compute" => {
                if let Some(ns) = val
                    .strip_suffix(" ns")
                    .and_then(|n| n.trim().parse::<u64>().ok())
                {
                    busy_ns = busy_ns.saturating_add(ns);
                }
            }
            _ => {}
        }
    }
    if matched_pdev {
        client_id.map(|c| (c, busy_ns))
    } else {
        None
    }
}

/// Resolves a `/sys/class/drm/cardN/device` path to its PCI slot string (e.g.
/// `0000:00:02.0`), which is what fdinfo reports as `drm-pdev`.
fn pdev_of(device: &Path) -> Option<String> {
    fs::canonicalize(device)
        .ok()?
        .file_name()?
        .to_str()
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "intel_tests.rs"]
mod tests;
