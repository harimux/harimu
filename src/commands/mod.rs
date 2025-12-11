use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use clap::{Parser, Subcommand};
use harimu::{
    agents::{self, VoteDirection},
    state::{self, Status},
    wallet::{self, WalletStore},
    plan_with_llm, Action, ActionArg, ActionRequest, AgentId, BrainMemory, BrainMode, Event,
    LlmClient, Position, StructureKind, TickResult, Vm,
};

#[derive(Parser)]
#[command(
    name = "harimu",
    version,
    about = "Harimu v0.x sandbox CLI (agents, Qi, ticks)",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize local Harimu state
    Init,
    /// Start an agent loop for continuous or bounded ticks
    Start {
        /// Agent address (defaults to first registered agent)
        #[arg(long)]
        agent: Option<String>,
        /// Starting Qi (used if agent is not already in runtime)
        #[arg(long, default_value_t = 3)]
        qi: harimu::Qi,
        /// Starting position as x,y,z (default: 0,0,0)
        #[arg(short = 'p', long, default_value = "0,0,0")]
        position: PositionArg,
        /// Number of ticks to run (omit for continuous)
        #[arg(short = 't', long)]
        ticks: Option<u64>,
        /// Decision driver: loop (deterministic) or llm (mocked planner)
        #[arg(long, default_value_t = BrainMode::Llm, value_enum)]
        brain: BrainMode,
        /// Ollama host (only used when --brain llm)
        #[arg(long, default_value = "http://127.0.0.1:11434")]
        llm_host: String,
        /// Ollama model (only used when --brain llm)
        #[arg(long, default_value = "llama2")]
        llm_model: String,
        /// Ollama timeout in ms (only used when --brain llm)
        #[arg(long, default_value_t = 15_000)]
        llm_timeout_ms: u64,
        /// Desired tick rate (ticks per second). If set, overrides delay-ms.
        #[arg(long)]
        tick_rate: Option<f64>,
        /// Delay between ticks in milliseconds
        #[arg(short = 'd', long, default_value_t = 0, help = "Delay between ticks in ms (used when --tick-rate is not set; default pacing falls back to 1 tick/sec)")]
        delay_ms: u64,
        /// Action (repeatable). Formats: scan | idle | move:<dx>,<dy>,<dz>. Defaults to a simple loop if omitted.
        #[arg(short = 'a', long = "action", value_name = "ACTION")]
        actions: Vec<ActionArg>,
    },
    /// Show runtime status
    Status,
    /// Mark the runtime as stopped
    Stop,
    /// Agent registry operations
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Wallet operations (local, file-backed)
    Wallet {
        #[command(subcommand)]
        command: WalletCommand,
    },
    /// Mine Qi into a wallet using PoW
    Mine {
        /// Optional wallet address (defaults to first wallet)
        #[arg(long)]
        address: Option<String>,
        /// Starting nonce for search
        #[arg(long, default_value_t = 0)]
        start_nonce: u64,
        /// Optional max iterations (omit to mine until interrupted)
        #[arg(long)]
        iterations: Option<u64>,
        /// Delay between solutions in milliseconds
        #[arg(long, default_value_t = 0)]
        delay_ms: u64,
    },
}

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Create a new agent entry (hash ignored; address is generated)
    Create,
    /// Show info for an agent
    Info { hash: String },
    /// List all agents
    List,
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

#[derive(Clone, Copy, Debug)]
pub struct PositionArg(pub Position);

impl FromStr for PositionArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.trim().split(',').collect();
        if parts.len() != 3 {
            return Err("Position must be formatted as x,y,z".into());
        }

        let x = parts[0]
            .trim()
            .parse::<i32>()
            .map_err(|_| "x must be an integer")?;
        let y = parts[1]
            .trim()
            .parse::<i32>()
            .map_err(|_| "y must be an integer")?;
        let z = parts[2]
            .trim()
            .parse::<i32>()
            .map_err(|_| "z must be an integer")?;

        Ok(PositionArg(Position { x, y, z }))
    }
}

pub fn run() {
    let cli = Cli::parse();
    if let Err(err) = dispatch(cli.command) {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn dispatch(command: Command) -> Result<(), String> {
    match command {
        Command::Init => run_init(),
        Command::Start {
            agent,
            qi,
            position,
            ticks,
            brain,
            llm_host,
            llm_model,
            llm_timeout_ms,
            tick_rate,
            delay_ms,
            actions,
        } => run_start(
            agent,
            qi,
            position.0,
            ticks,
            brain,
            llm_host,
            llm_model,
            llm_timeout_ms,
            tick_rate,
            delay_ms,
            actions,
        ),
        Command::Status => run_status(),
        Command::Stop => run_stop(),
        Command::Agent { command } => run_agent(command),
        Command::Wallet { command } => run_wallet(command),
        Command::Mine {
            address,
            start_nonce,
            iterations,
            delay_ms,
        } => run_wallet_mine(address, start_nonce, iterations, delay_ms),
    }
}

fn run_init() -> Result<(), String> {
    state::init_state().map_err(|e| e.to_string())?;
    println!("Initialized state at {}", state::state_file_path().display());
    Ok(())
}

fn run_status() -> Result<(), String> {
    match state::load_state().map_err(|e| e.to_string())? {
        None => {
            println!("Status: not initialized. Run `harimu init`.");
        }
        Some(state) => {
            println!(
                "Status: {:?} | last_tick={} | message={}",
                state.status,
                state.last_tick,
                state.message.unwrap_or_else(|| "-".into())
            );
        }
    }
    Ok(())
}

fn run_stop() -> Result<(), String> {
    let current = state::load_state().map_err(|e| e.to_string())?;
    let Some(prev) = current else {
        return Err("Not initialized. Run `harimu init` first.".into());
    };
    let updated = state::set_status(
        Status::Stopped,
        prev.last_tick,
        Some("stopped by user".into()),
    )
    .map_err(|e| e.to_string())?;
    println!("Stopped. last_tick={}", updated.last_tick);
    Ok(())
}

fn run_wallet(cmd: WalletCommand) -> Result<(), String> {
    let mut store = WalletStore::load().map_err(|e| e.to_string())?;

    match cmd {
        WalletCommand::Create => {
            let wallet = wallet::create_wallet().map_err(|e| e.to_string())?;
            store.upsert_wallet(wallet.clone());
            store.save().map_err(|e| e.to_string())?;
            println!("Created wallet: {}", wallet.address);
        }
        WalletCommand::Balance { address } => {
            let addr = if let Some(addr) = address {
                addr
            } else {
                store
                    .first_wallet()
                    .map(|w| w.address.clone())
                    .ok_or_else(|| "no wallets found; create one first".to_string())?
            };

            let wallet = store
                .get_wallet(&addr)
                .ok_or_else(|| format!("wallet {} not found", addr))?;
            println!("Wallet {} balance: {} Qi", wallet.address, wallet.balance);
        }
        WalletCommand::Transfer { from, to, amount } => {
            wallet::transfer(&mut store, &from, &to, amount)?;
            store.save().map_err(|e| e.to_string())?;
            println!(
                "Transferred {} Qi from {} to {}",
                amount, from, to
            );
        }
    }

    Ok(())
}

fn run_agent(cmd: AgentCommand) -> Result<(), String> {
    let mut store = agents::load().map_err(|e| e.to_string())?;

    match cmd {
        AgentCommand::Create => {
            let profile = agents::create_agent(&mut store, String::new()).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            println!("Created agent {} (qi={}, companions={})", profile.id, profile.qi, profile.companions);
        }
        AgentCommand::Info { hash } => {
            let profile = store
                .agents
                .get(&hash)
                .ok_or_else(|| format!("agent {} not found", hash))?;
            println!("Agent {} | qi={} | companions={}", profile.id, profile.qi, profile.companions);
        }
        AgentCommand::List => {
            if store.agents.is_empty() {
                println!("No agents found");
            } else {
                for agent in store.agents.values() {
                    println!("{} | qi={} | companions={}", agent.id, agent.qi, agent.companions);
                }
            }
        }
        AgentCommand::Spawn { hash } => {
            agents::spawn_companion(&mut store, &hash).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            println!("Spawned companion for agent {}", hash);
        }
        AgentCommand::Vote { action_id, direction } => {
            let dir = match direction {
                VoteDirectionArg::Up => VoteDirection::Up,
                VoteDirectionArg::Down => VoteDirection::Down,
            };
            agents::vote(&mut store, &action_id, dir);
            agents::save(&store).map_err(|e| e.to_string())?;
            let tally = store.votes.get(&action_id).cloned().unwrap_or_default();
            println!("Vote recorded for action {}: up={} down={}", action_id, tally.up, tally.down);
        }
        AgentCommand::Infuse { agent_id, amount } => {
            agents::infuse(&mut store, &agent_id, amount).map_err(|e| e.to_string())?;
            agents::save(&store).map_err(|e| e.to_string())?;
            let profile = store.agents.get(&agent_id).unwrap();
            println!("Infused {} Qi into agent {} (new qi={})", amount, agent_id, profile.qi);
        }
    }

    Ok(())
}

fn run_wallet_mine(
    address: Option<String>,
    start_nonce: u64,
    iterations: Option<u64>,
    delay_ms: u64,
) -> Result<(), String> {
    let mut store = WalletStore::load().map_err(|e| e.to_string())?;
    let address = if let Some(addr) = address {
        addr
    } else {
        store
            .first_wallet()
            .map(|w| w.address.clone())
            .ok_or_else(|| "no wallets found; create one first".to_string())?
    };
    let mut nonce = start_nonce;
    let mut mined = 0u64;

    println!(
        "Mining for wallet {} starting at nonce {} (difficulty {} leading zero byte(s))",
        address,
        start_nonce,
        harimu::POW_DIFFICULTY_BYTES
    );

    loop {
        let (found_nonce, reward) = wallet::mine(&mut store, &address, nonce)?;
        store.save().map_err(|e| e.to_string())?;

        mined = mined.saturating_add(1);
        println!(
            "[{}] Mined {} Qi with nonce {} | total_mined={} | balance={}",
            mined,
            reward,
            found_nonce,
            mined,
            store
                .get_wallet(&address)
                .map(|w| w.balance)
                .unwrap_or(0)
        );

        match iterations {
            Some(limit) if mined >= limit => break,
            _ => {}
        }

        nonce = found_nonce.wrapping_add(1);

        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
    }

    Ok(())
}

fn run_start(
    agent: Option<String>,
    qi: harimu::Qi,
    position: Position,
    ticks: Option<u64>,
    brain: BrainMode,
    llm_host: String,
    llm_model: String,
    llm_timeout_ms: u64,
    tick_rate: Option<f64>,
    delay_ms: u64,
    actions: Vec<ActionArg>,
) -> Result<(), String> {
    const DEFAULT_TICK_RATE: f64 = 1.0;

    let prior_state = match state::load_state().map_err(|e| e.to_string())? {
        Some(s) => Some(s),
        None => {
            let initialized = state::init_state().map_err(|e| e.to_string())?;
            println!(
                "State not found; initialized new state at {} (status={:?})",
                state::state_file_path().display(),
                initialized.status
            );
            Some(initialized)
        }
    };

    let mut vm = Vm::new();
    if let Some(s) = prior_state.as_ref() {
        if s.last_tick > 0 {
            vm.set_tick(s.last_tick);
            println!("Resuming from tick {}", s.last_tick);
        }
    }

    // Load agents; either run all or a specific one.
    let registry = agents::load().map_err(|e| e.to_string())?;
    let mut agent_ids = Vec::new();

    if let Some(addr) = agent {
        let agent_qi = registry
            .agents
            .get(&addr)
            .map(|a| a.qi as harimu::Qi)
            .unwrap_or(qi);
        let id = vm.spawn_agent(addr, agent_qi, position);
        agent_ids.push(id);
    } else {
        if registry.agents.is_empty() {
            return Err("no agents found; create one with `harimu agent create`".to_string());
        }
        for (addr, profile) in registry.agents.iter() {
            let id = vm.spawn_agent(addr.clone(), profile.qi as harimu::Qi, position);
            agent_ids.push(id);
        }
    }

    let action_cycle: Vec<ActionArg> = if actions.is_empty() {
        match brain {
            BrainMode::Loop => default_loop_actions(&agent_ids),
            BrainMode::Llm => default_llm_actions(&agent_ids),
        }
    } else {
        actions
    };

    let effective_delay = match tick_rate {
        Some(rate) => {
            if rate <= 0.0 {
                return Err("tick_rate must be greater than 0".into());
            }
            Duration::from_secs_f64(1.0 / rate)
        }
        None => {
            if delay_ms > 0 {
                Duration::from_millis(delay_ms)
            } else {
                Duration::from_secs_f64(1.0 / DEFAULT_TICK_RATE)
            }
        }
    };

    state::set_status(Status::Running, vm.world().tick(), Some("agent loop running".into()))
        .map_err(|e| e.to_string())?;

    match brain {
        BrainMode::Loop => run_loop(&agent_ids, &action_cycle, ticks, effective_delay, &mut vm)?,
        BrainMode::Llm => {
            let client = LlmClient::new(
                llm_host,
                llm_model,
                Duration::from_millis(llm_timeout_ms),
            )
            .map_err(|e| format!("ollama client: {}", e))?;

            run_llm_loop(
                &agent_ids,
                &action_cycle,
                ticks,
                effective_delay,
                &mut vm,
                client,
            )?
        }
    }

    state::set_status(
        Status::Stopped,
        vm.world().tick(),
        Some(format!("completed {} tick(s)", vm.world().tick())),
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn build_requests(
    agent_id: AgentId,
    partner: Option<AgentId>,
    actions: &[ActionArg],
    next_tick: u64,
) -> Vec<ActionRequest> {
    actions
        .iter()
        .map(|arg| {
            let mut action = arg.materialize(agent_id, next_tick);
            if let Action::Reproduce { partner: p } = action {
                if p == 0 {
                    if let Some(actual) = partner {
                        action = Action::Reproduce { partner: actual };
                    }
                }
            }
            ActionRequest::new(agent_id, action)
        })
        .collect()
}

fn run_loop(
    agent_ids: &[AgentId],
    action_cycle: &[ActionArg],
    ticks: Option<u64>,
    delay: Duration,
    vm: &mut Vm,
) -> Result<(), String> {
    let mut remaining = ticks;
    loop {
        let next_tick = vm.world().tick() + 1;
        let mut requests = Vec::new();
        for agent_id in agent_ids {
            let partner = agent_ids.iter().find(|&&id| id != *agent_id).copied();
            requests.extend(build_requests(*agent_id, partner, action_cycle, next_tick));
        }

        let tick = vm.step(&requests);
        println!("Tick {}", tick.tick);
        for agent_id in agent_ids {
            print_tick(&tick, vm, *agent_id);
        }

        state::set_status(
            Status::Running,
            vm.world().tick(),
            Some("agent loop running".into()),
        )
        .map_err(|e| e.to_string())?;

        if agent_ids
            .iter()
            .all(|id| vm.world().agent(*id).map(|a| !a.alive).unwrap_or(true))
        {
            break;
        }

        match remaining {
            Some(0) => break,
            Some(ref mut n) => {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    break;
                }
            }
            None => {}
        }

        if delay > Duration::ZERO {
            std::thread::sleep(delay);
        }
    }

    Ok(())
}

fn run_llm_loop(
    agent_ids: &[AgentId],
    action_cycle: &[ActionArg],
    ticks: Option<u64>,
    delay: Duration,
    vm: &mut Vm,
    client: LlmClient,
) -> Result<(), String> {
    let mut remaining = ticks;
    let mut memories: HashMap<AgentId, BrainMemory> = HashMap::new();

    loop {
        let next_tick = vm.world().tick() + 1;
        let mut requests = Vec::new();

        for agent_id in agent_ids {
            let memory = memories.entry(*agent_id).or_default();
            let partner = agent_ids.iter().find(|&&id| id != *agent_id).copied();
            let decision = plan_with_llm(
                vm,
                *agent_id,
                action_cycle,
                memory,
                Some(&client),
                next_tick,
            );

            println!("Tick {} | LLM planner | Agent {}", vm.world().tick() + 1, agent_id);
            println!(" 1) State     : {}", decision.summary);
            println!(" 2) Goal      : {}", harimu::DEFAULT_AGENT_GOAL);
            println!(" 3) Prompt    : {}", decision.prompt);
            println!(" 4) LLM reply : {}", decision.response);
            println!(" 5) Decision  : {:?}", decision.action);
            println!(" 6) Tx        : signed+submitted (simulated)");
            println!(" 7) Memory    : {} notes", memory.notes.len());

            let mut action = decision.action;
            if let Action::Reproduce { partner: p } = action {
                if p == 0 {
                    if let Some(actual) = partner {
                        action = Action::Reproduce { partner: actual };
                    }
                }
            }

            requests.push(ActionRequest::new(*agent_id, action));
        }

        if requests.is_empty() {
            break;
        }

        let tick = vm.step(&requests);
        for agent_id in agent_ids {
            print_tick(&tick, vm, *agent_id);
        }

        state::set_status(
            Status::Running,
            vm.world().tick(),
            Some("agent loop running (llm)".into()),
        )
        .map_err(|e| e.to_string())?;

        if agent_ids
            .iter()
            .all(|id| vm.world().agent(*id).map(|a| !a.alive).unwrap_or(true))
        {
            break;
        }

        match remaining {
            Some(0) => break,
            Some(ref mut n) => {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    break;
                }
            }
            None => {}
        }

        if delay > Duration::ZERO {
            std::thread::sleep(delay);
        }
    }

    Ok(())
}

fn default_llm_actions(agent_ids: &[AgentId]) -> Vec<ActionArg> {
    let mut actions = default_loop_actions(agent_ids);
    actions.push(ActionArg::BuildStructure {
        kind: StructureKind::Basic,
    });
    actions
}

fn default_loop_actions(agent_ids: &[AgentId]) -> Vec<ActionArg> {
    let mut actions = vec![
        ActionArg::Move { dx: 1, dy: 0, dz: 0 },
        ActionArg::Move { dx: 0, dy: 1, dz: 0 },
        ActionArg::Move { dx: 0, dy: 0, dz: 1 },
        ActionArg::Scan,
    ];

    if agent_ids.len() > 1 {
        let partner = agent_ids[0];
        actions.push(ActionArg::Reproduce { partner });
    }

    actions
}

fn print_tick(tick: &TickResult, vm: &Vm, agent_id: AgentId) {
    println!(
        "Tick {}: {} events, {} rejections",
        tick.tick,
        tick.events.len(),
        tick.rejections.len()
    );

    for event in &tick.events {
        println!(" - {}", describe_event(event));
    }

    if !tick.rejections.is_empty() {
        println!("Rejections:");
        for rejection in &tick.rejections {
            println!(
                " - agent {} action {:?}: {:?}",
                rejection.request.agent_id, rejection.request.action, rejection.error
            );
        }
    }

    if let Some(agent) = vm.world().agent(agent_id) {
        println!(
            "Agent {} (id {}) | qi={} | position=({}, {}, {}) | alive={} | age={}",
            agent.name,
            agent.id,
            agent.qi,
            agent.position.x,
            agent.position.y,
            agent.position.z,
            agent.alive,
            agent.age
        );
        let (structures_built, offspring) = agent_counters(vm, agent_id);
        println!(
            "Summary: structures_built={} | offspring={} | events_seen={}",
            structures_built,
            offspring,
            vm.world().events().len()
        );
    }
}

fn describe_event(event: &Event) -> String {
    match event {
        Event::TickStarted { tick } => format!("tick {} started", tick),
        Event::TickCompleted { tick } => format!("tick {} completed", tick),
        Event::AgentSpawned {
            agent_id,
            name,
            qi,
            position,
        } => format!(
            "agent {} ({}) spawned with qi={} at ({}, {}, {})",
            name, agent_id, qi, position.x, position.y, position.z
        ),
        Event::QiSpent {
            agent_id,
            amount,
            action,
        } => format!("agent {} spent {} qi on {}", agent_id, amount, action),
        Event::QiGained {
            agent_id,
            amount,
            source,
        } => format!("agent {} gained {} qi from {}", agent_id, amount, source),
        Event::AgentMoved { agent_id, from, to } => format!(
            "agent {} moved from ({}, {}, {}) to ({}, {}, {})",
            agent_id, from.x, from.y, from.z, to.x, to.y, to.z
        ),
        Event::AgentDied {
            agent_id,
            reason,
        } => format!("agent {} died: {:?}", agent_id, reason),
        Event::AgentReproduced {
            parent_a,
            parent_b,
            child_id,
        } => format!(
            "agents {} and {} reproduced; child={}",
            parent_a, parent_b, child_id
        ),
        Event::StructureBuilt {
            agent_id,
            kind,
            position,
            structure_id,
        } => format!(
            "agent {} built {} structure {} at ({}, {}, {})",
            agent_id, kind, structure_id, position.x, position.y, position.z
        ),
        Event::ActionObserved { agent_id, action } => {
            format!("agent {} observed action {}", agent_id, action)
        }
        Event::ScanReport {
            agent_id,
            position,
            qi,
            nearby_qi_sources,
            nearby_structures,
        } => format!(
            "agent {} scan at ({}, {}, {}) qi={} | qi_sources={} | structures={}",
            agent_id,
            position.x,
            position.y,
            position.z,
            qi,
            nearby_qi_sources.len(),
            nearby_structures.len()
        ),
    }
}

fn agent_counters(vm: &Vm, agent_id: AgentId) -> (usize, usize) {
    let mut structures = 0usize;
    let mut offspring = 0usize;
    for event in vm.world().events() {
        match event {
            Event::StructureBuilt { agent_id: a, .. } if *a == agent_id => structures += 1,
            Event::AgentReproduced { parent_a, parent_b, .. } if *parent_a == agent_id || *parent_b == agent_id => {
                offspring += 1
            }
            _ => {}
        }
    }
    (structures, offspring)
}
#[derive(Subcommand)]
pub enum WalletCommand {
    /// Create a new wallet (random address)
    Create,
    /// Check balance for a wallet
    Balance {
        /// Wallet address (defaults to first wallet if omitted)
        #[arg(long)]
        address: Option<String>,
    },
    /// Transfer Qi between wallets
    Transfer {
        /// Sender address
        #[arg(long)]
        from: String,
        /// Recipient address
        #[arg(long)]
        to: String,
        /// Amount of Qi to transfer
        #[arg(long)]
        amount: harimu::Qi,
    },
}
