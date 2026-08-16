//! The Shadowkeep item catalog, loaded from the JSON snapshot built by Sundial
//! from Destiny 2 build 86657.20.08.23. Nothing here reads game files: the
//! snapshot ships with the app, so start-up is a single parse.

use std::collections::HashMap;

use serde::Deserialize;

use crate::{
    dummy_items,
    model::{ARMOR_SLOTS, WEAPON_SLOTS, format_hash, slot_bucket},
};

const CATALOG_JSON: &[u8] = include_bytes!("../assets/catalog.json");
/// In-game description text, pulled from the packages by `prep-descriptions`.
const DESCRIPTIONS_JSON: &[u8] = include_bytes!("../assets/descriptions.json");
/// Armor 2.0 leaves its stat allocation plugs unnamed, so `prep-descriptions`
/// names them after the stats they grant.
const STAT_PLUGS_JSON: &[u8] = include_bytes!("../assets/stat-plugs.json");

#[derive(Clone, Debug, Deserialize)]
pub struct ItemDef {
    pub hash: u64,
    pub name: String,
    pub type_name: String,
    pub bucket_hash: u64,
    pub class_type: u64,
    pub default_plugs: Vec<Option<String>>,
    pub sockets: Vec<SocketDef>,
    #[serde(default)]
    pub abilities: AbilityOptions,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct SocketDef {
    pub socket_type: u16,
    #[serde(default)]
    pub pool: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AbilityOptions {
    pub movement: Vec<AbilityChoice>,
    pub grenade: Vec<AbilityChoice>,
    pub super_ability: Vec<AbilityChoice>,
    pub melee: Vec<AbilityChoice>,
    pub class_ability: Vec<AbilityChoice>,
    #[serde(default)]
    pub attunements: Vec<AttunementChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct AbilityChoice {
    pub entry: u64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AttunementChoice {
    pub name: String,
    pub super_abilities: Vec<AbilityChoice>,
    pub melee: AbilityChoice,
    pub perks: Vec<AbilityChoice>,
}

#[derive(Deserialize)]
struct CatalogFile {
    items: Vec<ItemDef>,
    names: HashMap<u64, String>,
    plug_pools: Vec<Vec<u64>>,
}

/// How wide a net a socket's plug list should cast.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum PlugFilter {
    /// Only the plugs the game lists for this exact socket.
    #[default]
    Compatible,
    /// Every plug seen on any socket of the same socket type.
    SocketType,
    /// Every non-cosmetic plug seen on any socket of this kind of gear.
    GearType,
    /// Every plug in the build, cosmetics included.
    All,
}

impl PlugFilter {
    pub const ALL: [Self; 4] = [
        Self::Compatible,
        Self::SocketType,
        Self::GearType,
        Self::All,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Compatible => "Compatible",
            Self::SocketType => "Socket type",
            Self::GearType => "Gear type",
            Self::All => "All",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Compatible => "Only plugs the game lists for this socket.",
            Self::SocketType => {
                "Every plug of this socket's type, even where it is not listed as allowed."
            }
            Self::GearType => {
                "Every plug used anywhere on this kind of gear: all weapon traits, intrinsics, and perks, or all armor mods and exotic traits. Shaders, ornaments, and trackers stay in their own sockets."
            }
            Self::All => {
                "Every plug in the build, shaders, ornaments, and trackers included. Most of these will not work outside their own socket."
            }
        }
    }

    pub const fn is_unsafe(self) -> bool {
        !matches!(self, Self::Compatible)
    }
}

/// Weapons, armor, and everything else pool their plugs separately.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum GearKind {
    Weapon,
    Armor,
    Other(u64),
}

pub struct Catalog {
    pub items: Vec<ItemDef>,
    pub names: HashMap<u64, String>,
    by_hash: HashMap<u64, usize>,
    plug_pools: Vec<Vec<u64>>,
    by_socket_type: HashMap<u16, Vec<u64>>,
    by_gear_kind: HashMap<GearKind, Vec<u64>>,
    every_plug: Vec<u64>,
    descriptions: HashMap<u64, String>,
    stat_plugs: HashMap<u64, String>,
    mod_plugs: std::collections::HashSet<u64>,
    sources: HashMap<u64, String>,
    cosmetic_pools: std::collections::HashSet<u32>,
}

/// The mod socket introduced with Armor 2.0.
const ARMOR_MOD_SOCKET: u16 = 643;

/// Sockets that hold an archetype rather than a perk in this build: weapon
/// intrinsics (176), armor archetypes and Year-1 exotic traits (14 and 377),
/// and the Armor 2.0 trait socket (677) that reprised copies of an exotic use
/// instead of 377. Frames share names across weapon classes and behave
/// differently on each, so plugs from these sockets say where they come from.
const INTRINSIC_SOCKET_TYPES: [u16; 4] = [176, 14, 377, 677];

/// Mod sockets, whose plugs this build almost entirely ships without art:
/// Armor 2.0's general, slot-specific, and seasonal mods, then the weapon mod
/// sockets — one per weapon family, all carrying Backup Mag and Boss Spec.
const MOD_SOCKET_TYPES: [u16; 26] = [
    643, 644, 645, 646, 647, 648, 649, 724, 734, 735, //
    687, 688, 689, 690, 691, 692, 694, 695, 696, 697, 698, 699, 700, 701, 702, 703,
];

/// Armor's stat allocation sockets: the four Armor 2.0 slots, and the single
/// socket a Year-1 piece uses for the same thing.
const STAT_SOCKET_TYPES: [u16; 5] = [676, 760, 761, 762, 763];

/// Sockets that are cosmetic without shipping a marker plug: a clan banner's
/// staff is chosen the way a shader is.
const COSMETIC_SOCKET_TYPES: [u16; 1] = [746];

/// A pool holding one of these is a cosmetic pool: shaders, ornaments, and
/// trackers each ship a "default" or disabled plug alongside their choices.
/// Everything in such a pool belongs only to its own socket, so it is kept out
/// of the gear-type list — you would not plug a gun skin into a perk slot.
fn is_cosmetic_marker(name: &str) -> bool {
    matches!(name, "Default Shader" | "Default Ornament" | "Tracker Disabled")
        || name.contains("Kill Tracker")
}

impl Catalog {
    /// Parses the bundled snapshot and pre-computes the two wider plug pools.
    pub fn load() -> Result<Self, String> {
        let file: CatalogFile = serde_json::from_slice(CATALOG_JSON)
            .map_err(|error| format!("The bundled catalog is invalid: {error}"))?;
        let CatalogFile {
            mut items,
            names,
            plug_pools,
        } = file;
        items.sort_by_key(|item| item.name.to_lowercase());
        let stat_plugs: HashMap<u64, String> = serde_json::from_slice(STAT_PLUGS_JSON)
            .map_err(|error| format!("The bundled stat plug names are invalid: {error}"))?;

        let mut cosmetic_pools: std::collections::HashSet<u32> = plug_pools
            .iter()
            .enumerate()
            .filter(|(_, pool)| {
                pool.iter()
                    .any(|hash| names.get(hash).is_some_and(|name| is_cosmetic_marker(name)))
            })
            .filter_map(|(index, _)| u32::try_from(index).ok())
            .collect();
        cosmetic_pools.extend(
            items
                .iter()
                .flat_map(|item| item.sockets.iter())
                .filter(|socket| COSMETIC_SOCKET_TYPES.contains(&socket.socket_type))
                .map(|socket| socket.pool),
        );
        let cosmetic: std::collections::HashSet<u64> = cosmetic_pools
            .iter()
            .filter_map(|index| plug_pools.get(*index as usize))
            .flatten()
            .copied()
            .collect();

        let mut by_socket_type = HashMap::<u16, Vec<u64>>::new();
        let mut by_gear_kind = HashMap::<GearKind, Vec<u64>>::new();
        for item in &items {
            let kind = gear_kind(item.bucket_hash);
            for socket in &item.sockets {
                let Some(pool) = plug_pools.get(socket.pool as usize) else {
                    continue;
                };
                by_socket_type
                    .entry(socket.socket_type)
                    .or_default()
                    .extend(pool.iter().copied());
                by_gear_kind
                    .entry(kind)
                    .or_default()
                    .extend(pool.iter().filter(|hash| !cosmetic.contains(hash)).copied());
            }
        }
        let mut every_plug: Vec<u64> = plug_pools.iter().flatten().copied().collect();
        let sort_by_name = |options: &mut Vec<u64>| {
            options.sort_unstable();
            options.dedup();
            // Named plugs first, alphabetically; this build leaves a few
            // hundred plugs unnamed, and they belong at the end.
            options.sort_by_cached_key(|hash| {
                match names.get(hash).or_else(|| stat_plugs.get(hash)) {
                    Some(name) => (false, name.to_lowercase()),
                    None => (true, String::new()),
                }
            });
        };
        by_socket_type.values_mut().for_each(&sort_by_name);
        by_gear_kind.values_mut().for_each(&sort_by_name);
        sort_by_name(&mut every_plug);

        let by_hash = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.hash, index))
            .collect();
        let mod_plugs: std::collections::HashSet<u64> = items
            .iter()
            .flat_map(|item| item.sockets.iter())
            .filter(|socket| MOD_SOCKET_TYPES.contains(&socket.socket_type))
            .filter_map(|socket| plug_pools.get(socket.pool as usize))
            .flatten()
            .copied()
            .collect();
        let sources = intrinsic_sources(&items, &names, &plug_pools);
        Ok(Self {
            items,
            names,
            by_hash,
            plug_pools,
            by_socket_type,
            by_gear_kind,
            every_plug,
            descriptions: serde_json::from_slice(DESCRIPTIONS_JSON)
                .map_err(|error| format!("The bundled descriptions are invalid: {error}"))?,
            stat_plugs,
            mod_plugs,
            sources,
            cosmetic_pools,
        })
    }

    pub fn get(&self, hash: u64) -> Option<&ItemDef> {
        self.by_hash.get(&hash).and_then(|index| self.items.get(*index))
    }

    pub fn get_for_bucket(&self, hash: u64, bucket: u64) -> Option<&ItemDef> {
        self.get(hash).filter(|item| item.bucket_hash == bucket)
    }

    pub fn plug_name(&self, hash: u64) -> &str {
        self.names
            .get(&hash)
            .or_else(|| self.stat_plugs.get(&hash))
            .map_or("Unnamed plug", String::as_str)
    }

    pub fn plug_label(&self, hash: u64) -> String {
        format!("{}  ({})", self.plug_name(hash), format_hash(hash))
    }

    /// The item's or plug's in-game description, where this build has one.
    pub fn description(&self, hash: u64) -> Option<&str> {
        self.descriptions.get(&hash).map(String::as_str)
    }

    /// Whether a plug is an armor or weapon mod, which is the category this
    /// build leaves almost entirely without art.
    pub fn is_mod_plug(&self, hash: u64) -> bool {
        self.mod_plugs.contains(&hash)
    }

    /// Whether a socket holds a stat allocation rather than a mod.
    pub fn is_stat_socket(&self, item: &ItemDef, socket_index: usize) -> bool {
        item.sockets
            .get(socket_index)
            .is_some_and(|socket| STAT_SOCKET_TYPES.contains(&socket.socket_type))
    }

    /// Whether a plug is one of Armor 2.0's stat allocations, which the game
    /// ships unnamed and without art.
    pub fn is_stat_plug(&self, hash: u64) -> bool {
        self.stat_plugs.contains_key(&hash)
    }

    /// Whether a socket holds shaders, ornaments, or trackers rather than
    /// anything that changes how the item plays.
    pub fn is_cosmetic_socket(&self, item: &ItemDef, socket_index: usize) -> bool {
        item.sockets
            .get(socket_index)
            .is_some_and(|socket| self.cosmetic_pools.contains(&socket.pool))
    }

    /// Which armor system a piece was built for. Armor 2.0 introduced the mod
    /// socket every reissue carries; a Year-1 definition of the same piece has
    /// none. `None` for anything that is not armor.
    pub fn armor_generation(&self, item: &ItemDef) -> Option<&'static str> {
        (gear_kind(item.bucket_hash) == GearKind::Armor).then(|| {
            if item
                .sockets
                .iter()
                .any(|socket| socket.socket_type == ARMOR_MOD_SOCKET)
            {
                "Armor 2.0"
            } else {
                "Armor 1.0"
            }
        })
    }

    /// Where an archetype plug comes from: the exotic that carries it, or the
    /// weapon or armor type it belongs to. `None` for ordinary perks, which are
    /// the same wherever they appear.
    pub fn source(&self, hash: u64) -> Option<&str> {
        self.sources.get(&hash).map(String::as_str)
    }

    /// The plug hashes a socket may show under the chosen filter.
    pub fn plug_options(&self, item: &ItemDef, socket_index: usize, filter: PlugFilter) -> Vec<u64> {
        let Some(socket) = item.sockets.get(socket_index) else {
            return Vec::new();
        };
        let options = match filter {
            PlugFilter::Compatible => self.plug_pools.get(socket.pool as usize).map(Vec::as_slice),
            PlugFilter::SocketType => self.by_socket_type.get(&socket.socket_type).map(Vec::as_slice),
            PlugFilter::GearType => self
                .by_gear_kind
                .get(&gear_kind(item.bucket_hash))
                .map(Vec::as_slice),
            PlugFilter::All => Some(self.every_plug.as_slice()),
        };
        let mut options = options.unwrap_or_default().to_vec();

        // Gear type drops shaders, ornaments, and trackers so they do not
        // clutter every other socket. On their own sockets they belong, so add
        // them back on top of everything else that filter offers.
        if filter == PlugFilter::GearType && self.is_cosmetic_socket(item, socket_index) {
            if let Some(cosmetics) = self.by_socket_type.get(&socket.socket_type) {
                options.extend(cosmetics.iter().copied());
                options.sort_unstable();
                options.dedup();
                options.sort_by_cached_key(|hash| match self.names.get(hash) {
                    Some(name) => (false, name.to_lowercase()),
                    None => (true, String::new()),
                });
            }
        }
        options
    }

    /// Items that can be equipped in a slot by this class.
    pub fn items_for_slot(
        &self,
        slot: &str,
        class_type: u64,
        show_dummy_items: bool,
    ) -> impl Iterator<Item = &ItemDef> {
        let bucket = slot_bucket(slot).unwrap_or_default();
        self.items.iter().filter(move |item| {
            item.bucket_hash == bucket
                && (item.class_type == 3 || item.class_type == class_type)
                && (show_dummy_items || !dummy_items::contains(item.hash))
        })
    }
}

/// Exotic weapons are the ones with a catalyst socket. The socket counts even
/// when it is empty: Shadowkeep shipped Monte Carlo, Divinity, Xenophage and
/// others before their catalysts existed, so the socket holds nothing but
/// "Empty Catalyst Socket". Being the only item with a given frame does not
/// make a weapon exotic — this build has a one-off caster-frame sword and
/// lightweight-frame hand cannon — so this is what decides whether a weapon
/// archetype is named after its item or its type.
fn has_catalyst_socket(
    item: &ItemDef,
    names: &HashMap<u64, String>,
    plug_pools: &[Vec<u64>],
) -> bool {
    item.sockets.iter().any(|socket| {
        plug_pools
            .get(socket.pool as usize)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .any(|plug| names.get(plug).is_some_and(|name| name.contains("Catalyst")))
    })
}

/// Labels each archetype plug with the exotic that carries it, or with the
/// single gear type its owners share. Plugs spread across several types stay
/// unlabelled: there is nothing specific to say about them.
fn intrinsic_sources(
    items: &[ItemDef],
    names: &HashMap<u64, String>,
    plug_pools: &[Vec<u64>],
) -> HashMap<u64, String> {
    let mut owners = HashMap::<u64, Vec<usize>>::new();
    for (index, item) in items.iter().enumerate() {
        for (socket_index, socket) in item.sockets.iter().enumerate() {
            if !INTRINSIC_SOCKET_TYPES.contains(&socket.socket_type) {
                continue;
            }
            let installed = item
                .default_plugs
                .get(socket_index)
                .and_then(Option::as_deref)
                .and_then(crate::model::parse_hash);
            for plug in plug_pools
                .get(socket.pool as usize)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .copied()
                .chain(installed)
            {
                let owners = owners.entry(plug).or_default();
                if !owners.contains(&index) {
                    owners.push(index);
                }
            }
        }
    }

    owners
        .into_iter()
        .filter_map(|(plug, owners)| {
            let owners: Vec<&ItemDef> = owners.iter().map(|index| &items[*index]).collect();
            // Owners are grouped by name, not by count: an exotic is reissued
            // as several item definitions — a Year-1 copy and an Armor 2.0
            // copy — that share a name and carry the same trait plug.
            let mut owner_names = owners.iter().map(|item| item.name.as_str());
            let name = owner_names.next()?;
            let one_owner = !name.is_empty() && owner_names.all(|other| other == name);
            // Only one armor piece ever carries a given exotic trait, while
            // legendary armor shares its archetypes; weapons need the catalyst
            // test, since a frame can be unique without being exotic.
            let exotic = owners
                .iter()
                .all(|item| gear_kind(item.bucket_hash) == GearKind::Armor)
                || owners
                    .iter()
                    .any(|item| has_catalyst_socket(item, names, plug_pools));
            if one_owner && exotic {
                return Some((plug, name.to_owned()));
            }

            // Otherwise the useful distinction is the gear type, which is what
            // separates one "Precision Frame" from the next.
            let mut types = owners
                .iter()
                .map(|item| item.type_name.as_str())
                .filter(|type_name| !type_name.is_empty());
            let first = types.next()?;
            types
                .all(|type_name| type_name == first)
                .then(|| (plug, first.to_owned()))
        })
        .collect()
}

fn gear_kind(bucket: u64) -> GearKind {
    let bucket_of = |slots: &[&str]| slots.iter().filter_map(|slot| slot_bucket(slot)).any(|b| b == bucket);
    if bucket_of(WEAPON_SLOTS) {
        GearKind::Weapon
    } else if bucket_of(ARMOR_SLOTS) {
        GearKind::Armor
    } else {
        GearKind::Other(bucket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_catalog_loads_and_widens_plug_pools() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        assert!(catalog.items.len() > 1000);

        let weapon = catalog
            .items
            .iter()
            .find(|item| !item.sockets.is_empty() && gear_kind(item.bucket_hash) == GearKind::Weapon)
            .expect("the catalog must contain a socketed weapon");
        let compatible = catalog.plug_options(weapon, 0, PlugFilter::Compatible).len();
        let socket_type = catalog.plug_options(weapon, 0, PlugFilter::SocketType).len();
        let gear_type = catalog.plug_options(weapon, 0, PlugFilter::GearType).len();
        let all = catalog.plug_options(weapon, 0, PlugFilter::All).len();
        assert!(compatible <= socket_type);
        assert!(gear_type > compatible && all > gear_type);
    }

    #[test]
    fn armor_definitions_report_which_system_they_were_built_for() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let generation = |hash| catalog.get(hash).and_then(|item| catalog.armor_generation(item));
        // The two definitions of one exotic: the Year-1 copy and its reissue.
        assert_eq!(generation(0x8E66_339E), Some("Armor 1.0"));
        assert_eq!(generation(0x719C_AD22), Some("Armor 2.0"));
        // Shadowkeep's own Dreambane set is 2.0 despite having no energy plug.
        assert_eq!(generation(0x2D53_A1AF), Some("Armor 2.0"));
        // Weapons have no armor system.
        assert_eq!(generation(0xF27C_CB67), None);
    }

    #[test]
    fn descriptions_are_bundled_for_the_plugs_this_build_ships() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        // Outlaw, a trait on many Shadowkeep weapons.
        assert_eq!(
            catalog.description(0x5B17_BB28),
            Some("Precision kills greatly decrease reload time.")
        );
        let described = catalog
            .every_plug
            .iter()
            .filter(|hash| catalog.description(**hash).is_some())
            .count();
        assert!(described * 10 > catalog.every_plug.len() * 7);
    }

    #[test]
    fn archetype_plugs_name_their_weapon_type_or_exotic() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        // Frames that share a name across weapon classes are told apart by the
        // type they belong to.
        assert_eq!(catalog.source(0x2AA1_0305), Some("Combat Bow"));
        assert_eq!(catalog.source(0x7A9E_708F), Some("Linear Fusion Rifle"));
        assert_eq!(catalog.source(0xF8AF_C186), Some("Sword"));
        // An exotic's own intrinsic names the exotic, on weapons and on armor.
        assert_eq!(catalog.source(0x7E6D_3552), Some("Rat King"));
        // Monte Carlo's catalyst had not shipped in this build, so its socket
        // is empty; the weapon is exotic all the same.
        assert_eq!(catalog.source(0x587A_C9C6), Some("Monte Carlo"));
        // Both of these are carried by two item definitions of the same
        // exotic: a Year-1 copy and its Armor 2.0 reissue.
        assert_eq!(catalog.source(0x7E52_5913), Some("Graviton Forfeit"));
        assert_eq!(catalog.source(0x0081_CFC4), Some("Contraverse Hold"));
        // Being the only item with a frame does not make it exotic: this build
        // ships one caster-frame sword and one lightweight-frame hand cannon,
        // and both stay named after their type.
        assert_eq!(catalog.source(0x0AF4_25C1), Some("Sword"));
        assert_eq!(catalog.source(0x8286_25A4), Some("Hand Cannon"));
        // Ordinary perks appear everywhere and get no label.
        assert_eq!(catalog.source(0x5B17_BB28), None);

        assert!(
            INTRINSIC_SOCKET_TYPES.iter().all(|socket_type| catalog
                .items
                .iter()
                .any(|item| item.sockets.iter().any(|s| s.socket_type == *socket_type)))
        );
        // Every socket that can hold an exotic armor trait resolves to the same
        // label, whichever reissue of the exotic is equipped.
        for item in catalog.items.iter().filter(|item| item.name == "Contraverse Hold") {
            let labelled = (0..item.sockets.len()).any(|socket| {
                catalog
                    .plug_options(item, socket, PlugFilter::Compatible)
                    .iter()
                    .any(|plug| catalog.source(*plug) == Some("Contraverse Hold"))
            });
            assert!(labelled, "0x{:08X} lost its exotic trait", item.hash);
        }
    }

    #[test]
    fn shaders_ornaments_and_trackers_stay_out_of_the_gear_type_list() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let weapon = catalog
            .items
            .iter()
            .find(|item| !item.sockets.is_empty() && gear_kind(item.bucket_hash) == GearKind::Weapon)
            .expect("the catalog must contain a socketed weapon");

        let cosmetic_in = |filter| {
            catalog
                .plug_options(weapon, 0, filter)
                .iter()
                .filter(|hash| {
                    catalog
                        .names
                        .get(hash)
                        .is_some_and(|name| name.ends_with("Ornament") || name == "Default Shader")
                })
                .count()
        };
        assert_eq!(cosmetic_in(PlugFilter::GearType), 0);
        assert!(cosmetic_in(PlugFilter::All) > 0);

        // The shader socket itself still offers every shader.
        let shaders = catalog
            .items
            .iter()
            .flat_map(|item| {
                (0..item.sockets.len()).map(move |index| (item, index))
            })
            .map(|(item, index)| catalog.plug_options(item, index, PlugFilter::Compatible))
            .find(|options| {
                options
                    .iter()
                    .any(|hash| catalog.names.get(hash).is_some_and(|name| name == "Default Shader"))
            })
            .expect("the catalog must contain a shader socket");
        assert!(shaders.len() > 100);
    }
}
