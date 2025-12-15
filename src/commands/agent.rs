use std::str::FromStr;

use clap::Subcommand;
use harimu::agents::{self, VoteDirection};

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Create a new agent entry (hash ignored; address is generated)
    Create,
    /// Show info for an agent
    Info { hash: String },
    /// List all agents
    List,
    /// Remove an agent entry
    Remove { hash: String },
    /// Spawn a companion for an agent
    Spawn { hash: String },
    /// Vote on an action id (hash) up/down
    Vote {
        action_id: String,
        direction: VoteDirectionArg,
    },
    /// Infuse Qi ("water") into an agent
    Infuse {
        agent_id: String,
        #[arg(long)]
        amount: u64,
    },
    /// Extend an agent's lifespan (in ticks)
    ExtendLife {
        #[arg(long)]
        agent_id: String,
        /// New max age in ticks
        #[arg(long, default_value_t = harimu::DEFAULT_MAX_AGENT_AGE)]
        max_age: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoteDirectionArg {
    Up,
    Down,
}

impl FromStr for VoteDirectionArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "up" => Ok(VoteDirectionArg::Up),
            "down" => Ok(VoteDirectionArg::Down),
            other => Err(format!("unknown vote direction '{}', use up|down", other)),
        }
    }
}

pub(super) fn run_agent(cmd: AgentCommand) -> Result<(), String> {
    let mut store = agents::load().map_err(|e| e.to_string())?;

    match cmd {
        AgentCommand::Create => {
            let profile =
                agents::create_agent(&mut store, String::new()).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            println!(
                "Created agent {} (qi={}, companions={})",
                profile.id, profile.qi, profile.companions
            );
        }
        AgentCommand::Info { hash } => {
            let profile = store
                .agents
                .get(&hash)
                .ok_or_else(|| format!("agent {} not found", hash))?;
            println!(
                "Agent {} | qi={} | companions={} | max_age={}",
                profile.id, profile.qi, profile.companions, profile.max_age
            );
        }
        AgentCommand::List => {
            if store.agents.is_empty() {
                println!("No agents found");
            } else {
                for agent in store.agents.values() {
                    println!(
                        "{} | qi={} | companions={} | max_age={}",
                        agent.id, agent.qi, agent.companions, agent.max_age
                    );
                }
            }
        }
        AgentCommand::Remove { hash } => {
            agents::remove_agent(&mut store, &hash).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            println!("Removed agent {}", hash);
        }
        AgentCommand::Spawn { hash } => {
            agents::spawn_companion(&mut store, &hash).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            println!("Spawned companion for agent {}", hash);
        }
        AgentCommand::Vote {
            action_id,
            direction,
        } => {
            let dir = match direction {
                VoteDirectionArg::Up => VoteDirection::Up,
                VoteDirectionArg::Down => VoteDirection::Down,
            };
            agents::vote(&mut store, &action_id, dir);
            agents::save(&store).map_err(|e| e.to_string())?;
            let tally = store.votes.get(&action_id).cloned().unwrap_or_default();
            println!(
                "Vote recorded for action {}: up={} down={}",
                action_id, tally.up, tally.down
            );
        }
        AgentCommand::Infuse { agent_id, amount } => {
            agents::infuse(&mut store, &agent_id, amount).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            let profile = store.agents.get(&agent_id).unwrap();
            println!(
                "Infused {} Qi into agent {} (new qi={})",
                amount, agent_id, profile.qi
            );
        }
        AgentCommand::ExtendLife { agent_id, max_age } => {
            agents::extend_life(&mut store, &agent_id, max_age).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            let profile = store.agents.get(&agent_id).unwrap();
            println!(
                "Extended lifespan for agent {} to {} ticks",
                agent_id, profile.max_age
            );
        }
    }

    Ok(())
}
