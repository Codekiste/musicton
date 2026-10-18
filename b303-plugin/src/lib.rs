//! B-303 as a VST3/CLAP plug-in: the same engine and editor as in Musicton.
#![allow(semicolon_in_expressions_from_non_local_macros)] // from nih-plug's VST3 export macro

use b303_core::{SeqBank, SeqPattern, B303, SPECS};
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Parameter fields are generated from the shared spec so IDs stay stable (`p00`..`p24`).
macro_rules! b303_params {
    ($($field:ident = $id:literal),* $(,)?) => {
        #[derive(Params)]
        pub struct B303Params {
            #[persist = "editor-state"]
            editor_state: Arc<EguiState>,
            /// The 8 internal patterns, saved with the host project.
            #[persist = "bank"]
            bank: Arc<RwLock<SeqBank>>,
            $(#[id = $id] pub $field: FloatParam,)*
        }

        impl B303Params {
            fn all(&self) -> [&FloatParam; SPECS.len()] {
                [$(&self.$field),*]
            }
        }

        impl Default for B303Params {
            fn default() -> Self {
                let mut i = 0usize;
                $(let $field = make_param(i); i += 1;)*
                let _ = i;
                Self {
                    editor_state: EguiState::from_size(b303_gui::W as u32 * 9 / 10, ((b303_gui::H + b303_gui::PANEL_H) * 0.9) as u32),
                    bank: Arc::new(RwLock::new(SeqBank::factory())),
                    $($field,)*
                }
            }
        }
    };
}

b303_params!(
    p00 = "p00", p01 = "p01", p02 = "p02", p03 = "p03", p04 = "p04", p05 = "p05", p06 = "p06", p07 = "p07",
    p08 = "p08", p09 = "p09", p10 = "p10", p11 = "p11", p12 = "p12", p13 = "p13", p14 = "p14", p15 = "p15",
    p16 = "p16", p17 = "p17", p18 = "p18", p19 = "p19", p20 = "p20", p21 = "p21", p22 = "p22", p23 = "p23",
    p24 = "p24",
);

fn make_param(i: usize) -> FloatParam {
    let sp = SPECS[i];
    let mut p = FloatParam::new(sp.name, sp.def, FloatRange::Linear { min: sp.min, max: sp.max })
        .with_value_to_string(Arc::new(move |v| SPECS[i].format(v)));
    if sp.step > 0.0 {
        p = p.with_step_size(sp.step);
    }
    p
}

/// State shared between the audio thread and the editor.
#[derive(Default)]
struct Shared {
    display: [AtomicF32; 3],
    level: AtomicF32,
    /// Notes from the editor's keyboard: (pitch, velocity, gate in seconds or None = held, off).
    notes: Mutex<Vec<(u8, u8, Option<f32>, bool)>>,
}

pub struct B303Plugin {
    params: Arc<B303Params>,
    shared: Arc<Shared>,
    engine: B303,
    sr: f32,
    values: Vec<f32>,
    applied: Vec<f32>,
    own_beat: f64,
}

impl Default for B303Plugin {
    fn default() -> Self {
        Self {
            params: Arc::new(B303Params::default()),
            shared: Arc::new(Shared::default()),
            engine: B303::new(44100.0),
            sr: 44100.0,
            values: vec![0.0; SPECS.len()],
            applied: vec![],
            own_beat: 0.0,
        }
    }
}

/// Connects the shared editor to the plug-in's parameters.
struct PluginHost<'a> {
    params: &'a B303Params,
    setter: &'a ParamSetter<'a>,
    shared: &'a Shared,
}

impl b303_gui::Host for PluginHost<'_> {
    fn param(&self, idx: usize) -> f32 {
        self.params.all()[idx].value()
    }
    fn set_param(&mut self, idx: usize, v: f32) {
        let p = self.params.all()[idx];
        self.setter.begin_set_parameter(p);
        self.setter.set_parameter(p, v);
        self.setter.end_set_parameter(p);
    }
    fn pattern(&self) -> SeqPattern {
        let idx = (self.param(b303_core::p::PATTERN).round() as usize).min(7);
        self.params.bank.read().patterns[idx].clone()
    }
    fn set_pattern(&mut self, pat: SeqPattern) {
        let idx = (self.param(b303_core::p::PATTERN).round() as usize).min(7);
        self.params.bank.write().patterns[idx] = pat;
    }
    fn display(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| self.shared.display[i].load(Ordering::Relaxed))
    }
    fn level(&self) -> f32 {
        self.shared.level.load(Ordering::Relaxed)
    }
    fn note_on(&mut self, pitch: u8, vel: u8) {
        self.shared.notes.lock().push((pitch, vel, None, false));
    }
    fn note_off(&mut self, pitch: u8) {
        self.shared.notes.lock().push((pitch, 0, None, true));
    }
    fn audition(&mut self, pitch: u8, vel: u8) {
        self.shared.notes.lock().push((pitch, vel, Some(0.18), false));
    }
    fn seed(&self) -> u32 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1)
    }
}

impl Plugin for B303Plugin {
    const NAME: &'static str = "B-303";
    const VENDOR: &'static str = "Musicton";
    const URL: &'static str = "https://github.com/Codekiste/musicton";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let shared = self.shared.clone();
        create_egui_editor(
            self.params.editor_state.clone(),
            b303_gui::EditorState::new(),
            |_, _| {},
            move |ctx, setter, state| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default().fill(egui::Color32::from_rgb(0xa8, 0xaa, 0xad)))
                    .show(ctx, |ui| {
                        let mut host = PluginHost { params: &params, setter, shared: &shared };
                        b303_gui::editor(ui, &mut host, state);
                    });
                // Keep the step LEDs and the meter moving.
                ctx.request_repaint();
            },
        )
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, buffer_config: &BufferConfig, _context: &mut impl InitContext<Self>) -> bool {
        self.sr = buffer_config.sample_rate;
        self.engine = B303::new(self.sr);
        self.applied.clear();
        true
    }

    fn reset(&mut self) {
        self.engine.all_off();
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let transport = context.transport();
        let bpm = transport.tempo.unwrap_or(120.0) as f32;
        let playing = transport.playing;
        let beat = transport.pos_beats().unwrap_or(self.own_beat);
        self.engine.set_tempo(bpm);
        self.engine.set_position(beat, playing);

        // Parameters and pattern bank.
        for (v, p) in self.values.iter_mut().zip(self.params.all()) {
            *v = p.value();
        }
        if self.values != self.applied {
            self.engine.set_params(&self.values);
            self.applied.clone_from(&self.values);
        }
        if let Some(bank) = self.params.bank.try_read() {
            self.engine.set_bank(Some(&bank));
        }
        // Notes from the editor keyboard.
        if let Some(mut q) = self.shared.notes.try_lock() {
            for (pitch, vel, gate, off) in q.drain(..) {
                if off {
                    self.engine.note_off(pitch);
                } else {
                    self.engine.note_on(pitch, vel, gate.map(|s| (s * self.sr) as u32), false);
                }
            }
        }

        let n = buffer.samples();
        let out = buffer.as_slice();
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        let (left, rest) = out.split_at_mut(1);
        let (l, r) = (&mut left[0][..], &mut rest[0][..]);
        let mut pos = 0;
        let mut next = context.next_event();
        while pos < n {
            // Render up to the next event (sample accurate notes).
            let end = match next {
                Some(ev) if (ev.timing() as usize) < n => (ev.timing() as usize).max(pos),
                _ => n,
            };
            if end > pos {
                self.engine.render(&mut l[pos..end], &mut r[pos..end]);
                pos = end;
            }
            match next {
                Some(ev) if (ev.timing() as usize) <= pos => {
                    match ev {
                        NoteEvent::NoteOn { note, velocity, .. } => self.engine.note_on(note, (velocity * 127.0).round().clamp(1.0, 127.0) as u8, None, false),
                        NoteEvent::NoteOff { note, .. } => self.engine.note_off(note),
                        _ => {}
                    }
                    next = context.next_event();
                }
                _ => {
                    if pos >= n {
                        break;
                    }
                }
            }
        }
        if !playing {
            self.own_beat = beat + n as f64 * bpm as f64 / 60.0 / self.sr as f64;
        }
        let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, s| m.max(s.abs()));
        let old = self.shared.level.load(Ordering::Relaxed);
        self.shared.level.store(peak.max(old * 0.9), Ordering::Relaxed);
        for (i, v) in self.engine.display().iter().enumerate() {
            self.shared.display[i].store(*v, Ordering::Relaxed);
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for B303Plugin {
    const CLAP_ID: &'static str = "media.ackermann.musicton.b303";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("acid bassline with internal sequencer");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Mono];
}

impl Vst3Plugin for B303Plugin {
    const VST3_CLASS_ID: [u8; 16] = *b"MusictonB303Bass";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth, Vst3SubCategory::Mono];
}

nih_export_clap!(B303Plugin);
nih_export_vst3!(B303Plugin);
