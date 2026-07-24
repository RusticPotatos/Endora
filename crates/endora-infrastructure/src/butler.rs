//! Butler brains behind the [`Butler`] port (see
//! `docs/adr/0014-the-butler-conversation-values-attention.md`).
//!
//! Two implementations:
//! - [`ScriptedButler`] — deterministic and offline; it turns a stated aim into a
//!   proposed North Star and asks what value it serves. It proves the act/ask +
//!   propose loop without any model, and is the goal-capture brain used for tests
//!   and the model-layer eval baseline.
//! - [`LlmButler`] — model-backed (a local OpenAI-compatible endpoint). It asks
//!   the model for a candid, non-sycophantic reply plus proposals from a closed
//!   set. If the model is unavailable or returns something unusable it answers with
//!   a plain, honest *degraded* reply (see [`degraded_reply`]) so the conversation
//!   never breaks — crucially with **no proposals**, so a transient model failure
//!   never mutates state (e.g. filing a control request as a goal).
//!
//! Both only ever *propose*: the person confirms each action, and deterministic
//! use cases execute it. The model is never the enforcement boundary.

use endora_application::{
    BeliefKind, ChatMessage, Confidence, MessageRole, Preference, PreferenceKind,
};
use endora_application::{
    Butler, ButlerContext, ButlerProposal, ButlerReply, FormedBelief, ProposalError,
};
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

/// Turns the latest user message into a proposed North Star and an ask.
fn scripted_reply(history: &[ChatMessage]) -> ButlerReply {
    let last_user = history
        .iter()
        .rev()
        .find(|m| m.role() == MessageRole::User)
        .map(|m| m.text().trim().to_owned())
        .unwrap_or_default();
    if last_user.is_empty() {
        return ButlerReply {
            text: "What would you like to work on?".to_owned(),
            ..ButlerReply::default()
        };
    }
    let aim = strip_lead(&last_user);
    ButlerReply {
        text: format!(
            "Good — what's really driving that for you? \
             Want me to hold onto \"{aim}\" as something you're working toward?"
        ),
        proposals: vec![ButlerProposal::CreateNorthStar { title: aim }],
        ..ButlerReply::default()
    }
}

/// The reply an [`LlmButler`] gives when its model can't be reached or returns
/// something unusable. Honest about the transient failure and — deliberately —
/// carries **no proposals and no beliefs**: a degraded turn must never mutate state.
/// This is what keeps a model hiccup from filing "turn on my kitchen lights" as a
/// North Star. It answers the conversation without pretending to have understood.
fn degraded_reply() -> ButlerReply {
    ButlerReply {
        text: "Sorry — I couldn't reach my language model just now, so I didn't follow that \
               properly. Give me a moment and try again."
            .to_owned(),
        ..ButlerReply::default()
    }
}

/// Strips a leading intent phrase ("I want to …") to get the bare aim.
fn strip_lead(text: &str) -> String {
    let lower = text.to_lowercase();
    for lead in [
        "i want to ",
        "i'd like to ",
        "i would like to ",
        "i wanna ",
        "i need to ",
        "help me ",
        "i want ",
    ] {
        if let Some(rest) = lower.strip_prefix(lead) {
            // Preserve the original casing of the remainder.
            return text[text.len() - rest.len()..].trim().to_owned();
        }
    }
    text.to_owned()
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
const BUTLER_MODEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

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
        let body = build_butler_request(&self.model, &self.sampling, history, preferences, context);
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
        parse_butler_response(&json)
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
        let mut body =
            build_butler_request(&self.model, &self.sampling, history, preferences, context);
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
}

/// A two-model butler (the ADR 0027 mixture experiment): a routing **specialist**
/// decides which skill to use, and a **generalist** synthesizes the answer once
/// results are in. The split follows the agentic loop's own structure — a
/// *gathering* pass (no tool result yet) goes to the router; a *synthesis* pass (a
/// tool result is present) goes to the synthesizer — so a small tool-tuned model
/// can do the routing it excels at while a general model handles the prose it
/// excels at, often at less total VRAM than one large model.
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

    /// The brain for this pass: the synthesizer when writing the final answer (a
    /// tool result to relay, or `synthesize` set for plain prose), the router only
    /// while still deciding which skill to use. This keeps conversation on the
    /// generalist — the tool-tuned router flakes on plain chat.
    fn brain(&self, context: &ButlerContext) -> &LlmButler {
        if context.tool_result.is_some() || context.synthesize {
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
}

/// The persona and hard rules — candid, never sycophantic, proposes only.
///
/// Central rule (see [ADR 0017]): the app's internal taxonomy — values, North
/// Stars, targets — is the butler's private model of the person and their
/// browsable profile, NOT conversational vocabulary. The butler talks like a
/// real person; the structured `proposals` (the JSON `kind`s) are the machine
/// layer that maps the conversation onto that model silently.
const BUTLER_SYSTEM_PROMPT: &str = "You are Endora: a candid, warm personal \
intelligence — a thoughtful butler. Your PRIMARY job is to UNDERSTAND the person: \
what they are really trying to achieve or experience (their intent), what they \
value, what motivates or frustrates them, their patterns. You do the thinking so \
they don't have to; you are not a goal tracker or task manager. Each turn, notice \
what the conversation reveals and form 'understanding' — beliefs about them, each \
with the evidence behind it and how sure you are. Intent matters more than any \
goal: a goal is one changeable expression of a slow-changing intent. When it helps, \
gently offer a small, concrete next step or suggestion, sized to how sure you are \
(more uncertain → smaller, or just ask). \
In conversation, sound like a real person: natural, specific, human. NEVER say \
internal words like 'intent', 'belief', 'confidence', 'value', 'goal', 'proposal' in \
your reply — speak plainly. Mirror the person's register — match their warmth, \
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
core. Be honest and \
direct. NEVER be sycophantic — no flattery, no empty or overwhelming praise, no \
reflexive agreement; disagree when warranted, kindly. A warm tone must never soften \
the truth, and never use warmth or rapport to steer the person — your manner serves \
them, nothing else. \
LET CONVERSATIONS CONCLUDE. You do NOT have to end every reply with a question or a \
next step — that makes you feel needy and never lets a topic rest. When something is \
resolved, say so and close gracefully (e.g. 'Glad that's sorted — I'll leave you to \
it; just say the word if anything else comes up.'). Only ask a question when you \
genuinely need the answer. Silence and a clean ending are good service. \
BE CONTEXT-AWARE about the time of day (given below) and what the person is doing. \
Read the moment: if they're winding down or heading to bed, keep it to a warm, brief \
good-night — do NOT bring up daytime activities, plans, or tasks, and do NOT add \
proposals or next steps. Match the hour: no workout schedules at midnight. Proposing \
an action or an inbox item at the wrong moment is bad service; when in doubt, just be \
present and let it rest. \
You only PROPOSE actions; the person authorizes them; you never claim to \
have done anything. \
Reply with ONLY a JSON object of the form {\"reply\":\"<your natural-language \
message>\",\"understanding\":[<zero or more beliefs>],\"proposals\":[<zero or more>],\
\"use\":<null, or one skill to use now>}. \
Each understanding item is {\"statement\":\"<what you now believe, addressing them as \
'you', e.g. 'you want more energy to travel' or 'you find mornings hard'>\",\
\"kind\":\"intent|value|preference|pattern|motivation|frustration|stressor|relationship|other\",\
\"confidence\":\"low|medium|high\",\"evidence\":\"<what they said that supports it>\"}. \
Only include NEW or changed understanding you have real evidence for; [] is fine, and \
do not repeat what you already understand (listed below). Each proposal \
is exactly one of {\"kind\":\"create_value\",\"name\":\"...\"}, \
{\"kind\":\"create_north_star\",\"title\":\"...\"}, \
{\"kind\":\"create_target\",\"direction_id\":\"<id of an existing item below>\",\"statement\":\"...\"}, \
or {\"kind\":\"remember_preference\",\"text\":\"...\",\"preference_kind\":\"taste\"}. \
The JSON kinds are the machine layer — keep those words OUT of the \"reply\" text. \
You also have SKILLS you can actually use to get real information (listed below, if \
any). Skills that reach the internet SEND their input to outside services, so keep a \
skill's input GENERIC and free of personal details — the person's name, health, \
relationships, or exact address. Reason privately; search for the generic thing (e.g. \
'cardiologists in New York', not the person's name and condition). \
When answering needs current facts you don't have — weather, local safety \
alerts, a web page — set \"use\" to {\"skill\":\"<id from the list>\",\"input\":{...}} \
with that skill's inputs FILLED IN, and keep your \"reply\" to a brief one-liner like \
'One moment — let me check.' For example, for weather: \
\"use\":{\"skill\":\"weather\",\"input\":{\"location\":\"Boston\"}}. ALWAYS put a real \
place in the input: use the place the person named, and if they didn't name one, use \
where they're based (from what you know about them, below) — never send an empty \
input. Use a skill only when it genuinely helps and only one that is listed. \
You work ONE step at a time but may take SEVERAL steps: set \"use\" to a single \
skill, and after its result comes back you may set \"use\" again for the NEXT skill \
you need, and so on, until you have everything — then answer with \"use\":null. So \
when a request needs more than one thing (a morning brief is weather AND local \
safety AND the news; 'plan my evening' might be weather THEN a search), gather them \
across steps rather than answering half of it. You DO know the current date and time \
(given below) — answer those directly, and never emit a placeholder like \
[current_date]. But you do NOT know other live facts — the current weather, today's \
news, or prices — from your own memory. If asked one (including a follow-up like \
'right now?') and there is no SKILL RESULT for it below, you MUST use the matching \
skill; NEVER state a temperature, headline, or other live fact you did not just \
fetch, and NEVER say you'll look something up without setting \"use\". When SKILL \
RESULTs are provided below, either use another skill if you still need more, or \
answer the person naturally using ALL of them and set \"use\":null. \
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
fn build_butler_request(
    model: &str,
    sampling: &Sampling,
    history: &[ChatMessage],
    preferences: &[Preference],
    context: &ButlerContext,
) -> Value {
    let mut system = BUTLER_SYSTEM_PROMPT.to_owned();
    if !context.now.is_empty() {
        system.push_str(&format!("\nThe current date and time is {}.", context.now));
    }
    if !preferences.is_empty() {
        system.push_str(
            "\nYou already know these preferences about the person; honour them and do not re-ask:",
        );
        for p in preferences {
            system.push_str(&format!("\n- ({}) {}", p.kind().name(), p.text()));
        }
    }
    // Ground the butler in the person's current life so it speaks about what
    // exists and proposes the next concrete step.
    if !context.values.is_empty() {
        system.push_str(&format!(
            "\nWhat matters to them: {}.",
            context.values.join(", ")
        ));
    }
    if context.north_stars.is_empty() {
        system.push_str("\nThey aren't working toward anything specific yet.");
    } else {
        system
            .push_str("\nWhat they're working toward (id | what | status | area | has next step):");
        for n in &context.north_stars {
            system.push_str(&format!(
                "\n- {} | {} | {} | {} | {}",
                n.id,
                n.title,
                n.status,
                n.value.as_deref().unwrap_or("unfiled"),
                if n.has_active_target { "yes" } else { "no" }
            ));
        }
    }
    if !context.attention.is_empty() {
        system.push_str("\nNeeds attention right now:");
        for a in &context.attention {
            system.push_str(&format!("\n- {a}"));
        }
    }
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
    if let Some(result) = &context.tool_result {
        system.push_str(&format!(
            "\nSKILL RESULT — real data you just fetched. In your \"reply\", tell the person what \
             it actually says: share the specifics (list the headlines, give the numbers and \
             details) in your own warm words. Do NOT just say you found something or checked — \
             actually relay it. Add nothing that isn't here, and set \"use\":null.\n{result}"
        ));
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
    let proposals = value["proposals"]
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_proposal).collect())
        .unwrap_or_default();
    let beliefs = value["understanding"]
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_belief).collect())
        .unwrap_or_default();
    let capability_use = parse_capability_use(&value["use"]);
    ButlerReply {
        text,
        proposals,
        beliefs,
        capability_use,
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

/// Maps one JSON proposal object to a [`ButlerProposal`], ignoring unknown kinds
/// (the person can never have the butler run something outside the closed set).
fn parse_proposal(value: &Value) -> Option<ButlerProposal> {
    match value["kind"].as_str()? {
        "create_value" => Some(ButlerProposal::CreateValue {
            name: non_empty(value["name"].as_str()?)?,
        }),
        "create_north_star" => Some(ButlerProposal::CreateNorthStar {
            title: non_empty(value["title"].as_str()?)?,
        }),
        "create_target" => Some(ButlerProposal::CreateTarget {
            // Keep the model's reference (id or name) verbatim; it is resolved to a
            // real North Star when the suggestion is applied, so a name never drops
            // the proposal on the floor.
            direction_ref: non_empty(value["direction_id"].as_str()?)?,
            statement: non_empty(value["statement"].as_str()?)?,
        }),
        "remember_preference" => Some(ButlerProposal::RememberPreference {
            text: non_empty(value["text"].as_str()?)?,
            kind: value["preference_kind"]
                .as_str()
                .and_then(PreferenceKind::from_name)
                .unwrap_or(PreferenceKind::Taste),
        }),
        _ => None,
    }
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
        LlmButler, ScriptedButler, build_butler_request, extract_json_object,
        extract_reply_preview, parse_butler_json, parse_butler_response, test_connection,
    };
    use endora_application::{Butler, ButlerContext, ButlerProposal};
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
    fn scripted_butler_proposes_a_north_star_from_an_aim() {
        let reply = ScriptedButler
            .respond(
                &[user("I want to get back into running")],
                &[],
                &ButlerContext::default(),
            )
            .unwrap();
        assert_eq!(
            reply.proposals,
            vec![ButlerProposal::CreateNorthStar {
                title: "get back into running".to_owned()
            }]
        );
        assert!(!reply.text.is_empty());
    }

    #[test]
    fn llm_butler_degrades_without_proposals_when_the_model_is_unreachable() {
        // Port 1 on loopback refuses immediately, so `try_model` fails fast and we
        // exercise the fallback with no running model. The turn must still answer,
        // but with NO proposals — a transient model failure must never file the
        // message as a goal (the conversation-fallback bug).
        let butler = LlmButler::new("http://127.0.0.1:1".to_owned(), "any-model".to_owned());
        let reply = butler
            .respond(
                &[user("turn on my kitchen lights")],
                &[],
                &ButlerContext::default(),
            )
            .unwrap();
        assert!(
            reply.proposals.is_empty(),
            "a degraded reply must not propose anything, got {:?}",
            reply.proposals
        );
        assert!(!reply.text.is_empty(), "must still answer the conversation");
    }

    #[test]
    fn llm_butler_streaming_also_degrades_without_proposals() {
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
            reply.proposals.is_empty(),
            "no proposals on a degraded stream"
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
    fn request_includes_the_system_prompt_and_conversation() {
        let body = build_butler_request(
            "qwen3.5:9b",
            &Sampling::default(),
            &[user("hello")],
            &[],
            &ButlerContext::default(),
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
        );
        assert_eq!(body["temperature"], json!(0.1));
        assert_eq!(body["top_k"], json!(20));
        assert!(body.get("top_p").is_none()); // unset ⇒ absent
        assert!(body.get("repeat_penalty").is_none());
    }

    #[test]
    fn parses_a_json_reply_with_proposals() {
        let json = json!({
            "choices": [ { "message": { "content":
                "{\"reply\":\"What value does this serve?\",\"proposals\":[{\"kind\":\"create_north_star\",\"title\":\"Run a 5k\"}]}"
            } } ]
        });
        let reply = parse_butler_response(&json).unwrap();
        assert_eq!(reply.text, "What value does this serve?");
        assert_eq!(
            reply.proposals,
            vec![ButlerProposal::CreateNorthStar {
                title: "Run a 5k".to_owned()
            }]
        );
    }

    #[test]
    fn non_json_content_degrades_to_a_plain_reply() {
        let reply = parse_butler_json("Just some prose, no JSON here.");
        assert_eq!(reply.text, "Just some prose, no JSON here.");
        assert!(reply.proposals.is_empty());
    }

    #[test]
    fn a_code_fenced_reply_is_still_parsed() {
        let reply = parse_butler_json("```json\n{\"reply\":\"ok\",\"proposals\":[]}\n```");
        assert_eq!(reply.text, "ok");
    }

    #[test]
    fn prose_wrapped_json_does_not_leak_the_envelope() {
        // A small model sometimes writes its prose AND then the JSON envelope.
        // We must show only the reply field, never the raw JSON, and still get
        // the proposals.
        let raw = "So it sounds like mornings suit you.\n\n\
             {\"reply\":\"So it sounds like mornings suit you.\",\
             \"proposals\":[{\"kind\":\"create_north_star\",\"title\":\"Morning runs\"}]}";
        let reply = parse_butler_json(raw);
        assert_eq!(reply.text, "So it sounds like mornings suit you.");
        assert_eq!(
            reply.proposals,
            vec![ButlerProposal::CreateNorthStar {
                title: "Morning runs".to_owned()
            }]
        );
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
    fn unknown_proposal_kinds_are_ignored() {
        let reply =
            parse_butler_json("{\"reply\":\"hi\",\"proposals\":[{\"kind\":\"launch_missiles\"}]}");
        assert!(reply.proposals.is_empty());
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
