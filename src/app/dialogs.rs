//! The modal windows: About, and the confirmations that guard a restore, a
//! discard, or an exit with unsaved changes.

use eframe::egui;

use super::{App, DISPLAY_VERSION, SUNRISE_URL};

impl App {
    pub(super) fn draw_dialogs(&mut self, ctx: &egui::Context) {
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
