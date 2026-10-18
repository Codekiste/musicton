//! B-303 engine: TB-303 style voice plus internal step sequencer.
//!
//! Signal path: saw/square VCO -> 4-pole nonlinear ladder (2x oversampled) with a high-pass in
//! the resonance feedback (like the 303's diode ladder) -> VCA -> optional overdrive.
//! Control: the MEG (filter envelope) decays over 0.2-2 s, accented notes always use the
//! shortest decay, open the VCA further and feed the "accent sweep" smoother whose speed
//! depends on the resonance knob (high resonance = slower, wobbling accents). Slides glide
//! the pitch without retriggering any envelope.

use crate::p as b303;
use crate::seq::SeqBank;
use crate::spec;
use musicton_dsp::*;
use std::f32::consts::PI;

pub struct B303 {
    sr: f32,
    p: Vec<f32>,
    osc: Osc,
    freq: f32,
    target: f32,
    gliding: bool,
    // Ladder state
    s: [f32; 4],
    fb_hp: f32,
    last_out: f32,
    // Envelopes
    meg: f32,
    meg_mul: f32,
    acc_env: f32,
    accent_on: bool,
    sweep: f32,
    amp: f32,
    amp_target: f32,
    gate: Option<u32>,
    gate_open: bool,
    prev_slide: bool,
    live_pitch: Option<u8>,
    dc: OnePole,
    sq_lp: OnePole,
    // Internal sequencer
    bank: Option<SeqBank>,
    bpm: f32,
    beat: f64,
    playing: bool,
    own_beat: f64,
    last_step: i64,
    seq_count: u64,
    cur_step: usize,
    was_running: bool,
    rng: Rng,
    // Condition (analog drift)
    drift_phase: f32,
    note_detune: f32,
    note_cutoff: f32,
    // Drive and delay
    tone_lp: [OnePole; 2],
    mt_peak: Biquad,
    dl: DelayLine,
    dr: DelayLine,
    dly_lp: [OnePole; 2],
    wow: f32,
    display: [f32; 3],
}

impl B303 {
    pub fn new(sr: f32) -> Self {
        let mut dc = OnePole::default();
        dc.set_lowpass(8.0, sr);
        let mut sq_lp = OnePole::default();
        sq_lp.set_lowpass(9000.0, sr);
        Self {
            sr,
            p: spec::defaults(),
            osc: Osc::default(),
            freq: 55.0,
            target: 55.0,
            gliding: false,
            s: [0.0; 4],
            fb_hp: 0.0,
            last_out: 0.0,
            meg: 0.0,
            meg_mul: 0.999,
            acc_env: 0.0,
            accent_on: false,
            sweep: 0.0,
            amp: 0.0,
            amp_target: 0.0,
            gate: None,
            gate_open: false,
            prev_slide: false,
            live_pitch: None,
            dc,
            sq_lp,
            bank: None,
            bpm: 120.0,
            beat: 0.0,
            playing: false,
            own_beat: 0.0,
            last_step: i64::MIN,
            seq_count: 0,
            cur_step: 0,
            was_running: false,
            rng: Rng::new(303),
            drift_phase: 0.0,
            note_detune: 0.0,
            note_cutoff: 1.0,
            tone_lp: Default::default(),
            mt_peak: {
                let mut b = Biquad::default();
                b.peaking(900.0, 1.2, 9.0, sr);
                b
            },
            dl: DelayLine::new((sr * 2.2) as usize),
            dr: DelayLine::new((sr * 2.2) as usize),
            dly_lp: Default::default(),
            wow: 0.0,
            display: [0.0; 3],
        }
    }

    /// Plays one step of the internal sequencer.
    fn seq_step(&mut self, n: u64, step_samples: f64) {
        let Some(bank) = &self.bank else { return };
        let pat = (self.p[b303::PATTERN].round() as usize).min(bank.patterns.len().saturating_sub(1));
        let Some(pattern) = bank.patterns.get(pat) else { return };
        let len = (pattern.len as usize).clamp(1, pattern.steps.len().max(1));
        let mode = self.p[b303::PLAY_MODE].round() as i32;
        let idx = match mode {
            1 => len - 1 - (n as usize % len),
            2 => {
                let period = (2 * len).saturating_sub(2).max(1);
                let m = n as usize % period;
                if m < len { m } else { period - m }
            }
            4 => ((self.rng.next() * 0.5 + 0.5) * len as f32) as usize % len,
            _ => n as usize % len,
        };
        self.cur_step = idx;
        let st = pattern.steps[idx];
        if st.time != 0 {
            return;
        }
        // Ties that follow extend the note (forward direction).
        let mut ties = 0;
        if mode == 0 || mode == 3 {
            while ties + 1 < len && pattern.steps[(idx + ties + 1) % len].time == 1 {
                ties += 1;
            }
        }
        let mut pitch = st.pitch as i32;
        if mode == 3 {
            // Invert: mirror the melody around the first note of the pattern.
            let pivot = pattern.steps.iter().find(|s| s.time == 0).map(|s| s.pitch as i32).unwrap_or(pitch);
            pitch = 2 * pivot - pitch;
        }
        let mut gate = (1 + ties) as f64 * step_samples * if ties > 0 { 1.0 } else { 0.55 };
        if st.slide {
            gate = (1 + ties) as f64 * step_samples + step_samples * 0.5;
        }
        self.note_on(pitch.clamp(12, 108) as u8, if st.accent { 120 } else { 90 }, Some(gate as u32), st.slide);
    }

    fn drive(&mut self, x: f32, ch: usize) -> f32 {
        let typ = self.p[b303::DRIVE_TYPE].round() as i32;
        let depth = self.p[b303::DRIVE];
        if typ == 0 || depth <= 0.001 {
            return x;
        }
        let g = 1.0 + depth * 24.0;
        let y = match typ {
            1 => {
                let v = x * g;
                if v > 0.0 { soft_clip(v) } else { soft_clip(v * 0.8) / 0.8 * 0.9 }
            }
            2 => (x * g).clamp(-0.55, 0.55) / 0.55 * 0.7,
            3 => {
                let v = x * g * 2.0;
                v.signum() * (1.0 - (-v.abs() * 1.5).exp()) * 0.7
            }
            _ => {
                let v = if ch == 0 { self.mt_peak.process(x) } else { x };
                (v * g * 1.5).clamp(-0.5, 0.5) * 1.3
            }
        };
        self.tone_lp[ch].lp(y) / (1.0 + depth * 0.6)
    }

    /// Filter cutoff in Hz from knobs and envelope states.
    fn cutoff(&self) -> f32 {
        let p = &self.p;
        let env_mod = p[b303::ENV_MOD];
        // Knob range ~300 Hz .. 2.4 kHz; more env mod lowers the resting cutoff like the original.
        let base = 300.0 * 2f32.powf(p[b303::CUTOFF] * 3.0) * 2f32.powf(-env_mod * 1.3);
        let base = base * 2f32.powf(p[b303::VCF_TRIM]) * self.note_cutoff;
        let env_oct = env_mod * 5.0 * self.meg;
        let accent_oct = self.sweep * p[b303::ACCENT] * 2.6;
        (base * 2f32.powf(env_oct + accent_oct)).clamp(20.0, 18000.0)
    }

    #[inline]
    fn ladder(&mut self, x: f32, g: f32, k: f32, hp_a: f32) -> f32 {
        // High-passed feedback keeps the bass when resonance is up (303 diode-ladder trait).
        let fb_in = self.last_out;
        self.fb_hp = fb_in + hp_a * (self.fb_hp - fb_in);
        let fb = fb_in - self.fb_hp;
        let mut v = soft_clip(x - k * fb);
        for s in &mut self.s {
            let y = *s + g * (v - *s);
            *s = y;
            v = soft_clip(y * 1.05) / 1.05;
        }
        // Blend in some of the third pole for the slightly softer 303 slope.
        self.last_out = self.s[3] * 0.85 + self.s[2] * 0.15;
        self.last_out
    }
}

impl B303 {
    pub fn set_params(&mut self, p: &[f32]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        let tone = 900.0 * 2f32.powf(p[b303::DRIVE_TONE] * 4.0);
        for f in &mut self.tone_lp {
            f.set_lowpass(tone, self.sr);
        }
        for f in &mut self.dly_lp {
            f.set_lowpass(if p[b303::DELAY_TYPE].round() as i32 == 3 { 2800.0 } else { 9000.0 }, self.sr);
        }
    }

    /// Updates the pattern bank. Cheap to call every block: it only copies when something changed.
    pub fn set_bank(&mut self, bank: Option<&SeqBank>) {
        if self.bank.as_ref() != bank {
            self.bank = bank.cloned();
        }
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }

    pub fn set_position(&mut self, beat: f64, playing: bool) {
        self.beat = beat;
        self.playing = playing;
    }

    /// [current step, running (0/1), pattern index] for the editor.
    pub fn display(&self) -> &[f32; 3] {
        &self.display
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, slide: bool) {
        let p = &self.p;
        let legato = self.gate_open && self.prev_slide;
        let cond = p[b303::CONDITION];
        if !legato {
            // Worn components: every note lands slightly off in pitch and brightness.
            self.note_detune = self.rng.next() * cond * 0.08;
            self.note_cutoff = 2f32.powf(self.rng.next() * cond * 0.12);
        }
        self.target = midi_to_freq(pitch as f32 + p[b303::TUNING] + p[b303::MASTER_TUNE] / 100.0 + self.note_detune);
        let accent = vel >= 110;
        if legato {
            self.gliding = true;
        } else {
            self.gliding = false;
            self.freq = self.target;
            self.meg = 1.0;
            let decay = if accent { 0.2 } else { 0.2 * 10f32.powf(p[b303::DECAY]) };
            self.meg_mul = (-2.3 / (decay * self.sr)).exp();
            self.accent_on = accent;
            if accent {
                self.acc_env = 1.0;
            }
        }
        self.amp_target = 0.65 + vel as f32 / 127.0 * 0.15;
        self.gate = gate;
        self.gate_open = true;
        self.prev_slide = slide;
        self.live_pitch = if gate.is_none() { Some(pitch) } else { None };
    }

    pub fn note_off(&mut self, pitch: u8) {
        if self.live_pitch == Some(pitch) {
            self.gate = Some(0);
            self.live_pitch = None;
        }
    }

    pub fn all_off(&mut self) {
        self.gate = Some(0);
        self.prev_slide = false;
    }

    pub fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let sr = self.sr;
        let p = self.p.clone();
        let square = p[b303::WAVE] > 0.5;
        let k = p[b303::RES].powf(0.8) * 3.95;
        let comp = 1.0 + k * 0.35;
        let glide = one_pole_coef(p[b303::SLIDE] / 3.0, sr);
        let a_att = one_pole_coef(0.002, sr);
        let a_rel = one_pole_coef(0.008, sr);
        let vca_decay = decay_mult(3.0, sr);
        let acc_decay = (-2.3 / (0.2 * sr)).exp();
        // Accent sweep smoother: slower with higher resonance.
        let sweep_coef = one_pole_coef(0.015 + p[b303::RES] * 0.11, sr);
        let hp_a = (-2.0 * PI * 120.0 / (sr * 2.0)).exp();
        let vol = p[b303::VOLUME];
        let accent_amt = p[b303::ACCENT];
        let cond = p[b303::CONDITION];
        let mut g = 0.0;
        // Internal sequencer.
        let run = p[b303::RUN] > 0.5;
        let pos_lock = p[b303::POS_LOCK] > 0.5;
        let scale = spec::SCALE_BEATS[(p[b303::SCALE].round() as usize).min(3)];
        let shuffle = p[b303::SHUFFLE] as f64;
        let beat_inc = self.bpm as f64 / 60.0 / sr as f64;
        let step_samples = scale * 60.0 / self.bpm as f64 * sr as f64;
        let synced = self.playing && pos_lock;
        let active = run && (self.playing || !pos_lock);
        if self.was_running && !active {
            self.gate = Some(0);
            self.prev_slide = false;
            self.last_step = i64::MIN;
        }
        self.was_running = active;
        if !synced {
            self.beat = self.own_beat;
        }
        // Delay settings.
        let dtype = p[b303::DELAY_TYPE].round() as i32;
        let dlevel = p[b303::DELAY_LEVEL];
        let dtime = if p[b303::TEMPO_SYNC] > 0.5 {
            const DIVS: [f32; 9] = [0.125, 1.0 / 6.0, 0.25, 1.0 / 3.0, 0.5, 0.75, 1.0, 1.5, 2.0];
            DIVS[((p[b303::DELAY_TIME] * 8.0).round() as usize).min(8)] * 60.0 / self.bpm
        } else {
            0.02 * 50f32.powf(p[b303::DELAY_TIME])
        };
        for i in 0..l.len() {
            if active {
                let b = self.beat + i as f64 * beat_inc;
                let k = (b / scale).floor() as i64;
                let frac = b / scale - k as f64;
                let k = if k.rem_euclid(2) == 1 && frac < shuffle * 0.5 { k - 1 } else { k };
                if k != self.last_step {
                    self.last_step = k;
                    let n = if synced { k.max(0) as u64 } else { self.seq_count };
                    self.seq_count += 1;
                    self.seq_step(n, step_samples);
                }
            }
            if let Some(gt) = self.gate.as_mut() {
                if *gt == 0 {
                    self.gate_open = false;
                    self.amp_target = 0.0;
                    self.gate = None;
                } else {
                    *gt -= 1;
                }
            }
            if self.gliding {
                self.freq = self.target + glide * (self.freq - self.target);
            }
            self.drift_phase = (self.drift_phase + 0.07 / sr).fract();
            self.meg *= self.meg_mul;
            self.acc_env *= acc_decay;
            let acc_cv = if self.accent_on { self.acc_env } else { 0.0 };
            self.sweep = acc_cv + sweep_coef * (self.sweep - acc_cv);
            if self.gate_open && self.amp_target > 0.0 {
                self.amp_target *= vca_decay;
            }
            let c = if self.amp_target > self.amp { a_att } else { a_rel };
            self.amp = self.amp_target + c * (self.amp - self.amp_target);
            if i % 4 == 0 {
                // One-pole coefficient at 2x oversampling.
                let fc = self.cutoff();
                g = 1.0 - (-2.0 * PI * fc / (sr * 2.0)).exp();
            }
            if self.amp < 0.00003 && !self.gate_open && self.meg < 0.001 && p[b303::DELAY_TYPE] < 0.5 {
                continue;
            }
            let drift = 1.0 + (self.drift_phase * std::f32::consts::TAU).sin() * cond * 0.004;
            let saw = self.osc.next(Wave::Saw, self.freq * drift, sr);
            let x = if square {
                // The 303 derives its square from the saw with a transistor shaper.
                self.sq_lp.lp(soft_clip(saw * 6.0 + 0.4) * 0.9)
            } else {
                saw
            } * 0.5;
            let mut y = 0.0;
            for _ in 0..2 {
                y = self.ladder(x, g, k, hp_a);
            }
            y *= comp;
            let vca = self.amp * (1.0 + accent_amt * acc_cv * 1.2);
            let mut out = self.dc.hp(y * vca * 1.6);
            out += self.rng.next() * cond * 0.0015;
            out = self.drive(out, 0);
            let (mut ol, mut or) = (out, out);
            if dtype > 0 {
                let mut t = dtime * sr;
                if dtype == 3 {
                    self.wow = (self.wow + 0.6 / sr).fract();
                    t *= 1.0 + (self.wow * std::f32::consts::TAU).sin() * 0.004;
                }
                let yl = self.dl.read(t);
                let yr = self.dr.read(t);
                let fb = 0.45;
                if dtype == 2 {
                    let fl = self.dly_lp[0].lp(yl);
                    let fr = self.dly_lp[1].lp(yr);
                    self.dl.write(out + fr * fb);
                    self.dr.write(fl * fb);
                    ol += yl * dlevel;
                    or += yr * dlevel;
                } else {
                    let f = self.dly_lp[0].lp(yl);
                    self.dl.write(out + f * fb);
                    ol += yl * dlevel;
                    or += yl * dlevel;
                }
            }
            l[i] += ol * vol;
            r[i] += or * vol;
        }
        self.beat += l.len() as f64 * beat_inc;
        if !synced {
            self.own_beat = self.beat;
        }
        self.display = [self.cur_step as f32, if active { 1.0 } else { 0.0 }, p[b303::PATTERN]];
    }
}

