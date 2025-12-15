# Harimu

Local sandbox for the Harimu artificial life simulation: agents with LLM-driven planning, Qi as energy, deterministic ticks, and programmable structures. See `whitepaper.md` for the full protocol and world design.

## Prerequisites

- Rust toolchain (cargo)
- Optional: LLM endpoint if you want `--brain llm` planning (defaults to OpenAI-compatible host `https://api.openai.com` and model `gpt-5-nano`; set `LLM_API_KEY`)

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

# Seed Qi nodes (persisted) and list them (charges the first wallet by default)
cargo run -- world infuse --amount 30 --recharge 2 --spread 0,0,0,12
cargo run -- world list --ore

# Start a tick loop (default LLM planner; defaults: provider=openai, host=https://api.openai.com, model=gpt-5-nano; set LLM_API_KEY)
cargo run -- start --ticks 5

# Use a different OpenAI-compatible endpoint (e.g., glm-4.6:cloud); host should point to the chat completion URL root
LLM_API_KEY=<your_key> cargo run -- start --ticks 5 --llm-provider openai --llm-host <hostname> --llm-model <model>

# Or use Ollama locally
cargo run -- start --ticks 5 --llm-provider ollama --llm-host http://127.0.0.1:11434 --llm-model glm-4.6:cloud

# Or run deterministic looped actions instead of LLM planning
cargo run -- start --ticks 5 --brain loop --action scan --action move:1,0,0

# Check status / stop
cargo run -- status
cargo run -- stop
```

## World Viewer (Godot)

- `cargo run -- world view` builds the bundled Godot viewer and launches a window (requires `godot4` or `godot` on PATH). Use `--no-launch` to skip launching or `--json` to print the snapshot.
- The viewer lives under `godot/`: Rust GDExtension in `godot/extension/`, Godot project in `godot/project/`.
- Snapshots are also written after each tick to `.harimu/world_snapshot.json` and can be consumed directly if you want to build your own renderer.
- Agents now have a default lifespan of 112 ticks; extend it with `cargo run -- agent extend-life --agent-id <id> --max-age <ticks>`.

### Notable flags (start)

- `--agent <addr>`: run a specific agent; otherwise all registered agents spawn with their stored Qi.
- `--qi <n>`: starting Qi if the agent is new (default 3).
- `--position x,y,z`: spawn position (default `0,0,0`).
- `--tick-rate <f64>` or `--delay-ms <u64>`: pacing between ticks.
- `--llm-host` / `--llm-model` / `--llm-timeout-ms`: Ollama config when `--brain llm`.
- `--llm-provider`: `ollama` (default) or `openai` for OpenAI-compatible endpoints.
- `--llm-api-key` (or env `LLM_API_KEY`): API key for OpenAI-compatible providers.
- `--action <...>`: repeatable; choose from `scan`, `idle`, or `move:dx,dy,dz` (more actions available via the LLM planner).

## Project Map

- `src/main.rs`: CLI entrypoint.
- `src/commands/`: clap definitions and command dispatch.
- `src/modules/`: runtime (VM, agents, wallet, state).
- `whitepaper.md`: protocol/world design.
