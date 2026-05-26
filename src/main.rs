#![allow(dead_code)]

mod app;
mod daemon;
mod monitor;
mod settings;
mod theme;
mod ui;

use app::App;

fn main() -> anyhow::Result<()> {
    // `--daemon` runs the headless sampler and never touches eframe — the
    // GUI code stays paged out for the lifetime of the process.
    if std::env::args().skip(1).any(|a| a == "--daemon") {
        return daemon::run();
    }

    // Make sure a background sampler is running so this launch (and the
    // next one) sees fresh history. No-op if one is already alive.
    daemon::spawn_if_absent();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([480.0, 400.0])
            .with_title("rproc"),
        ..Default::default()
    };
    eframe::run_native(
        "rproc",
        native_options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}
