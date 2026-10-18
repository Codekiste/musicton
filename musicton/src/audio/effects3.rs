//! Third batch of audio effects: performance mangler, multi-stage distortion, hybrid reverb with
//! shimmer, transient shaper, maximizer, true-peak limiter with loudness meter, stereo imager.

use super::dsp::*;
use super::effects::{mix_gains, Ctx, Effect};
use std::collections::VecDeque;
use std::f32::consts::PI;

#[inline]
fn cascade(b: &mut [Biquad; 2], x: f32) -> f32 {
    let y = b[0].process(x);
    b[1].process(y)
}

// ---------------------------------------------------------------------------------------------
// Beat Mangler: tempo-synced gate / stutter / tape-stop / reverse patterns.

/// One character per step: `-` pass, `x` gate, `r` repeat half step, `R` loop previous step,
/// `t` tape stop, `v` reverse previous step, `h` half speed.
pub const MANGLER_PATTERNS: [&str; 8] = [
    "-x-x-x-x-x-x-x-x",
    "--x---x---x-x-x-",
    "------------rrrr",
    "--------------tt",
    "------------vvvv",
    "hhhhhhhhhhhhhhhh",
    "--r-x-R---v-r-tt",
    "x-rr-vx-R-hh-t-t",
];

pub struct Mangler {
    buf: [Vec<f32>; 2],
    w: usize,
    step_w: usize,
    step_len: usize,
    last_step: i64,
    pattern: usize,
    step_beats: f64,
    dry: f32,
    wet: f32,
    gain: f32,
    sr: f32,
}

impl Mangler {
    pub fn new(sr: f32) -> Self {
        let n = (sr * 6.0) as usize;
        Self { buf: [vec![0.0; n], vec![0.0; n]], w: 0, step_w: 0, step_len: 1, last_step: -1, pattern: 0, step_beats: 0.25, dry: 0.0, wet: 1.0, gain: 1.0, sr }
    }
}

impl Effect for Mangler {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.pattern = (p[0].round() as usize).min(MANGLER_PATTERNS.len() - 1);
        self.step_beats = [0.125, 0.25, 0.5][(p[1].round() as usize).min(2)];
        (self.dry, self.wet) = mix_gains(p[2]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let n = self.buf[0].len();
        let beats_per_sample = ctx.bpm as f64 / 60.0 / self.sr as f64;
        let pat = MANGLER_PATTERNS[self.pattern].as_bytes();
        let ramp = 1.0 - one_pole_coef(0.002, self.sr);
        for i in 0..l.len() {
            self.buf[0][self.w] = l[i];
            self.buf[1][self.w] = r[i];
            let mode = if ctx.playing {
                let pos = ctx.beat + i as f64 * beats_per_sample;
                let step = (pos / self.step_beats).floor() as i64;
                if step != self.last_step {
                    self.last_step = step;
                    self.step_w = self.w;
                    self.step_len = ((self.step_beats / beats_per_sample) as usize).clamp(16, n / 3);
                }
                pat[(step.rem_euclid(16)) as usize]
            } else {
                b'-'
            };
            let t = (self.w + n - self.step_w) % n;
            let len = self.step_len;
            let (src, g) = match mode {
                b'x' => (None, 0.0),
                b'r' => (Some(self.step_w + t % (len / 2).max(1)), 1.0),
                b'R' => (Some(self.step_w + n - len + t % len), 1.0),
                b'v' => (Some(self.step_w + n - 1 - t.min(len - 1)), 1.0),
                b'h' => (Some(self.step_w + t / 2), 1.0),
                b't' => {
                    let f = (t as f32 / len as f32).min(1.0);
                    (Some(self.step_w + (len as f32 * (f - f * f * 0.5)) as usize), 1.0 - f * f)
                }
                _ => (None, 1.0),
            };
            self.gain += (g - self.gain) * ramp;
            let (wl, wr) = match src {
                Some(idx) => (self.buf[0][idx % n], self.buf[1][idx % n]),
                None => (l[i], r[i]),
            };
            l[i] = l[i] * self.dry + wl * self.gain * self.wet;
            r[i] = r[i] * self.dry + wr * self.gain * self.wet;
            self.w = (self.w + 1) % n;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Heat Stack: three saturation stages with filters, 2x oversampled.


#[derive(Default)]
pub struct HeatStack {
    drive: f32,
    stages: [i32; 3],
    bias: f32,
    pre: [Biquad; 2],
    post: [Biquad; 2],
    aa: [[Biquad; 2]; 2],
    prev: [f32; 2],
    dry: f32,
    wet: f32,
    out: f32,
}

fn heat(kind: i32, x: f32, bias: f32) -> f32 {
    match kind {
        1 => {
            let y = x + bias;
            (y.tanh() - bias.tanh()) * 1.1
        }
        2 => {
            let y = x * 0.8 + bias * 0.5;
            y / (1.0 + y.abs()) * 1.6
        }
        3 => {
            let y = (x * 3.0 + bias).clamp(-1.0, 1.0);
            y.signum() * (1.0 - (-(y.abs() * 4.0)).exp())
        }
        4 => (x * PI * 0.5 + bias).sin(),
        5 => (x + bias).abs() * 1.4 - 0.5,
        6 => ((x + bias) * 8.0).round() / 8.0,
        _ => x,
    }
}

impl Effect for HeatStack {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.drive = db_to_gain(p[0]);
        for k in 0..3 {
            self.stages[k] = p[1 + k].round() as i32;
        }
        self.bias = p[4];
        for ch in 0..2 {
            let (a, b) = (self.pre[ch], self.post[ch]);
            self.pre[ch].highpass(p[5], 0.707, ctx.sr);
            self.post[ch].lowpass(p[6], 0.707, ctx.sr);
            self.pre[ch].copy_state(&a);
            self.post[ch].copy_state(&b);
            for k in 0..2 {
                let o = self.aa[ch][k];
                self.aa[ch][k].lowpass(ctx.sr * 0.45, 0.6 + 0.3 * k as f32, ctx.sr * 2.0);
                self.aa[ch][k].copy_state(&o);
            }
        }
        (self.dry, self.wet) = mix_gains(p[7]);
        self.out = db_to_gain(p[8]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let dry = *x;
                let input = self.pre[ch].process(dry) * self.drive;
                // 2x oversampling: linear interpolation up, saturate, filter, decimate.
                let mut y = 0.0;
                for (k, s) in [(self.prev[ch] + input) * 0.5, input].into_iter().enumerate() {
                    let mut v = s;
                    for &st in &self.stages {
                        v = heat(st, v, self.bias);
                    }
                    v = cascade(&mut self.aa[ch], v);
                    if k == 1 {
                        y = v;
                    }
                }
                self.prev[ch] = input;
                let wet = self.post[ch].process(y) / self.drive.sqrt().max(1.0);
                *x = (dry * self.dry + wet * self.wet) * self.out;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Hybrid Reverb: convolution early reflections + feedback delay network tail with shimmer.

const CONV_B: usize = 64;
const CONV_N: usize = 128;

struct Convolver {
    /// IR partitions (spectra), one set per output channel.
    h: [Vec<([f32; CONV_N], [f32; CONV_N])>; 2],
    /// Frequency-domain delay line of input spectra.
    fdl: Vec<([f32; CONV_N], [f32; CONV_N])>,
    fdl_pos: usize,
    inbuf: [f32; CONV_N],
    fill: usize,
    out: [[f32; CONV_B]; 2],
    out_pos: usize,
}

impl Convolver {
    fn new(parts: usize) -> Self {
        Self {
            h: [vec![([0.0; CONV_N], [0.0; CONV_N]); parts], vec![([0.0; CONV_N], [0.0; CONV_N]); parts]],
            fdl: vec![([0.0; CONV_N], [0.0; CONV_N]); parts],
            fdl_pos: 0,
            inbuf: [0.0; CONV_N],
            fill: 0,
            out: [[0.0; CONV_B]; 2],
            out_pos: 0,
        }
    }

    fn set_ir(&mut self, ir: [&[f32]; 2]) {
        for ch in 0..2 {
            for (p, part) in self.h[ch].iter_mut().enumerate() {
                let (re, im) = part;
                re.fill(0.0);
                im.fill(0.0);
                for k in 0..CONV_B {
                    re[k] = ir[ch].get(p * CONV_B + k).copied().unwrap_or(0.0);
                }
                fft(re, im);
            }
        }
    }

    /// Feeds one mono sample, returns the stereo output (64 samples latency).
    fn tick(&mut self, x: f32) -> (f32, f32) {
        self.inbuf[CONV_B + self.fill] = x;
        self.fill += 1;
        let o = (self.out[0][self.out_pos], self.out[1][self.out_pos]);
        self.out_pos += 1;
        if self.fill == CONV_B {
            self.fill = 0;
            self.out_pos = 0;
            let parts = self.fdl.len();
            self.fdl_pos = (self.fdl_pos + parts - 1) % parts;
            {
                let (re, im) = &mut self.fdl[self.fdl_pos];
                re.copy_from_slice(&self.inbuf);
                im.fill(0.0);
                fft(re, im);
            }
            for ch in 0..2 {
                let mut ar = [0.0f32; CONV_N];
                let mut ai = [0.0f32; CONV_N];
                for p in 0..parts {
                    let (xr, xi) = &self.fdl[(self.fdl_pos + p) % parts];
                    let (hr, hi) = &self.h[ch][p];
                    for k in 0..CONV_N {
                        ar[k] += xr[k] * hr[k] - xi[k] * hi[k];
                        ai[k] += xr[k] * hi[k] + xi[k] * hr[k];
                    }
                }
                // Inverse FFT via conjugation.
                for v in ai.iter_mut() {
                    *v = -*v;
                }
                fft(&mut ar, &mut ai);
                for k in 0..CONV_B {
                    self.out[ch][k] = ar[CONV_B + k] / CONV_N as f32;
                }
            }
            self.inbuf.copy_within(CONV_B.., 0);
        }
        o
    }
}

/// Octave-up pitch shifter (two crossfaded taps on a sweeping delay).
struct Shifter {
    d: DelayLine,
    phase: f32,
    win: f32,
}

impl Shifter {
    fn new(sr: f32) -> Self {
        Self { d: DelayLine::new((sr * 0.1) as usize), phase: 0.0, win: sr * 0.05 }
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.d.write(x);
        // Ratio 2: the delay shrinks by one sample per sample.
        self.phase = (self.phase - 1.0 / self.win).rem_euclid(1.0);
        let p2 = (self.phase + 0.5) % 1.0;
        let w1 = (self.phase * PI).sin();
        let w2 = (p2 * PI).sin();
        self.d.read(1.0 + self.phase * self.win) * w1 + self.d.read(1.0 + p2 * self.win) * w2
    }
}

const FDN: usize = 8;
const FDN_LEN: [f32; FDN] = [0.0297, 0.0371, 0.0411, 0.0437, 0.0533, 0.0617, 0.0679, 0.0731];

pub struct HybridVerb {
    sr: f32,
    conv: Convolver,
    character: i32,
    size: f32,
    lines: Vec<DelayLine>,
    lens: [f32; FDN],
    fb: [f32; FDN],
    damp: [OnePole; FDN],
    damp_f: f32,
    pre: DelayLine,
    pre_d: f32,
    shift: Shifter,
    shimmer: f32,
    early: f32,
    late: f32,
    dry: f32,
    wet: f32,
    mod_ph: f32,
    decay: f32,
    shim_lp: OnePole,
}

impl HybridVerb {
    pub fn new(sr: f32) -> Self {
        let parts = ((sr * 0.16) as usize).div_ceil(CONV_B);
        let mut v = Self {
            sr,
            conv: Convolver::new(parts),
            character: -1,
            size: -1.0,
            lines: (0..FDN).map(|_| DelayLine::new((sr * 0.2) as usize)).collect(),
            lens: [1000.0; FDN],
            fb: [0.0; FDN],
            damp: [OnePole::default(); FDN],
            damp_f: 8000.0,
            pre: DelayLine::new((sr * 0.25) as usize),
            pre_d: 1.0,
            shift: Shifter::new(sr),
            shimmer: 0.0,
            early: 0.5,
            late: 0.5,
            dry: 1.0,
            wet: 0.0,
            mod_ph: 0.0,
            decay: 2.0,
            shim_lp: OnePole::default(),
        };
        v.shim_lp.set_lowpass(6000.0, sr);
        v
    }

    /// Synthesizes a decorrelated stereo early-reflection impulse for the room character.
    fn build_ir(&mut self) {
        let n = self.conv.fdl.len() * CONV_B;
        let (density, t60, bright, gap): (f32, f32, f32, f32) = match self.character {
            0 => (0.02, 0.10, 0.6, 0.004),
            1 => (0.008, 0.16, 0.5, 0.012),
            2 => (0.2, 0.12, 0.85, 0.0005),
            3 => (0.05, 0.13, 0.4, 0.007),
            _ => (0.004, 0.16, 0.45, 0.02),
        };
        let mut ir = [vec![0.0f32; n], vec![0.0f32; n]];
        for (ch, buf) in ir.iter_mut().enumerate() {
            let mut rng = Rng::new(7 + ch as u32 * 31 + self.character as u32 * 101);
            let mut lp = 0.0f32;
            let start = (gap * self.size.max(0.3) * self.sr) as usize;
            for (i, v) in buf.iter_mut().enumerate().skip(start) {
                let t = (i - start) as f32 / self.sr;
                let env = (-6.9 * t / (t60 * self.size.max(0.3))).exp();
                let r = rng.next();
                let tap = if (r * 0.5 + 0.5) < density.max(0.002) * (1.0 + 10.0 * t) { rng.next() * 3.0 } else { rng.next() * 0.08 };
                lp += (tap - lp) * bright;
                *v = lp * env;
            }
            let norm = buf.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
            for v in buf.iter_mut() {
                *v *= 0.5 / norm;
            }
        }
        self.conv.set_ir([&ir[0], &ir[1]]);
    }

    fn update_fdn(&mut self) {
        for k in 0..FDN {
            self.lens[k] = FDN_LEN[k] * self.size * self.sr;
            self.fb[k] = 10f32.powf(-3.0 * self.lens[k] / (self.decay * self.sr));
            self.damp[k].set_lowpass(self.damp_f, self.sr);
        }
    }
}

impl Effect for HybridVerb {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        let character = p[0].round() as i32;
        let size = p[1];
        let rebuild = character != self.character || (size - self.size).abs() > 0.02;
        self.character = character;
        self.size = size;
        if rebuild {
            self.build_ir();
        }
        self.decay = p[2];
        self.damp_f = p[3];
        self.pre_d = (p[4] * self.sr).max(1.0);
        self.early = (1.0 - p[5]).sqrt();
        self.late = p[5].sqrt();
        self.shimmer = p[6];
        (self.dry, self.wet) = mix_gains(p[7]);
        self.update_fdn();
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let h = 1.0 / (FDN as f32).sqrt();
        for i in 0..l.len() {
            let x = (l[i] + r[i]) * 0.5;
            self.pre.write(x);
            let xin = self.pre.read(self.pre_d);
            let (el, er) = self.conv.tick(xin);
            self.mod_ph = (self.mod_ph + 0.6 / self.sr).fract();
            let m = (self.mod_ph * 2.0 * PI).sin() * 6.0;
            let mut outs = [0.0f32; FDN];
            for k in 0..FDN {
                let md = if k % 2 == 0 { m } else { -m };
                outs[k] = self.damp[k].lp(self.lines[k].read(self.lens[k] + md)) * self.fb[k];
            }
            // Fast Hadamard mix.
            let mut y = outs;
            let mut len = 1;
            while len < FDN {
                for s in (0..FDN).step_by(len * 2) {
                    for j in s..s + len {
                        let (a, b) = (y[j], y[j + len]);
                        y[j] = a + b;
                        y[j + len] = a - b;
                    }
                }
                len *= 2;
            }
            let tail_l = outs[0] + outs[2] + outs[4] + outs[6];
            let tail_r = outs[1] + outs[3] + outs[5] + outs[7];
            let shim = if self.shimmer > 0.001 { self.shim_lp.lp(self.shift.tick((tail_l + tail_r) * 0.25)) * self.shimmer } else { 0.0 };
            let feed = (el + er) * 0.5 + xin * 0.3 + shim;
            for k in 0..FDN {
                let v = y[k] * h + feed * if k % 2 == 0 { 1.0 } else { -1.0 } * 0.35;
                self.lines[k].write(soft_clip(v));
            }
            let wl = el * self.early * 2.0 + tail_l * self.late * 0.5;
            let wr = er * self.early * 2.0 + tail_r * self.late * 0.5;
            l[i] = l[i] * self.dry + wl * self.wet;
            r[i] = r[i] * self.dry + wr * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Transient Shaper.

pub struct TransientShaper {
    /// Audio is delayed slightly so gain changes land before the onset they react to.
    look: [DelayLine; 2],
    look_n: f32,
    attack: f32,
    sustain: f32,
    fast: f32,
    slow: f32,
    tail: f32,
    gain_db: f32,
    out: f32,
    dry: f32,
    wet: f32,
    coefs: [f32; 6],
}

impl TransientShaper {
    pub fn new(sr: f32) -> Self {
        Self {
            look: [DelayLine::new((sr * 0.004) as usize), DelayLine::new((sr * 0.004) as usize)],
            look_n: (sr * 0.0015).round(),
            attack: 0.0,
            sustain: 0.0,
            fast: 0.0,
            slow: 0.0,
            tail: 0.0,
            gain_db: 0.0,
            out: 1.0,
            dry: 0.0,
            wet: 1.0,
            coefs: [0.0; 6],
        }
    }
}

impl Effect for TransientShaper {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.attack = p[0];
        self.sustain = p[1];
        self.out = db_to_gain(p[2]);
        (self.dry, self.wet) = mix_gains(p[3]);
        let s = p[4].clamp(0.25, 4.0);
        self.coefs = [
            one_pole_coef(0.0005 * s, ctx.sr),
            one_pole_coef(0.02 * s, ctx.sr),
            one_pole_coef(0.015 * s, ctx.sr),
            one_pole_coef(0.15 * s, ctx.sr),
            one_pole_coef(0.03 * s, ctx.sr),
            one_pole_coef(0.5 * s, ctx.sr),
        ];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let up = one_pole_coef(0.002, ctx.sr);
        let down = one_pole_coef(0.0002, ctx.sr);
        let c = self.coefs;
        let follow = |env: &mut f32, x: f32, a: f32, r: f32| {
            let k = if x > *env { a } else { r };
            *env = x + k * (*env - x);
        };
        for i in 0..l.len() {
            let x = l[i].abs().max(r[i].abs()) + 1e-6;
            follow(&mut self.fast, x, c[0], c[1]);
            follow(&mut self.slow, x, c[2], c[3]);
            follow(&mut self.tail, x, c[4], c[5]);
            let onset = gain_to_db(self.fast / self.slow.max(1e-6)).max(0.0);
            let decay = gain_to_db(self.tail / self.fast.max(1e-6)).max(0.0);
            // Sustain only acts on falling tails, never across a new onset.
            let decay = if onset > 1.0 { 0.0 } else { decay.min(18.0) };
            let target = (self.attack * onset * 1.5 + self.sustain * decay).clamp(-24.0, 24.0);
            let k = if target < self.gain_db { down } else { up };
            self.gain_db = target + k * (self.gain_db - target);
            let g = db_to_gain(self.gain_db) * self.out;
            self.look[0].write(l[i]);
            self.look[1].write(r[i]);
            let (a, b) = (self.look[0].read(self.look_n), self.look[1].read(self.look_n));
            l[i] = a * self.dry + a * g * self.wet;
            r[i] = b * self.dry + b * g * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Lookahead limiter core shared by the Maximizer and the True Peak Limiter.

struct LimiterCore {
    delay: [Vec<f32>; 2],
    pos: usize,
    look: usize,
    /// Sliding minimum of the required gain over the lookahead window: (index, gain).
    mins: VecDeque<(u64, f32)>,
    /// Box filter over the min-held gain.
    boxbuf: Vec<f32>,
    box_sum: f64,
    held: f32,
    rel: f32,
    t: u64,
    gr: f32,
}

impl LimiterCore {
    fn new(sr: f32) -> Self {
        let max = (sr * 0.011) as usize;
        Self {
            delay: [vec![0.0; max], vec![0.0; max]],
            pos: 0,
            look: 64,
            mins: VecDeque::with_capacity(max + 2),
            boxbuf: vec![1.0; max],
            box_sum: 64.0,
            held: 1.0,
            rel: 0.999,
            t: 0,
            gr: 0.0,
        }
    }
    fn set(&mut self, look_s: f32, release_s: f32, sr: f32) {
        let look = ((look_s * sr) as usize).clamp(8, self.delay[0].len() - 1);
        if look != self.look {
            self.look = look;
            self.boxbuf.fill(1.0);
            self.box_sum = look as f64;
            self.mins.clear();
        }
        self.rel = one_pole_coef(release_s, sr);
    }
    /// Pushes a sample pair with its detected peak; returns the delayed, limited pair.
    fn tick(&mut self, l: f32, r: f32, peak: f32, ceiling: f32) -> (f32, f32) {
        let need = if peak > ceiling { ceiling / peak } else { 1.0 };
        self.t += 1;
        while self.mins.back().is_some_and(|&(_, g)| g >= need) {
            self.mins.pop_back();
        }
        self.mins.push_back((self.t, need));
        while self.mins.front().is_some_and(|&(t, _)| t + self.look as u64 <= self.t) {
            self.mins.pop_front();
        }
        let target = self.mins.front().map(|m| m.1).unwrap_or(1.0);
        self.held = if target < self.held { target } else { target + self.rel * (self.held - target) };
        let bi = (self.t % self.look as u64) as usize;
        self.box_sum += (self.held - self.boxbuf[bi]) as f64;
        self.boxbuf[bi] = self.held;
        let g = ((self.box_sum / self.look as f64) as f32).min(1.0);
        let n = self.delay[0].len();
        self.delay[0][self.pos] = l;
        self.delay[1][self.pos] = r;
        let rp = (self.pos + n - self.look) % n;
        self.pos = (self.pos + 1) % n;
        self.gr = self.gr.max(-gain_to_db(g));
        (self.delay[0][rp] * g, self.delay[1][rp] * g)
    }
}

pub struct Maximizer {
    core: LimiterCore,
    gain: f32,
    ceiling: f32,
    warmth: f32,
    gr: f32,
}

impl Maximizer {
    pub fn new(sr: f32) -> Self {
        Self { core: LimiterCore::new(sr), gain: 1.0, ceiling: 1.0, warmth: 0.0, gr: 0.0 }
    }
}

impl Effect for Maximizer {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.gain = db_to_gain(p[0]);
        self.ceiling = db_to_gain(p[1]);
        self.core.set(0.0015, p[2], ctx.sr);
        self.warmth = p[3];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        self.core.gr = 0.0;
        for i in 0..l.len() {
            let mut a = l[i] * self.gain;
            let mut b = r[i] * self.gain;
            if self.warmth > 0.0 {
                // Gentle pre-saturation shaves peaks before the limiter has to.
                let k = 1.0 + self.warmth * 2.0;
                a = a * (1.0 - self.warmth) + (a * k).tanh() / k.tanh() * self.ceiling * self.warmth;
                b = b * (1.0 - self.warmth) + (b * k).tanh() / k.tanh() * self.ceiling * self.warmth;
            }
            let (x, y) = self.core.tick(a, b, a.abs().max(b.abs()), self.ceiling);
            l[i] = x.clamp(-self.ceiling, self.ceiling);
            r[i] = y.clamp(-self.ceiling, self.ceiling);
        }
        // Hold the meter briefly so short reductions stay visible.
        self.gr = self.core.gr.max(self.gr - 0.25);
    }
    fn reduction(&self) -> f32 {
        self.gr
    }
}

/// ITU-style loudness meter: K-weighting, momentary (400 ms), short-term (3 s) and gated
/// integrated loudness (histogram, no allocation on the audio thread).
struct Loudness {
    kw: [[Biquad; 2]; 2],
    sub: f64,
    sub_n: usize,
    sub_len: usize,
    /// Ring of 100 ms sub-block energies (30 = 3 s).
    ring: [f64; 30],
    ring_pos: usize,
    hist_n: Vec<u32>,
    hist_e: Vec<f64>,
    momentary: f32,
    short: f32,
}

impl Loudness {
    fn new(sr: f32) -> Self {
        let mut kw = [[Biquad::default(); 2]; 2];
        for ch in &mut kw {
            ch[0].high_shelf(1681.0, 4.0, sr);
            ch[1].highpass(38.0, 0.5, sr);
        }
        Self {
            kw,
            sub: 0.0,
            sub_n: 0,
            sub_len: (sr * 0.1) as usize,
            ring: [0.0; 30],
            ring_pos: 0,
            hist_n: vec![0; 800],
            hist_e: vec![0.0; 800],
            momentary: -70.0,
            short: -70.0,
        }
    }
    fn lufs(e: f64) -> f32 {
        if e <= 1e-12 { -99.0 } else { (-0.691 + 10.0 * e.log10()) as f32 }
    }
    fn tick(&mut self, l: f32, r: f32) {
        let a = cascade(&mut self.kw[0], l) as f64;
        let b = cascade(&mut self.kw[1], r) as f64;
        self.sub += a * a + b * b;
        self.sub_n += 1;
        if self.sub_n >= self.sub_len {
            self.ring[self.ring_pos] = self.sub / self.sub_n as f64;
            self.ring_pos = (self.ring_pos + 1) % 30;
            self.sub = 0.0;
            self.sub_n = 0;
            let m: f64 = (1..=4).map(|k| self.ring[(self.ring_pos + 30 - k) % 30]).sum::<f64>() / 4.0;
            let s: f64 = self.ring.iter().sum::<f64>() / 30.0;
            self.momentary = Self::lufs(m);
            self.short = Self::lufs(s);
            // 400 ms gating blocks with 75 % overlap feed the integrated histogram.
            if self.momentary > -70.0 {
                let bin = (((self.momentary + 70.0) * 10.0) as usize).min(799);
                self.hist_n[bin] += 1;
                self.hist_e[bin] += m;
            }
        }
    }
    fn integrated(&self) -> f32 {
        let (n, e) = self.hist_n.iter().zip(&self.hist_e).fold((0u64, 0.0f64), |(n, e), (&c, &x)| (n + c as u64, e + x));
        if n == 0 {
            return -70.0;
        }
        let rel_gate = Self::lufs(e / n as f64) - 10.0;
        let from = (((rel_gate + 70.0) * 10.0).max(0.0) as usize).min(799);
        let (n2, e2) = self.hist_n[from..].iter().zip(&self.hist_e[from..]).fold((0u64, 0.0f64), |(n, e), (&c, &x)| (n + c as u64, e + x));
        if n2 == 0 { -70.0 } else { Self::lufs(e2 / n2 as f64) }
    }
}

/// 4x oversampling interpolator for true-peak detection.
struct TruePeak {
    hist: [[f32; 12]; 2],
    coefs: [[f32; 12]; 4],
}

impl TruePeak {
    fn new() -> Self {
        let mut coefs = [[0.0; 12]; 4];
        for (ph, c) in coefs.iter_mut().enumerate() {
            for (k, v) in c.iter_mut().enumerate() {
                let x = k as f32 - 5.5 - (ph as f32 / 4.0 - 0.5);
                let sinc = if x.abs() < 1e-6 { 1.0 } else { (PI * x).sin() / (PI * x) };
                let w = 0.5 + 0.5 * (PI * x / 6.5).cos();
                *v = sinc * w.max(0.0);
            }
        }
        Self { hist: [[0.0; 12]; 2], coefs }
    }
    fn tick(&mut self, l: f32, r: f32) -> f32 {
        let mut peak = l.abs().max(r.abs());
        for (ch, x) in [l, r].into_iter().enumerate() {
            let h = &mut self.hist[ch];
            h.copy_within(1.., 0);
            h[11] = x;
            for c in &self.coefs {
                let y: f32 = h.iter().zip(c).map(|(a, b)| a * b).sum();
                peak = peak.max(y.abs());
            }
        }
        peak
    }
}

pub struct TruePeakLimiter {
    core: LimiterCore,
    tp: TruePeak,
    meter_in: Loudness,
    meter_out: Loudness,
    tp_out: TruePeak,
    ceiling: f32,
    input: f32,
    gr: f32,
    max_tp: f32,
    /// momentary, short-term, integrated (output), true peak (dBTP), input short-term, GR.
    disp: [f32; 6],
}

impl TruePeakLimiter {
    pub fn new(sr: f32) -> Self {
        Self {
            core: LimiterCore::new(sr),
            tp: TruePeak::new(),
            meter_in: Loudness::new(sr),
            meter_out: Loudness::new(sr),
            tp_out: TruePeak::new(),
            ceiling: 0.9,
            input: 1.0,
            gr: 0.0,
            max_tp: 0.0,
            disp: [-70.0; 6],
        }
    }
}

impl Effect for TruePeakLimiter {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.input = db_to_gain(p[0]);
        self.ceiling = db_to_gain(p[1]);
        self.core.set(p[3] / 1000.0, p[2], ctx.sr);
        if p[4] > 0.5 {
            // Reset button pressed: clear integrated loudness and the peak hold.
            self.meter_out.hist_n.fill(0);
            self.meter_out.hist_e.fill(0.0);
            self.max_tp = 0.0;
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        self.core.gr = 0.0;
        for i in 0..l.len() {
            let a = l[i] * self.input;
            let b = r[i] * self.input;
            self.meter_in.tick(a, b);
            let peak = self.tp.tick(a, b);
            let (x, y) = self.core.tick(a, b, peak * 1.02, self.ceiling);
            let x = x.clamp(-self.ceiling, self.ceiling);
            let y = y.clamp(-self.ceiling, self.ceiling);
            self.meter_out.tick(x, y);
            self.max_tp = self.max_tp.max(self.tp_out.tick(x, y));
            l[i] = x;
            r[i] = y;
        }
        self.gr = self.core.gr.max(self.gr - 0.25);
        self.disp = [
            self.meter_out.momentary,
            self.meter_out.short,
            self.meter_out.integrated(),
            gain_to_db(self.max_tp),
            self.meter_in.short,
            self.gr,
        ];
    }
    fn reduction(&self) -> f32 {
        self.gr
    }
    fn display(&self) -> Option<&[f32]> {
        Some(&self.disp)
    }
}

// ---------------------------------------------------------------------------------------------
// Stereo Imager: three-band mid/side width with mono bass and a correlation meter.

#[derive(Default)]
pub struct Imager {
    lp1: [[Biquad; 2]; 2],
    hp1: [[Biquad; 2]; 2],
    lp2: [[Biquad; 2]; 2],
    hp2: [[Biquad; 2]; 2],
    width: [f32; 3],
    out: f32,
    corr_lr: f32,
    corr_ll: f32,
    corr_rr: f32,
    disp: [f32; 2],
}

impl Effect for Imager {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.width = [p[0], p[1], p[2]];
        let (f1, f2) = (p[3], p[4].max(p[3] * 1.5));
        for ch in 0..2 {
            for k in 0..2 {
                let o = [self.lp1[ch][k], self.hp1[ch][k], self.lp2[ch][k], self.hp2[ch][k]];
                self.lp1[ch][k].lowpass(f1, 0.707, ctx.sr);
                self.hp1[ch][k].highpass(f1, 0.707, ctx.sr);
                self.lp2[ch][k].lowpass(f2, 0.707, ctx.sr);
                self.hp2[ch][k].highpass(f2, 0.707, ctx.sr);
                self.lp1[ch][k].copy_state(&o[0]);
                self.hp1[ch][k].copy_state(&o[1]);
                self.lp2[ch][k].copy_state(&o[2]);
                self.hp2[ch][k].copy_state(&o[3]);
            }
        }
        self.out = db_to_gain(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let c = one_pole_coef(0.3, ctx.sr);
        for i in 0..l.len() {
            let mut bands = [[0.0f32; 2]; 3];
            for (ch, x) in [l[i], r[i]].into_iter().enumerate() {
                let lo = cascade(&mut self.lp1[ch], x);
                let rest = cascade(&mut self.hp1[ch], x);
                let mid = cascade(&mut self.lp2[ch], rest);
                let hi = cascade(&mut self.hp2[ch], rest);
                bands[0][ch] = lo;
                bands[1][ch] = mid;
                bands[2][ch] = hi;
            }
            let (mut a, mut b) = (0.0, 0.0);
            for (k, band) in bands.iter().enumerate() {
                let m = (band[0] + band[1]) * 0.5;
                let s = (band[0] - band[1]) * 0.5 * self.width[k];
                a += m + s;
                b += m - s;
            }
            l[i] = a * self.out;
            r[i] = b * self.out;
            self.corr_lr = l[i] * r[i] + c * (self.corr_lr - l[i] * r[i]);
            self.corr_ll = l[i] * l[i] + c * (self.corr_ll - l[i] * l[i]);
            self.corr_rr = r[i] * r[i] + c * (self.corr_rr - r[i] * r[i]);
        }
        let corr = self.corr_lr / (self.corr_ll * self.corr_rr).sqrt().max(1e-9);
        self.disp = [corr.clamp(-1.0, 1.0), (self.corr_ll + self.corr_rr).sqrt()];
    }
    fn display(&self) -> Option<&[f32]> {
        Some(&self.disp)
    }
}
