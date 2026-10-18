//! Multi Sampler (key/velocity zones with round robin) and Slicer (loop chopping).

use super::dsp::*;
use super::instruments::InstrumentDsp;
use super::samples::{self, Sample};
use crate::model::{Instrument, Zone};
use crate::params::{self, multi, slicer};
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------
// Slice detection (shared with the UI, which draws the markers and builds clips).

/// Slice start positions in frames. `mode` 0 = transients, 1..3 = 8/16/32 equal slices.
pub fn detect_slices(s: &Sample, mode: usize, sensitivity: f32) -> Vec<usize> {
    let len = s.left.len();
    if len < 256 {
        return vec![0];
    }
    if mode > 0 {
        let n = [8, 16, 32][(mode - 1).min(2)];
        return (0..n).map(|k| k * len / n).collect();
    }
    // Energy-rise onset detection on 10 ms frames.
    let hop = (s.sample_rate * 0.01) as usize;
    let frames = len / hop;
    let energy: Vec<f32> = (0..frames)
        .map(|f| {
            let a = f * hop;
            (a..a + hop).map(|i| s.left[i] * s.left[i] + s.right[i] * s.right[i]).sum::<f32>() / hop as f32
        })
        .collect();
    let peak = energy.iter().cloned().fold(1e-9, f32::max);
    let thresh = 1.3 + (1.0 - sensitivity) * 4.0;
    let floor = peak * 0.002 * (1.0 - sensitivity * 0.9);
    let min_gap = ((s.sample_rate * 0.07) as usize / hop).max(1);
    let mut out = vec![0usize];
    let mut last = 0usize;
    for f in 2..frames {
        let prev = (energy[f - 1] + energy[f - 2]) * 0.5 + 1e-9;
        if energy[f] > floor && energy[f] / prev > thresh && f - last >= min_gap {
            // Back up to the start of the rise within the frame.
            out.push((f * hop).saturating_sub(hop / 2));
            last = f;
        }
    }
    out.dedup();
    out.truncate(64);
    out
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct MVoice {
    active: bool,
    pitch: u8,
    sample: Option<Arc<Sample>>,
    pos: f64,
    rate: f64,
    end: f64,
    amp: Adsr,
    fenv: Adsr,
    filt: [Svf; 2],
    gain: f32,
    vel: f32,
    gate_left: Option<u32>,
    age: u64,
    /// Release fade applied when a voice is stolen or cut.
    cut: f32,
}

pub struct MultiSampler {
    sr: f32,
    p: Vec<f32>,
    zones: Vec<(Zone, Option<Arc<Sample>>)>,
    voices: Vec<MVoice>,
    age: u64,
    rr: usize,
    rng: Rng,
}

impl MultiSampler {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: params::defaults(&params::MULTI_PARAMS), zones: vec![], voices: (0..32).map(|_| MVoice::default()).collect(), age: 0, rr: 0, rng: Rng::new(3) }
    }
}

fn steal(voices: &[MVoice], limit: usize) -> usize {
    let n = voices.len().min(limit.max(1));
    (0..n).find(|&i| !voices[i].active).or_else(|| (0..n).filter(|&i| voices[i].amp.is_released()).min_by_key(|&i| voices[i].age)).unwrap_or_else(|| (0..n).min_by_key(|&i| voices[i].age).unwrap_or(0))
}

impl InstrumentDsp for MultiSampler {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        for v in &mut self.voices {
            v.amp.set(p[multi::ATTACK], p[multi::DECAY], p[multi::SUSTAIN], p[multi::RELEASE], self.sr);
            v.fenv.set(0.002, p[multi::DECAY], 0.0, p[multi::RELEASE], self.sr);
        }
    }
    fn set_extra(&mut self, inst: &Instrument) {
        let same = self.zones.len() == inst.zones.len() && self.zones.iter().zip(&inst.zones).all(|(a, b)| a.0 == *b && a.1.is_some());
        if !same {
            self.zones = inst.zones.iter().map(|z| (z.clone(), samples::try_get(&z.path))).collect();
        }
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        let p = &self.p;
        let key = (pitch as i32 + p[multi::TRANSPOSE].round() as i32).clamp(0, 127) as u8;
        let hits: Vec<usize> = self.zones.iter().enumerate().filter(|(_, (z, s))| s.is_some() && key >= z.lo && key <= z.hi && vel >= z.vlo && vel <= z.vhi).map(|(i, _)| i).collect();
        if hits.is_empty() {
            return;
        }
        let chosen: Vec<usize> = match p[multi::ROUND_ROBIN].round() as i32 {
            1 => {
                self.rr = self.rr.wrapping_add(1);
                vec![hits[self.rr % hits.len()]]
            }
            2 => vec![hits[((self.rng.next() * 0.5 + 0.5) * hits.len() as f32) as usize % hits.len()]],
            _ => hits,
        };
        let limit = p[multi::VOICES].round() as usize;
        let vl = p[multi::VEL_LEVEL];
        let level = (1.0 - vl) + vl * vel as f32 / 127.0;
        let n = chosen.len() as f32;
        for zi in chosen {
            let (z, s) = &self.zones[zi];
            let s = s.clone().unwrap();
            self.age += 1;
            let i = steal(&self.voices, limit);
            let semis = key as f32 - z.root as f32 + z.tune / 100.0;
            let v = &mut self.voices[i];
            v.active = true;
            v.pitch = pitch;
            v.pos = 0.0;
            v.end = (s.left.len() - 1) as f64;
            v.rate = (s.sample_rate / self.sr) as f64 * 2f64.powf(semis as f64 / 12.0);
            v.sample = Some(s);
            v.gain = db_to_gain(z.gain) * level / n.sqrt();
            v.vel = vel as f32 / 127.0;
            v.gate_left = gate;
            v.age = self.age;
            v.cut = 1.0;
            v.amp.gate_on();
            v.fenv.gate_on();
        }
    }
    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch) {
            v.amp.gate_off();
            v.fenv.gate_off();
        }
    }
    fn all_off(&mut self) {
        for v in &mut self.voices {
            v.amp.gate_off();
            v.fenv.gate_off();
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = &self.p;
        let sr = self.sr;
        let vol = p[multi::VOLUME] * 0.8;
        let q = res_to_q(p[multi::RES]);
        let filter_on = p[multi::CUTOFF] < 19000.0 || p[multi::FENV] > 0.0;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let Some(s) = v.sample.clone() else {
                v.active = false;
                continue;
            };
            for i in 0..l.len() {
                if let Some(g) = v.gate_left.as_mut() {
                    if *g == 0 {
                        v.amp.gate_off();
                        v.fenv.gate_off();
                        v.gate_left = None;
                    } else {
                        *g -= 1;
                    }
                }
                if v.pos >= v.end {
                    v.active = false;
                    break;
                }
                let i0 = v.pos as usize;
                let fr = (v.pos - i0 as f64) as f32;
                let mut a = s.left[i0] * (1.0 - fr) + s.left[i0 + 1] * fr;
                let mut b = s.right[i0] * (1.0 - fr) + s.right[i0 + 1] * fr;
                let fe = v.fenv.next();
                if filter_on {
                    if i % 16 == 0 {
                        let cut = p[multi::CUTOFF] * 2f32.powf(p[multi::FENV] * fe * 6.0 + p[multi::VEL_CUT] * (v.vel - 1.0) * 4.0);
                        v.filt[0].set(cut, q, sr);
                        v.filt[1].set(cut, q, sr);
                    }
                    a = v.filt[0].process(a, FilterMode::Low);
                    b = v.filt[1].process(b, FilterMode::Low);
                }
                let g = v.amp.next() * v.gain * vol;
                l[i] += a * g;
                r[i] += b * g;
                v.pos += v.rate;
                if !v.amp.is_active() {
                    v.active = false;
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Slicer

#[derive(Default, Clone, Copy)]
struct SlVoice {
    active: bool,
    pitch: u8,
    pos: f64,
    start: f64,
    end: f64,
    rate: f64,
    env: f32,
    att: f32,
    releasing: bool,
    rel_mul: f32,
    level: f32,
    gate_left: Option<u32>,
    age: u64,
}

pub struct Slicer {
    sr: f32,
    p: Vec<f32>,
    sample: Option<Arc<Sample>>,
    path: Option<String>,
    slices: Vec<usize>,
    slice_key: (usize, i32),
    voices: [SlVoice; 8],
    bpm: f32,
    age: u64,
}

impl Slicer {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: params::defaults(&params::SLICER_PARAMS), sample: None, path: None, slices: vec![0], slice_key: (usize::MAX, -1), voices: [SlVoice::default(); 8], bpm: 120.0, age: 0 }
    }
}

impl InstrumentDsp for Slicer {
    fn set_params(&mut self, p: &[f32], samples_: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        let path = samples_.first().cloned().flatten();
        if path != self.path || (path.is_some() && self.sample.is_none()) {
            self.sample = path.as_deref().and_then(samples::try_get);
            self.path = path;
            self.slice_key = (usize::MAX, -1);
        }
        let mode = p[slicer::MODE].round() as usize;
        let sens = (p[slicer::SENS] * 100.0) as i32;
        if self.slice_key != (mode, sens) {
            if let Some(s) = &self.sample {
                self.slices = detect_slices(s, mode, p[slicer::SENS]);
                self.slice_key = (mode, sens);
            }
        }
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        let Some(s) = self.sample.clone() else { return };
        let idx = pitch as i32 - 36;
        if idx < 0 || idx as usize >= self.slices.len() {
            return;
        }
        let k = idx as usize;
        let len = s.left.len();
        let start = self.slices[k] as f64;
        let end = self.slices.get(k + 1).copied().unwrap_or(len - 1) as f64;
        let p = &self.p;
        let mut rate = (s.sample_rate / self.sr) as f64 * 2f64.powf(p[slicer::TRANSPOSE] as f64 / 12.0);
        if p[slicer::SYNC] > 0.5 {
            let loop_secs = len as f64 / s.sample_rate as f64;
            let target = p[slicer::BEATS] as f64 * 60.0 / self.bpm as f64;
            rate *= loop_secs / target;
        }
        let mode = p[slicer::PLAY].round() as i32;
        if mode != 1 {
            // Mono: the new slice cuts the previous ones.
            for v in self.voices.iter_mut().filter(|v| v.active) {
                v.releasing = true;
                v.rel_mul = decay_mult(0.005, self.sr);
            }
        }
        self.age += 1;
        let i = (0..8).find(|&i| !self.voices[i].active).unwrap_or_else(|| (0..8).min_by_key(|&i| self.voices[i].age).unwrap());
        let reverse = p[slicer::REVERSE] > 0.5;
        self.voices[i] = SlVoice {
            active: true,
            pitch,
            pos: if reverse { end } else { start },
            start,
            end,
            rate: if reverse { -rate } else { rate },
            env: 0.0,
            att: 1.0 / (p[slicer::ATTACK] * self.sr).max(1.0),
            releasing: false,
            rel_mul: decay_mult(p[slicer::RELEASE], self.sr),
            level: 0.3 + 0.7 * vel as f32 / 127.0,
            gate_left: if mode == 2 { gate } else { None },
            age: self.age,
        };
    }
    fn note_off(&mut self, pitch: u8) {
        if self.p[slicer::PLAY].round() as i32 == 2 {
            for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch) {
                v.releasing = true;
            }
        }
    }
    fn all_off(&mut self) {
        for v in &mut self.voices {
            v.releasing = true;
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let Some(s) = self.sample.clone() else { return };
        let vol = self.p[slicer::VOLUME];
        let rel_tail = (self.p[slicer::RELEASE] * self.sr) as f64;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in 0..l.len() {
                if let Some(g) = v.gate_left.as_mut() {
                    if *g == 0 {
                        v.releasing = true;
                        v.gate_left = None;
                    } else {
                        *g -= 1;
                    }
                }
                // Fade out near the slice end so slices never click.
                let left = if v.rate > 0.0 { v.end - v.pos } else { v.pos - v.start };
                if left <= 0.0 || v.pos < 0.0 || v.pos >= (s.left.len() - 1) as f64 {
                    v.active = false;
                    break;
                }
                if left < rel_tail.min(256.0) {
                    v.releasing = true;
                }
                if v.releasing {
                    v.env *= v.rel_mul;
                    if v.env < 0.0005 {
                        v.active = false;
                        break;
                    }
                } else {
                    v.env = (v.env + v.att).min(1.0);
                }
                let i0 = v.pos as usize;
                let fr = (v.pos - i0 as f64) as f32;
                let g = v.env * v.level * vol;
                l[i] += (s.left[i0] * (1.0 - fr) + s.left[i0 + 1] * fr) * g;
                r[i] += (s.right[i0] * (1.0 - fr) + s.right[i0 + 1] * fr) * g;
                v.pos += v.rate;
            }
        }
    }
}

/// Notes that replay a sliced loop: slice k plays on key 36 + k at its original position.
/// `steps` is the loop length in 16th steps.
pub fn slice_notes(slices: &[usize], frames: usize, steps: u32) -> Vec<crate::model::Note> {
    let pos = |f: usize| ((f as f64 / frames.max(1) as f64) * steps as f64).round() as u32;
    let mut out: Vec<crate::model::Note> = vec![];
    for (k, &s) in slices.iter().enumerate().take(64) {
        let step = pos(s).min(steps.saturating_sub(1));
        let next = slices.get(k + 1).map(|&n| pos(n)).unwrap_or(steps);
        if out.last().is_some_and(|n| n.step == step) {
            continue;
        }
        out.push(crate::model::Note { step, len: next.saturating_sub(step).max(1), pitch: 36 + k as u8, vel: 100, slide: false, skip: 0 });
    }
    out
}
