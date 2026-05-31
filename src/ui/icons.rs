use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

/// Resolves a process to an icon URI from freedesktop `.desktop` files and the
/// system icon theme.
///
/// The expensive work — building the `.desktop` index and the per-icon theme
/// `stat()` scans — runs on a worker thread so it never blocks the UI frame. A
/// cache miss enqueues a request and renders a placeholder; the icon pops in a
/// frame or two later once the worker answers and wakes the UI via
/// [`egui::Context::request_repaint`]. Results persist to `~/.cache/rproc/icons.tsv`,
/// so a warm cache serves most rows from memory without touching the worker.
pub struct Resolver {
    /// Spawned lazily on the first [`Resolver::pump`], once an `egui::Context`
    /// exists to wake the UI from.
    worker: Option<Worker>,
    /// `.desktop` index keys, reported by the worker once built. `None` until
    /// then — see [`Resolver::has_desktop_entry`].
    index_keys: Option<HashSet<String>>,
    /// `(proc_name, exe_path)` → resolved URI. A hit is a single map lookup with
    /// no worker round-trip. Seeded from disk, filled by worker responses.
    uri_cache: HashMap<String, Option<String>>,
    /// Keys handed to the worker and awaiting a response, so each is enqueued
    /// only once however many frames render before the answer lands.
    pending: HashSet<String>,
    /// Set once `uri_cache` diverges from disk. Gates the write in [`save_persistent`].
    dirty: bool,
    last_flush: Instant,
}

/// Dropping this closes `tx`, ending the worker's `recv()` loop.
struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Response>,
}

enum Request {
    Resolve {
        key: String,
        proc_name: String,
        exe_path: String,
    },
}

enum Response {
    IndexReady(HashSet<String>),
    Resolved { key: String, uri: Option<String> },
}

/// How long a persisted cache stays trusted before a full rescan. Icons move
/// rarely, so a weekly rebuild keeps newly installed apps picked up without
/// re-scanning every launch.
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Minimum gap between throttled disk flushes while the app runs. Bounds the
/// work an abrupt kill can lose to a few seconds of newly resolved icons.
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

impl Resolver {
    pub fn new() -> Self {
        let mut r = Self {
            worker: None,
            index_keys: None,
            uri_cache: HashMap::new(),
            pending: HashSet::new(),
            dirty: false,
            last_flush: Instant::now(),
        };
        r.load_persistent();
        r
    }

    /// Drains the worker's results and spawns it on first call. Must run once
    /// per frame before any `icon_uri` / `has_desktop_entry` query.
    pub fn pump(&mut self, ctx: &egui::Context) {
        if self.worker.is_none() {
            self.worker = Some(spawn_worker(ctx.clone()));
        }
        while let Ok(resp) = self.worker.as_ref().unwrap().rx.try_recv() {
            match resp {
                Response::IndexReady(keys) => self.index_keys = Some(keys),
                Response::Resolved { key, uri } => {
                    self.pending.remove(&key);
                    self.uri_cache.insert(key, uri);
                    self.dirty = true;
                }
            }
        }
    }

    pub fn icon_uri(&mut self, proc_name: &str, exe_path: &str) -> Option<String> {
        let key = make_key(proc_name, exe_path);
        if let Some(cached) = self.uri_cache.get(&key) {
            return cached.clone();
        }
        // Miss: hand off to the worker and render a placeholder until it answers.
        // `pending` keeps a key in flight from being re-enqueued every frame.
        if let Some(worker) = self.worker.as_ref()
            && self.pending.insert(key.clone())
        {
            let _ = worker.tx.send(Request::Resolve {
                key,
                proc_name: proc_name.to_string(),
                exe_path: exe_path.to_string(),
            });
        }
        None
    }

    /// Seeds `uri_cache` from `~/.cache/rproc/icons.tsv` when the file is
    /// younger than [`CACHE_TTL`]. Positive entries are validated against the
    /// filesystem once here — an app may have been updated or removed between
    /// runs — and dead ones are dropped so they get recomputed this session.
    /// A stale (or unreadable) file is simply ignored, which forces a fresh
    /// scan that rewrites the file: that is the weekly refresh.
    fn load_persistent(&mut self) {
        let Some(path) = cache_path() else { return };
        if let Ok(meta) = fs::metadata(&path)
            && let Ok(modified) = meta.modified()
            && let Ok(age) = SystemTime::now().duration_since(modified)
            && age > CACHE_TTL
        {
            return;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            return;
        };
        for line in content.lines() {
            let Some((key, uri)) = line.split_once('\t') else {
                continue;
            };
            let value = if uri.is_empty() {
                None
            } else if uri
                .strip_prefix("file://")
                .is_some_and(|p| Path::new(p).exists())
            {
                Some(uri.to_string())
            } else {
                // Icon file vanished since it was cached → drop it and let this
                // session recompute. Mark dirty so the cleaned set is written back.
                self.dirty = true;
                continue;
            };
            self.uri_cache.insert(key.to_string(), value);
        }
    }

    /// Writes `uri_cache` back to disk so the next launch skips the theme scan.
    /// Cheap (a few KB) and a no-op when nothing changed since the last write.
    /// Clears `dirty` so repeated calls don't rewrite unchanged data.
    pub fn save_persistent(&mut self) {
        if !self.dirty {
            return;
        }
        if let Some(path) = cache_path() {
            let _ = fs::write(&path, serialize_cache(&self.uri_cache));
        }
        self.dirty = false;
        self.last_flush = Instant::now();
    }

    /// Throttled flush meant to be called every frame. `on_exit` is the clean
    /// path, but it only fires on an orderly window close — a SIGTERM, session
    /// logout, or compositor teardown skips it and would otherwise discard every
    /// icon resolved this session. Writing at most once per [`FLUSH_INTERVAL`]
    /// caps that loss at a few seconds while staying off the hot path.
    pub fn flush_if_due(&mut self) {
        if self.dirty && self.last_flush.elapsed() >= FLUSH_INTERVAL {
            self.save_persistent();
        }
    }

    pub fn has_desktop_entry(&self, proc_name: &str, exe_path: &str) -> bool {
        // Before the index arrives everything reports "no entry" (background
        // section), then reshuffles into Apps once the keys land — a frame or
        // two on a cold start.
        self.index_keys
            .as_ref()
            .is_some_and(|keys| has_entry(keys, proc_name, exe_path))
    }
}

fn spawn_worker(ctx: egui::Context) -> Worker {
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let (res_tx, res_rx) = mpsc::channel::<Response>();
    std::thread::Builder::new()
        .name("icon-resolver".to_string())
        .spawn(move || {
            let index = build_index();
            if res_tx
                .send(Response::IndexReady(index.keys().cloned().collect()))
                .is_err()
            {
                return;
            }
            ctx.request_repaint();
            // Memoizes `icon_name` → URI so processes sharing an icon pay the
            // theme stat() scan once.
            let mut icon_cache: HashMap<String, Option<String>> = HashMap::new();
            while let Ok(Request::Resolve {
                key,
                proc_name,
                exe_path,
            }) = req_rx.recv()
            {
                let uri = compute_icon_uri(&index, &mut icon_cache, &proc_name, &exe_path);
                if res_tx.send(Response::Resolved { key, uri }).is_err() {
                    break;
                }
                // Paint the icon now instead of waiting for the next scheduled
                // repaint; egui coalesces a cold-load burst into one extra frame.
                ctx.request_repaint();
            }
        })
        .expect("failed to spawn icon-resolver thread");
    Worker {
        tx: req_tx,
        rx: res_rx,
    }
}

/// `proc_name` and `exe_path` joined by a NUL (which occurs in neither), so
/// distinct pairs never collide on a shared prefix.
fn make_key(proc_name: &str, exe_path: &str) -> String {
    let mut key = String::with_capacity(proc_name.len() + exe_path.len() + 1);
    key.push_str(proc_name);
    key.push('\0');
    key.push_str(exe_path);
    key
}

fn build_index() -> HashMap<String, String> {
    let mut map = HashMap::new();
    for dir in desktop_dirs() {
        scan_desktop_dir(&dir, &mut map);
    }
    map
}

/// Whether a `.desktop` index `key` and a lowercased process name share a prefix
/// long enough to be a confident match. The guard is on the *shared prefix*
/// (whichever string is the prefix of the other), not on `key.len()` alone, so a
/// short command name like "git" can't claim the icon of any longer "git*" app.
fn prefix_matches(key: &str, proc_lower: &str) -> bool {
    const MIN_PREFIX_LEN: usize = 5;
    (key.starts_with(proc_lower) && proc_lower.len() >= MIN_PREFIX_LEN)
        || (proc_lower.starts_with(key) && key.len() >= MIN_PREFIX_LEN)
}

fn has_entry(index_keys: &HashSet<String>, proc_name: &str, exe_path: &str) -> bool {
    desktop_candidates(proc_name, exe_path)
        .iter()
        .any(|c| !c.is_empty() && index_keys.contains(c))
}

fn compute_icon_uri(
    index: &HashMap<String, String>,
    icon_cache: &mut HashMap<String, Option<String>>,
    proc_name: &str,
    exe_path: &str,
) -> Option<String> {
    // Standard candidates
    for cand in desktop_candidates(proc_name, exe_path) {
        if cand.is_empty() {
            continue;
        }
        if let Some(icon) = index.get(&cand).cloned()
            && let Some(uri) = resolve_icon(icon_cache, &icon)
        {
            return Some(uri);
        }
    }
    // Prefix match, in two directions:
    //   - key.starts_with(proc): recovers a truncated process name —
    //     Linux caps `comm` at 15 chars, so "brave-browser-stable" arrives as
    //     "brave-browser-s" and must still match its key.
    //   - proc.starts_with(key): a longer process name carrying a suffix still
    //     matches the shorter app key (e.g. "signal-desktop" → "signal").
    // The guard must apply to the *shared prefix* length, which is whichever of
    // the two strings is the prefix — gating on `key.len()` alone let a 3-char
    // command like "git" match the icon of any longer "git*" app. Requiring the
    // shared prefix to be reasonably long rejects "git"/"ssh"/"vim"/"code"/"node"
    // while keeping "brave"/"signal"/"telegram". HashMap iteration order is
    // arbitrary, so when several keys match we pick the longest key (most
    // specific) and break ties lexicographically to stay deterministic.
    let proc_lower = proc_name.to_lowercase();
    let matching_icon: Option<String> = index
        .iter()
        .filter(|(key, _)| prefix_matches(key, &proc_lower))
        .max_by(|(a, _), (b, _)| a.len().cmp(&b.len()).then_with(|| b.cmp(a)))
        .map(|(_, icon)| icon.clone());

    if let Some(icon) = matching_icon
        && let Some(uri) = resolve_icon(icon_cache, &icon)
    {
        return Some(uri);
    }
    // Direct icon name lookup: an icon theme often ships an icon whose name
    // matches the binary/stem even when no .desktop entry declares it (e.g.
    // "blueman-applet" has no desktop file, but the theme has a "blueman"
    // icon via the stem). Try each candidate as a themed icon name.
    for cand in desktop_candidates(proc_name, exe_path) {
        if cand.is_empty() {
            continue;
        }
        if let Some(uri) = resolve_icon(icon_cache, &cand) {
            return Some(uri);
        }
    }
    // Generic fallback
    if let Some(uri) = resolve_icon(icon_cache, "application-x-executable") {
        return Some(uri);
    }
    None
}

fn resolve_icon(
    icon_cache: &mut HashMap<String, Option<String>>,
    icon_name: &str,
) -> Option<String> {
    if let Some(cached) = icon_cache.get(icon_name) {
        return cached.clone();
    }
    let result = if icon_name.starts_with('/') {
        // Absolute path straight from a desktop `Icon=` line (e.g.
        // python3.12.desktop points at an .xpm). Only accept formats the
        // egui image loaders can actually decode — otherwise the row shows
        // a broken-image glyph instead of falling back to a usable icon.
        let p = PathBuf::from(icon_name);
        if p.exists() && is_loadable_icon(&p) {
            Some(format!("file://{}", p.display()))
        } else {
            None
        }
    } else {
        lookup_icon(icon_name).map(|p| format!("file://{}", p.display()))
    };
    icon_cache.insert(icon_name.to_string(), result.clone());
    result
}

/// Image formats the egui loaders can decode (egui_extras `image` with the
/// `image` crate's png+jpeg features, plus `svg` via resvg). XPM, GIF, BMP, …
/// are NOT supported and must never be handed to the UI.
const LOADABLE_ICON_EXTS: [&str; 4] = ["svg", "png", "jpg", "jpeg"];

fn is_loadable_icon(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| LOADABLE_ICON_EXTS.contains(&e.as_str()))
}

fn lookup_icon(icon_name: &str) -> Option<PathBuf> {
    use std::sync::OnceLock;
    static THEME_CHAIN: OnceLock<Vec<PathBuf>> = OnceLock::new();

    let theme_dirs = THEME_CHAIN.get_or_init(|| {
        let mut dirs = Vec::new();

        let active_theme = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .trim_matches('\'')
                    .to_string();
                if s.is_empty() { None } else { Some(s) }
            })
            .unwrap_or_else(|| "hicolor".to_string());

        let icon_bases = {
            let mut bases = vec![
                PathBuf::from("/usr/share/icons"),
                PathBuf::from("/usr/local/share/icons"),
            ];
            if let Some(home) = std::env::var_os("HOME") {
                let home = PathBuf::from(home);
                bases.push(home.join(".local/share/icons"));
                bases.push(home.join(".icons"));
            }
            bases
        };

        // Collect themes following inheritance chain
        let mut themes = vec![active_theme];
        let mut seen = std::collections::HashSet::new();
        seen.insert(themes[0].clone());

        let mut i = 0;
        while i < themes.len() {
            let current = themes[i].clone(); // Clone to release borrow
            for base in &icon_bases {
                let index_path = base.join(&current).join("index.theme");
                if let Ok(content) = std::fs::read_to_string(&index_path) {
                    for line in content.lines() {
                        if let Some(parents) = line.strip_prefix("Inherits=") {
                            for parent in parents.split(',') {
                                let parent = parent.trim().to_string();
                                if !parent.is_empty() && seen.insert(parent.clone()) {
                                    themes.push(parent);
                                }
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        if seen.insert("hicolor".to_string()) {
            themes.push("hicolor".to_string());
        }
        // Adwaita is GNOME's default and has many generic icons
        if seen.insert("Adwaita".to_string()) {
            themes.push("Adwaita".to_string());
        }
        // Breeze / breeze-dark — KDE defaults (use whichever exists)
        for breeze in ["breeze", "breeze-dark"] {
            if seen.insert(breeze.to_string()) {
                themes.push(breeze.to_string());
            }
        }
        // Build search directories
        for theme in &themes {
            for base in &icon_bases {
                dirs.push(base.join(theme));
            }
        }

        // Most theme×base combinations don't exist (e.g. Adwaita under
        // ~/.icons). Drop them once here so a missed icon doesn't `stat()` its
        // way through dozens of phantom directories — the dominant cost on the
        // first scan of a fresh cache.
        dirs.retain(|d| d.exists());
        dirs
    });

    let names_to_try: [&str; 2] = [
        icon_name,
        icon_name.strip_suffix("-symbolic").unwrap_or(icon_name),
    ];

    let exts = LOADABLE_ICON_EXTS;
    let sizes = [
        "48x48", "64x64", "128x128", "32x32", "24x24", "22x22", "16x16", "scalable",
    ];
    let cats = [
        "apps",
        "devices",
        "places",
        "categories",
        "status",
        "emblems",
        "mimetypes",
    ];

    for name in &names_to_try {
        for dir in theme_dirs {
            for size in &sizes {
                for cat in &cats {
                    for ext in &exts {
                        let path = dir.join(size).join(cat).join(format!("{name}.{ext}"));
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    // Flat pixmap fallback: many apps (e.g. VS Code's `vscode.png`) ship only a
    // single icon under /usr/share/pixmaps and nothing in a themed layout. The
    // themed scan above uses the `<size>/<category>/` directory shape, which
    // misses both pixmaps and themes that nest as `<category>/<size>/` (Mint-Y).
    // Checking pixmaps last keeps a proper themed icon preferred when one exists.
    for name in &names_to_try {
        for base in pixmap_bases() {
            for ext in &exts {
                let path = base.join(format!("{name}.{ext}"));
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn pixmap_bases() -> Vec<PathBuf> {
    let mut bases = vec![PathBuf::from("/usr/share/pixmaps")];
    if let Some(home) = std::env::var_os("HOME") {
        bases.push(PathBuf::from(home).join(".local/share/pixmaps"));
    }
    bases
}

/// Path to the persisted icon cache under the shared `~/.cache/rproc` dir.
fn cache_path() -> Option<PathBuf> {
    crate::daemon::storage::cache_dir()
        .ok()
        .map(|d| d.join("icons.tsv"))
}

/// Renders `uri_cache` as the on-disk TSV: one `key\turi` line per entry, empty
/// `uri` for a negative result. Entries whose key or uri contain a tab/newline
/// are skipped — they'd corrupt the line format and never occur for real
/// process names or icon paths.
fn serialize_cache(uri_cache: &HashMap<String, Option<String>>) -> String {
    let mut buf = String::new();
    for (key, value) in uri_cache {
        let uri = value.as_deref().unwrap_or("");
        if key.contains(['\t', '\n']) || uri.contains(['\t', '\n']) {
            continue;
        }
        buf.push_str(key);
        buf.push('\t');
        buf.push_str(uri);
        buf.push('\n');
    }
    buf
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

fn desktop_candidates(proc_name: &str, exe_path: &str) -> Vec<String> {
    let exe_base = Path::new(exe_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let proc_lower = proc_name.to_lowercase();
    let stem = proc_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("")
        .to_string();
    let prefix: String = proc_lower.chars().take(10).collect();
    let mut cands = vec![exe_base, proc_lower.clone(), stem.clone()];
    if prefix != stem && prefix != proc_lower {
        cands.push(prefix);
    }
    cands
}

fn desktop_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        PathBuf::from("/var/lib/snapd/desktop/applications"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }
    dirs
}

fn scan_desktop_dir(dir: &Path, out: &mut HashMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        parse_desktop(&content, stem, out);
    }
}

fn parse_desktop(content: &str, file_stem: &str, out: &mut HashMap<String, String>) {
    let mut icon: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut try_exec: Option<String> = None;
    let mut wm_class: Option<String> = None;
    let mut in_main = false;
    let mut hidden = false;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Icon" => icon = Some(v.trim().to_string()),
            "Exec" => exec = Some(v.trim().to_string()),
            "TryExec" => try_exec = Some(v.trim().to_string()),
            "StartupWMClass" => wm_class = Some(v.trim().to_string()),
            "Hidden" => hidden = v.trim().eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    if hidden {
        return;
    }
    let Some(icon) = icon else {
        return;
    };

    let mut add = |key: &str| {
        let key = key.to_lowercase();
        if key.is_empty() {
            return;
        }
        out.entry(key).or_insert_with(|| icon.clone());
    };

    if let Some(e) = exec.as_deref()
        && let Some(bin) = exec_binary(e)
        && let Some(base) = Path::new(&bin).file_name().and_then(|s| s.to_str())
    {
        add(base);
    }
    if let Some(t) = try_exec.as_deref()
        && let Some(base) = Path::new(t).file_name().and_then(|s| s.to_str())
    {
        add(base);
    }
    if let Some(w) = wm_class.as_deref() {
        add(w);
    }
    add(file_stem);
}

fn exec_binary(exec: &str) -> Option<String> {
    let mut tokens = shell_split(exec);
    while let Some(tok) = tokens.first().cloned() {
        if tok.contains('=') && !tok.starts_with('/') {
            tokens.remove(0);
            continue;
        }
        let base = Path::new(&tok)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&tok);
        if matches!(base, "env" | "sh" | "bash" | "dbus-run-session") {
            tokens.remove(0);
            continue;
        }
        return Some(tok);
    }
    None
}

fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            }
            c => buf.push(c),
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
#[path = "icons_tests.rs"]
mod tests;
