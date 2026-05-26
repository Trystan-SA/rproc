use std::collections::VecDeque;

use crate::monitor::Snapshot;
use crate::theme;
use crate::ui::widgets;

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

fn render_cards(ui: &mut egui::Ui, snap: &Snapshot, current: &mut Section) {
    card_button_pct(
        ui,
        "cpu",
        "CPU",
        &snap.system.cpu_brand,
        snap.system.cpu_total,
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
            &combined_disk(rx, tx),
            theme::GRAPH_NET,
            Section::Network(i),
            current,
        );
    }
}

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

fn iface_label(nets: &[crate::monitor::system::NetInfo], idx: usize) -> String {
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

fn short_disk_name(n: &str) -> String {
    n.strip_prefix("/dev/").unwrap_or(n).to_string()
}

fn combined_disk(a: &VecDeque<f64>, b: &VecDeque<f64>) -> VecDeque<f64> {
    let len = a.len().max(b.len());
    let mut out = VecDeque::with_capacity(len);
    for i in 0..len {
        let av = a.get(i).copied().unwrap_or(0.0);
        let bv = b.get(i).copied().unwrap_or(0.0);
        out.push_back(av + bv);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn card_button_pct(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    subtitle: &str,
    value: f32,
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
        ui, id, title, subtitle, &label, history, 100.0, color, selected,
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
                    ui.label(egui::RichText::new(value).color(color).size(15.0));
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

// --- detail panels ---

fn panel_cpu(ui: &mut egui::Ui, snap: &Snapshot) {
    ui.heading(format!("CPU: {}", snap.system.cpu_brand));
    ui.label(
        egui::RichText::new(format!(
            "{} cores ({} logical) · {} MHz",
            snap.system.physical_cores, snap.system.logical_cores, snap.system.cpu_freq_mhz
        ))
        .color(theme::TEXT_DIM),
    );
    ui.add_space(8.0);

    widgets::card(ui, |ui| {
        ui.label("Total usage (last 60s)");
        widgets::big_plot(
            ui,
            "cpu_total_plot",
            &[("cpu", &snap.history.cpu_total, theme::GRAPH_CPU)],
            100.0,
            180.0,
            snap.sample_interval_ms,
        );
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Current").color(theme::TEXT_DIM));
                ui.label(egui::RichText::new(format!("{:.0}%", snap.system.cpu_total)).strong());
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(20.0);
                ui.label(egui::RichText::new("Uptime").color(theme::TEXT_DIM));
                ui.label(
                    egui::RichText::new(widgets::format_duration(snap.system.uptime_secs)).strong(),
                );
            });
        });
    });

    ui.add_space(8.0);
    widgets::card(ui, |ui| {
        ui.label("Per-core usage");
        ui.add_space(4.0);
        let cols = 4;
        let cores = &snap.system.per_core;
        let spacing_x = ui.spacing().item_spacing.x;
        let total_w = ui.available_width();
        let cell_w = ((total_w - spacing_x * (cols as f32 - 1.0)) / cols as f32).max(80.0);
        for (row_idx, row) in cores.chunks(cols).enumerate() {
            ui.horizontal(|ui| {
                for (col_idx, &v) in row.iter().enumerate() {
                    let core = row_idx * cols + col_idx;
                    ui.allocate_ui(egui::vec2(cell_w, 56.0), |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("Core {core}")).strong().small(),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(format!("{v:.0}%"));
                                    },
                                );
                            });
                            let empty: VecDeque<f32> = VecDeque::new();
                            let h = snap.history.per_core_cpu.get(core).unwrap_or(&empty);
                            widgets::sparkline(
                                ui,
                                &format!("core_spark_{core}"),
                                h,
                                100.0,
                                theme::GRAPH_CPU,
                                32.0,
                            );
                        });
                    });
                }
            });
            ui.add_space(2.0);
        }
    });
}

fn panel_memory(ui: &mut egui::Ui, snap: &Snapshot) {
    ui.heading("Memory");
    ui.label(
        egui::RichText::new(format!(
            "{} total",
            widgets::format_bytes(snap.system.ram_total)
        ))
        .color(theme::TEXT_DIM),
    );
    ui.add_space(8.0);

    widgets::card(ui, |ui| {
        ui.label("Usage (last 60s)");
        widgets::big_plot(
            ui,
            "ram_plot",
            &[("ram", &snap.history.ram_used_pct, theme::GRAPH_RAM)],
            100.0,
            180.0,
            snap.sample_interval_ms,
        );
        ui.add_space(8.0);
        widgets::stat(
            ui,
            "Used",
            &format!(
                "{} ({:.0}%)",
                widgets::format_bytes(snap.system.ram_used),
                snap.system.ram_used_pct
            ),
        );
        widgets::stat(
            ui,
            "Available",
            &widgets::format_bytes(snap.system.ram_total.saturating_sub(snap.system.ram_used)),
        );
        ui.separator();
        widgets::stat(
            ui,
            "Swap total",
            &widgets::format_bytes(snap.system.swap_total),
        );
        widgets::stat(
            ui,
            "Swap used",
            &widgets::format_bytes(snap.system.swap_used),
        );
    });
}

fn panel_disk(ui: &mut egui::Ui, snap: &Snapshot, idx: usize) {
    let Some(d) = snap.system.disks.get(idx) else {
        ui.label("No disk");
        return;
    };
    ui.heading(format!("Disk: {}", short_disk_name(&d.name)));
    let part_word = if d.partitions > 1 {
        "partitions"
    } else {
        "partition"
    };
    ui.label(
        egui::RichText::new(format!("{} · {} {}", d.fs, d.partitions, part_word))
            .color(theme::TEXT_DIM),
    );
    ui.add_space(8.0);

    let empty: VecDeque<f64> = VecDeque::new();
    let r_hist = snap.history.disk_read_bps.get(&d.name).unwrap_or(&empty);
    let w_hist = snap.history.disk_write_bps.get(&d.name).unwrap_or(&empty);

    widgets::card(ui, |ui| {
        ui.label("Total I/O (read + write, last 60s)");
        let max = widgets::max_in(r_hist.iter().zip(w_hist.iter()).map(|(a, b)| a + b)).max(1.0);
        widgets::big_plot_f64(
            ui,
            &format!("disk_plot_{idx}"),
            &[
                ("read", r_hist, theme::GRAPH_DISK),
                ("write", w_hist, theme::GRAPH_NET),
            ],
            max,
            180.0,
            snap.sample_interval_ms,
        );
        ui.add_space(8.0);
        widgets::stat(ui, "Read", &widgets::format_bps(d.read_bps));
        widgets::stat(ui, "Write", &widgets::format_bps(d.write_bps));
        ui.separator();
        widgets::stat(ui, "Total", &widgets::format_bytes(d.total));
        widgets::stat(ui, "Used", &widgets::format_bytes(d.used));
        if !d.mounts.is_empty() {
            ui.separator();
            for m in &d.mounts {
                widgets::stat(ui, "Mount", m);
            }
        }
    });
}

fn panel_network(ui: &mut egui::Ui, snap: &Snapshot, idx: usize) {
    let Some(n) = snap.system.nets.get(idx) else {
        ui.label("No interface");
        return;
    };
    ui.heading(iface_label(&snap.system.nets, idx));
    ui.label(egui::RichText::new(format!("{} · MAC {}", n.name, n.mac)).color(theme::TEXT_DIM));
    ui.add_space(8.0);

    let empty: VecDeque<f64> = VecDeque::new();
    let rx_hist = snap.history.net_rx_bps.get(&n.name).unwrap_or(&empty);
    let tx_hist = snap.history.net_tx_bps.get(&n.name).unwrap_or(&empty);

    widgets::card(ui, |ui| {
        ui.label("Throughput (last 60s)");
        let max = widgets::max_in(rx_hist.iter().chain(tx_hist.iter()).copied()).max(1.0);
        widgets::big_plot_f64(
            ui,
            "net_plot",
            &[
                ("rx", rx_hist, theme::GRAPH_NET),
                ("tx", tx_hist, theme::GRAPH_DISK),
            ],
            max,
            180.0,
            snap.sample_interval_ms,
        );
        ui.add_space(8.0);
        widgets::stat(ui, "Receive", &widgets::format_bps(n.rx_bps));
        widgets::stat(ui, "Send", &widgets::format_bps(n.tx_bps));
        ui.separator();
        widgets::stat(ui, "Total received", &widgets::format_bytes(n.rx_total));
        widgets::stat(ui, "Total sent", &widgets::format_bytes(n.tx_total));
    });
}

fn gpu_util_unavailable_banner(ui: &mut egui::Ui, vendor: &str) {
    // Tinted card with a yellow border-ish fill so it reads as a warning
    // without screaming. Same shape as widgets::card so it lines up.
    let bg = egui::Color32::from_rgba_unmultiplied(0xFF, 0xC4, 0x4D, 28);
    egui::Frame::new()
        .fill(bg)
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("GPU utilization unavailable")
                    .color(theme::WARN)
                    .strong(),
            );
            ui.add_space(4.0);
            let detail = if vendor.eq_ignore_ascii_case("Intel") {
                "rproc could not open the Intel i915/xe perf PMU. The kernel requires \
                 CAP_PERFMON, or kernel.perf_event_paranoid ≤ 2, to read GPU engine counters."
            } else {
                "rproc could not read this GPU's utilization counter. The kernel requires \
                 elevated permissions to access the perf PMU."
            };
            ui.label(egui::RichText::new(detail).color(theme::TEXT_DIM));
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Fix (pick one):").color(theme::TEXT_DIM));
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("  sudo setcap cap_perfmon=ep $(which rproc)")
                    .monospace()
                    .color(theme::TEXT),
            );
            ui.label(
                egui::RichText::new("  sudo sysctl kernel.perf_event_paranoid=2")
                    .monospace()
                    .color(theme::TEXT),
            );
        });
}

fn panel_gpu(ui: &mut egui::Ui, snap: &Snapshot, idx: usize) {
    let Some(g) = snap.gpus.get(idx) else {
        ui.label("No GPU");
        return;
    };
    ui.heading(format!("GPU: {}", g.name));
    ui.label(
        egui::RichText::new(format!("{} · driver {}", g.vendor, g.driver)).color(theme::TEXT_DIM),
    );
    ui.add_space(8.0);

    if g.util_pct.is_nan() {
        gpu_util_unavailable_banner(ui, &g.vendor);
        ui.add_space(8.0);
    }

    widgets::card(ui, |ui| {
        ui.label("Utilization (last 60s)");
        let empty: VecDeque<f32> = VecDeque::new();
        let util_hist = snap.history.gpu_util.get(idx).unwrap_or(&empty);
        let mem_hist = snap.history.gpu_mem_pct.get(idx).unwrap_or(&empty);
        widgets::big_plot(
            ui,
            "gpu_plot",
            &[
                ("util", util_hist, theme::GRAPH_GPU),
                ("vram", mem_hist, theme::GRAPH_RAM),
            ],
            100.0,
            180.0,
            snap.sample_interval_ms,
        );
        ui.add_space(8.0);
        let util_label = if g.util_pct.is_nan() {
            "N/A".to_string()
        } else {
            format!("{:.0}%", g.util_pct)
        };
        widgets::stat(ui, "Utilization", &util_label);
        if g.mem_total > 0 {
            widgets::stat(
                ui,
                "VRAM",
                &format!(
                    "{} / {} ({:.0}%)",
                    widgets::format_bytes(g.mem_used),
                    widgets::format_bytes(g.mem_total),
                    (g.mem_used as f32 / g.mem_total as f32) * 100.0
                ),
            );
        }
        if g.temp_c > 0.0 {
            widgets::stat(ui, "Temperature", &format!("{:.0} °C", g.temp_c));
        }
        if g.power_w > 0.0 {
            widgets::stat(ui, "Power", &format!("{:.1} W", g.power_w));
        }
        if g.clock_mhz > 0 {
            widgets::stat(ui, "Core clock", &format!("{} MHz", g.clock_mhz));
        }
        if g.mem_clock_mhz > 0 {
            widgets::stat(ui, "Memory clock", &format!("{} MHz", g.mem_clock_mhz));
        }
    });
}
