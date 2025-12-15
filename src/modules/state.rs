use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Initialized,
    Running,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeState {
    pub status: Status,
    pub last_tick: u64,
    pub message: Option<String>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            status: Status::Initialized,
            last_tick: 0,
            message: None,
        }
    }
}

fn state_dir() -> PathBuf {
    PathBuf::from(".harimu")
}

fn state_path() -> PathBuf {
    state_dir().join("state.json")
}

pub fn state_file_path() -> PathBuf {
    state_path()
}

pub fn init_state() -> io::Result<RuntimeState> {
    let state = RuntimeState::default();
    save_state(&state)?;
    Ok(state)
}

pub fn load_state() -> io::Result<Option<RuntimeState>> {
    let path = state_path();
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Ok(None);
    }

    let state: RuntimeState = serde_json::from_slice(&bytes).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse state file {}; delete it or run `harimu init` to reset: {}",
                state_path().display(),
                e
            ),
        )
    })?;
    Ok(Some(state))
}

pub fn save_state(state: &RuntimeState) -> io::Result<()> {
    let dir = state_dir();
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(state)?;
    fs::write(state_path(), json)?;
    Ok(())
}

pub fn set_status(
    status: Status,
    last_tick: u64,
    message: Option<String>,
) -> io::Result<RuntimeState> {
    let mut state = load_state()?.unwrap_or_default();
    state.status = status;
    state.last_tick = last_tick;
    state.message = message;
    save_state(&state)?;
    Ok(state)
}
