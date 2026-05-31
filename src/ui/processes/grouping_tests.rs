use super::*;

fn make_proc(
    pid: u32,
    name: &str,
    cpu_pct: f32,
    mem_bytes: u64,
    disk: f64,
    user: &str,
    status: &str,
) -> monitor::processes::ProcInfo {
    monitor::processes::ProcInfo {
        pid,
        name: name.to_string(),
        cpu_pct,
        mem_bytes,
        disk_read_bps: disk / 2.0,
        disk_write_bps: disk / 2.0,
        user: user.to_string(),
        status: status.to_string(),
        ..Default::default()
    }
}

#[test]
fn build_groups_aggregates_processes_with_same_name() {
    let procs = vec![
        make_proc(1, "chrome", 1.0, 100, 10.0, "alice", "Running"),
        make_proc(2, "chrome", 2.0, 200, 20.0, "alice", "Running"),
        make_proc(3, "firefox", 5.0, 500, 30.0, "alice", "Running"),
    ];
    let groups = build_groups(&procs, &|_| true);
    assert_eq!(groups.len(), 2);

    let chrome = groups.iter().find(|g| g.name == "chrome").unwrap();
    assert_eq!(chrome.procs.len(), 2);
    assert!((chrome.cpu_pct - 3.0).abs() < 1e-6);
    assert_eq!(chrome.mem_bytes, 300);
    assert!((chrome.disk_bps - 30.0).abs() < 1e-6);

    let firefox = groups.iter().find(|g| g.name == "firefox").unwrap();
    assert_eq!(firefox.procs.len(), 1);
}

#[test]
fn build_groups_respects_filter_predicate() {
    let procs = vec![
        make_proc(1, "chrome", 1.0, 100, 0.0, "alice", "Running"),
        make_proc(2, "firefox", 2.0, 200, 0.0, "alice", "Running"),
        make_proc(3, "vim", 3.0, 50, 0.0, "alice", "Running"),
    ];
    let groups = build_groups(&procs, &|p| p.name.starts_with('f'));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "firefox");
}

#[test]
fn build_groups_returns_groups_alphabetically() {
    // BTreeMap-backed: alphabetical by name is the expected pre-sort
    // order before `sort_groups` reorders by the user's chosen column.
    let procs = vec![
        make_proc(1, "zsh", 1.0, 0, 0.0, "u", "Running"),
        make_proc(2, "alpha", 1.0, 0, 0.0, "u", "Running"),
        make_proc(3, "mid", 1.0, 0, 0.0, "u", "Running"),
    ];
    let groups = build_groups(&procs, &|_| true);
    let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
    assert_eq!(names, vec!["alpha", "mid", "zsh"]);
}

#[test]
fn build_groups_empty_input_yields_empty() {
    let procs: Vec<monitor::processes::ProcInfo> = vec![];
    let groups = build_groups(&procs, &|_| true);
    assert!(groups.is_empty());
}

#[test]
fn sort_groups_cpu_descending_largest_first() {
    let procs = vec![
        make_proc(1, "low", 1.0, 0, 0.0, "u", "Running"),
        make_proc(2, "high", 50.0, 0, 0.0, "u", "Running"),
        make_proc(3, "mid", 10.0, 0, 0.0, "u", "Running"),
    ];
    let mut groups = build_groups(&procs, &|_| true);
    sort_groups(&mut groups, SortKey::Cpu, true);
    let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
    assert_eq!(names, vec!["high", "mid", "low"]);
}

#[test]
fn sort_groups_mem_ascending_smallest_first() {
    let procs = vec![
        make_proc(1, "big", 0.0, 1000, 0.0, "u", "Running"),
        make_proc(2, "small", 0.0, 10, 0.0, "u", "Running"),
        make_proc(3, "mid", 0.0, 100, 0.0, "u", "Running"),
    ];
    let mut groups = build_groups(&procs, &|_| true);
    sort_groups(&mut groups, SortKey::Mem, false);
    let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
    assert_eq!(names, vec!["small", "mid", "big"]);
}

#[test]
fn sort_groups_pid_uses_minimum_pid() {
    // For grouped processes the sort key on PID is the *lowest* PID in
    // the group — that's the closest thing to a stable "parent" rank.
    let procs = vec![
        make_proc(50, "a", 0.0, 0, 0.0, "u", "Running"),
        make_proc(10, "a", 0.0, 0, 0.0, "u", "Running"),
        make_proc(30, "b", 0.0, 0, 0.0, "u", "Running"),
    ];
    let mut groups = build_groups(&procs, &|_| true);
    sort_groups(&mut groups, SortKey::Pid, false);
    let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
    // group "a" has min PID 10, group "b" has min PID 30.
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn sort_children_pid_ascending() {
    let procs = [
        make_proc(3, "x", 0.0, 0, 0.0, "u", "Running"),
        make_proc(1, "x", 0.0, 0, 0.0, "u", "Running"),
        make_proc(2, "x", 0.0, 0, 0.0, "u", "Running"),
    ];
    let mut refs: Vec<&monitor::processes::ProcInfo> = procs.iter().collect();
    sort_children(&mut refs, SortKey::Pid, false);
    let pids: Vec<u32> = refs.iter().map(|p| p.pid).collect();
    assert_eq!(pids, vec![1, 2, 3]);
}
