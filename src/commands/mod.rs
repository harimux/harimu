use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use clap::{ArgAction, Parser, Subcommand};
use harimu::{
    Action, ActionArg, ActionRequest, AgentId, BrainMemory, BrainMode, Event, LlmClient,
    LlmProvider, OreKind, Position, StructureKind, StructureRecord, TickResult, Vm, agents,
    load_structure_store, plan_with_llm, record_successful_actions, reset_action_stats,
    save_action_stats, save_structure_store, save_world_snapshot, save_world_snapshot_tick,
    state::{self, Status},
    world::WorldQueries,
};

mod agent;
mod wallet;
mod world;

use agent::{run_agent, AgentCommand};
use wallet::{run_wallet, run_wallet_mine, WalletCommand};
use world::{run_world, WorldCommand};

const PID_FILE: &str = ".harimu/runtime.pid";

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
        /// LLM host/base URL (default OpenAI endpoint)
        #[arg(long, default_value = "https://api.openai.com")]
        llm_host: String,
        /// Model name (e.g., gpt-5-nano, gpt-4o-mini, glm-4.6:cloud). Interpreted by the selected provider.
        #[arg(long, default_value = "gpt-5-nano")]
        llm_model: String,
        /// LLM timeout in ms
        #[arg(long, default_value_t = 15_000)]
        llm_timeout_ms: u64,
        /// LLM provider: openai (default; OpenAI-style /v1/chat/completions) or ollama (local /api/chat)
        #[arg(long, default_value_t = LlmProvider::Openai, value_enum)]
        llm_provider: LlmProvider,
        /// API key for OpenAI-compatible providers (also reads LLM_API_KEY env var)
        #[arg(long)]
        llm_api_key: Option<String>,
        /// Desired tick rate (ticks per second). If set, overrides delay-ms.
        #[arg(long)]
        tick_rate: Option<f64>,
        /// Delay between ticks in milliseconds
        #[arg(
            short = 'd',
            long,
            default_value_t = 0,
            help = "Delay between ticks in ms (used when --tick-rate is not set; default pacing falls back to 1 tick/sec)"
        )]
        delay_ms: u64,
        /// Action (repeatable). Formats: scan | idle | move:<dx>,<dy>,<dz>. Defaults to a simple loop if omitted.
        #[arg(short = 'a', long = "action", value_name = "ACTION")]
        actions: Vec<ActionArg>,
        /// Run in the foreground (default is background)
        #[arg(long, action = ArgAction::SetTrue, default_value_t = false)]
        foreground: bool,
        /// Internal flag for background child process (do not use directly)
        #[arg(long, hide = true, default_value_t = false)]
        background_child: bool,
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
    /// World operations (Qi sources, nodes)
    World {
        #[command(subcommand)]
        command: WorldCommand,
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
            llm_provider,
            llm_api_key,
            tick_rate,
            delay_ms,
            actions,
            foreground,
            background_child,
        } => run_start(
            agent,
            qi,
            position.0,
            ticks,
            brain,
            llm_host,
            llm_model,
            llm_timeout_ms,
            llm_provider,
            llm_api_key,
            tick_rate,
            delay_ms,
            actions,
            foreground,
            background_child,
        ),
        Command::Status => run_status(),
        Command::Stop => run_stop(),
        Command::Agent { command } => run_agent(command),
        Command::Wallet { command } => run_wallet(command),
        Command::World { command } => run_world(command),
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
    println!(
        "Initialized state at {}",
        state::state_file_path().display()
    );
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
    print_action_summary()?;
    try_kill_background_process();
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
    llm_provider: LlmProvider,
    llm_api_key: Option<String>,
    tick_rate: Option<f64>,
    delay_ms: u64,
    actions: Vec<ActionArg>,
    foreground: bool,
    background_child: bool,
) -> Result<(), String> {
    let background = !foreground;
    if background && !background_child {
        return launch_background_start(
            agent,
            qi,
            position,
            ticks,
            brain,
            llm_host,
            llm_model,
            llm_timeout_ms,
            llm_provider,
            llm_api_key,
            tick_rate,
            delay_ms,
            actions,
        );
    }

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
    reset_action_stats().map_err(|e| format!("reset stats: {}", e))?;

    let qi_store = WorldQueries::qi_sources()?;
    if !qi_store.sources.is_empty() {
        vm.set_max_qi_supply(qi_store.total_qi_infused);
        for src in &qi_store.sources {
            vm.seed_ore_source(src.ore, src.position, src.capacity, src.recharge_per_tick);
        }
        println!(
            "Seeded {} ore node(s) into the world",
            qi_store.sources.len()
        );
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
        let max_age = registry
            .agents
            .get(&addr)
            .map(|a| a.max_age)
            .unwrap_or(harimu::DEFAULT_MAX_AGENT_AGE);
        let id = vm.spawn_agent_with_age(addr, agent_qi, position, max_age);
        agent_ids.push(id);
    } else {
        if registry.agents.is_empty() {
            return Err("no agents found; create one with `harimu agent create`".to_string());
        }
        for (addr, profile) in registry.agents.iter() {
            let id = vm.spawn_agent_with_age(
                addr.clone(),
                profile.qi as harimu::Qi,
                position,
                profile.max_age,
            );
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

    state::set_status(
        Status::Running,
        vm.world().tick(),
        Some("agent loop running".into()),
    )
    .map_err(|e| e.to_string())?;

    match brain {
        BrainMode::Loop => run_loop(&agent_ids, &action_cycle, ticks, effective_delay, &mut vm)?,
        BrainMode::Llm => {
            let api_key = llm_api_key
                .or_else(|| env::var("LLM_API_KEY").ok())
                .or_else(load_llm_key_from_file);
            let client = LlmClient::new(
                llm_host,
                llm_model,
                llm_provider,
                api_key,
                Duration::from_millis(llm_timeout_ms),
            )
            .map_err(|e| format!("llm client: {}", e))?;

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
    persist_structures(&tick.events)?;
    persist_world_view(vm);
    persist_action_stats(&requests, &tick);

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

fn load_llm_key_from_file() -> Option<String> {
    let path = PathBuf::from(".harimu/.key");
    let data = fs::read_to_string(&path).ok()?;
    let trimmed = data.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn run_llm_loop(
    agent_ids: &[AgentId],
    action_cycle: &[ActionArg],
    ticks: Option<u64>,
    delay: Duration,
    vm: &mut Vm,
    client: LlmClient,
) -> Result<(), String> {
    let llm_client = Some(client);
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
                llm_client.as_ref(),
                next_tick,
            );

            println!(
                "Tick {} | LLM planner | Agent {}",
                vm.world().tick() + 1,
                agent_id
            );
            println!(" 1) State     : {}", decision.summary);
            println!(" 2) Goal      : {}", harimu::DEFAULT_AGENT_GOAL);
            println!(" 3) Prompt    : {}", decision.prompt);
            println!(" 4) LLM reply : {}", decision.response);
            println!(" 5) Decision  : {:?}", decision.action);
            println!(" 6) Tx        : signed+submitted (simulated)");
            println!(" 7) Memory    : {} notes", memory.notes.len());
            println!(" 8) LLM model : {:?} {}", decision.provider, decision.model);

            if !decision.llm_ok {
                println!(
                    "LLM unreachable; falling back to loop action this tick. Reason: {}",
                    decision.response
                );
            } else {
            }

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
            record_outcome(&mut memories, &tick, *agent_id);
        }
        persist_structures(&tick.events)?;
        persist_world_view(vm);
        persist_action_stats(&requests, &tick);

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
    let mut actions = vec![
        ActionArg::Move {
            dx: 0,
            dy: 0,
            dz: 0,
        },
        ActionArg::Scan,
        ActionArg::BuildStructure {
            kind: StructureKind::Basic,
        },
        ActionArg::BuildStructure {
            kind: StructureKind::Programmable,
        },
        ActionArg::HarvestOre {
            ore: OreKind::Qi,
            source_id: 0,
        },
        ActionArg::HarvestOre {
            ore: OreKind::Transistor,
            source_id: 0,
        },
    ];

    if agent_ids.len() > 1 {
        let partner = agent_ids[0];
        actions.push(ActionArg::Reproduce { partner });
    }

    actions
}

fn default_loop_actions(agent_ids: &[AgentId]) -> Vec<ActionArg> {
    let mut actions = vec![
        ActionArg::Move {
            dx: 1,
            dy: 0,
            dz: 0,
        },
        ActionArg::Move {
            dx: 0,
            dy: 1,
            dz: 0,
        },
        ActionArg::Move {
            dx: 0,
            dy: 0,
            dz: 1,
        },
        ActionArg::Scan,
        ActionArg::BuildStructure {
            kind: StructureKind::Basic,
        },
        ActionArg::HarvestOre {
            ore: OreKind::Transistor,
            source_id: 0,
        },
        ActionArg::BuildStructure {
            kind: StructureKind::Programmable,
        },
    ];

    if agent_ids.len() > 1 {
        let partner = agent_ids[0];
        actions.push(ActionArg::Reproduce { partner });
    }

    actions
}

fn persist_structures(events: &[Event]) -> Result<(), String> {
    use std::collections::HashSet;

    let mut store = load_structure_store().map_err(|e| e.to_string())?;
    let mut seen: HashSet<u64> = store.structures.iter().map(|s| s.id).collect();
    let mut updated = false;

    for event in events {
        if let Event::StructureBuilt {
            agent_id,
            kind,
            position,
            structure_id,
        } = event
        {
            if seen.insert(*structure_id) {
                store.structures.push(StructureRecord {
                    id: *structure_id,
                    kind: *kind,
                    position: *position,
                    zone: position.zone(),
                    owner: *agent_id,
                });
                updated = true;
            }
        }
    }

    if updated {
        save_structure_store(&store).map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn record_outcome(
    memories: &mut HashMap<AgentId, BrainMemory>,
    tick: &TickResult,
    agent_id: AgentId,
) {
    const MEMORY_LIMIT: usize = 8;
    let memory = memories.entry(agent_id).or_default();

    if let Some(rej) = tick
        .rejections
        .iter()
        .find(|r| r.request.agent_id == agent_id)
    {
        memory.notes.push(format!(
            "tick {}: action {:?} rejected ({})",
            tick.tick, rej.request.action, rej.error
        ));
    } else {
        memory.notes.push(format!(
            "tick {}: action applied (events={})",
            tick.tick,
            tick.events.len()
        ));
    }

    if memory.notes.len() > MEMORY_LIMIT {
        let drop = memory.notes.len() - MEMORY_LIMIT;
        memory.notes.drain(0..drop);
    }
}

fn persist_world_view(vm: &Vm) {
    let snapshot = vm.snapshot();
    if let Err(err) = save_world_snapshot(&snapshot) {
        eprintln!("warning: failed to write world snapshot: {}", err);
    }
    if let Err(err) = save_world_snapshot_tick(&snapshot) {
        eprintln!("warning: failed to write tick snapshot: {}", err);
    }
}

fn persist_action_stats(requests: &[ActionRequest], tick: &TickResult) {
    let mut store = match harimu::load_action_stats() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("warning: failed to load action stats: {}", err);
            return;
        }
    };

    for req in requests {
        let rejected = tick
            .rejections
            .iter()
            .any(|r| r.request.agent_id == req.agent_id && r.request.action == req.action);
        if rejected {
            continue;
        }
        record_successful_actions(&mut store, req.agent_id, std::iter::once(req.action));
    }

    if let Err(err) = save_action_stats(&store) {
        eprintln!("warning: failed to save action stats: {}", err);
    }
}

fn print_action_summary() -> Result<(), String> {
    let store = harimu::load_action_stats().map_err(|e| e.to_string())?;
    if store.per_agent.is_empty() {
        println!("No action stats recorded.");
        return Ok(());
    }

    println!("Action summary per agent:");
    for (agent, stats) in store.per_agent.iter() {
        println!(
            " - agent {} | move={} scan={} build={} harvest={} reproduce={} idle={}",
            agent,
            stats.move_count,
            stats.scan_count,
            stats.build_count,
            stats.harvest_count,
            stats.reproduce_count,
            stats.idle_count
        );
    }
    Ok(())
}

fn print_tick(tick: &TickResult, vm: &Vm, agent_id: AgentId) {
    println!(
        "Tick {}: {} events, {} rejections",
        tick.tick,
        tick.events.len(),
        tick.rejections.len()
    );

    for event in &tick.events {
        println!(" - {}", describe_event(vm, event));
    }

    if !tick.rejections.is_empty() {
        println!("Rejections:");
        for rejection in &tick.rejections {
            println!(
                " - agent {} action {:?}: {:?}",
                agent_label(vm, rejection.request.agent_id),
                rejection.request.action,
                rejection.error
            );
        }
    }

    if let Some(agent) = vm.world().agent(agent_id) {
        println!(
            "Agent #{} | qi={} | transistors={} | position=({}, {}, {}) | alive={} | age={}",
            agent.id,
            agent.qi,
            agent.transistors,
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

fn agent_label(vm: &Vm, agent_id: AgentId) -> String {
    vm.world()
        .agent(agent_id)
        .map(|a| format!("#{}", a.id))
        .unwrap_or_else(|| format!("#{}", agent_id))
}

fn describe_event(vm: &Vm, event: &Event) -> String {
    match event {
        Event::TickStarted { tick } => format!("tick {} started", tick),
        Event::TickCompleted { tick } => format!("tick {} completed", tick),
        Event::AgentSpawned {
            agent_id,
            name,
            qi,
            position,
        } => format!(
            "agent #{} spawned (name={}) with qi={} at ({}, {}, {})",
            agent_id, name, qi, position.x, position.y, position.z
        ),
        Event::QiSpent {
            agent_id,
            amount,
            action,
        } => format!(
            "agent {} spent {} qi on {}",
            agent_label(vm, *agent_id),
            amount,
            action
        ),
        Event::OreGained {
            agent_id,
            ore,
            amount,
            source,
        } => format!(
            "agent {} gained {} {} from {}",
            agent_label(vm, *agent_id),
            amount,
            ore,
            source
        ),
        Event::AgentMoved { agent_id, from, to } => format!(
            "agent {} moved from ({}, {}, {}) to ({}, {}, {})",
            agent_label(vm, *agent_id),
            from.x,
            from.y,
            from.z,
            to.x,
            to.y,
            to.z
        ),
        Event::AgentDied { agent_id, reason } => {
            format!("agent {} died: {:?}", agent_label(vm, *agent_id), reason)
        }
        Event::AgentReproduced {
            parent_a,
            parent_b,
            child_id,
        } => format!(
            "agents {} and {} reproduced; child={}",
            agent_label(vm, *parent_a),
            agent_label(vm, *parent_b),
            agent_label(vm, *child_id)
        ),
        Event::StructureBuilt {
            agent_id,
            kind,
            position,
            structure_id,
        } => format!(
            "agent {} built {} structure {} at ({}, {}, {})",
            agent_label(vm, *agent_id),
            kind,
            structure_id,
            position.x,
            position.y,
            position.z
        ),
        Event::OreNodeHarvested {
            agent_id,
            ore,
            source_id,
            amount,
            remaining,
        } => format!(
            "agent {} harvested {} {} from node {} (remaining={})",
            agent_label(vm, *agent_id),
            amount,
            ore,
            source_id,
            remaining
        ),
        Event::OreNodeDrained {
            ore,
            source_id,
            position,
        } => format!(
            "{} node {} drained at ({}, {}, {})",
            ore, source_id, position.x, position.y, position.z
        ),
        Event::ActionObserved { agent_id, action } => {
            format!(
                "agent {} observed action {}",
                agent_label(vm, *agent_id),
                action
            )
        }
        Event::ScanReport {
            agent_id,
            position,
            qi,
            nearby_qi_sources,
            nearby_structures,
        } => format!(
            "agent {} scan at ({}, {}, {}) qi={} | ore_sources={} | structures={}",
            agent_label(vm, *agent_id),
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
            Event::AgentReproduced {
                parent_a, parent_b, ..
            } if *parent_a == agent_id || *parent_b == agent_id => offspring += 1,
            _ => {}
        }
    }
    (structures, offspring)
}

fn launch_background_start(
    agent: Option<String>,
    qi: harimu::Qi,
    position: Position,
    ticks: Option<u64>,
    brain: BrainMode,
    llm_host: String,
    llm_model: String,
    llm_timeout_ms: u64,
    llm_provider: LlmProvider,
    llm_api_key: Option<String>,
    tick_rate: Option<f64>,
    delay_ms: u64,
    actions: Vec<ActionArg>,
) -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
    let mut args = render_start_args(
        agent.clone(),
        qi,
        position,
        ticks,
        brain,
        llm_host,
        llm_model,
        llm_timeout_ms,
        llm_provider,
        llm_api_key.clone(),
        tick_rate,
        delay_ms,
        &actions,
    );
    args.push("--background-child".into());

    let child = std::process::Command::new(exe)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn background process: {}", e))?;

    let pid_path = PathBuf::from(PID_FILE);
    if let Some(parent) = pid_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&pid_path, format!("{}", child.id()))
        .map_err(|e| format!("failed to write pid file {}: {}", pid_path.display(), e))?;

    println!(
        "Started background agent loop (pid={}). Stop with `harimu stop`.",
        child.id()
    );
    Ok(())
}

fn render_start_args(
    agent: Option<String>,
    qi: harimu::Qi,
    position: Position,
    ticks: Option<u64>,
    brain: BrainMode,
    llm_host: String,
    llm_model: String,
    llm_timeout_ms: u64,
    llm_provider: LlmProvider,
    llm_api_key: Option<String>,
    tick_rate: Option<f64>,
    delay_ms: u64,
    actions: &[ActionArg],
) -> Vec<String> {
    let mut args = Vec::new();
    args.push("start".into());
    if let Some(agent) = agent {
        args.push("--agent".into());
        args.push(agent);
    }
    args.push("--qi".into());
    args.push(format!("{}", qi));
    args.push("--position".into());
    args.push(format!("{},{},{}", position.x, position.y, position.z));
    if let Some(t) = ticks {
        args.push("--ticks".into());
        args.push(format!("{}", t));
    }
    args.push("--llm-host".into());
    args.push(llm_host);
    args.push("--llm-model".into());
    args.push(llm_model);
    args.push("--llm-timeout-ms".into());
    args.push(format!("{}", llm_timeout_ms));
    if let Some(key) = llm_api_key {
        args.push("--llm-api-key".into());
        args.push(key);
    }
    if let Some(rate) = tick_rate {
        args.push("--tick-rate".into());
        args.push(rate.to_string());
    } else {
        args.push("--delay-ms".into());
        args.push(delay_ms.to_string());
    }

    args.push("--brain".into());
    args.push(brain_to_arg(brain).into());
    args.push("--llm-provider".into());
    args.push(llm_provider_to_arg(llm_provider).into());

    for action in actions {
        args.push("--action".into());
        args.push(render_action_arg(action));
    }

    args
}

fn brain_to_arg(brain: BrainMode) -> &'static str {
    match brain {
        BrainMode::Loop => "loop",
        BrainMode::Llm => "llm",
    }
}

fn llm_provider_to_arg(provider: LlmProvider) -> &'static str {
    match provider {
        LlmProvider::Ollama => "ollama",
        LlmProvider::Openai => "openai",
    }
}

fn render_action_arg(arg: &ActionArg) -> String {
    match arg {
        ActionArg::Scan => "scan".into(),
        ActionArg::Idle => "idle".into(),
        ActionArg::Move { dx, dy, dz } => format!("move:{},{},{}", dx, dy, dz),
        ActionArg::Reproduce { partner } => format!("reproduce:{}", partner),
        ActionArg::BuildStructure { kind } => format!("build:{}", kind),
        ActionArg::HarvestOre { ore, source_id } => {
            if *source_id > 0 {
                format!("harvest:{},{}", ore, source_id)
            } else {
                format!("harvest:{}", ore)
            }
        }
    }
}

fn try_kill_background_process() {
    let pid_path = PathBuf::from(PID_FILE);
    let pid_str = match fs::read_to_string(&pid_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let pid: u32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(_) => return,
    };

    let status = std::process::Command::new("kill")
        .arg(format!("{}", pid))
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("Stopped background process pid={}", pid);
            let _ = fs::remove_file(pid_path);
        }
        Ok(_) => eprintln!("warning: failed to stop background pid {}", pid),
        Err(_) => {}
    }
}
