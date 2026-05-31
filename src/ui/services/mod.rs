use std::time::Instant;

use egui_extras::{Column, TableBuilder};

use crate::monitor::services::{self, ServiceInfo, ServiceScope};
use crate::theme;

mod properties;

use properties::{ServicePropertiesView, render_service_properties_window};

pub struct State {
    pub entries: Vec<ServiceInfo>,
    pub last_loaded: Instant,
    pub filter: String,
    pub last_message: Option<(bool, String)>,
    pub show_only_running: bool,
    properties: Option<ServicePropertiesView>,
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
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 30.0)
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
                                    ui.label(
                                        egui::RichText::new(&s.active)
                                            .color(active_color(&s.active)),
                                    );
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

fn active_color(s: &str) -> egui::Color32 {
    match s {
        "active" => theme::OK,
        "activating" | "reloading" => theme::WARN,
        "failed" => theme::ERR,
        _ => theme::TEXT_DIM,
    }
}
