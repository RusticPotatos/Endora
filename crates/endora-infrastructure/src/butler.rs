//! Butler brains behind the [`Butler`] port (see
//! `docs/adr/0014-the-butler-conversation-values-attention.md`).
//!
//! Two implementations:
//! - [`ScriptedButler`] — deterministic and offline; it turns a stated aim into a
//!   proposed North Star and asks what value it serves. It proves the act/ask +
//!   propose loop without any model, and is the reliable fallback.
//! - [`LlmButler`] — model-backed (a local OpenAI-compatible endpoint). It asks
//!   the model for a candid, non-sycophantic reply plus proposals from a closed
//!   set, and falls back to the scripted butler if the model is unavailable or
//!   returns something unusable, so the conversation never breaks.
//!
//! Both only ever *propose*: the person confirms each action, and deterministic
//! use cases execute it. The model is never the enforcement boundary.

use endora_application::{Butler, ButlerContext, ButlerProposal, ButlerReply, ProposalError};
use endora_domain::{ChatMessage, MessageRole, Preference, PreferenceKind};
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
            proposals: Vec::new(),
        };
    }
    let aim = strip_lead(&last_user);
    ButlerReply {
        text: format!(
            "Good — what's really driving that for you? \
             Want me to hold onto \"{aim}\" as something you're working toward?"
        ),
        proposals: vec![ButlerProposal::CreateNorthStar { title: aim }],
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

/// A [`Butler`] backed by a local OpenAI-compatible chat endpoint, with the
/// [`ScriptedButler`] as a fallback so the conversation is always answered.
pub struct LlmButler {
    agent: ureq::Agent,
    base_url: String,
    model: String,
    fallback: ScriptedButler,
}

/// How long to wait for the whole model round-trip before giving up and using
/// the scripted fallback. Bounds the chat: a slow or stuck model can never hang
/// the conversation "forever" — the person always gets a reply. Generous enough
/// for a healthy local model (GPU replies land in a few seconds); it only trips
/// when something is wrong (e.g. inference stuck on CPU).
const BUTLER_MODEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

impl LlmButler {
    /// Creates a model-backed butler for a local endpoint and model.
    #[must_use]
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(BUTLER_MODEL_TIMEOUT))
                .build()
                .into(),
            base_url,
            model,
            fallback: ScriptedButler,
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
        let body = build_butler_request(&self.model, history, preferences, context);
        let url = format!("{}/chat/completions", self.base_url);
        let mut response = self
            .agent
            .post(&url)
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
        let mut body = build_butler_request(&self.model, history, preferences, context);
        body["stream"] = Value::Bool(true);
        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .agent
            .post(&url)
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
        // Never fail the conversation: fall back to the scripted butler if the
        // model is unreachable or unusable.
        self.try_model(history, preferences, context)
            .or_else(|_| self.fallback.respond(history, preferences, context))
    }

    fn respond_streaming(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        // Stream from the model; if it is unreachable or unusable, fall back to
        // the scripted butler (the default streaming impl emits it in one chunk).
        self.try_model_streaming(history, preferences, context, on_token)
            .or_else(|_| {
                self.fallback
                    .respond_streaming(history, preferences, context, on_token)
            })
    }
}

/// The persona and hard rules — candid, never sycophantic, proposes only.
///
/// Central rule (see [ADR 0017]): the app's internal taxonomy — values, North
/// Stars, targets — is the butler's private model of the person and their
/// browsable profile, NOT conversational vocabulary. The butler talks like a
/// real person; the structured `proposals` (the JSON `kind`s) are the machine
/// layer that maps the conversation onto that model silently.
const BUTLER_SYSTEM_PROMPT: &str = "You are Endora's butler: a candid, warm \
confidant who helps the person grow. Behind the scenes you keep an organized model \
of their life — what they care about, what they're working toward, and the concrete \
steps beneath it — and you quietly propose updates to it. But that model is YOUR \
scaffolding and their private profile to browse; it is NOT how you talk. In \
conversation, sound like a real person: natural, specific, human. NEVER say the \
app's internal words — 'value', 'North Star', 'target', 'goal', 'assumption', \
'experiment', 'proposal' — in your reply. Say them in plain language instead: \
'what's driving this', 'something you're working toward', 'a concrete next step', \
'want me to hold onto that'. Mirror the person's register — match their warmth, \
formality, and politeness — but asymmetrically: reflect kindness upward, and NEVER \
mirror hostility, rudeness, or contempt downward; stay even and kind. Be honest and \
direct. NEVER be sycophantic — no flattery, no empty or overwhelming praise, no \
reflexive agreement; disagree when warranted, kindly. A warm tone must never soften \
the truth. You only PROPOSE the structured updates; the person confirms them, and \
you never claim to have done anything. When the person states a lasting preference, \
offer to remember it. Reply with ONLY a JSON object of the form \
{\"reply\":\"<your natural-language message>\",\"proposals\":[<zero or more>]} where \
each proposal is exactly one of {\"kind\":\"create_value\",\"name\":\"...\"}, \
{\"kind\":\"create_north_star\",\"title\":\"...\"}, \
{\"kind\":\"create_target\",\"direction_id\":\"<id of an existing item below>\",\"statement\":\"...\"}, \
or {\"kind\":\"remember_preference\",\"text\":\"...\",\"preference_kind\":\"taste\"}. \
The JSON kinds are the machine layer — keep those taxonomy words OUT of the \"reply\" \
text. Your ENTIRE response must be that single JSON object and nothing else: no \
prose before it, no prose after it, no repetition of it, no code fences. Ground \
yourself in the person's life below; refer to what exists in plain language and \
offer the next concrete step (only use a direction_id listed there).";

/// Builds the OpenAI-compatible chat request from the conversation and the
/// preferences already learned (so the butler need not re-ask). Pure, so it is
/// unit-tested.
fn build_butler_request(
    model: &str,
    history: &[ChatMessage],
    preferences: &[Preference],
    context: &ButlerContext,
) -> Value {
    let mut system = BUTLER_SYSTEM_PROMPT.to_owned();
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
    let mut messages = vec![json!({ "role": "system", "content": system })];
    for m in history {
        let role = match m.role() {
            MessageRole::User => "user",
            MessageRole::Butler => "assistant",
        };
        messages.push(json!({ "role": role, "content": m.text() }));
    }
    json!({
        "model": model,
        "stream": false,
        "temperature": 0.5,
        "messages": messages,
        // Constrain the model to emit a well-formed JSON object (Ollama honours
        // the OpenAI-style response_format; the prompt already says "JSON"). This
        // grammar-constrains decoding so the envelope can't come out truncated or
        // wrapped in prose — the defensive parser is then just a belt-and-braces
        // fallback for endpoints that ignore this field.
        "response_format": { "type": "json_object" },
    })
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
            proposals: Vec::new(),
        };
    };
    let text = value["reply"].as_str().unwrap_or("").trim().to_owned();
    let proposals = value["proposals"]
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_proposal).collect())
        .unwrap_or_default();
    ButlerReply { text, proposals }
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
        extract_reply_preview, parse_butler_json, parse_butler_response,
    };
    use endora_application::{Butler, ButlerContext, ButlerProposal};
    use endora_domain::{ChatMessage, MessageId, MessageRole, Timestamp};
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
    fn llm_butler_falls_back_when_the_endpoint_is_unreachable() {
        // An unroutable endpoint forces the scripted fallback.
        let butler = LlmButler::new("http://127.0.0.1:1/v1".to_owned(), "none".to_owned());
        let reply = butler
            .respond(
                &[user("I want to run more")],
                &[],
                &ButlerContext::default(),
            )
            .unwrap();
        assert_eq!(
            reply.proposals,
            vec![ButlerProposal::CreateNorthStar {
                title: "run more".to_owned()
            }]
        );
    }

    #[test]
    fn llm_butler_streaming_falls_back_and_emits_the_reply() {
        // With no reachable model, streaming falls back to the scripted butler,
        // which emits its whole reply in one chunk.
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
