use std::collections::VecDeque;

use crate::monitor::Snapshot;
use crate::theme;
use crate::ui::widgets;

use super::Section;
use super::format::{combined_disk, iface_label, short_disk_name, temp_label};

pub(super) fn render_cards(ui: &mut egui::Ui, snap: &Snapshot, current: &mut Section) {
    card_button_pct(
        ui,
        "cpu",
        "CPU",
        &snap.system.cpu_brand,
        snap.system.cpu_total,
        snap.system.cpu_temp_c,
        &snap.history.cpu_total,
        theme::GRAPH_CPU,
        Section::Cpu,
        current,
    );
    let ram_value = format!(
        "{} / {}",
        widgets::format_bytes(snap.system.ram_used),
        widgets::format_bytes(snap.system.ram_total)
    );
    card_button_pct(
        ui,
        "mem",
        "Memory",
        &ram_value,
        snap.system.ram_used_pct,
        0.0,
        &snap.history.ram_used_pct,
        theme::GRAPH_RAM,
        Section::Memory,
        current,
    );
    for (i, gpu) in snap.gpus.iter().enumerate() {
        let title = format!("GPU {} ({})", i, gpu.vendor);
        let empty: VecDeque<f32> = VecDeque::new();
        let hist = snap.history.gpu_util.get(i).unwrap_or(&empty);
        card_button_pct(
            ui,
            &format!("gpu{i}"),
            &title,
            &gpu.name,
            gpu.util_pct,
            gpu.temp_c,
            hist,
            theme::GRAPH_GPU,
            Section::Gpu(i),
            current,
        );
    }
    let empty_f64: VecDeque<f64> = VecDeque::new();
    for (i, d) in snap.system.disks.iter().enumerate() {
        let v = widgets::format_bps(d.read_bps + d.write_bps);
        let r = snap
            .history
            .disk_read_bps
            .get(&d.name)
            .unwrap_or(&empty_f64);
        let w = snap
            .history
            .disk_write_bps
            .get(&d.name)
            .unwrap_or(&empty_f64);
        card_button_bps(
            ui,
            &format!("disk{i}"),
            &format!("Disk {}", short_disk_name(&d.name)),
            &v,
            d.temp_c,
            &combined_disk(r, w),
            theme::GRAPH_DISK,
            Section::Disk(i),
            current,
        );
    }
    let empty_net: VecDeque<f64> = VecDeque::new();
    for (i, n) in snap.system.nets.iter().enumerate() {
        let v = widgets::format_bps(n.rx_bps + n.tx_bps);
        let rx = snap.history.net_rx_bps.get(&n.name).unwrap_or(&empty_net);
        let tx = snap.history.net_tx_bps.get(&n.name).unwrap_or(&empty_net);
        card_button_bps(
            ui,
            &format!("net{i}"),
            &iface_label(&snap.system.nets, i),
            &v,
            0.0,
            &combined_disk(rx, tx),
            theme::GRAPH_NET,
            Section::Network(i),
            current,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn card_button_pct(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    subtitle: &str,
    value: f32,
    temp_c: f32,
    history: &VecDeque<f32>,
    color: egui::Color32,
    section: Section,
    current: &mut Section,
) {
    let selected = *current == section;
    let label = if value.is_nan() {
        "N/A".to_string()
    } else {
        format!("{value:.0}%")
    };
    let resp = card_button_inner(
        ui,
        id,
        title,
        subtitle,
        &label,
        temp_label(temp_c),
        history,
        100.0,
        color,
        selected,
    );
    if resp.clicked() {
        *current = section;
    }
}

#[allow(clippy::too_many_arguments)]
fn card_button_bps(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    value: &str,
    temp_c: f32,
    history: &VecDeque<f64>,
    color: egui::Color32,
    section: Section,
    current: &mut Section,
) {
    let selected = *current == section;
    let max = widgets::max_in(history.iter().copied()).max(1.0);
    let bg = if selected {
        egui::Color32::from_rgba_unmultiplied(0x60, 0xCD, 0xFF, 35)
    } else {
        theme::CARD_BG
    };
    let inner = egui::Frame::new()
        .fill(bg)
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(8))
        .show(ui, |ui| {
            ui.set_min_width(220.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).strong());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(value).color(color).size(15.0));
                        if let Some(t) = temp_label(temp_c) {
                            ui.label(egui::RichText::new(t).color(theme::TEXT_DIM).size(11.0));
                        }
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.allocate_ui(egui::vec2(80.0, 40.0), |ui| {
                        widgets::sparkline_f64(
                            ui,
                            &format!("spark_{id}"),
                            history,
                            max,
                            color,
                            40.0,
                        );
                    });
                });
            });
        });
    let rect = inner.response.rect;
    ui.add_space(6.0);
    let resp = ui.interact(
        rect,
        egui::Id::new(("perf_card_bps", id)),
        egui::Sense::click(),
    );
    if resp.clicked() {
        *current = section;
    }
}

#[allow(clippy::too_many_arguments)]
fn card_button_inner(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    subtitle: &str,
    value: &str,
    temp: Option<String>,
    history: &VecDeque<f32>,
    max: f32,
    color: egui::Color32,
    selected: bool,
) -> egui::Response {
    let bg = if selected {
        egui::Color32::from_rgba_unmultiplied(0x60, 0xCD, 0xFF, 35)
    } else {
        theme::CARD_BG
    };
    let inner = egui::Frame::new()
        .fill(bg)
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(8))
        .show(ui, |ui| {
            ui.set_min_width(220.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(title).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(value).color(color).strong().size(15.0));
                        if let Some(t) = &temp {
                            ui.label(egui::RichText::new(t).color(theme::TEXT_DIM).size(11.0));
                        }
                    });
                });
                ui.label(egui::RichText::new(subtitle).color(theme::TEXT_DIM).small());
                ui.add_space(2.0);
                widgets::sparkline(ui, &format!("spark_{id}"), history, max, color, 36.0);
            });
        });
    let rect = inner.response.rect;
    ui.add_space(6.0);
    ui.interact(rect, egui::Id::new(("perf_card", id)), egui::Sense::click())
}
