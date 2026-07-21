//! Execution-environment path resolution for the Slint desktop runner.

use std::fs;
use std::path::PathBuf;

use crate::DesktopError;

pub fn application_database_path() -> Result<PathBuf, DesktopError> {
    let directory = application_data_root()
        .ok_or(DesktopError::MissingDataDirectory)?
        .join("llm-app");
    fs::create_dir_all(&directory).map_err(DesktopError::CreateDataDirectory)?;
    Ok(directory.join("state.redb"))
}

fn application_data_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(path));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from);
    }

    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        Some(home.join("Library/Application Support"))
    } else {
        Some(home.join(".local/share"))
    }
}
