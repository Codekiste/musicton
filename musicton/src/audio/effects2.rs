//! The second batch of audio effects.

use super::dsp::*;
use super::effects::{mix_gains, Ctx, Effect};
use crate::params::{division_beats, named_beats, BEAT_GRIDS, BEAT_INTERVALS, EQ8_BAND_PARAMS};
use std::f32::consts::{PI, TAU};

fn hash01(x: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

fn lfo_shape(shape: i32, p: f32, cycle: u32) -> f32 {
    match shape {
        1 => 1.0 - 4.0 * (p - 0.5).abs(),
        2 => if p < 0.5 { 1.0 } else { -1.0 },
        3 => 2.0 * p - 1.0,
        4 => hash01(cycle) * 2.0 - 1.0,
        _ => (p * TAU).sin(),
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct AutoPan {
    amount: f32,
    rate: f32,
    shape: i32,
    phase: f32,
    offset: f32,
    ph: f32,
    cycle: u32,
}

impl Effect for AutoPan {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.amount = p[0];
        self.rate = p[1];
        self.shape = p[2].round() as i32;
        self.phase = p[3] / 360.0;
        self.offset = p[4] / 360.0;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let hz = ctx.bpm / 60.0 / division_beats(self.rate);
        for i in 0..l.len() {
            self.ph += hz / ctx.sr;
            if self.ph >= 1.0 {
                self.ph -= 1.0;
                self.cycle = self.cycle.wrapping_add(1);
            }
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let raw = self.ph + self.offset + ch as f32 * self.phase;
                let p = raw.fract();
                let lfo = lfo_shape(self.shape, p, self.cycle.wrapping_add(raw.floor() as u32).wrapping_mul(2).wrapping_add(ch as u32));
                buf[i] *= 1.0 - self.amount * (0.5 - 0.5 * lfo);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct BeatRepeat {
    buf: [Vec<f32>; 2],
    w: usize,
    interval: f32,
    offset: f32,
    grid: f32,
    chance: f32,
    gate: f32,
    decay: f32,
    rate: f32,
    mode: i32,
    vol: f32,
    active: bool,
    start: usize,
    elapsed: f64,
    grid_len: f64,
    gate_len: f64,
    level: f32,
    last_idx: i64,
    rng: Rng,
}

impl BeatRepeat {
    pub fn new(sr: f32) -> Self {
        let n = (sr * 10.0) as usize;
        Self {
            buf: [vec![0.0; n], vec![0.0; n]],
            w: 0,
            interval: 4.0,
            offset: 2.0,
            grid: 0.25,
            chance: 1.0,
            gate: 2.0,
            decay: 0.0,
            rate: 1.0,
            mode: 1,
            vol: 1.0,
            active: false,
            start: 0,
            elapsed: 0.0,
            grid_len: 1.0,
            gate_len: 1.0,
            level: 1.0,
            last_idx: i64::MIN,
            rng: Rng::new(777),
        }
    }
}

impl Effect for BeatRepeat {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.interval = named_beats(BEAT_INTERVALS[(p[0].round() as usize).min(BEAT_INTERVALS.len() - 1)]);
        self.offset = p[1] / 4.0;
        self.grid = named_beats(BEAT_GRIDS[(p[2].round() as usize).min(BEAT_GRIDS.len() - 1)]);
        self.chance = p[3];
        self.gate = p[4] / 4.0;
        self.decay = p[5];
        self.rate = 2f32.powf(p[6] / 12.0);
        self.mode = p[7].round() as i32;
        self.vol = db_to_gain(p[8]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let len = self.buf[0].len();
        let beats_per_sample = ctx.bpm as f64 / 60.0 / ctx.sr as f64;
        let spb = 60.0 / ctx.bpm as f64 * ctx.sr as f64;
        for i in 0..l.len() {
            let input = [l[i], r[i]];
            if ctx.playing {
                let beat = ctx.beat + i as f64 * beats_per_sample - self.offset as f64;
                let idx = (beat / self.interval as f64).floor() as i64;
                if idx != self.last_idx {
                    if self.last_idx != i64::MIN && (self.rng.next() * 0.5 + 0.5) < self.chance {
                        self.active = true;
                        self.elapsed = 0.0;
                        self.start = self.w;
                        self.grid_len = (self.grid as f64 * spb).max(16.0);
                        self.gate_len = self.gate as f64 * spb;
                        self.level = 1.0;
                    }
                    self.last_idx = idx;
                }
            } else {
                self.active = false;
                self.last_idx = i64::MIN;
            }
            self.buf[0][self.w] = input[0];
            self.buf[1][self.w] = input[1];
            self.w = (self.w + 1) % len;
            let mut rep = [0.0f32; 2];
            let mut repeating = false;
            if self.active {
                if self.elapsed >= self.grid_len {
                    repeating = true;
                    let pos = self.elapsed % self.grid_len;
                    if pos < 1.0 {
                        self.level *= 1.0 - self.decay * 0.35;
                    }
                    let off = (pos * self.rate as f64) as usize;
                    let j = (self.start + off) % len;
                    let fade = ((self.grid_len - pos) / 64.0).min(pos / 32.0).min(1.0) as f32;
                    rep = [self.buf[0][j] * self.level * fade, self.buf[1][j] * self.level * fade];
                }
                self.elapsed += 1.0;
                if self.elapsed >= self.gate_len.max(self.grid_len * 2.0) {
                    self.active = false;
                }
            }
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                buf[i] = match self.mode {
                    0 => input[ch] + rep[ch] * self.vol,
                    1 => if repeating { rep[ch] * self.vol } else { input[ch] },
                    _ => if repeating { rep[ch] * self.vol } else { 0.0 },
                };
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Chorus {
    d: [DelayLine; 2],
    rate: f32,
    depth: f32,
    delay: f32,
    fb: f32,
    width: f32,
    dry: f32,
    wet: f32,
    ph: f32,
    last: [f32; 2],
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        Self { d: [DelayLine::new((sr * 0.08) as usize), DelayLine::new((sr * 0.08) as usize)], rate: 0.5, depth: 0.4, delay: 8.0, fb: 0.0, width: 1.0, dry: 0.7, wet: 0.7, ph: 0.0, last: [0.0; 2] }
    }
}

impl Effect for Chorus {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.rate = p[0];
        self.depth = p[1];
        self.delay = p[2];
        self.fb = p[3];
        self.width = p[4];
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        for i in 0..l.len() {
            self.ph = (self.ph + self.rate / ctx.sr) % 1.0;
            let inp = [l[i], r[i]];
            let mut out = [0.0; 2];
            for ch in 0..2 {
                let lfo = ((self.ph + ch as f32 * 0.25 * self.width) * TAU).sin();
                let ms = self.delay + self.depth * 6.0 * (0.5 + 0.5 * lfo);
                let y = self.d[ch].read(ms / 1000.0 * ctx.sr);
                self.d[ch].write(inp[ch] + y * self.fb);
                out[ch] = y;
            }
            let m = (out[0] + out[1]) * 0.5;
            let (ol, or) = (m + (out[0] - m) * self.width, m + (out[1] - m) * self.width);
            self.last = [ol, or];
            l[i] = inp[0] * self.dry + ol * self.wet;
            r[i] = inp[1] * self.dry + or * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------

const CORPUS_RATIOS: [[f32; 8]; 7] = [
    [1.0, 2.756, 5.404, 8.933, 13.34, 18.64, 24.81, 31.87],
    [1.0, 3.99, 10.65, 20.0, 31.9, 46.0, 62.5, 81.0],
    [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
    [1.0, 1.594, 2.136, 2.296, 2.653, 2.918, 3.156, 3.501],
    [1.0, 1.42, 2.0, 2.55, 3.16, 3.67, 4.0, 4.6],
    [1.0, 3.0, 5.0, 7.0, 9.0, 11.0, 13.0, 15.0],
    [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
];

#[derive(Default)]
pub struct Corpus {
    modes: [[Biquad; 8]; 2],
    gains: [f32; 8],
    lp: [OnePole; 2],
    dry: f32,
    wet: f32,
}

impl Effect for Corpus {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        let t = (p[0].round() as usize).min(6);
        let f0 = p[1];
        let q = 20.0 + p[2] * p[2] * 600.0;
        for k in 0..8 {
            let f = f0 * CORPUS_RATIOS[t][k];
            for (ch, modes) in self.modes.iter_mut().enumerate() {
                modes[k].bandpass((f * (1.0 + ch as f32 * 0.003)).min(ctx.sr * 0.45), q, ctx.sr);
            }
            self.gains[k] = if f < ctx.sr * 0.45 { 1.0 / (k as f32 + 1.0).powf(2.0 - p[3] * 2.0) } else { 0.0 };
        }
        for f in &mut self.lp {
            f.set_lowpass(p[4], ctx.sr);
        }
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let inp = self.lp[ch].lp(*x);
                let mut y = 0.0;
                for k in 0..8 {
                    if self.gains[k] > 0.0 {
                        y += self.modes[ch][k].process(inp) * self.gains[k];
                    }
                }
                *x = *x * self.dry + soft_clip(y * 2.0) * self.wet;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct DrumBuss {
    drive: f32,
    crunch: f32,
    damp: [OnePole; 2],
    trans: f32,
    boom: f32,
    boom_f: [Svf; 2],
    boom_in: [OnePole; 2],
    boom_q: f32,
    crunch_hp: [OnePole; 2],
    fast: f32,
    slow: f32,
    dry: f32,
    wet: f32,
    out: f32,
}

impl Effect for DrumBuss {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.drive = p[0];
        self.crunch = p[1];
        for f in &mut self.damp {
            f.set_lowpass(p[2], ctx.sr);
        }
        self.trans = p[3];
        self.boom = p[4];
        self.boom_q = 4.0 + p[6] * 60.0;
        for f in &mut self.boom_f {
            f.set(p[5], self.boom_q, ctx.sr);
        }
        for f in &mut self.boom_in {
            f.set_lowpass(p[5] * 3.0, ctx.sr);
        }
        for f in &mut self.crunch_hp {
            f.set_lowpass(900.0, ctx.sr);
        }
        (self.dry, self.wet) = mix_gains(p[7]);
        self.out = db_to_gain(p[8]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let fa = one_pole_coef(0.001, ctx.sr);
        let fr = one_pole_coef(0.03, ctx.sr);
        let sa = one_pole_coef(0.02, ctx.sr);
        let srl = one_pole_coef(0.25, ctx.sr);
        let pre = 1.0 + self.drive * 5.0;
        let post = 1.0 / (1.0 + self.drive * 1.5);
        for i in 0..l.len() {
            let lvl = l[i].abs().max(r[i].abs());
            self.fast = lvl + if lvl > self.fast { fa } else { fr } * (self.fast - lvl);
            self.slow = lvl + if lvl > self.slow { sa } else { srl } * (self.slow - lvl);
            let ratio = (self.fast + 1e-5) / (self.slow + 1e-5);
            let tg = ratio.powf(self.trans * 1.5).clamp(0.25, 4.0);
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = buf[i];
                let mut y = x * tg;
                y = soft_clip(y * pre) * post;
                if self.crunch > 0.0 {
                    let hi = self.crunch_hp[ch].hp(y);
                    y += (soft_clip(hi * 8.0) * 0.25 - hi * 0.5) * self.crunch;
                }
                y = self.damp[ch].lp(y);
                if self.boom > 0.0 {
                    let low = self.boom_in[ch].lp(x);
                    let b = self.boom_f[ch].process(low, FilterMode::Band);
                    y += soft_clip(b * self.boom * 4.0) * 0.8;
                }
                buf[i] = (x * self.dry + y * self.wet) * self.out;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct DynamicTube {
    drive: f32,
    bias: f32,
    tilt: [OnePole; 2],
    tone: f32,
    tube: i32,
    out: f32,
    dry: f32,
    wet: f32,
    dc: [OnePole; 2],
}

impl Effect for DynamicTube {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.drive = db_to_gain(p[0]);
        self.bias = p[1] * 0.4;
        self.tone = p[2];
        self.tube = p[3].round() as i32;
        self.out = db_to_gain(p[4]);
        (self.dry, self.wet) = mix_gains(p[5]);
        for f in &mut self.tilt {
            f.set_lowpass(1200.0, ctx.sr);
        }
        for f in &mut self.dc {
            f.set_lowpass(10.0, ctx.sr);
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let b = self.bias;
        let comp = 1.0 / self.drive.sqrt().max(1.0);
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let v = *x * self.drive;
                let y = match self.tube {
                    0 => soft_clip(v + b) - soft_clip(b),
                    1 => {
                        let a = v + b;
                        (if a > 0.0 { soft_clip(a) } else { soft_clip(a * 0.6) / 0.6 * 0.8 }) - soft_clip(b)
                    }
                    _ => (v + b).clamp(-0.8, 0.8) - b.clamp(-0.8, 0.8),
                } * comp;
                let y = self.dc[ch].hp(y);
                let lo = self.tilt[ch].lp(y);
                let hi = y - lo;
                let y = lo * (1.0 - self.tone * 0.5) + hi * (1.0 + self.tone * 0.8);
                *x = (*x * self.dry + y * self.wet) * self.out;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// EQ Eight

#[derive(Clone, Copy, Default)]
pub struct Eq8Band {
    pub on: bool,
    pub stages: usize,
    pub bq: [Biquad; 4],
}

pub fn eq8_bands(p: &[f32], sr: f32) -> [Eq8Band; 8] {
    let mut bands = [Eq8Band::default(); 8];
    for (b, band) in bands.iter_mut().enumerate() {
        let o = b * EQ8_BAND_PARAMS;
        band.on = p[o] > 0.5;
        let typ = p[o + 1].round() as i32;
        let (f, g, q) = (p[o + 2].min(sr * 0.45), p[o + 3], p[o + 4]);
        band.stages = 1;
        match typ {
            0 => {
                band.stages = 4;
                band.bq[0].highpass(f, q, sr);
                for k in 1..4 {
                    band.bq[k].highpass(f, 0.707, sr);
                }
            }
            1 => band.bq[0].highpass(f, q, sr),
            2 => band.bq[0].low_shelf(f, g, sr),
            3 => band.bq[0].peaking(f, q, g, sr),
            4 => band.bq[0].notch(f, q, sr),
            5 => band.bq[0].high_shelf(f, g, sr),
            6 => band.bq[0].lowpass(f, q, sr),
            _ => {
                band.stages = 4;
                band.bq[0].lowpass(f, q, sr);
                for k in 1..4 {
                    band.bq[k].lowpass(f, 0.707, sr);
                }
            }
        }
    }
    bands
}

/// Combined EQ Eight response in dB at `f` (for the display).
pub fn eq8_response(bands: &[Eq8Band; 8], f: f32, sr: f32) -> f32 {
    let mut m = 1.0;
    for b in bands.iter().filter(|b| b.on) {
        for k in 0..b.stages {
            m *= b.bq[k].magnitude(f, sr);
        }
    }
    gain_to_db(m)
}

#[derive(Default)]
pub struct Eq8 {
    bands: [[Eq8Band; 8]; 2],
    out: f32,
}

impl Effect for Eq8 {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        let b = eq8_bands(p, ctx.sr);
        // Keep filter state when only coefficients change.
        for ch in 0..2 {
            for (dst, src) in self.bands[ch].iter_mut().zip(b.iter()) {
                let old = *dst;
                *dst = *src;
                for k in 0..4 {
                    dst.bq[k].copy_state(&old.bq[k]);
                }
            }
        }
        self.out = db_to_gain(p[8 * EQ8_BAND_PARAMS]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let mut y = *x;
                for b in self.bands[ch].iter_mut().filter(|b| b.on) {
                    for k in 0..b.stages {
                        y = b.bq[k].process(y);
                    }
                }
                *x = y * self.out;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Erosion {
    d: [DelayLine; 2],
    mode: i32,
    freq: f32,
    amount: f32,
    bp: [Svf; 2],
    osc: Osc,
    rng: [Rng; 2],
}

impl Erosion {
    pub fn new() -> Self {
        Self { d: [DelayLine::new(64), DelayLine::new(64)], mode: 0, freq: 4000.0, amount: 0.3, bp: Default::default(), osc: Osc::default(), rng: [Rng::new(3), Rng::new(99)] }
    }
}

impl Effect for Erosion {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.mode = p[0].round() as i32;
        self.freq = p[1];
        self.amount = p[2];
        let q = 0.5 + (1.0 - p[3]) * 8.0;
        for f in &mut self.bp {
            f.set(self.freq, q, ctx.sr);
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        for i in 0..l.len() {
            let sine = self.osc.sine(self.freq, ctx.sr);
            let n0 = self.bp[0].process(self.rng[0].next(), FilterMode::Band) * 3.0;
            let n1 = if self.mode == 1 { self.bp[1].process(self.rng[1].next(), FilterMode::Band) * 3.0 } else { n0 };
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let m = match self.mode {
                    2 => sine,
                    _ => if ch == 0 { n0 } else { n1 },
                };
                self.d[ch].write(buf[i]);
                buf[i] = self.d[ch].read(3.0 + self.amount * m * 2.5);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct FilterDelay {
    lines: [DelayLine; 3],
    bp: [Svf; 3],
    beats: [f32; 3],
    time: [Smooth; 3],
    fb: [f32; 3],
    vol: [f32; 3],
    dry: f32,
}

impl FilterDelay {
    pub fn new(sr: f32) -> Self {
        Self {
            lines: [DelayLine::new((sr * 6.0) as usize), DelayLine::new((sr * 6.0) as usize), DelayLine::new((sr * 6.0) as usize)],
            bp: Default::default(),
            beats: [0.5, 1.0, 0.75],
            time: [Smooth::new(sr * 0.25, 0.08, sr); 3],
            fb: [0.4; 3],
            vol: [0.6; 3],
            dry: 1.0,
        }
    }
}

impl Effect for FilterDelay {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        for k in 0..3 {
            let o = k * 4;
            self.beats[k] = division_beats(p[o]);
            self.bp[k].set(p[o + 1], 1.2, ctx.sr);
            self.fb[k] = p[o + 2];
            self.vol[k] = p[o + 3];
        }
        self.dry = p[12];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        for k in 0..3 {
            self.time[k].target = (self.beats[k] * 60.0 / ctx.bpm * ctx.sr).min(ctx.sr * 5.5);
        }
        for i in 0..l.len() {
            let inputs = [l[i], (l[i] + r[i]) * 0.5, r[i]];
            let mut outs = [0.0; 3];
            for k in 0..3 {
                let t = self.time[k].next();
                let y = self.lines[k].read(t);
                let f = self.bp[k].process(y, FilterMode::BandNorm);
                self.lines[k].write(inputs[k] + f * self.fb[k]);
                outs[k] = f * self.vol[k];
            }
            l[i] = l[i] * self.dry + outs[0] + outs[1];
            r[i] = r[i] * self.dry + outs[2] + outs[1];
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Flanger {
    d: [DelayLine; 2],
    delay: f32,
    rate: f32,
    depth: f32,
    fb: f32,
    spread: f32,
    dry: f32,
    wet: f32,
    ph: f32,
    last: [f32; 2],
}

impl Flanger {
    pub fn new(sr: f32) -> Self {
        Self { d: [DelayLine::new((sr * 0.05) as usize), DelayLine::new((sr * 0.05) as usize)], delay: 2.5, rate: 0.3, depth: 0.6, fb: 0.5, spread: 0.5, dry: 0.7, wet: 0.7, ph: 0.0, last: [0.0; 2] }
    }
}

impl Effect for Flanger {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.delay = p[0];
        self.rate = p[1];
        self.depth = p[2];
        self.fb = p[3];
        self.spread = p[4];
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        for i in 0..l.len() {
            self.ph = (self.ph + self.rate / ctx.sr) % 1.0;
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let lfo = ((self.ph + ch as f32 * self.spread * 0.5) * TAU).sin();
                let ms = (self.delay * (1.0 + self.depth * 0.95 * lfo)).max(0.05);
                let y = self.d[ch].read(ms / 1000.0 * ctx.sr);
                self.d[ch].write(buf[i] + y * self.fb);
                self.last[ch] = y;
                buf[i] = buf[i] * self.dry + y * self.wet;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct FreqShifter {
    h: [Hilbert; 2],
    ring: bool,
    shift: f32,
    wide: bool,
    drive: f32,
    dry: f32,
    wet: f32,
    ph: f32,
}

impl Effect for FreqShifter {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.ring = p[0] > 0.5;
        self.shift = p[1] + p[2];
        self.wide = p[3] > 0.5;
        self.drive = p[4];
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        for i in 0..l.len() {
            self.ph = (self.ph + self.shift / ctx.sr).rem_euclid(1.0);
            let (s, c) = (self.ph * TAU).sin_cos();
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = buf[i];
                let xin = if self.drive > 0.0 { soft_clip(x * (1.0 + self.drive * 6.0)) } else { x };
                let y = if self.ring {
                    xin * s
                } else {
                    let (re, im) = self.h[ch].process(xin);
                    let sign = if self.wide && ch == 1 { -1.0 } else { 1.0 };
                    re * c - im * s * sign
                };
                buf[i] = x * self.dry + y * self.wet;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Gate {
    open_th: f32,
    close_th: f32,
    att: f32,
    hold: f32,
    rel: f32,
    floor: f32,
    env: f32,
    gain: f32,
    hold_left: f32,
    open: bool,
    gr: f32,
}

impl Effect for Gate {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.open_th = db_to_gain(p[0]);
        self.close_th = db_to_gain(p[0] - p[1]);
        self.att = 1.0 / (p[2] * ctx.sr).max(1.0);
        self.hold = p[3] * ctx.sr;
        self.rel = decay_mult(p[4], ctx.sr);
        self.floor = db_to_gain(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let envc = one_pole_coef(0.002, ctx.sr);
        let mut min_g: f32 = 1.0;
        for i in 0..l.len() {
            let lvl = l[i].abs().max(r[i].abs());
            self.env = if lvl > self.env { lvl } else { lvl + envc * (self.env - lvl) };
            if self.env > self.open_th {
                self.open = true;
                self.hold_left = self.hold;
            } else if self.env < self.close_th {
                if self.hold_left > 0.0 {
                    self.hold_left -= 1.0;
                } else {
                    self.open = false;
                }
            }
            if self.open {
                self.gain = (self.gain + self.att).min(1.0);
            } else {
                self.gain = self.floor + (self.gain - self.floor) * self.rel;
            }
            let g = self.gain.max(self.floor);
            min_g = min_g.min(g);
            l[i] *= g;
            r[i] *= g;
        }
        self.gr = -gain_to_db(min_g);
    }
    fn reduction(&self) -> f32 {
        self.gr.min(60.0)
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Glue {
    thresh: f32,
    ratio: f32,
    att: f32,
    rel: f32,
    auto: bool,
    makeup: f32,
    dry: f32,
    wet: f32,
    clip: bool,
    env: f32,
    gr_db: f32,
    gr: f32,
}

impl Effect for Glue {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.thresh = p[0];
        self.ratio = [2.0, 4.0, 10.0][(p[1].round() as usize).min(2)];
        let att_ms = [0.01, 0.1, 0.3, 1.0, 3.0, 10.0, 30.0][(p[2].round() as usize).min(6)];
        self.att = one_pole_coef(att_ms / 1000.0, ctx.sr);
        let ri = p[3].round() as usize;
        self.auto = ri >= 6;
        let rel_s = [0.1, 0.2, 0.4, 0.6, 0.8, 1.2, 0.4][ri.min(6)];
        self.rel = one_pole_coef(rel_s, ctx.sr);
        self.makeup = db_to_gain(p[4]);
        (self.dry, self.wet) = mix_gains(p[5]);
        self.clip = p[6] > 0.5;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let fast_rel = one_pole_coef(0.1, ctx.sr);
        let slow_rel = one_pole_coef(1.2, ctx.sr);
        let mut max_gr: f32 = 0.0;
        for i in 0..l.len() {
            let lvl = gain_to_db(l[i].abs().max(r[i].abs()));
            let over = (lvl - self.thresh).max(0.0);
            let target = over * (1.0 - 1.0 / self.ratio);
            let rel = if self.auto {
                // Program-dependent release: short for transients, long for sustained reduction.
                let t = (self.gr_db / 12.0).clamp(0.0, 1.0);
                fast_rel + (slow_rel - fast_rel) * t
            } else {
                self.rel
            };
            self.gr_db = if target > self.gr_db { target + self.att * (self.gr_db - target) } else { target + rel * (self.gr_db - target) };
            max_gr = max_gr.max(self.gr_db);
            let g = db_to_gain(-self.gr_db) * self.makeup;
            for buf in [&mut *l, &mut *r] {
                let mut y = buf[i] * g;
                if self.clip {
                    y = soft_clip(y * 1.2) / 1.2;
                }
                buf[i] = buf[i] * self.dry + y * self.wet;
            }
        }
        let _ = self.env;
        self.gr = max_gr;
    }
    fn reduction(&self) -> f32 {
        self.gr
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Grain {
    active: bool,
    pos: f64,
    rate: f64,
    age: f32,
    len: f32,
}

pub struct GrainDelay {
    buf: [Vec<f32>; 2],
    w: usize,
    time: f32,
    spray: f32,
    freq: f32,
    pitch: f32,
    rand_pitch: f32,
    fb: f32,
    dry: f32,
    wet: f32,
    grains: [Grain; 16],
    next_grain: f32,
    rng: Rng,
    last: [f32; 2],
}

impl GrainDelay {
    pub fn new(sr: f32) -> Self {
        let n = (sr * 3.0) as usize;
        Self {
            buf: [vec![0.0; n], vec![0.0; n]],
            w: 0,
            time: 100.0,
            spray: 20.0,
            freq: 60.0,
            pitch: 0.0,
            rand_pitch: 0.0,
            fb: 0.2,
            dry: 0.7,
            wet: 0.7,
            grains: [Grain::default(); 16],
            next_grain: 0.0,
            rng: Rng::new(4242),
            last: [0.0; 2],
        }
    }
}

impl Effect for GrainDelay {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.time = p[0];
        self.spray = p[1];
        self.freq = p[2];
        self.pitch = p[3];
        self.rand_pitch = p[4];
        self.fb = p[5];
        (self.dry, self.wet) = mix_gains(p[6]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let len = self.buf[0].len();
        let grain_len = (2.0 / self.freq * ctx.sr).min(ctx.sr * 0.5);
        for i in 0..l.len() {
            self.next_grain -= 1.0;
            if self.next_grain <= 0.0 {
                self.next_grain = ctx.sr / self.freq;
                if let Some(g) = self.grains.iter_mut().find(|g| !g.active) {
                    let spray = (self.rng.next() * 0.5 + 0.5) * self.spray / 1000.0 * ctx.sr;
                    let delay = self.time / 1000.0 * ctx.sr + spray;
                    let semis = self.pitch + self.rng.next() * self.rand_pitch * 12.0;
                    let rate = 2f64.powf(semis as f64 / 12.0);
                    // Start far enough back that a faster-reading grain never overtakes the write head.
                    let lead = (grain_len as f64 * (rate - 1.0)).max(0.0);
                    g.pos = self.w as f64 - delay as f64 - lead;
                    g.rate = rate;
                    g.age = 0.0;
                    g.len = grain_len;
                    g.active = true;
                }
            }
            let mut out = [0.0f32; 2];
            for g in self.grains.iter_mut().filter(|g| g.active) {
                let wnd = (PI * g.age / g.len).sin().powi(2);
                let idx = g.pos.rem_euclid(len as f64);
                let i0 = idx as usize % len;
                let i1 = (i0 + 1) % len;
                let fr = (idx - idx.floor()) as f32;
                for (ch, o) in out.iter_mut().enumerate() {
                    *o += (self.buf[ch][i0] * (1.0 - fr) + self.buf[ch][i1] * fr) * wnd;
                }
                g.pos += g.rate;
                g.age += 1.0;
                if g.age >= g.len {
                    g.active = false;
                }
            }
            for o in &mut out {
                *o *= 0.7;
            }
            self.buf[0][self.w] = l[i] + out[0] * self.fb;
            self.buf[1][self.w] = r[i] + out[1] * self.fb;
            self.w = (self.w + 1) % len;
            self.last = out;
            l[i] = l[i] * self.dry + out[0] * self.wet;
            r[i] = r[i] * self.dry + out[1] * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Multiband {
    lo: [[Svf; 2]; 2],
    hi: [[Svf; 2]; 2],
    above_t: [f32; 3],
    above_r: [f32; 3],
    below_t: [f32; 3],
    below_r: [f32; 3],
    gain: [f32; 3],
    amount: f32,
    time: f32,
    out: f32,
    env: [f32; 3],
    gr: f32,
}

impl Effect for Multiband {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        for ch in 0..2 {
            for k in 0..2 {
                self.lo[ch][k].set(p[0], 0.707, ctx.sr);
                self.hi[ch][k].set(p[1], 0.707, ctx.sr);
            }
        }
        for b in 0..3 {
            let o = 2 + b * 5;
            self.above_t[b] = p[o];
            self.above_r[b] = p[o + 1];
            self.below_t[b] = p[o + 2];
            self.below_r[b] = p[o + 3];
            self.gain[b] = db_to_gain(p[o + 4]);
        }
        self.amount = p[17];
        self.time = p[18];
        self.out = db_to_gain(p[19]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        // Faster time constants for higher bands, scaled by Time.
        let att = [0.02, 0.008, 0.003].map(|t: f32| one_pole_coef(t * self.time, ctx.sr));
        let rel = [0.2, 0.12, 0.06].map(|t: f32| one_pole_coef(t * self.time, ctx.sr));
        let mut max_gr: f32 = 0.0;
        for i in 0..l.len() {
            let mut bands = [[0.0f32; 2]; 3];
            for (ch, buf) in [&*l, &*r].into_iter().enumerate() {
                let x = buf[i];
                let lo1 = self.lo[ch][0].process(x, FilterMode::Low);
                let lo = self.lo[ch][1].process(lo1, FilterMode::Low);
                let hi1 = self.hi[ch][0].process(x, FilterMode::High);
                let hi = self.hi[ch][1].process(hi1, FilterMode::High);
                bands[0][ch] = lo;
                bands[2][ch] = hi;
                bands[1][ch] = x - lo - hi;
            }
            let mut ol = 0.0;
            let mut or = 0.0;
            for b in 0..3 {
                let lvl = bands[b][0].abs().max(bands[b][1].abs());
                let c = if lvl > self.env[b] { att[b] } else { rel[b] };
                self.env[b] = lvl + c * (self.env[b] - lvl);
                let db = gain_to_db(self.env[b]);
                let mut gdb = 0.0;
                if db > self.above_t[b] {
                    gdb -= (db - self.above_t[b]) * (1.0 - 1.0 / self.above_r[b]);
                } else if db < self.below_t[b] && db > -80.0 {
                    gdb += ((self.below_t[b] - db) * (1.0 - 1.0 / self.below_r[b])).min(24.0);
                }
                gdb *= self.amount;
                max_gr = max_gr.max(-gdb);
                let g = db_to_gain(gdb) * self.gain[b];
                ol += bands[b][0] * g;
                or += bands[b][1] * g;
            }
            l[i] = ol * self.out;
            r[i] = or * self.out;
        }
        self.gr = max_gr.max(0.0);
    }
    fn reduction(&self) -> f32 {
        self.gr
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Overdrive {
    drive: f32,
    tone: [OnePole; 2],
    tone_amt: f32,
    bp: [Svf; 2],
    dynamics: f32,
    env: f32,
    dry: f32,
    wet: f32,
}

impl Effect for Overdrive {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.drive = p[0];
        self.tone_amt = p[1];
        for f in &mut self.tone {
            f.set_lowpass(800.0 + p[1] * 12000.0, ctx.sr);
        }
        let q = 1.0 / (2f32.powf(p[3] / 2.0) - 2f32.powf(-p[3] / 2.0)).max(0.1);
        for f in &mut self.bp {
            f.set(p[2], q.max(0.3), ctx.sr);
        }
        self.dynamics = p[4];
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let pre = 1.0 + self.drive * 30.0;
        let ec = one_pole_coef(0.05, ctx.sr);
        for i in 0..l.len() {
            let lvl = l[i].abs().max(r[i].abs());
            self.env = lvl + ec * (self.env - lvl);
            let comp = 1.0 / (1.0 + self.env * pre * (1.0 - self.dynamics) * 0.5);
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = buf[i];
                let b = self.bp[ch].process(x, FilterMode::Band) + x * 0.1;
                let d = b * pre * comp;
                let y = if d > 0.0 { soft_clip(d) } else { soft_clip(d * 1.3) * 0.85 };
                let y = self.tone[ch].lp(y) * 0.6;
                buf[i] = x * self.dry + y * self.wet;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Pedal {
    typ: i32,
    gain: f32,
    out: f32,
    eq: [[Biquad; 3]; 2],
    sub: bool,
    sub_f: [Biquad; 2],
    pre_hp: [OnePole; 2],
    dry: f32,
    wet: f32,
}

impl Effect for Pedal {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.typ = p[0].round() as i32;
        self.gain = p[1];
        self.out = db_to_gain(p[2]);
        for ch in 0..2 {
            self.eq[ch][0].low_shelf(150.0, p[3], ctx.sr);
            self.eq[ch][1].peaking(900.0, 0.8, p[4], ctx.sr);
            self.eq[ch][2].high_shelf(3500.0, p[5], ctx.sr);
            self.sub_f[ch].peaking(80.0, 0.8, 6.0, ctx.sr);
            self.pre_hp[ch].set_lowpass(if self.typ == 2 { 40.0 } else { 120.0 }, ctx.sr);
        }
        self.sub = p[6] > 0.5;
        (self.dry, self.wet) = mix_gains(p[7]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let g = match self.typ {
            0 => 1.0 + self.gain * 20.0,
            1 => 1.0 + self.gain * 80.0,
            _ => 1.0 + self.gain * 200.0,
        };
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let v = self.pre_hp[ch].hp(*x) * g;
                let mut y = match self.typ {
                    0 => soft_clip(v) * 0.7,
                    1 => soft_clip(v * 1.5).clamp(-0.7, 0.7),
                    _ => {
                        let a = v.clamp(-1.0, 1.0);
                        (a.signum() * (1.0 - (-a.abs() * 4.0).exp())) * 0.6
                    }
                };
                for b in &mut self.eq[ch] {
                    y = b.process(y);
                }
                if self.sub {
                    y = self.sub_f[ch].process(y);
                }
                *x = (*x * self.dry + y * self.wet) * self.out;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Phaser {
    poles: usize,
    freq: f32,
    rate: f32,
    amount: f32,
    fb: f32,
    spread: f32,
    dry: f32,
    wet: f32,
    ph: f32,
    z: [[f32; 12]; 2],
    last: [f32; 2],
    coef: [f32; 2],
}

impl Effect for Phaser {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.poles = (p[0].round() as usize).clamp(2, 12);
        self.freq = p[1];
        self.rate = p[2];
        self.amount = p[3];
        self.fb = p[4];
        self.spread = p[5];
        (self.dry, self.wet) = mix_gains(p[6]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        for i in 0..l.len() {
            self.ph = (self.ph + self.rate / ctx.sr) % 1.0;
            if i % 8 == 0 {
                for ch in 0..2 {
                    let lfo = ((self.ph + ch as f32 * self.spread * 0.5) * TAU).sin();
                    let f = (self.freq * 2f32.powf(lfo * self.amount * 2.5)).clamp(20.0, ctx.sr * 0.45);
                    let t = (PI * f / ctx.sr).tan();
                    self.coef[ch] = (t - 1.0) / (t + 1.0);
                }
            }
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let a = self.coef[ch];
                let mut y = buf[i] + self.last[ch] * self.fb;
                for k in 0..self.poles {
                    let z = self.z[ch][k];
                    let out = a * y + z;
                    self.z[ch][k] = y - a * out;
                    y = out;
                }
                self.last[ch] = y;
                buf[i] = buf[i] * self.dry + y * self.wet;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct PingPong {
    dl: DelayLine,
    dr: DelayLine,
    time: Smooth,
    beats: f32,
    fb: f32,
    bp: [Svf; 2],
    dry: f32,
    wet: f32,
}

impl PingPong {
    pub fn new(sr: f32) -> Self {
        Self { dl: DelayLine::new((sr * 6.0) as usize), dr: DelayLine::new((sr * 6.0) as usize), time: Smooth::new(sr * 0.25, 0.08, sr), beats: 0.5, fb: 0.5, bp: Default::default(), dry: 0.9, wet: 0.4 }
    }
}

impl Effect for PingPong {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.beats = division_beats(p[0]);
        self.fb = p[1];
        let q = 0.4 + (1.0 - p[3]) * 6.0;
        for f in &mut self.bp {
            f.set(p[2], q, ctx.sr);
        }
        (self.dry, self.wet) = mix_gains(p[4]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        self.time.target = (self.beats * 60.0 / ctx.bpm * ctx.sr).min(ctx.sr * 5.5);
        for i in 0..l.len() {
            let t = self.time.next();
            let yl = self.dl.read(t);
            let yr = self.dr.read(t);
            let fl = self.bp[0].process(yl, FilterMode::BandNorm);
            let fr = self.bp[1].process(yr, FilterMode::BandNorm);
            self.dl.write((l[i] + r[i]) * 0.5 + fr * self.fb);
            self.dr.write(fl * self.fb);
            l[i] = l[i] * self.dry + fl * self.wet;
            r[i] = r[i] * self.dry + fr * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Resonators {
    lines: [[DelayLine; 5]; 2],
    damp: [[OnePole; 5]; 2],
    periods: [f32; 5],
    fb: f32,
    width: f32,
    dry: f32,
    wet: f32,
}

impl Resonators {
    pub fn new(sr: f32) -> Self {
        let mk = || std::array::from_fn(|_| DelayLine::new((sr / 20.0) as usize));
        Self { lines: [mk(), mk()], damp: Default::default(), periods: [100.0; 5], fb: 0.99, width: 0.5, dry: 0.7, wet: 0.7 }
    }
}

impl Effect for Resonators {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        let root = p[0];
        let notes = [root, root + p[3], root + p[4], root + p[5], root + p[6]];
        for (k, n) in notes.iter().enumerate() {
            self.periods[k] = (ctx.sr / midi_to_freq(*n)).clamp(2.0, ctx.sr / 20.0 - 2.0);
        }
        self.fb = 0.9 + p[1].powf(0.3) * 0.0995;
        for ch in &mut self.damp {
            for f in ch.iter_mut() {
                f.set_lowpass(1000.0 + p[2] * 15000.0, ctx.sr);
            }
        }
        self.width = p[7];
        (self.dry, self.wet) = mix_gains(p[8]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for i in 0..l.len() {
            let m = (l[i] + r[i]) * 0.5;
            let mut out = [0.0f32; 2];
            for k in 0..5 {
                // Alternate resonators between the sides according to Width.
                let pan = if k % 2 == 0 { 1.0 - self.width * 0.5 } else { self.width * 0.5 };
                for ch in 0..2 {
                    let y = self.lines[ch][k].read(self.periods[k]);
                    let d = self.damp[ch][k].lp(y);
                    self.lines[ch][k].write(m * 0.1 + d * self.fb);
                    let g = if ch == 0 { pan } else { 1.0 - pan };
                    out[ch] += y * g;
                }
            }
            l[i] = l[i] * self.dry + soft_clip(out[0]) * self.wet;
            r[i] = r[i] * self.dry + soft_clip(out[1]) * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub struct SimpleDelay {
    d: [DelayLine; 2],
    t: [Smooth; 2],
    beats: [f32; 2],
    fb: f32,
    dry: f32,
    wet: f32,
}

impl SimpleDelay {
    pub fn new(sr: f32) -> Self {
        Self { d: [DelayLine::new((sr * 6.0) as usize), DelayLine::new((sr * 6.0) as usize)], t: [Smooth::new(sr * 0.25, 0.08, sr); 2], beats: [0.5, 0.75], fb: 0.4, dry: 0.9, wet: 0.4 }
    }
}

impl Effect for SimpleDelay {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.beats = [division_beats(p[0]), division_beats(p[1])];
        self.fb = p[2];
        (self.dry, self.wet) = mix_gains(p[3]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        for ch in 0..2 {
            self.t[ch].target = (self.beats[ch] * 60.0 / ctx.bpm * ctx.sr).min(ctx.sr * 5.5);
        }
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let t = self.t[ch].next();
                let y = self.d[ch].read(t);
                self.d[ch].write(*x + y * self.fb);
                *x = *x * self.dry + y * self.wet;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Vinyl {
    tracing: f32,
    bp: [Svf; 2],
    pinch: f32,
    density: f32,
    crackle_vol: f32,
    rng: Option<Rng>,
    crackle_lp: OnePole,
    pop: f32,
}

impl Effect for Vinyl {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.tracing = p[0];
        for f in &mut self.bp {
            f.set(p[1], 0.7, ctx.sr);
        }
        self.pinch = p[2];
        self.density = p[3];
        self.crackle_vol = p[4];
        self.crackle_lp.set_lowpass(5000.0, ctx.sr);
        if self.rng.is_none() {
            self.rng = Some(Rng::new(1999));
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let rng = self.rng.get_or_insert(Rng::new(1999));
        let chance = self.density * self.density * 2000.0 / ctx.sr;
        for i in 0..l.len() {
            if (rng.next() * 0.5 + 0.5) < chance {
                self.pop = rng.next() * self.crackle_vol;
            }
            let crackle = self.crackle_lp.lp(self.pop);
            self.pop *= 0.6;
            let (a, b) = (l[i], r[i]);
            let m = (a + b) * 0.5;
            let s = (a - b) * 0.5;
            let s = s + self.pinch * soft_clip(s * 4.0) * 0.3;
            let (mut a, mut b) = (m + s, m - s);
            for (ch, v) in [&mut a, &mut b].into_iter().enumerate() {
                let band = self.bp[ch].process(*v, FilterMode::Band);
                *v += band * band * self.tracing * 3.0 * band.signum().max(0.0);
                *v += crackle;
            }
            l[i] = a;
            r[i] = b;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Analysis devices: their output is published to the UI through `display()`.

pub const SPECTRUM_BANDS: usize = 96;
const FFT_N: usize = 2048;

pub struct Spectrum {
    buf: Vec<f32>,
    w: usize,
    count: usize,
    re: Vec<f32>,
    im: Vec<f32>,
    bands: Vec<f32>,
    smooth: f32,
    sr: f32,
}

impl Spectrum {
    pub fn new(sr: f32) -> Self {
        Self { buf: vec![0.0; FFT_N], w: 0, count: 0, re: vec![0.0; FFT_N], im: vec![0.0; FFT_N], bands: vec![-90.0; SPECTRUM_BANDS], smooth: 0.7, sr }
    }

    fn analyse(&mut self) {
        for k in 0..FFT_N {
            let x = self.buf[(self.w + k) % FFT_N];
            let wnd = 0.5 - 0.5 * (TAU * k as f32 / FFT_N as f32).cos();
            self.re[k] = x * wnd;
            self.im[k] = 0.0;
        }
        fft(&mut self.re, &mut self.im);
        let (lo, hi) = (20f32, 20000f32);
        for b in 0..SPECTRUM_BANDS {
            let f0 = lo * (hi / lo).powf(b as f32 / SPECTRUM_BANDS as f32);
            let f1 = lo * (hi / lo).powf((b + 1) as f32 / SPECTRUM_BANDS as f32);
            let i0 = ((f0 / self.sr * FFT_N as f32) as usize).clamp(1, FFT_N / 2 - 1);
            let i1 = ((f1 / self.sr * FFT_N as f32) as usize).clamp(i0, FFT_N / 2 - 1);
            let mut m: f32 = 0.0;
            for k in i0..=i1 {
                m = m.max((self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt());
            }
            let db = gain_to_db(m * 4.0 / FFT_N as f32);
            self.bands[b] = if db > self.bands[b] { db } else { db + self.smooth * (self.bands[b] - db) };
        }
    }
}

impl Effect for Spectrum {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.smooth = p[0];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for i in 0..l.len() {
            self.buf[self.w] = (l[i] + r[i]) * 0.5;
            self.w = (self.w + 1) % FFT_N;
            self.count += 1;
            if self.count >= FFT_N / 2 {
                self.count = 0;
                self.analyse();
            }
        }
    }
    fn display(&self) -> Option<&[f32]> {
        Some(&self.bands)
    }
}

pub struct Tuner {
    buf: Vec<f32>,
    w: usize,
    count: usize,
    out: [f32; 3],
    reference: f32,
    sr: f32,
    lin: Vec<f32>,
}

impl Tuner {
    pub fn new(sr: f32) -> Self {
        Self { buf: vec![0.0; 4096], w: 0, count: 0, out: [0.0, 0.0, 440.0], reference: 440.0, sr, lin: vec![0.0; 4096] }
    }

    fn detect(&mut self) {
        let n = self.buf.len();
        for k in 0..n {
            self.lin[k] = self.buf[(self.w + k) % n];
        }
        let x = &self.lin;
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
        if rms < 0.005 {
            self.out[1] = 0.0;
            return;
        }
        // Normalized squared-difference function (YIN-like), searching 40 Hz .. 2 kHz.
        let min_lag = (self.sr / 2000.0) as usize;
        let max_lag = ((self.sr / 40.0) as usize).min(n / 2);
        let w = n / 2;
        let mut best = (0usize, f32::MAX);
        let mut prev_cmnd = 1.0;
        let mut running = 0.0;
        for lag in 1..max_lag {
            let mut d = 0.0;
            for i in (0..w).step_by(2) {
                let diff = x[i] - x[i + lag];
                d += diff * diff;
            }
            running += d;
            let cmnd = d * lag as f32 / running.max(1e-9);
            if lag >= min_lag && cmnd < 0.15 && cmnd > prev_cmnd && best.0 == 0 {
                best = (lag - 1, prev_cmnd);
                break;
            }
            if lag >= min_lag && cmnd < best.1 {
                best = (lag, cmnd);
            }
            prev_cmnd = cmnd;
        }
        if best.0 == 0 || best.1 > 0.4 {
            self.out[1] = 0.0;
            return;
        }
        self.out[0] = self.sr / best.0 as f32;
        self.out[1] = 1.0 - best.1;
        self.out[2] = self.reference;
    }
}

impl Effect for Tuner {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.reference = p[0];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for i in 0..l.len() {
            self.buf[self.w] = (l[i] + r[i]) * 0.5;
            self.w = (self.w + 1) % self.buf.len();
            self.count += 1;
            if self.count >= 2048 {
                self.count = 0;
                self.detect();
            }
        }
    }
    fn display(&self) -> Option<&[f32]> {
        Some(&self.out)
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Vocoder {
    carrier: i32,
    pitch: f32,
    nbands: usize,
    lo: f32,
    hi: f32,
    formant: f32,
    rel: f32,
    dry: f32,
    wet: f32,
    mod_f: Vec<Svf>,
    car_f: Vec<[Svf; 2]>,
    env: Vec<f32>,
    osc: Osc,
    rng: Rng,
    sr: f32,
    dirty: bool,
}

impl Vocoder {
    pub fn new(sr: f32) -> Self {
        Self {
            carrier: 1,
            pitch: 45.0,
            nbands: 16,
            lo: 80.0,
            hi: 12000.0,
            formant: 0.0,
            rel: 0.05,
            dry: 0.0,
            wet: 1.0,
            mod_f: vec![Svf::default(); 32],
            car_f: vec![[Svf::default(); 2]; 32],
            env: vec![0.0; 32],
            osc: Osc::default(),
            rng: Rng::new(5),
            sr,
            dirty: true,
        }
    }

    fn rebuild(&mut self) {
        let n = self.nbands;
        let q = n as f32 * 0.45;
        let shift = 2f32.powf(self.formant);
        for b in 0..n {
            let f = self.lo * (self.hi / self.lo).powf((b as f32 + 0.5) / n as f32);
            self.mod_f[b].set(f, q, self.sr);
            for ch in 0..2 {
                self.car_f[b][ch].set((f * shift).min(self.sr * 0.45), q, self.sr);
            }
        }
        self.dirty = false;
    }
}

impl Effect for Vocoder {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.carrier = p[0].round() as i32;
        self.pitch = p[1];
        self.nbands = [8, 16, 32][(p[2].round() as usize).min(2)];
        self.lo = p[3];
        self.hi = p[4];
        self.formant = p[5];
        self.rel = p[6];
        (self.dry, self.wet) = mix_gains(p[7]);
        self.dirty = true;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        if self.dirty {
            self.rebuild();
        }
        let att = one_pole_coef(0.002, ctx.sr);
        let rel = one_pole_coef(self.rel, ctx.sr);
        let f = midi_to_freq(self.pitch);
        let n = self.nbands;
        for i in 0..l.len() {
            let m = (l[i] + r[i]) * 0.5;
            let car = match self.carrier {
                0 => self.rng.next(),
                1 => self.osc.next(Wave::Saw, f, ctx.sr),
                _ => self.osc.next(Wave::Square, f, ctx.sr),
            };
            let car_r = if self.carrier == 0 { self.rng.next() } else { car };
            let mut ol = 0.0;
            let mut or = 0.0;
            for b in 0..n {
                let mb = self.mod_f[b].process(m, FilterMode::Band).abs();
                let c = if mb > self.env[b] { att } else { rel };
                self.env[b] = mb + c * (self.env[b] - mb);
                let e = self.env[b] * 6.0;
                ol += self.car_f[b][0].process(car, FilterMode::Band) * e;
                or += self.car_f[b][1].process(car_r, FilterMode::Band) * e;
            }
            l[i] = l[i] * self.dry + soft_clip(ol * 2.0) * self.wet;
            r[i] = r[i] * self.dry + soft_clip(or * 2.0) * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct ChannelEq {
    hp_on: bool,
    b: [[Biquad; 4]; 2],
    out: f32,
}

impl Effect for ChannelEq {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.hp_on = p[0] > 0.5;
        for ch in 0..2 {
            let old = self.b[ch];
            self.b[ch][0].highpass(80.0, 0.707, ctx.sr);
            self.b[ch][1].low_shelf(100.0, p[1], ctx.sr);
            self.b[ch][2].peaking(p[3], 0.9, p[2], ctx.sr);
            self.b[ch][3].high_shelf(8000.0, p[4], ctx.sr);
            for k in 0..4 {
                self.b[ch][k].copy_state(&old[k]);
            }
        }
        self.out = db_to_gain(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let mut y = *x;
                if self.hp_on {
                    y = self.b[ch][0].process(y);
                }
                for k in 1..4 {
                    y = self.b[ch][k].process(y);
                }
                *x = y * self.out;
            }
        }
    }
}

