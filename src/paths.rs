//! Where the app keeps its own files, and how it finds Sunrise's settings.json.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::storage;

/// Sunrise reads its settings from one of these two places inside the install.
pub const SETTINGS_LAYOUTS: [&str; 2] = ["Sunrise/settings.json", "bin/x64/Sunrise/settings.json"];

/// `%LOCALAPPDATA%\Panoptes` on Windows, `$XDG_CONFIG_HOME/panoptes` elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Panoptes"));
    }
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|base| base.join("panoptes"))
}

pub fn preferences_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("paths.json"))
}

pub fn backup_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("backups"))
}

pub fn remembered_settings_path() -> Option<PathBuf> {
    let raw = fs::read_to_string(preferences_path()?).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    let path = PathBuf::from(value.get("settings")?.as_str()?);
    path.is_file().then_some(path)
}

pub fn remember_settings_path(path: &Path) -> Result<(), String> {
    let destination = preferences_path().ok_or("Could not locate the preferences folder")?;
    let parent = destination
        .parent()
        .ok_or("The preferences path has no parent folder")?;
    fs::create_dir_all(parent).map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
    let encoded = serde_json::to_vec_pretty(&serde_json::json!({ "settings": path }))
        .map_err(|e| format!("Could not encode preferences: {e}"))?;
    storage::replace_file(&destination, &encoded)
        .map_err(|e| format!("Could not save preferences: {e}"))
}

/// Accepts either a settings.json or a Destiny install folder containing one.
pub fn resolve_selection(selected: &Path) -> Result<PathBuf, String> {
    if selected.is_file() {
        return Ok(selected.to_path_buf());
    }
    let found: Vec<PathBuf> = SETTINGS_LAYOUTS
        .iter()
        .map(|layout| selected.join(layout))
        .filter(|path| path.is_file())
        .collect();
    match found.len() {
        1 => Ok(found[0].clone()),
        0 => Err(format!(
            "No Sunrise settings.json was found in {}. Checked {} and {}",
            selected.display(),
            SETTINGS_LAYOUTS[0],
            SETTINGS_LAYOUTS[1]
        )),
        _ => Err(format!(
            "Two settings.json files exist under {}. Pick the exact file Sunrise uses",
            selected.display()
        )),
    }
}
