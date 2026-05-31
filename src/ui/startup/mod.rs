use std::time::Instant;

use crate::monitor::startup::{self, StartupEntry, StartupSource};
use crate::theme;
use crate::ui::widgets;

mod filter;
mod properties;

use filter::{format_boot_time, parse_time_filter};
use properties::{StartupPropertiesView, build_properties_view, render_startup_properties_window};

pub struct State {
    pub entries: Vec<StartupEntry>,
    pub last_loaded: Instant,
    pub filter: String,
    pub last_error: Option<String>,
    properties: Option<StartupPropertiesView>,
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

fn scope_badge(s: &StartupSource) -> &'static str {
    match s {
        StartupSource::UserAutostart => "user · autostart",
        StartupSource::SystemAutostart => "system · autostart",
        StartupSource::SystemdSystem => "system · systemd",
        StartupSource::SystemdUser => "user · systemd",
    }
}
