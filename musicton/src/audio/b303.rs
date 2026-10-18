//! B-303 inside Musicton: the shared engine from `b303-core` as an instrument, plus helpers
//! to convert between clip notes and 303 steps.

use super::instruments::InstrumentDsp;
pub use b303_core::{random_pattern, Step, StepTime};

pub struct B303(b303_core::B303);

impl B303 {
    pub fn new(sr: f32) -> Self {
        Self(b303_core::B303::new(sr))
    }
}

impl InstrumentDsp for B303 {
    fn set_params(&mut self, p: &[f32], _: &[Option<String>]) {
        self.0.set_params(p);
    }
    fn set_extra(&mut self, inst: &crate::model::Instrument) {
        self.0.set_bank(inst.bank.as_ref());
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.0.set_tempo(bpm);
    }
    fn set_position(&mut self, beat: f64, playing: bool) {
        self.0.set_position(beat, playing);
    }
    fn display(&self) -> Option<&[f32]> {
        Some(self.0.display())
    }
    fn note_on(&mut self, pitch: u8, vel: u8, gate: Option<u32>, slide: bool) {
        self.0.note_on(pitch, vel, gate, slide);
    }
    fn note_off(&mut self, pitch: u8) {
        self.0.note_off(pitch);
    }
    fn all_off(&mut self) {
        self.0.all_off();
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.0.render(l, r);
    }
}

/// Reads clip notes into 303 steps (one per 16th).
pub fn notes_to_steps(notes: &[crate::model::Note], len: u32, default_pitch: u8) -> Vec<Step> {
    let mut steps = vec![Step { pitch: default_pitch, accent: false, slide: false, time: StepTime::Rest }; len as usize];
    let mut sorted: Vec<&crate::model::Note> = notes.iter().collect();
    sorted.sort_by_key(|n| n.step);
    for n in sorted {
        let s = n.step as usize;
        if s >= steps.len() {
            continue;
        }
        steps[s] = Step { pitch: n.pitch, accent: n.vel >= 110, slide: n.slide, time: StepTime::Note };
        for t in 1..n.len as usize {
            if s + t < steps.len() && steps[s + t].time == StepTime::Rest {
                steps[s + t] = Step { pitch: n.pitch, accent: false, slide: false, time: StepTime::Tie };
            }
        }
    }
    steps
}

/// Writes 303 steps back into clip notes (ties extend the previous note).
pub fn steps_to_notes(steps: &[Step]) -> Vec<crate::model::Note> {
    let mut out: Vec<crate::model::Note> = vec![];
    for (i, st) in steps.iter().enumerate() {
        match st.time {
            StepTime::Note => out.push(crate::model::Note { step: i as u32, len: 1, pitch: st.pitch, vel: if st.accent { 120 } else { 90 }, slide: st.slide }),
            StepTime::Tie => {
                if let Some(last) = out.last_mut() {
                    if last.step + last.len == i as u32 {
                        last.len += 1;
                    }
                }
            }
            StepTime::Rest => {}
        }
    }
    out
}

