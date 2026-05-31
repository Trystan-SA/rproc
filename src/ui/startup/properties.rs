use crate::monitor::services::{self, ServiceScope};
use crate::monitor::startup::{StartupEntry, StartupSource};
use crate::theme;
use crate::ui::widgets;

use super::{format_boot_time, scope_badge};

/// `systemctl show` for a systemd-sourced startup row is the same expensive
/// call as in the services tab. Cache the fetch so the modal stops spawning
/// `systemctl` on every repaint.
pub(super) struct StartupPropertiesView {
    pub(super) idx: usize,
    systemd: Option<services::ServiceProperties>,
}

/// Resolve the heavy properties (`systemctl show` for systemd rows) once
/// when the modal opens. Caller persists the result on `State`.
pub(super) fn build_properties_view(entries: &[StartupEntry], idx: usize) -> StartupPropertiesView {
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

pub(super) fn render_startup_properties_window(
    ctx: &egui::Context,
    properties: &mut Option<StartupPropertiesView>,
    entries: &[StartupEntry],
) {
    let Some(view) = properties.as_ref() else {
        return;
    };
    let idx = view.idx;
    let Some(e) = entries.get(idx) else {
        *properties = None;
        return;
    };
    let systemd_props = view.systemd.clone();

    let title = if e.name.is_empty() {
        e.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| e.exec.clone())
    } else {
        e.name.clone()
    };

    let mut open = true;
    let mut reload = false;
    let is_systemd = matches!(
        e.source,
        StartupSource::SystemdSystem | StartupSource::SystemdUser
    );
    egui::Window::new(format!("Properties: {title}"))
        .id(egui::Id::new(("startup_properties", idx)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            if is_systemd {
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
            }
            widgets::stat(ui, "Name", if e.name.is_empty() { &title } else { &e.name });
            widgets::stat(ui, "Source", scope_badge(&e.source));
            if !e.comment.is_empty() {
                widgets::stat(ui, "Description", &e.comment);
            }
            widgets::stat(ui, "Boot time", &format_boot_time(e.boot_time_ms));
            widgets::stat(
                ui,
                "State",
                if e.critical {
                    "Protected (managed by systemd)"
                } else if e.enabled {
                    "Enabled"
                } else {
                    "Disabled"
                },
            );
            ui.separator();
            if !is_systemd {
                // Desktop entry: the path on disk IS the .desktop file.
                widgets::path_field(
                    ui,
                    ".desktop file",
                    &e.path.to_string_lossy(),
                    widgets::OpenTarget::Parent,
                );
                ui.add_space(4.0);
                if !e.exec.is_empty() {
                    ui.label(egui::RichText::new("Exec").color(theme::TEXT_DIM));
                    ui.add(egui::Label::new(&e.exec).wrap());
                    ui.add_space(4.0);
                }
                if !e.icon.is_empty() {
                    widgets::stat(ui, "Icon", &e.icon);
                }
            }
            if let Some(props) = systemd_props {
                widgets::stat(ui, "Unit", &e.exec);
                if !props.unit_file_state.is_empty() {
                    widgets::stat(ui, "Unit file state", &props.unit_file_state);
                }
                if !props.active_state.is_empty() {
                    widgets::stat(ui, "Active", &props.active_state);
                }
                if !props.sub_state.is_empty() {
                    widgets::stat(ui, "Sub", &props.sub_state);
                }
                if !props.main_pid.is_empty() && props.main_pid != "0" {
                    widgets::stat(ui, "Main PID", &props.main_pid);
                }
                if !props.user.is_empty() {
                    widgets::stat(ui, "User", &props.user);
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
            }
        });
    if reload && let Some(v) = properties.as_mut() {
        *v = build_properties_view(entries, idx);
    }
    if !open {
        *properties = None;
    }
}
