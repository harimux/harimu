use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use clap::{ArgAction, Subcommand};
use harimu::{
    Position, Spread, load_structure_store, load_world_snapshot, save_world_snapshot,
    snapshot_from_persistent,
    world::{InfuseQiCommand, WorldCommands, WorldQueries},
};
use serde_json;

#[derive(Subcommand)]
pub enum WorldCommand {
    /// Infuse ore nodes into the world and persist them locally
    Infuse {
        /// Wallet address to fund the infusion (defaults to the first wallet)
        #[arg(long)]
        wallet: Option<String>,
        /// Total Qi to inject (convenient; splits into nodes automatically)
        #[arg(long)]
        amount: Option<harimu::Qi>,
        /// Number of Qi nodes to create (ignored when --amount is set)
        #[arg(long, default_value_t = 1)]
        count: u32,
        /// Capacity (Qi) for each node (used when --amount is not set; also used as chunk size when --amount is set)
        #[arg(long, default_value_t = 10)]
        capacity: harimu::Qi,
        /// Recharge per tick for each node
        #[arg(long, default_value_t = 1)]
        recharge: harimu::Qi,
        /// Center and radius for random placement: x,y,z,r (radius must be >= 0)
        #[arg(long, value_name = "x,y,z,r")]
        spread: Option<SpreadArg>,
        /// Optional RNG seed for reproducible placement
        #[arg(long)]
        seed: Option<u64>,
        /// Ore kind to infuse (qi or transistor). Transistors cost 100 Qi each.
        #[arg(long, default_value = "qi")]
        ore: harimu::OreKind,
    },
    /// List world elements
    List {
        /// Show ore nodes (Qi, transistor, etc.)
        #[arg(long)]
        ore: bool,
        /// Show agent-built structures
        #[arg(long)]
        structure: bool,
    },
    /// Export a world snapshot for visualization (e.g., Godot viewer)
    View {
        /// Print the snapshot JSON to stdout
        #[arg(long)]
        json: bool,
        /// Launch the bundled Godot viewer window (disable with --no-launch)
        #[arg(long = "no-launch", action = ArgAction::SetFalse, default_value_t = true)]
        launch: bool,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct SpreadArg(pub Spread);

impl FromStr for SpreadArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.trim().split(',').collect();
        if parts.len() != 4 {
            return Err("Spread must be formatted as x,y,z,r (radius >= 0)".into());
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
        let radius = parts[3]
            .trim()
            .parse::<i32>()
            .map_err(|_| "radius must be an integer")?;
        if radius < 0 {
            return Err("radius must be non-negative".into());
        }

        Ok(SpreadArg(Spread {
            center: Position { x, y, z },
            radius,
        }))
    }
}

pub(super) fn run_world(cmd: WorldCommand) -> Result<(), String> {
    match cmd {
        WorldCommand::Infuse {
            wallet,
            amount,
            count,
            capacity,
            recharge,
            spread,
            seed,
            ore,
        } => {
            let result = WorldCommands::infuse_qi(InfuseQiCommand {
                wallet,
                amount,
                count,
                capacity,
                recharge,
                spread: spread.map(|s| s.0).unwrap_or_default(),
                seed,
                ore,
            })?;

            println!(
                "Infused {} {} node(s) (recharge/tick={}) using wallet {} (charged {}, new balance {})",
                result.added.len(),
                result.ore,
                recharge,
                result.wallet_address,
                result.charged,
                result.wallet_balance
            );
            println!("Total Qi infused so far: {}", result.total_infused);
            let offset = result.total_after.saturating_sub(result.added.len());
            for (idx, src) in result.added.iter().enumerate() {
                println!(
                    " - node {} at ({}, {}, {}) capacity={} recharge/tick={} ore={}",
                    offset + idx + 1,
                    src.position.x,
                    src.position.y,
                    src.position.z,
                    src.capacity,
                    src.recharge_per_tick,
                    src.ore
                );
            }
        }
        WorldCommand::List { ore, structure } => {
            let show_ore = ore || (!ore && !structure);
            let show_structures = structure || (!ore && !structure);
            if show_ore {
                print_ore_nodes()?;
            }
            if show_structures {
                print_structures()?;
            }
        }
        WorldCommand::View { json, launch } => {
            let snapshot = match load_world_snapshot().map_err(|e| e.to_string())? {
                Some(s) => s,
                None => snapshot_from_persistent()?,
            };

            let path = save_world_snapshot(&snapshot)
                .map_err(|e| format!("failed to persist snapshot: {}", e))?;

            println!(
                "World snapshot: tick={} | agents={} | structures={} | ore_nodes={}",
                snapshot.tick,
                snapshot.agents.len(),
                snapshot.structures.len(),
                snapshot.ore_nodes.len()
            );
            println!(
                "Snapshot file written to {} (pass --json to print it here)",
                path.display()
            );

            if json {
                let json_str =
                    serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?;
                println!("{}", json_str);
            }

            if launch {
                launch_godot_viewer(&path)?;
            }
        }
    }

    Ok(())
}

fn print_ore_nodes() -> Result<(), String> {
    let store = WorldQueries::qi_sources()?;
    if store.sources.is_empty() {
                println!("No ore nodes infused yet. Use `harimu world infuse` to add some.");
    } else {
        println!("{} ore node(s):", store.sources.len());
        for (idx, src) in store.sources.iter().enumerate() {
            println!(
                " - {}: pos=({}, {}, {}) capacity={} recharge/tick={} ore={}",
                idx + 1,
                src.position.x,
                src.position.y,
                src.position.z,
                src.capacity,
                src.recharge_per_tick,
                src.ore
            );
        }
    }
    Ok(())
}

fn print_structures() -> Result<(), String> {
    let store = load_structure_store().map_err(|e| e.to_string())?;
    if store.structures.is_empty() {
        println!("No structures recorded yet.");
    } else {
        println!("{} structure(s):", store.structures.len());
        for s in &store.structures {
            println!(
                " - id={} kind={} owner={} pos=({}, {}, {}) zone=({},{},{})",
                s.id,
                s.kind,
                s.owner,
                s.position.x,
                s.position.y,
                s.position.z,
                s.zone.x,
                s.zone.y,
                s.zone.z
            );
        }
    }
    Ok(())
}

fn launch_godot_viewer(_snapshot_path: &Path) -> Result<(), String> {
    let manifest = Path::new("godot/extension/Cargo.toml");
    if !manifest.exists() {
        return Err("godot viewer crate missing (expected godot/extension/Cargo.toml)".into());
    }

    build_godot_extension(manifest)?;

    let lib_name = extension_filename();
    let built_lib = find_built_extension(&lib_name)
        .ok_or_else(|| format!("built library not found (looked for {} in target and godot/extension/target)", lib_name))?;

    let dest_dir = Path::new("godot/project")
        .join("addons")
        .join("harimu")
        .join("bin");
    fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    fs::copy(&built_lib, dest_dir.join(&lib_name)).map_err(|e| {
        format!(
            "failed to copy {} to viewer bin dir: {}",
            built_lib.display(),
            e
        )
    })?;

    let godot_bin = match find_godot_binary() {
        Ok(bin) => bin,
        Err(err) => {
            eprintln!(
                "warning: {}. Install Godot 4 CLI (godot4/godot) to auto-launch the viewer.",
                err
            );
            return Ok(());
        }
    };
    let status = Command::new(&godot_bin)
        .arg("--path")
        .arg("godot/project")
        .status()
        .map_err(|e| format!("failed to run {}: {}", godot_bin, e))?;

    if !status.success() {
        return Err(format!("godot viewer exited with status {}", status));
    }

    Ok(())
}

fn build_godot_extension(manifest: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--release")
        .status()
        .map_err(|e| format!("failed to run cargo build for viewer: {}", e))?;

    if !status.success() {
        return Err(format!("cargo build for godot viewer failed: {}", status));
    }

    Ok(())
}

fn find_godot_binary() -> Result<String, String> {
    const CANDIDATES: [&str; 2] = ["godot4", "godot"];
    for name in CANDIDATES {
        if which::which(name).is_ok() {
            return Ok(name.to_string());
        }
    }
    Err("godot executable not found (tried godot4, godot)".into())
}

fn extension_filename() -> String {
    match std::env::consts::OS {
        "macos" => "libharimu_godot_viewer.dylib".into(),
        "windows" => "harimu_godot_viewer.dll".into(),
        _ => "libharimu_godot_viewer.so".into(),
    }
}

fn find_built_extension(lib_name: &str) -> Option<PathBuf> {
    let candidates = [
        Path::new("target").join("release").join(lib_name),
        Path::new("target").join("debug").join(lib_name),
        Path::new("godot")
            .join("extension")
            .join("target")
            .join("release")
            .join(lib_name),
        Path::new("godot")
            .join("extension")
            .join("target")
            .join("debug")
            .join(lib_name),
    ];

    for cand in candidates {
        if cand.exists() {
            return Some(cand);
        }
    }

    None
}
