//! Audio effect devices.

use super::dsp::*;
use super::effects2 as e2;
use crate::model::DeviceKind;
use crate::params::division_beats;

pub struct Ctx {
    pub sr: f32,
    pub bpm: f32,
    /// Song position in beats at the start of the block (valid while playing).
    pub beat: f64,
    pub playing: bool,
}

pub trait Effect: Send {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx);
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx);
    fn on_kick(&mut self) {}
    /// Gain reduction in dB for display (compressor/limiter).
    fn reduction(&self) -> f32 {
        0.0
    }
    /// Analysis data for devices with a display (Analyzer, Tuner).
    fn display(&self) -> Option<&[f32]> {
        None
    }
}

pub fn create(kind: DeviceKind, sr: f32) -> Box<dyn Effect> {
    match kind {
        DeviceKind::FilterSweep => Box::new(FilterSweep::default()),
        DeviceKind::DjEq => Box::new(DjEq::default()),
        DeviceKind::Saturator => Box::new(Saturator::default()),
        DeviceKind::Echo => Box::new(Echo::new(sr)),
        DeviceKind::Reverb => Box::new(Reverb::new(sr)),
        DeviceKind::Compressor => Box::new(Compressor::default()),
        DeviceKind::Limiter => Box::new(Limiter::default()),
        DeviceKind::Sidechain => Box::new(Sidechain::default()),
        DeviceKind::Bitcrusher => Box::new(Bitcrusher::default()),
        DeviceKind::Utility => Box::new(Utility::default()),
        DeviceKind::PanLfo => Box::new(e2::PanLfo::default()),
        DeviceKind::Stutter => Box::new(e2::Stutter::new(sr)),
        DeviceKind::Chorus => Box::new(e2::Chorus::new(sr)),
        DeviceKind::BodyResonator => Box::new(e2::BodyResonator::default()),
        DeviceKind::DrumShaper => Box::new(e2::DrumShaper::default()),
        DeviceKind::TubeDrive => Box::new(e2::TubeDrive::default()),
        DeviceKind::Eq8 => Box::new(e2::Eq8::default()),
        DeviceKind::Degrader => Box::new(e2::Degrader::new()),
        DeviceKind::TripleDelay => Box::new(e2::TripleDelay::new(sr)),
        DeviceKind::Flanger => Box::new(e2::Flanger::new(sr)),
        DeviceKind::FreqShifter => Box::new(e2::FreqShifter::default()),
        DeviceKind::Gate => Box::new(e2::Gate::default()),
        DeviceKind::BusCompressor => Box::new(e2::BusComp::default()),
        DeviceKind::GrainDelay => Box::new(e2::GrainDelay::new(sr)),
        DeviceKind::Multiband => Box::new(e2::Multiband::default()),
        DeviceKind::Overdrive => Box::new(e2::Overdrive::default()),
        DeviceKind::Stompbox => Box::new(e2::Stompbox::default()),
        DeviceKind::Phaser => Box::new(e2::Phaser::default()),
        DeviceKind::PingPong => Box::new(e2::PingPong::new(sr)),
        DeviceKind::ChordResonator => Box::new(e2::ChordResonator::new(sr)),
        DeviceKind::StereoDelay => Box::new(e2::StereoDelay::new(sr)),
        DeviceKind::Vinyl => Box::new(e2::Vinyl::default()),
        DeviceKind::Analyzer => Box::new(e2::Analyzer::new(sr)),
        DeviceKind::Tuner => Box::new(e2::Tuner::new(sr)),
        DeviceKind::Vocoder => Box::new(e2::Vocoder::new(sr)),
        DeviceKind::ChannelEq => Box::new(e2::ChannelEq::default()),
    }
}

#[inline]
pub fn mix_gains(m: f32) -> (f32, f32) {
    ((m * std::f32::consts::FRAC_PI_2).cos(), (m * std::f32::consts::FRAC_PI_2).sin())
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct FilterSweep {
    mode: Option<FilterMode>,
    cutoff: f32,
    q: f32,
    lfo_amt: f32,
    rate: f32,
    shape: i32,
    phase: f32,
    f: [[Svf; 2]; 2],
}

impl Effect for FilterSweep {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.mode = Some(match p[0].round() as i32 {
            1 => FilterMode::High,
            2 => FilterMode::Band,
            _ => FilterMode::Low,
        });
        self.cutoff = p[1];
        self.q = res_to_q(p[2]);
        self.lfo_amt = p[3];
        self.rate = p[4];
        self.shape = p[5].round() as i32;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let mode = self.mode.unwrap_or(FilterMode::Low);
        let hz = ctx.bpm / 60.0 / division_beats(self.rate);
        for i in 0..l.len() {
            if i % 16 == 0 {
                let ph = self.phase;
                let lfo = match self.shape {
                    1 => 1.0 - 4.0 * (ph - 0.5).abs(),
                    2 => if ph < 0.5 { 1.0 } else { -1.0 },
                    3 => 2.0 * ph - 1.0,
                    _ => (ph * std::f32::consts::TAU).sin(),
                };
                let fc = self.cutoff * 2f32.powf(lfo * self.lfo_amt * 3.0);
                for ch in &mut self.f {
                    ch[0].set(fc, self.q, ctx.sr);
                    ch[1].set(fc, self.q.min(0.8), ctx.sr);
                }
            }
            self.phase = (self.phase + hz / ctx.sr) % 1.0;
            let a = self.f[0][0].process(l[i], mode);
            l[i] = self.f[0][1].process(a, mode);
            let b = self.f[1][0].process(r[i], mode);
            r[i] = self.f[1][1].process(b, mode);
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct DjEq {
    b: [[Biquad; 3]; 2],
}

impl Effect for DjEq {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        for ch in &mut self.b {
            ch[0].low_shelf(p[3], p[0], ctx.sr);
            ch[1].peaking((p[3] * p[4]).sqrt(), 0.7, p[1], ctx.sr);
            ch[2].high_shelf(p[4], p[2], ctx.sr);
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for (buf, b) in [l, r].into_iter().zip(self.b.iter_mut()) {
            for x in buf.iter_mut() {
                let y = b[0].process(*x);
                let y = b[1].process(y);
                *x = b[2].process(y);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Saturator {
    drive: f32,
    tone: [OnePole; 2],
    dry: f32,
    wet: f32,
    out: f32,
}

impl Effect for Saturator {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.drive = p[0];
        for t in &mut self.tone {
            t.set_lowpass(p[1], ctx.sr);
        }
        (self.dry, self.wet) = mix_gains(p[2]);
        self.out = db_to_gain(p[3]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let pre = 1.0 + self.drive * 12.0;
        let post = 1.0 / (1.0 + self.drive * 2.0);
        for (buf, t) in [l, r].into_iter().zip(self.tone.iter_mut()) {
            for x in buf.iter_mut() {
                let w = t.lp(soft_clip(*x * pre)) * post;
                *x = (*x * self.dry + w * self.wet) * self.out;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

struct Echo {
    dl: DelayLine,
    dr: DelayLine,
    time: Smooth,
    beats: f32,
    feedback: f32,
    pingpong: bool,
    hp: [OnePole; 2],
    lp: [OnePole; 2],
    dry: f32,
    wet: f32,
}

impl Echo {
    fn new(sr: f32) -> Self {
        Self {
            dl: DelayLine::new((sr * 6.0) as usize),
            dr: DelayLine::new((sr * 6.0) as usize),
            time: Smooth::new(sr * 0.3, 0.08, sr),
            beats: 0.75,
            feedback: 0.4,
            pingpong: true,
            hp: Default::default(),
            lp: Default::default(),
            dry: 1.0,
            wet: 0.3,
        }
    }
}

impl Effect for Echo {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.beats = division_beats(p[0]);
        self.feedback = p[1];
        self.pingpong = p[2].round() as i32 == 0;
        for f in &mut self.hp {
            f.set_lowpass(p[3], ctx.sr);
        }
        for f in &mut self.lp {
            f.set_lowpass(p[4], ctx.sr);
        }
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        self.time.target = (self.beats * 60.0 / ctx.bpm * ctx.sr).min(ctx.sr * 5.5);
        for i in 0..l.len() {
            let t = self.time.next();
            let yl = self.dl.read(t);
            let yr = self.dr.read(t);
            let (inl, inr) = (l[i], r[i]);
            let fl = self.lp[0].lp(self.hp[0].hp(yl));
            let fr = self.lp[1].lp(self.hp[1].hp(yr));
            if self.pingpong {
                // Mono input enters the left line; each repeat bounces to the other side.
                self.dl.write((inl + inr) * 0.5 + fr * self.feedback);
                self.dr.write(fl * self.feedback);
            } else {
                self.dl.write(inl + fl * self.feedback);
                self.dr.write(inr + fr * self.feedback);
            }
            l[i] = inl * self.dry + yl * self.wet;
            r[i] = inr * self.dry + yr * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Freeverb-style reverb

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    #[inline]
    fn process(&mut self, x: f32, fb: f32, damp: f32) -> f32 {
        let y = self.buf[self.idx];
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.idx] = x + self.store * fb;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let y = b - x;
        self.buf[self.idx] = x + b * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    pre: DelayLine,
    pre_samples: f32,
    hp: [OnePole; 2],
    fb: f32,
    damp: f32,
    width: f32,
    dry: f32,
    wet: f32,
}

impl Reverb {
    fn new(sr: f32) -> Self {
        let scale = sr / 44100.0;
        let mk = |spread: usize| {
            let combs = COMB_TUNING
                .iter()
                .map(|&t| Comb { buf: vec![0.0; ((t + spread) as f32 * scale) as usize], idx: 0, store: 0.0 })
                .collect();
            let aps = ALLPASS_TUNING
                .iter()
                .map(|&t| Allpass { buf: vec![0.0; ((t + spread) as f32 * scale) as usize], idx: 0 })
                .collect();
            (combs, aps)
        };
        let (cl, al) = mk(0);
        let (cr, ar) = mk(STEREO_SPREAD);
        Self {
            combs: [cl, cr],
            allpasses: [al, ar],
            pre: DelayLine::new((sr * 0.3) as usize),
            pre_samples: 1.0,
            hp: Default::default(),
            fb: 0.84,
            damp: 0.2,
            width: 1.0,
            dry: 1.0,
            wet: 0.3,
        }
    }
}

impl Effect for Reverb {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.fb = 0.7 + p[0] * 0.28;
        self.damp = p[1] * 0.4;
        self.pre_samples = (p[2] / 1000.0 * ctx.sr).max(1.0);
        for f in &mut self.hp {
            f.set_lowpass(p[3], ctx.sr);
        }
        self.width = p[4];
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let w1 = self.wet * (self.width / 2.0 + 0.5);
        let w2 = self.wet * ((1.0 - self.width) / 2.0);
        for i in 0..l.len() {
            self.pre.write((l[i] + r[i]) * 0.5);
            let x = self.pre.read(self.pre_samples);
            let x = self.hp[0].hp(x) * 0.03;
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                let mut acc = 0.0;
                for c in &mut self.combs[ch] {
                    acc += c.process(x, self.fb, self.damp);
                }
                for a in &mut self.allpasses[ch] {
                    acc = a.process(acc);
                }
                out[ch] = acc;
            }
            let (ol, or) = (out[0] * w1 + out[1] * w2, out[1] * w1 + out[0] * w2);
            l[i] = l[i] * self.dry + ol;
            r[i] = r[i] * self.dry + or;
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Compressor {
    thresh: f32,
    ratio: f32,
    att: f32,
    rel: f32,
    makeup: f32,
    env: f32,
    gr: f32,
}

impl Effect for Compressor {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.thresh = p[0];
        self.ratio = p[1];
        self.att = one_pole_coef(p[2], ctx.sr);
        self.rel = one_pole_coef(p[3], ctx.sr);
        self.makeup = db_to_gain(p[4]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let knee = 6.0;
        let mut max_gr: f32 = 0.0;
        for i in 0..l.len() {
            let peak = l[i].abs().max(r[i].abs());
            let c = if peak > self.env { self.att } else { self.rel };
            self.env = peak + c * (self.env - peak);
            let lvl = gain_to_db(self.env);
            let over = lvl - self.thresh;
            let gr = if over <= -knee / 2.0 {
                0.0
            } else if over >= knee / 2.0 {
                over * (1.0 - 1.0 / self.ratio)
            } else {
                let x = over + knee / 2.0;
                (1.0 - 1.0 / self.ratio) * x * x / (2.0 * knee)
            };
            max_gr = max_gr.max(gr);
            let g = db_to_gain(-gr) * self.makeup;
            l[i] *= g;
            r[i] *= g;
        }
        self.gr = max_gr;
    }
    fn reduction(&self) -> f32 {
        self.gr
    }
}

// ---------------------------------------------------------------------------------------------

struct Limiter {
    gain: f32,
    ceiling: f32,
    rel: f32,
    env: f32,
    gr: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self { gain: 1.0, ceiling: 1.0, rel: 0.999, env: 1.0, gr: 0.0 }
    }
}

impl Effect for Limiter {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.gain = db_to_gain(p[0]);
        self.ceiling = db_to_gain(p[1]);
        self.rel = one_pole_coef(p[2], ctx.sr);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let mut min_g: f32 = 1.0;
        for i in 0..l.len() {
            let a = l[i] * self.gain;
            let b = r[i] * self.gain;
            let peak = a.abs().max(b.abs());
            let need = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };
            // Instant attack, smooth release.
            self.env = if need < self.env { need } else { need + self.rel * (self.env - need) };
            min_g = min_g.min(self.env);
            l[i] = (a * self.env).clamp(-self.ceiling, self.ceiling);
            r[i] = (b * self.env).clamp(-self.ceiling, self.ceiling);
        }
        self.gr = -gain_to_db(min_g);
    }
    fn reduction(&self) -> f32 {
        self.gr
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Sidechain {
    amount: f32,
    release: f32,
    env: f32,
    target: f32,
    gr: f32,
}

impl Effect for Sidechain {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.amount = p[0];
        self.release = p[1];
    }
    fn on_kick(&mut self) {
        self.target = 1.0;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
        let att = one_pole_coef(0.003, ctx.sr);
        let rel = decay_mult(self.release * 1.5, ctx.sr);
        for i in 0..l.len() {
            if self.target > 0.0 {
                self.env = self.target + att * (self.env - self.target);
                if self.env > 0.98 {
                    self.target = 0.0;
                }
            } else {
                self.env *= rel;
            }
            let g = 1.0 - self.amount * self.env;
            l[i] *= g;
            r[i] *= g;
        }
        self.gr = -gain_to_db(1.0 - self.amount * self.env);
    }
    fn reduction(&self) -> f32 {
        self.gr
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Bitcrusher {
    steps: f32,
    down: f32,
    counter: f32,
    hold: [f32; 2],
    dry: f32,
    wet: f32,
}

impl Effect for Bitcrusher {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.steps = 2f32.powf(p[0]) / 2.0;
        self.down = p[1];
        (self.dry, self.wet) = mix_gains(p[2]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for i in 0..l.len() {
            self.counter += 1.0;
            if self.counter >= self.down {
                self.counter -= self.down;
                self.hold[0] = (l[i] * self.steps).round() / self.steps;
                self.hold[1] = (r[i] * self.steps).round() / self.steps;
            }
            l[i] = l[i] * self.dry + self.hold[0] * self.wet;
            r[i] = r[i] * self.dry + self.hold[1] * self.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Utility {
    gain: f32,
    pan: (f32, f32),
    width: f32,
    hp: [Svf; 2],
    hp_on: bool,
}

impl Effect for Utility {
    fn set_params(&mut self, p: &[f32], ctx: &Ctx) {
        self.gain = db_to_gain(p[0]);
        self.pan = pan_gains(p[1]);
        self.width = p[2];
        self.hp_on = p[3] > 12.0;
        for f in &mut self.hp {
            f.set(p[3], 0.7, ctx.sr);
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        for i in 0..l.len() {
            let (mut a, mut b) = (l[i], r[i]);
            if self.hp_on {
                a = self.hp[0].process(a, FilterMode::High);
                b = self.hp[1].process(b, FilterMode::High);
            }
            let m = (a + b) * 0.5;
            let s = (a - b) * 0.5 * self.width;
            l[i] = (m + s) * self.gain * self.pan.0;
            r[i] = (m - s) * self.gain * self.pan.1;
        }
    }
}
