//! MIDI/key mapping, MIDI learn, macros and arrangement recording.

use super::theme::*;
use super::{App, ClipRef};
use crate::audio::engine::Cmd;
use crate::model::*;
use egui::RichText;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LearnKind {
    Midi,
    Key,
}

impl App {
    pub(super) fn target_name(&self, t: MapTarget) -> String {
        let track_name = |id: u64| self.project.track(id).map(|t| t.name.clone()).unwrap_or_else(|| "?".into());
        match t {
            MapTarget::Volume(id) => format!("{} Volume", track_name(id)),
            MapTarget::Pan(id) => format!("{} Pan", track_name(id)),
            MapTarget::Send(id, k) => format!("{} Send {}", track_name(id), ["A", "B"][(k as usize).min(1)]),
            MapTarget::Mute(id) => format!("{} Activator", track_name(id)),
            MapTarget::Solo(id) => format!("{} Solo", track_name(id)),
            MapTarget::Instrument(id, i) => {
                let t = self.project.track(id);
                let p = t.and_then(|t| t.instrument.kind.defs().get(i)).map(|d| d.name).unwrap_or("?");
                format!("{} {}", track_name(id), p)
            }
            MapTarget::Device(dev, i) => {
                let d = self.find_device(dev);
                d.map(|d| format!("{} {}", d.kind.name(), d.kind.defs().get(i).map(|x| x.name).unwrap_or("?"))).unwrap_or_else(|| "Device".into())
            }
            MapTarget::MidiFx(id, i) => {
                let m = self.project.tracks.iter().flat_map(|t| t.midi_fx.iter()).find(|m| m.id == id);
                m.map(|m| format!("{} {}", m.kind.name(), m.kind.defs().get(i).map(|x| x.name).unwrap_or("?"))).unwrap_or_else(|| "MIDI Effect".into())
            }
            MapTarget::Macro(id, i) => {
                let n = self.project.track(id).and_then(|t| t.macros.get(i as usize)).map(|m| m.name.clone()).unwrap_or_default();
                format!("{} {}", track_name(id), n)
            }
            MapTarget::ReturnVolume(k) => format!("{} Volume", self.project.returns.get(k as usize).map(|r| r.name.as_str()).unwrap_or("Return")),
            MapTarget::MasterVolume => "Master Volume".into(),
            MapTarget::Crossfader => "Crossfader".into(),
            MapTarget::Tempo => "Tempo".into(),
            MapTarget::LaunchScene(s) => format!("Launch Scene {}", self.project.scenes.get(s).map(|x| x.name.as_str()).unwrap_or("?")),
            MapTarget::LaunchClip(id, s) => format!("Launch {} slot {}", track_name(id), s + 1),
            MapTarget::StopTrack(id) => format!("Stop {}", track_name(id)),
            MapTarget::StopAll => "Stop All Clips".into(),
            MapTarget::Play => "Play".into(),
            MapTarget::Stop => "Stop".into(),
        }
    }

    fn find_device(&self, id: u64) -> Option<&Device> {
        self.project
            .tracks
            .iter()
            .map(|t| &t.channel)
            .chain(self.project.returns.iter().map(|r| &r.channel))
            .chain(std::iter::once(&self.project.master))
            .flat_map(|c| c.devices.iter())
            .find(|d| d.id == id)
    }

    /// Current value of a continuous target, normalized to 0..1.
    pub(super) fn target_get(&self, t: MapTarget) -> Option<f32> {
        let p = &self.project;
        Some(match t {
            MapTarget::Volume(id) => p.track(id)?.channel.volume,
            MapTarget::Pan(id) => (p.track(id)?.channel.pan + 1.0) / 2.0,
            MapTarget::Send(id, k) => p.track(id)?.channel.sends[(k as usize).min(1)],
            MapTarget::Mute(id) => if p.track(id)?.channel.mute { 0.0 } else { 1.0 },
            MapTarget::Solo(id) => if p.track(id)?.channel.solo { 1.0 } else { 0.0 },
            MapTarget::Instrument(id, i) => {
                let t = p.track(id)?;
                t.instrument.kind.defs().get(i)?.to_norm(*t.instrument.params.get(i)?)
            }
            MapTarget::Device(id, i) => {
                let d = self.find_device(id)?;
                d.kind.defs().get(i)?.to_norm(*d.params.get(i)?)
            }
            MapTarget::MidiFx(id, i) => {
                let m = p.tracks.iter().flat_map(|t| t.midi_fx.iter()).find(|m| m.id == id)?;
                m.kind.defs().get(i)?.to_norm(*m.params.get(i)?)
            }
            MapTarget::Macro(id, i) => p.track(id)?.macros.get(i as usize)?.value,
            MapTarget::ReturnVolume(k) => p.returns.get(k as usize)?.channel.volume,
            MapTarget::MasterVolume => p.master.volume,
            MapTarget::Crossfader => (p.crossfader + 1.0) / 2.0,
            MapTarget::Tempo => (p.bpm - 60.0) / 140.0,
            _ => return None,
        })
    }

    pub(super) fn target_set(&mut self, t: MapTarget, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match t {
            MapTarget::Volume(id) => {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.channel.volume = v;
                }
            }
            MapTarget::Pan(id) => {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.channel.pan = v * 2.0 - 1.0;
                }
            }
            MapTarget::Send(id, k) => {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.channel.sends[(k as usize).min(1)] = v;
                }
            }
            MapTarget::Mute(id) => {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.channel.mute = v < 0.5;
                }
            }
            MapTarget::Solo(id) => {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.channel.solo = v >= 0.5;
                }
            }
            MapTarget::Instrument(id, i) => {
                if let Some(tr) = self.project.track_mut(id) {
                    if let (Some(d), Some(x)) = (tr.instrument.kind.defs().get(i), tr.instrument.params.get_mut(i)) {
                        *x = d.from_norm(v);
                    }
                }
            }
            MapTarget::Device(id, i) => {
                if let Some(d) = self.project.device_mut(id) {
                    if let Some(def) = d.kind.defs().get(i) {
                        d.params[i] = def.from_norm(v);
                    }
                }
            }
            MapTarget::MidiFx(id, i) => {
                if let Some(m) = self.project.tracks.iter_mut().flat_map(|t| t.midi_fx.iter_mut()).find(|m| m.id == id) {
                    if let Some(def) = m.kind.defs().get(i) {
                        m.params[i] = def.from_norm(v);
                    }
                }
            }
            MapTarget::Macro(id, i) => {
                self.set_macro(id, i as usize, v);
                return;
            }
            MapTarget::ReturnVolume(k) => {
                if let Some(r) = self.project.returns.get_mut(k as usize) {
                    r.channel.volume = v;
                }
            }
            MapTarget::MasterVolume => self.project.master.volume = v,
            MapTarget::Crossfader => self.project.crossfader = v * 2.0 - 1.0,
            MapTarget::Tempo => self.project.bpm = (60.0 + v * 140.0).round(),
            _ => {
                if v > 0.5 {
                    self.target_trigger(t);
                }
                return;
            }
        }
        self.touch();
    }

    pub(super) fn target_trigger(&mut self, t: MapTarget) {
        match t {
            MapTarget::LaunchScene(s) => self.send(Cmd::LaunchScene(s)),
            MapTarget::LaunchClip(id, s) => self.send(Cmd::Launch { track: id, scene: s }),
            MapTarget::StopTrack(id) => self.send(Cmd::StopTrack(id)),
            MapTarget::StopAll => self.send(Cmd::StopAll),
            MapTarget::Play => self.send(Cmd::Play),
            MapTarget::Stop => self.send(Cmd::Stop),
            MapTarget::Mute(id) => {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.channel.mute = !tr.channel.mute;
                }
                self.touch();
            }
            MapTarget::Solo(id) => {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.channel.solo = !tr.channel.solo;
                }
                self.touch();
            }
            other => {
                // Continuous targets on a button: toggle between min and max.
                let cur = self.target_get(other).unwrap_or(0.0);
                self.target_set(other, if cur > 0.5 { 0.0 } else { 1.0 });
            }
        }
    }

    pub(super) fn set_macro(&mut self, track: u64, i: usize, v: f32) {
        let Some(m) = self.project.track_mut(track).and_then(|t| t.macros.get_mut(i)) else { return };
        m.value = v;
        let targets = m.targets.clone();
        for (t, lo, hi) in targets {
            if !matches!(t, MapTarget::Macro(..)) {
                self.target_set(t, lo + (hi - lo) * v);
            }
        }
        self.touch();
    }

    /// Context menu entries for any mappable control.
    pub(super) fn map_menu(&mut self, ui: &mut egui::Ui, target: MapTarget, track: Option<u64>) {
        ui.label(RichText::new(self.target_name(target)).color(TEXT_DIM).size(11.0));
        if ui.button("MIDI Learn").on_hover_text("Then move a knob/fader or press a pad on your MIDI controller").clicked() {
            self.learning = Some((target, LearnKind::Midi));
            self.midi.learning.store(true, std::sync::atomic::Ordering::Relaxed);
            ui.close_menu();
        }
        if ui.button("Map to Computer Key").on_hover_text("Then press a key on your computer keyboard").clicked() {
            self.learning = Some((target, LearnKind::Key));
            ui.close_menu();
        }
        let maps: Vec<String> = self.project.mappings.iter().filter(|m| m.target == target).map(|m| m.source.label()).collect();
        if !maps.is_empty() {
            if ui.button(format!("Remove Mapping ({})", maps.join(", "))).clicked() {
                self.project.mappings.retain(|m| m.target != target);
                self.update_blocked_notes();
                self.touch();
                ui.close_menu();
            }
        }
        if let (Some(tid), false) = (track, target.is_trigger() || matches!(target, MapTarget::Macro(..))) {
            ui.menu_button("Map to Macro", |ui| {
                let names: Vec<String> = self.project.track(tid).map(|t| t.macros.iter().map(|m| m.name.clone()).collect()).unwrap_or_default();
                for (i, name) in names.iter().enumerate() {
                    if ui.button(name).clicked() {
                        let cur = self.target_get(target).unwrap_or(0.0);
                        if let Some(m) = self.project.track_mut(tid).and_then(|t| t.macros.get_mut(i)) {
                            m.targets.retain(|(t, _, _)| *t != target);
                            if m.targets.is_empty() {
                                m.value = 0.0;
                            }
                            m.targets.push((target, cur, 1.0));
                        }
                        self.touch();
                        self.toast(format!("Mapped to {name}: turning it moves the parameter from its current value to the maximum"));
                        ui.close_menu();
                    }
                }
            });
        }
    }

    /// Applies an incoming controller value (CC) to its mappings or completes MIDI learn.
    pub(super) fn handle_cc(&mut self, ch: u8, cc: u8, val: u8) {
        if let Some((target, LearnKind::Midi)) = self.learning {
            self.add_mapping(MapSource::Cc { ch, cc }, target);
            return;
        }
        let maps: Vec<Mapping> = self.project.mappings.iter().filter(|m| m.source == MapSource::Cc { ch, cc }).cloned().collect();
        for m in maps {
            let v = val as f32 / 127.0;
            if m.target.is_trigger() {
                if val >= 64 {
                    self.target_trigger(m.target);
                }
            } else {
                self.target_set(m.target, m.min + (m.max - m.min) * v);
            }
        }
    }

    pub(super) fn handle_mapped_note(&mut self, ch: u8, note: u8, vel: u8) {
        if let Some((target, LearnKind::Midi)) = self.learning {
            if vel > 0 {
                self.add_mapping(MapSource::Note { ch, note }, target);
            }
            return;
        }
        if vel == 0 {
            return;
        }
        let maps: Vec<Mapping> = self.project.mappings.iter().filter(|m| m.source == MapSource::Note { ch, note }).cloned().collect();
        for m in maps {
            self.target_trigger(m.target);
        }
    }

    /// Returns true when the key was consumed by learning or a mapping.
    pub(super) fn handle_mapped_key(&mut self, key: egui::Key) -> bool {
        let name = format!("{key:?}");
        if let Some((target, LearnKind::Key)) = self.learning {
            if key == egui::Key::Escape {
                self.learning = None;
            } else {
                self.add_mapping(MapSource::Key(name), target);
            }
            return true;
        }
        let maps: Vec<Mapping> = self.project.mappings.iter().filter(|m| m.source == MapSource::Key(name.clone())).cloned().collect();
        if maps.is_empty() {
            return false;
        }
        for m in maps {
            self.target_trigger(m.target);
        }
        true
    }

    fn add_mapping(&mut self, source: MapSource, target: MapTarget) {
        self.project.mappings.retain(|m| m.source != source);
        let label = source.label();
        self.project.mappings.push(Mapping { source, target, min: 0.0, max: 1.0 });
        self.learning = None;
        self.midi.learning.store(false, std::sync::atomic::Ordering::Relaxed);
        self.update_blocked_notes();
        self.touch();
        self.toast(format!("Mapped {label} → {}", self.target_name(target)));
    }

    /// Tells the MIDI thread which notes are mapped (they must not play the instrument).
    pub(super) fn update_blocked_notes(&mut self) {
        let notes: std::collections::HashSet<(u8, u8)> = self
            .project
            .mappings
            .iter()
            .filter_map(|m| if let MapSource::Note { ch, note } = m.source { Some((ch, note)) } else { None })
            .collect();
        *self.midi.blocked.write() = notes;
    }

    pub(super) fn mappings_window(&mut self, ctx: &egui::Context) {
        if !self.show_mappings {
            return;
        }
        let mut open = true;
        let mut remove = None;
        egui::Window::new("MIDI & Key Mappings").open(&mut open).default_width(420.0).show(ctx, |ui| {
            if self.project.mappings.is_empty() {
                ui.label(RichText::new("No mappings yet.\n\nRight-click any knob, fader, clip slot or scene and choose \"MIDI Learn\" or \"Map to Computer Key\".").color(TEXT_DIM));
            }
            egui::Grid::new("maps").striped(true).num_columns(4).show(ui, |ui| {
                for (i, m) in self.project.mappings.clone().iter().enumerate() {
                    ui.label(m.source.label());
                    ui.label("→");
                    ui.label(self.target_name(m.target));
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(i) = remove {
            self.project.mappings.remove(i);
            self.update_blocked_notes();
            self.touch();
        }
        self.show_mappings = open;
    }

    /// Arrangement record: writes the Session clips that play into the timeline.
    pub(super) fn update_arr_record(&mut self) {
        if !self.arr_rec || !self.status.playing {
            if !self.rec_segments.is_empty() {
                self.rec_segments.clear();
                self.touch();
            }
            return;
        }
        let pos = self.status.arr_pos.max(0.0) as u32;
        let statuses = self.status.tracks.clone();
        let mut changed = false;
        for ts in statuses {
            match ts.playing {
                Some(scene) => {
                    let current = self.rec_segments.get(&ts.id).copied();
                    match current {
                        Some((arr_id, sc, start)) if sc == scene => {
                            if let Some(c) = self.project.track_mut(ts.id).and_then(|t| t.arrangement.iter_mut().find(|c| c.id == arr_id)) {
                                let len = pos.saturating_sub(start) + 1;
                                if len > c.len {
                                    c.len = len;
                                    changed = true;
                                }
                            }
                        }
                        _ => {
                            let Some(mut clip) = self.clip(ClipRef::Session { track: ts.id, scene }).cloned() else { continue };
                            clip.follow = None;
                            let id = self.project.new_id();
                            if let Some(t) = self.project.track_mut(ts.id) {
                                t.arrangement.retain(|c| c.start + c.len <= pos || c.start > pos + 4096);
                                t.arrangement.push(ArrClip { id, start: pos, len: 1, clip });
                            }
                            self.rec_segments.insert(ts.id, (id, scene, pos));
                            changed = true;
                        }
                    }
                }
                None => {
                    self.rec_segments.remove(&ts.id);
                }
            }
        }
        if changed {
            // Publish to the engine without creating an undo step every frame.
            self.publish = true;
        }
    }
}
