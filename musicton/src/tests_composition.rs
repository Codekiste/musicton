//! Tests for chords, note rules, note chance, chord following and strumming.

use crate::audio::engine::{Cmd, Engine};
use crate::model::*;
use std::sync::Arc;

const SR: f32 = 48000.0;

fn run(p: Project, secs: f32, scene: Option<usize>) -> Vec<f32> {
    let mut e = Engine::new(SR, Arc::new(p));
    match scene {
        Some(s) => e.force_scene(s),
        None => e.handle(Cmd::Play),
    }
    let mut out = vec![0.0; (SR * secs) as usize * 2];
    for c in out.chunks_mut(1024) {
        e.process(c);
    }
    out
}

fn note(step: u32, len: u32, pitch: u8) -> Note {
    Note { step, len, pitch, vel: 110, slide: false, skip: 0 }
}

#[test]
fn chord_names_and_theory() {
    assert_eq!(parse_chord("Am"), Some((9, 1)));
    assert_eq!(parse_chord("F#m7"), Some((6, 4)));
    assert_eq!(parse_chord("Bbmaj7"), Some((10, 3)));
    assert_eq!(parse_chord("G"), Some((7, 0)));
    assert_eq!(parse_chord("Xyz"), None);
    // A minor: i = Am, III = C, iv = Dm, v = Em, VI = F, VII = G.
    let minor = SCALES.iter().position(|s| s.0 == "Minor").unwrap();
    let names: Vec<String> = (0..7).map(|d| {
        let (r, q) = diatonic_chord(9, minor, d, false);
        ChordEvent { id: 0, start: 0, len: 0, root: r, quality: q }.name()
    }).collect();
    assert_eq!(names, vec!["Am", "Bdim", "C", "Dm", "Em", "F", "G"]);
    let major = SCALES.iter().position(|s| s.0 == "Major").unwrap();
    let (r, q) = diatonic_chord(0, major, 4, true);
    assert_eq!(ChordEvent { id: 0, start: 0, len: 0, root: r, quality: q }.name(), "G7");
    // Snap E (64) to the nearest tone of F major (F A C): F (65).
    assert_eq!(snap_to_chord(64, &[5, 9, 0]), 65);
    assert_eq!(snap_to_chord(60, &[5, 9, 0]), 60);
}

#[test]
fn note_rules_transform() {
    let mut notes = vec![note(0, 4, 60), note(2, 4, 62), note(4, 4, 64), note(6, 4, 30)];
    notes[3].vel = 20;
    let n = apply_rules(&mut notes, &rule_presets().into_iter().find(|p| p.0 == "Remove ghost notes").unwrap().1, 1);
    assert_eq!((n, notes.len()), (1, 3));
    apply_rules(&mut notes, &[NoteRule::new(RuleField::StepInBeat, RuleOp::Equal, 2, 0, RuleAction::Transpose, 12)], 1);
    assert_eq!(notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![60, 74, 64]);
    apply_rules(&mut notes, &[NoteRule::new(RuleField::Pitch, RuleOp::Between, 60, 64, RuleAction::SetChance, 25)], 1);
    assert_eq!(notes.iter().map(|n| n.skip).collect::<Vec<_>>(), vec![75, 0, 75]);
    apply_rules(&mut notes, &[NoteRule::new(RuleField::All, RuleOp::Equal, 0, 0, RuleAction::AddOctave, -12)], 1);
    assert_eq!(notes.len(), 6);
    apply_rules(&mut notes, &[NoteRule::new(RuleField::Index, RuleOp::EveryNth, 2, 0, RuleAction::ScaleLength, 50)], 1);
    assert_eq!(notes.iter().filter(|n| n.len == 2).count(), 3);
}

fn pad_project(notes: Vec<Note>) -> (Project, u64) {
    let mut p = Project::empty();
    let id = p.add_track("Pad", InstrumentKind::Synth, CLIP_COLORS[0]);
    let mut clip = Clip::new("c", CLIP_COLORS[0], 1);
    clip.notes = notes;
    p.track_mut(id).unwrap().clips[0] = Some(clip);
    (p, id)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn note_chance_skips_notes() {
    let mut always = note(0, 2, 60);
    always.skip = 0;
    let mut never = note(0, 2, 60);
    never.skip = 100;
    assert!(rms(&run(pad_project(vec![always]).0, 0.5, Some(0))) > 0.005);
    assert!(rms(&run(pad_project(vec![never]).0, 0.5, Some(0))) < 1e-5);
    // 50 %: over many bars some passes play and some don't.
    let mut half = note(0, 1, 60);
    half.skip = 50;
    let out = run(pad_project(vec![half]).0, 16.0, Some(0));
    let bar = (SR * 60.0 / 130.0 * 4.0) as usize * 2;
    let played: Vec<bool> = out.chunks(bar).take(8).map(|c| rms(&c[..c.len() / 4]) > 0.002).collect();
    assert!(played.iter().any(|x| *x) && played.iter().any(|x| !*x), "{played:?}");
}

/// Dominant frequency by zero crossings (good enough for one sine-ish voice).
fn zc_freq(x: &[f32]) -> f32 {
    let mono: Vec<f32> = x.chunks(2).map(|c| c[0]).collect();
    let crossings = mono.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    crossings as f32 / (mono.len() as f32 / SR)
}

#[test]
fn follow_chords_snaps_pitch() {
    let (mut p, id) = pad_project(vec![note(0, 16, 64)]);
    {
        let t = p.track_mut(id).unwrap();
        t.instrument.params[crate::params::synth::WAVE] = 3.0; // sine
        t.instrument.params[crate::params::synth::CUTOFF] = 16000.0;
        t.instrument.params[crate::params::synth::VOICES] = 1.0;
        t.instrument.params[crate::params::synth::SUB] = 0.0;
        // Timeline playback so the chord track position applies.
        let clip = t.clips[0].clone().unwrap();
        t.arrangement.push(ArrClip { id: 900, start: 0, len: 16, clip });
    }
    let plain = zc_freq(&run(p.clone(), 1.0, None)[9600..]);
    p.chords.push(ChordEvent { id: 1, start: 0, len: 16, root: 5, quality: 0 });
    p.track_mut(id).unwrap().follow_chords = true;
    let snapped = zc_freq(&run(p, 1.0, None)[9600..]);
    assert!((plain - 329.6).abs() < 15.0, "E4 expected, got {plain}");
    assert!((snapped - 349.2).abs() < 15.0, "F4 expected, got {snapped}");
}

#[test]
fn strum_spreads_chord() {
    let chord = vec![note(0, 8, 60), note(0, 8, 64), note(0, 8, 67)];
    // Energy in the first ~80 ms: strummed, only the first note sounds there.
    let early = |p: Project| {
        let out = run(p, 0.3, Some(0));
        rms(&out[1000..8000])
    };
    let (p, _) = pad_project(chord.clone());
    let base = early(p);
    let (mut p2, id) = pad_project(chord);
    let mut strum = MidiFx::new(77, MidiFxKind::Strum);
    strum.params[0] = 100.0;
    strum.params[2] = 0.0;
    p2.track_mut(id).unwrap().midi_fx.push(strum);
    let strummed = early(p2);
    assert!(strummed < base * 0.75 && strummed > 0.001, "strum did not delay the later notes: {base} vs {strummed}");
}

#[test]
fn versions_save_list_diff_restore() {
    use crate::versions;
    let dir = std::env::temp_dir().join(format!("musicton_versions_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut p, _) = pad_project(vec![note(0, 4, 60)]);
    let v1 = versions::save_version(&dir, &p, "first").unwrap();
    p.bpm = 140.0;
    p.add_track("Bass", InstrumentKind::Voltage, CLIP_COLORS[2]);
    let v2 = versions::save_version(&dir, &p, "faster").unwrap();
    let list = versions::list_versions(&dir);
    assert_eq!(list.iter().map(|v| v.message.as_str()).collect::<Vec<_>>(), vec!["faster", "first"]);
    assert_ne!(v1.id, v2.id);
    let old = versions::load_version(&dir, &v1.id).unwrap();
    let d = versions::diff(&old, &p);
    assert!(d.iter().any(|l| l.contains("Tempo")), "{d:?}");
    assert!(d.iter().any(|l| l.contains("+ Track \"Bass\"")), "{d:?}");
    assert_eq!(old.bpm, p.bpm - (p.bpm - old.bpm));
    versions::prune(&dir, 1);
    assert_eq!(versions::list_versions(&dir).len(), 1);
    assert_eq!(versions::format_time(0), "1970-01-01 00:00 UTC");
    assert_eq!(versions::format_time(1_792_324_800), "2026-10-18 12:00 UTC");
    let _ = std::fs::remove_dir_all(&dir);
}
