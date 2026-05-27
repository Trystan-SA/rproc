use std::cell::OnceCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// Resolves a process to an icon URI by indexing the freedesktop `.desktop`
// files on disk and then locating the named icon under the standard
// `hicolor` theme + `pixmaps` search paths. The index (~150 files parsed) is
// built lazily on the first lookup rather than at construction, so it stays
// off the startup path — it's only needed once the Processes tab is opened.
// After that, per-row lookups are pure hashmap reads.
pub struct Resolver {
    name_to_icon: OnceCell<HashMap<String, String>>,
    cache: HashMap<String, Option<String>>,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            name_to_icon: OnceCell::new(),
            cache: HashMap::new(),
        }
    }

    /// The `.desktop` name→icon index, parsed on first access and cached for
    /// the process lifetime. Takes `&self` (via `OnceCell`) so the cheap
    /// `has_desktop_entry` check can trigger the build too.
    fn index(&self) -> &HashMap<String, String> {
        self.name_to_icon.get_or_init(|| {
            let mut map = HashMap::new();
            for dir in desktop_dirs() {
                scan_desktop_dir(&dir, &mut map);
            }
            map
        })
    }

    pub fn icon_uri(&mut self, proc_name: &str, exe_path: &str) -> Option<String> {
        for cand in desktop_candidates(proc_name, exe_path) {
            if cand.is_empty() {
                continue;
            }
            // Resolve the desktop→icon name first so the `index()` borrow ends
            // before `resolve_icon` needs `&mut self` for its cache.
            if let Some(icon) = self.index().get(&cand).cloned()
                && let Some(uri) = self.resolve_icon(&icon)
            {
                return Some(uri);
            }
        }
        None
    }

    /// Whether the process maps to a freedesktop `.desktop` entry — i.e. it's
    /// a launchable application rather than a background daemon. The Processes
    /// panel uses this to split "Apps" from background processes. Unlike
    /// `icon_uri` it doesn't require the named icon file to resolve on disk: a
    /// stale or missing icon shouldn't demote an app to the background section.
    pub fn has_desktop_entry(&self, proc_name: &str, exe_path: &str) -> bool {
        desktop_candidates(proc_name, exe_path)
            .iter()
            .any(|c| !c.is_empty() && self.index().contains_key(c))
    }

    fn resolve_icon(&mut self, icon: &str) -> Option<String> {
        if let Some(cached) = self.cache.get(icon) {
            return cached.clone();
        }
        let path = if icon.starts_with('/') {
            let p = PathBuf::from(icon);
            p.exists().then_some(p)
        } else {
            find_icon_file(icon)
        };
        let uri = path.map(|p| format!("file://{}", p.display()));
        self.cache.insert(icon.to_string(), uri.clone());
        uri
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

// Lowercased lookup keys for a process: the exe basename, the full process
// name, and the leading alphanumeric stem of the name (so "code-insiders" can
// still match "code"). Shared by `icon_uri` and `has_desktop_entry` so both
// agree on what counts as a match.
fn desktop_candidates(proc_name: &str, exe_path: &str) -> [String; 3] {
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
    [exe_base, proc_lower, stem]
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

// `Exec=` may be prefixed with env vars or `env VAR=1`, plus shell wrappers
// like sh/bash -c. Skip those to recover the actual binary token.
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

fn find_icon_file(name: &str) -> Option<PathBuf> {
    let exts = ["png", "svg"];

    for base in pixmap_bases() {
        for ext in exts {
            let p = base.join(format!("{name}.{ext}"));
            if p.exists() {
                return Some(p);
            }
        }
    }

    // Larger sizes first — they downscale cleanly to the 16 px row glyph; tiny
    // hicolor entries (16/22) tend to be bitmap-only and look muddy when
    // upscaled.
    let sizes = [
        "48x48", "64x64", "128x128", "256x256", "32x32", "24x24", "22x22", "16x16", "scalable",
    ];
    let categories = ["apps", "devices", "places", "categories"];
    for theme_base in icon_theme_bases() {
        for size in sizes {
            for cat in categories {
                for ext in exts {
                    let p = theme_base
                        .join(size)
                        .join(cat)
                        .join(format!("{name}.{ext}"));
                    if p.exists() {
                        return Some(p);
                    }
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

fn icon_theme_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        bases.push(home.join(".local/share/icons/hicolor"));
        bases.push(home.join(".icons/hicolor"));
    }
    bases.push(PathBuf::from("/usr/local/share/icons/hicolor"));
    bases.push(PathBuf::from("/usr/share/icons/hicolor"));
    bases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_candidates_lowercases_and_extracts_stem() {
        let c = desktop_candidates("Code-Insiders", "/usr/bin/Code");
        assert_eq!(c[0], "code"); // exe basename, lowercased
        assert_eq!(c[1], "code-insiders"); // full name, lowercased
        assert_eq!(c[2], "code"); // leading alphanumeric stem
    }

    #[test]
    fn desktop_candidates_empty_inputs_yield_empty_strings() {
        let c = desktop_candidates("", "");
        assert!(c.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn has_desktop_entry_matches_any_candidate() {
        let mut name_to_icon = HashMap::new();
        name_to_icon.insert("firefox".to_string(), "firefox".to_string());
        let r = Resolver {
            name_to_icon: OnceCell::from(name_to_icon),
            cache: HashMap::new(),
        };
        // Matches via the process name even though the exe basename differs.
        assert!(r.has_desktop_entry("firefox", "/usr/lib/firefox/firefox-bin"));
        // No desktop entry → background process.
        assert!(!r.has_desktop_entry("kworker/0:1", ""));
    }

    #[test]
    fn has_desktop_entry_matches_on_stem() {
        let mut name_to_icon = HashMap::new();
        name_to_icon.insert("code".to_string(), "vscode".to_string());
        let r = Resolver {
            name_to_icon: OnceCell::from(name_to_icon),
            cache: HashMap::new(),
        };
        // "code-insiders" has no direct entry, but its stem "code" does.
        assert!(r.has_desktop_entry("code-insiders", ""));
    }

    #[test]
    fn shell_split_basic() {
        assert_eq!(shell_split("a b c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn shell_split_preserves_quoted_spaces() {
        // `Exec=` lines often quote arguments with spaces — those must stay
        // as one token so the binary detection doesn't trip on the path.
        assert_eq!(
            shell_split(r#"/usr/bin/foo "an arg" bar"#),
            vec!["/usr/bin/foo", "an arg", "bar"],
        );
    }

    #[test]
    fn shell_split_collapses_runs_of_whitespace() {
        assert_eq!(shell_split("a   b\tc"), vec!["a", "b", "c"]);
    }

    #[test]
    fn shell_split_empty_input() {
        let out: Vec<String> = shell_split("");
        assert!(out.is_empty());
    }

    #[test]
    fn exec_binary_skips_env_assignments() {
        // `Exec=FOO=bar baz` → binary is `baz`, not the env assignment.
        assert_eq!(exec_binary("FOO=bar baz"), Some("baz".to_string()));
    }

    #[test]
    fn exec_binary_skips_env_command_wrapper() {
        // `env` wrapper is common in `.desktop` Exec lines.
        assert_eq!(
            exec_binary("env VAR=1 /usr/bin/firefox"),
            Some("/usr/bin/firefox".to_string())
        );
    }

    #[test]
    fn exec_binary_passes_through_absolute_path() {
        // Absolute paths that contain `=` should NOT be treated as env.
        assert_eq!(
            exec_binary("/usr/bin/foo=weird"),
            Some("/usr/bin/foo=weird".to_string())
        );
    }

    #[test]
    fn exec_binary_returns_first_real_token() {
        assert_eq!(
            exec_binary("/usr/bin/firefox --new-window"),
            Some("/usr/bin/firefox".to_string())
        );
    }

    #[test]
    fn exec_binary_empty_returns_none() {
        assert_eq!(exec_binary(""), None);
        assert_eq!(exec_binary("   "), None);
    }

    #[test]
    fn parse_desktop_basic_entry_indexes_by_file_stem() {
        let content = "\
[Desktop Entry]
Type=Application
Name=Firefox
Exec=/usr/bin/firefox %u
Icon=firefox
";
        let mut out = HashMap::new();
        parse_desktop(content, "firefox", &mut out);
        // At a minimum the file stem must resolve.
        assert_eq!(out.get("firefox"), Some(&"firefox".to_string()));
    }

    #[test]
    fn parse_desktop_indexes_by_exec_tryexec_and_wmclass() {
        let content = "\
[Desktop Entry]
Type=Application
Name=Code
Exec=/usr/share/code/code --new-window %F
TryExec=/usr/share/code/code
StartupWMClass=Code
Icon=vscode
";
        let mut out = HashMap::new();
        parse_desktop(content, "code", &mut out);
        // Every alias resolves to the same icon.
        assert_eq!(out.get("code"), Some(&"vscode".to_string()));
    }

    #[test]
    fn parse_desktop_ignores_hidden_entries() {
        // `Hidden=true` means the launcher is suppressed by the user — the
        // icon should not be indexed (it may be a stale/orphaned file).
        let content = "\
[Desktop Entry]
Type=Application
Name=Hidden app
Exec=/usr/bin/hidden
Icon=hidden
Hidden=true
";
        let mut out = HashMap::new();
        parse_desktop(content, "hidden", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn parse_desktop_ignores_non_main_sections() {
        // Keys outside `[Desktop Entry]` (e.g. action sub-sections) must not
        // pollute the main entry.
        let content = "\
[Desktop Entry]
Type=Application
Exec=/usr/bin/foo
Icon=foo

[Desktop Action NewWindow]
Exec=/usr/bin/bar
Icon=bar
";
        let mut out = HashMap::new();
        parse_desktop(content, "foo", &mut out);
        assert_eq!(out.get("foo"), Some(&"foo".to_string()));
        assert!(!out.values().any(|v| v == "bar"));
    }

    #[test]
    fn parse_desktop_no_icon_skipped() {
        // Without an Icon= line there's nothing to index.
        let content = "\
[Desktop Entry]
Type=Application
Exec=/usr/bin/foo
";
        let mut out = HashMap::new();
        parse_desktop(content, "foo", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn parse_desktop_existing_key_not_overwritten() {
        // First desktop file wins — important because we scan multiple dirs
        // (user before system) and the user override should not be clobbered.
        let mut out = HashMap::new();
        out.insert("foo".to_string(), "user-icon".to_string());

        let content = "\
[Desktop Entry]
Type=Application
Exec=/usr/bin/foo
Icon=system-icon
";
        parse_desktop(content, "foo", &mut out);
        assert_eq!(out.get("foo"), Some(&"user-icon".to_string()));
    }
}
