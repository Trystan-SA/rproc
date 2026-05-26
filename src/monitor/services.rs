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

/// Fetch detailed properties for a unit via `systemctl show`. Properties not
/// reported (or set to placeholders like "[not set]") collapse to empty strings.
pub fn show_properties(name: &str, scope: &ServiceScope) -> ServiceProperties {
    let mut cmd = Command::new("systemctl");
    if scope == &ServiceScope::User {
        cmd.arg("--user");
    }
    let props = [
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
    cmd.arg("show");
    cmd.arg(name);
    for p in &props {
        cmd.arg(format!("--property={p}"));
    }
    let mut out = ServiceProperties::default();
    let Ok(o) = cmd.output() else { return out };
    if !o.status.success() {
        return out;
    }
    let text = String::from_utf8_lossy(&o.stdout);
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
