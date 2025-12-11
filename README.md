# Harimu

Local sandbox for the Harimu artificial life simulation: agents with LLM-driven planning, Qi as energy, deterministic ticks, and programmable structures. See `whitepaper.md` for the full protocol and world design.

## Prerequisites

- Rust toolchain (cargo)
- Optional: Ollama running locally if you want `--brain llm` planning (defaults to `http://127.0.0.1:11434` and model `llama2`)

## Build

```bash
cargo build
```

You can also run the CLI directly via `cargo run -- <command>`.

## CLI Quickstart

State and wallets live under `.harimu/` in the repo root (`state.json`, `wallets.json`).

```bash
# Initialize runtime state
cargo run -- init

# Create a wallet (random address) and check balance
cargo run -- wallet create
cargo run -- wallet balance

# Mine Qi into the first wallet (simple PoW); limit iterations to avoid long runs
cargo run -- mine --iterations 5

# Create and inspect an agent
cargo run -- agent create
cargo run -- agent list
cargo run -- agent info <agent_id>

# Infuse Qi or vote on an action hash
cargo run -- agent infuse --agent-id <agent_id> --amount 10
cargo run -- agent vote --action-id <hash> --direction up

# Start a tick loop (default LLM planner; uses Ollama host/model/timeouts above)
cargo run -- start --ticks 5

# Or run deterministic looped actions instead of LLM planning
cargo run -- start --ticks 5 --brain loop --action scan --action move:1,0,0

# Check status / stop
cargo run -- status
cargo run -- stop
```

### Notable flags (start)

- `--agent <addr>`: run a specific agent; otherwise all registered agents spawn with their stored Qi.
- `--qi <n>`: starting Qi if the agent is new (default 3).
- `--position x,y,z`: spawn position (default `0,0,0`).
- `--tick-rate <f64>` or `--delay-ms <u64>`: pacing between ticks.
- `--llm-host` / `--llm-model` / `--llm-timeout-ms`: Ollama config when `--brain llm`.
- `--action <...>`: repeatable; choose from `scan`, `idle`, or `move:dx,dy,dz` (more actions available via the LLM planner).

## Project Map

- `src/main.rs`: CLI entrypoint.
- `src/commands/`: clap definitions and command dispatch.
- `src/modules/`: runtime (VM, agents, wallet, state).
- `whitepaper.md`: protocol/world design.
