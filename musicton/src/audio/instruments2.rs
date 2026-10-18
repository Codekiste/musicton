//! Sampler (sampler), Fm4 (FM), Morph and StringModel (physical string).

use super::dsp::*;
use super::instruments::InstrumentDsp;
use super::samples::{self, Sample};
use crate::params::{self, fmop, smp, strm, wavetable, OP_PARAMS};
use std::f32::consts::TAU;
use std::sync::{Arc, LazyLock};

fn pick_voice<T>(voices: &[T], active: impl Fn(&T) -> bool, released: impl Fn(&T) -> bool, age: impl Fn(&T) -> u64, limit: usize) -> usize {
    let n = voices.len().min(limit.max(1));
    if let Some(i) = (0..n).find(|&i| !active(&voices[i])) {
        return i;
    }
    if let Some(i) = (0..n).filter(|&i| released(&voices[i])).min_by_key(|&i| age(&voices[i])) {
        return i;
    }
    (0..n).min_by_key(|&i| age(&voices[i])).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Sampler

#[derive(Default)]
struct SVoice {
    active: bool,
    pitch: u8,
    pos: f64,
    rate: f64,
    target_rate: f64,
    start: f64,
    end: f64,
    looping: bool,
    gate: Option<u32>,
    held: bool,
    one_shot: bool,
    vel: f32,
    amp: Adsr,
    fl: Svf,
    fr: Svf,
    age: u64,
}

pub struct Sampler {
    sr: f32,
    p: Vec<f32>,
    sample: Option<Arc<Sample>>,
    path: Option<String>,
    voices: Vec<SVoice>,
    age: u64,
    bpm: f32,
    last_rate: f64,
}

impl Sampler {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            p: params::defaults(&params::SAMPLER_PARAMS),
            sample: None,
            path: None,
            voices: (0..16).map(|_| SVoice::default()).collect(),
            age: 0,
            bpm: 120.0,
            last_rate: 1.0,
        }
    }
}

impl InstrumentDsp for Sampler {
    fn set_params(&mut self, p: &[f32], samples_: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        let path = samples_.first().cloned().flatten();
        if path != self.path || (path.is_some() && self.sample.is_none()) {
            self.sample = path.as_deref().and_then(samples::try_get);
            self.path = path;
        }
        let (a, d, s, r) = (p[smp::ATTACK], p[smp::DECAY], p[smp::SUSTAIN], p[smp::RELEASE]);
        for v in &mut self.voices {
            v.amp.set(a, d, s, r, self.sr);
        }
    }

    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }

    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        let Some(s) = self.sample.clone() else { return };
        let p = &self.p;
        let len = s.left.len() as f64;
        if len < 2.0 {
            return;
        }
        let mode = p[smp::MODE].round() as i32;
        let (mut start, mut end) = ((p[smp::START] as f64 * len), (p[smp::END] as f64 * len));
        if end <= start + 16.0 {
            end = (start + 16.0).min(len);
        }
        let mut semis = p[smp::TRANSPOSE] + p[smp::DETUNE] / 100.0;
        if mode == 2 {
            let n = p[smp::SLICES].round().max(1.0) as i32;
            let idx = pitch as i32 - 36;
            if idx < 0 || idx >= n {
                return;
            }
            let slice = (end - start) / n as f64;
            start += slice * idx as f64;
            end = start + slice;
        } else if mode == 0 {
            semis += pitch as f32 - p[smp::ROOT];
        }
        let mut rate = (s.sample_rate / self.sr) as f64 * 2f64.powf(semis as f64 / 12.0);
        if p[smp::WARP] > 0.5 {
            // Sync: repitch so the whole region lasts `Beats` at the song tempo.
            let region_secs = (p[smp::END] - p[smp::START]).max(0.01) as f64 * len / s.sample_rate as f64;
            let target_secs = p[smp::BEATS] as f64 * 60.0 / self.bpm as f64;
            rate *= region_secs / target_secs;
        }
        self.age += 1;
        let limit = p[smp::VOICES].round() as usize;
        let mono = limit <= 1;
        let idx = pick_voice(&self.voices, |v| v.active, |v| v.amp.is_released(), |v| v.age, limit);
        let glide = p[smp::GLIDE];
        let v = &mut self.voices[idx];
        let legato = mono && v.active && glide > 0.0;
        v.target_rate = rate;
        if !legato {
            v.pos = start;
            v.rate = rate;
            v.amp.gate_on();
        } else {
            v.rate = self.last_rate;
        }
        self.last_rate = rate;
        v.active = true;
        v.pitch = pitch;
        v.start = start;
        v.end = end;
        v.looping = mode == 0 && p[smp::LOOP] > 0.5;
        v.one_shot = mode != 0;
        v.gate = gate;
        v.held = gate.is_none();
        v.vel = 1.0 - p[smp::VEL] * (1.0 - vel as f32 / 127.0);
        v.age = self.age;
    }

    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch && v.held) {
            v.held = false;
            if !v.one_shot {
                v.amp.gate_off();
            }
        }
    }

    fn all_off(&mut self) {
        for v in &mut self.voices {
            v.amp.gate_off();
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let Some(s) = self.sample.clone() else { return };
        let p = &self.p;
        let sr = self.sr;
        let mode = match p[smp::FTYPE].round() as i32 {
            1 => FilterMode::High,
            2 => FilterMode::Band,
            _ => FilterMode::Low,
        };
        let filter_on = !(mode == FilterMode::Low && p[smp::CUTOFF] >= 19500.0 && p[smp::FENV].abs() < 0.01);
        let q = res_to_q(p[smp::RES]);
        let vol = p[smp::VOLUME];
        let (pl, pr) = pan_gains(p[smp::PAN]);
        let glide = one_pole_coef(p[smp::GLIDE].max(0.0001) / 3.0, sr) as f64;
        let len = s.left.len();
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in 0..l.len() {
                if let Some(g) = v.gate.as_mut() {
                    if *g == 0 {
                        if !v.one_shot {
                            v.amp.gate_off();
                        }
                        v.gate = None;
                    } else {
                        *g -= 1;
                    }
                }
                v.rate = v.target_rate + glide * (v.rate - v.target_rate);
                if v.pos >= v.end {
                    if v.looping {
                        v.pos = v.start + (v.pos - v.end);
                    } else {
                        v.active = false;
                        break;
                    }
                }
                let i0 = (v.pos as usize).min(len - 2);
                let fr = (v.pos - i0 as f64) as f32;
                let mut a = s.left[i0] + (s.left[i0 + 1] - s.left[i0]) * fr;
                let mut b = s.right[i0] + (s.right[i0 + 1] - s.right[i0]) * fr;
                v.pos += v.rate;
                let env = v.amp.next();
                if filter_on {
                    if i % 16 == 0 {
                        let fc = (p[smp::CUTOFF] * 2f32.powf(p[smp::FENV] * env * 6.0)).clamp(20.0, 20000.0);
                        v.fl.set(fc, q, sr);
                        v.fr.set(fc, q, sr);
                    }
                    a = v.fl.process(a, mode);
                    b = v.fr.process(b, mode);
                }
                // Short fade at the region end avoids clicks on one-shots.
                let tail = ((v.end - v.pos) / v.rate.max(0.01)).min(64.0) as f32 / 64.0;
                let g = env * v.vel * vol * if v.looping { 1.0 } else { tail };
                l[i] += a * g * pl;
                r[i] += b * g * pr;
            }
            if !v.amp.is_active() {
                v.active = false;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fm4

/// mods[target][source]: which operators modulate which. Operators are processed D, C, B, A.
const ALGOS: [([[bool; 4]; 4], [bool; 4]); 8] = {
    const F: bool = false;
    const T: bool = true;
    [
        ([[F, T, F, F], [F, F, T, F], [F, F, F, T], [F, F, F, F]], [T, F, F, F]),
        ([[F, T, F, F], [F, F, T, T], [F, F, F, F], [F, F, F, F]], [T, F, F, F]),
        ([[F, T, F, F], [F, F, F, F], [F, F, F, T], [F, F, F, F]], [T, F, T, F]),
        ([[F, F, T, F], [F, F, T, F], [F, F, F, T], [F, F, F, F]], [T, T, F, F]),
        ([[F, F, F, T], [F, F, F, T], [F, F, F, T], [F, F, F, F]], [T, T, T, F]),
        ([[F, F, F, F], [F, F, F, F], [F, F, F, T], [F, F, F, F]], [T, T, T, F]),
        ([[F, F, F, F], [F, F, F, F], [F, F, F, F], [F, F, F, F]], [T, T, T, T]),
        ([[F, T, T, T], [F, F, F, F], [F, F, F, F], [F, F, F, F]], [T, F, F, F]),
    ]
};

#[derive(Default)]
struct OVoice {
    active: bool,
    pitch: u8,
    freq: f32,
    vel: f32,
    gate: Option<u32>,
    held: bool,
    phase: [f32; 4],
    env: [Adsr; 4],
    out: [f32; 4],
    fb: f32,
    fl: Svf,
    age: u64,
    pan: f32,
}

pub struct Fm4 {
    sr: f32,
    p: Vec<f32>,
    voices: Vec<OVoice>,
    age: u64,
}

impl Fm4 {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: params::defaults(&params::FM4_PARAMS), voices: (0..12).map(|_| OVoice::default()).collect(), age: 0 }
    }
    fn apply_env(&mut self) {
        let p = &self.p;
        for v in &mut self.voices {
            for k in 0..4 {
                let o = k * OP_PARAMS;
                v.env[k].set(p[o + 3], p[o + 4], p[o + 5], p[o + 6], self.sr);
            }
        }
    }
}

impl InstrumentDsp for Fm4 {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        self.apply_env();
    }

    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        self.age += 1;
        let idx = pick_voice(&self.voices, |v| v.active, |v| v.env[0].is_released(), |v| v.age, 12);
        let spread = self.p[fmop::SPREAD];
        let v = &mut self.voices[idx];
        v.active = true;
        v.pitch = pitch;
        v.freq = midi_to_freq(pitch as f32);
        v.vel = (vel as f32 / 127.0).powf(1.2);
        v.gate = gate;
        v.held = gate.is_none();
        v.age = self.age;
        v.phase = [0.0; 4];
        v.fb = 0.0;
        v.pan = if self.age % 2 == 0 { spread } else { -spread } * 0.5;
        for e in &mut v.env {
            e.gate_on();
        }
    }

    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch && v.held) {
            v.held = false;
            for e in &mut v.env {
                e.gate_off();
            }
        }
    }

    fn all_off(&mut self) {
        for v in &mut self.voices {
            for e in &mut v.env {
                e.gate_off();
            }
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = &self.p;
        let sr = self.sr;
        let (mods, carriers) = ALGOS[(p[fmop::ALGO].round() as usize).min(7)];
        let ncar = carriers.iter().filter(|c| **c).count().max(1) as f32;
        let fb = p[fmop::FEEDBACK] * 2.5;
        let filter_on = p[fmop::CUTOFF] < 19500.0;
        let q = res_to_q(p[fmop::RES]);
        let vol = p[fmop::VOLUME] * 0.5;
        let mut ratios = [1.0f32; 4];
        let mut levels = [0.0f32; 4];
        for k in 0..4 {
            let o = k * OP_PARAMS;
            ratios[k] = p[o] * 2f32.powf(p[o + 1] / 1200.0);
            levels[k] = p[o + 2];
        }
        for v in self.voices.iter_mut().filter(|v| v.active) {
            if filter_on {
                v.fl.set(p[fmop::CUTOFF], q, sr);
            }
            let (pl, pr) = pan_gains(v.pan);
            for i in 0..l.len() {
                if let Some(g) = v.gate.as_mut() {
                    if *g == 0 {
                        for e in &mut v.env {
                            e.gate_off();
                        }
                        v.gate = None;
                    } else {
                        *g -= 1;
                    }
                }
                let mut sum = 0.0;
                for k in (0..4).rev() {
                    let env = v.env[k].next();
                    let mut m = 0.0;
                    for src in 0..4 {
                        if mods[k][src] {
                            m += v.out[src];
                        }
                    }
                    if k == 3 {
                        m += v.fb * fb;
                    }
                    let y = (v.phase[k] * TAU + m * 4.0).sin() * env * levels[k];
                    v.phase[k] = (v.phase[k] + v.freq * ratios[k] / sr).fract();
                    if k == 3 {
                        v.fb = (v.fb + y) * 0.5;
                    }
                    v.out[k] = y;
                    if carriers[k] {
                        sum += y;
                    }
                }
                let mut y = sum / ncar.sqrt() * v.vel * vol;
                if filter_on {
                    y = v.fl.process(y, FilterMode::Low);
                }
                l[i] += y * pl;
                r[i] += y * pr;
            }
            if !(0..4).any(|k| carriers[k] && v.env[k].is_active()) {
                v.active = false;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Morph

const WT_SIZE: usize = 2048;
const WT_FRAMES: usize = 32;
const WT_MIPS: [usize; 3] = [128, 32, 8];

/// tables[table][mip][frame] -> samples
pub static TABLES: LazyLock<Vec<Vec<Vec<Vec<f32>>>>> = LazyLock::new(|| {
    (0..params::WAVETABLES.len())
        .map(|t| WT_MIPS.iter().map(|&h| (0..WT_FRAMES).map(|f| build_frame(t, f as f32 / (WT_FRAMES - 1) as f32, h)).collect()).collect())
        .collect()
});

fn harmonic_amp(table: usize, pos: f32, k: usize) -> f32 {
    let kf = k as f32;
    match table {
        // Basic shapes: sine -> triangle -> saw -> square
        0 => {
            let sine = if k == 1 { 1.0 } else { 0.0 };
            let tri = if k % 2 == 1 { 1.0 / (kf * kf) * if (k / 2) % 2 == 0 { 1.0 } else { -1.0 } } else { 0.0 };
            let saw = 1.0 / kf;
            let sq = if k % 2 == 1 { 1.0 / kf } else { 0.0 };
            let x = pos * 3.0;
            match x as i32 {
                0 => sine + (tri - sine) * x,
                1 => tri + (saw - tri) * (x - 1.0),
                _ => saw + (sq - saw) * (x - 2.0).min(1.0),
            }
        }
        // Harmonic sweep: a band of harmonics moving up.
        1 => {
            let center = 1.0 + pos * 40.0;
            let w = 2.0 + pos * 6.0;
            (-((kf - center) / w).powi(2)).exp() / kf.sqrt()
        }
        // Formant: vowel-like resonances that move with position.
        2 => {
            let f1 = 3.0 + pos * 6.0;
            let f2 = 8.0 + pos * 20.0;
            (1.0 / kf) * (1.0 + 6.0 * (-((kf - f1) / 1.5).powi(2)).exp() + 4.0 * (-((kf - f2) / 2.5).powi(2)).exp()) * 0.4
        }
        // PWM: pulse width from 50% to 5%.
        3 => {
            let duty = 0.5 - pos * 0.45;
            2.0 / (kf * std::f32::consts::PI) * (kf * std::f32::consts::PI * duty).sin()
        }
        // Digital: pseudo-random spectra that morph.
        _ => {
            let h = |x: u32| ((x.wrapping_mul(2654435761) >> 16) & 0xff) as f32 / 255.0;
            let a = h(k as u32 * 7 + (pos * 8.0) as u32 * 131);
            let b = h(k as u32 * 7 + ((pos * 8.0) as u32 + 1) * 131);
            let fr = (pos * 8.0).fract();
            (a + (b - a) * fr) / kf.powf(0.7)
        }
    }
}

fn build_frame(table: usize, pos: f32, max_h: usize) -> Vec<f32> {
    let mut re = vec![0.0f32; WT_SIZE];
    let mut im = vec![0.0f32; WT_SIZE];
    for k in 1..max_h.min(WT_SIZE / 2) {
        let a = harmonic_amp(table, pos, k);
        // Sine series: x(t) = sum a_k sin(2 pi k t) -> spectrum bins via inverse FFT trick.
        im[k] = -a * 0.5;
        im[WT_SIZE - k] = a * 0.5;
    }
    // Inverse FFT = conj(FFT(conj(X))) / N.
    for v in im.iter_mut() {
        *v = -*v;
    }
    fft(&mut re, &mut im);
    let mut out: Vec<f32> = re.iter().map(|v| *v).collect();
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    for v in &mut out {
        *v /= peak;
    }
    out
}

#[inline]
pub(crate) fn wt_read(table: usize, mip: usize, pos: f32, phase: f32) -> f32 {
    let t = &TABLES[table][mip];
    let fpos = pos.clamp(0.0, 1.0) * (WT_FRAMES - 1) as f32;
    let f0 = fpos as usize;
    let f1 = (f0 + 1).min(WT_FRAMES - 1);
    let ff = fpos - f0 as f32;
    let x = phase * WT_SIZE as f32;
    let i0 = x as usize % WT_SIZE;
    let i1 = (i0 + 1) % WT_SIZE;
    let xf = x - x.floor();
    let a = t[f0][i0] + (t[f0][i1] - t[f0][i0]) * xf;
    let b = t[f1][i0] + (t[f1][i1] - t[f1][i0]) * xf;
    a + (b - a) * ff
}

pub(crate) fn mip_for(freq: f32, sr: f32) -> usize {
    let allowed = sr * 0.5 / freq.max(1.0);
    WT_MIPS.iter().position(|&h| (h as f32) <= allowed).unwrap_or(WT_MIPS.len() - 1)
}

#[derive(Default)]
struct WVoice {
    active: bool,
    pitch: u8,
    freq: f32,
    vel: f32,
    gate: Option<u32>,
    held: bool,
    ph1: [f32; 5],
    ph2: [f32; 5],
    sub: Osc,
    amp: Adsr,
    fl: Svf,
    fr: Svf,
    age: u64,
}

pub struct Morph {
    sr: f32,
    p: Vec<f32>,
    voices: Vec<WVoice>,
    age: u64,
    lfo: f32,
    bpm: f32,
}

impl Morph {
    pub fn new(sr: f32) -> Self {
        LazyLock::force(&TABLES);
        Self { sr, p: params::defaults(&params::WAVETABLE_PARAMS), voices: (0..10).map(|_| WVoice::default()).collect(), age: 0, lfo: 0.0, bpm: 120.0 }
    }
}

impl InstrumentDsp for Morph {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        for v in &mut self.voices {
            v.amp.set(p[wavetable::ATTACK], p[wavetable::DECAY], p[wavetable::SUSTAIN], p[wavetable::RELEASE], self.sr);
        }
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        self.age += 1;
        let idx = pick_voice(&self.voices, |v| v.active, |v| v.amp.is_released(), |v| v.age, 10);
        let v = &mut self.voices[idx];
        v.active = true;
        v.pitch = pitch;
        v.freq = midi_to_freq(pitch as f32);
        v.vel = (vel as f32 / 127.0).powf(1.2);
        v.gate = gate;
        v.held = gate.is_none();
        v.age = self.age;
        for (u, ph) in v.ph1.iter_mut().enumerate() {
            *ph = (u as f32 * 0.37 + pitch as f32 * 0.11).fract();
        }
        for (u, ph) in v.ph2.iter_mut().enumerate() {
            *ph = (u as f32 * 0.53 + 0.2).fract();
        }
        v.amp.gate_on();
    }
    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch && v.held) {
            v.held = false;
            v.amp.gate_off();
        }
    }
    fn all_off(&mut self) {
        for v in &mut self.voices {
            v.amp.gate_off();
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = &self.p;
        let sr = self.sr;
        let t1 = (p[wavetable::T1].round() as usize).min(params::WAVETABLES.len() - 1);
        let t2 = (p[wavetable::T2].round() as usize).min(params::WAVETABLES.len() - 1);
        let n = p[wavetable::UNISON].round().clamp(1.0, 5.0) as usize;
        let spread = p[wavetable::SPREAD];
        let semi2 = 2f32.powf((p[wavetable::SEMI2] + p[wavetable::DET2] / 100.0) / 12.0);
        let mix = p[wavetable::MIX];
        let sub = p[wavetable::SUB];
        let q = res_to_q(p[wavetable::RES]);
        let vol = p[wavetable::VOLUME] * 0.45;
        let lfo_hz = self.bpm / 60.0 / params::division_beats(p[wavetable::LFO_RATE]);
        let lfo_amt = p[wavetable::LFO_POS];
        let env_pos = p[wavetable::ENV_POS];
        let norm = 1.0 / (n as f32).sqrt();
        let mut detunes = [1.0f32; 5];
        let mut pans = [(1.0f32, 1.0f32); 5];
        for u in 0..n {
            let s = if n == 1 { 0.0 } else { u as f32 / (n - 1) as f32 * 2.0 - 1.0 };
            detunes[u] = 2f32.powf(s * spread / 1200.0);
            pans[u] = (1.0 - s * 0.7, 1.0 + s * 0.7);
        }
        let lfo_start = self.lfo;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let mip1 = mip_for(v.freq * 1.05, sr);
            let mip2 = mip_for(v.freq * semi2 * 1.05, sr);
            let mut lfo = lfo_start;
            for i in 0..l.len() {
                if let Some(g) = v.gate.as_mut() {
                    if *g == 0 {
                        v.amp.gate_off();
                        v.gate = None;
                    } else {
                        *g -= 1;
                    }
                }
                lfo = (lfo + lfo_hz / sr).fract();
                let env = v.amp.next();
                let lfo_v = (lfo * TAU).sin() * 0.5 * lfo_amt;
                let pos1 = (p[wavetable::P1] + lfo_v + env_pos * env).clamp(0.0, 1.0);
                let pos2 = (p[wavetable::P2] + lfo_v + env_pos * env).clamp(0.0, 1.0);
                let (mut a, mut b) = (0.0, 0.0);
                for u in 0..n {
                    let f1 = v.freq * detunes[u];
                    let x1 = wt_read(t1, mip1, pos1, v.ph1[u]);
                    v.ph1[u] = (v.ph1[u] + f1 / sr).fract();
                    let mut x = x1 * (1.0 - mix);
                    if mix > 0.0 {
                        let x2 = wt_read(t2, mip2, pos2, v.ph2[u]);
                        v.ph2[u] = (v.ph2[u] + f1 * semi2 / sr).fract();
                        x += x2 * mix;
                    }
                    a += x * pans[u].0;
                    b += x * pans[u].1;
                }
                let s = if sub > 0.0 { v.sub.sine(v.freq * 0.5, sr) * sub } else { 0.0 };
                a = a * norm + s;
                b = b * norm + s;
                if i % 16 == 0 {
                    let fc = (p[wavetable::CUTOFF] * 2f32.powf(p[wavetable::FENV] * env * 5.0)).clamp(20.0, 20000.0);
                    v.fl.set(fc, q, sr);
                    v.fr.set(fc, q, sr);
                }
                let g = env * v.vel * vol;
                l[i] += v.fl.process(a, FilterMode::Low) * g;
                r[i] += v.fr.process(b, FilterMode::Low) * g;
            }
            if !v.amp.is_active() {
                v.active = false;
            }
        }
        self.lfo = (lfo_start + lfo_hz / sr * l.len() as f32).fract();
    }
}

// ---------------------------------------------------------------------------------------------
// StringModel

struct TVoice {
    active: bool,
    pitch: u8,
    freq: f32,
    vel: f32,
    gate: Option<u32>,
    held: bool,
    line: DelayLine,
    lp: OnePole,
    ap_state: f32,
    excite_left: u32,
    excite_lp: OnePole,
    rng: Rng,
    bowing: bool,
    level: f32,
    age: u64,
    vib: f32,
    pick: DelayLine,
}

pub struct StringModel {
    sr: f32,
    p: Vec<f32>,
    voices: Vec<TVoice>,
    body: [Biquad; 2],
    age: u64,
}

impl StringModel {
    pub fn new(sr: f32) -> Self {
        let mk = |i: u32| TVoice {
            active: false,
            pitch: 0,
            freq: 110.0,
            vel: 1.0,
            gate: None,
            held: false,
            line: DelayLine::new((sr / 20.0) as usize),
            lp: OnePole::default(),
            ap_state: 0.0,
            excite_left: 0,
            excite_lp: OnePole::default(),
            rng: Rng::new(17 + i * 101),
            bowing: false,
            level: 0.0,
            age: 0,
            vib: 0.0,
            pick: DelayLine::new((sr / 20.0) as usize),
        };
        let mut body = [Biquad::default(); 2];
        body[0].peaking(220.0, 1.5, 8.0, sr);
        body[1].peaking(900.0, 2.0, 5.0, sr);
        Self { sr, p: params::defaults(&params::STRING_PARAMS), voices: (0..8).map(mk).collect(), body, age: 0 }
    }
}

impl InstrumentDsp for StringModel {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        self.age += 1;
        let idx = pick_voice(&self.voices, |v| v.active, |v| !v.held && v.gate.is_none(), |v| v.age, 8);
        let p = &self.p;
        let sr = self.sr;
        let v = &mut self.voices[idx];
        v.active = true;
        v.pitch = pitch;
        v.freq = midi_to_freq(pitch as f32).max(25.0);
        v.vel = vel as f32 / 127.0;
        v.gate = gate;
        v.held = gate.is_none();
        v.age = self.age;
        let exciter = p[strm::EXCITER].round() as i32;
        v.bowing = exciter == 2;
        v.excite_left = match exciter {
            0 => (sr / v.freq) as u32,
            1 => (sr * 0.002) as u32,
            _ => u32::MAX,
        };
        v.excite_lp.set_lowpass(500.0 + p[strm::BRIGHT] * 12000.0 * (0.5 + v.vel * 0.5), sr);
        v.lp.set_lowpass(1000.0 + (1.0 - p[strm::DAMPING]) * 14000.0, sr);
        v.level = 1.0;
    }
    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch && v.held) {
            v.held = false;
            v.excite_left = 0;
            v.bowing = false;
        }
    }
    fn all_off(&mut self) {
        for v in &mut self.voices {
            v.held = false;
            v.gate = Some(0);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = &self.p;
        let sr = self.sr;
        let t60 = 0.3 + p[strm::DECAY].powi(2) * 15.0;
        let inharm = p[strm::INHARM] * 0.7;
        let pickup = p[strm::PICKUP] * 0.5;
        let vib_amt = p[strm::VIBRATO] * 0.006;
        let vol = p[strm::VOLUME] * 0.6;
        let body_mix = p[strm::BODY];
        let release_t60 = 0.08;
        let mut mono = [0.0f32; super::dsp::MAX_BLOCK];
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let period = sr / v.freq;
            for (i, out) in mono.iter_mut().enumerate().take(l.len()) {
                if let Some(g) = v.gate.as_mut() {
                    if *g == 0 {
                        v.gate = None;
                        v.held = false;
                        v.bowing = false;
                        v.excite_left = 0;
                    } else {
                        *g -= 1;
                    }
                }
                let damping_t60 = if v.held || v.gate.is_some() { t60 } else { release_t60 };
                let g = (-6.9 / (damping_t60 * v.freq)).exp();
                v.vib += 5.5 / sr;
                let per = period * (1.0 + (v.vib * TAU).sin() * vib_amt);
                let y = v.line.read(per.max(2.0));
                // One-pole all-pass for stiffness (inharmonicity).
                let ap = -inharm * y + v.ap_state;
                v.ap_state = y + inharm * ap;
                let fb = v.lp.lp(ap) * g;
                let mut ex = 0.0;
                if v.excite_left > 0 {
                    ex = v.excite_lp.lp(v.rng.next()) * v.vel * if v.bowing { 0.08 } else { 1.0 };
                    if v.excite_left != u32::MAX {
                        v.excite_left -= 1;
                    }
                }
                v.line.write(fb + ex);
                v.pick.write(y);
                let o = y - v.pick.read((per * pickup).max(1.0)) * 0.8;
                *out += o;
                v.level = v.level * 0.999 + o.abs() * 0.001;
                let _ = i;
            }
            if !v.held && v.gate.is_none() && v.excite_left == 0 && v.level < 0.0002 {
                v.active = false;
                v.level = 0.0;
            }
        }
        for i in 0..l.len() {
            let x = mono[i];
            let b0 = self.body[0].process(x);
            let b = self.body[1].process(b0);
            let y = soft_clip((x * (1.0 - body_mix) + b * body_mix) * vol * 0.7);
            l[i] += y;
            r[i] += y;
        }
    }
}
