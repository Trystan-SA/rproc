//! Headless background sampler that persists a 60-second rolling
//! window of system metrics so the GUI can show recent history the
//! moment it opens — even after a full restart.
//!
//! Lifecycle:
//! - `rproc --daemon` is the explicit entry point (systemd, manual launch).
//! - `spawn_if_absent()` is what the GUI calls on startup: it forks the
//!   current binary with `--daemon`, detaches it via `setsid(2)`, and
//!   lets the new process orphan-adopt onto PID 1. Closing the GUI then
//!   leaves the daemon untouched.

pub mod pidfile;
pub mod storage;

use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sysinfo::{Disks, MemoryRefreshKind, Networks, System};

use crate::monitor::{gpu, system as msystem};
use storage::{
    DiskSlot, GpuSlot, MAX_DISKS, MAX_GPUS, MAX_NETS, NetSlot, RingBuffer, Sample, name_to_bytes,
};

/// Fixed at 1 s so `CAPACITY = 60` samples means literally the last
/// 60 seconds, regardless of how the GUI is sampling.
const SAMPLE_PERIOD: Duration = Duration::from_secs(1);

pub fn run() -> anyhow::Result<()> {
    let pid_path = pidfile::pid_path()?;
    let _lock = match pidfile::PidFile::acquire(&pid_path)? {
        Some(lock) => lock,
        // Already running — exit silently so duplicate spawns are a no-op.
        None => return Ok(()),
    };

    let hist_path = storage::history_path()?;
    let mut ring = RingBuffer::open_writer(&hist_path)?;

    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut gpu_collector = gpu::GpuCollector::init();

    // sysinfo CPU usage needs two refreshes spaced apart to compute deltas.
    sys.refresh_cpu_usage();
    thread::sleep(Duration::from_millis(250));
    let mut last_refresh = Instant::now();

    loop {
        let now = Instant::now();
        let delta_secs = now.duration_since(last_refresh).as_secs_f64();
        last_refresh = now;

        sys.refresh_cpu_usage();
        sys.refresh_memory_specifics(MemoryRefreshKind::everything());
        nets.refresh(true);
        disks.refresh(true);

        let summary = msystem::SystemSummary::collect(&sys, &nets, &disks, delta_secs);
        let gpus = gpu_collector.sample();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Sort by name so the slot index a given interface/disk lands in
        // stays stable across samples — without this, an interface coming
        // or going would shuffle every other one and corrupt the per-name
        // history the GUI reconstructs on load.
        let mut sorted_nets: Vec<_> = summary.nets.iter().collect();
        sorted_nets.sort_by(|a, b| a.name.cmp(&b.name));
        let mut net_slots = [NetSlot::default(); MAX_NETS];
        for (slot, n) in net_slots.iter_mut().zip(sorted_nets.iter().take(MAX_NETS)) {
            *slot = NetSlot {
                name: name_to_bytes(&n.name),
                rx_bps: n.rx_bps as f32,
                tx_bps: n.tx_bps as f32,
            };
        }

        let mut sorted_disks: Vec<_> = summary.disks.iter().collect();
        sorted_disks.sort_by(|a, b| a.name.cmp(&b.name));
        let mut disk_slots = [DiskSlot::default(); MAX_DISKS];
        for (slot, d) in disk_slots
            .iter_mut()
            .zip(sorted_disks.iter().take(MAX_DISKS))
        {
            *slot = DiskSlot {
                name: name_to_bytes(&d.name),
                read_bps: d.read_bps as f32,
                write_bps: d.write_bps as f32,
            };
        }

        let mut gpu_slots = [GpuSlot::default(); MAX_GPUS];
        for (slot, g) in gpu_slots.iter_mut().zip(gpus.iter().take(MAX_GPUS)) {
            let mem_pct = if g.mem_total > 0 {
                (g.mem_used as f32 / g.mem_total as f32) * 100.0
            } else {
                0.0
            };
            *slot = GpuSlot {
                util_pct: g.util_pct,
                mem_pct,
            };
        }

        let sample = Sample {
            timestamp_secs: timestamp,
            cpu_total: summary.cpu_total,
            ram_used_pct: summary.ram_used_pct,
            nets: net_slots,
            disks: disk_slots,
            gpus: gpu_slots,
        };

        if let Err(e) = ring.append(&sample) {
            eprintln!("rprocd: failed to append sample: {e}");
        }

        let elapsed = now.elapsed();
        if elapsed < SAMPLE_PERIOD {
            thread::sleep(SAMPLE_PERIOD - elapsed);
        }
    }
}

/// Spawn `rproc --daemon` as a detached background process if none is
/// running. Best-effort: any failure is logged to stderr but doesn't
/// block the GUI from starting.
pub fn spawn_if_absent() {
    let pid_path = match pidfile::pid_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rproc: cache dir unavailable, skipping background sampler: {e}");
            return;
        }
    };
    if pidfile::PidFile::is_locked(&pid_path) {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rproc: cannot locate current_exe, skipping background sampler: {e}");
            return;
        }
    };

    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let spawn = unsafe {
        Command::new(exe)
            .arg("--daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Move the child into its own session so it survives the
            // GUI exiting and isn't reached by any SIGHUP propagated
            // from the launching terminal. setsid is async-signal-safe.
            .pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
            .spawn()
    };

    if let Err(e) = spawn {
        eprintln!("rproc: failed to spawn background sampler: {e}");
    }
}

/// Stop a running daemon, if any, by sending SIGTERM to the PID recorded in
/// the pidfile. The daemon installs no signal handler, so the default
/// SIGTERM disposition terminates it and the kernel releases its flock.
/// Best-effort: a missing pidfile or already-dead process is a no-op.
pub fn stop() {
    let pid_path = match pidfile::pid_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    if let Some(pid) = pidfile::PidFile::read_pid(&pid_path) {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}
