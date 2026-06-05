//! Battery state read from `/sys/class/power_supply`.
//!
//! Depending on the driver a battery reports either an `energy_*` set (µWh)
//! or a `charge_*` set (µAh); both are normalized to Wh here so the UI only
//! ever sees one unit.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Charging,
    Discharging,
    Full,
    NotCharging,
    #[default]
    Unknown,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Charging => "Charging",
            Status::Discharging => "Discharging",
            Status::Full => "Full",
            Status::NotCharging => "Not charging",
            Status::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Default)]
pub struct BatteryInfo {
    pub capacity_pct: f32,
    pub status: Status,
    /// Current draw/charge rate in W. `0.0` means the driver reports none.
    pub power_w: f32,
    pub energy_now_wh: f32,
    pub energy_full_wh: f32,
    pub energy_full_design_wh: f32,
    pub cycle_count: u32,
    pub technology: String,
    pub model: String,
    pub ac_online: bool,
}

impl BatteryInfo {
    /// Seconds until empty (discharging) or full (charging), extrapolated from
    /// the current rate. `None` when the rate is too small to be meaningful.
    pub fn time_left_secs(&self) -> Option<u64> {
        if self.power_w < 0.5 {
            return None;
        }
        let wh = match self.status {
            Status::Discharging => self.energy_now_wh,
            Status::Charging => (self.energy_full_wh - self.energy_now_wh).max(0.0),
            _ => return None,
        };
        Some((wh / self.power_w * 3600.0) as u64)
    }

    /// Full-charge capacity as a share of the design capacity.
    pub fn health_pct(&self) -> Option<f32> {
        (self.energy_full_design_wh > 0.0 && self.energy_full_wh > 0.0)
            .then(|| (self.energy_full_wh / self.energy_full_design_wh) * 100.0)
    }
}

/// Raw sysfs values for one battery, before unit normalization. Split from the
/// filesystem read so [`build`] stays a pure, testable transform.
#[derive(Default)]
struct Raw {
    status: Option<String>,
    capacity: Option<String>,
    energy_now: Option<String>,
    energy_full: Option<String>,
    energy_full_design: Option<String>,
    charge_now: Option<String>,
    charge_full: Option<String>,
    charge_full_design: Option<String>,
    power_now: Option<String>,
    current_now: Option<String>,
    voltage_now: Option<String>,
    voltage_min_design: Option<String>,
    cycle_count: Option<String>,
    technology: Option<String>,
    model_name: Option<String>,
}

impl Raw {
    fn load(dir: &Path) -> Self {
        Self {
            status: read(dir, "status"),
            capacity: read(dir, "capacity"),
            energy_now: read(dir, "energy_now"),
            energy_full: read(dir, "energy_full"),
            energy_full_design: read(dir, "energy_full_design"),
            charge_now: read(dir, "charge_now"),
            charge_full: read(dir, "charge_full"),
            charge_full_design: read(dir, "charge_full_design"),
            power_now: read(dir, "power_now"),
            current_now: read(dir, "current_now"),
            voltage_now: read(dir, "voltage_now"),
            voltage_min_design: read(dir, "voltage_min_design"),
            cycle_count: read(dir, "cycle_count"),
            technology: read(dir, "technology"),
            model_name: read(dir, "model_name"),
        }
    }
}

fn read(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Parse a sysfs micro-unit value (µWh, µW, µA, µV) into its base unit.
fn micro(v: &Option<String>) -> Option<f64> {
    v.as_deref()?.parse::<f64>().ok().map(|x| x / 1e6)
}

fn build(raw: &Raw, ac_online: bool) -> BatteryInfo {
    let volt_design = micro(&raw.voltage_min_design);
    let wh = |energy: &Option<String>, charge: &Option<String>| -> f32 {
        micro(energy)
            .or_else(|| Some(micro(charge)? * volt_design?))
            .unwrap_or(0.0) as f32
    };
    let energy_now_wh = wh(&raw.energy_now, &raw.charge_now);
    let energy_full_wh = wh(&raw.energy_full, &raw.charge_full);
    let power_w = micro(&raw.power_now)
        .or_else(|| Some(micro(&raw.current_now)? * micro(&raw.voltage_now)?))
        .unwrap_or(0.0)
        .abs() as f32;
    let capacity_pct = raw
        .capacity
        .as_deref()
        .and_then(|s| s.parse::<f32>().ok())
        .or_else(|| (energy_full_wh > 0.0).then(|| energy_now_wh / energy_full_wh * 100.0))
        .unwrap_or(0.0)
        .clamp(0.0, 100.0);
    let status = match raw.status.as_deref() {
        Some("Charging") => Status::Charging,
        Some("Discharging") => Status::Discharging,
        Some("Full") => Status::Full,
        Some("Not charging") => Status::NotCharging,
        _ => Status::Unknown,
    };
    BatteryInfo {
        capacity_pct,
        status,
        power_w,
        energy_now_wh,
        energy_full_wh,
        energy_full_design_wh: wh(&raw.energy_full_design, &raw.charge_full_design),
        cycle_count: raw
            .cycle_count
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        technology: raw.technology.clone().unwrap_or_default(),
        model: raw.model_name.clone().unwrap_or_default(),
        ac_online,
    }
}

/// First present battery under `/sys/class/power_supply`, or `None` on
/// battery-less machines. AC state is folded in from any online Mains supply.
pub fn collect() -> Option<BatteryInfo> {
    collect_from(Path::new("/sys/class/power_supply"))
}

fn collect_from(root: &Path) -> Option<BatteryInfo> {
    let mut battery_dir: Option<PathBuf> = None;
    let mut ac_online = false;
    for entry in fs::read_dir(root).ok()?.flatten() {
        let dir = entry.path();
        match read(&dir, "type").as_deref() {
            // Multi-battery laptops: keep the lowest-named one (BAT0) so the
            // choice is stable across the arbitrary read_dir order.
            Some("Battery")
                if read(&dir, "present").as_deref() != Some("0")
                    && battery_dir.as_ref().is_none_or(|cur| dir < *cur) =>
            {
                battery_dir = Some(dir);
            }
            Some("Mains") => {
                ac_online |= read(&dir, "online").as_deref() == Some("1");
            }
            _ => {}
        }
    }
    Some(build(&Raw::load(&battery_dir?), ac_online))
}

#[cfg(test)]
#[path = "battery_tests.rs"]
mod tests;
