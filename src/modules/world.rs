use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::modules::ore::OreKind;
use crate::modules::qi::{self, QiSourceSpec, QiSourceStore, Spread};
use crate::modules::vm::{Position, Qi};
use crate::modules::wallet::WalletStore;

const DEFAULT_CHUNK: Qi = 10;

#[derive(Debug, Clone)]
pub struct InfuseQiCommand {
    pub wallet: Option<String>,
    pub amount: Option<Qi>,
    pub count: u32,
    pub capacity: Qi,
    pub recharge: Qi,
    pub spread: Spread,
    pub seed: Option<u64>,
    pub ore: OreKind,
}

#[derive(Debug, Clone)]
pub struct InfuseQiResult {
    pub added: Vec<QiSourceSpec>,
    pub total_after: usize,
    pub total_infused: u64,
    pub wallet_address: String,
    pub wallet_balance: Qi,
    pub charged: Qi,
    pub ore: OreKind,
}

/// Command-side world mutations.
pub struct WorldCommands;

impl WorldCommands {
    pub fn infuse_qi(cmd: InfuseQiCommand) -> Result<InfuseQiResult, String> {
        let mut wallet_store = WalletStore::load().map_err(|e| e.to_string())?;
        let wallet_address = match &cmd.wallet {
            Some(addr) => addr.clone(),
            None => wallet_store
                .first_wallet()
                .map(|w| w.address.clone())
                .ok_or_else(|| "no wallets found; create one first".to_string())?,
        };

        let mut rng = match cmd.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_entropy(),
        };

        let specs = build_specs(&cmd, &mut rng)?;
        let charged: Qi = specs
            .iter()
            .map(|s| s.capacity)
            .fold(0u64, |acc, v| acc.saturating_add(v as u64))
            .try_into()
            .map_err(|_| "total Qi exceeds u32".to_string())?;

        // Non-qi ore is priced in Qi at a flat rate per unit.
        let cost_multiplier: u64 = match cmd.ore {
            OreKind::Qi => 1,
            OreKind::Transistor => 100,
        };
        let charged = charged
            .saturating_mul(cost_multiplier as Qi)
            .try_into()
            .map_err(|_| "ore cost exceeds u32".to_string())?;

        {
            let wallet = wallet_store
                .get_wallet_mut(&wallet_address)
                .ok_or_else(|| format!("wallet {} not found", wallet_address))?;
            if wallet.balance < charged {
                return Err(format!(
                    "insufficient wallet balance: have {}, need {}",
                    wallet.balance, charged
                ));
            }
            wallet.balance = wallet.balance.saturating_sub(charged);
        }
        wallet_store.save().map_err(|e| e.to_string())?;

        let mut qi_store = qi::load().map_err(|e| e.to_string())?;
        let total_after = qi_store.sources.len().saturating_add(specs.len());
        let previous_total = qi_store.total_qi_infused;
        qi_store.total_qi_infused = previous_total.saturating_add(charged as u64);
        qi_store.sources.extend(specs.iter().cloned());

        if let Err(err) = qi::save(&qi_store) {
            // Try to revert wallet charge on failure to persist nodes.
            if let Some(wallet) = wallet_store.get_wallet_mut(&wallet_address) {
                wallet.balance = wallet.balance.saturating_add(charged);
                let _ = wallet_store.save();
            }
            return Err(err.to_string());
        }

        let wallet_balance = wallet_store
            .get_wallet(&wallet_address)
            .map(|w| w.balance)
            .unwrap_or(0);

        Ok(InfuseQiResult {
            added: specs,
            total_after,
            total_infused: qi_store.total_qi_infused,
            wallet_address,
            wallet_balance,
            charged,
            ore: cmd.ore,
        })
    }
}

/// Query-side world reads.
pub struct WorldQueries;

impl WorldQueries {
    pub fn qi_sources() -> Result<QiSourceStore, String> {
        qi::load().map_err(|e| e.to_string())
    }
}

fn build_specs(cmd: &InfuseQiCommand, rng: &mut StdRng) -> Result<Vec<QiSourceSpec>, String> {
    let spread = cmd.spread;
    let recharge = cmd.recharge;
    let capacity = cmd.capacity;
    let mut specs = Vec::new();

    if let Some(total) = cmd.amount {
        if total == 0 {
            return Err("amount must be greater than 0".into());
        }
        let chunk = if capacity > 0 {
            capacity
        } else {
            DEFAULT_CHUNK
        };
        let mut remaining = total;
        while remaining > 0 {
            let cap = remaining.min(chunk);
            specs.push(QiSourceSpec {
                position: random_position(spread, rng),
                capacity: cap,
                recharge_per_tick: recharge,
                ore: cmd.ore,
            });
            remaining = remaining.saturating_sub(cap);
        }
    } else {
        if cmd.count == 0 {
            return Err("count must be at least 1".into());
        }
        for _ in 0..cmd.count {
            specs.push(QiSourceSpec {
                position: random_position(spread, rng),
                capacity,
                recharge_per_tick: recharge,
                ore: cmd.ore,
            });
        }
    }

    Ok(specs)
}

fn random_position(spread: Spread, rng: &mut StdRng) -> Position {
    let radius = spread.radius.max(0);
    Position {
        x: spread.center.x + rng.gen_range(-radius..=radius),
        y: spread.center.y + rng.gen_range(-radius..=radius),
        z: spread.center.z + rng.gen_range(-radius..=radius),
    }
}
