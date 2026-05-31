use std::fs;
use std::path::Path;

use super::{GpuInfo, pci_model, read_file_f32, read_file_u64};

pub(super) fn read(device: &Path) -> GpuInfo {
    let util = read_file_f32(&device.join("gpu_busy_percent")).unwrap_or(0.0);
    let mem_used = read_file_u64(&device.join("mem_info_vram_used")).unwrap_or(0);
    let mem_total = read_file_u64(&device.join("mem_info_vram_total")).unwrap_or(0);
    // hwmon temp (in millidegC) — look for any hwmon entry under device/hwmon/*/temp1_input
    let mut temp_c = 0.0;
    if let Ok(rd) = fs::read_dir(device.join("hwmon")) {
        for entry in rd.flatten() {
            if let Some(v) = read_file_f32(&entry.path().join("temp1_input")) {
                temp_c = v / 1000.0;
                break;
            }
        }
    }
    GpuInfo {
        vendor: "AMD".into(),
        name: pci_model(device).unwrap_or_else(|| "AMD GPU".into()),
        util_pct: util,
        mem_used,
        mem_total,
        temp_c,
        power_w: 0.0,
        clock_mhz: 0,
        mem_clock_mhz: 0,
        driver: "amdgpu".into(),
    }
}
