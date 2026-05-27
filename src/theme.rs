use egui::{Color32, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(0x60, 0xCD, 0xFF); // W11 mica blue
pub const BG: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1F);
pub const SIDEBAR_BG: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2A);
pub const PANEL_BG: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const CARD_BG: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);

pub const GRAPH_CPU: Color32 = Color32::from_rgb(0x39, 0xA7, 0xFF);
pub const GRAPH_RAM: Color32 = Color32::from_rgb(0xB4, 0x6A, 0xFF);
pub const GRAPH_DISK: Color32 = Color32::from_rgb(0x4E, 0xE0, 0xB3);
pub const GRAPH_NET: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x4E);
pub const GRAPH_GPU: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x8A);

pub const OK: Color32 = Color32::from_rgb(0x55, 0xD1, 0x7C);
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0xC4, 0x4D);
pub const ERR: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);

pub fn apply(ctx: &egui::Context) {
    // Drop the bundled emoji fonts. They're the heaviest blobs in the default
    // set (~1.5 MB of raw glyph data the font system would otherwise keep
    // parsed in memory) and rproc renders no emoji. Removing by name is a
    // no-op if egui ever renames them, so it can't accidentally blank the
    // Latin text fonts (Hack / Ubuntu-Light), which stay untouched.
    let mut fonts = egui::FontDefinitions::default();
    for emoji in ["NotoEmoji-Regular", "emoji-icon-font"] {
        fonts.font_data.remove(emoji);
        for family in fonts.families.values_mut() {
            family.retain(|name| name != emoji);
        }
    }
    ctx.set_fonts(fonts);

    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = PANEL_BG;
    visuals.widgets.noninteractive.bg_fill = SIDEBAR_BG;
    visuals.widgets.inactive.bg_fill = PANEL_BG;
    visuals.widgets.inactive.weak_bg_fill = PANEL_BG;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x38, 0x38, 0x38);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x38);
    visuals.widgets.active.bg_fill = Color32::from_rgb(0x44, 0x44, 0x44);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x44, 0x44, 0x44);
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(0x60, 0xCD, 0xFF, 60);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(8);
    style.text_styles = std::collections::BTreeMap::from([
        (
            TextStyle::Heading,
            FontId::new(20.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        ),
    ]);
    ctx.set_style(style);
}
