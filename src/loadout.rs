//! The loadout page: one row per gear piece, sockets laid out left to right,
//! each socket a button showing the icon of whatever is plugged into it.

use std::collections::HashMap;

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{AbilityChoice, AbilityOptions, Catalog, CosmeticKind, ItemDef, PlugFilter},
    icons::{Fallback, Icons},
    model::{
        ARMOR_SLOTS, SLOTS, SUBCLASS_BUCKET, WEAPON_SLOTS, class_name, format_hash,
        parse_unsigned_value, slot_label,
    },
    settings,
};

const SOCKET_ICON: f32 = 44.0;
/// Plug rows show the same icon a socket does, so a plug looks identical
/// whether it is installed or being chosen.
const PLUG_ROW_ICON: f32 = SOCKET_ICON;
/// Gear reads at half again the size of a plug, on its loadout row and in its
/// own picker. Gear that appears *as* a plug — an ornament, a shader — stays
/// plug-sized, because the size belongs to the menu, not to the item.
const GEAR_ICON: f32 = SOCKET_ICON * 1.5;
const NAME_FONT: f32 = 16.0;
const DETAIL_FONT: f32 = 13.0;
const PICKER_WIDTH: f32 = 580.0;
const PICKER_HEIGHT: f32 = 470.0;
const MAX_PICKER_ROWS: usize = 300;
/// A slot holds ten items in Destiny: the equipped one, and a 3x3 matrix of
/// the rest. Box 0 is the equipped one, the only box this build writes to.
const EQUIPPED_BOX: usize = 0;
const INVENTORY_ROWS: usize = 3;
const INVENTORY_COLUMNS: usize = 3;
const INVENTORY_CELL: f32 = GEAR_ICON;

/// A row holds five sockets. A pinned socket sits beside one in the first
/// column rather than in it, so a line can still run six across.
const MAX_ROW_WIDTH: usize = 5;
/// Three lines is as tall as a piece gets.
const MAX_ROWS: usize = 3;
/// What Sunrise ships characters at in this build.
const DEFAULT_ITEM_LEVEL: i64 = 106;

const CLASSES: &[(u64, &str)] = &[(0, "Titan"), (1, "Hunter"), (2, "Warlock")];
const RACES: &[(u64, &str)] = &[(0, "Human"), (1, "Awoken"), (2, "Exo")];
const GENDERS: &[(u64, &str)] = &[(0, "Male"), (1, "Female")];

/// UI-only state for the loadout page.
pub struct LoadoutState {
    pub filter: PlugFilter,
    pub show_dummy_items: bool,
    pub group_sockets: bool,
    searches: HashMap<egui::Id, String>,
    /// Which of a slot's ten inventory boxes each row has selected. Absent
    /// means the equipped one, which is what every row starts on.
    selections: HashMap<egui::Id, usize>,
    /// Items parked in the inventory boxes, keyed by the row and the box.
    /// Nothing here reaches the document until it is swapped in.
    parked: HashMap<(egui::Id, usize), Value>,
    /// Whether the last change touched the document. Editing a parked item
    /// changes nothing on disk, so it must not mark the file unsaved.
    pub edited_document: bool,
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
            parked: HashMap::new(),
            edited_document: true,
        }
    }
}

impl LoadoutState {
    /// Drops per-slot search text, and the inventory, when a different file is
    /// opened: parked items belong to the loadout they came out of.
    pub fn clear_pickers(&mut self) {
        self.searches.clear();
        self.selections.clear();
        self.parked.clear();
    }

    fn selection(&self, id: egui::Id) -> usize {
        self.selections.get(&id).copied().unwrap_or(EQUIPPED_BOX)
    }

    fn parked_hash(&self, id: egui::Id, box_index: usize) -> Option<u64> {
        self.parked
            .get(&(id, box_index))?
            .get("definition_hash")
            .and_then(parse_unsigned_value)
    }
}

/// What a page did to the document: a status line on success, a reason on
/// failure. `None` means nothing was touched this frame.
pub type Change = Result<String, String>;

/// Everything one picker needs to know about what it is choosing from.
struct Picker<'a> {
    id: egui::Id,
    options: &'a [u64],
    current: Option<u64>,
    allow_empty: bool,
    icon: f32,
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
/// which of that row's ten boxes.
#[derive(Clone, Copy)]
struct Editing<'a> {
    character: usize,
    slot: &'a str,
    row: egui::Id,
    target: Target,
}

impl Editing<'_> {
    /// What a status line calls the thing being edited.
    fn label(&self) -> String {
        match self.target {
            Target::Equipped => slot_label(self.slot).to_owned(),
            Target::Parked(box_index) => {
                format!("{} inventory {box_index}", slot_label(self.slot))
            }
        }
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

/// The sets a row is built from. Order here is the order along the row, and a
/// divider marks every change of group.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RowGroup {
    Perk,
    Mod,
    Stat,
    /// Sockets with nothing to choose from, and the Red War damage mods.
    Spare,
    Cosmetic,
}

impl RowGroup {
    /// What a socket sorts by. Heavier goes further right, so the cosmetics
    /// trail the row and the shader ends it.
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

/// One line of a piece's sockets. Segments are drawn left to right with a
/// separator between them, and an indented row starts under the second socket
/// of the first row rather than under the first.
struct SocketRow {
    indented: bool,
    segments: Vec<Vec<usize>>,
}

impl SocketRow {
    fn new(segments: impl IntoIterator<Item = Vec<usize>>) -> Self {
        Self {
            indented: false,
            segments: segments.into_iter().filter(|segment| !segment.is_empty()).collect(),
        }
    }

    fn indented(self) -> Self {
        Self { indented: true, ..self }
    }

    fn is_empty(&self) -> bool {
        self.segments.is_empty()
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
            .pointer(&format!("/state/characters/{character}/class"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    fn equipped(&self, character: usize, slot: &str) -> Option<&Value> {
        self.document
            .pointer(&format!("/state/characters/{character}/equipment/{slot}"))
    }

    fn equipped_hash(&self, character: usize, slot: &str) -> Option<u64> {
        self.equipped(character, slot)?
            .get("definition_hash")
            .and_then(parse_unsigned_value)
    }

    /// The item the editing column is pointed at, or `None` for an empty box.
    fn item_value(&self, editing: Editing<'_>) -> Option<&Value> {
        match editing.target {
            Target::Equipped => self
                .equipped(editing.character, editing.slot)
                .filter(|item| !item.is_null()),
            Target::Parked(box_index) => self.state.parked.get(&(editing.row, box_index)),
        }
    }

    fn item_value_mut(&mut self, editing: Editing<'_>) -> Option<&mut Value> {
        match editing.target {
            Target::Equipped => {
                let (character, slot) = (editing.character, editing.slot);
                self.document
                    .pointer_mut(&format!("/state/characters/{character}/equipment/{slot}"))
            }
            Target::Parked(box_index) => self.state.parked.get_mut(&(editing.row, box_index)),
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
            if ui
                .button("Randomize")
                .on_hover_text(
                    "Replace every slot with random gear: one equipped and nine held beside it, each rolled with random plugs. At most one exotic weapon is equipped.",
                )
                .clicked()
            {
                change = Some(self.randomize(character));
            }
            ui.separator();
            ui.checkbox(&mut self.state.group_sockets, "Group sockets")
                .on_hover_text(
                    "Lay each piece out by what its sockets do: cosmetics on a row of their own ending in the shader, a weapon's intrinsic and masterwork split off from its perks, armor's energy and stats split off from its mods. Untick to keep plain socket order.",
                );
        });
        if self.state.filter.is_unsafe() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 190, 80),
                "Plugs outside a socket's own list may break the item, corrupt the loadout, or crash Destiny 2.",
            );
        }
        ui.add_space(8.0);

        for &(slot, label, bucket) in SLOTS {
            if bucket == SUBCLASS_BUCKET {
                continue;
            }
            ui.push_id((character, slot), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(slot_change) = self.draw_slot(ui, character, slot, label, bucket) {
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
        slot: &str,
        label: &str,
        bucket: u64,
    ) -> Option<Change> {
        let id = inventory_id(character, slot);
        let selected = if holds_inventory(slot) { self.state.selection(id) } else { EQUIPPED_BOX };
        let target = Target::from_box(selected);
        let mut editor_change = None;
        let mut inventory_change = None;
        ui.horizontal_top(|ui| {
            let editor = editor_width(ui);
            let layout = egui::Layout::top_down(egui::Align::Min);
            let editing = Editing { character, slot, row: id, target };
            let left = ui.allocate_ui_with_layout(egui::vec2(editor, 0.0), layout, |ui| {
                ui.set_min_width(editor);
                editor_change = self.draw_item(ui, editing, label, bucket);
            });
            if !holds_inventory(slot) {
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
        // Editing a parked item leaves the file alone; everything else writes.
        if editor_change.is_some() {
            self.state.edited_document = target == Target::Equipped;
        }
        if inventory_change.is_some() {
            self.state.edited_document = true;
        }
        inventory_change.or(editor_change)
    }

    /// The selected item and its sockets: the column that does the editing,
    /// whether what it points at is equipped or waiting in a box.
    fn draw_item(
        &mut self,
        ui: &mut egui::Ui,
        editing: Editing<'_>,
        label: &str,
        bucket: u64,
    ) -> Option<Change> {
        let Editing { slot, target, .. } = editing;
        let value = self.item_value(editing);
        let item_hash = value
            .and_then(|value| value.get("definition_hash"))
            .and_then(parse_unsigned_value);
        let held_plugs = value.and_then(|value| value.get("plugs")).cloned();
        let item = item_hash
            .and_then(|hash| self.catalog.get_for_bucket(hash, bucket))
            .cloned();

        let heading = match target {
            Target::Equipped => label.to_owned(),
            Target::Parked(box_index) => format!("{label} · inventory {box_index}"),
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
                    if item_hash.is_some()
                        && (WEAPON_SLOTS.contains(&slot) || ARMOR_SLOTS.contains(&slot))
                    {
                        level_change = self.draw_level(ui, editing);
                    }
                });
                icon
            })
            .inner;
        let mut change = self.item_picker(ui, &icon, editing, bucket).or(level_change);

        let Some(item) = item else {
            return change;
        };
        let (plugs, native_defaults) =
            settings::displayed_plugs(held_plugs.as_ref(), &item.default_plugs);
        let socket_count = item.sockets.len().max(plugs.len());
        if socket_count == 0 {
            return change;
        }

        let rows = self.socket_rows(&item, slot, socket_count);
        let last_row = rows.len().saturating_sub(1);

        ui.add_space(4.0);
        // Where the first row's second socket starts, so that an indented row
        // lines up with it rather than with the socket leading the piece.
        let mut indent = 0.0;
        for (row_index, row) in rows.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                let left = ui.cursor().left();
                if row.indented {
                    ui.add_space(indent);
                }
                let mut column = 0;
                for (position, segment) in row.segments.iter().enumerate() {
                    if position > 0 {
                        divider(ui, icon_height(ui, SOCKET_ICON));
                    }
                    for socket_index in segment.iter().copied() {
                        if row_index == 0 && column == 1 {
                            indent = ui.cursor().left() - left;
                        }
                        let current = plugs.get(socket_index).and_then(parse_unsigned_value);
                        if let Some(socket_change) =
                            self.draw_socket(ui, editing, &item, socket_index, current)
                        {
                            change = Some(socket_change);
                        }
                        column += 1;
                    }
                }
                if row_index == last_row && native_defaults {
                    ui.label(egui::RichText::new("package defaults").weak().small())
                        .on_hover_text(
                            "This item has no authored plug list yet, so its defaults are shown.",
                        );
                }
            });
        }
        change
    }

    // ----------------------------------------------------------- inventory

    /// The ten boxes a slot can hold: the equipped one, a divider, then the
    /// 3x3 matrix of the rest. Only the equipped box has anything in it — the
    /// other nine are picked up by a later build.
    fn draw_inventory(
        &mut self,
        ui: &mut egui::Ui,
        character: usize,
        slot: &str,
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
                                let hash = self.state.parked_hash(id, box_index);
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

    // ---------------------------------------------------------- randomizer

    /// Fills every slot with random gear: one equipped, nine held beside it,
    /// each with a random plug rolled into every socket that offers any.
    fn randomize(&mut self, character: usize) -> Change {
        let mut rng = Rng::from_clock();
        let class = self.class_type(character);
        let boxes = INVENTORY_ROWS * INVENTORY_COLUMNS;
        let mut exotic_equipped = false;
        let mut rolled = 0;
        let mut slots = 0;
        for &(slot, _, bucket) in SLOTS {
            if bucket == SUBCLASS_BUCKET {
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
            let last_box = if holds_inventory(slot) { boxes } else { EQUIPPED_BOX };
            for box_index in EQUIPPED_BOX..=last_box {
                // One exotic in hand is plenty; the held boxes are free.
                let equipped_weapon = box_index == EQUIPPED_BOX && WEAPON_SLOTS.contains(&slot);
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
                let editing = Editing {
                    character,
                    slot,
                    row: inventory_id(character, slot),
                    target: Target::from_box(box_index),
                };
                self.install_random(editing, &item, &mut rng)?;
                rolled += 1;
            }
            self.state.selections.insert(inventory_id(character, slot), EQUIPPED_BOX);
        }
        Ok(format!("Rolled {rolled} items across {slots} slots"))
    }

    /// Puts one rolled item where the editing column would have put it, with a
    /// random plug in every socket the catalog offers plugs for.
    fn install_random(
        &mut self,
        editing: Editing<'_>,
        item: &ItemDef,
        rng: &mut Rng,
    ) -> Result<(), String> {
        match editing.target {
            Target::Equipped => settings::equip_definition(
                self.document,
                editing.character,
                editing.slot,
                item.hash,
                &item.default_plugs,
            )?,
            Target::Parked(box_index) => {
                let level = settings::inferred_item_level(self.document, editing.character);
                let held = self.state.parked.entry((editing.row, box_index)).or_insert(Value::Null);
                settings::set_item_definition(held, item.hash, &item.default_plugs, level)?;
            }
        }

        let rolls: Vec<(usize, u64)> = (0..item.sockets.len())
            .filter_map(|socket| {
                let options = self.catalog.plug_options(item, socket, PlugFilter::Compatible);
                rng.pick(&options).map(|hash| (socket, *hash))
            })
            .collect();
        let Some(value) = self.item_value_mut(editing) else {
            return Ok(());
        };
        for (socket, hash) in rolls {
            settings::set_item_plug(value, socket, &item.default_plugs, Some(hash))?;
        }
        Ok(())
    }

    /// Swaps the selected box with the equipped item. Emptying the equipped
    /// box only works on a weapon slot, which is where the error comes from.
    fn equip_parked(
        &mut self,
        character: usize,
        slot: &str,
        id: egui::Id,
        box_index: usize,
    ) -> Change {
        let incoming = self.state.parked.remove(&(id, box_index));
        let incoming_name = incoming
            .as_ref()
            .and_then(|item| item.get("definition_hash"))
            .and_then(parse_unsigned_value)
            .map(|hash| self.item_name(hash));
        match settings::swap_equipped(self.document, character, slot, incoming.clone()) {
            Ok(outgoing) => {
                let outgoing_name = outgoing
                    .as_ref()
                    .and_then(|item| item.get("definition_hash"))
                    .and_then(parse_unsigned_value)
                    .map(|hash| self.item_name(hash));
                if let Some(outgoing) = outgoing {
                    self.state.parked.insert((id, box_index), outgoing);
                }
                self.state.selections.insert(id, EQUIPPED_BOX);
                Ok(match (incoming_name, outgoing_name) {
                    (Some(equipped), Some(parked)) => {
                        format!("Equipped {equipped}, holding {parked} in inventory {box_index}")
                    }
                    (Some(equipped), None) => {
                        format!("Equipped {equipped} from inventory {box_index}")
                    }
                    (None, Some(parked)) => format!(
                        "Moved {parked} to inventory {box_index}, emptying the {}",
                        slot_label(slot)
                    ),
                    (None, None) => format!("Inventory {box_index} is empty"),
                })
            }
            Err(error) => {
                // The swap never happened, so the box keeps what it held.
                if let Some(item) = incoming {
                    self.state.parked.insert((id, box_index), item);
                }
                Err(error)
            }
        }
    }

    fn item_name(&self, hash: u64) -> String {
        self.catalog
            .get(hash)
            .map_or_else(|| format!("Unknown item {}", format_hash(hash)), |item| item.name.clone())
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

    // -------------------------------------------------------- socket layout

    /// How a piece's sockets fall into lines. A line is a pinned socket in the
    /// first column and a row of everything else beside it; a pinned socket is
    /// what a piece is built around rather than one of a set, so it keeps its
    /// column whether or not the row beside it has anything in it.
    fn socket_rows(&self, item: &ItemDef, slot: &str, socket_count: usize) -> Vec<SocketRow> {
        if !self.state.group_sockets {
            return ungrouped_rows(&(0..socket_count).collect::<Vec<_>>());
        }

        let pins = self.pinned_sockets(item, slot, socket_count);
        let mut loose: Vec<(u16, RowGroup, usize)> = (0..socket_count)
            .filter(|socket| !pins.contains(&Some(*socket)))
            .map(|socket| {
                let group = self.socket_group(item, slot, socket);
                (group.weight(self.catalog.cosmetic_kind(item, socket)), group, socket)
            })
            .collect();
        // Lightest first, and a socket's own number breaks a tie. Weights are
        // ordered by group, so each group comes out in one piece.
        loose.sort_unstable();
        let rows = pack_rows(&loose);

        let lines = rows.len().max(pins.iter().rposition(Option::is_some).map_or(0, |at| at + 1));
        (0..lines)
            .map(|line| {
                let pin = pins.get(line).copied().flatten();
                let segments = pin
                    .map(|socket| vec![socket])
                    .into_iter()
                    .chain(rows.get(line).cloned().unwrap_or_default());
                let row = SocketRow::new(segments);
                // A row without a pin beside it leaves that column clear.
                if pin.is_some() { row } else { row.indented() }
            })
            .filter(|row| !row.is_empty())
            .collect()
    }

    /// The sockets that hold the first column, by line: a weapon's frame above
    /// its masterwork, or the energy socket armor is built around — the
    /// archetype, on a Year-1 piece that has no energy socket.
    fn pinned_sockets(
        &self,
        item: &ItemDef,
        slot: &str,
        socket_count: usize,
    ) -> Vec<Option<usize>> {
        let find = |test: &dyn Fn(usize) -> bool| (0..socket_count).find(|socket| test(*socket));
        let mut pins = vec![None; MAX_ROWS];
        // The masterwork holds the second line, so it is claimed first and the
        // first line looks past it.
        if WEAPON_SLOTS.contains(&slot) {
            pins[1] = find(&|socket| self.catalog.is_masterwork_socket(item, socket));
        }
        let taken = pins[1];
        let free = |socket: usize| Some(socket) != taken;
        // Every piece pins something, so the first column always means the same
        // thing. The energy socket decides what an Armor 2.0 piece can hold, so
        // it outranks the trait beside it, and armor with none of those leads
        // with its masterwork. Failing all of that comes the first socket that
        // is neither spare, stat, nor cosmetic — none of which is what a piece
        // is built around — and failing even that, the first socket of all: a
        // piece can be nothing but empty sockets.
        pins[0] = find(&|socket| free(socket) && self.catalog.is_energy_socket(item, socket))
            .or_else(|| find(&|socket| free(socket) && self.catalog.is_intrinsic_socket(item, socket)))
            .or_else(|| find(&|socket| free(socket) && self.catalog.is_anchor_socket(item, socket)))
            .or_else(|| find(&|socket| free(socket) && self.catalog.is_masterwork_socket(item, socket)))
            .or_else(|| {
                find(&|socket| {
                    free(socket)
                        && self.catalog.cosmetic_kind(item, socket).is_none()
                        && !self.catalog.is_secondary_socket(item, socket)
                        && !self.catalog.is_stat_socket(item, socket)
                })
            })
            .or_else(|| find(&free));
        pins
    }

    /// Which set a socket belongs to. Sockets of one group sit together, and a
    /// divider separates them from the next group along the row.
    fn socket_group(&self, item: &ItemDef, slot: &str, socket: usize) -> RowGroup {
        if self.catalog.cosmetic_kind(item, socket).is_some() {
            RowGroup::Cosmetic
        } else if self.catalog.is_stat_socket(item, socket) {
            RowGroup::Stat
        } else if self.catalog.is_secondary_socket(item, socket) {
            RowGroup::Spare
        } else if ARMOR_SLOTS.contains(&slot) || self.catalog.is_mod_socket(item, socket) {
            RowGroup::Mod
        } else {
            RowGroup::Perk
        }
    }

    /// The item's level, which Sunrise stores per equipped item and which the
    /// game reads as its power. 106 is what this build's characters ship with.
    fn draw_level(&mut self, ui: &mut egui::Ui, editing: Editing<'_>) -> Option<Change> {
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
        editing: Editing<'_>,
        item: &ItemDef,
        socket_index: usize,
        current: Option<u64>,
    ) -> Option<Change> {
        let Editing { character, slot, target, .. } = editing;
        let id =
            ui.make_persistent_id(("socket", character, slot, target.box_index(), socket_index));
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

    /// The item picker, opened by a gear piece's icon.
    fn item_picker(
        &mut self,
        ui: &mut egui::Ui,
        anchor: &egui::Response,
        editing: Editing<'_>,
        bucket: u64,
    ) -> Option<Change> {
        let Editing { character, slot, row, target } = editing;
        let id = ui.make_persistent_id(("item", character, slot, target.box_index()));
        if anchor.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }

        let options: Vec<u64> = self
            .catalog
            .items_for_slot(slot, self.class_type(character), self.state.show_dummy_items)
            .map(|item| item.hash)
            .collect();
        let picker = Picker {
            id,
            options: &options,
            current: self
                .item_value(editing)
                .and_then(|item| item.get("definition_hash"))
                .and_then(parse_unsigned_value),
            // Clearing a box only empties the box; clearing the slot itself has
            // to leave the document valid, which only a weapon slot can.
            allow_empty: target != Target::Equipped || WEAPON_SLOTS.contains(&slot),
            icon: GEAR_ICON,
        };
        let selection = self.picker_popup(ui, anchor, picker, |catalog, hash| {
            catalog.get(hash).map_or_else(
                || (format!("Unknown item {}", format_hash(hash)), String::new()),
                |item| {
                    let detail = match catalog.armor_generation(item) {
                        Some(generation) => format!("{} · {generation}", item.type_name),
                        None => item.type_name.clone(),
                    };
                    (item.name.clone(), detail)
                },
            )
        })?;
        ui.memory_mut(egui::Memory::close_popup);

        let Some(hash) = selection else {
            return Some(match target {
                Target::Equipped => settings::set_weapon_slot_empty(self.document, character, slot)
                    .map(|()| format!("Emptied the {} slot", slot_label(slot))),
                Target::Parked(box_index) => {
                    self.state.parked.remove(&(row, box_index));
                    Ok(format!("Emptied {}", editing.label()))
                }
            });
        };
        let Some(item) = self.catalog.get_for_bucket(hash, bucket).cloned() else {
            return Some(Err("That item is not valid for this slot".to_owned()));
        };
        Some(match target {
            Target::Equipped => settings::equip_definition(
                self.document,
                character,
                slot,
                item.hash,
                &item.default_plugs,
            )
            .map(|()| format!("Equipped {}", item.name)),
            Target::Parked(box_index) => {
                let level = settings::inferred_item_level(self.document, character);
                let held = self.state.parked.entry((row, box_index)).or_insert(Value::Null);
                settings::set_item_definition(held, item.hash, &item.default_plugs, level)
                    .map(|()| format!("Put {} in inventory {box_index}", item.name))
            }
        })
    }

    /// The shared hover menu: a search box above an icon list. Returns
    /// `Some(selection)` on the frame a row is clicked, where the inner `None`
    /// means the "empty" row.
    fn picker_popup(
        &mut self,
        ui: &mut egui::Ui,
        anchor: &egui::Response,
        picker: Picker<'_>,
        describe: impl Fn(&Catalog, u64) -> (String, String),
    ) -> Option<Option<u64>> {
        let Picker {
            id,
            options,
            current,
            allow_empty,
            icon,
        } = picker;
        let mut query = self.state.searches.get(&id).cloned().unwrap_or_default();
        let mut selection = None;
        let mut cleared = false;

        egui::popup::popup_below_widget(
            ui,
            id,
            anchor,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(PICKER_WIDTH);
                ui.set_max_width(PICKER_WIDTH);
                let search = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("Search by name, description, or 0x hash…")
                        .font(egui::FontId::proportional(NAME_FONT))
                        .desired_width(PICKER_WIDTH - 16.0),
                );
                if ui.memory(|memory| memory.focused().is_none()) {
                    search.request_focus();
                }
                ui.separator();

                // Every word typed has to appear somewhere in the row: its
                // name, its type or hash, or its description. That makes
                // "accuracy" find the perks that mention it, and "reload
                // precision" narrow to the perks that mention both.
                let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
                let mut matches: Vec<(u64, String, String)> = options
                    .iter()
                    .filter(|hash| current != Some(**hash))
                    .map(|hash| {
                        let (name, detail) = describe(self.catalog, *hash);
                        (*hash, name, detail)
                    })
                    .filter(|(hash, name, detail)| {
                        terms.is_empty() || {
                            let haystack = format!(
                                "{name} {detail} {} {} {}",
                                format_hash(*hash),
                                self.catalog.source(*hash).unwrap_or_default(),
                                self.catalog.description(*hash).unwrap_or_default()
                            )
                            .to_lowercase();
                            terms.iter().all(|term| haystack.contains(term))
                        }
                    })
                    .collect();
                // A name hit is what the search was most likely aiming for, so
                // those come before rows that only matched on description.
                matches.sort_by_key(|(_, name, _)| {
                    let name = name.to_lowercase();
                    !terms.iter().all(|term| name.contains(term))
                });

                egui::ScrollArea::vertical()
                    .min_scrolled_height(PICKER_HEIGHT)
                    .max_height(PICKER_HEIGHT)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // What is installed stays pinned to the top, whatever
                        // the filter or the search is showing.
                        if let Some(hash) = current {
                            let (name, _) = describe(self.catalog, hash);
                            let name = format!("{name}  (installed)");
                            if row(ui, self.icons, self.catalog, icon, Some(hash), &name, "", true) {
                                selection = Some(hash);
                            }
                        }
                        if allow_empty
                            && row(
                                ui,
                                self.icons,
                                self.catalog,
                                icon,
                                None,
                                "Empty",
                                "",
                                current.is_none(),
                            )
                        {
                            cleared = true;
                        }
                        if current.is_some() || allow_empty {
                            ui.separator();
                        }
                        for (hash, name, detail) in matches.iter().take(MAX_PICKER_ROWS) {
                            if row(ui, self.icons, self.catalog, icon, Some(*hash), name, detail, false)
                            {
                                selection = Some(*hash);
                            }
                        }
                        match matches.len() {
                            0 => {
                                ui.label(egui::RichText::new("Nothing matches this search").weak());
                            }
                            shown if shown > MAX_PICKER_ROWS => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "…and {} more. Keep typing to narrow the list.",
                                        shown - MAX_PICKER_ROWS
                                    ))
                                    .weak()
                                    .small(),
                                );
                            }
                            _ => {}
                        }
                    });
            },
        );

        self.state.searches.insert(id, query);
        match (selection, cleared) {
            (Some(hash), _) => Some(Some(hash)),
            (None, true) => Some(None),
            (None, false) => None,
        }
    }

    fn icon_button(&mut self, ui: &mut egui::Ui, hash: Option<u64>, size: f32) -> egui::Response {
        let texture = match hash {
            Some(hash) if self.catalog.is_stat_plug(hash) => Some(self.icons.stat_plug(ui.ctx())),
            Some(hash) => self.icons.get(ui.ctx(), hash, fallback(self.catalog, hash)),
            None => Some(self.icons.empty(ui.ctx())),
        };
        match texture {
            Some(texture) => ui.add(
                egui::ImageButton::new((texture.id(), egui::vec2(size, size))).corner_radius(3),
            ),
            // Every socket resolves to art, the stand-in, or the empty plate, so
            // a bare button here only means a load deferred to the next frame.
            None => ui.add_sized([size + 8.0, size + 8.0], egui::Button::new("")),
        }
    }

    // ------------------------------------------------------ character fields

    pub fn draw_character_fields(&mut self, ui: &mut egui::Ui, character: usize) -> Option<Change> {
        let path = format!("/state/characters/{character}");
        let object = self.document.pointer(&path).and_then(Value::as_object)?;
        let read = |key: &str, fallback: u64| object.get(key).and_then(Value::as_u64).unwrap_or(fallback);
        let original_class = read("class", 0);
        let (mut class_type, mut race, mut gender) =
            (original_class, read("race", 0), read("gender", 0));
        let mut movement = read("movement_ability", 4);
        let mut grenade = read("grenade_ability", 7);
        let mut super_ability = read("super_ability", 10);
        let mut melee = read("melee_ability", 11);
        let mut class_ability = read("class_ability", 2);
        let soid = object
            .get("soid")
            .and_then(parse_unsigned_value)
            .map_or_else(|| "Unknown".to_owned(), format_hash);
        let ability_warning = settings::character_ability_issue(object);

        let mut subclass_hash = self
            .document
            .pointer(&format!("{path}/equipment/subclass/definition_hash"))
            .and_then(parse_unsigned_value);
        let subclasses: Vec<ItemDef> = self
            .catalog
            .items
            .iter()
            .filter(|item| item.bucket_hash == SUBCLASS_BUCKET && item.class_type == class_type)
            .cloned()
            .collect();
        let mut abilities = subclass_hash
            .and_then(|hash| self.catalog.get_for_bucket(hash, SUBCLASS_BUCKET))
            .map(|item| item.abilities.clone())
            .unwrap_or_default();
        let mut attunement = attunement_index(&abilities, super_ability, melee);
        let mut equip_subclass = None::<ItemDef>;

        ui.horizontal(|ui| {
            ui.heading(format!("Character {}", character + 1));
            ui.label(egui::RichText::new(soid).monospace().weak());
        });
        if let Some(warning) = &ability_warning {
            ui.colored_label(
                egui::Color32::from_rgb(255, 190, 80),
                format!("Warning: {warning}. Choose supported abilities below and save before launching."),
            );
        }
        ui.add_space(6.0);

        egui::Grid::new(("character", character))
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                for (label, id, value, choices) in [
                    ("Class", "class", &mut class_type, CLASSES),
                    ("Race", "race", &mut race, RACES),
                    ("Gender", "gender", &mut gender, GENDERS),
                ] {
                    ui.label(label);
                    combo(ui, id, value, choices);
                    ui.end_row();
                }

                ui.label("Subclass");
                let selected = subclass_hash
                    .and_then(|hash| subclasses.iter().find(|item| item.hash == hash))
                    .map_or("Unknown subclass", |item| item.name.as_str());
                egui::ComboBox::from_id_salt("subclass")
                    .selected_text(selected)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for subclass in &subclasses {
                            if ui
                                .selectable_label(subclass_hash == Some(subclass.hash), &subclass.name)
                                .clicked()
                                && subclass_hash != Some(subclass.hash)
                            {
                                subclass_hash = Some(subclass.hash);
                                abilities = subclass.abilities.clone();
                                (movement, grenade, super_ability, melee, class_ability) =
                                    default_abilities(class_type, &abilities);
                                attunement = attunement_index(&abilities, super_ability, melee);
                                equip_subclass = Some(subclass.clone());
                            }
                        }
                    });
                ui.end_row();

                ui.label("Attunement");
                let previous = attunement;
                let selected = abilities
                    .attunements
                    .get(attunement)
                    .map_or("No attunement data", |choice| choice.name.as_str());
                egui::ComboBox::from_id_salt("attunement")
                    .selected_text(selected)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (index, choice) in abilities.attunements.iter().enumerate() {
                            ui.selectable_value(&mut attunement, index, &choice.name);
                        }
                    });
                ui.end_row();

                // The super and melee entries must stay inside one attunement.
                if let Some(choice) = abilities.attunements.get(attunement) {
                    let pair_is_valid = choice.melee.entry == melee
                        && choice.super_abilities.iter().any(|s| s.entry == super_ability);
                    if attunement != previous || !pair_is_valid {
                        melee = choice.melee.entry;
                        super_ability = choice.super_abilities.first().map_or(10, |s| s.entry);
                    }
                    ui.label("Attunement perks");
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                choice
                                    .perks
                                    .iter()
                                    .map(|perk| perk.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" • "),
                            )
                            .weak(),
                        )
                        .wrap(),
                    );
                    ui.end_row();
                }

                ui.label("Movement ability");
                ability_combo(ui, "movement", &mut movement, &abilities.movement);
                ui.end_row();
                ui.label("Grenade ability");
                ability_combo(ui, "grenade", &mut grenade, &abilities.grenade);
                ui.end_row();
                if let Some(choice) = abilities.attunements.get(attunement) {
                    ui.label("Super ability");
                    ui.label(choice.super_abilities.first().map_or("Unknown super", |s| s.name.as_str()));
                    ui.end_row();
                    ui.label("Melee ability");
                    ui.label(&choice.melee.name);
                    ui.end_row();
                } else {
                    ui.label("Super ability");
                    ability_combo(ui, "super", &mut super_ability, &abilities.super_ability);
                    ui.end_row();
                    ui.label("Melee ability");
                    ability_combo(ui, "melee", &mut melee, &abilities.melee);
                    ui.end_row();
                }
                ui.label("Class ability");
                ability_combo(ui, "class_ability", &mut class_ability, &abilities.class_ability);
                ui.end_row();
            });

        let mut changed = false;
        if class_type != original_class {
            // Keep this class's own armor so the character stays wearable.
            let template = settings::collect_class_armor_defaults(self.document)
                .get(&class_type)
                .cloned();
            if let (Some(template), Some(object)) = (
                template,
                self.document.pointer_mut(&path).and_then(Value::as_object_mut),
            ) {
                changed |= settings::restore_class_armor(object, &template);
            }
            if let Some(subclass) = subclasses
                .iter()
                .find(|item| item.class_type == class_type)
                .cloned()
            {
                equip_subclass = Some(subclass);
            }
        }
        if let Some(object) = self.document.pointer_mut(&path).and_then(Value::as_object_mut) {
            for (key, value) in [
                ("class", class_type),
                ("race", race),
                ("gender", gender),
                ("movement_ability", movement),
                ("grenade_ability", grenade),
                ("super_ability", super_ability),
                ("melee_ability", melee),
                ("class_ability", class_ability),
            ] {
                if object.get(key).and_then(Value::as_u64) != Some(value) {
                    object.insert(key.into(), Value::from(value));
                    changed = true;
                }
            }
        }
        if let Some(subclass) = equip_subclass {
            return Some(
                settings::equip_definition(
                    self.document,
                    character,
                    "subclass",
                    subclass.hash,
                    &subclass.default_plugs,
                )
                .map(|()| format!("Equipped {}", subclass.name)),
            );
        }
        changed.then(|| Ok(format!("Updated character {}", character + 1)))
    }
}

/// One row of a picker: icon, name with where an archetype comes from, the
/// hash or type on the right, and the first line of the description below.
/// Painted by hand so every row lines up whether or not its icon has loaded.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    catalog: &Catalog,
    size: f32,
    hash: Option<u64>,
    name: &str,
    detail: &str,
    selected: bool,
) -> bool {
    let description = hash.and_then(|hash| catalog.description(hash));
    let source = hash.and_then(|hash| catalog.source(hash));
    let height = size + 8.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    let response = match description {
        Some(description) => response.on_hover_text(match source {
            Some(source) => format!("{name} · {source}\n\n{description}"),
            None => format!("{name}\n\n{description}"),
        }),
        None => response,
    };
    hash_menu(&response, hash);
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }

    // A highlighted row keeps the ordinary foreground colour and dims its
    // secondary text from that, rather than using the theme's weak grey, which
    // all but disappears against the selection fill.
    let highlighted = selected || response.hovered();
    if highlighted {
        let fill = if selected {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 3.0, fill);
    }
    let strong = if highlighted {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let weak = if highlighted {
        strong.gamma_multiply(0.75)
    } else {
        ui.visuals().weak_text_color()
    };
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 4.0, rect.center().y - size / 2.0),
        egui::vec2(size, size),
    );
    let texture = match hash {
        Some(hash) if catalog.is_stat_plug(hash) => Some(icons.stat_plug(ui.ctx())),
        Some(hash) => icons.get(ui.ctx(), hash, fallback(catalog, hash)),
        None => Some(icons.empty(ui.ctx())),
    };
    match texture {
        Some(texture) => egui::Image::new((texture.id(), icon_rect.size()))
            .corner_radius(2)
            .paint_at(ui, icon_rect),
        // Only a deferred load lands here; keep the column aligned meanwhile.
        None => {
            ui.painter()
                .rect_filled(icon_rect, 2.0, ui.visuals().faint_bg_color);
        }
    }

    let text_left = icon_rect.right() + 10.0;
    let text_right = rect.right() - 8.0;
    let description = description
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.replace('\n', " "));

    let detail = (!detail.is_empty()).then(|| {
        ui.painter().layout_no_wrap(
            detail.to_owned(),
            egui::FontId::proportional(DETAIL_FONT),
            weak,
        )
    });
    let name_width = text_right
        - text_left
        - detail.as_ref().map_or(0.0, |galley| galley.rect.width() + 12.0);
    let name = name_line(ui, name, source, strong, weak, name_width);
    // Descriptions run to several lines in game; one line of it here is enough
    // to tell two similar perks apart, and hovering shows the rest.
    let description = description
        .map(|text| truncated(ui, &text, DETAIL_FONT, weak, text_right - text_left));

    let stack = name.rect.height() + description.as_ref().map_or(0.0, |g| g.rect.height());
    let mut y = rect.center().y - stack / 2.0;
    if let Some(detail) = detail {
        ui.painter().galley(
            egui::pos2(text_right - detail.rect.width(), y + 2.0),
            detail,
            weak,
        );
    }
    y += name.rect.height();
    ui.painter()
        .galley(egui::pos2(text_left, y - name.rect.height()), name, strong);
    if let Some(description) = description {
        ui.painter()
            .galley(egui::pos2(text_left, y), description, weak);
    }
    response.clicked()
}

fn name_line(
    ui: &egui::Ui,
    name: &str,
    source: Option<&str>,
    color: egui::Color32,
    weak: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let Some(source) = source else {
        return truncated(ui, name, NAME_FONT, color, width);
    };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width.max(16.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.append(
        name,
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(NAME_FONT), color),
    );
    job.append(
        &format!("  ·  {source}"),
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(DETAIL_FONT), weak),
    );
    ui.fonts(|fonts| fonts.layout_job(job))
}

/// Lays out one line of text, ellipsized to fit the given width.
fn truncated(
    ui: &egui::Ui,
    text: &str,
    size: f32,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontId::proportional(size),
        color,
        width.max(16.0),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts(|fonts| fonts.layout_job(job))
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

/// Whether a slot keeps an inventory beside what it has equipped. The game
/// takes one clan banner and no more, so that row is the editor alone.
fn holds_inventory(slot: &str) -> bool {
    slot != "clan_banner"
}

/// The key a row's inventory is stored under. It has to be reachable without a
/// `Ui`, since the randomizer fills every box of every row in one go.
fn inventory_id(character: usize, slot: &str) -> egui::Id {
    egui::Id::new(("inventory", character, slot))
}

/// How tall an icon button of this size ends up.
fn icon_height(ui: &egui::Ui, icon: f32) -> f32 {
    icon + 2.0 * ui.spacing().button_padding.y
}

fn matrix_height(ui: &egui::Ui) -> f32 {
    let rows = INVENTORY_ROWS as f32;
    rows * icon_height(ui, INVENTORY_CELL) + (rows - 1.0) * ui.spacing().item_spacing.y
}

/// A divider of a known height. `ui.separator()` takes the height its row
/// could still grow into, which on the first row of the page is the rest of
/// the panel — and leaves a tall gap under it.
fn divider(ui: &mut egui::Ui, height: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.spacing().item_spacing.x, height), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

/// The widest line the editor draws: a pinned socket, a row of five beside it,
/// and a divider before each — one after the pin, and one wherever the group
/// changes, which a row of five single-socket groups reaches. Sizing for that
/// keeps a busy line from wrapping back under the pinned column. Fixing the
/// column here also puts the inventory right beside it rather than out at the
/// far edge of the window.
fn editor_width(ui: &egui::Ui) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    let icon = SOCKET_ICON + 2.0 * ui.spacing().button_padding.x;
    let icons = (MAX_ROW_WIDTH + 1) as f32;
    let dividers = MAX_ROW_WIDTH as f32;
    // Every widget on the line, with egui's spacing in each joint.
    icons * icon + dividers * gap + (icons + dividers - 1.0) * gap + 2.0
}

/// Fills rows with the sockets in weight order: a group stays whole where it
/// can, moving to the next row rather than being split across one. The last
/// row takes the remainder if a piece somehow carries more than the rows hold,
/// since a socket that is not drawn cannot be edited.
fn pack_rows(loose: &[(u16, RowGroup, usize)]) -> Vec<Vec<Vec<usize>>> {
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut last_group = None;
    for (_, group, socket) in loose {
        match runs.last_mut() {
            Some(run) if last_group == Some(*group) => run.push(*socket),
            _ => runs.push(vec![*socket]),
        }
        last_group = Some(*group);
    }

    let mut rows: Vec<Vec<Vec<usize>>> = vec![Vec::new()];
    let mut width = 0;
    for run in runs {
        let mut rest = run.as_slice();
        let mut continuing = false;
        while !rest.is_empty() {
            let space = MAX_ROW_WIDTH.saturating_sub(width);
            // A group that would be split but could stand whole on the next
            // row goes there instead.
            let whole_elsewhere = width > 0 && rest.len() > space && rest.len() <= MAX_ROW_WIDTH;
            if (space == 0 || whole_elsewhere) && rows.len() < MAX_ROWS {
                rows.push(Vec::new());
                width = 0;
                continuing = false;
            }
            let take = rest.len().min(MAX_ROW_WIDTH.saturating_sub(width).max(1));
            let row = rows.last_mut().expect("a row is always open");
            match row.last_mut() {
                Some(segment) if continuing => segment.extend_from_slice(&rest[..take]),
                _ => row.push(rest[..take].to_vec()),
            }
            width += take;
            continuing = true;
            rest = &rest[take..];
        }
    }
    rows
}

/// Plain socket order, six across. Rows below the first start a column in and
/// so hold one fewer, which is what keeps the widest gear to two rows.
fn ungrouped_rows(sockets: &[usize]) -> Vec<SocketRow> {
    let (first, rest) = sockets.split_at(sockets.len().min(MAX_ROW_WIDTH + 1));
    std::iter::once(SocketRow::new([first.to_vec()]))
        .chain(
            rest.chunks(MAX_ROW_WIDTH)
                .map(|chunk| SocketRow::new([chunk.to_vec()]).indented()),
        )
        .collect()
}

fn hash_menu(response: &egui::Response, hash: Option<u64>) {
    let Some(hash) = hash else {
        return;
    };
    response.context_menu(|ui| {
        for (label, text) in [
            ("Copy hash", format_hash(hash)),
            ("Copy ID", hash.to_string()),
        ] {
            if ui.button(label).clicked() {
                ui.ctx().copy_text(text);
                ui.close_menu();
            }
        }
    });
}

/// Mods get their own stand-in, since they are the category this build ships
/// without art.
fn fallback(catalog: &Catalog, hash: u64) -> Fallback {
    if catalog.is_mod_plug(hash) {
        Fallback::Mod
    } else {
        Fallback::Plug
    }
}

fn combo(ui: &mut egui::Ui, id: &str, value: &mut u64, choices: &[(u64, &str)]) {
    let selected = choices
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("Invalid", |(_, name)| *name);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(160.0)
        .show_ui(ui, |ui| {
            for &(candidate, name) in choices {
                ui.selectable_value(value, candidate, name);
            }
        });
}

fn ability_combo(ui: &mut egui::Ui, id: &str, value: &mut u64, choices: &[AbilityChoice]) {
    let selected = choices
        .iter()
        .find(|choice| choice.entry == *value)
        .map_or_else(|| format!("Unknown entry {value}"), |choice| choice.name.clone());
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(260.0)
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(value, choice.entry, &choice.name);
            }
            if choices.is_empty() {
                ui.label("No named choices for this subclass");
            }
        });
}

/// The attunement that owns a super and melee pairing.
fn attunement_index(abilities: &AbilityOptions, super_ability: u64, melee: u64) -> usize {
    let paths = &abilities.attunements;
    paths
        .iter()
        .position(|path| {
            path.melee.entry == melee
                && path.super_abilities.iter().any(|choice| choice.entry == super_ability)
        })
        .or_else(|| paths.iter().position(|path| path.melee.entry == melee))
        .or_else(|| {
            paths
                .iter()
                .position(|path| path.super_abilities.iter().any(|choice| choice.entry == super_ability))
        })
        .unwrap_or(0)
}

fn default_abilities(class_type: u64, abilities: &AbilityOptions) -> (u64, u64, u64, u64, u64) {
    let pick = |choices: &[AbilityChoice], preferred: u64| {
        choices
            .iter()
            .find(|choice| choice.entry == preferred)
            .or_else(|| choices.first())
            .map_or(preferred, |choice| choice.entry)
    };
    let movement = match class_type {
        0 | 1 => 6,
        _ => 5,
    };
    (
        pick(&abilities.movement, movement),
        pick(&abilities.grenade, 7),
        pick(&abilities.super_ability, 10),
        pick(&abilities.melee, 11),
        pick(&abilities.class_ability, 2),
    )
}

pub fn character_tab_label(character: &Value, index: usize) -> String {
    let class_type = character.get("class").and_then(Value::as_u64).unwrap_or(99);
    format!("Character {} · {}", index + 1, class_name(class_type))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_piece_lays_out_within_three_lines_of_six() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let mut document = Value::Null;
        let mut icons = Icons::new();
        let mut state = LoadoutState::default();
        let page = Page {
            document: &mut document,
            catalog: &catalog,
            icons: &mut icons,
            state: &mut state,
        };

        for item in &catalog.items {
            let Some(&(slot, ..)) = SLOTS.iter().find(|(_, _, bucket)| *bucket == item.bucket_hash)
            else {
                continue;
            };
            let rows = page.socket_rows(item, slot, item.sockets.len());
            let where_ = format!("{} ({slot})", item.name);
            assert!(rows.len() <= MAX_ROWS, "{where_} needs {} lines", rows.len());
            // Every piece of gear pins something, so the first line always
            // starts in the first column with a divider after it.
            if let Some(first) = rows.first() {
                assert!(!first.indented, "{where_} pins nothing");
                assert_eq!(first.segments[0].len(), 1, "{where_} pins more than one socket");
            }

            let mut drawn: Vec<usize> = Vec::new();
            for row in &rows {
                let width: usize = row.segments.iter().map(Vec::len).sum();
                // A pinned socket sits beside the row, not in it, so a line
                // runs one wider than a row does.
                assert!(width <= MAX_ROW_WIDTH + 1, "{where_} has a line {width} across");
                drawn.extend(row.segments.iter().flatten());
            }
            let mut sorted = drawn.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), drawn.len(), "{where_} drew a socket twice");
            assert_eq!(
                sorted,
                (0..item.sockets.len()).collect::<Vec<_>>(),
                "{where_} left a socket undrawn"
            );

            // Weight puts the shader last wherever one exists.
            let shader = (0..item.sockets.len())
                .find(|socket| catalog.cosmetic_kind(item, *socket) == Some(CosmeticKind::Shader));
            if let Some(shader) = shader {
                assert_eq!(drawn.last(), Some(&shader), "{where_} does not end on its shader");
            }
        }
    }

    #[test]
    fn randomizing_fills_every_box_of_every_slot() {
        let catalog = Catalog::load().expect("bundled catalog must parse");
        let mut document = serde_json::json!({
            "schema": 3,
            "state": { "characters": [{ "class": 1, "equipment": {} }] }
        });
        let mut icons = Icons::new();
        let mut state = LoadoutState::default();
        let mut page = Page {
            document: &mut document,
            catalog: &catalog,
            icons: &mut icons,
            state: &mut state,
        };
        page.randomize(0).expect("randomizing must not fail");

        let mut exotic_weapons = 0;
        for &(slot, _, bucket) in SLOTS {
            if bucket == SUBCLASS_BUCKET {
                continue;
            }
            let equipped = document
                .pointer(&format!("/state/characters/0/equipment/{slot}"))
                .and_then(|item| item.get("definition_hash"))
                .and_then(parse_unsigned_value)
                .unwrap_or_else(|| panic!("{slot} was left without an item"));
            if WEAPON_SLOTS.contains(&slot) {
                exotic_weapons += usize::from(
                    catalog.get(equipped).is_some_and(|item| catalog.is_exotic(item)),
                );
            }
            let row = inventory_id(0, slot);
            let held = (1..=INVENTORY_ROWS * INVENTORY_COLUMNS)
                .filter(|box_index| state.parked_hash(row, *box_index).is_some())
                .count();
            // A clan banner has no inventory to fill: the game equips one.
            let wanted = if holds_inventory(slot) { INVENTORY_ROWS * INVENTORY_COLUMNS } else { 0 };
            assert_eq!(held, wanted, "{slot} held the wrong number of items");
        }
        assert!(exotic_weapons <= 1, "the randomizer equipped {exotic_weapons} exotic weapons");
    }
}




