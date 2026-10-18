//! Pattern data of the internal sequencer and the acid pattern generator.

use musicton_dsp::Rng;
use serde::{Deserialize, Serialize};

/// One step of the B-303's internal sequencer (stored form).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqStep {
    pub pitch: u8,
    pub accent: bool,
    pub slide: bool,
    /// 0 = note, 1 = tie, 2 = rest
    pub time: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqPattern {
    pub steps: Vec<SeqStep>,
    pub len: u8,
}

/// The 8 internal patterns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqBank {
    pub patterns: Vec<SeqPattern>,
}

impl SeqBank {
    pub fn factory() -> Self {
        let minor = [0u8, 2, 3, 5, 7, 8, 10];
        let patterns = (0..8u32)
            .map(|i| SeqPattern {
                steps: random_pattern(16, 45, &minor, 0x5eed + i * 7919).into_iter().map(SeqStep::from).collect(),
                len: 16,
            })
            .collect();
        Self { patterns }
    }
}

impl Default for SeqBank {
    fn default() -> Self {
        Self::factory()
    }
}

impl From<Step> for SeqStep {
    fn from(s: Step) -> Self {
        SeqStep { pitch: s.pitch, accent: s.accent, slide: s.slide, time: s.time as u8 }
    }
}

impl From<SeqStep> for Step {
    fn from(s: SeqStep) -> Self {
        Step {
            pitch: s.pitch,
            accent: s.accent,
            slide: s.slide,
            time: match s.time {
                0 => StepTime::Note,
                1 => StepTime::Tie,
                _ => StepTime::Rest,
            },
        }
    }
}

/// One step of a 303 pattern, as shown in the pattern editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum StepTime {
    Note = 0,
    Tie = 1,
    Rest = 2,
}

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub pitch: u8,
    pub accent: bool,
    pub slide: bool,
    pub time: StepTime,
}

/// Classic-sounding random acid line in the given key/scale.
pub fn random_pattern(len: usize, root: u8, scale: &[u8], seed: u32) -> Vec<Step> {
    let mut rng = Rng::new(seed.max(1));
    let mut r = move || rng.next() * 0.5 + 0.5;
    (0..len)
        .map(|i| {
            let roll = r();
            let time = if i > 0 && roll < 0.12 { StepTime::Tie } else if roll < 0.28 { StepTime::Rest } else { StepTime::Note };
            let degree = if r() < 0.4 { 0 } else { scale[(r() * scale.len() as f32) as usize % scale.len()] };
            let octave = if r() < 0.22 { 12 } else if r() < 0.08 { -12 } else { 0 };
            Step {
                pitch: (root as i32 + degree as i32 + octave).clamp(24, 84) as u8,
                accent: r() < 0.28,
                slide: r() < 0.22,
                time,
            }
        })
        .collect()
}
