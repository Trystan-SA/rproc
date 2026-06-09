//! Wireless link quality, read from `/proc/net/wireless`.
//!
//! The kernel exposes a per-interface `link` quality and a `level` signal
//! reading (dBm). The `link` scale is driver-dependent (its maximum varies),
//! so for a comparable 0..100 series we derive `quality_pct` from the dBm
//! level the way NetworkManager does: -50 dBm and above is full, -100 dBm and
//! below is none, linear between. The raw values are kept for the stat lines.

use std::collections::HashMap;
use std::fs;

#[derive(Clone, Copy, Default)]
pub struct WifiSignal {
    pub link_quality: f32,
    pub signal_dbm: f32,
    pub quality_pct: f32,
}

const WIRELESS_PATH: &str = "/proc/net/wireless";

/// Per-interface wireless signal, keyed by interface name (e.g. `wlp130s0`).
/// Empty on machines without any wireless interface or where the file is
/// unreadable — wireless monitoring then simply doesn't appear.
pub fn collect() -> HashMap<String, WifiSignal> {
    fs::read_to_string(WIRELESS_PATH)
        .map(|s| parse_wireless(&s))
        .unwrap_or_default()
}

fn dbm_to_pct(dbm: f32) -> f32 {
    ((dbm + 100.0) * 2.0).clamp(0.0, 100.0)
}

fn parse_wireless(contents: &str) -> HashMap<String, WifiSignal> {
    let mut out = HashMap::new();
    // The first two lines are column headers; data rows start with `iface:`.
    for line in contents.lines().filter(|l| l.contains(':')) {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = iface.trim();
        if iface.is_empty() {
            continue;
        }
        // status, link, level, noise, ... — values may carry a trailing '.'.
        let mut fields = rest.split_whitespace();
        let _status = fields.next();
        let link = fields.next().and_then(parse_field);
        let level = fields.next().and_then(parse_field);
        let (Some(link_quality), Some(signal_dbm)) = (link, level) else {
            continue;
        };
        out.insert(
            iface.to_string(),
            WifiSignal {
                link_quality,
                signal_dbm,
                quality_pct: dbm_to_pct(signal_dbm),
            },
        );
    }
    out
}

fn parse_field(tok: &str) -> Option<f32> {
    tok.trim_end_matches('.').parse().ok()
}

#[cfg(test)]
#[path = "wifi_tests.rs"]
mod tests;
