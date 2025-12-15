use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::modules::ore::OreKind;
use crate::modules::structure::{StructureKind, StructureRecord, load_structure_store};
use crate::modules::vm::{AgentId, Position, Qi, DEFAULT_MAX_AGENT_AGE};
use crate::modules::world::WorldQueries;

fn default_max_age() -> u64 {
    DEFAULT_MAX_AGENT_AGE
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub id: AgentId,
    pub name: String,
    pub qi: Qi,
    pub transistors: Qi,
    pub position: Position,
    pub alive: bool,
    pub age: u64,
    #[serde(default = "default_max_age")]
    pub max_age: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OreNodeSnapshot {
    pub id: u64,
    pub ore: OreKind,
    pub position: Position,
    pub available: Qi,
    pub capacity: Qi,
    pub recharge_per_tick: Qi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureView {
    pub id: u64,
    pub kind: StructureKind,
    pub position: Position,
    pub owner: AgentId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub tick: u64,
    pub agents: Vec<AgentSnapshot>,
    pub ore_nodes: Vec<OreNodeSnapshot>,
    pub structures: Vec<StructureView>,
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(".harimu")
}

pub fn snapshot_file_path() -> PathBuf {
    snapshot_dir().join("world_snapshot.json")
}

pub fn snapshots_dir() -> PathBuf {
    snapshot_dir().join("world_snapshots")
}

pub fn save_world_snapshot(snapshot: &WorldSnapshot) -> io::Result<PathBuf> {
    let path = snapshot_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(snapshot)?;
    fs::write(&path, json)?;
    Ok(path)
}

pub fn save_world_snapshot_tick(snapshot: &WorldSnapshot) -> io::Result<PathBuf> {
    let dir = snapshots_dir();
    fs::create_dir_all(&dir)?;
    let filename = format!("tick_{:06}.json", snapshot.tick);
    let path = dir.join(filename);
    let json = serde_json::to_vec_pretty(snapshot)?;
    fs::write(&path, json)?;
    Ok(path)
}

pub fn load_world_snapshot() -> io::Result<Option<WorldSnapshot>> {
    let path = snapshot_file_path();
    if !path.exists() {
        return load_latest_snapshot_from_dir();
    }
    let bytes = fs::read(&path)?;
    if bytes.is_empty() {
        return load_latest_snapshot_from_dir();
    }
    let snapshot = serde_json::from_slice(&bytes)?;
    Ok(Some(snapshot))
}

pub fn load_latest_snapshot_from_dir() -> io::Result<Option<WorldSnapshot>> {
    let dir = snapshots_dir();
    let mut latest: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                match &latest {
                    Some(current) => {
                        if path > *current {
                            latest = Some(path);
                        }
                    }
                    None => latest = Some(path),
                }
            }
        }
    }

    let Some(path) = latest else {
        return Ok(None);
    };

    let bytes = fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let snapshot = serde_json::from_slice(&bytes)?;
    Ok(Some(snapshot))
}

pub fn snapshot_from_persistent() -> Result<WorldSnapshot, String> {
    let ore_store = WorldQueries::qi_sources().map_err(|e| e.to_string())?;
    let structure_store = load_structure_store().map_err(|e| e.to_string())?;

    let mut ore_nodes: Vec<OreNodeSnapshot> = ore_store
        .sources
        .iter()
        .enumerate()
        .map(|(idx, src)| OreNodeSnapshot {
            id: (idx + 1) as u64,
            ore: src.ore,
            position: src.position,
            available: src.capacity,
            capacity: src.capacity,
            recharge_per_tick: src.recharge_per_tick,
        })
        .collect();

    let mut structures: Vec<StructureView> = structure_store
        .structures
        .iter()
        .map(|s: &StructureRecord| StructureView {
            id: s.id,
            kind: s.kind,
            position: s.position,
            owner: s.owner,
        })
        .collect();

    ore_nodes.sort_by_key(|n| n.id);
    structures.sort_by_key(|s| s.id);

    Ok(WorldSnapshot {
        tick: 0,
        agents: Vec::new(),
        ore_nodes,
        structures,
    })
}
