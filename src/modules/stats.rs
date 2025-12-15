use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::modules::vm::{Action, AgentId};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionStats {
    pub move_count: u64,
    pub scan_count: u64,
    pub build_count: u64,
    pub harvest_count: u64,
    pub reproduce_count: u64,
    pub idle_count: u64,
}

impl ActionStats {
    pub fn record(&mut self, action: &Action) {
        match action {
            Action::Move { .. } => self.move_count = self.move_count.saturating_add(1),
            Action::Scan => self.scan_count = self.scan_count.saturating_add(1),
            Action::BuildStructure { .. } => self.build_count = self.build_count.saturating_add(1),
            Action::HarvestOre { .. } => self.harvest_count = self.harvest_count.saturating_add(1),
            Action::Reproduce { .. } => self.reproduce_count = self.reproduce_count.saturating_add(1),
            Action::Idle => self.idle_count = self.idle_count.saturating_add(1),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionStatsStore {
    pub per_agent: HashMap<AgentId, ActionStats>,
}

fn stats_dir() -> PathBuf {
    PathBuf::from(".harimu")
}

fn stats_path() -> PathBuf {
    stats_dir().join("action_stats.json")
}

pub fn reset_action_stats() -> io::Result<()> {
    let store = ActionStatsStore::default();
    save_action_stats(&store)
}

pub fn load_action_stats() -> io::Result<ActionStatsStore> {
    let path = stats_path();
    if !path.exists() {
        return Ok(ActionStatsStore::default());
    }

    let bytes = fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(ActionStatsStore::default());
    }

    let store: ActionStatsStore = serde_json::from_slice(&bytes)?;
    Ok(store)
}

pub fn save_action_stats(store: &ActionStatsStore) -> io::Result<()> {
    let dir = stats_dir();
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(store)?;
    fs::write(stats_path(), json)?;
    Ok(())
}

pub fn record_successful_actions(
    store: &mut ActionStatsStore,
    agent_id: AgentId,
    actions: impl Iterator<Item = Action>,
) {
    let stats = store.per_agent.entry(agent_id).or_default();
    for action in actions {
        stats.record(&action);
    }
}
