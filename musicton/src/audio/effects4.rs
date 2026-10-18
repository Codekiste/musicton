//! Fourth batch of audio effects: binaural 3D panner and real-time pitch correction.

use super::dsp::*;
use super::effects::{mix_gains, Ctx, Effect};
use crate::model::SCALES;
use std::f32::consts::PI;

// ---------------------------------------------------------------------------------------------
// Space Panner: binaural placement from interaural time and level differences, head shadow,
// a pinna notch for elevation, distance attenuation, air absorption and early reflections.

const HEAD_RADIUS: f32 = 0.0875;
const SPEED_OF_SOUND: f32 = 343.0;

pub struct SpacePanner {
    sr: f32,
    delay: [DelayLine; 2],
    shadow: [OnePole; 2],
    air: OnePole,
    pinna: [Biquad; 2],
    rear: [Biquad; 2],
    refl: DelayLine,
    params: [f32; 6],
    az: Smooth,
    el: Smooth,
    dist: Smooth,
    orbit: f32,
    counter: u32,
    dry: f32,
    wet: f32,
}

impl SpacePanner {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            delay: [DelayLine::new((sr * 0.003) as usize), DelayLine::new((sr * 0.003) as usize)],
            shadow: [OnePole::default(); 2],
            air: OnePole::default(),
            pinna: [Biquad::default(); 2],
            rear: [Biquad::default(); 2],
            refl: DelayLine::new((sr * 0.12) as usize),
            params: [0.0, 0.0, 1.0, 0.0, 0.3, 1.0],
            az: Smooth::new(0.0, 0.02, sr),
            el: Smooth::new(0.0, 0.02, sr),
            dist: Smooth::new(1.0, 0.05, sr),
            orbit: 0.0,
            counter: 0,
            dry: 0.0,
            wet: 1.0,
        }
    }

    /// Per-block filter update for the current position.
    fn update(&mut self, az_deg: f32, el_deg: f32, dist: f32) {
        let sr = self.sr;
        let az = az_deg.to_radians();
        let s = az.sin();
        // Head shadow: the far ear loses highs the further the source is to the side.
        let near_fc = 20000.0;
        let far_fc = 20000.0 - 18000.0 * s.abs().powf(0.7);
        let (lf, rf) = if s >= 0.0 { (far_fc, near_fc) } else { (near_fc, far_fc) };
        self.shadow[0].set_lowpass(lf.max(1200.0), sr);
        self.shadow[1].set_lowpass(rf.max(1200.0), sr);
        // Pinna notch moves up with elevation; sources above get a little air.
        let notch = 6500.0 + el_deg.clamp(-45.0, 90.0) / 90.0 * 4000.0;
        let behind = az.cos() < 0.0;
        for ch in 0..2 {
            let o = self.pinna[ch];
            self.pinna[ch].peaking(notch, 3.0, -9.0 + el_deg.max(0.0) / 90.0 * 4.0, sr);
            self.pinna[ch].copy_state(&o);
            let o = self.rear[ch];
            self.rear[ch].high_shelf(3500.0, if behind { -5.0 * (-az.cos()) } else { 1.5 * el_deg.max(0.0) / 90.0 }, sr);
            self.rear[ch].copy_state(&o);
        }
        self.air.set_lowpass((20000.0 / (1.0 + dist * 0.15)).max(2500.0), sr);
    }
}

impl Effect for SpacePanner {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        for (k, v) in self.params.iter_mut().enumerate() {
            *v = p[k];
        }
        (self.dry, self.wet) = mix_gains(p[5]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let n = l.len();
        let [az0, el, dist, orbit_rate, room, _] = self.params;
        // Orbit (degrees per second) rotates the source around the head.
        self.orbit = if orbit_rate == 0.0 { 0.0 } else { (self.orbit + orbit_rate * n as f32 / self.sr).rem_euclid(360.0) };
        let az_target = (az0 + self.orbit + 180.0).rem_euclid(360.0) - 180.0;
        // Take the short way round so the smoother never sweeps through the front.
        if (az_target - self.az.value).abs() > 180.0 {
            self.az.value += if az_target > self.az.value { 360.0 } else { -360.0 };
        }
        self.az.target = az_target;
        self.el.target = el;
        self.dist.target = dist.max(0.2);
        let azv = self.az.value;
        let elv = self.el.value;
        let dv = self.dist.value;
        if self.counter % 2 == 0 {
            self.update(azv, elv, dv);
        }
        self.counter = self.counter.wrapping_add(1);
        let gain = (1.0 / dv).min(2.0);
        let refl_amt = room * (dv / (dv + 2.0));
        for i in 0..n {
            let az = self.az.next().to_radians();
            self.el.next();
            self.dist.next();
            // Woodworth ITD (seconds), split so the near ear is undelayed.
            let theta = az.sin().asin();
            let itd = HEAD_RADIUS / SPEED_OF_SOUND * (theta.abs() + theta.abs().sin()) * self.sr;
            let (dl, dr) = if az.sin() >= 0.0 { (itd, 0.0) } else { (0.0, itd) };
            let x = self.air.lp((l[i] + r[i]) * 0.5) * gain;
            self.delay[0].write(x);
            self.delay[1].write(x);
            self.refl.write(x);
            let mut a = self.delay[0].read(1.0 + dl);
            let mut b = self.delay[1].read(1.0 + dr);
            // Interaural level difference on top of the shadow filter.
            let ild = 1.0 - 0.45 * az.sin().abs();
            if az.sin() >= 0.0 {
                a *= ild;
            } else {
                b *= ild;
            }
            a = self.rear[0].process(self.pinna[0].process(self.shadow[0].lp(a)));
            b = self.rear[1].process(self.pinna[1].process(self.shadow[1].lp(b)));
            // Early reflections: decorrelated taps per ear.
            if refl_amt > 0.001 {
                let ms = self.sr / 1000.0;
                let el_ = self.refl.read(11.0 * ms) * 0.5 + self.refl.read(23.0 * ms) * 0.35 + self.refl.read(41.0 * ms) * 0.25;
                let er_ = self.refl.read(13.0 * ms) * 0.5 + self.refl.read(29.0 * ms) * 0.35 + self.refl.read(37.0 * ms) * 0.25;
                a += el_ * refl_amt;
                b += er_ * refl_amt;
            }
            l[i] = l[i] * self.dry + a * self.wet;
            r[i] = r[i] * self.dry + b * self.wet;
        }
        let _ = PI;
    }
}

// ---------------------------------------------------------------------------------------------
// Pitch Fix: detects the pitch of a monophonic voice and shifts it to the nearest note of a key.

pub struct PitchFix {
    sr: f32,
    /// Decimated (sr/4) analysis buffer.
    dec: Vec<f32>,
    dec_w: usize,
    dec_acc: f32,
    dec_n: usize,
    hop: usize,
    lin: Vec<f32>,
    lp: Biquad,
    // Shifter.
    line: [DelayLine; 2],
    phase: f32,
    win: f32,
    ratio: f32,
    cur_shift: f32,
    detected: f32,
    key: u8,
    scale: usize,
    speed: f32,
    amount: f32,
    lo: f32,
    hi: f32,
    dry: f32,
    wet: f32,
    disp: [f32; 3],
}

impl PitchFix {
    pub fn new(sr: f32) -> Self {
        let mut lp = Biquad::default();
        lp.lowpass(sr / 10.0, 0.707, sr);
        Self {
            sr,
            dec: vec![0.0; 1024],
            dec_w: 0,
            dec_acc: 0.0,
            dec_n: 0,
            hop: 0,
            lin: vec![0.0; 1024],
            lp,
            line: [DelayLine::new((sr * 0.1) as usize), DelayLine::new((sr * 0.1) as usize)],
            phase: 0.0,
            win: sr * 0.03,
            ratio: 1.0,
            cur_shift: 0.0,
            detected: 0.0,
            key: 0,
            scale: 0,
            speed: 0.1,
            amount: 1.0,
            lo: 70.0,
            hi: 1000.0,
            dry: 0.0,
            wet: 1.0,
            disp: [0.0; 3],
        }
    }

    /// YIN-style detection on the decimated buffer; returns Hz or 0 when unvoiced.
    fn detect(&mut self) -> f32 {
        let n = self.dec.len();
        for k in 0..n {
            self.lin[k] = self.dec[(self.dec_w + k) % n];
        }
        let x = &self.lin;
        let dsr = self.sr / 4.0;
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
        if rms < 0.004 {
            return 0.0;
        }
        let w = n / 2;
        let min_lag = (dsr / self.hi).max(2.0) as usize;
        let max_lag = ((dsr / self.lo) as usize).min(w - 2);
        // Cumulative-mean-normalized difference, then the first dip below the threshold.
        let mut cm = [1.0f32; 512];
        let mut running = 0.0;
        for lag in 1..=max_lag + 1 {
            let mut d = 0.0;
            for i in 0..w {
                let diff = x[i] - x[i + lag];
                d += diff * diff;
            }
            running += d;
            cm[lag] = d * lag as f32 / running.max(1e-9);
        }
        let mut pick = 0;
        for lag in min_lag..=max_lag {
            if cm[lag] < 0.2 && cm[lag] <= cm[lag - 1] && cm[lag] <= cm[lag + 1] {
                pick = lag;
                break;
            }
        }
        if pick == 0 {
            pick = (min_lag..=max_lag).min_by(|a, b| cm[*a].total_cmp(&cm[*b])).unwrap_or(0);
            if pick == 0 || cm[pick] > 0.35 {
                return 0.0;
            }
        }
        // Parabolic interpolation around the dip for sub-sample lag precision.
        let (a, b, c) = (cm[pick - 1], cm[pick], cm[pick + 1]);
        let den = a - 2.0 * b + c;
        let frac = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
        dsr / (pick as f32 + frac.clamp(-0.5, 0.5))
    }

    fn target_midi(&self, midi: f32) -> f32 {
        let steps = SCALES[self.scale.min(SCALES.len() - 1)].1;
        let mut best = midi.round();
        let mut dist = f32::MAX;
        for oct in -1..=1 {
            for s in steps {
                let base = (midi / 12.0).floor() * 12.0 + (self.key as f32 + *s as f32) % 12.0 + oct as f32 * 12.0;
                if (base - midi).abs() < dist {
                    dist = (base - midi).abs();
                    best = base;
                }
            }
        }
        best
    }
}

impl Effect for PitchFix {
    fn set_params(&mut self, p: &[f32], _: &Ctx) {
        self.key = p[0].round() as u8 % 12;
        self.scale = p[1].round() as usize;
        self.speed = p[2];
        self.amount = p[3];
        self.lo = p[4];
        self.hi = p[5].max(p[4] * 2.0);
        (self.dry, self.wet) = mix_gains(p[6]);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: &Ctx) {
        let glide = one_pole_coef(self.speed.max(0.0005) * 0.15, self.sr);
        for i in 0..l.len() {
            let m = self.lp.process((l[i] + r[i]) * 0.5);
            self.dec_acc += m;
            self.dec_n += 1;
            if self.dec_n == 4 {
                self.dec[self.dec_w] = self.dec_acc * 0.25;
                self.dec_w = (self.dec_w + 1) % self.dec.len();
                self.dec_acc = 0.0;
                self.dec_n = 0;
                self.hop += 1;
                if self.hop >= 128 {
                    self.hop = 0;
                    self.detected = self.detect();
                }
            }
            // Desired shift in semitones (0 when unvoiced).
            let want = if self.detected > 0.0 {
                let midi = 69.0 + 12.0 * (self.detected / 440.0).log2();
                (self.target_midi(midi) - midi) * self.amount
            } else {
                0.0
            };
            self.cur_shift = want + glide * (self.cur_shift - want);
            self.ratio = 2f32.powf(self.cur_shift / 12.0);
            // Two crossfaded taps on a sweeping delay shift the pitch by `ratio`.
            self.phase = (self.phase + (1.0 - self.ratio) / self.win).rem_euclid(1.0);
            let p2 = (self.phase + 0.5) % 1.0;
            let w1 = (self.phase * PI).sin();
            let w2 = (p2 * PI).sin();
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = buf[i];
                self.line[ch].write(x);
                let y = self.line[ch].read(2.0 + self.phase * self.win) * w1 + self.line[ch].read(2.0 + p2 * self.win) * w2;
                buf[i] = x * self.dry + y * self.wet;
            }
        }
        let midi = if self.detected > 0.0 { 69.0 + 12.0 * (self.detected / 440.0).log2() } else { 0.0 };
        self.disp = [self.detected, midi, self.cur_shift];
    }
    fn display(&self) -> Option<&[f32]> {
        Some(&self.disp)
    }
}
