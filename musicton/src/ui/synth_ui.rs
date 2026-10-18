//! Editor for the Matrix FM synth: operator rows and the modulation matrix.

use super::theme::*;
use super::widgets;
use super::App;
use crate::model::*;
use crate::params::{self, mfm, mfm_mod, mfm_param, MFM_OPS, MFM_OP_PARAMS};
use egui::{vec2, RichText};

impl App {
    pub(super) fn matrix_fm_ui(&mut self, ui: &mut egui::Ui, track: u64, inst: &mut Instrument) {
        let defs = inst.kind.defs();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Operators").size(10.0).color(TEXT_DIM));
                for op in 0..MFM_OPS {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}", op + 1)).strong().color(ACCENT));
                        for j in 0..MFM_OP_PARAMS {
                            let idx = mfm_param(op, j);
                            let r = widgets::knob(ui, &mut inst.params[idx], &defs[idx], 20.0, false);
                            r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                            let txt = format!("{}: {}", defs[idx].name, defs[idx].format(inst.params[idx]));
                            self.param_hint(&r, &defs[idx], &txt);
                        }
                    });
                }
                ui.label(RichText::new("Ratio · Fine · Level · Out · A · D · S · R").size(9.5).color(TEXT_DIM));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Matrix (row modulates column)").size(10.0).color(TEXT_DIM));
                egui::Grid::new(("mfm_matrix", track)).spacing(vec2(2.0, 1.0)).show(ui, |ui| {
                    ui.label("");
                    for dst in 0..MFM_OPS {
                        ui.label(RichText::new(format!(">{}", dst + 1)).size(9.0).color(TEXT_DIM));
                    }
                    ui.end_row();
                    for src in 0..MFM_OPS {
                        ui.label(RichText::new(format!("{}", src + 1)).size(9.0).color(ACCENT));
                        for dst in 0..MFM_OPS {
                            let idx = mfm_mod(src, dst);
                            let r = widgets::knob(ui, &mut inst.params[idx], &defs[idx], 18.0, false);
                            r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                            let what = if src == dst { format!("Operator {} feedback", src + 1) } else { format!("Operator {} modulates operator {}", src + 1, dst + 1) };
                            self.param_hint(&r, &defs[idx], &what);
                        }
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                for idx in [mfm::VOLUME, mfm::VEL, mfm::VOICES, mfm::GLIDE] {
                    let r = widgets::param(ui, ("inst", track, idx), &mut inst.params[idx], &defs[idx], 30.0);
                    r.context_menu(|ui| self.map_menu(ui, MapTarget::Instrument(track, idx), Some(track)));
                    self.param_hint(&r, &defs[idx], "Matrix FM global parameter.");
                }
            });
        });
        let _ = params::MFM_GLOBAL;
    }
}
