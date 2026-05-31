use super::*;

#[test]
fn sort_key_roundtrip_all_variants() {
    // Every variant must roundtrip through the on-disk encoding,
    // otherwise saving today's sort silently resets to the default
    // tomorrow.
    for k in [
        SortKey::Name,
        SortKey::Pid,
        SortKey::User,
        SortKey::Cpu,
        SortKey::Mem,
        SortKey::Disk,
        SortKey::Status,
    ] {
        let s = k.as_str();
        assert_eq!(SortKey::from_str(s), Some(k), "roundtrip for {s}");
    }
}

#[test]
fn sort_key_from_str_rejects_unknown() {
    assert_eq!(SortKey::from_str(""), None);
    assert_eq!(SortKey::from_str("not-a-key"), None);
    // Case-sensitive — we control both sides of the format.
    assert_eq!(SortKey::from_str("cpu"), None);
}

#[test]
fn sort_prefs_roundtrip_via_file() {
    let dir = std::env::temp_dir().join(format!(
        "rproc-sort-prefs-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prefs.txt");
    save_sort_prefs_to(&path, SortKey::Mem, false).unwrap();
    let loaded = load_sort_prefs_from(&path);
    assert_eq!(loaded, Some((SortKey::Mem, false)));

    save_sort_prefs_to(&path, SortKey::Cpu, true).unwrap();
    assert_eq!(load_sort_prefs_from(&path), Some((SortKey::Cpu, true)));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sort_prefs_missing_file_returns_none() {
    let bogus = std::path::Path::new("/nonexistent/rproc/prefs.txt.does-not-exist");
    assert_eq!(load_sort_prefs_from(bogus), None);
}

#[test]
fn sort_prefs_partial_file_returns_none() {
    // We refuse to apply a half-saved file rather than silently
    // defaulting one half — the user would never notice.
    let dir = std::env::temp_dir().join(format!(
        "rproc-sort-partial-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("partial.txt");
    std::fs::write(&path, "sort=Cpu\n").unwrap();
    assert_eq!(load_sort_prefs_from(&path), None);

    std::fs::write(&path, "descending=true\n").unwrap();
    assert_eq!(load_sort_prefs_from(&path), None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn format_pct_low_values_show_decimal() {
    // Below 0.05 collapses to 0% so the column doesn't churn between
    // 0.0% / 0.1% for idle rows on every frame.
    assert_eq!(format_pct(0.0), "0%");
    assert_eq!(format_pct(0.04), "0%");
    assert_eq!(format_pct(0.5), "0.5%");
    assert_eq!(format_pct(9.9), "9.9%");
}

#[test]
fn format_pct_high_values_round_to_int() {
    assert_eq!(format_pct(10.0), "10%");
    assert_eq!(format_pct(42.49), "42%");
    assert_eq!(format_pct(100.0), "100%");
}

#[test]
fn url_encode_passes_through_unreserved() {
    // RFC 3986 unreserved set: A-Z a-z 0-9 - _ . ~
    assert_eq!(url_encode("Hello-World_1.2~3"), "Hello-World_1.2~3");
}

#[test]
fn url_encode_spaces_become_plus() {
    // We're building a Google query URL — spaces are application/x-www-form-urlencoded.
    assert_eq!(url_encode("linux process firefox"), "linux+process+firefox");
}

#[test]
fn url_encode_special_chars_become_percent_hex() {
    assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    assert_eq!(url_encode("?#/"), "%3F%23%2F");
}

#[test]
fn url_encode_non_ascii_byte_wise() {
    // UTF-8 byte for é (0xC3 0xA9) → "%C3%A9"
    assert_eq!(url_encode("é"), "%C3%A9");
}

#[test]
fn status_color_known_states_distinct_from_default() {
    // Regression: every known status string should map to a non-default
    // colour (otherwise the column loses its visual cue).
    let default = theme::TEXT;
    assert_ne!(status_color("Running"), default);
    assert_ne!(status_color("Idle"), default);
    assert_ne!(status_color("Stopped"), default);
    assert_ne!(status_color("Zombie"), default);
    // Unknown still hits default — guard against accidental match-all.
    assert_eq!(status_color("XyzUnknown"), default);
}

// --- grouping & sorting regression tests ---

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
