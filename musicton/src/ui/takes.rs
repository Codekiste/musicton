//! Take lanes: alternative recordings below a timeline track. Clicking a take clip comps it
//! into the main lane (the part it covers replaces what was there).

use super::theme::*;
use super::{App, ClipRef};
use crate::model::*;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

pub const TAKE_H: f32 = 30.0;

impl App {
    pub(super) fn add_take_lane(&mut self, track: u64) -> Option<u64> {
        let id = self.project.new_id();
        let t = self.project.track_mut(track)?;
        let n = t.takes.len() + 1;
        t.takes.push(TakeLane { id, name: format!("Take {n}"), clips: vec![] });
        t.show_takes = true;
        self.touch();
        Some(id)
    }

    /// Moves a timeline clip onto a new take lane (keeps it as an alternative).
    pub(super) fn move_clip_to_take(&mut self, track: u64, id: u64) {
        if self.guard_locked(track) {
            return;
        }
        let lane_id = self.project.new_id();
        let Some(t) = self.project.track_mut(track) else { return };
        let Some(i) = t.arrangement.iter().position(|c| c.id == id) else { return };
        let c = t.arrangement.remove(i);
        let n = t.takes.len() + 1;
        t.takes.push(TakeLane { id: lane_id, name: format!("Take {n}"), clips: vec![c] });
        t.show_takes = true;
        self.sel_clip = None;
        self.touch();
    }

    /// Comps a take clip into the main lane: it replaces whatever plays in its time range.
    pub(super) fn use_take_clip(&mut self, track: u64, lane: u64, id: u64) {
        if self.guard_locked(track) {
            return;
        }
        let Some(c) = self.project.track(track).and_then(|t| t.takes.iter().find(|l| l.id == lane)).and_then(|l| l.clips.iter().find(|c| c.id == id)).cloned() else { return };
        let nid = self.project.new_id();
        if let Some(t) = self.project.track_mut(track) {
            t.arrangement.push(ArrClip { id: nid, ..c });
        }
        self.resolve_overlaps(track, nid);
        self.sel_clip = Some(ClipRef::Arr { track, id: nid });
        self.touch();
    }

    pub(super) fn delete_take_lane(&mut self, track: u64, lane: u64) {
        if let Some(t) = self.project.track_mut(track) {
            t.takes.retain(|l| l.id != lane);
        }
        self.touch();
    }

    pub(super) fn take_header(&mut self, ui: &mut egui::Ui, r: Rect, track: u64, idx: usize) {
        let Some(lane) = self.project.track(track).and_then(|t| t.takes.get(idx)).cloned() else { return };
        let p = ui.painter();
        p.rect_filled(r, 0.0, PANEL_DARK);
        p.text(r.left_center() + vec2(18.0, 0.0), Align2::LEFT_CENTER, &lane.name, FontId::proportional(10.5), TEXT_DIM);
        let resp = ui.interact(r, ui.id().with(("take_hdr", lane.id)), Sense::click());
        resp.context_menu(|ui| {
            if ui.button("Delete Take Lane").clicked() {
                self.delete_take_lane(track, lane.id);
                ui.close_menu();
            }
            if ui.button("Hide Take Lanes").clicked() {
                if let Some(t) = self.project.track_mut(track) {
                    t.show_takes = false;
                }
                self.touch();
                ui.close_menu();
            }
        });
        self.hint(&resp, &lane.name, "Take lane with an alternative recording. Click a clip here to use that part in the track (comping). Right-click to delete the lane.");
    }

    pub(super) fn take_lane(&mut self, ui: &mut egui::Ui, row: Rect, lanes: Rect, track: u64, idx: usize) {
        let Some(lane) = self.project.track(track).and_then(|t| t.takes.get(idx)).cloned() else { return };
        let pps = self.arr.pps;
        let sx = self.arr.scroll_x;
        let p = ui.painter().with_clip_rect(lanes);
        p.rect_filled(row.intersect(lanes), 0.0, Color32::from_rgb(0x24, 0x24, 0x28));
        let main: Vec<(u32, u32)> = self.project.track(track).map(|t| t.arrangement.iter().map(|c| (c.start, c.start + c.len)).collect()).unwrap_or_default();
        for c in &lane.clips {
            let x0 = lanes.left() + (c.start as f32 - sx) * pps;
            let x1 = x0 + c.len as f32 * pps;
            if x1 < lanes.left() || x0 > lanes.right() {
                continue;
            }
            let r = Rect::from_min_max(pos2(x0, row.top() + 2.0), pos2(x1.max(x0 + 3.0), row.bottom() - 2.0));
            let col = color32(c.clip.color);
            // Parts that also play in the main lane are drawn brighter.
            let used = main.iter().any(|&(s, e)| s == c.start && e == c.start + c.len);
            p.rect_filled(r, 2.0, if used { darken(col, 0.8) } else { darken(col, 0.4) });
            p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, darken(col, 0.9)), egui::StrokeKind::Inside);
            p.with_clip_rect(r.intersect(lanes)).text(r.left_center() + vec2(3.0, 0.0), Align2::LEFT_CENTER, &c.clip.name, FontId::proportional(9.5), TEXT);
            let resp = ui.interact(r.intersect(lanes), ui.id().with(("take_clip", c.id)), Sense::click());
            if resp.clicked() {
                self.use_take_clip(track, lane.id, c.id);
            }
            resp.context_menu(|ui| {
                if ui.button("Use This Take").clicked() {
                    self.use_take_clip(track, lane.id, c.id);
                    ui.close_menu();
                }
                if ui.button("Delete from Take Lane").clicked() {
                    if let Some(l) = self.project.track_mut(track).and_then(|t| t.takes.iter_mut().find(|l| l.id == lane.id)) {
                        l.clips.retain(|x| x.id != c.id);
                    }
                    self.touch();
                    ui.close_menu();
                }
            });
            self.hint(&resp, &c.clip.name, "Click to use this take in the track: it replaces what plays in this time range.");
        }
    }
}
