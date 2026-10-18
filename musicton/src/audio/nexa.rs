//! Nexa: a Nexus-style ROMpler. Sounds come from a built-in preset "ROM" of layered synth
//! voices; the user picks a preset and shapes it with macros, arpeggiator, trancegate and FX.

use super::dsp::*;
use super::effects::{self, Ctx, Effect};
use super::instruments::InstrumentDsp;
use super::instruments2::{mip_for, wt_read};
use crate::model::DeviceKind;
use crate::params::{self, nexa, NEXA_ARP_RATE_BEATS, NEXA_GATE_RATE_BEATS};
use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Osc {
    Saw,
    SuperSaw,
    Square,
    Pulse,
    Sine,
    Tri,
    Fm,
    Noise,
    Pluck,
    Table(u8),
    Organ,
}

#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub osc: Osc,
    pub oct: i8,
    pub semi: f32,
    /// Unison voices (SuperSaw/Saw) and their spread in cents.
    pub voices: u8,
    pub detune: f32,
    pub level: f32,
    pub pan: f32,
    /// Pulse width / wavetable position / FM ratio / pluck brightness.
    pub p1: f32,
    /// FM index / pluck decay (s) / wavetable LFO depth.
    pub p2: f32,
    /// 0 lowpass, 1 highpass, 2 bandpass, 3 off.
    pub flt: u8,
    pub cutoff: f32,
    pub res: f32,
    /// Filter envelope depth in octaves.
    pub fenv: f32,
    pub fa: f32,
    pub fd: f32,
    pub fs: f32,
    pub a: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
    /// Pitch envelope: start offset in semitones gliding to 0 over `pitch_time`.
    pub pitch: f32,
    pub pitch_time: f32,
}

const L: Layer = Layer {
    osc: Osc::Saw,
    oct: 0,
    semi: 0.0,
    voices: 1,
    detune: 0.0,
    level: 0.8,
    pan: 0.0,
    p1: 0.5,
    p2: 1.0,
    flt: 0,
    cutoff: 8000.0,
    res: 0.1,
    fenv: 0.0,
    fa: 0.001,
    fd: 0.3,
    fs: 1.0,
    a: 0.003,
    d: 0.3,
    s: 1.0,
    r: 0.3,
    pitch: 0.0,
    pitch_time: 0.1,
};

#[derive(Clone, Copy, Debug)]
pub struct Fx {
    pub chorus: f32,
    pub delay: f32,
    pub delay_time: f32,
    pub fb: f32,
    pub reverb: f32,
    pub size: f32,
    pub dist: f32,
    pub low: f32,
    pub high: f32,
}

const FX: Fx = Fx { chorus: 0.0, delay: 0.0, delay_time: 5.0, fb: 0.4, reverb: 0.0, size: 0.7, dist: 0.0, low: 0.0, high: 0.0 };

pub struct Preset {
    pub name: &'static str,
    pub layers: &'static [Layer],
    pub fx: Fx,
    /// (mode, rate, octaves, gate)
    pub arp: Option<(u8, u8, u8, f32)>,
    /// (pattern, rate, depth)
    pub gate: Option<(u8, u8, f32)>,
}

pub const GATE_PATTERNS: &[(&str, &str)] = &[
    ("Trance A", "xx.xx.xx.x.xx.x."),
    ("Trance B", "x.xxx.x.xx.xx.x."),
    ("Offbeat", "..xx..xx..xx..xx"),
    ("Pulse 8ths", "xx..xx..xx..xx.."),
    ("16ths", "x.x.x.x.x.x.x.x."),
    ("Stutter", "xxxx.x.xxx.x.x.x"),
    ("Gallop", "x.xx.xx.x.xx.xx."),
    ("Syncopated", "x..x..x...x..x.."),
    ("Build", "x...x...x.x.xxxx"),
    ("Half Bar", "xxxxxxxx........"),
    ("Wave", "xxx.xx.x..x.xx.x"),
];

macro_rules! preset {
    ($name:expr, [$($l:expr),*], $fx:expr) => { Preset { name: $name, layers: &[$($l),*], fx: $fx, arp: None, gate: None } };
    ($name:expr, [$($l:expr),*], $fx:expr, arp $arp:expr) => { Preset { name: $name, layers: &[$($l),*], fx: $fx, arp: Some($arp), gate: None } };
    ($name:expr, [$($l:expr),*], $fx:expr, gate $g:expr) => { Preset { name: $name, layers: &[$($l),*], fx: $fx, arp: None, gate: Some($g) } };
}

const SS: Layer = Layer { osc: Osc::SuperSaw, voices: 7, detune: 22.0, ..L };
const PLUCK_ENV: Layer = Layer { d: 0.35, s: 0.0, r: 0.3, fd: 0.18, fs: 0.0, ..L };
const PAD_ENV: Layer = Layer { a: 1.0, d: 1.0, s: 0.85, r: 2.5, ..L };

pub static PRESETS: &[Preset] = &[
    // Leads
    preset!("LD Supersaw Lead", [Layer { cutoff: 9000.0, s: 0.9, r: 0.35, ..SS }, Layer { osc: Osc::Saw, oct: 1, level: 0.25, cutoff: 7000.0, ..L }], Fx { delay: 0.22, reverb: 0.25, chorus: 0.15, ..FX }),
    preset!("LD Trance Lead", [Layer { voices: 5, detune: 15.0, cutoff: 6000.0, fenv: 1.5, fd: 0.4, fs: 0.6, ..SS }, Layer { osc: Osc::Square, oct: -1, level: 0.4, cutoff: 3000.0, ..L }], Fx { delay: 0.3, reverb: 0.3, ..FX }),
    preset!("LD Hoover", [Layer { detune: 45.0, cutoff: 5000.0, pitch: -7.0, pitch_time: 0.12, ..SS }, Layer { osc: Osc::Pulse, oct: -1, p1: 0.25, level: 0.6, cutoff: 4000.0, pitch: -7.0, pitch_time: 0.12, ..L }], Fx { chorus: 0.5, reverb: 0.2, dist: 0.2, ..FX }),
    preset!("LD Square Lead", [Layer { osc: Osc::Pulse, p1: 0.3, cutoff: 5000.0, ..L }, Layer { osc: Osc::Square, oct: 1, level: 0.3, cutoff: 6000.0, ..L }], Fx { delay: 0.2, reverb: 0.15, ..FX }),
    preset!("LD Fifth Saw", [Layer { voices: 2, detune: 8.0, cutoff: 4000.0, fenv: 2.0, fd: 0.3, fs: 0.5, ..L }, Layer { semi: 7.0, voices: 2, detune: 8.0, level: 0.5, cutoff: 4000.0, fenv: 2.0, fd: 0.3, fs: 0.5, ..L }], Fx { delay: 0.2, ..FX }),
    preset!("LD Chip Lead", [Layer { osc: Osc::Square, flt: 3, a: 0.001, r: 0.05, ..L }], Fx { delay: 0.15, delay_time: 4.0, ..FX }),
    preset!("LD Screamer", [Layer { detune: 30.0, cutoff: 3000.0, res: 0.45, ..SS }], Fx { dist: 0.5, delay: 0.2, reverb: 0.2, ..FX }),
    preset!("LD Harmonic Lead", [Layer { cutoff: 3500.0, fenv: 3.0, fd: 0.3, fs: 0.4, ..L }, Layer { osc: Osc::Table(1), p1: 0.4, level: 0.5, flt: 3, ..L }], Fx { delay: 0.25, reverb: 0.2, ..FX }),
    // Plucks
    preset!("PL Trance Pluck", [Layer { voices: 7, detune: 18.0, cutoff: 600.0, fenv: 4.5, ..Layer { osc: Osc::SuperSaw, ..PLUCK_ENV } }], Fx { delay: 0.35, reverb: 0.3, ..FX }),
    preset!("PL Saw Pluck", [Layer { voices: 2, detune: 10.0, cutoff: 400.0, fenv: 5.0, fd: 0.12, ..PLUCK_ENV }], Fx { delay: 0.3, reverb: 0.2, ..FX }),
    preset!("PL Bell Pluck", [Layer { osc: Osc::Fm, p1: 3.5, p2: 2.0, flt: 3, d: 0.5, s: 0.0, r: 0.4, ..L }, Layer { cutoff: 800.0, fenv: 3.5, level: 0.4, ..PLUCK_ENV }], Fx { delay: 0.3, reverb: 0.35, ..FX }),
    preset!("PL Harp", [Layer { osc: Osc::Pluck, p1: 0.7, p2: 2.0, flt: 3, s: 0.0, d: 3.0, r: 1.0, ..L }], Fx { reverb: 0.35, ..FX }),
    preset!("PL Hollow Pluck", [Layer { osc: Osc::Pulse, p1: 0.2, flt: 2, cutoff: 1200.0, res: 0.3, fenv: 2.0, ..PLUCK_ENV }], Fx { delay: 0.35, reverb: 0.25, ..FX }),
    preset!("PL Marimba", [Layer { osc: Osc::Fm, p1: 4.0, p2: 1.5, flt: 3, d: 0.4, s: 0.0, r: 0.3, ..L }, Layer { osc: Osc::Sine, level: 0.5, flt: 3, d: 0.5, s: 0.0, r: 0.3, ..L }], Fx { reverb: 0.25, ..FX }),
    preset!("PL Glass", [Layer { osc: Osc::Table(2), p1: 0.8, flt: 3, d: 0.8, s: 0.0, r: 0.6, ..L }, Layer { osc: Osc::Fm, p1: 7.0, p2: 0.8, level: 0.35, flt: 3, d: 0.6, s: 0.0, r: 0.5, ..L }], Fx { reverb: 0.45, delay: 0.25, ..FX }),
    // Pads
    preset!("PD Supersaw Pad", [Layer { detune: 30.0, cutoff: 3000.0, ..Layer { osc: Osc::SuperSaw, voices: 7, ..PAD_ENV } }], Fx { chorus: 0.3, reverb: 0.5, ..FX }),
    preset!("PD Warm Pad", [Layer { voices: 2, detune: 12.0, cutoff: 1500.0, ..PAD_ENV }, Layer { osc: Osc::Tri, oct: -1, level: 0.5, flt: 3, ..PAD_ENV }], Fx { reverb: 0.45, chorus: 0.2, ..FX }),
    preset!("PD Dark Pad", [Layer { voices: 3, detune: 20.0, cutoff: 700.0, res: 0.3, a: 2.0, r: 3.0, ..PAD_ENV }, Layer { osc: Osc::Noise, level: 0.06, cutoff: 1500.0, a: 2.0, r: 3.0, ..PAD_ENV }], Fx { reverb: 0.6, ..FX }),
    preset!("PD Choir", [Layer { osc: Osc::Table(2), p1: 0.3, voices: 3, detune: 10.0, flt: 3, a: 0.9, ..PAD_ENV }, Layer { osc: Osc::Table(2), p1: 0.6, semi: 12.0, level: 0.35, flt: 3, a: 0.9, ..PAD_ENV }], Fx { reverb: 0.5, chorus: 0.4, ..FX }),
    preset!("PD Strings", [Layer { voices: 5, detune: 12.0, cutoff: 4000.0, a: 0.6, r: 1.2, ..Layer { osc: Osc::SuperSaw, ..PAD_ENV } }, Layer { oct: -1, level: 0.3, cutoff: 2000.0, a: 0.6, r: 1.2, ..PAD_ENV }], Fx { chorus: 0.5, reverb: 0.4, ..FX }),
    preset!("PD Atmosphere", [Layer { osc: Osc::Table(4), p1: 0.4, p2: 0.4, flt: 3, a: 2.0, r: 4.0, ..PAD_ENV }, Layer { osc: Osc::Noise, level: 0.15, flt: 2, cutoff: 2500.0, res: 0.4, a: 2.0, r: 4.0, ..PAD_ENV }], Fx { reverb: 0.8, delay: 0.3, ..FX }),
    preset!("PD Evolving", [Layer { osc: Osc::Table(1), p1: 0.2, p2: 0.8, voices: 3, detune: 14.0, cutoff: 5000.0, a: 1.5, ..PAD_ENV }], Fx { reverb: 0.55, chorus: 0.3, ..FX }),
    // Basses
    preset!("BS Reese", [Layer { oct: -1, voices: 2, detune: 18.0, cutoff: 600.0, res: 0.2, a: 0.005, r: 0.1, ..L }, Layer { oct: -2, level: 0.5, cutoff: 300.0, a: 0.005, r: 0.1, ..L }], Fx { dist: 0.3, ..FX }),
    preset!("BS Sub", [Layer { osc: Osc::Sine, oct: -1, flt: 3, a: 0.002, r: 0.1, ..L }], FX),
    preset!("BS Saw Bass", [Layer { oct: -1, cutoff: 300.0, fenv: 3.0, fd: 0.2, fs: 0.3, r: 0.08, ..L }], FX),
    preset!("BS Offbeat Trance", [Layer { oct: -1, voices: 2, detune: 8.0, cutoff: 900.0, fenv: 2.0, fd: 0.12, fs: 0.2, d: 0.2, s: 0.6, r: 0.08, ..L }], Fx { dist: 0.1, ..FX }),
    preset!("BS FM Bass", [Layer { osc: Osc::Fm, oct: -1, p1: 1.0, p2: 3.0, flt: 3, d: 0.25, s: 0.4, r: 0.08, ..L }], FX),
    preset!("BS Acid Square", [Layer { osc: Osc::Pulse, oct: -1, p1: 0.4, cutoff: 500.0, res: 0.6, fenv: 3.5, fd: 0.2, fs: 0.0, r: 0.06, ..L }], Fx { dist: 0.25, ..FX }),
    // Arps
    preset!("AR Trance Arp", [Layer { voices: 5, detune: 12.0, cutoff: 900.0, fenv: 3.5, ..Layer { osc: Osc::SuperSaw, ..PLUCK_ENV } }], Fx { delay: 0.3, reverb: 0.3, ..FX }, arp (0, 2, 2, 0.5)),
    preset!("AR Saw Sequence", [Layer { cutoff: 1200.0, fenv: 2.0, ..PLUCK_ENV }], Fx { delay: 0.25, ..FX }, arp (2, 2, 1, 0.6)),
    preset!("AR Bell Arp", [Layer { osc: Osc::Fm, p1: 3.5, p2: 2.0, flt: 3, d: 0.5, s: 0.0, r: 0.4, ..L }], Fx { delay: 0.4, reverb: 0.35, ..FX }, arp (3, 2, 2, 0.6)),
    preset!("AR Plucky Run", [Layer { osc: Osc::Pluck, p1: 0.8, p2: 1.0, flt: 3, s: 0.0, d: 2.0, r: 0.5, ..L }], Fx { reverb: 0.35, delay: 0.2, ..FX }, arp (0, 0, 3, 0.4)),
    preset!("AR Chord Pulse", [Layer { voices: 5, detune: 15.0, cutoff: 1500.0, fenv: 2.5, ..Layer { osc: Osc::SuperSaw, ..PLUCK_ENV } }], Fx { delay: 0.25, reverb: 0.3, ..FX }, arp (4, 4, 1, 0.5)),
    // Trancegate
    preset!("GT Gated Supersaw", [Layer { detune: 25.0, cutoff: 6000.0, s: 1.0, r: 0.4, ..SS }], Fx { reverb: 0.3, delay: 0.2, ..FX }, gate (0, 1, 1.0)),
    preset!("GT Gated Pad", [Layer { voices: 2, detune: 12.0, cutoff: 2000.0, a: 0.4, r: 1.5, ..PAD_ENV }], Fx { reverb: 0.4, ..FX }, gate (1, 1, 0.85)),
    preset!("GT Gated Choir", [Layer { osc: Osc::Table(2), p1: 0.4, voices: 3, detune: 10.0, flt: 3, a: 0.3, ..PAD_ENV }], Fx { reverb: 0.5, ..FX }, gate (5, 1, 0.9)),
    preset!("GT Offbeat Chords", [Layer { detune: 20.0, cutoff: 4000.0, s: 1.0, ..SS }], Fx { reverb: 0.25, ..FX }, gate (2, 1, 1.0)),
    // Keys
    preset!("KY E-Piano", [Layer { osc: Osc::Fm, p1: 1.0, p2: 1.2, flt: 3, d: 1.5, s: 0.2, r: 0.5, ..L }, Layer { osc: Osc::Sine, level: 0.5, flt: 3, d: 2.0, s: 0.3, r: 0.5, ..L }], Fx { chorus: 0.35, reverb: 0.2, ..FX }),
    preset!("KY Organ", [Layer { osc: Osc::Organ, flt: 3, a: 0.002, r: 0.05, ..L }], Fx { chorus: 0.4, reverb: 0.15, ..FX }),
    preset!("KY Piano", [Layer { osc: Osc::Pluck, p1: 0.9, p2: 3.0, flt: 3, d: 3.0, s: 0.0, r: 0.4, ..L }, Layer { osc: Osc::Fm, p1: 2.0, p2: 0.5, level: 0.3, flt: 3, d: 0.6, s: 0.0, r: 0.3, ..L }], Fx { reverb: 0.25, ..FX }),
    preset!("KY Stab Piano", [Layer { cutoff: 2500.0, fenv: 2.0, fd: 0.15, d: 0.4, s: 0.0, r: 0.2, ..L }, Layer { osc: Osc::Square, level: 0.4, cutoff: 2500.0, d: 0.4, s: 0.0, r: 0.2, ..L }], Fx { reverb: 0.2, ..FX }),
    // Bells
    preset!("BL FM Bell", [Layer { osc: Osc::Fm, p1: 3.5, p2: 3.0, flt: 3, d: 2.5, s: 0.0, r: 1.5, ..L }], Fx { reverb: 0.35, ..FX }),
    preset!("BL Music Box", [Layer { osc: Osc::Fm, p1: 5.0, p2: 1.0, flt: 3, d: 1.2, s: 0.0, r: 0.8, ..L }, Layer { osc: Osc::Sine, oct: 1, level: 0.4, flt: 3, d: 1.0, s: 0.0, r: 0.8, ..L }], Fx { reverb: 0.4, ..FX }),
    preset!("BL Tubular", [Layer { osc: Osc::Fm, p1: 2.76, p2: 2.5, flt: 3, d: 4.0, s: 0.0, r: 2.0, ..L }], Fx { reverb: 0.45, ..FX }),
    // Synths / stabs
    preset!("SY Rave Stab", [Layer { detune: 25.0, cutoff: 3000.0, d: 0.3, s: 0.0, r: 0.25, ..SS }, Layer { semi: 7.0, voices: 3, detune: 20.0, level: 0.5, cutoff: 3000.0, d: 0.3, s: 0.0, r: 0.25, ..SS }], Fx { reverb: 0.3, ..FX }),
    preset!("SY Dub Chord", [Layer { voices: 2, detune: 6.0, cutoff: 900.0, fenv: 1.5, d: 0.25, s: 0.0, r: 0.25, ..L }, Layer { semi: 3.0, voices: 2, detune: 6.0, level: 0.7, cutoff: 900.0, fenv: 1.5, d: 0.25, s: 0.0, r: 0.25, ..L }, Layer { semi: 7.0, voices: 2, detune: 6.0, level: 0.7, cutoff: 900.0, fenv: 1.5, d: 0.25, s: 0.0, r: 0.25, ..L }, Layer { semi: 10.0, level: 0.5, cutoff: 900.0, fenv: 1.5, d: 0.25, s: 0.0, r: 0.25, ..L }], Fx { delay: 0.45, delay_time: 5.0, fb: 0.55, reverb: 0.4, ..FX }),
    preset!("SY Big Room Stab", [Layer { detune: 30.0, cutoff: 7000.0, d: 0.4, s: 0.1, r: 0.3, ..SS }, Layer { oct: -1, level: 0.5, cutoff: 3000.0, d: 0.4, s: 0.1, r: 0.3, ..L }], Fx { dist: 0.3, reverb: 0.3, ..FX }),
    preset!("SY Brass", [Layer { voices: 2, detune: 8.0, cutoff: 1200.0, fenv: 2.0, fa: 0.08, fd: 0.3, fs: 0.4, a: 0.05, r: 0.3, ..L }], Fx { reverb: 0.25, chorus: 0.2, ..FX }),
    preset!("SY Minor Chord Pad", [Layer { detune: 20.0, cutoff: 3500.0, ..Layer { osc: Osc::SuperSaw, voices: 5, ..PAD_ENV } }, Layer { semi: 3.0, detune: 20.0, level: 0.6, cutoff: 3500.0, ..Layer { osc: Osc::SuperSaw, voices: 5, ..PAD_ENV } }, Layer { semi: 7.0, detune: 20.0, level: 0.6, cutoff: 3500.0, ..Layer { osc: Osc::SuperSaw, voices: 5, ..PAD_ENV } }], Fx { reverb: 0.5, chorus: 0.2, ..FX }),
    // FX
    preset!("FX Riser", [Layer { osc: Osc::Noise, flt: 2, cutoff: 300.0, res: 0.4, fenv: 5.0, fa: 4.0, fd: 1.0, fs: 1.0, a: 3.0, r: 1.0, ..L }, Layer { pitch: -12.0, pitch_time: 4.0, level: 0.4, cutoff: 6000.0, a: 3.0, r: 1.0, ..SS }], Fx { reverb: 0.5, delay: 0.3, ..FX }),
    preset!("FX Laser", [Layer { pitch: 36.0, pitch_time: 0.15, cutoff: 6000.0, d: 0.3, s: 0.0, r: 0.2, ..L }], Fx { delay: 0.4, reverb: 0.3, ..FX }),
    preset!("FX Downlifter", [Layer { osc: Osc::Noise, cutoff: 200.0, fenv: 6.0, fd: 2.0, fs: 0.0, d: 2.5, s: 0.0, r: 1.0, ..L }], Fx { reverb: 0.6, ..FX }),
    preset!("FX Space Drone", [Layer { osc: Osc::Table(4), p1: 0.6, p2: 0.6, flt: 3, a: 3.0, r: 4.0, ..PAD_ENV }, Layer { osc: Osc::Noise, level: 0.1, cutoff: 900.0, a: 3.0, r: 4.0, ..PAD_ENV }], Fx { reverb: 0.9, delay: 0.3, ..FX }),
];

pub fn preset_names() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.name).collect()
}

pub const CATEGORIES: &[(&str, &str)] = &[
    ("LD", "Leads"),
    ("PL", "Plucks"),
    ("PD", "Pads"),
    ("BS", "Basses"),
    ("AR", "Arps"),
    ("GT", "Trancegates"),
    ("KY", "Keys"),
    ("BL", "Bells"),
    ("SY", "Synths"),
    ("FX", "FX"),
];

/// Parameter values a preset sets when loaded (arp, gate and effects sections).
pub fn preset_defaults(idx: usize) -> Vec<(usize, f32)> {
    let Some(p) = PRESETS.get(idx) else { return vec![] };
    let mut v = vec![
        (nexa::ARP_ON, if p.arp.is_some() { 1.0 } else { 0.0 }),
        (nexa::GATE_ON, if p.gate.is_some() { 1.0 } else { 0.0 }),
        (nexa::CHORUS, p.fx.chorus),
        (nexa::DELAY, p.fx.delay),
        (nexa::DELAY_TIME, p.fx.delay_time),
        (nexa::DELAY_FB, p.fx.fb),
        (nexa::REVERB, p.fx.reverb),
        (nexa::REVERB_SIZE, p.fx.size),
        (nexa::DIST, p.fx.dist),
        (nexa::LOW, p.fx.low),
        (nexa::HIGH, p.fx.high),
    ];
    if let Some((mode, rate, oct, gate)) = p.arp {
        v.extend([(nexa::ARP_MODE, mode as f32), (nexa::ARP_RATE, rate as f32), (nexa::ARP_OCT, oct as f32), (nexa::ARP_GATE, gate)]);
    }
    if let Some((pat, rate, depth)) = p.gate {
        v.extend([(nexa::GATE_PATTERN, pat as f32), (nexa::GATE_RATE, rate as f32), (nexa::GATE_DEPTH, depth)]);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Engine

const MAX_VOICES: usize = 12;
const MAX_UNISON: usize = 7;

struct LVoice {
    osc: [Osc_; MAX_UNISON],
    amp: Adsr,
    fenv: Adsr,
    fl: Svf,
    fr: Svf,
    fm_mod: f32,
    ks: DelayLine,
    ks_lp: f32,
    ks_excite: u32,
    pitch_env: f32,
    rng: Rng,
}

/// Alias so the oscillator struct doesn't clash with the `Osc` enum above.
type Osc_ = super::dsp::Osc;

impl LVoice {
    fn new(sr: f32, seed: u32) -> Self {
        Self {
            osc: [Osc_::default(); MAX_UNISON],
            amp: Adsr::default(),
            fenv: Adsr::default(),
            fl: Svf::default(),
            fr: Svf::default(),
            fm_mod: 0.0,
            ks: DelayLine::new((sr / 20.0) as usize),
            ks_lp: 0.0,
            ks_excite: 0,
            pitch_env: 0.0,
            rng: Rng::new(seed),
        }
    }
}

struct NVoice {
    active: bool,
    note: u8,
    freq: f32,
    vel: f32,
    gate: Option<u32>,
    held: bool,
    age: u64,
    layers: Vec<LVoice>,
}

struct Held {
    pitch: u8,
    vel: u8,
    remaining: Option<u32>,
    order: u64,
}

pub struct Nexa {
    sr: f32,
    p: Vec<f32>,
    preset: usize,
    voices: Vec<NVoice>,
    age: u64,
    bpm: f32,
    beat: f64,
    playing: bool,
    own_beat: f64,
    held: Vec<Held>,
    order: u64,
    arp_idx: usize,
    arp_last_step: i64,
    arp_rng: Rng,
    gate_level: f32,
    vib: f32,
    wt_lfo: f32,
    chorus: Box<dyn Effect>,
    delay: Box<dyn Effect>,
    reverb: Box<dyn Effect>,
    eq: [[Biquad; 2]; 2],
    fx_sig: Vec<f32>,
    bl: [f32; MAX_BLOCK],
    br: [f32; MAX_BLOCK],
}

impl Nexa {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            p: params::defaults(&params::NEXA_PARAMS),
            preset: 0,
            voices: (0..MAX_VOICES)
                .map(|i| NVoice { active: false, note: 0, freq: 440.0, vel: 1.0, gate: None, held: false, age: 0, layers: (0..4).map(|k| LVoice::new(sr, 1 + i as u32 * 7 + k)).collect() })
                .collect(),
            age: 0,
            bpm: 120.0,
            beat: 0.0,
            playing: false,
            own_beat: 0.0,
            held: vec![],
            order: 0,
            arp_idx: 0,
            arp_last_step: i64::MIN,
            arp_rng: Rng::new(99),
            gate_level: 1.0,
            vib: 0.0,
            wt_lfo: 0.0,
            chorus: effects::create(DeviceKind::Chorus, sr),
            delay: effects::create(DeviceKind::PingPong, sr),
            reverb: effects::create(DeviceKind::Reverb, sr),
            eq: Default::default(),
            fx_sig: vec![],
            bl: [0.0; MAX_BLOCK],
            br: [0.0; MAX_BLOCK],
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx { sr: self.sr, bpm: self.bpm, beat: self.beat, playing: self.playing }
    }

    fn apply_fx_params(&mut self) {
        let p = &self.p;
        let sig = vec![p[nexa::CHORUS], p[nexa::DELAY], p[nexa::DELAY_TIME], p[nexa::DELAY_FB], p[nexa::REVERB], p[nexa::REVERB_SIZE], p[nexa::LOW], p[nexa::HIGH]];
        if sig == self.fx_sig {
            return;
        }
        let ctx = self.ctx();
        // Chorus: rate, depth, delay, feedback, width, mix
        self.chorus.set_params(&[0.6, 0.5, 9.0, 0.1, 1.0, p[nexa::CHORUS] * 0.7], &ctx);
        // Ping pong: time, feedback, freq, width, mix
        self.delay.set_params(&[p[nexa::DELAY_TIME], p[nexa::DELAY_FB], 2500.0, 0.3, p[nexa::DELAY] * 0.6], &ctx);
        // Reverb: size, damping, predelay, low cut, width, mix
        self.reverb.set_params(&[p[nexa::REVERB_SIZE], 0.35, 15.0, 200.0, 1.0, p[nexa::REVERB] * 0.6], &ctx);
        for ch in &mut self.eq {
            ch[0].low_shelf(200.0, p[nexa::LOW], self.sr);
            ch[1].high_shelf(5000.0, p[nexa::HIGH], self.sr);
        }
        self.fx_sig = sig;
    }

    fn start_voice(&mut self, pitch: u8, vel: u8, gate: Option<u32>) {
        self.age += 1;
        let idx = self
            .voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| self.voices.iter().enumerate().filter(|(_, v)| !v.held && v.gate.is_none()).min_by_key(|(_, v)| v.age).map(|(i, _)| i))
            .unwrap_or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap());
        let preset = &PRESETS[self.preset.min(PRESETS.len() - 1)];
        let p = &self.p;
        let att = 2f32.powf(p[nexa::ATTACK] * 3.0);
        let rel = 2f32.powf(p[nexa::RELEASE] * 3.0);
        let sr = self.sr;
        let v = &mut self.voices[idx];
        v.active = true;
        v.note = pitch;
        v.freq = midi_to_freq(pitch as f32);
        v.vel = 0.35 + 0.65 * (vel as f32 / 127.0);
        v.gate = gate;
        v.held = gate.is_none();
        v.age = self.age;
        for (lv, layer) in v.layers.iter_mut().zip(preset.layers.iter()) {
            lv.amp.set((layer.a * att).min(10.0), layer.d, layer.s, (layer.r * rel).min(15.0), sr);
            lv.fenv.set(layer.fa, layer.fd, layer.fs, layer.r * rel, sr);
            lv.amp.gate_on();
            lv.fenv.gate_on();
            lv.pitch_env = 1.0;
            for (u, o) in lv.osc.iter_mut().enumerate() {
                o.phase = ((pitch as u32 * 13 + u as u32 * 37) % 101) as f32 / 101.0;
            }
            if layer.osc == Osc::Pluck {
                lv.ks_excite = (sr / v.freq.max(20.0)) as u32;
            }
        }
    }

    fn release_voices(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.note == pitch && v.held) {
            v.held = false;
            for lv in &mut v.layers {
                lv.amp.gate_off();
                lv.fenv.gate_off();
            }
        }
    }

    fn arp_sequence(&mut self) -> Vec<(u8, u8)> {
        let mode = self.p[nexa::ARP_MODE].round() as i32;
        let octaves = self.p[nexa::ARP_OCT].round().max(1.0) as u8;
        let mut base: Vec<&Held> = self.held.iter().collect();
        if mode == 5 {
            base.sort_by_key(|h| h.order);
        } else {
            base.sort_by_key(|h| h.pitch);
        }
        let mut seq = vec![];
        for o in 0..octaves {
            for h in &base {
                if let Some(p) = h.pitch.checked_add(o * 12).filter(|p| *p <= 127) {
                    seq.push((p, h.vel));
                }
            }
        }
        match mode {
            1 => seq.reverse(),
            2 if seq.len() > 2 => {
                let mut down: Vec<(u8, u8)> = seq[1..seq.len() - 1].to_vec();
                down.reverse();
                seq.extend(down);
            }
            _ => {}
        }
        seq
    }

    fn arp_tick(&mut self, gate_samples: u32) {
        let mode = self.p[nexa::ARP_MODE].round() as i32;
        if mode == 4 {
            // Chord: retrigger all held notes.
            let notes: Vec<(u8, u8)> = self.held.iter().map(|h| (h.pitch, h.vel)).collect();
            for (p, v) in notes {
                self.start_voice(p, v, Some(gate_samples));
            }
            return;
        }
        let seq = self.arp_sequence();
        if seq.is_empty() {
            return;
        }
        let (p, v) = if mode == 3 {
            seq[((self.arp_rng.next() * 0.5 + 0.5) * seq.len() as f32) as usize % seq.len()]
        } else {
            seq[self.arp_idx % seq.len()]
        };
        self.arp_idx += 1;
        self.start_voice(p, v, Some(gate_samples));
    }
}

impl InstrumentDsp for Nexa {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.p.clear();
        self.p.extend_from_slice(p);
        self.preset = (p[nexa::PRESET].round().max(0.0) as usize).min(PRESETS.len() - 1);
        self.apply_fx_params();
    }

    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }

    fn set_position(&mut self, beat: f64, playing: bool) {
        self.beat = beat;
        self.playing = playing;
    }

    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, _slide: bool) {
        if self.p[nexa::ARP_ON] > 0.5 {
            self.held.retain(|h| h.pitch != pitch);
            self.order += 1;
            self.held.push(Held { pitch, vel, remaining: gate, order: self.order });
            if self.held.len() == 1 {
                self.arp_idx = 0;
                self.arp_last_step = i64::MIN;
            }
            return;
        }
        self.start_voice(pitch, vel, gate);
    }

    fn note_off(&mut self, pitch: u8) {
        self.held.retain(|h| !(h.pitch == pitch && h.remaining.is_none()));
        self.release_voices(pitch);
    }

    fn all_off(&mut self) {
        self.held.clear();
        for v in &mut self.voices {
            v.held = false;
            for lv in &mut v.layers {
                lv.amp.gate_off();
                lv.fenv.gate_off();
            }
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let sr = self.sr;
        let p = self.p.clone();
        let preset = &PRESETS[self.preset.min(PRESETS.len() - 1)];
        let beat_inc = self.bpm as f64 / 60.0 / sr as f64;
        if !self.playing {
            self.beat = self.own_beat;
        }
        // Arpeggiator: trigger on rate boundaries within this block.
        let arp_on = p[nexa::ARP_ON] > 0.5;
        let rate = NEXA_ARP_RATE_BEATS[(p[nexa::ARP_RATE].round() as usize).min(5)];
        let arp_gate = (rate * 60.0 / self.bpm as f64 * sr as f64 * p[nexa::ARP_GATE] as f64) as u32;
        let gate_on = p[nexa::GATE_ON] > 0.5;
        let pattern = GATE_PATTERNS[(p[nexa::GATE_PATTERN].round() as usize).min(GATE_PATTERNS.len() - 1)].1.as_bytes();
        let grate = NEXA_GATE_RATE_BEATS[(p[nexa::GATE_RATE].round() as usize).min(2)];
        let gdepth = p[nexa::GATE_DEPTH];
        let gcoef = one_pole_coef(0.001 + p[nexa::GATE_SMOOTH] * 0.03, sr);
        let cut_mul = 2f32.powf(p[nexa::CUTOFF] * 4.0 + p[nexa::MODWHEEL] * 1.0);
        let res_add = p[nexa::RES] * 0.5;
        let vib_depth = p[nexa::MODWHEEL] * 0.35;
        let dist = p[nexa::DIST];
        let vol = p[nexa::VOLUME] * 0.3 / (preset.layers.len() as f32).sqrt();
        self.bl[..n].fill(0.0);
        self.br[..n].fill(0.0);

        // Advance clip-note holds for the arpeggiator.
        for h in &mut self.held {
            if let Some(rm) = h.remaining.as_mut() {
                *rm = rm.saturating_sub(n as u32);
            }
        }
        self.held.retain(|h| h.remaining != Some(0));

        for i in 0..n {
            let b = self.beat + i as f64 * beat_inc;
            if arp_on && !self.held.is_empty() {
                let step = (b / rate).floor() as i64;
                if step != self.arp_last_step {
                    self.arp_last_step = step;
                    self.arp_tick(arp_gate.max(1));
                }
            }
            if gate_on {
                let gstep = ((b / grate).floor() as i64).rem_euclid(16) as usize;
                let on = pattern.get(gstep).is_some_and(|c| *c == b'x');
                let target = if on { 1.0 } else { 1.0 - gdepth };
                self.gate_level = target + gcoef * (self.gate_level - target);
            } else {
                self.gate_level = 1.0;
            }
            self.vib = (self.vib + 5.5 / sr).fract();
            self.wt_lfo = (self.wt_lfo + 0.12 / sr).fract();
            let vib = 2f32.powf((self.vib * TAU).sin() * vib_depth / 12.0);
            let wt_lfo = (self.wt_lfo * TAU).sin();
            let (mut sl, mut sr_) = (0.0f32, 0.0f32);
            for v in self.voices.iter_mut().filter(|v| v.active) {
                if let Some(g) = v.gate.as_mut() {
                    if *g == 0 {
                        for lv in &mut v.layers {
                            lv.amp.gate_off();
                            lv.fenv.gate_off();
                        }
                        v.gate = None;
                    } else {
                        *g -= 1;
                    }
                }
                let mut any = false;
                for (lv, layer) in v.layers.iter_mut().zip(preset.layers.iter()) {
                    let a = lv.amp.next();
                    let fe = lv.fenv.next();
                    if !lv.amp.is_active() {
                        continue;
                    }
                    any = true;
                    if layer.pitch != 0.0 {
                        lv.pitch_env *= (-1.0 / (layer.pitch_time.max(0.001) * sr)).exp();
                    }
                    let semis = layer.oct as f32 * 12.0 + layer.semi + layer.pitch * lv.pitch_env;
                    let f = v.freq * 2f32.powf(semis / 12.0) * vib;
                    let (mut x, mut y) = match layer.osc {
                        Osc::Saw | Osc::SuperSaw => {
                            let nu = (layer.voices as usize).clamp(1, MAX_UNISON);
                            let (mut a_, mut b_) = (0.0, 0.0);
                            for u in 0..nu {
                                let s = if nu == 1 { 0.0 } else { u as f32 / (nu - 1) as f32 * 2.0 - 1.0 };
                                let o = lv.osc[u].next(Wave::Saw, f * 2f32.powf(s * layer.detune / 1200.0), sr);
                                let w = if u == nu / 2 { 1.0 } else { 0.75 };
                                a_ += o * w * (1.0 - s * 0.7);
                                b_ += o * w * (1.0 + s * 0.7);
                            }
                            let norm = 1.0 / (nu as f32).sqrt();
                            (a_ * norm, b_ * norm)
                        }
                        Osc::Square => {
                            let o = lv.osc[0].next(Wave::Square, f, sr);
                            (o, o)
                        }
                        Osc::Pulse => {
                            let a_ = lv.osc[0].next(Wave::Saw, f, sr);
                            lv.osc[1].phase = (lv.osc[0].phase + layer.p1.clamp(0.05, 0.95)).fract();
                            let b_ = lv.osc[1].next(Wave::Saw, f, sr);
                            let o = (a_ - b_) * 0.6;
                            (o, o)
                        }
                        Osc::Sine => {
                            let o = lv.osc[0].sine(f, sr);
                            (o, o)
                        }
                        Osc::Tri => {
                            let o = lv.osc[0].next(Wave::Triangle, f, sr);
                            (o, o)
                        }
                        Osc::Fm => {
                            let m = lv.osc[1].sine(f * layer.p1, sr);
                            let idx = layer.p2 * (0.3 + 0.7 * fe);
                            let ph = lv.osc[0].phase;
                            let o = (ph * TAU + m * idx * 2.0).sin();
                            lv.osc[0].phase = (ph + f / sr).fract();
                            (o, o)
                        }
                        Osc::Noise => {
                            let o = lv.rng.next() * 0.7;
                            (o, lv.rng.next() * 0.7)
                        }
                        Osc::Pluck => {
                            let period = (sr / f.max(20.0)).max(2.0);
                            let ex = if lv.ks_excite > 0 {
                                lv.ks_excite -= 1;
                                lv.rng.next()
                            } else {
                                0.0
                            };
                            let yv = lv.ks.read(period);
                            let g = (-6.9 / (layer.p2.max(0.05) * f)).exp();
                            let bright = 0.2 + layer.p1 * 0.75;
                            lv.ks_lp = yv * bright + lv.ks_lp * (1.0 - bright);
                            lv.ks.write(ex + lv.ks_lp * g);
                            (yv, yv)
                        }
                        Osc::Table(t) => {
                            let pos = (layer.p1 + wt_lfo * layer.p2 * 0.3).clamp(0.0, 1.0);
                            let mip = mip_for(f, sr);
                            let nu = (layer.voices as usize).clamp(1, MAX_UNISON);
                            let (mut a_, mut b_) = (0.0, 0.0);
                            for u in 0..nu {
                                let s = if nu == 1 { 0.0 } else { u as f32 / (nu - 1) as f32 * 2.0 - 1.0 };
                                let ph = lv.osc[u].phase;
                                let o = wt_read((t as usize).min(params::WAVETABLES.len() - 1), mip, pos, ph);
                                lv.osc[u].phase = (ph + f * 2f32.powf(s * layer.detune / 1200.0) / sr).fract();
                                a_ += o * (1.0 - s * 0.6);
                                b_ += o * (1.0 + s * 0.6);
                            }
                            let norm = 1.0 / (nu as f32).sqrt();
                            (a_ * norm, b_ * norm)
                        }
                        Osc::Organ => {
                            let ph = lv.osc[0].phase;
                            let o = (ph * TAU).sin() + 0.6 * (ph * 2.0 * TAU).sin() + 0.4 * (ph * 3.0 * TAU).sin() + 0.25 * (ph * 4.0 * TAU).sin() + 0.15 * (ph * 8.0 * TAU).sin();
                            lv.osc[0].phase = (ph + f / sr).fract();
                            (o * 0.45, o * 0.45)
                        }
                    };
                    if layer.flt < 3 {
                        if i % 16 == 0 {
                            let fc = (layer.cutoff * cut_mul * 2f32.powf(layer.fenv * fe)).clamp(20.0, 19000.0);
                            let q = res_to_q((layer.res + res_add).clamp(0.0, 0.95));
                            lv.fl.set(fc, q, sr);
                            lv.fr.set(fc, q, sr);
                        }
                        let mode = match layer.flt {
                            1 => FilterMode::High,
                            2 => FilterMode::Band,
                            _ => FilterMode::Low,
                        };
                        x = lv.fl.process(x, mode);
                        y = lv.fr.process(y, mode);
                    }
                    let g = a * layer.level * v.vel;
                    let (pl, pr) = pan_gains(layer.pan);
                    sl += x * g * pl;
                    sr_ += y * g * pr;
                    lv.fm_mod = 0.0;
                }
                if !any {
                    v.active = false;
                }
            }
            let gl = self.gate_level;
            self.bl[i] = sl * vol * gl;
            self.br[i] = sr_ * vol * gl;
        }
        self.beat += n as f64 * beat_inc;
        if !self.playing {
            self.own_beat = self.beat;
        }

        // Effects: distortion -> chorus -> delay -> reverb -> EQ.
        if dist > 0.0 {
            let pre = 1.0 + dist * 10.0;
            let post = 1.0 / (1.0 + dist * 2.0);
            for i in 0..n {
                self.bl[i] = soft_clip(self.bl[i] * pre) * post;
                self.br[i] = soft_clip(self.br[i] * pre) * post;
            }
        }
        let ctx = self.ctx();
        if p[nexa::CHORUS] > 0.001 {
            self.chorus.process(&mut self.bl[..n], &mut self.br[..n], &ctx);
        }
        if p[nexa::DELAY] > 0.001 {
            self.delay.process(&mut self.bl[..n], &mut self.br[..n], &ctx);
        }
        if p[nexa::REVERB] > 0.001 {
            self.reverb.process(&mut self.bl[..n], &mut self.br[..n], &ctx);
        }
        let eq_on = p[nexa::LOW].abs() > 0.05 || p[nexa::HIGH].abs() > 0.05;
        for i in 0..n {
            let (mut a, mut b) = (self.bl[i], self.br[i]);
            if eq_on {
                let a0 = self.eq[0][0].process(a);
                a = self.eq[0][1].process(a0);
                let b0 = self.eq[1][0].process(b);
                b = self.eq[1][1].process(b0);
            }
            l[i] += a;
            r[i] += b;
        }
    }
}
