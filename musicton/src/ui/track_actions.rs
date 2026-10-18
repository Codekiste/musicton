//! Track operations behind the track header menu: clipboard, templates, folders, hiding,
//! protection, return tracks, freezing and bouncing to audio.

use super::{App, ClipRef, Sel};
use crate::audio::samples;
use crate::model::*;
use std::collections::HashMap;
use std::path::PathBuf;

/// Symbols offered for the track header.
pub const TRACK_ICONS: [&str; 16] = ["🥁", "🎹", "🎸", "🎤", "🎧", "🔊", "🎺", "🎻", "⚡", "★", "♪", "♫", "❄", "🔥", "💎", "🌊"];

fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("musicton")
}

fn default_track_path(audio: bool) -> PathBuf {
    data_dir().join(if audio { "default_audio_track.json" } else { "default_instrument_track.json" })
}

fn stamp() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect()
}

impl App {
    pub(super) fn track_locked(&self, id: u64) -> bool {
        self.project.track(id).is_some_and(|t| t.locked)
    }

    /// Refuses edits on protected tracks with a short message.
    pub(super) fn guard_locked(&mut self, id: u64) -> bool {
        if self.track_locked(id) {
            let name = self.project.track(id).map(|t| t.name.clone()).unwrap_or_default();
            self.toast(format!("'{name}' is protected - right-click the track and turn off Protect Track to edit it"));
            true
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------------------------
    // Clipboard and copies

    /// Inserts a copy of `t` at `at` with fresh ids for the track, its devices and timeline clips.
    pub(super) fn insert_track_copy(&mut self, mut t: Track, at: usize) -> u64 {
        t.id = self.project.new_id();
        let mut id_map = HashMap::new();
        fn remap(devs: &mut [Device], p: &mut Project, map: &mut HashMap<u64, u64>) {
            for d in devs {
                let nid = p.new_id();
                map.insert(d.id, nid);
                d.id = nid;
                for c in &mut d.chains {
                    remap(&mut c.devices, p, map);
                }
            }
        }
        remap(&mut t.channel.devices, &mut self.project, &mut id_map);
        for c in &mut t.arrangement {
            c.id = self.project.new_id();
        }
        for lane in &mut t.takes {
            lane.id = self.project.new_id();
            for c in &mut lane.clips {
                c.id = self.project.new_id();
            }
        }
        for a in &mut t.automation {
            if let AutoTarget::Device { id, .. } = &mut a.target {
                if let Some(n) = id_map.get(id) {
                    *id = *n;
                }
            }
        }
        t.clips.resize(self.project.scenes.len(), None);
        if t.group.is_some_and(|g| self.project.track(g).is_none()) {
            t.group = None;
        }
        if let Output::Track(o) = t.output {
            if self.project.track(o).is_none() {
                t.output = Output::Master;
            }
        }
        let nid = t.id;
        let at = at.min(self.project.tracks.len());
        self.project.tracks.insert(at, t);
        self.select_track(nid);
        self.touch();
        nid
    }

    pub(super) fn copy_track(&mut self, id: u64) {
        if let Some(t) = self.project.track(id) {
            self.track_clipboard = Some(t.clone());
            self.toast(format!("Copied track '{}'", t.name));
        }
    }

    pub(super) fn cut_track(&mut self, id: u64) {
        if self.guard_locked(id) {
            return;
        }
        self.copy_track(id);
        self.delete_track(id);
    }

    /// Pastes the copied track after `after` (or at the end).
    pub(super) fn paste_track(&mut self, after: Option<u64>) -> Option<u64> {
        let t = self.track_clipboard.clone()?;
        let at = after.and_then(|a| self.project.track_index(a)).map(|i| i + 1).unwrap_or(self.project.tracks.len());
        Some(self.insert_track_copy(t, at))
    }

    /// Same instrument, devices and mixer settings, but no clips.
    pub(super) fn new_track_same_settings(&mut self, id: u64) -> Option<u64> {
        let i = self.project.track_index(id)?;
        let mut t = self.project.tracks[i].clone();
        t.clips = vec![None; self.project.scenes.len()];
        t.arrangement.clear();
        t.automation.clear();
        t.takes.clear();
        t.frozen = None;
        t.locked = false;
        t.name = format!("{} 2", t.name);
        Some(self.insert_track_copy(t, i + 1))
    }

    /// Deletes tracks without clips that nothing depends on. Returns how many were removed.
    pub(super) fn delete_unused_tracks(&mut self) -> usize {
        let p = &self.project;
        let unused: Vec<u64> = p
            .tracks
            .iter()
            .filter(|t| {
                let has_clips = t.clips.iter().any(|c| c.is_some()) || !t.arrangement.is_empty() || t.takes.iter().any(|l| !l.clips.is_empty());
                let is_target = p.tracks.iter().any(|o| o.group == Some(t.id) || o.output == Output::Track(t.id))
                    || p.tracks.iter().flat_map(|o| &o.channel.devices).any(|d| d.sc_source == Some(t.id));
                !has_clips && !is_target && !t.locked && !t.arm
            })
            .map(|t| t.id)
            .collect();
        for id in &unused {
            self.delete_track(*id);
        }
        unused.len()
    }

    // -----------------------------------------------------------------------------------------
    // Organisation

    /// A folder groups tracks for overview only: members keep their own outputs.
    pub(super) fn make_folder(&mut self, id: u64) {
        let Some(i) = self.project.track_index(id) else { return };
        if self.project.tracks[i].instrument.kind == InstrumentKind::Group || self.project.tracks[i].group.is_some() {
            return;
        }
        let color = self.project.tracks[i].color;
        let gid = self.project.add_track("Folder", InstrumentKind::Group, color);
        let mut g = self.project.tracks.pop().unwrap();
        g.folder = true;
        g.icon = "📁".into();
        self.project.tracks.insert(i, g);
        if let Some(t) = self.project.track_mut(id) {
            t.group = Some(gid);
        }
        self.select_track(gid);
        self.touch();
        self.toast("Created a folder. Right-click other tracks > Move into Group to add them.");
    }

    pub(super) fn set_folder(&mut self, gid: u64, folder: bool) {
        if let Some(g) = self.project.track_mut(gid) {
            g.folder = folder;
        }
        self.touch();
    }

    /// Gives every track (and its clips) its own color from the palette.
    pub(super) fn auto_color_tracks(&mut self) {
        for (i, t) in self.project.tracks.iter_mut().enumerate() {
            let c = CLIP_COLORS[(i * 5 + 1) % CLIP_COLORS.len()];
            t.color = c;
            for clip in t.clips.iter_mut().flatten() {
                clip.color = c;
            }
            for ac in &mut t.arrangement {
                ac.clip.color = c;
            }
        }
        self.touch();
    }

    pub(super) fn set_track_color(&mut self, id: u64, c: [u8; 3]) {
        if let Some(t) = self.project.track_mut(id) {
            t.color = c;
            for clip in t.clips.iter_mut().flatten() {
                clip.color = c;
            }
            for ac in &mut t.arrangement {
                ac.clip.color = c;
            }
        }
        self.touch();
    }

    pub(super) fn set_track_icon(&mut self, id: u64, icon: &str) {
        if let Some(t) = self.project.track_mut(id) {
            t.icon = icon.to_string();
        }
        self.touch();
    }

    pub(super) fn set_track_hidden(&mut self, id: u64, hidden: bool) {
        if let Some(t) = self.project.track_mut(id) {
            t.hidden = hidden;
        }
        if hidden && self.sel == Sel::Track(id) {
            self.sel = Sel::Master;
        }
        self.touch();
    }

    pub(super) fn unhide_all_tracks(&mut self) {
        for t in &mut self.project.tracks {
            t.hidden = false;
        }
        self.touch();
    }

    pub(super) fn set_track_locked(&mut self, id: u64, locked: bool) {
        if let Some(t) = self.project.track_mut(id) {
            t.locked = locked;
        }
        self.touch();
    }

    pub(super) fn set_track_info(&mut self, id: u64, info: String) {
        if let Some(t) = self.project.track_mut(id) {
            t.info = info;
        }
        self.touch();
    }

    /// Selects every timeline clip of the track (for Join, Delete and the like).
    pub(super) fn select_all_clips(&mut self, id: u64) {
        let ids: Vec<u64> = self.project.track(id).map(|t| t.arrangement.iter().map(|c| c.id).collect()).unwrap_or_default();
        self.arr.multi.clear();
        for cid in &ids {
            self.arr.multi.insert((id, *cid));
        }
        if let Some(first) = ids.first() {
            self.sel_clip = Some(ClipRef::Arr { track: id, id: *first });
        }
        self.select_track(id);
    }

    /// Swaps the instrument for a preset (clips are kept when the type matches).
    pub(super) fn replace_instrument(&mut self, id: u64, preset: &str) {
        if self.guard_locked(id) {
            return;
        }
        self.drop_on_track(id, super::Payload::Instrument(crate::presets::preset(preset).name));
    }

    // -----------------------------------------------------------------------------------------
    // Return tracks

    pub(super) fn add_return_track(&mut self) -> Result<usize, String> {
        let k = self.project.returns.len();
        if k >= MAX_RETURNS {
            return Err(format!("At most {MAX_RETURNS} return tracks"));
        }
        let color = CLIP_COLORS[(k * 7 + 4) % CLIP_COLORS.len()];
        self.project.returns.push(ReturnTrack { name: format!("{} Return", return_letter(k)), color, channel: Channel::default() });
        self.sel = Sel::Return(k);
        self.touch();
        Ok(k)
    }

    /// Removes return `k`; sends, outputs, automation and mappings of later returns move down.
    pub(super) fn delete_return_track(&mut self, k: usize) {
        if k >= self.project.returns.len() {
            return;
        }
        self.project.returns.remove(k);
        let fix = |j: u8| -> Option<u8> {
            match (j as usize).cmp(&k) {
                std::cmp::Ordering::Less => Some(j),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(j - 1),
            }
        };
        for t in &mut self.project.tracks {
            for i in k..MAX_RETURNS - 1 {
                t.channel.sends[i] = t.channel.sends[i + 1];
            }
            t.channel.sends[MAX_RETURNS - 1] = 0.0;
            if let Output::Return(j) = t.output {
                t.output = fix(j).map(Output::Return).unwrap_or(Output::Master);
            }
            t.automation.retain_mut(|a| match a.target {
                AutoTarget::Send(j) => match fix(j) {
                    Some(n) => {
                        a.target = AutoTarget::Send(n);
                        true
                    }
                    None => false,
                },
                _ => true,
            });
        }
        self.project.mappings.retain_mut(|m| match m.target {
            MapTarget::Send(id, j) => match fix(j) {
                Some(n) => {
                    m.target = MapTarget::Send(id, n);
                    true
                }
                None => false,
            },
            MapTarget::ReturnVolume(j) => match fix(j) {
                Some(n) => {
                    m.target = MapTarget::ReturnVolume(n);
                    true
                }
                None => false,
            },
            _ => true,
        });
        self.sel = Sel::Master;
        self.touch();
    }

    // -----------------------------------------------------------------------------------------
    // Default tracks (templates)

    pub(super) fn save_default_track(&mut self, id: u64) -> Result<String, String> {
        let mut t = self.project.track(id).cloned().ok_or("no such track")?;
        if t.instrument.kind == InstrumentKind::Group {
            return Err("group tracks cannot be a default track".into());
        }
        let audio = t.instrument.kind == InstrumentKind::Audio;
        t.clips.clear();
        t.arrangement.clear();
        t.automation.clear();
        t.takes.clear();
        t.frozen = None;
        t.group = None;
        t.output = Output::Master;
        t.locked = false;
        t.hidden = false;
        t.arm = false;
        std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
        let s = serde_json::to_string_pretty(&t).map_err(|e| e.to_string())?;
        std::fs::write(default_track_path(audio), s).map_err(|e| e.to_string())?;
        Ok(format!("'{}' is now the default {} track", t.name, if audio { "audio" } else { "instrument" }))
    }

    /// Inserts the saved default track of the type (or a plain one if none was saved).
    pub(super) fn insert_default_track(&mut self, audio: bool, at: Option<usize>) -> u64 {
        let saved: Option<Track> = std::fs::read_to_string(default_track_path(audio)).ok().and_then(|s| serde_json::from_str(&s).ok());
        match saved {
            Some(mut t) => {
                t.color = CLIP_COLORS[(self.project.tracks.len() * 5 + 1) % CLIP_COLORS.len()];
                let at = at.unwrap_or(self.project.tracks.len());
                self.insert_track_copy(t, at)
            }
            None if audio => self.add_audio_track(at),
            None => self.add_track_from_preset("Dub Stab", at),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Rendering: freeze, flatten, bounce

    /// Renders `clip` through the track (instrument, MIDI effects and optionally its devices) into a WAV.
    /// `seamless` renders two passes and keeps the second, so effect tails wrap around the loop point.
    pub(super) fn render_clip_audio(&self, t: &Track, clip: &Clip, len: u32, with_fx: bool, seamless: bool) -> Result<AudioClip, String> {
        if len == 0 {
            return Err("empty clip".into());
        }
        let mut c = clip.clone();
        c.muted = false;
        let steps = if seamless { len * 2 } else { len };
        let skip = if seamless { len } else { 0 };
        self.render_region_audio(t, vec![ArrClip { id: u64::MAX - 7, start: 0, len: steps, clip: c }], skip, len, with_fx, &clip.name)
    }

    /// Renders timeline clips (positions relative to 0) through the track and keeps `len` steps from `skip`.
    pub(super) fn render_region_audio(&self, t: &Track, clips: Vec<ArrClip>, skip: u32, len: u32, with_fx: bool, label: &str) -> Result<AudioClip, String> {
        let sr = self.audio.sample_rate;
        let mut p = self.project.clone();
        let mut tt = t.clone();
        tt.clips = vec![None; p.scenes.len()];
        tt.arrangement = clips;
        // Automation positions refer to the timeline, not to the clip.
        tt.automation.clear();
        tt.channel.volume = 0.85;
        tt.channel.pan = 0.0;
        tt.channel.mute = false;
        tt.channel.solo = false;
        tt.channel.sends = [0.0; MAX_RETURNS];
        tt.channel.xfade = 0;
        if !with_fx {
            tt.channel.devices.clear();
        }
        tt.group = None;
        tt.output = Output::Master;
        tt.takes.clear();
        tt.frozen = None;
        p.tracks = vec![tt];
        p.master = Channel::default();
        p.playlist.clear();
        p.crossfader = 0.0;
        let data = crate::audio::render_buffer(p, skip + len, 0.0, sr);
        let step_secs = 60.0 / self.project.bpm as f64 / 4.0;
        let frames = |s: u32| (s as f64 * step_secs * sr as f64).round() as usize;
        let b = frames(skip + len).min(data.len() / 2);
        let a = frames(skip).min(b);
        let dir = data_dir().join("renders");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{} - {} {}.wav", safe_name(&t.name), safe_name(label), stamp()));
        crate::audio::write_wav(&path, &data[a * 2..b * 2], sr)?;
        let ps = path.to_string_lossy().to_string();
        samples::load(&ps)?;
        Ok(AudioClip { path: ps, warp: WarpMode::Repitch, bpm: self.project.bpm, gain: 0.0, transpose: 0.0, offset: 0.0 })
    }

    pub(super) fn rendered_clip(src: &Clip, a: AudioClip, len: u32) -> Clip {
        Clip { name: src.name.clone(), color: src.color, len, notes: vec![], audio: Some(a), follow: src.follow, gain: 0.0, muted: src.muted, link: 0 }
    }

    /// An audio copy of the track: every clip rendered, instrument replaced by audio playback.
    pub(super) fn render_track_copy(&self, t: &Track, with_fx: bool) -> Result<Track, String> {
        if t.instrument.kind == InstrumentKind::Group {
            return Err("group tracks have no clips to render".into());
        }
        let mut out = t.clone();
        for (i, c) in t.clips.iter().enumerate() {
            if let Some(c) = c {
                let a = self.render_clip_audio(t, c, c.len, with_fx, true)?;
                out.clips[i] = Some(Self::rendered_clip(c, a, c.len));
            }
        }
        for (j, ac) in t.arrangement.iter().enumerate() {
            let a = self.render_clip_audio(t, &ac.clip, ac.len, with_fx, false)?;
            out.arrangement[j].clip = Self::rendered_clip(&ac.clip, a, ac.len);
        }
        out.instrument = Instrument::new(InstrumentKind::Audio);
        out.midi_fx.clear();
        out.modulators.clear();
        out.follow_chords = false;
        out.frozen = None;
        out.takes.clear();
        out.arm = false;
        if with_fx {
            out.channel.devices.clear();
            out.automation.retain(|a| matches!(a.target, AutoTarget::Volume | AutoTarget::Pan | AutoTarget::Send(_)));
        } else {
            out.automation.retain(|a| !matches!(a.target, AutoTarget::Instrument(_)));
        }
        Ok(out)
    }

    /// Renders the track to audio and keeps the original to unfreeze later (saves CPU).
    pub(super) fn freeze_track(&mut self, id: u64) -> Result<String, String> {
        let t = self.project.track(id).cloned().ok_or("no such track")?;
        if t.frozen.is_some() {
            return Err("already frozen".into());
        }
        if t.locked {
            return Err("track is protected".into());
        }
        let mut frozen = self.render_track_copy(&t, true)?;
        frozen.frozen = Some(Box::new(t));
        let name = frozen.name.clone();
        if let Some(slot) = self.project.track_mut(id) {
            *slot = frozen;
        }
        self.touch();
        Ok(format!("Froze '{name}'"))
    }

    /// Restores the original track; mixer settings made while frozen are kept.
    pub(super) fn unfreeze_track(&mut self, id: u64) -> Result<String, String> {
        let cur = self.project.track(id).cloned().ok_or("no such track")?;
        let mut orig = *cur.frozen.clone().ok_or("track is not frozen")?;
        orig.channel.volume = cur.channel.volume;
        orig.channel.pan = cur.channel.pan;
        orig.channel.mute = cur.channel.mute;
        orig.channel.solo = cur.channel.solo;
        orig.channel.sends = cur.channel.sends;
        orig.channel.xfade = cur.channel.xfade;
        orig.name = cur.name.clone();
        orig.color = cur.color;
        orig.icon = cur.icon.clone();
        orig.info = cur.info.clone();
        orig.output = cur.output;
        orig.group = cur.group;
        orig.hidden = cur.hidden;
        if let Some(slot) = self.project.track_mut(id) {
            *slot = orig;
        }
        self.touch();
        Ok(format!("Unfroze '{}'", cur.name))
    }

    /// Makes a freeze permanent (or bounces in place): the track stays plain audio.
    pub(super) fn flatten_track(&mut self, id: u64) -> Result<String, String> {
        let frozen = self.project.track(id).is_some_and(|t| t.frozen.is_some());
        if !frozen {
            self.freeze_track(id)?;
        }
        if let Some(t) = self.project.track_mut(id) {
            t.frozen = None;
        }
        self.touch();
        Ok("Flattened to audio".into())
    }

    /// Replaces the track's clips with rendered audio; without effects the devices stay live.
    pub(super) fn bounce_in_place(&mut self, id: u64, with_fx: bool) -> Result<String, String> {
        let t = self.project.track(id).cloned().ok_or("no such track")?;
        if t.locked {
            return Err("track is protected".into());
        }
        let out = self.render_track_copy(&t, with_fx)?;
        if let Some(slot) = self.project.track_mut(id) {
            *slot = out;
        }
        self.touch();
        Ok(format!("Bounced '{}' in place{}", t.name, if with_fx { " (with effects)" } else { "" }))
    }

    /// Renders the track into a new audio track below it and mutes the original.
    pub(super) fn bounce_to_new_track(&mut self, id: u64) -> Result<u64, String> {
        let t = self.project.track(id).cloned().ok_or("no such track")?;
        let mut out = self.render_track_copy(&t, true)?;
        out.name = format!("{} (Bounce)", t.name);
        out.locked = false;
        let i = self.project.track_index(id).unwrap_or(0);
        let nid = self.insert_track_copy(out, i + 1);
        if let Some(src) = self.project.track_mut(id) {
            src.channel.mute = true;
        }
        self.touch();
        Ok(nid)
    }
}
