//! The ten boxes a slot can hold: the equipped item, and the 3x3 matrix of the
//! rest. Sunrise keeps one pool per character, so a slot's boxes are the held
//! items whose definitions name its bucket.

use eframe::egui;
use serde_json::Value;

use crate::{
    model::{Slot, parse_unsigned_value},
    settings,
    status::Change,
};

use super::{
    EQUIPPED_BOX, Editing, Page, Target,
    widgets::{divider, hash_menu, icon_height},
};

pub(super) const INVENTORY_ROWS: usize = 3;
pub(super) const INVENTORY_COLUMNS: usize = 3;
pub(super) const INVENTORY_CELL: f32 = super::widgets::GEAR_ICON;

impl Page<'_> {
    /// Where a slot's boxes are in the character's held items. Sunrise keeps
    /// one pool per character and places each item by the bucket its definition
    /// names, so a slot's row is the held items that belong to that bucket, in
    /// the order the file lists them.
    pub(super) fn held_for_slot(&self, character: usize, slot: &Slot) -> Vec<usize> {
        settings::character_inventory(self.document, character)
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.get("definition_hash")
                    .and_then(parse_unsigned_value)
                    .and_then(|hash| self.catalog.get(hash))
                    .is_some_and(|definition| definition.bucket_hash == slot.bucket)
            })
            .map(|(index, _)| index)
            .take(INVENTORY_ROWS * INVENTORY_COLUMNS)
            .collect()
    }

    /// Which held item a box is showing, if it has one yet.
    pub(super) fn held_index(&self, character: usize, slot: &Slot, box_index: usize) -> Option<usize> {
        self.held_for_slot(character, slot).get(box_index - 1).copied()
    }

    /// The ten boxes a slot can hold: the equipped one, a divider, then the
    /// 3x3 matrix of the rest. Only the equipped box has anything in it — the
    /// other nine are picked up by a later build.
    pub(super) fn draw_inventory(
        &mut self,
        ui: &mut egui::Ui,
        character: usize,
        slot: &'static Slot,
        id: egui::Id,
        selected: usize,
    ) -> Option<Change> {
        let equipped_hash = self.equipped_hash(character, slot);
        let mut picked = None;
        let mut change = None;
        {
            ui.horizontal_top(|ui| {
                let equipped = self
                    .inventory_box(ui, equipped_hash, selected == EQUIPPED_BOX)
                    .on_hover_text("Equipped");
                hash_menu(&equipped, equipped_hash);
                if equipped.clicked() {
                    picked = Some(EQUIPPED_BOX);
                }
                divider(ui, matrix_height(ui));
                ui.vertical(|ui| {
                    for row in 0..INVENTORY_ROWS {
                        ui.horizontal(|ui| {
                            for column in 0..INVENTORY_COLUMNS {
                                let box_index = EQUIPPED_BOX + 1 + row * INVENTORY_COLUMNS + column;
                                let hash = self
                                    .item_value(Editing {
                                        character,
                                        slot,
                                        target: Target::Parked(box_index),
                                    })
                                    .and_then(|item| item.get("definition_hash"))
                                    .and_then(parse_unsigned_value);
                                let hover = match hash {
                                    Some(hash) => {
                                        format!("Inventory {box_index}\n{}", self.item_name(hash))
                                    }
                                    None => format!("Inventory {box_index}\nEmpty"),
                                };
                                let held = self
                                    .inventory_box(ui, hash, selected == box_index)
                                    .on_hover_text(hover);
                                hash_menu(&held, hash);
                                if held.clicked() {
                                    picked = Some(box_index);
                                }
                            }
                        });
                    }
                });
            });
            let equip = ui
                .add_enabled(selected != EQUIPPED_BOX, egui::Button::new("Equip"))
                .on_hover_text("Swap the selected item with what is equipped.")
                .on_disabled_hover_text("This item is already equipped.");
            if equip.clicked() {
                change = Some(self.equip_parked(character, slot, id, selected));
            }
        }
        if let Some(box_index) = picked {
            self.state.selections.insert(id, box_index);
        }
        change
    }

    /// One inventory box, ringed when it is the one being edited.
    fn inventory_box(
        &mut self,
        ui: &mut egui::Ui,
        hash: Option<u64>,
        selected: bool,
    ) -> egui::Response {
        let response = self.icon_button(ui, hash, INVENTORY_CELL);
        if selected {
            ui.painter().rect_stroke(
                response.rect,
                3.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        response
    }

    /// Swaps the selected box with the equipped item. Emptying the equipped
    /// box only works on a weapon slot, which is where the error comes from.
    fn equip_parked(
        &mut self,
        character: usize,
        slot: &'static Slot,
        id: egui::Id,
        box_index: usize,
    ) -> Change {
        let held = self.held_index(character, slot, box_index);
        let name = |page: &Self, item: Option<&Value>| {
            item.and_then(|item| item.get("definition_hash"))
                .and_then(parse_unsigned_value)
                .map(|hash| page.item_name(hash))
        };
        let incoming = name(
            self,
            held.and_then(|held| settings::character_inventory(self.document, character).get(held)),
        );
        let outgoing = name(self, self.equipped(character, slot));

        settings::swap_equipped(self.document, character, slot, held)?;
        self.state.selections.insert(id, EQUIPPED_BOX);
        Ok(match (incoming, outgoing) {
            (Some(equipped), Some(held)) => {
                format!("Equipped {equipped}, holding {held} in inventory {box_index}")
            }
            (Some(equipped), None) => format!("Equipped {equipped} from inventory {box_index}"),
            (None, Some(held)) => format!(
                "Moved {held} to inventory {box_index}, emptying the {}",
                slot.label
            ),
            (None, None) => format!("Inventory {box_index} is empty"),
        })
    }
}

/// The key a row's inventory is stored under. It has to be reachable without a
/// `Ui`, since the randomizer fills every box of every row in one go.
pub(super) fn inventory_id(character: usize, slot: &Slot) -> egui::Id {
    egui::Id::new(("inventory", character, slot.name))
}

fn matrix_height(ui: &egui::Ui) -> f32 {
    let rows = INVENTORY_ROWS as f32;
    rows * icon_height(ui, INVENTORY_CELL) + (rows - 1.0) * ui.spacing().item_spacing.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{catalog::Catalog, icons::Icons, loadout::LoadoutState};

    fn slot(name: &str) -> &'static Slot {
        Slot::from_name(name).expect("the tests only name real slots")
    }

    #[test]
    fn a_slots_boxes_show_only_the_held_items_of_its_bucket() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let kinetic = catalog
            .items_for_slot(slot("kinetic"), 1, false)
            .next()
            .expect("the catalog must have a kinetic weapon")
            .clone();
        let ghost = catalog
            .items_for_slot(slot("ghost"), 1, false)
            .next()
            .expect("the catalog must have a ghost")
            .clone();

        let mut document = serde_json::json!({
            "version": 6,
            "state": { "characters": [{ "soid": "0x1", "class": 1, "equipment": {} }] }
        });
        settings::hold_definition(&mut document, 0, ghost.hash, &ghost.default_plugs).unwrap();
        settings::hold_definition(&mut document, 0, kinetic.hash, &kinetic.default_plugs).unwrap();

        let mut icons = Icons::new();
        let mut state = LoadoutState::default();
        let page = Page {
            document: &mut document,
            catalog: &catalog,
            icons: &mut icons,
            state: &mut state,
        };
        assert_eq!(page.held_for_slot(0, slot("kinetic")), vec![1]);
        assert_eq!(page.held_for_slot(0, slot("ghost")), vec![0]);
        assert_eq!(page.held_index(0, slot("kinetic"), 1), Some(1));
        assert_eq!(page.held_index(0, slot("kinetic"), 2), None);
    }
}
