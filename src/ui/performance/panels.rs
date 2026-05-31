use std::collections::VecDeque;

use crate::monitor::Snapshot;
use crate::theme;
use crate::ui::widgets;

use super::format::{iface_label, short_disk_name};

pub(super) fn panel_cpu(ui: &mut egui::Ui, snap: &Snapshot) {
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

pub(super) fn panel_memory(ui: &mut egui::Ui, snap: &Snapshot) {
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

pub(super) fn panel_disk(ui: &mut egui::Ui, snap: &Snapshot, idx: usize) {
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

pub(super) fn panel_network(ui: &mut egui::Ui, snap: &Snapshot, idx: usize) {
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

pub(super) fn panel_gpu(ui: &mut egui::Ui, snap: &Snapshot, idx: usize) {
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
