//! Voltage (analog-style poly synth), Matrix FM (6 operators, free modulation matrix) and
//! Grain Cloud (granular pad synth).

use super::dsp::*;
use super::instruments::InstrumentDsp;
use super::samples::{self, Sample};
use crate::params::{self, grain, mfm, mfm_mod, mfm_param, volt, MFM_OPS};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

fn pick(active: &[bool], released: &[bool], age: &[u64], limit: usize) -> usize {
    let n = active.len().min(limit.max(1));
    if let Some(i) = (0..n).find(|&i| !active[i]) {
        return i;
    }
    if let Some(i) = (0..n).filter(|&i| released[i]).min_by_key(|&i| age[i]) {
        return i;
    }
    (0..n).min_by_key(|&i| age[i]).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Voltage

#[derive(Default, Clone)]
struct VVoice {
    active: bool,
    pitch: u8,
    freq: f32,
    target: f32,
    vel: f32,
    oscs: [[Osc; 2]; 4],
    oscs2: [[Osc; 2]; 4],
    sub: Osc,
    f1: [Svf; 2],
    f2: [Svf; 2],
    amp: Adsr,
    fenv: Adsr,
    gate_left: Option<u32>,
    drift: [f32; 4],
    age: u64,
}

pub struct Voltage {
    sr: f32,
    p: Vec<f32>,
    voices: Vec<VVoice>,
    age: u64,
    lfo_ph: f32,
    lfo_sh: f32,
    rng: Rng,
    counter: u32,
}

impl Voltage {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: params::defaults(&params::VOLTAGE_PARAMS), voices: vec![VVoice::default(); 8], age: 0, lfo_ph: 0.0, lfo_sh: 0.0, rng: Rng::new(99), counter: 0 }
    }
}

fn wave_of(i: f32) -> u8 {
    i.round() as u8
}

/// Band-limited oscillator with a variable-width pulse built from two saws.
fn osc_tick(o: &mut [Osc; 2], wave: u8, f: f32, pw: f32, sr: f32) -> f32 {
    match wave {
        1 => {
            let ph = o[0].phase;
            let a = o[0].next(Wave::Saw, f, sr);
            o[1].phase = (ph + pw).fract();
            let b = o[1].next(Wave::Saw, f, sr);
            a - b
        }
        2 => o[0].next(Wave::Triangle, f, sr),
        3 => o[0].next(Wave::Sine, f, sr),
        _ => o[0].next(Wave::Saw, f, sr),
    }
}

impl InstrumentDsp for Voltage {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        for v in &mut self.voices {
            v.amp.set(p[volt::AA], p[volt::AD], p[volt::AS], p[volt::AR], self.sr);
            v.fenv.set(p[volt::FA], p[volt::FD], p[volt::FS], p[volt::FR], self.sr);
        }
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, slide: bool) {
        let p = &self.p;
        let limit = p[volt::VOICES].round() as usize;
        let mono = limit <= 1;
        let f = midi_to_freq(pitch as f32);
        self.age += 1;
        let active: Vec<bool> = self.voices.iter().map(|v| v.active).collect();
        let released: Vec<bool> = self.voices.iter().map(|v| v.amp.is_released()).collect();
        let ages: Vec<u64> = self.voices.iter().map(|v| v.age).collect();
        let i = if mono { 0 } else { pick(&active, &released, &ages, limit) };
        let glide = p[volt::GLIDE] > 0.0 || slide;
        let v = &mut self.voices[i];
        let legato = mono && v.active && !v.amp.is_released() && glide;
        if !(legato || (glide && v.active)) {
            v.freq = f;
        }
        v.target = f;
        v.pitch = pitch;
        v.vel = vel as f32 / 127.0;
        v.age = self.age;
        v.gate_left = gate;
        if !legato {
            v.amp.gate_on();
            v.fenv.gate_on();
        }
        if !v.active {
            for k in 0..4 {
                v.drift[k] = self.rng.next();
                v.oscs[k][0].phase = (self.rng.next() * 0.5 + 0.5).fract();
            }
        }
        v.active = true;
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
        let n = l.len();
        // Block-rate LFO.
        let lrate = p[volt::LRATE];
        let prev = self.lfo_ph;
        self.lfo_ph = (self.lfo_ph + lrate * n as f32 / sr).fract();
        if self.lfo_ph < prev {
            self.lfo_sh = self.rng.next();
        }
        let ph = self.lfo_ph;
        let lfo = match p[volt::LSHAPE].round() as i32 {
            1 => 1.0 - 4.0 * (ph - 0.5).abs(),
            2 => if ph < 0.5 { 1.0 } else { -1.0 },
            3 => ph * 2.0 - 1.0,
            4 => self.lfo_sh,
            _ => (ph * TAU).sin(),
        };
        let unison = (p[volt::UNISON].round() as usize).clamp(1, 4);
        let spread = p[volt::SPREAD];
        let glide_c = one_pole_coef(p[volt::GLIDE].max(0.0005) * 0.3, sr);
        let (w1, w2) = (wave_of(p[volt::W1]), wave_of(p[volt::W2]));
        let oct1 = 2f32.powf(p[volt::OCT1].round());
        let ratio2 = 2f32.powf((p[volt::SEMI2] + p[volt::DET2] / 100.0) / 12.0) * oct1;
        let pw = (p[volt::PW] + lfo * p[volt::L_PW] * 0.4).clamp(0.05, 0.95);
        let mix = p[volt::MIX];
        let ftype = p[volt::FTYPE].round() as i32;
        let q = res_to_q(p[volt::RES]);
        let drive = 1.0 + p[volt::DRIVE] * 6.0;
        let vol = p[volt::VOLUME] * 0.8 / (unison as f32).sqrt();
        let pitch_lfo = 2f32.powf(lfo * p[volt::L_PITCH] * 0.5 / 12.0 * 2.0);
        self.counter = self.counter.wrapping_add(1);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in 0..n {
                if let Some(g) = v.gate_left.as_mut() {
                    if *g == 0 {
                        v.amp.gate_off();
                        v.fenv.gate_off();
                        v.gate_left = None;
                    } else {
                        *g -= 1;
                    }
                }
                v.freq = v.target + glide_c * (v.freq - v.target);
                let fenv = v.fenv.next();
                if i % 8 == 0 {
                    let key = (v.freq / 261.6).powf(p[volt::KEYTRK]);
                    let cut = (p[volt::CUTOFF] * key * 2f32.powf(p[volt::FENV] * fenv * 6.0 + lfo * p[volt::L_CUT] * 2.0)).clamp(20.0, sr * 0.45);
                    for c in 0..2 {
                        v.f1[c].set(cut, if ftype == 0 { q * 0.7 } else { q }, sr);
                        v.f2[c].set(cut, 0.6, sr);
                    }
                }
                let base = v.freq * pitch_lfo;
                let mut sl = 0.0;
                let mut srr = 0.0;
                for u in 0..unison {
                    let det = if unison > 1 { (u as f32 / (unison - 1) as f32 - 0.5) * spread } else { 0.0 } + v.drift[u] * p[volt::DRIFT] * 4.0;
                    let fu = base * 2f32.powf(det / 1200.0);
                    let a = osc_tick(&mut v.oscs[u], w1, fu * oct1, pw, sr);
                    let b = osc_tick(&mut v.oscs2[u], w2, fu * ratio2, pw, sr);
                    let s = a * (1.0 - mix) + b * mix;
                    let pan = if unison > 1 { u as f32 / (unison - 1) as f32 } else { 0.5 };
                    sl += s * (1.0 - pan * 0.6);
                    srr += s * (0.4 + pan * 0.6);
                }
                let sub = v.sub.next(Wave::Square, base * oct1 * 0.5, sr) * p[volt::SUB];
                let noise = self.rng.next() * p[volt::NOISE];
                let mono_add = sub + noise;
                let mut out = [sl + mono_add, srr + mono_add];
                for (c, x) in out.iter_mut().enumerate() {
                    let y = (*x * drive * 0.5).tanh();
                    *x = match ftype {
                        0 => v.f2[c].process(v.f1[c].process(y, FilterMode::Low), FilterMode::Low),
                        1 => v.f1[c].process(y, FilterMode::Low),
                        2 => v.f1[c].process(y, FilterMode::BandNorm),
                        _ => v.f1[c].process(y, FilterMode::High),
                    };
                }
                let amp = v.amp.next() * (0.4 + 0.6 * v.vel) * vol;
                l[i] += out[0] * amp;
                r[i] += out[1] * amp;
                if !v.amp.is_active() {
                    v.active = false;
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Matrix FM

#[derive(Default, Clone)]
struct FmVoice {
    active: bool,
    pitch: u8,
    freq: f32,
    target: f32,
    vel: f32,
    phase: [f32; MFM_OPS],
    out: [f32; MFM_OPS],
    env: [Adsr; MFM_OPS],
    gate_left: Option<u32>,
    age: u64,
}

pub struct MatrixFm {
    sr: f32,
    p: Vec<f32>,
    voices: Vec<FmVoice>,
    age: u64,
}

impl MatrixFm {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: params::defaults(&params::MATRIX_FM_PARAMS), voices: vec![FmVoice::default(); 8], age: 0 }
    }
}

impl InstrumentDsp for MatrixFm {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        for v in &mut self.voices {
            for k in 0..MFM_OPS {
                v.env[k].set(p[mfm_param(k, mfm::A)], p[mfm_param(k, mfm::D)], p[mfm_param(k, mfm::S)], p[mfm_param(k, mfm::R)], self.sr);
            }
        }
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        let limit = self.p[mfm::VOICES].round() as usize;
        let mono = limit <= 1;
        self.age += 1;
        let active: Vec<bool> = self.voices.iter().map(|v| v.active).collect();
        let released: Vec<bool> = self.voices.iter().map(|v| v.env[0].is_released()).collect();
        let ages: Vec<u64> = self.voices.iter().map(|v| v.age).collect();
        let i = if mono { 0 } else { pick(&active, &released, &ages, limit) };
        let glide = self.p[mfm::GLIDE] > 0.0;
        let v = &mut self.voices[i];
        let f = midi_to_freq(pitch as f32);
        if !(glide && v.active) {
            v.freq = f;
            v.phase = [0.0; MFM_OPS];
            v.out = [0.0; MFM_OPS];
        }
        v.target = f;
        v.pitch = pitch;
        v.vel = vel as f32 / 127.0;
        v.gate_left = gate;
        v.age = self.age;
        v.active = true;
        for e in &mut v.env {
            e.gate_on();
        }
    }
    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch) {
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
        let mut ratio = [0.0f32; MFM_OPS];
        let mut level = [0.0f32; MFM_OPS];
        let mut outl = [0.0f32; MFM_OPS];
        let mut m = [[0.0f32; MFM_OPS]; MFM_OPS];
        for k in 0..MFM_OPS {
            ratio[k] = p[mfm_param(k, mfm::RATIO)] * 2f32.powf(p[mfm_param(k, mfm::FINE)] / 1200.0);
            level[k] = p[mfm_param(k, mfm::LEVEL)];
            outl[k] = p[mfm_param(k, mfm::OUT)];
            for (j, row) in m.iter_mut().enumerate() {
                // Modulation index up to ~4 radians.
                row[k] = p[mfm_mod(j, k)] * p[mfm_mod(j, k)] * 4.0;
            }
        }
        let vol = p[mfm::VOLUME] * 0.4;
        let vel_mod = p[mfm::VEL];
        let glide_c = one_pole_coef(p[mfm::GLIDE].max(0.0005) * 0.3, sr);
        let out_sum: f32 = outl.iter().sum::<f32>().max(1.0);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let vm = 1.0 - vel_mod + vel_mod * v.vel;
            for i in 0..l.len() {
                if let Some(g) = v.gate_left.as_mut() {
                    if *g == 0 {
                        for e in &mut v.env {
                            e.gate_off();
                        }
                        v.gate_left = None;
                    } else {
                        *g -= 1;
                    }
                }
                v.freq = v.target + glide_c * (v.freq - v.target);
                let prev = v.out;
                let mut s = 0.0;
                for k in 0..MFM_OPS {
                    let mut pm = 0.0;
                    for j in 0..MFM_OPS {
                        pm += m[j][k] * prev[j];
                    }
                    let e = v.env[k].next() * level[k];
                    let y = ((v.phase[k] * TAU) + pm * vm).sin() * e;
                    v.out[k] = y;
                    s += y * outl[k];
                    v.phase[k] = (v.phase[k] + v.freq * ratio[k] / sr).fract();
                }
                let y = s / out_sum * vol * (0.3 + 0.7 * v.vel);
                l[i] += y;
                r[i] += y;
                // Voice ends when every audible operator envelope has finished.
                if (0..MFM_OPS).all(|k| outl[k] == 0.0 || !v.env[k].is_active()) {
                    v.active = false;
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Grain Cloud

#[derive(Default, Clone, Copy)]
struct Grain {
    active: bool,
    pos: f64,
    rate: f64,
    len: u32,
    t: u32,
    gl: f32,
    gr: f32,
    amp: f32,
    voice: usize,
}

#[derive(Default, Clone)]
struct GVoice {
    active: bool,
    pitch: u8,
    vel: f32,
    env: Adsr,
    next_grain: f32,
    gate_left: Option<u32>,
    age: u64,
}

pub struct GrainCloud {
    sr: f32,
    p: Vec<f32>,
    builtin: [Arc<Sample>; 4],
    sample: Option<Arc<Sample>>,
    path: Option<String>,
    voices: Vec<GVoice>,
    grains: Vec<Grain>,
    scan: f32,
    rng: Rng,
    age: u64,
    filt: [Svf; 2],
}

/// Renders a few seconds of a built-in source for granulation.
fn build_source(kind: usize, sr: f32) -> Arc<Sample> {
    let n = (sr * 4.0) as usize;
    let mut left = vec![0.0f32; n];
    let mut right = vec![0.0f32; n];
    let mut rng = Rng::new(17 + kind as u32);
    // A minor-ninth chord on C3 (MIDI 48) so that pitch 60 plays it an octave up.
    let notes = [48.0, 55.0, 58.0, 62.0, 63.0];
    let mut f = [Svf::default(), Svf::default()];
    for (i, (a, b)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
        let t = i as f32 / sr;
        let mut x = [0.0f32; 2];
        for (k, nt) in notes.iter().enumerate() {
            let hz = midi_to_freq(*nt);
            for c in 0..2 {
                let det = 1.0 + (c as f32 - 0.5) * 0.004 + (t * (0.1 + k as f32 * 0.07)).sin() * 0.002;
                let ph = t * hz * det;
                x[c] += match kind {
                    0 => (ph * TAU).sin() * 0.6 + (ph * 2.0 * TAU).sin() * 0.25 + (ph * 3.01 * TAU).sin() * 0.12,
                    1 => {
                        // Formant-ish: saw through vowel resonances later.
                        2.0 * ph.fract() - 1.0
                    }
                    2 => (2.0 * ph.fract() - 1.0) * 0.7 + (2.0 * (ph * 1.003).fract() - 1.0) * 0.5,
                    _ => 0.0,
                } * 0.2;
            }
        }
        if kind == 3 {
            x = [rng.next() * 0.5, rng.next() * 0.5];
        }
        let fc = match kind {
            1 => 900.0 + 400.0 * (t * 0.7).sin(),
            3 => 600.0 + 3000.0 * (0.5 + 0.5 * (t * 0.5).sin()),
            _ => 5000.0,
        };
        for c in 0..2 {
            f[c].set(fc, if kind == 1 { 4.0 } else { 0.7 }, sr);
            let mode = if kind == 1 || kind == 3 { FilterMode::BandNorm } else { FilterMode::Low };
            x[c] = f[c].process(x[c], mode);
        }
        *a = x[0];
        *b = x[1];
    }
    // Normalize.
    let peak = left.iter().chain(right.iter()).fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    for v in left.iter_mut().chain(right.iter_mut()) {
        *v *= 0.7 / peak;
    }
    Arc::new(Sample { left, right, sample_rate: sr })
}

impl GrainCloud {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            p: params::defaults(&params::GRAIN_PARAMS),
            builtin: [build_source(0, sr), build_source(1, sr), build_source(2, sr), build_source(3, sr)],
            sample: None,
            path: None,
            voices: vec![GVoice::default(); 8],
            grains: vec![Grain::default(); 96],
            scan: 0.0,
            rng: Rng::new(5),
            age: 0,
            filt: [Svf::default(), Svf::default()],
        }
    }
    fn source(&self) -> Arc<Sample> {
        let s = self.p[grain::SOURCE].round() as usize;
        if s == 0 {
            if let Some(x) = &self.sample {
                return x.clone();
            }
            return self.builtin[0].clone();
        }
        self.builtin[(s - 1).min(3)].clone()
    }
}

impl InstrumentDsp for GrainCloud {
    fn set_params(&mut self, p: &[f32], samples_: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        let path = samples_.first().cloned().flatten();
        if path != self.path || (path.is_some() && self.sample.is_none()) {
            self.sample = path.as_deref().and_then(samples::try_get);
            self.path = path;
        }
        for v in &mut self.voices {
            v.env.set(p[grain::ATTACK], 0.1, 1.0, p[grain::RELEASE], self.sr);
        }
        for f in &mut self.filt {
            f.set(p[grain::CUTOFF], 0.7, self.sr);
        }
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        self.age += 1;
        let active: Vec<bool> = self.voices.iter().map(|v| v.active).collect();
        let released: Vec<bool> = self.voices.iter().map(|v| v.env.is_released()).collect();
        let ages: Vec<u64> = self.voices.iter().map(|v| v.age).collect();
        let i = pick(&active, &released, &ages, 8);
        let v = &mut self.voices[i];
        v.active = true;
        v.pitch = pitch;
        v.vel = vel as f32 / 127.0;
        v.gate_left = gate;
        v.age = self.age;
        v.next_grain = 0.0;
        v.env.gate_on();
    }
    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch) {
            v.env.gate_off();
        }
    }
    fn all_off(&mut self) {
        for v in &mut self.voices {
            v.env.gate_off();
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let src = self.source();
        let len = src.left.len();
        if len < 1024 {
            return;
        }
        let p = self.p.clone();
        let sr = self.sr;
        let n = l.len();
        self.scan = (self.scan + p[grain::SCAN] * 0.05 * n as f32 / sr).rem_euclid(1.0);
        let gsize = (p[grain::SIZE] * sr) as u32;
        let interval = sr / p[grain::DENSITY];
        let base_rate = src.sample_rate as f64 / sr as f64;
        let mut env_now = [0.0f32; 8];
        for (vi, v) in self.voices.iter_mut().enumerate() {
            if !v.active {
                continue;
            }
            if let Some(g) = v.gate_left.as_mut() {
                if *g as usize <= n {
                    v.env.gate_off();
                    v.gate_left = None;
                } else {
                    *g -= n as u32;
                }
            }
            // Envelope at block rate.
            let mut e = 0.0;
            for _ in 0..n {
                e = v.env.next();
            }
            env_now[vi] = e;
            if !v.env.is_active() {
                v.active = false;
                continue;
            }
            v.next_grain -= n as f32;
            while v.next_grain <= 0.0 {
                v.next_grain += interval * (0.7 + 0.6 * (self.rng.next() * 0.5 + 0.5));
                let Some(slot) = self.grains.iter_mut().position(|g| !g.active) else { break };
                let mut cents = self.rng.next() * p[grain::PITCH_SPRAY];
                let roll = self.rng.next() * 0.5 + 0.5;
                if roll < p[grain::OCTAVE_MIX] * 0.5 {
                    cents += 1200.0;
                } else if roll < p[grain::OCTAVE_MIX] {
                    cents -= 1200.0;
                }
                let semis = v.pitch as f32 - 60.0 + cents / 100.0;
                let mut rate = base_rate * 2f64.powf(semis as f64 / 12.0);
                if self.rng.next() * 0.5 + 0.5 < p[grain::REVERSE] {
                    rate = -rate;
                }
                let pos = (p[grain::POS] + self.scan + self.rng.next() * p[grain::SPRAY] * 0.5).rem_euclid(1.0) as f64 * len as f64;
                let pan = self.rng.next() * p[grain::SPREAD];
                self.grains[slot] = Grain {
                    active: true,
                    pos,
                    rate,
                    len: gsize.max(64),
                    t: 0,
                    gl: (1.0 - pan).min(1.0),
                    gr: (1.0 + pan).min(1.0),
                    amp: v.vel * 0.5 + 0.5,
                    voice: vi,
                };
            }
        }
        let vol = p[grain::VOLUME] * 1.4 / (p[grain::DENSITY] * p[grain::SIZE]).max(1.0).sqrt();
        let mut bl = [0.0f32; 64];
        let mut br = [0.0f32; 64];
        for g in self.grains.iter_mut().filter(|g| g.active) {
            for i in 0..n {
                if g.t >= g.len {
                    g.active = false;
                    break;
                }
                let w = (PI * g.t as f32 / g.len as f32).sin();
                let w = w * w;
                let idx = g.pos.rem_euclid(len as f64);
                let i0 = idx as usize;
                let fr = (idx - i0 as f64) as f32;
                let i1 = (i0 + 1) % len;
                let sl = src.left[i0] * (1.0 - fr) + src.left[i1] * fr;
                let sr_ = src.right[i0] * (1.0 - fr) + src.right[i1] * fr;
                let a = w * g.amp * env_now[g.voice];
                bl[i] += sl * a * g.gl;
                br[i] += sr_ * a * g.gr;
                g.pos += g.rate;
                g.t += 1;
            }
        }
        for i in 0..n {
            let a = self.filt[0].process(bl[i], FilterMode::Low) * vol;
            let b = self.filt[1].process(br[i], FilterMode::Low) * vol;
            l[i] += a;
            r[i] += b;
        }
    }
}
