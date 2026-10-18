//! Beat Pads: sixteen pads, each with a sample or synthesized drum voice, choke groups and
//! its own effect chain.

use super::dsp::*;
use super::effects::{self, Ctx, Effect};
use super::instruments::{DrumKit, InstrumentDsp};
use super::samples::{self, Sample};
use crate::model::{Device, DeviceKind, Instrument, Pad};
use crate::params::{self, lane_param, LANE_PAN, LANE_TUNE};
use std::sync::Arc;

const PADS: usize = 16;

struct PadFx {
    id: u64,
    kind: DeviceKind,
    on: bool,
    applied: Vec<f32>,
    fx: Box<dyn Effect>,
}

#[derive(Default)]
struct SampleVoice {
    active: bool,
    pos: f64,
    rate: f64,
    amp: f32,
    env: f32,
    mul: f32,
    choke: f32,
}

struct PadDsp {
    pad: Pad,
    sample: Option<Arc<Sample>>,
    synth: Option<DrumKit>,
    voice: SampleVoice,
    fx: Vec<PadFx>,
    l: [f32; 64],
    r: [f32; 64],
    gl: f32,
    gr: f32,
    /// Silent blocks since the last note; effects with tails keep running for a while.
    idle: u32,
}

pub struct BeatPads {
    sr: f32,
    p: Vec<f32>,
    pads: Vec<PadDsp>,
    bpm: f32,
    beat: f64,
    playing: bool,
    kick: bool,
}

impl BeatPads {
    pub fn new(sr: f32) -> Self {
        let pads = Pad::default_set()
            .into_iter()
            .map(|pad| PadDsp { pad, sample: None, synth: None, voice: SampleVoice::default(), fx: vec![], l: [0.0; 64], r: [0.0; 64], gl: 1.0, gr: 1.0, idle: u32::MAX })
            .collect();
        let mut b = Self { sr, p: params::defaults(&params::BEATPADS_PARAMS), pads, bpm: 120.0, beat: 0.0, playing: false, kick: false };
        for i in 0..PADS {
            b.configure(i);
        }
        b
    }

    fn ctx(&self) -> Ctx {
        Ctx { sr: self.sr, bpm: self.bpm, beat: self.beat, playing: self.playing }
    }

    /// Applies a pad's settings to its voice, synth and effect chain.
    fn configure(&mut self, i: usize) {
        let sr = self.sr;
        let global_tune = self.p.get(2).copied().unwrap_or(0.0);
        let ctx = self.ctx();
        let pd = &mut self.pads[i];
        pd.sample = pd.pad.sample.as_deref().and_then(samples::try_get);
        if pd.sample.is_none() {
            let kit = pd.synth.get_or_insert_with(|| DrumKit::new(sr));
            let mut kp = params::defaults(&params::DRUM_PARAMS);
            let lane = pd.pad.voice as usize % params::DRUM_LANES.len();
            kp[lane_param(lane, LANE_TUNE)] = (pd.pad.tune + global_tune).clamp(-24.0, 24.0);
            kp[lane_param(lane, LANE_PAN)] = 0.0;
            kit.set_params(&kp, &[]);
        }
        let (l, r) = pan_gains(pd.pad.pan);
        let g = db_to_gain(pd.pad.gain);
        pd.gl = l * g;
        pd.gr = r * g;
        sync_fx(&mut pd.fx, &pd.pad.devices, sr, &ctx);
    }

    fn trigger(&mut self, i: usize, vel: u8) {
        let choke = self.pads[i].pad.choke;
        if choke > 0 {
            for (k, other) in self.pads.iter_mut().enumerate() {
                if k != i && other.pad.choke == choke {
                    other.voice.choke = decay_mult(0.01, self.sr);
                    if let Some(kit) = other.synth.as_mut() {
                        kit.all_off();
                    }
                }
            }
        }
        let vs = self.p[1];
        let level = (1.0 - vs) + vs * vel as f32 / 127.0;
        let global_tune = self.p[2];
        let sr = self.sr;
        let pd = &mut self.pads[i];
        pd.idle = 0;
        if pd.pad.voice == 0 && pd.sample.is_none() {
            self.kick = true;
        }
        match &pd.sample {
            Some(s) => {
                let len = s.left.len() as f64;
                let rate = (s.sample_rate / sr) as f64 * 2f64.powf(((pd.pad.tune + global_tune) / 12.0) as f64);
                pd.voice = SampleVoice {
                    active: true,
                    pos: if pd.pad.reverse { len - 1.0 } else { 0.0 },
                    rate: if pd.pad.reverse { -rate } else { rate },
                    amp: level,
                    env: 1.0,
                    mul: decay_mult(pd.pad.decay.max(0.01), sr),
                    choke: 1.0,
                };
            }
            None => {
                if let Some(kit) = pd.synth.as_mut() {
                    kit.note_on(pd.pad.voice, (level * 127.0) as u8, None, false);
                }
            }
        }
    }
}

fn sync_fx(list: &mut Vec<PadFx>, devs: &[Device], sr: f32, ctx: &Ctx) {
    let mut old = std::mem::take(list);
    for d in devs.iter().filter(|d| d.kind != DeviceKind::Rack) {
        let mut f = match old.iter().position(|o| o.id == d.id && o.kind == d.kind) {
            Some(i) => old.swap_remove(i),
            None => PadFx { id: d.id, kind: d.kind, on: d.on, applied: vec![], fx: effects::create(d.kind, sr) },
        };
        f.on = d.on;
        if f.applied != d.params {
            f.fx.set_params(&d.params, ctx);
            f.applied.clone_from(&d.params);
        }
        list.push(f);
    }
}

impl InstrumentDsp for BeatPads {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        let tune_changed = self.p.get(2) != p.get(2);
        self.p.clear();
        self.p.extend_from_slice(p);
        if tune_changed {
            for i in 0..PADS {
                self.configure(i);
            }
        }
    }
    fn set_extra(&mut self, inst: &Instrument) {
        for i in 0..PADS.min(inst.pads.len()) {
            let changed = self.pads[i].pad != inst.pads[i] || (self.pads[i].pad.sample.is_some() && self.pads[i].sample.is_none());
            if changed {
                self.pads[i].pad = inst.pads[i].clone();
                self.configure(i);
            }
        }
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }
    fn set_position(&mut self, beat: f64, playing: bool) {
        self.beat = beat;
        self.playing = playing;
    }
    fn note_on(&mut self, pitch: u8, vel: u8, _gate: Option<u32>, _slide: bool) {
        // Pads answer to lane indices 0-15 and to the GM-style range from C1 (36).
        let i = if (pitch as usize) < PADS { pitch as usize } else { (pitch as usize).wrapping_sub(36) };
        if i < PADS {
            self.trigger(i, vel);
        }
    }
    fn note_off(&mut self, _pitch: u8) {}
    fn all_off(&mut self) {
        for pd in &mut self.pads {
            pd.voice.choke = decay_mult(0.02, self.sr);
            if let Some(k) = pd.synth.as_mut() {
                k.all_off();
            }
        }
    }
    fn take_kick(&mut self) -> bool {
        std::mem::take(&mut self.kick)
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(64);
        let ctx = self.ctx();
        let vol = self.p[0];
        // Effects keep processing ~4 s after the last hit so tails ring out.
        let tail_blocks = (self.sr * 4.0 / 64.0) as u32;
        for pd in &mut self.pads {
            if pd.idle > tail_blocks {
                continue;
            }
            pd.idle = pd.idle.saturating_add(1);
            pd.l[..n].fill(0.0);
            pd.r[..n].fill(0.0);
            if let Some(kit) = pd.synth.as_mut().filter(|_| pd.sample.is_none()) {
                kit.render(&mut pd.l[..n], &mut pd.r[..n]);
            }
            if let (true, Some(s)) = (pd.voice.active, pd.sample.as_ref()) {
                let v = &mut pd.voice;
                let len = s.left.len();
                for i in 0..n {
                    if v.pos < 0.0 || v.pos >= (len - 1) as f64 || v.env * v.choke < 0.0005 {
                        v.active = false;
                        break;
                    }
                    let i0 = v.pos as usize;
                    let fr = (v.pos - i0 as f64) as f32;
                    let g = v.amp * v.env * v.choke;
                    pd.l[i] += (s.left[i0] * (1.0 - fr) + s.left[i0 + 1] * fr) * g;
                    pd.r[i] += (s.right[i0] * (1.0 - fr) + s.right[i0 + 1] * fr) * g;
                    v.pos += v.rate;
                    v.env *= v.mul;
                    if v.choke < 1.0 {
                        v.choke *= decay_mult(0.01, 48000.0);
                    }
                }
            }
            for f in pd.fx.iter_mut().filter(|f| f.on) {
                f.fx.process(&mut pd.l[..n], &mut pd.r[..n], &ctx);
            }
            for i in 0..n {
                let a = pd.l[i] * pd.gl * vol;
                let b = pd.r[i] * pd.gr * vol;
                l[i] += if a.is_finite() { a.clamp(-8.0, 8.0) } else { 0.0 };
                r[i] += if b.is_finite() { b.clamp(-8.0, 8.0) } else { 0.0 };
            }
        }
    }
}
