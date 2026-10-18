//! Custom panels for the B-303 and Nexa instruments, and the 303 pattern editor.

use super::theme::*;
use super::widgets;
use super::{App, ClipRef};
use crate::audio::b303::{notes_to_steps, random_pattern, steps_to_notes, Step, StepTime};
use crate::audio::nexa::{self, CATEGORIES, GATE_PATTERNS, PRESETS};
use crate::model::*;
use crate::params::{self, ParamDef};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke};

const SILVER: Color32 = Color32::from_rgb(0xc4, 0xc6, 0xc8);
const SILVER_DARK: Color32 = Color32::from_rgb(0x9a, 0x9c, 0x9f);
const INK: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1e);
const ORANGE_303: Color32 = Color32::from_rgb(0xe8, 0x6a, 0x1a);
const LCD_BG: Color32 = Color32::from_rgb(0x10, 0x1c, 0x14);
const LCD_FG: Color32 = Color32::from_rgb(0x5c, 0xff, 0x9c);

impl App {
    // -----------------------------------------------------------------------------------------

    /// 303-style step editor: pitch, up/down, accent, slide and note/tie/rest per 16th.
    pub(super) fn b303_pattern_editor(&mut self, ui: &mut egui::Ui, r: ClipRef) {
        let Some(clip) = self.clip(r).cloned() else { return };
        let root = 36 + self.project.key % 12;
        let mut steps = notes_to_steps(&clip.notes, clip.len, root);
        let mut changed = false;
        let mut new_len = clip.len;
        ui.horizontal(|ui| {
            let rr = widgets::button(ui, "Random Acid", false);
            if rr.clicked() {
                let seed = (self.now() * 1000.0) as u32 ^ (clip.len * 7919);
                let scale = SCALES[self.project.scale.min(SCALES.len() - 1)].1;
                steps = random_pattern(steps.len(), root, scale, seed);
                changed = true;
            }
            self.hint(&rr, "Random Acid", "Generates a new acid line in the project key and scale (with accents, slides, ties and rests).");
            if widgets::button(ui, "Clear", false).clicked() {
                for s in &mut steps {
                    s.time = StepTime::Rest;
                    s.accent = false;
                    s.slide = false;
                }
                changed = true;
            }
            if widgets::button(ui, "◀", false).on_hover_text("Shift pattern left").clicked() {
                steps.rotate_left(1);
                changed = true;
            }
            if widgets::button(ui, "▶", false).on_hover_text("Shift pattern right").clicked() {
                steps.rotate_right(1);
                changed = true;
            }
            for (label, d) in [("-12", -12i32), ("-1", -1), ("+1", 1), ("+12", 12)] {
                if widgets::button(ui, label, false).on_hover_text("Transpose").clicked() {
                    for s in &mut steps {
                        s.pitch = (s.pitch as i32 + d).clamp(12, 96) as u8;
                    }
                    changed = true;
                }
            }
            ui.separator();
            ui.label("Steps");
            let mut n = clip.len as i32;
            if ui.add(egui::DragValue::new(&mut n).range(1..=64)).changed() {
                new_len = n as u32;
            }
            if widgets::button(ui, "Piano Roll", false).on_hover_text("Edit this clip in the piano roll instead").clicked() {
                self.b303_piano_roll = true;
            }
        });
        ui.add_space(4.0);
        let n = steps.len().max(1);
        let avail = ui.available_width();
        let cw = ((avail - 70.0) / n as f32).clamp(22.0, 54.0);
        let playhead = self.playhead_step(r).map(|p| p.floor() as usize);
        let label_w = 64.0;
        let rows = [("STEP", 16.0), ("NOTE", 54.0), ("OCT", 34.0), ("ACCENT", 20.0), ("SLIDE", 20.0), ("TIME", 22.0)];
        egui::ScrollArea::horizontal().drag_to_scroll(false).id_salt("b303_steps").show(ui, |ui| {
            let total_h: f32 = rows.iter().map(|r| r.1 + 2.0).sum();
            let (rect, _) = ui.allocate_exact_size(vec2(label_w + cw * n as f32, total_h), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(rect, 3.0, SILVER);
            let mut y = rect.top();
            for (ri, (name, h)) in rows.iter().enumerate() {
                p.text(pos2(rect.left() + 6.0, y + h / 2.0), Align2::LEFT_CENTER, *name, FontId::proportional(9.5), INK);
                for (i, st) in steps.iter_mut().enumerate() {
                    let cell = Rect::from_min_size(pos2(rect.left() + label_w + i as f32 * cw, y), vec2(cw - 2.0, *h));
                    let resp = ui.interact(cell, ui.id().with(("303", ri, i)), Sense::click_and_drag());
                    let beat_dark = (i / 4) % 2 == 1;
                    let base = if beat_dark { Color32::from_rgb(0x2a, 0x2a, 0x2d) } else { Color32::from_rgb(0x36, 0x36, 0x3a) };
                    let is_play = playhead == Some(i);
                    match ri {
                        0 => {
                            p.rect_filled(cell, 1.0, if is_play { LCD_FG } else { SILVER_DARK });
                            p.text(cell.center(), Align2::CENTER_CENTER, (i + 1).to_string(), FontId::proportional(9.0), INK);
                        }
                        1 => {
                            p.rect_filled(cell, 2.0, base);
                            let active = st.time != StepTime::Rest;
                            let col = if active { Color32::WHITE } else { Color32::from_rgb(0x77, 0x77, 0x77) };
                            p.text(cell.center() - vec2(0.0, 8.0), Align2::CENTER_CENTER, NOTE_NAMES[(st.pitch % 12) as usize], FontId::proportional(13.0), col);
                            p.text(cell.center() + vec2(0.0, 12.0), Align2::CENTER_CENTER, format!("{}", st.pitch as i32 / 12 - 2), FontId::proportional(9.0), col);
                            if resp.dragged() {
                                let dy = ui.ctx().input(|i| i.pointer.delta().y);
                                let acc = ui.ctx().data_mut(|d| {
                                    let a: &mut f32 = d.get_temp_mut_or_default(egui::Id::new(("303drag", i)));
                                    *a -= dy;
                                    let steps_ = (*a / 8.0) as i32;
                                    *a -= steps_ as f32 * 8.0;
                                    steps_
                                });
                                if acc != 0 {
                                    st.pitch = (st.pitch as i32 + acc).clamp(12, 96) as u8;
                                    if st.time == StepTime::Rest {
                                        st.time = StepTime::Note;
                                    }
                                    changed = true;
                                }
                            }
                            if resp.hovered() {
                                let sc = ui.input(|i| i.raw_scroll_delta.y);
                                if sc != 0.0 {
                                    st.pitch = (st.pitch as i32 + sc.signum() as i32).clamp(12, 96) as u8;
                                    changed = true;
                                }
                            }
                            if resp.clicked() {
                                let pos = resp.interact_pointer_pos().unwrap_or(cell.center());
                                st.pitch = (st.pitch as i32 + if pos.y < cell.center().y { 1 } else { -1 }).clamp(12, 96) as u8;
                                if st.time == StepTime::Rest {
                                    st.time = StepTime::Note;
                                }
                                changed = true;
                            }
                        }
                        2 => {
                            let half = Rect::from_min_size(cell.min, vec2(cell.width(), cell.height() / 2.0 - 1.0));
                            let half2 = Rect::from_min_size(pos2(cell.left(), cell.center().y + 1.0), vec2(cell.width(), cell.height() / 2.0 - 1.0));
                            let up = st.pitch >= root + 12;
                            let down = st.pitch < root;
                            p.rect_filled(half, 1.0, if up { ORANGE_303 } else { base });
                            p.rect_filled(half2, 1.0, if down { ORANGE_303 } else { base });
                            p.text(half.center(), Align2::CENTER_CENTER, "UP", FontId::proportional(8.5), if up { INK } else { TEXT_DIM });
                            p.text(half2.center(), Align2::CENTER_CENTER, "DN", FontId::proportional(8.5), if down { INK } else { TEXT_DIM });
                            if resp.clicked() {
                                let pos = resp.interact_pointer_pos().unwrap_or(cell.center());
                                let d = if pos.y < cell.center().y { 12 } else { -12 };
                                st.pitch = (st.pitch as i32 + d).clamp(12, 96) as u8;
                                changed = true;
                            }
                        }
                        3 | 4 => {
                            let on = if ri == 3 { st.accent } else { st.slide };
                            let col = if ri == 3 { ORANGE_303 } else { BLUE };
                            p.rect_filled(cell, 2.0, if on { col } else { base });
                            if resp.clicked() {
                                if ri == 3 {
                                    st.accent = !st.accent;
                                } else {
                                    st.slide = !st.slide;
                                }
                                if st.time == StepTime::Rest {
                                    st.time = StepTime::Note;
                                }
                                changed = true;
                            }
                        }
                        _ => {
                            let (txt, col) = match st.time {
                                StepTime::Note => ("●", Color32::WHITE),
                                StepTime::Tie => ("━", LCD_FG),
                                StepTime::Rest => ("·", Color32::from_rgb(0x77, 0x77, 0x77)),
                            };
                            p.rect_filled(cell, 2.0, base);
                            if txt == "●" {
                                p.circle_filled(cell.center(), 4.0, col);
                            } else {
                                p.text(cell.center(), Align2::CENTER_CENTER, txt, FontId::proportional(13.0), col);
                            }
                            if resp.clicked() {
                                st.time = match st.time {
                                    StepTime::Note => {
                                        if i > 0 {
                                            StepTime::Tie
                                        } else {
                                            StepTime::Rest
                                        }
                                    }
                                    StepTime::Tie => StepTime::Rest,
                                    StepTime::Rest => StepTime::Note,
                                };
                                changed = true;
                            }
                        }
                    }
                    if resp.hovered() {
                        let info = match ri {
                            1 => "Pitch of the step: click the upper/lower half for +/-1 semitone, drag up/down or use the mouse wheel.",
                            2 => "Octave: UP raises the step by an octave, DN lowers it.",
                            3 => "Accent: louder and brighter, with the typical accent sweep.",
                            4 => "Slide: glide from this step into the next one without retriggering.",
                            5 => "Time: ● note, ━ tie (holds the previous note), · rest. Click to cycle.",
                            _ => "Step number. The green step is playing.",
                        };
                        self.info = Some(("303 Pattern".into(), info.into()));
                    }
                }
                y += h + 2.0;
            }
        });
        if changed || new_len != clip.len {
            if new_len != clip.len {
                steps.resize(new_len as usize, Step { pitch: root, accent: false, slide: false, time: StepTime::Rest });
            }
            let notes = steps_to_notes(&steps);
            if let Some(c) = self.clip_mut(r) {
                c.notes = notes;
                c.len = new_len;
            }
            self.touch();
        }
    }

    // -----------------------------------------------------------------------------------------

    pub(super) fn nexa_panel(&mut self, ui: &mut egui::Ui, track: u64, inst: &mut Instrument) {
        use params::nexa as nx;
        let defs = InstrumentKind::Nexa.defs();
        let cur = (inst.params[nx::PRESET].round() as usize).min(PRESETS.len() - 1);
        let cat_id = egui::Id::new(("nexa_cat", track));
        let mut cat: String = ui.ctx().data(|d| d.get_temp(cat_id)).unwrap_or_else(|| PRESETS[cur].name[..2].to_string());
        let mut select: Option<usize> = None;
        egui::Frame::default().fill(Color32::from_rgb(0x1f, 0x22, 0x28)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x3a, 0x40, 0x4c))).corner_radius(4).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // Preset browser
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("NEXA").strong().size(16.0).color(Color32::from_rgb(0x6c, 0xc8, 0xff)));
                        ui.label(RichText::new("ROMpler").size(10.0).color(TEXT_DIM));
                    });
                    let (lcd, lresp) = ui.allocate_exact_size(vec2(250.0, 34.0), Sense::click());
                    ui.painter().rect_filled(lcd, 3.0, LCD_BG);
                    ui.painter().text(lcd.left_center() + vec2(8.0, -6.0), Align2::LEFT_CENTER, PRESETS[cur].name, FontId::monospace(13.0), LCD_FG);
                    ui.painter().text(lcd.left_center() + vec2(8.0, 9.0), Align2::LEFT_CENTER, format!("{} / {}", cur + 1, PRESETS.len()), FontId::monospace(9.0), Color32::from_rgb(0x3c, 0xa8, 0x6c));
                    self.hint(&lresp, "Preset Display", "The loaded sound. Use the arrows or the lists below to browse.");
                    ui.horizontal(|ui| {
                        if widgets::button(ui, "◀ Prev", false).clicked() {
                            select = Some((cur + PRESETS.len() - 1) % PRESETS.len());
                        }
                        if widgets::button(ui, "Next ▶", false).clicked() {
                            select = Some((cur + 1) % PRESETS.len());
                        }
                    });
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(80.0);
                            for (code, name) in CATEGORIES {
                                if ui.selectable_label(cat == *code, *name).clicked() {
                                    cat = code.to_string();
                                }
                            }
                        });
                        egui::ScrollArea::vertical().drag_to_scroll(false).id_salt(("nexa_list", track)).max_height(150.0).show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(160.0);
                                for (i, p) in PRESETS.iter().enumerate().filter(|(_, p)| p.name.starts_with(cat.as_str())) {
                                    let r = ui.selectable_label(i == cur, &p.name[3..]);
                                    if r.clicked() {
                                        select = Some(i);
                                    }
                                }
                            });
                        });
                    });
                });
                ui.separator();
                // Macros
                ui.vertical(|ui| {
                    ui.label(RichText::new("MACROS").size(10.0).color(TEXT_DIM));
                    ui.horizontal(|ui| {
                        for idx in [nx::CUTOFF, nx::RES] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 40.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                            r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                        }
                    });
                    ui.horizontal(|ui| {
                        for idx in [nx::ATTACK, nx::RELEASE] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 40.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                            r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                        }
                    });
                    ui.horizontal(|ui| {
                        for idx in [nx::VOLUME, nx::MODWHEEL] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 32.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                            r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                        }
                    });
                });
                ui.separator();
                // Arp + gate
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let on = inst.params[nx::ARP_ON] > 0.5;
                        let r = widgets::toggle(ui, "ARP", on, ACCENT, vec2(42.0, 18.0));
                        if r.clicked() {
                            inst.params[nx::ARP_ON] = if on { 0.0 } else { 1.0 };
                        }
                        self.hint(&r, "Arpeggiator", "Plays held notes one by one in time with the song.");
                        for idx in [nx::ARP_MODE, nx::ARP_RATE] {
                            widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 60.0);
                        }
                    });
                    ui.horizontal(|ui| {
                        for idx in [nx::ARP_OCT, nx::ARP_GATE] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 30.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let on = inst.params[nx::GATE_ON] > 0.5;
                        let r = widgets::toggle(ui, "GATE", on, ACCENT, vec2(42.0, 18.0));
                        if r.clicked() {
                            inst.params[nx::GATE_ON] = if on { 0.0 } else { 1.0 };
                        }
                        self.hint(&r, "Trancegate", "Chops the sound rhythmically with a 16-step pattern, synced to the song.");
                        for idx in [nx::GATE_PATTERN, nx::GATE_RATE] {
                            widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 70.0);
                        }
                    });
                    // Pattern display
                    let pat = GATE_PATTERNS[(inst.params[nx::GATE_PATTERN].round() as usize).min(GATE_PATTERNS.len() - 1)].1.as_bytes();
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0 * 11.0, 12.0), Sense::hover());
                    for (k, c) in pat.iter().enumerate() {
                        let cell = Rect::from_min_size(pos2(rect.left() + k as f32 * 11.0, rect.top()), vec2(9.0, 12.0));
                        ui.painter().rect_filled(cell, 1.0, if *c == b'x' { if inst.params[nx::GATE_ON] > 0.5 { ACCENT } else { TEXT_DIM } } else { KNOB_BG });
                    }
                    ui.horizontal(|ui| {
                        for idx in [nx::GATE_DEPTH, nx::GATE_SMOOTH] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 30.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                        }
                    });
                });
                ui.separator();
                // FX
                ui.vertical(|ui| {
                    ui.label(RichText::new("EFFECTS").size(10.0).color(TEXT_DIM));
                    ui.horizontal(|ui| {
                        for idx in [nx::DIST, nx::CHORUS, nx::DELAY, nx::REVERB] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 30.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                            r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                        }
                    });
                    ui.horizontal(|ui| {
                        widgets::param(ui, ("nx", track, nx::DELAY_TIME), &mut inst.params[nx::DELAY_TIME], &defs[nx::DELAY_TIME], 60.0);
                        for idx in [nx::DELAY_FB, nx::REVERB_SIZE] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 30.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                        }
                    });
                    ui.horizontal(|ui| {
                        for idx in [nx::LOW, nx::HIGH] {
                            let r = widgets::param(ui, ("nx", track, idx), &mut inst.params[idx], &defs[idx], 30.0);
                            self.nexa_hint(&r, idx, &defs[idx]);
                        }
                    });
                });
            });
        });
        ui.ctx().data_mut(|d| d.insert_temp(cat_id, cat));
        if let Some(i) = select {
            inst.params[nx::PRESET] = i as f32;
            for (k, v) in nexa::preset_defaults(i) {
                inst.params[k] = v;
            }
            for k in [nx::CUTOFF, nx::RES, nx::ATTACK, nx::RELEASE] {
                inst.params[k] = 0.0;
            }
            self.nexa_presets.insert(track, i);
            // Audition the new sound.
            self.send(crate::audio::engine::Cmd::NoteOn { track, pitch: 60, vel: 100 });
            self.remote_note_offs.push((std::time::Instant::now() + std::time::Duration::from_millis(500), track, 60));
        }
    }

    fn nexa_hint(&mut self, r: &egui::Response, idx: usize, def: &ParamDef) {
        use params::nexa as nx;
        let text = match idx {
            nx::CUTOFF => "Macro: moves the filter cutoff of every layer up or down (relative to the preset).",
            nx::RES => "Macro: adds or removes filter resonance.",
            nx::ATTACK => "Macro: slower or faster attack for all layers.",
            nx::RELEASE => "Macro: longer or shorter release tails.",
            nx::MODWHEEL => "Mod wheel: vibrato and a brighter filter.",
            nx::GATE_DEPTH => "How deep the trancegate cuts.",
            nx::GATE_SMOOTH => "Softens the edges of the trancegate steps.",
            nx::ARP_OCT => "Octave range of the arpeggio.",
            nx::ARP_GATE => "Length of each arpeggio note.",
            _ => "Nexa parameter.",
        };
        if r.hovered() || r.dragged() {
            self.info = Some((def.name.to_string(), text.to_string()));
        }
    }

    /// Applies preset defaults when a Nexa preset was changed elsewhere (MIDI mapping, remote).
    pub(super) fn sync_nexa_presets(&mut self) {
        let mut apply = vec![];
        for t in &self.project.tracks {
            if t.instrument.kind != InstrumentKind::Nexa {
                continue;
            }
            let idx = (t.instrument.params[params::nexa::PRESET].round().max(0.0) as usize).min(PRESETS.len() - 1);
            match self.nexa_presets.get(&t.id) {
                Some(old) if *old != idx => apply.push((t.id, idx)),
                None => {
                    self.nexa_presets.insert(t.id, idx);
                }
                _ => {}
            }
        }
        for (id, idx) in apply {
            if let Some(t) = self.project.track_mut(id) {
                for (k, v) in nexa::preset_defaults(idx) {
                    t.instrument.params[k] = v;
                }
            }
            self.nexa_presets.insert(id, idx);
            self.changed = true;
        }
    }
}
