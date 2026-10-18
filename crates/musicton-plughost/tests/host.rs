//! Spawns the host process and drives real plug-ins (skipped when they are not installed).

use musicton_plugproto::{NoteEvent, Reply, Request};
use std::io::{BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Host {
    child: Child,
    to: BufWriter<ChildStdin>,
    from: BufReader<ChildStdout>,
}

impl Host {
    fn spawn() -> Host {
        let mut child = Command::new(env!("CARGO_BIN_EXE_musicton-plughost")).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let to = BufWriter::new(child.stdin.take().unwrap());
        let from = BufReader::new(child.stdout.take().unwrap());
        Host { child, to, from }
    }
    fn call(&mut self, r: Request) -> Reply {
        r.write(&mut self.to).unwrap();
        self.to.flush().unwrap();
        loop {
            match Reply::read(&mut self.from).unwrap() {
                Reply::Param { .. } => continue,
                other => return other,
            }
        }
    }
    fn block(&mut self, frames: u32, events: Vec<NoteEvent>, input: Vec<f32>) -> Vec<f32> {
        match self.call(Request::Process { frames, tempo: 120.0, playing: true, events, input }) {
            Reply::Audio { output, .. } => output,
            other => panic!("unexpected {other:?}"),
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = Request::Quit.write(&mut self.to);
        let _ = self.to.flush();
        let _ = self.child.wait();
    }
}

fn workspace(p: &str) -> String {
    format!("{}/../../{p}", env!("CARGO_MANIFEST_DIR"))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn instrument_plays(path: &str) {
    if !std::path::Path::new(path).exists() {
        eprintln!("{path} not built - skipping");
        return;
    }
    let mut h = Host::spawn();
    let (instrument, params) = match h.call(Request::Load { path: path.into(), id: String::new(), sample_rate: 48000.0, max_block: 512 }) {
        Reply::Loaded { instrument, params, .. } => (instrument, params),
        other => panic!("load failed: {other:?}"),
    };
    assert!(instrument && !params.is_empty(), "{path}: instrument {instrument}, {} params", params.len());
    let mut out = vec![];
    for b in 0..200 {
        let ev = if b == 2 { vec![NoteEvent { offset: 0, on: true, key: 45, velocity: 120 }] } else { vec![] };
        out.extend(h.block(256, ev, vec![0.0; 512]));
    }
    assert!(out.iter().all(|v| v.is_finite()));
    assert!(rms(&out) > 0.001, "{path} stayed silent");
    // State round trip and a parameter change must not break anything.
    let state = match h.call(Request::GetState) {
        Reply::State(s) => s,
        other => panic!("{other:?}"),
    };
    assert!(!state.is_empty());
    assert_eq!(h.call(Request::SetParam { id: params[0].id, value: params[0].default }), Reply::Ok);
    assert_eq!(h.call(Request::SetState(state)), Reply::Ok);
    let _ = h.block(64, vec![], vec![0.0; 128]);
}

#[test]
fn clap_instrument() {
    instrument_plays(&workspace("target/bundled/B-303.clap"));
}

#[test]
fn vst3_instrument() {
    instrument_plays(&workspace("target/bundled/B-303.vst3"));
}

fn effect_rings(path: &str) {
    if !std::path::Path::new(path).exists() {
        eprintln!("{path} not installed - skipping");
        return;
    }
    let mut h = Host::spawn();
    match h.call(Request::Load { path: path.into(), id: String::new(), sample_rate: 48000.0, max_block: 512 }) {
        Reply::Loaded { instrument, .. } => assert!(!instrument),
        other => panic!("load failed: {other:?}"),
    }
    let mut impulse = vec![0.0f32; 512];
    impulse[0] = 1.0;
    impulse[256] = 1.0;
    let mut out = h.block(256, vec![], impulse);
    for _ in 0..80 {
        out.extend(h.block(256, vec![], vec![0.0; 512]));
    }
    assert!(out.iter().all(|v| v.is_finite()));
    let tail = rms(&out[4096..]);
    assert!(tail > 1e-5, "{path}: no reverb tail ({tail})");
}

#[test]
fn clap_effect() {
    effect_rings("/usr/lib/clap/DragonflyRoomReverb.clap");
}

#[test]
fn vst3_effect() {
    effect_rings("/usr/lib/vst3/DragonflyHallReverb.vst3");
}

#[test]
fn crashing_or_missing_plugin_reports_error() {
    let mut h = Host::spawn();
    match h.call(Request::Load { path: "/nonexistent/x.clap".into(), id: String::new(), sample_rate: 48000.0, max_block: 64 }) {
        Reply::Error(_) => {}
        other => panic!("expected an error, got {other:?}"),
    }
    // The host stays usable and returns silence.
    assert_eq!(h.block(4, vec![], vec![0.5; 8]), vec![0.0; 8]);
}

#[test]
fn quit_ends_the_process() {
    let mut h = Host::spawn();
    Request::Quit.write(&mut h.to).unwrap();
    h.to.flush().unwrap();
    for _ in 0..50 {
        if h.child.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("host did not exit after Quit");
}
