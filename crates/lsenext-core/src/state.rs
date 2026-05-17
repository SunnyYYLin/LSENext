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
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
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
}
