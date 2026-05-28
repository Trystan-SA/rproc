use crate::autostart;
use crate::daemon;
use crate::settings::{MAX_REFRESH_MS, MIN_REFRESH_MS, REFRESH_PRESETS, Settings};
use crate::theme;
use crate::ui::widgets;

#[derive(Default)]
pub struct State {
    // Cached result of the two `systemctl --user` probes used by the
    // autostart toggle. Each probe forks a process and round-trips DBus
    // (~10–50 ms), so calling them every frame turned the tab into a
    // repaint hot spot. Refreshed on tab open (via `invalidate`) and
    // after a successful enable/disable.
    autostart: Option<AutostartCache>,
}

#[derive(Copy, Clone)]
struct AutostartCache {
    available: bool,
    enabled: bool,
}

impl AutostartCache {
    fn probe() -> Self {
        Self {
            available: autostart::available(),
            enabled: autostart::is_enabled(),
        }
    }
}

impl State {
    /// Drop the cached autostart probe results so the next frame re-asks
    /// systemd. Call this when re-entering the Settings tab — values may
    /// have changed externally (e.g. via `systemctl` from a terminal).
    pub fn invalidate(&mut self) {
        self.autostart = None;
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut State, settings: &Settings) {
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

        // Probe systemd before the master toggle so we can cascade
        // autostart along with the daemon: turning the master off while
        // leaving the unit enabled would let systemd revive the daemon
        // at next login, contradicting the unchecked box.
        let cache = *state.autostart.get_or_insert_with(AutostartCache::probe);

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
            // Cascade to the autostart unit so the master toggle is the
            // single source of truth at the next login too. Skipped when
            // systemctl --user isn't reachable (Flatpak, no session bus)
            // or when the unit is already in the desired state.
            if cache.available && cache.enabled != enabled {
                let res = if enabled {
                    autostart::enable()
                } else {
                    autostart::disable()
                };
                if let Err(e) = res {
                    eprintln!("rproc: autostart cascade failed: {e}");
                }
                state.autostart = Some(AutostartCache::probe());
            }
        }

        // Nested sub-option: only meaningful when the daemon is allowed at
        // all. Greyed when off (with a hint) and when systemctl --user
        // isn't reachable (Flatpak sandbox, no session bus, etc.).
        let cache = *state.autostart.get_or_insert_with(AutostartCache::probe);
        let can_toggle = enabled && cache.available;
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            let mut autostart_on = cache.enabled;
            let mut resp = ui.add_enabled(
                can_toggle,
                egui::Checkbox::new(
                    &mut autostart_on,
                    "Launch at login (faster first boot)",
                ),
            );
            if !cache.available {
                resp = resp.on_disabled_hover_text(
                    "Autostart needs systemd --user and the rprocd.service unit. \
                     Unavailable in Flatpak or when running from an unpackaged build.",
                );
            } else if !enabled {
                resp = resp.on_disabled_hover_text(
                    "Turn on \"Keep the last 60 seconds in the background\" first.",
                );
            }
            if resp.changed() {
                let res = if autostart_on {
                    autostart::enable()
                } else {
                    autostart::disable()
                };
                if let Err(e) = res {
                    eprintln!("rproc: autostart toggle failed: {e}");
                }
                // Re-read the actual state from systemd: the command may
                // have failed, or the unit may end up `masked`/`enabled`
                // depending on the prior state.
                state.autostart = Some(AutostartCache::probe());
            }
        });

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
