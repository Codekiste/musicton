//! Versions window: save named snapshots, compare them with the current state, restore.

use super::theme::*;
use super::App;
use crate::versions::{self, VersionInfo};
use egui::RichText;

#[derive(Default)]
pub struct VersionsState {
    pub open: bool,
    pub message: String,
    list: Vec<VersionInfo>,
    loaded_for: String,
    compare: Option<(String, Vec<String>)>,
}

impl App {
    pub(super) fn save_version(&mut self, message: &str) -> Result<VersionInfo, String> {
        let dir = versions::versions_dir(&self.project.name);
        let info = versions::save_version(&dir, &self.project, message)?;
        versions::prune(&dir, 200);
        self.versions.loaded_for.clear();
        Ok(info)
    }

    pub(super) fn restore_version(&mut self, id: &str) -> Result<(), String> {
        let dir = versions::versions_dir(&self.project.name);
        let p = versions::load_version(&dir, id)?;
        // Keep the current state so a restore can itself be undone from the list.
        self.save_version("Before restore")?;
        let name = self.project.name.clone();
        self.set_project(p);
        self.project.name = name;
        self.touch();
        Ok(())
    }

    pub(super) fn versions_window(&mut self, ctx: &egui::Context) {
        if !self.versions.open {
            return;
        }
        if self.versions.loaded_for != self.project.name {
            self.versions.list = versions::list_versions(&versions::versions_dir(&self.project.name));
            self.versions.loaded_for = self.project.name.clone();
        }
        let mut open = true;
        let mut action: Option<(u8, String)> = None;
        egui::Window::new("Versions").open(&mut open).default_pos([520.0, 140.0]).default_width(460.0).show(ctx, |ui| {
            ui.label(RichText::new(format!("Project \"{}\" · now: {}", self.project.name, versions::summary(&self.project))).color(TEXT_DIM));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.versions.message).hint_text("What changed? (e.g. new drop)").desired_width(300.0));
                if ui.button("Save Version").clicked() {
                    let msg = if self.versions.message.trim().is_empty() { "Snapshot".to_string() } else { self.versions.message.trim().to_string() };
                    match self.save_version(&msg) {
                        Ok(_) => {
                            self.versions.message.clear();
                            self.toast("Version saved");
                        }
                        Err(e) => self.toast(format!("Could not save version: {e}")),
                    }
                }
            });
            ui.separator();
            if self.versions.list.is_empty() {
                ui.label(RichText::new("No versions yet. Every Save (Ctrl+S) also stores a version.").color(TEXT_DIM));
            }
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for v in &self.versions.list {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(versions::format_time(v.time)).monospace().size(10.5).color(TEXT_DIM));
                        ui.label(RichText::new(&v.message).strong());
                        ui.label(RichText::new(&v.summary).size(10.0).color(TEXT_DIM));
                        if ui.small_button("Compare").clicked() {
                            action = Some((0, v.id.clone()));
                        }
                        if ui.small_button("Restore").clicked() {
                            action = Some((1, v.id.clone()));
                        }
                    });
                }
            });
            if let Some((id, lines)) = &self.versions.compare {
                ui.separator();
                ui.label(RichText::new(format!("Changes since version {id}:")).color(ACCENT));
                for l in lines {
                    ui.label(RichText::new(l).size(11.0));
                }
            }
        });
        match action {
            Some((0, id)) => {
                let dir = versions::versions_dir(&self.project.name);
                match versions::load_version(&dir, &id) {
                    Ok(old) => self.versions.compare = Some((id, versions::diff(&old, &self.project))),
                    Err(e) => self.toast(format!("Could not read version: {e}")),
                }
            }
            Some((_, id)) => match self.restore_version(&id) {
                Ok(()) => self.toast("Version restored (the previous state was saved as 'Before restore')"),
                Err(e) => self.toast(format!("Restore failed: {e}")),
            },
            None => {}
        }
        if !open {
            self.versions.open = false;
            self.versions.compare = None;
        }
    }
}
