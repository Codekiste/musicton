//! Project data model. The UI owns a mutable `Project`; the audio engine receives immutable
//! snapshots (`Arc<Project>`) whenever it changes.

use crate::params::{self, ParamDef};
use serde::{Deserialize, Serialize};

pub const STEPS_PER_BAR: u32 = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum InstrumentKind {
    Drums,
    Bass,
    Synth,
    #[serde(alias = "Simpler")]
    Sampler,
    #[serde(alias = "Operator")]
    Fm4,
    #[serde(alias = "Wavetable")]
    Morph,
    #[serde(alias = "Tension")]
    StringModel,
    Audio,
    Group,
    B303,
    Nexa,
    Voltage,
    MatrixFm,
    GrainCloud,
    BeatPads,
    MultiSampler,
    Slicer,
}

impl InstrumentKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Drums => "Drum Machine",
            Self::Bass => "Acid Bass",
            Self::Synth => "Poly Synth",
            Self::Sampler => "Sampler",
            Self::Fm4 => "FM4",
            Self::Morph => "Morph",
            Self::StringModel => "String Model",
            Self::Audio => "Audio Track",
            Self::Group => "Group Track",
            Self::B303 => "B-303 Bassline",
            Self::Nexa => "Nexa",
            Self::Voltage => "Voltage",
            Self::MatrixFm => "Matrix FM",
            Self::GrainCloud => "Grain Cloud",
            Self::BeatPads => "Beat Pads",
            Self::MultiSampler => "Multi Sampler",
            Self::Slicer => "Slicer",
        }
    }
    pub const ALL: [InstrumentKind; 15] = [
        Self::Drums,
        Self::BeatPads,
        Self::B303,
        Self::Bass,
        Self::Nexa,
        Self::Voltage,
        Self::Synth,
        Self::MatrixFm,
        Self::Fm4,
        Self::Morph,
        Self::GrainCloud,
        Self::Sampler,
        Self::MultiSampler,
        Self::Slicer,
        Self::StringModel,
    ];
    pub fn info(self) -> &'static str {
        match self {
            Self::Drums => "Synthesized drum machine with ten lanes; every lane can also play a WAV sample.",
            Self::Bass => "Monophonic acid bass with accent and slide.",
            Self::Synth => "Subtractive polyphonic synthesizer with unison.",
            Self::Sampler => "Sampler: play a WAV melodically (Classic), as one-shot, or chop it into slices across the keyboard (Slice).",
            Self::Fm4 => "4-operator FM synthesizer with 8 algorithms - metallic bells, basses and plucks.",
            Self::Morph => "Two morphing wavetable oscillators with unison, filter and an LFO on the table position.",
            Self::StringModel => "Physically modelled string (Karplus-Strong) with pluck, hammer or bow exciter.",
            Self::Audio => "Plays audio clips (WAV loops and samples), warped to the song tempo.",
            Self::Group => "Sums its member tracks into one channel with its own effects and fader.",
            Self::B303 => "acid bassline: saw/square VCO, resonant diode-ladder style filter, accent with sweep circuit, slide - with a classic 16-step pattern editor.",
            Self::Voltage => "Analog-style polysynth: two oscillators with pulse-width modulation, sub and noise, 24/12 dB filter with drive, two envelopes, LFO, unison, glide and analog drift.",
            Self::MatrixFm => "Six-operator FM synth with a free modulation matrix: every operator can modulate every other (and itself), each with its own envelope and output level.",
            Self::MultiSampler => "Multisample instrument: key and velocity zones, each with its own WAV, round robin, filter and envelopes. Load a folder and the notes in the file names map the zones automatically.",
            Self::Slicer => "Chops a loop into slices at its transients or on a beat grid and plays them from C1 upwards. One click turns the slices into a clip that replays the loop.",
            Self::BeatPads => "Sixteen pads, each with its own sample or synthesized drum voice, tuning, decay, choke group and its own effect chain.",
            Self::GrainCloud => "Granular pad synth: clouds of tiny grains from a WAV or built-in sources, with position scan, spray, pitch scatter, octaves and reverse grains.",
            Self::Nexa => "preset ROMpler: browse ready-made trance/EDM sounds by category, shape them with macros, built-in arpeggiator, trancegate and effects.",
        }
    }
    pub fn defs(self) -> &'static [ParamDef] {
        match self {
            Self::Drums => &params::DRUM_PARAMS,
            Self::Bass => &params::BASS_PARAMS,
            Self::Synth => &params::SYNTH_PARAMS,
            Self::Sampler => &params::SAMPLER_PARAMS,
            Self::Fm4 => &params::FM4_PARAMS,
            Self::Morph => &params::WAVETABLE_PARAMS,
            Self::StringModel => &params::STRING_PARAMS,
            Self::Audio | Self::Group => &[],
            Self::B303 => &params::B303_PARAMS,
            Self::Nexa => &params::NEXA_PARAMS,
            Self::Voltage => &params::VOLTAGE_PARAMS,
            Self::MatrixFm => &params::MATRIX_FM_PARAMS,
            Self::GrainCloud => &params::GRAIN_PARAMS,
            Self::BeatPads => &params::BEATPADS_PARAMS,
            Self::MultiSampler => &params::MULTI_PARAMS,
            Self::Slicer => &params::SLICER_PARAMS,
        }
    }
    /// Instruments whose clips are edited in the piano roll with a melodic range.
    pub fn is_melodic(self) -> bool {
        !matches!(self, Self::Drums | Self::BeatPads | Self::Audio | Self::Group)
    }
    /// Instruments played lane by lane (pitch = lane index) in the step sequencer.
    pub fn is_drum_like(self) -> bool {
        matches!(self, Self::Drums | Self::BeatPads)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DeviceKind {
    #[serde(alias = "AutoFilter")]
    FilterSweep,
    #[serde(alias = "Eq3")]
    DjEq,
    Saturator,
    Echo,
    Reverb,
    Compressor,
    Limiter,
    Sidechain,
    #[serde(alias = "Redux")]
    Bitcrusher,
    Utility,
    #[serde(alias = "AutoPan")]
    PanLfo,
    #[serde(alias = "BeatRepeat")]
    Stutter,
    Chorus,
    #[serde(alias = "Corpus")]
    BodyResonator,
    #[serde(alias = "DrumBuss")]
    DrumShaper,
    #[serde(alias = "DynamicTube")]
    TubeDrive,
    Eq8,
    #[serde(alias = "Erosion")]
    Degrader,
    #[serde(alias = "FilterDelay")]
    TripleDelay,
    Flanger,
    FreqShifter,
    Gate,
    #[serde(alias = "GlueCompressor")]
    BusCompressor,
    GrainDelay,
    Multiband,
    Overdrive,
    #[serde(alias = "Pedal")]
    Stompbox,
    Phaser,
    PingPong,
    #[serde(alias = "Resonators")]
    ChordResonator,
    #[serde(alias = "SimpleDelay")]
    StereoDelay,
    Vinyl,
    #[serde(alias = "Spectrum")]
    Analyzer,
    Tuner,
    Vocoder,
    ChannelEq,
    Rack,
    BeatMangler,
    HeatStack,
    HybridReverb,
    TransientShaper,
    Maximizer,
    TruePeakLimiter,
    StereoImager,
    SpacePanner,
    PitchFix,
}

/// Browser folders for audio effect categories.
pub const DEVICE_CATEGORIES: &[(&str, &[DeviceKind])] = &[
    ("Delay & Loop", &[DeviceKind::BeatMangler, DeviceKind::Stutter, DeviceKind::Echo, DeviceKind::TripleDelay, DeviceKind::GrainDelay, DeviceKind::PingPong, DeviceKind::StereoDelay]),
    ("Distortion", &[DeviceKind::TubeDrive, DeviceKind::Degrader, DeviceKind::Overdrive, DeviceKind::Stompbox, DeviceKind::Bitcrusher, DeviceKind::Saturator, DeviceKind::HeatStack, DeviceKind::Vinyl]),
    ("Drums", &[DeviceKind::DrumShaper, DeviceKind::TransientShaper]),
    ("Dynamics", &[DeviceKind::Compressor, DeviceKind::Gate, DeviceKind::BusCompressor, DeviceKind::Limiter, DeviceKind::Multiband, DeviceKind::Sidechain, DeviceKind::Maximizer, DeviceKind::TruePeakLimiter]),
    ("EQ & Filters", &[DeviceKind::FilterSweep, DeviceKind::ChannelEq, DeviceKind::DjEq, DeviceKind::Eq8]),
    ("Modulation", &[DeviceKind::PanLfo, DeviceKind::Chorus, DeviceKind::Flanger, DeviceKind::FreqShifter, DeviceKind::Phaser]),
    ("Reverb & Resonance", &[DeviceKind::BodyResonator, DeviceKind::ChordResonator, DeviceKind::Reverb, DeviceKind::HybridReverb, DeviceKind::Vocoder]),
    ("Utilities", &[DeviceKind::Analyzer, DeviceKind::Tuner, DeviceKind::Utility, DeviceKind::StereoImager, DeviceKind::SpacePanner, DeviceKind::PitchFix, DeviceKind::Rack]),
];

impl DeviceKind {
    /// Devices whose detector can listen to another track.
    pub fn has_sidechain(self) -> bool {
        matches!(self, Self::Compressor | Self::Sidechain | Self::Gate | Self::BusCompressor)
    }
    pub fn all() -> impl Iterator<Item = DeviceKind> {
        DEVICE_CATEGORIES.iter().flat_map(|(_, k)| k.iter().copied())
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::FilterSweep => "Filter Sweep",
            Self::DjEq => "DJ EQ",
            Self::Saturator => "Saturator",
            Self::Echo => "Tape Echo",
            Self::Reverb => "Reverb",
            Self::Compressor => "Compressor",
            Self::Limiter => "Limiter",
            Self::Sidechain => "Sidechain",
            Self::Bitcrusher => "Bitcrusher",
            Self::Utility => "Utility",
            Self::PanLfo => "Pan LFO",
            Self::Stutter => "Stutter",
            Self::Chorus => "Chorus",
            Self::BodyResonator => "Body Resonator",
            Self::DrumShaper => "Drum Shaper",
            Self::TubeDrive => "Tube Drive",
            Self::Eq8 => "Parametric EQ",
            Self::Degrader => "Degrader",
            Self::TripleDelay => "Triple Delay",
            Self::Flanger => "Flanger",
            Self::FreqShifter => "Frequency Shifter",
            Self::Gate => "Gate",
            Self::BusCompressor => "Bus Compressor",
            Self::GrainDelay => "Grain Delay",
            Self::Multiband => "Multiband Compressor",
            Self::Overdrive => "Overdrive",
            Self::Stompbox => "Stompbox",
            Self::Phaser => "Phaser",
            Self::PingPong => "Ping Pong Delay",
            Self::ChordResonator => "Chord Resonator",
            Self::StereoDelay => "Stereo Delay",
            Self::Vinyl => "Vinyl",
            Self::Analyzer => "Analyzer",
            Self::Tuner => "Tuner",
            Self::Vocoder => "Vocoder",
            Self::ChannelEq => "Channel EQ",
            Self::Rack => "Rack",
            Self::BeatMangler => "Beat Mangler",
            Self::HeatStack => "Heat Stack",
            Self::HybridReverb => "Hybrid Reverb",
            Self::TransientShaper => "Transient Shaper",
            Self::Maximizer => "Maximizer",
            Self::TruePeakLimiter => "True Peak Limiter",
            Self::StereoImager => "Stereo Imager",
            Self::SpacePanner => "Space Panner",
            Self::PitchFix => "Pitch Fix",
        }
    }
    pub fn info(self) -> &'static str {
        match self {
            Self::FilterSweep => "Resonant filter with a tempo-synced LFO. The classic tool for techno filter sweeps and builds.",
            Self::DjEq => "Three-band DJ-style EQ. Pull a band down to -24 dB to kill it completely.",
            Self::Saturator => "Waveshaping distortion. Adds warmth, grit and perceived loudness.",
            Self::Echo => "Tempo-synced stereo ping-pong delay with filters in the feedback path.",
            Self::Reverb => "Algorithmic reverb. Long tails with a low cut make deep warehouse spaces.",
            Self::Compressor => "Dynamics compressor. Glues sounds together and tames peaks.",
            Self::Limiter => "Fast peak limiter for the master. Raise Gain for loudness, Ceiling sets the maximum level.",
            Self::Sidechain => "Ducks the track every time a kick plays - the pumping techno sound.",
            Self::Bitcrusher => "Bit and sample-rate reduction for lo-fi crunch.",
            Self::Utility => "Gain, pan, stereo width and a low cut to clean up non-bass tracks.",
            Self::PanLfo => "Tempo-synced LFO on volume and panning. Phase 180° = auto pan, 0° = tremolo.",
            Self::Stutter => "Captures a slice of the incoming audio and repeats it in rhythm - stutters, rolls and glitches. Chance sets how often it triggers.",
            Self::Chorus => "Modulated short delays that thicken and widen sounds.",
            Self::BodyResonator => "Simulates the resonance of physical objects (beams, membranes, pipes...). Great on percussion.",
            Self::DrumShaper => "All-in-one drum processor: drive, crunch, transient shaping and a tuned low-end Boom.",
            Self::TubeDrive => "Tube saturation with bias - warm, asymmetric distortion.",
            Self::Eq8 => "Eight-band parametric EQ with cuts, shelves, bells and a notch. Drag the dots in the display.",
            Self::Degrader => "Degrades the sound with modulated micro-delays driven by noise or a sine wave.",
            Self::TripleDelay => "Three independent filtered delay lines: left, stereo and right.",
            Self::Flanger => "Short modulated delay with feedback for jet-like sweeps.",
            Self::FreqShifter => "Shifts all frequencies by a fixed amount (inharmonic) or ring-modulates the signal.",
            Self::Gate => "Mutes the signal below the threshold. Tightens drums and removes noise.",
            Self::BusCompressor => "Bus compressor modelled on classic console designs - glues groups and the master together.",
            Self::GrainDelay => "Splits the delayed signal into grains with random spray and pitch.",
            Self::Multiband => "Three-band compressor/expander with upward and downward compression.",
            Self::Overdrive => "Band-limited overdrive like a guitar pedal.",
            Self::Stompbox => "Guitar pedal distortion: overdrive, distortion and fuzz with a 3-band EQ.",
            Self::Phaser => "Sweeping all-pass filter phaser.",
            Self::PingPong => "Delay bouncing between left and right through a band-pass filter.",
            Self::ChordResonator => "Five tuned resonators that turn any input into chords.",
            Self::StereoDelay => "Basic stereo delay with separate left and right times.",
            Self::Vinyl => "Record-player artefacts: tracing distortion, pinch and crackle.",
            Self::Analyzer => "Real-time frequency analyser.",
            Self::Tuner => "Shows the pitch of the incoming audio.",
            Self::Vocoder => "Imposes the spectrum of the track onto an internal saw, pulse or noise carrier - robot voices and talking synths.",
            Self::ChannelEq => "Simple, musical channel-strip EQ with low, sweepable mid and high bands.",
            Self::BeatMangler => "Tempo-synced performance effect: 16-step patterns of gates, stutters, loops, reverse, half-speed and tape stops. Only acts while the transport runs.",
            Self::HeatStack => "Three saturation stages in series (tube, tape, fuzz, fold, rectify, crush) with low cut, tone filter and bias, 2x oversampled.",
            Self::HybridReverb => "Convolution early reflections from five room characters plus a modulated feedback-delay tail, with octave-up shimmer and pre-delay.",
            Self::TransientShaper => "Boosts or softens the attack and the sustain of sounds independently of their level - punchier drums or tighter tails.",
            Self::Maximizer => "Loudness maximizer: input gain into a transparent lookahead limiter with optional warm pre-saturation.",
            Self::TruePeakLimiter => "Mastering limiter with 4x oversampled true-peak detection and a loudness meter (momentary, short-term, integrated LUFS, true peak).",
            Self::SpacePanner => "Binaural 3D panner for headphones: place a sound anywhere around, above or behind you, set its distance, let it orbit your head.",
            Self::PitchFix => "Real-time pitch correction for monophonic voices and instruments: snaps the pitch to the key and scale, from natural (slow) to hard robotic (fast).",
            Self::StereoImager => "Three-band stereo width (mono bass, wide highs) with adjustable crossovers and a correlation meter.",
            Self::Rack => "Container with parallel chains: each chain has its own devices, volume and pan; the outputs are summed (parallel compression, layered effects, dry/wet splits).",
        }
    }
    pub fn defs(self) -> &'static [ParamDef] {
        match self {
            Self::FilterSweep => &params::AUTOFILTER_PARAMS,
            Self::DjEq => &params::EQ3_PARAMS,
            Self::Saturator => &params::SATURATOR_PARAMS,
            Self::Echo => &params::ECHO_PARAMS,
            Self::Reverb => &params::REVERB_PARAMS,
            Self::Compressor => &params::COMPRESSOR_PARAMS,
            Self::Limiter => &params::LIMITER_PARAMS,
            Self::Sidechain => &params::SIDECHAIN_PARAMS,
            Self::Bitcrusher => &params::REDUX_PARAMS,
            Self::Utility => &params::UTILITY_PARAMS,
            Self::PanLfo => &params::AUTOPAN_PARAMS,
            Self::Stutter => &params::BEATREPEAT_PARAMS,
            Self::Chorus => &params::CHORUS_PARAMS,
            Self::BodyResonator => &params::CORPUS_PARAMS,
            Self::DrumShaper => &params::DRUMBUSS_PARAMS,
            Self::TubeDrive => &params::DYNTUBE_PARAMS,
            Self::Eq8 => &params::EQ8_PARAMS,
            Self::Degrader => &params::EROSION_PARAMS,
            Self::TripleDelay => &params::FILTERDELAY_PARAMS,
            Self::Flanger => &params::FLANGER_PARAMS,
            Self::FreqShifter => &params::FREQSHIFT_PARAMS,
            Self::Gate => &params::GATE_PARAMS,
            Self::BusCompressor => &params::GLUE_PARAMS,
            Self::GrainDelay => &params::GRAINDELAY_PARAMS,
            Self::Multiband => &params::MULTIBAND_PARAMS,
            Self::Overdrive => &params::OVERDRIVE_PARAMS,
            Self::Stompbox => &params::PEDAL_PARAMS,
            Self::Phaser => &params::PHASER_PARAMS,
            Self::PingPong => &params::PINGPONG_PARAMS,
            Self::ChordResonator => &params::RESONATORS_PARAMS,
            Self::StereoDelay => &params::SIMPLEDELAY_PARAMS,
            Self::Vinyl => &params::VINYL_PARAMS,
            Self::Analyzer => &params::SPECTRUM_PARAMS,
            Self::Tuner => &params::TUNER_PARAMS,
            Self::Vocoder => &params::VOCODER_PARAMS,
            Self::ChannelEq => &params::CHANNELEQ_PARAMS,
            Self::Rack => &params::RACK_PARAMS,
            Self::BeatMangler => &params::MANGLER_PARAMS,
            Self::HeatStack => &params::HEAT_PARAMS,
            Self::HybridReverb => &params::HYBRID_VERB_PARAMS,
            Self::TransientShaper => &params::TRANSIENT_PARAMS,
            Self::Maximizer => &params::MAXIMIZER_PARAMS,
            Self::TruePeakLimiter => &params::TP_LIMITER_PARAMS,
            Self::StereoImager => &params::IMAGER_PARAMS,
            Self::SpacePanner => &params::SPACE_PARAMS,
            Self::PitchFix => &params::PITCHFIX_PARAMS,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MidiFxKind {
    Arpeggiator,
    Chord,
    NoteLength,
    Pitch,
    Random,
    Scale,
    Velocity,
    Strum,
}

impl MidiFxKind {
    pub const ALL: [MidiFxKind; 8] = [Self::Arpeggiator, Self::Chord, Self::NoteLength, Self::Pitch, Self::Random, Self::Scale, Self::Strum, Self::Velocity];
    pub fn name(self) -> &'static str {
        match self {
            Self::Arpeggiator => "Arpeggiator",
            Self::Chord => "Chord",
            Self::NoteLength => "Note Length",
            Self::Pitch => "Pitch",
            Self::Random => "Random",
            Self::Scale => "Scale",
            Self::Velocity => "Velocity",
            Self::Strum => "Strum",
        }
    }
    pub fn info(self) -> &'static str {
        match self {
            Self::Arpeggiator => "Plays held notes one after another in a pattern. Works on clip notes and on what you play (while the transport runs).",
            Self::Chord => "Adds up to six notes at fixed intervals to every note - instant chords from single notes.",
            Self::NoteLength => "Sets every note to a fixed length (Sync) or scales its length (Gate).",
            Self::Pitch => "Transposes notes and drops those outside the Lowest/Highest range.",
            Self::Random => "Randomly transposes notes - great for generative melodies.",
            Self::Scale => "Forces every note into the chosen key and scale. No wrong notes.",
            Self::Velocity => "Reshapes, limits or randomizes note velocities.",
            Self::Strum => "Spreads the notes of a chord in time like a strummed guitar: up, down, alternating or random, with a velocity slope.",
        }
    }
    pub fn defs(self) -> &'static [ParamDef] {
        match self {
            Self::Arpeggiator => &params::ARP_PARAMS,
            Self::Chord => &params::CHORD_PARAMS,
            Self::NoteLength => &params::NOTELENGTH_PARAMS,
            Self::Pitch => &params::PITCH_PARAMS,
            Self::Random => &params::RANDOM_PARAMS,
            Self::Scale => &params::SCALE_PARAMS,
            Self::Velocity => &params::VELOCITY_PARAMS,
            Self::Strum => &params::STRUM_PARAMS,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MidiFx {
    pub id: u64,
    pub kind: MidiFxKind,
    pub on: bool,
    pub params: Vec<f32>,
}

impl MidiFx {
    pub fn new(id: u64, kind: MidiFxKind) -> Self {
        Self { id, kind, on: true, params: params::defaults(kind.defs()) }
    }
}

pub use b303_core::{SeqBank, SeqPattern, SeqStep};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instrument {
    pub kind: InstrumentKind,
    pub params: Vec<f32>,
    /// Drum machine only: optional WAV file per lane replacing the synthesized sound.
    #[serde(default)]
    pub samples: Vec<Option<String>>,
    /// B-303 only: internal sequencer patterns.
    #[serde(default)]
    pub bank: Option<SeqBank>,
    /// Beat Pads only.
    #[serde(default)]
    pub pads: Vec<Pad>,
    /// Multi Sampler only.
    #[serde(default)]
    pub zones: Vec<Zone>,
}

/// A key/velocity zone of the Multi Sampler.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub path: String,
    pub root: u8,
    pub lo: u8,
    pub hi: u8,
    pub vlo: u8,
    pub vhi: u8,
    /// Cents.
    pub tune: f32,
    /// dB.
    pub gain: f32,
}

impl Zone {
    pub fn new(path: &str, root: u8) -> Self {
        Self { path: path.into(), root, lo: root, hi: root, vlo: 0, vhi: 127, tune: 0.0, gain: 0.0 }
    }
}

/// Finds a note name (C3, F#2, Db4) or a MIDI number in a file name. C3 = 60.
pub fn note_in_name(name: &str) -> Option<u8> {
    let up: Vec<char> = name.chars().collect();
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let flats = [("DB", 1), ("EB", 3), ("GB", 6), ("AB", 8), ("BB", 10)];
    for i in 0..up.len() {
        // A note letter not preceded by another letter.
        if i > 0 && up[i - 1].is_ascii_alphabetic() {
            continue;
        }
        let c = up[i].to_ascii_uppercase();
        if !"CDEFGAB".contains(c) {
            continue;
        }
        let mut j = i + 1;
        let mut semi = names.iter().position(|n| *n == c.to_string()).unwrap() as i32;
        if j < up.len() && (up[j] == '#' || up[j] == 's') {
            semi += 1;
            j += 1;
        } else if j < up.len() && up[j] == 'b' {
            if let Some((_, s)) = flats.iter().find(|(f, _)| f.starts_with(c)) {
                semi = *s;
                j += 1;
            }
        }
        let neg = j < up.len() && up[j] == '-';
        if neg {
            j += 1;
        }
        if j < up.len() && up[j].is_ascii_digit() && (j + 1 >= up.len() || !up[j + 1].is_ascii_digit()) {
            let oct = up[j].to_digit(10).unwrap() as i32 * if neg { -1 } else { 1 };
            let midi = (oct + 2) * 12 + semi;
            if (0..128).contains(&midi) {
                return Some(midi as u8);
            }
        }
    }
    // Plain MIDI number surrounded by separators, e.g. "piano_060.wav".
    name.split(|c: char| !c.is_ascii_digit()).filter(|s| s.len() >= 2 && s.len() <= 3).filter_map(|s| s.parse::<u8>().ok()).find(|n| (21..=108).contains(n))
}

/// Builds zones from sample files: notes in names become roots, key ranges are split halfway
/// between neighbouring roots. Files without a note are laid out chromatically from C2.
pub fn zones_from_files(paths: &[String]) -> Vec<Zone> {
    let mut found: Vec<(u8, String)> = vec![];
    let mut next = 48u8;
    for p in paths {
        let stem = std::path::Path::new(p).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let root = note_in_name(&stem).unwrap_or_else(|| {
            next = next.saturating_add(1);
            next - 1
        });
        found.push((root, p.clone()));
    }
    found.sort_by_key(|f| f.0);
    found.dedup_by_key(|f| f.0);
    let n = found.len();
    (0..n)
        .map(|i| {
            let root = found[i].0;
            let lo = if i == 0 { 0 } else { ((found[i - 1].0 as u16 + root as u16) / 2 + 1) as u8 };
            let hi = if i + 1 == n { 127 } else { ((root as u16 + found[i + 1].0 as u16) / 2) as u8 };
            Zone { lo, hi, ..Zone::new(&found[i].1, root) }
        })
        .collect()
}

/// One pad of the Beat Pads instrument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    pub name: String,
    /// WAV sample; without one the pad plays the synthesized drum voice `voice`.
    pub sample: Option<String>,
    pub voice: u8,
    /// Semitones.
    pub tune: f32,
    /// dB.
    pub gain: f32,
    pub pan: f32,
    /// Amplitude decay in seconds (long values play the whole sample).
    pub decay: f32,
    /// 0 = none, 1..4 = choke group: a pad cuts off other pads of its group.
    pub choke: u8,
    pub reverse: bool,
    #[serde(default)]
    pub devices: Vec<Device>,
}

impl Pad {
    pub fn new(i: usize) -> Self {
        let voice = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 2, 4, 5, 9, 1][i % 16] as u8;
        let name = params::DRUM_LANES[voice as usize].to_string();
        // Closed and open hats choke each other.
        let choke = if voice == 4 || voice == 5 { 1 } else { 0 };
        Self { name, sample: None, voice, tune: 0.0, gain: 0.0, pan: 0.0, decay: 2.0, choke, reverse: false, devices: vec![] }
    }
    pub fn default_set() -> Vec<Pad> {
        (0..16).map(Pad::new).collect()
    }
}

impl Instrument {
    /// Lane names for drum-like instruments (sample file names where loaded).
    pub fn lane_names(&self) -> Vec<String> {
        let stem = |s: &str| std::path::Path::new(s).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        match self.kind {
            InstrumentKind::BeatPads => self.pads.iter().map(|p| if p.name.is_empty() { p.sample.as_deref().map(stem).unwrap_or_default() } else { p.name.clone() }).collect(),
            InstrumentKind::Drums => params::DRUM_LANES
                .iter()
                .enumerate()
                .map(|(i, n)| self.samples.get(i).cloned().flatten().map(|s| stem(&s)).unwrap_or_else(|| n.to_string()))
                .collect(),
            _ => vec![],
        }
    }
    pub fn new(kind: InstrumentKind) -> Self {
        let samples = match kind {
            InstrumentKind::Drums => vec![None; params::DRUM_LANES.len()],
            InstrumentKind::Sampler | InstrumentKind::GrainCloud | InstrumentKind::Slicer => vec![None],
            _ => vec![],
        };
        let bank = (kind == InstrumentKind::B303).then(SeqBank::factory);
        let pads = if kind == InstrumentKind::BeatPads { Pad::default_set() } else { vec![] };
        Self { kind, params: params::defaults(kind.defs()), samples, bank, pads, zones: vec![] }
    }
    pub fn normalize(&mut self) {
        normalize_params(&mut self.params, self.kind.defs());
        match self.kind {
            InstrumentKind::Drums => self.samples.resize(params::DRUM_LANES.len(), None),
            InstrumentKind::Sampler | InstrumentKind::GrainCloud | InstrumentKind::Slicer => self.samples.resize(1, None),
            InstrumentKind::B303 if self.bank.is_none() => self.bank = Some(SeqBank::factory()),
            InstrumentKind::BeatPads => {
                let n = self.pads.len();
                self.pads.extend((n..16).map(Pad::new));
                self.pads.truncate(16);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: u64,
    pub kind: DeviceKind,
    pub on: bool,
    pub params: Vec<f32>,
    /// Compressor/Gate/Sidechain: track whose signal drives the device instead of its input.
    #[serde(default)]
    pub sc_source: Option<u64>,
    /// Rack only: parallel chains whose outputs are summed.
    #[serde(default)]
    pub chains: Vec<RackChain>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RackChain {
    pub name: String,
    pub devices: Vec<Device>,
    /// Fader position like a channel (0.85 = 0 dB).
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
}

impl RackChain {
    pub fn new(name: &str) -> Self {
        Self { name: name.into(), devices: vec![], volume: 0.85, pan: 0.0, mute: false }
    }
}

impl Device {
    pub fn new(id: u64, kind: DeviceKind) -> Self {
        let chains = if kind == DeviceKind::Rack { vec![RackChain::new("Chain 1"), RackChain::new("Chain 2")] } else { vec![] };
        Self { id, kind, on: true, params: params::defaults(kind.defs()), sc_source: None, chains }
    }
    pub fn with(mut self, idx: usize, v: f32) -> Self {
        self.params[idx] = v;
        self
    }
}

fn normalize_params(p: &mut Vec<f32>, defs: &[ParamDef]) {
    if p.len() < defs.len() {
        p.extend(defs[p.len()..].iter().map(|d| d.def));
    }
    p.truncate(defs.len());
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
    pub devices: Vec<Device>,
    /// Fader position 0..1 (0.85 = 0 dB).
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub sends: [f32; 2],
    /// Crossfader assignment: 0 = none, 1 = A, 2 = B.
    #[serde(default)]
    pub xfade: u8,
}

impl Default for Channel {
    fn default() -> Self {
        Self { devices: vec![], volume: 0.85, pan: 0.0, mute: false, solo: false, sends: [0.0, 0.0], xfade: 0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub step: u32,
    pub len: u32,
    pub pitch: u8,
    pub vel: u8,
    #[serde(default)]
    pub slide: bool,
    /// Chance in percent that the note is skipped on a pass (0 = always plays).
    #[serde(default)]
    pub skip: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WarpMode {
    /// Plays at the original speed, not synced to the tempo.
    Off,
    /// Changes playback speed (and pitch) to follow the tempo, like a turntable.
    Repitch,
    /// Granular time-stretch: follows the tempo and keeps the pitch.
    Beats,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub path: String,
    pub warp: WarpMode,
    /// Original tempo of the material (used when warping).
    pub bpm: f32,
    pub gain: f32,
    pub transpose: f32,
    /// Start offset into the file in seconds.
    #[serde(default)]
    pub offset: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub color: [u8; 3],
    pub len: u32,
    pub notes: Vec<Note>,
    #[serde(default)]
    pub audio: Option<AudioClip>,
    #[serde(default)]
    pub follow: Option<Follow>,
    /// Clip gain in dB, applied before the device chain.
    #[serde(default)]
    pub gain: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FollowAction {
    Next,
    Previous,
    First,
    Last,
    Any,
    Other,
    Again,
    Stop,
}

impl FollowAction {
    pub const ALL: [FollowAction; 8] = [Self::Next, Self::Previous, Self::First, Self::Last, Self::Any, Self::Other, Self::Again, Self::Stop];
    pub fn name(self) -> &'static str {
        match self {
            Self::Next => "Next",
            Self::Previous => "Previous",
            Self::First => "First",
            Self::Last => "Last",
            Self::Any => "Any",
            Self::Other => "Other",
            Self::Again => "Play Again",
            Self::Stop => "Stop",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Follow {
    pub action: FollowAction,
    /// Bars after which the action fires.
    pub bars: u32,
    /// Probability 0..1 that the action happens (otherwise the clip keeps looping).
    pub chance: f32,
}

impl Clip {
    /// Builds an audio clip, guessing the loop tempo from the file length.
    pub fn from_audio(path: &str, frames: usize, sample_rate: f32, project_bpm: f32, color: [u8; 3]) -> Self {
        let secs = frames as f32 / sample_rate.max(1.0);
        let name = std::path::Path::new(path).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "Audio".into());
        // Loops: find a bar count that gives a plausible tempo near the project tempo.
        let mut best: Option<(f32, u32)> = None;
        for bars in [1u32, 2, 4, 8, 16, 32] {
            let bpm = bars as f32 * 4.0 * 60.0 / secs;
            if (70.0..=190.0).contains(&bpm) {
                let score = (bpm - project_bpm).abs();
                if best.is_none_or(|(b, _)| score < (b - project_bpm).abs()) {
                    best = Some((bpm, bars));
                }
            }
        }
        let lower = name.to_lowercase();
        let looks_like_loop = lower.contains("loop") || lower.contains("bpm") || secs >= 4.0;
        let (warp, bpm, len) = match best {
            Some((bpm, bars)) if looks_like_loop => (WarpMode::Beats, bpm, bars * STEPS_PER_BAR),
            _ => {
                let steps = (secs * project_bpm / 60.0 * 4.0).ceil().max(1.0) as u32;
                (WarpMode::Off, project_bpm, steps.div_ceil(4) * 4)
            }
        };
        Self {
            name,
            color,
            len,
            notes: vec![],
            audio: Some(AudioClip { path: path.into(), warp, bpm, gain: 0.0, transpose: 0.0, offset: 0.0 }),
            follow: None,
            gain: 0.0,
        }
    }

    pub fn new(name: &str, color: [u8; 3], bars: u32) -> Self {
        Self { name: name.into(), color, len: bars * STEPS_PER_BAR, notes: vec![], audio: None, follow: None, gain: 0.0 }
    }
    pub fn bars(&self) -> u32 {
        self.len.div_ceil(STEPS_PER_BAR)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AutoTarget {
    Volume,
    Pan,
    Send(u8),
    Instrument(usize),
    Device { id: u64, param: usize },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Automation {
    pub target: AutoTarget,
    /// (position in steps, normalized value 0..1), sorted by position.
    pub points: Vec<(f32, f32)>,
}

impl Automation {
    pub fn value_at(&self, pos: f32) -> Option<f32> {
        let pts = &self.points;
        let first = pts.first()?;
        if pos <= first.0 {
            return Some(first.1);
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if pos <= b.0 {
                let t = if b.0 > a.0 { (pos - a.0) / (b.0 - a.0) } else { 1.0 };
                return Some(a.1 + (b.1 - a.1) * t);
            }
        }
        Some(pts.last().unwrap().1)
    }
}

/// Anything a MIDI controller, a computer key or a macro can control.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum MapTarget {
    Volume(u64),
    Pan(u64),
    Send(u64, u8),
    Mute(u64),
    Solo(u64),
    Instrument(u64, usize),
    Device(u64, usize),
    MidiFx(u64, usize),
    Macro(u64, u8),
    ReturnVolume(u8),
    MasterVolume,
    Crossfader,
    Tempo,
    LaunchScene(usize),
    LaunchClip(u64, usize),
    StopTrack(u64),
    StopAll,
    Play,
    Stop,
}

impl MapTarget {
    /// Buttons fire on press; everything else is continuous.
    pub fn is_trigger(self) -> bool {
        matches!(self, Self::LaunchScene(_) | Self::LaunchClip(..) | Self::StopTrack(_) | Self::StopAll | Self::Play | Self::Stop | Self::Mute(_) | Self::Solo(_))
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum MapSource {
    Cc { ch: u8, cc: u8 },
    Note { ch: u8, note: u8 },
    Key(String),
}

impl MapSource {
    pub fn label(&self) -> String {
        match self {
            Self::Cc { ch, cc } => format!("CC {cc} (ch {})", ch + 1),
            Self::Note { ch, note } => format!("Note {} (ch {})", note_name(*note), ch + 1),
            Self::Key(k) => format!("Key {k}"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mapping {
    pub source: MapSource,
    pub target: MapTarget,
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Macro {
    pub name: String,
    pub value: f32,
    /// Mapped parameters with their range (normalized 0..1).
    pub targets: Vec<(MapTarget, f32, f32)>,
}

pub fn default_macros() -> Vec<Macro> {
    (1..=8).map(|i| Macro { name: format!("Macro {i}"), value: 0.0, targets: vec![] }).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArrClip {
    pub id: u64,
    /// Start in steps (16ths) on the timeline.
    pub start: u32,
    /// Length on the timeline in steps; the clip loops if this exceeds `clip.len`.
    pub len: u32,
    pub clip: Clip,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub instrument: Instrument,
    pub channel: Channel,
    pub clips: Vec<Option<Clip>>,
    #[serde(default)]
    pub arrangement: Vec<ArrClip>,
    #[serde(default)]
    pub automation: Vec<Automation>,
    #[serde(default)]
    pub midi_fx: Vec<MidiFx>,
    #[serde(default = "default_macros")]
    pub macros: Vec<Macro>,
    /// Audio tracks: armed for recording from the audio input.
    #[serde(default)]
    pub arm: bool,
    /// Group track this track belongs to.
    #[serde(default)]
    pub group: Option<u64>,
    /// Group tracks: members hidden in the Launcher.
    #[serde(default)]
    pub folded: bool,
    /// Where the track's output goes (groups take precedence).
    #[serde(default)]
    pub output: Output,
    #[serde(default)]
    pub modulators: Vec<Modulator>,
    /// Clip notes are bent to the nearest tone of the chord track's current chord.
    #[serde(default)]
    pub follow_chords: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Output {
    #[default]
    Master,
    /// Into another track's input (audio and group tracks process it with their devices).
    Track(u64),
    Return(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ModShape {
    Sine,
    Triangle,
    Saw,
    Square,
    Random,
    /// Attack/decay envelope retriggered by every note of the track.
    Envelope,
}

impl ModShape {
    pub const ALL: [ModShape; 6] = [Self::Sine, Self::Triangle, Self::Saw, Self::Square, Self::Random, Self::Envelope];
    pub fn name(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::Saw => "Saw",
            Self::Square => "Square",
            Self::Random => "Random",
            Self::Envelope => "Envelope",
        }
    }
}

/// An LFO or envelope that moves one parameter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Modulator {
    pub id: u64,
    pub shape: ModShape,
    /// Division index (see `params::DIVISIONS`) when synced, otherwise Hz.
    pub rate: f32,
    pub synced: bool,
    /// -1..1, scaled around the parameter's current value (normalized).
    pub depth: f32,
    /// Envelope attack / decay in seconds.
    pub attack: f32,
    pub decay: f32,
    pub target: Option<AutoTarget>,
    pub on: bool,
}

impl Modulator {
    pub fn new(id: u64) -> Self {
        Self { id, shape: ModShape::Sine, rate: 10.0, synced: true, depth: 0.3, attack: 0.005, decay: 0.3, target: None, on: true }
    }
}

/// Named locator on the timeline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub id: u64,
    pub name: String,
    pub pos: u32,
}

/// A song section on the arranger lane.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Section {
    pub id: u64,
    pub name: String,
    pub start: u32,
    pub len: u32,
    pub color: [u8; 3],
}

/// Chord qualities: name and intervals from the root.
pub const CHORD_QUALITIES: &[(&str, &[u8])] = &[
    ("", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("7", &[0, 4, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("m7", &[0, 3, 7, 10]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("add9", &[0, 4, 7, 14]),
    ("m9", &[0, 3, 7, 10, 14]),
    ("5", &[0, 7]),
];

/// A chord on the chord track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChordEvent {
    pub id: u64,
    pub start: u32,
    pub len: u32,
    /// Pitch class 0-11 (C = 0).
    pub root: u8,
    /// Index into `CHORD_QUALITIES`.
    pub quality: usize,
}

impl ChordEvent {
    pub fn name(&self) -> String {
        format!("{}{}", NOTE_NAMES[self.root as usize % 12], CHORD_QUALITIES[self.quality.min(CHORD_QUALITIES.len() - 1)].0)
    }
    pub fn intervals(&self) -> &'static [u8] {
        CHORD_QUALITIES[self.quality.min(CHORD_QUALITIES.len() - 1)].1
    }
    /// Pitch classes of the chord.
    pub fn classes(&self) -> Vec<u8> {
        self.intervals().iter().map(|i| (self.root + i) % 12).collect()
    }
    /// Chord notes voiced upwards from `base` (MIDI).
    pub fn voicing(&self, base: u8) -> Vec<u8> {
        self.intervals().iter().map(|i| base.saturating_add(*i)).filter(|p| *p <= 127).collect()
    }
}

/// Moves a pitch to the nearest chord tone (ties go down).
pub fn snap_to_chord(pitch: u8, classes: &[u8]) -> u8 {
    if classes.is_empty() {
        return pitch;
    }
    for d in 0..12i32 {
        for cand in [pitch as i32 - d, pitch as i32 + d] {
            if (0..128).contains(&cand) && classes.contains(&((cand % 12) as u8)) {
                return cand as u8;
            }
        }
    }
    pitch
}

/// Diatonic chord on scale degree `degree` (0-based) of a key/scale, stacking thirds.
pub fn diatonic_chord(key: u8, scale: usize, degree: usize, sevenths: bool) -> (u8, usize) {
    let steps = SCALES[scale.min(SCALES.len() - 1)].1;
    let n = steps.len();
    let pc = |k: usize| -> i32 { steps[k % n] as i32 + 12 * (k / n) as i32 };
    let root = pc(degree);
    let third = pc(degree + 2) - root;
    let fifth = pc(degree + 4) - root;
    let seventh = pc(degree + 6) - root;
    let q = match (third, fifth, sevenths, seventh) {
        (4, 7, false, _) => 0,
        (3, 7, false, _) => 1,
        (3, 6, _, _) => 7,
        (4, 8, _, _) => 8,
        (4, 7, true, 10) => 2,
        (4, 7, true, _) => 3,
        (3, 7, true, _) => 4,
        (2, 7, _, _) => 5,
        _ => 0,
    };
    (((key as i32 + root).rem_euclid(12)) as u8, q)
}

/// A pattern (scene) placed on the playlist.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaylistBlock {
    pub id: u64,
    pub scene: usize,
    pub start: u32,
    pub len: u32,
    pub lane: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReturnTrack {
    pub name: String,
    pub color: [u8; 3],
    pub channel: Channel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    /// Launch quantization in steps (0 = none).
    pub quant: u32,
    pub tracks: Vec<Track>,
    pub returns: Vec<ReturnTrack>,
    pub master: Channel,
    pub scenes: Vec<Scene>,
    pub loop_on: bool,
    pub loop_start: u32,
    pub loop_len: u32,
    pub next_id: u64,
    #[serde(default = "default_key")]
    pub key: u8,
    #[serde(default)]
    pub scale: usize,
    #[serde(default)]
    pub mappings: Vec<Mapping>,
    /// -1 = full A, 0 = center, 1 = full B.
    #[serde(default)]
    pub crossfader: f32,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub sections: Vec<Section>,
    /// Arranger chain: section ids played in this order (when `chain_on`).
    #[serde(default)]
    pub chain: Vec<u64>,
    #[serde(default)]
    pub chain_on: bool,
    #[serde(default)]
    pub playlist: Vec<PlaylistBlock>,
    #[serde(default)]
    pub chords: Vec<ChordEvent>,
}

fn default_key() -> u8 {
    9
}

pub const CLIP_COLORS: [[u8; 3]; 24] = [
    [0xff, 0x94, 0xa6], [0xff, 0xa5, 0x29], [0xcc, 0x99, 0x27], [0xf7, 0xf4, 0x7c],
    [0xbf, 0xfb, 0x00], [0x1a, 0xff, 0x2f], [0x25, 0xff, 0xa8], [0x5c, 0xff, 0xe8],
    [0x8b, 0xc5, 0xff], [0x54, 0x80, 0xe4], [0x92, 0xa7, 0xff], [0xd8, 0x6c, 0xe4],
    [0xe5, 0x53, 0xa0], [0xff, 0xff, 0xff], [0xff, 0x36, 0x36], [0xf6, 0x6c, 0x03],
    [0x99, 0x72, 0x4b], [0xff, 0xf0, 0x34], [0x87, 0xff, 0x67], [0x3d, 0xc3, 0x00],
    [0x00, 0xbf, 0xaf], [0x19, 0xe9, 0xff], [0x10, 0xa4, 0xee], [0x7c, 0x4b, 0xff],
];

pub const SCALES: &[(&str, &[u8])] = &[
    ("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    ("Minor", &[0, 2, 3, 5, 7, 8, 10]),
    ("Major", &[0, 2, 4, 5, 7, 9, 11]),
    ("Dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("Phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("Harmonic Minor", &[0, 2, 3, 5, 7, 8, 11]),
    ("Minor Pentatonic", &[0, 3, 5, 7, 10]),
];

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn note_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p as i32 / 12 - 2)
}

impl Project {
    pub fn empty() -> Self {
        Self {
            name: "Untitled".into(),
            bpm: 130.0,
            swing: 0.0,
            quant: STEPS_PER_BAR,
            tracks: vec![],
            returns: vec![
                ReturnTrack {
                    name: "A Reverb".into(),
                    color: [0xd8, 0x6c, 0xe4],
                    channel: Channel {
                        devices: vec![Device::new(1, DeviceKind::Reverb).with(5, 1.0)],
                        ..Default::default()
                    },
                },
                ReturnTrack {
                    name: "B Delay".into(),
                    color: [0x8b, 0xc5, 0xff],
                    channel: Channel {
                        devices: vec![Device::new(2, DeviceKind::Echo).with(5, 1.0)],
                        ..Default::default()
                    },
                },
            ],
            master: Channel {
                devices: vec![Device::new(3, DeviceKind::Compressor)
                    .with(0, -12.0)
                    .with(1, 2.0)
                    .with(2, 0.02)
                    .with(4, 1.0),
                    Device::new(4, DeviceKind::Limiter)],
                ..Default::default()
            },
            scenes: (1..=8).map(|i| Scene { name: i.to_string() }).collect(),
            loop_on: false,
            loop_start: 0,
            loop_len: 8 * STEPS_PER_BAR,
            next_id: 100,
            key: 9,
            scale: 1,
            mappings: vec![],
            crossfader: 0.0,
            markers: vec![],
            sections: vec![],
            chain: vec![],
            chain_on: false,
            playlist: vec![],
            chords: vec![],
        }
    }

    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    // --- Arranger sections -------------------------------------------------------------------

    pub const SECTION_NAMES: [&'static str; 6] = ["Intro", "Build", "Drop", "Break", "Drop 2", "Outro"];

    /// Adds a section; returns its id.
    pub fn add_section(&mut self, name: Option<&str>, start: u32, len: u32) -> u64 {
        let id = self.new_id();
        let n = self.sections.len();
        let name = name.map(str::to_string).unwrap_or_else(|| Self::SECTION_NAMES[n % 6].to_string());
        let color = CLIP_COLORS[(n * 5 + 2) % CLIP_COLORS.len()];
        self.sections.push(Section { id, name, start, len: len.max(1), color });
        self.sections.sort_by_key(|s| s.start);
        id
    }

    /// Shifts all timeline content (clips, automation, markers, sections) at or after `at`.
    fn shift_from(&mut self, at: u32, delta: i64) {
        let sh = |x: u32| (x as i64 + delta).max(0) as u32;
        for t in &mut self.tracks {
            for c in t.arrangement.iter_mut().filter(|c| c.start >= at) {
                c.start = sh(c.start);
            }
            for a in &mut t.automation {
                for pt in a.points.iter_mut().filter(|pt| pt.0 >= at as f32) {
                    pt.0 = (pt.0 + delta as f32).max(0.0);
                }
            }
        }
        for m in self.markers.iter_mut().filter(|m| m.pos >= at) {
            m.pos = sh(m.pos);
        }
        for s in self.sections.iter_mut().filter(|s| s.start >= at) {
            s.start = sh(s.start);
        }
    }

    /// Moves a section together with the clips and automation inside it.
    pub fn move_section(&mut self, id: u64, to: u32) {
        let Some(s) = self.sections.iter_mut().find(|s| s.id == id) else { return };
        let (from, len) = (s.start, s.len);
        s.start = to;
        let delta = to as f32 - from as f32;
        let inside = |x: f32| x >= from as f32 && x < (from + len) as f32;
        for t in &mut self.tracks {
            for c in t.arrangement.iter_mut().filter(|c| inside(c.start as f32)) {
                c.start = (c.start as f32 + delta).max(0.0) as u32;
            }
            for a in &mut t.automation {
                for pt in a.points.iter_mut().filter(|pt| inside(pt.0)) {
                    pt.0 = (pt.0 + delta).max(0.0);
                }
                a.points.sort_by(|x, y| x.0.total_cmp(&y.0));
            }
        }
        self.sections.sort_by_key(|s| s.start);
    }

    /// Inserts a copy of a section (with its content) right after it; later content moves right.
    pub fn duplicate_section(&mut self, id: u64) -> Option<u64> {
        let s = self.sections.iter().find(|s| s.id == id)?.clone();
        let end = s.start + s.len;
        self.shift_from(end, s.len as i64);
        for ti in 0..self.tracks.len() {
            let copies: Vec<ArrClip> = self.tracks[ti]
                .arrangement
                .iter()
                .filter(|c| c.start >= s.start && c.start < end)
                .cloned()
                .collect();
            for mut c in copies {
                c.id = self.new_id();
                c.len = c.len.min(end - c.start);
                c.start += s.len;
                self.tracks[ti].arrangement.push(c);
            }
            self.tracks[ti].arrangement.sort_by_key(|c| c.start);
            for a in &mut self.tracks[ti].automation {
                let extra: Vec<(f32, f32)> =
                    a.points.iter().filter(|p| p.0 >= s.start as f32 && p.0 < end as f32).map(|p| (p.0 + s.len as f32, p.1)).collect();
                a.points.extend(extra);
                a.points.sort_by(|x, y| x.0.total_cmp(&y.0));
            }
        }
        let nid = self.new_id();
        self.sections.push(Section { id: nid, name: s.name.clone(), start: end, len: s.len, color: s.color });
        self.sections.sort_by_key(|s| s.start);
        Some(nid)
    }

    /// Removes a section with its content and closes the gap.
    pub fn delete_section(&mut self, id: u64) {
        let Some(i) = self.sections.iter().position(|s| s.id == id) else { return };
        let s = self.sections.remove(i);
        let end = s.start + s.len;
        for t in &mut self.tracks {
            t.arrangement.retain(|c| c.start < s.start || c.start >= end);
            for c in t.arrangement.iter_mut().filter(|c| c.start < s.start && c.start + c.len > s.start) {
                c.len = s.start - c.start;
            }
            for a in &mut t.automation {
                a.points.retain(|p| p.0 < s.start as f32 || p.0 >= end as f32);
            }
        }
        self.markers.retain(|m| m.pos < s.start || m.pos >= end);
        self.shift_from(end, -(s.len as i64));
        self.chain.retain(|c| *c != id);
    }

    /// Chord on the chord track at a timeline position.
    pub fn chord_at(&self, pos: u32) -> Option<&ChordEvent> {
        self.chords.iter().find(|c| pos >= c.start && pos < c.start + c.len)
    }

    /// Content end of the playlist in steps.
    pub fn playlist_end(&self) -> u32 {
        self.playlist.iter().map(|b| b.start + b.len).max().unwrap_or(0)
    }

    pub fn track(&self, id: u64) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }
    pub fn track_mut(&mut self, id: u64) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }
    pub fn track_index(&self, id: u64) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    pub fn add_track(&mut self, name: &str, kind: InstrumentKind, color: [u8; 3]) -> u64 {
        let id = self.new_id();
        self.tracks.push(Track {
            id,
            name: name.into(),
            color,
            instrument: Instrument::new(kind),
            channel: Channel::default(),
            clips: vec![None; self.scenes.len()],
            arrangement: vec![],
            automation: vec![],
            midi_fx: vec![],
            macros: default_macros(),
            arm: false,
            group: None,
            folded: false,
            output: Output::Master,
            modulators: vec![],
            follow_chords: false,
        });
        id
    }

    pub fn add_scene(&mut self, at: usize) {
        self.scenes.insert(at, Scene { name: (self.scenes.len() + 1).to_string() });
        for t in &mut self.tracks {
            t.clips.insert(at, None);
        }
    }

    pub fn remove_scene(&mut self, at: usize) {
        if self.scenes.len() <= 1 {
            return;
        }
        self.scenes.remove(at);
        for t in &mut self.tracks {
            t.clips.remove(at);
        }
    }

    /// Whether a track is audible considering solo (a soloed group keeps its members audible).
    pub fn solo_ok(&self, t: &Track) -> bool {
        if !self.any_solo() || t.channel.solo {
            return true;
        }
        if let Some(g) = t.group.and_then(|g| self.track(g)) {
            if g.channel.solo {
                return true;
            }
        }
        // A group stays audible if one of its members is soloed.
        t.instrument.kind == InstrumentKind::Group && self.tracks.iter().any(|m| m.group == Some(t.id) && m.channel.solo)
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.channel.solo) || self.returns.iter().any(|r| r.channel.solo)
    }

    /// End of the arrangement in steps.
    pub fn arrangement_end(&self) -> u32 {
        self.tracks
            .iter()
            .flat_map(|t| t.arrangement.iter().map(|c| c.start + c.len))
            .max()
            .unwrap_or(0)
    }

    pub fn device_mut(&mut self, id: u64) -> Option<&mut Device> {
        // Pad effect chains of Beat Pads instruments.
        let in_pad = self.tracks.iter().enumerate().find_map(|(ti, t)| t.instrument.pads.iter().position(|p| contains_device(&p.devices, id)).map(|pi| (ti, pi)));
        if let Some((ti, pi)) = in_pad {
            return find_device(&mut self.tracks[ti].instrument.pads[pi].devices, id);
        }
        let chans = self
            .tracks
            .iter_mut()
            .map(|t| &mut t.channel)
            .chain(self.returns.iter_mut().map(|r| &mut r.channel))
            .chain(std::iter::once(&mut self.master));
        for ch in chans {
            if let Some(d) = find_device(&mut ch.devices, id) {
                return Some(d);
            }
        }
        None
    }

    /// Repair a project after loading (parameter lists grown in newer versions, etc.).
    pub fn normalize(&mut self) {
        let n = self.scenes.len();
        let mut max_id = self.next_id;
        for t in &mut self.tracks {
            t.instrument.normalize();
            t.clips.resize(n, None);
            t.macros.resize_with(8, Macro::default);
            max_id = max_id.max(t.id);
            for c in &t.arrangement {
                max_id = max_id.max(c.id);
            }
            for m in &mut t.midi_fx {
                normalize_params(&mut m.params, m.kind.defs());
                max_id = max_id.max(m.id);
            }
        }
        let chans = self
            .tracks
            .iter_mut()
            .map(|t| &mut t.channel)
            .chain(self.returns.iter_mut().map(|r| &mut r.channel))
            .chain(std::iter::once(&mut self.master));
        for ch in chans {
            normalize_devices(&mut ch.devices, &mut max_id);
        }
        for t in &mut self.tracks {
            for m in &t.modulators {
                max_id = max_id.max(m.id);
            }
            for pad in &mut t.instrument.pads {
                normalize_devices(&mut pad.devices, &mut max_id);
            }
        }
        let ids = self.markers.iter().map(|m| m.id).chain(self.sections.iter().map(|s| s.id)).chain(self.playlist.iter().map(|b| b.id));
        max_id = ids.fold(max_id, u64::max);
        self.next_id = max_id + 1;
    }
}

pub fn color32(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// Finds a device by id, including devices nested in rack chains.
pub fn find_device(devs: &mut [Device], id: u64) -> Option<&mut Device> {
    for d in devs {
        if d.id == id {
            return Some(d);
        }
        for c in &mut d.chains {
            if let Some(x) = find_device(&mut c.devices, id) {
                return Some(x);
            }
        }
    }
    None
}

fn normalize_devices(devs: &mut [Device], max_id: &mut u64) {
    for d in devs {
        normalize_params(&mut d.params, d.kind.defs());
        *max_id = (*max_id).max(d.id);
        for c in &mut d.chains {
            normalize_devices(&mut c.devices, max_id);
        }
    }
}

fn contains_device(devs: &[Device], id: u64) -> bool {
    devs.iter().any(|d| d.id == id || d.chains.iter().any(|c| contains_device(&c.devices, id)))
}

// ---------------------------------------------------------------------------------------------
// Note rules: condition -> action transforms for clips.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleField {
    All,
    Pitch,
    Velocity,
    Length,
    /// Position within the bar (0-15).
    StepInBar,
    /// Position within the beat (0-3: 0 = on the beat, 2 = off-beat 8th).
    StepInBeat,
    /// Index of the note in time order.
    Index,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleOp {
    Equal,
    Less,
    Greater,
    Between,
    /// value % a == b
    EveryNth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleAction {
    Transpose,
    SetVelocity,
    AddVelocity,
    ScaleVelocity,
    RandomVelocity,
    SetLength,
    ScaleLength,
    SetChance,
    Delete,
    AddOctave,
}

impl RuleField {
    pub const ALL: [RuleField; 7] = [Self::All, Self::Pitch, Self::Velocity, Self::Length, Self::StepInBar, Self::StepInBeat, Self::Index];
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "All notes",
            Self::Pitch => "Pitch",
            Self::Velocity => "Velocity",
            Self::Length => "Length",
            Self::StepInBar => "Step in bar",
            Self::StepInBeat => "Step in beat",
            Self::Index => "Note number",
        }
    }
}

impl RuleOp {
    pub const ALL: [RuleOp; 5] = [Self::Equal, Self::Less, Self::Greater, Self::Between, Self::EveryNth];
    pub fn name(self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::Less => "<",
            Self::Greater => ">",
            Self::Between => "between",
            Self::EveryNth => "every",
        }
    }
}

impl RuleAction {
    pub const ALL: [RuleAction; 10] = [
        Self::Transpose,
        Self::SetVelocity,
        Self::AddVelocity,
        Self::ScaleVelocity,
        Self::RandomVelocity,
        Self::SetLength,
        Self::ScaleLength,
        Self::SetChance,
        Self::Delete,
        Self::AddOctave,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Transpose => "Transpose (st)",
            Self::SetVelocity => "Set velocity",
            Self::AddVelocity => "Add velocity",
            Self::ScaleVelocity => "Scale velocity (%)",
            Self::RandomVelocity => "Randomize velocity (+/-)",
            Self::SetLength => "Set length (steps)",
            Self::ScaleLength => "Scale length (%)",
            Self::SetChance => "Set chance (%)",
            Self::Delete => "Delete",
            Self::AddOctave => "Add octave (st)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteRule {
    pub field: RuleField,
    pub op: RuleOp,
    pub a: i32,
    pub b: i32,
    pub action: RuleAction,
    pub value: i32,
}

impl NoteRule {
    pub fn new(field: RuleField, op: RuleOp, a: i32, b: i32, action: RuleAction, value: i32) -> Self {
        Self { field, op, a, b, action, value }
    }
}

/// Ready-made rule sets: name and rules.
pub fn rule_presets() -> Vec<(&'static str, Vec<NoteRule>)> {
    use RuleAction as A;
    use RuleField as F;
    use RuleOp as O;
    vec![
        ("Humanize velocity", vec![NoteRule::new(F::All, O::Equal, 0, 0, A::RandomVelocity, 12)]),
        ("Accent the beats", vec![NoteRule::new(F::StepInBeat, O::Equal, 0, 0, A::AddVelocity, 20), NoteRule::new(F::StepInBeat, O::Greater, 0, 0, A::AddVelocity, -12)]),
        ("Thin out off-beats", vec![NoteRule::new(F::StepInBeat, O::Greater, 0, 0, A::SetChance, 60)]),
        ("Remove ghost notes", vec![NoteRule::new(F::Velocity, O::Less, 40, 0, A::Delete, 0)]),
        ("Octave up above C4", vec![NoteRule::new(F::Pitch, O::Greater, 71, 0, A::Transpose, 12)]),
        ("Staccato", vec![NoteRule::new(F::All, O::Equal, 0, 0, A::ScaleLength, 50)]),
        ("Double low notes an octave down", vec![NoteRule::new(F::Pitch, O::Less, 48, 0, A::AddOctave, -12)]),
        ("Every 4th note louder", vec![NoteRule::new(F::Index, O::EveryNth, 4, 0, A::AddVelocity, 25)]),
    ]
}

/// Applies rules in order to a clip's notes; returns how many notes were touched.
pub fn apply_rules(notes: &mut Vec<Note>, rules: &[NoteRule], seed: u32) -> usize {
    let mut rng = seed | 1;
    let mut touched = 0;
    for rule in rules {
        notes.sort_by_key(|n| (n.step, n.pitch));
        let mut out = Vec::with_capacity(notes.len());
        for (idx, n) in notes.iter().enumerate() {
            let v = match rule.field {
                RuleField::All => 0,
                RuleField::Pitch => n.pitch as i32,
                RuleField::Velocity => n.vel as i32,
                RuleField::Length => n.len as i32,
                RuleField::StepInBar => (n.step % 16) as i32,
                RuleField::StepInBeat => (n.step % 4) as i32,
                RuleField::Index => idx as i32,
            };
            let hit = rule.field == RuleField::All
                || match rule.op {
                    RuleOp::Equal => v == rule.a,
                    RuleOp::Less => v < rule.a,
                    RuleOp::Greater => v > rule.a,
                    RuleOp::Between => v >= rule.a.min(rule.b) && v <= rule.a.max(rule.b),
                    RuleOp::EveryNth => rule.a > 0 && v.rem_euclid(rule.a) == rule.b.rem_euclid(rule.a.max(1)),
                };
            if !hit {
                out.push(*n);
                continue;
            }
            touched += 1;
            let mut m = *n;
            let vel = |x: i32| x.clamp(1, 127) as u8;
            match rule.action {
                RuleAction::Transpose => m.pitch = (m.pitch as i32 + rule.value).clamp(0, 127) as u8,
                RuleAction::SetVelocity => m.vel = vel(rule.value),
                RuleAction::AddVelocity => m.vel = vel(m.vel as i32 + rule.value),
                RuleAction::ScaleVelocity => m.vel = vel(m.vel as i32 * rule.value / 100),
                RuleAction::RandomVelocity => {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let span = rule.value.abs() * 2 + 1;
                    m.vel = vel(m.vel as i32 + (rng % span as u32) as i32 - rule.value.abs());
                }
                RuleAction::SetLength => m.len = rule.value.max(1) as u32,
                RuleAction::ScaleLength => m.len = ((m.len as i32 * rule.value + 50) / 100).max(1) as u32,
                RuleAction::SetChance => m.skip = (100 - rule.value.clamp(0, 100)) as u8,
                RuleAction::Delete => continue,
                RuleAction::AddOctave => {
                    out.push(m);
                    m.pitch = (m.pitch as i32 + rule.value).clamp(0, 127) as u8;
                }
            }
            out.push(m);
        }
        *notes = out;
    }
    notes.sort_by_key(|n| (n.step, n.pitch));
    touched
}

/// Parses chord names like "Am", "F#m7", "Bbmaj7", "Dsus4", "G" into (root, quality index).
pub fn parse_chord(name: &str) -> Option<(u8, usize)> {
    let s = name.trim();
    let mut chars = s.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"].iter().position(|n| *n == letter.to_string())? as i32;
    let rest: String = chars.collect();
    let (shift, qual) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest.as_str())
    };
    let qual = match qual {
        "" | "maj" | "M" => "",
        "min" | "-" => "m",
        q => q,
    };
    let q = CHORD_QUALITIES.iter().position(|(n, _)| *n == qual)?;
    Some(((base + shift).rem_euclid(12) as u8, q))
}
