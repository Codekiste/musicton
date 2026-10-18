//! Chord pads, note-rule editor and the "follow chords" switch.

use super::theme::*;
use super::widgets;
use super::{App, ClipRef};
use crate::model::*;
use egui::{vec2, Color32, RichText, Sense};

#[derive(Default)]
pub struct ChordPadState {
    pub open: bool,
    pub sevenths: bool,
    /// Octave offset of the voicing (0 = around C3).
    pub octave: i32,
    pub inversion: usize,
    held: Option<(usize, Vec<u8>)>,
}

impl ChordPadState {
    /// The chord on degree `d` of the project key, as (root, quality, MIDI notes).
    pub fn chord(&self, p: &Project, d: usize) -> (u8, usize, Vec<u8>) {
        let scale = if SCALES[p.scale.min(SCALES.len() - 1)].1.len() == 7 { p.scale } else { 2 };
        let (root, q) = diatonic_chord(p.key, scale, d % 7, self.sevenths);
        let base = (48 + root as i32 + 12 * (self.octave + (d / 7) as i32)).clamp(12, 108) as u8;
        let mut notes = ChordEvent { id: 0, start: 0, len: 0, root, quality: q }.voicing(base);
        for _ in 0..self.inversion.min(notes.len().saturating_sub(1)) {
            let low = notes.remove(0);
            notes.push(low + 12);
        }
        (root, q, notes)
    }
}

const DEGREES: [&str; 8] = ["I", "II", "III", "IV", "V", "VI", "VII", "I'"];

impl App {
    pub(super) fn chord_pads_window(&mut self, ctx: &egui::Context) {
        if !self.chord_pads.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Chord Pads").open(&mut open).resizable(false).default_pos([330.0, 150.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Key {} {}", NOTE_NAMES[self.project.key as usize % 12], SCALES[self.project.scale.min(SCALES.len() - 1)].0)).color(TEXT_DIM));
                let r = widgets::toggle(ui, "7ths", self.chord_pads.sevenths, ACCENT, vec2(38.0, 18.0));
                if r.clicked() {
                    self.chord_pads.sevenths = !self.chord_pads.sevenths;
                }
                ui.add(egui::DragValue::new(&mut self.chord_pads.octave).range(-2..=2).prefix("Oct "));
                ui.add(egui::DragValue::new(&mut self.chord_pads.inversion).range(0..=3).prefix("Inv "));
            });
            ui.horizontal(|ui| {
                for d in 0..8 {
                    let (root, q, notes) = self.chord_pads.chord(&self.project, d);
                    let name = ChordEvent { id: 0, start: 0, len: 0, root, quality: q }.name();
                    let held = self.chord_pads.held.as_ref().is_some_and(|h| h.0 == d);
                    let (rect, resp) = ui.allocate_exact_size(vec2(58.0, 58.0), Sense::click_and_drag());
                    let p = ui.painter();
                    p.rect_filled(rect, 4.0, if held { ACCENT } else if resp.hovered() { PANEL_LIGHT } else { PANEL_DARK });
                    p.text(rect.center_top() + vec2(0.0, 6.0), egui::Align2::CENTER_TOP, DEGREES[d], egui::FontId::proportional(10.0), if held { Color32::BLACK } else { TEXT_DIM });
                    p.text(rect.center() + vec2(0.0, 6.0), egui::Align2::CENTER_CENTER, &name, egui::FontId::proportional(14.0), if held { Color32::BLACK } else { TEXT });
                    let down = resp.is_pointer_button_down_on();
                    if down && !held {
                        self.release_chord_pad();
                        for &n in &notes {
                            self.live_note_on(None, n, 100, false);
                        }
                        self.chord_pads.held = Some((d, notes.clone()));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Add to chord track at playhead").clicked() {
                            let pos = (self.status.arr_pos.max(0.0) as u32 / 16) * 16;
                            self.add_chord_event(pos, 16, root, q);
                            ui.close_menu();
                        }
                    });
                    self.hint(&resp, &name, "Hold to play the chord on the selected track (records while Record is on). Right-click to put it on the chord track.");
                }
            });
            if !ui.input(|i| i.pointer.primary_down()) {
                self.release_chord_pad();
            }
            ui.label(RichText::new("Number keys 1-8 also work while this window is focused.").size(9.5).color(TEXT_DIM));
            for (k, key) in [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7, egui::Key::Num8].iter().enumerate() {
                if ui.input(|i| i.key_pressed(*key)) && ui.ui_contains_pointer() {
                    let (_, _, notes) = self.chord_pads.chord(&self.project, k);
                    self.release_chord_pad();
                    for &n in &notes {
                        self.live_note_on(None, n, 100, false);
                    }
                    self.chord_pads.held = Some((k, notes));
                }
            }
        });
        if !open {
            self.release_chord_pad();
            self.chord_pads.open = false;
        }
    }

    fn release_chord_pad(&mut self) {
        if let Some((_, notes)) = self.chord_pads.held.take() {
            for n in notes {
                self.live_note_off(None, n, false);
            }
        }
    }

    pub(super) fn add_chord_event(&mut self, start: u32, len: u32, root: u8, quality: usize) -> u64 {
        let id = self.project.new_id();
        self.project.chords.retain(|c| c.start + c.len <= start || c.start >= start + len);
        self.project.chords.push(ChordEvent { id, start, len, root, quality });
        self.project.chords.sort_by_key(|c| c.start);
        self.touch();
        id
    }

    pub(super) fn rules_window(&mut self, ctx: &egui::Context) {
        if !self.roll.rules_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Note Rules").open(&mut open).default_pos([620.0, 330.0]).default_width(560.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("Presets", |ui| {
                    for (name, rules) in rule_presets() {
                        if ui.button(name).clicked() {
                            self.roll.rules = rules;
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("+ Rule").clicked() {
                    self.roll.rules.push(NoteRule::new(RuleField::All, RuleOp::Equal, 0, 0, RuleAction::AddVelocity, 10));
                }
            });
            let mut remove = None;
            egui::Grid::new("rules").spacing(vec2(4.0, 3.0)).show(ui, |ui| {
                ui.label(RichText::new("If").color(TEXT_DIM));
                ui.label("");
                ui.label("");
                ui.label("");
                ui.label(RichText::new("then").color(TEXT_DIM));
                ui.end_row();
                for (i, r) in self.roll.rules.iter_mut().enumerate() {
                    egui::ComboBox::from_id_salt(("rf", i)).width(90.0).selected_text(r.field.name()).show_ui(ui, |ui| {
                        for f in RuleField::ALL {
                            ui.selectable_value(&mut r.field, f, f.name());
                        }
                    });
                    if r.field == RuleField::All {
                        ui.label("");
                        ui.label("");
                    } else {
                        egui::ComboBox::from_id_salt(("ro", i)).width(60.0).selected_text(r.op.name()).show_ui(ui, |ui| {
                            for o in RuleOp::ALL {
                                ui.selectable_value(&mut r.op, o, o.name());
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut r.a).range(-127..=127));
                            if matches!(r.op, RuleOp::Between | RuleOp::EveryNth) {
                                ui.label(if r.op == RuleOp::Between { "and" } else { "from" });
                                ui.add(egui::DragValue::new(&mut r.b).range(-127..=127));
                            }
                        });
                    }
                    ui.label("");
                    egui::ComboBox::from_id_salt(("ra", i)).width(150.0).selected_text(r.action.name()).show_ui(ui, |ui| {
                        for a in RuleAction::ALL {
                            ui.selectable_value(&mut r.action, a, a.name());
                        }
                    });
                    if r.action != RuleAction::Delete {
                        ui.add(egui::DragValue::new(&mut r.value).range(-127..=200));
                    } else {
                        ui.label("");
                    }
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                self.roll.rules.remove(i);
            }
            ui.separator();
            let target = self.sel_clip;
            ui.horizontal(|ui| {
                let label = match target {
                    Some(ClipRef::Session { scene, .. }) => format!("Apply to clip (scene {})", scene + 1),
                    Some(ClipRef::Arr { .. }) => "Apply to timeline clip".into(),
                    None => "Select a clip first".into(),
                };
                if ui.add_enabled(target.is_some(), egui::Button::new(label)).clicked() {
                    let rules = self.roll.rules.clone();
                    let seed = (self.now() * 1000.0) as u32;
                    if let Some(c) = target.and_then(|r| self.clip_mut(r)) {
                        let n = apply_rules(&mut c.notes, &rules, seed);
                        self.touch();
                        self.toast(format!("Rules touched {n} notes (Ctrl+Z to undo)"));
                    }
                }
            });
        });
        if !open {
            self.roll.rules_open = false;
        }
    }

    /// "Follow chords" switch shown in the device chain title row.
    pub(super) fn follow_chords_toggle(&mut self, ui: &mut egui::Ui, track: u64) {
        let Some(t) = self.project.track(track) else { return };
        if !t.instrument.kind.is_melodic() {
            return;
        }
        let on = t.follow_chords;
        let r = widgets::toggle(ui, "Follow Chords", on, ACCENT, vec2(92.0, 18.0));
        if r.clicked() {
            if let Some(t) = self.project.track_mut(track) {
                t.follow_chords = !on;
            }
            self.touch();
        }
        self.hint(&r, "Follow Chords", "Bends this track's clip notes to the nearest tone of the chord track's current chord - one bassline or arp follows every chord change.");
    }
}
