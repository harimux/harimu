use std::time::Duration;

use clap::Subcommand;
use harimu::{
    wallet::{self, WalletStore},
    POW_DIFFICULTY_BYTES, Qi,
};

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
        amount: Qi,
    },
}

pub(super) fn run_wallet(cmd: WalletCommand) -> Result<(), String> {
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
            println!("Transferred {} Qi from {} to {}", amount, from, to);
        }
    }

    Ok(())
}

pub(super) fn run_wallet_mine(
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
        address, start_nonce, POW_DIFFICULTY_BYTES
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
            store.get_wallet(&address).map(|w| w.balance).unwrap_or(0)
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
