#![allow(dead_code)]

slint::include_modules!();

mod app;
mod daemon;
mod monitor;
mod settings;
mod theme;
mod ui;

fn main() -> anyhow::Result<()> {
    // `--daemon` runs the headless sampler and never touches the GUI — the
    // Slint code stays paged out for the lifetime of the process.
    if std::env::args().skip(1).any(|a| a == "--daemon") {
        return daemon::run();
    }

    let settings = settings::Settings::load();

    // Keep a background sampler alive so this launch (and the next) sees fresh
    // history. No-op if one is already running; skipped when disabled.
    if settings.daemon_enabled() {
        daemon::spawn_if_absent();
    }

    app::run(settings)
}
