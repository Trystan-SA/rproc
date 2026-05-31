//! Per-process GPU utilization, the optional companion to the CPU/RAM/disk
//! attribution. Two independent, vendor-specific sources feed one merged list:
//!
//! * **NVIDIA** — NVML `process_utilization_stats`, which reports recent
//!   per-PID SM (compute/graphics) utilization. The proprietary driver does
//!   not expose the DRM fdinfo counters below, so this is the only path.
//! * **AMD / Intel** (and any other DRM driver: nouveau, etc.) —
//!   `/proc/<pid>/fdinfo` `drm-engine-*` nanosecond counters, delta'd against
//!   the previous sample over the wall-clock interval to yield a busy%.
//!
//! Both sources are stateful (NVML last-seen timestamp; fdinfo ns deltas), so
//! this is a collector the sampler owns and ticks once per sample — and only
//! while the attribution feature is on.
//!
//! Cost note: the fdinfo path walks every visible PID's open fds each tick,
//! which is the heaviest scan in the whole sampler. It is gated to systems that
//! actually have an AMD/Intel DRM card, and the whole feature is opt-in, so a
//! default run never pays for it. Reading another user's `fdinfo` needs
//! privileges, so unprivileged runs attribute only the current user's GPU
//! processes (NVML still reports all PIDs on the device).

use std::collections::HashMap;
use std::fs;

use nvml_wrapper::Nvml;
use sysinfo::{Pid, System};

use super::attribution::{ProcShare, TOP_N};

const VENDOR_AMD: &str = "0x1002";
const VENDOR_INTEL: &str = "0x8086";

pub struct GpuAttribution {
    /// Whether any AMD/Intel DRM card is present. Gates the costly fdinfo walk
    /// so NVIDIA-only or GPU-less systems never pay for it.
    scan_fdinfo: bool,
    /// Per-PID cumulative GPU-engine nanoseconds from the previous sample, used
    /// to derive a busy% over the wall-clock delta.
    prev_busy_ns: HashMap<u32, u64>,
    /// Highest NVML sample timestamp (μs) consumed so far, so each tick only
    /// pulls genuinely fresh per-process samples.
    nvml_last_ts: u64,
}

impl GpuAttribution {
    pub fn init() -> Self {
        Self {
            scan_fdinfo: has_amd_or_intel_drm(),
            prev_busy_ns: HashMap::new(),
            nvml_last_ts: 0,
        }
    }

    /// Top GPU contributors this sample, as utilization percentages. Empty when
    /// no source is available. `delta_secs` is the wall-clock spacing used to
    /// turn fdinfo nanosecond deltas into a busy%.
    pub fn sample(&mut self, sys: &System, nvml: Option<&Nvml>, delta_secs: f64) -> Vec<ProcShare> {
        // A PID lives on one GPU vendor, so the two sources rarely touch the
        // same key; when they do we keep the peak rather than sum (avoids a
        // double-counted >100%).
        let mut shares: HashMap<u32, f32> = HashMap::new();

        if self.scan_fdinfo {
            self.sample_fdinfo(sys, delta_secs, &mut shares);
        }
        if let Some(nvml) = nvml {
            self.sample_nvml(nvml, &mut shares);
        }

        let mut out: Vec<ProcShare> = shares
            .into_iter()
            .filter(|&(_, v)| v > 0.0)
            .map(|(pid, value)| ProcShare {
                pid,
                name: proc_name(sys, pid),
                value,
                bytes: 0,
            })
            .collect();
        out.sort_unstable_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(TOP_N);
        out
    }

    fn sample_fdinfo(&mut self, sys: &System, delta_secs: f64, shares: &mut HashMap<u32, f32>) {
        let dt_ns = delta_secs.max(1e-3) * 1e9;
        let mut cur: HashMap<u32, u64> = HashMap::new();
        for pid in sys.processes().keys() {
            let pid = pid.as_u32();
            if let Some(busy) = read_pid_busy_ns(pid) {
                cur.insert(pid, busy);
            }
        }
        for (&pid, &busy) in &cur {
            // First time we see a PID we have no baseline; treat it as 0% this
            // tick rather than charging it all its lifetime's engine time.
            let prev = self.prev_busy_ns.get(&pid).copied().unwrap_or(busy);
            let delta = busy.saturating_sub(prev);
            let pct = ((delta as f64 / dt_ns) * 100.0).clamp(0.0, 100.0) as f32;
            let e = shares.entry(pid).or_insert(0.0);
            *e = e.max(pct);
        }
        // Drop PIDs that no longer hold a DRM fd so the baseline map can't grow
        // without bound.
        self.prev_busy_ns = cur;
    }

    fn sample_nvml(&mut self, nvml: &Nvml, shares: &mut HashMap<u32, f32>) {
        let Ok(count) = nvml.device_count() else {
            return;
        };
        let mut max_ts = self.nvml_last_ts;
        for i in 0..count {
            let Ok(dev) = nvml.device_by_index(i) else {
                continue;
            };
            // A NotFound/empty result just means no recent per-process activity.
            if let Ok(samples) = dev.process_utilization_stats(self.nvml_last_ts) {
                for s in samples {
                    max_ts = max_ts.max(s.timestamp);
                    let e = shares.entry(s.pid).or_insert(0.0);
                    *e = e.max(s.sm_util as f32);
                }
            }
        }
        self.nvml_last_ts = max_ts;
    }
}

/// Sum the GPU-engine busy nanoseconds across this PID's DRM clients, or `None`
/// if it holds no DRM fd. Deduplicates by `drm-client-id` so dup'd fds (which
/// report identical cumulative counters) aren't counted multiple times.
fn read_pid_busy_ns(pid: u32) -> Option<u64> {
    let entries = fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    let mut per_client: HashMap<u64, u64> = HashMap::new();
    let mut any = false;
    for e in entries.flatten() {
        // Cheap filter: only fds pointing at a DRM node can carry engine stats.
        let is_dri = fs::read_link(e.path())
            .map(|l| l.to_string_lossy().contains("/dri/"))
            .unwrap_or(false);
        if !is_dri {
            continue;
        }
        let fdnum = e.file_name();
        let info = format!("/proc/{pid}/fdinfo/{}", fdnum.to_string_lossy());
        let Ok(text) = fs::read_to_string(&info) else {
            continue;
        };
        if let Some((client, busy)) = parse_fdinfo(&text) {
            any = true;
            let slot = per_client.entry(client).or_insert(0);
            *slot = (*slot).max(busy);
        }
    }
    any.then(|| per_client.values().sum())
}

/// Parse one DRM `fdinfo` blob into `(client_id, summed_engine_ns)`, or `None`
/// if it isn't a DRM fd. Lines look like:
///
/// ```text
/// drm-driver:           i915
/// drm-client-id:        12345
/// drm-engine-render:    1856738263588 ns
/// drm-engine-capacity-video: 2          (a capacity, not ns — skipped)
/// ```
fn parse_fdinfo(text: &str) -> Option<(u64, u64)> {
    let mut client: Option<u64> = None;
    let mut busy: u64 = 0;
    let mut is_drm = false;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        match k {
            "drm-driver" => is_drm = true,
            "drm-client-id" => client = v.parse().ok(),
            _ if k.starts_with("drm-engine-") && !k.starts_with("drm-engine-capacity") => {
                // Value is "<ns> ns"; tolerate a bare integer too.
                let digits = v.strip_suffix("ns").unwrap_or(v).trim();
                if let Ok(ns) = digits.parse::<u64>() {
                    busy = busy.saturating_add(ns);
                }
            }
            _ => {}
        }
    }
    if !is_drm {
        return None;
    }
    // Some drivers omit drm-client-id; fall back to a single synthetic client.
    Some((client.unwrap_or(0), busy))
}

fn proc_name(sys: &System, pid: u32) -> String {
    sys.process(Pid::from_u32(pid))
        .map(|p| p.name().to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("pid {pid}"))
}

/// True when at least one AMD or Intel DRM card is present — i.e. the fdinfo
/// engine counters are worth scanning for.
fn has_amd_or_intel_drm() -> bool {
    let Ok(rd) = fs::read_dir("/sys/class/drm") else {
        return false;
    };
    for entry in rd.flatten() {
        let n = entry.file_name();
        let n = n.to_string_lossy();
        if !n.starts_with("card") || n.contains('-') {
            continue;
        }
        let vendor = fs::read_to_string(entry.path().join("device/vendor")).unwrap_or_default();
        if matches!(vendor.trim(), VENDOR_AMD | VENDOR_INTEL) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const I915_SAMPLE: &str = "\
pos:	0
flags:	02100002
drm-driver:	i915
drm-client-id:	42
drm-total-system0:	288036 KiB
drm-engine-render:	1000 ns
drm-engine-copy:	200 ns
drm-engine-video:	0 ns
drm-engine-capacity-video:	2
drm-engine-compute:	50 ns
";

    #[test]
    fn parse_fdinfo_sums_engine_ns_and_skips_capacity() {
        let (client, busy) = parse_fdinfo(I915_SAMPLE).unwrap();
        assert_eq!(client, 42);
        // render + copy + video + compute = 1000 + 200 + 0 + 50; capacity ignored.
        assert_eq!(busy, 1250);
    }

    #[test]
    fn parse_fdinfo_rejects_non_drm() {
        assert!(parse_fdinfo("pos:\t0\nflags:\t02000002\n").is_none());
    }

    #[test]
    fn parse_fdinfo_missing_client_id_uses_fallback() {
        let text = "drm-driver:\tamdgpu\ndrm-engine-gfx:\t500 ns\n";
        let (client, busy) = parse_fdinfo(text).unwrap();
        assert_eq!(client, 0);
        assert_eq!(busy, 500);
    }
}
