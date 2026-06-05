//! Selection + value formatting for the per-process graph attribution. The
//! rendering now lives in `ui/performance.slint`; this maps a hovered plot-x
//! onto the matching history sample and formats each share for display.

use std::collections::VecDeque;

use crate::monitor::attribution::{Attribution, ProcShare};
use crate::ui::widgets;

#[derive(Copy, Clone)]
pub enum Kind {
    Cpu,
    Ram,
    Disk,
    Gpu,
    Battery,
}

/// The shares for `kind` at the sample the pointer is hovering, or `None` when
/// the pointer is off the plot / in the pre-feature region of the history.
pub fn shares_at(
    history: &VecDeque<Attribution>,
    kind: Kind,
    snapped_x: Option<f64>,
) -> Option<&Vec<ProcShare>> {
    snapped_x
        .and_then(|x| widgets::sample_for_plot_x(x, history.len()))
        .and_then(|idx| history.get(idx))
        .map(|a| match kind {
            Kind::Cpu => &a.cpu,
            Kind::Ram => &a.ram,
            Kind::Disk => &a.disk,
            Kind::Gpu => &a.gpu,
            Kind::Battery => &a.battery,
        })
}

pub fn format_value(kind: Kind, s: &ProcShare) -> String {
    match kind {
        Kind::Cpu | Kind::Gpu => fmt_pct(s.value),
        Kind::Ram => format!("{} ({})", widgets::format_bytes(s.bytes), fmt_pct(s.value)),
        Kind::Disk => widgets::format_bps(s.value as f64),
        // "~" flags the value as an estimate (CPU share × measured discharge).
        Kind::Battery => format!("~{} W", fmt_watts(s.value)),
    }
}

fn fmt_watts(v: f32) -> String {
    if v < 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.1}")
    }
}

fn fmt_pct(v: f32) -> String {
    if v < 10.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}
