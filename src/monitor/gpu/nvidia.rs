use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};

use super::GpuInfo;

pub(super) fn read(dev: &nvml_wrapper::Device, driver: &str) -> GpuInfo {
    let name = dev.name().unwrap_or_else(|_| "NVIDIA GPU".into());
    let util = dev.utilization_rates().map(|u| u.gpu as f32).unwrap_or(0.0);
    let mem = dev.memory_info().ok();
    let temp = dev.temperature(TemperatureSensor::Gpu).unwrap_or(0) as f32;
    let power = dev.power_usage().map(|p| p as f32 / 1000.0).unwrap_or(0.0);
    let clock = dev.clock_info(Clock::Graphics).unwrap_or(0);
    let mem_clock = dev.clock_info(Clock::Memory).unwrap_or(0);
    GpuInfo {
        vendor: "NVIDIA".into(),
        name,
        util_pct: util,
        mem_used: mem.as_ref().map(|m| m.used).unwrap_or(0),
        mem_total: mem.map(|m| m.total).unwrap_or(0),
        temp_c: temp,
        power_w: power,
        clock_mhz: clock,
        mem_clock_mhz: mem_clock,
        driver: driver.to_string(),
    }
}
