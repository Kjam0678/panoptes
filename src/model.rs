//! Shared slot tables and hash helpers.

use serde_json::Value;

/// Equipment slots as Project Sunrise names them, with their display label and
/// Destiny inventory bucket hash.
pub const SLOTS: &[(&str, &str, u64)] = &[
    ("kinetic", "Kinetic", 1_498_876_634),
    ("energy", "Energy", 2_465_295_065),
    ("heavy", "Power", 953_998_645),
    ("helmet", "Helmet", 3_448_274_439),
    ("gauntlets", "Gauntlets", 3_551_918_588),
    ("chest", "Chest", 14_239_492),
    ("legs", "Legs", 20_886_954),
    ("class_item", "Class item", 1_585_787_867),
    ("ghost", "Ghost", 4_023_194_814),
    ("vehicle", "Vehicle", 2_025_709_351),
    ("ship", "Ship", 284_967_655),
    ("subclass", "Subclass", 3_284_755_031),
    ("clan_banner", "Clan banner", 4_292_445_962),
    ("emblem", "Emblem", 4_274_335_291),
    ("emote", "Emote", 2_401_704_334),
    ("finisher", "Finisher", 3_683_254_069),
];

pub const WEAPON_SLOTS: &[&str] = &["kinetic", "energy", "heavy"];
pub const ARMOR_SLOTS: &[&str] = &["helmet", "gauntlets", "chest", "legs", "class_item"];
pub const SUBCLASS_BUCKET: u64 = 3_284_755_031;

/// Instance identifiers this editor allocates start well above Sunrise's own.
pub const GENERATED_INSTANCE_SOID_START: u64 = 0x4000_0000_0000_0001;
pub const MAX_SETTINGS_BYTES: usize = 64 * 1024 - 1;

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

pub fn slot_label(slot: &str) -> &str {
    SLOTS
        .iter()
        .find_map(|(name, label, _)| (*name == slot).then_some(*label))
        .unwrap_or(slot)
}

pub fn slot_bucket(slot: &str) -> Option<u64> {
    SLOTS
        .iter()
        .find_map(|(name, _, bucket)| (*name == slot).then_some(*bucket))
}

pub const fn class_name(class_type: u64) -> &'static str {
    match class_type {
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        _ => "Invalid class",
    }
}
