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
    /// Writing the prompt cache (the one-hour price: the dearer, so a guess errs safe).
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
    if !crate::is_private_endpoint(base) {
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

/// The price of a model, from Anthropic's pricing page (October 2026), for the models the
/// team may use; dated ids (`claude-sonnet-5-5-20261001`) price as their family. `prompt` is
/// the call's input tokens, for the one model whose price depends on it.
#[must_use]
pub fn price_of(model: &str, prompt: u64) -> Option<Price> {
    let is = |family: &str| model == family || model.starts_with(&format!("{family}-"));
    if is("claude-sonnet-5-5") {
        return Some(Price {
            input: 2.0,
            cache_write: 4.0,
            cache_read: 0.10,
            output: 10.0,
        });
    }
    if is("claude-opus-5-5") {
        return Some(Price {
            input: 4.0,
            cache_write: 8.0,
            cache_read: 0.20,
            output: 20.0,
        });
    }
    if is("claude-haiku-5-5") {
        if prompt > HAIKU_LONG_PROMPT {
            return Some(Price {
                input: 0.50,
                cache_write: 1.0,
                cache_read: 0.05,
                output: 2.50,
            });
        }
        return Some(Price {
            input: 0.10,
            cache_write: 0.20,
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
/// `message_start`, the output (a running total) in each `message_delta`.
#[derive(Debug, Default)]
pub struct StreamedUsage {
    pending: String,
    used: Used,
}

impl StreamedUsage {
    /// Takes the next piece of the stream, whatever its size.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.push_str(&String::from_utf8_lossy(bytes));
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
                    Some("message_start") => {
                        let started = Used::from_usage(&v["message"]["usage"]);
                        self.used = Used {
                            output: self.used.output.max(started.output),
                            ..started
                        };
                    }
                    Some("message_delta") => {
                        if let Some(out) = v["usage"]["output_tokens"].as_u64() {
                            self.used.output = self.used.output.max(out);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// What the stream has reported so far.
    #[must_use]
    pub const fn used(&self) -> Used {
        self.used
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            14_000,
            "100k read at 10¢, 1k written at $4"
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
    }
}
