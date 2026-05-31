pub(super) fn open_in_file_manager(exe: &str) {
    let path = std::path::Path::new(exe);
    let target = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

pub(super) fn open_search(name: &str) {
    let q = format!("linux process {name}");
    let url = format!("https://www.google.com/search?q={}", url_encode(&q));
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub(super) fn url_encode(s: &str) -> String {
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
