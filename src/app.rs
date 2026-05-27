use crate::monitor::Sampler;
use crate::settings::Settings;
use crate::theme;
use crate::ui;

#[derive(PartialEq, Copy, Clone)]
pub enum Tab {
    Processes,
    Performance,
    Startup,
    Services,
    Settings,
}

pub struct App {
    sampler: Sampler,
    pub tab: Tab,
    pub settings: Settings,
    pub processes: ui::processes::State,
    pub performance: ui::performance::State,
    pub startup: ui::startup::State,
    pub services: ui::services::State,
    pub settings_state: ui::settings::State,
}

impl App {
    pub fn new(settings: Settings) -> Self {
        Self {
            sampler: Sampler::start(settings.refresh_handle()),
            tab: Tab::Performance,
            settings,
            processes: ui::processes::State::new(),
            performance: ui::performance::State::default(),
            startup: ui::startup::State::default(),
            services: ui::services::State::default(),
            settings_state: ui::settings::State::default(),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Repaint roughly twice as often as the sampler ticks so plots animate
        // smoothly and the hover crosshair tracks the cursor.
        let refresh = self.settings.refresh_ms().max(50);
        ctx.request_repaint_after(std::time::Duration::from_millis(refresh / 2));

        let snap = self.sampler.snapshot();

        // Below this window width the sidebar collapses to icons-only so the
        // central panel keeps usable room. Threshold matches the point where
        // 220 px of sidebar + ~250 px cards + a viable detail pane stops fitting.
        let compact_sidebar = ctx.screen_rect().width() < 900.0;
        let (sidebar_width, sidebar_margin) = if compact_sidebar {
            (56.0, 6)
        } else {
            (220.0, 12)
        };

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(sidebar_width)
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR_BG)
                    .inner_margin(egui::Margin::same(sidebar_margin)),
            )
            .show(ctx, |ui| {
                ui::sidebar::show(ui, &mut self.tab, compact_sidebar);
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(20)),
            )
            .show(ctx, |ui| match self.tab {
                Tab::Processes => ui::processes::show(ui, &mut self.processes, &snap),
                Tab::Performance => ui::performance::show(ui, &mut self.performance, &snap),
                Tab::Startup => ui::startup::show(ui, &mut self.startup),
                Tab::Services => ui::services::show(ui, &mut self.services),
                Tab::Settings => ui::settings::show(ui, &mut self.settings_state, &self.settings),
            });
    }
}
