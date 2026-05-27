use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
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
    pub processes: Vec<processes::ProcInfo>,
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
}

impl Sampler {
    pub fn start(refresh_ms: Arc<AtomicU64>) -> Self {
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
        thread::Builder::new()
            .name("rproc-sampler".into())
            .spawn(move || sampler_loop(inner_t, refresh_ms))
            .expect("spawn sampler");
        Self { inner }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.inner.lock().unwrap().clone()
    }
}

fn sampler_loop(out: Arc<Mutex<Arc<Snapshot>>>, refresh_ms: Arc<AtomicU64>) {
    let mut sys = System::new_all();
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
        nets.refresh(true);
        disks.refresh(true);
        components.refresh(true);
        users.refresh();

        let summary = system::SystemSummary::collect(&sys, &nets, &disks, &components, delta_secs);
        let procs = processes::collect(&sys, &users);
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
        working.processes = procs;
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

        let elapsed = now.elapsed();
        let target = Duration::from_millis(
            refresh_ms
                .load(Ordering::Relaxed)
                .clamp(MIN_REFRESH_MS, MAX_REFRESH_MS),
        );
        if elapsed < target {
            thread::sleep(target - elapsed);
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
mod tests {
    use super::*;

    #[test]
    fn push_capped_grows_until_cap() {
        let mut q: VecDeque<i32> = VecDeque::new();
        for v in 0..5 {
            push_capped(&mut q, v, 5);
        }
        assert_eq!(q.len(), 5);
        assert_eq!(q.front(), Some(&0));
        assert_eq!(q.back(), Some(&4));
    }

    #[test]
    fn push_capped_drops_oldest_when_full() {
        // Once the cap is reached, the front (oldest) drops on every push so
        // the queue keeps a fixed-size rolling window of the most recent
        // samples — this is the invariant the plots rely on.
        let mut q: VecDeque<i32> = VecDeque::new();
        for v in 0..7 {
            push_capped(&mut q, v, 5);
        }
        assert_eq!(q.len(), 5);
        assert_eq!(q.front(), Some(&2));
        assert_eq!(q.back(), Some(&6));
    }

    #[test]
    fn push_capped_holds_cap_under_burst() {
        // After many pushes the queue size should plateau at exactly `cap`,
        // never grow past it — this is the safety net the plot widgets
        // depend on for their fixed-width X axis.
        let mut q: VecDeque<i32> = VecDeque::new();
        for v in 0..1000 {
            push_capped(&mut q, v, 60);
        }
        assert_eq!(q.len(), 60);
        // And the contents are the last 60 values.
        assert_eq!(q.front(), Some(&940));
        assert_eq!(q.back(), Some(&999));
    }

    #[test]
    fn arc_snapshot_publication_smoke() {
        // We can't easily exercise the sampler thread without running the
        // full pipeline, but we CAN verify the Arc<Snapshot> publication
        // surface keeps the cheap-clone invariant: cloning the published
        // value must return the same pointer rather than reallocating.
        let inner: Arc<Mutex<Arc<Snapshot>>> = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let a = inner.lock().unwrap().clone();
        let b = inner.lock().unwrap().clone();
        assert!(
            Arc::ptr_eq(&a, &b),
            "cloned Arcs must share the same allocation"
        );

        // Swap publishes a new Snapshot — the previous handles keep pointing
        // at the old data, the new lock yields the new one.
        {
            let mut guard = inner.lock().unwrap();
            *guard = Arc::new(Snapshot {
                ready: true,
                ..Snapshot::default()
            });
        }
        let c = inner.lock().unwrap().clone();
        assert!(c.ready);
        assert!(!a.ready);
        assert!(!Arc::ptr_eq(&a, &c));
    }
}
