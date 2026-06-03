use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::monitor::services::{self, ServiceInfo, ServiceScope};
use crate::theme;
use crate::{MainWindow, ServiceRow};

pub mod properties;

pub struct State {
    pub entries: Vec<ServiceInfo>,
    pub filter: String,
    pub message: Option<(bool, String)>,
    pub show_only_running: bool,
    rows_model: Rc<VecModel<ServiceRow>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: services::list(),
            filter: String::new(),
            message: None,
            show_only_running: false,
            rows_model: Rc::new(VecModel::default()),
        }
    }
}

impl State {
    pub fn refresh(&mut self) {
        self.entries = services::list();
    }

    /// Run a systemctl action against the unit at `index`, record the outcome,
    /// and reload the list on success.
    pub fn action(&mut self, index: usize, action: &str) {
        let Some(svc) = self.entries.get(index).cloned() else {
            return;
        };
        match services::control(&svc.name, action, &svc.scope) {
            Ok(()) => {
                self.message = Some((true, format!("{action} {} ok", svc.name)));
                self.refresh();
            }
            Err(e) => {
                self.message = Some((false, format!("{action} {}: {e}", svc.name)));
            }
        }
    }
}

fn ss(s: &str) -> SharedString {
    s.into()
}

pub fn scope_str(s: &ServiceScope) -> &'static str {
    match s {
        ServiceScope::System => "system",
        ServiceScope::User => "user",
    }
}

fn active_color(s: &str) -> slint::Color {
    match s {
        "active" => theme::ok(),
        "activating" | "reloading" => theme::warn(),
        "failed" => theme::err(),
        _ => theme::text_dim(),
    }
}

pub fn apply(window: &MainWindow, state: &State) {
    let filter = state.filter.to_lowercase();
    let mut rows: Vec<ServiceRow> = Vec::new();
    for (i, s) in state.entries.iter().enumerate() {
        if state.show_only_running && s.active != "active" {
            continue;
        }
        if !filter.is_empty()
            && !s.name.to_lowercase().contains(&filter)
            && !s.description.to_lowercase().contains(&filter)
        {
            continue;
        }
        rows.push(ServiceRow {
            index: i as i32,
            name: ss(&s.name),
            scope: ss(scope_str(&s.scope)),
            active: ss(&s.active),
            active_color: active_color(&s.active),
            sub: ss(&s.sub),
            description: ss(&s.description),
        });
    }
    let shown = rows.len();
    crate::ui::model::sync(&state.rows_model, rows);
    window.set_svc_rows(ModelRc::from(state.rows_model.clone()));
    window.set_svc_count_label(ss(&format!("{shown} units shown")));
    window.set_svc_running_only(state.show_only_running);
    match &state.message {
        Some((ok, msg)) => {
            window.set_svc_message(ss(msg));
            window.set_svc_message_ok(*ok);
        }
        None => window.set_svc_message(ss("")),
    }
}
