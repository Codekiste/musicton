//! Beat Pads editor: 4x4 pad grid, the selected pad's settings and its effect chain.

use super::theme::*;
use super::widgets;
use super::{App, Payload};
use crate::audio::engine::Cmd;
use crate::model::*;
use crate::params::{self, ParamDef, Unit};
use egui::{vec2, Color32, RichText};

impl App {
    pub(super) fn beat_pads_ui(&mut self, ui: &mut egui::Ui, track: u64, inst: &mut Instrument) {
        let sel = self.drum_lane.min(15);
        ui.horizontal_top(|ui| {
            // Pad grid, bottom row = pads 1-4 like a hardware pad controller.
            ui.vertical(|ui| {
                for row in (0..4).rev() {
                    ui.horizontal(|ui| {
                        for col in 0..4 {
                            let i = row * 4 + col;
                            let Some(pad) = inst.pads.get(i).cloned() else { continue };
                            let label: String = pad.name.chars().take(8).collect();
                            let color = if pad.sample.is_some() { BLUE } else { ACCENT };
                            let r = widgets::toggle(ui, &format!("{}\n{label}", i + 1), sel == i, color, vec2(54.0, 38.0));
                            if r.clicked() {
                                self.drum_lane = i;
                                self.send(Cmd::NoteOn { track, pitch: i as u8, vel: 110 });
                            }
                            if let Some(Payload::Sample(path)) = self.drop_target(ui, r.rect, |p| matches!(p, Payload::Sample(_))) {
                                let p = path.to_string_lossy().to_string();
                                if crate::audio::samples::load(&p).is_ok() {
                                    inst.pads[i].sample = Some(p);
                                    inst.pads[i].name = path.file_stem().map(|f| f.to_string_lossy().chars().take(14).collect()).unwrap_or_default();
                                    self.drum_lane = i;
                                }
                            }
                            let info = format!(
                                "Pad {} - {}. Click to play and edit it, drop a WAV from the browser to load it.{}",
                                i + 1,
                                if pad.sample.is_some() { "sample" } else { "synthesized drum" },
                                if pad.choke > 0 { format!(" Choke group {}.", pad.choke) } else { String::new() }
                            );
                            self.hint(&r, &pad.name, &info);
                        }
                    });
                }
                ui.horizontal(|ui| {
                    let defs = inst.kind.defs();
                    for idx in 0..defs.len() {
                        let r = widgets::param(ui, ("pads", track, idx), &mut inst.params[idx], &defs[idx], 26.0);
                        self.param_hint(&r, &defs[idx], "Applies to all pads.");
                    }
                });
            });
            ui.separator();
            let Some(pad) = inst.pads.get_mut(sel) else { return };
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Pad {}", sel + 1)).strong().color(ACCENT));
                    ui.add(egui::TextEdit::singleline(&mut pad.name).desired_width(100.0));
                });
                ui.horizontal(|ui| {
                    match &pad.sample {
                        Some(s) => {
                            let name = std::path::Path::new(s).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                            ui.label(RichText::new(name).size(10.5).color(TEXT_DIM));
                            if ui.small_button("Synth").on_hover_text("Use a synthesized drum voice instead").clicked() {
                                pad.sample = None;
                            }
                        }
                        None => {
                            egui::ComboBox::from_id_salt(("padvoice", track, sel))
                                .width(90.0)
                                .selected_text(params::DRUM_LANES[pad.voice as usize % params::DRUM_LANES.len()])
                                .show_ui(ui, |ui| {
                                    for (k, n) in params::DRUM_LANES.iter().enumerate() {
                                        if ui.selectable_label(pad.voice as usize == k, *n).clicked() {
                                            pad.voice = k as u8;
                                        }
                                    }
                                });
                        }
                    }
                    if ui.small_button("Load WAV...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("WAV", &["wav", "WAV"]).pick_file() {
                            let p = path.to_string_lossy().to_string();
                            match crate::audio::samples::load(&p) {
                                Ok(_) => {
                                    pad.sample = Some(p);
                                    pad.name = path.file_stem().map(|f| f.to_string_lossy().chars().take(14).collect()).unwrap_or_default();
                                }
                                Err(e) => self.toast(format!("Could not load sample: {e}")),
                            }
                        }
                    }
                });
                let defs = [
                    ParamDef::new("Tune", -24.0, 24.0, 0.0, Unit::St),
                    ParamDef::new("Gain", -36.0, 12.0, 0.0, Unit::Db),
                    ParamDef::new("Pan", -1.0, 1.0, 0.0, Unit::Pan),
                    ParamDef::new("Decay", 0.02, 8.0, 2.0, Unit::Sec).log(),
                ];
                ui.horizontal(|ui| {
                    for (k, def) in defs.iter().enumerate() {
                        let v = match k {
                            0 => &mut pad.tune,
                            1 => &mut pad.gain,
                            2 => &mut pad.pan,
                            _ => &mut pad.decay,
                        };
                        let r = widgets::param(ui, ("pad", track, sel, k), v, def, 30.0);
                        self.param_hint(&r, def, "Pad setting. Decay only affects samples.");
                    }
                    ui.vertical(|ui| {
                        egui::ComboBox::from_id_salt(("padchoke", track, sel))
                            .width(70.0)
                            .selected_text(if pad.choke == 0 { "No choke".to_string() } else { format!("Choke {}", pad.choke) })
                            .show_ui(ui, |ui| {
                                for c in 0..=4u8 {
                                    let label = if c == 0 { "No choke".to_string() } else { format!("Choke {c}") };
                                    if ui.selectable_label(pad.choke == c, label).clicked() {
                                        pad.choke = c;
                                    }
                                }
                            });
                        let r = widgets::toggle(ui, "Reverse", pad.reverse, ACCENT, vec2(60.0, 18.0));
                        if r.clicked() {
                            pad.reverse = !pad.reverse;
                        }
                    });
                });
                // Pad effect chain.
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Pad FX").size(10.0).color(TEXT_DIM));
                    let mut remove = None;
                    for (k, d) in pad.devices.iter_mut().enumerate() {
                        let open = self.rack.open == Some((u64::MAX - track, sel, k));
                        let col = if open { SELECT } else if d.on { PANEL_LIGHT } else { KNOB_BG };
                        let r = widgets::toggle(ui, d.kind.name(), open, col, vec2(8.0 + d.kind.name().len() as f32 * 6.0, 17.0));
                        if r.clicked() {
                            self.rack.open = if open { None } else { Some((u64::MAX - track, sel, k)) };
                        }
                        r.context_menu(|ui| {
                            if ui.button(if d.on { "Bypass" } else { "Activate" }).clicked() {
                                d.on = !d.on;
                                ui.close_menu();
                            }
                            if ui.button("Remove").clicked() {
                                remove = Some(k);
                                ui.close_menu();
                            }
                        });
                    }
                    if let Some(k) = remove {
                        pad.devices.remove(k);
                        self.rack.open = None;
                    }
                    ui.menu_button("+", |ui| {
                        for (folder, kinds) in DEVICE_CATEGORIES {
                            ui.menu_button(*folder, |ui| {
                                for kind in kinds.iter().copied().filter(|k| *k != DeviceKind::Rack) {
                                    if ui.button(kind.name()).clicked() {
                                        let id = self.project.new_id();
                                        pad.devices.push(Device::new(id, kind));
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                    });
                });
                if let Some((rid, ps, k)) = self.rack.open {
                    if rid == u64::MAX - track && ps == sel {
                        if let Some(d) = pad.devices.get_mut(k) {
                            let defs = d.kind.defs();
                            egui::Grid::new(("padfx", d.id)).spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                                for idx in 0..defs.len() {
                                    let r = widgets::param(ui, ("dev", d.id, idx), &mut d.params[idx], &defs[idx], 26.0);
                                    self.param_hint(&r, &defs[idx], d.kind.info());
                                    if idx % 7 == 6 {
                                        ui.end_row();
                                    }
                                }
                            });
                        }
                    }
                }
                let _ = Color32::WHITE;
            });
        });
    }
}
