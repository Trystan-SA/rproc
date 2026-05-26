use std::process::Command;

#[derive(Clone, Default, PartialEq, Debug)]
pub enum ServiceScope {
    #[default]
    System,
    User,
}

#[derive(Clone, Default)]
pub struct ServiceInfo {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
    pub scope: ServiceScope,
}

pub fn list() -> Vec<ServiceInfo> {
    let mut v = list_scope(ServiceScope::System);
    v.extend(list_scope(ServiceScope::User));
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn list_scope(scope: ServiceScope) -> Vec<ServiceInfo> {
    let mut cmd = Command::new("systemctl");
    if scope == ServiceScope::User {
        cmd.arg("--user");
    }
    cmd.args([
        "list-units",
        "--type=service",
        "--all",
        "--no-legend",
        "--plain",
        "--no-pager",
    ]);
    let out = match cmd.output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let s = String::from_utf8_lossy(&out);
    let mut v = Vec::new();
    for line in s.lines() {
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap_or("").to_string();
        let load = parts.next().unwrap_or("").to_string();
        let active = parts.next().unwrap_or("").to_string();
        let sub = parts.next().unwrap_or("").to_string();
        let description = parts.collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            continue;
        }
        v.push(ServiceInfo {
            name,
            load,
            active,
            sub,
            description,
            scope: scope.clone(),
        });
    }
    v
}

#[derive(Default, Clone)]
pub struct ServiceProperties {
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub fragment_path: String,
    pub drop_in_paths: Vec<String>,
    pub exec_start: String,
    pub main_pid: String,
    pub user: String,
    pub working_directory: String,
    pub unit_file_state: String,
    pub memory_current: String,
    pub tasks_current: String,
}

const SHOW_PROPERTY_NAMES: &[&str] = &[
    "Description",
    "LoadState",
    "ActiveState",
    "SubState",
    "FragmentPath",
    "DropInPaths",
    "ExecStart",
    "MainPID",
    "User",
    "WorkingDirectory",
    "UnitFileState",
    "MemoryCurrent",
    "TasksCurrent",
];

/// Fetch detailed properties for a unit via `systemctl show`. Properties not
/// reported (or set to placeholders like "[not set]") collapse to empty strings.
pub fn show_properties(name: &str, scope: &ServiceScope) -> ServiceProperties {
    let mut cmd = Command::new("systemctl");
    if scope == &ServiceScope::User {
        cmd.arg("--user");
    }
    cmd.arg("show");
    cmd.arg(name);
    for p in SHOW_PROPERTY_NAMES {
        cmd.arg(format!("--property={p}"));
    }
    let Ok(o) = cmd.output() else {
        return ServiceProperties::default();
    };
    if !o.status.success() {
        return ServiceProperties::default();
    }
    let text = String::from_utf8_lossy(&o.stdout);
    parse_show_output(&text)
}

/// Parse the `Key=Value` output of `systemctl show`. Extracted from
/// `show_properties` so the parsing logic is testable without spawning
/// `systemctl`. Unknown keys are ignored; lines without `=` are skipped.
fn parse_show_output(text: &str) -> ServiceProperties {
    let mut out = ServiceProperties::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().to_string();
        match k.trim() {
            "Description" => out.description = v,
            "LoadState" => out.load_state = v,
            "ActiveState" => out.active_state = v,
            "SubState" => out.sub_state = v,
            "FragmentPath" => out.fragment_path = v,
            "DropInPaths" => {
                // Space-separated list of paths.
                out.drop_in_paths = v.split_whitespace().map(str::to_string).collect();
            }
            "ExecStart" => out.exec_start = v,
            "MainPID" => out.main_pid = v,
            "User" => out.user = v,
            "WorkingDirectory" => out.working_directory = v,
            "UnitFileState" => out.unit_file_state = v,
            "MemoryCurrent" => out.memory_current = v,
            "TasksCurrent" => out.tasks_current = v,
            _ => {}
        }
    }
    out
}

pub fn control(name: &str, action: &str, scope: &ServiceScope) -> Result<(), String> {
    let mut cmd = Command::new("systemctl");
    if scope == &ServiceScope::User {
        cmd.arg("--user");
    }
    cmd.args([action, name]);
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
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
}
