use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

use chrono::Utc;
use clap::ValueEnum;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_toon::to_string_pretty;
use std::fs;
use std::path::PathBuf;

use crate::modules::ore::OreKind;
use crate::modules::structure::StructureKind;
use crate::modules::vm::{Action, AgentId, SCAN_RANGE, Vm};

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
    HarvestOre { ore: OreKind, source_id: u64 },
}

impl ActionArg {
    pub fn label(&self) -> String {
        match self {
            ActionArg::Scan => "scan".to_string(),
            ActionArg::Idle => "idle".to_string(),
            ActionArg::Move { .. } => "move".to_string(),
            ActionArg::Reproduce { .. } => "reproduce".to_string(),
            ActionArg::BuildStructure { kind } => format!("build_{}", kind),
            ActionArg::HarvestOre { ore, .. } => format!("harvest_{}", ore),
        }
    }

    pub fn materialize(&self, _agent_id: AgentId, _next_tick: u64) -> Action {
        match *self {
            ActionArg::Scan => Action::Scan,
            ActionArg::Idle => Action::Idle,
            ActionArg::Move { dx, dy, dz } => Action::Move { dx, dy, dz },
            ActionArg::Reproduce { partner } => Action::Reproduce { partner },
            ActionArg::BuildStructure { kind } => Action::BuildStructure { kind },
            ActionArg::HarvestOre { ore, source_id } => Action::HarvestOre { ore, source_id },
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
            v if v.starts_with("build") => {
                let kind = if v == "build_structure" {
                    StructureKind::Basic
                } else if let Some(suffix) = v.strip_prefix("build_") {
                    StructureKind::from_str(suffix.trim())
                        .map_err(|_| "unknown structure kind; use basic|programmable|qi")?
                } else if let Some(val) = rest {
                    StructureKind::from_str(val.trim())
                        .map_err(|_| "structure kind must be basic|programmable|qi".to_string())?
                } else {
                    StructureKind::Basic
                };
                Ok(ActionArg::BuildStructure { kind })
            }
            v if v.starts_with("harvest") => {
                let mut ore = if let Some(suffix) = v.strip_prefix("harvest_") {
                    <OreKind as FromStr>::from_str(suffix.trim()).unwrap_or_default()
                } else {
                    OreKind::Qi
                };

                let mut source_id = 0u64;
                if let Some(val) = rest {
                    let parts: Vec<_> = val.split(',').collect();
                    if parts.len() == 1 {
                        let token = parts[0].trim();
                        if let Ok(id) = token.parse::<u64>() {
                            source_id = id;
                        } else if let Ok(parsed_ore) = <OreKind as FromStr>::from_str(token) {
                            ore = parsed_ore;
                        } else {
                            return Err("harvest expects source_id or ore,source_id".into());
                        }
                    } else {
                        ore = <OreKind as FromStr>::from_str(parts[0].trim())
                            .map_err(|_| "ore must be qi or transistor")?;
                        source_id = parts[1]
                            .trim()
                            .parse::<u64>()
                            .map_err(|_| "source_id must be an integer".to_string())?;
                    }
                }

                Ok(ActionArg::HarvestOre { ore, source_id })
            }
            _ => Err(format!(
                "Unknown action '{}'. Use scan | idle | move:<dx>,<dy>,<dz> | reproduce:<agent_id> | build[:kind] | harvest[:ore,source_id]",
                verb
            )),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct BrainMemory {
    pub notes: Vec<String>,
}

const MEMORY_LIMIT: usize = 5;

#[derive(Debug, Clone)]
pub struct LlmDecision {
    pub summary: String,
    pub observations: Vec<String>,
    pub prompt: String,
    pub request_json: String,
    pub response_json: String,
    pub response: String,
    pub model: String,
    pub provider: LlmProvider,
    pub action: Action,
    pub llm_ok: bool,
}

#[derive(Debug, Clone)]
pub struct LlmClient {
    host: String,
    model: String,
    provider: LlmProvider,
    api_key: Option<String>,
    http: Client,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LlmProvider {
    Ollama,
    Openai,
}

impl LlmClient {
    pub fn new(
        host: impl Into<String>,
        model: impl Into<String>,
        provider: LlmProvider,
        api_key: Option<String>,
        timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        let host = host.into();
        let model = model.into();
        let http = Client::builder().timeout(timeout).build()?;

        Ok(Self {
            host,
            model,
            provider,
            api_key,
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
    let observations = observe_world(vm, agent_id);
    let last_feedback = memory
        .notes
        .last()
        .cloned()
        .unwrap_or_else(|| "none yet".into());
    let memory_notes = memory_context(memory, MEMORY_LIMIT);
    let prompt = build_prompt(
        &summary,
        &observations,
        &memory_notes,
        &last_feedback,
        DEFAULT_AGENT_GOAL,
        candidates,
        vm,
        agent_id,
    );

    let fallback_action = || choose_action(vm, agent_id, candidates, next_tick);

    let (request_json, response_json, response, mut action, llm_ok, model, provider) = match client
    {
        Some(client) => match call_chat(client, &prompt, candidates, agent_id, next_tick) {
            Ok(result) => {
                log_llm_call(
                    &result.provider,
                    &result.model,
                    &result.request_json,
                    &result.response_json,
                );
                (
                    result.request_json,
                    result.response_json,
                    result.reply_text,
                    result.action,
                    true,
                    client.model.clone(),
                    client.provider,
                )
            }
            Err(err) => (
                String::from("not sent (error building/sending request)"),
                String::from("not available"),
                format!("error: {}", err),
                fallback_action(),
                false,
                client.model.clone(),
                client.provider,
            ),
        },
        None => (
            String::from("not sent (no llm client)"),
            String::from("not available"),
            String::from("llm client missing; fallback to loop"),
            fallback_action(),
            false,
            String::from("unknown"),
            LlmProvider::Ollama,
        ),
    };

    // Safety override if low on Qi.
    action = survival_override(vm, agent_id, candidates, next_tick, action);

    push_memory(
        memory,
        format!(
            "tick {} | state: {} | obs: [{}] | decision: {} | llm: {}",
            vm.world().tick(),
            summary,
            observations.join(" ; "),
            action_token(&action),
            truncate(&response, 120)
        ),
    );

    LlmDecision {
        summary,
        observations,
        prompt,
        request_json,
        response_json,
        response,
        model,
        provider,
        action,
        llm_ok,
    }
}

fn choose_action(vm: &Vm, agent_id: AgentId, candidates: &[ActionArg], next_tick: u64) -> Action {
    let (qi, transistors) = vm
        .world()
        .agent(agent_id)
        .map(|a| (a.qi, a.transistors))
        .unwrap_or((0, 0));

    for action in candidates {
        let materialized = action.materialize(agent_id, next_tick);
        if matches!(
            materialized,
            Action::BuildStructure {
                kind: StructureKind::Programmable
            }
        ) && transistors == 0
        {
            continue;
        }
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
            "Agent #{} at ({}, {}, {}) qi={} transistors={} age={} last_tick={}",
            agent.id,
            agent.position.x,
            agent.position.y,
            agent.position.z,
            agent.qi,
            agent.transistors,
            agent.age,
            vm.world().tick()
        )
    } else {
        format!(
            "Agent {} missing; world tick={}",
            agent_id,
            vm.world().tick()
        )
    }
}

fn observe_world(vm: &Vm, agent_id: AgentId) -> Vec<String> {
    let mut notes = Vec::new();
    let Some(agent) = vm.world().agent(agent_id) else {
        return notes;
    };

    let pos = agent.position;
    notes.push("ore nodes unknown; scan to discover nearby deposits".into());

    let nearby_agents: Vec<_> = vm
        .world()
        .agents()
        .filter(|(id, a)| **id != agent_id && pos.within_range(a.position, SCAN_RANGE))
        .map(|(id, a)| {
            format!(
                "agent {} at ({},{},{}) qi={} transistors={}",
                id, a.position.x, a.position.y, a.position.z, a.qi, a.transistors
            )
        })
        .collect();

    if nearby_agents.is_empty() {
        notes.push("no other agents nearby".into());
    } else {
        notes.push(format!("nearby agents: {}", nearby_agents.join(" | ")));
    }

    notes
}

fn memory_context(memory: &BrainMemory, limit: usize) -> Vec<String> {
    let len = memory.notes.len();
    let start = len.saturating_sub(limit);
    memory.notes[start..].to_vec()
}

fn push_memory(memory: &mut BrainMemory, entry: String) {
    memory.notes.push(entry);
    if memory.notes.len() > MEMORY_LIMIT {
        let drop = memory.notes.len() - MEMORY_LIMIT;
        memory.notes.drain(0..drop);
    }
}

fn log_llm_call(provider: &LlmProvider, model: &str, request_json: &str, response_json: &str) {
    use std::fs::OpenOptions;
    let timestamp = Utc::now().to_rfc3339();
    let dir = PathBuf::from("logs");
    if let Err(err) = fs::create_dir_all(&dir) {
        eprintln!("warn: failed to create logs dir: {}", err);
        return;
    }
    let path = dir.join("llm.log");
    let content = format!(
        "[{}] provider={} model={}\nrequest:\n{}\nresponse:\n{}\n\n",
        timestamp,
        format!("{:?}", provider).to_lowercase(),
        model,
        request_json,
        response_json
    );
    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, content.as_bytes()));
    if let Err(err) = result {
        eprintln!("warn: failed to write llm log {}: {}", path.display(), err);
    }
}

fn build_prompt(
    summary: &str,
    observations: &[String],
    memory_notes: &[String],
    last_feedback: &str,
    goal: &str,
    candidates: &[ActionArg],
    _vm: &Vm,
    _agent_id: AgentId,
) -> String {
    let mut actions: Vec<String> = candidates.iter().map(|a| a.label()).collect();
    actions.sort();
    actions.dedup();
    let action_schema = vec![
        "move(x,y,z)",
        "scan(radius)",
        "build_<structure_kind>",
        "reproduce(partner_id)",
        "harvest_<ore_kind>(source_id)",
    ];
    let structure_kinds = vec!["basic", "programmable", "qi"];
    let ore_kinds = vec!["qi", "transistor"];

    let payload = json!({
        "goal": goal,
        "state": summary,
        "observations": observations,
        "memory": memory_notes,
        "last_feedback": last_feedback,
        "actions": actions,
        "action_schema": action_schema,
        "structure_kinds": structure_kinds,
        "ore_kinds": ore_kinds,
        "reply": { "action": "one_of(actions)" }
    });

    let toon = to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());

    format!(
        "You are an autonomous agent. Choose exactly one action from `actions`, fill in any needed parameters (move(x,y,z), scan(radius), build_<structure_kind>, reproduce(partner_id), harvest_<ore_kind>(source_id)), and reply ONLY in TOON with `action: <label>`. Input:\n{toon}"
    )
}

fn system_prompt() -> String {
    format!(
        "You are an autonomous agent inside a voxel-based, blockchain-synchronized world. Act to advance this goal: {}. Choose exactly one action from the provided list, include concrete parameters (e.g., move(x,y,z)), and respond ONLY in TOON with `action: <label>`.",
        DEFAULT_AGENT_GOAL
    )
}

fn build_chat_messages(user_prompt: &str) -> Vec<Message> {
    vec![
        Message {
            role: "system".into(),
            content: system_prompt(),
        },
        Message {
            role: "user".into(),
            content: user_prompt.into(),
        },
    ]
}

fn call_chat(
    client: &LlmClient,
    prompt: &str,
    candidates: &[ActionArg],
    agent_id: AgentId,
    next_tick: u64,
) -> Result<OllamaResult, String> {
    let mut attempts = 0;
    let max_attempts = 3;
    let mut last_err = String::new();

    while attempts < max_attempts {
        attempts += 1;
        let jitter_ms = if attempts == 1 {
            0
        } else {
            // simple jitter: 50-150ms
            50 + (rand::random::<u64>() % 100)
        };
        if jitter_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(jitter_ms));
        }

        let result = match client.provider {
            LlmProvider::Ollama => call_ollama(client, prompt, candidates, agent_id, next_tick),
            LlmProvider::Openai => call_openai(client, prompt, candidates, agent_id, next_tick),
        };

        match result {
            Ok(res) => return Ok(res),
            Err(e) => {
                last_err = e;
                if attempts >= max_attempts {
                    break;
                }
            }
        }
    }

    Err(format!(
        "llm failed after {} attempt(s): {}",
        attempts, last_err
    ))
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
        messages: build_chat_messages(prompt),
    };

    let request_json =
        serde_json::to_string_pretty(&body).map_err(|e| format!("encode request: {}", e))?;

    let resp = client
        .http
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| format!("http: {}", e))?;
    let status = resp.status();
    let raw_body = resp.text().map_err(|e| format!("read body: {}", e))?;

    let parsed: ChatResponse = serde_json::from_str(&raw_body)
        .map_err(|e| format!("decode: {}; status={} body={}", e, status, raw_body))?;

    let response_json = serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| raw_body.clone());

    let text = parsed.message.content;
    let parsed = parse_action(&text, candidates, agent_id, next_tick);
    let action = parsed
        .clone()
        .unwrap_or_else(|| choose_action_fallback(candidates, agent_id, next_tick));
    let reply_text = parsed
        .map(|a| format!("TOON{{action={}}}", action_token(&a)))
        .unwrap_or_else(|| truncate(&text, 120));

    Ok(OllamaResult {
        request_json,
        response_json,
        reply_text,
        action,
        model: client.model.clone(),
        provider: client.provider,
    })
}

fn call_openai(
    client: &LlmClient,
    prompt: &str,
    candidates: &[ActionArg],
    agent_id: AgentId,
    next_tick: u64,
) -> Result<OllamaResult, String> {
    let url = {
        let trimmed = client.host.trim_end_matches('/');
        if trimmed.ends_with("/v1/chat/completions") {
            trimmed.to_string()
        } else {
            format!("{}/v1/chat/completions", trimmed)
        }
    };

    let body = OpenAiChatRequest {
        model: client.model.clone(),
        stream: false,
        temperature: None,
        messages: build_chat_messages(prompt),
    };

    let request_json =
        serde_json::to_string_pretty(&body).map_err(|e| format!("encode request: {}", e))?;

    let resp = client
        .http
        .post(&url)
        .json(&body)
        .headers(build_openai_headers(&client.api_key)?)
        .send()
        .map_err(|e| format!("http: {}", e))?;
    let status = resp.status();
    let raw_body = resp.text().map_err(|e| format!("read body: {}", e))?;

    let parsed: OpenAiChatResponse = serde_json::from_str(&raw_body)
        .map_err(|e| format!("decode: {}; status={} body={}", e, status, raw_body))?;

    let response_json = serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| raw_body.clone());

    let text = parsed
        .choices
        .get(0)
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    let parsed = parse_action(&text, candidates, agent_id, next_tick);
    let action = parsed
        .clone()
        .unwrap_or_else(|| choose_action_fallback(candidates, agent_id, next_tick));
    let reply_text = parsed
        .map(|a| format!("TOON{{action={}}}", action_token(&a)))
        .unwrap_or_else(|| truncate(&text, 120));

    Ok(OllamaResult {
        request_json,
        response_json,
        reply_text,
        action,
        model: client.model.clone(),
        provider: client.provider,
    })
}

fn build_openai_headers(api_key: &Option<String>) -> Result<reqwest::header::HeaderMap, String> {
    use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(key) = api_key {
        let value = format!("Bearer {}", key);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&value).map_err(|e| e.to_string())?,
        );
    } else {
        return Err("missing LLM API key; set --llm-api-key or LLM_API_KEY".into());
    }
    Ok(headers)
}

fn parse_action(
    text: &str,
    candidates: &[ActionArg],
    _agent_id: AgentId,
    _next_tick: u64,
) -> Option<Action> {
    let allowed: HashSet<String> = candidates
        .iter()
        .map(|c| c.label().to_lowercase())
        .collect();

    let try_parse = |s: &str| parse_action_string(s, &allowed);

    // Try JSON schema: { "action": "<label>" }
    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(action_str) = json_value
            .get("action")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
        {
            if let Some(action) = try_parse(action_str) {
                return Some(action);
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

        if let Some(action) = try_parse(label) {
            return Some(action);
        }
    }

    // Fallback: `action:<label>` prefix
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("action:") {
            if let Some(action) = try_parse(rest.trim()) {
                return Some(action);
            }
        }
    }

    // Last resort: look for any candidate label substring
    let lower = text.to_lowercase();
    for verb in &allowed {
        if lower.contains(verb) {
            if let Some(action) = parse_action_string(verb, &allowed) {
                return Some(action);
            }
        }
    }

    None
}

fn normalize_verb(raw: &str) -> String {
    raw.trim().to_lowercase()
}

fn parse_action_string(raw: &str, allowed: &HashSet<String>) -> Option<Action> {
    let trimmed = raw.trim().trim_matches(|c| c == '{' || c == '}');
    let (verb_part, args_part) = if let Some(start) = trimmed.find('(') {
        let end = trimmed.rfind(')').unwrap_or(trimmed.len());
        (trimmed[..start].trim(), &trimmed[start + 1..end])
    } else {
        (trimmed, "")
    };

    let verb = normalize_verb(verb_part);
    let mut base = verb.as_str();
    let mut suffix: Option<&str> = None;
    if let Some(rest) = verb.strip_prefix("build_") {
        base = "build";
        suffix = Some(rest);
    } else if verb == "build_structure" {
        base = "build";
    } else if let Some(rest) = verb.strip_prefix("harvest_") {
        base = "harvest";
        suffix = Some(rest);
    } else if verb == "harvest_qi" {
        base = "harvest";
        suffix = Some("qi");
    }

    let base_label = base.to_string();
    let allowed_prefix = allowed.contains(&verb)
        || allowed.contains(&base_label)
        || allowed
            .iter()
            .any(|candidate| candidate.starts_with(&(base_label.clone() + "_")));
    if !allowed_prefix {
        return None;
    }

    let args: Vec<String> = args_part
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect();

    match base {
        "move" => {
            if args.len() < 3 {
                return None;
            }
            let dx = args.get(0)?.parse().ok()?;
            let dy = args.get(1)?.parse().ok()?;
            let dz = args.get(2)?.parse().ok()?;
            Some(Action::Move { dx, dy, dz })
        }
        "scan" => Some(Action::Scan),
        "build" => {
            let mut kind = suffix
                .and_then(|k| StructureKind::from_str(k).ok())
                .unwrap_or(StructureKind::Basic);
            if let Some(arg_kind) = args
                .get(0)
                .and_then(|k| StructureKind::from_str(k).ok())
            {
                kind = arg_kind;
            }
            Some(Action::BuildStructure { kind })
        }
        "reproduce" => {
            let partner = args.get(0).and_then(|p| p.parse().ok()).unwrap_or(0);
            Some(Action::Reproduce { partner })
        }
        "harvest" => {
            let mut ore = suffix
                .and_then(|o| <OreKind as FromStr>::from_str(o).ok())
                .unwrap_or(OreKind::Qi);
            let mut source_id = 0u64;
            if let Some(first) = args.get(0) {
                if let Ok(parsed_id) = first.parse() {
                    source_id = parsed_id;
                } else if let Ok(parsed_ore) = <OreKind as FromStr>::from_str(first) {
                    ore = parsed_ore;
                }
            }
            if let Some(second) = args.get(1) {
                if let Ok(parsed_id) = second.parse() {
                    source_id = parsed_id;
                }
            }
            Some(Action::HarvestOre { ore, source_id })
        }
        "idle" => Some(Action::Idle),
        _ => None,
    }
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
struct OpenAiChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChoice {
    message: ChatMessage,
}

struct OllamaResult {
    request_json: String,
    response_json: String,
    reply_text: String,
    action: Action,
    model: String,
    provider: LlmProvider,
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
        Action::HarvestOre { ore, source_id } => format!("harvest_{}({})", ore, source_id),
    }
}
