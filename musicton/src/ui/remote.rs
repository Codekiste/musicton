//! Executes remote-control commands (from the MCP bridge) on the UI thread.

use super::{App, ClipRef, Detail, Sel, View};
use crate::audio::dsp::{fader_to_gain, gain_to_db};
use crate::audio::engine::{Cmd, Engine};
use crate::model::*;
use crate::params::ParamDef;
use crate::remote::Request;
use base64::Engine as _;
use egui::{pos2, vec2, Event, Modifiers, PointerButton};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

type R = Result<Value, String>;

fn db(g: f32) -> f64 {
    (gain_to_db(g) as f64 * 10.0).round() / 10.0
}

fn vol_db(pos: f32) -> f64 {
    db(fader_to_gain(pos))
}

/// Fader position for a gain in dB (inverse of `fader_to_gain`).
fn db_to_fader(d: f32) -> f32 {
    (0.85 * 10f32.powf(d / 20.0).powf(1.0 / 2.2)).clamp(0.0, 1.0)
}

fn arg_f32(a: &Value, k: &str) -> Option<f32> {
    a.get(k).and_then(|v| v.as_f64()).map(|v| v as f32)
}
fn arg_u64(a: &Value, k: &str) -> Option<u64> {
    a.get(k).and_then(|v| v.as_u64())
}
fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}
fn arg_bool(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(|v| v.as_bool())
}

fn device_json(d: &Device) -> Value {
    let mut o = json!({"id": d.id, "type": d.kind.name(), "on": d.on});
    if let Some(sc) = d.sc_source {
        o["sidechain_from_track_id"] = json!(sc);
    }
    if d.kind == DeviceKind::Rack {
        o["chains"] = Value::Array(
            d.chains
                .iter()
                .map(|c| json!({"name": c.name, "volume": c.volume, "pan": c.pan, "mute": c.mute, "devices": c.devices.iter().map(device_json).collect::<Vec<_>>()}))
                .collect(),
        );
    }
    o
}

pub(super) fn output_name(o: Output, p: &Project) -> String {
    match o {
        Output::Master => "master".into(),
        Output::Return(k) => format!("return_{}", ["a", "b"][(k as usize).min(1)]),
        Output::Track(id) => p.track(id).map(|t| t.name.clone()).unwrap_or_else(|| "?".into()),
    }
}

fn params_json(defs: &[ParamDef], values: &[f32]) -> Value {
    Value::Array(
        defs.iter()
            .zip(values)
            .enumerate()
            .map(|(i, (d, v))| {
                let mut o = json!({ "index": i, "name": d.name, "value": v, "display": d.format(*v), "min": d.min, "max": d.max });
                if d.is_choice() {
                    o["options"] = json!(d.options);
                }
                o
            })
            .collect(),
    )
}

/// Finds a parameter by index or (case-insensitive) name. Drum lane params accept "Kick Tune" style.
fn find_param(defs: &[ParamDef], key: &Value, kind: Option<InstrumentKind>) -> Result<usize, String> {
    if let Some(i) = key.as_u64() {
        return if (i as usize) < defs.len() { Ok(i as usize) } else { Err(format!("parameter index {i} out of range (0..{})", defs.len())) };
    }
    let name = key.as_str().ok_or("param must be a name or an index")?.to_lowercase();
    if kind == Some(InstrumentKind::Drums) {
        for (lane, lname) in crate::params::DRUM_LANES.iter().enumerate() {
            let prefix = lname.to_lowercase();
            if let Some(rest) = name.strip_prefix(&prefix) {
                let rest = rest.trim();
                for p in 0..crate::params::DRUM_LANE_PARAMS {
                    let idx = crate::params::lane_param(lane, p);
                    if defs[idx].name.to_lowercase() == rest {
                        return Ok(idx);
                    }
                }
            }
        }
    }
    defs.iter().position(|d| d.name.to_lowercase() == name).ok_or_else(|| {
        let names: Vec<&str> = defs.iter().map(|d| d.name).collect();
        format!("unknown parameter '{name}'. Available: {}", names.join(", "))
    })
}

/// Value from `value` (real units, or option name for choice params) or `norm` (0..1).
fn param_value(def: &ParamDef, args: &Value) -> Result<f32, String> {
    if let Some(n) = arg_f32(args, "norm") {
        return Ok(def.from_norm(n));
    }
    match args.get("value") {
        Some(Value::String(s)) if def.is_choice() => def
            .options
            .iter()
            .position(|o| o.eq_ignore_ascii_case(s))
            .map(|i| i as f32)
            .ok_or_else(|| format!("'{s}' is not an option ({})", def.options.join(", "))),
        Some(v) => v.as_f64().map(|v| (v as f32).clamp(def.min, def.max)).ok_or("value must be a number".into()),
        None => Err("give 'value' (real units) or 'norm' (0..1)".into()),
    }
}

fn note_json(n: &Note) -> Value {
    let mut o = json!({ "step": n.step, "len": n.len, "pitch": n.pitch, "name": note_name(n.pitch), "vel": n.vel, "slide": n.slide });
    if n.skip > 0 {
        o["chance"] = json!(100 - n.skip as u32);
    }
    o
}

fn parse_notes(v: &Value) -> Result<Vec<Note>, String> {
    let arr = v.as_array().ok_or("notes must be an array")?;
    arr.iter()
        .map(|n| {
            Ok(Note {
                step: arg_u64(n, "step").ok_or("note needs 'step' (16th index)")? as u32,
                len: arg_u64(n, "len").unwrap_or(1).max(1) as u32,
                pitch: arg_u64(n, "pitch").ok_or("note needs 'pitch' (MIDI number; drum lane 0-9 on drum tracks)")?.min(127) as u8,
                vel: arg_u64(n, "vel").unwrap_or(100).clamp(1, 127) as u8,
                slide: arg_bool(n, "slide").unwrap_or(false),
                // "chance" in percent (100 = always).
                skip: 100 - arg_u64(n, "chance").unwrap_or(100).min(100) as u8,
            })
        })
        .collect()
}

fn parse_key(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or_else(|| egui::Key::ALL.iter().copied().find(|k| k.name().eq_ignore_ascii_case(name) || format!("{k:?}").eq_ignore_ascii_case(name)))
}

fn parse_mods(v: &Value) -> Modifiers {
    let m = v.get("modifiers").cloned().unwrap_or(json!([]));
    let has = |k: &str| m.as_array().is_some_and(|a| a.iter().any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case(k)))) || m.get(k).and_then(|b| b.as_bool()).unwrap_or(false);
    let ctrl = has("ctrl") || has("command");
    Modifiers { alt: has("alt"), ctrl, shift: has("shift"), mac_cmd: false, command: ctrl }
}

impl App {
    pub(super) fn poll_remote(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.remote_rx.clone() else { return };
        while let Ok(req) = rx.try_recv() {
            self.handle_request(req, ctx);
        }
        // Scheduled note offs from the `note` command.
        let now = Instant::now();
        let due: Vec<(u64, u8)> = self.remote_note_offs.iter().filter(|(t, _, _)| *t <= now).map(|(_, tr, p)| (*tr, *p)).collect();
        self.remote_note_offs.retain(|(t, _, _)| *t > now);
        for (track, pitch) in due {
            self.send(Cmd::NoteOff { track, pitch });
        }
        if !self.remote_note_offs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(10));
        }
        if self.input_queue.is_empty() {
            if let Some(reply) = self.input_reply.take() {
                let _ = reply.send(Ok(json!({ "done": true })));
            }
        } else {
            ctx.request_repaint();
        }
    }

    /// Feeds queued synthetic input events (one batch per frame).
    pub(super) fn inject_input(&mut self, raw: &mut egui::RawInput) {
        if let Some(batch) = self.input_queue.pop_front() {
            raw.events.extend(batch);
        }
    }

    /// Delivers screenshots requested over the remote API.
    pub(super) fn deliver_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let images: Vec<Arc<egui::ColorImage>> = ctx.input(|i| {
            i.raw.events.iter().chain(i.events.iter()).filter_map(|e| if let Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }).collect()
        });
        let Some(img) = images.first() else {
            ctx.request_repaint();
            return;
        };
        let ppp = ctx.pixels_per_point();
        self.plugin_overlay = None;
        for (reply, scale) in self.pending_shots.drain(..) {
            let _ = reply.send(encode_png(img, scale).map(|(b64, w, h)| {
                json!({ "png_base64": b64, "width": w, "height": h, "pixels_per_point": ppp, "scale": scale,
                        "note": "UI coordinates for ui_input are in points: pixel / (pixels_per_point / scale)" })
            }));
        }
    }

    fn handle_request(&mut self, req: Request, ctx: &egui::Context) {
        match req.cmd.as_str() {
            "screenshot" if req.args.get("plugin").is_some() => {
                // eframe can't screenshot extra viewports, so the editor is drawn once as an
                // overlay in the main window and that frame is captured.
                let scale = arg_u64(&req.args, "downscale").unwrap_or(1).clamp(1, 4) as u32;
                match self.resolve_track(req.args.get("plugin")) {
                    Ok(id) => {
                        self.plugin_overlay = Some(id);
                        self.pending_shots.push((req.reply, scale));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        let _ = req.reply.send(Err(e));
                    }
                }
            }
            "screenshot" => {
                let scale = arg_u64(&req.args, "downscale").unwrap_or(1).clamp(1, 4) as u32;
                self.pending_shots.push((req.reply, scale));
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                ctx.request_repaint();
            }
            "ui_input" => match self.queue_input(&req.args) {
                Ok(()) => {
                    self.input_reply = Some(req.reply);
                    ctx.request_repaint();
                }
                Err(e) => {
                    let _ = req.reply.send(Err(e));
                }
            },
            _ => {
                let r = self.exec(&req.cmd, &req.args);
                let _ = req.reply.send(r);
            }
        }
    }

    fn queue_input(&mut self, args: &Value) -> Result<(), String> {
        let events = args.get("events").and_then(|e| e.as_array()).ok_or("ui_input needs 'events': [...]")?;
        for ev in events {
            let typ = arg_str(ev, "type").ok_or("each event needs a 'type'")?;
            let mods = parse_mods(ev);
            let p = |kx: &str, ky: &str| -> Result<egui::Pos2, String> {
                Ok(pos2(arg_f32(ev, kx).ok_or(format!("'{typ}' needs '{kx}'"))?, arg_f32(ev, ky).ok_or(format!("'{typ}' needs '{ky}'"))?))
            };
            match typ {
                "move" => self.input_queue.push_back(vec![Event::PointerMoved(p("x", "y")?)]),
                "click" | "double_click" | "right_click" => {
                    let pos = p("x", "y")?;
                    let button = if typ == "right_click" || arg_str(ev, "button") == Some("right") { PointerButton::Secondary } else { PointerButton::Primary };
                    self.input_queue.push_back(vec![Event::PointerMoved(pos)]);
                    let times = if typ == "double_click" { 2 } else { 1 };
                    for _ in 0..times {
                        self.input_queue.push_back(vec![Event::PointerButton { pos, button, pressed: true, modifiers: mods }]);
                        self.input_queue.push_back(vec![Event::PointerButton { pos, button, pressed: false, modifiers: mods }]);
                    }
                }
                "drag" => {
                    let a = p("x", "y")?;
                    let b = p("to_x", "to_y")?;
                    let steps = arg_u64(ev, "steps").unwrap_or(12).max(2) as usize;
                    self.input_queue.push_back(vec![Event::PointerMoved(a)]);
                    self.input_queue.push_back(vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: mods }]);
                    for s in 1..=steps {
                        let t = s as f32 / steps as f32;
                        self.input_queue.push_back(vec![Event::PointerMoved(a + (b - a) * t)]);
                    }
                    self.input_queue.push_back(vec![Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: mods }]);
                }
                "scroll" => {
                    let pos = p("x", "y")?;
                    let delta = vec2(arg_f32(ev, "dx").unwrap_or(0.0), arg_f32(ev, "dy").unwrap_or(0.0));
                    self.input_queue.push_back(vec![Event::PointerMoved(pos)]);
                    self.input_queue.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, modifiers: mods }]);
                }
                "key" => {
                    let name = arg_str(ev, "key").ok_or("key event needs 'key' (e.g. Space, Tab, A, Enter, Escape, Delete)")?;
                    let key = parse_key(name).ok_or(format!("unknown key '{name}'"))?;
                    let hold = arg_u64(ev, "hold_frames").unwrap_or(1).max(1);
                    self.input_queue.push_back(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }]);
                    for _ in 1..hold {
                        self.input_queue.push_back(vec![]);
                    }
                    self.input_queue.push_back(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: mods }]);
                }
                "text" => self.input_queue.push_back(vec![Event::Text(arg_str(ev, "text").unwrap_or("").to_string())]),
                "wait" => {
                    for _ in 0..arg_u64(ev, "frames").unwrap_or(5) {
                        self.input_queue.push_back(vec![]);
                    }
                }
                other => return Err(format!("unknown event type '{other}' (move, click, double_click, right_click, drag, scroll, key, text, wait)")),
            }
        }
        // Let the UI settle for a couple of frames before answering.
        self.input_queue.push_back(vec![]);
        self.input_queue.push_back(vec![]);
        Ok(())
    }

    // -----------------------------------------------------------------------------------------

    pub(super) fn resolve_track(&self, v: Option<&Value>) -> Result<u64, String> {
        let v = v.ok_or("missing 'track' (index from get_state, or track name)")?;
        if let Some(i) = v.as_u64() {
            return self.project.tracks.get(i as usize).map(|t| t.id).ok_or(format!("no track with index {i}"));
        }
        let s = v.as_str().ok_or("track must be an index or a name")?;
        self.project
            .tracks
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(s))
            .map(|t| t.id)
            .ok_or_else(|| format!("no track named '{s}'"))
    }

    fn resolve_channel(&self, v: Option<&Value>) -> Result<Sel, String> {
        if let Some(s) = v.and_then(|v| v.as_str()) {
            match s.to_lowercase().as_str() {
                "master" => return Ok(Sel::Master),
                "a" | "return a" | "reverb" => return Ok(Sel::Return(0)),
                "b" | "return b" | "delay" => return Ok(Sel::Return(1)),
                _ => {}
            }
        }
        self.resolve_track(v).map(Sel::Track)
    }

    pub(super) fn scene_arg(&self, a: &Value) -> Result<usize, String> {
        let s = arg_u64(a, "scene").ok_or("missing 'scene' (0-based index)")? as usize;
        if s < self.project.scenes.len() {
            Ok(s)
        } else {
            Err(format!("scene {s} does not exist (0..{})", self.project.scenes.len()))
        }
    }

    fn device_json(d: &Device, reduction: Option<f32>, with_params: bool) -> Value {
        let mut o = json!({ "id": d.id, "type": d.kind.name(), "on": d.on });
        if with_params {
            o["params"] = params_json(d.kind.defs(), &d.params);
        }
        if let Some(r) = reduction {
            o["gain_reduction_db"] = json!(r);
        }
        o
    }

    fn clip_json(c: &Clip, with_notes: bool) -> Value {
        let mut o = json!({ "name": c.name, "bars": c.len as f32 / 16.0, "steps": c.len, "notes": c.notes.len() });
        if with_notes {
            o["note_list"] = Value::Array(c.notes.iter().map(note_json).collect());
        }
        if let Some(a) = &c.audio {
            o["audio"] = json!({ "path": a.path, "warp": format!("{:?}", a.warp), "bpm": a.bpm, "gain_db": a.gain, "transpose": a.transpose });
        }
        if c.gain != 0.0 {
            o["clip_gain_db"] = json!(c.gain);
        }
        if let Some(f) = c.follow {
            o["follow"] = json!({ "action": f.action.name(), "bars": f.bars, "chance": f.chance });
        }
        o
    }

    fn state_json(&self) -> Value {
        let p = &self.project;
        let tracks: Vec<Value> = p
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let ts = self.track_status(t.id).cloned().unwrap_or_default();
                let clips: Vec<Value> = t
                    .clips
                    .iter()
                    .enumerate()
                    .filter_map(|(s, c)| c.as_ref().map(|c| {
                        let mut o = Self::clip_json(c, false);
                        o["scene"] = json!(s);
                        o
                    }))
                    .collect();
                json!({
                    "index": i, "id": t.id, "name": t.name, "type": t.instrument.kind.name(),
                    "group": t.group.and_then(|g| p.track_index(g)),
                    "volume_db": vol_db(t.channel.volume), "pan": t.channel.pan, "mute": t.channel.mute, "solo": t.channel.solo,
                    "sends": t.channel.sends, "crossfade": (["none", "A", "B"][t.channel.xfade.min(2) as usize]), "armed": t.arm,
                    "playing_scene": ts.playing, "queued": ts.queued, "follows_arrangement": ts.follow_arr,
                    "meter_db": db(self.meter(t.id)[0].max(self.meter(t.id)[1])),
                    "clips": clips,
                    "devices": t.channel.devices.iter().map(device_json).collect::<Vec<_>>(),
                    "output": output_name(t.output, p),
                    "follow_chords": t.follow_chords,
                    "modulators": t.modulators.iter().map(|m| json!({"id": m.id, "shape": m.shape.name(), "rate": m.rate, "synced": m.synced, "depth": m.depth, "target": m.target.map(|x| format!("{x:?}")), "on": m.on})).collect::<Vec<_>>(),
                    "midi_fx": t.midi_fx.iter().map(|m| json!({"id": m.id, "type": m.kind.name(), "on": m.on})).collect::<Vec<_>>(),
                    "arrangement_clips": t.arrangement.iter().map(|c| json!({"id": c.id, "name": c.clip.name, "start_bar": c.start as f32 / 16.0 + 1.0, "bars": c.len as f32 / 16.0})).collect::<Vec<_>>(),
                    "automation": t.automation.iter().map(|a| format!("{:?} ({} points)", a.target, a.points.len())).collect::<Vec<_>>(),
                })
            })
            .collect();
        let sel = match self.sel {
            Sel::Track(id) => json!({ "track": p.track_index(id) }),
            Sel::Return(k) => json!({ "return": k }),
            Sel::Master => json!("master"),
        };
        json!({
            "project": {
                "name": p.name, "bpm": p.bpm, "swing": p.swing, "quantization_steps": p.quant,
                "key": NOTE_NAMES[p.key as usize % 12], "scale": SCALES[p.scale.min(SCALES.len() - 1)].0,
                "loop": { "on": p.loop_on, "start_bar": p.loop_start / 16 + 1, "bars": p.loop_len / 16 },
                "crossfader": p.crossfader, "arrangement_bars": p.arrangement_end() as f32 / 16.0,
                "markers": p.markers.iter().map(|m| json!({"id": m.id, "name": m.name, "bar": m.pos as f32 / 16.0 + 1.0})).collect::<Vec<_>>(),
                "sections": p.sections.iter().map(|x| json!({"id": x.id, "name": x.name, "start_bar": x.start as f32 / 16.0 + 1.0, "bars": x.len as f32 / 16.0})).collect::<Vec<_>>(),
                "chain": { "on": p.chain_on, "order": p.chain.iter().filter_map(|id| p.sections.iter().find(|x| x.id == *id).map(|x| x.name.clone())).collect::<Vec<_>>() },
                "chords": p.chords.iter().map(|c| json!({"id": c.id, "name": c.name(), "start_bar": c.start as f32 / 16.0 + 1.0, "bars": c.len as f32 / 16.0})).collect::<Vec<_>>(),
                "playlist": p.playlist.iter().map(|b| json!({"id": b.id, "scene": b.scene, "start_bar": b.start as f32 / 16.0 + 1.0, "bars": b.len as f32 / 16.0, "lane": b.lane})).collect::<Vec<_>>(),
                "file": self.file.as_ref().map(|f| f.display().to_string()),
            },
            "transport": {
                "playing": self.status.playing, "position_bar": self.status.arr_pos / 16.0 + 1.0,
                "record": self.rec, "arrangement_record": self.arr_rec, "metronome": self.metronome,
                "cpu_percent": (self.status.cpu * 100.0).round(), "sample_rate": self.audio.sample_rate,
                "audio_device": self.audio.device_name, "audio_error": self.audio.error,
                "master_meter_db": db(self.meter(super::MASTER_METER)[0].max(self.meter(super::MASTER_METER)[1])),
            },
            "view": format!("{:?}", self.view), "detail": format!("{:?}", self.detail), "selection": sel,
            "selected_clip": self.sel_clip.map(|c| format!("{c:?}")),
            "scenes": p.scenes.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
            "tracks": tracks,
            "returns": p.returns.iter().map(|r| json!({"name": r.name, "volume_db": vol_db(r.channel.volume),
                "devices": r.channel.devices.iter().map(|d| json!({"id": d.id, "type": d.kind.name(), "on": d.on})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "master": { "volume_db": vol_db(p.master.volume),
                "devices": p.master.devices.iter().map(|d| json!({"id": d.id, "type": d.kind.name(), "on": d.on})).collect::<Vec<_>>() },
            "mappings": p.mappings.iter().map(|m| format!("{} -> {}", m.source.label(), self.target_name(m.target))).collect::<Vec<_>>(),
        })
    }

    fn exec(&mut self, cmd: &str, a: &Value) -> R {
        if let Some(r) = self.exec_workflow(cmd, a) {
            return r;
        }
        match cmd {
            "ping" => Ok(json!({ "app": "Musicton", "version": env!("CARGO_PKG_VERSION") })),
            "get_state" => Ok(self.state_json()),
            "get_track" => {
                let id = self.resolve_track(a.get("track"))?;
                let t = self.project.track(id).unwrap();
                let red = |d: &Device| self.status.reductions.iter().find(|(i, _)| *i == d.id).map(|(_, r)| *r);
                Ok(json!({
                    "name": t.name, "type": t.instrument.kind.name(),
                    "instrument_params": params_json(t.instrument.kind.defs(), &t.instrument.params),
                    "samples": t.instrument.samples,
                    "midi_fx": t.midi_fx.iter().map(|m| json!({"id": m.id, "type": m.kind.name(), "on": m.on, "params": params_json(m.kind.defs(), &m.params)})).collect::<Vec<_>>(),
                    "devices": t.channel.devices.iter().map(|d| Self::device_json(d, red(d), true)).collect::<Vec<_>>(),
                    "macros": t.macros.iter().map(|m| json!({"name": m.name, "value": m.value, "targets": m.targets.iter().map(|(t, _, _)| self.target_name(*t)).collect::<Vec<_>>()})).collect::<Vec<_>>(),
                    "automation": t.automation.iter().map(|au| json!({"target": format!("{:?}", au.target), "points": au.points})).collect::<Vec<_>>(),
                }))
            }
            "get_channel_devices" => {
                let sel = self.resolve_channel(a.get("channel"))?;
                let ch = self.channel_mut(sel).ok_or("no such channel")?.clone();
                let red = |d: &Device| self.status.reductions.iter().find(|(i, _)| *i == d.id).map(|(_, r)| *r);
                Ok(Value::Array(ch.devices.iter().map(|d| Self::device_json(d, red(d), true)).collect()))
            }
            "get_clip" => {
                let id = self.resolve_track(a.get("track"))?;
                let r = match arg_u64(a, "arrangement_clip_id") {
                    Some(cid) => ClipRef::Arr { track: id, id: cid },
                    None => ClipRef::Session { track: id, scene: self.scene_arg(a)? },
                };
                let c = self.clip(r).ok_or("no clip there")?;
                Ok(Self::clip_json(c, true))
            }
            "get_catalog" => Ok(json!({
                "instrument_presets": crate::presets::INSTRUMENT_PRESETS.iter().map(|p| json!({"name": p.name, "instrument": p.kind.name()})).collect::<Vec<_>>(),
                "track_types": ["audio", "group"],
                "audio_effects": DeviceKind::all().map(|k| k.name()).collect::<Vec<_>>(),
                "midi_effects": MidiFxKind::ALL.iter().map(|k| k.name()).collect::<Vec<_>>(),
                "patterns": crate::presets::PATTERNS.iter().map(|p| json!({"name": p.name, "for": p.kind.name(), "bars": p.bars})).collect::<Vec<_>>(),
                "drum_lanes": crate::params::DRUM_LANES,
                "sample_folders": self.browser.dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>(),
            })),
            "list_samples" => {
                if !self.browser.scanned {
                    self.browser.rescan();
                }
                let filter = arg_str(a, "filter").unwrap_or("").to_lowercase();
                let list: Vec<String> = self.browser.samples.iter().map(|p| p.display().to_string()).filter(|p| p.to_lowercase().contains(&filter)).take(300).collect();
                Ok(json!(list))
            }
            "get_meters" => Ok(json!({
                "playing": self.status.playing, "position_bar": self.status.arr_pos / 16.0 + 1.0, "cpu_percent": (self.status.cpu * 100.0).round(),
                "master_db": db(self.meter(super::MASTER_METER)[0].max(self.meter(super::MASTER_METER)[1])),
                "tracks": self.project.tracks.iter().map(|t| json!({"name": t.name, "db": db(self.meter(t.id)[0].max(self.meter(t.id)[1]))})).collect::<Vec<_>>(),
                "gain_reduction": self.status.reductions.iter().map(|(id, r)| json!({"device": id, "db": r})).collect::<Vec<_>>(),
            })),

            // Transport
            "play" => {
                if let Some(bar) = arg_f32(a, "from_bar") {
                    self.send(Cmd::SetStart(((bar - 1.0).max(0.0) * 16.0) as u32));
                }
                self.send(Cmd::Play);
                Ok(json!("playing"))
            }
            "stop" => {
                self.send(Cmd::Stop);
                Ok(json!("stopped"))
            }
            "set_transport" => {
                if let Some(v) = arg_f32(a, "bpm") {
                    self.project.bpm = v.clamp(40.0, 300.0);
                }
                if let Some(v) = arg_f32(a, "swing") {
                    self.project.swing = v.clamp(0.0, 1.0);
                }
                if let Some(v) = arg_u64(a, "quantization_steps") {
                    self.project.quant = v as u32;
                }
                if let Some(v) = arg_bool(a, "loop") {
                    self.project.loop_on = v;
                }
                if let Some(v) = arg_f32(a, "loop_start_bar") {
                    self.project.loop_start = ((v - 1.0).max(0.0) * 16.0) as u32;
                }
                if let Some(v) = arg_f32(a, "loop_bars") {
                    self.project.loop_len = (v.max(0.25) * 16.0) as u32;
                }
                if let Some(v) = arg_bool(a, "metronome") {
                    self.metronome = v;
                    self.send(Cmd::Metronome(v));
                }
                if let Some(v) = arg_bool(a, "record") {
                    self.rec = v;
                }
                if let Some(v) = arg_bool(a, "arrangement_record") {
                    self.arr_rec = v;
                }
                if let Some(v) = arg_f32(a, "crossfader") {
                    self.project.crossfader = v.clamp(-1.0, 1.0);
                }
                if let Some(k) = arg_str(a, "key") {
                    if let Some(i) = NOTE_NAMES.iter().position(|n| n.eq_ignore_ascii_case(k)) {
                        self.project.key = i as u8;
                    }
                }
                if let Some(s) = arg_str(a, "scale") {
                    if let Some(i) = SCALES.iter().position(|x| x.0.eq_ignore_ascii_case(s)) {
                        self.project.scale = i;
                    }
                }
                self.touch();
                Ok(self.state_json()["project"].clone())
            }
            "launch_clip" => {
                let track = self.resolve_track(a.get("track"))?;
                let scene = self.scene_arg(a)?;
                self.send(Cmd::Launch { track, scene });
                Ok(json!("launched (quantized to the global quantization)"))
            }
            "launch_scene" => {
                let s = self.scene_arg(a)?;
                self.send(Cmd::LaunchScene(s));
                Ok(json!(format!("scene {s} launched")))
            }
            "stop_track" => {
                let t = self.resolve_track(a.get("track"))?;
                self.send(Cmd::StopTrack(t));
                Ok(json!("stopping"))
            }
            "stop_all_clips" => {
                self.send(Cmd::StopAll);
                Ok(json!("stopping all"))
            }
            "back_to_arrangement" => {
                self.send(Cmd::BackToArrangement);
                Ok(json!("ok"))
            }
            "note" => {
                let track = self.resolve_track(a.get("track"))?;
                let pitch = arg_u64(a, "pitch").ok_or("missing 'pitch'")?.min(127) as u8;
                let vel = arg_u64(a, "vel").unwrap_or(100).clamp(1, 127) as u8;
                let ms = arg_u64(a, "duration_ms").unwrap_or(300);
                self.send(Cmd::NoteOn { track, pitch, vel });
                self.remote_note_offs.push((Instant::now() + Duration::from_millis(ms), track, pitch));
                Ok(json!("note played"))
            }

            // Tracks and mixer
            "add_track" => {
                let id = match (arg_str(a, "preset"), arg_str(a, "type")) {
                    (Some(p), _) => {
                        if !crate::presets::INSTRUMENT_PRESETS.iter().any(|x| x.name.eq_ignore_ascii_case(p)) {
                            return Err(format!("unknown preset '{p}' - see get_catalog"));
                        }
                        let name = crate::presets::INSTRUMENT_PRESETS.iter().find(|x| x.name.eq_ignore_ascii_case(p)).unwrap().name;
                        self.add_track_from_preset(name, None)
                    }
                    (None, Some("audio")) => self.add_audio_track(None),
                    (None, Some("group")) => {
                        let id = self.project.add_track("Group", InstrumentKind::Group, CLIP_COLORS[7]);
                        self.touch();
                        id
                    }
                    _ => return Err("give 'preset' (instrument preset name) or 'type': 'audio' | 'group'".into()),
                };
                if let Some(n) = arg_str(a, "name") {
                    if let Some(t) = self.project.track_mut(id) {
                        t.name = n.to_string();
                    }
                }
                Ok(json!({ "index": self.project.track_index(id), "id": id }))
            }
            "delete_track" => {
                let id = self.resolve_track(a.get("track"))?;
                self.delete_track(id);
                Ok(json!("deleted"))
            }
            "set_track" => {
                let id = self.resolve_track(a.get("track"))?;
                let group = match a.get("group") {
                    Some(Value::Null) => Some(None),
                    Some(g) => Some(Some(self.resolve_track(Some(g))?)),
                    None => None,
                };
                let t = self.project.track_mut(id).unwrap();
                if let Some(n) = arg_str(a, "name") {
                    t.name = n.to_string();
                }
                if let Some(v) = arg_f32(a, "volume_db") {
                    t.channel.volume = db_to_fader(v);
                }
                if let Some(v) = arg_f32(a, "volume") {
                    t.channel.volume = v.clamp(0.0, 1.0);
                }
                if let Some(v) = arg_f32(a, "pan") {
                    t.channel.pan = v.clamp(-1.0, 1.0);
                }
                if let Some(v) = arg_bool(a, "mute") {
                    t.channel.mute = v;
                }
                if let Some(v) = arg_bool(a, "solo") {
                    t.channel.solo = v;
                }
                if let Some(v) = arg_f32(a, "send_a") {
                    t.channel.sends[0] = v.clamp(0.0, 1.0);
                }
                if let Some(v) = arg_f32(a, "send_b") {
                    t.channel.sends[1] = v.clamp(0.0, 1.0);
                }
                if let Some(v) = arg_str(a, "crossfade") {
                    t.channel.xfade = match v.to_lowercase().as_str() { "a" => 1, "b" => 2, _ => 0 };
                }
                if let Some(v) = arg_bool(a, "arm") {
                    if v != t.arm {
                        self.toggle_arm(id);
                    }
                }
                if let Some(g) = group {
                    if let Some(t) = self.project.track_mut(id) {
                        t.group = g;
                    }
                }
                self.touch();
                Ok(self.state_json()["tracks"][self.project.track_index(id).unwrap()].clone())
            }
            "set_channel" => {
                let sel = self.resolve_channel(a.get("channel"))?;
                let ch = self.channel_mut(sel).ok_or("no such channel")?;
                if let Some(v) = arg_f32(a, "volume_db") {
                    ch.volume = db_to_fader(v);
                }
                if let Some(v) = arg_f32(a, "pan") {
                    ch.pan = v.clamp(-1.0, 1.0);
                }
                if let Some(v) = arg_bool(a, "mute") {
                    ch.mute = v;
                }
                self.touch();
                Ok(json!("ok"))
            }

            // Devices
            "add_device" => {
                let sel = self.resolve_channel(a.get("channel").or(a.get("track")))?;
                let name = arg_str(a, "device").ok_or("missing 'device' (effect name, see get_catalog)")?;
                if let Some(kind) = DeviceKind::all().find(|k| k.name().eq_ignore_ascii_case(name)) {
                    let at = arg_u64(a, "position").map(|p| p as usize);
                    self.add_device(sel, kind, at);
                    return Ok(json!({ "device_id": self.sel_device }));
                }
                if let Some(kind) = MidiFxKind::ALL.iter().copied().find(|k| k.name().eq_ignore_ascii_case(name)) {
                    let Sel::Track(t) = sel else { return Err("MIDI effects go on tracks".into()) };
                    self.add_midi_fx(t, kind, arg_u64(a, "position").map(|p| p as usize));
                    let id = self.project.track(t).and_then(|t| t.midi_fx.iter().rev().find(|m| m.kind == kind)).map(|m| m.id);
                    return Ok(json!({ "midi_fx_id": id }));
                }
                Err(format!("unknown device '{name}'"))
            }
            "remove_device" => {
                let id = arg_u64(a, "id").ok_or("missing 'id' (device or MIDI effect id)")?;
                let mut found = false;
                for ch in self.project.tracks.iter_mut().map(|t| &mut t.channel).chain(self.project.returns.iter_mut().map(|r| &mut r.channel)).chain(std::iter::once(&mut self.project.master)) {
                    let n = ch.devices.len();
                    ch.devices.retain(|d| d.id != id);
                    found |= ch.devices.len() != n;
                }
                for t in &mut self.project.tracks {
                    let n = t.midi_fx.len();
                    t.midi_fx.retain(|m| m.id != id);
                    found |= t.midi_fx.len() != n;
                }
                if !found {
                    return Err(format!("no device with id {id}"));
                }
                self.touch();
                Ok(json!("removed"))
            }
            "set_device_on" => {
                let id = arg_u64(a, "id").ok_or("missing 'id'")?;
                let on = arg_bool(a, "on").ok_or("missing 'on'")?;
                if let Some(d) = self.project.device_mut(id) {
                    d.on = on;
                } else if let Some(m) = self.project.tracks.iter_mut().flat_map(|t| t.midi_fx.iter_mut()).find(|m| m.id == id) {
                    m.on = on;
                } else {
                    return Err(format!("no device with id {id}"));
                }
                self.touch();
                Ok(json!("ok"))
            }
            "set_param" => {
                let key = a.get("param").ok_or("missing 'param' (name or index)")?;
                let (name, display) = if let Some(dev) = arg_u64(a, "device_id") {
                    if let Some(d) = self.project.device_mut(dev) {
                        let defs = d.kind.defs();
                        let i = find_param(defs, key, None)?;
                        d.params[i] = param_value(&defs[i], a)?;
                        (format!("{} {}", d.kind.name(), defs[i].name), defs[i].format(d.params[i]))
                    } else if let Some(m) = self.project.tracks.iter_mut().flat_map(|t| t.midi_fx.iter_mut()).find(|m| m.id == dev) {
                        let defs = m.kind.defs();
                        let i = find_param(defs, key, None)?;
                        m.params[i] = param_value(&defs[i], a)?;
                        (format!("{} {}", m.kind.name(), defs[i].name), defs[i].format(m.params[i]))
                    } else {
                        return Err(format!("no device with id {dev}"));
                    }
                } else {
                    let id = self.resolve_track(a.get("track"))?;
                    let t = self.project.track_mut(id).unwrap();
                    let kind = t.instrument.kind;
                    let defs = kind.defs();
                    let i = find_param(defs, key, Some(kind))?;
                    t.instrument.params[i] = param_value(&defs[i], a)?;
                    (format!("{} {}", kind.name(), defs[i].name), defs[i].format(t.instrument.params[i]))
                };
                self.touch();
                Ok(json!({ "param": name, "now": display }))
            }
            "load_instrument_preset" => {
                let id = self.resolve_track(a.get("track"))?;
                let name = arg_str(a, "preset").ok_or("missing 'preset'")?;
                let pr = crate::presets::INSTRUMENT_PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name)).ok_or("unknown preset")?;
                let t = self.project.track_mut(id).unwrap();
                if t.instrument.kind != pr.kind {
                    return Err(format!("track is a {}, preset is for {}", t.instrument.kind.name(), pr.kind.name()));
                }
                let samples = t.instrument.samples.clone();
                t.instrument = pr.build();
                t.instrument.samples = samples;
                self.touch();
                Ok(json!("loaded"))
            }

            // Clips
            "create_clip" => {
                let id = self.resolve_track(a.get("track"))?;
                let scene = self.scene_arg(a)?;
                let t = self.project.track(id).unwrap();
                let (color, tname) = (t.color, t.name.clone());
                let clip = if let Some(pat) = arg_str(a, "pattern") {
                    let p = crate::presets::PATTERNS.iter().find(|p| p.name.eq_ignore_ascii_case(pat)).ok_or(format!("unknown pattern '{pat}'"))?;
                    p.clip(color)
                } else {
                    let bars = arg_f32(a, "bars").unwrap_or(1.0).max(0.25);
                    let mut c = Clip::new(arg_str(a, "name").unwrap_or(&tname), color, 1);
                    c.len = ((bars * 4.0).round() as u32).max(1) * 4;
                    if let Some(n) = a.get("notes") {
                        c.notes = parse_notes(n)?;
                    }
                    c
                };
                self.create_clip(id, scene, clip);
                Ok(json!("created"))
            }
            "set_clip" => {
                let id = self.resolve_track(a.get("track"))?;
                let r = match arg_u64(a, "arrangement_clip_id") {
                    Some(cid) => ClipRef::Arr { track: id, id: cid },
                    None => ClipRef::Session { track: id, scene: self.scene_arg(a)? },
                };
                let notes = a.get("notes").map(parse_notes).transpose()?;
                let add = a.get("add_notes").map(parse_notes).transpose()?;
                let c = self.clip_mut(r).ok_or("no clip there")?;
                if let Some(n) = arg_str(a, "name") {
                    c.name = n.to_string();
                }
                if let Some(b) = arg_f32(a, "bars") {
                    c.len = ((b * 4.0).round() as u32).max(1) * 4;
                }
                if let Some(g) = arg_f32(a, "clip_gain_db") {
                    c.gain = g.clamp(-36.0, 12.0);
                }
                if let Some(n) = notes {
                    c.notes = n;
                }
                if let Some(n) = add {
                    c.notes.extend(n);
                }
                if let Some(f) = a.get("follow") {
                    c.follow = match f.get("action").and_then(|x| x.as_str()) {
                        Some(act) => {
                            let action = FollowAction::ALL.iter().copied().find(|x| x.name().eq_ignore_ascii_case(act)).ok_or("unknown follow action")?;
                            Some(Follow { action, bars: arg_u64(f, "bars").unwrap_or(4) as u32, chance: arg_f32(f, "chance").unwrap_or(1.0) })
                        }
                        None => None,
                    };
                }
                if let Some(au) = c.audio.as_mut() {
                    if let Some(w) = arg_str(a, "warp") {
                        au.warp = match w.to_lowercase().as_str() { "off" => WarpMode::Off, "repitch" | "re-pitch" => WarpMode::Repitch, _ => WarpMode::Beats };
                    }
                    if let Some(v) = arg_f32(a, "audio_bpm") {
                        au.bpm = v;
                    }
                    if let Some(v) = arg_f32(a, "gain_db") {
                        au.gain = v;
                    }
                    if let Some(v) = arg_f32(a, "transpose") {
                        au.transpose = v;
                    }
                }
                let out = Self::clip_json(c, false);
                self.touch();
                Ok(out)
            }
            "delete_clip" => {
                let id = self.resolve_track(a.get("track"))?;
                let r = match arg_u64(a, "arrangement_clip_id") {
                    Some(cid) => ClipRef::Arr { track: id, id: cid },
                    None => ClipRef::Session { track: id, scene: self.scene_arg(a)? },
                };
                self.sel_clip = Some(r);
                self.delete_selected_clip();
                Ok(json!("deleted"))
            }
            "load_sample" => {
                let id = self.resolve_track(a.get("track"))?;
                let path = std::path::PathBuf::from(arg_str(a, "path").ok_or("missing 'path' (WAV file)")?);
                if let Some(lane) = arg_u64(a, "drum_lane") {
                    self.assign_sample(Some(id), lane as usize, &path);
                } else {
                    let scene = arg_u64(a, "scene").unwrap_or(self.sel_scene as u64) as usize;
                    self.drop_sample_on_slot(id, scene.min(self.project.scenes.len() - 1), &path);
                }
                Ok(json!("loaded"))
            }
            "add_scene" => {
                let at = arg_u64(a, "index").map(|i| i as usize).unwrap_or(self.project.scenes.len());
                self.project.add_scene(at.min(self.project.scenes.len()));
                if let Some(n) = arg_str(a, "name") {
                    let i = at.min(self.project.scenes.len() - 1);
                    self.project.scenes[i].name = n.to_string();
                }
                self.touch();
                Ok(json!({ "scenes": self.project.scenes.len() }))
            }
            "rename_scene" => {
                let s = self.scene_arg(a)?;
                self.project.scenes[s].name = arg_str(a, "name").ok_or("missing 'name'")?.to_string();
                self.touch();
                Ok(json!("ok"))
            }

            // Timeline
            "scenes_to_arrangement" => {
                self.scenes_to_arrangement();
                Ok(json!({ "arrangement_bars": self.project.arrangement_end() as f32 / 16.0 }))
            }
            "add_arrangement_clip" => {
                let id = self.resolve_track(a.get("track"))?;
                let scene = self.scene_arg(a)?;
                let clip = self.clip(ClipRef::Session { track: id, scene }).cloned().ok_or("no session clip at that scene to copy")?;
                let start = ((arg_f32(a, "start_bar").ok_or("missing 'start_bar' (1-based)")? - 1.0).max(0.0) * 16.0) as u32;
                let len = arg_f32(a, "bars").map(|b| (b * 16.0) as u32).unwrap_or(clip.len);
                let cid = self.project.new_id();
                self.project.track_mut(id).unwrap().arrangement.push(ArrClip { id: cid, start, len, clip });
                self.touch();
                Ok(json!({ "arrangement_clip_id": cid }))
            }
            "clear_arrangement" => {
                for t in &mut self.project.tracks {
                    t.arrangement.clear();
                    t.automation.clear();
                }
                self.touch();
                Ok(json!("cleared"))
            }
            "set_automation" => {
                let id = self.resolve_track(a.get("track"))?;
                let target = match arg_str(a, "target").ok_or("missing 'target': volume | pan | send_a | send_b | instrument | device")? {
                    "volume" => AutoTarget::Volume,
                    "pan" => AutoTarget::Pan,
                    "send_a" => AutoTarget::Send(0),
                    "send_b" => AutoTarget::Send(1),
                    "instrument" => {
                        let t = self.project.track(id).unwrap();
                        AutoTarget::Instrument(find_param(t.instrument.kind.defs(), a.get("param").ok_or("missing 'param'")?, Some(t.instrument.kind))?)
                    }
                    "device" => {
                        let dev = arg_u64(a, "device_id").ok_or("missing 'device_id'")?;
                        let d = self.project.device_mut(dev).ok_or("no such device")?;
                        AutoTarget::Device { id: dev, param: find_param(d.kind.defs(), a.get("param").ok_or("missing 'param'")?, None)? }
                    }
                    other => return Err(format!("unknown target '{other}'")),
                };
                let pts = a.get("points").and_then(|p| p.as_array()).ok_or("missing 'points': [[bar, value_0_to_1], ...]")?;
                let mut points: Vec<(f32, f32)> = pts
                    .iter()
                    .filter_map(|p| Some((((p.get(0)?.as_f64()? as f32) - 1.0).max(0.0) * 16.0, (p.get(1)?.as_f64()? as f32).clamp(0.0, 1.0))))
                    .collect();
                points.sort_by(|x, y| x.0.total_cmp(&y.0));
                let t = self.project.track_mut(id).unwrap();
                t.automation.retain(|au| au.target != target);
                if !points.is_empty() {
                    t.automation.push(Automation { target, points });
                }
                self.touch();
                Ok(json!("ok"))
            }

            // View and selection
            "set_view" => {
                match arg_str(a, "view") {
                    Some("session") => self.view = View::Session,
                    Some("arrangement") => self.view = View::Arrangement,
                    Some(v) => return Err(format!("unknown view '{v}'")),
                    None => {}
                }
                match arg_str(a, "detail") {
                    Some("clip") => self.detail = Detail::Clip,
                    Some("devices") => self.detail = Detail::Devices,
                    _ => {}
                }
                if a.get("track").is_some() {
                    let sel = self.resolve_channel(a.get("track"))?;
                    self.sel = sel;
                    if let (Sel::Track(t), Some(s)) = (sel, arg_u64(a, "scene")) {
                        if self.clip(ClipRef::Session { track: t, scene: s as usize }).is_some() {
                            self.select_clip(ClipRef::Session { track: t, scene: s as usize });
                        } else {
                            self.sel_scene = s as usize;
                        }
                    }
                }
                for (k, flag) in [("browser", &mut self.show_browser), ("info", &mut self.show_info), ("detail_view", &mut self.show_detail)] {
                    if let Some(v) = arg_bool(a, k) {
                        *flag = v;
                    }
                }
                Ok(json!({ "view": format!("{:?}", self.view), "detail": format!("{:?}", self.detail) }))
            }

            // Project
            "open_plugin" => {
                let id = self.resolve_track(a.get("track"))?;
                let open = arg_bool(a, "open").unwrap_or(true);
                let kind = self.project.track(id).map(|t| t.instrument.kind);
                if !matches!(kind, Some(InstrumentKind::B303 | InstrumentKind::Nexa)) {
                    return Err("only B-303 and Nexa have plug-in windows".into());
                }
                self.plugins.open.insert(id, open);
                self.plugins.hidden = false;
                Ok(json!({ "open": open }))
            }
            "undo" => {
                self.undo();
                Ok(json!("ok"))
            }
            "redo" => {
                self.redo();
                Ok(json!("ok"))
            }
            "load_demo" => {
                self.set_project(super::demo_with_audio());
                self.file = None;
                Ok(json!("demo loaded"))
            }
            "new_project" => {
                self.file_new();
                Ok(json!("new project"))
            }
            "save_project" => {
                let path = arg_str(a, "path").map(std::path::PathBuf::from).or(self.file.clone()).ok_or("missing 'path'")?;
                let s = serde_json::to_string_pretty(&self.project).map_err(|e| e.to_string())?;
                std::fs::write(&path, s).map_err(|e| e.to_string())?;
                self.file = Some(path.clone());
                Ok(json!(format!("saved {}", path.display())))
            }
            "open_project" => {
                let path = std::path::PathBuf::from(arg_str(a, "path").ok_or("missing 'path'")?);
                let p = super::load_project_file(&path)?;
                self.set_project(p);
                self.file = Some(path);
                Ok(json!("opened"))
            }
            "render" => self.render_cmd(a),
            other => Err(format!("unknown command '{other}'")),
        }
    }

    /// Offline render of the current project with level analysis (does not touch live playback).
    fn render_cmd(&mut self, a: &Value) -> R {
        let mut p = self.project.clone();
        p.loop_on = false;
        if let Some(t) = a.get("solo_track") {
            let id = self.resolve_track(Some(t))?;
            for tr in &mut p.tracks {
                tr.channel.solo = tr.id == id;
            }
        }
        let sr = 44100u32;
        let source = arg_str(a, "source").unwrap_or("scene");
        let bars = match source {
            "arrangement" => arg_f32(a, "bars").unwrap_or(p.arrangement_end() as f32 / 16.0),
            _ => arg_f32(a, "bars").unwrap_or(4.0),
        }
        .clamp(0.25, 512.0);
        let secs = bars * 4.0 * 60.0 / p.bpm;
        let mut e = Engine::new(sr as f32, Arc::new(p.clone()));
        match source {
            "arrangement" => {
                if let Some(b) = arg_f32(a, "from_bar") {
                    e.handle(Cmd::SetStart(((b - 1.0).max(0.0) * 16.0) as u32));
                }
                e.handle(Cmd::Play)
            }
            _ => e.force_scene(self.scene_arg(a)?),
        }
        let frames = (secs * sr as f32) as usize;
        let mut out = vec![0.0f32; frames * 2];
        for chunk in out.chunks_mut(1024) {
            e.process(chunk);
        }
        // Analysis: peak, RMS, per-bar RMS and a rough low/mid/high balance.
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len().max(1) as f32).sqrt();
        let bar_frames = (4.0 * 60.0 / p.bpm * sr as f32) as usize;
        let per_bar: Vec<f64> = out
            .chunks((bar_frames * 2).max(2))
            .map(|c| db((c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()))
            .collect();
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        let (al, ah) = ((-2.0 * std::f32::consts::PI * 200.0 / sr as f32).exp(), (-2.0 * std::f32::consts::PI * 4000.0 / sr as f32).exp());
        let (mut el, mut em, mut eh) = (0.0f64, 0.0f64, 0.0f64);
        for f in out.chunks(2) {
            let x = (f[0] + f[1]) * 0.5;
            lo = x + al * (lo - x);
            hi = x + ah * (hi - x);
            let (l, h) = (lo, x - hi);
            let m = x - l - h;
            el += (l * l) as f64;
            em += (m * m) as f64;
            eh += (h * h) as f64;
        }
        let tot = (el + em + eh).max(1e-12);
        let clipped = out.iter().filter(|s| s.abs() >= 0.999).count();
        let mut result = json!({
            "seconds": (secs as f64 * 100.0).round() / 100.0, "bars": bars as f64, "peak_db": db(peak), "rms_db": db(rms), "crest_db": ((db(peak) - db(rms)) * 10.0).round() / 10.0, "bpm": p.bpm as f64,
            "silent": peak < 0.001, "clipped_samples": clipped, "rms_per_bar_db": per_bar,
            "energy_split_percent": { "low_below_200hz": (el / tot * 100.0).round(), "mid": (em / tot * 100.0).round(), "high_above_4khz": (eh / tot * 100.0).round() },
        });
        if let Some(path) = arg_str(a, "save_wav") {
            let spec = hound::WavSpec { channels: 2, sample_rate: sr, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
            for s in &out {
                w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).map_err(|e| e.to_string())?;
            }
            w.finalize().map_err(|e| e.to_string())?;
            result["saved"] = json!(path);
        }
        Ok(result)
    }
}

fn encode_png(img: &egui::ColorImage, scale: u32) -> Result<(String, u32, u32), String> {
    let (w, h) = (img.size[0] as u32 / scale, img.size[1] as u32 / scale);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let px = img.pixels[((y * scale) as usize) * img.size[0] + (x * scale) as usize];
            data.extend_from_slice(&[px.r(), px.g(), px.b()]);
        }
    }
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&data).map_err(|e| e.to_string())?;
    }
    Ok((base64::engine::general_purpose::STANDARD.encode(buf), w, h))
}
