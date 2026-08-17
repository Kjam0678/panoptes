//! The application shell: file handling, navigation, and the pages that are
//! not the loadout editor.

mod cli;
mod dialogs;
mod document;
mod pages;

use std::path::PathBuf;

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::Catalog,
    game_settings,
    icons::Icons,
    loadout,
    paths, settings,
    status::Change,
    theme,
};

use cli::check;
use document::Document;

const DISPLAY_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const SUNRISE_URL: &str = "https://github.com/stanuwu/Sunrise";

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Loadout,
    GameSettings,
    Json,
    Paths,
}

pub struct App {
    catalog: Catalog,
    icons: Icons,
    document: Option<Document>,
    view: View,
    character: usize,
    loadout: loadout::LoadoutState,
    game_settings_tab: game_settings::Tab,
    key_binding_ui: game_settings::KeyBindingUiState,
    status: String,
    status_is_error: bool,
    about_open: bool,
    reload_confirmation_open: bool,
    restore_confirmation_open: bool,
    exit_confirmation_open: bool,
    exit_confirmed: bool,
}

impl App {
    fn new(catalog: Catalog) -> Self {
        let mut app = Self {
            catalog,
            icons: Icons::new(),
            document: None,
            view: View::Loadout,
            character: 0,
            loadout: loadout::LoadoutState::default(),
            game_settings_tab: game_settings::Tab::Player,
            key_binding_ui: game_settings::KeyBindingUiState::default(),
            status: "Choose Project Sunrise's settings.json to begin".to_owned(),
            status_is_error: false,
            about_open: false,
            reload_confirmation_open: false,
            restore_confirmation_open: false,
            exit_confirmation_open: false,
            exit_confirmed: false,
        };
        if let Some(path) = paths::remembered_settings_path() {
            app.open(path);
        }
        app
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.status = message.into();
        self.status_is_error = is_error;
    }

    /// Reports what a page did: a status line, or the reason it failed.
    fn report(&mut self, change: Change) {
        match change {
            Ok(message) => self.set_status(message, false),
            Err(error) => self.set_status(error, true),
        }
    }

    fn open(&mut self, path: PathBuf) {
        let document = match Document::open(path) {
            Ok(document) => document,
            Err(error) => return self.set_status(error, true),
        };
        let path = document.path.clone();
        let warning = document.source_warning.clone().or_else(|| {
            game_settings::future_schema_version(&document.value).map(|version| {
                format!("it uses schema {version}, which is newer than this build was tested with")
            })
        });
        self.character = 0;
        self.loadout.clear_pickers();
        self.key_binding_ui.clear_pickers();
        self.document = Some(document);

        self.report(match (warning, paths::remember_settings_path(&path)) {
            (Some(warning), _) => Err(format!(
                "Loaded, but {warning}. Unrecognized fields are preserved, and a safety copy is made beside settings.json before saving"
            )),
            (None, Err(error)) => Err(format!("Loaded, but the path was not remembered: {error}")),
            (None, Ok(())) => Ok(format!("Loaded {}", path.display())),
        });
    }

    /// Both entry points accept either the settings file itself or the install
    /// folder that holds it.
    fn choose(&mut self, folder: bool) {
        let dialog = rfd::FileDialog::new();
        let picked = if folder {
            dialog
                .set_title("Select the Destiny 2 Shadowkeep install folder")
                .pick_folder()
        } else {
            dialog
                .set_title("Select Sunrise's settings.json")
                .add_filter("Sunrise settings", &["json"])
                .pick_file()
        };
        let Some(picked) = picked else {
            return;
        };
        match paths::resolve_selection(&picked) {
            Ok(path) => self.open(path),
            Err(error) => self.set_status(error, true),
        }
    }

    fn reload(&mut self) {
        let Some(path) = self.document.as_ref().map(|document| document.path.clone()) else {
            return;
        };
        self.open(path);
    }

    fn save(&mut self) {
        let Some(document) = self.document.as_mut() else {
            return;
        };
        if let Err(error) = settings::verify_source_unchanged(&document.path, &document.persisted) {
            self.set_status(format!("Not saved: {error}"), true);
            return;
        }
        let repaired = settings::repair_known_ability_pairs(&mut document.value);
        document.dirty |= repaired > 0;
        let current_warning = settings::validate_document(&document.value).err();
        let warning = document.source_warning.clone().or_else(|| current_warning.clone());
        let safety_copy = if warning.is_some() {
            match settings::create_adjacent_backup(&document.path) {
                Ok(path) => Some(path),
                Err(error) => {
                    self.set_status(
                        format!("Not saved: the file holds an unexpected setting and its safety copy failed: {error}"),
                        true,
                    );
                    return;
                }
            }
        } else {
            None
        };

        let repair_note = match repaired {
            0 => String::new(),
            1 => " Corrected one invalid ability pairing.".to_owned(),
            count => format!(" Corrected {count} invalid ability pairings."),
        };
        // A save that went through still reports as a problem when the file
        // held something unrecognized, so the safety copy gets noticed.
        let outcome = match settings::save_json(&document.path, &document.value) {
            Ok(backup) => {
                document.persisted = document.value.clone();
                document.source_warning = current_warning;
                document.dirty = false;
                match (warning, safety_copy) {
                    (Some(warning), Some(copy)) => Err(format!(
                        "Saved after detecting an unexpected setting ({warning}).{repair_note} Untouched source: {}. Backup: {}",
                        copy.display(),
                        backup.display()
                    )),
                    _ => Ok(format!("Saved.{repair_note} Backup: {}", backup.display())),
                }
            }
            Err(error) => Err(match safety_copy {
                Some(path) => format!("{error} Untouched source: {}.", path.display()),
                None => error,
            }),
        };
        self.report(outcome);
    }

    /// Replaces the current file with any earlier backup this app wrote.
    fn restore_backup(&mut self) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let path = document.path.clone();
        let Some(chosen) = rfd::FileDialog::new()
            .set_title("Choose a backup to restore")
            .set_directory(paths::backup_dir().unwrap_or_else(|| path.clone()))
            .add_filter("Settings backup", &["json", "bak"])
            .pick_file()
        else {
            return;
        };

        let restored = settings::load_json(&chosen)
            .and_then(|restored| {
                let safety_copy = settings::create_adjacent_backup(&path)
                    .map_err(|error| format!("Not restored; the safety copy failed: {error}"))?;
                let backup = settings::save_json(&path, &restored)?;
                Ok(format!(
                    "Restored {}. Previous file: {}. Backup: {}",
                    chosen.display(),
                    safety_copy.display(),
                    backup.display()
                ))
            });
        if restored.is_ok() {
            self.open(path);
        }
        self.report(restored);
    }

    fn apply_raw_json(&mut self) {
        let Some(document) = self.document.as_mut() else {
            return;
        };
        let parsed = serde_json::from_str::<Value>(&document.raw_json).map_err(|error| {
            format!(
                "JSON syntax error at line {}, column {}: {error}",
                error.line(),
                error.column()
            )
        });
        // An unexpected setting still applies, but it is reported as a problem.
        let applied = parsed.and_then(|value| {
            document.value = value;
            document.dirty = true;
            match settings::validate_document(&document.value).err() {
                Some(warning) => Err(format!(
                    "Applied with an unexpected setting: {warning}. Saving will first copy the source to settings.json.bak"
                )),
                None => Ok("JSON applied; click Save to write it".to_owned()),
            }
        });
        self.character = self
            .character
            .min(self.character_count().saturating_sub(1));
        self.report(applied);
    }

    fn character_count(&self) -> usize {
        self.document
            .as_ref()
            .map_or(0, Document::character_count)
    }

}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.icons.begin_frame();
        let dirty = self.document.as_ref().is_some_and(|document| document.dirty);
        if ctx.input(|input| input.viewport().close_requested()) && dirty && !self.exit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_confirmation_open = true;
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if dirty {
                    ui.label(egui::RichText::new("Unsaved changes").color(theme::UNSAVED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
                        self.save();
                    }
                    if ui
                        .add_enabled(self.document.is_some(), egui::Button::new("Reload"))
                        .clicked()
                    {
                        if dirty {
                            self.reload_confirmation_open = true;
                        } else {
                            self.reload();
                        }
                    }
                });
            });
        });

        egui::SidePanel::left("navigation")
            .resizable(false)
            .default_width(170.0)
            .show(ctx, |ui| {
                ui.heading("Editor");
                ui.add_space(6.0);
                let loaded = self.document.is_some();
                for (view, label) in [
                    (View::Loadout, "Characters & loadouts"),
                    (View::GameSettings, "Game settings"),
                    (View::Json, "All settings (JSON)"),
                    (View::Paths, "Paths"),
                ] {
                    let enabled = loaded || view == View::Paths;
                    if ui
                        .add_enabled(enabled, egui::SelectableLabel::new(self.view == view, label))
                        .clicked()
                    {
                        if view == View::Json && self.view != View::Json {
                            if let Some(document) = self.document.as_mut() {
                                document.raw_json =
                                    serde_json::to_string_pretty(&document.value).unwrap_or_default();
                            }
                        }
                        self.view = view;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if ui.button("About").clicked() {
                        self.about_open = true;
                    }
                });
            });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let color = if self.status_is_error {
                theme::ERROR
            } else {
                ui.visuals().text_color()
            };
            ui.add(egui::Label::new(egui::RichText::new(&self.status).color(color)).wrap());
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.document.is_none() {
                if self.view == View::Paths {
                    self.draw_paths(ui);
                } else {
                    self.draw_welcome(ui);
                }
                return;
            }
            match self.view {
                View::Loadout => self.draw_loadout(ui),
                View::GameSettings => {
                    let mut changed = false;
                    if let Some(document) = self.document.as_mut() {
                        changed = game_settings::draw_page(
                            ui,
                            &mut document.value,
                            &mut self.game_settings_tab,
                            &mut self.key_binding_ui,
                        );
                        document.dirty |= changed;
                    }
                    if changed {
                        self.set_status("Game setting updated; click Save to write it", false);
                    }
                }
                View::Json => self.draw_json(ui),
                View::Paths => self.draw_paths(ui),
            }
        });

        self.draw_dialogs(ctx);
        if self.icons.wants_repaint() {
            ctx.request_repaint();
        }
    }
}

pub fn run() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    if let Some(flag) = args.next() {
        if flag == "--check" {
            let path = args
                .next()
                .map(PathBuf::from)
                .or_else(paths::remembered_settings_path);
            let Some(path) = path else {
                eprintln!("panoptes: --check needs a settings.json path");
                std::process::exit(2);
            };
            match check(&path) {
                Ok(summary) => println!("{summary}"),
                Err(error) => {
                    eprintln!("panoptes: {error}");
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
    }

    let catalog = match Catalog::load() {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("panoptes: {error}");
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 820.0])
            .with_min_inner_size([760.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Panoptes",
        options,
        Box::new(move |cc| {
            // A preference, not just the current visuals: eframe otherwise
            // follows the desktop and would flip the window to light mode.
            cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
            Ok(Box::new(App::new(catalog)))
        }),
    )
}
