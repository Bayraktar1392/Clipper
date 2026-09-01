use std::{
    fs,
    path::{Path, PathBuf},
};

const QUEUE_FILE: &str = "queue.txt";
const CONFIG_EXT: &str = "download_dir";

/// The path to the app's config directory (`~/.config/clipper`).
fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".config").join("clipper"))
}

/// Returns the path to the queue persistence file under the user config dir.
fn queue_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(QUEUE_FILE))
}

/// Loads the previously persisted queue text, if any. Returns an empty
/// string when nothing has been saved (first launch).
pub fn load_queue() -> String {
    let Some(path) = queue_file() else {
        return String::new();
    };
    fs::read_to_string(path).unwrap_or_default()
}

/// Persists the current queue text so it survives app restarts. Failures
/// (e.g. a read-only home) are non-fatal: the queue simply won't be
/// remembered next time.
pub fn save_queue(text: &str) {
    let Some(path) = queue_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, text);
}

/// The downloads directory: a user-chosen folder if one was picked via the
/// folder chooser, otherwise `~/Downloads/Clipper`.
pub fn download_directory() -> Result<PathBuf, crate::error::AppError> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        crate::error::AppError::CommandFailed("filesystem".into(), "HOME is not set".into())
    })?;
    let dir = config_dir().map(|d| d.join(CONFIG_EXT));
    if let Some(path) = dir
        && let Ok(saved) = fs::read_to_string(path)
    {
        let trimmed = saved.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(home.join("Downloads").join("Clipper"))
}

/// Persists a user-chosen download directory so it survives restarts.
pub fn set_download_directory(path: &Path) {
    let Some(dir) = config_dir() else {
        return;
    };
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join(CONFIG_EXT), path.display().to_string());
}

/// Opens the downloads folder in the system file manager, if one exists.
pub fn open_download_directory() {
    let Ok(dir) = download_directory() else {
        return;
    };
    reveal_in_file_manager(&dir);
}

/// Best-effort "reveal in file manager" for an existing file or folder.
pub fn reveal_in_file_manager(path: &Path) {
    for opener in ["xdg-open", "gio", "open"] {
        if std::process::Command::new(opener)
            .arg(path)
            .status()
            .is_ok()
        {
            return;
        }
    }
}
