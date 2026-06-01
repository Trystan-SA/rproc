use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::monitor::startup::{self, StartupEntry, StartupSource};
use crate::{MainWindow, StartupRow};

pub mod filter;
pub mod properties;

use filter::{format_boot_time, parse_time_filter};

pub struct State {
    pub entries: Vec<StartupEntry>,
    pub filter: String,
    pub error: Option<String>,
    normal_model: Rc<VecModel<StartupRow>>,
    critical_model: Rc<VecModel<StartupRow>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: startup::collect(),
            filter: String::new(),
            error: None,
            normal_model: Rc::new(VecModel::default()),
            critical_model: Rc::new(VecModel::default()),
        }
    }
}

impl State {
    pub fn reload(&mut self) {
        self.entries = startup::collect();
        self.error = None;
    }

    pub fn toggle(&mut self, index: usize, enabled: bool) {
        let Some(entry) = self.entries.get(index).cloned() else {
            return;
        };
        match startup::set_enabled(&entry, enabled) {
            Ok(()) => {
                self.entries[index].enabled = enabled;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("Failed to toggle {}: {e}", entry.name));
            }
        }
    }
}

fn ss(s: &str) -> SharedString {
    s.into()
}

pub fn scope_badge(s: &StartupSource) -> &'static str {
    match s {
        StartupSource::UserAutostart => "user · autostart",
        StartupSource::SystemAutostart => "system · autostart",
        StartupSource::SystemdSystem => "system · systemd",
        StartupSource::SystemdUser => "user · systemd",
    }
}

pub(crate) fn entry_name(e: &StartupEntry) -> String {
    if e.name.is_empty() {
        e.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        e.name.clone()
    }
}

fn to_row(e: &StartupEntry, index: usize) -> StartupRow {
    let is_desktop = matches!(
        e.source,
        StartupSource::UserAutostart | StartupSource::SystemAutostart
    );
    StartupRow {
        index: index as i32,
        name: ss(&entry_name(e)),
        badge: ss(scope_badge(&e.source)),
        comment: ss(&e.comment),
        exec: ss(&e.exec),
        boot_time: ss(&format_boot_time(e.boot_time_ms)),
        boot_known: e.boot_time_ms.is_some(),
        enabled: e.enabled,
        locked: e.critical,
        is_desktop,
    }
}

pub fn apply(window: &MainWindow, state: &State) {
    let raw_filter = state.filter.trim();
    let time_filter = parse_time_filter(raw_filter);
    let text_filter = if time_filter.is_some() {
        String::new()
    } else {
        raw_filter.to_lowercase()
    };

    let mut normal: Vec<StartupRow> = Vec::new();
    let mut critical: Vec<StartupRow> = Vec::new();
    for (idx, e) in state.entries.iter().enumerate() {
        if let Some((op, secs)) = time_filter {
            let ms = e.boot_time_ms.unwrap_or(0) as f64 / 1_000.0;
            if !op.matches(ms, secs) {
                continue;
            }
        } else if !text_filter.is_empty()
            && !e.name.to_lowercase().contains(&text_filter)
            && !e.exec.to_lowercase().contains(&text_filter)
        {
            continue;
        }
        if e.critical {
            critical.push(to_row(e, idx));
        } else {
            normal.push(to_row(e, idx));
        }
    }

    crate::ui::model::sync(&state.normal_model, normal);
    crate::ui::model::sync(&state.critical_model, critical);
    window.set_start_normal(ModelRc::from(state.normal_model.clone()));
    window.set_start_critical(ModelRc::from(state.critical_model.clone()));
    window.set_start_error(ss(state.error.as_deref().unwrap_or("")));
}
