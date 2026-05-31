use std::collections::VecDeque;

fn iface_kind(name: &str) -> &'static str {
    if name.starts_with("wl") {
        "Wi-Fi"
    } else if name.starts_with("ww") || name.starts_with("wwan") {
        "Mobile broadband"
    } else if name.starts_with("usb") || name.starts_with("rndis") {
        "USB tethering"
    } else if name.starts_with("en") || name.starts_with("eth") {
        "Ethernet"
    } else {
        "Network"
    }
}

pub(super) fn iface_label(nets: &[crate::monitor::system::NetInfo], idx: usize) -> String {
    let Some(n) = nets.get(idx) else {
        return "Network".to_string();
    };
    let kind = iface_kind(&n.name);
    let same_kind: Vec<usize> = nets
        .iter()
        .enumerate()
        .filter(|(_, m)| iface_kind(&m.name) == kind)
        .map(|(i, _)| i)
        .collect();
    if same_kind.len() <= 1 {
        kind.to_string()
    } else {
        let rank = same_kind.iter().position(|&i| i == idx).unwrap_or(0) + 1;
        format!("{kind} {rank}")
    }
}

pub(super) fn short_disk_name(n: &str) -> String {
    n.strip_prefix("/dev/").unwrap_or(n).to_string()
}

pub(super) fn combined_disk(a: &VecDeque<f64>, b: &VecDeque<f64>) -> VecDeque<f64> {
    let len = a.len().max(b.len());
    let mut out = VecDeque::with_capacity(len);
    for i in 0..len {
        let av = a.get(i).copied().unwrap_or(0.0);
        let bv = b.get(i).copied().unwrap_or(0.0);
        out.push_back(av + bv);
    }
    out
}

/// Format a hwmon temperature for display, or `None` when no sensor reading
/// is available (`0.0` is the "unavailable" sentinel used across the monitor).
pub(super) fn temp_label(temp_c: f32) -> Option<String> {
    (temp_c.is_finite() && temp_c > 0.0).then(|| format!("({temp_c:.0} °C)"))
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
