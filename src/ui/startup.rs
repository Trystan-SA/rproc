use std::time::Instant;

use crate::monitor::services::{self, ServiceScope};
use crate::monitor::startup::{self, StartupEntry, StartupSource};
use crate::theme;
use crate::ui::widgets;

/// `systemctl show` for a systemd-sourced startup row is the same expensive
/// call as in the services tab. Cache the fetch so the modal stops spawning
/// `systemctl` on every repaint.
pub struct StartupPropertiesView {
    pub idx: usize,
    pub systemd: Option<services::ServiceProperties>,
}

pub struct State {
    pub entries: Vec<StartupEntry>,
    pub last_loaded: Instant,
    pub filter: String,
    pub last_error: Option<String>,
    pub properties: Option<StartupPropertiesView>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: startup::collect(),
            last_loaded: Instant::now(),
            filter: String::new(),
            last_error: None,
            properties: None,
        }
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal(|ui| {
        ui.heading("Startup apps");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("\u{21BB}").on_hover_text("Reload").clicked() {
                state.entries = startup::collect();
                state.last_loaded = Instant::now();
                state.last_error = None;
            }
            ui.add(
                egui::TextEdit::singleline(&mut state.filter)
                    .hint_text("Filter by name or time (e.g. >0.5)")
                    .desired_width(220.0),
            );
        });
    });
    ui.label(
        egui::RichText::new(
            "Sorted by boot time (descending). Units managed by systemd dependencies are protected and cannot be disabled.",
        )
        .color(theme::TEXT_DIM),
    );
    ui.add_space(10.0);

    if let Some(err) = &state.last_error {
        ui.colored_label(theme::ERR, err);
        ui.add_space(8.0);
    }

    let raw_filter = state.filter.trim();
    let time_filter = parse_time_filter(raw_filter);
    let text_filter = if time_filter.is_some() {
        String::new()
    } else {
        raw_filter.to_lowercase()
    };
    let mut to_toggle: Vec<(usize, bool)> = Vec::new();
    let mut open_properties_idx: Option<usize> = None;

    // Partition: critical (locked) first, then the rest.
    let mut critical_idx: Vec<usize> = Vec::new();
    let mut normal_idx: Vec<usize> = Vec::new();
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
            critical_idx.push(idx);
        } else {
            normal_idx.push(idx);
        }
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        if !normal_idx.is_empty() {
            ui.label(
                egui::RichText::new("Startup apps & services")
                    .color(theme::TEXT)
                    .strong(),
            );
            ui.add_space(6.0);
            for idx in &normal_idx {
                render_row(
                    ui,
                    &state.entries[*idx],
                    *idx,
                    &mut to_toggle,
                    &mut open_properties_idx,
                    false,
                );
            }
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);
        }

        if !critical_idx.is_empty() {
            ui.label(
                egui::RichText::new("Protected by systemd")
                    .color(theme::WARN)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(
                    "These services are pulled in by dependency (state: static/generated/alias). \
                     `systemctl disable` has no effect on them; they are managed by other units.",
                )
                .color(theme::TEXT_DIM)
                .small(),
            );
            ui.add_space(6.0);
            for idx in &critical_idx {
                render_row(
                    ui,
                    &state.entries[*idx],
                    *idx,
                    &mut to_toggle,
                    &mut open_properties_idx,
                    true,
                );
            }
        }
    });

    if let Some(idx) = open_properties_idx {
        let same = matches!(&state.properties, Some(v) if v.idx == idx);
        if !same {
            state.properties = Some(build_properties_view(&state.entries, idx));
        }
    }
    render_startup_properties_window(ui.ctx(), &mut state.properties, &state.entries);

    for (idx, enabled) in to_toggle {
        let entry = state.entries[idx].clone();
        match startup::set_enabled(&entry, enabled) {
            Ok(()) => {
                state.entries[idx].enabled = enabled;
                state.last_error = None;
            }
            Err(e) => {
                state.last_error = Some(format!("Failed to toggle {}: {e}", entry.name));
            }
        }
    }
}

fn render_row(
    ui: &mut egui::Ui,
    e: &StartupEntry,
    idx: usize,
    to_toggle: &mut Vec<(usize, bool)>,
    open_properties_idx: &mut Option<usize>,
    locked: bool,
) {
    let card = egui::Frame::new()
        .fill(theme::CARD_BG)
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(if e.name.is_empty() {
                                e.path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_default()
                            } else {
                                e.name.clone()
                            })
                            .strong()
                            .size(14.0),
                        );
                        ui.label(
                            egui::RichText::new(scope_badge(&e.source))
                                .color(theme::TEXT_DIM)
                                .small(),
                        );
                        if locked {
                            ui.label(
                                egui::RichText::new("protected")
                                    .color(theme::WARN)
                                    .small()
                                    .strong(),
                            );
                        }
                    });
                    if !e.comment.is_empty() {
                        ui.label(
                            egui::RichText::new(&e.comment)
                                .color(theme::TEXT_DIM)
                                .small(),
                        );
                    }
                    ui.label(
                        egui::RichText::new(&e.exec)
                            .color(theme::TEXT_DIM)
                            .monospace()
                            .small(),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut enabled = e.enabled;
                    let label = if locked {
                        "Protected"
                    } else if enabled {
                        "Enabled"
                    } else {
                        "Disabled"
                    };
                    ui.add_enabled_ui(!locked, |ui| {
                        if ui.toggle_value(&mut enabled, label).changed() {
                            to_toggle.push((idx, enabled));
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format_boot_time(e.boot_time_ms))
                            .monospace()
                            .color(if e.boot_time_ms.is_some() {
                                theme::TEXT
                            } else {
                                theme::TEXT_DIM
                            }),
                    );
                });
            });
        });
    // Make the whole card right-clickable so it shares the Process tab's
    // discovery pattern.
    let resp = ui
        .interact(
            card.response.rect,
            egui::Id::new(("startup_card", idx)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.double_clicked() {
        *open_properties_idx = Some(idx);
    }
    resp.context_menu(|ui| {
        ui.set_min_width(180.0);
        if ui.button("Copy name").clicked() {
            let copy = if e.name.is_empty() {
                e.path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                e.name.clone()
            };
            ui.ctx().copy_text(copy);
            ui.close();
        }
        // Desktop entries point at an actual file on disk; systemd entries
        // store the unit name in `path`, so "Open .desktop file" is meaningless.
        let is_desktop = matches!(
            e.source,
            StartupSource::UserAutostart | StartupSource::SystemAutostart
        );
        if is_desktop && ui.button("Open .desktop file").clicked() {
            widgets::open_path(&e.path.to_string_lossy(), widgets::OpenTarget::Parent);
            ui.close();
        }
        ui.separator();
        if ui.button("Properties").clicked() {
            *open_properties_idx = Some(idx);
            ui.close();
        }
    });
    ui.add_space(6.0);
}

/// Resolve the heavy properties (`systemctl show` for systemd rows) once
/// when the modal opens. Caller persists the result on `State`.
fn build_properties_view(entries: &[StartupEntry], idx: usize) -> StartupPropertiesView {
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

fn render_startup_properties_window(
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

fn scope_badge(s: &StartupSource) -> &'static str {
    match s {
        StartupSource::UserAutostart => "user · autostart",
        StartupSource::SystemAutostart => "system · autostart",
        StartupSource::SystemdSystem => "system · systemd",
        StartupSource::SystemdUser => "user · systemd",
    }
}

#[derive(Copy, Clone)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

impl Op {
    fn matches(self, value: f64, target: f64) -> bool {
        match self {
            Op::Ge => value >= target,
            Op::Gt => value > target,
            Op::Le => value <= target,
            Op::Lt => value < target,
            Op::Eq => (value - target).abs() < 0.05,
        }
    }
}

fn parse_time_filter(s: &str) -> Option<(Op, f64)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (Op::Ge, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (Op::Le, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (Op::Gt, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (Op::Lt, r)
    } else if let Some(r) = s.strip_prefix('=') {
        (Op::Eq, r)
    } else {
        (Op::Ge, s)
    };
    let rest = rest.trim().trim_end_matches('s').trim();
    rest.parse::<f64>().ok().map(|v| (op, v))
}

fn format_boot_time(ms: Option<u64>) -> String {
    match ms {
        None => "—".into(),
        Some(0) => "<1 ms".into(),
        Some(ms) if ms < 1_000 => format!("{ms} ms"),
        Some(ms) if ms < 60_000 => format!("{:.2} s", ms as f64 / 1_000.0),
        Some(ms) => {
            let total_s = ms / 1_000;
            let min = total_s / 60;
            let s = total_s % 60;
            format!("{min}m {s}s")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_time_filter_with_no_prefix_means_ge() {
        // Bare number = ">=" so users can type "0.5" without thinking about
        // operators for the common "show slow startups" case.
        let (op, v) = parse_time_filter("0.5").unwrap();
        assert!(matches!(op, Op::Ge));
        assert_eq!(v, 0.5);
    }

    #[test]
    fn parse_time_filter_all_operator_prefixes() {
        assert!(matches!(parse_time_filter(">1").unwrap().0, Op::Gt));
        assert!(matches!(parse_time_filter("<1").unwrap().0, Op::Lt));
        assert!(matches!(parse_time_filter(">=1").unwrap().0, Op::Ge));
        assert!(matches!(parse_time_filter("<=1").unwrap().0, Op::Le));
        assert!(matches!(parse_time_filter("=1").unwrap().0, Op::Eq));
    }

    #[test]
    fn parse_time_filter_strips_trailing_unit_suffix() {
        // The hint text suggests `>0.5` but users often type `>0.5s`.
        let (_, v) = parse_time_filter(">0.5s").unwrap();
        assert_eq!(v, 0.5);
    }

    #[test]
    fn parse_time_filter_rejects_non_numeric() {
        assert!(parse_time_filter("hello").is_none());
        assert!(parse_time_filter("").is_none());
        assert!(parse_time_filter(">").is_none());
    }

    #[test]
    fn op_matches_inclusive_vs_exclusive() {
        assert!(Op::Ge.matches(1.0, 1.0));
        assert!(!Op::Gt.matches(1.0, 1.0));
        assert!(Op::Le.matches(1.0, 1.0));
        assert!(!Op::Lt.matches(1.0, 1.0));
    }

    #[test]
    fn op_matches_eq_uses_epsilon() {
        // Eq tolerates 50 ms drift — the boot-time series is reported with
        // ~100 ms granularity, so strict equality would never fire.
        assert!(Op::Eq.matches(1.02, 1.0));
        assert!(Op::Eq.matches(0.96, 1.0));
        assert!(!Op::Eq.matches(1.06, 1.0));
        assert!(!Op::Eq.matches(0.93, 1.0));
    }

    #[test]
    fn format_boot_time_none_dash() {
        assert_eq!(format_boot_time(None), "—");
    }

    #[test]
    fn format_boot_time_unit_boundaries() {
        assert_eq!(format_boot_time(Some(0)), "<1 ms");
        assert_eq!(format_boot_time(Some(1)), "1 ms");
        assert_eq!(format_boot_time(Some(999)), "999 ms");
        assert_eq!(format_boot_time(Some(1_000)), "1.00 s");
        assert_eq!(format_boot_time(Some(12_345)), "12.35 s");
        assert_eq!(format_boot_time(Some(59_999)), "60.00 s");
        assert_eq!(format_boot_time(Some(60_000)), "1m 0s");
        assert_eq!(format_boot_time(Some(125_000)), "2m 5s");
    }
}
