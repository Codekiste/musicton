//! Left-hand browser: sounds, instruments, effects, clips and samples.

use super::theme::*;
use super::{App, Payload};
use crate::model::{DeviceKind, InstrumentKind};
use crate::presets::{INSTRUMENT_PRESETS, PATTERNS};
use egui::{RichText, Sense};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Sounds,
    Drums,
    Instruments,
    AudioEffects,
    MidiEffects,
    Clips,
    Samples,
}

const CATEGORIES: [(Category, &str, &str); 7] = [
    (Category::Sounds, "Sounds", "All instrument presets."),
    (Category::Drums, "Drums", "Drum machine kits and drum patterns."),
    (Category::Instruments, "Instruments", "The raw instruments: Drum Machine, Acid Bass and Poly Synth."),
    (Category::AudioEffects, "Audio Effects", "Effects you can drop onto any track, return or the master."),
    (Category::MidiEffects, "MIDI Effects", "Note processors that sit before the instrument: arpeggiator, chords, scales and more."),
    (Category::Clips, "Clips", "Ready-made MIDI patterns. Drop them onto a clip slot or the arrangement."),
    (Category::Samples, "Samples", "WAV files from your sample folders. Drop them onto a drum lane in the Device Chain."),
];

pub struct BrowserState {
    pub cat: Category,
    pub search: String,
    pub dirs: Vec<PathBuf>,
    pub samples: Vec<PathBuf>,
    pub scanned: bool,
}

pub fn user_sample_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("musicton").join("samples")
}

impl BrowserState {
    pub fn new() -> Self {
        let mut dirs = vec![user_sample_dir()];
        if let Some(home) = dirs::home_dir() {
            for d in ["Samples", "samples", "Music/Samples", "Musik/Samples"] {
                dirs.push(home.join(d));
            }
        }
        if let Some(m) = dirs::audio_dir() {
            dirs.push(m);
        }
        dirs.dedup();
        Self { cat: Category::Sounds, search: String::new(), dirs, samples: vec![], scanned: false }
    }

    pub fn rescan(&mut self) {
        self.samples.clear();
        for d in self.dirs.clone() {
            scan(&d, 0, &mut self.samples);
        }
        self.samples.sort();
        self.samples.dedup();
        self.scanned = true;
    }
}

fn scan(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 5 || out.len() > 5000 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan(&p, depth + 1, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("wav")) {
            out.push(p);
        }
    }
}

impl App {
    pub(super) fn browser_panel(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.browser.search).hint_text("Search").desired_width(f32::INFINITY));
        ui.add_space(4.0);
        ui.label(RichText::new("Categories").size(10.5).color(TEXT_DIM));
        for (cat, name, info) in CATEGORIES {
            let r = ui.selectable_label(self.browser.cat == cat, name);
            if r.clicked() {
                self.browser.cat = cat;
                if cat == Category::Samples && !self.browser.scanned {
                    self.browser.rescan();
                }
            }
            self.hint(&r, name, info);
        }
        ui.separator();
        let search = self.browser.search.to_lowercase();
        let matches = |s: &str| search.is_empty() || s.to_lowercase().contains(&search);
        egui::ScrollArea::vertical().drag_to_scroll(false).id_salt("browser_items").auto_shrink([false, false]).show(ui, |ui| {
            match self.browser.cat {
                Category::Sounds | Category::Drums | Category::Instruments => {
                    let filter: &dyn Fn(InstrumentKind) -> bool = match self.browser.cat {
                        Category::Drums => &|k| k == InstrumentKind::Drums,
                        _ => &|_| true,
                    };
                    let raw = self.browser.cat == Category::Instruments;
                    for kind in InstrumentKind::ALL {
                        if !filter(kind) {
                            continue;
                        }
                        if raw {
                            let first = INSTRUMENT_PRESETS.iter().find(|p| p.kind == kind).unwrap();
                            if matches(kind.name()) {
                                self.browser_item(ui, kind.name(), Payload::Instrument(first.name), kind.info());
                            }
                            continue;
                        }
                        ui.label(RichText::new(kind.name()).size(10.5).color(TEXT_DIM));
                        for p in INSTRUMENT_PRESETS.iter().filter(|p| p.kind == kind && matches(p.name)) {
                            self.browser_item(ui, p.name, Payload::Instrument(p.name), p.info);
                        }
                    }
                    if self.browser.cat == Category::Drums {
                        ui.label(RichText::new("Drum Patterns").size(10.5).color(TEXT_DIM));
                        for p in PATTERNS.iter().filter(|p| p.kind == InstrumentKind::Drums && matches(p.name)) {
                            self.browser_item(ui, p.name, Payload::Pattern(p.name), "Drum pattern. Drop onto a drum track clip slot.");
                        }
                    }
                }
                Category::AudioEffects => {
                    for (folder, kinds) in crate::model::DEVICE_CATEGORIES {
                        let visible: Vec<DeviceKind> = kinds.iter().copied().filter(|k| matches(k.name())).collect();
                        if visible.is_empty() {
                            continue;
                        }
                        let open = !search.is_empty();
                        egui::CollapsingHeader::new(RichText::new(*folder).size(11.5))
                            .id_salt(("fx_folder", *folder))
                            .default_open(true)
                            .open(if open { Some(true) } else { None })
                            .show(ui, |ui| {
                                for kind in visible {
                                    self.browser_item(ui, kind.name(), Payload::Device(kind), kind.info());
                                }
                            });
                    }
                }
                Category::MidiEffects => {
                    for kind in crate::model::MidiFxKind::ALL {
                        if matches(kind.name()) {
                            self.browser_item(ui, kind.name(), Payload::MidiFx(kind), kind.info());
                        }
                    }
                }
                Category::Clips => {
                    for kind in [InstrumentKind::Drums, InstrumentKind::Bass, InstrumentKind::Synth, InstrumentKind::Sampler] {
                        ui.label(RichText::new(format!("{} Clips", kind.name())).size(10.5).color(TEXT_DIM));
                        for p in PATTERNS.iter().filter(|p| p.kind == kind && matches(p.name)) {
                            let info = format!("{} bar(s), for {} tracks. Drop onto a clip slot or the arrangement.", p.bars, kind.name());
                            self.browser_item(ui, p.name, Payload::Pattern(p.name), &info);
                        }
                    }
                }
                Category::Samples => {
                    ui.horizontal(|ui| {
                        if ui.small_button("Rescan").clicked() {
                            self.browser.rescan();
                        }
                        if ui.small_button("Add Folder...").clicked() {
                            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                self.browser.dirs.push(d);
                                self.browser.rescan();
                            }
                        }
                    });
                    if self.browser.samples.is_empty() {
                        ui.label(RichText::new(format!(
                            "No WAV files found.\nPut samples into\n{}\nor ~/Samples, or add a folder.",
                            user_sample_dir().display()
                        )).size(11.0).color(TEXT_DIM));
                    }
                    for path in self.browser.samples.clone() {
                        let name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                        if matches(&name) {
                            let info = format!("{}\n\nDrag into a clip slot or the arrangement of an audio track (or the empty area to create one), onto a Sampler, or onto a drum lane. Double-click loads it into the selected track.", path.display());
                            self.browser_item(ui, &name, Payload::Sample(path), &info);
                        }
                    }
                }
            }
        });
    }

    fn browser_item(&mut self, ui: &mut egui::Ui, name: &str, payload: Payload, info: &str) {
        // A full-width row (not a Label) so dragging never turns into text selection.
        let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::click_and_drag());
        if r.hovered() || r.dragged() {
            ui.painter().rect_filled(rect, 2.0, egui::Color32::from_white_alpha(if r.dragged() { 30 } else { 14 }));
            ui.ctx().set_cursor_icon(if r.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        ui.painter().with_clip_rect(rect).text(
            rect.left_center() + egui::vec2(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(12.0),
            TEXT,
        );
        if r.drag_started() && self.drag.is_none() {
            self.drag = Some(payload.clone());
        }
        if r.double_clicked() {
            self.browser_activate(payload);
        }
        self.hint(&r, name, info);
    }

    fn browser_activate(&mut self, payload: Payload) {
        match payload {
            Payload::Instrument(name) => {
                self.add_track_from_preset(name, None);
            }
            Payload::Device(kind) => self.add_device(self.sel, kind, None),
            Payload::MidiFx(kind) => match self.selected_track() {
                Some(t) => self.add_midi_fx(t, kind, None),
                None => self.toast("Select a track for the MIDI effect"),
            },
            Payload::Pattern(name) => {
                let Some(track) = self.selected_track() else {
                    self.toast("Select a track first");
                    return;
                };
                self.place_pattern(track, self.sel_scene, name);
            }
            Payload::Sample(path) => match self.selected_track() {
                Some(t) => self.drop_sample_on_slot(t, self.sel_scene, &path),
                None => self.toast("Select a track first"),
            },
            Payload::SessionClip { .. } => {}
        }
    }

    pub(super) fn place_pattern(&mut self, track: u64, scene: usize, name: &str) {
        let Some(pat) = crate::presets::pattern(name) else { return };
        let Some(t) = self.project.track(track) else { return };
        let fits = t.instrument.kind == pat.kind
            || (matches!(pat.kind, InstrumentKind::Bass | InstrumentKind::Synth) && t.instrument.kind.is_melodic());
        if !fits {
            self.toast(format!("\"{}\" is a {} pattern - drop it on a {} track", pat.name, pat.kind.name(), pat.kind.name()));
            return;
        }
        let clip = pat.clip(t.color);
        self.create_clip(track, scene, clip);
    }

    pub(super) fn assign_sample(&mut self, track: Option<u64>, lane: usize, path: &Path) {
        let Some(track) = track else {
            self.toast("Select a drum track first");
            return;
        };
        let p = path.to_string_lossy().to_string();
        if let Err(e) = crate::audio::samples::load(&p) {
            self.toast(format!("Could not load sample: {e}"));
            return;
        }
        let Some(t) = self.project.track_mut(track) else { return };
        if t.instrument.kind != InstrumentKind::Drums {
            self.toast("Samples can be loaded into drum machine lanes");
            return;
        }
        if let Some(slot) = t.instrument.samples.get_mut(lane) {
            *slot = Some(p);
            self.touch();
            self.toast(format!("Loaded sample into lane {}", crate::params::DRUM_LANES[lane]));
        }
    }
}
