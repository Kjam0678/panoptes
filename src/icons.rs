//! Lazily decodes the baked icon for a hash and keeps it as a GPU texture.
//!
//! Every icon is compiled into the binary as one pack built by `prep-icons`,
//! so the app ships as a single file with nothing to install beside it. Any
//! watermark was already composited in, making an icon one decode and upload.

use std::collections::HashMap;

use eframe::egui;

/// Decoding every icon of a large plug list in one frame would stutter, so a
/// frame loads at most this many new icons and repaints for the rest.
const LOADS_PER_FRAME: usize = 24;

/// Stands in for the several hundred plugs this build ships without art. One
/// texture is shared by every one of them.
const UNKNOWN_PLUG: &[u8] = include_bytes!("../assets/default_icons/unknown-plug.png");
/// Drawn for a socket with nothing installed.
const EMPTY_PLUG: &[u8] = include_bytes!("../assets/default_icons/empty-plug.jpg");
/// Drawn for Armor 2.0's stat allocation plugs, which ship no art of their own.
const STAT_PLUG: &[u8] = include_bytes!("../assets/default_icons/armor_stat_plug.jpg");
/// Drawn for the armor and weapon mods this build ships without art.
const UNKNOWN_MOD: &[u8] = include_bytes!("../assets/default_icons/unknown-mod.png");

/// `MAGIC`, the entry count, then that many 12-byte `[hash, offset, length]`
/// entries sorted by hash, then the image bytes. Written by `prep-icons`.
const ICON_PACK: &[u8] = include_bytes!("../assets/icons.pack");
const MAGIC: &[u8; 8] = b"SD2ICON1";
const ENTRY: usize = 12;

/// Which stand-in an art-less hash falls back to.
#[derive(Clone, Copy)]
pub enum Fallback {
    Plug,
    Mod,
}

pub struct Icons {
    textures: HashMap<u64, Option<egui::TextureHandle>>,
    unknown: Option<egui::TextureHandle>,
    unknown_mod: Option<egui::TextureHandle>,
    empty: Option<egui::TextureHandle>,
    stat: Option<egui::TextureHandle>,
    budget: usize,
    deferred: bool,
}

impl Icons {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            unknown: None,
            unknown_mod: None,
            empty: None,
            stat: None,
            budget: LOADS_PER_FRAME,
            deferred: false,
        }
    }

    /// How many icons this build ships, for the Paths page.
    pub fn packed() -> usize {
        entry_count()
    }

    pub fn begin_frame(&mut self) {
        self.budget = LOADS_PER_FRAME;
        self.deferred = false;
    }

    /// True when icons were left unloaded this frame and a repaint is needed.
    pub const fn wants_repaint(&self) -> bool {
        self.deferred
    }

    /// The icon for a hash, or the stand-in this build has for it. `None` only
    /// while a busy frame defers the load.
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        hash: u64,
        fallback: Fallback,
    ) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(&hash) {
            return texture.clone();
        }
        if self.budget == 0 {
            self.deferred = true;
            return None;
        }
        self.budget -= 1;
        let texture = Some(match Self::load(ctx, hash) {
            Some(texture) => texture,
            None => self.stand_in(ctx, fallback),
        });
        self.textures.insert(hash, texture.clone());
        texture
    }

    fn stand_in(&mut self, ctx: &egui::Context, fallback: Fallback) -> egui::TextureHandle {
        match fallback {
            Fallback::Plug => self
                .unknown
                .get_or_insert_with(|| embedded(ctx, "icon-unknown", UNKNOWN_PLUG)),
            Fallback::Mod => self
                .unknown_mod
                .get_or_insert_with(|| embedded(ctx, "icon-unknown-mod", UNKNOWN_MOD)),
        }
        .clone()
    }

    /// The plate drawn for a socket with nothing installed.
    pub fn empty(&mut self, ctx: &egui::Context) -> egui::TextureHandle {
        self.empty
            .get_or_insert_with(|| embedded(ctx, "icon-empty", EMPTY_PLUG))
            .clone()
    }

    /// The plate drawn for every stat allocation plug.
    pub fn stat_plug(&mut self, ctx: &egui::Context) -> egui::TextureHandle {
        self.stat
            .get_or_insert_with(|| embedded(ctx, "icon-stat", STAT_PLUG))
            .clone()
    }

    fn load(ctx: &egui::Context, hash: u64) -> Option<egui::TextureHandle> {
        let bytes = packed_icon(u32::try_from(hash).ok()?)?;
        let image = image::load_from_memory(bytes).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        Some(ctx.load_texture(format!("icon-{hash:08X}"), image, egui::TextureOptions::LINEAR))
    }
}

fn embedded(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let image = image::load_from_memory(bytes)
        .expect("embedded icons must decode")
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        egui::TextureOptions::LINEAR,
    )
}

fn entry_count() -> usize {
    if !ICON_PACK.starts_with(MAGIC) {
        return 0;
    }
    ICON_PACK
        .get(8..12)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, |bytes| u32::from_le_bytes(bytes) as usize)
}

/// Binary-searches the index for one hash and returns its image bytes.
fn packed_icon(hash: u32) -> Option<&'static [u8]> {
    let count = entry_count();
    let index = ICON_PACK.get(12..12 + count * ENTRY)?;
    let field = |entry: usize, field: usize| {
        let at = entry * ENTRY + field * 4;
        index
            .get(at..at + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map_or(0, u32::from_le_bytes)
    };

    let (mut low, mut high) = (0, count);
    while low < high {
        let middle = low + (high - low) / 2;
        match field(middle, 0).cmp(&hash) {
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
            std::cmp::Ordering::Equal => {
                let (offset, length) = (field(middle, 1) as usize, field(middle, 2) as usize);
                return ICON_PACK.get(offset..offset + length);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiled_pack_holds_every_icon_and_decodes_them() {
        assert!(ICON_PACK.starts_with(MAGIC));
        assert!(entry_count() > 8000);

        // Outlaw, and an item icon, both baked with their watermark.
        for hash in [0x5B17_BB28, 0x21E2_DCAF] {
            let bytes = packed_icon(hash).expect("icon must be packed");
            let image = image::load_from_memory(bytes).expect("icon must decode");
            assert_eq!((image.width(), image.height()), (96, 96));
        }
        // A hash this build has no art for resolves to nothing, not to garbage.
        assert!(packed_icon(0xFFFF_FFFF).is_none());

        // The stand-ins decode too: `embedded` panics if one ever does not,
        // and that would happen in front of a user rather than here.
        for bytes in [UNKNOWN_PLUG, UNKNOWN_MOD, EMPTY_PLUG, STAT_PLUG] {
            image::load_from_memory(bytes).expect("a stand-in icon must decode");
        }
    }
}
