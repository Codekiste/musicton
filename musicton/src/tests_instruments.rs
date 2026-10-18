//! Tests for zone mapping, slicing, Beat Pads and the new synths.

use crate::audio::instruments::{create, InstrumentDsp};
use crate::audio::samplers::{detect_slices, slice_notes};
use crate::audio::samples::{self, Sample};
use crate::model::*;

const SR: f32 = 48000.0;

fn render(dsp: &mut dyn InstrumentDsp, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0; frames];
    let mut r = vec![0.0; frames];
    for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
        dsp.render(a, b);
    }
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn write_wav(name: &str, data: &[f32]) -> String {
    let path = std::env::temp_dir().join(name);
    let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for v in data {
        w.write_sample((v.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    let s = path.to_string_lossy().to_string();
    samples::load(&s).unwrap();
    s
}

#[test]
fn note_names_in_files() {
    assert_eq!(note_in_name("Piano_C3"), Some(60));
    assert_eq!(note_in_name("strings F#2 soft"), Some(54));
    assert_eq!(note_in_name("Pad-Db4"), Some(73));
    assert_eq!(note_in_name("bass_060"), Some(60));
    assert_eq!(note_in_name("Kick"), None);
    let z = zones_from_files(&["x/a_C3.wav".into(), "x/a_C4.wav".into(), "x/a_G3.wav".into()]);
    assert_eq!(z.iter().map(|z| z.root).collect::<Vec<_>>(), vec![60, 67, 72]);
    assert_eq!((z[0].lo, z[0].hi), (0, 63));
    assert_eq!((z[1].lo, z[1].hi), (64, 69));
    assert_eq!((z[2].lo, z[2].hi), (70, 127));
}

#[test]
fn transient_slicing_finds_hits() {
    // Four decaying hits half a second apart.
    let n = 96000;
    let mut left = vec![0.0f32; n];
    for k in 0..4 {
        for i in 0..8000 {
            let t = i as f32 / SR;
            left[k * 24000 + i] = (t * 180.0 * std::f32::consts::TAU).sin() * (-t * 30.0).exp() * 0.8;
        }
    }
    let s = Sample { right: left.clone(), left, sample_rate: SR };
    let sl = detect_slices(&s, 0, 0.5);
    assert_eq!(sl.len(), 4, "slices: {sl:?}");
    for (k, &f) in sl.iter().enumerate() {
        assert!((f as i64 - (k * 24000) as i64).abs() < 600, "slice {k} at {f}");
    }
    assert_eq!(detect_slices(&s, 2, 0.5).len(), 16);
    let notes = slice_notes(&sl, n, 16);
    assert_eq!(notes.iter().map(|n| (n.step, n.pitch)).collect::<Vec<_>>(), vec![(0, 36), (4, 37), (8, 38), (12, 39)]);
}

#[test]
fn slicer_plays_each_slice() {
    let tone: Vec<f32> = (0..48000).map(|i| (i as f32 / SR * 220.0 * std::f32::consts::TAU).sin() * 0.5).collect();
    let path = write_wav("musicton_slicer.wav", &tone);
    let mut inst = Instrument::new(InstrumentKind::Slicer);
    inst.samples[0] = Some(path);
    inst.params[crate::params::slicer::MODE] = 1.0;
    inst.params[crate::params::slicer::SYNC] = 0.0;
    let mut d = create(InstrumentKind::Slicer, SR);
    d.set_params(&inst.params, &inst.samples);
    d.note_on(36 + 7, 100, None, false);
    assert!(rms(&render(d.as_mut(), 4000).0) > 0.05);
    d.note_on(36 + 8, 100, None, false);
    // Only 8 slices: key 44 plays nothing new.
    let (l, _) = render(d.as_mut(), 9000);
    assert!(rms(&l[6000..]) < 0.01);
}

#[test]
fn multi_sampler_round_robin_and_velocity() {
    let a = write_wav("musicton_rr_a.wav", &vec![0.5; 24000]);
    let b = write_wav("musicton_rr_b.wav", &vec![-0.25; 24000]);
    let mut inst = Instrument::new(InstrumentKind::MultiSampler);
    inst.zones = vec![Zone { lo: 0, hi: 127, ..Zone::new(&a, 60) }, Zone { lo: 0, hi: 127, ..Zone::new(&b, 60) }];
    inst.params[crate::params::multi::ROUND_ROBIN] = 1.0;
    let mut d = create(InstrumentKind::MultiSampler, SR);
    d.set_params(&inst.params, &[]);
    d.set_extra(&inst);
    let mut signs = vec![];
    for _ in 0..4 {
        d.all_off();
        render(d.as_mut(), 48000);
        d.note_on(60, 127, None, false);
        let (l, _) = render(d.as_mut(), 2000);
        signs.push(l[1500] > 0.0);
    }
    assert_eq!(signs, vec![false, true, false, true], "round robin must alternate");
    // Velocity layers: a soft zone and a loud zone.
    inst.zones[0].vhi = 63;
    inst.zones[1].vlo = 64;
    inst.params[crate::params::multi::ROUND_ROBIN] = 0.0;
    d.set_params(&inst.params, &[]);
    d.set_extra(&inst);
    d.all_off();
    render(d.as_mut(), 48000);
    d.note_on(60, 30, None, false);
    assert!(render(d.as_mut(), 2000).0[1500] > 0.0, "soft hit plays zone A");
}

#[test]
fn beat_pads_choke_and_pad_fx() {
    let tone: Vec<f32> = (0..96000).map(|i| (i as f32 / SR * 300.0 * std::f32::consts::TAU).sin() * 0.5).collect();
    let path = write_wav("musicton_pad.wav", &tone);
    let mut inst = Instrument::new(InstrumentKind::BeatPads);
    for k in 0..2 {
        inst.pads[k].sample = Some(path.clone());
        inst.pads[k].choke = 1;
        inst.pads[k].decay = 8.0;
    }
    inst.pads[1].gain = -60.0;
    let mut d = create(InstrumentKind::BeatPads, SR);
    d.set_params(&inst.params, &[]);
    d.set_extra(&inst);
    d.note_on(0, 127, None, false);
    let before = rms(&render(d.as_mut(), 4800).0);
    d.note_on(1, 127, None, false);
    let (l, _) = render(d.as_mut(), 9600);
    assert!(before > 0.1);
    assert!(rms(&l[4800..]) < before * 0.05, "pad 2 must choke pad 1");
    // Pad effects: a Bitcrusher on pad 3 changes its sound.
    inst.pads[2].sample = Some(path);
    let plain = {
        d.set_extra(&inst);
        d.note_on(2, 127, None, false);
        render(d.as_mut(), 4800).0
    };
    let mut crush = Device::new(1, DeviceKind::Bitcrusher);
    let defs = crush.kind.defs();
    for (i, def) in defs.iter().enumerate() {
        crush.params[i] = if def.name.contains("Mix") || def.name.contains("Wet") { def.max } else { def.min };
    }
    inst.pads[2].devices.push(crush);
    d.set_extra(&inst);
    d.note_on(2, 127, None, false);
    let fx = render(d.as_mut(), 4800).0;
    let diff: f32 = plain.iter().zip(&fx).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 1.0, "pad effect had no effect");
}

#[test]
fn synths_are_polyphonic_and_release() {
    for kind in [InstrumentKind::Voltage, InstrumentKind::MatrixFm, InstrumentKind::GrainCloud] {
        let inst = Instrument::new(kind);
        let mut d = create(kind, SR);
        d.set_params(&inst.params, &inst.samples);
        d.note_on(60, 110, None, false);
        let one = rms(&render(d.as_mut(), 48000).0[24000..]);
        d.note_on(64, 110, None, false);
        d.note_on(67, 110, None, false);
        let three = rms(&render(d.as_mut(), 48000).0[24000..]);
        assert!(one > 0.005, "{} silent", kind.name());
        assert!(three > one * 1.2, "{} not polyphonic: {one} vs {three}", kind.name());
        d.all_off();
        let (l, r) = render(d.as_mut(), 48000 * 14);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        assert!(rms(&l[l.len() - 4800..]) < 1e-4, "{} never releases", kind.name());
    }
}
