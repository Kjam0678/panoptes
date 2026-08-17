//! The searchable icon list a socket or a piece of gear opens, and the rows it
//! is drawn from.

use eframe::egui;

use crate::{
    catalog::Catalog,
    icons::Icons,
    model::{format_hash, parse_unsigned_value, pointer},
    settings,
    status::Change,
};

use super::{
    Editing, Page, Target,
    widgets::{GEAR_ICON, SOCKET_ICON, hash_menu, texture},
};

/// Plug rows show the same icon a socket does, so a plug looks identical
/// whether it is installed or being chosen.
pub(super) const PLUG_ROW_ICON: f32 = SOCKET_ICON;

pub(super) const NAME_FONT: f32 = 16.0;
pub(super) const DETAIL_FONT: f32 = 13.0;
const PICKER_WIDTH: f32 = 580.0;
const PICKER_HEIGHT: f32 = 470.0;
const MAX_PICKER_ROWS: usize = 300;

/// Everything one picker needs to know about what it is choosing from.
pub(super) struct Picker<'a> {
    pub(super) id: egui::Id,
    pub(super) options: &'a [u64],
    pub(super) current: Option<u64>,
    pub(super) allow_empty: bool,
    pub(super) icon: f32,
}

impl Page<'_> {
    /// The item picker, opened by a gear piece's icon.
    pub(super) fn item_picker(
        &mut self,
        ui: &mut egui::Ui,
        anchor: &egui::Response,
        editing: Editing,
    ) -> Option<Change> {
        let Editing { character, slot, target } = editing;
        let id = ui.make_persistent_id(("item", character, slot.name, target.box_index()));
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
            allow_empty: target != Target::Equipped || slot.is_weapon(),
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
                    .map(|()| format!("Emptied the {} slot", slot.label)),
                Target::Parked(box_index) => match self.held_index(character, slot, box_index) {
                    Some(held) => settings::take_held_item(self.document, character, held)
                        .map(|_| format!("Emptied {}", editing.label())),
                    None => Ok(format!("{} is already empty", editing.label())),
                },
            });
        };
        let Some(item) = self.catalog.get_for_bucket(hash, slot.bucket).cloned() else {
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
            // An empty box has nothing in the file yet, so choosing an item for
            // it adds one to what the character holds.
            Target::Parked(box_index) => match self.held_index(character, slot, box_index) {
                Some(held) => {
                    let level = settings::inferred_item_level(self.document, character);
                    match self.document.pointer_mut(&pointer::held(character, held)) {
                        Some(value) => {
                            settings::set_item_definition(value, item.hash, &item.default_plugs, level)
                                .map(|()| format!("Put {} in inventory {box_index}", item.name))
                        }
                        None => Err("That item is no longer there".to_owned()),
                    }
                }
                None => {
                    settings::hold_definition(self.document, character, item.hash, &item.default_plugs)
                        .map(|_| format!("Put {} in inventory {box_index}", item.name))
                }
            },
        })
    }

    /// The shared hover menu: a search box above an icon list. Returns
    /// `Some(selection)` on the frame a row is clicked, where the inner `None`
    /// means the "empty" row.
    pub(super) fn picker_popup(
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
}

/// One row of a picker: icon, name with where an archetype comes from, the
/// hash or type on the right, and the first line of the description below.
/// Painted by hand so every row lines up whether or not its icon has loaded.
#[allow(clippy::too_many_arguments)]
pub(super) fn row(
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
    match texture(icons, catalog, ui, hash) {
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
