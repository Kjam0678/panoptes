//! The character's own fields: class, race, gender, subclass, and the
//! abilities a subclass allows.

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{AbilityChoice, AbilityOptions, ItemDef},
    model::{
        CLASSES, GENDERS, RACES, SUBCLASS_BUCKET, class_name, format_hash, parse_unsigned_value,
        pointer, subclass_slot,
    },
    settings,
    status::Change,
    theme,
};

use super::Page;

impl Page<'_> {
    pub fn draw_character_fields(&mut self, ui: &mut egui::Ui, character: usize) -> Option<Change> {
        let path = pointer::character(character);
        let object = self.document.pointer(&path).and_then(Value::as_object)?;
        let read = |key: &str, fallback: u64| object.get(key).and_then(Value::as_u64).unwrap_or(fallback);
        let original_class = read("class", 0);
        let (mut class_type, mut race, mut gender) =
            (original_class, read("race", 0), read("gender", 0));
        let mut movement = read("movement_ability", 4);
        let mut grenade = read("grenade_ability", 7);
        let mut super_ability = read("super_ability", 10);
        let mut melee = read("melee_ability", 11);
        let mut class_ability = read("class_ability", 2);
        let soid = object
            .get("soid")
            .and_then(parse_unsigned_value)
            .map_or_else(|| "Unknown".to_owned(), format_hash);
        let ability_warning = settings::character_ability_issue(object);

        let mut subclass_hash = self
            .document
            .pointer(&format!("{path}/equipment/subclass/definition_hash"))
            .and_then(parse_unsigned_value);
        let subclasses: Vec<ItemDef> = self
            .catalog
            .items
            .iter()
            .filter(|item| item.bucket_hash == SUBCLASS_BUCKET && item.class_type == class_type)
            .cloned()
            .collect();
        let mut abilities = subclass_hash
            .and_then(|hash| self.catalog.get_for_bucket(hash, SUBCLASS_BUCKET))
            .map(|item| item.abilities.clone())
            .unwrap_or_default();
        let mut attunement = attunement_index(&abilities, super_ability, melee);
        let mut equip_subclass = None::<ItemDef>;

        ui.horizontal(|ui| {
            ui.heading(format!("Character {}", character + 1));
            ui.label(egui::RichText::new(soid).monospace().weak());
        });
        if let Some(warning) = &ability_warning {
            ui.colored_label(
                theme::WARNING,
                format!("Warning: {warning}. Choose supported abilities below and save before launching."),
            );
        }
        ui.add_space(6.0);

        egui::Grid::new(("character", character))
            .num_columns(2)
            .spacing(theme::FORM_GRID_SPACING)
            .show(ui, |ui| {
                for (label, id, value, choices) in [
                    ("Class", "class", &mut class_type, CLASSES),
                    ("Race", "race", &mut race, RACES),
                    ("Gender", "gender", &mut gender, GENDERS),
                ] {
                    ui.label(label);
                    combo(ui, id, value, choices);
                    ui.end_row();
                }

                ui.label("Subclass");
                let selected = subclass_hash
                    .and_then(|hash| subclasses.iter().find(|item| item.hash == hash))
                    .map_or("Unknown subclass", |item| item.name.as_str());
                egui::ComboBox::from_id_salt("subclass")
                    .selected_text(selected)
                    .width(theme::WIDE_COMBO)
                    .show_ui(ui, |ui| {
                        for subclass in &subclasses {
                            if ui
                                .selectable_label(subclass_hash == Some(subclass.hash), &subclass.name)
                                .clicked()
                                && subclass_hash != Some(subclass.hash)
                            {
                                subclass_hash = Some(subclass.hash);
                                abilities = subclass.abilities.clone();
                                (movement, grenade, super_ability, melee, class_ability) =
                                    default_abilities(class_type, &abilities);
                                attunement = attunement_index(&abilities, super_ability, melee);
                                equip_subclass = Some(subclass.clone());
                            }
                        }
                    });
                ui.end_row();

                ui.label("Attunement");
                let previous = attunement;
                let selected = abilities
                    .attunements
                    .get(attunement)
                    .map_or("No attunement data", |choice| choice.name.as_str());
                egui::ComboBox::from_id_salt("attunement")
                    .selected_text(selected)
                    .width(theme::WIDE_COMBO)
                    .show_ui(ui, |ui| {
                        for (index, choice) in abilities.attunements.iter().enumerate() {
                            ui.selectable_value(&mut attunement, index, &choice.name);
                        }
                    });
                ui.end_row();

                // The super and melee entries must stay inside one attunement.
                if let Some(choice) = abilities.attunements.get(attunement) {
                    let pair_is_valid = choice.melee.entry == melee
                        && choice.super_abilities.iter().any(|s| s.entry == super_ability);
                    if attunement != previous || !pair_is_valid {
                        melee = choice.melee.entry;
                        super_ability = choice.super_abilities.first().map_or(10, |s| s.entry);
                    }
                    ui.label("Attunement perks");
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                choice
                                    .perks
                                    .iter()
                                    .map(|perk| perk.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" • "),
                            )
                            .weak(),
                        )
                        .wrap(),
                    );
                    ui.end_row();
                }

                ui.label("Movement ability");
                ability_combo(ui, "movement", &mut movement, &abilities.movement);
                ui.end_row();
                ui.label("Grenade ability");
                ability_combo(ui, "grenade", &mut grenade, &abilities.grenade);
                ui.end_row();
                if let Some(choice) = abilities.attunements.get(attunement) {
                    ui.label("Super ability");
                    ui.label(choice.super_abilities.first().map_or("Unknown super", |s| s.name.as_str()));
                    ui.end_row();
                    ui.label("Melee ability");
                    ui.label(&choice.melee.name);
                    ui.end_row();
                } else {
                    ui.label("Super ability");
                    ability_combo(ui, "super", &mut super_ability, &abilities.super_ability);
                    ui.end_row();
                    ui.label("Melee ability");
                    ability_combo(ui, "melee", &mut melee, &abilities.melee);
                    ui.end_row();
                }
                ui.label("Class ability");
                ability_combo(ui, "class_ability", &mut class_ability, &abilities.class_ability);
                ui.end_row();
            });

        let mut changed = false;
        if class_type != original_class {
            // Keep this class's own armor so the character stays wearable.
            let template = settings::collect_class_armor_defaults(self.document)
                .get(&class_type)
                .cloned();
            if let (Some(template), Some(object)) = (
                template,
                self.document.pointer_mut(&path).and_then(Value::as_object_mut),
            ) {
                changed |= settings::restore_class_armor(object, &template);
            }
            if let Some(subclass) = subclasses
                .iter()
                .find(|item| item.class_type == class_type)
                .cloned()
            {
                equip_subclass = Some(subclass);
            }
        }
        if let Some(object) = self.document.pointer_mut(&path).and_then(Value::as_object_mut) {
            for (key, value) in [
                ("class", class_type),
                ("race", race),
                ("gender", gender),
                ("movement_ability", movement),
                ("grenade_ability", grenade),
                ("super_ability", super_ability),
                ("melee_ability", melee),
                ("class_ability", class_ability),
            ] {
                if object.get(key).and_then(Value::as_u64) != Some(value) {
                    object.insert(key.into(), Value::from(value));
                    changed = true;
                }
            }
        }
        if let Some(subclass) = equip_subclass {
            return Some(
                settings::equip_definition(
                    self.document,
                    character,
                    subclass_slot(),
                    subclass.hash,
                    &subclass.default_plugs,
                )
                .map(|()| format!("Equipped {}", subclass.name)),
            );
        }
        changed.then(|| Ok(format!("Updated character {}", character + 1)))
    }
}

fn combo(ui: &mut egui::Ui, id: &str, value: &mut u64, choices: &[(u64, &str)]) {
    let selected = choices
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("Invalid", |(_, name)| *name);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(theme::NARROW_COMBO)
        .show_ui(ui, |ui| {
            for &(candidate, name) in choices {
                ui.selectable_value(value, candidate, name);
            }
        });
}

fn ability_combo(ui: &mut egui::Ui, id: &str, value: &mut u64, choices: &[AbilityChoice]) {
    let selected = choices
        .iter()
        .find(|choice| choice.entry == *value)
        .map_or_else(|| format!("Unknown entry {value}"), |choice| choice.name.clone());
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(theme::WIDE_COMBO)
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(value, choice.entry, &choice.name);
            }
            if choices.is_empty() {
                ui.label("No named choices for this subclass");
            }
        });
}

/// The attunement that owns a super and melee pairing.
fn attunement_index(abilities: &AbilityOptions, super_ability: u64, melee: u64) -> usize {
    let paths = &abilities.attunements;
    paths
        .iter()
        .position(|path| {
            path.melee.entry == melee
                && path.super_abilities.iter().any(|choice| choice.entry == super_ability)
        })
        .or_else(|| paths.iter().position(|path| path.melee.entry == melee))
        .or_else(|| {
            paths
                .iter()
                .position(|path| path.super_abilities.iter().any(|choice| choice.entry == super_ability))
        })
        .unwrap_or(0)
}

fn default_abilities(class_type: u64, abilities: &AbilityOptions) -> (u64, u64, u64, u64, u64) {
    let pick = |choices: &[AbilityChoice], preferred: u64| {
        choices
            .iter()
            .find(|choice| choice.entry == preferred)
            .or_else(|| choices.first())
            .map_or(preferred, |choice| choice.entry)
    };
    let movement = match class_type {
        0 | 1 => 6,
        _ => 5,
    };
    (
        pick(&abilities.movement, movement),
        pick(&abilities.grenade, 7),
        pick(&abilities.super_ability, 10),
        pick(&abilities.melee, 11),
        pick(&abilities.class_ability, 2),
    )
}

pub fn character_tab_label(character: &Value, index: usize) -> String {
    let class_type = character.get("class").and_then(Value::as_u64).unwrap_or(99);
    format!("Character {} · {}", index + 1, class_name(class_type))
}
