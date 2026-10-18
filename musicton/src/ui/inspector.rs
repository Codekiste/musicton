//! Inspector: the panel on the left with settings of the selected clip and track,
//! two channel strips (selected channel and master) and the quick help.

use super::theme::*;
use super::{App, ClipRef, Sel};
use crate::model::*;
use egui::{pos2, vec2, Align2, FontId, Rect, Sense};

pub const INSPECTOR_W: f32 = 236.0;

impl App {
    pub(super) fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        let full = ui.available_rect_before_wrap();
        let help_h = if self.show_info { 150.0 } else { 0.0 };
        egui::ScrollArea::vertical().id_salt("inspector").max_height(full.height() - help_h).auto_shrink([false, false]).show(ui, |ui| {
            self.inspector_clip(ui);
            self.inspector_track(ui);
            // Channel strips: the selected channel and the master.
            let strip_h = 520.0;
            let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), strip_h), Sense::hover());
            let w = (area.width() - 4.0) / 2.0;
            let sel = self.sel;
            let number = match sel {
                Sel::Track(id) => self.project.track_index(id).map(|i| i + 1).unwrap_or(0),
                _ => 0,
            };
            let devs = |c: Option<&Channel>| c.map(|c| c.devices.len()).unwrap_or(0);
            let sel_devs = match sel {
                Sel::Track(id) => devs(self.project.track(id).map(|t| &t.channel)),
                Sel::Return(k) => devs(self.project.returns.get(k).map(|r| &r.channel)),
                Sel::Master => 0,
            };
            let slots = (sel_devs.max(self.project.master.devices.len()) + 1).clamp(2, 6);
            // Keep the strips compact: no EQ thumbnail or long send lists in the narrow panel.
            let saved = (self.mixer.show_eq, self.mixer.show_sends);
            self.mixer.show_eq = false;
            self.mixer.show_sends = self.project.returns.len() <= 4;
            if sel != Sel::Master {
                self.channel_strip(ui, Rect::from_min_size(area.min, vec2(w, area.height())), sel, number, slots);
            }
            self.channel_strip(ui, Rect::from_min_size(pos2(area.left() + w + 4.0, area.top()), vec2(w, area.height())), Sel::Master, 0, slots);
            (self.mixer.show_eq, self.mixer.show_sends) = saved;
        });
        if self.show_info {
            let r = Rect::from_min_max(pos2(full.left(), full.bottom() - help_h + 4.0), full.max);
            ui.painter().rect_filled(r, 6.0, PANEL_DARK);
            ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink(8.0)), |ui| {
                ui.label(egui::RichText::new("Quick Help").size(10.0).color(TEXT_DIM));
                self.info_view(ui);
            });
        }
    }

    fn section(ui: &mut egui::Ui, title: &str) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        gradient(ui.painter(), r, PANEL_LIGHT, PANEL);
        ui.painter().text(r.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(11.5), TEXT);
    }

    /// Settings of the selected clip/region.
    fn inspector_clip(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.sel_clip else { return };
        let Some(mut c) = self.clip(r).cloned() else { return };
        let before = c.clone();
        Self::section(ui, &format!("Region: {}", c.name));
        let mut arr_len = match r {
            ClipRef::Arr { track, id } => self.project.track(track).and_then(|t| t.arrangement.iter().find(|a| a.id == id)).map(|a| a.len),
            _ => None,
        };
        let arr_before = arr_len;
        egui::Grid::new("insp_clip").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            ui.label(egui::RichText::new("Name").color(TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(140.0));
            ui.end_row();
            ui.label(egui::RichText::new("Mute").color(TEXT_DIM));
            ui.checkbox(&mut c.muted, "");
            ui.end_row();
            ui.label(egui::RichText::new("Gain").color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut c.gain).range(-36.0..=12.0).speed(0.1).max_decimals(1).suffix(" dB"));
            ui.end_row();
            let mut bars = c.len as f32 / STEPS_PER_BAR as f32;
            ui.label(egui::RichText::new("Loop").color(TEXT_DIM));
            if ui.add(egui::DragValue::new(&mut bars).range(0.25..=64.0).speed(0.05).max_decimals(2).suffix(" bars")).changed() {
                c.len = ((bars * STEPS_PER_BAR as f32).round() as u32).max(1);
            }
            ui.end_row();
            if let Some(len) = arr_len.as_mut() {
                let mut b = *len as f32 / STEPS_PER_BAR as f32;
                ui.label(egui::RichText::new("Length").color(TEXT_DIM));
                if ui.add(egui::DragValue::new(&mut b).range(0.25..=512.0).speed(0.05).max_decimals(2).suffix(" bars")).changed() {
                    *len = ((b * STEPS_PER_BAR as f32).round() as u32).max(1);
                }
                ui.end_row();
            }
            if let Some(a) = c.audio.as_mut() {
                ui.label(egui::RichText::new("Transpose").color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut a.transpose).range(-24.0..=24.0).speed(0.1).max_decimals(0).suffix(" st"));
                ui.end_row();
                ui.label(egui::RichText::new("Warp").color(TEXT_DIM));
                egui::ComboBox::from_id_salt("insp_warp").width(100.0).selected_text(format!("{:?}", a.warp)).show_ui(ui, |ui| {
                    for w in [WarpMode::Off, WarpMode::Repitch, WarpMode::Beats] {
                        ui.selectable_value(&mut a.warp, w, format!("{w:?}"));
                    }
                });
                ui.end_row();
            } else {
                ui.label(egui::RichText::new("Notes").color(TEXT_DIM));
                ui.label(c.notes.len().to_string());
                ui.end_row();
            }
        });
        let changed = c.name != before.name || c.muted != before.muted || c.gain != before.gain || c.len != before.len || c.audio.as_ref().map(|a| (a.transpose, a.warp)) != before.audio.as_ref().map(|a| (a.transpose, a.warp));
        if (changed || arr_len != arr_before) && !self.guard_locked(r.track()) {
            if let Some(dst) = self.clip_mut(r) {
                *dst = c;
            }
            if let (ClipRef::Arr { track, id }, Some(len)) = (r, arr_len) {
                if let Some(a) = self.project.track_mut(track).and_then(|t| t.arrangement.iter_mut().find(|a| a.id == id)) {
                    a.len = len;
                }
            }
            self.touch();
        }
        ui.add_space(4.0);
    }

    /// Settings of the selected track.
    fn inspector_track(&mut self, ui: &mut egui::Ui) {
        let Sel::Track(id) = self.sel else {
            let name = match self.sel {
                Sel::Return(k) => self.project.returns.get(k).map(|r| r.name.clone()).unwrap_or_default(),
                _ => "Master".into(),
            };
            Self::section(ui, &name);
            return;
        };
        let Some(t) = self.project.track(id).cloned() else { return };
        Self::section(ui, &format!("Track: {}{}", if t.icon.is_empty() { String::new() } else { format!("{} ", t.icon) }, t.name));
        egui::Grid::new("insp_track").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            ui.label(egui::RichText::new("Instrument").color(TEXT_DIM));
            ui.label(t.instrument.kind.name());
            ui.end_row();
            ui.label(egui::RichText::new("Clips").color(TEXT_DIM));
            ui.label(format!("{} in Launcher, {} on timeline", t.clips.iter().flatten().count(), t.arrangement.len()));
            ui.end_row();
            if !t.info.is_empty() {
                ui.label(egui::RichText::new("Info").color(TEXT_DIM));
                ui.label(egui::RichText::new(&t.info).size(11.0));
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            let mut locked = t.locked;
            if ui.checkbox(&mut locked, "Protect").changed() {
                self.set_track_locked(id, locked);
            }
            if t.instrument.kind != InstrumentKind::Group {
                let label = if t.frozen.is_some() { "Unfreeze" } else { "Freeze" };
                if ui.button(label).clicked() {
                    let r = if t.frozen.is_some() { self.unfreeze_track(id) } else { self.freeze_track(id) };
                    match r {
                        Ok(m) => self.toast(m),
                        Err(e) => self.toast(format!("Not possible: {e}")),
                    }
                }
            }
        });
        ui.add_space(4.0);
    }
}
