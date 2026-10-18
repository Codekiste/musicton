//! Arranger lane (song sections), markers, section chain and the pattern playlist.

use super::theme::*;
use super::widgets;
use super::App;
use crate::audio::engine::Cmd;
use crate::model::*;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, UiBuilder};

pub const MARK_H: f32 = 15.0;
pub const SEC_H: f32 = 21.0;
pub const CHORD_H: f32 = 19.0;
pub const PL_LANES: u32 = 8;
pub const PL_LANE_H: f32 = 40.0;

#[derive(Clone, Copy, Debug)]
enum Drag {
    Section { id: u64, grab: f32, to: u32 },
    SectionEnd { id: u64 },
    Marker { id: u64 },
    Block { id: u64, grab: f32 },
    BlockEnd { id: u64 },
    Chord { id: u64, grab: f32 },
    ChordEnd { id: u64 },
}

#[derive(Default)]
pub struct ArrangerState {
    drag: Option<Drag>,
    /// Shows the pattern playlist instead of the track lanes.
    pub playlist: bool,
    /// Scene placed by double-clicking a playlist lane.
    pub pl_scene: usize,
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn bar_snap(step: f32, alt: bool) -> u32 {
    let g = if alt { 4.0 } else { 16.0 };
    ((step / g).round() * g).max(0.0) as u32
}

impl App {
    pub(super) fn marker_strip(&mut self, ui: &mut egui::Ui, r: Rect, x_of: &dyn Fn(f32, f32) -> f32) {
        let p = ui.painter().with_clip_rect(r);
        let sx = self.arr.scroll_x;
        let pps = self.arr.pps;
        p.rect_filled(r, 0.0, PANEL_DARK);
        p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, LINE));
        let resp = ui.interact(r, ui.id().with("markers"), Sense::click_and_drag());
        let pos = resp.interact_pointer_pos().or(resp.hover_pos());
        let alt = ui.input(|i| i.modifiers.alt);
        let mut hit = None;
        for m in &self.project.markers {
            let x = x_of(m.pos as f32, sx);
            let w = 8.0 + m.name.len() as f32 * 6.0;
            let flag = Rect::from_min_size(pos2(x, r.top() + 1.0), vec2(w, r.height() - 2.0));
            p.rect_filled(flag, 2.0, Color32::from_rgb(0x55, 0x55, 0x60));
            p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(1.5_f32, TEXT));
            p.text(pos2(x + 4.0, r.center().y), Align2::LEFT_CENTER, &m.name, FontId::proportional(10.0), TEXT);
            if pos.is_some_and(|q| flag.contains(q)) {
                hit = Some(m.id);
            }
        }
        if let Some(pos) = pos {
            let step = (pos.x - r.left()) / pps + sx;
            if resp.drag_started() {
                if let Some(id) = hit {
                    self.arr.ext.drag = Some(Drag::Marker { id });
                }
            }
            if let (true, Some(Drag::Marker { id })) = (resp.dragged(), self.arr.ext.drag) {
                let snapped = self.snap(step, alt);
                if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
                    m.pos = snapped;
                }
                self.touch();
            }
            if resp.clicked() {
                if let Some(m) = hit.and_then(|id| self.project.markers.iter().find(|m| m.id == id)) {
                    self.send(Cmd::SetStart(m.pos));
                }
            }
            if resp.double_clicked() && hit.is_none() {
                let pos = self.snap(step, alt);
                self.add_marker(None, pos);
            }
        }
        if resp.drag_stopped() {
            self.arr.ext.drag = None;
            self.project.markers.sort_by_key(|m| m.pos);
        }
        if let Some(id) = hit {
            resp.context_menu(|ui| {
                if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
                    ui.label("Marker name");
                    if ui.text_edit_singleline(&mut m.name).changed() {
                        self.changed = true;
                    }
                }
                if ui.button("Delete Marker").clicked() {
                    self.project.markers.retain(|m| m.id != id);
                    self.touch();
                    ui.close_menu();
                }
            });
        }
        self.hint(&resp, "Markers", "Double-click to add a marker, click a marker to jump to it, drag to move it. Right-click to rename or delete.");
    }

    pub(super) fn add_marker(&mut self, name: Option<&str>, pos: u32) -> u64 {
        let id = self.project.new_id();
        let name = name.map(str::to_string).unwrap_or_else(|| format!("M{}", self.project.markers.len() + 1));
        self.project.markers.push(Marker { id, name, pos });
        self.project.markers.sort_by_key(|m| m.pos);
        self.touch();
        id
    }

    pub(super) fn section_lane(&mut self, ui: &mut egui::Ui, r: Rect, x_of: &dyn Fn(f32, f32) -> f32) {
        let p = ui.painter().with_clip_rect(r);
        let sx = self.arr.scroll_x;
        let pps = self.arr.pps;
        p.rect_filled(r, 0.0, PANEL);
        p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, LINE));
        let resp = ui.interact(r, ui.id().with("sections"), Sense::click_and_drag());
        let pos = resp.interact_pointer_pos().or(resp.hover_pos());
        let alt = ui.input(|i| i.modifiers.alt);
        let mut hit = None;
        let mut edge = false;
        for s in &self.project.sections {
            let (mut start, len) = (s.start, s.len);
            if let Some(Drag::Section { id, to, .. }) = self.arr.ext.drag {
                if id == s.id {
                    start = to;
                }
            }
            let sr = Rect::from_x_y_ranges(x_of(start as f32, sx)..=x_of((start + len) as f32, sx) - 1.0, r.top() + 2.0..=r.bottom() - 2.0);
            p.rect_filled(sr, 3.0, rgb(s.color).gamma_multiply(0.85));
            let mut label = s.name.clone();
            let chain_pos: Vec<String> =
                self.project.chain.iter().enumerate().filter(|(_, c)| **c == s.id).map(|(i, _)| (i + 1).to_string()).collect();
            if !chain_pos.is_empty() {
                label = format!("{label}  [{}]", chain_pos.join(","));
            }
            p.text(pos2(sr.left() + 5.0, sr.center().y), Align2::LEFT_CENTER, label, FontId::proportional(11.0), Color32::BLACK);
            if let Some(q) = pos {
                if sr.contains(q) {
                    hit = Some(s.id);
                    edge = (q.x - sr.right()).abs() < 6.0;
                }
            }
        }
        if let Some(pos) = pos {
            let step = (pos.x - r.left()) / pps + sx;
            if resp.drag_started() {
                if let Some(id) = hit {
                    let s = self.project.sections.iter().find(|s| s.id == id).unwrap();
                    self.arr.ext.drag = Some(if edge { Drag::SectionEnd { id } } else { Drag::Section { id, grab: step - s.start as f32, to: s.start } });
                }
            }
            if resp.dragged() {
                match self.arr.ext.drag {
                    Some(Drag::Section { id, grab, .. }) => {
                        self.arr.ext.drag = Some(Drag::Section { id, grab, to: bar_snap(step - grab, alt) });
                    }
                    Some(Drag::SectionEnd { id }) => {
                        if let Some(s) = self.project.sections.iter_mut().find(|s| s.id == id) {
                            s.len = bar_snap(step, alt).saturating_sub(s.start).max(4);
                        }
                        self.touch();
                    }
                    _ => {}
                }
            }
            if resp.clicked() {
                if let Some(s) = hit.and_then(|id| self.project.sections.iter().find(|s| s.id == id)) {
                    self.send(Cmd::SetStart(s.start));
                }
            }
            if resp.double_clicked() && hit.is_none() {
                let start = ((step / 16.0).floor() * 16.0).max(0.0) as u32;
                self.project.add_section(None, start, 128);
                self.touch();
            }
        }
        if resp.drag_stopped() {
            if let Some(Drag::Section { id, to, .. }) = self.arr.ext.drag {
                self.project.move_section(id, to);
                self.touch();
            }
            self.arr.ext.drag = None;
        }
        if let Some(id) = hit {
            resp.context_menu(|ui| self.section_menu(ui, id));
        }
        self.hint(
            &resp,
            "Arranger Lane",
            "Song sections. Double-click to add one, drag to move it with all clips and automation inside, drag the right edge to resize. Right-click: rename, duplicate, delete, loop, add to the play chain.",
        );
    }

    fn section_menu(&mut self, ui: &mut egui::Ui, id: u64) {
        let Some(s) = self.project.sections.iter_mut().find(|s| s.id == id) else { return };
        ui.label("Section name");
        if ui.text_edit_singleline(&mut s.name).changed() {
            self.changed = true;
        }
        ui.horizontal(|ui| {
            for c in CLIP_COLORS.iter().step_by(3) {
                let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                ui.painter().rect_filled(r, 2.0, rgb(*c));
                if resp.clicked() {
                    s.color = *c;
                    self.changed = true;
                }
            }
        });
        let (start, len) = (s.start, s.len);
        ui.separator();
        if ui.button("Play from here").clicked() {
            self.send(Cmd::SetStart(start));
            self.send(Cmd::Play);
            ui.close_menu();
        }
        if ui.button("Loop this section").clicked() {
            self.project.loop_start = start;
            self.project.loop_len = len;
            self.project.loop_on = true;
            self.touch();
            ui.close_menu();
        }
        if ui.button("Add to chain").clicked() {
            self.project.chain.push(id);
            self.touch();
            ui.close_menu();
        }
        if ui.button("Duplicate (with content)").clicked() {
            self.project.duplicate_section(id);
            self.touch();
            ui.close_menu();
        }
        if ui.button("Delete (with content)").clicked() {
            self.project.delete_section(id);
            self.touch();
            ui.close_menu();
        }
        if ui.button("Remove section only").clicked() {
            self.project.sections.retain(|s| s.id != id);
            self.project.chain.retain(|c| *c != id);
            self.touch();
            ui.close_menu();
        }
    }

    /// Chord track: chords on the timeline that drive "Follow Chords" tracks.
    pub(super) fn chord_lane(&mut self, ui: &mut egui::Ui, r: Rect, x_of: &dyn Fn(f32, f32) -> f32) {
        let p = ui.painter().with_clip_rect(r);
        let sx = self.arr.scroll_x;
        let pps = self.arr.pps;
        p.rect_filled(r, 0.0, PANEL_DARK);
        p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, LINE));
        let resp = ui.interact(r, ui.id().with("chords"), Sense::click_and_drag());
        let pos = resp.interact_pointer_pos().or(resp.hover_pos());
        let alt = ui.input(|i| i.modifiers.alt);
        let mut hit = None;
        let mut edge = false;
        for c in &self.project.chords {
            let cr = Rect::from_x_y_ranges(x_of(c.start as f32, sx)..=x_of((c.start + c.len) as f32, sx) - 1.0, r.top() + 2.0..=r.bottom() - 2.0);
            p.rect_filled(cr, 3.0, Color32::from_rgb(0x4a, 0x56, 0x6e));
            p.text(pos2(cr.left() + 4.0, cr.center().y), Align2::LEFT_CENTER, c.name(), FontId::proportional(11.0), TEXT);
            if let Some(q) = pos {
                if cr.contains(q) {
                    hit = Some(c.id);
                    edge = (q.x - cr.right()).abs() < 6.0;
                }
            }
        }
        if self.project.chords.is_empty() {
            p.text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, "Chords - double-click to add", FontId::proportional(10.0), TEXT_DIM);
        }
        if let Some(q) = pos {
            let step = (q.x - r.left()) / pps + sx;
            if resp.drag_started() {
                if let Some(id) = hit {
                    let c = self.project.chords.iter().find(|c| c.id == id).unwrap();
                    self.arr.ext.drag = Some(if edge { Drag::ChordEnd { id } } else { Drag::Chord { id, grab: step - c.start as f32 } });
                }
            }
            if resp.dragged() {
                match self.arr.ext.drag {
                    Some(Drag::Chord { id, grab }) => {
                        let s = self.snap(step - grab, alt);
                        if let Some(c) = self.project.chords.iter_mut().find(|c| c.id == id) {
                            c.start = s;
                        }
                        self.touch();
                    }
                    Some(Drag::ChordEnd { id }) => {
                        let e = self.snap(step, alt);
                        if let Some(c) = self.project.chords.iter_mut().find(|c| c.id == id) {
                            c.len = e.saturating_sub(c.start).max(4);
                        }
                        self.touch();
                    }
                    _ => {}
                }
            }
            if resp.double_clicked() && hit.is_none() {
                // Cycle through a classic progression: I - VI - IV - V.
                let start = ((step / 16.0).floor() * 16.0).max(0.0) as u32;
                let deg = [0usize, 5, 3, 4][self.project.chords.len() % 4];
                let scale = if SCALES[self.project.scale.min(SCALES.len() - 1)].1.len() == 7 { self.project.scale } else { 2 };
                let (root, q) = diatonic_chord(self.project.key, scale, deg, false);
                self.add_chord_event(start, 16, root, q);
            }
        }
        if resp.drag_stopped() {
            self.arr.ext.drag = None;
            self.project.chords.sort_by_key(|c| c.start);
        }
        if let Some(id) = hit {
            resp.context_menu(|ui| {
                let Some(c) = self.project.chords.iter_mut().find(|c| c.id == id) else { return };
                ui.label("Root");
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(220.0);
                    for (k, n) in NOTE_NAMES.iter().enumerate() {
                        if ui.selectable_label(c.root as usize == k, *n).clicked() {
                            c.root = k as u8;
                            self.changed = true;
                        }
                    }
                });
                ui.label("Quality");
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(220.0);
                    for (k, (n, _)) in CHORD_QUALITIES.iter().enumerate() {
                        if ui.selectable_label(c.quality == k, if n.is_empty() { "maj" } else { n }).clicked() {
                            c.quality = k;
                            self.changed = true;
                        }
                    }
                });
                ui.separator();
                if ui.button("Delete chord").clicked() {
                    self.project.chords.retain(|c| c.id != id);
                    self.touch();
                    ui.close_menu();
                }
            });
        }
        self.hint(&resp, "Chord Track", "The song's chords. Double-click to add one (I-VI-IV-V of the key), drag to move, drag the edge to resize, right-click to change root and quality. Tracks with 'Follow Chords' bend their notes to the current chord.");
    }

    /// Chain editor and playlist toggle, shown in the timeline's corner.
    pub(super) fn arranger_tools(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let r = widgets::toggle(ui, "Playlist", self.arr.ext.playlist, ACCENT, vec2(52.0, 18.0));
            if r.clicked() {
                self.arr.ext.playlist = !self.arr.ext.playlist;
            }
            self.hint(&r, "Playlist", "Switches the lanes to the pattern playlist: place scenes as blocks on free lanes. The timeline plays a block's clips wherever a track has no timeline clip.");
            let r = widgets::toggle(ui, "Pads", self.chord_pads.open, ACCENT, vec2(38.0, 18.0));
            if r.clicked() {
                self.chord_pads.open = !self.chord_pads.open;
            }
            self.hint(&r, "Chord Pads", "Opens the chord pads: the chords of the song key on eight pads, played on the selected track.");
            let r = widgets::toggle(ui, "Chain", self.project.chain_on, ACCENT, vec2(44.0, 18.0));
            if r.clicked() {
                self.project.chain_on = !self.project.chain_on;
                self.touch();
            }
            self.hint(&r, "Section Chain", "Plays the sections in chain order instead of left to right. Right-click a section to add it; right-click this button to edit the order.");
            r.context_menu(|ui| {
                ui.label("Play order");
                if self.project.chain.is_empty() {
                    ui.weak("Empty - right-click a section and choose 'Add to chain'.");
                }
                let mut action = None;
                for (i, cid) in self.project.chain.iter().enumerate() {
                    let name = self.project.sections.iter().find(|s| s.id == *cid).map(|s| s.name.clone()).unwrap_or_else(|| "?".into());
                    ui.horizontal(|ui| {
                        ui.label(format!("{}. {name}", i + 1));
                        if ui.small_button("Up").clicked() {
                            action = Some((i, 0));
                        }
                        if ui.small_button("Down").clicked() {
                            action = Some((i, 1));
                        }
                        if ui.small_button("×").clicked() {
                            action = Some((i, 2));
                        }
                    });
                }
                if let Some((i, a)) = action {
                    let c = &mut self.project.chain;
                    match a {
                        0 if i > 0 => c.swap(i, i - 1),
                        1 if i + 1 < c.len() => c.swap(i, i + 1),
                        2 => {
                            c.remove(i);
                        }
                        _ => {}
                    }
                    self.touch();
                }
                if ui.button("Chain all sections").clicked() {
                    self.project.chain = self.project.sections.iter().map(|s| s.id).collect();
                    self.touch();
                }
            });
        });
    }

    /// Pattern playlist: scenes as blocks on free lanes.
    pub(super) fn playlist_view(&mut self, ui: &mut egui::Ui, lanes: Rect, headers: Rect, x_of: &dyn Fn(f32, f32) -> f32) {
        let sx = self.arr.scroll_x;
        let pps = self.arr.pps;
        let lp = ui.painter().with_clip_rect(lanes);
        let lane_rect = |k: u32| Rect::from_min_size(pos2(lanes.left(), lanes.top() + k as f32 * PL_LANE_H), vec2(lanes.width(), PL_LANE_H));
        for k in 0..PL_LANES {
            lp.line_segment([lane_rect(k).left_bottom(), lane_rect(k).right_bottom()], Stroke::new(1.0_f32, LINE));
        }
        let resp = ui.interact(Rect::from_min_size(lanes.min, vec2(lanes.width(), PL_LANES as f32 * PL_LANE_H)), ui.id().with("playlist"), Sense::click_and_drag());
        let pos = resp.interact_pointer_pos().or(resp.hover_pos());
        let alt = ui.input(|i| i.modifiers.alt);
        let mut hit = None;
        let mut edge = false;
        for b in &self.project.playlist {
            let lr = lane_rect(b.lane.min(PL_LANES - 1));
            let br = Rect::from_x_y_ranges(x_of(b.start as f32, sx)..=x_of((b.start + b.len) as f32, sx) - 1.0, lr.top() + 2.0..=lr.bottom() - 2.0);
            let col = rgb(CLIP_COLORS[(b.scene * 3) % CLIP_COLORS.len()]);
            lp.rect_filled(br, 3.0, col.gamma_multiply(0.8));
            lp.rect_stroke(br, 3.0, Stroke::new(1.0_f32, col), egui::StrokeKind::Inside);
            let name = self.project.scenes.get(b.scene).map(|s| s.name.as_str()).unwrap_or("?");
            lp.text(br.left_top() + vec2(5.0, 3.0), Align2::LEFT_TOP, name, FontId::proportional(11.0), Color32::BLACK);
            // Mini overview: one tick per track that has a clip in this scene.
            let mut ty = br.top() + 18.0;
            for t in self.project.tracks.iter().filter(|t| t.clips.get(b.scene).is_some_and(|c| c.is_some())).take(5) {
                lp.line_segment([pos2(br.left() + 4.0, ty), pos2(br.right() - 4.0, ty)], Stroke::new(2.0_f32, rgb(t.color).gamma_multiply(0.6)));
                ty += 3.5;
            }
            if let Some(q) = pos {
                if br.contains(q) {
                    hit = Some(b.id);
                    edge = (q.x - br.right()).abs() < 6.0;
                }
            }
        }
        if let Some(q) = pos {
            let step = (q.x - lanes.left()) / pps + sx;
            let lane = (((q.y - lanes.top()) / PL_LANE_H).floor().max(0.0) as u32).min(PL_LANES - 1);
            if resp.drag_started() {
                if let Some(id) = hit {
                    let b = self.project.playlist.iter().find(|b| b.id == id).unwrap();
                    self.arr.ext.drag = Some(if edge { Drag::BlockEnd { id } } else { Drag::Block { id, grab: step - b.start as f32 } });
                }
            }
            if resp.dragged() {
                match self.arr.ext.drag {
                    Some(Drag::Block { id, grab }) => {
                        let start = self.snap(step - grab, alt);
                        if let Some(b) = self.project.playlist.iter_mut().find(|b| b.id == id) {
                            b.start = start;
                            b.lane = lane;
                        }
                        self.touch();
                    }
                    Some(Drag::BlockEnd { id }) => {
                        let end = self.snap(step, alt);
                        if let Some(b) = self.project.playlist.iter_mut().find(|b| b.id == id) {
                            b.len = end.saturating_sub(b.start).max(4);
                        }
                        self.touch();
                    }
                    _ => {}
                }
            }
            if resp.double_clicked() && hit.is_none() {
                let start = self.snap_floor(step, alt);
                let scene = self.arr.ext.pl_scene;
                self.add_block(scene, start, lane, None);
            }
        }
        if resp.drag_stopped() {
            self.arr.ext.drag = None;
        }
        if let Some(id) = hit {
            resp.context_menu(|ui| {
                ui.label("Pattern");
                let names: Vec<String> = self.project.scenes.iter().map(|s| s.name.clone()).collect();
                for (i, n) in names.iter().enumerate() {
                    if ui.button(n).clicked() {
                        if let Some(b) = self.project.playlist.iter_mut().find(|b| b.id == id) {
                            b.scene = i;
                        }
                        self.touch();
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("Duplicate").clicked() {
                    if let Some(b) = self.project.playlist.iter().find(|b| b.id == id).cloned() {
                        self.add_block(b.scene, b.start + b.len, b.lane, Some(b.len));
                    }
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    self.project.playlist.retain(|b| b.id != id);
                    self.touch();
                    ui.close_menu();
                }
            });
        }
        self.hint(&resp, "Playlist", "Pick a pattern on the left, double-click a lane to place it. Drag blocks to move them (also between lanes), drag the right edge to resize. Right-click to change the pattern, duplicate or delete.");

        // Pattern palette in the header column.
        let hp = ui.painter().with_clip_rect(headers);
        hp.rect_filled(headers, 0.0, PANEL);
        ui.scope_builder(UiBuilder::new().max_rect(headers.shrink(6.0)), |ui| {
            ui.label(egui::RichText::new("PATTERNS").size(10.0).color(TEXT_DIM));
            let names: Vec<String> = self.project.scenes.iter().map(|s| s.name.clone()).collect();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    let col = rgb(CLIP_COLORS[(i * 3) % CLIP_COLORS.len()]);
                    let sel = self.arr.ext.pl_scene == i;
                    let r = widgets::toggle(ui, &format!("{}  {n}", i + 1), sel, col, vec2(headers.width() - 16.0, 20.0));
                    if r.clicked() {
                        self.arr.ext.pl_scene = i;
                    }
                }
            });
        });
    }

    pub(super) fn add_block(&mut self, scene: usize, start: u32, lane: u32, len: Option<u32>) -> u64 {
        let len = len.unwrap_or_else(|| {
            let l = self.project.tracks.iter().filter_map(|t| t.clips.get(scene).and_then(|c| c.as_ref())).map(|c| c.len).max().unwrap_or(16);
            l.div_ceil(16).max(1) * 16
        });
        let id = self.project.new_id();
        self.project.playlist.push(PlaylistBlock { id, scene, start, len, lane: lane.min(PL_LANES - 1) });
        self.touch();
        id
    }
}
