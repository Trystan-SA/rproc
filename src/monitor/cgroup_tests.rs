use super::*;

const PREFIX: &str = "/user.slice/user-1000.slice/user@1000.service";

fn v2(path: &str) -> String {
    format!("0::{PREFIX}{path}")
}

#[test]
fn gnome_launched_app() {
    assert_eq!(
        app_id(&v2("/app.slice/app-gnome-firefox-3210.scope")).as_deref(),
        Some("firefox")
    );
}

#[test]
fn flatpak_keeps_reverse_dns_id() {
    assert_eq!(
        app_id(&v2("/app.slice/app-flatpak-org.mozilla.firefox-1234.scope")).as_deref(),
        Some("org.mozilla.firefox")
    );
}

#[test]
fn launcherless_scope() {
    assert_eq!(
        app_id(&v2("/app.slice/app-org.gnome.Nautilus-555.scope")).as_deref(),
        Some("org.gnome.Nautilus")
    );
}

#[test]
fn dbus_activated_service() {
    assert_eq!(
        app_id(&v2("/app.slice/dbus-:1.2-org.gnome.Foo@0.service")).as_deref(),
        Some("org.gnome.Foo")
    );
}

#[test]
fn templated_pid_instance_stripped() {
    assert_eq!(
        app_id(&v2("/app.slice/app-gnome-org.gnome.Console@12345.service")).as_deref(),
        Some("org.gnome.Console")
    );
}

#[test]
fn escaped_dash_in_id() {
    assert_eq!(
        app_id(&v2("/app.slice/app-gnome-foo\\x2dbar-99.scope")).as_deref(),
        Some("foo-bar")
    );
}

#[test]
fn snap_app() {
    assert_eq!(
        app_id(&v2(
            "/app.slice/snap.spotify.spotify-12345678-1234-1234-1234-123456789abc.scope"
        ))
        .as_deref(),
        Some("spotify_spotify")
    );
}

#[test]
fn backgrounded_app_still_counts() {
    assert_eq!(
        app_id(&v2("/background.slice/app-gnome-foo-1.scope")).as_deref(),
        Some("foo")
    );
}

#[test]
fn terminal_spawned_shell_is_not_an_app() {
    // The shell inside a terminal lives under app.slice but in a vte-spawn
    // scope, not an `app-` unit — it must stay in Background.
    assert_eq!(
        app_id(&v2(
            "/app.slice/vte-spawn-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.scope"
        )),
        None
    );
}

#[test]
fn session_service_is_not_an_app() {
    assert_eq!(app_id(&v2("/session.slice/pipewire.service")), None);
}

#[test]
fn system_daemon_is_not_an_app() {
    assert_eq!(app_id("0::/system.slice/cron.service"), None);
    assert_eq!(app_id("0::/system.slice/NetworkManager.service"), None);
}

#[test]
fn portal_service_under_app_slice_is_not_an_app() {
    // xdg-desktop-portal runs as a plain `.service` (no `app-` prefix).
    assert_eq!(app_id(&v2("/app.slice/xdg-desktop-portal.service")), None);
}

#[test]
fn root_and_empty() {
    assert_eq!(app_id("0::/"), None);
    assert_eq!(app_id(""), None);
}

#[test]
fn cgroup_v1_multiline() {
    let content = format!(
        "12:pids:{PREFIX}/app.slice/app-gnome-firefox-3210.scope\n\
         11:memory:{PREFIX}/app.slice/app-gnome-firefox-3210.scope\n\
         1:name=systemd:{PREFIX}/app.slice/app-gnome-firefox-3210.scope"
    );
    assert_eq!(app_id(&content).as_deref(), Some("firefox"));
}
