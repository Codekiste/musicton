//! Right-click menus for track headers, return tracks, clips and empty timeline space,
//! plus the small windows they open (track info text, track header buttons).

use super::theme::*;
use super::track_actions::TRACK_ICONS;
use super::widgets;
use super::{App, ClipRef, RenameTarget};
use crate::audio::analysis::MidiMode;
use crate::model::*;
use serde::{Deserialize, Serialize};

/// Which buttons the timeline track headers show.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeaderCfg {
    pub freeze: bool,
    pub protect: bool,
    pub hide: bool,
    pub arm: bool,
    pub takes: bool,
    #[serde(skip)]
    pub open: bool,
}

impl Default for HeaderCfg {
    fn default() -> Self {
        Self { freeze: true, protect: false, hide: false, arm: true, takes: true, open: false }
    }
}

impl HeaderCfg {
    fn path() -> std::path::PathBuf {
        dirs::data_dir().unwrap_or_else(|| ".".into()).join("musicton").join("track_header.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    fn save(&self) {
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::create_dir_all(Self::path().parent().unwrap());
            let _ = std::fs::write(Self::path(), s);
        }
    }
}

fn item(ui: &mut egui::Ui, label: &str) -> bool {
    let clicked = ui.button(label).clicked();
    if clicked {
        ui.close_menu();
    }
    clicked
}

fn item_on(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let clicked = ui.add_enabled(enabled, egui::Button::new(label)).clicked();
    if clicked {
        ui.close_menu();
    }
    clicked
}

fn check(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let clicked = ui.button(format!("{} {label}", if on { "✔" } else { "   " })).clicked();
    if clicked {
        ui.close_menu();
    }
    clicked
}

fn save_dialog(name: &str, ext: &str, filter: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().add_filter(filter, &[ext]).set_file_name(format!("{name}.{ext}")).save_file()
}

impl App {
    fn report(&mut self, r: Result<String, String>) {
        match r {
            Ok(m) => self.toast(m),
            Err(e) => self.toast(format!("Not possible: {e}")),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Track header

    pub(super) fn track_menu(&mut self, ui: &mut egui::Ui, id: u64) {
        let Some(t) = self.project.track(id).cloned() else { return };
        let is_group = t.instrument.kind == InstrumentKind::Group;
        ui.set_min_width(210.0);

        // Edit
        if item(ui, "Rename") {
            self.renaming = Some((RenameTarget::Track(id), t.name.clone()));
        }
        if item(ui, "Edit Info Text…") {
            self.info_edit = Some((id, t.info.clone()));
        }
        ui.separator();
        if item_on(ui, "Cut Track", !t.locked) {
            self.cut_track(id);
        }
        if item(ui, "Copy Track") {
            self.copy_track(id);
        }
        if item_on(ui, "Paste Track", self.track_clipboard.is_some()) {
            self.paste_track(Some(id));
        }
        if item(ui, "Duplicate Track") {
            self.duplicate_track(id);
        }
        if item_on(ui, "New Track with Same Settings", !is_group) {
            self.new_track_same_settings(id);
        }
        if item_on(ui, "Delete Track", !t.locked) {
            self.delete_track(id);
        }
        ui.separator();

        // Insert
        ui.menu_button("Insert Track", |ui| {
            let at = self.project.track_index(id).map(|i| i + 1);
            if item(ui, "Instrument Track (default)") {
                self.insert_default_track(false, at);
            }
            if item(ui, "Audio Track (default)") {
                self.insert_default_track(true, at);
            }
            ui.separator();
            for (label, preset) in [("Drum Track", "Punch Kit"), ("Bass Track", "Classic Acid"), ("Synth Track", "Dub Stab"), ("Sampler Track", "Sampler")] {
                if item(ui, label) {
                    self.add_track_from_preset(preset, at);
                }
            }
            ui.separator();
            if item_on(ui, "Return Track", self.project.returns.len() < MAX_RETURNS) {
                let r = self.add_return_track().map(|k| format!("Added return track {}", return_letter(k)));
                self.report(r);
            }
        });

        // Organise
        ui.menu_button("Group & Folder", |ui| {
            if is_group {
                if check(ui, "Folder only (no shared bus)", t.folder) {
                    self.set_folder(id, !t.folder);
                }
                if item(ui, "Ungroup Tracks") {
                    self.ungroup(id);
                }
                if check(ui, "Fold Members", t.folded) {
                    if let Some(g) = self.project.track_mut(id) {
                        g.folded = !g.folded;
                    }
                    self.touch();
                }
            } else if t.group.is_some() {
                if item(ui, "Remove from Group") {
                    if let Some(tr) = self.project.track_mut(id) {
                        tr.group = None;
                    }
                    self.touch();
                }
            } else {
                if item(ui, "Group Track (shared bus)   Ctrl+G") {
                    self.group_track(id);
                }
                if item(ui, "Put into New Folder") {
                    self.make_folder(id);
                }
                let groups: Vec<(u64, String)> = self.project.tracks.iter().filter(|g| g.instrument.kind == InstrumentKind::Group).map(|g| (g.id, g.name.clone())).collect();
                if !groups.is_empty() {
                    ui.menu_button("Move into Group", |ui| {
                        for (gid, gname) in groups {
                            if item(ui, &gname) {
                                self.move_into_group(id, gid);
                            }
                        }
                    });
                }
            }
            ui.separator();
            if item(ui, "Delete Unused Tracks") {
                let n = self.delete_unused_tracks();
                self.toast(format!("Deleted {n} unused track{}", if n == 1 { "" } else { "s" }));
            }
        });

        ui.menu_button("Color & Icon", |ui| {
            ui.label("Color");
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(8.0 * 18.0);
                for c in CLIP_COLORS {
                    if widgets::color_swatch(ui, color32(c), t.color == c).clicked() {
                        self.set_track_color(id, c);
                        ui.close_menu();
                    }
                }
            });
            if item(ui, "Auto-Color All Tracks") {
                self.auto_color_tracks();
            }
            ui.separator();
            ui.label("Icon");
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(8.0 * 26.0);
                for icon in TRACK_ICONS {
                    if ui.selectable_label(t.icon == icon, icon).clicked() {
                        self.set_track_icon(id, icon);
                        ui.close_menu();
                    }
                }
            });
            if item_on(ui, "No Icon", !t.icon.is_empty()) {
                self.set_track_icon(id, "");
            }
        });

        // Render
        ui.menu_button("Render", |ui| {
            let can = !is_group && !t.locked;
            if t.frozen.is_some() {
                if item(ui, "Unfreeze Track") {
                    let r = self.unfreeze_track(id);
                    self.report(r);
                }
                if item(ui, "Flatten (make freeze permanent)") {
                    let r = self.flatten_track(id);
                    self.report(r);
                }
            } else {
                if item_on(ui, "Freeze Track", can) {
                    let r = self.freeze_track(id);
                    self.report(r);
                }
                if item_on(ui, "Freeze and Flatten", can) {
                    let r = self.flatten_track(id);
                    self.report(r);
                }
            }
            ui.separator();
            if item_on(ui, "Bounce in Place (with effects)", can && t.frozen.is_none()) {
                let r = self.bounce_in_place(id, true);
                self.report(r);
            }
            if item_on(ui, "Bounce in Place (effects stay live)", can && t.frozen.is_none()) {
                let r = self.bounce_in_place(id, false);
                self.report(r);
            }
            if item_on(ui, "Bounce to New Track", !is_group) {
                let r = self.bounce_to_new_track(id).map(|_| "Bounced to a new audio track (original muted)".to_string());
                self.report(r);
            }
        });

        // View
        ui.menu_button("View", |ui| {
            let auto_on = self.auto_open.contains(&id);
            if check(ui, "Show Automation", auto_on) {
                if auto_on {
                    self.auto_open.remove(&id);
                } else {
                    self.auto_open.insert(id);
                }
            }
            if check(ui, "Show Take Lanes", t.show_takes) {
                if let Some(tr) = self.project.track_mut(id) {
                    tr.show_takes = !tr.show_takes;
                }
                self.touch();
            }
            if item_on(ui, "Add Take Lane", !is_group) {
                self.add_take_lane(id);
            }
            ui.separator();
            if check(ui, "Hide Track", t.hidden) {
                self.set_track_hidden(id, !t.hidden);
            }
            let n_hidden = self.project.tracks.iter().filter(|x| x.hidden).count();
            if check(ui, &format!("Show Hidden Tracks ({n_hidden})"), self.show_hidden) {
                self.show_hidden = !self.show_hidden;
            }
            if item_on(ui, "Unhide All Tracks", n_hidden > 0) {
                self.unhide_all_tracks();
            }
            if check(ui, "Protect Track", t.locked) {
                self.set_track_locked(id, !t.locked);
            }
            ui.separator();
            if item_on(ui, "Select All Clips", !t.arrangement.is_empty()) {
                self.select_all_clips(id);
            }
            if item(ui, "Configure Track Header…") {
                self.header_cfg.open = true;
            }
        });

        if !is_group && t.instrument.kind != InstrumentKind::Audio {
            ui.menu_button("Replace Instrument", |ui| {
                ui.set_max_height(420.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut last = None;
                    for p in crate::presets::INSTRUMENT_PRESETS {
                        if last != Some(p.kind) {
                            ui.label(egui::RichText::new(p.kind.name()).size(10.0).color(TEXT_DIM));
                            last = Some(p.kind);
                        }
                        if item(ui, p.name) {
                            self.replace_instrument(id, p.name);
                        }
                    }
                });
            });
        }
        if item_on(ui, "Save as Default Track", !is_group) {
            let r = self.save_default_track(id);
            self.report(r);
        }
    }

    /// Menu of a return track header.
    pub(super) fn return_menu(&mut self, ui: &mut egui::Ui, k: usize) {
        if item_on(ui, "Add Return Track", self.project.returns.len() < MAX_RETURNS) {
            let r = self.add_return_track().map(|k| format!("Added return track {}", return_letter(k)));
            self.report(r);
        }
        if item_on(ui, "Delete Return Track", self.project.returns.len() > 1) {
            self.delete_return_track(k);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Clips

    /// Entries shared by Launcher and timeline clips.
    fn clip_common_items(&mut self, ui: &mut egui::Ui, r: ClipRef) {
        let Some(c) = self.clip(r).cloned() else { return };
        let is_audio = c.audio.is_some();
        if item(ui, "Rename") {
            self.renaming = Some((RenameTarget::Clip(r), c.name.clone()));
        }
        if check(ui, "Mute Clip", c.muted) {
            self.toggle_clip_mute(r);
        }
        ui.menu_button("Color", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(8.0 * 18.0);
                for col in CLIP_COLORS {
                    if widgets::color_swatch(ui, color32(col), c.color == col).clicked() {
                        if let Some(cl) = self.clip_mut(r) {
                            cl.color = col;
                        }
                        self.touch();
                        ui.close_menu();
                    }
                }
            });
        });
        if c.link != 0 && item(ui, "Unlink (make independent)") {
            self.unlink_clip(r);
        }
        ui.separator();
        if is_audio {
            ui.menu_button("Audio", |ui| {
                if item(ui, "Reverse") {
                    let res = self.reverse_audio_clip(r).map(|_| "Reversed".to_string());
                    self.report(res);
                }
                ui.menu_button("Warp", |ui| {
                    let cur = c.audio.as_ref().map(|a| a.warp);
                    for (w, label) in [(WarpMode::Off, "Off (original speed)"), (WarpMode::Repitch, "Repitch (speed and pitch follow tempo)"), (WarpMode::Beats, "Beats (tempo without pitch change)")] {
                        if check(ui, label, cur == Some(w)) {
                            self.set_warp(r, w);
                        }
                    }
                    ui.separator();
                    if item(ui, "Fit to Bars (detect tempo)") {
                        let res = self.fit_audio_to_bars(r);
                        self.report(res);
                    }
                });
                if item(ui, "Pitch Correction (add to track)") {
                    self.add_pitch_correction(r.track());
                }
                ui.separator();
                ui.menu_button("Convert to Sampler Track", |ui| {
                    for (kind, label) in [(InstrumentKind::Slicer, "Slices by Transients (Slicer)"), (InstrumentKind::BeatPads, "Slices on Pads (Beat Pads)"), (InstrumentKind::MultiSampler, "Whole Sample (Multi Sampler)")] {
                        if item(ui, label) {
                            let res = self.convert_to_sampler(r, kind).map(|_| format!("Created a {} track", kind.name()));
                            self.report(res);
                        }
                    }
                });
                ui.menu_button("Audio to MIDI", |ui| {
                    for (mode, label) in [(MidiMode::Melody, "Melody"), (MidiMode::Harmony, "Harmony"), (MidiMode::Drums, "Drums")] {
                        if item(ui, label) {
                            let res = self.audio_to_midi(r, mode).map(|(_, n)| format!("{n} notes on a new track"));
                            self.report(res);
                            self.touch();
                        }
                    }
                });
                if item(ui, "Split into Stems") {
                    let res = self.split_stems(r).map(|ids| format!("{} stem tracks created", ids.len()));
                    self.report(res);
                }
            });
        } else {
            ui.menu_button("Notes", |ui| {
                ui.menu_button("Quantize", |ui| {
                    for (g, label) in [(2u32, "1/8"), (4, "1/4"), (8, "1/2"), (16, "1 Bar")] {
                        if item(ui, label) {
                            self.quantize_clip(r, g);
                        }
                    }
                });
                if item(ui, "Legato") {
                    self.legato_clip(r);
                }
                if item(ui, "Double Note Lengths") {
                    self.scale_note_lengths(r, 2.0);
                }
                if item(ui, "Halve Note Lengths") {
                    self.scale_note_lengths(r, 0.5);
                }
                ui.separator();
                if item(ui, "Velocity: All 100") {
                    self.set_velocities(r, 100);
                }
                if item(ui, "Velocity: Even Out") {
                    self.compress_velocities(r);
                }
                if item(ui, "Velocity: Humanize") {
                    self.humanize_velocities(r);
                }
            });
        }
        ui.menu_button("Export", |ui| {
            if item(ui, "Export Audio File…") {
                if let Some(p) = save_dialog(&c.name, "wav", "WAV") {
                    let res = self.export_clip_audio(r, &p, false).map(|p| format!("Exported {p}"));
                    self.report(res);
                }
            }
            if item(ui, "Export as Loop (tempo in name)…") {
                if let Some(p) = save_dialog(&c.name, "wav", "WAV") {
                    let res = self.export_clip_audio(r, &p, true).map(|p| format!("Exported {p}"));
                    self.report(res);
                }
            }
            if item_on(ui, "Export MIDI File…", !is_audio) {
                if let Some(p) = save_dialog(&c.name, "mid", "MIDI") {
                    let res = self.export_clip_midi(r, &p).map(|p| format!("Exported {p}"));
                    self.report(res);
                }
            }
        });
    }

    /// Timeline clip menu. `at` is the step under the mouse.
    pub(super) fn arr_clip_menu(&mut self, ui: &mut egui::Ui, track: u64, id: u64, at: u32) {
        ui.set_min_width(220.0);
        let r = ClipRef::Arr { track, id };
        let Some(ac) = self.project.track(track).and_then(|t| t.arrangement.iter().find(|c| c.id == id)).cloned() else { return };
        let locked = self.track_locked(track);
        if item_on(ui, "Cut", !locked) {
            self.clip_clipboard = Some((ac.clip.clone(), ac.len));
            self.sel_clip = Some(r);
            self.delete_selected_clip();
        }
        if item(ui, "Copy") {
            self.clip_clipboard = Some((ac.clip.clone(), ac.len));
        }
        if item_on(ui, "Paste after Clip", self.clip_clipboard.is_some() && !locked) {
            self.paste_clip_at(track, ac.start + ac.len);
        }
        if item(ui, "Duplicate   (Ctrl+D)") {
            self.sel_clip = Some(r);
            self.duplicate_selected_clip();
        }
        if item_on(ui, "Duplicate as Linked Copy", !locked) {
            let res = self.duplicate_linked(track, id).map(|_| "Linked copy: editing one edits both".to_string());
            self.report(res);
        }
        if item_on(ui, "Delete   (Del)", !locked) {
            self.sel_clip = Some(r);
            self.delete_selected_clip();
        }
        ui.separator();
        let ph = self.status.arr_pos.floor() as u32;
        if item_on(ui, "Split at Mouse Position", !locked) {
            let res = self.split_clip(track, id, at).map(|_| "Split".to_string());
            self.report(res);
        }
        if item_on(ui, "Split at Playhead", !locked && ph > ac.start && ph < ac.start + ac.len) {
            let res = self.split_clip(track, id, ph).map(|_| "Split".to_string());
            self.report(res);
        }
        let multi: Vec<u64> = self.arr.multi.iter().filter(|(t, _)| *t == track).map(|(_, c)| *c).collect();
        let mut join_ids = multi.clone();
        if !join_ids.contains(&id) {
            join_ids.push(id);
        }
        if item_on(ui, "Join Selected Clips", !locked && join_ids.len() > 1) {
            let res = self.join_clips(track, &join_ids).map(|_| "Joined".to_string());
            self.report(res);
        }
        let in_loop = self.loop_range().is_some_and(|(s, l)| ac.start < s + l && ac.start + ac.len > s);
        if item_on(ui, "Crop to Loop Region", !locked && in_loop) {
            let res = self.crop_clip_to_loop(track, id).map(|_| "Cropped".to_string());
            self.report(res);
        }
        if ac.len > ac.clip.len {
            if item(ui, "Unloop (play once)") {
                self.unloop_clip(track, id);
            }
        } else if item(ui, "Loop to Next Clip") {
            self.loop_clip(track, id);
        }
        if item(ui, "Set Loop to Clip") {
            self.project.loop_start = ac.start;
            self.project.loop_len = ac.len;
            self.project.loop_on = true;
            self.touch();
        }
        let has_takes = self.project.track(track).is_some_and(|t| !t.takes.is_empty());
        if item_on(ui, "Move to Take Lane", !locked) {
            self.move_clip_to_take(track, id);
        }
        let _ = has_takes;
        ui.separator();
        self.clip_common_items(ui, r);
    }

    /// Launcher clip slot menu.
    pub(super) fn session_clip_menu(&mut self, ui: &mut egui::Ui, track: u64, scene: usize) {
        ui.set_min_width(210.0);
        let r = ClipRef::Session { track, scene };
        let has_clip = self.clip(r).is_some();
        let locked = self.track_locked(track);
        if has_clip {
            if item(ui, "Launch") {
                self.send(crate::audio::engine::Cmd::Launch { track, scene });
            }
            if item_on(ui, "Cut", !locked) {
                self.clip_clipboard = self.clip(r).cloned().map(|c| {
                    let l = c.len;
                    (c, l)
                });
                self.sel_clip = Some(r);
                self.delete_selected_clip();
            }
            if item(ui, "Copy") {
                self.clip_clipboard = self.clip(r).cloned().map(|c| {
                    let l = c.len;
                    (c, l)
                });
            }
        }
        if item_on(ui, "Paste", self.clip_clipboard.is_some() && !locked) {
            if let Some((c, _)) = self.clip_clipboard.clone() {
                self.create_clip(track, scene, c);
            }
        }
        if has_clip {
            if item(ui, "Duplicate   (Ctrl+D)") {
                self.sel_clip = Some(r);
                self.duplicate_selected_clip();
            }
            if item_on(ui, "Delete   (Del)", !locked) {
                self.sel_clip = Some(r);
                self.delete_selected_clip();
            }
            ui.menu_button("Next Action", |ui| {
                let cur = self.clip(r).and_then(|c| c.follow);
                if check(ui, "None", cur.is_none()) {
                    if let Some(c) = self.clip_mut(r) {
                        c.follow = None;
                    }
                    self.touch();
                }
                for a in FollowAction::ALL {
                    if check(ui, a.name(), cur.is_some_and(|f| f.action == a)) {
                        if let Some(c) = self.clip_mut(r) {
                            let (bars, chance) = c.follow.map(|f| (f.bars, f.chance)).unwrap_or((c.bars().max(1), 1.0));
                            c.follow = Some(Follow { action: a, bars, chance });
                        }
                        self.touch();
                    }
                }
            });
            ui.separator();
            self.clip_common_items(ui, r);
        } else {
            if item_on(ui, "Insert MIDI Clip", !locked) {
                self.new_empty_clip(track, scene);
            }
            if item_on(ui, "Capture Played Notes Here", !locked) {
                self.select_track(track);
                self.sel_scene = scene;
                self.capture_midi();
            }
        }
        ui.separator();
        if item(ui, "Insert Scene Below") {
            self.project.add_scene(scene + 1);
            self.touch();
        }
    }

    /// Menu on empty timeline space: paste and the time commands on the loop region.
    pub(super) fn lane_menu(&mut self, ui: &mut egui::Ui, track: u64, at: u32) {
        ui.set_min_width(230.0);
        let locked = self.track_locked(track);
        if item_on(ui, "Paste Here", self.clip_clipboard.is_some() && !locked) {
            self.paste_clip_at(track, at);
        }
        if item_on(ui, "Insert MIDI Clip Here", !locked) {
            let (name, color) = self.project.track(track).map(|t| (t.name.clone(), t.color)).unwrap_or_default();
            let clip = Clip::new(&name, color, 1);
            let id = self.project.new_id();
            let len = clip.len;
            if let Some(t) = self.project.track_mut(track) {
                t.arrangement.push(ArrClip { id, start: at / 4 * 4, len, clip });
            }
            self.resolve_overlaps(track, id);
            self.touch();
        }
        ui.separator();
        let range = self.loop_range();
        let label = range.map(|(s, l)| format!(" (bars {}-{})", s / STEPS_PER_BAR + 1, (s + l).div_ceil(STEPS_PER_BAR))).unwrap_or_default();
        ui.label(egui::RichText::new(format!("Time - uses the loop region{label}")).size(10.0).color(TEXT_DIM));
        if item_on(ui, "Insert Silence", range.is_some()) {
            if let Some((s, l)) = range {
                self.insert_time(s, l);
            }
        }
        if item_on(ui, "Delete Time", range.is_some()) {
            if let Some((s, l)) = range {
                self.delete_time(s, l);
            }
        }
        if item_on(ui, "Duplicate Time", range.is_some()) {
            if let Some((s, l)) = range {
                self.duplicate_time(s, l);
            }
        }
        ui.separator();
        if item_on(ui, "Select All Clips of Track", self.project.track(track).is_some_and(|t| !t.arrangement.is_empty())) {
            self.select_all_clips(track);
        }
    }

    pub(super) fn paste_clip_at(&mut self, track: u64, at: u32) {
        let Some((mut clip, len)) = self.clip_clipboard.clone() else { return };
        if self.guard_locked(track) {
            return;
        }
        let id = self.project.new_id();
        if let Some(t) = self.project.track_mut(track) {
            clip.color = t.color;
            t.arrangement.push(ArrClip { id, start: at, len, clip });
        }
        self.resolve_overlaps(track, id);
        self.sel_clip = Some(ClipRef::Arr { track, id });
        self.touch();
    }

    // -----------------------------------------------------------------------------------------
    // Windows

    pub(super) fn track_windows(&mut self, ctx: &egui::Context) {
        if let Some((id, mut text)) = self.info_edit.take() {
            let name = self.project.track(id).map(|t| t.name.clone()).unwrap_or_default();
            let mut open = true;
            let mut done = false;
            egui::Window::new(format!("Info Text - {name}")).open(&mut open).collapsible(false).resizable(true).default_width(320.0).show(ctx, |ui| {
                ui.label(egui::RichText::new("A note about this track for you or collaborators. It appears in the help panel when you point at the track.").size(10.5).color(TEXT_DIM));
                ui.add(egui::TextEdit::multiline(&mut text).desired_rows(6).desired_width(f32::INFINITY));
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
            if done || !open {
                self.set_track_info(id, text);
            } else {
                self.info_edit = Some((id, text));
            }
        }
        if self.header_cfg.open {
            let mut cfg = self.header_cfg;
            let mut open = true;
            egui::Window::new("Track Header Buttons").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(egui::RichText::new("Buttons shown in the timeline track headers:").size(10.5).color(TEXT_DIM));
                ui.checkbox(&mut cfg.freeze, "Freeze");
                ui.checkbox(&mut cfg.protect, "Protect");
                ui.checkbox(&mut cfg.hide, "Hide");
                ui.checkbox(&mut cfg.arm, "Record Arm (audio tracks)");
                ui.checkbox(&mut cfg.takes, "Take Lanes");
            });
            cfg.open = open;
            if cfg != self.header_cfg {
                self.header_cfg = cfg;
                cfg.save();
            }
        }
    }
}
