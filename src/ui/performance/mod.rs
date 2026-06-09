use std::collections::VecDeque;
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::monitor::Snapshot;
use crate::monitor::battery;
use crate::theme;
use crate::ui::widgets::{
    self, HISTORY_LEN, format_bytes, format_duration, format_pct_value, format_time_ago,
};
use crate::ui::{attribution, graph};
use crate::{AttribRow, CardData, CoreCell, GraphSeries, MainWindow, StatLine};

pub mod format;

use format::{combined_disk, iface_label, short_disk_name, temp_label};

#[derive(Default, PartialEq, Copy, Clone)]
pub enum Section {
    #[default]
    Cpu,
    Memory,
    Disk(usize),
    Network(usize),
    Gpu(usize),
    Battery,
}

pub struct State {
    pub section: Section,
    pub detail_collapsed: bool,
    /// Snapped plot-x the pointer is hovering over the detail graph (0..59).
    pub hover: Option<f64>,
    /// Persistent model for the left-hand cards so clicks aren't dropped when a
    /// refresh tick lands between a card's press and release.
    cards: Rc<VecModel<CardData>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            section: Section::default(),
            detail_collapsed: false,
            hover: None,
            cards: Rc::new(VecModel::default()),
        }
    }
}

impl State {
    pub fn select(&mut self, id: &str) {
        self.section = match id {
            "cpu" => Section::Cpu,
            "mem" => Section::Memory,
            "battery" => Section::Battery,
            _ => {
                if let Some(n) = id.strip_prefix("gpu") {
                    Section::Gpu(n.parse().unwrap_or(0))
                } else if let Some(n) = id.strip_prefix("disk") {
                    Section::Disk(n.parse().unwrap_or(0))
                } else if let Some(n) = id.strip_prefix("net") {
                    Section::Network(n.parse().unwrap_or(0))
                } else {
                    Section::Cpu
                }
            }
        };
        // A different section invalidates the hovered sample.
        self.hover = None;
    }
}

fn ss(s: &str) -> SharedString {
    s.into()
}

fn stat(label: &str, value: &str) -> StatLine {
    StatLine {
        label: ss(label),
        value: ss(value),
        separator: false,
    }
}

fn separator() -> StatLine {
    StatLine {
        label: ss(""),
        value: ss(""),
        separator: true,
    }
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

pub fn apply(window: &MainWindow, state: &State, snap: &Snapshot, attribution_enabled: bool) {
    // Update the persistent cards model in place (don't replace it) so a click
    // landing across a refresh tick isn't dropped.
    crate::ui::model::sync(&state.cards, build_cards(state, snap));
    window.set_perf_cards(ModelRc::from(state.cards.clone()));
    window.set_perf_detail_collapsed(state.detail_collapsed);
    apply_detail(window, state, snap, attribution_enabled);
}

fn build_cards(state: &State, snap: &Snapshot) -> Vec<CardData> {
    let mut out = Vec::new();

    out.push(card_pct(
        "cpu",
        "CPU",
        &snap.system.cpu_brand,
        snap.system.cpu_total,
        snap.system.cpu_temp_c,
        &snap.history.cpu_total,
        theme::graph_cpu(),
        state.section == Section::Cpu,
    ));

    let ram_value = format!(
        "{} / {}",
        format_bytes(snap.system.ram_used),
        format_bytes(snap.system.ram_total)
    );
    out.push(card_pct(
        "mem",
        "Memory",
        &ram_value,
        snap.system.ram_used_pct,
        0.0,
        &snap.history.ram_used_pct,
        theme::graph_ram(),
        state.section == Section::Memory,
    ));

    let empty_f32: VecDeque<f32> = VecDeque::new();
    for (i, gpu) in snap.gpus.iter().enumerate() {
        let hist = snap.history.gpu_util.get(i).unwrap_or(&empty_f32);
        out.push(card_pct(
            &format!("gpu{i}"),
            &format!("GPU {} ({})", i, gpu.vendor),
            &gpu.name,
            gpu.util_pct,
            gpu.temp_c,
            hist,
            theme::graph_gpu(),
            state.section == Section::Gpu(i),
        ));
    }

    let empty_f64: VecDeque<f64> = VecDeque::new();
    for (i, d) in snap.system.disks.iter().enumerate() {
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
        out.push(card_bps(
            &format!("disk{i}"),
            &format!("Disk {}", short_disk_name(&d.name)),
            d.read_bps + d.write_bps,
            d.temp_c,
            &combined_disk(r, w),
            theme::graph_disk(),
            state.section == Section::Disk(i),
        ));
    }

    for (i, n) in snap.system.nets.iter().enumerate() {
        let rx = snap.history.net_rx_bps.get(&n.name).unwrap_or(&empty_f64);
        let tx = snap.history.net_tx_bps.get(&n.name).unwrap_or(&empty_f64);
        out.push(card_bps(
            &format!("net{i}"),
            &iface_label(&snap.system.nets, i),
            n.rx_bps + n.tx_bps,
            0.0,
            &combined_disk(rx, tx),
            theme::graph_net(),
            state.section == Section::Network(i),
        ));
    }

    if let Some(b) = &snap.battery {
        // The sparkline tracks power draw, not charge level — over 60 s the
        // level is a flat line; the draw is where the activity shows. Orange
        // while discharging, green otherwise, matching the detail graph.
        // A full battery draws ~0 W, which would read as an *empty* graph —
        // show the (full) charge level instead.
        let values = if b.status == battery::Status::Full {
            graph::norm_f32(&snap.history.battery_pct, 100.0)
        } else {
            let abs: VecDeque<f32> = snap
                .history
                .battery_power_w
                .iter()
                .map(|v| v.abs())
                .collect();
            let max = widgets::max_in(abs.iter().map(|v| *v as f64)).max(1.0) as f32;
            graph::norm_f32(&abs, max)
        };
        let color = if b.status == battery::Status::Discharging {
            theme::graph_battery_drain()
        } else {
            theme::graph_battery()
        };
        out.push(CardData {
            id: ss("battery"),
            title: ss("Battery"),
            subtitle: ss(b.status.label()),
            value: ss(&format!("{:.0}%", b.capacity_pct)),
            temp: ss(""),
            color,
            values,
            selected: state.section == Section::Battery,
        });
    }

    out
}

#[allow(clippy::too_many_arguments)]
fn card_pct(
    id: &str,
    title: &str,
    subtitle: &str,
    value: f32,
    temp_c: f32,
    history: &VecDeque<f32>,
    color: slint::Color,
    selected: bool,
) -> CardData {
    let value_str = if value.is_nan() {
        "N/A".to_string()
    } else {
        format!("{value:.0}%")
    };
    CardData {
        id: ss(id),
        title: ss(title),
        subtitle: ss(subtitle),
        value: ss(&value_str),
        temp: ss(&temp_label(temp_c).unwrap_or_default()),
        color,
        values: graph::norm_f32(history, 100.0),
        selected,
    }
}

#[allow(clippy::too_many_arguments)]
fn card_bps(
    id: &str,
    title: &str,
    value: f64,
    temp_c: f32,
    history: &VecDeque<f64>,
    color: slint::Color,
    selected: bool,
) -> CardData {
    let max = widgets::max_in(history.iter().copied()).max(1.0);
    CardData {
        id: ss(id),
        title: ss(title),
        subtitle: ss(""),
        value: ss(&widgets::format_bps(value)),
        temp: ss(&temp_label(temp_c).unwrap_or_default()),
        color,
        values: graph::norm_f64(history, max),
        selected,
    }
}

fn series_f32(history: &VecDeque<f32>, max: f32, color: slint::Color) -> GraphSeries {
    GraphSeries {
        color,
        values: graph::norm_f32(history, max),
    }
}

fn series_f64(history: &VecDeque<f64>, max: f64, color: slint::Color) -> GraphSeries {
    GraphSeries {
        color,
        values: graph::norm_f64(history, max),
    }
}

fn apply_detail(window: &MainWindow, state: &State, snap: &Snapshot, attribution_enabled: bool) {
    let empty_f32: VecDeque<f32> = VecDeque::new();
    let empty_f64: VecDeque<f64> = VecDeque::new();

    let title;
    let mut subtitle = String::new();
    let mut graph_title = "Usage (last 60s)";
    let mut series: Vec<GraphSeries> = Vec::new();
    let mut aux_series: Vec<GraphSeries> = Vec::new();
    let mut aux_title = String::new();
    let mut stats: Vec<StatLine> = Vec::new();
    let mut cores: Vec<CoreCell> = Vec::new();
    let mut show_cores = false;
    let mut gpu_warning = String::new();

    match state.section {
        Section::Cpu => {
            title = format!("CPU: {}", snap.system.cpu_brand);
            subtitle = format!(
                "{} cores ({} logical) · {} MHz",
                snap.system.physical_cores, snap.system.logical_cores, snap.system.cpu_freq_mhz
            );
            series.push(series_f32(
                &snap.history.cpu_total,
                100.0,
                theme::graph_cpu(),
            ));
            stats.push(stat("Current", &format!("{:.0}%", snap.system.cpu_total)));
            stats.push(stat("Uptime", &format_duration(snap.system.uptime_secs)));
            show_cores = true;
            for (i, v) in snap.system.per_core.iter().enumerate() {
                let h = snap.history.per_core_cpu.get(i).unwrap_or(&empty_f32);
                cores.push(CoreCell {
                    label: ss(&format!("Core {i}")),
                    value: ss(&format!("{v:.0}%")),
                    values: graph::norm_f32(h, 100.0),
                });
            }
        }
        Section::Memory => {
            title = "Memory".into();
            subtitle = format!("{} total", format_bytes(snap.system.ram_total));
            series.push(series_f32(
                &snap.history.ram_used_pct,
                100.0,
                theme::graph_ram(),
            ));
            stats.push(stat(
                "Used",
                &format!(
                    "{} ({:.0}%)",
                    format_bytes(snap.system.ram_used),
                    snap.system.ram_used_pct
                ),
            ));
            stats.push(stat(
                "Available",
                &format_bytes(snap.system.ram_total.saturating_sub(snap.system.ram_used)),
            ));
            stats.push(separator());
            stats.push(stat("Swap total", &format_bytes(snap.system.swap_total)));
            stats.push(stat("Swap used", &format_bytes(snap.system.swap_used)));
        }
        Section::Disk(i) => {
            if let Some(d) = snap.system.disks.get(i) {
                title = format!("Disk: {}", short_disk_name(&d.name));
                let part_word = if d.partitions > 1 {
                    "partitions"
                } else {
                    "partition"
                };
                subtitle = format!("{} · {} {}", d.fs, d.partitions, part_word);
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
                let max = widgets::max_in(r.iter().zip(w.iter()).map(|(a, b)| a + b)).max(1.0);
                series.push(series_f64(r, max, theme::graph_disk()));
                series.push(series_f64(w, max, theme::graph_net()));
                stats.push(stat("Read", &widgets::format_bps(d.read_bps)));
                stats.push(stat("Write", &widgets::format_bps(d.write_bps)));
                stats.push(separator());
                stats.push(stat("Total", &format_bytes(d.total)));
                stats.push(stat("Used", &format_bytes(d.used)));
                if !d.mounts.is_empty() {
                    stats.push(separator());
                    for m in &d.mounts {
                        stats.push(stat("Mount", m));
                    }
                }
            } else {
                title = "No disk".into();
            }
        }
        Section::Network(i) => {
            if let Some(n) = snap.system.nets.get(i) {
                title = iface_label(&snap.system.nets, i);
                subtitle = format!("{} · MAC {}", n.name, n.mac);
                let rx = snap.history.net_rx_bps.get(&n.name).unwrap_or(&empty_f64);
                let tx = snap.history.net_tx_bps.get(&n.name).unwrap_or(&empty_f64);
                let max = widgets::max_in(rx.iter().chain(tx.iter()).copied()).max(1.0);
                series.push(series_f64(rx, max, theme::graph_net()));
                series.push(series_f64(tx, max, theme::graph_disk()));
                stats.push(stat("Receive", &widgets::format_bps(n.rx_bps)));
                stats.push(stat("Send", &widgets::format_bps(n.tx_bps)));
                stats.push(separator());
                stats.push(stat("Total received", &format_bytes(n.rx_total)));
                stats.push(stat("Total sent", &format_bytes(n.tx_total)));
                if let Some(w) = &n.wifi {
                    let q = snap.history.wifi_quality.get(&n.name).unwrap_or(&empty_f32);
                    aux_series.push(series_f32(q, 100.0, theme::graph_wifi()));
                    aux_title = "Signal quality (last 60s)".into();
                    stats.push(separator());
                    stats.push(stat("Signal quality", &format!("{:.0}%", w.quality_pct)));
                    stats.push(stat("Signal level", &format!("{:.0} dBm", w.signal_dbm)));
                    stats.push(stat("Link quality", &format!("{:.0}", w.link_quality)));
                }
                // Network attribution is intentionally unavailable.
            } else {
                title = "No interface".into();
            }
        }
        Section::Gpu(i) => {
            if let Some(g) = snap.gpus.get(i) {
                title = format!("GPU: {}", g.name);
                subtitle = format!("{} · driver {}", g.vendor, g.driver);
                if g.util_pct.is_nan() {
                    gpu_warning = if g.vendor.eq_ignore_ascii_case("Intel") {
                        "rproc could not open the Intel i915/xe perf PMU. The kernel requires \
                         CAP_PERFMON, or kernel.perf_event_paranoid ≤ 2, to read GPU engine counters."
                            .into()
                    } else {
                        "rproc could not read this GPU's utilization counter. The kernel requires \
                         elevated permissions to access the perf PMU."
                            .into()
                    };
                }
                let util = snap.history.gpu_util.get(i).unwrap_or(&empty_f32);
                series.push(series_f32(util, 100.0, theme::graph_gpu()));
                if g.mem_total > 0 {
                    let mem = snap.history.gpu_mem_pct.get(i).unwrap_or(&empty_f32);
                    aux_series.push(series_f32(mem, 100.0, theme::graph_ram()));
                    aux_title = if g.mem_shared {
                        "Memory, shared (last 60s)".into()
                    } else {
                        "VRAM (last 60s)".into()
                    };
                }
                let util_label = if g.util_pct.is_nan() {
                    "N/A".to_string()
                } else {
                    format!("{:.0}%", g.util_pct)
                };
                stats.push(stat("Utilization", &util_label));
                if g.mem_total > 0 {
                    stats.push(stat(
                        if g.mem_shared {
                            "Memory (shared)"
                        } else {
                            "VRAM"
                        },
                        &format!(
                            "{} / {} ({:.0}%)",
                            format_bytes(g.mem_used),
                            format_bytes(g.mem_total),
                            (g.mem_used as f32 / g.mem_total as f32) * 100.0
                        ),
                    ));
                }
                if g.temp_c > 0.0 {
                    stats.push(stat("Temperature", &format!("{:.0}C", g.temp_c)));
                }
                if g.power_w > 0.0 {
                    stats.push(stat("Power", &format!("{:.1} W", g.power_w)));
                }
                if g.clock_mhz > 0 {
                    stats.push(stat("Core clock", &format!("{} MHz", g.clock_mhz)));
                }
                if g.mem_clock_mhz > 0 {
                    stats.push(stat("Memory clock", &format!("{} MHz", g.mem_clock_mhz)));
                }
            } else {
                title = "No GPU".into();
            }
        }
        Section::Battery => {
            if let Some(b) = &snap.battery {
                title = "Battery".into();
                let mut parts: Vec<&str> = Vec::new();
                if !b.model.is_empty() {
                    parts.push(&b.model);
                }
                if !b.technology.is_empty() {
                    parts.push(&b.technology);
                }
                subtitle = parts.join(" · ");
                if b.status == battery::Status::Full {
                    // ~0 W when full would read as an empty graph; plot the
                    // (full) charge level instead.
                    graph_title = "Charge (last 60s)";
                    series.push(series_f32(
                        &snap.history.battery_pct,
                        100.0,
                        theme::graph_battery(),
                    ));
                } else {
                    graph_title = "Power (last 60s)";
                    // Split the signed series so discharge (orange) and charge
                    // (green) segments keep their color through the history.
                    let power = &snap.history.battery_power_w;
                    let max = widgets::max_in(power.iter().map(|v| v.abs() as f64)).max(1.0) as f32;
                    let drain: VecDeque<f32> = power.iter().map(|v| v.max(0.0)).collect();
                    let gain: VecDeque<f32> = power.iter().map(|v| (-v).max(0.0)).collect();
                    series.push(series_f32(&drain, max, theme::graph_battery_drain()));
                    series.push(series_f32(&gain, max, theme::graph_battery()));
                }
                stats.push(stat("Charge", &format!("{:.0}%", b.capacity_pct)));
                let status = if b.ac_online && b.status != battery::Status::Charging {
                    format!("{} (AC connected)", b.status.label())
                } else {
                    b.status.label().to_string()
                };
                stats.push(stat("Status", &status));
                if b.power_w > 0.05 {
                    stats.push(stat("Power", &format!("{:.1} W", b.power_w)));
                }
                if let Some(secs) = b.time_left_secs() {
                    let label = if b.status == battery::Status::Charging {
                        "Time to full"
                    } else {
                        "Time remaining"
                    };
                    stats.push(stat(label, &format_duration(secs)));
                }
                if b.energy_full_wh > 0.0 || b.health_pct().is_some() || b.cycle_count > 0 {
                    stats.push(separator());
                }
                if b.energy_full_wh > 0.0 {
                    stats.push(stat(
                        "Energy",
                        &format!("{:.1} / {:.1} Wh", b.energy_now_wh, b.energy_full_wh),
                    ));
                }
                if let Some(h) = b.health_pct() {
                    stats.push(stat(
                        "Health",
                        &format!("{:.0}% of {:.1} Wh design", h, b.energy_full_design_wh),
                    ));
                }
                if b.cycle_count > 0 {
                    stats.push(stat("Cycle count", &b.cycle_count.to_string()));
                }
            } else {
                title = "No battery".into();
            }
        }
    }

    window.set_perf_detail_title(ss(&title));
    window.set_perf_detail_subtitle(ss(&subtitle));
    window.set_perf_graph_title(ss(graph_title));
    window.set_perf_detail_series(model(series));
    window.set_perf_aux_series(model(aux_series));
    window.set_perf_aux_title(ss(&aux_title));
    window.set_perf_detail_stats(model(stats));
    window.set_perf_detail_cores(model(cores));
    window.set_perf_show_cores(show_cores);
    window.set_perf_gpu_warning(ss(&gpu_warning));

    // Crosshair, readout and attribution depend only on the hovered sample, not
    // on the series/cores models built above. Refresh them in their own pass so
    // a pointer move can update just the overlay — replacing the detail models
    // here would recreate every delegate (and relayout) on each mouse move.
    apply_hover(window, state, snap, attribution_enabled);
}

/// Per-section series refs feeding the hover readout, plus the attribution kind
/// that section maps to. Read-only and allocation-light (no model building), so
/// it is cheap to recompute on every pointer move.
fn section_refs<'a>(
    section: Section,
    snap: &'a Snapshot,
    empty_f32: &'a VecDeque<f32>,
    empty_f64: &'a VecDeque<f64>,
) -> (Vec<(String, SeriesRef<'a>)>, Option<attribution::Kind>) {
    let mut data: Vec<(String, SeriesRef<'a>)> = Vec::new();
    let kind = match section {
        Section::Cpu => {
            data.push((String::new(), SeriesRef::Pct(&snap.history.cpu_total)));
            Some(attribution::Kind::Cpu)
        }
        Section::Memory => {
            data.push((String::new(), SeriesRef::Pct(&snap.history.ram_used_pct)));
            Some(attribution::Kind::Ram)
        }
        Section::Disk(i) => snap.system.disks.get(i).map(|d| {
            let r = snap.history.disk_read_bps.get(&d.name).unwrap_or(empty_f64);
            let w = snap
                .history
                .disk_write_bps
                .get(&d.name)
                .unwrap_or(empty_f64);
            data.push(("read".into(), SeriesRef::Bps(r)));
            data.push(("write".into(), SeriesRef::Bps(w)));
            attribution::Kind::Disk
        }),
        Section::Network(i) => {
            if let Some(n) = snap.system.nets.get(i) {
                let rx = snap.history.net_rx_bps.get(&n.name).unwrap_or(empty_f64);
                let tx = snap.history.net_tx_bps.get(&n.name).unwrap_or(empty_f64);
                data.push(("receive".into(), SeriesRef::Bps(rx)));
                data.push(("send".into(), SeriesRef::Bps(tx)));
            }
            None
        }
        Section::Gpu(i) => snap.gpus.get(i).map(|g| {
            let util = snap.history.gpu_util.get(i).unwrap_or(empty_f32);
            data.push(("util".into(), SeriesRef::Pct(util)));
            if g.mem_total > 0 {
                let mem = snap.history.gpu_mem_pct.get(i).unwrap_or(empty_f32);
                let name = if g.mem_shared { "mem" } else { "vram" };
                data.push((name.into(), SeriesRef::Pct(mem)));
            }
            attribution::Kind::Gpu
        }),
        Section::Battery => {
            // Mirror the plotted series: charge level leads when full,
            // power draw otherwise.
            if snap
                .battery
                .as_ref()
                .is_some_and(|b| b.status == battery::Status::Full)
            {
                data.push((String::new(), SeriesRef::Pct(&snap.history.battery_pct)));
                data.push((
                    "power".into(),
                    SeriesRef::Watts(&snap.history.battery_power_w),
                ));
            } else {
                data.push((
                    String::new(),
                    SeriesRef::Watts(&snap.history.battery_power_w),
                ));
                data.push(("charge".into(), SeriesRef::Pct(&snap.history.battery_pct)));
            }
            Some(attribution::Kind::Battery)
        }
    };
    (data, kind)
}

/// Updates only the hover crosshair, readout label and attribution rows for the
/// current section. Cheap enough to run on every pointer move — it never
/// rebuilds the detail series, per-core graphs or stat models.
pub fn apply_hover(window: &MainWindow, state: &State, snap: &Snapshot, attribution_enabled: bool) {
    let empty_f32: VecDeque<f32> = VecDeque::new();
    let empty_f64: VecDeque<f64> = VecDeque::new();
    let (series_data, attrib_kind) = section_refs(state.section, snap, &empty_f32, &empty_f64);

    let hover = state.hover;
    window.set_perf_hover_active(hover.is_some());
    if let Some(x) = hover {
        window.set_perf_hover_x((x / (HISTORY_LEN - 1) as f64) as f32);
        let samples_ago = (HISTORY_LEN - 1) as i64 - x.round() as i64;
        let mut label = format_time_ago(samples_ago, snap.sample_interval_ms);
        for (name, sref) in &series_data {
            let value = sref.value_at(x);
            let line = match (name.is_empty(), value) {
                (true, Some(v)) => v,
                (false, Some(v)) => format!("{name}  {v}"),
                (true, None) => "—".into(),
                (false, None) => format!("{name}  —"),
            };
            label.push('\n');
            label.push_str(&line);
        }
        window.set_perf_hover_label(ss(&label));
    } else {
        window.set_perf_hover_x(0.0);
        window.set_perf_hover_label(ss(""));
    }

    let history = &snap.history.attribution;
    let active = attribution_enabled && attrib_kind.is_some() && !history.is_empty();
    window.set_perf_attrib_active(active);
    if active {
        let kind = attrib_kind.unwrap();
        let shares = attribution::shares_at(history, kind, hover);
        match shares {
            Some(list) if !list.is_empty() => {
                window.set_perf_attrib_empty(false);
                let rows: Vec<AttribRow> = list
                    .iter()
                    .map(|s| AttribRow {
                        name: ss(&s.name),
                        pid: s.pid as i32,
                        value: ss(&attribution::format_value(kind, s)),
                    })
                    .collect();
                window.set_perf_attrib_rows(model(rows));
            }
            Some(_) => {
                window.set_perf_attrib_empty(true);
                window.set_perf_attrib_rows(model(Vec::new()));
            }
            None => {
                window.set_perf_attrib_empty(false);
                window.set_perf_attrib_rows(model(Vec::new()));
            }
        }
    } else {
        window.set_perf_attrib_empty(false);
        window.set_perf_attrib_rows(model(Vec::new()));
    }
}

/// Borrowed series used only to compute the hover readout value at a plot-x.
/// The variant picks the unit the value is formatted with.
enum SeriesRef<'a> {
    Pct(&'a VecDeque<f32>),
    Watts(&'a VecDeque<f32>),
    Bps(&'a VecDeque<f64>),
}

impl SeriesRef<'_> {
    fn value_at(&self, snapped_x: f64) -> Option<String> {
        match self {
            SeriesRef::Pct(d) => widgets::sample_for_plot_x(snapped_x, d.len())
                .and_then(|i| d.get(i))
                .map(|v| format_pct_value(*v as f64)),
            // Signed watts (see History::battery_power_w): negative = charging.
            SeriesRef::Watts(d) => widgets::sample_for_plot_x(snapped_x, d.len())
                .and_then(|i| d.get(i))
                .map(|v| {
                    if *v < 0.0 {
                        format!("{:.1} W charging", -v)
                    } else {
                        format!("{v:.1} W")
                    }
                }),
            SeriesRef::Bps(d) => widgets::sample_for_plot_x(snapped_x, d.len())
                .and_then(|i| d.get(i))
                .map(|v| widgets::format_bps(*v)),
        }
    }
}
