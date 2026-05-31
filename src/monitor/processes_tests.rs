use super::*;

#[test]
fn derive_status_nonzero_cpu_always_running() {
    // A process with measurable CPU is Running regardless of the
    // instantaneous scheduler state — that's the whole point of the
    // override.
    assert_eq!(derive_status("Sleeping", 0.5), "Running");
    assert_eq!(derive_status("Run", 99.9), "Running");
    assert_eq!(derive_status("Zombie", 5.0), "Running");
}

#[test]
fn derive_status_zero_cpu_collapses_sleep_variants_to_idle() {
    for raw in ["Sleep", "Sleeping", "Idle", "Parked", "Wakekill", "Waking"] {
        assert_eq!(derive_status(raw, 0.0), "Idle", "raw={raw}");
    }
}

#[test]
fn derive_status_zero_cpu_running_states() {
    assert_eq!(derive_status("Run", 0.0), "Running");
    assert_eq!(derive_status("Running", 0.0), "Running");
}

#[test]
fn derive_status_zero_cpu_exceptional_states_pass_through() {
    assert_eq!(derive_status("UninterruptibleDiskSleep", 0.0), "Waiting");
    assert_eq!(derive_status("LockBlocked", 0.0), "Waiting");
    assert_eq!(derive_status("Stopped", 0.0), "Stopped");
    assert_eq!(derive_status("Tracing", 0.0), "Stopped");
    assert_eq!(derive_status("Zombie", 0.0), "Zombie");
    assert_eq!(derive_status("Dead", 0.0), "Zombie");
}

#[test]
fn derive_status_unknown_state_passes_through_verbatim() {
    // Defensive fallback: never lose information when sysinfo invents
    // a new status variant we haven't mapped yet.
    assert_eq!(derive_status("SomeFutureState", 0.0), "SomeFutureState");
}

#[test]
fn find_config_paths_returns_empty_for_unknown_binary() {
    // No `.unlikely_to_exist_xyz_123_rproc` dir under HOME or /etc.
    let paths = find_config_paths("unlikely_to_exist_xyz_123_rproc", "");
    assert!(paths.is_empty(), "expected empty, got {paths:?}");
}

#[test]
fn find_config_paths_empty_inputs_yields_empty() {
    let paths = find_config_paths("", "");
    assert!(paths.is_empty());
}
