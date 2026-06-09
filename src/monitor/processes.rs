use sysinfo::{System, Users};

#[derive(Clone, Default)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub exe: String,
    pub cmd: String,
    pub user: String,
    pub cpu_pct: f32,
    pub mem_bytes: u64,
    pub virt_bytes: u64,
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    pub status: String,
    pub threads: usize,
    pub start_time: u64,
    /// freedesktop app id from the process's systemd cgroup when it was launched
    /// as an application (see [`super::cgroup`]); drives the Apps/Background split.
    pub app_id: Option<String>,
}

/// Map sysinfo's instantaneous status to a more meaningful label for the UI.
///
/// On Linux, most user processes are caught in "Sleeping" between work bursts
/// — labeling them as sleep is noisy. Any process with non-zero CPU usage over
/// the sampling window is reported as Running. Sleep-like idle states collapse
/// to a single "Idle". Exceptional states (Stopped, Zombie, etc.) pass through.
fn derive_status(raw: &str, cpu_pct: f32) -> String {
    if cpu_pct > 0.0 {
        return "Running".to_string();
    }
    match raw {
        "Run" | "Running" => "Running",
        "Sleep" | "Sleeping" | "Idle" | "Parked" | "Wakekill" | "Waking" => "Idle",
        "UninterruptibleDiskSleep" | "LockBlocked" => "Waiting",
        "Stop" | "Stopped" | "Tracing" => "Stopped",
        "Zombie" | "Dead" => "Zombie",
        other => other,
    }
    .to_string()
}

pub fn collect(sys: &System, users: &Users) -> Vec<ProcInfo> {
    // sysinfo reports per-process CPU as a percentage of a SINGLE core (a
    // process pinning two cores reads as ~200%), while global CPU is averaged
    // over all logical cores (capped at 100%). Normalize per-process by the
    // core count so both scales agree — otherwise a child looks louder than
    // the whole system on multi-core machines.
    let cores = sys.cpus().len().max(1) as f32;
    let mut list = Vec::with_capacity(sys.processes().len());
    for (pid, p) in sys.processes() {
        // On Linux, sysinfo reports each thread as a separate entry. Threads share
        // their parent's RSS, so sorting by memory shows N duplicate rows at the
        // same value (e.g. 50× "Slack" at 2.0 GB, all Sleeping). Skip them.
        if p.thread_kind().is_some() {
            continue;
        }
        let user = p
            .user_id()
            .and_then(|uid| users.list().iter().find(|u| u.id() == uid))
            .map(|u| u.name().to_string())
            .unwrap_or_default();
        let usage = p.disk_usage();
        // /proc/<pid>/cmdline is NUL-separated and often carries empty trailing
        // fields; dropping them avoids stray double/trailing spaces in the join.
        let cmd_vec: Vec<String> = p
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let cpu_pct = p.cpu_usage() / cores;
        let raw_status = format!("{:?}", p.status());
        let status = derive_status(&raw_status, cpu_pct);
        let app_id = std::fs::read_to_string(format!("/proc/{}/cgroup", pid.as_u32()))
            .ok()
            .and_then(|c| super::cgroup::app_id(&c));
        list.push(ProcInfo {
            pid: pid.as_u32(),
            parent: p.parent().map(|p| p.as_u32()),
            name: p.name().to_string_lossy().into_owned(),
            exe: p
                .exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            cmd: cmd_vec.join(" "),
            user,
            cpu_pct,
            mem_bytes: p.memory(),
            virt_bytes: p.virtual_memory(),
            disk_read_bps: usage.read_bytes as f64,
            disk_write_bps: usage.written_bytes as f64,
            status,
            threads: p.tasks().map(|t| t.len()).unwrap_or(0),
            start_time: p.start_time(),
            app_id,
        });
    }
    list
}

pub fn terminate(pid: u32) -> bool {
    // SIGTERM first; UI can offer a follow-up "Force kill" → SIGKILL.
    std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn force_kill(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn suspend(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-STOP", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn resume(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-CONT", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn read_cwd(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn read_fd_count(pid: u32) -> Option<usize> {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .ok()
        .map(|it| it.count())
}

// Best-effort lookup of well-known config locations on Linux:
//   * $XDG_CONFIG_HOME/<name> (defaults to ~/.config/<name>)
//   * ~/.<name>               (legacy dotdir convention)
//   * /etc/<name>             (system-wide config dir)
//   * /etc/<name>.conf        (system-wide config file)
// Tries both the process `name` and the exe basename, plus a Capitalized
// variant (so "code" finds `~/.config/Code`). Returns only existing paths.
pub fn find_config_paths(name: &str, exe: &str) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;

    let exe_base = std::path::Path::new(exe)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let mut candidate_names: Vec<String> = Vec::new();
    for n in [name, exe_base.as_str()] {
        let n = n.trim();
        if n.is_empty() {
            continue;
        }
        candidate_names.push(n.to_string());
        candidate_names.push(n.to_lowercase());
        let mut chars = n.chars();
        if let Some(first) = chars.next() {
            let capitalized: String = first.to_uppercase().chain(chars).collect();
            candidate_names.push(capitalized);
        }
    }
    candidate_names.sort();
    candidate_names.dedup();

    let xdg_config: PathBuf = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    let home: Option<PathBuf> = std::env::var_os("HOME").map(PathBuf::from);

    let mut out: Vec<PathBuf> = Vec::new();
    for cn in &candidate_names {
        let mut try_push = |p: PathBuf| {
            if p.exists() && !out.contains(&p) {
                out.push(p);
            }
        };
        if !xdg_config.as_os_str().is_empty() {
            try_push(xdg_config.join(cn));
        }
        if let Some(h) = home.as_ref() {
            try_push(h.join(format!(".{cn}")));
        }
        try_push(PathBuf::from(format!("/etc/{cn}")));
        try_push(PathBuf::from(format!("/etc/{cn}.conf")));
    }
    out
}

#[cfg(test)]
#[path = "processes_tests.rs"]
mod tests;
