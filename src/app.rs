use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use slint::{ComponentHandle, ModelRc, SharedString, TimerMode, VecModel};

use crate::daemon;
use crate::monitor::Sampler;
use crate::monitor::startup::StartupSource;
use crate::monitor::{Snapshot, processes as procmon};
use crate::settings::Settings;
use crate::theme;
use crate::ui::context_menu::{self, Act, ContextMenu, Target};
use crate::ui::processes::RowRef;
use crate::ui::processes::properties::{
    ProcessPropertiesView, build_properties_view as proc_props,
};
use crate::ui::services::properties::ServicePropertiesView;
use crate::ui::startup::properties::{
    StartupPropertiesView, build_properties_view as startup_props,
};
use crate::ui::widgets::{self, OpenTarget, format_bytes, format_duration};
use crate::ui::{performance, processes, services, settings as settings_ui, startup};
use crate::{MainWindow, MenuEntry, PathField, StatLine, Theme};

/// Which entity the Properties modal is showing. Each holds the cached heavy
/// lookup so the modal doesn't re-walk `/proc` or re-spawn `systemctl` per tick.
enum PropEntity {
    Process(ProcessPropertiesView),
    Service(ServicePropertiesView),
    Startup(StartupPropertiesView),
}

struct UiState {
    sampler: Sampler,
    settings: Settings,
    snapshot: Arc<Snapshot>,
    perf: performance::State,
    proc: processes::State,
    services: services::State,
    startup: startup::State,
    prop: Option<PropEntity>,
    ctx: ContextMenu,
}

pub fn run(settings: Settings) -> anyhow::Result<()> {
    let window = MainWindow::new()?;

    // Apply the persisted light/dark choice before the first render: the Slint
    // `Theme` global drives the `.slint` palette, the atomic drives the colors
    // the glue bakes into row/series data.
    let dark = settings.dark_mode();
    theme::set_dark(dark);
    window.global::<Theme>().set_dark(dark);

    let sampler = Sampler::start(
        settings.refresh_handle(),
        settings.attribution_handle(),
        settings.gpu_handle(),
    );

    let state = Rc::new(RefCell::new(UiState {
        sampler,
        settings: settings.clone(),
        snapshot: Arc::new(Snapshot::default()),
        perf: performance::State::default(),
        proc: processes::State::new(),
        services: services::State::default(),
        startup: startup::State::default(),
        prop: None,
        ctx: ContextMenu::default(),
    }));

    // Optional: open straight onto a given tab (handy for measuring RAM of a
    // specific view). Defaults to Performance.
    if let Ok(t) = std::env::var("RPROC_TAB")
        && let Ok(t) = t.parse::<i32>()
    {
        window.set_tab(t);
    }

    install_callbacks(&window, &state);

    {
        let mut st = state.borrow_mut();
        tick(&window, &mut st);
    }

    // The UI polls the published snapshot; the sampler runs on its own thread at
    // the configured cadence. 250 ms keeps plots animating without spinning.
    let timer = slint::Timer::default();
    {
        let w = window.as_weak();
        let st = state.clone();
        timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            if let Some(window) = w.upgrade() {
                let mut s = st.borrow_mut();
                tick(&window, &mut s);
            }
        });
    }

    window.run()?;

    // Persist resolved icons on clean shutdown (see ui::icons::Resolver).
    state.borrow_mut().proc.save_icon_cache();
    Ok(())
}

fn tick(window: &MainWindow, st: &mut UiState) {
    st.sampler.set_processes_active(window.get_tab() == 0);
    // Paused: keep the displayed snapshot frozen and skip the periodic
    // re-render; interactions still render on demand via their callbacks.
    if window.get_paused() {
        return;
    }
    st.snapshot = st.sampler.snapshot();
    st.proc.flush_icon_cache_if_due();
    render(window, st);

    // Hand freed heap back to the OS each tick. Rebuilding the row models and
    // decoding icons allocates short-lived buffers; without this the glibc arena
    // keeps the high-water mark resident and RSS reads far above live data.
    #[cfg(target_os = "linux")]
    // SAFETY: malloc_trim takes no arguments referencing our memory and is
    // always safe to call; it only releases unused arena pages.
    unsafe {
        libc::malloc_trim(0);
    }
}

fn render(window: &MainWindow, st: &mut UiState) {
    match window.get_tab() {
        0 => processes::apply(window, &mut st.proc, &st.snapshot),
        1 => performance::apply(
            window,
            &st.perf,
            &st.snapshot,
            st.settings.attribution_enabled(),
        ),
        2 => startup::apply(window, &st.startup),
        3 => services::apply(window, &st.services),
        4 => settings_ui::apply(window, &st.settings),
        _ => {}
    }
    apply_prop(window, st);
}

fn ss(s: &str) -> SharedString {
    s.into()
}

fn install_callbacks(window: &MainWindow, state: &Rc<RefCell<UiState>>) {
    macro_rules! handler {
        (|$w:ident, $s:ident $(, $arg:ident : $ty:ty)*| $body:block) => {{
            let st = state.clone();
            let weak = window.as_weak();
            move |$($arg : $ty),*| {
                if let Some($w) = weak.upgrade() {
                    let mut $s = st.borrow_mut();
                    $body
                    render(&$w, &mut $s);
                }
            }
        }};
    }

    // --- Navigation ---
    window.on_select_tab(handler!(|w, s, t: i32| {
        w.set_tab(t);
    }));
    window.on_toggle_pause(handler!(|w, s| {
        let paused = !w.get_paused();
        w.set_paused(paused);
        s.sampler.set_paused(paused);
    }));

    // --- Performance ---
    window.on_perf_select(handler!(|w, s, id: SharedString| {
        s.perf.select(&id);
    }));
    window.on_perf_toggle_detail(handler!(|w, s| {
        s.perf.detail_collapsed = w.get_perf_detail_collapsed();
    }));
    // Hover updates only the crosshair/readout overlay, so they bypass the full
    // `render()` the macro emits and refresh just the overlay — a full re-render
    // per pointer move recreated the detail delegates and could feed back into
    // `mouse-x changed`, stalling the UI.
    window.on_perf_hovered({
        let st = state.clone();
        let weak = window.as_weak();
        move |x: f32| {
            if let Some(w) = weak.upgrade() {
                let mut s = st.borrow_mut();
                let span = (widgets::HISTORY_LEN - 1) as f64;
                s.perf.hover = Some((x as f64 * span).round().clamp(0.0, span));
                let enabled = s.settings.attribution_enabled();
                performance::apply_hover(&w, &s.perf, &s.snapshot, enabled);
            }
        }
    });
    window.on_perf_hover_cleared({
        let st = state.clone();
        let weak = window.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                let mut s = st.borrow_mut();
                s.perf.hover = None;
                let enabled = s.settings.attribution_enabled();
                performance::apply_hover(&w, &s.perf, &s.snapshot, enabled);
            }
        }
    });

    // --- Processes ---
    window.on_proc_filter_changed(handler!(|w, s, t: SharedString| {
        s.proc.filter = t.to_string();
    }));
    window.on_proc_sort(handler!(|w, s, k: i32| {
        s.proc.toggle_sort(k);
    }));
    window.on_proc_row_clicked(handler!(|w, s, i: i32| {
        s.proc.row_clicked(i as usize);
    }));
    window.on_proc_toggle_group(handler!(|w, s, g: SharedString| {
        s.proc.toggle_group(&g);
    }));
    window.on_proc_end_task(handler!(|w, s| {
        if let Some(pid) = s.proc.selected_pid.take() {
            let _ = procmon::terminate(pid);
        }
    }));
    window.on_proc_force_kill(handler!(|w, s| {
        if let Some(pid) = s.proc.selected_pid.take() {
            let _ = procmon::force_kill(pid);
        }
    }));
    window.on_proc_suspend_resume(handler!(|w, s| {
        if let Some(pid) = s.proc.selected_pid {
            let stopped = s
                .snapshot
                .processes
                .iter()
                .find(|p| p.pid == pid)
                .map(|p| matches!(p.status.as_str(), "Stop" | "Stopped"))
                .unwrap_or(false);
            if stopped {
                let _ = procmon::resume(pid);
            } else {
                let _ = procmon::suspend(pid);
            }
        }
    }));
    window.on_proc_properties(handler!(|w, s| {
        if let Some(pid) = s.proc.selected_pid {
            open_process_props(&mut s, pid);
        }
    }));
    window.on_proc_open_location(handler!(|w, s| {
        if let Some(pid) = s.proc.selected_pid
            && let Some(p) = s.snapshot.processes.iter().find(|p| p.pid == pid)
        {
            processes::open_in_file_manager(&p.exe);
        }
    }));
    window.on_proc_search(handler!(|w, s| {
        if let Some(pid) = s.proc.selected_pid
            && let Some(p) = s.snapshot.processes.iter().find(|p| p.pid == pid)
        {
            processes::open_search(&p.name);
        }
    }));
    window.on_proc_copy_pid(handler!(|w, s| {
        if let Some(pid) = s.proc.selected_pid {
            processes::copy_to_clipboard(&pid.to_string());
        }
    }));
    window.on_proc_end_all(handler!(|w, s| {
        if let Some(name) = s.proc.selected_group.take() {
            let pids: Vec<u32> = s
                .snapshot
                .processes
                .iter()
                .filter(|p| p.name == name)
                .map(|p| p.pid)
                .collect();
            for pid in pids {
                let _ = procmon::terminate(pid);
            }
        }
    }));
    window.on_proc_force_kill_all(handler!(|w, s| {
        if let Some(name) = s.proc.selected_group.take() {
            let pids: Vec<u32> = s
                .snapshot
                .processes
                .iter()
                .filter(|p| p.name == name)
                .map(|p| p.pid)
                .collect();
            for pid in pids {
                let _ = procmon::force_kill(pid);
            }
        }
    }));
    window.on_proc_group_properties(handler!(|w, s| {
        if let Some(name) = s.proc.selected_group.clone() {
            let main = s
                .snapshot
                .processes
                .iter()
                .filter(|p| p.name == name)
                .map(|p| p.pid)
                .min();
            if let Some(pid) = main {
                open_process_props(&mut s, pid);
            }
        }
    }));

    // --- Services ---
    window.on_svc_reload(handler!(|w, s| {
        s.services.refresh();
    }));
    window.on_svc_running_only_toggled(handler!(|w, s| {
        s.services.show_only_running = !s.services.show_only_running;
    }));
    window.on_svc_filter_changed(handler!(|w, s, t: SharedString| {
        s.services.filter = t.to_string();
    }));
    window.on_svc_action(handler!(|w, s, i: i32, a: SharedString| {
        s.services.action(i as usize, &a);
    }));
    window.on_svc_properties(handler!(|w, s, i: i32| {
        if let Some(svc) = s.services.entries.get(i as usize) {
            let view = ServicePropertiesView::fetch(svc.name.clone(), svc.scope.clone());
            s.prop = Some(PropEntity::Service(view));
        }
    }));

    // --- Startup ---
    window.on_start_reload(handler!(|w, s| {
        s.startup.reload();
    }));
    window.on_start_filter_changed(handler!(|w, s, t: SharedString| {
        s.startup.filter = t.to_string();
    }));
    window.on_start_toggle(handler!(|w, s, i: i32, e: bool| {
        s.startup.toggle(i as usize, e);
    }));
    window.on_start_properties(handler!(|w, s, i: i32| {
        let view = startup_props(&s.startup.entries, i as usize);
        s.prop = Some(PropEntity::Startup(view));
    }));
    window.on_start_open_desktop(handler!(|w, s, i: i32| {
        if let Some(e) = s.startup.entries.get(i as usize) {
            widgets::open_path(&e.path.to_string_lossy(), OpenTarget::Parent);
        }
    }));

    // --- Right-click context menu ---
    window.on_proc_row_context(handler!(|w, s, i: i32, x: f32, y: f32| {
        s.proc.row_clicked(i as usize);
        if let Some((target, built)) = proc_menu(&s, i as usize) {
            arm_ctx(&w, &mut s, target, built, x, y);
        }
    }));
    window.on_svc_row_context(handler!(|w, s, i: i32, x: f32, y: f32| {
        if let Some((target, built)) = svc_menu(&s, i as usize) {
            arm_ctx(&w, &mut s, target, built, x, y);
        }
    }));
    window.on_start_row_context(handler!(|w, s, i: i32, x: f32, y: f32| {
        if let Some((target, built)) = start_menu(&s, i as usize) {
            arm_ctx(&w, &mut s, target, built, x, y);
        }
    }));
    window.on_ctx_activate(handler!(|w, s, action: i32| {
        if let Some((target, act)) = s.ctx.resolve(action as usize) {
            context_dispatch(&mut s, target, act);
        }
        s.ctx.close();
        w.set_ctx_open(false);
    }));
    window.on_ctx_dismiss(handler!(|w, s| {
        s.ctx.close();
        w.set_ctx_open(false);
    }));

    // --- Settings ---
    window.on_cfg_set_refresh(handler!(|w, s, ms: i32| {
        s.settings.set_refresh_ms(ms as u64);
    }));
    window.on_cfg_daemon_toggled(handler!(|w, s, e: bool| {
        s.settings.set_daemon_enabled(e);
        if e {
            daemon::spawn_if_absent();
        } else {
            daemon::stop();
        }
    }));
    window.on_cfg_attribution_toggled(handler!(|w, s, e: bool| {
        s.settings.set_attribution_enabled(e);
    }));
    window.on_cfg_gpu_toggled(handler!(|w, s, e: bool| {
        s.settings.set_gpu_enabled(e);
    }));
    window.on_cfg_theme_toggled(handler!(|w, s, dark: bool| {
        s.settings.set_dark_mode(dark);
        theme::set_dark(dark);
        w.global::<Theme>().set_dark(dark);
    }));

    // --- Properties modal ---
    window.on_prop_close(handler!(|w, s| {
        s.prop = None;
    }));
    window.on_prop_reload(handler!(|w, s| {
        reload_prop(&mut s);
    }));
    window.on_open_path({
        // No state mutation / re-render needed — just launch the file manager.
        move |path: SharedString, parent: bool| {
            let target = if parent {
                OpenTarget::Parent
            } else {
                OpenTarget::Self_
            };
            widgets::open_path(&path, target);
        }
    });
}

fn open_process_props(st: &mut UiState, pid: u32) {
    if let Some(p) = st.snapshot.processes.iter().find(|p| p.pid == pid) {
        st.prop = Some(PropEntity::Process(proc_props(p)));
    }
}

type Menu = (ModelRc<MenuEntry>, Vec<Act>);

fn arm_ctx(w: &MainWindow, st: &mut UiState, target: Target, menu: Menu, x: f32, y: f32) {
    let (items, acts) = menu;
    st.ctx.arm(target, acts);
    w.set_ctx_items(items);
    w.set_ctx_x(x);
    w.set_ctx_y(y);
    w.set_ctx_open(true);
}

fn proc_menu(st: &UiState, i: usize) -> Option<(Target, Menu)> {
    match st.proc.row_ref(i)? {
        RowRef::Proc(pid) => {
            let p = st.snapshot.processes.iter().find(|p| p.pid == pid)?;
            let stopped = matches!(p.status.as_str(), "Stop" | "Stopped");
            let has_exe = !p.exe.is_empty() && std::path::Path::new(&p.exe).exists();
            let has_cmd = !p.cmd.is_empty();
            let mut b = context_menu::Builder::new();
            b.item("End task", Act::EndTask, true);
            b.item("Force kill", Act::ForceKill, true);
            b.item(
                if stopped { "Resume" } else { "Suspend" },
                Act::SuspendResume,
                true,
            );
            b.sep();
            b.item("Open file location", Act::OpenLocation, has_exe);
            b.item("Search online", Act::SearchOnline, true);
            b.sep();
            b.item("Copy PID", Act::CopyPid, true);
            b.item("Copy name", Act::CopyName, true);
            b.item("Copy command line", Act::CopyCmd, has_cmd);
            b.sep();
            b.item("Properties", Act::Properties, true);
            Some((Target::Process(pid), b.finish()))
        }
        RowRef::Group(name) => {
            let group: Vec<&_> = st
                .snapshot
                .processes
                .iter()
                .filter(|p| p.name == name)
                .collect();
            let n = group.len();
            let all_stopped = !group.is_empty()
                && group
                    .iter()
                    .all(|p| matches!(p.status.as_str(), "Stop" | "Stopped"));
            let main_exe = group
                .iter()
                .min_by_key(|p| p.pid)
                .map(|p| p.exe.clone())
                .unwrap_or_default();
            let has_exe = !main_exe.is_empty() && std::path::Path::new(&main_exe).exists();
            let mut b = context_menu::Builder::new();
            b.item(format!("End all ({n})"), Act::EndAll, true);
            b.item(format!("Force kill all ({n})"), Act::ForceKillAll, true);
            let suspend = if all_stopped {
                format!("Resume all ({n})")
            } else {
                format!("Suspend all ({n})")
            };
            b.item(suspend, Act::SuspendResumeAll, true);
            b.sep();
            b.item("Open file location", Act::OpenLocation, has_exe);
            b.item("Search online", Act::SearchOnline, true);
            b.sep();
            b.item("Copy name", Act::CopyName, true);
            b.item("Copy all PIDs", Act::CopyAllPids, true);
            b.sep();
            b.item("Properties", Act::Properties, true);
            Some((Target::Group(name), b.finish()))
        }
        RowRef::Section => None,
    }
}

fn svc_menu(st: &UiState, i: usize) -> Option<(Target, Menu)> {
    st.services.entries.get(i)?;
    let mut b = context_menu::Builder::new();
    b.item("Start", Act::SvcStart, true);
    b.item("Stop", Act::SvcStop, true);
    b.item("Restart", Act::SvcRestart, true);
    b.sep();
    b.item("Copy unit name", Act::CopyUnit, true);
    b.sep();
    b.item("Properties", Act::Properties, true);
    Some((Target::Service(i), b.finish()))
}

fn start_menu(st: &UiState, i: usize) -> Option<(Target, Menu)> {
    let e = st.startup.entries.get(i)?;
    let is_desktop = matches!(
        e.source,
        StartupSource::UserAutostart | StartupSource::SystemAutostart
    );
    let mut b = context_menu::Builder::new();
    b.item("Copy name", Act::CopyName, true);
    if is_desktop {
        b.item("Open .desktop file", Act::OpenDesktop, true);
    }
    b.sep();
    b.item("Properties", Act::Properties, true);
    Some((Target::Startup(i), b.finish()))
}

fn context_dispatch(st: &mut UiState, target: Target, act: Act) {
    match target {
        Target::Process(pid) => match act {
            Act::EndTask => {
                let _ = procmon::terminate(pid);
            }
            Act::ForceKill => {
                let _ = procmon::force_kill(pid);
            }
            Act::SuspendResume => {
                let stopped = st
                    .snapshot
                    .processes
                    .iter()
                    .find(|p| p.pid == pid)
                    .map(|p| matches!(p.status.as_str(), "Stop" | "Stopped"))
                    .unwrap_or(false);
                if stopped {
                    let _ = procmon::resume(pid);
                } else {
                    let _ = procmon::suspend(pid);
                }
            }
            Act::OpenLocation => {
                if let Some(p) = st.snapshot.processes.iter().find(|p| p.pid == pid) {
                    processes::open_in_file_manager(&p.exe);
                }
            }
            Act::SearchOnline => {
                if let Some(p) = st.snapshot.processes.iter().find(|p| p.pid == pid) {
                    processes::open_search(&p.name);
                }
            }
            Act::CopyPid => processes::copy_to_clipboard(&pid.to_string()),
            Act::CopyName => {
                if let Some(p) = st.snapshot.processes.iter().find(|p| p.pid == pid) {
                    processes::copy_to_clipboard(&p.name);
                }
            }
            Act::CopyCmd => {
                if let Some(p) = st.snapshot.processes.iter().find(|p| p.pid == pid) {
                    processes::copy_to_clipboard(&p.cmd);
                }
            }
            Act::Properties => open_process_props(st, pid),
            _ => {}
        },
        Target::Group(name) => dispatch_group(st, &name, act),
        Target::Service(i) => match act {
            Act::SvcStart => st.services.action(i, "start"),
            Act::SvcStop => st.services.action(i, "stop"),
            Act::SvcRestart => st.services.action(i, "restart"),
            Act::CopyUnit => {
                if let Some(svc) = st.services.entries.get(i) {
                    processes::copy_to_clipboard(&svc.name.clone());
                }
            }
            Act::Properties => {
                if let Some(svc) = st.services.entries.get(i) {
                    let view = ServicePropertiesView::fetch(svc.name.clone(), svc.scope.clone());
                    st.prop = Some(PropEntity::Service(view));
                }
            }
            _ => {}
        },
        Target::Startup(i) => match act {
            Act::CopyName => {
                if let Some(e) = st.startup.entries.get(i) {
                    let copy = if e.name.is_empty() {
                        e.path
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    } else {
                        e.name.clone()
                    };
                    processes::copy_to_clipboard(&copy);
                }
            }
            Act::OpenDesktop => {
                if let Some(e) = st.startup.entries.get(i) {
                    widgets::open_path(&e.path.to_string_lossy(), OpenTarget::Parent);
                }
            }
            Act::Properties => {
                let view = startup_props(&st.startup.entries, i);
                st.prop = Some(PropEntity::Startup(view));
            }
            _ => {}
        },
    }
}

fn dispatch_group(st: &mut UiState, name: &str, act: Act) {
    let pids: Vec<u32> = st
        .snapshot
        .processes
        .iter()
        .filter(|p| p.name == name)
        .map(|p| p.pid)
        .collect();
    match act {
        Act::EndAll => {
            for pid in &pids {
                let _ = procmon::terminate(*pid);
            }
        }
        Act::ForceKillAll => {
            for pid in &pids {
                let _ = procmon::force_kill(*pid);
            }
        }
        Act::SuspendResumeAll => {
            let all_stopped = !pids.is_empty()
                && st
                    .snapshot
                    .processes
                    .iter()
                    .filter(|p| p.name == name)
                    .all(|p| matches!(p.status.as_str(), "Stop" | "Stopped"));
            for pid in &pids {
                if all_stopped {
                    let _ = procmon::resume(*pid);
                } else {
                    let _ = procmon::suspend(*pid);
                }
            }
        }
        Act::OpenLocation => {
            if let Some(p) = st
                .snapshot
                .processes
                .iter()
                .filter(|p| p.name == name)
                .min_by_key(|p| p.pid)
            {
                processes::open_in_file_manager(&p.exe);
            }
        }
        Act::SearchOnline => processes::open_search(name),
        Act::CopyName => processes::copy_to_clipboard(name),
        Act::CopyAllPids => {
            let joined = pids
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            processes::copy_to_clipboard(&joined);
        }
        Act::Properties => {
            if let Some(pid) = pids.iter().copied().min() {
                open_process_props(st, pid);
            }
        }
        _ => {}
    }
}

fn reload_prop(st: &mut UiState) {
    match &st.prop {
        Some(PropEntity::Process(v)) => {
            let pid = v.pid;
            open_process_props(st, pid);
        }
        Some(PropEntity::Service(v)) => {
            let (name, scope) = (v.name.clone(), v.scope.clone());
            st.prop = Some(PropEntity::Service(ServicePropertiesView::fetch(
                name, scope,
            )));
        }
        Some(PropEntity::Startup(v)) => {
            let idx = v.idx;
            let view = startup_props(&st.startup.entries, idx);
            st.prop = Some(PropEntity::Startup(view));
        }
        None => {}
    }
}

struct PropContent {
    title: String,
    stats: Vec<StatLine>,
    paths: Vec<PathField>,
    free_label: String,
    free_text: String,
    can_reload: bool,
}

fn apply_prop(window: &MainWindow, st: &mut UiState) {
    let mut content: Option<PropContent> = None;
    let mut close = false;
    match &st.prop {
        Some(PropEntity::Process(v)) => match st.snapshot.processes.iter().find(|p| p.pid == v.pid)
        {
            Some(p) => content = Some(process_content(v, p)),
            None => close = true,
        },
        Some(PropEntity::Service(v)) => content = Some(service_content(v)),
        Some(PropEntity::Startup(v)) => content = Some(startup_content(v, &st.startup.entries)),
        None => {}
    }
    if close {
        st.prop = None;
    }
    match content {
        Some(c) => {
            window.set_prop_title(ss(&c.title));
            window.set_prop_stats(ModelRc::new(VecModel::from(c.stats)));
            window.set_prop_paths(ModelRc::new(VecModel::from(c.paths)));
            window.set_prop_free_label(ss(&c.free_label));
            window.set_prop_free_text(ss(&c.free_text));
            window.set_prop_can_reload(c.can_reload);
            window.set_prop_open(true);
        }
        None => window.set_prop_open(false),
    }
}

fn stat(label: &str, value: &str) -> StatLine {
    StatLine {
        label: ss(label),
        value: ss(value),
        separator: false,
    }
}

fn separator() -> StatLine {
    StatLine {
        label: ss(""),
        value: ss(""),
        separator: true,
    }
}

fn path_field(label: &str, path: &str, parent: bool, compact: bool) -> PathField {
    PathField {
        label: ss(label),
        path: ss(path),
        compact,
        open_parent: parent,
    }
}

fn process_content(view: &ProcessPropertiesView, p: &procmon::ProcInfo) -> PropContent {
    let uptime = SystemTime::now()
        .duration_since(UNIX_EPOCH + Duration::from_secs(p.start_time))
        .ok()
        .map(|d| format_duration(d.as_secs()));

    let mut stats = vec![
        stat("PID", &p.pid.to_string()),
        stat(
            "Parent PID",
            &p.parent
                .map(|v| v.to_string())
                .unwrap_or_else(|| "—".into()),
        ),
        stat("User", if p.user.is_empty() { "—" } else { &p.user }),
        stat("Status", &p.status),
        separator(),
        stat("CPU", &format!("{:.1}%", p.cpu_pct)),
        stat("Memory (RSS)", &format_bytes(p.mem_bytes)),
        stat("Virtual memory", &format_bytes(p.virt_bytes)),
        stat("Threads", &p.threads.to_string()),
    ];
    if let Some(fds) = view.fd_count {
        stats.push(stat("Open file descriptors", &fds.to_string()));
    }
    if let Some(up) = &uptime {
        stats.push(stat("Running for", up));
    }

    let mut paths = Vec::new();
    if !p.exe.is_empty() {
        paths.push(path_field("Executable", &p.exe, true, false));
    }
    if let Some(cwd) = &view.cwd {
        paths.push(path_field("Working directory", cwd, false, false));
    }
    for c in &view.configs {
        paths.push(path_field("Config", &c.to_string_lossy(), false, true));
    }

    PropContent {
        title: format!("Properties: {}", p.name),
        stats,
        paths,
        free_label: if p.cmd.is_empty() {
            String::new()
        } else {
            "Command line".into()
        },
        free_text: p.cmd.clone(),
        can_reload: true,
    }
}

fn service_content(view: &ServicePropertiesView) -> PropContent {
    let d = &view.data;
    let mut stats = vec![
        stat("Unit", &view.name),
        stat("Scope", services::scope_str(&view.scope)),
    ];
    push_if(&mut stats, "Description", &d.description);
    stats.push(separator());
    push_if(&mut stats, "Load", &d.load_state);
    push_if(&mut stats, "Active", &d.active_state);
    push_if(&mut stats, "Sub", &d.sub_state);
    push_if(&mut stats, "Unit file state", &d.unit_file_state);
    if !d.main_pid.is_empty() && d.main_pid != "0" {
        stats.push(stat("Main PID", &d.main_pid));
    }
    push_if(&mut stats, "User", &d.user);
    if !d.memory_current.is_empty()
        && d.memory_current != "[not set]"
        && d.memory_current != "0"
        && let Ok(bytes) = d.memory_current.parse::<u64>()
    {
        stats.push(stat("Memory", &format_bytes(bytes)));
    }
    if !d.tasks_current.is_empty() && d.tasks_current != "[not set]" {
        stats.push(stat("Tasks", &d.tasks_current));
    }

    let mut paths = Vec::new();
    if !d.fragment_path.is_empty() {
        paths.push(path_field("Unit file", &d.fragment_path, true, false));
    }
    for p in &d.drop_in_paths {
        paths.push(path_field("Drop-in", p, false, true));
    }
    if !d.working_directory.is_empty() && d.working_directory != "[not set]" {
        paths.push(path_field(
            "Working directory",
            &d.working_directory,
            false,
            false,
        ));
    }

    PropContent {
        title: format!("Properties: {}", view.name),
        stats,
        paths,
        free_label: if d.exec_start.is_empty() {
            String::new()
        } else {
            "ExecStart".into()
        },
        free_text: d.exec_start.clone(),
        can_reload: true,
    }
}

fn startup_content(
    view: &StartupPropertiesView,
    entries: &[crate::monitor::startup::StartupEntry],
) -> PropContent {
    let Some(e) = entries.get(view.idx) else {
        return PropContent {
            title: "Properties".into(),
            stats: Vec::new(),
            paths: Vec::new(),
            free_label: String::new(),
            free_text: String::new(),
            can_reload: false,
        };
    };
    let title = startup::entry_name(e);
    let is_systemd = matches!(
        e.source,
        StartupSource::SystemdSystem | StartupSource::SystemdUser
    );

    let mut stats = vec![
        stat("Name", &title),
        stat("Source", startup::scope_badge(&e.source)),
    ];
    push_if(&mut stats, "Description", &e.comment);
    stats.push(stat(
        "Boot time",
        &crate::ui::startup::filter::format_boot_time(e.boot_time_ms),
    ));
    stats.push(stat(
        "State",
        if e.critical {
            "Protected (managed by systemd)"
        } else if e.enabled {
            "Enabled"
        } else {
            "Disabled"
        },
    ));
    stats.push(separator());

    let mut paths = Vec::new();
    let mut free_label = String::new();
    let mut free_text = String::new();

    if !is_systemd {
        paths.push(path_field(
            ".desktop file",
            &e.path.to_string_lossy(),
            true,
            false,
        ));
        if !e.exec.is_empty() {
            free_label = "Exec".into();
            free_text = e.exec.clone();
        }
        if !e.icon.is_empty() {
            stats.push(stat("Icon", &e.icon));
        }
    }

    if let Some(props) = &view.systemd {
        stats.push(stat("Unit", &e.exec));
        push_if(&mut stats, "Unit file state", &props.unit_file_state);
        push_if(&mut stats, "Active", &props.active_state);
        push_if(&mut stats, "Sub", &props.sub_state);
        if !props.main_pid.is_empty() && props.main_pid != "0" {
            stats.push(stat("Main PID", &props.main_pid));
        }
        push_if(&mut stats, "User", &props.user);
        if !props.fragment_path.is_empty() {
            paths.push(path_field("Unit file", &props.fragment_path, true, false));
        }
        for p in &props.drop_in_paths {
            paths.push(path_field("Drop-in", p, false, true));
        }
        if !props.working_directory.is_empty() && props.working_directory != "[not set]" {
            paths.push(path_field(
                "Working directory",
                &props.working_directory,
                false,
                false,
            ));
        }
        if !props.exec_start.is_empty() {
            free_label = "ExecStart".into();
            free_text = props.exec_start.clone();
        }
    }

    PropContent {
        title: format!("Properties: {title}"),
        stats,
        paths,
        free_label,
        free_text,
        can_reload: is_systemd,
    }
}

fn push_if(stats: &mut Vec<StatLine>, label: &str, value: &str) {
    if !value.is_empty() {
        stats.push(stat(label, value));
    }
}
