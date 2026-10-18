//! Mixer: full-height channel strips with EQ, instrument, inserts, sends, output, pan, fader and meters.

use super::theme::*;
use super::widgets;
use super::{App, Detail, RenameTarget, Sel, MASTER_METER, RETURN_METER};
use crate::audio::dsp::{fader_to_gain, gain_to_db};
use crate::model::*;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Shape, Stroke, UiBuilder};
use std::collections::HashMap;

const STRIP_W: f32 = 92.0;
const MASTER_W: f32 = 104.0;
const GAP: f32 = 10.0;
const ROW_H: f32 = 18.0;
const EQ_H: f32 = 40.0;
const TOOLBAR_H: f32 = 24.0;
const MIN_FADER_H: f32 = 140.0;

/// Fader scale marks (dB) next to the fader.
const SCALE: [f32; 10] = [3.0, 0.0, -3.0, -6.0, -10.0, -15.0, -20.0, -30.0, -40.0, -60.0];

pub struct MixerState {
    pub show_eq: bool,
    pub show_inserts: bool,
    pub show_sends: bool,
    pub show_io: bool,
    /// The editor panel below the mixer; opened by clicking an insert, the EQ or the instrument.
    pub detail: bool,
    /// Per meter: highest level since the last reset and the falling peak-hold marker.
    peaks: HashMap<u64, Peak>,
}

#[derive(Clone, Copy, Default)]
struct Peak {
    max: f32,
    hold: f32,
    at: f64,
}

impl Default for MixerState {
    fn default() -> Self {
        Self { show_eq: true, show_inserts: true, show_sends: true, show_io: true, detail: false, peaks: HashMap::new() }
    }
}

/// Fader position (0..1) for a dB value; inverse of `fader_to_gain`.
fn db_to_pos(db: f32) -> f32 {
    0.85 * 10f32.powf(db / 20.0).powf(1.0 / 2.2)
}

/// Meter height fraction for a dB value, on the same scale as the fader so the marks fit both.
fn meter_frac(db: f32) -> f32 {
    if db < -100.0 {
        0.0
    } else {
        db_to_pos(db).clamp(0.0, 1.0)
    }
}

fn fmt_db(db: f32) -> String {
    if db < -100.0 {
        "-inf".into()
    } else {
        format!("{db:.1}")
    }
}

fn meter_id(sel: Sel) -> u64 {
    match sel {
        Sel::Track(id) => id,
        Sel::Return(k) => RETURN_METER + k as u64,
        Sel::Master => MASTER_METER,
    }
}

impl App {
    pub(super) fn mixer_view(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, BG);
        let (bar, _) = ui.allocate_exact_size(vec2(full.width(), TOOLBAR_H), Sense::hover());
        self.mixer_toolbar(ui, bar);

        let ids = self.visible_tracks();
        let n_ret = self.project.returns.len();
        let width = ids.len() as f32 * STRIP_W + GAP + n_ret as f32 * STRIP_W + GAP + MASTER_W + 8.0;
        // All strips share one insert slot count so the sections line up.
        let max_dev = ids
            .iter()
            .filter_map(|id| self.project.track(*id).map(|t| t.channel.devices.len()))
            .chain(self.project.returns.iter().map(|r| r.channel.devices.len()))
            .chain([self.project.master.devices.len()])
            .max()
            .unwrap_or(0);
        let slots = (max_dev + 1).max(4);
        let top_h = self.mixer_top_height(slots);
        let height = (ui.available_height()).max(top_h + MIN_FADER_H + 3.0 * ROW_H + 40.0);

        egui::ScrollArea::both().id_salt("mixer").drag_to_scroll(false).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let (area, _) = ui.allocate_exact_size(vec2(width.max(ui.available_width()), height), Sense::hover());
            let mut x = area.left() + 4.0;
            for (i, id) in ids.iter().copied().enumerate() {
                let r = Rect::from_min_size(pos2(x, area.top()), vec2(STRIP_W - 2.0, area.height()));
                self.channel_strip(ui, r, Sel::Track(id), i + 1, slots);
                x += STRIP_W;
            }
            x += GAP;
            for k in 0..n_ret {
                let r = Rect::from_min_size(pos2(x, area.top()), vec2(STRIP_W - 2.0, area.height()));
                self.channel_strip(ui, r, Sel::Return(k), k, slots);
                x += STRIP_W;
            }
            x += GAP;
            let r = Rect::from_min_size(pos2(x, area.top()), vec2(MASTER_W - 2.0, area.height()));
            self.channel_strip(ui, r, Sel::Master, 0, slots);
        });
    }

    fn mixer_top_height(&self, slots: usize) -> f32 {
        let m = &self.mixer;
        let mut h = 4.0;
        if m.show_eq {
            h += EQ_H + 4.0;
        }
        h += ROW_H + 4.0; // instrument / input
        if m.show_inserts {
            h += 12.0 + slots as f32 * ROW_H + 4.0;
        }
        if m.show_sends {
            h += 12.0 + self.project.returns.len() as f32 * ROW_H + 4.0;
        }
        if m.show_io {
            h += 12.0 + ROW_H + 4.0;
        }
        h
    }

    fn mixer_toolbar(&mut self, ui: &mut egui::Ui, r: Rect) {
        ui.painter().rect_filled(r, 0.0, PANEL);
        ui.scope_builder(UiBuilder::new().max_rect(r.shrink2(vec2(6.0, 2.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(egui::RichText::new("Mixer").size(12.0).strong());
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Show").size(10.0).color(TEXT_DIM));
                let mut toggles = [
                    (self.mixer.show_eq, "EQ", "EQ display at the top of each strip."),
                    (self.mixer.show_inserts, "Inserts", "Effect slots of each channel."),
                    (self.mixer.show_sends, "Sends", "Send levels to the return tracks."),
                    (self.mixer.show_io, "Output", "Output routing of each channel."),
                ];
                for (on, label, body) in toggles.iter_mut() {
                    let w = 10.0 + label.len() as f32 * 6.5;
                    let resp = widgets::toggle(ui, label, *on, BLUE, vec2(w, 18.0));
                    if resp.clicked() {
                        *on = !*on;
                    }
                    self.hint(&resp, label, body);
                }
                self.mixer.show_eq = toggles[0].0;
                self.mixer.show_inserts = toggles[1].0;
                self.mixer.show_sends = toggles[2].0;
                self.mixer.show_io = toggles[3].0;
                ui.add_space(8.0);
                let resp = widgets::button(ui, "Reset Peaks", false);
                if resp.clicked() {
                    self.mixer.peaks.clear();
                }
                self.hint(&resp, "Reset Peaks", "Clears the peak readouts above all faders.");
            });
        });
    }

    fn channel_strip(&mut self, ui: &mut egui::Ui, r: Rect, sel: Sel, number: usize, slots: usize) {
        let (name, color) = match sel {
            Sel::Track(id) => match self.project.track(id) {
                Some(t) => (t.name.clone(), Color32::from_rgb(t.color[0], t.color[1], t.color[2])),
                None => return,
            },
            Sel::Return(k) => {
                let rt = &self.project.returns[k];
                (rt.name.clone(), Color32::from_rgb(rt.color[0], rt.color[1], rt.color[2]))
            }
            Sel::Master => ("Master".to_string(), Color32::from_rgb(0x9a, 0x9a, 0x9a)),
        };
        let selected = self.sel == sel;
        let p = ui.painter().clone();
        p.rect_filled(r, 3.0, PANEL);
        if selected {
            p.rect_stroke(r, 3.0, Stroke::new(1.0_f32, SELECT), egui::StrokeKind::Inside);
        }
        let strip = ui.interact(r, ui.id().with(("mix_strip", format!("{sel:?}"))), Sense::click());
        let inner = r.shrink2(vec2(4.0, 4.0));
        let mut y = inner.top();
        let row = |y: f32, h: f32| Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), h));

        if self.mixer.show_eq {
            self.strip_eq(ui, row(y, EQ_H), sel);
            y += EQ_H + 4.0;
        }
        self.strip_input(ui, row(y, ROW_H), sel);
        y += ROW_H + 4.0;
        if self.mixer.show_inserts {
            p.text(pos2(inner.left(), y), Align2::LEFT_TOP, "Inserts", FontId::proportional(9.0), TEXT_DIM);
            y += 12.0;
            self.strip_inserts(ui, row(y, slots as f32 * ROW_H), sel, slots);
            y += slots as f32 * ROW_H + 4.0;
        }
        if self.mixer.show_sends {
            p.text(pos2(inner.left(), y), Align2::LEFT_TOP, "Sends", FontId::proportional(9.0), TEXT_DIM);
            y += 12.0;
            let send_h = self.project.returns.len() as f32 * ROW_H;
            if let Sel::Track(_) = sel {
                self.strip_sends(ui, row(y, send_h), sel);
            }
            y += send_h + 4.0;
        }
        if self.mixer.show_io {
            p.text(pos2(inner.left(), y), Align2::LEFT_TOP, "Output", FontId::proportional(9.0), TEXT_DIM);
            y += 12.0;
            let rr = row(y, ROW_H);
            match sel {
                Sel::Track(id) => {
                    ui.scope_builder(UiBuilder::new().max_rect(rr), |ui| {
                        ui.style_mut().spacing.combo_width = rr.width();
                        self.output_combo(ui, id);
                    });
                }
                _ => {
                    p.rect_filled(rr, 2.0, PANEL_DARK);
                    let txt = if sel == Sel::Master { "Stereo Out" } else { "Master" };
                    p.text(rr.center(), Align2::CENTER_CENTER, txt, FontId::proportional(10.0), TEXT_DIM);
                }
            }
            y += ROW_H + 4.0;
        }

        // Lower part: pan, readouts, fader with meter, buttons, name.
        let name_h = 22.0;
        let btn_h = 20.0;
        let bottom = inner.bottom();
        let name_r = Rect::from_min_max(pos2(inner.left(), bottom - name_h), pos2(inner.right(), bottom));
        let btn_r = Rect::from_min_max(pos2(inner.left(), name_r.top() - btn_h - 4.0), pos2(inner.right(), name_r.top() - 4.0));
        self.strip_controls(ui, Rect::from_min_max(pos2(inner.left(), y), pos2(inner.right(), btn_r.top() - 4.0)), btn_r, sel);

        // Name field in the track color.
        p.rect_filled(name_r, 2.0, color);
        let renaming = matches!((&self.renaming, sel), (Some((RenameTarget::Track(a), _)), Sel::Track(b)) if *a == b);
        if renaming {
            self.rename_field(ui, name_r);
        } else {
            let label = match sel {
                Sel::Track(_) => format!("{number} {name}"),
                _ => name.clone(),
            };
            p.with_clip_rect(name_r.shrink(2.0)).text(name_r.center(), Align2::CENTER_CENTER, label, FontId::proportional(11.0), on_color(color));
        }
        let nresp = ui.interact(name_r, ui.id().with(("mix_name", format!("{sel:?}"))), Sense::click());
        if let Sel::Track(id) = sel {
            if nresp.double_clicked() {
                self.renaming = Some((RenameTarget::Track(id), name.clone()));
            }
            nresp.context_menu(|ui| self.track_menu(ui, id));
        }
        if let Sel::Return(k) = sel {
            nresp.context_menu(|ui| self.return_menu(ui, k));
        }
        if nresp.clicked() {
            self.sel = sel;
            self.detail = Detail::Devices;
        }
        self.hint(&nresp, &name, "Channel name. Click to select, double-click to rename, right-click for track options.");

        if let Some(payload) = self.drop_target(ui, r, |p| matches!(p, super::Payload::Device(_) | super::Payload::MidiFx(_) | super::Payload::Instrument(_) | super::Payload::Plugin(_))) {
            match (payload, sel) {
                (super::Payload::Device(kind), _) => self.add_device(sel, kind, None),
                (super::Payload::Plugin(i), _) => self.add_plugin(i, Some(sel)),
                (p, Sel::Track(id)) => self.drop_on_track(id, p),
                _ => {}
            }
        }
        if strip.clicked() {
            self.sel = sel;
            self.detail = Detail::Devices;
        }
    }

    fn channel(&self, sel: Sel) -> Option<&Channel> {
        match sel {
            Sel::Track(id) => self.project.track(id).map(|t| &t.channel),
            Sel::Return(k) => self.project.returns.get(k).map(|r| &r.channel),
            Sel::Master => Some(&self.project.master),
        }
    }

    /// EQ thumbnail: shows the channel's first EQ; click opens it or inserts a Channel EQ.
    fn strip_eq(&mut self, ui: &mut egui::Ui, r: Rect, sel: Sel) {
        let eq = self.channel(sel).and_then(|c| c.devices.iter().find(|d| d.kind.name().contains("EQ")).map(|d| (d.id, d.kind, d.on, d.params.clone())));
        let p = ui.painter_at(r);
        p.rect_filled(r, 2.0, KNOB_BG);
        for i in 1..4 {
            let x = r.left() + r.width() * i as f32 / 4.0;
            p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(1.0_f32, PANEL_DARK));
        }
        p.line_segment([pos2(r.left(), r.center().y), pos2(r.right(), r.center().y)], Stroke::new(1.0_f32, PANEL_DARK));
        let resp = ui.interact(r, ui.id().with(("mix_eq", format!("{sel:?}"))), Sense::click());
        match &eq {
            Some((_, kind, on, params)) => {
                // Sketch the curve from the band gains (every parameter in dB units of the EQ).
                let gains: Vec<f32> = kind
                    .defs()
                    .iter()
                    .zip(params)
                    .filter(|(d, _)| d.unit == crate::params::Unit::Db && d.min < 0.0 && !matches!(d.name, "Output" | "Gain" | "Out"))
                    .map(|(_, v)| *v)
                    .collect();
                let col = if *on { ACCENT } else { TEXT_DIM };
                if !gains.is_empty() {
                    let n = gains.len();
                    let pts: Vec<_> = (0..=40)
                        .map(|i| {
                            let t = i as f32 / 40.0;
                            let mut g = 0.0;
                            for (b, gain) in gains.iter().enumerate() {
                                let c = (b as f32 + 0.5) / n as f32;
                                let w = (-((t - c) * n as f32 * 1.6).powi(2)).exp();
                                g += gain * w;
                            }
                            pos2(r.left() + t * r.width(), r.center().y - (g / 24.0).clamp(-1.0, 1.0) * (r.height() / 2.0 - 3.0))
                        })
                        .collect();
                    p.add(Shape::line(pts, Stroke::new(1.5_f32, col)));
                } else {
                    p.text(r.center(), Align2::CENTER_CENTER, kind.name(), FontId::proportional(10.0), col);
                }
            }
            None => {
                p.text(r.center(), Align2::CENTER_CENTER, "EQ", FontId::proportional(11.0), if resp.hovered() { TEXT } else { TEXT_DIM });
            }
        }
        if resp.clicked() {
            match eq {
                Some((id, ..)) => {
                    self.sel = sel;
                    self.sel_device = Some(id);
                    self.detail = Detail::Devices;
                    self.mixer.detail = true;
                }
                None => self.add_device(sel, DeviceKind::ChannelEq, Some(0)),
            }
        }
        self.hint(&resp, "Channel EQ", "Shows the first EQ of this channel. Click to edit it, or to insert a Channel EQ if there is none.");
    }

    /// Instrument slot (tracks) or channel type label.
    fn strip_input(&mut self, ui: &mut egui::Ui, r: Rect, sel: Sel) {
        let (txt, body) = match sel {
            Sel::Track(id) => {
                let Some(t) = self.project.track(id) else { return };
                if t.instrument.kind == InstrumentKind::Audio {
                    ("Audio In".to_string(), "Audio track: plays audio clips and records the audio input.")
                } else {
                    (t.instrument.kind.name().to_string(), "The instrument of this track. Click to edit it.")
                }
            }
            Sel::Return(k) => (format!("Return {}", return_letter(k)), "Return track fed by the Send knobs."),
            Sel::Master => ("Stereo Out".to_string(), "The master output."),
        };
        let resp = ui.interact(r, ui.id().with(("mix_in", format!("{sel:?}"))), Sense::click());
        let p = ui.painter();
        p.rect_filled(r, 2.0, if resp.hovered() { PANEL_LIGHT } else { SLOT });
        p.with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, &txt, FontId::proportional(10.5), if matches!(sel, Sel::Track(_)) { SELECT } else { TEXT_DIM });
        if resp.clicked() {
            self.sel = sel;
            self.sel_device = None;
            self.detail = Detail::Devices;
            self.mixer.detail = true;
        }
        self.hint(&resp, &txt, body);
    }

    fn strip_inserts(&mut self, ui: &mut egui::Ui, r: Rect, sel: Sel, slots: usize) {
        let devs: Vec<(u64, String, bool)> = self
            .channel(sel)
            .map(|c| {
                c.devices
                    .iter()
                    .map(|d| (d.id, d.plugin.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| d.kind.name().to_string()), d.on))
                    .collect()
            })
            .unwrap_or_default();
        enum Act {
            Toggle(usize),
            Remove(usize),
            Up(usize),
            Down(usize),
        }
        let mut act = None;
        for k in 0..slots {
            let sr = Rect::from_min_size(pos2(r.left(), r.top() + k as f32 * ROW_H), vec2(r.width(), ROW_H - 2.0));
            let resp = ui.interact(sr, ui.id().with(("mix_ins", format!("{sel:?}"), k)), Sense::click());
            let p = ui.painter();
            match devs.get(k) {
                Some((id, name, on)) => {
                    let active = self.sel == sel && self.sel_device == Some(*id);
                    let bg = if active { Color32::from_rgb(0x3d, 0x55, 0x70) } else if resp.hovered() { PANEL_LIGHT } else { Color32::from_rgb(0x2f, 0x3c, 0x4a) };
                    p.rect_filled(sr, 2.0, bg);
                    // Power dot on the left toggles bypass.
                    let dot = Rect::from_min_size(sr.left_top(), vec2(14.0, sr.height()));
                    p.circle_filled(dot.center(), 3.5, if *on { ACCENT } else { KNOB_BG });
                    p.with_clip_rect(sr.with_min_x(dot.right()).shrink(1.0)).text(
                        pos2(dot.right() + 1.0, sr.center().y),
                        Align2::LEFT_CENTER,
                        name,
                        FontId::proportional(10.0),
                        if *on { TEXT } else { TEXT_DIM },
                    );
                    if resp.clicked() {
                        let on_dot = resp.interact_pointer_pos().is_some_and(|pt| dot.contains(pt));
                        if on_dot {
                            act = Some(Act::Toggle(k));
                        } else {
                            self.sel = sel;
                            self.sel_device = Some(*id);
                            self.detail = Detail::Devices;
                            self.mixer.detail = true;
                        }
                    }
                    resp.context_menu(|ui| {
                        if ui.button(if *on { "Bypass" } else { "Enable" }).clicked() {
                            act = Some(Act::Toggle(k));
                            ui.close_menu();
                        }
                        if k > 0 && ui.button("Move Up").clicked() {
                            act = Some(Act::Up(k));
                            ui.close_menu();
                        }
                        if k + 1 < devs.len() && ui.button("Move Down").clicked() {
                            act = Some(Act::Down(k));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Remove").clicked() {
                            act = Some(Act::Remove(k));
                            ui.close_menu();
                        }
                    });
                    self.hint(&resp, name, "Insert effect. Click to edit it below, click the dot to bypass, right-click to reorder or remove.");
                }
                None => {
                    p.rect_filled(sr, 2.0, if resp.hovered() { PANEL_LIGHT } else { SLOT });
                    if k == devs.len() {
                        p.text(sr.center(), Align2::CENTER_CENTER, "+", FontId::proportional(12.0), if resp.hovered() { TEXT } else { TEXT_DIM });
                        let popup = ui.make_persistent_id(("mix_add", format!("{sel:?}")));
                        if resp.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(popup));
                        }
                        egui::popup_below_widget(ui, popup, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(170.0);
                            for (folder, kinds) in DEVICE_CATEGORIES {
                                ui.menu_button(*folder, |ui| {
                                    for kind in kinds.iter().copied() {
                                        if ui.button(kind.name()).on_hover_text(kind.info()).clicked() {
                                            self.add_device(sel, kind, None);
                                            ui.memory_mut(|m| m.close_popup());
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        });
                        self.hint(&resp, "Add Insert", "Click to add an effect to this channel, or drop one from the browser.");
                    }
                }
            }
        }
        let Some(act) = act else { return };
        let Some(ch) = self.channel_mut(sel) else { return };
        match act {
            Act::Toggle(i) => ch.devices[i].on = !ch.devices[i].on,
            Act::Remove(i) => {
                ch.devices.remove(i);
            }
            Act::Up(i) => ch.devices.swap(i, i - 1),
            Act::Down(i) => ch.devices.swap(i, i + 1),
        }
        self.touch();
    }

    fn strip_sends(&mut self, ui: &mut egui::Ui, r: Rect, sel: Sel) {
        let Sel::Track(id) = sel else { return };
        let Some(mut sends) = self.project.track(id).map(|t| t.channel.sends) else { return };
        let before = sends;
        let n_ret = self.project.returns.len();
        for (k, v) in sends.iter_mut().enumerate().take(n_ret) {
            let sr = Rect::from_min_size(pos2(r.left(), r.top() + k as f32 * ROW_H), vec2(r.width(), ROW_H - 2.0));
            let resp = ui.interact(sr, ui.id().with(("mix_send", id, k)), Sense::click_and_drag());
            if resp.dragged() {
                let fine = ui.input(|i| i.modifiers.shift);
                *v = (*v + resp.drag_delta().x / sr.width() * if fine { 0.2 } else { 1.0 }).clamp(0.0, 1.0);
            }
            if resp.double_clicked() {
                *v = 0.0;
            }
            let p = ui.painter();
            p.rect_filled(sr, 2.0, KNOB_BG);
            let fill = Rect::from_min_size(sr.min, vec2(sr.width() * *v, sr.height()));
            p.rect_filled(fill, 2.0, Color32::from_rgb(0x2e, 0x6b, 0x52));
            let ret = self.project.returns.get(k).map(|r| r.name.clone()).unwrap_or_default();
            let clip = p.with_clip_rect(sr.shrink(1.0));
            clip.text(sr.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, &ret, FontId::proportional(9.5), TEXT);
            clip.text(sr.right_center() - vec2(3.0, 0.0), Align2::RIGHT_CENTER, format!("{:.0}", *v * 100.0), FontId::monospace(9.0), TEXT_DIM);
            resp.context_menu(|ui| self.map_menu(ui, MapTarget::Send(id, k as u8), Some(id)));
            self.hint(&resp, &format!("Send to {ret}"), "Drag sideways to set how much of this track goes to the return (Shift: fine). Double-click for off. Right-click to map.");
        }
        if sends != before {
            if let Some(t) = self.project.track_mut(id) {
                t.channel.sends = sends;
            }
            self.touch();
        }
    }

    /// Pan, level readouts, fader with meter and the mute/solo/arm buttons.
    fn strip_controls(&mut self, ui: &mut egui::Ui, r: Rect, btn_r: Rect, sel: Sel) {
        let Some(mut ch) = self.channel(sel).cloned() else { return };
        let before = (ch.volume, ch.pan, ch.mute, ch.solo);
        let mid = meter_id(sel);
        let armed_audio = match sel {
            Sel::Track(id) => self.project.track(id).map(|t| (t.arm, t.instrument.kind == InstrumentKind::Audio)),
            _ => None,
        };
        let level = match armed_audio {
            Some((true, true)) => [self.input_level(); 2],
            _ => self.meter(mid),
        };
        // Peak tracking.
        let now = ui.input(|i| i.time);
        let lv = level[0].max(level[1]);
        let pk = self.mixer.peaks.entry(mid).or_default();
        pk.max = pk.max.max(lv);
        if lv >= pk.hold || now - pk.at > 1.5 {
            pk.hold = lv;
            pk.at = now;
        }
        let peak = *pk;

        // Pan knob.
        let pan_r = Rect::from_min_size(r.min, vec2(r.width(), 34.0));
        let mut exclusive_solo = false;
        if sel != Sel::Master {
            ui.scope_builder(UiBuilder::new().max_rect(pan_r), |ui| {
                ui.vertical_centered(|ui| {
                    let rp = widgets::pan_knob(ui, &mut ch.pan);
                    if let Sel::Track(id) = sel {
                        rp.context_menu(|ui| self.map_menu(ui, MapTarget::Pan(id), Some(id)));
                    }
                    self.hint(&rp, "Pan", "Stereo position. Double-click to center.");
                });
            });
            let pan = if ch.pan.abs() < 0.005 { "C".to_string() } else if ch.pan < 0.0 { format!("{:.0}L", -ch.pan * 50.0) } else { format!("{:.0}R", ch.pan * 50.0) };
            ui.painter().text(pan_r.right_top() + vec2(-2.0, 2.0), Align2::RIGHT_TOP, pan, FontId::monospace(9.0), TEXT_DIM);
        }

        // Readouts: fader value and peak (click resets).
        let ro = Rect::from_min_size(pos2(r.left(), pan_r.bottom() + 2.0), vec2(r.width(), 16.0));
        let half = ro.width() / 2.0 - 1.0;
        let vr = Rect::from_min_size(ro.min, vec2(half, ro.height()));
        let pr = Rect::from_min_size(pos2(ro.left() + half + 2.0, ro.top()), vec2(half, ro.height()));
        let p = ui.painter();
        p.rect_filled(vr, 2.0, KNOB_BG);
        p.text(vr.center(), Align2::CENTER_CENTER, fmt_db(gain_to_db(fader_to_gain(ch.volume))), FontId::monospace(9.5), TEXT);
        let clip_hit = peak.max >= 1.0;
        p.rect_filled(pr, 2.0, if clip_hit { REC } else { KNOB_BG });
        p.text(pr.center(), Align2::CENTER_CENTER, fmt_db(gain_to_db(peak.max)), FontId::monospace(9.5), if clip_hit { Color32::WHITE } else { TEXT_DIM });
        let presp = ui.interact(pr, ui.id().with(("mix_peak", mid)), Sense::click());
        if presp.clicked() {
            self.mixer.peaks.remove(&mid);
        }
        self.hint(&presp, "Peak", "Highest level since the last reset; red means the channel went over 0 dB. Click to reset.");

        // Fader and meter.
        let fr = Rect::from_min_max(pos2(r.left(), ro.bottom() + 6.0), pos2(r.right(), r.bottom()));
        let fresp = self.big_fader(ui, fr, &mut ch.volume, level, peak.hold, mid);
        let vol_target = match sel {
            Sel::Track(id) => MapTarget::Volume(id),
            Sel::Return(k) => MapTarget::ReturnVolume(k as u8),
            Sel::Master => MapTarget::MasterVolume,
        };
        let tid = if let Sel::Track(id) = sel { Some(id) } else { None };
        fresp.context_menu(|ui| self.map_menu(ui, vol_target, tid));
        self.hint(&fresp, "Volume", "Drag the fader (Shift: fine), double-click for 0 dB. Right-click to map it to a MIDI control.");

        // Buttons.
        if sel != Sel::Master {
            let any_solo = self.project.any_solo();
            ui.scope_builder(UiBuilder::new().max_rect(btn_r), |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let w = if armed_audio.is_some_and(|(_, audio)| audio) { 24.0 } else { 36.0 };
                    let rm = widgets::toggle(ui, "M", ch.mute, Color32::from_rgb(0x4f, 0x9c, 0xe8), vec2(w, 18.0));
                    if rm.clicked() {
                        ch.mute = !ch.mute;
                    }
                    if let Sel::Track(id) = sel {
                        rm.context_menu(|ui| self.map_menu(ui, MapTarget::Mute(id), None));
                    }
                    self.hint(&rm, "Mute", "Silences this channel.");
                    let rs = widgets::toggle(ui, "S", ch.solo, Color32::from_rgb(0xf0, 0xd0, 0x30), vec2(w, 18.0));
                    if rs.clicked() {
                        ch.solo = !ch.solo;
                        exclusive_solo = ch.solo && !ui.input(|i| i.modifiers.command);
                    }
                    if let Sel::Track(id) = sel {
                        rs.context_menu(|ui| self.map_menu(ui, MapTarget::Solo(id), None));
                    }
                    self.hint(&rs, "Solo", "Plays only soloed channels. Ctrl-click to solo several.");
                    if let Some((armed, true)) = armed_audio {
                        let rr = widgets::toggle(ui, "R", armed, REC, vec2(w, 18.0));
                        if rr.clicked() {
                            if let Sel::Track(id) = sel {
                                self.toggle_arm(id);
                            }
                        }
                        self.hint(&rr, "Record Arm", "Arms this audio track for recording from the audio input.");
                    }
                });
            });
            if any_solo && !ch.solo && matches!(sel, Sel::Track(_)) {
                ui.painter().rect_stroke(btn_r, 2.0, Stroke::new(1.0_f32, Color32::from_rgb(0x60, 0x58, 0x20)), egui::StrokeKind::Outside);
            }
        }

        if (ch.volume, ch.pan, ch.mute, ch.solo) != before {
            if exclusive_solo {
                for t in &mut self.project.tracks {
                    t.channel.solo = false;
                }
                for rt in &mut self.project.returns {
                    rt.channel.solo = false;
                }
            }
            if let Some(c) = self.channel_mut(sel) {
                c.volume = ch.volume;
                c.pan = ch.pan;
                c.mute = ch.mute;
                c.solo = ch.solo;
            }
            self.touch();
        }
    }

    /// Tall fader with dB scale, stereo meter and peak-hold marker.
    fn big_fader(&mut self, ui: &mut egui::Ui, r: Rect, value: &mut f32, level: [f32; 2], hold: f32, mid: u64) -> egui::Response {
        let track_x = r.left() + 14.0;
        let meter = Rect::from_min_max(pos2(r.right() - 16.0, r.top() + 6.0), pos2(r.right() - 2.0, r.bottom() - 6.0));
        // Fader and meter share one vertical scale.
        let lane = Rect::from_min_max(pos2(r.left(), r.top()), pos2(r.left() + 30.0, r.bottom()));
        let span = Rect::from_min_max(pos2(lane.left(), r.top() + 6.0), pos2(lane.right(), r.bottom() - 6.0));
        let y_of = |pos: f32| span.bottom() - span.height() * pos.clamp(0.0, 1.0);
        let mut resp = ui.interact(lane, ui.id().with(("mix_fader", mid)), Sense::click_and_drag());
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            *value = (*value - resp.drag_delta().y / span.height() * if fine { 0.15 } else { 1.0 }).clamp(0.0, 1.0);
            resp.mark_changed();
        }
        if resp.double_clicked() {
            *value = 0.85;
            resp.mark_changed();
        }
        let p = ui.painter_at(r.expand(2.0));
        // Fader track.
        p.rect_filled(Rect::from_min_max(pos2(track_x - 2.0, span.top()), pos2(track_x + 2.0, span.bottom())), 2.0, KNOB_BG);
        // Scale between fader and meter.
        for db in SCALE {
            let y = y_of(db_to_pos(db));
            let t = if db > 0.0 { format!("+{db:.0}") } else { format!("{db:.0}") };
            p.text(pos2(meter.left() - 3.0, y), Align2::RIGHT_CENTER, t, FontId::proportional(8.5), if db == 0.0 { TEXT } else { TEXT_DIM });
            p.line_segment([pos2(track_x + 4.0, y), pos2(track_x + 9.0, y)], Stroke::new(1.0_f32, if db == 0.0 { TEXT } else { GRID_STRONG }));
        }
        // Meter with color zones.
        p.rect_filled(meter, 1.0, KNOB_BG);
        let w = (meter.width() - 2.0) / 2.0;
        for (i, lv) in level.iter().enumerate() {
            let db = gain_to_db(*lv);
            let f = meter_frac(db);
            let x = meter.left() + i as f32 * (w + 2.0);
            let top = meter.bottom() - meter.height() * f;
            for (lo, hi, col) in [(-120.0, -18.0, PLAY), (-18.0, -6.0, Color32::from_rgb(0xb8, 0xe0, 0x40)), (-6.0, 0.0, Color32::from_rgb(0xf0, 0xd0, 0x30)), (0.0, 6.0, REC)] {
                let y0 = meter.bottom() - meter.height() * meter_frac(lo);
                let y1 = meter.bottom() - meter.height() * meter_frac(hi);
                let seg_top = y1.max(top);
                if seg_top < y0 {
                    p.rect_filled(Rect::from_min_max(pos2(x, seg_top), pos2(x + w, y0)), 0.0, col);
                }
            }
        }
        let hold_db = gain_to_db(hold);
        if hold_db > -60.0 {
            let y = meter.bottom() - meter.height() * meter_frac(hold_db);
            p.line_segment([pos2(meter.left(), y), pos2(meter.right(), y)], Stroke::new(1.5_f32, if hold_db > 0.0 { REC } else { TEXT }));
        }
        let zero = meter.bottom() - meter.height() * meter_frac(0.0);
        p.line_segment([pos2(meter.left() - 1.0, zero), pos2(meter.right() + 1.0, zero)], Stroke::new(1.0_f32, PANEL));
        // Fader cap.
        let y = y_of(*value);
        let cap = Rect::from_center_size(pos2(track_x, y), vec2(24.0, 30.0));
        let hot = resp.dragged() || resp.hovered();
        p.rect_filled(cap.translate(vec2(1.0, 2.0)), 3.0, Color32::from_black_alpha(90));
        p.rect_filled(cap, 3.0, if hot { Color32::from_rgb(0x8a, 0x8a, 0x8a) } else { Color32::from_rgb(0x6e, 0x6e, 0x6e) });
        p.rect_filled(Rect::from_min_max(cap.left_top(), pos2(cap.right(), cap.top() + 10.0)), 3.0, Color32::from_rgb(0x82, 0x82, 0x82));
        p.line_segment([pos2(cap.left() + 3.0, y), pos2(cap.right() - 3.0, y)], Stroke::new(2.0_f32, if resp.dragged() { ACCENT } else { Color32::WHITE }));
        resp.on_hover_text(format!("{} dB", fmt_db(gain_to_db(fader_to_gain(*value)))))
    }
}
