mod audio;
mod library;
mod midi;
mod model;
mod params;
mod presets;
mod remote;
mod versions;
mod ui;
#[cfg(test)]
mod tests_workflow;
#[cfg(test)]
mod tests_effects;
#[cfg(test)]
mod tests_instruments;
#[cfg(test)]
mod tests_composition;
#[cfg(test)]
mod tests_analysis;

fn main() -> eframe::Result {
    // `--x11` runs through XWayland: Wayland compositors pause hidden windows, which would also
    // pause remote control (MCP). X11 windows keep rendering when covered or minimized.
    if std::env::args().any(|a| a == "--x11") || std::env::var("MUSICTON_X11").is_ok() {
        // SAFETY: called at startup before any other threads exist.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Musicton")
            .with_app_id("musicton")
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("Musicton", options, Box::new(|cc| Ok(Box::new(ui::App::new(cc)))))
}

#[cfg(test)]
mod tests {
    use crate::audio::engine::{Cmd, Engine};
    use std::sync::Arc;

    fn render(p: crate::model::Project, secs: f32, scene: Option<usize>) -> Vec<f32> {
        let sr = 48000.0;
        let mut e = Engine::new(sr, Arc::new(p));
        match scene {
            Some(s) => e.force_scene(s),
            None => e.handle(Cmd::Play),
        }
        let mut out = vec![0.0; (sr * secs) as usize * 2];
        for chunk in out.chunks_mut(1024) {
            e.process(chunk);
        }
        out
    }

    #[test]
    fn all_effects_are_stable() {
        use crate::audio::effects::{create, Ctx};
        use crate::model::DeviceKind;
        let sr = 48000.0;
        for kind in DeviceKind::all() {
            let defs = kind.defs();
            // Defaults, then every parameter at min and at max.
            let mut settings = vec![crate::params::defaults(defs)];
            settings.push(defs.iter().map(|d| d.min).collect());
            settings.push(defs.iter().map(|d| d.max).collect());
            for (si, p) in settings.iter().enumerate() {
                let mut fx = create(kind, sr);
                let mut rng = 1u32;
                let mut peak = 0.0f32;
                let mut first_sec = 0.0f32;
                let mut last_sec = 0.0f32;
                let blocks = sr as usize * 3 / 64;
                for block in 0..blocks {
                    let ctx = Ctx { sr, bpm: 130.0, beat: block as f64 * 64.0 / sr as f64 * 130.0 / 60.0, playing: true };
                    fx.set_params(p, &ctx);
                    let mut l = [0.0f32; 64];
                    let mut r = [0.0f32; 64];
                    for i in 0..64 {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        let t = (block * 64 + i) as f32 / sr;
                        let x = (rng as f32 / u32::MAX as f32 - 0.5) * 0.5 + (t * 220.0 * std::f32::consts::TAU).sin() * 0.3;
                        l[i] = x;
                        r[i] = x * 0.9;
                    }
                    fx.process(&mut l, &mut r, &ctx);
                    for v in l.iter().chain(r.iter()) {
                        assert!(v.is_finite(), "{} produced NaN (setting {si})", kind.name());
                        peak = peak.max(v.abs());
                        if block < blocks / 3 {
                            first_sec = first_sec.max(v.abs());
                        } else if block > blocks * 2 / 3 {
                            last_sec = last_sec.max(v.abs());
                        }
                    }
                }
                assert!(last_sec < first_sec * 4.0 + 1.0, "{} unstable: {first_sec} -> {last_sec} (setting {si})", kind.name());
                if si == 0 {
                    println!("{:<20} default peak {:.3}", kind.name(), peak);
                    // Default settings must stay in a sane range (input peak is ~0.55).
                    assert!(peak < 4.0, "{} default settings too hot / self-oscillating: {peak}", kind.name());
                }
            }
        }
    }

    #[test]
    fn midi_effects_chain() {
        use crate::model::*;
        let mut p = Project::empty();
        let id = p.add_track("Arp", InstrumentKind::Synth, CLIP_COLORS[0]);
        for (i, kind) in MidiFxKind::ALL.iter().enumerate() {
            let t = p.track_mut(id).unwrap();
            t.midi_fx.push(MidiFx::new(1000 + i as u64, *kind));
        }
        let mut clip = Clip::new("c", CLIP_COLORS[0], 1);
        clip.notes.push(Note { step: 0, len: 16, pitch: 57, vel: 100, slide: false, skip: 0 });
        p.track_mut(id).unwrap().clips[0] = Some(clip);
        let out = render(p, 3.0, Some(0));
        assert!(out.iter().all(|s| s.is_finite()));
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.01, "arp chain silent");
    }

    #[test]
    fn all_instruments_play() {
        use crate::model::*;
        // A short test sample for Sampler.
        let wav = std::env::temp_dir().join("musicton_test_tone.wav");
        {
            let spec = hound::WavSpec { channels: 1, sample_rate: 44100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut w = hound::WavWriter::create(&wav, spec).unwrap();
            for i in 0..44100 {
                let t = i as f32 / 44100.0;
                w.write_sample(((t * 220.0 * std::f32::consts::TAU).sin() * 0.5 * 32767.0) as i16).unwrap();
            }
            w.finalize().unwrap();
        }
        let wav_s = wav.to_string_lossy().to_string();
        crate::audio::samples::load(&wav_s).unwrap();
        for preset in crate::presets::INSTRUMENT_PRESETS {
            let mut p = Project::empty();
            let id = p.add_track("T", preset.kind, CLIP_COLORS[0]);
            let t = p.track_mut(id).unwrap();
            t.instrument = preset.build();
            if matches!(preset.kind, InstrumentKind::Sampler | InstrumentKind::Slicer) {
                t.instrument.samples[0] = Some(wav_s.clone());
            }
            if preset.kind == InstrumentKind::Slicer {
                // Equal slices so the test tone has something to play.
                t.instrument.params[crate::params::slicer::MODE] = 1.0;
            }
            if preset.kind == InstrumentKind::MultiSampler {
                t.instrument.zones = vec![Zone { lo: 0, hi: 127, ..Zone::new(&wav_s, 57) }];
            }
            let mut clip = Clip::new("c", CLIP_COLORS[0], 1);
            for s in 0..4u32 {
                let pitch = match preset.kind {
                    InstrumentKind::Drums => s as u8,
                    InstrumentKind::Sampler | InstrumentKind::Slicer => 36 + s as u8,
                    InstrumentKind::BeatPads => s as u8,
                    _ => 48 + s as u8 * 3,
                };
                clip.notes.push(Note { step: s * 4, len: 3, pitch, vel: 110, slide: false, skip: 0 });
            }
            t.clips[0] = Some(clip);
            let out = render(p, 2.0, Some(0));
            assert!(out.iter().all(|s| s.is_finite()), "{} produced NaN", preset.name);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!("{:<18} peak {peak:.3}", preset.name);
            assert!(peak > 0.02, "{} is silent", preset.name);
        }
    }

    #[test]
    fn core_library_and_audio_warp() {
        use crate::model::*;
        let root = std::env::temp_dir().join("musicton_lib_test");
        let _ = std::fs::remove_dir_all(&root);
        let t0 = std::time::Instant::now();
        let n = crate::library::build_core_library(&root).unwrap();
        println!("library: {n} files in {:.2}s", t0.elapsed().as_secs_f32());
        assert!(n > 40);
        let lp = root.join(crate::library::CORE_DIR).join("Loops/Drums/Rolling Techno 130bpm.wav");
        let path = lp.to_string_lossy().to_string();
        let s = crate::audio::samples::load(&path).unwrap();
        // Play the 130 BPM loop in a 140 BPM project in each warp mode.
        for warp in [WarpMode::Beats, WarpMode::Repitch, WarpMode::Off] {
            let mut clip = Clip::from_audio(&path, s.left.len(), s.sample_rate, 140.0, CLIP_COLORS[0]);
            let a = clip.audio.as_mut().unwrap();
            println!("guessed bpm {:.2} len {} warp {:?}", a.bpm, clip.len, a.warp);
            assert!((a.bpm - 130.0).abs() < 0.5, "tempo guess");
            a.warp = warp;
            let mut p = Project::empty();
            p.bpm = 140.0;
            let id = p.add_track("A", InstrumentKind::Audio, CLIP_COLORS[0]);
            p.track_mut(id).unwrap().clips[0] = Some(clip);
            let out = render(p, 4.0, Some(0));
            assert!(out.iter().all(|v| v.is_finite()));
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            println!("{warp:?}: peak {peak:.3}");
            assert!(peak > 0.05, "{warp:?} silent");
        }
    }

    #[test]
    fn group_routing() {
        use crate::model::*;
        let mk = |group_mute: bool| {
            let mut p = Project::empty();
            let g = p.add_track("G", InstrumentKind::Group, CLIP_COLORS[0]);
            let d = p.add_track("D", InstrumentKind::Drums, CLIP_COLORS[1]);
            let pat = crate::presets::pattern("Four to the Floor").unwrap();
            let t = p.track_mut(d).unwrap();
            t.group = Some(g);
            t.clips[0] = Some(pat.clip(CLIP_COLORS[1]));
            p.track_mut(g).unwrap().channel.mute = group_mute;
            render(p, 1.0, Some(0)).iter().fold(0.0f32, |m, s| m.max(s.abs()))
        };
        let on = mk(false);
        let off = mk(true);
        println!("group on {on:.3} muted {off:.4}");
        assert!(on > 0.1 && off < 0.01);
    }

    #[test]
    fn nexa_presets_and_b303() {
        use crate::model::*;
        let chord = |p: u8| vec![Note { step: 0, len: 12, pitch: p, vel: 110, slide: false, skip: 0 }, Note { step: 0, len: 12, pitch: p + 3, vel: 100, slide: false, skip: 0 }, Note { step: 0, len: 12, pitch: p + 7, vel: 100, slide: false, skip: 0 }];
        let mut quiet = vec![];
        for (i, pr) in crate::audio::nexa::PRESETS.iter().enumerate() {
            let mut p = Project::empty();
            p.master.devices.clear();
            let id = p.add_track("N", InstrumentKind::Nexa, CLIP_COLORS[0]);
            let t = p.track_mut(id).unwrap();
            t.instrument.params[crate::params::nexa::PRESET] = i as f32;
            for (k, v) in crate::audio::nexa::preset_defaults(i) {
                t.instrument.params[k] = v;
            }
            let mut clip = Clip::new("c", CLIP_COLORS[0], 1);
            clip.notes = chord(if pr.name.starts_with("BS") { 36 } else { 60 });
            t.clips[0] = Some(clip);
            let out = render(p, 3.5, Some(0));
            assert!(out.iter().all(|s| s.is_finite()), "{} NaN", pr.name);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
            println!("{:<22} peak {peak:.3} rms {rms:.3}", pr.name);
            assert!(peak < 4.0, "{} too loud: {peak}", pr.name);
            if peak < 0.03 {
                quiet.push(pr.name);
            }
        }
        assert!(quiet.is_empty(), "silent presets: {quiet:?}");
        // B-303 with an acid line including accents and slides.
        for pr in crate::presets::INSTRUMENT_PRESETS.iter().filter(|p| p.kind == InstrumentKind::B303) {
            let mut p = Project::empty();
            p.master.devices.clear();
            let id = p.add_track("B", InstrumentKind::B303, CLIP_COLORS[0]);
            let t = p.track_mut(id).unwrap();
            t.instrument = pr.build();
            t.clips[0] = Some(crate::presets::pattern("Acid Line 2").unwrap().clip(CLIP_COLORS[0]));
            let out = render(p, 3.0, Some(0));
            assert!(out.iter().all(|s| s.is_finite()));
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!("{:<22} peak {peak:.3}", pr.name);
            assert!(peak > 0.05 && peak < 4.0, "{} level {peak}", pr.name);
        }
    }

    #[test]
    #[ignore]
    fn nexa_spike_debug() {
        use crate::model::*;
        for fx_off in [false, true] {
            let mut p = Project::empty();
            p.master.devices.clear();
            let id = p.add_track("N", InstrumentKind::Nexa, CLIP_COLORS[0]);
            let t = p.track_mut(id).unwrap();
            let i = crate::audio::nexa::PRESETS.iter().position(|x| x.name == std::env::var("NEXA_DEBUG").as_deref().unwrap_or("PL Saw Pluck")).unwrap();
            t.instrument.params[0] = i as f32;
            if !fx_off {
                for (k, v) in crate::audio::nexa::preset_defaults(i) {
                    t.instrument.params[k] = v;
                }
            }
            let mut clip = Clip::new("c", CLIP_COLORS[0], 1);
            clip.notes = vec![Note { step: 0, len: 12, pitch: 60, vel: 110, slide: false, skip: 0 }, Note { step: 0, len: 12, pitch: 63, vel: 100, slide: false, skip: 0 }, Note { step: 0, len: 12, pitch: 67, vel: 100, slide: false, skip: 0 }];
            t.clips[0] = Some(clip);
            let out = render(p, 3.5, Some(0));
            let (idx, peak) = out.iter().enumerate().fold((0, 0.0f32), |(bi, bm), (i, s)| if s.abs() > bm { (i, s.abs()) } else { (bi, bm) });
            println!("fx_off={fx_off} peak {peak:.3} at frame {} ({:.4}s) around {:?}", idx / 2, idx as f32 / 2.0 / 48000.0, &out[idx.saturating_sub(6)..(idx + 6).min(out.len())]);
        }
    }

    #[test]
    fn b303_internal_sequencer() {
        use crate::model::*;
        use crate::params::b303;
        for (mode, delay, drive) in [(0.0, 0.0, 1.0), (2.0, 2.0, 3.0), (4.0, 3.0, 4.0)] {
            let mut p = Project::empty();
            p.master.devices.clear();
            let id = p.add_track("B", InstrumentKind::B303, CLIP_COLORS[0]);
            let t = p.track_mut(id).unwrap();
            t.instrument.params[b303::RUN] = 1.0;
            t.instrument.params[b303::PLAY_MODE] = mode;
            t.instrument.params[b303::DELAY_TYPE] = delay;
            t.instrument.params[b303::DRIVE_TYPE] = drive;
            t.instrument.params[b303::DRIVE] = 0.5;
            t.instrument.params[b303::SHUFFLE] = 0.5;
            // A dummy empty clip just to start the transport; the notes come from the bank.
            t.clips[0] = Some(Clip::new("empty", CLIP_COLORS[0], 1));
            let out = render(p, 2.0, Some(0));
            assert!(out.iter().all(|s| s.is_finite()));
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!("mode {mode} delay {delay} drive {drive}: peak {peak:.3}");
            assert!(peak > 0.05 && peak < 2.0);
        }
    }

    #[test]
    fn demo_renders_audio() {
        for scene in 0..6 {
            let out = render(crate::presets::demo_project(), 4.0, Some(scene));
            assert!(out.iter().all(|s| s.is_finite()), "NaN in scene {scene}");
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
            println!("scene {scene}: peak {peak:.3} rms {rms:.3}");
            assert!(peak > 0.05, "scene {scene} silent");
        }
        let out = render(crate::presets::demo_project(), 6.0, None);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!("arrangement: peak {peak:.3}");
        assert!(peak > 0.05);
    }
}
