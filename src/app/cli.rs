//! `--check`, which validates a settings.json without opening a window.

use serde_json::Value;

use crate::{catalog::Catalog, model::pointer, settings};

/// `--check <settings.json>` validates a file without opening a window.
pub(super) fn check(path: &std::path::Path) -> Result<String, String> {
    let document = settings::load_json(path)?;
    settings::validate_document(&document).map_err(|error| format!("Invalid settings: {error}"))?;
    let catalog = Catalog::load()?;
    Ok(format!(
        "Valid: {} characters, {} catalog items, save size {} bytes",
        document
            .pointer(pointer::CHARACTERS)
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        catalog.items.len(),
        settings::encode_settings(&document)?.len() + 1
    ))
}
