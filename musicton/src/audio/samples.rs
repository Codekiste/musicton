//! Global bank of decoded WAV samples, shared between the UI (which loads them) and the
//! audio thread (which only does non-blocking lookups).

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

pub struct Sample {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: f32,
}

static BANK: LazyLock<RwLock<HashMap<String, Arc<Sample>>>> = LazyLock::new(Default::default);

pub fn load(path: &str) -> Result<Arc<Sample>, String> {
    if let Some(s) = BANK.read().get(path) {
        return Ok(s.clone());
    }
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let data: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().filter_map(Result::ok).map(|s| s as f32 * scale).collect()
        }
    };
    let frames = data.len() / ch;
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for f in 0..frames {
        let l = data[f * ch];
        let r = if ch > 1 { data[f * ch + 1] } else { l };
        left.push(l);
        right.push(r);
    }
    let s = Arc::new(Sample { left, right, sample_rate: spec.sample_rate as f32 });
    BANK.write().insert(path.to_string(), s.clone());
    Ok(s)
}

/// Non-blocking lookup for the audio thread.
pub fn try_get(path: &str) -> Option<Arc<Sample>> {
    BANK.try_read()?.get(path).cloned()
}
