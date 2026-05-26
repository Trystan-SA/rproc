use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// Resolves a process to an icon URI by indexing the freedesktop `.desktop`
// files on disk and then locating the named icon under the standard
// `hicolor` theme + `pixmaps` search paths. Built once at startup; per-row
// lookups are pure hashmap reads.
pub struct Resolver {
    name_to_icon: HashMap<String, String>,
    cache: HashMap<String, Option<String>>,
}

impl Resolver {
    pub fn new() -> Self {
        let mut name_to_icon = HashMap::new();
        for dir in desktop_dirs() {
            scan_desktop_dir(&dir, &mut name_to_icon);
        }
        Self {
            name_to_icon,
            cache: HashMap::new(),
        }
    }

    pub fn icon_uri(&mut self, proc_name: &str, exe_path: &str) -> Option<String> {
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

        let candidates = [exe_base, proc_lower, stem];
        for cand in candidates {
            if cand.is_empty() {
                continue;
            }
            if let Some(icon) = self.name_to_icon.get(&cand).cloned()
                && let Some(uri) = self.resolve_icon(&icon)
            {
                return Some(uri);
            }
        }
        None
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
