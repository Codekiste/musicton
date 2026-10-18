//! MCP server (stdio) that lets an AI assistant operate and test a running Musicton.
//! It forwards tool calls to the app's local control port (127.0.0.1:47800).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

const PORT_DEFAULT: u16 = 47800;

struct Tool {
    name: &'static str,
    cmd: &'static str,
    description: &'static str,
    schema: fn() -> Value,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}
fn none() -> Value {
    obj(json!({}), &[])
}

const TRACK: &str = "Track index (0-based, from musicton_get_state) or exact track name";

fn tools() -> Vec<Tool> {
    vec![
        Tool { name: "musicton_launch_app", cmd: "", description: "Start the Musicton app if it is not running (waits until its control port answers).", schema: none },
        Tool { name: "musicton_get_state", cmd: "get_state", description: "Full overview: project (tempo, loop, key), transport (playing, position, CPU, master level), view/selection, scenes, every track with clips, devices, MIDI effects, arrangement clips, mixer values and live meter, returns, master, mappings.", schema: none },
        Tool { name: "musicton_get_track", cmd: "get_track", description: "Details of one track: all instrument parameters (index, name, value, display, range, options), MIDI effects and audio devices with all parameters and their ids, macros, automation.", schema: || obj(json!({"track": {"description": TRACK}}), &["track"]) },
        Tool { name: "musicton_get_channel_devices", cmd: "get_channel_devices", description: "Devices (with all parameters) of a track, of 'A'/'B' returns or of 'master'.", schema: || obj(json!({"channel": {"description": "Track index/name, 'A', 'B' or 'master'"}}), &["channel"]) },
        Tool { name: "musicton_get_clip", cmd: "get_clip", description: "A clip with all its notes (step = 16th note index, len in 16ths, pitch, vel, slide) or audio settings.", schema: || obj(json!({"track": {"description": TRACK}, "scene": {"type": "integer"}, "arrangement_clip_id": {"type": "integer", "description": "Instead of scene: an arrangement clip id"}}), &["track"]) },
        Tool { name: "musicton_get_catalog", cmd: "get_catalog", description: "Available instrument presets, audio effects, MIDI effects, clip patterns, drum lane names and sample folders.", schema: none },
        Tool { name: "musicton_list_samples", cmd: "list_samples", description: "WAV files in the sample folders (max 300), optionally filtered by a substring.", schema: || obj(json!({"filter": {"type": "string"}}), &[]) },
        Tool { name: "musicton_get_meters", cmd: "get_meters", description: "Live levels: master and per-track peak in dB, gain reduction of compressors/limiters/sidechains, CPU, playing state and position.", schema: none },
        Tool { name: "musicton_play", cmd: "play", description: "Start playback (optionally from a bar of the arrangement).", schema: || obj(json!({"from_bar": {"type": "number", "description": "1-based bar"}}), &[]) },
        Tool { name: "musicton_stop", cmd: "stop", description: "Stop playback.", schema: none },
        Tool { name: "musicton_set_transport", cmd: "set_transport", description: "Set tempo, swing, launch quantization, loop, metronome, record/arrangement-record, crossfader, key and scale. Only given fields change.", schema: || obj(json!({
            "bpm": {"type": "number"}, "swing": {"type": "number", "description": "0..1"}, "quantization_steps": {"type": "integer", "description": "0=none, 4=1/4, 16=1 bar"},
            "loop": {"type": "boolean"}, "loop_start_bar": {"type": "number"}, "loop_bars": {"type": "number"}, "metronome": {"type": "boolean"},
            "record": {"type": "boolean"}, "arrangement_record": {"type": "boolean"}, "crossfader": {"type": "number", "description": "-1 (A) .. 1 (B)"},
            "key": {"type": "string"}, "scale": {"type": "string"}}), &[]) },
        Tool { name: "musicton_launch_clip", cmd: "launch_clip", description: "Launch a Launcher clip (quantized). Starts the transport if stopped.", schema: || obj(json!({"track": {"description": TRACK}, "scene": {"type": "integer"}}), &["track", "scene"]) },
        Tool { name: "musicton_launch_scene", cmd: "launch_scene", description: "Launch a whole Launcher scene (row).", schema: || obj(json!({"scene": {"type": "integer"}}), &["scene"]) },
        Tool { name: "musicton_stop_track", cmd: "stop_track", description: "Stop the Launcher clip of a track.", schema: || obj(json!({"track": {"description": TRACK}}), &["track"]) },
        Tool { name: "musicton_stop_all_clips", cmd: "stop_all_clips", description: "Stop all Launcher clips.", schema: none },
        Tool { name: "musicton_back_to_arrangement", cmd: "back_to_arrangement", description: "Make all tracks follow the Timeline again.", schema: none },
        Tool { name: "musicton_play_note", cmd: "note", description: "Play a single live note on a track (like pressing a key). Drum tracks: pitch = lane 0-9.", schema: || obj(json!({"track": {"description": TRACK}, "pitch": {"type": "integer"}, "vel": {"type": "integer"}, "duration_ms": {"type": "integer"}}), &["track", "pitch"]) },
        Tool { name: "musicton_add_track", cmd: "add_track", description: "Add a track: give an instrument 'preset' (see catalog, e.g. '909 Punch', 'TB Acid', 'Dub Stab', 'FM Bell') or 'type': 'audio' | 'group'.", schema: || obj(json!({"preset": {"type": "string"}, "type": {"type": "string", "enum": ["audio", "group"]}, "name": {"type": "string"}}), &[]) },
        Tool { name: "musicton_delete_track", cmd: "delete_track", description: "Delete a track.", schema: || obj(json!({"track": {"description": TRACK}}), &["track"]) },
        Tool { name: "musicton_set_track", cmd: "set_track", description: "Change a track: name, volume_db, pan (-1..1), mute, solo, send_a/send_b (0..1), crossfade ('A','B','none'), arm (audio tracks), group (group track index/name or null).", schema: || obj(json!({
            "track": {"description": TRACK}, "name": {"type": "string"}, "volume_db": {"type": "number"}, "pan": {"type": "number"}, "mute": {"type": "boolean"}, "solo": {"type": "boolean"},
            "send_a": {"type": "number"}, "send_b": {"type": "number"}, "crossfade": {"type": "string"}, "arm": {"type": "boolean"}, "group": {"description": "Group track or null"}}), &["track"]) },
        Tool { name: "musicton_set_channel", cmd: "set_channel", description: "Volume/pan/mute of 'master', return 'A' or 'B' (or a track).", schema: || obj(json!({"channel": {"type": "string"}, "volume_db": {"type": "number"}, "pan": {"type": "number"}, "mute": {"type": "boolean"}}), &["channel"]) },
        Tool { name: "musicton_add_device", cmd: "add_device", description: "Add an audio effect (to a track, 'A', 'B' or 'master') or a MIDI effect (tracks only). Returns its id.", schema: || obj(json!({"channel": {"description": "Track index/name, 'A', 'B' or 'master'"}, "device": {"type": "string", "description": "e.g. 'Reverb', 'Parametric EQ', 'Sidechain', 'Arpeggiator'"}, "position": {"type": "integer"}}), &["channel", "device"]) },
        Tool { name: "musicton_remove_device", cmd: "remove_device", description: "Remove an audio or MIDI effect by id.", schema: || obj(json!({"id": {"type": "integer"}}), &["id"]) },
        Tool { name: "musicton_set_device_on", cmd: "set_device_on", description: "Bypass or enable a device by id.", schema: || obj(json!({"id": {"type": "integer"}, "on": {"type": "boolean"}}), &["id", "on"]) },
        Tool { name: "musicton_set_param", cmd: "set_param", description: "Set a parameter. Instrument: give 'track'. Effect/MIDI effect: give 'device_id'. 'param' is a name (case-insensitive; drum lanes like 'Kick Decay') or index. Value in real units ('value', or option name for choices) or normalized ('norm' 0..1).", schema: || obj(json!({
            "track": {"description": TRACK}, "device_id": {"type": "integer"}, "param": {"description": "Name or index"}, "value": {"description": "Number in real units, or option name"}, "norm": {"type": "number"}}), &["param"]) },
        Tool { name: "musicton_load_instrument_preset", cmd: "load_instrument_preset", description: "Load an instrument preset onto an existing track of the same instrument type.", schema: || obj(json!({"track": {"description": TRACK}, "preset": {"type": "string"}}), &["track", "preset"]) },
        Tool { name: "musicton_create_clip", cmd: "create_clip", description: "Create a Launcher clip: from a 'pattern' (catalog) or from 'notes' with 'bars'. Notes: [{step (16th index), len (16ths), pitch (MIDI; drum lane 0-9 on drum tracks), vel, slide}].", schema: || obj(json!({
            "track": {"description": TRACK}, "scene": {"type": "integer"}, "pattern": {"type": "string"}, "bars": {"type": "number"}, "name": {"type": "string"}, "notes": {"type": "array", "items": {"type": "object"}}}), &["track", "scene"]) },
        Tool { name: "musicton_set_clip", cmd: "set_clip", description: "Edit a clip: name, bars, replace 'notes' or append 'add_notes', follow action {action, bars, chance}; audio clips: warp ('beats','repitch','off'), audio_bpm, gain_db, transpose.", schema: || obj(json!({
            "track": {"description": TRACK}, "scene": {"type": "integer"}, "arrangement_clip_id": {"type": "integer"}, "name": {"type": "string"}, "bars": {"type": "number"},
            "notes": {"type": "array", "items": {"type": "object"}}, "add_notes": {"type": "array", "items": {"type": "object"}}, "follow": {"type": "object"},
            "warp": {"type": "string"}, "audio_bpm": {"type": "number"}, "gain_db": {"type": "number"}, "transpose": {"type": "number"}}), &["track"]) },
        Tool { name: "musicton_delete_clip", cmd: "delete_clip", description: "Delete a Launcher clip (scene) or an arrangement clip (arrangement_clip_id).", schema: || obj(json!({"track": {"description": TRACK}, "scene": {"type": "integer"}, "arrangement_clip_id": {"type": "integer"}}), &["track"]) },
        Tool { name: "musicton_load_sample", cmd: "load_sample", description: "Load a WAV: onto an audio track's slot (creates a warped audio clip), into a Sampler, or onto a drum lane ('drum_lane').", schema: || obj(json!({"track": {"description": TRACK}, "path": {"type": "string"}, "scene": {"type": "integer"}, "drum_lane": {"type": "integer"}}), &["track", "path"]) },
        Tool { name: "musicton_add_scene", cmd: "add_scene", description: "Insert a scene.", schema: || obj(json!({"index": {"type": "integer"}, "name": {"type": "string"}}), &[]) },
        Tool { name: "musicton_rename_scene", cmd: "rename_scene", description: "Rename a scene.", schema: || obj(json!({"scene": {"type": "integer"}, "name": {"type": "string"}}), &["scene", "name"]) },
        Tool { name: "musicton_scenes_to_arrangement", cmd: "scenes_to_arrangement", description: "Append all non-empty scenes to the arrangement (at least 8 bars each).", schema: none },
        Tool { name: "musicton_add_arrangement_clip", cmd: "add_arrangement_clip", description: "Copy a Launcher clip onto the arrangement timeline at a bar (it loops to fill 'bars').", schema: || obj(json!({"track": {"description": TRACK}, "scene": {"type": "integer"}, "start_bar": {"type": "number"}, "bars": {"type": "number"}}), &["track", "scene", "start_bar"]) },
        Tool { name: "musicton_clear_arrangement", cmd: "clear_arrangement", description: "Remove all arrangement clips and automation.", schema: none },
        Tool { name: "musicton_set_automation", cmd: "set_automation", description: "Replace an arrangement automation envelope. target: volume|pan|send_a|send_b|instrument (needs param)|device (needs device_id+param). points: [[bar (1-based, fractional ok), value 0..1], ...]. Empty points removes it.", schema: || obj(json!({
            "track": {"description": TRACK}, "target": {"type": "string"}, "param": {"description": "Name or index"}, "device_id": {"type": "integer"}, "points": {"type": "array", "items": {"type": "array"}}}), &["track", "target", "points"]) },
        Tool { name: "musicton_render", cmd: "render", description: "Render offline (faster than real time, without disturbing playback) and analyse: peak/RMS dB, crest, clipping, RMS per bar, low/mid/high energy split. source 'scene' (+scene index) or 'arrangement' (+from_bar). Optional solo_track and save_wav path.", schema: || obj(json!({
            "source": {"type": "string", "enum": ["scene", "arrangement"]}, "scene": {"type": "integer"}, "bars": {"type": "number"}, "from_bar": {"type": "number"},
            "solo_track": {"description": TRACK}, "save_wav": {"type": "string"}}), &[]) },
        Tool { name: "musicton_screenshot", cmd: "screenshot", description: "Screenshot of the Musicton window (PNG), or of a track's plug-in window with 'plugin': track. 'downscale' 1-4 reduces size. Coordinates for musicton_ui_input are UI points = pixels / (pixels_per_point / downscale) (main window only).", schema: || obj(json!({"downscale": {"type": "integer"}, "plugin": {"description": TRACK}}), &[]) },
        Tool { name: "musicton_open_plugin", cmd: "open_plugin", description: "Open or close the plug-in window (editor) of a B-303 or Nexa track.", schema: || obj(json!({"track": {"description": TRACK}, "open": {"type": "boolean"}}), &["track"]) },
        Tool { name: "musicton_ui_input", cmd: "ui_input", description: "Send mouse/keyboard input to the UI (in points). events: [{type:'click'|'double_click'|'right_click', x, y, modifiers?}, {type:'drag', x, y, to_x, to_y, steps?}, {type:'scroll', x, y, dy}, {type:'move', x, y}, {type:'key', key:'Space'|'Tab'|'A'|'Enter'|..., modifiers?:['ctrl','shift','alt']}, {type:'text', text}, {type:'wait', frames}]. Returns when the events were processed.", schema: || obj(json!({"events": {"type": "array", "items": {"type": "object"}}}), &["events"]) },
        Tool { name: "musicton_set_view", cmd: "set_view", description: "Switch view ('session'|'arrangement'), detail ('clip'|'devices'), select a track (and scene/clip), show/hide browser, info, detail_view.", schema: || obj(json!({
            "view": {"type": "string"}, "detail": {"type": "string"}, "track": {"description": TRACK}, "scene": {"type": "integer"}, "browser": {"type": "boolean"}, "info": {"type": "boolean"}, "detail_view": {"type": "boolean"}}), &[]) },
        Tool { name: "musicton_undo", cmd: "undo", description: "Undo.", schema: none },
        Tool { name: "musicton_redo", cmd: "redo", description: "Redo.", schema: none },
        Tool { name: "musicton_load_demo", cmd: "load_demo", description: "Load the Warehouse demo project.", schema: none },
        Tool { name: "musicton_new_project", cmd: "new_project", description: "Start a new empty project (one drum track).", schema: none },
        Tool { name: "musicton_save_project", cmd: "save_project", description: "Save the project (.mton) to a path.", schema: || obj(json!({"path": {"type": "string"}}), &[]) },
        Tool { name: "musicton_open_project", cmd: "open_project", description: "Open a .mton project.", schema: || obj(json!({"path": {"type": "string"}}), &["path"]) },
    ]
}

struct Bridge {
    port: u16,
    conn: Option<(TcpStream, BufReader<TcpStream>)>,
}

impl Bridge {
    fn connect(&mut self) -> Result<(), String> {
        if self.conn.is_some() {
            return Ok(());
        }
        let s = TcpStream::connect(("127.0.0.1", self.port)).map_err(|_| {
            format!("Musicton is not running (nothing listens on 127.0.0.1:{}). Call musicton_launch_app or start Musicton.", self.port)
        })?;
        s.set_read_timeout(Some(Duration::from_secs(60))).ok();
        let r = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
        self.conn = Some((s, r));
        Ok(())
    }

    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        for attempt in 0..2 {
            self.connect()?;
            let (w, r) = self.conn.as_mut().unwrap();
            let line = json!({ "cmd": cmd, "args": args }).to_string();
            let mut resp = String::new();
            let ok = writeln!(w, "{line}").is_ok() && r.read_line(&mut resp).map(|n| n > 0).unwrap_or(false);
            if !ok {
                self.conn = None;
                if attempt == 0 {
                    continue;
                }
                return Err("lost connection to Musicton".into());
            }
            let v: Value = serde_json::from_str(&resp).map_err(|e| format!("bad response: {e}"))?;
            return if v["ok"].as_bool() == Some(true) { Ok(v["result"].clone()) } else { Err(v["error"].as_str().unwrap_or("error").to_string()) };
        }
        unreachable!()
    }

    fn launch(&mut self) -> Result<Value, String> {
        if self.call("ping", json!({})).is_ok() {
            return Ok(json!("Musicton is already running"));
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let app = exe.with_file_name("musicton");
        use std::os::unix::process::CommandExt;
        std::process::Command::new(&app)
            .arg("--x11")
            .process_group(0)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", app.display()))?;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(150));
            if self.call("ping", json!({})).is_ok() {
                return Ok(json!("Musicton started"));
            }
        }
        Err("Musicton was started but its control port did not answer".into())
    }
}

fn result_content(tool: &str, r: Result<Value, String>) -> Value {
    match r {
        Ok(v) if tool == "musicton_screenshot" => {
            let data = v["png_base64"].as_str().unwrap_or("").to_string();
            let mut meta = v.clone();
            meta.as_object_mut().map(|m| m.remove("png_base64"));
            json!({ "content": [ { "type": "image", "data": data, "mimeType": "image/png" }, { "type": "text", "text": meta.to_string() } ] })
        }
        Ok(v) => {
            let text = if v.is_string() { v.as_str().unwrap().to_string() } else { serde_json::to_string_pretty(&v).unwrap_or_default() };
            json!({ "content": [ { "type": "text", "text": text } ] })
        }
        Err(e) => json!({ "content": [ { "type": "text", "text": e } ], "isError": true }),
    }
}

fn main() {
    let port = std::env::var("MUSICTON_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT_DEFAULT);
    let mut bridge = Bridge { port, conn: None };
    let tools = tools();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let id = msg.get("id").cloned();
        let method = msg["method"].as_str().unwrap_or("");
        let Some(id) = id else { continue }; // notifications need no answer
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => {
                let version = msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18").to_string();
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "musicton", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Controls the Musicton DAW. Start with musicton_get_state (launch the app with musicton_launch_app if needed). Tracks are addressed by index or name, scenes by 0-based index, bars are 1-based. Clip notes use 16th-note steps. Use musicton_render to check audio objectively and musicton_screenshot + musicton_ui_input to test the UI."
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": (t.schema)() })).collect::<Vec<_>>()
            })),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let args = msg["params"].get("arguments").cloned().unwrap_or(json!({}));
                match tools.iter().find(|t| t.name == name) {
                    Some(t) if t.name == "musicton_launch_app" => Ok(result_content(name, bridge.launch())),
                    Some(t) => Ok(result_content(name, bridge.call(t.cmd, args))),
                    None => Err((-32602, format!("unknown tool {name}"))),
                }
            }
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        let resp = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, m)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": m } }),
        };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}
