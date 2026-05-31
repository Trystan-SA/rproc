use crate::monitor::Snapshot;
use crate::theme;

mod cards;
mod format;
mod panels;

use cards::render_cards;
use panels::{panel_cpu, panel_disk, panel_gpu, panel_memory, panel_network};

#[derive(Default, PartialEq, Copy, Clone)]
pub enum Section {
    #[default]
    Cpu,
    Memory,
    Disk(usize),
    Network(usize),
    Gpu(usize),
}

#[derive(Default)]
pub struct State {
    pub section: Section,
    /// User-controlled collapse of the right detail panel. Independent of the
    /// auto-hide breakpoint — at narrow widths the panel is hidden regardless.
    pub detail_collapsed: bool,
}

pub fn show(ui: &mut egui::Ui, state: &mut State, snap: &Snapshot) {
    let avail = ui.available_size();
    // Below this width the detail pane stops being useful (the plots collapse
    // to a few dozen pixels). Drop it and let the cards — which already carry
    // sparklines — take the full row.
    let auto_hide = avail.x < 600.0;
    let hide_detail = auto_hide || state.detail_collapsed;
    // Only offer "expand" when the user did the collapsing themselves — if
    // the window is too narrow there's nowhere to expand to.
    let show_expand_button = state.detail_collapsed && !auto_hide;

    if hide_detail {
        if show_expand_button {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if collapse_button(ui, true).clicked() {
                        state.detail_collapsed = false;
                    }
                });
            });
        }
        egui::ScrollArea::vertical()
            .id_salt("perf-sidebar")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                render_cards(ui, snap, &mut state.section);
            });
        return;
    }

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(250.0, avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("perf-sidebar")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        render_cards(ui, snap, &mut state.section);
                    });
            },
        );

        ui.add_space(8.0);

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if collapse_button(ui, false).clicked() {
                        state.detail_collapsed = true;
                    }
                });
            });
            match state.section {
                Section::Cpu => panel_cpu(ui, snap),
                Section::Memory => panel_memory(ui, snap),
                Section::Disk(i) => panel_disk(ui, snap, i),
                Section::Network(i) => panel_network(ui, snap, i),
                Section::Gpu(i) => panel_gpu(ui, snap, i),
            }
        });
    });
}

/// Chevron toggle that lives at the top-right of the detail panel (or, when
/// the panel is collapsed by the user, at the top-right of the cards area).
/// `expand == true` paints a left-facing chevron (panel will reappear from the
/// right); `false` paints a right-facing chevron (panel will retract to the right).
fn collapse_button(ui: &mut egui::Ui, expand: bool) -> egui::Response {
    let size = egui::vec2(26.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let bg = if resp.hovered() {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 24)
    } else {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(6), bg);
    let stroke = egui::Stroke::new(1.6, theme::TEXT);
    let c = rect.center();
    let s = 4.0;
    let (start_x, tip_x) = if expand {
        (c.x + s / 2.0, c.x - s / 2.0)
    } else {
        (c.x - s / 2.0, c.x + s / 2.0)
    };
    painter.line_segment(
        [egui::pos2(start_x, c.y - s), egui::pos2(tip_x, c.y)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(tip_x, c.y), egui::pos2(start_x, c.y + s)],
        stroke,
    );
    let tip_text = if expand {
        "Show details"
    } else {
        "Hide details"
    };
    resp.on_hover_text(tip_text)
}
