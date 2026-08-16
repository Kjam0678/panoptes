//! The application shell: file handling, navigation, and the pages that are
//! not the loadout editor.

use std::path::PathBuf;

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::Catalog,
    game_settings, icons::Icons, loadout, model::MAX_SETTINGS_BYTES, paths, settings,
};

const DISPLAY_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const SUNRISE_URL: &str = "https://github.com/stanuwu/Sunrise";

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Loadout,
    GameSettings,
    Json,
    Paths,
}

/// The loaded settings.json and everything derived from it.
struct Document {
    path: PathBuf,
    value: Value,
    persisted: Value,
    source_warning: Option<String>,
    raw_json: String,
    dirty: bool,
}

impl Document {
    fn open(path: PathBuf) -> Result<Self, String> {
        let value = settings::load_json(&path)?;
        Ok(Self {
            source_warning: settings::validate_document(&value).err(),
            raw_json: serde_json::to_string_pretty(&value).unwrap_or_default(),
            persisted: value.clone(),
            value,
            path,
            dirty: false,
        })
    }

    fn character_count(&self) -> usize {
        self.value
            .pointer("/state/characters")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    }
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
    fn report(&mut self, change: loadout::Change) {
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

    // ------------------------------------------------------------------ pages

    fn draw_welcome(&mut self, ui: &mut egui::Ui) {
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

    fn draw_loadout(&mut self, ui: &mut egui::Ui) {
        let Some(document) = self.document.as_mut() else {
            return;
        };
        let tabs: Vec<(usize, String)> = document
            .value
            .pointer("/state/characters")
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

    fn draw_paths(&mut self, ui: &mut egui::Ui) {
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

    fn draw_json(&mut self, ui: &mut egui::Ui) {
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
                    ui.label(egui::RichText::new("Unsaved changes").color(egui::Color32::YELLOW));
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
                egui::Color32::LIGHT_RED
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

impl App {
    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        if self.about_open {
            let mut open = true;
            egui::Window::new("About Panoptes")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.vertical_centered(|ui| {
                        ui.heading("Panoptes");
                        ui.label(egui::RichText::new(DISPLAY_VERSION).weak());
                    });
                    ui.add_space(10.0);
                    ui.label("Edits Project Sunrise's characters, loadouts, sockets, and game settings for Destiny 2 Shadowkeep build 86657.20.08.23.");
                    ui.add_space(6.0);
                    ui.hyperlink_to("Project Sunrise on GitHub", SUNRISE_URL);
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(
                            "Not affiliated with or endorsed by Bungie Inc. or Sony Interactive Entertainment. Destiny and related intellectual property are owned by Bungie Inc. and their respective rights holders.",
                        )
                        .small()
                        .weak(),
                    );
                });
            self.about_open = open;
        }

        if self.reload_confirmation_open {
            match confirm(
                ctx,
                "Discard unsaved changes?",
                "Reloading discards changes that have not been saved.",
                "Discard and reload",
            ) {
                Some(true) => {
                    self.reload_confirmation_open = false;
                    self.reload();
                }
                Some(false) => self.reload_confirmation_open = false,
                None => {}
            }
        }

        if self.restore_confirmation_open {
            match confirm(
                ctx,
                "Restore a backup?",
                "This replaces the entire settings.json. The current file is copied to settings.json.bak and to a timestamped backup first, and unsaved changes are discarded.",
                "Choose a backup…",
            ) {
                Some(true) => {
                    self.restore_confirmation_open = false;
                    self.restore_backup();
                }
                Some(false) => self.restore_confirmation_open = false,
                None => {}
            }
        }

        if self.exit_confirmation_open {
            let mut open = true;
            let (mut save_and_exit, mut discard, mut cancel) = (false, false, false);
            egui::Window::new("Unsaved changes")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Save your changes before closing?");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        save_and_exit = ui.button("Save and exit").clicked();
                        discard = ui.button("Discard and exit").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            self.exit_confirmation_open = open && !save_and_exit && !discard && !cancel;
            if save_and_exit {
                self.save();
                if !self.document.as_ref().is_some_and(|document| document.dirty) {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else if discard {
                self.exit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

/// A modal with one confirming button; `Some(true)` confirms, `Some(false)`
/// cancels, `None` means it is still open.
fn confirm(ctx: &egui::Context, title: &str, body: &str, confirm_label: &str) -> Option<bool> {
    let mut open = true;
    let (mut confirmed, mut cancelled) = (false, false);
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(body);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                confirmed = ui.button(confirm_label).clicked();
                cancelled = ui.button("Cancel").clicked();
            });
        });
    match (confirmed, cancelled || !open) {
        (true, _) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
    }
}

/// `--check <settings.json>` validates a file without opening a window.
fn check(path: &std::path::Path) -> Result<String, String> {
    let document = settings::load_json(path)?;
    settings::validate_document(&document).map_err(|error| format!("Invalid settings: {error}"))?;
    let catalog = Catalog::load()?;
    Ok(format!(
        "Valid: {} characters, {} catalog items, save size {} bytes",
        document
            .pointer("/state/characters")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        catalog.items.len(),
        settings::encode_settings(&document)?.len() + 1
    ))
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
