use std::path::PathBuf;

use crate::daemon::storage;

#[derive(Default, PartialEq, Copy, Clone, Debug)]
pub(super) enum SortKey {
    Name,
    Pid,
    User,
    #[default]
    Cpu,
    Mem,
    Disk,
    Status,
}

impl SortKey {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Pid => "Pid",
            SortKey::User => "User",
            SortKey::Cpu => "Cpu",
            SortKey::Mem => "Mem",
            SortKey::Disk => "Disk",
            SortKey::Status => "Status",
        }
    }

    pub(super) fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "Name" => SortKey::Name,
            "Pid" => SortKey::Pid,
            "User" => SortKey::User,
            "Cpu" => SortKey::Cpu,
            "Mem" => SortKey::Mem,
            "Disk" => SortKey::Disk,
            "Status" => SortKey::Status,
            _ => return None,
        })
    }
}

fn sort_prefs_path() -> Option<PathBuf> {
    storage::cache_dir()
        .ok()
        .map(|d| d.join("processes_sort.txt"))
}

pub(super) fn load_sort_prefs() -> Option<(SortKey, bool)> {
    let path = sort_prefs_path()?;
    load_sort_prefs_from(&path)
}

pub(super) fn save_sort_prefs(sort: SortKey, descending: bool) {
    if let Some(path) = sort_prefs_path() {
        let _ = save_sort_prefs_to(&path, sort, descending);
    }
}

pub(super) fn load_sort_prefs_from(path: &std::path::Path) -> Option<(SortKey, bool)> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut sort = None;
    let mut desc = None;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("sort=") {
            sort = SortKey::from_str(v.trim());
        } else if let Some(v) = line.strip_prefix("descending=") {
            desc = match v.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    Some((sort?, desc?))
}

pub(super) fn save_sort_prefs_to(
    path: &std::path::Path,
    sort: SortKey,
    descending: bool,
) -> std::io::Result<()> {
    let content = format!("sort={}\ndescending={}\n", sort.as_str(), descending);
    std::fs::write(path, content)
}

#[cfg(test)]
#[path = "sort_tests.rs"]
mod tests;
