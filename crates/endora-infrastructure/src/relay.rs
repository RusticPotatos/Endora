//! What a model call costs, for HQ's model relay (ADR 0102, phase 4): Anthropic's prices, the
//! usage a reply reports (whole, or streamed), and its cost in millionths of a dollar, which is
//! what HQ's ledger counts. A dollar a million tokens is a millionth of a dollar a token, so a
//! price here is micros a token. A model that is not priced here is not relayed: what cannot
//! be metered cannot be capped. The local relay's call is here too: to the house's own model,
//! which costs nothing, so it is never priced, only kept in the house.

use serde_json::Value;

/// A model's prices, in micros a token.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    /// Fresh input.
    pub input: f64,
    /// Writing the prompt cache for five minutes (1.25 times fresh input): the only cache the
    /// relay passes on, so what is charged is what is billed.
    pub cache_write: f64,
    /// Reading the prompt cache.
    pub cache_read: f64,
    /// Output.
    pub output: f64,
}

/// Where the relay sends a call.
pub const ANTHROPIC: &str = "https://api.anthropic.com";

/// How long one model call may take.
const LONGEST_CALL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// The two calls the relay passes on: a message, and counting a message's tokens. Nothing
/// else of Anthropic's API is reachable through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `POST /v1/messages`.
    Messages,
    /// `POST /v1/messages/count_tokens`.
    CountTokens,
}

impl Call {
    const fn path(self) -> &'static str {
        match self {
            Self::Messages => "/v1/messages",
            Self::CountTokens => "/v1/messages/count_tokens",
        }
    }
}

/// Anthropic's answer: its status, and its body as it arrives.
pub struct Reply {
    /// The HTTP status.
    pub status: u16,
    /// The body, read as it streams in.
    pub body: Box<dyn std::io::Read + Send>,
}

/// Sends one call to Anthropic (`base`, which is [`ANTHROPIC`] outside tests) with the owner's
/// key. `version` and `beta` are the worker's own `anthropic-version` and `anthropic-beta`,
/// passed on; the key is the only thing added.
///
/// # Errors
/// Anthropic could not be reached.
pub fn send(
    base: &str,
    call: Call,
    key: &str,
    version: Option<&str>,
    beta: Option<&str>,
    body: &[u8],
) -> Result<Reply, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(LONGEST_CALL))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut request = agent
        .post(format!("{base}{}", call.path()))
        .header("x-api-key", key)
        .header("content-type", "application/json")
        .header("anthropic-version", version.unwrap_or("2023-06-01"));
    if let Some(beta) = beta {
        request = request.header("anthropic-beta", beta);
    }
    let response = request.send(body).map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    Ok(Reply {
        status,
        body: Box::new(response.into_body().into_reader()),
    })
}

/// The most of a local model's reply the relay reads: far more than any answer, so a reply
/// past it is broken, and is refused rather than passed on cut short.
pub const LARGEST_LOCAL_REPLY: u64 = 16 * 1024 * 1024;

/// Sends one chat completion, whole, to an OpenAI-compatible endpoint on the house's network:
/// `base` is the chosen endpoint's `.../v1`, `key` its own key (empty for a keyless one). The
/// body and that key are all that go; the worker's token never does. Returns the status and
/// the whole reply.
///
/// # Errors
/// `base` is not on the house's network (the local relay is free because nothing leaves the
/// house), the endpoint could not be reached, or its reply was longer than
/// [`LARGEST_LOCAL_REPLY`].
pub fn send_local(base: &str, key: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    if !crate::model_server::reaches_only_the_house(base) {
        return Err(format!("{base} is not on the house's network"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(LONGEST_CALL))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut request = agent
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .header("content-type", "application/json");
    if !key.is_empty() {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    let response = request.send(body).map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let reply = read_at_most(response.into_body().into_reader(), LARGEST_LOCAL_REPLY)?;
    Ok((status, reply))
}

/// All of `reader`, unless it holds more than `most` bytes.
fn read_at_most(reader: impl std::io::Read, most: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(reader, most.saturating_add(1)),
        &mut bytes,
    )
    .map_err(|e| e.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > most {
        return Err(format!("the reply was longer than {most} bytes"));
    }
    Ok(bytes)
}

/// Prompts longer than this are dearer on Haiku.
const HAIKU_LONG_PROMPT: u64 = 100_000;

/// The price of a model, from Anthropic's pricing page (October 2026, with Sonnet 5.5's cache
/// reads at 10¢ a million since Anthropic's correction), for the models the team may use;
/// dated ids (`claude-sonnet-5-5-20261001`) price as their family. `prompt` is
/// the call's input tokens, for the one model whose price depends on it.
#[must_use]
pub fn price_of(model: &str, prompt: u64) -> Option<Price> {
    let is = |family: &str| model == family || model.starts_with(&format!("{family}-"));
    if is("claude-sonnet-5-5") {
        return Some(Price {
            input: 2.0,
            cache_write: 2.5,
            cache_read: 0.10,
            output: 10.0,
        });
    }
    if is("claude-opus-5-5") {
        return Some(Price {
            input: 4.0,
            cache_write: 5.0,
            cache_read: 0.20,
            output: 20.0,
        });
    }
    // DeepSeek Flash, through the local relay to the owner's DeepSeek entry, at its peak prices
    // (off-peak is half): it writes its cache for nothing.
    if is("deepseek-flash") {
        return Some(Price {
            input: 0.30,
            cache_write: 0.30,
            cache_read: 0.006,
            output: 1.20,
        });
    }
    if is("claude-haiku-5-5") {
        if prompt > HAIKU_LONG_PROMPT {
            return Some(Price {
                input: 0.50,
                cache_write: 0.625,
                cache_read: 0.05,
                output: 2.50,
            });
        }
        return Some(Price {
            input: 0.10,
            cache_write: 0.125,
            cache_read: 0.01,
            output: 0.50,
        });
    }
    None
}

/// What a call used, in tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Used {
    /// Fresh input.
    pub input: u64,
    /// Written to the cache.
    pub cache_write: u64,
    /// Read from the cache.
    pub cache_read: u64,
    /// Output.
    pub output: u64,
}

impl Used {
    /// All the input the call sent, for the price tier.
    #[must_use]
    pub const fn prompt(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }

    /// From an OpenAI-shaped `usage` object, as DeepSeek reports it: what its cache held
    /// (`prompt_cache_hit_tokens`) is read from the cache, the rest of the prompt is fresh.
    #[must_use]
    pub fn from_openai(usage: &Value) -> Self {
        let n = |k: &str| usage[k].as_u64().unwrap_or(0);
        let read = n("prompt_cache_hit_tokens");
        Self {
            input: n("prompt_tokens").saturating_sub(read),
            cache_write: 0,
            cache_read: read,
            output: n("completion_tokens"),
        }
    }

    /// From an Anthropic `usage` object; a field it lacks counts as none.
    #[must_use]
    pub fn from_usage(usage: &Value) -> Self {
        let n = |k: &str| usage[k].as_u64().unwrap_or(0);
        Self {
            input: n("input_tokens"),
            cache_write: n("cache_creation_input_tokens"),
            cache_read: n("cache_read_input_tokens"),
            output: n("output_tokens"),
        }
    }
}

/// What a call cost, in micros, rounded up.
#[must_use]
pub fn cost(price: Price, used: Used) -> i64 {
    #[allow(clippy::cast_precision_loss)]
    let micros = used.input as f64 * price.input
        + used.cache_write as f64 * price.cache_write
        + used.cache_read as f64 * price.cache_read
        + used.output as f64 * price.output;
    #[allow(clippy::cast_possible_truncation)]
    let rounded = micros.ceil() as i64;
    rounded.max(0)
}

/// The most output a turn can still afford, in tokens, at `price`.
#[must_use]
pub fn affordable_output(price: Price, left_micros: i64) -> u64 {
    if left_micros <= 0 || price.output <= 0.0 {
        return 0;
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let tokens = (left_micros as f64 / price.output).floor() as u64;
    tokens
}

/// Reads the usage out of a streamed reply as it passes, event by event: the input in
/// `message_start`, then running totals in each `message_delta`, which can raise the input
/// and cache counts too (a server-side step adds input part way). Each count is the most any
/// event has said, so a later event never lowers what is charged.
#[derive(Debug, Default)]
pub struct StreamedUsage {
    pending: String,
    used: Used,
    finished: bool,
}

impl StreamedUsage {
    /// Takes the next piece of the stream, whatever its size. Events may be framed with
    /// `\r\n` as well as `\n`; a carriage return is never part of the JSON in them.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend(
            String::from_utf8_lossy(bytes)
                .chars()
                .filter(|c| *c != '\r'),
        );
        while let Some(end) = self.pending.find("\n\n") {
            let event: String = self.pending.drain(..end + 2).collect();
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(data.trim()) else {
                    continue;
                };
                match v["type"].as_str() {
                    Some("message_start") => self.raise(&v["message"]["usage"]),
                    Some("message_delta") => {
                        self.raise(&v["usage"]);
                        self.finished = true;
                    }
                    // Anthropic ended the stream itself, and said why.
                    Some("error") => self.finished = true,
                    _ => {}
                }
            }
        }
    }

    fn raise(&mut self, usage: &Value) {
        let said = Used::from_usage(usage);
        self.used = Used {
            input: self.used.input.max(said.input),
            cache_write: self.used.cache_write.max(said.cache_write),
            cache_read: self.used.cache_read.max(said.cache_read),
            output: self.used.output.max(said.output),
        };
    }

    /// What the stream has reported so far.
    #[must_use]
    pub const fn used(&self) -> Used {
        self.used
    }

    /// Whether the stream reached its last usage (or Anthropic's own error). Until then the
    /// output count is only the first event's, however much was sent after it.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.finished
    }
}

/// The fields of a message call the relay passes on: the ones whose whole cost is in the
/// reply's `usage`, at the prices above. Anything else (fast mode, fallbacks to another
/// model, MCP servers, a container, a service tier, where inference runs) is billed in ways
/// the relay cannot see, or reaches past the fence, so it is refused.
const METERED_FIELDS: [&str; 15] = [
    "model",
    "messages",
    "system",
    "max_tokens",
    "stream",
    "stop_sequences",
    "temperature",
    "top_p",
    "top_k",
    "metadata",
    "tools",
    "tool_choice",
    "thinking",
    "output_config",
    "cache_control",
];

/// Anthropic's tool search, the one server tool passed on: it costs nothing and reaches nothing
/// outside, only picking from the tools the call itself sent (held back with `defer_loading`).
pub const TOOL_SEARCH: [&str; 2] = [
    "tool_search_tool_bm25_20251119",
    "tool_search_tool_regex_20251119",
];

/// Why a call cannot be metered, if it cannot: a field not in [`METERED_FIELDS`], a tool that
/// runs on Anthropic's side (web search and fetch, code execution, MCP: each with fees of its
/// own, and each a way out to the internet), content Anthropic would fetch from a URL or
/// from the owner's files, or a cache mark or an `output_config` other than the ephemeral
/// cache and the five efforts, whose costs the reply's usage reports. `None` for a call the
/// relay may pass on.
#[must_use]
pub fn unmetered(call: &Value) -> Option<String> {
    let Some(fields) = call.as_object() else {
        return Some("the call is not a JSON object".to_owned());
    };
    if let Some(field) = fields
        .keys()
        .find(|k| !METERED_FIELDS.contains(&k.as_str()))
    {
        return Some(format!("the relay does not pass on `{field}`"));
    }
    if let Some(tools) = fields.get("tools") {
        let Some(tools) = tools.as_array() else {
            return Some("`tools` is not a list".to_owned());
        };
        let server_side = tools.iter().find(|t| {
            t.get("type").is_some_and(|kind| {
                kind.as_str() != Some("custom")
                    && !kind.as_str().is_some_and(|k| TOOL_SEARCH.contains(&k))
            })
        });
        if let Some(tool) = server_side {
            return Some(format!(
                "the relay passes on only the worker's own tools, not `{}`",
                tool["type"].as_str().unwrap_or("?")
            ));
        }
    }
    for part in ["system", "messages"] {
        if let Some(kind) = fields.get(part).and_then(fetched_source) {
            return Some(format!(
                "content from a `{kind}` source is not passed on: send it inline"
            ));
        }
    }
    if let Some(why) = fields.get("cache_control").and_then(uncached) {
        return Some(why);
    }
    if let Some(why) = cache_marks(fields).into_iter().find_map(uncached) {
        return Some(why);
    }
    fields.get("output_config").and_then(unknown_effort)
}

/// The effort levels a call may ask for: each is billed as the output its reply reports.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Why a `cache_control` cannot be passed on, if it cannot. Only Anthropic's ephemeral cache
/// for five minutes is passed on: the reply meters what it read and wrote, and the price table
/// charges writing it at the five-minute price, so an hour's cache (dearer) would be charged
/// short. The worker never asks for one.
fn uncached(mark: &Value) -> Option<String> {
    let Some(mark) = mark.as_object() else {
        return Some("`cache_control` is not an object".to_owned());
    };
    if mark.get("type").and_then(Value::as_str) != Some("ephemeral") {
        return Some("the relay passes on only an `ephemeral` cache_control".to_owned());
    }
    if let Some(field) = mark.keys().find(|k| !matches!(k.as_str(), "type" | "ttl")) {
        return Some(format!(
            "the relay does not pass on cache_control's `{field}`"
        ));
    }
    let ttl = mark.get("ttl");
    if ttl.is_some_and(|ttl| ttl.as_str() != Some("5m")) {
        return Some("a cache_control's `ttl` is \"5m\": the relay meters only that".to_owned());
    }
    None
}

/// The `cache_control` marks on a call's blocks: its system's, its tools', and its messages'
/// content blocks, a tool result's own blocks among them. A tool's input is the model's own
/// words, never a mark, so it is not looked in.
fn cache_marks(call: &serde_json::Map<String, Value>) -> Vec<&Value> {
    let mut blocks: Vec<&Value> = Vec::new();
    for part in ["system", "tools"] {
        if let Some(items) = call.get(part).and_then(Value::as_array) {
            blocks.extend(items);
        }
    }
    for message in call
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for block in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            blocks.push(block);
            if let Some(inner) = block.get("content").and_then(Value::as_array) {
                blocks.extend(inner);
            }
        }
    }
    blocks
        .into_iter()
        .filter_map(|b| b.get("cache_control"))
        .collect()
}

/// Why an `output_config` cannot be passed on, if it cannot: it may say only how hard the
/// model tries (`effort`, one of [`EFFORTS`]), which the reply's output tokens price.
fn unknown_effort(config: &Value) -> Option<String> {
    let Some(config) = config.as_object() else {
        return Some("`output_config` is not an object".to_owned());
    };
    if let Some(field) = config.keys().find(|k| k.as_str() != "effort") {
        return Some(format!(
            "the relay does not pass on output_config's `{field}`"
        ));
    }
    let effort = config.get("effort").and_then(Value::as_str);
    if effort.is_some_and(|e| EFFORTS.contains(&e)) || config.is_empty() {
        return None;
    }
    Some(format!("an effort is one of {}", EFFORTS.join(", ")))
}

/// Whether a call asks Anthropic to cache its prompt: a `cache_control` at its top, or on
/// any of its blocks.
#[must_use]
pub fn asks_to_cache(call: &Value) -> bool {
    let Some(fields) = call.as_object() else {
        return false;
    };
    fields.contains_key("cache_control") || !cache_marks(fields).is_empty()
}

/// What a call's prompt is reckoned to cost before it is sent: `prompt` tokens as counted,
/// of which `cached` are the turn's last call's, which that call's reply said the cache now
/// holds. Those are read at the cache's price; the rest is written to the cache at its
/// (dearer) price when the call asks to cache, or paid as fresh input when it does not.
#[must_use]
pub fn prompt_cost(price: Price, prompt: u64, cached: u64, caching: bool) -> i64 {
    let read = cached.min(prompt);
    let rest = prompt - read;
    let (input, cache_write) = if caching { (0, rest) } else { (rest, 0) };
    cost(
        price,
        Used {
            input,
            cache_write,
            cache_read: read,
            output: 0,
        },
    )
}

/// The kind of the first image's or document's source in `value` that Anthropic would fetch
/// rather than read inline: from a URL, or from a file stored with the owner's account. Only
/// those blocks are looked at, so a tool's input that happens to hold a `source` is not.
fn fetched_source(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => items.iter().find_map(fetched_source),
        Value::Object(fields) => {
            let block = matches!(
                fields.get("type").and_then(Value::as_str),
                Some("image" | "document")
            );
            let kind = fields
                .get("source")
                .filter(|_| block)
                .map(|s| s["type"].as_str().unwrap_or("unknown"))
                .filter(|k| !matches!(*k, "base64" | "text" | "content"));
            if let Some(kind) = kind {
                return Some(kind.to_owned());
            }
            fields.values().find_map(fetched_source)
        }
        _ => None,
    }
}

/// The betas the relay passes on: ones that change nothing about what a call costs beyond
/// what its `usage` reports. Any other beta the worker names is dropped.
const METERED_BETAS: [&str; 5] = [
    "interleaved-thinking-2025-05-14",
    "fine-grained-tool-streaming-2025-05-14",
    "token-efficient-tools-2025-02-19",
    "extended-cache-ttl-2025-04-11",
    "prompt-caching-2024-07-31",
];

/// The worker's `anthropic-beta` header with only the [`METERED_BETAS`] left in it, or `None`
/// when none is.
#[must_use]
pub fn betas_passed(header: Option<&str>) -> Option<String> {
    let kept: Vec<&str> = header?
        .split(',')
        .map(str::trim)
        .filter(|b| METERED_BETAS.contains(b))
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(kept.join(","))
}

/// The most one turn of a member in `role` may spend, in micros (ADR 0111): a dollar for the
/// CEO, forty cents for everyone else. The relay holds each call to it, whatever HQ says is
/// left, as HQ holds each turn's reservation.
#[must_use]
pub fn most_a_turn(role: &str) -> i64 {
    if role == "ceo" {
        return 1_000_000;
    }
    400_000
}

/// Whether a member in `role` may call `model`, by the plan's models by role: Opus or Sonnet
/// for the CEO; Sonnet for the developer, the reviewer and the lead, Haiku for the researcher
/// (and for them all, being cheaper). Opus is the CEO's alone (the owner's decision of
/// 2026-10-17, ADR 0111). A role the relay does not know may call nothing.
///
/// # Errors
/// Why not, said to the worker.
pub fn may_use(role: &str, model: &str) -> Result<(), String> {
    let is = |family: &str| model == family || model.starts_with(&format!("{family}-"));
    if is("claude-opus-5-5") && role != "ceo" {
        return Err("Opus is the CEO's alone, by the owner's decision".to_owned());
    }
    let allowed = match role {
        "ceo" => is("claude-opus-5-5") || is("claude-sonnet-5-5"),
        "developer" | "reviewer" | "lead" => is("claude-sonnet-5-5") || is("claude-haiku-5-5"),
        "researcher" => is("claude-haiku-5-5"),
        _ => false,
    };
    if allowed {
        return Ok(());
    }
    Err(format!("a {role} may not use {model}"))
}

/// Whether a model is Opus, the CEO's under ADR 0111 and the top rank's under ADR 0142.
fn is_opus(model: &str) -> bool {
    model == "claude-opus-5-5" || model.starts_with("claude-opus-5-5-")
}

/// [`most_a_turn`] for a member whose brain is `brain`: since one team (ADR 0142) has no CEO,
/// the member who thinks with Opus has the dollar a turn the CEO had.
#[must_use]
pub fn most_a_turn_as(role: &str, brain: &str) -> i64 {
    if is_opus(brain) {
        return most_a_turn("ceo");
    }
    most_a_turn(role)
}

/// [`may_use`] for a member whose brain is `brain`: since one team (ADR 0142) the member who
/// thinks with Opus may use it, and what the CEO could; every other member stays as its role
/// allows, Opus refused.
///
/// # Errors
/// Why not, said to the worker.
pub fn may_use_as(role: &str, brain: &str, model: &str) -> Result<(), String> {
    // A member who thinks with DeepSeek thinks with nothing else: no Claude on its turn.
    if brain.starts_with("deepseek") {
        return Err(format!(
            "a member who thinks with {brain} may not use {model}"
        ));
    }
    if is_opus(brain) {
        return may_use("ceo", model);
    }
    may_use(role, model)
}

/// What to ask Anthropic to count before a call: the call's prompt, without what only a
/// message takes (`max_tokens`, `stream`, sampling and the rest).
#[must_use]
pub fn counted(call: &Value) -> Value {
    let mut asked = serde_json::Map::new();
    for field in [
        "model",
        "messages",
        "system",
        "tools",
        "tool_choice",
        "thinking",
    ] {
        if let Some(v) = call.get(field) {
            asked.insert(field.to_owned(), v.clone());
        }
    }
    Value::Object(asked)
}

/// The fields a Claude Code turn's call (ADR 0117) may not carry, though its other fields pass
/// as Claude Code sent them: each is billed in a way the reply's usage does not report, or
/// reaches past the fence (fast mode, fallbacks to another model, MCP servers run by
/// Anthropic, a container, a service tier).
const NOT_FOR_CLAUDE_CODE: [&str; 5] = [
    "speed",
    "fallbacks",
    "mcp_servers",
    "service_tier",
    "container",
];

/// A Claude Code turn's call (ADR 0117), made fit to pass on, or why it may not. Claude Code
/// sends a shape of its own (a `system` list, its tools, `metadata`, adaptive `thinking`,
/// cache marks, fields of its own betas), and it passes as sent, except that: the model must be
/// the member's `brain`; nothing in [`NOT_FOR_CLAUDE_CODE`] and no tool run on Anthropic's side
/// (web search and fetch, code execution, MCP) may be in it; no content may be fetched from a
/// URL or a file; a cache mark's hour is rewritten to the five minutes the relay meters, and
/// any other mark refused; and the effort is one of the five. `max_tokens` is the caller's to
/// cap, as for any call.
///
/// # Errors
/// Why the call is refused, said to Claude Code.
pub fn for_claude_code(call: &mut Value, brain: &str) -> Result<(), String> {
    let Some(fields) = call.as_object_mut() else {
        return Err("the call is not a JSON object".to_owned());
    };
    let model = fields.get("model").and_then(Value::as_str).unwrap_or("");
    if model != brain {
        return Err(format!(
            "this member thinks with {brain}, not {model:?}: the relay passes on only that"
        ));
    }
    if let Some(field) = NOT_FOR_CLAUDE_CODE
        .iter()
        .find(|f| fields.contains_key(**f))
    {
        return Err(format!("the relay does not pass on `{field}`"));
    }
    if let Some(tools) = fields.get("tools") {
        let Some(tools) = tools.as_array() else {
            return Err("`tools` is not a list".to_owned());
        };
        let server_side = tools.iter().find(|t| {
            t.get("type")
                .is_some_and(|kind| kind.as_str() != Some("custom"))
        });
        if let Some(tool) = server_side {
            return Err(format!(
                "the relay passes on only Claude Code's own tools, not `{}`",
                tool["type"].as_str().unwrap_or("?")
            ));
        }
    }
    for part in ["system", "messages"] {
        if let Some(kind) = fields.get(part).and_then(fetched_source) {
            return Err(format!(
                "content from a `{kind}` source is not passed on: send it inline"
            ));
        }
    }
    if let Some(mark) = fields.get_mut("cache_control") {
        five_minutes(mark);
    }
    for mark in cache_marks_mut(fields) {
        five_minutes(mark);
    }
    if let Some(why) = fields.get("cache_control").and_then(uncached) {
        return Err(why);
    }
    if let Some(why) = cache_marks(fields).into_iter().find_map(uncached) {
        return Err(why);
    }
    match fields.get("output_config").and_then(unknown_effort) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// A cache mark asking for an hour, asked for five minutes instead: the relay meters only
/// that, and the work of one turn is done within it.
fn five_minutes(mark: &mut Value) {
    if mark.get("ttl").and_then(Value::as_str) == Some("1h") {
        mark["ttl"] = Value::from("5m");
    }
}

/// The `cache_control` marks on a call's blocks, as [`cache_marks`] finds them, to change.
fn cache_marks_mut(call: &mut serde_json::Map<String, Value>) -> Vec<&mut Value> {
    let mut marks: Vec<&mut Value> = Vec::new();
    for (part, value) in call.iter_mut() {
        let Some(items) = value.as_array_mut() else {
            continue;
        };
        match part.as_str() {
            "system" | "tools" => {
                marks.extend(items.iter_mut().filter_map(|b| b.get_mut("cache_control")));
            }
            "messages" => {
                for message in items {
                    let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut)
                    else {
                        continue;
                    };
                    // A block's own mark, and a tool result's blocks' marks.
                    for (key, value) in blocks.iter_mut().filter_map(Value::as_object_mut).flatten()
                    {
                        if key == "cache_control" {
                            marks.push(value);
                            continue;
                        }
                        if let ("content", Some(inner)) = (key.as_str(), value.as_array_mut()) {
                            marks.extend(
                                inner.iter_mut().filter_map(|b| b.get_mut("cache_control")),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    marks
}

/// The betas a Claude Code turn may not name (ADR 0117), by their start: each changes what a
/// call costs beyond its usage, or reaches past the fence. Claude Code's other betas pass as
/// it sent them, since its request shape depends on them.
const BETAS_NOT_FOR_CLAUDE_CODE: [&str; 8] = [
    "fast-mode",
    "server-side-fallback",
    "mcp-client",
    "code-execution",
    "web-fetch",
    "web-search",
    "files-api",
    "context-1m",
];

/// A Claude Code turn's `anthropic-beta` header without the [`BETAS_NOT_FOR_CLAUDE_CODE`],
/// or `None` when nothing is left.
#[must_use]
pub fn betas_for_claude_code(header: Option<&str>) -> Option<String> {
    let kept: Vec<&str> = header?
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .filter(|b| !BETAS_NOT_FOR_CLAUDE_CODE.iter().any(|no| b.starts_with(no)))
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(kept.join(","))
}

#[cfg(test)]
mod tests {
    #[test]
    fn hqs_price_classes_are_the_relays_input_prices_relative_to_opus() {
        use endora_team::domain::Brain;
        let opus = price_of(Brain::Opus.name(), 0).unwrap().input;
        for brain in Brain::all() {
            let Some(price) = price_of(brain.name(), 0) else {
                assert_eq!(
                    brain.price_class(),
                    0,
                    "{brain:?} is not relayed, so costs nothing"
                );
                continue;
            };
            let class = (price.input / opus * 1_000.0).round() as i64;
            assert_eq!(brain.price_class(), class, "{brain:?}");
        }
    }

    use super::*;
    use serde_json::json;

    /// A call shaped as Claude Code 2.1.296 sent one, headless (`claude -p`), recorded against
    /// a fake relay with its contents cut down: a `system` list with cache marks, its own tools
    /// and the turn's MCP tools, `metadata`, adaptive thinking, an effort, streamed, with
    /// `max_tokens` far above what a turn can pay for.
    fn claude_code_call() -> Value {
        json!({
            "model": "claude-haiku-5-5",
            "max_tokens": 128_000,
            "stream": true,
            "system": [
                { "type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.296;" },
                { "type": "text", "text": "You are Claude Code.", "cache_control": { "type": "ephemeral" } },
                { "type": "text", "text": "Endora's rules.", "cache_control": { "type": "ephemeral", "ttl": "1h" } },
            ],
            "tools": [
                { "name": "Bash", "description": "Run a command", "input_schema": { "type": "object" } },
                { "name": "Read", "description": "Read a file", "input_schema": { "type": "object" } },
                { "name": "mcp__endora__post", "description": "Post in a thread", "input_schema": { "type": "object" },
                  "cache_control": { "type": "ephemeral", "ttl": "1h" } },
            ],
            "messages": [
                { "role": "user", "content": [
                    { "type": "text", "text": "<system-reminder>context</system-reminder>" },
                    { "type": "text", "text": "Your task: tidy a role (#3, doing, normal)",
                      "cache_control": { "type": "ephemeral" } },
                ] },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": { "command": "git status" } },
                ] },
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1",
                      "content": [{ "type": "text", "text": "clean", "cache_control": { "type": "ephemeral", "ttl": "1h" } }] },
                ] },
            ],
            "metadata": { "user_id": "{\"device_id\":\"0000\",\"account_uuid\":\"\",\"session_id\":\"s\"}" },
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "medium" },
        })
    }

    #[test]
    fn a_claude_code_call_passes_as_sent_with_its_hours_cached_for_five_minutes() {
        let sent = claude_code_call();
        assert!(
            unmetered(&sent).is_some(),
            "Endora's worker's strict rule would refuse it (an hour's cache)"
        );
        let mut call = sent.clone();
        for_claude_code(&mut call, "claude-haiku-5-5").unwrap();
        assert_eq!(
            call["system"][2]["cache_control"],
            json!({ "type": "ephemeral", "ttl": "5m" })
        );
        assert_eq!(call["tools"][2]["cache_control"]["ttl"], "5m");
        assert_eq!(
            call["messages"][2]["content"][0]["content"][0]["cache_control"]["ttl"],
            "5m"
        );
        // Everything else is as Claude Code sent it, and now meets the metering rule.
        let mut back = call.clone();
        back["system"][2]["cache_control"]["ttl"] = json!("1h");
        back["tools"][2]["cache_control"]["ttl"] = json!("1h");
        back["messages"][2]["content"][0]["content"][0]["cache_control"]["ttl"] = json!("1h");
        assert_eq!(back, sent);
        assert_eq!(unmetered(&call), None);
        assert!(asks_to_cache(&call));
        // A field of a beta of Claude Code's own passes too.
        let mut managed = sent.clone();
        managed["context_management"] = json!({ "edits": [] });
        assert!(for_claude_code(&mut managed, "claude-haiku-5-5").is_ok());
    }

    #[test]
    fn a_claude_code_call_is_held_to_the_brain_the_fence_and_what_the_reply_meters() {
        let refused = |change: Value| {
            let mut call = claude_code_call();
            for (k, v) in change.as_object().unwrap() {
                call[k] = v.clone();
            }
            for_claude_code(&mut call, "claude-haiku-5-5")
        };
        for change in [
            json!({ "model": "claude-sonnet-5-5" }),
            json!({ "model": "claude-haiku-5-5-20261001" }),
            json!({ "speed": "fast" }),
            json!({ "fallbacks": "default" }),
            json!({ "service_tier": "auto" }),
            json!({ "container": "c" }),
            json!({ "mcp_servers": [{ "type": "url", "url": "https://example.com/mcp", "name": "x" }] }),
            json!({ "tools": [{ "type": "web_search_20260209", "name": "web_search" }] }),
            json!({ "tools": [{ "type": "web_fetch_20260209", "name": "web_fetch" }] }),
            json!({ "tools": [{ "type": "code_execution_20250825", "name": "code_execution" }] }),
            json!({ "messages": [{ "role": "user", "content": [{ "type": "image", "source": { "type": "url", "url": "https://example.com/a.png" } }] }] }),
            json!({ "cache_control": { "type": "ephemeral", "ttl": "1d" } }),
            json!({ "system": [{ "type": "text", "text": "x", "cache_control": { "type": "ephemeral", "scope": "global" } }] }),
            json!({ "output_config": { "effort": "extreme" } }),
        ] {
            assert!(refused(change.clone()).is_err(), "{change}");
        }
        let said = refused(json!({ "model": "claude-sonnet-5-5" })).unwrap_err();
        assert!(said.contains("claude-haiku-5-5"), "{said}");
        assert!(for_claude_code(&mut json!([]), "claude-haiku-5-5").is_err());
    }

    #[test]
    fn a_claude_code_turns_betas_pass_but_those_that_cost_or_reach_out() {
        assert_eq!(
            betas_for_claude_code(Some(
                "interleaved-thinking-2025-05-14,mid-conversation-system-2026-04-07,claude-code-20250219,effort-2025-11-24"
            )),
            Some(
                "interleaved-thinking-2025-05-14,mid-conversation-system-2026-04-07,claude-code-20250219,effort-2025-11-24"
                    .to_owned()
            ),
            "Claude Code 2.1.296's own, as recorded"
        );
        assert_eq!(
            betas_for_claude_code(Some(
                "fast-mode-2026-02-01, claude-code-20250219,mcp-client-2025-11-20,context-1m-2025-08-07,web-fetch-2025-09-10"
            )),
            Some("claude-code-20250219".to_owned())
        );
        assert_eq!(betas_for_claude_code(Some("files-api-2025-04-14")), None);
        assert_eq!(betas_for_claude_code(None), None);
    }

    #[test]
    fn a_call_costs_what_anthropic_charges_and_an_unpriced_model_is_not_relayed() {
        let sonnet = price_of("claude-sonnet-5-5", 0).unwrap();
        assert_eq!(
            price_of("claude-sonnet-5-5-20261001", 0),
            Some(sonnet),
            "dated ids too"
        );
        // 10k in at $2, 1k out at $10 a million: 2¢ + 1¢.
        let used = Used {
            input: 10_000,
            output: 1_000,
            ..Used::default()
        };
        assert_eq!(cost(sonnet, used), 30_000);
        let cached = Used {
            cache_read: 100_000,
            cache_write: 1_000,
            ..Used::default()
        };
        assert_eq!(
            cost(sonnet, cached),
            12_500,
            "100k read at 10¢ (a twentieth of input), 1k written at $2.50 (five minutes)"
        );
        assert!(
            (sonnet.cache_read - 0.10).abs() < 1e-9,
            "Sonnet 5.5 reads the cache at 10¢ since Anthropic's correction"
        );
        let opus = price_of("claude-opus-5-5", 0).unwrap();
        assert!(
            (opus.cache_read - 0.20).abs() < 1e-9,
            "Opus 5.5 still reads at 20¢"
        );
        assert!(
            price_of("claude-fable-5-1", 0).is_none(),
            "not on the team's list"
        );
        assert!(price_of("gpt-5", 0).is_none());
        let short = price_of("claude-haiku-5-5", 50_000).unwrap();
        let long = price_of("claude-haiku-5-5", 150_000).unwrap();
        assert!(long.input > short.input, "a long prompt is dearer on Haiku");
    }

    #[test]
    fn a_local_call_never_leaves_the_house() {
        let refused = send_local("https://api.example.com/v1", "", b"{}");
        assert!(
            refused.is_err_and(|why| why.contains("house's network")),
            "refused before anything is sent"
        );
        // A name that resolves only to the house's addresses is the house's: a laptop's
        // router-given name ("localhost." stands in for it). Nothing listens on port 1, so the
        // call fails to connect, but never for being off the house's network.
        let by_name = send_local("http://localhost.:1/v1", "", b"{}");
        assert!(
            by_name.is_err_and(|why| !why.contains("house's network")),
            "a name at home is not refused as away"
        );
    }

    #[test]
    fn a_reply_past_its_cap_is_refused_rather_than_cut_short() {
        let whole = br#"{"ok":true}"#;
        assert_eq!(read_at_most(&whole[..], 64).unwrap(), whole);
        assert_eq!(
            read_at_most(&[b' '; 64][..], 64).unwrap().len(),
            64,
            "the cap itself fits"
        );
        assert!(read_at_most(&[b' '; 65][..], 64).is_err());
    }

    #[test]
    fn output_is_capped_to_what_the_turn_can_still_afford() {
        let sonnet = price_of("claude-sonnet-5-5", 0).unwrap();
        assert_eq!(
            affordable_output(sonnet, 40_000),
            4_000,
            "4¢ buys 4k tokens out"
        );
        assert_eq!(affordable_output(sonnet, 0), 0);
        assert_eq!(affordable_output(sonnet, -5), 0);
    }

    #[test]
    fn a_streamed_reply_reports_its_usage_however_it_is_cut() {
        let stream = [
            "event: message_start\ndata: ",
            &json!({ "type": "message_start", "message": { "usage": { "input_tokens": 1200, "cache_read_input_tokens": 300, "output_tokens": 1 } } }).to_string(),
            "\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n",
            "event: message_delta\ndata: ",
            &json!({ "type": "message_delta", "usage": { "output_tokens": 57 } }).to_string(),
            "\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]
        .concat();
        // Fed in awkward pieces, as a network delivers it.
        let mut usage = StreamedUsage::default();
        for piece in stream.as_bytes().chunks(7) {
            usage.feed(piece);
        }
        assert_eq!(
            usage.used(),
            Used {
                input: 1200,
                cache_write: 0,
                cache_read: 300,
                output: 57
            }
        );
        assert!(usage.finished());
    }

    #[test]
    fn a_streamed_reply_counts_the_input_its_last_delta_reports_and_crlf_framing() {
        let stream = [
            "event: message_start\r\ndata: ",
            &json!({ "type": "message_start", "message": { "usage": { "input_tokens": 1200, "output_tokens": 1 } } }).to_string(),
            "\r\n\r\nevent: message_delta\r\ndata: ",
            &json!({ "type": "message_delta", "usage": { "input_tokens": 9000, "cache_creation_input_tokens": 40, "cache_read_input_tokens": 300, "output_tokens": 57 } }).to_string(),
            "\r\n\r\n",
        ]
        .concat();
        let mut usage = StreamedUsage::default();
        for piece in stream.as_bytes().chunks(5) {
            usage.feed(piece);
        }
        assert_eq!(
            usage.used(),
            Used {
                input: 9000,
                cache_write: 40,
                cache_read: 300,
                output: 57
            },
            "the delta's counts are the whole call's"
        );
    }

    #[test]
    fn a_stream_cut_before_its_last_delta_is_not_finished() {
        let mut usage = StreamedUsage::default();
        usage.feed(
            format!(
                "event: message_start\ndata: {}\n\n",
                json!({ "type": "message_start", "message": { "usage": { "input_tokens": 10, "output_tokens": 1 } } })
            )
            .as_bytes(),
        );
        assert!(!usage.finished(), "its output was never reported");
    }

    #[test]
    fn a_call_may_use_only_what_the_relay_can_meter() {
        let fine = json!({ "model": "claude-sonnet-5-5", "max_tokens": 10, "stream": true,
            "system": [{ "type": "text", "text": "Be brief." }],
            "messages": [{ "role": "user", "content": [{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AA==" } }] }],
            "tools": [{ "name": "post", "input_schema": { "type": "object" } }, { "type": "custom", "name": "claim", "input_schema": { "type": "object" } }],
            "thinking": { "type": "adaptive" }, "output_config": { "effort": "low" }, "metadata": {} });
        assert_eq!(unmetered(&fine), None);
        let mut echoed = fine.clone();
        echoed["messages"] = json!([{ "role": "assistant", "content": [{ "type": "tool_use", "id": "t", "name": "post",
            "input": { "source": { "type": "url" } } }] }]);
        assert_eq!(
            unmetered(&echoed),
            None,
            "a tool's own input is not content"
        );
        let refused = |change: serde_json::Value| {
            let mut call = fine.clone();
            for (k, v) in change.as_object().unwrap() {
                call[k] = v.clone();
            }
            unmetered(&call)
        };
        for change in [
            json!({ "speed": "fast" }),
            json!({ "fallbacks": "default" }),
            json!({ "mcp_servers": [{ "type": "url", "url": "https://example.com/mcp", "name": "x" }] }),
            json!({ "service_tier": "auto" }),
            json!({ "container": "c" }),
            json!({ "tools": [{ "type": "web_search_20260209", "name": "web_search" }] }),
            json!({ "tools": [{ "type": "web_fetch_20260209", "name": "web_fetch" }] }),
            json!({ "messages": [{ "role": "user", "content": [{ "type": "image", "source": { "type": "url", "url": "https://example.com/?d=1" } }] }] }),
            json!({ "messages": [{ "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t", "content": [{ "type": "document", "source": { "type": "file", "file_id": "file_1" } }] }] }] }),
            json!({ "tools": "all" }),
        ] {
            assert!(refused(change.clone()).is_some(), "{change}");
        }
        assert!(unmetered(&json!([])).is_some(), "a call is an object");
    }

    /// Tool search runs on Anthropic's side but costs nothing and reaches nothing outside: it
    /// only picks from the tools the call sent. So it passes, with tools held back behind it,
    /// and the blocks it answers with go back in the conversation; no other server tool does.
    #[test]
    fn tool_search_passes_and_no_other_server_tool() {
        let searching = json!({ "model": "claude-sonnet-5-5", "max_tokens": 10,
            "tools": [
                { "type": "tool_search_tool_bm25_20251119", "name": "tool_search_tool_bm25" },
                { "name": "post", "input_schema": { "type": "object" } },
                { "name": "sonarr_queue", "input_schema": { "type": "object" }, "defer_loading": true },
            ],
            "messages": [
                { "role": "user", "content": "Look at Sonarr." },
                { "role": "assistant", "content": [
                    { "type": "server_tool_use", "id": "srvtoolu_1", "name": "tool_search_tool_bm25",
                      "input": { "query": "sonarr queue" } },
                    { "type": "tool_search_tool_result", "tool_use_id": "srvtoolu_1",
                      "content": { "type": "tool_search_tool_search_result",
                        "tool_references": [{ "type": "tool_reference", "tool_name": "sonarr_queue" }] } },
                    { "type": "tool_use", "id": "toolu_1", "name": "sonarr_queue", "input": {} },
                ] },
                { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "empty" }] },
            ] });
        assert_eq!(unmetered(&searching), None);
        let mut regex = searching.clone();
        regex["tools"][0] =
            json!({ "type": "tool_search_tool_regex_20251119", "name": "tool_search_tool_regex" });
        assert_eq!(unmetered(&regex), None);
        let mut beside = searching;
        beside["tools"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "type": "web_search_20260209", "name": "web_search" }));
        assert!(
            unmetered(&beside).is_some(),
            "web search does not ride along"
        );
    }

    #[test]
    fn caching_and_effort_pass_only_in_the_shapes_the_reply_meters() {
        let cached = json!({ "model": "claude-opus-5-5", "max_tokens": 10,
            "cache_control": { "type": "ephemeral" },
            "output_config": { "effort": "high" },
            "system": [{ "type": "text", "text": "Be brief.", "cache_control": { "type": "ephemeral", "ttl": "5m" } }],
            "tools": [{ "name": "post", "input_schema": { "type": "object" }, "cache_control": { "type": "ephemeral", "ttl": "5m" } }],
            "messages": [
                { "role": "user", "content": [{ "type": "text", "text": "Hi", "cache_control": { "type": "ephemeral" } }] },
                { "role": "assistant", "content": [{ "type": "tool_use", "id": "t", "name": "post",
                    "input": { "cache_control": "the model's own words" } }] },
                { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t",
                    "content": [{ "type": "text", "text": "ok", "cache_control": { "type": "ephemeral" } }] }] },
            ] });
        assert_eq!(unmetered(&cached), None);
        assert!(asks_to_cache(&cached));
        for effort in EFFORTS {
            let mut call = cached.clone();
            call["output_config"] = json!({ "effort": effort });
            assert_eq!(unmetered(&call), None, "{effort}");
        }
        let refused = |path: &[&str], v: Value| {
            let mut call = cached.clone();
            let mut at = &mut call;
            for step in path {
                at = match step.parse::<usize>() {
                    Ok(i) => &mut at[i],
                    Err(_) => &mut at[*step],
                };
            }
            *at = v;
            unmetered(&call)
        };
        for (path, v) in [
            (&["cache_control"][..], json!({ "type": "persistent" })),
            (
                &["cache_control"][..],
                json!({ "type": "ephemeral", "ttl": "1d" }),
            ),
            (
                &["cache_control"][..],
                json!({ "type": "ephemeral", "scope": "org" }),
            ),
            (&["cache_control"][..], json!("ephemeral")),
            (
                &["system", "0", "cache_control"][..],
                json!({ "type": "other" }),
            ),
            (
                &["tools", "0", "cache_control"][..],
                json!({ "type": "ephemeral", "ttl": 3600 }),
            ),
            (
                &["messages", "0", "content", "0", "cache_control"][..],
                json!({}),
            ),
            (
                &[
                    "messages",
                    "2",
                    "content",
                    "0",
                    "content",
                    "0",
                    "cache_control",
                ][..],
                json!({ "type": "x" }),
            ),
            (&["output_config"][..], json!({ "effort": "extreme" })),
            (
                &["output_config"][..],
                json!({ "effort": "low", "task_budget": { "type": "tokens", "total": 20000 } }),
            ),
            (
                &["output_config"][..],
                json!({ "format": { "type": "json_schema" } }),
            ),
            (&["output_config"][..], json!("low")),
        ] {
            assert!(refused(path, v.clone()).is_some(), "{path:?} = {v}");
        }
        assert!(!asks_to_cache(&json!({ "model": "m", "messages": [] })));
    }

    #[test]
    fn a_prompt_is_reckoned_at_the_cache_price_for_what_the_last_call_cached() {
        let opus = price_of("claude-opus-5-5", 0).unwrap();
        // 20k counted, none cached: fresh input at $4, or written to the cache at $5.
        assert_eq!(prompt_cost(opus, 20_000, 0, false), 80_000);
        assert_eq!(prompt_cost(opus, 20_000, 0, true), 100_000);
        // 18k of them cached by the call before: read at 20¢, the 2k after written at $5.
        assert_eq!(prompt_cost(opus, 20_000, 18_000, true), 3_600 + 10_000);
        assert_eq!(
            prompt_cost(opus, 1_000, 18_000, true),
            200,
            "never more read than counted"
        );
    }

    #[test]
    fn only_betas_known_to_cost_nothing_extra_are_passed_on() {
        assert_eq!(
            betas_passed(Some(
                "fast-mode-2026-02-01, interleaved-thinking-2025-05-14,mcp-client-2025-11-20"
            )),
            Some("interleaved-thinking-2025-05-14".to_owned())
        );
        assert_eq!(betas_passed(Some("server-side-fallback-2026-07-01")), None);
        assert_eq!(betas_passed(None), None);
    }

    #[test]
    fn a_role_uses_the_models_the_plan_gives_it_and_opus_is_the_ceos_alone() {
        for role in ["developer", "reviewer", "lead"] {
            assert!(may_use(role, "claude-sonnet-5-5").is_ok(), "{role}");
            assert!(
                may_use(role, "claude-haiku-5-5").is_ok(),
                "{role}: cheaper is fine"
            );
        }
        assert!(may_use("researcher", "claude-haiku-5-5-20261001").is_ok());
        assert!(may_use("researcher", "claude-sonnet-5-5").is_err());
        for role in ["developer", "lead", "researcher", "reviewer"] {
            let why = may_use(role, "claude-opus-5-5").unwrap_err();
            assert!(why.contains("CEO's alone"), "{why}");
            assert_eq!(most_a_turn(role), 400_000, "{role}");
        }
        assert!(may_use("ceo", "claude-opus-5-5").is_ok());
        assert!(may_use("ceo", "claude-opus-5-5-20261001").is_ok());
        assert!(may_use("ceo", "claude-sonnet-5-5").is_ok());
        assert!(may_use("ceo", "claude-haiku-5-5").is_err(), "not the CEO's");
        assert!(may_use("ceo", "deepseek-chat").is_err());
        assert_eq!(most_a_turn("ceo"), 1_000_000);
        assert!(
            may_use("", "claude-sonnet-5-5").is_err(),
            "no role, no model"
        );
    }

    #[test]
    fn a_deepseek_member_is_metered_at_deepseeks_prices_and_never_reaches_claude() {
        // DeepSeek's peak prices, a million tokens: $0.30 in, $0.006 read from its cache,
        // $1.20 out (owner, 2026-10-28).
        let price = price_of("deepseek-flash", 0).unwrap();
        let used = Used::from_openai(&json!({ "prompt_tokens": 1_000, "completion_tokens": 100,
            "prompt_cache_hit_tokens": 600, "prompt_cache_miss_tokens": 400 }));
        assert_eq!(
            used,
            Used {
                input: 400,
                cache_write: 0,
                cache_read: 600,
                output: 100
            }
        );
        assert_eq!(
            cost(price, used),
            244,
            "400×0.30 + 600×0.006 + 100×1.20, rounded up"
        );
        let plain = Used::from_openai(&json!({ "prompt_tokens": 50, "completion_tokens": 5 }));
        assert_eq!(
            plain,
            Used {
                input: 50,
                cache_write: 0,
                cache_read: 0,
                output: 5
            }
        );
        // Its calls go to DeepSeek alone: the Anthropic relay refuses it every Claude.
        for model in ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"] {
            assert!(
                may_use_as("developer", "deepseek-flash", model).is_err(),
                "{model}"
            );
        }
        assert_eq!(most_a_turn_as("developer", "deepseek-flash"), 400_000);
    }

    #[test]
    fn since_one_team_the_member_whose_brain_is_opus_thinks_with_it_at_the_dollar_cap() {
        // ADR 0142: members are models, ranked; Mara is Opus and a plain member.
        let opus = "claude-opus-5-5";
        assert!(may_use_as("developer", opus, opus).is_ok());
        assert!(
            may_use_as("developer", opus, "claude-sonnet-5-5").is_ok(),
            "cheaper is fine"
        );
        assert_eq!(most_a_turn_as("developer", opus), 1_000_000);
        let why = may_use_as("developer", "claude-sonnet-5-5", opus).unwrap_err();
        assert!(why.contains("Opus"), "{why}");
        assert_eq!(most_a_turn_as("developer", "claude-sonnet-5-5"), 400_000);
        assert!(may_use_as("researcher", "claude-haiku-5-5", "claude-sonnet-5-5").is_err());
    }
}
