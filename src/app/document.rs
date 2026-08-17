//! The loaded settings.json and everything derived from it.

use std::path::PathBuf;

use serde_json::Value;

use crate::{model::pointer, settings};

/// The loaded settings.json and everything derived from it.
pub(super) struct Document {
    pub(super) path: PathBuf,
    pub(super) value: Value,
    pub(super) persisted: Value,
    pub(super) source_warning: Option<String>,
    pub(super) raw_json: String,
    pub(super) dirty: bool,
}

impl Document {
    pub(super) fn open(path: PathBuf) -> Result<Self, String> {
        let value = settings::load_json(&path)?;
        Ok(Self {
            source_warning: settings::validate_document(&value).err(),
            raw_json: serde_json::to_string_pretty(&value).unwrap_or_default(),
            persisted: value.clone(),
            value,
            path,
            dirty: false,
        })
    }

    pub(super) fn character_count(&self) -> usize {
        self.value
            .pointer(pointer::CHARACTERS)
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    }
}
