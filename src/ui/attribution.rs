//! Rendering for the optional per-process graph attribution.
//!
//! A Performance panel hands us the history of per-sample top-N shares plus the
//! plot-x the pointer is currently hovering (returned by the plot widget). We
//! map that x to the matching sample and list the heaviest processes for it.
//!
//! Kept separate from `widgets` so the plotting code stays generic and unaware
//! of the feature: it only reports *where* the cursor is; we decide what to show.

use std::collections::VecDeque;

use crate::monitor::attribution::Attribution;
use crate::theme;
use crate::ui::widgets;

/// Which resource's shares a panel wants listed.
#[derive(Copy, Clone)]
pub enum Kind {
    Cpu,
    Ram,
    Disk,
}

/// Draw the attribution panel beneath a graph. `snapped_x` is the plot-x the
/// pointer is over (as returned by `widgets::big_plot*`), or `None` when the
/// pointer is away from the plot. `history` aligns newest-sample-on-the-right
/// just like the plotted series, so we map the hovered x through the same
/// helper the plot uses.
pub fn show(
    ui: &mut egui::Ui,
    history: &VecDeque<Attribution>,
    kind: Kind,
    snapped_x: Option<f64>,
) {
    let shares = snapped_x
        .and_then(|x| widgets::sample_for_plot_x(x, history.len()))
        .and_then(|idx| history.get(idx))
        .map(|a| match kind {
            Kind::Cpu => &a.cpu,
            Kind::Ram => &a.ram,
            Kind::Disk => &a.disk,
        });

    ui.add_space(6.0);
    egui::Frame::new()
        .fill(theme::PANEL_BG)
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            match shares {
                // Pointer not over a sample we have attribution for (away from
                // the plot, or in the pre-feature region of the history).
                None => {
                    ui.label(
                        egui::RichText::new(
                            "Hover the graph to see which processes were busiest at that moment.",
                        )
                        .color(theme::TEXT_DIM)
                        .small(),
                    );
                }
                Some(list) if list.is_empty() => {
                    ui.label(
                        egui::RichText::new("No measurable process activity at this point.")
                            .color(theme::TEXT_DIM)
                            .small(),
                    );
                }
                Some(list) => {
                    ui.label(
                        egui::RichText::new("Top processes at cursor")
                            .strong()
                            .small(),
                    );
                    ui.add_space(4.0);
                    for s in list {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{}  ", s.name))
                                    .color(theme::TEXT),
                            );
                            ui.label(
                                egui::RichText::new(format!("({})", s.pid))
                                    .color(theme::TEXT_DIM)
                                    .small(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format_value(kind, s.value))
                                            .color(theme::ACCENT)
                                            .strong(),
                                    );
                                },
                            );
                        });
                    }
                }
            }
        });
}

fn format_value(kind: Kind, v: f32) -> String {
    match kind {
        Kind::Cpu | Kind::Ram => {
            if v < 10.0 {
                format!("{v:.1}%")
            } else {
                format!("{v:.0}%")
            }
        }
        Kind::Disk => widgets::format_bps(v as f64),
    }
}
