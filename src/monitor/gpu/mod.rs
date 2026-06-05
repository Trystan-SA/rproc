use std::fs;
use std::path::{Path, PathBuf};

use nvml_wrapper::Nvml;

mod amd;
mod intel;
mod nvidia;

use intel::IntelFdinfo;

#[derive(Clone, Default, Debug)]
pub struct GpuInfo {
    pub vendor: String,
    pub name: String,
    pub util_pct: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Memory is carved out of system RAM (integrated GPU): `mem_total` is
    /// the shared ceiling, not dedicated VRAM.
    pub mem_shared: bool,
    pub temp_c: f32,
    pub power_w: f32,
    pub clock_mhz: u32,
    pub mem_clock_mhz: u32,
    pub driver: String,
}

pub struct GpuCollector {
    nvml: Option<Nvml>,
    amd_cards: Vec<(PathBuf, String)>,
    intel_cards: Vec<(PathBuf, String)>,
    intel_fdinfo: Vec<Option<IntelFdinfo>>,
    ram_total: u64,
}

impl GpuCollector {
    pub fn init() -> Self {
        let nvml = Nvml::init().ok();
        let (amd, intel) = scan_drm();
        // Utilization comes from per-client fdinfo busy counters; each sampler
        // matches its own card by PCI slot, so multiple Intel GPUs disambiguate.
        let intel_fdinfo: Vec<Option<IntelFdinfo>> =
            intel.iter().map(|d| IntelFdinfo::new(d)).collect();
        // The GPU name is static, but resolving it via `pci_model` can spawn
        // `glxinfo` (a full GL context). Resolve once here so per-tick sampling
        // never forks a subprocess — re-spawning it every tick stalled the UI.
        let amd_cards = amd
            .into_iter()
            .map(|d| {
                let name = pci_model(&d).unwrap_or_else(|| "AMD GPU".into());
                (d, name)
            })
            .collect();
        let intel_cards = intel
            .into_iter()
            .map(|d| {
                let name = pci_model(&d).unwrap_or_else(|| "Intel GPU".into());
                (d, name)
            })
            .collect();
        Self {
            nvml,
            amd_cards,
            intel_cards,
            intel_fdinfo,
            ram_total: meminfo_total_bytes(),
        }
    }

    /// Borrow the NVML handle (if any) so the per-process GPU attribution can
    /// query `process_utilization_stats` without owning a second NVML init.
    pub fn nvml(&self) -> Option<&Nvml> {
        self.nvml.as_ref()
    }

    pub fn sample(&mut self) -> Vec<GpuInfo> {
        let mut out = Vec::new();
        if let Some(nvml) = &self.nvml
            && let Ok(count) = nvml.device_count()
        {
            let driver = nvml.sys_driver_version().unwrap_or_default();
            for i in 0..count {
                if let Ok(dev) = nvml.device_by_index(i) {
                    out.push(nvidia::read(&dev, &driver));
                }
            }
        }
        for (p, name) in &self.amd_cards {
            out.push(amd::read(p, name));
        }
        for ((p, name), fdinfo) in self.intel_cards.iter().zip(self.intel_fdinfo.iter_mut()) {
            out.push(intel::read(p, fdinfo.as_mut(), name, self.ram_total));
        }
        out
    }
}

fn scan_drm() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut amd = Vec::new();
    let mut intel = Vec::new();
    let Ok(rd) = fs::read_dir("/sys/class/drm") else {
        return (amd, intel);
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if !n.starts_with("card") || n.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        let vendor = fs::read_to_string(device.join("vendor")).unwrap_or_default();
        match vendor.trim() {
            "0x1002" => amd.push(device),
            "0x8086" => intel.push(device),
            _ => {}
        }
    }
    (amd, intel)
}

/// Shared-memory ceiling for integrated GPUs, whose buffers live in RAM.
fn meminfo_total_bytes() -> u64 {
    let Ok(s) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    s.lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|v| v.trim().strip_suffix("kB"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

fn read_file_u64(p: &PathBuf) -> Option<u64> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn read_file_f32(p: &PathBuf) -> Option<f32> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn pci_model(device: &Path) -> Option<String> {
    // 1. Try the label file
    if let Ok(label) = fs::read_to_string(device.join("label")) {
        let t = label.trim();
        if !t.is_empty() {
            return Some(t.into());
        }
    }

    // 2. Try glxinfo for the active GPU name (works for AMD/Intel GPUs)
    if let Ok(output) = std::process::Command::new("glxinfo").args(["-B"]).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(renderer) = line.strip_prefix("OpenGL renderer string: ") {
                // Strip driver details in parentheses: "AMD Radeon RX 6750 XT"
                let name = renderer.split(" (").next().unwrap_or(renderer);
                return Some(name.trim().to_string());
            }
        }
    }

    // 3. Fall back to device directory name
    device
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
}
