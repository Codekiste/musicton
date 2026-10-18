//! Engine tests for racks, routing, sidechain, modulators, playlist and the section chain.

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
    for chunk in out.chunks_mut(1024) {
        e.process(chunk);
    }
    assert!(out.iter().all(|s| s.is_finite()));
    out
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// A synth track holding one long note in scene 0.
fn pad(p: &mut Project) -> u64 {
    let id = p.add_track("Pad", InstrumentKind::Synth, CLIP_COLORS[0]);
    let mut clip = Clip::new("pad", CLIP_COLORS[0], 1);
    clip.notes.push(Note { step: 0, len: 16, pitch: 48, vel: 110, slide: false });
    p.track_mut(id).unwrap().clips[0] = Some(clip);
    id
}

fn kicks(p: &mut Project) -> u64 {
    let id = p.add_track("Kick", InstrumentKind::Drums, CLIP_COLORS[1]);
    let mut clip = Clip::new("kick", CLIP_COLORS[1], 1);
    for s in (0..16).step_by(4) {
        clip.notes.push(Note { step: s, len: 1, pitch: 0, vel: 127, slide: false });
    }
    p.track_mut(id).unwrap().clips[0] = Some(clip);
    id
}

#[test]
fn rack_runs_chains_in_parallel() {
    let mut p = Project::empty();
    let id = pad(&mut p);
    let base = rms(&run(p.clone(), 1.0, Some(0)));
    let mut rack = Device::new(9000, DeviceKind::Rack);
    rack.chains[1].devices.push(Device::new(9001, DeviceKind::Bitcrusher));
    p.track_mut(id).unwrap().channel.devices.push(rack.clone());
    let wet = rms(&run(p.clone(), 1.0, Some(0)));
    assert!(wet > base * 0.3, "rack lost the signal: {base} -> {wet}");
    // All chains muted and fully wet: silence.
    for c in &mut rack.chains {
        c.mute = true;
    }
    p.track_mut(id).unwrap().channel.devices[0] = rack;
    let muted = rms(&run(p, 1.0, Some(0))[(SR as usize)..]);
    assert!(muted < 1e-4, "muted rack still passes audio: {muted}");
}

#[test]
fn track_output_routing() {
    let mut p = Project::empty();
    let a = pad(&mut p);
    let bus = p.add_track("Bus", InstrumentKind::Synth, CLIP_COLORS[2]);
    p.track_mut(a).unwrap().output = Output::Track(bus);
    let routed = rms(&run(p.clone(), 1.0, Some(0)));
    assert!(routed > 0.001, "routed signal missing");
    p.track_mut(bus).unwrap().channel.mute = true;
    let muted = rms(&run(p.clone(), 1.0, Some(0))[(SR as usize / 2)..]);
    assert!(muted < 1e-4, "muting the bus must silence its sources: {muted}");
    // A feedback loop must not hang or go silent.
    p.track_mut(bus).unwrap().channel.mute = false;
    p.track_mut(bus).unwrap().output = Output::Track(a);
    assert!(rms(&run(p, 1.0, Some(0))) > 0.001);
}

#[test]
fn sidechain_from_other_track_ducks() {
    let mut p = Project::empty();
    let pid = pad(&mut p);
    let kid = kicks(&mut p);
    p.track_mut(kid).unwrap().channel.mute = true;
    let mut comp = Device::new(9100, DeviceKind::Compressor);
    let defs = comp.kind.defs();
    let thr = defs.iter().position(|d| d.name.starts_with("Thresh")).unwrap();
    let ratio = defs.iter().position(|d| d.name == "Ratio").unwrap();
    comp.params[thr] = -24.0;
    comp.params[ratio] = defs[ratio].max;
    let mut no_sc = p.clone();
    no_sc.track_mut(pid).unwrap().channel.devices.push(comp.clone());
    comp.sc_source = Some(kid);
    p.track_mut(pid).unwrap().channel.devices.push(comp);
    // Without a sidechain the compressor reacts to the pad itself; compare the pad's level
    // swings: with the kick as key it pumps.
    let swing = |x: &[f32]| {
        let w = (SR as usize / 50) * 2;
        let v: Vec<f32> = x.chunks(w).skip(10).map(rms).collect();
        let max = v.iter().cloned().fold(0.0, f32::max);
        let min = v.iter().cloned().fold(f32::MAX, f32::min);
        max / min.max(1e-6)
    };
    let ra = run(no_sc, 2.0, Some(0));
    let rb = run(p, 2.0, Some(0));
    let a = swing(&ra);
    let b = swing(&rb);
    assert!(b > a * 1.5, "sidechain did not pump: {a} vs {b}");
}

#[test]
fn modulator_moves_volume() {
    let mut p = Project::empty();
    let id = pad(&mut p);
    let flat = run(p.clone(), 2.0, Some(0));
    let mut m = Modulator::new(9200);
    m.shape = ModShape::Square;
    m.rate = crate::params::div_index("1/4") as f32;
    m.depth = 1.0;
    m.target = Some(AutoTarget::Volume);
    p.track_mut(id).unwrap().modulators.push(m);
    let moved = run(p, 2.0, Some(0));
    let spread = |x: &[f32]| {
        let v: Vec<f32> = x.chunks(SR as usize / 20).skip(4).map(rms).collect();
        v.iter().cloned().fold(0.0, f32::max) / v.iter().cloned().fold(f32::MAX, f32::min).max(1e-6)
    };
    assert!(spread(&moved) > spread(&flat) * 2.0, "modulator had no effect");
}

#[test]
fn playlist_plays_scene_clips() {
    let mut p = Project::empty();
    pad(&mut p);
    let silent = rms(&run(p.clone(), 1.0, None));
    assert!(silent < 1e-4, "nothing on the timeline should be silent");
    p.playlist.push(PlaylistBlock { id: 9300, scene: 0, start: 0, len: 32, lane: 0 });
    assert!(rms(&run(p, 1.0, None)) > 0.001, "playlist block did not play");
}

#[test]
fn clip_gain_scales_level() {
    let mut p = Project::empty();
    let id = pad(&mut p);
    let a = rms(&run(p.clone(), 1.0, Some(0)));
    p.track_mut(id).unwrap().clips[0].as_mut().unwrap().gain = -12.0;
    let b = rms(&run(p, 1.0, Some(0)));
    let ratio = b / a;
    assert!((0.15..0.4).contains(&ratio), "-12 dB clip gain gave ratio {ratio}");
}

#[test]
fn section_chain_jumps() {
    let mut p = Project::empty();
    let id = pad(&mut p);
    let clip = p.track(id).unwrap().clips[0].clone().unwrap();
    // Content only in the second section (bars 2-3).
    p.track_mut(id).unwrap().arrangement.push(ArrClip { id: 9400, start: 16, len: 32, clip });
    let a = p.add_section(Some("A"), 0, 16);
    let b = p.add_section(Some("B"), 16, 32);
    p.chain = vec![a];
    p.chain_on = true;
    // Bar 1 is empty, and the chain loops on it.
    assert!(rms(&run(p.clone(), 3.0, None)) < 1e-4);
    // The start marker is outside the chain: playback jumps to the chain's first section.
    p.chain = vec![b, a];
    assert!(rms(&run(p, 1.0, None)) > 0.001, "chain did not start at its first section");
}

#[test]
fn section_editing_moves_content() {
    let mut p = Project::empty();
    let id = pad(&mut p);
    let clip = p.track(id).unwrap().clips[0].clone().unwrap();
    p.track_mut(id).unwrap().arrangement.push(ArrClip { id: 9500, start: 16, len: 16, clip });
    p.track_mut(id).unwrap().arrangement.push(ArrClip { id: 9501, start: 64, len: 16, clip: Clip::new("late", CLIP_COLORS[3], 1) });
    let s = p.add_section(Some("Drop"), 16, 32);
    p.duplicate_section(s);
    let starts: Vec<u32> = p.track(id).unwrap().arrangement.iter().map(|c| c.start).collect();
    assert_eq!(starts, vec![16, 48, 96], "duplicate must copy content and push later clips");
    assert_eq!(p.sections.len(), 2);
    p.delete_section(s);
    let starts: Vec<u32> = p.track(id).unwrap().arrangement.iter().map(|c| c.start).collect();
    assert_eq!(starts, vec![16, 64]);
    let s2 = p.sections[0].id;
    p.move_section(s2, 0);
    assert_eq!(p.track(id).unwrap().arrangement[0].start, 0);
}
