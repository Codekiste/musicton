//! The audio engine: transport, clip sequencing (session + arrangement), mixer and routing.
//! It runs inside the audio callback, and offline for WAV export.

use super::dsp::*;
use super::effects::{self, Ctx, Effect};
use super::instruments::{self, InstrumentDsp};
use super::midifx::{self, Ev, FxCtx, MidiFxDsp};
use super::Shared;
use crate::model::*;
use crate::params;
use crossbeam_channel::Receiver;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug)]
pub enum Cmd {
    Play,
    Stop,
    Launch { track: u64, scene: usize },
    LaunchScene(usize),
    StopTrack(u64),
    StopAll,
    BackToArrangement,
    NoteOn { track: u64, pitch: u8, vel: u8 },
    NoteOff { track: u64, pitch: u8 },
    SetStart(u32),
    Metronome(bool),
}

#[derive(Clone, Default, Debug)]
pub struct TrackStatus {
    pub id: u64,
    pub playing: Option<usize>,
    pub queued: Option<Option<usize>>,
    pub clip_pos: f32,
    pub start_tick: u64,
    pub arr_clip: Option<u64>,
    pub follow_arr: bool,
    pub meter: [f32; 2],
}

#[derive(Clone, Default, Debug)]
pub struct Status {
    pub playing: bool,
    pub tick: u64,
    pub step_frac: f32,
    pub arr_pos: f32,
    pub start_marker: u32,
    pub tracks: Vec<TrackStatus>,
    pub returns: [[f32; 2]; 2],
    pub master: [f32; 2],
    pub cpu: f32,
    pub reductions: Vec<(u64, f32)>,
    pub displays: Vec<(u64, Vec<f32>)>,
    pub sample_rate: f32,
}

struct DevDsp {
    id: u64,
    kind: DeviceKind,
    on: bool,
    fx: Box<dyn Effect>,
    applied: Vec<f32>,
    scratch: Vec<f32>,
    /// Sidechain source track.
    sc: Option<u64>,
    /// Parallel chains of a Rack device.
    chains: Vec<ChannelDsp>,
}

impl DevDsp {
    fn apply(&mut self, ctx: &Ctx, force: bool) {
        if force || self.scratch != self.applied {
            self.fx.set_params(&self.scratch, ctx);
            self.applied.clone_from(&self.scratch);
        }
    }
}

struct ChannelDsp {
    devices: Vec<DevDsp>,
    l: [f32; MAX_BLOCK],
    r: [f32; MAX_BLOCK],
    gain: Smooth,
    pan_l: Smooth,
    pan_r: Smooth,
    mute: Smooth,
    sends: [Smooth; 2],
    meter: [f32; 2],
    /// Newly created channels jump straight to their mix values instead of fading in.
    fresh: bool,
}

impl ChannelDsp {
    fn new(sr: f32) -> Self {
        Self {
            devices: vec![],
            l: [0.0; MAX_BLOCK],
            r: [0.0; MAX_BLOCK],
            gain: Smooth::new(1.0, 0.01, sr),
            pan_l: Smooth::new(1.0, 0.01, sr),
            pan_r: Smooth::new(1.0, 0.01, sr),
            mute: Smooth::new(1.0, 0.005, sr),
            sends: [Smooth::new(0.0, 0.01, sr), Smooth::new(0.0, 0.01, sr)],
            meter: [0.0; 2],
            fresh: true,
        }
    }

    fn sync_devices(&mut self, devs: &[Device], sr: f32) {
        let mut old = std::mem::take(&mut self.devices);
        for d in devs {
            let mut dd = match old.iter().position(|o| o.id == d.id && o.kind == d.kind) {
                Some(i) => old.swap_remove(i),
                None => DevDsp {
                    id: d.id,
                    kind: d.kind,
                    on: d.on,
                    fx: effects::create(d.kind, sr),
                    applied: vec![],
                    scratch: vec![],
                    sc: None,
                    chains: vec![],
                },
            };
            dd.on = d.on;
            dd.sc = d.sc_source;
            if d.kind == DeviceKind::Rack {
                dd.chains.resize_with(d.chains.len(), || ChannelDsp::new(sr));
                for (cd, c) in dd.chains.iter_mut().zip(&d.chains) {
                    cd.sync_devices(&c.devices, sr);
                    cd.set_mix(c.volume, c.pan, [0.0; 2], c.mute);
                    cd.settle();
                }
            }
            self.devices.push(dd);
        }
    }

    fn set_mix(&mut self, vol: f32, pan: f32, sends: [f32; 2], muted: bool) {
        self.gain.target = fader_to_gain(vol);
        let (l, r) = pan_gains(pan);
        self.pan_l.target = l;
        self.pan_r.target = r;
        self.mute.target = if muted { 0.0 } else { 1.0 };
        self.sends[0].target = sends[0];
        self.sends[1].target = sends[1];
    }

    /// Called after the first mix setup: skip the smoothing ramp.
    fn settle(&mut self) {
        if self.fresh {
            self.fresh = false;
            let [s0, s1] = &mut self.sends;
            for s in [&mut self.gain, &mut self.pan_l, &mut self.pan_r, &mut self.mute, s0, s1] {
                s.value = s.target;
            }
        }
    }

    fn process_devices(&mut self, n: usize, ctx: &Ctx) {
        for d in self.devices.iter_mut().filter(|d| d.on) {
            if d.kind == DeviceKind::Rack {
                process_rack(d, &mut self.l[..n], &mut self.r[..n], ctx);
                continue;
            }
            d.fx.process(&mut self.l[..n], &mut self.r[..n], ctx);
        }
        // Safety net for live use: a misbehaving device must never take down the whole mix.
        for x in self.l[..n].iter_mut().chain(self.r[..n].iter_mut()) {
            *x = if x.is_finite() { x.clamp(-32.0, 32.0) } else { 0.0 };
        }
    }

    /// Applies fader/pan/mute, adds into the destination and optional send buses.
    fn output(&mut self, n: usize, dl: &mut [f32], dr: &mut [f32], mut sends: Option<&mut [[[f32; MAX_BLOCK]; 2]; 2]>) {
        for i in 0..n {
            let g = self.gain.next() * self.mute.next();
            let a = self.l[i] * g * self.pan_l.next();
            let b = self.r[i] * g * self.pan_r.next();
            self.meter[0] = self.meter[0].max(a.abs());
            self.meter[1] = self.meter[1].max(b.abs());
            dl[i] += a;
            dr[i] += b;
            if let Some(s) = sends.as_deref_mut() {
                for k in 0..2 {
                    let sg = self.sends[k].next();
                    s[k][0][i] += a * sg;
                    s[k][1][i] += b * sg;
                }
            }
        }
    }
}

/// Runs the input through every chain of a rack in parallel and sums the results.
fn process_rack(d: &mut DevDsp, l: &mut [f32], r: &mut [f32], ctx: &Ctx) {
    let n = l.len();
    let wet = d.applied.first().copied().unwrap_or(1.0).clamp(0.0, 1.0);
    let out = db_to_gain(d.applied.get(1).copied().unwrap_or(0.0));
    let mut sl = [0.0f32; MAX_BLOCK];
    let mut sr = [0.0f32; MAX_BLOCK];
    for c in &mut d.chains {
        c.l[..n].copy_from_slice(l);
        c.r[..n].copy_from_slice(r);
        c.process_devices(n, ctx);
        c.output(n, &mut sl, &mut sr, None);
        c.meter = [0.0; 2];
    }
    if d.chains.is_empty() {
        sl[..n].copy_from_slice(l);
        sr[..n].copy_from_slice(r);
    }
    for i in 0..n {
        l[i] = (l[i] * (1.0 - wet) + sl[i] * wet) * out;
        r[i] = (r[i] * (1.0 - wet) + sr[i] * wet) * out;
    }
}

/// Where a track's signal goes after its fader.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Dest {
    Master,
    Track(usize),
    Return(usize),
}

#[derive(Clone, Copy, Default)]
struct ModState {
    phase: f32,
    env: f32,
    stage: u8,
    rnd: f32,
}

struct MfxDsp {
    id: u64,
    kind: MidiFxKind,
    on: bool,
    fx: Box<dyn MidiFxDsp>,
    applied: Vec<f32>,
}

struct TrackDsp {
    /// Destination after the fader (group, any other track, a return or the master).
    dest: Dest,
    clip_gain: Smooth,
    mods: Vec<ModState>,
    mod_vals: Vec<(AutoTarget, f32)>,
    mod_trigger: bool,
    /// Delayed notes (strummed chords) waiting to start.
    pending: Vec<Ev>,
    gin: [[f32; MAX_BLOCK]; 2],
    mfx: Vec<MfxDsp>,
    live_map: Vec<(u8, Vec<u8>)>,
    id: u64,
    kind: InstrumentKind,
    inst: Box<dyn InstrumentDsp>,
    ch: ChannelDsp,
    playing: Option<usize>,
    queued: Option<Option<usize>>,
    start_tick: u64,
    follow_arr: bool,
    arr_clip: Option<u64>,
    inst_applied: Vec<f32>,
    inst_scratch: Vec<f32>,
    auto_vals: Vec<(AutoTarget, f32)>,
}

impl TrackDsp {
    fn sync_mfx(&mut self, list: &[MidiFx]) {
        let mut old = std::mem::take(&mut self.mfx);
        for m in list {
            let mut d = match old.iter().position(|o| o.id == m.id && o.kind == m.kind) {
                Some(i) => old.swap_remove(i),
                None => MfxDsp { id: m.id, kind: m.kind, on: m.on, fx: midifx::create(m.kind), applied: vec![] },
            };
            d.on = m.on;
            if d.applied != m.params {
                d.fx.set_params(&m.params);
                d.applied.clone_from(&m.params);
            }
            self.mfx.push(d);
        }
    }

    /// Runs events through the MIDI effects starting at `from` and plays the result.
    fn play_chain(&mut self, from: usize, mut evs: Vec<Ev>, ctx: &FxCtx) -> Vec<Ev> {
        for m in self.mfx.iter_mut().skip(from).filter(|m| m.on) {
            evs = m.fx.process_batch(evs, ctx);
        }
        for e in &evs {
            if e.delay > 0 {
                self.pending.push(*e);
            } else {
                self.inst.note_on(e.pitch, e.vel, e.gate, e.slide);
            }
        }
        if !evs.is_empty() {
            self.mod_trigger = true;
        }
        evs
    }

    fn new(t: &Track, sr: f32) -> Self {
        Self {
            dest: Dest::Master,
            clip_gain: Smooth::new(1.0, 0.01, sr),
            mods: vec![],
            mod_vals: vec![],
            mod_trigger: false,
            pending: Vec::with_capacity(16),
            gin: [[0.0; MAX_BLOCK]; 2],
            mfx: vec![],
            live_map: vec![],
            id: t.id,
            kind: t.instrument.kind,
            inst: instruments::create(t.instrument.kind, sr),
            ch: ChannelDsp::new(sr),
            playing: None,
            queued: None,
            start_tick: 0,
            follow_arr: true,
            arr_clip: None,
            inst_applied: vec![],
            inst_scratch: vec![],
            auto_vals: vec![],
        }
    }
}

struct Click {
    phase: Osc,
    freq: f32,
    env: f32,
    mul: f32,
}

pub struct Engine {
    sr: f32,
    project: Arc<Project>,
    tracks: Vec<TrackDsp>,
    returns: Vec<ChannelDsp>,
    master: ChannelDsp,
    playing: bool,
    tick: u64,
    arr_pos: u32,
    last_arr_pos: u32,
    start_marker: u32,
    samples_to_step: f64,
    cur_step_len: f64,
    metronome: bool,
    click: Click,
    rx: Option<Receiver<Cmd>>,
    shared: Option<Arc<Shared>>,
    cpu: f32,
    reductions: Vec<(u64, f32)>,
    rng: u32,
    /// Processing order: sources before the tracks they feed.
    order: Vec<usize>,
    chain_idx: usize,
}

impl Engine {
    pub fn new(sr: f32, project: Arc<Project>) -> Self {
        let mut e = Self {
            sr,
            project: Arc::new(Project::empty()),
            tracks: vec![],
            returns: vec![],
            master: ChannelDsp::new(sr),
            playing: false,
            tick: 0,
            arr_pos: 0,
            last_arr_pos: 0,
            start_marker: 0,
            samples_to_step: 0.0,
            cur_step_len: 1.0,
            metronome: false,
            click: Click { phase: Osc::default(), freq: 1000.0, env: 0.0, mul: decay_mult(0.04, sr) },
            rx: None,
            shared: None,
            cpu: 0.0,
            reductions: Vec::with_capacity(64),
            rng: 0x1234_5678,
            order: vec![],
            chain_idx: 0,
        };
        e.sync(project);
        e
    }

    pub fn attach(&mut self, rx: Receiver<Cmd>, shared: Arc<Shared>) {
        self.rx = Some(rx);
        self.shared = Some(shared);
    }

    fn ctx(&self) -> Ctx {
        let frac = if self.playing { (1.0 - self.samples_to_step / self.cur_step_len).clamp(0.0, 1.0) } else { 0.0 };
        Ctx {
            sr: self.sr,
            bpm: self.project.bpm.clamp(20.0, 999.0),
            beat: (self.tick.saturating_sub(1) as f64 + frac) / 4.0,
            playing: self.playing,
        }
    }

    fn base_step_samples(&self) -> f64 {
        60.0 / self.project.bpm.clamp(20.0, 999.0) as f64 / 4.0 * self.sr as f64
    }

    pub fn sync(&mut self, p: Arc<Project>) {
        let sr = self.sr;
        let mut old = std::mem::take(&mut self.tracks);
        for t in &p.tracks {
            let mut td = match old.iter().position(|o| o.id == t.id) {
                Some(i) => {
                    let mut td = old.swap_remove(i);
                    if td.kind != t.instrument.kind {
                        td.inst = instruments::create(t.instrument.kind, sr);
                        td.kind = t.instrument.kind;
                        td.inst_applied.clear();
                    }
                    td
                }
                None => TrackDsp::new(t, sr),
            };
            td.ch.sync_devices(&t.channel.devices, sr);
            td.sync_mfx(&t.midi_fx);
            if let Some(s) = td.playing {
                if t.clips.get(s).is_none_or(|c| c.is_none()) {
                    td.playing = None;
                }
            }
            self.tracks.push(td);
        }
        for (ti, t) in p.tracks.iter().enumerate() {
            let group = t.group.and_then(|g| p.tracks.iter().position(|x| x.id == g && x.instrument.kind == InstrumentKind::Group));
            let dest = match t.output {
                Output::Track(id) => p.tracks.iter().position(|x| x.id == id).filter(|&i| i != ti).map(Dest::Track),
                Output::Return(k) if (k as usize) < p.returns.len().min(2) => Some(Dest::Return(k as usize)),
                _ => None,
            };
            let td = &mut self.tracks[ti];
            td.dest = dest.or(group.map(Dest::Track)).unwrap_or(Dest::Master);
            td.mods.resize_with(t.modulators.len(), ModState::default);
        }
        self.order = route_order(&mut self.tracks);
        while self.returns.len() < p.returns.len() {
            self.returns.push(ChannelDsp::new(sr));
        }
        self.returns.truncate(p.returns.len());
        for (rd, rt) in self.returns.iter_mut().zip(&p.returns) {
            rd.sync_devices(&rt.channel.devices, sr);
        }
        self.master.sync_devices(&p.master.devices, sr);
        self.project = p;
        for ti in 0..self.tracks.len() {
            self.apply_track(ti, true);
        }
        let ctx = self.ctx();
        let p = self.project.clone();
        for (rd, rt) in self.returns.iter_mut().zip(&p.returns) {
            apply_devices(rd, &rt.channel.devices, &[], &[], &ctx, true);
            let c = &rt.channel;
            rd.set_mix(c.volume, c.pan, [0.0; 2], c.mute);
            rd.settle();
        }
        apply_devices(&mut self.master, &p.master.devices, &[], &[], &ctx, true);
        let m = &p.master;
        self.master.set_mix(m.volume, 0.0, [0.0; 2], false);
        self.master.settle();
    }

    /// Advances the track modulators by one block and pushes the new values if any are active.
    fn run_modulators(&mut self, n: usize) {
        let p = self.project.clone();
        let beat_hz = p.bpm.clamp(20.0, 999.0) / 60.0;
        for ti in 0..self.tracks.len() {
            let t = &p.tracks[ti];
            let td = &mut self.tracks[ti];
            let trig = std::mem::take(&mut td.mod_trigger);
            if t.modulators.is_empty() {
                if !td.mod_vals.is_empty() {
                    td.mod_vals.clear();
                    self.apply_track(ti, false);
                }
                continue;
            }
            td.mod_vals.clear();
            let dt = n as f32 / self.sr;
            for (m, st) in t.modulators.iter().zip(td.mods.iter_mut()) {
                let hz = if m.synced { beat_hz / params::division_beats(m.rate) } else { m.rate.max(0.001) };
                let prev = st.phase;
                st.phase = (st.phase + hz * dt).fract();
                if st.phase < prev {
                    self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    st.rnd = (self.rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
                }
                if trig {
                    st.stage = 1;
                }
                let v = match m.shape {
                    ModShape::Sine => (st.phase * std::f32::consts::TAU).sin(),
                    ModShape::Triangle => 1.0 - 4.0 * (st.phase - 0.5).abs(),
                    ModShape::Saw => st.phase * 2.0 - 1.0,
                    ModShape::Square => if st.phase < 0.5 { 1.0 } else { -1.0 },
                    ModShape::Random => st.rnd,
                    ModShape::Envelope => {
                        match st.stage {
                            1 => {
                                st.env += dt / m.attack.max(0.001);
                                if st.env >= 1.0 {
                                    st.env = 1.0;
                                    st.stage = 2;
                                }
                            }
                            _ => st.env *= (-dt / m.decay.max(0.001) * 4.0).exp(),
                        }
                        st.env
                    }
                };
                if let (true, Some(target)) = (m.on, m.target) {
                    td.mod_vals.push((target, v * m.depth * 0.5));
                }
            }
            self.apply_track(ti, false);
        }
    }

    fn apply_track(&mut self, ti: usize, force: bool) {
        let p = self.project.clone();
        let ctx = self.ctx();
        let any_solo = p.any_solo();
        let t = &p.tracks[ti];
        let td = &mut self.tracks[ti];
        let defs = t.instrument.kind.defs();
        td.inst_scratch.clear();
        td.inst_scratch.extend_from_slice(&t.instrument.params);
        let (mut vol, mut pan, mut sends) = (t.channel.volume, t.channel.pan, t.channel.sends);
        for &(target, v) in &td.auto_vals {
            match target {
                AutoTarget::Instrument(i) if i < defs.len() => td.inst_scratch[i] = defs[i].from_norm(v),
                AutoTarget::Volume => vol = v,
                AutoTarget::Pan => pan = v * 2.0 - 1.0,
                AutoTarget::Send(k) => sends[(k as usize).min(1)] = v,
                _ => {}
            }
        }
        for &(target, off) in &td.mod_vals {
            match target {
                AutoTarget::Instrument(i) if i < defs.len() => {
                    td.inst_scratch[i] = defs[i].from_norm((defs[i].to_norm(td.inst_scratch[i]) + off).clamp(0.0, 1.0))
                }
                AutoTarget::Volume => vol = (vol + off).clamp(0.0, 1.0),
                AutoTarget::Pan => pan = (pan + off * 2.0).clamp(-1.0, 1.0),
                AutoTarget::Send(k) => sends[(k as usize).min(1)] = (sends[(k as usize).min(1)] + off).clamp(0.0, 1.0),
                _ => {}
            }
        }
        td.inst.set_tempo(p.bpm.clamp(20.0, 999.0));
        if force {
            td.inst.set_extra(&t.instrument);
        }
        if force || td.inst_scratch != td.inst_applied {
            td.inst.set_params(&td.inst_scratch, &t.instrument.samples);
            td.inst_applied.clone_from(&td.inst_scratch);
        }
        apply_devices(&mut td.ch, &t.channel.devices, &td.auto_vals, &td.mod_vals, &ctx, force);
        let muted = t.channel.mute || (any_solo && !p.solo_ok(t));
        let xf = p.crossfader.clamp(-1.0, 1.0);
        let xgain = match t.channel.xfade {
            1 => if xf <= 0.0 { 1.0 } else { (xf * std::f32::consts::FRAC_PI_2).cos() },
            2 => if xf >= 0.0 { 1.0 } else { (-xf * std::f32::consts::FRAC_PI_2).cos() },
            _ => 1.0,
        };
        td.ch.set_mix(vol, pan, sends, muted);
        td.ch.gain.target *= xgain;
        td.ch.settle();
    }

    pub fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play => self.start(),
            Cmd::Stop => {
                if self.playing {
                    self.playing = false;
                    for td in &mut self.tracks {
                        td.inst.all_off();
                        td.pending.clear();
                        td.playing = None;
                        td.queued = None;
                        td.arr_clip = None;
                        for m in &mut td.mfx {
                            m.fx.reset();
                        }
                    }
                } else {
                    self.start_marker = 0;
                }
                self.arr_pos = self.start_marker;
                self.last_arr_pos = self.start_marker;
            }
            Cmd::Launch { track, scene } => {
                if let Some(td) = self.tracks.iter_mut().find(|t| t.id == track) {
                    td.queued = Some(Some(scene));
                }
                self.start();
            }
            Cmd::LaunchScene(scene) => {
                let p = self.project.clone();
                for (td, t) in self.tracks.iter_mut().zip(&p.tracks) {
                    let has = t.clips.get(scene).is_some_and(|c| c.is_some());
                    td.queued = Some(if has { Some(scene) } else { None });
                }
                self.start();
            }
            Cmd::StopTrack(id) => {
                if let Some(td) = self.tracks.iter_mut().find(|t| t.id == id) {
                    if self.playing {
                        td.queued = Some(None);
                    } else {
                        td.playing = None;
                        td.follow_arr = false;
                    }
                }
            }
            Cmd::StopAll => {
                for td in &mut self.tracks {
                    td.queued = Some(None);
                }
            }
            Cmd::BackToArrangement => {
                for td in &mut self.tracks {
                    td.follow_arr = true;
                    td.queued = None;
                    if td.playing.take().is_some() {
                        td.inst.all_off();
                    }
                }
            }
            Cmd::NoteOn { track, pitch, vel } => {
                let ctx = self.fx_ctx();
                if let Some(td) = self.tracks.iter_mut().find(|t| t.id == track) {
                    let out = td.play_chain(0, vec![Ev { pitch, vel, gate: None, slide: false, src: pitch, delay: 0 }], &ctx);
                    td.live_map.retain(|(p, _)| *p != pitch);
                    td.live_map.push((pitch, out.iter().filter(|e| e.gate.is_none()).map(|e| e.pitch).collect()));
                }
                self.dispatch_kicks();
            }
            Cmd::NoteOff { track, pitch } => {
                if let Some(td) = self.tracks.iter_mut().find(|t| t.id == track) {
                    for m in &mut td.mfx {
                        m.fx.note_off(pitch);
                    }
                    match td.live_map.iter().position(|(p, _)| *p == pitch) {
                        Some(i) => {
                            let (_, outs) = td.live_map.remove(i);
                            for p in outs {
                                td.inst.note_off(p);
                            }
                        }
                        None => td.inst.note_off(pitch),
                    }
                }
            }
            Cmd::SetStart(pos) => {
                self.start_marker = pos;
                self.arr_pos = pos;
                self.last_arr_pos = pos;
                if self.playing {
                    for td in self.tracks.iter_mut().filter(|t| t.follow_arr && t.playing.is_none()) {
                        td.inst.all_off();
                    }
                }
            }
            Cmd::Metronome(on) => self.metronome = on,
        }
    }

    fn fx_ctx(&self) -> FxCtx {
        FxCtx { step_samples: self.base_step_samples(), bpm: self.project.bpm.clamp(20.0, 999.0), sr: self.sr }
    }

    fn start(&mut self) {
        if !self.playing {
            self.playing = true;
            self.tick = 0;
            self.arr_pos = self.start_marker;
            self.samples_to_step = 0.0;
            self.chain_idx = 0;
            let p = self.project.clone();
            if p.chain_on && !p.chain.is_empty() {
                let secs: Vec<&Section> = p.chain.iter().filter_map(|id| p.sections.iter().find(|s| s.id == *id)).collect();
                // From the song start the chain begins at its first entry; a start marker set
                // inside a chained section starts there.
                let inside = if self.arr_pos == 0 { None } else { secs.iter().position(|s| self.arr_pos >= s.start && self.arr_pos < s.start + s.len) };
                match inside {
                    Some(i) => self.chain_idx = i,
                    None => {
                        if let Some(s) = secs.first() {
                            self.arr_pos = s.start;
                        }
                    }
                }
            }
        }
    }

    /// Launch every clip of a scene immediately (used by the exporter).
    pub fn force_scene(&mut self, scene: usize) {
        let p = self.project.clone();
        for (td, t) in self.tracks.iter_mut().zip(&p.tracks) {
            td.follow_arr = false;
            td.playing = t.clips.get(scene).and_then(|c| c.as_ref()).map(|_| scene);
            td.start_tick = 0;
        }
        self.start();
    }

    fn dispatch_kicks(&mut self) {
        let mut kick = false;
        for td in &mut self.tracks {
            kick |= td.inst.take_kick();
        }
        if !kick {
            return;
        }
        let chans = self.tracks.iter_mut().map(|t| &mut t.ch).chain(self.returns.iter_mut()).chain(std::iter::once(&mut self.master));
        for ch in chans {
            for d in &mut ch.devices {
                d.fx.on_kick();
            }
        }
    }

    fn do_step(&mut self) {
        let p = self.project.clone();
        let tick = self.tick;
        let step_len = self.base_step_samples();
        if tick % p.quant.max(1) as u64 == 0 {
            for td in &mut self.tracks {
                if let Some(q) = td.queued.take() {
                    if td.playing.is_some() && q != td.playing {
                        td.inst.all_off();
                    }
                    td.playing = q;
                    td.start_tick = tick;
                    td.follow_arr = false;
                    td.arr_clip = None;
                }
            }
        }
        // Follow actions: when a clip has played for its follow time, launch the next one.
        for ti in 0..self.tracks.len() {
            let td = &self.tracks[ti];
            let (Some(scene), None) = (td.playing, td.queued) else { continue };
            let t = &p.tracks[ti];
            let Some(Some(clip)) = t.clips.get(scene) else { continue };
            let Some(f) = clip.follow else { continue };
            let period = f.bars.max(1) as u64 * STEPS_PER_BAR as u64;
            let elapsed = tick - td.start_tick;
            if elapsed == 0 || elapsed % period != 0 {
                continue;
            }
            self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let roll = (self.rng >> 8) as f32 / (1u32 << 24) as f32;
            if roll >= f.chance {
                continue;
            }
            let filled: Vec<usize> = (0..t.clips.len()).filter(|&s| t.clips[s].is_some()).collect();
            let pos = filled.iter().position(|&s| s == scene).unwrap_or(0);
            let pick = |r: u32| filled[(r as usize) % filled.len()];
            let next = match f.action {
                FollowAction::Next => Some(filled[(pos + 1) % filled.len()]),
                FollowAction::Previous => Some(filled[(pos + filled.len() - 1) % filled.len()]),
                FollowAction::First => filled.first().copied(),
                FollowAction::Last => filled.last().copied(),
                FollowAction::Any => Some(pick(self.rng >> 4)),
                FollowAction::Other => {
                    let others: Vec<usize> = filled.iter().copied().filter(|&s| s != scene).collect();
                    if others.is_empty() { Some(scene) } else { Some(others[(self.rng >> 4) as usize % others.len()]) }
                }
                FollowAction::Again => Some(scene),
                FollowAction::Stop => None,
            };
            let td = &mut self.tracks[ti];
            if next != Some(scene) {
                td.inst.all_off();
            }
            td.playing = next;
            td.start_tick = tick;
        }
        if self.metronome && tick % 4 == 0 {
            self.click.freq = if self.arr_pos % 16 == 0 { 2400.0 } else { 1500.0 };
            self.click.env = 0.5;
            self.click.phase = Osc::default();
        }
        let arr = self.arr_pos;
        self.last_arr_pos = arr;
        let chord = p.chord_at(arr).map(|c| c.classes());
        let mut chance_rng = self.rng ^ (tick as u32).wrapping_mul(2654435761);
        for ti in 0..self.tracks.len() {
            let t = &p.tracks[ti];
            let td = &mut self.tracks[ti];
            let mut clip_and_local: Option<(&Clip, u32)> = None;
            if let Some(sc) = td.playing {
                match t.clips.get(sc).and_then(|c| c.as_ref()) {
                    Some(c) if c.len > 0 => clip_and_local = Some((c, ((tick - td.start_tick) % c.len as u64) as u32)),
                    _ => td.playing = None,
                }
            } else if td.follow_arr {
                let found = t.arrangement.iter().find(|ac| arr >= ac.start && arr < ac.start + ac.len);
                let prev = td.arr_clip;
                td.arr_clip = found.map(|ac| ac.id);
                if let Some(ac) = found {
                    if ac.clip.len > 0 {
                        clip_and_local = Some((&ac.clip, (arr - ac.start) % ac.clip.len));
                    }
                } else {
                    // Playlist: a block plays the clips of its scene where the timeline is empty.
                    let block = p.playlist.iter().find(|b| {
                        arr >= b.start && arr < b.start + b.len && t.clips.get(b.scene).is_some_and(|c| c.as_ref().is_some_and(|c| c.len > 0))
                    });
                    if let Some(b) = block {
                        let c = t.clips[b.scene].as_ref().unwrap();
                        td.arr_clip = Some(u64::MAX - b.id);
                        clip_and_local = Some((c, (arr - b.start) % c.len));
                    }
                }
                if prev.is_some() && prev != td.arr_clip {
                    td.inst.all_off();
                }
            }
            if let Some((c, _)) = clip_and_local {
                td.clip_gain.target = db_to_gain(c.gain.clamp(-60.0, 24.0));
            }
            if td.kind == InstrumentKind::Audio {
                td.inst.audio_sync(clip_and_local.and_then(|(c, local)| c.audio.as_ref().map(|a| (a, local as f64 / 4.0))));
            }
            let fx_ctx = FxCtx { step_samples: step_len, bpm: p.bpm, sr: self.sr };
            if let Some((clip, local)) = clip_and_local {
                let mut evs = vec![];
                for n in clip.notes.iter().filter(|n| n.step == local) {
                    let mut gate = n.len.max(1) as f64 * step_len;
                    if n.slide {
                        gate += step_len * 0.5;
                    } else {
                        gate -= step_len * 0.08;
                    }
                    if n.skip > 0 {
                        // Note probability.
                        chance_rng = chance_rng.wrapping_mul(1664525).wrapping_add(1013904223);
                        if ((chance_rng >> 8) % 100) < n.skip as u32 {
                            continue;
                        }
                    }
                    let pitch = match &chord {
                        Some(cl) if t.follow_chords && !td.kind.is_drum_like() => snap_to_chord(n.pitch, cl),
                        _ => n.pitch,
                    };
                    evs.push(Ev { pitch, vel: n.vel, gate: Some(gate.max(1.0) as u32), slide: n.slide, src: pitch, delay: 0 });
                }
                if !evs.is_empty() {
                    td.play_chain(0, evs, &fx_ctx);
                }
            }
            // Generators (arpeggiator) emit on every step and feed the rest of the chain.
            for gi in 0..td.mfx.len() {
                if !td.mfx[gi].on {
                    continue;
                }
                let mut out = vec![];
                td.mfx[gi].fx.tick(tick, &fx_ctx, &mut out);
                if !out.is_empty() {
                    td.play_chain(gi + 1, out, &fx_ctx);
                }
            }
            // Timeline automation.
            let had_auto = !td.auto_vals.is_empty();
            td.auto_vals.clear();
            if td.follow_arr && td.playing.is_none() {
                for a in &t.automation {
                    if let Some(v) = a.value_at(arr as f32) {
                        td.auto_vals.push((a.target, v));
                    }
                }
            }
            if had_auto || !td.auto_vals.is_empty() {
                self.apply_track(ti, false);
            }
        }
        self.rng = self.rng.wrapping_add(chance_rng | 1);
        self.dispatch_kicks();
        self.tick += 1;
        self.arr_pos += 1;
        // Arranger chain: jump from the end of one section to the start of the next one.
        if p.chain_on && !p.chain.is_empty() {
            let cur = p.chain.get(self.chain_idx % p.chain.len()).and_then(|id| p.sections.iter().find(|s| s.id == *id));
            if let Some(sec) = cur {
                if self.arr_pos == sec.start + sec.len {
                    self.chain_idx = (self.chain_idx + 1) % p.chain.len();
                    if let Some(next) = p.sections.iter().find(|s| s.id == p.chain[self.chain_idx]) {
                        self.arr_pos = next.start;
                    }
                }
            }
            return;
        }
        if p.loop_on && p.loop_len > 0 && self.arr_pos == p.loop_start + p.loop_len {
            self.arr_pos = p.loop_start;
        }
    }

    fn render(&mut self, out: &mut [f32], n: usize) {
        let ctx = self.ctx();
        let mut ml = [0.0f32; MAX_BLOCK];
        let mut mr = [0.0f32; MAX_BLOCK];
        let mut sends = [[[0.0f32; MAX_BLOCK]; 2]; 2];
        for td in &mut self.tracks {
            td.gin[0][..n].fill(0.0);
            td.gin[1][..n].fill(0.0);
        }
        self.run_modulators(n);
        // Tracks in routing order: sources first, then the tracks (groups, resamplers) they feed.
        for oi in 0..self.order.len() {
            let ti = self.order[oi];
            let mut sc_ch = std::mem::replace(&mut self.tracks[ti].ch, ChannelDsp::new(self.sr));
            let has_sc = sc_ch.devices.iter().any(|d| d.sc.is_some());
            let td = &mut self.tracks[ti];
            sc_ch.l[..n].fill(0.0);
            sc_ch.r[..n].fill(0.0);
            if !td.pending.is_empty() {
                let n32 = n as u32;
                for e in td.pending.iter_mut() {
                    e.delay = e.delay.saturating_sub(n32);
                }
                while let Some(i) = td.pending.iter().position(|e| e.delay == 0) {
                    let e = td.pending.swap_remove(i);
                    td.inst.note_on(e.pitch, e.vel, e.gate, e.slide);
                }
            }
            if td.kind != InstrumentKind::Group {
                td.inst.set_position(ctx.beat, ctx.playing);
                td.inst.render(&mut sc_ch.l[..n], &mut sc_ch.r[..n]);
                for i in 0..n {
                    let g = td.clip_gain.next();
                    sc_ch.l[i] *= g;
                    sc_ch.r[i] *= g;
                }
            }
            for i in 0..n {
                sc_ch.l[i] += td.gin[0][i];
                sc_ch.r[i] += td.gin[1][i];
            }
            if has_sc {
                sidechain_feed(&mut sc_ch, &self.tracks, n);
            }
            sc_ch.process_devices(n, &ctx);
            let dest = self.tracks[ti].dest;
            match dest {
                Dest::Track(g) => {
                    let mut gl = [0.0f32; MAX_BLOCK];
                    let mut gr = [0.0f32; MAX_BLOCK];
                    sc_ch.output(n, &mut gl, &mut gr, Some(&mut sends));
                    let gd = &mut self.tracks[g];
                    for i in 0..n {
                        gd.gin[0][i] += gl[i];
                        gd.gin[1][i] += gr[i];
                    }
                }
                Dest::Return(k) => {
                    let mut gl = [0.0f32; MAX_BLOCK];
                    let mut gr = [0.0f32; MAX_BLOCK];
                    sc_ch.output(n, &mut gl, &mut gr, Some(&mut sends));
                    for i in 0..n {
                        sends[k][0][i] += gl[i];
                        sends[k][1][i] += gr[i];
                    }
                }
                Dest::Master => sc_ch.output(n, &mut ml, &mut mr, Some(&mut sends)),
            }
            self.tracks[ti].ch = sc_ch;
        }
        for (k, rd) in self.returns.iter_mut().enumerate().take(2) {
            rd.l[..n].copy_from_slice(&sends[k][0][..n]);
            rd.r[..n].copy_from_slice(&sends[k][1][..n]);
            sidechain_feed(rd, &self.tracks, n);
            rd.process_devices(n, &ctx);
            rd.output(n, &mut ml, &mut mr, None);
        }
        if self.click.env > 0.0001 {
            for i in 0..n {
                let c = self.click.phase.sine(self.click.freq, self.sr) * self.click.env;
                self.click.env *= self.click.mul;
                ml[i] += c;
                mr[i] += c;
            }
        }
        let m = &mut self.master;
        m.l[..n].copy_from_slice(&ml[..n]);
        m.r[..n].copy_from_slice(&mr[..n]);
        sidechain_feed(m, &self.tracks, n);
        m.process_devices(n, &ctx);
        let mut ol = [0.0f32; MAX_BLOCK];
        let mut or = [0.0f32; MAX_BLOCK];
        m.output(n, &mut ol, &mut or, None);
        for i in 0..n {
            out[i * 2] = ol[i].clamp(-1.0, 1.0);
            out[i * 2 + 1] = or[i].clamp(-1.0, 1.0);
        }
    }

    /// Fill an interleaved stereo buffer.
    pub fn process(&mut self, out: &mut [f32]) {
        let t0 = Instant::now();
        if let Some(rx) = self.rx.clone() {
            while let Ok(cmd) = rx.try_recv() {
                self.handle(cmd);
            }
        }
        if let Some(shared) = self.shared.clone() {
            let p = shared.project.load_full();
            if !Arc::ptr_eq(&p, &self.project) {
                self.sync(p);
            }
        }
        let frames = out.len() / 2;
        let mut idx = 0;
        while idx < frames {
            if self.playing && self.samples_to_step <= 0.0 {
                self.do_step();
                let base = self.base_step_samples();
                let s = self.project.swing.clamp(0.0, 1.0) as f64 * 0.33;
                // `tick` was already advanced, so an odd value means the step just played was even.
                let len = if self.tick % 2 == 1 { base * (1.0 + s) } else { base * (1.0 - s) };
                self.samples_to_step += len;
                self.cur_step_len = len;
            }
            let mut n = (frames - idx).min(MAX_BLOCK);
            if self.playing {
                n = n.min(self.samples_to_step.ceil().max(1.0) as usize);
            }
            self.render(&mut out[idx * 2..(idx + n) * 2], n);
            if self.playing {
                self.samples_to_step -= n as f64;
            }
            idx += n;
        }
        let elapsed = t0.elapsed().as_secs_f32();
        let budget = frames as f32 / self.sr;
        self.cpu = self.cpu * 0.9 + (elapsed / budget.max(1e-6)) * 0.1;
        self.publish();
    }

    fn publish(&mut self) {
        let Some(shared) = self.shared.clone() else { return };
        let Some(mut st) = shared.status.try_lock() else { return };
        let frac = if self.playing { (1.0 - self.samples_to_step / self.cur_step_len).clamp(0.0, 1.0) as f32 } else { 0.0 };
        st.playing = self.playing;
        st.tick = self.tick.saturating_sub(1);
        st.step_frac = frac;
        st.arr_pos = if self.playing { self.last_arr_pos as f32 + frac } else { self.arr_pos as f32 };
        st.start_marker = self.start_marker;
        st.sample_rate = self.sr;
        st.cpu = self.cpu;
        st.tracks.resize_with(self.tracks.len(), Default::default);
        let p = &self.project;
        for ((ts, td), t) in st.tracks.iter_mut().zip(&mut self.tracks).zip(&p.tracks) {
            ts.id = td.id;
            ts.playing = td.playing;
            ts.queued = td.queued;
            ts.start_tick = td.start_tick;
            ts.follow_arr = td.follow_arr;
            ts.arr_clip = td.arr_clip;
            ts.clip_pos = td
                .playing
                .and_then(|s| t.clips.get(s).and_then(|c| c.as_ref()))
                .map(|c| ((self.tick.saturating_sub(1).saturating_sub(td.start_tick)) % c.len.max(1) as u64) as f32 + frac)
                .unwrap_or(0.0);
            ts.meter[0] = ts.meter[0].max(td.ch.meter[0]);
            ts.meter[1] = ts.meter[1].max(td.ch.meter[1]);
            td.ch.meter = [0.0; 2];
        }
        for (k, rd) in self.returns.iter_mut().enumerate().take(2) {
            st.returns[k][0] = st.returns[k][0].max(rd.meter[0]);
            st.returns[k][1] = st.returns[k][1].max(rd.meter[1]);
            rd.meter = [0.0; 2];
        }
        st.master[0] = st.master[0].max(self.master.meter[0]);
        st.master[1] = st.master[1].max(self.master.meter[1]);
        self.master.meter = [0.0; 2];
        self.reductions.clear();
        let chans = self.tracks.iter().map(|t| &t.ch).chain(self.returns.iter()).chain(std::iter::once(&self.master));
        for ch in chans {
            for d in &ch.devices {
                if matches!(d.kind, DeviceKind::Compressor | DeviceKind::Limiter | DeviceKind::Sidechain | DeviceKind::Gate | DeviceKind::BusCompressor | DeviceKind::Maximizer | DeviceKind::TruePeakLimiter) {
                    self.reductions.push((d.id, d.fx.reduction()));
                }
            }
        }
        st.reductions.clone_from(&self.reductions);
        // Analysis displays (Analyzer, Tuner): reuse vectors to avoid allocating per callback.
        let chans = self.tracks.iter().map(|t| &t.ch).chain(self.returns.iter()).chain(std::iter::once(&self.master));
        let mut n = 0;
        for td in &self.tracks {
            if let Some(data) = td.inst.display() {
                if n >= st.displays.len() {
                    st.displays.push((td.id, Vec::with_capacity(data.len())));
                }
                let slot = &mut st.displays[n];
                slot.0 = td.id;
                slot.1.clear();
                slot.1.extend_from_slice(data);
                n += 1;
            }
        }
        for ch in chans {
            for d in &ch.devices {
                if let Some(data) = d.fx.display() {
                    if n >= st.displays.len() {
                        st.displays.push((d.id, Vec::with_capacity(data.len())));
                    }
                    let slot = &mut st.displays[n];
                    slot.0 = d.id;
                    slot.1.clear();
                    slot.1.extend_from_slice(data);
                    n += 1;
                }
            }
        }
        st.displays.truncate(n);
    }
}

fn apply_devices(ch: &mut ChannelDsp, devs: &[Device], autos: &[(AutoTarget, f32)], mods: &[(AutoTarget, f32)], ctx: &Ctx, force: bool) {
    for (dd, d) in ch.devices.iter_mut().zip(devs) {
        dd.scratch.clear();
        dd.scratch.extend_from_slice(&d.params);
        let defs = d.kind.defs();
        for &(target, v) in autos {
            if let AutoTarget::Device { id, param } = target {
                if id == d.id && param < defs.len() {
                    dd.scratch[param] = defs[param].from_norm(v);
                }
            }
        }
        for &(target, off) in mods {
            if let AutoTarget::Device { id, param } = target {
                if id == d.id && param < defs.len() {
                    dd.scratch[param] = defs[param].from_norm((defs[param].to_norm(dd.scratch[param]) + off).clamp(0.0, 1.0));
                }
            }
        }
        dd.apply(ctx, force);
        for (cd, c) in dd.chains.iter_mut().zip(&d.chains) {
            apply_devices(cd, &c.devices, autos, mods, ctx, force);
            cd.set_mix(c.volume, c.pan, [0.0; 2], c.mute);
        }
    }
}

/// Sorts tracks so every source is processed before the track it feeds. Feedback loops are
/// broken by sending the offending track to the master.
fn route_order(tracks: &mut [TrackDsp]) -> Vec<usize> {
    let n = tracks.len();
    let mut depth = vec![0usize; n];
    for i in 0..n {
        let mut seen = vec![i];
        let mut cur = i;
        while let Dest::Track(j) = tracks[cur].dest {
            if seen.contains(&j) {
                tracks[cur].dest = Dest::Master;
                break;
            }
            seen.push(j);
            cur = j;
        }
        depth[i] = seen.len();
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(depth[i]));
    order
}

/// Copies the current (or previous block's) post-device signal of a source track.
fn sidechain_feed(ch: &mut ChannelDsp, tracks: &[TrackDsp], n: usize) {
    for d in ch.devices.iter_mut() {
        let Some(src) = d.sc else { continue };
        if let Some(s) = tracks.iter().find(|t| t.id == src) {
            d.fx.sidechain_input(&s.ch.l[..n], &s.ch.r[..n]);
        }
    }
}
