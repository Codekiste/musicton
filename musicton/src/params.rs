//! Parameter definitions for instruments and devices. Parameter values are stored as plain
//! `Vec<f32>` in definition order, so the audio thread can index them cheaply.

use std::sync::LazyLock;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Curve {
    Lin,
    Log,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unit {
    None,
    Pct,
    Hz,
    Db,
    Sec,
    Ms,
    St,
    Pan,
    Ratio,
    Ct,
    X,
    Note,
    Deg,
}

#[derive(Clone, Debug)]
pub struct ParamDef {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub def: f32,
    pub curve: Curve,
    pub unit: Unit,
    pub step: f32,
    pub options: &'static [&'static str],
}

impl ParamDef {
    pub const fn new(name: &'static str, min: f32, max: f32, def: f32, unit: Unit) -> Self {
        Self { name, min, max, def, curve: Curve::Lin, unit, step: 0.0, options: &[] }
    }
    pub const fn log(mut self) -> Self {
        self.curve = Curve::Log;
        self
    }
    pub const fn step(mut self, s: f32) -> Self {
        self.step = s;
        self
    }
    pub const fn options(name: &'static str, options: &'static [&'static str], def: usize) -> Self {
        Self {
            name,
            min: 0.0,
            max: (options.len() - 1) as f32,
            def: def as f32,
            curve: Curve::Lin,
            unit: Unit::None,
            step: 1.0,
            options,
        }
    }

    pub fn is_choice(&self) -> bool {
        !self.options.is_empty()
    }

    /// Value -> 0..1 knob position.
    pub fn to_norm(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        match self.curve {
            Curve::Lin => (v - self.min) / (self.max - self.min),
            Curve::Log => (v / self.min).ln() / (self.max / self.min).ln(),
        }
    }

    /// 0..1 knob position -> value.
    pub fn from_norm(&self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        let v = match self.curve {
            Curve::Lin => self.min + n * (self.max - self.min),
            Curve::Log => self.min * (self.max / self.min).powf(n),
        };
        if self.step > 0.0 {
            ((v - self.min) / self.step).round() * self.step + self.min
        } else {
            v
        }
    }

    pub fn format(&self, v: f32) -> String {
        if self.is_choice() {
            return self.options.get(v.round() as usize).unwrap_or(&"?").to_string();
        }
        match self.unit {
            Unit::None => {
                if self.step >= 1.0 {
                    format!("{}", v.round() as i32)
                } else {
                    format!("{v:.2}")
                }
            }
            Unit::Pct => format!("{:.0} %", v * 100.0),
            Unit::Hz => {
                if v >= 1000.0 {
                    format!("{:.2} kHz", v / 1000.0)
                } else {
                    format!("{v:.0} Hz")
                }
            }
            Unit::Db => format!("{v:.1} dB"),
            Unit::Sec => {
                if v < 1.0 {
                    format!("{:.0} ms", v * 1000.0)
                } else {
                    format!("{v:.2} s")
                }
            }
            Unit::Ms => format!("{v:.0} ms"),
            Unit::St => format!("{v:+.1} st"),
            Unit::Pan => {
                if v.abs() < 0.01 {
                    "C".into()
                } else if v < 0.0 {
                    format!("{:.0}L", -v * 50.0)
                } else {
                    format!("{:.0}R", v * 50.0)
                }
            }
            Unit::Ratio => format!("{v:.1} : 1"),
            Unit::Ct => format!("{v:.0} ct"),
            Unit::X => format!("{v:.2}x"),
            Unit::Note => crate::model::note_name(v.round().clamp(0.0, 127.0) as u8),
            Unit::Deg => format!("{v:.0}°"),
        }
    }
}

pub fn defaults(defs: &[ParamDef]) -> Vec<f32> {
    defs.iter().map(|d| d.def).collect()
}

pub const DIVISIONS: &[&str] = &[
    "1/32", "1/16T", "1/16", "1/8T", "1/8", "1/8D", "1/4T", "1/4", "1/4D", "1/2", "1 bar", "2 bars", "4 bars", "8 bars",
];
const DIVISION_BEATS: [f32; 14] = [
    0.125, 1.0 / 6.0, 0.25, 1.0 / 3.0, 0.5, 0.75, 2.0 / 3.0, 1.0, 1.5, 2.0, 4.0, 8.0, 16.0, 32.0,
];

pub fn division_beats(idx: f32) -> f32 {
    DIVISION_BEATS[(idx.round() as usize).min(DIVISION_BEATS.len() - 1)]
}

pub fn div_index(name: &str) -> usize {
    DIVISIONS.iter().position(|d| *d == name).unwrap_or(7)
}

// ---------------------------------------------------------------------------------------------
// Instruments

pub const DRUM_LANES: &[&str] =
    &["Kick", "Clap", "Snare", "Rim", "Closed HH", "Open HH", "Ride", "Low Tom", "Hi Tom", "Perc"];
pub const DRUM_KIT_PARAMS: usize = 2;
pub const DRUM_LANE_PARAMS: usize = 5;
pub const LANE_TUNE: usize = 0;
pub const LANE_DECAY: usize = 1;
pub const LANE_TONE: usize = 2;
pub const LANE_LEVEL: usize = 3;
pub const LANE_PAN: usize = 4;

pub fn lane_param(lane: usize, p: usize) -> usize {
    DRUM_KIT_PARAMS + lane * DRUM_LANE_PARAMS + p
}

pub static DRUM_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    let mut v = vec![
        ParamDef::new("Drive", 0.0, 1.0, 0.15, Unit::Pct),
        ParamDef::new("Volume", 0.0, 1.2, 0.9, Unit::Pct),
    ];
    for _ in DRUM_LANES {
        v.push(ParamDef::new("Tune", -12.0, 12.0, 0.0, Unit::St));
        v.push(ParamDef::new("Decay", 0.2, 2.5, 1.0, Unit::X));
        v.push(ParamDef::new("Tone", 0.0, 1.0, 0.5, Unit::Pct));
        v.push(ParamDef::new("Level", 0.0, 1.0, 0.8, Unit::Pct));
        v.push(ParamDef::new("Pan", -1.0, 1.0, 0.0, Unit::Pan));
    }
    v
});

pub mod bass {
    pub const WAVE: usize = 0;
    pub const OCTAVE: usize = 1;
    pub const CUTOFF: usize = 2;
    pub const RES: usize = 3;
    pub const ENV_MOD: usize = 4;
    pub const DECAY: usize = 5;
    pub const ACCENT: usize = 6;
    pub const GLIDE: usize = 7;
    pub const SUB: usize = 8;
    pub const DRIVE: usize = 9;
    pub const VOLUME: usize = 10;
}

pub static BASS_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Wave", &["Saw", "Square"], 0),
        ParamDef::new("Octave", -2.0, 2.0, 0.0, Unit::None).step(1.0),
        ParamDef::new("Cutoff", 40.0, 8000.0, 420.0, Unit::Hz).log(),
        ParamDef::new("Reso", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Env Mod", 0.0, 1.0, 0.55, Unit::Pct),
        ParamDef::new("Decay", 0.03, 2.0, 0.3, Unit::Sec).log(),
        ParamDef::new("Accent", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Glide", 0.01, 0.4, 0.07, Unit::Sec).log(),
        ParamDef::new("Sub", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Drive", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Volume", 0.0, 1.0, 0.6, Unit::Pct),
    ]
});

pub mod synth {
    pub const WAVE: usize = 0;
    pub const DETUNE: usize = 1;
    pub const VOICES: usize = 2;
    pub const SUB: usize = 3;
    pub const CUTOFF: usize = 4;
    pub const RES: usize = 5;
    pub const ENV_AMT: usize = 6;
    pub const ATTACK: usize = 7;
    pub const DECAY: usize = 8;
    pub const SUSTAIN: usize = 9;
    pub const RELEASE: usize = 10;
    pub const VOLUME: usize = 11;
}

pub static SYNTH_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Wave", &["Saw", "Square", "Triangle", "Sine"], 0),
        ParamDef::new("Detune", 0.0, 50.0, 12.0, Unit::Ct),
        ParamDef::new("Voices", 1.0, 5.0, 3.0, Unit::None).step(1.0),
        ParamDef::new("Sub", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("Cutoff", 60.0, 16000.0, 1800.0, Unit::Hz).log(),
        ParamDef::new("Reso", 0.0, 1.0, 0.15, Unit::Pct),
        ParamDef::new("Flt Env", 0.0, 1.0, 0.35, Unit::Pct),
        ParamDef::new("Attack", 0.001, 3.0, 0.004, Unit::Sec).log(),
        ParamDef::new("Decay", 0.01, 3.0, 0.35, Unit::Sec).log(),
        ParamDef::new("Sustain", 0.0, 1.0, 0.4, Unit::Pct),
        ParamDef::new("Release", 0.01, 5.0, 0.3, Unit::Sec).log(),
        ParamDef::new("Volume", 0.0, 1.0, 0.45, Unit::Pct),
    ]
});

// ---------------------------------------------------------------------------------------------
// Audio effects

pub static AUTOFILTER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Type", &["Lowpass", "Highpass", "Bandpass"], 0),
        ParamDef::new("Freq", 20.0, 20000.0, 2000.0, Unit::Hz).log(),
        ParamDef::new("Res", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("LFO Amt", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::options("Rate", DIVISIONS, 10),
        ParamDef::options("Shape", &["Sine", "Triangle", "Square", "Saw"], 0),
    ]
});

pub static EQ3_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Low", -24.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Mid", -24.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("High", -24.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Low Freq", 50.0, 800.0, 250.0, Unit::Hz).log(),
        ParamDef::new("Hi Freq", 1500.0, 12000.0, 3500.0, Unit::Hz).log(),
    ]
});

pub static SATURATOR_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Drive", 0.0, 1.0, 0.4, Unit::Pct),
        ParamDef::new("Tone", 500.0, 20000.0, 12000.0, Unit::Hz).log(),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Output", -24.0, 12.0, 0.0, Unit::Db),
    ]
});

pub static ECHO_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Time", DIVISIONS, 5),
        ParamDef::new("Feedback", 0.0, 0.95, 0.45, Unit::Pct),
        ParamDef::options("Ping Pong", &["On", "Off"], 0),
        ParamDef::new("HP", 20.0, 3000.0, 250.0, Unit::Hz).log(),
        ParamDef::new("LP", 500.0, 20000.0, 6000.0, Unit::Hz).log(),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.3, Unit::Pct),
    ]
});

pub static REVERB_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Size", 0.0, 1.0, 0.75, Unit::Pct),
        ParamDef::new("Damping", 0.0, 1.0, 0.4, Unit::Pct),
        ParamDef::new("Predelay", 0.0, 250.0, 20.0, Unit::Ms),
        ParamDef::new("Low Cut", 20.0, 1500.0, 250.0, Unit::Hz).log(),
        ParamDef::new("Width", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.3, Unit::Pct),
    ]
});

pub static COMPRESSOR_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Thresh", -60.0, 0.0, -18.0, Unit::Db),
        ParamDef::new("Ratio", 1.0, 20.0, 4.0, Unit::Ratio),
        ParamDef::new("Attack", 0.0005, 0.3, 0.01, Unit::Sec).log(),
        ParamDef::new("Release", 0.01, 1.0, 0.15, Unit::Sec).log(),
        ParamDef::new("Makeup", 0.0, 24.0, 3.0, Unit::Db),
    ]
});

pub static LIMITER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Gain", 0.0, 18.0, 2.0, Unit::Db),
        ParamDef::new("Ceiling", -12.0, 0.0, -0.5, Unit::Db),
        ParamDef::new("Release", 0.01, 1.0, 0.08, Unit::Sec).log(),
    ]
});

pub static SIDECHAIN_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Amount", 0.0, 1.0, 0.7, Unit::Pct),
        ParamDef::new("Release", 0.02, 0.8, 0.2, Unit::Sec).log(),
    ]
});

pub static REDUX_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Bits", 1.0, 16.0, 8.0, Unit::None).step(1.0),
        ParamDef::new("Downsample", 1.0, 32.0, 1.0, Unit::X).log(),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
    ]
});

pub static UTILITY_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Gain", -36.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Pan", -1.0, 1.0, 0.0, Unit::Pan),
        ParamDef::new("Width", 0.0, 2.0, 1.0, Unit::Pct),
        ParamDef::new("Low Cut", 10.0, 1000.0, 10.0, Unit::Hz).log(),
    ]
});

// ---------------------------------------------------------------------------------------------
// More audio effects

pub const BEAT_INTERVALS: &[&str] = &["1/4", "1/2", "1 bar", "2 bars", "4 bars"];
pub const BEAT_GRIDS: &[&str] = &["1/32", "1/16T", "1/16", "1/8T", "1/8", "1/4", "1/2"];

pub fn named_beats(name: &str) -> f32 {
    division_beats(div_index(name) as f32)
}

pub static AUTOPAN_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Amount", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::options("Rate", DIVISIONS, 7),
        ParamDef::options("Shape", &["Sine", "Triangle", "Square", "Saw", "Random"], 0),
        ParamDef::new("Phase", 0.0, 360.0, 180.0, Unit::Deg),
        ParamDef::new("Offset", 0.0, 360.0, 0.0, Unit::Deg),
    ]
});

pub static BEATREPEAT_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Interval", BEAT_INTERVALS, 2),
        ParamDef::new("Offset", 0.0, 15.0, 8.0, Unit::None).step(1.0),
        ParamDef::options("Grid", BEAT_GRIDS, 2),
        ParamDef::new("Chance", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Gate", 1.0, 32.0, 8.0, Unit::None).step(1.0),
        ParamDef::new("Decay", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Pitch", -24.0, 0.0, 0.0, Unit::St).step(1.0),
        ParamDef::options("Mode", &["Mix", "Insert", "Gate"], 1),
        ParamDef::new("Volume", -24.0, 6.0, 0.0, Unit::Db),
    ]
});

pub static CHORUS_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Rate", 0.05, 5.0, 0.6, Unit::Hz).log(),
        ParamDef::new("Depth", 0.0, 1.0, 0.4, Unit::Pct),
        ParamDef::new("Delay", 1.0, 30.0, 8.0, Unit::Ms),
        ParamDef::new("Feedback", 0.0, 0.9, 0.0, Unit::Pct),
        ParamDef::new("Width", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub const CORPUS_TYPES: &[&str] = &["Beam", "Marimba", "String", "Membrane", "Plate", "Pipe", "Tube"];

pub static CORPUS_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Type", CORPUS_TYPES, 0),
        ParamDef::new("Tune", 40.0, 2000.0, 220.0, Unit::Hz).log(),
        ParamDef::new("Decay", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Bright", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Input LP", 200.0, 20000.0, 8000.0, Unit::Hz).log(),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static DRUMBUSS_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Drive", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("Crunch", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Damp", 1000.0, 20000.0, 20000.0, Unit::Hz).log(),
        ParamDef::new("Transients", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Boom", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Boom Freq", 30.0, 90.0, 50.0, Unit::Hz),
        ParamDef::new("Boom Decay", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Output", -24.0, 12.0, 0.0, Unit::Db),
    ]
});

pub static DYNTUBE_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Drive", 0.0, 36.0, 6.0, Unit::Db),
        ParamDef::new("Bias", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Tone", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::options("Tube", &["A", "B", "C"], 0),
        ParamDef::new("Output", -24.0, 12.0, -3.0, Unit::Db),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
    ]
});

pub const EQ8_TYPES: &[&str] = &["Low Cut 48", "Low Cut 12", "Low Shelf", "Bell", "Notch", "High Shelf", "High Cut 12", "High Cut 48"];
pub const EQ8_BAND_PARAMS: usize = 5;

pub static EQ8_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    const FREQS: [f32; 8] = [30.0, 100.0, 250.0, 600.0, 1500.0, 3500.0, 8000.0, 16000.0];
    const TYPES: [usize; 8] = [1, 2, 3, 3, 3, 3, 5, 6];
    let mut v = vec![];
    for b in 0..8 {
        v.push(ParamDef::options("On", &["Off", "On"], if b == 0 || b == 7 { 0 } else { 1 }));
        v.push(ParamDef::options("Type", EQ8_TYPES, TYPES[b]));
        v.push(ParamDef::new("Freq", 20.0, 20000.0, FREQS[b], Unit::Hz).log());
        v.push(ParamDef::new("Gain", -15.0, 15.0, 0.0, Unit::Db));
        v.push(ParamDef::new("Q", 0.1, 18.0, 0.71, Unit::None).log());
    }
    v.push(ParamDef::new("Output", -12.0, 12.0, 0.0, Unit::Db));
    v
});

pub static EROSION_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Mode", &["Noise", "Wide Noise", "Sine"], 0),
        ParamDef::new("Freq", 800.0, 18000.0, 4000.0, Unit::Hz).log(),
        ParamDef::new("Amount", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Width", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static FILTERDELAY_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    let mut v = vec![];
    for (time, freq) in [(4usize, 600.0f32), (7, 1500.0), (5, 4000.0)] {
        v.push(ParamDef::options("Time", DIVISIONS, time));
        v.push(ParamDef::new("Freq", 50.0, 18000.0, freq, Unit::Hz).log());
        v.push(ParamDef::new("Feedback", 0.0, 0.95, 0.4, Unit::Pct));
        v.push(ParamDef::new("Volume", 0.0, 1.0, 0.6, Unit::Pct));
    }
    v.push(ParamDef::new("Dry", 0.0, 1.0, 1.0, Unit::Pct));
    v
});

pub static FLANGER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Delay", 0.1, 10.0, 2.5, Unit::Ms).log(),
        ParamDef::new("Rate", 0.02, 5.0, 0.3, Unit::Hz).log(),
        ParamDef::new("Depth", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Feedback", -0.95, 0.95, 0.5, Unit::Pct),
        ParamDef::new("Spread", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static FREQSHIFT_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Mode", &["Shift", "Ring"], 0),
        ParamDef::new("Coarse", -1000.0, 1000.0, 50.0, Unit::Hz).step(1.0),
        ParamDef::new("Fine", -50.0, 50.0, 0.0, Unit::Hz),
        ParamDef::options("Wide", &["Off", "On"], 0),
        ParamDef::new("Drive", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static GATE_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Thresh", -80.0, 0.0, -40.0, Unit::Db),
        ParamDef::new("Return", 0.0, 24.0, 3.0, Unit::Db),
        ParamDef::new("Attack", 0.0001, 0.1, 0.0005, Unit::Sec).log(),
        ParamDef::new("Hold", 0.001, 0.5, 0.02, Unit::Sec).log(),
        ParamDef::new("Release", 0.005, 2.0, 0.15, Unit::Sec).log(),
        ParamDef::new("Floor", -80.0, 0.0, -80.0, Unit::Db),
    ]
});

pub const GLUE_ATTACKS: &[&str] = &["0.01 ms", "0.1 ms", "0.3 ms", "1 ms", "3 ms", "10 ms", "30 ms"];
pub const GLUE_RELEASES: &[&str] = &["0.1 s", "0.2 s", "0.4 s", "0.6 s", "0.8 s", "1.2 s", "Auto"];

pub static GLUE_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Thresh", -40.0, 0.0, -10.0, Unit::Db),
        ParamDef::options("Ratio", &["2", "4", "10"], 0),
        ParamDef::options("Attack", GLUE_ATTACKS, 5),
        ParamDef::options("Release", GLUE_RELEASES, 6),
        ParamDef::new("Makeup", 0.0, 20.0, 0.0, Unit::Db),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::options("Soft Clip", &["Off", "On"], 0),
    ]
});

pub static GRAINDELAY_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Time", 1.0, 2000.0, 100.0, Unit::Ms).log(),
        ParamDef::new("Spray", 0.0, 500.0, 20.0, Unit::Ms),
        ParamDef::new("Frequency", 1.0, 150.0, 60.0, Unit::Hz).log(),
        ParamDef::new("Pitch", -24.0, 24.0, 0.0, Unit::St),
        ParamDef::new("Rand Pitch", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Feedback", 0.0, 0.95, 0.2, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static MULTIBAND_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    let mut v = vec![
        ParamDef::new("Low Freq", 60.0, 1000.0, 120.0, Unit::Hz).log(),
        ParamDef::new("High Freq", 1000.0, 15000.0, 2500.0, Unit::Hz).log(),
    ];
    for _ in 0..3 {
        v.push(ParamDef::new("Above Thr", -60.0, 0.0, -20.0, Unit::Db));
        v.push(ParamDef::new("Above Ratio", 1.0, 30.0, 2.0, Unit::Ratio).log());
        v.push(ParamDef::new("Below Thr", -70.0, 0.0, -40.0, Unit::Db));
        v.push(ParamDef::new("Below Ratio", 1.0, 5.0, 1.0, Unit::Ratio));
        v.push(ParamDef::new("Gain", -24.0, 24.0, 0.0, Unit::Db));
    }
    v.push(ParamDef::new("Amount", 0.0, 1.0, 1.0, Unit::Pct));
    v.push(ParamDef::new("Time", 0.1, 10.0, 1.0, Unit::X).log());
    v.push(ParamDef::new("Output", -24.0, 24.0, 0.0, Unit::Db));
    v
});

pub static OVERDRIVE_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Drive", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Tone", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Freq", 50.0, 20000.0, 800.0, Unit::Hz).log(),
        ParamDef::new("Width", 0.5, 9.0, 6.0, Unit::None),
        ParamDef::new("Dynamics", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
    ]
});

pub static PEDAL_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Type", &["Overdrive", "Distortion", "Fuzz"], 0),
        ParamDef::new("Gain", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Output", -24.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Bass", -12.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Mid", -12.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Treble", -12.0, 12.0, 0.0, Unit::Db),
        ParamDef::options("Sub", &["Off", "On"], 0),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
    ]
});

pub static PHASER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Poles", 2.0, 12.0, 4.0, Unit::None).step(2.0),
        ParamDef::new("Freq", 50.0, 5000.0, 700.0, Unit::Hz).log(),
        ParamDef::new("Rate", 0.02, 10.0, 0.4, Unit::Hz).log(),
        ParamDef::new("Amount", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Feedback", -0.95, 0.95, 0.5, Unit::Pct),
        ParamDef::new("Spread", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static PINGPONG_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Time", DIVISIONS, 4),
        ParamDef::new("Feedback", 0.0, 0.95, 0.5, Unit::Pct),
        ParamDef::new("Freq", 50.0, 18000.0, 1000.0, Unit::Hz).log(),
        ParamDef::new("Width", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.3, Unit::Pct),
    ]
});

pub static RESONATORS_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Note", 24.0, 96.0, 48.0, Unit::Note).step(1.0),
        ParamDef::new("Decay", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Color", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("II", -24.0, 24.0, 7.0, Unit::St).step(1.0),
        ParamDef::new("III", -24.0, 24.0, 12.0, Unit::St).step(1.0),
        ParamDef::new("IV", -24.0, 24.0, 19.0, Unit::St).step(1.0),
        ParamDef::new("V", -24.0, 24.0, 24.0, Unit::St).step(1.0),
        ParamDef::new("Width", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static SIMPLEDELAY_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Left", DIVISIONS, 4),
        ParamDef::options("Right", DIVISIONS, 5),
        ParamDef::new("Feedback", 0.0, 0.95, 0.4, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.3, Unit::Pct),
    ]
});

pub static VINYL_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Tracing", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("Trace Freq", 500.0, 10000.0, 3000.0, Unit::Hz).log(),
        ParamDef::new("Pinch", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Crackle", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Crackle Vol", 0.0, 1.0, 0.2, Unit::Pct),
    ]
});

pub static SPECTRUM_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| vec![ParamDef::new("Smooth", 0.0, 0.98, 0.7, Unit::Pct)]);

pub static TUNER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| vec![ParamDef::new("Reference", 410.0, 470.0, 440.0, Unit::Hz).step(1.0)]);

pub static VOCODER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Carrier", &["Noise", "Saw", "Pulse"], 1),
        ParamDef::new("Pitch", 24.0, 84.0, 45.0, Unit::Note).step(1.0),
        ParamDef::options("Bands", &["8", "16", "32"], 1),
        ParamDef::new("Low", 50.0, 1000.0, 80.0, Unit::Hz).log(),
        ParamDef::new("High", 1000.0, 18000.0, 12000.0, Unit::Hz).log(),
        ParamDef::new("Formant", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Release", 0.005, 0.5, 0.05, Unit::Sec).log(),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
    ]
});

pub static CHANNELEQ_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("HP 80", &["Off", "On"], 0),
        ParamDef::new("Low", -15.0, 15.0, 0.0, Unit::Db),
        ParamDef::new("Mid", -12.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Mid Freq", 120.0, 7500.0, 1000.0, Unit::Hz).log(),
        ParamDef::new("High", -15.0, 15.0, 0.0, Unit::Db),
        ParamDef::new("Output", -12.0, 12.0, 0.0, Unit::Db),
    ]
});

// ---------------------------------------------------------------------------------------------
// MIDI effects

pub const ARP_RATES: &[&str] = &["1/16", "1/8", "1/8D", "1/4", "1/2", "1 bar"];
pub const ARP_RATE_STEPS: [u64; 6] = [1, 2, 3, 4, 8, 16];
pub const SCALE_NAMES: &[&str] = &["Chromatic", "Minor", "Major", "Dorian", "Phrygian", "Harmonic Minor", "Minor Pentatonic"];
pub const KEY_NAMES: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub static ARP_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Style", &["Up", "Down", "UpDown", "DownUp", "Converge", "Diverge", "Random", "Played"], 0),
        ParamDef::options("Rate", ARP_RATES, 0),
        ParamDef::new("Gate", 0.1, 2.0, 0.8, Unit::Pct),
        ParamDef::new("Octaves", 1.0, 4.0, 1.0, Unit::None).step(1.0),
        ParamDef::options("Hold", &["Off", "On"], 0),
        ParamDef::new("Vel Decay", 0.0, 1.0, 0.0, Unit::Pct),
    ]
});

pub static CHORD_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    let mut v: Vec<ParamDef> = [3.0f32, 7.0, 0.0, 0.0, 0.0, 0.0]
        .iter()
        .map(|&d| ParamDef::new("Shift", -36.0, 36.0, d, Unit::St).step(1.0))
        .collect();
    v.push(ParamDef::new("Velocity", 0.0, 1.0, 1.0, Unit::Pct));
    v
});

pub static NOTELENGTH_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Mode", &["Sync", "Gate"], 0),
        ParamDef::options("Length", DIVISIONS, 2),
        ParamDef::new("Gate", 0.1, 4.0, 1.0, Unit::Pct),
    ]
});

pub static PITCH_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Pitch", -48.0, 48.0, 0.0, Unit::St).step(1.0),
        ParamDef::new("Lowest", 0.0, 127.0, 0.0, Unit::Note).step(1.0),
        ParamDef::new("Highest", 0.0, 127.0, 127.0, Unit::Note).step(1.0),
    ]
});

pub static RANDOM_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Chance", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Choices", 1.0, 24.0, 2.0, Unit::None).step(1.0),
        ParamDef::new("Scale", 1.0, 24.0, 12.0, Unit::St).step(1.0),
        ParamDef::options("Sign", &["Add", "Sub", "Bi"], 0),
    ]
});

pub static SCALE_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Base", KEY_NAMES, 9),
        ParamDef::options("Scale", SCALE_NAMES, 1),
        ParamDef::new("Transpose", -24.0, 24.0, 0.0, Unit::St).step(1.0),
    ]
});

pub static VELOCITY_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Mode", &["Scale", "Fixed"], 0),
        ParamDef::new("Drive", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Out Low", 1.0, 127.0, 1.0, Unit::None).step(1.0),
        ParamDef::new("Out High", 1.0, 127.0, 127.0, Unit::None).step(1.0),
        ParamDef::new("Random", 0.0, 64.0, 0.0, Unit::None).step(1.0),
    ]
});

// ---------------------------------------------------------------------------------------------
// More instruments

pub mod smp {
    pub const MODE: usize = 0;
    pub const ROOT: usize = 1;
    pub const TRANSPOSE: usize = 2;
    pub const DETUNE: usize = 3;
    pub const START: usize = 4;
    pub const END: usize = 5;
    pub const LOOP: usize = 6;
    pub const SLICES: usize = 7;
    pub const FTYPE: usize = 8;
    pub const CUTOFF: usize = 9;
    pub const RES: usize = 10;
    pub const FENV: usize = 11;
    pub const ATTACK: usize = 12;
    pub const DECAY: usize = 13;
    pub const SUSTAIN: usize = 14;
    pub const RELEASE: usize = 15;
    pub const VOICES: usize = 16;
    pub const GLIDE: usize = 17;
    pub const VEL: usize = 18;
    pub const VOLUME: usize = 19;
    pub const PAN: usize = 20;
    pub const WARP: usize = 21;
    pub const BEATS: usize = 22;
}

pub static SAMPLER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Mode", &["Classic", "One-Shot", "Slice"], 0),
        ParamDef::new("Root", 0.0, 127.0, 60.0, Unit::Note).step(1.0),
        ParamDef::new("Transpose", -48.0, 48.0, 0.0, Unit::St).step(1.0),
        ParamDef::new("Detune", -50.0, 50.0, 0.0, Unit::Ct),
        ParamDef::new("Start", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("End", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::options("Loop", &["Off", "On"], 0),
        ParamDef::new("Slices", 2.0, 64.0, 16.0, Unit::None).step(1.0),
        ParamDef::options("Filter", &["Lowpass", "Highpass", "Bandpass"], 0),
        ParamDef::new("Cutoff", 30.0, 20000.0, 20000.0, Unit::Hz).log(),
        ParamDef::new("Res", 0.0, 1.0, 0.1, Unit::Pct),
        ParamDef::new("Flt Env", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Attack", 0.001, 5.0, 0.001, Unit::Sec).log(),
        ParamDef::new("Decay", 0.01, 5.0, 0.6, Unit::Sec).log(),
        ParamDef::new("Sustain", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Release", 0.005, 10.0, 0.08, Unit::Sec).log(),
        ParamDef::new("Voices", 1.0, 16.0, 8.0, Unit::None).step(1.0),
        ParamDef::new("Glide", 0.0, 0.5, 0.0, Unit::Sec),
        ParamDef::new("Vel>Vol", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Volume", 0.0, 1.5, 0.8, Unit::Pct),
        ParamDef::new("Pan", -1.0, 1.0, 0.0, Unit::Pan),
        ParamDef::options("Warp", &["Off", "Sync"], 0),
        ParamDef::new("Beats", 0.25, 64.0, 4.0, Unit::None).step(0.25),
    ]
});

pub const OP_PARAMS: usize = 7;
pub const OPERATOR_ALGOS: &[&str] = &["1 Stack", "2 Y", "3 Pairs", "4 Branch", "5 Fan", "6 Organ+1", "7 Additive", "8 Comb"];

pub mod fmop {
    pub const ALGO: usize = 28;
    pub const FEEDBACK: usize = 29;
    pub const CUTOFF: usize = 30;
    pub const RES: usize = 31;
    pub const SPREAD: usize = 32;
    pub const VOLUME: usize = 33;
}

pub static FM4_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    let mut v = vec![];
    let levels = [1.0f32, 0.45, 0.3, 0.0];
    let names = ["A", "B", "C", "D"];
    for k in 0..4 {
        let _ = names[k];
        v.push(ParamDef::new("Ratio", 0.5, 16.0, if k == 0 { 1.0 } else { (k + 1) as f32 }, Unit::X).step(0.5));
        v.push(ParamDef::new("Fine", -100.0, 100.0, 0.0, Unit::Ct));
        v.push(ParamDef::new("Level", 0.0, 1.0, levels[k], Unit::Pct));
        v.push(ParamDef::new("Attack", 0.001, 5.0, 0.002, Unit::Sec).log());
        v.push(ParamDef::new("Decay", 0.01, 8.0, if k == 0 { 0.8 } else { 0.3 }, Unit::Sec).log());
        v.push(ParamDef::new("Sustain", 0.0, 1.0, if k == 0 { 0.6 } else { 0.2 }, Unit::Pct));
        v.push(ParamDef::new("Release", 0.005, 10.0, 0.2, Unit::Sec).log());
    }
    v.push(ParamDef::options("Algorithm", OPERATOR_ALGOS, 0));
    v.push(ParamDef::new("Feedback", 0.0, 1.0, 0.0, Unit::Pct));
    v.push(ParamDef::new("Cutoff", 30.0, 20000.0, 20000.0, Unit::Hz).log());
    v.push(ParamDef::new("Res", 0.0, 1.0, 0.0, Unit::Pct));
    v.push(ParamDef::new("Spread", 0.0, 1.0, 0.0, Unit::Pct));
    v.push(ParamDef::new("Volume", 0.0, 1.0, 0.55, Unit::Pct));
    v
});

pub const WAVETABLES: &[&str] = &["Basic Shapes", "Harmonic Sweep", "Formant", "PWM", "Digital"];

pub mod wavetable {
    pub const T1: usize = 0;
    pub const P1: usize = 1;
    pub const T2: usize = 2;
    pub const P2: usize = 3;
    pub const SEMI2: usize = 4;
    pub const DET2: usize = 5;
    pub const MIX: usize = 6;
    pub const SUB: usize = 7;
    pub const UNISON: usize = 8;
    pub const SPREAD: usize = 9;
    pub const CUTOFF: usize = 10;
    pub const RES: usize = 11;
    pub const FENV: usize = 12;
    pub const ATTACK: usize = 13;
    pub const DECAY: usize = 14;
    pub const SUSTAIN: usize = 15;
    pub const RELEASE: usize = 16;
    pub const LFO_RATE: usize = 17;
    pub const LFO_POS: usize = 18;
    pub const ENV_POS: usize = 19;
    pub const VOLUME: usize = 20;
}

pub static WAVETABLE_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Osc 1", WAVETABLES, 0),
        ParamDef::new("Pos 1", 0.0, 1.0, 0.35, Unit::Pct),
        ParamDef::options("Osc 2", WAVETABLES, 1),
        ParamDef::new("Pos 2", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Semi 2", -24.0, 24.0, 0.0, Unit::St).step(1.0),
        ParamDef::new("Detune 2", -50.0, 50.0, 7.0, Unit::Ct),
        ParamDef::new("Osc Mix", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Sub", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Unison", 1.0, 5.0, 3.0, Unit::None).step(1.0),
        ParamDef::new("Spread", 0.0, 50.0, 15.0, Unit::Ct),
        ParamDef::new("Cutoff", 30.0, 20000.0, 4000.0, Unit::Hz).log(),
        ParamDef::new("Res", 0.0, 1.0, 0.15, Unit::Pct),
        ParamDef::new("Flt Env", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Attack", 0.001, 5.0, 0.005, Unit::Sec).log(),
        ParamDef::new("Decay", 0.01, 5.0, 0.6, Unit::Sec).log(),
        ParamDef::new("Sustain", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Release", 0.005, 8.0, 0.4, Unit::Sec).log(),
        ParamDef::options("LFO Rate", DIVISIONS, 10),
        ParamDef::new("LFO>Pos", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("Env>Pos", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Volume", 0.0, 1.0, 0.45, Unit::Pct),
    ]
});

pub mod strm {
    pub const EXCITER: usize = 0;
    pub const DECAY: usize = 1;
    pub const DAMPING: usize = 2;
    pub const INHARM: usize = 3;
    pub const PICKUP: usize = 4;
    pub const BODY: usize = 5;
    pub const BRIGHT: usize = 6;
    pub const VIBRATO: usize = 7;
    pub const VOLUME: usize = 8;
}

pub static STRING_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Exciter", &["Pluck", "Hammer", "Bow"], 0),
        ParamDef::new("Decay", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Damping", 0.0, 1.0, 0.4, Unit::Pct),
        ParamDef::new("Inharm", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Pickup", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("Body", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Bright", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Vibrato", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Volume", 0.0, 1.0, 0.7, Unit::Pct),
    ]
});

// ---------------------------------------------------------------------------------------------
// B-303 (acid bassline) and Nexa (preset ROMpler)

pub mod b303 {
    pub use b303_core::p::*;
}


/// B-303 parameters, generated from the shared spec in `b303-core`.
pub static B303_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    b303_core::SPECS
        .iter()
        .map(|sp| {
            if sp.is_choice() {
                return ParamDef::options(sp.name, sp.options, sp.def as usize);
            }
            let unit = match sp.unit {
                b303_core::Unit::Pct => Unit::Pct,
                b303_core::Unit::St => Unit::St,
                b303_core::Unit::Sec => Unit::Sec,
                b303_core::Unit::Ct => Unit::Ct,
                b303_core::Unit::Plain => Unit::None,
            };
            ParamDef::new(sp.name, sp.min, sp.max, sp.def, unit).step(sp.step)
        })
        .collect()
});

pub mod nexa {
    pub const PRESET: usize = 0;
    pub const CUTOFF: usize = 1;
    pub const RES: usize = 2;
    pub const ATTACK: usize = 3;
    pub const RELEASE: usize = 4;
    pub const VOLUME: usize = 5;
    pub const ARP_ON: usize = 6;
    pub const ARP_MODE: usize = 7;
    pub const ARP_RATE: usize = 8;
    pub const ARP_OCT: usize = 9;
    pub const ARP_GATE: usize = 10;
    pub const GATE_ON: usize = 11;
    pub const GATE_PATTERN: usize = 12;
    pub const GATE_RATE: usize = 13;
    pub const GATE_DEPTH: usize = 14;
    pub const GATE_SMOOTH: usize = 15;
    pub const CHORUS: usize = 16;
    pub const DELAY: usize = 17;
    pub const DELAY_TIME: usize = 18;
    pub const DELAY_FB: usize = 19;
    pub const REVERB: usize = 20;
    pub const REVERB_SIZE: usize = 21;
    pub const DIST: usize = 22;
    pub const LOW: usize = 23;
    pub const HIGH: usize = 24;
    pub const MODWHEEL: usize = 25;
}

pub const NEXA_ARP_MODES: &[&str] = &["Up", "Down", "UpDown", "Random", "Chord", "Played"];
pub const NEXA_ARP_RATES: &[&str] = &["1/32", "1/16T", "1/16", "1/8T", "1/8", "1/4"];
pub const NEXA_ARP_RATE_BEATS: [f64; 6] = [0.125, 1.0 / 6.0, 0.25, 1.0 / 3.0, 0.5, 1.0];
pub const NEXA_GATE_RATES: &[&str] = &["1/32", "1/16", "1/8"];
pub const NEXA_GATE_RATE_BEATS: [f64; 3] = [0.125, 0.25, 0.5];

pub static NEXA_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    let presets: &'static [&'static str] = Box::leak(crate::audio::nexa::preset_names().into_boxed_slice());
    let gates: &'static [&'static str] = Box::leak(crate::audio::nexa::GATE_PATTERNS.iter().map(|g| g.0).collect::<Vec<_>>().into_boxed_slice());
    vec![
        ParamDef::options("Preset", presets, 0),
        ParamDef::new("Cutoff", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Resonance", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Attack", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Release", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Volume", 0.0, 1.0, 0.7, Unit::Pct),
        ParamDef::options("Arp", &["Off", "On"], 0),
        ParamDef::options("Arp Mode", NEXA_ARP_MODES, 0),
        ParamDef::options("Arp Rate", NEXA_ARP_RATES, 2),
        ParamDef::new("Arp Octaves", 1.0, 4.0, 1.0, Unit::None).step(1.0),
        ParamDef::new("Arp Gate", 0.1, 1.0, 0.6, Unit::Pct),
        ParamDef::options("Trancegate", &["Off", "On"], 0),
        ParamDef::options("Gate Pattern", gates, 0),
        ParamDef::options("Gate Rate", NEXA_GATE_RATES, 1),
        ParamDef::new("Gate Depth", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Gate Smooth", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("Chorus", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Delay", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::options("Delay Time", DIVISIONS, 5),
        ParamDef::new("Delay FB", 0.0, 0.9, 0.4, Unit::Pct),
        ParamDef::new("Reverb", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Reverb Size", 0.0, 1.0, 0.7, Unit::Pct),
        ParamDef::new("Distortion", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Low", -12.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("High", -12.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Mod Wheel", 0.0, 1.0, 0.0, Unit::Pct),
    ]
});

pub const MANGLER_PATTERN_NAMES: &[&str] = &["Gate 1/8", "Offbeat Gate", "Stutter End", "Tape Stop End", "Reverse Fill", "Half Time", "Glitch A", "Glitch B"];

pub static MANGLER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Pattern", MANGLER_PATTERN_NAMES, 0),
        ParamDef::options("Step", &["1/32", "1/16", "1/8"], 1),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
    ]
});

pub static HEAT_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    const T: &[&str] = &["Off", "Tube", "Tape", "Fuzz", "Fold", "Rectify", "Crush"];
    vec![
        ParamDef::new("Drive", 0.0, 36.0, 12.0, Unit::Db),
        ParamDef::options("Stage 1", T, 1),
        ParamDef::options("Stage 2", T, 2),
        ParamDef::options("Stage 3", T, 0),
        ParamDef::new("Bias", -0.5, 0.5, 0.0, Unit::None),
        ParamDef::new("Low Cut", 20.0, 1000.0, 30.0, Unit::Hz).log(),
        ParamDef::new("Tone", 1000.0, 20000.0, 12000.0, Unit::Hz).log(),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Output", -24.0, 12.0, 0.0, Unit::Db),
    ]
});

pub static HYBRID_VERB_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Character", &["Room", "Hall", "Plate", "Chamber", "Cathedral"], 1),
        ParamDef::new("Size", 0.3, 2.0, 1.0, Unit::X),
        ParamDef::new("Decay", 0.2, 20.0, 3.0, Unit::Sec).log(),
        ParamDef::new("Damping", 1000.0, 18000.0, 7000.0, Unit::Hz).log(),
        ParamDef::new("Pre-Delay", 0.0, 0.2, 0.02, Unit::Sec),
        ParamDef::new("Early/Late", 0.0, 1.0, 0.6, Unit::Pct),
        ParamDef::new("Shimmer", 0.0, 0.8, 0.0, Unit::Pct),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 0.3, Unit::Pct),
    ]
});

pub static TRANSIENT_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Attack", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Sustain", -1.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Output", -24.0, 12.0, 0.0, Unit::Db),
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Speed", 0.25, 4.0, 1.0, Unit::X).log(),
    ]
});

pub static MAXIMIZER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Gain", 0.0, 24.0, 3.0, Unit::Db),
        ParamDef::new("Ceiling", -12.0, 0.0, -0.3, Unit::Db),
        ParamDef::new("Release", 0.005, 1.0, 0.08, Unit::Sec).log(),
        ParamDef::new("Warmth", 0.0, 1.0, 0.0, Unit::Pct),
    ]
});

pub static TP_LIMITER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Input", -12.0, 24.0, 0.0, Unit::Db),
        ParamDef::new("Ceiling", -6.0, 0.0, -1.0, Unit::Db),
        ParamDef::new("Release", 0.005, 1.0, 0.1, Unit::Sec).log(),
        ParamDef::new("Lookahead", 1.0, 10.0, 3.0, Unit::Ms),
        ParamDef::options("Reset Meter", &["-", "Reset"], 0),
    ]
});

pub static IMAGER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Low Width", 0.0, 2.0, 0.0, Unit::X),
        ParamDef::new("Mid Width", 0.0, 2.0, 1.0, Unit::X),
        ParamDef::new("High Width", 0.0, 2.0, 1.3, Unit::X),
        ParamDef::new("Low X", 60.0, 600.0, 150.0, Unit::Hz).log(),
        ParamDef::new("High X", 1000.0, 12000.0, 4000.0, Unit::Hz).log(),
        ParamDef::new("Output", -12.0, 12.0, 0.0, Unit::Db),
    ]
});

pub static RACK_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Dry/Wet", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Output", -24.0, 12.0, 0.0, Unit::Db),
    ]
});

// ---------------------------------------------------------------------------------------------
// Voltage (analog poly), Matrix FM, Grain Cloud

pub mod volt {
    pub const W1: usize = 0;
    pub const OCT1: usize = 1;
    pub const W2: usize = 2;
    pub const SEMI2: usize = 3;
    pub const DET2: usize = 4;
    pub const MIX: usize = 5;
    pub const PW: usize = 6;
    pub const SUB: usize = 7;
    pub const NOISE: usize = 8;
    pub const CUTOFF: usize = 9;
    pub const RES: usize = 10;
    pub const FTYPE: usize = 11;
    pub const DRIVE: usize = 12;
    pub const FENV: usize = 13;
    pub const KEYTRK: usize = 14;
    pub const FA: usize = 15;
    pub const FD: usize = 16;
    pub const FS: usize = 17;
    pub const FR: usize = 18;
    pub const AA: usize = 19;
    pub const AD: usize = 20;
    pub const AS: usize = 21;
    pub const AR: usize = 22;
    pub const LRATE: usize = 23;
    pub const LSHAPE: usize = 24;
    pub const L_PITCH: usize = 25;
    pub const L_CUT: usize = 26;
    pub const L_PW: usize = 27;
    pub const UNISON: usize = 28;
    pub const SPREAD: usize = 29;
    pub const GLIDE: usize = 30;
    pub const DRIFT: usize = 31;
    pub const VOICES: usize = 32;
    pub const VOLUME: usize = 33;
}

pub static VOLTAGE_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    const W: &[&str] = &["Saw", "Pulse", "Triangle", "Sine"];
    vec![
        ParamDef::options("Osc1 Wave", W, 0),
        ParamDef::new("Osc1 Oct", -2.0, 2.0, 0.0, Unit::None).step(1.0),
        ParamDef::options("Osc2 Wave", W, 0),
        ParamDef::new("Osc2 Semi", -12.0, 12.0, 0.0, Unit::St).step(1.0),
        ParamDef::new("Osc2 Detune", -50.0, 50.0, 7.0, Unit::Ct),
        ParamDef::new("Osc Mix", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Pulse Width", 0.05, 0.95, 0.5, Unit::Pct),
        ParamDef::new("Sub", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Noise", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Cutoff", 30.0, 18000.0, 2200.0, Unit::Hz).log(),
        ParamDef::new("Reso", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::options("Filter", &["LP 24", "LP 12", "BP", "HP"], 0),
        ParamDef::new("Drive", 0.0, 1.0, 0.1, Unit::Pct),
        ParamDef::new("Flt Env", -1.0, 1.0, 0.4, Unit::Pct),
        ParamDef::new("Key Track", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("F Attack", 0.001, 4.0, 0.005, Unit::Sec).log(),
        ParamDef::new("F Decay", 0.01, 4.0, 0.4, Unit::Sec).log(),
        ParamDef::new("F Sustain", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("F Release", 0.01, 6.0, 0.4, Unit::Sec).log(),
        ParamDef::new("A Attack", 0.001, 4.0, 0.003, Unit::Sec).log(),
        ParamDef::new("A Decay", 0.01, 4.0, 0.5, Unit::Sec).log(),
        ParamDef::new("A Sustain", 0.0, 1.0, 0.7, Unit::Pct),
        ParamDef::new("A Release", 0.01, 6.0, 0.35, Unit::Sec).log(),
        ParamDef::new("LFO Rate", 0.05, 30.0, 4.0, Unit::Hz).log(),
        ParamDef::options("LFO Shape", &["Sine", "Triangle", "Square", "Saw", "S&H"], 0),
        ParamDef::new("LFO>Pitch", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("LFO>Cutoff", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("LFO>PW", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Unison", 1.0, 4.0, 1.0, Unit::None).step(1.0),
        ParamDef::new("Spread", 0.0, 50.0, 10.0, Unit::Ct),
        ParamDef::new("Glide", 0.0, 1.0, 0.0, Unit::Sec),
        ParamDef::new("Drift", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Voices", 1.0, 8.0, 8.0, Unit::None).step(1.0),
        ParamDef::new("Volume", 0.0, 1.0, 0.4, Unit::Pct),
    ]
});

/// Matrix FM: 6 operators x 8 params, then a 6x6 modulation matrix (row = source,
/// column = destination; the diagonal is feedback), then globals.
pub const MFM_OPS: usize = 6;
pub const MFM_OP_PARAMS: usize = 8;
pub const MFM_MATRIX: usize = MFM_OPS * MFM_OP_PARAMS;
pub const MFM_GLOBAL: usize = MFM_MATRIX + MFM_OPS * MFM_OPS;
pub mod mfm {
    pub const RATIO: usize = 0;
    pub const FINE: usize = 1;
    pub const LEVEL: usize = 2;
    pub const OUT: usize = 3;
    pub const A: usize = 4;
    pub const D: usize = 5;
    pub const S: usize = 6;
    pub const R: usize = 7;
    pub const VOLUME: usize = super::MFM_GLOBAL;
    pub const VEL: usize = super::MFM_GLOBAL + 1;
    pub const VOICES: usize = super::MFM_GLOBAL + 2;
    pub const GLIDE: usize = super::MFM_GLOBAL + 3;
}

pub fn mfm_param(op: usize, p: usize) -> usize {
    op * MFM_OP_PARAMS + p
}
pub fn mfm_mod(src: usize, dst: usize) -> usize {
    MFM_MATRIX + src * MFM_OPS + dst
}

pub static MATRIX_FM_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    const NAMES: [[&str; 8]; 6] = [
        ["Op1 Ratio", "Op1 Fine", "Op1 Level", "Op1 Out", "Op1 Attack", "Op1 Decay", "Op1 Sustain", "Op1 Release"],
        ["Op2 Ratio", "Op2 Fine", "Op2 Level", "Op2 Out", "Op2 Attack", "Op2 Decay", "Op2 Sustain", "Op2 Release"],
        ["Op3 Ratio", "Op3 Fine", "Op3 Level", "Op3 Out", "Op3 Attack", "Op3 Decay", "Op3 Sustain", "Op3 Release"],
        ["Op4 Ratio", "Op4 Fine", "Op4 Level", "Op4 Out", "Op4 Attack", "Op4 Decay", "Op4 Sustain", "Op4 Release"],
        ["Op5 Ratio", "Op5 Fine", "Op5 Level", "Op5 Out", "Op5 Attack", "Op5 Decay", "Op5 Sustain", "Op5 Release"],
        ["Op6 Ratio", "Op6 Fine", "Op6 Level", "Op6 Out", "Op6 Attack", "Op6 Decay", "Op6 Sustain", "Op6 Release"],
    ];
    const MODS: [[&str; 6]; 6] = [
        ["1>1", "1>2", "1>3", "1>4", "1>5", "1>6"],
        ["2>1", "2>2", "2>3", "2>4", "2>5", "2>6"],
        ["3>1", "3>2", "3>3", "3>4", "3>5", "3>6"],
        ["4>1", "4>2", "4>3", "4>4", "4>5", "4>6"],
        ["5>1", "5>2", "5>3", "5>4", "5>5", "5>6"],
        ["6>1", "6>2", "6>3", "6>4", "6>5", "6>6"],
    ];
    let mut v = vec![];
    for (op, n) in NAMES.iter().enumerate() {
        v.push(ParamDef::new(n[0], 0.25, 16.0, [1.0, 2.0, 3.0, 1.0, 4.0, 0.5][op], Unit::X).step(0.25));
        v.push(ParamDef::new(n[1], -50.0, 50.0, 0.0, Unit::Ct));
        v.push(ParamDef::new(n[2], 0.0, 1.0, if op == 0 { 1.0 } else { 0.6 }, Unit::Pct));
        v.push(ParamDef::new(n[3], 0.0, 1.0, if op == 0 { 1.0 } else { 0.0 }, Unit::Pct));
        v.push(ParamDef::new(n[4], 0.001, 4.0, 0.002, Unit::Sec).log());
        v.push(ParamDef::new(n[5], 0.01, 8.0, [1.2, 0.4, 0.8, 0.3, 0.2, 2.0][op], Unit::Sec).log());
        v.push(ParamDef::new(n[6], 0.0, 1.0, if op == 0 { 0.6 } else { 0.2 }, Unit::Pct));
        v.push(ParamDef::new(n[7], 0.01, 8.0, 0.4, Unit::Sec).log());
    }
    for (src, row) in MODS.iter().enumerate() {
        for (dst, name) in row.iter().enumerate() {
            // Default patch: op2 -> op1 classic two-operator tone.
            let def = if src == 1 && dst == 0 { 0.35 } else { 0.0 };
            v.push(ParamDef::new(name, 0.0, 1.0, def, Unit::Pct));
        }
    }
    v.push(ParamDef::new("Volume", 0.0, 1.0, 0.4, Unit::Pct));
    v.push(ParamDef::new("Vel > Mod", 0.0, 1.0, 0.5, Unit::Pct));
    v.push(ParamDef::new("Voices", 1.0, 8.0, 8.0, Unit::None).step(1.0));
    v.push(ParamDef::new("Glide", 0.0, 1.0, 0.0, Unit::Sec));
    v
});

pub mod grain {
    pub const SOURCE: usize = 0;
    pub const POS: usize = 1;
    pub const SPRAY: usize = 2;
    pub const SCAN: usize = 3;
    pub const SIZE: usize = 4;
    pub const DENSITY: usize = 5;
    pub const PITCH_SPRAY: usize = 6;
    pub const OCTAVE_MIX: usize = 7;
    pub const REVERSE: usize = 8;
    pub const SPREAD: usize = 9;
    pub const CUTOFF: usize = 10;
    pub const ATTACK: usize = 11;
    pub const RELEASE: usize = 12;
    pub const VOLUME: usize = 13;
}

pub static GRAIN_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Source", &["Sample", "Glass Pad", "Choir", "Strings", "Noise Wash"], 1),
        ParamDef::new("Position", 0.0, 1.0, 0.3, Unit::Pct),
        ParamDef::new("Spray", 0.0, 1.0, 0.15, Unit::Pct),
        ParamDef::new("Scan", -1.0, 1.0, 0.05, Unit::X),
        ParamDef::new("Grain Size", 0.01, 0.5, 0.12, Unit::Sec).log(),
        ParamDef::new("Density", 2.0, 120.0, 30.0, Unit::Hz).log(),
        ParamDef::new("Pitch Spray", 0.0, 100.0, 8.0, Unit::Ct),
        ParamDef::new("Octaves", 0.0, 1.0, 0.15, Unit::Pct),
        ParamDef::new("Reverse", 0.0, 1.0, 0.2, Unit::Pct),
        ParamDef::new("Stereo", 0.0, 1.0, 0.7, Unit::Pct),
        ParamDef::new("Cutoff", 200.0, 18000.0, 9000.0, Unit::Hz).log(),
        ParamDef::new("Attack", 0.005, 8.0, 0.8, Unit::Sec).log(),
        ParamDef::new("Release", 0.05, 12.0, 2.5, Unit::Sec).log(),
        ParamDef::new("Volume", 0.0, 1.0, 0.5, Unit::Pct),
    ]
});

pub static BEATPADS_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Volume", 0.0, 1.0, 0.8, Unit::Pct),
        ParamDef::new("Vel > Level", 0.0, 1.0, 0.7, Unit::Pct),
        ParamDef::new("Tune", -12.0, 12.0, 0.0, Unit::St),
    ]
});

pub mod multi {
    pub const ATTACK: usize = 0;
    pub const DECAY: usize = 1;
    pub const SUSTAIN: usize = 2;
    pub const RELEASE: usize = 3;
    pub const CUTOFF: usize = 4;
    pub const RES: usize = 5;
    pub const FENV: usize = 6;
    pub const VEL_LEVEL: usize = 7;
    pub const VEL_CUT: usize = 8;
    pub const ROUND_ROBIN: usize = 9;
    pub const TRANSPOSE: usize = 10;
    pub const VOICES: usize = 11;
    pub const VOLUME: usize = 12;
}

pub static MULTI_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Attack", 0.001, 4.0, 0.002, Unit::Sec).log(),
        ParamDef::new("Decay", 0.01, 8.0, 1.0, Unit::Sec).log(),
        ParamDef::new("Sustain", 0.0, 1.0, 1.0, Unit::Pct),
        ParamDef::new("Release", 0.01, 8.0, 0.4, Unit::Sec).log(),
        ParamDef::new("Cutoff", 60.0, 20000.0, 20000.0, Unit::Hz).log(),
        ParamDef::new("Reso", 0.0, 1.0, 0.1, Unit::Pct),
        ParamDef::new("Flt Env", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::new("Vel > Level", 0.0, 1.0, 0.7, Unit::Pct),
        ParamDef::new("Vel > Cutoff", 0.0, 1.0, 0.0, Unit::Pct),
        ParamDef::options("Overlaps", &["Layer", "Round Robin", "Random"], 0),
        ParamDef::new("Transpose", -24.0, 24.0, 0.0, Unit::St).step(1.0),
        ParamDef::new("Voices", 1.0, 32.0, 24.0, Unit::None).step(1.0),
        ParamDef::new("Volume", 0.0, 1.0, 0.7, Unit::Pct),
    ]
});

pub mod slicer {
    pub const MODE: usize = 0;
    pub const SENS: usize = 1;
    pub const BEATS: usize = 2;
    pub const SYNC: usize = 3;
    pub const TRANSPOSE: usize = 4;
    pub const ATTACK: usize = 5;
    pub const RELEASE: usize = 6;
    pub const PLAY: usize = 7;
    pub const REVERSE: usize = 8;
    pub const VOLUME: usize = 9;
}

pub const SLICER_MODES: &[&str] = &["Transients", "8 Slices", "16 Slices", "32 Slices"];

pub static SLICER_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::options("Slice By", SLICER_MODES, 0),
        ParamDef::new("Sensitivity", 0.0, 1.0, 0.5, Unit::Pct),
        ParamDef::new("Beats", 1.0, 32.0, 8.0, Unit::None).step(1.0),
        ParamDef::options("Tempo Sync", &["Off", "On"], 1),
        ParamDef::new("Transpose", -24.0, 24.0, 0.0, Unit::St),
        ParamDef::new("Attack", 0.0005, 0.2, 0.001, Unit::Sec).log(),
        ParamDef::new("Release", 0.005, 2.0, 0.03, Unit::Sec).log(),
        ParamDef::options("Playback", &["Mono (cut)", "Poly", "Gate"], 0),
        ParamDef::options("Reverse", &["Off", "On"], 0),
        ParamDef::new("Volume", 0.0, 1.0, 0.8, Unit::Pct),
    ]
});

pub static STRUM_PARAMS: LazyLock<Vec<ParamDef>> = LazyLock::new(|| {
    vec![
        ParamDef::new("Time", 0.0, 120.0, 25.0, Unit::Ms),
        ParamDef::options("Direction", &["Up", "Down", "Alternate", "Random"], 0),
        ParamDef::new("Vel Slope", -0.5, 0.5, -0.1, Unit::Pct),
        ParamDef::new("Humanize", 0.0, 1.0, 0.1, Unit::Pct),
    ]
});
