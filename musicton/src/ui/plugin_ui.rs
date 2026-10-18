//! "External plugin" handling: instruments with their own editor (B-303, Nexa) show a
//! compact device in the chain with a wrench button that opens the editor in a separate window.

use super::theme::*;
use super::App;
use crate::audio::engine::Cmd;
use crate::model::*;
use crate::params::{self, b303, ParamDef};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

const SILVER_DARK: Color32 = Color32::from_rgb(0xa8, 0xaa, 0xad);
use std::collections::HashMap;

pub struct PluginState {
    pub open: HashMap<u64, bool>,
    pub hidden: bool,
    pub b303: HashMap<u64, b303_gui::EditorState>,
}

impl Default for PluginState {
    fn default() -> Self {
        Self { open: HashMap::new(), hidden: false, b303: HashMap::new() }
    }
}

impl App {
    fn b303_param(&self, track: u64, idx: usize) -> f32 {
        self.project.track(track).and_then(|t| t.instrument.params.get(idx).copied()).unwrap_or(0.0)
    }

    fn set_b303_param(&mut self, track: u64, idx: usize, v: f32) {
        if let Some(t) = self.project.track_mut(track) {
            if let Some(x) = t.instrument.params.get_mut(idx) {
                if *x != v {
                    *x = v;
                    self.changed = true;
                }
            }
        }
    }

    fn b303_pattern(&self, track: u64) -> Option<SeqPattern> {
        let t = self.project.track(track)?;
        let pat = (t.instrument.params[b303::PATTERN].round() as usize).min(7);
        t.instrument.bank.as_ref()?.patterns.get(pat).cloned()
    }

    fn set_b303_pattern(&mut self, track: u64, p: SeqPattern) {
        if let Some(t) = self.project.track_mut(track) {
            let pat = (t.instrument.params[b303::PATTERN].round() as usize).min(7);
            if let Some(bank) = t.instrument.bank.as_mut() {
                if let Some(slot) = bank.patterns.get_mut(pat) {
                    *slot = p;
                    self.changed = true;
                }
            }
        }
    }

    /// Writes the current internal pattern into the first empty clip slot of the track.
    fn b303_export(&mut self, track: u64) {
        let Some(pat) = self.b303_pattern(track) else { return };
        let steps: Vec<crate::audio::b303::Step> = pat
            .steps
            .iter()
            .take(pat.len as usize)
            .map(|s| crate::audio::b303::Step {
                pitch: s.pitch,
                accent: s.accent,
                slide: s.slide,
                time: match s.time {
                    0 => crate::audio::b303::StepTime::Note,
                    1 => crate::audio::b303::StepTime::Tie,
                    _ => crate::audio::b303::StepTime::Rest,
                },
            })
            .collect();
        let Some(t) = self.project.track(track) else { return };
        let Some(scene) = (self.sel_scene..t.clips.len()).chain(0..self.sel_scene).find(|&s| t.clips[s].is_none()) else {
            self.toast("No empty clip slot on this track");
            return;
        };
        let n = self.b303_param(track, b303::PATTERN).round() as usize + 1;
        let mut clip = Clip::new(&format!("303 Pattern {n}"), t.color, 1);
        clip.len = pat.len.max(1) as u32;
        clip.notes = crate::audio::b303::steps_to_notes(&steps);
        self.create_clip(track, scene, clip);
        self.toast(format!("Pattern {n} written to clip slot {}", scene + 1));
    }

    /// Reads the selected clip of the track into the current internal pattern.
    fn b303_import(&mut self, track: u64) {
        let Some(r) = self.sel_clip.filter(|c| c.track() == track) else {
            self.toast("Select a clip on this track first");
            return;
        };
        let Some(clip) = self.clip(r).cloned() else { return };
        let len = clip.len.clamp(1, 16);
        let steps = crate::audio::b303::notes_to_steps(&clip.notes, 16, 45);
        let pat = SeqPattern { steps: steps.iter().map(|s| SeqStep { pitch: s.pitch, accent: s.accent, slide: s.slide, time: s.time as u8 }).collect(), len: len as u8 };
        self.set_b303_pattern(track, pat);
        self.toast("Clip loaded into the internal pattern");
    }

    // -----------------------------------------------------------------------------------------
    // Device chain: compact plugin device

    pub(super) fn plugin_device(&mut self, ui: &mut egui::Ui, track: u64, inst: &mut Instrument) {
        let kind = inst.kind;
        let open = self.plugins.open.get(&track).copied().unwrap_or(false);
        let shown: Vec<usize> = match kind {
            InstrumentKind::B303 => vec![b303::CUTOFF, b303::RES, b303::ENV_MOD, b303::DECAY, b303::ACCENT, b303::VOLUME, b303::DRIVE, b303::RUN, b303::PATTERN],
            _ => {
                use params::nexa as nx;
                vec![nx::PRESET, nx::CUTOFF, nx::RES, nx::ATTACK, nx::RELEASE, nx::VOLUME, nx::ARP_ON, nx::GATE_ON, nx::REVERB]
            }
        };
        let defs = kind.defs();
        egui::Frame::default()
            .fill(PANEL_LIGHT)
            .stroke(Stroke::new(1.0_f32, if open { SELECT } else { LINE }))
            .corner_radius(3)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_min_height(200.0);
                    ui.set_width(250.0);
                    ui.horizontal(|ui| {
                        let r = super::widgets::toggle(ui, "🔧", open, BLUE, vec2(26.0, 18.0));
                        if r.clicked() {
                            self.plugins.open.insert(track, !open);
                            self.plugins.hidden = false;
                        }
                        self.hint(&r, "Plug-In Edit", "Opens the instrument's own editor in a separate window. Ctrl+Alt+P shows/hides all plug-in windows.");
                        let title = ui.add(egui::Label::new(egui::RichText::new(kind.name()).strong()).sense(Sense::click()));
                        if title.double_clicked() {
                            self.plugins.open.insert(track, true);
                        }
                        self.hint(&title, kind.name(), kind.info());
                    });
                    ui.add_space(4.0);
                    for idx in shown {
                        let def = &defs[idx];
                        ui.horizontal(|ui| {
                            ui.add_sized(vec2(78.0, 16.0), egui::Label::new(egui::RichText::new(def.name).size(11.0).color(TEXT_DIM)).truncate());
                            let r = param_slider(ui, &mut inst.params[idx], def, 150.0);
                            r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                            if r.hovered() || r.dragged() {
                                self.info = Some((def.name.to_string(), format!("{}\n\nDrag to change, double-click to reset. Open the editor with the wrench button for all controls.", def.format(inst.params[idx]))));
                            }
                        });
                    }
                    if kind == InstrumentKind::Nexa {
                        // Changing the preset here must also load its arp/gate/FX settings.
                        let idx = (inst.params[params::nexa::PRESET].round() as usize).min(crate::audio::nexa::PRESETS.len() - 1);
                        if self.nexa_presets.get(&track) != Some(&idx) {
                            for (k, v) in crate::audio::nexa::preset_defaults(idx) {
                                inst.params[k] = v;
                            }
                            self.nexa_presets.insert(track, idx);
                        }
                    }
                });
            });
    }

    // -----------------------------------------------------------------------------------------
    // Plug-in windows

    pub(super) fn plugin_windows(&mut self, ctx: &egui::Context) {
        if self.plugins.hidden {
            return;
        }
        let open: Vec<u64> = self.plugins.open.iter().filter(|(_, o)| **o).map(|(id, _)| *id).collect();
        for id in open {
            let Some(t) = self.project.track(id) else {
                self.plugins.open.remove(&id);
                continue;
            };
            let kind = t.instrument.kind;
            if !matches!(kind, InstrumentKind::B303 | InstrumentKind::Nexa) {
                self.plugins.open.remove(&id);
                continue;
            }
            let panel = self.plugins.b303.get(&id).map(|u| u.panel).unwrap_or(true);
            let size = match kind {
                InstrumentKind::B303 => [b303_gui::W * 0.92, (b303_gui::H + if panel { b303_gui::PANEL_H } else { 0.0 }) * 0.92],
                _ => [1040.0, 300.0],
            };
            let title = format!("{} — {}", kind.name(), t.name);
            let vid = egui::ViewportId::from_hash_of(("plugin", id));
            ctx.show_viewport_immediate(
                vid,
                egui::ViewportBuilder::default().with_title(title).with_inner_size(size).with_min_inner_size([400.0, 200.0]).with_app_id("musicton-plugin"),
                |ctx, _class| {
                    egui::CentralPanel::default().frame(egui::Frame::default().fill(if kind == InstrumentKind::B303 { SILVER_DARK } else { BG })).show(ctx, |ui| {
                        match kind {
                            InstrumentKind::B303 => self.b303_editor(ui, id),
                            _ => {
                                let Some(mut inst) = self.project.track(id).map(|t| t.instrument.clone()) else { return };
                                let before = inst.params.clone();
                                egui::ScrollArea::both().drag_to_scroll(false).show(ui, |ui| self.nexa_panel(ui, id, &mut inst));
                                if inst.params != before {
                                    if let Some(t) = self.project.track_mut(id) {
                                        t.instrument = inst;
                                    }
                                    self.changed = true;
                                }
                            }
                        }
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.plugins.open.insert(id, false);
                    }
                },
            );
        }
    }

    /// Draws a plug-in editor over the main window (used for remote screenshots).
    pub(super) fn plugin_overlay_ui(&mut self, ctx: &egui::Context) {
        let Some(id) = self.plugin_overlay else { return };
        let Some(kind) = self.project.track(id).map(|t| t.instrument.kind) else { return };
        egui::Area::new(egui::Id::new("plugin_overlay")).order(egui::Order::Foreground).fade_in(false).fixed_pos(pos2(0.0, 0.0)).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let panel = self.plugins.b303.get(&id).map(|u| u.panel).unwrap_or(true);
            let size = match kind {
                InstrumentKind::B303 => {
                    let h = b303_gui::H + if panel { b303_gui::PANEL_H } else { 0.0 };
                    let sc = (screen.width() / b303_gui::W).min(screen.height() / h);
                    vec2(b303_gui::W * sc, h * sc)
                }
                _ => vec2(1040.0, 300.0),
            };
            ui.set_min_size(size);
            ui.painter().rect_filled(Rect::from_min_size(pos2(0.0, 0.0), size), 0.0, SILVER_DARK);
            ui.allocate_ui(size, |ui| match kind {
                InstrumentKind::B303 => self.b303_editor(ui, id),
                _ => {
                    let Some(mut inst) = self.project.track(id).map(|t| t.instrument.clone()) else { return };
                    self.nexa_panel(ui, id, &mut inst);
                }
            });
        });
    }

    // -----------------------------------------------------------------------------------------
    // B-303 editor (shared crate) connected to Musicton

    fn b303_editor(&mut self, ui: &mut egui::Ui, track: u64) {
        let mut st = self.plugins.b303.remove(&track).unwrap_or_else(b303_gui::EditorState::new);
        b303_gui::editor(ui, &mut MusictonHost { app: self, track }, &mut st);
        self.plugins.b303.insert(track, st);
    }
}

/// Connects the shared B-303 editor to a Musicton track.
struct MusictonHost<'a> {
    app: &'a mut App,
    track: u64,
}

impl b303_gui::Host for MusictonHost<'_> {
    fn param(&self, idx: usize) -> f32 {
        self.app.b303_param(self.track, idx)
    }
    fn set_param(&mut self, idx: usize, v: f32) {
        self.app.set_b303_param(self.track, idx, v);
    }
    fn pattern(&self) -> SeqPattern {
        self.app.b303_pattern(self.track).unwrap_or_else(|| SeqBank::factory().patterns[0].clone())
    }
    fn set_pattern(&mut self, p: SeqPattern) {
        self.app.set_b303_pattern(self.track, p);
    }
    fn display(&self) -> [f32; 3] {
        let d = self.app.status.displays.iter().find(|(i, _)| *i == self.track).map(|(_, d)| d.as_slice()).unwrap_or(&[]);
        [d.first().copied().unwrap_or(0.0), d.get(1).copied().unwrap_or(0.0), d.get(2).copied().unwrap_or(0.0)]
    }
    fn level(&self) -> f32 {
        let m = self.app.meter(self.track);
        m[0].max(m[1])
    }
    fn note_on(&mut self, pitch: u8, vel: u8) {
        self.app.send(Cmd::NoteOn { track: self.track, pitch, vel });
        // Safety net in case the release gets lost.
        self.app.remote_note_offs.push((std::time::Instant::now() + std::time::Duration::from_secs(3), self.track, pitch));
    }
    fn note_off(&mut self, pitch: u8) {
        self.app.send(Cmd::NoteOff { track: self.track, pitch });
        let t = self.track;
        self.app.remote_note_offs.retain(|(_, tr, p)| !(*tr == t && *p == pitch));
    }
    fn audition(&mut self, pitch: u8, vel: u8) {
        self.app.send(Cmd::NoteOn { track: self.track, pitch, vel });
        self.app.remote_note_offs.push((std::time::Instant::now() + std::time::Duration::from_millis(180), self.track, pitch));
    }
    fn root(&self) -> i32 {
        36 + self.app.project.key as i32 % 12
    }
    fn scale(&self) -> &'static [u8] {
        SCALES[self.app.project.scale.min(SCALES.len() - 1)].1
    }
    fn seed(&self) -> u32 {
        (self.app.now() * 1000.0) as u32 ^ self.track as u32
    }
    fn hint(&mut self, title: &str, body: &str) {
        self.app.info = Some((title.to_string(), body.to_string()));
    }
    fn toggle_help(&mut self) {
        self.app.show_info = !self.app.show_info;
    }
    fn knob_menu(&mut self, ui: &mut egui::Ui, idx: usize) {
        self.app.map_menu(ui, MapTarget::Instrument(self.track, idx), Some(self.track));
    }
    fn option_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Write pattern to clip").clicked() {
            self.app.b303_export(self.track);
            ui.close_menu();
        }
        if ui.button("Load selected clip into pattern").clicked() {
            self.app.b303_import(self.track);
            ui.close_menu();
        }
    }
    fn write_pattern(&mut self) {
        self.app.b303_export(self.track);
    }
}

/// Horizontal parameter slider for configured plug-in parameters.
fn param_slider(ui: &mut egui::Ui, value: &mut f32, def: &ParamDef, w: f32) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, 16.0), Sense::click_and_drag());
    let mut norm = def.to_norm(*value);
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        norm = (norm + resp.drag_delta().x / if fine { w * 5.0 } else { w }).clamp(0.0, 1.0);
        let nv = def.from_norm(norm);
        if nv != *value {
            *value = nv;
            resp.mark_changed();
        }
    }
    if def.is_choice() && resp.clicked() {
        *value = ((value.round() as usize + 1) % def.options.len()) as f32;
        resp.mark_changed();
    }
    if resp.double_clicked() {
        *value = def.def;
        resp.mark_changed();
    }
    let p = ui.painter();
    p.rect_filled(rect, 2.0, KNOB_BG);
    let fill = Rect::from_min_size(rect.min, vec2(rect.width() * def.to_norm(*value), rect.height()));
    p.rect_filled(fill, 2.0, if resp.dragged() { ACCENT } else { Color32::from_rgb(0x8a, 0x6a, 0x2a) });
    p.text(rect.center(), Align2::CENTER_CENTER, def.format(*value), FontId::proportional(10.5), TEXT);
    resp
}
