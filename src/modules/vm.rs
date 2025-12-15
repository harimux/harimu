use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::modules::ore::OreKind;
use crate::modules::structure::{Structure, StructureKind};
use crate::modules::view::{AgentSnapshot, OreNodeSnapshot, StructureView, WorldSnapshot};

pub type AgentId = u64;
pub type Qi = u32;

/// Number of leading zero bytes required for PoW validity.
pub const POW_DIFFICULTY_BYTES: usize = 2;
/// Qi reward for a valid PoW.
pub const POW_REWARD: Qi = 5;
/// Size of a zone along each axis.
pub const ZONE_SIZE: i32 = 16;
/// How far a scan can see in Chebyshev distance (voxels).
pub const SCAN_RANGE: i32 = 8;
/// How close an agent must be to harvest Qi.
pub const HARVEST_RANGE: i32 = 1;
/// Max ore units that can be harvested from a node per action.
pub const HARVEST_PER_ACTION: Qi = 3;
/// Default agent lifespan in ticks unless extended by the creator.
pub const DEFAULT_MAX_AGENT_AGE: u64 = 112;
/// Maximum movement radius per action (Chebyshev distance).
pub const MAX_MOVE_RADIUS: i32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QiSource {
    pub id: u64,
    pub ore: OreKind,
    pub position: Position,
    pub capacity: Qi,
    pub current: Qi,
    pub recharge_per_tick: Qi,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QiSourceSnapshot {
    pub id: u64,
    pub ore: OreKind,
    pub position: Position,
    pub available: Qi,
    pub capacity: Qi,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureSnapshot {
    pub id: u64,
    pub kind: StructureKind,
    pub position: Position,
}

fn pow_hash(agent_id: AgentId, tick: u64, nonce: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(agent_id.to_le_bytes());
    hasher.update(tick.to_le_bytes());
    hasher.update(nonce.to_le_bytes());
    let bytes = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

pub fn pow_valid(agent_id: AgentId, tick: u64, nonce: u64) -> bool {
    let hash = pow_hash(agent_id, tick, nonce);
    hash.iter().take(POW_DIFFICULTY_BYTES).all(|b| *b == 0)
}

fn nearest_ore_source(sources: &[QiSource], ore: OreKind, position: Position) -> Option<QiSource> {
    let mut best: Option<(i32, QiSource)> = None;
    for src in sources {
        if src.ore != ore {
            continue;
        }
        if src.current == 0 {
            continue;
        }
        if !position.within_range(src.position, SCAN_RANGE) {
            continue;
        }
        let dist = (position.x - src.position.x).abs()
            + (position.y - src.position.y).abs()
            + (position.z - src.position.z).abs();
        match &mut best {
            Some((best_dist, best_src)) => {
                if dist < *best_dist {
                    *best_dist = dist;
                    *best_src = src.clone();
                }
            }
            None => best = Some((dist, src.clone())),
        }
    }
    best.map(|(_, src)| src)
}

/// Find a valid nonce starting from `start_nonce` (inclusive).
pub fn pow_solve(agent_id: AgentId, tick: u64, start_nonce: u64) -> u64 {
    let mut nonce = start_nonce;
    loop {
        if pow_valid(agent_id, tick, nonce) {
            return nonce;
        }
        nonce = nonce.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Position {
    pub const fn origin() -> Self {
        Self { x: 0, y: 0, z: 0 }
    }

    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            z: self.z + dz,
        }
    }

    pub fn zone(self) -> Zone {
        Zone {
            x: self.x.div_euclid(ZONE_SIZE),
            y: self.y.div_euclid(ZONE_SIZE),
            z: self.z.div_euclid(ZONE_SIZE),
        }
    }

    pub fn within_range(self, other: Position, range: i32) -> bool {
        let dx = (self.x - other.x).abs();
        let dy = (self.y - other.y).abs();
        let dz = (self.z - other.z).abs();
        dx <= range && dy <= range && dz <= range
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Zone {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Scan,
    Move { dx: i32, dy: i32, dz: i32 },
    Reproduce { partner: AgentId },
    BuildStructure { kind: StructureKind },
    HarvestOre { ore: OreKind, source_id: u64 },
    Idle,
}

impl Action {
    pub const fn qi_cost(&self) -> Qi {
        match self {
            Action::Scan | Action::Idle => 0,
            Action::Move { .. } => 0,
            Action::Reproduce { .. } => 0,
            Action::BuildStructure { .. } => 1,
            Action::HarvestOre { .. } => 1,
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Action::Scan => "scan",
            Action::Move { .. } => "move",
            Action::Reproduce { .. } => "reproduce",
            Action::BuildStructure { .. } => "build_structure",
            Action::HarvestOre { .. } => "harvest",
            Action::Idle => "idle",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    TickStarted {
        tick: u64,
    },
    TickCompleted {
        tick: u64,
    },
    AgentSpawned {
        agent_id: AgentId,
        name: String,
        qi: Qi,
        position: Position,
    },
    QiSpent {
        agent_id: AgentId,
        amount: Qi,
        action: &'static str,
    },
    OreGained {
        agent_id: AgentId,
        ore: OreKind,
        amount: Qi,
        source: &'static str,
    },
    AgentMoved {
        agent_id: AgentId,
        from: Position,
        to: Position,
    },
    AgentDied {
        agent_id: AgentId,
        reason: DeathReason,
    },
    ActionObserved {
        agent_id: AgentId,
        action: &'static str,
    },
    AgentReproduced {
        parent_a: AgentId,
        parent_b: AgentId,
        child_id: AgentId,
    },
    StructureBuilt {
        agent_id: AgentId,
        kind: StructureKind,
        position: Position,
        structure_id: u64,
    },
    OreNodeHarvested {
        agent_id: AgentId,
        ore: OreKind,
        source_id: u64,
        amount: Qi,
        remaining: Qi,
    },
    OreNodeDrained {
        ore: OreKind,
        source_id: u64,
        position: Position,
    },
    ScanReport {
        agent_id: AgentId,
        position: Position,
        qi: Qi,
        nearby_qi_sources: Vec<QiSourceSnapshot>,
        nearby_structures: Vec<StructureSnapshot>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeathReason {
    Age,
    Hazard,
    Corruption,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    AgentNotFound(AgentId),
    AgentDead(AgentId),
    InsufficientQi {
        agent_id: AgentId,
        required: Qi,
        available: Qi,
    },
    InsufficientOre {
        agent_id: AgentId,
        ore: OreKind,
        required: Qi,
        available: Qi,
    },
    InvalidPow {
        agent_id: AgentId,
        nonce: u64,
    },
    PositionOccupied {
        agent_id: AgentId,
        target: Position,
        occupied_by: AgentId,
    },
    ReproductionDeclined {
        agent_id: AgentId,
        partner: AgentId,
    },
    PartnerNotFound {
        agent_id: AgentId,
        partner: AgentId,
    },
    PartnerOutOfZone {
        agent_id: AgentId,
        partner: AgentId,
    },
    StructureSpaceOccupied {
        agent_id: AgentId,
        position: Position,
    },
    OreSourceUnavailable {
        agent_id: AgentId,
        ore: OreKind,
        source_id: Option<u64>,
    },
    OreSourceDepleted {
        agent_id: AgentId,
        ore: OreKind,
        source_id: u64,
        available: Qi,
    },
    MoveOutOfRange {
        agent_id: AgentId,
        dx: i32,
        dy: i32,
        dz: i32,
    },
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::AgentNotFound(id) => write!(f, "agent {} not found", id),
            ActionError::AgentDead(id) => write!(f, "agent {} is dead", id),
            ActionError::InsufficientQi {
                agent_id,
                required,
                available,
            } => write!(
                f,
                "agent {} has insufficient qi: required {}, available {}",
                agent_id, required, available
            ),
            ActionError::InsufficientOre {
                agent_id,
                ore,
                required,
                available,
            } => write!(
                f,
                "agent {} has insufficient {}: required {}, available {}",
                agent_id,
                ore,
                required,
                available
            ),
            ActionError::InvalidPow { agent_id, nonce } => {
                write!(f, "invalid PoW for agent {} with nonce {}", agent_id, nonce)
            }
            ActionError::PositionOccupied {
                agent_id,
                target,
                occupied_by,
            } => write!(
                f,
                "agent {} cannot move to occupied position ({}, {}, {}) held by {}",
                agent_id, target.x, target.y, target.z, occupied_by
            ),
            ActionError::ReproductionDeclined { agent_id, partner } => write!(
                f,
                "agent {} reproduction declined/not mutually agreed with {}",
                agent_id, partner
            ),
            ActionError::PartnerNotFound { agent_id, partner } => write!(
                f,
                "agent {} reproduction partner {} not found",
                agent_id, partner
            ),
            ActionError::PartnerOutOfZone { agent_id, partner } => write!(
                f,
                "agent {} reproduction partner {} not in same zone",
                agent_id, partner
            ),
            ActionError::StructureSpaceOccupied { agent_id, position } => write!(
                f,
                "agent {} cannot build structure at ({}, {}, {}) (occupied)",
                agent_id, position.x, position.y, position.z
            ),
            ActionError::OreSourceUnavailable {
                agent_id,
                ore,
                source_id,
            } => {
                if let Some(id) = source_id {
                    write!(
                        f,
                        "agent {} cannot harvest {} source {} (unavailable or out of range)",
                        agent_id, ore, id
                    )
                } else {
                    write!(
                        f,
                        "agent {} has no {} source in range to harvest",
                        agent_id, ore
                    )
                }
            }
            ActionError::OreSourceDepleted {
                agent_id,
                ore,
                source_id,
                available,
            } => write!(
                f,
                "agent {} cannot harvest depleted {} source {} (available {}; need >= {})",
                agent_id,
                ore,
                source_id,
                available,
                HARVEST_PER_ACTION
            ),
            ActionError::MoveOutOfRange { agent_id, dx, dy, dz } => write!(
                f,
                "agent {} move exceeds max radius {} (requested {},{},{} )",
                agent_id, MAX_MOVE_RADIUS, dx, dy, dz
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRequest {
    pub agent_id: AgentId,
    pub action: Action,
}

impl ActionRequest {
    pub fn new(agent_id: AgentId, action: Action) -> Self {
        Self { agent_id, action }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRejection {
    pub request: ActionRequest,
    pub error: ActionError,
}

#[derive(Debug)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    pub qi: Qi,
    pub transistors: Qi,
    pub position: Position,
    pub alive: bool,
    pub age: u64,
    pub max_age: u64,
    pub discovered_zones: HashSet<Zone>,
}

impl Agent {
    fn spend_qi(&mut self, amount: Qi) -> Result<(), ActionError> {
        if self.qi < amount {
            return Err(ActionError::InsufficientQi {
                agent_id: self.id,
                required: amount,
                available: self.qi,
            });
        }

        self.qi -= amount;
        Ok(())
    }

    fn gain_ore(&mut self, ore: OreKind, amount: Qi) {
        match ore {
            OreKind::Qi => {
                self.qi = self.qi.saturating_add(amount);
            }
            OreKind::Transistor => {
                self.transistors = self.transistors.saturating_add(amount);
            }
        }
    }

    fn spend_ore(&mut self, ore: OreKind, amount: Qi) -> Result<(), ActionError> {
        match ore {
            OreKind::Qi => self.spend_qi(amount),
            OreKind::Transistor => {
                if self.transistors < amount {
                    return Err(ActionError::InsufficientOre {
                        agent_id: self.id,
                        ore,
                        required: amount,
                        available: self.transistors,
                    });
                }
                self.transistors -= amount;
                Ok(())
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct World {
    pub(crate) tick: u64,
    next_agent_id: AgentId,
    next_structure_id: u64,
    next_qi_source_id: u64,
    max_qi_supply: Option<u64>,
    recycled_qi: u64,
    agents: HashMap<AgentId, Agent>,
    events: Vec<Event>,
    occupied: HashMap<Position, AgentId>,
    structures: Vec<Structure>,
    qi_sources: Vec<QiSource>,
}

impl World {
    pub fn new() -> Self {
        Self {
            tick: 0,
            next_agent_id: 1,
            next_structure_id: 1,
            next_qi_source_id: 1,
            max_qi_supply: None,
            recycled_qi: 0,
            agents: HashMap::new(),
            events: Vec::new(),
            occupied: HashMap::new(),
            structures: Vec::new(),
            qi_sources: Vec::new(),
        }
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn spawn_agent(
        &mut self,
        name: impl Into<String>,
        qi: Qi,
        position: Position,
    ) -> AgentId {
        self.spawn_agent_with_age(name, qi, position, DEFAULT_MAX_AGENT_AGE)
    }

    pub fn spawn_agent_with_age(
        &mut self,
        name: impl Into<String>,
        qi: Qi,
        position: Position,
        max_age: u64,
    ) -> AgentId {
        let mut pos = position;
        // Ensure no two agents share the same coordinates; walk +x until a free spot is found.
        while self.occupied.contains_key(&pos) {
            pos = pos.offset(1, 0, 0);
        }

        let agent_id = self.next_agent_id;
        self.next_agent_id += 1;

        let agent = Agent {
            id: agent_id,
            name: name.into(),
            qi,
            transistors: 0,
            position: pos,
            alive: true,
            age: 0,
            max_age: max_age.max(1),
            discovered_zones: {
                let mut set = HashSet::new();
                set.insert(pos.zone());
                set
            },
        };

        self.events.push(Event::AgentSpawned {
            agent_id,
            name: agent.name.clone(),
            qi: agent.qi,
            position: agent.position,
        });

        self.agents.insert(agent_id, agent);
        self.occupied.insert(pos, agent_id);
        agent_id
    }

    pub fn agent(&self, id: AgentId) -> Option<&Agent> {
        self.agents.get(&id)
    }

    pub fn agents(&self) -> impl Iterator<Item = (&AgentId, &Agent)> {
        self.agents.iter()
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn qi_sources(&self) -> &[QiSource] {
        &self.qi_sources
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        let mut agents: Vec<AgentSnapshot> = self
            .agents
            .values()
            .map(|a| AgentSnapshot {
                id: a.id,
                name: a.name.clone(),
                qi: a.qi,
                transistors: a.transistors,
                position: a.position,
                alive: a.alive,
                age: a.age,
                max_age: a.max_age,
            })
            .collect();

        let mut ore_nodes: Vec<OreNodeSnapshot> = self
            .qi_sources
            .iter()
            .map(|src| OreNodeSnapshot {
                id: src.id,
                ore: src.ore,
                position: src.position,
                available: src.current,
                capacity: src.capacity,
                recharge_per_tick: src.recharge_per_tick,
            })
            .collect();

        let mut structures: Vec<StructureView> = self
            .structures
            .iter()
            .map(|s| StructureView {
                id: s.id,
                kind: s.kind,
                position: s.position,
                owner: s.owner,
            })
            .collect();

        agents.sort_by_key(|a| a.id);
        ore_nodes.sort_by_key(|n| n.id);
        structures.sort_by_key(|s| s.id);

        WorldSnapshot {
            tick: self.tick,
            agents,
            ore_nodes,
            structures,
        }
    }

    pub fn set_max_qi_supply(&mut self, max: u64) {
        self.max_qi_supply = Some(max);
    }

    fn recycle_qi(&mut self, amount: Qi) {
        self.recycled_qi = self.recycled_qi.saturating_add(amount as u64);
    }

    fn total_qi_supply(&self) -> u64 {
        let agents_qi: u64 = self
            .agents
            .values()
            .map(|a| a.qi as u64)
            .fold(0u64, |acc, v| acc.saturating_add(v));
        let sources_qi: u64 = self
            .qi_sources
            .iter()
            .filter(|s| s.ore == OreKind::Qi)
            .map(|s| s.current as u64)
            .fold(0u64, |acc, v| acc.saturating_add(v));
        agents_qi
            .saturating_add(sources_qi)
            .saturating_add(self.recycled_qi)
    }

    pub fn add_qi_source(
        &mut self,
        ore: OreKind,
        position: Position,
        capacity: Qi,
        recharge_per_tick: Qi,
    ) -> u64 {
        let id = self.next_qi_source_id;
        self.next_qi_source_id += 1;
        let source = QiSource {
            id,
            ore,
            position,
            capacity,
            current: capacity,
            recharge_per_tick,
        };
        self.qi_sources.push(source);
        id
    }

    fn recharge_qi_sources(&mut self) {
        let mut qi_budget = self
            .max_qi_supply
            .map(|max| max.saturating_sub(self.total_qi_supply()))
            .unwrap_or(u64::MAX);
        let mut pool = self.recycled_qi;

        for source in &mut self.qi_sources {
            if source.ore != OreKind::Qi {
                let new_level = source.current.saturating_add(source.recharge_per_tick);
                source.current = new_level.min(source.capacity);
                continue;
            }

            let headroom = (source.capacity.saturating_sub(source.current)) as u64;
            if headroom == 0 {
                continue;
            }

            let allowance = source.recharge_per_tick as u64;
            // First refill from recycled pool (conserved Qi).
            let from_pool = pool.min(headroom).min(allowance);
            if from_pool > 0 {
                source.current = source
                    .current
                    .saturating_add(from_pool as Qi)
                    .min(source.capacity);
                pool = pool.saturating_sub(from_pool);
            }

            let remaining_allowance = allowance.saturating_sub(from_pool);
            let remaining_headroom =
                headroom.saturating_sub(from_pool).min((source.capacity - source.current) as u64);
            if remaining_allowance > 0 && remaining_headroom > 0 && qi_budget > 0 {
                let mint = remaining_allowance
                    .min(remaining_headroom)
                    .min(qi_budget);
                if mint > 0 {
                    source.current = source
                        .current
                        .saturating_add(mint as Qi)
                        .min(source.capacity);
                    qi_budget = qi_budget.saturating_sub(mint);
                }
            }
        }

        self.recycled_qi = pool;
    }

    fn nearby_qi_sources(&self, position: Position, range: i32) -> Vec<QiSourceSnapshot> {
        self.qi_sources
            .iter()
            .filter(|s| s.position.within_range(position, range))
            .map(|s| QiSourceSnapshot {
                id: s.id,
                ore: s.ore,
                position: s.position,
                available: s.current,
                capacity: s.capacity,
            })
            .collect()
    }

    fn nearby_structures(&self, position: Position, range: i32) -> Vec<StructureSnapshot> {
        self.structures
            .iter()
            .filter(|s| s.position.within_range(position, range))
            .map(|s| StructureSnapshot {
                id: s.id,
                kind: s.kind,
                position: s.position,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickResult {
    pub tick: u64,
    pub events: Vec<Event>,
    pub rejections: Vec<ActionRejection>,
}

#[derive(Debug, Default)]
pub struct Vm {
    world: World,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            world: World::new(),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        self.world.snapshot()
    }

    /// Read-only access to a single agent's state.
    pub fn agent(&self, agent_id: AgentId) -> Option<&Agent> {
        self.world.agent(agent_id)
    }

    /// Registry view of all agents in the VM.
    pub fn agent_registry(&self) -> impl Iterator<Item = (&AgentId, &Agent)> {
        self.world.agents()
    }

    /// Set the current tick counter (used when resuming from persisted state).
    pub fn set_tick(&mut self, tick: u64) {
        self.world.tick = tick;
    }

    pub fn set_max_qi_supply(&mut self, max: u64) {
        self.world.set_max_qi_supply(max);
    }

    pub fn spawn_agent(&mut self, name: impl Into<String>, qi: Qi, position: Position) -> AgentId {
        self.world.spawn_agent(name, qi, position)
    }

    pub fn spawn_agent_with_age(
        &mut self,
        name: impl Into<String>,
        qi: Qi,
        position: Position,
        max_age: u64,
    ) -> AgentId {
        self.world.spawn_agent_with_age(name, qi, position, max_age)
    }

    pub fn kill_agent(
        &mut self,
        agent_id: AgentId,
        reason: DeathReason,
    ) -> Result<(), ActionError> {
        let agent = self
            .world
            .agents
            .get_mut(&agent_id)
            .ok_or(ActionError::AgentNotFound(agent_id))?;

        if !agent.alive {
            return Ok(());
        }

        agent.alive = false;
        self.world.occupied.remove(&agent.position);
        self.world
            .events
            .push(Event::AgentDied { agent_id, reason });
        Ok(())
    }

    pub fn seed_qi_source(
        &mut self,
        position: Position,
        capacity: Qi,
        recharge_per_tick: Qi,
    ) -> u64 {
        self.seed_ore_source(OreKind::Qi, position, capacity, recharge_per_tick)
    }

    pub fn seed_ore_source(
        &mut self,
        ore: OreKind,
        position: Position,
        capacity: Qi,
        recharge_per_tick: Qi,
    ) -> u64 {
        self.world
            .add_qi_source(ore, position, capacity, recharge_per_tick)
    }

    pub fn step(&mut self, actions: &[ActionRequest]) -> TickResult {
        let tick = self.world.tick + 1;
        let mut tick_events = vec![Event::TickStarted { tick }];
        let mut rejections = Vec::new();

        // World progression before actions (e.g., recharge Qi sources).
        self.world.recharge_qi_sources();

        // Precompute mutual reproduction consents for this tick.
        let mut intents: HashMap<AgentId, AgentId> = HashMap::new();
        for req in actions {
            if let Action::Reproduce { partner } = req.action {
                intents.insert(req.agent_id, partner);
            }
        }
        let mut mutual_pairs: HashSet<(AgentId, AgentId)> = HashSet::new();
        for (a, b) in intents.iter() {
            if let Some(back) = intents.get(b) {
                if *back == *a {
                    let pair = if a < b { (*a, *b) } else { (*b, *a) };
                    mutual_pairs.insert(pair);
                }
            }
        }
        let snapshot: HashMap<AgentId, (Position, bool)> = self
            .world
            .agents
            .iter()
            .map(|(id, agent)| (*id, (agent.position, agent.alive)))
            .collect();

        for request in actions.iter().cloned() {
            match self.apply_action(request.clone(), tick, &mutual_pairs, &snapshot) {
                Ok(mut events) => tick_events.append(&mut events),
                Err(error) => rejections.push(ActionRejection { request, error }),
            }
        }

        tick_events.append(&mut self.enforce_age_limits());
        tick_events.push(Event::TickCompleted { tick });

        self.world.tick = tick;
        self.world.events.extend(tick_events.clone());

        TickResult {
            tick,
            events: tick_events,
            rejections,
        }
    }

    fn apply_action(
        &mut self,
        request: ActionRequest,
        _tick: u64,
        mutual_pairs: &HashSet<(AgentId, AgentId)>,
        snapshot: &HashMap<AgentId, (Position, bool)>,
    ) -> Result<Vec<Event>, ActionError> {
        let mut events = Vec::new();
        let mut pending_child: Option<(String, Position, AgentId, AgentId)> = None;
        let mut pending_scan: Option<(AgentId, Position, Qi)> = None;
        let mut pending_harvest: Option<(AgentId, OreKind, u64)> = None;
        let mut reclaimed_qi: Qi = 0;

        {
            let agent = self
                .world
                .agents
                .get_mut(&request.agent_id)
                .ok_or(ActionError::AgentNotFound(request.agent_id))?;

            if !agent.alive {
                return Err(ActionError::AgentDead(request.agent_id));
            }

            match request.action {
                Action::Move { dx, dy, dz } => {
                    let max_delta = dx.abs().max(dy.abs()).max(dz.abs());
                    if max_delta > MAX_MOVE_RADIUS {
                        return Err(ActionError::MoveOutOfRange {
                            agent_id: agent.id,
                            dx,
                            dy,
                            dz,
                        });
                    }
                    let from = agent.position;
                    let to = agent.position.offset(dx, dy, dz);
                    if let Some(other) = self.world.occupied.get(&to) {
                        if *other != agent.id {
                            return Err(ActionError::PositionOccupied {
                                agent_id: agent.id,
                                target: to,
                                occupied_by: *other,
                            });
                        }
                    }

                    let from_zone = from.zone();
                    let to_zone = to.zone();
                    if from_zone != to_zone && !agent.discovered_zones.contains(&to_zone) {
                        agent.discovered_zones.insert(to_zone);
                    }

                    agent.spend_qi(1)?;
                    events.push(Event::QiSpent {
                        agent_id: agent.id,
                        amount: 1,
                        action: request.action.label(),
                    });
                    reclaimed_qi = reclaimed_qi.saturating_add(1);

                    self.world.occupied.remove(&from);
                    agent.position = to;
                    self.world.occupied.insert(to, agent.id);
                    events.push(Event::AgentMoved {
                        agent_id: agent.id,
                        from,
                        to,
                    });
                }
                Action::Scan => {
                    events.push(Event::ActionObserved {
                        agent_id: agent.id,
                        action: "scan",
                    });
                    pending_scan = Some((agent.id, agent.position, agent.qi));
                }
                Action::Reproduce { partner } => {
                    let agent_id = agent.id;
                    let agent_zone = agent.position.zone();
                    let child_position = agent.position;

                    let (partner_pos, partner_alive) = snapshot
                        .get(&partner)
                        .copied()
                        .ok_or(ActionError::PartnerNotFound { agent_id, partner })?;
                    if !partner_alive {
                        return Err(ActionError::PartnerNotFound { agent_id, partner });
                    }
                    if agent_zone != partner_pos.zone() {
                        return Err(ActionError::PartnerOutOfZone { agent_id, partner });
                    }

                    let pair = if agent_id < partner {
                        (agent_id, partner)
                    } else {
                        (partner, agent_id)
                    };
                    if !mutual_pairs.contains(&pair) {
                        return Err(ActionError::ReproductionDeclined { agent_id, partner });
                    }

                    agent.spend_qi(1)?;
                    events.push(Event::QiSpent {
                        agent_id: agent.id,
                        amount: 1,
                        action: request.action.label(),
                    });
                    reclaimed_qi = reclaimed_qi.saturating_add(1);

                    let child_name = format!("Child-{}-{}", agent_id, partner);
                    pending_child = Some((child_name, child_position, agent_id, partner));
                }
                Action::BuildStructure { kind } => {
                    if self
                        .world
                        .structures
                        .iter()
                        .any(|s| s.position == agent.position)
                    {
                        return Err(ActionError::StructureSpaceOccupied {
                            agent_id: agent.id,
                            position: agent.position,
                        });
                    }

                    // Programmable structures require transistor ore in addition to Qi energy.
                    if kind == StructureKind::Programmable {
                        agent.spend_ore(OreKind::Transistor, 1)?;
                    }

                    let cost = Action::BuildStructure { kind }.qi_cost();
                    if cost > 0 {
                        agent.spend_qi(cost)?;
                        events.push(Event::QiSpent {
                            agent_id: agent.id,
                            amount: cost,
                            action: request.action.label(),
                        });
                        reclaimed_qi = reclaimed_qi.saturating_add(cost);
                    }

                    let structure_id = self.world.next_structure_id;
                    self.world.next_structure_id += 1;
                    let structure = Structure {
                        id: structure_id,
                        kind,
                        position: agent.position,
                        zone: agent.position.zone(),
                        owner: agent.id,
                    };
                    self.world.structures.push(structure);
                    events.push(Event::StructureBuilt {
                        agent_id: agent.id,
                        kind,
                        position: agent.position,
                        structure_id,
                    });
                }
                Action::HarvestOre { ore, source_id } => {
                    let selected = if source_id == 0 {
                        nearest_ore_source(&self.world.qi_sources, ore, agent.position)
                    } else {
                        self.world
                            .qi_sources
                            .iter()
                            .find(|s| s.id == source_id && s.ore == ore)
                            .cloned()
                    };

                    let Some(src) = selected else {
                        return Err(ActionError::OreSourceUnavailable {
                            agent_id: agent.id,
                            ore,
                            source_id: (source_id != 0).then_some(source_id),
                        });
                    };

                    if !agent.position.within_range(src.position, HARVEST_RANGE) {
                        return Err(ActionError::OreSourceUnavailable {
                            agent_id: agent.id,
                            ore,
                            source_id: (source_id != 0).then_some(source_id),
                        });
                    }

                    if src.current < HARVEST_PER_ACTION {
                        return Err(ActionError::OreSourceDepleted {
                            agent_id: agent.id,
                            ore,
                            source_id: src.id,
                            available: src.current,
                        });
                    }

                    agent.spend_qi(1)?;
                    events.push(Event::QiSpent {
                        agent_id: agent.id,
                        amount: 1,
                        action: request.action.label(),
                    });
                    reclaimed_qi = reclaimed_qi.saturating_add(1);

                    pending_harvest = Some((agent.id, ore, src.id));
                }
                Action::Idle => {}
            }

            agent.age += 1;
        }

        if reclaimed_qi > 0 {
            self.world.recycle_qi(reclaimed_qi);
        }

        if let Some((agent_id, position, qi)) = pending_scan {
            let nearby_sources = self.world.nearby_qi_sources(position, SCAN_RANGE);
            let nearby_structures = self.world.nearby_structures(position, SCAN_RANGE);
            events.push(Event::ScanReport {
                agent_id,
                position,
                qi,
                nearby_qi_sources: nearby_sources,
                nearby_structures,
            });
        }

        if let Some((child_name, child_position, parent_a, parent_b)) = pending_child {
            let child_id = self.world.spawn_agent(child_name, 1, child_position);
            events.push(Event::AgentReproduced {
                parent_a,
                parent_b,
                child_id,
            });
        }

        if let Some((agent_id, ore, source_id)) = pending_harvest {
            if let Some(src) = self
                .world
                .qi_sources
                .iter_mut()
                .find(|s| s.id == source_id && s.ore == ore)
            {
                let amount = src.current.min(HARVEST_PER_ACTION);
                src.current = src.current.saturating_sub(amount);
                if let Some(agent) = self.world.agents.get_mut(&agent_id) {
                    agent.gain_ore(ore, amount);
                }

                events.push(Event::OreGained {
                    agent_id,
                    ore,
                    amount,
                    source: "ore_node",
                });
                events.push(Event::OreNodeHarvested {
                    agent_id,
                    ore,
                    source_id,
                    amount,
                    remaining: src.current,
                });

                if src.current == 0 {
                    events.push(Event::OreNodeDrained {
                        ore,
                        source_id,
                        position: src.position,
                    });
                }
            }
        }

        Ok(events)
    }

    fn enforce_age_limits(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut doomed = Vec::new();
        for agent in self.world.agents.values() {
            if agent.alive && agent.age >= agent.max_age {
                doomed.push(agent.id);
            }
        }

        for agent_id in doomed {
            if let Some(event) = self.mark_agent_dead(agent_id, DeathReason::Age) {
                events.push(event);
            }
        }

        events
    }

    fn mark_agent_dead(&mut self, agent_id: AgentId, reason: DeathReason) -> Option<Event> {
        let agent = self.world.agents.get_mut(&agent_id)?;
        if !agent.alive {
            return None;
        }

        agent.alive = false;
        self.world.occupied.remove(&agent.position);
        Some(Event::AgentDied { agent_id, reason })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qi_spend_and_death() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Nova", 2, Position::origin());

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move {
                dx: 1,
                dy: 0,
                dz: 0,
            },
        )]);

        assert!(tick.rejections.is_empty());
        assert_eq!(vm.world().tick(), 1);

        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 1); // 2 start - 1 upkeep
        assert!(agent.alive);
    }

    #[test]
    fn hitting_zero_qi_does_not_kill_agent() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Nova", 1, Position::origin());

        let _ = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move {
                dx: 1,
                dy: 0,
                dz: 0,
            },
        )]);

        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 0);
        assert!(agent.alive, "agent should remain alive at zero qi");
    }

    #[test]
    fn moving_within_zone_is_free() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Walker", 2, Position::origin());

        let _tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move {
                dx: 1,
                dy: 0,
                dz: 0,
            },
        )]);

        assert!(_tick.rejections.is_empty());
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 1); // cost to move
    }

    #[test]
    fn move_within_limit_allowed() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Walker", 3, Position::origin());

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move {
                dx: MAX_MOVE_RADIUS,
                dy: 0,
                dz: 0,
            },
        )]);

        assert!(tick.rejections.is_empty());
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 2); // 3 start -1 move
    }

    #[test]
    fn move_beyond_limit_is_rejected() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Walker", 4, Position::origin());

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move {
                dx: MAX_MOVE_RADIUS + 1,
                dy: 0,
                dz: 0,
            },
        )]);

        assert_eq!(tick.rejections.len(), 1);
        assert!(matches!(
            tick.rejections[0].error,
            ActionError::MoveOutOfRange { .. }
        ));
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 4); // no charge on rejection
    }

    #[test]
    fn move_into_occupied_is_rejected() {
        let mut vm = Vm::new();
        let a1 = vm.spawn_agent("A1", 3, Position::origin());
        let _a2 = vm.spawn_agent("A2", 3, Position::origin()); // gets shifted to (1,0,0)

        let tick = vm.step(&[ActionRequest::new(
            a1,
            Action::Move {
                dx: 1,
                dy: 0,
                dz: 0,
            },
        )]);

        assert_eq!(tick.rejections.len(), 1);
        assert!(matches!(
            tick.rejections[0].error,
            ActionError::PositionOccupied { target, .. } if target == Position { x:1, y:0, z:0 }
        ));
    }

    #[test]
    fn dead_agents_cannot_act() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Orchid", 1, Position::origin());

        vm.kill_agent(agent_id, DeathReason::Hazard).unwrap();

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move {
                dx: 1,
                dy: 0,
                dz: 0,
            },
        )]);

        assert_eq!(tick.rejections.len(), 1);
        assert!(matches!(
            tick.rejections[0].error,
            ActionError::AgentDead(id) if id == agent_id
        ));
    }

    #[test]
    fn scanning_is_free() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Scout", 3, Position::origin());

        let tick = vm.step(&[ActionRequest::new(agent_id, Action::Scan)]);

        assert!(tick.rejections.is_empty());
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 3); // scan is free
        assert!(tick.events.iter().any(|e| matches!(
            e,
            Event::ActionObserved { agent_id: id, action: "scan" } if *id == agent_id
        )));
    }

    #[test]
    fn build_structure_costs_qi_and_records() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Builder", 3, Position::origin());

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::BuildStructure {
                kind: StructureKind::Basic,
            },
        )]);

        assert!(tick.rejections.is_empty());
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 2); // 3 start -1 build
        assert_eq!(vm.world.structures.len(), 1);
        assert!(
            tick.events.iter().any(
                |e| matches!(e, Event::StructureBuilt { agent_id: id, .. } if *id == agent_id)
            )
        );
        assert!(tick.events.iter().any(|e| matches!(
            e,
            Event::QiSpent {
                action: "build_structure",
                amount: 1,
                ..
            }
        )));
    }

    #[test]
    fn cannot_build_on_existing_structure() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Builder", 5, Position::origin());

        let _ = vm.step(&[ActionRequest::new(
            agent_id,
            Action::BuildStructure {
                kind: StructureKind::Basic,
            },
        )]);
        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::BuildStructure {
                kind: StructureKind::Basic,
            },
        )]);

        assert_eq!(tick.rejections.len(), 1);
        assert!(matches!(
            tick.rejections[0].error,
            ActionError::StructureSpaceOccupied { .. }
        ));
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 4); // 5 start -1 build (second attempt rejected, no charge)
    }

    #[test]
    fn qi_sources_recharge_each_tick() {
        let mut vm = Vm::new();
        vm.set_max_qi_supply(10);
        let source_id = vm.seed_qi_source(Position::origin(), 10, 2);
        if let Some(src) = vm.world.qi_sources.iter_mut().find(|s| s.id == source_id) {
            src.current = 1;
        }

        let _ = vm.step(&[]);
        let level_after_first = vm
            .world
            .qi_sources
            .iter()
            .find(|s| s.id == source_id)
            .map(|s| s.current)
            .unwrap();
        assert_eq!(level_after_first, 3);

        let _ = vm.step(&[]);
        let level_after_second = vm
            .world
            .qi_sources
            .iter()
            .find(|s| s.id == source_id)
            .map(|s| s.current)
            .unwrap();
        assert_eq!(level_after_second, 5);
    }

    #[test]
    fn qi_recharge_respects_global_cap() {
        let mut vm = Vm::new();
        vm.set_max_qi_supply(5);
        let source_id = vm.seed_qi_source(Position::origin(), 10, 3);
        if let Some(src) = vm.world.qi_sources.iter_mut().find(|s| s.id == source_id) {
            src.current = 0;
        }

        let _ = vm.step(&[]);
        let after_first = vm
            .world
            .qi_sources
            .iter()
            .find(|s| s.id == source_id)
            .map(|s| s.current)
            .unwrap();
        assert_eq!(after_first, 3);

        let _ = vm.step(&[]);
        let after_second = vm
            .world
            .qi_sources
            .iter()
            .find(|s| s.id == source_id)
            .map(|s| s.current)
            .unwrap();
        assert_eq!(after_second, 5); // capped by global supply
    }

    #[test]
    fn scan_reports_local_state() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Scout", 3, Position::origin());
        vm.seed_qi_source(Position { x: 1, y: 0, z: 0 }, 5, 0);

        let _ = vm.step(&[ActionRequest::new(
            agent_id,
            Action::BuildStructure {
                kind: StructureKind::Basic,
            },
        )]);

        let tick = vm.step(&[ActionRequest::new(agent_id, Action::Scan)]);

        let report = tick.events.iter().find_map(|e| match e {
            Event::ScanReport {
                agent_id: id,
                nearby_qi_sources,
                nearby_structures,
                ..
            } if *id == agent_id => Some((nearby_qi_sources.clone(), nearby_structures.clone())),
            _ => None,
        });

        let (sources, structures) = report.expect("scan report missing");
        assert!(
            sources
                .iter()
                .any(|s| s.position == Position { x: 1, y: 0, z: 0 }),
            "expected qi source near agent"
        );
        assert_eq!(structures.len(), 1);
    }

    #[test]
    fn harvests_qi_from_nearest_node() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Harvester", 3, Position::origin());
        let src_id = vm.seed_qi_source(Position { x: 1, y: 0, z: 0 }, 5, 0);

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::HarvestOre {
                ore: OreKind::Qi,
                source_id: 0,
            },
        )]);

        assert!(tick.rejections.is_empty());
        let agent = vm.world().agent(agent_id).unwrap();
        // cost 1, gain min(5, HARVEST_PER_ACTION=3) = 3
        assert_eq!(agent.qi, 5);
        assert!(tick.events.iter().any(|e| matches!(
            e,
            Event::OreNodeHarvested { ore, source_id, amount, .. } if *ore == OreKind::Qi && *source_id == src_id && *amount == HARVEST_PER_ACTION
        )));
    }

    #[test]
    fn harvest_fails_without_source() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Harvester", 3, Position::origin());

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::HarvestOre {
                ore: OreKind::Qi,
                source_id: 42,
            },
        )]);

        assert_eq!(tick.rejections.len(), 1);
        assert!(matches!(
            tick.rejections[0].error,
            ActionError::OreSourceUnavailable { .. }
        ));
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 3);
    }

    #[test]
    fn harvest_rejected_when_depleted() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Harvester", 3, Position::origin());
        let src_id = vm.seed_qi_source(Position::origin(), 10, 1);
        if let Some(src) = vm.world.qi_sources.iter_mut().find(|s| s.id == src_id) {
            src.current = 1;
        }

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::HarvestOre {
                ore: OreKind::Qi,
                source_id: src_id,
            },
        )]);

        assert_eq!(tick.rejections.len(), 1);
        assert!(matches!(
            tick.rejections[0].error,
            ActionError::OreSourceDepleted {
                ore,
                source_id,
                available,
                ..
            } if ore == OreKind::Qi && source_id == src_id && available < HARVEST_PER_ACTION
        ));
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 3);
    }

    #[test]
    fn programmable_structure_requires_transistors() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Builder", 3, Position::origin());

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::BuildStructure {
                kind: StructureKind::Programmable,
            },
        )]);

        assert_eq!(tick.rejections.len(), 1);
        assert!(matches!(
            tick.rejections[0].error,
            ActionError::InsufficientOre {
                ore: OreKind::Transistor,
                ..
            }
        ));
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 3);
        assert_eq!(agent.transistors, 0);
    }

    #[test]
    fn harvest_transistor_and_build_programmable() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Tinkerer", 4, Position::origin());
        vm.seed_ore_source(OreKind::Transistor, Position::origin(), 5, 0);

        let harvest_tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::HarvestOre {
                ore: OreKind::Transistor,
                source_id: 0,
            },
        )]);

        assert!(harvest_tick.rejections.is_empty());
        let agent_after_harvest = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent_after_harvest.transistors, HARVEST_PER_ACTION);
        assert_eq!(agent_after_harvest.qi, 3); // 4 start -1 harvest cost

        let build_tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::BuildStructure {
                kind: StructureKind::Programmable,
            },
        )]);

        assert!(build_tick.rejections.is_empty());
        let agent_after_build = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent_after_build.transistors, HARVEST_PER_ACTION - 1);
        assert_eq!(agent_after_build.qi, 2); // 3 after harvest -1 build cost
        assert_eq!(vm.world.structures.len(), 1);
        assert!(build_tick.events.iter().any(|e| matches!(
            e,
            Event::StructureBuilt {
                kind: StructureKind::Programmable,
                ..
            }
        )));
    }
}
