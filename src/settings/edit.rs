//! Changing what a character has: equipping, holding, swapping, and installing
//! plugs. Every edit leaves the document in a shape [`super::validate`] accepts,
//! or refuses and changes nothing.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::model::{
    GENERATED_INSTANCE_SOID_START, MAX_CHARACTER_INVENTORY, SLOTS, Slot, format_hash,
    is_definition_hash, parse_unsigned_value, pointer,
};

use super::{character_items, characters};

// -------------------------------------------------------------- document edits

fn default_plug_values(defaults: &[Option<String>]) -> Vec<Value> {
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

fn next_instance_soid(document: &Value) -> u64 {
    let mut used = HashSet::new();
    for character in characters(document).filter_map(Value::as_object) {
        for item in character_items(character) {
            if let Some(soid) = item.get("instance_soid").and_then(parse_unsigned_value) {
                used.insert(soid);
            }
        }
    }
    // Sunrise's own instances sit far below this one, and a character may hold
    // 135 items, so a free identifier is always a few steps away at most.
    (GENERATED_INSTANCE_SOID_START..)
        .find(|candidate| !used.contains(candidate))
        .expect("the generated instance range cannot be exhausted")
}

pub fn inferred_item_level(document: &Value, character_index: usize) -> i64 {
    document
        .pointer(&pointer::equipment(character_index))
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

/// Everything an item carries except the instance that says which one it is,
/// in the order Sunrise writes them.
fn item_fields(definition_hash: u64, defaults: &[Option<String>], level: i64) -> Map<String, Value> {
    let mut item = Map::new();
    point_at_definition(&mut item, definition_hash, defaults);
    item.insert("level".into(), Value::from(level));
    item.insert("quantity".into(), Value::from(1));
    item
}

/// A whole item, instance and all.
fn new_item(
    instance_soid: u64,
    definition_hash: u64,
    defaults: &[Option<String>],
    level: i64,
) -> Value {
    let mut item = Map::new();
    item.insert(
        "instance_soid".into(),
        Value::String(format!("0x{instance_soid:016X}")),
    );
    item.append(&mut item_fields(definition_hash, defaults, level));
    Value::Object(item)
}

/// Points an existing item at a definition, with that definition's package
/// defaults in its sockets. Whatever else the item carries is left alone.
fn point_at_definition(
    item: &mut Map<String, Value>,
    definition_hash: u64,
    defaults: &[Option<String>],
) {
    item.insert(
        "definition_hash".into(),
        Value::String(format_hash(definition_hash)),
    );
    item.insert("plugs".into(), Value::Array(default_plug_values(defaults)));
}

/// Equips an item, installing its package-default plugs.
pub fn equip_definition(
    document: &mut Value,
    character_index: usize,
    slot: &Slot,
    definition_hash: u64,
    default_plugs: &[Option<String>],
) -> Result<(), String> {
    if !is_definition_hash(definition_hash) {
        return Err(format!(
            "Cannot equip an invalid definition hash in the {} slot",
            slot.label
        ));
    }
    let equipped_at = pointer::equipped(character_index, slot);
    let replacement = match document.pointer(&equipped_at) {
        Some(Value::Object(_)) => None,
        Some(Value::Null) | None => {
            let instance_soid = next_instance_soid(document);
            let level = inferred_item_level(document, character_index);
            Some(new_item(instance_soid, definition_hash, default_plugs, level))
        }
        Some(_) => {
            return Err(format!(
                "The {} slot must be an object or null before it can be changed",
                slot.label
            ));
        }
    };

    let equipment = equipment_mut(document, character_index)?;
    if let Some(replacement) = replacement {
        equipment.insert(slot.name.into(), replacement);
        return Ok(());
    }
    let equipped = equipment
        .get_mut(slot.name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Missing equipment slot: {}", slot.name))?;
    point_at_definition(equipped, definition_hash, default_plugs);
    Ok(())
}

fn equipment_mut(
    document: &mut Value,
    character_index: usize,
) -> Result<&mut Map<String, Value>, String> {
    document
        .pointer_mut(&pointer::equipment(character_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "The selected character has no equipment object".to_owned())
}

/// The items a character holds unequipped. Sunrise places each one by the
/// bucket its definition names, so the array is a pool rather than a grid.
pub fn character_inventory(document: &Value, character_index: usize) -> &[Value] {
    document
        .pointer(&pointer::inventory(character_index))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn character_inventory_mut(
    document: &mut Value,
    character_index: usize,
) -> Result<&mut Vec<Value>, String> {
    let character = document
        .pointer_mut(&pointer::character(character_index))
        .and_then(Value::as_object_mut)
        .ok_or("The selected character is not in this file")?;
    let held = character.entry("inventory").or_insert_with(|| Value::Array(Vec::new()));
    if held.is_null() {
        *held = Value::Array(Vec::new());
    }
    held.as_array_mut().ok_or_else(|| "That character's inventory is not an array".to_owned())
}

/// Adds one item to what a character holds, with the package defaults in its
/// sockets, and says where it landed.
pub fn hold_definition(
    document: &mut Value,
    character_index: usize,
    definition_hash: u64,
    defaults: &[Option<String>],
) -> Result<usize, String> {
    if !is_definition_hash(definition_hash) {
        return Err("Cannot hold an invalid definition hash".to_owned());
    }
    let level = inferred_item_level(document, character_index);
    let instance = next_instance_soid(document);
    let item = new_item(instance, definition_hash, defaults, level);

    let held = character_inventory_mut(document, character_index)?;
    if held.len() >= MAX_CHARACTER_INVENTORY {
        return Err(format!(
            "This character already holds {MAX_CHARACTER_INVENTORY} items, which is all Sunrise reserves room for"
        ));
    }
    held.push(item);
    Ok(held.len() - 1)
}

/// Empties what a character holds, so a fresh roll replaces the lot rather
/// than piling onto it.
pub fn clear_inventory(document: &mut Value, character_index: usize) -> Result<(), String> {
    character_inventory_mut(document, character_index)?.clear();
    Ok(())
}

/// Takes one held item out, leaving the rest in order.
pub fn take_held_item(
    document: &mut Value,
    character_index: usize,
    held_index: usize,
) -> Result<Value, String> {
    let held = character_inventory_mut(document, character_index)?;
    if held_index >= held.len() {
        return Err("That item is no longer there".to_owned());
    }
    Ok(held.remove(held_index))
}

/// Swaps a held item with what a slot has equipped. Each item keeps its own
/// instance, since an instance is the item rather than the place it sits.
/// Holding the equipped item without one to replace it empties the slot, which
/// only a weapon slot may be.
pub fn swap_equipped(
    document: &mut Value,
    character_index: usize,
    slot: &Slot,
    held_index: Option<usize>,
) -> Result<(), String> {
    let equipped_at = pointer::equipped(character_index, slot);
    let outgoing = match document.pointer(&equipped_at) {
        Some(Value::Object(_)) => document.pointer(&equipped_at).cloned(),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(format!(
                "The {} slot contains unexpected data and was not changed",
                slot.label
            ));
        }
    };
    // Both halves are checked before either moves, so a refusal leaves the
    // file exactly as it was.
    if held_index.is_none() && !slot.is_weapon() {
        return Err(format!(
            "Only weapon slots can be left empty; {} was not changed",
            slot.label
        ));
    }
    let room = character_inventory(document, character_index).len()
        + usize::from(outgoing.is_some())
        - usize::from(held_index.is_some());
    if room > MAX_CHARACTER_INVENTORY {
        return Err(format!(
            "This character already holds {MAX_CHARACTER_INVENTORY} items, which is all Sunrise reserves room for"
        ));
    }

    let incoming = match held_index {
        Some(held_index) => Some(take_held_item(document, character_index, held_index)?),
        None => None,
    };
    if let Some(outgoing) = outgoing {
        let held = character_inventory_mut(document, character_index)?;
        // Back into the box it came out of, so the row does not reshuffle.
        held.insert(held_index.unwrap_or(held.len()).min(held.len()), outgoing);
    }

    let equipment = equipment_mut(document, character_index)?;
    equipment.insert(slot.name.into(), incoming.unwrap_or(Value::Null));
    Ok(())
}

pub fn set_weapon_slot_empty(
    document: &mut Value,
    character_index: usize,
    slot: &Slot,
) -> Result<(), String> {
    if !slot.is_weapon() {
        return Err(format!(
            "Only weapon slots can be set to empty; {} was not changed",
            slot.label
        ));
    }
    let equipment = equipment_mut(document, character_index)?;
    match equipment.get(slot.name) {
        Some(Value::Object(_) | Value::Null) | None => {
            equipment.insert(slot.name.into(), Value::Null);
            Ok(())
        }
        Some(_) => Err(format!(
            "The {} slot contains unexpected data and was not changed",
            slot.label
        )),
    }
}

/// Installs (or clears) one socket, materializing default plugs first so an
/// untouched item keeps the rest of its package defaults. It works on a single
/// equipment entry rather than on a slot of the document, because an item
/// waiting in an inventory box is edited the same way as an equipped one.
pub fn set_item_plug(
    item: &mut Value,
    socket_index: usize,
    defaults: &[Option<String>],
    hash: Option<u64>,
) -> Result<(), String> {
    let object = item.as_object_mut().ok_or("That item has no plugs to change")?;
    let plugs = object.entry("plugs").or_insert(Value::Null);
    if plugs.is_null() {
        *plugs = Value::Array(default_plug_values(defaults));
    }
    let plugs = plugs.as_array_mut().ok_or("That item's plugs are not a list")?;
    while plugs.len() <= socket_index {
        plugs.push(Value::Null);
    }
    plugs[socket_index] = hash.map(format_hash).map_or(Value::Null, Value::String);
    Ok(())
}

/// Points an equipment entry at a definition, with that definition's package
/// defaults in its sockets. A null entry becomes a whole item at `level`; it
/// gets an instance SOID from the slot it is later swapped into.
pub fn set_item_definition(
    item: &mut Value,
    definition_hash: u64,
    defaults: &[Option<String>],
    level: i64,
) -> Result<(), String> {
    if !is_definition_hash(definition_hash) {
        return Err("Cannot hold an invalid definition hash".to_owned());
    }
    match item {
        Value::Object(object) => {
            point_at_definition(object, definition_hash, defaults);
            Ok(())
        }
        Value::Null => {
            *item = Value::Object(item_fields(definition_hash, defaults, level));
            Ok(())
        }
        _ => Err("That item is not an object and was not changed".to_owned()),
    }
}

/// Armor from each class already in the file, used when switching a character's
/// class so its armor stays wearable.
pub fn collect_class_armor_defaults(document: &Value) -> HashMap<u64, HashMap<String, Value>> {
    let mut defaults = HashMap::new();
    for character in characters(document) {
        let (Some(class_type), Some(equipment)) = (
            character.get("class").and_then(Value::as_u64),
            character.get("equipment").and_then(Value::as_object),
        ) else {
            continue;
        };
        let armor = armor_slots()
            .filter_map(|slot| {
                equipment
                    .get(slot.name)
                    .cloned()
                    .map(|item| (slot.name.to_owned(), item))
            })
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
    for slot in armor_slots() {
        let (Some(replacement), Some(existing)) = (
            defaults.get(slot.name).and_then(Value::as_object),
            equipment.get(slot.name).and_then(Value::as_object),
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
        if equipment.get(slot.name) != Some(&merged) {
            equipment.insert(slot.name.into(), merged);
            changed = true;
        }
    }
    changed
}

fn armor_slots() -> impl Iterator<Item = &'static Slot> {
    SLOTS.iter().filter(|slot| slot.is_armor())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::validate_characters;

    fn slot(name: &str) -> &'static Slot {
        Slot::from_name(name).expect("the tests only name real slots")
    }

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
    fn an_item_can_be_built_and_edited_before_it_is_ever_equipped() {
        let defaults = vec![Some("0x11111111".into()), Some("0x22222222".into())];
        let mut document = document();
        let held = hold_definition(&mut document, 0, 0x0000_BEEF, &defaults).unwrap();
        let item = document.pointer_mut(&format!("/state/characters/0/inventory/{held}")).unwrap();
        set_item_plug(item, 1, &defaults, Some(0x3333_3333)).unwrap();

        let item = &character_inventory(&document, 0)[held];
        assert_eq!(item["definition_hash"], "0x0000BEEF");
        assert_eq!(item["plugs"], serde_json::json!(["0x11111111", "0x33333333"]));
        // A held item is a real instance, and cannot share one with the
        // kinetic weapon already in the file.
        assert_ne!(item["instance_soid"], "0x0000000000000001");
        assert!(validate_characters(&document).is_ok());
    }

    #[test]
    fn equipping_a_held_item_swaps_the_two_places_it_can_sit() {
        let mut document = document();
        let defaults = vec![Some("0x11111111".into())];
        hold_definition(&mut document, 0, 0x0000_BEEF, &defaults).unwrap();

        swap_equipped(&mut document, 0, slot("kinetic"), Some(0)).unwrap();
        let equipped = document.pointer("/state/characters/0/equipment/kinetic").unwrap();
        assert_eq!(equipped["definition_hash"], "0x0000BEEF");
        // Each item keeps the instance it was created with.
        assert_eq!(equipped["instance_soid"], "0x4000000000000001");
        let held = character_inventory(&document, 0);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0]["definition_hash"], "0x0000ABCD");
        assert_eq!(held[0]["instance_soid"], "0x0000000000000001");
        assert!(validate_characters(&document).is_ok());

        // Swapping back returns the file to where it started.
        swap_equipped(&mut document, 0, slot("kinetic"), Some(0)).unwrap();
        let equipped = document.pointer("/state/characters/0/equipment/kinetic").unwrap();
        assert_eq!(equipped["definition_hash"], "0x0000ABCD");
        assert_eq!(character_inventory(&document, 0)[0]["definition_hash"], "0x0000BEEF");
    }

    #[test]
    fn only_a_weapon_slot_can_be_emptied_by_putting_its_item_away() {
        let mut document = document();
        document["state"]["characters"][0]["equipment"]["ghost"] = serde_json::json!({
            "instance_soid": "0x0000000000000002",
            "definition_hash": "0x0000FEED",
            "level": 106,
            "quantity": 1,
            "plugs": null
        });
        let before = document.clone();

        assert!(swap_equipped(&mut document, 0, slot("ghost"), None).is_err());
        assert_eq!(document, before, "a refused swap must not touch the file");

        swap_equipped(&mut document, 0, slot("kinetic"), None).unwrap();
        assert!(document.pointer("/state/characters/0/equipment/kinetic").unwrap().is_null());
        assert_eq!(character_inventory(&document, 0)[0]["definition_hash"], "0x0000ABCD");
    }

    #[test]
    fn a_character_cannot_hold_more_than_sunrise_reserves_room_for() {
        let mut document = document();
        for _ in 0..MAX_CHARACTER_INVENTORY {
            hold_definition(&mut document, 0, 0x0000_BEEF, &[]).unwrap();
        }
        assert_eq!(character_inventory(&document, 0).len(), MAX_CHARACTER_INVENTORY);
        assert!(hold_definition(&mut document, 0, 0x0000_BEEF, &[]).is_err());
        // Nor by putting an equipped item away, which would make one more.
        assert!(swap_equipped(&mut document, 0, slot("kinetic"), None).is_err());
        assert!(validate_characters(&document).is_ok());
    }

    #[test]
    fn setting_a_plug_materializes_the_package_defaults_first() {
        let mut document = document();
        let defaults = vec![Some("0x11111111".into()), None, Some("0x33333333".into())];
        let item = document.pointer_mut("/state/characters/0/equipment/kinetic").unwrap();
        set_item_plug(item, 1, &defaults, Some(0x2222_2222)).unwrap();
        assert_eq!(
            document.pointer("/state/characters/0/equipment/kinetic/plugs"),
            Some(&serde_json::json!(["0x11111111", "0x22222222", "0x33333333"]))
        );
    }

    #[test]
    fn clearing_a_plug_writes_null_without_shortening_the_array() {
        let mut document = document();
        let defaults = vec![Some("0x11111111".into()), Some("0x22222222".into())];
        let item = document.pointer_mut("/state/characters/0/equipment/kinetic").unwrap();
        set_item_plug(item, 0, &defaults, None).unwrap();
        assert_eq!(
            document.pointer("/state/characters/0/equipment/kinetic/plugs"),
            Some(&serde_json::json!([Value::Null, "0x22222222"]))
        );
    }

    #[test]
    fn equipping_into_an_empty_slot_allocates_an_unused_instance() {
        let mut document = document();
        set_weapon_slot_empty(&mut document, 0, slot("energy")).unwrap();
        equip_definition(&mut document, 0, slot("energy"), 0x0000_BEEF, &[Some("0x1".into())]).unwrap();
        let energy = document.pointer("/state/characters/0/equipment/energy").unwrap();
        assert_eq!(energy["definition_hash"], "0x0000BEEF");
        assert_ne!(energy["instance_soid"], "0x0000000000000001");
        assert_eq!(energy["plugs"], serde_json::json!(["0x1"]));
    }
}
