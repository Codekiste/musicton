//! Behaviour tests for the mastering, dynamics and performance effects.

use crate::audio::effects::{create, Ctx, Effect};
use crate::model::DeviceKind;
use crate::params;

const SR: f32 = 48000.0;

fn fx(kind: DeviceKind, set: &[(&str, f32)]) -> (Box<dyn Effect>, Vec<f32>) {
    let defs = kind.defs();
    let mut p = params::defaults(defs);
    for (name, v) in set {
        let i = defs.iter().position(|d| d.name == *name).unwrap_or_else(|| panic!("{} has no param {name}", kind.name()));
        p[i] = *v;
    }
    (create(kind, SR), p)
}

/// Runs a signal generator through the effect, returns (left, right).
fn run(kind: DeviceKind, set: &[(&str, f32)], secs: f32, playing: bool, gen: impl Fn(usize) -> (f32, f32)) -> (Vec<f32>, Vec<f32>, Box<dyn Effect>) {
    let (mut e, p) = fx(kind, set);
    let n = (SR * secs) as usize;
    let (mut ol, mut or) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for b in 0..n / 64 {
        let ctx = Ctx { sr: SR, bpm: 120.0, beat: (b * 64) as f64 / SR as f64 * 2.0, playing };
        e.set_params(&p, &ctx);
        let mut l = [0.0f32; 64];
        let mut r = [0.0f32; 64];
        for i in 0..64 {
            (l[i], r[i]) = gen(b * 64 + i);
        }
        e.process(&mut l, &mut r, &ctx);
        ol.extend_from_slice(&l);
        or.extend_from_slice(&r);
    }
    (ol, or, e)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}
fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}
fn sine(i: usize, f: f32, a: f32) -> f32 {
    (i as f32 / SR * f * std::f32::consts::TAU).sin() * a
}

#[test]
fn true_peak_limiter_holds_ceiling_and_meters_loudness() {
    // Loud near-Nyquist content produces inter-sample peaks above the sample peaks.
    let (l, r, e) = run(DeviceKind::TruePeakLimiter, &[("Input", 12.0), ("Ceiling", -1.0)], 4.0, true, |i| {
        let x = sine(i, 11025.0 * 0.999, 0.9) + sine(i, 100.0, 0.5);
        (x, x)
    });
    let ceil = 10f32.powf(-1.0 / 20.0);
    assert!(peak(&l[4800..]) <= ceil + 1e-4 && peak(&r[4800..]) <= ceil + 1e-4);
    let d = e.display().unwrap();
    assert!(d[3] < -0.5, "true peak above ceiling: {} dBTP", d[3]);
    assert!(d[1] > -20.0 && d[1] < 0.0, "short-term loudness implausible: {}", d[1]);
    assert!(d[2] > -20.0 && d[2] < 0.0, "integrated loudness implausible: {}", d[2]);
    assert!(e.reduction() > 1.0);
}

#[test]
fn loudness_of_reference_sine() {
    // A 1 kHz sine at -20 dBFS on both channels reads about -20 LUFS (K-weighting is ~0 dB
    // at 1 kHz and two channels add 3 dB to the -3 dB RMS of a sine).
    let a = 10f32.powf(-20.0 / 20.0);
    let (_, _, e) = run(DeviceKind::TruePeakLimiter, &[], 5.0, true, |i| (sine(i, 1000.0, a), sine(i, 1000.0, a)));
    let d = e.display().unwrap();
    assert!((d[2] + 20.0).abs() < 1.0, "expected ~-20 LUFS, got {}", d[2]);
}

#[test]
fn maximizer_raises_level_under_ceiling() {
    let gen = |i: usize| {
        let x = sine(i, 80.0, 0.3) + if i % 24000 < 400 { 0.6 } else { 0.0 };
        (x, x)
    };
    let (dry, _, _) = run(DeviceKind::Utility, &[], 2.0, true, gen);
    let (l, _, e) = run(DeviceKind::Maximizer, &[("Gain", 9.0), ("Ceiling", -0.5)], 2.0, true, gen);
    assert!(peak(&l) <= 10f32.powf(-0.5 / 20.0) + 1e-4);
    assert!(rms(&l) > rms(&dry) * 1.8, "maximizer did not raise loudness");
    let _ = e;
}

#[test]
fn transient_shaper_changes_attack() {
    // Decaying plucks every 250 ms.
    let gen = |i: usize| {
        let t = (i % 12000) as f32 / SR;
        let x = sine(i, 220.0, 0.5) * (-t * 12.0).exp();
        (x, x)
    };
    let crest = |x: &[f32]| peak(x) / rms(x);
    let (plain, _, _) = run(DeviceKind::TransientShaper, &[], 2.0, true, gen);
    let (punchy, _, _) = run(DeviceKind::TransientShaper, &[("Attack", 1.0)], 2.0, true, gen);
    let (soft, _, _) = run(DeviceKind::TransientShaper, &[("Attack", -1.0), ("Sustain", 1.0)], 2.0, true, gen);
    assert!(crest(&punchy) > crest(&plain) * 1.1, "attack boost: {} vs {}", crest(&punchy), crest(&plain));
    assert!(crest(&soft) < crest(&plain) * 0.9, "softening: {} vs {}", crest(&soft), crest(&plain));
}

#[test]
fn imager_width_controls_side_signal() {
    let gen = |i: usize| (sine(i, 2000.0, 0.4), sine(i, 2000.0, 0.4) * 0.2 + sine(i, 2300.0, 0.3));
    let side = |l: &[f32], r: &[f32]| rms(&l.iter().zip(r).map(|(a, b)| a - b).collect::<Vec<_>>()[4800..]);
    let (l0, r0, _) = run(DeviceKind::StereoImager, &[("Mid Width", 0.0), ("High Width", 0.0), ("Low Width", 0.0)], 1.0, true, gen);
    let (l1, r1, _) = run(DeviceKind::StereoImager, &[("Mid Width", 1.0), ("High Width", 1.0), ("Low Width", 1.0)], 1.0, true, gen);
    let (l2, r2, e) = run(DeviceKind::StereoImager, &[("Mid Width", 2.0), ("High Width", 2.0)], 1.0, true, gen);
    assert!(side(&l0, &r0) < 0.01, "width 0 must be mono");
    assert!(side(&l2, &r2) > side(&l1, &r1) * 1.5);
    assert!(e.display().unwrap()[0] < 1.0);
}

#[test]
fn beat_mangler_gates_only_while_playing() {
    let gen = |i: usize| (sine(i, 300.0, 0.5), sine(i, 300.0, 0.5));
    let (stopped, _, _) = run(DeviceKind::BeatMangler, &[("Pattern", 0.0)], 1.0, false, gen);
    let (playing, _, _) = run(DeviceKind::BeatMangler, &[("Pattern", 0.0)], 1.0, true, gen);
    assert!((rms(&stopped) - 0.5 / 2f32.sqrt()).abs() < 0.02, "must pass audio when stopped");
    // Every second 16th is muted.
    assert!(rms(&playing) < rms(&stopped) * 0.8);
    // Tape stop and reverse patterns stay finite and audible.
    for pat in 1..8 {
        let (x, _, _) = run(DeviceKind::BeatMangler, &[("Pattern", pat as f32)], 2.0, true, gen);
        assert!(x.iter().all(|v| v.is_finite()) && rms(&x) > 0.05, "pattern {pat}");
    }
}

#[test]
fn hybrid_reverb_tail_and_shimmer() {
    let imp = |i: usize| if i < 64 { (0.8, 0.8) } else { (0.0, 0.0) };
    let (l, r, _) = run(DeviceKind::HybridReverb, &[("Dry/Wet", 1.0), ("Decay", 3.0)], 3.0, true, imp);
    let tail = rms(&l[48000..72000]);
    assert!(tail > 1e-4, "no reverb tail");
    assert!(rms(&l[130000..]) < tail, "tail must decay");
    // Decorrelated stereo.
    let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 1.0);
    let (s, _, _) = run(DeviceKind::HybridReverb, &[("Dry/Wet", 1.0), ("Shimmer", 0.8), ("Decay", 3.0)], 3.0, true, |i| (sine(i, 440.0, 0.3), sine(i, 440.0, 0.3)));
    assert!(s.iter().all(|v| v.is_finite()) && peak(&s) < 4.0, "shimmer unstable");
}

#[test]
fn heat_stack_adds_harmonics() {
    let gen = |i: usize| (sine(i, 200.0, 0.5), sine(i, 200.0, 0.5));
    let (y, _, _) = run(DeviceKind::HeatStack, &[("Drive", 24.0), ("Tone", 20000.0)], 1.0, true, gen);
    // Energy at 3x the fundamental (correlation with a 600 Hz sine).
    let h3 = |x: &[f32]| (x.iter().enumerate().skip(4800).map(|(i, v)| v * sine(i, 600.0, 1.0)).sum::<f32>() / x.len() as f32).abs();
    let (clean, _, _) = run(DeviceKind::Utility, &[], 1.0, true, gen);
    assert!(h3(&y) > h3(&clean) * 10.0 + 1e-3, "no harmonics added");
}

/// Frequency from positive zero crossings.
fn zc(x: &[f32]) -> f32 {
    let c = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    c as f32 / (x.len() as f32 / SR)
}

#[test]
fn pitch_fix_snaps_to_scale() {
    // 452 Hz is between A4 (440) and A#4; in A minor it must land on A4.
    let gen = |i: usize| (sine(i, 452.0, 0.5), sine(i, 452.0, 0.5));
    let (l, _, e) = run(DeviceKind::PitchFix, &[("Key", 9.0), ("Scale", 1.0), ("Speed", 0.0)], 2.0, true, gen);
    let f = zc(&l[48000..]);
    assert!((f - 440.0).abs() < 6.0, "expected ~440 Hz, got {f}");
    assert!((e.display().unwrap()[0] - 452.0).abs() < 8.0, "detector off: {:?}", e.display());
    // Amount 0 leaves it alone.
    let (l, _, _) = run(DeviceKind::PitchFix, &[("Amount", 0.0)], 1.0, true, gen);
    assert!((zc(&l[24000..]) - 452.0).abs() < 6.0);
}

#[test]
fn space_panner_places_sound() {
    let gen = |i: usize| {
        let x = sine(i, 700.0, 0.4) + sine(i, 5000.0, 0.2);
        (x, x)
    };
    let (l, r, _) = run(DeviceKind::SpacePanner, &[("Azimuth", 90.0), ("Room", 0.0)], 1.0, true, gen);
    assert!(rms(&r[4800..]) > rms(&l[4800..]) * 1.5, "source at +90 must be louder on the right");
    let (l, r, _) = run(DeviceKind::SpacePanner, &[("Azimuth", -90.0), ("Room", 0.0)], 1.0, true, gen);
    assert!(rms(&l[4800..]) > rms(&r[4800..]) * 1.5);
    let (near, _, _) = run(DeviceKind::SpacePanner, &[("Distance", 1.0)], 1.0, true, gen);
    let (far, _, _) = run(DeviceKind::SpacePanner, &[("Distance", 8.0)], 1.0, true, gen);
    assert!(rms(&far[4800..]) < rms(&near[4800..]) * 0.5, "distance must attenuate");
    let (l, r, _) = run(DeviceKind::SpacePanner, &[("Orbit", 180.0)], 3.0, true, gen);
    assert!(l.iter().chain(&r).all(|v| v.is_finite()));
}
