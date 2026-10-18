//! Studio look: deep slate panels, soft gradients, rounded controls and a blue accent.

use egui::{Color32, FontId, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x18, 0x19, 0x1c);
pub const PANEL: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x31);
pub const PANEL_LIGHT: Color32 = Color32::from_rgb(0x3a, 0x3d, 0x44);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x21, 0x22, 0x26);
pub const SLOT: Color32 = Color32::from_rgb(0x25, 0x27, 0x2b);
pub const LINE: Color32 = Color32::from_rgb(0x12, 0x13, 0x15);
pub const GRID: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x32);
pub const GRID_STRONG: Color32 = Color32::from_rgb(0x41, 0x44, 0x4c);
pub const TEXT: Color32 = Color32::from_rgb(0xe3, 0xe5, 0xea);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8d, 0x93, 0x9e);
pub const TEXT_DARK: Color32 = Color32::from_rgb(0x10, 0x11, 0x14);
/// Main accent: active buttons, selected values, knob arcs.
pub const ACCENT: Color32 = Color32::from_rgb(0x4d, 0xa3, 0xff);
pub const PLAY: Color32 = Color32::from_rgb(0x3d, 0xd6, 0x8c);
pub const REC: Color32 = Color32::from_rgb(0xff, 0x4d, 0x5e);
pub const SELECT: Color32 = Color32::from_rgb(0xa8, 0xd2, 0xff);
pub const KNOB_BG: Color32 = Color32::from_rgb(0x15, 0x16, 0x19);
/// Secondary accent (panel toggles, solo).
pub const BLUE: Color32 = Color32::from_rgb(0x3f, 0xc7, 0xc0);
/// Mute, solo and cycle colors.
pub const MUTE_COL: Color32 = Color32::from_rgb(0x4f, 0x8f, 0xe8);
pub const SOLO_COL: Color32 = Color32::from_rgb(0xf2, 0xc9, 0x4c);
pub const CYCLE: Color32 = Color32::from_rgb(0xe8, 0xc4, 0x4a);
/// LCD display.
pub const LCD_BG: Color32 = Color32::from_rgb(0x0f, 0x14, 0x1b);
pub const LCD_TEXT: Color32 = Color32::from_rgb(0xc8, 0xe4, 0xff);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(TextStyle::Body, FontId::proportional(12.5));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(12.5));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(10.5));
    style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(16.0));
    style.spacing.item_spacing = egui::vec2(5.0, 4.0);
    style.spacing.button_padding = egui::vec2(7.0, 2.0);
    style.spacing.interact_size.y = 20.0;
    style.spacing.menu_margin = egui::Margin::same(6);
    // No text selection, so dragging from labels always means drag & drop.
    style.interaction.selectable_labels = false;
    // Always-visible scroll bars (not the thin floating ones that only appear on hover).
    style.spacing.scroll = egui::style::ScrollStyle { bar_width: 10.0, handle_min_length: 24.0, ..egui::style::ScrollStyle::solid() };
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(0x30, 0x33, 0x39);
    v.window_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(0x48, 0x4c, 0x55));
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(100) };
    v.extreme_bg_color = PANEL_DARK;
    v.faint_bg_color = PANEL_DARK;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgb(0x2f, 0x6f, 0xc4);
    v.hyperlink_color = ACCENT;
    let r = egui::CornerRadius::same(4);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.bg_fill = PANEL_DARK;
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x33, 0x36, 0x3c);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(0x1a, 0x1b, 0x1e));
    v.widgets.hovered.bg_fill = PANEL_LIGHT;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x43, 0x47, 0x4f);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(0x55, 0x5a, 0x64));
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x4b, 0x50, 0x59);
    v.widgets.open.weak_bg_fill = Color32::from_rgb(0x3d, 0x41, 0x49);
    ctx.set_style(style);
}

/// Vertical gradient fill (top to bottom), used for bars, headers and buttons.
pub fn gradient(p: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}

pub fn lighten(c: Color32, f: f32) -> Color32 {
    lerp(c, Color32::WHITE, f)
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
