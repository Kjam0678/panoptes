//! What Sunrise will accept. Every rule here is one the game enforces on its
//! side, and a file that breaks any of them is refused whole.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::{
    game_settings,
    model::{
        MAX_CHARACTER_INVENTORY, SLOTS, Slot, format_hash, is_definition_hash,
        parse_unsigned_value, pointer,
    },
};

use super::character_items;

// ------------------------------------------------------------------ validation

pub fn validate_document(document: &Value) -> Result<(), String> {
    game_settings::validate(document)?;
    validate_characters(document)
}

pub fn validate_characters(document: &Value) -> Result<(), String> {
    const MAX_CHARACTERS: usize = 3;

    let Some(characters) = document.pointer(pointer::CHARACTERS) else {
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
            if Slot::from_name(slot).is_none() {
                return Err(format!("Character {number} has an unknown equipment slot: {slot}"));
            }
        }
        for slot in SLOTS {
            let Some(equipped) = equipment.get(slot.name).filter(|value| !value.is_null()) else {
                continue;
            };
            validate_item(equipped, &format!("Character {number} {}", slot.label))?;
        }

        let Some(held) = character.get("inventory") else {
            continue;
        };
        let held = held
            .as_array()
            .ok_or_else(|| format!("Character {number} inventory must be an array"))?;
        if held.len() > MAX_CHARACTER_INVENTORY {
            return Err(format!(
                "Character {number} cannot hold more than {MAX_CHARACTER_INVENTORY} unequipped items"
            ));
        }
        for (position, item) in held.iter().enumerate() {
            validate_item(item, &format!("Character {number} inventory item {}", position + 1))?;
        }

        // One instance is one item, so the same SOID cannot name two of them.
        let mut instances = HashSet::new();
        for item in character_items(character) {
            let Some(soid) = item.get("instance_soid").and_then(parse_unsigned_value) else {
                continue;
            };
            if !instances.insert(soid) {
                return Err(format!(
                    "Character {number} uses instance SOID {} for two items",
                    format_hash(soid)
                ));
            }
        }
    }
    Ok(())
}

/// One equipped or held item. Sunrise reads the same shape either way, and
/// rejects the whole file over any part of it.
fn validate_item(item: &Value, whose: &str) -> Result<(), String> {
    const MAX_PLUGS: usize = 12;
    /// Locked and tracked, the only item-state bits this build accepts.
    const ITEM_FLAGS: u64 = 0x3;

    let item = item.as_object().ok_or_else(|| format!("{whose} must be an object or null"))?;
    item.get("definition_hash")
        .and_then(parse_unsigned_value)
        .filter(|hash| is_definition_hash(*hash))
        .ok_or_else(|| format!("{whose} has an invalid definition hash"))?;
    item.get("instance_soid")
        .and_then(parse_unsigned_value)
        .filter(|soid| *soid != 0)
        .ok_or_else(|| format!("{whose} has an invalid instance SOID"))?;
    item.get("level")
        .and_then(Value::as_i64)
        .filter(|level| (0..=i64::from(i32::MAX)).contains(level))
        .ok_or_else(|| format!("{whose} has an invalid item level"))?;
    item.get("quantity")
        .and_then(Value::as_i64)
        .filter(|quantity| (1..=i64::from(i32::MAX)).contains(quantity))
        .ok_or_else(|| format!("{whose} has an invalid quantity"))?;
    if let Some(flags) = item.get("flags") {
        flags
            .as_u64()
            .filter(|flags| *flags <= ITEM_FLAGS)
            .ok_or_else(|| format!("{whose} has item flags this build does not accept"))?;
    }
    match item.get("plugs") {
        Some(Value::Null) | None => {}
        Some(Value::Array(plugs)) => {
            if plugs.len() > MAX_PLUGS {
                return Err(format!("{whose} cannot contain more than {MAX_PLUGS} plugs"));
            }
            if plugs.iter().any(|plug| {
                !plug.is_null() && !parse_unsigned_value(plug).is_some_and(is_definition_hash)
            }) {
                return Err(format!("{whose} contains an invalid plug hash"));
            }
        }
        _ => return Err(format!("{whose} plugs must be null or an array")),
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
    let pairs = supported_ability_pairs(middle_super);

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
    (!pairs.contains(&(super_ability, melee))).then(|| {
        let [first, second, third] = pairs.map(|(accepted, with)| format!("{accepted}/{with}"));
        format!(
            "has an unsupported super and melee combination ({super_ability}/{melee}) for {subclass_name}; expected {first}, {second}, or {third}"
        )
    })
}

/// The super and melee pairings Shadowkeep accepts for a subclass. Anything
/// else sends the game to character creation, so this is both what the warning
/// checks and what the repair puts back.
const fn supported_ability_pairs(middle_super: u64) -> [(u64, u64); 3] {
    [(10, 11), (10, 15), (middle_super, 21)]
}

pub fn repair_known_ability_pairs(document: &mut Value) -> usize {
    let Some(characters) = document.pointer_mut(pointer::CHARACTERS).and_then(Value::as_array_mut) else {
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
        if supported_ability_pairs(middle_super).contains(&(super_ability, melee)) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
