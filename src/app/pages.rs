//! The pages that are not the loadout editor: the welcome screen, the paths
//! page, and the raw JSON editor.

use eframe::egui;
use serde_json::Value;

use crate::{
    game_settings,
    icons::Icons,
    loadout,
    model::{MAX_SETTINGS_BYTES, pointer},
    paths,
};

use super::App;

impl App {
    pub(super) fn draw_welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.heading("Panoptes");
            ui.label("A fast loadout editor for Project Sunrise on Destiny 2 Shadowkeep.");
            ui.add_space(16.0);
            if ui.button("Choose settings.json…").clicked() {
                self.choose(false);
            }
            ui.add_space(6.0);
            if ui.button("Choose the Destiny 2 install folder…").clicked() {
                self.choose(true);
            }
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new(format!(
                    "Sunrise keeps its settings at {} or {}",
                    paths::SETTINGS_LAYOUTS[0],
                    paths::SETTINGS_LAYOUTS[1]
                ))
                .weak()
                .small(),
            );
        });
    }

    pub(super) fn draw_loadout(&mut self, ui: &mut egui::Ui) {
        let Some(document) = self.document.as_mut() else {
            return;
        };
        let tabs: Vec<(usize, String)> = document
            .value
            .pointer(pointer::CHARACTERS)
            .and_then(Value::as_array)
            .map(|characters| {
                characters
                    .iter()
                    .enumerate()
                    .map(|(index, character)| (index, loadout::character_tab_label(character, index)))
                    .collect()
            })
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for (index, label) in tabs {
                if ui.selectable_label(self.character == index, label).clicked() {
                    self.character = index;
                }
            }
        });
        ui.separator();

        let character = self.character;
        let mut page = loadout::Page {
            document: &mut document.value,
            catalog: &self.catalog,
            icons: &mut self.icons,
            state: &mut self.loadout,
        };
        let mut change = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            change = page.draw_character_fields(ui, character);
            ui.add_space(10.0);
            if let Some(equipment_change) = page.draw_equipment(ui, character) {
                change = Some(equipment_change);
            }
        });
        if let Some(change) = change {
            document.dirty |= change.is_ok();
            self.report(change);
        }
    }

    pub(super) fn draw_paths(&mut self, ui: &mut egui::Ui) {
        ui.heading("Paths");
        ui.add_space(8.0);
        egui::Grid::new("paths")
            .num_columns(3)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label("Sunrise settings");
                ui.monospace(
                    self.document
                        .as_ref()
                        .map_or_else(|| "None selected".to_owned(), |document| document.path.display().to_string()),
                );
                if ui.button("Choose…").clicked() {
                    self.choose(false);
                }
                ui.end_row();
                ui.label("Settings schema");
                ui.monospace(
                    self.document
                        .as_ref()
                        .and_then(|document| game_settings::schema_version(&document.value))
                        .map_or_else(|| "Missing or invalid".to_owned(), |version| version.to_string()),
                );
                ui.end_row();
                ui.label("Save size limit");
                ui.monospace(format!("{MAX_SETTINGS_BYTES} bytes"));
                ui.end_row();
                ui.label("Backups");
                ui.monospace(
                    paths::backup_dir()
                        .map_or_else(|| "Unavailable".to_owned(), |dir| dir.display().to_string()),
                );
                ui.end_row();
                ui.label("Icons");
                ui.monospace(format!("{} compiled in", Icons::packed()));
                ui.end_row();
                ui.label("Catalog");
                ui.monospace(format!("{} bundled items (build 86657.20.08.23)", self.catalog.items.len()));
                ui.end_row();
            });

        ui.add_space(18.0);
        ui.separator();
        ui.add_space(10.0);
        ui.heading("Restore a backup");
        ui.label("Replace the current settings.json with an earlier backup. The current file is copied to settings.json.bak first.");
        if ui
            .add_enabled(self.document.is_some(), egui::Button::new("Restore a backup…"))
            .clicked()
        {
            self.restore_confirmation_open = true;
        }
    }

    pub(super) fn draw_json(&mut self, ui: &mut egui::Ui) {
        let mut apply = false;
        let mut reset = false;
        ui.horizontal(|ui| {
            ui.heading("All settings");
            apply = ui.button("Apply JSON").clicked();
            reset = ui.button("Reset editor").clicked();
        });
        ui.label("Edit anything the guided pages do not cover, then Apply JSON. Save writes applied changes to disk.");
        ui.add_space(6.0);
        if let Some(document) = self.document.as_mut() {
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut document.raw_json)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(40),
                );
            });
        }
        if apply {
            self.apply_raw_json();
        }
        if reset {
            if let Some(document) = self.document.as_mut() {
                document.raw_json = serde_json::to_string_pretty(&document.value).unwrap_or_default();
            }
            self.set_status("JSON editor reset to the current settings", false);
        }
    }
}
