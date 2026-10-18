//! Export Audio dialog.

use super::theme::*;
use super::App;
use crate::audio::{export_stems, export_wav, BitDepth, ExportSource};
use crate::model::Project;
use std::sync::atomic::Ordering;

pub struct ExportDialog {
    arrangement: bool,
    scene: usize,
    bars: u32,
    sample_rate: u32,
    depth: BitDepth,
    normalize: bool,
    stems: bool,
}

impl ExportDialog {
    pub fn new(p: &Project) -> Self {
        Self { arrangement: p.arrangement_end() > 0, scene: 0, bars: 16, sample_rate: 44100, depth: BitDepth::Int16, normalize: false, stems: false }
    }
}

impl App {
    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.export_job {
            let done = job.result.lock().take();
            let progress = job.progress.load(Ordering::Relaxed) as f32 / 1000.0;
            match done {
                Some(Ok(path)) => {
                    self.export_job = None;
                    self.toast(format!("Exported {path}"));
                }
                Some(Err(e)) => {
                    self.export_job = None;
                    self.toast(format!("Export failed: {e}"));
                }
                None => {
                    egui::Window::new("Rendering...").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                        ui.add(egui::ProgressBar::new(progress).show_percentage().desired_width(260.0));
                    });
                    ctx.request_repaint();
                }
            }
            return;
        }
        let Some(mut d) = self.export.take() else { return };
        let mut open = true;
        let mut start = false;
        egui::Window::new("Export Audio")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("export_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Rendered Track");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.stems, false, "Master");
                        ui.radio_value(&mut d.stems, true, "All Individual Tracks");
                    });
                    ui.end_row();
                    ui.label("Source");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.arrangement, true, "Timeline");
                        ui.radio_value(&mut d.arrangement, false, "Scene");
                    });
                    ui.end_row();
                    if !d.arrangement {
                        ui.label("Scene");
                        egui::ComboBox::from_id_salt("exp_scene")
                            .selected_text(self.project.scenes.get(d.scene).map(|s| s.name.clone()).unwrap_or_default())
                            .show_ui(ui, |ui| {
                                for (i, s) in self.project.scenes.iter().enumerate() {
                                    ui.selectable_value(&mut d.scene, i, &s.name);
                                }
                            });
                        ui.end_row();
                        ui.label("Length");
                        ui.add(egui::DragValue::new(&mut d.bars).range(1..=512).suffix(" bars"));
                        ui.end_row();
                    } else {
                        ui.label("Length");
                        let bars = self.project.arrangement_end() as f32 / 16.0;
                        ui.label(format!("{bars:.1} bars (+3 s tail)"));
                        ui.end_row();
                    }
                    ui.label("Sample Rate");
                    egui::ComboBox::from_id_salt("exp_sr").selected_text(format!("{}", d.sample_rate)).show_ui(ui, |ui| {
                        for sr in [44100, 48000, 88200, 96000] {
                            ui.selectable_value(&mut d.sample_rate, sr, sr.to_string());
                        }
                    });
                    ui.end_row();
                    ui.label("Bit Depth");
                    egui::ComboBox::from_id_salt("exp_bits")
                        .selected_text(match d.depth {
                            BitDepth::Int16 => "16",
                            BitDepth::Int24 => "24",
                            BitDepth::Float32 => "32 (float)",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.depth, BitDepth::Int16, "16");
                            ui.selectable_value(&mut d.depth, BitDepth::Int24, "24");
                            ui.selectable_value(&mut d.depth, BitDepth::Float32, "32 (float)");
                        });
                    ui.end_row();
                    ui.label("Normalize");
                    ui.checkbox(&mut d.normalize, "");
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Export...").clicked() {
                        start = true;
                    }
                    ui.label(egui::RichText::new("Renders offline, faster than real time.").color(TEXT_DIM).size(10.5));
                });
            });
        if start {
            let source = if d.arrangement { ExportSource::Arrangement } else { ExportSource::Scene { scene: d.scene, bars: d.bars } };
            if d.stems {
                if let Some(dir) = rfd::FileDialog::new().set_title("Folder for the stems").pick_folder() {
                    self.export_job = Some(export_stems(self.project.clone(), source, d.sample_rate, d.depth, d.normalize, dir.to_string_lossy().to_string()));
                }
                return;
            }
            let name = format!("{}.wav", self.project.name);
            if let Some(mut path) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(name).save_file() {
                if path.extension().is_none() {
                    path.set_extension("wav");
                }
                self.export_job = Some(export_wav(
                    self.project.clone(),
                    source,
                    d.sample_rate,
                    d.depth,
                    d.normalize,
                    path.to_string_lossy().to_string(),
                ));
            }
            return;
        }
        if open {
            self.export = Some(d);
        }
    }
}
