//! Whole-file writes that never leave a half-written settings.json behind.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Writes the contents beside the destination, flushes them to disk, and then
/// replaces the destination in one filesystem operation.
pub fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent folder")
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}-{nonce}.tmp",
        file_name.to_string_lossy(),
        std::process::id()
    ));

    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_never_leaves_a_partial_or_temporary_file() {
        let directory = std::env::temp_dir().join(format!("panoptes-storage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("settings.json");
        fs::write(&destination, b"old").unwrap();

        replace_file(&destination, b"complete replacement").unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"complete replacement");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&directory);
    }
}
