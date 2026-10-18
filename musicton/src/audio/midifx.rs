//! MIDI effects: they transform note events before they reach the instrument.

use super::dsp::Rng;
use crate::model::{MidiFxKind, SCALES};
use crate::params::{division_beats, ARP_RATE_STEPS};

#[derive(Clone, Copy, Debug)]
pub struct Ev {
    pub pitch: u8,
    pub vel: u8,
    /// Note length in samples; `None` = held until note off (live playing).
    pub gate: Option<u32>,
    pub slide: bool,
    /// Original live pitch, used to match note offs through the chain.
    pub src: u8,
    /// Samples to wait before the note starts (strumming).
    pub delay: u32,
}

pub trait MidiFxDsp: Send {
    fn set_params(&mut self, p: &[f32]);
    fn process(&mut self, e: Ev, ctx: &FxCtx, out: &mut Vec<Ev>);
    /// All events that start together (one step or one live chord); defaults to `process`.
    fn process_batch(&mut self, evs: Vec<Ev>, ctx: &FxCtx) -> Vec<Ev> {
        let mut out = Vec::with_capacity(evs.len());
        for e in evs {
            self.process(e, ctx, &mut out);
        }
        out
    }
    fn note_off(&mut self, _src: u8) {}
    /// Called on every 16th step while the transport runs (generators like the arpeggiator).
    fn tick(&mut self, _tick: u64, _ctx: &FxCtx, _out: &mut Vec<Ev>) {}
    fn reset(&mut self) {}
}

pub struct FxCtx {
    pub step_samples: f64,
    pub bpm: f32,
    pub sr: f32,
}

pub fn create(kind: MidiFxKind) -> Box<dyn MidiFxDsp> {
    match kind {
        MidiFxKind::Arpeggiator => Box::new(Arp::default()),
        MidiFxKind::Chord => Box::new(Chord::default()),
        MidiFxKind::NoteLength => Box::new(NoteLength::default()),
        MidiFxKind::Pitch => Box::new(Pitch::default()),
        MidiFxKind::Random => Box::new(Random { p: vec![], rng: Rng::new(31337) }),
        MidiFxKind::Scale => Box::new(Scale::default()),
        MidiFxKind::Velocity => Box::new(Velocity { p: vec![], rng: Rng::new(4711) }),
        MidiFxKind::Strum => Box::new(Strum { p: vec![0.0; 4], rng: Rng::new(99), flip: false }),
    }
}

fn clamp_pitch(p: i32) -> Option<u8> {
    (0..=127).contains(&p).then_some(p as u8)
}

// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Chord {
    shifts: Vec<i32>,
    vel: f32,
}

impl MidiFxDsp for Chord {
    fn set_params(&mut self, p: &[f32]) {
        self.shifts = p[..6].iter().map(|v| v.round() as i32).filter(|v| *v != 0).collect();
        self.vel = p[6];
    }
    fn process(&mut self, e: Ev, _: &FxCtx, out: &mut Vec<Ev>) {
        out.push(e);
        for s in &self.shifts {
            if let Some(p) = clamp_pitch(e.pitch as i32 + s) {
                out.push(Ev { pitch: p, vel: ((e.vel as f32 * self.vel).round() as u8).max(1), ..e });
            }
        }
    }
}

#[derive(Default)]
struct NoteLength {
    sync: bool,
    beats: f32,
    gate: f32,
}

impl MidiFxDsp for NoteLength {
    fn set_params(&mut self, p: &[f32]) {
        self.sync = p[0] < 0.5;
        self.beats = division_beats(p[1]);
        self.gate = p[2];
    }
    fn process(&mut self, mut e: Ev, ctx: &FxCtx, out: &mut Vec<Ev>) {
        if let Some(g) = e.gate {
            e.gate = Some(if self.sync {
                (self.beats * 60.0 / ctx.bpm * ctx.sr) as u32
            } else {
                (g as f32 * self.gate) as u32
            }
            .max(1));
        }
        out.push(e);
    }
}

#[derive(Default)]
struct Pitch {
    shift: i32,
    lo: u8,
    hi: u8,
}

impl MidiFxDsp for Pitch {
    fn set_params(&mut self, p: &[f32]) {
        self.shift = p[0].round() as i32;
        self.lo = p[1].round() as u8;
        self.hi = p[2].round() as u8;
    }
    fn process(&mut self, e: Ev, _: &FxCtx, out: &mut Vec<Ev>) {
        if let Some(p) = clamp_pitch(e.pitch as i32 + self.shift) {
            if p >= self.lo && p <= self.hi {
                out.push(Ev { pitch: p, ..e });
            }
        }
    }
}

struct Random {
    p: Vec<f32>,
    rng: Rng,
}

impl MidiFxDsp for Random {
    fn set_params(&mut self, p: &[f32]) {
        self.p = p.to_vec();
    }
    fn process(&mut self, e: Ev, _: &FxCtx, out: &mut Vec<Ev>) {
        let (chance, choices, scale, sign) = (self.p[0], self.p[1].round() as i32, self.p[2].round() as i32, self.p[3].round() as i32);
        let mut pitch = e.pitch as i32;
        if self.rng.next() * 0.5 + 0.5 < chance {
            let pick = 1 + ((self.rng.next() * 0.5 + 0.5) * choices as f32).floor().min(choices as f32 - 1.0) as i32;
            let dir = match sign {
                0 => 1,
                1 => -1,
                _ => if self.rng.next() > 0.0 { 1 } else { -1 },
            };
            pitch += dir * pick * scale;
        }
        if let Some(p) = clamp_pitch(pitch) {
            out.push(Ev { pitch: p, ..e });
        }
    }
}

#[derive(Default)]
struct Scale {
    base: i32,
    scale: usize,
    transpose: i32,
}

impl MidiFxDsp for Scale {
    fn set_params(&mut self, p: &[f32]) {
        self.base = p[0].round() as i32;
        self.scale = (p[1].round() as usize).min(SCALES.len() - 1);
        self.transpose = p[2].round() as i32;
    }
    fn process(&mut self, e: Ev, _: &FxCtx, out: &mut Vec<Ev>) {
        let steps = SCALES[self.scale].1;
        let p = e.pitch as i32 + self.transpose;
        let rel = (p - self.base).rem_euclid(12);
        // Snap down to the nearest scale degree.
        let snapped = steps.iter().map(|&s| s as i32).filter(|&s| s <= rel).max().unwrap_or(0);
        if let Some(np) = clamp_pitch(p - rel + snapped) {
            out.push(Ev { pitch: np, ..e });
        }
    }
}

struct Velocity {
    p: Vec<f32>,
    rng: Rng,
}

impl MidiFxDsp for Velocity {
    fn set_params(&mut self, p: &[f32]) {
        self.p = p.to_vec();
    }
    fn process(&mut self, mut e: Ev, _: &FxCtx, out: &mut Vec<Ev>) {
        let (fixed, drive, lo, hi, rnd) = (self.p[0] > 0.5, self.p[1], self.p[2], self.p[3], self.p[4]);
        let mut v = if fixed {
            hi
        } else {
            let x = e.vel as f32 / 127.0;
            // Drive bends the curve: positive = louder, negative = softer.
            let curved = if drive >= 0.0 { x.powf(1.0 / (1.0 + drive * 3.0)) } else { x.powf(1.0 - drive * 3.0) };
            lo + curved * (hi - lo)
        };
        v += self.rng.next() * rnd;
        e.vel = v.round().clamp(1.0, 127.0) as u8;
        out.push(e);
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Held {
    ev: Ev,
    remaining: Option<f64>,
    order: u64,
}

#[derive(Default)]
struct Arp {
    style: i32,
    rate: u64,
    gate: f32,
    octaves: i32,
    hold: bool,
    vel_decay: f32,
    held: Vec<Held>,
    counter: u64,
    order: u64,
    step: usize,
    rng: Option<Rng>,
    latch_fresh: bool,
}

impl Arp {
    fn sequence(&self) -> Vec<Ev> {
        let mut base: Vec<Held> = self.held.clone();
        if self.style == 7 {
            base.sort_by_key(|h| h.order);
        } else {
            base.sort_by_key(|h| h.ev.pitch);
        }
        let mut notes = vec![];
        for o in 0..self.octaves {
            for h in &base {
                if let Some(p) = clamp_pitch(h.ev.pitch as i32 + o * 12) {
                    notes.push(Ev { pitch: p, ..h.ev });
                }
            }
        }
        let n = notes.len();
        match self.style {
            1 => notes.reverse(),
            2 if n > 2 => {
                let mut down: Vec<Ev> = notes[1..n - 1].to_vec();
                down.reverse();
                notes.extend(down);
            }
            3 if n > 2 => {
                notes.reverse();
                let mut up: Vec<Ev> = notes[1..n - 1].to_vec();
                up.reverse();
                notes.extend(up);
            }
            4 | 5 => {
                let mut out = vec![];
                let (mut i, mut j) = (0i32, n as i32 - 1);
                while i <= j {
                    out.push(notes[i as usize]);
                    if i != j {
                        out.push(notes[j as usize]);
                    }
                    i += 1;
                    j -= 1;
                }
                if self.style == 5 {
                    out.reverse();
                }
                notes = out;
            }
            _ => {}
        }
        notes
    }
}

impl MidiFxDsp for Arp {
    fn set_params(&mut self, p: &[f32]) {
        self.style = p[0].round() as i32;
        self.rate = ARP_RATE_STEPS[(p[1].round() as usize).min(ARP_RATE_STEPS.len() - 1)];
        self.gate = p[2];
        self.octaves = p[3].round().max(1.0) as i32;
        let hold = p[4] > 0.5;
        if self.hold && !hold {
            self.held.retain(|h| h.remaining.is_some_and(|r| r > 0.0));
        }
        self.hold = hold;
        self.vel_decay = p[5];
    }

    fn process(&mut self, e: Ev, _: &FxCtx, _out: &mut Vec<Ev>) {
        if self.hold && self.latch_fresh {
            // In latch mode a new chord replaces the held one.
            self.held.clear();
            self.latch_fresh = false;
        }
        self.held.retain(|h| h.ev.pitch != e.pitch);
        self.order += 1;
        self.held.push(Held { ev: e, remaining: e.gate.map(|g| g as f64), order: self.order });
    }

    fn note_off(&mut self, src: u8) {
        if !self.hold {
            self.held.retain(|h| !(h.remaining.is_none() && h.ev.src == src));
        }
        if self.held.iter().all(|h| h.remaining.is_none()) {
            self.latch_fresh = true;
        }
    }

    fn tick(&mut self, tick: u64, ctx: &FxCtx, out: &mut Vec<Ev>) {
        // Age clip notes (live notes are released by note_off).
        let hold = self.hold;
        for h in &mut self.held {
            if let Some(r) = h.remaining.as_mut() {
                *r -= ctx.step_samples;
            }
        }
        if !hold {
            self.held.retain(|h| h.remaining.is_none_or(|r| r > 0.0));
        } else if self.held.iter().all(|h| h.remaining.is_some_and(|r| r <= 0.0)) {
            self.latch_fresh = true;
        }
        if self.held.is_empty() {
            self.step = 0;
            return;
        }
        if tick % self.rate != 0 {
            return;
        }
        let seq = self.sequence();
        if seq.is_empty() {
            return;
        }
        let ev = if self.style == 6 {
            let rng = self.rng.get_or_insert(Rng::new(97));
            seq[((rng.next() * 0.5 + 0.5) * seq.len() as f32) as usize % seq.len()]
        } else {
            seq[self.step % seq.len()]
        };
        let decay = 1.0 - self.vel_decay * (self.step % seq.len().max(1)) as f32 / seq.len().max(1) as f32;
        self.step += 1;
        self.counter += 1;
        let gate = (self.rate as f64 * ctx.step_samples * self.gate as f64).max(1.0) as u32;
        out.push(Ev { gate: Some(gate), vel: ((ev.vel as f32 * decay) as u8).max(1), ..ev });
    }

    fn reset(&mut self) {
        self.held.clear();
        self.step = 0;
    }
}

// ---------------------------------------------------------------------------------------------

struct Strum {
    p: Vec<f32>,
    rng: Rng,
    flip: bool,
}

impl MidiFxDsp for Strum {
    fn set_params(&mut self, p: &[f32]) {
        self.p = p.to_vec();
    }
    fn process(&mut self, e: Ev, _: &FxCtx, out: &mut Vec<Ev>) {
        out.push(e);
    }
    fn process_batch(&mut self, mut evs: Vec<Ev>, ctx: &FxCtx) -> Vec<Ev> {
        if evs.len() < 2 {
            return evs;
        }
        evs.sort_by_key(|e| e.pitch);
        let dir = self.p[1].round() as i32;
        let down = match dir {
            1 => true,
            2 => {
                self.flip = !self.flip;
                self.flip
            }
            3 => self.rng.next() > 0.0,
            _ => false,
        };
        if down {
            evs.reverse();
        }
        let step = self.p[0] / 1000.0 * ctx.sr;
        let n = evs.len() as f32;
        for (k, e) in evs.iter_mut().enumerate() {
            let jitter = 1.0 + self.rng.next() * self.p[3] * 0.5;
            e.delay += (k as f32 * step * jitter).max(0.0) as u32;
            let slope = 1.0 + self.p[2] * (k as f32 / (n - 1.0) * 2.0 - 1.0);
            e.vel = ((e.vel as f32 * slope).round() as i32).clamp(1, 127) as u8;
            if let Some(g) = e.gate.as_mut() {
                *g = g.saturating_sub(e.delay).max(64);
            }
        }
        evs
    }
}
