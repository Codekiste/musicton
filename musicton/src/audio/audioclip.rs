//! Audio track playback with warping: Off, Re-Pitch and Beats (granular time-stretch).

use super::dsp::*;
use super::instruments::InstrumentDsp;
use super::samples::{self, Sample};
use crate::model::{AudioClip, WarpMode};
use std::f32::consts::PI;
use std::sync::Arc;

#[derive(Clone, Copy, Default)]
struct Grain {
    active: bool,
    src: f64,
    rate: f64,
    age: f32,
}

pub struct AudioPlayer {
    sr: f32,
    clip: Option<AudioClip>,
    sample: Option<Arc<Sample>>,
    /// Position in song beats relative to the clip start.
    beat: f64,
    bpm: f32,
    playing: bool,
    level: f32,
    jump_fade: f32,
    grains: [Grain; 4],
    hop_left: f32,
    rp_pos: f64,
}

const GRAIN_SECS: f32 = 0.06;

impl AudioPlayer {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            clip: None,
            sample: None,
            beat: 0.0,
            bpm: 120.0,
            playing: false,
            level: 0.0,
            jump_fade: 1.0,
            grains: [Grain::default(); 4],
            hop_left: 0.0,
            rp_pos: 0.0,
        }
    }

    /// Source position in seconds for a given clip position in beats.
    fn src_secs(&self, clip: &AudioClip, beat: f64) -> f64 {
        let secs = match clip.warp {
            WarpMode::Off => beat * 60.0 / self.bpm as f64,
            _ => beat * 60.0 / clip.bpm.max(1.0) as f64,
        };
        clip.offset as f64 + secs
    }

    #[inline]
    fn read(s: &Sample, pos: f64) -> (f32, f32) {
        if pos < 0.0 {
            return (0.0, 0.0);
        }
        let i = pos as usize;
        if i + 1 >= s.left.len() {
            return (0.0, 0.0);
        }
        let f = (pos - i as f64) as f32;
        (s.left[i] + (s.left[i + 1] - s.left[i]) * f, s.right[i] + (s.right[i + 1] - s.right[i]) * f)
    }
}

impl InstrumentDsp for AudioPlayer {
    fn set_params(&mut self, _: &[f32], _: &[Option<String>]) {}
    fn note_on(&mut self, _: u8, _: u8, _: Option<u32>, _: bool) {}
    fn note_off(&mut self, _: u8) {}
    fn all_off(&mut self) {
        self.playing = false;
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }

    fn audio_sync(&mut self, clip: Option<(&AudioClip, f64)>) {
        match clip {
            None => self.playing = false,
            Some((c, beat)) => {
                if self.clip.as_ref() != Some(c) {
                    if self.clip.as_ref().map(|x| &x.path) != Some(&c.path) || self.sample.is_none() {
                        self.sample = samples::try_get(&c.path);
                    }
                    self.clip = Some(c.clone());
                    self.jump_fade = 0.0;
                    self.grains = [Grain::default(); 4];
                    self.rp_pos = self.src_secs(c, beat);
                }
                // Resync if we drifted or wrapped around the clip loop.
                if !self.playing || (beat - self.beat).abs() > 0.05 {
                    if self.playing {
                        self.jump_fade = 0.0;
                    }
                    self.rp_pos = self.src_secs(c, beat);
                    self.grains = [Grain::default(); 4];
                    self.hop_left = 0.0;
                }
                self.beat = beat;
                self.playing = true;
            }
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (Some(clip), Some(s)) = (self.clip.clone(), self.sample.clone()) else { return };
        let sr = self.sr;
        let gain = db_to_gain(clip.gain);
        let beat_inc = self.bpm as f64 / 60.0 / sr as f64;
        let file_rate = s.sample_rate as f64;
        let grain_len = GRAIN_SECS * sr;
        let hop = grain_len / 2.0;
        let pitch = 2f64.powf(clip.transpose as f64 / 12.0);
        for i in 0..l.len() {
            let target = if self.playing { 1.0 } else { 0.0 };
            self.level += (target - self.level) * 0.004;
            self.jump_fade = (self.jump_fade + 1.0 / 128.0).min(1.0);
            if self.level < 0.0001 && !self.playing {
                continue;
            }
            let (mut a, mut b) = (0.0, 0.0);
            match clip.warp {
                WarpMode::Off | WarpMode::Repitch => {
                    let rate = match clip.warp {
                        WarpMode::Repitch => self.bpm as f64 / clip.bpm.max(1.0) as f64,
                        _ => pitch,
                    };
                    (a, b) = Self::read(&s, self.rp_pos * file_rate);
                    self.rp_pos += rate / sr as f64;
                }
                WarpMode::Beats => {
                    self.hop_left -= 1.0;
                    if self.hop_left <= 0.0 {
                        self.hop_left = hop;
                        let src = self.src_secs(&clip, self.beat) * file_rate;
                        if let Some(g) = self.grains.iter_mut().find(|g| !g.active) {
                            g.active = true;
                            g.age = 0.0;
                            g.rate = pitch * file_rate / sr as f64;
                            g.src = src;
                        }
                    }
                    for g in self.grains.iter_mut().filter(|g| g.active) {
                        let w = (PI * g.age / grain_len).sin().powi(2);
                        let (x, y) = Self::read(&s, g.src);
                        a += x * w;
                        b += y * w;
                        g.src += g.rate;
                        g.age += 1.0;
                        if g.age >= grain_len {
                            g.active = false;
                        }
                    }
                }
            }
            if self.playing {
                self.beat += beat_inc;
            }
            let g = gain * self.level * self.jump_fade;
            l[i] += a * g;
            r[i] += b * g;
        }
    }
}
