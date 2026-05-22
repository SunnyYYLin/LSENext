use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("LOCALAPPDATA is not available")]
    MissingLocalAppData,
    #[error("source does not exist: {0}")]
    MissingSource(PathBuf),
    #[error("failed to access path {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize state: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to update Explorer menu registration while {operation}, Win32 error {code}")]
    Registry { operation: &'static str, code: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PickedSource {
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionState {
    pub picked_at_unix: u64,
    pub sources: Vec<PickedSource>,
}

pub fn state_file() -> Result<PathBuf, StateError> {
    let base = env::var_os("LOCALAPPDATA").ok_or(StateError::MissingLocalAppData)?;
    Ok(PathBuf::from(base).join("LSENext").join("state.json"))
}

pub fn save_sources(paths: &[PathBuf]) -> Result<SelectionState, StateError> {
    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = fs::metadata(path).map_err(|source| StateError::Io {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(StateError::MissingSource(path.clone()));
        }
        sources.push(PickedSource {
            path: path.clone(),
            is_dir: metadata.is_dir(),
        });
    }

    let state = SelectionState {
        picked_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        sources,
    };

    let path = state_file()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StateError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let json = serde_json::to_vec_pretty(&state)?;
    fs::write(&path, json).map_err(|source| StateError::Io { path, source })?;
    sync_explorer_menu(Some(&state))?;
    Ok(state)
}

pub fn load_state() -> Result<Option<SelectionState>, StateError> {
    let path = state_file()?;
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(&path).map_err(|source| StateError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(Some(serde_json::from_slice(&data)?))
}

pub fn clear_state() -> Result<(), StateError> {
    let path = state_file()?;
    match fs::remove_file(&path) {
        Ok(()) => sync_explorer_menu(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => sync_explorer_menu(None),
        Err(source) => Err(StateError::Io { path, source }),
    }
}

pub fn validate_target_dir(path: &Path) -> Result<(), StateError> {
    let metadata = fs::metadata(path).map_err(|source| StateError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(StateError::MissingSource(path.to_path_buf()))
    }
}

fn sync_explorer_menu(state: Option<&SelectionState>) -> Result<(), StateError> {
    platform::sync_explorer_menu(state)
}

#[cfg(test)]
fn menu_subcommands(state: Option<&SelectionState>) -> (String, String) {
    const PICK_SOURCE: &str = "LSENext.PickSource";
    const DROP_SYMBOLIC: &str = "LSENext.DropSymbolic";
    const DROP_JUNCTION: &str = "LSENext.DropJunction";
    const DROP_HARDLINK: &str = "LSENext.DropHardLink";
    const BACKGROUND_DROP_SYMBOLIC: &str = "LSENext.BackgroundDropSymbolic";
    const BACKGROUND_DROP_JUNCTION: &str = "LSENext.BackgroundDropJunction";
    const BACKGROUND_DROP_HARDLINK: &str = "LSENext.BackgroundDropHardLink";
    const CLEAR_SOURCE: &str = "LSENext.ClearSource";

    let can_junction = state
        .map(|state| state.sources.iter().all(|source| source.is_dir))
        .unwrap_or(false);
    let can_hardlink = state
        .map(|state| state.sources.iter().all(|source| !source.is_dir))
        .unwrap_or(false);
    let has_state = state.is_some();

    let directory_commands = if has_state && can_junction {
        [PICK_SOURCE, DROP_SYMBOLIC, DROP_JUNCTION, CLEAR_SOURCE].join(";")
    } else if has_state && can_hardlink {
        [PICK_SOURCE, DROP_SYMBOLIC, DROP_HARDLINK, CLEAR_SOURCE].join(";")
    } else if has_state {
        [PICK_SOURCE, DROP_SYMBOLIC, CLEAR_SOURCE].join(";")
    } else {
        PICK_SOURCE.to_string()
    };

    let background_commands = if has_state && can_junction {
        [
            BACKGROUND_DROP_SYMBOLIC,
            BACKGROUND_DROP_JUNCTION,
            CLEAR_SOURCE,
        ]
        .join(";")
    } else if has_state && can_hardlink {
        [
            BACKGROUND_DROP_SYMBOLIC,
            BACKGROUND_DROP_HARDLINK,
            CLEAR_SOURCE,
        ]
        .join(";")
    } else if has_state {
        [BACKGROUND_DROP_SYMBOLIC, CLEAR_SOURCE].join(";")
    } else {
        CLEAR_SOURCE.to_string()
    };

    (directory_commands, background_commands)
}

#[cfg(windows)]
mod platform {
    use super::{SelectionState, StateError};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};

    const FILE_MENU_KEY: &str = r"Software\Classes\*\shell\LSENext";
    const DIRECTORY_MENU_KEY: &str = r"Software\Classes\Directory\shell\LSENext";
    const BACKGROUND_MENU_KEY: &str = r"Software\Classes\Directory\Background\shell\LSENext";

    pub fn sync_explorer_menu(_state: Option<&SelectionState>) -> Result<(), StateError> {
        delete_tree(FILE_MENU_KEY)?;
        delete_tree(DIRECTORY_MENU_KEY)?;
        delete_tree(BACKGROUND_MENU_KEY)?;
        Ok(())
    }

    fn delete_tree(key_path: &str) -> Result<(), StateError> {
        let key_path = wide_null(key_path);
        let result = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key_path.as_ptr()) };
        if result != ERROR_SUCCESS && result != 2 {
            return Err(StateError::Registry {
                operation: "deleting HKCU classic menu key",
                code: result,
            });
        }
        Ok(())
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{SelectionState, StateError};

    pub fn sync_explorer_menu(_state: Option<&SelectionState>) -> Result<(), StateError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_state_round_trips() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\file.txt"),
                is_dir: false,
            }],
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: SelectionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn file_source_menu_subcommands_do_not_include_junction() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\file.txt"),
                is_dir: false,
            }],
        };
        let (directory, background) = menu_subcommands(Some(&state));
        assert_eq!(
            directory,
            "LSENext.PickSource;LSENext.DropSymbolic;LSENext.DropHardLink;LSENext.ClearSource"
        );
        assert_eq!(
            background,
            "LSENext.BackgroundDropSymbolic;LSENext.BackgroundDropHardLink;LSENext.ClearSource"
        );
    }

    #[test]
    fn directory_source_menu_subcommands_include_junction() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\folder"),
                is_dir: true,
            }],
        };
        let (directory, background) = menu_subcommands(Some(&state));
        assert_eq!(
            directory,
            "LSENext.PickSource;LSENext.DropSymbolic;LSENext.DropJunction;LSENext.ClearSource"
        );
        assert_eq!(
            background,
            "LSENext.BackgroundDropSymbolic;LSENext.BackgroundDropJunction;LSENext.ClearSource"
        );
    }
}
