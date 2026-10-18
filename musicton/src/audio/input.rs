//! Audio input for recording onto audio tracks. The stream is opened only when a track is armed.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Default)]
pub struct InputState {
    pub recording: bool,
    /// Interleaved stereo.
    pub data: Vec<f32>,
    pub level: f32,
}

pub struct Input {
    _stream: Option<cpal::Stream>,
    pub state: Arc<Mutex<InputState>>,
    pub sample_rate: u32,
    pub device_name: String,
    pub error: Option<String>,
}

impl Input {
    pub fn open() -> Self {
        let state = Arc::new(Mutex::new(InputState::default()));
        match open_stream(state.clone()) {
            Ok((stream, sr, name)) => Self { _stream: Some(stream), state, sample_rate: sr, device_name: name, error: None },
            Err(e) => Self { _stream: None, state, sample_rate: 48000, device_name: "none".into(), error: Some(e) },
        }
    }
}

fn open_stream(state: Arc<Mutex<InputState>>) -> Result<(cpal::Stream, u32, String), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("no audio input device found")?;
    let name = device.name().unwrap_or_else(|_| "default".into());
    let cfg = device.default_input_config().map_err(|e| e.to_string())?;
    let format = cfg.sample_format();
    let config: cpal::StreamConfig = cfg.into();
    let ch = config.channels.max(1) as usize;
    let sr = config.sample_rate.0;
    let err = |e| eprintln!("input stream error: {e}");
    let push = move |st: &Arc<Mutex<InputState>>, frames: &mut dyn Iterator<Item = (f32, f32)>| {
        let mut s = st.lock();
        let mut peak: f32 = s.level * 0.9;
        let rec = s.recording;
        for (l, r) in frames {
            peak = peak.max(l.abs()).max(r.abs());
            if rec {
                s.data.push(l);
                s.data.push(r);
            }
        }
        s.level = peak;
    };
    let st = state.clone();
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                let mut it = data.chunks(ch).map(|f| (f[0], if ch > 1 { f[1] } else { f[0] }));
                push(&st, &mut it);
            },
            err,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                let mut it = data.chunks(ch).map(|f| {
                    let l = f[0] as f32 / 32768.0;
                    (l, if ch > 1 { f[1] as f32 / 32768.0 } else { l })
                });
                push(&st, &mut it);
            },
            err,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            &config,
            move |data: &[i32], _| {
                let mut it = data.chunks(ch).map(|f| {
                    let l = f[0] as f32 / 2147483648.0;
                    (l, if ch > 1 { f[1] as f32 / 2147483648.0 } else { l })
                });
                push(&st, &mut it);
            },
            err,
            None,
        ),
        other => return Err(format!("unsupported input format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, sr, name))
}

pub fn write_recording(dir: &std::path::Path, data: &[f32], sr: u32) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("Recording {stamp}.wav"));
    let spec = hound::WavSpec { channels: 2, sample_rate: sr, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
    for s in &data[..data.len() & !1] {
        w.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0) as i32).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())?;
    Ok(path)
}
