//! Clip operations behind the clip menus: split, join, crop, time editing, linked copies,
//! audio tools (reverse, sampler conversion, warp), MIDI tools and export.

use super::{App, ClipRef};
use crate::audio::samples;
use crate::model::*;
use std::path::{Path, PathBuf};

fn renders_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("musicton").join("renders")
}

fn stamp() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// File seconds covered by one step of an audio clip.
fn secs_per_step(a: &AudioClip, project_bpm: f32) -> f32 {
    let bpm = if a.warp == WarpMode::Off { project_bpm } else { a.bpm.max(1.0) };
    60.0 / bpm / 4.0
}

/// Notes of a looping clip laid out over `len` steps, shifted by `offset`.
fn unrolled_notes(c: &Clip, len: u32, offset: u32) -> Vec<Note> {
    let mut out = vec![];
    if c.len == 0 {
        return out;
    }
    let mut rep = 0;
    while rep * c.len < len {
        for n in &c.notes {
            let s = rep * c.len + n.step;
            if s < len {
                out.push(Note { step: s + offset, len: n.len.min(len - s), ..*n });
            }
        }
        rep += 1;
    }
    out
}

/// Standard MIDI file (format 0, 96 ticks per quarter) with the clip's notes.
pub fn midi_file(c: &Clip, bpm: f32) -> Vec<u8> {
    const TPQ: u32 = 96;
    let tick = |step: u32| step * TPQ / 4;
    let mut events: Vec<(u32, [u8; 3])> = vec![];
    for n in &c.notes {
        events.push((tick(n.step), [0x90, n.pitch, n.vel.max(1)]));
        events.push((tick(n.step + n.len.max(1)), [0x80, n.pitch, 0]));
    }
    events.sort_by_key(|(t, e)| (*t, e[0] == 0x90));
    let vlq = |mut v: u32, out: &mut Vec<u8>| {
        let mut buf = [0u8; 5];
        let mut i = 4;
        buf[i] = (v & 0x7f) as u8;
        v >>= 7;
        while v > 0 {
            i -= 1;
            buf[i] = (v & 0x7f) as u8 | 0x80;
            v >>= 7;
        }
        out.extend_from_slice(&buf[i..]);
    };
    let mut trk = vec![];
    let us = (60_000_000.0 / bpm.max(1.0)) as u32;
    trk.extend_from_slice(&[0x00, 0xff, 0x51, 0x03, (us >> 16) as u8, (us >> 8) as u8, us as u8]);
    let name = c.name.as_bytes();
    trk.extend_from_slice(&[0x00, 0xff, 0x03]);
    vlq(name.len() as u32, &mut trk);
    trk.extend_from_slice(name);
    let mut last = 0;
    for (t, e) in events {
        vlq(t - last, &mut trk);
        trk.extend_from_slice(&e);
        last = t;
    }
    vlq(tick(c.len).saturating_sub(last), &mut trk);
    trk.extend_from_slice(&[0xff, 0x2f, 0x00]);
    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0, 1, (TPQ >> 8) as u8, TPQ as u8]);
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(trk.len() as u32).to_be_bytes());
    out.extend_from_slice(&trk);
    out
}

impl App {
    fn arr_clip(&self, track: u64, id: u64) -> Option<ArrClip> {
        self.project.track(track)?.arrangement.iter().find(|c| c.id == id).cloned()
    }

    /// The content of a timeline clip from `rel` steps into it (loops are rotated, audio is offset).
    pub(super) fn clip_from(&self, ac: &ArrClip, rel: u32) -> Clip {
        let mut c = ac.clip.clone();
        if c.len == 0 {
            return c;
        }
        let r = rel % c.len;
        if let Some(a) = c.audio.as_mut() {
            a.offset += r as f32 * secs_per_step(a, self.project.bpm);
            c.len -= r;
        } else {
            let len = c.len;
            for n in &mut c.notes {
                n.step = (n.step + len - r) % len;
            }
            c.notes.sort_by_key(|n| n.step);
        }
        c
    }

    /// Splits a timeline clip at `at` (absolute step).
    pub(super) fn split_clip(&mut self, track: u64, id: u64, at: u32) -> Result<u64, String> {
        if self.guard_locked(track) {
            return Err("track is protected".into());
        }
        let ac = self.arr_clip(track, id).ok_or("no such clip")?;
        if at <= ac.start || at >= ac.start + ac.len {
            return Err("the split point is not inside the clip".into());
        }
        let rel = at - ac.start;
        let right = ArrClip { id: self.project.new_id(), start: at, len: ac.len - rel, clip: self.clip_from(&ac, rel) };
        let rid = right.id;
        let t = self.project.track_mut(track).unwrap();
        if let Some(c) = t.arrangement.iter_mut().find(|c| c.id == id) {
            c.len = rel;
        }
        t.arrangement.push(right);
        t.arrangement.sort_by_key(|c| c.start);
        self.touch();
        Ok(rid)
    }

    /// Joins timeline clips of one track into one clip (MIDI: notes merged; audio: rendered).
    pub(super) fn join_clips(&mut self, track: u64, ids: &[u64]) -> Result<u64, String> {
        if self.guard_locked(track) {
            return Err("track is protected".into());
        }
        let t = self.project.track(track).cloned().ok_or("no such track")?;
        let mut parts: Vec<ArrClip> = t.arrangement.iter().filter(|c| ids.contains(&c.id)).cloned().collect();
        if parts.len() < 2 {
            return Err("select at least two clips of the same track (Ctrl-click)".into());
        }
        parts.sort_by_key(|c| c.start);
        let start = parts[0].start;
        let end = parts.iter().map(|c| c.start + c.len).max().unwrap();
        let len = end - start;
        let first = parts[0].clip.clone();
        let clip = if parts.iter().any(|c| c.clip.audio.is_some()) {
            let shifted: Vec<ArrClip> = parts.iter().map(|c| ArrClip { start: c.start - start, ..c.clone() }).collect();
            let a = self.render_region_audio(&t, shifted, 0, len, false, &format!("{} Join", first.name))?;
            Self::rendered_clip(&first, a, len)
        } else {
            let mut c = first.clone();
            c.len = len;
            c.link = 0;
            c.notes = parts.iter().flat_map(|p| unrolled_notes(&p.clip, p.len, p.start - start)).collect();
            c.notes.sort_by_key(|n| n.step);
            c
        };
        let nid = self.project.new_id();
        let tr = self.project.track_mut(track).unwrap();
        tr.arrangement.retain(|c| !ids.contains(&c.id));
        tr.arrangement.push(ArrClip { id: nid, start, len, clip });
        tr.arrangement.sort_by_key(|c| c.start);
        self.arr.multi.clear();
        self.sel_clip = Some(ClipRef::Arr { track, id: nid });
        self.touch();
        Ok(nid)
    }

    /// Trims a timeline clip to the loop region.
    pub(super) fn crop_clip_to_loop(&mut self, track: u64, id: u64) -> Result<(), String> {
        if self.guard_locked(track) {
            return Err("track is protected".into());
        }
        let ac = self.arr_clip(track, id).ok_or("no such clip")?;
        let (ls, le) = (self.project.loop_start, self.project.loop_start + self.project.loop_len);
        let (s, e) = (ac.start.max(ls), (ac.start + ac.len).min(le));
        if s >= e {
            return Err("the clip is outside the loop region".into());
        }
        let clip = self.clip_from(&ac, s - ac.start);
        if let Some(c) = self.project.track_mut(track).and_then(|t| t.arrangement.iter_mut().find(|c| c.id == id)) {
            c.start = s;
            c.len = e - s;
            c.clip = clip;
        }
        self.touch();
        Ok(())
    }

    /// Inserts `len` empty steps at `at` on all tracks (clips, automation, sections, markers, chords move right).
    pub(super) fn insert_time(&mut self, at: u32, len: u32) {
        if len == 0 {
            return;
        }
        // Split clips that cross the insert point.
        let crossing: Vec<(u64, u64)> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.arrangement.iter().filter(|c| c.start < at && c.start + c.len > at).map(move |c| (t.id, c.id)))
            .collect();
        for (t, c) in crossing {
            let locked = self.track_locked(t);
            if let Some(tr) = self.project.track_mut(t) {
                tr.locked = false;
            }
            let _ = self.split_clip(t, c, at);
            if let Some(tr) = self.project.track_mut(t) {
                tr.locked = locked;
            }
        }
        let shift = |p: &mut u32| {
            if *p >= at {
                *p += len;
            }
        };
        for t in &mut self.project.tracks {
            for c in &mut t.arrangement {
                shift(&mut c.start);
            }
            for a in &mut t.automation {
                for pt in &mut a.points {
                    if pt.0 >= at as f32 {
                        pt.0 += len as f32;
                    }
                }
            }
        }
        for s in &mut self.project.sections {
            if s.start >= at {
                s.start += len;
            } else if s.start + s.len > at {
                s.len += len;
            }
        }
        for m in &mut self.project.markers {
            shift(&mut m.pos);
        }
        for c in &mut self.project.chords {
            shift(&mut c.start);
        }
        self.touch();
    }

    /// Removes the steps `at..at+len` on all tracks; everything after moves left.
    pub(super) fn delete_time(&mut self, at: u32, len: u32) {
        if len == 0 {
            return;
        }
        let end = at + len;
        let mut keep_parts: Vec<(u64, ArrClip)> = vec![];
        for ti in 0..self.project.tracks.len() {
            let clips = std::mem::take(&mut self.project.tracks[ti].arrangement);
            let tid = self.project.tracks[ti].id;
            for c in clips {
                let (cs, ce) = (c.start, c.start + c.len);
                if ce <= at {
                    keep_parts.push((tid, c));
                } else if cs >= end {
                    keep_parts.push((tid, ArrClip { start: cs - len, ..c }));
                } else {
                    if cs < at {
                        keep_parts.push((tid, ArrClip { len: at - cs, ..c.clone() }));
                    }
                    if ce > end {
                        let clip = self.clip_from(&c, end - cs);
                        let id = self.project.new_id();
                        keep_parts.push((tid, ArrClip { id, start: at, len: ce - end, clip }));
                    }
                }
            }
        }
        for (tid, c) in keep_parts {
            if let Some(t) = self.project.track_mut(tid) {
                t.arrangement.push(c);
            }
        }
        for t in &mut self.project.tracks {
            t.arrangement.sort_by_key(|c| c.start);
            for a in &mut t.automation {
                a.points.retain(|p| p.0 < at as f32 || p.0 >= end as f32);
                for pt in &mut a.points {
                    if pt.0 >= end as f32 {
                        pt.0 -= len as f32;
                    }
                }
            }
        }
        let map = |p: u32| if p <= at { p } else if p >= end { p - len } else { at };
        for s in &mut self.project.sections {
            let (ns, ne) = (map(s.start), map(s.start + s.len));
            s.start = ns;
            s.len = ne - ns;
        }
        self.project.sections.retain(|s| s.len > 0);
        self.project.markers.retain(|m| m.pos < at || m.pos >= end);
        for m in &mut self.project.markers {
            if m.pos >= end {
                m.pos -= len;
            }
        }
        self.project.chords.retain(|c| c.start < at || c.start >= end);
        for c in &mut self.project.chords {
            if c.start >= end {
                c.start -= len;
            }
        }
        self.touch();
    }

    /// Repeats the steps `at..at+len` right after themselves on all tracks.
    pub(super) fn duplicate_time(&mut self, at: u32, len: u32) {
        if len == 0 {
            return;
        }
        let end = at + len;
        self.insert_time(end, len);
        let mut copies: Vec<(u64, ArrClip)> = vec![];
        for t in &self.project.tracks {
            for c in &t.arrangement {
                let (cs, ce) = (c.start, c.start + c.len);
                if ce <= at || cs >= end {
                    continue;
                }
                let s = cs.max(at);
                let e = ce.min(end);
                copies.push((t.id, ArrClip { id: 0, start: s + len, len: e - s, clip: self.clip_from(c, s - cs) }));
            }
        }
        for (tid, mut c) in copies {
            c.id = self.project.new_id();
            if let Some(t) = self.project.track_mut(tid) {
                t.arrangement.push(c);
            }
        }
        for t in &mut self.project.tracks {
            t.arrangement.sort_by_key(|c| c.start);
            for a in &mut t.automation {
                let dup: Vec<(f32, f32)> = a.points.iter().filter(|p| p.0 >= at as f32 && p.0 < end as f32).map(|p| (p.0 + len as f32, p.1)).collect();
                a.points.extend(dup);
                a.points.sort_by(|x, y| x.0.total_cmp(&y.0));
            }
        }
        self.touch();
    }

    /// The loop region as the time selection for the time commands.
    pub(super) fn loop_range(&self) -> Option<(u32, u32)> {
        (self.project.loop_len > 0).then_some((self.project.loop_start, self.project.loop_len))
    }

    /// Copies a timeline clip right after itself as a linked copy (editing one edits both).
    pub(super) fn duplicate_linked(&mut self, track: u64, id: u64) -> Result<u64, String> {
        let ac = self.arr_clip(track, id).ok_or("no such clip")?;
        let link = if ac.clip.link != 0 { ac.clip.link } else { self.project.new_id() };
        let nid = self.project.new_id();
        let t = self.project.track_mut(track).unwrap();
        if let Some(c) = t.arrangement.iter_mut().find(|c| c.id == id) {
            c.clip.link = link;
        }
        let mut copy = ac.clone();
        copy.id = nid;
        copy.start = ac.start + ac.len;
        copy.clip.link = link;
        t.arrangement.push(copy);
        t.arrangement.sort_by_key(|c| c.start);
        self.resolve_overlaps(track, nid);
        self.touch();
        Ok(nid)
    }

    /// Turns a linked copy into an independent clip.
    pub(super) fn unlink_clip(&mut self, r: ClipRef) {
        if let Some(c) = self.clip_mut(r) {
            c.link = 0;
        }
        self.touch();
    }

    /// Copies the content of the selected clip to its linked copies.
    pub(super) fn sync_linked(&mut self) {
        let Some(r) = self.sel_clip else { return };
        let Some(src) = self.clip(r).cloned() else { return };
        if src.link == 0 {
            return;
        }
        for t in &mut self.project.tracks {
            let all = t.clips.iter_mut().flatten().chain(t.arrangement.iter_mut().map(|a| &mut a.clip));
            for c in all {
                if c.link == src.link && (c.notes != src.notes || c.len != src.len) {
                    c.notes.clone_from(&src.notes);
                    c.len = src.len;
                    c.audio.clone_from(&src.audio);
                    c.gain = src.gain;
                }
            }
        }
    }

    pub(super) fn toggle_clip_mute(&mut self, r: ClipRef) {
        if self.guard_locked(r.track()) {
            return;
        }
        if let Some(c) = self.clip_mut(r) {
            c.muted = !c.muted;
        }
        self.touch();
    }

    /// Extends a timeline clip so it repeats up to the next clip (or 4 loops when nothing follows).
    pub(super) fn loop_clip(&mut self, track: u64, id: u64) {
        let Some(ac) = self.arr_clip(track, id) else { return };
        let next = self.project.track(track).and_then(|t| t.arrangement.iter().filter(|c| c.start > ac.start).map(|c| c.start).min());
        let len = match next {
            Some(n) => n - ac.start,
            None => ac.clip.len.max(1) * 4,
        };
        if let Some(c) = self.project.track_mut(track).and_then(|t| t.arrangement.iter_mut().find(|c| c.id == id)) {
            c.len = len.max(1);
        }
        self.touch();
    }

    /// Back to a single pass of the clip.
    pub(super) fn unloop_clip(&mut self, track: u64, id: u64) {
        if let Some(c) = self.project.track_mut(track).and_then(|t| t.arrangement.iter_mut().find(|c| c.id == id)) {
            c.len = c.clip.len.max(1);
        }
        self.touch();
    }

    // -----------------------------------------------------------------------------------------
    // Audio tools

    /// The used part of an audio clip's file (from its offset, one loop long).
    fn audio_segment(&self, c: &Clip) -> Result<(Vec<f32>, Vec<f32>, f32), String> {
        let a = c.audio.as_ref().ok_or("not an audio clip")?;
        let s = samples::load(&a.path)?;
        let sps = secs_per_step(a, self.project.bpm);
        let from = ((a.offset * s.sample_rate) as usize).min(s.left.len());
        let to = (from + (c.len as f32 * sps * s.sample_rate) as usize).min(s.left.len());
        Ok((s.left[from..to].to_vec(), s.right[from..to].to_vec(), s.sample_rate))
    }

    fn write_stereo(path: &Path, l: &[f32], r: &[f32], sr: f32) -> Result<String, String> {
        let data: Vec<f32> = l.iter().zip(r).flat_map(|(a, b)| [*a, *b]).collect();
        crate::audio::write_wav(path, &data, sr as u32)?;
        let p = path.to_string_lossy().to_string();
        samples::load(&p)?;
        Ok(p)
    }

    /// Plays an audio clip backwards (writes a reversed copy of the used audio).
    pub(super) fn reverse_audio_clip(&mut self, r: ClipRef) -> Result<(), String> {
        if self.guard_locked(r.track()) {
            return Err("track is protected".into());
        }
        let c = self.clip(r).cloned().ok_or("no clip")?;
        let (mut l, mut rr, sr) = self.audio_segment(&c)?;
        l.reverse();
        rr.reverse();
        std::fs::create_dir_all(renders_dir()).map_err(|e| e.to_string())?;
        let path = renders_dir().join(format!("{} reversed {}.wav", c.name.replace(['/', '\\'], "_"), stamp()));
        let p = Self::write_stereo(&path, &l, &rr, sr)?;
        if let Some(c) = self.clip_mut(r) {
            if let Some(a) = c.audio.as_mut() {
                a.path = p;
                a.offset = 0.0;
            }
            if !c.name.ends_with(" (rev)") {
                c.name.push_str(" (rev)");
            }
        }
        self.touch();
        Ok(())
    }

    pub(super) fn set_warp(&mut self, r: ClipRef, warp: WarpMode) {
        if let Some(a) = self.clip_mut(r).and_then(|c| c.audio.as_mut()) {
            a.warp = warp;
        }
        self.touch();
    }

    /// Guesses the tempo from the clip length (a whole number of bars) and follows the project tempo.
    pub(super) fn fit_audio_to_bars(&mut self, r: ClipRef) -> Result<String, String> {
        let c = self.clip(r).cloned().ok_or("no clip")?;
        let a = c.audio.clone().ok_or("not an audio clip")?;
        let s = samples::load(&a.path)?;
        let frames = s.left.len() - ((a.offset * s.sample_rate) as usize).min(s.left.len());
        let fitted = Clip::from_audio(&a.path, frames, s.sample_rate, self.project.bpm, c.color);
        let fa = fitted.audio.clone().unwrap();
        if let Some(cm) = self.clip_mut(r) {
            if let Some(x) = cm.audio.as_mut() {
                x.bpm = fa.bpm;
                x.warp = if fa.warp == WarpMode::Off { WarpMode::Beats } else { fa.warp };
            }
            cm.len = fitted.len;
        }
        self.touch();
        Ok(format!("Tempo set to {:.1} BPM ({} bars)", fa.bpm, fitted.len / STEPS_PER_BAR))
    }

    /// New Slicer, Beat Pads or Multi Sampler track playing the clip's audio.
    pub(super) fn convert_to_sampler(&mut self, r: ClipRef, kind: InstrumentKind) -> Result<u64, String> {
        let c = self.clip(r).cloned().ok_or("no clip")?;
        let (l, rr, sr) = self.audio_segment(&c)?;
        std::fs::create_dir_all(renders_dir()).map_err(|e| e.to_string())?;
        let base = c.name.replace(['/', '\\'], "_");
        let whole = Self::write_stereo(&renders_dir().join(format!("{base} {}.wav", stamp())), &l, &rr, sr)?;
        let src = r.track();
        let at = self.project.track_index(src).map(|i| i + 1);
        let preset = match kind {
            InstrumentKind::Slicer => "Slicer",
            InstrumentKind::BeatPads => "Beat Pads",
            _ => "Multi Sampler",
        };
        let tid = self.add_track_from_preset(preset, at);
        if self.project.track(tid).is_some_and(|t| t.instrument.kind != kind) {
            if let Some(t) = self.project.track_mut(tid) {
                t.instrument = Instrument::new(kind);
            }
        }
        let smp = samples::load(&whole)?;
        let slices = crate::audio::samplers::detect_slices(&smp, 0, 0.5);
        let mut clip = Clip::new(&format!("{} Slices", c.name), c.color, 1);
        clip.len = c.len;
        match kind {
            InstrumentKind::Slicer => {
                if let Some(t) = self.project.track_mut(tid) {
                    t.instrument.samples = vec![Some(whole.clone())];
                    let p = &mut t.instrument.params;
                    if p.len() > crate::params::slicer::SYNC {
                        p[crate::params::slicer::BEATS] = c.len as f32 / 4.0;
                        p[crate::params::slicer::SYNC] = 1.0;
                    }
                }
                clip.notes = crate::audio::samplers::slice_notes(&slices, smp.left.len(), c.len);
            }
            InstrumentKind::BeatPads => {
                // One WAV per slice on the pads (up to 16), triggered in the original rhythm.
                let mut bounds: Vec<usize> = slices.iter().copied().take(16).collect();
                bounds.push(smp.left.len());
                let mut pads = Pad::default_set();
                for k in 0..bounds.len() - 1 {
                    let (a, b) = (bounds[k], bounds[k + 1]);
                    let p = Self::write_stereo(&renders_dir().join(format!("{base} slice {} {}.wav", k + 1, stamp())), &smp.left[a..b], &smp.right[a..b], smp.sample_rate)?;
                    pads[k].sample = Some(p);
                    pads[k].name = format!("Slice {}", k + 1);
                    pads[k].choke = 0;
                    let step = ((a as f64 / smp.left.len().max(1) as f64) * c.len as f64).round() as u32;
                    let next = ((b as f64 / smp.left.len().max(1) as f64) * c.len as f64).round() as u32;
                    clip.notes.push(Note { step: step.min(c.len.saturating_sub(1)), len: (next - step).max(1), pitch: 36 + k as u8, vel: 100, slide: false, skip: 0 });
                }
                if let Some(t) = self.project.track_mut(tid) {
                    t.instrument.pads = pads;
                }
            }
            _ => {
                if let Some(t) = self.project.track_mut(tid) {
                    let mut z = Zone::new(&whole, 60);
                    z.lo = 0;
                    z.hi = 127;
                    t.instrument.zones = vec![z];
                }
                clip.notes = vec![Note { step: 0, len: c.len, pitch: 60, vel: 100, slide: false, skip: 0 }];
            }
        }
        if let Some(t) = self.project.track_mut(tid) {
            t.name = format!("{} {}", c.name, kind.name());
            clip.color = t.color;
        }
        self.place_clip_like(r, tid, clip);
        if let Some(t) = self.project.track_mut(src) {
            t.channel.mute = true;
        }
        self.touch();
        Ok(tid)
    }

    /// Puts `clip` on `track` at the same spot (scene or timeline position) as `r`.
    pub(super) fn place_clip_like(&mut self, r: ClipRef, track: u64, clip: Clip) {
        match r {
            ClipRef::Session { scene, .. } => {
                if let Some(t) = self.project.track_mut(track) {
                    if scene < t.clips.len() {
                        t.clips[scene] = Some(clip);
                    }
                }
            }
            ClipRef::Arr { track: src, id } => {
                if let Some(ac) = self.arr_clip(src, id) {
                    let nid = self.project.new_id();
                    if let Some(t) = self.project.track_mut(track) {
                        t.arrangement.push(ArrClip { id: nid, start: ac.start, len: ac.len, clip });
                    }
                }
            }
        }
    }

    /// Adds pitch correction (timing stays with the warp modes) at the start of the track's chain.
    pub(super) fn add_pitch_correction(&mut self, track: u64) {
        self.add_device(super::Sel::Track(track), DeviceKind::PitchFix, Some(0));
    }

    // -----------------------------------------------------------------------------------------
    // MIDI tools

    pub(super) fn edit_notes(&mut self, r: ClipRef, f: impl FnOnce(&mut Clip)) {
        if self.guard_locked(r.track()) {
            return;
        }
        if let Some(c) = self.clip_mut(r) {
            f(c);
            c.notes.sort_by_key(|n| n.step);
        }
        self.sync_linked_from(r);
        self.touch();
    }

    fn sync_linked_from(&mut self, r: ClipRef) {
        let prev = self.sel_clip;
        self.sel_clip = Some(r);
        self.sync_linked();
        self.sel_clip = prev;
    }

    pub(super) fn quantize_clip(&mut self, r: ClipRef, grid: u32) {
        let g = grid.max(1);
        self.edit_notes(r, |c| {
            let len = c.len.max(1);
            for n in &mut c.notes {
                n.step = ((n.step + g / 2) / g * g) % len;
            }
        });
    }

    /// Every note lasts until the next one starts.
    pub(super) fn legato_clip(&mut self, r: ClipRef) {
        self.edit_notes(r, |c| {
            c.notes.sort_by_key(|n| n.step);
            let starts: Vec<u32> = c.notes.iter().map(|n| n.step).collect();
            let len = c.len;
            for n in &mut c.notes {
                let next = starts.iter().copied().find(|s| *s > n.step).unwrap_or(len);
                n.len = (next - n.step).max(1);
            }
        });
    }

    pub(super) fn scale_note_lengths(&mut self, r: ClipRef, factor: f32) {
        self.edit_notes(r, |c| {
            for n in &mut c.notes {
                n.len = ((n.len as f32 * factor).round() as u32).max(1);
            }
        });
    }

    pub(super) fn set_velocities(&mut self, r: ClipRef, vel: u8) {
        self.edit_notes(r, |c| {
            for n in &mut c.notes {
                n.vel = vel.clamp(1, 127);
            }
        });
    }

    /// Pulls velocities halfway towards their average (more even dynamics).
    pub(super) fn compress_velocities(&mut self, r: ClipRef) {
        self.edit_notes(r, |c| {
            if c.notes.is_empty() {
                return;
            }
            let avg = c.notes.iter().map(|n| n.vel as f32).sum::<f32>() / c.notes.len() as f32;
            for n in &mut c.notes {
                n.vel = ((n.vel as f32 + avg) / 2.0).round().clamp(1.0, 127.0) as u8;
            }
        });
    }

    pub(super) fn humanize_velocities(&mut self, r: ClipRef) {
        self.edit_notes(r, |c| {
            let mut seed = 0x9e37_79b9u32 ^ c.notes.len() as u32;
            for n in &mut c.notes {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                n.vel = (n.vel as i32 + (seed % 25) as i32 - 12).clamp(1, 127) as u8;
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // Export

    /// Renders the clip through its track (with effects) into `path`.
    pub(super) fn export_clip_audio(&mut self, r: ClipRef, path: &Path, as_loop: bool) -> Result<String, String> {
        let t = self.project.track(r.track()).cloned().ok_or("no track")?;
        let c = self.clip(r).cloned().ok_or("no clip")?;
        let len = match r {
            ClipRef::Arr { track, id } if !as_loop => self.arr_clip(track, id).map(|a| a.len).unwrap_or(c.len),
            _ => c.len,
        };
        let a = self.render_clip_audio(&t, &c, len, true, as_loop)?;
        let mut path = path.to_path_buf();
        if as_loop {
            // Loops carry their tempo in the name so other tools (and the browser) can sync them.
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| c.name.clone());
            let tag = format!("{:.0}bpm", self.project.bpm);
            if !stem.contains(&tag) {
                path.set_file_name(format!("{stem} {tag}.wav"));
            }
        }
        std::fs::copy(&a.path, &path).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
    }

    pub(super) fn export_clip_midi(&mut self, r: ClipRef, path: &Path) -> Result<String, String> {
        let c = self.clip(r).cloned().ok_or("no clip")?;
        if c.audio.is_some() {
            return Err("audio clips have no notes - use Audio to MIDI first".into());
        }
        std::fs::write(path, midi_file(&c, self.project.bpm)).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
    }
}
