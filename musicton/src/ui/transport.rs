//! Top transport/control bar.

use super::theme::*;
use super::widgets;
use super::{export, App, View};
use crate::audio::engine::Cmd;
use crate::midi::Midi;
use crate::model::{MapTarget, NOTE_NAMES, SCALES, STEPS_PER_BAR};
use egui::{pos2, vec2, Align2, FontId, Sense, Shape, Stroke};

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    ToStart,
    Play,
    Stop,
    Rec,
    Cycle,
    Click,
}

/// Rounded transport button with a drawn symbol.
fn icon_button(ui: &mut egui::Ui, icon: Icon, active: bool, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 28.0), Sense::click());
    let p = ui.painter();
    let (top, bottom) = if active && icon != Icon::Play && icon != Icon::Rec {
        (lighten(color, 0.15), darken(color, 0.8))
    } else if resp.hovered() {
        (egui::Color32::from_rgb(0x4c, 0x50, 0x59), egui::Color32::from_rgb(0x38, 0x3b, 0x42))
    } else {
        (egui::Color32::from_rgb(0x3e, 0x41, 0x48), egui::Color32::from_rgb(0x2a, 0x2c, 0x31))
    };
    p.rect_filled(rect, 5.0, bottom);
    p.rect_filled(egui::Rect::from_min_max(rect.min, pos2(rect.right(), rect.center().y)), egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, lerp(top, bottom, 0.3));
    p.rect_stroke(rect, 5.0, Stroke::new(1.0_f32, egui::Color32::from_rgb(0x14, 0x15, 0x18)), egui::StrokeKind::Inside);
    let c = rect.center();
    let on_bg = active && icon != Icon::Play && icon != Icon::Rec;
    let col = if on_bg { egui::Color32::WHITE } else if active { color } else { TEXT };
    match icon {
        Icon::ToStart => {
            p.rect_filled(egui::Rect::from_center_size(c + vec2(-5.0, 0.0), vec2(2.0, 11.0)), 0.0, col);
            p.add(Shape::convex_polygon(vec![c + vec2(5.0, -6.0), c + vec2(5.0, 6.0), c + vec2(-3.5, 0.0)], col, Stroke::NONE));
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![c + vec2(-5.0, -7.0), c + vec2(7.0, 0.0), c + vec2(-5.0, 7.0)], col, Stroke::NONE));
        }
        Icon::Stop => {
            p.rect_filled(egui::Rect::from_center_size(c, vec2(11.0, 11.0)), 1.5, col);
        }
        Icon::Rec => {
            p.circle_filled(c, 6.5, if active { color } else { darken(REC, 0.8) });
        }
        Icon::Cycle => {
            let st = Stroke::new(2.0_f32, col);
            p.line_segment([c + vec2(-8.0, -3.0), c + vec2(6.0, -3.0)], st);
            p.line_segment([c + vec2(8.0, 3.0), c + vec2(-6.0, 3.0)], st);
            p.add(Shape::convex_polygon(vec![c + vec2(5.0, -7.0), c + vec2(9.0, -3.0), c + vec2(5.0, 1.0)], col, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![c + vec2(-5.0, 7.0), c + vec2(-9.0, 3.0), c + vec2(-5.0, -1.0)], col, Stroke::NONE));
        }
        Icon::Click => {
            p.add(Shape::convex_polygon(vec![c + vec2(-6.0, 7.0), c + vec2(6.0, 7.0), c + vec2(2.5, -7.0), c + vec2(-2.5, -7.0)], egui::Color32::TRANSPARENT, Stroke::new(1.5_f32, col)));
            p.line_segment([c + vec2(0.0, 4.0), c + vec2(5.0, -5.0)], Stroke::new(1.5_f32, col));
        }
    }
    resp
}

impl App {
    pub(super) fn transport_bar(&mut self, ui: &mut egui::Ui) {
        let bar = ui.max_rect().expand2(vec2(8.0, 6.0));
        gradient(ui.painter(), bar, egui::Color32::from_rgb(0x36, 0x39, 0x40), egui::Color32::from_rgb(0x25, 0x27, 0x2c));
        ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0_f32, LINE));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.menu_button(egui::RichText::new("☰").size(16.0), |ui| self.main_menu(ui));
            ui.add_space(4.0);
            // Panels.
            let r = widgets::toggle(ui, "Inspector", self.show_inspector, ACCENT, vec2(68.0, 24.0));
            if r.clicked() {
                self.show_inspector = !self.show_inspector;
            }
            self.hint(&r, "Inspector", "Shows or hides the Inspector on the left: clip settings, the channel strip of the selected track and the master, and the quick help.");
            let r = widgets::toggle(ui, "Editor", self.detail_visible(), ACCENT, vec2(52.0, 24.0));
            if r.clicked() {
                self.toggle_detail();
            }
            self.hint(&r, "Editor", "Shows or hides the editor at the bottom (notes or devices; Shift+Tab switches).");
            let r = widgets::toggle(ui, "Browser", self.show_browser, ACCENT, vec2(60.0, 24.0));
            if r.clicked() {
                self.show_browser = !self.show_browser;
            }
            self.hint(&r, "Browser", "Shows or hides the browser on the right with instruments, effects, clips, samples and plug-ins.");

            // Transport, centred in the bar.
            let lcd_w = 430.0;
            let group_w = 6.0 * 38.0 + 12.0 + lcd_w + 150.0;
            let free = ui.available_width() - 330.0;
            ui.add_space(((free - group_w) / 2.0).max(8.0));
            let r = icon_button(ui, Icon::ToStart, false, TEXT);
            if r.clicked() {
                self.send(Cmd::Stop);
                self.send(Cmd::Stop);
            }
            self.hint(&r, "Go to Beginning", "Stops and moves the playhead back to the start.");
            let r = icon_button(ui, Icon::Stop, false, TEXT);
            if r.clicked() {
                self.send(Cmd::Stop);
            }
            r.context_menu(|ui| self.map_menu(ui, MapTarget::Stop, None));
            self.hint(&r, "Stop", "Stops playback. Press again to return the start marker to the beginning.");
            let r = icon_button(ui, Icon::Play, self.status.playing, PLAY);
            if r.clicked() {
                self.send(Cmd::Play);
            }
            r.context_menu(|ui| self.map_menu(ui, MapTarget::Play, None));
            self.hint(&r, "Play", "Starts playback from the start marker (Space).");
            let r = icon_button(ui, Icon::Rec, self.rec, REC);
            if r.clicked() {
                self.rec = !self.rec;
            }
            self.hint(&r, "Record", "When enabled, notes you play (computer keys or MIDI) are recorded into the playing clip of the selected track. If nothing is playing, a new clip is created (F9).");
            let r = icon_button(ui, Icon::Cycle, self.project.loop_on, CYCLE);
            if r.clicked() {
                self.project.loop_on = !self.project.loop_on;
                self.touch();
            }
            self.hint(&r, "Cycle", "Repeats the cycle region of the timeline (drag it in the ruler).");
            let r = icon_button(ui, Icon::Click, self.metronome, ACCENT);
            if r.clicked() {
                self.metronome = !self.metronome;
                self.send(Cmd::Metronome(self.metronome));
            }
            self.hint(&r, "Metronome", "Toggles the click (M).");
            ui.add_space(6.0);
            self.lcd(ui, lcd_w);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    let r = widgets::toggle(ui, "Tap", false, ACCENT, vec2(34.0, 18.0));
                    if r.clicked() {
                        self.tap_tempo();
                    }
                    self.hint(&r, "Tap Tempo", "Click repeatedly in time to set the tempo.");
                    let r = widgets::toggle(ui, "Capture", false, ACCENT, vec2(56.0, 18.0));
                    if r.clicked() {
                        self.capture_midi();
                    }
                    self.hint(&r, "Capture", "Turns what you just played on the selected track into a new clip - even if Record was off.");
                    let r = widgets::toggle(ui, "Rec Timeline", self.arr_rec, REC, vec2(84.0, 18.0));
                    if r.clicked() {
                        self.arr_rec = !self.arr_rec;
                    }
                    self.hint(&r, "Timeline Record", "Records your Launcher performance into the Timeline: every clip you launch is written onto the timeline at the current position.");
                });
                ui.horizontal(|ui| {
                    let mut swing = self.project.swing * 100.0;
                    let r = ui.add(egui::DragValue::new(&mut swing).range(0.0..=100.0).speed(0.5).fixed_decimals(0).prefix("Swing ").suffix("%"));
                    if r.changed() {
                        self.project.swing = swing / 100.0;
                        self.touch();
                    }
                    self.hint(&r, "Swing", "Swing amount applied to every 16th note offbeat.");
                    let quants = [(0u32, "Launch: None"), (1, "Launch: 1/16"), (4, "Launch: 1/4"), (8, "Launch: 1/2"), (16, "Launch: 1 Bar"), (32, "Launch: 2 Bars"), (64, "Launch: 4 Bars")];
                    let cur = quants.iter().find(|q| q.0 == self.project.quant).map(|q| q.1).unwrap_or("Launch: 1 Bar");
                    let r = egui::ComboBox::from_id_salt("quant")
                        .width(96.0)
                        .selected_text(cur)
                        .show_ui(ui, |ui| {
                            for (v, name) in quants {
                                if ui.selectable_label(self.project.quant == v, name).clicked() {
                                    self.project.quant = v;
                                    self.touch();
                                }
                            }
                        })
                        .response;
                    self.hint(&r, "Launch Quantization", "Launched clips start on the next multiple of this value.");
                });
            });
            let overridden = self.status.tracks.iter().any(|t| !t.follow_arr) && self.project.arrangement_end() > 0;
            if overridden {
                let r = widgets::toggle(ui, "Back to Timeline", true, ACCENT, vec2(116.0, 24.0));
                if r.clicked() {
                    self.send(Cmd::BackToArrangement);
                }
                self.hint(&r, "Back to Timeline", "Some tracks play Launcher clips instead of the Timeline. Click to return all of them.");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // View switch as one segmented control.
                for (v, label, title, body) in [
                    (View::Mixer, "Mixer", "Mixer", "Full mixer with channel strips (Ctrl+M)."),
                    (View::Session, "Launcher", "Launcher", "Clip grid for jamming and live performance (Tab)."),
                    (View::Arrangement, "Tracks", "Tracks", "The timeline with your tracks and regions (Tab)."),
                ] {
                    let r = widgets::toggle(ui, label, self.view == v, ACCENT, vec2(70.0, 24.0));
                    if r.clicked() {
                        self.view = v;
                    }
                    self.hint(&r, title, body);
                }
                ui.add_space(10.0);
                let cpu = (self.status.cpu * 100.0).min(999.0);
                let (rect, r) = ui.allocate_exact_size(vec2(64.0, 24.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(rect, 4.0, LCD_BG);
                let fill = egui::Rect::from_min_size(rect.min + vec2(3.0, 15.0), vec2((rect.width() - 6.0) * (cpu / 100.0).clamp(0.0, 1.0), 5.0));
                p.rect_filled(fill, 2.0, if cpu > 80.0 { REC } else { PLAY });
                p.text(rect.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, format!("CPU {cpu:.0}%"), FontId::proportional(10.0), LCD_TEXT);
                self.hint(&r, "CPU Load", "How much of the available audio processing time is used.");
            });
        });
    }

    /// The central display: position, time, tempo, signature and key.
    fn lcd(&mut self, ui: &mut egui::Ui, w: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(rect, 6.0, LCD_BG);
        p.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, egui::Color32::from_rgb(0x05, 0x07, 0x0a)), egui::StrokeKind::Inside);
        gradient(&p, egui::Rect::from_min_max(rect.min + vec2(6.0, 1.0), pos2(rect.right() - 6.0, rect.top() + 8.0)), egui::Color32::from_white_alpha(10), egui::Color32::TRANSPARENT);
        let dim = egui::Color32::from_rgb(0x5e, 0x77, 0x92);
        let pos = self.status.arr_pos.max(0.0);
        let step = pos as u32;
        let cols = [rect.left() + 12.0, rect.left() + 186.0, rect.left() + 282.0, rect.left() + 352.0];
        // Position.
        // Bar, beat, sixteenth and tick, each in its own column so they never overlap.
        let parts = [format!("{}", step / STEPS_PER_BAR + 1), format!("{}", (step % STEPS_PER_BAR) / 4 + 1), format!("{}", step % 4 + 1), format!("{:03}", ((pos.fract()) * 240.0) as u32 + 1)];
        let xs = [44.0, 80.0, 108.0, 150.0];
        for (i, (part, label)) in parts.iter().zip(["BAR", "BEAT", "DIV", "TICK"]).enumerate() {
            let size = if i == 3 { 14.0 } else { 20.0 };
            p.text(pos2(cols[0] + xs[i], rect.top() + if i == 3 { 8.0 } else { 3.0 }), Align2::RIGHT_TOP, part, FontId::monospace(size), LCD_TEXT);
            p.text(pos2(cols[0] + xs[i], rect.bottom() - 3.0), Align2::RIGHT_BOTTOM, label, FontId::proportional(8.0), dim);
        }
        // Time.
        let secs = pos as f64 * 60.0 / self.project.bpm as f64 / 4.0;
        let t = format!("{:02}:{:02}.{}", (secs / 60.0) as u32, (secs % 60.0) as u32, ((secs.fract()) * 10.0) as u32);
        p.text(pos2(cols[1], rect.top() + 7.0), Align2::LEFT_TOP, t, FontId::monospace(14.0), LCD_TEXT);
        p.text(pos2(cols[1], rect.bottom() - 3.0), Align2::LEFT_BOTTOM, "TIME", FontId::proportional(8.5), dim);
        // Tempo (drag to change, right-click to map).
        let tr = egui::Rect::from_min_size(pos2(cols[2] - 12.0, rect.top() + 2.0), vec2(64.0, rect.height() - 4.0));
        let resp = ui.interact(tr, ui.id().with("lcd_tempo"), Sense::click_and_drag());
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            self.project.bpm = (self.project.bpm - resp.drag_delta().y * if fine { 0.02 } else { 0.25 }).clamp(40.0, 300.0);
            self.touch();
        }
        if resp.double_clicked() {
            self.project.bpm = self.project.bpm.round();
            self.touch();
        }
        resp.context_menu(|ui| self.map_menu(ui, MapTarget::Tempo, None));
        let tcol = if resp.hovered() || resp.dragged() { egui::Color32::WHITE } else { LCD_TEXT };
        p.text(pos2(cols[2] - 12.0, rect.top() + 7.0), Align2::LEFT_TOP, format!("{:.2}", self.project.bpm), FontId::monospace(14.0), tcol);
        p.text(pos2(cols[2] - 12.0, rect.bottom() - 3.0), Align2::LEFT_BOTTOM, "TEMPO", FontId::proportional(8.5), dim);
        self.hint(&resp, "Tempo", "Drag up or down to change the tempo (Shift: fine), double-click to round it. Right-click to map it to a MIDI control.");
        // Signature and key.
        p.text(pos2(cols[3] - 4.0, rect.top() + 7.0), Align2::LEFT_TOP, "4/4", FontId::monospace(13.0), LCD_TEXT);
        let kr = egui::Rect::from_min_size(pos2(cols[3] + 34.0, rect.top() + 2.0), vec2(rect.right() - cols[3] - 38.0, rect.height() - 4.0));
        let kresp = ui.interact(kr, ui.id().with("lcd_key"), Sense::click());
        let scale_name = SCALES[self.project.scale.min(SCALES.len() - 1)].0;
        let short: String = scale_name.chars().take(3).collect();
        p.text(pos2(kr.left(), rect.top() + 7.0), Align2::LEFT_TOP, format!("{} {}", NOTE_NAMES[self.project.key as usize % 12], short.to_lowercase()), FontId::monospace(13.0), if kresp.hovered() { egui::Color32::WHITE } else { LCD_TEXT });
        p.text(pos2(cols[3] - 4.0, rect.bottom() - 3.0), Align2::LEFT_BOTTOM, "SIG", FontId::proportional(8.0), dim);
        p.text(pos2(kr.left(), rect.bottom() - 3.0), Align2::LEFT_BOTTOM, "KEY", FontId::proportional(8.0), dim);
        let popup = ui.make_persistent_id("lcd_key_popup");
        if kresp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        egui::popup_below_widget(ui, popup, &kresp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(220.0);
            let mut key = self.project.key as usize;
            let mut scale = self.project.scale;
            ui.horizontal_wrapped(|ui| {
                for (i, n) in NOTE_NAMES.iter().enumerate() {
                    ui.selectable_value(&mut key, i, *n);
                }
            });
            ui.separator();
            for (i, sc) in SCALES.iter().enumerate() {
                ui.selectable_value(&mut scale, i, sc.0);
            }
            if key as u8 != self.project.key || scale != self.project.scale {
                self.project.key = key as u8;
                self.project.scale = scale;
                self.touch();
            }
        });
        self.hint(&kresp, "Key and Scale", "Click to choose the key and scale (highlighted in the piano roll, used by the scale tools).");
        // Master level under the display.
        let m = self.meter(super::MASTER_METER);
        let lvl = ((crate::audio::dsp::gain_to_db(m[0].max(m[1])) + 60.0) / 60.0).clamp(0.0, 1.0);
        p.rect_filled(egui::Rect::from_min_size(pos2(rect.left() + 6.0, rect.bottom() - 2.5), vec2((rect.width() - 12.0) * lvl, 1.5)), 0.0, PLAY);
    }

    fn main_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("New Project   (Ctrl+N)").clicked() {
            self.file_new();
            ui.close_menu();
        }
        if ui.button("Load Demo Project").clicked() {
            self.set_project(super::demo_with_audio());
            self.file = None;
            ui.close_menu();
        }
        if ui.button("Open...   (Ctrl+O)").clicked() {
            ui.close_menu();
            self.file_open();
        }
        if ui.button("Save   (Ctrl+S)").clicked() {
            ui.close_menu();
            self.file_save(false);
        }
        if ui.button("Save As...   (Ctrl+Shift+S)").clicked() {
            ui.close_menu();
            self.file_save(true);
        }
        if ui.button("Import Video...").on_hover_text("Puts a video on the timeline (needs ffmpeg) - score to picture").clicked() {
            ui.close_menu();
            if let Some(p) = rfd::FileDialog::new().add_filter("Video", &["mp4", "mov", "mkv", "webm", "avi", "m4v"]).pick_file() {
                self.import_video(p.to_string_lossy().to_string());
            }
        }
        if self.project.video.is_some() && ui.button("Show Video").clicked() {
            self.video.open = true;
            ui.close_menu();
        }
        if ui.button("Versions...").clicked() {
            self.versions.open = true;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Export Audio/Video...   (Ctrl+E)").clicked() {
            self.export = Some(export::ExportDialog::new(&self.project, self.audio.sample_rate));
            ui.close_menu();
        }
        ui.separator();
        ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
            if ui.button("Undo   (Ctrl+Z)").clicked() {
                self.undo();
            }
        });
        ui.add_enabled_ui(!self.redo.is_empty(), |ui| {
            if ui.button("Redo   (Ctrl+Y)").clicked() {
                self.redo();
            }
        });
        ui.separator();
        if ui.button("MIDI & Key Mappings...").clicked() {
            self.show_mappings = true;
            ui.close_menu();
        }
        ui.menu_button("Samples", |ui| {
            if ui.button("Download free CC0 drum samples").on_hover_text(crate::library::CC0_URL).clicked() {
                ui.close_menu();
                if crate::library::git_available() {
                    let root = super::browser::user_sample_dir();
                    self.run_job("Downloading CC0 drum samples...", move || {
                        crate::library::download_cc0(&root).map(|p| format!("Downloaded to {}", p.display()))
                    });
                } else {
                    self.toast("Downloading needs git - install it with your package manager");
                }
            }
            if ui.button("Rebuild Musicton Core library").clicked() {
                ui.close_menu();
                let root = super::browser::user_sample_dir();
                self.run_job("Rendering the Musicton Core sample library...", move || {
                    crate::library::build_core_library(&root).map(|n| format!("Rendered {n} samples")).map_err(|e| e.to_string())
                });
            }
            if ui.button("Open sample folder").clicked() {
                ui.close_menu();
                let dir = super::browser::user_sample_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
            }
        });
        ui.menu_button("MIDI Input", |ui| {
            if ui.button("Refresh").clicked() {
                self.midi_ports = Midi::list_ports();
            }
            if ui.selectable_label(self.midi.port_name.is_none(), "None").clicked() {
                self.midi.disconnect();
            }
            for port in self.midi_ports.clone() {
                let on = self.midi.port_name.as_deref() == Some(port.as_str());
                if ui.selectable_label(on, &port).clicked() {
                    let ctx = ui.ctx().clone();
                    if let Err(e) = self.midi.connect(&port, self.audio.tx.clone(), self.midi_tx.clone(), ctx) {
                        self.toast(format!("MIDI: {e}"));
                    }
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(format!("Audio: {} @ {} Hz", self.audio.device_name, self.audio.sample_rate)).color(TEXT_DIM));
    }
}
