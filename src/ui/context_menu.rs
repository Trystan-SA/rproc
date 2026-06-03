use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::MenuEntry;

/// What the open menu acts on. Cloned out at activation so dispatch doesn't
/// depend on the live selection.
#[derive(Clone)]
pub enum Target {
    Process(u32),
    Group(String),
    Service(usize),
    Startup(usize),
}

/// A single menu action, dispatched in `app.rs` against the stored `Target`.
#[derive(Clone, Copy, PartialEq)]
pub enum Act {
    EndTask,
    ForceKill,
    SuspendResume,
    EndAll,
    ForceKillAll,
    SuspendResumeAll,
    OpenLocation,
    SearchOnline,
    CopyPid,
    CopyName,
    CopyCmd,
    CopyAllPids,
    CopyUnit,
    SvcStart,
    SvcStop,
    SvcRestart,
    OpenDesktop,
    Properties,
}

#[derive(Default)]
pub struct ContextMenu {
    pub open: bool,
    target: Option<Target>,
    acts: Vec<Act>,
}

impl ContextMenu {
    /// Store the target and the clickable actions for a freshly built menu.
    pub fn arm(&mut self, target: Target, acts: Vec<Act>) {
        self.target = Some(target);
        self.acts = acts;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.target = None;
        self.acts.clear();
    }

    /// Resolve an activated entry's `action` index to its target + action.
    pub fn resolve(&self, action: usize) -> Option<(Target, Act)> {
        match (&self.target, self.acts.get(action)) {
            (Some(t), Some(a)) => Some((t.clone(), *a)),
            _ => None,
        }
    }
}

/// Builds the parallel `MenuEntry` model (for Slint) and `Act` list (for
/// dispatch). Separators don't consume an action slot.
pub struct Builder {
    entries: Vec<MenuEntry>,
    acts: Vec<Act>,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            acts: Vec::new(),
        }
    }

    pub fn item(&mut self, label: impl Into<SharedString>, act: Act, enabled: bool) -> &mut Self {
        self.entries.push(MenuEntry {
            label: label.into(),
            action: self.acts.len() as i32,
            enabled,
            separator: false,
        });
        self.acts.push(act);
        self
    }

    pub fn sep(&mut self) -> &mut Self {
        self.entries.push(MenuEntry {
            label: SharedString::default(),
            action: -1,
            enabled: false,
            separator: true,
        });
        self
    }

    /// Consume the builder into (Slint model, action list).
    pub fn finish(self) -> (ModelRc<MenuEntry>, Vec<Act>) {
        (
            ModelRc::from(Rc::new(VecModel::from(self.entries))),
            self.acts,
        )
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}
