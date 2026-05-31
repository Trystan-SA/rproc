use crate::daemon;
use crate::settings::{MAX_REFRESH_MS, MIN_REFRESH_MS, REFRESH_PRESETS, Settings};
use crate::theme;
use crate::ui::widgets;

#[derive(Default)]
pub struct State {}

pub fn show(ui: &mut egui::Ui, _state: &mut State, settings: &Settings) {
    ui.heading("Settings");
    ui.label(
        egui::RichText::new("Tweak how rproc samples and displays system data.")
            .color(theme::TEXT_DIM),
    );
    ui.add_space(16.0);

    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Refresh rate").strong().size(15.0));
                ui.label(
                    egui::RichText::new(
                        "How often the sampler thread polls the system. \
                         Lower intervals feel snappier but use more CPU.",
                    )
                    .color(theme::TEXT_DIM)
                    .small(),
                );
            });
        });
        ui.add_space(10.0);

        let mut current = settings.refresh_ms();

        // Preset chips
        ui.horizontal_wrapped(|ui| {
            for (ms, label) in REFRESH_PRESETS {
                let selected = current == *ms;
                if preset_chip(ui, label, selected).clicked() {
                    settings.set_refresh_ms(*ms);
                    current = *ms;
                }
            }
        });

        ui.add_space(12.0);

        // Fine slider for arbitrary values.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Custom").color(theme::TEXT_DIM).small());
            let mut value = current;
            let resp = ui.add(
                egui::Slider::new(&mut value, MIN_REFRESH_MS..=MAX_REFRESH_MS)
                    .logarithmic(true)
                    .suffix(" ms"),
            );
            if resp.changed() {
                settings.set_refresh_ms(value);
                current = value;
            }
        });

        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!("Currently sampling every {}", format_ms(current)))
                .color(theme::ACCENT)
                .strong(),
        );
    });

    ui.add_space(12.0);

    widgets::card(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Background history")
                    .strong()
                    .size(15.0),
            );
            ui.label(
                egui::RichText::new(
                    "Run a tiny background process that records the last 60 s of \
                     CPU, memory, disk, network and GPU activity. When on, rproc \
                     shows that recent history the moment you reopen it, even after \
                     a restart. When off, no background process runs, but history \
                     starts empty each time you open the window.",
                )
                .color(theme::TEXT_DIM)
                .small(),
            );
        });
        ui.add_space(10.0);

        let mut enabled = settings.daemon_enabled();
        if ui
            .checkbox(
                &mut enabled,
                egui::RichText::new("Keep the last 60 seconds in the background").strong(),
            )
            .changed()
        {
            settings.set_daemon_enabled(enabled);
            // Apply the change immediately: start the daemon now, or stop the
            // one that's currently running.
            if enabled {
                daemon::spawn_if_absent();
            } else {
                daemon::stop();
            }
        }

        ui.add_space(6.0);
        let (status, color) = if enabled {
            ("Background sampler running", theme::ACCENT)
        } else {
            ("Background sampler off", theme::TEXT_DIM)
        };
        ui.label(egui::RichText::new(status).color(color).strong());
    });

    ui.add_space(12.0);

    widgets::card(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Per-process graph attribution")
                    .strong()
                    .size(15.0),
            );
            ui.label(
                egui::RichText::new(
                    "Record the heaviest processes behind each point on the CPU, \
                     memory, disk and GPU graphs. When on, hover any point on those \
                     graphs to see the top processes for that moment. This makes \
                     the sampler scan the full process list every tick, so it's \
                     off by default to keep the core lightweight. History is kept \
                     only while the window is open and never written to disk.",
                )
                .color(theme::TEXT_DIM)
                .small(),
            );
        });
        ui.add_space(10.0);

        let mut enabled = settings.attribution_enabled();
        if ui
            .checkbox(
                &mut enabled,
                egui::RichText::new("Show top processes on graph hover").strong(),
            )
            .changed()
        {
            settings.set_attribution_enabled(enabled);
        }

        ui.add_space(6.0);
        let (status, color) = if enabled {
            (
                "Attribution on — hover the CPU / Memory / Disk / GPU graphs",
                theme::ACCENT,
            )
        } else {
            ("Attribution off", theme::TEXT_DIM)
        };
        ui.label(egui::RichText::new(status).color(color).strong());
    });

    ui.add_space(12.0);

    widgets::card(ui, |ui| {
        ui.label(egui::RichText::new("About").strong().size(15.0));
        ui.add_space(4.0);
        widgets::stat(ui, "Version", env!("CARGO_PKG_VERSION"));
        widgets::stat(
            ui,
            "Build",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        );
    });
}

fn preset_chip(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let bg = if selected {
        egui::Color32::from_rgba_unmultiplied(0x60, 0xCD, 0xFF, 50)
    } else {
        theme::PANEL_BG
    };
    let fg = if selected { theme::ACCENT } else { theme::TEXT };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(fg).strong())
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(6))
            .min_size(egui::vec2(80.0, 28.0)),
    )
}

fn format_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}
