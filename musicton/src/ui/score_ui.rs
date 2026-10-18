//! Score view: the clip in standard notation on a grand staff (read-only), MusicXML export.

use super::theme::*;
use super::{App, ClipRef};
use crate::score::{key_signature, score_events, sig_accidental, spell, staff_pos};
use egui::{pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke};

const GAP: f32 = 9.0;
/// Staff position drawn at the top reference line (F5).
const TOP_POS: i32 = 38;

fn sharp(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.2_f32, col);
    p.line_segment([c + vec2(-1.5, -6.0), c + vec2(-1.5, 6.0)], s);
    p.line_segment([c + vec2(1.5, -6.5), c + vec2(1.5, 5.5)], s);
    p.line_segment([c + vec2(-3.5, -1.0), c + vec2(3.5, -2.5)], Stroke::new(2.0_f32, col));
    p.line_segment([c + vec2(-3.5, 2.5), c + vec2(3.5, 1.0)], Stroke::new(2.0_f32, col));
}

fn flat(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.2_f32, col);
    p.line_segment([c + vec2(-2.0, -8.0), c + vec2(-2.0, 3.0)], s);
    let pts: Vec<Pos2> = (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0 * std::f32::consts::PI;
            c + vec2(-2.0 + 4.0 * t.sin(), 3.0 - 2.5 * (1.0 - t.cos()))
        })
        .collect();
    p.add(Shape::line(pts, s));
}

fn natural(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.2_f32, col);
    p.line_segment([c + vec2(-2.0, -6.0), c + vec2(-2.0, 3.0)], s);
    p.line_segment([c + vec2(2.0, -3.0), c + vec2(2.0, 6.0)], s);
    p.line_segment([c + vec2(-2.0, 3.0), c + vec2(2.0, 1.5)], Stroke::new(2.0_f32, col));
    p.line_segment([c + vec2(-2.0, -1.5), c + vec2(2.0, -3.0)], Stroke::new(2.0_f32, col));
}

impl App {
    pub(super) fn score_view(&mut self, ui: &mut egui::Ui, r: ClipRef) {
        let Some(clip) = self.clip(r).cloned() else { return };
        let sig = key_signature(self.project.key, self.project.scale);
        let events = score_events(&clip);
        let bars = clip.len.max(1).div_ceil(16);
        let avail = ui.available_size();
        let header = 64.0 + sig.unsigned_abs() as f32 * 8.0;
        let bar_w = 180.0f32.max((avail.x - header - 10.0) / bars.min(4) as f32);
        let per_line = (((avail.x - header - 10.0) / bar_w).floor() as u32).max(1);
        let lines = bars.div_ceil(per_line);
        let system_h = 22.0 * GAP;
        let playhead = self.playhead_step(r);
        egui::ScrollArea::vertical().id_salt("score").auto_shrink([false, false]).show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(avail.x.max(300.0), lines as f32 * system_h + 10.0), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 2.0, Color32::from_rgb(0xf4, 0xf1, 0xe8));
            let ink = Color32::from_rgb(0x1c, 0x1c, 0x1c);
            let thin = Stroke::new(1.0_f32, ink);
            for line in 0..lines {
                let y0 = rect.top() + 4.0 * GAP + line as f32 * system_h;
                let y_of = |pos: i32| y0 + (TOP_POS - pos) as f32 * GAP / 2.0;
                let first_bar = line * per_line;
                let nbars = per_line.min(bars - first_bar);
                let x0 = rect.left() + 8.0;
                let x1 = x0 + header + nbars as f32 * bar_w;
                // Staff lines: treble E4..F5 (30..38), bass G2..A3 (18..26).
                for pos in (30..=38).step_by(2).chain((18..=26).step_by(2)) {
                    p.line_segment([pos2(x0, y_of(pos)), pos2(x1, y_of(pos))], thin);
                }
                p.line_segment([pos2(x0, y_of(38)), pos2(x0, y_of(18))], Stroke::new(1.5_f32, ink));
                // Clefs (letter forms) and key signature.
                p.text(pos2(x0 + 4.0, y_of(32)), Align2::LEFT_CENTER, "G", FontId::proportional(30.0), ink);
                p.text(pos2(x0 + 4.0, y_of(24)), Align2::LEFT_CENTER, "F", FontId::proportional(24.0), ink);
                const SHARP_POS: [i32; 7] = [38, 35, 39, 36, 33, 37, 34];
                const FLAT_POS: [i32; 7] = [34, 37, 33, 36, 32, 35, 31];
                for k in 0..sig.unsigned_abs() as usize {
                    let x = x0 + 30.0 + k as f32 * 8.0;
                    for off in [0, -14] {
                        if sig > 0 {
                            sharp(&p, pos2(x, y_of(SHARP_POS[k] + off)), ink);
                        } else {
                            flat(&p, pos2(x, y_of(FLAT_POS[k] + off)), ink);
                        }
                    }
                }
                if line == 0 {
                    p.text(pos2(x0 + 2.0, y0 - 2.5 * GAP), Align2::LEFT_BOTTOM, format!("Tempo {:.0}", self.project.bpm), FontId::proportional(11.0), ink);
                }
                let cell = bar_w / 16.0;
                let x_of = |step: u32| x0 + header + ((step - first_bar * 16) as f32 + 0.5) * cell;
                for b in 0..=nbars {
                    let x = x0 + header + b as f32 * bar_w;
                    p.line_segment([pos2(x, y_of(38)), pos2(x, y_of(18))], if b == nbars && first_bar + nbars == bars { Stroke::new(2.5_f32, ink) } else { thin });
                    if b < nbars {
                        p.text(pos2(x + 2.0, y_of(38) - 4.0), Align2::LEFT_BOTTOM, format!("{}", first_bar + b + 1), FontId::proportional(9.0), Color32::GRAY);
                    }
                }
                // Accidentals already shown in a bar (letter+octave -> alteration).
                let mut bar_acc: std::collections::HashMap<(u32, i32), i32> = Default::default();
                for e in events.iter().filter(|e| e.start / 16 >= first_bar && e.start / 16 < first_bar + nbars) {
                    let x = x_of(e.start);
                    let (_, dotted) = match e.dur {
                        12 | 6 | 3 => (0, true),
                        _ => (0, false),
                    };
                    if e.pitches.is_empty() {
                        // Rests in the treble staff.
                        let mid = y_of(34);
                        match e.dur {
                            16 => {
                                p.rect_filled(Rect::from_min_size(pos2(x - 4.0, y_of(36)), vec2(8.0, GAP / 2.0)), 0.0, ink);
                            }
                            8 | 12 => {
                                p.rect_filled(Rect::from_min_size(pos2(x - 4.0, y_of(34) - GAP / 2.0), vec2(8.0, GAP / 2.0)), 0.0, ink);
                            }
                            4 | 6 => {
                                let pts = vec![pos2(x - 2.0, mid - 8.0), pos2(x + 2.0, mid - 4.0), pos2(x - 2.0, mid), pos2(x + 2.0, mid + 4.0), pos2(x - 1.0, mid + 7.0)];
                                p.add(Shape::line(pts, Stroke::new(2.0_f32, ink)));
                            }
                            _ => {
                                let flags = if e.dur == 1 { 2 } else { 1 };
                                p.line_segment([pos2(x + 3.0, mid - 5.0), pos2(x - 1.0, mid + 7.0)], Stroke::new(1.3_f32, ink));
                                for k in 0..flags {
                                    p.circle_filled(pos2(x - 1.0, mid - 4.0 + k as f32 * 4.0), 1.6, ink);
                                }
                            }
                        }
                        if dotted {
                            p.circle_filled(pos2(x + 7.0, mid), 1.4, ink);
                        }
                        continue;
                    }
                    let poss: Vec<i32> = e.pitches.iter().map(|&pt| staff_pos(pt, sig)).collect();
                    let filled = e.dur <= 6;
                    let up = (poss.iter().sum::<i32>() as f32 / poss.len() as f32) < if poss[0] >= 28 { 34.0 } else { 22.0 };
                    for (&pt, &pos) in e.pitches.iter().zip(&poss) {
                        let y = y_of(pos);
                        // Ledger lines.
                        let ledgers: Vec<i32> = if pos >= 40 {
                            (40..=pos).step_by(2).collect()
                        } else if pos <= 16 {
                            (pos..=16).filter(|q| q % 2 == 0).collect()
                        } else if pos == 28 || pos == 29 {
                            vec![28]
                        } else {
                            vec![]
                        };
                        for l in ledgers {
                            p.line_segment([pos2(x - 7.0, y_of(l)), pos2(x + 7.0, y_of(l))], thin);
                        }
                        let head = Rect::from_center_size(pos2(x, y), vec2(9.0, GAP * 0.9));
                        if filled {
                            p.add(Shape::ellipse_filled(head.center(), head.size() / 2.0, ink));
                        } else {
                            p.add(Shape::ellipse_stroke(head.center(), head.size() / 2.0, Stroke::new(1.4_f32, ink)));
                        }
                        if dotted {
                            p.circle_filled(pos2(x + 8.0, y - if pos % 2 == 0 { GAP / 2.0 } else { 0.0 }), 1.4, ink);
                        }
                        // Accidentals against the key signature and earlier notes in the bar.
                        let (letter, acc) = spell(pt, sig);
                        let key = (e.start / 16, pos);
                        let current = *bar_acc.get(&key).unwrap_or(&sig_accidental(letter, sig));
                        if acc != current && !e.tie_stop {
                            let c = pos2(x - 11.0, y);
                            match acc {
                                1 => sharp(&p, c, ink),
                                -1 => flat(&p, c, ink),
                                _ => natural(&p, c, ink),
                            }
                            bar_acc.insert(key, acc);
                        }
                    }
                    if e.dur < 16 {
                        let (lo, hi) = (*poss.iter().min().unwrap(), *poss.iter().max().unwrap());
                        let (sx, y_start, y_end) = if up { (x + 4.0, y_of(lo), y_of(hi) - 3.5 * GAP) } else { (x - 4.0, y_of(hi), y_of(lo) + 3.5 * GAP) };
                        p.line_segment([pos2(sx, y_start), pos2(sx, y_end)], Stroke::new(1.2_f32, ink));
                        let flags = match e.dur {
                            1 => 2,
                            2 | 3 => 1,
                            _ => 0,
                        };
                        for k in 0..flags {
                            let fy = y_end + if up { k as f32 * 5.0 } else { -(k as f32) * 5.0 };
                            p.line_segment([pos2(sx, fy), pos2(sx + 6.0, fy + if up { 7.0 } else { -7.0 })], Stroke::new(1.6_f32, ink));
                        }
                    }
                    if e.tie_start {
                        let y = y_of(poss[0]) + if up { 6.0 } else { -6.0 };
                        let w = e.dur as f32 * cell;
                        let pts: Vec<Pos2> = (0..=8).map(|i| {
                            let t = i as f32 / 8.0;
                            pos2(x + 4.0 + t * (w - 6.0), y + (t * std::f32::consts::PI).sin() * if up { 4.0 } else { -4.0 })
                        }).collect();
                        p.add(Shape::line(pts, thin));
                    }
                }
                if let Some(ph) = playhead {
                    if (ph as u32) / 16 >= first_bar && (ph as u32) / 16 < first_bar + nbars {
                        let x = x0 + header + (ph - (first_bar * 16) as f32) * cell;
                        p.line_segment([pos2(x, y_of(40)), pos2(x, y_of(16))], Stroke::new(1.5_f32, ACCENT));
                    }
                }
            }
            self.hint(&resp, "Score", "The clip in standard notation (grand staff, key signature from the project key). Notes starting together form chords; long notes are tied across bar lines. Export MusicXML to continue in a notation program.");
        });
    }

    pub(super) fn export_musicxml(&mut self, r: ClipRef) {
        let Some(clip) = self.clip(r).cloned() else { return };
        let Some(path) = rfd::FileDialog::new().add_filter("MusicXML", &["musicxml", "xml"]).set_file_name(format!("{}.musicxml", clip.name)).save_file() else { return };
        let xml = crate::score::to_musicxml(&clip, &self.project.name, self.project.bpm, self.project.key, self.project.scale);
        match std::fs::write(&path, xml) {
            Ok(()) => self.toast(format!("Exported {}", path.display())),
            Err(e) => self.toast(format!("Export failed: {e}")),
        }
    }
}
