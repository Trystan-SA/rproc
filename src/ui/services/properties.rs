use crate::monitor::services::{self, ServiceScope};
use crate::theme;
use crate::ui::widgets;

use super::scope_str;

/// `systemctl show` is expensive (one process spawn per call). The modal must
/// not re-fetch on every frame, so we cache the last result keyed by the open
/// unit. A Reload button inside the modal re-fetches on demand.
pub(super) struct ServicePropertiesView {
    pub(super) name: String,
    pub(super) scope: ServiceScope,
    pub(super) data: services::ServiceProperties,
}

pub(super) fn render_service_properties_window(
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
