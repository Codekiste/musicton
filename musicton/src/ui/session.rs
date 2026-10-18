//! Launcher: clip grid, scene launcher and mixer.

use super::theme::*;
use super::widgets;
use super::{App, ClipRef, Detail, Payload, RenameTarget, Sel, MASTER_METER, RETURN_METER};
use crate::audio::engine::Cmd;
use crate::model::*;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Shape, Stroke, UiBuilder};

pub const COL_W: f32 = 112.0;
const SLOT_H: f32 = 22.0;
const HEADER_H: f32 = 22.0;
const DROP_W: f32 = 150.0;
const MASTER_W: f32 = 120.0;
const MIXER_H: f32 = 232.0;

impl App {
    pub(super) fn session_view(&mut self, ui: &mut egui::Ui) {
        let n = self.visible_tracks().len();
        let width = n as f32 * COL_W + DROP_W + 2.0 * COL_W + MASTER_W;
        egui::ScrollArea::horizontal().drag_to_scroll(false).id_salt("session_h").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let (hdr, _) = ui.allocate_exact_size(vec2(width, HEADER_H), Sense::hover());
            self.session_headers(ui, hdr);
            let grid_h = (ui.available_height() - MIXER_H - SLOT_H - 4.0).max(60.0);
            egui::ScrollArea::vertical().drag_to_scroll(false).id_salt("session_v").max_height(grid_h).auto_shrink([false, false]).show(ui, |ui| {
                let rows = self.project.scenes.len() as f32 + 1.0;
                let h = (rows * SLOT_H).max(grid_h);
                let (grid, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
                self.session_grid(ui, grid);
            });
            ui.add_space(2.0);
            let (stop_row, _) = ui.allocate_exact_size(vec2(width, SLOT_H), Sense::hover());
            self.session_stop_row(ui, stop_row);
            ui.add_space(2.0);
            let (mix, _) = ui.allocate_exact_size(vec2(width, MIXER_H), Sense::hover());
            self.session_mixer(ui, mix);
        });
    }

    fn col_x(&self, rect: Rect, col: usize) -> f32 {
        let n = self.visible_tracks().len();
        if col < n {
            rect.left() + col as f32 * COL_W
        } else {
            rect.left() + n as f32 * COL_W + DROP_W + (col - n) as f32 * COL_W
        }
    }

    fn master_x(&self, rect: Rect) -> f32 {
        rect.left() + self.visible_tracks().len() as f32 * COL_W + DROP_W + 2.0 * COL_W
    }

    fn session_headers(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let ids: Vec<u64> = self.visible_tracks();
        for (i, id) in ids.iter().copied().enumerate() {
            let r = Rect::from_min_size(pos2(self.col_x(rect, i), rect.top()), vec2(COL_W - 1.0, HEADER_H));
            self.track_header(ui, r, id);
        }
        let n = ids.len();
        for k in 0..2 {
            let r = Rect::from_min_size(pos2(self.col_x(rect, n + k), rect.top()), vec2(COL_W - 1.0, HEADER_H));
            let rt = &self.project.returns[k];
            let col = color32(rt.color);
            let selected = self.sel == Sel::Return(k);
            ui.painter().rect_filled(r, 0.0, if selected { col } else { darken(col, 0.75) });
            ui.painter().text(r.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, &rt.name, FontId::proportional(12.0), on_color(col));
            let resp = ui.interact(r, ui.id().with(("ret_hdr", k)), Sense::click());
            if resp.clicked() {
                self.sel = Sel::Return(k);
                self.detail = Detail::Devices;
            }
            if let Some(Payload::Device(kind)) = self.drop_target(ui, r, |p| matches!(p, Payload::Device(_))) {
                self.add_device(Sel::Return(k), kind, None);
            }
            self.hint(&resp, "Return Track", "Return tracks receive signal from the Send knobs of every track. Click to edit its effects.");
        }
        let r = Rect::from_min_size(pos2(self.master_x(rect), rect.top()), vec2(MASTER_W, HEADER_H));
        let col = Color32::from_rgb(0xff, 0x66, 0xc4);
        ui.painter().rect_filled(r, 0.0, if self.sel == Sel::Master { col } else { darken(col, 0.75) });
        ui.painter().text(r.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, "Master", FontId::proportional(12.0), TEXT_DARK);
        let resp = ui.interact(r, ui.id().with("master_hdr"), Sense::click());
        if resp.clicked() {
            self.sel = Sel::Master;
            self.detail = Detail::Devices;
        }
        if let Some(Payload::Device(kind)) = self.drop_target(ui, r, |p| matches!(p, Payload::Device(_))) {
            self.add_device(Sel::Master, kind, None);
        }
        self.hint(&resp, "Master Track", "The final mix bus. Click to edit the master effects (compressor, limiter).");
    }

    pub(super) fn track_header(&mut self, ui: &mut egui::Ui, r: Rect, id: u64) {
        let Some(t) = self.project.track(id) else { return };
        let col = color32(t.color);
        let selected = self.sel == Sel::Track(id);
        let name = t.name.clone();
        let idx = self.project.track_index(id).unwrap_or(0);
        ui.painter().rect_filled(r, 0.0, if selected { col } else { darken(col, 0.72) });
        let is_group = t.instrument.kind == InstrumentKind::Group;
        let folded = t.folded;
        let group_color = t.group.and_then(|g| self.project.track(g)).map(|g| color32(g.color));
        let txt = if is_group { format!("{} {name}", if folded { "▶" } else { "▼" }) } else { format!("{} {}", idx + 1, name) };
        if is_group {
            let tri = Rect::from_min_size(r.min, vec2(18.0, r.height()));
            let tr = ui.interact(tri, ui.id().with(("fold", id)), Sense::click());
            if tr.clicked() {
                if let Some(t) = self.project.track_mut(id) {
                    t.folded = !t.folded;
                }
                self.touch();
            }
            self.hint(&tr, "Fold Group", "Shows or hides the tracks inside this group.");
        }
        if let Some(gc) = group_color {
            ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 3.0)), 0.0, gc);
        }
        let resp = ui.interact(r.with_min_x(if is_group { r.left() + 18.0 } else { r.left() }), ui.id().with(("trk_hdr", id)), Sense::click());
        if self.renaming.as_ref().is_some_and(|(t, _)| *t == RenameTarget::Track(id)) {
            self.rename_field(ui, r);
        } else {
            ui.painter().with_clip_rect(r).text(r.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, txt, FontId::proportional(12.0), on_color(col));
        }
        if resp.clicked() {
            self.select_track(id);
            self.detail = Detail::Devices;
        }
        if resp.double_clicked() {
            self.renaming = Some((RenameTarget::Track(id), name.clone()));
        }
        resp.context_menu(|ui| self.track_menu(ui, id));
        if let Some(p) = self.drop_target(ui, r, |p| matches!(p, Payload::Device(_) | Payload::Instrument(_) | Payload::MidiFx(_))) {
            self.drop_on_track(id, p);
        }
        self.hint(&resp, &name, "Track title bar. Click to select and show its devices, double-click to rename, right-click for options. Drop instruments or effects here.");
    }

    /// Handles devices, MIDI effects and instruments dropped anywhere on a track.
    pub(super) fn drop_on_track(&mut self, id: u64, payload: Payload) {
        match payload {
            Payload::Device(kind) => self.add_device(Sel::Track(id), kind, None),
            Payload::MidiFx(kind) => self.add_midi_fx(id, kind, None),
            Payload::Instrument(preset) => {
                let pr = crate::presets::preset(preset);
                if let Some(t) = self.project.track_mut(id) {
                    if t.instrument.kind != pr.kind {
                        // Different instrument type: clips would not fit, so add a new track instead.
                        let at = self.project.track_index(id).map(|i| i + 1);
                        self.add_track_from_preset(preset, at);
                    } else {
                        let samples = t.instrument.samples.clone();
                        t.instrument = pr.build();
                        t.instrument.samples = samples;
                        self.select_track(id);
                        self.detail = Detail::Devices;
                        self.touch();
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn track_menu(&mut self, ui: &mut egui::Ui, id: u64) {
        if ui.button("Rename").clicked() {
            let name = self.project.track(id).map(|t| t.name.clone()).unwrap_or_default();
            self.renaming = Some((RenameTarget::Track(id), name));
            ui.close_menu();
        }
        if ui.button("Duplicate Track").clicked() {
            self.duplicate_track(id);
            ui.close_menu();
        }
        let (is_group, in_group) = self.project.track(id).map(|t| (t.instrument.kind == InstrumentKind::Group, t.group.is_some())).unwrap_or((false, false));
        if is_group {
            if ui.button("Ungroup Tracks").clicked() {
                self.ungroup(id);
                ui.close_menu();
            }
        } else if in_group {
            if ui.button("Remove from Group").clicked() {
                if let Some(t) = self.project.track_mut(id) {
                    t.group = None;
                }
                self.touch();
                ui.close_menu();
            }
        } else {
            if ui.button("Group Track   (Ctrl+G)").clicked() {
                self.group_track(id);
                ui.close_menu();
            }
            let groups: Vec<(u64, String)> = self.project.tracks.iter().filter(|t| t.instrument.kind == InstrumentKind::Group).map(|t| (t.id, t.name.clone())).collect();
            if !groups.is_empty() {
                ui.menu_button("Move into Group", |ui| {
                    for (gid, gname) in groups {
                        if ui.button(gname).clicked() {
                            self.move_into_group(id, gid);
                            ui.close_menu();
                        }
                    }
                });
            }
        }
        if ui.button("Delete Track").clicked() {
            self.delete_track(id);
            ui.close_menu();
        }
        ui.separator();
        ui.label("Color");
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(8.0 * 18.0);
            for c in CLIP_COLORS {
                if widgets::color_swatch(ui, color32(c), false).clicked() {
                    if let Some(t) = self.project.track_mut(id) {
                        t.color = c;
                        for clip in t.clips.iter_mut().flatten() {
                            clip.color = c;
                        }
                        for ac in &mut t.arrangement {
                            ac.clip.color = c;
                        }
                    }
                    self.touch();
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        self.add_track_menu_items(ui);
    }

    fn add_track_menu_items(&mut self, ui: &mut egui::Ui) {
        for (label, preset) in [("Add Drum Track", "909 Punch"), ("Add Bass Track", "TB Acid"), ("Add Synth Track", "Dub Stab"), ("Add Sampler Track", "Sampler")] {
            if ui.button(label).clicked() {
                self.add_track_from_preset(preset, None);
                ui.close_menu();
            }
        }
        if ui.button("Add Audio Track").clicked() {
            self.add_audio_track(None);
            ui.close_menu();
        }
    }

    pub(super) fn rename_field(&mut self, ui: &mut egui::Ui, r: Rect) {
        let Some((target, mut text)) = self.renaming.clone() else { return };
        let resp = ui.put(r, egui::TextEdit::singleline(&mut text).font(FontId::proportional(12.0)));
        resp.request_focus();
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            self.renaming = None;
            return;
        }
        if enter || resp.lost_focus() {
            self.renaming = None;
            match target {
                RenameTarget::Track(id) => {
                    if let Some(t) = self.project.track_mut(id) {
                        t.name = text;
                    }
                }
                RenameTarget::Scene(s) => {
                    if let Some(sc) = self.project.scenes.get_mut(s) {
                        sc.name = text;
                    }
                }
                RenameTarget::Clip(c) => {
                    if let Some(clip) = self.clip_mut(c) {
                        clip.name = text;
                    }
                }
            }
            self.touch();
        } else {
            self.renaming = Some((target, text));
        }
    }

    fn session_grid(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, PANEL_DARK);
        let n_scenes = self.project.scenes.len();
        let ids: Vec<u64> = self.visible_tracks();
        let blink = (ui.input(|i| i.time) * 4.0) as i64 % 2 == 0;
        for (ti, &id) in ids.iter().enumerate() {
            let x = self.col_x(rect, ti);
            for si in 0..n_scenes {
                let r = Rect::from_min_size(pos2(x, rect.top() + si as f32 * SLOT_H), vec2(COL_W - 1.0, SLOT_H - 1.0));
                self.clip_slot(ui, r, id, si, blink);
            }
            // The empty area below the slots belongs to the track too.
            let below = Rect::from_min_max(pos2(x, rect.top() + n_scenes as f32 * SLOT_H), pos2(x + COL_W - 1.0, rect.bottom()));
            if let Some(p) = self.drop_target(ui, below, |p| matches!(p, Payload::Device(_) | Payload::MidiFx(_) | Payload::Instrument(_))) {
                self.drop_on_track(id, p);
            }
        }
        // Drop area for new tracks.
        let n = ids.len();
        let dx = rect.left() + n as f32 * COL_W;
        let drop = Rect::from_min_size(pos2(dx, rect.top()), vec2(DROP_W - 1.0, rect.height()));
        p.text(drop.center_top() + vec2(0.0, 60.0), Align2::CENTER_CENTER, "Drop Instruments\nand Clips Here", FontId::proportional(12.0), TEXT_DIM);
        let resp = ui.interact(drop, ui.id().with("drop_area"), Sense::click());
        resp.context_menu(|ui| self.add_track_menu_items(ui));
        if resp.double_clicked() {
            self.add_track_from_preset("Dub Stab", None);
        }
        self.hint(&resp, "Add Tracks", "Drop an instrument here to create a new track, a clip to create a track that fits it, or a WAV sample to create an audio track. Right-click to add tracks.");
        match self.drop_target(ui, drop, |p| matches!(p, Payload::Instrument(_) | Payload::Pattern(_) | Payload::Sample(_))) {
            Some(Payload::Sample(path)) => {
                let id = self.add_audio_track(None);
                let pos = ui.ctx().input(|i| i.pointer.latest_pos()).unwrap_or(rect.min);
                let scene = (((pos.y - rect.top()) / SLOT_H) as usize).min(n_scenes - 1);
                self.drop_sample_on_slot(id, scene, &path);
            }
            Some(Payload::Instrument(preset)) => {
                self.add_track_from_preset(preset, None);
            }
            Some(Payload::Pattern(name)) => {
                if let Some(pat) = crate::presets::pattern(name) {
                    let preset = crate::presets::INSTRUMENT_PRESETS.iter().find(|p| p.kind == pat.kind).unwrap().name;
                    let id = self.add_track_from_preset(preset, None);
                    let pos = ui.ctx().input(|i| i.pointer.latest_pos()).unwrap_or(rect.min);
                    let scene = (((pos.y - rect.top()) / SLOT_H) as usize).min(n_scenes - 1);
                    self.place_pattern(id, scene, name);
                }
            }
            _ => {}
        }
        // Return columns are empty in the clip grid.
        for k in 0..2 {
            let x = self.col_x(rect, n + k);
            for si in 0..n_scenes {
                let r = Rect::from_min_size(pos2(x, rect.top() + si as f32 * SLOT_H), vec2(COL_W - 1.0, SLOT_H - 1.0));
                p.rect_filled(r, 0.0, PANEL_DARK);
            }
        }
        // Scene launchers in the master column.
        let mx = self.master_x(rect);
        for si in 0..n_scenes {
            let r = Rect::from_min_size(pos2(mx, rect.top() + si as f32 * SLOT_H), vec2(MASTER_W, SLOT_H - 1.0));
            self.scene_slot(ui, r, si);
        }
        let r = Rect::from_min_size(pos2(mx, rect.top() + n_scenes as f32 * SLOT_H), vec2(MASTER_W, SLOT_H - 1.0));
        let resp = ui.interact(r, ui.id().with("add_scene"), Sense::click());
        p.text(r.center(), Align2::CENTER_CENTER, "+ Scene", FontId::proportional(11.0), if resp.hovered() { TEXT } else { TEXT_DIM });
        if resp.clicked() {
            self.project.add_scene(n_scenes);
            self.touch();
        }
        self.hint(&resp, "Insert Scene", "Adds a new empty scene (row) at the bottom.");
    }

    fn group_slot(&mut self, ui: &mut egui::Ui, r: Rect, group: u64, scene: usize) {
        let p = ui.painter().clone();
        let members: Vec<u64> = self.project.tracks.iter().filter(|t| t.group == Some(group)).map(|t| t.id).collect();
        let has = members.iter().any(|m| self.project.track(*m).is_some_and(|t| t.clips.get(scene).is_some_and(|c| c.is_some())));
        let playing = members.iter().any(|m| self.track_status(*m).is_some_and(|s| s.playing == Some(scene)));
        let col = self.project.track(group).map(|t| color32(t.color)).unwrap_or(TEXT_DIM);
        let resp = ui.interact(r, ui.id().with(("gslot", group, scene)), Sense::click());
        p.rect_filled(r, 0.0, if has { darken(col, 0.45) } else { SLOT });
        if has {
            let c0 = r.left_center() + vec2(9.0, 0.0);
            p.add(Shape::convex_polygon(vec![c0 + vec2(-4.0, -5.0), c0 + vec2(5.0, 0.0), c0 + vec2(-4.0, 5.0)], if playing { PLAY } else { TEXT }, Stroke::NONE));
        }
        if resp.clicked() && has {
            for m in members {
                let has_clip = self.project.track(m).is_some_and(|t| t.clips.get(scene).is_some_and(|c| c.is_some()));
                self.send(if has_clip { Cmd::Launch { track: m, scene } } else { Cmd::StopTrack(m) });
            }
        }
        self.hint(&resp, "Group Slot", "Launches the clips of all tracks in this group in this scene.");
    }

    fn clip_slot(&mut self, ui: &mut egui::Ui, r: Rect, track: u64, scene: usize, blink: bool) {
        if self.project.track(track).is_some_and(|t| t.instrument.kind == InstrumentKind::Group) {
            self.group_slot(ui, r, track, scene);
            return;
        }
        let p = ui.painter().clone();
        let Some(t) = self.project.track(track) else { return };
        let clip: Option<Clip> = t.clips.get(scene).cloned().flatten();
        let ts = self.track_status(track).cloned().unwrap_or_default();
        let playing = ts.playing == Some(scene);
        let queued = ts.queued == Some(Some(scene));
        let selected = self.sel_clip == Some(ClipRef::Session { track, scene }) || (clip.is_none() && self.sel == Sel::Track(track) && self.sel_scene == scene);
        let btn = Rect::from_min_size(r.min, vec2(18.0, r.height()));
        let resp = ui.interact(r, ui.id().with(("slot", track, scene)), Sense::click_and_drag());
        let hover_btn = resp.hover_pos().is_some_and(|pos| btn.contains(pos));
        match &clip {
            Some(c) => {
                let col = color32(c.color);
                p.rect_filled(r, 0.0, if playing { col } else { darken(col, 0.8) });
                let tri_col = if playing {
                    PLAY
                } else if queued && blink {
                    PLAY
                } else if hover_btn {
                    Color32::WHITE
                } else {
                    on_color(col)
                };
                let c0 = btn.center();
                p.add(Shape::convex_polygon(vec![c0 + vec2(-4.0, -5.0), c0 + vec2(5.0, 0.0), c0 + vec2(-4.0, 5.0)], tri_col, Stroke::NONE));
                if self.renaming.as_ref().is_some_and(|(t, _)| *t == RenameTarget::Clip(ClipRef::Session { track, scene })) {
                    self.rename_field(ui, r.with_min_x(btn.right()));
                } else {
                    p.with_clip_rect(r.shrink(1.0)).text(
                        pos2(btn.right() + 1.0, r.center().y),
                        Align2::LEFT_CENTER,
                        &c.name,
                        FontId::proportional(11.5),
                        on_color(col),
                    );
                }
                if playing {
                    let frac = ts.clip_pos / c.len.max(1) as f32;
                    let bar = Rect::from_min_size(pos2(r.left(), r.bottom() - 3.0), vec2(r.width() * frac, 3.0));
                    p.rect_filled(bar, 0.0, Color32::from_black_alpha(120));
                }
            }
            None => {
                p.rect_filled(r, 0.0, SLOT);
                let c0 = btn.center();
                let sq = Rect::from_center_size(c0, vec2(7.0, 7.0));
                let col = if queued && blink { PLAY } else if hover_btn { TEXT } else { Color32::from_rgb(0x55, 0x55, 0x55) };
                p.rect_filled(sq, 0.0, col);
            }
        }
        if selected {
            p.rect_stroke(r, 0.0, Stroke::new(1.5_f32, Color32::WHITE), egui::StrokeKind::Inside);
        }

        if resp.clicked() {
            if hover_btn {
                if clip.is_some() {
                    self.send(Cmd::Launch { track, scene });
                } else {
                    self.send(Cmd::StopTrack(track));
                }
            } else if clip.is_some() {
                self.select_clip(ClipRef::Session { track, scene });
            } else {
                self.select_track(track);
                self.sel_scene = scene;
                self.sel_clip = None;
            }
        }
        if resp.double_clicked() && !hover_btn {
            if clip.is_none() {
                self.new_empty_clip(track, scene);
            } else {
                self.select_clip(ClipRef::Session { track, scene });
                self.renaming = None;
            }
        }
        if resp.drag_started() && clip.is_some() && !hover_btn {
            self.drag = Some(Payload::SessionClip { track, scene });
        }
        let has_clip = clip.is_some();
        resp.context_menu(|ui| {
            if has_clip {
                if ui.button("Launch").clicked() {
                    self.send(Cmd::Launch { track, scene });
                    ui.close_menu();
                }
                if ui.button("Rename").clicked() {
                    let name = self.clip(ClipRef::Session { track, scene }).map(|c| c.name.clone()).unwrap_or_default();
                    self.renaming = Some((RenameTarget::Clip(ClipRef::Session { track, scene }), name));
                    ui.close_menu();
                }
                if ui.button("Duplicate   (Ctrl+D)").clicked() {
                    self.sel_clip = Some(ClipRef::Session { track, scene });
                    self.duplicate_selected_clip();
                    ui.close_menu();
                }
                if ui.button("Delete   (Del)").clicked() {
                    self.sel_clip = Some(ClipRef::Session { track, scene });
                    self.delete_selected_clip();
                    ui.close_menu();
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(8.0 * 18.0);
                    for c in CLIP_COLORS {
                        if widgets::color_swatch(ui, color32(c), false).clicked() {
                            if let Some(clip) = self.clip_mut(ClipRef::Session { track, scene }) {
                                clip.color = c;
                            }
                            self.touch();
                            ui.close_menu();
                        }
                    }
                });
            } else if ui.button("Insert MIDI Clip").clicked() {
                self.new_empty_clip(track, scene);
                ui.close_menu();
            }
            ui.separator();
            if has_clip {
                self.map_menu(ui, MapTarget::LaunchClip(track, scene), None);
            } else {
                self.map_menu(ui, MapTarget::StopTrack(track), None);
            }
        });
        match self.drop_target(ui, r, |p| !matches!(p, Payload::SessionClip { track: t, scene: s } if *t == track && *s == scene)) {
            Some(p @ (Payload::Device(_) | Payload::MidiFx(_) | Payload::Instrument(_))) => self.drop_on_track(track, p),
            Some(Payload::Pattern(name)) => self.place_pattern(track, scene, name),
            Some(Payload::Sample(path)) => self.drop_sample_on_slot(track, scene, &path),
            Some(Payload::SessionClip { track: st, scene: ss }) => {
                if (st, ss) != (track, scene) {
                    if let Some(c) = self.clip(ClipRef::Session { track: st, scene: ss }).cloned() {
                        let copy = ui.input(|i| i.modifiers.command || i.modifiers.alt);
                        if !copy {
                            if let Some(t) = self.project.track_mut(st) {
                                t.clips[ss] = None;
                            }
                        }
                        self.create_clip(track, scene, c);
                    }
                }
            }
            _ => {}
        }
        if resp.hovered() {
            if clip.is_some() {
                self.info = Some((
                    "Clip".into(),
                    "Click the triangle to launch (quantized to the global quantization). Click the clip to edit it below, drag to move it (Ctrl: copy), drag into the Timeline to place it on the timeline. Right-click for options.".into(),
                ));
            } else {
                self.info = Some((
                    "Clip Slot".into(),
                    "Empty slot. Double-click to create a MIDI clip, drop a clip from the browser, or click the square to stop this track.".into(),
                ));
            }
        }
    }

    fn scene_slot(&mut self, ui: &mut egui::Ui, r: Rect, scene: usize) {
        let p = ui.painter().clone();
        let resp = ui.interact(r, ui.id().with(("scene", scene)), Sense::click());
        let active = self.status.tracks.iter().any(|t| t.playing == Some(scene));
        p.rect_filled(r, 0.0, if resp.hovered() { PANEL_LIGHT } else { PANEL });
        let c0 = r.left_center() + vec2(10.0, 0.0);
        p.add(Shape::convex_polygon(
            vec![c0 + vec2(-4.0, -5.0), c0 + vec2(5.0, 0.0), c0 + vec2(-4.0, 5.0)],
            if active { PLAY } else { TEXT_DIM },
            Stroke::NONE,
        ));
        if self.renaming.as_ref().is_some_and(|(t, _)| *t == RenameTarget::Scene(scene)) {
            self.rename_field(ui, r.with_min_x(r.left() + 20.0));
        } else {
            p.text(r.left_center() + vec2(22.0, 0.0), Align2::LEFT_CENTER, &self.project.scenes[scene].name, FontId::proportional(12.0), TEXT);
        }
        if resp.clicked() {
            self.send(Cmd::LaunchScene(scene));
            self.sel_scene = scene;
        }
        if resp.double_clicked() {
            self.renaming = Some((RenameTarget::Scene(scene), self.project.scenes[scene].name.clone()));
        }
        resp.context_menu(|ui| {
            if ui.button("Launch Scene").clicked() {
                self.send(Cmd::LaunchScene(scene));
                ui.close_menu();
            }
            if ui.button("Rename").clicked() {
                self.renaming = Some((RenameTarget::Scene(scene), self.project.scenes[scene].name.clone()));
                ui.close_menu();
            }
            if ui.button("Insert Scene").clicked() {
                self.project.add_scene(scene + 1);
                self.touch();
                ui.close_menu();
            }
            if ui.button("Duplicate Scene").clicked() {
                self.project.add_scene(scene + 1);
                self.project.scenes[scene + 1].name = format!("{} Copy", self.project.scenes[scene].name);
                for t in &mut self.project.tracks {
                    t.clips[scene + 1] = t.clips[scene].clone();
                }
                self.touch();
                ui.close_menu();
            }
            if ui.button("Delete Scene").clicked() {
                self.project.remove_scene(scene);
                self.validate_selection();
                self.touch();
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Copy Scenes to Timeline").clicked() {
                self.scenes_to_arrangement();
                ui.close_menu();
            }
            ui.separator();
            self.map_menu(ui, MapTarget::LaunchScene(scene), None);
        });
        self.hint(&resp, "Scene", "Click to launch every clip in this row at once (empty slots stop their tracks). Double-click to rename, right-click for options.");
    }

    pub(super) fn scenes_to_arrangement(&mut self) {
        let end_bar = self.project.arrangement_end().div_ceil(STEPS_PER_BAR);
        let bars: Vec<u32> = (0..self.project.scenes.len())
            .map(|s| {
                let max = self.project.tracks.iter().filter_map(|t| t.clips[s].as_ref()).map(|c| c.bars()).max().unwrap_or(0);
                if max == 0 { 0 } else { max.max(8) }
            })
            .collect();
        // Skip empty scenes by giving them zero length.
        crate::presets::scenes_to_arrangement(&mut self.project, &bars, end_bar);
        self.touch();
        self.toast("Scenes copied to the Timeline");
    }

    fn session_stop_row(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let ids: Vec<u64> = self.visible_tracks();
        for (i, id) in ids.iter().copied().enumerate() {
            let r = Rect::from_min_size(pos2(self.col_x(rect, i), rect.top()), vec2(COL_W - 1.0, SLOT_H));
            let resp = ui.interact(r, ui.id().with(("stop", id)), Sense::click());
            p.rect_filled(r, 0.0, PANEL);
            let playing = self.track_status(id).is_some_and(|t| t.playing.is_some());
            p.rect_filled(Rect::from_center_size(r.center(), vec2(9.0, 9.0)), 0.0, if resp.hovered() { TEXT } else if playing { TEXT_DIM } else { Color32::from_rgb(0x55, 0x55, 0x55) });
            if resp.clicked() {
                self.send(Cmd::StopTrack(id));
            }
            self.hint(&resp, "Clip Stop Button", "Stops the clip playing on this track (quantized).");
        }
        let r = Rect::from_min_size(pos2(self.master_x(rect), rect.top()), vec2(MASTER_W, SLOT_H));
        let resp = ui.interact(r, ui.id().with("stop_all"), Sense::click());
        p.rect_filled(r, 0.0, if resp.hovered() { PANEL_LIGHT } else { PANEL });
        p.text(r.center(), Align2::CENTER_CENTER, "■ Stop All Clips", FontId::proportional(11.0), TEXT);
        if resp.clicked() {
            self.send(Cmd::StopAll);
        }
        resp.context_menu(|ui| self.map_menu(ui, MapTarget::StopAll, None));
        self.hint(&resp, "Stop All Clips", "Stops every playing Launcher clip.");
    }

    fn session_mixer(&mut self, ui: &mut egui::Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, PANEL);
        let ids: Vec<u64> = self.visible_tracks();
        for (i, id) in ids.iter().copied().enumerate() {
            let r = Rect::from_min_size(pos2(self.col_x(rect, i), rect.top()), vec2(COL_W - 1.0, rect.height()));
            ui.painter().line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0_f32, LINE));
            self.mixer_strip(ui, r, Sel::Track(id), i + 1);
        }
        let n = ids.len();
        for k in 0..2 {
            let r = Rect::from_min_size(pos2(self.col_x(rect, n + k), rect.top()), vec2(COL_W - 1.0, rect.height()));
            self.mixer_strip(ui, r, Sel::Return(k), k);
        }
        let r = Rect::from_min_size(pos2(self.master_x(rect), rect.top()), vec2(MASTER_W, rect.height()));
        self.mixer_strip(ui, r, Sel::Master, 0);
    }

    fn mixer_strip(&mut self, ui: &mut egui::Ui, r: Rect, sel: Sel, number: usize) {
        let armed_audio = match sel {
            Sel::Track(id) => self.project.track(id).is_some_and(|t| t.arm && t.instrument.kind == InstrumentKind::Audio),
            _ => false,
        };
        let is_audio = match sel {
            Sel::Track(id) => self.project.track(id).is_some_and(|t| t.instrument.kind == InstrumentKind::Audio),
            _ => false,
        };
        let mut arm_clicked = false;
        let meter = match sel {
            Sel::Track(_) if armed_audio => [self.input_level(); 2],
            Sel::Track(id) => self.meter(id),
            Sel::Return(k) => self.meter(RETURN_METER + k as u64),
            Sel::Master => self.meter(MASTER_METER),
        };
        let any_solo = self.project.any_solo();
        let mut ch = match sel {
            Sel::Track(id) => self.project.track(id).map(|t| t.channel.clone()),
            Sel::Return(k) => self.project.returns.get(k).map(|r| r.channel.clone()),
            Sel::Master => Some(self.project.master.clone()),
        }
        .unwrap();
        let before = (ch.volume, ch.pan, ch.mute, ch.solo, ch.sends, ch.xfade);
        let mut xf = self.project.crossfader;
        let mut select = false;
        let strip = ui.interact(r, ui.id().with(("strip", format!("{sel:?}"))), Sense::click());
        ui.scope_builder(UiBuilder::new().max_rect(r.shrink2(vec2(4.0, 4.0))), |ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            ui.vertical(|ui| {
                if let Sel::Track(_) = sel {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Sends").size(10.0).color(TEXT_DIM));
                    });
                    ui.horizontal(|ui| {
                        let r1 = widgets::send_knob(ui, &mut ch.sends[0], "A");
                        let r2 = widgets::send_knob(ui, &mut ch.sends[1], "B");
                        if let Sel::Track(id) = sel {
                            r1.context_menu(|ui| self.map_menu(ui, MapTarget::Send(id, 0), Some(id)));
                            r2.context_menu(|ui| self.map_menu(ui, MapTarget::Send(id, 1), Some(id)));
                        }
                        self.hint(&r1, "Send A", "How much of this track goes to Return A (reverb by default).");
                        self.hint(&r2, "Send B", "How much of this track goes to Return B (delay by default).");
                    });
                } else {
                    ui.add_space(48.0);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if sel != Sel::Master {
                        let rp = widgets::pan_knob(ui, &mut ch.pan);
                        if let Sel::Track(id) = sel {
                            rp.context_menu(|ui| self.map_menu(ui, MapTarget::Pan(id), Some(id)));
                        }
                        self.hint(&rp, "Pan", "Stereo position. Double-click to center.");
                    } else {
                        // Crossfader in the master strip.
                        let (rect, resp) = ui.allocate_exact_size(vec2(60.0, 16.0), Sense::click_and_drag());
                        let p = ui.painter();
                        p.rect_filled(rect.shrink2(vec2(0.0, 6.0)), 2.0, KNOB_BG);
                        let x = rect.left() + (xf + 1.0) / 2.0 * rect.width();
                        p.rect_filled(Rect::from_center_size(pos2(x, rect.center().y), vec2(6.0, 14.0)), 1.0, if resp.dragged() { ACCENT } else { TEXT });
                        p.text(rect.left_top(), Align2::LEFT_BOTTOM, "A", FontId::proportional(8.0), TEXT_DIM);
                        p.text(rect.right_top(), Align2::RIGHT_BOTTOM, "B", FontId::proportional(8.0), TEXT_DIM);
                        if resp.dragged() {
                            xf = (xf + resp.drag_delta().x / rect.width() * 2.0).clamp(-1.0, 1.0);
                        }
                        if resp.double_clicked() {
                            xf = 0.0;
                        }
                        resp.context_menu(|ui| self.map_menu(ui, MapTarget::Crossfader, None));
                        self.hint(&resp, "Crossfader", "Fades between tracks assigned to A and B (A/B buttons in each track strip). Double-click to center. Right-click to map it to a MIDI fader.");
                    }
                    let db = crate::audio::dsp::gain_to_db(crate::audio::dsp::fader_to_gain(ch.volume));
                    ui.label(egui::RichText::new(if db < -100.0 { "-inf".into() } else { format!("{db:.1}") }).size(10.5).monospace());
                });
                ui.horizontal(|ui| {
                    let rf = widgets::fader(ui, &mut ch.volume, meter, 110.0);
                    let vol_target = match sel {
                        Sel::Track(id) => MapTarget::Volume(id),
                        Sel::Return(k) => MapTarget::ReturnVolume(k as u8),
                        Sel::Master => MapTarget::MasterVolume,
                    };
                    let tid = if let Sel::Track(id) = sel { Some(id) } else { None };
                    rf.context_menu(|ui| self.map_menu(ui, vol_target, tid));
                    self.hint(&rf, "Volume", "Track volume. Drag (Shift: fine), double-click for 0 dB. The meter shows the output level.");
                    ui.vertical(|ui| {
                        if sel != Sel::Master {
                            let label = match sel {
                                Sel::Return(k) => ["A", "B"][k].to_string(),
                                _ => number.to_string(),
                            };
                            let ra = widgets::toggle(ui, &label, !ch.mute, ACCENT, vec2(30.0, 20.0));
                            if ra.clicked() {
                                ch.mute = !ch.mute;
                            }
                            if let Sel::Track(id) = sel {
                                ra.context_menu(|ui| self.map_menu(ui, MapTarget::Mute(id), None));
                            }
                            self.hint(&ra, "Track Activator", "Turns the track on/off (mute).");
                            let soloed_out = any_solo && !ch.solo && matches!(sel, Sel::Track(_));
                            let rs = widgets::toggle(ui, "S", ch.solo, BLUE, vec2(30.0, 20.0));
                            if let Sel::Track(id) = sel {
                                rs.context_menu(|ui| self.map_menu(ui, MapTarget::Solo(id), None));
                            }
                            if rs.clicked() {
                                let exclusive = !ui.input(|i| i.modifiers.command);
                                ch.solo = !ch.solo;
                                if exclusive && ch.solo {
                                    select = true;
                                }
                            }
                            self.hint(&rs, "Solo", "Plays only soloed tracks. Ctrl-click to solo several tracks.");
                            if soloed_out {
                                ui.label(egui::RichText::new("muted").size(9.0).color(TEXT_DIM));
                            }
                            if is_audio {
                                let rr = widgets::toggle(ui, "●", armed_audio, REC, vec2(30.0, 18.0));
                                if rr.clicked() {
                                    arm_clicked = true;
                                }
                                self.hint(&rr, "Arm Recording", "Arms this audio track. With Record on, playback records the audio input (microphone / interface) into a new clip. The meter shows the input level while armed.");
                            }
                            if let Sel::Track(_) = sel {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    for (k, l) in [(1u8, "A"), (2u8, "B")] {
                                        let rx = widgets::toggle(ui, l, ch.xfade == k, Color32::from_rgb(0xff, 0x9c, 0x40), vec2(14.0, 14.0));
                                        if rx.clicked() {
                                            ch.xfade = if ch.xfade == k { 0 } else { k };
                                        }
                                        self.hint(&rx, "Crossfader Assign", "Assigns the track to side A or B of the crossfader (in the Master strip).");
                                    }
                                });
                            }
                        }
                    });
                });
            });
        });
        let after = (ch.volume, ch.pan, ch.mute, ch.solo, ch.sends, ch.xfade);
        if xf != self.project.crossfader {
            self.project.crossfader = xf;
            self.touch();
        }
        if before != after {
            if select && ch.solo {
                // Exclusive solo: clear the others.
                for t in &mut self.project.tracks {
                    t.channel.solo = false;
                }
                for rt in &mut self.project.returns {
                    rt.channel.solo = false;
                }
            }
            if let Some(c) = self.channel_mut(sel) {
                *c = ch;
            }
            self.touch();
        }
        match self.drop_target(ui, r, |p| matches!(p, Payload::Device(_) | Payload::MidiFx(_) | Payload::Instrument(_))) {
            Some(Payload::Device(kind)) => self.add_device(sel, kind, None),
            Some(p) => {
                if let Sel::Track(id) = sel {
                    self.drop_on_track(id, p);
                }
            }
            None => {}
        }
        if arm_clicked {
            if let Sel::Track(id) = sel {
                self.toggle_arm(id);
            }
        }
        if strip.clicked() {
            self.sel = sel;
            self.detail = Detail::Devices;
        }
    }
}
