use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::modules::vm::{POW_DIFFICULTY_BYTES, POW_REWARD, Qi};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub address: String,
    pub balance: Qi,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WalletStore {
    pub wallets: HashMap<String, Wallet>,
}

impl WalletStore {
    pub fn load() -> io::Result<Self> {
        let path = wallet_path();
        if !path.exists() {
            return Ok(WalletStore::default());
        }

        let data = fs::read(path)?;
        if data.is_empty() {
            return Ok(WalletStore::default());
        }

        let store: WalletStore = serde_json::from_slice(&data).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "failed to parse wallet store {}; delete it to reset: {}",
                    wallet_path().display(),
                    e
                ),
            )
        })?;

        Ok(store)
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = wallet_dir();
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_vec_pretty(self)?;
        fs::write(wallet_path(), json)?;
        Ok(())
    }

    pub fn upsert_wallet(&mut self, wallet: Wallet) {
        self.wallets.insert(wallet.address.clone(), wallet);
    }

    pub fn get_wallet(&self, address: &str) -> Option<&Wallet> {
        self.wallets.get(address)
    }

    pub fn get_wallet_mut(&mut self, address: &str) -> Option<&mut Wallet> {
        self.wallets.get_mut(address)
    }

    pub fn first_wallet(&self) -> Option<&Wallet> {
        self.wallets.values().next()
    }
}

fn wallet_dir() -> PathBuf {
    PathBuf::from(".harimu")
}

fn wallet_path() -> PathBuf {
    wallet_dir().join("wallets.json")
}

pub fn create_wallet() -> io::Result<Wallet> {
    let mut bytes = [0u8; 20];
    OsRng.fill_bytes(&mut bytes);
    let address = hex::encode(bytes);
    Ok(Wallet {
        address,
        balance: 0,
    })
}

pub fn transfer(store: &mut WalletStore, from: &str, to: &str, amount: Qi) -> Result<(), String> {
    if amount == 0 || from == to {
        return Ok(());
    }

    {
        let from_wallet = store
            .get_wallet_mut(from)
            .ok_or_else(|| format!("sender wallet {} not found", from))?;
        if from_wallet.balance < amount {
            return Err(format!(
                "insufficient balance: have {}, need {}",
                from_wallet.balance, amount
            ));
        }
        from_wallet.balance -= amount;
    }

    let to_wallet = store
        .get_wallet_mut(to)
        .ok_or_else(|| format!("recipient wallet {} not found", to))?;

    to_wallet.balance = to_wallet.balance.saturating_add(amount);

    Ok(())
}

pub fn wallet_pow_valid(address: &str, nonce: u64) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(address.as_bytes());
    hasher.update(nonce.to_le_bytes());
    let hash = hasher.finalize();
    hash.iter().take(POW_DIFFICULTY_BYTES).all(|b| *b == 0)
}

pub fn wallet_pow_solve(address: &str, start_nonce: u64) -> u64 {
    let mut nonce = start_nonce;
    loop {
        if wallet_pow_valid(address, nonce) {
            return nonce;
        }
        nonce = nonce.wrapping_add(1);
    }
}

pub fn mine(store: &mut WalletStore, address: &str, start_nonce: u64) -> Result<(u64, Qi), String> {
    let wallet = store
        .get_wallet_mut(address)
        .ok_or_else(|| format!("wallet {} not found", address))?;

    let nonce = wallet_pow_solve(address, start_nonce);
    let reward = POW_REWARD;
    wallet.balance = wallet.balance.saturating_add(reward);
    Ok((nonce, reward))
}
