//! Key bindings: the picker that edits them, the names Sunrise accepts,
//! and what counts as a valid binding in each schema.

use eframe::egui;
use serde_json::{Map, Value};

use crate::theme;

use super::{
    fields::{invalid, missing, missing_group},
    tables::ACTIONS,
    validate::group,
};

#[derive(Default)]
pub(crate) struct KeyBindingUiState {
    action_search: String,
    picker: BindingPickerState,
}

impl KeyBindingUiState {
    pub(crate) fn clear_pickers(&mut self) {
        self.picker = BindingPickerState::default();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BindingModifier {
    #[default]
    None,
    Shift,
    Control,
    Alt,
}

impl BindingModifier {
    const ALL: [(Self, &'static str); 4] = [
        (Self::None, "None"),
        (Self::Shift, "Shift"),
        (Self::Control, "Ctrl"),
        (Self::Alt, "Alt"),
    ];

    const fn input_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Shift => Some("shift"),
            Self::Control => Some("control"),
            Self::Alt => Some("alt"),
        }
    }
}

#[derive(Default)]
struct BindingPickerState {
    query: String,
    modifier: BindingModifier,
}

pub(super) fn draw_key_bindings(
    ui: &mut egui::Ui,
    settings: &mut Map<String, Value>,
    state: &mut KeyBindingUiState,
    editable: bool,
) -> bool {
    let Some(bindings) = settings
        .get_mut("key_bindings")
        .and_then(Value::as_object_mut)
    else {
        missing_group(ui, "key bindings");
        return false;
    };
    ui.heading("Key bindings");
    if editable {
        ui.label("Choose a primary and secondary input for each action. Changes apply after Destiny 2 is fully restarted.");
    } else {
        ui.label(
            "Guided editing is available for Sunrise schema 3 and 6. These bindings are shown read-only.",
        );
    }
    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(&mut state.action_search)
            .hint_text("Search actions…")
            .desired_width(320.0),
    );
    ui.add_space(8.0);
    let needle = state.action_search.trim().to_lowercase();
    let mut changed = false;
    egui::Grid::new("game_key_bindings_grid")
        .num_columns(3)
        .spacing(theme::FORM_GRID_SPACING)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Action");
            ui.strong("Primary");
            ui.strong("Secondary");
            ui.end_row();
            let mut visible = 0usize;
            for &(key, label) in ACTIONS {
                if !needle.is_empty()
                    && !label.to_lowercase().contains(&needle)
                    && !key.contains(&needle)
                {
                    continue;
                }
                visible += 1;
                ui.label(label);
                let Some(binding) = bindings.get_mut(key).and_then(Value::as_object_mut) else {
                    missing(ui);
                    missing(ui);
                    ui.end_row();
                    continue;
                };
                if editable {
                    changed |=
                        binding_picker(ui, state, key, "primary", binding.get_mut("primary"));
                    changed |=
                        binding_picker(ui, state, key, "secondary", binding.get_mut("secondary"));
                } else {
                    binding_label(ui, binding.get("primary"));
                    binding_label(ui, binding.get("secondary"));
                }
                ui.end_row();
            }
            if visible == 0 {
                ui.label(egui::RichText::new("No matching actions").weak());
                ui.end_row();
            }
        });
    changed
}

fn binding_picker(
    ui: &mut egui::Ui,
    state: &mut KeyBindingUiState,
    action: &str,
    half: &str,
    value: Option<&mut Value>,
) -> bool {
    let Some(value) = value else {
        missing(ui);
        return false;
    };

    let (label, valid) = binding_value_label(value);
    let label = if valid {
        egui::RichText::new(label)
    } else {
        egui::RichText::new(label).color(theme::ERROR)
    };
    let popup_id = ui.make_persistent_id(("key-binding-picker", action, half));
    let button = ui.add_sized(
        [220.0, ui.spacing().interact_size.y],
        egui::Button::new(label),
    );
    if button.clicked() {
        state.picker = BindingPickerState {
            query: String::new(),
            modifier: value
                .as_str()
                .and_then(modified_input)
                .map_or(BindingModifier::None, |(modifier, _)| {
                    binding_modifier(modifier)
                }),
        };
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }

    let picker = &mut state.picker;
    let mut selection = None::<Option<String>>;
    egui::popup::popup_below_widget(
        ui,
        popup_id,
        &button,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(400.0);
            ui.label(egui::RichText::new("Modifier").strong());
            ui.horizontal_wrapped(|ui| {
                for (modifier, label) in BindingModifier::ALL {
                    ui.selectable_value(&mut picker.modifier, modifier, label);
                }
            });
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut picker.query)
                    .hint_text("Search key names…")
                    .desired_width(380.0),
            );
            ui.separator();

            let current = value.as_str().map(trim_input_name);
            let needle = picker.query.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .min_scrolled_height(300.0)
                .max_height(400.0)
                .show(ui, |ui| {
                    if ui.selectable_label(value.is_null(), "Unassigned").clicked() {
                        selection = Some(None);
                    }
                    ui.separator();

                    let mut visible = 0usize;
                    for &key in NAMED_INPUTS {
                        let display = display_input_name(key);
                        if !needle.is_empty()
                            && !key.to_lowercase().contains(&needle)
                            && !display.to_lowercase().contains(&needle)
                        {
                            continue;
                        }
                        visible += 1;
                        let input = picker
                            .modifier
                            .input_name()
                            .map_or_else(|| key.to_owned(), |modifier| format!("{modifier}+{key}"));
                        debug_assert!(valid_named_input(&input));
                        if ui
                            .selectable_label(
                                current.is_some_and(|current| current.eq_ignore_ascii_case(&input)),
                                display,
                            )
                            .clicked()
                        {
                            selection = Some(Some(input));
                        }
                    }
                    if visible == 0 {
                        ui.label(egui::RichText::new("No matching keys found").weak());
                    }
                });
        },
    );

    let Some(selection) = selection else {
        return false;
    };
    let Ok(changed) = set_named_binding_value(value, selection.as_deref()) else {
        return false;
    };
    ui.memory_mut(egui::Memory::close_popup);
    changed
}

fn binding_label(ui: &mut egui::Ui, value: Option<&Value>) {
    let Some(value) = value else {
        return missing(ui);
    };
    if value.is_null() {
        ui.label(egui::RichText::new("Unassigned").weak());
    } else if let Some(code) = value.as_u64() {
        ui.add_enabled(false, egui::Label::new(code.to_string()));
    } else if let Some(name) = value.as_str() {
        ui.add_enabled(false, egui::Label::new(name));
    } else {
        invalid(ui);
    }
}

pub(super) fn validate_key_bindings(settings: &Map<String, Value>) -> Result<(), String> {
    let bindings = group(settings, "key_bindings")?;
    for &(key, label) in ACTIONS {
        let binding = bindings
            .get(key)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("Key binding {label} must be an object"))?;
        input_code(binding, label, "primary")?;
        input_code(binding, label, "secondary")?;
    }
    Ok(())
}

// These are the decoded input names accepted by Sunrise schemas 3 (Project
// Sunrise 0.2 and 0.2.1). Sunrise's raw table contains both its backslash name
// and its JSON-escaped spelling; serde represents the usable value as one
// decoded backslash, leaving 120 logical choices here. Matching is ASCII
// case-insensitive, just like Sunrise.
const NAMED_INPUTS: &[&str; 120] = &[
    "escape",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "print screen",
    "scroll lock",
    "pause",
    "`",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "0",
    "-",
    "=",
    "backspace",
    "tab",
    "q",
    "w",
    "e",
    "r",
    "t",
    "y",
    "u",
    "i",
    "o",
    "p",
    "[",
    "]",
    r"\",
    "caps lock",
    "a",
    "s",
    "d",
    "f",
    "g",
    "h",
    "j",
    "k",
    "l",
    ";",
    "'",
    "return",
    "left shift",
    "z",
    "x",
    "c",
    "v",
    "b",
    "n",
    "m",
    ",",
    ".",
    "/",
    "right shift",
    "left control",
    "left windows",
    "left alt",
    "space",
    "right alt",
    "right windows",
    "menu",
    "right control",
    "up",
    "down",
    "left",
    "right",
    "insert",
    "home",
    "page up",
    "delete",
    "end",
    "page down",
    "num lock",
    "keypad /",
    "keypad *",
    "keypad 0",
    "keypad 1",
    "keypad 2",
    "keypad 3",
    "keypad 4",
    "keypad 5",
    "keypad 6",
    "keypad 7",
    "keypad 8",
    "keypad 9",
    "keypad -",
    "keypad +",
    "keypad enter",
    "keypad .",
    "<",
    "shift",
    "control",
    "key_windows",
    "alt",
    "left mouse button",
    "middle mouse button",
    "right mouse button",
    "extra mouse button 1",
    "extra mouse button 2",
    "mouse wheel up",
    "mouse wheel down",
    "unused",
    "ctrl",
    "left ctrl",
    "right ctrl",
];

const MODIFIER_INPUTS: &[&str; 12] = &[
    "left shift",
    "right shift",
    "shift",
    "left control",
    "right control",
    "control",
    "ctrl",
    "left ctrl",
    "right ctrl",
    "left alt",
    "right alt",
    "alt",
];

fn trim_input_name(name: &str) -> &str {
    name.trim_matches([' ', '\t'])
}

fn matches_input_name(candidate: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| candidate.eq_ignore_ascii_case(name))
}

fn modified_input(name: &str) -> Option<(&str, &str)> {
    let name = trim_input_name(name);
    if name.is_empty() || matches_input_name(name, NAMED_INPUTS) {
        return None;
    }
    let separator = name.find(['+', '-'])?;
    let modifier = trim_input_name(&name[..separator]);
    let key = trim_input_name(&name[separator + 1..]);
    (matches_input_name(modifier, MODIFIER_INPUTS) && matches_input_name(key, NAMED_INPUTS))
        .then_some((modifier, key))
}

fn valid_named_input(name: &str) -> bool {
    let name = trim_input_name(name);
    !name.is_empty() && (matches_input_name(name, NAMED_INPUTS) || modified_input(name).is_some())
}

fn binding_modifier(name: &str) -> BindingModifier {
    if matches_input_name(name, &["left shift", "right shift", "shift"]) {
        BindingModifier::Shift
    } else if matches_input_name(
        name,
        &[
            "left control",
            "right control",
            "control",
            "ctrl",
            "left ctrl",
            "right ctrl",
        ],
    ) {
        BindingModifier::Control
    } else if matches_input_name(name, &["left alt", "right alt", "alt"]) {
        BindingModifier::Alt
    } else {
        BindingModifier::None
    }
}

fn display_input_part(name: &str) -> String {
    name.replace('_', " ")
        .split(' ')
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_input_name(name: &str) -> String {
    modified_input(name).map_or_else(
        || display_input_part(trim_input_name(name)),
        |(modifier, key)| {
            format!(
                "{} + {}",
                display_input_part(modifier),
                display_input_part(key)
            )
        },
    )
}

fn binding_value_label(value: &Value) -> (String, bool) {
    if value.is_null() {
        ("Unassigned".into(), true)
    } else if let Some(name) = value.as_str() {
        if valid_named_input(name) {
            (display_input_name(name), true)
        } else {
            (format!("Invalid: {name}"), false)
        }
    } else {
        ("Invalid value".into(), false)
    }
}

fn set_named_binding_value(value: &mut Value, input: Option<&str>) -> Result<bool, String> {
    if let Some(input) = input
        && !valid_named_input(input)
    {
        return Err(format!("Unsupported Sunrise key name: {input}"));
    }
    let replacement = input.map_or(Value::Null, |input| Value::String(input.into()));
    if *value == replacement {
        return Ok(false);
    }
    *value = replacement;
    Ok(true)
}

/// One half of a binding. Schema 6 names its keys, so a numeric code is the
/// mark of an older layout rather than a value this build can read.
fn input_code(binding: &Map<String, Value>, label: &str, half: &str) -> Result<(), String> {
    let Some(value) = binding.get(half) else {
        return Err(format!("Key binding {label} is missing its {half} value"));
    };
    if value.is_null() || value.as_str().is_some_and(valid_named_input) {
        return Ok(());
    }
    Err(format!(
        "Key binding {label} {half} must be unassigned, a recognized key name, or one modifier plus a key"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_named_key_bindings_are_accepted() {
        let named = serde_json::json!({"primary": "left mouse button", "secondary": null});
        let named = named.as_object().unwrap();
        assert_eq!(input_code(named, "Fire", "primary"), Ok(()));
        // Unassigned is a real state, not a missing value.
        assert_eq!(input_code(named, "Fire", "secondary"), Ok(()));

        // The numeric codes of the layouts before schema 6 are not read.
        let numeric = serde_json::json!({"primary": 109, "secondary": null});
        assert!(input_code(numeric.as_object().unwrap(), "Fire", "primary").is_err());

        let missing = serde_json::json!({"secondary": null});
        assert!(input_code(missing.as_object().unwrap(), "Fire", "primary").is_err());
    }

    #[test]
    fn named_key_binding_validation_matches_sunrise() {
        for valid in [
            "left mouse button",
            "A",
            "\tCTRL + keypad -\t",
            "right alt-page down",
            r"\",
        ] {
            assert!(valid_named_input(valid), "expected {valid:?} to be valid");
        }

        for invalid in [
            "not-a-key",
            "left windows+a",
            "shift+ctrl+a",
            "shift+",
            "a+b",
            "\nA\n",
            r"\\",
        ] {
            assert!(
                !valid_named_input(invalid),
                "expected {invalid:?} to be invalid"
            );
        }

        let invalid = serde_json::json!({"primary": "not-a-key", "secondary": null});
        assert!(input_code(invalid.as_object().unwrap(), "Fire", "primary").is_err());
    }

    #[test]
    fn every_picker_choice_is_accepted_by_sunrise() {
        for &key in NAMED_INPUTS {
            assert!(valid_named_input(key), "direct key {key:?}");
            for modifier in ["shift", "control", "alt"] {
                let input = format!("{modifier}+{key}");
                assert!(valid_named_input(&input), "modified key {input:?}");
            }
        }
    }

    #[test]
    fn named_binding_edits_only_replace_the_selected_value() {
        let mut binding = serde_json::json!({
            "primary": "not-a-key",
            "secondary": null,
            "future_binding_data": { "keep": [1, 2, 3] }
        });

        let untouched = binding.clone();
        assert!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), Some("not-a-key"))
                .is_err()
        );
        assert_eq!(binding, untouched);

        assert_eq!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), Some("control+a")),
            Ok(true)
        );
        assert_eq!(
            binding,
            serde_json::json!({
                "primary": "control+a",
                "secondary": null,
                "future_binding_data": { "keep": [1, 2, 3] }
            })
        );
        assert_eq!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), Some("control+a")),
            Ok(false)
        );
        assert_eq!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), None),
            Ok(true)
        );
        assert!(binding.pointer("/primary").unwrap().is_null());
        assert_eq!(
            binding.pointer("/future_binding_data/keep"),
            Some(&serde_json::json!([1, 2, 3]))
        );
    }
}
