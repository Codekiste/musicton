//! Offline audio analysis: audio-to-MIDI (drums, melody, harmony) and a DSP stem splitter
//! (harmonic/percussive separation, low-band bass, centre extraction for the lead/vocal stem).

use super::dsp::fft;
use super::samples::Sample;
use crate::model::Note;

const N: usize = 2048;
const HOP: usize = 512;

fn hann() -> Vec<f32> {
    (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect()
}

/// Short-time Fourier transform: frames x (N/2 + 1) complex bins.
fn stft(x: &[f32], win: &[f32]) -> Vec<Vec<(f32, f32)>> {
    let frames = x.len().div_ceil(HOP) + 1;
    let mut out = Vec::with_capacity(frames);
    let mut re = vec![0.0f32; N];
    let mut im = vec![0.0f32; N];
    for f in 0..frames {
        let start = f as isize * HOP as isize - (N / 2) as isize;
        for i in 0..N {
            let idx = start + i as isize;
            re[i] = if idx >= 0 && (idx as usize) < x.len() { x[idx as usize] * win[i] } else { 0.0 };
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        out.push((0..=N / 2).map(|k| (re[k], im[k])).collect());
    }
    out
}

/// Inverse STFT by overlap-add (Hann analysis and synthesis, hop N/4).
fn istft(spec: &[Vec<(f32, f32)>], len: usize, win: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0f32; len + N];
    let mut norm = vec![0.0f32; len + N];
    let mut re = vec![0.0f32; N];
    let mut im = vec![0.0f32; N];
    for (f, frame) in spec.iter().enumerate() {
        for k in 0..=N / 2 {
            re[k] = frame[k].0;
            im[k] = -frame[k].1;
            if k > 0 && k < N / 2 {
                re[N - k] = frame[k].0;
                im[N - k] = frame[k].1;
            }
        }
        fft(&mut re, &mut im);
        let start = f as isize * HOP as isize - (N / 2) as isize;
        for i in 0..N {
            let idx = start + i as isize;
            if idx >= 0 && (idx as usize) < out.len() {
                out[idx as usize] += re[i] / N as f32 * win[i];
                norm[idx as usize] += win[i] * win[i];
            }
        }
    }
    out.truncate(len);
    for (o, n) in out.iter_mut().zip(norm) {
        if n > 1e-6 {
            *o /= n;
        }
    }
    out
}

fn median(v: &mut [f32]) -> f32 {
    let m = v.len() / 2;
    *v.select_nth_unstable_by(m, |a, b| a.total_cmp(b)).1
}

pub struct Stems {
    pub drums: (Vec<f32>, Vec<f32>),
    pub bass: (Vec<f32>, Vec<f32>),
    pub lead: (Vec<f32>, Vec<f32>),
    pub other: (Vec<f32>, Vec<f32>),
}

/// Splits a stereo mix into drums, bass, lead (centre, mid-high harmonic) and other.
/// Pure DSP - no trained model - so expect a useful remix tool, not studio-clean stems.
pub fn split_stems(s: &Sample) -> Stems {
    let win = hann();
    let sl = stft(&s.left, &win);
    let sr_ = stft(&s.right, &win);
    let frames = sl.len();
    let bins = N / 2 + 1;
    let mag: Vec<Vec<f32>> = (0..frames)
        .map(|f| (0..bins).map(|k| ((sl[f][k].0.powi(2) + sl[f][k].1.powi(2)) + (sr_[f][k].0.powi(2) + sr_[f][k].1.powi(2))).sqrt()).collect())
        .collect();
    // Median filters: across time (harmonic = steady) and across frequency (percussive = broadband).
    let (ht, pf) = (17usize, 17usize);
    let mut harm = vec![vec![0.0f32; bins]; frames];
    let mut perc = vec![vec![0.0f32; bins]; frames];
    let mut buf = Vec::with_capacity(ht.max(pf));
    for f in 0..frames {
        for k in 0..bins {
            buf.clear();
            for t in f.saturating_sub(ht / 2)..(f + ht / 2 + 1).min(frames) {
                buf.push(mag[t][k]);
            }
            harm[f][k] = median(&mut buf);
            buf.clear();
            for q in k.saturating_sub(pf / 2)..(k + pf / 2 + 1).min(bins) {
                buf.push(mag[f][q]);
            }
            perc[f][k] = median(&mut buf);
        }
    }
    let bin_hz = s.sample_rate / N as f32;
    let mk = || vec![vec![(0.0f32, 0.0f32); bins]; frames];
    let (mut dl, mut dr, mut bl, mut br, mut ll, mut lr, mut ol, mut or) = (mk(), mk(), mk(), mk(), mk(), mk(), mk(), mk());
    for f in 0..frames {
        for k in 0..bins {
            let (h, p) = (harm[f][k].powi(2), perc[f][k].powi(2));
            let mp = p / (h + p + 1e-12);
            let mh = 1.0 - mp;
            let hz = k as f32 * bin_hz;
            let (a, b) = (sl[f][k], sr_[f][k]);
            // Centre-ness: 1 when left and right agree in level and phase.
            let la = (a.0 * a.0 + a.1 * a.1).sqrt();
            let lb = (b.0 * b.0 + b.1 * b.1).sqrt();
            let dot = a.0 * b.0 + a.1 * b.1;
            let centre = (2.0 * dot / (la * la + lb * lb + 1e-12)).max(0.0).powi(2);
            let bass_w = if hz < 120.0 { 1.0 } else if hz < 200.0 { (200.0 - hz) / 80.0 } else { 0.0 };
            let lead_w = if (220.0..7000.0).contains(&hz) { centre } else { 0.0 } * (1.0 - bass_w);
            let other_w = (1.0 - bass_w - lead_w).max(0.0);
            let scale = |c: (f32, f32), w: f32| (c.0 * w, c.1 * w);
            dl[f][k] = scale(a, mp);
            dr[f][k] = scale(b, mp);
            bl[f][k] = scale(a, mh * bass_w);
            br[f][k] = scale(b, mh * bass_w);
            ll[f][k] = scale(a, mh * lead_w);
            lr[f][k] = scale(b, mh * lead_w);
            ol[f][k] = scale(a, mh * other_w);
            or[f][k] = scale(b, mh * other_w);
        }
    }
    let len = s.left.len();
    let back = |x: &[Vec<(f32, f32)>]| istft(x, len, &win);
    Stems { drums: (back(&dl), back(&dr)), bass: (back(&bl), back(&br)), lead: (back(&ll), back(&lr)), other: (back(&ol), back(&or)) }
}

// ---------------------------------------------------------------------------------------------
// Audio to MIDI

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MidiMode {
    Drums,
    Melody,
    Harmony,
}

fn secs_to_step(t: f32, bpm: f32) -> u32 {
    (t * bpm / 60.0 * 4.0).round().max(0.0) as u32
}

/// Converts audio to notes. `bpm` is the tempo of the material (steps are 16ths of it).
pub fn audio_to_midi(s: &Sample, mode: MidiMode, bpm: f32) -> Vec<Note> {
    match mode {
        MidiMode::Drums => drums_to_midi(s, bpm),
        MidiMode::Melody => melody_to_midi(s, bpm),
        MidiMode::Harmony => harmony_to_midi(s, bpm),
    }
}

fn mono(s: &Sample) -> Vec<f32> {
    s.left.iter().zip(&s.right).map(|(a, b)| (a + b) * 0.5).collect()
}

/// Onsets, each classified as kick (lane 0), snare (lane 2) or hi-hat (lane 4) by its spectrum.
fn drums_to_midi(s: &Sample, bpm: f32) -> Vec<Note> {
    let x = mono(s);
    let onsets = super::samplers::detect_slices(s, 0, 0.6);
    let win = hann();
    let bin_hz = s.sample_rate / N as f32;
    let mut out: Vec<Note> = vec![];
    for &o in &onsets {
        let mut re: Vec<f32> = (0..N).map(|i| x.get(o + i).copied().unwrap_or(0.0) * win[i]).collect();
        let mut im = vec![0.0f32; N];
        if re.iter().all(|v| v.abs() < 1e-5) {
            continue;
        }
        fft(&mut re, &mut im);
        let (mut low, mut mid, mut high) = (0.0f32, 0.0f32, 0.0f32);
        for k in 1..N / 2 {
            let e = re[k] * re[k] + im[k] * im[k];
            let hz = k as f32 * bin_hz;
            if hz < 150.0 {
                low += e;
            } else if hz < 4000.0 {
                mid += e;
            } else {
                high += e;
            }
        }
        let tot = low + mid + high + 1e-12;
        let lane = if low / tot > 0.45 {
            0
        } else if high / tot > 0.4 {
            4
        } else {
            2
        };
        let peak = (0..N.min(x.len().saturating_sub(o))).map(|i| x[o + i].abs()).fold(0.0f32, f32::max);
        let step = secs_to_step(o as f32 / s.sample_rate, bpm);
        if out.iter().any(|n| n.step == step && n.pitch == lane) {
            continue;
        }
        out.push(Note { step, len: 1, pitch: lane, vel: (60.0 + peak * 80.0).clamp(30.0, 127.0) as u8, slide: false, skip: 0 });
    }
    out
}

/// Monophonic pitch tracking (YIN on 2048-sample frames, 10 ms hop) and note segmentation.
pub fn pitch_track(s: &Sample) -> Vec<(f32, Option<f32>, f32)> {
    let x = mono(s);
    let sr = s.sample_rate;
    let hop = (sr * 0.01) as usize;
    let w = 1024usize;
    let max_lag = ((sr / 55.0) as usize).min(w - 2);
    let min_lag = (sr / 1200.0) as usize;
    let mut out = vec![];
    let mut cm = vec![1.0f32; max_lag + 2];
    let mut pos = 0;
    while pos + w + max_lag + 2 < x.len() {
        let frame = &x[pos..pos + w + max_lag + 2];
        let rms = (frame[..w].iter().map(|v| v * v).sum::<f32>() / w as f32).sqrt();
        let mut f0 = None;
        if rms > 0.01 {
            let mut running = 0.0;
            for lag in 1..=max_lag + 1 {
                let mut d = 0.0;
                for i in (0..w).step_by(2) {
                    let diff = frame[i] - frame[i + lag];
                    d += diff * diff;
                }
                running += d;
                cm[lag] = d * lag as f32 / running.max(1e-9);
            }
            if let Some(lag) = (min_lag.max(2)..=max_lag).find(|&l| cm[l] < 0.15 && cm[l] <= cm[l + 1]) {
                let (a, b, c) = (cm[lag - 1], cm[lag], cm[lag + 1]);
                let den = a - 2.0 * b + c;
                let frac = if den.abs() > 1e-9 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
                f0 = Some(sr / (lag as f32 + frac));
            }
        }
        out.push((pos as f32 / sr, f0, rms));
        pos += hop;
    }
    out
}

fn melody_to_midi(s: &Sample, bpm: f32) -> Vec<Note> {
    let track = pitch_track(s);
    let mut notes: Vec<Note> = vec![];
    let mut cur: Option<(u8, f32, f32, f32)> = None; // pitch, start, last, peak rms
    let close = |cur: Option<(u8, f32, f32, f32)>, notes: &mut Vec<Note>| {
        if let Some((p, t0, t1, peak)) = cur {
            if t1 - t0 >= 0.04 {
                let step = secs_to_step(t0, bpm);
                let end = secs_to_step(t1 + 0.01, bpm).max(step + 1);
                notes.push(Note { step, len: end - step, pitch: p, vel: (50.0 + peak * 200.0).clamp(40.0, 127.0) as u8, slide: false, skip: 0 });
            }
        }
    };
    for &(t, f0, rms) in &track {
        let midi = f0.map(|f| (69.0 + 12.0 * (f / 440.0).log2()).round().clamp(0.0, 127.0) as u8);
        match (midi, cur) {
            (Some(m), Some((p, t0, _, pk))) if m == p => cur = Some((p, t0, t, pk.max(rms))),
            (Some(m), _) => {
                close(cur, &mut notes);
                cur = Some((m, t, t, rms));
            }
            (None, _) => {
                close(cur, &mut notes);
                cur = None;
            }
        }
    }
    close(cur, &mut notes);
    // Merge notes that land on the same step (pitch wobble).
    notes.dedup_by(|b, a| a.step == b.step);
    notes
}

/// Chords per beat from a chroma vector (strongest pitch classes).
fn harmony_to_midi(s: &Sample, bpm: f32) -> Vec<Note> {
    let x = mono(s);
    let sr = s.sample_rate;
    let beat = (60.0 / bpm * sr) as usize;
    let fft_n = 8192usize;
    let win: Vec<f32> = (0..fft_n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / fft_n as f32).cos()).collect();
    let mut notes: Vec<Note> = vec![];
    let mut prev: Vec<u8> = vec![];
    let mut b = 0;
    while b * beat < x.len() {
        let start = b * beat;
        let mut re: Vec<f32> = (0..fft_n).map(|i| x.get(start + i).copied().unwrap_or(0.0) * win[i]).collect();
        let mut im = vec![0.0f32; fft_n];
        fft(&mut re, &mut im);
        let mut chroma = [0.0f32; 12];
        for k in 1..fft_n / 2 {
            let hz = k as f32 * sr / fft_n as f32;
            if !(60.0..2000.0).contains(&hz) {
                continue;
            }
            let midi = 69.0 + 12.0 * (hz / 440.0).log2();
            let pc = (midi.round() as i32).rem_euclid(12) as usize;
            chroma[pc] += (re[k] * re[k] + im[k] * im[k]).sqrt();
        }
        let max = chroma.iter().cloned().fold(0.0, f32::max);
        let mut pcs: Vec<(usize, f32)> = chroma.iter().cloned().enumerate().filter(|(_, v)| max > 1e-3 && *v > max * 0.5).collect();
        pcs.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut chord: Vec<u8> = pcs.iter().take(4).map(|(pc, _)| 48 + *pc as u8).collect();
        chord.sort();
        let step = secs_to_step(start as f32 / sr, bpm);
        if !chord.is_empty() {
            if chord == prev {
                // Extend the previous chord.
                for n in notes.iter_mut().filter(|n| n.step + n.len == step && chord.contains(&n.pitch)) {
                    n.len += 4;
                }
            } else {
                for p in &chord {
                    notes.push(Note { step, len: 4, pitch: *p, vel: 90, slide: false, skip: 0 });
                }
            }
        }
        prev = chord;
        b += 1;
    }
    notes
}
