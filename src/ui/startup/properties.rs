use crate::monitor::services::{self, ServiceScope};
use crate::monitor::startup::{StartupEntry, StartupSource};

/// Cached `systemctl show` result for a systemd-sourced startup row (the same
/// expensive call as the Services tab). `None` for desktop-entry rows.
pub struct StartupPropertiesView {
    pub idx: usize,
    pub systemd: Option<services::ServiceProperties>,
}

pub fn build_properties_view(entries: &[StartupEntry], idx: usize) -> StartupPropertiesView {
    let systemd = entries.get(idx).and_then(|e| {
        let is_systemd = matches!(
            e.source,
            StartupSource::SystemdSystem | StartupSource::SystemdUser
        );
        if !is_systemd {
            return None;
        }
        let scope = if matches!(e.source, StartupSource::SystemdUser) {
            ServiceScope::User
        } else {
            ServiceScope::System
        };
        Some(services::show_properties(&e.exec, &scope))
    });
    StartupPropertiesView { idx, systemd }
}
