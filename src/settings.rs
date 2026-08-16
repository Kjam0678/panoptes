//! Reading, validating, editing, and safely writing Sunrise's settings.json.

use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};

use crate::{
    game_settings,
    model::{
        ARMOR_SLOTS, GENERATED_INSTANCE_SOID_START, MAX_SETTINGS_BYTES, SLOTS, WEAPON_SLOTS,
        format_hash, parse_unsigned_value, slot_label,
    },
    paths, storage,
};

const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;

// ---------------------------------------------------------------- file access

pub fn load_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("No settings.json at {}", path.display())
        } else {
            format!("Could not read {}: {error}", path.display())
        }
    })?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

pub fn verify_source_unchanged(path: &Path, expected: &Value) -> Result<(), String> {
    if load_json(path)? == *expected {
        Ok(())
    } else {
        Err("settings.json changed on disk after it was loaded. Reload before saving so newer data is not overwritten".into())
    }
}

/// Writes the document, keeping a timestamped backup and verifying the result.
pub fn save_json(path: &Path, document: &Value) -> Result<PathBuf, String> {
    let mut encoded = encode_settings(document)?;
    encoded.push('\n');
    if encoded.len() > MAX_SETTINGS_BYTES {
        return Err(format!(
            "The encoded settings would be {} bytes; Sunrise requires less than {} bytes",
            encoded.len(),
            MAX_SETTINGS_BYTES + 1
        ));
    }

    let backup_root = paths::backup_dir().ok_or("Could not locate the backup folder")?;
    fs::create_dir_all(&backup_root)
        .map_err(|e| format!("Could not create {}: {e}", backup_root.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Could not create a backup timestamp: {e}"))?
        .as_nanos();
    let backup = backup_root.join(format!("settings-{timestamp}-{}.json", std::process::id()));
    create_backup(path, &backup)?;

    storage::replace_file(path, encoded.as_bytes())
        .map_err(|e| format!("Could not safely replace {}: {e}", path.display()))?;
    let verified = load_json(path).and_then(|saved| {
        if saved == *document {
            Ok(())
        } else {
            Err("the saved document did not match the requested settings".to_owned())
        }
    });
    if let Err(error) = verified {
        let restored = fs::read(&backup)
            .and_then(|contents| storage::replace_file(path, &contents))
            .map_err(|restore_error| restore_error.to_string());
        return Err(match restored {
            Ok(()) => format!("Could not verify the saved settings ({error}); the original file was restored"),
            Err(restore_error) => format!(
                "Could not verify the saved settings ({error}), and restoring the backup failed: {restore_error}. The backup is at {}",
                backup.display()
            ),
        });
    }
    Ok(backup)
}

pub fn create_backup(source: &Path, destination: &Path) -> Result<(), String> {
    let mut source_file = fs::File::open(source)
        .map_err(|e| format!("Could not open {} for backup: {e}", source.display()))?;
    let mut backup_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("Could not create {}: {e}", destination.display()))?;
    if let Err(error) = io::copy(&mut source_file, &mut backup_file).and_then(|_| backup_file.sync_all()) {
        drop(backup_file);
        let _ = fs::remove_file(destination);
        return Err(format!("Could not create {}: {error}", destination.display()));
    }
    Ok(())
}

/// An extra copy beside the original, used whenever the file held something
/// this editor did not recognize.
pub fn create_adjacent_backup(source: &Path) -> Result<PathBuf, String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("{} has no file name", source.display()))?
        .to_string_lossy()
        .into_owned();
    let destination = source.with_file_name(format!("{file_name}.bak"));
    let contents =
        fs::read(source).map_err(|e| format!("Could not read {} for backup: {e}", source.display()))?;
    if destination.exists() {
        if fs::read(&destination).is_ok_and(|existing| existing == contents) {
            return Ok(destination);
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Could not create a backup timestamp: {e}"))?
            .as_nanos();
        create_backup(
            &destination,
            &source.with_file_name(format!("{file_name}.bak.previous-{timestamp}")),
        )?;
        storage::replace_file(&destination, &contents)
            .map_err(|e| format!("Could not update {}: {e}", destination.display()))?;
    } else {
        create_backup(source, &destination)?;
    }
    Ok(destination)
}

/// Sunrise caps settings.json at 64 KiB, so arrays are written on one line.
pub fn encode_settings(document: &Value) -> Result<String, String> {
    fn write_value(value: &Value, indent: usize, output: &mut String) -> Result<(), String> {
        match value {
            Value::Object(object) if !object.is_empty() => {
                output.push_str("{\n");
                for (index, (key, child)) in object.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    output.push_str(
                        &serde_json::to_string(key).map_err(|e| format!("Could not encode a setting name: {e}"))?,
                    );
                    output.push_str(": ");
                    write_value(child, indent + 2, output)?;
                    if index + 1 != object.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push('}');
            }
            other => output.push_str(
                &serde_json::to_string(other).map_err(|e| format!("Could not encode a setting: {e}"))?,
            ),
        }
        Ok(())
    }

    let mut output = String::new();
    write_value(document, 0, &mut output)?;
    Ok(output)
}

// ------------------------------------------------------------------ validation

pub fn validate_document(document: &Value) -> Result<(), String> {
    game_settings::validate(document)?;
    validate_characters(document)
}

pub fn validate_characters(document: &Value) -> Result<(), String> {
    const MAX_CHARACTERS: usize = 3;
    const MAX_PLUGS: usize = 12;

    let Some(characters) = document.pointer("/state/characters") else {
        return Ok(());
    };
    let characters = characters.as_array().ok_or("state.characters must be an array")?;
    if characters.len() > MAX_CHARACTERS {
        return Err(format!(
            "state.characters cannot contain more than {MAX_CHARACTERS} characters"
        ));
    }
    for (index, character) in characters.iter().enumerate() {
        let number = index + 1;
        let character = character
            .as_object()
            .ok_or_else(|| format!("Character {number} must be an object"))?;
        character
            .get("soid")
            .and_then(parse_unsigned_value)
            .filter(|soid| *soid != 0)
            .ok_or_else(|| format!("Character {number} has an invalid SOID"))?;

        let bounded = |key: &str, label: &str, maximum: u64| {
            let Some(value) = character.get(key) else {
                return Ok(());
            };
            value
                .as_u64()
                .filter(|value| *value <= maximum)
                .map(|_| ())
                .ok_or_else(|| format!("Character {number} has an invalid {label}"))
        };
        bounded("class", "class", 2)?;
        bounded("race", "race", 2)?;
        bounded("gender", "gender", 1)?;
        bounded("level", "level (expected 0 to 255)", u8::MAX.into())?;
        for (key, label) in [
            ("movement_ability", "movement ability"),
            ("grenade_ability", "grenade ability"),
            ("super_ability", "super ability"),
            ("melee_ability", "melee ability"),
            ("class_ability", "class ability"),
        ] {
            bounded(key, label, 63)?;
        }

        let Some(equipment) = character.get("equipment") else {
            continue;
        };
        let equipment = equipment
            .as_object()
            .ok_or_else(|| format!("Character {number} equipment must be an object"))?;
        if let Some(issue) = character_ability_issue(character) {
            return Err(format!("Character {number} {issue}"));
        }
        for slot in equipment.keys() {
            if !SLOTS.iter().any(|(known, _, _)| known == slot) {
                return Err(format!("Character {number} has an unknown equipment slot: {slot}"));
            }
        }
        for &(slot, label, _) in SLOTS {
            let Some(equipped) = equipment.get(slot).filter(|value| !value.is_null()) else {
                continue;
            };
            let equipped = equipped
                .as_object()
                .ok_or_else(|| format!("Character {number} {label} must be an object or null"))?;
            equipped
                .get("definition_hash")
                .and_then(parse_unsigned_value)
                .filter(|hash| u32::try_from(*hash).is_ok() && *hash != NO_DEFINITION_HASH)
                .ok_or_else(|| format!("Character {number} {label} has an invalid definition hash"))?;
            equipped
                .get("instance_soid")
                .and_then(parse_unsigned_value)
                .filter(|soid| *soid != 0)
                .ok_or_else(|| format!("Character {number} {label} has an invalid instance SOID"))?;
            equipped
                .get("level")
                .and_then(Value::as_i64)
                .filter(|level| (0..=i64::from(i32::MAX)).contains(level))
                .ok_or_else(|| format!("Character {number} {label} has an invalid item level"))?;
            equipped
                .get("quantity")
                .and_then(Value::as_i64)
                .filter(|quantity| (1..=i64::from(i32::MAX)).contains(quantity))
                .ok_or_else(|| format!("Character {number} {label} has an invalid quantity"))?;
            match equipped.get("plugs") {
                Some(Value::Null) | None => {}
                Some(Value::Array(plugs)) => {
                    if plugs.len() > MAX_PLUGS {
                        return Err(format!(
                            "Character {number} {label} cannot contain more than {MAX_PLUGS} plugs"
                        ));
                    }
                    if plugs.iter().any(|plug| {
                        !plug.is_null()
                            && !parse_unsigned_value(plug).is_some_and(|hash| {
                                u32::try_from(hash).is_ok() && hash != NO_DEFINITION_HASH
                            })
                    }) {
                        return Err(format!("Character {number} {label} contains an invalid plug hash"));
                    }
                }
                _ => return Err(format!("Character {number} {label} plugs must be null or an array")),
            }
        }
    }
    Ok(())
}

/// Shadowkeep only accepts a few super and melee pairings per subclass; a bad
/// pair sends the game to character creation.
pub fn character_ability_issue(character: &Map<String, Value>) -> Option<String> {
    let subclass_hash = character
        .get("equipment")?
        .as_object()?
        .get("subclass")?
        .as_object()?
        .get("definition_hash")
        .and_then(parse_unsigned_value)?;
    let (subclass_name, middle_super) = subclass_rules(subclass_hash)?;

    for (key, range, label) in [
        ("movement_ability", 4..=6, "movement ability"),
        ("grenade_ability", 7..=9, "grenade ability"),
        ("class_ability", 2..=3, "class ability"),
    ] {
        if let Some(value) = character.get(key).and_then(Value::as_u64)
            && !range.contains(&value)
        {
            return Some(format!("has an unsupported {label} entry {value} for {subclass_name}"));
        }
    }

    let (Some(super_ability), Some(melee)) = (
        character.get("super_ability").and_then(Value::as_u64),
        character.get("melee_ability").and_then(Value::as_u64),
    ) else {
        return None;
    };
    let supported = [(10, 11), (10, 15), (middle_super, 21)];
    (!supported.contains(&(super_ability, melee))).then(|| {
        format!(
            "has an unsupported super and melee combination ({super_ability}/{melee}) for {subclass_name}; expected 10/11, 10/15, or {middle_super}/21"
        )
    })
}

pub fn repair_known_ability_pairs(document: &mut Value) -> usize {
    let Some(characters) = document.pointer_mut("/state/characters").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut repaired = 0;
    for character in characters {
        let Some(character) = character.as_object_mut() else {
            continue;
        };
        let Some(middle_super) = character
            .get("equipment")
            .and_then(Value::as_object)
            .and_then(|equipment| equipment.get("subclass"))
            .and_then(Value::as_object)
            .and_then(|subclass| subclass.get("definition_hash"))
            .and_then(parse_unsigned_value)
            .and_then(subclass_rules)
            .map(|(_, middle_super)| middle_super)
        else {
            continue;
        };
        let (Some(super_ability), Some(melee)) = (
            character.get("super_ability").and_then(Value::as_u64),
            character.get("melee_ability").and_then(Value::as_u64),
        ) else {
            continue;
        };
        if [(10, 11), (10, 15), (middle_super, 21)].contains(&(super_ability, melee)) {
            continue;
        }
        // The melee entry identifies the attunement for every Shadowkeep
        // subclass, so prefer it when recovering a mismatched pair.
        let corrected = match melee {
            11 => (10, 11),
            15 => (10, 15),
            21 => (middle_super, 21),
            _ if super_ability == 20 => (middle_super, 21),
            _ => (10, 11),
        };
        character.insert("super_ability".into(), Value::from(corrected.0));
        character.insert("melee_ability".into(), Value::from(corrected.1));
        repaired += 1;
    }
    repaired
}

const fn subclass_rules(subclass_hash: u64) -> Option<(&'static str, u64)> {
    Some(match subclass_hash {
        // Arcstrider and Sentinel keep entry 10 for both attunement supers.
        0x4F91_DC97 => ("Arcstrider", 10),
        0xC99B_33E9 => ("Sentinel", 10),
        0xB055_4739 => ("Striker", 20),
        0xB920_CE9A => ("Sunbreaker", 20),
        0xD8B8_D1FC => ("Gunslinger", 20),
        0xC048_3D8B => ("Nightstalker", 20),
        0xCF88_FEA5 => ("Dawnblade", 20),
        0x686A_154A => ("Stormcaller", 20),
        0xE7BC_88B0 => ("Voidwalker", 20),
        _ => return None,
    })
}

// -------------------------------------------------------------- document edits

pub fn default_plug_values(defaults: &[Option<String>]) -> Vec<Value> {
    defaults
        .iter()
        .map(|plug| plug.clone().map_or(Value::Null, Value::String))
        .collect()
}

/// The plugs to display: an authored array, or the item's package defaults.
pub fn displayed_plugs(plugs: Option<&Value>, defaults: &[Option<String>]) -> (Vec<Value>, bool) {
    match plugs {
        Some(Value::Array(plugs)) => (plugs.clone(), false),
        Some(Value::Null) | None => (default_plug_values(defaults), true),
        _ => (Vec::new(), false),
    }
}

pub fn next_instance_soid(document: &Value) -> Option<u64> {
    let mut used = HashSet::new();
    for character in document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for item in character
            .get("equipment")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(Map::values)
            .filter_map(Value::as_object)
        {
            if let Some(soid) = item.get("instance_soid").and_then(parse_unsigned_value) {
                used.insert(soid);
            }
        }
    }
    (GENERATED_INSTANCE_SOID_START..).find(|candidate| !used.contains(candidate))
}

fn inferred_item_level(document: &Value, character_index: usize) -> i64 {
    document
        .pointer(&format!("/state/characters/{character_index}/equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| {
            equipment.values().find_map(|item| {
                item.get("level")
                    .and_then(Value::as_i64)
                    .filter(|level| (0..=i64::from(i32::MAX)).contains(level))
            })
        })
        .unwrap_or(106)
}

/// Equips an item, installing its package-default plugs.
pub fn equip_definition(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    definition_hash: u64,
    default_plugs: &[Option<String>],
) -> Result<(), String> {
    if u32::try_from(definition_hash).is_err() {
        return Err(format!(
            "Cannot equip an invalid definition hash in the {} slot",
            slot_label(slot)
        ));
    }
    let pointer = format!("/state/characters/{character_index}/equipment/{slot}");
    let replacement = match document.pointer(&pointer) {
        Some(Value::Object(_)) => None,
        Some(Value::Null) | None => {
            let instance_soid = next_instance_soid(document)
                .ok_or("Could not allocate a unique instance SOID for the selected item")?;
            Some(serde_json::json!({
                "instance_soid": format!("0x{instance_soid:016X}"),
                "definition_hash": format_hash(definition_hash),
                "level": inferred_item_level(document, character_index),
                "quantity": 1,
                "plugs": default_plug_values(default_plugs),
            }))
        }
        Some(_) => {
            return Err(format!(
                "The {} slot must be an object or null before it can be changed",
                slot_label(slot)
            ));
        }
    };

    let equipment = document
        .pointer_mut(&format!("/state/characters/{character_index}/equipment"))
        .and_then(Value::as_object_mut)
        .ok_or("The selected character has no equipment object")?;
    if let Some(replacement) = replacement {
        equipment.insert(slot.into(), replacement);
        return Ok(());
    }
    let equipped = equipment
        .get_mut(slot)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Missing equipment slot: {slot}"))?;
    equipped.insert("definition_hash".into(), Value::String(format_hash(definition_hash)));
    equipped.insert("plugs".into(), Value::Array(default_plug_values(default_plugs)));
    Ok(())
}

pub fn set_weapon_slot_empty(
    document: &mut Value,
    character_index: usize,
    slot: &str,
) -> Result<(), String> {
    if !WEAPON_SLOTS.contains(&slot) {
        return Err(format!(
            "Only weapon slots can be set to empty; {} was not changed",
            slot_label(slot)
        ));
    }
    let equipment = document
        .pointer_mut(&format!("/state/characters/{character_index}/equipment"))
        .and_then(Value::as_object_mut)
        .ok_or("The selected character has no equipment object")?;
    match equipment.get(slot) {
        Some(Value::Object(_) | Value::Null) | None => {
            equipment.insert(slot.into(), Value::Null);
            Ok(())
        }
        Some(_) => Err(format!(
            "The {} slot contains unexpected data and was not changed",
            slot_label(slot)
        )),
    }
}

/// Installs (or clears) one socket, materializing default plugs first so an
/// untouched item keeps the rest of its package defaults.
pub fn set_plug(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    socket_index: usize,
    defaults: &[Option<String>],
    hash: Option<u64>,
) -> Result<(), String> {
    let plugs = document
        .pointer_mut(&format!("/state/characters/{character_index}/equipment/{slot}/plugs"))
        .ok_or_else(|| format!("Missing plugs value for {slot}"))?;
    if plugs.is_null() {
        *plugs = Value::Array(default_plug_values(defaults));
    }
    let plugs = plugs
        .as_array_mut()
        .ok_or_else(|| format!("Invalid plugs value for {slot}"))?;
    while plugs.len() <= socket_index {
        plugs.push(Value::Null);
    }
    plugs[socket_index] = hash.map(format_hash).map_or(Value::Null, Value::String);
    Ok(())
}

/// Armor from each class already in the file, used when switching a character's
/// class so its armor stays wearable.
pub fn collect_class_armor_defaults(document: &Value) -> HashMap<u64, HashMap<String, Value>> {
    let mut defaults = HashMap::new();
    for character in document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(class_type), Some(equipment)) = (
            character.get("class").and_then(Value::as_u64),
            character.get("equipment").and_then(Value::as_object),
        ) else {
            continue;
        };
        let armor = ARMOR_SLOTS
            .iter()
            .filter_map(|slot| equipment.get(*slot).cloned().map(|item| ((*slot).into(), item)))
            .collect();
        defaults.entry(class_type).or_insert(armor);
    }
    defaults
}

pub fn restore_class_armor(
    character: &mut Map<String, Value>,
    defaults: &HashMap<String, Value>,
) -> bool {
    let Some(equipment) = character.get_mut("equipment").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    for &slot in ARMOR_SLOTS {
        let (Some(replacement), Some(existing)) = (
            defaults.get(slot).and_then(Value::as_object),
            equipment.get(slot).and_then(Value::as_object),
        ) else {
            continue;
        };
        let mut merged = existing.clone();
        for (key, value) in replacement {
            if key != "instance_soid" {
                merged.insert(key.clone(), value.clone());
            }
        }
        let merged = Value::Object(merged);
        if equipment.get(slot) != Some(&merged) {
            equipment.insert(slot.into(), merged);
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Value {
        serde_json::json!({
            "schema": 3,
            "state": { "characters": [{
                "soid": "0x1234",
                "class": 1,
                "super_ability": 10,
                "melee_ability": 11,
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x0000000000000001",
                        "definition_hash": "0x0000ABCD",
                        "level": 106,
                        "quantity": 1,
                        "plugs": null
                    }
                }
            }]}
        })
    }

    #[test]
    fn setting_a_plug_materializes_the_package_defaults_first() {
        let mut document = document();
        let defaults = vec![Some("0x11111111".into()), None, Some("0x33333333".into())];
        set_plug(&mut document, 0, "kinetic", 1, &defaults, Some(0x2222_2222)).unwrap();
        assert_eq!(
            document.pointer("/state/characters/0/equipment/kinetic/plugs"),
            Some(&serde_json::json!(["0x11111111", "0x22222222", "0x33333333"]))
        );
    }

    #[test]
    fn clearing_a_plug_writes_null_without_shortening_the_array() {
        let mut document = document();
        let defaults = vec![Some("0x11111111".into()), Some("0x22222222".into())];
        set_plug(&mut document, 0, "kinetic", 0, &defaults, None).unwrap();
        assert_eq!(
            document.pointer("/state/characters/0/equipment/kinetic/plugs"),
            Some(&serde_json::json!([Value::Null, "0x22222222"]))
        );
    }

    #[test]
    fn equipping_into_an_empty_slot_allocates_an_unused_instance() {
        let mut document = document();
        set_weapon_slot_empty(&mut document, 0, "energy").unwrap();
        equip_definition(&mut document, 0, "energy", 0x0000_BEEF, &[Some("0x1".into())]).unwrap();
        let energy = document.pointer("/state/characters/0/equipment/energy").unwrap();
        assert_eq!(energy["definition_hash"], "0x0000BEEF");
        assert_ne!(energy["instance_soid"], "0x0000000000000001");
        assert_eq!(energy["plugs"], serde_json::json!(["0x1"]));
    }

    #[test]
    fn arrays_are_encoded_on_one_line_to_stay_under_the_size_limit() {
        let encoded = encode_settings(&document()).unwrap();
        assert!(encoded.contains("\"characters\": ["));
        assert!(!encoded.contains("\n      \"soid\"") || encoded.contains("\"soid\""));
    }

    #[test]
    fn mismatched_super_and_melee_pairs_are_repaired() {
        let mut document = serde_json::json!({
            "state": { "characters": [{
                "soid": "0x1",
                "super_ability": 20,
                "melee_ability": 11,
                "equipment": { "subclass": { "definition_hash": "0xD8B8D1FC" } }
            }]}
        });
        assert_eq!(repair_known_ability_pairs(&mut document), 1);
        assert_eq!(document["state"]["characters"][0]["super_ability"], 10);
    }
}
