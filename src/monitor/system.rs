use std::collections::HashMap;

use sysinfo::{Components, Disks, Networks, System};

#[derive(Clone, Default)]
pub struct SystemSummary {
    pub host_name: String,
    pub os_name: String,
    pub kernel: String,
    pub uptime_secs: u64,
    pub cpu_total: f32,
    pub per_core: Vec<f32>,
    pub cpu_brand: String,
    pub cpu_freq_mhz: u64,
    /// CPU package temperature in °C. `0.0` means no sensor was found.
    pub cpu_temp_c: f32,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub ram_total: u64,
    pub ram_used: u64,
    pub ram_used_pct: f32,
    pub swap_total: u64,
    pub swap_used: u64,
    pub net_rx_bps: f64,
    pub net_tx_bps: f64,
    pub disks: Vec<DiskInfo>,
    pub nets: Vec<NetInfo>,
}

/// Aggregated counters for one physical disk, summed across its partitions.
#[derive(Clone, Default)]
pub struct DiskInfo {
    pub name: String,
    pub mounts: Vec<String>,
    pub fs: String,
    pub partitions: usize,
    pub total: u64,
    pub used: u64,
    pub read_bps: f64,
    pub write_bps: f64,
    /// Drive temperature in °C, read from the device's hwmon sensor.
    /// `0.0` means no sensor was found (or the kernel `drivetemp` module
    /// isn't loaded for SATA drives).
    pub temp_c: f32,
}

/// Map a partition device path to its parent physical disk.
/// `/dev/nvme0n1p2` → `/dev/nvme0n1`, `/dev/sda1` → `/dev/sda`,
/// `/dev/mmcblk0p1` → `/dev/mmcblk0`. Leaves unknown layouts (dm-*, zd*, …) untouched.
fn physical_disk_name(name: &str) -> String {
    let has_dev = name.starts_with("/dev/");
    let stripped = if has_dev { &name[5..] } else { name };

    let base: &str = if stripped.starts_with("nvme") || stripped.starts_with("mmcblk") {
        match stripped.rfind('p') {
            Some(p)
                if p > 0
                    && !stripped[p + 1..].is_empty()
                    && stripped[p + 1..].chars().all(|c| c.is_ascii_digit())
                    && stripped[..p]
                        .chars()
                        .last()
                        .is_some_and(|c| c.is_ascii_digit()) =>
            {
                &stripped[..p]
            }
            _ => stripped,
        }
    } else if stripped.starts_with("sd")
        || stripped.starts_with("vd")
        || stripped.starts_with("hd")
        || stripped.starts_with("xvd")
    {
        let trimmed = stripped.trim_end_matches(|c: char| c.is_ascii_digit());
        if trimmed.is_empty() {
            stripped
        } else {
            trimmed
        }
    } else {
        stripped
    };

    if has_dev {
        format!("/dev/{base}")
    } else {
        base.to_string()
    }
}

#[derive(Clone, Default)]
pub struct NetInfo {
    pub name: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub mac: String,
}

/// Keep only interfaces that represent real outbound connectivity:
/// Ethernet (`en*`, `eth*`), WiFi (`wl*`), mobile broadband / tethering
/// (`ww*`, `wwan*`, `usb*`, `rndis*`). Drops loopback, Docker bridges
/// and veths, libvirt/VBox/VMware bridges, VPN/tun-tap, and WireGuard.
fn is_relevant_iface(name: &str) -> bool {
    const PREFIXES: &[&str] = &["en", "eth", "wl", "ww", "wwan", "usb", "rndis"];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// CPU package temperature in °C, or `0.0` if no suitable sensor is found.
///
/// sysinfo labels each hwmon channel; depending on the driver the chip name
/// may or may not be prefixed, so we match on the channel keyword rather than
/// the chip. Preference order: whole-package reading (`Package id 0` on Intel,
/// `Tctl` / `Tdie` on AMD) → hottest individual core → a generic CPU thermal
/// zone (`cpu_thermal` / `acpitz` on ARM). NVMe channels (`Composite`,
/// `Sensor N`) are skipped so a hot drive never masquerades as the CPU.
fn cpu_temperature(components: &Components) -> f32 {
    let mut package: Option<f32> = None;
    let mut core: Option<f32> = None;
    let mut generic: Option<f32> = None;
    for c in components {
        let Some(t) = c.temperature() else { continue };
        if !t.is_finite() || t <= 0.0 {
            continue;
        }
        let l = c.label().to_ascii_lowercase();
        if l.contains("composite") || l.contains("sensor") || l.contains("nvme") {
            continue;
        }
        if l.contains("package") || l.contains("tctl") || l.contains("tdie") {
            package = Some(package.map_or(t, |p| p.max(t)));
        } else if l.starts_with("core ") || l.contains(" core ") {
            core = Some(core.map_or(t, |h| h.max(t)));
        } else if l.starts_with("coretemp")
            || l.starts_with("k10temp")
            || l.starts_with("zenpower")
            || l.starts_with("cpu")
            || l.starts_with("acpitz")
        {
            generic = Some(generic.map_or(t, |g| g.max(t)));
        }
    }
    package.or(core).or(generic).unwrap_or(0.0)
}

/// Drive temperature in °C for a physical disk like `/dev/nvme0n1`, read from
/// its hwmon sensor under sysfs. Returns `0.0` when unavailable.
///
/// The block device exposes its controller's hwmon at
/// `/sys/block/<dev>/device/hwmon*/temp1_input` (NVMe `Composite`, or the
/// SATA `drivetemp` reading when that kernel module is loaded).
fn disk_temperature(name: &str) -> f32 {
    let dev = name.strip_prefix("/dev/").unwrap_or(name);
    if dev.is_empty() || dev.contains('/') {
        return 0.0;
    }
    let Ok(entries) = std::fs::read_dir(format!("/sys/block/{dev}/device")) else {
        return 0.0;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let is_hwmon = p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("hwmon"));
        if !is_hwmon {
            continue;
        }
        if let Some(t) = std::fs::read_to_string(p.join("temp1_input"))
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            && t > 0.0
        {
            return t / 1000.0;
        }
    }
    0.0
}

/// Current CPU frequency in MHz, read from sysfs.
fn current_cpu_freq() -> u64 {
    if let Ok(freq) =
        std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        && let Ok(khz) = freq.trim().parse::<u64>()
    {
        return khz / 1000;
    }
    0
}

impl SystemSummary {
    /// Build a snapshot of the current system state.
    ///
    /// `interval_secs` is the time elapsed since the previous sysinfo refresh.
    /// We use it to normalize byte counters (network RX/TX, disk read/write)
    /// into bytes/second — sysinfo only exposes "bytes since last refresh",
    /// so without this divisor a 10 s sampling interval would show 10× the
    /// actual throughput.
    pub fn collect(
        sys: &System,
        nets: &Networks,
        disks: &Disks,
        components: &Components,
        interval_secs: f64,
    ) -> Self {
        let interval_secs = interval_secs.max(0.001);
        let global_cpu = sys.global_cpu_usage();
        let per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let cpu_brand = sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default();
        let cpu_freq_mhz = current_cpu_freq();
        let logical_cores = sys.cpus().len();
        let physical_cores = sys.physical_core_count().unwrap_or(logical_cores);
        let cpu_temp_c = cpu_temperature(components);

        let ram_total = sys.total_memory();
        let ram_used = sys.used_memory();
        let ram_pct = if ram_total > 0 {
            (ram_used as f32 / ram_total as f32) * 100.0
        } else {
            0.0
        };

        let mut net_rx = 0.0;
        let mut net_tx = 0.0;
        let mut net_list = Vec::new();
        for (name, data) in nets.iter() {
            if !is_relevant_iface(name) {
                continue;
            }
            let rx = data.received() as f64 / interval_secs;
            let tx = data.transmitted() as f64 / interval_secs;
            net_rx += rx;
            net_tx += tx;
            net_list.push(NetInfo {
                name: name.clone(),
                rx_bps: rx,
                tx_bps: tx,
                rx_total: data.total_received(),
                tx_total: data.total_transmitted(),
                mac: data.mac_address().to_string(),
            });
        }

        // Aggregate partitions into their parent physical disk so each card
        // and graph in the UI represents one physical device, not a slice of it.
        let mut by_disk: HashMap<String, DiskInfo> = HashMap::new();
        let mut fs_by_disk: HashMap<String, Vec<String>> = HashMap::new();
        for disk in disks.list() {
            let fs = disk.file_system().to_string_lossy().into_owned();
            // Skip pseudo / overlay filesystems — they're docker/snap noise.
            if matches!(
                fs.as_str(),
                "overlay" | "squashfs" | "tmpfs" | "devtmpfs" | "fuse" | "fuse.snapfuse"
            ) {
                continue;
            }
            let raw = disk.name().to_string_lossy().into_owned();
            let phys = physical_disk_name(&raw);
            let usage = disk.usage();
            let entry = by_disk.entry(phys.clone()).or_insert_with(|| DiskInfo {
                name: phys.clone(),
                ..Default::default()
            });
            entry.partitions += 1;
            entry
                .mounts
                .push(disk.mount_point().to_string_lossy().into_owned());
            entry.total += disk.total_space();
            entry.used += disk.total_space().saturating_sub(disk.available_space());
            entry.read_bps += usage.read_bytes as f64 / interval_secs;
            entry.write_bps += usage.written_bytes as f64 / interval_secs;
            let fs_list = fs_by_disk.entry(phys).or_default();
            if !fs_list.contains(&fs) {
                fs_list.push(fs);
            }
        }

        let mut disk_list: Vec<DiskInfo> = by_disk
            .into_iter()
            .map(|(name, mut d)| {
                if let Some(fsl) = fs_by_disk.get(&name) {
                    d.fs = fsl.join(", ");
                }
                d.temp_c = disk_temperature(&name);
                d
            })
            .collect();
        disk_list.sort_by(|a, b| a.name.cmp(&b.name));

        Self {
            host_name: System::host_name().unwrap_or_default(),
            os_name: System::long_os_version().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_default(),
            uptime_secs: System::uptime(),
            cpu_total: global_cpu,
            per_core,
            cpu_brand,
            cpu_freq_mhz,
            cpu_temp_c,
            physical_cores,
            logical_cores,
            ram_total,
            ram_used,
            ram_used_pct: ram_pct,
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            net_rx_bps: net_rx,
            net_tx_bps: net_tx,
            disks: disk_list,
            nets: net_list,
        }
    }
}
