//! Local control server (127.0.0.1 only) used by the `musicton-mcp` bridge.
//! Protocol: one JSON object per line, `{"cmd": "...", "args": {...}}`, answered by
//! `{"ok": true, "result": ...}` or `{"ok": false, "error": "..."}`.

use crossbeam_channel::{Receiver, Sender};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 47800;

pub struct Request {
    pub cmd: String,
    pub args: Value,
    pub reply: Sender<Result<Value, String>>,
}

pub fn port() -> u16 {
    std::env::var("MUSICTON_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// Starts the server thread. Requests are handed to the UI thread, which owns the project.
pub fn start(ctx: egui::Context) -> Option<Receiver<Request>> {
    if std::env::var("MUSICTON_NO_REMOTE").is_ok() {
        return None;
    }
    let listener = match TcpListener::bind(("127.0.0.1", port())) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("remote control disabled: {e}");
            return None;
        }
    };
    let (tx, rx) = crossbeam_channel::unbounded::<Request>();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let Ok(read_half) = stream.try_clone() else { return };
                let mut writer = stream;
                for line in BufReader::new(read_half).lines() {
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    let answer = match serde_json::from_str::<Value>(&line) {
                        Ok(v) => {
                            let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("").to_string();
                            let args = v.get("args").cloned().unwrap_or(json!({}));
                            let (rtx, rrx) = crossbeam_channel::bounded(1);
                            let _ = tx.send(Request { cmd, args, reply: rtx });
                            ctx.request_repaint();
                            match rrx.recv_timeout(Duration::from_secs(20)) {
                                Ok(Ok(result)) => json!({ "ok": true, "result": result }),
                                Ok(Err(e)) => json!({ "ok": false, "error": e }),
                                Err(_) => json!({
                                    "ok": false,
                                    "error": "Musicton did not answer in time: its window is probably hidden or minimized and Wayland pauses hidden windows. Bring the window to the front, or start Musicton with --x11 (musicton_launch_app does this)."
                                }),
                            }
                        }
                        Err(e) => json!({ "ok": false, "error": format!("invalid JSON: {e}") }),
                    };
                    if writeln!(writer, "{answer}").is_err() {
                        break;
                    }
                }
            });
        }
    });
    Some(rx)
}
