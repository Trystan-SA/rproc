use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use egui_extras::{Column, TableBuilder, TableRow};

use crate::daemon::storage;
use crate::monitor::{self, Snapshot};
use crate::theme;
use crate::ui::icons;
use crate::ui::widgets;

#[derive(Default, PartialEq, Copy, Clone, Debug)]
enum SortKey {
    Name,
    Pid,
    User,
    #[default]
    Cpu,
    Mem,
    Disk,
    Status,
}

impl SortKey {
    fn as_str(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Pid => "Pid",
            SortKey::User => "User",
            SortKey::Cpu => "Cpu",
            SortKey::Mem => "Mem",
            SortKey::Disk => "Disk",
            SortKey::Status => "Status",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "Name" => SortKey::Name,
            "Pid" => SortKey::Pid,
            "User" => SortKey::User,
            "Cpu" => SortKey::Cpu,
            "Mem" => SortKey::Mem,
            "Disk" => SortKey::Disk,
            "Status" => SortKey::Status,
            _ => return None,
        })
    }
}

fn sort_prefs_path() -> Option<PathBuf> {
    storage::cache_dir()
        .ok()
        .map(|d| d.join("processes_sort.txt"))
}

fn load_sort_prefs() -> Option<(SortKey, bool)> {
    let path = sort_prefs_path()?;
    load_sort_prefs_from(&path)
}

fn save_sort_prefs(sort: SortKey, descending: bool) {
    if let Some(path) = sort_prefs_path() {
        let _ = save_sort_prefs_to(&path, sort, descending);
    }
}

fn load_sort_prefs_from(path: &std::path::Path) -> Option<(SortKey, bool)> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut sort = None;
    let mut desc = None;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("sort=") {
            sort = SortKey::from_str(v.trim());
        } else if let Some(v) = line.strip_prefix("descending=") {
            desc = match v.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    Some((sort?, desc?))
}

fn save_sort_prefs_to(
    path: &std::path::Path,
    sort: SortKey,
    descending: bool,
) -> std::io::Result<()> {
    let content = format!("sort={}\ndescending={}\n", sort.as_str(), descending);
    std::fs::write(path, content)
}

/// Heavy lookups behind the Properties modal — each one walks `/proc` and
/// performs a fistful of syscalls. Computed when the modal opens, then
/// re-used until the user clicks Reload or opens a different PID.
pub struct ProcessPropertiesView {
    pub pid: u32,
    pub cwd: Option<String>,
    pub fd_count: Option<usize>,
    pub configs: Vec<PathBuf>,
}

pub struct State {
    sort: SortKey,
    descending: bool,
    selected_pid: Option<u32>,
    selected_group: Option<String>,
    expanded: HashSet<String>,
    filter: String,
    icons: icons::Resolver,
    properties: Option<ProcessPropertiesView>,
}

impl State {
    pub fn new() -> Self {
        let (sort, descending) = load_sort_prefs().unwrap_or((SortKey::Cpu, true));
        Self {
            sort,
            descending,
            selected_pid: None,
            selected_group: None,
            expanded: HashSet::new(),
            filter: String::new(),
            icons: icons::Resolver::new(),
            properties: None,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

struct Group<'a> {
    name: &'a str,
    procs: Vec<&'a monitor::processes::ProcInfo>,
    cpu_pct: f32,
    mem_bytes: u64,
    disk_bps: f64,
}

/// Group processes by name and roll up CPU / memory / disk totals. Keying on
/// `&str` (borrowed from `procs`) skips ~N `String` allocations per frame,
/// where N is the number of processes. The `matches` predicate filters before
/// grouping so empty groups don't appear in the result.
fn build_groups<'a>(
    procs: &'a [monitor::processes::ProcInfo],
    matches: &dyn Fn(&monitor::processes::ProcInfo) -> bool,
) -> Vec<Group<'a>> {
    let mut by_name: BTreeMap<&str, Vec<&monitor::processes::ProcInfo>> = BTreeMap::new();
    for p in procs {
        if matches(p) {
            by_name.entry(p.name.as_str()).or_default().push(p);
        }
    }
    by_name
        .into_iter()
        .map(|(name, procs)| {
            let cpu_pct = procs.iter().map(|p| p.cpu_pct).sum();
            let mem_bytes = procs.iter().map(|p| p.mem_bytes).sum();
            let disk_bps = procs
                .iter()
                .map(|p| p.disk_read_bps + p.disk_write_bps)
                .sum();
            Group {
                name,
                procs,
                cpu_pct,
                mem_bytes,
                disk_bps,
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Row<'a> {
    SectionHeader(&'static str),
    GroupHeader { g: &'a Group<'a>, expanded: bool },
    Single(&'a monitor::processes::ProcInfo),
    Child(&'a monitor::processes::ProcInfo),
}

/// Split the built groups into the "Apps" section (processes with a freedesktop
/// `.desktop` entry — launchable applications) and the background section
/// (daemons, kernel threads, helpers). Each section is sorted independently by
/// the caller so background processes can never rank above apps.
fn append_section<'a>(
    visible: &mut Vec<Row<'a>>,
    title: &'static str,
    groups: &'a [Group<'a>],
    filter_active: bool,
    expanded_set: &HashSet<String>,
) {
    if groups.is_empty() {
        return;
    }
    visible.push(Row::SectionHeader(title));
    for g in groups {
        if g.procs.len() == 1 {
            visible.push(Row::Single(g.procs[0]));
        } else {
            let expanded = filter_active || expanded_set.contains(g.name);
            visible.push(Row::GroupHeader { g, expanded });
            if expanded {
                for p in &g.procs {
                    visible.push(Row::Child(p));
                }
            }
        }
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut State, snap: &Snapshot) {
    // Title row + selection actions on the right.
    ui.horizontal(|ui| {
        ui.heading("Processes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(pid) = state.selected_pid {
                if ui.button("Force kill").clicked() {
                    let _ = monitor::processes::force_kill(pid);
                    state.selected_pid = None;
                }
                if ui.button("End task").clicked() {
                    let _ = monitor::processes::terminate(pid);
                    state.selected_pid = None;
                }
            } else if let Some(name) = state.selected_group.as_deref() {
                let pids: Vec<u32> = snap
                    .processes
                    .iter()
                    .filter(|p| p.name == name)
                    .map(|p| p.pid)
                    .collect();
                let n = pids.len();
                let force = ui.button(format!("Force kill ({n})")).clicked();
                let end = ui.button(format!("End task ({n})")).clicked();
                if force {
                    for pid in &pids {
                        let _ = monitor::processes::force_kill(*pid);
                    }
                    state.selected_group = None;
                } else if end {
                    for pid in &pids {
                        let _ = monitor::processes::terminate(*pid);
                    }
                    state.selected_group = None;
                }
            } else {
                ui.add_enabled(false, egui::Button::new("End task"));
            }
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut state.filter)
                    .hint_text("Filter by name or PID")
                    .desired_width(220.0),
            );
        });
    });

    ui.add_space(8.0);

    let filter = state.filter.trim().to_lowercase();
    let filter_active = !filter.is_empty();
    let matches = |p: &monitor::processes::ProcInfo| -> bool {
        if !filter_active {
            return true;
        }
        p.name.to_lowercase().contains(&filter) || p.pid.to_string().contains(&filter)
    };

    let groups: Vec<Group> = build_groups(&snap.processes, &matches);

    // Apps (have a .desktop entry) on top, background processes below. Each
    // section sorts on its own so a busy daemon never jumps above the apps.
    let (mut apps, mut services): (Vec<Group>, Vec<Group>) = groups.into_iter().partition(|g| {
        g.procs
            .iter()
            .any(|p| state.icons.has_desktop_entry(&p.name, &p.exe))
    });
    for section in [&mut apps, &mut services] {
        sort_groups(section, state.sort, state.descending);
        for g in section.iter_mut() {
            sort_children(&mut g.procs, state.sort, state.descending);
        }
    }

    let mut visible: Vec<Row> = Vec::with_capacity(snap.processes.len() + 2);
    append_section(&mut visible, "Apps", &apps, filter_active, &state.expanded);
    append_section(
        &mut visible,
        "Background processes",
        &services,
        filter_active,
        &state.expanded,
    );

    let total = snap.processes.len();
    let total_cpu_used: f32 = snap.system.cpu_total;
    let total_mem_used: f32 = snap.system.ram_used_pct;
    let selected_pid = state.selected_pid;
    let selected_group = state.selected_group.clone();

    // Deferred mutations: the rows closure can't easily reach `state` because
    // the table cell closures move captures around. Collect events here and
    // apply them after the table is done.
    let mut toggle_group: Option<String> = None;
    let mut click_pid: Option<u32> = None;
    let mut click_group: Option<String> = None;
    let mut open_properties_pid: Option<u32> = None;
    let icons = &mut state.icons;
    let sort = &mut state.sort;
    let descending = &mut state.descending;

    egui::Frame::new()
        .fill(theme::PANEL_BG)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            let header_height = 26.0;
            let row_height = 22.0;
            let section_height = 30.0;

            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(260.0).at_least(140.0).clip(true))
                .column(Column::initial(60.0).at_least(50.0))
                .column(Column::initial(100.0).at_least(70.0))
                .column(Column::initial(80.0).at_least(60.0))
                .column(Column::initial(100.0).at_least(80.0))
                .column(Column::initial(120.0).at_least(90.0))
                .column(Column::remainder().at_least(70.0))
                .header(header_height, |mut header| {
                    header.col(|ui| {
                        sortable_header(ui, "Name", SortKey::Name, sort, descending);
                    });
                    header.col(|ui| {
                        sortable_header(ui, "PID", SortKey::Pid, sort, descending);
                    });
                    header.col(|ui| {
                        sortable_header(ui, "User", SortKey::User, sort, descending);
                    });
                    header.col(|ui| {
                        sortable_header(
                            ui,
                            &format!("CPU  {:.0}%", total_cpu_used),
                            SortKey::Cpu,
                            sort,
                            descending,
                        );
                    });
                    header.col(|ui| {
                        sortable_header(
                            ui,
                            &format!("Memory  {:.0}%", total_mem_used),
                            SortKey::Mem,
                            sort,
                            descending,
                        );
                    });
                    header.col(|ui| {
                        sortable_header(ui, "Disk (R+W)", SortKey::Disk, sort, descending);
                    });
                    header.col(|ui| {
                        sortable_header(ui, "Status", SortKey::Status, sort, descending);
                    });
                })
                .body(|body| {
                    let heights = visible.iter().map(|r| match r {
                        Row::SectionHeader(_) => section_height,
                        _ => row_height,
                    });
                    body.heterogeneous_rows(heights, |mut row| {
                        let idx = row.index();
                        match visible[idx] {
                            Row::SectionHeader(title) => {
                                render_section_header(&mut row, title);
                            }
                            Row::GroupHeader { g, expanded } => {
                                row.set_selected(selected_group.as_deref() == Some(g.name));
                                render_group_header(
                                    &mut row,
                                    g,
                                    expanded,
                                    &mut toggle_group,
                                    icons,
                                );
                                let resp = row
                                    .response()
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if resp.clicked() || resp.secondary_clicked() {
                                    click_group = Some(g.name.to_string());
                                }
                                resp.context_menu(|ui| {
                                    group_context_menu(ui, g, &mut open_properties_pid)
                                });
                            }
                            Row::Single(p) => {
                                row.set_selected(selected_pid == Some(p.pid));
                                render_proc_row(&mut row, p, false, icons);
                                let resp = row
                                    .response()
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if resp.clicked() || resp.secondary_clicked() {
                                    click_pid = Some(p.pid);
                                }
                                resp.context_menu(|ui| {
                                    proc_context_menu(ui, p, &mut open_properties_pid)
                                });
                            }
                            Row::Child(p) => {
                                row.set_selected(selected_pid == Some(p.pid));
                                render_proc_row(&mut row, p, true, icons);
                                let resp = row
                                    .response()
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if resp.clicked() || resp.secondary_clicked() {
                                    click_pid = Some(p.pid);
                                }
                                resp.context_menu(|ui| {
                                    proc_context_menu(ui, p, &mut open_properties_pid)
                                });
                            }
                        }
                    });
                });
        });

    if let Some(name) = toggle_group
        && !state.expanded.remove(&name)
    {
        state.expanded.insert(name);
    }
    if let Some(pid) = click_pid {
        state.selected_pid = Some(pid);
        state.selected_group = None;
    } else if let Some(name) = click_group {
        state.selected_group = Some(name);
        state.selected_pid = None;
    }
    if let Some(pid) = open_properties_pid {
        let same = matches!(&state.properties, Some(v) if v.pid == pid);
        if !same && let Some(p) = snap.processes.iter().find(|p| p.pid == pid) {
            state.properties = Some(build_properties_view(p));
        }
    }
    render_properties_window(ui.ctx(), &mut state.properties, snap);

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{total} processes"))
                .color(theme::TEXT_DIM)
                .small(),
        );
        if !snap.ready {
            ui.label(
                egui::RichText::new("  · sampling…")
                    .color(theme::TEXT_DIM)
                    .small(),
            );
        }
    });
}

fn render_section_header(row: &mut TableRow<'_, '_>, title: &str) {
    row.col(|ui| {
        ui.add_space(8.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new(title.to_uppercase())
                    .color(theme::ACCENT)
                    .strong(),
            )
            .selectable(false),
        );
    });
    // Leave the remaining six columns blank — the header is a divider, not a
    // data row.
    for _ in 0..6 {
        row.col(|_| {});
    }
}

fn render_group_header(
    row: &mut TableRow<'_, '_>,
    g: &Group<'_>,
    expanded: bool,
    toggle: &mut Option<String>,
    icons: &mut icons::Resolver,
) {
    let icon_uri = g
        .procs
        .first()
        .and_then(|p| icons.icon_uri(&p.name, &p.exe));
    row.col(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (rect, arrow_resp) =
            ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::click());
        let dir = if expanded {
            CaretDir::Down
        } else {
            CaretDir::Right
        };
        draw_caret(ui.painter(), rect, theme::TEXT, dir);
        let arrow_resp = arrow_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        draw_icon(ui, icon_uri.as_deref());
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("{}  ({})", g.name, g.procs.len())).strong(),
            )
            .truncate()
            .selectable(false),
        );
        if arrow_resp.clicked() {
            *toggle = Some(g.name.to_string());
        }
    });
    row.col(|_| {});
    row.col(|_| {});
    row.col(|ui| {
        ui.label(egui::RichText::new(format_pct(g.cpu_pct)).strong());
    });
    row.col(|ui| {
        ui.label(egui::RichText::new(widgets::format_bytes(g.mem_bytes)).strong());
    });
    row.col(|ui| {
        ui.label(egui::RichText::new(widgets::format_bps(g.disk_bps)).strong());
    });
    row.col(|_| {});
}

fn render_proc_row(
    row: &mut TableRow<'_, '_>,
    p: &monitor::processes::ProcInfo,
    indent: bool,
    icons: &mut icons::Resolver,
) {
    let icon_uri = icons.icon_uri(&p.name, &p.exe);
    row.col(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if indent {
            ui.add_space(20.0);
        }
        draw_icon(ui, icon_uri.as_deref());
        let resp = ui.add(egui::Label::new(&p.name).truncate().selectable(false));
        resp.on_hover_text(if p.cmd.is_empty() { &p.exe } else { &p.cmd });
    });
    row.col(|ui| {
        ui.label(p.pid.to_string());
    });
    row.col(|ui| {
        ui.label(&p.user);
    });
    row.col(|ui| {
        ui.label(format_pct(p.cpu_pct));
    });
    row.col(|ui| {
        ui.label(widgets::format_bytes(p.mem_bytes));
    });
    row.col(|ui| {
        let combined = p.disk_read_bps + p.disk_write_bps;
        let resp = ui.label(widgets::format_bps(combined));
        resp.on_hover_text(format!(
            "Read: {}\nWrite: {}",
            widgets::format_bps(p.disk_read_bps),
            widgets::format_bps(p.disk_write_bps),
        ));
    });
    row.col(|ui| {
        ui.label(egui::RichText::new(&p.status).color(status_color(&p.status)));
    });
}

fn sortable_header(
    ui: &mut egui::Ui,
    label: &str,
    key: SortKey,
    sort: &mut SortKey,
    descending: &mut bool,
) {
    let is_active = *sort == key;
    let color = if is_active {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };
    let dir = if !is_active || *descending {
        CaretDir::Down
    } else {
        CaretDir::Up
    };

    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(egui::Label::new(egui::RichText::new(label).strong()).selectable(false));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
            draw_caret(ui.painter(), rect, color, dir);
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    if resp.clicked() {
        if *sort == key {
            *descending = !*descending;
        } else {
            *sort = key;
            *descending = matches!(key, SortKey::Cpu | SortKey::Mem | SortKey::Disk);
        }
        save_sort_prefs(*sort, *descending);
    }
}

#[derive(Copy, Clone)]
enum CaretDir {
    Down,
    Up,
    Right,
}

fn draw_caret(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32, dir: CaretDir) {
    let c = rect.center();
    let half_w = rect.width() * 0.5;
    let half_h = rect.height() * 0.35;
    let pts = match dir {
        CaretDir::Down => vec![
            egui::pos2(c.x - half_w, c.y - half_h),
            egui::pos2(c.x + half_w, c.y - half_h),
            egui::pos2(c.x, c.y + half_h),
        ],
        CaretDir::Up => vec![
            egui::pos2(c.x - half_w, c.y + half_h),
            egui::pos2(c.x + half_w, c.y + half_h),
            egui::pos2(c.x, c.y - half_h),
        ],
        CaretDir::Right => vec![
            egui::pos2(c.x - half_h, c.y - half_w),
            egui::pos2(c.x - half_h, c.y + half_w),
            egui::pos2(c.x + half_h, c.y),
        ],
    };
    painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

fn sort_groups(groups: &mut [Group], key: SortKey, desc: bool) {
    groups.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Pid => a
                .procs
                .iter()
                .map(|p| p.pid)
                .min()
                .cmp(&b.procs.iter().map(|p| p.pid).min()),
            SortKey::User => a
                .procs
                .first()
                .map(|p| p.user.as_str())
                .cmp(&b.procs.first().map(|p| p.user.as_str())),
            SortKey::Cpu => a
                .cpu_pct
                .partial_cmp(&b.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortKey::Mem => a.mem_bytes.cmp(&b.mem_bytes),
            SortKey::Disk => a
                .disk_bps
                .partial_cmp(&b.disk_bps)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortKey::Status => a
                .procs
                .first()
                .map(|p| p.status.as_str())
                .cmp(&b.procs.first().map(|p| p.status.as_str())),
        };
        if desc { ord.reverse() } else { ord }
    });
}

fn sort_children(rows: &mut [&monitor::processes::ProcInfo], key: SortKey, desc: bool) {
    rows.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::User => a.user.cmp(&b.user),
            SortKey::Cpu => a
                .cpu_pct
                .partial_cmp(&b.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortKey::Mem => a.mem_bytes.cmp(&b.mem_bytes),
            SortKey::Disk => {
                let ax = a.disk_read_bps + a.disk_write_bps;
                let bx = b.disk_read_bps + b.disk_write_bps;
                ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
            }
            SortKey::Status => a.status.cmp(&b.status),
        };
        if desc { ord.reverse() } else { ord }
    });
}

fn format_pct(v: f32) -> String {
    if v < 0.05 {
        "0%".into()
    } else if v < 10.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}

fn proc_context_menu(
    ui: &mut egui::Ui,
    p: &monitor::processes::ProcInfo,
    open_properties_pid: &mut Option<u32>,
) {
    ui.set_min_width(200.0);
    if ui.button("End task").clicked() {
        let _ = monitor::processes::terminate(p.pid);
        ui.close();
    }
    if ui.button("Force kill").clicked() {
        let _ = monitor::processes::force_kill(p.pid);
        ui.close();
    }
    let is_stopped = matches!(p.status.as_str(), "Stop" | "Stopped");
    let suspend_label = if is_stopped { "Resume" } else { "Suspend" };
    if ui.button(suspend_label).clicked() {
        if is_stopped {
            let _ = monitor::processes::resume(p.pid);
        } else {
            let _ = monitor::processes::suspend(p.pid);
        }
        ui.close();
    }
    ui.separator();
    let exe_path = std::path::Path::new(&p.exe);
    let has_exe = !p.exe.is_empty() && exe_path.exists();
    if ui
        .add_enabled(has_exe, egui::Button::new("Open file location"))
        .clicked()
    {
        open_in_file_manager(&p.exe);
        ui.close();
    }
    if ui.button("Search online  \u{2197}").clicked() {
        open_search(&p.name);
        ui.close();
    }
    ui.separator();
    if ui.button("Copy PID").clicked() {
        ui.ctx().copy_text(p.pid.to_string());
        ui.close();
    }
    if ui.button("Copy name").clicked() {
        ui.ctx().copy_text(p.name.clone());
        ui.close();
    }
    if ui
        .add_enabled(!p.cmd.is_empty(), egui::Button::new("Copy command line"))
        .clicked()
    {
        ui.ctx().copy_text(p.cmd.clone());
        ui.close();
    }
    ui.separator();
    if ui.button("Properties").clicked() {
        *open_properties_pid = Some(p.pid);
        ui.close();
    }
}

fn group_context_menu(ui: &mut egui::Ui, g: &Group<'_>, open_properties_pid: &mut Option<u32>) {
    ui.set_min_width(220.0);
    let n = g.procs.len();
    // Lowest PID ≈ the parent/oldest in the group — use it as the "main"
    // process for actions that target a single representative (Properties,
    // Open file location).
    let main = g.procs.iter().min_by_key(|p| p.pid).copied();

    if ui.button(format!("End all ({n})")).clicked() {
        for p in &g.procs {
            let _ = monitor::processes::terminate(p.pid);
        }
        ui.close();
    }
    if ui.button(format!("Force kill all ({n})")).clicked() {
        for p in &g.procs {
            let _ = monitor::processes::force_kill(p.pid);
        }
        ui.close();
    }
    let all_stopped = g
        .procs
        .iter()
        .all(|p| matches!(p.status.as_str(), "Stop" | "Stopped"));
    let suspend_label = if all_stopped {
        format!("Resume all ({n})")
    } else {
        format!("Suspend all ({n})")
    };
    if ui.button(suspend_label).clicked() {
        for p in &g.procs {
            if all_stopped {
                let _ = monitor::processes::resume(p.pid);
            } else {
                let _ = monitor::processes::suspend(p.pid);
            }
        }
        ui.close();
    }
    ui.separator();
    if let Some(p) = main {
        let exe_path = std::path::Path::new(&p.exe);
        let has_exe = !p.exe.is_empty() && exe_path.exists();
        if ui
            .add_enabled(has_exe, egui::Button::new("Open file location"))
            .clicked()
        {
            open_in_file_manager(&p.exe);
            ui.close();
        }
    }
    if ui.button("Search online  \u{2197}").clicked() {
        open_search(g.name);
        ui.close();
    }
    ui.separator();
    if ui.button("Copy name").clicked() {
        ui.ctx().copy_text(g.name.to_string());
        ui.close();
    }
    if ui.button("Copy all PIDs").clicked() {
        let pids = g
            .procs
            .iter()
            .map(|p| p.pid.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        ui.ctx().copy_text(pids);
        ui.close();
    }
    if let Some(p) = main {
        ui.separator();
        if ui.button("Properties (main process)").clicked() {
            *open_properties_pid = Some(p.pid);
            ui.close();
        }
    }
}

fn open_in_file_manager(exe: &str) {
    let path = std::path::Path::new(exe);
    let target = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

fn open_search(name: &str) {
    let q = format!("linux process {name}");
    let url = format!("https://www.google.com/search?q={}", url_encode(&q));
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Resolve cwd, fd count, and well-known config paths once. The result is
/// stable enough (working dir + fd count drift slowly, configs almost never)
/// that we don't refresh on every frame — the modal exposes a Reload button.
fn build_properties_view(p: &monitor::processes::ProcInfo) -> ProcessPropertiesView {
    ProcessPropertiesView {
        pid: p.pid,
        cwd: monitor::processes::read_cwd(p.pid),
        fd_count: monitor::processes::read_fd_count(p.pid),
        configs: monitor::processes::find_config_paths(&p.name, &p.exe),
    }
}

fn render_properties_window(
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

fn draw_icon(ui: &mut egui::Ui, uri: Option<&str>) {
    let Some(uri) = uri else { return };
    let size = egui::Vec2::splat(16.0);
    let image = egui::Image::new(uri.to_string())
        .fit_to_exact_size(size)
        .maintain_aspect_ratio(true)
        .show_loading_spinner(false);
    // Skip silently on decode errors / missing format support so we don't show
    // a broken-image glyph in the row.
    if image.load_for_size(ui.ctx(), size).is_err() {
        return;
    }
    ui.add(image);
}

fn status_color(s: &str) -> egui::Color32 {
    match s {
        "Run" | "Running" => theme::OK,
        "Sleep" | "Sleeping" | "Idle" => theme::TEXT_DIM,
        "Waiting" | "Stop" | "Stopped" => theme::WARN,
        "Zombie" | "Dead" => theme::ERR,
        _ => theme::TEXT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_key_roundtrip_all_variants() {
        // Every variant must roundtrip through the on-disk encoding,
        // otherwise saving today's sort silently resets to the default
        // tomorrow.
        for k in [
            SortKey::Name,
            SortKey::Pid,
            SortKey::User,
            SortKey::Cpu,
            SortKey::Mem,
            SortKey::Disk,
            SortKey::Status,
        ] {
            let s = k.as_str();
            assert_eq!(SortKey::from_str(s), Some(k), "roundtrip for {s}");
        }
    }

    #[test]
    fn sort_key_from_str_rejects_unknown() {
        assert_eq!(SortKey::from_str(""), None);
        assert_eq!(SortKey::from_str("not-a-key"), None);
        // Case-sensitive — we control both sides of the format.
        assert_eq!(SortKey::from_str("cpu"), None);
    }

    #[test]
    fn sort_prefs_roundtrip_via_file() {
        let dir = std::env::temp_dir().join(format!(
            "rproc-sort-prefs-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prefs.txt");
        save_sort_prefs_to(&path, SortKey::Mem, false).unwrap();
        let loaded = load_sort_prefs_from(&path);
        assert_eq!(loaded, Some((SortKey::Mem, false)));

        save_sort_prefs_to(&path, SortKey::Cpu, true).unwrap();
        assert_eq!(load_sort_prefs_from(&path), Some((SortKey::Cpu, true)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sort_prefs_missing_file_returns_none() {
        let bogus = std::path::Path::new("/nonexistent/rproc/prefs.txt.does-not-exist");
        assert_eq!(load_sort_prefs_from(bogus), None);
    }

    #[test]
    fn sort_prefs_partial_file_returns_none() {
        // We refuse to apply a half-saved file rather than silently
        // defaulting one half — the user would never notice.
        let dir = std::env::temp_dir().join(format!(
            "rproc-sort-partial-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("partial.txt");
        std::fs::write(&path, "sort=Cpu\n").unwrap();
        assert_eq!(load_sort_prefs_from(&path), None);

        std::fs::write(&path, "descending=true\n").unwrap();
        assert_eq!(load_sort_prefs_from(&path), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_pct_low_values_show_decimal() {
        // Below 0.05 collapses to 0% so the column doesn't churn between
        // 0.0% / 0.1% for idle rows on every frame.
        assert_eq!(format_pct(0.0), "0%");
        assert_eq!(format_pct(0.04), "0%");
        assert_eq!(format_pct(0.5), "0.5%");
        assert_eq!(format_pct(9.9), "9.9%");
    }

    #[test]
    fn format_pct_high_values_round_to_int() {
        assert_eq!(format_pct(10.0), "10%");
        assert_eq!(format_pct(42.49), "42%");
        assert_eq!(format_pct(100.0), "100%");
    }

    #[test]
    fn url_encode_passes_through_unreserved() {
        // RFC 3986 unreserved set: A-Z a-z 0-9 - _ . ~
        assert_eq!(url_encode("Hello-World_1.2~3"), "Hello-World_1.2~3");
    }

    #[test]
    fn url_encode_spaces_become_plus() {
        // We're building a Google query URL — spaces are application/x-www-form-urlencoded.
        assert_eq!(url_encode("linux process firefox"), "linux+process+firefox");
    }

    #[test]
    fn url_encode_special_chars_become_percent_hex() {
        assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
        assert_eq!(url_encode("?#/"), "%3F%23%2F");
    }

    #[test]
    fn url_encode_non_ascii_byte_wise() {
        // UTF-8 byte for é (0xC3 0xA9) → "%C3%A9"
        assert_eq!(url_encode("é"), "%C3%A9");
    }

    #[test]
    fn status_color_known_states_distinct_from_default() {
        // Regression: every known status string should map to a non-default
        // colour (otherwise the column loses its visual cue).
        let default = theme::TEXT;
        assert_ne!(status_color("Running"), default);
        assert_ne!(status_color("Idle"), default);
        assert_ne!(status_color("Stopped"), default);
        assert_ne!(status_color("Zombie"), default);
        // Unknown still hits default — guard against accidental match-all.
        assert_eq!(status_color("XyzUnknown"), default);
    }

    // --- grouping & sorting regression tests ---

    fn make_proc(
        pid: u32,
        name: &str,
        cpu_pct: f32,
        mem_bytes: u64,
        disk: f64,
        user: &str,
        status: &str,
    ) -> monitor::processes::ProcInfo {
        monitor::processes::ProcInfo {
            pid,
            name: name.to_string(),
            cpu_pct,
            mem_bytes,
            disk_read_bps: disk / 2.0,
            disk_write_bps: disk / 2.0,
            user: user.to_string(),
            status: status.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn build_groups_aggregates_processes_with_same_name() {
        let procs = vec![
            make_proc(1, "chrome", 1.0, 100, 10.0, "alice", "Running"),
            make_proc(2, "chrome", 2.0, 200, 20.0, "alice", "Running"),
            make_proc(3, "firefox", 5.0, 500, 30.0, "alice", "Running"),
        ];
        let groups = build_groups(&procs, &|_| true);
        assert_eq!(groups.len(), 2);

        let chrome = groups.iter().find(|g| g.name == "chrome").unwrap();
        assert_eq!(chrome.procs.len(), 2);
        assert!((chrome.cpu_pct - 3.0).abs() < 1e-6);
        assert_eq!(chrome.mem_bytes, 300);
        assert!((chrome.disk_bps - 30.0).abs() < 1e-6);

        let firefox = groups.iter().find(|g| g.name == "firefox").unwrap();
        assert_eq!(firefox.procs.len(), 1);
    }

    #[test]
    fn build_groups_respects_filter_predicate() {
        let procs = vec![
            make_proc(1, "chrome", 1.0, 100, 0.0, "alice", "Running"),
            make_proc(2, "firefox", 2.0, 200, 0.0, "alice", "Running"),
            make_proc(3, "vim", 3.0, 50, 0.0, "alice", "Running"),
        ];
        let groups = build_groups(&procs, &|p| p.name.starts_with('f'));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "firefox");
    }

    #[test]
    fn build_groups_returns_groups_alphabetically() {
        // BTreeMap-backed: alphabetical by name is the expected pre-sort
        // order before `sort_groups` reorders by the user's chosen column.
        let procs = vec![
            make_proc(1, "zsh", 1.0, 0, 0.0, "u", "Running"),
            make_proc(2, "alpha", 1.0, 0, 0.0, "u", "Running"),
            make_proc(3, "mid", 1.0, 0, 0.0, "u", "Running"),
        ];
        let groups = build_groups(&procs, &|_| true);
        let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
        assert_eq!(names, vec!["alpha", "mid", "zsh"]);
    }

    #[test]
    fn build_groups_empty_input_yields_empty() {
        let procs: Vec<monitor::processes::ProcInfo> = vec![];
        let groups = build_groups(&procs, &|_| true);
        assert!(groups.is_empty());
    }

    #[test]
    fn sort_groups_cpu_descending_largest_first() {
        let procs = vec![
            make_proc(1, "low", 1.0, 0, 0.0, "u", "Running"),
            make_proc(2, "high", 50.0, 0, 0.0, "u", "Running"),
            make_proc(3, "mid", 10.0, 0, 0.0, "u", "Running"),
        ];
        let mut groups = build_groups(&procs, &|_| true);
        sort_groups(&mut groups, SortKey::Cpu, true);
        let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
        assert_eq!(names, vec!["high", "mid", "low"]);
    }

    #[test]
    fn sort_groups_mem_ascending_smallest_first() {
        let procs = vec![
            make_proc(1, "big", 0.0, 1000, 0.0, "u", "Running"),
            make_proc(2, "small", 0.0, 10, 0.0, "u", "Running"),
            make_proc(3, "mid", 0.0, 100, 0.0, "u", "Running"),
        ];
        let mut groups = build_groups(&procs, &|_| true);
        sort_groups(&mut groups, SortKey::Mem, false);
        let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
        assert_eq!(names, vec!["small", "mid", "big"]);
    }

    #[test]
    fn sort_groups_pid_uses_minimum_pid() {
        // For grouped processes the sort key on PID is the *lowest* PID in
        // the group — that's the closest thing to a stable "parent" rank.
        let procs = vec![
            make_proc(50, "a", 0.0, 0, 0.0, "u", "Running"),
            make_proc(10, "a", 0.0, 0, 0.0, "u", "Running"),
            make_proc(30, "b", 0.0, 0, 0.0, "u", "Running"),
        ];
        let mut groups = build_groups(&procs, &|_| true);
        sort_groups(&mut groups, SortKey::Pid, false);
        let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
        // group "a" has min PID 10, group "b" has min PID 30.
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn sort_children_pid_ascending() {
        let procs = [
            make_proc(3, "x", 0.0, 0, 0.0, "u", "Running"),
            make_proc(1, "x", 0.0, 0, 0.0, "u", "Running"),
            make_proc(2, "x", 0.0, 0, 0.0, "u", "Running"),
        ];
        let mut refs: Vec<&monitor::processes::ProcInfo> = procs.iter().collect();
        sort_children(&mut refs, SortKey::Pid, false);
        let pids: Vec<u32> = refs.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![1, 2, 3]);
    }
}
