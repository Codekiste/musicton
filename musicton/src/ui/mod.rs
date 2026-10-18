//! Application shell: layout, selection, undo, keyboard, files, live playing and recording.

mod analysis_ui;
mod arrangement;
mod arranger;
mod browser;
mod clip_editor;
mod composition_ui;
mod devices;
mod export;
mod instruments_ui;
mod mapping;
mod pads_ui;
mod plugin_ui;
mod plugins_ui;
mod rack_ui;
mod samplers_ui;
mod remote;
mod remote_workflow;
mod score_ui;
mod session;
mod synth_ui;
pub mod theme;
mod transport;
mod versions_ui;
mod video_ui;
mod widgets;

use crate::audio::engine::{Cmd, Status};
use crate::audio::{samples, Audio, ExportJob};
use crate::midi::{Midi, MidiEvent};
use crate::model::*;
use crate::params::DRUM_LANES;
use crate::presets;
use crossbeam_channel::{Receiver, Sender};
use egui::{Context, Key, Modifiers};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;
use theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Session,
    Arrangement,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Detail {
    Clip,
    Devices,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sel {
    Track(u64),
    Return(usize),
    Master,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipRef {
    Session { track: u64, scene: usize },
    Arr { track: u64, id: u64 },
}

impl ClipRef {
    pub fn track(&self) -> u64 {
        match *self {
            ClipRef::Session { track, .. } | ClipRef::Arr { track, .. } => track,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Payload {
    Instrument(&'static str),
    Device(DeviceKind),
    MidiFx(MidiFxKind),
    Pattern(&'static str),
    SessionClip { track: u64, scene: usize },
    Sample(PathBuf),
    /// Index into the scanned plug-in list.
    Plugin(usize),
}

impl Payload {
    fn label(&self) -> String {
        match self {
            Payload::Instrument(n) | Payload::Pattern(n) => n.to_string(),
            Payload::Device(k) => k.name().to_string(),
            Payload::MidiFx(k) => k.name().to_string(),
            Payload::SessionClip { .. } => "Clip".into(),
            Payload::Sample(p) => p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
            Payload::Plugin(_) => "Plug-in".into(),
        }
    }
}

struct HeldNote {
    key: Option<Key>,
    track: u64,
    pitch: u8,
    vel: u8,
    start_tick: f64,
    start_arr: f64,
}

#[derive(Clone)]
struct Captured {
    track: u64,
    pitch: u8,
    vel: u8,
    start: f64,
    len: f64,
}

pub const RETURN_METER: u64 = u64::MAX - 10;
pub const MASTER_METER: u64 = u64::MAX;

pub struct App {
    project: Project,
    committed: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    changed: bool,
    publish: bool,
    uncommitted: bool,
    audio: Audio,
    status: Status,
    meters: HashMap<u64, [f32; 2]>,
    midi: Midi,
    midi_tx: Sender<MidiEvent>,
    midi_rx: Receiver<MidiEvent>,
    midi_ports: Vec<String>,
    view: View,
    detail: Detail,
    show_browser: bool,
    show_info: bool,
    show_detail: bool,
    sel: Sel,
    sel_clip: Option<ClipRef>,
    sel_scene: usize,
    sel_device: Option<u64>,
    rack: rack_ui::RackUiState,
    chord_pads: composition_ui::ChordPadState,
    versions: versions_ui::VersionsState,
    video: video_ui::VideoState,
    plugins_state: plugins_ui::PluginsState,
    drag: Option<Payload>,
    info: Option<(String, String)>,
    kb_octave: i32,
    kb_vel: u8,
    held: Vec<HeldNote>,
    rec: bool,
    metronome: bool,
    capture: VecDeque<Captured>,
    file: Option<PathBuf>,
    toast: Option<(String, f64)>,
    start_time: Instant,
    tap_times: Vec<f64>,
    renaming: Option<(RenameTarget, String)>,
    arr: arrangement::ArrState,
    roll: clip_editor::RollState,
    browser: browser::BrowserState,
    export: Option<export::ExportDialog>,
    export_job: Option<ExportJob>,
    drum_lane: usize,
    auto_open: HashSet<u64>,
    /// Debug: MUSICTON_SHOT=<file.ppm> renders a few frames, saves a screenshot and quits.
    shot: Option<String>,
    frames: u64,
    peak_cache: HashMap<(String, usize), std::sync::Arc<Vec<(f32, f32)>>>,
    /// Background jobs (library rendering, downloads): label and result slot.
    jobs: Vec<(String, std::sync::Arc<parking_lot::Mutex<Option<Result<String, String>>>>)>,
    learning: Option<(MapTarget, mapping::LearnKind)>,
    show_mappings: bool,
    arr_rec: bool,
    rec_segments: HashMap<u64, (u64, usize, u32)>,
    input: Option<crate::audio::input::Input>,
    /// Audio recording in progress: armed tracks and the start positions (session tick, arrangement step).
    audio_rec: Option<(Vec<u64>, u64, u32)>,
    remote_rx: Option<Receiver<crate::remote::Request>>,
    pending_shots: Vec<(Sender<Result<serde_json::Value, String>>, u32)>,
    input_queue: VecDeque<Vec<egui::Event>>,
    input_reply: Option<Sender<Result<serde_json::Value, String>>>,
    remote_note_offs: Vec<(Instant, u64, u8)>,
    nexa_presets: HashMap<u64, usize>,
    b303_piano_roll: bool,
    plugins: plugin_ui::PluginState,
    plugin_overlay: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenameTarget {
    Track(u64),
    Scene(usize),
    Clip(ClipRef),
}

fn autosave_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("musicton").join("autosave.mton")
}

fn load_project_file(path: &std::path::Path) -> Result<Project, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut p: Project = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    p.normalize();
    Ok(p)
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        theme::apply(&cc.egui_ctx);
        // First start: render the bundled sample library before building the demo (well under a second).
        let lib_root = browser::user_sample_dir();
        if !crate::library::core_library_exists(&lib_root) {
            if let Err(e) = crate::library::build_core_library(&lib_root) {
                eprintln!("could not render sample library: {e}");
            }
        }
        let project = load_project_file(&autosave_path()).unwrap_or_else(|_| demo_with_audio());
        load_project_samples(&project);
        let audio = Audio::start(&project);
        let (midi_tx, midi_rx) = crossbeam_channel::bounded(1024);
        let sel = project.tracks.first().map(|t| Sel::Track(t.id)).unwrap_or(Sel::Master);
        let mut app = Self {
            committed: project.clone(),
            project,
            undo: vec![],
            redo: vec![],
            changed: false,
            publish: false,
            uncommitted: false,
            status: Status::default(),
            meters: HashMap::new(),
            midi: Midi::new(),
            midi_tx,
            midi_rx,
            midi_ports: Midi::list_ports(),
            view: View::Session,
            detail: Detail::Devices,
            show_browser: true,
            show_info: true,
            show_detail: true,
            sel,
            sel_clip: None,
            sel_scene: 0,
            sel_device: None,
            rack: Default::default(),
            chord_pads: Default::default(),
            versions: Default::default(),
            video: Default::default(),
            plugins_state: Default::default(),
            drag: None,
            info: None,
            kb_octave: 4,
            kb_vel: 100,
            held: vec![],
            rec: false,
            metronome: false,
            capture: VecDeque::new(),
            file: None,
            toast: None,
            start_time: Instant::now(),
            tap_times: vec![],
            renaming: None,
            arr: Default::default(),
            roll: Default::default(),
            browser: browser::BrowserState::new(),
            export: None,
            export_job: None,
            drum_lane: 0,
            auto_open: HashSet::new(),
            shot: std::env::var("MUSICTON_SHOT").ok(),
            frames: 0,
            peak_cache: HashMap::new(),
            jobs: vec![],
            learning: None,
            show_mappings: false,
            arr_rec: false,
            rec_segments: HashMap::new(),
            input: None,
            audio_rec: None,
            remote_rx: crate::remote::start(cc.egui_ctx.clone()),
            pending_shots: vec![],
            input_queue: VecDeque::new(),
            input_reply: None,
            remote_note_offs: vec![],
            nexa_presets: HashMap::new(),
            b303_piano_roll: false,
            plugins: Default::default(),
            plugin_overlay: None,
            audio,
        };
        app.update_blocked_notes();

        if std::env::var("MUSICTON_VIEW").is_ok_and(|v| v == "arrangement") {
            app.view = View::Arrangement;
        }
        if let Ok(v) = std::env::var("MUSICTON_CLIP") {
            // "<track index>,<scene>"
            let mut it = v.split(',').filter_map(|x| x.parse::<usize>().ok());
            if let (Some(ti), Some(sc)) = (it.next(), it.next()) {
                if let Some(id) = app.project.tracks.get(ti).map(|t| t.id) {
                    app.select_clip(ClipRef::Session { track: id, scene: sc });
                }
            }
        }
        if let Ok(list) = std::env::var("MUSICTON_ADD_FX") {
            // Debug helper: comma-separated device names appended to the first track.
            let target = std::env::var("MUSICTON_TRACK").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            if let Some(id) = app.project.tracks.get(target).map(|t| t.id) {
                for name in list.split(',') {
                    if let Some(kind) = DeviceKind::all().find(|k| k.name() == name) {
                        app.add_device(Sel::Track(id), kind, None);
                    } else if let Some(kind) = MidiFxKind::ALL.iter().find(|k| k.name() == name) {
                        app.add_midi_fx(id, *kind, None);
                    }
                }
                app.publish = true;
            }
        }
        if let Ok(v) = std::env::var("MUSICTON_TRACK") {
            if let Some(id) = v.parse::<usize>().ok().and_then(|i| app.project.tracks.get(i)).map(|t| t.id) {
                app.select_track(id);
                app.detail = Detail::Devices;
            }
        }
        if std::env::var("MUSICTON_AUTO").is_ok() {
            if let Some(t) = app.project.tracks.get(2) {
                app.auto_open.insert(t.id);
            }
        }
        if let Some(e) = app.audio.error.clone() {
            app.toast(format!("Audio error: {e}"));
        }
        // Auto-connect the first hardware MIDI input if there is one.
        if let Some(port) = app.midi_ports.iter().find(|p| !p.contains("Through")).cloned() {
            let _ = app.midi.connect(&port, app.audio.tx.clone(), app.midi_tx.clone(), cc.egui_ctx.clone());
        }
        app
    }

    // -----------------------------------------------------------------------------------------
    // State helpers

    fn now(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.now() + 3.5));
    }

    /// Marks the project as edited: it is pushed to the engine and committed to undo history.
    fn touch(&mut self) {
        self.changed = true;
    }

    fn hint(&mut self, resp: &egui::Response, title: &str, body: &str) {
        if resp.hovered() {
            self.info = Some((title.to_string(), body.to_string()));
        }
    }

    fn send(&self, cmd: Cmd) {
        self.audio.send(cmd);
    }

    fn selected_track(&self) -> Option<u64> {
        match self.sel {
            Sel::Track(id) if self.project.track(id).is_some() => Some(id),
            _ => None,
        }
    }

    fn track_status(&self, id: u64) -> Option<&crate::audio::engine::TrackStatus> {
        self.status.tracks.iter().find(|t| t.id == id)
    }

    fn select_track(&mut self, id: u64) {
        self.sel = Sel::Track(id);
    }

    fn clip(&self, r: ClipRef) -> Option<&Clip> {
        match r {
            ClipRef::Session { track, scene } => self.project.track(track)?.clips.get(scene)?.as_ref(),
            ClipRef::Arr { track, id } => self.project.track(track)?.arrangement.iter().find(|c| c.id == id).map(|c| &c.clip),
        }
    }

    fn clip_mut(&mut self, r: ClipRef) -> Option<&mut Clip> {
        match r {
            ClipRef::Session { track, scene } => self.project.track_mut(track)?.clips.get_mut(scene)?.as_mut(),
            ClipRef::Arr { track, id } => {
                self.project.track_mut(track)?.arrangement.iter_mut().find(|c| c.id == id).map(|c| &mut c.clip)
            }
        }
    }

    fn channel_mut(&mut self, sel: Sel) -> Option<&mut Channel> {
        match sel {
            Sel::Track(id) => self.project.track_mut(id).map(|t| &mut t.channel),
            Sel::Return(i) => self.project.returns.get_mut(i).map(|r| &mut r.channel),
            Sel::Master => Some(&mut self.project.master),
        }
    }

    fn select_clip(&mut self, r: ClipRef) {
        self.sel_clip = Some(r);
        self.select_track(r.track());
        if let ClipRef::Session { scene, .. } = r {
            self.sel_scene = scene;
        }
        self.detail = Detail::Clip;
        self.roll.scroll_to_notes = true;
    }

    fn create_clip(&mut self, track: u64, scene: usize, clip: Clip) {
        if let Some(t) = self.project.track_mut(track) {
            if scene < t.clips.len() {
                t.clips[scene] = Some(clip);
                self.touch();
                self.select_clip(ClipRef::Session { track, scene });
            }
        }
    }

    fn add_audio_track(&mut self, at: Option<usize>) -> u64 {
        let color = CLIP_COLORS[(self.project.tracks.len() * 5 + 3) % CLIP_COLORS.len()];
        let n = self.project.tracks.iter().filter(|t| t.instrument.kind == InstrumentKind::Audio).count();
        let name = if n == 0 { "Audio".to_string() } else { format!("Audio {}", n + 1) };
        let id = self.project.add_track(&name, InstrumentKind::Audio, color);
        if let Some(at) = at {
            let t = self.project.tracks.pop().unwrap();
            self.project.tracks.insert(at.min(self.project.tracks.len()), t);
        }
        self.select_track(id);
        self.touch();
        id
    }

    /// Builds an audio clip for a WAV file (loading it into the sample bank).
    fn audio_clip(&mut self, path: &std::path::Path, color: [u8; 3]) -> Option<Clip> {
        let p = path.to_string_lossy().to_string();
        match samples::load(&p) {
            Ok(s) => Some(Clip::from_audio(&p, s.left.len(), s.sample_rate, self.project.bpm, color)),
            Err(e) => {
                self.toast(format!("Could not load {}: {e}", path.display()));
                None
            }
        }
    }

    /// Drops a sample onto a clip slot: audio tracks get an audio clip, Sampler loads it,
    /// drum tracks put it on the selected lane.
    fn drop_sample_on_slot(&mut self, track: u64, scene: usize, path: &std::path::Path) {
        let Some(t) = self.project.track(track) else { return };
        let (kind, color) = (t.instrument.kind, t.color);
        match kind {
            InstrumentKind::Audio => {
                if let Some(c) = self.audio_clip(path, color) {
                    self.create_clip(track, scene, c);
                }
            }
            InstrumentKind::Sampler | InstrumentKind::Slicer | InstrumentKind::GrainCloud => {
                let p = path.to_string_lossy().to_string();
                if samples::load(&p).is_ok() {
                    if let Some(t) = self.project.track_mut(track) {
                        t.instrument.samples[0] = Some(p);
                        if kind == InstrumentKind::GrainCloud {
                            t.instrument.params[crate::params::grain::SOURCE] = 0.0;
                        }
                    }
                    self.touch();
                    self.toast(format!("Sample loaded into {}", kind.name()));
                }
            }
            InstrumentKind::MultiSampler => {
                let p = path.to_string_lossy().to_string();
                if samples::load(&p).is_ok() {
                    if let Some(t) = self.project.track_mut(track) {
                        let mut files: Vec<String> = t.instrument.zones.iter().map(|z| z.path.clone()).collect();
                        files.push(p);
                        t.instrument.zones = zones_from_files(&files);
                    }
                    self.touch();
                    self.toast("Sample added as a zone");
                }
            }
            InstrumentKind::Drums | InstrumentKind::BeatPads => self.assign_sample(Some(track), self.drum_lane, path),
            _ => self.toast("Drop samples on an audio track, a sampler instrument or a drum lane"),
        }
    }

    fn new_empty_clip(&mut self, track: u64, scene: usize) {
        let Some(t) = self.project.track(track) else { return };
        if t.instrument.kind == InstrumentKind::Audio {
            self.toast("Audio tracks play samples - drag a WAV from the Samples browser into the slot");
            return;
        }
        if t.instrument.kind == InstrumentKind::Group {
            return;
        }
        let name = format!("{} {}", t.name, scene + 1);
        let clip = Clip::new(&name, t.color, if t.instrument.kind.is_drum_like() { 1 } else { 2 });
        self.create_clip(track, scene, clip);
    }

    fn add_track_from_preset(&mut self, preset: &str, at: Option<usize>) -> u64 {
        let pr = presets::preset(preset);
        let color = CLIP_COLORS[(self.project.tracks.len() * 5 + 1) % CLIP_COLORS.len()];
        let name = match pr.kind {
            InstrumentKind::Drums => "Drums",
            InstrumentKind::Bass => "Bass",
            InstrumentKind::Synth => "Synth",
            k => k.name(),
        };
        let n = self.project.tracks.iter().filter(|t| t.name.starts_with(name)).count();
        let name = if n == 0 { name.to_string() } else { format!("{name} {}", n + 1) };
        let id = self.project.add_track(&name, pr.kind, color);
        self.project.track_mut(id).unwrap().instrument = pr.build();
        if let Some(at) = at {
            let t = self.project.tracks.pop().unwrap();
            let at = at.min(self.project.tracks.len());
            self.project.tracks.insert(at, t);
        }
        self.select_track(id);
        self.detail = Detail::Devices;
        self.touch();
        if matches!(pr.kind, InstrumentKind::B303 | InstrumentKind::Nexa) {
            // Auto-open plug-in windows.
            self.plugins.open.insert(id, true);
        }
        id
    }

    fn add_device(&mut self, sel: Sel, kind: DeviceKind, at: Option<usize>) {
        let id = self.project.new_id();
        if let Some(ch) = self.channel_mut(sel) {
            let d = Device::new(id, kind);
            match at {
                Some(i) if i <= ch.devices.len() => ch.devices.insert(i, d),
                _ => ch.devices.push(d),
            }
            self.sel = sel;
            self.sel_device = Some(id);
            self.detail = Detail::Devices;
            self.touch();
        }
    }

    fn run_job(&mut self, label: &str, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
        let slot = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let s2 = slot.clone();
        std::thread::spawn(move || {
            let r = f();
            *s2.lock() = Some(r);
        });
        self.jobs.push((label.to_string(), slot));
        self.toast(label.to_string());
    }

    fn poll_jobs(&mut self) {
        let mut done = vec![];
        self.jobs.retain(|(_, slot)| match slot.lock().take() {
            Some(r) => {
                done.push(r);
                false
            }
            None => true,
        });
        for r in done {
            match r {
                Ok(msg) => self.toast(msg),
                Err(e) => self.toast(format!("Failed: {e}")),
            }
            self.browser.rescan();
        }
    }

    pub(super) fn toggle_arm(&mut self, track: u64) {
        let Some(t) = self.project.track_mut(track) else { return };
        t.arm = !t.arm;
        let armed = t.arm;
        self.touch();
        if armed && self.input.is_none() {
            let inp = crate::audio::input::Input::open();
            match &inp.error {
                Some(e) => self.toast(format!("Audio input: {e}")),
                None => self.toast(format!("Input: {} @ {} Hz - press Record and Play to record", inp.device_name, inp.sample_rate)),
            }
            self.input = Some(inp);
        }
    }

    pub(super) fn input_level(&self) -> f32 {
        self.input.as_ref().map(|i| i.state.lock().level).unwrap_or(0.0)
    }

    /// Starts/stops audio recording on armed audio tracks with the transport and Record button.
    fn update_audio_record(&mut self) {
        let armed: Vec<u64> = self.project.tracks.iter().filter(|t| t.arm && t.instrument.kind == InstrumentKind::Audio).map(|t| t.id).collect();
        let should = self.rec && self.status.playing && !armed.is_empty() && self.input.as_ref().is_some_and(|i| i.error.is_none());
        match (&self.audio_rec, should) {
            (None, true) => {
                if let Some(inp) = &self.input {
                    let mut st = inp.state.lock();
                    st.data.clear();
                    st.recording = true;
                }
                self.audio_rec = Some((armed, self.status.tick, self.status.arr_pos.max(0.0) as u32));
            }
            (Some(_), false) => {
                let (tracks, _, arr_start) = self.audio_rec.take().unwrap();
                let Some(inp) = &self.input else { return };
                let (data, sr) = {
                    let mut st = inp.state.lock();
                    st.recording = false;
                    (std::mem::take(&mut st.data), inp.sample_rate)
                };
                if data.len() < 2 * 1000 {
                    return;
                }
                let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("musicton").join("recordings");
                match crate::audio::input::write_recording(&dir, &data, sr) {
                    Ok(path) => {
                        let p = path.to_string_lossy().to_string();
                        let _ = samples::load(&p);
                        let frames = data.len() / 2;
                        let steps = (frames as f32 / sr as f32 * self.project.bpm / 60.0 * 4.0).ceil() as u32;
                        for track in tracks {
                            let color = self.project.track(track).map(|t| t.color).unwrap_or([200, 200, 200]);
                            let mut clip = Clip::from_audio(&p, frames, sr as f32, self.project.bpm, color);
                            if let Some(a) = clip.audio.as_mut() {
                                a.warp = WarpMode::Beats;
                                a.bpm = self.project.bpm;
                            }
                            clip.len = steps.div_ceil(STEPS_PER_BAR).max(1) * STEPS_PER_BAR;
                            clip.name = format!("Rec {}", self.project.track(track).map(|t| t.name.clone()).unwrap_or_default());
                            if self.arr_rec {
                                let id = self.project.new_id();
                                let len = clip.len;
                                if let Some(t) = self.project.track_mut(track) {
                                    t.arrangement.push(ArrClip { id, start: arr_start, len, clip });
                                }
                            } else if let Some(t) = self.project.track(track) {
                                let slot = (self.sel_scene..t.clips.len()).chain(0..self.sel_scene).find(|&s| t.clips[s].is_none());
                                match slot {
                                    Some(s) => self.create_clip(track, s, clip),
                                    None => self.toast("No empty slot for the recording"),
                                }
                            }
                        }
                        self.touch();
                        self.toast(format!("Recorded {:.1} s", frames as f32 / sr as f32));
                    }
                    Err(e) => self.toast(format!("Could not save recording: {e}")),
                }
            }
            _ => {}
        }
    }

    fn add_midi_fx(&mut self, track: u64, kind: MidiFxKind, at: Option<usize>) {
        let id = self.project.new_id();
        let Some(t) = self.project.track_mut(track) else { return };
        let m = MidiFx::new(id, kind);
        match at {
            Some(i) if i <= t.midi_fx.len() => t.midi_fx.insert(i, m),
            _ => t.midi_fx.push(m),
        }
        self.sel = Sel::Track(track);
        self.detail = Detail::Devices;
        self.touch();
    }

    /// Tracks shown as Launcher columns (members of folded groups are hidden).
    pub(super) fn visible_tracks(&self) -> Vec<u64> {
        self.project
            .tracks
            .iter()
            .filter(|t| !t.group.and_then(|g| self.project.track(g)).is_some_and(|g| g.folded))
            .map(|t| t.id)
            .collect()
    }

    pub(super) fn group_track(&mut self, id: u64) {
        let Some(i) = self.project.track_index(id) else { return };
        if self.project.tracks[i].instrument.kind == InstrumentKind::Group || self.project.tracks[i].group.is_some() {
            return;
        }
        let color = self.project.tracks[i].color;
        let gid = self.project.add_track("Group", InstrumentKind::Group, color);
        let g = self.project.tracks.pop().unwrap();
        self.project.tracks.insert(i, g);
        if let Some(t) = self.project.track_mut(id) {
            t.group = Some(gid);
        }
        self.select_track(gid);
        self.touch();
        self.toast("Created a group. Right-click other tracks > Move into Group to add them.");
    }

    pub(super) fn move_into_group(&mut self, id: u64, gid: u64) {
        let Some(t) = self.project.track_mut(id) else { return };
        t.group = Some(gid);
        // Keep members right after their group (and its other members).
        let i = self.project.track_index(id).unwrap();
        let track = self.project.tracks.remove(i);
        let gi = self.project.track_index(gid).unwrap_or(0);
        let mut at = gi + 1;
        while at < self.project.tracks.len() && self.project.tracks[at].group == Some(gid) {
            at += 1;
        }
        self.project.tracks.insert(at, track);
        self.touch();
    }

    pub(super) fn ungroup(&mut self, gid: u64) {
        for t in &mut self.project.tracks {
            if t.group == Some(gid) {
                t.group = None;
            }
        }
        self.delete_track(gid);
    }

    fn delete_track(&mut self, id: u64) {
        let is_group = self.project.track(id).is_some_and(|t| t.instrument.kind == InstrumentKind::Group);
        if is_group {
            for t in &mut self.project.tracks {
                if t.group == Some(id) {
                    t.group = None;
                }
            }
        }
        if let Some(i) = self.project.track_index(id) {
            self.project.tracks.remove(i);
            if self.sel_clip.is_some_and(|c| c.track() == id) {
                self.sel_clip = None;
            }
            self.sel = self.project.tracks.get(i.saturating_sub(1)).map(|t| Sel::Track(t.id)).unwrap_or(Sel::Master);
            self.touch();
        }
    }

    fn duplicate_track(&mut self, id: u64) {
        let Some(i) = self.project.track_index(id) else { return };
        let mut t = self.project.tracks[i].clone();
        t.id = self.project.new_id();
        t.name = format!("{} Copy", t.name);
        let mut id_map = HashMap::new();
        for d in &mut t.channel.devices {
            let nid = self.project.new_id();
            id_map.insert(d.id, nid);
            d.id = nid;
        }
        for c in &mut t.arrangement {
            c.id = self.project.new_id();
        }
        for a in &mut t.automation {
            if let AutoTarget::Device { id, .. } = &mut a.target {
                if let Some(n) = id_map.get(id) {
                    *id = *n;
                }
            }
        }
        let nid = t.id;
        self.project.tracks.insert(i + 1, t);
        self.select_track(nid);
        self.touch();
    }

    fn delete_selected_clip(&mut self) {
        let Some(r) = self.sel_clip else { return };
        match r {
            ClipRef::Session { track, scene } => {
                if let Some(t) = self.project.track_mut(track) {
                    if let Some(c) = t.clips.get_mut(scene) {
                        *c = None;
                    }
                }
            }
            ClipRef::Arr { track, id } => {
                if let Some(t) = self.project.track_mut(track) {
                    t.arrangement.retain(|c| c.id != id);
                }
            }
        }
        self.sel_clip = None;
        self.touch();
    }

    fn duplicate_selected_clip(&mut self) {
        let Some(r) = self.sel_clip else { return };
        match r {
            ClipRef::Session { track, scene } => {
                let Some(clip) = self.clip(r).cloned() else { return };
                let n = self.project.scenes.len();
                let target = (scene + 1..n).find(|&s| self.project.track(track).is_some_and(|t| t.clips[s].is_none()));
                let target = match target {
                    Some(s) => s,
                    None => {
                        self.project.add_scene(n);
                        n
                    }
                };
                self.create_clip(track, target, clip);
            }
            ClipRef::Arr { track, id } => {
                let new_id = self.project.new_id();
                let Some(t) = self.project.track_mut(track) else { return };
                let Some(ac) = t.arrangement.iter().find(|c| c.id == id).cloned() else { return };
                let start = ac.start + ac.len;
                t.arrangement.retain(|c| !(c.start < start + ac.len && c.start + c.len > start));
                t.arrangement.push(ArrClip { id: new_id, start, ..ac });
                self.sel_clip = Some(ClipRef::Arr { track, id: new_id });
                self.touch();
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Undo / redo and engine sync

    fn end_frame(&mut self, ctx: &Context) {
        self.sync_nexa_presets();
        if self.changed {
            self.changed = false;
            self.publish = true;
            self.uncommitted = true;
        }
        if self.publish {
            self.publish = false;
            self.audio.publish(&self.project);
        }
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        if self.uncommitted && !pointer_down {
            self.uncommitted = false;
            let prev = std::mem::replace(&mut self.committed, self.project.clone());
            self.undo.push(prev);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, prev));
            self.committed = self.project.clone();
            self.nexa_presets.clear();
            self.publish = true;
            self.validate_selection();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, next));
            self.committed = self.project.clone();
            self.nexa_presets.clear();
            self.publish = true;
            self.validate_selection();
        }
    }

    fn validate_selection(&mut self) {
        if let Some(r) = self.sel_clip {
            if self.clip(r).is_none() {
                self.sel_clip = None;
            }
        }
        if let Sel::Track(id) = self.sel {
            if self.project.track(id).is_none() {
                self.sel = self.project.tracks.first().map(|t| Sel::Track(t.id)).unwrap_or(Sel::Master);
            }
        }
    }

    fn set_project(&mut self, p: Project) {
        load_project_samples(&p);
        self.send(Cmd::Stop);
        self.send(Cmd::Stop);
        self.project = p;
        self.committed = self.project.clone();
        self.undo.clear();
        self.redo.clear();
        self.sel_clip = None;
        self.sel = self.project.tracks.first().map(|t| Sel::Track(t.id)).unwrap_or(Sel::Master);
        self.publish = true;
    }

    // -----------------------------------------------------------------------------------------
    // Files

    fn file_new(&mut self) {
        self.set_project(presets::new_project());
        self.file = None;
    }

    fn file_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Musicton Project", &["mton", "json"]).pick_file() {
            match load_project_file(&path) {
                Ok(p) => {
                    self.set_project(p);
                    self.toast(format!("Opened {}", path.display()));
                    self.file = Some(path);
                }
                Err(e) => self.toast(format!("Could not open project: {e}")),
            }
        }
    }

    fn file_save(&mut self, save_as: bool) {
        let path = match (&self.file, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("Musicton Project", &["mton"])
                .set_file_name(format!("{}.mton", self.project.name))
                .save_file(),
        };
        let Some(mut path) = path else { return };
        if path.extension().is_none() {
            path.set_extension("mton");
        }
        if let Some(stem) = path.file_stem() {
            self.project.name = stem.to_string_lossy().to_string();
        }
        self.capture_plugin_states();
        match serde_json::to_string_pretty(&self.project).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.toast(format!("Saved {}", path.display()));
                self.file = Some(path);
                let _ = self.save_version("Saved");
            }
            Err(e) => self.toast(format!("Save failed: {e}")),
        }
    }

    fn autosave(&self) {
        let path = autosave_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string(&self.project) {
            let _ = std::fs::write(path, s);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Live playing, recording, capture

    fn transport_toggle(&mut self) {
        if self.status.playing {
            self.send(Cmd::Stop);
        } else {
            self.send(Cmd::Play);
        }
    }

    fn pos_now(&self) -> (f64, f64) {
        if self.status.playing {
            (self.status.tick as f64 + self.status.step_frac as f64, self.status.arr_pos as f64)
        } else {
            let steps = self.now() * self.project.bpm as f64 / 60.0 * 4.0;
            (steps, steps)
        }
    }

    fn live_note_on(&mut self, key: Option<Key>, pitch: u8, vel: u8, from_midi: bool) {
        let Some(track) = self.selected_track() else { return };
        if !from_midi {
            self.send(Cmd::NoteOn { track, pitch, vel });
        }
        let (start_tick, start_arr) = self.pos_now();
        self.held.push(HeldNote { key, track, pitch, vel, start_tick, start_arr });
        if self.rec && self.status.playing {
            self.ensure_record_clip(track);
        }
    }

    fn live_note_off(&mut self, key: Option<Key>, pitch: u8, from_midi: bool) {
        let idx = self.held.iter().position(|h| if key.is_some() { h.key == key } else { h.pitch == pitch && h.key.is_none() });
        let Some(i) = idx else {
            if !from_midi {
                if let Some(track) = self.selected_track() {
                    self.send(Cmd::NoteOff { track, pitch });
                }
            }
            return;
        };
        let h = self.held.remove(i);
        if !from_midi {
            self.send(Cmd::NoteOff { track: h.track, pitch: h.pitch });
        }
        let (now_tick, _) = self.pos_now();
        let len = (now_tick - h.start_tick).max(0.5);
        self.capture.push_back(Captured { track: h.track, pitch: h.pitch, vel: h.vel, start: h.start_tick, len });
        while self.capture.len() > 1024 {
            self.capture.pop_front();
        }
        if self.rec && self.status.playing {
            self.record_note(&h, len);
        }
    }

    /// Launcher recording: if the track isn't playing anything, create and launch a new clip.
    fn ensure_record_clip(&mut self, track: u64) {
        let Some(ts) = self.track_status(track).cloned() else { return };
        if ts.playing.is_some() || ts.queued.is_some() || (ts.follow_arr && ts.arr_clip.is_some()) {
            return;
        }
        let Some(t) = self.project.track(track) else { return };
        let Some(scene) = (self.sel_scene..t.clips.len()).chain(0..self.sel_scene).find(|&s| t.clips[s].is_none()) else {
            self.toast("No empty clip slot to record into");
            return;
        };
        let clip = Clip::new(&format!("{} Rec", t.name), t.color, 4);
        self.create_clip(track, scene, clip);
        self.send(Cmd::Launch { track, scene });
        self.toast("Recording into a new clip");
    }

    fn record_note(&mut self, h: &HeldNote, len: f64) {
        let Some(ts) = self.track_status(h.track).cloned() else { return };
        let is_drum = self.project.track(h.track).is_some_and(|t| t.instrument.kind.is_drum_like());
        let (r, local_raw) = if let Some(scene) = ts.playing {
            (ClipRef::Session { track: h.track, scene }, h.start_tick.round() - ts.start_tick as f64)
        } else if let (true, Some(id)) = (ts.follow_arr, ts.arr_clip) {
            let start = self.project.track(h.track).and_then(|t| t.arrangement.iter().find(|c| c.id == id)).map(|c| c.start).unwrap_or(0);
            (ClipRef::Arr { track: h.track, id }, h.start_arr.round() - start as f64)
        } else {
            return;
        };
        let Some(clip) = self.clip_mut(r) else { return };
        let len_steps = if is_drum { 1 } else { len.round().max(1.0) as u32 };
        let step = (local_raw.max(0.0) as u32) % clip.len.max(1);
        clip.notes.retain(|n| !(n.step == step && n.pitch == h.pitch));
        clip.notes.push(Note { step, len: len_steps, pitch: h.pitch, vel: h.vel, slide: false, skip: 0 });
        self.touch();
    }

    fn capture_midi(&mut self) {
        let Some(track) = self.selected_track() else {
            self.toast("Select a track first");
            return;
        };
        let notes: Vec<Captured> = self.capture.iter().filter(|c| c.track == track).cloned().collect();
        let Some(last) = notes.iter().map(|n| n.start).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s)))) else {
            self.toast("Nothing to capture - play something on the selected track first");
            return;
        };
        let phrase: Vec<&Captured> = notes.iter().filter(|n| n.start >= last - 128.0).collect();
        let first = phrase.iter().map(|n| n.start).fold(f64::MAX, f64::min);
        let t0 = if self.status.playing { (first / 16.0).floor() * 16.0 } else { first.round() };
        let end = phrase.iter().map(|n| n.start + n.len).fold(0.0, f64::max);
        let bars = (((end - t0) / 16.0).ceil().max(1.0) as u32).next_power_of_two().min(8);
        let t = self.project.track(track).unwrap();
        let is_drum = t.instrument.kind.is_drum_like();
        let mut clip = Clip::new(&format!("{} Capture", t.name), t.color, bars);
        for n in phrase {
            let step = ((n.start - t0).round().max(0.0) as u32) % clip.len;
            if clip.notes.iter().any(|x| x.step == step && x.pitch == n.pitch) {
                continue;
            }
            clip.notes.push(Note { step, len: if is_drum { 1 } else { n.len.round().max(1.0) as u32 }, pitch: n.pitch, vel: n.vel, slide: false, skip: 0 });
        }
        let scene = (self.sel_scene..t.clips.len()).chain(0..self.sel_scene).find(|&s| t.clips[s].is_none());
        match scene {
            Some(s) => {
                self.create_clip(track, s, clip);
                self.toast(format!("Captured {bars} bar(s) of MIDI"));
            }
            None => self.toast("No empty clip slot on this track"),
        }
    }

    fn tap_tempo(&mut self) {
        let now = self.now();
        self.tap_times.retain(|t| now - t < 2.5);
        self.tap_times.push(now);
        if self.tap_times.len() >= 3 {
            let n = self.tap_times.len();
            let avg = (self.tap_times[n - 1] - self.tap_times[0]) / (n - 1) as f64;
            self.project.bpm = (60.0 / avg).clamp(40.0, 300.0).round() as f32;
            self.touch();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Keyboard

    fn handle_keys(&mut self, ctx: &Context) {
        // Tab switches views; claim it before egui uses it for focus traversal.
        let tab = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
        let shift_tab = ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab));
        if ctx.wants_keyboard_input() {
            return;
        }
        if tab {
            self.view = if self.view == View::Session { View::Arrangement } else { View::Session };
        }
        if shift_tab {
            self.detail = if self.detail == Detail::Clip { Detail::Devices } else { Detail::Clip };
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key { key, pressed, repeat, modifiers, .. } = ev else { continue };
            if modifiers.command {
                if !pressed {
                    continue;
                }
                match key {
                    Key::Z if modifiers.shift => self.redo(),
                    Key::Z => self.undo(),
                    Key::Y => self.redo(),
                    Key::S => self.file_save(modifiers.shift),
                    Key::O => self.file_open(),
                    Key::N => self.file_new(),
                    Key::D => self.duplicate_selected_clip(),
                    Key::E => self.export = Some(export::ExportDialog::new(&self.project, self.audio.sample_rate)),
                    Key::T => {
                        self.add_track_from_preset("Dub Stab", None);
                    }
                    Key::G => {
                        if let Some(t) = self.selected_track() {
                            self.group_track(t);
                        }
                    }
                    Key::P if modifiers.alt => {
                        // Show/hide plug-in windows.
                        let any_open = self.plugins.open.values().any(|o| *o);
                        if any_open && !self.plugins.hidden {
                            self.plugins.hidden = true;
                        } else {
                            self.plugins.hidden = false;
                            if !any_open {
                                if let Some(t) = self.selected_track().filter(|t| self.project.track(*t).is_some_and(|x| matches!(x.instrument.kind, InstrumentKind::B303 | InstrumentKind::Nexa))) {
                                    self.plugins.open.insert(t, true);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if pressed && !repeat && (self.learning.is_some_and(|(_, k)| k == mapping::LearnKind::Key) || self.project.mappings.iter().any(|m| m.source == MapSource::Key(format!("{key:?}")))) {
                self.handle_mapped_key(key);
                continue;
            }
            if !pressed && self.project.mappings.iter().any(|m| m.source == MapSource::Key(format!("{key:?}"))) {
                continue;
            }
            if let Some(semi) = piano_key(key) {
                let is_drum = self.selected_track().and_then(|id| self.project.track(id)).is_some_and(|t| t.instrument.kind.is_drum_like());
                let pitch = if is_drum {
                    let lanes = self.selected_track().and_then(|id| self.project.track(id)).map(|t| t.instrument.lane_names().len()).unwrap_or(DRUM_LANES.len());
                    if semi >= lanes as i32 {
                        continue;
                    }
                    semi as u8
                } else {
                    ((self.kb_octave + 1) * 12 + semi).clamp(0, 127) as u8
                };
                if pressed && !repeat {
                    self.live_note_on(Some(key), pitch, self.kb_vel, false);
                } else if !pressed {
                    self.live_note_off(Some(key), pitch, false);
                }
                continue;
            }
            if !pressed || repeat {
                continue;
            }
            match key {
                Key::Space => self.transport_toggle(),
                Key::Z => self.kb_octave = (self.kb_octave - 1).max(-1),
                Key::X => self.kb_octave = (self.kb_octave + 1).min(8),
                Key::C => self.kb_vel = self.kb_vel.saturating_sub(20).max(1),
                Key::V => self.kb_vel = (self.kb_vel + 20).min(127),
                Key::M => {
                    self.metronome = !self.metronome;
                    self.send(Cmd::Metronome(self.metronome));
                }
                Key::Delete | Key::Backspace => {
                    if !self.delete_selected_note() {
                        self.delete_selected_clip();
                    }
                }
                Key::Enter => {
                    if let Some(ClipRef::Session { track, scene }) = self.sel_clip {
                        self.send(Cmd::Launch { track, scene });
                    }
                }
                Key::F9 => self.rec = !self.rec,
                _ => {}
            }
        }
    }

    fn poll_midi(&mut self) {
        while let Ok(ev) = self.midi_rx.try_recv() {
            match ev {
                MidiEvent::NoteOn { pitch, vel } => self.live_note_on(None, pitch, vel, true),
                MidiEvent::NoteOff { pitch } => self.live_note_off(None, pitch, true),
                MidiEvent::Cc { ch, cc, val } => self.handle_cc(ch, cc, val),
                MidiEvent::Mapped { ch, note, vel } => self.handle_mapped_note(ch, note, vel),
            }
        }
        let target = self.selected_track().unwrap_or(0);
        self.midi.target_track.store(target, std::sync::atomic::Ordering::Relaxed);
        let is_drum = self.project.track(target).is_some_and(|t| t.instrument.kind.is_drum_like());
        self.midi.target_is_drum.store(is_drum, std::sync::atomic::Ordering::Relaxed);
    }

    fn read_status(&mut self) {
        {
            let mut st = self.audio.shared.status.lock();
            self.status = st.clone();
            for t in &mut st.tracks {
                t.meter = [0.0; 2];
            }
            st.returns = [[0.0; 2]; 2];
            st.master = [0.0; 2];
        }
        let decay = 0.82;
        let mut upd = |id: u64, v: [f32; 2]| {
            let m = self.meters.entry(id).or_insert([0.0; 2]);
            m[0] = v[0].max(m[0] * decay);
            m[1] = v[1].max(m[1] * decay);
        };
        for t in &self.status.tracks {
            upd(t.id, t.meter);
        }
        upd(RETURN_METER, self.status.returns[0]);
        upd(RETURN_METER + 1, self.status.returns[1]);
        upd(MASTER_METER, self.status.master);
    }

    fn meter(&self, id: u64) -> [f32; 2] {
        self.meters.get(&id).copied().unwrap_or_default()
    }

    // -----------------------------------------------------------------------------------------
    // Drag and drop

    /// Returns the payload if it was dropped onto `rect` this frame (and highlights the target).
    fn drop_target(&mut self, ui: &egui::Ui, rect: egui::Rect, accept: impl Fn(&Payload) -> bool) -> Option<Payload> {
        let payload = self.drag.as_ref()?;
        if !accept(payload) {
            return None;
        }
        let pos = ui.ctx().input(|i| i.pointer.latest_pos())?;
        if !rect.contains(pos) {
            return None;
        }
        ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0_f32, ACCENT), egui::StrokeKind::Inside);
        if ui.ctx().input(|i| i.pointer.any_released()) {
            return self.drag.take();
        }
        None
    }

    fn draw_drag(&self, ctx: &Context) {
        let Some(p) = &self.drag else { return };
        let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drag")));
        let text = p.label();
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), TEXT_DARK);
        let rect = egui::Rect::from_min_size(pos + egui::vec2(12.0, 6.0), galley.size() + egui::vec2(10.0, 6.0));
        painter.rect_filled(rect, 3.0, ACCENT);
        painter.galley(rect.min + egui::vec2(5.0, 3.0), galley, TEXT_DARK);
    }

    // -----------------------------------------------------------------------------------------
    // Layout

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let now = self.now();
            if let Some((target, kind)) = self.learning {
                let what = if kind == mapping::LearnKind::Midi { "Move a control on your MIDI controller" } else { "Press a computer key" };
                ui.colored_label(BLUE, format!("{what} to map \"{}\"  (Esc cancels)", self.target_name(target)));
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.learning = None;
                    self.midi.learning.store(false, std::sync::atomic::Ordering::Relaxed);
                }
            } else if let Some((label, _)) = self.jobs.first() {
                ui.spinner();
                ui.colored_label(ACCENT, label);
            } else if let Some((msg, until)) = &self.toast {
                if *until > now {
                    ui.colored_label(ACCENT, msg);
                } else {
                    self.toast = None;
                }
            } else if let Some((title, _)) = &self.info {
                ui.colored_label(TEXT_DIM, title);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let kb = format!(
                    "Keys: oct {}  vel {}   |   {} @ {} Hz",
                    self.kb_octave,
                    self.kb_vel,
                    self.audio.device_name,
                    self.audio.sample_rate
                );
                ui.colored_label(TEXT_DIM, kb);
                if let Some(port) = &self.midi.port_name {
                    ui.colored_label(TEXT_DIM, format!("MIDI: {port}  |"));
                }
                if let Some(track) = self.selected_track().and_then(|id| self.project.track(id)) {
                    ui.colored_label(color32(track.color), format!("{}  |", track.name));
                }
            });
        });
    }

    fn info_view(&mut self, ui: &mut egui::Ui) {
        let (title, body) = self.info.clone().unwrap_or_else(|| {
            (
                "Help Panel".into(),
                "Hover over any control to see what it does.\n\nTab: Launcher/Timeline\nShift+Tab: Clip/Device view\nSpace: Play/Stop\nA-K row: play notes, Z/X octave\nCtrl+Z/Y: Undo/Redo".into(),
            )
        });
        ui.label(egui::RichText::new(title).strong().size(12.5));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(body).size(11.5).color(TEXT));
    }
}

impl App {
    fn debug_screenshot(&mut self, ctx: &Context) {
        let Some(path) = self.shot.clone() else { return };
        self.frames += 1;
        if self.frames % 30 == 0 {
            eprintln!("debug screenshot request at frame {}", self.frames);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let images: Vec<_> = ctx.input(|i| {
            i.raw.events.iter().chain(i.events.iter()).filter_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }).collect()
        });
        for img in images {
            let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
            for px in &img.pixels {
                out.extend_from_slice(&[px.r(), px.g(), px.b()]);
            }
            let _ = std::fs::write(&path, out);
            self.shot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn piano_key(key: Key) -> Option<i32> {
    Some(match key {
        Key::A => 0,
        Key::W => 1,
        Key::S => 2,
        Key::E => 3,
        Key::D => 4,
        Key::F => 5,
        Key::T => 6,
        Key::G => 7,
        Key::Y => 8,
        Key::H => 9,
        Key::U => 10,
        Key::J => 11,
        Key::K => 12,
        Key::O => 13,
        Key::L => 14,
        Key::P => 15,
        _ => return None,
    })
}

/// The demo project plus an FX audio track using the Core library (if it is available).
pub fn demo_with_audio() -> Project {
    let mut p = presets::demo_project();
    let core = browser::user_sample_dir().join(crate::library::CORE_DIR);
    let riser = core.join("FX/Noise Riser 8 Bars 130bpm.wav");
    let impact = core.join("FX/Impact.wav");
    let (Ok(rs), Ok(is)) = (samples::load(&riser.to_string_lossy()), samples::load(&impact.to_string_lossy())) else { return p };
    let color = CLIP_COLORS[13];
    let id = p.add_track("FX", InstrumentKind::Audio, color);
    let rclip = Clip::from_audio(&riser.to_string_lossy(), rs.left.len(), rs.sample_rate, p.bpm, color);
    let iclip = Clip::from_audio(&impact.to_string_lossy(), is.left.len(), is.sample_rate, p.bpm, color);
    let rid = p.new_id();
    let iid = p.new_id();
    let t = p.track_mut(id).unwrap();
    t.channel.sends = [0.3, 0.0];
    t.channel.volume = 0.7;
    t.clips[3] = Some(rclip.clone());
    t.clips[4] = Some(iclip.clone());
    // Riser across the break (bars 49-56), impact on the first beat of the peak.
    t.arrangement.push(ArrClip { id: rid, start: 48 * STEPS_PER_BAR, len: rclip.len, clip: rclip });
    t.arrangement.push(ArrClip { id: iid, start: 56 * STEPS_PER_BAR, len: iclip.len, clip: iclip });
    p
}

fn load_project_samples(p: &Project) {
    for t in &p.tracks {
        for s in t.instrument.samples.iter().flatten() {
            if let Err(e) = samples::load(s) {
                eprintln!("could not load sample {s}: {e}");
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.shot.is_some() && self.frames < 3 {
            eprintln!("frame {}", self.frames);
        }
        self.info = None;
        self.deliver_screenshots(ctx);
        self.poll_remote(ctx);
        self.poll_jobs();
        self.read_status();
        self.poll_midi();
        self.handle_keys(ctx);

        egui::TopBottomPanel::top("transport")
            .exact_height(34.0)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(6, 4)))
            .show(ctx, |ui| self.transport_bar(ui));

        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(22.0)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(8, 2)))
            .show(ctx, |ui| self.status_bar(ui));

        if self.show_browser {
            egui::SidePanel::left("browser")
                .resizable(true)
                .default_width(210.0)
                .width_range(160.0..=400.0)
                .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::same(4)))
                .show(ctx, |ui| self.browser_panel(ui));
        }

        if self.show_detail {
            egui::TopBottomPanel::bottom("detail")
                .resizable(true)
                .default_height(290.0)
                .height_range(160.0..=600.0)
                .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(3)))
                .show(ctx, |ui| {
                    ui.horizontal_top(|ui| {
                        if self.show_info {
                            egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                                ui.set_width(180.0);
                                ui.set_height(ui.available_height());
                                ui.vertical(|ui| self.info_view(ui));
                            });
                        }
                        egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_height(ui.available_height());
                            ui.vertical(|ui| match self.detail {
                                Detail::Clip => self.clip_detail(ui),
                                Detail::Devices => self.device_detail(ui),
                            });
                        });
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(3)))
            .show(ctx, |ui| match self.view {
                View::Session => self.session_view(ui),
                View::Arrangement => self.arrangement_view(ui),
            });

        self.export_window(ctx);
        self.mappings_window(ctx);
        self.plugin_windows(ctx);
        self.chord_pads_window(ctx);
        self.rules_window(ctx);
        self.versions_window(ctx);
        self.video_window(ctx);
        self.sync_plugins();
        self.plugin_overlay_ui(ctx);
        self.update_arr_record();
        self.update_audio_record();
        self.draw_drag(ctx);
        if ctx.input(|i| i.pointer.any_released()) {
            self.drag = None;
        }
        self.end_frame(ctx);
        self.debug_screenshot(ctx);

        if self.status.playing || !self.held.is_empty() || self.drag.is_some() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.inject_input(raw_input);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.autosave();
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(20)
    }
}
