//! Editors for the Multi Sampler (zones) and the Slicer (slice markers, slices to clip).

use super::theme::*;
use super::widgets;
use super::{App, Payload};
use crate::audio::samplers::{detect_slices, slice_notes};
use crate::model::*;
use crate::params::{self, slicer};
use egui::{pos2, vec2, Align2, FontId, RichText, Sense, Stroke};

impl App {
    pub(super) fn multi_sampler_ui(&mut self, ui: &mut egui::Ui, track: u64, inst: &mut Instrument) {
        ui.horizontal(|ui| {
            if ui.button("Load Folder...").on_hover_text("Map every WAV in a folder; notes in the file names (C3, F#2 ...) become the zone roots").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    let mut files: Vec<String> = std::fs::read_dir(&dir)
                        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav"))).map(|p| p.to_string_lossy().to_string()).collect())
                        .unwrap_or_default();
                    files.sort();
                    files.retain(|f| crate::audio::samples::load(f).is_ok());
                    if files.is_empty() {
                        self.toast("No WAV files in that folder");
                    } else {
                        inst.zones = zones_from_files(&files);
                        self.toast(format!("Mapped {} zones", inst.zones.len()));
                    }
                }
            }
            if ui.button("Add WAVs...").clicked() {
                if let Some(paths) = rfd::FileDialog::new().add_filter("WAV", &["wav", "WAV"]).pick_files() {
                    let mut files: Vec<String> = inst.zones.iter().map(|z| z.path.clone()).collect();
                    files.extend(paths.iter().map(|p| p.to_string_lossy().to_string()).filter(|f| crate::audio::samples::load(f).is_ok()));
                    inst.zones = zones_from_files(&files);
                }
            }
            if !inst.zones.is_empty() && ui.button("Clear").clicked() {
                inst.zones.clear();
            }
        });
        // Keyboard overview of the zones.
        let (rect, resp) = ui.allocate_exact_size(vec2(520.0, 26.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2.0, KNOB_BG);
        let kx = |k: u8| rect.left() + k as f32 / 128.0 * rect.width();
        for (i, z) in inst.zones.iter().enumerate() {
            let col = color32(CLIP_COLORS[(i * 5) % CLIP_COLORS.len()]);
            let zr = egui::Rect::from_x_y_ranges(kx(z.lo)..=kx(z.hi.saturating_add(1)) - 1.0, rect.top() + 2.0..=rect.bottom() - 8.0);
            p.rect_filled(zr, 1.0, col.gamma_multiply(0.7));
            p.line_segment([pos2(kx(z.root) + 1.0, rect.bottom() - 8.0), pos2(kx(z.root) + 1.0, rect.bottom())], Stroke::new(2.0_f32, col));
        }
        for oct in 0..11 {
            p.text(pos2(kx(oct * 12), rect.bottom()), Align2::LEFT_BOTTOM, format!("C{}", oct as i32 - 2), FontId::proportional(7.5), TEXT_DIM);
        }
        if inst.zones.is_empty() {
            p.text(rect.center(), Align2::CENTER_CENTER, "No zones - load a folder, add WAVs or drop samples here", FontId::proportional(11.0), TEXT_DIM);
        }
        if let Some(Payload::Sample(path)) = self.drop_target(ui, rect, |p| matches!(p, Payload::Sample(_))) {
            let f = path.to_string_lossy().to_string();
            if crate::audio::samples::load(&f).is_ok() {
                let mut files: Vec<String> = inst.zones.iter().map(|z| z.path.clone()).collect();
                files.push(f);
                inst.zones = zones_from_files(&files);
            }
        }
        self.hint(&resp, "Zones", "Each coloured bar is a key zone; the tick marks its root note (where the sample plays at original pitch).");
        // Zone table.
        let mut remove = None;
        egui::ScrollArea::vertical().max_height(70.0).id_salt(("zones", track)).show(ui, |ui| {
            egui::Grid::new(("zonegrid", track)).spacing(vec2(6.0, 1.0)).show(ui, |ui| {
                for h in ["Sample", "Root", "Low", "High", "Vel", "", "Tune", "Gain", ""] {
                    ui.label(RichText::new(h).size(9.0).color(TEXT_DIM));
                }
                ui.end_row();
                for (i, z) in inst.zones.iter_mut().enumerate() {
                    let name = std::path::Path::new(&z.path).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                    ui.label(RichText::new(name.chars().take(18).collect::<String>()).size(10.0));
                    ui.add(egui::DragValue::new(&mut z.root).range(0..=127).custom_formatter(|v, _| note_name(v as u8)));
                    ui.add(egui::DragValue::new(&mut z.lo).range(0..=127).custom_formatter(|v, _| note_name(v as u8)));
                    ui.add(egui::DragValue::new(&mut z.hi).range(0..=127).custom_formatter(|v, _| note_name(v as u8)));
                    ui.add(egui::DragValue::new(&mut z.vlo).range(0..=127));
                    ui.add(egui::DragValue::new(&mut z.vhi).range(0..=127));
                    ui.add(egui::DragValue::new(&mut z.tune).range(-100.0..=100.0).suffix(" ct"));
                    ui.add(egui::DragValue::new(&mut z.gain).range(-36.0..=12.0).speed(0.1).suffix(" dB"));
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(i) = remove {
            inst.zones.remove(i);
        }
    }

    pub(super) fn slicer_ui(&mut self, ui: &mut egui::Ui, track: u64, inst: &mut Instrument) {
        let path = inst.samples.first().cloned().flatten();
        let sample = path.as_deref().and_then(|p| crate::audio::samples::load(p).ok());
        let (rect, resp) = ui.allocate_exact_size(vec2(520.0, 64.0), Sense::click());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2.0, KNOB_BG);
        let mut slices = vec![];
        match (&sample, &path) {
            (Some(s), Some(pth)) => {
                let peaks = self.peaks(pth, s, rect.width() as usize);
                let mid = rect.center().y;
                for (i, (lo, hi)) in peaks.iter().enumerate() {
                    let x = rect.left() + i as f32;
                    p.line_segment([pos2(x, mid - hi * rect.height() * 0.46), pos2(x, mid - lo * rect.height() * 0.46)], Stroke::new(1.0_f32, BLUE));
                }
                slices = detect_slices(s, inst.params[slicer::MODE].round() as usize, inst.params[slicer::SENS]);
                let len = s.left.len().max(1);
                for (k, &f) in slices.iter().enumerate() {
                    let x = rect.left() + f as f32 / len as f32 * rect.width();
                    p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, ACCENT));
                    if slices.len() <= 32 || k % 2 == 0 {
                        p.text(pos2(x + 2.0, rect.top() + 1.0), Align2::LEFT_TOP, note_name(36 + k as u8), FontId::proportional(8.0), ACCENT);
                    }
                }
                p.text(rect.right_bottom() - vec2(4.0, 2.0), Align2::RIGHT_BOTTOM, format!("{} slices", slices.len()), FontId::proportional(10.0), TEXT_DIM);
            }
            _ => {
                p.text(rect.center(), Align2::CENTER_CENTER, "Drop a loop here (or click to load)", FontId::proportional(12.0), TEXT_DIM);
            }
        }
        let load = |s: String, inst: &mut Instrument, app: &mut App| match crate::audio::samples::load(&s) {
            Ok(_) => inst.samples[0] = Some(s),
            Err(e) => app.toast(format!("Could not load sample: {e}")),
        };
        if resp.clicked() {
            if let Some(pth) = rfd::FileDialog::new().add_filter("WAV", &["wav", "WAV"]).pick_file() {
                load(pth.to_string_lossy().to_string(), inst, self);
            }
        }
        if let Some(Payload::Sample(pth)) = self.drop_target(ui, rect, |p| matches!(p, Payload::Sample(_))) {
            load(pth.to_string_lossy().to_string(), inst, self);
        }
        self.hint(&resp, "Slicer", "Orange lines are the slices; slice 1 plays on C1, the next on C#1 and so on. Change 'Slice By' and 'Sensitivity' to re-chop.");
        let r = widgets::button(ui, "Slices to Clip", false);
        if r.clicked() {
            if let Some(s) = &sample {
                let beats = if inst.params[slicer::SYNC] > 0.5 {
                    inst.params[slicer::BEATS]
                } else {
                    s.left.len() as f32 / s.sample_rate * self.project.bpm / 60.0
                };
                let steps = ((beats * 4.0).round() as u32).max(1);
                let bars = steps.div_ceil(STEPS_PER_BAR).max(1);
                let mut clip = Clip::new("Slices", self.project.track(track).map(|t| t.color).unwrap_or(CLIP_COLORS[0]), bars);
                clip.notes = slice_notes(&slices, s.left.len(), steps);
                let scene = self.project.track(track).and_then(|t| t.clips.iter().position(|c| c.is_none()));
                match scene {
                    Some(sc) => self.create_clip(track, sc, clip),
                    None => self.toast("No free clip slot on this track"),
                }
            }
        }
        self.hint(&r, "Slices to Clip", "Creates a clip in the next free slot that plays every slice at its original position - the loop replays exactly, ready to rearrange.");
        let _ = params::SLICER_MODES;
    }
}
