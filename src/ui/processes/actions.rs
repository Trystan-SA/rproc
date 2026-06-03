pub(crate) fn open_in_file_manager(exe: &str) {
    let path = std::path::Path::new(exe);
    let target = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

/// Best-effort copy to the system clipboard. Slint's software backend exposes
/// no clipboard API, so we shell out to the Wayland (`wl-copy`) or X11
/// (`xclip`) helper, whichever is present. Silently does nothing if neither is.
pub(crate) fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let candidates: [(&str, &[&str]); 2] =
        [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])];
    for (bin, args) in candidates {
        let Ok(mut child) = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
        return;
    }
}

pub(crate) fn open_search(name: &str) {
    let q = format!("linux process {name}");
    let url = format!("https://www.google.com/search?q={}", url_encode(&q));
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub(crate) fn url_encode(s: &str) -> String {
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

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
