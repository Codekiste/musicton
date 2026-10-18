//! Timeline: timeline with clips, loop brace and automation lanes.

use super::theme::*;
use super::arranger::{ArrangerState, CHORD_H, MARK_H, SEC_H};
use super::widgets;
use super::{App, ClipRef, Detail, Payload, Sel};
use crate::audio::engine::Cmd;
use crate::model::*;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, UiBuilder};
use std::collections::HashMap;

const HEADER_W: f32 = 176.0;
const RULER_H: f32 = 32.0;
const LANE_H: f32 = 54.0;
const AUTO_H: f32 = 56.0;

#[derive(Clone, Copy, Debug)]
enum ArrDrag {
    Move { track: u64, id: u64, grab: f32 },
    Resize { track: u64, id: u64 },
    LoopMove { grab: f32 },
    LoopStart,
    LoopEnd,
    Point { track: u64, auto: usize, point: usize },
}

pub struct ArrState {
    pub pps: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    drag: Option<ArrDrag>,
    pub targets: HashMap<u64, AutoTarget>,
    pub follow: bool,
    pub ext: ArrangerState,
}

impl Default for ArrState {
    fn default() -> Self {
        Self { pps: 4.0, scroll_x: 0.0, scroll_y: 0.0, drag: None, targets: HashMap::new(), follow: true, ext: ArrangerState::default() }
    }
}

pub fn auto_targets(p: &Project, t: &Track) -> Vec<(AutoTarget, String)> {
    let mut v = vec![
        (AutoTarget::Volume, "Mixer: Volume".to_string()),
        (AutoTarget::Pan, "Mixer: Pan".to_string()),
        (AutoTarget::Send(0), format!("Mixer: Send {}", p.returns[0].name)),
        (AutoTarget::Send(1), format!("Mixer: Send {}", p.returns[1].name)),
    ];
    let defs = t.instrument.kind.defs();
    let limit = if t.instrument.kind == InstrumentKind::Drums { 2 } else { defs.len() };
    for (i, d) in defs.iter().enumerate().take(limit) {
        if !d.is_choice() {
            v.push((AutoTarget::Instrument(i), format!("{}: {}", t.instrument.kind.name(), d.name)));
        }
    }
    for dev in &t.channel.devices {
        for (i, d) in dev.kind.defs().iter().enumerate() {
            if !d.is_choice() {
                v.push((AutoTarget::Device { id: dev.id, param: i }, format!("{}: {}", dev.kind.name(), d.name)));
            }
        }
    }
    v
}

/// Current static value of an automation target, normalized.
fn base_norm(t: &Track, target: AutoTarget) -> f32 {
    match target {
        AutoTarget::Volume => t.channel.volume,
        AutoTarget::Pan => (t.channel.pan + 1.0) / 2.0,
        AutoTarget::Send(k) => t.channel.sends[(k as usize).min(1)],
        AutoTarget::Instrument(i) => t.instrument.kind.defs().get(i).map(|d| d.to_norm(t.instrument.params[i])).unwrap_or(0.5),
        AutoTarget::Device { id, param } => t
            .channel
            .devices
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| d.kind.defs().get(param).map(|def| def.to_norm(d.params[param])))
            .unwrap_or(0.5),
    }
}

impl App {
    pub(super) fn snap(&self, step: f32, alt: bool) -> u32 {
        let grid = if alt {
            1.0
        } else if self.arr.pps < 2.5 {
            16.0
        } else {
            4.0
        };
        ((step / grid).round() * grid).max(0.0) as u32
    }

    pub(super) fn snap_floor(&self, step: f32, alt: bool) -> u32 {
        let grid = if alt {
            1.0
        } else if self.arr.pps < 2.5 {
            16.0
        } else {
            4.0
        };
        ((step / grid).floor() * grid).max(0.0) as u32
    }

    pub(super) fn arrangement_view(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let ruler = Rect::from_min_max(pos2(full.left() + HEADER_W, full.top()), pos2(full.right(), full.top() + RULER_H));
        let top_h = RULER_H + MARK_H + SEC_H + CHORD_H;
        let marks = Rect::from_min_max(pos2(ruler.left(), ruler.bottom()), pos2(ruler.right(), ruler.bottom() + MARK_H));
        let secs = Rect::from_min_max(pos2(ruler.left(), marks.bottom()), pos2(ruler.right(), marks.bottom() + SEC_H));
        let chords = Rect::from_min_max(pos2(ruler.left(), secs.bottom()), pos2(ruler.right(), secs.bottom() + CHORD_H));
        let lanes = Rect::from_min_max(pos2(full.left() + HEADER_W, full.top() + top_h), full.max);
        let headers = Rect::from_min_max(pos2(full.left(), full.top() + top_h), pos2(full.left() + HEADER_W, full.bottom()));
        let pps = self.arr.pps;
        let x_of = |s: f32, sx: f32| lanes.left() + (s - sx) * pps;

        // Scrolling and zooming.
        if ui.rect_contains_pointer(full) {
            let (delta, zoom, mods, pos) = ui.input(|i| (i.raw_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.latest_pos()));
            if zoom != 1.0 {
                let anchor = pos.map(|p| (p.x - lanes.left()) / pps + self.arr.scroll_x).unwrap_or(self.arr.scroll_x);
                self.arr.pps = (pps * zoom).clamp(0.5, 40.0);
                if let Some(p) = pos {
                    self.arr.scroll_x = (anchor - (p.x - lanes.left()) / self.arr.pps).max(0.0);
                }
            } else if mods.shift || delta.x != 0.0 {
                let d = if delta.x != 0.0 { delta.x } else { delta.y };
                self.arr.scroll_x = (self.arr.scroll_x - d / pps).max(0.0);
                self.arr.follow = false;
            } else if delta.y != 0.0 {
                self.arr.scroll_y = (self.arr.scroll_y - delta.y).max(0.0);
            }
        }
        // Follow the playhead.
        if self.status.playing && self.arr.follow {
            let visible = lanes.width() / self.arr.pps;
            let pos = self.status.arr_pos;
            if pos > self.arr.scroll_x + visible * 0.85 || pos < self.arr.scroll_x {
                self.arr.scroll_x = (pos - visible * 0.1).max(0.0);
            }
        }
        let pps = self.arr.pps;
        let sx = self.arr.scroll_x;
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, PANEL_DARK);

        // Rows.
        let ids: Vec<u64> = self.project.tracks.iter().map(|t| t.id).collect();
        let mut rows = vec![];
        let mut y = lanes.top() - self.arr.scroll_y;
        for &id in &ids {
            rows.push((id, false, y));
            y += LANE_H;
            if self.auto_open.contains(&id) {
                rows.push((id, true, y));
                y += AUTO_H;
            }
        }
        let content_bottom = y;
        let max_scroll = (content_bottom + self.arr.scroll_y - lanes.bottom() + 40.0).max(0.0);
        self.arr.scroll_y = self.arr.scroll_y.min(max_scroll);

        // Grid.
        let lp = p.with_clip_rect(lanes);
        let first_bar = (sx / 16.0).floor() as i64;
        let last_bar = ((sx + lanes.width() / pps) / 16.0).ceil() as i64;
        for bar in first_bar..=last_bar {
            let x = x_of(bar as f32 * 16.0, sx);
            lp.line_segment([pos2(x, lanes.top()), pos2(x, lanes.bottom())], Stroke::new(1.0_f32, if bar % 4 == 0 { GRID_STRONG } else { GRID }));
            if pps >= 4.0 {
                for b in 1..4 {
                    let xb = x + b as f32 * 4.0 * pps;
                    lp.line_segment([pos2(xb, lanes.top()), pos2(xb, lanes.bottom())], Stroke::new(1.0_f32, Color32::from_rgb(0x30, 0x30, 0x30)));
                }
            }
        }
        // Loop region shading.
        if self.project.loop_on {
            let lx0 = x_of(self.project.loop_start as f32, sx);
            let lx1 = x_of((self.project.loop_start + self.project.loop_len) as f32, sx);
            lp.rect_filled(Rect::from_x_y_ranges(lx0..=lx1, lanes.y_range()), 0.0, Color32::from_rgba_unmultiplied(255, 176, 31, 10));
        }

        if self.arr.ext.playlist {
            rows.clear();
        }
        // Lanes.
        for &(id, is_auto, y) in &rows {
            let row = Rect::from_min_size(pos2(lanes.left(), y), vec2(lanes.width(), if is_auto { AUTO_H } else { LANE_H }));
            if row.bottom() < lanes.top() || row.top() > lanes.bottom() {
                continue;
            }
            lp.line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0_f32, LINE));
            if is_auto {
                self.automation_lane(ui, row, lanes, id);
            } else {
                self.clip_lane(ui, row, lanes, id);
            }
        }

        // Headers (drawn on top of the lane area's left side).
        let hp = p.with_clip_rect(headers);
        hp.rect_filled(headers, 0.0, PANEL);
        for &(id, is_auto, y) in &rows {
            let r = Rect::from_min_size(pos2(headers.left(), y), vec2(HEADER_W - 2.0, if is_auto { AUTO_H } else { LANE_H } - 1.0));
            if r.bottom() < headers.top() || r.top() > headers.bottom() {
                continue;
            }
            ui.scope_builder(UiBuilder::new().max_rect(r.intersect(headers)), |ui| {
                ui.set_clip_rect(headers);
                if is_auto {
                    self.automation_header(ui, r, id);
                } else {
                    self.arr_track_header(ui, r, id);
                }
            });
        }
        if self.arr.ext.playlist {
            self.playlist_view(ui, lanes, headers, &x_of);
        }
        if ids.is_empty() && !self.arr.ext.playlist {
            p.text(lanes.center(), Align2::CENTER_CENTER, "Drop an instrument from the browser into the Launcher to create a track", FontId::proportional(13.0), TEXT_DIM);
        }

        // Ruler.
        self.ruler(ui, ruler, x_of);
        self.marker_strip(ui, marks, &x_of);
        self.section_lane(ui, secs, &x_of);
        self.chord_lane(ui, chords, &x_of);
        // Corner with tools.
        let corner = Rect::from_min_max(full.min, pos2(full.left() + HEADER_W, full.top() + top_h));
        p.rect_filled(corner, 0.0, PANEL);
        ui.scope_builder(UiBuilder::new().max_rect(corner.shrink(4.0)), |ui| {
            ui.horizontal(|ui| {
                let r = widgets::button(ui, "−", false);
                if r.clicked() {
                    self.arr.pps = (self.arr.pps / 1.5).max(0.5);
                }
                self.hint(&r, "Zoom Out", "Zoom out of the timeline (Ctrl+Mouse wheel).");
                let r = widgets::button(ui, "+", false);
                if r.clicked() {
                    self.arr.pps = (self.arr.pps * 1.5).min(40.0);
                }
                self.hint(&r, "Zoom In", "Zoom into the timeline (Ctrl+Mouse wheel).");
                let r = widgets::toggle(ui, "Follow", self.arr.follow, ACCENT, vec2(46.0, 19.0));
                if r.clicked() {
                    self.arr.follow = !self.arr.follow;
                }
                self.hint(&r, "Follow", "Scrolls the timeline to keep the playhead visible.");
                let r = widgets::button(ui, "Scenes→", false);
                if r.clicked() {
                    self.scenes_to_arrangement();
                }
                self.hint(&r, "Copy Scenes to Timeline", "Appends all non-empty scenes one after another to the end of the arrangement (at least 8 bars each).");
            });
            ui.add_space(4.0);
            self.arranger_tools(ui);
        });

        // Playhead and start marker.
        let px = x_of(self.status.arr_pos, sx);
        if px >= lanes.left() && px <= lanes.right() {
            p.line_segment([pos2(px, ruler.top()), pos2(px, lanes.bottom())], Stroke::new(1.0_f32, if self.status.playing { Color32::WHITE } else { TEXT_DIM }));
        }
        let mx = x_of(self.status.start_marker as f32, sx);
        if mx >= lanes.left() {
            p.add(egui::Shape::convex_polygon(
                vec![pos2(mx - 5.0, ruler.bottom() - 8.0), pos2(mx + 5.0, ruler.bottom() - 8.0), pos2(mx, ruler.bottom())],
                TEXT,
                Stroke::NONE,
            ));
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(ArrDrag::Move { track, id, .. } | ArrDrag::Resize { track, id }) = self.arr.drag {
                self.resolve_overlaps(track, id);
            }
            self.arr.drag = None;
        }
    }

    fn ruler(&mut self, ui: &mut egui::Ui, ruler: Rect, x_of: impl Fn(f32, f32) -> f32) {
        let p = ui.painter().with_clip_rect(ruler);
        let sx = self.arr.scroll_x;
        let pps = self.arr.pps;
        p.rect_filled(ruler, 0.0, PANEL);
        let brace = Rect::from_min_max(ruler.min, pos2(ruler.right(), ruler.top() + 11.0));
        let ls = self.project.loop_start as f32;
        let le = (self.project.loop_start + self.project.loop_len) as f32;
        let lr = Rect::from_x_y_ranges(x_of(ls, sx)..=x_of(le, sx), brace.top() + 1.0..=brace.bottom() - 1.0);
        p.rect_filled(lr, 1.0, if self.project.loop_on { ACCENT } else { Color32::from_rgb(0x70, 0x70, 0x70) });
        let first_bar = (sx / 16.0).floor() as i64;
        let last_bar = ((sx + ruler.width() / pps) / 16.0).ceil() as i64;
        let bar_step = if pps < 1.0 { 8 } else if pps < 2.0 { 4 } else if pps < 3.0 { 2 } else { 1 };
        for bar in first_bar.max(0)..=last_bar {
            let x = x_of(bar as f32 * 16.0, sx);
            if bar % bar_step == 0 {
                p.line_segment([pos2(x, ruler.top() + 13.0), pos2(x, ruler.bottom())], Stroke::new(1.0_f32, TEXT_DIM));
                p.text(pos2(x + 3.0, ruler.top() + 13.0), Align2::LEFT_TOP, (bar + 1).to_string(), FontId::proportional(10.5), TEXT);
            }
        }

        let resp = ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
        let pos = resp.interact_pointer_pos().or(resp.hover_pos());
        let alt = ui.input(|i| i.modifiers.alt);
        if let Some(pos) = pos {
            let step = (pos.x - ruler.left()) / pps + sx;
            let in_brace = pos.y <= brace.bottom();
            if resp.drag_started() && in_brace {
                let near = |x: f32| (pos.x - x).abs() < 6.0;
                self.arr.drag = Some(if near(lr.left()) {
                    ArrDrag::LoopStart
                } else if near(lr.right()) {
                    ArrDrag::LoopEnd
                } else {
                    ArrDrag::LoopMove { grab: step - ls }
                });
            }
            if resp.dragged() {
                match self.arr.drag {
                    Some(ArrDrag::LoopMove { grab }) => {
                        self.project.loop_start = self.snap(step - grab, alt);
                        self.touch();
                    }
                    Some(ArrDrag::LoopStart) => {
                        let end = self.project.loop_start + self.project.loop_len;
                        let s = self.snap(step, alt).min(end.saturating_sub(4));
                        self.project.loop_start = s;
                        self.project.loop_len = end - s;
                        self.touch();
                    }
                    Some(ArrDrag::LoopEnd) => {
                        let e = self.snap(step, alt).max(self.project.loop_start + 4);
                        self.project.loop_len = e - self.project.loop_start;
                        self.touch();
                    }
                    _ => {}
                }
            }
            if resp.clicked() {
                if in_brace {
                    self.project.loop_on = !self.project.loop_on;
                    self.touch();
                } else {
                    self.send(Cmd::SetStart(self.snap_floor(step, alt)));
                }
            }
            if resp.double_clicked() && !in_brace {
                self.send(Cmd::SetStart(self.snap_floor(step, alt)));
                self.send(Cmd::Play);
            }
        }
        self.hint(
            &resp,
            "Ruler / Loop Brace",
            "Click the ruler to set the start marker, double-click to play from there. Top strip: loop brace - drag to move, drag its edges to resize, click to toggle the loop.",
        );
    }

    fn arr_track_header(&mut self, ui: &mut egui::Ui, r: Rect, id: u64) {
        let Some(t) = self.project.track(id) else { return };
        let col = color32(t.color);
        let selected = self.sel == Sel::Track(id);
        let resp = ui.interact(r, ui.id().with(("arr_hdr", id)), Sense::click());
        ui.painter().rect_filled(r, 0.0, if selected { PANEL_LIGHT } else { PANEL });
        ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(5.0, r.height())), 0.0, col);
        let name = t.name.clone();
        let (mute, solo) = (t.channel.mute, t.channel.solo);
        let idx = self.project.track_index(id).unwrap_or(0) + 1;
        ui.painter().text(r.left_top() + vec2(10.0, 5.0), Align2::LEFT_TOP, &name, FontId::proportional(12.0), TEXT);
        let follow = self.track_status(id).is_none_or(|s| s.follow_arr);
        if !follow {
            ui.painter().text(r.left_top() + vec2(10.0, 21.0), Align2::LEFT_TOP, "playing Launcher", FontId::proportional(9.5), ACCENT);
        }
        let mut new_mute = mute;
        let mut new_solo = solo;
        let auto_on = self.auto_open.contains(&id);
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(r.right() - 84.0, r.bottom() - 24.0), vec2(84.0, 22.0))), |ui| {
            ui.horizontal(|ui| {
                let ra = widgets::toggle(ui, &idx.to_string(), !mute, ACCENT, vec2(24.0, 18.0));
                if ra.clicked() {
                    new_mute = !mute;
                }
                self.hint(&ra, "Track Activator", "Turns the track on/off (mute).");
                let rs = widgets::toggle(ui, "S", solo, BLUE, vec2(22.0, 18.0));
                if rs.clicked() {
                    new_solo = !solo;
                }
                self.hint(&rs, "Solo", "Plays only soloed tracks.");
                let rau = widgets::toggle(ui, "A", auto_on, ACCENT, vec2(22.0, 18.0));
                if rau.clicked() {
                    if auto_on {
                        self.auto_open.remove(&id);
                    } else {
                        self.auto_open.insert(id);
                    }
                }
                self.hint(&rau, "Automation Lane", "Shows the automation lane for this track. Draw breakpoints to change volume, pan, sends or any device parameter over time.");
            });
        });
        if new_mute != mute || new_solo != solo {
            if let Some(t) = self.project.track_mut(id) {
                t.channel.mute = new_mute;
                t.channel.solo = new_solo;
            }
            self.touch();
        }
        if resp.clicked() {
            self.select_track(id);
            self.detail = Detail::Devices;
        }
        resp.context_menu(|ui| self.track_menu(ui, id));
        if let Some(p) = self.drop_target(ui, r, |p| matches!(p, Payload::Device(_) | Payload::MidiFx(_) | Payload::Instrument(_))) {
            self.drop_on_track(id, p);
        }
        self.hint(&resp, &name, "Track header. Click to select, right-click for options. Drop effects or instruments here.");
    }

    fn clip_lane(&mut self, ui: &mut egui::Ui, row: Rect, lanes: Rect, track: u64) {
        let pps = self.arr.pps;
        let sx = self.arr.scroll_x;
        let alt = ui.input(|i| i.modifiers.alt);
        let lane_rect = row.intersect(lanes);
        if self.project.track(track).is_some_and(|t| t.instrument.kind == InstrumentKind::Group) {
            ui.painter().with_clip_rect(lanes).rect_filled(lane_rect, 0.0, Color32::from_rgb(0x26, 0x26, 0x2a));
            return;
        }
        let bg = ui.interact(lane_rect, ui.id().with(("lane", track)), Sense::click());
        let step_at = |x: f32| (x - lanes.left()) / pps + sx;
        if bg.clicked() {
            self.sel_clip = None;
            self.select_track(track);
        }
        if bg.double_clicked() {
            if let Some(pos) = bg.interact_pointer_pos() {
                let start = self.snap_floor(step_at(pos.x), false) / 16 * 16;
                let (name, color, kind) = {
                    let t = self.project.track(track).unwrap();
                    (t.name.clone(), t.color, t.instrument.kind)
                };
                let clip = Clip::new(&name, color, if kind == InstrumentKind::Drums { 1 } else { 2 });
                let id = self.project.new_id();
                let len = clip.len;
                self.project.track_mut(track).unwrap().arrangement.push(ArrClip { id, start, len, clip });
                self.resolve_overlaps(track, id);
                self.select_clip(ClipRef::Arr { track, id });
                self.touch();
            }
        }
        self.hint(&bg, "Timeline Lane", "Drag clips here from the Launcher or the browser. Double-click to create an empty clip. Alt disables snapping.");

        let p = ui.painter().with_clip_rect(lanes);
        let clips: Vec<ArrClip> = self.project.track(track).map(|t| t.arrangement.clone()).unwrap_or_default();
        let playing_id = self.track_status(track).and_then(|s| s.arr_clip);
        for ac in &clips {
            let x0 = lanes.left() + (ac.start as f32 - sx) * pps;
            let x1 = x0 + ac.len as f32 * pps;
            if x1 < lanes.left() || x0 > lanes.right() {
                continue;
            }
            let r = Rect::from_min_max(pos2(x0, row.top() + 1.0), pos2(x1.max(x0 + 3.0), row.bottom() - 2.0));
            let col = color32(ac.clip.color);
            let selected = self.sel_clip == Some(ClipRef::Arr { track, id: ac.id });
            p.rect_filled(r, 2.0, darken(col, 0.55));
            let head = Rect::from_min_size(r.min, vec2(r.width(), 13.0));
            p.rect_filled(head, 2.0, col);
            p.with_clip_rect(r.intersect(lanes)).text(head.left_center() + vec2(3.0, 0.0), Align2::LEFT_CENTER, &ac.clip.name, FontId::proportional(10.5), on_color(col));
            // Note preview, repeated for looped clips.
            let body = Rect::from_min_max(pos2(r.left(), head.bottom() + 2.0), pos2(r.right(), r.bottom() - 2.0));
            if let Some(a) = &ac.clip.audio {
                self.draw_audio_preview(&p.with_clip_rect(body.intersect(lanes)), body, a, ac.clip.len, ac.len, x0, pps, lerp(col, Color32::WHITE, 0.3));
            }
            if !ac.clip.notes.is_empty() && ac.clip.len > 0 {
                let (lo, hi) = ac.clip.notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.pitch), h.max(n.pitch)));
                let span = (hi - lo).max(6) as f32;
                let pc = p.with_clip_rect(body.intersect(lanes));
                let reps = ac.len.div_ceil(ac.clip.len);
                for rep in 0..reps {
                    let off = rep * ac.clip.len;
                    let rx = x0 + off as f32 * pps;
                    if rx > lanes.right() {
                        break;
                    }
                    if rep > 0 {
                        pc.line_segment([pos2(rx, body.top()), pos2(rx, body.bottom())], Stroke::new(1.0_f32, darken(col, 0.8)));
                    }
                    for n in &ac.clip.notes {
                        if off + n.step >= ac.len {
                            continue;
                        }
                        let nx = rx + n.step as f32 * pps;
                        let ny = body.bottom() - (n.pitch - lo) as f32 / span * (body.height() - 3.0) - 2.0;
                        let w = (n.len as f32 * pps - 1.0).max(1.5);
                        pc.rect_filled(Rect::from_min_size(pos2(nx, ny - 1.0), vec2(w, 2.5)), 0.0, lerp(col, Color32::WHITE, 0.4));
                    }
                }
            }
            if playing_id == Some(ac.id) && self.status.playing {
                p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, PLAY), egui::StrokeKind::Inside);
            }
            if selected {
                p.rect_stroke(r, 2.0, Stroke::new(1.5_f32, Color32::WHITE), egui::StrokeKind::Inside);
            }
            let resp = ui.interact(r.intersect(lanes), ui.id().with(("ac", ac.id)), Sense::click_and_drag());
            let near_edge = resp.hover_pos().is_some_and(|pos| (pos.x - r.right()).abs() < 6.0);
            if near_edge || matches!(self.arr.drag, Some(ArrDrag::Resize { .. })) && resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if resp.clicked() {
                self.select_clip(ClipRef::Arr { track, id: ac.id });
            }
            if resp.double_clicked() {
                self.select_clip(ClipRef::Arr { track, id: ac.id });
                self.detail = Detail::Clip;
            }
            if resp.drag_started() {
                let pos = resp.interact_pointer_pos().unwrap_or(r.min);
                self.select_clip(ClipRef::Arr { track, id: ac.id });
                self.arr.drag = Some(if near_edge {
                    ArrDrag::Resize { track, id: ac.id }
                } else {
                    ArrDrag::Move { track, id: ac.id, grab: step_at(pos.x) - ac.start as f32 }
                });
            }
            resp.context_menu(|ui| {
                if ui.button("Duplicate   (Ctrl+D)").clicked() {
                    self.sel_clip = Some(ClipRef::Arr { track, id: ac.id });
                    self.duplicate_selected_clip();
                    ui.close_menu();
                }
                if ui.button("Delete   (Del)").clicked() {
                    self.sel_clip = Some(ClipRef::Arr { track, id: ac.id });
                    self.delete_selected_clip();
                    ui.close_menu();
                }
                if ui.button("Set Loop to Clip").clicked() {
                    self.project.loop_start = ac.start;
                    self.project.loop_len = ac.len;
                    self.project.loop_on = true;
                    self.touch();
                    ui.close_menu();
                }
            });
            self.hint(&resp, &ac.clip.name, "Timeline clip. Drag to move (also to other tracks), drag the right edge to loop/shorten it, double-click to edit notes. Alt disables snapping.");
        }

        // Active drag for clips of this track.
        if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
            match self.arr.drag {
                Some(ArrDrag::Move { track: t, id, grab }) if t == track && ui.input(|i| i.pointer.primary_down()) => {
                    let new_start = self.snap(step_at(pos.x) - grab, alt);
                    // Moving between tracks: find the track row under the pointer.
                    let target = self.track_at_y(lanes, pos.y).unwrap_or(t);
                    if let Some(tr) = self.project.track_mut(t) {
                        if let Some(i) = tr.arrangement.iter().position(|c| c.id == id) {
                            if target != t {
                                let mut ac = tr.arrangement.remove(i);
                                ac.start = new_start;
                                if let Some(dst) = self.project.track_mut(target) {
                                    let color = dst.color;
                                    ac.clip.color = color;
                                    dst.arrangement.push(ac);
                                    self.arr.drag = Some(ArrDrag::Move { track: target, id, grab });
                                    self.sel_clip = Some(ClipRef::Arr { track: target, id });
                                }
                                self.touch();
                            } else if tr.arrangement[i].start != new_start {
                                tr.arrangement[i].start = new_start;
                                self.touch();
                            }
                        }
                    }
                }
                Some(ArrDrag::Resize { track: t, id }) if t == track && ui.input(|i| i.pointer.primary_down()) => {
                    if let Some(tr) = self.project.track_mut(t) {
                        if let Some(c) = tr.arrangement.iter_mut().find(|c| c.id == id) {
                            let end = ((pos.x - lanes.left()) / pps + sx).max(c.start as f32 + 1.0);
                            let grid = if alt { 1.0 } else { 4.0 };
                            let new_len = (((end - c.start as f32) / grid).round() * grid).max(grid) as u32;
                            if new_len != c.len {
                                c.len = new_len;
                                self.touch();
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Drops from Launcher or browser.
        match self.drop_target(ui, lane_rect, |p| matches!(p, Payload::SessionClip { .. } | Payload::Pattern(_) | Payload::Sample(_))) {
            Some(Payload::Sample(path)) => {
                let is_audio = self.project.track(track).is_some_and(|t| t.instrument.kind == InstrumentKind::Audio);
                if is_audio {
                    let color = self.project.track(track).map(|t| t.color).unwrap_or([200, 200, 200]);
                    if let Some(clip) = self.audio_clip(&path, color) {
                        let pos = ui.input(|i| i.pointer.latest_pos()).unwrap_or(row.min);
                        self.insert_arr_clip(track, step_at(pos.x), clip, alt);
                    }
                } else {
                    self.toast("Drop samples onto an audio track lane");
                }
            }
            Some(Payload::SessionClip { track: st, scene }) => {
                if let Some(clip) = self.clip(ClipRef::Session { track: st, scene }).cloned() {
                    let pos = ui.input(|i| i.pointer.latest_pos()).unwrap_or(row.min);
                    self.insert_arr_clip(track, step_at(pos.x), clip, alt);
                }
            }
            Some(Payload::Pattern(name)) => {
                if let (Some(pat), Some(t)) = (crate::presets::pattern(name), self.project.track(track)) {
                    if pat.kind == t.instrument.kind || (matches!(pat.kind, InstrumentKind::Bass | InstrumentKind::Synth) && t.instrument.kind.is_melodic()) {
                        let clip = pat.clip(t.color);
                        let pos = ui.input(|i| i.pointer.latest_pos()).unwrap_or(row.min);
                        self.insert_arr_clip(track, step_at(pos.x), clip, alt);
                    } else {
                        self.toast(format!("\"{}\" needs a {} track", pat.name, pat.kind.name()));
                    }
                }
            }
            _ => {}
        }
    }

    /// Waveform preview of an audio clip on the timeline, repeated when looped.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_audio_preview(&mut self, p: &egui::Painter, body: Rect, a: &AudioClip, clip_len: u32, total_len: u32, x0: f32, pps: f32, col: Color32) {
        let Ok(s) = crate::audio::samples::load(&a.path) else { return };
        let peaks = self.peaks(&a.path, &s, 1024);
        let secs_per_step = match a.warp {
            WarpMode::Off => 60.0 / self.project.bpm / 4.0,
            _ => 60.0 / a.bpm.max(1.0) / 4.0,
        };
        let dur = s.left.len() as f32 / s.sample_rate;
        let mid = body.center().y;
        let h = body.height() * 0.45;
        let clip = p.clip_rect();
        let x_start = x0.max(clip.left());
        let x_end = (x0 + total_len as f32 * pps).min(clip.right());
        let mut x = x_start;
        while x < x_end {
            let step = (x - x0) / pps;
            let local = step % clip_len.max(1) as f32;
            let t = a.offset + local * secs_per_step;
            if t < dur {
                let idx = ((t / dur) * peaks.len() as f32) as usize;
                if let Some((lo, hi)) = peaks.get(idx) {
                    p.line_segment([pos2(x, mid - hi * h), pos2(x, mid - lo * h)], Stroke::new(1.0_f32, col));
                }
            }
            x += 1.0;
        }
    }

    fn insert_arr_clip(&mut self, track: u64, step: f32, mut clip: Clip, alt: bool) {
        let start = self.snap_floor(step, alt);
        let id = self.project.new_id();
        if let Some(t) = self.project.track_mut(track) {
            clip.color = t.color;
            let len = clip.len;
            t.arrangement.push(ArrClip { id, start, len, clip });
        }
        self.resolve_overlaps(track, id);
        self.select_clip(ClipRef::Arr { track, id });
        self.touch();
    }

    fn track_at_y(&self, lanes: Rect, y: f32) -> Option<u64> {
        let mut top = lanes.top() - self.arr.scroll_y;
        for t in &self.project.tracks {
            if y >= top && y < top + LANE_H {
                return Some(t.id);
            }
            top += LANE_H;
            if self.auto_open.contains(&t.id) {
                top += AUTO_H;
            }
        }
        None
    }

    /// The clip `id` wins: other clips on the track that overlap it are trimmed or removed.
    fn resolve_overlaps(&mut self, track: u64, id: u64) {
        let Some(t) = self.project.track_mut(track) else { return };
        let Some(me) = t.arrangement.iter().find(|c| c.id == id).cloned() else { return };
        let (s, e) = (me.start, me.start + me.len);
        let mut extra = vec![];
        t.arrangement.retain_mut(|c| {
            if c.id == id {
                return true;
            }
            let (cs, ce) = (c.start, c.start + c.len);
            if ce <= s || cs >= e {
                return true;
            }
            if cs >= s && ce <= e {
                return false;
            }
            if cs < s && ce > e {
                // Split around the winner: keep the left part here, add the right part.
                let mut right = c.clone();
                right.start = e;
                right.len = ce - e;
                extra.push(right);
                c.len = s - cs;
                return true;
            }
            if cs < s {
                c.len = s - cs;
            } else {
                c.len = ce - e;
                c.start = e;
            }
            true
        });
        for mut r in extra {
            r.id = self.project.new_id();
            if let Some(t) = self.project.track_mut(track) {
                t.arrangement.push(r);
            }
        }
        if let Some(t) = self.project.track_mut(track) {
            t.arrangement.sort_by_key(|c| c.start);
        }
    }

    fn current_target(&self, track: u64) -> AutoTarget {
        if let Some(t) = self.arr.targets.get(&track) {
            return *t;
        }
        self.project.track(track).and_then(|t| t.automation.first()).map(|a| a.target).unwrap_or(AutoTarget::Volume)
    }

    fn automation_header(&mut self, ui: &mut egui::Ui, r: Rect, track: u64) {
        ui.painter().rect_filled(r, 0.0, PANEL_DARK);
        let Some(t) = self.project.track(track) else { return };
        let targets = auto_targets(&self.project, t);
        let cur = self.current_target(track);
        let cur_name = targets.iter().find(|(tg, _)| *tg == cur).map(|(_, n)| n.clone()).unwrap_or_else(|| "?".into());
        let has: Vec<AutoTarget> = t.automation.iter().filter(|a| !a.points.is_empty()).map(|a| a.target).collect();
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                egui::ComboBox::from_id_salt(("auto_target", track)).width(HEADER_W - 24.0).selected_text(cur_name).show_ui(ui, |ui| {
                    for (tg, name) in &targets {
                        let label = if has.contains(tg) { format!("● {name}") } else { name.clone() };
                        if ui.selectable_label(*tg == cur, label).clicked() {
                            self.arr.targets.insert(track, *tg);
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                if ui.small_button("Clear").on_hover_text("Delete all breakpoints of this parameter").clicked() {
                    if let Some(t) = self.project.track_mut(track) {
                        t.automation.retain(|a| a.target != cur);
                    }
                    self.touch();
                }
            });
        });
    }

    fn automation_lane(&mut self, ui: &mut egui::Ui, row: Rect, lanes: Rect, track: u64) {
        let pps = self.arr.pps;
        let sx = self.arr.scroll_x;
        let target = self.current_target(track);
        let p = ui.painter().with_clip_rect(lanes.intersect(row));
        p.rect_filled(row, 0.0, Color32::from_rgb(0x26, 0x26, 0x26));
        let Some(t) = self.project.track(track) else { return };
        let base = base_norm(t, target);
        let auto_idx = t.automation.iter().position(|a| a.target == target);
        let pts: Vec<(f32, f32)> = auto_idx.map(|i| t.automation[i].points.clone()).unwrap_or_default();
        let inner = row.shrink2(vec2(0.0, 5.0));
        let to_screen = |s: f32, v: f32| pos2(lanes.left() + (s - sx) * pps, inner.bottom() - v * inner.height());
        let from_screen = |pos: egui::Pos2| (((pos.x - lanes.left()) / pps + sx).max(0.0), ((inner.bottom() - pos.y) / inner.height()).clamp(0.0, 1.0));
        let line_col = Color32::from_rgb(0xff, 0x50, 0x50);
        if pts.is_empty() {
            let y = to_screen(0.0, base).y;
            p.line_segment([pos2(lanes.left(), y), pos2(lanes.right(), y)], Stroke::new(1.0_f32, darken(line_col, 0.5)));
        } else {
            let mut line = vec![pos2(lanes.left(), to_screen(0.0, pts[0].1).y)];
            line.extend(pts.iter().map(|&(s, v)| to_screen(s, v)));
            line.push(pos2(lanes.right(), to_screen(0.0, pts.last().unwrap().1).y));
            p.add(egui::Shape::line(line, Stroke::new(1.5_f32, line_col)));
        }
        let lane_resp = ui.interact(row.intersect(lanes), ui.id().with(("auto_lane", track)), Sense::click());
        let alt = ui.input(|i| i.modifiers.alt);
        // Points.
        for (pi, &(s, v)) in pts.iter().enumerate() {
            let c = to_screen(s, v);
            if !lanes.contains(c) && (c.x < lanes.left() || c.x > lanes.right()) {
                continue;
            }
            let pr = Rect::from_center_size(c, vec2(10.0, 10.0));
            let resp = ui.interact(pr, ui.id().with(("pt", track, pi)), Sense::click_and_drag());
            p.circle_filled(c, if resp.hovered() { 4.5 } else { 3.5 }, line_col);
            if resp.drag_started() {
                self.arr.drag = Some(ArrDrag::Point { track, auto: auto_idx.unwrap(), point: pi });
            }
            if resp.double_clicked() || resp.secondary_clicked() {
                if let (Some(ai), Some(t)) = (auto_idx, self.project.track_mut(track)) {
                    t.automation[ai].points.remove(pi);
                    if t.automation[ai].points.is_empty() {
                        t.automation.remove(ai);
                    }
                    self.touch();
                }
                return;
            }
            let def_info = format!("Breakpoint at bar {:.2}. Drag to move, double-click or right-click to delete.", s / 16.0 + 1.0);
            self.hint(&resp, "Automation Breakpoint", &def_info);
        }
        if let Some(ArrDrag::Point { track: tt, auto, point }) = self.arr.drag {
            if tt == track && ui.input(|i| i.pointer.primary_down()) {
                if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                    let (s, v) = from_screen(pos);
                    let s = if alt { s } else { (s).round() };
                    if let Some(a) = self.project.track_mut(track).and_then(|t| t.automation.get_mut(auto)) {
                        if point < a.points.len() {
                            let lo = if point > 0 { a.points[point - 1].0 } else { 0.0 };
                            let hi = a.points.get(point + 1).map(|p| p.0).unwrap_or(f32::MAX);
                            a.points[point] = (s.clamp(lo, hi), v);
                            self.touch();
                        }
                    }
                }
            }
        }
        if lane_resp.clicked() {
            if let Some(pos) = lane_resp.interact_pointer_pos() {
                let (s, v) = from_screen(pos);
                let s = if alt { s } else { s.round() };
                if let Some(t) = self.project.track_mut(track) {
                    let idx = match t.automation.iter().position(|a| a.target == target) {
                        Some(i) => i,
                        None => {
                            t.automation.push(Automation { target, points: vec![] });
                            t.automation.len() - 1
                        }
                    };
                    let a = &mut t.automation[idx];
                    let at = a.points.iter().position(|p| p.0 > s).unwrap_or(a.points.len());
                    a.points.insert(at, (s, v));
                    self.touch();
                }
            }
        }
        self.hint(&lane_resp, "Automation Lane", "Click to add breakpoints; the parameter follows the red line during Timeline playback. Choose the parameter on the left.");
    }
}
