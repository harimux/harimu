use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;

use crate::modules::vm::{AgentId, Position, Zone};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureKind {
    Basic,
    Programmable,
    Qi,
}

impl fmt::Display for StructureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructureKind::Basic => write!(f, "basic"),
            StructureKind::Programmable => write!(f, "programmable"),
            StructureKind::Qi => write!(f, "qi"),
        }
    }
}

impl FromStr for StructureKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "basic" => Ok(StructureKind::Basic),
            "programmable" => Ok(StructureKind::Programmable),
            "qi" | "qi-node" | "qinode" | "qi_node" => Ok(StructureKind::Qi),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Structure {
    pub id: u64,
    pub kind: StructureKind,
    pub position: Position,
    pub zone: Zone,
    pub owner: AgentId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureRecord {
    pub id: u64,
    pub kind: StructureKind,
    pub position: Position,
    pub zone: Zone,
    pub owner: AgentId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureStore {
    pub structures: Vec<StructureRecord>,
}

fn store_dir() -> PathBuf {
    PathBuf::from(".harimu")
}

fn store_path() -> PathBuf {
    store_dir().join("structures.json")
}

pub fn load_structure_store() -> io::Result<StructureStore> {
    let path = store_path();
    if !path.exists() {
        return Ok(StructureStore::default());
    }

    let bytes = fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(StructureStore::default());
    }

    let store: StructureStore = serde_json::from_slice(&bytes).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse structure store {}; delete it to reset: {}",
                path.display(),
                e
            ),
        )
    })?;

    Ok(store)
}

pub fn save_structure_store(store: &StructureStore) -> io::Result<()> {
    fs::create_dir_all(store_dir())?;
    let json = serde_json::to_vec_pretty(store)?;
    fs::write(store_path(), json)?;
    Ok(())
}
