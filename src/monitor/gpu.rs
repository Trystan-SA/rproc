use std::fs;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use libc::c_int;
use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};

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
                    out.push(read_nvml(&dev, &driver));
                }
            }
        }
        for p in &self.amd_cards {
            out.push(read_amd(p));
        }
        for (p, pmu) in self.intel_cards.iter().zip(self.intel_pmus.iter_mut()) {
            out.push(read_intel(p, pmu.as_mut()));
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

fn read_nvml(dev: &nvml_wrapper::Device, driver: &str) -> GpuInfo {
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

fn read_file_u64(p: &PathBuf) -> Option<u64> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn read_file_f32(p: &PathBuf) -> Option<f32> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn read_amd(device: &Path) -> GpuInfo {
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

fn read_intel(device: &Path, pmu: Option<&mut IntelPmu>) -> GpuInfo {
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
    // NaN sentinel = "utilization unavailable" (no PMU access). The UI
    // renders this as "N/A" instead of a misleading 0 %.
    let util_pct = match pmu {
        Some(p) => p.sample().unwrap_or(f32::NAN),
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

// --- Intel i915 / xe PMU ---------------------------------------------------
//
// sysfs exposes Intel GPU frequency but not utilization. The i915 (and newer
// `xe`) drivers publish a perf PMU at /sys/bus/event_source/devices/{i915,xe}
// with engine-busy and rc6-residency counters. We read `rc6-residency-gt0`
// (nanoseconds the GT spent in RC6 sleep) and derive busy% as
// `1 - delta_rc6 / delta_time_enabled`. Single fd, single read per frame.
//
// Requires either CAP_PERFMON or kernel.perf_event_paranoid <= 2; we degrade
// silently to 0 % if the syscall is refused.

#[repr(C)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    bp_addr: u64,
}

const PERF_FORMAT_TOTAL_TIME_ENABLED: u64 = 1;
const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 8;

struct IntelPmu {
    fd: OwnedFd,
    last_rc6_ns: u64,
    last_time_ns: u64,
    primed: bool,
}

impl IntelPmu {
    fn open() -> Option<Self> {
        let (pmu_type, config, cpu) = pmu_lookup("i915").or_else(|| pmu_lookup("xe"))?;
        let attr = PerfEventAttr {
            type_: pmu_type,
            size: std::mem::size_of::<PerfEventAttr>() as u32,
            config,
            sample_period: 0,
            sample_type: 0,
            read_format: PERF_FORMAT_TOTAL_TIME_ENABLED,
            flags: 0,
            wakeup_events: 0,
            bp_type: 0,
            bp_addr: 0,
        };
        // pid=-1 + cpu=<pmu cpumask first> → system-wide counter on the PMU's
        // home CPU, which is what the kernel demands for device PMUs.
        let raw = unsafe {
            libc::syscall(
                libc::SYS_perf_event_open,
                &attr as *const PerfEventAttr,
                -1i32,
                cpu as c_int,
                -1i32,
                PERF_FLAG_FD_CLOEXEC,
            )
        };
        if raw < 0 {
            // Most common cause is kernel.perf_event_paranoid > 2 without
            // CAP_PERFMON. Print once at startup so users running unprivileged
            // know what knob to turn.
            let errno = std::io::Error::last_os_error();
            eprintln!(
                "rproc: Intel GPU utilization disabled (perf_event_open: {errno}). \
                 Fix: `sudo setcap cap_perfmon=ep <binary>` or `sudo sysctl kernel.perf_event_paranoid=2`."
            );
            return None;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw as c_int) };
        Some(Self {
            fd,
            last_rc6_ns: 0,
            last_time_ns: 0,
            primed: false,
        })
    }

    fn sample(&mut self) -> Option<f32> {
        let mut buf = [0u64; 2]; // value, time_enabled
        let n = unsafe {
            libc::read(
                self.fd.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                std::mem::size_of_val(&buf),
            )
        };
        if n != std::mem::size_of_val(&buf) as isize {
            return None;
        }
        let rc6_ns = buf[0];
        let time_ns = buf[1];
        if !self.primed {
            self.last_rc6_ns = rc6_ns;
            self.last_time_ns = time_ns;
            self.primed = true;
            return Some(0.0);
        }
        let d_rc6 = rc6_ns.saturating_sub(self.last_rc6_ns);
        let d_time = time_ns.saturating_sub(self.last_time_ns);
        self.last_rc6_ns = rc6_ns;
        self.last_time_ns = time_ns;
        if d_time == 0 {
            return Some(0.0);
        }
        let idle = (d_rc6 as f64 / d_time as f64).min(1.0);
        Some(((1.0 - idle) * 100.0) as f32)
    }
}

/// Returns (pmu_type, event_config, home_cpu) for the requested driver +
/// rc6-residency event, or None if the PMU / event isn't present.
fn pmu_lookup(driver: &str) -> Option<(u32, u64, u32)> {
    let base = format!("/sys/bus/event_source/devices/{driver}");
    let pmu_type: u32 = fs::read_to_string(format!("{base}/type"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let raw = fs::read_to_string(format!("{base}/events/rc6-residency-gt0")).ok()?;
    let config = parse_pmu_event_config(&raw)?;
    let cpu = fs::read_to_string(format!("{base}/cpumask"))
        .ok()
        .and_then(|s| parse_first_cpu(&s))
        .unwrap_or(0);
    Some((pmu_type, config, cpu))
}

/// Parses a sysfs PMU event spec like "config=0x100003" or
/// "event=0x12,umask=0x01" → returns the `config` value as u64.
fn parse_pmu_event_config(s: &str) -> Option<u64> {
    for pair in s.trim().split(',') {
        let (k, v) = pair.split_once('=')?;
        if k.trim() == "config" {
            let v = v.trim();
            let v = v.strip_prefix("0x").unwrap_or(v);
            return u64::from_str_radix(v, 16).ok();
        }
    }
    None
}

/// Parses a Linux cpumask like "0", "0-3", or "0,4" and returns the first CPU.
fn parse_first_cpu(s: &str) -> Option<u32> {
    s.trim().split(&['-', ','][..]).next()?.trim().parse().ok()
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;
