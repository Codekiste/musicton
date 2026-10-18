//! Instruments: synthesized drum machine, 303-style acid bass and a poly synth.

use super::dsp::*;
use super::samples::{self, Sample};
use crate::model::InstrumentKind;
use crate::params::{self, bass, synth};
use std::sync::Arc;

pub trait InstrumentDsp: Send {
    fn set_params(&mut self, p: &[f32], samples: &[Option<String>]);
    /// `gate` is the note length in samples; `None` holds the note until `note_off`.
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, slide: bool);
    fn note_off(&mut self, pitch: u8);
    fn all_off(&mut self);
    /// Adds into `l`/`r`.
    fn render(&mut self, l: &mut [f32], r: &mut [f32]);
    /// True once if a kick was triggered since the last call (drives sidechain devices).
    fn take_kick(&mut self) -> bool {
        false
    }
    fn set_tempo(&mut self, _bpm: f32) {}
    /// Song position (beats) at the start of each block, for tempo-synced internals.
    fn set_position(&mut self, _beat: f64, _playing: bool) {}
    /// Non-parameter instrument data (e.g. the B-303 pattern bank).
    fn set_extra(&mut self, _inst: &crate::model::Instrument) {}
    /// Data for the UI (e.g. the B-303 sequencer position).
    fn display(&self) -> Option<&[f32]> {
        None
    }
    /// Audio tracks: which audio clip plays and where (in beats from the clip start).
    fn audio_sync(&mut self, _clip: Option<(&crate::model::AudioClip, f64)>) {}
}

pub fn create(kind: InstrumentKind, sr: f32) -> Box<dyn InstrumentDsp> {
    match kind {
        InstrumentKind::Drums => Box::new(DrumKit::new(sr)),
        InstrumentKind::Bass => Box::new(MonoBass::new(sr)),
        InstrumentKind::Synth => Box::new(PolySynth::new(sr)),
        InstrumentKind::Sampler => Box::new(super::instruments2::Sampler::new(sr)),
        InstrumentKind::Fm4 => Box::new(super::instruments2::Fm4::new(sr)),
        InstrumentKind::Morph => Box::new(super::instruments2::Morph::new(sr)),
        InstrumentKind::StringModel => Box::new(super::instruments2::StringModel::new(sr)),
        InstrumentKind::Audio => Box::new(super::audioclip::AudioPlayer::new(sr)),
        InstrumentKind::Group => Box::new(Silent),
        InstrumentKind::B303 => Box::new(super::b303::B303::new(sr)),
        InstrumentKind::Nexa => Box::new(super::nexa::Nexa::new(sr)),
    }
}

/// Group tracks have no instrument; their input is the sum of their members.
struct Silent;

impl InstrumentDsp for Silent {
    fn set_params(&mut self, _: &[f32], _: &[Option<String>]) {}
    fn note_on(&mut self, _: u8, _: u8, _: Option<u32>, _: bool) {}
    fn note_off(&mut self, _: u8) {}
    fn all_off(&mut self) {}
    fn render(&mut self, _: &mut [f32], _: &mut [f32]) {}
}

// ---------------------------------------------------------------------------------------------
// Drum machine

const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const MAX_DRUM_VOICES: usize = 32;

#[derive(Clone, Copy, Default)]
struct LaneP {
    tf: f32,
    decay: f32,
    tone: f32,
    level: f32,
    pan: f32,
}

#[derive(Default)]
struct DrumVoice {
    active: bool,
    lane: usize,
    t: u32,
    gain: f32,
    gl: f32,
    gr: f32,
    p: LaneP,
    osc: [Osc; 6],
    f1: Svf,
    f2: Svf,
    rng: Option<Rng>,
    env_a: f32,
    mul_a: f32,
    env_b: f32,
    mul_b: f32,
    env_p: f32,
    mul_p: f32,
    env_p2: f32,
    mul_p2: f32,
    choke: f32,
    choke_mul: f32,
    sample: Option<Arc<Sample>>,
    spos: f64,
    srate: f64,
    send: f64,
}

pub struct DrumKit {
    sr: f32,
    voices: Vec<DrumVoice>,
    lanes: [LaneP; 10],
    samples: Vec<Option<Arc<Sample>>>,
    sample_paths: Vec<Option<String>>,
    drive: f32,
    volume: f32,
    kicked: bool,
    seed: u32,
}

impl DrumKit {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            voices: (0..MAX_DRUM_VOICES).map(|_| DrumVoice::default()).collect(),
            lanes: [LaneP::default(); 10],
            samples: vec![None; 10],
            sample_paths: vec![None; 10],
            drive: 0.0,
            volume: 1.0,
            kicked: false,
            seed: 12345,
        }
    }

    fn trigger(&mut self, lane: usize, vel: u8) {
        if lane >= 10 {
            return;
        }
        let sr = self.sr;
        let p = self.lanes[lane];
        if lane == 4 {
            // Closed hat chokes the open hat.
            for v in self.voices.iter_mut().filter(|v| v.active && v.lane == 5) {
                v.choke_mul = decay_mult(0.02, sr);
            }
        }
        if lane == 0 {
            self.kicked = true;
        }
        let idx = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| (0..self.voices.len()).max_by_key(|&i| self.voices[i].t).unwrap());
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let (gl, gr) = pan_gains(p.pan);
        let v = &mut self.voices[idx];
        *v = DrumVoice {
            active: true,
            lane,
            gain: p.level * (vel as f32 / 127.0).powf(1.3) * 1.2,
            gl,
            gr,
            p,
            rng: Some(Rng::new(self.seed)),
            env_a: 1.0,
            env_b: 1.0,
            env_p: 1.0,
            env_p2: 1.0,
            choke: 1.0,
            choke_mul: 1.0,
            ..Default::default()
        };
        let d = p.decay;
        let tone = p.tone;
        let tf = p.tf;
        if let Some(s) = self.samples[lane].clone() {
            v.srate = (s.sample_rate / sr) as f64 * tf as f64;
            v.send = s.left.len() as f64 * d.min(1.0) as f64;
            v.sample = Some(s);
            return;
        }
        match lane {
            0 => {
                v.mul_a = decay_mult(0.45 * d, sr);
                v.mul_b = one_pole_coef(0.003, sr);
                v.mul_p = one_pole_coef(0.012, sr);
                v.mul_p2 = one_pole_coef(0.05, sr);
                v.f1.set(1800.0, 0.7, sr);
            }
            1 => {
                v.f1.set(1000.0 * tf + tone * 800.0, 1.4, sr);
                v.env_a = 0.85;
                v.mul_a = decay_mult(0.22 * d, sr);
            }
            2 => {
                v.mul_a = decay_mult(0.11 * d, sr);
                v.mul_b = decay_mult(0.2 * d, sr);
                v.mul_p = one_pole_coef(0.01, sr);
                v.f1.set(1200.0 + tone * 2500.0, 0.7, sr);
            }
            3 => {
                v.mul_a = decay_mult(0.035 * d, sr);
                v.f1.set(1700.0 * tf, 3.0 + tone * 6.0, sr);
            }
            4..=6 => {
                let ride = lane == 6;
                v.f1.set(if ride { 7000.0 } else { 10000.0 } * (0.8 + tone * 0.4), if ride { 1.5 } else { 0.8 }, sr);
                v.f2.set(if ride { 4500.0 } else { 6500.0 } + tone * 2000.0, 0.7, sr);
                let t = match lane {
                    4 => 0.06,
                    5 => 0.45,
                    _ => 1.6,
                };
                v.mul_a = decay_mult(t * d, sr);
                for (i, o) in v.osc.iter_mut().enumerate() {
                    o.phase = (i as f32 * 0.37) % 1.0;
                }
            }
            7 | 8 => {
                v.mul_a = decay_mult(0.35 * d, sr);
                v.mul_b = decay_mult(0.03, sr);
                v.mul_p = one_pole_coef(0.03, sr);
                v.f1.set(3000.0, 0.7, sr);
            }
            _ => {
                v.mul_a = decay_mult(0.16 * d, sr);
                v.f1.set(800.0 * tf, 1.0 + tone * 4.0, sr);
            }
        }
    }
}

impl DrumVoice {
    #[inline]
    fn next(&mut self, sr: f32) -> f32 {
        let p = self.p;
        let tf = p.tf;
        let ts = self.t as f32 / sr;
        self.t += 1;
        if let Some(s) = &self.sample {
            let i = self.spos as usize;
            if i + 1 >= s.left.len() || self.spos >= self.send {
                self.active = false;
                return 0.0;
            }
            let frac = (self.spos - i as f64) as f32;
            let x = (s.left[i] + (s.left[i + 1] - s.left[i]) * frac + s.right[i] + (s.right[i + 1] - s.right[i]) * frac) * 0.5;
            self.spos += self.srate;
            let remain = ((self.send - self.spos) / self.srate) as f32 / sr;
            let fade = (remain / 0.03).min(1.0);
            return x * fade * self.choke_step();
        }
        let rng = self.rng.as_mut().unwrap();
        let out = match self.lane {
            0 => {
                self.env_p *= self.mul_p;
                self.env_p2 *= self.mul_p2;
                let f0 = 48.0 * tf;
                let f = f0 * (1.0 + 5.0 * self.env_p + 0.8 * self.env_p2);
                let body = soft_clip(self.osc[0].sine(f, sr) * (1.0 + p.tone * 2.5));
                let att = (ts / 0.0015).min(1.0);
                self.env_a *= self.mul_a;
                self.env_b *= self.mul_b;
                let click = self.f1.process(rng.next(), FilterMode::High) * self.env_b * (0.25 + p.tone * 0.5);
                body * self.env_a * att + click
            }
            1 => {
                let n = self.f1.process(rng.next(), FilterMode::Band);
                let g = if ts < 0.031 {
                    (-(ts % 0.011) / 0.003).exp()
                } else {
                    self.env_a *= self.mul_a;
                    self.env_a
                };
                n * g * 2.2
            }
            2 => {
                self.env_p *= self.mul_p;
                let f = 185.0 * tf * (1.0 + 0.2 * self.env_p);
                let tone = self.osc[0].sine(f, sr) * 0.7 + self.osc[1].sine(f * 1.78, sr) * 0.35;
                self.env_a *= self.mul_a;
                self.env_b *= self.mul_b;
                let n = self.f1.process(rng.next(), FilterMode::High);
                tone * self.env_a * (1.2 - p.tone) * 0.8 + n * self.env_b * (0.4 + p.tone * 0.6)
            }
            3 => {
                let x = self.osc[0].sine(1700.0 * tf, sr) + self.osc[1].sine(470.0 * tf, sr);
                self.env_a *= self.mul_a;
                self.f1.process(x, FilterMode::Band) * self.env_a * 2.0
            }
            4..=6 => {
                let ride = self.lane == 6;
                let mult = tf * if ride { 1.45 } else { 1.0 };
                let mut m = 0.0;
                for (o, f) in self.osc.iter_mut().zip(METAL) {
                    m += o.naive_square(f * mult, sr);
                }
                let x = m * 0.25 + rng.next() * if ride { 0.15 } else { 0.35 };
                let y = self.f2.process(self.f1.process(x, FilterMode::Band), FilterMode::High);
                let att = (ts / 0.0005).min(1.0);
                self.env_a *= self.mul_a;
                y * self.env_a * att * if ride { 1.0 } else { 1.4 }
            }
            7 | 8 => {
                self.env_p *= self.mul_p;
                let f = if self.lane == 7 { 90.0 } else { 145.0 } * tf * (1.0 + 0.7 * self.env_p);
                self.env_a *= self.mul_a;
                self.env_b *= self.mul_b;
                let n = self.f1.process(rng.next(), FilterMode::Low) * self.env_b * 0.15 * p.tone;
                self.osc[0].sine(f, sr) * self.env_a * 0.9 + n
            }
            _ => {
                let x = self.osc[0].naive_square(540.0 * tf, sr) + self.osc[1].naive_square(800.0 * tf, sr);
                self.env_a *= self.mul_a;
                self.f1.process(x, FilterMode::Band) * self.env_a * 0.9
            }
        };
        if self.env_a < 0.0003 && ts > 0.04 {
            self.active = false;
        }
        out * self.choke_step()
    }

    #[inline]
    fn choke_step(&mut self) -> f32 {
        if self.choke_mul < 1.0 {
            self.choke *= self.choke_mul;
            if self.choke < 0.0005 {
                self.active = false;
            }
        }
        self.choke
    }
}

impl InstrumentDsp for DrumKit {
    fn set_params(&mut self, p: &[f32], samples: &[Option<String>]) {
        self.drive = p[0];
        self.volume = p[1];
        for lane in 0..10 {
            let g = |i| p[params::lane_param(lane, i)];
            self.lanes[lane] = LaneP {
                tf: 2f32.powf(g(params::LANE_TUNE) / 12.0),
                decay: g(params::LANE_DECAY),
                tone: g(params::LANE_TONE),
                level: g(params::LANE_LEVEL),
                pan: g(params::LANE_PAN),
            };
            let path = samples.get(lane).cloned().flatten();
            if path != self.sample_paths[lane] || (path.is_some() && self.samples[lane].is_none()) {
                self.samples[lane] = path.as_deref().and_then(samples::try_get);
                self.sample_paths[lane] = path;
            }
        }
    }

    fn note_on(&mut self, pitch: u8, vel: u8, _gate: Option<u32>, _slide: bool) {
        self.trigger(pitch as usize, vel);
    }

    fn note_off(&mut self, _pitch: u8) {}

    fn all_off(&mut self) {
        for v in &mut self.voices {
            v.choke_mul = decay_mult(0.01, self.sr);
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let sr = self.sr;
        let drive = self.drive;
        let comp = 1.0 / (1.0 + drive * 1.5);
        for i in 0..l.len() {
            let mut sl = 0.0;
            let mut sr_ = 0.0;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                let x = v.next(sr) * v.gain;
                sl += x * v.gl;
                sr_ += x * v.gr;
            }
            if drive > 0.001 {
                sl = soft_clip(sl * (1.0 + drive * 5.0)) * comp;
                sr_ = soft_clip(sr_ * (1.0 + drive * 5.0)) * comp;
            }
            l[i] += sl * self.volume;
            r[i] += sr_ * self.volume;
        }
    }

    fn take_kick(&mut self) -> bool {
        std::mem::take(&mut self.kicked)
    }
}

// ---------------------------------------------------------------------------------------------
// Acid bass

pub struct MonoBass {
    sr: f32,
    p: Vec<f32>,
    osc: Osc,
    sub: Osc,
    f1: Svf,
    f2: Svf,
    freq: f32,
    target: f32,
    gliding: bool,
    amp: f32,
    amp_target: f32,
    fenv: f32,
    fenv_mul: f32,
    accent: f32,
    gate: Option<u32>,
    gate_open: bool,
    prev_slide: bool,
    live_pitch: Option<u8>,
    counter: u32,
}

impl MonoBass {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            p: params::defaults(&params::BASS_PARAMS),
            osc: Osc::default(),
            sub: Osc::default(),
            f1: Svf::default(),
            f2: Svf::default(),
            freq: 55.0,
            target: 55.0,
            gliding: false,
            amp: 0.0,
            amp_target: 0.0,
            fenv: 0.0,
            fenv_mul: 0.999,
            accent: 0.0,
            gate: None,
            gate_open: false,
            prev_slide: false,
            live_pitch: None,
            counter: 0,
        }
    }
}

impl InstrumentDsp for MonoBass {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
    }

    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, slide: bool) {
        let p = &self.p;
        let legato = self.gate_open && self.prev_slide;
        self.target = midi_to_freq(pitch as f32 + 12.0 * p[bass::OCTAVE]);
        if legato {
            self.gliding = true;
        } else {
            self.gliding = false;
            self.freq = self.target;
            self.accent = if vel >= 110 { p[bass::ACCENT] } else { 0.0 };
            self.fenv = 1.0;
            let dec = p[bass::DECAY] * if self.accent > 0.0 { 0.5 } else { 1.0 };
            self.fenv_mul = decay_mult(dec * 2.0, self.sr);
        }
        self.amp_target = (0.5 + vel as f32 / 127.0 * 0.4) * (1.0 + self.accent * 0.6);
        self.gate = gate;
        self.gate_open = true;
        self.prev_slide = slide;
        self.live_pitch = if gate.is_none() { Some(pitch) } else { None };
    }

    fn note_off(&mut self, pitch: u8) {
        if self.live_pitch == Some(pitch) {
            self.gate = Some(0);
            self.live_pitch = None;
        }
    }

    fn all_off(&mut self) {
        self.gate = Some(0);
        self.prev_slide = false;
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let sr = self.sr;
        let p = &self.p;
        let wave = Wave::from_index(p[bass::WAVE]);
        let q = 0.6 + p[bass::RES].powf(1.5) * 22.0;
        let glide = one_pole_coef(p[bass::GLIDE] / 3.0, sr);
        let a_att = one_pole_coef(0.0015, sr);
        let a_rel = one_pole_coef(0.006, sr);
        let drive = p[bass::DRIVE];
        let vol = p[bass::VOLUME] * 0.8;
        let sub = p[bass::SUB];
        let env_range = p[bass::ENV_MOD] * 5000.0 * (1.0 + self.accent * 1.5);
        let cutoff = p[bass::CUTOFF];
        for i in 0..l.len() {
            if let Some(g) = self.gate.as_mut() {
                if *g == 0 {
                    self.gate_open = false;
                    self.amp_target = 0.0;
                    self.gate = None;
                } else {
                    *g -= 1;
                }
            }
            if self.gliding {
                self.freq = self.target + glide * (self.freq - self.target);
            }
            let c = if self.amp_target > self.amp { a_att } else { a_rel };
            self.amp = self.amp_target + c * (self.amp - self.amp_target);
            self.fenv *= self.fenv_mul;
            if self.counter % 4 == 0 {
                let fc = (cutoff + env_range * self.fenv).min(18000.0);
                self.f1.set(fc, q, sr);
                self.f2.set(fc, 0.6, sr);
            }
            self.counter = self.counter.wrapping_add(1);
            if self.amp < 0.00005 && !self.gate_open {
                continue;
            }
            let s = self.osc.next(wave, self.freq, sr) + sub * 0.6 * self.sub.next(Wave::Square, self.freq * 0.5, sr);
            let mut y = self.f2.process(self.f1.process(s, FilterMode::Low), FilterMode::Low);
            y *= self.amp;
            y = soft_clip(y * (1.0 + drive * 6.0)) / (1.0 + drive * 2.0);
            l[i] += y * vol;
            r[i] += y * vol;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Poly synth

const MAX_POLY: usize = 16;

#[derive(Default)]
struct PolyVoice {
    active: bool,
    pitch: u8,
    freq: f32,
    vel: f32,
    gate: Option<u32>,
    oscs: [Osc; 5],
    sub: Osc,
    fl: Svf,
    fr: Svf,
    amp: Adsr,
    fenv: Adsr,
    age: u64,
}

pub struct PolySynth {
    sr: f32,
    p: Vec<f32>,
    voices: Vec<PolyVoice>,
    age: u64,
    counter: u32,
}

impl PolySynth {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            p: params::defaults(&params::SYNTH_PARAMS),
            voices: (0..MAX_POLY).map(|_| PolyVoice::default()).collect(),
            age: 0,
            counter: 0,
        }
    }
}

impl InstrumentDsp for PolySynth {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        let (a, d, s, r) = (p[synth::ATTACK], p[synth::DECAY], p[synth::SUSTAIN], p[synth::RELEASE]);
        for v in &mut self.voices {
            v.amp.set(a, d, s, r, self.sr);
            v.fenv.set(a, d, s, r, self.sr);
        }
    }

    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        self.age += 1;
        let idx = self
            .voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| self.voices.iter().enumerate().filter(|(_, v)| v.amp.is_released()).min_by_key(|(_, v)| v.age).map(|(i, _)| i))
            .unwrap_or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap());
        let p = &self.p;
        let sr = self.sr;
        let v = &mut self.voices[idx];
        v.active = true;
        v.pitch = pitch;
        v.freq = midi_to_freq(pitch as f32);
        v.vel = (vel as f32 / 127.0).powf(1.2);
        v.gate = gate;
        v.age = self.age;
        for (i, o) in v.oscs.iter_mut().enumerate() {
            o.phase = ((pitch as u32 * 7 + i as u32 * 13) % 17) as f32 / 17.0;
        }
        v.amp.set(p[synth::ATTACK], p[synth::DECAY], p[synth::SUSTAIN], p[synth::RELEASE], sr);
        v.fenv.set(p[synth::ATTACK], p[synth::DECAY], p[synth::SUSTAIN], p[synth::RELEASE], sr);
        v.amp.gate_on();
        v.fenv.gate_on();
    }

    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.pitch == pitch && v.gate.is_none()) {
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
        let sr = self.sr;
        let p = &self.p;
        let wave = Wave::from_index(p[synth::WAVE]);
        let n = p[synth::VOICES].round().clamp(1.0, 5.0) as usize;
        let detune = p[synth::DETUNE];
        let sub = p[synth::SUB];
        let q = 0.5 + p[synth::RES].powf(1.5) * 18.0;
        let cutoff = p[synth::CUTOFF];
        let env_amt = p[synth::ENV_AMT] * 9000.0;
        let vol = p[synth::VOLUME] * 0.5;
        let norm = 0.6 / (n as f32).sqrt();
        // Detune factors and stereo positions per unison voice.
        let mut ratios = [1.0f32; 5];
        let mut pans = [(1.0f32, 1.0f32); 5];
        for u in 0..n {
            let spread = if n == 1 { 0.0 } else { u as f32 / (n - 1) as f32 * 2.0 - 1.0 };
            ratios[u] = 2f32.powf(spread * detune / 1200.0);
            pans[u] = (1.0 - spread * 0.6, 1.0 + spread * 0.6);
        }
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in 0..l.len() {
                if let Some(g) = v.gate.as_mut() {
                    if *g == 0 {
                        v.amp.gate_off();
                        v.fenv.gate_off();
                        v.gate = None;
                    } else {
                        *g -= 1;
                    }
                }
                let a = v.amp.next();
                let fe = v.fenv.next();
                if i % 8 == 0 {
                    let fc = (cutoff + env_amt * fe).min(18000.0);
                    v.fl.set(fc, q, sr);
                    v.fr.set(fc, q, sr);
                }
                let mut sl = 0.0;
                let mut sr_ = 0.0;
                for u in 0..n {
                    let x = v.oscs[u].next(wave, v.freq * ratios[u], sr);
                    sl += x * pans[u].0;
                    sr_ += x * pans[u].1;
                }
                let s = if sub > 0.0 { v.sub.sine(v.freq * 0.5, sr) * sub } else { 0.0 };
                sl = sl * norm + s;
                sr_ = sr_ * norm + s;
                let g = a * v.vel * vol;
                l[i] += v.fl.process(sl, FilterMode::Low) * g;
                r[i] += v.fr.process(sr_, FilterMode::Low) * g;
            }
            if !v.amp.is_active() {
                v.active = false;
            }
        }
        self.counter = self.counter.wrapping_add(l.len() as u32);
    }
}
