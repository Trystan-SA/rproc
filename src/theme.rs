//! Colors reused by the Rust glue when it builds per-row / per-series data for
//! Slint. The full palette also lives in `ui/theme.slint` (the `Theme` global);
//! these mirror the handful the glue needs to set programmatically.
//!
//! Light/dark is a process-wide choice: the glue rebuilds row/series colors
//! every tick, so a single atomic read here keeps the call sites (which take no
//! theme argument) unchanged while the colors follow the user's selection. Set
//! it from `app.rs` whenever the theme toggles, and once at startup.

use std::sync::atomic::{AtomicBool, Ordering};

use slint::Color;

static DARK: AtomicBool = AtomicBool::new(true);

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
