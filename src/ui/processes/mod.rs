use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use slint::{Image, ModelRc, SharedString, VecModel};

use crate::monitor::{self, Snapshot};
use crate::theme;
use crate::ui::icons;
use crate::ui::widgets;
use crate::{MainWindow, ProcRow};

mod actions;
mod grouping;
pub mod properties;
mod sort;

pub(crate) use actions::{copy_to_clipboard, open_in_file_manager, open_search};
use grouping::{Group, Row, append_section, build_groups, sort_children, sort_groups};
use sort::{SortKey, load_sort_prefs, save_sort_prefs};

/// Pixel size icons are rasterized to. Rows render them at ~16 px; a touch more
/// keeps them crisp without bloating the cache.
const ICON_PX: u32 = 20;

/// What a visible row points at, so a click by model index can resolve to a
/// selection without re-deriving the table.
#[derive(Clone)]
pub enum RowRef {
    Section,
    Group(String),
    Proc(u32),
}

pub struct State {
    sort: SortKey,
    descending: bool,
    pub selected_pid: Option<u32>,
    pub selected_group: Option<String>,
    expanded: HashSet<String>,
    pub filter: String,
    icons: icons::Resolver,
    images: HashMap<String, Image>,
    row_refs: Vec<RowRef>,
    /// Persistent row model — updated in place so clicks aren't dropped when a
    /// refresh tick lands between a row's press and release.
    rows_model: Rc<VecModel<ProcRow>>,
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
            images: HashMap::new(),
            row_refs: Vec::new(),
            rows_model: Rc::new(VecModel::default()),
        }
    }

    pub fn save_icon_cache(&mut self) {
        self.icons.save_persistent();
    }

    pub fn flush_icon_cache_if_due(&mut self) {
        self.icons.flush_if_due();
    }

    pub fn toggle_sort(&mut self, key: i32) {
        let key = key_from_int(key);
        if self.sort == key {
            self.descending = !self.descending;
        } else {
            self.sort = key;
            self.descending = matches!(key, SortKey::Cpu | SortKey::Mem | SortKey::Disk);
        }
        save_sort_prefs(self.sort, self.descending);
    }

    pub fn toggle_group(&mut self, name: &str) {
        if !self.expanded.remove(name) {
            self.expanded.insert(name.to_string());
        }
    }

    pub fn row_ref(&self, index: usize) -> Option<RowRef> {
        self.row_refs.get(index).cloned()
    }

    pub fn row_clicked(&mut self, index: usize) {
        match self.row_refs.get(index).cloned() {
            Some(RowRef::Proc(pid)) => {
                self.selected_pid = Some(pid);
                self.selected_group = None;
            }
            Some(RowRef::Group(name)) => {
                self.selected_group = Some(name);
                self.selected_pid = None;
            }
            _ => {}
        }
    }

    fn image_for(&mut self, name: &str, exe: &str) -> Image {
        match self.icons.icon_uri(name, exe) {
            Some(uri) => {
                if let Some(img) = self.images.get(&uri) {
                    return img.clone();
                }
                // Rasterize to display size (not native) so the cache stays tiny.
                let path = uri.strip_prefix("file://").unwrap_or(&uri);
                let img =
                    icons::decode_scaled(std::path::Path::new(path), ICON_PX).unwrap_or_default();
                self.images.insert(uri, img.clone());
                img
            }
            None => Image::default(),
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

fn ss(s: &str) -> SharedString {
    s.into()
}

pub fn apply(window: &MainWindow, state: &mut State, snap: &Snapshot) {
    state.icons.pump();

    let filter = state.filter.trim().to_lowercase();
    let filter_active = !filter.is_empty();
    let matches = |p: &monitor::processes::ProcInfo| -> bool {
        if !filter_active {
            return true;
        }
        p.name.to_lowercase().contains(&filter) || p.pid.to_string().contains(&filter)
    };

    let groups: Vec<Group> = build_groups(&snap.processes, &matches);
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

    let selected_pid = state.selected_pid;
    let selected_group = state.selected_group.clone();

    let mut rows: Vec<ProcRow> = Vec::with_capacity(visible.len());
    let mut refs: Vec<RowRef> = Vec::with_capacity(visible.len());

    for row in &visible {
        match row {
            Row::SectionHeader(title) => {
                rows.push(section_row(&title.to_uppercase()));
                refs.push(RowRef::Section);
            }
            Row::GroupHeader { g, expanded } => {
                let (name, exe) = g
                    .procs
                    .first()
                    .map(|p| (p.name.clone(), p.exe.clone()))
                    .unwrap_or_default();
                let icon = state.image_for(&name, &exe);
                rows.push(ProcRow {
                    kind: 1,
                    label: ss(g.name),
                    pid: 0,
                    pid_text: ss(""),
                    user: ss(""),
                    cpu: ss(&format_pct(g.cpu_pct)),
                    mem: ss(&widgets::format_bytes(g.mem_bytes)),
                    disk: ss(&widgets::format_bps(g.disk_bps)),
                    disk_tooltip: ss(""),
                    status: ss(""),
                    status_color: theme::text(),
                    icon,
                    expanded: *expanded,
                    selected: selected_group.as_deref() == Some(g.name),
                    group_key: ss(g.name),
                    count: g.procs.len() as i32,
                    indent: false,
                });
                refs.push(RowRef::Group(g.name.to_string()));
            }
            Row::Single(p) => {
                rows.push(proc_row(state, p, false, selected_pid));
                refs.push(RowRef::Proc(p.pid));
            }
            Row::Child(p) => {
                rows.push(proc_row(state, p, true, selected_pid));
                refs.push(RowRef::Proc(p.pid));
            }
        }
    }

    state.row_refs = refs;
    crate::ui::model::sync(&state.rows_model, rows);
    window.set_proc_rows(ModelRc::from(state.rows_model.clone()));
    window.set_proc_count_label(ss(&format!("{} processes", snap.processes.len())));
    window.set_proc_sampling(snap.ready);
    window.set_proc_sort_key(int_from_key(state.sort));
    window.set_proc_sort_desc(state.descending);
    window.set_proc_cpu_header(ss(&format!("CPU  {:.0}%", snap.system.cpu_total)));
    window.set_proc_mem_header(ss(&format!("Memory  {:.0}%", snap.system.ram_used_pct)));

    // Drop a stale selection when its process / group is gone.
    if let Some(pid) = state.selected_pid
        && !snap.processes.iter().any(|p| p.pid == pid)
    {
        state.selected_pid = None;
    }
    if let Some(name) = state.selected_group.clone()
        && !snap.processes.iter().any(|p| p.name == name)
    {
        state.selected_group = None;
    }

    window.set_proc_selected_pid(state.selected_pid.map(|p| p as i32).unwrap_or(-1));
    window.set_proc_selected_group(ss(state.selected_group.as_deref().unwrap_or("")));
    let count = state
        .selected_group
        .as_deref()
        .map(|name| snap.processes.iter().filter(|p| p.name == name).count())
        .unwrap_or(0);
    window.set_proc_selected_count(count as i32);
    let suspend_label = state
        .selected_pid
        .and_then(|pid| snap.processes.iter().find(|p| p.pid == pid))
        .map(|p| {
            if matches!(p.status.as_str(), "Stop" | "Stopped") {
                "Resume"
            } else {
                "Suspend"
            }
        })
        .unwrap_or("Suspend");
    window.set_proc_suspend_label(ss(suspend_label));
}

fn section_row(title: &str) -> ProcRow {
    ProcRow {
        kind: 0,
        label: ss(title),
        pid: 0,
        pid_text: ss(""),
        user: ss(""),
        cpu: ss(""),
        mem: ss(""),
        disk: ss(""),
        disk_tooltip: ss(""),
        status: ss(""),
        status_color: theme::text(),
        icon: Image::default(),
        expanded: false,
        selected: false,
        group_key: ss(""),
        count: 0,
        indent: false,
    }
}

fn proc_row(
    state: &mut State,
    p: &monitor::processes::ProcInfo,
    indent: bool,
    selected_pid: Option<u32>,
) -> ProcRow {
    let icon = state.image_for(&p.name, &p.exe);
    let combined = p.disk_read_bps + p.disk_write_bps;
    ProcRow {
        kind: if indent { 3 } else { 2 },
        label: ss(&p.name),
        pid: p.pid as i32,
        pid_text: ss(&p.pid.to_string()),
        user: ss(&p.user),
        cpu: ss(&format_pct(p.cpu_pct)),
        mem: ss(&widgets::format_bytes(p.mem_bytes)),
        disk: ss(&widgets::format_bps(combined)),
        disk_tooltip: ss(&format!(
            "Read: {}\nWrite: {}",
            widgets::format_bps(p.disk_read_bps),
            widgets::format_bps(p.disk_write_bps)
        )),
        status: ss(&p.status),
        status_color: status_color(&p.status),
        icon,
        expanded: false,
        selected: selected_pid == Some(p.pid),
        group_key: ss(""),
        count: 0,
        indent,
    }
}

fn key_from_int(k: i32) -> SortKey {
    match k {
        0 => SortKey::Name,
        1 => SortKey::Pid,
        2 => SortKey::User,
        3 => SortKey::Cpu,
        4 => SortKey::Mem,
        5 => SortKey::Disk,
        6 => SortKey::Status,
        _ => SortKey::Cpu,
    }
}

fn int_from_key(k: SortKey) -> i32 {
    match k {
        SortKey::Name => 0,
        SortKey::Pid => 1,
        SortKey::User => 2,
        SortKey::Cpu => 3,
        SortKey::Mem => 4,
        SortKey::Disk => 5,
        SortKey::Status => 6,
    }
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

fn status_color(s: &str) -> slint::Color {
    match s {
        "Run" | "Running" => theme::ok(),
        "Sleep" | "Sleeping" | "Idle" => theme::text_dim(),
        "Waiting" | "Stop" | "Stopped" => theme::warn(),
        "Zombie" | "Dead" => theme::err(),
        _ => theme::text(),
    }
}

#[cfg(test)]
#[path = "processes_tests.rs"]
mod tests;
