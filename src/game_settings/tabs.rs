//! The guided pages: the player name, and one page per group of settings
//! Sunrise replicates.

use eframe::egui;
use serde_json::{Map, Value};

use crate::{model::pointer, theme};

use super::{
    fields::{draw_setting, group_mut, missing_group},
    tables::SettingGroup,
};

pub(super) fn draw_player(ui: &mut egui::Ui, document: &mut Value) -> bool {
    ui.heading("Player");
    ui.label("Change the player name shown by Project Sunrise in Destiny 2.");
    ui.add_space(8.0);

    let Some(value) = document.pointer(pointer::PERSONA_NAME) else {
        ui.colored_label(
            theme::ERROR,
            "This settings.json has no steam.user.persona_name field.",
        );
        return false;
    };
    let Some(current) = value.as_str() else {
        ui.colored_label(
            theme::ERROR,
            "steam.user.persona_name must be text.",
        );
        return false;
    };

    let mut edited = current.to_owned();
    let response = ui.add(
        egui::TextEdit::singleline(&mut edited)
            .desired_width(360.0)
            .char_limit(63),
    );
    ui.label(egui::RichText::new(format!("{}/63", edited.len())).weak());
    ui.label("Use 1–63 printable ASCII characters. Changes take effect after fully restarting Destiny 2.");

    if !response.changed() {
        return false;
    }

    set_player_name(document, &edited)
}

fn valid_player_name(name: &str) -> Option<&str> {
    (!name.is_empty() && name.len() <= 63 && name.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
        .then_some(name)
}

fn set_player_name(document: &mut Value, name: &str) -> bool {
    let Some(name) = valid_player_name(name) else {
        return false;
    };
    let Some(value) = document.pointer_mut(pointer::PERSONA_NAME) else {
        return false;
    };
    if !value.is_string() || value.as_str() == Some(name) {
        return false;
    }
    *value = Value::String(name.to_owned());
    true
}

/// A group's page: its heading, then a row for each setting it describes.
pub(super) fn draw_group(
    ui: &mut egui::Ui,
    settings: &mut Map<String, Value>,
    group: &SettingGroup,
) -> bool {
    let Some(values) = group_mut(settings, group.name) else {
        missing_group(ui, group.name);
        return false;
    };
    ui.heading(group.heading);
    ui.label(group.description);
    ui.add_space(8.0);
    egui::Grid::new(("game_settings", group.name))
        .num_columns(2)
        .spacing(theme::SETTINGS_GRID_SPACING)
        .striped(true)
        .show(ui, |ui| {
            let mut changed = false;
            for setting in group.settings {
                changed |= draw_setting(ui, values, setting);
            }
            changed
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_name_matches_sunrise_persona_format() {
        assert_eq!(valid_player_name("Player"), Some("Player"));
        assert!(valid_player_name(&"x".repeat(63)).is_some());
        assert_eq!(valid_player_name(""), None);
        assert_eq!(valid_player_name(&"x".repeat(64)), None);
        assert_eq!(valid_player_name("Guardian\n"), None);
        assert_eq!(valid_player_name("Guardián"), None);
    }

    #[test]
    fn player_name_edit_preserves_every_other_json_value() {
        let mut document = serde_json::json!({
            "steam": {
                "user": {
                    "persona_name": "Player",
                    "future_user_setting": { "keep": [1, 2, 3] }
                },
                "future_steam_setting": true
            },
            "unknown_top_level_data": { "also_keep": "untouched" }
        });
        let mut expected = document.clone();
        *expected.pointer_mut("/steam/user/persona_name").unwrap() =
            Value::String("Guardian".into());

        assert!(set_player_name(&mut document, "Guardian"));

        assert_eq!(document, expected);
    }
}
