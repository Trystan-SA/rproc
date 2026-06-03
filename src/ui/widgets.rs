//! Pure formatting and plot-mapping helpers shared by the model builders.
//! No rendering lives here anymore — drawing is declared in the `.slint` files
//! and fed from the glue in this module's siblings.

/// Number of samples in the rolling history window (matches sampler config).
pub const HISTORY_LEN: usize = 60;

pub fn format_bytes(b: u64) -> String {
    let v = b as f64;
    if v >= 1_099_511_627_776.0 {
        format!("{:.1} TB", v / 1_099_511_627_776.0)
    } else if v >= 1_073_741_824.0 {
        format!("{:.1} GB", v / 1_073_741_824.0)
    } else if v >= 1_048_576.0 {
        format!("{:.1} MB", v / 1_048_576.0)
    } else if v >= 1024.0 {
        format!("{:.0} KB", v / 1024.0)
    } else {
        format!("{b} B")
    }
}

pub fn format_bps(b: f64) -> String {
    if b >= 1_000_000_000.0 {
        format!("{:.2} GB/s", b / 1_000_000_000.0)
    } else if b >= 1_000_000.0 {
        format!("{:.1} MB/s", b / 1_000_000.0)
    } else if b >= 1_000.0 {
        format!("{:.0} KB/s", b / 1_000.0)
    } else {
        format!("{:.0} B/s", b)
    }
}

pub fn format_duration(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Map a sample index (0 = oldest in queue) to its X coordinate on the plot,
/// such that the newest sample is anchored at the right edge
/// (`x = HISTORY_LEN - 1`).
pub fn plot_x_for_sample(sample_idx: usize, data_len: usize) -> f64 {
    (HISTORY_LEN.saturating_sub(data_len) + sample_idx) as f64
}

/// Inverse of `plot_x_for_sample`: given a hovered plot X, return the
/// corresponding sample index, or `None` if the hover is in the empty zone to
/// the left of the data line.
pub fn sample_for_plot_x(plot_x: f64, data_len: usize) -> Option<usize> {
    if data_len == 0 {
        return None;
    }
    let offset = HISTORY_LEN as i64 - data_len as i64;
    let idx = plot_x.round() as i64 - offset;
    if idx < 0 || (idx as usize) >= data_len {
        None
    } else {
        Some(idx as usize)
    }
}

pub fn format_pct_value(v: f64) -> String {
    if v < 10.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}

/// Convert a `samples_ago` offset + sample interval into a human-readable label.
pub fn format_time_ago(samples_ago: i64, sample_interval_ms: u64) -> String {
    if samples_ago <= 0 {
        return "now".to_string();
    }
    let interval = sample_interval_ms.max(1) as i64;
    let ms = samples_ago * interval;
    if ms < 1000 {
        format!("-{ms} ms")
    } else if ms < 60_000 {
        let secs = ms as f64 / 1000.0;
        if interval < 1000 {
            format!("-{secs:.1} s")
        } else {
            format!("-{} s", ms / 1000)
        }
    } else {
        let minutes = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("-{minutes}m {secs:02}s")
    }
}

pub fn max_in<I: Iterator<Item = f64>>(it: I) -> f64 {
    it.fold(0.0_f64, f64::max)
}

#[derive(Copy, Clone)]
pub enum OpenTarget {
    /// Open the path itself — used for directories.
    Self_,
    /// Open the parent — used for files (so the file manager lands on the
    /// containing folder).
    Parent,
}

pub fn open_path(path: &str, target: OpenTarget) {
    let p = std::path::Path::new(path);
    let dest = match target {
        OpenTarget::Self_ => p,
        OpenTarget::Parent => p.parent().unwrap_or(p),
    };
    let _ = std::process::Command::new("xdg-open").arg(dest).spawn();
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
