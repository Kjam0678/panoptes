//! How a piece's sockets fall into lines.
//!
//! A line is two independent things side by side: the pinned column, and the
//! row beside it. The pinned column belongs to pinned sockets alone — a row
//! never reaches into it, and it stays reserved on every line whether or not
//! that line pins anything, so the rows of a piece all start at one place.
//! Each has its own rule for what goes in it and its own bound on how much.

use crate::catalog::{CosmeticKind, ItemDef};
use crate::model::Slot;

use super::Page;

/// How many sockets a row holds before it wraps to the next line. This bounds
/// the row area alone; the pinned column is not part of it.
pub(super) const MAX_ROW_WIDTH: usize = 5;

/// How many lines the pinned column can claim. Pins stack down the column, so
/// this only has to clear the most any piece specifies — a weapon's intrinsic
/// and masterwork, or a Solstice piece's intrinsic and glow.
pub(super) const MAX_PINS: usize = 3;

/// The sets a row is built from. Each group gets its own row, and the order
/// here is the order those rows appear down the piece.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RowGroup {
    Perk,
    Mod,
    Stat,
    /// Sockets with nothing to choose from, and the Red War damage mods.
    Spare,
    Cosmetic,
}

impl RowGroup {
    /// What a socket sorts by. Heavier sorts later, so the cosmetics trail the
    /// piece and the shader ends it.
    fn weight(self, cosmetic: Option<CosmeticKind>) -> u16 {
        match self {
            Self::Perk => 10,
            Self::Mod => 20,
            Self::Stat => 30,
            Self::Spare => 40,
            Self::Cosmetic => 50 + cosmetic.map_or(0, CosmeticKind::order),
        }
    }
}

/// One line: at most one pinned socket in the reserved column, and the row
/// beside it. Either may be empty — a piece with more pins than rows leaves
/// the row side blank, and one with more rows than pins leaves the column
/// blank — but the column's width is held on every line either way.
pub(super) struct SocketLine {
    pub(super) pinned: Option<usize>,
    pub(super) row: Vec<usize>,
}

impl Page<'_> {
    /// The lines a piece's sockets are drawn on. The pinned column and the
    /// rows are worked out separately and then set beside each other, so
    /// neither can push the other around.
    pub(super) fn socket_lines(
        &self,
        item: &ItemDef,
        slot: &Slot,
        socket_count: usize,
    ) -> Vec<SocketLine> {
        // Plain socket order fills the rows and pins nothing, leaving the
        // column reserved but empty.
        if !self.state.group_sockets {
            let rows = wrapped_rows(&(0..socket_count).collect::<Vec<_>>());
            return lines(Vec::new(), rows);
        }

        let pinned = self.pinned_sockets(item, slot, socket_count);
        let rows = self.grouped_rows(item, slot, socket_count, &pinned);
        lines(pinned, rows)
    }

    /// The sockets that hold the pinned column, top to bottom. Nothing here is
    /// inferred: a socket is pinned only where this names one, and a piece that
    /// matches nothing leaves the column empty rather than being given a pin it
    /// has no use for.
    ///
    /// Each rule names the socket by what it is rather than by where it sits,
    /// because position varies: a Ghost's projection is anywhere from its
    /// second socket to its sixth, and the one ship with a single socket keeps
    /// its transmat effect in it.
    fn pinned_sockets(&self, item: &ItemDef, slot: &Slot, socket_count: usize) -> Vec<usize> {
        let catalog = self.catalog;
        let find = |test: &dyn Fn(usize) -> bool| (0..socket_count).find(|socket| test(*socket));
        let mut pins: Vec<usize> = Vec::new();

        if slot.is_weapon() {
            // The frame, and the masterwork or catalyst under it. Which socket
            // carries the masterwork varies from weapon to weapon.
            pins.extend(find(&|socket| catalog.is_intrinsic_socket(item, socket)));
            pins.extend(find(&|socket| catalog.is_masterwork_socket(item, socket)));
            // The damage mod a Red War-era weapon carries — kinetic, or the
            // elemental one an energy weapon has in its place. It sorts to the
            // bottom of the column whatever else is pinned above it.
            pins.extend(find(&|socket| catalog.is_damage_mod_socket(item, socket)));
        } else if slot.is_armor() {
            if catalog.is_mask(item) {
                // A mask has one socket and that is the entire item.
                pins.extend(find(&|_| true));
            } else if catalog.is_modern_armor(item) {
                // Armor 2.0's upgrades. A piece without them pins nothing.
                pins.extend(find(&|socket| catalog.is_energy_socket(item, socket)));
            } else {
                // Year-1 armor pins what it is built around and its masterwork
                // both, and either may be absent: most class items have no
                // intrinsic, and their masterwork is the tier track.
                pins.extend(find(&|socket| catalog.is_armor_intrinsic_socket(item, socket)));
                pins.extend(find(&|socket| catalog.is_masterwork_socket(item, socket)));
            }
            // A Solstice piece pins its glow as well as whatever else it holds.
            pins.extend(find(&|socket| catalog.is_glow_socket(item, socket)));
        } else if slot.name == "vehicle" {
            pins.extend(find(&|socket| catalog.is_vehicle_drive_socket(item, socket)));
        } else if slot.name == "ship" {
            pins.extend(find(&|socket| catalog.is_transmat_socket(item, socket)));
        } else if slot.name == "ghost" {
            pins.extend(find(&|socket| catalog.is_projection_socket(item, socket)));
        } else if slot.name == "clan_banner" {
            pins.extend(find(&|socket| catalog.is_banner_staff_socket(item, socket)));
        }

        // The same weight that orders the rows orders the column: heavier sorts
        // further down, and a socket's own number breaks a tie.
        pins.sort_by_key(|socket| (self.pin_weight(item, slot, *socket), *socket));
        pins.dedup();
        pins
    }

    /// What a pin sorts by down the column. The damage mod carries the heaviest
    /// weight there is, so it sits below every other pin whatever group it
    /// would otherwise sort with.
    fn pin_weight(&self, item: &ItemDef, slot: &Slot, socket: usize) -> u16 {
        if self.catalog.is_damage_mod_socket(item, socket) {
            u16::MAX
        } else {
            self.socket_weight(item, slot, socket)
        }
    }

    /// What a socket sorts by, in the column and along a row alike.
    fn socket_weight(&self, item: &ItemDef, slot: &Slot, socket: usize) -> u16 {
        self.socket_group(item, slot, socket)
            .weight(self.catalog.cosmetic_kind(item, socket))
    }

    /// The rows, one group to a row. A group wider than a row wraps onto
    /// further rows of its own rather than sharing with the next group.
    fn grouped_rows(
        &self,
        item: &ItemDef,
        slot: &Slot,
        socket_count: usize,
        pinned: &[usize],
    ) -> Vec<Vec<usize>> {
        let mut loose: Vec<(u16, RowGroup, usize)> = (0..socket_count)
            .filter(|socket| !pinned.contains(socket))
            .map(|socket| (self.socket_weight(item, slot, socket), self.socket_group(item, slot, socket), socket))
            .collect();
        // Lightest first, and a socket's own number breaks a tie. Weights are
        // ordered by group, so each group comes out in one piece.
        loose.sort_unstable();

        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut open = None;
        for (_, group, socket) in loose {
            let full = rows.last().is_some_and(|row: &Vec<usize>| row.len() >= MAX_ROW_WIDTH);
            if open != Some(group) || full {
                rows.push(Vec::new());
            }
            rows.last_mut().expect("a row was just opened").push(socket);
            open = Some(group);
        }
        rows
    }

    /// Which set a socket belongs to. Each set is drawn on a row of its own.
    fn socket_group(&self, item: &ItemDef, slot: &Slot, socket: usize) -> RowGroup {
        if self.catalog.cosmetic_kind(item, socket).is_some() {
            RowGroup::Cosmetic
        } else if self.catalog.is_stat_socket(item, socket) {
            RowGroup::Stat
        } else if self.catalog.is_secondary_socket(item, socket) {
            RowGroup::Mod
        } else if slot.is_armor() || self.catalog.is_mod_socket(item, socket) {
            RowGroup::Mod
        } else {
            RowGroup::Perk
        }
    }
}

/// Sets the two columns beside each other. A piece is as tall as the taller of
/// them, and the pinned column is capped independently of the rows.
fn lines(pinned: Vec<usize>, rows: Vec<Vec<usize>>) -> Vec<SocketLine> {
    let pinned: Vec<usize> = pinned.into_iter().take(MAX_PINS).collect();
    (0..pinned.len().max(rows.len()))
        .map(|line| SocketLine {
            pinned: pinned.get(line).copied(),
            row: rows.get(line).cloned().unwrap_or_default(),
        })
        .collect()
}

/// Plain socket order, wrapped to the width of a row.
fn wrapped_rows(sockets: &[usize]) -> Vec<Vec<usize>> {
    sockets
        .chunks(MAX_ROW_WIDTH)
        .map(<[usize]>::to_vec)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;







    /// Year-1 armor pins its intrinsic and its masterwork both, one above
    /// the other.
    #[test]
    fn year_one_armor_pins_its_intrinsic_and_its_masterwork() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        let mut both = 0;
        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash).filter(|slot| slot.is_armor())
            else {
                continue;
            };
            let named = |test: &dyn Fn(usize) -> bool| (0..item.sockets.len()).find(|s| test(*s));
            let (Some(intrinsic), Some(masterwork)) = (
                named(&|s| catalog.is_armor_intrinsic_socket(item, s)),
                named(&|s| catalog.is_masterwork_socket(item, s)),
            ) else {
                continue;
            };
            let pins = page.pinned_sockets(item, slot, item.sockets.len());
            assert!(
                pins.contains(&intrinsic) && pins.contains(&masterwork),
                "{} pins {:?}, not both its intrinsic and masterwork",
                item.name,
                pins
            );
            both += 1;
        }
        assert_eq!(both, 931, "armor stopped pinning both");
    }

    /// Every socket a rule names is actually pinned. The glow in particular
    /// is a family of ten types — one per armor slot, twice over — so keying
    /// on one of them silently loses the other nine.
    #[test]
    fn every_socket_a_rule_names_is_pinned() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        let mut glows = 0;
        let mut damage_mods = 0;
        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash) else {
                continue;
            };
            let pins = page.pinned_sockets(item, slot, item.sockets.len());
            let named = |test: &dyn Fn(usize) -> bool| (0..item.sockets.len()).find(|s| test(*s));

            if let Some(glow) = named(&|s| catalog.is_glow_socket(item, s)) {
                assert!(
                    pins.contains(&glow),
                    "{} left its glow at socket {} unpinned",
                    item.name,
                    glow + 1
                );
                glows += 1;
            }
            if slot.is_weapon()
                && let Some(damage) = named(&|s| catalog.is_damage_mod_socket(item, s))
            {
                assert!(
                    pins.contains(&damage),
                    "{} left its damage mod at socket {} unpinned",
                    item.name,
                    damage + 1
                );
                damage_mods += 1;
            }
        }
        // Both Solstice generations, across all five armor slots.
        assert_eq!(glows, 150, "the glow family stopped being recognised whole");
        assert_eq!(damage_mods, 318, "damage mod sockets stopped being recognised");
    }



    /// Each rule pins the socket it names, wherever that socket sits.
    #[test]
    fn every_pin_lands_on_the_socket_its_rule_names() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        let mut seen = std::collections::BTreeMap::<&str, usize>::new();
        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash) else {
                continue;
            };
            let where_ = format!("{} ({})", item.name, slot.name);
            let pins = page.pinned_sockets(item, slot, item.sockets.len());
            assert!(pins.len() <= MAX_PINS, "{where_} specifies {} pins", pins.len());

            for &pin in &pins {
                let named = if slot.is_weapon() {
                    catalog.is_intrinsic_socket(item, pin)
                        || catalog.is_masterwork_socket(item, pin)
                        || catalog.is_damage_mod_socket(item, pin)
                } else if slot.is_armor() {
                    catalog.is_glow_socket(item, pin)
                        || (catalog.is_mask(item) && pin == 0)
                        || catalog.is_energy_socket(item, pin)
                        || catalog.is_armor_intrinsic_socket(item, pin)
                        || catalog.is_masterwork_socket(item, pin)
                } else {
                    catalog.is_vehicle_drive_socket(item, pin)
                        || catalog.is_transmat_socket(item, pin)
                        || catalog.is_projection_socket(item, pin)
                        || catalog.is_banner_staff_socket(item, pin)
                };
                assert!(named, "{where_} pinned socket {} for no stated reason", pin + 1);
                *seen.entry(slot.name).or_default() += 1;
            }
        }
        // Every rule has to be reached by something in the catalog, or it is
        // describing gear this build does not ship.
        for slot in ["kinetic", "helmet", "class_item", "vehicle", "ship", "ghost", "clan_banner"] {
            assert!(seen.contains_key(slot), "nothing in {slot} was ever pinned");
        }
    }

    /// The rules that say "if it exists" pin nothing when it does not, rather
    /// than falling back to whatever socket happens to be first.
    #[test]
    fn a_piece_that_matches_no_rule_pins_nothing() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        let unpinnable = catalog
            .items
            .iter()
            .filter(|item| {
                Slot::from_bucket(item.bucket_hash).is_some_and(|slot| {
                    slot.name == "class_item"
                        && !catalog.is_modern_armor(item)
                        && (0..item.sockets.len()).all(|socket| {
                            !catalog.is_masterwork_socket(item, socket)
                                && !catalog.is_armor_intrinsic_socket(item, socket)
                                && !catalog.is_glow_socket(item, socket)
                        })
                })
            })
            .count();
        assert!(unpinnable > 0, "no Year-1 class item lacks everything pinnable");

        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash) else {
                continue;
            };
            for pin in page.pinned_sockets(item, slot, item.sockets.len()) {
                assert!(
                    pin < item.sockets.len(),
                    "{} pinned a socket it does not have",
                    item.name
                );
            }
        }
    }

    /// A weapon pins its frame above its masterwork, whichever socket that is.
    #[test]
    fn a_weapons_frame_sits_above_its_masterwork() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);
        let kinetic = Slot::from_name("kinetic").expect("the slot table has the kinetic slot");

        let mut both = 0;
        for item in catalog.items_for_slot(kinetic, 1, false) {
            let pins = page.pinned_sockets(item, kinetic, item.sockets.len());
            let masterwork = pins
                .iter()
                .position(|socket| catalog.is_masterwork_socket(item, *socket));
            let frame = pins
                .iter()
                .position(|socket| catalog.is_intrinsic_socket(item, *socket));
            if let (Some(frame), Some(masterwork)) = (frame, masterwork) {
                assert!(frame < masterwork, "{} pins its masterwork first", item.name);
                both += 1;
            }
        }
        assert!(both > 100, "weapons stopped pinning both");
    }

    /// The damage mod carries the heaviest weight, so it sits below every
    /// other pin on the weapons that have one — kinetic and energy alike.
    #[test]
    fn the_damage_mod_is_the_last_pin_of_a_weapon() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        let mut carried = 0;
        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash).filter(|slot| slot.is_weapon())
            else {
                continue;
            };
            let pins = page.pinned_sockets(item, slot, item.sockets.len());
            let Some(at) = pins
                .iter()
                .position(|socket| catalog.is_damage_mod_socket(item, *socket))
            else {
                continue;
            };
            assert_eq!(at, pins.len() - 1, "{} pins its damage mod above another", item.name);
            carried += 1;
        }
        assert!(carried > 50, "damage mod sockets stopped being pinned");
    }

    use serde_json::Value;

    use crate::{catalog::Catalog, icons::Icons, loadout::LoadoutState, model::Slot};

    fn page<'a>(
        document: &'a mut Value,
        catalog: &'a Catalog,
        icons: &'a mut Icons,
        state: &'a mut LoadoutState,
    ) -> Page<'a> {
        Page { document, catalog, icons, state }
    }

    /// Whatever else changes, every socket has to be reachable exactly once:
    /// one that is not drawn cannot be edited.
    #[test]
    fn every_socket_is_drawn_exactly_once() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();

        for grouped in [true, false] {
            state.group_sockets = grouped;
            let page = page(&mut document, &catalog, &mut icons, &mut state);
            for item in &catalog.items {
                let Some(slot) = Slot::from_bucket(item.bucket_hash) else {
                    continue;
                };
                let where_ = format!("{} ({}, grouped {grouped})", item.name, slot.name);
                let drawn: Vec<usize> = page
                    .socket_lines(item, slot, item.sockets.len())
                    .iter()
                    .flat_map(|line| line.pinned.into_iter().chain(line.row.iter().copied()))
                    .collect();

                let mut sorted = drawn.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted.len(), drawn.len(), "{where_} drew a socket twice");
                assert_eq!(
                    sorted,
                    (0..item.sockets.len()).collect::<Vec<_>>(),
                    "{where_} left a socket undrawn"
                );
            }
        }
    }

    /// The two columns are bounded separately: a row never runs past its own
    /// width, and the pinned column never claims more lines than it may.
    #[test]
    fn each_column_keeps_to_its_own_bound() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash) else {
                continue;
            };
            let where_ = format!("{} ({})", item.name, slot.name);
            let lines = page.socket_lines(item, slot, item.sockets.len());
            assert!(
                lines.iter().filter(|line| line.pinned.is_some()).count() <= MAX_PINS,
                "{where_} pins more than the column holds"
            );
            for line in &lines {
                assert!(
                    line.row.len() <= MAX_ROW_WIDTH,
                    "{where_} has a row {} across",
                    line.row.len()
                );
            }
        }
    }

    /// One group to a row: no row mixes two, and a group only continues onto
    /// another row because it filled the one before.
    #[test]
    fn a_row_holds_one_group_and_wraps_only_when_full() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash) else {
                continue;
            };
            let where_ = format!("{} ({})", item.name, slot.name);
            let lines = page.socket_lines(item, slot, item.sockets.len());
            let mut previous: Option<(RowGroup, usize)> = None;
            for line in lines.iter().filter(|line| !line.row.is_empty()) {
                let groups: Vec<RowGroup> = line
                    .row
                    .iter()
                    .map(|socket| page.socket_group(item, slot, *socket))
                    .collect();
                let group = groups[0];
                assert!(
                    groups.iter().all(|other| *other == group),
                    "{where_} mixed two groups on one row"
                );
                if let Some((previous_group, width)) = previous {
                    assert!(
                        previous_group != group || width == MAX_ROW_WIDTH,
                        "{where_} split a group across rows that were not full"
                    );
                }
                previous = Some((group, line.row.len()));
            }
        }
    }

    /// Weight order still runs down the piece, so the shader ends it.
    #[test]
    fn the_shader_is_the_last_socket_of_a_piece() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let (mut document, mut icons) = (Value::Null, Icons::new());
        let mut state = LoadoutState::default();
        let page = page(&mut document, &catalog, &mut icons, &mut state);

        let mut checked = 0;
        for item in &catalog.items {
            let Some(slot) = Slot::from_bucket(item.bucket_hash) else {
                continue;
            };
            let Some(shader) = (0..item.sockets.len())
                .find(|socket| catalog.cosmetic_kind(item, *socket) == Some(CosmeticKind::Shader))
            else {
                continue;
            };
            let last = page
                .socket_lines(item, slot, item.sockets.len())
                .iter()
                .flat_map(|line| line.row.iter().copied())
                .last();
            assert_eq!(last, Some(shader), "{} does not end on its shader", item.name);
            checked += 1;
        }
        assert!(checked > 100, "shaders stopped being recognised");
    }
}
