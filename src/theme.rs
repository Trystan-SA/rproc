//! Colors reused by the Rust glue when it builds per-row / per-series data for
//! Slint. The full palette also lives in `ui/theme.slint` (the `Theme` global);
//! these mirror the handful the glue needs to set programmatically.

use slint::Color;

pub fn accent() -> Color {
    Color::from_rgb_u8(0x60, 0xCD, 0xFF)
}
pub fn text() -> Color {
    Color::from_rgb_u8(0xE6, 0xE6, 0xE6)
}
pub fn text_dim() -> Color {
    Color::from_rgb_u8(0x9A, 0x9A, 0x9A)
}
pub fn ok() -> Color {
    Color::from_rgb_u8(0x55, 0xD1, 0x7C)
}
pub fn warn() -> Color {
    Color::from_rgb_u8(0xFF, 0xC4, 0x4D)
}
pub fn err() -> Color {
    Color::from_rgb_u8(0xFF, 0x6B, 0x6B)
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
