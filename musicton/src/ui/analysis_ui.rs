//! Audio clip tools: convert to MIDI (drums, melody, harmony) and split into stems.

use super::{App, ClipRef};
use crate::audio::analysis::{self, MidiMode};
use crate::audio::samples::{self, Sample};
use crate::model::*;
use std::sync::Arc;

impl App {
    /// The audio of a clip from its start offset, plus the tempo its steps refer to.
    fn clip_audio(&self, r: ClipRef) -> Result<(Sample, AudioClip, Clip), String> {
        let clip = self.clip(r).cloned().ok_or("no clip")?;
        let a = clip.audio.clone().ok_or("not an audio clip")?;
        let s: Arc<Sample> = samples::load(&a.path)?;
        let off = ((a.offset * s.sample_rate) as usize).min(s.left.len());
        Ok((Sample { left: s.left[off..].to_vec(), right: s.right[off..].to_vec(), sample_rate: s.sample_rate }, a, clip))
    }

    /// Places a new clip on a fresh track at the same spot (scene or timeline) as `r`.
    fn place_like(&mut self, r: ClipRef, track: u64, clip: Clip) {
        match r {
            ClipRef::Session { scene, .. } => {
                if let Some(t) = self.project.track_mut(track) {
                    if scene < t.clips.len() {
                        t.clips[scene] = Some(clip);
                    }
                }
            }
            ClipRef::Arr { track: src, id } => {
                let pos = self.project.track(src).and_then(|t| t.arrangement.iter().find(|c| c.id == id)).map(|c| (c.start, c.len));
                if let Some((start, len)) = pos {
                    let nid = self.project.new_id();
                    if let Some(t) = self.project.track_mut(track) {
                        t.arrangement.push(ArrClip { id: nid, start, len, clip });
                    }
                }
            }
        }
        self.touch();
    }

    pub(super) fn audio_to_midi(&mut self, r: ClipRef, mode: MidiMode) -> Result<(u64, usize), String> {
        let (s, a, clip) = self.clip_audio(r)?;
        let tempo = if a.warp == WarpMode::Off { self.project.bpm } else { a.bpm };
        let mut notes = analysis::audio_to_midi(&s, mode, tempo);
        notes.retain(|n| n.step < clip.len);
        if notes.is_empty() {
            return Err("nothing detected".into());
        }
        let preset = match mode {
            MidiMode::Drums => "Punch Kit",
            MidiMode::Melody => "Voltage Pluck",
            MidiMode::Harmony => "Voltage Pad",
        };
        let track = self.add_track_from_preset(preset, None);
        let label = match mode {
            MidiMode::Drums => "Drums",
            MidiMode::Melody => "Melody",
            MidiMode::Harmony => "Harmony",
        };
        if let Some(t) = self.project.track_mut(track) {
            t.name = format!("{} {label}", clip.name);
        }
        let color = self.project.track(track).map(|t| t.color).unwrap_or(clip.color);
        let mut mc = Clip::new(&format!("{} ({label})", clip.name), color, 1);
        mc.len = clip.len;
        let n = notes.len();
        mc.notes = notes;
        self.place_like(r, track, mc);
        Ok((track, n))
    }

    pub(super) fn split_stems(&mut self, r: ClipRef) -> Result<Vec<u64>, String> {
        let (s, a, clip) = self.clip_audio(r)?;
        let stems = analysis::split_stems(&s);
        let dir = dirs::data_dir().unwrap_or_else(|| ".".into()).join("musicton").join("stems");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let base: String = std::path::Path::new(&a.path).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let mut ids = vec![];
        for (name, (l, rr)) in [("Drums", &stems.drums), ("Bass", &stems.bass), ("Lead", &stems.lead), ("Other", &stems.other)] {
            let path = dir.join(format!("{base} - {name} {stamp}.wav"));
            let spec = hound::WavSpec { channels: 2, sample_rate: s.sample_rate as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
            let mut w = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
            for (x, y) in l.iter().zip(rr.iter()) {
                w.write_sample(*x).map_err(|e| e.to_string())?;
                w.write_sample(*y).map_err(|e| e.to_string())?;
            }
            w.finalize().map_err(|e| e.to_string())?;
            let p = path.to_string_lossy().to_string();
            samples::load(&p)?;
            let track = self.add_audio_track(None);
            if let Some(t) = self.project.track_mut(track) {
                t.name = format!("{} {name}", clip.name);
            }
            let mut c = clip.clone();
            c.name = format!("{} {name}", clip.name);
            c.audio = Some(AudioClip { path: p, offset: 0.0, ..a.clone() });
            self.place_like(r, track, c);
            ids.push(track);
        }
        // Silence the original so the stems replace it.
        let src = match r {
            ClipRef::Session { track, .. } | ClipRef::Arr { track, .. } => track,
        };
        if let Some(t) = self.project.track_mut(src) {
            t.channel.mute = true;
        }
        self.touch();
        Ok(ids)
    }
}
