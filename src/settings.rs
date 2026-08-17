//! Sunrise's settings.json: reading and writing it, deciding what it may
//! contain, and editing what it holds.

mod edit;
mod file;
mod validate;

pub use edit::*;
pub use file::*;
pub use validate::*;

use serde_json::{Map, Value};

use crate::model::pointer;

/// The characters in the document, or nothing where the file has none.
fn characters(document: &Value) -> impl Iterator<Item = &Value> {
    document
        .pointer(pointer::CHARACTERS)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// Every item a character has, equipped or held. Sunrise reads the same shape
/// in either place, so most rules over a character's items apply to both.
fn character_items(character: &Map<String, Value>) -> impl Iterator<Item = &Map<String, Value>> {
    let equipped = character
        .get("equipment")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::values);
    let held = character
        .get("inventory")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    equipped.chain(held).filter_map(Value::as_object)
}
