//! MIDI keyboard input via ALSA (midir). Notes go straight to the engine for low latency and
//! are also forwarded to the UI for recording and Capture MIDI.

use crate::audio::engine::Cmd;
use crossbeam_channel::Sender;
use midir::{MidiInput, MidiInputConnection};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub enum MidiEvent {
    NoteOn { pitch: u8, vel: u8 },
    NoteOff { pitch: u8 },
    /// Control change (knobs, faders).
    Cc { ch: u8, cc: u8, val: u8 },
    /// A note that is mapped (or being learned) - it controls the app instead of playing.
    Mapped { ch: u8, note: u8, vel: u8 },
}

pub struct Midi {
    conn: Option<MidiInputConnection<()>>,
    pub port_name: Option<String>,
    pub target_track: Arc<AtomicU64>,
    /// Drum tracks map incoming notes to lanes, so the UI tells us the track type.
    pub target_is_drum: Arc<std::sync::atomic::AtomicBool>,
    pub learning: Arc<std::sync::atomic::AtomicBool>,
    pub blocked: Arc<parking_lot::RwLock<std::collections::HashSet<(u8, u8)>>>,
}

impl Midi {
    pub fn new() -> Self {
        Self {
            conn: None,
            port_name: None,
            target_track: Arc::new(AtomicU64::new(0)),
            target_is_drum: Arc::new(Default::default()),
            learning: Arc::new(Default::default()),
            blocked: Arc::new(Default::default()),
        }
    }

    pub fn list_ports() -> Vec<String> {
        let Ok(input) = MidiInput::new("musicton-list") else { return vec![] };
        input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect()
    }

    pub fn connect(&mut self, name: &str, engine: Sender<Cmd>, ui: Sender<MidiEvent>, repaint: egui::Context) -> Result<(), String> {
        self.conn = None;
        let input = MidiInput::new("musicton").map_err(|e| e.to_string())?;
        let ports = input.ports();
        let port = ports
            .iter()
            .find(|p| input.port_name(p).ok().as_deref() == Some(name))
            .ok_or("MIDI port not found")?
            .clone();
        let track = self.target_track.clone();
        let is_drum = self.target_is_drum.clone();
        let learning = self.learning.clone();
        let blocked = self.blocked.clone();
        let conn = input
            .connect(
                &port,
                "musicton-in",
                move |_, msg, _| {
                    if msg.len() < 3 {
                        return;
                    }
                    let kind = msg[0] & 0xf0;
                    let ch = msg[0] & 0x0f;
                    let (raw_pitch, vel) = (msg[1], msg[2]);
                    if kind == 0xb0 {
                        let _ = ui.try_send(MidiEvent::Cc { ch, cc: raw_pitch, val: vel });
                        repaint.request_repaint();
                        return;
                    }
                    if kind == 0x90 || kind == 0x80 {
                        let mapped = learning.load(Ordering::Relaxed) || blocked.try_read().is_some_and(|b| b.contains(&(ch, raw_pitch)));
                        if mapped {
                            let v = if kind == 0x80 { 0 } else { vel };
                            let _ = ui.try_send(MidiEvent::Mapped { ch, note: raw_pitch, vel: v });
                            repaint.request_repaint();
                            return;
                        }
                    }
                    let pitch = if is_drum.load(Ordering::Relaxed) { drum_lane(raw_pitch) } else { Some(raw_pitch) };
                    let Some(pitch) = pitch else { return };
                    let t = track.load(Ordering::Relaxed);
                    let ev = match kind {
                        0x90 if vel > 0 => MidiEvent::NoteOn { pitch, vel },
                        0x80 | 0x90 => MidiEvent::NoteOff { pitch },
                        _ => return,
                    };
                    match ev {
                        MidiEvent::NoteOn { pitch, vel } => {
                            let _ = engine.try_send(Cmd::NoteOn { track: t, pitch, vel });
                        }
                        MidiEvent::NoteOff { pitch } => {
                            let _ = engine.try_send(Cmd::NoteOff { track: t, pitch });
                        }
                        _ => {}
                    }
                    let _ = ui.try_send(ev);
                    repaint.request_repaint();
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        self.conn = Some(conn);
        self.port_name = Some(name.to_string());
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.conn = None;
        self.port_name = None;
    }
}

/// General MIDI drum map (C1 = 36 kick ...) to drum machine lanes; anything else maps by
/// semitone offset from C so plain keyboards work too.
pub fn drum_lane(pitch: u8) -> Option<u8> {
    let lane = match pitch {
        35 | 36 => 0,
        39 => 1,
        38 | 40 => 2,
        37 => 3,
        42 | 44 => 4,
        46 => 5,
        51 | 59 => 6,
        41 | 43 | 45 => 7,
        47 | 48 | 50 => 8,
        56 | 49 | 57 => 9,
        p => (p % 12) as i32,
    };
    (lane < 10).then_some(lane as u8)
}
