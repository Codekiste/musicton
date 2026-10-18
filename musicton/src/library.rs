//! The bundled "Musicton Core" sample library (rendered from the built-in instruments on first
//! start, so it is ours to ship) and the optional CC0 sample pack download.

use crate::audio::engine::Engine;
use crate::model::*;
use crate::params::{self, lane_param, LANE_DECAY, LANE_TONE, LANE_TUNE};
use crate::presets;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SR: u32 = 44100;
const BPM: f32 = 130.0;
pub const CORE_DIR: &str = "Musicton Core";
pub const CC0_DIR: &str = "CC0 Free Drum Samples";
pub const CC0_URL: &str = "https://github.com/Boochi44/free-drum-samples.git";

fn render(p: Project, secs: f32) -> Vec<f32> {
    let mut e = Engine::new(SR as f32, Arc::new(p));
    e.force_scene(0);
    let mut out = vec![0.0f32; (secs * SR as f32) as usize * 2];
    for chunk in out.chunks_mut(512) {
        e.process(chunk);
    }
    out
}

fn write_wav(path: &Path, data: &[f32], trim: bool) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut end = data.len();
    if trim {
        // Cut trailing silence and add a short fade.
        while end > 2 && data[end - 1].abs() < 0.0005 && data[end - 2].abs() < 0.0005 {
            end -= 2;
        }
    }
    let spec = hound::WavSpec { channels: 2, sample_rate: SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(std::io::Error::other)?;
    end &= !1;
    let fade = 256usize.min(end / 2);
    for (i, s) in data[..end].iter().enumerate() {
        let g = if trim && i + fade * 2 > end { (end - i) as f32 / (fade * 2) as f32 } else { 1.0 };
        w.write_sample((s.clamp(-1.0, 1.0) * g * 32767.0) as i16).map_err(std::io::Error::other)?;
    }
    w.finalize().map_err(std::io::Error::other)
}

/// A project with one track playing `notes` from scene 0. The master is left clean
/// (no limiter) so one-shots keep their natural dynamics.
fn one_track(inst: Instrument, notes: Vec<Note>, bars: u32, fx: Vec<Device>, send_a: f32) -> Project {
    let mut p = Project::empty();
    p.bpm = BPM;
    p.master.devices.clear();
    let id = p.add_track("T", inst.kind, CLIP_COLORS[0]);
    let t = p.track_mut(id).unwrap();
    t.instrument = inst;
    t.channel.devices = fx;
    t.channel.sends = [send_a, 0.0];
    let mut clip = Clip::new("c", CLIP_COLORS[0], bars);
    clip.notes = notes;
    t.clips[0] = Some(clip);
    p
}

fn note(step: u32, len: u32, pitch: u8, vel: u8) -> Note {
    Note { step, len, pitch, vel, slide: false }
}

fn drum_hit(lane: usize, tune: f32, decay: f32, tone: f32, drive: f32) -> Vec<f32> {
    let mut inst = presets::preset("909 Punch").build();
    inst.params[0] = drive;
    inst.params[lane_param(lane, LANE_TUNE)] = tune;
    inst.params[lane_param(lane, LANE_DECAY)] = decay;
    inst.params[lane_param(lane, LANE_TONE)] = tone;
    inst.params[lane_param(lane, params::LANE_LEVEL)] = 0.9;
    // Long clip so the hit plays exactly once.
    render(one_track(inst, vec![note(0, 1, lane as u8, 120)], 8, vec![], 0.0), 3.0)
}

pub fn core_library_exists(root: &Path) -> bool {
    root.join(CORE_DIR).join("README.txt").exists()
}

/// Renders the core library into `root/Musicton Core`. Takes a few seconds; run it on a thread.
pub fn build_core_library(root: &Path) -> std::io::Result<usize> {
    let base = root.join(CORE_DIR);
    let mut count = 0;
    let mut save = |rel: &str, data: Vec<f32>, trim: bool| -> std::io::Result<()> {
        write_wav(&base.join(rel), &data, trim)?;
        count += 1;
        Ok(())
    };

    // One-shot drums.
    let kicks = [("Kick 909", 0.0, 1.0, 0.5, 0.15), ("Kick Deep", -3.0, 1.6, 0.3, 0.1), ("Kick Punch", 1.0, 0.8, 0.8, 0.3), ("Kick Hard", -1.0, 1.3, 0.9, 0.7), ("Kick Short", 2.0, 0.5, 0.6, 0.2), ("Kick Rumble", -5.0, 2.4, 0.4, 0.5)];
    for (name, tune, decay, tone, drive) in kicks {
        save(&format!("Drums/Kicks/{name}.wav"), drum_hit(0, tune, decay, tone, drive), true)?;
    }
    let others: &[(usize, &str, &[(f32, f32, f32)])] = &[
        (1, "Claps/Clap", &[(0.0, 1.0, 0.5), (-2.0, 1.6, 0.3), (2.0, 0.7, 0.8)]),
        (2, "Snares/Snare", &[(0.0, 1.0, 0.5), (-3.0, 1.4, 0.8), (3.0, 0.6, 0.3)]),
        (3, "Percussion/Rim", &[(0.0, 1.0, 0.5), (-4.0, 1.0, 0.8)]),
        (4, "Hats/Closed Hat", &[(0.0, 1.0, 0.5), (0.0, 0.6, 0.8), (-2.0, 1.4, 0.3)]),
        (5, "Hats/Open Hat", &[(0.0, 1.0, 0.5), (0.0, 1.6, 0.8), (-2.0, 0.7, 0.3)]),
        (6, "Cymbals/Ride", &[(0.0, 1.0, 0.5), (-2.0, 1.6, 0.7)]),
        (7, "Percussion/Low Tom", &[(0.0, 1.0, 0.5), (-5.0, 1.5, 0.2)]),
        (8, "Percussion/Hi Tom", &[(0.0, 1.0, 0.5), (4.0, 0.8, 0.6)]),
        (9, "Percussion/Cowbell", &[(0.0, 1.0, 0.5), (-7.0, 1.3, 0.8)]),
    ];
    for (lane, name, variants) in others {
        for (i, (tune, decay, tone)) in variants.iter().enumerate() {
            save(&format!("Drums/{name} {}.wav", i + 1), drum_hit(*lane, *tune, *decay, *tone, 0.1), true)?;
        }
    }

    // Drum loops (2 bars at 130 BPM, named with the tempo so warping detects it).
    for pat in presets::PATTERNS.iter().filter(|p| p.kind == InstrumentKind::Drums && p.bars <= 2) {
        let clip = pat.clip(CLIP_COLORS[1]);
        let reps = 2 / pat.bars.max(1);
        let mut notes = vec![];
        for r in 0..reps {
            notes.extend(clip.notes.iter().map(|n| Note { step: n.step + r * clip.len, ..*n }));
        }
        let inst = presets::preset("909 Punch").build();
        let data = render(one_track(inst, notes, 2, vec![], 0.0), 2.0 * 4.0 * 60.0 / BPM);
        save(&format!("Loops/Drums/{} 130bpm.wav", pat.name), data, false)?;
    }

    // Bass and synth loops.
    for (pat_name, preset, folder) in [
        ("Acid Line 1", "TB Acid", "Bass"),
        ("Acid Line 2", "TB Acid", "Bass"),
        ("Rolling 16ths", "Rolling Sub", "Bass"),
        ("Offbeat Bass", "Rolling Sub", "Bass"),
        ("Hypnotic 3-Step", "Square Acid", "Bass"),
        ("Minor Stabs", "Dub Stab", "Synth"),
        ("Dub Chord", "Dub Stab", "Synth"),
        ("Arp Am", "Pluck", "Synth"),
    ] {
        let pat = presets::pattern(pat_name).unwrap();
        let clip = pat.clip(CLIP_COLORS[0]);
        let bars = 2;
        let reps = bars / pat.bars.max(1);
        let mut notes = vec![];
        for r in 0..reps.max(1) {
            notes.extend(clip.notes.iter().map(|n| Note { step: n.step + r * clip.len, ..*n }));
        }
        let fx = if folder == "Synth" { vec![Device::new(900, DeviceKind::Echo).with(5, 0.25)] } else { vec![] };
        let data = render(one_track(presets::preset(preset).build(), notes, bars, fx, 0.15), bars as f32 * 4.0 * 60.0 / BPM);
        save(&format!("Loops/{folder}/{pat_name} {preset} 130bpm.wav"), data, false)?;
    }

    // Synth one-shots (stabs and hits), rooted on A.
    for (name, preset, pitches) in [
        ("Stab Am7", "Dub Stab", vec![57u8, 60, 64, 67]),
        ("Stab Am9", "Dub Stab", vec![57, 60, 64, 67, 71]),
        ("Hoover A", "Hoover Lead", vec![45, 57]),
        ("FM Bell A", "FM Bell", vec![69]),
        ("FM Metal Hit", "FM Metal Perc", vec![45]),
        ("Pluck A", "Pluck", vec![57]),
        ("Pad Am", "Warehouse Pad", vec![57, 60, 64]),
        ("Bass A", "Rolling Sub", vec![33]),
        ("Acid A", "TB Acid", vec![45]),
        ("String A", "Plucked String", vec![45]),
    ] {
        let notes = pitches.iter().map(|&p| note(0, if name.starts_with("Pad") { 32 } else { 2 }, p, 110)).collect();
        let secs = if name.starts_with("Pad") { 6.0 } else { 2.5 };
        let data = render(one_track(presets::preset(preset).build(), notes, 16, vec![], 0.2), secs);
        save(&format!("One Shots/Synth/{name}.wav"), data, true)?;
    }

    // FX: noise riser, downlifter and impact.
    let sr = SR as f32;
    let riser_len = (8.0 * 4.0 * 60.0 / BPM * sr) as usize;
    let mut riser = Vec::with_capacity(riser_len * 2);
    let mut seed = 12345u32;
    let mut lp = [0.0f32; 2];
    for i in 0..riser_len {
        let t = i as f32 / riser_len as f32;
        let cutoff = 200.0 * (60.0f32).powf(t);
        let a = (-2.0 * std::f32::consts::PI * cutoff / sr).exp();
        for v in lp.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let n = seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
            *v = n + a * (*v - n);
            riser.push(*v * t * t * 0.9);
        }
    }
    save("FX/Noise Riser 8 Bars 130bpm.wav", riser.clone(), false)?;
    let mut down: Vec<f32> = riser.chunks(2).rev().flat_map(|c| [c[0], c[1]]).collect();
    down.truncate(riser.len() / 2);
    save("FX/Downlifter.wav", down, true)?;
    let mut impact = drum_hit(0, -6.0, 2.5, 0.9, 0.8);
    let crash = drum_hit(5, -3.0, 2.5, 0.9, 0.3);
    for (a, b) in impact.iter_mut().zip(crash) {
        *a = (*a + b * 0.6).clamp(-1.0, 1.0);
    }
    save("FX/Impact.wav", impact, true)?;

    std::fs::write(
        base.join("README.txt"),
        "Musicton Core Library\n\nAll sounds in this folder were rendered from Musicton's built-in instruments.\nYou may use them freely in your music, commercially or not (CC0 / public domain).\n",
    )?;
    Ok(count)
}

pub fn git_available() -> bool {
    std::process::Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Clones the CC0 drum sample pack (shallow) into `root/CC0 Free Drum Samples`.
pub fn download_cc0(root: &Path) -> Result<PathBuf, String> {
    let dest = root.join(CC0_DIR);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("git")
        .args(["clone", "--depth", "1", CC0_URL])
        .arg(&dest)
        .output()
        .map_err(|e| format!("git not available: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let _ = std::fs::remove_dir_all(dest.join(".git"));
    std::fs::write(
        dest.join("LICENSE-NOTE.txt"),
        format!("Downloaded from {CC0_URL}\nThe repository README states the samples are released under CC0 1.0.\n"),
    )
    .map_err(|e| e.to_string())?;
    Ok(dest)
}
