//! Notation helpers: key signatures, note spelling, splitting clips into notated durations
//! (with ties at bar lines), and MusicXML export.

use crate::model::{Clip, SCALES};

/// Sharps (+) or flats (-) of the key signature for a key and scale (modes use their relative major).
pub fn key_signature(key: u8, scale: usize) -> i32 {
    let name = SCALES.get(scale).map(|s| s.0).unwrap_or("Major");
    let major_root = match name {
        "Minor" | "Harmonic Minor" | "Minor Pentatonic" => key + 3,
        "Dorian" => key + 10,
        "Phrygian" => key + 8,
        "Chromatic" => 0,
        _ => key,
    } % 12;
    [0, -5, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5][major_root as usize]
}

/// Letter index (C=0 .. B=6) and accidental (-1, 0, +1) for a pitch in a key signature.
pub fn spell(pitch: u8, sig: i32) -> (i32, i32) {
    let pc = pitch % 12;
    const SHARP: [(i32, i32); 12] = [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (3, 1), (4, 0), (4, 1), (5, 0), (5, 1), (6, 0)];
    const FLAT: [(i32, i32); 12] = [(0, 0), (1, -1), (1, 0), (2, -1), (2, 0), (3, 0), (4, -1), (4, 0), (5, -1), (5, 0), (6, -1), (6, 0)];
    if sig >= 0 { SHARP[pc as usize] } else { FLAT[pc as usize] }
}

/// Diatonic staff position (C4 = 28, each step one line or space).
pub fn staff_pos(pitch: u8, sig: i32) -> i32 {
    let (letter, _) = spell(pitch, sig);
    let octave = pitch as i32 / 12 - 1;
    // B#/Cb edge cases do not occur with this spelling table.
    octave * 7 + letter
}

/// Accidental the key signature gives a letter (+1 sharp, -1 flat, 0 none).
pub fn sig_accidental(letter: i32, sig: i32) -> i32 {
    const SHARPS: [i32; 7] = [3, 0, 4, 1, 5, 2, 6]; // F C G D A E B
    const FLATS: [i32; 7] = [6, 2, 5, 1, 4, 0, 3]; // B E A D G C F
    if sig > 0 && SHARPS[..sig as usize].contains(&letter) {
        1
    } else if sig < 0 && FLATS[..(-sig) as usize].contains(&letter) {
        -1
    } else {
        0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoreEvent {
    /// Start in 16th steps.
    pub start: u32,
    /// Notated duration in 16ths: one of 1, 2, 3, 4, 6, 8, 12, 16.
    pub dur: u32,
    /// Empty for rests.
    pub pitches: Vec<u8>,
    pub tie_start: bool,
    pub tie_stop: bool,
}

const VALUES: [u32; 8] = [16, 12, 8, 6, 4, 3, 2, 1];

/// Splits a span into notatable values, never crossing a beat boundary with dotted values
/// that would hide the beat (good enough for readable 4/4).
fn decompose(mut start: u32, mut len: u32) -> Vec<(u32, u32)> {
    let mut out = vec![];
    while len > 0 {
        let in_bar = start % 16;
        let v = *VALUES.iter().find(|&&v| v <= len && (in_bar % v.min(4) == 0 || v < 4) && in_bar + v <= 16).unwrap_or(&1);
        out.push((start, v));
        start += v;
        len -= v;
    }
    out
}

/// Turns a clip into notated events: notes starting together form a chord lasting until the
/// next onset (or their own end), gaps become rests; everything is split at bar lines with ties.
pub fn score_events(clip: &Clip) -> Vec<ScoreEvent> {
    let len = clip.len.max(1);
    let mut starts: Vec<u32> = clip.notes.iter().filter(|n| n.step < len).map(|n| n.step).collect();
    starts.sort();
    starts.dedup();
    let mut spans: Vec<(u32, u32, Vec<u8>)> = vec![];
    let mut cursor = 0;
    for (i, &s) in starts.iter().enumerate() {
        if s > cursor {
            spans.push((cursor, s - cursor, vec![]));
        }
        let next = starts.get(i + 1).copied().unwrap_or(len);
        let mut pitches: Vec<u8> = clip.notes.iter().filter(|n| n.step == s).map(|n| n.pitch).collect();
        pitches.sort();
        pitches.dedup();
        let end = clip.notes.iter().filter(|n| n.step == s).map(|n| s + n.len).max().unwrap_or(s + 1).min(next).max(s + 1);
        spans.push((s, end - s, pitches));
        cursor = end;
    }
    if cursor < len {
        spans.push((cursor, len - cursor, vec![]));
    }
    let mut out = vec![];
    for (start, dur, pitches) in spans {
        // Split at bar lines first, then into notatable values.
        let mut pieces = vec![];
        let mut s = start;
        let end = start + dur;
        while s < end {
            let bar_end = (s / 16 + 1) * 16;
            let e = end.min(bar_end);
            pieces.extend(decompose(s, e - s));
            s = e;
        }
        let n = pieces.len();
        for (k, (s, v)) in pieces.into_iter().enumerate() {
            let tied = !pitches.is_empty();
            out.push(ScoreEvent { start: s, dur: v, pitches: pitches.clone(), tie_start: tied && k + 1 < n, tie_stop: tied && k > 0 });
        }
    }
    out
}

fn note_type(dur: u32) -> (&'static str, bool) {
    match dur {
        16 => ("whole", false),
        12 => ("half", true),
        8 => ("half", false),
        6 => ("quarter", true),
        4 => ("quarter", false),
        3 => ("eighth", true),
        2 => ("eighth", false),
        _ => ("16th", false),
    }
}

/// MusicXML (partwise) for one clip, 4/4, divisions = 4 per quarter.
pub fn to_musicxml(clip: &Clip, title: &str, bpm: f32, key: u8, scale: usize) -> String {
    let sig = key_signature(key, scale);
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 3.1 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n");
    x.push_str(&format!("<score-partwise version=\"3.1\"><work><work-title>{}</work-title></work>\n", esc(title)));
    x.push_str("<identification><encoding><software>Musicton</software></encoding></identification>\n");
    x.push_str(&format!("<part-list><score-part id=\"P1\"><part-name>{}</part-name></score-part></part-list>\n<part id=\"P1\">\n", esc(&clip.name)));
    let events = score_events(clip);
    let bars = clip.len.max(1).div_ceil(16);
    for bar in 0..bars {
        x.push_str(&format!("<measure number=\"{}\">", bar + 1));
        if bar == 0 {
            x.push_str(&format!("<attributes><divisions>4</divisions><key><fifths>{sig}</fifths></key><time><beats>4</beats><beat-type>4</beat-type></time><clef><sign>G</sign><line>2</line></clef></attributes>"));
            x.push_str(&format!("<direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>{}</per-minute></metronome></direction-type><sound tempo=\"{}\"/></direction>", bpm.round(), bpm.round()));
        }
        for e in events.iter().filter(|e| e.start / 16 == bar) {
            let (ty, dot) = note_type(e.dur);
            if e.pitches.is_empty() {
                x.push_str(&format!("<note><rest/><duration>{}</duration><type>{ty}</type>{}</note>", e.dur, if dot { "<dot/>" } else { "" }));
                continue;
            }
            for (i, &p) in e.pitches.iter().enumerate() {
                let (letter, acc) = spell(p, sig);
                let step = ["C", "D", "E", "F", "G", "A", "B"][letter as usize];
                let octave = p as i32 / 12 - 1;
                x.push_str("<note>");
                if i > 0 {
                    x.push_str("<chord/>");
                }
                x.push_str(&format!("<pitch><step>{step}</step>{}<octave>{octave}</octave></pitch>", if acc != 0 { format!("<alter>{acc}</alter>") } else { String::new() }));
                x.push_str(&format!("<duration>{}</duration>", e.dur));
                if e.tie_stop {
                    x.push_str("<tie type=\"stop\"/>");
                }
                if e.tie_start {
                    x.push_str("<tie type=\"start\"/>");
                }
                x.push_str(&format!("<type>{ty}</type>{}", if dot { "<dot/>" } else { "" }));
                if e.tie_stop || e.tie_start {
                    x.push_str("<notations>");
                    if e.tie_stop {
                        x.push_str("<tied type=\"stop\"/>");
                    }
                    if e.tie_start {
                        x.push_str("<tied type=\"start\"/>");
                    }
                    x.push_str("</notations>");
                }
                x.push_str("</note>");
            }
        }
        x.push_str("</measure>\n");
    }
    x.push_str("</part>\n</score-partwise>\n");
    x
}
