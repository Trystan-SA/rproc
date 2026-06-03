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
    pub temp_c: f32,
    pub power_w: f32,
    pub clock_mhz: u32,
    pub mem_clock_mhz: u32,
    pub driver: String,
}

pub struct GpuCollector {
    nvml: Option<Nvml>,
    amd_cards: Vec<PathBuf>,
    intel_cards: Vec<PathBuf>,
    intel_fdinfo: Vec<Option<IntelFdinfo>>,
}

impl GpuCollector {
    pub fn init() -> Self {
        let nvml = Nvml::init().ok();
        let (amd, intel) = scan_drm();
        // Utilization comes from per-client fdinfo busy counters; each sampler
        // matches its own card by PCI slot, so multiple Intel GPUs disambiguate.
        let intel_fdinfo: Vec<Option<IntelFdinfo>> =
            intel.iter().map(|d| IntelFdinfo::new(d)).collect();
        Self {
            nvml,
            amd_cards: amd,
            intel_cards: intel,
            intel_fdinfo,
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
        for p in &self.amd_cards {
            out.push(amd::read(p));
        }
        for (p, fdinfo) in self.intel_cards.iter().zip(self.intel_fdinfo.iter_mut()) {
            out.push(intel::read(p, fdinfo.as_mut()));
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
