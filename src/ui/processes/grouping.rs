use std::collections::{BTreeMap, HashSet};

use crate::monitor;

use super::sort::SortKey;

pub(super) struct Group<'a> {
    pub(super) name: &'a str,
    pub(super) procs: Vec<&'a monitor::processes::ProcInfo>,
    pub(super) cpu_pct: f32,
    pub(super) mem_bytes: u64,
    pub(super) disk_bps: f64,
}

/// Group processes by name and roll up CPU / memory / disk totals. Keying on
/// `&str` (borrowed from `procs`) skips ~N `String` allocations per frame,
/// where N is the number of processes. The `matches` predicate filters before
/// grouping so empty groups don't appear in the result.
pub(super) fn build_groups<'a>(
    procs: &'a [monitor::processes::ProcInfo],
    matches: &dyn Fn(&monitor::processes::ProcInfo) -> bool,
) -> Vec<Group<'a>> {
    let mut by_name: BTreeMap<&str, Vec<&monitor::processes::ProcInfo>> = BTreeMap::new();
    for p in procs {
        if matches(p) {
            by_name.entry(p.name.as_str()).or_default().push(p);
        }
    }
    by_name
        .into_iter()
        .map(|(name, procs)| {
            let cpu_pct = procs.iter().map(|p| p.cpu_pct).sum();
            let mem_bytes = procs.iter().map(|p| p.mem_bytes).sum();
            let disk_bps = procs
                .iter()
                .map(|p| p.disk_read_bps + p.disk_write_bps)
                .sum();
            Group {
                name,
                procs,
                cpu_pct,
                mem_bytes,
                disk_bps,
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(super) enum Row<'a> {
    SectionHeader(&'static str),
    GroupHeader { g: &'a Group<'a>, expanded: bool },
    Single(&'a monitor::processes::ProcInfo),
    Child(&'a monitor::processes::ProcInfo),
}

/// Split the built groups into the "Apps" section (processes with a freedesktop
/// `.desktop` entry — launchable applications) and the background section
/// (daemons, kernel threads, helpers). Each section is sorted independently by
/// the caller so background processes can never rank above apps.
pub(super) fn append_section<'a>(
    visible: &mut Vec<Row<'a>>,
    title: &'static str,
    groups: &'a [Group<'a>],
    filter_active: bool,
    expanded_set: &HashSet<String>,
) {
    if groups.is_empty() {
        return;
    }
    visible.push(Row::SectionHeader(title));
    for g in groups {
        if g.procs.len() == 1 {
            visible.push(Row::Single(g.procs[0]));
        } else {
            let expanded = filter_active || expanded_set.contains(g.name);
            visible.push(Row::GroupHeader { g, expanded });
            if expanded {
                for p in &g.procs {
                    visible.push(Row::Child(p));
                }
            }
        }
    }
}

pub(super) fn sort_groups(groups: &mut [Group], key: SortKey, desc: bool) {
    groups.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Pid => a
                .procs
                .iter()
                .map(|p| p.pid)
                .min()
                .cmp(&b.procs.iter().map(|p| p.pid).min()),
            SortKey::User => a
                .procs
                .first()
                .map(|p| p.user.as_str())
                .cmp(&b.procs.first().map(|p| p.user.as_str())),
            SortKey::Cpu => a
                .cpu_pct
                .partial_cmp(&b.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortKey::Mem => a.mem_bytes.cmp(&b.mem_bytes),
            SortKey::Disk => a
                .disk_bps
                .partial_cmp(&b.disk_bps)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortKey::Status => a
                .procs
                .first()
                .map(|p| p.status.as_str())
                .cmp(&b.procs.first().map(|p| p.status.as_str())),
        };
        if desc { ord.reverse() } else { ord }
    });
}

pub(super) fn sort_children(rows: &mut [&monitor::processes::ProcInfo], key: SortKey, desc: bool) {
    rows.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::User => a.user.cmp(&b.user),
            SortKey::Cpu => a
                .cpu_pct
                .partial_cmp(&b.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortKey::Mem => a.mem_bytes.cmp(&b.mem_bytes),
            SortKey::Disk => {
                let ax = a.disk_read_bps + a.disk_write_bps;
                let bx = b.disk_read_bps + b.disk_write_bps;
                ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
            }
            SortKey::Status => a.status.cmp(&b.status),
        };
        if desc { ord.reverse() } else { ord }
    });
}

#[cfg(test)]
#[path = "grouping_tests.rs"]
mod tests;
