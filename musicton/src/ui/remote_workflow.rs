//! Remote commands for arranger, playlist, routing, sidechain, racks and modulators.

use super::App;
use crate::model::*;
use serde_json::{json, Value};

type R = Result<Value, String>;

fn f(a: &Value, k: &str) -> Option<f32> {
    a.get(k).and_then(|v| v.as_f64()).map(|v| v as f32)
}
fn u(a: &Value, k: &str) -> Option<u64> {
    a.get(k).and_then(|v| v.as_u64())
}
fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}
fn b(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(|v| v.as_bool())
}
/// 1-based bar -> steps.
fn bar(a: &Value, k: &str) -> Option<u32> {
    f(a, k).map(|x| ((x - 1.0).max(0.0) * 16.0).round() as u32)
}
fn bars(a: &Value, k: &str) -> Option<u32> {
    f(a, k).map(|x| (x * 16.0).round().max(1.0) as u32)
}

impl App {
    fn section_arg(&self, a: &Value) -> Result<u64, String> {
        if let Some(id) = u(a, "id") {
            if self.project.sections.iter().any(|x| x.id == id) {
                return Ok(id);
            }
        }
        let name = s(a, "section").or(s(a, "name")).ok_or("missing section 'id' or 'section' name")?;
        self.project.sections.iter().find(|x| x.name.eq_ignore_ascii_case(name)).map(|x| x.id).ok_or(format!("no section '{name}'"))
    }

    pub(super) fn exec_workflow(&mut self, cmd: &str, a: &Value) -> Option<R> {
        let r = match cmd {
            "arranger" => self.cmd_arranger(a),
            "playlist" => self.cmd_playlist(a),
            "set_routing" => self.cmd_routing(a),
            "set_sidechain" => self.cmd_sidechain(a),
            "rack" => self.cmd_rack(a),
            "modulator" => self.cmd_modulator(a),
            "get_device_meter" => return Some(self.cmd_meter(a)),
            "set_pad" => self.cmd_pad(a),
            "set_zones" => self.cmd_zones(a),
            "slices_to_clip" => self.cmd_slices(a),
            "chord_track" => self.cmd_chords(a),
            "set_follow_chords" => {
                let tid = match self.resolve_track(a.get("track")) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                let on = b(a, "on").unwrap_or(true);
                self.project.track_mut(tid).unwrap().follow_chords = on;
                Ok(json!({ "follow_chords": on }))
            }
            "transform_clip" => self.cmd_transform(a),
            "add_chord_to_clip" => self.cmd_clip_chord(a),
            "versions" => self.cmd_versions(a),
            "export_musicxml" => {
                let r = match self.clip_ref(a) {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                };
                let Some(clip) = self.clip(r).cloned() else { return Some(Err("no clip there".into())) };
                let xml = crate::score::to_musicxml(&clip, &self.project.name, self.project.bpm, self.project.key, self.project.scale);
                match s(a, "path") {
                    Some(p) => std::fs::write(p, &xml).map(|_| json!({ "written": p, "bytes": xml.len() })).map_err(|e| e.to_string()),
                    None => Ok(json!({ "musicxml": xml })),
                }
            }
            "audio_to_midi" => {
                let r = match self.clip_ref(a) {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                };
                let mode = match s(a, "mode").unwrap_or("melody") {
                    "drums" => crate::audio::analysis::MidiMode::Drums,
                    "harmony" | "chords" => crate::audio::analysis::MidiMode::Harmony,
                    _ => crate::audio::analysis::MidiMode::Melody,
                };
                self.audio_to_midi(r, mode).map(|(t, n)| json!({ "new_track_index": self.project.track_index(t), "notes": n }))
            }
            "split_stems" => {
                let r = match self.clip_ref(a) {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                };
                self.split_stems(r).map(|ids| json!({ "stem_tracks": ids.iter().filter_map(|t| self.project.track_index(*t)).collect::<Vec<_>>() }))
            }
            _ => return None,
        };
        if r.is_ok() {
            self.touch();
        }
        Some(r)
    }

    /// Live measurements of a device (loudness meter, correlation, gain reduction).
    fn cmd_meter(&mut self, a: &Value) -> R {
        let id = u(a, "device_id").ok_or("missing 'device_id'")?;
        let kind = self.project.device_mut(id).ok_or("no such device")?.kind;
        let data = self.status.displays.iter().find(|(d, _)| *d == id).map(|(_, v)| v.clone()).unwrap_or_default();
        let gr = self.status.reductions.iter().find(|(d, _)| *d == id).map(|(_, r)| *r);
        let r1 = |v: f32| (v * 10.0).round() / 10.0;
        let mut o = json!({ "device": kind.name(), "gain_reduction_db": gr.map(r1) });
        match kind {
            DeviceKind::TruePeakLimiter if data.len() >= 6 => {
                o["momentary_lufs"] = json!(r1(data[0]));
                o["short_term_lufs"] = json!(r1(data[1]));
                o["integrated_lufs"] = json!(r1(data[2]));
                o["true_peak_dbtp"] = json!(r1(data[3]));
                o["input_short_term_lufs"] = json!(r1(data[4]));
            }
            DeviceKind::StereoImager if !data.is_empty() => o["correlation"] = json!((data[0] * 100.0).round() / 100.0),
            _ => o["raw"] = json!(data.iter().take(16).collect::<Vec<_>>()),
        }
        Ok(o)
    }

    fn cmd_pad(&mut self, a: &Value) -> R {
        let tid = self.resolve_track(a.get("track"))?;
        let i = u(a, "pad").ok_or("missing 'pad' (0-15)")? as usize;
        let sample = match s(a, "sample") {
            Some("") => Some(None),
            Some(p) => {
                crate::audio::samples::load(p)?;
                Some(Some(p.to_string()))
            }
            None => None,
        };
        let device = match s(a, "add_device") {
            Some(n) => Some(DeviceKind::all().find(|k| k.name().eq_ignore_ascii_case(n) && *k != DeviceKind::Rack).ok_or(format!("unknown effect '{n}'"))?),
            None => None,
        };
        let new_id = self.project.new_id();
        let t = self.project.track_mut(tid).unwrap();
        if t.instrument.kind != InstrumentKind::BeatPads {
            return Err("track is not a Beat Pads instrument".into());
        }
        let pad = t.instrument.pads.get_mut(i).ok_or("pad must be 0-15")?;
        if let Some(smp) = sample {
            if let Some(p) = &smp {
                pad.name = std::path::Path::new(p).file_stem().map(|f| f.to_string_lossy().chars().take(14).collect()).unwrap_or_default();
            }
            pad.sample = smp;
        }
        if let Some(n) = s(a, "name") {
            pad.name = n.to_string();
        }
        if let Some(v) = s(a, "voice") {
            pad.voice = crate::params::DRUM_LANES.iter().position(|l| l.eq_ignore_ascii_case(v)).ok_or(format!("voice must be one of {:?}", crate::params::DRUM_LANES))? as u8;
        }
        if let Some(v) = f(a, "tune") {
            pad.tune = v.clamp(-24.0, 24.0);
        }
        if let Some(v) = f(a, "gain_db") {
            pad.gain = v.clamp(-60.0, 12.0);
        }
        if let Some(v) = f(a, "pan") {
            pad.pan = v.clamp(-1.0, 1.0);
        }
        if let Some(v) = f(a, "decay") {
            pad.decay = v.clamp(0.02, 8.0);
        }
        if let Some(v) = u(a, "choke") {
            pad.choke = (v as u8).min(4);
        }
        if let Some(v) = b(a, "reverse") {
            pad.reverse = v;
        }
        let mut out = json!({ "pad": i, "name": pad.name });
        if let Some(k) = device {
            pad.devices.push(Device::new(new_id, k));
            out["device_id"] = json!(new_id);
        }
        Ok(out)
    }

    fn cmd_zones(&mut self, a: &Value) -> R {
        let tid = self.resolve_track(a.get("track"))?;
        let mut files: Vec<String> = match (s(a, "folder"), a.get("files").and_then(|v| v.as_array())) {
            (Some(dir), _) => std::fs::read_dir(dir)
                .map_err(|e| e.to_string())?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            (None, Some(list)) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => return Err("give 'folder' or 'files'".into()),
        };
        files.sort();
        files.retain(|p| crate::audio::samples::load(p).is_ok());
        let zones = zones_from_files(&files);
        let t = self.project.track_mut(tid).unwrap();
        if t.instrument.kind != InstrumentKind::MultiSampler {
            return Err("track is not a Multi Sampler".into());
        }
        t.instrument.zones = zones;
        Ok(json!({ "zones": t.instrument.zones.iter().map(|z| json!({"file": z.path, "root": note_name(z.root), "low": note_name(z.lo), "high": note_name(z.hi)})).collect::<Vec<_>>() }))
    }

    fn cmd_slices(&mut self, a: &Value) -> R {
        use crate::params::slicer;
        let tid = self.resolve_track(a.get("track"))?;
        let t = self.project.track(tid).unwrap();
        if t.instrument.kind != InstrumentKind::Slicer {
            return Err("track is not a Slicer".into());
        }
        let path = t.instrument.samples.first().cloned().flatten().ok_or("load a sample into the Slicer first (musicton_load_sample)")?;
        let smp = crate::audio::samples::load(&path)?;
        let p = &t.instrument.params;
        let slices = crate::audio::samplers::detect_slices(&smp, p[slicer::MODE].round() as usize, p[slicer::SENS]);
        let beats = if p[slicer::SYNC] > 0.5 { p[slicer::BEATS] } else { smp.left.len() as f32 / smp.sample_rate * self.project.bpm / 60.0 };
        let steps = ((beats * 4.0).round() as u32).max(1);
        let mut clip = Clip::new("Slices", t.color, steps.div_ceil(STEPS_PER_BAR).max(1));
        clip.notes = crate::audio::samplers::slice_notes(&slices, smp.left.len(), steps);
        let scene = match u(a, "scene") {
            Some(sc) => sc as usize,
            None => t.clips.iter().position(|c| c.is_none()).ok_or("no free clip slot")?,
        };
        let n = clip.notes.len();
        self.create_clip(tid, scene, clip);
        Ok(json!({ "scene": scene, "slices": slices.len(), "notes": n }))
    }

    fn cmd_chords(&mut self, a: &Value) -> R {
        match s(a, "action").ok_or("missing 'action' (set, add, delete, clear)")? {
            "set" => {
                // "Am F C G" over 'bars_each' bars from 'start_bar'.
                let prog = s(a, "progression").ok_or("missing 'progression', e.g. \"Am F C G\"")?;
                let each = bars(a, "bars_each").unwrap_or(16);
                let mut pos = bar(a, "start_bar").unwrap_or(0);
                let reps = u(a, "repeat").unwrap_or(1).max(1);
                let parsed: Vec<(u8, usize)> = prog.split_whitespace().map(|c| parse_chord(c).ok_or(format!("unknown chord '{c}'"))).collect::<Result<_, _>>()?;
                if b(a, "replace").unwrap_or(true) {
                    self.project.chords.clear();
                }
                for _ in 0..reps {
                    for &(root, q) in &parsed {
                        self.add_chord_event(pos, each, root, q);
                        pos += each;
                    }
                }
                Ok(json!({ "chords": self.project.chords.iter().map(|c| json!({"id": c.id, "name": c.name(), "start_bar": c.start as f32 / 16.0 + 1.0, "bars": c.len as f32 / 16.0})).collect::<Vec<_>>() }))
            }
            "add" => {
                let (root, q) = parse_chord(s(a, "chord").ok_or("missing 'chord'")?).ok_or("unknown chord")?;
                let id = self.add_chord_event(bar(a, "start_bar").unwrap_or(0), bars(a, "bars").unwrap_or(16), root, q);
                Ok(json!({ "chord_id": id }))
            }
            "delete" => {
                let id = u(a, "id").ok_or("missing 'id'")?;
                self.project.chords.retain(|c| c.id != id);
                Ok(json!("deleted"))
            }
            "clear" => {
                self.project.chords.clear();
                Ok(json!("cleared"))
            }
            other => Err(format!("unknown action '{other}'")),
        }
    }

    fn clip_ref(&self, a: &Value) -> Result<super::ClipRef, String> {
        let track = self.resolve_track(a.get("track"))?;
        Ok(match u(a, "arrangement_clip_id") {
            Some(id) => super::ClipRef::Arr { track, id },
            None => super::ClipRef::Session { track, scene: self.scene_arg(a)? },
        })
    }

    fn cmd_transform(&mut self, a: &Value) -> R {
        let r = self.clip_ref(a)?;
        let rules: Vec<NoteRule> = if let Some(name) = s(a, "preset") {
            rule_presets().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, r)| r).ok_or_else(|| format!("unknown preset; available: {:?}", rule_presets().iter().map(|p| p.0).collect::<Vec<_>>()))?
        } else {
            serde_json::from_value(a.get("rules").cloned().ok_or("give 'preset' or 'rules'")?).map_err(|e| format!("rules: {e}. Example: [{{\"field\":\"Velocity\",\"op\":\"Less\",\"a\":40,\"b\":0,\"action\":\"Delete\",\"value\":0}}]"))?
        };
        let seed = u(a, "seed").unwrap_or(1) as u32;
        let c = self.clip_mut(r).ok_or("no clip there")?;
        let n = apply_rules(&mut c.notes, &rules, seed);
        Ok(json!({ "touched": n, "notes": c.notes.len() }))
    }

    fn cmd_clip_chord(&mut self, a: &Value) -> R {
        let r = self.clip_ref(a)?;
        let (root, q) = parse_chord(s(a, "chord").ok_or("missing 'chord' (e.g. Am7)")?).ok_or("unknown chord")?;
        let octave = a.get("octave").and_then(|v| v.as_i64()).unwrap_or(3) as i32;
        let step = u(a, "step").unwrap_or(0) as u32;
        let len = u(a, "len").unwrap_or(16) as u32;
        let vel = u(a, "vel").unwrap_or(100).clamp(1, 127) as u8;
        let base = ((octave + 2) * 12 + root as i32).clamp(0, 115) as u8;
        let notes = ChordEvent { id: 0, start: 0, len: 0, root, quality: q }.voicing(base);
        let c = self.clip_mut(r).ok_or("no clip there")?;
        for p in &notes {
            c.notes.push(Note { step, len, pitch: *p, vel, slide: false, skip: 0 });
        }
        Ok(json!({ "added": notes.iter().map(|p| note_name(*p)).collect::<Vec<_>>() }))
    }

    fn cmd_versions(&mut self, a: &Value) -> R {
        use crate::versions;
        let dir = versions::versions_dir(&self.project.name);
        match s(a, "action").ok_or("missing 'action' (save, list, compare, restore)")? {
            "save" => {
                let v = self.save_version(s(a, "message").unwrap_or("Snapshot"))?;
                Ok(json!({ "id": v.id, "summary": v.summary }))
            }
            "list" => Ok(json!(versions::list_versions(&dir).iter().map(|v| json!({"id": v.id, "time": versions::format_time(v.time), "message": v.message, "summary": v.summary})).collect::<Vec<_>>())),
            "compare" => {
                let old = versions::load_version(&dir, s(a, "id").ok_or("missing 'id'")?)?;
                Ok(json!({ "changes_since": versions::diff(&old, &self.project) }))
            }
            "restore" => {
                self.restore_version(s(a, "id").ok_or("missing 'id'")?)?;
                Ok(json!("restored (previous state saved as 'Before restore')"))
            }
            other => Err(format!("unknown action '{other}'")),
        }
    }

    fn cmd_arranger(&mut self, a: &Value) -> R {
        let p = &mut self.project;
        match s(a, "action").ok_or("missing 'action'")? {
            "add_section" => {
                let start = bar(a, "start_bar").unwrap_or_else(|| p.sections.iter().map(|x| x.start + x.len).max().unwrap_or(0));
                let id = p.add_section(s(a, "name"), start, bars(a, "bars").unwrap_or(128));
                Ok(json!({ "section_id": id }))
            }
            "move_section" => {
                let id = self.section_arg(a)?;
                let to = bar(a, "start_bar").ok_or("missing 'start_bar'")?;
                self.project.move_section(id, to);
                Ok(json!("moved"))
            }
            "resize_section" => {
                let id = self.section_arg(a)?;
                let len = bars(a, "bars").ok_or("missing 'bars'")?;
                if let Some(x) = self.project.sections.iter_mut().find(|x| x.id == id) {
                    x.len = len;
                }
                Ok(json!("ok"))
            }
            "rename_section" => {
                let id = self.section_arg(a)?;
                let n = s(a, "new_name").ok_or("missing 'new_name'")?;
                if let Some(x) = self.project.sections.iter_mut().find(|x| x.id == id) {
                    x.name = n.to_string();
                }
                Ok(json!("ok"))
            }
            "duplicate_section" => {
                let id = self.section_arg(a)?;
                Ok(json!({ "section_id": self.project.duplicate_section(id) }))
            }
            "delete_section" => {
                let id = self.section_arg(a)?;
                if b(a, "keep_content") == Some(true) {
                    self.project.sections.retain(|x| x.id != id);
                    self.project.chain.retain(|c| *c != id);
                } else {
                    self.project.delete_section(id);
                }
                Ok(json!("deleted"))
            }
            "add_marker" => {
                let pos = bar(a, "bar").ok_or("missing 'bar'")?;
                Ok(json!({ "marker_id": self.add_marker(s(a, "name"), pos) }))
            }
            "delete_marker" => {
                let id = u(a, "id").ok_or("missing marker 'id'")?;
                p.markers.retain(|m| m.id != id);
                Ok(json!("deleted"))
            }
            "set_chain" => {
                let list = a.get("sections").and_then(|v| v.as_array()).ok_or("missing 'sections': [name or id, ...]")?;
                let mut chain = vec![];
                for v in list {
                    let id = match v {
                        Value::Number(n) => n.as_u64(),
                        Value::String(n) => p.sections.iter().find(|x| x.name.eq_ignore_ascii_case(n)).map(|x| x.id),
                        _ => None,
                    }
                    .ok_or(format!("unknown section {v}"))?;
                    chain.push(id);
                }
                p.chain = chain;
                if let Some(on) = b(a, "on") {
                    p.chain_on = on;
                }
                Ok(json!({ "chain_len": p.chain.len(), "on": p.chain_on }))
            }
            "chain_on" => {
                p.chain_on = b(a, "on").unwrap_or(true);
                Ok(json!({ "on": p.chain_on }))
            }
            other => Err(format!("unknown action '{other}' (add_section, move_section, resize_section, rename_section, duplicate_section, delete_section, add_marker, delete_marker, set_chain, chain_on)")),
        }
    }

    fn cmd_playlist(&mut self, a: &Value) -> R {
        match s(a, "action").ok_or("missing 'action'")? {
            "add" => {
                let scene = u(a, "scene").ok_or("missing 'scene'")? as usize;
                if scene >= self.project.scenes.len() {
                    return Err(format!("no scene {scene}"));
                }
                let start = bar(a, "start_bar").unwrap_or_else(|| self.project.playlist_end());
                let id = self.add_block(scene, start, u(a, "lane").unwrap_or(0) as u32, bars(a, "bars"));
                Ok(json!({ "block_id": id }))
            }
            "move" => {
                let id = u(a, "id").ok_or("missing block 'id'")?;
                let bl = self.project.playlist.iter_mut().find(|x| x.id == id).ok_or("no such block")?;
                if let Some(st) = bar(a, "start_bar") {
                    bl.start = st;
                }
                if let Some(l) = u(a, "lane") {
                    bl.lane = (l as u32).min(super::arranger::PL_LANES - 1);
                }
                if let Some(l) = bars(a, "bars") {
                    bl.len = l;
                }
                if let Some(sc) = u(a, "scene") {
                    bl.scene = sc as usize;
                }
                Ok(json!("ok"))
            }
            "delete" => {
                let id = u(a, "id").ok_or("missing block 'id'")?;
                self.project.playlist.retain(|x| x.id != id);
                Ok(json!("deleted"))
            }
            "clear" => {
                self.project.playlist.clear();
                Ok(json!("cleared"))
            }
            "show" => {
                self.arr.ext.playlist = b(a, "on").unwrap_or(true);
                self.view = super::View::Arrangement;
                Ok(json!({ "playlist_visible": self.arr.ext.playlist }))
            }
            other => Err(format!("unknown action '{other}' (add, move, delete, clear, show)")),
        }
    }

    fn cmd_routing(&mut self, a: &Value) -> R {
        let id = self.resolve_track(a.get("track"))?;
        let out = match a.get("output").ok_or("missing 'output': master | return_a | return_b | track")? {
            Value::String(x) if x.eq_ignore_ascii_case("master") => Output::Master,
            Value::String(x) if x.eq_ignore_ascii_case("return_a") => Output::Return(0),
            Value::String(x) if x.eq_ignore_ascii_case("return_b") => Output::Return(1),
            v => {
                let t = self.resolve_track(Some(v))?;
                if t == id {
                    return Err("a track cannot feed itself".into());
                }
                Output::Track(t)
            }
        };
        self.project.track_mut(id).unwrap().output = out;
        Ok(json!({ "output": super::remote::output_name(out, &self.project) }))
    }

    fn cmd_sidechain(&mut self, a: &Value) -> R {
        let dev = u(a, "device_id").ok_or("missing 'device_id'")?;
        let src = match a.get("source") {
            None | Some(Value::Null) => None,
            Some(v) => Some(self.resolve_track(Some(v))?),
        };
        let d = self.project.device_mut(dev).ok_or("no such device")?;
        if !d.kind.has_sidechain() {
            return Err(format!("{} has no sidechain input (use Compressor, Bus Compressor, Gate or Sidechain)", d.kind.name()));
        }
        d.sc_source = src;
        Ok(json!({ "sidechain_from_track_id": src }))
    }

    fn cmd_rack(&mut self, a: &Value) -> R {
        let rid = u(a, "device_id").ok_or("missing 'device_id' (the Rack)")?;
        let action = s(a, "action").ok_or("missing 'action'")?.to_string();
        let new_id = self.project.new_id();
        let d = self.project.device_mut(rid).ok_or("no such device")?;
        if d.kind != DeviceKind::Rack {
            return Err("device is not a Rack".into());
        }
        let ci = u(a, "chain").unwrap_or(0) as usize;
        match action.as_str() {
            "add_chain" => {
                let n = d.chains.len() + 1;
                d.chains.push(RackChain::new(s(a, "name").unwrap_or(&format!("Chain {n}"))));
                Ok(json!({ "chain": d.chains.len() - 1 }))
            }
            "remove_chain" => {
                if ci >= d.chains.len() {
                    return Err("no such chain".into());
                }
                d.chains.remove(ci);
                Ok(json!("removed"))
            }
            "set_chain" => {
                let c = d.chains.get_mut(ci).ok_or("no such chain")?;
                if let Some(v) = f(a, "volume") {
                    c.volume = v.clamp(0.0, 1.0);
                }
                if let Some(v) = f(a, "pan") {
                    c.pan = v.clamp(-1.0, 1.0);
                }
                if let Some(v) = b(a, "mute") {
                    c.mute = v;
                }
                if let Some(v) = s(a, "name") {
                    c.name = v.to_string();
                }
                Ok(json!("ok"))
            }
            "add_device" => {
                let name = s(a, "device").ok_or("missing 'device'")?;
                let kind = DeviceKind::all().find(|k| k.name().eq_ignore_ascii_case(name) && *k != DeviceKind::Rack).ok_or(format!("unknown effect '{name}'"))?;
                let c = d.chains.get_mut(ci).ok_or("no such chain")?;
                c.devices.push(Device::new(new_id, kind));
                Ok(json!({ "device_id": new_id }))
            }
            other => Err(format!("unknown action '{other}' (add_chain, remove_chain, set_chain, add_device)")),
        }
    }

    fn cmd_modulator(&mut self, a: &Value) -> R {
        let tid = self.resolve_track(a.get("track"))?;
        let action = s(a, "action").ok_or("missing 'action' (add, set, remove)")?.to_string();
        // Resolve the target before borrowing the track mutably.
        let target = match s(a, "target") {
            None => None,
            Some(tg) => Some(match tg {
                "none" => None,
                "volume" => Some(AutoTarget::Volume),
                "pan" => Some(AutoTarget::Pan),
                "send_a" => Some(AutoTarget::Send(0)),
                "send_b" => Some(AutoTarget::Send(1)),
                "instrument" => {
                    let t = self.project.track(tid).unwrap();
                    let defs = t.instrument.kind.defs();
                    let key = a.get("param").ok_or("missing 'param'")?;
                    Some(AutoTarget::Instrument(param_index(defs, key)?))
                }
                "device" => {
                    let dev = u(a, "device_id").ok_or("missing 'device_id'")?;
                    let d = self.project.device_mut(dev).ok_or("no such device")?;
                    let key = a.get("param").ok_or("missing 'param'")?;
                    Some(AutoTarget::Device { id: dev, param: param_index(d.kind.defs(), key)? })
                }
                other => return Err(format!("unknown target '{other}'")),
            }),
        };
        let new_id = self.project.new_id();
        let t = self.project.track_mut(tid).unwrap();
        let m = match action.as_str() {
            "add" => {
                t.modulators.push(Modulator::new(new_id));
                t.modulators.last_mut().unwrap()
            }
            "set" => {
                let id = u(a, "id").ok_or("missing modulator 'id'")?;
                t.modulators.iter_mut().find(|m| m.id == id).ok_or("no such modulator")?
            }
            "remove" => {
                let id = u(a, "id").ok_or("missing modulator 'id'")?;
                t.modulators.retain(|m| m.id != id);
                return Ok(json!("removed"));
            }
            other => return Err(format!("unknown action '{other}'")),
        };
        if let Some(sh) = s(a, "shape") {
            m.shape = ModShape::ALL.iter().copied().find(|x| x.name().eq_ignore_ascii_case(sh)).ok_or(format!("unknown shape '{sh}'"))?;
        }
        if let Some(div) = s(a, "rate") {
            m.synced = true;
            m.rate = crate::params::div_index(div) as f32;
        } else if let Some(hz) = f(a, "rate_hz") {
            m.synced = false;
            m.rate = hz.clamp(0.01, 40.0);
        }
        if let Some(v) = f(a, "depth") {
            m.depth = v.clamp(-1.0, 1.0);
        }
        if let Some(v) = f(a, "attack") {
            m.attack = v.max(0.001);
        }
        if let Some(v) = f(a, "decay") {
            m.decay = v.max(0.01);
        }
        if let Some(v) = b(a, "on") {
            m.on = v;
        }
        if let Some(tg) = target {
            m.target = tg;
        }
        Ok(json!({ "modulator_id": m.id }))
    }
}

fn param_index(defs: &[crate::params::ParamDef], key: &Value) -> Result<usize, String> {
    if let Some(i) = key.as_u64() {
        return if (i as usize) < defs.len() { Ok(i as usize) } else { Err(format!("no param {i}")) };
    }
    let n = key.as_str().ok_or("param must be a name or index")?;
    defs.iter().position(|d| d.name.eq_ignore_ascii_case(n)).ok_or(format!("unknown param '{n}'"))
}
