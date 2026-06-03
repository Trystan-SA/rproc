use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn build_index() -> HashMap<String, String> {
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

pub(super) fn has_entry(index_keys: &HashSet<String>, proc_name: &str, exe_path: &str) -> bool {
    desktop_candidates(proc_name, exe_path)
        .iter()
        .any(|c| !c.is_empty() && index_keys.contains(c))
}

pub(super) fn compute_icon_uri(
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
#[path = "freedesktop_tests.rs"]
mod tests;
