//! Custom widgets: knobs, faders with meters, toggle buttons.

use super::theme::*;
use crate::audio::dsp::{fader_to_gain, gain_to_db};
use crate::params::{ParamDef, Unit};
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};
use std::f32::consts::PI;

fn arc_points(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = ((a1 - a0).abs() / 0.12).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            pos2(c.x + r * a.cos(), c.y - r * a.sin())
        })
        .collect()
}

/// Rotary knob for a numeric parameter. Drag vertically, Shift for fine, double-click to reset.
pub fn knob(ui: &mut Ui, value: &mut f32, def: &ParamDef, size: f32, show_label: bool) -> Response {
    let h = size + if show_label { 26.0 } else { 0.0 };
    let w = size.max(46.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    let mut norm = def.to_norm(*value);
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let d = resp.drag_delta();
        norm = (norm + (d.x - d.y) / if fine { 800.0 } else { 160.0 }).clamp(0.0, 1.0);
        let nv = def.from_norm(norm);
        if nv != *value {
            *value = nv;
            resp.mark_changed();
        }
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            norm = (norm + scroll.signum() * 0.02).clamp(0.0, 1.0);
            *value = def.from_norm(norm);
            resp.mark_changed();
        }
    }
    if resp.double_clicked() {
        *value = def.def;
        resp.mark_changed();
    }
    let p = ui.painter_at(rect.expand(2.0));
    let top = if show_label { rect.top() + 12.0 } else { rect.top() };
    let c = pos2(rect.center().x, top + size / 2.0);
    let r = size / 2.0 - 3.0;
    if show_label {
        p.text(pos2(rect.center().x, rect.top()), Align2::CENTER_TOP, def.name, FontId::proportional(10.0), TEXT_DIM);
    }
    let a_start = 1.25 * PI;
    let a_end = -0.25 * PI;
    p.add(Shape::line(arc_points(c, r, a_start, a_end), Stroke::new(3.0_f32, KNOB_BG)));
    let bipolar = def.min < 0.0 && def.max > 0.0;
    let zero = if bipolar { def.to_norm(0.0) } else { 0.0 };
    let a_zero = a_start + (a_end - a_start) * zero;
    let a_val = a_start + (a_end - a_start) * norm;
    let col = if resp.dragged() || resp.hovered() { lerp(ACCENT, Color32::WHITE, 0.25) } else { ACCENT };
    if (a_val - a_zero).abs() > 0.01 {
        p.add(Shape::line(arc_points(c, r, a_zero, a_val), Stroke::new(3.0_f32, col)));
    }
    let tip = pos2(c.x + (r - 1.0) * a_val.cos(), c.y - (r - 1.0) * a_val.sin());
    p.line_segment([pos2(c.x + 0.3 * r * a_val.cos(), c.y - 0.3 * r * a_val.sin()), tip], Stroke::new(2.0_f32, TEXT));
    if show_label {
        p.text(
            pos2(rect.center().x, rect.bottom()),
            Align2::CENTER_BOTTOM,
            def.format(*value),
            FontId::proportional(10.0),
            if resp.dragged() { ACCENT } else { TEXT },
        );
    }
    resp
}

/// Knob or dropdown depending on the parameter type.
pub fn param(ui: &mut Ui, id: impl std::hash::Hash, value: &mut f32, def: &ParamDef, size: f32) -> Response {
    if !def.is_choice() {
        return knob(ui, value, def, size, true);
    }
    let r = ui
        .vertical(|ui| {
            ui.set_width(size.max(70.0));
            ui.label(egui::RichText::new(def.name).size(10.0).color(TEXT_DIM));
            let mut idx = value.round() as usize;
            let before = idx;
            egui::ComboBox::from_id_salt(id)
                .width(size.max(70.0) - 4.0)
                .selected_text(def.options.get(idx).copied().unwrap_or("?"))
                .show_ui(ui, |ui| {
                    for (i, o) in def.options.iter().enumerate() {
                        ui.selectable_value(&mut idx, i, *o);
                    }
                });
            if idx != before {
                *value = idx as f32;
                true
            } else {
                false
            }
        });
    let mut resp = r.response;
    if r.inner {
        resp.mark_changed();
    }
    resp
}

/// Small pan knob without labels (mixer strip).
pub fn pan_knob(ui: &mut Ui, value: &mut f32) -> Response {
    const DEF: ParamDef = ParamDef::new("Pan", -1.0, 1.0, 0.0, Unit::Pan);
    knob(ui, value, &DEF, 26.0, false).on_hover_text(DEF.format(*value))
}

pub fn send_knob(ui: &mut Ui, value: &mut f32, label: &str) -> Response {
    let def = ParamDef::new("Send", 0.0, 1.0, 0.0, Unit::Pct);
    let r = knob(ui, value, &def, 24.0, false);
    ui.painter().text(r.rect.center_bottom() + vec2(0.0, 1.0), Align2::CENTER_TOP, label, FontId::proportional(9.0), TEXT_DIM);
    r.on_hover_text(format!("Send {label}: {}", def.format(*value)))
}

fn meter_color(db: f32) -> Color32 {
    if db > -0.5 {
        REC
    } else if db > -6.0 {
        Color32::from_rgb(0xf0, 0xd0, 0x30)
    } else {
        PLAY
    }
}

/// Map dB to meter height fraction (0 dB at ~88%).
fn db_to_frac(db: f32) -> f32 {
    ((db + 60.0) / 66.0).clamp(0.0, 1.0)
}

pub fn draw_meter(p: &egui::Painter, rect: Rect, level: [f32; 2]) {
    p.rect_filled(rect, 0.0, KNOB_BG);
    let w = (rect.width() - 1.0) / 2.0;
    for (i, lv) in level.iter().enumerate() {
        let db = gain_to_db(*lv);
        let f = db_to_frac(db);
        let x = rect.left() + i as f32 * (w + 1.0);
        let r = Rect::from_min_max(pos2(x, rect.bottom() - rect.height() * f), pos2(x + w, rect.bottom()));
        p.rect_filled(r, 0.0, meter_color(db));
    }
}

/// Volume fader with stereo meter. Value is a fader position (0..1, 0.85 = 0 dB).
pub fn fader(ui: &mut Ui, value: &mut f32, level: [f32; 2], height: f32) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(40.0, height), Sense::click_and_drag());
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let dy = resp.drag_delta().y;
        *value = (*value - dy / if fine { height * 6.0 } else { height }).clamp(0.0, 1.0);
        resp.mark_changed();
    }
    if resp.double_clicked() {
        *value = 0.85;
        resp.mark_changed();
    }
    let p = ui.painter_at(rect);
    let meter = Rect::from_min_size(pos2(rect.left() + 22.0, rect.top() + 2.0), vec2(9.0, height - 4.0));
    draw_meter(&p, meter, level);
    // Scale ticks.
    for db in [0.0f32, -12.0, -24.0, -36.0, -48.0] {
        let y = meter.bottom() - meter.height() * db_to_frac(db);
        p.line_segment([pos2(meter.right() + 1.0, y), pos2(meter.right() + 4.0, y)], Stroke::new(1.0_f32, TEXT_DIM));
    }
    // Fader track and thumb (triangle).
    let track = Rect::from_min_size(pos2(rect.left() + 9.0, rect.top() + 2.0), vec2(3.0, height - 4.0));
    p.rect_filled(track, 1.0, KNOB_BG);
    let gain_db = gain_to_db(fader_to_gain(*value));
    let y = track.bottom() - track.height() * db_to_frac(gain_db.max(-60.0)).max(*value * 0.02);
    let col = if resp.dragged() { ACCENT } else { TEXT };
    p.add(Shape::convex_polygon(
        vec![pos2(rect.left() + 13.0, y), pos2(rect.left() + 21.0, y - 5.0), pos2(rect.left() + 21.0, y + 5.0)],
        col,
        Stroke::NONE,
    ));
    resp.on_hover_text(format!("{:.1} dB", gain_db))
}

/// Square toggle button (mute/solo/arm/activator).
pub fn toggle(ui: &mut Ui, text: &str, on: bool, on_color: Color32, size: egui::Vec2) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let bg = if on { on_color } else if resp.hovered() { PANEL_LIGHT } else { PANEL_DARK };
    p.rect_filled(rect, 2.0, bg);
    p.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, LINE), egui::StrokeKind::Inside);
    let fg = if on { on_color_text(on_color) } else { TEXT };
    if text == "●" {
        p.circle_filled(rect.center(), 4.0, fg);
    } else {
        p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(11.0), fg);
    }
    resp
}

fn on_color_text(c: Color32) -> Color32 {
    on_color(c)
}

/// Flat text button.
pub fn button(ui: &mut Ui, text: &str, active: bool) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), TEXT);
    let size = vec2(galley.size().x + 12.0, 19.0);
    toggle(ui, text, active, ACCENT, size)
}

pub fn color_swatch(ui: &mut Ui, c: Color32, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
    ui.painter().rect_filled(rect, 2.0, c);
    if selected || resp.hovered() {
        ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.5_f32, Color32::WHITE), egui::StrokeKind::Outside);
    }
    resp
}
