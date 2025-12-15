use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::modules::vm::DEFAULT_MAX_AGENT_AGE;

fn default_max_age() -> u64 {
    DEFAULT_MAX_AGENT_AGE
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub qi: u64,
    pub companions: u32,
    #[serde(default = "default_max_age")]
    pub max_age: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VoteTally {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentStore {
    pub agents: HashMap<String, AgentProfile>,
    pub votes: HashMap<String, VoteTally>,
}

fn agents_dir() -> PathBuf {
    PathBuf::from(".harimu")
}

fn agents_path() -> PathBuf {
    agents_dir().join("agents.json")
}

pub fn load() -> io::Result<AgentStore> {
    let path = agents_path();
    if !path.exists() {
        return Ok(AgentStore::default());
    }

    let data = fs::read(&path)?;
    if data.is_empty() {
        return Ok(AgentStore::default());
    }

    let store: AgentStore = serde_json::from_slice(&data).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse agents file {}; delete it to reset: {}",
                path.display(),
                e
            ),
        )
    })?;

    Ok(store)
}

pub fn save(store: &AgentStore) -> io::Result<()> {
    fs::create_dir_all(agents_dir())?;
    let json = serde_json::to_vec_pretty(store)?;
    fs::write(agents_path(), json)?;
    Ok(())
}

pub fn create_agent(store: &mut AgentStore, id: String) -> Result<AgentProfile, String> {
    let mut bytes = [0u8; 20];
    OsRng.fill_bytes(&mut bytes);
    let address = hex::encode(bytes);

    if store.agents.contains_key(&id) {
        return Err(format!("agent {} already exists", id));
    }

    let profile = AgentProfile {
        id: address.clone(),
        qi: 0,
        companions: 0,
        max_age: DEFAULT_MAX_AGENT_AGE,
    };
    store.agents.insert(address.clone(), profile.clone());
    Ok(profile)
}

pub fn infuse(store: &mut AgentStore, id: &str, amount: u64) -> Result<(), String> {
    let agent = store
        .agents
        .get_mut(id)
        .ok_or_else(|| format!("agent {} not found", id))?;
    agent.qi = agent.qi.saturating_add(amount);
    Ok(())
}

pub fn extend_life(store: &mut AgentStore, id: &str, max_age: u64) -> Result<(), String> {
    let agent = store
        .agents
        .get_mut(id)
        .ok_or_else(|| format!("agent {} not found", id))?;
    agent.max_age = max_age.max(1);
    Ok(())
}

pub fn spawn_companion(store: &mut AgentStore, id: &str) -> Result<(), String> {
    let agent = store
        .agents
        .get_mut(id)
        .ok_or_else(|| format!("agent {} not found", id))?;
    agent.companions = agent.companions.saturating_add(1);
    Ok(())
}

pub fn remove_agent(store: &mut AgentStore, id: &str) -> Result<(), String> {
    if store.agents.remove(id).is_some() {
        Ok(())
    } else {
        Err(format!("agent {} not found", id))
    }
}

pub fn vote(store: &mut AgentStore, action_id: &str, direction: VoteDirection) {
    let tally = store.votes.entry(action_id.to_string()).or_default();
    match direction {
        VoteDirection::Up => tally.up = tally.up.saturating_add(1),
        VoteDirection::Down => tally.down = tally.down.saturating_add(1),
    }
}

pub fn transfer_qi(
    store: &mut AgentStore,
    from: &str,
    to: &str,
    amount: u64,
) -> Result<(), String> {
    if amount == 0 || from == to {
        return Ok(());
    }

    {
        let from_agent = store
            .agents
            .get_mut(from)
            .ok_or_else(|| format!("agent {} not found", from))?;
        if from_agent.qi < amount {
            return Err(format!(
                "insufficient qi: have {}, need {}",
                from_agent.qi, amount
            ));
        }
        from_agent.qi = from_agent.qi.saturating_sub(amount);
    }

    let to_agent = store
        .agents
        .get_mut(to)
        .ok_or_else(|| format!("agent {} not found", to))?;
    to_agent.qi = to_agent.qi.saturating_add(amount);

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum VoteDirection {
    Up,
    Down,
}
