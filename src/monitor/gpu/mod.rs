use std::fs;
use std::path::{Path, PathBuf};

use nvml_wrapper::Nvml;

mod amd;
mod intel;
mod nvidia;

use intel::IntelPmu;

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
    intel_pmus: Vec<Option<IntelPmu>>,
}

impl GpuCollector {
    pub fn init() -> Self {
        let nvml = Nvml::init().ok();
        let (amd, intel) = scan_drm();
        // The i915/xe PMU is system-wide (a single counter set for all engines).
        // Open it once and attach it to the first Intel card; additional cards
        // fall back to 0 % until per-card disambiguation is wired up.
        let mut intel_pmus: Vec<Option<IntelPmu>> = intel.iter().map(|_| None).collect();
        if !intel_pmus.is_empty() {
            intel_pmus[0] = IntelPmu::open();
        }
        Self {
            nvml,
            amd_cards: amd,
            intel_cards: intel,
            intel_pmus,
        }
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
        for (p, pmu) in self.intel_cards.iter().zip(self.intel_pmus.iter_mut()) {
            out.push(intel::read(p, pmu.as_mut()));
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
    // Try modalias / device / vendor id labels — fall back to drm card name.
    if let Ok(label) = fs::read_to_string(device.join("label")) {
        let t = label.trim();
        if !t.is_empty() {
            return Some(t.into());
        }
    }
    device
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
}
