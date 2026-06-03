use super::*;

#[test]
fn parse_show_output_extracts_all_known_keys() {
    let text = "\
Description=My test unit
LoadState=loaded
ActiveState=active
SubState=running
FragmentPath=/etc/systemd/system/foo.service
DropInPaths=/etc/systemd/system/foo.service.d/a.conf /etc/systemd/system/foo.service.d/b.conf
ExecStart={ path=/usr/bin/foo ; argv[]=/usr/bin/foo --bar ; ignore_errors=no }
MainPID=1234
User=alice
WorkingDirectory=/srv/foo
UnitFileState=enabled
MemoryCurrent=2097152
TasksCurrent=12
";
    let p = parse_show_output(text);
    assert_eq!(p.description, "My test unit");
    assert_eq!(p.load_state, "loaded");
    assert_eq!(p.active_state, "active");
    assert_eq!(p.sub_state, "running");
    assert_eq!(p.fragment_path, "/etc/systemd/system/foo.service");
    assert_eq!(
        p.drop_in_paths,
        vec![
            "/etc/systemd/system/foo.service.d/a.conf".to_string(),
            "/etc/systemd/system/foo.service.d/b.conf".to_string(),
        ]
    );
    assert!(p.exec_start.starts_with("{ path=/usr/bin/foo"));
    assert_eq!(p.main_pid, "1234");
    assert_eq!(p.user, "alice");
    assert_eq!(p.working_directory, "/srv/foo");
    assert_eq!(p.unit_file_state, "enabled");
    assert_eq!(p.memory_current, "2097152");
    assert_eq!(p.tasks_current, "12");
}

#[test]
fn parse_show_output_skips_unknown_and_malformed_lines() {
    let text = "Description=hi\nNotAKey\nFoo=bar\nLoadState=loaded\n";
    let p = parse_show_output(text);
    assert_eq!(p.description, "hi");
    assert_eq!(p.load_state, "loaded");
    // Unknown/malformed lines must not corrupt other fields.
    assert!(p.fragment_path.is_empty());
}

#[test]
fn parse_show_output_empty_input_returns_default() {
    let p = parse_show_output("");
    assert!(p.description.is_empty());
    assert!(p.drop_in_paths.is_empty());
}

#[test]
fn parse_show_output_handles_empty_values() {
    // systemctl emits `Key=` for unset properties.
    let text = "Description=\nMainPID=0\nWorkingDirectory=[not set]\n";
    let p = parse_show_output(text);
    assert!(p.description.is_empty());
    assert_eq!(p.main_pid, "0");
    // Sentinel passes through; consumers in the UI handle it.
    assert_eq!(p.working_directory, "[not set]");
}

#[test]
fn parse_show_output_drop_in_paths_single_path() {
    let p = parse_show_output("DropInPaths=/etc/systemd/system/foo.service.d/only.conf\n");
    assert_eq!(p.drop_in_paths.len(), 1);
    assert_eq!(
        p.drop_in_paths[0],
        "/etc/systemd/system/foo.service.d/only.conf"
    );
}

#[test]
fn parse_show_output_drop_in_paths_empty_when_unset() {
    let p = parse_show_output("DropInPaths=\n");
    assert!(p.drop_in_paths.is_empty());
}

#[test]
fn show_property_names_cover_all_parsed_fields() {
    // Regression guard: if a new field is added to `ServiceProperties`,
    // both the show query and the parser must learn about it. We can't
    // enumerate struct fields at runtime, but we can at least keep the
    // query list in sync with the parser's match arms.
    let probe = "\
Description=x
LoadState=x
ActiveState=x
SubState=x
FragmentPath=x
DropInPaths=x
ExecStart=x
MainPID=x
User=x
WorkingDirectory=x
UnitFileState=x
MemoryCurrent=x
TasksCurrent=x
";
    let p = parse_show_output(probe);
    for key in SHOW_PROPERTY_NAMES {
        let field_set = match *key {
            "Description" => !p.description.is_empty(),
            "LoadState" => !p.load_state.is_empty(),
            "ActiveState" => !p.active_state.is_empty(),
            "SubState" => !p.sub_state.is_empty(),
            "FragmentPath" => !p.fragment_path.is_empty(),
            "DropInPaths" => !p.drop_in_paths.is_empty(),
            "ExecStart" => !p.exec_start.is_empty(),
            "MainPID" => !p.main_pid.is_empty(),
            "User" => !p.user.is_empty(),
            "WorkingDirectory" => !p.working_directory.is_empty(),
            "UnitFileState" => !p.unit_file_state.is_empty(),
            "MemoryCurrent" => !p.memory_current.is_empty(),
            "TasksCurrent" => !p.tasks_current.is_empty(),
            other => panic!("queried key {other} has no parser arm"),
        };
        assert!(field_set, "queried key {key} not stored by parser");
    }
}
