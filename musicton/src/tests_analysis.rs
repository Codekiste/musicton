//! Audio-to-MIDI and stem splitting on synthetic signals.

use crate::audio::analysis::{audio_to_midi, split_stems, MidiMode};
use crate::audio::samples::Sample;
use std::f32::consts::TAU;

const SR: f32 = 48000.0;

fn mono_sample(x: Vec<f32>) -> Sample {
    Sample { right: x.clone(), left: x, sample_rate: SR }
}

fn hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

#[test]
fn melody_to_midi_finds_notes() {
    // C4 E4 G4, half a second each = one beat at 120 BPM.
    let mut x = vec![0.0f32; (SR * 1.6) as usize];
    for (k, m) in [60.0, 64.0, 67.0].iter().enumerate() {
        let start = (k as f32 * 0.5 * SR) as usize;
        for i in 0..(0.45 * SR) as usize {
            let t = i as f32 / SR;
            x[start + i] = (t * hz(*m) * TAU).sin() * 0.5 * (1.0 - (-t * 200.0).exp());
        }
    }
    let notes = audio_to_midi(&mono_sample(x), MidiMode::Melody, 120.0);
    let got: Vec<(u32, u8)> = notes.iter().map(|n| (n.step, n.pitch)).collect();
    assert_eq!(got, vec![(0, 60), (4, 64), (8, 67)], "{notes:?}");
}

#[test]
fn drums_to_midi_classifies_hits() {
    let mut x = vec![0.0f32; SR as usize];
    let mut rng = 1u32;
    let mut noise = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng as f32 / u32::MAX as f32 - 0.5
    };
    // Kick at 0, hat at 0.25 s (step 2 at 120 BPM), snare at 0.5 s (step 4).
    for i in 0..6000 {
        let t = i as f32 / SR;
        x[i] += (t * 55.0 * TAU).sin() * (-t * 20.0).exp() * 0.9;
    }
    let mut prev = 0.0;
    for i in 0..3000 {
        let n = noise();
        let hp = n - prev;
        prev = n;
        x[12000 + i] += hp * 0.6 * (-(i as f32) / 600.0).exp();
    }
    for i in 0..6000 {
        let t = i as f32 / SR;
        x[24000 + i] += (noise() * 0.5 + (t * 190.0 * TAU).sin() * 0.4) * (-t * 25.0).exp();
    }
    let notes = audio_to_midi(&mono_sample(x), MidiMode::Drums, 120.0);
    let got: Vec<(u32, u8)> = notes.iter().map(|n| (n.step, n.pitch)).collect();
    assert_eq!(got, vec![(0, 0), (2, 4), (4, 2)], "{notes:?}");
}

#[test]
fn harmony_to_midi_finds_chord() {
    let x: Vec<f32> = (0..SR as usize)
        .map(|i| {
            let t = i as f32 / SR;
            [60.0, 64.0, 67.0].iter().map(|m| (t * hz(*m) * TAU).sin() * 0.2).sum()
        })
        .collect();
    let notes = audio_to_midi(&mono_sample(x), MidiMode::Harmony, 120.0);
    let mut pcs: Vec<u8> = notes.iter().filter(|n| n.step == 0).map(|n| n.pitch % 12).collect();
    pcs.sort();
    assert_eq!(pcs, vec![0, 4, 7], "{notes:?}");
}

#[test]
fn stems_separate_and_sum_back() {
    let n = (SR * 2.0) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    let mut rng = 7u32;
    for i in 0..n {
        let t = i as f32 / SR;
        let bass = (t * 55.0 * TAU).sin() * 0.4;
        // Wide pad: different notes left and right.
        let pad_l = (t * 600.0 * TAU).sin() * 0.15;
        let pad_r = (t * 750.0 * TAU).sin() * 0.15;
        // Clicks every quarter second.
        let click = if i % 12000 < 200 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32 - 0.5) * (1.0 - (i % 12000) as f32 / 200.0)
        } else {
            0.0
        };
        l[i] = bass + pad_l + click;
        r[i] = bass + pad_r + click;
    }
    let s = Sample { left: l.clone(), right: r.clone(), sample_rate: SR };
    let st = split_stems(&s);
    let e = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
    let total: Vec<f32> = (0..n).map(|i| st.drums.0[i] + st.bass.0[i] + st.lead.0[i] + st.other.0[i]).collect();
    let err: f32 = total.iter().zip(&l).map(|(a, b)| (a - b) * (a - b)).sum::<f32>() / e(&l);
    assert!(err < 0.02, "stems must sum back to the mix (error {err})");
    // The bass stem carries most of the 55 Hz energy, the drums stem the clicks.
    assert!(e(&st.bass.0) > e(&st.lead.0) && e(&st.bass.0) > e(&st.other.0) * 0.8);
    let click_win = |x: &[f32]| e(&x[12000..12200]) / e(&x[18000..18200]).max(1e-9);
    assert!(click_win(&st.drums.0) > 5.0, "drum stem should be mostly clicks");
    assert!(e(&st.lead.0) < e(&st.other.0), "the wide pad is not centre material");
}

#[test]
fn video_pipeline_with_ffmpeg() {
    use crate::video;
    if !video::available() {
        eprintln!("ffmpeg not installed - skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("musicton_video_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=duration=2:size=320x180:rate=25", "-f", "lavfi", "-i", "sine=frequency=330:duration=2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&src)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let s = src.to_string_lossy().to_string();
    let (dur, audio) = video::probe(&s).unwrap();
    assert!((dur - 2.0).abs() < 0.2 && audio);
    let n = video::extract_frames(&s, &dir.join("frames")).unwrap();
    assert!((14..=18).contains(&n), "frames: {n}");
    let (w, h, rgba) = video::load_frame(&dir.join("frames").join("00001.png")).unwrap();
    assert_eq!((w, rgba.len()), (video::FRAME_WIDTH as usize, w * h * 4));
    let wav = dir.join("a.wav");
    video::extract_audio(&s, &wav).unwrap();
    let out = dir.join("out.mp4");
    video::mux(&s, &wav.to_string_lossy(), 0.5, &out.to_string_lossy()).unwrap();
    let (d2, a2) = video::probe(&out.to_string_lossy()).unwrap();
    assert!(a2 && d2 > 1.5, "muxed file: {d2} s, audio {a2}");
    let _ = std::fs::remove_dir_all(&dir);
}
