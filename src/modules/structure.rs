use std::fmt;
use std::str::FromStr;

use crate::modules::vm::{AgentId, Position, Zone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureKind {
    Basic,
}

impl fmt::Display for StructureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructureKind::Basic => write!(f, "basic"),
        }
    }
}

impl FromStr for StructureKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "basic" => Ok(StructureKind::Basic),
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
