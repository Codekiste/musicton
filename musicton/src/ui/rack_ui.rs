//! Rack device (parallel chains), sidechain source selection and track modulators.

use super::arrangement::auto_targets;
use super::theme::*;
use super::widgets;
use super::App;
use crate::model::*;
use crate::params::{self, ParamDef, Unit};
use egui::{vec2, Color32, RichText};

#[derive(Default)]
pub struct RackUiState {
    /// (rack id, chain index, device index) whose parameters are shown.
    pub open: Option<(u64, usize, usize)>,
}

impl App {
    /// Sidechain source picker for compressors, gates and the ducker. Returns true if changed.
    pub(super) fn sidechain_combo(&mut self, ui: &mut egui::Ui, d: &mut Device, own_track: Option<u64>) -> bool {
        let cur = d.sc_source.and_then(|id| self.project.track(id)).map(|t| t.name.clone()).unwrap_or_else(|| "Self".into());
        let mut changed = false;
        let r = egui::ComboBox::from_id_salt(("sc", d.id)).width(84.0).selected_text(RichText::new(format!("SC: {cur}")).size(10.0)).show_ui(ui, |ui| {
            if ui.selectable_label(d.sc_source.is_none(), "Self (no sidechain)").clicked() {
                d.sc_source = None;
                changed = true;
            }
            for t in self.project.tracks.iter().filter(|t| Some(t.id) != own_track) {
                if ui.selectable_label(d.sc_source == Some(t.id), &t.name).clicked() {
                    d.sc_source = Some(t.id);
                    changed = true;
                }
            }
        });
        self.hint(&r.response, "Sidechain", "Lets another track's signal drive this device's detector - e.g. duck a bass with the kick.");
        changed
    }

    /// Body of a Rack device: macro-style dry/wet and output plus the chain list.
    pub(super) fn rack_ui(&mut self, ui: &mut egui::Ui, d: &mut Device) -> bool {
        let mut dirty = false;
        let defs = d.kind.defs();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for idx in 0..defs.len() {
                    let r = widgets::param(ui, ("dev", d.id, idx), &mut d.params[idx], &defs[idx], 30.0);
                    dirty |= r.changed();
                    self.param_hint(&r, &defs[idx], d.kind.info());
                }
            });
            let vol_def = ParamDef::new("Vol", 0.0, 1.0, 0.85, Unit::Pct);
            let pan_def = ParamDef::new("Pan", -1.0, 1.0, 0.0, Unit::Pan);
            let mut remove_chain = None;
            for ci in 0..d.chains.len() {
                egui::Frame::default().fill(PANEL_DARK).corner_radius(2).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let c = &mut d.chains[ci];
                        let r = widgets::toggle(ui, "M", c.mute, Color32::from_rgb(0xd0, 0x60, 0x40), vec2(16.0, 16.0));
                        if r.clicked() {
                            c.mute = !c.mute;
                            dirty = true;
                        }
                        let r = ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(56.0).font(egui::TextStyle::Small));
                        dirty |= r.changed();
                        dirty |= widgets::knob(ui, &mut c.volume, &vol_def, 20.0, false).changed();
                        dirty |= widgets::knob(ui, &mut c.pan, &pan_def, 20.0, false).changed();
                        let mut remove_dev = None;
                        for di in 0..c.devices.len() {
                            let dev = &mut c.devices[di];
                            let open = self.rack.open == Some((d.id, ci, di));
                            let col = if open { SELECT } else if dev.on { PANEL_LIGHT } else { KNOB_BG };
                            let r = widgets::toggle(ui, dev.kind.name(), open, col, vec2(8.0 + dev.kind.name().len() as f32 * 6.0, 16.0));
                            if r.clicked() {
                                self.rack.open = if open { None } else { Some((d.id, ci, di)) };
                            }
                            r.context_menu(|ui| {
                                if ui.button(if dev.on { "Bypass" } else { "Activate" }).clicked() {
                                    dev.on = !dev.on;
                                    dirty = true;
                                    ui.close_menu();
                                }
                                if ui.button("Remove").clicked() {
                                    remove_dev = Some(di);
                                    ui.close_menu();
                                }
                            });
                        }
                        if let Some(di) = remove_dev {
                            c.devices.remove(di);
                            self.rack.open = None;
                            dirty = true;
                        }
                        ui.menu_button("+", |ui| {
                            for (folder, kinds) in DEVICE_CATEGORIES {
                                ui.menu_button(*folder, |ui| {
                                    for kind in kinds.iter().copied().filter(|k| *k != DeviceKind::Rack) {
                                        if ui.button(kind.name()).clicked() {
                                            let id = self.project.new_id();
                                            d.chains[ci].devices.push(Device::new(id, kind));
                                            dirty = true;
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        });
                        if ui.small_button("×").on_hover_text("Remove chain").clicked() {
                            remove_chain = Some(ci);
                        }
                    });
                });
            }
            if let Some(ci) = remove_chain {
                d.chains.remove(ci);
                self.rack.open = None;
                dirty = true;
            }
            let r = ui.small_button("+ Chain");
            if r.clicked() {
                let n = d.chains.len() + 1;
                d.chains.push(RackChain::new(&format!("Chain {n}")));
                dirty = true;
            }
            self.hint(&r, "Rack", "Each chain gets the rack's input and runs in parallel; the chains are mixed together. Click a device in a chain to edit its parameters.");
            // Parameters of the opened chain device.
            if let Some((rid, ci, di)) = self.rack.open {
                if rid == d.id {
                    if let Some(dev) = d.chains.get_mut(ci).and_then(|c| c.devices.get_mut(di)) {
                        let defs = dev.kind.defs();
                        ui.label(RichText::new(dev.kind.name()).size(10.0).color(TEXT_DIM));
                        egui::Grid::new(("rackdev", dev.id)).spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                            for idx in 0..defs.len() {
                                let r = widgets::param(ui, ("dev", dev.id, idx), &mut dev.params[idx], &defs[idx], 28.0);
                                dirty |= r.changed();
                                if idx % 6 == 5 {
                                    ui.end_row();
                                }
                            }
                        });
                    }
                }
            }
        });
        dirty
    }

    /// LFO / envelope modulators of a track.
    pub(super) fn modulator_panel(&mut self, ui: &mut egui::Ui, track: u64) {
        let Some(t) = self.project.track(track) else { return };
        let mut mods = t.modulators.clone();
        let targets = auto_targets(&self.project, t);
        let mut dirty = false;
        let mut remove = None;
        let depth_def = ParamDef::new("Depth", -1.0, 1.0, 0.3, Unit::Pct);
        let hz_def = ParamDef::new("Rate", 0.01, 40.0, 1.0, Unit::Hz).log();
        let att_def = ParamDef::new("Attack", 0.001, 2.0, 0.005, Unit::Sec).log();
        let dec_def = ParamDef::new("Decay", 0.01, 8.0, 0.3, Unit::Sec).log();
        egui::Frame::default()
            .fill(Color32::from_rgb(0x2c, 0x33, 0x38))
            .stroke(egui::Stroke::new(1.0_f32, LINE))
            .corner_radius(3)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_min_height(200.0);
                    ui.label(RichText::new("Modulators").strong().color(TEXT_DIM));
                    for (i, m) in mods.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let r = widgets::toggle(ui, "●", m.on, ACCENT, vec2(16.0, 16.0));
                            if r.clicked() {
                                m.on = !m.on;
                                dirty = true;
                            }
                            egui::ComboBox::from_id_salt(("modshape", track, m.id)).width(64.0).selected_text(m.shape.name()).show_ui(ui, |ui| {
                                for s in ModShape::ALL {
                                    if ui.selectable_label(m.shape == s, s.name()).clicked() {
                                        m.shape = s;
                                        dirty = true;
                                    }
                                }
                            });
                            let tname = m.target.and_then(|tg| targets.iter().find(|(x, _)| *x == tg)).map(|(_, n)| n.clone()).unwrap_or_else(|| "— target —".into());
                            egui::ComboBox::from_id_salt(("modtgt", track, m.id)).width(120.0).selected_text(RichText::new(tname).size(10.0)).show_ui(ui, |ui| {
                                if ui.selectable_label(m.target.is_none(), "None").clicked() {
                                    m.target = None;
                                    dirty = true;
                                }
                                for (tg, n) in &targets {
                                    if ui.selectable_label(m.target == Some(*tg), n).clicked() {
                                        m.target = Some(*tg);
                                        dirty = true;
                                    }
                                }
                            });
                            if ui.small_button("×").clicked() {
                                remove = Some(i);
                            }
                        });
                        ui.horizontal(|ui| {
                            if m.shape == ModShape::Envelope {
                                dirty |= widgets::knob(ui, &mut m.attack, &att_def, 24.0, true).changed();
                                dirty |= widgets::knob(ui, &mut m.decay, &dec_def, 24.0, true).changed();
                            } else {
                                let r = widgets::toggle(ui, "Sync", m.synced, ACCENT, vec2(34.0, 16.0));
                                if r.clicked() {
                                    m.synced = !m.synced;
                                    m.rate = if m.synced { 7.0 } else { 1.0 };
                                    dirty = true;
                                }
                                if m.synced {
                                    let cur = params::DIVISIONS[(m.rate.round() as usize).min(params::DIVISIONS.len() - 1)];
                                    egui::ComboBox::from_id_salt(("modrate", track, m.id)).width(54.0).selected_text(cur).show_ui(ui, |ui| {
                                        for (k, name) in params::DIVISIONS.iter().enumerate() {
                                            if ui.selectable_label(m.rate.round() as usize == k, *name).clicked() {
                                                m.rate = k as f32;
                                                dirty = true;
                                            }
                                        }
                                    });
                                } else {
                                    dirty |= widgets::knob(ui, &mut m.rate, &hz_def, 24.0, true).changed();
                                }
                            }
                            dirty |= widgets::knob(ui, &mut m.depth, &depth_def, 24.0, true).changed();
                        });
                        ui.separator();
                    }
                    let r = ui.button("+ Modulator");
                    if r.clicked() {
                        let id = self.project.new_id();
                        let mut m = Modulator::new(id);
                        m.target = targets.iter().find(|(tg, _)| matches!(tg, AutoTarget::Device { .. } | AutoTarget::Instrument(_))).map(|(tg, _)| *tg);
                        mods.push(m);
                        dirty = true;
                    }
                    self.hint(&r, "Modulators", "LFOs and envelopes that move any parameter of this track continuously. Envelope modulators restart on every note.");
                });
            });
        if let Some(i) = remove {
            mods.remove(i);
            dirty = true;
        }
        if dirty {
            if let Some(t) = self.project.track_mut(track) {
                t.modulators = mods;
            }
            self.touch();
        }
    }
}
