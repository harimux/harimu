use std::collections::{HashMap, HashSet};
use std::fmt;

use sha2::{Digest, Sha256};

use crate::modules::structure::{Structure, StructureKind};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QiSource {
    pub id: u64,
    pub position: Position,
    pub capacity: Qi,
    pub current: Qi,
    pub recharge_per_tick: Qi,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QiSourceSnapshot {
    pub id: u64,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    Idle,
}

impl Action {
    pub const fn qi_cost(&self) -> Qi {
        match self {
            Action::Scan | Action::Idle => 0,
            Action::Move { .. } => 0,
            Action::Reproduce { .. } => 0,
            Action::BuildStructure { .. } => 1,
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Action::Scan => "scan",
            Action::Move { .. } => "move",
            Action::Reproduce { .. } => "reproduce",
            Action::BuildStructure { .. } => "build_structure",
            Action::Idle => "idle",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    TickStarted { tick: u64 },
    TickCompleted { tick: u64 },
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
    QiGained {
        agent_id: AgentId,
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
    pub position: Position,
    pub alive: bool,
    pub age: u64,
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
}

#[derive(Debug, Default)]
pub struct World {
    pub(crate) tick: u64,
    next_agent_id: AgentId,
    next_structure_id: u64,
    next_qi_source_id: u64,
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
            position: pos,
            alive: true,
            age: 0,
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

    pub fn add_qi_source(
        &mut self,
        position: Position,
        capacity: Qi,
        recharge_per_tick: Qi,
    ) -> u64 {
        let id = self.next_qi_source_id;
        self.next_qi_source_id += 1;
        let source = QiSource {
            id,
            position,
            capacity,
            current: capacity,
            recharge_per_tick,
        };
        self.qi_sources.push(source);
        id
    }

    fn recharge_qi_sources(&mut self) {
        for source in &mut self.qi_sources {
            let new_level = source
                .current
                .saturating_add(source.recharge_per_tick);
            source.current = new_level.min(source.capacity);
        }
    }

    fn nearby_qi_sources(&self, position: Position, range: i32) -> Vec<QiSourceSnapshot> {
        self.qi_sources
            .iter()
            .filter(|s| s.position.within_range(position, range))
            .map(|s| QiSourceSnapshot {
                id: s.id,
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

    pub fn spawn_agent(
        &mut self,
        name: impl Into<String>,
        qi: Qi,
        position: Position,
    ) -> AgentId {
        self.world.spawn_agent(name, qi, position)
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
        self.world.events.push(Event::AgentDied { agent_id, reason });
        Ok(())
    }

    pub fn seed_qi_source(
        &mut self,
        position: Position,
        capacity: Qi,
        recharge_per_tick: Qi,
    ) -> u64 {
        self.world.add_qi_source(position, capacity, recharge_per_tick)
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
                        .ok_or(ActionError::PartnerNotFound {
                            agent_id,
                            partner,
                        })?;
                    if !partner_alive {
                        return Err(ActionError::PartnerNotFound {
                            agent_id,
                            partner,
                        });
                    }
                    if agent_zone != partner_pos.zone() {
                        return Err(ActionError::PartnerOutOfZone {
                            agent_id,
                            partner,
                        });
                    }

                    let pair = if agent_id < partner {
                        (agent_id, partner)
                    } else {
                        (partner, agent_id)
                    };
                    if !mutual_pairs.contains(&pair) {
                        return Err(ActionError::ReproductionDeclined {
                            agent_id,
                            partner,
                        });
                    }

                    agent.spend_qi(1)?;
                    events.push(Event::QiSpent {
                        agent_id: agent.id,
                        amount: 1,
                        action: request.action.label(),
                    });

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

                    let cost = Action::BuildStructure { kind }.qi_cost();
                    if cost > 0 {
                        agent.spend_qi(cost)?;
                        events.push(Event::QiSpent {
                            agent_id: agent.id,
                            amount: cost,
                            action: request.action.label(),
                        });
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
                Action::Idle => {}
            }

            agent.age += 1;
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

        Ok(events)
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
            Action::Move { dx: 1, dy: 0, dz: 0 },
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
            Action::Move { dx: 1, dy: 0, dz: 0 },
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
            Action::Move { dx: 1, dy: 0, dz: 0 },
        )]);

        assert!(_tick.rejections.is_empty());
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 1); // cost to move
    }

    #[test]
    fn moving_across_zone_costs_qi() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Walker", 3, Position::origin());

        let tick = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move { dx: ZONE_SIZE, dy: 0, dz: 0 },
        )]);

        assert!(tick.rejections.is_empty());
        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 2); // 3 start -1 move
    }

    #[test]
    fn moving_across_zone_twice_only_costs_once() {
        let mut vm = Vm::new();
        let agent_id = vm.spawn_agent("Walker", 4, Position::origin());

        let _ = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move { dx: ZONE_SIZE, dy: 0, dz: 0 },
        )]);
        let _ = vm.step(&[ActionRequest::new(
            agent_id,
            Action::Move { dx: -ZONE_SIZE, dy: 0, dz: 0 },
        )]);

        let agent = vm.world().agent(agent_id).unwrap();
        assert_eq!(agent.qi, 2); // 4 start -1 move *2
    }

    #[test]
    fn move_into_occupied_is_rejected() {
        let mut vm = Vm::new();
        let a1 = vm.spawn_agent("A1", 3, Position::origin());
        let _a2 = vm.spawn_agent("A2", 3, Position::origin()); // gets shifted to (1,0,0)

        let tick = vm.step(&[ActionRequest::new(
            a1,
            Action::Move { dx: 1, dy: 0, dz: 0 },
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
            Action::Move { dx: 1, dy: 0, dz: 0 },
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
        assert!(tick
            .events
            .iter()
            .any(|e| matches!(e, Event::StructureBuilt { agent_id: id, .. } if *id == agent_id)));
        assert!(tick.events.iter().any(|e| matches!(
            e,
            Event::QiSpent { action: "build_structure", amount: 1, .. }
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
            } if *id == agent_id => {
                Some((nearby_qi_sources.clone(), nearby_structures.clone()))
            }
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
}
