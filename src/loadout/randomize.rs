//! Filling every slot with random gear, one equipped and nine held beside it.

use crate::{
    catalog::{ItemDef, PlugFilter},
    model::{SLOTS, pointer},
    settings,
    status::Change,
};

use super::{
    EQUIPPED_BOX, Editing, Page, Target,
    inventory::{INVENTORY_COLUMNS, INVENTORY_ROWS, inventory_id},
};

impl Page<'_> {
    /// Fills every slot with random gear: one equipped, nine held beside it,
    /// each with a random plug rolled into every socket that offers any.
    /// `plugs` decides where those come from: the socket's own list, or the
    /// wider one its socket type allows anywhere in the build.
    pub(super) fn randomize(&mut self, character: usize, plugs: PlugFilter) -> Change {
        // A roll replaces what the character holds; otherwise a second roll
        // would run into the room Sunrise reserves.
        settings::clear_inventory(self.document, character)?;
        let mut rng = Rng::from_clock();
        let class = self.class_type(character);
        let boxes = INVENTORY_ROWS * INVENTORY_COLUMNS;
        let mut exotic_equipped = false;
        let mut rolled = 0;
        let mut slots = 0;
        for slot in SLOTS {
            if slot.is_subclass() {
                continue;
            }
            let candidates: Vec<ItemDef> = self
                .catalog
                .items_for_slot(slot, class, self.state.show_dummy_items)
                .cloned()
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let ordinary: Vec<ItemDef> = candidates
                .iter()
                .filter(|item| !self.catalog.is_exotic(item))
                .cloned()
                .collect();
            slots += 1;
            let mut used: Vec<u64> = Vec::new();
            let last_box = if slot.holds_inventory { boxes } else { EQUIPPED_BOX };
            for box_index in EQUIPPED_BOX..=last_box {
                // One exotic in hand is plenty; the held boxes are free.
                let equipped_weapon = box_index == EQUIPPED_BOX && slot.is_weapon();
                let pool = if equipped_weapon && exotic_equipped { &ordinary } else { &candidates };
                // A few retries keep a slot from holding the same gun twice
                // without looping forever over a short list.
                let mut choice = rng.pick(pool);
                for _ in 0..8 {
                    match choice {
                        Some(item) if used.contains(&item.hash) => choice = rng.pick(pool),
                        _ => break,
                    }
                }
                let Some(item) = choice.cloned() else {
                    continue;
                };
                used.push(item.hash);
                if equipped_weapon {
                    exotic_equipped |= self.catalog.is_exotic(&item);
                }
                let editing = Editing { character, slot, target: Target::from_box(box_index) };
                self.install_random(editing, &item, &mut rng, plugs)?;
                rolled += 1;
            }
            self.state.selections.insert(inventory_id(character, slot), EQUIPPED_BOX);
        }
        Ok(match plugs {
            PlugFilter::Compatible => format!("Rolled {rolled} items across {slots} slots"),
            filter => format!(
                "Rolled {rolled} items across {slots} slots, with plugs from the {} list",
                filter.label().to_lowercase()
            ),
        })
    }

    /// Puts one rolled item where the editing column would have put it, with a
    /// random plug in every socket the catalog offers plugs for.
    fn install_random(
        &mut self,
        editing: Editing,
        item: &ItemDef,
        rng: &mut Rng,
        plugs: PlugFilter,
    ) -> Result<(), String> {
        let character = editing.character;
        // The rolled item is addressed by where it landed rather than by which
        // box shows it: the character may already hold others of its bucket.
        let pointer = match editing.target {
            Target::Equipped => {
                settings::equip_definition(
                    self.document,
                    character,
                    editing.slot,
                    item.hash,
                    &item.default_plugs,
                )?;
                editing.equipment_pointer()
            }
            Target::Parked(_) => {
                let held =
                    settings::hold_definition(self.document, character, item.hash, &item.default_plugs)?;
                pointer::held(character, held)
            }
        };

        let rolls: Vec<(usize, u64)> = (0..item.sockets.len())
            .filter_map(|socket| {
                let options = self.catalog.plug_options(item, socket, plugs);
                rng.pick(&options).map(|hash| (socket, *hash))
            })
            .collect();
        let Some(value) = self.document.pointer_mut(&pointer) else {
            return Ok(());
        };
        for (socket, hash) in rolls {
            settings::set_item_plug(value, socket, &item.default_plugs, Some(hash))?;
        }
        Ok(())
    }
}

/// A small xorshift seeded from the clock. The randomizer wants variety, not
/// cryptography, and this keeps the dependency list where it is.
struct Rng(u64);

impl Rng {
    fn from_clock() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                since.as_secs().wrapping_mul(1_000_000_000).wrapping_add(since.subsec_nanos().into())
            });
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `None` for an empty list, which is most sockets: plenty of them offer
    /// nothing to roll.
    fn pick<'a, T>(&mut self, options: &'a [T]) -> Option<&'a T> {
        let count = u64::try_from(options.len()).ok().filter(|count| *count > 0)?;
        let index = usize::try_from(self.next() % count).ok()?;
        options.get(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::Catalog,
        icons::Icons,
        loadout::LoadoutState,
        model::parse_unsigned_value,
    };

    #[test]
    fn picking_from_an_empty_list_yields_nothing() {
        let mut rng = Rng::from_clock();
        // Most sockets have no plugs to offer, so the randomizer asks this of
        // an empty list constantly; a modulo by zero would take the app down.
        assert!(rng.pick::<u64>(&[]).is_none());
        assert_eq!(rng.pick(&[7_u64]), Some(&7));

        let options = [1_u64, 2, 3, 4];
        for _ in 0..64 {
            assert!(options.contains(rng.pick(&options).expect("a non-empty list always picks")));
        }
    }

    #[test]
    fn randomizing_fills_every_box_of_every_slot() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let mut document = serde_json::json!({
            "version": 6,
            "state": { "characters": [{
                "soid": "0x9EAA300100100101",
                "class": 1,
                "equipment": {},
                "inventory": []
            }] }
        });
        let mut icons = Icons::new();
        let mut state = LoadoutState::default();
        let mut page = Page {
            document: &mut document,
            catalog: &catalog,
            icons: &mut icons,
            state: &mut state,
        };
        page.randomize(0, PlugFilter::Compatible).expect("randomizing must not fail");

        let mut exotic_weapons = 0;
        for slot in SLOTS {
            if slot.is_subclass() {
                continue;
            }
            let equipped = document
                .pointer(&format!("/state/characters/0/equipment/{}", slot.name))
                .and_then(|item| item.get("definition_hash"))
                .and_then(parse_unsigned_value)
                .unwrap_or_else(|| panic!("{} was left without an item", slot.name));
            if slot.is_weapon() {
                exotic_weapons += usize::from(
                    catalog.get(equipped).is_some_and(|item| catalog.is_exotic(item)),
                );
            }
            let held = settings::character_inventory(&document, 0)
                .iter()
                .filter(|item| {
                    item.get("definition_hash")
                        .and_then(parse_unsigned_value)
                        .and_then(|hash| catalog.get(hash))
                        .is_some_and(|definition| definition.bucket_hash == slot.bucket)
                })
                .count();
            // A clan banner has no inventory to fill: the game equips one.
            let wanted = if slot.holds_inventory { INVENTORY_ROWS * INVENTORY_COLUMNS } else { 0 };
            assert_eq!(held, wanted, "{} held the wrong number of items", slot.name);
        }
        assert!(exotic_weapons <= 1, "the randomizer equipped {exotic_weapons} exotic weapons");

        // Everything it wrote has to be something Sunrise will read back.
        settings::validate_characters(&document).expect("a rolled loadout must be valid");
        let held = settings::character_inventory(&document, 0).len();
        assert!(held <= crate::model::MAX_CHARACTER_INVENTORY, "{held} items is too many to hold");
    }

    #[test]
    fn rolling_replaces_what_a_character_already_holds() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let mut document = serde_json::json!({
            "version": 6,
            "state": { "characters": [{
                "soid": "0x9EAA300100100101",
                "class": 1,
                "equipment": {},
                "inventory": [{
                    "instance_soid": "0x4000000000000011",
                    "definition_hash": "0xE516CF40",
                    "level": 106,
                    "quantity": 1,
                    "plugs": null
                }]
            }] }
        });
        let mut icons = Icons::new();
        let mut state = LoadoutState::default();
        let mut page = Page {
            document: &mut document,
            catalog: &catalog,
            icons: &mut icons,
            state: &mut state,
        };
        page.randomize(0, PlugFilter::Compatible).expect("the first roll must work");
        // The wider roll writes the same shape, from a longer list of plugs.
        page.randomize(0, PlugFilter::SocketType).expect("a second roll must work too");

        // Nine per slot, for every slot that holds an inventory.
        let slots = SLOTS
            .iter()
            .filter(|slot| !slot.is_subclass() && slot.holds_inventory)
            .count();
        let held = settings::character_inventory(&document, 0);
        assert_eq!(
            held.len(),
            slots * INVENTORY_ROWS * INVENTORY_COLUMNS,
            "a second roll left the wrong number of items behind"
        );
        // The one the file came with had no rolled sockets, and is gone.
        assert!(held.iter().all(|item| item["plugs"].is_array()));
        // Every rolled item is its own instance, and carries its own sockets.
        let instances: std::collections::HashSet<&str> =
            held.iter().filter_map(|item| item["instance_soid"].as_str()).collect();
        assert_eq!(instances.len(), held.len(), "two held items share an instance");
        settings::validate_characters(&document).expect("a rolled loadout must be valid");
    }
}
