//! Clip Editor: clip properties, drum step sequencer and piano roll.

use super::theme::*;
use super::widgets;
use super::{App, ClipRef, Payload, RenameTarget};
use crate::audio::engine::Cmd;
use crate::model::*;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

#[derive(Clone, Copy, Debug)]
enum NoteDrag {
    Move { idx: usize, grab_step: f32, grab_pitch: i32 },
    Resize { idx: usize },
    Velocity,
}

pub struct RollState {
    pub top: f32,
    drag: Option<NoteDrag>,
    pub selected: Option<usize>,
    pub draw_len: u32,
    pub scroll_to_notes: bool,
    paint: Option<bool>,
    audition: Option<(u64, u8)>,
    clip: Option<ClipRef>,
    /// Chord stamp: clicking draws this chord quality instead of one note.
    pub stamp: Option<usize>,
    /// Show the notes of the other tracks faintly behind this clip.
    pub ghosts: bool,
    pub rules_open: bool,
    pub rules: Vec<NoteRule>,
    /// Show the clip as notation instead of the piano roll.
    pub score: bool,
}

impl Default for RollState {
    fn default() -> Self {
        Self {
            top: 72.0,
            drag: None,
            selected: None,
            draw_len: 1,
            scroll_to_notes: true,
            paint: None,
            audition: None,
            clip: None,
            stamp: None,
            ghosts: false,
            rules_open: false,
            rules: vec![NoteRule::new(RuleField::All, RuleOp::Equal, 0, 0, RuleAction::RandomVelocity, 10)],
            score: false,
        }
    }
}

const ROW_H: f32 = 11.0;
const KEYS_W: f32 = 48.0;
const PROPS_W: f32 = 176.0;

impl App {
    pub(super) fn delete_selected_note(&mut self) -> bool {
        let (Some(r), Some(i)) = (self.sel_clip, self.roll.selected) else { return false };
        let Some(c) = self.clip_mut(r) else { return false };
        if i < c.notes.len() {
            c.notes.remove(i);
            self.roll.selected = None;
            self.touch();
            return true;
        }
        false
    }

    pub(super) fn clip_detail(&mut self, ui: &mut egui::Ui) {
        if self.roll.clip != self.sel_clip {
            self.roll.clip = self.sel_clip;
            self.roll.selected = None;
            self.roll.drag = None;
        }
        let Some(r) = self.sel_clip.filter(|r| self.clip(*r).is_some()) else {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("Select or double-click a clip slot to edit a clip.\nDrop a clip from the browser to get started.").color(TEXT_DIM));
            });
            return;
        };
        let Some(track) = self.project.track(r.track()) else { return };
        let kind = track.instrument.kind;
        let track_id = track.id;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(PROPS_W);
                self.clip_props(ui, r, kind);
            });
            ui.separator();
            ui.vertical(|ui| {
                if self.clip(r).is_some_and(|c| c.audio.is_some()) {
                    self.audio_editor(ui, r);
                } else if kind == InstrumentKind::B303 && !self.b303_piano_roll {
                    self.b303_pattern_editor(ui, r);
                } else if kind.is_drum_like() {
                    self.step_sequencer(ui, r, track_id);
                } else if kind == InstrumentKind::Audio {
                    ui.label(egui::RichText::new("This clip has no audio. Drop a WAV onto the slot.").color(TEXT_DIM));
                } else {
                    if kind == InstrumentKind::B303 && widgets::button(ui, "303 Pattern", false).on_hover_text("Back to the 303 step editor").clicked() {
                        self.b303_piano_roll = false;
                    }
                    if self.roll.score {
                        self.score_view(ui, r);
                    } else {
                        self.piano_roll(ui, r, track_id, kind);
                    }
                }
            });
        });
    }

    fn clip_props(&mut self, ui: &mut egui::Ui, r: ClipRef, kind: InstrumentKind) {
        let Some(clip) = self.clip(r).cloned() else { return };
        let mut c = clip.clone();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Clip").strong());
            let where_ = match r {
                ClipRef::Session { scene, .. } => format!("Launcher, scene {}", scene + 1),
                ClipRef::Arr { .. } => "Timeline".into(),
            };
            ui.label(egui::RichText::new(where_).size(10.0).color(TEXT_DIM));
        });
        let resp = ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(PROPS_W - 8.0));
        self.hint(&resp, "Clip Name", "Name of the clip.");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            ui.set_max_width(PROPS_W);
            for col in CLIP_COLORS.iter().take(16) {
                if widgets::color_swatch(ui, color32(*col), c.color == *col).clicked() {
                    c.color = *col;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Length");
            let mut bars = c.len as f32 / STEPS_PER_BAR as f32;
            let resp = ui.add(egui::DragValue::new(&mut bars).range(0.25..=64.0).speed(0.05).max_decimals(2).suffix(" bars"));
            if resp.changed() {
                c.len = ((bars * 4.0).round() as u32).max(1) * 4;
            }
            self.hint(&resp, "Clip Length", "Loop length of the clip. Notes beyond the end are kept but not played.");
        });
        ui.horizontal(|ui| {
            ui.label("Gain");
            let resp = ui.add(egui::DragValue::new(&mut c.gain).range(-36.0..=12.0).speed(0.1).max_decimals(1).suffix(" dB"));
            if resp.double_clicked() {
                c.gain = 0.0;
            }
            self.hint(&resp, "Clip Gain", "Level of this clip before the track's devices - balance clips without touching the fader. Double-click for 0 dB.");
        });
        ui.horizontal(|ui| {
            let r1 = widgets::button(ui, "×2", false);
            if r1.clicked() {
                let len = c.len;
                let copies: Vec<Note> = c.notes.iter().filter(|n| n.step < len).map(|n| Note { step: n.step + len, ..*n }).collect();
                c.notes.extend(copies);
                c.len = len * 2;
            }
            self.hint(&r1, "Duplicate Loop", "Doubles the clip length and copies its notes into the new half.");
            let r2 = widgets::button(ui, "÷2", false);
            if r2.clicked() && c.len >= 8 {
                c.len /= 2;
            }
            self.hint(&r2, "Halve Length", "Halves the loop length.");
            let r3 = widgets::button(ui, "Clear", false);
            if r3.clicked() {
                c.notes.clear();
            }
            self.hint(&r3, "Clear", "Removes all notes from the clip.");
        });
        ui.horizontal(|ui| {
            let r4 = widgets::button(ui, "Reverse", false);
            if r4.clicked() {
                let len = c.len;
                for n in &mut c.notes {
                    n.step = (len as i32 - n.step as i32 - n.len as i32).max(0) as u32;
                }
            }
            self.hint(&r4, "Reverse", "Mirrors the notes in time.");
            let r5 = widgets::button(ui, "Humanize", false);
            if r5.clicked() {
                let mut seed = 0x1234_5678u32;
                for n in &mut c.notes {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let d = (seed % 25) as i32 - 12;
                    n.vel = (n.vel as i32 + d).clamp(1, 127) as u8;
                }
            }
            self.hint(&r5, "Humanize", "Randomizes note velocities slightly for a more natural groove.");
        });
        if let ClipRef::Session { .. } = r {
            ui.horizontal(|ui| {
                ui.label("Follow");
                let cur = c.follow.map(|f| f.action.name()).unwrap_or("None");
                let resp = egui::ComboBox::from_id_salt("follow").width(76.0).selected_text(cur).show_ui(ui, |ui| {
                    if ui.selectable_label(c.follow.is_none(), "None").clicked() {
                        c.follow = None;
                    }
                    for a in FollowAction::ALL {
                        if ui.selectable_label(c.follow.is_some_and(|f| f.action == a), a.name()).clicked() {
                            let (bars, chance) = c.follow.map(|f| (f.bars, f.chance)).unwrap_or((c.bars().max(1), 1.0));
                            c.follow = Some(Follow { action: a, bars, chance });
                        }
                    }
                });
                self.hint(&resp.response, "Next Action", "What happens after this clip has played for the given number of bars: launch the next/previous/first/last/a random clip in this track, play again, or stop. Build self-playing live sets.");
                if let Some(f) = c.follow.as_mut() {
                    ui.add(egui::DragValue::new(&mut f.bars).range(1..=64).suffix(" bars"));
                    let mut pct = f.chance * 100.0;
                    if ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).suffix("%")).on_hover_text("Chance").changed() {
                        f.chance = pct / 100.0;
                    }
                }
            });
        }
        if !kind.is_drum_like() && c.audio.is_none() {
            ui.horizontal(|ui| {
                ui.label("Transpose");
                for (label, d) in [("-12", -12i32), ("-1", -1), ("+1", 1), ("+12", 12)] {
                    if widgets::button(ui, label, false).clicked() {
                        for n in &mut c.notes {
                            n.pitch = (n.pitch as i32 + d).clamp(0, 127) as u8;
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Draw");
                let lens = [(1u32, "1/16"), (2, "1/8"), (4, "1/4"), (8, "1/2"), (16, "1 bar")];
                let cur = lens.iter().find(|l| l.0 == self.roll.draw_len).map(|l| l.1).unwrap_or("1/16");
                egui::ComboBox::from_id_salt("draw_len").width(60.0).selected_text(cur).show_ui(ui, |ui| {
                    for (v, n) in lens {
                        ui.selectable_value(&mut self.roll.draw_len, v, n);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Stamp");
                let cur = match self.roll.stamp {
                    Some(q) => format!("Chord {}", if CHORD_QUALITIES[q].0.is_empty() { "maj" } else { CHORD_QUALITIES[q].0 }),
                    None => "Single note".into(),
                };
                let r = egui::ComboBox::from_id_salt("stamp").width(84.0).selected_text(cur).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.roll.stamp, None, "Single note");
                    for (q, (name, _)) in CHORD_QUALITIES.iter().enumerate() {
                        ui.selectable_value(&mut self.roll.stamp, Some(q), format!("Chord {}", if name.is_empty() { "maj" } else { name }));
                    }
                });
                self.hint(&r.response, "Chord Stamp", "Clicking in the piano roll draws a whole chord with its root on the clicked key. Add a Strum MIDI effect to spread it in time.");
            });
            ui.horizontal(|ui| {
                let r = widgets::toggle(ui, "Ghosts", self.roll.ghosts, ACCENT, vec2(52.0, 18.0));
                if r.clicked() {
                    self.roll.ghosts = !self.roll.ghosts;
                }
                self.hint(&r, "Ghost Notes", "Shows the notes of the other tracks (same scene, or overlapping on the timeline) faintly behind this clip - write basslines that fit the chords.");
                let r = widgets::toggle(ui, "Rules...", self.roll.rules_open, ACCENT, vec2(52.0, 18.0));
                if r.clicked() {
                    self.roll.rules_open = !self.roll.rules_open;
                }
                self.hint(&r, "Note Rules", "Rule editor: if a note matches a condition (pitch, velocity, position ...), apply an action (transpose, velocity, length, chance, delete ...).");
            });
            ui.horizontal(|ui| {
                let b = widgets::toggle(ui, "Score", self.roll.score, ACCENT, vec2(52.0, 18.0));
                if b.clicked() {
                    self.roll.score = !self.roll.score;
                }
                self.hint(&b, "Score View", "Shows the clip in standard notation instead of the piano roll.");
                let b = widgets::button(ui, "MusicXML", false);
                if b.clicked() {
                    self.export_musicxml(r);
                }
                self.hint(&b, "Export MusicXML", "Saves the clip as MusicXML for notation programs.");
            });
            if let Some(i) = self.roll.selected {
                if let Some(n) = c.notes.get_mut(i) {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}", note_name(n.pitch)));
                        let mut v = n.vel as i32;
                        if ui.add(egui::DragValue::new(&mut v).range(1..=127).prefix("Vel ")).changed() {
                            n.vel = v as u8;
                        }
                        if kind == InstrumentKind::Bass {
                            ui.checkbox(&mut n.slide, "Slide");
                        }
                    });
                    ui.horizontal(|ui| {
                        let mut chance = 100 - n.skip as i32;
                        let r = ui.add(egui::DragValue::new(&mut chance).range(0..=100).prefix("Chance ").suffix(" %"));
                        if r.changed() {
                            n.skip = (100 - chance) as u8;
                        }
                        self.hint(&r, "Note Chance", "Probability that this note plays on each pass - keeps loops evolving.");
                    });
                }
            }
        } else if c.audio.is_none() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Click: toggle step · Drag: paint\nRight-click: velocity · Wheel: velocity\nClick lane name: audition").size(10.0).color(TEXT_DIM));
        }
        if c.name != clip.name || c.color != clip.color || c.len != clip.len || c.notes != clip.notes || c.follow != clip.follow || c.gain != clip.gain {
            if let Some(dst) = self.clip_mut(r) {
                *dst = c;
            }
            self.touch();
        }
        let _ = RenameTarget::Clip(r);
    }

    fn audio_editor(&mut self, ui: &mut egui::Ui, r: ClipRef) {
        let Some(clip) = self.clip(r).cloned() else { return };
        let Some(a) = clip.audio.clone() else { return };
        let mut na = a.clone();
        let mut len = clip.len;
        ui.horizontal(|ui| {
            ui.label("Warp");
            let resp = egui::ComboBox::from_id_salt("warp_mode")
                .width(80.0)
                .selected_text(match na.warp {
                    WarpMode::Off => "Off",
                    WarpMode::Repitch => "Re-Pitch",
                    WarpMode::Beats => "Beats",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut na.warp, WarpMode::Beats, "Beats");
                    ui.selectable_value(&mut na.warp, WarpMode::Repitch, "Re-Pitch");
                    ui.selectable_value(&mut na.warp, WarpMode::Off, "Off");
                })
                .response;
            self.hint(&resp, "Warp Mode", "Beats: follows the song tempo and keeps the pitch (granular time-stretch). Re-Pitch: follows the tempo by changing speed and pitch like a turntable. Off: plays at the original speed.");
            ui.separator();
            let r1 = ui.add(egui::DragValue::new(&mut na.bpm).range(20.0..=400.0).speed(0.1).fixed_decimals(2).prefix("Seg. BPM "));
            self.hint(&r1, "Segment BPM", "The original tempo of the sample. Musicton guesses it from the file length - correct it here if the loop doesn't line up.");
            if widgets::button(ui, "×2", false).clicked() {
                na.bpm = (na.bpm * 2.0).min(400.0);
            }
            if widgets::button(ui, "÷2", false).clicked() {
                na.bpm = (na.bpm / 2.0).max(20.0);
            }
            ui.separator();
            let r2 = ui.add(egui::DragValue::new(&mut na.transpose).range(-48.0..=48.0).speed(0.1).fixed_decimals(1).prefix("Transp. ").suffix(" st"));
            self.hint(&r2, "Transpose", "Pitch shift in semitones (Beats and Off modes).");
            let r3 = ui.add(egui::DragValue::new(&mut na.gain).range(-48.0..=24.0).speed(0.1).fixed_decimals(1).prefix("Gain ").suffix(" dB"));
            self.hint(&r3, "Clip Gain", "Volume of this clip.");
            let r4 = ui.add(egui::DragValue::new(&mut na.offset).range(0.0..=600.0).speed(0.005).fixed_decimals(3).prefix("Start ").suffix(" s"));
            self.hint(&r4, "Start Offset", "Where in the file playback begins.");
            ui.separator();
            if widgets::button(ui, "Fit Length", false).on_hover_text("Set the clip length to the whole file at the segment tempo").clicked() {
                if let Ok(s) = crate::audio::samples::load(&na.path) {
                    let secs = (s.left.len() as f32 / s.sample_rate - na.offset).max(0.01);
                    let tempo = if na.warp == WarpMode::Off { self.project.bpm } else { na.bpm };
                    len = ((secs * tempo / 60.0 * 4.0).round() as u32).max(4);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Convert");
            for (label, mode, tip) in [
                ("Drums to MIDI", crate::audio::analysis::MidiMode::Drums, "Detects kicks, snares and hi-hats and writes them into a new drum track."),
                ("Melody to MIDI", crate::audio::analysis::MidiMode::Melody, "Tracks the pitch of a monophonic line (voice, bass, lead) and writes the notes into a new synth track."),
                ("Harmony to MIDI", crate::audio::analysis::MidiMode::Harmony, "Finds the strongest notes per beat and writes them as chords into a new pad track."),
            ] {
                let b = widgets::button(ui, label, false);
                if b.clicked() {
                    match self.audio_to_midi(r, mode) {
                        Ok((_, n)) => self.toast(format!("{label}: {n} notes")),
                        Err(e) => self.toast(format!("{label}: {e}")),
                    }
                }
                self.hint(&b, label, tip);
            }
            let b = widgets::button(ui, "Split Stems", false);
            if b.clicked() {
                match self.split_stems(r) {
                    Ok(ids) => self.toast(format!("Split into {} stem tracks (original muted)", ids.len())),
                    Err(e) => self.toast(format!("Split failed: {e}")),
                }
            }
            self.hint(&b, "Split Stems", "Separates the clip into Drums, Bass, Lead (centre melody / vocals) and Other on four new audio tracks. Pure signal processing: great for remixing and sampling, not perfectly clean.");
        });
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(vec2(avail.x.max(200.0), (avail.y - 4.0).max(60.0)), Sense::click());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2.0, PANEL_DARK);
        let pps = rect.width() / len.max(1) as f32;
        for s in 0..=len {
            let x = rect.left() + s as f32 * pps;
            let c = if s % 16 == 0 { TEXT_DIM } else if s % 4 == 0 { GRID_STRONG } else { continue };
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, c));
            if s % 16 == 0 && s < len {
                p.text(pos2(x + 3.0, rect.top() + 2.0), Align2::LEFT_TOP, format!("{}", s / 16 + 1), FontId::proportional(10.0), TEXT_DIM);
            }
        }
        let col = color32(clip.color);
        self.draw_audio_preview(&p, rect, &na, len, len, rect.left(), pps, col);
        if let Some(ph) = self.playhead_step(r) {
            let x = rect.left() + ph * pps;
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, Color32::WHITE));
        }
        let name = std::path::Path::new(&na.path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        p.text(rect.right_bottom() - vec2(6.0, 4.0), Align2::RIGHT_BOTTOM, name, FontId::proportional(10.5), TEXT_DIM);
        self.hint(&resp, "Sample Display", "The waveform as it plays in the clip, with bar and beat lines. If the transients don't sit on the beat lines, adjust Seg. BPM.");
        if na != a || len != clip.len {
            if let Some(c) = self.clip_mut(r) {
                c.audio = Some(na);
                c.len = len;
            }
            self.touch();
        }
    }

    pub(super) fn playhead_step(&self, r: ClipRef) -> Option<f32> {
        if !self.status.playing {
            return None;
        }
        let ts = self.track_status(r.track())?;
        let clip = self.clip(r)?;
        match r {
            ClipRef::Session { scene, .. } => (ts.playing == Some(scene)).then_some(ts.clip_pos),
            ClipRef::Arr { track, id } => {
                if ts.arr_clip != Some(id) || !ts.follow_arr {
                    return None;
                }
                let ac = self.project.track(track)?.arrangement.iter().find(|c| c.id == id)?;
                Some((self.status.arr_pos - ac.start as f32).rem_euclid(clip.len.max(1) as f32))
            }
        }
    }

    /// Notes of other melodic tracks that sound together with clip `r`, mapped into its steps.
    fn ghost_notes(&self, r: ClipRef, len: u32) -> Vec<(Note, Color32)> {
        let mut out = vec![];
        match r {
            ClipRef::Session { track, scene } => {
                for t in self.project.tracks.iter().filter(|t| t.id != track && t.instrument.kind.is_melodic()) {
                    if let Some(Some(c)) = t.clips.get(scene) {
                        let cl = c.len.max(1);
                        for rep in 0..len.div_ceil(cl) {
                            out.extend(c.notes.iter().filter(|n| n.step < cl).map(|n| Note { step: n.step + rep * cl, ..*n }).filter(|n| n.step < len).map(|n| (n, color32(t.color))));
                        }
                    }
                }
            }
            ClipRef::Arr { track, id } => {
                let Some(me) = self.project.track(track).and_then(|t| t.arrangement.iter().find(|c| c.id == id)) else { return out };
                let (s0, s1) = (me.start, me.start + len.min(me.len.max(len)));
                for t in self.project.tracks.iter().filter(|t| t.id != track && t.instrument.kind.is_melodic()) {
                    for ac in t.arrangement.iter().filter(|ac| ac.start < s1 && ac.start + ac.len > s0) {
                        let cl = ac.clip.len.max(1);
                        for rep in 0..ac.len.div_ceil(cl) {
                            for n in ac.clip.notes.iter().filter(|n| n.step < cl) {
                                let g = ac.start + rep * cl + n.step;
                                if g >= s0 && g < s1 && g < ac.start + ac.len {
                                    out.push((Note { step: g - s0, ..*n }, color32(t.color)));
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    fn step_sequencer(&mut self, ui: &mut egui::Ui, r: ClipRef, track: u64) {
        let Some(clip) = self.clip(r).cloned() else { return };
        let names = self.project.track(track).map(|t| t.instrument.lane_names()).unwrap_or_default();
        let lanes = names.len().max(1);
        let label_w = 92.0;
        let avail = ui.available_size();
        let row_h = ((avail.y - 8.0) / lanes as f32).clamp(14.0, 24.0);
        let steps = clip.len.max(1) as usize;
        let cell_w = ((avail.x - label_w - 6.0) / steps as f32).max(14.0);
        let grid_w = cell_w * steps as f32;
        let playhead = self.playhead_step(r);
        ui.horizontal_top(|ui| {
            // Lane labels.
            let (lr, _) = ui.allocate_exact_size(vec2(label_w, row_h * lanes as f32), Sense::hover());
            for lane in 0..lanes {
                let rr = Rect::from_min_size(pos2(lr.left(), lr.top() + lane as f32 * row_h), vec2(label_w - 2.0, row_h - 1.0));
                let resp = ui.interact(rr, ui.id().with(("lane_lbl", lane)), Sense::click());
                let sel = self.drum_lane == lane;
                ui.painter().rect_filled(rr, 0.0, if sel { PANEL_LIGHT } else if resp.hovered() { Color32::from_rgb(0x3e, 0x3e, 0x3e) } else { PANEL_DARK });
                let name = names.get(lane).cloned().unwrap_or_default();
                ui.painter().with_clip_rect(rr).text(rr.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, name, FontId::proportional(11.0), if sel { ACCENT } else { TEXT });
                if resp.clicked() {
                    self.drum_lane = lane;
                    self.send(Cmd::NoteOn { track, pitch: lane as u8, vel: 110 });
                }
                if let Some(Payload::Sample(path)) = self.drop_target(ui, rr, |p| matches!(p, Payload::Sample(_))) {
                    self.drum_lane = lane;
                    self.assign_sample(Some(track), lane, &path);
                }
                self.hint(&resp, &names.get(lane).cloned().unwrap_or_default(), "Drum lane. Click to audition and select it (its sound parameters show in the Device Chain). Drop a WAV sample here to replace the sound.");
            }
            egui::ScrollArea::horizontal().drag_to_scroll(false).id_salt("steps_h").show(ui, |ui| {
                let (gr, resp) = ui.allocate_exact_size(vec2(grid_w, row_h * lanes as f32), Sense::click_and_drag());
                let p = ui.painter_at(gr);
                p.rect_filled(gr, 0.0, PANEL_DARK);
                let col = color32(clip.color);
                for s in 0..steps {
                    let x = gr.left() + s as f32 * cell_w;
                    let beat_shade = (s / 4) % 2 == 1;
                    for lane in 0..lanes {
                        let cell = Rect::from_min_size(pos2(x, gr.top() + lane as f32 * row_h), vec2(cell_w - 1.0, row_h - 1.0));
                        let bg = if beat_shade { Color32::from_rgb(0x2c, 0x2c, 0x2c) } else { Color32::from_rgb(0x36, 0x36, 0x36) };
                        p.rect_filled(cell, 1.0, bg);
                    }
                    if s % 16 == 0 && s > 0 {
                        p.line_segment([pos2(x - 0.5, gr.top()), pos2(x - 0.5, gr.bottom())], Stroke::new(1.0_f32, TEXT_DIM));
                    }
                }
                for n in &clip.notes {
                    if (n.pitch as usize) < lanes && (n.step as usize) < steps {
                        let cell = Rect::from_min_size(
                            pos2(gr.left() + n.step as f32 * cell_w, gr.top() + n.pitch as f32 * row_h),
                            vec2(cell_w - 1.0, row_h - 1.0),
                        );
                        let t = n.vel as f32 / 127.0;
                        p.rect_filled(cell, 1.0, lerp(darken(col, 0.35), col, t));
                        if n.vel >= 120 {
                            p.rect_stroke(cell.shrink(1.0), 1.0, Stroke::new(1.0_f32, Color32::WHITE), egui::StrokeKind::Inside);
                        }
                    }
                }
                if let Some(ph) = playhead {
                    let x = gr.left() + ph.floor() * cell_w;
                    p.rect_filled(Rect::from_min_size(pos2(x, gr.top()), vec2(cell_w - 1.0, gr.height())), 0.0, Color32::from_white_alpha(28));
                }
                // Interaction.
                let cell_at = |pos: egui::Pos2| -> Option<(u32, u8)> {
                    if !gr.contains(pos) {
                        return None;
                    }
                    Some((((pos.x - gr.left()) / cell_w) as u32, ((pos.y - gr.top()) / row_h) as u8))
                };
                let pressed = resp.hovered() && ui.input(|i| i.pointer.primary_pressed());
                let down = ui.input(|i| i.pointer.primary_down());
                let shift = ui.input(|i| i.modifiers.shift);
                let pos = ui.input(|i| i.pointer.latest_pos());
                let mut changed = false;
                let mut notes = clip.notes.clone();
                if pressed {
                    if let Some((s, l)) = pos.and_then(cell_at) {
                        let exists = notes.iter().position(|n| n.step == s && n.pitch == l);
                        if shift {
                            if let Some(i) = exists {
                                notes[i].vel = if notes[i].vel >= 120 { 100 } else { 127 };
                            } else {
                                notes.push(Note { step: s, len: 1, pitch: l, vel: 127, slide: false, skip: 0 });
                            }
                            self.roll.paint = None;
                        } else {
                            self.roll.paint = Some(exists.is_none());
                        }
                        changed = true;
                        self.drum_lane = l as usize;
                    }
                }
                if let (Some(paint), true) = (self.roll.paint, down) {
                    if let Some((s, l)) = pos.and_then(cell_at) {
                        let exists = notes.iter().position(|n| n.step == s && n.pitch == l);
                        match (paint, exists) {
                            (true, None) => {
                                notes.push(Note { step: s, len: 1, pitch: l, vel: 100, slide: false, skip: 0 });
                                changed = true;
                                if pressed {
                                    self.send(Cmd::NoteOn { track, pitch: l, vel: 100 });
                                }
                            }
                            (false, Some(i)) => {
                                notes.remove(i);
                                changed = true;
                            }
                            _ => {}
                        }
                    }
                }
                if !down {
                    self.roll.paint = None;
                }
                if resp.secondary_clicked() {
                    if let Some((s, l)) = pos.and_then(cell_at) {
                        if let Some(n) = notes.iter_mut().find(|n| n.step == s && n.pitch == l) {
                            n.vel = match n.vel {
                                v if v >= 120 => 100,
                                v if v >= 90 => 70,
                                v if v >= 60 => 40,
                                _ => 127,
                            };
                            changed = true;
                        }
                    }
                }
                if resp.hovered() {
                    let scroll = ui.input(|i| i.raw_scroll_delta.y);
                    if scroll != 0.0 {
                        if let Some((s, l)) = pos.and_then(cell_at) {
                            if let Some(n) = notes.iter_mut().find(|n| n.step == s && n.pitch == l) {
                                n.vel = (n.vel as i32 + scroll.signum() as i32 * 8).clamp(1, 127) as u8;
                                changed = true;
                            }
                        }
                    }
                }
                if changed {
                    if let Some(c) = self.clip_mut(r) {
                        c.notes = notes;
                    }
                    self.touch();
                }
                self.hint(&resp, "Step Sequencer", "Click or drag to set steps. Shift-click for accents (white frame), right-click or mouse wheel to change velocity. Brighter = louder.");
            });
        });
    }

    fn piano_roll(&mut self, ui: &mut egui::Ui, r: ClipRef, track: u64, kind: InstrumentKind) {
        let Some(clip) = self.clip(r).cloned() else { return };
        let is_bass = kind == InstrumentKind::Bass;
        let avail = ui.available_size();
        let extra_rows = if is_bass { 2.0 * 16.0 } else { 0.0 };
        let vel_h = 44.0;
        let grid_h = (avail.y - vel_h - extra_rows - 6.0).max(80.0);
        let steps = clip.len.max(1) as usize;
        let cell_w = ((avail.x - KEYS_W - 6.0) / steps as f32).max(8.0);
        let grid_w = cell_w * steps as f32;
        let rows_visible = (grid_h / ROW_H).floor();
        if self.roll.scroll_to_notes {
            self.roll.scroll_to_notes = false;
            if !clip.notes.is_empty() {
                let (lo, hi) = clip.notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.pitch), h.max(n.pitch)));
                let mid = (lo as f32 + hi as f32) / 2.0;
                self.roll.top = (mid + rows_visible / 2.0).clamp(rows_visible, 127.0);
            } else {
                self.roll.top = if is_bass { 60.0 } else { 78.0 };
            }
        }
        let top = self.roll.top.round();
        let scale = SCALES[self.project.scale.min(SCALES.len() - 1)].1;
        let key = self.project.key;
        let in_scale = |p: u8| scale.contains(&((p + 12 - key % 12) % 12));
        let playhead = self.playhead_step(r);
        let col = color32(clip.color);

        ui.horizontal_top(|ui| {
            // Keyboard.
            let (kr, kresp) = ui.allocate_exact_size(vec2(KEYS_W, grid_h), Sense::click());
            let p = ui.painter_at(kr);
            for row in 0..rows_visible as i32 {
                let pitch = top as i32 - row;
                if !(0..=127).contains(&pitch) {
                    continue;
                }
                let pitch = pitch as u8;
                let rr = Rect::from_min_size(pos2(kr.left(), kr.top() + row as f32 * ROW_H), vec2(KEYS_W, ROW_H - 1.0));
                let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                p.rect_filled(rr, 0.0, if black { Color32::from_rgb(0x22, 0x22, 0x22) } else { Color32::from_rgb(0xc8, 0xc8, 0xc8) });
                if pitch % 12 == 0 {
                    p.text(rr.right_center() - vec2(3.0, 0.0), Align2::RIGHT_CENTER, note_name(pitch), FontId::proportional(9.0), TEXT_DARK);
                }
            }
            if kresp.clicked() {
                if let Some(pos) = kresp.interact_pointer_pos() {
                    let pitch = (top - ((pos.y - kr.top()) / ROW_H).floor()).clamp(0.0, 127.0) as u8;
                    self.send(Cmd::NoteOn { track, pitch, vel: 100 });
                    self.roll.audition = Some((track, pitch));
                }
            }
            self.hint(&kresp, "Piano Keys", "Click a key to hear it. Scroll the note grid with the mouse wheel.");

            egui::ScrollArea::horizontal().drag_to_scroll(false).id_salt("roll_h").show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                    let (gr, resp) = ui.allocate_exact_size(vec2(grid_w, grid_h), Sense::click_and_drag());
                    let p = ui.painter_at(gr);
                    p.rect_filled(gr, 0.0, PANEL_DARK);
                    for row in 0..rows_visible as i32 {
                        let pitch = top as i32 - row;
                        if !(0..=127).contains(&pitch) {
                            continue;
                        }
                        let pitch = pitch as u8;
                        let rr = Rect::from_min_size(pos2(gr.left(), gr.top() + row as f32 * ROW_H), vec2(grid_w, ROW_H - 1.0));
                        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                        let bg = if !in_scale(pitch) {
                            Color32::from_rgb(0x2a, 0x2a, 0x2a)
                        } else if black {
                            Color32::from_rgb(0x33, 0x33, 0x33)
                        } else {
                            Color32::from_rgb(0x3b, 0x3b, 0x3b)
                        };
                        p.rect_filled(rr, 0.0, bg);
                        if pitch % 12 == key % 12 {
                            p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(0x50, 0x50, 0x50)));
                        }
                    }
                    for s in 0..=steps {
                        let x = gr.left() + s as f32 * cell_w;
                        let c = if s % 16 == 0 { TEXT_DIM } else if s % 4 == 0 { GRID_STRONG } else { Color32::from_rgb(0x2e, 0x2e, 0x2e) };
                        p.line_segment([pos2(x, gr.top()), pos2(x, gr.bottom())], Stroke::new(1.0_f32, c));
                    }
                    let note_rect = |n: &Note| {
                        let y = gr.top() + (top - n.pitch as f32) * ROW_H;
                        Rect::from_min_size(pos2(gr.left() + n.step as f32 * cell_w, y), vec2((n.len as f32 * cell_w - 1.0).max(3.0), ROW_H - 1.0))
                    };
                    if self.roll.ghosts {
                        for (n, gc) in self.ghost_notes(r, clip.len) {
                            let nr = note_rect(&n);
                            if nr.bottom() < gr.top() || nr.top() > gr.bottom() {
                                continue;
                            }
                            p.rect_filled(nr, 1.0, gc.gamma_multiply(0.22));
                            p.rect_stroke(nr, 1.0, Stroke::new(1.0_f32, gc.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                        }
                    }
                    for (i, n) in clip.notes.iter().enumerate() {
                        let nr = note_rect(n);
                        if nr.bottom() < gr.top() || nr.top() > gr.bottom() {
                            continue;
                        }
                        let sel = self.roll.selected == Some(i);
                        let t = n.vel as f32 / 127.0;
                        let fill = lerp(darken(col, 0.4), col, t);
                        p.rect_filled(nr, 1.0, if n.skip > 0 { fill.gamma_multiply(0.35 + 0.65 * (100 - n.skip) as f32 / 100.0) } else { fill });
                        if n.skip > 0 && nr.width() > 22.0 {
                            p.text(nr.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, format!("{}%", 100 - n.skip), FontId::proportional(8.5), Color32::BLACK);
                        }
                        p.rect_stroke(nr, 1.0, Stroke::new(1.0_f32, if sel { Color32::WHITE } else { darken(col, 0.3) }), egui::StrokeKind::Inside);
                        if n.slide {
                            p.line_segment([nr.right_center(), nr.right_center() + vec2(cell_w * 0.5, 0.0)], Stroke::new(2.0_f32, col));
                        }
                    }
                    if let Some(ph) = playhead {
                        let x = gr.left() + ph * cell_w;
                        p.line_segment([pos2(x, gr.top()), pos2(x, gr.bottom())], Stroke::new(1.0_f32, Color32::WHITE));
                    }

                    // Interaction.
                    let pos = ui.input(|i| i.pointer.latest_pos());
                    let pressed = resp.hovered() && ui.input(|i| i.pointer.primary_pressed());
                    let down = ui.input(|i| i.pointer.primary_down());
                    let alt = ui.input(|i| i.modifiers.alt);
                    let step_at = |x: f32| ((x - gr.left()) / cell_w).max(0.0);
                    let pitch_at = |y: f32| (top - ((y - gr.top()) / ROW_H).floor()).clamp(0.0, 127.0) as i32;
                    let mut notes = clip.notes.clone();
                    let mut changed = false;
                    if resp.hovered() {
                        let scroll = ui.input(|i| i.raw_scroll_delta.y);
                        if scroll != 0.0 {
                            self.roll.top = (self.roll.top + scroll.signum() * 2.0).clamp(rows_visible, 127.0);
                        }
                        if let Some(pos) = pos {
                            if let Some(n) = notes.iter().find(|n| note_rect(n).contains(pos)) {
                                let nr = note_rect(n);
                                if pos.x > nr.right() - 5.0 {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                                }
                            }
                        }
                    }
                    if pressed {
                        if let Some(pos) = pos {
                            let hit = notes.iter().rposition(|n| note_rect(n).contains(pos));
                            match hit {
                                Some(i) => {
                                    let nr = note_rect(&notes[i]);
                                    self.roll.selected = Some(i);
                                    self.roll.drag = Some(if pos.x > nr.right() - 5.0 {
                                        NoteDrag::Resize { idx: i }
                                    } else {
                                        NoteDrag::Move { idx: i, grab_step: step_at(pos.x) - notes[i].step as f32, grab_pitch: pitch_at(pos.y) - notes[i].pitch as i32 }
                                    });
                                    let p = notes[i].pitch;
                                    self.send(Cmd::NoteOn { track, pitch: p, vel: notes[i].vel });
                                    self.roll.audition = Some((track, p));
                                }
                                None => {
                                    let step = step_at(pos.x).floor() as u32;
                                    let pitch = pitch_at(pos.y) as u8;
                                    if (step as usize) < steps {
                                        if let Some(q) = self.roll.stamp {
                                            for iv in CHORD_QUALITIES[q].1.iter().skip(1) {
                                                if let Some(p2) = pitch.checked_add(*iv).filter(|p| *p <= 127) {
                                                    notes.push(Note { step, len: self.roll.draw_len, pitch: p2, vel: 95, slide: false, skip: 0 });
                                                }
                                            }
                                        }
                                        notes.push(Note { step, len: self.roll.draw_len, pitch, vel: 100, slide: false, skip: 0 });
                                        let i = notes.len() - 1;
                                        self.roll.selected = Some(i);
                                        self.roll.drag = Some(NoteDrag::Move { idx: i, grab_step: step_at(pos.x) - step as f32, grab_pitch: 0 });
                                        changed = true;
                                        self.send(Cmd::NoteOn { track, pitch, vel: 100 });
                                        self.roll.audition = Some((track, pitch));
                                    }
                                }
                            }
                        }
                    }
                    if down {
                        if let (Some(drag), Some(pos)) = (self.roll.drag, pos) {
                            match drag {
                                NoteDrag::Move { idx, grab_step, grab_pitch } if idx < notes.len() => {
                                    let raw = step_at(pos.x) - grab_step;
                                    let step = if alt { raw.round() } else { raw.round() }.clamp(0.0, steps as f32 - 1.0) as u32;
                                    let pitch = (pitch_at(pos.y) - grab_pitch).clamp(0, 127) as u8;
                                    if notes[idx].step != step || notes[idx].pitch != pitch {
                                        if notes[idx].pitch != pitch {
                                            if let Some((t, old)) = self.roll.audition.take() {
                                                self.send(Cmd::NoteOff { track: t, pitch: old });
                                            }
                                            self.send(Cmd::NoteOn { track, pitch, vel: notes[idx].vel });
                                            self.roll.audition = Some((track, pitch));
                                        }
                                        notes[idx].step = step;
                                        notes[idx].pitch = pitch;
                                        changed = true;
                                    }
                                }
                                NoteDrag::Resize { idx } if idx < notes.len() => {
                                    let end = step_at(pos.x).round() as i32;
                                    let len = (end - notes[idx].step as i32).max(1) as u32;
                                    if len != notes[idx].len {
                                        notes[idx].len = len;
                                        self.roll.draw_len = len;
                                        changed = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else {
                        self.roll.drag = None;
                        if let Some((t, p)) = self.roll.audition.take() {
                            self.send(Cmd::NoteOff { track: t, pitch: p });
                        }
                    }
                    if resp.double_clicked() || resp.secondary_clicked() {
                        if let Some(pos) = pos {
                            if let Some(i) = notes.iter().rposition(|n| note_rect(n).contains(pos)) {
                                notes.remove(i);
                                self.roll.selected = None;
                                self.roll.drag = None;
                                changed = true;
                            }
                        }
                    }
                    self.hint(&resp, "Piano Roll", "Click to draw a note, drag to move it, drag its right edge to change its length. Double-click or right-click deletes. Mouse wheel scrolls. Grey rows are outside the selected scale.");

                    // 303 accent and slide rows.
                    if is_bass {
                        for (row, label) in [(0, "Accent"), (1, "Slide")] {
                            let (rr, rresp) = ui.allocate_exact_size(vec2(grid_w, 15.0), Sense::click());
                            let p = ui.painter_at(rr);
                            p.rect_filled(rr, 0.0, PANEL_DARK);
                            for s in 0..steps {
                                let cell = Rect::from_min_size(pos2(rr.left() + s as f32 * cell_w, rr.top()), vec2(cell_w - 1.0, 14.0));
                                let at: Vec<&Note> = notes.iter().filter(|n| n.step as usize == s).collect();
                                let on = if row == 0 { at.iter().any(|n| n.vel >= 110) } else { at.iter().any(|n| n.slide) };
                                let has = !at.is_empty();
                                p.rect_filled(cell, 1.0, if on { if row == 0 { ACCENT } else { BLUE } } else if has { Color32::from_rgb(0x44, 0x44, 0x44) } else { Color32::from_rgb(0x30, 0x30, 0x30) });
                            }
                            p.text(rr.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(9.0), TEXT_DIM);
                            if rresp.clicked() {
                                if let Some(pos) = rresp.interact_pointer_pos() {
                                    let s = ((pos.x - rr.left()) / cell_w) as u32;
                                    let on = if row == 0 { notes.iter().any(|n| n.step == s && n.vel >= 110) } else { notes.iter().any(|n| n.step == s && n.slide) };
                                    for n in notes.iter_mut().filter(|n| n.step == s) {
                                        if row == 0 {
                                            n.vel = if on { 90 } else { 120 };
                                        } else {
                                            n.slide = !on;
                                        }
                                        changed = true;
                                    }
                                }
                            }
                            let info = if row == 0 {
                                "Accent row: click a step with a note to accent it (velocity 120). Accents open the filter and play louder."
                            } else {
                                "Slide row: click a step with a note to glide from it into the next note without retriggering - the classic 303 slide."
                            };
                            self.hint(&rresp, label, info);
                        }
                    }

                    // Velocity lane.
                    let (vr, vresp) = ui.allocate_exact_size(vec2(grid_w, vel_h), Sense::click_and_drag());
                    let p = ui.painter_at(vr);
                    p.rect_filled(vr, 0.0, PANEL_DARK);
                    for (i, n) in notes.iter().enumerate() {
                        let x = vr.left() + n.step as f32 * cell_w + 1.0;
                        let h = n.vel as f32 / 127.0 * (vel_h - 4.0);
                        let c = if self.roll.selected == Some(i) { Color32::WHITE } else { col };
                        p.line_segment([pos2(x, vr.bottom()), pos2(x, vr.bottom() - h)], Stroke::new(2.0_f32, c));
                        p.circle_filled(pos2(x, vr.bottom() - h), 2.5, c);
                    }
                    if vresp.dragged() || vresp.clicked() {
                        if let Some(pos) = vresp.interact_pointer_pos() {
                            let s = ((pos.x - vr.left()) / cell_w) as u32;
                            let v = (((vr.bottom() - pos.y) / (vel_h - 4.0)) * 127.0).clamp(1.0, 127.0) as u8;
                            let sel = self.roll.selected;
                            let target: Vec<usize> = match sel {
                                Some(i) if notes.get(i).is_some_and(|n| n.step == s) => vec![i],
                                _ => notes.iter().enumerate().filter(|(_, n)| n.step == s).map(|(i, _)| i).collect(),
                            };
                            for i in target {
                                notes[i].vel = v;
                                changed = true;
                            }
                            self.roll.drag = Some(NoteDrag::Velocity);
                        }
                    }
                    self.hint(&vresp, "Velocity", "Drag the stems to set how hard each note is played.");

                    if changed {
                        if let Some(c) = self.clip_mut(r) {
                            c.notes = notes;
                        }
                        self.touch();
                    }
                });
            });
        });
    }
}
