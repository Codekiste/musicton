//! Instrument presets, clip patterns and the demo project.

use crate::model::*;
use crate::params::{self, bass, lane_param, operator, simpler, synth, tension, wavetable, LANE_DECAY, LANE_LEVEL, LANE_TONE, LANE_TUNE, OP_PARAMS};

pub struct InstrumentPreset {
    pub name: &'static str,
    pub kind: InstrumentKind,
    pub info: &'static str,
    overrides: fn() -> Vec<(usize, f32)>,
}

impl InstrumentPreset {
    pub fn build(&self) -> Instrument {
        let mut inst = Instrument::new(self.kind);
        for (i, v) in (self.overrides)() {
            inst.params[i] = v;
        }
        inst
    }
}

pub static INSTRUMENT_PRESETS: &[InstrumentPreset] = &[
    InstrumentPreset {
        name: "909 Punch",
        kind: InstrumentKind::Drums,
        info: "Classic punchy 909-style kit. Ten synthesized lanes: kick, clap, snare, rim, hats, ride, toms and perc.",
        overrides: || vec![],
    },
    InstrumentPreset {
        name: "Hard Industrial",
        kind: InstrumentKind::Drums,
        info: "Distorted, long and aggressive kit for warehouse and industrial techno.",
        overrides: || {
            vec![
                (0, 0.6),
                (lane_param(0, LANE_DECAY), 1.5),
                (lane_param(0, LANE_TONE), 0.85),
                (lane_param(0, LANE_TUNE), -2.0),
                (lane_param(1, LANE_DECAY), 1.6),
                (lane_param(2, LANE_TONE), 0.9),
                (lane_param(4, LANE_TONE), 0.8),
                (lane_param(5, LANE_DECAY), 1.4),
            ]
        },
    },
    InstrumentPreset {
        name: "Minimal Tight",
        kind: InstrumentKind::Drums,
        info: "Short, dry and clicky kit for minimal and micro-house grooves.",
        overrides: || {
            let mut v = vec![(0, 0.0)];
            for lane in 0..10 {
                v.push((lane_param(lane, LANE_DECAY), 0.6));
            }
            v.push((lane_param(0, LANE_TUNE), 2.0));
            v.push((lane_param(3, LANE_LEVEL), 0.9));
            v
        },
    },
    InstrumentPreset {
        name: "B-303 Classic",
        kind: InstrumentKind::B303,
        info: "The classic acid bassline: saw wave, high resonance, medium env mod.",
        overrides: || b303_core::spec::PRESETS[0].2.to_vec(),
    },
    InstrumentPreset {
        name: "B-303 Squelch",
        kind: InstrumentKind::B303,
        info: "Screaming resonance, deep env mod and strong accents - automate Cutoff.",
        overrides: || b303_core::spec::PRESETS[1].2.to_vec(),
    },
    InstrumentPreset {
        name: "B-303 Square",
        kind: InstrumentKind::B303,
        info: "Hollow square-wave 303.",
        overrides: || b303_core::spec::PRESETS[2].2.to_vec(),
    },
    InstrumentPreset {
        name: "B-303 Distorted",
        kind: InstrumentKind::B303,
        info: "Overdriven acid for hard techno.",
        overrides: || b303_core::spec::PRESETS[3].2.to_vec(),
    },
    InstrumentPreset {
        name: "B-303 Deep",
        kind: InstrumentKind::B303,
        info: "Low, round bassline with little resonance and long decay.",
        overrides: || b303_core::spec::PRESETS[4].2.to_vec(),
    },
    InstrumentPreset {
        name: "Nexa",
        kind: InstrumentKind::Nexa,
        info: "preset ROMpler. Pick a sound from the preset browser in the Device Chain.",
        overrides: || vec![],
    },
    InstrumentPreset {
        name: "TB Acid",
        kind: InstrumentKind::Bass,
        info: "Squelchy 303-style acid line. Accents (velocity >= 110) open the filter, slides glide into the next note.",
        overrides: || {
            vec![
                (bass::CUTOFF, 380.0),
                (bass::RES, 0.78),
                (bass::ENV_MOD, 0.7),
                (bass::DECAY, 0.25),
                (bass::ACCENT, 0.7),
                (bass::DRIVE, 0.5),
            ]
        },
    },
    InstrumentPreset {
        name: "Square Acid",
        kind: InstrumentKind::Bass,
        info: "Hollow square-wave acid bass.",
        overrides: || vec![(bass::WAVE, 1.0), (bass::CUTOFF, 300.0), (bass::RES, 0.7), (bass::ENV_MOD, 0.6), (bass::DRIVE, 0.4)],
    },
    InstrumentPreset {
        name: "Rolling Sub",
        kind: InstrumentKind::Bass,
        info: "Dark, rounded rolling bass with sub layer. Pairs well with a Sidechain device.",
        overrides: || {
            vec![
                (bass::CUTOFF, 220.0),
                (bass::RES, 0.2),
                (bass::ENV_MOD, 0.3),
                (bass::DECAY, 0.15),
                (bass::SUB, 0.6),
                (bass::DRIVE, 0.25),
                (bass::OCTAVE, 0.0),
            ]
        },
    },
    InstrumentPreset {
        name: "Dub Stab",
        kind: InstrumentKind::Synth,
        info: "Short filtered chord stab - send it into the delay and reverb returns for dub techno.",
        overrides: || {
            vec![
                (synth::DETUNE, 8.0),
                (synth::VOICES, 3.0),
                (synth::CUTOFF, 700.0),
                (synth::RES, 0.3),
                (synth::ENV_AMT, 0.35),
                (synth::ATTACK, 0.002),
                (synth::DECAY, 0.22),
                (synth::SUSTAIN, 0.0),
                (synth::RELEASE, 0.25),
                (synth::VOLUME, 0.5),
            ]
        },
    },
    InstrumentPreset {
        name: "Warehouse Pad",
        kind: InstrumentKind::Synth,
        info: "Wide detuned pad with slow attack for breakdowns.",
        overrides: || {
            vec![
                (synth::DETUNE, 20.0),
                (synth::VOICES, 5.0),
                (synth::CUTOFF, 1100.0),
                (synth::RES, 0.1),
                (synth::ENV_AMT, 0.15),
                (synth::ATTACK, 1.2),
                (synth::DECAY, 1.5),
                (synth::SUSTAIN, 0.8),
                (synth::RELEASE, 2.5),
                (synth::VOLUME, 0.35),
            ]
        },
    },
    InstrumentPreset {
        name: "Hoover Lead",
        kind: InstrumentKind::Synth,
        info: "Rave hoover: heavily detuned saws with sub.",
        overrides: || {
            vec![
                (synth::DETUNE, 45.0),
                (synth::VOICES, 5.0),
                (synth::SUB, 0.5),
                (synth::CUTOFF, 4000.0),
                (synth::ENV_AMT, 0.2),
                (synth::ATTACK, 0.03),
                (synth::SUSTAIN, 0.7),
                (synth::RELEASE, 0.4),
            ]
        },
    },
    InstrumentPreset {
        name: "Pluck",
        kind: InstrumentKind::Synth,
        info: "Short percussive pluck for arps and hooks.",
        overrides: || {
            vec![
                (synth::WAVE, 1.0),
                (synth::VOICES, 1.0),
                (synth::CUTOFF, 500.0),
                (synth::ENV_AMT, 0.6),
                (synth::DECAY, 0.18),
                (synth::SUSTAIN, 0.0),
                (synth::RELEASE, 0.15),
            ]
        },
    },
    InstrumentPreset {
        name: "Sampler",
        kind: InstrumentKind::Sampler,
        info: "Empty sampler. Drop a WAV from the Samples browser onto it, then play it across the keyboard.",
        overrides: || vec![],
    },
    InstrumentPreset {
        name: "Loop Slicer",
        kind: InstrumentKind::Sampler,
        info: "Slice mode: the sample is cut into 16 slices on C1 upwards, synced to the tempo. Load a drum break and rearrange it.",
        overrides: || vec![(simpler::MODE, 2.0), (simpler::SLICES, 16.0), (simpler::WARP, 1.0), (simpler::BEATS, 4.0), (simpler::RELEASE, 0.02)],
    },
    InstrumentPreset {
        name: "Sampled Keys",
        kind: InstrumentKind::Sampler,
        info: "Classic mode with loop and a soft envelope for playing samples like a keyboard.",
        overrides: || vec![(simpler::LOOP, 1.0), (simpler::ATTACK, 0.01), (simpler::RELEASE, 0.4), (simpler::CUTOFF, 6000.0)],
    },
    InstrumentPreset {
        name: "FM Bell",
        kind: InstrumentKind::Fm4,
        info: "Glassy FM bell - inharmonic ratio with a long decay.",
        overrides: || {
            vec![
                (OP_PARAMS, 3.5),
                (OP_PARAMS + 2, 0.55),
                (OP_PARAMS + 4, 2.5),
                (OP_PARAMS + 5, 0.0),
                (4, 3.0),
                (5, 0.0),
                (6, 2.0),
                (operator::VOLUME, 0.45),
            ]
        },
    },
    InstrumentPreset {
        name: "FM Bass",
        kind: InstrumentKind::Fm4,
        info: "Punchy FM bass with a snappy modulator envelope.",
        overrides: || {
            vec![
                (OP_PARAMS, 1.0),
                (OP_PARAMS + 2, 0.7),
                (OP_PARAMS + 4, 0.18),
                (OP_PARAMS + 5, 0.1),
                (4, 0.5),
                (5, 0.7),
                (6, 0.08),
                (operator::CUTOFF, 2500.0),
                (operator::FEEDBACK, 0.0),
            ]
        },
    },
    InstrumentPreset {
        name: "FM E-Piano",
        kind: InstrumentKind::Fm4,
        info: "DX-style electric piano: two operator pairs.",
        overrides: || {
            vec![
                (operator::ALGO, 2.0),
                (OP_PARAMS, 14.0),
                (OP_PARAMS + 2, 0.22),
                (OP_PARAMS + 4, 0.25),
                (OP_PARAMS + 5, 0.0),
                (2 * OP_PARAMS, 1.0),
                (2 * OP_PARAMS + 2, 0.6),
                (2 * OP_PARAMS + 4, 1.5),
                (2 * OP_PARAMS + 5, 0.3),
                (3 * OP_PARAMS, 1.0),
                (3 * OP_PARAMS + 2, 0.35),
                (3 * OP_PARAMS + 4, 1.0),
                (4, 2.0),
                (5, 0.2),
                (6, 0.6),
                (operator::SPREAD, 0.4),
            ]
        },
    },
    InstrumentPreset {
        name: "FM Metal Perc",
        kind: InstrumentKind::Fm4,
        info: "Short metallic hit for industrial percussion.",
        overrides: || {
            vec![
                (OP_PARAMS, 7.0),
                (OP_PARAMS + 1, 37.0),
                (OP_PARAMS + 2, 0.9),
                (OP_PARAMS + 4, 0.12),
                (OP_PARAMS + 5, 0.0),
                (4, 0.25),
                (5, 0.0),
                (6, 0.1),
                (operator::FEEDBACK, 0.4),
                (3 * OP_PARAMS + 2, 0.4),
                (3 * OP_PARAMS, 11.0),
            ]
        },
    },
    InstrumentPreset {
        name: "Morph Pad",
        kind: InstrumentKind::Morph,
        info: "Evolving pad - the LFO slowly moves through the formant table.",
        overrides: || {
            vec![
                (wavetable::T1, 2.0),
                (wavetable::T2, 0.0),
                (wavetable::ATTACK, 1.0),
                (wavetable::RELEASE, 2.5),
                (wavetable::SUSTAIN, 0.8),
                (wavetable::LFO_POS, 0.6),
                (wavetable::LFO_RATE, params::div_index("4 bars") as f32),
                (wavetable::UNISON, 5.0),
                (wavetable::SPREAD, 20.0),
                (wavetable::CUTOFF, 2500.0),
            ]
        },
    },
    InstrumentPreset {
        name: "Morph Lead",
        kind: InstrumentKind::Morph,
        info: "Bright detuned lead.",
        overrides: || vec![(wavetable::P1, 0.7), (wavetable::MIX, 0.5), (wavetable::CUTOFF, 6000.0), (wavetable::FENV, 0.4), (wavetable::LFO_POS, 0.0)],
    },
    InstrumentPreset {
        name: "Digital Bass",
        kind: InstrumentKind::Morph,
        info: "Gritty digital bass with an envelope sweeping the table.",
        overrides: || {
            vec![
                (wavetable::T1, 4.0),
                (wavetable::UNISON, 1.0),
                (wavetable::MIX, 0.0),
                (wavetable::SUB, 0.5),
                (wavetable::DECAY, 0.25),
                (wavetable::SUSTAIN, 0.3),
                (wavetable::ENV_POS, 0.6),
                (wavetable::LFO_POS, 0.0),
                (wavetable::CUTOFF, 1800.0),
            ]
        },
    },
    InstrumentPreset {
        name: "Plucked String",
        kind: InstrumentKind::StringModel,
        info: "Karplus-Strong plucked string.",
        overrides: || vec![],
    },
    InstrumentPreset {
        name: "Hammered Dulcimer",
        kind: InstrumentKind::StringModel,
        info: "Bright hammered string with stiffness.",
        overrides: || vec![(tension::EXCITER, 1.0), (tension::INHARM, 0.3), (tension::DAMPING, 0.2), (tension::BRIGHT, 0.9)],
    },
    InstrumentPreset {
        name: "Bowed Drone",
        kind: InstrumentKind::StringModel,
        info: "Continuously bowed string - dark techno drones.",
        overrides: || vec![(tension::EXCITER, 2.0), (tension::DECAY, 0.9), (tension::DAMPING, 0.6), (tension::VIBRATO, 0.3), (tension::BODY, 0.6)],
    },
];

pub fn preset(name: &str) -> &'static InstrumentPreset {
    INSTRUMENT_PRESETS.iter().find(|p| p.name == name).unwrap_or(&INSTRUMENT_PRESETS[0])
}

// ---------------------------------------------------------------------------------------------
// Patterns

pub struct Pattern {
    pub name: &'static str,
    pub kind: InstrumentKind,
    pub bars: u32,
    build: fn() -> Vec<Note>,
}

impl Pattern {
    pub fn clip(&self, color: [u8; 3]) -> Clip {
        let mut c = Clip::new(self.name, color, self.bars);
        c.notes = (self.build)();
        c
    }
}

const K: u8 = 0;
const CP: u8 = 1;
const SN: u8 = 2;
const RIM: u8 = 3;
const CH: u8 = 4;
const OH: u8 = 5;
const RD: u8 = 6;
const TL: u8 = 7;
const TH: u8 = 8;
const PC: u8 = 9;

fn n(step: u32, len: u32, pitch: u8, vel: u8) -> Note {
    Note { step, len, pitch, vel, slide: false }
}
fn hits(lane: u8, steps: &[u32], vel: u8) -> Vec<Note> {
    steps.iter().map(|&s| n(s, 1, lane, vel)).collect()
}
fn every(lane: u8, start: u32, interval: u32, total: u32, vel: u8) -> Vec<Note> {
    (start..total).step_by(interval as usize).map(|s| n(s, 1, lane, vel)).collect()
}
fn chord(step: u32, len: u32, pitches: &[u8], vel: u8) -> Vec<Note> {
    pitches.iter().map(|&p| n(step, len, p, vel)).collect()
}
fn acid(steps: &[(u32, u8, bool, bool)]) -> Vec<Note> {
    steps.iter().map(|&(s, p, acc, slide)| Note { step: s, len: 1, pitch: p, vel: if acc { 120 } else { 90 }, slide }).collect()
}

pub static PATTERNS: &[Pattern] = &[
    Pattern {
        name: "Four to the Floor",
        kind: InstrumentKind::Drums,
        bars: 1,
        build: || {
            let mut v = every(K, 0, 4, 16, 120);
            v.extend(every(OH, 2, 4, 16, 90));
            v.extend(hits(CP, &[4, 12], 100));
            v.extend(every(CH, 0, 2, 16, 55));
            v
        },
    },
    Pattern {
        name: "Rolling Techno",
        kind: InstrumentKind::Drums,
        bars: 2,
        build: || {
            let mut v = every(K, 0, 4, 32, 120);
            v.extend(every(OH, 2, 4, 32, 85));
            for s in 0..32 {
                v.push(n(s, 1, CH, if s % 4 == 2 { 40 } else if s % 2 == 1 { 75 } else { 55 }));
            }
            v.extend(hits(RIM, &[3, 11, 14, 19, 27, 30], 80));
            v.extend(hits(CP, &[4, 12, 20, 28], 95));
            v.extend(hits(PC, &[7, 23, 29], 70));
            v
        },
    },
    Pattern {
        name: "Offbeat Hats",
        kind: InstrumentKind::Drums,
        bars: 1,
        build: || {
            let mut v = every(OH, 2, 4, 16, 100);
            v.extend(every(CH, 1, 2, 16, 60));
            v
        },
    },
    Pattern {
        name: "Broken Kick",
        kind: InstrumentKind::Drums,
        bars: 2,
        build: || {
            let mut v = hits(K, &[0, 6, 10, 16, 19, 26, 28], 120);
            v.extend(hits(CP, &[4, 12, 20, 28], 100));
            v.extend(every(CH, 0, 2, 32, 60));
            v.extend(hits(TL, &[14, 30], 90));
            v
        },
    },
    Pattern {
        name: "Industrial Stomp",
        kind: InstrumentKind::Drums,
        bars: 1,
        build: || {
            let mut v = every(K, 0, 4, 16, 127);
            v.extend(hits(K, &[14], 90));
            v.extend(hits(SN, &[4, 12], 110));
            v.extend(hits(CP, &[4, 12], 100));
            v.extend(every(CH, 0, 1, 16, 70));
            v.extend(hits(TL, &[3, 11], 80));
            v.extend(hits(PC, &[6, 15], 75));
            v
        },
    },
    Pattern {
        name: "Minimal Groove",
        kind: InstrumentKind::Drums,
        bars: 1,
        build: || {
            let mut v = every(K, 0, 4, 16, 115);
            v.extend(hits(RIM, &[3, 6, 11, 14], 85));
            v.extend(hits(CH, &[2, 6, 10, 13, 14], 70));
            v.extend(hits(PC, &[9], 70));
            v
        },
    },
    Pattern {
        name: "Ride Groove",
        kind: InstrumentKind::Drums,
        bars: 1,
        build: || {
            let mut v = every(RD, 0, 2, 16, 70);
            v.extend(every(RD, 2, 4, 16, 100));
            v
        },
    },
    Pattern {
        name: "Kick Only",
        kind: InstrumentKind::Drums,
        bars: 1,
        build: || every(K, 0, 4, 16, 120),
    },
    Pattern {
        name: "Snare Build",
        kind: InstrumentKind::Drums,
        bars: 4,
        build: || {
            let mut v = vec![];
            for s in (0..32).step_by(4) {
                v.push(n(s, 1, SN, 60 + s as u8));
            }
            for s in (32..48).step_by(2) {
                v.push(n(s, 1, SN, 80 + (s - 32) as u8));
            }
            for s in 48..64 {
                v.push(n(s, 1, SN, 95 + (s - 48) as u8 * 2));
            }
            v.extend(every(K, 0, 4, 48, 115));
            v.extend(hits(TH, &[60, 62], 100));
            v
        },
    },
    Pattern {
        name: "Offbeat Bass",
        kind: InstrumentKind::Bass,
        bars: 1,
        build: || [2, 6, 10, 14].iter().map(|&s| n(s, 2, 33, 100)).collect(),
    },
    Pattern {
        name: "Rolling 16ths",
        kind: InstrumentKind::Bass,
        bars: 1,
        build: || {
            (0..16)
                .filter(|s| s % 4 != 0)
                .map(|s| n(s, 1, if s == 7 || s == 15 { 45 } else { 33 }, if s % 4 == 2 { 110 } else { 85 }))
                .collect()
        },
    },
    Pattern {
        name: "Acid Line 1",
        kind: InstrumentKind::Bass,
        bars: 1,
        build: || {
            acid(&[
                (0, 45, true, false),
                (1, 45, false, false),
                (2, 57, false, true),
                (3, 55, false, false),
                (5, 45, true, false),
                (6, 48, false, true),
                (7, 50, false, false),
                (8, 45, false, false),
                (10, 57, true, false),
                (11, 45, false, false),
                (12, 52, false, true),
                (13, 53, false, false),
                (14, 45, true, false),
                (15, 55, false, false),
            ])
        },
    },
    Pattern {
        name: "Acid Line 2",
        kind: InstrumentKind::Bass,
        bars: 2,
        build: || {
            acid(&[
                (0, 45, true, false),
                (2, 45, false, true),
                (3, 57, false, false),
                (4, 45, false, false),
                (6, 60, true, true),
                (7, 57, false, false),
                (9, 45, false, false),
                (10, 48, true, false),
                (12, 45, false, true),
                (13, 43, false, false),
                (14, 45, false, false),
                (16, 45, true, false),
                (18, 57, false, true),
                (19, 55, false, true),
                (20, 53, false, false),
                (22, 45, true, false),
                (24, 48, false, true),
                (25, 50, false, false),
                (26, 45, false, false),
                (28, 57, true, false),
                (30, 55, false, true),
                (31, 52, false, false),
            ])
        },
    },
    Pattern {
        name: "Hypnotic 3-Step",
        kind: InstrumentKind::Bass,
        bars: 1,
        build: || (0..16).step_by(3).enumerate().map(|(i, s)| n(s as u32, 2, if i % 2 == 0 { 45 } else { 52 }, 100)).collect(),
    },
    Pattern {
        name: "Slices In Order",
        kind: InstrumentKind::Sampler,
        bars: 1,
        build: || (0..16).map(|s| n(s, 1, 36 + s as u8, 100)).collect(),
    },
    Pattern {
        name: "Slice Shuffle",
        kind: InstrumentKind::Sampler,
        bars: 1,
        build: || {
            let order = [0u8, 1, 2, 3, 0, 1, 6, 7, 8, 9, 2, 3, 12, 6, 14, 15];
            order.iter().enumerate().map(|(s, &o)| n(s as u32, 1, 36 + o, if s % 4 == 0 { 115 } else { 90 })).collect()
        },
    },
    Pattern {
        name: "Minor Stabs",
        kind: InstrumentKind::Synth,
        bars: 1,
        build: || {
            let mut v = chord(3, 1, &[57, 60, 64, 67], 100);
            v.extend(chord(10, 1, &[57, 60, 64, 67], 90));
            v
        },
    },
    Pattern {
        name: "Dub Chord",
        kind: InstrumentKind::Synth,
        bars: 2,
        build: || {
            let mut v = chord(2, 1, &[57, 60, 64, 67, 71], 100);
            v.extend(chord(18, 1, &[57, 60, 64, 67, 71], 95));
            v.extend(chord(27, 1, &[55, 59, 62, 66], 85));
            v
        },
    },
    Pattern {
        name: "Pad Am-F-C-G",
        kind: InstrumentKind::Synth,
        bars: 4,
        build: || {
            let mut v = chord(0, 16, &[57, 60, 64], 90);
            v.extend(chord(16, 16, &[53, 57, 60], 90));
            v.extend(chord(32, 16, &[55, 60, 64], 90));
            v.extend(chord(48, 16, &[55, 59, 62], 90));
            v
        },
    },
    Pattern {
        name: "Arp Am",
        kind: InstrumentKind::Synth,
        bars: 1,
        build: || {
            let notes = [57, 60, 64, 69, 64, 60, 57, 64];
            (0..16).map(|s| n(s, 1, notes[s as usize % notes.len()], if s % 4 == 0 { 110 } else { 80 })).collect()
        },
    },
];

pub fn pattern(name: &str) -> Option<&'static Pattern> {
    PATTERNS.iter().find(|p| p.name == name)
}

// ---------------------------------------------------------------------------------------------
// Demo project

pub fn new_project() -> Project {
    let mut p = Project::empty();
    let id = p.add_track("Drums", InstrumentKind::Drums, CLIP_COLORS[1]);
    p.track_mut(id).unwrap().instrument = preset("909 Punch").build();
    p
}

pub fn demo_project() -> Project {
    let mut p = Project::empty();
    p.name = "Warehouse Demo".into();
    p.bpm = 130.0;
    let scene_names = ["Intro", "Groove", "Acid", "Break", "Peak", "Outro", "7", "8"];
    for (s, name) in p.scenes.iter_mut().zip(scene_names) {
        s.name = name.into();
    }

    let tracks: [(&str, &str, [u8; 3]); 5] = [
        ("Drums", "909 Punch", CLIP_COLORS[1]),
        ("Sub Bass", "Rolling Sub", CLIP_COLORS[9]),
        ("Acid", "TB Acid", CLIP_COLORS[4]),
        ("Stabs", "Dub Stab", CLIP_COLORS[11]),
        ("Pad", "Warehouse Pad", CLIP_COLORS[20]),
    ];
    let mut ids = vec![];
    for (name, preset_name, color) in tracks {
        let pr = preset(preset_name);
        let id = p.add_track(name, pr.kind, color);
        p.track_mut(id).unwrap().instrument = pr.build();
        ids.push(id);
    }

    // Devices and sends.
    let d1 = p.new_id();
    let d2 = p.new_id();
    let d3 = p.new_id();
    let d4 = p.new_id();
    let d5 = p.new_id();
    let d6 = p.new_id();
    let d7 = p.new_id();
    {
        let t = p.track_mut(ids[0]).unwrap();
        t.channel.sends = [0.08, 0.0];
        t.channel.volume = 0.82;
    }
    {
        let t = p.track_mut(ids[1]).unwrap();
        t.channel.devices.push(Device::new(d1, DeviceKind::Sidechain).with(0, 0.85).with(1, 0.18));
        t.channel.volume = 0.78;
    }
    {
        let t = p.track_mut(ids[2]).unwrap();
        t.channel.devices.push(Device::new(d2, DeviceKind::Saturator).with(0, 0.25).with(2, 0.6));
        t.channel.devices.push(Device::new(d3, DeviceKind::Sidechain).with(0, 0.4));
        t.channel.sends = [0.12, 0.3];
        t.channel.volume = 0.7;
    }
    {
        let t = p.track_mut(ids[3]).unwrap();
        t.channel.devices.push(Device::new(d4, DeviceKind::Sidechain).with(0, 0.6));
        t.channel.devices.push(Device::new(d5, DeviceKind::Utility).with(3, 180.0));
        t.channel.sends = [0.35, 0.45];
        t.channel.volume = 0.68;
    }
    {
        let t = p.track_mut(ids[4]).unwrap();
        t.channel.devices.push(
            Device::new(d6, DeviceKind::FilterSweep).with(1, 1400.0).with(3, 0.35).with(4, params::div_index("4 bars") as f32),
        );
        t.channel.devices.push(Device::new(d7, DeviceKind::Sidechain).with(0, 0.75).with(1, 0.3));
        t.channel.sends = [0.4, 0.1];
        t.channel.volume = 0.66;
    }

    // Launcher clips: (track index, scene, pattern).
    let layout: &[(usize, usize, &str)] = &[
        (0, 0, "Minimal Groove"),
        (4, 0, "Pad Am-F-C-G"),
        (0, 1, "Rolling Techno"),
        (1, 1, "Rolling 16ths"),
        (3, 1, "Dub Chord"),
        (0, 2, "Rolling Techno"),
        (1, 2, "Rolling 16ths"),
        (2, 2, "Acid Line 1"),
        (2, 3, "Acid Line 2"),
        (3, 3, "Dub Chord"),
        (4, 3, "Pad Am-F-C-G"),
        (0, 4, "Industrial Stomp"),
        (1, 4, "Offbeat Bass"),
        (2, 4, "Acid Line 2"),
        (3, 4, "Minor Stabs"),
        (0, 5, "Four to the Floor"),
        (4, 5, "Pad Am-F-C-G"),
    ];
    for &(ti, scene, pat) in layout {
        let color = p.tracks[ti].color;
        if let Some(pt) = pattern(pat) {
            p.tracks[ti].clips[scene] = Some(pt.clip(color));
        }
    }

    scenes_to_arrangement(&mut p, &[16, 16, 16, 8, 16, 8], 0);

    // Filter sweep automation on the acid line across the "Acid" section (bars 33-48).
    let acid = p.track_mut(ids[2]).unwrap();
    let cutoff_def = &params::BASS_PARAMS[bass::CUTOFF];
    acid.automation.push(Automation {
        target: AutoTarget::Instrument(bass::CUTOFF),
        points: vec![
            (32.0 * 16.0, cutoff_def.to_norm(180.0)),
            (48.0 * 16.0 - 1.0, cutoff_def.to_norm(1600.0)),
            (56.0 * 16.0, cutoff_def.to_norm(500.0)),
            (72.0 * 16.0, cutoff_def.to_norm(1200.0)),
        ],
    });
    p
}

/// Lays out the given scenes one after another on the timeline (`bars` per scene).
pub fn scenes_to_arrangement(p: &mut Project, bars: &[u32], start_bar: u32) {
    let mut pos = start_bar * STEPS_PER_BAR;
    for (scene, &b) in bars.iter().enumerate() {
        let len = b * STEPS_PER_BAR;
        for ti in 0..p.tracks.len() {
            if let Some(Some(clip)) = p.tracks[ti].clips.get(scene).cloned() {
                let id = p.new_id();
                p.tracks[ti].arrangement.push(ArrClip { id, start: pos, len, clip });
            }
        }
        pos += len;
    }
}
