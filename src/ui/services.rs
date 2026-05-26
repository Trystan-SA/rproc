use std::time::Instant;

use egui_extras::{Column, TableBuilder};

use crate::monitor::services::{self, ServiceInfo, ServiceScope};
use crate::theme;
use crate::ui::widgets;

/// `systemctl show` is expensive (one process spawn per call). The modal must
/// not re-fetch on every frame, so we cache the last result keyed by the open
/// unit. A Reload button inside the modal re-fetches on demand.
pub struct ServicePropertiesView {
    pub name: String,
    pub scope: ServiceScope,
    pub data: services::ServiceProperties,
}

pub struct State {
    pub entries: Vec<ServiceInfo>,
    pub last_loaded: Instant,
    pub filter: String,
    pub last_message: Option<(bool, String)>,
    pub show_only_running: bool,
    pub properties: Option<ServicePropertiesView>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: services::list(),
            last_loaded: Instant::now(),
            filter: String::new(),
            last_message: None,
            show_only_running: false,
            properties: None,
        }
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal(|ui| {
        ui.heading("Services");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("\u{21BB}").on_hover_text("Reload").clicked() {
                refresh(state);
            }
            ui.toggle_value(&mut state.show_only_running, "Running only");
            ui.add(
                egui::TextEdit::singleline(&mut state.filter)
                    .hint_text("Filter…")
                    .desired_width(180.0),
            );
        });
    });
    ui.label(egui::RichText::new("systemd system + user units (.service).").color(theme::TEXT_DIM));

    if let Some((ok, msg)) = &state.last_message {
        ui.colored_label(if *ok { theme::OK } else { theme::ERR }, msg);
    }
    ui.add_space(8.0);

    let filter = state.filter.to_lowercase();
    let rows: Vec<usize> = state
        .entries
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            if state.show_only_running && s.active != "active" {
                return false;
            }
            filter.is_empty()
                || s.name.to_lowercase().contains(&filter)
                || s.description.to_lowercase().contains(&filter)
        })
        .map(|(i, _)| i)
        .collect();

    let mut actions: Vec<(usize, &'static str)> = Vec::new();
    let mut open_properties: Option<(String, ServiceScope)> = None;

    egui::Frame::new()
        .fill(theme::PANEL_BG)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(290.0).at_least(180.0).clip(true))
                .column(Column::initial(70.0).at_least(60.0))
                .column(Column::initial(80.0).at_least(70.0))
                .column(Column::initial(80.0).at_least(70.0))
                .column(Column::remainder().at_least(200.0).clip(true))
                .column(Column::initial(230.0))
                .header(26.0, |mut h| {
                    h.col(|ui| {
                        ui.strong("Unit");
                    });
                    h.col(|ui| {
                        ui.strong("Scope");
                    });
                    h.col(|ui| {
                        ui.strong("Active");
                    });
                    h.col(|ui| {
                        ui.strong("Sub");
                    });
                    h.col(|ui| {
                        ui.strong("Description");
                    });
                    h.col(|ui| {
                        ui.strong("Actions");
                    });
                })
                .body(|body| {
                    body.rows(24.0, rows.len(), |mut row| {
                        let idx = rows[row.index()];
                        let s = &state.entries[idx];
                        row.col(|ui| {
                            ui.add(egui::Label::new(&s.name).truncate());
                        });
                        row.col(|ui| {
                            ui.label(scope_str(&s.scope));
                        });
                        row.col(|ui| {
                            ui.label(egui::RichText::new(&s.active).color(active_color(&s.active)));
                        });
                        row.col(|ui| {
                            ui.label(&s.sub);
                        });
                        row.col(|ui| {
                            ui.add(egui::Label::new(&s.description).truncate());
                        });
                        row.col(|ui| {
                            ui.horizontal(|ui| {
                                if ui.small_button("Start").clicked() {
                                    actions.push((idx, "start"));
                                }
                                if ui.small_button("Stop").clicked() {
                                    actions.push((idx, "stop"));
                                }
                                if ui.small_button("Restart").clicked() {
                                    actions.push((idx, "restart"));
                                }
                            });
                        });
                        let resp = row.response();
                        if resp.double_clicked() {
                            open_properties = Some((s.name.clone(), s.scope.clone()));
                        }
                        resp.context_menu(|ui| {
                            ui.set_min_width(180.0);
                            if ui.button("Start").clicked() {
                                actions.push((idx, "start"));
                                ui.close();
                            }
                            if ui.button("Stop").clicked() {
                                actions.push((idx, "stop"));
                                ui.close();
                            }
                            if ui.button("Restart").clicked() {
                                actions.push((idx, "restart"));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Copy unit name").clicked() {
                                ui.ctx().copy_text(s.name.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Properties").clicked() {
                                open_properties = Some((s.name.clone(), s.scope.clone()));
                                ui.close();
                            }
                        });
                    });
                });
        });

    if let Some((name, scope)) = open_properties {
        // Only refetch when the user opens a different unit — re-clicking the
        // same one keeps the cached data so the modal stays cheap.
        let same = matches!(&state.properties, Some(v) if v.name == name && v.scope == scope);
        if !same {
            state.properties = Some(ServicePropertiesView {
                data: services::show_properties(&name, &scope),
                name,
                scope,
            });
        }
    }
    render_service_properties_window(ui.ctx(), &mut state.properties);

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(format!("{} units shown", rows.len()))
            .color(theme::TEXT_DIM)
            .small(),
    );

    for (idx, action) in actions {
        let svc = state.entries[idx].clone();
        match services::control(&svc.name, action, &svc.scope) {
            Ok(()) => {
                state.last_message = Some((true, format!("{action} {} ok", svc.name)));
                refresh(state);
            }
            Err(e) => {
                state.last_message = Some((false, format!("{action} {}: {e}", svc.name)));
            }
        }
    }
}

fn refresh(state: &mut State) {
    state.entries = services::list();
    state.last_loaded = Instant::now();
}

fn scope_str(s: &ServiceScope) -> &'static str {
    match s {
        ServiceScope::System => "system",
        ServiceScope::User => "user",
    }
}

fn render_service_properties_window(
    ctx: &egui::Context,
    properties: &mut Option<ServicePropertiesView>,
) {
    let Some(view) = properties.as_ref() else {
        return;
    };

    let mut open = true;
    let mut reload = false;
    let props = &view.data;
    let name = view.name.clone();
    let scope = view.scope.clone();
    egui::Window::new(format!("Properties: {name}"))
        .id(egui::Id::new((
            "svc_properties",
            name.clone(),
            scope_str(&scope),
        )))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("\u{21BB}")
                        .on_hover_text("Reload properties")
                        .clicked()
                    {
                        reload = true;
                    }
                });
            });
            widgets::stat(ui, "Unit", &name);
            widgets::stat(ui, "Scope", scope_str(&scope));
            if !props.description.is_empty() {
                widgets::stat(ui, "Description", &props.description);
            }
            ui.separator();
            if !props.load_state.is_empty() {
                widgets::stat(ui, "Load", &props.load_state);
            }
            if !props.active_state.is_empty() {
                widgets::stat(ui, "Active", &props.active_state);
            }
            if !props.sub_state.is_empty() {
                widgets::stat(ui, "Sub", &props.sub_state);
            }
            if !props.unit_file_state.is_empty() {
                widgets::stat(ui, "Unit file state", &props.unit_file_state);
            }
            if !props.main_pid.is_empty() && props.main_pid != "0" {
                widgets::stat(ui, "Main PID", &props.main_pid);
            }
            if !props.user.is_empty() {
                widgets::stat(ui, "User", &props.user);
            }
            if !props.memory_current.is_empty()
                && props.memory_current != "[not set]"
                && props.memory_current != "0"
                && let Ok(bytes) = props.memory_current.parse::<u64>()
            {
                widgets::stat(ui, "Memory", &widgets::format_bytes(bytes));
            }
            if !props.tasks_current.is_empty() && props.tasks_current != "[not set]" {
                widgets::stat(ui, "Tasks", &props.tasks_current);
            }
            ui.separator();
            if !props.fragment_path.is_empty() {
                widgets::path_field(
                    ui,
                    "Unit file",
                    &props.fragment_path,
                    widgets::OpenTarget::Parent,
                );
                ui.add_space(4.0);
            }
            if !props.drop_in_paths.is_empty() {
                ui.label(egui::RichText::new("Drop-in files").color(theme::TEXT_DIM));
                for p in &props.drop_in_paths {
                    widgets::path_field_compact(ui, p);
                }
                ui.add_space(4.0);
            }
            if !props.working_directory.is_empty() && props.working_directory != "[not set]" {
                widgets::path_field(
                    ui,
                    "Working directory",
                    &props.working_directory,
                    widgets::OpenTarget::Self_,
                );
                ui.add_space(4.0);
            }
            if !props.exec_start.is_empty() {
                ui.label(egui::RichText::new("ExecStart").color(theme::TEXT_DIM));
                ui.add(egui::Label::new(&props.exec_start).wrap());
            }
        });
    if reload && let Some(v) = properties.as_mut() {
        v.data = services::show_properties(&name, &scope);
    }
    if !open {
        *properties = None;
    }
}

fn active_color(s: &str) -> egui::Color32 {
    match s {
        "active" => theme::OK,
        "activating" | "reloading" => theme::WARN,
        "failed" => theme::ERR,
        _ => theme::TEXT_DIM,
    }
}
