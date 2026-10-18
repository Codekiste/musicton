//! Top transport/control bar.

use super::theme::*;
use super::widgets;
use super::{export, App, View};
use crate::audio::engine::Cmd;
use crate::midi::Midi;
use crate::model::{MapTarget, SCALES, NOTE_NAMES, STEPS_PER_BAR};
use egui::{pos2, vec2, Align2, FontId, Sense, Shape, Stroke};

#[derive(Clone, Copy)]
enum Icon {
    Play,
    Stop,
    Rec,
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, active: bool, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, if resp.hovered() { PANEL_LIGHT } else { PANEL_DARK });
    let c = rect.center();
    let col = if active { color } else { TEXT };
    match icon {
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![c + vec2(-5.0, -6.0), c + vec2(6.0, 0.0), c + vec2(-5.0, 6.0)], col, Stroke::NONE));
        }
        Icon::Stop => {
            p.rect_filled(egui::Rect::from_center_size(c, vec2(10.0, 10.0)), 0.0, col);
        }
        Icon::Rec => {
            p.circle_filled(c, 5.5, if active { color } else { TEXT_DIM });
        }
    }
    resp
}

impl App {
    pub(super) fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let r = widgets::button(ui, "TAP", false);
            if r.clicked() {
                self.tap_tempo();
            }
            self.hint(&r, "Tap Tempo", "Click repeatedly in time to set the tempo.");

            let mut bpm = self.project.bpm;
            let r = ui.add(egui::DragValue::new(&mut bpm).range(40.0..=300.0).speed(0.2).fixed_decimals(2).suffix(" BPM"));
            if r.changed() {
                self.project.bpm = bpm;
                self.touch();
            }
            r.context_menu(|ui| self.map_menu(ui, MapTarget::Tempo, None));
            self.hint(&r, "Tempo", "Song tempo in beats per minute. Drag or double-click to type.");

            let mut swing = self.project.swing * 100.0;
            let r = ui.add(egui::DragValue::new(&mut swing).range(0.0..=100.0).speed(0.5).fixed_decimals(0).prefix("Swing ").suffix("%"));
            if r.changed() {
                self.project.swing = swing / 100.0;
                self.touch();
            }
            self.hint(&r, "Global Groove", "Swing amount applied to every 16th note offbeat.");

            ui.label("4 / 4");
            let r = widgets::toggle(ui, "Metro", self.metronome, ACCENT, vec2(42.0, 19.0));
            if r.clicked() {
                self.metronome = !self.metronome;
                self.send(Cmd::Metronome(self.metronome));
            }
            self.hint(&r, "Metronome", "Toggles the click track (M).");

            let quants = [(0u32, "None"), (1, "1/16"), (4, "1/4"), (8, "1/2"), (16, "1 Bar"), (32, "2 Bars"), (64, "4 Bars")];
            let cur = quants.iter().find(|q| q.0 == self.project.quant).map(|q| q.1).unwrap_or("1 Bar");
            let r = egui::ComboBox::from_id_salt("quant")
                .width(62.0)
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
            self.hint(&r, "Global Quantization", "Launched clips start on the next multiple of this value.");

            ui.separator();

            // Position display.
            let pos = self.status.arr_pos.max(0.0) as u32;
            let txt = format!("{:>3}. {}. {}", pos / STEPS_PER_BAR + 1, (pos % STEPS_PER_BAR) / 4 + 1, pos % 4 + 1);
            let r = ui.add(egui::Label::new(egui::RichText::new(txt).monospace().size(13.0).color(ACCENT)).sense(Sense::click()));
            self.hint(&r, "Timeline Position", "Current song position in bars, beats and sixteenths.");

            let r = icon_button(ui, Icon::Play, self.status.playing, PLAY);
            if r.clicked() {
                self.send(Cmd::Play);
            }
            r.context_menu(|ui| self.map_menu(ui, MapTarget::Play, None));
            self.hint(&r, "Play", "Starts playback from the start marker (Space).");
            let r = icon_button(ui, Icon::Stop, false, TEXT);
            if r.clicked() {
                self.send(Cmd::Stop);
            }
            r.context_menu(|ui| self.map_menu(ui, MapTarget::Stop, None));
            self.hint(&r, "Stop", "Stops playback. Press again to return the start marker to the beginning.");
            let r = icon_button(ui, Icon::Rec, self.rec, REC);
            if r.clicked() {
                self.rec = !self.rec;
            }
            self.hint(&r, "Record", "When enabled, notes you play (computer keys or MIDI) are recorded into the playing clip of the selected track. If nothing is playing, a new clip is created (F9).");

            let r = widgets::toggle(ui, "Timeline Rec", self.arr_rec, REC, vec2(50.0, 19.0));
            if r.clicked() {
                self.arr_rec = !self.arr_rec;
            }
            self.hint(&r, "Timeline Record", "Records your Launcher performance into the Timeline: every clip you launch is written onto the timeline at the current position.");

            let r = widgets::button(ui, "Capture", false);
            if r.clicked() {
                self.capture_midi();
            }
            self.hint(&r, "Recall MIDI", "Turns what you just played on the selected track into a new clip - even if Record was off.");

            ui.separator();
            let r = widgets::toggle(ui, "Loop", self.project.loop_on, ACCENT, vec2(40.0, 19.0));
            if r.clicked() {
                self.project.loop_on = !self.project.loop_on;
                self.touch();
            }
            self.hint(&r, "Timeline Loop", "Loops the arrangement between the loop brace start and end.");
            let mut ls = self.project.loop_start / STEPS_PER_BAR + 1;
            let mut ll = self.project.loop_len / STEPS_PER_BAR;
            let r1 = ui.add(egui::DragValue::new(&mut ls).range(1..=999).speed(0.1));
            let r2 = ui.add(egui::DragValue::new(&mut ll).range(1..=256).speed(0.1).suffix(" bars"));
            if r1.changed() || r2.changed() {
                self.project.loop_start = (ls - 1) * STEPS_PER_BAR;
                self.project.loop_len = ll * STEPS_PER_BAR;
                self.touch();
            }
            self.hint(&r1, "Loop Start", "First bar of the loop.");
            self.hint(&r2, "Loop Length", "Length of the loop in bars.");

            let overridden = self.status.tracks.iter().any(|t| !t.follow_arr) && self.project.arrangement_end() > 0;
            if overridden {
                let r = widgets::toggle(ui, "Back to Timeline", true, ACCENT, vec2(128.0, 19.0));
                if r.clicked() {
                    self.send(Cmd::BackToArrangement);
                }
                self.hint(&r, "Back to Timeline", "Some tracks play Launcher clips instead of the Timeline. Click to return all of them.");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("☰", |ui| self.main_menu(ui));
                let r = widgets::toggle(ui, "I", self.show_info, BLUE, vec2(20.0, 19.0));
                if r.clicked() {
                    self.show_info = !self.show_info;
                }
                self.hint(&r, "Help Panel", "Shows or hides this help panel.");
                let r = widgets::toggle(ui, "Detail", self.show_detail, BLUE, vec2(42.0, 19.0));
                if r.clicked() {
                    self.show_detail = !self.show_detail;
                }
                self.hint(&r, "Editor Panel", "Shows or hides the Clip/Device view at the bottom.");
                let r = widgets::toggle(ui, "Browser", self.show_browser, BLUE, vec2(54.0, 19.0));
                if r.clicked() {
                    self.show_browser = !self.show_browser;
                }
                self.hint(&r, "Browser", "Shows or hides the browser with instruments, effects, clips and samples.");

                let r = widgets::toggle(ui, "Timeline", self.view == View::Arrangement, TEXT, vec2(80.0, 19.0));
                if r.clicked() {
                    self.view = View::Arrangement;
                }
                self.hint(&r, "Timeline", "Timeline view for building finished tracks (Tab).");
                let r = widgets::toggle(ui, "Launcher", self.view == View::Session, TEXT, vec2(56.0, 19.0));
                if r.clicked() {
                    self.view = View::Session;
                }
                self.hint(&r, "Launcher", "Clip grid for jamming and live performance (Tab).");

                ui.separator();
                let cpu = (self.status.cpu * 100.0).min(999.0);
                let r = ui.colored_label(if cpu > 80.0 { REC } else { TEXT_DIM }, format!("CPU {cpu:.0}%"));
                self.hint(&r, "CPU Load", "How much of the available audio processing time is used.");
                let mut key = self.project.key as usize;
                let mut scale = self.project.scale;
                let r = egui::ComboBox::from_id_salt("scale")
                    .width(84.0)
                    .selected_text(SCALES[scale.min(SCALES.len() - 1)].0)
                    .show_ui(ui, |ui| {
                        for (i, s) in SCALES.iter().enumerate() {
                            ui.selectable_value(&mut scale, i, s.0);
                        }
                    })
                    .response;
                let r2 = egui::ComboBox::from_id_salt("key")
                    .width(36.0)
                    .selected_text(NOTE_NAMES[key % 12])
                    .show_ui(ui, |ui| {
                        for (i, n) in NOTE_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut key, i, *n);
                        }
                    })
                    .response;
                if key as u8 != self.project.key || scale != self.project.scale {
                    self.project.key = key as u8;
                    self.project.scale = scale;
                    self.touch();
                }
                self.hint(&r, "Scale", "Highlights notes of this scale in the piano roll.");
                self.hint(&r2, "Key", "Root note for the scale highlighting.");
            });
        });
        // Simple level meter strip under the bar (master).
        let rect = ui.max_rect();
        let m = self.meter(super::MASTER_METER);
        let w = rect.width() * ((crate::audio::dsp::gain_to_db(m[0].max(m[1])) + 60.0) / 60.0).clamp(0.0, 1.0);
        ui.painter().rect_filled(egui::Rect::from_min_size(pos2(rect.left(), rect.bottom() + 2.0), vec2(w, 2.0)), 0.0, PLAY);
        let _ = (Align2::LEFT_TOP, FontId::default());
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
            self.export = Some(export::ExportDialog::new(&self.project));
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
