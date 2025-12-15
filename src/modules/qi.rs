use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::modules::ore::OreKind;
use crate::modules::vm::{Position, Qi};

fn default_ore_kind() -> OreKind {
    OreKind::Qi
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QiSourceSpec {
    pub position: Position,
    pub capacity: Qi,
    pub recharge_per_tick: Qi,
    #[serde(default = "default_ore_kind")]
    pub ore: OreKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QiSourceStore {
    pub sources: Vec<QiSourceSpec>,
    #[serde(default)]
    pub total_qi_infused: u64,
}

fn store_dir() -> PathBuf {
    PathBuf::from(".harimu")
}

fn store_path() -> PathBuf {
    store_dir().join("qi_sources.json")
}

pub fn load() -> io::Result<QiSourceStore> {
    let path = store_path();
    if !path.exists() {
        return Ok(QiSourceStore::default());
    }

    let bytes = fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(QiSourceStore::default());
    }

    let store: QiSourceStore = serde_json::from_slice(&bytes).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse qi source store {}; delete it to reset: {}",
                path.display(),
                e
            ),
        )
    })?;

    Ok(store)
}

pub fn save(store: &QiSourceStore) -> io::Result<()> {
    fs::create_dir_all(store_dir())?;
    let json = serde_json::to_vec_pretty(store)?;
    fs::write(store_path(), json)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Spread {
    pub center: Position,
    pub radius: i32,
}

impl Default for Spread {
    fn default() -> Self {
        Self {
            center: Position::origin(),
            radius: 8,
        }
    }
}
