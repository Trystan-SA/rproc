use std::collections::VecDeque;

use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Text, VLine};

use crate::theme;

/// Number of samples in the rolling history window (matches sampler config).
const HISTORY_LEN: usize = 60;

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
/// (`x = HISTORY_LEN - 1`). When the queue isn't yet full, the data line lives
/// on the right and grows leftward — matches Windows Task Manager / Grafana.
fn plot_x_for_sample(sample_idx: usize, data_len: usize) -> f64 {
    (HISTORY_LEN.saturating_sub(data_len) + sample_idx) as f64
}

/// Inverse of `plot_x_for_sample`: given a hovered plot X, return the
/// corresponding sample index, or `None` if the hover is in the empty zone
/// to the left of the data line.
fn sample_for_plot_x(plot_x: f64, data_len: usize) -> Option<usize> {
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

fn collect_points_f32(data: &VecDeque<f32>) -> PlotPoints<'static> {
    let n = data.len();
    data.iter()
        .enumerate()
        .map(|(i, v)| [plot_x_for_sample(i, n), *v as f64])
        .collect()
}

fn collect_points_f64(data: &VecDeque<f64>) -> PlotPoints<'static> {
    let n = data.len();
    data.iter()
        .enumerate()
        .map(|(i, v)| [plot_x_for_sample(i, n), *v])
        .collect()
}

/// Compact sparkline showing the recent ~60s of a 0..max metric.
pub fn sparkline(
    ui: &mut egui::Ui,
    id: &str,
    data: &VecDeque<f32>,
    max_value: f32,
    color: egui::Color32,
    height: f32,
) {
    Plot::new(id)
        .height(height)
        .show_axes(false)
        .show_grid(false)
        .show_x(false)
        .show_y(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_y(0.0)
        .include_y(max_value as f64)
        .include_x(0.0)
        .include_x((HISTORY_LEN - 1) as f64)
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new("", collect_points_f32(data))
                    .color(color)
                    .width(1.5)
                    .fill(0.0)
                    .fill_alpha(0.18),
            );
        });
}

pub fn sparkline_f64(
    ui: &mut egui::Ui,
    id: &str,
    data: &VecDeque<f64>,
    max_value: f64,
    color: egui::Color32,
    height: f32,
) {
    Plot::new(id)
        .height(height)
        .show_axes(false)
        .show_grid(false)
        .show_x(false)
        .show_y(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_y(0.0)
        .include_y(max_value.max(1.0))
        .include_x(0.0)
        .include_x((HISTORY_LEN - 1) as f64)
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new("", collect_points_f64(data))
                    .color(color)
                    .width(1.5)
                    .fill(0.0)
                    .fill_alpha(0.18),
            );
        });
}

/// Bigger plot for the detail view in Performance tab.
///
/// Renders one line per series. When the pointer hovers the plot, draws a
/// vertical crosshair plus a stacked readout of every series' value at the
/// hovered sample. The newest sample is anchored to the right edge, so the
/// crosshair always lands on real data wherever the user hovers within the
/// data range.
pub fn big_plot(
    ui: &mut egui::Ui,
    id: &str,
    series: &[(&str, &VecDeque<f32>, egui::Color32)],
    max_value: f32,
    height: f32,
    sample_interval_ms: u64,
) {
    // Lock bounds explicitly: the hover overlay reads `plot_bounds().max()[1]`
    // to place labels, and with auto-bounds + 5% margin the labels would
    // expand the bounds each frame and visibly zoom the Y axis out on hover.
    Plot::new(id)
        .height(height)
        .show_axes(false)
        .show_grid(true)
        .show_x(false)
        .show_y(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .set_margin_fraction(egui::Vec2::new(0.0, 0.05))
        .default_x_bounds(0.0, (HISTORY_LEN - 1) as f64)
        .default_y_bounds(0.0, max_value as f64)
        .show(ui, |plot_ui| {
            for (name, data, color) in series {
                plot_ui.line(
                    Line::new(*name, collect_points_f32(data))
                        .color(*color)
                        .width(2.0)
                        .fill(0.0)
                        .fill_alpha(0.22),
                );
            }
            draw_hover_overlay(plot_ui, series, format_pct_value, sample_interval_ms);
        });
}

pub fn big_plot_f64(
    ui: &mut egui::Ui,
    id: &str,
    series: &[(&str, &VecDeque<f64>, egui::Color32)],
    max_value: f64,
    height: f32,
    sample_interval_ms: u64,
) {
    Plot::new(id)
        .height(height)
        .show_axes(false)
        .show_grid(true)
        .show_x(false)
        .show_y(false)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .set_margin_fraction(egui::Vec2::new(0.0, 0.05))
        .default_x_bounds(0.0, (HISTORY_LEN - 1) as f64)
        .default_y_bounds(0.0, max_value.max(1.0))
        .show(ui, |plot_ui| {
            for (name, data, color) in series {
                plot_ui.line(
                    Line::new(*name, collect_points_f64(data))
                        .color(*color)
                        .width(2.0)
                        .fill(0.0)
                        .fill_alpha(0.22),
                );
            }
            draw_hover_overlay_f64(plot_ui, series, format_bps, sample_interval_ms);
        });
}

fn format_pct_value(v: f64) -> String {
    if v < 10.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}

/// Convert a `samples_ago` offset + sample interval into a human-readable
/// label. Picks units that match the sampling cadence: sub-second sampling
/// shows milliseconds, second-scale shows seconds, longer shows minutes.
fn format_time_ago(samples_ago: i64, sample_interval_ms: u64) -> String {
    if samples_ago <= 0 {
        return "now".to_string();
    }
    let interval = sample_interval_ms.max(1) as i64;
    let ms = samples_ago * interval;
    if ms < 1000 {
        format!("-{ms} ms")
    } else if ms < 60_000 {
        let secs = ms as f64 / 1000.0;
        // Round to tenths when the interval is sub-second, integer otherwise.
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

fn draw_hover_overlay<F>(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    series: &[(&str, &VecDeque<f32>, egui::Color32)],
    formatter: F,
    sample_interval_ms: u64,
) where
    F: Fn(f64) -> String,
{
    let Some(coord) = plot_ui.pointer_coordinate() else {
        return;
    };
    let bounds = plot_ui.plot_bounds();
    let min_x = bounds.min()[0];
    let max_x = bounds.max()[0];
    let max_y = bounds.max()[1];
    if coord.x < min_x || coord.x > max_x {
        return;
    }
    let max_len = series.iter().map(|(_, d, _)| d.len()).max().unwrap_or(0);
    let snapped_x = coord.x.round().clamp(0.0, (HISTORY_LEN - 1) as f64);
    let sample = sample_for_plot_x(snapped_x, max_len);

    plot_ui.vline(
        VLine::new("", snapped_x)
            .stroke(egui::Stroke::new(1.0, theme::TEXT_DIM))
            .color(theme::TEXT_DIM),
    );

    let samples_ago = ((HISTORY_LEN - 1) as i64) - snapped_x as i64;
    let header = format_time_ago(samples_ago, sample_interval_ms);
    let mid = (min_x + max_x) / 2.0;
    let anchor = if coord.x > mid {
        egui::Align2::RIGHT_TOP
    } else {
        egui::Align2::LEFT_TOP
    };

    plot_ui.text(
        Text::new(
            "",
            PlotPoint::new(snapped_x, max_y * 0.97),
            egui::RichText::new(header).color(theme::TEXT_DIM).strong(),
        )
        .anchor(anchor),
    );

    for (slot, (name, data, color)) in series.iter().enumerate() {
        let value_str = match sample.and_then(|idx| data.get(idx)) {
            Some(v) => formatter(*v as f64),
            None => "—".to_string(),
        };
        let label = if name.is_empty() {
            value_str
        } else {
            format!("{name}  {value_str}")
        };
        let y = max_y * (0.87 - slot as f64 * 0.10);
        plot_ui.text(
            Text::new(
                "",
                PlotPoint::new(snapped_x, y),
                egui::RichText::new(label).color(*color).strong(),
            )
            .anchor(anchor),
        );
    }
}

fn draw_hover_overlay_f64<F>(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    series: &[(&str, &VecDeque<f64>, egui::Color32)],
    formatter: F,
    sample_interval_ms: u64,
) where
    F: Fn(f64) -> String,
{
    let Some(coord) = plot_ui.pointer_coordinate() else {
        return;
    };
    let bounds = plot_ui.plot_bounds();
    let min_x = bounds.min()[0];
    let max_x = bounds.max()[0];
    let max_y = bounds.max()[1];
    if coord.x < min_x || coord.x > max_x {
        return;
    }
    let max_len = series.iter().map(|(_, d, _)| d.len()).max().unwrap_or(0);
    let snapped_x = coord.x.round().clamp(0.0, (HISTORY_LEN - 1) as f64);
    let sample = sample_for_plot_x(snapped_x, max_len);

    plot_ui.vline(
        VLine::new("", snapped_x)
            .stroke(egui::Stroke::new(1.0, theme::TEXT_DIM))
            .color(theme::TEXT_DIM),
    );

    let samples_ago = ((HISTORY_LEN - 1) as i64) - snapped_x as i64;
    let header = format_time_ago(samples_ago, sample_interval_ms);
    let mid = (min_x + max_x) / 2.0;
    let anchor = if coord.x > mid {
        egui::Align2::RIGHT_TOP
    } else {
        egui::Align2::LEFT_TOP
    };

    plot_ui.text(
        Text::new(
            "",
            PlotPoint::new(snapped_x, max_y * 0.97),
            egui::RichText::new(header).color(theme::TEXT_DIM).strong(),
        )
        .anchor(anchor),
    );

    for (slot, (name, data, color)) in series.iter().enumerate() {
        let value_str = match sample.and_then(|idx| data.get(idx)) {
            Some(v) => formatter(*v),
            None => "—".to_string(),
        };
        let label = if name.is_empty() {
            value_str
        } else {
            format!("{name}  {value_str}")
        };
        let y = max_y * (0.87 - slot as f64 * 0.10);
        plot_ui.text(
            Text::new(
                "",
                PlotPoint::new(snapped_x, y),
                egui::RichText::new(label).color(*color).strong(),
            )
            .anchor(anchor),
        );
    }
}

/// Card frame matching the W11-inspired panel look.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::CARD_BG)
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(8))
        .show(ui, add)
        .inner
}

/// Stat line: a label on the left, a strong value on the right.
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(theme::TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).strong());
        });
    });
}

pub fn max_in<I: Iterator<Item = f64>>(it: I) -> f64 {
    it.fold(0.0_f64, f64::max)
}

#[derive(Copy, Clone)]
pub enum OpenTarget {
    /// Open the path itself — used for directories.
    Self_,
    /// Open the parent — used for files (so the file manager lands on the
    /// containing folder with the file selected).
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

/// Labeled path block: dim label + Open button on the first row, wrapped
/// clickable path on the second. Clicking either opens the path (or its
/// parent, per `target`) in the file manager.
pub fn path_field(ui: &mut egui::Ui, label: &str, path: &str, target: OpenTarget) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(theme::TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new("Open").small())
                .on_hover_text("Open in file manager")
                .clicked()
            {
                open_path(path, target);
            }
        });
    });
    let resp = ui
        .add(
            egui::Label::new(egui::RichText::new(path).color(theme::TEXT))
                .wrap()
                .sense(egui::Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Open in file manager");
    if resp.clicked() {
        open_path(path, target);
    }
}

/// Single-line variant: the row IS the path. Clickable label + small Open
/// button on the right. Always opens the path itself (not its parent).
pub fn path_field_compact(ui: &mut egui::Ui, path: &str) {
    ui.horizontal(|ui| {
        let resp = ui
            .add(
                egui::Label::new(egui::RichText::new(path).color(theme::TEXT))
                    .truncate()
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Open in file manager");
        if resp.clicked() {
            open_path(path, OpenTarget::Self_);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new("Open").small())
                .on_hover_text("Open in file manager")
                .clicked()
            {
                open_path(path, OpenTarget::Self_);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_picks_right_unit() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(1_099_511_627_776), "1.0 TB");
    }

    #[test]
    fn format_bytes_boundary_just_below_kb_stays_in_bytes() {
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn format_bps_picks_right_unit() {
        // bps uses powers of 1000 (SI), unlike format_bytes which uses 1024.
        assert_eq!(format_bps(0.0), "0 B/s");
        assert_eq!(format_bps(999.0), "999 B/s");
        assert_eq!(format_bps(1_000.0), "1 KB/s");
        assert_eq!(format_bps(1_000_000.0), "1.0 MB/s");
        assert_eq!(format_bps(1_000_000_000.0), "1.00 GB/s");
    }

    #[test]
    fn format_duration_picks_the_largest_unit_present() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(125), "2m 5s");
        assert_eq!(format_duration(3600), "1h 0m 0s");
        assert_eq!(format_duration(3725), "1h 2m 5s");
        assert_eq!(format_duration(86_400), "1d 0h 0m");
        assert_eq!(format_duration(90_061), "1d 1h 1m");
    }

    #[test]
    fn plot_x_anchors_newest_at_right_when_full() {
        // Full queue: oldest sample at x=0, newest at x=HISTORY_LEN-1.
        assert_eq!(plot_x_for_sample(0, HISTORY_LEN), 0.0);
        assert_eq!(
            plot_x_for_sample(HISTORY_LEN - 1, HISTORY_LEN),
            (HISTORY_LEN - 1) as f64
        );
    }

    #[test]
    fn plot_x_anchors_newest_at_right_when_partial() {
        // Half-full queue (30 samples): newest still lands at HISTORY_LEN-1,
        // oldest at HISTORY_LEN - data_len.
        let n = 30;
        assert_eq!(plot_x_for_sample(0, n), (HISTORY_LEN - n) as f64);
        assert_eq!(plot_x_for_sample(n - 1, n), (HISTORY_LEN - 1) as f64);
    }

    #[test]
    fn sample_for_plot_x_inverse_of_plot_x_for_sample() {
        for n in [1, 15, 30, HISTORY_LEN] {
            for i in 0..n {
                let x = plot_x_for_sample(i, n);
                assert_eq!(sample_for_plot_x(x, n), Some(i), "n={n} i={i}");
            }
        }
    }

    #[test]
    fn sample_for_plot_x_returns_none_outside_data() {
        // Left of the data region (when partially full) is empty space.
        assert_eq!(sample_for_plot_x(0.0, 10), None);
        // Right of HISTORY_LEN-1 is past the end.
        assert_eq!(sample_for_plot_x(HISTORY_LEN as f64, HISTORY_LEN), None);
        // Empty queue: nothing to hover.
        assert_eq!(sample_for_plot_x(5.0, 0), None);
    }

    #[test]
    fn max_in_returns_zero_for_empty() {
        let empty: [f64; 0] = [];
        assert_eq!(max_in(empty.iter().copied()), 0.0);
    }

    #[test]
    fn max_in_handles_negatives_by_collapsing_to_zero() {
        // Reductions seed with 0.0 — negative-only input still yields 0.
        assert_eq!(max_in([-5.0, -1.0, -100.0].iter().copied()), 0.0);
    }

    #[test]
    fn max_in_returns_largest_positive() {
        assert_eq!(max_in([0.5, 3.0, 1.7, 2.9].iter().copied()), 3.0);
    }
}
