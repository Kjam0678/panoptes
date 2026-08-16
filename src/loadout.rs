//! The loadout page: one row per gear piece, sockets laid out left to right,
//! each socket a button showing the icon of whatever is plugged into it.

use std::collections::HashMap;

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{AbilityChoice, AbilityOptions, Catalog, ItemDef, PlugFilter},
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
/// Sockets across when they are not grouped, so a piece with eleven of them
/// wraps at a predictable width instead of stretching the panel. Rows after
/// the first start a column in and so hold one fewer.
const SOCKETS_PER_ROW: usize = 6;
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
        }
    }
}

impl LoadoutState {
    /// Drops per-slot search text when a different file is opened.
    pub fn clear_pickers(&mut self) {
        self.searches.clear();
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

    fn draw_slot(
        &mut self,
        ui: &mut egui::Ui,
        character: usize,
        slot: &str,
        label: &str,
        bucket: u64,
    ) -> Option<Change> {
        let equipped = self.equipped(character, slot);
        let is_empty = equipped.is_some_and(Value::is_null);
        let equipped_hash = equipped
            .and_then(|value| value.get("definition_hash"))
            .and_then(parse_unsigned_value);
        let item = equipped_hash
            .and_then(|hash| self.catalog.get_for_bucket(hash, bucket))
            .cloned();

        let title = match (is_empty, &item) {
            (true, _) => "Empty".to_owned(),
            (false, Some(item)) => item.name.clone(),
            (false, None) => format!(
                "Unknown item {}",
                equipped_hash.map_or_else(|| "<missing>".to_owned(), format_hash)
            ),
        };
        // The gear icon is the picker's only trigger, matching every socket.
        let mut level_change = None;
        let icon = ui
            .horizontal(|ui| {
                let icon = self
                    .icon_button(ui, equipped_hash, GEAR_ICON)
                    .on_hover_text(format!("{label}: {title}\nClick to change"));
                hash_menu(&icon, equipped_hash);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(label).strong().size(13.0));
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
                    if !is_empty
                        && (WEAPON_SLOTS.contains(&slot) || ARMOR_SLOTS.contains(&slot))
                    {
                        level_change = self.draw_level(ui, character, slot);
                    }
                });
                icon
            })
            .inner;
        let mut change = self
            .item_picker(ui, &icon, character, slot, bucket)
            .or(level_change);

        let Some(item) = item else {
            return change;
        };
        let (plugs, native_defaults) = settings::displayed_plugs(
            self.document
                .pointer(&format!("/state/characters/{character}/equipment/{slot}/plugs")),
            &item.default_plugs,
        );
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
                        ui.separator();
                    }
                    for socket_index in segment.iter().copied() {
                        if row_index == 0 && column == 1 {
                            indent = ui.cursor().left() - left;
                        }
                        let current = plugs.get(socket_index).and_then(parse_unsigned_value);
                        if let Some(socket_change) =
                            self.draw_socket(ui, character, slot, &item, socket_index, current)
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

    // -------------------------------------------------------- socket layout

    /// How a piece's sockets fall into rows, which depends on the gear type:
    /// a weapon leads with its intrinsic and hangs its masterwork underneath,
    /// armor leads with its energy socket and puts its stats on the next row,
    /// and everything else fits on one row with its cosmetics pushed right.
    fn socket_rows(&self, item: &ItemDef, slot: &str, socket_count: usize) -> Vec<SocketRow> {
        let (functional, mut cosmetic): (Vec<usize>, Vec<usize>) =
            (0..socket_count).partition(|index| {
                !self.state.group_sockets || self.catalog.cosmetic_kind(item, *index).is_none()
            });
        // Tracker, ornament, projection, then the shader that ends every row,
        // whatever order the item lists them in. The sort is stable, so
        // anything unclassified keeps its socket order.
        cosmetic.sort_by_key(|index| self.catalog.cosmetic_kind(item, *index));

        let mut rows = if !self.state.group_sockets {
            ungrouped_rows(&functional)
        } else if WEAPON_SLOTS.contains(&slot) {
            self.weapon_rows(item, functional, cosmetic)
        } else if ARMOR_SLOTS.contains(&slot) {
            self.armor_rows(item, functional, cosmetic)
        } else if slot == "ship" {
            // A ship's shader is one of two sockets, so a separator before it
            // would be more furniture than the row can carry.
            vec![SocketRow::new([functional.into_iter().chain(cosmetic).collect()])]
        } else {
            vec![SocketRow::new([functional, cosmetic])]
        };
        rows.retain(|row| !row.is_empty());
        rows
    }

    fn weapon_rows(
        &self,
        item: &ItemDef,
        functional: Vec<usize>,
        cosmetic: Vec<usize>,
    ) -> Vec<SocketRow> {
        let (masterwork, perks): (Vec<usize>, Vec<usize>) = functional
            .into_iter()
            .partition(|index| self.catalog.is_masterwork_socket(item, *index));
        // A Red War damage mod, or a socket with nothing in it, keeps the
        // masterwork company rather than crowding the perks.
        let (secondary, perks): (Vec<usize>, Vec<usize>) = perks
            .into_iter()
            .partition(|index| self.catalog.is_secondary_socket(item, *index));
        let [intrinsic, rest] = lead_with(perks, 0);
        let perk_row = SocketRow::new([intrinsic, rest]);

        // Only the masterwork and the sockets beside it sit under the
        // intrinsic; anything below them lines up with the perks.
        if !secondary.is_empty() {
            return vec![
                perk_row,
                SocketRow::new([masterwork, secondary]),
                SocketRow::new([cosmetic]).indented(),
            ];
        }
        let under_intrinsic = !masterwork.is_empty();
        let below = SocketRow::new([masterwork, cosmetic]);
        vec![perk_row, if under_intrinsic { below } else { below.indented() }]
    }

    fn armor_rows(
        &self,
        item: &ItemDef,
        functional: Vec<usize>,
        cosmetic: Vec<usize>,
    ) -> Vec<SocketRow> {
        let (stats, mut mods): (Vec<usize>, Vec<usize>) = functional
            .into_iter()
            .partition(|index| self.catalog.is_stat_socket(item, *index));
        // Year-1 armor pads itself out with sockets that hold nothing; an Aeon
        // piece has five and would run to eleven across. Only as many drop to
        // the row below as the width demands, since the first of them carries
        // the Aeon perk and belongs beside the mods.
        let mut spilled = Vec::new();
        while mods.len() > SOCKETS_PER_ROW {
            let Some(last) = mods
                .iter()
                .rposition(|index| self.catalog.is_secondary_socket(item, *index))
            else {
                break;
            };
            spilled.insert(0, mods.remove(last));
        }
        // Armor 2.0's energy socket leads the row the way a weapon's intrinsic
        // does, since it decides what the mods beside it can be. A Year-1 piece
        // has none and leads with the armor archetype it lists first.
        let lead = mods
            .iter()
            .position(|index| self.catalog.is_energy_socket(item, *index))
            .unwrap_or_default();
        let [energy, rest] = lead_with(mods, lead);
        vec![
            SocketRow::new([energy, rest]),
            // Spilled sockets run straight into the stats: both are the same
            // kind of afterthought, and a divider would read as a boundary
            // that is not there.
            SocketRow::new([spilled.into_iter().chain(stats).collect()]).indented(),
            SocketRow::new([cosmetic]).indented(),
        ]
    }

    /// The item's level, which Sunrise stores per equipped item and which the
    /// game reads as its power. 106 is what this build's characters ship with.
    fn draw_level(&mut self, ui: &mut egui::Ui, character: usize, slot: &str) -> Option<Change> {
        let pointer = format!("/state/characters/{character}/equipment/{slot}/level");
        let stored = self
            .document
            .pointer(&pointer)
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
        let value = self.document.pointer_mut(&pointer)?;
        *value = Value::from(level);
        Some(Ok(format!("Set {} to level {level}", slot_label(slot))))
    }

    /// One socket: an icon button that opens its searchable plug list.
    fn draw_socket(
        &mut self,
        ui: &mut egui::Ui,
        character: usize,
        slot: &str,
        item: &ItemDef,
        socket_index: usize,
        current: Option<u64>,
    ) -> Option<Change> {
        let id = ui.make_persistent_id(("socket", character, slot, socket_index));
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

        let socket = format!("{} socket {}", slot_label(slot), socket_index + 1);
        Some(
            settings::set_plug(
                self.document,
                character,
                slot,
                socket_index,
                &item.default_plugs,
                selection,
            )
            .map(|()| match selection {
                Some(hash) => format!("Installed {} in {socket}", self.catalog.plug_name(hash)),
                None => format!("Cleared {socket}"),
            }),
        )
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
        character: usize,
        slot: &str,
        bucket: u64,
    ) -> Option<Change> {
        let id = ui.make_persistent_id(("item", character, slot));
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
                .equipped(character, slot)
                .and_then(|value| value.get("definition_hash"))
                .and_then(parse_unsigned_value),
            allow_empty: WEAPON_SLOTS.contains(&slot),
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

        Some(match selection {
            Some(hash) => match self.catalog.get_for_bucket(hash, bucket).cloned() {
                Some(item) => settings::equip_definition(
                    self.document,
                    character,
                    slot,
                    item.hash,
                    &item.default_plugs,
                )
                .map(|()| format!("Equipped {}", item.name)),
                None => Err("That item is not valid for this slot".to_owned()),
            },
            None => settings::set_weapon_slot_empty(self.document, character, slot)
                .map(|()| format!("Emptied the {} slot", slot_label(slot))),
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

/// Plain socket order, six across. Rows below the first start a column in and
/// so hold one fewer, which is what keeps the widest gear to two rows.
fn ungrouped_rows(sockets: &[usize]) -> Vec<SocketRow> {
    let (first, rest) = sockets.split_at(sockets.len().min(SOCKETS_PER_ROW));
    std::iter::once(SocketRow::new([first.to_vec()]))
        .chain(
            rest.chunks(SOCKETS_PER_ROW - 1)
                .map(|chunk| SocketRow::new([chunk.to_vec()]).indented()),
        )
        .collect()
}

/// Splits the socket that leads a row off from the rest, in order.
fn lead_with(mut sockets: Vec<usize>, lead: usize) -> [Vec<usize>; 2] {
    let head = (lead < sockets.len()).then(|| sockets.remove(lead));
    [head.into_iter().collect(), sockets]
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
