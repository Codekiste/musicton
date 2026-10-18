//! Video track: import (frames + audio via ffmpeg), picture window synced to the playhead,
//! thumbnail lane on the timeline and muxing the exported mix back under the video.

use super::theme::*;
use super::App;
use crate::model::*;
use crate::video;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};
use std::collections::HashMap;
use std::sync::Arc;

type ImportSlot = Arc<parking_lot::Mutex<Option<Result<(VideoTrack, Option<String>), String>>>>;

#[derive(Default)]
pub struct VideoState {
    pub open: bool,
    import: Option<ImportSlot>,
    frame: Option<(u32, egui::TextureHandle)>,
    thumbs: HashMap<u32, egui::TextureHandle>,
    /// MP4 path to create once the running WAV export finishes.
    pub mux_after_export: Option<String>,
}

pub const VIDEO_H: f32 = 36.0;

/// Does the heavy lifting off the UI thread (or inline for remote control).
pub fn import_blocking(path: &str, start: u32) -> Result<(VideoTrack, Option<String>), String> {
    let (duration, has_audio) = video::probe(path)?;
    let stem: String = std::path::Path::new(path).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let base = dirs::data_dir().unwrap_or_else(|| ".".into()).join("musicton").join("video").join(format!("{stem}-{stamp}"));
    let count = video::extract_frames(path, &base)?;
    let audio = if has_audio {
        let wav = base.join("audio.wav");
        video::extract_audio(path, &wav).ok().map(|_| wav.to_string_lossy().to_string())
    } else {
        None
    };
    Ok((VideoTrack { path: path.to_string(), start, duration, frames: base.to_string_lossy().to_string(), frame_count: count }, audio))
}

impl App {
    pub(super) fn import_video(&mut self, path: String) {
        if !video::available() {
            self.toast("Video needs ffmpeg - install it with your package manager");
            return;
        }
        let slot: ImportSlot = Arc::new(parking_lot::Mutex::new(None));
        let s2 = slot.clone();
        std::thread::spawn(move || {
            *s2.lock() = Some(import_blocking(&path, 0));
        });
        self.video.import = Some(slot);
        self.toast("Importing video...");
    }

    /// Puts an imported video (and its audio on a new audio track) into the project.
    pub(super) fn apply_video(&mut self, v: VideoTrack, audio: Option<String>) {
        if let Some(wav) = audio {
            let path = std::path::PathBuf::from(&wav);
            let track = self.add_audio_track(None);
            if let Some(mut clip) = self.audio_clip(&path, CLIP_COLORS[9]) {
                clip.name = "Video Audio".into();
                if let Some(a) = clip.audio.as_mut() {
                    a.warp = WarpMode::Off;
                }
                let len = (v.duration * self.project.bpm / 60.0 * 4.0).ceil() as u32;
                clip.len = len.max(1);
                let id = self.project.new_id();
                if let Some(t) = self.project.track_mut(track) {
                    t.name = "Video Audio".into();
                    t.arrangement.push(ArrClip { id, start: v.start, len: len.max(1), clip });
                }
            }
        }
        self.project.video = Some(v);
        self.video.thumbs.clear();
        self.video.frame = None;
        self.video.open = true;
        self.touch();
    }

    fn poll_video_import(&mut self) {
        let Some(slot) = &self.video.import else { return };
        let res = slot.lock().take();
        match res {
            Some(Ok((v, audio))) => {
                self.video.import = None;
                let n = v.frame_count;
                self.apply_video(v, audio);
                self.toast(format!("Video imported ({n} frames)"));
            }
            Some(Err(e)) => {
                self.video.import = None;
                self.toast(format!("Video import failed: {e}"));
            }
            None => {}
        }
    }

    fn frame_index(&self, v: &VideoTrack, step: f32) -> Option<u32> {
        let secs = (step - v.start as f32) / 4.0 * 60.0 / self.project.bpm.max(1.0);
        if secs < 0.0 || secs > v.duration {
            return None;
        }
        Some(((secs * video::FRAME_FPS) as u32).min(v.frame_count.saturating_sub(1)))
    }

    fn load_texture(ctx: &egui::Context, v: &VideoTrack, idx: u32, name: &str) -> Option<egui::TextureHandle> {
        let path = std::path::Path::new(&v.frames).join(format!("{:05}.png", idx + 1));
        let (w, h, rgba) = video::load_frame(&path)?;
        Some(ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR))
    }

    pub(super) fn video_window(&mut self, ctx: &egui::Context) {
        self.poll_video_import();
        if self.video.import.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let Some(v) = self.project.video.clone() else { return };
        if !self.video.open {
            return;
        }
        let idx = self.frame_index(&v, self.status.arr_pos);
        if let Some(i) = idx {
            if self.video.frame.as_ref().is_none_or(|f| f.0 != i) {
                if let Some(t) = Self::load_texture(ctx, &v, i, "video_frame") {
                    self.video.frame = Some((i, t));
                }
            }
        }
        let mut open = true;
        egui::Window::new("Video").open(&mut open).default_pos([900.0, 120.0]).default_width(380.0).show(ctx, |ui| {
            let w = ui.available_width().max(160.0);
            match (&self.video.frame, idx) {
                (Some((_, tex)), Some(_)) => {
                    let size = tex.size_vec2();
                    ui.image((tex.id(), vec2(w, w * size.y / size.x.max(1.0))));
                }
                _ => {
                    let (r, _) = ui.allocate_exact_size(vec2(w, w * 9.0 / 16.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, Color32::BLACK);
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, "(outside the video)", FontId::proportional(11.0), TEXT_DIM);
                }
            }
            let name = std::path::Path::new(&v.path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let secs = (self.status.arr_pos - v.start as f32) / 4.0 * 60.0 / self.project.bpm.max(1.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{name}  {:.2} / {:.1} s", secs.max(0.0), v.duration)).size(10.5).color(TEXT_DIM));
                if ui.small_button("Remove video").clicked() {
                    self.project.video = None;
                    self.touch();
                }
            });
        });
        if !open {
            self.video.open = false;
        }
    }

    /// Thumbnail strip on the timeline; drag it to move the video.
    pub(super) fn video_lane(&mut self, ui: &mut egui::Ui, r: Rect, x_of: &dyn Fn(f32, f32) -> f32) {
        let Some(v) = self.project.video.clone() else { return };
        let p = ui.painter().with_clip_rect(r);
        p.rect_filled(r, 0.0, Color32::from_rgb(0x1a, 0x1a, 0x1a));
        let sx = self.arr.scroll_x;
        let len_steps = v.duration * self.project.bpm / 60.0 * 4.0;
        let x0 = x_of(v.start as f32, sx);
        let x1 = x_of(v.start as f32 + len_steps, sx);
        let thumb_w = (r.height() - 4.0) * 16.0 / 9.0;
        let mut x = x0.max(r.left() - thumb_w);
        let ctx = ui.ctx().clone();
        while x < x1.min(r.right()) {
            let step = (x - r.left()) / self.arr.pps + sx;
            if let Some(i) = self.frame_index(&v, step) {
                let key = i / 4;
                if !self.video.thumbs.contains_key(&key) && self.video.thumbs.len() < 400 {
                    if let Some(t) = Self::load_texture(&ctx, &v, key * 4, &format!("thumb{key}")) {
                        self.video.thumbs.insert(key, t);
                    }
                }
                if let Some(t) = self.video.thumbs.get(&key) {
                    let rr = Rect::from_min_size(pos2(x, r.top() + 2.0), vec2(thumb_w, r.height() - 4.0));
                    p.image(t.id(), rr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
            x += thumb_w + 1.0;
        }
        p.rect_stroke(Rect::from_x_y_ranges(x0..=x1, r.top() + 1.0..=r.bottom() - 1.0), 2.0, Stroke::new(1.0_f32, ACCENT), egui::StrokeKind::Inside);
        let resp = ui.interact(r, ui.id().with("video_lane"), Sense::click_and_drag());
        if resp.dragged() {
            let d = resp.drag_delta().x / self.arr.pps;
            if let Some(vv) = self.project.video.as_mut() {
                vv.start = (vv.start as f32 + d).max(0.0) as u32;
            }
            self.touch();
        }
        if resp.double_clicked() {
            self.video.open = true;
        }
        self.hint(&resp, "Video", "The imported video. Drag to move it on the timeline, double-click to show the picture window. Export with 'MP4 with video' to get the finished clip.");
    }
}
