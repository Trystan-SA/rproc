use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

use super::{StartupEntry, StartupSource};

/// Parse `systemd-analyze [--user] blame` and return one entry per service.
pub(super) fn collect(user: bool, protected: &HashSet<String>) -> Vec<StartupEntry> {
    let mut cmd = Command::new("systemd-analyze");
    if user {
        cmd.arg("--user");
    }
    cmd.args(["blame", "--no-pager"]);
    let Ok(out) = cmd.output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let source = if user {
        StartupSource::SystemdUser
    } else {
        StartupSource::SystemdSystem
    };
    let mut entries = Vec::new();
    let mut units: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Format: "<duration> <unit>"
        let Some((dur_str, unit)) = line.rsplit_once(' ') else {
            continue;
        };
        let unit = unit.trim().to_string();
        // Only list services — targets/sockets/mounts are infrastructure that
        // users shouldn't toggle from a "startup apps" screen.
        if !unit.ends_with(".service") {
            continue;
        }
        let Some(ms) = parse_duration_to_ms(dur_str.trim()) else {
            continue;
        };
        let is_protected = protected.contains(&unit);
        units.push(unit.clone());
        entries.push(StartupEntry {
            source: source.clone(),
            path: PathBuf::from(&unit),
            name: unit.trim_end_matches(".service").to_string(),
            exec: unit.clone(),
            comment: String::new(),
            icon: String::new(),
            enabled: true,
            boot_time_ms: Some(ms),
            critical: is_protected,
        });
    }
    let descriptions = fetch_descriptions(user, &units);
    for e in &mut entries {
        if let Some(d) = descriptions.get(&e.exec) {
            e.comment = d.clone();
        }
    }
    entries
}

/// Batch-fetch unit descriptions via `systemctl show --property=Id,Description`.
/// Returns a map keyed by the unit name (`Id`).
fn fetch_descriptions(user: bool, units: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if units.is_empty() {
        return map;
    }
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    cmd.args(["show", "--property=Id", "--property=Description", "--"]);
    cmd.args(units);
    let Ok(out) = cmd.output() else { return map };
    if !out.status.success() {
        return map;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // `systemctl show` separates units with a blank line; properties are KEY=VALUE.
    let mut id = String::new();
    let mut desc = String::new();
    for line in text.lines() {
        if line.is_empty() {
            if !id.is_empty() {
                map.insert(std::mem::take(&mut id), std::mem::take(&mut desc));
            } else {
                desc.clear();
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("Id=") {
            id = v.to_string();
        } else if let Some(v) = line.strip_prefix("Description=") {
            desc = v.to_string();
        }
    }
    if !id.is_empty() {
        map.insert(id, desc);
    }
    map
}

/// Set of service units that the user cannot meaningfully disable: those whose
/// unit-file state is `static`, `generated`, or `alias`. These are pulled in by
/// dependency, not by enable symlinks, so `systemctl disable` is a no-op or
/// refused. This is the right proxy for "do not disable" — unlike
/// `systemd-analyze critical-chain`, which reports the slowest path to the
/// default target (a performance view, not a safety view).
pub(super) fn protected_units(user: bool) -> HashSet<String> {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    cmd.args([
        "list-unit-files",
        "--type=service",
        "--no-legend",
        "--no-pager",
        "--plain",
    ]);
    let Ok(out) = cmd.output() else {
        return HashSet::new();
    };
    if !out.status.success() {
        return HashSet::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut set = HashSet::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(unit) = parts.next() else { continue };
        let Some(state) = parts.next() else { continue };
        if matches!(state, "static" | "generated" | "alias") {
            set.insert(unit.to_string());
        }
    }
    set
}

/// Parse systemd-analyze duration strings: "12ms", "1.234s", "1min 2.345s", "2h 3min 4s".
fn parse_duration_to_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total: f64 = 0.0;
    let mut matched_any = false;
    let mut rest = s;
    while !rest.is_empty() {
        rest = rest.trim_start();
        let num_end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if num_end == 0 {
            return None;
        }
        let num: f64 = rest[..num_end].parse().ok()?;
        let after = &rest[num_end..];
        let unit_end = after
            .find(|c: char| c.is_ascii_digit() || c.is_whitespace())
            .unwrap_or(after.len());
        let unit = &after[..unit_end];
        let factor_ms = match unit {
            "ms" => 1.0,
            "s" => 1_000.0,
            "min" => 60_000.0,
            "h" => 3_600_000.0,
            "d" => 86_400_000.0,
            _ => {
                return if matched_any {
                    Some(total as u64)
                } else {
                    None
                };
            }
        };
        total += num * factor_ms;
        matched_any = true;
        rest = &after[unit_end..];
    }
    Some(total as u64)
}

pub(super) fn set_enabled(entry: &StartupEntry, enabled: bool) -> Result<(), String> {
    let mut cmd = Command::new("systemctl");
    if entry.source == StartupSource::SystemdUser {
        cmd.arg("--user");
    }
    cmd.args([if enabled { "enable" } else { "disable" }, &entry.exec]);
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
#[path = "systemd_tests.rs"]
mod tests;
