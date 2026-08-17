//! The loadout page: one row per gear piece, sockets laid out left to right,
//! each socket a button showing the icon of whatever is plugged into it.

mod character;
mod inventory;
mod layout;
mod picker;
mod randomize;
mod widgets;

pub use character::character_tab_label;

use std::collections::HashMap;

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{Catalog, ItemDef, PlugFilter},
    icons::Icons,
    model::{SLOTS, Slot, format_hash, parse_unsigned_value, pointer},
    settings,
    status::Change,
    theme,
};

use inventory::inventory_id;
use picker::{PLUG_ROW_ICON, Picker};
use widgets::{
    GEAR_ICON, SOCKET_ICON, divider, editor_width, hash_menu, icon_height, pin_column_width,
};

/// A slot holds ten items in Destiny: the equipped one, and a 3x3 matrix of
/// the rest. Box 0 is the equipped one, the only box this build writes to.
const EQUIPPED_BOX: usize = 0;

/// What Sunrise ships characters at in this build.
const DEFAULT_ITEM_LEVEL: i64 = 106;

/// UI-only state for the loadout page.
pub struct LoadoutState {
    pub filter: PlugFilter,
    pub show_dummy_items: bool,
    pub group_sockets: bool,
    searches: HashMap<egui::Id, String>,
    /// Which of a slot's ten inventory boxes each row has selected. Absent
    /// means the equipped one, which is what every row starts on.
    selections: HashMap<egui::Id, usize>,
}

impl Default for LoadoutState {
    fn default() -> Self {
        Self {
            filter: PlugFilter::default(),
            show_dummy_items: false,
            // Sockets that change how an item plays lead the row; the ones that
            // only change how it looks follow. Most edits are after the former.
            group_sockets: true,
            searches: HashMap::new(),
            selections: HashMap::new(),
        }
    }
}

impl LoadoutState {
    /// Drops per-slot search text and box selections when a different file is
    /// opened.
    pub fn clear_pickers(&mut self) {
        self.searches.clear();
        self.selections.clear();
    }

    fn selection(&self, id: egui::Id) -> usize {
        self.selections.get(&id).copied().unwrap_or(EQUIPPED_BOX)
    }
}

/// What the editing column is pointed at. The equipped item lives in the
/// document; the other nine boxes are held beside it until they are swapped in,
/// and edit exactly the same way.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Equipped,
    Parked(usize),
}

/// Which item the editing column is working on: the slot it belongs to, and
/// which of that slot's ten boxes.
#[derive(Clone, Copy)]
struct Editing {
    character: usize,
    slot: &'static Slot,
    target: Target,
}

impl Editing {
    /// What a status line calls the thing being edited.
    fn label(&self) -> String {
        match self.target {
            Target::Equipped => self.slot.label.to_owned(),
            Target::Parked(box_index) => {
                format!("{} inventory {box_index}", self.slot.label)
            }
        }
    }

    /// Where in the document this item sits, once it is known to be there.
    fn equipment_pointer(&self) -> String {
        pointer::equipped(self.character, self.slot)
    }
}

impl Target {
    fn from_box(box_index: usize) -> Self {
        if box_index == EQUIPPED_BOX {
            Self::Equipped
        } else {
            Self::Parked(box_index)
        }
    }

    /// Which box this is, which also keeps the two columns' widget ids apart.
    fn box_index(self) -> usize {
        match self {
            Self::Equipped => EQUIPPED_BOX,
            Self::Parked(box_index) => box_index,
        }
    }
}

pub struct Page<'a> {
    pub document: &'a mut Value,
    pub catalog: &'a Catalog,
    pub icons: &'a mut Icons,
    pub state: &'a mut LoadoutState,
}

impl Page<'_> {
    fn class_type(&self, character: usize) -> u64 {
        self.document
            .pointer(&format!("{}/class", pointer::character(character)))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    fn equipped(&self, character: usize, slot: &Slot) -> Option<&Value> {
        self.document.pointer(&pointer::equipped(character, slot))
    }

    fn equipped_hash(&self, character: usize, slot: &Slot) -> Option<u64> {
        self.equipped(character, slot)?
            .get("definition_hash")
            .and_then(parse_unsigned_value)
    }



    /// The item the editing column is pointed at, or `None` for an empty box.
    fn item_value(&self, editing: Editing) -> Option<&Value> {
        let pointer = self.item_pointer(editing)?;
        self.document.pointer(&pointer).filter(|item| !item.is_null())
    }

    fn item_value_mut(&mut self, editing: Editing) -> Option<&mut Value> {
        let pointer = self.item_pointer(editing)?;
        self.document.pointer_mut(&pointer)
    }

    fn item_pointer(&self, editing: Editing) -> Option<String> {
        let character = editing.character;
        match editing.target {
            Target::Equipped => Some(editing.equipment_pointer()),
            Target::Parked(box_index) => {
                let held = self.held_index(character, editing.slot, box_index)?;
                Some(pointer::held(character, held))
            }
        }
    }

    // ------------------------------------------------------------ gear rows

    pub fn draw_equipment(&mut self, ui: &mut egui::Ui, character: usize) -> Option<Change> {
        let mut change = None;

        ui.heading("Loadout");
        ui.horizontal_wrapped(|ui| {
            ui.label("Show plugs:");
            for filter in PlugFilter::ALL {
                ui.selectable_value(&mut self.state.filter, filter, filter.label())
                    .on_hover_text(filter.description());
            }
            ui.separator();
            ui.checkbox(&mut self.state.show_dummy_items, "Dummy items")
                .on_hover_text("Include display-only definitions that cannot normally be obtained.");
            ui.separator();
            // The roll draws from whatever list is on screen, so the filter
            // beside it chooses how far the gear strays.
            let filter = self.state.filter;
            let hover = format!(
                "Replace every slot with random gear: one equipped and nine held beside it, at most one exotic weapon. Plugs are rolled from the {} list — {}",
                filter.label().to_lowercase(),
                filter.description()
            );
            if ui.button("Randomize").on_hover_text(hover).clicked() {
                change = Some(self.randomize(character, filter));
            }
            ui.separator();
            ui.checkbox(&mut self.state.group_sockets, "Group sockets")
                .on_hover_text(
                    "Lay each piece out by what its sockets do: cosmetics on a row of their own ending in the shader, a weapon's intrinsic and masterwork split off from its perks, armor's energy and stats split off from its mods. Untick to keep plain socket order.",
                );
        });
        if self.state.filter.is_unsafe() {
            ui.colored_label(
                theme::WARNING,
                "Plugs outside a socket's own list may break the item, corrupt the loadout, or crash Destiny 2.",
            );
        }
        ui.add_space(8.0);

        for slot in SLOTS {
            if slot.is_subclass() {
                continue;
            }
            ui.push_id((character, slot.name), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(slot_change) = self.draw_slot(ui, character, slot) {
                        change = Some(slot_change);
                    }
                });
            });
            ui.add_space(4.0);
        }
        change
    }

    /// A slot's row: what is equipped and how it is set up on the left, the ten
    /// boxes the slot can hold on the right.
    fn draw_slot(
        &mut self,
        ui: &mut egui::Ui,
        character: usize,
        slot: &'static Slot,
    ) -> Option<Change> {
        let id = inventory_id(character, slot);
        let selected = if slot.holds_inventory { self.state.selection(id) } else { EQUIPPED_BOX };
        let target = Target::from_box(selected);
        let mut editor_change = None;
        let mut inventory_change = None;
        ui.horizontal_top(|ui| {
            let editor = editor_width(ui);
            let layout = egui::Layout::top_down(egui::Align::Min);
            let editing = Editing { character, slot, target };
            let left = ui.allocate_ui_with_layout(egui::vec2(editor, 0.0), layout, |ui| {
                ui.set_min_width(editor);
                editor_change = self.draw_item(ui, editing);
            });
            if !slot.holds_inventory {
                return;
            }
            let gap = ui.spacing().item_spacing.x;
            ui.add_space(gap);
            let split = ui.cursor().left();
            ui.add_space(gap);
            let right = ui.vertical(|ui| {
                inventory_change = self.draw_inventory(ui, character, slot, id, selected);
            });
            // Painted rather than allocated: a separator widget would take the
            // height the row could still grow to, which on the page's first row
            // is everything below it.
            let bottom = left.response.rect.bottom().max(right.response.rect.bottom());
            ui.painter().vline(
                split,
                left.response.rect.top()..=bottom,
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
        });
        inventory_change.or(editor_change)
    }

    /// The selected item and its sockets: the column that does the editing,
    /// whether what it points at is equipped or waiting in a box.
    fn draw_item(&mut self, ui: &mut egui::Ui, editing: Editing) -> Option<Change> {
        let Editing { slot, target, .. } = editing;
        let value = self.item_value(editing);
        let item_hash = value
            .and_then(|value| value.get("definition_hash"))
            .and_then(parse_unsigned_value);
        let held_plugs = value.and_then(|value| value.get("plugs")).cloned();
        let item = item_hash
            .and_then(|hash| self.catalog.get_for_bucket(hash, slot.bucket))
            .cloned();

        let heading = match target {
            Target::Equipped => slot.label.to_owned(),
            Target::Parked(box_index) => format!("{} · inventory {box_index}", slot.label),
        };
        let title = match (item_hash, &item) {
            (None, _) => "Empty".to_owned(),
            (Some(_), Some(item)) => item.name.clone(),
            (Some(hash), None) => format!("Unknown item {}", format_hash(hash)),
        };
        // The gear icon is the picker's only trigger, matching every socket.
        let mut level_change = None;
        let icon = ui
            .horizontal(|ui| {
                let icon = self
                    .icon_button(ui, item_hash, GEAR_ICON)
                    .on_hover_text(format!("{heading}: {title}\nClick to change"));
                hash_menu(&icon, item_hash);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&heading).strong().size(13.0));
                    ui.label(egui::RichText::new(&title).size(15.0));
                    if let Some(item) = &item {
                        let mut line = format!("{} · {}", item.type_name, format_hash(item.hash));
                        if let Some(generation) = self.catalog.armor_generation(item) {
                            line.push_str(" · ");
                            line.push_str(generation);
                        }
                        ui.label(egui::RichText::new(line).weak().small());
                    }
                    // Only weapons and armor carry a power level; a ship or a
                    // ghost has the field but nothing reads it.
                    if item_hash.is_some() && (slot.is_weapon() || slot.is_armor()) {
                        level_change = self.draw_level(ui, editing);
                    }
                });
                icon
            })
            .inner;
        let mut change = self.item_picker(ui, &icon, editing).or(level_change);

        let Some(item) = item else {
            return change;
        };
        let (plugs, native_defaults) =
            settings::displayed_plugs(held_plugs.as_ref(), &item.default_plugs);
        let socket_count = item.sockets.len().max(plugs.len());
        if socket_count == 0 {
            return change;
        }

        let lines = self.socket_lines(&item, slot, socket_count);

        ui.add_space(4.0);
        for line in &lines {
            ui.horizontal(|ui| {
                // The pinned column keeps its width on every line, so the rows
                // all start at one place whether or not anything is pinned
                // beside them.
                // The cell is exactly as tall as a socket button, so a pin
                // sits at the same height as the row beside it and every line
                // is the same height whether or not it pins anything.
                let cell = egui::vec2(pin_column_width(ui), icon_height(ui, SOCKET_ICON));
                let pinned = ui.allocate_ui_with_layout(
                    cell,
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(cell);
                        let socket_index = line.pinned?;
                        let current = plugs.get(socket_index).and_then(parse_unsigned_value);
                        self.draw_socket(ui, editing, &item, socket_index, current)
                    },
                );
                if let Some(pin_change) = pinned.inner {
                    change = Some(pin_change);
                }
                divider(ui, icon_height(ui, SOCKET_ICON));
                for socket_index in line.row.iter().copied() {
                    let current = plugs.get(socket_index).and_then(parse_unsigned_value);
                    if let Some(socket_change) =
                        self.draw_socket(ui, editing, &item, socket_index, current)
                    {
                        change = Some(socket_change);
                    }
                }
            });
        }
        if native_defaults {
            ui.label(egui::RichText::new("package defaults").weak().small())
                .on_hover_text("This item has no authored plug list yet, so its defaults are shown.");
        }
        change
    }

    // ----------------------------------------------------------- inventory


    // ---------------------------------------------------------- randomizer




    fn item_name(&self, hash: u64) -> String {
        self.catalog
            .get(hash)
            .map_or_else(|| format!("Unknown item {}", format_hash(hash)), |item| item.name.clone())
    }


    // -------------------------------------------------------- socket layout




    /// The item's level, which Sunrise stores per equipped item and which the
    /// game reads as its power. 106 is what this build's characters ship with.
    fn draw_level(&mut self, ui: &mut egui::Ui, editing: Editing) -> Option<Change> {
        let stored = self
            .item_value(editing)
            .and_then(|item| item.get("level"))
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_ITEM_LEVEL);
        let mut level = stored;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Level").weak().small());
            ui.add(
                egui::DragValue::new(&mut level)
                    .speed(1.0)
                    .range(0..=i64::from(i32::MAX)),
            )
            .on_hover_text("The item's power level. Sunrise ships characters at 106.");
        });
        if level == stored {
            return None;
        }
        let item = self.item_value_mut(editing)?;
        item.as_object_mut()?.insert("level".into(), Value::from(level));
        Some(Ok(format!("Set {} to level {level}", editing.label())))
    }

    /// One socket: an icon button that opens its searchable plug list.
    fn draw_socket(
        &mut self,
        ui: &mut egui::Ui,
        editing: Editing,
        item: &ItemDef,
        socket_index: usize,
        current: Option<u64>,
    ) -> Option<Change> {
        let Editing { character, slot, target, .. } = editing;
        let id =
            ui.make_persistent_id(("socket", character, slot.name, target.box_index(), socket_index));
        let button = self
            .icon_button(ui, current, SOCKET_ICON)
            .on_hover_text(self.socket_hover(socket_index, current));
        hash_menu(&button, current);
        if button.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }

        // Building the list is only worth it while the menu is actually open.
        let options = if ui.memory(|memory| memory.is_popup_open(id)) {
            self.catalog.plug_options(item, socket_index, self.state.filter)
        } else {
            Vec::new()
        };
        let picker = Picker {
            id,
            options: &options,
            current,
            allow_empty: true,
            icon: PLUG_ROW_ICON,
        };
        let selection = self.picker_popup(ui, &button, picker, |catalog, hash| {
            (catalog.plug_name(hash).to_owned(), format_hash(hash))
        })?;
        ui.memory_mut(egui::Memory::close_popup);

        let socket = format!("{} socket {}", editing.label(), socket_index + 1);
        let installed = match self.item_value_mut(editing) {
            Some(value) => {
                settings::set_item_plug(value, socket_index, &item.default_plugs, selection)
            }
            None => Err(format!("{socket} is not there to change")),
        };
        Some(installed.map(|()| match selection {
            Some(hash) => format!("Installed {} in {socket}", self.catalog.plug_name(hash)),
            None => format!("Cleared {socket}"),
        }))
    }

    fn socket_hover(&self, socket_index: usize, current: Option<u64>) -> String {
        let Some(hash) = current else {
            return format!("Socket {}\nEmpty", socket_index + 1);
        };
        let mut hover = format!("Socket {}\n{}", socket_index + 1, self.catalog.plug_label(hash));
        if let Some(source) = self.catalog.source(hash) {
            hover.push('\n');
            hover.push_str(source);
        }
        if let Some(description) = self.catalog.description(hash) {
            hover.push_str("\n\n");
            hover.push_str(description);
        }
        hover
    }




    // ------------------------------------------------------ character fields

}
