//! Colors and global style (dark theme).

use egui::{Color32, FontId, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const PANEL: Color32 = Color32::from_rgb(0x36, 0x36, 0x36);
pub const PANEL_LIGHT: Color32 = Color32::from_rgb(0x42, 0x42, 0x42);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const SLOT: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
pub const LINE: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const GRID: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
pub const GRID_STRONG: Color32 = Color32::from_rgb(0x52, 0x52, 0x52);
pub const TEXT: Color32 = Color32::from_rgb(0xd2, 0xd2, 0xd2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8c, 0x8c, 0x8c);
pub const TEXT_DARK: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
pub const ACCENT: Color32 = Color32::from_rgb(0xff, 0xb0, 0x1f);
pub const PLAY: Color32 = Color32::from_rgb(0x3c, 0xe0, 0x60);
pub const REC: Color32 = Color32::from_rgb(0xff, 0x45, 0x45);
pub const SELECT: Color32 = Color32::from_rgb(0x8f, 0xc8, 0xff);
pub const KNOB_BG: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const BLUE: Color32 = Color32::from_rgb(0x5d, 0xb8, 0xff);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(TextStyle::Body, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(10.0));
    style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(15.0));
    style.spacing.item_spacing = egui::vec2(4.0, 3.0);
    style.spacing.button_padding = egui::vec2(5.0, 1.0);
    style.spacing.interact_size.y = 18.0;
    // No text selection, so dragging from labels always means drag & drop.
    style.interaction.selectable_labels = false;
    // Always-visible scroll bars (not the thin floating ones that only appear on hover).
    style.spacing.scroll = egui::style::ScrollStyle { bar_width: 10.0, handle_min_length: 24.0, ..egui::style::ScrollStyle::solid() };
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL_LIGHT;
    v.extreme_bg_color = PANEL_DARK;
    v.faint_bg_color = PANEL_DARK;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgb(0x4a, 0x78, 0xa8);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.bg_fill = PANEL_DARK;
    v.widgets.inactive.weak_bg_fill = PANEL_DARK;
    v.widgets.hovered.bg_fill = PANEL_LIGHT;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x4a, 0x4a, 0x4a);
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x55, 0x55, 0x55);
    ctx.set_style(style);
}

pub fn darken(c: Color32, f: f32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * f) as u8, (c.g() as f32 * f) as u8, (c.b() as f32 * f) as u8)
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Text color that reads on top of a clip color.
pub fn on_color(c: Color32) -> Color32 {
    let lum = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if lum > 140.0 {
        TEXT_DARK
    } else {
        Color32::WHITE
    }
}
