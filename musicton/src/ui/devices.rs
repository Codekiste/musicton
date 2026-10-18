//! Device Chain: instrument panel and audio effect chain of the selected track.

use super::theme::*;
use super::widgets;
use super::{App, Payload, Sel};
use crate::audio::engine::Cmd;
use crate::model::*;
use crate::params::{self, DRUM_LANES, DRUM_LANE_PARAMS};
use egui::{vec2, Color32, RichText, Sense};

const KNOB: f32 = 34.0;

impl App {
    pub(super) fn device_detail(&mut self, ui: &mut egui::Ui) {
        let sel = self.sel;
        let (title, color) = match sel {
            Sel::Track(id) => match self.project.track(id) {
                Some(t) => (t.name.clone(), color32(t.color)),
                None => return,
            },
            Sel::Return(k) => (self.project.returns[k].name.clone(), color32(self.project.returns[k].color)),
            Sel::Master => ("Master".into(), Color32::from_rgb(0xff, 0x66, 0xc4)),
        };
        egui::ScrollArea::horizontal().drag_to_scroll(false).id_salt("devices_h").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    let (r, _) = ui.allocate_exact_size(vec2(6.0, ui.available_height().min(230.0)), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, color);
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).strong());
                    ui.horizontal_top(|ui| {
                        if let Sel::Track(id) = sel {
                            self.macro_panel(ui, id);
                            self.modulator_panel(ui, id);
                            let n_mfx = self.project.track(id).map(|t| t.midi_fx.len()).unwrap_or(0);
                            for i in 0..n_mfx {
                                self.midi_fx_panel(ui, id, i);
                            }
                            self.instrument_panel(ui, id);
                        }
                        let n = self.channel_mut(sel).map(|c| c.devices.len()).unwrap_or(0);
                        for i in 0..n {
                            self.effect_panel(ui, sel, i);
                        }
                        self.device_drop_zone(ui, sel, n);
                    });
                });
            });
        });
    }

    fn panel_frame<R>(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
        egui::Frame::default()
            .fill(PANEL_LIGHT)
            .stroke(egui::Stroke::new(1.0_f32, if selected { SELECT } else { LINE }))
            .corner_radius(3)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| ui.vertical(add).inner)
    }

    fn instrument_panel(&mut self, ui: &mut egui::Ui, track: u64) {
        let Some(t) = self.project.track(track) else { return };
        let kind = t.instrument.kind;
        if kind == InstrumentKind::Group {
            Self::panel_frame(ui, false, |ui| {
                ui.set_min_height(200.0);
                ui.set_width(200.0);
                ui.label(RichText::new("Group Track").strong().color(ACCENT));
                ui.add_space(6.0);
                ui.label(RichText::new("All tracks in this group are summed here. Effects added to the right process the whole group (e.g. a Bus Compressor on all drums).").size(11.0).color(TEXT_DIM));
            });
            return;
        }
        if kind == InstrumentKind::Audio {
            Self::panel_frame(ui, false, |ui| {
                ui.set_min_height(200.0);
                ui.set_width(200.0);
                ui.label(RichText::new("Audio Track").strong().color(ACCENT));
                ui.add_space(6.0);
                ui.label(RichText::new("Drag WAV loops and samples from the browser into this track's clip slots or arrangement lane. They are warped to the song tempo.\n\nAdd audio effects to the right.").size(11.0).color(TEXT_DIM));
            });
            return;
        }
        if matches!(kind, InstrumentKind::B303 | InstrumentKind::Nexa) {
            let mut inst = t.instrument.clone();
            let before = inst.params.clone();
            self.plugin_device(ui, track, &mut inst);
            if inst.params != before {
                if let Some(t) = self.project.track_mut(track) {
                    t.instrument = inst;
                }
                self.touch();
            }
            return;
        }
        let mut inst = t.instrument.clone();
        let before = inst.params.clone();
        let before_samples = inst.samples.clone();
        Self::panel_frame(ui, false, |ui| {
            ui.set_min_height(200.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(kind.name()).strong().color(ACCENT));
                ui.menu_button("Presets", |ui| {
                    for p in crate::presets::INSTRUMENT_PRESETS.iter().filter(|p| p.kind == kind) {
                        if ui.button(p.name).clicked() {
                            let samples = inst.samples.clone();
                            inst = p.build();
                            inst.samples = samples;
                            ui.close_menu();
                        }
                    }
                });
            });
            let defs = kind.defs();
            match kind {
                InstrumentKind::Drums => {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            egui::Grid::new("lanes").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                                for (i, name) in DRUM_LANES.iter().enumerate() {
                                    let short: String = name.chars().take(6).collect();
                                    let has_sample = inst.samples.get(i).is_some_and(|s| s.is_some());
                                    let label = if has_sample { format!("{short}*") } else { short };
                                    let r = widgets::toggle(ui, &label, self.drum_lane == i, ACCENT, vec2(52.0, 20.0));
                                    if r.clicked() {
                                        self.drum_lane = i;
                                        self.send(Cmd::NoteOn { track, pitch: i as u8, vel: 110 });
                                    }
                                    if let Some(Payload::Sample(path)) = self.drop_target(ui, r.rect, |p| matches!(p, Payload::Sample(_))) {
                                        self.drum_lane = i;
                                        let pth = path.to_string_lossy().to_string();
                                        if crate::audio::samples::load(&pth).is_ok() {
                                            inst.samples[i] = Some(pth);
                                        }
                                    }
                                    self.hint(&r, name, "Drum lane (pad). Click to select and audition it. Drop a WAV sample onto it to replace the synthesized sound. * = sample loaded.");
                                    if i % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                        });
                        ui.separator();
                        let lane = self.drum_lane.min(DRUM_LANES.len() - 1);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(DRUM_LANES[lane]).color(ACCENT));
                            ui.horizontal(|ui| {
                                for p in 0..DRUM_LANE_PARAMS {
                                    let idx = params::lane_param(lane, p);
                                    let r = widgets::param(ui, ("drum", idx), &mut inst.params[idx], &defs[idx], KNOB);
                                    self.param_hint(&r, &defs[idx], "Drum lane parameter.");
                                    r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                                }
                            });
                            let sample = inst.samples.get(lane).cloned().flatten();
                            ui.horizontal(|ui| {
                                let name = sample
                                    .as_deref()
                                    .map(|s| std::path::Path::new(s).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
                                    .unwrap_or_else(|| "Synthesized".into());
                                ui.label(RichText::new(name).size(10.5).color(TEXT_DIM));
                                if ui.small_button("Load WAV...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().add_filter("WAV", &["wav", "WAV"]).pick_file() {
                                        let pth = path.to_string_lossy().to_string();
                                        match crate::audio::samples::load(&pth) {
                                            Ok(_) => inst.samples[lane] = Some(pth),
                                            Err(e) => self.toast(format!("Could not load sample: {e}")),
                                        }
                                    }
                                }
                                if sample.is_some() && ui.small_button("Synth").on_hover_text("Go back to the synthesized sound").clicked() {
                                    inst.samples[lane] = None;
                                }
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                for idx in 0..params::DRUM_KIT_PARAMS {
                                    let r = widgets::param(ui, ("kit", idx), &mut inst.params[idx], &defs[idx], KNOB);
                                    self.param_hint(&r, &defs[idx], "Applies to the whole kit.");
                                    r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                                }
                            });
                        });
                    });
                }
                InstrumentKind::Fm4 => {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            for (k, name) in ["A", "B", "C", "D"].iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(*name).strong().color(ACCENT));
                                    for j in 0..params::OP_PARAMS {
                                        let idx = k * params::OP_PARAMS + j;
                                        let r = widgets::knob(ui, &mut inst.params[idx], &defs[idx], 22.0, false);
                                        r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                                        let txt = format!("Fm4 {name} {}: {}", defs[idx].name, defs[idx].format(inst.params[idx]));
                                        self.param_hint(&r, &defs[idx], &txt);
                                        if r.changed() {
                                            self.info = Some((format!("Op {name} {}", defs[idx].name), defs[idx].format(inst.params[idx])));
                                        }
                                    }
                                });
                            }
                            ui.label(RichText::new("Ratio · Fine · Level · A · D · S · R").size(9.5).color(TEXT_DIM));
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            let start = 4 * params::OP_PARAMS;
                            for row in [start..start + 3, start + 3..defs.len()] {
                                ui.horizontal(|ui| {
                                    for idx in row {
                                        let r = widgets::param(ui, ("inst", track, idx), &mut inst.params[idx], &defs[idx], KNOB);
                                        r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                                        self.param_hint(&r, &defs[idx], "Fm4 global parameter. The algorithm decides which operators modulate which (carriers are heard, modulators shape the timbre).");
                                    }
                                });
                            }
                        });
                    });
                }
                _ => {
                    if kind == InstrumentKind::Sampler {
                        self.simpler_sample_view(ui, track, &mut inst);
                    }
                    let rows = defs.len().div_ceil(12).max(2);
                    let cols = defs.len().div_ceil(rows);
                    for row in 0..rows {
                        ui.horizontal(|ui| {
                            for idx in row * cols..((row + 1) * cols).min(defs.len()) {
                                let r = widgets::param(ui, ("inst", track, idx), &mut inst.params[idx], &defs[idx], KNOB);
                                self.param_hint(&r, &defs[idx], instrument_param_info(kind, idx));
                                r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                            }
                        });
                    }
                }
            }
        });
        if let Some(Payload::MidiFx(kind)) = self.drop_target(ui, ui.min_rect(), |p| matches!(p, Payload::MidiFx(_))) {
            self.add_midi_fx(track, kind, None);
        }
        if inst.params != before || inst.samples != before_samples {
            if let Some(t) = self.project.track_mut(track) {
                t.instrument = inst;
            }
            self.touch();
        }
    }

    fn macro_panel(&mut self, ui: &mut egui::Ui, track: u64) {
        let Some(t) = self.project.track(track) else { return };
        let macros = t.macros.clone();
        let def = params::ParamDef::new("Macro", 0.0, 1.0, 0.0, params::Unit::Pct);
        egui::Frame::default()
            .fill(Color32::from_rgb(0x30, 0x34, 0x3a))
            .stroke(egui::Stroke::new(1.0_f32, LINE))
            .corner_radius(3)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_min_height(200.0);
                    ui.label(RichText::new("Macros").strong().color(TEXT_DIM));
                    egui::Grid::new(("macros", track)).spacing(vec2(2.0, 4.0)).show(ui, |ui| {
                        for (i, m) in macros.iter().enumerate() {
                            ui.vertical(|ui| {
                                let mut v = m.value;
                                let r = widgets::knob(ui, &mut v, &def, 28.0, false);
                                let mapped = !m.targets.is_empty();
                                let name: String = m.name.chars().take(9).collect();
                                ui.label(RichText::new(name).size(9.5).color(if mapped { ACCENT } else { TEXT_DIM }));
                                if r.changed() {
                                    self.set_macro(track, i, v);
                                }
                                let targets: Vec<String> = m.targets.iter().map(|(t, _, _)| self.target_name(*t)).collect();
                                let info = if targets.is_empty() {
                                    "Macro knob - one knob that moves many parameters. Right-click any parameter and choose \"Map to Macro\".".to_string()
                                } else {
                                    format!("Controls:\n{}", targets.join("\n"))
                                };
                                self.hint(&r, &m.name, &info);
                                r.context_menu(|ui| {
                                    if !m.targets.is_empty() && ui.button("Clear Macro Mappings").clicked() {
                                        if let Some(mm) = self.project.track_mut(track).and_then(|t| t.macros.get_mut(i)) {
                                            mm.targets.clear();
                                        }
                                        self.touch();
                                        ui.close_menu();
                                    }
                                    let mut name = m.name.clone();
                                    if ui.text_edit_singleline(&mut name).changed() {
                                        if let Some(mm) = self.project.track_mut(track).and_then(|t| t.macros.get_mut(i)) {
                                            mm.name = name;
                                        }
                                        self.touch();
                                    }
                                    ui.separator();
                                    self.map_menu(ui, MapTarget::Macro(track, i as u8), None);
                                });
                            });
                            if i % 4 == 3 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
    }

    fn midi_fx_panel(&mut self, ui: &mut egui::Ui, track: u64, i: usize) {
        let Some(m) = self.project.track(track).and_then(|t| t.midi_fx.get(i).cloned()) else { return };
        let mut d = m.clone();
        let defs = d.kind.defs();
        let mut action: Option<i32> = None;
        let resp = egui::Frame::default()
            .fill(Color32::from_rgb(0x3a, 0x40, 0x48))
            .stroke(egui::Stroke::new(1.0_f32, LINE))
            .corner_radius(3)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_min_height(200.0);
                    ui.horizontal(|ui| {
                        let r = widgets::toggle(ui, "●", d.on, ACCENT, vec2(18.0, 17.0));
                        if r.clicked() {
                            d.on = !d.on;
                        }
                        self.hint(&r, "Device Activator", "Turns the MIDI effect on or off.");
                        let r = ui.label(RichText::new(d.kind.name()).strong().color(if d.on { TEXT } else { TEXT_DIM }));
                        self.hint(&r, d.kind.name(), d.kind.info());
                        ui.add_space(8.0);
                        if ui.small_button("◀").clicked() {
                            action = Some(-1);
                        }
                        if ui.small_button("▶").clicked() {
                            action = Some(1);
                        }
                        if ui.small_button("×").clicked() {
                            action = Some(0);
                        }
                    });
                    let rows = if defs.len() > 4 { 2 } else { 1 };
                    let cols = defs.len().div_ceil(rows);
                    for row in 0..rows {
                        ui.horizontal(|ui| {
                            for idx in row * cols..((row + 1) * cols).min(defs.len()) {
                                let r = widgets::param(ui, ("mfx", d.id, idx), &mut d.params[idx], &defs[idx], KNOB);
                                self.param_hint(&r, &defs[idx], d.kind.info());
                                r.context_menu(|ui| self.map_menu(ui, MapTarget::MidiFx(d.id, idx), Some(track)));
                            }
                        });
                    }
                });
            });
        if let Some(Payload::MidiFx(kind)) = self.drop_target(ui, resp.response.rect, |p| matches!(p, Payload::MidiFx(_))) {
            self.add_midi_fx(track, kind, Some(i));
            return;
        }
        let Some(t) = self.project.track_mut(track) else { return };
        match action {
            Some(0) => t.midi_fx.remove(i).id,
            Some(-1) if i > 0 => {
                t.midi_fx.swap(i, i - 1);
                0
            }
            Some(1) if i + 1 < t.midi_fx.len() => {
                t.midi_fx.swap(i, i + 1);
                0
            }
            _ => {
                if d.on == m.on && d.params == m.params {
                    return;
                }
                t.midi_fx[i] = d;
                0
            }
        };
        self.touch();
    }

    fn simpler_sample_view(&mut self, ui: &mut egui::Ui, track: u64, inst: &mut Instrument) {
        use params::smp;
        let path = inst.samples.first().cloned().flatten();
        let (rect, resp) = ui.allocate_exact_size(vec2(520.0, 58.0), Sense::click());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2.0, KNOB_BG);
        match path.as_deref().and_then(|pth| crate::audio::samples::load(pth).ok()) {
            Some(sample) => {
                let peaks = self.peaks(path.as_deref().unwrap(), &sample, rect.width() as usize);
                let mid = rect.center().y;
                for (i, (lo, hi)) in peaks.iter().enumerate() {
                    let x = rect.left() + i as f32;
                    p.line_segment([egui::pos2(x, mid - hi * rect.height() * 0.48), egui::pos2(x, mid - lo * rect.height() * 0.48)], egui::Stroke::new(1.0_f32, BLUE));
                }
                let sx = rect.left() + inst.params[smp::START] * rect.width();
                let ex = rect.left() + inst.params[smp::END] * rect.width();
                p.rect_filled(egui::Rect::from_min_max(rect.min, egui::pos2(sx, rect.bottom())), 0.0, egui::Color32::from_black_alpha(140));
                p.rect_filled(egui::Rect::from_min_max(egui::pos2(ex, rect.top()), rect.max), 0.0, egui::Color32::from_black_alpha(140));
                if inst.params[smp::MODE].round() as i32 == 2 {
                    let n = inst.params[smp::SLICES].round().max(1.0) as usize;
                    for k in 0..n {
                        let x = sx + (ex - sx) * k as f32 / n as f32;
                        p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.0_f32, ACCENT));
                        p.text(egui::pos2(x + 2.0, rect.top() + 1.0), egui::Align2::LEFT_TOP, note_name(36 + k as u8), egui::FontId::proportional(8.0), ACCENT);
                    }
                }
                let name = std::path::Path::new(path.as_deref().unwrap()).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                let secs = sample.left.len() as f32 / sample.sample_rate;
                p.text(rect.right_bottom() - vec2(4.0, 2.0), egui::Align2::RIGHT_BOTTOM, format!("{name}  {secs:.2}s"), egui::FontId::proportional(10.0), TEXT_DIM);
            }
            None => {
                p.text(rect.center(), egui::Align2::CENTER_CENTER, "Drop a WAV sample here (or click to load)", egui::FontId::proportional(12.0), TEXT_DIM);
            }
        }
        if resp.clicked() {
            if let Some(pth) = rfd::FileDialog::new().add_filter("WAV", &["wav", "WAV"]).pick_file() {
                let s = pth.to_string_lossy().to_string();
                match crate::audio::samples::load(&s) {
                    Ok(_) => inst.samples[0] = Some(s),
                    Err(e) => self.toast(format!("Could not load sample: {e}")),
                }
            }
        }
        if let Some(Payload::Sample(pth)) = self.drop_target(ui, rect, |p| matches!(p, Payload::Sample(_))) {
            let s = pth.to_string_lossy().to_string();
            match crate::audio::samples::load(&s) {
                Ok(_) => inst.samples[0] = Some(s),
                Err(e) => self.toast(format!("Could not load sample: {e}")),
            }
        }
        let _ = track;
        self.hint(&resp, "Sampler Sample", "The loaded sample. Shaded areas are outside Start/End. In Slice mode the orange lines show which key (from C1) plays which slice. Click to load a WAV or drop one from the browser.");
    }

    /// Min/max peaks of a sample for waveform drawing (cached per path and width).
    pub(super) fn peaks(&mut self, path: &str, s: &crate::audio::samples::Sample, width: usize) -> std::sync::Arc<Vec<(f32, f32)>> {
        let key = (path.to_string(), width);
        if let Some(p) = self.peak_cache.get(&key) {
            return p.clone();
        }
        let n = s.left.len().max(1);
        let w = width.max(1);
        let v: Vec<(f32, f32)> = (0..w)
            .map(|i| {
                let a = i * n / w;
                let b = ((i + 1) * n / w).max(a + 1).min(n);
                let step = ((b - a) / 64).max(1);
                let mut lo: f32 = 0.0;
                let mut hi: f32 = 0.0;
                for j in (a..b).step_by(step) {
                    let x = (s.left[j] + s.right[j]) * 0.5;
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
                (lo, hi)
            })
            .collect();
        let arc = std::sync::Arc::new(v);
        self.peak_cache.insert(key, arc.clone());
        arc
    }

    pub(super) fn param_hint(&mut self, r: &egui::Response, def: &params::ParamDef, info: &str) {
        if r.hovered() || r.dragged() {
            self.info = Some((
                def.name.to_string(),
                format!("{info}\n\nRange: {} – {}\nDrag up/down (Shift: fine), mouse wheel, double-click to reset.", def.format(def.min), def.format(def.max)),
            ));
        }
    }

    fn effect_panel(&mut self, ui: &mut egui::Ui, sel: Sel, i: usize) {
        let Some(dev) = self.channel_mut(sel).and_then(|c| c.devices.get(i).cloned()) else { return };
        let mut d = dev.clone();
        let defs = d.kind.defs();
        let reduction = self.status.reductions.iter().find(|(id, _)| *id == d.id).map(|(_, r)| *r);
        let selected = self.sel_device == Some(d.id);
        let mut action: Option<i32> = None;
        let mut dirty = false;
        let own_track = if let Sel::Track(t) = sel { Some(t) } else { None };
        let resp = Self::panel_frame(ui, selected, |ui| {
            ui.set_min_height(200.0);
            ui.horizontal(|ui| {
                let r = widgets::toggle(ui, "●", d.on, ACCENT, vec2(18.0, 17.0));
                if r.clicked() {
                    d.on = !d.on;
                }
                self.hint(&r, "Device Activator", "Turns the device on or off (bypass).");
                let r = ui.add(egui::Label::new(RichText::new(d.kind.name()).strong().color(if d.on { TEXT } else { TEXT_DIM })).sense(Sense::click()));
                if r.clicked() {
                    self.sel_device = Some(d.id);
                }
                self.hint(&r, d.kind.name(), d.kind.info());
                ui.add_space(8.0);
                if ui.small_button("◀").on_hover_text("Move left").clicked() {
                    action = Some(-1);
                }
                if ui.small_button("▶").on_hover_text("Move right").clicked() {
                    action = Some(1);
                }
                if ui.small_button("×").on_hover_text("Delete device").clicked() {
                    action = Some(0);
                }
                if d.kind.has_sidechain() {
                    dirty |= self.sidechain_combo(ui, &mut d, own_track);
                }
            });
            let rows = if defs.len() > 10 { 3 } else { 2 };
            let cols = defs.len().div_ceil(rows).max(1);
            ui.horizontal_top(|ui| {
                match d.kind {
                    DeviceKind::Eq8 => self.eq8_ui(ui, &mut d),
                    DeviceKind::Analyzer => self.spectrum_ui(ui, &mut d),
                    DeviceKind::Tuner => self.tuner_ui(ui, &mut d),
                    DeviceKind::Rack => dirty |= self.rack_ui(ui, &mut d),
                    _ => {
                        ui.vertical(|ui| {
                            for row in 0..rows {
                                ui.horizontal(|ui| {
                                    for idx in row * cols..((row + 1) * cols).min(defs.len()) {
                                        let r = widgets::param(ui, ("dev", d.id, idx), &mut d.params[idx], &defs[idx], KNOB);
                                        self.param_hint(&r, &defs[idx], d.kind.info());
                                        let sel_track = if let Sel::Track(t) = sel { Some(t) } else { None };
                                        r.context_menu(|ui| self.map_menu(ui, MapTarget::Device(d.id, idx), sel_track));
                                    }
                                });
                            }
                        });
                    }
                }
                if let Some(gr) = reduction {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("GR").size(9.0).color(TEXT_DIM));
                        let (rect, r) = ui.allocate_exact_size(vec2(8.0, 120.0), Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, KNOB_BG);
                        let h = (gr / 24.0).clamp(0.0, 1.0) * rect.height();
                        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, vec2(8.0, h)), 0.0, ACCENT);
                        ui.label(RichText::new(format!("{gr:.1}")).size(9.0).color(TEXT_DIM));
                        self.hint(&r, "Gain Reduction", "How many dB the device is currently turning the signal down.");
                    });
                }
            });
        });
        if resp.response.clicked() {
            self.sel_device = Some(d.id);
        }
        // Drop an effect onto a device to insert it before.
        if let Some(Payload::Device(kind)) = self.drop_target(ui, resp.response.rect, |p| matches!(p, Payload::Device(_))) {
            self.add_device(sel, kind, Some(i));
            return;
        }
        let Some(ch) = self.channel_mut(sel) else { return };
        match action {
            Some(0) => {
                ch.devices.remove(i);
                self.touch();
            }
            Some(-1) if i > 0 => {
                ch.devices.swap(i, i - 1);
                self.touch();
            }
            Some(1) if i + 1 < ch.devices.len() => {
                ch.devices.swap(i, i + 1);
                self.touch();
            }
            _ => {
                if dirty || d.on != dev.on || d.params != dev.params {
                    ch.devices[i] = d;
                    self.touch();
                }
            }
        }
    }

    fn device_drop_zone(&mut self, ui: &mut egui::Ui, sel: Sel, n: usize) {
        let (rect, resp) = ui.allocate_exact_size(vec2(170.0, 200.0), Sense::click());
        ui.painter().rect_stroke(rect.shrink(2.0), 3.0, egui::Stroke::new(1.0_f32, GRID_STRONG), egui::StrokeKind::Inside);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop Audio Effects Here\n\n(or click to add)",
            egui::FontId::proportional(11.5),
            TEXT_DIM,
        );
        match self.drop_target(ui, rect, |p| matches!(p, Payload::Device(_) | Payload::MidiFx(_))) {
            Some(Payload::Device(kind)) => self.add_device(sel, kind, Some(n)),
            Some(Payload::MidiFx(kind)) => {
                if let Sel::Track(t) = sel {
                    self.add_midi_fx(t, kind, None);
                }
            }
            _ => {}
        }
        let popup = ui.make_persistent_id(("add_device", format!("{sel:?}")));
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        egui::popup_below_widget(ui, popup, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(170.0);
            for (folder, kinds) in crate::model::DEVICE_CATEGORIES {
                ui.menu_button(*folder, |ui| {
                    for kind in kinds.iter().copied() {
                        if ui.button(kind.name()).on_hover_text(kind.info()).clicked() {
                            self.add_device(sel, kind, Some(n));
                            ui.memory_mut(|m| m.close_popup());
                            ui.close_menu();
                        }
                    }
                });
            }
        });
        self.hint(&resp, "Device Chain", "Audio effects are processed left to right. Drag effects from the browser here, or click to choose one.");
    }
}

fn instrument_param_info(kind: InstrumentKind, idx: usize) -> &'static str {
    use params::{bass, synth};
    match (kind, idx) {
        (InstrumentKind::Bass, bass::CUTOFF) => "Filter cutoff - the heart of the acid sound. Automate it in the Timeline for sweeps.",
        (InstrumentKind::Bass, bass::RES) => "Filter resonance. High values give the squelchy 303 scream.",
        (InstrumentKind::Bass, bass::ENV_MOD) => "How far the filter envelope opens the cutoff on every note.",
        (InstrumentKind::Bass, bass::DECAY) => "Filter envelope decay time.",
        (InstrumentKind::Bass, bass::ACCENT) => "Extra filter and volume punch for accented notes (velocity >= 110).",
        (InstrumentKind::Bass, bass::GLIDE) => "Slide time between notes that have Slide enabled.",
        (InstrumentKind::Bass, bass::SUB) => "Adds a square sub-oscillator one octave below.",
        (InstrumentKind::Bass, bass::DRIVE) => "Distortion after the filter.",
        (InstrumentKind::Synth, synth::DETUNE) => "Detune spread between unison voices.",
        (InstrumentKind::Synth, synth::VOICES) => "Number of stacked (unison) oscillators per note.",
        (InstrumentKind::Synth, synth::ENV_AMT) => "How far the envelope opens the filter.",
        (InstrumentKind::Synth, synth::ATTACK | synth::DECAY | synth::SUSTAIN | synth::RELEASE) => "Amplitude and filter envelope (ADSR).",
        _ => "Instrument parameter.",
    }
}

// ---------------------------------------------------------------------------------------------
// Custom device displays

const EQ_W: f32 = 340.0;
const EQ_H: f32 = 130.0;

fn freq_to_x(f: f32, r: egui::Rect) -> f32 {
    r.left() + (f / 20.0).log10() / 3.0 * r.width()
}
fn x_to_freq(x: f32, r: egui::Rect) -> f32 {
    20.0 * 10f32.powf(((x - r.left()) / r.width()).clamp(0.0, 1.0) * 3.0)
}

impl App {
    fn eq8_ui(&mut self, ui: &mut egui::Ui, d: &mut Device) {
        use crate::params::{EQ8_BAND_PARAMS, EQ8_TYPES};
        let defs = d.kind.defs();
        let sel_id = egui::Id::new(("eq8_band", d.id));
        let mut band: usize = ui.ctx().data(|m| m.get_temp(sel_id)).unwrap_or(2);
        ui.vertical(|ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(EQ_W, EQ_H), Sense::click_and_drag());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 2.0, KNOB_BG);
            for f in [50.0f32, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
                let x = freq_to_x(f, rect);
                p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.0_f32, egui::Color32::from_gray(40)));
            }
            let y_of = |db: f32| rect.center().y - db / 18.0 * (rect.height() / 2.0);
            for db in [-12.0f32, 0.0, 12.0] {
                let y = y_of(db);
                p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], egui::Stroke::new(1.0_f32, egui::Color32::from_gray(if db == 0.0 { 60 } else { 40 })));
            }
            let sr = self.status.sample_rate.max(44100.0);
            let bands = crate::audio::effects2::eq8_bands(&d.params, sr);
            let pts: Vec<egui::Pos2> = (0..=120)
                .map(|i| {
                    let x = rect.left() + i as f32 / 120.0 * rect.width();
                    let db = crate::audio::effects2::eq8_response(&bands, x_to_freq(x, rect), sr).clamp(-30.0, 30.0);
                    egui::pos2(x, y_of(db).clamp(rect.top(), rect.bottom()))
                })
                .collect();
            p.add(egui::Shape::line(pts, egui::Stroke::new(2.0_f32, ACCENT)));
            // Band handles.
            for b in 0..8 {
                let o = b * EQ8_BAND_PARAMS;
                let on = d.params[o] > 0.5;
                let typ = d.params[o + 1].round() as usize;
                let gain_shown = matches!(typ, 2 | 3 | 5);
                let c = egui::pos2(freq_to_x(d.params[o + 2], rect), if gain_shown { y_of(d.params[o + 3]) } else { rect.center().y });
                let col = if b == band { egui::Color32::WHITE } else if on { ACCENT } else { TEXT_DIM };
                p.circle_filled(c, 7.0, col);
                p.text(c, egui::Align2::CENTER_CENTER, (b + 1).to_string(), egui::FontId::proportional(9.0), TEXT_DARK);
            }
            if let Some(pos) = resp.interact_pointer_pos() {
                if resp.drag_started() || resp.clicked() {
                    // Pick the nearest band handle.
                    let mut best = (band, f32::MAX);
                    for b in 0..8 {
                        let o = b * EQ8_BAND_PARAMS;
                        let typ = d.params[o + 1].round() as usize;
                        let y = if matches!(typ, 2 | 3 | 5) { y_of(d.params[o + 3]) } else { rect.center().y };
                        let dist = (egui::pos2(freq_to_x(d.params[o + 2], rect), y) - pos).length();
                        if dist < best.1 {
                            best = (b, dist);
                        }
                    }
                    band = best.0;
                    d.params[band * EQ8_BAND_PARAMS] = 1.0;
                }
                if resp.dragged() {
                    let o = band * EQ8_BAND_PARAMS;
                    d.params[o + 2] = x_to_freq(pos.x, rect).clamp(20.0, 20000.0);
                    if matches!(d.params[o + 1].round() as usize, 2 | 3 | 5) {
                        d.params[o + 3] = ((rect.center().y - pos.y) / (rect.height() / 2.0) * 18.0).clamp(-15.0, 15.0);
                    }
                }
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll != 0.0 {
                    let o = band * EQ8_BAND_PARAMS + 4;
                    d.params[o] = (d.params[o] * if scroll > 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(0.1, 18.0);
                }
            }
            self.hint(&resp, "Parametric EQ Display", "Drag a numbered dot to change frequency and gain of that band, mouse wheel changes its Q. Click the band buttons below to switch bands on/off.");
            ui.horizontal(|ui| {
                for b in 0..8 {
                    let o = b * EQ8_BAND_PARAMS;
                    let on = d.params[o] > 0.5;
                    let r = widgets::toggle(ui, &(b + 1).to_string(), on, if b == band { egui::Color32::WHITE } else { ACCENT }, vec2(22.0, 16.0));
                    if r.clicked() {
                        if band == b {
                            d.params[o] = if on { 0.0 } else { 1.0 };
                        }
                        band = b;
                    }
                }
            });
        });
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("Band {}", band + 1)).color(ACCENT));
            let o = band * EQ8_BAND_PARAMS;
            ui.horizontal(|ui| {
                for k in 1..3 {
                    let r = widgets::param(ui, ("eq8", d.id, o + k), &mut d.params[o + k], &defs[o + k], KNOB);
                    self.param_hint(&r, &defs[o + k], d.kind.info());
                }
            });
            ui.horizontal(|ui| {
                for k in 3..5 {
                    let r = widgets::param(ui, ("eq8", d.id, o + k), &mut d.params[o + k], &defs[o + k], KNOB);
                    self.param_hint(&r, &defs[o + k], d.kind.info());
                }
                let oi = 8 * EQ8_BAND_PARAMS;
                let r = widgets::param(ui, ("eq8", d.id, oi), &mut d.params[oi], &defs[oi], KNOB);
                self.param_hint(&r, &defs[oi], "Output gain.");
            });
            let _ = EQ8_TYPES;
        });
        ui.ctx().data_mut(|m| m.insert_temp(sel_id, band));
    }

    fn spectrum_ui(&mut self, ui: &mut egui::Ui, d: &mut Device) {
        let data = self.status.displays.iter().find(|(id, _)| *id == d.id).map(|(_, v)| v.clone()).unwrap_or_default();
        ui.vertical(|ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(EQ_W, EQ_H + 20.0), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 2.0, KNOB_BG);
            for f in [100.0f32, 1000.0, 10000.0] {
                let x = freq_to_x(f, rect);
                p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.0_f32, egui::Color32::from_gray(45)));
                let label = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") };
                p.text(egui::pos2(x + 2.0, rect.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(9.0), TEXT_DIM);
            }
            if !data.is_empty() {
                let n = data.len();
                let pts: Vec<egui::Pos2> = data
                    .iter()
                    .enumerate()
                    .map(|(i, db)| {
                        let x = rect.left() + (i as f32 + 0.5) / n as f32 * rect.width();
                        let y = rect.bottom() - ((db + 90.0) / 90.0).clamp(0.0, 1.0) * rect.height();
                        egui::pos2(x, y)
                    })
                    .collect();
                let mut fill = pts.clone();
                fill.push(rect.right_bottom());
                fill.push(rect.left_bottom());
                for w in pts.windows(2) {
                    p.add(egui::Shape::convex_polygon(
                        vec![w[0], w[1], egui::pos2(w[1].x, rect.bottom()), egui::pos2(w[0].x, rect.bottom())],
                        egui::Color32::from_rgba_unmultiplied(93, 184, 255, 70),
                        egui::Stroke::NONE,
                    ));
                }
                p.add(egui::Shape::line(pts, egui::Stroke::new(1.5_f32, BLUE)));
            } else {
                p.text(rect.center(), egui::Align2::CENTER_CENTER, "Play something to see its spectrum", egui::FontId::proportional(11.0), TEXT_DIM);
            }
            self.hint(&resp, "Analyzer", "Real-time frequency analysis of the signal at this point in the chain (20 Hz - 20 kHz, 0 to -90 dB).");
        });
        let defs = d.kind.defs();
        let r = widgets::param(ui, ("spec", d.id), &mut d.params[0], &defs[0], KNOB);
        self.param_hint(&r, &defs[0], "How slowly the display falls back.");
    }

    fn tuner_ui(&mut self, ui: &mut egui::Ui, d: &mut Device) {
        let data = self.status.displays.iter().find(|(id, _)| *id == d.id).map(|(_, v)| v.clone()).unwrap_or_default();
        ui.vertical(|ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(220.0, EQ_H), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 2.0, KNOB_BG);
            if data.len() >= 3 && data[1] > 0.0 {
                let (freq, reference) = (data[0], data[2]);
                let midi = 69.0 + 12.0 * (freq / reference).log2();
                let nearest = midi.round();
                let cents = (midi - nearest) * 100.0;
                let in_tune = cents.abs() < 5.0;
                p.text(rect.center() - vec2(0.0, 18.0), egui::Align2::CENTER_CENTER, note_name(nearest.clamp(0.0, 127.0) as u8), egui::FontId::proportional(36.0), if in_tune { PLAY } else { TEXT });
                p.text(rect.center() + vec2(0.0, 16.0), egui::Align2::CENTER_CENTER, format!("{cents:+.0} cents   {freq:.1} Hz"), egui::FontId::proportional(11.0), TEXT_DIM);
                let bar_y = rect.bottom() - 18.0;
                p.line_segment([egui::pos2(rect.left() + 10.0, bar_y), egui::pos2(rect.right() - 10.0, bar_y)], egui::Stroke::new(1.0_f32, TEXT_DIM));
                let x = rect.center().x + cents / 50.0 * (rect.width() / 2.0 - 10.0);
                p.circle_filled(egui::pos2(x, bar_y), 5.0, if in_tune { PLAY } else { ACCENT });
                p.line_segment([egui::pos2(rect.center().x, bar_y - 6.0), egui::pos2(rect.center().x, bar_y + 6.0)], egui::Stroke::new(1.0_f32, TEXT));
            } else {
                p.text(rect.center(), egui::Align2::CENTER_CENTER, "No pitch detected", egui::FontId::proportional(12.0), TEXT_DIM);
            }
            self.hint(&resp, "Tuner", "Detects the pitch of monophonic audio and shows how far it is from the nearest note.");
        });
        let defs = d.kind.defs();
        let r = widgets::param(ui, ("tuner", d.id), &mut d.params[0], &defs[0], KNOB);
        self.param_hint(&r, &defs[0], "Reference pitch for A4.");
    }
}
