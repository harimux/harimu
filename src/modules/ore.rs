use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OreKind {
    Qi,
    Transistor,
}

impl OreKind {
    pub const fn label(self) -> &'static str {
        match self {
            OreKind::Qi => "qi",
            OreKind::Transistor => "transistor",
        }
    }
}

impl Default for OreKind {
    fn default() -> Self {
        OreKind::Qi
    }
}

impl fmt::Display for OreKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

impl FromStr for OreKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "qi" => Ok(OreKind::Qi),
            "transistor" => Ok(OreKind::Transistor),
            _ => Err(()),
        }
    }
}
