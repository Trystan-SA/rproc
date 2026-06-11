//! Colors reused by the Rust glue when it builds per-row / per-series data for
//! Slint. The full palette also lives in `ui/theme.slint` (the `Theme` global);
//! these mirror the handful the glue needs to set programmatically.
//!
//! Light/dark is a process-wide choice: the glue rebuilds row/series colors
//! every tick, so a single atomic read here keeps the call sites (which take no
//! theme argument) unchanged while the colors follow the user's selection. Set
//! it from `app.rs` whenever the theme toggles, and once at startup.
//!
//! `set_theme(Theme)` resolves `Theme::System` via D-Bus / gsettings and stores
//! the result; `set_dark(bool)` is the low-level setter used by the glue when
//! the resolved value is already known. Theme detection happens at startup
//! and when the user toggles the setting there is no background polling.

use std::sync::atomic::{AtomicBool, Ordering};

use slint::Color;

#[cfg(test)]
mod tests;

static DARK: AtomicBool = AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Theme enum + global
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Theme {
    Dark = 0,
    Light = 1,
    /// Follow the desktop colour scheme. Detected via D-Bus (portal) with a
    /// gsettings fallback for GNOME/Cinnamon. Resolved once at startup
    /// and when the user selects this option in settings.
    System = 2,
}

// ---------------------------------------------------------------------------
// System theme detection
// ---------------------------------------------------------------------------

/// Resolve a `Theme` to whether the UI should render dark. For
/// `Theme::System`, probes the desktop colour-scheme preference; falls back
/// to dark if detection fails.
pub fn resolve_theme(t: Theme) -> bool {
    match t {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => detect_system_theme() != Some(Theme::Light),
    }
}

/// Set the process-wide theme, resolving `Theme::System` via D-Bus / gsettings.
/// Returns the resolved dark/light boolean so the caller can forward it to the
/// Slint `Theme.dark` global.
pub fn set_theme(t: Theme) -> bool {
    let dark = resolve_theme(t);
    DARK.store(dark, Ordering::Relaxed);
    dark
}

/// Probe the desktop colour-scheme preference. Returns `Some(Theme::Light)`
/// for a light desktop, `Some(Theme::Dark)` for dark, and `None` if
/// detection failed (falls back to dark).
///
/// Tries in order:
/// 1. D-Bus freedesktop portal (`org.freedesktop.portal.Settings.Read`)
///    — the cross-desktop standard (GNOME, KDE, wlroots, etc.).
/// 2. `gsettings` — GNOME/Cinnamon fallback when D-Bus isn't available.
pub fn detect_system_theme() -> Option<Theme> {
    detect_via_dbus().or_else(detect_via_gsettings)
}

fn detect_via_dbus() -> Option<Theme> {
    let out = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings.Read",
            "string:org.freedesktop.appearance",
            "string:color-scheme",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_dbus_color_scheme(&String::from_utf8_lossy(&out.stdout))
}

/// Parse the portal reply from `dbus-send --print-reply`. The value arrives
/// as `variant variant uint32 N` — possibly on one line — so scan tokens
/// rather than expecting `uint32` at the start of a line.
fn parse_dbus_color_scheme(text: &str) -> Option<Theme> {
    let mut tokens = text.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok == "uint32" {
            return match tokens.next()?.parse::<u32>().ok()? {
                1 => Some(Theme::Dark),
                _ => Some(Theme::Light),
            };
        }
    }
    None
}

fn detect_via_gsettings() -> Option<Theme> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("prefer-dark") {
        Some(Theme::Dark)
    } else {
        Some(Theme::Light)
    }
}

pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
}

fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

fn pick(dark_rgb: (u8, u8, u8), light_rgb: (u8, u8, u8)) -> Color {
    let (r, g, b) = if dark() { dark_rgb } else { light_rgb };
    Color::from_rgb_u8(r, g, b)
}

pub fn accent() -> Color {
    pick((0x60, 0xCD, 0xFF), (0x00, 0x67, 0xC0))
}
pub fn text() -> Color {
    pick((0xE6, 0xE6, 0xE6), (0x1A, 0x1A, 0x1A))
}
pub fn text_dim() -> Color {
    pick((0x9A, 0x9A, 0x9A), (0x5C, 0x5C, 0x5C))
}
pub fn ok() -> Color {
    pick((0x55, 0xD1, 0x7C), (0x2E, 0x9E, 0x54))
}
pub fn warn() -> Color {
    pick((0xFF, 0xC4, 0x4D), (0xB8, 0x86, 0x0B))
}
pub fn err() -> Color {
    pick((0xFF, 0x6B, 0x6B), (0xD1, 0x34, 0x38))
}

pub fn graph_cpu() -> Color {
    Color::from_rgb_u8(0x39, 0xA7, 0xFF)
}
pub fn graph_ram() -> Color {
    Color::from_rgb_u8(0xB4, 0x6A, 0xFF)
}
pub fn graph_disk() -> Color {
    Color::from_rgb_u8(0x4E, 0xE0, 0xB3)
}
pub fn graph_net() -> Color {
    Color::from_rgb_u8(0xFF, 0xB0, 0x4E)
}
pub fn graph_gpu() -> Color {
    Color::from_rgb_u8(0xFF, 0x5C, 0x8A)
}
pub fn graph_wifi() -> Color {
    Color::from_rgb_u8(0x4E, 0xC9, 0xFF)
}
pub fn graph_battery() -> Color {
    Color::from_rgb_u8(0x8B, 0xE0, 0x4E)
}
pub fn graph_battery_drain() -> Color {
    Color::from_rgb_u8(0xFF, 0xA9, 0x4D)
}
