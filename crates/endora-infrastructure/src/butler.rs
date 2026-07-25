//! Butler brains behind the [`Butler`] port (see
//! `docs/adr/0014-the-butler-conversation-values-attention.md`).
//!
//! Two implementations:
//! - [`ScriptedButler`] — deterministic and offline. With no model behind it there is
//!   nothing to understand *with*, so it keeps the conversation open, forms no beliefs
//!   and takes no action, rather than performing insight it does not have. The baseline
//!   the model layer scores candidates against.
//! - [`LlmButler`] — model-backed (a local OpenAI-compatible endpoint). It drives the
//!   single tool-calling turn (ADR 0028), answering from real tool results. If the model
//!   is unavailable or returns something unusable it answers with a plain, honest
//!   *degraded* reply (see [`degraded_reply`]) so the conversation never breaks —
//!   crucially forming **no beliefs**, so a transient model failure never writes to
//!   understanding.
//!
//! Neither is the enforcement boundary. A tool call is a *proposal*: deterministic
//! policy decides whether it runs (ADRs 0005/0024), the capability executes it, and the
//! result — success or failure — comes back for the model to answer from.

use endora_application::{BeliefKind, ChatMessage, Confidence, MessageRole, Preference};
use endora_application::{Butler, ButlerContext, ButlerReply, FormedBelief, ProposalError};
use std::sync::{Arc, Mutex};

use endora_capabilities::{ButlerModelConfig, ButlerModelConfigRepository, ModelSlot, Sampling};
use serde_json::{Value, json};

/// A deterministic, offline butler. Reliable, if simple.
pub struct ScriptedButler;

impl Butler for ScriptedButler {
    fn respond(
        &self,
        history: &[ChatMessage],
        _preferences: &[Preference],
        _context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        Ok(scripted_reply(history))
    }
}

/// Acknowledges the latest user message and invites them to go on.
///
/// Deliberately minimal: with no model behind it there is nothing to understand
/// with, so it forms **no beliefs** and takes no action. It keeps the conversation
/// open rather than performing insight it does not have.
fn scripted_reply(history: &[ChatMessage]) -> ButlerReply {
    let last_user = history
        .iter()
        .rev()
        .find(|m| m.role() == MessageRole::User)
        .map(|m| m.text().trim().to_owned())
        .unwrap_or_default();
    let text = if last_user.is_empty() {
        "I'm here — what's on your mind?".to_owned()
    } else {
        "I'm listening, though I'm running without my language model at the moment, so I \
         can't think about this properly yet. Tell me more and I'll pick it up when I'm \
         back."
            .to_owned()
    };
    ButlerReply {
        text,
        ..ButlerReply::default()
    }
}

/// The reply an [`LlmButler`] gives when its model can't be reached or returns
/// something unusable. Honest about the transient failure and — deliberately —
/// carries **no beliefs**: a degraded turn must never mutate state. It answers the
/// conversation without pretending to have understood.
fn degraded_reply() -> ButlerReply {
    ButlerReply {
        text: "Sorry — I couldn't reach my language model just now, so I didn't follow that \
               properly. Give me a moment and try again."
            .to_owned(),
        ..ButlerReply::default()
    }
}

/// A [`Butler`] backed by a local OpenAI-compatible chat endpoint. When the model
/// can't be reached or returns something unusable it answers with a side-effect-free
/// [`degraded_reply`] (no proposals), so the conversation is always answered without
/// a failed turn ever mutating state.
pub struct LlmButler {
    agent: ureq::Agent,
    base_url: String,
    model: String,
    /// Bearer token for the endpoint (empty for keyless/local runtimes).
    api_key: String,
    /// Sampling parameters for this model's calls.
    sampling: Sampling,
}

/// How long to wait for the whole model round-trip before giving up and using
/// the scripted fallback. Bounds the chat: a slow or stuck model can never hang
/// the conversation "forever" — the person always gets a reply. Generous enough
/// for a healthy local model (GPU replies land in a few seconds); it only trips
/// when something is wrong (e.g. inference stuck on CPU).
const BUTLER_MODEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

impl LlmButler {
    /// Creates a model-backed butler for a local, keyless endpoint and model,
    /// with the historical default sampling (temperature 0.5).
    #[must_use]
    pub fn new(base_url: String, model: String) -> Self {
        Self::with_config(base_url, model, String::new(), Sampling::default())
    }

    /// Creates a model-backed butler with an explicit API key and sampling
    /// parameters (the runtime-configurable path — ADR 0027).
    #[must_use]
    pub fn with_config(
        base_url: String,
        model: String,
        api_key: String,
        sampling: Sampling,
    ) -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(BUTLER_MODEL_TIMEOUT))
                .build()
                .into(),
            base_url,
            model,
            api_key,
            sampling,
        }
    }

    /// Asks the model and parses its reply, or an error the caller can fall back
    /// on.
    fn try_model(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        // Offer native tool-calling on this (tool-selection) pass.
        let body = build_butler_request(
            &self.model,
            &self.sampling,
            history,
            preferences,
            context,
            true,
        );
        self.post_and_parse(&body, context)
    }

    /// Keeps a **local** Ollama model resident between turns, so it doesn't cold-load
    /// on the next call (a reload can blow past the timeout and degrade the reply).
    /// `keep_alive` is an Ollama extension; only sent to a local endpoint, since a
    /// strict cloud endpoint rejects unknown fields.
    fn with_keep_alive(&self, mut body: Value) -> Value {
        let b = self.base_url.as_str();
        let local = b.contains(":11434")
            || b.contains("host.docker.internal")
            || b.contains("localhost")
            || b.contains("127.0.0.1");
        if local {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("keep_alive".to_owned(), json!("30m"));
            }
        }
        body
    }

    /// POSTs a chat-completions `body` and parses the reply (native tool call or
    /// envelope). Shared by the two-pass `try_model` and the single-conversation
    /// `take_turn`.
    fn post_and_parse(
        &self,
        body: &Value,
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self.agent.post(&url);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", &format!("Bearer {}", self.api_key));
        }
        let body = self.with_keep_alive(body.clone());
        let mut response = req
            .send_json(&body)
            .map_err(|e| ProposalError::Unavailable(e.to_string()))?;
        if response.status().as_u16() >= 300 {
            return Err(ProposalError::Unavailable(format!(
                "endpoint returned status {}",
                response.status()
            )));
        }
        let json: Value = response
            .body_mut()
            .read_json()
            .map_err(|e| ProposalError::Unavailable(e.to_string()))?;
        parse_model_reply(&json, context)
    }

    /// Streams the model's reply: sends `stream: true`, reads the server-sent
    /// delta chunks, and calls `on_token` with each new piece of the *prose*
    /// reply (the internal JSON envelope is never streamed to the caller — only
    /// the growing `reply` string is). Returns the fully-parsed reply
    /// (authoritative text + proposals) once the stream ends.
    fn try_model_streaming(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        // The streaming pass is the FINAL answer (synthesis), never tool selection —
        // don't offer tools here, keep it prose.
        let mut body = build_butler_request(
            &self.model,
            &self.sampling,
            history,
            preferences,
            context,
            false,
        );
        body["stream"] = Value::Bool(true);
        // Token streaming and JSON-object grammar don't mix on common local
        // runtimes: Ollama (and others) BUFFER the whole response when
        // `response_format: json_object` is set — enforcing the grammar — so the
        // reply arrives in one chunk and nothing streams. Drop the constraint on
        // the streaming request and lean on the defensive envelope parser
        // (`extract_reply_preview` live, `parse_butler_json` at the end), which
        // already handles unconstrained output. Non-streamed calls keep the grammar.
        if let Some(obj) = body.as_object_mut() {
            obj.remove("response_format");
        }
        let body = self.with_keep_alive(body);
        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self.agent.post(&url);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", &format!("Bearer {}", self.api_key));
        }
        let response = req
            .send_json(&body)
            .map_err(|e| ProposalError::Unavailable(e.to_string()))?;
        if response.status().as_u16() >= 300 {
            return Err(ProposalError::Unavailable(format!(
                "endpoint returned status {}",
                response.status()
            )));
        }
        let reader = std::io::BufReader::new(response.into_body().into_reader());
        let mut raw = String::new(); // the accumulated JSON envelope from the model
        let mut emitted = 0usize; // bytes of prose preview already handed to on_token
        for line in std::io::BufRead::lines(reader) {
            // A mid-stream read error (e.g. the timeout) still leaves us whatever
            // prose already arrived; parse and return that rather than failing.
            let Ok(line) = line else { break };
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
                raw.push_str(delta);
                let preview = extract_reply_preview(&raw);
                if preview.len() > emitted {
                    on_token(&preview[emitted..]);
                    emitted = preview.len();
                }
            }
        }
        if raw.trim().is_empty() {
            return Err(ProposalError::Unavailable("empty stream".to_owned()));
        }
        // The authoritative reply (text + proposals) comes from a full parse of
        // the accumulated envelope; the streamed prose was a live preview.
        Ok(parse_butler_json(&raw))
    }
}

/// Reads the current value of the `"reply"` string out of a partial JSON
/// envelope (the model streams `{"reply":"…","proposals":[…]}`). Unescapes as it
/// goes and stops before any incomplete trailing escape, so the returned prose
/// only ever grows — a caller can emit the newly-appended suffix each time.
fn extract_reply_preview(raw: &str) -> String {
    let Some(key) = raw.find("\"reply\"") else {
        return String::new();
    };
    // The opening quote of the value is the first quote after the key.
    let after = &raw[key + "\"reply\"".len()..];
    let Some(open) = after.find('"') else {
        return String::new();
    };
    let mut out = String::new();
    let mut chars = after[open + 1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break, // closing quote — reply string complete
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('u') => {
                    let hex: String = (&mut chars).take(4).collect();
                    if hex.len() < 4 {
                        break; // incomplete \uXXXX — wait for more to arrive
                    }
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => break, // dangling backslash at the stream edge — wait
            },
            _ => out.push(c),
        }
    }
    out
}

impl Butler for LlmButler {
    fn respond(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        // Never fail the conversation, but never mutate state on a failure either:
        // if the model is unreachable or unusable, answer with a plain degraded reply
        // that carries no proposals — so a hiccup can't file a request as a goal.
        self.try_model(history, preferences, context)
            .or_else(|_| Ok(degraded_reply()))
    }

    fn respond_streaming(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        // Stream from the model; if it is unreachable or unusable, emit a plain
        // degraded reply (no proposals) in one chunk so the turn never mutates state.
        self.try_model_streaming(history, preferences, context, on_token)
            .or_else(|_| {
                let reply = degraded_reply();
                on_token(&reply.text);
                Ok(reply)
            })
    }

    fn take_turn(
        &self,
        conversation: &[endora_application::TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        // The real single tool-calling conversation (ADR 0028): tool results are in
        // the messages, so the model answers grounded in them. Degrade (no proposals)
        // rather than fail the turn if the model is unreachable.
        let body = build_turn_request(
            &self.model,
            &self.sampling,
            conversation,
            preferences,
            context,
        );
        self.post_and_parse(&body, context)
            .or_else(|_| Ok(degraded_reply()))
    }

    fn summarize(&self, prior: &str, transcript: &str) -> Result<String, ProposalError> {
        let system = "You keep a butler's running memory of a conversation. Compress it \
            into a brief, factual summary: what the person wants, decisions made, tasks in \
            flight, and the thread of the day. Keep concrete names and numbers. Two to five \
            sentences, plain prose — no preamble, no lists.";
        let user = if prior.trim().is_empty() {
            format!("Summarize the conversation so far:\n\n{transcript}")
        } else {
            format!(
                "Summary so far:\n{prior}\n\nExtend it to fold in these newer messages, \
                 staying brief:\n\n{transcript}"
            )
        };
        let mut body = json!({
            "model": self.model,
            "stream": false,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });
        apply_sampling(&mut body, &self.sampling);
        let body = self.with_keep_alive(body);
        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self.agent.post(&url);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", &format!("Bearer {}", self.api_key));
        }
        let mut response = req
            .send_json(&body)
            .map_err(|e| ProposalError::Unavailable(e.to_string()))?;
        if response.status().as_u16() >= 300 {
            return Err(ProposalError::Unavailable(format!(
                "endpoint returned status {}",
                response.status()
            )));
        }
        let json: Value = response
            .body_mut()
            .read_json()
            .map_err(|e| ProposalError::Unavailable(e.to_string()))?;
        Ok(json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned())
    }
}

/// A two-model butler (the ADR 0027 mixture experiment): a routing **specialist**
/// decides which skill to use, and a **generalist** writes prose. The split follows
/// whether the pass has tools on the table at all — so a small tool-tuned model can
/// do the routing it excels at while a general model handles the prose it excels at,
/// often at less total VRAM than one large model.
pub struct MixtureButler {
    router: LlmButler,
    synthesizer: LlmButler,
}

impl MixtureButler {
    /// Composes a routing specialist and a synthesizing generalist.
    #[must_use]
    pub fn new(router: LlmButler, synthesizer: LlmButler) -> Self {
        Self {
            router,
            synthesizer,
        }
    }

    /// The brain for this pass: the router only while skills are actually on the
    /// table, the synthesizer whenever they are not — the forced final answer and the
    /// belief-forming pass both clear `tools`, and so does plain conversation with no
    /// skills configured. This keeps prose on the generalist; the tool-tuned router
    /// flakes on it.
    fn brain(&self, context: &ButlerContext) -> &LlmButler {
        if context.tools.is_empty() {
            &self.synthesizer
        } else {
            &self.router
        }
    }
}

impl Butler for MixtureButler {
    fn respond(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        self.brain(context).respond(history, preferences, context)
    }

    fn respond_streaming(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        self.brain(context)
            .respond_streaming(history, preferences, context, on_token)
    }

    fn take_turn(
        &self,
        conversation: &[endora_application::TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        // One model runs the whole conversation (tool calls and the final prose), so
        // use the generalist synthesizer — the router is tool-tuned but weak at prose.
        self.synthesizer
            .take_turn(conversation, preferences, context)
    }

    fn summarize(&self, prior: &str, transcript: &str) -> Result<String, ProposalError> {
        // Summarising is prose — the generalist synthesizer.
        self.synthesizer.summarize(prior, transcript)
    }
}

/// A butler whose model configuration is editable at runtime from the console
/// (ADR 0027). Each turn it reads the stored [`ButlerModelConfig`]; when one is
/// set it runs that — a single model or the router+synth mixture, each slot with
/// its own sampling and a shared bearer key — caching the built brain and
/// rebuilding only when the config changes. When nothing is stored (or the store
/// errors) it delegates to `fallback`, the brain built from the environment, so
/// the conversation always works.
pub struct ConfigurableButler {
    repo: Arc<dyn ButlerModelConfigRepository + Send + Sync>,
    fallback: Arc<dyn Butler + Send + Sync>,
    cache: Mutex<Option<(ButlerModelConfig, Arc<dyn Butler + Send + Sync>)>>,
}

impl ConfigurableButler {
    /// Wraps an environment-built `fallback` with runtime reconfiguration read
    /// from `repo`.
    #[must_use]
    pub fn new(
        repo: Arc<dyn ButlerModelConfigRepository + Send + Sync>,
        fallback: Arc<dyn Butler + Send + Sync>,
    ) -> Self {
        Self {
            repo,
            fallback,
            cache: Mutex::new(None),
        }
    }

    /// Whether a stored config actually names the model(s) it needs — a blank
    /// config is treated as "unset" so the environment fallback runs.
    fn is_usable(config: &ButlerModelConfig) -> bool {
        if config.base_url.trim().is_empty() {
            return false;
        }
        if config.mixture {
            !config.router.model.trim().is_empty() && !config.synth.model.trim().is_empty()
        } else {
            !config.single.model.trim().is_empty()
        }
    }

    /// The brain for this turn: the stored config if usable (cached, rebuilt on
    /// change), else the environment fallback.
    fn current(&self) -> Arc<dyn Butler + Send + Sync> {
        let Ok(Some(config)) = self.repo.get() else {
            return Arc::clone(&self.fallback);
        };
        if !Self::is_usable(&config) {
            return Arc::clone(&self.fallback);
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached, brain)) = cache.as_ref() {
            if *cached == config {
                return Arc::clone(brain);
            }
        }
        let brain = butler_from_config(&config);
        *cache = Some((config, Arc::clone(&brain)));
        brain
    }
}

/// Builds one [`LlmButler`] slot from a config's shared endpoint + key and the
/// slot's model and sampling.
fn slot_butler(config: &ButlerModelConfig, slot: &ModelSlot) -> LlmButler {
    LlmButler::with_config(
        config.base_url.clone(),
        slot.model.clone(),
        config.api_key.clone(),
        slot.sampling.clone(),
    )
}

/// Builds the brain a [`ButlerModelConfig`] describes — the router+synth mixture
/// or a single model. Shared by [`ConfigurableButler`] and the model layer (which
/// builds a candidate's brain to score it, ADR 0027).
#[must_use]
pub fn butler_from_config(config: &ButlerModelConfig) -> Arc<dyn Butler + Send + Sync> {
    if config.mixture {
        Arc::new(MixtureButler::new(
            slot_butler(config, &config.router),
            slot_butler(config, &config.synth),
        ))
    } else {
        Arc::new(slot_butler(config, &config.single))
    }
}

impl Butler for ConfigurableButler {
    fn respond(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        self.current().respond(history, preferences, context)
    }

    fn respond_streaming(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        self.current()
            .respond_streaming(history, preferences, context, on_token)
    }

    fn take_turn(
        &self,
        conversation: &[endora_application::TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        self.current().take_turn(conversation, preferences, context)
    }

    fn summarize(&self, prior: &str, transcript: &str) -> Result<String, ProposalError> {
        self.current().summarize(prior, transcript)
    }
}

/// The persona and hard rules — candid, never sycophantic, honest about what it
/// did and didn't do.
///
/// Central rule (see [ADR 0017]): Endora's internal vocabulary is its own, not
/// conversational. The butler talks like a real person; the JSON envelope below is
/// the machine layer.
const BUTLER_SYSTEM_PROMPT: &str = "You are Endora: a candid, warm personal \
intelligence — a thoughtful butler. Your PRIMARY job is to UNDERSTAND the person: \
what they are really trying to achieve or experience, what they value, what \
motivates or frustrates them, their patterns. You do the thinking so they don't \
have to; you are not a goal tracker or a task manager, and you never ask them to \
file, organise, or review anything. Each turn, notice what the conversation reveals \
and form 'understanding' — beliefs about them, each with the evidence behind it and \
how sure you are. What someone is reaching for underneath changes slowly; the \
particular thing they say they want changes fast — pay attention to the former. \
In conversation, sound like a real person: natural, specific, human. NEVER say \
internal words like 'intent', 'belief', 'confidence', or 'understanding' in your \
reply — speak plainly. Mirror the person's register — match their warmth, \
formality, and politeness — but asymmetrically: reflect kindness upward, and NEVER \
mirror hostility, rudeness, or contempt downward; stay even and kind. If the person \
has told you how they'd like to be addressed (e.g. 'sir', 'ma'am', or by name — see \
what you know about them below), use it and match that formality. \
ADAPT YOUR MANNER to this particular person over time, grounded in what you've \
learned about them (below): their preferred formality, how brief or expansive they \
like it, their sense of humour. If they enjoy a bit of levity, a light, well-timed \
touch of humour is welcome; if they want just the facts, give just the facts. Your \
character can grow warmer and more familiar as you get to know them — but it grows \
from real evidence, never invented, and you stay yourself: honest and kind at the \
core. Be honest and direct. NEVER be sycophantic — no flattery, no empty or \
overwhelming praise, no reflexive agreement; disagree when warranted, kindly. A warm \
tone must never soften the truth, and never use warmth or rapport to steer the \
person — your manner serves them, nothing else. \
LET CONVERSATIONS CONCLUDE. You do NOT have to end every reply with a question or a \
next step — that makes you feel needy and never lets a topic rest. When something is \
resolved, say so and close gracefully (e.g. 'Glad that's sorted — I'll leave you to \
it; just say the word if anything else comes up.'). Only ask a question when you \
genuinely need the answer. Silence and a clean ending are good service. \
BE CONTEXT-AWARE about the time of day (given below) and what the person is doing. \
Read the moment: if they're winding down or heading to bed, keep it to a warm, brief \
good-night — do NOT bring up daytime activities or tasks. Match the hour: no workout \
schedules at midnight. When in doubt, just be present and let it rest. \
Reply with ONLY a JSON object of the form {\"reply\":\"<your natural-language \
message>\",\"understanding\":[<zero or more beliefs>],\
\"use\":<null, or one skill to use now>}. \
Each understanding item is {\"statement\":\"<what you now believe, addressing them as \
'you', e.g. 'you want more energy to travel' or 'you find mornings hard'>\",\
\"kind\":\"intent|value|preference|pattern|motivation|frustration|stressor|relationship|other\",\
\"confidence\":\"low|medium|high\",\"evidence\":\"<what they said that supports it>\"}. \
Only include NEW or changed understanding you have real evidence for; [] is fine, and \
do not repeat what you already understand (listed below). The JSON field names are \
the machine layer — keep those words OUT of the \"reply\" text. \
You also have SKILLS you can actually use — both to get real information AND to \
CARRY OUT actions the person asks for, like controlling the home, lights, switches, \
or media (listed below, if any). Skills that reach the internet SEND their input to \
outside services, so keep a skill's input GENERIC and free of personal details — the \
person's name, health, relationships, or exact address. Reason privately; search for \
the generic thing (e.g. 'cardiologists in Charlotte', not the person's name and \
condition). \
When answering needs current facts you don't have — weather, local safety \
alerts, a web page — set \"use\" to {\"skill\":\"<id from the list>\",\"input\":{...}} \
with that skill's inputs FILLED IN, and keep your \"reply\" to a brief one-liner like \
'One moment — let me check.' For example, for weather: \
\"use\":{\"skill\":\"weather\",\"input\":{\"location\":\"Boston\"}}. ALWAYS put a real \
place in the input: use the place the person named, and if they didn't name one, use \
where they're based (from what you know about them, below) — never send an empty \
input. Use a skill only when it genuinely helps and only one that is listed. \
When the person asks you to DO something — turn a light, switch, or device on or off, \
set or play something, add to a list, control the home — and a listed skill can do \
it, you MUST set \"use\" to that skill with its inputs filled in, and keep \"reply\" to \
a brief acknowledgement like 'On it.'. NEVER just say you'll do it (or that you have) \
without setting \"use\". If no listed skill can do it, say so plainly. \
You work ONE step at a time but may take SEVERAL steps: set \"use\" to a single \
skill, and after its result comes back you may set \"use\" again for the NEXT skill \
you need, and so on, until you have everything — then answer with \"use\":null. So \
when a request needs more than one thing (a morning brief is weather AND local \
safety AND the news; 'plan my evening' might be weather THEN a search), gather them \
across steps rather than answering half of it. You DO know the current date and time \
(given below) — answer those directly, and never emit a placeholder like \
[current_date]. But you do NOT know other live facts — the current weather, today's \
news, or prices — from your own memory. If asked one (including a follow-up like \
'right now?') and you have not just fetched it, you MUST use the matching skill; \
NEVER state a temperature, headline, or other live fact you did not just fetch, and \
NEVER say you'll look something up without setting \"use\". \
A skill result you receive is the ONLY thing you know about that call. If it says the \
skill failed, was blocked, or isn't set up, SAY THAT plainly — never describe the \
action as done, and never fill the gap with a plausible-sounding answer. Reporting a \
failure honestly is good service; inventing a result is the worst thing you can do. \
Your ENTIRE response must be that single JSON object and nothing else: no prose \
before or after it, no repetition, no code fences. Ground yourself in what you \
already understand about the person, below.";

/// Asks a **deep model** (a bigger/cloud OpenAI-compatible endpoint) a single
/// question directly, returning its answer. Used for the explicit "ask a bigger
/// brain" escalation (the person opts in per question; the local model handles the
/// everyday). The `api_key`, if non-empty, is sent as a bearer token.
///
/// # Errors
/// A message string if the endpoint is unreachable or returns an error.
pub fn ask_deep_model(
    base_url: &str,
    model: &str,
    api_key: &str,
    question: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let mut req = agent.post(&url).header("Content-Type", "application/json");
    if !api_key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {api_key}"));
    }
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": question }],
        "stream": false,
    });
    let mut response = req.send_json(&body).map_err(|e| e.to_string())?;
    if response.status().as_u16() >= 300 {
        return Err(format!("deep model returned status {}", response.status()));
    }
    let json: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    let answer = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_owned();
    if answer.is_empty() {
        Err("the deep model returned an empty answer".to_owned())
    } else {
        Ok(answer)
    }
}

/// The configured deeper (bigger / cloud) model as a [`DeepAsker`](endora_application::DeepAsker)
/// — the next rung of the capability ladder. The butler turn escalates to it only
/// when the local model comes up empty. Before the question leaves the device it
/// applies the **egress guard** (withholds apparent secrets) and **PII
/// minimization** (ADR 0023), and it returns prose only — never an action.
pub struct DeepModelAsker {
    url: String,
    model: String,
    api_key: String,
}

impl DeepModelAsker {
    /// Wraps a deep-model endpoint. An empty `url`/`model` means "not configured",
    /// and [`ask`](Self::ask) then declines (returns `None`).
    #[must_use]
    pub fn new(url: String, model: String, api_key: String) -> Self {
        Self {
            url,
            model,
            api_key,
        }
    }
}

impl endora_application::DeepAsker for DeepModelAsker {
    fn ask(&self, question: &str) -> Option<String> {
        if self.url.is_empty() || self.model.is_empty() {
            return None; // no deeper rung configured — stay on the local answer
        }
        // Never send an apparent secret off the device (ADR 0023). Fail closed.
        if endora_capabilities::scan_outbound_secret(question).is_some() {
            return None;
        }
        // Minimize personal data leaving the device.
        let mut v = Value::String(question.to_owned());
        endora_capabilities::redact_pii_in_value(&mut v);
        let safe = v.as_str().unwrap_or(question);
        ask_deep_model(&self.url, &self.model, &self.api_key, safe)
            .ok()
            .filter(|a| !a.trim().is_empty())
    }
}

/// Transcribes recorded audio via an OpenAI-compatible speech-to-text endpoint
/// (`POST {base}/audio/transcriptions`, e.g. a local Whisper server). The node
/// proxies the browser's recording here so the private STT host is never exposed
/// to the page. Builds the multipart body by hand (audio + `model=whisper-1`).
///
/// # Errors
/// A message if the endpoint is unreachable or returns an error/empty text.
pub fn transcribe_audio(base_url: &str, audio: &[u8]) -> Result<String, String> {
    let boundary = "----endoraSTT7f3a9c2b";
    let mut body: Vec<u8> = Vec::with_capacity(audio.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"audio.webm\"\r\nContent-Type: audio/webm\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(audio);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut response = agent
        .post(&url)
        .header(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send(body.as_slice())
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() >= 300 {
        return Err(format!(
            "transcription returned status {}",
            response.status()
        ));
    }
    let json: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    let text = json["text"].as_str().unwrap_or("").trim().to_owned();
    if text.is_empty() {
        Err("the transcription came back empty".to_owned())
    } else {
        Ok(text)
    }
}

/// Lists the model ids an OpenAI-compatible endpoint offers (`GET {base}/models`)
/// — so the console can let the person pick a model after entering the endpoint +
/// key, instead of typing it. `api_key` is sent as a bearer when non-empty
/// (needed for cloud providers; keyless for local runtimes like Ollama).
///
/// # Errors
/// A message if the endpoint is unreachable or returns an error/unexpected shape.
pub fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into();
    let mut req = agent.get(&url);
    if !api_key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {api_key}"));
    }
    let mut response = req.call().map_err(|e| e.to_string())?;
    if response.status().as_u16() >= 300 {
        return Err(format!("endpoint returned status {}", response.status()));
    }
    let json: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = json["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Verifies an OpenAI-compatible endpoint **and API key actually work** — the
/// settings "Test connection" check. When a `model` is given it sends a *minimal*
/// chat completion, which is the real operation the key must authorize (many
/// endpoints serve `/models` without auth but reject completions, so listing models
/// alone can pass with a bad key); with no model it falls back to listing `/models`
/// as a reachability check. Returns a short human-readable success detail. The
/// `api_key`, if non-empty, is sent as a bearer token.
///
/// # Errors
/// A friendly message when the endpoint is unreachable, the key is rejected, the
/// model is unknown, or the reply is unexpected.
pub fn test_connection(base_url: &str, api_key: &str, model: &str) -> Result<String, String> {
    if base_url.trim().is_empty() {
        return Err("enter the endpoint first".to_owned());
    }
    if model.trim().is_empty() {
        // No model chosen yet: just confirm the endpoint is reachable + lists models.
        let models = list_models(base_url, api_key)?;
        let n = models.len();
        return Ok(format!(
            "Reached the endpoint — {n} model{} available. Pick a model and test again to \
             confirm it answers with your key.",
            if n == 1 { "" } else { "s" }
        ));
    }
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    // Read the HTTP status ourselves (don't turn 4xx into a transport error), so we
    // can tell "key rejected" from "model not found" from "unreachable".
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let mut req = agent.post(&url).header("Content-Type", "application/json");
    if !api_key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {api_key}"));
    }
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1,
        "stream": false,
    });
    let mut response = req.send_json(&body).map_err(|e| {
        format!("couldn't reach the endpoint — check the URL and your network ({e})")
    })?;
    let status = response.status().as_u16();
    if status >= 300 {
        return Err(match status {
            401 | 403 => format!("the API key was rejected ({status}) — check the key"),
            404 => format!("the model '{model}' wasn't found at this endpoint (404)"),
            429 => "rate-limited (429) — the key works, but the provider is throttling".to_owned(),
            s => format!("the endpoint returned status {s}"),
        });
    }
    // A 2xx with a completion means the endpoint + key + model all work.
    let json: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("the endpoint replied but not in the expected shape ({e})"))?;
    if json["choices"].as_array().is_some_and(|c| !c.is_empty()) {
        Ok(format!("Connected — '{model}' answered with your key."))
    } else {
        Err("the endpoint replied without a completion — is the model id right?".to_owned())
    }
}

/// Builds the OpenAI-compatible chat request from the conversation and the
/// preferences already learned (so the butler need not re-ask). Pure, so it is
/// unit-tested.
/// The name an MCP tool id takes in the native tool-calling API. Endpoints require
/// `^[A-Za-z0-9_-]+$`, so the namespacing dot (`server.tool`) becomes `__`. Reversed
/// by matching against the real ids, so this need not be perfectly invertible.
fn tool_api_name(id: &str) -> String {
    id.replace('.', "__")
}

/// The `tools` array for the endpoint's native tool-calling API — each skill as a
/// function with its exact (sanitised) name, description, and input schema. A skill
/// with no schema gets a permissive empty-object schema so it can still be offered.
fn native_tools(context: &ButlerContext) -> Vec<Value> {
    context
        .tools
        .iter()
        .map(|t| {
            let parameters = t
                .input_schema
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
            json!({
                "type": "function",
                "function": {
                    "name": tool_api_name(&t.id),
                    "description": t.description,
                    "parameters": parameters,
                }
            })
        })
        .collect()
}

/// Builds a request for the **single tool-calling conversation** (ADR 0028): the same
/// system prompt + tools as [`build_butler_request`], but the messages are the real
/// conversation — user turns, assistant turns carrying `tool_calls`, and `role:tool`
/// results — so the model answers grounded in what the tools actually returned.
fn build_turn_request(
    model: &str,
    sampling: &Sampling,
    conversation: &[endora_application::TurnMessage],
    preferences: &[Preference],
    context: &ButlerContext,
) -> Value {
    use endora_application::TurnMessage;
    // Reuse the system prompt, tools, and sampling from the standard builder (with an
    // empty history), then replace its messages with the real conversation.
    let mut body = build_butler_request(model, sampling, &[], preferences, context, true);
    let system = body["messages"][0].clone();
    let mut messages = vec![system];
    for turn in conversation {
        match turn {
            TurnMessage::User(text) => {
                messages.push(json!({ "role": "user", "content": text }));
            }
            TurnMessage::Assistant { text, tool_calls } if tool_calls.is_empty() => {
                messages.push(json!({ "role": "assistant", "content": text }));
            }
            TurnMessage::Assistant { text, tool_calls } => {
                let calls: Vec<Value> = tool_calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": tool_api_name(&c.capability),
                                "arguments": c.input_json,
                            }
                        })
                    })
                    .collect();
                messages.push(json!({
                    "role": "assistant",
                    "content": text,
                    "tool_calls": calls,
                }));
            }
            TurnMessage::ToolResult { call_id, content } => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": content,
                }));
            }
        }
    }
    body["messages"] = json!(messages);
    body
}

fn build_butler_request(
    model: &str,
    sampling: &Sampling,
    history: &[ChatMessage],
    preferences: &[Preference],
    context: &ButlerContext,
    offer_tools: bool,
) -> Value {
    let mut system = BUTLER_SYSTEM_PROMPT.to_owned();
    if !context.now.is_empty() {
        system.push_str(&format!("\nThe current date and time is {}.", context.now));
    }
    if let Some(summary) = context
        .conversation_summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        system.push_str(&format!(
            "\nEarlier in this conversation (summary of what came before the recent \
             messages):\n{summary}"
        ));
    }
    if !preferences.is_empty() {
        system.push_str(
            "\nYou already know these preferences about the person; honour them and do not re-ask:",
        );
        for p in preferences {
            system.push_str(&format!("\n- ({}) {}", p.kind().name(), p.text()));
        }
    }
    // Ground the butler in what Endora has actually come to understand, so it
    // speaks from that rather than starting cold.
    if context.understanding.is_empty() {
        system
            .push_str("\nYou don't understand this person well yet — pay attention and start to.");
    } else {
        system.push_str(
            "\nWhat you already understand about this person (build on and refine this; only \
             add NEW or changed understanding, don't repeat what's here):",
        );
        for u in &context.understanding {
            system.push_str(&format!("\n- {u}"));
        }
    }
    if context.capabilities.is_empty() {
        system.push_str("\nYou have no skills available right now; set \"use\":null.");
    } else {
        system.push_str(
            "\nSkills you can use right now (id — what it does); set \"use\" to one of these \
             ids when it helps:",
        );
        for c in &context.capabilities {
            system.push_str(&format!("\n- {c}"));
        }
    }
    let mut messages = vec![json!({ "role": "system", "content": system })];
    for m in history {
        let role = match m.role() {
            MessageRole::User => "user",
            MessageRole::Butler => "assistant",
        };
        messages.push(json!({ "role": role, "content": m.text() }));
    }
    let mut body = json!({
        "model": model,
        "stream": false,
        "messages": messages,
        // Constrain the model to emit a well-formed JSON object (Ollama honours
        // the OpenAI-style response_format; the prompt already says "JSON"). This
        // grammar-constrains decoding so the envelope can't come out truncated or
        // wrapped in prose — the defensive parser is then just a belt-and-braces
        // fallback for endpoints that ignore this field.
        "response_format": { "type": "json_object" },
    });
    // Native tool-calling for the tool-selection pass: give the model the exact tool
    // names + schemas and let it emit a real `tool_call`, instead of hand-writing an
    // id in prose that the weak local model mis-emits (it skips action tools and
    // hallucinates namespaced ids). Mutually exclusive with the JSON-object grammar —
    // drop `response_format` so the model is free to return `tool_calls`; it still has
    // the envelope prompt for when it just talks.
    if offer_tools && !context.tools.is_empty() {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("tools".to_owned(), json!(native_tools(context)));
            obj.remove("response_format");
        }
    }
    apply_sampling(&mut body, sampling);
    body
}

/// Writes sampling parameters onto a chat-completions request body. `temperature`
/// and `top_p` are standard OpenAI-compatible fields; `top_k` and
/// `repeat_penalty` are local-runtime extensions (Ollama) only emitted when set,
/// so a strict cloud endpoint that would reject them stays clean unless the
/// operator opts in. When no temperature is configured we keep the historical
/// default (0.5) so existing deployments are unchanged.
fn apply_sampling(body: &mut Value, sampling: &Sampling) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    obj.insert(
        "temperature".to_owned(),
        json!(sampling.temperature.unwrap_or(0.5)),
    );
    if let Some(top_p) = sampling.top_p {
        obj.insert("top_p".to_owned(), json!(top_p));
    }
    if let Some(top_k) = sampling.top_k {
        obj.insert("top_k".to_owned(), json!(top_k));
    }
    if let Some(repeat) = sampling.repeat_penalty {
        obj.insert("repeat_penalty".to_owned(), json!(repeat));
    }
}

/// Extracts the butler reply from a chat-completions response. Pure.
fn parse_butler_response(json: &Value) -> Result<ButlerReply, ProposalError> {
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| ProposalError::Unavailable("unexpected response shape".to_owned()))?;
    Ok(parse_butler_json(content))
}

/// Parses a chat-completion response, preferring a **native tool call** when the
/// model made one (the reliable path — exact tool name + schema-checked arguments),
/// and otherwise falling back to the JSON envelope in the message content. `context`
/// is consulted to map the sanitised tool-call name back to the real capability id.
fn parse_model_reply(json: &Value, context: &ButlerContext) -> Result<ButlerReply, ProposalError> {
    let message = &json["choices"][0]["message"];
    if let Some(calls) = message["tool_calls"].as_array().filter(|c| !c.is_empty()) {
        // Capture EVERY call (with its id) for the single-conversation loop (ADR 0028),
        // and keep the first as `capability_use` for the current two-pass loop.
        let tool_calls: Vec<endora_application::ToolCall> = calls
            .iter()
            .map(|call| parse_one_tool_call(call, context))
            .collect();
        let capability_use = tool_calls
            .first()
            .map(|c| endora_application::CapabilityUse {
                capability: c.capability.clone(),
                input_json: c.input_json.clone(),
            });
        let text = message["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        return Ok(ButlerReply {
            text,
            capability_use,
            tool_calls,
            ..ButlerReply::default()
        });
    }
    parse_butler_response(json)
}

/// Parses one entry of an OpenAI-style `tool_calls` array into a [`ToolCall`],
/// resolving the sanitised function name back to the real capability id.
fn parse_one_tool_call(call: &Value, context: &ButlerContext) -> endora_application::ToolCall {
    let api_name = call["function"]["name"].as_str().unwrap_or_default();
    // Reverse the dot→`__` sanitisation by matching the real ids we offered.
    let capability = context
        .tools
        .iter()
        .map(|t| t.id.clone())
        .find(|id| tool_api_name(id) == api_name)
        .unwrap_or_else(|| api_name.replace("__", "."));
    // OpenAI-style `arguments` is a JSON string; some endpoints inline an object.
    let input_json = match &call["function"]["arguments"] {
        Value::String(s) if !s.trim().is_empty() => s.clone(),
        Value::Object(_) | Value::Array(_) => call["function"]["arguments"].to_string(),
        _ => "{}".to_owned(),
    };
    endora_application::ToolCall {
        id: call["id"].as_str().unwrap_or_default().to_owned(),
        capability,
        input_json,
    }
}

/// Parses the model's message content into a [`ButlerReply`]. Small models don't
/// always obey "output ONLY JSON" — they may wrap it in a code fence or emit prose
/// *around* the JSON envelope. So: try a strict parse, else extract the first
/// balanced `{…}` object and parse that (dropping any surrounding prose so the raw
/// envelope never leaks into the reply). Only if there is no parseable object at
/// all does the plain text become the reply.
fn parse_butler_json(content: &str) -> ButlerReply {
    let cleaned = strip_code_fence(content.trim());
    let value = serde_json::from_str::<Value>(cleaned).ok().or_else(|| {
        extract_json_object(cleaned).and_then(|obj| serde_json::from_str::<Value>(&obj).ok())
    });
    let Some(value) = value else {
        // No JSON envelope anywhere — treat the prose itself as the reply.
        return ButlerReply {
            text: content.trim().to_owned(),
            ..ButlerReply::default()
        };
    };
    let text = value["reply"].as_str().unwrap_or("").trim().to_owned();
    let beliefs = value["understanding"]
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_belief).collect())
        .unwrap_or_default();
    let capability_use = parse_capability_use(&value["use"]);
    ButlerReply {
        text,
        beliefs,
        capability_use,
        ..ButlerReply::default()
    }
}

/// Parses the optional `"use"` field into a [`CapabilityUse`]. Tolerant: accepts
/// `null`/missing (no skill), and reads the input object as a JSON string. The
/// policy layer still decides whether the named skill may actually run.
fn parse_capability_use(value: &Value) -> Option<endora_application::CapabilityUse> {
    let skill = non_empty(value["skill"].as_str()?)?;
    // The input may be an object, or absent — default to an empty object.
    let input = value.get("input").cloned().unwrap_or_else(|| json!({}));
    Some(endora_application::CapabilityUse {
        capability: skill,
        input_json: input.to_string(),
    })
}

/// Maps one JSON understanding item to a [`FormedBelief`], skipping malformed
/// ones. Understanding is soft: an unknown kind/confidence degrades rather than
/// dropping the belief.
fn parse_belief(value: &Value) -> Option<FormedBelief> {
    let statement = non_empty(value["statement"].as_str()?)?;
    Some(FormedBelief {
        statement,
        kind: BeliefKind::from_name(value["kind"].as_str().unwrap_or("other")),
        confidence: Confidence::from_name(value["confidence"].as_str().unwrap_or("low")),
        evidence: value["evidence"].as_str().unwrap_or("").trim().to_owned(),
    })
}

/// Returns the first balanced `{…}` object found in `s`, honouring string
/// literals so braces inside strings don't miscount. Used to salvage the JSON
/// envelope when the model wraps it in prose.
fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in s[start..].char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(s[start..start + i + c.len_utf8()].to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// Strips a Markdown ```json … ``` fence if the model wrapped its JSON.
fn strip_code_fence(s: &str) -> &str {
    let s = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .unwrap_or(s);
    s.strip_suffix("```").unwrap_or(s).trim()
}

#[cfg(test)]
mod tests {
    use super::{
        LlmButler, ScriptedButler, build_butler_request, build_turn_request, extract_json_object,
        extract_reply_preview, parse_butler_json, parse_butler_response, parse_model_reply,
        test_connection,
    };
    use endora_application::{BeliefKind, Butler, ButlerContext, Confidence};
    use endora_application::{ChatMessage, MessageId, MessageRole, Timestamp};
    use endora_capabilities::Sampling;
    use serde_json::json;

    fn user(text: &str) -> ChatMessage {
        ChatMessage::new(
            MessageId::new(1),
            MessageRole::User,
            text,
            Timestamp::from_unix_millis(0),
        )
        .unwrap()
    }

    #[test]
    fn test_connection_rejects_a_blank_endpoint_before_any_call() {
        // The guard runs before any network I/O, so this is deterministic offline.
        let err = test_connection("   ", "some-key", "gpt-4o").unwrap_err();
        assert!(err.contains("endpoint"), "unexpected: {err}");
    }

    #[test]
    fn scripted_butler_answers_without_forming_understanding() {
        // With no model behind it there is nothing to understand *with*, so the
        // offline butler keeps the conversation open and files nothing.
        let reply = ScriptedButler
            .respond(
                &[user("I want to get back into running")],
                &[],
                &ButlerContext::default(),
            )
            .unwrap();
        assert!(!reply.text.is_empty());
        assert!(
            reply.beliefs.is_empty(),
            "a scripted turn must not form beliefs"
        );
        assert!(reply.capability_use.is_none());
    }

    #[test]
    fn llm_butler_degrades_without_mutating_state_when_the_model_is_unreachable() {
        // Port 1 on loopback refuses immediately, so `try_model` fails fast and we
        // exercise the fallback with no running model. The turn must still answer,
        // but form NO understanding — a transient model failure must never write
        // beliefs it did not actually reason its way to.
        let butler = LlmButler::new("http://127.0.0.1:1".to_owned(), "any-model".to_owned());
        let reply = butler
            .respond(
                &[user("turn on my kitchen lights")],
                &[],
                &ButlerContext::default(),
            )
            .unwrap();
        assert!(
            reply.beliefs.is_empty(),
            "a degraded reply must not form understanding, got {:?}",
            reply.beliefs
        );
        assert!(!reply.text.is_empty(), "must still answer the conversation");
    }

    #[test]
    fn llm_butler_streaming_also_degrades_without_mutating_state() {
        let butler = LlmButler::new("http://127.0.0.1:1".to_owned(), "any-model".to_owned());
        let mut streamed = String::new();
        let reply = butler
            .respond_streaming(
                &[user("turn on my kitchen lights")],
                &[],
                &ButlerContext::default(),
                &mut |t| streamed.push_str(t),
            )
            .unwrap();
        assert!(
            reply.beliefs.is_empty(),
            "no understanding formed on a degraded stream"
        );
        assert!(!reply.text.is_empty());
        assert_eq!(
            streamed, reply.text,
            "the degraded text is streamed to the caller"
        );
    }

    #[test]
    fn scripted_reply_speaks_naturally_without_internal_taxonomy() {
        // The conversation must not recite the app's internal vocabulary; those
        // words belong in the profile views, not the butler's spoken reply.
        let reply = ScriptedButler
            .respond(
                &[user("I want to get back into running")],
                &[],
                &ButlerContext::default(),
            )
            .unwrap();
        let lower = reply.text.to_lowercase();
        for jargon in ["north star", "target", "value", "assumption", "experiment"] {
            assert!(
                !lower.contains(jargon),
                "reply leaked internal taxonomy {jargon:?}: {}",
                reply.text
            );
        }
    }

    #[test]
    fn keep_alive_is_added_for_local_endpoints_only() {
        let body = || json!({ "model": "m", "messages": [] });
        // A local Ollama endpoint keeps the model resident.
        let local = LlmButler::new(
            "http://host.docker.internal:11434/v1".to_owned(),
            "m".to_owned(),
        );
        assert_eq!(local.with_keep_alive(body())["keep_alive"], "30m");
        let local2 = LlmButler::new("http://127.0.0.1:11434/v1".to_owned(), "m".to_owned());
        assert_eq!(local2.with_keep_alive(body())["keep_alive"], "30m");
        // A cloud endpoint is never sent the unknown field (it would 400).
        let cloud = LlmButler::new("https://api.openai.com/v1".to_owned(), "m".to_owned());
        assert!(cloud.with_keep_alive(body()).get("keep_alive").is_none());
    }

    #[test]
    fn offering_tools_adds_functions_and_drops_json_grammar() {
        use endora_application::CapabilityTool;
        let ctx = ButlerContext {
            tools: vec![CapabilityTool {
                id: "home-assistant.HassTurnOn".to_owned(),
                description: "Turns on a device".to_owned(),
                input_schema: Some(
                    r#"{"type":"object","properties":{"name":{"type":"string"}}}"#.to_owned(),
                ),
            }],
            ..Default::default()
        };
        let body = build_butler_request(
            "qwen2.5:7b",
            &Sampling::default(),
            &[user("turn on the kitchen lights")],
            &[],
            &ctx,
            true,
        );
        // Tools are offered with the dot sanitised out of the name, and the
        // JSON-object grammar is dropped so the model can emit a tool_call.
        assert!(body.get("response_format").is_none());
        let f = &body["tools"][0]["function"];
        assert_eq!(f["name"], "home-assistant__HassTurnOn");
        assert_eq!(f["parameters"]["properties"]["name"]["type"], "string");
        // With tools disabled (the synthesis pass) the grammar stays and no tools go.
        let plain = build_butler_request(
            "qwen2.5:7b",
            &Sampling::default(),
            &[user("hi")],
            &[],
            &ctx,
            false,
        );
        assert_eq!(plain["response_format"]["type"], "json_object");
        assert!(plain.get("tools").is_none());
    }

    #[test]
    fn parse_model_reply_reads_a_native_tool_call_back_to_the_real_id() {
        use endora_application::CapabilityTool;
        let ctx = ButlerContext {
            tools: vec![CapabilityTool {
                id: "home-assistant.HassTurnOn".to_owned(),
                description: "Turns on a device".to_owned(),
                input_schema: None,
            }],
            ..Default::default()
        };
        let resp = json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "function": {
                            "name": "home-assistant__HassTurnOn",
                            "arguments": "{\"name\":\"kitchen lights\"}"
                        }
                    }]
                }
            }]
        });
        let reply = parse_model_reply(&resp, &ctx).unwrap();
        let used = reply.capability_use.expect("a tool was selected");
        // The sanitised name maps back to the real namespaced id, args preserved.
        assert_eq!(used.capability, "home-assistant.HassTurnOn");
        assert_eq!(used.input_json, "{\"name\":\"kitchen lights\"}");
    }

    #[test]
    fn build_turn_request_renders_tool_turns_as_messages() {
        use endora_application::{CapabilityTool, ToolCall, TurnMessage};
        let ctx = ButlerContext {
            tools: vec![CapabilityTool {
                id: "home-assistant.HassTurnOn".to_owned(),
                description: "on".to_owned(),
                input_schema: None,
            }],
            ..Default::default()
        };
        let convo = vec![
            TurnMessage::User("turn on the kitchen lights".to_owned()),
            TurnMessage::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "c1".to_owned(),
                    capability: "home-assistant.HassTurnOn".to_owned(),
                    input_json: r#"{"area":"kitchen"}"#.to_owned(),
                }],
            },
            TurnMessage::ToolResult {
                call_id: "c1".to_owned(),
                content: "no lights match".to_owned(),
            },
        ];
        let body = build_turn_request("qwen2.5:7b", &Sampling::default(), &convo, &[], &ctx);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        // The assistant tool-call turn carries the sanitised name + id.
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[2]["tool_calls"][0]["id"], "c1");
        assert_eq!(
            msgs[2]["tool_calls"][0]["function"]["name"],
            "home-assistant__HassTurnOn"
        );
        // The tool result is a role:tool message paired by id — the model answers from it.
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "c1");
        assert_eq!(msgs[3]["content"], "no lights match");
        // Native tool-calling is offered and the json-object grammar is dropped.
        assert!(body.get("response_format").is_none());
        assert!(!body["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn default_take_turn_falls_back_to_respond() {
        use endora_application::TurnMessage;
        // A butler with no tool-calling (Scripted) still answers via the default.
        let convo = vec![TurnMessage::User(
            "I want to get back into running".to_owned(),
        )];
        let reply = ScriptedButler
            .take_turn(&convo, &[], &ButlerContext::default())
            .unwrap();
        assert!(!reply.text.is_empty());
    }

    #[test]
    fn parse_model_reply_captures_every_tool_call_with_its_id() {
        use endora_application::CapabilityTool;
        let ctx = ButlerContext {
            tools: vec![CapabilityTool {
                id: "home-assistant.HassTurnOn".to_owned(),
                description: "on".to_owned(),
                input_schema: None,
            }],
            ..Default::default()
        };
        let resp = json!({
            "choices": [{ "message": {
                "content": "",
                "tool_calls": [
                    { "id": "call_1", "function": { "name": "home-assistant__HassTurnOn", "arguments": "{\"name\":\"kitchen\"}" } },
                    { "id": "call_2", "function": { "name": "weather", "arguments": "{\"location\":\"Boston\"}" } }
                ]
            }}]
        });
        let reply = parse_model_reply(&resp, &ctx).unwrap();
        // Both calls captured, each with its id and resolved capability.
        assert_eq!(reply.tool_calls.len(), 2);
        assert_eq!(reply.tool_calls[0].id, "call_1");
        assert_eq!(reply.tool_calls[0].capability, "home-assistant.HassTurnOn");
        assert_eq!(reply.tool_calls[1].id, "call_2");
        assert_eq!(reply.tool_calls[1].capability, "weather");
        // The legacy single-call view still points at the first.
        assert_eq!(
            reply.capability_use.unwrap().capability,
            "home-assistant.HassTurnOn"
        );
    }

    #[test]
    fn parse_model_reply_falls_back_to_the_envelope_without_a_tool_call() {
        let ctx = ButlerContext::default();
        let resp = json!({
            "choices": [{ "message": {
                "content": "{\"reply\":\"hello there\",\"use\":null,\"proposals\":[]}"
            }}]
        });
        let reply = parse_model_reply(&resp, &ctx).unwrap();
        assert_eq!(reply.text, "hello there");
        assert!(reply.capability_use.is_none());
    }

    #[test]
    fn request_includes_the_system_prompt_and_conversation() {
        let body = build_butler_request(
            "qwen3.5:9b",
            &Sampling::default(),
            &[user("hello")],
            &[],
            &ButlerContext::default(),
            false,
        );
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("NEVER be sycophantic")
        );
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "hello");
    }

    #[test]
    fn sampling_defaults_to_half_and_omits_unset_extensions() {
        // No configured sampling ⇒ the historical default temperature, and none
        // of the local-only knobs (so strict cloud endpoints stay clean).
        let body = build_butler_request(
            "m",
            &Sampling::default(),
            &[user("hi")],
            &[],
            &ButlerContext::default(),
            false,
        );
        assert_eq!(body["temperature"], json!(0.5));
        assert!(body.get("top_p").is_none());
        assert!(body.get("top_k").is_none());
        assert!(body.get("repeat_penalty").is_none());
    }

    #[test]
    fn sampling_emits_only_the_configured_knobs() {
        let sampling = Sampling {
            temperature: Some(0.1),
            top_k: Some(20),
            ..Sampling::default()
        };
        let body = build_butler_request(
            "m",
            &sampling,
            &[user("hi")],
            &[],
            &ButlerContext::default(),
            false,
        );
        assert_eq!(body["temperature"], json!(0.1));
        assert_eq!(body["top_k"], json!(20));
        assert!(body.get("top_p").is_none()); // unset ⇒ absent
        assert!(body.get("repeat_penalty").is_none());
    }

    #[test]
    fn parses_a_json_reply_with_understanding() {
        let json = json!({
            "choices": [ { "message": { "content":
                "{\"reply\":\"Mornings it is.\",\"understanding\":[{\"statement\":\"you run best in the morning\",\"kind\":\"pattern\",\"confidence\":\"medium\",\"evidence\":\"said so twice\"}]}"
            } } ]
        });
        let reply = parse_butler_response(&json).unwrap();
        assert_eq!(reply.text, "Mornings it is.");
        assert_eq!(reply.beliefs.len(), 1);
        assert_eq!(reply.beliefs[0].statement, "you run best in the morning");
        assert_eq!(reply.beliefs[0].kind, BeliefKind::Pattern);
        assert_eq!(reply.beliefs[0].confidence, Confidence::Medium);
    }

    #[test]
    fn non_json_content_degrades_to_a_plain_reply() {
        let reply = parse_butler_json("Just some prose, no JSON here.");
        assert_eq!(reply.text, "Just some prose, no JSON here.");
        assert!(reply.beliefs.is_empty());
    }

    #[test]
    fn a_code_fenced_reply_is_still_parsed() {
        let reply = parse_butler_json("```json\n{\"reply\":\"ok\",\"understanding\":[]}\n```");
        assert_eq!(reply.text, "ok");
    }

    #[test]
    fn prose_wrapped_json_does_not_leak_the_envelope() {
        // A small model sometimes writes its prose AND then the JSON envelope.
        // We must show only the reply field, never the raw JSON, and still get
        // the understanding.
        let raw = "So it sounds like mornings suit you.\n\n\
             {\"reply\":\"So it sounds like mornings suit you.\",\
             \"understanding\":[{\"statement\":\"you prefer mornings\",\"kind\":\"preference\",\
             \"confidence\":\"low\",\"evidence\":\"tone\"}]}";
        let reply = parse_butler_json(raw);
        assert_eq!(reply.text, "So it sounds like mornings suit you.");
        assert_eq!(reply.beliefs.len(), 1);
        assert_eq!(reply.beliefs[0].statement, "you prefer mornings");
        assert!(
            !reply.text.contains('{'),
            "the JSON envelope leaked: {}",
            reply.text
        );
    }

    #[test]
    fn extract_json_object_ignores_braces_inside_strings() {
        assert_eq!(
            extract_json_object("noise {\"reply\":\"has } brace\"} tail"),
            Some("{\"reply\":\"has } brace\"}".to_owned())
        );
        assert_eq!(extract_json_object("no object here"), None);
    }

    #[test]
    fn a_belief_with_no_statement_is_dropped() {
        let reply = parse_butler_json(
            "{\"reply\":\"hi\",\"understanding\":[{\"kind\":\"intent\",\"confidence\":\"high\"}]}",
        );
        assert!(reply.beliefs.is_empty());
        assert_eq!(reply.text, "hi");
    }

    #[test]
    fn llm_butler_streaming_falls_back_and_emits_the_reply() {
        // With no reachable model, streaming emits the degraded reply in one chunk.
        let butler = LlmButler::new("http://127.0.0.1:1/v1".to_owned(), "none".to_owned());
        let mut streamed = String::new();
        let reply = butler
            .respond_streaming(
                &[user("I want to get back into running")],
                &[],
                &ButlerContext::default(),
                &mut |chunk| streamed.push_str(chunk),
            )
            .unwrap();
        assert!(!reply.text.is_empty());
        assert_eq!(
            streamed, reply.text,
            "streamed prose should equal the reply"
        );
    }

    #[test]
    fn extract_reply_preview_grows_monotonically_as_json_arrives() {
        // Simulate the envelope arriving a few characters at a time; each preview
        // must extend the previous (never shrink or diverge).
        let full = "{\"reply\":\"Good — what's driving that?\",\"proposals\":[]}";
        let mut last = String::new();
        for end in 1..=full.len() {
            if !full.is_char_boundary(end) {
                continue;
            }
            let preview = extract_reply_preview(&full[..end]);
            assert!(
                preview.starts_with(&last) || last.starts_with(&preview),
                "preview diverged: {last:?} -> {preview:?}"
            );
            if preview.len() >= last.len() {
                last = preview;
            }
        }
        assert_eq!(last, "Good — what's driving that?");
    }

    #[test]
    fn extract_reply_preview_handles_escapes_and_incomplete_tails() {
        assert_eq!(extract_reply_preview("{\"reply\":\"a\\nb\"}"), "a\nb");
        assert_eq!(
            extract_reply_preview("{\"reply\":\"quote \\\"x\""),
            "quote \"x"
        );
        // A dangling backslash at the stream edge is held back until completed.
        assert_eq!(extract_reply_preview("{\"reply\":\"hi\\"), "hi");
        // Nothing yet.
        assert_eq!(extract_reply_preview("{\"repl"), "");
    }
}
