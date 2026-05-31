use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::{StartupEntry, StartupSource};

pub(super) fn scan_dir(dir: PathBuf, source: StartupSource) -> Vec<StartupEntry> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(&dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        if let Some(e) = parse_desktop(&path, source.clone()) {
            out.push(e);
        }
    }
    out
}

fn parse_desktop(path: &PathBuf, source: StartupSource) -> Option<StartupEntry> {
    let content = fs::read_to_string(path).ok()?;
    let mut kv: HashMap<String, String> = HashMap::new();
    let mut in_main = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim().into(), v.trim().into());
        }
    }
    let hidden = kv.get("Hidden").map(|s| s == "true").unwrap_or(false);
    let xdg_autostart_enabled = kv
        .get("X-GNOME-Autostart-enabled")
        .map(|s| s == "true")
        .unwrap_or(true);
    Some(StartupEntry {
        path: path.clone(),
        source,
        name: kv.get("Name").cloned().unwrap_or_default(),
        exec: kv.get("Exec").cloned().unwrap_or_default(),
        comment: kv.get("Comment").cloned().unwrap_or_default(),
        icon: kv.get("Icon").cloned().unwrap_or_default(),
        enabled: !hidden && xdg_autostart_enabled,
        boot_time_ms: None,
        critical: false,
    })
}

pub(super) fn set_enabled(entry: &StartupEntry, enabled: bool) -> std::io::Result<()> {
    let content = fs::read_to_string(&entry.path)?;

    let mut new_lines: Vec<String> = Vec::new();
    let mut in_main = false;
    let mut wrote_hidden = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_main && !wrote_hidden {
                new_lines.push(format!("Hidden={}", !enabled));
                wrote_hidden = true;
            }
            in_main = trimmed == "[Desktop Entry]";
            new_lines.push(line.to_string());
            continue;
        }
        if in_main && trimmed.starts_with("Hidden=") {
            new_lines.push(format!("Hidden={}", !enabled));
            wrote_hidden = true;
        } else if in_main && trimmed.starts_with("X-GNOME-Autostart-enabled=") {
            new_lines.push(format!("X-GNOME-Autostart-enabled={enabled}"));
        } else {
            new_lines.push(line.to_string());
        }
    }
    if in_main && !wrote_hidden {
        new_lines.push(format!("Hidden={}", !enabled));
    }

    let target = if entry.source == StartupSource::SystemAutostart {
        let home = std::env::var("HOME")
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "no HOME"))?;
        let user_dir = PathBuf::from(format!("{home}/.config/autostart"));
        fs::create_dir_all(&user_dir)?;
        user_dir.join(
            entry
                .path
                .file_name()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no name"))?,
        )
    } else {
        entry.path.clone()
    };
    fs::write(&target, new_lines.join("\n"))
}
