//! One row of a settings grid. Every editable setting is drawn through
//! here, so a missing or malformed one reads the same wherever it appears.

use eframe::egui;
use serde_json::{Map, Value};

use crate::{model::name_of, theme};

use super::tables::{Domain, Setting};

/// One row of a settings grid: the label, then the widget for the value, or
/// why there is no widget. Every editable setting is drawn through here, so a
/// missing or malformed one reads the same wherever it appears.
///
/// `read` pulls the setting out in the type its widget edits, and `edit` draws
/// that widget and returns the replacement only when the user moved it.
fn field<T>(
    ui: &mut egui::Ui,
    values: &mut Map<String, Value>,
    key: &str,
    label: &str,
    read: impl Fn(&Value) -> Option<T>,
    edit: impl FnOnce(&mut egui::Ui, T) -> Option<Value>,
) -> bool {
    ui.label(label);
    let changed = match values.get_mut(key) {
        None => {
            missing(ui);
            false
        }
        Some(value) => match read(value) {
            None => {
                invalid(ui);
                false
            }
            Some(current) => match edit(ui, current) {
                Some(replacement) => {
                    *value = replacement;
                    true
                }
                None => false,
            },
        },
    };
    ui.end_row();
    changed
}

/// Draws whichever widget the setting's domain calls for.
pub(super) fn draw_setting(
    ui: &mut egui::Ui,
    values: &mut Map<String, Value>,
    setting: &Setting,
) -> bool {
    let Setting { key, label, domain } = setting;
    match *domain {
        Domain::Flag => field(ui, values, key, label, Value::as_bool, |ui, mut checked| {
            ui.checkbox(&mut checked, "")
                .changed()
                .then(|| Value::Bool(checked))
        }),
        Domain::Choice(choices) => {
            field(ui, values, key, label, Value::as_u64, |ui, mut current| {
                let mut changed = false;
                egui::ComboBox::from_id_salt(("game_setting", key))
                    .selected_text(name_of(choices, current).unwrap_or("Invalid value"))
                    .width(theme::COMBO)
                    .show_ui(ui, |ui| {
                        for &(candidate, name) in choices {
                            changed |= ui.selectable_value(&mut current, candidate, name).changed();
                        }
                    });
                changed.then(|| Value::from(current))
            })
        }
        Domain::Range { minimum, maximum } => {
            field(ui, values, key, label, Value::as_u64, |ui, mut current| {
                ui.add(egui::Slider::new(&mut current, minimum..=maximum))
                    .changed()
                    .then(|| Value::from(current))
            })
        }
        Domain::Offset {
            minimum,
            maximum,
            display_offset,
        } => field(ui, values, key, label, Value::as_u64, |ui, current| {
            let mut displayed = current.saturating_add(display_offset);
            ui.add(egui::Slider::new(
                &mut displayed,
                minimum + display_offset..=maximum + display_offset,
            ))
            .changed()
            .then(|| Value::from(displayed - display_offset))
        }),
        Domain::Decimal {
            minimum,
            maximum,
            step,
        } => field(ui, values, key, label, Value::as_f64, |ui, mut current| {
            ui.add(
                egui::Slider::new(&mut current, minimum..=maximum)
                    .step_by(step)
                    .fixed_decimals(1),
            )
            .changed()
            // An infinite or NaN value has no JSON number to write, and
            // leaves the setting as it was.
            .then(|| serde_json::Number::from_f64(current).map(Value::Number))
            .flatten()
        }),
        // Shown so the page accounts for every setting in the group, but
        // Sunrise requires these to stay exactly as they are.
        Domain::Exact(_) | Domain::ExactDecimal(_) => {
            ui.label(*label);
            match values.get(*key) {
                Some(value) => {
                    ui.add_enabled(false, egui::Label::new(value.to_string()))
                        .on_hover_text("Project Sunrise requires this exact value.");
                }
                None => missing(ui),
            }
            ui.end_row();
            false
        }
    }
}

pub(super) fn missing(ui: &mut egui::Ui) {
    ui.colored_label(theme::ERROR, "Missing");
}

pub(super) fn invalid(ui: &mut egui::Ui) {
    ui.colored_label(theme::ERROR, "Invalid value");
}

pub(super) fn group_mut<'a>(
    settings: &'a mut Map<String, Value>,
    name: &str,
) -> Option<&'a mut Map<String, Value>> {
    settings.get_mut(name)?.as_object_mut()
}

pub(super) fn missing_group(ui: &mut egui::Ui, name: &str) {
    ui.colored_label(
        theme::ERROR,
        format!("The {name} settings group is missing or malformed."),
    );
}
