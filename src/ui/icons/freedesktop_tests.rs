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
fn has_entry_matches_any_candidate() {
    let keys: HashSet<String> = ["firefox".to_string()].into_iter().collect();
    // Matches via the process name even though the exe basename differs.
    assert!(has_entry(&keys, "firefox", "/usr/lib/firefox/firefox-bin"));
    // No desktop entry → background process.
    assert!(!has_entry(&keys, "kworker/0:1", ""));
}

#[test]
fn has_entry_matches_on_stem() {
    let keys: HashSet<String> = ["code".to_string()].into_iter().collect();
    // "code-insiders" has no direct entry, but its stem "code" does.
    assert!(has_entry(&keys, "code-insiders", ""));
}

#[test]
fn prefix_matches_recovers_truncated_and_suffixed_names() {
    // comm truncates at 15 chars: "brave-browser-stable" arrives truncated
    // and must still match its full key.
    assert!(prefix_matches("brave-browser-stable", "brave-browser-s"));
    // Longer process name carrying a suffix matches the shorter app key.
    assert!(prefix_matches("signal", "signal-desktop"));
    // Exact app names of decent length still match longer keys.
    assert!(prefix_matches("telegram-desktop", "telegram"));
    assert!(prefix_matches("brave-browser", "brave"));
}

#[test]
fn prefix_matches_rejects_short_command_names() {
    // The bug: a 3-char command grabbing the icon of any longer "git*" app.
    assert!(!prefix_matches("github-desktop", "git"));
    assert!(!prefix_matches("gitg", "git"));
    assert!(!prefix_matches("ssh-agent", "ssh"));
    assert!(!prefix_matches("codium", "code"));
    assert!(!prefix_matches("node-red", "node"));
    // Short key on the other side is rejected too.
    assert!(!prefix_matches("vim", "vim-gtk3"));
}

#[test]
fn is_loadable_icon_rejects_unsupported_formats() {
    // egui can decode png/svg/jpeg; xpm (python3.12.desktop's icon) and the
    // like must be rejected so we fall through to a usable fallback.
    assert!(is_loadable_icon(Path::new("/usr/share/pixmaps/vscode.png")));
    assert!(is_loadable_icon(Path::new("/x/icon.SVG")));
    assert!(is_loadable_icon(Path::new("/x/photo.jpeg")));
    assert!(!is_loadable_icon(Path::new(
        "/usr/share/pixmaps/python3.12.xpm"
    )));
    assert!(!is_loadable_icon(Path::new("/x/icon.gif")));
    assert!(!is_loadable_icon(Path::new("/x/noext")));
}

#[test]
fn pixmap_bases_includes_system_dir_first() {
    // The flat pixmap fallback must check the system dir; apps like VS Code
    // ship only /usr/share/pixmaps/vscode.png and nothing themed.
    let bases = pixmap_bases();
    assert_eq!(bases[0], PathBuf::from("/usr/share/pixmaps"));
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
