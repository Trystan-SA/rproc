//! Recover the freedesktop app id a process was launched as from its systemd
//! cgroup. On modern desktops every app the session launches lands in its own
//! `app.slice` scope named after its `.desktop` id, and child processes inherit
//! that cgroup — so this is the authoritative signal for the Apps/Background
//! split, far more reliable than matching a process name against installed
//! `.desktop` files. Mirrors the heuristic Resources and Mission Center use.

/// App id parsed from `/proc/<pid>/cgroup` content, e.g. `"firefox"`,
/// `"org.mozilla.firefox"`, or `"spotify_spotify"` for snaps. `None` when the
/// process isn't a user-launched application (kernel threads, daemons, the
/// shell, terminal-spawned shells).
pub fn app_id(cgroup: &str) -> Option<String> {
    cgroup.lines().find_map(|line| {
        // cgroup v2 lines are `0::/path`; v1 lines are `id:controller:/path`.
        // Take from the first '/' — the column separators are colons but the
        // path can itself contain a colon (dbus unit names like `dbus-:1.2-…`).
        let path = &line[line.find('/')?..];
        app_id_from_path(path)
    })
}

fn app_id_from_path(path: &str) -> Option<String> {
    let unit = path.rsplit('/').next()?;
    if let Some(id) = snap_app_id(unit) {
        return Some(id);
    }
    // Only the user app/background slices hold launched applications. The shell,
    // its session services, and terminal-spawned shells (`vte-spawn-*`) live
    // elsewhere or under non-`app-` units and must stay in Background.
    if !path.contains("/app.slice/") && !path.contains("/background.slice/") {
        return None;
    }
    systemd_app_id(unit)
}

fn systemd_app_id(unit: &str) -> Option<String> {
    let body = unit
        .strip_suffix(".scope")
        .or_else(|| unit.strip_suffix(".service"))?;
    let rest = body
        .strip_prefix("app-")
        .or_else(|| body.strip_prefix("dbus-:"))?;
    let rest = strip_instance(rest);
    // `app-<launcher>-<appid>` or `app-<appid>`. systemd escapes any literal
    // dash in the id as `\x2d`, so splitting on '-' never cuts a real id: the
    // first segment is the launcher (gnome/kde/flatpak/…) when a second exists.
    let mut parts = rest.split('-').filter(|s| !s.is_empty());
    let first = parts.next()?;
    let id = unescape(parts.next().unwrap_or(first));
    (!id.is_empty()).then_some(id)
}

fn snap_app_id(unit: &str) -> Option<String> {
    let body = unit.strip_prefix("snap.")?.strip_suffix(".scope")?;
    // `snap.<pkg>.<app>-<uuid>`; drop the trailing UUID and join pkg/app with
    // '_' to match the `<pkg>_<app>.desktop` files snap installs.
    let body = strip_uuid(body).unwrap_or(body);
    let (pkg, app) = body.split_once('.')?;
    if pkg.is_empty() || app.is_empty() {
        return None;
    }
    Some(format!("{pkg}_{app}"))
}

/// Drop the launch instance systemd appends — `-<pid>` for scopes, `@<pid>` for
/// templated units — so the id is stable across launches.
fn strip_instance(s: &str) -> &str {
    for sep in ['-', '@'] {
        if let Some((head, tail)) = s.rsplit_once(sep)
            && !tail.is_empty()
            && tail.bytes().all(|b| b.is_ascii_digit())
        {
            return head;
        }
    }
    s
}

/// Strip a trailing `-XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` UUID (the anchor in a
/// snap scope name, since the app segment may itself contain dashes).
fn strip_uuid(s: &str) -> Option<&str> {
    const UUID_LEN: usize = 36;
    let head = s.len().checked_sub(UUID_LEN + 1)?;
    let (head, tail) = s.split_at(head);
    is_uuid(tail.strip_prefix('-')?).then_some(head)
}

fn is_uuid(s: &str) -> bool {
    let mut groups = s.split('-');
    [8usize, 4, 4, 4, 12].iter().all(|&len| {
        groups
            .next()
            .is_some_and(|g| g.len() == len && g.bytes().all(|b| b.is_ascii_hexdigit()))
    }) && groups.next().is_none()
}

/// Decode systemd's `\xHH` escapes (notably `\x2d` for a literal dash). App ids
/// are ASCII, so a byte-wise decode is sufficient.
fn unescape(s: &str) -> String {
    if !s.contains("\\x") {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 4 <= b.len()
            && b[i + 1] == b'x'
            && let (Some(h), Some(l)) = (
                (b[i + 2] as char).to_digit(16),
                (b[i + 3] as char).to_digit(16),
            )
        {
            out.push((h * 16 + l) as u8 as char);
            i += 4;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
#[path = "cgroup_tests.rs"]
mod tests;
