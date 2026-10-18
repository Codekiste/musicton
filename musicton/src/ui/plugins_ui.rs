//! External VST3/CLAP plug-ins: browser list and scanning, adding them to tracks, keeping the
//! sandboxed host processes in sync with the project, and the generic parameter panel.

use super::theme::*;
use super::widgets;
use super::{App, Payload, Sel};
use crate::audio::plugbridge::{self, Bridge};
use crate::model::*;
use base64::Engine as _;
use egui::{vec2, Color32, RichText};
use musicton_plugproto::PluginDesc;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

type ScanSlot = Arc<parking_lot::Mutex<Option<Vec<PluginDesc>>>>;

#[derive(Default)]
pub struct PluginsState {
    pub list: Vec<PluginDesc>,
    pub scan: Option<ScanSlot>,
    filter: HashMap<u64, String>,
    last_sync: u32,
}

/// Every plug-in in the project: bridge key (device or track id) and its reference.
fn project_plugins(p: &Project) -> Vec<(u64, PluginRef)> {
    let mut out = vec![];
    let chans = p.tracks.iter().map(|t| &t.channel).chain(p.returns.iter().map(|r| &r.channel)).chain(std::iter::once(&p.master));
    for ch in chans {
        for d in &ch.devices {
            if let (DeviceKind::Plugin, Some(pr)) = (d.kind, &d.plugin) {
                out.push((d.id, pr.clone()));
            }
        }
    }
    for t in &p.tracks {
        if let (InstrumentKind::Plugin, Some(pr)) = (t.instrument.kind, &t.instrument.plugin) {
            out.push((t.id, pr.clone()));
        }
    }
    out
}

fn plugin_ref_mut(p: &mut Project, key: u64) -> Option<&mut PluginRef> {
    if let Some(i) = p.tracks.iter().position(|t| t.id == key && t.instrument.kind == InstrumentKind::Plugin) {
        return p.tracks[i].instrument.plugin.as_mut();
    }
    p.device_mut(key).and_then(|d| d.plugin.as_mut())
}

impl App {
    pub(super) fn load_plugin_list(&mut self) {
        self.plugins_state.list = plugbridge::cached_plugins();
        if self.plugins_state.list.is_empty() {
            self.start_plugin_scan();
        }
    }

    pub(super) fn start_plugin_scan(&mut self) {
        let slot: ScanSlot = Arc::new(parking_lot::Mutex::new(None));
        let s2 = slot.clone();
        std::thread::spawn(move || {
            *s2.lock() = Some(plugbridge::scan_all(&[]));
        });
        self.plugins_state.scan = Some(slot);
        self.toast("Scanning plug-ins (each one in a sandbox)...");
    }

    pub(super) fn plugin_browser(&mut self, ui: &mut egui::Ui, matches: &dyn Fn(&str) -> bool) {
        ui.horizontal(|ui| {
            let scanning = self.plugins_state.scan.is_some();
            if ui.add_enabled(!scanning, egui::Button::new(if scanning { "Scanning..." } else { "Rescan" })).clicked() {
                self.start_plugin_scan();
            }
            ui.label(RichText::new(format!("{} found", self.plugins_state.list.len())).size(10.0).color(TEXT_DIM));
        });
        for (title, inst) in [("Instruments", true), ("Effects", false)] {
            let items: Vec<(usize, PluginDesc)> = self.plugins_state.list.iter().cloned().enumerate().filter(|(_, d)| d.instrument == inst && (matches(&d.name) || matches(&d.vendor))).collect();
            if items.is_empty() {
                continue;
            }
            ui.label(RichText::new(title).size(10.5).color(TEXT_DIM));
            for (i, d) in items {
                let info = format!("{} · {} · {}\n{}\nDouble-click or drag onto a track{}.", d.vendor, d.format.to_uppercase(), if d.instrument { "instrument" } else { "effect" }, d.path, if d.instrument { " (or the empty area for a new track)" } else { " or into the Device Chain" });
                self.browser_item(ui, &format!("{} ({})", d.name, d.format.to_uppercase()), Payload::Plugin(i), &info);
            }
        }
        if self.plugins_state.list.is_empty() && self.plugins_state.scan.is_none() {
            ui.label(RichText::new("No plug-ins found in ~/.clap, ~/.vst3, /usr/lib/clap, /usr/lib/vst3.").size(10.5).color(TEXT_DIM));
        }
    }

    /// Adds a scanned plug-in: effects go into `target`'s chain, instruments get a new track
    /// (or replace the instrument of a plug-in track they are dropped on).
    pub(super) fn add_plugin(&mut self, index: usize, target: Option<Sel>) {
        let Some(d) = self.plugins_state.list.get(index).cloned() else { return };
        self.add_plugin_desc(&d, target);
    }

    pub(super) fn add_plugin_desc(&mut self, d: &PluginDesc, target: Option<Sel>) -> u64 {
        let pr = plugbridge::desc_to_ref(d);
        if d.instrument {
            let color = CLIP_COLORS[(self.project.tracks.len() * 5 + 7) % CLIP_COLORS.len()];
            let id = self.project.add_track(&d.name, InstrumentKind::Plugin, color);
            if let Some(t) = self.project.track_mut(id) {
                t.instrument.plugin = Some(pr);
            }
            self.select_track(id);
            self.detail = super::Detail::Devices;
            self.touch();
            id
        } else {
            let sel = target.unwrap_or(self.sel);
            let id = self.project.new_id();
            if let Some(ch) = self.channel_mut(sel) {
                let mut dev = Device::new(id, DeviceKind::Plugin);
                dev.plugin = Some(pr);
                ch.devices.push(dev);
                self.sel = sel;
                self.sel_device = Some(id);
                self.detail = super::Detail::Devices;
                self.touch();
            }
            id
        }
    }

    /// Starts/stops host processes to match the project and pulls in parameter info and edits.
    pub(super) fn sync_plugins(&mut self) {
        let done = self.plugins_state.scan.as_ref().and_then(|slot| slot.lock().take());
        {
            if let Some(list) = done {
                self.plugins_state.list = list;
                self.plugins_state.scan = None;
                let n = self.plugins_state.list.len();
                self.toast(format!("{n} plug-ins found"));
            }
        }
        self.plugins_state.last_sync = self.plugins_state.last_sync.wrapping_add(1);
        if self.plugins_state.last_sync % 10 != 0 {
            return;
        }
        let wanted = project_plugins(&self.project);
        let sr = self.audio.sample_rate as f32;
        for (key, pr) in &wanted {
            let restart = match plugbridge::get_blocking(*key) {
                Some(b) => b.plugin.path != pr.path || b.plugin.id != pr.id,
                None => true,
            };
            if restart {
                Bridge::start(*key, pr.clone(), sr);
            }
        }
        for key in plugbridge::keys() {
            if !wanted.iter().any(|(k, _)| *k == key) {
                plugbridge::remove(key);
            }
        }
        // Parameter info after loading, and edits made in the plug-ins' own editors.
        let mut changed = false;
        for (key, _) in &wanted {
            let Some(b) = plugbridge::get_blocking(*key) else { continue };
            let loaded = b.loaded.lock().clone();
            let updates = std::mem::take(&mut *b.param_updates.lock());
            if let Some(pr) = plugin_ref_mut(&mut self.project, *key) {
                if let Some(l) = loaded {
                    if pr.infos.is_empty() && !l.params.is_empty() {
                        pr.infos = l.params.clone();
                        pr.has_gui = l.gui;
                        if pr.values.is_empty() {
                            pr.values = l.values.clone();
                        }
                        changed = true;
                    }
                }
                for (id, v) in updates {
                    match pr.values.iter_mut().find(|x| x.0 == id) {
                        Some(x) => x.1 = v,
                        None => pr.values.push((id, v)),
                    }
                    changed = true;
                }
            }
        }
        if changed {
            // Plug-in bookkeeping, not an undo step.
            self.changed = true;
        }
    }

    pub(super) fn plugins_state_poke(&mut self) {
        self.plugins_state.last_sync = 9;
    }

    /// Runs the sync immediately (remote control waits for plug-ins to load).
    pub(super) fn sync_plugins_now(&mut self) {
        self.plugins_state.last_sync = 9;
        self.sync_plugins();
    }

    pub(super) fn plugin_infos(&mut self, key: u64) -> Option<Vec<PlugParam>> {
        plugin_ref_mut(&mut self.project, key).map(|p| p.infos.clone())
    }

    /// Stores every running plug-in's state in the project (before saving).
    pub(super) fn capture_plugin_states(&mut self) {
        for (key, _) in project_plugins(&self.project) {
            let Some(b) = plugbridge::get_blocking(key) else { continue };
            if let Some(state) = b.get_state(Duration::from_millis(800)) {
                if let Some(pr) = plugin_ref_mut(&mut self.project, key) {
                    pr.state = Some(base64::engine::general_purpose::STANDARD.encode(state));
                }
            }
        }
    }

    pub(super) fn plugin_set_param(&mut self, key: u64, id: u32, value: f64) {
        if let Some(b) = plugbridge::get_blocking(key) {
            b.set_param(id, value);
        }
        if let Some(pr) = plugin_ref_mut(&mut self.project, key) {
            match pr.values.iter_mut().find(|x| x.0 == id) {
                Some(x) => x.1 = value,
                None => pr.values.push((id, value)),
            }
            // A saved state would override the new value on reload.
            pr.state = None;
        }
        self.touch();
    }

    pub(super) fn plugin_show_editor(&mut self, key: u64) {
        let Some(b) = plugbridge::get_blocking(key) else {
            self.toast("The plug-in is still starting");
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = b.show_gui(true) {
                eprintln!("plug-in editor: {e}");
            }
        });
    }

    /// Generic panel for a plug-in (device or instrument). Returns nothing; edits go straight to
    /// the plug-in and the project.
    pub(super) fn plugin_panel(&mut self, ui: &mut egui::Ui, key: u64, pr: &PluginRef) {
        let bridge = plugbridge::get_blocking(key);
        let status = match &bridge {
            None => ("starting...".to_string(), TEXT_DIM),
            Some(b) if !b.alive.load(std::sync::atomic::Ordering::Relaxed) => {
                (format!("stopped: {}", b.error.lock().clone().unwrap_or_else(|| "process ended".into())), Color32::from_rgb(0xe0, 0x60, 0x50))
            }
            Some(b) if b.loaded.lock().is_none() => ("loading...".to_string(), TEXT_DIM),
            Some(_) => (format!("{} · {}", pr.vendor, pr.format.to_uppercase()), TEXT_DIM),
        };
        ui.vertical(|ui| {
            ui.set_width(300.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&status.0).size(10.0).color(status.1));
                if pr.has_gui {
                    let r = widgets::button(ui, "Editor", false);
                    if r.clicked() {
                        self.plugin_show_editor(key);
                    }
                    self.hint(&r, "Plug-in Editor", "Opens the plug-in's own window.");
                }
                if bridge.as_ref().is_some_and(|b| !b.alive.load(std::sync::atomic::Ordering::Relaxed)) && widgets::button(ui, "Restart", false).clicked() {
                    plugbridge::remove(key);
                    self.plugins_state.last_sync = 9;
                }
            });
            let filter = self.plugins_state.filter.entry(key).or_default();
            ui.add(egui::TextEdit::singleline(filter).hint_text("Filter parameters").desired_width(290.0));
            let f = filter.to_lowercase();
            let mut edits = vec![];
            egui::ScrollArea::vertical().id_salt(("plugparams", key)).max_height(150.0).show(ui, |ui| {
                for p in pr.infos.iter().filter(|p| f.is_empty() || p.name.to_lowercase().contains(&f)).take(200) {
                    let mut v = pr.values.iter().find(|x| x.0 == p.id).map(|x| x.1).unwrap_or(p.default);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(p.name.chars().take(18).collect::<String>()).size(10.0));
                        let mut slider = egui::Slider::new(&mut v, p.min..=p.max.max(p.min + 1e-9)).show_value(true);
                        if p.steps > 0 {
                            slider = slider.step_by((p.max - p.min) / p.steps as f64);
                        }
                        if ui.add_sized(vec2(180.0, 16.0), slider).changed() {
                            edits.push((p.id, v));
                        }
                    });
                }
            });
            if pr.infos.is_empty() {
                ui.label(RichText::new("Parameters appear once the plug-in has loaded.").size(10.0).color(TEXT_DIM));
            }
            for (id, v) in edits {
                self.plugin_set_param(key, id, v);
            }
        });
    }
}
