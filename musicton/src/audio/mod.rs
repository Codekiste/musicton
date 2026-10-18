//! Audio output (cpal) and offline export.

pub mod audioclip;
pub mod b303;
pub mod dsp;
pub mod effects;
pub mod effects2;
pub mod effects3;
pub mod effects4;
pub mod instruments3;
pub mod beatpads;
pub mod samplers;
pub mod analysis;
pub mod engine;
pub mod input;
pub mod instruments;
pub mod instruments2;
pub mod midifx;
pub mod nexa;
pub mod samples;

use crate::model::Project;
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use crossbeam_channel::{Receiver, Sender};
use engine::{Cmd, Engine, Status};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub struct Shared {
    pub project: ArcSwap<Project>,
    pub status: Mutex<Status>,
}

pub struct Audio {
    pub shared: Arc<Shared>,
    pub tx: Sender<Cmd>,
    _stream: Option<cpal::Stream>,
    pub sample_rate: u32,
    pub device_name: String,
    pub error: Option<String>,
}

impl Audio {
    pub fn start(project: &Project) -> Self {
        let shared = Arc::new(Shared {
            project: ArcSwap::from_pointee(project.clone()),
            status: Mutex::new(Status::default()),
        });
        let (tx, rx) = crossbeam_channel::bounded(4096);
        match open_stream(shared.clone(), rx) {
            Ok((stream, sr, name)) => Self { shared, tx, _stream: Some(stream), sample_rate: sr, device_name: name, error: None },
            Err(e) => {
                eprintln!("audio: {e}");
                Self { shared, tx, _stream: None, sample_rate: 48000, device_name: "none".into(), error: Some(e) }
            }
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.try_send(cmd);
    }

    pub fn publish(&self, project: &Project) {
        self.shared.project.store(Arc::new(project.clone()));
    }
}

fn open_stream(shared: Arc<Shared>, rx: Receiver<Cmd>) -> Result<(cpal::Stream, u32, String), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no audio output device found")?;
    let name = device.name().unwrap_or_else(|_| "default".into());
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let mut config: cpal::StreamConfig = supported.into();
    config.channels = config.channels.max(1);
    let sr = config.sample_rate.0;
    let make_engine = || {
        let mut e = Engine::new(sr as f32, shared.project.load_full());
        e.attach(rx.clone(), shared.clone());
        e
    };
    // Prefer a small buffer for low latency, fall back to the device default.
    let mut last_err = String::new();
    for buffer in [cpal::BufferSize::Fixed(256), cpal::BufferSize::Fixed(512), cpal::BufferSize::Default] {
        let mut cfg = config.clone();
        cfg.buffer_size = buffer;
        let res = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &cfg, make_engine()),
            cpal::SampleFormat::I16 => build::<i16>(&device, &cfg, make_engine()),
            cpal::SampleFormat::I32 => build::<i32>(&device, &cfg, make_engine()),
            cpal::SampleFormat::U16 => build::<u16>(&device, &cfg, make_engine()),
            other => return Err(format!("unsupported sample format {other:?}")),
        };
        match res {
            Ok(stream) => {
                stream.play().map_err(|e| e.to_string())?;
                return Ok((stream, sr, name));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut engine: Engine,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let mut buf: Vec<f32> = vec![0.0; 8192];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels;
                if buf.len() < frames * 2 {
                    buf.resize(frames * 2, 0.0);
                }
                let stereo = &mut buf[..frames * 2];
                engine.process(stereo);
                for (f, frame) in data.chunks_mut(channels).enumerate() {
                    let (l, r) = (stereo[f * 2], stereo[f * 2 + 1]);
                    for (c, s) in frame.iter_mut().enumerate() {
                        let v = match (channels, c) {
                            (1, _) => (l + r) * 0.5,
                            (_, 0) => l,
                            (_, 1) => r,
                            _ => 0.0,
                        };
                        *s = T::from_sample(v);
                    }
                }
            },
            |e| eprintln!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------
// Offline export

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ExportSource {
    Arrangement,
    Scene { scene: usize, bars: u32 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BitDepth {
    Int16,
    Int24,
    Float32,
}

pub struct ExportJob {
    pub progress: Arc<AtomicU32>,
    pub result: Arc<Mutex<Option<Result<String, String>>>>,
}

pub fn export_wav(project: Project, source: ExportSource, sr: u32, depth: BitDepth, normalize: bool, path: String) -> ExportJob {
    let progress = Arc::new(AtomicU32::new(0));
    let result = Arc::new(Mutex::new(None));
    let (prog, res) = (progress.clone(), result.clone());
    std::thread::spawn(move || {
        let r = render_to_file(project, source, sr, depth, normalize, &path, &prog, 0.0, 1.0).map(|_| path);
        *res.lock() = Some(r);
    });
    ExportJob { progress, result }
}

/// Renders every track on its own (soloed, with its effects and sends) into `dir`.
pub fn export_stems(project: Project, source: ExportSource, sr: u32, depth: BitDepth, normalize: bool, dir: String) -> ExportJob {
    let progress = Arc::new(AtomicU32::new(0));
    let result = Arc::new(Mutex::new(None));
    let (prog, res) = (progress.clone(), result.clone());
    std::thread::spawn(move || {
        let n = project.tracks.len().max(1);
        let mut r = Ok(dir.clone());
        for i in 0..project.tracks.len() {
            let mut p = project.clone();
            for (j, t) in p.tracks.iter_mut().enumerate() {
                t.channel.solo = i == j;
                t.channel.mute = false;
            }
            let name: String = project.tracks[i].name.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { '_' }).collect();
            let path = std::path::Path::new(&dir).join(format!("{:02} {name}.wav", i + 1)).to_string_lossy().to_string();
            if let Err(e) = render_to_file(p, source, sr, depth, normalize, &path, &prog, i as f64 / n as f64, 1.0 / n as f64) {
                r = Err(e);
                break;
            }
        }
        *res.lock() = Some(r);
    });
    ExportJob { progress, result }
}

fn render_to_file(
    mut project: Project,
    source: ExportSource,
    sr: u32,
    depth: BitDepth,
    normalize: bool,
    path: &str,
    progress: &AtomicU32,
    prog_base: f64,
    prog_scale: f64,
) -> Result<(), String> {
    project.loop_on = false;
    let steps = match source {
        ExportSource::Arrangement => project.arrangement_end(),
        ExportSource::Scene { bars, .. } => bars * crate::model::STEPS_PER_BAR,
    };
    if steps == 0 {
        return Err("Nothing to export - the arrangement is empty.".into());
    }
    let step_secs = 60.0 / project.bpm as f64 / 4.0;
    let tail = 3.0;
    let total = ((steps as f64 * step_secs + tail) * sr as f64) as usize;
    let music_end = (steps as f64 * step_secs * sr as f64) as usize;
    let mut engine = Engine::new(sr as f32, Arc::new(project));
    match source {
        ExportSource::Arrangement => engine.handle(Cmd::Play),
        ExportSource::Scene { scene, .. } => engine.force_scene(scene),
    }
    let mut out = vec![0.0f32; total * 2];
    let chunk = 512;
    let mut pos = 0;
    let mut stopped = false;
    while pos < total {
        let n = chunk.min(total - pos);
        if !stopped && pos >= music_end {
            engine.handle(Cmd::Stop);
            stopped = true;
        }
        engine.process(&mut out[pos * 2..(pos + n) * 2]);
        pos += n;
        progress.store(((prog_base + pos as f64 / total as f64 * prog_scale) * 1000.0) as u32, Ordering::Relaxed);
    }
    if normalize {
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if peak > 0.0001 {
            let g = 0.98 / peak;
            out.iter_mut().for_each(|s| *s *= g);
        }
    }
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sr,
        bits_per_sample: match depth {
            BitDepth::Int16 => 16,
            BitDepth::Int24 => 24,
            BitDepth::Float32 => 32,
        },
        sample_format: if depth == BitDepth::Float32 { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for s in out {
        let r = match depth {
            BitDepth::Int16 => w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16),
            BitDepth::Int24 => w.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0) as i32),
            BitDepth::Float32 => w.write_sample(s),
        };
        r.map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}
