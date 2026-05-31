use std::path::PathBuf;

mod desktop;
mod systemd;

#[derive(Clone, Default, PartialEq, Debug)]
pub enum StartupSource {
    #[default]
    UserAutostart,
    SystemAutostart,
    SystemdSystem,
    SystemdUser,
}

#[derive(Clone, Default)]
pub struct StartupEntry {
    pub source: StartupSource,
    pub path: PathBuf,
    pub name: String,
    pub exec: String,
    pub comment: String,
    pub icon: String,
    pub enabled: bool,
    pub boot_time_ms: Option<u64>,
    pub critical: bool,
}

pub fn collect() -> Vec<StartupEntry> {
    let mut out = Vec::new();

    if let Ok(home) = std::env::var("HOME") {
        out.extend(desktop::scan_dir(
            PathBuf::from(format!("{home}/.config/autostart")),
            StartupSource::UserAutostart,
        ));
    }
    out.extend(desktop::scan_dir(
        PathBuf::from("/etc/xdg/autostart"),
        StartupSource::SystemAutostart,
    ));

    let protected_system = systemd::protected_units(false);
    let protected_user = systemd::protected_units(true);

    out.extend(systemd::collect(false, &protected_system));
    out.extend(systemd::collect(true, &protected_user));

    // Sort: critical first (so user sees what's protected), then by descending boot time,
    // then entries without a measured time at the end, alphabetically.
    out.sort_by(|a, b| {
        b.critical
            .cmp(&a.critical)
            .then_with(|| match (a.boot_time_ms, b.boot_time_ms) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            })
    });
    out
}

/// Toggle an autostart entry. Returns Err for critical systemd units (cannot be disabled).
pub fn set_enabled(entry: &StartupEntry, enabled: bool) -> Result<(), String> {
    if entry.critical {
        return Err(
            "This unit is managed by systemd dependencies (static/generated) and cannot be disabled directly."
                .into(),
        );
    }
    match entry.source {
        StartupSource::UserAutostart | StartupSource::SystemAutostart => {
            desktop::set_enabled(entry, enabled).map_err(|e| e.to_string())
        }
        StartupSource::SystemdSystem | StartupSource::SystemdUser => {
            systemd::set_enabled(entry, enabled)
        }
    }
}
