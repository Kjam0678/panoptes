//! The game settings page: the tabs Sunrise replicates to Destiny 2, and
//! what each of them will accept.

mod fields;
mod key_bindings;
mod tables;
mod tabs;
mod validate;

pub(crate) use key_bindings::KeyBindingUiState;
use key_bindings::draw_key_bindings;
use tables::{AUDIO, CONTROLS, DISPLAY, INTERFACE, SOCIAL};
use tabs::{draw_group, draw_player};
pub(crate) use validate::validate;

use eframe::egui;
use serde_json::{Map, Value};

use crate::{model::pointer, theme};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tab {
    Player,
    Controls,
    Audio,
    Display,
    Interface,
    Social,
    KeyBindings,
}

/// The one Sunrise layout this build reads. Schema 6 brought held items, a
/// megabyte of room, and profile items in their own form; the layouts before
/// it are no longer supported.
const SCHEMA: u64 = 6;

/// Refuses anything this build was not written against, which is what stops a
/// file of another layout being read as though it were this one.
fn require_supported_schema(document: &Value) -> Result<(), String> {
    match schema_version(document) {
        Some(SCHEMA) => Ok(()),
        Some(version) => Err(format!(
            "Project Sunrise settings schema version {version} is not supported by this build, which reads schema {SCHEMA}"
        )),
        None => Err("Project Sunrise settings schema version is missing or invalid".into()),
    }
}

pub(crate) fn schema_version(document: &Value) -> Option<u64> {
    document.get("version").and_then(Value::as_u64)
}

pub(crate) fn future_schema_version(document: &Value) -> Option<u64> {
    schema_version(document).filter(|version| *version > SCHEMA)
}

fn key_bindings_editable(document: &Value) -> bool {
    schema_version(document) == Some(SCHEMA)
}

pub(crate) fn draw_page(
    ui: &mut egui::Ui,
    document: &mut Value,
    tab: &mut Tab,
    key_bindings: &mut KeyBindingUiState,
) -> bool {
    let bindings_editable = key_bindings_editable(document);
    ui.heading("Game settings");
    ui.label("Edit the settings replicated to Destiny 2 by Project Sunrise.");
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(tab, Tab::Player, "Player");
        ui.selectable_value(tab, Tab::Controls, "Controls");
        ui.selectable_value(tab, Tab::Audio, "Audio");
        ui.selectable_value(tab, Tab::Display, "Display");
        ui.selectable_value(tab, Tab::Interface, "Interface");
        ui.selectable_value(tab, Tab::Social, "Social");
        ui.selectable_value(tab, Tab::KeyBindings, "Key bindings")
            .on_hover_text(if bindings_editable {
                "Edit named key bindings, the form Sunrise has read since schema 3."
            } else {
                "Key bindings are shown read-only for this settings schema."
            });
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .show(ui, |ui| match *tab {
            Tab::Player => draw_player(ui, document),
            Tab::Controls => draw_account_settings(ui, document, |ui, s| draw_group(ui, s, &CONTROLS)),
            Tab::Audio => draw_account_settings(ui, document, |ui, s| draw_group(ui, s, &AUDIO)),
            Tab::Display => draw_account_settings(ui, document, |ui, s| draw_group(ui, s, &DISPLAY)),
            Tab::Interface => {
                draw_account_settings(ui, document, |ui, s| draw_group(ui, s, &INTERFACE))
            }
            Tab::Social => draw_account_settings(ui, document, |ui, s| draw_group(ui, s, &SOCIAL)),
            Tab::KeyBindings => draw_account_settings(ui, document, |ui, settings| {
                draw_key_bindings(ui, settings, key_bindings, bindings_editable)
            }),
        })
        .inner
}

fn draw_account_settings(
    ui: &mut egui::Ui,
    document: &mut Value,
    draw: impl FnOnce(&mut egui::Ui, &mut Map<String, Value>) -> bool,
) -> bool {
    let Some(settings) = document
        .pointer_mut(pointer::ACCOUNT_SETTINGS)
        .and_then(Value::as_object_mut)
    else {
        ui.colored_label(
            theme::ERROR,
            "This settings.json has no state.account.settings object.",
        );
        return false;
    };
    draw(ui, settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_schema_six_is_supported() {
        assert_eq!(require_supported_schema(&serde_json::json!({"version": 6})), Ok(()));
        // The layouts before it are readable but no longer edited.
        for version in [2, 3, 4, 7] {
            let document = serde_json::json!({ "version": version });
            assert!(require_supported_schema(&document).is_err(), "schema {version}");
            assert!(!key_bindings_editable(&document));
        }
        assert!(require_supported_schema(&serde_json::json!({})).is_err());
        assert!(key_bindings_editable(&serde_json::json!({"version": 6})));
    }

    #[test]
    fn only_newer_schema_versions_require_a_confirmation() {
        assert_eq!(schema_version(&serde_json::json!({"version": 3})), Some(3));
        assert_eq!(schema_version(&serde_json::json!({"version": "3"})), None);
        assert_eq!(
            future_schema_version(&serde_json::json!({"version": 2})),
            None
        );
        assert_eq!(
            future_schema_version(&serde_json::json!({"version": 3})),
            None
        );
        // 4 and 5 came and went before the one this build reads; neither is
        // ahead of it, so neither is a reason to warn.
        assert_eq!(
            future_schema_version(&serde_json::json!({"version": 6})),
            None
        );
        assert_eq!(
            future_schema_version(&serde_json::json!({"version": 7})),
            Some(7)
        );
    }
}
