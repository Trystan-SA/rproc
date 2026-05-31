use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sysinfo::{
    Components, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate, System,
    Users,
};

use super::{gpu, processes, system};
use crate::settings::{MAX_REFRESH_MS, MIN_REFRESH_MS};

const HISTORY_LEN: usize = 60;

#[derive(Clone, Default)]
pub struct Snapshot {
    pub ready: bool,
    /// Sampling period (ms) used when collecting the most recent samples.
    /// Consumed by plot widgets to render the X-axis time labels — without
    /// this the labels would always read "-Xs" assuming a 1 s tick, which
    /// is wrong when the user picks a faster/slower refresh rate.
    pub sample_interval_ms: u64,
    pub system: system::SystemSummary,
    pub history: History,
    pub processes: Arc<Vec<processes::ProcInfo>>,
    pub gpus: Vec<gpu::GpuInfo>,
}

#[derive(Clone, Default)]
pub struct History {
    pub cpu_total: VecDeque<f32>,
    pub per_core_cpu: Vec<VecDeque<f32>>,
    pub ram_used_pct: VecDeque<f32>,
    /// Per-interface throughput history, keyed by interface name
    /// (e.g. `wlp4s0`, `enp0s31f6`).
    pub net_rx_bps: HashMap<String, VecDeque<f64>>,
    pub net_tx_bps: HashMap<String, VecDeque<f64>>,
    /// Per-physical-disk history, keyed by device path (e.g. `/dev/nvme0n1`).
    pub disk_read_bps: HashMap<String, VecDeque<f64>>,
    pub disk_write_bps: HashMap<String, VecDeque<f64>>,
    pub gpu_util: Vec<VecDeque<f32>>,
    pub gpu_mem_pct: Vec<VecDeque<f32>>,
}

pub struct Sampler {
    // Published snapshot. We wrap a Mutex around `Arc<Snapshot>` instead of
    // `Snapshot` so the UI thread only clones the Arc pointer per frame (cheap)
    // rather than every `ProcInfo`/`History` field (~hundreds of allocations).
    // The sampler thread builds its working copy locally and only takes the
    // mutex briefly at the end of each tick to swap the published Arc.
    inner: Arc<Mutex<Arc<Snapshot>>>,
    // Set by the UI each frame: true only while the Processes tab is showing.
    // The full process table (name + exe + full cmdline + user, ×N hundred
    // PIDs) is only built while it's actually on screen — the app opens on the
    // Performance tab, so at startup this table is never collected at all.
    processes_active: Arc<AtomicBool>,
}

impl Sampler {
    pub fn start(refresh_ms: Arc<AtomicU64>, ctx: egui::Context) -> Self {
        // Default off: the app opens on Performance, so the process table stays
        // empty until the user first visits the Processes tab.
        let processes_active = Arc::new(AtomicBool::new(false));
        // Pre-fill the rolling history from the daemon's on-disk ring-buffer
        // so the user sees up to the last 60 s of activity as soon as the
        // window opens — even if rproc was just relaunched. CPU per-core
        // is the one detail series we don't persist (8–16× the storage
        // cost for little user value); it stays empty until the GUI samples.
        let mut initial = Snapshot::default();
        if let Ok(path) = crate::daemon::storage::history_path()
            && let Ok(samples) = crate::daemon::storage::RingBuffer::read_all(&path)
            && !samples.is_empty()
        {
            prefill_history(&mut initial.history, &samples);
        }
        let inner = Arc::new(Mutex::new(Arc::new(initial)));

        let inner_t = inner.clone();
        let active_t = processes_active.clone();
        thread::Builder::new()
            .name("rproc-sampler".into())
            .spawn(move || sampler_loop(inner_t, refresh_ms, active_t, ctx))
            .expect("spawn sampler");
        Self {
            inner,
            processes_active,
        }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.inner.lock().unwrap().clone()
    }

    /// Tell the sampler whether the process table is currently on screen.
    /// When false, the next tick skips the per-PID refresh; the last collected
    /// list is retained so reopening the tab shows it immediately.
    pub fn set_processes_active(&self, on: bool) {
        self.processes_active.store(on, Ordering::Relaxed);
    }
}

fn sampler_loop(
    out: Arc<Mutex<Arc<Snapshot>>>,
    refresh_ms: Arc<AtomicU64>,
    processes_active: Arc<AtomicBool>,
    ctx: egui::Context,
) {
    // `System::new()` + a CPU refresh avoids `new_all()`'s upfront scan of
    // every PID's cmdline/exe/environ. The loop below repopulates the process
    // table each tick with a narrow `ProcessRefreshKind` (no environ), so the
    // full initial read was pure waste. The CPU refresh here is just to size
    // `per_core_cpu` correctly below; the delta-bearing refresh stays further down.
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    let mut nets = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut components = Components::new_with_refreshed_list();
    let mut users = Users::new_with_refreshed_list();
    let mut gpu_collector = gpu::GpuCollector::init();

    // Start from whatever's been published (the prefill from disk) so we
    // don't drop the history we just loaded. After this point the working
    // copy is owned exclusively by this thread — no lock needed to mutate.
    let mut working: Snapshot = (**out.lock().unwrap()).clone();
    working.history.per_core_cpu = (0..sys.cpus().len())
        .map(|_| VecDeque::with_capacity(HISTORY_LEN))
        .collect();

    // sysinfo CPU usage requires two refreshes spaced apart to compute deltas.
    sys.refresh_cpu_usage();
    thread::sleep(Duration::from_millis(250));

    // Track the wall-clock interval between consecutive sysinfo refreshes.
    // We pass this to `SystemSummary::collect` so byte counters get normalized
    // into bytes/sec regardless of the configured sampling rate.
    let mut last_refresh = Instant::now();

    loop {
        let now = Instant::now();
        let delta_secs = now.duration_since(last_refresh).as_secs_f64();
        last_refresh = now;

        sys.refresh_cpu_usage();
        sys.refresh_memory_specifics(MemoryRefreshKind::everything());

        // Only walk the process table while the Processes tab is on screen.
        // Skipping it leaves sysinfo's per-PID map empty (we start from
        // `System::new()`), so the cmdline/exe/user strings for hundreds of
        // processes are never allocated until the user actually asks to see them.
        // When hidden we keep the previous list so the tab paints it on reopen.
        let want_procs = processes_active.load(Ordering::Relaxed);
        if want_procs {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing()
                    .with_cpu()
                    .with_memory()
                    .with_disk_usage()
                    .with_user(sysinfo::UpdateKind::OnlyIfNotSet)
                    .with_cmd(sysinfo::UpdateKind::OnlyIfNotSet)
                    .with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
            );
            users.refresh();
            working.processes = Arc::new(processes::collect(&sys, &users));
        }

        nets.refresh(true);
        disks.refresh(true);
        components.refresh(true);

        let summary = system::SystemSummary::collect(&sys, &nets, &disks, &components, delta_secs);
        let gpus = gpu_collector.sample();

        push_capped(
            &mut working.history.cpu_total,
            summary.cpu_total,
            HISTORY_LEN,
        );
        if working.history.per_core_cpu.len() != summary.per_core.len() {
            working.history.per_core_cpu = (0..summary.per_core.len())
                .map(|_| VecDeque::with_capacity(HISTORY_LEN))
                .collect();
        }
        for (i, c) in summary.per_core.iter().enumerate() {
            if let Some(q) = working.history.per_core_cpu.get_mut(i) {
                push_capped(q, *c, HISTORY_LEN);
            }
        }
        push_capped(
            &mut working.history.ram_used_pct,
            summary.ram_used_pct,
            HISTORY_LEN,
        );
        let mut net_present: HashSet<String> = HashSet::with_capacity(summary.nets.len());
        for n in &summary.nets {
            net_present.insert(n.name.clone());
            let r = working
                .history
                .net_rx_bps
                .entry(n.name.clone())
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(r, n.rx_bps, HISTORY_LEN);
            let w = working
                .history
                .net_tx_bps
                .entry(n.name.clone())
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(w, n.tx_bps, HISTORY_LEN);
        }
        working
            .history
            .net_rx_bps
            .retain(|k, _| net_present.contains(k));
        working
            .history
            .net_tx_bps
            .retain(|k, _| net_present.contains(k));

        let mut present: HashSet<String> = HashSet::with_capacity(summary.disks.len());
        for d in &summary.disks {
            present.insert(d.name.clone());
            let r = working
                .history
                .disk_read_bps
                .entry(d.name.clone())
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(r, d.read_bps, HISTORY_LEN);
            let w = working
                .history
                .disk_write_bps
                .entry(d.name.clone())
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(w, d.write_bps, HISTORY_LEN);
        }
        working
            .history
            .disk_read_bps
            .retain(|k, _| present.contains(k));
        working
            .history
            .disk_write_bps
            .retain(|k, _| present.contains(k));

        if working.history.gpu_util.len() != gpus.len() {
            working.history.gpu_util = (0..gpus.len())
                .map(|_| VecDeque::with_capacity(HISTORY_LEN))
                .collect();
            working.history.gpu_mem_pct = (0..gpus.len())
                .map(|_| VecDeque::with_capacity(HISTORY_LEN))
                .collect();
        }
        for (i, g) in gpus.iter().enumerate() {
            if let Some(q) = working.history.gpu_util.get_mut(i) {
                push_capped(q, g.util_pct, HISTORY_LEN);
            }
            let mem_pct = if g.mem_total > 0 {
                (g.mem_used as f32 / g.mem_total as f32) * 100.0
            } else {
                0.0
            };
            if let Some(q) = working.history.gpu_mem_pct.get_mut(i) {
                push_capped(q, mem_pct, HISTORY_LEN);
            }
        }

        working.system = summary;
        working.gpus = gpus;
        working.ready = true;
        // Surface the *current* sampling period so plot widgets can label
        // their X axis correctly. Read before the sleep so it reflects the
        // interval just used to space these samples.
        working.sample_interval_ms = refresh_ms
            .load(Ordering::Relaxed)
            .clamp(MIN_REFRESH_MS, MAX_REFRESH_MS);

        // Publish: one Snapshot clone per tick (≈1 Hz at default settings)
        // instead of one per UI frame.
        *out.lock().unwrap() = Arc::new(working.clone());
        // Wake the UI now rather than at its next interval-tied repaint, so a
        // just-collected process list shows immediately after the tab opens.
        ctx.request_repaint();

        let elapsed = now.elapsed();
        let target = Duration::from_millis(
            refresh_ms
                .load(Ordering::Relaxed)
                .clamp(MIN_REFRESH_MS, MAX_REFRESH_MS),
        );
        if elapsed < target {
            let remaining = target - elapsed;
            // If we just published a process list, nothing's waiting on us —
            // sleep the whole interval. Otherwise (Processes tab not showing)
            // poll in short slices so opening the tab mid-sleep wakes us within
            // ~one slice instead of up to a full refresh interval (1 s default).
            // The process scan itself is only a few ms; the felt latency was
            // purely this sleep. Polling a relaxed atomic every 40 ms is free.
            if want_procs {
                thread::sleep(remaining);
            } else {
                let slice = Duration::from_millis(40);
                let mut slept = Duration::ZERO;
                while slept < remaining && !processes_active.load(Ordering::Relaxed) {
                    let nap = slice.min(remaining - slept);
                    thread::sleep(nap);
                    slept += nap;
                }
            }
        }
    }
}

fn push_capped<T>(q: &mut VecDeque<T>, v: T, cap: usize) {
    if q.len() == cap {
        q.pop_front();
    }
    q.push_back(v);
}

/// Replay persisted samples into the live history buffers. Each network
/// and disk slot carries its own name, so per-interface / per-device
/// series are reconstructed in the same `HashMap<String, _>` shape the
/// live sampler maintains — including the case where an interface only
/// shows up in part of the window (its `VecDeque` will simply be shorter).
fn prefill_history(history: &mut History, samples: &[crate::daemon::storage::Sample]) {
    use crate::daemon::storage::name_from_bytes;

    for s in samples {
        push_capped(&mut history.cpu_total, s.cpu_total, HISTORY_LEN);
        push_capped(&mut history.ram_used_pct, s.ram_used_pct, HISTORY_LEN);

        for slot in &s.nets {
            let name = name_from_bytes(&slot.name);
            if name.is_empty() {
                continue;
            }
            let r = history
                .net_rx_bps
                .entry(name.clone())
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(r, slot.rx_bps as f64, HISTORY_LEN);
            let w = history
                .net_tx_bps
                .entry(name)
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(w, slot.tx_bps as f64, HISTORY_LEN);
        }

        for slot in &s.disks {
            let name = name_from_bytes(&slot.name);
            if name.is_empty() {
                continue;
            }
            let r = history
                .disk_read_bps
                .entry(name.clone())
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(r, slot.read_bps as f64, HISTORY_LEN);
            let w = history
                .disk_write_bps
                .entry(name)
                .or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            push_capped(w, slot.write_bps as f64, HISTORY_LEN);
        }

        for (idx, slot) in s.gpus.iter().enumerate() {
            // NaN sentinel marks an unused slot — skip without growing the
            // gpu_util / gpu_mem_pct vectors beyond the real device count.
            if slot.util_pct.is_nan() {
                continue;
            }
            while history.gpu_util.len() <= idx {
                history.gpu_util.push(VecDeque::with_capacity(HISTORY_LEN));
            }
            while history.gpu_mem_pct.len() <= idx {
                history
                    .gpu_mem_pct
                    .push(VecDeque::with_capacity(HISTORY_LEN));
            }
            push_capped(&mut history.gpu_util[idx], slot.util_pct, HISTORY_LEN);
            push_capped(&mut history.gpu_mem_pct[idx], slot.mem_pct, HISTORY_LEN);
        }
    }
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
