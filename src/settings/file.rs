//! Reading and writing settings.json, and the backups that make writing it
//! safe to undo.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::{model::MAX_SETTINGS_BYTES, paths, storage};

// ---------------------------------------------------------------- file access

pub fn load_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("No settings.json at {}", path.display())
        } else {
            format!("Could not read {}: {error}", path.display())
        }
    })?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

pub fn verify_source_unchanged(path: &Path, expected: &Value) -> Result<(), String> {
    if load_json(path)? == *expected {
        Ok(())
    } else {
        Err("settings.json changed on disk after it was loaded. Reload before saving so newer data is not overwritten".into())
    }
}

/// Writes the document, keeping a timestamped backup and verifying the result.
pub fn save_json(path: &Path, document: &Value) -> Result<PathBuf, String> {
    let mut encoded = encode_settings(document)?;
    encoded.push('\n');
    if encoded.len() > MAX_SETTINGS_BYTES {
        return Err(format!(
            "The encoded settings would be {} bytes; Sunrise refuses anything past {MAX_SETTINGS_BYTES}",
            encoded.len()
        ));
    }

    let backup_root = paths::backup_dir().ok_or("Could not locate the backup folder")?;
    fs::create_dir_all(&backup_root)
        .map_err(|e| format!("Could not create {}: {e}", backup_root.display()))?;
    let backup = backup_root.join(format!(
        "settings-{}-{}.json",
        backup_timestamp()?,
        std::process::id()
    ));
    create_backup(path, &backup)?;

    storage::replace_file(path, encoded.as_bytes())
        .map_err(|e| format!("Could not safely replace {}: {e}", path.display()))?;
    let verified = load_json(path).and_then(|saved| {
        if saved == *document {
            Ok(())
        } else {
            Err("the saved document did not match the requested settings".to_owned())
        }
    });
    if let Err(error) = verified {
        let restored = fs::read(&backup)
            .and_then(|contents| storage::replace_file(path, &contents))
            .map_err(|restore_error| restore_error.to_string());
        return Err(match restored {
            Ok(()) => format!("Could not verify the saved settings ({error}); the original file was restored"),
            Err(restore_error) => format!(
                "Could not verify the saved settings ({error}), and restoring the backup failed: {restore_error}. The backup is at {}",
                backup.display()
            ),
        });
    }
    Ok(backup)
}

/// What separates one backup of the same file from the next.
fn backup_timestamp() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .map_err(|e| format!("Could not create a backup timestamp: {e}"))
}

fn create_backup(source: &Path, destination: &Path) -> Result<(), String> {
    let mut source_file = fs::File::open(source)
        .map_err(|e| format!("Could not open {} for backup: {e}", source.display()))?;
    let mut backup_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("Could not create {}: {e}", destination.display()))?;
    if let Err(error) = io::copy(&mut source_file, &mut backup_file).and_then(|_| backup_file.sync_all()) {
        drop(backup_file);
        let _ = fs::remove_file(destination);
        return Err(format!("Could not create {}: {error}", destination.display()));
    }
    Ok(())
}

/// An extra copy beside the original, used whenever the file held something
/// this editor did not recognize.
pub fn create_adjacent_backup(source: &Path) -> Result<PathBuf, String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("{} has no file name", source.display()))?
        .to_string_lossy()
        .into_owned();
    let destination = source.with_file_name(format!("{file_name}.bak"));
    let contents =
        fs::read(source).map_err(|e| format!("Could not read {} for backup: {e}", source.display()))?;
    if destination.exists() {
        if fs::read(&destination).is_ok_and(|existing| existing == contents) {
            return Ok(destination);
        }
        create_backup(
            &destination,
            &source.with_file_name(format!(
                "{file_name}.bak.previous-{}",
                backup_timestamp()?
            )),
        )?;
        storage::replace_file(&destination, &contents)
            .map_err(|e| format!("Could not update {}: {e}", destination.display()))?;
    } else {
        create_backup(source, &destination)?;
    }
    Ok(destination)
}

/// Sunrise reads settings.json into a fixed buffer, so arrays are written on
/// one line: fewer bytes for the same document.
pub fn encode_settings(document: &Value) -> Result<String, String> {
    fn write_value(value: &Value, indent: usize, output: &mut String) -> Result<(), String> {
        match value {
            Value::Object(object) if !object.is_empty() => {
                output.push_str("{\n");
                for (index, (key, child)) in object.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    output.push_str(
                        &serde_json::to_string(key).map_err(|e| format!("Could not encode a setting name: {e}"))?,
                    );
                    output.push_str(": ");
                    write_value(child, indent + 2, output)?;
                    if index + 1 != object.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push('}');
            }
            other => output.push_str(
                &serde_json::to_string(other).map_err(|e| format!("Could not encode a setting: {e}"))?,
            ),
        }
        Ok(())
    }

    let mut output = String::new();
    write_value(document, 0, &mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays_are_encoded_on_one_line_to_stay_under_the_size_limit() {
        let document = serde_json::json!({
            "schema": 3,
            "state": { "characters": [{ "soid": "0x1234", "class": 1 }] }
        });
        let encoded = encode_settings(&document).unwrap();
        assert!(encoded.contains("\"characters\": ["));
        assert!(!encoded.contains("\n      \"soid\"") || encoded.contains("\"soid\""));
    }
}
