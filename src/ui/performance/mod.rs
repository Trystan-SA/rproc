use std::collections::VecDeque;
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::monitor::Snapshot;
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
    let mut series: Vec<GraphSeries> = Vec::new();
    let mut stats: Vec<StatLine> = Vec::new();
    let mut cores: Vec<CoreCell> = Vec::new();
    let mut show_cores = false;
    let mut gpu_warning = String::new();
    let mut attrib_kind: Option<attribution::Kind> = None;
    // Per-series labels used by the hover readout.
    let mut series_data: Vec<(String, SeriesRef<'_>)> = Vec::new();

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
            series_data.push((String::new(), SeriesRef::F32(&snap.history.cpu_total, true)));
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
            attrib_kind = Some(attribution::Kind::Cpu);
        }
        Section::Memory => {
            title = "Memory".into();
            subtitle = format!("{} total", format_bytes(snap.system.ram_total));
            series.push(series_f32(
                &snap.history.ram_used_pct,
                100.0,
                theme::graph_ram(),
            ));
            series_data.push((
                String::new(),
                SeriesRef::F32(&snap.history.ram_used_pct, true),
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
            attrib_kind = Some(attribution::Kind::Ram);
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
                series_data.push(("read".into(), SeriesRef::F64(r)));
                series_data.push(("write".into(), SeriesRef::F64(w)));
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
                attrib_kind = Some(attribution::Kind::Disk);
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
                series_data.push(("rx".into(), SeriesRef::F64(rx)));
                series_data.push(("tx".into(), SeriesRef::F64(tx)));
                stats.push(stat("Receive", &widgets::format_bps(n.rx_bps)));
                stats.push(stat("Send", &widgets::format_bps(n.tx_bps)));
                stats.push(separator());
                stats.push(stat("Total received", &format_bytes(n.rx_total)));
                stats.push(stat("Total sent", &format_bytes(n.tx_total)));
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
                let mem = snap.history.gpu_mem_pct.get(i).unwrap_or(&empty_f32);
                series.push(series_f32(util, 100.0, theme::graph_gpu()));
                series.push(series_f32(mem, 100.0, theme::graph_ram()));
                series_data.push(("util".into(), SeriesRef::F32(util, true)));
                series_data.push(("vram".into(), SeriesRef::F32(mem, true)));
                let util_label = if g.util_pct.is_nan() {
                    "N/A".to_string()
                } else {
                    format!("{:.0}%", g.util_pct)
                };
                stats.push(stat("Utilization", &util_label));
                if g.mem_total > 0 {
                    stats.push(stat(
                        "VRAM",
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
                attrib_kind = Some(attribution::Kind::Gpu);
            } else {
                title = "No GPU".into();
            }
        }
    }

    window.set_perf_detail_title(ss(&title));
    window.set_perf_detail_subtitle(ss(&subtitle));
    window.set_perf_detail_series(model(series));
    window.set_perf_detail_stats(model(stats));
    window.set_perf_detail_cores(model(cores));
    window.set_perf_show_cores(show_cores);
    window.set_perf_gpu_warning(ss(&gpu_warning));

    // Hover crosshair + readout.
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

    // Attribution panel.
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
enum SeriesRef<'a> {
    /// f32 percentage series; the bool marks it as a percentage for formatting.
    F32(&'a VecDeque<f32>, bool),
    F64(&'a VecDeque<f64>),
}

impl SeriesRef<'_> {
    fn value_at(&self, snapped_x: f64) -> Option<String> {
        match self {
            SeriesRef::F32(d, _) => widgets::sample_for_plot_x(snapped_x, d.len())
                .and_then(|i| d.get(i))
                .map(|v| format_pct_value(*v as f64)),
            SeriesRef::F64(d) => widgets::sample_for_plot_x(snapped_x, d.len())
                .and_then(|i| d.get(i))
                .map(|v| widgets::format_bps(*v)),
        }
    }
}
