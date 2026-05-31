use std::path::PathBuf;

use crate::monitor::{self, Snapshot};
use crate::theme;
use crate::ui::widgets;

/// Heavy lookups behind the Properties modal — each one walks `/proc` and
/// performs a fistful of syscalls. Computed when the modal opens, then
/// re-used until the user clicks Reload or opens a different PID.
pub(super) struct ProcessPropertiesView {
    pub(super) pid: u32,
    cwd: Option<String>,
    fd_count: Option<usize>,
    configs: Vec<PathBuf>,
}

/// Resolve cwd, fd count, and well-known config paths once. The result is
/// stable enough (working dir + fd count drift slowly, configs almost never)
/// that we don't refresh on every frame — the modal exposes a Reload button.
pub(super) fn build_properties_view(p: &monitor::processes::ProcInfo) -> ProcessPropertiesView {
    ProcessPropertiesView {
        pid: p.pid,
        cwd: monitor::processes::read_cwd(p.pid),
        fd_count: monitor::processes::read_fd_count(p.pid),
        configs: monitor::processes::find_config_paths(&p.name, &p.exe),
    }
}

pub(super) fn render_properties_window(
    ctx: &egui::Context,
    properties: &mut Option<ProcessPropertiesView>,
    snap: &Snapshot,
) {
    let Some(view) = properties.as_ref() else {
        return;
    };
    let pid = view.pid;
    let Some(p) = snap.processes.iter().find(|p| p.pid == pid) else {
        // Process exited — auto-close.
        *properties = None;
        return;
    };

    let uptime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH + std::time::Duration::from_secs(p.start_time))
        .ok()
        .map(|d| widgets::format_duration(d.as_secs()));

    let mut open = true;
    let mut reload = false;
    egui::Window::new(format!("Properties: {}", p.name))
        .id(egui::Id::new(("proc_properties", pid)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(520.0)
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
            widgets::stat(ui, "PID", &p.pid.to_string());
            widgets::stat(
                ui,
                "Parent PID",
                &p.parent
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "—".into()),
            );
            widgets::stat(ui, "User", if p.user.is_empty() { "—" } else { &p.user });
            widgets::stat(ui, "Status", &p.status);
            ui.separator();
            widgets::stat(ui, "CPU", &format!("{:.1}%", p.cpu_pct));
            widgets::stat(ui, "Memory (RSS)", &widgets::format_bytes(p.mem_bytes));
            widgets::stat(ui, "Virtual memory", &widgets::format_bytes(p.virt_bytes));
            widgets::stat(ui, "Threads", &p.threads.to_string());
            if let Some(fds) = view.fd_count {
                widgets::stat(ui, "Open file descriptors", &fds.to_string());
            }
            if let Some(up) = uptime {
                widgets::stat(ui, "Running for", &up);
            }
            ui.separator();
            if !p.exe.is_empty() {
                widgets::path_field(ui, "Executable", &p.exe, widgets::OpenTarget::Parent);
                ui.add_space(4.0);
            }
            if let Some(cwd) = &view.cwd {
                widgets::path_field(ui, "Working directory", cwd, widgets::OpenTarget::Self_);
                ui.add_space(4.0);
            }
            if !view.configs.is_empty() {
                ui.label(egui::RichText::new("Config").color(theme::TEXT_DIM));
                for path in &view.configs {
                    widgets::path_field_compact(ui, &path.to_string_lossy());
                }
                ui.add_space(4.0);
            }
            if !p.cmd.is_empty() {
                ui.label(egui::RichText::new("Command line").color(theme::TEXT_DIM));
                ui.add(egui::Label::new(&p.cmd).wrap());
            }
        });
    if reload
        && let Some(v) = properties.as_mut()
        && let Some(p) = snap.processes.iter().find(|p| p.pid == pid)
    {
        *v = build_properties_view(p);
    }
    if !open {
        *properties = None;
    }
}
