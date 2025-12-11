use std::str::FromStr;
use std::time::Duration;

use clap::ValueEnum;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::modules::structure::StructureKind;
use crate::modules::vm::{Action, AgentId, Vm};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum BrainMode {
    /// Deterministic loop (uses provided/default action cycle)
    Loop,
    /// LLM-driven loop (mocked planner that chooses from candidates)
    Llm,
}

pub const DEFAULT_AGENT_GOAL: &str = "Evolve, survive, build machines, form territories, and develop civilizations inside a voxel-based, blockchain-synchronized environment.";

#[derive(Clone, Debug)]
pub enum ActionArg {
    Scan,
    Idle,
    Move { dx: i32, dy: i32, dz: i32 },
    Reproduce { partner: AgentId },
    BuildStructure { kind: StructureKind },
}

impl ActionArg {
    pub fn label(&self) -> String {
        match self {
            ActionArg::Scan => "scan".to_string(),
            ActionArg::Idle => "idle".to_string(),
            ActionArg::Move { dx, dy, dz } => format!("move({},{},{})", dx, dy, dz),
            ActionArg::Reproduce { partner } => {
                if *partner == 0 {
                    "reproduce(<partner>)".to_string()
                } else {
                    format!("reproduce({})", partner)
                }
            }
            ActionArg::BuildStructure { .. } => "build_structure".to_string(),
        }
    }

    pub fn materialize(&self, _agent_id: AgentId, _next_tick: u64) -> Action {
        match *self {
            ActionArg::Scan => Action::Scan,
            ActionArg::Idle => Action::Idle,
            ActionArg::Move { dx, dy, dz } => Action::Move { dx, dy, dz },
            ActionArg::Reproduce { partner } => Action::Reproduce { partner },
            ActionArg::BuildStructure { kind } => Action::BuildStructure { kind },
        }
    }
}

impl FromStr for ActionArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();
        let (verb, rest) = match trimmed.split_once(':') {
            Some((verb, rest)) => (verb.to_lowercase(), Some(rest)),
            None => (trimmed.to_lowercase(), None),
        };

        match verb.as_str() {
            "scan" => Ok(ActionArg::Scan),
            "idle" => Ok(ActionArg::Idle),
            "move" => {
                let coords = rest.ok_or("move requires dx,dy,dz e.g. move:1,0,-1")?;
                let parts: Vec<_> = coords.split(',').collect();
                if parts.len() != 3 {
                    return Err("move requires exactly three coordinates".into());
                }

                let dx = parts[0]
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| "dx must be an integer")?;
                let dy = parts[1]
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| "dy must be an integer")?;
                let dz = parts[2]
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| "dz must be an integer")?;

                Ok(ActionArg::Move { dx, dy, dz })
            }
            "reproduce" => {
                let partner = match rest {
                    Some(val) => val
                        .trim()
                        .parse::<AgentId>()
                        .map_err(|_| "partner must be an integer".to_string())?,
                    None => 0,
                };
                Ok(ActionArg::Reproduce { partner })
            }
            "build_structure" => {
                let kind = match rest {
                    Some(val) => StructureKind::from_str(val.trim())
                        .map_err(|_| "structure kind must be basic".to_string())?,
                    None => StructureKind::Basic,
                };
                Ok(ActionArg::BuildStructure { kind })
            }
            _ => Err(format!(
                "Unknown action '{}'. Use scan | idle | move:<dx>,<dy>,<dz> | reproduce:<agent_id> | build_structure[:kind]",
                verb
            )),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct BrainMemory {
    pub notes: Vec<String>,
}

const MEMORY_LIMIT: usize = 16;

#[derive(Debug, Clone)]
pub struct LlmDecision {
    pub summary: String,
    pub prompt: String,
    pub request_json: String,
    pub response: String,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub struct LlmClient {
    host: String,
    model: String,
    http: Client,
}

impl LlmClient {
    pub fn new(host: impl Into<String>, model: impl Into<String>, timeout: Duration) -> Result<Self, reqwest::Error> {
        let host = host.into();
        let model = model.into();
        let http = Client::builder().timeout(timeout).build()?;

        Ok(Self {
            host,
            model,
            http,
        })
    }
}

pub fn plan_with_llm(
    vm: &Vm,
    agent_id: AgentId,
    candidates: &[ActionArg],
    memory: &mut BrainMemory,
    client: Option<&LlmClient>,
    next_tick: u64,
) -> LlmDecision {
    let summary = summarize_world(vm, agent_id);
    let memory_text = memory_context(memory, MEMORY_LIMIT);
    let prompt = build_prompt(&summary, &memory_text, candidates, vm, agent_id);

    let fallback_action = || choose_action(vm, agent_id, candidates, next_tick);

    let (request_json, response, mut action) = match client {
        Some(client) => match call_ollama(client, &prompt, candidates, agent_id, next_tick) {
            Ok(result) => (result.request_json, result.reply_text, result.action),
            Err(err) => (
                String::from("not sent (error building/sending request)"),
                format!("error: {}", err),
                fallback_action(),
            ),
        },
        None => (
            String::from("not sent (no llm client)"),
            String::from("llm client missing; fallback to loop"),
            fallback_action(),
        ),
    };

    // Safety override if low on Qi.
    action = survival_override(vm, agent_id, candidates, next_tick, action);

    push_memory(
        memory,
        format!(
            "tick {} | state: {} | decision: {} | llm: {}",
            vm.world().tick(),
            summary,
            action_token(&action),
            truncate(&response, 120)
        ),
    );

    LlmDecision {
        summary,
        prompt,
        request_json,
        response,
        action,
    }
}

fn choose_action(vm: &Vm, agent_id: AgentId, candidates: &[ActionArg], next_tick: u64) -> Action {
    let qi = vm.world().agent(agent_id).map(|a| a.qi).unwrap_or(0);

    for action in candidates {
        let materialized = action.materialize(agent_id, next_tick);
        if materialized.qi_cost() <= qi {
            return materialized;
        }
    }

    Action::Idle
}

fn survival_override(
    _vm: &Vm,
    _agent_id: AgentId,
    _candidates: &[ActionArg],
    _next_tick: u64,
    action: Action,
) -> Action {
    action
}

fn summarize_world(vm: &Vm, agent_id: AgentId) -> String {
    if let Some(agent) = vm.world().agent(agent_id) {
        format!(
            "Agent {} at ({}, {}, {}) qi={} age={} last_tick={}",
            agent_id,
            agent.position.x,
            agent.position.y,
            agent.position.z,
            agent.qi,
            agent.age,
            vm.world().tick()
        )
    } else {
        format!("Agent {} missing; world tick={}", agent_id, vm.world().tick())
    }
}

fn memory_context(memory: &BrainMemory, limit: usize) -> String {
    if memory.notes.is_empty() {
        return "none".into();
    }
    let len = memory.notes.len();
    let start = len.saturating_sub(limit);
    memory.notes[start..].join(" | ")
}

fn push_memory(memory: &mut BrainMemory, entry: String) {
    memory.notes.push(entry);
    if memory.notes.len() > MEMORY_LIMIT {
        let drop = memory.notes.len() - MEMORY_LIMIT;
        memory.notes.drain(0..drop);
    }
}

fn build_prompt(
    summary: &str,
    memory_text: &str,
    candidates: &[ActionArg],
    _vm: &Vm,
    _agent_id: AgentId,
) -> String {
    let options = candidates
        .iter()
        .map(|a| a.label())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "memory=[{memory_text}]; state={summary}; actions=[{options}]; reply only TOON{{action=<label>}} from actions."
    )
}

fn call_ollama(
    client: &LlmClient,
    prompt: &str,
    candidates: &[ActionArg],
    agent_id: AgentId,
    next_tick: u64,
) -> Result<OllamaResult, String> {
    let url = format!("{}/api/chat", client.host.trim_end_matches('/'));

    let body = ChatRequest {
        model: client.model.clone(),
        stream: false,
        messages: vec![Message {
            role: "user".into(),
            content: prompt.into(),
        }],
    };

    let request_json =
        serde_json::to_string_pretty(&body).map_err(|e| format!("encode request: {}", e))?;

    let response: ChatResponse = client
        .http
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| format!("http: {}", e))?
        .json()
        .map_err(|e| format!("decode: {}", e))?;

    let text = response.message.content;
    let parsed = parse_action(&text, candidates, agent_id, next_tick);
    let action = parsed.clone().unwrap_or_else(|| choose_action_fallback(candidates, agent_id, next_tick));
    let reply_text = parsed
        .map(|a| format!("TOON{{action={}}}", action_token(&a)))
        .unwrap_or_else(|| truncate(&text, 120));

    Ok(OllamaResult {
        request_json,
        reply_text,
        action,
    })
}

fn parse_action(text: &str, candidates: &[ActionArg], agent_id: AgentId, next_tick: u64) -> Option<Action> {
    // Try JSON schema: { "action": "<label>" }
    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(action_str) = json_value
            .get("action")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
        {
            for action in candidates {
                if action.label().eq_ignore_ascii_case(action_str) {
                    return Some(action.materialize(agent_id, next_tick));
                }
            }
        }
    }

    // TOON style: look for action=<label>
    if let Some(idx) = text.to_lowercase().find("action=") {
        let slice = &text[idx + "action=".len()..];
        let slice = slice
            .trim_start_matches(|c: char| c.is_whitespace() || c == '{' || c == '[' || c == '(');
        let end = slice
            .find(|c: char| c.is_whitespace() || c == '}' || c == ']' || c == ')' || c == ',')
            .unwrap_or(slice.len());
        let label = slice[..end].trim();

        for action in candidates {
            if action.label().eq_ignore_ascii_case(label) {
                return Some(action.materialize(agent_id, next_tick));
            }
        }
    }

    // Fallback: `action:<label>` prefix
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("action:") {
            let label = rest.trim();
            for action in candidates {
                if action.label().eq_ignore_ascii_case(label) {
                    return Some(action.materialize(agent_id, next_tick));
                }
            }
        }
    }

    // Last resort: look for any candidate label substring
    for action in candidates {
        if text.to_lowercase().contains(&action.label().to_lowercase()) {
            return Some(action.materialize(agent_id, next_tick));
        }
    }

    None
}

fn choose_action_fallback(candidates: &[ActionArg], _agent_id: AgentId, _next_tick: u64) -> Action {
    // Default to idle if the LLM reply is unusable.
    candidates
        .first()
        .map(|_| Action::Idle)
        .unwrap_or(Action::Idle)
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

struct OllamaResult {
    request_json: String,
    reply_text: String,
    action: Action,
}

fn truncate(text: &str, max: usize) -> String {
    // Truncate on char boundaries to avoid UTF-8 panics when logs contain emoji.
    let mut chars = text.char_indices();
    let cutoff = match chars.nth(max) {
        Some((idx, _)) => idx,
        None => return text.to_string(),
    };
    format!("{}...", &text[..cutoff])
}

fn action_token(action: &Action) -> String {
    match action {
        Action::Scan => "scan".to_string(),
        Action::Idle => "idle".to_string(),
        Action::Move { dx, dy, dz } => format!("move({},{},{})", dx, dy, dz),
        Action::Reproduce { partner } => format!("reproduce({})", partner),
        Action::BuildStructure { kind } => format!("build_structure({})", kind),
    }
}
