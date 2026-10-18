//! Low-level DSP building blocks.

use std::f32::consts::PI;

pub const MAX_BLOCK: usize = 64;

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 1e-6 {
        -120.0
    } else {
        20.0 * g.log10()
    }
}

/// Fader position (0..1) to linear gain. 0.85 is unity, 1.0 is about +3 dB.
#[inline]
pub fn fader_to_gain(pos: f32) -> f32 {
    (pos / 0.85).max(0.0).powf(2.2)
}

#[inline]
pub fn midi_to_freq(p: f32) -> f32 {
    440.0 * 2f32.powf((p - 69.0) / 12.0)
}

/// Coefficient for a one-pole smoother reaching ~63% in `time` seconds.
#[inline]
pub fn one_pole_coef(time: f32, sr: f32) -> f32 {
    if time <= 0.0 {
        0.0
    } else {
        (-1.0 / (time * sr)).exp()
    }
}

/// Multiplier for an exponential decay that falls 60 dB over `t60` seconds.
#[inline]
pub fn decay_mult(t60: f32, sr: f32) -> f32 {
    (-6.9 / (t60.max(0.0005) * sr)).exp()
}

#[inline]
pub fn soft_clip(x: f32) -> f32 {
    // Cheap tanh approximation, accurate enough for saturation.
    if x < -3.0 {
        -1.0
    } else if x > 3.0 {
        1.0
    } else {
        x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
    }
}

#[derive(Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Saw,
    Square,
    Triangle,
    Sine,
}

impl Wave {
    pub fn from_index(i: f32) -> Self {
        match i.round() as i32 {
            1 => Self::Square,
            2 => Self::Triangle,
            3 => Self::Sine,
            _ => Self::Saw,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Osc {
    pub phase: f32,
    tri: f32,
}

impl Osc {
    #[inline]
    pub fn next(&mut self, wave: Wave, freq: f32, sr: f32) -> f32 {
        let dt = (freq / sr).clamp(0.0, 0.49);
        let t = self.phase;
        let v = match wave {
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Square => {
                let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                v -= poly_blep((t + 0.5) % 1.0, dt);
                v
            }
            Wave::Triangle => {
                // Leaky-integrated band-limited square.
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t + 0.5) % 1.0, dt);
                self.tri = dt * 4.0 * sq + (1.0 - dt * 0.05) * self.tri;
                self.tri
            }
            Wave::Sine => (t * 2.0 * PI).sin(),
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        v
    }

    #[inline]
    pub fn sine(&mut self, freq: f32, sr: f32) -> f32 {
        let v = (self.phase * 2.0 * PI).sin();
        self.phase += freq / sr;
        self.phase -= self.phase.floor();
        v
    }

    /// Naive square, used for metallic drum partials where aliasing is part of the sound.
    #[inline]
    pub fn naive_square(&mut self, freq: f32, sr: f32) -> f32 {
        let v = if self.phase < 0.5 { 1.0 } else { -1.0 };
        self.phase += freq / sr;
        self.phase -= self.phase.floor();
        v
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterMode {
    Low,
    High,
    /// Band-pass with gain Q at the centre (resonant).
    Band,
    /// Band-pass with unity gain at the centre - safe inside feedback loops.
    BandNorm,
}

/// Topology-preserving state variable filter (Cytomic / Simper).
#[derive(Clone, Copy)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Default for Svf {
    fn default() -> Self {
        let mut s = Self { ic1: 0.0, ic2: 0.0, a1: 0.0, a2: 0.0, a3: 0.0, k: 1.0 };
        s.set(1000.0, 0.7, 48000.0);
        s
    }
}

impl Svf {
    /// `q` is the filter Q (0.5 = no resonance, ~30 = screaming).
    #[inline]
    pub fn set(&mut self, cutoff: f32, q: f32, sr: f32) {
        let fc = cutoff.clamp(10.0, sr * 0.48);
        let g = (PI * fc / sr).tan();
        self.k = 1.0 / q.max(0.3);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    #[inline]
    pub fn process(&mut self, v0: f32, mode: FilterMode) -> f32 {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            FilterMode::Low => v2,
            FilterMode::Band => v1,
            FilterMode::BandNorm => v1 * self.k,
            FilterMode::High => v0 - self.k * v1 - v2,
        }
    }
}

/// Resonance 0..1 -> Q.
#[inline]
pub fn res_to_q(res: f32) -> f32 {
    0.5 + res * res * 24.0
}

#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn set_coefs(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn low_shelf(&mut self, f: f32, db: f32, sr: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / 2.0 * 2f32.sqrt();
        let sa = 2.0 * a.sqrt() * alpha;
        self.set_coefs(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        );
    }

    pub fn high_shelf(&mut self, f: f32, db: f32, sr: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / 2.0 * 2f32.sqrt();
        let sa = 2.0 * a.sqrt() * alpha;
        self.set_coefs(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        );
    }

    pub fn peaking(&mut self, f: f32, q: f32, db: f32, sr: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        self.set_coefs(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a);
    }

    pub fn lowpass(&mut self, f: f32, q: f32, sr: f32) {
        let w = 2.0 * PI * f.min(sr * 0.49) / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.05));
        self.set_coefs((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    pub fn highpass(&mut self, f: f32, q: f32, sr: f32) {
        let w = 2.0 * PI * f.min(sr * 0.49) / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.05));
        self.set_coefs((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    /// Band-pass with 0 dB peak gain.
    pub fn bandpass(&mut self, f: f32, q: f32, sr: f32) {
        let w = 2.0 * PI * f.min(sr * 0.49) / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.05));
        self.set_coefs(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    pub fn notch(&mut self, f: f32, q: f32, sr: f32) {
        let w = 2.0 * PI * f.min(sr * 0.49) / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.05));
        self.set_coefs(1.0, -2.0 * c, 1.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    /// Magnitude response at `f` (linear).
    pub fn magnitude(&self, f: f32, sr: f32) -> f32 {
        let w = 2.0 * PI * f / sr;
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = self.b1 * s1 + self.b2 * s2;
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = self.a1 * s1 + self.a2 * s2;
        ((nr * nr + ni * ni) / (dr * dr + di * di).max(1e-12)).sqrt()
    }

    pub fn copy_state(&mut self, other: &Biquad) {
        self.z1 = other.z1;
        self.z2 = other.z2;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

#[derive(Clone, Copy, Default)]
pub struct OnePole {
    z: f32,
    a: f32,
}

impl OnePole {
    pub fn set_lowpass(&mut self, f: f32, sr: f32) {
        self.a = (-2.0 * PI * f.min(sr * 0.49) / sr).exp();
    }
    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        self.z = x + self.a * (self.z - x);
        self.z
    }
    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        x - self.lp(x)
    }
}

/// Parameter smoother.
#[derive(Clone, Copy)]
pub struct Smooth {
    pub value: f32,
    pub target: f32,
    coef: f32,
}

impl Smooth {
    pub fn new(v: f32, time: f32, sr: f32) -> Self {
        Self { value: v, target: v, coef: one_pole_coef(time, sr) }
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value = self.target + self.coef * (self.value - self.target);
        self.value
    }
}

pub struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        Self { buf: vec![0.0; max_samples.max(4)], pos: 0 }
    }
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
    }
    /// Read `delay` samples behind the most recent write (fractional, linear interpolation).
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 2) as f32);
        let di = d.floor() as usize;
        let frac = d - di as f32;
        let i0 = (self.pos + len - di) % len;
        let i1 = (i0 + len - 1) % len;
        self.buf[i0] * (1.0 - frac) + self.buf[i1] * frac
    }
}

/// Linear-attack, exponential decay/release ADSR.
#[derive(Clone, Copy)]
pub struct Adsr {
    pub level: f32,
    stage: Stage,
    attack_inc: f32,
    decay_mul: f32,
    release_mul: f32,
    sustain: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

impl Default for Adsr {
    fn default() -> Self {
        Self { level: 0.0, stage: Stage::Idle, attack_inc: 1.0, decay_mul: 0.999, release_mul: 0.999, sustain: 1.0 }
    }
}

impl Adsr {
    pub fn set(&mut self, a: f32, d: f32, s: f32, r: f32, sr: f32) {
        self.attack_inc = 1.0 / (a.max(0.0005) * sr);
        self.decay_mul = decay_mult(d, sr);
        self.release_mul = decay_mult(r, sr);
        self.sustain = s;
    }
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }
    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }
    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }
    pub fn is_released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Idle)
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_mul;
            }
            Stage::Release => {
                self.level *= self.release_mul;
                if self.level < 0.0001 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// Balance-style pan: center keeps both channels at unity.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

/// In-place radix-2 complex FFT. `re.len()` must be a power of two.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// IIR Hilbert transformer (two all-pass chains with ~90° phase difference).
#[derive(Clone, Copy, Default)]
pub struct Hilbert {
    s: [[f32; 4]; 8],
    delay: f32,
}

const HILBERT_A: [f32; 4] = [0.6923878, 0.9360654322959, 0.9882295226860, 0.9987488452737];
const HILBERT_B: [f32; 4] = [0.4021921162426, 0.8561710882420, 0.9722909545651, 0.9952884791278];

impl Hilbert {
    /// Returns (in-phase, quadrature).
    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let mut a = x;
        for (i, k) in HILBERT_A.iter().enumerate() {
            let st = &mut self.s[i];
            let y = k * k * (a + st[1]) - st[0];
            st[0] = st[2];
            st[2] = a;
            st[1] = st[3];
            st[3] = y;
            a = y;
        }
        let mut b = x;
        for (i, k) in HILBERT_B.iter().enumerate() {
            let st = &mut self.s[i + 4];
            let y = k * k * (b + st[1]) - st[0];
            st[0] = st[2];
            st[2] = b;
            st[1] = st[3];
            st[3] = y;
            b = y;
        }
        let i_out = self.delay;
        self.delay = a;
        (i_out, b)
    }
}
