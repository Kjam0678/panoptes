//! The small pieces every part of the page draws with: icon buttons, the art
//! behind them, dividers of a known height, and the widths the row is built to.

use eframe::egui;

use crate::{
    catalog::Catalog,
    icons::{Fallback, Icons},
    model::format_hash,
};

use super::{Page, layout::MAX_ROW_WIDTH};

pub(super) const SOCKET_ICON: f32 = 44.0;
/// Gear reads at half again the size of a plug, on its loadout row and in its
/// own picker. Gear that appears *as* a plug — an ornament, a shader — stays
/// plug-sized, because the size belongs to the menu, not to the item.
pub(super) const GEAR_ICON: f32 = SOCKET_ICON * 1.5;

impl Page<'_> {
    pub(super) fn icon_button(&mut self, ui: &mut egui::Ui, hash: Option<u64>, size: f32) -> egui::Response {
        match texture(self.icons, self.catalog, ui, hash) {
            Some(texture) => ui.add(
                egui::ImageButton::new((texture.id(), egui::vec2(size, size))).corner_radius(3),
            ),
            // Every socket resolves to art, the stand-in, or the empty plate, so
            // a bare button here only means a load deferred to the next frame.
            None => ui.add_sized([size + 8.0, size + 8.0], egui::Button::new("")),
        }
    }
}

/// How tall an icon button of this size ends up. egui pads an image button by
/// `button_padding.x` on every side rather than by the vertical padding, so a
/// square icon stays square — and anything set beside one has to use the same
/// figure or it will not line up.
pub(super) fn icon_height(ui: &egui::Ui, icon: f32) -> f32 {
    icon + 2.0 * ui.spacing().button_padding.x
}

/// A divider of a known height. `ui.separator()` takes the height its row
/// could still grow into, which on the first row of the page is the rest of
/// the panel — and leaves a tall gap under it.
pub(super) fn divider(ui: &mut egui::Ui, height: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.spacing().item_spacing.x, height), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

/// The width the pinned column holds on every line, whether or not that line
/// pins anything. An icon button plus a little room, so the divider beside it
/// sits at one place down the whole piece.
pub(super) fn pin_column_width(ui: &egui::Ui) -> f32 {
    SOCKET_ICON + 2.0 * ui.spacing().button_padding.x + 3.0
}

/// The width of a line: the pinned column, the divider that closes it, and a
/// full row beside it. Fixing the column here keeps a busy line from wrapping
/// and puts the inventory right beside the editor rather than out at the far
/// edge of the window.
pub(super) fn editor_width(ui: &egui::Ui) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    let icon = SOCKET_ICON + 2.0 * ui.spacing().button_padding.x;
    let row = MAX_ROW_WIDTH as f32;
    // The column, the spacing and divider that close it, then the row itself
    // with egui's spacing in each joint.
    pin_column_width(ui) + 3.0 * gap + row * icon + (row - 1.0) * gap + 2.0
}

pub(super) fn hash_menu(response: &egui::Response, hash: Option<u64>) {
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

/// The art for a socket or a piece of gear: the hash's own icon, the plate
/// every stat allocation shares, the empty plate, or the stand-in this build
/// has for an art-less plug. `None` only while a busy frame defers the load,
/// which is why both callers keep drawing something in its place.
pub(super) fn texture(
    icons: &mut Icons,
    catalog: &Catalog,
    ui: &egui::Ui,
    hash: Option<u64>,
) -> Option<egui::TextureHandle> {
    let Some(hash) = hash else {
        return Some(icons.empty(ui.ctx()));
    };
    if catalog.is_stat_plug(hash) {
        return Some(icons.stat_plug(ui.ctx()));
    }
    // Mods get their own stand-in, since they are the category this build
    // ships without art.
    let fallback = if catalog.is_mod_plug(hash) {
        Fallback::Mod
    } else {
        Fallback::Plug
    };
    icons.get(ui.ctx(), hash, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pinned column reserves a cell of `icon_height`, and the divider is
    /// drawn to it, so both have to agree with what egui actually gives an
    /// icon button. Guessing at the padding is what put them 6px apart.
    #[test]
    fn a_cell_is_exactly_as_tall_as_the_socket_button_it_holds() {
        let ctx = egui::Context::default();
        let mut measured = (0.0, 0.0);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let image = egui::ColorImage::from_rgba_unmultiplied([8, 8], &[255u8; 8 * 8 * 4]);
                let texture = ui.ctx().load_texture("cell", image, egui::TextureOptions::LINEAR);
                let button = ui.add(
                    egui::ImageButton::new((texture.id(), egui::vec2(SOCKET_ICON, SOCKET_ICON)))
                        .corner_radius(3),
                );
                measured = (button.rect.height(), icon_height(ui, SOCKET_ICON));
            });
        });
        assert_eq!(
            measured.0, measured.1,
            "a pinned socket would not line up with the row beside it"
        );
    }
}
