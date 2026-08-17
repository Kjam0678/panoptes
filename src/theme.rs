//! The colours and metrics the pages share, so that one kind of message looks
//! the same wherever it appears.

use eframe::egui::Color32;

/// A setting that is missing, or holds something this build cannot edit.
pub const ERROR: Color32 = Color32::LIGHT_RED;
/// A file that will still be written, but not as the user may expect: an
/// unsupported ability pairing, or a plug list wide enough to break an item.
pub const WARNING: Color32 = Color32::from_rgb(255, 190, 80);
/// Edits that have not reached disk yet.
pub const UNSAVED: Color32 = Color32::YELLOW;

/// Row and column spacing for the game settings grids.
pub const SETTINGS_GRID_SPACING: [f32; 2] = [18.0, 9.0];
/// The tighter spacing of the character and key binding forms.
pub const FORM_GRID_SPACING: [f32; 2] = [18.0, 8.0];

/// A combo box holding one of the short enumerations: a class, a race.
pub const NARROW_COMBO: f32 = 160.0;
/// A game setting's combo box.
pub const COMBO: f32 = 210.0;
/// A combo box holding named choices long enough to need the room: subclasses,
/// abilities, key bindings.
pub const WIDE_COMBO: f32 = 260.0;
