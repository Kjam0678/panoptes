//! The vocabulary every other module shares: equipment slots, the small
//! enumerations Sunrise stores as integers, and the hash helpers.

use serde_json::Value;

/// What a slot holds, which is what decides how the rest of the app treats it.
/// Only a weapon slot may be left empty, only armor is kept wearable across a
/// class change, and only those two carry a power level the game reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotKind {
    Weapon,
    Armor,
    /// Ghosts, ships, emblems, the subclass — everything else.
    Gear,
}

/// One equipment slot as Project Sunrise names it. Every rule that varies by
/// slot lives in this table, so a slot is looked up once and then asked.
#[derive(PartialEq, Eq, Debug)]
pub struct Slot {
    /// The key this slot has in a character's `equipment` object.
    pub name: &'static str,
    pub label: &'static str,
    /// The Destiny inventory bucket this slot's items belong to.
    pub bucket: u64,
    pub kind: SlotKind,
    /// Whether the slot keeps unequipped items beside what it has equipped.
    /// The game takes one clan banner and no more, so that row is the editor
    /// alone.
    pub holds_inventory: bool,
}

impl Slot {
    pub fn from_name(name: &str) -> Option<&'static Self> {
        SLOTS.iter().find(|slot| slot.name == name)
    }

    pub fn from_bucket(bucket: u64) -> Option<&'static Self> {
        SLOTS.iter().find(|slot| slot.bucket == bucket)
    }

    pub const fn is_weapon(&self) -> bool {
        matches!(self.kind, SlotKind::Weapon)
    }

    pub const fn is_armor(&self) -> bool {
        matches!(self.kind, SlotKind::Armor)
    }

    /// The subclass is equipment in the file but not gear on the page: it is
    /// edited with the character's abilities instead of on a loadout row.
    pub const fn is_subclass(&self) -> bool {
        self.bucket == SUBCLASS_BUCKET
    }
}

macro_rules! slot {
    ($name:literal, $label:literal, $bucket:literal, $kind:ident, $holds:literal) => {
        Slot {
            name: $name,
            label: $label,
            bucket: $bucket,
            kind: SlotKind::$kind,
            holds_inventory: $holds,
        }
    };
}

/// Every slot, in the order the loadout page draws them.
pub const SLOTS: &[Slot] = &[
    slot!("kinetic", "Kinetic", 1_498_876_634, Weapon, true),
    slot!("energy", "Energy", 2_465_295_065, Weapon, true),
    slot!("heavy", "Power", 953_998_645, Weapon, true),
    slot!("helmet", "Helmet", 3_448_274_439, Armor, true),
    slot!("gauntlets", "Gauntlets", 3_551_918_588, Armor, true),
    slot!("chest", "Chest", 14_239_492, Armor, true),
    slot!("legs", "Legs", 20_886_954, Armor, true),
    slot!("class_item", "Class item", 1_585_787_867, Armor, true),
    slot!("ghost", "Ghost", 4_023_194_814, Gear, true),
    slot!("vehicle", "Vehicle", 2_025_709_351, Gear, true),
    slot!("ship", "Ship", 284_967_655, Gear, true),
    slot!("subclass", "Subclass", 3_284_755_031, Gear, true),
    slot!("clan_banner", "Clan banner", 4_292_445_962, Gear, false),
    slot!("emblem", "Emblem", 4_274_335_291, Gear, true),
    slot!("emote", "Emote", 2_401_704_334, Gear, true),
    slot!("finisher", "Finisher", 3_683_254_069, Gear, true),
];

pub const SUBCLASS_BUCKET: u64 = 3_284_755_031;

/// The subclass slot, which the character editor equips directly rather than
/// through a loadout row.
pub fn subclass_slot() -> &'static Slot {
    Slot::from_bucket(SUBCLASS_BUCKET).expect("the slot table always holds the subclass")
}

/// The enumerations Sunrise stores as small integers on a character.
pub const CLASSES: &[(u64, &str)] = &[(0, "Titan"), (1, "Hunter"), (2, "Warlock")];
pub const RACES: &[(u64, &str)] = &[(0, "Human"), (1, "Awoken"), (2, "Exo")];
pub const GENDERS: &[(u64, &str)] = &[(0, "Male"), (1, "Female")];

/// Instance identifiers this editor allocates start well above Sunrise's own.
pub const GENERATED_INSTANCE_SOID_START: u64 = 0x4000_0000_0000_0001;
/// What Sunrise reads into fixed storage; a longer file is refused outright.
pub const MAX_SETTINGS_BYTES: usize = 1024 * 1024;
/// Unequipped items one character can hold. Sunrise's 16 equipment buckets
/// reserve 151 native rows, one of which each slot spends on what it equips.
pub const MAX_CHARACTER_INVENTORY: usize = 135;

/// JSON pointers into settings.json, so the shape of Sunrise's file is
/// described here rather than in a `format!` at each place that reaches into
/// it.
pub mod pointer {
    use super::Slot;

    pub const CHARACTERS: &str = "/state/characters";
    pub const ACCOUNT_SETTINGS: &str = "/state/account/settings";
    pub const PERSONA_NAME: &str = "/steam/user/persona_name";

    pub fn character(index: usize) -> String {
        format!("{CHARACTERS}/{index}")
    }

    pub fn equipment(index: usize) -> String {
        format!("{CHARACTERS}/{index}/equipment")
    }

    pub fn equipped(index: usize, slot: &Slot) -> String {
        format!("{CHARACTERS}/{index}/equipment/{}", slot.name)
    }

    pub fn inventory(index: usize) -> String {
        format!("{CHARACTERS}/{index}/inventory")
    }

    /// One of the items a character holds unequipped, by its place in the pool.
    pub fn held(index: usize, held_index: usize) -> String {
        format!("{CHARACTERS}/{index}/inventory/{held_index}")
    }
}

/// The FNV basis Sunrise writes where an item has no definition. A real
/// definition never carries it.
pub const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;

/// Whether a hash can name a definition: they are 32-bit, and the empty
/// sentinel names nothing.
pub fn is_definition_hash(hash: u64) -> bool {
    u32::try_from(hash).is_ok() && hash != NO_DEFINITION_HASH
}

pub fn format_hash(hash: u64) -> String {
    format!("0x{hash:08X}")
}

pub fn parse_hash(text: &str) -> Option<u64> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))?;
    if digits.is_empty() || digits.len() > 16 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

/// Sunrise writes hashes as `"0x…"` strings but accepts numbers, so read both.
pub fn parse_unsigned_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(parse_hash))
}

/// What one of the integer enumerations calls a value.
pub fn name_of(table: &[(u64, &'static str)], value: u64) -> Option<&'static str> {
    table
        .iter()
        .find_map(|(candidate, name)| (*candidate == value).then_some(*name))
}

pub fn class_name(class_type: u64) -> &'static str {
    name_of(CLASSES, class_type).unwrap_or("Invalid class")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_unique_and_reachable_by_either_key() {
        for slot in SLOTS {
            assert_eq!(Slot::from_name(slot.name), Some(slot));
            assert_eq!(Slot::from_bucket(slot.bucket), Some(slot));
        }
        assert_eq!(Slot::from_name("nothing"), None);
        // The clan banner is the one slot the game keeps no inventory for.
        assert_eq!(
            SLOTS.iter().filter(|slot| !slot.holds_inventory).count(),
            1
        );
        assert_eq!(SLOTS.iter().filter(|slot| slot.is_weapon()).count(), 3);
        assert_eq!(SLOTS.iter().filter(|slot| slot.is_armor()).count(), 5);
        assert_eq!(SLOTS.iter().filter(|slot| slot.is_subclass()).count(), 1);
        assert_eq!(subclass_slot().name, "subclass");
    }

    #[test]
    fn only_thirty_two_bit_hashes_that_are_not_the_empty_marker_name_a_definition() {
        assert!(is_definition_hash(0x0000_BEEF));
        assert!(!is_definition_hash(NO_DEFINITION_HASH));
        assert!(!is_definition_hash(u64::from(u32::MAX) + 1));
    }
}
