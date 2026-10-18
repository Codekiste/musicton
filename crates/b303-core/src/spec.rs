//! Parameter list of the B-303. The index of each entry is its parameter number everywhere
//! (engine, Musicton projects, plug-in automation).

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unit {
    Plain,
    Pct,
    St,
    Sec,
    Ct,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub def: f32,
    pub unit: Unit,
    pub step: f32,
    pub options: &'static [&'static str],
}

impl Spec {
    const fn float(name: &'static str, min: f32, max: f32, def: f32, unit: Unit) -> Self {
        Self { name, min, max, def, unit, step: 0.0, options: &[] }
    }
    const fn choice(name: &'static str, options: &'static [&'static str], def: usize) -> Self {
        Self { name, min: 0.0, max: (options.len() - 1) as f32, def: def as f32, unit: Unit::Plain, step: 1.0, options }
    }
    const fn int(name: &'static str, min: f32, max: f32, def: f32) -> Self {
        Self { name, min, max, def, unit: Unit::Plain, step: 1.0, options: &[] }
    }

    pub fn is_choice(&self) -> bool {
        !self.options.is_empty()
    }

    pub fn format(&self, v: f32) -> String {
        if self.is_choice() {
            return self.options.get(v.round() as usize).copied().unwrap_or("?").to_string();
        }
        match self.unit {
            Unit::Pct => format!("{:.0} %", v * 100.0),
            Unit::St => format!("{v:+.1} st"),
            Unit::Sec => format!("{:.0} ms", v * 1000.0),
            Unit::Ct => format!("{v:+.0} ct"),
            Unit::Plain if self.step >= 1.0 => format!("{}", v.round() as i32 + if self.name == "Pattern" { 1 } else { 0 }),
            Unit::Plain => format!("{v:.2}"),
        }
    }
}

/// Parameter indices.
pub mod p {
    pub const WAVE: usize = 0;
    pub const TUNING: usize = 1;
    pub const CUTOFF: usize = 2;
    pub const RES: usize = 3;
    pub const ENV_MOD: usize = 4;
    pub const DECAY: usize = 5;
    pub const ACCENT: usize = 6;
    pub const VOLUME: usize = 7;
    pub const DRIVE: usize = 8;
    pub const SLIDE: usize = 9;
    pub const VCF_TRIM: usize = 10;
    pub const MASTER_TUNE: usize = 11;
    pub const CONDITION: usize = 12;
    pub const DRIVE_TYPE: usize = 13;
    pub const DRIVE_TONE: usize = 14;
    pub const DELAY_TYPE: usize = 15;
    pub const DELAY_TIME: usize = 16;
    pub const DELAY_LEVEL: usize = 17;
    pub const TEMPO_SYNC: usize = 18;
    pub const RUN: usize = 19;
    pub const PATTERN: usize = 20;
    pub const PLAY_MODE: usize = 21;
    pub const SCALE: usize = 22;
    pub const SHUFFLE: usize = 23;
    pub const POS_LOCK: usize = 24;
    /// Parameters that make up the sound (everything before the sequencer section).
    pub const SOUND: std::ops::RangeInclusive<usize> = 0..=TEMPO_SYNC;
}

pub const DRIVE_TYPES: &[&str] = &["Off", "OD", "DS", "FZ", "MT"];
pub const DELAY_TYPES: &[&str] = &["Off", "CH1", "PING", "TAPE"];
pub const PLAY_MODES: &[&str] = &["Forward", "Reverse", "Fwd&Rev", "Invert", "Random"];
pub const SCALES: &[&str] = &["1/8T", "1/16", "1/16T", "1/32"];
pub const SCALE_BEATS: [f64; 4] = [1.0 / 3.0, 0.25, 1.0 / 6.0, 0.125];

pub const SPECS: [Spec; 25] = [
    Spec::choice("Waveform", &["Saw", "Square"], 0),
    Spec::float("Tuning", -12.0, 12.0, 0.0, Unit::St),
    Spec::float("Cutoff Freq", 0.0, 1.0, 0.35, Unit::Pct),
    Spec::float("Resonance", 0.0, 1.0, 0.7, Unit::Pct),
    Spec::float("Env Mod", 0.0, 1.0, 0.5, Unit::Pct),
    Spec::float("Decay", 0.0, 1.0, 0.35, Unit::Pct),
    Spec::float("Accent", 0.0, 1.0, 0.6, Unit::Pct),
    Spec::float("Volume", 0.0, 1.0, 0.7, Unit::Pct),
    Spec::float("Depth", 0.0, 1.0, 0.0, Unit::Pct),
    Spec::float("Slide Time", 0.02, 0.25, 0.06, Unit::Sec),
    Spec::float("VCF Trim", -1.0, 1.0, 0.0, Unit::Pct),
    Spec::float("Master Tune", -50.0, 50.0, 0.0, Unit::Ct),
    Spec::float("Condition", 0.0, 1.0, 0.2, Unit::Pct),
    Spec::choice("Drive Type", DRIVE_TYPES, 1),
    Spec::float("Tone", 0.0, 1.0, 0.5, Unit::Pct),
    Spec::choice("Delay Type", DELAY_TYPES, 0),
    Spec::float("Time", 0.0, 1.0, 0.4, Unit::Pct),
    Spec::float("Level", 0.0, 1.0, 0.3, Unit::Pct),
    Spec::choice("Tempo Sync", &["Off", "On"], 1),
    Spec::choice("Run", &["Stop", "Run"], 0),
    Spec::int("Pattern", 0.0, 7.0, 0.0),
    Spec::choice("Play Mode", PLAY_MODES, 0),
    Spec::choice("Scale", SCALES, 1),
    Spec::float("Shuffle", 0.0, 1.0, 0.0, Unit::Pct),
    Spec::choice("Position Lock", &["Off", "On"], 1),
];

pub fn defaults() -> Vec<f32> {
    SPECS.iter().map(|s| s.def).collect()
}

/// Factory sounds: (name, description, overrides).
pub const PRESETS: &[(&str, &str, &[(usize, f32)])] = &[
    ("B-303 Classic", "The classic acid bassline: saw wave, high resonance, medium env mod.", &[]),
    ("B-303 Squelch", "Screaming resonance, deep env mod and strong accents - automate Cutoff.", &[(p::RES, 0.92), (p::ENV_MOD, 0.75), (p::ACCENT, 0.9), (p::CUTOFF, 0.25), (p::DECAY, 0.5)]),
    ("B-303 Square", "Hollow square-wave 303.", &[(p::WAVE, 1.0), (p::RES, 0.6), (p::ENV_MOD, 0.45), (p::CUTOFF, 0.4)]),
    ("B-303 Distorted", "Overdriven acid for hard techno.", &[(p::DRIVE, 0.7), (p::RES, 0.85), (p::ENV_MOD, 0.6), (p::ACCENT, 0.8), (p::VOLUME, 0.6)]),
    ("B-303 Deep", "Low, round bassline with little resonance and long decay.", &[(p::CUTOFF, 0.2), (p::RES, 0.35), (p::ENV_MOD, 0.3), (p::DECAY, 0.8), (p::ACCENT, 0.4)]),
];

pub fn preset_params(idx: usize) -> Vec<f32> {
    let mut v = defaults();
    if let Some((_, _, ov)) = PRESETS.get(idx) {
        for (i, x) in ov.iter() {
            v[*i] = *x;
        }
    }
    v
}
