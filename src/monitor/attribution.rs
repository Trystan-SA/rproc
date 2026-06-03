//! Optional per-process attribution for the Performance graphs.
//!
//! When enabled, the sampler captures — for each history sample — the few
//! heaviest processes per resource (CPU / RAM / disk), so hovering a point on a
//! graph can answer "who was responsible at that moment". This is deliberately
//! self-contained and gated behind a setting: when the feature is off, none of
//! this runs and the core sampler keeps its lean per-tick cost.
//!
//! Network is intentionally excluded — sysinfo exposes no per-process network
//! accounting, so there is nothing to attribute. GPU is excluded too: reliable
//! per-process GPU usage isn't available across vendors.
//!
//! Note this lives only in the live in-memory history; it is never persisted to
//! the on-disk ring buffer, so the background daemon stays untouched and free.

use sysinfo::System;

/// How many heaviest processes we keep per resource, per sample.
pub const TOP_N: usize = 5;

/// One process's share of a single resource at one sample. `value` is the
/// quantity we rank by, its unit depending on the list: CPU and RAM are
/// percentages, disk is bytes/sec. `bytes` carries the absolute size when one
/// is meaningful (RAM: resident memory) so the UI can show e.g. "1.2 GB (5%)";
/// it is 0 for CPU and disk where no absolute size applies.
#[derive(Clone)]
pub struct ProcShare {
    pub pid: u32,
    pub name: String,
    pub value: f32,
    pub bytes: u64,
}

/// Top-N heaviest processes per resource captured for one history sample.
/// An empty vector means nothing was using that resource at the time (or that
/// attribution was off when the sample was taken).
#[derive(Clone, Default)]
pub struct Attribution {
    pub cpu: Vec<ProcShare>,
    pub ram: Vec<ProcShare>,
    pub disk: Vec<ProcShare>,
    /// Per-process GPU utilization (%). Filled separately by
    /// [`super::gpu_attribution`] since it needs vendor-specific, stateful
    /// sources; empty when no GPU source is available.
    pub gpu: Vec<ProcShare>,
}

/// Compute the per-resource top-N directly from an already-refreshed `System`,
/// without building the full `ProcInfo` table the Processes tab uses.
///
/// `delta_secs` normalizes disk byte counters (sysinfo reports them as bytes
/// since the previous refresh) into bytes/sec, matching the disk graph's units.
/// Process names are captured here, so a process that later exits still shows up
/// in the historical sample that recorded it.
///
/// The caller is responsible for having refreshed `sys` with CPU, memory and
/// disk-usage process data before calling this.
pub fn collect(sys: &System, delta_secs: f64) -> Attribution {
    // sysinfo reports per-process CPU as a percentage of a single core; the
    // graphs show CPU averaged across all logical cores. Normalize so the
    // attribution scale matches the curve the user is hovering.
    let cores = sys.cpus().len().max(1) as f32;
    let ram_total = sys.total_memory();
    let dt = delta_secs.max(1e-3);

    let mut cpu: Vec<ProcShare> = Vec::new();
    let mut ram: Vec<ProcShare> = Vec::new();
    let mut disk: Vec<ProcShare> = Vec::new();

    for (pid, p) in sys.processes() {
        // On Linux sysinfo lists each thread as its own entry sharing the
        // parent's RSS; skip them so memory isn't counted many times over.
        if p.thread_kind().is_some() {
            continue;
        }
        let pid = pid.as_u32();
        let name = p.name().to_string_lossy();

        // Every process is a candidate (including idle ones) so the lists
        // always fill to TOP_N; the ranking decides what surfaces.
        let cpu_pct = p.cpu_usage() / cores;
        cpu.push(ProcShare {
            pid,
            name: name.clone().into_owned(),
            value: cpu_pct,
            bytes: 0,
        });

        let mem = p.memory();
        let mem_pct = if ram_total > 0 {
            (mem as f32 / ram_total as f32) * 100.0
        } else {
            0.0
        };
        ram.push(ProcShare {
            pid,
            name: name.clone().into_owned(),
            value: mem_pct,
            bytes: mem,
        });

        let io = p.disk_usage();
        let bps = (io.read_bytes + io.written_bytes) as f64 / dt;
        disk.push(ProcShare {
            pid,
            name: name.into_owned(),
            value: bps as f32,
            bytes: 0,
        });
    }

    Attribution {
        cpu: top_n(cpu),
        ram: top_n(ram),
        disk: top_n(disk),
        // Filled by the caller via gpu_attribution::sample — needs stateful,
        // vendor-specific sources this sysinfo-only pass doesn't have.
        gpu: Vec::new(),
    }
}

/// Keep the `TOP_N` highest-value shares, descending. Every process is a
/// candidate, so on any real system this always yields a full `TOP_N` rows
/// (a future threshold setting could prune low contributors here instead).
fn top_n(mut v: Vec<ProcShare>) -> Vec<ProcShare> {
    v.sort_unstable_by(|a, b| {
        b.value
            .partial_cmp(&a.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(TOP_N);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn share(pid: u32, value: f32) -> ProcShare {
        ProcShare {
            pid,
            name: format!("p{pid}"),
            value,
            bytes: 0,
        }
    }

    #[test]
    fn top_n_keeps_highest_descending() {
        let v = vec![
            share(1, 3.0),
            share(2, 10.0),
            share(3, 1.0),
            share(4, 7.0),
            share(5, 5.0),
            share(6, 9.0),
        ];
        let out = top_n(v);
        assert_eq!(out.len(), TOP_N);
        let values: Vec<f32> = out.iter().map(|s| s.value).collect();
        assert_eq!(values, vec![10.0, 9.0, 7.0, 5.0, 3.0]);
    }

    #[test]
    fn top_n_shorter_than_cap_is_left_sorted() {
        let out = top_n(vec![share(1, 2.0), share(2, 8.0)]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].value, 8.0);
        assert_eq!(out[1].value, 2.0);
    }

    #[test]
    fn top_n_handles_nan_without_panicking() {
        // partial_cmp returns None for NaN; the fallback ordering must keep
        // sort_unstable_by from panicking on a bad comparator.
        let out = top_n(vec![share(1, f32::NAN), share(2, 4.0), share(3, 1.0)]);
        assert_eq!(out.len(), 3);
    }
}
